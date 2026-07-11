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

    fn entry_path(&self, key: &CacheKey) -> PathBuf {
        let (id, mtime) = *key;
        let mtime_unix = mtime.duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default().as_secs();
        self.dir.join(format!("{}-{}.json", id, mtime_unix))
    }

    /// Read cached body; updates LRU recency. Returns clone to avoid
    /// callers fighting `lru.get` borrow lifetime.
    pub fn get(&self, key: &CacheKey) -> Option<Vec<u8>> {
        let mut lru = self.inner.lock().unwrap();
        lru.get(key).map(|e| e.body.clone())
    }

    /// Get from cache or compute via `compute`, persisting the result
    /// to disk. If the cache is full, evicts LRU entries until
    /// `bytes_in_cache <= max_bytes * 80%`.
    pub async fn get_or_compute<F, Fut>(&self, key: CacheKey, compute: F) -> Result<Vec<u8>>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<Vec<u8>>>,
    {
        if let Some(body) = self.get(&key) {
            return Ok(body);
        }
        let body = compute().await?;
        self.insert(key, body.clone()).await?;
        Ok(body)
    }

    async fn insert(&self, key: CacheKey, body: Vec<u8>) -> Result<()> {
        let size = body.len() as u64;
        let final_path = self.entry_path(&key);
        let tmp_path = self.dir.join(format!(
            ".tmp-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        tokio::fs::write(&tmp_path, &body).await?;
        tokio::fs::rename(&tmp_path, &final_path).await?;

        let mut lru = self.inner.lock().unwrap();
        // lru::LruCache::push returns the evicted entry if over capacity.
        // We use put() to control ordering precisely.
        if let Some(old) = lru.put(key, CacheEntry { body, last_accessed: Instant::now() }) {
            // old entry was displaced; nothing to do, capacity-side handled by lru crate.
            let _ = old;
        }
        self.bytes_in_cache.fetch_add(size, Ordering::Relaxed);
        self.evict_to_waterline_locked(&mut lru).await;
        Ok(())
    }

    /// Drain bytes down to 80% of `max_bytes` by popping LRU entries.
    /// Caller MUST hold the inner mutex lock (passes &mut LruCache).
    /// Shared by `insert` (inline, lock already held) and `gc` (locks then calls).
    async fn evict_to_waterline_locked(&self, lru: &mut LruCache<CacheKey, CacheEntry>) {
        let waterline = self.max_bytes * 80 / 100;
        while self.bytes_in_cache.load(Ordering::Relaxed) > waterline {
            match lru.pop_lru() {
                Some((evicted_key, evicted)) => {
                    let ev_size = evicted.body.len() as u64;
                    self.bytes_in_cache.fetch_sub(ev_size, Ordering::Relaxed);
                    let _ = tokio::fs::remove_file(self.entry_path(&evicted_key)).await;
                }
                None => break,
            }
        }
    }

    /// Background GC entry point. Drains to 80% waterline if over budget;
    /// no-op otherwise. Called by the lib.rs spawn loop every 30s.
    pub async fn gc(&self) -> Result<()> {
        let mut lru = self.inner.lock().unwrap();
        self.evict_to_waterline_locked(&mut lru).await;
        Ok(())
    }
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

    #[tokio::test]
    async fn get_or_compute_miss_triggers_compute_and_writes_disk() {
        let dir = tempfile::tempdir().unwrap();
        let cache = PreviewCache::new(dir.path(), 1024 * 1024).unwrap();
        let key = (1, mtime_from_unix(1_700_000_000));
        let mut called = 0;
        let body = cache
            .get_or_compute(key, || async {
                called += 1;
                Ok::<_, anyhow::Error>(b"computed".to_vec())
            })
            .await
            .unwrap();
        assert_eq!(body, b"computed");
        assert_eq!(called, 1);

        // Disk file exists matching naming convention.
        let on_disk = std::fs::read(dir.path().join("1-1700000000.json")).unwrap();
        assert_eq!(on_disk, b"computed");
        assert_eq!(cache.bytes_in_cache(), 8);
    }

    #[tokio::test]
    async fn get_or_compute_hit_does_not_recompute() {
        let dir = tempfile::tempdir().unwrap();
        let cache = PreviewCache::new(dir.path(), 1024 * 1024).unwrap();
        let key = (7, mtime_from_unix(1_700_000_001));
        let _ = cache
            .get_or_compute(key, || async { Ok::<_, anyhow::Error>(b"abc".to_vec()) })
            .await
            .unwrap();
        let mut called = 0;
        let body = cache
            .get_or_compute(key, || async {
                called += 1;
                Ok::<_, anyhow::Error>(b"DIFFERENT".to_vec())
            })
            .await
            .unwrap();
        assert_eq!(body, b"abc");
        assert_eq!(called, 0, "compute closure should NOT be re-invoked on hit");
    }

    #[tokio::test]
    async fn different_mtime_yields_separate_cache_entries() {
        let dir = tempfile::tempdir().unwrap();
        let cache = PreviewCache::new(dir.path(), 1024 * 1024).unwrap();
        let k1 = (1, mtime_from_unix(1_700_000_010));
        let k2 = (1, mtime_from_unix(1_700_000_020));
        let _ = cache.get_or_compute(k1, || async { Ok::<_, anyhow::Error>(b"v1".to_vec()) }).await.unwrap();
        let _ = cache.get_or_compute(k2, || async { Ok::<_, anyhow::Error>(b"v2".to_vec()) }).await.unwrap();

        assert_eq!(cache.bytes_in_cache(), 4);
        assert!(dir.path().join("1-1700000010.json").exists());
        assert!(dir.path().join("1-1700000020.json").exists());
    }

    #[tokio::test]
    async fn insert_over_max_bytes_evicts_oldest_until_under_waterline() {
        let dir = tempfile::tempdir().unwrap();
        // 100 bytes total budget; each entry 30 bytes. Insert 5 → expect ~3 entries left.
        let cache = PreviewCache::new(dir.path(), 100).unwrap();
        for i in 0..5i64 {
            let key = (i, mtime_from_unix(1_700_000_100 + i as u64));
            let _ = cache
                .get_or_compute(key, || async {
                    Ok::<_, anyhow::Error>(vec![b'x'; 30])
                })
                .await
                .unwrap();
        }
        // Waterline = 100 * 80% = 80 → drop to ≤ 80 bytes kept (2 entries = 60 bytes).
        assert!(cache.bytes_in_cache() <= 80, "should be at or under waterline; got {}", cache.bytes_in_cache());
        // Oldest entries evicted; newest 2 retained.
        assert!(cache.get(&(4, mtime_from_unix(1_700_000_104))).is_some());
        assert!(cache.get(&(0, mtime_from_unix(1_700_000_100))).is_none());
        // Disk files for evicted entries deleted.
        assert!(!dir.path().join("0-1700000100.json").exists());
    }

    #[tokio::test]
    async fn gc_drains_to_waterline_when_over_budget() {
        let dir = tempfile::tempdir().unwrap();
        // Pre-write 5 entries (30 bytes each = 150 total) directly to disk.
        // Simulates restart where disk cache outlives the in-memory budget.
        for i in 0..5i64 {
            let name = format!("{}-{}.json", i, 1_700_000_300 + i as u64);
            std::fs::write(dir.path().join(name), vec![b'x'; 30]).unwrap();
        }
        // max_bytes = 80 (waterline = 64). All 5 entries (150 bytes) are
        // loaded by reload_from_disk → over budget until gc runs.
        let cache = PreviewCache::new(dir.path(), 80).unwrap();
        assert_eq!(cache.bytes_in_cache(), 150);

        cache.gc().await.unwrap();
        assert!(cache.bytes_in_cache() <= 64, "gc should drain to waterline; got {}", cache.bytes_in_cache());
    }
}