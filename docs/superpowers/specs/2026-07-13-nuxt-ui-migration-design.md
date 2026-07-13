# Nuxt UI v3 迁移设计稿

**日期**：2026-07-13
**范围**：全局 UI 重建（剔除 Naive UI，单 commit 一次性重写所有 view / component）
**业务逻辑**：零改动（store / composable / API 调用 / Tauri command / HTTP 路由全部保留）

## 动机

Naive UI 是纯组件库，缺乏明确的设计原则 / 设计语言。用户希望用有强设计语言的 UI 系统（Tailwind CSS 原子类 + Nuxt UI v3 的 design tokens）。同时 Tailwind 提供细粒度样式控制，便于后续响应式 / 动效扩展。

## 技术栈变更

| 类别 | 旧 | 新 |
|---|---|---|
| 组件库 | `naive-ui@^2.41` | `@nuxt/ui@^3` |
| 样式 | `vfonts@^0.0.3` | `tailwindcss@^4` + `@tailwindcss/vite@^4`（字体 fallback 由 Tailwind v4 `@theme` 配置系统字体） |
| 工具 | — | `@vueuse/core`（`useColorMode` 等） |
| 构建 | `@vitejs/plugin-vue` | 同上（无变化） |
| 框架 | Vue 3.5 + Vite 6 | 同上（**不**上 Nuxt 框架） |

## 配置改动

### `package.json`

新增依赖：

```json
{
  "@nuxt/ui": "^3.0.0",
  "tailwindcss": "^4.0.0",
  "@tailwindcss/vite": "^4.0.0",
  "@vueuse/core": "^11.0.0"
}
```

移除依赖：`naive-ui`、`vfonts`

### `vite.config.ts`

- 在 `plugins: [vue()]` 后追加 `ui()` 和 `tailwindcss()`
- `manualChunks` 函数：
  - 删 `id.includes("node_modules/naive-ui")` 分支
  - 加 `id.includes("node_modules/@nuxt/ui")` → `"nuxt-ui"`
  - 加 `id.includes("node_modules/reka-ui")` → `"reka-ui"`
  - `id.includes("node_modules/@vueuse")` 合并进 `vue-vendor`
- `chunkSizeWarningLimit` 从 1300 降到 800

### `main.ts`

- 去掉 `import naive from "naive-ui"` 和 `app.use(naive)`
- 加 `import ui from "@nuxt/ui"`，加 `app.use(ui)`
- 加 `import { useColorMode } from "@vueuse/core"`，初始化 `colorMode = useColorMode({ attribute: "class", modes: { light: "light", dark: "dark" } })`
- Tauri event 监听逻辑保持

### `styles/base.css` → `styles/main.css`

- 删 `vfonts` 的 `@import`
- 加 `@import "tailwindcss";`
- 加 `@import "@nuxt/ui";`
- 保留项目级自定义工具类（如有）

## 主题与 toggle 按钮

- **模式**：light / dark 二选一，跟随系统默认 + 手动 toggle 覆盖
- **位置**：`App.vue` 顶部 nav 栏右侧
- **组件**：`UButton` icon-only，`icon` 在 `i-lucide-sun` / `i-lucide-moon` 之间切换
- **行为**：点击 → `colorMode.preference = colorMode.value === "dark" ? "light" : "dark"`
- **持久化**：仅在 session 内有效，刷新回 `system`

## 组件映射表

| Naive UI | Nuxt UI v3 | 备注 |
|---|---|---|
| `NConfigProvider` + `NMessageProvider` + `NDialogProvider` + `NLoadingBarProvider` | `UApp`（单组件包 toast / modal / loading） | |
| `NButton` | `UButton` | icon / loading / block 同名 |
| `NCard` | `UCard` | header / body / footer 同名 |
| `NDataTable` | `UTable` | rowKey 字段略变 |
| `NImage` | `UImg` | preview 走现有 `usePreviewState` composable |
| `NTag` | `UBadge` | variant 配色调整 |
| `NInput` | `UInput` | |
| `NSelect` | `USelectMenu` | `v-model:value` → `v-model` |
| `NModal` | `UModal` | `v-model:show` → `v-model:open` |
| `NPopconfirm` | `UPopover` + 自定义按钮 | Nuxt UI 无内置 popconfirm |
| `NSpace` | `<div class="flex gap-x">` | Tailwind 替代 |
| `NLayout` / `NLayoutSider` | `<div class="flex h-screen">` | Tailwind 替代 |
| `NMenu` | `UTabs` | 顶部 nav 改 tabs |
| `NIcon`（renderIcon） | `UIcon name="i-lucide-xxx"` | Iconify 命名规则 |
| `useMessage()` | `useToast()` | API 略变 |
| `useDialog()` | 自维护 `ref<boolean>` + `UModal` | 无 dialog service |
| `NEmpty` | `UEmpty` | |
| `NSkeleton` | `USkeleton` | |

## 视图与组件迁移清单

### 视图（8 个）

1. **`App.vue`**：删 `NConfigProvider` 套壳，`UApp` 包整个 layout；顶部加 `UTabs` + light/dark toggle `UButton`
2. **`Library.vue`**：`NDataTable` → `UTable`；搜索框 → `UInput`；状态筛选 → `USelectMenu`
3. **`Detail.vue`**：缩略图栅格用 Tailwind grid；元数据表单用 `UInput` / `UTextarea`
4. **`Inbox.vue`**：冲突列表 + RAR error 提示
5. **`Conflict.vue` / `ConflictCompare.vue`**：对比视图，header 改 `UCard`，底部操作改 `UButton` group
6. **`RecycleBin.vue`**：分 present / gone 两组，`UTabs` 切换
7. **`Dirty.vue`**：`UTable`
8. **`Settings.vue`**：表单 + 按钮组

### 组件（4 个）

- `FileCard.vue`：卡片 + 状态 badge + 操作按钮
- `FullscreenPreview.vue`：`UModal` + 翻页按钮
- `PermanentDeleteDialog.vue` / `RestoreDialog.vue`：`UModal` + 确认逻辑

业务逻辑（store / composable / API 调用）**完全不动**。

## 测试与验证

1. `pnpm exec vue-tsc --noEmit` 必过
2. `pnpm tauri dev` 手动跑 8 个页面 + 4 个组件，肉眼检查：搜索、筛选、冲突解决、归档、回收站、脏数据、设置、缩略图全屏预览
3. light/dark toggle：点击切换 + 刷新页面回 system
4. `pnpm build` 通过 + 检查产物：
   - `naive-ui` chunk **不再出现**
   - `@nuxt/ui` / `reka-ui` chunk 按需拆出
   - 总体积应小于迁移前（Naive UI monolithic 1.2 MB → Nuxt UI tree-shake 后通常 < 800 KB）
5. 浏览器控制台无 404 / 警告残留 Naive UI class

## 风险与对策

| 风险 | 影响 | 对策 |
|---|---|---|
| Tailwind Preflight 与现有 `base.css` 冲突 | 样式错乱 | 迁移后跑 dev 肉眼检查 + 控制台报错扫一遍 |
| `chunkSizeWarningLimit` 改 800 仍超 | build 警告 | 进一步细分 manualChunks |
| Iconify 缺图标 | 部分图标显示不出 | 迁移前列出现有所有图标名，逐一查 lucide / heroicons 集 |
| `NPopconfirm` / `useDialog` 替换 | 组件内部需重写 | 提前列表，逐个改 |
| `naive-ui` 类型残留 | `vue-tsc` 报错 | 全局搜 `naive-ui` import，逐一替换 |
| `vfonts` 字体丢失 | 视觉差异 | Tailwind v4 `@theme` 配置 font-family 用系统默认 sans-serif stack，视觉差异在可接受范围 |

## 提交粒度

**单 commit 全部完成**（用户指定）。commit message：

```
refactor(ui): migrate from Naive UI to Nuxt UI v3 + Tailwind CSS v4

替换 UI 框架，建立基于 Tailwind 原子类的设计语言。业务逻辑零改动。

- 新增 @nuxt/ui@^3、tailwindcss@^4、@vueuse/core
- 移除 naive-ui、vfonts
- 8 views + 4 components 全部按 Nuxt UI v3 组件映射表重写
- App.vue 顶部加 light/dark toggle 按钮，跟随系统
- vite.config.ts: 加 ui() + tailwindcss() 插件，manualChunks 重拆
- styles/base.css → styles/main.css（Tailwind v4 入口）

验证：pnpm exec vue-tsc --noEmit + pnpm build + 手动 8 页遍历
```

## 不在范围

- 业务逻辑（state machine / scanner / dirty_scanner / preview_cache / HTTP）零改动
- 后端 Rust 代码零改动
- 数据库迁移零改动
- 响应式断点重设计（沿用 Tailwind 默认 sm/md/lg/xl）
- 自定义 design tokens / 主题色（用 Nuxt UI 默认 Emerald）
- 字体替换（用 Tailwind v4 系统字体 sans-serif stack，不再依赖 vfonts）