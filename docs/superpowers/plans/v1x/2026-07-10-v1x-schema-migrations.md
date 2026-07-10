# V1.x Sub-Plan 5 — Versioned Schema Migrations

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans. Implements umbrella candidate **#5**.

**Goal:** Replace the idempotent `init_schema` (which always creates the same tables) with a versioned migration system that records the current schema version and applies forward migrations in order.

**Architecture:** A new `schema_version` table holds a single row with the current version. `init_schema` is split into `ensure_version_table` + a sequence of `vN` migration functions that run in order. New columns are introduced by `ALTER TABLE` wrapped in `IF NOT EXISTS` semantics (SQLite has no native `ADD COLUMN IF NOT EXISTS`; emulate by querying `PRAGMA table_info`).

**Tech Stack:** Same as today (sea-orm + manual SQL via `Statement`).

---

## Task 1: Add the version table

**Files:**
- Modify: `src-tauri/src/db/migrations.rs`

- [ ] **Step 1: Append a versioned migration skeleton**

Add the following to the bottom of `src-tauri/src/db/migrations.rs`:

```rust
//! Versioned schema migrations. The single source of truth for what
//! columns exist is the ordered list of `migrations` below. To add a
//! V1.x change, append a new closure; never edit an existing one.

use sea_orm::{ConnectionTrait, DatabaseConnection, Statement};

pub const CURRENT_VERSION: i64 = 2;

pub async fn init_schema_versioned(conn: &DatabaseConnection) -> Result<()> {
    let backend = conn.get_database_backend();

    // 1. Ensure version table exists.
    conn.execute(Statement::from_string(
        backend.clone(),
        "CREATE TABLE IF NOT EXISTS schema_version (version INTEGER PRIMARY KEY, applied_at TEXT NOT NULL)".to_string(),
    )).await?;

    // 2. Read current version.
    let rows: Vec<(i64,)> = conn.query_all(Statement::from_string(
        backend.clone(),
        "SELECT version FROM schema_version ORDER BY version DESC LIMIT 1".to_string(),
    )).await?;
    let current = rows.first().map(|(v,)| *v).unwrap_or(0);

    // 3. Apply migrations in order.
    let migrations: Vec<(i64, &str, Box<dyn Fn(&DatabaseConnection) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send>> + Send + Sync>)> = vec![
        (1, "initial schema", Box::new(|c| Box::pin(async move { initial_schema(c).await }))),
        (2, "add doujinshi_file.rating", Box::new(|c| Box::pin(async move { add_rating(c).await }))),
    ];

    for (v, name, f) in migrations {
        if v > current {
            eprintln!("applying migration v{} ({})", v, name);
            f(conn).await?;
            conn.execute(Statement::from_string(
                backend.clone(),
                format!("INSERT INTO schema_version(version, applied_at) VALUES({}, '{}')", v, chrono::Utc::now()),
            )).await?;
        }
    }

    Ok(())
}

async fn initial_schema(conn: &DatabaseConnection) -> Result<()> {
    init_schema(conn).await
}

async fn add_rating(conn: &DatabaseConnection) -> Result<()> {
    // Skip if column already present (covers the case where init_schema
    // was run before this migration was introduced).
    let backend = conn.get_database_backend();
    let rows: Vec<(String,)> = conn.query_all(Statement::from_string(
        backend.clone(),
        "SELECT name FROM pragma_table_info('doujinshi_file') WHERE name='rating'".to_string(),
    )).await?;
    if rows.is_empty() {
        conn.execute(Statement::from_string(
            backend.clone(),
            "ALTER TABLE doujinshi_file ADD COLUMN rating INTEGER".to_string(),
        )).await?;
    }
    Ok(())
}
```

- [ ] **Step 2: Update lib.rs to call the new function**

In `src-tauri/src/lib.rs`, change every `db::migrations::init_schema(...)` call to `db::migrations::init_schema_versioned(...)`.

- [ ] **Step 3: Verify build**

Run: `cd src-tauri && cargo build --offline`
Expected: `Finished`.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/db/migrations.rs src-tauri/src/lib.rs
git commit -m "feat(db): versioned schema migrations (v1 -> v2)"
```

---

## Task 2: Fresh-install + upgrade integration tests

**Files:**
- Create: `src-tauri/tests/migrations.rs`

- [ ] **Step 1: Write the tests**

```rust
use doujinshi_records::db::{self, migrations};
use sea_orm::{ConnectionTrait, Statement};

async fn current_version(conn: &sea_orm::DatabaseConnection) -> i64 {
    let backend = conn.get_database_backend();
    let rows: Vec<(i64,)> = conn
        .query_all(Statement::from_string(
            backend,
            "SELECT MAX(version) FROM schema_version".to_string(),
        ))
        .await
        .unwrap();
    rows.first().map(|(v,)| *v).unwrap_or(0)
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

    // Manually apply only v1 (initial_schema).
    migrations::init_schema(&conn).await.unwrap();
    migrations::init_schema_versioned(&conn).await.unwrap();

    // v2 should have added the column.
    let backend = conn.get_database_backend();
    let rows: Vec<(String,)> = conn
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
```

- [ ] **Step 2: Run**

Run: `cd src-tauri && cargo test --offline --test migrations`
Expected: 3 passed; 0 failed.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/tests/migrations.rs
git commit -m "test(db): migration version fresh-install + upgrade + idempotent"
```

---

## Task 3: Update the doujinshi_file entity to expose `rating`

**Files:**
- Modify: `src-tauri/src/db/entities/doujinshi_file.rs`

- [ ] **Step 1: Add `rating` field**

In the `Model` struct, append after `note: Option<String>`:
```rust
    pub rating: Option<i32>,
```

In the `Column` enum, append:
```rust
    Rating,
```

In the `Relation` enum nothing changes.

In the `PrimaryKey` enum nothing changes (id is still the PK).

In the implementation block (`impl Column`) ensure `Rating` is mapped via sea-orm's derive macro; since the entity uses `DeriveEntityModel`, re-running `cargo build --offline` will pick it up automatically.

- [ ] **Step 2: Verify build**

Run: `cd src-tauri && cargo build --offline`
Expected: `Finished`.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/db/entities/doujinshi_file.rs
git commit -m "feat(db): expose doujinshi_file.rating column"
```

---

## Task 4: Full regression

- [ ] **Step 1: Run everything**

Run: `cd src-tauri && cargo test --offline`
Expected: 18 (from sub-plans 1 + 2) + 3 (this plan) = 21 passed; 0 failed.

- [ ] **Step 2: Build clean**

Run: `cd src-tauri && cargo build --offline`
Expected: `Finished`, no warnings.

---

## Self-review

- [ ] `schema_version` table has one row per applied migration, in order.
- [ ] `ALTER TABLE ADD COLUMN` is gated on `pragma_table_info` so the migration can be replayed against an already-upgraded DB.
- [ ] `CURRENT_VERSION` is the only place the highest schema number is hardcoded.
- [ ] Existing data survives: an upgrade test took a v1 DB and added the column without losing rows.
