use anyhow::Result;
use sea_orm::DatabaseConnection;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::AppHandle;
use tauri::Emitter;
use tokio::sync::Mutex;

#[derive(Clone)]
pub struct Scanner {
    pub conn: DatabaseConnection,
    pub inbox_dir: Arc<PathBuf>,
    pub covers_dir: Arc<PathBuf>,
    pub identified_dir: Arc<PathBuf>,
    pub state: Arc<Mutex<ScannerState>>,
    pub app_handle: Arc<Mutex<Option<AppHandle>>>,
}

#[derive(Default)]
pub struct ScannerState {
    pub last_scan_count: usize,
    pub last_scan_at: Option<chrono::DateTime<chrono::Utc>>,
    pub is_watching: bool,
}

impl Scanner {
    pub async fn new(
        conn: DatabaseConnection,
        inbox_dir: PathBuf,
        covers_dir: PathBuf,
        identified_dir: PathBuf,
    ) -> Self {
        Self {
            conn,
            inbox_dir: Arc::new(inbox_dir),
            covers_dir: Arc::new(covers_dir),
            identified_dir: Arc::new(identified_dir),
            state: Arc::new(Mutex::new(ScannerState::default())),
            app_handle: Arc::new(Mutex::new(None)),
        }
    }

    pub async fn set_app_handle(&self, handle: AppHandle) {
        *self.app_handle.lock().await = Some(handle);
    }

    pub async fn scan_inbox_once(&self) -> Result<usize> {
        let mut processed = 0usize;
        let mut entries = tokio::fs::read_dir(&*self.inbox_dir).await?;
        while let Some(e) = entries.next_entry().await? {
            let p = e.path();
            if !is_candidate(&p) {
                continue;
            }
            let outcome = crate::services::identifier::identify_file(
                &self.conn,
                &p,
                &self.covers_dir,
                &self.identified_dir,
                None,
            )
            .await?;
            log_outcome(&outcome);
            processed += 1;
        }
        let mut st = self.state.lock().await;
        st.last_scan_count = processed;
        st.last_scan_at = Some(chrono::Utc::now());
        drop(st);

        // Notify frontend (best-effort)
        if let Some(handle) = self.app_handle.lock().await.clone() {
            let _ = handle.emit("library-updated", processed);
        }

        Ok(processed)
    }

    pub fn start_watcher(&self) -> Result<()> {
        use notify::{RecursiveMode, Watcher};
        use notify_debouncer_full::new_debouncer;
        let inbox = self.inbox_dir.clone();
        let scanner = self.clone();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            let (tx, rx) = std::sync::mpsc::channel();
            let mut debouncer = match new_debouncer(std::time::Duration::from_secs(2), None, tx) {
                Ok(d) => d,
                Err(e) => {
                    tracing::error!("failed to create debouncer: {:?}", e);
                    return;
                }
            };
            if let Err(e) = debouncer.watcher().watch(&*inbox, RecursiveMode::NonRecursive) {
                tracing::error!("watch error: {:?}", e);
                return;
            }
            for res in rx {
                if res.is_ok() {
                    let _ = rt.block_on(scanner.scan_inbox_once());
                }
            }
        });
        Ok(())
    }
}

fn is_candidate(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_lowercase());
    matches!(ext.as_deref(), Some("zip") | Some("rar"))
}

fn log_outcome(outcome: &crate::services::identifier::IdentifyOutcome) {
    use crate::services::identifier::IdentifyOutcome::*;
    match outcome {
        AlreadyKnown(_) => {}
        NewIdentified(id) => tracing::info!(id, "new file identified"),
        Conflict { a_id, .. } => tracing::warn!(a_id, "conflict detected"),
        Error(e) => tracing::error!(error = e, "identify failed"),
    }
}
