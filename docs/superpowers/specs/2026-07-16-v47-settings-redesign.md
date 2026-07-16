# Spec — 设置页重设计 + 备份快照展示 mtime（V4.7 增量）

> 日期：2026-07-16
> 状态：implemented
> 范围：**SettingsView 改版成运维控制台风（卡片 + 字段统一 + 操作一致）** + 备份快照列表补 `mtime` 列
> 触发：用户在 2026-07-16 通过 `/frontend-design` 反映设置页内容排版零散、字段布局不一致、操作细节不统一

## 背景

Settings 页（`src/views/SettingsView.vue`）原本把所有可配置项堆在一个 flex-col 容器里：每个项一个 `n-card`，卡内是「标题 + 描述 + 控件 + 操作」自由拼接——同类控件（备份目录 / HTTP 端口）的字段排版两套，与 LibraryView / InboxView / DirtyView 的「横向 article 列表」调性脱节。

### 用户反馈（2026-07-16）

> "优化「设置」页面的内容排版、布局，统一操作细节"

具体痛点：

1. **字段布局不一致**：备份目录用 label-input 横排（垂直居中），HTTP 端口却把 input + switch + 描述竖排——用户切换视线后发现"原来不是这样的"
2. **操作分散**：HTTP Token 的「重新生成」嵌在控件区，「复制」按钮混在 path-list；用户期望 ops 区独立且一致
3. **运行时数据长成一片**：5 个目录（资源根 / 4 个数据目录 / 封面缓存）的 key-value 列表无视觉分组
4. **HTTP API 没有内联文档**：路由散在侧栏代码，用户得切到 `src-tauri/src/http/api.rs` 才能看清有哪些端点

### 调性参考

- LibraryView / InboxView / DirtyView / RecycleBinView 已统一为「页面标题 + 描述卡片 + h2 文章列表 + 横向 article + tag + 操作按钮」骨架
- Settings 页是同样的"内容驱动"页，但功能不是列表而是配置——应该采用「**分组 + 卡片字段统一**」风格

## 目标

- Settings 页按功能分成 **运行时 / 数据 / 外部集成** 三大组（section eyebrow）
- 每组内多个 card，卡片骨架统一为「head（标题 + tag）+ desc + 控件区 + 操作区」
- 字段布局统一：所有需要「label + 控件」的项用 `.control-row`（flex row + align center + label + 控件），不再"控件 / 控件 / label"竖排
- 操作按钮统一：危险操作（删除 / 重新生成 / 立即备份）用 `n-popconfirm` 二次确认；保存类（保存配置）放操作区首位
- HTTP API 内联文档：路由表 + Bearer token 说明 + 接口地址（带复制按钮）
- 备份快照列表补 `mtime` 列（RFC3339 → 本地 YYYY-MM-DD HH:MM）

## 决策汇总

| 决策点 | 选择 |
|---|---|
| 分组方式 | 运行时（端口 / Token / Inbox / 手动扫描）/ 数据（资源目录 / 数据备份）/ 外部集成（HTTP API）—— 按"它影响什么"分，不是按"它是什么类型"分 |
| Card 骨架 | `.settings-card`（背景 + 1px 边框 + 圆角） + `.settings-card-body`（padding + flex-col gap）+ head（标题 + tag） + desc + controls（控件区） + actions（操作区） |
| 控件行布局 | `.control-row`（flex row + align center + gap 12px），label 64px min-width |
| 操作区位置 | 永远放在 desc / controls 下方，与 controls 用 padding-top: 4px 视觉分隔 |
| 危险操作 | `n-popconfirm` 二次确认（重新生成 Token / 删除快照 / 立即备份）|
| HTTP Token 显示 | 专用 `.token-display` 容器（obsidian-deep 背景 + 边框 + 圆角），内含 `Token` label + mono value (flex 1) + 内联「复制」按钮；「重新生成」放 action 区 |
| HTTP API 文档 | 内联 `<table>`：METHOD / PATH / 描述三列；附「接口地址」容器（带复制按钮）方便用户直接用 |
| 备份快照列表 | grid 4 列：路径 / 时间 / 大小 / 操作；路径超长 tooltip |
| `mtime` 展示 | RFC3339 UTC → 本地时刻 `YYYY-MM-DD HH:MM`，用户判断"是不是我想要的那份" |
| **去 AI 味** | 第一版有"左侧色条 + section eyebrow uppercase + decorative tag"被用户指出"AI 味"，已全部回滚——保留字段统一 / 分组 / 内联文档三个真实价值 |

## 改动清单

### 1. SettingsView.vue 重写

按 `运行时 → 数据 → 外部集成` 三组划分。Vue 模板骨架：

```
<header page title + 副标: "运行时 · 数据 · 外部集成">
  <section 运行时>
    <h2>运行时</h2>
    <article settings-card>HTTP 端口</article>
    <article settings-card>HTTP Token</article>
    <article settings-card>Inbox 目录</article>
    <article settings-card>手动扫描</article>
  </section>
  <section 数据>
    <h2>数据</h2>
    <article settings-card>资源目录</article>
    <article settings-card>数据备份</article>
  </section>
  <section 外部集成>
    <h2>外部集成</h2>
    <article settings-card>HTTP API</article>
  </section>
```

### 2. `.control-row` 字段统一

```css
.control-row {
  display: flex;
  align-items: center;
  gap: 12px;
}
.control-label {
  font-size: var(--text-caption);
  color: var(--color-smoke);
  min-width: 64px;
}
.control-input {
  flex: 1;
  min-width: 0;
}
```

用于：

- 备份目录（label "备份目录" + input placeholder "默认 resources/backups/"）
- 保留最近 N 个（label "保留最近" + n-input-number + 描述 "个快照（0 = 不限）"）
- HTTP 端口（n-input-number + n-switch + 状态描述）—— 这块没有 label，因为 input/switch/状态三者本身就是表单

### 3. `.token-display` 容器

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

参考 minimax 官网「订阅 Key」展示风格——深色容器里 mono 字符串居中突出，复制按钮内置。

### 4. `.snapshot-list` 4 列网格

```css
.snapshot-head,
.snapshot-row {
  display: grid;
  grid-template-columns: 1fr 130px 70px 132px;
  /* 路径 / 时间 / 大小 / 操作 */
  align-items: center;
  gap: 12px;
  padding: 8px 12px;
}
```

### 5. `apiRoutes` computed + `.api-table`

```typescript
const apiRoutes = computed(() => [
  { method: "GET", path: "/api/health",                         note: "健康检查（无需 Token）" },
  { method: "GET", path: "/api/doujinshi/search?q=...",         note: "标题/社团/文件名模糊搜索（需 Token）" },
  { method: "GET", path: "/api/doujinshi/check?hash=<blake3>", note: "检查哈希是否在库（需 Token）" },
  { method: "GET", path: "/api/doujinshi/by-hash/<hash>",       note: "按哈希查询（需 Token）" },
  { method: "GET", path: "/api/doujinshi/<id>",                 note: "按 ID 查询（需 Token）" },
  { method: "GET", path: "/api/covers/by-hash/<hash>",          note: "按哈希取封面（需 Token）" },
  { method: "GET", path: "/api/covers/<file_id>",               note: "按 ID 取封面（需 Token）" },
])
```

`api-table` 与 `.snapshot-list` 共用头部背景 (`--color-ash`) + hover 行为。

### 6. `fmtMtime(iso)` 工具函数

```typescript
/// RFC3339 (UTC) → 本地时区 YYYY-MM-DD HH:MM。后端 chrono::Utc 写入，
/// 文件名也用 UTC 时刻；前端展示成本地时间让用户判断"是不是我想要的那份"。
function fmtMtime(iso: string): string {
  const d = new Date(iso)
  if (isNaN(d.getTime())) return iso
  const pad = (n: number) => String(n).padStart(2, "0")
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ` +
         `${pad(d.getHours())}:${pad(d.getMinutes())}`
}
```

`BackupSnapshot.mtime` 字段（`types/api.ts` 已定义）从不被显示升级到「和路径 / 大小并列」。

## 关键文件清单

| 路径 | 改动 |
|---|---|
| `src/views/SettingsView.vue` | 重写为运维控制台风：3 个 section + 7 张 card；统一骨架；HTTP API 内联文档；备份快照补 mtime |
| 已有 `src/api/tauri.ts` `getBackupConfig / setBackupConfig / listBackups / backupNow / stageRestore / deleteBackup` | 不变 |

## 复用现有代码

- `useSettingsStore.apiBase` —— HTTP API 文档的"接口地址"直接用
- `lucide-vue-next` 图标：`RefreshCw / ClipboardCopy / RotateCw / Play / Save / Trash2`
- `naive-ui` 组件：`NButton / NTag / NSpin / NCode / NInputNumber / NSwitch / NInput / NPopconfirm / NTooltip`
- `tokens` —— `var(--text-caption) / var(--color-smoke) / var(--surface-card) / var(--radius-cards)` 等

## 验证

```bash
pnpm tauri dev
```

肉眼检查（与 LibraryView / InboxView / DirtyView 调性是否对齐）：

1. HTTP 端口控件与"备份目录"控件**同款横排**（label + input + 描述）
2. HTTP Token 容器像一个完整的小组件，不再是「裸字符串 + 按钮」拼接
3. 数据 section 第一个 card 是「资源目录」→ 5 个 key-value path-list
4. 数据备份 card：底部 `snapshot-list` 4 列（路径 / 时间 / 大小 / 操作），时间显示 `2026-07-16 19:42` 这种本地时刻
5. 外部集成 section 「HTTP API」：表头 METHOD / PATH / 描述；hover 行变灰
6. 顶部 3 个 section 排序合理：先运行时（高频），再数据（低频），最后外部集成（一次配置）

### 类型检查

```bash
pnpm exec vue-tsc --noEmit
```

## 风险

- **去 AI 味不彻底**：用户第一轮已经指出"色条 / uppercase tracking / decorative tag"过于 AI 化，**本 spec 已全部回滚**——保留三个真实价值（字段统一 / 分组 / 内联文档）+ 与其他 view 同款（圆角 / 边框 / padding）。后续若用户再指出，按具体反馈回滚对应样式
- **HTTP API 路由表与实现脱节**：路由表硬编码在 `SettingsView.vue::apiRoutes`，新加路由时两处都要改。**接受**：路由表是给用户看的"快速参考"，不替代源码；TODO 注释建议后续从 `http/api.rs` 自动生成（如 extractor-macro）

## 不在范围内

- HTTP API 自动从 Rust 代码生成路由表（需要 build.rs / typestate，工作量大）
- 主题切换 UI（项目已有 useThemeStore 在 sider 里）
- 国际化 / 多语言（项目当前只有中文 UI）
- 设置导出 / 导入（用户当前需求没提）

## 历史决策锚点

- 用户 2026-07-16 第一句话："优化「设置」页面的内容排版、布局，统一操作细节"——确定范围是 UI 调优
- 用户第二句话（去 AI 味）：「这样一些样式很AI，比如很喜欢在卡片左侧加一个颜色条」—— **回滚装饰性元素，保留实用价值**
- 用户第三句话（HTTP 端口/Token 修复）：「HTTP Token 的 token 的地方，看起来有点单调」—— 引入 `.token-display` 容器，参考 minimax 官网"订阅 Key"风格
- 备份快照 mtime 缺失是用户主动提出的微调：「嗯……可以吧，加上，却是是觉得少了点什么」
