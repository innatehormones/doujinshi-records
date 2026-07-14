//! 5 状态机的转移核心。
//!
//! 规则：DB UPDATE + 文件移动是一笔交易，必须都成功；src 不存在直接返 Err
//! 拒绝（前端 catch 后报"文件已丢失，无法 [动作]"），绝不静默更新 `current_location`
//! 制造数据 / 文件脱钩的脏态。
//!
//! 5 个合法转移：
//!   - identified → archived (Archive)
//!   - identified → will_delete (MarkForDelete)
//!   - will_delete → identified (Restore)
//!   - archived → identified (Restore)
//!   - will_delete → permanently_deleted (PermanentlyDelete) — 回收站彻底删除
//!   - identified → permanently_deleted (PermanentlyDelete) — 冲突 replace_b 的 A 行 ghost
//! 其他转移非法，调用方应先检查状态。
//!
//! `PermanentlyDelete` 不走"rename 到目标目录"那一套，源文件存在就 best-effort
//! 删、不存在就 no-op（这是"用户意图已删除"的状态机最终态，盘上文件不存是预期
//! 而非异常）。Archive / MarkForDelete / Restore 三种走的是另一套护栏：
//! src 不存在 / dest 已有同名 → 返 Err，DB 不动。
//!
//! 历史 spec 写"src 不存在时 no-op + physically_deleted=true"是 best-effort，
//! 适用于后台扫描（scanner / dirty_scanner）但不应套用到用户主动点按钮的转移
//! ——那等于撒谎说"操作成功"。本模块专门负责用户主动转移，src 缺失一律报错
//! （除 PermanentlyDelete）；启动扫描仍由 `dirty_scanner` 维护 `has_physical_file`。

use anyhow::{anyhow, Result};
use sea_orm::{ActiveModelTrait, DatabaseConnection, EntityTrait, Set};
use std::path::{Path, PathBuf};

use crate::db::entities::doujinshi_file;

#[derive(Debug, Clone, Copy)]
pub enum TransitionKind {
    Archive,
    Restore,
    MarkForDelete,
    /// 移到 permanently_deleted（5 状态机最终态）。源文件存在就 best-effort 删、
    /// 不存在就 no-op；不走"src 不存在则拒绝"的护栏。
    PermanentlyDelete,
}

impl TransitionKind {
    fn target(&self, from: &str) -> Option<&'static str> {
        match (self, from) {
            (TransitionKind::Archive, "identified") => Some("archived"),
            (TransitionKind::Restore, "will_delete") => Some("identified"),
            (TransitionKind::Restore, "archived") => Some("identified"),
            (TransitionKind::MarkForDelete, "identified") => Some("will_delete"),
            (TransitionKind::PermanentlyDelete, "will_delete") => Some("permanently_deleted"),
            (TransitionKind::PermanentlyDelete, "identified") => Some("permanently_deleted"),
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

    let src = PathBuf::from(&row.current_path);
    let mut am: doujinshi_file::ActiveModel = row.into();

    if matches!(kind, TransitionKind::PermanentlyDelete) {
        // 最终态：best-effort 删源文件（不存在就跳过；权限 / 占用失败也跳过），
        // 然后落 permanently_deleted。`has_physical_file=false` 显式写，不
        // 等启动扫描——这一行记录从此刻起在 UI 上对用户的语义就是"已删"。
        if src.exists() {
            let _ = std::fs::remove_file(&src);
        }
        am.current_location = Set(target.into());
        am.has_physical_file = Set(false);
        am.updated_at = Set(chrono::Utc::now());
        am.update(conn).await?;
        return Ok(());
    }

    let target_dir = match target {
        "identified" => identified_dir,
        "will_delete" => will_delete_dir,
        "archived" => archived_dir,
        other => return Err(anyhow!("unknown target {}", other)),
    };

    let filename = src
        .file_name()
        .ok_or_else(|| anyhow!("invalid source path: {}", src.display()))?;
    let dest = target_dir.join(filename);

    if !src.exists() {
        // 源文件不在盘上：拒绝转移，绝不静默改 DB。
        // `has_physical_file` 由 `dirty_scanner` 启动扫描维护，不由转移路径写。
        return Err(anyhow!(
            "file {} physical file missing (expected at {})",
            id,
            src.display()
        ));
    }
    std::fs::create_dir_all(target_dir)?;
    if dest.exists() {
        // 目标位置已有同名文件：拒绝执行，让用户自己清理（删多出来的 / 改名），
        // 之后再试。跟 inbox 入库的 `conflict` 表不是一回事，那个走流程；这里
        // 只是单步拒绝，避免静默覆盖或制造"两个 DB 行指向同一盘上文件"的脏态。
        return Err(anyhow!(
            "file {} target already exists at {}",
            id,
            dest.display()
        ));
    }
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
    async fn transition_fails_when_source_file_missing() {
        let (_dir, identified, will_delete, archived) = setup_dirs().await;
        let conn = open_db(_dir.path()).await;
        let id = seed_row(&conn, "identified", "missing/f.zip").await;

        let err = transition_with_dirs(
            &conn,
            id,
            TransitionKind::Archive,
            &identified,
            &will_delete,
            &archived,
        )
        .await
        .unwrap_err();
        assert!(
            err.to_string().contains("physical file missing"),
            "err: {}",
            err
        );

        // 转移失败：DB 不动，current_location 仍为 identified。
        let row = doujinshi_file::Entity::find_by_id(id)
            .one(&conn)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.current_location, "identified");
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
    }

    #[tokio::test]
    async fn transition_fails_when_target_exists() {
        let (_dir, identified, will_delete, archived) = setup_dirs().await;
        let conn = open_db(_dir.path()).await;

        let src = identified.join("f.zip");
        std::fs::write(&src, b"data").unwrap();
        let id = seed_row(&conn, "identified", &src.to_string_lossy()).await;

        // 提前在 will_delete 放同名文件，模拟用户手动塞进去的冲突。
        std::fs::write(will_delete.join("f.zip"), b"preexisting").unwrap();

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
        assert!(
            err.to_string().contains("target already exists"),
            "err: {}",
            err
        );

        // 转移失败：DB 不动，src 应保留不动，预放文件也应原样存在。
        let row = doujinshi_file::Entity::find_by_id(id)
            .one(&conn)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.current_location, "identified");
        assert!(src.exists(), "src 应保留");
        let dst_content = std::fs::read(will_delete.join("f.zip")).unwrap();
        assert_eq!(dst_content, b"preexisting", "预放文件不应被覆盖");
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

    #[tokio::test]
    async fn transition_to_permanently_delete_removes_file_and_marks_gone() {
        let (_dir, identified, will_delete, archived) = setup_dirs().await;
        let conn = open_db(_dir.path()).await;

        let src = will_delete.join("f.zip");
        std::fs::write(&src, b"data").unwrap();
        let id = seed_row(&conn, "will_delete", &src.to_string_lossy()).await;

        transition_with_dirs(
            &conn,
            id,
            TransitionKind::PermanentlyDelete,
            &identified,
            &will_delete,
            &archived,
        )
        .await
        .unwrap();

        assert!(!src.exists(), "源文件应被 best-effort 删");
        let row = doujinshi_file::Entity::find_by_id(id)
            .one(&conn)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.current_location, "permanently_deleted");
        assert!(!row.has_physical_file);
    }

    #[tokio::test]
    async fn transition_to_permanently_delete_succeeds_when_source_missing() {
        // PermanentlyDelete 跟其它转移不同：源文件不在 = 预期状态（用户/系统
        // 已经清掉了），不应当成异常拒绝。直接落 permanently_deleted 即可。
        let (_dir, identified, will_delete, archived) = setup_dirs().await;
        let conn = open_db(_dir.path()).await;
        let id = seed_row(&conn, "will_delete", "missing/f.zip").await;

        transition_with_dirs(
            &conn,
            id,
            TransitionKind::PermanentlyDelete,
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
        assert_eq!(row.current_location, "permanently_deleted");
        assert!(!row.has_physical_file);
    }

    #[tokio::test]
    async fn transition_to_permanently_delete_from_identified_works() {
        // conflict resolve_conflict ReplaceB 的 A 行走的也是这条转移——
        // identified → permanently_deleted 同样合法。
        let (_dir, identified, will_delete, archived) = setup_dirs().await;
        let conn = open_db(_dir.path()).await;
        let src = identified.join("a.zip");
        std::fs::write(&src, b"data").unwrap();
        let id = seed_row(&conn, "identified", &src.to_string_lossy()).await;

        transition_with_dirs(
            &conn,
            id,
            TransitionKind::PermanentlyDelete,
            &identified,
            &will_delete,
            &archived,
        )
        .await
        .unwrap();

        assert!(!src.exists());
        let row = doujinshi_file::Entity::find_by_id(id)
            .one(&conn)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.current_location, "permanently_deleted");
    }
}