use crate::db::entities::doujinshi_file;
use crate::error::{AppError, AppResult};
use crate::models::file_summary;
use crate::AppState;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};
use tauri::State;

#[tauri::command]
pub async fn list_recycle(
    state: State<'_, AppState>,
) -> AppResult<(Vec<file_summary::FileSummary>, Vec<file_summary::FileSummary>)> {
    // V3: present/gone 改按 has_physical_file 分组——present = 文件仍在，
    // gone = 文件已被外部清走/已被 permanent_delete 真删。FileSummary 不
    // 暴露 physically_deleted 字段所以这里走 has_physical_file 通道。
    let present = doujinshi_file::Entity::find()
        .filter(
            doujinshi_file::Column::CurrentLocation
                .eq("will_delete")
                .and(doujinshi_file::Column::HasPhysicalFile.eq(true)),
        )
        .all(&state.conn)
        .await?;
    let gone = doujinshi_file::Entity::find()
        .filter(
            doujinshi_file::Column::CurrentLocation
                .eq("will_delete")
                .and(doujinshi_file::Column::HasPhysicalFile.eq(false)),
        )
        .all(&state.conn)
        .await?;
    let mut ids: Vec<i64> = present.iter().chain(gone.iter()).map(|m| m.id).collect();
    ids.sort();
    ids.dedup();
    let conflict_map = file_summary::open_conflict_map(&state.conn, &ids).await;
    let map_summaries = |rows: &[doujinshi_file::Model]| -> Vec<file_summary::FileSummary> {
        rows.iter()
            .map(|m| {
                let has = conflict_map.get(&m.id).copied().unwrap_or(false);
                file_summary::from_model_with_conflict_state(m, has)
            })
            .collect()
    };
    Ok((map_summaries(&present), map_summaries(&gone)))
}

#[tauri::command]
pub async fn permanent_delete(state: State<'_, AppState>, id: i64) -> AppResult<()> {
    crate::commands::guards::ensure_no_open_conflict(&state.conn, id).await?;
    let file = doujinshi_file::Entity::find_by_id(id)
        .one(&state.conn)
        .await?
        .ok_or(AppError::NotFound)?;
    if !file.physically_deleted {
        let path = std::path::PathBuf::from(&file.current_path);
        if path.exists() {
            std::fs::remove_file(&path)?;
        }
    }
    let mut am: doujinshi_file::ActiveModel = file.into();
    am.physically_deleted = Set(true);
    am.has_physical_file = Set(false);
    am.updated_at = Set(chrono::Utc::now());
    am.update(&state.conn).await?;
    crate::services::identifier::record_event(&state.conn, id, "deleted", None)
        .await
        .map_err(|e| AppError::Other(e.to_string()))?;
    Ok(())
}

#[tauri::command]
pub async fn restore_from_recycle(state: State<'_, AppState>, id: i64) -> AppResult<()> {
    let file = doujinshi_file::Entity::find_by_id(id)
        .one(&state.conn)
        .await?
        .ok_or(AppError::NotFound)?;
    let current = std::path::PathBuf::from(&file.current_path);
    let filename = current
        .file_name()
        .ok_or_else(|| AppError::Other("invalid path".into()))?
        .to_owned();
    let target = state.config.identified_dir().join(&filename);
    std::fs::create_dir_all(state.config.identified_dir())?;
    if target.exists() {
        return Err(AppError::Other("target already exists".into()));
    }
    std::fs::rename(&current, &target)?;
    let mut am: doujinshi_file::ActiveModel = file.into();
    am.current_path = Set(target.to_string_lossy().into_owned());
    am.current_location = Set("identified".into());
    am.marked_for_delete = Set(false);
    am.has_physical_file = Set(true);
    am.updated_at = Set(chrono::Utc::now());
    am.update(&state.conn).await?;
    crate::services::identifier::record_event(&state.conn, id, "restore_from_recycle", None)
        .await
        .map_err(|e| AppError::Other(e.to_string()))?;
    Ok(())
}

