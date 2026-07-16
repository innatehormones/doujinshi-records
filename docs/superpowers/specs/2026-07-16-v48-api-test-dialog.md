# Spec — HTTP API 测试弹窗（V4.8 增量）

> 日期：2026-07-16
> 状态：implemented
> 范围：**Settings 页 HTTP API 路由表加「测试」按钮**——点击打开弹窗，左侧 cURL 代码 + 参数输入、右侧响应结果
> 触发：用户 2026-07-16 提出「设置页面的『HTTP API』是否可以增加接口测试」，明确要求「只在弹窗上实现，方便拆分/移除」
> 前序：V4.7（设置页重设计，引入内联 HTTP API 路由表）+ 数据备份 spec（暴露 `auth_token` / `apiBase`）

## 背景

V4.7 改版后 Settings 页底部「外部集成」section 内联了 HTTP API 路由表（7 个 GET 端点），但**只展示不能测试**——用户想验证「这个 token 能否真调到 `/api/doujinshi/search`」就得切到 PowerShell / 浏览器 / 笔记本 App，不能在 app 内闭环。

V4.7 spec 在「不在范围内」明确写出"路由表与实现脱节"作为已知问题——本 spec 不解决自动生成路由表（build.rs 工作量大），但补上"打开就能测"的快速验证入口。

## 目标

- 在 HTTP API 路由表每行右侧加「测试」按钮
- 点击 → 打开全屏弹窗
- 弹窗**两栏布局**：
  - **左侧 Request**：路径参数输入（解析 `path` 里的 `<placeholder>` 和 query 里的 `<placeholder>`） + cURL 代码预览 + 复制按钮 + 发送按钮
  - **右侧 Response**：状态码 + 用时 + 响应体（JSON 自动 pretty-print）+ 错误信息（如有）
- 弹窗内请求直接走 `fetch()`，自动带上当前 `auth_token`；不依赖 Tauri invoke

## 决策汇总

| 决策点 | 选择 |
|---|---|
| 组件位置 | `src/components/ApiTestDialog.vue` 新文件；SettingsView 只 import + 渲染 + 暴露 `show` state |
| 移除成本 | 删 ApiTestDialog.vue + SettingsView 删 import / 模板一行 + 移除 api-table 的「操作」列 |
| 弹窗风格 | `n-modal` 全屏尺寸 + 标题 + 关闭 X，不带系统级 OK/Cancel 按钮 |
| 布局 | 桌面端左右两栏（≥ 800px）；窄屏自动堆叠 |
| cURL 字段 | method + url（含 apiBase 前缀） + `-H "Authorization: Bearer <token>"` |
| 是否自动发请求 | **不自动**——用户改完输入点「发送」才触发，避免误触 |
| 鉴权 | 自动用 `useSettingsStore().data.auth_token`，弹窗顶部小字提示「用当前 Token」；不提供 input 切换 |
| 路径 placeholder | 解析正则 `/<(\w+)>/g` 提取 `<id>` / `<hash>` / `<file_id>` / `<blake3>` 这些，渲染 n-input |
| query placeholder | 同上（`?hash=<blake3>` 拆出 blake3 输入） |
| 路径无 placeholder | 不显示输入栏，直接拼路径 |
| 响应展示 | `<n-code>` 包 pre，JSON pretty-print（2 空格缩进）；非 JSON 原始显示 |
| 错误处理 | `fetch` reject / 4xx / 5xx 都展示在响应栏（红色边框 + 错误文本），不弹 toast 打断 |
| 视觉复用 | 沿用 SettingsView 的 `.token-display` / `.api-base` 等样式（在 dialog scoped style 内复制，不引入全局 style 改动） |
| 性能 / 安全 | 纯浏览器 fetch 走 Tauri WebView 正常 HTTP；不暴露 token 到本地存储；关弹窗清空输入与响应 |

## 改动清单

### 1. `src/components/ApiTestDialog.vue` 新建

Props：
- `show: boolean`
- `method: string`（"GET" / "POST" 等，目前路由表都是 GET，但留扩展）
- `path: string`（带 `<placeholder>` 的原 path，与 api-table 行一致）

Internal state：
- `params: Record<string, string>` —— 用户输入
- `response: { status: number; ms: number; body: string; error?: string } | null`
- `sending: boolean`

Computed：
- `pathInputs` —— `[{name, label}, ...]`（从 path 提取的 path 参数）
- `queryInputs` —— `[{name, label}, ...]`（从 query 提取的 query 参数）
- `fullPath` —— 替换后路径（path 内 + query 内 placeholder 替换为 users 输入）
- `curlText` —— 多行 cURL（`-X METHOD` + url + `-H "Authorization: Bearer <token>"` + 可选 `-H "Content-Type: ..."` 占位）

Action：
- `send()` —— `fetch(apiBase + fullPath, { method, headers: { Authorization: `Bearer ${token}` } })` → 记录 status / 用时 / body / error

Template：
- 顶部关闭按钮
- 左栏：param inputs（n-form-item 风格）+ cURL `<n-code>` + 「复制 cURL」「发送」按钮
- 右栏：`n-empty`（未发） / 状态行 + `<n-code>`（body pretty-print）

### 2. `src/views/SettingsView.vue`

- import `ApiTestDialog`
- 引入 reactive state：`activeRoute` (null | {method, path}) + `showDialog` (boolean)
- api-table 加第四列「操作」，渲染 `<n-button size="tiny" @click="openTest(r)">测试</n-button>`
- 模板底部：`<api-test-dialog :show="showDialog" :method="activeRoute?.method ?? ''" :path="activeRoute?.path ?? ''" @update:show="showDialog = $event" @close="activeRoute = null" />`

## 关键文件清单

| 路径 | 改动 |
|---|---|
| `src/components/ApiTestDialog.vue` | 新建（dialog 主体） |
| `src/views/SettingsView.vue` | import + 状态 + 操作列 + 渲染 dialog |
| `docs/superpowers/specs/2026-07-16-v48-api-test-dialog.md` | 本 spec |

## 复用现有代码

- `useSettingsStore`（`stores/index.ts`）—— `apiBase` + `data.auth_token`
- `n-modal` / `n-empty` / `n-code` / `n-input` / `n-button` / `useMessage`（naive-ui）
- lucide icons：`X`（关闭）、`Play`（发送）、`ClipboardCopy`（复制）
- 现有 SettingsView 的卡片骨架 + token-display 样式（在 dialog scoped 内复制，不污染全局）

## 验证

### 手工 e2e

```bash
pnpm tauri dev
```

1. Settings 页底部「外部集成 → HTTP API」表格确认 7 行
2. 每行「操作」列有「测试」按钮
3. 点 `/api/doujinshi/search?q=...` → 弹窗打开，左栏出现 `q` 输入框，右栏空（提示「未发送」）
4. 输入 `同人志`、点「发送」→ 右栏出现 200 + JSON body + 用时
5. 点 `/api/doujinshi/by-hash/<hash>` → 出现 hash 输入框；空时点发送拿到 404 或 400；填一个真实 hash 拿到 200
6. 改 token 用「重新生成 Token」→ 重开 dialog 用旧 token 调 → 401 提示
7. 关掉弹窗 → 状态清空

### 类型检查

```bash
pnpm exec vue-tsc --noEmit
```

## 风险

- **Tauri WebView CORS**：后端开 CORS（CLAUDE.md 已声明）；同源 127.0.0.1 fetch 没跨域问题
- **token 暴露在 cURL 预览里**：弹窗自带，复制即分享；用户已知（这是诊断 / 复制工具），接受
- **大响应体卡 UI**：JSON pretty-print 拿 10MB 响应可能慢。**接受**：路由表内 7 个端点都返回轻量数据（封面图二进制走 `<img>` 不在此）；如有大响应再加分页
- **fetch 不会自动给 token 续期 / 失败**：用户重新生成 token 后需要重开弹窗让 internal state 重新读 store
- **增加桌面依赖**：使用内嵌项目已有的 naive-ui + lucide，无新依赖

## 不在范围内

- **自动生成路由表**（V4.7 「不在范围内」遗留）—— 仍需手维护 `apiRoutes` 列表
- **POST / PATCH / PUT**（路由表当前只有 GET）—— 组件预留 `method` prop；目前所有按钮都是 GET
- **Request body**（GET 无 body；POST 时本 spec 不实现，但 `<n-code>` 旁可扩展 textarea）
- **持久化测试历史**（每次开 dialog 清空）
- **多 endpoint batch 测试** / **Postman 替代品**

## 历史决策锚点

- 用户 2026-07-16 原话："设置页面的『HTTP API』，是否可以增加接口测试"—— 决定在 Settings 页内做
- "目前只在弹窗上实现这个功能，方便拆分，或者不要的时候移除这个功能"—— **决定单组件 + 极简接线**
- "目前也只是测试性质的功能"—— **不追求完整测试套件，能用就行**
- V4.7「路由表与实现脱节」风险—— 本 spec 不解决，仅提供"能测"快速入口
