//! V4 启动脏数据扫描：扫 3 个数据目录（in_library/archived/recycle 三个业务状态
//! 各对应一个目录，但 deleted 不对应任何目录，所以不扫）。
//!
//! dirty_scanner 维护 `file_state`（present/missing），不维护 status：
//! - 目录有文件但 DB 无匹配 → 写 dirty_data(reason='orphan_file')
//! - DB 行 last_seen_path 在对应目录但文件丢失 → file_state='missing' + dirty_data
//! - DB 行 last_seen_path 漂出期望目录，但期望目录里有同文件名文件 → 自动修回
//!   真实位置 + file_state='present'，陈旧路径留 dirty_data
//! - 找不到则 dirty_data(reason='location_path_mismatch')
//!
//! inbox 目录不扫描（scanner 还在处理）。
//!
//! 注：detected_dir 用目录名（identified/will_delete/archived）写 dirty_data，
//! 以保持与 V3 schema 视觉一致；state_machine 写的 overwritten_by_state_switch
//! 用 V4 status 名（in_library/recycle/archived）——这两套语义不同，仅作为标签。

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

    // 3 个目录都扫；deleted 没有对应目录，不扫
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
            .filter(dirty_data::Column::ResolvedAt.is_null())
            .one(conn)
            .await?;
        if exists.is_some() {
            continue;
        }

        let matching_row = doujinshi_file::Entity::find()
            .filter(doujinshi_file::Column::LastSeenPath.eq(&path))
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
            am.file_state = Set("present".into());
            am.update(conn).await?;
        }
    }
    Ok(())
}

/// V4：按 status 把每行推到期望目录里找回真实文件。
async fn scan_db_for_missing_files(
    conn: &DatabaseConnection,
    identified_dir: &Path,
    will_delete_dir: &Path,
    archived_dir: &Path,
    report: &mut ScanReport,
) -> Result<()> {
    let rows = doujinshi_file::Entity::find()
        .filter(doujinshi_file::Column::Status.is_in(["in_library", "recycle", "archived"]))
        .all(conn)
        .await?;
    for row in rows {
        let expected_dir = match row.status.as_str() {
            "in_library" => identified_dir,
            "recycle" => will_delete_dir,
            "archived" => archived_dir,
            // 包括 deleted：deleted 不扫，但理论上 deleted 行不该进这里（filter 排除）
            // 万一进来就 skip
            _ => continue,
        };
        // dirty_data.detected_dir 字段沿用 V3 目录名（"identified"/"will_delete"/"archived"）
        let detected_dir_label = match row.status.as_str() {
            "in_library" => "identified",
            "recycle" => "will_delete",
            "archived" => "archived",
            _ => "unknown",
        };
        let p = std::path::Path::new(&row.last_seen_path);
        let in_expected_dir = p.starts_with(expected_dir);
        let status_label = row.status.clone();
        let last_seen_path_str = row.last_seen_path.clone();

        if in_expected_dir {
            let exists = p.exists();
            let mut am: doujinshi_file::ActiveModel = row.into();
            am.file_state = Set(if exists { "present".into() } else { "missing".into() });
            if !exists {
                report.db_missing_files += 1;
                let am_dirty = dirty_data::ActiveModel {
                    file_path: Set(last_seen_path_str.clone()),
                    file_size: Set(0),
                    detected_dir: Set(detected_dir_label.into()),
                    reason: Set("db_row_file_missing".into()),
                    first_seen_at: Set(chrono::Utc::now().to_rfc3339()),
                    ..Default::default()
                };
                am_dirty.insert(conn).await?;
            }
            am.update(conn).await?;
            continue;
        }

        // last_seen_path 不在期望目录下：尝试按 filename 在期望目录里找回。
        let filename = match p.file_name() {
            Some(name) => name.to_owned(),
            None => {
                let am_dirty = dirty_data::ActiveModel {
                    file_path: Set(last_seen_path_str.clone()),
                    file_size: Set(0),
                    detected_dir: Set(detected_dir_label.into()),
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
            am.last_seen_path = Set(fixed.clone());
            am.file_state = Set("present".into());
            // 陈旧 last_seen_path 留作 dirty_data 一条
            let am_dirty = dirty_data::ActiveModel {
                file_path: Set(last_seen_path_str.clone()),
                file_size: Set(0),
                detected_dir: Set(detected_dir_label.into()),
                reason: Set("location_path_mismatch_resolved".into()),
                first_seen_at: Set(chrono::Utc::now().to_rfc3339()),
                ..Default::default()
            };
            am_dirty.insert(conn).await?;
            am.update(conn).await?;
        } else {
            am.file_state = Set("missing".into());
            let am_dirty = dirty_data::ActiveModel {
                file_path: Set(last_seen_path_str.clone()),
                file_size: Set(0),
                detected_dir: Set(detected_dir_label.into()),
                reason: Set("location_path_mismatch".into()),
                first_seen_at: Set(chrono::Utc::now().to_rfc3339()),
                ..Default::default()
            };
            am_dirty.insert(conn).await?;
            am.update(conn).await?;
            report.db_missing_files += 1;
        }

        // 抑制未用变量警告
        let _ = status_label;
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

    /// V4：DB 行 last_seen_path 指向的文件丢失 → file_state='missing'
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
            last_seen_path: Set(identified.join("g.zip").to_string_lossy().into_owned()),
            status: Set("in_library".into()),
            file_state: Set("present".into()),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        };
        m.insert(&conn).await.unwrap();

        let report = scan(&conn, &identified, &will_delete, &archived).await.unwrap();

        assert_eq!(report.db_missing_files, 1);
        let row = doujinshi_file::Entity::find().one(&conn).await.unwrap().unwrap();
        assert_eq!(row.file_state, "missing");
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

    /// V4：自愈仍按 filename 在期望目录找回，修 last_seen_path + file_state
    #[tokio::test]
    async fn scan_self_heals_stale_last_seen_path_when_file_in_expected_dir() {
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
            last_seen_path: Set(stale_path.to_string_lossy().into_owned()),
            status: Set("recycle".into()),
            file_state: Set("missing".into()),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        };
        m.insert(&conn).await.unwrap();

        let _ = scan(&conn, &identified, &will_delete, &archived).await.unwrap();

        let row = doujinshi_file::Entity::find().one(&conn).await.unwrap().unwrap();
        assert_eq!(row.last_seen_path, real_path.to_string_lossy(),
            "last_seen_path 应被改回到 will_delete 下的真实位置");
        assert_eq!(row.file_state, "present");

        let dirty = dirty_data::Entity::find()
            .filter(dirty_data::Column::Reason.eq("location_path_mismatch_resolved"))
            .all(&conn)
            .await.unwrap();
        assert_eq!(dirty.len(), 1, "陈旧 last_seen_path 应留 dirty_data 记录");
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
            last_seen_path: Set(stale_path.to_string_lossy().into_owned()),
            status: Set("recycle".into()),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        };
        m.insert(&conn).await.unwrap();

        let report = scan(&conn, &identified, &will_delete, &archived).await.unwrap();
        assert_eq!(report.db_missing_files, 1);

        let row = doujinshi_file::Entity::find().one(&conn).await.unwrap().unwrap();
        assert_eq!(row.file_state, "missing");
        let dirty = dirty_data::Entity::find()
            .filter(dirty_data::Column::Reason.eq("location_path_mismatch"))
            .all(&conn)
            .await.unwrap();
        assert_eq!(dirty.len(), 1);
        assert_eq!(dirty[0].file_path, stale_path.to_string_lossy());
    }

    /// V4：扫描发现文件存在 → file_state='present'
    #[tokio::test]
    async fn scan_updates_file_state_to_present_when_file_exists() {
        let dir = tempfile::tempdir().unwrap();
        let identified = dir.path().join("identified");
        let will_delete = dir.path().join("will_delete");
        let archived = dir.path().join("archived");
        std::fs::create_dir_all(&identified).unwrap();
        std::fs::create_dir_all(&will_delete).unwrap();
        std::fs::create_dir_all(&archived).unwrap();
        let real = identified.join("g.zip");
        touch(&real);

        let conn = open_db(dir.path()).await;
        let now = chrono::Utc::now();
        let m = doujinshi_file::ActiveModel {
            title: Set("t".into()),
            filename: Set("g.zip".into()),
            hash: Set("h".into()),
            ext: Set("zip".into()),
            size_bytes: Set(0),
            last_seen_path: Set(real.to_string_lossy().into_owned()),
            status: Set("in_library".into()),
            file_state: Set("missing".into()),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        };
        m.insert(&conn).await.unwrap();

        let _ = scan(&conn, &identified, &will_delete, &archived).await.unwrap();
        let row = doujinshi_file::Entity::find().one(&conn).await.unwrap().unwrap();
        assert_eq!(row.file_state, "present");
    }

    /// 已 soft-resolve（resolved_at 已写）的脏数据行不阻挡下一次扫描写新行
    /// ——该行已记账，不会再被重用为「已存在」的过滤条件。
    #[tokio::test]
    async fn scan_ignores_resolved_dirty_rows_for_dedup_check() {
        let dir = tempfile::tempdir().unwrap();
        let identified = dir.path().join("identified");
        let will_delete = dir.path().join("will_delete");
        let archived = dir.path().join("archived");
        std::fs::create_dir_all(&identified).unwrap();
        std::fs::create_dir_all(&will_delete).unwrap();
        std::fs::create_dir_all(&archived).unwrap();
        touch(&identified.join("orphan.zip"));

        let conn = open_db(dir.path()).await;
        // 第一轮：写一条 dirty_data 行，并把 resolved_at 直接填上模拟「已软删」。
        let path_str = identified.join("orphan.zip").to_string_lossy().into_owned();
        let now = chrono::Utc::now().to_rfc3339();
        let am = dirty_data::ActiveModel {
            file_path: Set(path_str.clone()),
            file_size: Set(1),
            detected_dir: Set("identified".into()),
            reason: Set("orphan_file".into()),
            first_seen_at: Set(now),
            resolved_at: Set(Some(chrono::Utc::now().to_rfc3339())),
            ..Default::default()
        };
        am.insert(&conn).await.unwrap();

        // 把文件移走再放回来（模拟「重新入库」后被 mv）—— scanner 不该被 resolved 行
        // 阻挡、再写一条 orphan_file 脏数据。
        std::fs::remove_file(identified.join("orphan.zip")).unwrap();
        touch(&identified.join("orphan.zip"));

        let report = scan(&conn, &identified, &will_delete, &archived).await.unwrap();

        // 不应被旧 resolved 行 skip → orphan 被检测并写新 dirty_data
        assert_eq!(report.orphans, 1);
        let rows = dirty_data::Entity::find().all(&conn).await.unwrap();
        // 老的 resolved 行 + 新写的未 resolved 行 = 2 条
        assert_eq!(rows.len(), 2, "resolved 行不应阻挡新 orphan 写入");
        let unresolved: Vec<_> =
            rows.iter().filter(|r| r.resolved_at.is_none()).collect();
        assert_eq!(unresolved.len(), 1);
    }
}
