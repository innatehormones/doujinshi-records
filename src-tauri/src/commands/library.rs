use crate::db::entities::doujinshi_file;
use crate::error::{AppError, AppResult};
use crate::models::file_summary;
use crate::AppState;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, QueryOrder, QuerySelect, Set};
use serde::Deserialize;
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

/// Partial-update body for `update_metadata`. Fields set to `Some(_)`
/// overwrite the existing value; fields left as `None` are untouched
/// (so callers can patch one field at a time).
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct MetadataPatch {
    pub title: Option<String>,
    pub circle: Option<String>,
    pub series: Option<String>,
    pub translator: Option<String>,
    pub version: Option<String>,
    pub note: Option<String>,
    pub rating: Option<i32>,
}

/// Apply a partial metadata patch to a single doujinshi row. Called
/// both by the `update_metadata` Tauri command and by the HTTP
/// `PATCH /api/doujinshi/:id` handler.
pub async fn apply_metadata_patch(
    conn: &sea_orm::DatabaseConnection,
    id: i64,
    patch: MetadataPatch,
) -> AppResult<()> {
    use sea_orm::EntityTrait;
    let row = doujinshi_file::Entity::find_by_id(id)
        .one(conn)
        .await?
        .ok_or_else(|| AppError::Other(format!("file {} not found", id)))?;
    let mut am: doujinshi_file::ActiveModel = row.into();
    if let Some(v) = patch.title       { am.title       = Set(v); }
    if let Some(v) = patch.circle      { am.circle      = Set(Some(v)); }
    if let Some(v) = patch.series      { am.series      = Set(Some(v)); }
    if let Some(v) = patch.translator  { am.translator  = Set(Some(v)); }
    if let Some(v) = patch.version     { am.version_tag = Set(Some(v)); }
    if let Some(v) = patch.note        { am.note        = Set(Some(v)); }
    if let Some(v) = patch.rating      { am.rating      = Set(Some(v)); }
    am.updated_at = Set(chrono::Utc::now());
    am.update(conn).await?;
    Ok(())
}

#[tauri::command]
pub async fn update_metadata(
    state: State<'_, AppState>,
    id: i64,
    patch: MetadataPatch,
) -> AppResult<()> {
    apply_metadata_patch(&state.conn, id, patch).await
}

#[tauri::command]
pub async fn get_by_id(
    state: State<'_, AppState>,
    id: i64,
) -> AppResult<file_summary::FileSummary> {
    use sea_orm::EntityTrait;
    let row = doujinshi_file::Entity::find_by_id(id)
        .one(&state.conn)
        .await?
        .ok_or_else(|| AppError::Other(format!("file {} not found", id)))?;
    Ok(file_summary::from_model(&row))
}

/// Re-run the identifier on a specific inbox file path with the size
/// gate skipped. Used by the frontend's "still extract large RAR"
/// confirmation — the user has acknowledged the disk risk so we let
/// the extraction proceed. Surfaces `IdentifierError` to the frontend
/// by serialising into the same payload shape used by the
/// `rar-error` event so the caller can render an error card.
#[tauri::command]
pub async fn force_extract(
    state: State<'_, AppState>,
    path: String,
) -> AppResult<()> {
    let p = std::path::PathBuf::from(&path);
    if !p.exists() {
        return Err(AppError::Other(format!("file not found: {}", path)));
    }
    let outcome = crate::services::identifier::identify_file(
        &state.conn,
        &p,
        &state.covers_dir,
        &state.config.identified_dir(),
        None,
        true, // skip_size_gate
    )
    .await
    .map_err(|e| match e.to_rar_payload() {
        Some(payload) => AppError::Other(serde_json::to_string(&payload).unwrap_or_else(|_| e.to_string())),
        None => AppError::Other(e.to_string()),
    })?;
    use crate::services::identifier::IdentifyOutcome::*;
    match outcome {
        AlreadyKnown(_) | NewIdentified(_) => Ok(()),
        Conflict { .. } => Err(AppError::Other("conflict after force extract".into())),
        Error(e) => Err(AppError::Other(e)),
    }
}

#[cfg(test)]
mod tests {
    //! Integration-style tests against a real SQLite on disk. The
    //! entity's `Default` impl + `Set(...)` ergonomics already cover
    //! the partial-update path; we just need a row to operate on.
    use super::*;
    use crate::db;
    use sea_orm::{ActiveModelTrait, Set};

    async fn seed_row(conn: &sea_orm::DatabaseConnection) -> i64 {
        let now = chrono::Utc::now();
        let am = doujinshi_file::ActiveModel {
            title: Set("旧标题".into()),
            filename: Set("m.zip".into()),
            hash: Set("h".into()),
            ext: Set("zip".into()),
            size_bytes: Set(0),
            circle: Set(Some("旧社团".into())),
            note: Set(None),
            current_path: Set("/tmp/m.zip".into()),
            current_location: Set("identified".into()),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        };
        am.insert(conn).await.unwrap().id
    }

    #[tokio::test]
    async fn update_metadata_changes_only_specified_fields() {
        let dir = tempfile::tempdir().unwrap();
        let conn = db::connect(&dir.path().join("t.db")).await.unwrap();
        db::migrations::init_schema_versioned(&conn).await.unwrap();
        let id = seed_row(&conn).await;

        let patch = MetadataPatch {
            title: Some("新标题".into()),
            note: Some("hi".into()),
            ..Default::default()
        };
        apply_metadata_patch(&conn, id, patch).await.unwrap();

        let row = doujinshi_file::Entity::find_by_id(id)
            .one(&conn)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.title, "新标题");
        assert_eq!(row.circle.as_deref(), Some("旧社团"));
        assert_eq!(row.note.as_deref(), Some("hi"));
    }
}
