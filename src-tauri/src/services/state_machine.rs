//! V4 状态机：DB 优先 + 文件 best-effort。
//!
//! 规则变更（相对 V3）：
//! - 任意 status → 任意 status（V3 的"非法转移"概念消失）。
//! - 源文件缺失不阻塞 status 更新；DB 永远更新，文件搬运 no-op。
//! - 目标目录同名 = 视为孤儿，自动覆盖并写 `dirty_data(reason='overwritten_by_state_switch')`。
//! - 跨设备 rename 走 copy + remove 兜底。
//!
//! 销毁（`status='deleted'`）不走本模块，由 `commands::recycle::permanent_delete_inner`
//! 实现：复合操作（status=deleted + file_state=absent_confirmed + best-effort remove）。

use anyhow::{anyhow, Result};
use sea_orm::{ActiveModelTrait, DatabaseConnection, EntityTrait, QueryFilter, Set};
use sea_orm::ColumnTrait;
use std::path::{Path, PathBuf};

use crate::db::entities::{dirty_data, doujinshi_file};

#[derive(Debug, Clone, Copy)]
pub enum TransitionKind {
    /// 任意 status → archived
    Archive,
    /// 任意 status → in_library
    Restore,
    /// 任意 status → recycle
    MarkForDelete,
}

impl TransitionKind {
    pub fn target(self) -> &'static str {
        match self {
            TransitionKind::Archive => "archived",
            TransitionKind::Restore => "in_library",
            TransitionKind::MarkForDelete => "recycle",
        }
    }
}

pub async fn transition(
    conn: &DatabaseConnection,
    id: i64,
    kind: TransitionKind,
) -> Result<()> {
    let cfg = crate::config::AppConfig::load()?;
    transition_with_dirs(
        conn,
        id,
        kind,
        &cfg.identified_dir(),
        &cfg.will_delete_dir(),
        &cfg.archived_dir(),
    )
    .await
}

pub async fn transition_with_dirs(
    conn: &DatabaseConnection,
    id: i64,
    kind: TransitionKind,
    identified_dir: &Path,
    will_delete_dir: &Path,
    archived_dir: &Path,
) -> Result<()> {
    let target = kind.target();

    let row = doujinshi_file::Entity::find_by_id(id)
        .one(conn)
        .await?
        .ok_or_else(|| anyhow!("file {} not found", id))?;

    let target_dir = match target {
        "in_library" => identified_dir,
        "recycle" => will_delete_dir,
        "archived" => archived_dir,
        _ => return Err(anyhow!("unknown target status: {}", target)),
    };

    let src = PathBuf::from(&row.last_seen_path);
    let mut am: doujinshi_file::ActiveModel = row.into();

    // 文件搬运（best-effort）。
    // 任意 status → 任意 to：策略只有"搬 / 不搬"两条。
    if src.exists() {
        let basename = src
            .file_name()
            .ok_or_else(|| anyhow!("invalid source path: {}", src.display()))?;
        let dest = target_dir.join(basename);

        std::fs::create_dir_all(target_dir)?;

        // 目标位置已有同名 → 视为孤儿，自动覆盖。
        // 先记录孤儿：原目标文件此刻将被覆盖，写一条 dirty_data 留底。
        if dest.exists() {
            let dirty = dirty_data::ActiveModel {
                file_path: Set(dest.to_string_lossy().into_owned()),
                file_size: Set(0),
                detected_dir: Set(target.to_string()),
                reason: Set("overwritten_by_state_switch".into()),
                first_seen_at: Set(chrono::Utc::now().to_rfc3339()),
                ..Default::default()
            };
            // best-effort；写失败不影响主流程
            let _ = dirty.insert(conn).await;
        }

        if let Err(e) = std::fs::rename(&src, &dest) {
            // 跨设备 fallback（Windows ERROR_NOT_SAME_DEVICE=17）
            if matches!(e.kind(), std::io::ErrorKind::CrossesDevices)
                || e.raw_os_error() == Some(17)
            {
                std::fs::copy(&src, &dest)?;
                std::fs::remove_file(&src)?;
            } else {
                return Err(e.into());
            }
        }

        am.last_seen_path = Set(dest.to_string_lossy().into_owned());
        // file_state 保持现有值，搬运成功 = present（已是 present）或被扫描器刷过
    } else {
        // 源文件不在：搬运 no-op，标记 missing
        am.file_state = Set("missing".into());
    }

    am.status = Set(target.into());
    am.updated_at = Set(chrono::Utc::now());
    am.update(conn).await?;
    Ok(())
}

/// V4 帮助函数：扫 4 个业务 status 中除 deleted 之外的 3 个（用于 dirty_scanner 等）
pub fn non_deleted_statuses() -> [&'static str; 3] {
    ["in_library", "archived", "recycle"]
}

/// 当前 status 是否是 deleted（用于 collision check 排除）
pub fn is_deleted_status(s: &str) -> bool {
    s == "deleted"
}

/// V4 帮助：dirty_scanner 在检查 expected_dir 时使用，避免重新字符串散落各处
pub fn expected_dir_for_status<'a>(
    status: &str,
    identified_dir: &'a Path,
    will_delete_dir: &'a Path,
    archived_dir: &'a Path,
) -> Option<&'a Path> {
    match status {
        "in_library" => Some(identified_dir),
        "recycle" => Some(will_delete_dir),
        "archived" => Some(archived_dir),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{self, migrations};
    use sea_orm::ActiveModelTrait;

    async fn setup_dirs() -> (tempfile::TempDir, PathBuf, PathBuf, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let identified = dir.path().join("identified");
        let will_delete = dir.path().join("will_delete");
        let archived = dir.path().join("archived");
        std::fs::create_dir_all(&identified).unwrap();
        std::fs::create_dir_all(&will_delete).unwrap();
        std::fs::create_dir_all(&archived).unwrap();
        (dir, identified, will_delete, archived)
    }

    async fn seed_row(
        conn: &sea_orm::DatabaseConnection,
        status: &str,
        last_seen_path: &str,
        file_state: &str,
    ) -> i64 {
        let now = chrono::Utc::now();
        let m = doujinshi_file::ActiveModel {
            title: Set("t".into()),
            filename: Set("f.zip".into()),
            hash: Set("h".into()),
            ext: Set("zip".into()),
            size_bytes: Set(0),
            last_seen_path: Set(last_seen_path.into()),
            status: Set(status.into()),
            file_state: Set(file_state.into()),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        };
        m.insert(conn).await.unwrap().id
    }

    async fn open_db(dir: &std::path::Path) -> sea_orm::DatabaseConnection {
        let conn = db::connect(&dir.join("t.db")).await.unwrap();
        migrations::init_schema_versioned(&conn).await.unwrap();
        conn
    }

    /// V4 新语义：源文件缺失时状态切换仍成功（DB 优先）
    #[tokio::test]
    async fn transition_succeeds_when_source_file_missing() {
        let (_dir, identified, will_delete, archived) = setup_dirs().await;
        let conn = open_db(_dir.path()).await;
        let id = seed_row(&conn, "in_library", "missing/f.zip", "missing").await;

        transition_with_dirs(
            &conn,
            id,
            TransitionKind::Archive,
            &identified,
            &will_delete,
            &archived,
        )
        .await
        .unwrap();

        let row = doujinshi_file::Entity::find_by_id(id)
            .one(&conn)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.status, "archived");
        // 源文件不在 → last_seen_path 保留历史值
        assert_eq!(row.last_seen_path, "missing/f.zip");
        assert_eq!(row.file_state, "missing");
    }

    /// V4 新语义：目标目录同名 = 视为孤儿，自动覆盖 + dirty_data 写入
    #[tokio::test]
    async fn transition_overwrites_orphan_in_target_dir() {
        let (_dir, identified, will_delete, archived) = setup_dirs().await;
        let conn = open_db(_dir.path()).await;

        let src = identified.join("f.zip");
        std::fs::write(&src, b"new").unwrap();
        let id = seed_row(&conn, "in_library", &src.to_string_lossy(), "present").await;

        // 在 will_delete 放同名孤儿
        std::fs::write(will_delete.join("f.zip"), b"orphan").unwrap();

        transition_with_dirs(
            &conn,
            id,
            TransitionKind::MarkForDelete,
            &identified,
            &will_delete,
            &archived,
        )
        .await
        .unwrap();

        // will_delete/f.zip 被覆盖
        let content = std::fs::read(will_delete.join("f.zip")).unwrap();
        assert_eq!(content, b"new");
        // dirty_data 新增 overwritten_by_state_switch
        let rows = crate::db::entities::dirty_data::Entity::find()
            .all(&conn)
            .await
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].reason, "overwritten_by_state_switch");
        assert_eq!(rows[0].detected_dir, "recycle");
    }

    /// V4：任意 status 可切到任意 status（V3 的"非法转移"概念消失）
    #[tokio::test]
    async fn any_to_any_status_is_allowed() {
        let (_dir, identified, will_delete, archived) = setup_dirs().await;
        let conn = open_db(_dir.path()).await;
        // deleted → in_library 应当成功（V3 这是非法）
        let id = seed_row(&conn, "deleted", "missing/f.zip", "absent_confirmed").await;

        transition_with_dirs(
            &conn,
            id,
            TransitionKind::Restore,
            &identified,
            &will_delete,
            &archived,
        )
        .await
        .unwrap();

        let row = doujinshi_file::Entity::find_by_id(id)
            .one(&conn)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.status, "in_library");
    }

    /// V4：所有转移都允许源文件缺失
    #[tokio::test]
    async fn all_transitions_succeed_when_source_missing() {
        let (_dir, identified, will_delete, archived) = setup_dirs().await;
        let conn = open_db(_dir.path()).await;

        for (from, kind, expect_to) in [
            ("in_library", TransitionKind::Archive, "archived"),
            ("in_library", TransitionKind::MarkForDelete, "recycle"),
            ("archived", TransitionKind::Restore, "in_library"),
            ("recycle", TransitionKind::Restore, "in_library"),
            ("deleted", TransitionKind::Restore, "in_library"),
        ] {
            let id = seed_row(&conn, from, "missing/f.zip", "missing").await;
            transition_with_dirs(&conn, id, kind, &identified, &will_delete, &archived)
                .await
                .unwrap();
            let row = doujinshi_file::Entity::find_by_id(id)
                .one(&conn)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(row.status, expect_to, "from={:?}", from);
        }
    }

    /// V4：源文件存在时正常搬运
    #[tokio::test]
    async fn transition_moves_file_when_present() {
        let (_dir, identified, will_delete, archived) = setup_dirs().await;
        let conn = open_db(_dir.path()).await;

        let src = identified.join("f.zip");
        std::fs::write(&src, b"data").unwrap();
        let id = seed_row(&conn, "in_library", &src.to_string_lossy(), "present").await;

        transition_with_dirs(
            &conn,
            id,
            TransitionKind::Archive,
            &identified,
            &will_delete,
            &archived,
        )
        .await
        .unwrap();

        assert!(!src.exists(), "src 应被移走");
        assert!(archived.join("f.zip").exists(), "dest 应存在");

        let row = doujinshi_file::Entity::find_by_id(id)
            .one(&conn)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.status, "archived");
    }
}
