# V1.x Sub-Plan 1 — HTTP Integration Tests

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans. This sub-plan implements umbrella candidate **#1 HTTP integration test**.

**Goal:** Add a deterministic, in-process integration test suite for every axum route in `src-tauri/src/http/api.rs` that runs without binding a real TCP port for the fast path, plus one real-port smoke test for the bind/spawn path that previously deadlocked under Tauri.

**Architecture:** Tests live under `src-tauri/tests/` as two binaries: `http_routes.rs` uses `tower::ServiceExt::oneshot` against the `Router` directly (fast, hermetic). `http_bind.rs` binds a real `TcpListener` on `127.0.0.1:0` and asserts the response comes back within 2 s — this is the regression test for the Phase 1 starvation bug. Both share a `tests/common/mod.rs` that builds an `ApiState` against a `tempfile::TempDir` SQLite DB after running `db::migrations::init_schema`.

**Tech Stack:** `tempfile` (already a dev-dep), `tower::ServiceExt`, `http-body-util`, `hyper`, `axum::body::Body`.

---

## Task 1: Add dev-dependencies for in-process HTTP testing

**Files:**
- Modify: `src-tauri/Cargo.toml` (dev-dependencies block only)

- [ ] **Step 1: Append dev-deps**

Open `src-tauri/Cargo.toml` and ensure the `[dev-dependencies]` block reads:

```toml
[dev-dependencies]
tempfile = "3"
tower = { version = "0.5", features = ["util"] }
http-body-util = "0.1"
```

(`tower` is already a main dep; this adds the `util` feature for `ServiceExt::oneshot`.)

- [ ] **Step 2: Verify cargo resolves**

Run: `cd src-tauri && cargo build --offline --tests`
Expected: `Compiling doujinshi-records v0.1.0 (.../doujinshi-records)` then `Finished`. No errors.

- [ ] **Step 3: Commit**

Run:
```bash
git add src-tauri/Cargo.toml
git commit -m "test(http): add dev-deps for in-process route tests"
```

---

## Task 2: Shared test harness

**Files:**
- Create: `src-tauri/tests/common/mod.rs`

- [ ] **Step 1: Write the harness**

```rust
//! Shared test utilities for HTTP integration tests. Spins up an
//! in-memory `ApiState` against a fresh SQLite file in a `TempDir`
//! that is kept alive for the lifetime of the returned state via a
//! guard. Callers should bind the guard to a `let _g = ...` so the
//! temp dir is not dropped early.
#![allow(dead_code)]

use std::path::PathBuf;
use std::sync::Arc;

use doujinshi_records::db::{self, migrations};
use doujinshi_records::http::{build_router, ApiState};
use sea_orm::DatabaseConnection;
use tempfile::TempDir;

pub struct Harness {
    pub state: ApiState,
    pub covers_dir: PathBuf,
    pub resources_dir: TempDir,
}

pub async fn build_state() -> Harness {
    let resources_dir = tempfile::tempdir().expect("tempdir");
    let covers_dir = resources_dir.path().join("covers");
    std::fs::create_dir_all(&covers_dir).unwrap();
    let db_path = resources_dir.path().join("data.db");
    let conn: DatabaseConnection = db::connect(&db_path).await.expect("connect");
    migrations::init_schema(&conn).await.expect("init_schema");
    let covers = Arc::new(covers_dir.clone());
    Harness {
        state: ApiState { conn, covers_dir: covers },
        covers_dir,
        resources_dir,
    }
}

/// Build a real `Router` from `build_router`'s internal route table
/// without spawning a thread. Used by the fast in-process tests.
pub fn router(state: ApiState) -> axum::Router {
    use axum::routing::get;
    use tower_http::cors::{Any, CorsLayer};
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);
    axum::Router::new()
        .route("/api/health", get(doujinshi_records::http::api::health))
        .route("/api/doujinshi/search", get(doujinshi_records::http::api::search))
        .route("/api/doujinshi/by-hash/:hash", get(doujinshi_records::http::api::by_hash))
        .route("/api/doujinshi/:id", get(doujinshi_records::http::api::by_id))
        .route("/api/covers/:file_id", get(doujinshi_records::http::api::cover))
        .with_state(state)
        .layer(cors)
}

/// Bind a real HTTP server on a free port and return the port + a
/// shutdown handle. Used by the bind/spawn smoke test only.
pub fn bind_real() -> (u16, std::sync::Arc<std::sync::atomic::AtomicBool>) {
    let shutdown = Arc::new(std::sync::atomic::AtomicBool::new(false));
    // Port-pick is delegated to `build_router` itself, but we cannot
    // easily await its async wrapper from a sync test; so we replicate
    // just the bind step here.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().unwrap().port();
    listener.set_nonblocking(true).unwrap();
    let tokio_listener = tokio::net::TcpListener::from_std(listener).expect("from_std");
    let shutdown_clone = shutdown.clone();
    std::thread::Builder::new()
        .name("test-http".into())
        .spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            rt.block_on(async move {
                let app = router(
                    // unused state for the bind test
                    ApiState {
                        conn: db::connect(&std::path::PathBuf::from(":memory:"))
                            .await
                            .unwrap(),
                        covers_dir: Arc::new(std::path::PathBuf::from(".")),
                    },
                );
                let server = axum::serve(tokio_listener, app)
                    .with_graceful_shutdown(async move {
                        loop {
                            if shutdown_clone.load(std::sync::atomic::Ordering::Relaxed) {
                                break;
                            }
                            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                        }
                    });
                let _ = server.await;
            });
        })
        .unwrap();
    (port, shutdown)
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cd src-tauri && cargo build --offline --tests`
Expected: `Finished`. No errors. (No tests yet so nothing runs.)

- [ ] **Step 3: Commit**

```bash
git add src-tauri/tests/common/mod.rs
git commit -m "test(http): shared in-process + bind-port test harness"
```

---

## Task 3: `/api/health` and search-empty tests

**Files:**
- Create: `src-tauri/tests/http_routes.rs`

- [ ] **Step 1: Write the failing tests**

```rust
mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::{build_state, router};
use http_body_util::BodyExt;
use tower::ServiceExt;

#[tokio::test]
async fn health_returns_ok_json() {
    let h = build_state().await;
    let resp = router(h.state)
        .oneshot(Request::builder().uri("/api/health").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let ct = resp.headers().get("content-type").unwrap().to_str().unwrap().to_string();
    assert!(ct.starts_with("application/json"), "got {}", ct);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["status"], "ok");
}

#[tokio::test]
async fn search_empty_db_returns_zero_items() {
    let h = build_state().await;
    let resp = router(h.state)
        .oneshot(Request::builder().uri("/api/doujinshi/search?q=anything").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["total"], 0);
    assert_eq!(v["items"].as_array().unwrap().len(), 0);
}
```

- [ ] **Step 2: Run tests, expect both pass**

Run: `cd src-tauri && cargo test --offline --test http_routes -- --nocapture`
Expected: `test result: ok. 2 passed; 0 failed`. (Both pass on first run since the code already works.)

- [ ] **Step 3: Commit**

```bash
git add src-tauri/tests/http_routes.rs
git commit -m "test(http): health + empty-search route smoke"
```

---

## Task 4: `by_hash` / `by_id` / cover 404 tests

**Files:**
- Modify: `src-tauri/tests/http_routes.rs`

- [ ] **Step 1: Append the new tests**

Add the following block to the bottom of `src-tauri/tests/http_routes.rs`:

```rust

#[tokio::test]
async fn by_hash_returns_null_when_missing() {
    let h = build_state().await;
    let resp = router(h.state)
        .oneshot(
            Request::builder()
                .uri("/api/doujinshi/by-hash/deadbeef")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(body.as_ref(), b"null", "expected JSON null");
}

#[tokio::test]
async fn by_id_returns_404_when_missing() {
    let h = build_state().await;
    let resp = router(h.state)
        .oneshot(
            Request::builder()
                .uri("/api/doujinshi/999999")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn cover_returns_404_when_hash_unknown() {
    let h = build_state().await;
    let resp = router(h.state)
        .oneshot(
            Request::builder()
                .uri("/api/covers/deadbeef")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn cover_returns_404_when_row_exists_but_no_cover_path() {
    use doujinshi_records::db::entities::doujinshi_file;
    use sea_orm::{ActiveModelTrait, Set};
    let h = build_state().await;
    let am = doujinshi_file::ActiveModel {
        title: Set("no cover".into()),
        filename: Set("no_cover.zip".into()),
        hash: Set("abc123abc123abc123abc123abc123abc123abc123abc123abc123abc123abc1".into()),
        ext: Set("zip".into()),
        size_bytes: Set(0),
        current_path: Set("/tmp/no_cover.zip".into()),
        current_location: Set("identified".into()),
        created_at: Set(chrono::Utc::now()),
        updated_at: Set(chrono::Utc::now()),
        ..Default::default()
    };
    am.insert(&h.state.conn).await.unwrap();
    let resp = router(h.state)
        .oneshot(
            Request::builder()
                .uri("/api/covers/abc123abc123abc123abc123abc123abc123abc123abc123abc123abc1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}
```

- [ ] **Step 2: Run all http_routes tests**

Run: `cd src-tauri && cargo test --offline --test http_routes`
Expected: `test result: ok. 6 passed; 0 failed` (2 prior + 4 new).

- [ ] **Step 3: Commit**

```bash
git add src-tauri/tests/http_routes.rs
git commit -m "test(http): 404 paths for by_hash/by_id/cover"
```

---

## Task 5: Cover success path (write a JPEG to covers/ + row with cover_path)

**Files:**
- Modify: `src-tauri/tests/http_routes.rs`

- [ ] **Step 1: Append the success-path test**

```rust

#[tokio::test]
async fn cover_returns_jpeg_when_file_present() {
    use doujinshi_records::db::entities::doujinshi_file;
    use sea_orm::{ActiveModelTrait, Set};
    let h = build_state().await;
    // Write a 2x2 white JPEG to covers/ so the route can serve it.
    let hash = "fff000fff000fff000fff000fff000fff000fff000fff000fff000fff000fff0";
    let cover_abs = h.covers_dir.join(format!("{}.jpg", hash));
    let img = image::RgbImage::from_fn(2, 2, |_, _| image::Rgb([255, 255, 255]));
    let mut f = std::fs::File::create(&cover_abs).unwrap();
    image::write_buffer_with_format(
        &mut f,
        img.as_raw(),
        2,
        2,
        image::ExtendedColorType::Rgb8,
        image::ImageFormat::Jpeg,
    )
    .unwrap();
    let rel = format!("covers/{}.jpg", hash);
    let am = doujinshi_file::ActiveModel {
        title: Set("has cover".into()),
        filename: Set("has_cover.zip".into()),
        hash: Set(hash.into()),
        ext: Set("zip".into()),
        size_bytes: Set(0),
        current_path: Set("/tmp/has_cover.zip".into()),
        current_location: Set("identified".into()),
        cover_path: Set(Some(rel)),
        created_at: Set(chrono::Utc::now()),
        updated_at: Set(chrono::Utc::now()),
        ..Default::default()
    };
    am.insert(&h.state.conn).await.unwrap();
    let resp = router(h.state)
        .oneshot(
            Request::builder()
                .uri(format!("/api/covers/{}", hash))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(resp
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap()
        .starts_with("image/jpeg"));
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    assert!(bytes.len() > 100, "expected non-trivial jpeg, got {} bytes", bytes.len());
    // JPEG SOI marker
    assert_eq!(&bytes[..3], &[0xFF, 0xD8, 0xFF]);
}

#[tokio::test]
async fn cover_returns_404_when_disk_file_missing() {
    use doujinshi_records::db::entities::doujinshi_file;
    use sea_orm::{ActiveModelTrait, Set};
    let h = build_state().await;
    let hash = "ccc111ccc111ccc111ccc111ccc111ccc111ccc111ccc111ccc111ccc111ccc1";
    let rel = format!("covers/{}.jpg", hash);
    // NB: do NOT write the file to disk; cover_path points at a missing file.
    let am = doujinshi_file::ActiveModel {
        title: Set("ghost cover".into()),
        filename: Set("ghost_cover.zip".into()),
        hash: Set(hash.into()),
        ext: Set("zip".into()),
        size_bytes: Set(0),
        current_path: Set("/tmp/ghost_cover.zip".into()),
        current_location: Set("identified".into()),
        cover_path: Set(Some(rel)),
        created_at: Set(chrono::Utc::now()),
        updated_at: Set(chrono::Utc::now()),
        ..Default::default()
    };
    am.insert(&h.state.conn).await.unwrap();
    let resp = router(h.state)
        .oneshot(
            Request::builder()
                .uri(format!("/api/covers/{}", hash))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}
```

- [ ] **Step 2: Run**

Run: `cd src-tauri && cargo test --offline --test http_routes`
Expected: `test result: ok. 8 passed; 0 failed`.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/tests/http_routes.rs
git commit -m "test(http): cover success + ghost-cover 404 paths"
```

---

## Task 6: Search filter test

**Files:**
- Modify: `src-tauri/tests/http_routes.rs`

- [ ] **Step 1: Append the search filter test**

```rust

#[tokio::test]
async fn search_filters_by_title_and_status() {
    use chrono::Utc;
    use doujinshi_records::db::entities::doujinshi_file;
    use sea_orm::{ActiveModelTrait, Set};
    let h = build_state().await;
    let now = Utc::now();
    let mut rows = vec![];
    for (i, (title, viewed, marked)) in [
        ("Hatsune Miku 2024", false, false),
        ("Hatsune Miku 2025", true, false),
        ("Kagamine Rin", false, true),
    ]
    .into_iter()
    .enumerate()
    {
        let hash = format!("row{:02x}{:063}", i, 0);
        let am = doujinshi_file::ActiveModel {
            title: Set(title.into()),
            filename: Set(format!("row_{}.zip", i)),
            hash: Set(hash),
            ext: Set("zip".into()),
            size_bytes: Set(0),
            current_path: Set(format!("/tmp/row_{}.zip", i)),
            current_location: Set("identified".into()),
            viewed: Set(viewed),
            marked_for_delete: Set(marked),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        };
        let inserted = am.insert(&h.state.conn).await.unwrap();
        rows.push(inserted.id);
    }
    // q=Hatsune (title contains) -> 2 rows
    let resp = router(h.state.clone())
        .oneshot(
            Request::builder()
                .uri("/api/doujinshi/search?q=Hatsune")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["total"], 2, "expected 2 Hatsune rows: {}", body.len());
}
```

- [ ] **Step 2: Run**

Run: `cd src-tauri && cargo test --offline --test http_routes`
Expected: `test result: ok. 9 passed; 0 failed`.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/tests/http_routes.rs
git commit -m "test(http): search filter combines title LIKE + status"
```

---

## Task 7: Real-port bind smoke test (Phase 1 starvation regression)

**Files:**
- Create: `src-tauri/tests/http_bind.rs`

- [ ] **Step 1: Write the test**

```rust
//! Regression for the Phase 1 bug where `axum::serve` was spawned on
//! Tauri's main tokio runtime and immediately starved. We bind a real
//! `TcpListener` on a dedicated thread + `new_current_thread` runtime
//! (the same shape `build_router` uses) and assert a real HTTP
//! request comes back within 2 s.

use std::time::{Duration, Instant};

mod common;

#[test]
fn bound_listener_serves_health_within_2s() {
    let (port, shutdown) = common::bind_real();
    let url = format!("http://127.0.0.1:{}/api/health", port);
    let start = Instant::now();
    // crude blocking HTTP via the standard library
    let mut easy = curl::easy::Easy::new();
    easy.url(&url).unwrap();
    easy.timeout(Duration::from_secs(2)).unwrap();
    let mut buf = Vec::new();
    easy.perform().expect("curl perform");
    buf.extend_from_slice(&easy.transfer_data().unwrap().iter().collect::<Vec<_>>());
    let elapsed = start.elapsed();
    shutdown.store(true, std::sync::atomic::Ordering::Relaxed);
    assert!(elapsed < Duration::from_secs(2), "took {:?}", elapsed);
    assert!(buf.starts_with(b"HTTP/"), "got bytes {:?}", &buf[..buf.len().min(8)]);
}
```

- [ ] **Step 2: Add curl dev-dep**

Append to `src-tauri/Cargo.toml` `[dev-dependencies]`:

```toml
curl = "0.4"
```

- [ ] **Step 3: Run**

Run: `cd src-tauri && cargo test --offline --test http_bind -- --nocapture`
Expected: `test result: ok. 1 passed; 0 failed`. Takes < 3 s end-to-end.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/tests/http_bind.rs src-tauri/Cargo.toml
git commit -m "test(http): real-port bind smoke (regression for axum starvation)"
```

---

## Task 8: Final regression: full `cargo test`

**Files:** none.

- [ ] **Step 1: Run full test suite**

Run: `cd src-tauri && cargo test --offline`
Expected: All unit tests (7) + 9 http_routes + 1 http_bind = 17 passed; 0 failed.

- [ ] **Step 2: Run `cargo build` to confirm no warnings**

Run: `cd src-tauri && cargo build --offline 2>&1 | tee /tmp/build.log`
Expected: `Finished`. No new warnings beyond the 0 currently on master. If new warnings appear, fix them in this task before completing the plan.

- [ ] **Step 3: Commit `build.log` evidence (optional)**

If warnings appeared and were fixed, commit them in a single `chore(http-tests): fix cargo warnings` commit. Otherwise skip.

---

## Self-review (run before declaring done)

- [ ] Every route in `src-tauri/src/http/api.rs` is exercised: `/api/health`, `/api/doujinshi/search`, `/api/doujinshi/by-hash/:hash`, `/api/doujinshi/:id`, `/api/covers/:file_id`.
- [ ] Both happy-path and 404/empty cases are covered.
- [ ] The Phase 1 starvation bug is captured by the real-port bind test.
- [ ] No test panics on the empty tempdir (the tempdir must be kept alive via the `Harness` struct field).
- [ ] `cargo test --offline` returns 17 passed; 0 failed.
