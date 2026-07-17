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
}

#[tauri::command]
pub async fn list_recycle(
    state: State<'_, AppState>,
    present_limit: Option<u64>,
    present_offset: Option<u64>,
) -> AppResult<RecyclePage> {
    // V4.6 简化：文件回收站只展示「待删除文件」（status='recycle' +
    // file_state='present'）。原本按 file_state 三态分 present / gone
    // 两段，gone 段（status='recycle' + file_state≠present）的记录现在
    // 可在 Library 页面用 status filter（recycle / deleted）找到——
    // permanent_delete_inner 永久删除时已经把 status 推到 'deleted'，
    // 真正剩在 status='recycle' 但 file_state≠present 的只是 dirty_scanner
    // 标记 missing 的少数边缘 case，不再专设 UI 段落。
    let present_limit = present_limit.unwrap_or(50);
    let present_offset = present_offset.unwrap_or(0);

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

    let ids: Vec<i64> = present_rows.iter().map(|m| m.id).collect();
    let conflict_map = file_summary::open_conflict_map(&state.conn, &ids).await;
    let items: Vec<file_summary::FileSummary> = present_rows
        .iter()
        .map(|m| {
            let has = conflict_map.get(&m.id).copied().unwrap_or(false);
            file_summary::from_model_with_conflict_state(m, has)
        })
        .collect();
    Ok(RecyclePage {
        present: Page { items, total: present_total },
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
    am.updated_at = Set(chrono::Utc::now());
    am.update(conn).await?;
    Ok(())
}

/// V4：从 recycle 取回到 in_library 由 `commands::library::restore`
/// 通过 state_machine 统一处理（任意 status→in_library），本文件不再
/// 单设入口。

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
