use crate::db::entities::{conflict, doujinshi_file};
use crate::error::AppResult;
use crate::AppState;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};
use serde::{Deserialize, Serialize};
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

/// What the user decided to do with a name+ext collision. Mapped 1:1
/// to the four buttons on the ConflictView page.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictAction {
    /// Keep A unchanged. Delete the B file from the inbox.
    KeepA,
    /// Drop A's zip; let B become the new identified row.
    ReplaceB,
    /// Keep both: B enters the library with a " (copy)" suffix on
    /// the filename so the name+ext collision check won't trip again.
    KeepBoth,
    /// Leave B in the inbox untouched. Original `resolve_conflict`
    /// behaviour.
    Skip,
}

/// Backwards-compatible shim: old callers passing only `id` keep
/// working — treated as `Skip`. New callers should pass `action`.
#[tauri::command]
pub async fn resolve_conflict(
    state: State<'_, AppState>,
    id: i64,
    action: Option<ConflictAction>,
) -> AppResult<()> {
    let action = action.unwrap_or(ConflictAction::Skip);
    let row = conflict::Entity::find_by_id(id)
        .one(&state.conn)
        .await?
        .ok_or_else(|| crate::error::AppError::Other("conflict not found".into()))?;

    match action {
        ConflictAction::Skip => {
            // Original behaviour: just mark resolved; B stays in inbox.
        }
        ConflictAction::KeepA => {
            // Delete the inbox file; A keeps its row and zip.
            let p = std::path::Path::new(&row.b_file_path);
            if p.exists() {
                let _ = std::fs::remove_file(p);
            }
        }
        ConflictAction::ReplaceB => {
            // Delete A's zip on disk, then push B through the normal
            // identifier pipeline. A's row stays (mark physically
            // deleted so the filename_alias path doesn't claim the
            // orphan hash).
            let a_row = doujinshi_file::Entity::find_by_id(row.a_file_id)
                .one(&state.conn)
                .await?;
            if let Some(a) = a_row {
                let a_path = std::path::Path::new(&a.current_path);
                if a_path.exists() {
                    let _ = std::fs::remove_file(a_path);
                }
            }
            let b_path = std::path::PathBuf::from(&row.b_file_path);
            if b_path.exists() {
                let _ = crate::services::identifier::identify_file(
                    &state.conn,
                    &b_path,
                    &state.config.covers_dir(),
                    &state.config.identified_dir(),
                    None,
                )
                .await;
            }
        }
        ConflictAction::KeepBoth => {
            // Same as ReplaceB but with a " (copy)" suffix so the
            // filename no longer collides with A.
            let b_path = std::path::PathBuf::from(&row.b_file_path);
            if b_path.exists() {
                let _ = crate::services::identifier::identify_file(
                    &state.conn,
                    &b_path,
                    &state.config.covers_dir(),
                    &state.config.identified_dir(),
                    Some("(copy)"),
                )
                .await;
            }
        }
    }

    // All four actions mark the conflict as resolved. ReplaceB /
    // KeepBoth also leave a fresh `doujinshi_file` row in place,
    // but the user has acknowledged the conflict so we don't keep
    // it nagging in the inbox.
    let mut am: conflict::ActiveModel = row.into();
    am.resolved = Set(true);
    am.update(&state.conn).await?;
    Ok(())
}