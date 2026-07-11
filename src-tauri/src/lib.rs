pub mod commands;
pub mod config;
pub mod db;
pub mod error;
pub mod http;
pub mod models;
pub mod services;

use std::sync::{Arc, RwLock};
use sea_orm::DatabaseConnection;

pub struct AppState {
    pub conn: DatabaseConnection,
    pub scanner: Arc<services::scanner::Scanner>,
    pub covers_dir: Arc<std::path::PathBuf>,
    pub config: config::AppConfig,
    /// Bearer token. `RwLock` so `regenerate_auth_token` can swap the
    /// value at runtime without dropping HTTP requests.
    pub auth_token: Arc<RwLock<String>>,
}

pub async fn run(cfg: config::AppConfig, conn: DatabaseConnection) {
    let scanner = Arc::new(
        services::scanner::Scanner::new(
            conn.clone(),
            cfg.inbox_dir(),
            cfg.covers_dir(),
            cfg.identified_dir(),
        )
        .await,
    );

    if let Err(e) = scanner.start_watcher() {
        eprintln!("failed to start watcher: {:?}", e);
    }

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

    let api_state = http::ApiState {
        conn: conn.clone(),
        covers_dir: covers_dir.clone(),
        identified_dir: Arc::new(cfg.identified_dir()),
        will_delete_dir: Arc::new(cfg.will_delete_dir()),
        archived_dir: Arc::new(cfg.archived_dir()),
        auth_token: auth_token.clone(),
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
    };

    tauri::Builder::default()
        .setup({
            let scanner = scanner.clone();
            move |app| {
                let handle = app.handle().clone();
                let scanner = scanner.clone();
                tauri::async_runtime::spawn(async move {
                    scanner.set_app_handle(handle).await;
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
            commands::settings::manual_scan,
            commands::settings::regenerate_auth_token,
            commands::settings::set_http_port,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
