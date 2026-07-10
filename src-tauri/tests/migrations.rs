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
