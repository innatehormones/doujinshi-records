use crate::db::entities::dirty_data;
use crate::error::{AppError, AppResult};
use crate::models::Page;
use crate::AppState;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, QuerySelect, Set};
use std::path::PathBuf;
use tauri::State;

#[tauri::command]
pub async fn list_dirty(
    state: State<'_, AppState>,
    limit: Option<u64>,
    offset: Option<u64>,
) -> AppResult<Page<dirty_data::Model>> {
    let limit = limit.unwrap_or(50);
    let offset = offset.unwrap_or(0);
    // 过滤已软删（resolved_at IS NOT NULL）的行——list 只展示活跃脏数据。
    let q = dirty_data::Entity::find()
        .filter(dirty_data::Column::ResolvedAt.is_null());
    let total = q.clone().count(&state.conn).await?;
    let items = q
        .offset(offset)
        .limit(limit)
        .all(&state.conn)
        .await?;
    Ok(Page { items, total })
}

/// 重新入库脏数据条目 —— 仅适用 `reason="orphan_file"`（目录里有文件但 DB
/// 没有对应行）。**Mover-only**：只把文件从 `detected_dir` 搬到 `inbox_dir`，
/// 写 `resolved_at` 软删这一条 dirty_data 行。
///
/// 完整的入库流程（BLAKE3 命中 reactivate / 不命中新建 / 撞名写 conflict）
/// 由已经在跑的 `scanner::Scanner` notify watcher 接管，2s 防抖后异步处理。
/// UI 因此立即返回，不阻塞 hash + 抽封面。
///
/// 错误反馈：
/// - 文件已不存在 → error 透出 + dirty 行不动
/// - inbox 已有同名 → error 拒绝（避免静默覆盖） + dirty 行不动
/// - scanner 跑失败（rar 抽封面失败、撞名等）由 `rar-error` 事件 +
///   `ConflictView` 兜底
///
/// 拆 inner 是为了避开 Tauri runtime 依赖 —— 测试可直接调 inner，无需拉
/// AppState / Scanner / wry-tao 整套 GUI stack。
pub async fn reingest_dirty_entry_inner(
    conn: &sea_orm::DatabaseConnection,
    inbox_dir: &std::path::Path,
    id: i64,
) -> AppResult<()> {
    let row = dirty_data::Entity::find_by_id(id)
        .one(conn)
        .await?
        .ok_or_else(|| AppError::Other(format!("dirty_data id={id} not found")))?;

    if row.reason != "orphan_file" {
        return Err(AppError::Other(format!(
            "only orphan_file entries can be reingested, got reason={}",
            row.reason
        )));
    }
    let path = PathBuf::from(&row.file_path);
    if !path.exists() {
        return Err(AppError::Other(format!(
            "file no longer on disk: {}",
            row.file_path
        )));
    }

    // Mover-only：把文件搬到 inbox/，由 scanner::Scanner 接管入库流程。
    std::fs::create_dir_all(inbox_dir)?;
    let file_name = path.file_name().ok_or_else(|| {
        AppError::Other(format!("invalid file path: {}", row.file_path))
    })?;
    let target = inbox_dir.join(file_name);
    if target.exists() {
        return Err(AppError::Other(format!(
            "inbox already has a file with the same name: {}",
            target.display()
        )));
    }
    if path != target {
        std::fs::rename(&path, &target)?;
    }

    let mut am: dirty_data::ActiveModel = row.into();
    am.resolved_at = Set(Some(chrono::Utc::now().to_rfc3339()));
    am.update(conn).await?;
    Ok(())
}

#[tauri::command]
pub async fn reingest_dirty_entry(
    state: State<'_, AppState>,
    id: i64,
) -> AppResult<()> {
    reingest_dirty_entry_inner(&state.conn, &state.config.inbox_dir(), id).await
}
