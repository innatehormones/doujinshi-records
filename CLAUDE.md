# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## 项目说明

- **同人志档案**（doujinshi-records）：本地 Tauri 桌面应用，管理个人同人志库。
- 监控 `resources/doujinshi/` 入库新压缩包，计算 BLAKE3 哈希并提取 webp 封面。
- 文件按 4 状态机流转：`inbox → identified / will_delete / archived`。DB 行只增不改，数据永生。
- 启动时扫描 `identified/will_delete/archived` 三个目录，把"DB 无对应行"的孤儿写入 `dirty_data` 表，由"脏数据"页提示用户。
- 暴露本地 HTTP API（127.0.0.1）供浏览器扩展查询。
- 仅管理本地文件，不下载或分发内容。

## 技术栈

- **后端**：Rust + Tauri 2 + SeaORM 1.1 (SQLite) + Axum 0.7 + notify-debouncer-full + BLAKE3 + lru
- **前端**：Vue 3 + TypeScript + Naive UI + Pinia + Vue Router
- **包管理**：pnpm workspace（单包）
- **平台**：Windows 10/11（Tauri 2 WebView2）

## 开发命令

所有命令在项目根目录运行。

### 日常开发

```bash
# 安装依赖（首次或新增依赖后）
pnpm install

# 完整开发模式（启动 Vite + Tauri + Rust 后端）
pnpm tauri dev

# 仅前端开发（Vite HMR，后端需独立运行）
pnpm dev

# 构建前端（类型检查 + Vite build）
pnpm build

# 构建发布包
pnpm tauri build
```

### Rust 端

```bash
# 类型检查（后端）
cd src-tauri && cargo check

# 单独运行某个测试
cd src-tauri && cargo test <test_name>           # e.g. cargo test hashes_known_content
cd src-tauri && cargo test <module_path>::       # e.g. cargo test services::hasher::

# 运行所有测试
cd src-tauri && cargo test
```

> `src-tauri` 是独立的 crate；测试模块位于各 service 文件 `#[cfg(test)]` 内（如 `hasher.rs`、`cover.rs`、`filename_parser.rs`）。

### 前端类型检查

```bash
# 严格 TypeScript 检查（与 build 同源）
pnpm exec vue-tsc --noEmit
```

## 项目结构

```
doujinshi-records/
├── src/                        # Vue 3 前端
│   ├── views/                  # 6 个页面：Library / Detail / Inbox / Conflict / RecycleBin / Dirty / Settings
│   ├── components/             # 通用组件（FileCard 等）
│   ├── stores/index.ts         # Pinia 状态：library / recycle / inbox / dirty / settings
│   ├── api/tauri.ts            # Tauri invoke 封装（与后端命令一一对应）
│   ├── types/api.ts            # 前后端共享类型定义（含 current_location / has_physical_file / DirtyEntry）
│   └── router.ts               # 7 个路由
├── src-tauri/
│   ├── src/
│   │   ├── lib.rs              # Tauri 启动、命令注册、HTTP 端口持久化、dirty_scanner 启动
│   │   ├── main.rs             # tokio::main、配置加载、DB 初始化（含 recovery probe）
│   │   ├── config.rs           # AppConfig + 资源目录派生方法（identified/will_delete/archived/inbox/preview_cache）
│   │   ├── error.rs            # AppError / AppResult
│   │   ├── commands/           # Tauri 命令（library / inbox / recycle / dirty / settings）
│   │   ├── services/           # 业务核心：scanner / identifier / hasher / filename_parser / archive / cover / cover_format / state_machine / dirty_scanner / preview_cache
│   │   ├── http/               # Axum 路由 + ApiState + 鉴权中间件，HTTP 服务跑在独立线程 + 独立 tokio runtime
│   │   ├── db/                 # SeaORM 实体 + 版本化迁移（init_schema_versioned，CURRENT_VERSION=5）
│   │   └── models/             # 跨前后端序列化结构（FileSummary）
│   ├── capabilities/default.json
│   ├── tauri.conf.json         # devUrl=http://localhost:1420
│   └── Cargo.toml
├── resources/                  # 运行时数据（git 忽略大部分内容）
│   ├── doujinshi/              # 入库：放 .zip/.rar
│   ├── doujinshi-identified/   # 已识别
│   ├── doujinshi-will-delete/  # 待删除
│   ├── doujinshi-archived/     # 归档
│   ├── covers/                 # 提取的封面（~100 KB WebP）
│   ├── _preview_cache/         # HTTP images 响应体 LRU 缓存（disk-backed，gzip 自动）
│   └── data.db                 # SQLite
├── docs/superpowers/           # 设计 spec + 实施 plan
└── .claude/                    # 本项目的 CodeGraph 指令（已配置）
```

## 架构要点

### 后端数据流

`scanner.rs` 启动 `notify-debouncer-full`（2 秒防抖窗口）监控 inbox 目录。任一文件事件触发 `scan_inbox_once` → 遍历 `*.zip` / `*.rar` → 调用 `identifier::identify_file`：

1. BLAKE3 哈希
2. 哈希命中 → 记录 alias；若旧行已不再 `identified`，调用 `reactivate_row` 把源文件移回 `identified/`，更新 `current_location` + `has_physical_file`
3. 文件名 + 扩展名冲突 → 写入 `conflict` 表，停在 inbox
4. 提取封面（`archive::list_images` → `pick_cover` → `cover::extract_and_save` 用 `cover_format::encode_webp` 输出 ≤100 KB）
5. 移动到 `doujinshi-identified/`
6. 插入 `doujinshi_file` 行 + `filename_alias` + `scan_event`

扫描结束 `emit("library-updated", n)` 通知前端。

### 4 状态机（state_machine.rs）

`current_location` 字段驱动状态：`identified` / `will_delete` / `archived`（+ `inbox` 仅作前台投影）。状态转移集中在 `state_machine::transition_with_dirs`：DB 事务更新列 + `std::fs::rename` 移动文件。跨设备 rename 走 copy + remove 兜底。

- `transition_with_dirs(conn, id, Archive | Restore | MarkForDelete, identified_dir, will_delete_dir, archived_dir)`——4 参数版本便于单测
- 非法转移（如 `archived → will_delete`）返回 Err；HTTP 路由映射成 409
- `has_physical_file` 字段只由 `dirty_scanner` 启动扫描线程维护；状态转移不主动更新它

### 启动脏数据扫描（dirty_scanner.rs）

启动一次扫描 `identified/`、`will_delete/`、`archived/` 三目录：
- 文件存在但 DB 无行 → 写入 `dirty_data`（孤儿）
- DB 行有 `current_path` 但磁盘文件丢失 → 行 `has_physical_file = false`，前端显示"文件丢失"标签

`inbox/` 不扫（scanner 还在处理中）。结果给前端 `/dirty` 页面 + `/api/dirty` HTTP 端点。

### 版本化迁移（db/migrations.rs）

`init_schema_versioned` 按 `schema_version` 表的有序 `MIGRATIONS` 数组逐版本向前推进；V2 → V5 之间为非破坏性（`ALTER TABLE` 默认值 + `CREATE TABLE IF NOT EXISTS`），已有数据零损耗。`CURRENT_VERSION = 5`。

### 前端状态

Pinia store 持有列表数据，监听 `library-updated` 事件刷新 5 个 store（library / recycle / inbox / dirty / settings）。所有写操作直接调 Tauri command 并乐观更新本地状态。

### HTTP API（独立运行时）

`http::build_router` 在独立 `std::thread` + `current_thread` tokio runtime 启动 Axum，**不**依赖 Tauri 占用的 `#[tokio::main]` 运行时（避免 starvation）。首次启动绑定 `api_port` 设置中保留的端口，被占用则回退到 `127.0.0.1:0`，实际端口持久化到 `app_setting` 表供下次优先使用。CORS 全开；除 `/api/health` 与 `/api/covers/*` 外全部路由走 Bearer token 鉴权。

端点清单：
- 查询：`/api/health`、`/api/doujinshi/search`、`/api/doujinshi/by-hash/<hash>`、`/api/doujinshi/check?hash=`、`/api/doujinshi/<id>`、`/api/doujinshi/<id>/images`
- 写操作：`/api/doujinshi/<id>/viewed` / `/archive` / `/restore`、`/api/conflicts/<id>/compare`
- 资源：`/api/covers/<file_id>` / `/api/covers/by-hash/<hash>`（两者鉴权豁免，浏览器 `<img>` 直读）
- 元数据：`/api/dirty` 列出孤儿文件

### 跨设备 rename 兜底

`state_machine::transition_with_dirs` 和 V2 时点的 `library::move_to_will_delete` / `recycle::restore_from_recycle` 都做了 `std::fs::rename` 的 `CrossesDevices` / Windows `ERROR_NOT_SAME_DEVICE` 兜底（copy + remove）。spec 已记录此风险。

## 工作流

继承自 `AGENTS.md`：

- **必须先判断任务等级**（Level 0~3），再选择流程：Level 0（文案/样式）= Implement + Review；Level 1（单文件）= Plan + Implement + Review；Level 2（多文件/新模块）= `/plan-eng-review` + Brainstorm + Plan + Implement + `/review` + `/qa`；Level 3（架构/数据库）= 完整链。
- **禁止直接开始编码**。复杂任务先 Plan。
- 优先使用 gstack + Superpowers；默认不依赖 OpenSpec。

## 关键文档

- 设计 spec：`docs/superpowers/specs/2026-07-09-doujinshi-records-design.md`
- V3 spec（归档 + 脏数据 + webp）：`docs/superpowers/specs/2026-07-11-v3-archive-and-dirty-data.md`
- V3.1 spec（LRU preview cache）：`docs/superpowers/specs/2026-07-11-v31-lru-preview-cache.md`
- 实施 plan：`docs/superpowers/plans/2026-07-09-doujinshi-records-v1.md`
- V3 plan：`docs/superpowers/plans/2026-07-11-v3-archive-and-dirty-data.md`
- V3.1 plan：`docs/superpowers/plans/2026-07-11-v31-lru-preview-cache.md`
- v1.x 增量 plan：`docs/superpowers/plans/v1x/`
- v2 增量 plan：`docs/superpowers/plans/v2/`
- 项目总览：`README.md`
- 协作工作流：`AGENTS.md`

## 开发注意

- `pnpm.onlyBuiltDependencies`（旧字段，pnpm 10 已移到 `pnpm.workspace`）。
- 扫描器只看 `resources/doujinshi/` 顶层，**不递归子目录**；inbox 之外的 3 个目录由 dirty_scanner 启动扫描。
- 防抖窗口 2 秒，所以新文件 ~2-3 秒内出现在 Library。
- DB schema 在 `src-tauri/src/db/migrations.rs`，用版本化迁移（`init_schema_versioned` + `CURRENT_VERSION`）；`init_schema` 是 v1 bootstrap。**新增列或表必须 append 一个 MIGRATIONS 条目**，不要改旧条目。
- 后端服务日志通过 `tracing`，运行 `pnpm tauri dev` 时在终端可见。
- HTTP 端口是 OS 随机分配，**不**固定；前端 `useSettingsStore` 是唯一权威来源。
- 文件状态转移一律走 `state_machine::transition_with_dirs`，不要在外层 command 拼装 file-rename 逻辑。
- 封面输出是 webp（lossless），由 `cover_format::encode_webp` 负责；HTTP 路由的 Content-Type 仍是 `image/jpeg` 因为现有 jpg 历史封面也走该路径，浏览器对 jpg 解析正常。
- **`/api/doujinshi/:id/images` 走 LRU preview cache**：磁盘 `_preview_cache/<id>-<mtime>.json` + 内存 `lru::LruCache`，cache key `(file_id, zip_mtime)` 自动随 zip 改动失效；HTTP ETag = `"{id}-{mtime_unix}"` 触发 304 短路。后台 30s GC 兜底压回 80% waterline。Handler 写盘用 `tokio::spawn` fire-and-forget，命中优先。
