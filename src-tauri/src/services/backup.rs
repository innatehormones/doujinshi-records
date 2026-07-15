//! `data.db` 的本地备份。
//!
//! 走 SQLite `VACUUM INTO` + temp+rename 保证原子写；BLAKE3 dedup 避免无变化备份；
//! 还原通过 `.restore-pending.json` 标记 + 启动期 apply 解耦 UI 与文件操作。

use serde::{Deserialize, Serialize};

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