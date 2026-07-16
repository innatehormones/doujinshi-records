//! `data.db` 的本地备份。
//!
//! 走 SQLite `VACUUM INTO` + temp+rename 保证原子写；BLAKE3 dedup 避免无变化备份；
//! 还原通过 `.restore-pending.json` 标记 + 启动期 apply 解耦 UI 与文件操作。
//!
//! 模块拆分：
//! - `storage` — `BackupStorage` trait + `LocalFsStorage`（未来加 S3/WebDAV 只新加 struct）
//! - `service` — `BackupService` 核心逻辑（Task 6 引入）

use serde::{Deserialize, Serialize};
use std::path::Path;

pub mod service;
pub mod storage;

pub use service::{BackupService, DbSettingsHandle, SettingsHandle};
pub use storage::{BackupStorage, LocalFsStorage, SnapshotInfo};

/// 用户可见的备份配置。`dir = ""` 表示使用默认 `resources/backups/`。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BackupConfig {
    pub dir: String,
    pub retention_count: u32,
}

impl Default for BackupConfig {
    fn default() -> Self {
        Self { dir: String::new(), retention_count: 10 }
    }
}

/// 生成备份文件名 `data-{RFC3339 紧凑}.db`。紧凑版去冒号，加毫秒防同秒覆盖。
/// 始终用 UTC：跨时区机器还原时文件名一致，避免命名混乱。
pub fn backup_filename(ts: chrono::DateTime<chrono::Utc>) -> String {
    format!("data-{}.db", ts.format("%Y-%m-%dT%H-%M-%S%.3fZ"))
}

/// 整文件读入 + BLAKE3。DB 规模 KB~MB 级，全文读入 + BLAKE3（项目最熟路径）
/// 比 streaming 哈希代码量少且更快——BLAKE3 单核 ~1GB/s，MB 级文件 < 10ms。
pub fn hash_db_file(path: &Path) -> anyhow::Result<String> {
    let bytes = std::fs::read(path)?;
    Ok(blake3::hash(&bytes).to_hex().to_string())
}

/// 启动期检测：是否有用户触发的还原待执行。
/// 写在 `db_path` 同目录（`resources/.restore-pending.json`），
/// `main.rs` 在打开 DB 之前读取并应用。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RestorePending {
    pub src: String,
    pub requested_at: String,
}

pub fn write_restore_marker(path: &Path, pending: &RestorePending) -> anyhow::Result<()> {
    let json = serde_json::to_string_pretty(pending)?;
    std::fs::write(path, json)?;
    Ok(())
}

pub fn read_restore_marker(path: &Path) -> anyhow::Result<Option<RestorePending>> {
    if !path.exists() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(path)?;
    let pending: RestorePending = serde_json::from_str(&text)?;
    Ok(Some(pending))
}

pub fn clear_restore_marker(path: &Path) {
    let _ = std::fs::remove_file(path);
}

/// 校验路径是不是合法 SQLite（magic header = "SQLite format 3\0"）。
/// 启动期 apply 与 stage_restore 都用它拦截非 SQLite 文件，避免把烂字节当 DB。
pub fn validate_sqlite_file(path: &Path) -> anyhow::Result<()> {
    use std::io::Read;
    let mut f = std::fs::File::open(path)?;
    let mut header = [0u8; 16];
    f.read_exact(&mut header)?;
    if &header == b"SQLite format 3\x00" {
        Ok(())
    } else {
        Err(anyhow::anyhow!("not a valid SQLite file: {}", path.display()))
    }
}

/// 启动期检查 + 应用待执行的还原。返回 `Ok(Some(src))` 表示已替换，
/// `Ok(None)` 表示无 marker。`src` 校验失败抛 Err：marker 保留供排查，
/// db 不动。
pub async fn apply_pending_restore(
    db_path: &Path,
    marker_path: &Path,
) -> anyhow::Result<Option<String>> {
    let pending = match read_restore_marker(marker_path)? {
        Some(p) => p,
        None => return Ok(None),
    };
    let src = Path::new(&pending.src);
    validate_sqlite_file(src)?;
    std::fs::copy(src, db_path)?;
    clear_restore_marker(marker_path);
    Ok(Some(pending.src))
}

/// Backup bookkeeping（不能放 app_setting：写入会改 DB 文件，破坏 dedup 的 hash 比对）。
/// 写在 `<backup_dir>/backup_state.json`，每次 backup_now 末更新。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct BackupState {
    #[serde(default)]
    pub last_md5: String,
    #[serde(default)]
    pub last_at: String,
}

pub async fn read_backup_state(dir: &Path) -> anyhow::Result<BackupState> {
    let path = dir.join("backup_state.json");
    if !path.exists() {
        return Ok(BackupState::default());
    }
    let text = std::fs::read_to_string(&path)?;
    if text.trim().is_empty() {
        return Ok(BackupState::default());
    }
    Ok(serde_json::from_str(&text)?)
}

pub async fn write_backup_state(dir: &Path, state: &BackupState) -> anyhow::Result<()> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join("backup_state.json");
    let json = serde_json::to_string_pretty(state)?;
    // 原子写：tmp + rename，避免半截被读到
    let tmp = dir.join(".backup_state.json.tmp");
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}