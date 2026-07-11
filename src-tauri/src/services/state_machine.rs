//! 4 状态机的转移核心。
//!
//! 规则：DB UPDATE + best-effort 文件移动；src 不存在时 no-op + physically_deleted=true。
//! 4 个合法转移：
//!   - identified → archived (Archive)
//!   - identified → will_delete (MarkForDelete)
//!   - will_delete → identified (Restore)
//!   - archived → identified (Restore)
//! 其他转移非法，调用方应先检查状态。

use anyhow::{anyhow, Result};
use sea_orm::{ActiveModelTrait, DatabaseConnection, EntityTrait, Set};
use std::path::{Path, PathBuf};

use crate::db::entities::doujinshi_file;

#[derive(Debug, Clone, Copy)]
pub enum TransitionKind {
    Archive,
    Restore,
    MarkForDelete,
}

impl TransitionKind {
    fn target(&self, from: &str) -> Option<&'static str> {
        match (self, from) {
            (TransitionKind::Archive, "identified") => Some("archived"),
            (TransitionKind::Restore, "will_delete") => Some("identified"),
            (TransitionKind::Restore, "archived") => Some("identified"),
            (TransitionKind::MarkForDelete, "identified") => Some("will_delete"),
            _ => None,
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
    let row = doujinshi_file::Entity::find_by_id(id)
        .one(conn)
        .await?
        .ok_or_else(|| anyhow!("file {} not found", id))?;

    let target = kind
        .target(&row.current_location)
        .ok_or_else(|| {
            anyhow!(
                "illegal transition {:?} from {}",
                kind,
                row.current_location
            )
        })?;

    let target_dir = match target {
        "identified" => identified_dir,
        "will_delete" => will_delete_dir,
        "archived" => archived_dir,
        other => return Err(anyhow!("unknown target {}", other)),
    };

    let src = PathBuf::from(&row.current_path);
    let filename = src
        .file_name()
        .ok_or_else(|| anyhow!("invalid source path: {}", src.display()))?;
    let dest = target_dir.join(filename);

    let mut am: doujinshi_file::ActiveModel = row.into();

    if src.exists() {
        std::fs::create_dir_all(target_dir)?;
        if let Err(e) = std::fs::rename(&src, &dest) {
            if matches!(e.kind(), std::io::ErrorKind::CrossesDevices)
                || e.raw_os_error() == Some(17)
            {
                std::fs::copy(&src, &dest)?;
                std::fs::remove_file(&src)?;
            } else {
                return Err(e.into());
            }
        }
        am.physically_deleted = Set(false);
    } else {
        am.physically_deleted = Set(true);
    }

    am.current_location = Set(target.into());
    am.current_path = Set(dest.to_string_lossy().into_owned());
    am.updated_at = Set(chrono::Utc::now());
    am.update(conn).await?;
    Ok(())
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
        location: &str,
        current_path: &str,
    ) -> i64 {
        let now = chrono::Utc::now();
        let m = doujinshi_file::ActiveModel {
            title: Set("t".into()),
            filename: Set("f.zip".into()),
            hash: Set("h".into()),
            ext: Set("zip".into()),
            size_bytes: Set(0),
            current_path: Set(current_path.into()),
            current_location: Set(location.into()),
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

    #[tokio::test]
    async fn transition_updates_location_only_when_no_file() {
        let (_dir, identified, will_delete, archived) = setup_dirs().await;
        let conn = open_db(_dir.path()).await;
        let id = seed_row(&conn, "identified", "missing/f.zip").await;

        transition_with_dirs(&conn, id, TransitionKind::Archive, &identified, &will_delete, &archived)
            .await
            .unwrap();

        let row = doujinshi_file::Entity::find_by_id(id)
            .one(&conn)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.current_location, "archived");
        assert!(row.physically_deleted, "src 不存在 → physically_deleted=true");
    }

    #[tokio::test]
    async fn transition_moves_file_when_present() {
        let (_dir, identified, will_delete, archived) = setup_dirs().await;
        let conn = open_db(_dir.path()).await;

        let src = identified.join("f.zip");
        std::fs::write(&src, b"data").unwrap();
        let id = seed_row(&conn, "identified", &src.to_string_lossy()).await;

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
        assert_eq!(row.current_location, "archived");
        assert!(!row.physically_deleted);
    }

    #[tokio::test]
    async fn transition_rejects_illegal() {
        let (_dir, identified, will_delete, archived) = setup_dirs().await;
        let conn = open_db(_dir.path()).await;
        let id = seed_row(&conn, "archived", "missing/f.zip").await;

        let err = transition_with_dirs(
            &conn,
            id,
            TransitionKind::MarkForDelete,
            &identified,
            &will_delete,
            &archived,
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("illegal"), "err: {}", err);
    }
}