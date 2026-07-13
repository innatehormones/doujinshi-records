use anyhow::Result;
use sea_orm::DatabaseConnection;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::AppHandle;
use tauri::Emitter;
use tokio::sync::Mutex;

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct ScanStatus {
    pub is_scanning: bool,
    pub processed: usize,
    pub total: usize,
    pub failed: usize,
}

#[derive(Clone)]
pub struct Scanner {
    pub conn: DatabaseConnection,
    pub inbox_dir: Arc<PathBuf>,
    pub covers_dir: Arc<PathBuf>,
    pub identified_dir: Arc<PathBuf>,
    pub state: Arc<Mutex<ScannerState>>,
    pub app_handle: Arc<Mutex<Option<AppHandle>>>,
    scan_guard: Arc<Mutex<()>>,
}

#[derive(Default)]
pub struct ScannerState {
    pub last_scan_count: usize,
    pub last_scan_at: Option<chrono::DateTime<chrono::Utc>>,
    pub is_watching: bool,
    pub scan_status: ScanStatus,
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
            scan_guard: Arc::new(Mutex::new(())),
        }
    }

    pub async fn set_app_handle(&self, handle: AppHandle) {
        *self.app_handle.lock().await = Some(handle);
    }

    pub async fn status(&self) -> ScanStatus {
        self.state.lock().await.scan_status.clone()
    }

    async fn update_scan_status(&self, status: ScanStatus) {
        self.state.lock().await.scan_status = status.clone();
        if let Some(handle) = self.app_handle.lock().await.clone() {
            let _ = handle.emit("scanner-status", status);
        }
    }

    pub async fn scan_inbox_once(&self) -> Result<usize> {
        let _scan_guard = self.scan_guard.lock().await;
        let mut candidates = Vec::new();
        let mut entries = tokio::fs::read_dir(&*self.inbox_dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if is_candidate(&path) {
                candidates.push(path);
            }
        }
        if candidates.is_empty() {
            return Ok(0);
        }

        let mut status = ScanStatus {
            is_scanning: true,
            processed: 0,
            total: candidates.len(),
            failed: 0,
        };
        self.update_scan_status(status.clone()).await;

        for path in candidates {
            let outcome = crate::services::identifier::identify_file(
                &self.conn,
                &path,
                &self.covers_dir,
                &self.identified_dir,
                None,
                false,
            )
            .await
            .unwrap_or_else(|error| {
                use crate::services::identifier::IdentifyOutcome;
                tracing::error!(error = %error, "identify_file failed");
                let is_rar = path
                    .extension()
                    .and_then(|s| s.to_str())
                    .map(|s| s.eq_ignore_ascii_case("rar"))
                    .unwrap_or(false);
                if is_rar {
                    if let Some(payload) = error.to_rar_payload() {
                        if let Ok(handle_guard) = self.app_handle.try_lock() {
                            if let Some(handle) = handle_guard.clone() {
                                let _ = handle.emit(
                                    "rar-error",
                                    serde_json::json!({
                                        "filename": path.file_name().and_then(|n| n.to_str()).unwrap_or(""),
                                        "file_path": path.to_string_lossy(),
                                        "error": payload,
                                    }),
                                );
                            }
                        }
                    }
                }
                IdentifyOutcome::Error(error.to_string())
            });
            if matches!(
                outcome,
                crate::services::identifier::IdentifyOutcome::Error(_)
            ) {
                status.failed += 1;
            }
            log_outcome(&outcome);
            status.processed += 1;
            self.update_scan_status(status.clone()).await;
        }

        status.is_scanning = false;
        {
            let mut state = self.state.lock().await;
            state.last_scan_count = status.processed;
            state.last_scan_at = Some(chrono::Utc::now());
            state.scan_status = status.clone();
        }
        if let Some(handle) = self.app_handle.lock().await.clone() {
            let _ = handle.emit("scanner-status", status.clone());
            let _ = handle.emit("library-updated", status.processed);
        }

        Ok(status.processed)
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
            if let Err(e) = debouncer
                .watcher()
                .watch(&*inbox, RecursiveMode::NonRecursive)
            {
                tracing::error!("watch error: {:?}", e);
                return;
            }
            if let Err(e) = rt.block_on(scanner.scan_inbox_once()) {
                tracing::error!("startup inbox scan failed: {:?}", e);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{self, migrations};

    async fn build_scanner(root: &Path) -> Scanner {
        let inbox = root.join("inbox");
        let covers = root.join("covers");
        let identified = root.join("identified");
        std::fs::create_dir_all(&inbox).unwrap();
        std::fs::create_dir_all(&covers).unwrap();
        std::fs::create_dir_all(&identified).unwrap();
        let conn = db::connect(&root.join("test.db")).await.unwrap();
        migrations::init_schema_versioned(&conn).await.unwrap();
        Scanner::new(conn, inbox, covers, identified).await
    }

    #[tokio::test]
    async fn empty_scan_keeps_status_hidden() {
        let dir = tempfile::tempdir().unwrap();
        let scanner = build_scanner(dir.path()).await;

        assert_eq!(scanner.scan_inbox_once().await.unwrap(), 0);
        assert_eq!(scanner.status().await, ScanStatus::default());
    }

    #[tokio::test]
    async fn invalid_zip_counts_as_failed_and_finishes() {
        let dir = tempfile::tempdir().unwrap();
        let scanner = build_scanner(dir.path()).await;
        std::fs::write(scanner.inbox_dir.join("broken.zip"), b"not a zip").unwrap();

        assert_eq!(scanner.scan_inbox_once().await.unwrap(), 1);
        assert_eq!(
            scanner.status().await,
            ScanStatus {
                is_scanning: false,
                processed: 1,
                total: 1,
                failed: 1,
            }
        );
    }

    #[test]
    fn scan_status_serializes_for_frontend() {
        let status = ScanStatus {
            is_scanning: true,
            processed: 3,
            total: 12,
            failed: 1,
        };

        assert_eq!(
            serde_json::to_value(status).unwrap(),
            serde_json::json!({
                "is_scanning": true,
                "processed": 3,
                "total": 12,
                "failed": 1,
            })
        );
    }
}
