//! `BackupService` 核心逻辑。
//!
//! 设计原则：
//! - **解耦**：通过 `SettingsHandle` trait 抽象配置读写（service 层不直连 SeaORM）；
//!   通过 `BackupStorage` trait 抽象存储后端（未来加 S3/WebDAV 只新加 struct）。
//! - **安全**：`inflight: tokio::sync::Mutex<()>` 串行化所有 `backup_now` 调用；
//!   temp+rename 保证写原子；启动期 magic 校验拦截非 SQLite 文件。
//! - **高效**：BLAKE3 dedup + VACUUM INTO + 无后台定时器（启动期单次检查）。
//!
//! 单实例通过 `Arc<BackupService>` 在 `AppState` 里持有；commands 层只做薄壳调用。

use sea_orm::{ConnectionTrait, Statement};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;

use super::storage::{BackupStorage, LocalFsStorage, SnapshotInfo};
use super::{backup_filename, hash_db_file, BackupConfig};

/// `BackupResult` —— `backup_now` 返回。
/// `skipped: Some(reason)` 表示本次因内容未变而跳过。
#[derive(Debug, Clone, Serialize)]
pub struct BackupResult {
    pub path: PathBuf,
    pub size_bytes: u64,
    pub md5: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skipped: Option<String>,
}

/// `SettingsHandle` 把「app_setting 读写」抽出来——service 层不依赖 SeaORM，
/// 单测可注入 `FakeSettings`（测试文件里实现）。
///
/// 用 `Pin<Box<dyn Future + Send>>` 返回类型保持 dyn 兼容（`Arc<dyn SettingsHandle>`
/// 在 `BackupService` 里持有），不引 `async-trait` 依赖。
pub trait SettingsHandle: Send + Sync {
    fn read(
        &self,
        key: &str,
    ) -> Pin<Box<dyn std::future::Future<Output = anyhow::Result<Option<String>>> + Send + '_>>;
    fn write(
        &self,
        key: &str,
        value: &str,
    ) -> Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>> + Send + '_>>;
}

/// 真正用的实现——包一层 `db::read_setting` / `db::write_setting`。
/// 持有 `Arc<DatabaseConnection>` 以满足 `Arc<dyn SettingsHandle>` 的 `'static` 要求。
pub struct DbSettingsHandle {
    pub conn: Arc<sea_orm::DatabaseConnection>,
}

impl SettingsHandle for DbSettingsHandle {
    fn read(
        &self,
        key: &str,
    ) -> Pin<Box<dyn std::future::Future<Output = anyhow::Result<Option<String>>> + Send + '_>>
    {
        let key = key.to_string();
        Box::pin(async move { Ok(crate::db::read_setting(&self.conn, &key).await?) })
    }
    fn write(
        &self,
        key: &str,
        value: &str,
    ) -> Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>> + Send + '_>> {
        let key = key.to_string();
        let value = value.to_string();
        Box::pin(async move {
            crate::db::write_setting(&self.conn, &key, &value).await?;
            Ok(())
        })
    }
}

/// 备份/还原服务核心。`db_path` + `default_dir` + `storage` (trait obj) + `settings` (trait obj)。
pub struct BackupService {
    pub(super) db_path: PathBuf,
    pub(super) default_dir: PathBuf,
    pub(super) storage: Arc<dyn BackupStorage>,
    pub(super) settings: Arc<dyn SettingsHandle>,
    /// 同一时刻只允许一个 `backup_now` 跑（防用户连点 + 自动备份并发）。
    pub(super) inflight: Mutex<()>,
}

impl BackupService {
    pub fn new(
        db_path: PathBuf,
        default_dir: PathBuf,
        storage: Arc<dyn BackupStorage>,
        settings: Arc<dyn SettingsHandle>,
    ) -> Self {
        Self { db_path, default_dir, storage, settings, inflight: Mutex::new(()) }
    }

    /// 配套工厂：用 `LocalFsStorage` + `DbSettingsHandle`。
    /// 主要给 `lib.rs::run` 和 `main.rs` 用——单测用 `FakeSettings`。
    pub fn new_with_db(
        db_path: PathBuf,
        default_dir: PathBuf,
        conn: Arc<sea_orm::DatabaseConnection>,
    ) -> Self {
        Self::new(
            db_path,
            default_dir,
            Arc::new(LocalFsStorage),
            Arc::new(DbSettingsHandle { conn }),
        )
    }

    pub fn resolve_backup_dir(&self, cfg: &BackupConfig) -> PathBuf {
        if cfg.dir.is_empty() { self.default_dir.clone() } else { PathBuf::from(&cfg.dir) }
    }

    pub async fn get_config(&self) -> anyhow::Result<BackupConfig> {
        let dir = self.settings.read("backup_dir").await?.unwrap_or_default();
        let retention_count = self
            .settings
            .read("backup_retention_count")
            .await?
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(10);
        Ok(BackupConfig { dir, retention_count })
    }

    pub async fn set_config(&self, dir: Option<&str>, retention: u32) -> anyhow::Result<()> {
        if let Some(d) = dir {
            self.settings.write("backup_dir", d).await?;
        }
        self.settings
            .write("backup_retention_count", &retention.to_string())
            .await?;
        Ok(())
    }

    pub async fn list_backups(&self) -> anyhow::Result<Vec<SnapshotInfo>> {
        let cfg = self.get_config().await?;
        let dir = self.resolve_backup_dir(&cfg);
        self.storage.list_snapshots(&dir)
    }

    /// 核心：建一份当前 DB 的快照。
    ///
    /// 流程：
    /// 1. `try_lock inflight`——并发 caller 立刻拿到 Err("already in progress")
    /// 2. 读 config + 算当前 BLAKE3
    /// 3. dedup：跟上次 MD5 一致则只 touch last_at，返回 `skipped = "content unchanged"`
    /// 4. 不一致：VACUUM INTO `.tmp-<uuid>.db` → 算 size → fs::rename 到 final path
    ///    （同 fs 下 rename 原子；跨设备场景后续 task 加 copy fallback）
    /// 5. 写 backup_last_md5 + touch backup_last_at
    /// 6. retention 清理：保留最新 N 个，其余删
    pub async fn backup_now(&self) -> anyhow::Result<BackupResult> {
        let _guard = self.inflight.try_lock().map_err(|_| {
            anyhow::anyhow!("backup already in progress")
        })?;

        // 1. 读 config + 准备目录
        let cfg = self.get_config().await?;
        let dir = self.resolve_backup_dir(&cfg);
        std::fs::create_dir_all(&dir)?;

        // 2. 当前 MD5
        let current_md5 = hash_db_file(&self.db_path)?;

        // 3. dedup：内容未变则 skip
        let last_md5 = self.settings.read("backup_last_md5").await?;
        if last_md5.as_deref() == Some(&current_md5) {
            let snapshots = self.storage.list_snapshots(&dir)?;
            let last_path = snapshots.first().map(|s| s.path.clone());
            let last_size = snapshots.first().map(|s| s.size_bytes).unwrap_or(0);
            self.touch_last_at().await?;
            return Ok(BackupResult {
                path: last_path.unwrap_or(dir.join("(none)")),
                size_bytes: last_size,
                md5: current_md5,
                skipped: Some("content unchanged".into()),
            });
        }

        // 4. VACUUM INTO tmp → rename 到 final
        let now = chrono::Utc::now();
        let final_path = dir.join(backup_filename(now));
        let tmp_path = dir.join(format!(".tmp-{}.db", Uuid::new_v4()));
        self.vacuum_into(&tmp_path).await?;
        let tmp_size = std::fs::metadata(&tmp_path)?.len();
        std::fs::rename(&tmp_path, &final_path)?;

        // 5. 算 final 的 MD5（同 tmp；rename 不改内容）+ 写 settings
        let new_md5 = hash_db_file(&final_path)?;
        self.settings.write("backup_last_md5", &new_md5).await?;
        self.touch_last_at().await?;

        // 6. retention 清理
        self.apply_retention(&dir, cfg.retention_count)?;

        Ok(BackupResult {
            path: final_path,
            size_bytes: tmp_size,
            md5: new_md5,
            skipped: None,
        })
    }

    /// 用单独打开的临时连接做 VACUUM INTO，避免阻塞常驻 app 连接。
    async fn vacuum_into(&self, dst: &Path) -> anyhow::Result<()> {
        let conn = crate::db::connect(&self.db_path).await?;
        // 单引号转义：path 里的 ' 替换成 ''（SQLite 标准做法）。
        let escaped = dst.to_string_lossy().replace('\'', "''");
        let sql = format!("VACUUM INTO '{}'", escaped);
        conn.execute(Statement::from_string(conn.get_database_backend(), sql)).await?;
        Ok(())
    }

    async fn touch_last_at(&self) -> anyhow::Result<()> {
        let now = chrono::Utc::now().to_rfc3339();
        self.settings.write("backup_last_at", &now).await?;
        Ok(())
    }

    /// `keep = 0` 表示禁用清理（保留所有）。
    pub(super) fn apply_retention(&self, dir: &Path, keep: u32) -> anyhow::Result<()> {
        if keep == 0 {
            return Ok(());
        }
        let snapshots = self.storage.list_snapshots(dir)?;
        for s in snapshots.iter().skip(keep as usize) {
            self.storage.delete_snapshot(&s.path)?;
        }
        Ok(())
    }
}