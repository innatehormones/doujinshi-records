# Spec — 脏数据页「重新入库」按钮（V4 增量）

> 日期：2026-07-16
> 状态：implemented
> 范围：**脏数据页加一个可操作的入口**——对 `reason="orphan_file"` 条目加「重新入库」按钮，让用户把孤儿文件交给 scanner 重新入库
> 前序：V4（2026-07-15，数据与文件解耦，引入 `dirty_data` 表）+ V3 spec「V3.1 加「重新识别」操作」（V3 时代预留的扩展点）

## 背景

V4 spec 把"数据是真理、文件只是附庸"作为核心原则——`dirty_scanner` 启动时**只观测、不擅自改动**：发现不一致 → 改 `file_state` + 写 `dirty_data` 行，**不动 status**。这是为了避免系统自动恢复"用户特意造成的差异"（如临时把 archived 里的文件挪出来扫描一遍封面）。

V4 spec 的「待澄清」段也明确表示：用户遇到孤儿文件时**没有操作按钮**——"留作未来扩展"。

实际用户场景显示这个限制太严：

- 备份还原后，备份前的几条 doujinshi 行被回滚掉，对应文件留在 `identified/`。`dirty_scanner` 看到文件但 DB 没行 → 写 `dirty_data(reason='orphan_file')`
- 这些孤儿文件**本来是合法数据**（甚至已经被 scanner 跑过一次），但没有 UI 入口能"再走一遍入库流程把它们救回来"
- 现状：用户只能手动 `mv` 到 inbox 重启 app / 等下次启动（启动 scanner 不会自动跑 inbox 之外的目录）

需要：让脏数据页**对 orphan_file 提供一个操作按钮**——"重新入库"，让用户能救回这些孤儿。

V3 spec 已在「V3.1 加「重新识别」操作」段预留了扩展点（"DirtyView 只读，V3.1 加清掉 / 重新识别"）。本 spec 落地该扩展点。

## 目标

在脏数据页对 `reason="orphan_file"` 条目加「重新入库」按钮：

- UI 立即响应（不阻塞）
- 文件被搬到 `inbox/`，由已经在跑的 `scanner::Scanner` notify watcher 接管入库流程
- dirty_data 行立即软删（成功失败都是 dirty_data 行消失——失败由现有 UI 兜底）

## 决策汇总

| 决策点 | 选择 |
|---|---|
| 按钮可见性 | `reason === "orphan_file"` 才显示——其他 reason（`db_row_file_missing` / `location_path_mismatch*` / `overwritten_by_state_switch`）没有合理统一操作 |
| 触发方式 | `<n-popconfirm>` 二次确认，避免误点 |
| **后端实现** | **Mover-only**：`fs::rename` 到 `inbox/` + 写 `resolved_at`，**不**调 `identifier::identify_file` |
| 完整入库 | 由 `scanner::Scanner` 接管（notify watcher + 2s 防抖 → `scan_inbox_once` → `identifier::identify_file`） |
| 失败兜底 | 文件不存在 / inbox 已有同名 → 弹 error + dirty 行不动（前置检查）。scanner 跑失败（rar / 撞名）由 `rar-error` 事件 + `ConflictView` 兜底 |
| 软删机制 | `dirty_data` 加 `resolved_at`（nullable TEXT/RFC3339），**不** DELETE——历史 dirty row 仍可追溯，且 `scan_dir` dedup check 用 `ResolvedAt.is_null()` 过滤后不会被已 resolve 行阻挡 |
| 标签修正 | `detected_dir="identified"` 的 `dirLabel` 从「已入库」（歧义）改为「入库目录」，与「回收站 / 归档」风格一致 |
| 卡片角标 | orphan_file 卡片额外加 `<n-tag type="warning">孤儿</n-tag>` 让 reason 语义醒目 |

## 关键决策：为什么是 mover-only

v1 草案让 `reingest_dirty_entry_inner` 调 `identifier::identify_file` 走完整入库流程：hash → 抽封面 → 写 DB → 搬文件。**全部在 UI 点击的同一次 invoke 里同步完成**。

问题：大文件（GB 级）hash 几秒到几十秒，rar 抽封面更久。UI 看起来就"卡住"——用户报告"按了没反应"。

最终方案：**只 `fs::rename` 到 inbox/，由 scanner 接管入库**。

| 维度 | v1（同步入库） | v2（mover-only） |
|---|---|---|
| UI 响应 | 大文件明显卡 | ms 级返回 |
| 复用 pipeline | 否 | 完全复用 `scanner::Scanner` 既有并发 / 防抖 / 进度事件 |
| 撞名处理 | 同 | 同（`ConflictView` 兜底） |
| rar 失败 | 同 | 同（`rar-error` 事件兜底） |
| dirty_data resolve 时机 | 同步入库成功 | 同步 mv 成功 |
| 失败时 dirty_data | 保持 unresolved | 保持 unresolved（前置检查失败）或写 `resolved_at`（mv 成功但 scanner 跑失败——**这里有取舍**，见下） |

**取舍**：scanner 跑失败时，dirty_data 行已 resolve（我们 mv 完就 resolve 了），UI 看不到"重新入库失败"的反馈。

- 接受：rar / 撞名两种最常见失败都有 UI 兜底（`rar-error` 事件 → Inbox 卡片 / `ConflictView`）
- 不接受：extraction failed / 其他静默失败——但这些是已知噪声，rar 之外的格式（zip）走 fast path 几乎不会失败；rar 失败要么走 forceExtract retry，要么系统已经 emit 了 rar-error 卡片
- 替代方案（hold resolve 等 scanner 完成）需要 scanner 知道 dirty_data 行存在 + outcome 反馈，复杂度高很多

## 改动清单

### 1. Schema 迁移 v10 (`db/migrations.rs`)

`CURRENT_VERSION` 9 → 10。`MIGRATIONS` 数组 append 一条：

```rust
(
    10,
    "add dirty_data.resolved_at for soft-resolve",
    "ALTER TABLE dirty_data ADD COLUMN resolved_at TEXT",
),
```

`ALTER TABLE ADD COLUMN` 由 `apply_migration` 走 `pragma_table_info` 幂等检查。

**附带修复**：`apply_migration` 的 ALTER TABLE 路径如果发现表不存在就跳过——让"人工停在 v5 之前的测试"也能继续往下跑 v10。`SELECT name FROM sqlite_master WHERE type='table' AND name='<table>'` 检查；空就 Ok 返回。

### 2. dirty_data Model (`db/entities/dirty_data.rs`)

加 `pub resolved_at: Option<String>` 字段。SeaORM 按 `Option` 推 nullable，不需要注解。

### 3. dirty_scanner (`services/dirty_scanner.rs`)

`scan_dir` 的「已存在 dirty_data 行就 skip」必须忽略已 resolved 行，否则 resolved 行会阻挡后续相同 file_path 的 orphan 写入：

```rust
let exists = dirty_data::Entity::find()
    .filter(dirty_data::Column::FilePath.eq(&path))
    .filter(dirty_data::Column::ResolvedAt.is_null())
    .one(conn)
    .await?;
```

### 4. `commands::dirty::reingest_dirty_entry_inner`

Mover-only 实现：

```rust
pub async fn reingest_dirty_entry_inner(
    conn: &sea_orm::DatabaseConnection,
    inbox_dir: &std::path::Path,
    id: i64,
) -> AppResult<()> {
    let row = dirty_data::Entity::find_by_id(id)
        .one(conn)
        .await?
        .ok_or_else(|| AppError::Other(format!("dirty_data id={id} not found")))?;

    if row.reason != "orphan_file" {
        return Err(AppError::Other(format!(
            "only orphan_file entries can be reingested, got reason={}",
            row.reason
        )));
    }
    let path = PathBuf::from(&row.file_path);
    if !path.exists() {
        return Err(AppError::Other(format!(
            "file no longer on disk: {}",
            row.file_path
        )));
    }

    std::fs::create_dir_all(inbox_dir)?;
    let file_name = path.file_name().ok_or_else(|| {
        AppError::Other(format!("invalid file path: {}", row.file_path))
    })?;
    let target = inbox_dir.join(file_name);
    if target.exists() {
        return Err(AppError::Other(format!(
            "inbox already has a file with the same name: {}",
            target.display()
        )));
    }
    if path != target {
        std::fs::rename(&path, &target)?;
    }

    let mut am: dirty_data::ActiveModel = row.into();
    am.resolved_at = Set(Some(chrono::Utc::now().to_rfc3339()));
    am.update(conn).await?;
    Ok(())
}
```

**关键点**：
- 删 `covers_dir` / `identified_dir` / `skip_size_gate` 三个参数——mover-only 不需要
- 删 `use crate::services::identifier;`
- 前置检查：reason != orphan_file 拒绝 / 文件不存在拒绝 / inbox 同名拒绝——**任一失败 dirty_data 行都不动**

### 5. tauri wrapper (`commands::dirty.rs`)

```rust
#[tauri::command]
pub async fn reingest_dirty_entry(
    state: State<'_, AppState>,
    id: i64,
) -> AppResult<()> {
    reingest_dirty_entry_inner(&state.conn, &state.config.inbox_dir(), id).await
}
```

注册到 `lib.rs::run` 的 `invoke_handler` 列表。

### 6. list_dirty 过滤 resolved (`commands/dirty.rs`)

`list_dirty` 加 `.filter(ResolvedAt.is_null())`，count + items 同步。否则已 resolve 行还会再出现在 list，污染 UI。

### 7. 前端

- `types/api.ts::DirtyEntry` 加 `resolved_at: string | null`
- `api/tauri.ts::reingestDirtyEntry` 签名：`(id: number) => invoke<void>("reingest_dirty_entry", { id })`
- `stores/index.ts::useDirtyStore` 加 `reingest(id)` action：调 API + `load()` 刷新
- `views/DirtyView.vue`：
  - `dirLabel('identified')` 从「已入库」改为「入库目录」
  - 描述文案同步
  - orphan_file 卡片加 `<n-tag type="warning">孤儿</n-tag>` 角标
  - 卡片右侧加 `<n-popconfirm @positive-click="onReingest(e.id)">` 按钮
  - popconfirm 文案：「重新入库会把文件搬到入库目录让 scanner 自动入库。是否继续？」

## 关键文件清单

| 路径 | 改动 |
|---|---|
| `src-tauri/src/db/migrations.rs` | v10 + apply_migration 表存在性 guard |
| `src-tauri/src/db/entities/dirty_data.rs` | `Model.resolved_at: Option<String>` |
| `src-tauri/src/services/dirty_scanner.rs` | scan_dir 增 `ResolvedAt.is_null()` 过滤 |
| `src-tauri/src/commands/dirty.rs` | list_dirty 过滤 resolved；新增 mover-only reingest_dirty_entry + inner |
| `src-tauri/src/lib.rs` | invoke_handler 注册 |
| `src/api/tauri.ts` | `reingestDirtyEntry(id)` |
| `src/stores/index.ts` | `useDirtyStore.reingest` action |
| `src/views/DirtyView.vue` | 标签 + 角标 + 按钮 + popconfirm |
| `src/types/api.ts` | `DirtyEntry.resolved_at` |
| `src-tauri/tests/dirty_reingest.rs` | 4 个 case：mover / inbox 同名拒绝 / 非 orphan 拒绝 / 文件缺失拒绝 |

## 复用现有代码

- `scanner::Scanner::scan_inbox_once` (`services/scanner.rs:77`) — notify watcher 触发后跑 `identify_file` 完整流程，2s 防抖；rar-error / conflict 错误路径已就绪
- `config::AppConfig::inbox_dir()` (`config.rs:23`) — `resources/doujinshi/` 路径
- `apply_migration` (`migrations.rs:270`) — ALTER TABLE ADD COLUMN 幂等
- `useDirtyStore` 的 `load + filter` pattern (`stores/index.ts:315`)

## 验证

### 后端测试 (`tests/dirty_reingest.rs`)

- **happy path**：构造 DB + 写一个 orphan zip + 调 `reingest_dirty_entry_inner` → 断言文件搬到 inbox/ + dirty_data.resolved_at 已写 + DB 没新 doujinshi 行（mover-only 不入库）
- **inbox 同名冲突**：pre-create `inbox/dup.zip` → 调 reingest → 断言 error + dirty_data.resolved_at 未写 + 原文件未搬
- **非 orphan reason**：写一条 `db_row_file_missing` 行 → 调 reingest → 断言 error "only orphan_file" + dirty_data.resolved_at 未写
- **文件缺失**：写 dirty_data 指向不存在的路径 → 调 reingest → 断言 error "no longer on disk" + dirty_data.resolved_at 未写

### 端到端

```bash
cd src-tauri && cargo test --test dirty_reingest
pnpm tauri dev
```

1. 制造 orphan：备份前入库 1 文件 → 还原备份 → 重启，脏数据页出现 1 条「孤儿 / 入库目录 / X.zip」
2. 点「重新入库」→ popconfirm 二次确认 → **立即**卡片消失
3. ~2-3s 后右下角浮窗显示 scanner-status（is_scanning: true → false）
4. Library 页面多 1 张新卡
5. 大文件（GB 级 rar）验证：制造孤儿 → 点「重新入库」→ 立即消失 → scanner-status 进度浮窗 → 完成后 library 出现新卡
6. 撞名验证：让 orphan 跟已有行 (filename, ext) 撞 → mv 到 inbox 后 scanner 写 conflict → ConflictView 解决
7. 已 resolve 行不阻挡新 orphan：手动 UPDATE resolved_at = now() 模拟「重新入库」成功的行 → 把文件从 identified 移走再放回 → 启动 dirty_scanner → 应写新 dirty_data 行（不 skip）

## 风险

- **scanner 静默失败**：rar 抽封面失败 / extraction 失败等有 `rar-error` 事件兜底，但其他静默失败（系统 IO 错、磁盘满）dirty_data 行已 resolve，用户无感。**接受**：rar 之外格式几乎不失败；rar 失败有 forceExtract 路径。
- **inbox 临时堆积**：用户连续点 10 个「重新入库」会 10 个文件同时出现在 inbox，scanner 2s 防抖后批量处理。`scan_inbox_once` 内部走 `scan_guard` 串行化，DB 写入互斥。
- **跨设备 rename**：`fs::rename` 跨设备会失败（Windows `ERROR_NOT_SAME_DEVICE=17`）。`identified/` 和 `inbox/` 都在 `resources/` 下，跨设备概率极低——但理论上 `resources/` 可以是网络盘。**不解决**：项目当前部署全是本地盘。

## 不在范围内

- `db_row_file_missing` / `location_path_mismatch*` / `overwritten_by_state_switch` 不加按钮——没有合理的统一操作
- dirty_data 等 scanner 完成才 resolve——mover-only 模式下 scanner 不知道 dirty_data 的存在，强制绑定会让逻辑复杂得多
- `skip_size_gate` 透传——mover-only 不涉及，rar 大文件由 scanner 自己的 `forceExtract` 路径（Inbox 卡片 retry 按钮）兜底

## 历史决策锚点

- V4「只观测、不擅自改动」原则 + 「无操作按钮」决定在本 spec 后部分放开：只对 `orphan_file` 允许用户主动重跑入库流程
- `dirty_data` 软删除（UPDATE resolved_at）替代 DELETE 落实：保留历史 dirty row 可追溯 + 让 `scan_dir` 的 dedup check 用 `ResolvedAt.is_null()` 过滤避免阻挡后续 orphan 写入
- V3 spec「V3.1 加「重新识别」操作」预留扩展点 — 本 spec 落地该扩展点
