# doujinshi-records

本地 Tauri 桌面应用，管理个人同人志库。

应用监控 inbox 目录的新压缩包，计算哈希、提取封面，
把每份同人志数据按 4 状态（`in_library / archived / recycle / deleted`）追踪，文件本身作为观测值与「可能存在的附庸」（`present / missing / absent_confirmed`）。数据由用户决定；文件只是视图。
本地 HTTP API 暴露在 `127.0.0.1`，供浏览器扩展与其他本地工具查询。

> 仅个人使用。内容全部留在本机。**不下载、不分发**同人志——只整理你已有的文件。

## 功能

- 监听 `resources/doujinshi/` 的新 ZIP / RAR 压缩包
- BLAKE3 哈希去重 + 文件名解析（标题 / 社团 / 系列 / 译者 / 版本）
- 自动抽取封面（压缩到约 100 KB WebP 格式）本地保存
- V4 双字段模型：业务 `status ∈ {in_library, archived, recycle, deleted}`（任意可切）+ 文件 `file_state ∈ {present, missing, absent_confirmed}`（扫描 / 销毁维护）
- 状态机「DB 优先 + 文件 best-effort」：源文件缺失不阻塞 status 更新；目标目录同名视为孤儿（`dirty_data(reason='overwritten_by_state_switch')`），自动覆盖
- 销毁复合操作：`status=deleted` + `file_state=absent_confirmed` + best-effort 删文件 + LRU 缩略图失效
- 启动时脏数据扫描：扫 4 个状态目录（不扫 deleted），按 file_state 三态更新
- 文件名冲突检测：停在 Inbox，等用户决定跳过或比对；冲突 ReplaceB 把旧记录推到 `deleted + absent_confirmed`（不是终态，可恢复）
- 回收站视图：按 `file_state` 分 present / gone 两段；还原 + 永久删除
- 本地 HTTP API 供浏览器扩展（`/api/health`、`/api/doujinshi/...`）
- 实时更新：扫描器 emit `library-updated` 事件，前端自动刷新

## 技术栈

- Rust + Tauri 2（后端 + 窗口）
- SeaORM 1.1 操作 SQLite
- Axum 0.7（HTTP API）
- notify-debouncer-full 0.3（文件系统监听）
- BLAKE3（哈希）
- image 0.25 + zip / rar（封面抽取）
- Vue 3 + TypeScript + Naive UI（前端）
- Pinia（状态），Vue Router

## 项目目录

```
doujinshi-records/
  resources/
    doujinshi/                 # inbox：把新压缩包拖到这里
    doujinshi-identified/      # 识别后自动移到这里
    doujinshi-will-delete/     # 用户标记删除的文件
    doujinshi-archived/        # 归档文件
    covers/                    # 抽取出的封面（约 100 KB / 张，webp）
  src/                         # Vue 前端
  src-tauri/
    src/
      commands/                # Tauri 命令（前后端桥接）
      db/                      # SeaORM 实体 + 原始 SQL 迁移
      http/                    # Axum 路由 + handlers
      services/                # scanner / identifier / hasher / parser / archive / cover / dirty_scanner / state_machine
  docs/superpowers/            # 设计 spec + 实施 plan
```

## 快速开始

### 环境要求

- Rust 1.77+
- Node.js 20+
- pnpm 9+
- Windows 10 / 11（Tauri 2 WebView2 运行时已自带）

### 安装依赖

```bash
pnpm install
```

### 开发模式（启动 Vite + Tauri）

```bash
pnpm tauri dev
```

首次运行会自动创建 `resources/`、应用 SQLite schema，并在终端打印：

```
http api listening on http://127.0.0.1:<port>
```

实际端口可在 Settings 页面查看。

### 发布构建

```bash
pnpm tauri build
```

## HTTP API

所有端点服务在 `http://127.0.0.1:<随机端口>`（见 Settings 页面取实际 URL）。CORS 对任意 origin 开放。

| 方法 | 路径 | 说明 |
|---|---|---|
| GET | `/api/health` | 健康检查 |
| GET | `/api/doujinshi/search?q=<query>` | 按标题 / 社团 / 文件名搜索 |
| GET | `/api/doujinshi/by-hash/<hash>` | 按 BLAKE3 哈希查找 |
| GET | `/api/doujinshi/<id>` | 单条记录 |
| POST | `/api/doujinshi/<id>/archive` | 任意 status → `archived`（V4） |
| POST | `/api/doujinshi/<id>/restore` | 任意 status → `in_library`，含 `deleted → in_library`（V4） |
| GET | `/api/covers/<file_id>` | 封面 WebP（约 100 KB） |
| GET | `/api/dirty` | 列出孤儿文件（脏数据扫描结果） |

PowerShell 示例：

```powershell
$port = (Get-Content resources/.api-port)  # 实际端口见 Settings
Invoke-RestMethod "http://127.0.0.1:$port/api/health"
Invoke-RestMethod "http://127.0.0.1:$port/api/doujinshi/search?q=sample"
```

浏览器扩展典型用法：

- 「这本同人志我下载过没？」——按标题或哈希搜索
- 「我看过没？打算留还是删？」——查响应里的 `viewed`、`status`、`file_state` 字段

## 数据模型

主表 `doujinshi_file`（业务 `status` 4 值 + 文件 `file_state` 3 态 + `last_seen_path`），辅助表 `filename_alias`、`conflict`、`scan_event`、`dirty_data`。设置存在 `app_setting`。完整 schema 见 `src-tauri/src/db/migrations.rs`。

## V4 迁移说明（2026-07-15，schema v8）

V4 在 V7 schema 之上做 3 处增量改动（均为非破坏性升级）：

1. **`doujinshi_file.file_state` 新列**（TEXT，默认 `'present'`）。`ALTER TABLE ADD COLUMN` + `pragma_table_info` 幂等检查。
2. **`doujinshi_file.current_location` 重命名为 `status`**（SQLite 3.25+ `RENAME COLUMN`，幂等：检测列不存在则跳过）。
3. **`doujinshi_file.current_path` 重命名为 `last_seen_path`**。
4. **数据回填**：`status='permanently_deleted' → 'deleted'`；`has_physical_file=0 → file_state='missing'`。

启动时 `db::migrations::init_schema_versioned` 会按 `schema_version` 顺序应用所有未跑的迁移，并对每个迁移做幂等检查。**已存在的数据不会被删除或重建**——V3 用户升级即用。

唯一可见的行为变化：

- Library 默认过滤从「active」（V4: 排除 recycle + deleted）→ 之前版本是「in_library」单值；切换过滤可见已删记录
- 状态切换不再因源文件缺失而拒绝；可以手动把 missing 的 archived 切回 in_library
- RecycleBin 现按 `file_state` 分 present / gone 两段；missing 文件显示但不可还原
- DetailView 在文件丢失时显示 n-alert 提示

## 开发注意

- 监听器有 2 秒防抖窗口，新文件约 2–3 秒内出现在 Library。
- 扫描器**只**处理 `resources/doujinshi/` 顶层的 `.zip` / `.rar`，子目录被忽略。
- 数据库在 `<resources>/data.db`（首次运行时创建，schema 由 `init_schema_versioned` 自动迁移到 CURRENT_VERSION=8）。
- 启动脏数据扫描只跑 4 个状态目录（`identified / will_delete / archived`，不扫 deleted）——`dirty_scanner` 在启动时同步触发一次。

## 设计文档

- 设计 spec（V1 基础）：`docs/superpowers/specs/2026-07-09-doujinshi-records-design.md`
- V3 spec（已被 V4 取代）：`docs/superpowers/specs/2026-07-11-v3-archive-and-dirty-data.md`
- V3.1 spec（LRU preview cache）：`docs/superpowers/specs/2026-07-11-v31-lru-preview-cache.md`
- **V4 spec（数据与文件解耦，当前权威）**：`docs/superpowers/specs/2026-07-15-decouple-data-and-file.md`
- V4 实施 plan：`docs/superpowers/plans/2026-07-15-decouple-data-and-file.md`
- 实施 plan（V1）：`docs/superpowers/plans/2026-07-09-doujinshi-records-v1.md`
- V3 plan（归档 + 脏数据 + webp）：`docs/superpowers/plans/2026-07-11-v3-archive-and-dirty-data.md`
- V1.x 增量 plan：`docs/superpowers/plans/v1x/`
- V2 增量 plan：`docs/superpowers/plans/v2/`
- V4 设计由来（用户原始需求对话）：`docs/数据与文件状态机逻辑分析.md`
