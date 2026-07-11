use doujinshi_records::db::{self, entities::doujinshi_file, migrations};
use sea_orm::{ConnectionTrait, EntityTrait, Statement};

// V2 → V3 迁移：DB schema 是 V2 风格（含 rating + marked_for_delete +
// physically_deleted，不含 has_physical_file），且有一行 V2 风格的数据。
// 跑完 init_schema_versioned 后，行应保留 + 列 + dirty_data 表就位。

#[tokio::test]
async fn v2_upgrade_to_v3_preserves_existing_rows() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("data.db");
    let conn = db::connect(&db_path).await.unwrap();

    // 1) V2 初始 schema
    migrations::init_schema(&conn).await.unwrap();

    // 2) 用裸 SQL 插入一行 V2 风格数据（绕过 SeaORM ActiveModel，
    //    因为当前 entity 定义包含 has_physical_file 字段）。
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

    // 3) V3 启动：跑 init_schema_versioned，v1 已被 init_schema 应用，
    //    runner 接着应用 v2/v3 (no-op if already applied)、v4 (add
    //    has_physical_file)、v5 (create dirty_data)。
    migrations::init_schema_versioned(&conn).await.unwrap();

    // 4) 行还在
    let rows = doujinshi_file::Entity::find().all(&conn).await.unwrap();
    assert_eq!(rows.len(), 1, "V2 row should survive V3 upgrade");
    let row = &rows[0];
    assert_eq!(row.title, "[V2] existing");
    assert_eq!(row.current_location, "identified");
    assert!(
        row.has_physical_file,
        "V2 upgrade should default has_physical_file to true"
    );

    // 5) V3 列就位
    let cols = conn
        .query_all(Statement::from_string(
            backend.clone(),
            "SELECT name FROM pragma_table_info('doujinshi_file') WHERE name='has_physical_file'"
                .to_string(),
        ))
        .await
        .unwrap();
    assert_eq!(cols.len(), 1, "has_physical_file column should exist");

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
