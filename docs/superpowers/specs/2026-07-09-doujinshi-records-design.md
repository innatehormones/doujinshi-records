# 同人志管理（doujinshi-records）— 设计文档

> 日期：2026-07-09
> 状态：v0.3.1（self-review 后）

## 目标

构建一个本地同人志（混杂 18 禁）数据管理工具，覆盖以下核心循环：

1. 用户下载的同人志压缩文件落到待识别目录
2. 系统自动识别、计算哈希、解析命名、提取封面、入库到已识别目录
3. 前端浏览、查看、决定保留或删除
4. 删除走"软删 → 回收站 → 终删"三段式，数据始终保留
5. 对外暴露 HTTP API，供浏览器扩展查询"是否已下载/已看过/已删"

最终以 Tauri 应用形态发布。

## 技术栈

| 项 | 选择 |
|---|---|
| 后端 | Rust + Tauri 2 |
| 前端 | Vue 3 + TypeScript + Naive UI |
| 数据库 | SQLite + SeaORM |
| 哈希 | BLAKE3 |
| 文件监听 | `notify` crate |
| HTTP 服务 | axum（本地 127.0.0.1 随机端口） |

## 架构

```
┌─────────────────────────────────────────────┐
│ Vue 3 前端 (Naive UI)                        │
│   - 调用 Tauri 命令 (invoke)                 │
│   - 通过 fetch 调本地 HTTP API                │
└─────────────────────────────────────────────┘
        │                       │
        ▼                       ▼
┌──────────────────┐   ┌────────────────────┐
│ Tauri 命令层     │   │ Axum HTTP 服务     │
│ (前端 ↔ Rust)     │   │ 127.0.0.1:PORT     │
│                  │   │ 给浏览器扩展用       │
└──────────────────┘   └────────────────────┘
        │                       │
        └──────────┬────────────┘
                   ▼
        ┌──────────────────┐
        │ 服务层            │
        │  - 扫描服务        │
        │  - 哈希服务        │
        │  - 封面服务        │
        │  - 归档服务        │
        │  - 识别服务        │
        └──────────────────┘
                   │
        ┌──────────┼──────────┐
        ▼          ▼          ▼
    SeaORM    文件系统     notify
    (SQLite)  (resources/)  (监听)
```

核心原则：**所有读写都经过服务层**。Tauri 命令和 HTTP handler 都是薄壳，只做参数校验和返回值包装。

## 项目结构

```
doujinshi-records/
├── src-tauri/                      # Rust 后端
│   ├── src/
│   │   ├── main.rs                 # Tauri 入口，启动 axum
│   │   ├── commands/               # Tauri 命令（前端调）
│   │   │   ├── files.rs
│   │   │   ├── library.rs
│   │   │   └── recycle.rs
│   │   ├── http/                   # axum HTTP handler
│   │   │   └── api.rs
│   │   ├── services/               # 业务逻辑
│   │   │   ├── scanner.rs
│   │   │   ├── hasher.rs
│   │   │   ├── cover.rs
│   │   │   ├── archive.rs
│   │   │   └── identifier.rs
│   │   ├── db/                     # SeaORM 实体 + 迁移
│   │   │   ├── entities/
│   │   │   └── migrations/
│   │   ├── models/                 # 领域模型
│   │   └── config.rs
│   ├── Cargo.toml
│   └── tauri.conf.json
├── src/                            # Vue 前端
│   ├── views/
│   │   ├── LibraryView.vue         # 已识别库
│   │   ├── InboxView.vue           # 待识别
│   │   ├── ConflictView.vue        # 冲突对比（V2）
│   │   ├── RecycleBinView.vue      # 回收站
│   │   └── SettingsView.vue
│   ├── components/
│   ├── api/                        # Tauri 命令 + HTTP 客户端封装
│   ├── stores/                     # Pinia
│   ├── types/
│   ├── App.vue
│   └── main.ts
├── resources/                      # 数据文件夹（用户文件，不进 git）
│   ├── doujinshi/                  # 待识别
│   ├── doujinshi-identified/       # 已识别
│   ├── doujinshi-will-delete/      # 待删除
│   ├── covers/                     # 提取的封面（持久化缓存）
│   └── data.db                     # SQLite
├── docs/
│   └── superpowers/specs/
├── package.json
└── README.md
```

## 数据库 Schema

### 表 `doujinshi_file`（文件主表）

每行 = 一个 zip/rar 文件。

| 字段 | 类型 | 说明 |
|---|---|---|
| `id` | INTEGER PK | 自增 |
| `title` | TEXT NOT NULL | 同人志名（解析或原始） |
| `filename` | TEXT NOT NULL | 原始文件名（最近一次扫描时的名称） |
| `hash` | TEXT NOT NULL UNIQUE | BLAKE3 哈希（hex，64 字符） |
| `ext` | TEXT NOT NULL | `zip`/`rar`/... 小写，不带点 |
| `size_bytes` | INTEGER NOT NULL | 文件大小（字节） |
| `circle` | TEXT NULL | 社团名（解析） |
| `series` | TEXT NULL | 系列名（解析） |
| `translator` | TEXT NULL | 翻译版本标签 |
| `version_tag` | TEXT NULL | `DL版` / `カラー版` 等 |
| `current_path` | TEXT NOT NULL | 当前实际路径（相对 resources/） |
| `current_location` | TEXT NOT NULL | `inbox` / `identified` / `will_delete` |
| `cover_path` | TEXT NULL | 封面相对路径（`resources/covers/xxx.jpg`） |
| `marked_for_delete` | BOOLEAN NOT NULL DEFAULT 0 | 用户标了想删 |
| `physically_deleted` | BOOLEAN NOT NULL DEFAULT 0 | 文件本体已删（数据保留） |
| `viewed` | BOOLEAN NOT NULL DEFAULT 0 | 是否查看过 |
| `note` | TEXT NULL | 笔记字段（V2 用） |
| `created_at` | DATETIME NOT NULL | 入库时间 |
| `updated_at` | DATETIME NOT NULL | 状态更新时间 |

**关键决策：**
- `hash` 唯一 —— 相同 hash 视为同一份文件，不重复入库
- `current_path` + `current_location` 两个字段看似冗余，但便于查"现在在哪"
- `physically_deleted=true` 后 `current_path` 不动，保留历史；查询"在磁盘上有文件"时需附加 `physically_deleted=false`

### 表 `filename_alias`（同名/别名追踪）

| 字段 | 类型 | 说明 |
|---|---|---|
| `id` | INTEGER PK | |
| `file_id` | INTEGER FK → doujinshi_file | 关联到文件 |
| `alias_filename` | TEXT NOT NULL | 出现过的原始文件名 |
| `first_seen_at` | DATETIME | 首次发现时间 |
| UNIQUE | (`file_id`, `alias_filename`) | |

**作用**：文件改名后重新扫描，仍能识别为同一份（靠 hash）；同时记录所有出现过的名字，便于审计。

### 表 `conflict`（文件名+后缀冲突）

| 字段 | 类型 | 说明 |
|---|---|---|
| `id` | INTEGER PK | |
| `a_file_id` | INTEGER FK NOT NULL | 已入库的文件（identified） |
| `b_file_path` | TEXT NOT NULL | 新发现的，绝对路径（仍在 inbox，未入库） |
| `b_filename` | TEXT NOT NULL | 新文件的当前文件名 |
| `b_hash` | TEXT NULL | 已计算的 hash（可能为 NULL 表示未完成） |
| `reason` | TEXT NOT NULL | `name_ext_collision`，目前唯一合法值 |
| `resolved` | BOOLEAN DEFAULT 0 | 用户处理过没 |
| `created_at` | DATETIME | |

**触发条件（精确）**：
- 在 `doujinshi-identified/` 中已存在文件 `F`
- 在 `doujinshi/` 中发现新文件 `N` 满足 `N.filename + N.ext == F.filename + F.ext` 且 `N.hash != F.hash`
- 因 `hash` 唯一约束，`N` 无法入库，单独写一行 `conflict`，`N` 物理上保留在 `doujinshi/`

**`b_file_path` 而不是 `b_file_id` 的原因**：冲突文件**未经入库**，所以没有 id；要在前端展示它的封面也得有自己的 path。

### 表 `scan_event`（扫描事件日志）

| 字段 | 类型 | 说明 |
|---|---|---|
| `id` | INTEGER PK | |
| `event_type` | TEXT | `new_file` / `hash_match` / `moved` / `deleted` / `conflict` |
| `file_id` | INTEGER FK NULL | |
| `detail` | TEXT NULL | JSON 存额外信息 |
| `created_at` | DATETIME | |

V1 简单用，V2 可以做时间线/活动流。

### 表 `app_setting`（应用配置）

| 字段 | 类型 | 说明 |
|---|---|---|
| `key` | TEXT PK | |
| `value` | TEXT | |
| `updated_at` | DATETIME | |

存：HTTP 服务端口、扫描间隔、是否自动识别等。

## 核心工作流

### 入库流程

```
文件进入 doujinshi/
       │
       ▼
   notify 触发 / 手动扫描
       │
       ▼
  1. 计算 BLAKE3（流式，大文件不爆内存）
       │
       ▼
  2. 查 hash 是否已存在？
       │
       ├─ 存在 → 更新 filename_alias + doujinshi_file.filename 和 current_path
       │         current_location 不动（保留原值）
       │         → 结束
       │
       └─ 不存在 ↓
       │
  3. 解析文件名 → 拆出 circle/title/series/translator/version_tag
       │
       ▼
  4. 查 (filename+ext) 在 identified 里有没有？
       │
       ├─ 有 → 写 conflict 表
       │       → 文件留在 inbox
       │       → 写 scan_event（conflict）
       │       → emit('conflict-detected') → 结束
       │
       └─ 没有 ↓
       │
  5. 提取封面（见下方"封面提取规则"）
       │
       ▼
  6. 移动文件到 doujinshi-identified/
       │
       ▼
  7. 写 doujinshi_file（current_location='identified'）
       │
       ▼
  8. 写 scan_event（new_file）
       │
       ▼
  9. Tauri 事件 emit('file-added') → 前端刷新
```

### 删除流程（双确认）

```
用户在 Library 列表点"删除"
       │
       ▼
  弹窗 A（普通确认）：是否标为待删除？
       │
       ├─ 取消 → 结束
       │
       └─ 确认 ↓
       │
  marked_for_delete = true
  写 scan_event（mark_for_delete）
       │
       ▼
  弹窗 B（二次确认，物理不同位置 —— 确认按钮位于弹窗右下角）：
  ┌─────────────────────────────────┐
  │  确认要移到待删除区吗？            │
  │                                  │
  │  文件：xxx.zip (12.3MB)           │
  │  位置：→ doujinshi-will-delete   │
  │                                  │
  │  [取消]              [移到待删除] │  ← 确认按钮在右下角
  └─────────────────────────────────┘
       │
       ├─ 取消 → marked_for_delete = false → 结束
       │
       └─ "移到待删除"（点击右下角确认按钮）↓
       │
  移动文件到 doujinshi-will-delete/
  current_location = 'will_delete'
  current_path = 相对新位置
  写 scan_event（moved）
       │
       ▼
  emit('file-moved')
```

**为什么两次确认**：第一次是"标删除意图"（可恢复，标错了改回去），第二次才是"物理移动"（不可恢复的前奏）。最终物理删除在回收站页单独操作。

### 还原流程（从 will_delete 收回）

```
用户在 Recycle Bin 点"还原"
       │
       ▼
  弹窗（单确认，左下角）：
  ┌─────────────────────────────────┐
  │  还原文件到已识别库？              │
  │                                  │
  │  文件：xxx.zip (12.3MB)           │
  │  位置：→ doujinshi-identified    │
  │  [还原]              [取消]      │  ← 主操作在左下角，与删除错开
  └─────────────────────────────────┘
       │
       ├─ 取消 → 结束
       │
       └─ 还原 ↓
       │
  移动文件到 doujinshi-identified/
  current_location = 'identified'
  marked_for_delete = false
  写 scan_event（restore_from_recycle）
       │
       ▼
  emit('file-moved')
```

### 终删流程

```
用户在 Recycle Bin 页面点"永久删除"
       │
       ▼
  弹窗（终删警告，数据保留，右下角确认）：
  ┌─────────────────────────────────┐
  │  永久删除文件？                  │
  │  文件：xxx.zip (12.3MB)         │
  │  ⚠ 数据记录会保留                │
  │  [取消]       [永久删除]         │  ← 危险操作按钮在右下角，红色
  └─────────────────────────────────┘
       │
       ├─ 取消 → 结束
       │
       └─ 永久删除 ↓
       │
  删除 doujinshi-will-delete/xxx.zip
  physically_deleted = true
  current_path = (保留历史值，不变)
  写 scan_event（deleted）
       │
       ▼
  emit('file-removed')
```

## 命名解析（V1 简单版）

正则匹配常见模式：

```
[社团名 (作者)] 作品名 (系列名) [译者] [版本].zip
```

提取策略（按优先级）：
1. 第一个 `[...]` 块 → circle
2. 紧随其后的 `作品名` → title（到下一个 `(` 或 `[`）
3. 第一个 `(...)` 块 → series
4. 后续 `[...]` 块 → translator / version_tag
5. 拆不出来的，title = 完整文件名（去掉后缀）

V1 不会做语义级解析（不查 dlstation 之类），保持简单。

## 封面提取规则（精确）

**目标**：从 zip 中提取一张 ≤ 100KB 的 JPEG 作为缩略图。

**步骤**：
1. 用 `zip` crate 流式打开压缩包，列举 entry。
2. 筛选候选图片：
   - 扩展名 `jpg` / `jpeg` / `png` / `webp`（大小写不敏感）
   - **排除** `thumbnail` / `thumb` / `cover_small` / `sample` 关键词（避免选到制作方放的小预览图）
3. 候选优先级：
   - 1st：文件名包含 `cover` / `表紙` / `封面` 的
   - 2nd：文件名在压缩包中最靠前（按 zip entry 顺序）
   - 3rd：体积最大的（说明是高清封面）
4. 选定后解压到 `resources/covers/<hash>.jpg`（用 hash 命名避免冲突）：
   - 原图若是 JPEG：缩放到长边 800px，JPEG quality 75 起步；若 > 100KB 则降到 quality 60；最差降到 quality 40，但**不**降到长边 600px 以下
   - 原图若是 PNG/WebP：`image` crate 解码 → 转 RGB → 按上述 JPEG 参数输出
5. 失败兜底：解不出任何图片，`cover_path = NULL`，前端显示"无封面"占位
6. **不**支持多层 zip（嵌套压缩包），写到 `scan_event` 后跳过

**为什么是 100KB 不是 50KB**：浏览器扩展要拉，小一点加载快；100KB 在列表网格里画质和速度都够用。

## 前端页面

### LibraryView（默认页 / 已识别库）

```
┌─ 同人志管理 ────────────────────────────────────┐
│ 🔍 [搜索: title/circle/filename  ]   [筛选 ▾]  │
│ ┌─ 排序: 入库时间↓ ─ [显示已删除的] [未看过的]  │
│                                                  │
│ ┌──────┐ ┌──────┐ ┌──────┐ ┌──────┐ ┌──────┐  │
│ │封面  │ │封面  │ │封面  │ │封面  │ │封面  │  │
│ │      │ │      │ │      │ │      │ │      │  │
│ │✓看过 │ │🗑待删│ │      │ │      │ │      │  │
│ └──────┘ └──────┘ └──────┘ └──────┘ └──────┘  │
│ 負けヒロイン…  許婚のあかり…  もっと、チョロい…  │
│ MAD CAPSULE   お絵かきおじさん スタジオN.BALL     │
│ 12.3MB        8.9MB         18.0MB               │
└──────────────────────────────────────────────────┘
```

**功能**：
- 卡片网格（Naive `n-grid` + `n-card`），每张显示封面 + title + circle + size
- 卡片左下角小图标：`✓看过` / `🗑待删` / `❌已删`（叠加显示）
- 顶部搜索框（前端模糊匹配 title/circle/filename）
- 筛选：状态（全部/未看/已看/待删/已删）、后缀
- 点击卡片 → 弹 Detail Modal（V1 简化版：显示封面大图 + 元数据 + 标看过/标删除 按钮）

### InboxView（待识别）

```
┌─ 待识别 (3) ───────────────────────────────────┐
│ 等待用户处理的新文件                              │
│                                                  │
│ ⚠ 1 个冲突未处理                                │
│                                                  │
│ ┌────────┐ ┌────────┐ ┌────────┐                │
│ │ 封面    │ │ 封面    │ │ 封面    │                │
│ │        │ │  ⚠冲突  │ │        │                │
│ └────────┘ └────────┘ └────────┘                │
│ ファイル名.zip  ファイル名.zip  ファイル名.zip     │
│ 12.3MB        8.9MB        18.0MB                │
│ [跳过]        [⚠ 比对]      [跳过]              │
└──────────────────────────────────────────────────┘
```

**功能**：
- 这里展示的有两类：
  - **冲突未处理**：从 `conflict` 表查 `resolved=false` 的条目（V1 默认开封面图，V2 加对比页）
  - **扫描中 / 失败重试**：`scan_event.event_type IN ('conflict', ...)` 但未正常入库的文件
- 每张冲突卡片两个按钮：
  - `跳过`：标记 `conflict.resolved=true`（不删除文件，留在 inbox 让用户手动处理）
  - `比对`：跳到 `ConflictView`（V2 实现，V1 占位显示"建设中"）

### RecycleBinView（回收站）

```
┌─ 回收站 (5) ───────────────────────────────────┐
│ 这些文件已移到 will-delete，可永久删除             │
│ 数据记录会保留                                    │
│                                                  │
│ ┌─ 已物理删除 (3) ────────────────────────────┐ │
│ │ ┌────────┐ ┌────────┐ ┌────────┐           │ │
│ │ │ 占位    │ │ 占位    │ │ 占位    │           │ │
│ │ └────────┘ └────────┘ └────────┘           │ │
│ │ ファイル名.zip  ファイル名.zip  ファイル名.zip │ │
│ │ (已删，仅数据保留)                            │ │
│ └──────────────────────────────────────────────┘ │
│ ┌─ 待删 (2) ──────────────────────────────────┐ │
│ │ ┌────────┐ ┌────────┐                       │ │
│ │ │ 封面    │ │ 封面    │                       │ │
│ │ └────────┘ └────────┘                       │ │
│ │ ファイル名.zip  ファイル名.zip               │ │
│ │ [永久删除]      [还原]                       │ │
│ └──────────────────────────────────────────────┘ │
└──────────────────────────────────────────────────┘
```

**功能**：
- 上半：`physically_deleted=true` 的已删记录（封面显示"已删"占位图，可看元数据，V1 不提供重置/恢复文件功能）
- 下半：`physically_deleted=false AND current_location='will_delete'` 的待删文件
- 顶部：[清空所有待删] 按钮（二次确认，逐个删除）

### SettingsView（V1 简单版）

- HTTP API 端口（显示当前 + 复制 API 地址）
- 资源目录路径（显示绝对路径 + 在 Explorer 中打开）
- 自动扫描开关
- 关于页（版本、API 文档链接）

## HTTP API（给浏览器扩展用）

**Base URL**：`http://127.0.0.1:PORT`（V1 随机端口，V2 可在设置里改）
**CORS**：允许 `*`（本地工具，扩展用不到复杂 CORS 规则）

### V1 必须的端点

```
GET  /api/health
     → { status: "ok", version: "0.1.0" }

GET  /api/doujinshi/search
     Query: ?q=...&circle=...&status=...&limit=50&offset=0
     status 取值: viewed | not_viewed | marked | deleted | all(默认)
     → {
         items: [{
           id, title, circle, hash, ext, size_bytes,
           viewed, marked_for_delete, physically_deleted,
           current_location, cover_url
         }],
         total: 123
       }

GET  /api/doujinshi/by-hash/:hash
     → 单条 doujinshi 详情（同上 items 元素结构）
     → 404 if not found

GET  /api/doujinshi/:id
     → 同上（按 id 查）

GET  /api/covers/:file_id
     → 返回 JPEG 二进制（content-type: image/jpeg）
     → 404 if not found
     → 扩展可以直接 <img src="http://127.0.0.1:PORT/api/covers/123">
```

`cover_url` 字段返回相对路径如 `/api/covers/123`，前端组合域名。

### V2 计划

- `GET /api/doujinshi/check?hash=...` —— 浏览器扩展"我看到了这本，已下载过吗？"
- `POST /api/doujinshi/:id/viewed` —— 标记看过
- `GET /api/doujinshi/cover-by-hash/:hash` —— 通过 hash 直接拿封面（插件常用）

## Tauri 命令 vs HTTP API 的分工

| 调用方 | 用什么 |
|---|---|
| 前端 Vue | Tauri `invoke()` 命令（类型安全、零网络） |
| 浏览器扩展 | HTTP API（必须，因为扩展在 webview 之外） |
| 其他脚本 | HTTP API |

Tauri 命令层和 HTTP handler 层**共享 service 层**，避免逻辑重复。

## 数据流总览

```
   ┌────── 文件入 inbox ──────┐
   ▼                            │
[notify] ─→ [Scanner] ─→ [Hasher] ─→ [DB 查 hash]
                                   │
                       ┌───────────┴───────────┐
                       │                       │
                  命中 hash               未命中 hash
                       │                       │
                更新 alias+path        解析命名 → 查 (filename+ext)
                       │                       │
                       │              ┌────────┴────────┐
                       │              │                 │
                       │         命中 (识别中已存在)    无冲突
                       │              │                 │
                       │         写 conflict          提取封面
                       │         留在 inbox            │
                       │              │            移到 identified
                       │              │            写 file 表
                       │              │            emit('file-added')
                       └──────┬───────┘            │
                              ▼
                          结束
```

## 实施分阶段

| 阶段 | 内容 | 验收 |
|---|---|---|
| **V1 MVP** | 扫描、哈希、命名解析、入库、移动到 identified、冲突提示、回收站、HTTP API、Library/Inbox/Recycle 三个页面、封面提取 | 跑通完整流程：丢文件 → 自动入库 → 列表查看 → 标删 → 回收站终删 |
| **V2 完善** | 冲突对比页、详细观看页（图片预览）、搜索/标签、设置页、Tauri 打包 | 浏览器扩展可调 API |
| V3+ | 评分、笔记、相似度查找、批量操作 | — |

### V1 范围

- ✅ 后台扫描 + 手动扫描
- ✅ 哈希（BLAKE3）、命名解析（circle/title/series/translator/version）
- ✅ 自动移动到 identified；冲突不入库、留待前端处理
- ✅ 封面提取（按规则缩放/压缩到 ≤ 100KB JPEG）
- ✅ 删除流程：软删标记 → 移到 will-delete → 终删（数据保留）
- ✅ 还原流程（从 will-delete 收回 identified）
- ✅ Library / Inbox / Recycle 三个核心页面
- ✅ HTTP API：V1 列出的 5 个端点
- ❌ 冲突对比页（V2 加）
- ❌ 详细观看（V2 加）
- ❌ Tauri 打包（V2 加）

## 风险与待确认

| 项 | 风险 | 备注 |
|---|---|---|
| rar 支持 | `unrar` crate 受协议限制，纯 Rust 解压不完整 | V1 仅支持 zip 完整解压；rar 可读但不解压，V2 加 |
| 18 禁内容审核 | 用户自行管理，不引入审核逻辑 | 明确不做 |
| 浏览器扩展 API 安全 | 本地 127.0.0.1，理论上同机进程可访问 | V2 加 token 鉴权 |
| 大文件哈希 | 流式实现，单文件 1GB+ 应 < 5s | 已规划 |
| 封面图片格式 | zip 内可能是 png/jpg/webp，统一转 JPEG | 已规划 |
| 命名解析准确率 | 100% 解析不可能 | 解析失败则 title=原文件名 |
| HTTP 端口冲突 | 随机端口可能仍冲突 | V2 加端口占用检测 + 重试 |

## 验收标准（V1 完工时）

1. ✅ 拖一个 zip 到 `doujinshi/`，自动出现在 Library 页面
2. ✅ 复制同名不同 hash 的 zip 到 `doujinshi/`，不重复入库，Inbox 出现冲突条目
3. ✅ Library 点击删除 → 二次确认 → 文件移到 will-delete，Recycle 出现
4. ✅ Recycle 永久删除 → 文件消失，数据保留（搜索仍能找到）
5. ✅ Recycle 还原 → 文件回到 identified，Library 出现
6. ✅ 浏览器扩展能用 `GET /api/doujinshi/search?q=xxx` 拿到结果
7. ✅ `GET /api/covers/123` 返回封面 JPEG
8. ✅ 修改 zip 文件名后重新扫描，filename_alias 表加新别名，hash 不变
