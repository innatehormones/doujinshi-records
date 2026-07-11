use crate::db::entities::dirty_data;
use crate::error::AppResult;
use crate::AppState;
use sea_orm::EntityTrait;
use tauri::State;

#[tauri::command]
pub async fn list_dirty(
    state: State<'_, AppState>,
) -> AppResult<Vec<dirty_data::Model>> {
    let rows = dirty_data::Entity::find().all(&state.conn).await?;
    Ok(rows)
}