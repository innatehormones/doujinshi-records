use crate::AppState;
use crate::db;
use crate::http::Port;
use crate::error::AppResult;
use serde::Serialize;
use tauri::State;

#[derive(Debug, Serialize)]
pub struct SettingsView {
    pub resources_dir: String,
    pub inbox_dir: String,
    pub identified_dir: String,
    pub will_delete_dir: String,
    pub covers_dir: String,
    pub api_url: String,
    pub scanner_watching: bool,
    pub auth_token: String,
    pub http_port: u16,
    /// `true` when the persisted `api_port` is non-zero (the user
    /// asked for a fixed port rather than letting the OS pick).
    pub http_port_locked: bool,
}

#[tauri::command]
pub async fn get_settings(
    state: State<'_, AppState>,
    port: State<'_, Port>,
) -> AppResult<SettingsView> {
    // Pull the locked-port flag straight from the setting row so the
    // UI reflects whatever the user last persisted, even if the
    // listener had to fall back to a random port this run.
    let stored_port: Option<u16> = db::read_setting(&state.conn, "api_port")
        .await
        .ok()
        .flatten()
        .and_then(|s| s.parse().ok());
    let http_port_locked = stored_port.map(|p| p != 0).unwrap_or(false);
    Ok(SettingsView {
        resources_dir: state.config.resources_dir.to_string_lossy().into_owned(),
        inbox_dir: state.config.inbox_dir().to_string_lossy().into_owned(),
        identified_dir: state.config.identified_dir().to_string_lossy().into_owned(),
        will_delete_dir: state.config.will_delete_dir().to_string_lossy().into_owned(),
        covers_dir: state.config.covers_dir().to_string_lossy().into_owned(),
        api_url: format!("http://127.0.0.1:{}", **port),
        scanner_watching: true,
        auth_token: state
            .auth_token
            .read()
            .map(|s| s.clone())
            .unwrap_or_default(),
        http_port: **port,
        http_port_locked,
    })
}

#[tauri::command]
pub async fn manual_scan(state: State<'_, AppState>) -> AppResult<usize> {
    state.scanner.scan_inbox_once().await.map_err(Into::into)
}

/// Regenerate the HTTP bearer token. Persists the new value to
/// `app_setting.auth_token` and swaps the in-memory `RwLock<String>`
/// that the axum middleware reads. Old token stops working
/// immediately for new requests.
#[tauri::command]
pub async fn regenerate_auth_token(state: State<'_, AppState>) -> AppResult<String> {
    let new = crate::http::auth_token::generate();
    db::write_setting(&state.conn, "auth_token", &new).await?;
    if let Ok(mut guard) = state.auth_token.write() {
        *guard = new.clone();
    }
    Ok(new)
}

/// Persist a preferred HTTP port. `0` means "OS-assigned, don't
/// lock". Takes effect on the next launch; the running listener is
/// unaffected.
#[tauri::command]
pub async fn set_http_port(state: State<'_, AppState>, port: u16) -> AppResult<()> {
    db::write_setting(&state.conn, "api_port", &port.to_string()).await?;
    Ok(())
}