//! `data.db` 的本地备份。
//!
//! 走 SQLite `VACUUM INTO` + temp+rename 保证原子写；BLAKE3 dedup 避免无变化备份；
//! 还原通过 `.restore-pending.json` 标记 + 启动期 apply 解耦 UI 与文件操作。

use serde::{Deserialize, Serialize};
use std::path::Path;

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

/// 生成备份文件名 `data-{RFC3339 紧凑}.db`。紧凑版去冒号，保证文件名安全。
/// 始终用 UTC：跨时区机器还原时文件名一致，避免命名混乱。
pub fn backup_filename(ts: chrono::DateTime<chrono::Utc>) -> String {
    format!("data-{}.db", ts.format("%Y-%m-%dT%H-%M-%SZ"))
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