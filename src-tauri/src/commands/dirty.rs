use crate::db::entities::dirty_data;
use crate::error::{AppError, AppResult};
use crate::models::Page;
use crate::services::identifier;
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
/// 没有对应行）。把该文件交给 `identifier::identify_file` 跑完整入库流程：
/// BLAKE3 命中已存在行 → reactivate；不命中 → 新 row；撞名 → 写 conflict。
/// 不论哪种 outcome，最终都打 `resolved_at` 软删这一条 dirty_data 行。
///
/// `skip_size_gate`：对应 RAR 大小豁免，跟 scanner 走同一套；UI 默认 false，
/// 大文件场景可由 `forceExtract` 路径手动开。
///
/// 拆 inner 是为了避开 Tauri runtime 依赖 —— 测试可直接调 inner，无需拉
/// AppState / Scanner / wry-tao 整套 GUI stack。
pub async fn reingest_dirty_entry_inner(
    conn: &sea_orm::DatabaseConnection,
    covers_dir: &std::path::Path,
    identified_dir: &std::path::Path,
    id: i64,
    skip_size_gate: bool,
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

    // 复用 scanner 主路径。撞 naming collision 时由 identifier 写 conflict 表
    // 走 ConflictView。IdentifierError 没派生 From 进 AppError，包装成字符串透出。
    identifier::identify_file(conn, &path, covers_dir, identified_dir, None, skip_size_gate)
        .await
        .map_err(|e| AppError::Other(e.to_string()))?;

    let mut am: dirty_data::ActiveModel = row.into();
    am.resolved_at = Set(Some(chrono::Utc::now().to_rfc3339()));
    am.update(conn).await?;
    Ok(())
}

#[tauri::command]
pub async fn reingest_dirty_entry(
    state: State<'_, AppState>,
    id: i64,
    skip_size_gate: bool,
) -> AppResult<()> {
    reingest_dirty_entry_inner(
        &state.conn,
        &state.covers_dir,
        &state.config.identified_dir(),
        id,
        skip_size_gate,
    )
    .await
}