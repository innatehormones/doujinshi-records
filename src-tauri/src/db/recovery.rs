//! Detect a corrupt SQLite file and back it up before recreating.
//!
//! Heuristic: every SQLite database begins with the 16-byte ASCII
//! string `SQLite format 3\0`. We stat the file and read the first
//! 16 bytes; if the magic does not match we treat the file as
//! corrupt and rename it. We deliberately avoid opening a SQLite
//! connection first — on Windows the OS file lock that an open
//! connection holds blocks the subsequent `rename`, and a flaky
//! file (lock contention, antivirus) can spuriously trigger recovery.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const SQLITE_MAGIC: &[u8; 16] = b"SQLite format 3\0";

pub enum RecoveryAction {
    Noop,
    BackedUp { backup_path: PathBuf },
}

pub async fn probe_and_recover(db_path: &Path) -> anyhow::Result<RecoveryAction> {
    let Ok(mut file) = std::fs::File::open(db_path) else {
        // Nothing on disk yet — caller will create a fresh DB.
        return Ok(RecoveryAction::Noop);
    };
    let mut head = [0u8; 16];
    if file.read(&mut head)? != 16 {
        // File is too short to be a SQLite db.
    } else if head == *SQLITE_MAGIC {
        return Ok(RecoveryAction::Noop);
    }

    let ts = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let backup = db_path.with_extension(format!("db.bak-{}", ts));
    std::fs::rename(db_path, &backup)?;
    Ok(RecoveryAction::BackedUp { backup_path: backup })
}
