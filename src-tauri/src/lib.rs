pub mod commands;
pub mod config;
pub mod db;
pub mod error;
pub mod http;
pub mod models;
pub mod services;

use sea_orm::DatabaseConnection;
use std::sync::{Arc, RwLock};

pub struct AppState {
    pub conn: DatabaseConnection,
    pub scanner: Arc<services::scanner::Scanner>,
    pub covers_dir: Arc<std::path::PathBuf>,
    pub config: config::AppConfig,
    /// Bearer token. `RwLock` so `regenerate_auth_token` can swap the
    /// value at runtime without dropping HTTP requests.
    pub auth_token: Arc<RwLock<String>>,
    /// LRU preview cache（磁盘 + 内存双层）。HTTP images 端点共用。
    pub preview_cache: Arc<services::preview_cache::PreviewCache>,
}

pub async fn run(cfg: config::AppConfig, conn: DatabaseConnection) {
    // V3 startup contract:
    //   * `main.rs` already called `db::migrations::init_schema_versioned`,
    //     so by the time we land here the schema is at `CURRENT_VERSION`.
    //     V2 → V3 is a non-destructive additive upgrade: `has_physical_file`
    //     column gets `DEFAULT 1`, and `dirty_data` is created with
    //     `IF NOT EXISTS`. Existing rows are never touched.
    //   * `cfg.ensure_dirs()` already ran in main.rs before the connection;
    //     directories are also re-checked defensively below.

    cfg.ensure_dirs().ok();

    let scanner = Arc::new(
        services::scanner::Scanner::new(
            conn.clone(),
            cfg.inbox_dir(),
            cfg.covers_dir(),
            cfg.identified_dir(),
        )
        .await,
    );

    // V3: spawn the dirty-data scanner in the background. Single sweep
    // on startup; cheap enough to run synchronously without blocking
    // UI init.
    let dirty_conn = conn.clone();
    let dirty_cfg = cfg.clone();
    tauri::async_runtime::spawn(async move {
        let report = services::dirty_scanner::scan(
            &dirty_conn,
            &dirty_cfg.identified_dir(),
            &dirty_cfg.will_delete_dir(),
            &dirty_cfg.archived_dir(),
        )
        .await;
        match report {
            Ok(r) => tracing::info!(
                "dirty scan complete: {} orphans, {} missing files",
                r.orphans,
                r.db_missing_files
            ),
            Err(e) => tracing::warn!("dirty scan failed: {:?}", e),
        }
    });

    let covers_dir = Arc::new(cfg.covers_dir());

    // First-launch token bootstrap: read app_setting.auth_token; if
    // missing, generate a fresh 32-byte URL-safe base64 token and persist
    // it. Never log the token value — it is the bearer credential that
    // protects every non-exempt HTTP route (see http::auth).
    let auth_token = match db::read_setting(&conn, "auth_token").await {
        Ok(Some(t)) if !t.is_empty() => t,
        _ => {
            let new = http::auth_token::generate();
            if let Err(e) = db::write_setting(&conn, "auth_token", &new).await {
                eprintln!("failed to persist auth_token: {:?}", e);
            }
            new
        }
    };
    let auth_token = Arc::new(RwLock::new(auth_token));

    // LRU preview cache：磁盘 + 内存双层。HTTP images 端点复用。
    // 启动时扫盘重建 LRU，损坏文件自动清理。
    let preview_cache = match services::preview_cache::PreviewCache::new(
        &cfg.preview_cache_dir(),
        cfg.preview_cache_max_bytes,
    ) {
        Ok(c) => Arc::new(c),
        Err(e) => {
            eprintln!("preview_cache init failed, falling back to empty: {:?}", e);
            Arc::new(
                services::preview_cache::PreviewCache::new(
                    std::path::Path::new("."),
                    cfg.preview_cache_max_bytes,
                )
                .expect("inline empty cache"),
            )
        }
    };

    // 后台 GC：每 30s 把 cache 压回 80% waterline。HTTP handler 读路径
    // 上也会 inline evict，这是兜底。
    {
        let cache_for_gc = preview_cache.clone();
        tauri::async_runtime::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(30)).await;
                if let Err(e) = cache_for_gc.gc().await {
                    eprintln!("preview_cache gc failed: {:?}", e);
                }
            }
        });
    }

    let api_state = http::ApiState {
        conn: conn.clone(),
        covers_dir: covers_dir.clone(),
        identified_dir: Arc::new(cfg.identified_dir()),
        will_delete_dir: Arc::new(cfg.will_delete_dir()),
        archived_dir: Arc::new(cfg.archived_dir()),
        auth_token: auth_token.clone(),
        preview_cache: preview_cache.clone(),
    };

    // Try the previously-persisted HTTP port first; fall back to a free
    // OS-assigned port if it's been grabbed by something else. Persist
    // whatever port we actually got so the next launch can prefer it.
    let preferred = db::read_setting(&conn, "api_port")
        .await
        .ok()
        .flatten()
        .and_then(|s| s.parse::<u16>().ok());
    let port = match http::build_router(api_state, preferred) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("failed to start http api: {:?}", e);
            return;
        }
    };
    if let Err(e) = db::write_setting(&conn, "api_port", &port.to_string()).await {
        eprintln!("failed to persist api_port: {:?}", e);
    }
    println!("http api listening on http://127.0.0.1:{}", port);

    let cfg_clone = cfg.clone();
    let state = AppState {
        conn: conn.clone(),
        scanner: scanner.clone(),
        covers_dir,
        config: cfg_clone,
        auth_token: auth_token.clone(),
        preview_cache: preview_cache.clone(),
    };

    tauri::Builder::default()
        .setup({
            let scanner = scanner.clone();
            move |app| {
                let handle = app.handle().clone();
                let scanner = scanner.clone();
                tauri::async_runtime::spawn(async move {
                    scanner.set_app_handle(handle).await;
                    if let Err(e) = scanner.start_watcher() {
                        tracing::error!("failed to start watcher: {:?}", e);
                    }
                });
                Ok(())
            }
        })
        .manage(state)
        .manage(http::Port(port))
        .invoke_handler(tauri::generate_handler![
            commands::library::list_library,
            commands::library::mark_viewed,
            commands::library::unmark_viewed,
            commands::library::mark_for_delete,
            commands::library::unmark_for_delete,
            commands::library::move_to_will_delete,
            commands::library::update_metadata,
            commands::library::get_by_id,
            commands::library::force_extract,
            commands::library::archive,
            commands::library::restore,
            commands::recycle::list_recycle,
            commands::recycle::permanent_delete,
            commands::recycle::restore_from_recycle,
            commands::inbox::list_conflicts,
            commands::inbox::resolve_conflict,
            commands::dirty::list_dirty,
            commands::settings::get_settings,
            commands::settings::get_scan_status,
            commands::settings::manual_scan,
            commands::settings::regenerate_auth_token,
            commands::settings::set_http_port,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
