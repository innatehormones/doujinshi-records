//! Integration tests for `services::backup`.
//!
//! 单元测试本应放 `#[cfg(test)] mod` 在 src/ 内，但 `cargo test --lib` 在
//! Windows 触发 STATUS_ENTRYPOINT_NOT_FOUND（DLL 路径冲突）。已知的：
//! `cargo test --test <file>` 走独立二进制可正常跑，所以把测试统一放这里。
//! 待 DLL 问题排查清楚后可迁回 src/。

use doujinshi_records::services::backup::{
    backup_filename, clear_restore_marker, hash_db_file, read_restore_marker, write_restore_marker,
    BackupConfig, BackupService, BackupStorage, LocalFsStorage, RestorePending, SettingsHandle,
};
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
    let ts = chrono::DateTime::parse_from_rfc3339("2026-07-15T18:30:45Z")
        .unwrap()
        .with_timezone(&chrono::Utc);
    assert_eq!(backup_filename(ts), "data-2026-07-15T18-30-45Z.db");
}

#[test]
fn backup_filename_uses_utc() {
    // 不同时区都归一为 UTC 后命名——避免本地时区在不同机器上撞名
    let ts_local = chrono::DateTime::parse_from_rfc3339("2026-07-15T10:30:45-08:00")
        .unwrap()
        .with_timezone(&chrono::Utc);
    assert_eq!(backup_filename(ts_local), "data-2026-07-15T18-30-45Z.db");
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