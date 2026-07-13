//! 启动时扫描 identified/will_delete/archived 三个目录：
//! - 目录有文件但 DB 无匹配 → 写 dirty_data
//! - DB 行 current_path 在对应目录不存在 → has_physical_file=false
//! - DB 行 current_path 与 current_location 推导出的位置不一致 →
//!   优先按 location 在期望目录下找回真实文件并修正 current_path；
//!   找不到则记 dirty_data 让用户处理
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

/// 把每行 DB 记录按 current_location 推到期望目录里找回真实文件。
///
/// - 文件确实在期望目录 → 修正 current_path，标记 has_physical_file=true
/// - 文件不在期望目录、current_path 已陈旧 → 记 dirty_data 提示用户处理
/// - 文件确实在 current_path（无须修复路径） → 仅刷 has_physical_file
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
        let in_expected_dir = p.starts_with(expected_dir);
        // 提前 clone 出后面仍要用的字段，避免 row.into() 之后被 move。
        let location = row.current_location.clone();
        let current_path_str = row.current_path.clone();

        if in_expected_dir {
            let exists = p.exists();
            let mut am: doujinshi_file::ActiveModel = row.into();
            am.has_physical_file = Set(exists);
            if !exists {
                report.db_missing_files += 1;
                let am_dirty = dirty_data::ActiveModel {
                    file_path: Set(current_path_str.clone()),
                    file_size: Set(0),
                    detected_dir: Set(location.clone()),
                    reason: Set("db_row_file_missing".into()),
                    first_seen_at: Set(chrono::Utc::now().to_rfc3339()),
                    ..Default::default()
                };
                am_dirty.insert(conn).await?;
            }
            am.update(conn).await?;
            continue;
        }

        // current_path 不在 location 期望目录下：尝试按 filename 在期望目录里找回。
        let filename = match p.file_name() {
            Some(name) => name.to_owned(),
            None => {
                let am_dirty = dirty_data::ActiveModel {
                    file_path: Set(current_path_str.clone()),
                    file_size: Set(0),
                    detected_dir: Set(location.clone()),
                    reason: Set("location_path_mismatch".into()),
                    first_seen_at: Set(chrono::Utc::now().to_rfc3339()),
                    ..Default::default()
                };
                am_dirty.insert(conn).await?;
                report.db_missing_files += 1;
                continue;
            }
        };
        let candidate = expected_dir.join(&filename);
        let mut am: doujinshi_file::ActiveModel = row.into();
        if candidate.exists() {
            let fixed = candidate.to_string_lossy().into_owned();
            am.current_path = Set(fixed.clone());
            am.has_physical_file = Set(true);
            // 陈旧 current_path 留作 dirty_data 一条，让用户知道曾被改过。
            let am_dirty = dirty_data::ActiveModel {
                file_path: Set(current_path_str.clone()),
                file_size: Set(0),
                detected_dir: Set(location.clone()),
                reason: Set("location_path_mismatch_resolved".into()),
                first_seen_at: Set(chrono::Utc::now().to_rfc3339()),
                ..Default::default()
            };
            am_dirty.insert(conn).await?;
            am.update(conn).await?;
        } else {
            am.has_physical_file = Set(false);
            let am_dirty = dirty_data::ActiveModel {
                file_path: Set(current_path_str.clone()),
                file_size: Set(0),
                detected_dir: Set(location.clone()),
                reason: Set("location_path_mismatch".into()),
                first_seen_at: Set(chrono::Utc::now().to_rfc3339()),
                ..Default::default()
            };
            am_dirty.insert(conn).await?;
            am.update(conn).await?;
            report.db_missing_files += 1;
        }
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

    #[tokio::test]
    async fn scan_self_heals_stale_current_path_when_file_in_expected_dir() {
        // current_path 指向 inbox（旧 bug 留下），但 location=will_delete，
        // 文件其实就在 will_delete 目录下 → 启动扫描应自动改回正确路径。
        let dir = tempfile::tempdir().unwrap();
        let identified = dir.path().join("identified");
        let will_delete = dir.path().join("will_delete");
        let archived = dir.path().join("archived");
        let inbox = dir.path().join("inbox");
        std::fs::create_dir_all(&identified).unwrap();
        std::fs::create_dir_all(&will_delete).unwrap();
        std::fs::create_dir_all(&archived).unwrap();
        std::fs::create_dir_all(&inbox).unwrap();

        let real_path = will_delete.join("heal.zip");
        touch(&real_path);

        let stale_path = inbox.join("heal.zip");
        let conn = open_db(dir.path()).await;
        let now = chrono::Utc::now();
        let m = doujinshi_file::ActiveModel {
            title: Set("heal".into()),
            filename: Set("heal.zip".into()),
            hash: Set("hh".into()),
            ext: Set("zip".into()),
            size_bytes: Set(0),
            current_path: Set(stale_path.to_string_lossy().into_owned()),
            current_location: Set("will_delete".into()),
            has_physical_file: Set(false),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        };
        m.insert(&conn).await.unwrap();

        let _ = scan(&conn, &identified, &will_delete, &archived).await.unwrap();

        let row = doujinshi_file::Entity::find().one(&conn).await.unwrap().unwrap();
        assert_eq!(row.current_path, real_path.to_string_lossy(),
            "current_path 应被改回到 will_delete 下的真实位置");
        assert!(row.has_physical_file, "has_physical_file 应被刷成 true");

        let dirty = dirty_data::Entity::find()
            .filter(dirty_data::Column::Reason.eq("location_path_mismatch_resolved"))
            .all(&conn)
            .await.unwrap();
        assert_eq!(dirty.len(), 1, "陈旧 current_path 应留 dirty_data 记录");
        assert_eq!(dirty[0].file_path, stale_path.to_string_lossy());
    }

    #[tokio::test]
    async fn scan_marks_unrecoverable_mismatch_as_dirty() {
        let dir = tempfile::tempdir().unwrap();
        let identified = dir.path().join("identified");
        let will_delete = dir.path().join("will_delete");
        let archived = dir.path().join("archived");
        let inbox = dir.path().join("inbox");
        std::fs::create_dir_all(&identified).unwrap();
        std::fs::create_dir_all(&will_delete).unwrap();
        std::fs::create_dir_all(&archived).unwrap();
        std::fs::create_dir_all(&inbox).unwrap();

        let stale_path = inbox.join("gone.zip");
        let conn = open_db(dir.path()).await;
        let now = chrono::Utc::now();
        let m = doujinshi_file::ActiveModel {
            title: Set("gone".into()),
            filename: Set("gone.zip".into()),
            hash: Set("hh".into()),
            ext: Set("zip".into()),
            size_bytes: Set(0),
            current_path: Set(stale_path.to_string_lossy().into_owned()),
            current_location: Set("will_delete".into()),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        };
        m.insert(&conn).await.unwrap();

        let report = scan(&conn, &identified, &will_delete, &archived).await.unwrap();
        assert_eq!(report.db_missing_files, 1);

        let row = doujinshi_file::Entity::find().one(&conn).await.unwrap().unwrap();
        assert!(!row.has_physical_file);
        let dirty = dirty_data::Entity::find()
            .filter(dirty_data::Column::Reason.eq("location_path_mismatch"))
            .all(&conn)
            .await.unwrap();
        assert_eq!(dirty.len(), 1);
        assert_eq!(dirty[0].file_path, stale_path.to_string_lossy());
    }
}