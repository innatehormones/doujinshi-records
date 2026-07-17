use crate::db::entities::{conflict, doujinshi_file};
use crate::error::AppResult;
use crate::models::Page;
use crate::AppState;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, PaginatorTrait, QueryFilter,
    QuerySelect, Set,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::State;

#[derive(Debug, Serialize)]
pub struct ConflictItem {
    pub id: i64,
    pub a_file_id: i64,
    pub a_title: String,
    /// A 端封面 URL（`/api/covers/<hash>`）。`None` 表示 A 行已被删 /
    /// 找不到对应 doujinshi 行，或 hash 字段为空。B 端没入库，永远没封面。
    /// 前端 InboxView「显示封面」开关打开时用这个。
    pub a_cover_url: Option<String>,
    pub b_filename: String,
    pub b_file_path: String,
    pub created_at: String,
}

#[tauri::command]
pub async fn list_conflicts(
    state: State<'_, AppState>,
    limit: Option<u64>,
    offset: Option<u64>,
) -> AppResult<Page<ConflictItem>> {
    list_conflicts_inner(&state.conn, limit, offset).await
}

/// Pure DB-side list, reachable from integration tests without
/// pulling in `tauri::AppState` (which drags the GUI subsystem into
/// the test binary — STATUS_ENTRYPOINT_NOT_FOUND on Windows).
pub async fn list_conflicts_inner(
    conn: &DatabaseConnection,
    limit: Option<u64>,
    offset: Option<u64>,
) -> AppResult<Page<ConflictItem>> {
    let limit = limit.unwrap_or(50);
    let offset = offset.unwrap_or(0);
    let q = conflict::Entity::find().filter(conflict::Column::Resolved.eq(false));
    let total = q.clone().count(conn).await?;
    let rows = q.offset(offset).limit(limit).all(conn).await?;
    let mut items = Vec::with_capacity(rows.len());
    for c in rows {
        let a = doujinshi_file::Entity::find_by_id(c.a_file_id).one(conn).await?;
        let (a_title, a_cover_url) = match a {
            Some(m) => {
                let url = if m.hash.is_empty() {
                    None
                } else {
                    Some(format!("/api/covers/{}", m.hash))
                };
                (m.title, url)
            }
            None => (String::new(), None),
        };
        items.push(ConflictItem {
            id: c.id,
            a_file_id: c.a_file_id,
            a_title,
            a_cover_url,
            b_filename: c.b_filename,
            b_file_path: c.b_file_path,
            created_at: c.created_at.to_rfc3339(),
        });
    }
    Ok(Page { items, total })
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
    resolve_conflict_inner(
        &state.conn,
        &state.config.covers_dir(),
        &state.config.identified_dir(),
        id,
        action.unwrap_or(ConflictAction::Skip),
    )
    .await
}

/// Inner logic for `resolve_conflict`. Takes only the bits it
/// actually needs (DB handle + a couple of paths) so it is reachable
/// from integration tests without going through `AppState` — and
/// without pulling in the `tauri` crate (and its `tao`/`wry` GUI
/// deps that the test runner can't load on Windows).
pub async fn resolve_conflict_inner(
    conn: &DatabaseConnection,
    covers_dir: &std::path::Path,
    identified_dir: &std::path::Path,
    id: i64,
    action: ConflictAction,
) -> AppResult<()> {
    let row = conflict::Entity::find_by_id(id)
        .one(conn)
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
            // V4：把 A 推进到 status='deleted'（非终态）+ file_state='absent_confirmed'：
            // A 的 zip best-effort 删掉、A 的行留在历史里——`deleted` 不参与撞名
            // 查询，后续 B 就能用 A 的 filename 正常入库，不撞名。如果用户想
            // 让 A 复活，可手动把 status 切回 in_library。
            let a_row = doujinshi_file::Entity::find_by_id(row.a_file_id)
                .one(conn)
                .await?;
            if let Some(a) = a_row {
                let a_path = std::path::Path::new(&a.last_seen_path);
                if a_path.exists() {
                    let _ = std::fs::remove_file(a_path);
                }
                let mut am: doujinshi_file::ActiveModel = a.into();
                am.status = Set("deleted".into());
                am.file_state = Set("absent_confirmed".into());
                am.updated_at = Set(chrono::Utc::now());
                let _ = am.update(conn).await;
            }
            let b_path = PathBuf::from(&row.b_file_path);
            if b_path.exists() {
                let _ = crate::services::identifier::identify_file(
                    conn,
                    &b_path,
                    covers_dir,
                    identified_dir,
                    None,
                    false,
                )
                .await;
            }
        }
        ConflictAction::KeepBoth => {
            // Same as ReplaceB but with a " (copy)" suffix so the
            // filename no longer collides with A.
            let b_path = PathBuf::from(&row.b_file_path);
            if b_path.exists() {
                let _ = crate::services::identifier::identify_file(
                    conn,
                    &b_path,
                    covers_dir,
                    identified_dir,
                    Some("(copy)"),
                    false,
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
    am.update(conn).await?;
    Ok(())
}