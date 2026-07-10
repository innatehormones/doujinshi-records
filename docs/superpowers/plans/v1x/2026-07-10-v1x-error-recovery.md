# V1.x Sub-Plan 2 — Error-Recovery Paths

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans. Implements umbrella candidate **#2**.

**Goal:** Make the four most common "first-launch / disk weirdness" failure modes recoverable instead of crashing the binary or returning a generic 500.

**Architecture:** Each failure mode is detected early (startup for #1 + #2, request-time for #3 + #4) and falls back to a defined behaviour. Tests live alongside the http integration tests from sub-plan 1 (sharing the `common` module).

**Tech Stack:** Same as sub-plan 1. Adds a 1x1 transparent JPEG placeholder to `resources/covers/_missing.jpg` that ships with the repo.

---

## Failure modes covered

| # | Failure | Where detected | Behaviour |
|---|---|---|---|
| 1 | `data.db` is corrupt (random bytes or wrong schema) | Startup, after `Database::connect` | Back up to `data.db.bak-<ts>`, recreate empty DB, log to stderr |
| 2 | `resources/` directory is missing | Startup, before `ensure_dirs` | Call `ensure_dirs` (already exists; verify it's called before first use) |
| 3 | Cover row exists but `<hash>.jpg` file missing on disk | `http/api.rs::cover` | Serve a built-in 1x1 transparent JPEG with `200` and `image/png` instead of `404` |
| 4 | Frontend `FileCard` cover image 404 | `FileCard.vue` `@error` | Hide `<img>`, show the "无封面" placeholder already used for `cover_url: null` |

---

## Task 1: Detect-and-backup on DB corruption

**Files:**
- Modify: `src-tauri/src/lib.rs` (after the `db::connect` call, before `db::migrations::init_schema`)
- Modify: `src-tauri/src/error.rs` (add `RecoverableDb` variant if not present)
- Create: `src-tauri/src/db/recovery.rs`

- [ ] **Step 1: Create `db/recovery.rs`**

```rust
//! Detect a corrupt SQLite file and back it up before recreating.
//! Heuristic: try `PRAGMA quick_check`; if it returns anything other
//! than `ok`, treat the file as corrupt. `sea_orm`'s
//! `Database::connect` only fails on the most egregious corruption;
//! subtle corruption surfaces as SQL syntax errors during
//! `init_schema`, so we run a tiny probe query.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};
use sea_orm::{ConnectionTrait, DatabaseConnection, Statement};

pub enum RecoveryAction {
    Noop,
    BackedUp { backup_path: std::path::PathBuf },
}

pub async fn probe_and_recover(conn: &DatabaseConnection, db_path: &Path) -> anyhow::Result<RecoveryAction> {
    let backend = conn.get_database_backend();
    let result = conn
        .execute(Statement::from_string(
            backend.clone(),
            "PRAGMA quick_check".to_string(),
        ))
        .await;
    if result.is_ok() {
        return Ok(RecoveryAction::Noop);
    }
    // Back up then signal caller to recreate.
    let ts = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let backup = db_path.with_extension(format!("db.bak-{}", ts));
    std::fs::rename(db_path, &backup)?;
    Ok(RecoveryAction::BackedUp { backup_path: backup })
}
```

- [ ] **Step 2: Wire into `lib.rs` startup**

Replace the block right after `let conn = db::connect(...)` in `src-tauri/src/lib.rs` with:

```rust
    let conn = db::connect(&cfg.db_path()).await.expect("db connect");
    match db::recovery::probe_and_recover(&conn, &cfg.db_path()).await {
        Ok(db::recovery::RecoveryAction::BackedUp { backup_path }) => {
            eprintln!("WARN: corrupt db moved to {}, recreating", backup_path.display());
            let conn = db::connect(&cfg.db_path()).await.expect("db connect (after recovery)");
            db::migrations::init_schema(&conn).await.expect("init_schema (after recovery)");
            // continue with the new conn
            return run_inner(cfg, conn).await;
        }
        Ok(db::recovery::RecoveryAction::Noop) => {}
        Err(e) => panic!("db probe failed: {:?}", e),
    }
    db::migrations::init_schema(&conn).await.expect("init_schema");
    run_inner(cfg, conn).await;
```

Then refactor: rename the existing `pub async fn run` to `pub async fn run_inner` and add a new thin `pub async fn run` that does the recovery dance.

- [ ] **Step 3: Verify build**

Run: `cd src-tauri && cargo build --offline`
Expected: `Finished`. No errors.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/db/recovery.rs src-tauri/src/lib.rs
git commit -m "feat(db): back up corrupt data.db before recreating"
```

---

## Task 2: Cover 404 -> built-in placeholder

**Files:**
- Modify: `src-tauri/src/http/api.rs` (final branch of `cover`)
- Create: `src-tauri/src/http/placeholder.rs` (returns the bytes)

- [ ] **Step 1: Add placeholder module**

```rust
//! 1x1 transparent PNG used as the cover-404 fallback. Served with
//! `image/png` so the frontend's <img> can still try to decode.

pub const PLACEHOLDER_PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A,
    0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01,
    0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x44, 0x41,
    0x54, 0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00,
    0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE,
    0x42, 0x60, 0x82,
];

pub fn placeholder_response() -> (axum::http::StatusCode, [(axum::http::HeaderName, &'static str); 1], Vec<u8>) {
    use axum::http::{header, StatusCode};
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "image/png")],
        PLACEHOLDER_PNG.to_vec(),
    )
}
```

- [ ] **Step 2: Update `cover` handler**

In `src-tauri/src/http/api.rs`, change the final fallthrough in `cover` (the one currently returning `StatusCode::NOT_FOUND, format!("cover not found: tried {}")`) to:

```rust
    crate::http::placeholder::placeholder_response().into_response()
```

Add the import `use crate::http::placeholder;` at the top.

- [ ] **Step 3: Verify build**

Run: `cd src-tauri && cargo build --offline`
Expected: `Finished`.

- [ ] **Step 4: Update existing 404 tests**

In `src-tauri/tests/http_routes.rs`, change the three `cover_*_returns_404*` tests to expect `StatusCode::OK` and `content-type: image/png` instead. The new behaviour is "serve placeholder" not "404".

- [ ] **Step 5: Run tests**

Run: `cd src-tauri && cargo test --offline --test http_routes`
Expected: 9 passed; 0 failed.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/http/placeholder.rs src-tauri/src/http/api.rs src-tauri/tests/http_routes.rs
git commit -m "feat(http): serve 1x1 PNG placeholder when cover file missing"
```

---

## Task 3: Frontend cover 404 fallback (already covered by Task 2)

The frontend already renders the "无封面" placeholder when `cover_url` is null. The 404-to-placeholder fix in Task 2 means `cover_url` is now always populated and resolves to either the real JPEG or the placeholder PNG. **No further frontend changes required for candidate #4.** Verify by inspecting `src/components/FileCard.vue` — it should render an `<img :src="...">` that hides itself when `cover_url` is `null`.

- [ ] **Step 1: Sanity-check FileCard**

Run: `Select-String -Path src/components/FileCard.vue -Pattern 'cover_url' -SimpleMatch`
Expected: at least one match where the template renders an `<img v-if="cover_url">`.

If the existing implementation already falls back to a placeholder div, **skip Task 3** and move to Task 4.

---

## Task 4: DB corruption integration test

**Files:**
- Modify: `src-tauri/tests/http_routes.rs`

- [ ] **Step 1: Add the corruption test**

```rust

#[tokio::test]
async fn probe_and_recover_moves_corrupt_db() {
    use doujinshi_records::db::recovery::{probe_and_recover, RecoveryAction};
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("data.db");
    std::fs::write(&db_path, b"this is not a sqlite file at all").unwrap();
    // Open the corrupt file via sea_orm (it may still succeed because
    // sqlite-with-mode=rwc creates the file fresh if missing; but the
    // PRAGMA quick_check will fail).
    let conn = doujinshi_records::db::connect(&db_path).await.unwrap();
    let result = probe_and_recover(&conn, &db_path).await.unwrap();
    match result {
        RecoveryAction::BackedUp { backup_path } => {
            assert!(backup_path.exists());
            assert!(!db_path.exists(), "corrupt file should be renamed");
        }
        RecoveryAction::Noop => panic!("expected BackedUp for non-sqlite bytes"),
    }
}
```

- [ ] **Step 2: Run**

Run: `cd src-tauri && cargo test --offline --test http_routes probe_and_recover`
Expected: 1 passed.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/tests/http_routes.rs
git commit -m "test(db): probe_and_recover backs up non-sqlite bytes"
```

---

## Task 5: Final regression run

- [ ] **Step 1: Full test run**

Run: `cd src-tauri && cargo test --offline`
Expected: 17 (from sub-plan 1) + 1 (this plan) = 18 passed; 0 failed.

- [ ] **Step 2: Build clean**

Run: `cd src-tauri && cargo build --offline`
Expected: `Finished`, no warnings.
