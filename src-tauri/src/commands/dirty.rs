use crate::db::entities::dirty_data;
use crate::error::AppResult;
use crate::models::Page;
use crate::AppState;
use sea_orm::{EntityTrait, PaginatorTrait, QuerySelect};
use tauri::State;

#[tauri::command]
pub async fn list_dirty(
    state: State<'_, AppState>,
    limit: Option<u64>,
    offset: Option<u64>,
) -> AppResult<Page<dirty_data::Model>> {
    let limit = limit.unwrap_or(50);
    let offset = offset.unwrap_or(0);
    let q = dirty_data::Entity::find();
    let total = q.clone().count(&state.conn).await?;
    let items = q
        .offset(offset)
        .limit(limit)
        .all(&state.conn)
        .await?;
    Ok(Page { items, total })
}