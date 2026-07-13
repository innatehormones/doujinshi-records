//! 文件写入前的统一守卫。
//!
//! - `ensure_no_open_conflict`：目标文件存在尚未解决的冲突时拒绝写操作。
//!   规则：conflict 表里 `a_file_id = id AND resolved = false` 即视为冲突未解决。
//!   在前端表现出"归档 / 移到回收站 / 彻底删除"等按钮在该状态下被禁用，
//!   后端仍做兜底拦截（防止 HTTP / 浏览器扩展绕过 UI）。

use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, PaginatorTrait, QueryFilter};

use crate::db::entities::conflict;
use crate::error::{AppError, AppResult};

pub async fn ensure_no_open_conflict(conn: &DatabaseConnection, file_id: i64) -> AppResult<()> {
    let count = conflict::Entity::find()
        .filter(conflict::Column::AFileId.eq(file_id))
        .filter(conflict::Column::Resolved.eq(false))
        .count(conn)
        .await?;
    if count > 0 {
        return Err(AppError::ConflictPending {
            count: count as usize,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::db::entities::conflict;
    use crate::db::migrations;
    use sea_orm::{ActiveModelTrait, Set};

    async fn open_db(dir: &std::path::Path) -> sea_orm::DatabaseConnection {
        let conn = db::connect(&dir.join("t.db")).await.unwrap();
        migrations::init_schema_versioned(&conn).await.unwrap();
        conn
    }

    async fn insert_conflict(conn: &sea_orm::DatabaseConnection, a_file_id: i64, resolved: bool) {
        let am = conflict::ActiveModel {
            a_file_id: Set(a_file_id),
            b_file_path: Set("/tmp/b.zip".into()),
            b_filename: Set("b.zip".into()),
            b_hash: Set(None),
            reason: Set("name_ext_collision".into()),
            resolved: Set(resolved),
            created_at: Set(chrono::Utc::now()),
            ..Default::default()
        };
        am.insert(conn).await.unwrap();
    }

    #[tokio::test]
    async fn blocks_when_open_conflict_exists() {
        let dir = tempfile::tempdir().unwrap();
        let conn = open_db(dir.path()).await;
        insert_conflict(&conn, 7, false).await;
        let err = ensure_no_open_conflict(&conn, 7).await.unwrap_err();
        assert!(matches!(err, AppError::ConflictPending { count: 1 }), "got: {:?}", err);
    }

    #[tokio::test]
    async fn allows_when_conflict_resolved() {
        let dir = tempfile::tempdir().unwrap();
        let conn = open_db(dir.path()).await;
        insert_conflict(&conn, 8, true).await;
        ensure_no_open_conflict(&conn, 8).await.unwrap();
    }

    #[tokio::test]
    async fn allows_when_no_conflict_rows() {
        let dir = tempfile::tempdir().unwrap();
        let conn = open_db(dir.path()).await;
        ensure_no_open_conflict(&conn, 9).await.unwrap();
    }
}