//! 启动时扫描 identified/will_delete/archived 三个目录：
//! - 目录有文件但 DB 无匹配 → 写 dirty_data
//! - DB 行 current_path 在对应目录不存在 → has_physical_file=false
//!
//! inbox 目录不扫描（inbox 文件本就没入库）。

use anyhow::Result;
use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set};
use std::path::Path;
use walkdir::WalkDir;

use crate::db::entities::{dirty_data, doujinshi_file};

pub async fn scan(
    conn: &DatabaseConnection,
    identified_dir: &Path,
    will_delete_dir: &Path,
    archived_dir: &Path,
) -> Result<ScanReport> {
    let mut report = ScanReport::default();

    for (dir, name) in [
        (identified_dir, "identified"),
        (will_delete_dir, "will_delete"),
        (archived_dir, "archived"),
    ] {
        scan_dir(conn, dir, name, &mut report).await?;
    }

    scan_db_for_missing_files(conn, identified_dir, will_delete_dir, archived_dir, &mut report)
        .await?;

    Ok(report)
}

#[derive(Debug, Default)]
pub struct ScanReport {
    pub orphans: usize,
    pub db_missing_files: usize,
}

async fn scan_dir(
    conn: &DatabaseConnection,
    dir: &Path,
    detected_dir: &str,
    report: &mut ScanReport,
) -> Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    for entry in WalkDir::new(dir) {
        let e = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        if !e.file_type().is_file() {
            continue;
        }
        // `.gitkeep` 是仓库占位文件，git tracking 一个空目录用的；它
        // 不是真实的同人志文件，启动扫描不应把它当成 orphan 写入
        // dirty_data 表。
        if e.file_name().to_string_lossy() == ".gitkeep" {
            continue;
        }
        let path = e.path().to_string_lossy().into_owned();
        let size = e.metadata().map(|m| m.len() as i64).unwrap_or(0);

        let exists = dirty_data::Entity::find()
            .filter(dirty_data::Column::FilePath.eq(&path))
            .one(conn)
            .await?;
        if exists.is_some() {
            continue;
        }

        let matching_row = doujinshi_file::Entity::find()
            .filter(doujinshi_file::Column::CurrentPath.eq(&path))
            .one(conn)
            .await?;

        if matching_row.is_none() {
            let am = dirty_data::ActiveModel {
                file_path: Set(path),
                file_size: Set(size),
                detected_dir: Set(detected_dir.into()),
                reason: Set("orphan_file".into()),
                first_seen_at: Set(chrono::Utc::now().to_rfc3339()),
                ..Default::default()
            };
            am.insert(conn).await?;
            report.orphans += 1;
        } else {
            let mut am: doujinshi_file::ActiveModel = matching_row.unwrap().into();
            am.has_physical_file = Set(true);
            am.update(conn).await?;
        }
    }
    Ok(())
}

async fn scan_db_for_missing_files(
    conn: &DatabaseConnection,
    identified_dir: &Path,
    will_delete_dir: &Path,
    archived_dir: &Path,
    report: &mut ScanReport,
) -> Result<()> {
    let rows = doujinshi_file::Entity::find()
        .filter(doujinshi_file::Column::CurrentLocation.is_in(["identified", "will_delete", "archived"]))
        .all(conn)
        .await?;
    for row in rows {
        let expected_dir = match row.current_location.as_str() {
            "identified" => identified_dir,
            "will_delete" => will_delete_dir,
            "archived" => archived_dir,
            _ => continue,
        };
        let p = std::path::Path::new(&row.current_path);
        let exists_in_expected = p.exists() && p.starts_with(expected_dir);
        let mut am: doujinshi_file::ActiveModel = row.into();
        am.has_physical_file = Set(exists_in_expected);
        if !exists_in_expected {
            report.db_missing_files += 1;
        }
        am.update(conn).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{self, migrations};

    fn touch(p: &std::path::Path) {
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(p, b"x").unwrap();
    }

    async fn open_db(dir: &std::path::Path) -> sea_orm::DatabaseConnection {
        let conn = db::connect(&dir.join("t.db")).await.unwrap();
        migrations::init_schema_versioned(&conn).await.unwrap();
        conn
    }

    #[tokio::test]
    async fn scan_detects_orphan_files() {
        let dir = tempfile::tempdir().unwrap();
        let identified = dir.path().join("identified");
        let will_delete = dir.path().join("will_delete");
        let archived = dir.path().join("archived");
        std::fs::create_dir_all(&identified).unwrap();
        std::fs::create_dir_all(&will_delete).unwrap();
        std::fs::create_dir_all(&archived).unwrap();
        touch(&identified.join("orphan.zip"));

        let conn = open_db(dir.path()).await;
        let report = scan(&conn, &identified, &will_delete, &archived).await.unwrap();

        assert_eq!(report.orphans, 1);
        let rows = dirty_data::Entity::find().all(&conn).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].file_path, identified.join("orphan.zip").to_string_lossy());
        assert_eq!(rows[0].detected_dir, "identified");
    }

    #[tokio::test]
    async fn scan_marks_db_rows_with_missing_files() {
        let dir = tempfile::tempdir().unwrap();
        let identified = dir.path().join("identified");
        let will_delete = dir.path().join("will_delete");
        let archived = dir.path().join("archived");
        std::fs::create_dir_all(&identified).unwrap();
        std::fs::create_dir_all(&will_delete).unwrap();
        std::fs::create_dir_all(&archived).unwrap();

        let conn = open_db(dir.path()).await;
        let now = chrono::Utc::now();
        let m = doujinshi_file::ActiveModel {
            title: Set("t".into()),
            filename: Set("g.zip".into()),
            hash: Set("hh".into()),
            ext: Set("zip".into()),
            size_bytes: Set(0),
            current_path: Set(identified.join("g.zip").to_string_lossy().into_owned()),
            current_location: Set("identified".into()),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        };
        m.insert(&conn).await.unwrap();

        let report = scan(&conn, &identified, &will_delete, &archived).await.unwrap();

        assert_eq!(report.db_missing_files, 1);
        let row = doujinshi_file::Entity::find().one(&conn).await.unwrap().unwrap();
        assert!(!row.has_physical_file);
    }

    #[tokio::test]
    async fn scan_does_not_check_inbox() {
        let dir = tempfile::tempdir().unwrap();
        let identified = dir.path().join("identified");
        let will_delete = dir.path().join("will_delete");
        let archived = dir.path().join("archived");
        let inbox = dir.path().join("inbox");
        std::fs::create_dir_all(&inbox).unwrap();
        touch(&inbox.join("not_yet.zip"));

        let conn = open_db(dir.path()).await;
        let report = scan(&conn, &identified, &will_delete, &archived).await.unwrap();

        assert_eq!(report.orphans, 0);
        assert_eq!(report.db_missing_files, 0);
        let rows = dirty_data::Entity::find().all(&conn).await.unwrap();
        assert_eq!(rows.len(), 0);
    }

    #[tokio::test]
    async fn scan_ignores_gitkeep() {
        // .gitkeep 是仓库占位文件，不是真同人志，扫描要跳过。
        let dir = tempfile::tempdir().unwrap();
        let identified = dir.path().join("identified");
        let will_delete = dir.path().join("will_delete");
        let archived = dir.path().join("archived");
        std::fs::create_dir_all(&identified).unwrap();
        std::fs::create_dir_all(&will_delete).unwrap();
        std::fs::create_dir_all(&archived).unwrap();
        touch(&identified.join(".gitkeep"));
        touch(&will_delete.join(".gitkeep"));
        touch(&archived.join(".gitkeep"));

        let conn = open_db(dir.path()).await;
        let report = scan(&conn, &identified, &will_delete, &archived).await.unwrap();

        assert_eq!(report.orphans, 0, ".gitkeep must not appear in dirty_data");
        let rows = dirty_data::Entity::find().all(&conn).await.unwrap();
        assert_eq!(rows.len(), 0);
    }
}