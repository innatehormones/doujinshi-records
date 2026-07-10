use crate::AppState;
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
}

#[tauri::command]
pub async fn get_settings(
    state: State<'_, AppState>,
    port: State<'_, Port>,
) -> AppResult<SettingsView> {
    Ok(SettingsView {
        resources_dir: state.config.resources_dir.to_string_lossy().into_owned(),
        inbox_dir: state.config.inbox_dir().to_string_lossy().into_owned(),
        identified_dir: state.config.identified_dir().to_string_lossy().into_owned(),
        will_delete_dir: state.config.will_delete_dir().to_string_lossy().into_owned(),
        covers_dir: state.config.covers_dir().to_string_lossy().into_owned(),
        api_url: format!("http://127.0.0.1:{}", **port),
        scanner_watching: true,
    })
}

#[tauri::command]
pub async fn manual_scan(state: State<'_, AppState>) -> AppResult<usize> {
    state.scanner.scan_inbox_once().await.map_err(Into::into)
}

