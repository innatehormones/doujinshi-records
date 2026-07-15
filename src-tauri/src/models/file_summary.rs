use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, PaginatorTrait, QueryFilter};

use crate::db::entities::{conflict, doujinshi_file};

/// V4：FileSummary 用 status + file_state 双字段模型。
#[derive(Debug, Clone, serde::Serialize)]
pub struct FileSummary {
    pub id: i64,
    pub title: String,
    pub circle: Option<String>,
    pub hash: String,
    pub ext: String,
    pub size_bytes: i64,
    pub viewed: bool,
    /// 业务状态：`in_library / archived / recycle / deleted`
    pub status: String,
    /// 文件状态：`present / missing / absent_confirmed`
    pub file_state: String,
    pub cover_url: Option<String>,
    /// 文件是否挂在未解决的命名冲突。`true` 时前端应禁用归档 / 移到回收站 /
    /// 彻底删除等按钮（后端同样兜底拦截，浏览器扩展或 HTTP 调用绕不开）。
    pub has_open_conflict: bool,
}

/// 把一行 doujinshi_file 转成前端用的 FileSummary。
///
/// `has_open_conflict` 需要一次额外查询。`list_library` / `list_recycle`
/// 这类批量端点应在循环外批量化查询避免 N+1，这里只服务于单条 `get_by_id`
/// 风格的场景；批量端点请用 `from_model_with_conflict_state`。
pub async fn from_model(conn: &DatabaseConnection, m: &doujinshi_file::Model) -> FileSummary {
    let has_open_conflict = has_open_conflict_for(conn, m.id).await;
    from_model_with_conflict_state(m, has_open_conflict)
}

/// 已知冲突状态时的纯映射版本。批量端点先批查一次 conflict.a_file_id IN (...)
/// 再循环调用本函数，避免每行一次 round-trip。
pub fn from_model_with_conflict_state(m: &doujinshi_file::Model, has_open_conflict: bool) -> FileSummary {
    FileSummary {
        id: m.id,
        title: m.title.clone(),
        circle: m.circle.clone(),
        hash: m.hash.clone(),
        ext: m.ext.clone(),
        size_bytes: m.size_bytes,
        viewed: m.viewed,
        status: m.status.clone(),
        file_state: m.file_state.clone(),
        cover_url: m.cover_path.as_ref().map(|_| format!("/api/covers/{}", m.hash)),
        has_open_conflict,
    }
}

async fn has_open_conflict_for(conn: &DatabaseConnection, file_id: i64) -> bool {
    conflict::Entity::find()
        .filter(conflict::Column::AFileId.eq(file_id))
        .filter(conflict::Column::Resolved.eq(false))
        .count(conn)
        .await
        .map(|n| n > 0)
        .unwrap_or(false)
}

/// 批量查：传入一组 file_id，返回 `id -> has_open_conflict`。
pub async fn open_conflict_map(conn: &DatabaseConnection, ids: &[i64]) -> std::collections::HashMap<i64, bool> {
    use std::collections::HashMap;
    let mut out = HashMap::new();
    if ids.is_empty() {
        return out;
    }
    let rows = conflict::Entity::find()
        .filter(conflict::Column::AFileId.is_in(ids.to_vec()))
        .filter(conflict::Column::Resolved.eq(false))
        .all(conn)
        .await
        .unwrap_or_default();
    for id in ids {
        out.insert(*id, false);
    }
    for c in rows {
        out.insert(c.a_file_id, true);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_model_includes_status_and_file_state() {
        let now = chrono::Utc::now();
        let m = doujinshi_file::Model {
            id: 1,
            title: "t".into(),
            filename: "f.zip".into(),
            hash: "h".into(),
            ext: "zip".into(),
            size_bytes: 100,
            circle: None,
            series: None,
            translator: None,
            version_tag: None,
            status: "deleted".into(),
            last_seen_path: "p".into(),
            cover_path: Some("covers/h.pwb".into()),
            marked_for_delete: false,
            has_physical_file: false,
            file_state: "absent_confirmed".into(),
            viewed: false,
            note: None,
            rating: None,
            created_at: now,
            updated_at: now,
        };
        let s = from_model_with_conflict_state(&m, true);
        assert_eq!(s.status, "deleted");
        assert_eq!(s.file_state, "absent_confirmed");
        assert!(!s.has_physical_file);
        assert!(s.has_open_conflict);
        assert_eq!(s.cover_url.as_deref(), Some("/api/covers/h"));
    }
}
