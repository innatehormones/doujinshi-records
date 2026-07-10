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
    let present = doujinshi_file::Entity::find()
        .filter(
            doujinshi_file::Column::CurrentLocation
                .eq("will_delete")
                .and(doujinshi_file::Column::PhysicallyDeleted.eq(false)),
        )
        .all(&state.conn)
        .await?;
    let gone = doujinshi_file::Entity::find()
        .filter(
            doujinshi_file::Column::CurrentLocation
                .eq("will_delete")
                .and(doujinshi_file::Column::PhysicallyDeleted.eq(true)),
        )
        .all(&state.conn)
        .await?;
    Ok((
        present.iter().map(file_summary::from_model).collect(),
        gone.iter().map(file_summary::from_model).collect(),
    ))
}

#[tauri::command]
pub async fn permanent_delete(state: State<'_, AppState>, id: i64) -> AppResult<()> {
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
    am.updated_at = Set(chrono::Utc::now());
    am.update(&state.conn).await?;
    crate::services::identifier::record_event(&state.conn, id, "restore_from_recycle", None)
        .await
        .map_err(|e| AppError::Other(e.to_string()))?;
    Ok(())
}

