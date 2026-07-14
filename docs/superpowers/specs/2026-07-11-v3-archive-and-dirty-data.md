# V3 Spec — 归档目录 + 脏数据扫描 + webp 封面

> 日期：2026-07-11
> 状态：draft（待用户 review）
> 范围：**V3 MVP**——①目录与数据模型重构、②webp 封面、③脏数据扫描
> **V3.1 推迟**：LRU 预览缓存、相册式详情页 UI

## 目标

V2 的核心循环（识别入库 / 标记删除 / 回收站清理）已经稳定。V3 解决三个问题：

1. **归档目录缺失**——用户希望把"已认可"的同人志从 `identified/` 移走，给收藏盘腾空间；归档目录里的文件用户可手动拿走
2. **脏数据不可见**——DB 行 / 文件脱钩时（用户手动挪走、崩溃、外部工具改动）系统不知道，需要一个扫描 + 展示机制
3. **封面体积**——V2 用 jpg 存封面，~50KB/张；改 webp 可降到 ~20KB，节省一半

## 非目标（V3.1 推迟）

- **LRU 预览缓存**（`resources/_preview_cache/`）——V3 不实现，目录预留
- **相册式详情页**（用户主动触发预览、上下张切换）——V3 维持 V2 的"识别时自动解压 + 详情页全量展示"
- **预览空间预算**（2GB 上限、启动检测剩余空间）——推迟到 V3.1

## 核心模型

### 4 状态机

`doujinshi_file.current_location ∈ {"inbox", "identified", "will_delete", "archived"}`，互斥。

合法转移：

```
inbox → identified       (识别入库)
identified → will_delete (删除)
identified → archived    (归档)
will_delete → identified (从回收站取回)
archived → identified    (从归档取回)
```

非法转移由后端拒绝（如 `archived → will_delete`）。

> **文件缺失时的行为（用户主动转移）**：源文件不在盘上时转移**直接失败返 Err**，绝不静默改 `current_location`。`physically_deleted` 列仍由 `dirty_scanner` 启动扫描维护，不由转移路径写。这条规则专管用户主动点按钮的转移（archive / restore / mark_for_delete），后台自动流程（scanner / dirty_scanner）的 best-effort 行为见 §启动脏数据扫描。
>
> **目标位置已有同名文件**：同样直接失败返 Err。典型场景是用户把文件手动塞进 will_delete / archived 目录后，再点"取回" / "归档"。不动 DB、不动盘上任何一份（不覆盖、不删）；用户自己处理（删多出来的 / 改名）后再点。和 `inbox` 入库撞名走的 `conflict` 表不是一回事——状态转移不在那张表上挂记录，V3 范围内就是单步拒绝，V3.1+ 再议。

### 数据永生

`doujinshi_file` 行**永不 DELETE**。`physically_deleted=true` 仅是标记，API 仍可查询（浏览器扩展据此避免推荐重复下载已删同人志）。

### 文件 = 影子

DB 行是真相，文件是影子。状态转移 = `UPDATE current_location` + best-effort 文件移动（src 不存在时 no-op）。文件存在性由 `has_physical_file` 列表示，**仅由启动扫描线程更新**，状态转移不维护。

## 目录布局

```
resources/
├── doujinshi/                ← inbox（待入库）
├── doujinshi-identified/     ← identified（已入库）
├── doujinshi-will-delete/    ← will_delete（回收站）
├── doujinshi-archived/       ← archived（归档，可被用户手动清空）
├── covers/                   ← webp 封面（替代 jpg）
├── _preview_cache/           ← V3.1 预留（V3 不写入）
└── data.db
```

`config.rs` 新增 `archived_dir()` / `preview_cache_dir()`；`ensure_dirs()` 加这俩。

## 数据表

### `doujinshi_file`（改）

加一列：

```sql
ALTER TABLE doujinshi_file ADD COLUMN has_physical_file INTEGER NOT NULL DEFAULT 1;
```

迁移策略：V3 上线**清空 data.db 重建**（见迁移章节），不走 ALTER。

### `dirty_data`（新表）

```sql
CREATE TABLE IF NOT EXISTS dirty_data (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    file_path TEXT NOT NULL UNIQUE,
    file_size INTEGER NOT NULL,
    detected_dir TEXT NOT NULL,    -- 'identified' | 'will_delete' | 'archived'
    reason TEXT NOT NULL,          -- 'orphan_file' | 'db_missing_match'
    first_seen_at TEXT NOT NULL
);
```

- `reason='orphan_file'`：目录有文件但 DB 无匹配（inbox 除外——inbox 文件本就没入库）
- `reason='db_missing_match'`：当前 V3 不写此 reason；保留为未来扩展

### SeaORM 实体

`db/entities/dirty_data.rs` 新建；`entities/mod.rs` 注册。

## 状态转移 + 文件缺失语义

| 用户操作 | DB 更新 | 文件操作 | 失败行为 |
|---|---|---|---|
| 归档（identified → archived） | 成功时 `UPDATE current_location='archived'` + `physically_deleted=false` | `rename(src→archived_dir/)` | src 不在盘上 / 目标位置已有同名 → 返 Err，DB 不动；HTTP 409 + 可读 body；前端 toast 报错 |
| 删除（identified → will_delete） | 同上结构 | 同上 | 同上 |
| 取回（will_delete/archived → identified） | 同上结构 | `rename(src→identified_dir/)` | 同上 |

`has_physical_file` 不在状态转移路径里写——**只有启动扫描线程**更新它。`physically_deleted` 仅在转移**成功**时写 `false`；失败 / 缺失时不写。

### 行复活

同 hash 文件重新入库时（`identifier::identify_file` 命中已有 `hash`）：

1. 把源文件从 `inbox_dir/` 移到 `identified_dir/{filename}`（**V2 是源文件不动仅更新 current_path；V3 必须移动**——保证状态机不变量：current_location='identified' ⇒ 文件在 identified/ 下）
2. 写一行 `filename_alias`（沿用 V2 逻辑）
3. 更新行：

```sql
UPDATE doujinshi_file
SET filename=?, current_path=?, current_location='identified',
    physically_deleted=0, updated_at=?
WHERE id=? AND hash=?;
```

启动扫描看到新文件 → `has_physical_file=true`。

## 识别流程

沿用 V2（含 V2 #7 RAR 子系统）：
- `scanner::scan_inbox_once` 监控 `inbox/` → `identifier::identify_file` 处理每个文件
- hash 命中 → 复用行（见上行复活）
- name+ext 冲突 → 写 `conflict` 表，用户走 `ConflictView` 决策
- 抽封面改 webp（见下）

唯一变化：`extract_and_save` 输出 `.webp` 而非 `.jpg`。

## webp 封面

### 后端

`cover::extract_and_save(data, dest_dir, hash)`：

- 输出路径：`{dest_dir}/{hash}.webp`
- 编码：`image::codecs::webp::WebPEncoder::new`，quality 控制在让输出 ≤100KB（同 V2 预算）
- 失败 fallback：如果 webp 编码失败，记录 warning 并**写入 `.jpg` 占位**，UI 显示空封面（避免阻塞识别）

### HTTP API

- `GET /api/doujinshi/:id/cover` 返回 webp（`Content-Type: image/webp`）
- `FileSummary.cover_url` 后缀从 `.jpg` 改 `.webp`
- 浏览器扩展（未来）：同步读 webp

### 数据库列

`doujinshi_file.cover_path` 存 `covers/{hash}.webp`（V2 是 `.jpg`）。

## 启动脏数据扫描

### 触发时机

`lib.rs::run` 启动后异步触发一次扫描（不阻塞 UI 初始化）。

### 扫描流程

1. 遍历 `identified_dir/` / `will_delete_dir/` / `archived_dir/` 三个目录
2. 对每个文件 `{path}`：
   - 计算 `{basename, size, dir}` 三元组
   - 在 `dirty_data` 查 `{file_path}` 是否已存在 → 跳过
   - 在 `doujinshi_file` 查 `current_path={path}` 是否有匹配行
     - 有 → 行 `has_physical_file=true`
     - 无 → 插入 `dirty_data` 行
3. 遍历 `doujinshi_file` 表所有 `current_location IN ('identified','will_delete','archived')` 的行：
   - 检查 `current_path` 是否在对应目录中存在
   - 不存在 → 行 `has_physical_file=false`
4. 扫描完成 emit `"scan-complete"` 事件，UI 刷新

### 性能

- 单次扫描遍历文件数 ≪ 10k（个人库），用 `walkdir` 同步遍历 + 单 SeaORM 连接
- 预期耗时 < 1s；后台线程跑，不阻塞 UI

### 错误处理

- 单个文件 stat 失败（如权限）→ 跳过 + 记录 warning，不中断扫描
- DB 写失败 → 重试 3 次，仍失败则记到 tracing

## 前端改动

### LibraryView

- **卡片操作按钮**：
  - 已入库 + 有文件：显示 `归档` `删除`
  - 已入库 + 无文件：显示 `取回` `删除`（disabled 提示"文件丢失"；"取回"对无文件行 no-op，仅 DB 更新）
  - 回收站：`取回` `彻底清理`（二次确认；底层调 V2 的 `permanent_delete` 命令）
  - 归档：`取回`（对无文件行 no-op）；`删除` 显示"文件丢失，无法删除"的提示
- **顶部筛选下拉**：增加 `current_location` 筛选项（全部 / 已入库 / 回收站 / 归档）；与 V2 的 `status` 筛选（已看/未看）**叠加**（两个维度的 AND 关系）
- **移除** `marked_for_delete` 相关 chip 显示（V2 的"已标记删除"逻辑被 current_location 替代）

### 新页 `/dirty`

- 路由：`src/router.ts` 加 `dirty` 路由
- 视图：`src/views/DirtyView.vue`（新建）
- 列表展示 `dirty_data` 行：file_path + size + detected_dir + reason + first_seen_at
- 只读，V3 不提供"清掉" / "重新识别" 等操作（V3.1 加）

### DetailView

- **不变**——V2 全量解压预览逻辑保留，V3.1 改相册式
- 仅展示字段调整：`marked_for_delete` chip 移除，改为根据 `current_location` 显示不同 tag

### Stores

- 新增 `useDirtyStore`：`dirty[]` + `load()` + 监听 `scan-complete` 事件刷新
- `useLibraryStore`：增加 `currentLocationFilter` + 按 location 筛选

## HTTP API

### 现有

- `GET /api/doujinshi/:id`：返回字段加 `current_location`、`has_physical_file`；移除 `marked_for_delete`
- `PATCH /api/doujinshi/:id`：不变
- `POST /api/doujinshi/:id/mark-viewed`：不变

### 新增

- `POST /api/doujinshi/:id/archive`：identified → archived
- `POST /api/doujinshi/:id/restore`：will_delete 或 archived → identified
- `GET /api/dirty`：列出 `dirty_data` 表内容

### 删除

- `POST /api/doujinshi/:id/mark-for-delete`（V2 端点）：保留语义但改实现——不再设 `marked_for_delete=true`，改 `UPDATE current_location='will_delete'` + 移文件。**保留同名端点**以便不破坏未来浏览器扩展兼容性。

## Tauri Commands

### 改

- `mark_for_delete` → 改为"移到 will_delete"
- `unmark_for_delete` → 改为"从 will_delete 取回到 identified"

### 新增

- `archive(id: i64)`：移到 archived
- `restore(id: i64)`：从 will_delete / archived 取回到 identified
- `list_dirty()`：列出 dirty_data 表

## 迁移策略（V3 上线一次性）

V3 上线启动时**完全清空**：

```
resources/doujinshi/                ← 清空（用户重新拷贝压缩包到这里）
resources/doujinshi-identified/     ← 清空
resources/doujinshi-will-delete/    ← 清空
resources/doujinshi-archived/       ← 新建空目录
resources/covers/                   ← 清空（重新生成 webp）
resources/_preview_cache/           ← 新建空目录
resources/data.db                   ← 删表重建
```

执行：在 `lib.rs::run` 检测 `app_setting.schema_version`，V3 之前不存在 `v3` 标记 → 走 clear + init_v3_schema。**这一步前用户必须备份压缩包**（README 标注）。

迁移步骤由用户手动执行：
1. 关闭 V2
2. 备份 `resources/doujinshi-identified/`（或其他需要保留的）到外部位置
3. 启动 V3 → 自动清空 → 重新拷贝压缩包到 `resources/doujinshi/`
4. V3 启动扫描识别入库

## 测试

### 单元测试

- `cover::extract_and_save` 输出 webp（断言文件 magic bytes = `RIFF....WEBP`）
- `cover::extract_and_save` 输出大小 ≤ 100KB
- `identifier` 状态转移：合法转移成功、非法转移失败、文件缺失时 DB 转移仍然成功
- `state_machine` 转移护栏：源文件缺失 → DB 不动；目标位置已有同名 → DB 不动、盘上两份文件都不动

### 集成测试

- 启动扫描：构造 5 个文件 + 3 个 DB 行（含 1 个孤儿、1 个 DB 行无文件）→ 断言 dirty_data 有 1 行 + 4 个行 has_physical_file 正确
- 状态转移 API：archive/restore/mark_for_delete 端到端测试
- HTTP API：archive / restore / list_dirty 端点

### 手动 E2E

- 装 V3 → 启动 → 验证 covers/ 生成 webp
- 拖一个 zip → 识别 → 验证 identified/ 有文件 + covers/ 有 webp
- LibraryView 点"归档" → 验证文件移到 archived/ + 行 current_location=archived
- LibraryView 点"删除" → 验证文件移到 will_delete/ + 行 current_location=will_delete
- 回收站点"取回" → 验证文件移回 identified/
- 把归档目录文件手动拿走 → 重启 → 验证 has_physical_file=false + dirty 页无新行（因为 DB 行匹配 current_path，孤儿检测不报）
- 归档 → 在资源管理器删 archived 目录的文件 → **不重启直接点"取回"** → 验证：前端 toast 报"文件已丢失"类错误，DB 行 current_location 仍为 `archived`，未被静默改写为 `identified`
- 拖同 hash 文件重新入库 → 验证行复用（filename 更新、physically_deleted=false）

## 风险

- **状态转移并发**：扫描线程更新 `has_physical_file` 时，如果用户正在转移状态，可能短暂不一致。V3 单用户本地应用，可忽略。
- **archive 命令失败**：rename 跨设备（V2 已知问题）→ copy + remove fallback。
- **`dirty_data` 表膨胀**：仅在脏数据出现时增长，预期很慢；不提供清理入口（V3.1 加）。
- **`physically_deleted` 语义扩展**：从"清空回收站"扩展为"文件不存在"（仅由 `dirty_scanner` 维护），转移路径不再写它。

## 待澄清（V3 落地前）

无——核心问题已对齐。