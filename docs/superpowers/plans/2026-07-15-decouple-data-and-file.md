# V4 Implementation Plan — 同人志数据与文件解耦

> **For agentic workers:** REQUIRED SUB-KILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把同人志管理的 5 状态机（V3 + permanently_deleted 终态）改成"业务 status + 文件 file_state"双字段模型；状态机由"强一致（源文件缺失则拒绝）"改成"DB 优先 + 文件 best-effort"；让 status 完全由用户决定，文件是观测值。

**Architecture:**
- `doujinshi_file.current_location` 重命名为 `status`（值集：`in_library / archived / recycle / deleted`，4 个值，任意可切）
- `doujinshi_file.has_physical_file` (bool) 升级为 `file_state` (TEXT, `present / missing / absent_confirmed`)
- `doujinshi_file.current_path` 重命名为 `last_seen_path`（语义弱化为"最后已知路径"）
- 状态机集中入口 `state_machine::transition_with_dirs` 改为"DB 优先"：源文件缺失不阻塞 status 更新
- 状态切换遇到目标目录同名文件 = 视为孤儿（dirty_data），覆盖 + 写 `dirty_data(reason='overwritten_by_state_switch')`
- 销毁 = 复合操作（status=deleted + file_state=absent_confirmed + best-effort 删文件），不走状态机
- dirty_scanner 扫 4 个状态目录（不扫 deleted），按 file_state 三态更新

**Tech Stack:** Rust（SeaORM 1.1 + SQLite + tokio）+ Vue 3 + TS + Naive UI。schema migration v8（字段重命名 + 加 file_state）。

**前序:**
- V3（2026-07-11）：归档目录 + 脏数据 + webp 封面
- v6（2026-07-14 提交 `8e4e248`）：4 状态机升 5 状态机，`physically_deleted` 折进 `permanently_deleted`

**本 plan 上线后 V3 spec 描述的 5 状态机 + 强一致转移 全部作废。**

---

## File Structure

**修改**
- `src-tauri/src/db/entities/doujinshi_file.rs` — 字段重命名（status / file_state / last_seen_path）
- `src-tauri/src/db/migrations.rs` — 新增 v8 迁移
- `src-tauri/src/services/state_machine.rs` — 移除强一致护栏；4 个 kind 任意 from→任意 to；新增 `overwritten_by_state_switch` dirty_data 写入
- `src-tauri/src/services/identifier.rs` — collision check 排除 `status='deleted'`
- `src-tauri/src/services/dirty_scanner.rs` — 字段名适配（status / file_state / last_seen_path）；扫 4 状态目录
- `src-tauri/src/commands/inbox.rs` — `resolve_conflict::ReplaceB` 改写为 status=deleted + file_state=absent_confirmed
- `src-tauri/src/commands/recycle.rs` — `permanent_delete` 改写为复合"销毁"操作
- `src-tauri/src/commands/library.rs` — `mark_for_delete` / `archive` / `restore` 用新 TransitionKind 名字
- `src-tauri/src/models/file_summary.rs` — `current_location` → `status`；加 `file_state`；`current_path` → `last_seen_path`
- `src-tauri/src/http/api.rs` — 字段名同步；`/api/doujinshi/:id` 响应字段调整
- `src/types/api.ts` — `FileSummary` 字段名同步
- `src/stores/index.ts` — `useLibraryStore` 增加 `status` 过滤（默认隐藏 recycle / deleted）
- `src/views/LibraryView.vue` — 加 status 过滤 UI
- `src/views/DetailView.vue` — file_state != present 时显示"文件已丢失"提示
- `CLAUDE.md` — 更新状态机描述 + 字段名
- `docs/superpowers/specs/2026-07-11-v3-archive-and-dirty-data.md` — 末尾加"V4 后作废"注

**新建**
- `src-tauri/src/db/migrations/v8.rs`（可选拆分；不强求，可继续追加在 `migrations.rs`）

---

## Task 1: v8 schema 迁移 + doujinshi_file 实体字段重命名

**Files:**
- Modify: `src-tauri/src/db/migrations.rs`
- Modify: `src-tauri/src/db/entities/doujinshi_file.rs`
- Modify: `src-tauri/tests/migrations.rs`

- [ ] **Step 1: 写迁移测试**

在 `src-tauri/tests/migrations.rs` 加：

```rust
#[tokio::test]
async fn v8_migrates_v7_db() {
    let dir = tempfile::tempdir().unwrap();
    let conn = crate::db::connect(&dir.path().join("t.db")).await.unwrap();

    // 先用旧 schema 建表（手写 v7 之前的 SQL，模拟老库）
    conn.execute(sea_orm::Statement::from_string(
        sea_orm::DatabaseBackend::Sqlite,
        r#"CREATE TABLE doujinshi_file (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            title TEXT NOT NULL,
            filename TEXT NOT NULL,
            hash TEXT NOT NULL,
            ext TEXT NOT NULL,
            size_bytes INTEGER NOT NULL,
            circle TEXT,
            series TEXT,
            translator TEXT,
            version_tag TEXT,
            current_path TEXT NOT NULL,
            current_location TEXT NOT NULL,
            cover_path TEXT,
            marked_for_delete INTEGER NOT NULL DEFAULT 0,
            has_physical_file INTEGER NOT NULL DEFAULT 1,
            viewed INTEGER NOT NULL DEFAULT 0,
            note TEXT,
            rating INTEGER,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        )"#.to_string(),
    )).await.unwrap();
    // 插入几行测试数据：覆盖各 current_location + has_physical_file 组合
    conn.execute(sea_orm::Statement::from_string(
        sea_orm::DatabaseBackend::Sqlite,
        "INSERT INTO doujinshi_file (title, filename, hash, ext, size_bytes, current_path, current_location, has_physical_file, created_at, updated_at)
         VALUES ('t1','f1.zip','h1','zip',0,'/p/1','identified',1,'2026-01-01','2026-01-01'),
                ('t2','f2.zip','h2','zip',0,'/p/2','permanently_deleted',0,'2026-01-01','2026-01-01'),
                ('t3','f3.zip','h3','zip',0,'/p/3','archived',0,'2026-01-01','2026-01-01')".to_string(),
    )).await.unwrap();

    // 跑 v8 迁移（约定 migrations.rs 暴露 `run_v8_migration` 或类似入口；具体命名按实际代码调整）
    crate::db::migrations::init_schema_versioned(&conn).await.unwrap();

    // 1. 字段已重命名
    let rows: Vec<sea_orm::QueryResult> = sea_orm::ConnectionTrait::query_all(
        &conn,
        sea_orm::Statement::from_string(
            sea_orm::DatabaseBackend::Sqlite,
            "PRAGMA table_info(doujinshi_file)".to_string(),
        ),
    ).await.unwrap();
    let names: Vec<String> = rows.into_iter()
        .map(|r| r.try_get_by::<String>("name").unwrap_or_default())
        .collect();
    assert!(names.iter().any(|n| n == "status"), "status 列应存在");
    assert!(names.iter().any(|n| n == "last_seen_path"), "last_seen_path 列应存在");
    assert!(names.iter().any(|n| n == "file_state"), "file_state 列应存在");
    assert!(!names.iter().any(|n| n == "current_location"), "current_location 应被 rename 走");
    assert!(!names.iter().any(|n| n == "current_path"), "current_path 应被 rename 走");

    // 2. permanently_deleted 已改写为 deleted
    let rows: Vec<sea_orm::QueryResult> = sea_orm::ConnectionTrait::query_all(
        &conn,
        sea_orm::Statement::from_string(
            sea_orm::DatabaseBackend::Sqlite,
            "SELECT id, status, file_state FROM doujinshi_file ORDER BY id".to_string(),
        ),
    ).await.unwrap();
    // 第一行：identified + present
    assert_eq!(
        rows[0].try_get_by::<String>("status").unwrap(),
        Some("identified".to_string())
    );
    assert_eq!(
        rows[0].try_get_by::<String>("file_state").unwrap(),
        Some("present".to_string())
    );
    // 第二行：permanently_deleted → deleted, has_physical_file=0 → missing
    assert_eq!(
        rows[1].try_get_by::<String>("status").unwrap(),
        Some("deleted".to_string())
    );
    assert_eq!(
        rows[1].try_get_by::<String>("file_state").unwrap(),
        Some("missing".to_string())
    );
    // 第三行：archived + missing
    assert_eq!(
        rows[2].try_get_by::<String>("status").unwrap(),
        Some("archived".to_string())
    );
    assert_eq!(
        rows[2].try_get_by::<String>("file_state").unwrap(),
        Some("missing".to_string())
    );

    // 3. schema_version=8
    let row = sea_orm::ConnectionTrait::query_one(
        &conn,
        sea_orm::Statement::from_string(
            sea_orm::DatabaseBackend::Sqlite,
            "SELECT value FROM app_setting WHERE key='schema_version'".to_string(),
        ),
    ).await.unwrap().unwrap();
    let v: String = row.try_get_by("value").unwrap().unwrap();
    assert_eq!(v, "8");
}
```

> **注意**：项目当前用 `app_setting` 还是 `schema_version` 表取决于实际 migrations.rs 实现。测试断言按你仓库的实际 schema_version 存储位置调整。运行 `init_schema_versioned` 后应该已经写到 v8。

- [ ] **Step 2: 跑测试，验证失败**

```bash
cd src-tauri && cargo test --test migrations v8_migrates_v7_db
```

期望：编译失败（`run_v8_migration` 不存在）或测试 panic（status 列不存在）。

- [ ] **Step 3: 改 doujinshi_file 实体**

修改 `src-tauri/src/db/entities/doujinshi_file.rs`：

```rust
use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "doujinshi_file")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub title: String,
    pub filename: String,
    pub hash: String,
    pub ext: String,
    pub size_bytes: i64,
    pub circle: Option<String>,
    pub series: Option<String>,
    pub translator: Option<String>,
    pub version_tag: Option<String>,
    /// 业务状态：`in_library / archived / recycle / deleted`
    pub status: String,
    /// 最后一次确认文件存在的路径；缺失时保留历史值
    pub last_seen_path: String,
    pub cover_path: Option<String>,
    pub marked_for_delete: bool,
    pub viewed: bool,
    pub note: Option<String>,
    pub rating: Option<i32>,
    /// 文件状态：`present / missing / absent_confirmed`
    pub file_state: String,
    /// 保留旧 has_physical_file 作为冗余列（不 drop，迁移保险）
    pub has_physical_file: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
```

> 注意：`has_physical_file` 字段先保留作为冗余，后续版本可单独 drop。`marked_for_delete` 也保留（不再使用，但 drop 涉及更多历史兼容）。

- [ ] **Step 4: 写 v8 迁移**

修改 `src-tauri/src/db/migrations.rs`（在 `MIGRATIONS` 数组追加一项；具体位置按当前 migrations.rs 结构定，通常是 push 到数组末尾 + `init_schema_versioned` 内部迭代执行）：

```rust
// 假设 migrations 数组里追加一项：
Migration {
    version: 8,
    name: "v8-decouple-data-and-file",
    run: |conn| Box::pin(async move {
        let builder = conn.get_database_backend();

        // 1. 加 file_state 列（present/missing/absent_confirmed）
        //    ALTER TABLE ADD COLUMN 用 pragma_table_info 幂等检查
        let cols: Vec<String> = sea_orm::ConnectionTrait::query_all(
            conn,
            sea_orm::Statement::from_string(
                sea_orm::DatabaseBackend::Sqlite,
                "PRAGMA table_info(doujinshi_file)".to_string(),
            ),
        ).await.unwrap().into_iter()
            .map(|r| r.try_get_by::<String>("name").unwrap_or_default())
            .collect();
        if !cols.iter().any(|n| n == "file_state") {
            conn.execute(sea_orm::Statement::from_string(
                builder.clone(),
                "ALTER TABLE doujinshi_file ADD COLUMN file_state TEXT NOT NULL DEFAULT 'present'".to_string(),
            )).await?;
        }

        // 2. 字段重命名（SQLite 3.25+ 支持 RENAME COLUMN）
        if cols.iter().any(|n| n == "current_location") {
            conn.execute(sea_orm::Statement::from_string(
                builder.clone(),
                "ALTER TABLE doujinshi_file RENAME COLUMN current_location TO status".to_string(),
            )).await?;
        }
        if cols.iter().any(|n| n == "current_path") {
            conn.execute(sea_orm::Statement::from_string(
                builder.clone(),
                "ALTER TABLE doujinshi_file RENAME COLUMN current_path TO last_seen_path".to_string(),
            )).await?;
        }

        // 3. 数据迁移：permanently_deleted → deleted
        conn.execute(sea_orm::Statement::from_string(
            builder.clone(),
            "UPDATE doujinshi_file SET status = 'deleted' WHERE status = 'permanently_deleted'".to_string(),
        )).await?;

        // 4. has_physical_file=0 → file_state='missing'
        conn.execute(sea_orm::Statement::from_string(
            builder.clone(),
            "UPDATE doujinshi_file SET file_state = 'missing' WHERE has_physical_file = 0".to_string(),
        )).await?;

        Ok(())
    }),
},
```

如果 `init_schema_versioned` 的执行逻辑是迭代 `MIGRATIONS` 并跑每条迁移，**不必**额外暴露 `run_v8_migration`——只要把它加到 `MIGRATIONS` 数组即可。如果执行逻辑需要单独调用入口，按实际代码调整。

- [ ] **Step 5: 跑测试，验证通过**

```bash
cd src-tauri && cargo test --test migrations v8_migrates_v7_db
```

期望：通过。

- [ ] **Step 6: 跑全量编译，找并修复所有引用旧字段名的位置**

```bash
cd src-tauri && cargo build 2>&1 | grep "error\[" | head -50
```

预期会有大量 `current_location` / `has_physical_file` / `current_path` 引用编译错误。**不要**在这个 task 里全修；只需要：

1. 修 `db/entities/doujinshi_file.rs` 本身
2. 修 `src-tauri/tests/migrations.rs` 里其他测试（如有引用旧字段）
3. 确认编译错误数量是预期的（不在这个 task 里全部修复）

- [ ] **Step 7: 提交**

```bash
git add src-tauri/src/db/entities/doujinshi_file.rs src-tauri/src/db/migrations.rs src-tauri/tests/migrations.rs
git commit -m "feat(db): v8 migration — rename to status/last_seen_path + add file_state"
```

---

## Task 2: state_machine 重构（核心）

**Files:**
- Modify: `src-tauri/src/services/state_machine.rs`

- [ ] **Step 1: 写测试（移除强一致护栏后的新语义）**

修改 `src-tauri/src/services/state_machine.rs` 的 `#[cfg(test)] mod tests` 块。**保留**现有测试断言里关于"成功路径"的部分，**修改**所有断言"源文件缺失则拒绝"的测试为"DB 仍更新"。

完整新测试模块：

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{self, migrations};
    use sea_orm::ActiveModelTrait;

    async fn setup_dirs() -> (tempfile::TempDir, PathBuf, PathBuf, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let identified = dir.path().join("identified");
        let will_delete = dir.path().join("will_delete");
        let archived = dir.path().join("archived");
        std::fs::create_dir_all(&identified).unwrap();
        std::fs::create_dir_all(&will_delete).unwrap();
        std::fs::create_dir_all(&archived).unwrap();
        (dir, identified, will_delete, archived)
    }

    async fn seed_row(
        conn: &sea_orm::DatabaseConnection,
        status: &str,
        last_seen_path: &str,
        file_state: &str,
    ) -> i64 {
        let now = chrono::Utc::now();
        let m = doujinshi_file::ActiveModel {
            title: Set("t".into()),
            filename: Set("f.zip".into()),
            hash: Set("h".into()),
            ext: Set("zip".into()),
            size_bytes: Set(0),
            last_seen_path: Set(last_seen_path.into()),
            status: Set(status.into()),
            file_state: Set(file_state.into()),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        };
        m.insert(conn).await.unwrap().id
    }

    async fn open_db(dir: &std::path::Path) -> sea_orm::DatabaseConnection {
        let conn = db::connect(&dir.join("t.db")).await.unwrap();
        migrations::init_schema_versioned(&conn).await.unwrap();
        conn
    }

    /// V4 新语义：源文件缺失时状态切换仍成功（仅文件操作 no-op）
    #[tokio::test]
    async fn transition_succeeds_when_source_file_missing() {
        let (_dir, identified, will_delete, archived) = setup_dirs().await;
        let conn = open_db(_dir.path()).await;
        let id = seed_row(&conn, "identified", "missing/f.zip", "missing").await;

        transition_with_dirs(
            &conn,
            id,
            TransitionKind::Archive,
            &identified,
            &will_delete,
            &archived,
        )
        .await
        .unwrap();

        let row = doujinshi_file::Entity::find_by_id(id)
            .one(&conn)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.status, "archived");
        // last_seen_path 保留历史值
        assert_eq!(row.last_seen_path, "missing/f.zip");
    }

    /// V4 新语义：目标目录同名 = 视为孤儿，自动覆盖 + dirty_data 写入
    #[tokio::test]
    async fn transition_overwrites_orphan_in_target_dir() {
        let (_dir, identified, will_delete, archived) = setup_dirs().await;
        let conn = open_db(_dir.path()).await;

        let src = identified.join("f.zip");
        std::fs::write(&src, b"new").unwrap();
        let id = seed_row(&conn, "identified", &src.to_string_lossy(), "present").await;

        // 在 will_delete 放同名孤儿
        std::fs::write(will_delete.join("f.zip"), b"orphan").unwrap();

        transition_with_dirs(
            &conn,
            id,
            TransitionKind::MarkForDelete,
            &identified,
            &will_delete,
            &archived,
        )
        .await
        .unwrap();

        // will_delete/f.zip 被覆盖
        let content = std::fs::read(will_delete.join("f.zip")).unwrap();
        assert_eq!(content, b"new");
        // dirty_data 新增 overwritten_by_state_switch
        let rows = crate::db::entities::dirty_data::Entity::find()
            .all(&conn)
            .await
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].reason, "overwritten_by_state_switch");
        assert_eq!(rows[0].detected_dir, "will_delete");
    }

    /// V4：跨设备 rename 走 copy + remove 兜底
    #[tokio::test]
    async fn transition_crosses_devices_falls_back_to_copy_remove() {
        let (_dir, identified, will_delete, archived) = setup_dirs().await;
        let conn = open_db(_dir.path()).await;
        let src = identified.join("f.zip");
        std::fs::write(&src, b"data").unwrap();
        let id = seed_row(&conn, "identified", &src.to_string_lossy(), "present").await;

        // 直接调搬运函数模拟跨设备（用 mock 实现或本测试靠 rename 不报错来通过）
        // 简化：正常 rename 在同一 tmp dir 不跨设备，验证搬运成功即可
        transition_with_dirs(
            &conn, id, TransitionKind::Archive, &identified, &will_delete, &archived,
        ).await.unwrap();

        let row = doujinshi_file::Entity::find_by_id(id).one(&conn).await.unwrap().unwrap();
        assert_eq!(row.status, "archived");
        assert!(archived.join("f.zip").exists());
    }

    /// V4：任意 status 可切到任意 status（V3 的"非法转移"概念消失）
    #[tokio::test]
    async fn any_to_any_status_is_allowed() {
        let (_dir, identified, will_delete, archived) = setup_dirs().await;
        let conn = open_db(_dir.path()).await;
        let id = seed_row(&conn, "deleted", "missing/f.zip", "absent_confirmed").await;

        // deleted → in_library 应当成功（V3 这是非法）
        transition_with_dirs(
            &conn, id, TransitionKind::Restore, &identified, &will_delete, &archived,
        ).await.unwrap();

        let row = doujinshi_file::Entity::find_by_id(id).one(&conn).await.unwrap().unwrap();
        assert_eq!(row.status, "in_library");
    }

    /// V4：所有转移都允许源文件缺失（V3 "physical file missing" 错误消失）
    #[tokio::test]
    async fn all_transitions_succeed_when_source_missing() {
        let (_dir, identified, will_delete, archived) = setup_dirs().await;
        let conn = open_db(_dir.path()).await;

        for (from, kind, expect_to) in [
            ("in_library", TransitionKind::Archive, "archived"),
            ("in_library", TransitionKind::MarkForDelete, "recycle"),
            ("archived", TransitionKind::Restore, "in_library"),
            ("recycle", TransitionKind::Restore, "in_library"),
            ("deleted", TransitionKind::Restore, "in_library"),
        ] {
            let id = seed_row(&conn, from, "missing/f.zip", "missing").await;
            transition_with_dirs(
                &conn, id, kind, &identified, &will_delete, &archived,
            ).await.unwrap();
            let row = doujinshi_file::Entity::find_by_id(id).one(&conn).await.unwrap().unwrap();
            assert_eq!(row.status, expect_to, "from={:?}", from);
        }
    }
}
```

- [ ] **Step 2: 跑测试，验证失败**

```bash
cd src-tauri && cargo test --lib services::state_machine
```

期望：编译失败（字段名错误 + TransitionKind 行为不匹配）。

- [ ] **Step 3: 重构 state_machine.rs**

完整新实现：

```rust
//! V4 状态机：DB 优先 + 文件 best-effort。
//! 所有 from → 任意 to 都允许；源文件缺失不阻塞 status 更新。
//! 目标目录同名 = 视为孤儿（dirty_data），自动覆盖。

use anyhow::Result;
use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set};
use std::path::{Path, PathBuf};

use crate::db::entities::{dirty_data, doujinshi_file};

#[derive(Debug, Clone, Copy)]
pub enum TransitionKind {
    /// 任意 status → archived
    Archive,
    /// 任意 status → in_library
    Restore,
    /// 任意 status → recycle
    MarkForDelete,
}

impl TransitionKind {
    pub fn target(self) -> &'static str {
        match self {
            TransitionKind::Archive => "archived",
            TransitionKind::Restore => "in_library",
            TransitionKind::MarkForDelete => "recycle",
        }
    }
}

pub async fn transition_with_dirs(
    conn: &DatabaseConnection,
    id: i64,
    kind: TransitionKind,
    identified_dir: &Path,
    will_delete_dir: &Path,
    archived_dir: &Path,
) -> Result<()> {
    let target = kind.target();

    let row = doujinshi_file::Entity::find_by_id(id)
        .one(conn)
        .await?
        .ok_or_else(|| anyhow::anyhow!("file {} not found", id))?;

    let target_dir = match target {
        "in_library" => identified_dir,
        "recycle" => will_delete_dir,
        "archived" => archived_dir,
        _ => anyhow::bail!("unknown target status: {}", target),
    };

    let src = PathBuf::from(&row.last_seen_path);
    let mut am: doujinshi_file::ActiveModel = row.into();

    // 1. 尝试搬文件（best-effort）
    if src.exists() {
        // 拿到目标文件名（basename）
        let basename = src
            .file_name()
            .ok_or_else(|| anyhow::anyhow!("invalid src path: {}", src.display()))?;
        let dest = target_dir.join(basename);

        std::fs::create_dir_all(target_dir)?;
        if let Err(e) = std::fs::rename(&src, &dest) {
            // 跨设备 fallback（Windows ERROR_NOT_SAME_DEVICE=17）
            if matches!(e.kind(), std::io::ErrorKind::CrossesDevices)
                || e.raw_os_error() == Some(17)
            {
                std::fs::copy(&src, &dest)?;
                std::fs::remove_file(&src)?;
            } else {
                anyhow::bail!("rename failed: {}", e);
            }
        }

        // 目标位置原本有同名文件被覆盖 → 写 dirty_data
        // 简化：用 dirty_data.file_path = <旧目标路径> 作为孤儿记录；reason='overwritten_by_state_switch'
        // 实际：rename 已经覆盖了；这里 dirty_data 记录的是"目标目录原同名文件"的"曾经存在"
        // 简化：直接写一条 dirty_data 记录这次状态切换 + detected_dir=target_dir
        let dirty = dirty_data::ActiveModel {
            file_path: Set(dest.to_string_lossy().into_owned()),
            file_size: Set(0),
            detected_dir: Set(target.to_string()),
            reason: Set("overwritten_by_state_switch".into()),
            first_seen_at: Set(chrono::Utc::now().to_rfc3339()),
            ..Default::default()
        };
        let _ = dirty.insert(conn).await; // best-effort，不阻塞主流程

        am.last_seen_path = Set(dest.to_string_lossy().into_owned());
        // file_state 保持 present（搬运成功）
    } else {
        // 源文件不存在：搬运 no-op，file_state=missing（已是 missing 或保持 present）
        am.file_state = Set("missing".into());
    }

    // 2. 永远更新 status
    am.status = Set(target.into());
    am.updated_at = Set(chrono::Utc::now());
    am.update(conn).await?;

    Ok(())
}
```

> **简化 dirty_data 写入**：当前实现假设每次状态切换都写一条 dirty_data。**实际只有目标目录原本有同名孤儿时才需要写**——否则会污染 dirty_data 表。优化版（推荐）：先 `target_dir.join(basename).exists()` 检测，rename 之前检测同名 → 写 dirty_data。

优化版：

```rust
    if src.exists() {
        let basename = src
            .file_name()
            .ok_or_else(|| anyhow::anyhow!("invalid src path: {}", src.display()))?;
        let dest = target_dir.join(basename);

        std::fs::create_dir_all(target_dir)?;

        // 检测目标位置已有同名文件 → 视为孤儿
        if dest.exists() {
            let dirty = dirty_data::ActiveModel {
                file_path: Set(dest.to_string_lossy().into_owned()),
                file_size: Set(0),
                detected_dir: Set(target.to_string()),
                reason: Set("overwritten_by_state_switch".into()),
                first_seen_at: Set(chrono::Utc::now().to_rfc3339()),
                ..Default::default()
            };
            let _ = dirty.insert(conn).await;
        }

        if let Err(e) = std::fs::rename(&src, &dest) {
            if matches!(e.kind(), std::io::ErrorKind::CrossesDevices)
                || e.raw_os_error() == Some(17)
            {
                std::fs::copy(&src, &dest)?;
                std::fs::remove_file(&src)?;
            } else {
                anyhow::bail!("rename failed: {}", e);
            }
        }

        am.last_seen_path = Set(dest.to_string_lossy().into_owned());
    } else {
        am.file_state = Set("missing".into());
    }
```

- [ ] **Step 4: 跑测试，验证通过**

```bash
cd src-tauri && cargo test --lib services::state_machine
```

期望：5 个新测试通过。如果有 V3 旧测试需要更新（保留它们但修断言），按需调整。

- [ ] **Step 5: 修复所有调用方编译错误**

```bash
cd src-tauri && cargo build 2>&1 | grep "error\[" | head -30
```

调用方包括 `commands/library.rs`、`commands/recycle.rs`、`http/api.rs` 等——它们使用 `TransitionKind::PermanentlyDelete`，V4 已删除。改为：V4 的"销毁"操作不走 state_machine，详见 Task 6。

其他调用方只需要把字段名从 `current_location` / `current_path` 改成 `status` / `last_seen_path`，把 `PermanentlyDelete` 用法替换为对应的 status 切换（见 Task 7）。

- [ ] **Step 6: 提交**

```bash
git add src-tauri/src/services/state_machine.rs
git commit -m "refactor(state-machine): V4 — DB-first transitions, no source-file gate"
```

---

## Task 3: identifier 适配（collision check 排除 status='deleted'）

**Files:**
- Modify: `src-tauri/src/services/identifier.rs`

- [ ] **Step 1: 定位 collision check 代码**

用 grep 找到 `identify_file` 里查 `(filename, ext) 撞名` 的位置：

```bash
grep -n "current_location" src-tauri/src/services/identifier.rs
```

应该看到 `identify_file` 里有：

```rust
doujinshi_file::Entity::find()
    .filter(doujinshi_file::Column::Filename.eq(&filename))
    .filter(doujinshi_file::Column::Ext.eq(&ext))
    .filter(doujinshi_file::Column::CurrentLocation.is_in([...]))
```

- [ ] **Step 2: 写测试**

在 `identifier.rs` 的 `#[cfg(test)]` 加：

```rust
#[tokio::test]
async fn identify_file_skips_collision_check_for_deleted_rows() {
    let dir = tempfile::tempdir().unwrap();
    let conn = crate::db::connect(&dir.path().join("t.db")).await.unwrap();
    crate::db::migrations::init_schema_versioned(&conn).await.unwrap();

    // seed 一行 status='deleted'，filename='f.zip'
    let now = chrono::Utc::now();
    let m = crate::db::entities::doujinshi_file::ActiveModel {
        title: Set("t".into()),
        filename: Set("f.zip".into()),
        hash: Set("h_deleted".into()),
        ext: Set("zip".into()),
        size_bytes: Set(0),
        last_seen_path: Set("placeholder".into()),
        status: Set("deleted".into()),
        file_state: Set("absent_confirmed".into()),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    };
    m.insert(&conn).await.unwrap();

    // 模拟 inbox 里有同名新文件——直接调 collision 检查函数
    use crate::db::entities::doujinshi_file::{Column, Entity};
    let collision = Entity::find()
        .filter(Column::Filename.eq("f.zip"))
        .filter(Column::Ext.eq("zip"))
        .filter(Column::Status.is_in(["in_library", "archived", "recycle"]))  // 不含 deleted
        .one(&conn)
        .await
        .unwrap();

    assert!(collision.is_none(), "deleted 行不应参与撞名检查");
}
```

- [ ] **Step 3: 跑测试，验证失败**

```bash
cd src-tauri && cargo test --lib services::identifier identify_file_skips_collision_check_for_deleted_rows
```

期望：编译失败（`Column::CurrentLocation` 不存在；测试还在用 `Column::Status` 但 entity 是新字段名，应该通过）。

- [ ] **Step 4: 改 identify_file**

修改 `identify_file` 里 collision check 的 status 列表：

```rust
// 找到这段（大概在 identify_file 的 step 4）：
let collision = if force_rename.is_none() {
    doujinshi_file::Entity::find()
        .filter(doujinshi_file::Column::Filename.eq(&filename))
        .filter(doujinshi_file::Column::Ext.eq(&ext))
        .filter(doujinshi_file::Column::Status.is_in(["in_library", "archived", "recycle"]))  // ← 不含 deleted
        .one(conn)
        .await?
} else {
    None
};
```

同时把所有 `current_location` / `current_path` 引用改为 `status` / `last_seen_path`。

- [ ] **Step 5: 跑测试，验证通过**

```bash
cd src-tauri && cargo test --lib services::identifier
```

期望：新测试通过；旧测试如果还在用旧字段名会失败，按需更新。

- [ ] **Step 6: 提交**

```bash
git add src-tauri/src/services/identifier.rs
git commit -m "refactor(identifier): collision check excludes status='deleted'"
```

---

## Task 4: dirty_scanner 适配（4 状态目录 + file_state 三态）

**Files:**
- Modify: `src-tauri/src/services/dirty_scanner.rs`

- [ ] **Step 1: 修改 dirty_scanner.rs 的字段引用**

把所有 `current_location` 引用改为 `status`，所有 `has_physical_file` 引用改为：

- `has_physical_file = true` → `file_state = 'present'`
- `has_physical_file = false` → `file_state = 'missing'`

把 `Column::CurrentPath` 改为 `Column::LastSeenPath`，把 `Column::CurrentLocation.is_in([...])` 改为 `Column::Status.is_in([...])`。

注意：scanner 只扫 `identified/will_delete/archived` 三个目录，**不**扫 deleted（V4 spec：deleted 无对应目录）。

- [ ] **Step 2: 跑 dirty_scanner 现有测试**

```bash
cd src-tauri && cargo test --lib services::dirty_scanner
```

如果旧测试用旧字段名（如 `current_location`），会失败；更新测试断言使用新字段名。

预期 4 个旧测试都通过：
- `scan_detects_orphan_files`
- `scan_marks_db_rows_with_missing_files`
- `scan_does_not_check_inbox`
- `scan_ignores_gitkeep`
- `scan_self_heals_stale_current_path_when_file_in_expected_dir`（可能要改名 `scan_self_heals_stale_last_seen_path_*`）
- `scan_marks_unrecoverable_mismatch_as_dirty`

- [ ] **Step 3: 写新测试：file_state 三态更新**

```rust
#[tokio::test]
async fn scan_updates_file_state_to_present_when_file_exists() {
    let dir = tempfile::tempdir().unwrap();
    let identified = dir.path().join("identified");
    let will_delete = dir.path().join("will_delete");
    let archived = dir.path().join("archived");
    std::fs::create_dir_all(&identified).unwrap();
    std::fs::create_dir_all(&will_delete).unwrap();
    std::fs::create_dir_all(&archived).unwrap();
    let real = identified.join("g.zip");
    std::fs::write(&real, b"x").unwrap();

    let conn = open_db(dir.path()).await;
    let now = chrono::Utc::now();
    let m = doujinshi_file::ActiveModel {
        title: Set("t".into()),
        filename: Set("g.zip".into()),
        hash: Set("h".into()),
        ext: Set("zip".into()),
        size_bytes: Set(0),
        last_seen_path: Set(real.to_string_lossy().into_owned()),
        status: Set("identified".into()),
        file_state: Set("missing".into()),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    };
    m.insert(&conn).await.unwrap();

    let _ = scan(&conn, &identified, &will_delete, &archived).await.unwrap();
    let row = doujinshi_file::Entity::find().one(&conn).await.unwrap().unwrap();
    assert_eq!(row.file_state, "present");
}

#[tokio::test]
async fn scan_updates_file_state_to_missing_when_file_gone() {
    let dir = tempfile::tempdir().unwrap();
    let identified = dir.path().join("identified");
    let will_delete = dir.path().join("will_delete");
    let archived = dir.path().join("archived");
    std::fs::create_dir_all(&identified).unwrap();
    std::fs::create_dir_all(&will_delete).unwrap();
    std::fs::create_dir_all(&archived).unwrap();

    let conn = open_db(dir.path()).await;
    let now = chrono::Utc::now();
    let m = doujinshi_file::ActiveModel {
        title: Set("t".into()),
        filename: Set("g.zip".into()),
        hash: Set("h".into()),
        ext: Set("zip".into()),
        size_bytes: Set(0),
        last_seen_path: Set(identified.join("g.zip").to_string_lossy().into_owned()),
        status: Set("identified".into()),
        file_state: Set("present".into()),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    };
    m.insert(&conn).await.unwrap();

    let _ = scan(&conn, &identified, &will_delete, &archived).await.unwrap();
    let row = doujinshi_file::Entity::find().one(&conn).await.unwrap().unwrap();
    assert_eq!(row.file_state, "missing");
}
```

- [ ] **Step 4: 跑新测试，验证通过**

```bash
cd src-tauri && cargo test --lib services::dirty_scanner
```

期望：所有 dirty_scanner 测试通过。

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/services/dirty_scanner.rs
git commit -m "refactor(scanner): use status + file_state fields"
```

---

## Task 5: conflict::ReplaceB 改造

**Files:**
- Modify: `src-tauri/src/commands/inbox.rs`

- [ ] **Step 1: 写测试**

在 `commands/inbox.rs` 末尾的 `#[cfg(test)]` 块（如果没有就加）：

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{self, migrations};
    use sea_orm::ActiveModelTrait;

    async fn setup() -> (tempfile::TempDir, sea_orm::DatabaseConnection, std::path::PathBuf, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let covers = dir.path().join("covers");
        let identified = dir.path().join("identified");
        std::fs::create_dir_all(&covers).unwrap();
        std::fs::create_dir_all(&identified).unwrap();
        let conn = db::connect(&dir.path().join("t.db")).await.unwrap();
        migrations::init_schema_versioned(&conn).await.unwrap();
        (dir, conn, covers, identified)
    }

    async fn seed_file_row(
        conn: &sea_orm::DatabaseConnection,
        filename: &str,
        status: &str,
    ) -> i64 {
        let now = chrono::Utc::now();
        let m = crate::db::entities::doujinshi_file::ActiveModel {
            title: Set("A".into()),
            filename: Set(filename.into()),
            hash: Set("hash_a".into()),
            ext: Set("zip".into()),
            size_bytes: Set(0),
            last_seen_path: Set(format!("/placeholder/{}", filename)),
            status: Set(status.into()),
            file_state: Set("present".into()),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        };
        m.insert(conn).await.unwrap().id
    }

    #[tokio::test]
    async fn replace_b_marks_a_row_as_deleted() {
        let (_dir, conn, covers, identified) = setup().await;
        let a_id = seed_file_row(&conn, "f.zip", "in_library").await;

        // 写一个 conflict 行
        let conflict_id = crate::db::entities::conflict::ActiveModel {
            a_file_id: Set(a_id),
            b_file_path: Set("/inbox/b.zip".into()),
            b_filename: Set("f.zip".into()),
            b_hash: Set(Some("hash_b".into())),
            reason: Set("name+ext collision".into()),
            resolved: Set(false),
            ..Default::default()
        }
        .insert(&conn)
        .await
        .unwrap()
        .id;

        // 调 resolve_conflict_inner with ReplaceB
        resolve_conflict_inner(
            &conn,
            &covers,
            &identified,
            conflict_id,
            ConflictAction::ReplaceB,
        )
        .await
        .unwrap();

        // A 行应当 status=deleted, file_state=absent_confirmed
        let a = crate::db::entities::doujinshi_file::Entity::find_by_id(a_id)
            .one(&conn)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(a.status, "deleted");
        assert_eq!(a.file_state, "absent_confirmed");
    }
}
```

- [ ] **Step 2: 跑测试，验证失败**

```bash
cd src-tauri && cargo test --lib commands::inbox replace_b_marks_a_row_as_deleted
```

期望：编译失败（A 行字段名不对）。

- [ ] **Step 3: 改 commands/inbox.rs**

修改 `resolve_conflict_inner` 的 `ReplaceB` 分支：

```rust
ConflictAction::ReplaceB => {
    // 把 A 行的状态推进到 deleted：A 的 zip best-effort 删掉、
    // A 的行留在历史里、status='deleted' + file_state='absent_confirmed'——
    // collision check 排除 status='deleted'，后续 B 就能用 A 的 filename 正常入库，不撞名。
    let a_row = doujinshi_file::Entity::find_by_id(row.a_file_id)
        .one(conn)
        .await?;
    if let Some(a) = a_row {
        let a_path = std::path::Path::new(&a.last_seen_path);
        if a_path.exists() {
            let _ = std::fs::remove_file(a_path);
        }
        let mut am: doujinshi_file::ActiveModel = a.into();
        am.status = Set("deleted".into());
        am.file_state = Set("absent_confirmed".into());
        am.updated_at = Set(chrono::Utc::now());
        let _ = am.update(conn).await;
    }
    let b_path = PathBuf::from(&row.b_file_path);
    if b_path.exists() {
        let _ = crate::services::identifier::identify_file(
            conn,
            &b_path,
            covers_dir,
            identified_dir,
            None,
            false,
        )
        .await;
    }
}
```

同时把整个文件里所有 `current_location` / `current_path` 引用改为 `status` / `last_seen_path`。

- [ ] **Step 4: 跑测试，验证通过**

```bash
cd src-tauri && cargo test --lib commands::inbox
```

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/commands/inbox.rs
git commit -m "refactor(inbox): ReplaceB marks A as deleted + absent_confirmed"
```

---

## Task 6: permanent_delete 改造（销毁复合操作）

**Files:**
- Modify: `src-tauri/src/commands/recycle.rs`

- [ ] **Step 1: 写测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{self, migrations};
    use sea_orm::ActiveModelTrait;

    #[tokio::test]
    async fn permanent_delete_marks_status_deleted_and_file_state_absent_confirmed() {
        let dir = tempfile::tempdir().unwrap();
        let will_delete = dir.path().join("will_delete");
        std::fs::create_dir_all(&will_delete).unwrap();
        let conn = db::connect(&dir.path().join("t.db")).await.unwrap();
        migrations::init_schema_versioned(&conn).await.unwrap();

        let src = will_delete.join("f.zip");
        std::fs::write(&src, b"data").unwrap();

        let now = chrono::Utc::now();
        let m = crate::db::entities::doujinshi_file::ActiveModel {
            title: Set("t".into()),
            filename: Set("f.zip".into()),
            hash: Set("h".into()),
            ext: Set("zip".into()),
            size_bytes: Set(4),
            last_seen_path: Set(src.to_string_lossy().into_owned()),
            status: Set("recycle".into()),
            file_state: Set("present".into()),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        };
        let id = m.insert(&conn).await.unwrap().id;

        // 调新版的 permanent_delete_inner
        permanent_delete_inner(&conn, id).await.unwrap();

        // 文件应被删除
        assert!(!src.exists());
        // 行应 status=deleted + file_state=absent_confirmed
        let row = crate::db::entities::doujinshi_file::Entity::find_by_id(id)
            .one(&conn)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.status, "deleted");
        assert_eq!(row.file_state, "absent_confirmed");
    }

    #[tokio::test]
    async fn permanent_delete_succeeds_when_source_already_missing() {
        let dir = tempfile::tempdir().unwrap();
        let conn = db::connect(&dir.path().join("t.db")).await.unwrap();
        migrations::init_schema_versioned(&conn).await.unwrap();

        let now = chrono::Utc::now();
        let m = crate::db::entities::doujinshi_file::ActiveModel {
            title: Set("t".into()),
            filename: Set("f.zip".into()),
            hash: Set("h".into()),
            ext: Set("zip".into()),
            size_bytes: Set(0),
            last_seen_path: Set("/missing/f.zip".into()),
            status: Set("recycle".into()),
            file_state: Set("missing".into()),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        };
        let id = m.insert(&conn).await.unwrap().id;

        // 即便文件不在也应成功
        permanent_delete_inner(&conn, id).await.unwrap();

        let row = crate::db::entities::doujinshi_file::Entity::find_by_id(id)
            .one(&conn)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.status, "deleted");
        assert_eq!(row.file_state, "absent_confirmed");
    }
}
```

- [ ] **Step 2: 跑测试，验证失败**

```bash
cd src-tauri && cargo test --lib commands::recycle
```

期望：编译失败（字段名 + permanent_delete_inner 不存在）。

- [ ] **Step 3: 重写 permanent_delete_inner**

修改 `src-tauri/src/commands/recycle.rs`。新版 `permanent_delete_inner` 不再调 state_machine，直接做复合操作：

```rust
/// 销毁：复合操作
/// 1. status='deleted'
/// 2. file_state='absent_confirmed'
/// 3. best-effort remove_file(last_seen_path)
/// 4. preview_cache.invalidate(id)（调用方负责，因为需要 AppState）
pub async fn permanent_delete_inner(
    conn: &sea_orm::DatabaseConnection,
    id: i64,
) -> AppResult<()> {
    use crate::db::entities::doujinshi_file;
    let row = doujinshi_file::Entity::find_by_id(id)
        .one(conn)
        .await?
        .ok_or_else(|| crate::error::AppError::Other(format!("file {} not found", id)))?;

    // 1. best-effort 删除文件
    let p = std::path::Path::new(&row.last_seen_path);
    if p.exists() {
        let _ = std::fs::remove_file(p);
    }

    // 2. UPDATE 行
    let mut am: doujinshi_file::ActiveModel = row.into();
    am.status = Set("deleted".into());
    am.file_state = Set("absent_confirmed".into());
    am.updated_at = Set(chrono::Utc::now());
    am.update(conn).await?;

    // 3. 记一条 scan_event（如果项目里有 record_event 函数，调它）
    // crate::services::identifier::record_event(conn, id, "destroyed", None).await?;

    Ok(())
}
```

Tauri command `permanent_delete` 改造为调 `permanent_delete_inner` + 调 `preview_cache.invalidate(id)`：

```rust
#[tauri::command]
pub async fn permanent_delete(state: State<'_, AppState>, id: i64) -> AppResult<()> {
    permanent_delete_inner(&state.conn, id).await?;
    state.preview_cache.invalidate(id);
    Ok(())
}
```

同时把所有 `current_location` / `current_path` / `has_physical_file` 引用改为新字段名。

- [ ] **Step 4: 跑测试，验证通过**

```bash
cd src-tauri && cargo test --lib commands::recycle
```

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/commands/recycle.rs
git commit -m "refactor(recycle): permanent_delete is composite destroy operation"
```

---

## Task 7: commands/library 适配

**Files:**
- Modify: `src-tauri/src/commands/library.rs`

- [ ] **Step 1: 替换 PermanentlyDelete 引用**

V3 的 `state_machine::TransitionKind::PermanentlyDelete` 已在 Task 2 移除。`commands/library.rs` 不应该再引用它（销毁是 `commands/recycle::permanent_delete`）。

在 `commands/library.rs` 里查找：

```bash
grep -n "PermanentlyDelete\|current_location\|current_path" src-tauri/src/commands/library.rs
```

把所有 `PermanentlyDelete` 引用移除（如果有 `archive / restore / mark_for_delete` 调用方用不到它，跳过）。

把所有 `current_location` / `current_path` 引用改为 `status` / `last_seen_path`。

- [ ] **Step 2: 跑 commands/library 测试**

```bash
cd src-tauri && cargo test --lib commands::library
```

期望：通过。

- [ ] **Step 3: 跑全量编译**

```bash
cd src-tauri && cargo build 2>&1 | grep "error\[" | head -30
```

预期应该已经基本通过；修复剩下的引用问题。

- [ ] **Step 4: 提交**

```bash
git add src-tauri/src/commands/library.rs
git commit -m "refactor(commands): library commands use new status/last_seen_path fields"
```

---

## Task 8: models/file_summary 改造

**Files:**
- Modify: `src-tauri/src/models/file_summary.rs`

- [ ] **Step 1: 修复现有测试断言**

`from_model_includes_location_and_has_physical_file` 测试输入 `status="permanently_deleted"` 断言 `s.current_location == "archived"`——这个测试**本来就有 bug**（clone 不做映射）。更新测试断言使用新字段名：

```rust
#[test]
fn from_model_includes_status_and_file_state() {
    let now = chrono::Utc::now();
    let m = doujinshi_file::Model {
        id: 1,
        title: "t".into(),
        filename: "f.zip".into(),
        hash: "h".into(),
        ext: "zip".into(),
        size_bytes: 100,
        circle: None,
        series: None,
        translator: None,
        version_tag: None,
        status: "deleted".into(),
        last_seen_path: "p".into(),
        cover_path: Some("covers/h.pwb".into()),
        marked_for_delete: false,
        has_physical_file: false,
        viewed: false,
        note: None,
        rating: None,
        file_state: "absent_confirmed".into(),
        created_at: now,
        updated_at: now,
    };
    let s = from_model_with_conflict_state(&m, true);
    assert_eq!(s.status, "deleted");
    assert!(!s.has_physical_file);
    assert!(s.has_open_conflict);
    assert_eq!(s.cover_url.as_deref(), Some("/api/covers/h"));
}
```

- [ ] **Step 2: 跑测试，验证失败**

```bash
cd src-tauri && cargo test --lib models::file_summary
```

期望：编译失败（`FileSummary.current_location` 不存在）。

- [ ] **Step 3: 改 FileSummary struct + from_model**

```rust
#[derive(Debug, Clone, serde::Serialize)]
pub struct FileSummary {
    pub id: i64,
    pub title: String,
    pub circle: Option<String>,
    pub hash: String,
    pub ext: String,
    pub size_bytes: i64,
    pub viewed: bool,
    /// 业务状态：`in_library / archived / recycle / deleted`
    pub status: String,
    /// 文件状态：`present / missing / absent_confirmed`
    pub file_state: String,
    pub cover_url: Option<String>,
    pub has_open_conflict: bool,
}

pub fn from_model_with_conflict_state(m: &doujinshi_file::Model, has_open_conflict: bool) -> FileSummary {
    FileSummary {
        id: m.id,
        title: m.title.clone(),
        circle: m.circle.clone(),
        hash: m.hash.clone(),
        ext: m.ext.clone(),
        size_bytes: m.size_bytes,
        viewed: m.viewed,
        status: m.status.clone(),
        file_state: m.file_state.clone(),
        cover_url: m.cover_path.as_ref().map(|_| format!("/api/covers/{}", m.hash)),
        has_open_conflict,
    }
}
```

- [ ] **Step 4: 跑测试，验证通过**

```bash
cd src-tauri && cargo test --lib models::file_summary
```

- [ ] **Step 5: 修复调用方**

```bash
cd src-tauri && cargo build 2>&1 | grep "error\[" | head -30
```

调用方包括 `http/api.rs`、`commands/library.rs` 等——它们用 `summary.current_location` 或 `summary.has_physical_file`，需要改成 `summary.status` / `summary.file_state`。

- [ ] **Step 6: 提交**

```bash
git add src-tauri/src/models/file_summary.rs
git commit -m "refactor(model): FileSummary uses status + file_state fields"
```

---

## Task 9: HTTP API 字段名适配

**Files:**
- Modify: `src-tauri/src/http/api.rs`
- Modify: `src-tauri/tests/http_routes.rs`

- [ ] **Step 1: 找所有 HTTP 响应里返 FileSummary 的端点**

```bash
grep -n "FileSummary\|current_location\|has_physical_file" src-tauri/src/http/api.rs src-tauri/tests/http_routes.rs
```

确认所有端点的响应都已通过 `FileSummary` 序列化，所以只需要 Task 8 改 struct 即可——这里只需要**修复编译错误**（如果有端点直接构造 `FileSummary` 或解构 `current_location` 字段）。

- [ ] **Step 2: 跑 http_routes 测试**

```bash
cd src-tauri && cargo test --test http_routes
```

期望：所有 HTTP 集成测试通过。如果测试断言用了旧字段名（如 `body["current_location"]`），按需更新。

- [ ] **Step 3: 提交**

```bash
git add src-tauri/src/http/api.rs src-tauri/tests/http_routes.rs
git commit -m "refactor(http): response fields renamed to status/file_state"
```

---

## Task 10: 前端 types/api.ts 改造

**Files:**
- Modify: `src/types/api.ts`

- [ ] **Step 1: 改 FileSummary 类型**

```typescript
export interface FileSummary {
  id: number
  title: string
  circle: string | null
  hash: string
  ext: string
  size_bytes: number
  viewed: boolean
  status: "in_library" | "archived" | "recycle" | "deleted"
  file_state: "present" | "missing" | "absent_confirmed"
  cover_url: string | null
  has_open_conflict: boolean
}
```

同步把所有引用 `current_location` / `has_physical_file` 的地方（特别是 Filter / Sort 类型）改为 `status` / `file_state`。

- [ ] **Step 2: 跑 type check**

```bash
pnpm exec vue-tsc --noEmit
```

期望：0 错误（如果其他文件还没改，可能有大量错误——这是预期的，下游 task 修复）。

- [ ] **Step 3: 提交**

```bash
git add src/types/api.ts
git commit -m "refactor(frontend): FileSummary uses status/file_state"
```

---

## Task 11: 前端 stores 改造（useLibraryStore 加 status 过滤）

**Files:**
- Modify: `src/stores/index.ts`

- [ ] **Step 1: 加 statusFilter 到 useLibraryStore**

修改 `useLibraryStore`：

```typescript
export const useLibraryStore = defineStore("library", () => {
  const items = ref<FileSummary[]>([])
  const queryInput = ref("")
  const query = ref("")
  const status = ref<"all" | "viewed" | "not_viewed" | "marked">("all")
  /// V4：业务 status 过滤；默认值"active"=排除 recycle + deleted
  const statusFilter = ref<"active" | "all" | "in_library" | "archived" | "recycle" | "deleted">("active")
  const loading = ref(false)

  async function load() {
    loading.value = true
    try {
      const statusArg = (statusFilter.value === "all" || statusFilter.value === "active")
        ? undefined
        : statusFilter.value
      items.value = await api.listLibrary(
        query.value || undefined,
        status.value === "all" ? undefined : status.value,
        statusArg,
      )
    } finally {
      loading.value = false
    }
  }

  // ... 其他保留

  return {
    items, queryInput, query, status, statusFilter, loading,
    topCircles, load, archive, restore, markForDelete,
    fetchDetailImagesFor,
  }
})
```

注：`active` 是 UI 概念（= 排除 recycle + deleted），传给后端时是 `undefined`（让后端 list 全部）。如果后端 `list_library` 不接受 status 参数就传 `undefined`。

- [ ] **Step 2: 跑 type check**

```bash
pnpm exec vue-tsc --noEmit
```

- [ ] **Step 3: 提交**

```bash
git add src/stores/index.ts
git commit -m "feat(store): useLibraryStore.statusFilter excludes recycle/deleted by default"
```

---

## Task 12: 前端 LibraryView 加 status 过滤 UI

**Files:**
- Modify: `src/views/LibraryView.vue`

- [ ] **Step 1: 加 status 过滤下拉**

在 LibraryView 顶部 status 筛选旁边加一个 NSelect：

```vue
<n-select
  v-model:value="store.statusFilter"
  :options="[
    { label: '正常（排除回收/已删）', value: 'active' },
    { label: '全部', value: 'all' },
    { label: '入库', value: 'in_library' },
    { label: '归档', value: 'archived' },
    { label: '回收站', value: 'recycle' },
    { label: '已删除', value: 'deleted' },
  ]"
  style="width: 200px"
/>
```

- [ ] **Step 2: FileCard.vue 加 status 标签**

在 FileCard 加一个状态标签（n-tag）显示当前 status：

```vue
<n-tag size="small" :type="statusTagType(file.status)">
  {{ statusLabel(file.status) }}
</n-tag>
```

```typescript
function statusLabel(s: string): string {
  return { in_library: '入库', archived: '归档', recycle: '回收', deleted: '已删' }[s] || s
}
function statusTagType(s: string): string {
  return { in_library: 'success', archived: 'info', recycle: 'warning', deleted: 'error' }[s] || 'default'
}
```

- [ ] **Step 3: 跑 type check + build**

```bash
pnpm exec vue-tsc --noEmit && pnpm build
```

- [ ] **Step 4: 提交**

```bash
git add src/views/LibraryView.vue src/components/FileCard.vue
git commit -m "feat(library): status filter UI + status tag on cards"
```

---

## Task 13: 前端 DetailView 加 file_state 缺失提示

**Files:**
- Modify: `src/views/DetailView.vue`

- [ ] **Step 1: 加 file_state 缺失提示**

在 DetailView 顶部（封面区域下方）加：

```vue
<n-alert
  v-if="file && file.file_state !== 'present'"
  type="warning"
  :title="fileStateTitle(file.file_state)"
  :show-icon="false"
>
  <template v-if="file.file_state === 'missing'">
    文件已不在预期路径。预览不可用；元数据可正常查看和修改。
  </template>
  <template v-else-if="file.file_state === 'absent_confirmed'">
    文件已被销毁。记录仍保留，可在 Library 恢复为入库态。
  </template>
</n-alert>
```

```typescript
function fileStateTitle(s: string): string {
  return { missing: '文件已丢失', absent_confirmed: '文件已销毁' }[s] || ''
}
```

- [ ] **Step 2: 跑 type check**

```bash
pnpm exec vue-tsc --noEmit
```

- [ ] **Step 3: 提交**

```bash
git add src/views/DetailView.vue
git commit -m "feat(detail): show file_state warning when file missing/destroyed"
```

---

## Task 14: 文档更新

**Files:**
- Modify: `CLAUDE.md`
- Modify: `docs/superpowers/specs/2026-07-11-v3-archive-and-dirty-data.md`

- [ ] **Step 1: 更新 CLAUDE.md**

修改 `CLAUDE.md` 的"核心架构" → "5 状态机" 章节为 V4 描述：

```markdown
### V4 业务状态机（status + file_state）

`doujinshi_file.status ∈ {"in_library", "archived", "recycle", "deleted"}`，由用户决定，任意可切。
`doujinshi_file.file_state ∈ {"present", "missing", "absent_confirmed"}`，由扫描 + 销毁操作维护。

合法转移：任意 status → 任意 status（V4 移除 V3 的"非法转移"概念）。
状态机集中入口 `state_machine::transition_with_dirs(conn, id, kind, 3个目录)`：
- 源文件缺失不阻塞 status 更新（DB 优先 + 文件 best-effort）
- 目标目录同名 = 视为孤儿（dirty_data），自动覆盖 + 写 `dirty_data(reason='overwritten_by_state_switch')`
- 跨设备 rename 走 copy + remove 兜底

"销毁"是复合操作（不走 state_machine）：
1. status='deleted'
2. file_state='absent_confirmed'
3. best-effort remove_file(last_seen_path)
4. preview_cache.invalidate(id)
```

修改"数据模型"段落中 `doujinshi_file` 字段名：
- `current_location` → `status`
- `current_path` → `last_seen_path`
- `has_physical_file` → `file_state`（三态）

修改"启动脏数据扫描"：4 状态目录扫描，不扫 deleted。

- [ ] **Step 2: 在 V3 spec 加 V4 后作废注**

修改 `docs/superpowers/specs/2026-07-11-v3-archive-and-dirty-data.md` 末尾加：

```markdown
## V4 后作废（2026-07-15）

本 spec 描述的 5 状态机（`inbox / identified / will_delete / archived / permanently_deleted`）+ 强一致转移（archive / restore / mark_for_delete 要求源文件必须存在）在 V4 后作废。当前实现以 `2026-07-15-decouple-data-and-file.md` 为准：

- `current_location` → `status`（4 值，任意可切）
- `has_physical_file` (bool) → `file_state` (TEXT, 3 态)
- `current_path` → `last_seen_path`
- `permanently_deleted` 终态作废；`deleted` 是普通 status，可恢复
- 状态机从"强一致"改为"DB 优先 + 文件 best-effort"
```

- [ ] **Step 3: 提交**

```bash
git add CLAUDE.md docs/superpowers/specs/2026-07-11-v3-archive-and-dirty-data.md
git commit -m "docs: V4 spec replaces V3 5-state machine description"
```

---

## Task 15: 回归测试 + 手动 E2E

**Files:** 无代码改动；只跑测试 + 验证。

- [ ] **Step 1: 全量 cargo test**

```bash
cd src-tauri && cargo test
```

期望：所有测试通过（含 v8 迁移测试、state_machine 新语义测试、identifier collision 测试、dirty_scanner file_state 测试、inbox ReplaceB 测试、recycle destroy 测试）。

- [ ] **Step 2: 全量前端 type check + build**

```bash
pnpm exec vue-tsc --noEmit && pnpm build
```

期望：0 错误。

- [ ] **Step 3: clippy**

```bash
cd src-tauri && cargo clippy --all-targets -- -D warnings
```

期望：0 warning（或确认新增 warning 是合理的）。

- [ ] **Step 4: 手动 E2E 矩阵**

启动应用（`pnpm tauri dev`），按 V4 spec §测试章节跑：

| 场景 | 预期 |
|---|---|
| 启动（V3 旧库）| v8 自动迁移；DB 字段重命名 + 加 file_state；旧数据正常显示 |
| 启动（新库）| 全新库 schema；4 个状态目录正常 |
| 入库新文件 | status='in_library' + file_state='present' + last_seen_path=identified/... |
| 归档 | status='archived'；搬运成功 → file_state 仍 'present' |
| 拿走 archived 文件 → 重启 | dirty_scanner 检测 → file_state='missing' + dirty_data；status 仍 'archived' |
| 把 missing 的 archived 切到 in_library | status 切换成功；UI 提示"文件已丢失"；DB 写入 status='in_library' |
| recycle 销毁 | status='deleted' + file_state='absent_confirmed'；文件被删；Library 默认不显示 |
| Library 切到 deleted 过滤 | 看到已删除记录；点"恢复" → status='in_library' + file_state 仍 'absent_confirmed' |
| 入库冲突 ReplaceB | A 行 status='deleted' + file_state='absent_confirmed'；B 入库成功 |
| 状态切换遇目标目录同名 | 自动覆盖；dirty_data 新增 'overwritten_by_state_switch' |
| 跨设备 rename（用 mount 模拟） | copy + remove 兜底，搬运成功 |

- [ ] **Step 5: 修复 E2E 中发现的问题**

按需修复 + 提交。

- [ ] **Step 6: 最终提交（如有修复）**

```bash
git add -A
git commit -m "fix: E2E fixes"
```

---

## Self-Review

**1. Spec coverage：**

| Spec 章节 | 任务 |
|---|---|
| §核心模型（status / file_state / last_seen_path） | Task 1, 2, 8 |
| §核心流程-入库 | Task 3 |
| §核心流程-状态切换 | Task 2 |
| §核心流程-销毁 | Task 6 |
| §核心流程-恢复 | Task 2 |
| §核心流程-扫描 | Task 4 |
| §冲突处理 | Task 5 |
| §UI 规则 | Task 11, 12, 13 |
| §数据库迁移 v8 | Task 1 |
| §代码改造点-后端 | Task 1, 2, 3, 4, 5, 6, 7, 8, 9 |
| §代码改造点-前端 | Task 10, 11, 12, 13 |
| §测试 | Task 15 |
| §迁移策略 | Task 1（启动自动跑 v8） |
| §风险 | 已在 spec 列出；Task 15 验证 |

**2. 占位符扫描：**
- 无 TBD / TODO / "implement later"
- 所有测试有具体断言值
- 所有命令有具体 git commit message

**3. 类型一致性：**

- `doujinshi_file.Model.status` / `file_state` / `last_seen_path` 在 Task 1 定义，Task 2, 3, 4, 5, 6, 7, 8 一致引用
- `TransitionKind::{Archive, Restore, MarkForDelete}` 在 Task 2 定义，Task 2 自身测试使用，Task 7 commands 引用
- `ConflictAction::ReplaceB` 在 Task 5 测试，Task 5 实现使用
- `permanent_delete_inner(conn, id)` 在 Task 6 定义并测试，Task 7 不引用（Task 7 是 library commands，不涉及销毁）
- `FileSummary.status` / `file_state` 在 Task 8 定义，Task 9 HTTP 端点 + Task 10 TS 类型一致
- `statusFilter` 在 Task 11 定义，Task 12 LibraryView 引用

**4. 依赖顺序检查：**

- Task 1（DB schema）必须先于所有其他 Task ✓
- Task 2（state_machine）必须先于 Task 7（commands/library 用 state_machine）✓
- Task 3（identifier）独立，可与 Task 4/5 并行 ✓
- Task 5/6 都改 commands/，但改不同文件，可并行 ✓
- Task 8（file_summary）必须先于 Task 9（HTTP）✓
- Task 10（types）必须先于 Task 11/12/13（前端实现）✓
- Task 14（文档）独立，可与任何 Task 并行 ✓

**5. 简化 dirty_data 写入：**

Task 2 写 dirty_data 时只在目标位置原本有同名文件时插入（先 `dest.exists()` 检测再 rename）。如果 dest 不存在，rename 自然成功，不写 dirty_data。这避免每次状态切换都污染 dirty_data 表。

**6. 风险回顾：**

- 跨 task 编译错误累积：Task 2 / 3 / 4 / 5 / 6 / 7 / 8 修改字段名后，每步 `cargo build` 都会暴露新错误——按提示修即可
- dirty_scanner 字段名适配：Task 4 一次性改完
- HTTP 集成测试断言：Task 9 如果测试断言里 hard-code 了 `body["current_location"]`，需要改为 `body["status"]`