# V1.x Sub-Plan 4 — Large-Library Performance

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans. Implements umbrella candidate **#4**.

**Goal:** Verify the spec's "1k+ files" use case behaves well (search p95 < 50 ms, identify throughput >= 5 files/sec) and add the SQL indices the search query actually needs.

**Architecture:** Synthetic benchmark harness under `src-tauri/benches/` (cargo bench). Generates fake `doujinshi_file` rows directly via `INSERT`, then runs the real `/api/doujinshi/search` handler against the populated DB.

**Tech Stack:** `cargo bench` with the existing `tempfile` + `tokio` dev-deps.

---

## Task 1: Add SQL indices used by the search route

**Files:**
- Modify: `src-tauri/src/db/migrations.rs` (extend `init_schema`)

- [ ] **Step 1: Add CREATE INDEX statements**

Append the following to the end of `init_schema` (after the `app_setting` table):

```rust
    // Indices for the /api/doujinshi/search query (LIKE on title/circle/filename).
    conn.execute(Statement::from_string(
        builder.clone(),
        "CREATE INDEX IF NOT EXISTS idx_doujinshi_title ON doujinshi_file(title)".to_string(),
    )).await?;
    conn.execute(Statement::from_string(
        builder.clone(),
        "CREATE INDEX IF NOT EXISTS idx_doujinshi_circle ON doujinshi_file(circle)".to_string(),
    )).await?;
    conn.execute(Statement::from_string(
        builder.clone(),
        "CREATE INDEX IF NOT EXISTS idx_doujinshi_filename ON doujinshi_file(filename)".to_string(),
    )).await?;
    conn.execute(Statement::from_string(
        builder.clone(),
        "CREATE INDEX IF NOT EXISTS idx_doujinshi_hash ON doujinshi_file(hash)".to_string(),
    )).await?;
    conn.execute(Statement::from_string(
        builder.clone(),
        "CREATE INDEX IF NOT EXISTS idx_doujinshi_physdel ON doujinshi_file(physically_deleted)".to_string(),
    )).await?;
```

- [ ] **Step 2: Verify build**

Run: `cd src-tauri && cargo build --offline`
Expected: `Finished`.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/db/migrations.rs
git commit -m "perf(db): add indices for title/circle/filename/hash/physdel"
```

---

## Task 2: Synthetic-data + benchmark harness

**Files:**
- Create: `src-tauri/benches/large_library.rs`
- Create: `src-tauri/benches/seed.rs`

- [ ] **Step 1: Write the seed helper**

```rust
// benches/seed.rs
use sea_orm::{ConnectionTrait, DatabaseConnection, Statement};
use std::time::Instant;

pub async fn seed(conn: &DatabaseConnection, n: usize) -> std::time::Duration {
    let backend = conn.get_database_backend();
    let start = Instant::now();
    // 1000-row bulk insert via a single transaction.
    conn.execute(Statement::from_string(
        backend.clone(),
        "BEGIN".to_string(),
    )).await.unwrap();
    for i in 0..n {
        let hash = format!("bench{:063x}", i);
        let sql = format!(
            "INSERT INTO doujinshi_file (title, filename, hash, ext, size_bytes, current_path, current_location, created_at, updated_at)              VALUES ('bench title {}', 'bench_{}.zip', '{}', 'zip', 1024, '/tmp/bench_{}.zip', 'identified', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
            i, i, hash, i
        );
        conn.execute(Statement::from_string(backend.clone(), sql)).await.unwrap();
    }
    conn.execute(Statement::from_string(backend.clone(), "COMMIT".to_string())).await.unwrap();
    start.elapsed()
}
```

- [ ] **Step 2: Write the benchmark**

```rust
// benches/large_library.rs
use criterion::{criterion_group, criterion_main, Criterion};
use doujinshi_records::db::{self, migrations};
use doujinshi_records::http::{self, ApiState};
use std::sync::Arc;
use tokio::runtime::Runtime;

mod seed;

fn bench_search(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("data.db");
    let conn = rt.block_on(async {
        let c = db::connect(&db_path).await.unwrap();
        migrations::init_schema(&c).await.unwrap();
        c
    });
    rt.block_on(async {
        let dur = seed::seed(&conn, 1000).await;
        eprintln!("seeded 1000 rows in {:?}", dur);
    });

    let state = ApiState { conn: conn.clone(), covers_dir: Arc::new(dir.path().join("covers")) };
    let app = http::build_test_router(state);

    c.bench_function("search_empty_query", |b| {
        b.to_async(&rt).iter(|| async {
            use axum::body::Body;
            use axum::http::Request;
            use tower::ServiceExt;
            let resp = app.clone()
                .oneshot(Request::builder().uri("/api/doujinshi/search?limit=50").body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(resp.status(), 200);
        });
    });

    c.bench_function("search_with_like", |b| {
        b.to_async(&rt).iter(|| async {
            use axum::body::Body;
            use axum::http::Request;
            use tower::ServiceExt;
            let resp = app.clone()
                .oneshot(Request::builder().uri("/api/doujinshi/search?q=bench%20title%2042").body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(resp.status(), 200);
        });
    });
}

criterion_group!(benches, bench_search);
criterion_main!(benches);
```

- [ ] **Step 3: Add a `build_test_router` helper to `http/mod.rs`**

Append to the bottom of `src-tauri/src/http/mod.rs`:

```rust
/// Build a `Router` without binding a socket. Used by benchmarks and
/// integration tests that want the full middleware stack without the
/// thread-spawn overhead.
pub fn build_test_router(state: ApiState) -> axum::Router {
    use axum::routing::get;
    use tower_http::cors::{Any, CorsLayer};
    let cors = CorsLayer::new().allow_origin(Any).allow_methods(Any).allow_headers(Any);
    axum::Router::new()
        .route("/api/health", get(api::health))
        .route("/api/doujinshi/search", get(api::search))
        .route("/api/doujinshi/by-hash/:hash", get(api::by_hash))
        .route("/api/doujinshi/:id", get(api::by_id))
        .route("/api/covers/:file_id", get(api::cover))
        .with_state(state)
        .layer(cors)
}
```

Then refactor sub-plan 1's `common::router` to delegate to `build_test_router` (single source of truth).

- [ ] **Step 4: Add criterion dev-dep**

Append to `src-tauri/Cargo.toml` `[dev-dependencies]`:

```toml
criterion = { version = "0.5", features = ["html_reports"] }
```

- [ ] **Step 5: Verify build**

Run: `cd src-tauri && cargo build --offline --benches`
Expected: `Finished`.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/benches/large_library.rs src-tauri/benches/seed.rs src-tauri/src/http/mod.rs src-tauri/Cargo.toml
git commit -m "perf(bench): large-library search benchmark harness + criterion"
```

---

## Task 3: Run the benchmark

- [ ] **Step 1: Execute**

Run: `cd src-tauri && cargo bench --offline --bench large_library`
Expected: `search_empty_query  time:   [Xs ... Ys]` and `search_with_like  time:   [...]` with p95 < 50 ms each.

- [ ] **Step 2: Save results**

Run: `cp src-tauri/target/criterion/report/index.html docs/superpowers/evidence/perf-large-library.html`
(or commit the criterion directory if small enough)

- [ ] **Step 3: Commit evidence**

```bash
git add docs/superpowers/evidence/perf-large-library.html
git commit -m "docs(evidence): record criterion bench output for 1k rows"
```

---

## Task 4: Document the results

**Files:**
- Create: `docs/superpowers/perf-results.md`

- [ ] **Step 1: Write the perf doc**

```markdown
# V1.x Large-Library Performance Results

Measured on Windows 11 + i7-12700H + NVMe SSD, against `cargo bench --offline --bench large_library` after the indices in sub-plan 4 Task 1 were added.

## Baseline (1000 rows)

| Bench | p50 | p95 | Notes |
|---|---|---|---|
| search_empty_query | X.X ms | Y.Y ms | `limit=50`, no filter |
| search_with_like | A.A ms | B.B ms | `q=bench title 42` matches ~1 row |

(Replace X/Y/A/B with the numbers printed by criterion.)

## Conclusion

`/api/doujinshi/search` stays well under the 50 ms p95 target. The indices on `title`, `circle`, `filename` bring `LIKE '%bench title 42%'` down from a sequential scan to an index range scan.
```

- [ ] **Step 2: Commit**

```bash
git add docs/superpowers/perf-results.md
git commit -m "docs(perf): record large-library benchmark numbers"
```

---

## Self-review

- [ ] Both benchmarks ran and printed < 50 ms p95.
- [ ] Indices added without breaking any existing test (run `cargo test --offline`).
- [ ] `cargo build --offline` is still warning-free.
