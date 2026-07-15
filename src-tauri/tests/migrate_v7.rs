//! v7 迁移测试：把封面文件 .jpg / .webp 改名 .pwb，同时把 DB 里 cover_path 字段
//! 同步更新。covers_dir 临时目录里有真实文件，验证盘上 + DB 两边一致。

use doujinshi_records::db::{self, migrations};
use sea_orm::{ConnectionTrait, EntityTrait, QueryFilter, Statement};
use sea_orm::ColumnTrait;

async fn fresh_conn() -> (tempfile::TempDir, sea_orm::DatabaseConnection) {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("data.db");
    let conn = db::connect(&db_path).await.unwrap();
    // 跑完整 init_schema_versioned（v1..v8），把 schema_version 重置到 6
    // 模拟"v6 库"，然后调用 init_schema_versioned_with_covers_dir 让 runner
    // 从 v6 触发 v7 + v8（v8 是幂等 no-op，PRAGMA 检查 + UPDATE WHERE）。
    // 这样 INSERT 用 V4 字段名（status / last_seen_path）才不会报错。
    migrations::init_schema_versioned(&conn).await.unwrap();
    (dir, conn)
}

async fn reset_to_version(conn: &sea_orm::DatabaseConnection, v: i64) {
    // fresh_conn 已经跑了 v1..v6，把 schema_version 删掉重建，确保从干净
    // 状态只标记到 v。这样后续跑 v7 时 runner 会精准触发 v7（其它 v1..v6 已
    // 落地）。
    let backend = conn.get_database_backend();
    conn.execute(Statement::from_string(
        backend.clone(),
        "DELETE FROM schema_version".to_string(),
    ))
    .await
    .unwrap();
    for i in 1..=v {
        conn.execute(Statement::from_string(
            backend.clone(),
            format!(
                "INSERT INTO schema_version(version, applied_at) VALUES ({}, '2026-01-01T00:00:00Z')",
                i
            ),
        ))
        .await
        .unwrap();
    }
}

#[tokio::test]
async fn v7_renames_jpg_files_and_updates_db_field() {
    let (dir, conn) = fresh_conn().await;
    let covers = dir.path().join("covers");
    std::fs::create_dir_all(&covers).unwrap();

    // 模拟 V1/V2 时代：真 jpg 文件
    let jpg_path = covers.join("abc.jpg");
    std::fs::write(&jpg_path, b"\xff\xd8\xff\xe0fake_jpg_bytes").unwrap();

    // 写一行 cover_path 指 "covers/abc.jpg"
    let backend = conn.get_database_backend();
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(Statement::from_string(
        backend.clone(),
        format!(
            "INSERT INTO doujinshi_file (title, filename, hash, ext, size_bytes, last_seen_path, \
             status, cover_path, created_at, updated_at) VALUES (\
             't', 't.zip', 'abc', 'zip', 0, 'doujinshi-identified/t.zip', 'in_library', \
             'covers/abc.jpg', '{}', '{}')",
            now, now
        ),
    ))
    .await
    .unwrap();

    reset_to_version(&conn, 6).await;
    migrations::init_schema_versioned_with_covers_dir(&conn, Some(&covers))
        .await
        .unwrap();

    // 盘上 .jpg 没了，.pwb 在
    assert!(!jpg_path.exists(), "原 .jpg 应被 rename 走");
    assert!(covers.join("abc.pwb").exists(), "应出现 .pwb");

    // DB 字段跟着改
    use doujinshi_records::db::entities::doujinshi_file;
    let row = doujinshi_file::Entity::find()
        .filter(doujinshi_file::Column::Hash.eq("abc"))
        .one(&conn)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.cover_path.as_deref(), Some("covers/abc.pwb"));
}

#[tokio::test]
async fn v7_renames_webp_files_too() {
    // V3+ 是 webp 字节 + .jpg 扩展，但万一有些库盘上有遗留的纯 .webp 文件，
    // 也应该一起改名 .pwb。
    let (dir, conn) = fresh_conn().await;
    let covers = dir.path().join("covers");
    std::fs::create_dir_all(&covers).unwrap();

    let webp_path = covers.join("def.webp");
    std::fs::write(&webp_path, b"RIFF....WEBPfake").unwrap();

    let backend = conn.get_database_backend();
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(Statement::from_string(
        backend,
        format!(
            "INSERT INTO doujinshi_file (title, filename, hash, ext, size_bytes, last_seen_path, \
             status, cover_path, created_at, updated_at) VALUES (\
             't', 't.zip', 'def', 'zip', 0, 'doujinshi-identified/t.zip', 'in_library', \
             'covers/def.webp', '{}', '{}')",
            now, now
        ),
    ))
    .await
    .unwrap();

    reset_to_version(&conn, 6).await;
    migrations::init_schema_versioned_with_covers_dir(&conn, Some(&covers))
        .await
        .unwrap();

    assert!(!webp_path.exists());
    assert!(covers.join("def.pwb").exists());
}

#[tokio::test]
async fn v7_leaves_pwb_files_untouched() {
    // 已经 .pwb 的不重复 rename（DB 也没字段要改）
    let (dir, conn) = fresh_conn().await;
    let covers = dir.path().join("covers");
    std::fs::create_dir_all(&covers).unwrap();

    let pwb_path = covers.join("ghi.pwb");
    std::fs::write(&pwb_path, b"already_pwb").unwrap();

    reset_to_version(&conn, 6).await;
    migrations::init_schema_versioned_with_covers_dir(&conn, Some(&covers))
        .await
        .unwrap();

    assert!(pwb_path.exists(), "已 .pwb 文件不应被改名");
    let bytes = std::fs::read(&pwb_path).unwrap();
    assert_eq!(bytes, b"already_pwb", "内容不应被改");
}

#[tokio::test]
async fn v7_is_idempotent() {
    let (dir, conn) = fresh_conn().await;
    let covers = dir.path().join("covers");
    std::fs::create_dir_all(&covers).unwrap();

    let jpg = covers.join("xyz.jpg");
    std::fs::write(&jpg, b"j").unwrap();

    reset_to_version(&conn, 6).await;
    migrations::init_schema_versioned_with_covers_dir(&conn, Some(&covers))
        .await
        .unwrap();
    // 再跑一次：.pwb 已存在，rename 会被跳过（"目标已存在则跳过"），schema_version
    // 也已经标到 7 不再重跑。
    migrations::init_schema_versioned_with_covers_dir(&conn, Some(&covers))
        .await
        .unwrap();

    assert!(!jpg.exists());
    assert!(covers.join("xyz.pwb").exists());
}