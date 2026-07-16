//! Tests for `commands::settings::open_path`.
//!
//! 路径不存在 case 验证错误返回；happy path 在 Windows 真去 spawn
//! explorer.exe / 非 Windows 走 xdg-open / open——只验「不返错」，
//! 不验文件管理器真的弹起来。

use doujinshi_records::commands::settings::open_path;

#[tokio::test]
async fn open_path_rejects_missing_path() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("does_not_exist");
    let err = open_path(missing.to_string_lossy().into_owned())
        .await
        .unwrap_err();
    let msg = format!("{err:?}");
    assert!(
        msg.contains("路径不存在"),
        "expected 路径不存在 error, got: {msg}"
    );
}

#[tokio::test]
async fn open_path_accepts_existing_directory() {
    let dir = tempfile::tempdir().unwrap();
    open_path(dir.path().to_string_lossy().into_owned())
        .await
        .unwrap();
}