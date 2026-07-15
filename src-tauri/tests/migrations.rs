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
async fn v8_renames_current_location_to_status_and_adds_file_state() {
    // 模拟 V6/V7 数据库（v6 后没有 physically_deleted 列；v7 没改 doujinshi_file 字段，
    // 只动 cover_path）。手写建表 + 插入 v7-era rows，标 schema_version=7，
    // 让 init_schema_versioned 精准触发 v8。
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("data.db");
    let conn = db::connect(&db_path).await.unwrap();

    let backend = conn.get_database_backend();

    // v7-era 表：current_location + current_path + has_physical_file
    conn.execute(Statement::from_string(
        backend.clone(),
        r#"CREATE TABLE doujinshi_file (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            title TEXT NOT NULL,
            filename TEXT NOT NULL,
            hash TEXT NOT NULL UNIQUE,
            ext TEXT NOT NULL,
            size_bytes INTEGER NOT NULL,
            circle TEXT,
            series TEXT,
            translator TEXT,
            version_tag TEXT,
            current_path TEXT NOT NULL,
            current_location TEXT NOT NULL,
            cover_path TEXT,
            marked_for_delete INTEGER NOT NULL DEFAULT 0,
            has_physical_file INTEGER NOT NULL DEFAULT 1,
            viewed INTEGER NOT NULL DEFAULT 0,
            note TEXT,
            rating INTEGER,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        )"#
        .to_string(),
    ))
    .await
    .unwrap();

    // schema_version 表 + 标到 7
    conn.execute(Statement::from_string(
        backend.clone(),
        "CREATE TABLE IF NOT EXISTS schema_version (version INTEGER PRIMARY KEY, applied_at TEXT NOT NULL)".to_string(),
    ))
    .await
    .unwrap();
    for i in 1..=7 {
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

    // 插入三行：覆盖 status 取值 + has_physical_file 三态
    let now = "2026-01-01T00:00:00Z";
    conn.execute(Statement::from_string(
        backend.clone(),
        format!(
            "INSERT INTO doujinshi_file (title, filename, hash, ext, size_bytes, current_path, \
             current_location, has_physical_file, created_at, updated_at) VALUES \
             ('t1','f1.zip','h1','zip',0,'/p/1','identified',1,'{now}','{now}'),\
             ('t2','f2.zip','h2','zip',0,'/p/2','permanently_deleted',0,'{now}','{now}'),\
             ('t3','f3.zip','h3','zip',0,'/p/3','archived',0,'{now}','{now}')"
        ),
    ))
    .await
    .unwrap();

    // 触发 v8
    migrations::init_schema_versioned(&conn).await.unwrap();

    // 1. schema_version 升到 8
    assert_eq!(current_version(&conn).await, 8);
    assert_eq!(current_version(&conn).await, migrations::CURRENT_VERSION);

    // 2. 字段重命名：current_location → status, current_path → last_seen_path
    //    file_state 新列存在
    let cols: Vec<String> = conn
        .query_all(Statement::from_string(
            backend.clone(),
            "SELECT name FROM pragma_table_info('doujinshi_file')".to_string(),
        ))
        .await
        .unwrap()
        .into_iter()
        .map(|r| r.try_get_by::<String>("name").unwrap_or_default())
        .collect();
    assert!(cols.iter().any(|n| n == "status"), "status 列应存在");
    assert!(cols.iter().any(|n| n == "last_seen_path"), "last_seen_path 列应存在");
    assert!(cols.iter().any(|n| n == "file_state"), "file_state 列应存在");
    assert!(!cols.iter().any(|n| n == "current_location"), "current_location 应被 rename 走");
    assert!(!cols.iter().any(|n| n == "current_path"), "current_path 应被 rename 走");

    // 3. 数据迁移：permanently_deleted → deleted, has_physical_file=0 → file_state='missing'
    let rows: Vec<sea_orm::QueryResult> = conn
        .query_all(Statement::from_string(
            backend.clone(),
            "SELECT id, status, file_state, last_seen_path FROM doujinshi_file ORDER BY id".to_string(),
        ))
        .await
        .unwrap();
    // t1: identified + present（has_physical_file=1 → file_state='present'）
    assert_eq!(rows[0].try_get_by::<String>("status").unwrap(), "identified");
    assert_eq!(rows[0].try_get_by::<String>("file_state").unwrap(), "present");
    assert_eq!(rows[0].try_get_by::<String>("last_seen_path").unwrap(), "/p/1");
    // t2: permanently_deleted → deleted, has_physical_file=0 → file_state='missing'
    assert_eq!(rows[1].try_get_by::<String>("status").unwrap(), "deleted");
    assert_eq!(rows[1].try_get_by::<String>("file_state").unwrap(), "missing");
    // t3: archived + missing
    assert_eq!(rows[2].try_get_by::<String>("status").unwrap(), "archived");
    assert_eq!(rows[2].try_get_by::<String>("file_state").unwrap(), "missing");
}

#[tokio::test]
async fn v8_is_idempotent() {
    // v8 跑两遍不应报错（pragma 检查 + UPDATE WHERE 天然幂等）
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("data.db");
    let conn = db::connect(&db_path).await.unwrap();

    let backend = conn.get_database_backend();
    conn.execute(Statement::from_string(
        backend.clone(),
        r#"CREATE TABLE doujinshi_file (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            title TEXT NOT NULL,
            filename TEXT NOT NULL,
            hash TEXT NOT NULL UNIQUE,
            ext TEXT NOT NULL,
            size_bytes INTEGER NOT NULL,
            circle TEXT,
            series TEXT,
            translator TEXT,
            version_tag TEXT,
            current_path TEXT NOT NULL,
            current_location TEXT NOT NULL,
            cover_path TEXT,
            marked_for_delete INTEGER NOT NULL DEFAULT 0,
            has_physical_file INTEGER NOT NULL DEFAULT 1,
            viewed INTEGER NOT NULL DEFAULT 0,
            note TEXT,
            rating INTEGER,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        )"#
        .to_string(),
    ))
    .await
    .unwrap();

    conn.execute(Statement::from_string(
        backend.clone(),
        "CREATE TABLE IF NOT EXISTS schema_version (version INTEGER PRIMARY KEY, applied_at TEXT NOT NULL)".to_string(),
    ))
    .await
    .unwrap();
    for i in 1..=7 {
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

    migrations::init_schema_versioned(&conn).await.unwrap();
    migrations::init_schema_versioned(&conn).await.unwrap();
    assert_eq!(current_version(&conn).await, migrations::CURRENT_VERSION);
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
