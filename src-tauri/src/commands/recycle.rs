use crate::db::entities::doujinshi_file;
use crate::error::{AppError, AppResult};
use crate::models::{file_summary, Page};
use crate::AppState;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, PaginatorTrait, QueryFilter,
    QuerySelect, Set,
};
use serde::Serialize;
use tauri::State;

#[derive(Debug, Serialize)]
pub struct RecyclePage {
    pub present: Page<file_summary::FileSummary>,
    pub gone: Page<file_summary::FileSummary>,
}

#[tauri::command]
pub async fn list_recycle(
    state: State<'_, AppState>,
    present_limit: Option<u64>,
    present_offset: Option<u64>,
    gone_limit: Option<u64>,
    gone_offset: Option<u64>,
) -> AppResult<RecyclePage> {
    // V4：按 status='recycle' + file_state 三态分组。
    // deleted 不在这里（在 Library 过滤 status=deleted 里）。
    let present_limit = present_limit.unwrap_or(50);
    let present_offset = present_offset.unwrap_or(0);
    let gone_limit = gone_limit.unwrap_or(50);
    let gone_offset = gone_offset.unwrap_or(0);

    let present_q = doujinshi_file::Entity::find().filter(
        doujinshi_file::Column::Status
            .eq("recycle")
            .and(doujinshi_file::Column::FileState.eq("present")),
    );
    let present_total = present_q.clone().count(&state.conn).await?;
    let present_rows = present_q
        .clone()
        .offset(present_offset)
        .limit(present_limit)
        .all(&state.conn)
        .await?;

    let gone_q = doujinshi_file::Entity::find().filter(
        doujinshi_file::Column::Status
            .eq("recycle")
            .and(doujinshi_file::Column::FileState.ne("present")),
    );
    let gone_total = gone_q.clone().count(&state.conn).await?;
    let gone_rows = gone_q
        .clone()
        .offset(gone_offset)
        .limit(gone_limit)
        .all(&state.conn)
        .await?;

    let mut ids: Vec<i64> = present_rows.iter().chain(gone_rows.iter()).map(|m| m.id).collect();
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
    Ok(RecyclePage {
        present: Page { items: map_summaries(&present_rows), total: present_total },
        gone: Page { items: map_summaries(&gone_rows), total: gone_total },
    })
}

/// V4："销毁"复合操作——不走 state_machine。
/// 直接做：status='deleted' + file_state='absent_confirmed' + best-effort 删盘上文件
/// + 写 scan_event('destroyed') + 清理预览缓存。
#[tauri::command]
pub async fn permanent_delete(state: State<'_, AppState>, id: i64) -> AppResult<()> {
    crate::commands::guards::ensure_no_open_conflict(&state.conn, id).await?;
    permanent_delete_inner(&state.conn, id).await?;
    state.preview_cache.invalidate(id);
    crate::services::identifier::record_event(&state.conn, id, "destroyed", None)
        .await
        .map_err(|e| AppError::Other(e.to_string()))?;
    Ok(())
}

/// V4 inner：可被 HTTP handler / integration test 直接调用。
pub async fn permanent_delete_inner(
    conn: &DatabaseConnection,
    id: i64,
) -> AppResult<()> {
    let row = doujinshi_file::Entity::find_by_id(id)
        .one(conn)
        .await?
        .ok_or_else(|| AppError::Other(format!("file {} not found", id)))?;
    let p = std::path::Path::new(&row.last_seen_path);
    if p.exists() {
        // 失败也不阻塞主流程（用户可能已手动删了等）
        let _ = std::fs::remove_file(p);
    }
    let mut am: doujinshi_file::ActiveModel = row.into();
    am.status = Set("deleted".into());
    am.file_state = Set("absent_confirmed".into());
    am.has_physical_file = Set(false);
    am.updated_at = Set(chrono::Utc::now());
    am.update(conn).await?;
    Ok(())
}

/// V4：从 recycle 取回到 in_library。
/// 注：`commands::library::restore` 通过 state_machine 已支持任意 status→in_library
/// 的取回，本函数保留以兼容现有 Tauri command 注册。
#[tauri::command]
pub async fn restore_from_recycle(state: State<'_, AppState>, id: i64) -> AppResult<()> {
    let file = doujinshi_file::Entity::find_by_id(id)
        .one(&state.conn)
        .await?
        .ok_or(AppError::NotFound)?;
    let current = std::path::PathBuf::from(&file.last_seen_path);
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
    am.last_seen_path = Set(target.to_string_lossy().into_owned());
    am.status = Set("in_library".into());
    am.file_state = Set("present".into());
    am.has_physical_file = Set(true);
    am.marked_for_delete = Set(false);
    am.updated_at = Set(chrono::Utc::now());
    am.update(&state.conn).await?;
    crate::services::identifier::record_event(&state.conn, id, "restore_from_recycle", None)
        .await
        .map_err(|e| AppError::Other(e.to_string()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{self, migrations};

    #[tokio::test]
    async fn permanent_delete_marks_status_deleted_and_file_state_absent_confirmed() {
        let dir = tempfile::tempdir().unwrap();
        let will_delete = dir.path().join("will_delete");
        std::fs::create_dir_all(&will_delete).unwrap();
        let conn = db::connect(&dir.path().join("t.db")).await.unwrap();
        migrations::init_schema_versioned(&conn).await.unwrap();

        let src = will_delete.join("f.zip");
        std::fs::write(&src, b"data").unwrap();

        let now = chrono::Utc::now();
        let m = doujinshi_file::ActiveModel {
            title: Set("t".into()),
            filename: Set("f.zip".into()),
            hash: Set("h".into()),
            ext: Set("zip".into()),
            size_bytes: Set(4),
            last_seen_path: Set(src.to_string_lossy().into_owned()),
            status: Set("recycle".into()),
            file_state: Set("present".into()),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        };
        let id = m.insert(&conn).await.unwrap().id;

        permanent_delete_inner(&conn, id).await.unwrap();

        assert!(!src.exists(), "源文件应被 best-effort 删");
        let row = doujinshi_file::Entity::find_by_id(id)
            .one(&conn)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.status, "deleted");
        assert_eq!(row.file_state, "absent_confirmed");
    }

    #[tokio::test]
    async fn permanent_delete_succeeds_when_source_already_missing() {
        let dir = tempfile::tempdir().unwrap();
        let conn = db::connect(&dir.path().join("t.db")).await.unwrap();
        migrations::init_schema_versioned(&conn).await.unwrap();

        let now = chrono::Utc::now();
        let m = doujinshi_file::ActiveModel {
            title: Set("t".into()),
            filename: Set("f.zip".into()),
            hash: Set("h".into()),
            ext: Set("zip".into()),
            size_bytes: Set(0),
            last_seen_path: Set("/missing/f.zip".into()),
            status: Set("recycle".into()),
            file_state: Set("missing".into()),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        };
        let id = m.insert(&conn).await.unwrap().id;

        permanent_delete_inner(&conn, id).await.unwrap();

        let row = doujinshi_file::Entity::find_by_id(id)
            .one(&conn)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.status, "deleted");
        assert_eq!(row.file_state, "absent_confirmed");
    }
}
