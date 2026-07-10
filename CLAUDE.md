# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## 项目说明

- **同人志档案**（doujinshi-records）：本地 Tauri 桌面应用，管理个人同人志库。
- 监控 `resources/doujinshi/` 入库新压缩包，计算 BLAKE3 哈希并提取封面，状态流：identified → will-delete → permanently-deleted。
- 暴露本地 HTTP API（127.0.0.1）供浏览器扩展查询。
- 仅管理本地文件，不下载或分发内容。

## 技术栈

- **后端**：Rust + Tauri 2 + SeaORM 1.1 (SQLite) + Axum 0.7 + notify-debouncer-full + BLAKE3
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
│   ├── views/                  # 4 个页面：Library / Inbox / RecycleBin / Settings
│   ├── components/             # 通用组件（FileCard、DeleteDialogA/B、Restore/Permanent）
│   ├── stores/index.ts         # Pinia 状态：library / recycle / inbox / settings
│   ├── api/tauri.ts            # Tauri invoke 封装（与后端命令一一对应）
│   ├── types/api.ts            # 前后端共享类型定义
│   └── router.ts               # 4 个路由
├── src-tauri/
│   ├── src/
│   │   ├── lib.rs              # Tauri 启动、命令注册、HTTP 端口持久化
│   │   ├── main.rs             # tokio::main、配置加载、DB 初始化
│   │   ├── config.rs           # AppConfig + 资源目录派生方法
│   │   ├── error.rs            # AppError / AppResult
│   │   ├── commands/           # 13 个 Tauri 命令（library / inbox / recycle / settings）
│   │   ├── services/           # 业务核心：scanner / identifier / hasher / filename_parser / archive / cover
│   │   ├── http/               # Axum 路由 + ApiState，HTTP 服务跑在独立线程 + 独立 tokio runtime
│   │   ├── db/                 # SeaORM 实体 + 原始 SQL 迁移
│   │   └── models/             # 跨前后端序列化结构（FileSummary）
│   ├── capabilities/default.json
│   ├── tauri.conf.json         # devUrl=http://localhost:1420
│   └── Cargo.toml
├── resources/                  # 运行时数据（git 忽略大部分内容）
│   ├── doujinshi/              # 入库：放 .zip/.rar
│   ├── doujinshi-identified/   # 已识别
│   ├── doujinshi-will-delete/  # 待删除
│   ├── covers/                 # 提取的封面（~100 KB JPEG）
│   └── data.db                 # SQLite
├── docs/superpowers/           # 设计 spec + 实施 plan
└── .claude/                    # 本项目的 CodeGraph 指令（已配置）
```

## 架构要点

### 后端数据流

`scanner.rs` 启动 `notify-debouncer-full`（2 秒防抖窗口）监控 inbox 目录。任一文件事件触发 `scan_inbox_once` → 遍历 `*.zip` / `*.rar` → 调用 `identifier::identify_file`：

1. BLAKE3 哈希
2. 哈希命中 → 记录 alias、更新路径
3. 文件名 + 扩展名冲突 → 写入 `conflict` 表，停在 inbox
4. 提取封面（`archive::list_images` → `pick_cover` → `cover::extract_and_save` 压缩到 ≤100 KB）
5. 移动到 `doujinshi-identified/`
6. 插入 `doujinshi_file` 行 + `filename_alias` + `scan_event`

扫描结束 `emit("library-updated", n)` 通知前端。

### 前端状态

Pinia store 持有列表数据，监听 `library-updated` 事件刷新 3 个 store（library / recycle / inbox）。所有写操作直接调 Tauri command 并乐观更新本地状态。

### HTTP API（独立运行时）

`http::build_router` 在独立 `std::thread` + `current_thread` tokio runtime 启动 Axum，**不**依赖 Tauri 占用的 `#[tokio::main]` 运行时（避免 starvation）。首次启动绑定 `api_port` 设置中保留的端口，被占用则回退到 `127.0.0.1:0`，实际端口持久化到 `app_setting` 表供下次优先使用。CORS 全开。

### 跨设备 rename 兜底

`library::move_to_will_delete` 和 `recycle::restore_from_recycle` 都做了 `std::fs::rename` 的 `CrossesDevices` / Windows `ERROR_NOT_SAME_DEVICE` 兜底（copy + remove）。spec 已记录此风险。

## 工作流

继承自 `AGENTS.md`：

- **必须先判断任务等级**（Level 0~3），再选择流程：Level 0（文案/样式）= Implement + Review；Level 1（单文件）= Plan + Implement + Review；Level 2（多文件/新模块）= `/plan-eng-review` + Brainstorm + Plan + Implement + `/review` + `/qa`；Level 3（架构/数据库）= 完整链。
- **禁止直接开始编码**。复杂任务先 Plan。
- 优先使用 gstack + Superpowers；默认不依赖 OpenSpec。

## 关键文档

- 设计 spec：`docs/superpowers/specs/2026-07-09-doujinshi-records-design.md`
- 实施 plan：`docs/superpowers/plans/2026-07-09-doujinshi-records-v1.md`
- v1.x 增量 plan：`docs/superpowers/plans/v1x/`
- 项目总览：`README.md`
- 协作工作流：`AGENTS.md`

## 开发注意

- `pnpm.onlyBuiltDependencies` 已配置 `esbuild` 和 `vue-demi`；pnpm 报错时检查 `package.json` 此节是否还在。
- 扫描器只看 `resources/doujinshi/` 顶层，**不递归子目录**。
- 防抖窗口 2 秒，所以新文件 ~2-3 秒内出现在 Library。
- DB schema 在 `src-tauri/src/db/migrations.rs`（`init_schema` 幂等创建，非 SeaORM migration 框架）。
- 后端服务日志通过 `tracing`，运行 `pnpm tauri dev` 时在终端可见。
- HTTP 端口是 OS 随机分配，**不**固定；前端 `useSettingsStore` 是唯一权威来源。
