//! `BackupStorage` trait + 本地文件系统实现。
//!
//! **解耦 + 易扩展**：第一版只实现 `LocalFsStorage`；未来加 `S3Storage` /
//! `WebDavStorage` 只新加 struct + `impl BackupStorage`，`BackupService` 和
//! commands 层完全不变。

use anyhow::Result;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// 备份快照元信息（用于 UI 列表）
#[derive(Debug, Clone, Serialize)]
pub struct SnapshotInfo {
    pub path: PathBuf,
    pub mtime: SystemTime,
    pub size_bytes: u64,
}

/// 存储后端 trait。
///
/// **注意**：`write_snapshot` 在 `LocalFsStorage` 上故意 unimplemented——
/// `BackupService` 走 SQLite `VACUUM INTO` 直接生成目标文件，绕过存储层；
/// trait 里保留方法仅为未来其他后端对齐接口。
pub trait BackupStorage: Send + Sync {
    fn write_snapshot(&self, dst: &Path) -> Result<()>;
    fn list_snapshots(&self, dir: &Path) -> Result<Vec<SnapshotInfo>>;
    fn delete_snapshot(&self, path: &Path) -> Result<()>;
}

/// 本地文件系统实现。SQLite 文件复制由调用方负责（用 VACUUM INTO），
/// 这里只承担「列目录 + 删除文件」职责。
pub struct LocalFsStorage;

impl BackupStorage for LocalFsStorage {
    fn write_snapshot(&self, _dst: &Path) -> Result<()> {
        // 实际上 BackupService 走 VACUUM INTO 直接生成 dst，
        // 不需要 storage 层介入。这里保留 trait 方法是为未来其他后端对齐接口。
        unimplemented!(
            "BackupService uses VACUUM INTO directly; storage trait retained for future backends"
        )
    }

    fn list_snapshots(&self, dir: &Path) -> Result<Vec<SnapshotInfo>> {
        let mut out = Vec::new();
        if !dir.exists() {
            return Ok(out);
        }
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let name = match path.file_name().and_then(|n| n.to_str()) {
                Some(n) => n,
                None => continue,
            };
            // 只列 data-*.db，过滤 .tmp-* 临时文件
            if !name.starts_with("data-") || !name.ends_with(".db") || name.contains(".tmp-") {
                continue;
            }
            let meta = entry.metadata()?;
            out.push(SnapshotInfo {
                path,
                mtime: meta.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                size_bytes: meta.len(),
            });
        }
        // mtime 倒序（最新在前）
        out.sort_by(|a, b| b.mtime.cmp(&a.mtime));
        Ok(out)
    }

    fn delete_snapshot(&self, path: &Path) -> Result<()> {
        if path.exists() {
            std::fs::remove_file(path)?;
        }
        Ok(())
    }
}