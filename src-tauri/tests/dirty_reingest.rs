//! Tests for `commands::dirty::reingest_dirty_entry_inner`.
//!
//! 走 inner 路径 —— 不绕 AppState，避免拉 Tauri runtime。

mod common;

use doujinshi_records::commands::dirty::reingest_dirty_entry_inner;
use doujinshi_records::db::{
    self,
    entities::{dirty_data, doujinshi_file},
};
use sea_orm::{ActiveModelTrait, DatabaseConnection, EntityTrait, Set};
use std::io::Write;

struct TestEnv {
    conn: DatabaseConnection,
    covers_dir: std::path::PathBuf,
    identified_dir: std::path::PathBuf,
    _resources_dir: tempfile::TempDir,
}

async fn make_env() -> TestEnv {
    let resources_dir = tempfile::tempdir().unwrap();
    let covers_dir = resources_dir.path().join("covers");
    let identified_dir = resources_dir.path().join("identified");
    std::fs::create_dir_all(&covers_dir).unwrap();
    std::fs::create_dir_all(&identified_dir).unwrap();
    let db_path = resources_dir.path().join("data.db");
    let conn = db::connect(&db_path).await.expect("connect");
    db::migrations::init_schema_versioned(&conn).await.expect("init");
    TestEnv { conn, covers_dir, identified_dir, _resources_dir: resources_dir }
}

/// 建一个最小合法 zip，写到 identified_dir/filename.zip。注意：filename
/// 已经含 .zip 后缀会让 identifier 内部 stem/suffix 处理叠 ext，最终盘上文件名变
/// "filename.zip.zip"。我们用无 ext 的内部名（让 identifier 自己拼 zip），避免
/// 这个 corner case 干扰断言。
fn write_orphan_zip(dir: &std::path::Path, stem_no_ext: &str) -> std::path::PathBuf {
    // 1x1 透明 png —— webp encoder 不挑内容，只要认得到一种 image magic 即可抽。
    let png: &[u8] = &[
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f,
        0x15, 0xc4, 0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0x00,
        0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0d, 0x0a, 0x2d, 0xb4, 0x00, 0x00, 0x00, 0x00, 0x49,
        0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
    ];
    let path = dir.join(format!("{stem_no_ext}.zip"));
    let f = std::fs::File::create(&path).unwrap();
    let mut zw = zip::ZipWriter::new(f);
    let opts = zip::write::SimpleFileOptions::default();
    zw.start_file("cover.png", opts).unwrap();
    zw.write_all(png).unwrap();
    zw.start_file("dummy.txt", opts).unwrap();
    zw.write_all(b"x").unwrap();
    zw.finish().unwrap();
    path
}

/// 在 DB 写一条 dirty_data 行（orphan_file），并返回 id。
async fn insert_orphan_row(
    conn: &DatabaseConnection,
    file_path: String,
    detected_dir: &str,
) -> i64 {
    let am = dirty_data::ActiveModel {
        file_path: Set(file_path),
        file_size: Set(1),
        detected_dir: Set(detected_dir.into()),
        reason: Set("orphan_file".into()),
        first_seen_at: Set(chrono::Utc::now().to_rfc3339()),
        ..Default::default()
    };
    let m = am.insert(conn).await.unwrap();
    m.id
}

/// 正常路径：identified/ 里的孤儿文件 → 调 reingest → 软删 dirty_data 行 +
/// 新 doujinshi 行 + 封面 + identified/<原名>.zip 自留（文件已在 identified/，
/// 触发 identifier 的 self-rename 分支，跳过 fs::rename）。
#[tokio::test]
async fn reingest_creates_doujinshi_row_and_resolves() {
    let env = make_env().await;
    let zip_path = write_orphan_zip(&env.identified_dir, "[circle] title");
    let id = insert_orphan_row(
        &env.conn,
        zip_path.to_string_lossy().into_owned(),
        "identified",
    )
    .await;

    let res = reingest_dirty_entry_inner(&env.conn, &env.covers_dir, &env.identified_dir, id, false)
        .await;
    res.unwrap();

    // dirty_data 行软删
    let row = dirty_data::Entity::find_by_id(id)
        .one(&env.conn)
        .await
        .unwrap()
        .unwrap();
    assert!(row.resolved_at.is_some(), "resolved_at 必须被写入");

    // 新 doujinshi 行
    let rows = doujinshi_file::Entity::find().all(&env.conn).await.unwrap();
    assert_eq!(rows.len(), 1, "新入库应产 1 行");
    let new_row = &rows[0];
    assert_eq!(new_row.title, "title");
    assert_eq!(new_row.circle.as_deref(), Some("circle"));
    assert_eq!(new_row.status, "in_library");
    assert_eq!(new_row.file_state, "present");
    let new_row = &rows[0];
    assert_eq!(new_row.title, "title");
    assert_eq!(new_row.circle.as_deref(), Some("circle"));
    assert_eq!(new_row.status, "in_library");
    assert_eq!(new_row.file_state, "present");

    // identified/ 应留 1 个同名前缀的 zip（rename 后名字可能改成 circle title.zip）
    let entries: Vec<_> = std::fs::read_dir(&env.identified_dir)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .ends_with(".zip")
        })
        .collect();
    assert_eq!(entries.len(), 1, "identified/ 应剩 1 个 zip");
}

/// reason 非 orphan_file → 拒绝，dirty_data 行不动。
#[tokio::test]
async fn reingest_rejects_non_orphan_reason() {
    let env = make_env().await;
    // 写一条非 orphan 的脏数据
    let am = dirty_data::ActiveModel {
        file_path: Set(format!("{}", env.identified_dir.join("missing.zip").display())),
        file_size: Set(0),
        detected_dir: Set("identified".into()),
        reason: Set("db_row_file_missing".into()),
        first_seen_at: Set(chrono::Utc::now().to_rfc3339()),
        ..Default::default()
    };
    let m = am.insert(&env.conn).await.unwrap();

    let err = reingest_dirty_entry_inner(
        &env.conn,
        &env.covers_dir,
        &env.identified_dir,
        m.id,
        false,
    )
    .await
    .unwrap_err();
    assert!(format!("{err:?}").contains("only orphan_file"));

    let still = dirty_data::Entity::find_by_id(m.id)
        .one(&env.conn)
        .await
        .unwrap()
        .unwrap();
    assert!(still.resolved_at.is_none(), "失败的请求不应写 resolved_at");
}

/// dirty_data 指向的文件已被删 → 拒绝。
#[tokio::test]
async fn reingest_rejects_missing_file() {
    let env = make_env().await;
    let am = dirty_data::ActiveModel {
        file_path: Set(format!("{}", env.identified_dir.join("ghost.zip").display())),
        file_size: Set(0),
        detected_dir: Set("identified".into()),
        reason: Set("orphan_file".into()),
        first_seen_at: Set(chrono::Utc::now().to_rfc3339()),
        ..Default::default()
    };
    let m = am.insert(&env.conn).await.unwrap();

    let err = reingest_dirty_entry_inner(
        &env.conn,
        &env.covers_dir,
        &env.identified_dir,
        m.id,
        false,
    )
    .await
    .unwrap_err();
    assert!(format!("{err:?}").contains("no longer on disk"));
}
