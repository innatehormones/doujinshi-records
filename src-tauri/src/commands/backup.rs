//! Tauri commands 包 BackupService。
//!
//! 设计：commands 是「薄壳」——只做参数解析、错误转 AppError；
//! 核心逻辑都在 `services::backup` 里。tests 用 services 层覆盖，commands 层
//! 基本是 type bridge 不另测。

use crate::error::AppResult;
use crate::services::backup::{BackupConfig, BackupResult, BackupService, SnapshotInfo};
use std::path::PathBuf;
use std::sync::Arc;
use tauri::State;

/// 共享 BackupService 句柄。在 AppState 里以 `Arc<BackupService>` 持有。
pub type SharedBackupService = Arc<BackupService>;

#[tauri::command]
pub async fn get_backup_config(
    backup: State<'_, SharedBackupService>,
) -> AppResult<BackupConfig> {
    Ok(backup.get_config().await?)
}

#[tauri::command]
pub async fn set_backup_config(
    backup: State<'_, SharedBackupService>,
    dir: Option<String>,
    retention_count: u32,
) -> AppResult<()> {
    Ok(backup.set_config(dir.as_deref(), retention_count).await?)
}

#[tauri::command]
pub async fn list_backups(
    backup: State<'_, SharedBackupService>,
) -> AppResult<Vec<SnapshotInfo>> {
    Ok(backup.list_backups().await?)
}

#[tauri::command]
pub async fn backup_now(
    backup: State<'_, SharedBackupService>,
) -> AppResult<BackupResult> {
    Ok(backup.backup_now().await?)
}

#[tauri::command]
pub async fn stage_restore(
    backup: State<'_, SharedBackupService>,
    src: String,
) -> AppResult<()> {
    Ok(backup.stage_restore(&PathBuf::from(src)).await?)
}

#[tauri::command]
pub async fn delete_backup(
    backup: State<'_, SharedBackupService>,
    snapshot: String,
) -> AppResult<()> {
    backup.delete_snapshot(&PathBuf::from(snapshot)).await?;
    Ok(())
}
