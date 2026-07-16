//! Integration tests for `services::backup`.
//!
//! 单元测试本应放 `#[cfg(test)] mod` 在 src/ 内，但 `cargo test --lib` 在
//! Windows 触发 STATUS_ENTRYPOINT_NOT_FOUND（DLL 路径冲突）。已知的：
//! `cargo test --test <file>` 走独立二进制可正常跑，所以把测试统一放这里。
//! 待 DLL 问题排查清楚后可迁回 src/。

use doujinshi_records::db;
use doujinshi_records::services::backup::{
    apply_pending_restore, backup_filename, clear_restore_marker, hash_db_file, read_restore_marker,
    validate_sqlite_file, write_restore_marker, BackupConfig, BackupService, BackupStorage,
    LocalFsStorage, RestorePending, SettingsHandle,
};
use sea_orm::{ActiveModelTrait, EntityTrait, Set};
use std::collections::HashMap;
use std::sync::{Arc, Mutex as StdMutex};

#[test]
fn backup_config_defaults() {
    let c = BackupConfig::default();
    assert_eq!(c.dir, ""); // 空 = 用默认目录
    assert_eq!(c.retention_count, 10);
}

#[test]
fn backup_config_serde_round_trip() {
    let c = BackupConfig { dir: "D:/backups".into(), retention_count: 5 };
    let s = serde_json::to_string(&c).unwrap();
    let back: BackupConfig = serde_json::from_str(&s).unwrap();
    assert_eq!(back.dir, "D:/backups");
    assert_eq!(back.retention_count, 5);
}

#[test]
fn backup_filename_compact_rfc3339() {
    let ts = chrono::DateTime::parse_from_rfc3339("2026-07-15T18:30:45.123Z")
        .unwrap()
        .with_timezone(&chrono::Utc);
    assert_eq!(backup_filename(ts), "data-2026-07-15T18-30-45.123Z.db");
}

#[test]
fn backup_filename_uses_utc() {
    // 不同时区都归一为 UTC 后命名——避免本地时区在不同机器上撞名
    let ts_local = chrono::DateTime::parse_from_rfc3339("2026-07-15T10:30:45-08:00")
        .unwrap()
        .with_timezone(&chrono::Utc);
    assert_eq!(backup_filename(ts_local), "data-2026-07-15T18-30-45.000Z.db");
}

#[test]
fn backup_filename_unique_within_same_second() {
    // 毫秒精度让连续两次 backup 落在不同时分秒但同秒时也能区分
    let ts1 = chrono::DateTime::parse_from_rfc3339("2026-07-15T18:30:45.001Z")
        .unwrap()
        .with_timezone(&chrono::Utc);
    let ts2 = chrono::DateTime::parse_from_rfc3339("2026-07-15T18:30:45.999Z")
        .unwrap()
        .with_timezone(&chrono::Utc);
    assert_ne!(backup_filename(ts1), backup_filename(ts2));
}

#[test]
fn hash_db_file_matches_blake3() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");
    std::fs::write(&path, b"hello world").unwrap();
    let actual = hash_db_file(&path).unwrap();
    let direct = blake3::hash(b"hello world").to_hex().to_string();
    assert_eq!(actual, direct);
}

#[test]
fn restore_marker_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let marker = dir.path().join(".restore-pending.json");
    let pending = RestorePending {
        src: "C:/backups/data-2026-07-15T18-30-45Z.db".into(),
        requested_at: "2026-07-15T18:30:50Z".into(),
    };
    write_restore_marker(&marker, &pending).unwrap();
    let back = read_restore_marker(&marker).unwrap().unwrap();
    assert_eq!(back.src, pending.src);
    assert_eq!(back.requested_at, pending.requested_at);
}

#[test]
fn restore_marker_absent_returns_none() {
    let dir = tempfile::tempdir().unwrap();
    let marker = dir.path().join(".restore-pending.json");
    assert!(read_restore_marker(&marker).unwrap().is_none());
}

#[test]
fn restore_marker_clear_removes_file() {
    let dir = tempfile::tempdir().unwrap();
    let marker = dir.path().join(".restore-pending.json");
    let pending = RestorePending { src: "x".into(), requested_at: "y".into() };
    write_restore_marker(&marker, &pending).unwrap();
    clear_restore_marker(&marker);
    assert!(!marker.exists());
    clear_restore_marker(&marker); // 二次调用也不报错
}

#[test]
fn local_fs_list_filters_only_backup_files() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("data-2026-07-14T18-30-45Z.db"), b"x").unwrap();
    // Windows mtime 精度 ~16ms；sleep 拉大差距确保排序稳定
    std::thread::sleep(std::time::Duration::from_millis(50));
    std::fs::write(dir.path().join("data-2026-07-15T18-30-45Z.db"), b"x").unwrap();
    std::fs::write(dir.path().join("foo.txt"), b"x").unwrap();
    std::fs::write(dir.path().join("data-.tmp-uuid.db"), b"x").unwrap(); // temp 文件应被过滤

    let storage = LocalFsStorage;
    let list = storage.list_snapshots(dir.path()).unwrap();
    assert_eq!(list.len(), 2);
    // mtime 倒序
    assert!(list[0].path.to_string_lossy().contains("2026-07-15"));
    assert!(list[1].path.to_string_lossy().contains("2026-07-14"));
}

#[test]
fn local_fs_delete_removes_file() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("data-2026-07-15T18-30-45Z.db");
    std::fs::write(&p, b"x").unwrap();
    LocalFsStorage.delete_snapshot(&p).unwrap();
    assert!(!p.exists());
}

#[test]
fn local_fs_delete_missing_is_ok() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("nope.db");
    LocalFsStorage.delete_snapshot(&p).unwrap(); // 不报错
}

#[derive(Clone)]
struct FakeSettings {
    inner: Arc<StdMutex<HashMap<String, String>>>,
}
impl FakeSettings {
    fn new() -> Self { Self { inner: Arc::new(StdMutex::new(HashMap::new())) } }
    fn get(&self, key: &str) -> Option<String> {
        self.inner.lock().unwrap().get(key).cloned()
    }
}
impl SettingsHandle for FakeSettings {
    fn read(
        &self,
        key: &str,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<Option<String>>> + Send + '_>,
    > {
        let key = key.to_string();
        Box::pin(async move { Ok(self.inner.lock().unwrap().get(&key).cloned()) })
    }
    fn write(
        &self,
        key: &str,
        value: &str,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<()>> + Send + '_>,
    > {
        let key = key.to_string();
        let value = value.to_string();
        Box::pin(async move {
            self.inner.lock().unwrap().insert(key, value);
            Ok(())
        })
    }
}

#[tokio::test]
async fn service_get_config_returns_defaults_when_empty() {
    let dir = tempfile::tempdir().unwrap();
    let svc = BackupService::new(
        dir.path().join("data.db"),
        dir.path().join("backups"),
        Arc::new(LocalFsStorage),
        Arc::new(FakeSettings::new()),
    );
    let cfg = svc.get_config().await.unwrap();
    assert_eq!(cfg, BackupConfig::default());
}

#[tokio::test]
async fn service_set_config_persists() {
    let dir = tempfile::tempdir().unwrap();
    let settings = FakeSettings::new();
    let svc = BackupService::new(
        dir.path().join("data.db"),
        dir.path().join("backups"),
        Arc::new(LocalFsStorage),
        Arc::new(settings.clone()),
    );
    svc.set_config(Some("D:/custom"), 5).await.unwrap();
    let cfg = svc.get_config().await.unwrap();
    assert_eq!(cfg.dir, "D:/custom");
    assert_eq!(cfg.retention_count, 5);
    assert_eq!(settings.get("backup_dir").as_deref(), Some("D:/custom"));
    assert_eq!(settings.get("backup_retention_count").as_deref(), Some("5"));
}

#[test]
fn service_resolve_backup_dir_empty_falls_back_to_default() {
    use std::path::PathBuf;
    let svc = BackupService::new(
        PathBuf::from("/tmp/data.db"),
        PathBuf::from("/tmp/resources/backups"),
        Arc::new(LocalFsStorage),
        Arc::new(FakeSettings::new()),
    );
    let cfg = BackupConfig { dir: String::new(), retention_count: 10 };
    assert_eq!(svc.resolve_backup_dir(&cfg), PathBuf::from("/tmp/resources/backups"));

    let cfg2 = BackupConfig { dir: "D:/custom".into(), retention_count: 5 };
    assert_eq!(svc.resolve_backup_dir(&cfg2), PathBuf::from("D:/custom"));
}

#[tokio::test]
async fn list_backups_returns_sorted_snapshots() {
    let dir = tempfile::tempdir().unwrap();
    let backup_dir = dir.path().join("backups");
    std::fs::create_dir_all(&backup_dir).unwrap();
    // 3 个备份 + 1 个无关文件；按写入顺序让 mtime 递增
    let f1 = backup_dir.join("data-2026-07-13T10-00-00Z.db");
    let f2 = backup_dir.join("data-2026-07-14T10-00-00Z.db");
    let f3 = backup_dir.join("data-2026-07-15T10-00-00Z.db");
    std::fs::write(&f1, b"old").unwrap();
    std::thread::sleep(std::time::Duration::from_millis(50));
    std::fs::write(&f2, b"mid").unwrap();
    std::thread::sleep(std::time::Duration::from_millis(50));
    std::fs::write(&f3, b"new").unwrap();
    std::fs::write(backup_dir.join("readme.txt"), b"x").unwrap();

    let svc = BackupService::new(
        dir.path().join("data.db"),
        backup_dir.clone(),
        Arc::new(LocalFsStorage),
        Arc::new(FakeSettings::new()),
    );
    let list = svc.list_backups().await.unwrap();
    assert_eq!(list.len(), 3);
    assert_eq!(list[0].path, f3); // 最新
    assert_eq!(list[2].path, f1); // 最旧
    for s in &list {
        assert!(s.size_bytes > 0);
    }
}

/// 测试 helper：建一个真 SQLite + 真 DbSettingsHandle 拼的 BackupService。
/// 返回 `BackupService` 和 `Arc<DatabaseConnection>`（helper 改成顶级 fn 后调用方
/// 各自 manage 连接生命周期；这里采用 owned Arc 方式）。
async fn make_svc_with_db(tmp: &std::path::Path) -> (BackupService, Arc<sea_orm::DatabaseConnection>) {
    use std::path::PathBuf;
    let conn = db::connect(&tmp.join("data.db")).await.unwrap();
    db::migrations::init_schema_versioned(&conn).await.unwrap();
    let conn_arc = Arc::new(conn);
    let svc = BackupService::new_with_db(
        PathBuf::from(tmp.join("data.db")),
        PathBuf::from(tmp.join("backups")),
        conn_arc.clone(),
    );
    (svc, conn_arc)
}

/// 在测试 DB 插 N 条最小 doujinshi 行（用于「让 DB 内容 hash 变化」的 helper）。
/// 用全局 AtomicUsize 防 hash UNIQUE 冲突。返回值是插完后总行数。
async fn insert_rows(conn: &sea_orm::DatabaseConnection, n: usize) -> usize {
    use doujinshi_records::db::entities::doujinshi_file;
    static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let now = chrono::Utc::now();
    for _ in 0..n {
        let i = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let am = doujinshi_file::ActiveModel {
            title: Set(format!("test-{i}")),
            filename: Set(format!("f{i}.zip")),
            hash: Set(format!("h{i}")),
            ext: Set("zip".into()),
            size_bytes: Set(0),
            status: Set("in_library".into()),
            file_state: Set("present".into()),
            last_seen_path: Set("/x".into()),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        };
        am.insert(conn).await.unwrap();
    }
    COUNTER.load(std::sync::atomic::Ordering::Relaxed)
}

#[tokio::test]
async fn backup_now_writes_via_vacuum_into() {
    let dir = tempfile::tempdir().unwrap();
    let (svc, conn) = make_svc_with_db(dir.path()).await;
    insert_rows(&conn, 3).await;

    let result = svc.backup_now().await.unwrap();
    assert!(result.path.exists(), "backup file should exist");
    assert!(result.size_bytes > 0);

    // 验证备份文件是有效 SQLite 且含原数据
    let backup_conn = db::connect(&result.path).await.unwrap();
    use doujinshi_records::db::entities::doujinshi_file;
    let rows = doujinshi_file::Entity::find().all(&backup_conn).await.unwrap();
    assert_eq!(rows.len(), 3);
}

#[tokio::test]
async fn backup_now_atomic_no_half_files_on_failure() {
    let dir = tempfile::tempdir().unwrap();
    let backup_dir = dir.path().join("backups");
    std::fs::create_dir_all(&backup_dir).unwrap();

    // db_path 指向一个目录而非文件——任何打开 SQLite 的尝试都会失败。
    // （早期版本用 nonexistent.db，但 db::connect 走 rwc 模式会自动建空库，
    //  不能触发失败路径）
    let svc = BackupService::new(
        dir.path().to_path_buf(),
        backup_dir.clone(),
        Arc::new(LocalFsStorage),
        Arc::new(FakeSettings::new()),
    );

    let result = svc.backup_now().await;
    assert!(result.is_err(), "以目录作 db_path 应触发失败");

    // 备份目录不应残留 .tmp-* 或半截 .db
    let entries: Vec<_> = std::fs::read_dir(&backup_dir)
        .unwrap()
        .filter_map(Result::ok)
        .collect();
    assert!(entries.is_empty(), "no half-files should remain");
}

#[tokio::test]
async fn backup_now_skips_when_content_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let (svc, _conn) = make_svc_with_db(dir.path()).await;
    let backup_dir = dir.path().join("backups");

    let r1 = svc.backup_now().await.unwrap();
    assert!(r1.skipped.is_none());

    let r2 = svc.backup_now().await.unwrap();
    assert_eq!(r2.skipped.as_deref(), Some("content unchanged"));
    assert_eq!(r2.md5, r1.md5);

    let snapshots = LocalFsStorage.list_snapshots(&backup_dir).unwrap();
    assert_eq!(snapshots.len(), 1);
}

#[tokio::test]
async fn backup_now_creates_new_when_content_changed() {
    let dir = tempfile::tempdir().unwrap();
    let (svc, conn) = make_svc_with_db(dir.path()).await;
    let backup_dir = dir.path().join("backups");

    svc.backup_now().await.unwrap();
    // 改 DB 内容（SeaORM insert）
    insert_rows(&conn, 1).await;

    let r2 = svc.backup_now().await.unwrap();
    assert!(r2.skipped.is_none());

    let snapshots = LocalFsStorage.list_snapshots(&backup_dir).unwrap();
    assert_eq!(snapshots.len(), 2);
}

#[tokio::test]
async fn backup_now_trims_to_retention_count() {
    let dir = tempfile::tempdir().unwrap();
    let (svc, conn) = make_svc_with_db(dir.path()).await;
    svc.set_config(Some(""), 2).await.unwrap();
    let backup_dir = dir.path().join("backups");

    // 4 次不同内容：每次 insert 一行让 source hash 变化
    for _ in 0..4 {
        insert_rows(&conn, 1).await;
        svc.backup_now().await.unwrap();
    }

    let snapshots = LocalFsStorage.list_snapshots(&backup_dir).unwrap();
    assert_eq!(snapshots.len(), 2, "retention=2 应只留最新 2 个");
}

#[tokio::test]
async fn backup_now_retention_zero_keeps_all() {
    let dir = tempfile::tempdir().unwrap();
    let (svc, conn) = make_svc_with_db(dir.path()).await;
    svc.set_config(Some(""), 0).await.unwrap();
    let backup_dir = dir.path().join("backups");

    for _ in 0..3 {
        insert_rows(&conn, 1).await;
        svc.backup_now().await.unwrap();
    }

    let snapshots = LocalFsStorage.list_snapshots(&backup_dir).unwrap();
    assert_eq!(snapshots.len(), 3);
}

#[tokio::test]
async fn backup_now_serializes_concurrent_callers() {
    let dir = tempfile::tempdir().unwrap();
    let (svc, _conn) = make_svc_with_db(dir.path()).await;
    let svc = Arc::new(svc);

    let s1 = svc.clone();
    let s2 = svc.clone();
    let h1 = tokio::spawn(async move { s1.backup_now().await });
    let h2 = tokio::spawn(async move { s2.backup_now().await });
    let (r1, r2) = tokio::join!(h1, h2);

    let results = vec![r1.unwrap(), r2.unwrap()];
    let oks = results.iter().filter(|r| r.is_ok()).count();
    let errs = results.iter().filter(|r| r.is_err()).count();
    assert_eq!(oks, 1, "恰好一个应成功");
    assert_eq!(errs, 1, "另一个应失败");
}

// ─── Restore staging + apply ─────────────────────────────────────────────

/// 建一个最小合法 SQLite（给 stage_restore / apply 测试用）。
/// db::connect 走 rwc 模式创空文件；必须跑一次 schema 让 SQLite 写出 magic 头。
async fn write_min_sqlite(path: &std::path::Path) {
    let conn = db::connect(path).await.unwrap();
    db::migrations::init_schema_versioned(&conn).await.unwrap();
}

#[tokio::test]
async fn validate_sqlite_accepts_real_db() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("ok.db");
    write_min_sqlite(&p).await;
    assert!(validate_sqlite_file(&p).is_ok());
}

#[test]
fn validate_sqlite_rejects_junk() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("junk.db");
    std::fs::write(&p, b"not sqlite, just text").unwrap();
    assert!(validate_sqlite_file(&p).is_err());
}

#[test]
fn validate_sqlite_rejects_missing() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("nope.db");
    assert!(validate_sqlite_file(&p).is_err());
}

#[tokio::test]
async fn stage_restore_writes_marker_for_valid_src() {
    let dir = tempfile::tempdir().unwrap();
    let _db_path = dir.path().join("data.db");
    let src = dir.path().join("snapshot.db");
    write_min_sqlite(&src).await;

    let (svc, _conn) = make_svc_with_db(dir.path()).await;
    svc.stage_restore(&src).await.unwrap();

    let marker = dir.path().join(".restore-pending.json");
    let pending = read_restore_marker(&marker).unwrap().unwrap();
    assert_eq!(pending.src, src.to_string_lossy());
    assert!(!pending.requested_at.is_empty());
}

#[tokio::test]
async fn stage_restore_rejects_non_sqlite_src() {
    let dir = tempfile::tempdir().unwrap();
    let junk = dir.path().join("junk.db");
    std::fs::write(&junk, b"not sqlite").unwrap();

    let (svc, _conn) = make_svc_with_db(dir.path()).await;
    assert!(svc.stage_restore(&junk).await.is_err());

    // marker 不该被写
    let marker = dir.path().join(".restore-pending.json");
    assert!(read_restore_marker(&marker).unwrap().is_none());
}

#[tokio::test]
async fn apply_pending_restore_no_marker_returns_none() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("data.db");
    let marker = dir.path().join(".restore-pending.json");
    std::fs::write(&db_path, b"current").unwrap();

    let result = apply_pending_restore(&db_path, &marker).await.unwrap();
    assert!(result.is_none());
    // db 未动
    assert_eq!(std::fs::read(&db_path).unwrap(), b"current");
}

#[tokio::test]
async fn apply_pending_restore_replaces_db_and_clears_marker() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("snapshot.db");
    write_min_sqlite(&src).await;
    let db_path = dir.path().join("data.db");
    std::fs::write(&db_path, b"old content").unwrap();
    let marker = dir.path().join(".restore-pending.json");
    write_restore_marker(&marker, &RestorePending {
        src: src.to_string_lossy().into(),
        requested_at: "2026-07-15T00:00:00Z".into(),
    })
    .unwrap();

    let result = apply_pending_restore(&db_path, &marker).await.unwrap();
    assert_eq!(result, Some(src.to_string_lossy().into()));

    // db 已被 src 覆盖
    assert_eq!(std::fs::read(&db_path).unwrap(), std::fs::read(&src).unwrap());
    // marker 已清
    assert!(!marker.exists());
}

#[tokio::test]
async fn apply_pending_restore_rejects_non_sqlite_src_leaves_db_intact() {
    let dir = tempfile::tempdir().unwrap();
    let junk = dir.path().join("junk.db");
    std::fs::write(&junk, b"corrupted").unwrap();
    let db_path = dir.path().join("data.db");
    std::fs::write(&db_path, b"current good").unwrap();
    let marker = dir.path().join(".restore-pending.json");
    write_restore_marker(&marker, &RestorePending {
        src: junk.to_string_lossy().into(),
        requested_at: "2026-07-15T00:00:00Z".into(),
    })
    .unwrap();

    let result = apply_pending_restore(&db_path, &marker).await;
    assert!(result.is_err(), "坏 src 应抛 Err");

    // db 未动，marker 保留供排查
    assert_eq!(std::fs::read(&db_path).unwrap(), b"current good");
    assert!(marker.exists());
}

// ─── Auto-backup threshold ───────────────────────────────────────────────

use doujinshi_records::services::backup::{read_backup_state as _rbs, write_backup_state as _wbs, BackupState};

async fn write_state_last_at(dir: &std::path::Path, last_at: chrono::DateTime<chrono::Utc>) {
    std::fs::create_dir_all(dir).unwrap();
    let mut state = BackupState::default();
    state.last_at = last_at.to_rfc3339();
    state.last_md5 = "fake".into();
    _wbs(dir, &state).await.unwrap();
}

#[tokio::test]
async fn should_auto_backup_true_when_no_history() {
    let dir = tempfile::tempdir().unwrap();
    let (svc, _conn) = make_svc_with_db(dir.path()).await;
    // sidecar 不存在 → 无历史 → 触发
    assert!(svc.should_auto_backup(24).await.unwrap());
}

#[tokio::test]
async fn should_auto_backup_false_when_recent() {
    let dir = tempfile::tempdir().unwrap();
    let (svc, _conn) = make_svc_with_db(dir.path()).await;
    let backup_dir = dir.path().join("backups");
    write_state_last_at(&backup_dir, chrono::Utc::now() - chrono::Duration::hours(1)).await;
    assert!(!svc.should_auto_backup(24).await.unwrap());
}

#[tokio::test]
async fn should_auto_backup_true_when_old() {
    let dir = tempfile::tempdir().unwrap();
    let (svc, _conn) = make_svc_with_db(dir.path()).await;
    let backup_dir = dir.path().join("backups");
    write_state_last_at(&backup_dir, chrono::Utc::now() - chrono::Duration::hours(25)).await;
    assert!(svc.should_auto_backup(24).await.unwrap());
}

#[tokio::test]
async fn should_auto_backup_treats_unparseable_last_at_as_old() {
    let dir = tempfile::tempdir().unwrap();
    let (svc, _conn) = make_svc_with_db(dir.path()).await;
    let backup_dir = dir.path().join("backups");
    std::fs::create_dir_all(&backup_dir).unwrap();
    // 手动写个破 JSON 模拟文件腐败
    std::fs::write(backup_dir.join("backup_state.json"), b"{\"last_at\":\"garbage\"}").unwrap();
    assert!(svc.should_auto_backup(24).await.unwrap());
    // 确认读出来确实是 last_at="garbage"
    let s = _rbs(&backup_dir).await.unwrap();
    assert_eq!(s.last_at, "garbage");
}

// ─── End-to-end backup → modify → restore ────────────────────────────────

#[tokio::test]
async fn end_to_end_backup_modify_restore() {
    let dir = tempfile::tempdir().unwrap();
    let (svc, conn) = make_svc_with_db(dir.path()).await;
    let db_path = dir.path().join("data.db");

    // 1. 初始备份：1 行
    insert_rows(&conn, 1).await;
    let r1 = svc.backup_now().await.unwrap();
    assert!(r1.skipped.is_none());

    // 2. 改动后再备：2 行
    insert_rows(&conn, 1).await;
    let r2 = svc.backup_now().await.unwrap();
    assert!(r2.skipped.is_none());
    assert_ne!(r1.path, r2.path);

    let snaps = svc.list_backups().await.unwrap();
    assert_eq!(snaps.len(), 2);
    // mtime 倒序：r2 是更新的
    assert_eq!(snaps[0].path, r2.path);
    let newer = r2.path.clone();

    // 3. 把 in-memory conn drop 再 stage_restore——保险起见避免文件 lock
    drop(svc);
    drop(conn);

    let svc2 = BackupService::new_with_db(
        db_path.clone(),
        dir.path().join("backups"),
        Arc::new(db::connect(&db_path).await.unwrap()),
    );
    svc2.stage_restore(&newer).await.unwrap();

    // 4. 模拟启动期 apply
    let marker = dir.path().join(".restore-pending.json");
    let result = apply_pending_restore(&db_path, &marker).await.unwrap();
    let newer_str = newer.to_string_lossy().into_owned();
    assert_eq!(result.as_deref(), Some(newer_str.as_str()));

    // 5. 内容已换成 newer 快照（2 行）
    let after = db::connect(&db_path).await.unwrap();
    use doujinshi_records::db::entities::doujinshi_file;
    let count = doujinshi_file::Entity::find().all(&after).await.unwrap().len();
    assert_eq!(count, 2, "restore 后 DB 应回到 2 行快照");

    // 6. marker 已清
    assert!(!marker.exists());
}