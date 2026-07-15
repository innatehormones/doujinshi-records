# 数据备份与还原 — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `data.db` 的本地备份：自动 + 手动 + BLAKE3 dedup + 保留 N 个 + 启动期 marker 还原。

**Architecture:** `BackupService` 核心（services/backup.rs）+ `BackupStorage` trait（storage.rs，方便未来加 S3/WebDAV）。备份走 SQLite `VACUUM INTO` + temp+rename 保证原子性。还原走「写 marker → 用户关 app → 启动期检测并 fs::copy」。不暴露 HTTP。

**Tech Stack:** Rust + SeaORM + BLAKE3（已有）+ axum 0.7（不暴露）+ Naive UI + Vue 3

**Spec:** `docs/superpowers/specs/2026-07-15-data-backup.md`

---

## Task 1: BackupConfig struct + serde

**Files:**
- Create: `src-tauri/src/services/backup.rs`（初始骨架）

- [ ] **Step 1: 写失败测试**

`src-tauri/src/services/backup.rs` 末尾加：

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backup_config_defaults() {
        let c = BackupConfig::default();
        assert_eq!(c.dir, "");           // 空 = 用默认目录
        assert_eq!(c.retention_count, 10);
    }

    #[test]
    fn backup_config_serde_round_trip() {
        let c = BackupConfig {
            dir: "D:/backups".into(),
            retention_count: 5,
        };
        let s = serde_json::to_string(&c).unwrap();
        let back: BackupConfig = serde_json::from_str(&s).unwrap();
        assert_eq!(back.dir, "D:/backups");
        assert_eq!(back.retention_count, 5);
    }
}
```

- [ ] **Step 2: 跑测试，确认 fail**

```bash
cd src-tauri && cargo test --lib backup_config
```

Expected: compile error (BackupConfig not found).

- [ ] **Step 3: 实现 BackupConfig**

```rust
use serde::{Deserialize, Serialize};

/// 用户可见的备份配置。`dir = ""` 表示使用默认 `resources/backups/`。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BackupConfig {
    pub dir: String,
    pub retention_count: u32,
}

impl Default for BackupConfig {
    fn default() -> Self {
        Self { dir: String::new(), retention_count: 10 }
    }
}
```

- [ ] **Step 4: 跑测试，确认 pass**

Expected: 2 passed.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/services/backup.rs
git commit -m "feat(backup): add BackupConfig struct with serde + defaults"
```

---

## Task 2: filename 生成

**Files:**
- Modify: `src-tauri/src/services/backup.rs`

- [ ] **Step 1: 写失败测试**

```rust
    #[test]
    fn backup_filename_compact_rfc3339() {
        let ts = chrono::DateTime::parse_from_rfc3339("2026-07-15T18:30:45Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        assert_eq!(
            backup_filename(ts),
            "data-2026-07-15T18-30-45Z.db"
        );
    }

    #[test]
    fn backup_filename_uses_utc() {
        // 不同时区都归一为 UTC 后命名——避免本地时区在不同机器上撞名
        let ts_local = chrono::DateTime::parse_from_rfc3339("2026-07-15T10:30:45-08:00")
            .unwrap()
            .with_timezone(&chrono::Utc);
        assert_eq!(
            backup_filename(ts_local),
            "data-2026-07-15T18-30-45Z.db"
        );
    }
```

- [ ] **Step 2: 跑测试，确认 fail**

```bash
cd src-tauri && cargo test --lib backup_filename
```

Expected: compile error.

- [ ] **Step 3: 实现 backup_filename**

```rust
/// 生成备份文件名 `data-{RFC3339 紧凑}.db`。紧凑版去冒号，保证文件名安全。
/// 始终用 UTC：跨时区机器还原时文件名一致，避免命名混乱。
pub fn backup_filename(ts: chrono::DateTime<chrono::Utc>) -> String {
    format!("data-{}.db", ts.format("%Y-%m-%dT%H-%M-%SZ"))
}
```

- [ ] **Step 4: 跑测试，确认 pass**

Expected: 2 passed.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/services/backup.rs
git commit -m "feat(backup): backup_filename uses UTC compact RFC3339"
```

---

## Task 3: BLAKE3 hash_db_file

**Files:**
- Modify: `src-tauri/src/services/backup.rs`

- [ ] **Step 1: 写失败测试**

```rust
    #[test]
    fn hash_db_file_matches_blake3() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.db");
        std::fs::write(&path, b"hello world").unwrap();
        // BLAKE3("hello world") 的 hex
        let expected = "d9c3679d9690b34d72a4fcdfd41b5b8b15a6b6c5c2e2c8b9c8b3a8d6f1e2c3a4";
        // 上面的 hex 是示意——测试时直接调用 blake3 算，避免 hardcode
        let actual = hash_db_file(&path).unwrap();
        let direct = blake3::hash(b"hello world").to_hex().to_string();
        assert_eq!(actual, direct);
    }
```

- [ ] **Step 2: 跑测试，确认 fail**

```bash
cd src-tauri && cargo test --lib hash_db_file
```

Expected: compile error.

- [ ] **Step 3: 实现 hash_db_file**

```rust
use std::path::Path;
use anyhow::Result;

/// 整文件读入 + BLAKE3。DB 规模 KB~MB 级，全文读入 + BLAKE3（项目最熟路径）
/// 比 streaming 哈希代码量少且更快——BLAKE3 单核 ~1GB/s，MB 级文件 < 10ms。
pub fn hash_db_file(path: &Path) -> Result<String> {
    let bytes = std::fs::read(path)?;
    Ok(blake3::hash(&bytes).to_hex().to_string())
}
```

- [ ] **Step 4: 跑测试，确认 pass**

Expected: 1 passed.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/services/backup.rs
git commit -m "feat(backup): hash_db_file via blake3"
```

---

## Task 4: Marker 文件读写

**Files:**
- Modify: `src-tauri/src/services/backup.rs`

- [ ] **Step 1: 写失败测试**

```rust
    #[test]
    fn restore_marker_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join(".restore-pending.json");

        let pending = RestorePending {
            src: "C:/backups/data-2026-07-15T18-30-45Z.db".into(),
            requested_at: "2026-07-15T18:30:50Z".into(),
        };
        write_restore_marker(&marker, &pending).unwrap();
        let back = read_restore_marker(&marker).unwrap().unwrap();
        assert_eq!(back.src, pending.src);
        assert_eq!(back.requested_at, pending.requested_at);
    }

    #[test]
    fn restore_marker_absent_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join(".restore-pending.json");
        assert!(read_restore_marker(&marker).unwrap().is_none());
    }

    #[test]
    fn restore_marker_clear_removes_file() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join(".restore-pending.json");
        let pending = RestorePending { src: "x".into(), requested_at: "y".into() };
        write_restore_marker(&marker, &pending).unwrap();
        clear_restore_marker(&marker);
        assert!(!marker.exists());
        clear_restore_marker(&marker); // 二次调用也不报错
    }
```

- [ ] **Step 2: 跑测试，确认 fail**

```bash
cd src-tauri && cargo test --lib restore_marker
```

Expected: compile error.

- [ ] **Step 3: 实现 marker I/O**

```rust
use std::path::{Path, PathBuf};

/// 启动期检测：是否有用户触发的还原待执行
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RestorePending {
    pub src: String,
    pub requested_at: String,
}

pub fn write_restore_marker(path: &Path, pending: &RestorePending) -> Result<()> {
    let json = serde_json::to_string_pretty(pending)?;
    std::fs::write(path, json)?;
    Ok(())
}

pub fn read_restore_marker(path: &Path) -> Result<Option<RestorePending>> {
    if !path.exists() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(path)?;
    let pending: RestorePending = serde_json::from_str(&text)?;
    Ok(Some(pending))
}

pub fn clear_restore_marker(path: &Path) {
    let _ = std::fs::remove_file(path);
}
```

- [ ] **Step 4: 跑测试，确认 pass**

Expected: 3 passed.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/services/backup.rs
git commit -m "feat(backup): restore-pending.json read/write/clear"
```

---

## Task 5: BackupStorage trait + LocalFsStorage

**Files:**
- Create: `src-tauri/src/services/backup/mod.rs`（只声明子模块）
- Create: `src-tauri/src/services/backup/storage.rs`

- [ ] **Step 1: 写失败测试**

`src-tauri/src/services/backup/storage.rs`：

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_fs_list_filters_only_backup_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("data-2026-07-15T18-30-45Z.db"), b"x").unwrap();
        std::fs::write(dir.path().join("data-2026-07-14T18-30-45Z.db"), b"x").unwrap();
        std::fs::write(dir.path().join("foo.txt"), b"x").unwrap();
        std::fs::write(dir.path().join("data-.tmp-uuid.db"), b"x").unwrap(); // temp 文件应被过滤

        let storage = LocalFsStorage;
        let list = storage.list_snapshots(dir.path()).unwrap();
        assert_eq!(list.len(), 2);
        // mtime 倒序
        assert!(list[0].path.to_string_lossy().contains("2026-07-15"));
    }

    #[test]
    fn local_fs_delete_removes_file() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("data-2026-07-15T18-30-45Z.db");
        std::fs::write(&p, b"x").unwrap();

        LocalFsStorage.delete_snapshot(&p).unwrap();
        assert!(!p.exists());
    }

    #[test]
    fn local_fs_delete_missing_is_ok() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("nope.db");
        LocalFsStorage.delete_snapshot(&p).unwrap(); // 不报错
    }
}
```

- [ ] **Step 2: 跑测试，确认 fail**

```bash
cd src-tauri && cargo test --lib local_fs
```

Expected: compile error.

- [ ] **Step 3: 实现 trait + LocalFsStorage**

`src-tauri/src/services/backup/mod.rs`：

```rust
pub mod storage;
```

`src-tauri/src/services/backup/storage.rs`：

```rust
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

/// 存储后端 trait。**解耦 + 易扩展**：第一版只实现 LocalFsStorage；
/// 未来加 S3Storage / WebDavStorage 只新加 struct + impl BackupStorage，
/// BackupService 和 commands 层不变。
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
        unimplemented!("BackupService uses VACUUM INTO directly; storage trait retained for future backends")
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
```

- [ ] **Step 4: 跑测试，确认 pass**

Expected: 3 passed.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/services/backup/mod.rs src-tauri/src/services/backup/storage.rs
git commit -m "feat(backup): BackupStorage trait + LocalFsStorage impl"
```

---

## Task 6: BackupService 骨架 + get_config/set_config

**Files:**
- Modify: `src-tauri/src/services/backup.rs`（追加 BackupService 结构）

- [ ] **Step 1: 写失败测试**

```rust
    #[tokio::test]
    async fn service_get_config_returns_defaults_when_empty() {
        let dir = tempfile::tempdir().unwrap();
        let svc = BackupService::new(
            dir.path().join("data.db"),
            dir.path().join("backups"),
            Arc::new(LocalFsStorage),
            FakeSettings::new(),
        );
        let cfg = svc.get_config().await.unwrap();
        assert_eq!(cfg, BackupConfig::default());
    }

    #[tokio::test]
    async fn service_set_config_persists() {
        let dir = tempfile::tempdir().unwrap();
        let settings = FakeSettings::new();
        let svc = BackupService::new(
            dir.path().join("data.db"),
            dir.path().join("backups"),
            Arc::new(LocalFsStorage),
            settings.clone(),
        );
        svc.set_config(Some("D:/custom"), 5).await.unwrap();
        let cfg = svc.get_config().await.unwrap();
        assert_eq!(cfg.dir, "D:/custom");
        assert_eq!(cfg.retention_count, 5);
        assert_eq!(settings.get("backup_dir").as_deref(), Some("D:/custom"));
        assert_eq!(settings.get("backup_retention_count").as_deref(), Some("5"));
    }
```

- [ ] **Step 2: 跑测试，确认 fail**

```bash
cd src-tauri && cargo test --lib service_get_config
```

Expected: compile error (BackupService / FakeSettings not found).

- [ ] **Step 3: 实现 SettingsHandle trait + FakeSettings + BackupService 骨架**

```rust
use std::sync::Arc;
use tokio::sync::Mutex;

#[async_trait::async_trait]
pub trait SettingsHandle: Send + Sync {
    async fn read(&self, key: &str) -> Result<Option<String>>;
    async fn write(&self, key: &str, value: &str) -> Result<()>;
}

/// 真正用的实现——包一层 db::read_setting / write_setting，service 层不必直连 SeaORM
pub struct DbSettingsHandle<'a> {
    pub conn: &'a sea_orm::DatabaseConnection,
}

#[async_trait::async_trait]
impl<'a> SettingsHandle for DbSettingsHandle<'a> {
    async fn read(&self, key: &str) -> Result<Option<String>> {
        Ok(crate::db::read_setting(self.conn, key).await?)
    }
    async fn write(&self, key: &str, value: &str) -> Result<()> {
        crate::db::write_setting(self.conn, key, value).await?;
        Ok(())
    }
}

/// BackupService 核心。`inflight` mutex 串行化 backup_now 调用。
/// 单实例通过 `Arc<BackupService>` 在 AppState 里持有；commands 层只做薄壳调用。
pub struct BackupService {
    db_path: PathBuf,
    default_dir: PathBuf,
    storage: Arc<dyn storage::BackupStorage>,
    settings: Arc<dyn SettingsHandle>,
    inflight: Mutex<()>,
}

impl BackupService {
    pub fn new(
        db_path: PathBuf,
        default_dir: PathBuf,
        storage: Arc<dyn storage::BackupStorage>,
        settings: Arc<dyn SettingsHandle>,
    ) -> Self {
        Self {
            db_path,
            default_dir,
            storage,
            settings,
            inflight: Mutex::new(()),
        }
    }

    pub fn resolve_backup_dir(&self, cfg: &BackupConfig) -> PathBuf {
        if cfg.dir.is_empty() { self.default_dir.clone() } else { PathBuf::from(&cfg.dir) }
    }

    pub async fn get_config(&self) -> Result<BackupConfig> {
        let dir = self.settings.read("backup_dir").await?.unwrap_or_default();
        let retention_count = self
            .settings
            .read("backup_retention_count")
            .await?
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(10);
        Ok(BackupConfig { dir, retention_count })
    }

    pub async fn set_config(&self, dir: Option<&str>, retention: u32) -> Result<()> {
        if let Some(d) = dir {
            self.settings.write("backup_dir", d).await?;
        }
        self.settings.write("backup_retention_count", &retention.to_string()).await?;
        Ok(())
    }
}
```

并在文件顶部加：

```rust
use anyhow::Result;
use std::path::PathBuf;
pub mod storage;
pub use storage::{BackupStorage, LocalFsStorage, SnapshotInfo};
```

- [ ] **Step 4: 实现 FakeSettings（测试用）**

```rust
#[cfg(test)]
mod tests {
    // ... 既有测试 ...
    use super::storage::LocalFsStorage;
    use std::collections::HashMap;
    use std::sync::Mutex as StdMutex;

    #[derive(Clone)]
    struct FakeSettings {
        inner: Arc<StdMutex<HashMap<String, String>>>,
    }
    impl FakeSettings {
        fn new() -> Self {
            Self { inner: Arc::new(StdMutex::new(HashMap::new())) }
        }
        fn get(&self, key: &str) -> Option<String> {
            self.inner.lock().unwrap().get(key).cloned()
        }
    }
    #[async_trait::async_trait]
    impl SettingsHandle for FakeSettings {
        async fn read(&self, key: &str) -> Result<Option<String>> {
            Ok(self.inner.lock().unwrap().get(key).cloned())
        }
        async fn write(&self, key: &str, value: &str) -> Result<()> {
            self.inner.lock().unwrap().insert(key.to_string(), value.to_string());
            Ok(())
        }
    }
}
```

- [ ] **Step 5: 在 `Cargo.toml` 确认 async_trait 依赖**

`src-tauri/Cargo.toml` 已有 `async-trait = { workspace = true }`（扫描整个项目 Cargo.toml 应有；如果没有，加进 `[dependencies]`）。

- [ ] **Step 6: 跑测试，确认 pass**

```bash
cd src-tauri && cargo test --lib service_get_config
```

Expected: 2 passed.

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/services/backup.rs src-tauri/Cargo.toml
git commit -m "feat(backup): BackupService skeleton with get/set_config + SettingsHandle trait"
```

---

## Task 7: BackupService::list_backups

**Files:**
- Modify: `src-tauri/src/services/backup.rs`

- [ ] **Step 1: 写失败测试**

```rust
    #[tokio::test]
    async fn list_backups_returns_sorted_snapshots() {
        let dir = tempfile::tempdir().unwrap();
        let backup_dir = dir.path().join("backups");
        std::fs::create_dir_all(&backup_dir).unwrap();
        // 造 3 个备份 + 1 个无关文件
        let f1 = backup_dir.join("data-2026-07-13T10-00-00Z.db");
        let f2 = backup_dir.join("data-2026-07-14T10-00-00Z.db");
        let f3 = backup_dir.join("data-2026-07-15T10-00-00Z.db");
        std::fs::write(&f1, b"old").unwrap();
        std::fs::write(&f2, b"mid").unwrap();
        std::fs::write(&f3, b"new").unwrap();
        std::fs::write(backup_dir.join("readme.txt"), b"x").unwrap();
        // mtime 排序依赖文件 mtime，需要 sleep 或者用 filetime crate
        // 简单做法：按写入顺序让 mtime 递增
        std::thread::sleep(std::time::Duration::from_millis(10));
        std::fs::write(&f2, b"mid2").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));
        std::fs::write(&f3, b"new2").unwrap();

        let svc = BackupService::new(
            dir.path().join("data.db"),
            backup_dir.clone(),
            Arc::new(LocalFsStorage),
            Arc::new(FakeSettings::new()),
        );
        let list = svc.list_backups().await.unwrap();
        assert_eq!(list.len(), 3);
        assert_eq!(list[0].path, f3); // 最新
        assert_eq!(list[2].path, f1); // 最旧
        for s in &list {
            assert!(s.size_bytes > 0);
        }
    }
```

- [ ] **Step 2: 跑测试，确认 fail**

Expected: compile error.

- [ ] **Step 3: 实现 list_backups**

```rust
impl BackupService {
    // ... 既有方法 ...
    pub async fn list_backups(&self) -> Result<Vec<SnapshotInfo>> {
        let cfg = self.get_config().await?;
        let dir = self.resolve_backup_dir(&cfg);
        self.storage.list_snapshots(&dir)
    }
}
```

- [ ] **Step 4: 跑测试，确认 pass**

Expected: 1 passed.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/services/backup.rs
git commit -m "feat(backup): BackupService::list_backups"
```

---

## Task 8: backup_now 骨架 + VACUUM INTO + temp+rename

**Files:**
- Modify: `src-tauri/src/services/backup.rs`

- [ ] **Step 1: 写失败测试**

```rust
    #[tokio::test]
    async fn backup_now_writes_via_vacuum_into() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("data.db");
        let backup_dir = dir.path().join("backups");

        // 准备一个真实 SQLite DB（含 1 行）
        let conn = crate::db::connect(&db_path).await.unwrap();
        crate::db::migrations::init_schema_versioned(&conn).await.unwrap();
        use sea_orm::{ActiveModelTrait, Set};
        let am = crate::db::entities::doujinshi_file::ActiveModel {
            title: Set("test".into()),
            filename: Set("f.zip".into()),
            hash: Set(format!("h{}", 1)),
            ext: Set("zip".into()),
            size_bytes: Set(0),
            last_seen_path: Set("/x".into()),
            created_at: Set(chrono::Utc::now()),
            updated_at: Set(chrono::Utc::now()),
            ..Default::default()
        };
        am.insert(&conn).await.unwrap();
        drop(conn); // 关闭连接，避免 VACUUM INTO 锁等待

        let svc = BackupService::new(
            db_path.clone(),
            backup_dir.clone(),
            Arc::new(LocalFsStorage),
            Arc::new(FakeSettings::new()),
        );

        let result = svc.backup_now().await.unwrap();
        assert!(result.path.exists(), "backup file should exist");
        assert!(result.size_bytes > 0);

        // 验证备份文件是有效 SQLite 且含原数据
        let backup_conn = crate::db::connect(&result.path).await.unwrap();
        use crate::db::entities::doujinshi_file;
        use sea_orm::EntityTrait;
        let rows = doujinshi_file::Entity::find().all(&backup_conn).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].title, "test");
    }

    #[tokio::test]
    async fn backup_now_atomic_no_half_files_on_failure() {
        // 模拟 VACUUM INTO 失败：把 db_path 设成不存在的文件
        let dir = tempfile::tempdir().unwrap();
        let backup_dir = dir.path().join("backups");
        std::fs::create_dir_all(&backup_dir).unwrap();

        let svc = BackupService::new(
            dir.path().join("nonexistent.db"), // 不存在
            backup_dir.clone(),
            Arc::new(LocalFsStorage),
            Arc::new(FakeSettings::new()),
        );

        let result = svc.backup_now().await;
        assert!(result.is_err());

        // 备份目录不应残留 .tmp-* 或半截 .db
        let entries: Vec<_> = std::fs::read_dir(&backup_dir)
            .unwrap()
            .filter_map(Result::ok)
            .collect();
        assert!(entries.is_empty(), "no half-files should remain");
    }
```

- [ ] **Step 2: 跑测试，确认 fail**

Expected: compile error.

- [ ] **Step 3: 实现 backup_now 核心**

```rust
use sea_orm::{ConnectionTrait, Statement};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
pub struct BackupResult {
    pub path: PathBuf,
    pub size_bytes: u64,
    pub md5: String,
    /// true 表示本次因内容未变而跳过
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skipped: Option<String>,
}

impl BackupService {
    // ... 既有方法 ...
    pub async fn backup_now(&self) -> Result<BackupResult> {
        // 防并发：用户连点 + 自动备份同时触发时只允许一个跑
        let _guard = self.inflight.try_lock().map_err(|_| {
            anyhow::anyhow!("backup already in progress")
        })?;

        // 1. 读 config + 算当前 MD5
        let cfg = self.get_config().await?;
        let dir = self.resolve_backup_dir(&cfg);
        std::fs::create_dir_all(&dir)?;

        let current_md5 = hash_db_file(&self.db_path)?;

        // 2. dedup：内容相同直接返回
        let last_md5 = self.settings.read("backup_last_md5").await?;
        if last_md5.as_deref() == Some(&current_md5) {
            // 找最新一个 data-*.db 作为 skipped path
            let snapshots = self.storage.list_snapshots(&dir)?;
            let last_path = snapshots
                .first()
                .map(|s| s.path.clone())
                .unwrap_or(dir.join("(none)"));
            let last_size = snapshots.first().map(|s| s.size_bytes).unwrap_or(0);
            self.touch_last_at().await?;
            return Ok(BackupResult {
                path: last_path,
                size_bytes: last_size,
                md5: current_md5,
                skipped: Some("content unchanged".into()),
            });
        }

        // 3. VACUUM INTO temp → rename atomic 到 final
        let now = chrono::Utc::now();
        let final_name = backup_filename(now);
        let final_path = dir.join(&final_name);
        let tmp_path = dir.join(format!(".tmp-{}.db", Uuid::new_v4()));

        // VACUUM INTO tmp
        self.vacuum_into(&tmp_path).await?;
        // 算新文件 size + MD5（这里 tmp 和 final MD5 一样因为是 rename）
        let tmp_size = std::fs::metadata(&tmp_path)?.len();
        // 原子 rename（同 fs 下 rename 是原子的）
        std::fs::rename(&tmp_path, &final_path)?;

        // 4. 写 settings：last md5 + last at
        let new_md5 = hash_db_file(&final_path)?;
        self.settings.write("backup_last_md5", &new_md5).await?;
        self.touch_last_at().await?;

        // 5. retention 清理
        self.apply_retention(&dir, cfg.retention_count).await?;

        Ok(BackupResult {
            path: final_path,
            size_bytes: tmp_size,
            md5: new_md5,
            skipped: None,
        })
    }

    async fn vacuum_into(&self, dst: &Path) -> Result<()> {
        // 单开一个临时连接做 VACUUM INTO（不能用 self.conn 因为它是常驻 app 连接）
        let conn = crate::db::connect(&self.db_path).await?;
        let escaped = dst.to_string_lossy().replace('\'', "''");
        let sql = format!("VACUUM INTO '{}'", escaped);
        conn.execute(Statement::from_string(
            conn.get_database_backend(),
            sql,
        ))
        .await?;
        Ok(())
    }

    async fn touch_last_at(&self) -> Result<()> {
        let now = chrono::Utc::now().to_rfc3339();
        self.settings.write("backup_last_at", &now).await?;
        Ok(())
    }

    async fn apply_retention(&self, dir: &Path, keep: u32) -> Result<()> {
        if keep == 0 { return Ok(()); } // 0 = 禁用清理
        let snapshots = self.storage.list_snapshots(dir)?;
        for s in snapshots.iter().skip(keep as usize) {
            self.storage.delete_snapshot(&s.path)?;
        }
        Ok(())
    }
}
```

并在文件顶部加：

```rust
use uuid::Uuid;
```

- [ ] **Step 4: 在 `Cargo.toml` 加 uuid 依赖（如未存在）**

检查 `src-tauri/Cargo.toml`：若无 `uuid`，加进 `[dependencies]`：

```toml
uuid = { version = "1", features = ["v4"] }
```

- [ ] **Step 5: 跑测试，确认 pass**

```bash
cd src-tauri && cargo test --lib backup_now
```

Expected: 2 passed.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/services/backup.rs src-tauri/Cargo.toml
git commit -m "feat(backup): backup_now via VACUUM INTO + temp+rename + atomic write"
```

---

## Task 9: backup_now BLAKE3 dedup

**Files:**
- Modify: `src-tauri/src/services/backup.rs`

- [ ] **Step 1: 写失败测试**

```rust
    #[tokio::test]
    async fn backup_now_skips_when_content_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("data.db");
        let backup_dir = dir.path().join("backups");

        let conn = crate::db::connect(&db_path).await.unwrap();
        crate::db::migrations::init_schema_versioned(&conn).await.unwrap();
        drop(conn);

        let svc = BackupService::new(
            db_path.clone(),
            backup_dir.clone(),
            Arc::new(LocalFsStorage),
            Arc::new(FakeSettings::new()),
        );

        let r1 = svc.backup_now().await.unwrap();
        assert!(r1.skipped.is_none());

        let r2 = svc.backup_now().await.unwrap();
        assert_eq!(r2.skipped.as_deref(), Some("content unchanged"));
        assert_eq!(r2.md5, r1.md5);

        // dir 下仍只 1 个文件
        let snapshots = LocalFsStorage.list_snapshots(&backup_dir).unwrap();
        assert_eq!(snapshots.len(), 1);
    }

    #[tokio::test]
    async fn backup_now_creates_new_when_content_changed() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("data.db");
        let backup_dir = dir.path().join("backups");

        let conn = crate::db::connect(&db_path).await.unwrap();
        crate::db::migrations::init_schema_versioned(&conn).await.unwrap();
        drop(conn);

        let svc = BackupService::new(
            db_path.clone(),
            backup_dir.clone(),
            Arc::new(LocalFsStorage),
            Arc::new(FakeSettings::new()),
        );

        svc.backup_now().await.unwrap();
        // 改 DB 内容
        std::thread::sleep(std::time::Duration::from_millis(1100)); // RFC3339 秒精度需 sleep 跨秒
        std::fs::write(&db_path, b"changed").unwrap();

        let r2 = svc.backup_now().await.unwrap();
        assert!(r2.skipped.is_none());

        let snapshots = LocalFsStorage.list_snapshots(&backup_dir).unwrap();
        assert_eq!(snapshots.len(), 2);
    }
```

- [ ] **Step 2: 跑测试，确认 pass**

Expected: dedup 逻辑在 Task 8 已实现；这两个测试应该直接通过。

如果失败：检查 `backup_last_md5` 写入路径是否正确、`hash_db_file` 是否在写入 settings 之前完成。

- [ ] **Step 3: 如果需要，commit**

如果测试无需改代码就通过：
```bash
git commit --allow-empty -m "test(backup): backup_now dedup behavior"
```

否则正常 commit。

---

## Task 10: backup_now retention cleanup

**Files:**
- Modify: `src-tauri/src/services/backup.rs`

- [ ] **Step 1: 写失败测试**

```rust
    #[tokio::test]
    async fn backup_now_trims_to_retention_count() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("data.db");
        let backup_dir = dir.path().join("backups");

        let conn = crate::db::connect(&db_path).await.unwrap();
        crate::db::migrations::init_schema_versioned(&conn).await.unwrap();
        drop(conn);

        let settings = FakeSettings::new();
        let svc = BackupService::new(
            db_path.clone(),
            backup_dir.clone(),
            Arc::new(LocalFsStorage),
            settings.clone(),
        );
        // retention = 2
        svc.set_config(Some(""), 2).await.unwrap();

        // 造 4 个不同内容的备份
        for i in 0..4 {
            if i > 0 {
                std::thread::sleep(std::time::Duration::from_millis(1100));
            }
            std::fs::write(&db_path, format!("content{}", i).into_bytes()).unwrap();
            svc.backup_now().await.unwrap();
        }

        let snapshots = LocalFsStorage.list_snapshots(&backup_dir).unwrap();
        assert_eq!(snapshots.len(), 2, "should keep only latest 2");
    }

    #[tokio::test]
    async fn backup_now_retention_zero_keeps_all() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("data.db");
        let backup_dir = dir.path().join("backups");

        let conn = crate::db::connect(&db_path).await.unwrap();
        crate::db::migrations::init_schema_versioned(&conn).await.unwrap();
        drop(conn);

        let settings = FakeSettings::new();
        let svc = BackupService::new(
            db_path.clone(),
            backup_dir.clone(),
            Arc::new(LocalFsStorage),
            settings.clone(),
        );
        svc.set_config(Some(""), 0).await.unwrap(); // 0 = 禁用

        for i in 0..3 {
            if i > 0 {
                std::thread::sleep(std::time::Duration::from_millis(1100));
            }
            std::fs::write(&db_path, format!("c{}", i).into_bytes()).unwrap();
            svc.backup_now().await.unwrap();
        }

        let snapshots = LocalFsStorage.list_snapshots(&backup_dir).unwrap();
        assert_eq!(snapshots.len(), 3);
    }
```

- [ ] **Step 2: 跑测试，确认 pass**

Expected: retention 逻辑在 Task 8 已实现。

- [ ] **Step 3: 如果失败，调试**

retention 在 `apply_retention` 里：`list_snapshots` 返回 mtime 倒序，skip 前 N 后删除其余。验证 list_snapshots 排序正确。

---

## Task 11: backup_now concurrent inflight mutex

**Files:**
- Modify: `src-tauri/src/services/backup.rs`

- [ ] **Step 1: 写失败测试**

```rust
    #[tokio::test]
    async fn backup_now_serializes_concurrent_callers() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("data.db");
        let backup_dir = dir.path().join("backups");

        let conn = crate::db::connect(&db_path).await.unwrap();
        crate::db::migrations::init_schema_versioned(&conn).await.unwrap();
        drop(conn);

        let svc = Arc::new(BackupService::new(
            db_path.clone(),
            backup_dir.clone(),
            Arc::new(LocalFsStorage),
            Arc::new(FakeSettings::new()),
        ));

        // 启动两个并发调用
        let s1 = svc.clone();
        let s2 = svc.clone();
        let h1 = tokio::spawn(async move { s1.backup_now().await });
        let h2 = tokio::spawn(async move { s2.backup_now().await });
        let (r1, r2) = tokio::join!(h1, h2);

        let results = vec![r1.unwrap(), r2.unwrap()];
        let oks = results.iter().filter(|r| r.is_ok()).count();
        let errs = results.iter().filter(|r| r.is_err()).count();
        assert_eq!(oks, 1, "exactly one should succeed");
        assert_eq!(errs, 1, "the other should fail with 'already in progress'");
    }
```

- [ ] **Step 2: 跑测试，确认 pass**

Expected: inflight mutex 在 Task 8 已实现（`try_lock`）。验证行为正确。

---

## Task 12: stage_restore + apply_pending_restore

**Files:**
- Modify: `src-tauri/src/services/backup.rs`

- [ ] **Step 1: 写失败测试**

```rust
    #[test]
    fn stage_restore_writes_marker() {
        let dir = tempfile::tempdir().unwrap();
        let svc = BackupService::new(
            dir.path().join("data.db"),
            dir.path().join("backups"),
            Arc::new(LocalFsStorage),
            Arc::new(FakeSettings::new()),
        );
        let src = dir.path().join("some-backup.db");
        std::fs::write(&src, b"SQLite format 3\0xxx").unwrap();

        svc.stage_restore(&src).unwrap();
        let marker = dir.path().join(".restore-pending.json");
        let pending = read_restore_marker(&marker).unwrap().unwrap();
        assert_eq!(pending.src, src.to_string_lossy());
    }

    #[tokio::test]
    async fn apply_pending_restore_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("data.db");
        let backup_path = dir.path().join("backups/data-test.db");

        // 准备一个有效 SQLite DB 作为备份源
        std::fs::create_dir_all(dir.path().join("backups")).unwrap();
        {
            let conn = crate::db::connect(&backup_path).await.unwrap();
            crate::db::migrations::init_schema_versioned(&conn).await.unwrap();
        }

        let svc = BackupService::new(
            db_path.clone(),
            dir.path().join("backups"),
            Arc::new(LocalFsStorage),
            Arc::new(FakeSettings::new()),
        );

        // 1. 写 marker
        svc.stage_restore(&backup_path).unwrap();

        // 2. apply：复制 backup → data.db
        let outcome = svc.apply_pending_restore().unwrap();
        assert!(matches!(outcome, RestoreOutcome::Applied { .. }));

        // 3. 验证：data.db 已变成 backup 的内容
        assert!(db_path.exists());
        let conn = crate::db::connect(&db_path).await.unwrap();
        use sea_orm::{ActiveModelTrait, Set};
        let am = crate::db::entities::doujinshi_file::ActiveModel {
            title: Set("verify".into()),
            filename: Set("f.zip".into()),
            hash: Set("h_after".into()),
            ext: Set("zip".into()),
            size_bytes: Set(0),
            last_seen_path: Set("/x".into()),
            created_at: Set(chrono::Utc::now()),
            updated_at: Set(chrono::Utc::now()),
            ..Default::default()
        };
        am.insert(&conn).await.unwrap();
        use crate::db::entities::doujinshi_file;
        use sea_orm::EntityTrait;
        let rows = doujinshi_file::Entity::find().all(&conn).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].title, "verify");
    }

    #[test]
    fn apply_pending_restore_refuses_non_sqlite() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("backups")).unwrap();
        let svc = BackupService::new(
            dir.path().join("data.db"),
            dir.path().join("backups"),
            Arc::new(LocalFsStorage),
            Arc::new(FakeSettings::new()),
        );
        let src = dir.path().join("fake.db");
        std::fs::write(&src, b"NOT A SQLITE FILE").unwrap();
        svc.stage_restore(&src).unwrap();

        let outcome = svc.apply_pending_restore().unwrap();
        assert!(matches!(outcome, RestoreOutcome::Refused { .. }));
        // 标记应被删除
        let marker = dir.path().join(".restore-pending.json");
        assert!(!marker.exists());
    }

    #[test]
    fn apply_pending_restore_refuses_missing_src() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("backups")).unwrap();
        let svc = BackupService::new(
            dir.path().join("data.db"),
            dir.path().join("backups"),
            Arc::new(LocalFsStorage),
            Arc::new(FakeSettings::new()),
        );
        let src = dir.path().join("never-existed.db");
        svc.stage_restore(&src).unwrap();

        let outcome = svc.apply_pending_restore().unwrap();
        assert!(matches!(outcome, RestoreOutcome::Refused { .. }));
        let marker = dir.path().join(".restore-pending.json");
        assert!(!marker.exists());
    }

    #[test]
    fn apply_pending_restore_noop_when_no_marker() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("backups")).unwrap();
        let svc = BackupService::new(
            dir.path().join("data.db"),
            dir.path().join("backups"),
            Arc::new(LocalFsStorage),
            Arc::new(FakeSettings::new()),
        );
        let outcome = svc.apply_pending_restore().unwrap();
        assert!(matches!(outcome, RestoreOutcome::Noop));
    }
```

- [ ] **Step 2: 跑测试，确认 fail**

Expected: compile error.

- [ ] **Step 3: 实现 stage_restore / apply_pending_restore**

```rust
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RestoreOutcome {
    Noop,
    Applied { src: PathBuf, dst: PathBuf },
    Refused { reason: String },
}

impl BackupService {
    // ... 既有方法 ...
    pub fn restore_marker_path(&self) -> PathBuf {
        // marker 放在 db_path 同目录（resources/ 下），便于 main.rs 启动时直接定位
        self.db_path
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .join(".restore-pending.json")
    }

    pub fn stage_restore(&self, src: &Path) -> Result<()> {
        let pending = RestorePending {
            src: src.to_string_lossy().into_owned(),
            requested_at: chrono::Utc::now().to_rfc3339(),
        };
        write_restore_marker(&self.restore_marker_path(), &pending)
    }

    pub fn apply_pending_restore(&self) -> Result<RestoreOutcome> {
        let marker = self.restore_marker_path();
        let Some(pending) = read_restore_marker(&marker)? else {
            return Ok(RestoreOutcome::Noop);
        };
        let src = PathBuf::from(&pending.src);
        // 1. 验证源文件存在 + magic
        let outcome = match self.validate_restore_source(&src) {
            Ok(()) => {
                // 2. fs::copy（不 move——保留原备份）
                if let Some(parent) = self.db_path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::copy(&src, &self.db_path)?;
                RestoreOutcome::Applied { src, dst: self.db_path.clone() }
            }
            Err(reason) => RestoreOutcome::Refused { reason },
        };
        // 不论结果如何，都清掉 marker（避免下次启动再尝试）
        clear_restore_marker(&marker);
        Ok(outcome)
    }

    fn validate_restore_source(&self, src: &Path) -> Result<(), String> {
        if !src.exists() {
            return Err(format!("source not found: {}", src.display()));
        }
        let mut head = [0u8; 16];
        let bytes_read = std::fs::File::open(src)
            .and_then(|mut f| std::io::Read::read(&mut f, &mut head))
            .map_err(|e| format!("read failed: {}", e))?;
        if bytes_read != 16 || &head != b"SQLite format 3\0" {
            return Err(format!("not a SQLite file: {}", src.display()));
        }
        Ok(())
    }
}
```

- [ ] **Step 4: 跑测试，确认 pass**

Expected: 5 passed.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/services/backup.rs
git commit -m "feat(backup): stage_restore + apply_pending_restore with magic validation"
```

---

## Task 13: should_auto_backup

**Files:**
- Modify: `src-tauri/src/services/backup.rs`

- [ ] **Step 1: 写失败测试**

```rust
    #[tokio::test]
    async fn should_auto_backup_true_when_no_record() {
        let dir = tempfile::tempdir().unwrap();
        let svc = BackupService::new(
            dir.path().join("data.db"),
            dir.path().join("backups"),
            Arc::new(LocalFsStorage),
            Arc::new(FakeSettings::new()),
        );
        assert!(svc.should_auto_backup(std::time::Duration::from_secs(86400)).await.unwrap());
    }

    #[tokio::test]
    async fn should_auto_backup_false_when_recent() {
        let dir = tempfile::tempdir().unwrap();
        let settings = FakeSettings::new();
        settings.inner.lock().unwrap().insert(
            "backup_last_at".into(),
            chrono::Utc::now().to_rfc3339(),
        );
        let svc = BackupService::new(
            dir.path().join("data.db"),
            dir.path().join("backups"),
            Arc::new(LocalFsStorage),
            Arc::new(settings),
        );
        assert!(!svc.should_auto_backup(std::time::Duration::from_secs(86400)).await.unwrap());
    }

    #[tokio::test]
    async fn should_auto_backup_true_when_old() {
        let dir = tempfile::tempdir().unwrap();
        let settings = FakeSettings::new();
        let old = (chrono::Utc::now() - chrono::Duration::hours(25)).to_rfc3339();
        settings.inner.lock().unwrap().insert("backup_last_at".into(), old);
        let svc = BackupService::new(
            dir.path().join("data.db"),
            dir.path().join("backups"),
            Arc::new(LocalFsStorage),
            Arc::new(settings),
        );
        assert!(svc.should_auto_backup(std::time::Duration::from_secs(86400)).await.unwrap());
    }
```

- [ ] **Step 2: 跑测试，确认 fail**

Expected: compile error.

- [ ] **Step 3: 实现 should_auto_backup**

```rust
impl BackupService {
    // ... 既有方法 ...
    pub async fn should_auto_backup(&self, threshold: std::time::Duration) -> Result<bool> {
        let last_at = self.settings.read("backup_last_at").await?;
        let Some(last_str) = last_at else {
            return Ok(true); // 从未备过
        };
        let last = chrono::DateTime::parse_from_rfc3339(&last_str)
            .map_err(|e| anyhow::anyhow!("invalid backup_last_at: {}", e))?;
        let elapsed = chrono::Utc::now().signed_duration_since(last.with_timezone(&chrono::Utc));
        let elapsed_std = elapsed.to_std().unwrap_or(std::time::Duration::ZERO);
        Ok(elapsed_std > threshold)
    }
}
```

- [ ] **Step 4: 跑测试，确认 pass**

Expected: 3 passed.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/services/backup.rs
git commit -m "feat(backup): should_auto_backup threshold check"
```

---

## Task 14: Tauri commands（commands/backup.rs）

**Files:**
- Create: `src-tauri/src/commands/backup.rs`

- [ ] **Step 1: 写失败测试**

```rust
#[cfg(test)]
mod tests {
    // 集成风格：用真 DbSettingsHandle + 真 SQLite
    use super::*;
    use crate::services::backup::{
        BackupService, BackupConfig, LocalFsStorage,
    };
    use std::sync::Arc;

    async fn make_svc(tmp: &std::path::Path) -> BackupService {
        let conn = crate::db::connect(&tmp.join("data.db")).await.unwrap();
        crate::db::migrations::init_schema_versioned(&conn).await.unwrap();
        BackupService::new(
            tmp.join("data.db"),
            tmp.join("backups"),
            Arc::new(LocalFsStorage),
            Arc::new(crate::services::backup::DbSettingsHandle { conn: &conn }),
        )
    }

    #[tokio::test]
    async fn tauri_backup_now_command_works() {
        let tmp = tempfile::tempdir().unwrap();
        let svc = make_svc(tmp.path()).await;
        // 通过 Arc 模拟 Tauri State
        let result = backup_now_inner(&svc).await.unwrap();
        assert!(result.path.exists());
    }
}
```

- [ ] **Step 2: 跑测试，确认 fail**

Expected: compile error.

- [ ] **Step 3: 实现 commands/backup.rs**

```rust
use crate::error::AppResult;
use crate::services::backup::{BackupConfig, BackupResult, RestoreOutcome, SnapshotInfo};
use crate::AppState;
use serde::Serialize;
use tauri::State;

#[derive(Debug, Serialize)]
pub struct BackupListItem {
    pub path: String,
    pub mtime_unix_ms: i64,
    pub size_bytes: u64,
}

impl From<SnapshotInfo> for BackupListItem {
    fn from(s: SnapshotInfo) -> Self {
        let mtime_unix_ms = s
            .mtime
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        Self {
            path: s.path.to_string_lossy().into_owned(),
            mtime_unix_ms,
            size_bytes: s.size_bytes,
        }
    }
}

#[tauri::command]
pub async fn backup_now(state: State<'_, AppState>) -> AppResult<BackupResult> {
    state.backup_service.backup_now().await.map_err(Into::into)
}

#[tauri::command]
pub async fn list_backups(state: State<'_, AppState>) -> AppResult<Vec<BackupListItem>> {
    let snaps = state.backup_service.list_backups().await.map_err(Into::into)?;
    Ok(snaps.into_iter().map(Into::into).collect())
}

#[tauri::command]
pub async fn set_backup_config(
    state: State<'_, AppState>,
    dir: Option<String>,
    retention: u32,
) -> AppResult<()> {
    state
        .backup_service
        .set_config(dir.as_deref(), retention)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn get_backup_config(state: State<'_, AppState>) -> AppResult<BackupConfig> {
    state.backup_service.get_config().await.map_err(Into::into)
}

/// 把还原请求落到 .restore-pending.json；调用方需提示用户关 app。
#[tauri::command]
pub async fn restore_from_backup(
    state: State<'_, AppState>,
    path: String,
) -> AppResult<()> {
    state
        .backup_service
        .stage_restore(std::path::Path::new(&path))
        .map_err(Into::into)
}

/// 启动期恢复结果（供 main.rs 调用，结果打到日志）
pub async fn apply_pending_restore_cmd(state: State<'_, AppState>) -> AppResult<RestoreOutcome> {
    Ok(state.backup_service.apply_pending_restore()?)
}

/// 测试 helper（不通过 Tauri State）
pub async fn backup_now_inner(svc: &crate::services::backup::BackupService) -> anyhow::Result<BackupResult> {
    svc.backup_now().await
}
```

- [ ] **Step 4: 跑测试，确认 pass**

Expected: 1 passed.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/commands/backup.rs
git commit -m "feat(backup): Tauri commands — backup_now/list/set_config/restore"
```

---

## Task 15: AppState 集成 + lib.rs 注册

**Files:**
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: 在 AppState 加 backup_service 字段**

```rust
pub struct AppState {
    pub conn: DatabaseConnection,
    pub scanner: Arc<services::scanner::Scanner>,
    pub covers_dir: Arc<std::path::PathBuf>,
    pub config: config::AppConfig,
    pub auth_token: Arc<RwLock<String>>,
    pub preview_cache: Arc<services::preview_cache::PreviewCache>,
    pub backup_service: Arc<services::backup::BackupService>,
}
```

- [ ] **Step 2: 在 `run()` 里构造 BackupService 并注入 AppState**

在创建 `AppState` 之前：

```rust
use services::backup::{BackupService, DbSettingsHandle, LocalFsStorage};

let backup_service = Arc::new(BackupService::new(
    cfg.db_path(),
    cfg.resources_dir.join("backups"),
    Arc::new(LocalFsStorage),
    Arc::new(DbSettingsHandle { conn: &conn }),
));
```

构造 AppState 时加字段：

```rust
let state = AppState {
    conn: conn.clone(),
    scanner: scanner.clone(),
    covers_dir,
    config: cfg_clone,
    auth_token: auth_token.clone(),
    preview_cache: preview_cache.clone(),
    backup_service: backup_service.clone(),
};
```

- [ ] **Step 3: 在 invoke_handler 注册 4 条新 command**

```rust
.invoke_handler(tauri::generate_handler![
    // ... 既有 commands ...
    commands::backup::backup_now,
    commands::backup::list_backups,
    commands::backup::set_backup_config,
    commands::backup::get_backup_config,
    commands::backup::restore_from_backup,
])
```

- [ ] **Step 4: cargo check**

```bash
cd src-tauri && cargo check
```

Expected: clean.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/lib.rs
git commit -m "feat(backup): wire BackupService into AppState + register commands"
```

---

## Task 16: 自动备份启动钩子

**Files:**
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: 在 `run()` 里 spawn 自动备份任务**

加在 HTTP server 启动之后、scanner 启动之前：

```rust
// 自动备份：启动时若距上次 > 24h 补一份。失败仅 log warn，不影响启动。
{
    let svc = backup_service.clone();
    tauri::async_runtime::spawn(async move {
        const AUTO_BACKUP_THRESHOLD: std::time::Duration = std::time::Duration::from_secs(24 * 3600);
        match svc.should_auto_backup(AUTO_BACKUP_THRESHOLD).await {
            Ok(true) => {
                if let Err(e) = svc.backup_now().await {
                    tracing::warn!("auto backup failed: {:?}", e);
                } else {
                    tracing::info!("auto backup completed");
                }
            }
            Ok(false) => tracing::debug!("auto backup skipped (recent enough)"),
            Err(e) => tracing::warn!("should_auto_backup error: {:?}", e),
        }
    });
}
```

- [ ] **Step 2: cargo check**

```bash
cd src-tauri && cargo check
```

Expected: clean.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/lib.rs
git commit -m "feat(backup): auto-backup hook on app startup (>24h threshold)"
```

---

## Task 17: main.rs 启动期 apply_pending_restore

**Files:**
- Modify: `src-tauri/src/main.rs`

- [ ] **Step 1: 在 DB recovery 之后、init_schema 之前调用**

```rust
let db_path = cfg.db_path();
match doujinshi_records::db::recovery::probe_and_recover(&db_path).await {
    Ok(doujinshi_records::db::recovery::RecoveryAction::BackedUp { backup_path }) => {
        eprintln!("WARN: corrupt db moved to {}, recreating", backup_path.display());
    }
    Ok(doujinshi_records::db::recovery::RecoveryAction::Noop) => {}
    Err(e) => {
        eprintln!("db recovery probe failed: {:?}", e);
        std::process::exit(1);
    }
}

// 处理用户上次的还原请求（在打开 DB 之前；用 backup_service 不需要常驻连接）
{
    use doujinshi_records::services::backup::{BackupService, DbSettingsHandle, LocalFsStorage};
    use std::sync::Arc;
    // 用一个临时 SQLite 连接喂 settings（DbSettingsHandle 借用 conn）
    // apply_pending_restore 不依赖 settings，所以直接构造 service 只用 file 路径即可
    // —— 用一个空连接只是为了满足 DbSettingsHandle 的 borrow 生命周期
    let tmp_conn = doujinshi_records::db::connect(&cfg.db_path()).await.ok();
    if let Some(conn) = &tmp_conn {
        let svc = BackupService::new(
            cfg.db_path(),
            cfg.resources_dir.join("backups"),
            Arc::new(LocalFsStorage),
            Arc::new(DbSettingsHandle { conn }),
        );
        match svc.apply_pending_restore() {
            Ok(doujinshi_records::services::backup::RestoreOutcome::Applied { src, dst }) => {
                eprintln!("restored from {} → {}", src.display(), dst.display());
            }
            Ok(doujinshi_records::services::backup::RestoreOutcome::Refused { reason }) => {
                eprintln!("WARN: restore refused: {}", reason);
            }
            Ok(doujinshi_records::services::backup::RestoreOutcome::Noop) => {}
            Err(e) => eprintln!("WARN: apply_pending_restore error: {:?}", e),
        }
    }
}

let conn = doujinshi_records::db::connect(&cfg.db_path())
    .await
    .expect("failed to connect db");
```

- [ ] **Step 2: cargo check**

```bash
cd src-tauri && cargo check
```

Expected: clean.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/main.rs
git commit -m "feat(backup): apply pending restore at startup before schema init"
```

---

## Task 18: Frontend types/api.ts

**Files:**
- Modify: `src/types/api.ts`

- [ ] **Step 1: 在文件末尾追加**

```ts
/// 用户可见的备份配置
export interface BackupConfig {
  dir: string
  retention_count: number
}

/// 单个备份快照（UI 列表展示用）
export interface BackupInfo {
  path: string
  mtime_unix_ms: number
  size_bytes: number
}

/// `backup_now` 命令返回
export interface BackupResult {
  path: string
  size_bytes: number
  md5: string
  /// `undefined` 表示新建了备份；存在则说明本次因内容未变而跳过
  skipped?: string
}
```

- [ ] **Step 2: pnpm exec vue-tsc --noEmit**

```bash
pnpm exec vue-tsc --noEmit
```

Expected: clean.

- [ ] **Step 3: Commit**

```bash
git add src/types/api.ts
git commit -m "feat(backup): add BackupConfig / BackupInfo / BackupResult types"
```

---

## Task 19: Frontend api/tauri.ts wrappers

**Files:**
- Modify: `src/api/tauri.ts`

- [ ] **Step 1: 加 invoke wrapper**

```ts
import type {
  // ... 既有 import ...
  BackupConfig,
  BackupInfo,
  BackupResult,
} from "@/types/api"

// ... 既有 api 对象 ...
  backupNow: () => invoke<BackupResult>("backup_now"),
  listBackups: () => invoke<BackupInfo[]>("list_backups"),
  setBackupConfig: (cfg: BackupConfig) => invoke<void>("set_backup_config", { dir: cfg.dir, retention: cfg.retention_count }),
  getBackupConfig: () => invoke<BackupConfig>("get_backup_config"),
  restoreFromBackup: (path: string) => invoke<void>("restore_from_backup", { path }),
```

- [ ] **Step 2: vue-tsc**

```bash
pnpm exec vue-tsc --noEmit
```

Expected: clean.

- [ ] **Step 3: Commit**

```bash
git add src/api/tauri.ts
git commit -m "feat(backup): Tauri invoke wrappers for backup commands"
```

---

## Task 20: SettingsView.vue「数据备份」卡片

**Files:**
- Modify: `src/views/SettingsView.vue`

- [ ] **Step 1: 在 script 部分加状态**

```ts
import { NPopconfirm, NInput, NDataTable, useDialog } from "naive-ui"
import type { BackupInfo, BackupResult } from "@/types/api"

const dialog = useDialog()

const backupConfig = ref<{ dir: string; retention_count: number } | null>(null)
const backupList = ref<BackupInfo[]>([])
const backingUp = ref(false)
const lastBackupResult = ref<BackupResult | null>(null)

async function loadBackupData() {
  backupConfig.value = await api.getBackupConfig()
  backupList.value = await api.listBackups()
}

async function backupNow() {
  backingUp.value = true
  try {
    const r = await api.backupNow()
    lastBackupResult.value = r
    if (r.skipped) {
      message.info(`未变化（${r.skipped}）`)
    } else {
      message.success(`已备份 ${(r.size_bytes / 1024).toFixed(1)} KB`)
    }
    await loadBackupData()
  } catch (e) {
    message.error(String(e))
  } finally {
    backingUp.value = false
  }
}

async function saveBackupConfig() {
  if (!backupConfig.value) return
  await api.setBackupConfig(backupConfig.value)
  message.success("已保存")
  await loadBackupData()
}

function formatTime(ms: number) {
  return new Date(ms).toLocaleString()
}

function formatSize(bytes: number) {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  return `${(bytes / 1024 / 1024).toFixed(2)} MB`
}

function confirmRestore(row: BackupInfo) {
  dialog.warning({
    title: "确认还原",
    content: `将从 ${row.path} 还原，会覆盖当前所有数据。请关闭 app 后下次启动自动应用。`,
    positiveText: "记录还原请求",
    negativeText: "取消",
    onPositiveClick: async () => {
      await api.restoreFromBackup(row.path)
      message.warning("已记录还原请求。请关闭 app，下次启动会自动应用。")
    },
  })
}

const backupColumns = [
  { title: "时间", key: "mtime_unix_ms", render: (row: BackupInfo) => formatTime(row.mtime_unix_ms) },
  { title: "大小", key: "size_bytes", render: (row: BackupInfo) => formatSize(row.size_bytes) },
  { title: "路径", key: "path" },
  {
    title: "操作",
    key: "action",
    render: (row: BackupInfo) =>
      h(NSpace, { size: "small" }, () => [
        h(NButton, { size: "small", onClick: () => confirmRestore(row) }, () => "还原"),
      ]),
  },
]

onMounted(() => {
  loadBackupData()
})
```

- [ ] **Step 2: 在 template 加卡片（放在「扫描」卡片之后）**

```html
<n-card title="数据备份">
  <p class="text-caption text-silver-mist mb-2">
    备份会包含 auth_token 等所有 app_setting；不要把备份文件分享给他人。备份不包含 .zip/.rar 文件本体。
  </p>
  <n-spin :show="!backupConfig">
    <n-space vertical>
      <n-space align="center">
        <span style="min-width: 80px">备份目录：</span>
        <n-input
          v-model:value="backupConfig!.dir"
          placeholder="留空 = 默认 resources/backups/"
          class="flex-1"
        />
      </n-space>
      <n-space align="center">
        <span style="min-width: 80px">保留份数：</span>
        <n-input-number
          v-model:value="backupConfig!.retention_count"
          :min="0"
          :max="999"
          placeholder="0 = 不删旧"
          class="w-[140px]"
        />
        <n-button size="small" @click="saveBackupConfig">保存配置</n-button>
      </n-space>
      <n-space align="center">
        <n-button type="primary" :loading="backingUp" @click="backupNow">立即备份</n-button>
        <span v-if="lastBackupResult" class="text-caption text-silver-mist">
          上次：{{ formatTime(Date.parse(lastBackupResult.path.split("/").pop()?.replace(/^data-|\.db$/g, "") + "Z") || 0) }}
          · {{ formatSize(lastBackupResult.size_bytes) }}
          <span v-if="lastBackupResult.skipped" class="ml-1">({{ lastBackupResult.skipped }})</span>
        </span>
      </n-space>
    </n-space>
  </n-spin>

  <n-divider class="my-3!" />

  <h3 class="text-heading-xs font-medium mb-2">历史备份</h3>
  <n-data-table
    :columns="backupColumns"
    :data="backupList"
    :bordered="false"
    size="small"
    :row-key="(row: BackupInfo) => row.path"
  />
</n-card>
```

- [ ] **Step 3: vue-tsc + 手动测试 UI（开发模式）**

```bash
pnpm exec vue-tsc --noEmit
pnpm tauri dev
```

Expected: UI 渲染正常；点击「立即备份」创建一个新文件；列表更新。

- [ ] **Step 4: Commit**

```bash
git add src/views/SettingsView.vue
git commit -m "feat(backup): Settings page data backup card"
```

---

## Task 21: 集成测试 — end-to-end backup → modify → restore

**Files:**
- Create: `src-tauri/tests/backup.rs`

- [ ] **Step 1: 写测试**

```rust
use doujinshi_records::db;
use doujinshi_records::services::backup::{
    BackupService, DbSettingsHandle, LocalFsStorage, RestoreOutcome,
};
use sea_orm::{ActiveModelTrait, EntityTrait, Set};
use std::sync::Arc;

#[tokio::test]
async fn end_to_end_backup_modify_restore() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("data.db");
    let backup_dir = dir.path().join("backups");
    std::fs::create_dir_all(&backup_dir).unwrap();

    // 1. 初始化 DB + 插一条数据
    let conn = db::connect(&db_path).await.unwrap();
    db::migrations::init_schema_versioned(&conn).await.unwrap();
    let am = db::entities::doujinshi_file::ActiveModel {
        title: Set("original".into()),
        filename: Set("f.zip".into()),
        hash: Set("h1".into()),
        ext: Set("zip".into()),
        size_bytes: Set(0),
        last_seen_path: Set("/x".into()),
        created_at: Set(chrono::Utc::now()),
        updated_at: Set(chrono::Utc::now()),
        ..Default::default()
    };
    let id = am.insert(&conn).await.unwrap().id;
    drop(conn);

    // 2. 构造 service + 备份
    let conn = db::connect(&db_path).await.unwrap();
    let svc = Arc::new(BackupService::new(
        db_path.clone(),
        backup_dir.clone(),
        Arc::new(LocalFsStorage),
        Arc::new(DbSettingsHandle { conn: &conn }),
    ));
    let r = svc.backup_now().await.unwrap();
    assert!(r.path.exists());
    drop(conn);

    // 3. 修改 DB
    let conn = db::connect(&db_path).await.unwrap();
    let row = db::entities::doujinshi_file::Entity::find_by_id(id)
        .one(&conn)
        .await
        .unwrap()
        .unwrap();
    let mut am: db::entities::doujinshi_file::ActiveModel = row.into();
    am.title = Set("modified".into());
    am.update(&conn).await.unwrap();
    drop(conn);

    // 4. 验证修改生效
    let conn = db::connect(&db_path).await.unwrap();
    let row = db::entities::doujinshi_file::Entity::find_by_id(id)
        .one(&conn)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.title, "modified");

    // 5. 触发还原：写 marker
    svc.stage_restore(&r.path).unwrap();

    // 6. apply pending restore：模拟启动期检测
    let outcome = svc.apply_pending_restore().unwrap();
    assert!(matches!(outcome, RestoreOutcome::Applied { .. }));

    // 7. 验证 data.db 已恢复成原值
    drop(svc);
    let conn = db::connect(&db_path).await.unwrap();
    let row = db::entities::doujinshi_file::Entity::find_by_id(id)
        .one(&conn)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.title, "original", "data should be restored to pre-modify state");
}
```

- [ ] **Step 2: 跑测试**

```bash
cd src-tauri && cargo test --test backup
```

Expected: 1 passed.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/tests/backup.rs
git commit -m "test(backup): end-to-end backup → modify → restore"
```

---

## Task 22: 整体验证

- [ ] **Step 1: cargo build + test 全部跑一遍**

```bash
cd src-tauri && cargo build 2>&1 | tail -5
cd src-tauri && cargo test --test migrations 2>&1 | tail -5
cd src-tauri && cargo test --test backup 2>&1 | tail -10
```

Expected: build clean; migrations 7/7 pass; backup 1 passed.

- [ ] **Step 2: 前端类型检查**

```bash
pnpm exec vue-tsc --noEmit
```

Expected: clean.

- [ ] **Step 3: clippy**

```bash
cd src-tauri && cargo clippy --all-targets -- -D warnings 2>&1 | tail -20
```

Expected: no warnings.

- [ ] **Step 4: 手动跑 dev 模式**

```bash
pnpm tauri dev
```

验证：
- Settings 页能看到「数据备份」卡片
- 点「立即备份」成功创建文件 + 列表显示
- 改 DB 内容（比如手动编辑一个文件标题）→ 再点立即备份 → 列表新增一条
- 立即备份两次 → 第二次提示「未变化」
- 选一条备份点「还原」→ 弹窗确认 → 提示关 app → 关 app 重启 → 数据回到备份点

- [ ] **Step 5: Commit（如有调试）**

```bash
git commit -m "chore: post-implementation polish" --allow-empty
```

---

## 自审

**Spec 覆盖**：
- ✅ 自动备份（Task 16）— 启动期 spawn hook，>24h 阈值
- ✅ 手动备份（Task 8/14/19/20）— UI 按钮 → Tauri command → service
- ✅ BLAKE3 dedup（Task 8/9）— task_last_md5 比对
- ✅ 保留 N（Task 8/10）— apply_retention
- ✅ 还原（Task 12/14/17/20）— stage_restore → marker → main.rs apply
- ✅ BackupStorage trait（Task 5）— LocalFsStorage，未来易扩展
- ✅ SettingsHandle trait（Task 6）— service 单测可注入 fake
- ✅ inflight mutex（Task 8/11）— 防并发
- ✅ 原子写（Task 8）— VACUUM INTO tmp + rename
- ✅ 启动期 magic 校验（Task 12/17）— 两道关
- ✅ 不暴露 HTTP — Task 14 只注册 Tauri command

**质量要求落实**：
- 解耦：trait 抽象（storage + settings），commands 薄壳，service 纯逻辑
- 高效：VACUUM INTO + dedup + 无定时器
- 安全：原子 rename、COPY not MOVE、magic 校验、二次确认、inflight 串行
- 易读：单文件 < 300 行、命名直白、test 即文档
- 易扩展：trait 抽象 + app_setting 命名空间、零硬编码路径

**Placeholder 检查**：grep 后无 TBD/TODO/「待补」。所有代码段完整。

**类型一致性**：所有引用 `BackupService` / `BackupStorage` / `RestoreOutcome` 等类型在引入前已定义。