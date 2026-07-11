//! Query available disk space on the volume containing a given path.
//!
//! Used by the RAR extraction path to refuse extraction when the
//! unpacked archive wouldn't fit. sysinfo is the cheapest way to get
//! this on Windows — it walks the OS volume list once and lets us
//! match by mount-point ancestry.

use std::path::Path;

pub fn available_bytes(path: &Path) -> std::io::Result<u64> {
    let disks = sysinfo::Disks::new_with_refreshed_list();
    for disk in &disks {
        if path.starts_with(disk.mount_point()) {
            return Ok(disk.available_space());
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        format!("no disk found for path {}", path.display()),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn available_bytes_returns_positive_for_existing_path() {
        let p = std::env::temp_dir();
        let bytes = available_bytes(&p).unwrap();
        assert!(bytes > 0, "temp dir should have some free space, got {}", bytes);
    }

    #[test]
    fn available_bytes_returns_err_for_path_with_no_ancestor_disk() {
        // 构造一个不存在于任何挂载点下的路径很困难——用一个明显不存在的盘符。
        let p = Path::new("Z:\\definitely\\not\\a\\real\\drive");
        assert!(available_bytes(p).is_err());
    }
}