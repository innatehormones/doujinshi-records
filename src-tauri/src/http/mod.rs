use anyhow::{Context, Result};
use axum::Router;
use sea_orm::DatabaseConnection;
use std::sync::{Arc, RwLock};
use tower_http::cors::{Any, CorsLayer};

pub mod api;
pub mod auth;
pub mod auth_token;
pub mod placeholder;
pub mod port_allocator;

#[derive(Clone)]
pub struct ApiState {
    pub conn: DatabaseConnection,
    pub covers_dir: Arc<std::path::PathBuf>,
    pub identified_dir: Arc<std::path::PathBuf>,
    pub will_delete_dir: Arc<std::path::PathBuf>,
    pub archived_dir: Arc<std::path::PathBuf>,
    /// Bearer token checked by `auth::require_auth`. Wrapped in
    /// `RwLock` so the `regenerate_auth_token` Tauri command can swap
    /// the value without restarting the HTTP listener.
    pub auth_token: Arc<RwLock<String>>,
    /// LRU preview cache（磁盘 + 内存双层）。HTTP images 端点共用。
    pub preview_cache: Arc<crate::services::preview_cache::PreviewCache>,
}

pub struct Port(pub u16);
impl std::ops::Deref for Port {
    type Target = u16;
    fn deref(&self) -> &u16 {
        &self.0
    }
}

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
        // V2: PATCH metadata + GET by id share the same path. The
        // :id/images sibling is registered just below.
        .route(
            "/api/doujinshi/:id",
            get(api::by_id).patch(api::patch_metadata),
        )
        .route("/api/doujinshi/:id/images", get(api::images))
        .route("/api/doujinshi/:id/images/:index", get(api::image_at))
        .route(
            "/api/doujinshi/:id/images/:index/thumb",
            axum::routing::put(api::put_image_thumb),
        )
        .route(
            "/api/doujinshi/:id/viewed",
            axum::routing::post(api::mark_viewed_http),
        )
        // V2: same as /api/covers/:file_id but hash-keyed. Must
        // come before the :file_id wildcard.
        .route("/api/covers/by-hash/:hash", get(api::cover_by_hash))
        .route("/api/covers/:file_id", get(api::cover))
        // V2 conflict compare (placed AFTER /api/covers so the
        // dynamic `:id` segment here doesn't compete with any
        // wildcard route above).
        .route("/api/conflicts/:id/compare", get(api::compare))
        .route(
            "/api/doujinshi/:id/archive",
            axum::routing::post(api::archive),
        )
        .route(
            "/api/doujinshi/:id/restore",
            axum::routing::post(api::restore),
        )
        .route("/api/dirty", get(api::list_dirty))
        .with_state(state.clone())
        // Bearer-token auth: must sit inside `with_state` (state-aware)
        // and on the OUTSIDE of `.layer(cors)` so preflight OPTIONS can
        // still reach the routes. CORS is added last so it wraps auth.
        .layer(axum::middleware::from_fn_with_state(
            state,
            auth::require_auth,
        ))
        .layer(cors);

    let listener = match preferred_port {
        Some(p) => {
            port_allocator::bind_with_retry(p, 3)
                .context("binding HTTP listener on preferred port")?
                .0
        }
        None => {
            port_allocator::bind_with_retry(0, 1)
                .context("binding HTTP listener")?
                .0
        }
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
            let rt = match tokio::runtime::Builder::new_multi_thread()
                .worker_threads(4)
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
        .route(
            "/api/doujinshi/:id",
            get(api::by_id).patch(api::patch_metadata),
        )
        .route("/api/doujinshi/:id/images", get(api::images))
        .route("/api/doujinshi/:id/images/:index", get(api::image_at))
        .route(
            "/api/doujinshi/:id/images/:index/thumb",
            axum::routing::put(api::put_image_thumb),
        )
        .route(
            "/api/doujinshi/:id/viewed",
            axum::routing::post(api::mark_viewed_http),
        )
        .route("/api/covers/by-hash/:hash", get(api::cover_by_hash))
        .route("/api/covers/:file_id", get(api::cover))
        .route("/api/conflicts/:id/compare", get(api::compare))
        .route(
            "/api/doujinshi/:id/archive",
            axum::routing::post(api::archive),
        )
        .route(
            "/api/doujinshi/:id/restore",
            axum::routing::post(api::restore),
        )
        .route("/api/dirty", get(api::list_dirty))
        .with_state(state.clone())
        .layer(axum::middleware::from_fn_with_state(
            state,
            auth::require_auth,
        ))
        .layer(cors)
}
