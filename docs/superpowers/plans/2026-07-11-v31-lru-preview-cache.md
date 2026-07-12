# V3.1 Implementation Plan — LRU Preview Cache

> **状态**：已实现，但实施过程中大幅偏离原 plan。原始 plan 是给 `/images` 端点的响应体加 `(file_id, mtime)` 缓存；落地后改成 per-image `(file_id, image_index)` 单图 webp 缓存 + Worker 流水线 + 懒加载 + 全屏预览。实际架构与现状以 spec [`2026-07-11-v31-lru-preview-cache.md`](../specs/2026-07-11-v31-lru-preview-cache.md) 为准。本文件保留作历史档案。
>
> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 给 `GET /api/doujinshi/<id>/images` 加响应体 LRU 缓存，磁盘长缓存 + 内存 LRU 索引，HTTP ETag 304 路径。

**Architecture:** 单新模块 `services::preview_cache::PreviewCache`，双层（磁盘 `_preview_cache/<id>-<mtime>.json` + 内存 `lru::LruCache`），cache key `(file_id, zip_mtime_unix)` 自动随 mtime 变化失效。后台 30s GC 兜底；HTTP handler 读时 inline evict。

**Tech Stack:** Rust（`lru` crate + `tokio::fs`）+ Tauri 2 + Axum 0.7。镜像 V3 已建立的 `services::xxx` 单文件单测模式（`#[cfg(test)] mod tests` 内嵌）。

---

## File Structure

**新建**
- `src-tauri/src/services/preview_cache.rs` — `PreviewCache` struct + 单元测试（约 350 行，含 tests）
- `tests/preview_cache_integration.rs` — 启动扫盘重建 LRU + 损坏文件自愈的集成测试

**修改**
- `src-tauri/Cargo.toml` — 加 `lru = "0.12"` 依赖
- `src-tauri/src/config.rs` — `preview_cache_max_bytes: u64` 字段（默认 200 MiB）+ 测试
- `src-tauri/src/lib.rs` — `AppState` 加 `Arc<PreviewCache>`，`run()` 中 spawn 后台 GC + 初始化 cache
- `src-tauri/src/http/api.rs` — `images` handler 加 ETag + cache peek 路径
- `src-tauri/tests/common/mod.rs` — `build_state_with_token` 多构造 `PreviewCache`（把 `_preview_cache/` 加进 test harness 的临时目录）
- `src-tauri/tests/http_routes.rs` — `images_endpoint_returns_304_when_etag_matches` + `images_endpoint_caches_response` 两个测试

---

## Task 1: Cargo.toml 加 lru 依赖

**Files:**
- Modify: `src-tauri/Cargo.toml`

- [ ] **Step 1: 加 lru 到 dependencies**

在 `[dependencies]` 段尾加：

```toml
lru = "0.12"
```

- [ ] **Step 2: 验证编译**

```bash
cd src-tauri && cargo check
```

期望：成功，下载 `lru` crate，无警告。

- [ ] **Step 3: 提交**

```bash
git add src-tauri/Cargo.toml src-tauri/Cargo.lock
git commit -m "build(deps): add lru crate for preview cache"
```

---

## Task 2: AppConfig 加 preview_cache_max_bytes + ensure_dirs 创建 _preview_cache/

**Files:**
- Modify: `src-tauri/src/config.rs`

- [ ] **Step 1: 加字段（默认 200 MiB）**

在 `pub struct AppConfig` 内追加：

```rust
    /// LRU preview cache 容量上限（字节）。200 MiB 默认。
    pub preview_cache_max_bytes: u64,
```

`AppConfig::load()` 里 hardcode 默认值（暂无 TOML）：

```rust
    AppConfig {
        resources_dir,
        preview_cache_max_bytes: 200 * 1024 * 1024,
    }
```

- [ ] **Step 2: ensure_dirs 加入 _preview_cache**

`ensure_dirs` 函数末尾（创建 covers_dir 后）加：

```rust
        if !cache_dir.exists() {
            std::fs::create_dir_all(&cache_dir).ok();
        }
```

`cache_dir` 计算为 `self.resources_dir.join("_preview_cache")`。如果函数里已有 `let preview_cache_dir = ...`，把名字对齐。

同时给 `impl AppConfig` 加便捷方法（让 `lib.rs` 不用重复拼路径）：

```rust
    /// `_preview_cache/` 目录的绝对路径（无需保证已存在）。
    pub fn preview_cache_dir(&self) -> std::path::PathBuf {
        self.resources_dir.join("_preview_cache")
    }
```

- [ ] **Step 3: 写测试**

在 `config.rs` 末尾 `#[cfg(test)]` 块加：

```rust
    #[test]
    fn preview_cache_max_bytes_defaults_to_200mib() {
        let cfg = AppConfig {
            resources_dir: std::path::PathBuf::from("r"),
            preview_cache_max_bytes: 200 * 1024 * 1024,
        };
        assert_eq!(cfg.preview_cache_max_bytes, 209_715_200);
    }

    #[test]
    fn ensure_dirs_creates_preview_cache() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = AppConfig {
            resources_dir: dir.path().to_path_buf(),
            preview_cache_max_bytes: 0,
        };
        cfg.ensure_dirs().unwrap();
        assert!(dir.path().join("_preview_cache").exists());
    }
```

- [ ] **Step 4: 跑测试，验证通过**

```bash
cd src-tauri && cargo test --lib config::tests
```

期望：4 个测试通过（含已有 2 个）。

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/config.rs
git commit -m "feat(config): preview_cache_max_bytes field + ensure_dirs creates _preview_cache"
```

---

## Task 3: PreviewCache 模块骨架 + new() + 启动扫盘重建 LRU

**Files:**
- Create: `src-tauri/src/services/preview_cache.rs`

- [ ] **Step 1: 写失败测试**

文件内容（暂时只放 stub struct + 第一个测试）：

```rust
//! LRU preview cache：磁盘 + 内存双层，HTTP images 响应体长缓存。

use std::collections::HashMap;
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
```

- [ ] **Step 2: 跑测试，验证通过**

```bash
cd src-tauri && cargo test --lib services::preview_cache::tests
```

期望：2 个测试通过。

- [ ] **Step 3: 提交**

```bash
git add src-tauri/src/services/preview_cache.rs
git commit -m "feat(cache): PreviewCache skeleton + startup scan rebuilds LRU"
```

---

## Task 4: get_or_compute miss → compute → 写盘

**Files:**
- Modify: `src-tauri/src/services/preview_cache.rs`

- [ ] **Step 1: 加实现方法**

在 `impl PreviewCache` 块中追加：

```rust
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
        // Excess bytes beyond max_bytes: drain to 80% waterline by evicting LRU.
        let waterline = self.max_bytes * 80 / 100;
        while self.bytes_in_cache.load(Ordering::Relaxed) > waterline {
            if let Some((evicted_key, evicted)) = lru.pop_lru() {
                let ev_size = evicted.body.len() as u64;
                self.bytes_in_cache.fetch_sub(ev_size, Ordering::Relaxed);
                let _ = tokio::fs::remove_file(self.entry_path(&evicted_key)).await;
            } else {
                break;
            }
        }
        Ok(())
    }
```

- [ ] **Step 2: 加测试**

在 `mod tests` 内追加：

```rust
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
```

- [ ] **Step 3: 跑测试**

```bash
cd src-tauri && cargo test --lib services::preview_cache::tests
```

期望：4 个测试通过。

- [ ] **Step 4: 提交**

```bash
git add src-tauri/src/services/preview_cache.rs
git commit -m "feat(cache): get_or_compute with disk write + LRU insert"
```

---

## Task 5: mtime 隔离（同 file_id 不同 mtime）

**Files:**
- Modify: `src-tauri/src/services/preview_cache.rs`

- [ ] **Step 1: 加测试**

```rust
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
```

- [ ] **Step 2: 跑测试，验证通过**

```bash
cd src-tauri && cargo test --lib services::preview_cache::tests
```

期望：5 个测试通过。

- [ ] **Step 3: 提交**

```bash
git add src-tauri/src/services/preview_cache.rs
git commit -m "test(cache): different mtime keys are kept separate"
```

---

## Task 6: 容量超限 inline evict + gc() 公开 API

**Files:**
- Modify: `src-tauri/src/services/preview_cache.rs`

- [ ] **Step 1: 加 inline evict 测试**

```rust
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
```

- [ ] **Step 2: 提取 evict_to_waterline_locked + 暴露 gc()**

把 `insert()` 里的 eviction 循环提到一个私有 helper（接收 `&mut LruCache`，避免和 `insert()` 自身持锁时死锁），再加公开 `gc()`：

```rust
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
```

把 `insert()` 里原来的 inline while-loop 整段删掉，替换为末尾一句（仍在 `let mut lru = self.inner.lock().unwrap();` 作用域内）：

```rust
        self.evict_to_waterline_locked(&mut lru).await;
        Ok(())
    }
```

- [ ] **Step 3: 加 gc() 测试**

```rust
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
```

- [ ] **Step 4: 跑测试，验证通过**

```bash
cd src-tauri && cargo test --lib services::preview_cache::tests
```

期望：7 个测试通过（6 个 inline evict + 1 个 gc）。

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/services/preview_cache.rs
git commit -m "feat(cache): inline LRU evict + gc() public API"
```

---

## Task 7: invalidate(id) 清理该 file_id 所有 entries

**Files:**
- Modify: `src-tauri/src/services/preview_cache.rs`

- [ ] **Step 1: 加方法**

在 `impl PreviewCache` 内追加：

```rust
    pub fn invalidate(&self, id: i64) {
        let mut lru = self.inner.lock().unwrap();
        let keys: Vec<CacheKey> = lru.iter().map(|(k, _)| *k).filter(|(k_id, _)| *k_id == id).collect();
        for k in keys {
            if let Some(entry) = lru.pop(&k) {
                self.bytes_in_cache.fetch_sub(entry.body.len() as u64, Ordering::Relaxed);
            }
            let _ = std::fs::remove_file(self.entry_path(&k));
        }
    }
```

- [ ] **Step 2: 加测试**

```rust
    #[tokio::test]
    async fn invalidate_removes_all_entries_for_id() {
        let dir = tempfile::tempdir().unwrap();
        let cache = PreviewCache::new(dir.path(), 1024 * 1024).unwrap();
        let k1 = (5, mtime_from_unix(1_700_000_200));
        let k2 = (5, mtime_from_unix(1_700_000_201));
        let k3 = (6, mtime_from_unix(1_700_000_202));
        for k in [k1, k2, k3] {
            let _ = cache.get_or_compute(k, || async { Ok::<_, anyhow::Error>(b"x".to_vec()) }).await.unwrap();
        }
        assert_eq!(cache.bytes_in_cache(), 3);

        cache.invalidate(5);

        assert_eq!(cache.bytes_in_cache(), 1);
        assert!(cache.get(&k1).is_none());
        assert!(cache.get(&k2).is_none());
        assert!(cache.get(&k3).is_some());
        assert!(!dir.path().join("5-1700000200.json").exists());
        assert!(!dir.path().join("5-1700000201.json").exists());
        assert!(dir.path().join("6-1700000202.json").exists());
    }
```

- [ ] **Step 3: 跑测试**

```bash
cd src-tauri && cargo test --lib services::preview_cache::tests
```

期望：7 个测试通过。

- [ ] **Step 4: 提交**

```bash
git add src-tauri/src/services/preview_cache.rs
git commit -m "feat(cache): invalidate(id) clears all entries for that file"
```

---

## Task 8: 启动清理损坏文件

**Files:**
- Modify: `src-tauri/src/services/preview_cache.rs`

- [ ] **Step 1: 加 reload 时跳过 + 删除损坏文件**

修改 `reload_from_disk`：在文件名解析失败的分支加 remove_file。

```rust
        for entry in std::fs::read_dir(&self.dir)? {
            let entry = entry?;
            let p = entry.path();
            if !p.is_file() {
                continue;
            }
            let Some(name) = p.file_name().and_then(|s| s.to_str()) else {
                let _ = std::fs::remove_file(&p);
                continue;
            };
            let Some((id_str, mtime_str)) = name.split_once('-') else {
                let _ = std::fs::remove_file(&p);
                continue;
            };
            let (Ok(id), Ok(mtime_unix)) = (id_str.parse::<i64>(), mtime_str.parse::<u64>()) else {
                let _ = std::fs::remove_file(&p);
                continue;
            };
            let Some(mtime) = SystemTime::UNIX_EPOCH.checked_add(std::time::Duration::from_secs(mtime_unix)) else {
                let _ = std::fs::remove_file(&p);
                continue;
            };
            let body = match std::fs::read(&p) {
                Ok(b) => b,
                Err(_) => { let _ = std::fs::remove_file(&p); continue; }
            };
            let size = body.len() as u64;
            lru.put((id, mtime), CacheEntry { body, last_accessed: Instant::now() });
            bytes += size;
        }
```

- [ ] **Step 2: 加测试**

```rust
    #[test]
    fn reload_drops_malformed_files_and_keeps_good_ones() {
        let dir = tempfile::tempdir().unwrap();
        // 1 well-formed entry + 2 malformed
        std::fs::write(dir.path().join("3-555.json"), b"keepme").unwrap();
        std::fs::write(dir.path().join("garbage-not-id.json"), b"x").unwrap();
        std::fs::write(dir.path().join("not_a_file_at_all"), b"x").unwrap();

        let cache = PreviewCache::new(dir.path(), 1024 * 1024).unwrap();

        // Good entry loaded (6 bytes "keepme").
        assert_eq!(cache.bytes_in_cache(), 6);
        // Malformed deleted.
        assert!(!dir.path().join("garbage-not-id.json").exists());
        assert!(!dir.path().join("not_a_file_at_all").exists());
        assert!(dir.path().join("3-555.json").exists());
    }
```

- [ ] **Step 3: 跑测试**

```bash
cd src-tauri && cargo test --lib services::preview_cache::tests
```

期望：8 个测试通过。

- [ ] **Step 4: 提交**

```bash
git add src-tauri/src/services/preview_cache.rs
git commit -m "feat(cache): drop malformed entries on startup reload"
```

---

## Task 9: AppState 接 Arc<PreviewCache> + lib.rs spawn 后台 GC

**Files:**
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: 加 AppState 字段**

```rust
pub struct AppState {
    pub conn: DatabaseConnection,
    pub scanner: Arc<services::scanner::Scanner>,
    pub covers_dir: Arc<std::path::PathBuf>,
    pub config: config::AppConfig,
    /// Bearer token. `RwLock` so `regenerate_auth_token` can swap the
    /// value at runtime without dropping HTTP requests.
    pub auth_token: Arc<RwLock<String>>,
    /// LRU preview cache（磁盘 + 内存双层）。HTTP images 端点共用。
    pub preview_cache: Arc<services::preview_cache::PreviewCache>,
}
```

- [ ] **Step 2: 在 `run()` 中初始化 + spawn GC**

在 `lib.rs::run` 现有 `cfg.ensure_dirs().ok();` 之后加：

```rust
    let preview_cache = Arc::new(
        services::preview_cache::PreviewCache::new(
            &cfg.preview_cache_dir(),
            cfg.preview_cache_max_bytes,
        )
        .unwrap_or_else(|e| {
            eprintln!("preview_cache init fallback to empty: {:?}", e);
            services::preview_cache::PreviewCache::new(
                std::path::Path::new("."),
                cfg.preview_cache_max_bytes,
            ).expect("inline empty cache")
        })
    );

    // Background GC: every 30s, drain to 80% waterline if over budget.
    {
        let cache_for_gc = preview_cache.clone();
        tauri::async_runtime::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(30)).await;
                if let Err(e) = cache_for_gc.gc().await {
                    eprintln!("preview_cache gc failed: {:?}", e);
                }
            }
        });
    }
```

把 `preview_cache.clone()` 一份给 `AppState` 构造：

```rust
    let state = AppState {
        conn: conn.clone(),
        scanner: scanner.clone(),
        covers_dir,
        config: cfg_clone,
        auth_token: auth_token.clone(),
        preview_cache: preview_cache.clone(),
    };
```

- [ ] **Step 3: 跑 build，验证编译**

```bash
cd src-tauri && cargo build
```

期望：成功。需要给 `services::preview_cache` 模块加上 pub mod 声明（如果还没有）；检查 `lib.rs` 顶部 `pub mod services;` 与 `services/mod.rs` 内是否 `pub mod preview_cache;`。

- [ ] **Step 4: 跑既有测试，验证无回归**

```bash
cd src-tauri && cargo test --lib
```

期望：所有现有 + preview_cache 8 个测试全过。

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/lib.rs src-tauri/src/services/preview_cache.rs src-tauri/src/services/mod.rs
git commit -m "feat(lib): wire PreviewCache into AppState + spawn background GC"
```

---

## Task 10: images handler 接 cache + ETag

**Files:**
- Modify: `src-tauri/src/http/api.rs`

- [ ] **Step 1: 加 ETag + cache peek 路径**

替换现有 `pub async fn images` 函数体为：

```rust
pub async fn images(
    State(s): State<ApiState>,
    Path(id): Path<i64>,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    use sea_orm::EntityTrait;
    let row = match doujinshi_file::Entity::find_by_id(id).one(&s.conn).await {
        Ok(Some(r)) => r,
        Ok(None) => return (StatusCode::NOT_FOUND, "no file").into_response(),
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };
    let path = std::path::Path::new(&row.current_path);
    if !path.exists() {
        return (
            StatusCode::OK,
            [(header::ETAG, format!("\"{}-missing\"", id))],
            Json(json!(ImagesResponse {
                file_id: id,
                images: vec![],
                zip_missing: true,
            })),
        )
            .into_response();
    }

    // mtime → ETag. Zip changed → mtime moved → cache miss; no manual invalidation needed.
    let mtime = match path.metadata().and_then(|m| m.modified()) {
        Ok(t) => t,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };
    let etag = format!(
        "\"{}-{}\"",
        id,
        mtime.duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    );

    // If-None-Match → 304
    if let Some(if_none_match) = headers.get(header::IF_NONE_MATCH).and_then(|v| v.to_str().ok()) {
        if if_none_match == etag {
            return (
                StatusCode::NOT_MODIFIED,
                [(header::ETAG, etag.clone())],
            )
                .into_response();
        }
    }

    let key: services::preview_cache::CacheKey = (id, mtime);

    // Try cache.
    if let Some(body) = s.preview_cache.get(&key) {
        return (
            StatusCode::OK,
            [
                (header::CONTENT_TYPE, "application/json"),
                (header::ETAG, etag.clone()),
            ],
            body,
        )
            .into_response();
    }

    // Compute.
    let entries = match crate::services::archive::list_images(path) {
        Ok(e) => e,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };
    let images: Vec<ImageEntry> = entries
        .into_iter()
        .map(|e| {
            let b64 = base64::engine::general_purpose::STANDARD.encode(&e.data);
            ImageEntry {
                data_url: format!("data:image/{};base64,{}", guess_image_ext(&e.name), b64),
                name: e.name,
            }
        })
        .collect();
    let response = ImagesResponse { file_id: id, images, zip_missing: false };
    let body = match serde_json::to_vec(&response) {
        Ok(b) => b,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };

    // Store (async — fire and forget on tokio runtime).
    let cache_for_write = s.preview_cache.clone();
    let body_for_write = body.clone();
    tokio::spawn(async move {
        if let Err(e) = cache_for_write
            .get_or_compute(key, || async { Ok::<_, anyhow::Error>(body_for_write) })
            .await
        {
            eprintln!("preview_cache write failed: {:?}", e);
        }
    });

    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "application/json"),
            (header::ETAG, etag.clone()),
        ],
        body,
    )
        .into_response()
}
```

**注**：在文件顶部 `use` 段加：

```rust
use std::time::SystemTime;
use crate::http::ApiState;
```

确保 `services::preview_cache::CacheKey` 等引用可见（`pub` 已加）。

- [ ] **Step 2: 跑 build 验证**

```bash
cd src-tauri && cargo build
```

期望：成功。

- [ ] **Step 3: 跑既有 HTTP 测试，验证无回归**

```bash
cd src-tauri && cargo test --test http_routes --lib
```

期望：所有既有 26 个 http_routes 测试通过。

- [ ] **Step 4: 提交**

```bash
git add src-tauri/src/http/api.rs
git commit -m "feat(http): images handler adds ETag + LRU cache check"
```

---

## Task 11: 测试 harness 加 PreviewCache + HTTP 304 测试

**Files:**
- Modify: `src-tauri/tests/common/mod.rs`
- Modify: `src-tauri/tests/http_routes.rs`

- [ ] **Step 1: harness 加 PreviewCache**

`tests/common/mod.rs::build_state_with_token` 中，给 `ApiState` 加 `preview_cache` 字段。找到构造 `ApiState` 的位置（V3 后 ApiState 含 identified_dir / will_delete_dir / archived_dir 等 Arc），在 struct 初始化里加：

```rust
    let preview_cache = Arc::new(
        doujinshi_records::services::preview_cache::PreviewCache::new(
            &resources_dir.path().join("_preview_cache"),
            1024 * 1024, // 1 MiB for tests
        )
        .unwrap()
    );
```

以及把 `preview_cache: preview_cache.clone()` 加到 `ApiState { ... }` 的字段列表。注意 `tests/common/mod.rs` 里 `build_test_router` 还有第二个 `ApiState { ... }`，那里也得加 `preview_cache` 字段（直接 `Arc::new(...test_path...)`）。

- [ ] **Step 2: 加 304 测试**

`tests/http_routes.rs` 末尾追加：

```rust
#[tokio::test]
async fn images_endpoint_returns_304_when_etag_matches() {
    let h = build_state().await;
    // Seed a doujinshi_file row with current_path pointing at a real zip.
    let zip_path = h.resources_dir.path().join("real.zip");
    std::fs::write(&zip_path, b"PK").unwrap(); // minimal zip magic; list_images will return [].
    let hash = "c001c001c001c001c001c001c001c001c001c001c001c001c001c001c001c001";
    let now = chrono::Utc::now();
    let am = doujinshi_file::ActiveModel {
        title: Set("e2e".into()),
        filename: Set("real.zip".into()),
        hash: Set(hash.into()),
        ext: Set("zip".into()),
        size_bytes: Set(2),
        current_path: Set(zip_path.to_string_lossy().into_owned()),
        current_location: Set("identified".into()),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    };
    let m = am.insert(&h.state.conn).await.unwrap();
    let id = m.id;

    // First request → 200 + ETag header.
    let resp = router(h.state.clone())
        .oneshot(authed_request("GET", &format!("/api/doujinshi/{}/images", id)))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let etag = resp.headers().get("etag").unwrap().to_str().unwrap().to_string();
    assert!(etag.starts_with(&format!("\"{}-", id)));

    // Second request with If-None-Match → 304.
    let req = Request::builder()
        .method("GET")
        .uri(format!("/api/doujinshi/{}/images", id))
        .header("authorization", format!("Bearer {}", TEST_TOKEN))
        .header("if-none-match", etag.clone())
        .body(Body::empty())
        .unwrap();
    let resp2 = router(h.state.clone()).oneshot(req).await.unwrap();
    assert_eq!(resp2.status(), StatusCode::NOT_MODIFIED);
}

#[tokio::test]
async fn images_endpoint_serves_cached_response_on_second_hit() {
    let h = build_state().await;
    let zip_path = h.resources_dir.path().join("real.zip");
    std::fs::write(&zip_path, b"PK").unwrap();
    let hash = "c002c002c002c002c002c002c002c002c002c002c002c002c002c002c002c002";
    let now = chrono::Utc::now();
    let am = doujinshi_file::ActiveModel {
        title: Set("cached".into()),
        filename: Set("real.zip".into()),
        hash: Set(hash.into()),
        ext: Set("zip".into()),
        size_bytes: Set(2),
        current_path: Set(zip_path.to_string_lossy().into_owned()),
        current_location: Set("identified".into()),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    };
    let m = am.insert(&h.state.conn).await.unwrap();
    let id = m.id;

    // Two back-to-back requests: cache should serve second from disk.
    let url = format!("/api/doujinshi/{}/images", id);
    let resp1 = router(h.state.clone()).oneshot(authed_request("GET", &url)).await.unwrap();
    assert_eq!(resp1.status(), StatusCode::OK);

    // Right after the first response, the disk cache file should exist.
    let cache_dir = h.resources_dir.path().join("_preview_cache");
    let entry_count = std::fs::read_dir(&cache_dir).unwrap().filter(|e| e.is_ok()).count();
    assert!(entry_count >= 1, "expected at least one cache entry on disk");

    let _resp2 = router(h.state).oneshot(authed_request("GET", &url)).await.unwrap();
}
```

加必要 `use`：

```rust
use http_body_util::BodyExt; // already imported above; check
```

如果需要导入 `use std::sync::Arc;`（确认全 test 文件已有）。

- [ ] **Step 3: 跑测试，验证通过**

```bash
cd src-tauri && cargo test --test http_routes
```

期望：现有 26 + 新 2 = 28 个测试通过。

- [ ] **Step 4: 提交**

```bash
git add src-tauri/tests/common/mod.rs src-tauri/tests/http_routes.rs
git commit -m "test(http): 304 If-None-Match + second-hit cache test"
```

---

## Task 12: 全量回归 + 文档同步

**Files:**
- Modify: `CLAUDE.md`（一行：架构要点加 preview cache 模块名）

- [ ] **Step 1: 全量 cargo test**

```bash
cd src-tauri && cargo test --test http_routes --test inbox_resolve --test migrations --test http_bind --test migrate_v3 --lib
```

期望：所有测试通过；preview_cache 8 + http_routes 28 + 既有通过。

- [ ] **Step 2: 前端 type check + build**

```bash
cd /d/NewCode/doujinshi-records && pnpm exec vue-tsc --noEmit && pnpm build
```

期望：0 错误。

- [ ] **Step 3: 文档同步**

`CLAUDE.md` 架构要点的"services"列举里加 `preview_cache`；services/test 中加 preview_cache 模块。`关键文档`段加 V3.1 spec + V3.1 plan。

- [ ] **Step 4: 提交**

```bash
git add CLAUDE.md
git commit -m "docs: sync CLAUDE.md with V3.1 PreviewCache"
```
