# doujinshi-records

本地 Tauri 桌面应用，管理个人同人志库。

应用监控 inbox 目录的新压缩包，计算哈希、提取封面，
把每份文件按 `identified → will-delete → permanently-deleted` 三个阶段追踪。
本地 HTTP API 暴露在 `127.0.0.1`，供浏览器扩展与其他本地工具查询。

> 仅个人使用。内容全部留在本机。**不下载、不分发**同人志——只整理你已有的文件。

## 功能

- 监听 `resources/doujinshi/` 的新 ZIP / RAR 压缩包
- BLAKE3 哈希去重 + 文件名解析（标题 / 社团 / 系列 / 译者 / 版本）
- 自动抽取封面（压缩到约 100 KB WebP 格式）本地保存
- 4 状态管理：`inbox → identified / will_delete / archived`，互相转移
- 启动时脏数据扫描：发现位于数据目录但 DB 无对应行的孤儿文件
- 文件名冲突检测：停在 Inbox，等用户决定跳过或比对
- 二次确认删除（对话框 A → 对话框 B），防误触
- 回收站视图：还原 + 永久删除
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
| POST | `/api/doujinshi/<id>/archive` | 移到归档目录 |
| POST | `/api/doujinshi/<id>/restore` | 取回到已入库 |
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
- 「我看过没？打算留还是删？」——查响应里的 `viewed` 和 `marked_for_delete` 字段

## 数据模型

主表 `doujinshi_file`（含 `current_location` 4 状态字段 + `has_physical_file`），辅助表 `filename_alias`、`conflict`、`scan_event`、`dirty_data`。设置存在 `app_setting`。完整 schema 见 `src-tauri/src/db/migrations.rs`。

## V3 迁移说明

V3 在 V2 schema 之上做两处增量改动：

1. **`doujinshi_file.has_physical_file` 新列**（默认 `1` = true）。`ALTER TABLE` 自动应用，V2 行直接获得该列。
2. **`dirty_data` 新表**。`CREATE TABLE IF NOT EXISTS`，幂等创建，不影响 V2 数据。

启动时 `db::migrations::init_schema_versioned` 会按 `schema_version` 顺序应用所有未跑的迁移，并对每个迁移做幂等检查（`pragma_table_info` 检测列存不存在）。**已存在的数据不会被删除或重建**——V2 用户升级即用。

新增的 `doujinshi-archived/` 数据目录第一次启动时自动 `mkdir`（`AppConfig::ensure_dirs`）。如果 V2 业务上手动挪过文件到该目录，启动后会被识别为合法归档文件。

唯一可见的行为变化：

- 封面格式由 jpg 改 webp——只影响 V3 之后新入库的文件（旧 jpg 封面仍以 `image/jpeg` 通过 `/api/covers/<hash>` 提供）。
- Library 视图多了「归档」位置筛选 + 「脏数据」独立页面。

## 开发注意

- 监听器有 2 秒防抖窗口，新文件约 2–3 秒内出现在 Library。
- 扫描器**只**处理 `resources/doujinshi/` 顶层的 `.zip` / `.rar`，子目录被忽略。
- 数据库在 `<resources>/data.db`（首次运行时创建，schema 由 `init_schema_versioned` 自动迁移到 CURRENT_VERSION）。

## 设计文档

- 设计 spec（V1 基础）：`docs/superpowers/specs/2026-07-09-doujinshi-records-design.md`
- V3 spec（归档 + 脏数据 + webp）：`docs/superpowers/specs/2026-07-11-v3-archive-and-dirty-data.md`
- 实施 plan（V1）：`docs/superpowers/plans/2026-07-09-doujinshi-records-v1.md`
- V3 plan（归档 + 脏数据 + webp）：`docs/superpowers/plans/2026-07-11-v3-archive-and-dirty-data.md`
- V1.x 增量 plan：`docs/superpowers/plans/v1x/`
- V2 增量 plan：`docs/superpowers/plans/v2/`
