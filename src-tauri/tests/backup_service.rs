//! Integration tests for `services::backup`.
//!
//! 单元测试本应放 `#[cfg(test)] mod` 在 src/ 内，但 `cargo test --lib` 在
//! Windows 触发 STATUS_ENTRYPOINT_NOT_FOUND（DLL 路径冲突）。已知的：
//! `cargo test --test <file>` 走独立二进制可正常跑，所以把测试统一放这里。
//! 待 DLL 问题排查清楚后可迁回 src/。

use doujinshi_records::services::backup::BackupConfig;

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