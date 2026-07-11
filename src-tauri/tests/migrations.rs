use doujinshi_records::db::{self, migrations};
use sea_orm::{ConnectionTrait, QueryResult, Statement};

async fn current_version(conn: &sea_orm::DatabaseConnection) -> i64 {
    let backend = conn.get_database_backend();
    let rows: Vec<QueryResult> = conn
        .query_all(Statement::from_string(
            backend,
            "SELECT MAX(version) AS max_v FROM schema_version".to_string(),
        ))
        .await
        .unwrap();
    rows.first()
        .and_then(|qr| qr.try_get_by::<i64, _>("max_v").ok())
        .unwrap_or(0)
}

#[tokio::test]
async fn fresh_install_lands_at_current_version() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("data.db");
    let conn = db::connect(&db_path).await.unwrap();
    migrations::init_schema_versioned(&conn).await.unwrap();
    assert_eq!(current_version(&conn).await, migrations::CURRENT_VERSION);
}

#[tokio::test]
async fn upgrade_from_v1_adds_rating_column() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("data.db");
    let conn = db::connect(&db_path).await.unwrap();

    // Manually apply only v1 (initial_schema), then bring the versioned
    // runner over the top so it picks up from v0 → v2.
    migrations::init_schema(&conn).await.unwrap();
    migrations::init_schema_versioned(&conn).await.unwrap();

    let backend = conn.get_database_backend();
    let rows = conn
        .query_all(Statement::from_string(
            backend,
            "SELECT name FROM pragma_table_info('doujinshi_file') WHERE name='rating'".to_string(),
        ))
        .await
        .unwrap();
    assert_eq!(rows.len(), 1, "rating column should exist after upgrade");
    assert_eq!(current_version(&conn).await, migrations::CURRENT_VERSION);
}

#[tokio::test]
async fn rerun_is_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("data.db");
    let conn = db::connect(&db_path).await.unwrap();
    migrations::init_schema_versioned(&conn).await.unwrap();
    migrations::init_schema_versioned(&conn).await.unwrap();
    migrations::init_schema_versioned(&conn).await.unwrap();
    assert_eq!(current_version(&conn).await, migrations::CURRENT_VERSION);
}

#[tokio::test]
async fn v3_adds_has_physical_file_and_dirty_data() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("data.db");
    let conn = db::connect(&db_path).await.unwrap();
    migrations::init_schema_versioned(&conn).await.unwrap();

    let backend = conn.get_database_backend();

    // has_physical_file 列存在
    let rows = conn
        .query_all(Statement::from_string(
            backend.clone(),
            "SELECT name FROM pragma_table_info('doujinshi_file') WHERE name='has_physical_file'".to_string(),
        ))
        .await
        .unwrap();
    assert_eq!(rows.len(), 1, "has_physical_file column should exist");

    // dirty_data 表存在
    let rows = conn
        .query_all(Statement::from_string(
            backend,
            "SELECT name FROM sqlite_master WHERE type='table' AND name='dirty_data'".to_string(),
        ))
        .await
        .unwrap();
    assert_eq!(rows.len(), 1, "dirty_data table should exist");
}

#[tokio::test]
async fn upgrade_from_v3_picks_up_v4_and_v5() {
    // 模拟 V2 数据库：init_schema 已含 V2 所有列（含 rating / auth_token）。
    // 把 schema_version 标记到 v3，再让 runner 跑 v4/v5。
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("data.db");
    let conn = db::connect(&db_path).await.unwrap();

    migrations::init_schema(&conn).await.unwrap();
    let backend = conn.get_database_backend();
    conn.execute(Statement::from_string(
        backend.clone(),
        "CREATE TABLE IF NOT EXISTS schema_version (version INTEGER PRIMARY KEY, applied_at TEXT NOT NULL)".to_string(),
    ))
    .await
    .unwrap();
    conn.execute(Statement::from_string(
        backend,
        "INSERT INTO schema_version(version, applied_at) VALUES (3, '2026-01-01T00:00:00Z')".to_string(),
    ))
    .await
    .unwrap();

    migrations::init_schema_versioned(&conn).await.unwrap();
    assert_eq!(current_version(&conn).await, migrations::CURRENT_VERSION);

    // 列加上了
    let rows = conn
        .query_all(Statement::from_string(
            conn.get_database_backend(),
            "SELECT name FROM pragma_table_info('doujinshi_file') WHERE name='has_physical_file'".to_string(),
        ))
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
}
