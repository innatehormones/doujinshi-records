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

pub const CURRENT_VERSION: i64 = 7;

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
    (
        6,
        "fold physically_deleted into current_location='permanently_deleted'",
        // 1) 旧 (physically_deleted) 上的两个索引必须先删，SQLite 拒绝 DROP
        //    COLUMN 时留有引用该列的索引。
        // 2) 把现有 physically_deleted=1 的行升级到 5 状态机的 permanently_deleted
        //    —— 状态机现在能完整表达"用户已永久删除"这个意图。
        // 3) 砍掉旧列。
        // 4) 新加 (current_location, created_at) 复合索引，替代原来给
        //    search 计数 + ORDER BY created_at 用的 (physically_deleted, created_at)。
        "DROP INDEX IF EXISTS idx_doujinshi_physdel;\
         DROP INDEX IF EXISTS idx_doujinshi_physdel_created;\
         UPDATE doujinshi_file SET current_location = 'permanently_deleted', has_physical_file = 0 WHERE physically_deleted = 1;\
         ALTER TABLE doujinshi_file DROP COLUMN physically_deleted;\
         CREATE INDEX IF NOT EXISTS idx_doujinshi_location_created ON doujinshi_file(current_location, created_at)",
    ),
    (
        7,
        "rename cover files from .jpg/.webp to .pwb",
        // 占位：body 是空字符串。盘上文件 rename 需要 covers_dir 路径（属于
        // AppConfig，不该污染 db::migrations），所以 v7 由 runner 单独调用
        // apply_v7_cover_extension_rename 处理，绕过 apply_migration。
        "",
    ),
];

pub async fn init_schema_versioned(conn: &DatabaseConnection) -> Result<()> {
    init_schema_versioned_with_covers_dir(conn, None).await
}

/// `covers_dir` 仅 v7 迁移需要（要把盘上的 .jpg / .webp 改名 .pwb）。
/// 其它迁移不需要外部参数，因此调用方在不涉及 v7 时可以传 `None`。
/// 测试可以传一个临时目录走完整路径。
pub async fn init_schema_versioned_with_covers_dir(
    conn: &DatabaseConnection,
    covers_dir: Option<&std::path::Path>,
) -> Result<()> {
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
            if v == 7 {
                // covers_dir 是 None 时（单测常见情况：空库 + 临时目录里没有
                // 老格式封面文件）就跳过盘上 rename，光跑 DB 字段更新。
                if let Some(dir) = covers_dir {
                    apply_v7_cover_extension_rename(conn, dir).await?;
                }
            } else {
                apply_migration(conn, v, migration.2).await?;
            }
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

/// v7: 把所有封面文件从 .jpg / .webp 改名 .pwb，同时 UPDATE DB 字段。
///
/// 业务原因：.pwb 是项目自定义扩展（跟 preview cache 用同一个），Windows Search
/// / OneDrive / 看图软件不会把它当图片收编索引 / 自动云同步 / 抢着打开 —— 跟
/// preview cache 当初选 .pwb 是同一个理由。
///
/// 旧库两种情况并存：
/// - V1/V2 时代：封面是真正的 jpg（`encode_webp` 之前）
/// - V3+ 时代：`encode_webp` 早就产 webp 字节，但 identifier.rs 一直把扩展名写成
///   .jpg，导致磁盘上是 "webp 字节 + .jpg 后缀" 这种混合文件
///
/// v7 把这两种都归一成 .pwb（盘上文件 + DB 字段同步改）。HTTP `cover` handler 按
/// magic bytes 探测 mime，本来就不依赖扩展名，所以这次改名不影响运行时。
///
/// 顺序：先 UPDATE DB（让字段名稳定下来，万一盘上 rename 失败也不至于让 DB 指
/// 向不存在的文件路径 —— DB 更新失败会让整个迁移事务性中断）。
async fn apply_v7_cover_extension_rename(
    conn: &DatabaseConnection,
    covers_dir: &std::path::Path,
) -> Result<()> {
    let backend = conn.get_database_backend();

    // 1) DB 字段先改。replace 一次性把 .jpg / .webp 折成 .pwb。
    conn.execute(Statement::from_string(
        backend.clone(),
        "UPDATE doujinshi_file \
         SET cover_path = REPLACE(REPLACE(cover_path, '.jpg', '.pwb'), '.webp', '.pwb') \
         WHERE cover_path LIKE '%.jpg' OR cover_path LIKE '%.webp'"
            .to_string(),
    ))
    .await?;

    // 2) 盘上文件改名。盘上可能混杂 .jpg / .webp 两种扩展名（V1/V2 时代写真 jpg，
    //    V3+ 是 webp 字节 + .jpg 后缀），都重命名。
    if covers_dir.exists() {
        let entries = std::fs::read_dir(covers_dir)?;
        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let name = match path.file_name().and_then(|n| n.to_str()) {
                Some(n) => n,
                None => continue,
            };
            let new_name_opt: Option<String> = if name.ends_with(".jpg") {
                // .jpg = 4 字符，切掉尾部扩展名 + 补 .pwb
                let stem = &name[..name.len() - 4];
                Some(format!("{}.pwb", stem))
            } else if name.ends_with(".webp") {
                // .webp = 5 字符
                let stem = &name[..name.len() - 5];
                Some(format!("{}.pwb", stem))
            } else {
                None
            };
            if let Some(new_name) = new_name_opt {
                let new_path = covers_dir.join(&new_name);
                if !new_path.exists() {
                    std::fs::rename(&path, &new_path)?;
                }
                // 目标已存在就跳过：要么是已重命名（重跑迁移），要么是撞名；
                // 两种情况都不需要再动。
            }
        }
    }

    Ok(())
}
