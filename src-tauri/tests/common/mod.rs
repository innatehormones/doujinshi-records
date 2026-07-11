//! Shared test utilities for HTTP integration tests. Spins up an
//! in-memory `ApiState` against a fresh SQLite file in a `TempDir`
//! that is kept alive for the lifetime of the returned state via a
//! guard. Callers should bind the guard to a `let _g = ...` so the
//! temp dir is not dropped early.
#![allow(dead_code)]

use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use axum::body::Body;
use axum::http::Request;
use doujinshi_records::db::{self, migrations};
use doujinshi_records::http::{ApiState, build_test_router};
use sea_orm::DatabaseConnection;
use tempfile::TempDir;

pub const TEST_TOKEN: &str = "test-token";

pub struct Harness {
    pub state: ApiState,
    pub covers_dir: PathBuf,
    pub resources_dir: TempDir,
}

pub async fn build_state() -> Harness {
    build_state_with_token(TEST_TOKEN).await
}

pub async fn build_state_with_token(token: &str) -> Harness {
    let resources_dir = tempfile::tempdir().expect("tempdir");
    let covers_dir = resources_dir.path().join("covers");
    std::fs::create_dir_all(&covers_dir).unwrap();
    let db_path = resources_dir.path().join("data.db");
    let conn: DatabaseConnection = db::connect(&db_path).await.expect("connect");
    migrations::init_schema_versioned(&conn).await.expect("init_schema");
    let covers = Arc::new(covers_dir.clone());
    let identified = resources_dir.path().join("identified");
    let will_delete = resources_dir.path().join("will_delete");
    let archived = resources_dir.path().join("archived");
    std::fs::create_dir_all(&identified).unwrap();
    std::fs::create_dir_all(&will_delete).unwrap();
    std::fs::create_dir_all(&archived).unwrap();
    Harness {
        state: ApiState {
            conn,
            covers_dir: covers,
            identified_dir: Arc::new(identified),
            will_delete_dir: Arc::new(will_delete),
            archived_dir: Arc::new(archived),
            auth_token: Arc::new(RwLock::new(token.to_string())),
            preview_cache: Arc::new(
                doujinshi_records::services::preview_cache::PreviewCache::new(
                    &resources_dir.path().join("_preview_cache"),
                    1024 * 1024,
                )
                .unwrap(),
            ),
        },
        covers_dir,
        resources_dir,
    }
}

/// Build a real `Router` from the route table without spawning a
/// thread. Used by the fast in-process tests.
pub fn router(state: ApiState) -> axum::Router {
    build_test_router(state)
}

/// Build a request pre-loaded with the default test token. Auth-exempt
/// paths (`/api/health`) ignore the header, so this is safe to use
/// everywhere. Tests that specifically exercise 401/403 paths can
/// ignore this helper and construct their own `Request` builder.
pub fn authed_request(method: &str, uri: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header("Authorization", format!("Bearer {}", TEST_TOKEN))
        .body(Body::empty())
        .unwrap()
}

/// Bind a real HTTP server on a free port and return the port + a
/// shutdown handle. Used by the bind/spawn smoke test only.
pub fn bind_real() -> (u16, std::sync::Arc<std::sync::atomic::AtomicBool>) {
    let shutdown = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let ready = Arc::new(std::sync::atomic::AtomicBool::new(false));
    // Pre-bind on a known port (outside the runtime — bind itself is
    // sync); the listener is converted to a tokio listener INSIDE the
    // runtime below because `from_std` requires a reactor context.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().unwrap().port();
    listener.set_nonblocking(true).unwrap();
    // Hold a real on-disk SQLite path for the dummy ApiState. The DB
    // is never queried (the bind test only hits /api/health) but
    // ApiState requires a live `DatabaseConnection`. Leaking the
    // TempDir means the file lives for the whole process — fine for a
    // single test invocation.
    let temp_db_dir = Box::leak(Box::new(tempfile::tempdir().expect("tempdir")));
    let db_path = temp_db_dir.path().join("data.db");
    let shutdown_clone = shutdown.clone();
    let ready_clone = ready.clone();
    std::thread::Builder::new()
        .name("test-http".into())
        .spawn(move || {
            // multi_thread so sqlx can park its background worker on a
            // separate OS thread; new_current_thread deadlocks the
            // SQLite driver when the awaited future tries to drive IO.
            let rt = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .unwrap();
            rt.block_on(async move {
                let tokio_listener =
                    tokio::net::TcpListener::from_std(listener).expect("from_std");
                let conn = db::connect(&db_path).await.expect("db connect");
                let app = build_test_router(ApiState {
                    conn,
                    covers_dir: Arc::new(std::path::PathBuf::from(".")),
                    identified_dir: Arc::new(std::path::PathBuf::from("identified")),
                    will_delete_dir: Arc::new(std::path::PathBuf::from("will_delete")),
                    archived_dir: Arc::new(std::path::PathBuf::from("archived")),
                    auth_token: Arc::new(RwLock::new("test-token".into())),
                    preview_cache: Arc::new(
                        doujinshi_records::services::preview_cache::PreviewCache::new(
                            std::path::Path::new("."),
                            1024 * 1024,
                        )
                        .unwrap(),
                    ),
                });
                let server = axum::serve(tokio_listener, app)
                    .with_graceful_shutdown(async move {
                        loop {
                            if shutdown_clone.load(std::sync::atomic::Ordering::Relaxed) {
                                break;
                            }
                            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                        }
                    });
                // Give the executor a chance to poll the future once so
                // the OS-level socket transitions to accept() mode
                // before the test tries to dial.
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                ready_clone.store(true, std::sync::atomic::Ordering::Release);
                let _ = server.await;
            });
        })
        .unwrap();
    // Spin briefly until the server thread has set `ready`. Avoids a
    // race where the test connects before axum::serve has reached
    // accept().
    for _ in 0..200 {
        if ready.load(std::sync::atomic::Ordering::Acquire) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    (port, shutdown)
}
