use crate::db::entities::doujinshi_file;
use crate::error::{AppError, AppResult};
use crate::models::file_summary;
use crate::AppState;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, QueryOrder, QuerySelect, Set};
use tauri::State;

#[tauri::command]
pub async fn list_library(
    state: State<'_, AppState>,
    q: Option<String>,
    status: Option<String>,
    limit: Option<u64>,
    offset: Option<u64>,
) -> AppResult<Vec<file_summary::FileSummary>> {
    let conn = &state.conn;
    let mut query = doujinshi_file::Entity::find()
        .filter(doujinshi_file::Column::CurrentLocation.eq("identified"));
    if let Some(s) = status.as_deref() {
        query = match s {
            "viewed" => query.filter(doujinshi_file::Column::Viewed.eq(true)),
            "not_viewed" => query.filter(doujinshi_file::Column::Viewed.eq(false)),
            "marked" => query.filter(doujinshi_file::Column::MarkedForDelete.eq(true)),
            _ => query,
        };
    }
    if let Some(qs) = q.as_deref().filter(|s| !s.is_empty()) {
        let pattern = format!("%{}%", qs);
        query = query.filter(
            doujinshi_file::Column::Title
                .like(&pattern)
                .or(doujinshi_file::Column::Circle.like(&pattern))
                .or(doujinshi_file::Column::Filename.like(&pattern)),
        );
    }
    let rows = query
        .order_by_desc(doujinshi_file::Column::CreatedAt)
        .limit(limit.unwrap_or(50))
    .offset(offset.unwrap_or(0))
        .all(conn)
        .await?;
    let mut out = Vec::with_capacity(rows.len());
    for m in rows {
        out.push(file_summary::from_model(&m));
    }
    Ok(out)
}

#[tauri::command]
pub async fn mark_viewed(state: State<'_, AppState>, id: i64) -> AppResult<()> {
    set_flag(&state, id, |m| { m.viewed = Set(true); }).await
}

#[tauri::command]
pub async fn unmark_viewed(state: State<'_, AppState>, id: i64) -> AppResult<()> {
    set_flag(&state, id, |m| { m.viewed = Set(false); }).await
}

#[tauri::command]
pub async fn mark_for_delete(state: State<'_, AppState>, id: i64) -> AppResult<()> {
    set_flag(&state, id, |m| { m.marked_for_delete = Set(true); }).await
}

#[tauri::command]
pub async fn unmark_for_delete(state: State<'_, AppState>, id: i64) -> AppResult<()> {
    set_flag(&state, id, |m| { m.marked_for_delete = Set(false); }).await
}

#[tauri::command]
pub async fn move_to_will_delete(state: State<'_, AppState>, id: i64) -> AppResult<()> {
    use sea_orm::EntityTrait;
    let row = doujinshi_file::Entity::find_by_id(id)
        .one(&state.conn)
        .await?
        .ok_or_else(|| AppError::Other(format!("file {} not found", id)))?;
    let src = std::path::PathBuf::from(&row.current_path);
    let dst_dir = state.config.will_delete_dir();
    std::fs::create_dir_all(&dst_dir)?;
    let dst = dst_dir.join(src.file_name().unwrap_or_default());
    if src.exists() {
        // std::fs::rename fails with CrossesDevices when src and dst are
        // on different volumes (e.g. inbox on D:, library on E:). Fall
        // back to copy + remove to keep the delete flow working in that
        // case - the spec calls this out under "Known Build-Time Risks".
        if let Err(e) = std::fs::rename(&src, &dst) {
            if matches!(e.kind(), std::io::ErrorKind::CrossesDevices)
                || e.raw_os_error() == Some(17) // ERROR_NOT_SAME_DEVICE on Windows
            {
                std::fs::copy(&src, &dst)?;
                std::fs::remove_file(&src)?;
            } else {
                return Err(e.into());
            }
        }
    }
    let mut am: doujinshi_file::ActiveModel = row.into();
    am.current_path = Set(dst.to_string_lossy().into_owned());
    am.current_location = Set("will_delete".into());
    am.marked_for_delete = Set(false);
    am.updated_at = Set(chrono::Utc::now());
    am.update(&state.conn).await?;
    Ok(())
}

async fn set_flag<F>(state: &AppState, id: i64, mut apply: F) -> AppResult<()>
where
    F: FnMut(&mut doujinshi_file::ActiveModel),
{
    use sea_orm::EntityTrait;
    let row = doujinshi_file::Entity::find_by_id(id)
        .one(&state.conn)
        .await?
        .ok_or_else(|| AppError::Other(format!("file {} not found", id)))?;
    let mut am: doujinshi_file::ActiveModel = row.into();
    apply(&mut am);
    am.updated_at = Set(chrono::Utc::now());
    am.update(&state.conn).await?;
    Ok(())
}
