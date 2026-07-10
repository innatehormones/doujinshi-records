# doujinshi-records

本地 Tauri 桌面应用，管理个人同人志库。

应用监控 inbox 目录的新压缩包，计算哈希、提取封面，
把每份文件按 `identified → will-delete → permanently-deleted` 三个阶段追踪。
本地 HTTP API 暴露在 `127.0.0.1`，供浏览器扩展与其他本地工具查询。

> 仅个人使用。内容全部留在本机。**不下载、不分发**同人志——只整理你已有的文件。

## 功能

- 监听 `resources/doujinshi/` 的新 ZIP / RAR 压缩包
- BLAKE3 哈希去重 + 文件名解析（标题 / 社团 / 系列 / 译者 / 版本）
- 自动抽取封面（压缩到约 100 KB JPEG）本地保存
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
    covers/                    # 抽取出的封面（约 100 KB / 张）
  src/                         # Vue 前端
  src-tauri/
    src/
      commands/                # Tauri 命令（前后端桥接）
      db/                      # SeaORM 实体 + 原始 SQL 迁移
      http/                    # Axum 路由 + handlers
      services/                # scanner / identifier / hasher / parser / archive / cover
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
| GET | `/api/covers/<file_id>` | 封面 JPEG（约 100 KB） |

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

主表 `doujinshi_file`，辅助表 `filename_alias`、`conflict`、`scan_event`。设置存在 `app_setting`。完整 schema 见 `src-tauri/src/db/migrations.rs`。

## 开发注意

- `pnpm.onlyBuiltDependencies` 配置了 `esbuild` 和 `vue-demi`。pnpm 报错时检查 `package.json` 此节是否还在。
- 监听器有 2 秒防抖窗口，新文件约 2–3 秒内出现在 Library。
- 扫描器**只**处理 `resources/doujinshi/` 顶层的 `.zip` / `.rar`，子目录被忽略。
- 数据库在 `<resources>/doujinshi.db`（首次运行时创建）。

## 设计文档

- 设计 spec：`docs/superpowers/specs/2026-07-09-doujinshi-records-design.md`
- 实施 plan：`docs/superpowers/plans/2026-07-09-doujinshi-records-v1.md`
- V1.x 增量 plan：`docs/superpowers/plans/v1x/`
