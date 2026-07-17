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
- 双字段数据模型：业务 `status ∈ {in_library, archived, recycle, deleted}`（任意可切）+ 文件 `file_state ∈ {present, missing, absent_confirmed}`（扫描 / 销毁维护）
- 状态机「DB 优先 + 文件 best-effort」：源文件缺失不阻塞 status 更新；目标目录同名视为孤儿（`dirty_data(reason='overwritten_by_state_switch')`），自动覆盖
- 销毁复合操作：`status=deleted` + `file_state=absent_confirmed` + best-effort 删文件 + LRU 缩略图失效
- 启动时脏数据扫描：扫 3 个状态目录（不扫 deleted），按 file_state 三态更新
- 脏数据页对 `orphan_file` 条目提供「重新入库」按钮：mover-only，`fs::rename` 到 inbox + 软删 dirty_data 行，剩下的入库由后台 scanner 接管（UI 立即返回，撞名 / rar 失败由 ConflictView / rar-error 兜底）
- 文件名冲突检测：停在 Inbox，等用户决定跳过或比对；冲突 ReplaceB 把旧记录推到 `deleted + absent_confirmed`（不是终态，可恢复）
- 文件回收站视图：只展示「待删除文件」（`status='recycle' + file_state='present'`），还原 / 永久删除；已被销毁的记录可在 Library 用 status filter（recycle / deleted）找到
- Inbox / RecycleBin 列表封面显示开关：标题右侧 Image/Rows3 切换，开启时每条卡片左侧渲染 64×80 缩略图；待处理冲突只显示 A 端封面（B 没入库无封面）
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
    covers/                    # 抽取出的封面（webp 字节，文件名 `<hash>.pwb`，自定义后缀防止被看图软件收编）
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
| POST | `/api/doujinshi/<id>/archive` | 任意 status → `archived` |
| POST | `/api/doujinshi/<id>/restore` | 任意 status → `in_library`，含 `deleted → in_library` |
| GET | `/api/covers/<file_id>` | 封面 WebP（约 100 KB） |
| GET | `/api/dirty` | 列出孤儿文件（脏数据扫描结果） |

PowerShell 示例（端口存在 `app_setting.api_port`，实际值见 Settings 页）：

```powershell
$port = 1421  # 替换为 Settings 页显示的端口
Invoke-RestMethod "http://127.0.0.1:$port/api/health"
Invoke-RestMethod "http://127.0.0.1:$port/api/doujinshi/search?q=sample"
```

浏览器扩展典型用法：

- 「这本同人志我下载过没？」——按标题或哈希搜索
- 「我看过没？打算留还是删？」——查响应里的 `viewed`、`status`、`file_state` 字段

## 数据模型

主表 `doujinshi_file`（业务 `status` 4 值 + 文件 `file_state` 3 态 + `last_seen_path`），辅助表 `conflict`、`scan_event`、`dirty_data`。设置存在 `app_setting`。完整 schema 与版本化迁移见 `src-tauri/src/db/migrations.rs`。

## 开发注意

- 监听器有 2 秒防抖窗口，新文件约 2–3 秒内出现在 Library。
- 扫描器**只**处理 `resources/doujinshi/` 顶层的 `.zip` / `.rar`，子目录被忽略。
- 数据库在 `<resources>/data.db`（首次运行时创建，schema 由 `init_schema_versioned` 自动迁移到 CURRENT_VERSION=12）。
- 启动脏数据扫描只跑 3 个状态目录（`identified / will_delete / archived`，不扫 deleted）——`dirty_scanner` 在启动时同步触发一次。

## 设计文档

按主题归档，不带版本号（避免被「V 编号」误导成按时间线去读旧 spec）。完整 spec 入口见 [`docs/superpowers/`](docs/superpowers/)。

- 数据与文件解耦 spec（业务 status + 文件 file_state 双字段，当前权威）：`docs/superpowers/specs/2026-07-15-decouple-data-and-file.md`
- 数据备份与还原 spec：`docs/superpowers/specs/2026-07-15-data-backup.md`
- 脏数据页「重新入库」spec：`docs/superpowers/specs/2026-07-16-dirty-reingest-button.md`
- 文件回收站简化 spec：`docs/superpowers/specs/2026-07-16-v46-recycle-simplification.md`
- 设置页重设计 spec：`docs/superpowers/specs/2026-07-16-v47-settings-redesign.md`
- HTTP API 测试弹窗 spec：`docs/superpowers/specs/2026-07-16-v48-api-test-dialog.md`
- 列表封面开关 spec：`docs/superpowers/specs/2026-07-16-v49-cover-toggle.md`
- 0.2.0 release plan：`docs/superpowers/plans/2026-07-17-v020-release.md`
