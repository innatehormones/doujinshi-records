# Spec — 列表封面显示开关

> 日期：2026-07-16
> 状态：implemented
> 范围：**InboxView「待处理冲突」+ RecycleBinView「待删除文件」列表加封面显示开关**——标题右侧 Image/Rows3 切换按钮，开启时每条卡片左侧渲染 64×80 缩略图
> 触发：用户 2026-07-16 提出「在标题『待处理冲突』『待删除文件』的右侧增加一个封面显示的切换按钮」
> 前序：同人志数据与文件解耦 spec（2026-07-15，FileSummary 带 cover_url）+ 文件回收站简化 spec（2026-07-16，RecycleBin 简化）

## 背景

两个列表页（Inbox / RecycleBin）默认只展示纯文字卡片——标题 + hash + filename + 按钮。但 Library 页用 FileCard 渲染 3:4 封面，卡片一眼能识别同人志。这两个列表是「出问题才看的页」（冲突 / 待删），列项数通常 < 50，加封面后视觉识别度会高很多——用户不用读 hash 前 12 位也能「哦这就是那本」。

## 目标

- Inbox / RecycleBin 列表项**标题右侧**加 Image/Rows3 切换按钮
- 开启时每条卡片**左侧**加 64×80 cover（横向布局，不改 card 高度）
- 待处理冲突只显示 **A 端封面**（已在库里的那本）——B 端还没入库，没封面
- 待删除文件直接显示该行 cover（FileSummary 自带 `cover_url`）
- 默认关闭，避免一上来 50 条全请求 HTTP 拉图

## 决策汇总

| 决策点 | 选择 |
|---|---|
| 脏数据页 | **不做**——`DirtyEntry` 没对应 doujinshi 行，要封面就得当场抽 zip/rar，IO 重且对纯路径条目无意义 |
| 待处理冲突显示哪一端 | 只 A（B 没入库没封面） |
| 切换按钮位置 | 标题右侧，紧贴「待删除文件 (N)」/「待处理冲突 (N)」字样 |
| 切换按钮图标 | lucide `Image`（关）↔ `Rows3`（开）——Image 暗示「我要看图」，Rows3 暗示「现在只显示文字行」 |
| 按钮尺寸 | 28×28 px，比 sider-icon-btn (40×40) 小一档——这里只是 inline toggle，不是主要操作 |
| 按钮样式 | 复用 sider-icon-btn 的色板：透明背景 / silver-mist / phosphor-green active / forest-depth active |
| 默认状态 | **关闭**——避免一打开就触发 24 张 HTTP 拉图 |
| 状态持久化 | **不持久**——`ref(false)` 本地状态，关页 / 刷新重置。这是看图辅助，不是用户偏好 |
| 卡片布局变化 | 横向：cover 64×80 → 文字 → 按钮列。`items-start` 顶对齐，card 高度基本不变 |
| cover 加载 | `<img loading="lazy">`——列表滚到才拉；HTTP 走 `useSettingsStore.apiBase + cover_url` |
| cover 缺失 | 待处理冲突：A 行 hash 空时 `a_cover_url = None`，前端不渲染 `<img>`，但保留占位空 div 保证行高对齐；待删除文件：FileSummary 没 cover_url 时同上 |
| 边缘 case：A 行不存在 | FK ON DELETE CASCADE 已在 schema 层挡掉——A 不在了 conflict 跟着删；代码里仍给 `a_cover_url = None` 兜底 |

## 改动清单

### 1. 后端 `src-tauri/src/commands/inbox.rs`

- `ConflictItem` 加 `a_cover_url: Option<String>`
- 抽出 `list_conflicts_inner(conn, limit, offset)` 方便测试
- A 行查询时同步拿 hash，hash 非空则拼 `format!("/api/covers/{}", m.hash)`，否则 None

### 2. 前端 `src/types/api.ts`

- `ConflictItem` 加 `a_cover_url: string | null`

### 3. 前端 `src/views/InboxView.vue`

- import `Image, Rows3` from `@lucide/vue`，import `useSettingsStore`
- `const showCover = ref(false)`
- 标题改成 `<div class="flex items-center gap-3">` 包 h2 + 按钮
- 卡片 `<article>` 加 `<img v-if="showCover && c.a_cover_url" :src="settings.apiBase + c.a_cover_url" loading="lazy" class="size-16 ...">` + 占位空 div
- `v-memo="[c.id, showCover]"`（showCover 进 memo 依赖，切换时触发重渲）
- scoped style `.cover-toggle` 28×28

### 4. 前端 `src/views/RecycleBinView.vue`

- 同样：import Image/Rows3 + settings store + showCover ref + 按钮 + 卡片左侧 `<img>` + scoped style

### 5. 测试 `src-tauri/tests/inbox_resolve.rs`

新增 2 个 case：

- `list_conflicts_populates_a_cover_url_from_a_hash`：A 行 hash 非空时 `a_cover_url` 是 `/api/covers/<hash>` 形式
- `list_conflicts_a_cover_url_is_none_when_a_hash_empty`：A 行 hash 为空时 `a_cover_url = None`（畸形数据防御，原 A 行被删 case 触发不到——FK CASCADE 已挡）

## 不在范围内（明确排除）

- 脏数据页封面：见上
- 待处理冲突显示 B 端封面：B 没入库，临时抽 cover 要走 archive::list + cover::pick_cover 一整套，对「决策要不要入库」的快路径来说太重
- 状态持久化（localStorage / pinia）：用户在哪个视图开了关了下次还得重新决定，开销不值得
- 缩放 / 多列网格：保持现有单列卡片流，只加左侧缩略图
- B 端缩略图占位：B 没封面时左侧不放占位，保持 card 紧凑

## 复用现有代码

- `useSettingsStore.apiBase` (`stores/index.ts:32`) — HTTP 绝对 URL 前缀
- `FileSummary.cover_url` (`models/file_summary.rs:58`) — 已是 `/api/covers/<hash>` 形式
- `http::api::cover` handler (`http/api.rs:94`) — `/api/covers/<hash>` 路由
- sider-icon-btn 色板（`App.vue:198-222`）：`color-silver-mist` / `color-snow` / `color-ash` / `color-phosphor-green` / `color-forest-depth`

## 验证

### 自动化

- `cargo test --test inbox_resolve` — 6/6 全过（原 4 + 新 2）
- `pnpm exec vue-tsc --noEmit` — 干净

### 端到端

1. 制造场景：inbox 丢一个会撞名的 zip → 重启 scanner → InboxView 出现 1 条「待处理冲突」
2. 默认状态下：标题右侧 Image 图标灰，卡片无封面
3. 点 Image → 按钮变 Rows3 + 绿高亮，每条卡片左侧出现 A 端封面
4. 切到 RecycleBin 页（独立组件，独立 ref）：默认又是关
5. 关 RecycleBin 的开关，回 Inbox（inbox 仍在路由历史内）——开关状态不串
6. 刷新整个 app → 开关回到默认关

## 影响面

- API 字段 `ConflictItem` 加 `a_cover_url` 是**非破坏性增量**——前端不读这字段也能跑（store 类型同步更新）。HTTP API 浏览器扩展如果解析 ConflictItem，新字段不影响老解析
- 后端 `list_conflicts` 没改 SQL / schema，只改了序列化字段——DB 迁移不需要
- 性能：默认关 = 零开销；开启时 24 张图 ≈ 24 个 HTTP GET 各 < 50ms（webp 已压到 ~100KB），首屏有 `loading="lazy"` 兜底

## 后续可能（不在本次）

- B 端封面：scanner 在 detect collision 时就把 B 抽一张临时缩略图到 `_conflict_previews/<b_path_hash>.webp`，list_conflicts 直接返回这条路径。需要新表 + 新 migration + scanner 多做一步 IO，且对「决策要不要入库」的快路径加成有限
- 网格 / 列数：根据 viewport 宽度自适应 2 / 3 / 4 列
- 持久化偏好：放 Settings 页「列表显示」section