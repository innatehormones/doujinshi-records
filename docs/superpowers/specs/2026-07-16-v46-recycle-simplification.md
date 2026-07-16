# Spec — 文件回收站页面只保留「待删除文件」（V4.6 增量）

> 日期：2026-07-16
> 状态：implemented
> 范围：**文件回收站（RecycleBinView）移除「已从硬盘删除」段**——只展示 `status='recycle' + file_state='present'`；`status='recycle' + file_state ∈ {missing, absent_confirmed}` 的对应记录走 Library status filter 找到
> 前序：V4（数据与文件解耦，引入 `file_state`） + 同一会话内的 sider 文案调整「回收站」→「文件回收站」

## 背景

V4 之后 `doujinshi_file` 同时持 `status`（4 值）和 `file_state`（3 态）。文件回收站页（`RecycleBinView`）原本按 `file_state` 把 `status='recycle'` 的行分两段：

- **待删除文件**：`file_state='present'` —— 文件还在 `doujinshi-will-delete/`，用户可还原 / 永久删除
- **已从硬盘删除**：`file_state ∈ {'missing', 'absent_confirmed'}` —— 文件不在盘上，仅「删除」让用户能从 DB 抹掉这条记录

这两段的 UI 差异是：present 段有「还原 / 删除」两按钮，gone 段只有「删除」（数据保留可还原 = status 切回 in_library 是 recovery 路径）。

V4.6 决定把 gone 段移除，让 RecycleBinView 退化成单段「待删除文件」。理由：

1. **gone 段的「删除」按钮含义模糊**——对 `file_state='missing'` 行点删除会触发 `permanent_delete_inner`（复合操作，best-effort 删文件），但文件已经不在了，操作等于「切 status=deleted」的单字段写。用户报告「以为是删文件结果是改名」是认知负担。
2. **Library status filter 已支持 `recycle` / `deleted` 项** —— 用户能在 Library 列出 `status='recycle'`（无论 file_state），按 file_state 角标区分 missing / absent_confirmed，行为可预测。
3. **gone 段当前 0 数据**：实际上线时没有用户真的进 gone 段操作；页面顶部「共 N 条」的 N 只算 present 段，gone 段要单独 N — UI 噪音。
4. **未删先决（V4 原则）**：「在数据里仍可见，只是在文件回收站页面拿掉一段入口」——不抹数据，不动 status，只收 UI。

## 目标

- `RecycleBinView` 只展示「待删除文件」（`status='recycle' + file_state='present'`）
- gone 段的行不消失，仍可在 Library 用 status filter（`recycle` / `deleted`）找到
- 后端 `list_recycle` 不再分段（只有一个 present 段），store / types 同步瘦身

## 决策汇总

| 决策点 | 选择 |
|---|---|
| gone 段数据 | **不删 DB 行**，只从前端分段展示移除；Library 用 status filter 仍可见 |
| gone 段 UI 入口 | 无（用户用 Library status filter 找到对应行） |
| 后端 `list_recycle` shape | `RecyclePage { present: Page<FileSummary> }` —— gone 段、gone_pager 全删除 |
| 分页逻辑 | 仅 `presentPage` / `presentTotal` / `showPresentPager`，与删除前一致 |
| 「删除」按钮文案 | 不变 —— `permanent_delete_inner` 仍是复合操作（status=deleted + best-effort 删文件 + file_state=absent_confirmed），对 present 行点 = 物理删文件 + 状态推到 deleted |
| sider 文案 | 「回收站」→「文件回收站」（同次改动），与「入库冲突处理」风格一致 + 与 Library chip 的「回收站」区分 |

## 改动清单

### 1. 后端：commands/recycle.rs

`RecyclePage` 结构去掉 `gone`：

```rust
/// V4.6：文件回收站首页专属 shape——只返回「待删除文件」段
/// （status='recycle' + file_state='present'）。原本按 file_state
/// 分 present/gone 两段，gone 段移除；对应记录可在 Library 用
/// status filter 找到。
#[derive(Serialize)]
pub struct RecyclePage {
    pub present: Page<FileSummary>,
}
```

`list_recycle` 签名去掉 `gone_limit` / `gone_offset`，handler 只查 present 并分页。

### 2. 前端 types/api.ts

```typescript
export interface RecyclePage {
  present: Page<FileSummary>
}
```

`Page<T>` 已在类型里定义。

### 3. 前端 api/tauri.ts

```typescript
listRecycle: (presentLimit?: number, presentOffset?: number) =>
  invoke<RecyclePage>("list_recycle", { presentLimit, presentOffset }),
```

### 4. 前端 stores/index.ts useRecycleStore

只保留 `present` / `presentTotal` / `presentPage` / `presentTotalPages` / `showPresentPager` / `load` / `gotoPresentPage` / `permanentDelete` / `restore` —— gone 相关字段全部删除。

`permanentDelete(id)` 实现：**先调后端复合操作，再用本地 filter 把 entry 从 `present` 移除 + `presentTotal--`** —— 避免下次 load 之前还显示在「待删除文件」。理由：后端推送 `status='deleted' + file_state='absent_confirmed'`，跟本签 `status='recycle' + file_state='present'` 过滤不匹配。

### 5. 前端 views/RecycleBinView.vue

按 `views/InboxView.vue` / `views/DirtyView.vue` 的横向 article list 风格重写：

```
[ header: "文件回收站" | "共 N 条" ]
[ 描述卡片: 这些文件已经移出已识别库，但仍占用硬盘空间…]
[ h2: "待删除文件 (N)" ]
[ n-spin: loading ]
  [ n-empty: "文件回收站为空。" ← store.loading && presentTotal === 0 ]
  [ article list: 每行展示 tag「回收」+ 标题 + 大小 / 社团 + hash / 文件名 + 「还原」「删除」按钮 ]
[ n-pagination: 仅当 presentTotalPages > 1 ]
[ PermanentDeleteDialog + RestoreDialog: popconfirm 替代 ]
```

骨架对齐 InboxView / DirtyView 三个卡片族（一致卡片边距 `rounded-cards border border-border bg-card p-4` + `flex flex-col gap-2` 列表容器）。

## 关键文件清单

| 路径 | 改动 |
|---|---|
| `src-tauri/src/commands/recycle.rs` | `RecyclePage` 去 gone 字段；`list_recycle` 签名去 `gone_limit` / `gone_offset` |
| `src/api/tauri.ts` | `listRecycle` 签名简化 |
| `src/types/api.ts` | `RecyclePage` 去 gone；注释更新到 V4.6 |
| `src/stores/index.ts` | `useRecycleStore` 去 gone 相关 ref / action |
| `src/views/RecycleBinView.vue` | 改横向 article list，与 InboxView / DirtyView 同款 |

## 复用现有代码

- `Page<T>` （`types/api.ts` 通用分页）
- InboxView / DirtyView 卡片骨架（vue template 模式）
- `permanent_delete_inner` / `restore_from_recycle_inner` 不变 —— 「删除」「还原」按钮行为一致

## 验证

### 后端

无新增测试 —— `list_recycle` 的 present 分页逻辑已经在 V4 spec 范围测过，本次只去 gone 字段。

### 端到端

```bash
pnpm tauri dev
```

1. 准备：Library 卡片点「移到回收站」把文件搬到 will_delete/ —— 文件回收站页面出现 1 条「待删除文件」
2. 文件被外部工具从 will_delete/ 删除 —— 重启 app —— 启动 `dirty_scanner` 把行 `file_state='missing'` —— **文件回收站页面该条消失**（gone 段移除生效）
3. Library 顶部 filter 切到「回收站」（status='recycle'）—— 仍能列出这条 missing 行，可恢复 / 永久删除
4. 切到「已删除」（status='deleted'）—— 列出所有已彻底删除行（status 流过该状态后）

## 风险

- **gone 段数据查找路径变化**：用户习惯进 RecycleBin 找 missing 行将不再直接可见，必须用 Library filter。**接受**：Library status filter 是 V4 已有的入口，文档补一句即可。
- **N 计数与过滤不一致**：`RecycleBinView` 的「共 N 条」只算 present 段。如果用户期待看到所有 recycle 总数，会困惑 —— 描述卡片明确「这里的文件已经移出已识别库」即可消歧。

## 不在范围内

- 不新增 `absent_confirmed` 行专用 UI 段 —— Library 已支持
- 不动 `permanent_delete_inner` 的复合操作语义
- 不动 sider 路由 / LibraryView chip（chip 文案保持「回收站」与 sider「文件回收站」区分 —— chip 是 filter label，sider 是页面名）
- 不动 `dirty_scanner.rs` —— 它继续按 V4 语义写 dirty_data + 改 file_state，与本 spec 独立

## 历史决策锚点

- V4「数据永生」原则 —— 本 spec 不删数据、不改 status，只去 UI 分段
- V4 「Library 用 status filter 区分 recycle / deleted」是 gone 段移除的前提
- 用户 2026-07-16 一句话决定："「文件回收站」页面，只保留「待删除文件」的逻辑吧，「已从硬盘删除」的逻辑移除"
