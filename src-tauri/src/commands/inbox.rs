use crate::db::entities::{conflict, doujinshi_file};
use crate::error::AppResult;
use crate::AppState;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};
use serde::Serialize;
use tauri::State;

#[derive(Debug, Serialize)]
pub struct ConflictItem {
    pub id: i64,
    pub a_file_id: i64,
    pub a_title: String,
    pub b_filename: String,
    pub b_file_path: String,
    pub created_at: String,
}

#[tauri::command]
pub async fn list_conflicts(state: State<'_, AppState>) -> AppResult<Vec<ConflictItem>> {
    let rows = conflict::Entity::find()
        .filter(conflict::Column::Resolved.eq(false))
        .all(&state.conn)
        .await?;
    let mut out = Vec::with_capacity(rows.len());
    for c in rows {
        let a = doujinshi_file::Entity::find_by_id(c.a_file_id)
            .one(&state.conn)
            .await?;
        let a_title = a.map(|m| m.title).unwrap_or_default();
        out.push(ConflictItem {
            id: c.id,
            a_file_id: c.a_file_id,
            a_title,
            b_filename: c.b_filename,
            b_file_path: c.b_file_path,
            created_at: c.created_at.to_rfc3339(),
        });
    }
    Ok(out)
}

#[tauri::command]
pub async fn resolve_conflict(state: State<'_, AppState>, id: i64) -> AppResult<()> {
    let row = conflict::Entity::find_by_id(id)
        .one(&state.conn)
        .await?
        .ok_or_else(|| crate::error::AppError::Other("conflict not found".into()))?;
    let mut am: conflict::ActiveModel = row.into();
    am.resolved = Set(true);
    am.update(&state.conn).await?;
    Ok(())
}

