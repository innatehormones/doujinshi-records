use std::net::TcpListener;
use std::sync::Arc;
use anyhow::{Context, Result};
use axum::Router;
use sea_orm::DatabaseConnection;
use tower_http::cors::{Any, CorsLayer};

pub mod api;
pub mod placeholder;

#[derive(Clone)]
pub struct ApiState {
    pub conn: DatabaseConnection,
    pub covers_dir: Arc<std::path::PathBuf>,
}

pub struct Port(pub u16);
impl std::ops::Deref for Port { type Target = u16; fn deref(&self) -> &u16 { &self.0 } }

/// Build the axum router and bind it on a free port.
///
/// If `preferred_port` is `Some(p)` the listener first tries to bind to
/// `127.0.0.1:p`; on `AddrInUse` it falls back to `127.0.0.1:0` (OS-assigned)
/// and the caller is responsible for persisting the actual port that came back.
/// This lets the binary keep using the same port across restarts unless
/// something else has grabbed it in the meantime.
pub fn build_router(state: ApiState, preferred_port: Option<u16>) -> Result<u16> {
    use axum::routing::get;

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let router = Router::new()
        .route("/api/health", get(api::health))
        .route("/api/doujinshi/search", get(api::search))
        // V2: explicit alias, registered before by-hash so the
        // literal "check" path is not eaten by the `:hash` wildcard.
        .route("/api/doujinshi/check", get(api::check))
        .route("/api/doujinshi/by-hash/:hash", get(api::by_hash))
        .route("/api/doujinshi/:id", get(api::by_id))
        .route("/api/doujinshi/:id/viewed", axum::routing::post(api::mark_viewed_http))
        // V2: same as /api/covers/:file_id but hash-keyed. Must
        // come before the :file_id wildcard.
        .route("/api/covers/by-hash/:hash", get(api::cover_by_hash))
        .route("/api/covers/:file_id", get(api::cover))
        .with_state(state)
        .layer(cors);

    let listener = match preferred_port {
        Some(p) => TcpListener::bind(("127.0.0.1", p))
            .or_else(|_| TcpListener::bind("127.0.0.1:0"))
            .context("binding HTTP listener")?,
        None => TcpListener::bind("127.0.0.1:0").context("binding HTTP listener")?,
    };
    let port = listener.local_addr()?.port();
    listener.set_nonblocking(true)?;
    let tokio_listener = tokio::net::TcpListener::from_std(listener)?;
    let make_service = router.into_make_service();

    // Run the HTTP server on a dedicated thread + tokio runtime so it does
    // not depend on the #[tokio::main] runtime that Tauri takes over once
    // `tauri::Builder::default().run()` is called. Without this isolation,
    // the spawned `axum::serve` task is starved and accepted connections
    // hang forever.
    std::thread::Builder::new()
        .name("doujinshi-http-api".into())
        .spawn(move || {
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    eprintln!("http api runtime init failed: {:?}", e);
                    return;
                }
            };
            rt.block_on(async move {
                if let Err(e) = axum::serve(tokio_listener, make_service).await {
                    eprintln!("http api serve error: {:?}", e);
                }
            });
        })?;

    Ok(port)
}

/// Build a `Router` without binding a socket. Used by benchmarks and
/// integration tests that want the full middleware stack without the
/// thread-spawn overhead.
pub fn build_test_router(state: ApiState) -> axum::Router {
    use axum::routing::get;
    use tower_http::cors::{Any, CorsLayer};
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);
    axum::Router::new()
        .route("/api/health", get(api::health))
        .route("/api/doujinshi/search", get(api::search))
        .route("/api/doujinshi/check", get(api::check))
        .route("/api/doujinshi/by-hash/:hash", get(api::by_hash))
        .route("/api/doujinshi/:id", get(api::by_id))
        .route("/api/doujinshi/:id/viewed", axum::routing::post(api::mark_viewed_http))
        .route("/api/covers/by-hash/:hash", get(api::cover_by_hash))
        .route("/api/covers/:file_id", get(api::cover))
        .with_state(state)
        .layer(cors)
}
