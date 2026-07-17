use doujinshi_records::db::{self, entities::doujinshi_file, migrations};
use sea_orm::{ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter, Statement};

// V2 → V3 → V6 迁移测试：DB schema 是 V2 风格（含 rating + marked_for_delete +
// physically_deleted，不含 has_physical_file），且有一行 V2 风格的数据。
// 跑完 init_schema_versioned 后，行应保留 + 列 + dirty_data 表就位 + v6 把
// physically_deleted 折进 current_location。

#[tokio::test]
async fn v2_upgrade_to_v3_preserves_existing_rows() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("data.db");
    let conn = db::connect(&db_path).await.unwrap();

    // 1) V2 初始 schema
    migrations::init_schema(&conn).await.unwrap();

    // 2) 用裸 SQL 插入一行 V2 风格数据（绕过 SeaORM ActiveModel，
    //    因为当前 entity 定义已不含 physically_deleted）。
    let backend = conn.get_database_backend();
    conn.execute(Statement::from_string(
        backend.clone(),
        "INSERT INTO doujinshi_file (
            title, filename, hash, ext, size_bytes, current_path,
            current_location, marked_for_delete, physically_deleted,
            viewed, created_at, updated_at
         ) VALUES (
            '[V2] existing', 'a.zip',
            'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
            'zip', 1024, 'doujinshi-identified/a.zip',
            'identified', 0, 0,
            0, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z'
         )".to_string(),
    ))
    .await
    .unwrap();

    // 3) 启动：跑 init_schema_versioned，v1 已被 init_schema 应用，
    //    runner 接着应用 v2/v3 (no-op if already applied)、v4 (add
    //    has_physical_file)、v5 (create dirty_data)、v6 (fold
    //    physically_deleted into current_location + drop column)、
    //    v8~v12（v7 是 cover 文件 rename 占位，无 covers_dir 时跳过盘上 IO）。
    migrations::init_schema_versioned(&conn).await.unwrap();

    // 4) 行还在
    let rows = doujinshi_file::Entity::find().all(&conn).await.unwrap();
    assert_eq!(rows.len(), 1, "V2 row should survive V3 upgrade");
    let row = &rows[0];
    assert_eq!(row.title, "[V2] existing");
    assert_eq!(row.status, "in_library");

    // 5) v6 之后 physically_deleted 列已砍
    let cols = conn
        .query_all(Statement::from_string(
            backend.clone(),
            "SELECT name FROM pragma_table_info('doujinshi_file') WHERE name='physically_deleted'"
                .to_string(),
        ))
        .await
        .unwrap();
    assert_eq!(cols.len(), 0, "physically_deleted column should be dropped after v6");

    // 5b) v11 把 has_physical_file 也砍了（V4 file_state 已覆盖其语义）
    let cols = conn
        .query_all(Statement::from_string(
            backend.clone(),
            "SELECT name FROM pragma_table_info('doujinshi_file') WHERE name='has_physical_file'"
                .to_string(),
        ))
        .await
        .unwrap();
    assert_eq!(cols.len(), 0, "has_physical_file column should be dropped after v11");

    // 6) dirty_data 表就位
    let tbls = conn
        .query_all(Statement::from_string(
            backend,
            "SELECT name FROM sqlite_master WHERE type='table' AND name='dirty_data'"
                .to_string(),
        ))
        .await
        .unwrap();
    assert_eq!(tbls.len(), 1, "dirty_data table should exist");
}

/// V3 时代（v5 schema）但行里 physically_deleted=1 的库升到 v6：
/// 应被改成 current_location='permanently_deleted'，列被砍。
#[tokio::test]
async fn v3_physically_deleted_rows_migrate_to_permanently_deleted() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("data.db");
    let conn = db::connect(&db_path).await.unwrap();

    // 1) 模拟一个 v5 schema 的库：v1 落表（含 v5 之前的所有列，包括
    //    physically_deleted），然后 schema_version 标到 5，让后续 runner 只
    //    跑 v6~v12。直接 init_schema_versioned 不会因 v11 DROP COLUMN
    //    has_physical_file 失败——apply_migration 走 pragma_table_info 幂等
    //    检查；如果 v11 时列还在就 drop（v5 库显然还在）。
    migrations::init_schema(&conn).await.unwrap();
    let backend = conn.get_database_backend();
    conn.execute(Statement::from_string(
        backend.clone(),
        "CREATE TABLE IF NOT EXISTS schema_version (version INTEGER PRIMARY KEY, applied_at TEXT NOT NULL)".to_string(),
    ))
    .await
    .unwrap();
    // v4 才会加 has_physical_file —— 直接 ALTER 补上，让 INSERT 语法保持 v5 形态。
    conn.execute(Statement::from_string(
        backend.clone(),
        "ALTER TABLE doujinshi_file ADD COLUMN has_physical_file INTEGER NOT NULL DEFAULT 1".to_string(),
    ))
    .await
    .unwrap();
    for i in 1..=5 {
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

    // 2) 直接 SQL 塞两行：一行 physically_deleted=0（普通行）、一行
    //    physically_deleted=1（升 v6 前是"已物理删除"语义）。
    let mkrow = |title: &str, pd: i64| {
        let stmt = format!(
            "INSERT INTO doujinshi_file (title, filename, hash, ext, size_bytes, current_path, \
             current_location, marked_for_delete, physically_deleted, has_physical_file, viewed, \
             created_at, updated_at) VALUES (\
             '{}', 'a.zip', '{}', 'zip', 0, 'doujinshi-identified/a.zip', 'identified', 0, {}, 1, 0, \
             '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
            title,
            title, // hash 用 title 当 dummy
            pd,
        );
        Statement::from_string(backend.clone(), stmt)
    };
    conn.execute(mkrow("alive", 0)).await.unwrap();
    conn.execute(mkrow("dead", 1)).await.unwrap();

    // 3) 跑 v6~v12
    migrations::init_schema_versioned(&conn).await.unwrap();

    // 4) alive 仍 in_library
    let alive = doujinshi_file::Entity::find()
        .filter(doujinshi_file::Column::Title.eq("alive"))
        .one(&conn)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(alive.status, "in_library");

    // 5) dead 已落 deleted（v6 把 permanently_deleted 重命名为 deleted，v8 完成）
    let dead = doujinshi_file::Entity::find()
        .filter(doujinshi_file::Column::Title.eq("dead"))
        .one(&conn)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(dead.status, "deleted");
    // v6 把 has_physical_file=0，v8 把 has_physical_file=0 → file_state='missing'，
    // v11 砍 has_physical_file 列——最终语义落在 file_state。
    assert_eq!(dead.file_state, "missing");
}

#[tokio::test]
async fn fresh_v3_install_reaches_current_version() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("data.db");
    let conn = db::connect(&db_path).await.unwrap();
    migrations::init_schema_versioned(&conn).await.unwrap();

    let backend = conn.get_database_backend();
    let rows = conn
        .query_all(Statement::from_string(
            backend,
            "SELECT MAX(version) AS max_v FROM schema_version".to_string(),
        ))
        .await
        .unwrap();
    let max_v: i64 = rows
        .first()
        .and_then(|qr| qr.try_get_by::<i64, _>("max_v").ok())
        .unwrap_or(0);
    assert_eq!(max_v, migrations::CURRENT_VERSION);
}
