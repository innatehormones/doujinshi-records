# 同人志档案 · 设计语言

> 目的：把项目的视觉/排版/组件约定沉淀为单一参考。新增页面、组件、状态色前先读这一篇，避免「靠直觉做」导致整体走样。
>
> 维护原则：**每个决策背后的理由写在 commit / spec 里**，本文件只描述「是什么」和「在哪里用」。改 token、改骨架前先讨论再改——一旦下游代码大量依赖，回滚成本极高。

---

## 1. 视觉定位

**一句话价值主张**：沉静的深色控制台，给管理个人收藏的人留出整页的工作区，不抢内容（封面 / 标题 / 文件名）的注意力。

**调性关键词**：可观测、克制、矿物感、双主题、信息密度高但不挤压。

**不是什么**：
- 不是「SaaS Dashboard 模板」——没有大数字 + KPI + 渐变 hero
- 不是「Wabi-sabi 出版物」——没有衬线大字 + 米色背景 + 摄影头图
- 不是「Material/Bootstrap 默认风」——button 全 pill 不是圆角 4px，sider 永久折叠不是展开抽屉

---

## 2. 设计系统：Token

所有视觉变量集中在 [`src/styles/tailwind.css`](../src/styles/tailwind.css) 的 `@theme` / `:root` / `[data-theme="light"]` 三块。改任何值先想「会破坏多少处」。

### 2.1 调色板

按「语义层」组织，不是按「色相」组织：

| 层级 | 暗色 | 亮色 | 用途 |
|---|---|---|---|
| **品牌强调** | `phosphor-green` `#3ecf8e` | 同 | primary 按钮、active 状态、链接激活色（CRT 磷光绿——项目元灵感） |
| 品牌 pressed | `mint-pulse` `#00c573` | `#009960` | primaryColorPressed / 链接 |
| 品牌 active 背景 | `forest-depth` `#1f4b37` | `#c6efd6` | active 菜单项背景 |
| **文字** 3 档 | `snow` `#fafafa` / `silver-mist` `#b4b4b4` / `smoke` `#898989` | `#121212` / `#4a4a4a` / `#6a6a6a` | 主 / 次 / 弱化 |
| 文字 disabled | `graphite` `#4d4d4d` | `#c4c4c4` | 不可交互 |
| **表面** 4 档 | `obsidian-deep` `#0d0d0d` / `obsidian` `#121212` / `card=obsidian` / `ash` `#242424` | `#f0f0f0` / `#fafafa` / `#ffffff` / `#f5f5f5` | embed / canvas / card / elevated |
| **边框** 2 档 | `charcoal` `#2e2e2e` / `slate` `#393939` | `#e5e5e5` / `#d4d4d4` | 默认 / hover / strong |
| **状态色** | `ember-orange` `#d97706` / `ember-red` `#d03050` / `archive-blue` `#5b8def` | 同 | 警告 / 危险 / 归档 |

**规则**：
- 文字色只用 snow / silver-mist / smoke 三档，不要硬编 hex
- 表面色按层级用，不要把 card 背景设成 ash（card 比 canvas 高一档，不升到 elevated）
- 状态色只给语义事件，**不要做装饰**：ember-red 只用于「销毁」类操作，不要拿来做 hover 高亮
- 强调色（phosphor-green）克制使用——primary 按钮、active 状态、链接；普通文字不要绿

### 2.2 字体

```
--font-ui:    "Inter", "Manrope", ui-sans-serif, system-ui, ...
--font-mono:  "JetBrains Mono", "Source Code Pro", ui-monospace, ...
```

- **UI 全文用 sans**（font-sans / font-ui 都是它）
- **Mono 用于**：
  - 路径、token、URL、文件名
  - 文件大小 / 计数（让数字一眼能扫）
  - code / pre / 表格 path 列
  - 状态栏 / 错误细节
  - Settings 页 api-table 的 METHOD 列（`GET` 字样 mono 化）
- 标题统一 `font-weight: 500`——**不用 700 bold**。「加粗靠字号和颜色，不靠字重」是项目硬性约定

### 2.3 字号 / 行高 / 间距

5 档字号 + 5 档行高一一对应：

| token | px | 行高 | 用途 |
|---|---|---|---|
| `text-caption` | 12 | 1.5 | 描述 / 标签 / 表头 / 副标 |
| `text-body-sm` | 14 | 1.43 | 正文（也是 base 字号） |
| `text-body` | 16 | 1.5 | 备用 |
| `text-subheading` | 18 | 1.38 | h2 子标题 |
| `text-heading-sm` | 24 | 1.33 | h1 页面标题 |
| `text-heading` | 36 | 1.2 | 备用 hero |

**间距 token**（只在 `tailwind.css` 的 `:root` 定义，不走 Tailwind 默认尺度）：
`8 / 16 / 24 / 32 / 40 / 48 / 64 / 80`（页内 gap / card 内 padding 用这些）

### 2.4 圆角 4 档

| token | px | 用在哪 |
|---|---|---|
| `radius-tags` | 9999 | 所有 n-tag、pill 按钮、状态 badge |
| `radius-cards` | 16 | 卡片（FileCard / settings-card / 描述卡片） |
| `radius-inputs` | 8 | input / select / token-display / api-base 容器 |
| `radius-buttons` | 9999 | 所有按钮 |

icon 按钮不走上面 4 档，自带 6~8px 圆角（[FileCard 28×28 inline btn]、[sider 40×40]、[settings path-act 28×28]）。

### 2.5 阴影

只 1 个：`shadow-sm: 0 4px 6px -1px rgb(0 0 0 / 10%), 0 2px 4px -2px rgb(0 0 0 / 10%)`。
**默认不投影**——卡片靠 1px 边框 + 颜色对比分层，不靠 box-shadow。亮色主题把阴影减弱到 6% / 4%。

---

## 3. 主题系统

三档：**system / light / dark**。Sider 右下角图标按钮循环切换。

实现：CSS 变量双轨制
- **`tailwind.css`**：`--color-*` / `--surface-*` 在 `:root` 和 `:root[data-theme="light"]` 重新赋值（响应 `<html data-theme>` 切换）
- **`src/styles/theme-overrides.ts`**：`buildThemeOverrides(isDark)` 给 Naive UI 组件库注入同名 token（响应 `useThemeStore.isDark` 计算）

**为什么双轨**：Naive UI 组件（NCard / NInput / NMenu / NTag）的颜色走 JS 注入 token，不读 CSS 变量。两边必须保持同步。

**改色板前必做**：同步改两处，并跑一遍所有 view（尤其 InboxView 的 n-alert + SettingsView 的 n-tag 状态色映射）。

---

## 4. 应用骨架

### 4.1 Shell（[`src/App.vue`](../src/App.vue)）

```
┌──┬───────────────────────────────────────┐
│  │                                       │
│ 📚│                                       │
│ 📥│            RouterView                │
│ 🗑│            (page content)             │
│ ⚠ │                                       │
│  │                                       │
│  │                                       │
│ ⏏ │                              [Toast] │
│ ⚙ │                                       │
└──┴───────────────────────────────────────┘
 ↑                                                ↑
 64px 永久折叠 sider                  按需浮现的浮窗
```

- **Sider**：64px 宽，**永久折叠**（无展开 / 无菜单名）。
- **菜单图标**：`Library / Inbox / Recycle / AlertTriangle`——图标自身就是入口，菜单名是噪音（App.vue 注释里写死的硬性约定）
- **Sider 底部**：主题切换 + 设置入口两个 icon button，**绝对定位**贴底 12px（避免菜单多时被挤变形）
- **active 状态**：`is-active` class → 文字 phosphor-green + 背景 forest-depth
- **Content**：右侧铺满，`overflow-y: auto`，背景 surface-canvas

### 4.2 Page 容器（`.page`）

定义在 `tailwind.css` 的 `@layer components`：
```css
.page {
  display: flex;
  min-height: 100%;
  flex-direction: column;
  gap: var(--spacing-24);
  padding: var(--page-pad-y) var(--page-pad-x); /* 24px 32px */
}
```

所有 view 的根都是 `.page`。不要改成 max-width 居中——内容驱动型页面要全宽铺。

### 4.3 Page 头部

```html
<header class="flex items-baseline justify-between gap-4">
  <h1 class="text-heading-sm font-medium text-snow tracking-body">我的同人志</h1>
  <span class="font-mono text-caption text-smoke tracking-[0.1em]">共 1324 条</span>
</header>
```

- 标题 h1 24px / font-medium
- 副标 mono 12px smoke，跟标题之间用「 · 」间隔，**永远靠右**（justify-between）
- 副标放「条目总数」「section 名」「待处理数」——任何能一眼读到状态信息的字
- 副标允许替换：SettingsView 是「运行时 · 数据 · 外部集成」；InboxView 是「共 N 个待处理」

---

## 5. 页面模式

项目只有两种页面骨架。**新增 view 时先决定是哪种**。

### 5.1 内容驱动页（Library / Inbox / Dirty / Recycle）

适合**「列表本身就是页面」**的场景。

```
[Header: h1 + 副标]
[描述卡片]                          ← 银雾色 12px 一段话解释场景
[Filter Bar]                        ← Library 有（搜索 + status 下拉 + circle chips）
[H2: 子标题 + 右侧操作按钮]         ← 比如「待处理冲突 (3)」+ 封面开关
[List / Grid]
[Pagination]                        ← 总数 > pageSize 时
```

**描述卡片**（统一）：
```html
<div class="rounded-cards border border-border bg-card px-5 py-4">
  <p class="text-caption leading-[1.5] text-silver-mist">
    一段话告诉用户这个页面管什么、什么算正常、什么时候该介入。
  </p>
</div>
```

**列表两种呈现**：
- **网格**（Library）：`grid-cols-4 lg:5 xl:6 2xl:7 3xl:8 gap-5`，FileCard
- **横向 article 流**（Inbox/Dirty/Recycle）：`flex flex-col gap-3`，article 单行横排

### 5.2 卡片分组页（Settings）

适合**「按功能域配置，不是浏览数据」**的场景。

```
[Header: h1 + 副标「运行时 · 数据 · 外部集成」]
[Section: 运行时]
  [Card: HTTP 端口]
  [Card: HTTP Token]
  ...
[Section: 数据]
  [Card: 资源目录]
  [Card: 数据备份]
[Section: 外部集成]
  [Card: HTTP API]
```

每个 section 之间 `gap: 32px`（不是 24px），让 section 之间明显分隔。

---

## 6. 组件系统

### 6.1 SettingsCard（settings-card）

骨架统一为：

```
┌─ settings-card ─────────────────────┐
│ settings-card-body                  │
│  ┌─ settings-card-head ───────────┐ │
│  │ h3.settings-card-title        │ │
│  │ <n-tag 或 n-button>   ← 右侧 │ │
│  └────────────────────────────────┘ │
│  p.settings-card-desc              │ ← 12px 银雾色
│  .settings-card-controls            │ ← control-row 字段
│  .settings-card-actions             │ ← padding-top: 4px 分隔
└─────────────────────────────────────┘
```

CSS 在 `SettingsView.vue` 的 scoped style，**不抽全局**——只这一处用，不污染其他 view。

### 6.2 FileCard（[`src/components/FileCard.vue`](../src/components/FileCard.vue)）

`aspect-[3/4]` 封面 + 标签行 + 标题 + 社团/大小 + 行动按钮。

**3:4 比例是硬性约定**——同人志本身是这个比例，封面图失真比小难察觉。

**右上角小 badge**（size-5 圆形 + 边框 + obsidian/85 背景 + backdrop-blur）：
- recycle → Trash2 图标，ember-orange
- archived → Archive 图标，archive-blue
- deleted → X 图标，ember-red
- file_state ≠ present → AlertCircle，ember-red

**底部行动按钮**：pill 风格（9999px），根据 status 条件渲染不同组（`v-if="file.status === 'in_library'"` 等 4 个分支）。
- in_library：归档（中性）+ 回收（ember-red 边框 + 文字，hover 红底 8%）
- recycle：取回（中性）+ 销毁（仅 file_state=present 时，ember-red 同上）
- archived / deleted：取回 / 恢复（中性）

**危险操作包 n-popconfirm 二次确认**——文案必须具体到「对哪条数据做什么」，例如「把《{{title}}》移到回收站？随时可在回收站页取回。」

### 6.3 横向 Article（Inbox / Dirty / Recycle 列表项）

```html
<article class="flex items-start gap-4 rounded-cards border border-border bg-card p-4">
  <img class="size-16 shrink-0 ..." />          <!-- 可选，封面开关 -->
  <div class="flex min-w-0 flex-1 flex-col gap-1.5">  <!-- 文字块，truncate 友好 -->
    <div class="flex items-center gap-2">
      <n-tag size="small">入库目录</n-tag>
      <n-tag size="small" type="warning">孤儿文件</n-tag>
      <span class="font-mono text-caption text-smoke">12.3 MB</span>
      <span class="ml-auto font-mono text-[11px] text-smoke">2026-07-17 19:42</span>
    </div>
    <div class="text-body-sm font-medium text-snow">{{ filename }}</div>
    <div class="text-caption text-silver-mist">辅助说明...</div>
  </div>
  <n-popconfirm>...</n-popconfirm>
</article>
```

**约定**：
- 封面 64×80（4:5，跟同人志方向一致）
- 文字块 `flex-1 min-w-0` 保证 truncate 不撑破
- 标签行「label label meta ··· 右对齐时间戳」是默认顺序
- 行动按钮靠右，pill 风格

### 6.4 按钮系统

| 用途 | 样式 | 例子 |
|---|---|---|
| 主操作 | `n-button type="primary"`（phosphor-green 实底） | 立即扫描、保存、确认解压 |
| 次操作 | 透明 + border-slate + hover border-graphite + hover bg-snow/4 | 跳过、刷新 |
| 危险 | 透明 + border-ember-red + text-ember-red + hover bg-ember-red/8 | 回收、销毁、删除快照 |
| 链接激活 | inline-flex + 文字 phosphor-green | 设置页里的复制按钮文字链 |
| icon 按钮（sider） | 40×40，透明，hover ash+border，active phosphor-green+forest-depth | 主题切换、设置入口 |
| icon 按钮（inline） | 28×28，hover ash+border | 封面开关、path 列表打开按钮 |
| 危险二次确认 | `n-popconfirm` 包 trigger 按钮 | 所有 ember-red 操作 |
| 保存类 | n-button type="primary"，放在 actions 区首位 | 保存配置 |
| 批量执行 | n-popconfirm 包 trigger | 立即备份、删除快照 |

**规则**：
- 按钮永远 pill（9999px），**不**用 4px 圆角
- 主操作一个 card 里只 1 个，**不要**主+次都是 primary
- 危险按钮永远包 popconfirm，**不要**只靠 hover 文字变红
- icon 按钮 28 vs 40 看上下文：sider 是 40、inline 是 28、列表里行动按钮里的图标是 13px 内嵌

### 6.5 Input / Select / Switch

- 圆角 8px
- 边框 1px charcoal，hover graphite，focus phosphor-green（**不带 box-shadow**——`boxShadowFocus: "0 0 0 0 transparent"`）
- placeholder smoke
- 高度 medium=36px（默认），small=32px（toolbar 内）

### 6.6 Token Display（Settings 页 HTTP Token 卡片内）

```css
.token-display {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 8px 12px;
  background: var(--color-obsidian-deep);
  border: 1px solid var(--surface-border);
  border-radius: 8px;
}
```

obsidian-deep 容器里 mono 字符串 + 内联复制按钮。**整个页面只有这一处需要深嵌背景**——其他都用 card/elevated。

### 6.7 全屏预览（FullscreenPreview）

`fixed inset-0 z-[1000]` + `bg-black/88`，左右导航按钮 `bg-white/8` hover `white/18`，右上关闭 `bg-white/12` hover `white/20`。

**关闭规则**：
- 右上 × 按钮：可以关
- Esc 键：可以关
- 点击遮罩空白：**不关**（避免误触）
- 输入框聚焦时 ← → 键：浏览器原生（光标移动），不翻页

---

## 7. 状态色语义

`src/lib/file-state.ts` 集中管理 status / file_state → tag type 的映射，所有 view 共享。**不要在 view 内联写 if-else 给 n-tag type**。

```ts
statusTagType(s)         // { in_library: 'success', archived: 'info', recycle: 'warning', deleted: 'error' }
fileStateTagType(s)      // { missing: 'warning', absent_confirmed: 'error' }
reasonTagType(reason)    // dirty_data reason → warning / default
```

**lib 单一权威**——改一处生效所有 view，理由统一。

### 颜色映射总表

| 概念 | 颜色 | 说明 |
|---|---|---|
| in_library（已入库） | success（绿） | 正常存在 |
| archived（已归档） | info / archive-blue | 不在主流程 |
| recycle（待删除） | warning / ember-orange | 用户已标记，等待最终决定 |
| deleted（已删） | error / ember-red | 已销毁，文件 absent_confirmed |
| file_state=missing | warning | 数据在盘上没了 |
| file_state=absent_confirmed | error | 用户确认删除 |
| orphan_file（脏数据） | warning | 入库目录里 DB 无对应行 |
| 冲突 | warning | B 端文件名撞 A 端 |
| rar-error | error / warning 按 kind | unrar_not_installed/extraction_failed → error，too_large → warning |
| API 测试错误 | error 红边框 | 弹窗内响应栏，不弹 toast |

---

## 8. 写作约定（命名 + 文案）

### 8.1 用户动词

按钮文案永远是动词 + 对象，**不要**用「Submit」「OK」「Cancel」这种通用词。

| 抽象 | 项目用词 |
|---|---|
| 归档 | 归档 |
| 移到回收站 | 回收（按钮）/ 移到回收站（确认文案） |
| 取回 | 取回 |
| 销毁 | 销毁（复合操作）/ 永久删除（按钮）/ 彻底清理（确认文案） |
| 删除（仅 DB 行） | 删除 |
| 恢复 status | 恢复 |
| 重新入库 | 重新入库 |
| 跳过冲突 | 跳过 |
| 内容比对 | 内容比对 |
| 立即扫描 | 立即扫描 |
| 立即备份 | 立即备份 |
| 保存配置 | 保存 |
| 重新生成 Token | 重新生成 |
| 取消 | 取消 |

**副标 / 标题用名词短语**：「待处理冲突 (3)」「脏数据条目 (12)」「RAR 处理失败 (2)」。

### 8.2 描述卡片（一段话告诉用户场景）

- 用「这是 / 这里 / 如果」开头，不要用「欢迎使用」「强大功能」开头
- 解释**这个页面管什么、什么算正常、什么时候该介入**
- 一段话，不分点；分点不如描述卡片 + 列表

### 8.3 占位 / 空状态文案

- Library 空：「还没有文件，把压缩包丢进 `resources/doujinshi/` 即可。」
- Inbox 空：「没有待处理冲突」
- Dirty 空：「无脏数据」
- Settings 备份空：「还没有备份。」

不写「暂无数据」「点击添加」这种通用模板。

### 8.4 时间展示

- 后端写 chrono::Utc RFC3339，前端展示成本地时刻
- Settings 快照：`YYYY-MM-DD HH:MM`（mono 12px smoke）
- Dirty 条目：`Date.toLocaleString()`（完整本地时间）
- 文件 mtime：**不带时区后缀**（让用户一看就是本地时刻，不被 +0800 干扰判断）

### 8.5 大小 / 数字

- 文件大小：`formatBytes()` 工具函数，1.2 MB / 832 KB / 12.3 MB（保留 1 位小数）
- 计数：「共 1324 条」（不要写「1,324」「1.3k」）

---

## 9. 明确避免

每个 AI 接手项目最容易踩的雷，提前写在前面：

### 9.1 「AI 味装饰」——硬性禁止

**第一版 SettingsView 出现过这些**，用户立刻指出，全部回滚。**永远不要再加回来**：

- 卡片左侧色条（accent border）
- section eyebrow 大写 + tracking 加宽（`SECTION 1`）
- decorative tag / pill（无信息量只装饰的）
- gradient background
- 数字大字 + 单位小字 + 渐变 accent 那种 hero stat

> 设计原则：「**结构是信息**，不是装饰。如果某个元素不编码内容信息，删掉它。」

### 9.2 「模板感」陷阱

不要走这 3 个默认方向（用户已经看过太多）：
1. 米色背景 + 高对比衬线大字 + terracotta accent
2. 近黑背景 + 单一亮 acid-green/朱红 accent
3. 报纸 broadsheet 风格 hairline rule + 0 圆角 + 多列

如果新页面非要在三者里选一个，先停下来想想：「这是项目本身决定的，还是我在偷懒？」

### 9.3 「颜色当 hover」

不要用「hover 时变 phosphor-green」做反馈。hover 用 ash 背景 + border 提升一档就够了。绿色保留给「已经激活」「完成」「成功」这类状态。

### 9.4 「Bold 字重加粗」

不要 `font-bold`。标题用 font-medium + 大字号，颜色 snow。**整站没有 700 weight**。

### 9.5 「Box-shadow 替代边框」

不要在卡片上堆 box-shadow 做层次。卡片靠 1px border + surface 颜色差分层。shadow-sm 仅在少量 popover / dropdown 用。

### 9.6 「讲一半的设计」

- 不要写「按颜色编码」却不写为什么这样编码
- 不要写「AI 智能推荐」这种 placeholder
- 任何占位文案都要问一句「用户在真实场景下到底看什么」
- 永远不要写 `TODO: 优化样式` / `placeholder text` 类注释

---

## 10. 响应式

桌面为主，**32px page padding 是硬性桌面基准**。Library 网格断点：

| 断点 | 列数 | 说明 |
|---|---|---|
| < lg | 4 | 默认 |
| ≥ lg（1024px） | 5 | |
| ≥ xl（1280px） | 6 | |
| ≥ 2xl（1536px） | 7 | |
| ≥ 3xl（1600px，**自定义断点**） | 8 | `tailwind.css` 加了 `--breakpoint-3xl: 100rem` |

其他 view 不做网格断点适配——左侧 64px sider + 单列卡片流，desktop only 没问题。

窗口缩小时**只做合理的最小值**，不做 mobile 适配——本项目是桌面管理工具。

---

## 11. 视觉一致性 checklist

新增页面 / 组件前对照检查：

- [ ] 根节点是 `<div class="page">`
- [ ] header 用 h1 24px font-medium + 副标 mono 12px smoke
- [ ] 描述卡片（如果需要）用了 `rounded-cards border border-border bg-card px-5 py-4`
- [ ] 子标题 h2 用 `text-subheading font-medium text-snow tracking-body`
- [ ] 卡片 / 列表项用 `rounded-cards border border-border bg-card p-4`
- [ ] 文字只用 snow / silver-mist / smoke 三档
- [ ] mono 字体只用在：路径 / token / URL / 数字 / 代码 / METHOD 列
- [ ] 主操作按钮用 n-button primary，**全站只 1 个 primary per card**
- [ ] 危险按钮走 ember-red + n-popconfirm 二次确认
- [ ] 标签行按 `label label meta ··· right-aligned timestamp` 排列
- [ ] icon 按钮尺寸：sider 40 / inline 28 / 内嵌 13px
- [ ] 状态色映射走 `lib/file-state.ts`，不在 view 内联判断
- [ ] 主题切换后 token 显示一致（改色板要同步改 tailwind.css + theme-overrides.ts）
- [ ] 没有：色条、eyebrow uppercase、gradient、shadow 当层次、bold 700、placeholder 文案

---

## 12. 文件位置速查

| 想改的东西 | 改哪里 |
|---|---|
| 颜色 / 字体 / 字号 / 行高 / 间距 / 圆角 / 阴影 | [`src/styles/tailwind.css`](../src/styles/tailwind.css) `@theme` + `:root` + `[data-theme="light"]` |
| Naive UI 组件颜色（NCard / NMenu / NTag / NInput / ...） | [`src/styles/theme-overrides.ts`](../src/styles/theme-overrides.ts) `buildThemeOverrides()` |
| Sider 布局 / 主题切换按钮 | [`src/App.vue`](../src/App.vue) |
| Page 容器 / 全局基础样式 | `tailwind.css` 的 `@layer base` + `.page` |
| 列表内容驱动页模板 | [`src/views/InboxView.vue`](../src/views/InboxView.vue)（横向 article 流参考） / [`src/views/DirtyView.vue`](../src/views/DirtyView.vue) |
| 卡片分组页模板 | [`src/views/SettingsView.vue`](../src/views/SettingsView.vue) |
| 卡片组件 | [`src/components/FileCard.vue`](../src/components/FileCard.vue) |
| 弹窗模板（双栏 Request / Response） | [`src/components/ApiTestDialog.vue`](../src/components/ApiTestDialog.vue) |
| 全屏预览 | [`src/components/FullscreenPreview.vue`](../src/components/FullscreenPreview.vue) |
| status / file_state → tag type 映射 | [`src/lib/file-state.ts`](../src/lib/file-state.ts) |
| 字节 / 时间格式化 | [`src/lib/format.ts`](../src/lib/format.ts) |

---

## 13. 历史决策锚点

- **obsidian 黑底 + phosphor-green 强调**：项目初始色板，灵感来自终端 CRT 屏幕的磷光余晖。`#3ecf8e` 比标准 `#22c55e` 偏冷、偏荧光，跟 "可观测 / 沉静" 的调性匹配
- **64px 永久折叠 sider**：菜单只有 4 项，展开菜单名是噪音——直接图标当入口，让出横向空间给内容
- **Sider 底部主题切换 + 设置**：用户不会高频切主题，藏在底部不抢入口位；设置入口也压低，因为大多数用户配一次就不动
- **Settings 重设计「去 AI 味」**：第一版加了色条 + uppercase eyebrow + decorative tag，被用户当场指出。回滚后保留三个真实价值：字段统一 / 分组 / 内联文档
- **Inbox / Dirty / Recycle 用横向 article 而非网格**：列表项数通常 < 50，列出来比挤成网格更易扫；每条带 description / 路径 / 时间戳 / 按钮，横向排版刚好
- **FileCard 3:4 比例**：同人志本身是这个比例，封面缩略图不变形
- **危险操作 n-popconfirm 二次确认**：销毁是不可逆 IO，必须用户主动二次确认。**不**用 inline confirm / undo toast 替代
- **状态色 lib 集中**：每个 view 自己 if-else 给 tag type 会发散；统一在 `lib/file-state.ts` 里，理由（哪个 status 配哪个色）只在一处维护