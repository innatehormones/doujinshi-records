use sea_orm::{ConnectionTrait, DatabaseConnection, Statement, QueryResult};
use anyhow::Result;

// Create all tables if they do not exist. Idempotent.
pub async fn init_schema(conn: &DatabaseConnection) -> Result<()> {
    let builder = conn.get_database_backend();

    // doujinshi_file
    conn.execute(Statement::from_string(
        builder.clone(),
        "CREATE TABLE IF NOT EXISTS doujinshi_file (
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
            physically_deleted INTEGER NOT NULL DEFAULT 0,
            viewed INTEGER NOT NULL DEFAULT 0,
            note TEXT,
            rating INTEGER,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        )".to_string(),
    )).await?;

    // filename_alias
    conn.execute(Statement::from_string(
        builder.clone(),
        "CREATE TABLE IF NOT EXISTS filename_alias (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            file_id INTEGER NOT NULL,
            alias_filename TEXT NOT NULL,
            first_seen_at TEXT NOT NULL,
            UNIQUE(file_id, alias_filename),
            FOREIGN KEY (file_id) REFERENCES doujinshi_file(id) ON DELETE CASCADE
        )".to_string(),
    )).await?;

    // conflict
    conn.execute(Statement::from_string(
        builder.clone(),
        "CREATE TABLE IF NOT EXISTS conflict (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            a_file_id INTEGER NOT NULL,
            b_file_path TEXT NOT NULL,
            b_filename TEXT NOT NULL,
            b_hash TEXT,
            reason TEXT NOT NULL,
            resolved INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL,
            FOREIGN KEY (a_file_id) REFERENCES doujinshi_file(id) ON DELETE CASCADE
        )".to_string(),
    )).await?;

    // scan_event
    conn.execute(Statement::from_string(
        builder.clone(),
        "CREATE TABLE IF NOT EXISTS scan_event (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            event_type TEXT NOT NULL,
            file_id INTEGER,
            detail TEXT,
            created_at TEXT NOT NULL,
            FOREIGN KEY (file_id) REFERENCES doujinshi_file(id) ON DELETE SET NULL
        )".to_string(),
    )).await?;

    // app_setting
    conn.execute(Statement::from_string(
        builder.clone(),
        "CREATE TABLE IF NOT EXISTS app_setting (
            key TEXT PRIMARY KEY,
            value TEXT,
            updated_at TEXT NOT NULL
        )".to_string(),
    )).await?;

    // Indices for the /api/doujinshi/search query — `LIKE '%...%'` cannot
    // use a plain B-tree index, but the planner still picks them up to
    // narrow the rowset before the predicate is evaluated. The
    // (physically_deleted, created_at) compound index supports both
    // the count(*) filter and the ORDER BY created_at DESC path.
    for idx in [
        "CREATE INDEX IF NOT EXISTS idx_doujinshi_title ON doujinshi_file(title)",
        "CREATE INDEX IF NOT EXISTS idx_doujinshi_circle ON doujinshi_file(circle)",
        "CREATE INDEX IF NOT EXISTS idx_doujinshi_filename ON doujinshi_file(filename)",
        "CREATE INDEX IF NOT EXISTS idx_doujinshi_hash ON doujinshi_file(hash)",
        "CREATE INDEX IF NOT EXISTS idx_doujinshi_physdel ON doujinshi_file(physically_deleted)",
        "CREATE INDEX IF NOT EXISTS idx_doujinshi_physdel_created ON doujinshi_file(physically_deleted, created_at)",
    ] {
        conn.execute(Statement::from_string(builder.clone(), idx.to_string()))
            .await?;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Versioned migration runner
//
// The single source of truth for which columns exist is the ordered
// `MIGRATIONS` list below. To add a V1.x change, append a new entry;
// never edit an existing one. Each entry must be idempotent because
// `init_schema_versioned` may be replayed against an already-upgraded DB.

pub const CURRENT_VERSION: i64 = 5;

/// (version, human-readable name, body of the migration SQL to apply when
/// moving from `version - 1` to `version`). Each migration must guard itself
/// (PRAGMA check, etc.) so replaying is a no-op.
const MIGRATIONS: &[(i64, &str, &str)] = &[
    (
        1,
        "initial schema",
        "", // signal: delegate to `init_schema`
    ),
    (
        2,
        "add doujinshi_file.rating",
        "ALTER TABLE doujinshi_file ADD COLUMN rating INTEGER",
    ),
    (
        3,
        "add app_setting.auth_token",
        "ALTER TABLE app_setting ADD COLUMN auth_token TEXT",
    ),
    (
        4,
        "add doujinshi_file.has_physical_file",
        "ALTER TABLE doujinshi_file ADD COLUMN has_physical_file INTEGER NOT NULL DEFAULT 1",
    ),
    (
        5,
        "create dirty_data table",
        "CREATE TABLE IF NOT EXISTS dirty_data (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            file_path TEXT NOT NULL UNIQUE,
            file_size INTEGER NOT NULL,
            detected_dir TEXT NOT NULL,
            reason TEXT NOT NULL,
            first_seen_at TEXT NOT NULL
        )",
    ),
];

pub async fn init_schema_versioned(conn: &DatabaseConnection) -> Result<()> {
    let backend = conn.get_database_backend();

    conn.execute(Statement::from_string(
        backend.clone(),
        "CREATE TABLE IF NOT EXISTS schema_version (version INTEGER PRIMARY KEY, applied_at TEXT NOT NULL)".to_string(),
    )).await?;

    let rows: Vec<QueryResult> = conn.query_all(Statement::from_string(
        backend.clone(),
        "SELECT version FROM schema_version ORDER BY version DESC LIMIT 1".to_string(),
    )).await?;
    let current: i64 = rows
        .first()
        .and_then(|qr| qr.try_get_by::<i64, _>("version").ok())
        .unwrap_or(0);

    for migration in MIGRATIONS {
        let v = migration.0;
        if v > current {
            eprintln!("applying migration v{} ({})", v, migration.1);
            apply_migration(conn, v, migration.2).await?;
            // RFC3339 has no quote/SQL-meta chars, so interpolation is safe
            // — and we generated the string ourselves (no user input).
            let now = chrono::Utc::now().to_rfc3339();
            conn.execute(Statement::from_string(
                backend.clone(),
                format!(
                    "INSERT INTO schema_version(version, applied_at) VALUES({}, '{}')",
                    v, now
                ),
            )).await?;
        }
    }
    Ok(())
}

async fn apply_migration(conn: &DatabaseConnection, version: i64, body: &str) -> Result<()> {
    if version == 1 {
        // v1 is the bootstrap — re-use the v0 init schema unchanged.
        return init_schema(conn).await;
    }
    // For vN where N >= 2, parse `ALTER TABLE <table> ADD COLUMN <col>` and
    // ask the table whether the migration has already been applied (covers
    // the case where the table was built by an older `init_schema` that
    // already had the column). Falls through to a plain execute for any
    // other SQL body.
    let backend = conn.get_database_backend();
    let trimmed = body.trim();
    if let Some(rest) = trimmed.strip_prefix("ALTER TABLE ") {
        if let Some((table, after_table)) = rest.split_once(' ') {
            if let Some(col_part) = after_table.strip_prefix("ADD COLUMN ") {
                let col = col_part.split_whitespace().next().unwrap_or("");
                let rows: Vec<QueryResult> = conn
                    .query_all(Statement::from_string(
                        backend.clone(),
                        format!(
                            "SELECT name FROM pragma_table_info('{}') WHERE name='{}'",
                            table, col
                        ),
                    ))
                    .await?;
                if rows.is_empty() {
                    conn.execute(Statement::from_string(backend, body.to_string()))
                        .await?;
                }
                return Ok(());
            }
        }
    }
    conn.execute(Statement::from_string(backend, body.to_string()))
        .await?;
    Ok(())
}
