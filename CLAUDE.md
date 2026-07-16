# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## 项目说明

**同人志档案**（doujinshi-records）：本地 Tauri 2 桌面应用，管理个人同人志库。

- 监控 `resources/doujinshi/` 入库新压缩包，BLAKE3 哈希去重 + 提取 ≤100 KB webp 封面。
- V4 双字段模型：业务 `status ∈ {in_library, archived, recycle, deleted}`（4 值，任意可切，DB 行只增不改即"数据永生"）+ 文件 `file_state ∈ {present, missing, absent_confirmed}`（3 态，扫描 + 销毁操作维护）。
- 启动时扫 3 个数据目录，把"DB 无对应行"的孤儿写入 `dirty_data`；把"DB 有行但磁盘丢失"的行打 `file_state='missing'`，由"脏数据"页提示用户。
- 暴露本地 HTTP API（`127.0.0.1`，Bearer token 鉴权）供浏览器扩展查询。
- 仅管理本地文件，**不下载、不分发**内容。

## 核心概念（必读）

这一节先讲清 5 件事——**同人志数据是什么、压缩文件是什么、它们的关系、状态机、操作**——任何 AI 接手前请先读懂。

### 同人志数据（doujinshi data）

- **本质**：DB 里 `doujinshi_file` 表的一行。**同人志记录永远只增不减**（"数据永生"）。
- **组成**：
  - **元数据**（人为 / 解析得到）：`title / circle / series / translator / version / note / rating`、原文件名、`hash`（BLAKE3 源文件指纹）、`size_bytes`
  - **业务状态 `status`**：4 值（见下），**完全由用户决定**
  - **文件状态 `file_state`**：3 态（见下），**由扫描 / 销毁操作维护**
  - **`last_seen_path`**：最后一次确认文件存在时的路径；缺失时保留历史值
  - **`viewed`**：已读标记
  - **`has_open_conflict`**：是否有未解决的 `conflict` 表记录（前端禁用状态切换 + 后端 guard 兜底）
- **派生数据**：从压缩包内容抽取出来、不在 DB 主表行里——封面（`resources/covers/<hash>.pwb`）、预览缩略图（`resources/_preview_cache/<file_id>-<idx>.webp`）。删主表行不会删派生数据，反之亦然。

### 压缩文件（archive file）

- **本质**：物理存在的 `.zip` / `.rar` 文件，位于 `resources/doujinshi*/` 之一（顶层、不递归子目录）。
- **性质**：文件**不是同人志数据的必备附属**——一条 doujinshi 行可能没有对应文件（文件被用户拿走、被外部工具删了、被"销毁"操作删了），反之目录里的孤儿文件也可能在 DB 里找不到对应行（外部塞入、未识别）。
- **目录与 status 的对应（仅是惯例，不是约束）**：
  - `resources/doujinshi/` — inbox：等待入库识别
  - `resources/doujinshi-identified/` — 通常 `status='in_library'` 时搬到这里
  - `resources/doujinshi-will-delete/` — 通常 `status='recycle'` 时搬到这里
  - `resources/doujinshi-archived/` — 通常 `status='archived'` 时搬到这里
  - `resources/covers/` — 封面派生数据
  - `resources/_preview_cache/` — 缩略图 LRU
- **跨设备 rename 兜底**：`std::fs::rename` 跨设备（`CrossesDevices` / Windows `ERROR_NOT_SAME_DEVICE=17`）自动 `copy + remove`。
- **不做"重新打包压缩"**：本系统只移动 / 抽取现有压缩文件，**不**生成 / 修改压缩文件本身。

### 数据 ↔ 文件 关系

- **数据是真实来源**（truth source），文件只是"可能存在的附庸"。
- 状态机迁移时：文件存在 → 尽力搬运 / 删除；文件不存在 → 文件操作 no-op，但**仍允许 status 更新**。
- 启动时 `dirty_scanner` 一次性观测：发现不一致 → 改 `file_state` + 写 `dirty_data`，**不动 status**。
- 文件销毁（用户操作）：复合操作——同时设 `status='deleted'` + `file_state='absent_confirmed'` + best-effort `remove_file`。
- `status='deleted'` ≠ 文件一定不在盘上。`status='deleted'` 只是用户的"业务结论"；`file_state` 才是文件此刻是否在盘上的事实。

### 状态机（status + file_state 双轴）

| 字段 | 取值 | 由谁维护 | 谁可改 |
|---|---|---|---|
| `status` | `in_library` / `archived` / `recycle` / `deleted` | 用户 | **任意来源的任意 status 可切到任意 status**（V4 移除"非法转移"） |
| `file_state` | `present` / `missing` / `absent_confirmed` | 扫描器 + 销毁操作 | 启动 `dirty_scanner` / `permanent_delete_inner` / `reactivate_row` / 状态转移搬运成功路径 |

- **合法转移规则**：任意 `status` → 任意 `status`；`file_state` 自动跟随（搬运成功 → present；搬运 no-op → missing）。
- **状态转移入口**：`services::state_machine::transition_with_dirs(conn, id, kind, identified_dir, will_delete_dir, archived_dir)` 三目录必传（用于决定文件搬去哪）。
- **"销毁"是复合操作**（不走 state_machine）：
  1. `status='deleted'`
  2. `file_state='absent_confirmed'`
  3. best-effort `remove_file(last_seen_path)`
  4. `preview_cache.invalidate(id)`（调用方负责）
- **目标目录同名 = 视为孤儿**：自动覆盖 + 写 `dirty_data(reason='overwritten_by_state_switch')`（不要拒绝转移）。
- **冲突解决 ReplaceB**：旧 A 行同样推到 `status='deleted' + file_state='absent_confirmed'`，让 B 入库占用同名（不再有"permanently_deleted 终态"概念）。

### 操作（用户能做什么 / 系统自动做什么）

**用户主动操作**：

| 操作 | 触发 | 数据影响 | 文件影响 |
|---|---|---|---|
| 入库识别 | 拖文件到 `resources/doujinshi/` | 新增 doujinshi 行（`status='in_library'`） | 从 inbox/ 搬到 identified/ |
| 归档 | Library 卡片点「归档」 | `status: 任意 → archived` | 搬到 archived/（若存在） |
| 移到回收站 | Library 卡片点「回收」 | `status: 任意 → recycle` | 搬到 will_delete/（若存在） |
| 销毁 | RecycleBin 卡片点「删除」 | `status='deleted' + file_state='absent_confirmed'` | best-effort 删源文件 + 失效 LRU 缩略图 |
| 取回 / 恢复 | RecycleBin 卡片点「还原」/「恢复」 | `status: recycle/archived/deleted → in_library` | 搬到 identified/（若存在）；`file_state` 仍可能 missing |
| 元数据编辑 | DetailView 表单保存 | PATCH 字段（`MetadataPatch`） | 无 |
| 冲突解决 | ConflictView 选 4 选 1 | 标 resolved / 推 A 到 deleted / B 入库 | 见 conflict 章节 |
| 重新入库 | 脏数据页 orphan_file 卡片「重新入库」 | `dirty_data.resolved_at` 软删（不动 doujinshi 表） | `fs::rename` 到 `doujinshi/`，由 `scanner::Scanner` notify watcher 接管入库（BLAKE3 / 抽封面 / 落盘） |

**系统自动行为**：

| 触发 | 行为 |
|---|---|
| 启动 | `init_schema_versioned` → 必要时跑迁移；`PreviewCache::new` → 扫盘加载；`dirty_scanner::scan` → 一次性回填 file_state + dirty_data；scanner watcher → 启动监听 |
| inbox 文件变化（防抖 2s） | `scan_inbox_once` 遍历 → identify_file（BLAKE3 算 hash / 命中 reactivate / 撞名 conflict / 抽封面 / 落盘） |
| `library-updated` 事件 | 前端并发刷新 3 个 store（library / recycle / inbox） |
| `scanner-status` 事件 | 进度条浮窗 |
| `rar-error` 事件 | rar 识别失败记录去重后展示 |

## 技术栈

- **后端**：Rust（stable，rustfmt + clippy）+ Tauri 2 + SeaORM 1.1（SQLite）+ Axum 0.7 + notify-debouncer-full 0.3 + BLAKE3 + lru 0.12 + webp 0.3
- **前端**：Vue 3 + TypeScript + Naive UI + Tailwind CSS 4（Vite 插件 + `@theme` token）+ Pinia + Vue Router
- **包管理**：pnpm 10 workspace（单包），依赖在根目录 `package.json`
- **平台**：Windows 10/11（Tauri 2 WebView2）

环境要求：Rust 1.77+（项目固定 stable）、Node 20+、pnpm 9+。

## 开发命令

所有命令在项目根目录运行。

### 日常开发

```bash
pnpm install                              # 装依赖（首次或新增依赖后）
pnpm tauri dev                            # 完整开发（Vite + Tauri + Rust）
# RUST_LOG=info,sea_orm=info pnpm tauri dev   # 临时开 SeaORM SQL 日志查慢查询
# RUST_LOG=debug pnpm tauri dev               # 临时全 debug 排查
pnpm dev                                  # 仅前端（Vite HMR，后端需独立运行）
pnpm build                                # 前端构建（vue-tsc --noEmit + Vite build）
pnpm tauri build                          # 打包发布

# Rust 类型检查
cd src-tauri && cargo check
cd src-tauri && cargo clippy --all-targets

# Rust 测试（按名字运行单个）
cd src-tauri && cargo test <name>         # 如 cargo test hashes_known_content
cd src-tauri && cargo test <module>::     # 如 cargo test services::hasher::
cd src-tauri && cargo test                # 全部

# 前端严格类型检查（与 build 同源）
pnpm exec vue-tsc --noEmit

# 清空运行时数据（开发重置）
python scripts/wipe_data.py                  # 仅删 data.db
python scripts/wipe_data.py --include-files  # 额外清 covers/ / 缓存 / 4 个文件状态目录
# 应用必须先关，否则 SQLite 文件锁住删不掉（Windows）
```

`src-tauri` 是独立 crate。测试模块在各 service 文件的 `#[cfg(test)]` 内（`hasher.rs`、`cover.rs`、`filename_parser.rs`、`state_machine.rs`、`preview_cache.rs`、`identifier.rs` 等）。HTTP 集成测试在 `src-tauri/tests/`。

## 项目结构（仅列非显然部分）

```
src-tauri/src/
├── main.rs              # tokio::main、配置加载、DB recovery probe、init_schema_versioned
├── lib.rs               # AppState 装配、HTTP 端口持久化、scanner 启动、preview_cache GC
├── config.rs            # AppConfig + 资源目录派生方法
├── error.rs             # AppError（Io/Db/NotFound/ConflictPending/Other/Anyhow）
├── db/
│   ├── migrations.rs    # MIGRATIONS 数组 + init_schema_versioned_with_covers_dir，CURRENT_VERSION=8
│   ├── recovery.rs      # 启动时按 SQLite magic 头检测 corruption，备份后重建
│   └── entities/        # SeaORM 实体（doujinshi_file / conflict / scan_event / app_setting / ...）
├── services/
│   ├── scanner.rs       # notify-debouncer-full + 2s 防抖；scan_inbox_once
│   ├── identifier.rs    # 核心流程：hash → 命中则 reactivate/忽略；否则 parse → 撞名则 conflict（排除 deleted） → 抽封面 → 落 identified
│   ├── state_machine.rs # V4 状态机：DB 优先 + 文件 best-effort；任意 status→任意 status；目标目录同名视为孤儿（dirty_data）
│   ├── dirty_scanner.rs # 启动一次扫 4 个状态目录（不扫 deleted），回填 file_state + dirty_data
│   ├── preview_cache.rs # 磁盘 + 内存 LRU；key=(file_id, image_index)；80% waterline
│   ├── archive.rs       # zip/rar 解析、pick_cover、list_image_names_sorted
│   ├── cover_format.rs  # webp 编码（≤100 KB）
│   ├── hasher.rs        # BLAKE3
│   ├── filename_parser.rs
│   ├── rar_detect.rs    # 探测 unrar/7z；2 个 tool
│   └── disk_space.rs    # preflight
├── commands/            # Tauri invoke 命令；library / inbox / recycle / dirty / settings / guards
└── http/
    ├── mod.rs           # build_router 在独立 std::thread + multi-thread tokio 启动（避免 Tauri runtime starvation）
    ├── api.rs           # 全部 HTTP handler
    ├── auth.rs          # Bearer token 中间件（exempt 列表见下）
    ├── auth_token.rs    # 32 字节 URL-safe base64 生成
    └── port_allocator.rs
```

```
src/
├── views/               # Library / Detail / Inbox / Conflict(/compare/:id) / RecycleBin / Dirty / Settings
├── stores/index.ts      # 5 个 store：useScanStatusStore / useSettingsStore / useLibraryStore / useRecycleStore / useDirtyStore / useInboxStore
├── api/
│   ├── tauri.ts         # invoke 封装（与 Tauri command 一一对应）
│   └── http.ts          # 走 Bearer token 的 fetch 封装（apiGet/Post/Patch/Put）
├── composables/
│   ├── useThumbnailPipeline.ts  # Worker 调度（并发 2，blob URL 生命周期，PUT 落盘）
│   └── usePreviewState.ts       # 全屏预览 open/index
├── components/          # FileCard / FullscreenPreview / PermanentDeleteDialog / RestoreDialog
├── workers/previewThumb.worker.ts  # OffscreenCanvas 把原图转 800px webp q=0.7
└── types/api.ts         # 前后端共享类型（FileSummary / DirtyEntry / RarError / ConflictAction / MetadataPatch ...）
```

```
resources/                # 运行时数据（git 忽略）
├── doujinshi/            # inbox：等待入库识别（顶层 .zip/.rar，不递归子目录）
├── doujinshi-identified/ # 通常对应 status='in_library' 的文件存放位置
├── doujinshi-will-delete/# 通常对应 status='recycle' 的文件存放位置（回收站）
├── doujinshi-archived/   # 通常对应 status='archived' 的文件存放位置
├── covers/               # 抽取的封面（webp V3+，旧 V1/V2 是 jpg；HTTP 按 magic bytes 探测 mime；V7+ 文件名 .pwb）
├── _preview_cache/       # LRU 缩略图：<file_id>-<image_index>.webp
└── data.db               # SQLite
```

> 目录与 status 的对应**只是惯例**：状态机搬运时按 target status 把文件搬到对应目录；但用户 / 外部工具可以从任一目录挪走文件而不通知软件——下次启动 `dirty_scanner` 会发现并打 `file_state='missing'`。

## 核心架构

### 数据流（inbox → in_library，自动识别入库）

> 这是系统**唯一**的自动状态变更——用户拖文件到 inbox，由 scanner 调 identifier 把"无名压缩文件"变成"一条新的 doujinshi 数据 + 一个搬到 identified/ 的文件"。其他 status 切换都是用户手动操作。

`scanner.rs::Scanner::scan_inbox_once` 遍历 `*.zip` / `*.rar` → `identifier::identify_file`：

1. RAR size gate（zip 不受影响；≤200 MB 静默通过，~1 GB 拒，>1 GB 拒）
2. BLAKE3 算源文件 hash
3. 命中已存在行（同 hash）：
   - 行 `status='in_library'` → 删 inbox 副本，刷 `filename_alias` + `filename`（避免 dirty_scanner 把原 identified 副本当孤儿）
   - 行 `status` ∈ {`archived`, `recycle`, `deleted`} → `reactivate_row`：把源文件 mv 回 `doujinshi-identified/`、`status → in_library`、刷 alias；`deleted` 行也能复活（V4 允许任意 status 切换）
4. 未命中：解析文件名（`filename_parser`）
5. `(filename, ext)` 撞名 → 写 `conflict` 表，留在 inbox 等待用户在 ConflictView 解决（collision check 排除 `status='deleted'`）
6. 抽封面（zip：list + pick_cover 直读；rar：调用 unrar/7z 解到 tempdir 后 list+pick）
7. 落盘到 `doujinshi-identified/`、插 `doujinshi_file` 行（`status='in_library'`、`file_state='present'`）、写 `filename_alias` + `scan_event`

`identify_file` 出错时：rar 会通过 `rar-error` 事件把 `RarErrorPayload` 推到前端展示；扫描结束发 `library-updated`（带 processed 数）和 `scanner-status`。

### 启动脏数据扫描（dirty_scanner.rs）

启动一次扫 `identified/` / `will_delete/` / `archived/` 三个目录（`inbox/` 不扫，scanner 还在处理；`deleted` 无对应目录，不扫）：

- 目录里有文件但 DB 无对应行 → 写 `dirty_data`（reason=`orphan_file`）
- DB 行的 `last_seen_path` 已在 status 期望目录下但文件丢失 → `file_state='missing'` + dirty_data（reason=`db_row_file_missing`）
- DB 行的 `last_seen_path` 不在期望目录但期望目录里有同文件名文件 → 自动修 `last_seen_path` 找回，原陈旧路径留 dirty_data 记录（reason=`location_path_mismatch_resolved`）；找不到则 dirty_data（reason=`location_path_mismatch`）
- 文件正常存在 → `file_state='present'`
- `.gitkeep` 文件跳过

### 启动 SQLite recovery（db/recovery.rs）

启动时检查 `data.db` 前 16 字节是不是 `SQLite format 3\0`。不是 → 改名 `data.db.bak-<ts>` 后由 `init_schema_versioned` 建空库。**不**先尝试打开 DB（Windows 上 OS 锁会拦 rename；刻意不重试避免误伤）。

### 版本化迁移（db/migrations.rs）

`MIGRATIONS` 数组是唯一真相，新增列或表 **必须 append 一条**，不要改旧条目。`CURRENT_VERSION=8`。

- v1 = `init_schema` bootstrap
- v2+ = `ALTER TABLE ADD COLUMN` 或 `CREATE TABLE IF NOT EXISTS`
- `apply_migration` 对 `ALTER TABLE ADD COLUMN` 会用 `pragma_table_info` 幂等检查（兼容老库已有该列的情况）
- v8 用 `RENAME COLUMN` 重命名 `current_location→status`、`current_path→last_seen_path`，并加 `file_state` 列 + 数据回填（`permanently_deleted→deleted`、`has_physical_file=0→file_state='missing'`）；幂等检查同列存在性
- v2→v8 非破坏性：列带 `DEFAULT`，`CREATE TABLE IF NOT EXISTS`，已有数据零损耗

### HTTP API（独立 runtime）

`http::build_router` 在独立 `std::thread` + multi-thread tokio runtime（4 worker）启动，**不**依赖 Tauri 占用的 `#[tokio::main]` 运行时（否则 axum task 被饿死）。监听顺序：先绑 `app_setting.api_port`，被占则回退 `127.0.0.1:0`，实际端口持久化到 `app_setting.api_port`。

**鉴权白名单**（不需 Bearer token，其余全部走 `require_auth`）：
- `GET /api/health`
- `GET /api/covers/*`（浏览器 `<img>` 直读）
- `GET /api/doujinshi/:id/images*`（缩略图直读）

路由表（顺序敏感；`/by-hash` 必须在 `:hash` 通配之前，`/covers/by-hash` 必须在 `:file_id` 之前）：

| 路径 | 方法 | 说明 |
|---|---|---|
| `/api/health` | GET | 健康检查，返 `{status, version}` |
| `/api/doujinshi/search?q=&status=&limit=&offset=` | GET | 标题/社团/文件名 `LIKE %q%`；返 `{items, total}` |
| `/api/doujinshi/check?hash=` | GET | 按 BLAKE3 查单条（浏览器扩展友好别名） |
| `/api/doujinshi/by-hash/:hash` | GET | 同上（路径风格） |
| `/api/doujinshi/:id` | GET | 单条详情 |
| `/api/doujinshi/:id` | PATCH | `MetadataPatch` 部分更新；`None` 字段不写 |
| `/api/doujinshi/:id/images` | GET | 列图，ETag = `"{id}-{mtime_unix}"` 触发 304；`Cache-Control: no-store`（因为 `thumb_cached` 依赖运行时磁盘状态） |
| `/api/doujinshi/:id/images/:index` | GET | 单图：cache hit 吐 webp，miss 直返原图（mime 按 magic bytes 探测） |
| `/api/doujinshi/:id/images/:index/thumb` | PUT | 前端把 Worker 转好的 webp 落 LRU；已存在返 204 幂等 |
| `/api/doujinshi/:id/viewed` | POST | 标已读，204 |
| `/api/doujinshi/:id/archive` | POST | V4 任意 status → `archived` |
| `/api/doujinshi/:id/restore` | POST | V4 任意 status → `in_library`（含 deleted→in_library） |
| `/api/conflicts/:id/compare` | GET | 冲突对比（A 端 + B 端） |
| `/api/covers/:file_id` | GET | 封面（webp/jpg，magic bytes 探测） |
| `/api/covers/by-hash/:hash` | GET | 同上（hash 风格） |
| `/api/dirty` | GET | 列出 `dirty_data` 全部 |

URL 中 path-only（如 `cover_url`、`/api/doujinshi/:id/images/:index`）前端用 `useSettingsStore.apiBase` 拼绝对 URL。

### Tauri 事件（前后端实时同步）

| 事件名 | 何时发 | payload |
|---|---|---|
| `library-updated` | scanner 扫完一轮 | `number`（processed） |
| `scanner-status` | 扫描进度每文件 + 完成时 | `ScanStatus`（`is_scanning` / `processed` / `total` / `failed`） |
| `rar-error` | `identify_file` 在 rar 上失败 | `RarErrorEntry`（filename / file_path / RarError） |

`main.ts` 把 `library-updated` 转去并发刷新 3 个 store（library / recycle / inbox）。`scanner-status` 走 `useScanStatusStore`（首次进入还跑 `get_scan_status` 拿快照）。`rar-error` 走 `useInboxStore.pushRarError`（按 `file_path` 去重）。

### LRU Preview Cache（preview_cache.rs）

- 目录 `resources/_preview_cache/`，单文件 `<file_id>-<image_index>.webp`
- 内存 `lru::LruCache<CacheKey=(i64, usize), CacheEntry>` + 字节数 `AtomicU64`
- 容量默认 200 MiB；超 80% waterline 弹出最旧
- 启动 `PreviewCache::new` 扫盘 `reload_from_disk`：malformed 文件按 `<id>-<idx>.webp` 解析失败就删
- 后台 `gc()` 每 30s 兜底压回 waterline（lib.rs spawn 的循环）
- 状态转移 / 重识别后 `invalidate(id)` 清该 id 全部 entry
- `/images` 端点 `thumb_cached` 字段用 `contains OR is_on_disk`——`is_on_disk` 兜底防止 LRU/磁盘不一致时前端误判未缓存
- `image_mime` / `cover_mime` 按 magic bytes 探测：webp（`RIFF....WEBP`）/png（`\x89PNG\r\n\x1a\n`）/其他默认 jpeg（浏览器通常能猜对）

### 前端状态（stores/index.ts）

5 个 store；`useSettingsStore` 持 `apiBase` 和 `auth_token` 是 HTTP 调用的唯一权威来源。

- `useLibraryStore`：`items` / `queryInput`（v-model 用） / `query`（防抖 300ms 后真正查询） / `status`（all/viewed/not_viewed/marked） / `statusFilter`（V4 business status: active/all/in_library/archived/recycle/deleted，默认 active=排除 recycle+deleted） / `topCircles`（top 10 社团） / `visibleItems`（active 模式下二次过滤）
- `useRecycleStore`：`present`（`file_state='present'`）/ `gone`（`file_state != 'present'`：missing+absent_confirmed）—— V4 把"文件还在"和"文件已丢失/销毁"分组
- `useInboxStore`：`conflicts` + `rarErrors`（按 `file_path` 去重）+ `retryExtractLarge`（前端确认后调 `force_extract` 跳过 size gate）
- `useDirtyStore`：`entries`（直接 `dirty_data` 表）
- `useScanStatusStore`：右下浮窗展示进度，`visible = total > 0 && !dismissed`

## 关键约束（开发时易踩）

- **不要**在外层 command 里拼装 file-rename 逻辑，所有状态转移走 `state_machine::transition_with_dirs`。
- **不要**在 store 里加写后逻辑触发其他 store 重载——`library-updated` 事件已统一处理，避免循环。
- **不要**改 `MIGRATIONS` 旧条目，新增列/表必须 append；并保证幂等（用 `CREATE TABLE IF NOT EXISTS` 或 `pragma_table_info` 判列存不存在）。
- **不要**假设前后端 event 总能送达：HTTP 路由必须独立可调用（前端 store 刷新、`apiGet/Post/Put/Patch` 都走 HTTP），Tauri command 是辅助通道。
- 启动扫描器**只**处理 `resources/doujinshi/` 顶层 `.zip`/`.rar`，不递归子目录。inbox 之外的 3 个目录由 `dirty_scanner` 启动时扫一次。
- 防抖窗口 2 秒，新文件 ~2-3 秒内出现在 Library。
- HTTP 端口是 OS 随机分配，**不**固定；端口持久化到 `app_setting.api_port`，**不**直接读该值来拼 URL，必须用 `useSettingsStore.apiBase`（用户在 Settings 改完要重启 listener，但 `api_port` 已是持久化值）。
- 封面输出是 webp（V3+；`cover_format::encode_webp`），HTTP `cover` handler 按 magic bytes 探测 mime 而不是看扩展名（旧 V1/V2 写的 jpg 也走该路由）。
- `app_setting` 表里至少含：`auth_token`（首次启动 32 字节 URL-safe base64 生成，persisted）、`api_port`（`0`=OS 随机，否则锁定端口）。
- 冲突解决 4 选 1（`commands::inbox::ConflictAction`）：`keep_a`（删 B，保留 A）/ `replace_b`（删 A zip，让 B 顶替）/ `keep_both`（B 加 `(copy)` 后缀入库）/ `skip`（原行为，只标 resolved）。`has_open_conflict=true` 时前端禁用归档/移回收站/彻底删除等按钮，后端 `commands::guards::ensure_no_open_conflict` 兜底（HTTP 浏览器扩展绕不过）。

## 关键文档

- 设计 spec（V1 基础）：`docs/superpowers/specs/2026-07-09-doujinshi-records-design.md`
- V3 spec（归档 + 脏数据 + webp，已被 V4 取代）：`docs/superpowers/specs/2026-07-11-v3-archive-and-dirty-data.md`
- V3.1 spec（LRU preview cache）：`docs/superpowers/specs/2026-07-11-v31-lru-preview-cache.md`
- V4 spec（数据与文件解耦，当前权威）：`docs/superpowers/specs/2026-07-15-decouple-data-and-file.md`
- V4 实施 plan：`docs/superpowers/plans/2026-07-15-decouple-data-and-file.md`
- V4.5 增量 spec（脏数据页「重新入库」按钮 + mover-only）：`docs/superpowers/specs/2026-07-16-dirty-reingest-button.md`
- 实施 plan：`docs/superpowers/plans/2026-07-09-doujinshi-records-v1.md` / `2026-07-11-v3-archive-and-dirty-data.md` / `2026-07-11-v31-lru-preview-cache.md`
- 增量 plan：`docs/superpowers/plans/v1x/` / `v2/`
- 项目总览：`README.md`
- CodeGraph 使用：`.claude/CLAUDE.md`（Claude Code 专用）/ `AGENTS.md`（多 agent 共享）
