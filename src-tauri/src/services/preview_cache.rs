//! LRU preview cache：磁盘 + 内存双层，HTTP images 响应体长缓存。

use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Instant, SystemTime};

use anyhow::{anyhow, Result};
use lru::LruCache;

pub type CacheKey = (i64, SystemTime);

pub struct CacheEntry {
    pub body: Vec<u8>,
    pub last_accessed: Instant,
}

pub struct PreviewCache {
    inner: Mutex<LruCache<CacheKey, CacheEntry>>,
    max_bytes: u64,
    bytes_in_cache: AtomicU64,
    dir: PathBuf,
}

impl PreviewCache {
    /// Build a cache, scanning `dir` for prior on-disk entries to
    /// repopulate the in-memory LRU.
    pub fn new(dir: &Path, max_bytes: u64) -> Result<Self> {
        if !dir.exists() {
            std::fs::create_dir_all(dir).map_err(|e| anyhow!("mkdir preview_cache: {}", e))?;
        }
        let cap = NonZeroUsize::new(1024).unwrap();
        let cache = Self {
            inner: Mutex::new(LruCache::new(cap)),
            max_bytes,
            bytes_in_cache: AtomicU64::new(0),
            dir: dir.to_path_buf(),
        };
        cache.reload_from_disk()?;
        Ok(cache)
    }

    fn reload_from_disk(&self) -> Result<()> {
        let mut lru = self.inner.lock().unwrap();
        let mut bytes: u64 = 0;
        for entry in std::fs::read_dir(&self.dir)? {
            let entry = entry?;
            let p = entry.path();
            if !p.is_file() {
                continue;
            }
            let Some(name) = p.file_name().and_then(|s| s.to_str()) else { continue };
            // Filename shape: `<id>-<mtime>.json`. Strip extension first so
            // split_once('-') doesn't capture `.json` into the mtime token.
            let stem = name.strip_suffix(".json").unwrap_or(name);
            let Some((id_str, mtime_str)) = stem.split_once('-') else { continue };
            let (Ok(id), Ok(mtime_unix)) = (id_str.parse::<i64>(), mtime_str.parse::<u64>()) else { continue };
            let Some(mtime) = SystemTime::UNIX_EPOCH.checked_add(std::time::Duration::from_secs(mtime_unix)) else { continue };
            let body = match std::fs::read(&p) {
                Ok(b) => b,
                Err(_) => continue,
            };
            let size = body.len() as u64;
            lru.put((id, mtime), CacheEntry { body, last_accessed: Instant::now() });
            bytes += size;
        }
        self.bytes_in_cache.store(bytes, Ordering::Relaxed);
        Ok(())
    }

    pub fn max_bytes(&self) -> u64 { self.max_bytes }

    pub fn bytes_in_cache(&self) -> u64 { self.bytes_in_cache.load(Ordering::Relaxed) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn mtime_from_unix(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
    }

    #[test]
    fn new_with_empty_dir_yields_empty_cache() {
        let dir = tempfile::tempdir().unwrap();
        let cache = PreviewCache::new(dir.path(), 1024 * 1024).unwrap();
        assert_eq!(cache.bytes_in_cache(), 0);
    }

    #[test]
    fn new_rebuilds_lru_from_existing_entries() {
        let dir = tempfile::tempdir().unwrap();
        // Pre-write two entries on disk matching the file naming convention.
        std::fs::write(dir.path().join("42-1000.json"), b"hello").unwrap();
        std::fs::write(dir.path().join("99-2000.json"), b"world!").unwrap();

        let cache = PreviewCache::new(dir.path(), 1024 * 1024).unwrap();
        assert_eq!(cache.bytes_in_cache(), 11);
    }
}