//! `BackupService` 核心逻辑。
//!
//! 设计原则：
//! - **解耦**：通过 `SettingsHandle` trait 抽象配置读写（service 层不直连 SeaORM）
//! - **安全**：`inflight: tokio::sync::Mutex<()>` 串行化所有 `backup_now` 调用
//! - **高效**：BLAKE3 dedup + VACUUM INTO + 无后台定时器
//!
//! 单实例通过 `Arc<BackupService>` 在 `AppState` 里持有；commands 层只做薄壳调用。

use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::Mutex;

use super::storage::{BackupStorage, LocalFsStorage};
use super::BackupConfig;

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
#[allow(dead_code)] // 字段在后续 task（backup_now / stage_restore / apply_pending_restore）使用
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
}