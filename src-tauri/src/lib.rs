pub mod commands;
pub mod config;
pub mod db;
pub mod error;
pub mod http;
pub mod models;
pub mod services;

use std::sync::Arc;
use sea_orm::DatabaseConnection;

pub struct AppState {
    pub conn: DatabaseConnection,
    pub scanner: Arc<services::scanner::Scanner>,
    pub covers_dir: Arc<std::path::PathBuf>,
    pub config: config::AppConfig,
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
    let auth_token = Arc::new(auth_token);

    let api_state = http::ApiState {
        conn: conn.clone(),
        covers_dir: covers_dir.clone(),
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
            commands::recycle::list_recycle,
            commands::recycle::permanent_delete,
            commands::recycle::restore_from_recycle,
            commands::inbox::list_conflicts,
            commands::inbox::resolve_conflict,
            commands::settings::get_settings,
            commands::settings::manual_scan,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
