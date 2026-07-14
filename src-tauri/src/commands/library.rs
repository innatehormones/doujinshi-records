use crate::db::entities::doujinshi_file;
use crate::error::{AppError, AppResult};
use crate::models::{file_summary, Page};
use crate::AppState;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder, QuerySelect,
    Set,
};
use serde::{Deserialize, Serialize};
use tauri::State;

#[tauri::command]
pub async fn list_library(
    state: State<'_, AppState>,
    q: Option<String>,
    location: Option<String>,
    limit: Option<u64>,
    offset: Option<u64>,
) -> AppResult<Page<file_summary::FileSummary>> {
    let conn = &state.conn;
    let mut query = doujinshi_file::Entity::find();
    if let Some(loc) = location.as_deref().filter(|s| !s.is_empty() && *s != "all") {
        query = match loc {
            "physically_deleted" => query.filter(doujinshi_file::Column::PhysicallyDeleted.eq(true)),
            other => query.filter(doujinshi_file::Column::CurrentLocation.eq(other)),
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
    let limit = limit.unwrap_or(50);
    let offset = offset.unwrap_or(0);
    let total = query.clone().count(conn).await?;
    let rows = query
        .order_by_desc(doujinshi_file::Column::CreatedAt)
        .limit(limit)
        .offset(offset)
        .all(conn)
        .await?;
    let ids: Vec<i64> = rows.iter().map(|m| m.id).collect();
    let conflict_map = file_summary::open_conflict_map(conn, &ids).await;
    let items: Vec<file_summary::FileSummary> = rows
        .iter()
        .map(|m| {
            let has = conflict_map.get(&m.id).copied().unwrap_or(false);
            file_summary::from_model_with_conflict_state(m, has)
        })
        .collect();
    Ok(Page { items, total })
}

/// Library 页顶部社团快捷筛选。独立端点而不是从 list_library 取——
/// 那是按 limit/offset 切片的子集，按它聚合只算"当前页 top"，误导用户。
/// 全表按出现次数排序，方便"我主要跟的几个社团"快速过滤。
#[derive(Debug, Serialize)]
pub struct CircleCount {
    pub circle: String,
    pub count: u64,
}

#[tauri::command]
pub async fn top_circles(
    state: State<'_, AppState>,
    limit: Option<u64>,
) -> AppResult<Vec<CircleCount>> {
    use sea_orm::{DbBackend, Statement};
    use sea_orm::ConnectionTrait;
    let limit = limit.unwrap_or(10);
    // SQL 侧 GROUP BY + ORDER BY + LIMIT 一次搞定；驱动侧 group_by 海量
    // 字符串开销不小，而且我们其实只关心前 N。
    let stmt = Statement::from_string(
        DbBackend::Sqlite,
        format!(
            "SELECT circle, COUNT(*) as cnt FROM doujinshi_file \
             WHERE circle IS NOT NULL AND circle != '' \
             GROUP BY circle ORDER BY cnt DESC LIMIT {}",
            limit
        ),
    );
    let rows = state.conn.query_all(stmt).await?;
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        let circle: String = r.try_get_by("circle").unwrap_or_default();
        let count: i64 = r.try_get_by("cnt").unwrap_or(0);
        if circle.is_empty() {
            continue;
        }
        out.push(CircleCount { circle, count: count.max(0) as u64 });
    }
    Ok(out)
}

#[tauri::command]
pub async fn mark_for_delete(state: State<'_, AppState>, id: i64) -> AppResult<()> {
    crate::commands::guards::ensure_no_open_conflict(&state.conn, id).await?;
    state_machine_transition(&state, id, crate::services::state_machine::TransitionKind::MarkForDelete).await
}

#[tauri::command]
pub async fn unmark_for_delete(state: State<'_, AppState>, id: i64) -> AppResult<()> {
    state_machine_transition(&state, id, crate::services::state_machine::TransitionKind::Restore).await
}

#[tauri::command]
pub async fn archive(state: State<'_, AppState>, id: i64) -> AppResult<()> {
    crate::commands::guards::ensure_no_open_conflict(&state.conn, id).await?;
    state_machine_transition(&state, id, crate::services::state_machine::TransitionKind::Archive).await
}

#[tauri::command]
pub async fn restore(state: State<'_, AppState>, id: i64) -> AppResult<()> {
    state_machine_transition(&state, id, crate::services::state_machine::TransitionKind::Restore).await
}

async fn state_machine_transition(
    state: &AppState,
    id: i64,
    kind: crate::services::state_machine::TransitionKind,
) -> AppResult<()> {
    crate::services::state_machine::transition_with_dirs(
        &state.conn,
        id,
        kind,
        &state.config.identified_dir(),
        &state.config.will_delete_dir(),
        &state.config.archived_dir(),
    )
    .await?;
    // zip 移动/删除后清掉对应 id 的预览缓存（key 是 (id, idx)，状态变了内容不再有意义）。
    state.preview_cache.invalidate(id);
    Ok(())
}

#[tauri::command]
pub async fn move_to_will_delete(state: State<'_, AppState>, id: i64) -> AppResult<()> {
    crate::commands::guards::ensure_no_open_conflict(&state.conn, id).await?;
    state_machine_transition(
        &state,
        id,
        crate::services::state_machine::TransitionKind::MarkForDelete,
    )
    .await
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
    Ok(file_summary::from_model(&state.conn, &row).await)
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
