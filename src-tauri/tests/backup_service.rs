//! Integration tests for `services::backup`.
//!
//! 单元测试本应放 `#[cfg(test)] mod` 在 src/ 内，但 `cargo test --lib` 在
//! Windows 触发 STATUS_ENTRYPOINT_NOT_FOUND（DLL 路径冲突）。已知的：
//! `cargo test --test <file>` 走独立二进制可正常跑，所以把测试统一放这里。
//! 待 DLL 问题排查清楚后可迁回 src/。

use doujinshi_records::services::backup::{backup_filename, hash_db_file, BackupConfig};

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