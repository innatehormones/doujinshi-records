# Spec — 同人志数据与文件解耦（V4）

> 日期：2026-07-15
> 状态：draft（待用户 review）
> 范围：**同人志业务状态与文件状态解耦**——状态机由"强一致"改成"DB 优先 + 文件 best-effort"
> 前序：V3（2026-07-11，加归档目录 + 脏数据 + webp 封面）+ v6（5 状态机折叠 physically_deleted）

## 背景

V3 的 5 状态机（`inbox / identified / will_delete / archived / permanently_deleted`）把"业务状态"与"文件存在性"绑死：

- **archive / restore / mark_for_delete** 三种转移要求源文件必须存在，否则 DB 也不动——`Err("physical file missing")`，HTTP 409
- **permanently_deleted** 是"终态"——只能从 will_delete / identified 进去，不能再切回
- **ReplaceB 冲突解决**为了让 A 行释放文件名，把 A 推进 permanently_deleted（= 终态）

业务上这带来几个别扭：

1. 用户拿走 archived 目录的文件做备份后，"取回"按钮会失败
2. recycle 里的同人志"彻底清理"后变成终态、不可恢复——但用户可能后悔
3. UI 上要分清"文件丢了"和"用户已经删了"两种情况，靠 `current_location='permanently_deleted'` 表达后者；但同时它又是终态

## 目标

把"同人志的业务状态"和"文件是否还在"拆开，让前者纯粹由人决定、后者作为观测值。

1. **同人志记录只增不减**——`doujinshi_file` 行永不 DELETE（保留 V3 数据永生原则）
2. **业务状态完全由人决定**——4 个值，任意可切回 `in_library`
3. **文件是附庸**——丢失不影响 status 变更
4. **状态切换永远成功**——文件搬运 best-effort，失败/冲突只反馈
5. **入库冲突由用户决策**——filename 撞名 / 同 hash 已存在走 conflict 表四选一（保留 V3）

## 非目标

- 不重新设计冲突 UI（沿用 keep_a / replace_b / keep_both / skip）
- 不重新设计扫描机制（仍是启动扫描一次）
- 不重写 HTTP API（仅调整字段语义）
- 不修改 inbox 自动识别流程

## 核心模型

### 字段重定义

| 新字段 | 类型 | 含义 | 来源（旧 → 新） |
|---|---|---|---|
| `status` | TEXT | 业务状态：`in_library / archived / recycle / deleted` | `current_location`（改名 + 4 值化） |
| `file_state` | TEXT | 文件状态：`present / missing / absent_confirmed` | `has_physical_file`（bool → 三态） |
| `last_seen_path` | TEXT | 最后一次确认文件存在的路径 | `current_path`（语义弱化，缺失时保留历史值） |

### `status` 业务状态语义

- **`in_library`**：入库态。正常浏览、预览（若 file_state=present）
- **`archived`**：归档。用户认可的长期保存对象；文件可在归档目录被人工拿走
- **`recycle`**：回收站。集中放文件待用户决定是否销毁
- **`deleted`**：已删除。人为确认无文件；记录仍在，可恢复回 `in_library`

**关键不变量**：4 个 status 之间没有任何"方向性"约束——任意 status 可手动切到任意 status（包括自己切自己，no-op）。这条不变量把"业务状态"与"文件目录"完全解耦。

### `file_state` 文件状态语义

- **`present`**：扫描/操作确认文件存在。可预览、可搬运、可抽取封面
- **`missing`**：扫描发现文件不在预期路径（外部误删、磁盘损坏、用户主动拿走等）。**仅提示，不阻塞任何 status/元数据操作**
- **`absent_confirmed`**：人为确认不存在（仅由"销毁"操作显式写入）

`file_state` 的写入路径限定为：
1. `dirty_scanner`：扫描后刷 `present` 或 `missing`（不写 `absent_confirmed`）
2. `state_machine` 状态切换搬运前检测：若 `last_seen_path` 不存在 → 写 `missing`（不写 `absent_confirmed`）
3. `permanent_delete`（"销毁"操作）：同时写 `status='deleted'` 和 `file_state='absent_confirmed'`

搬运过程失败（permission / disk full 等）时 `file_state` **保持原值**，不主动写 missing——因为搬运失败 ≠ 文件不存在，文件可能仍在原位置。

### `last_seen_path` 语义

- 当 `file_state=present` 时更新
- 当 `file_state` 变为 missing / absent_confirmed 时**保留历史值**（用于追溯）
- 状态切换时**只更新 status 和尝试搬文件**，不主动更新 last_seen_path；只有成功确认文件存在后才更新
- dirty_scanner 自愈时（期望目录找回同名文件）会更新

### 与 V3 的语义变化

| V3 | V4 | 说明 |
|---|---|---|
| `current_location='permanently_deleted'`（终态） | `status='deleted'`，可恢复 | deleted 不再是终态 |
| `has_physical_file=0`（bool） | `file_state='missing'/'absent_confirmed'` | 三态化，区分"扫描发现" vs "人为确认" |
| `current_path`（必指向存在的文件） | `last_seen_path`（历史值，文件可不在） | 语义弱化 |
| 转移要求源文件存在 | 转移永远成功，搬运 best-effort | 数据哲学变更 |

## 核心流程

### 入库（自动，唯一自动化流程）

```
scanner.scan_inbox_once → identifier.identify_file

1. 算 BLAKE3 hash（zip / rar 源文件本身）
2. hash 命中（doujinshi_file 表存在同 hash 行）：
   a. 行 status='in_library'：
      - 删 inbox 副本
      - 写 filename_alias
      - 更新 filename + updated_at（不动 last_seen_path，保留原位置）
   b. 行 status='archived' / 'recycle' / 'deleted'：
      - 调 reactivate_row：移 inbox 副本到 identified_dir + 写 filename_alias + UPDATE
      - status='in_library', file_state='present', last_seen_path=identified/<新 filename>
3. hash 未命中：
   a. parse filename
   b. (filename, ext) 撞名查询（仅在 status IN ('in_library','archived','recycle') 内查）：
      - 命中 → 写 conflict 表，文件留 inbox 等用户决策
   c. 无冲突：
      - 抽封面（zip 直读 / rar 走 unrar/7z）
      - 移 inbox 文件到 identified_dir/<filename or (copy)>
      - INSERT doujinshi_file 行（status='in_library', file_state='present', last_seen_path=identified/<...>）
```

唯一变化：`status='deleted'` 不参与撞名查询——deleted 是"已无文件"状态，不应占用 filename。

### 状态切换（手动，永远成功）

用户主动改 status。规则：

1. **永远成功**：HTTP 永远返 204 / DB 总是更新
2. **文件搬运（best-effort）**：
   - 若 `file_state='present'`：尝试 `rename(last_seen_path → 目标目录/<basename>)`
   - 跨设备：copy + remove 兜底
   - 目标目录有同名文件 → **视为孤儿（dirty_data）**，覆盖 + 写 `dirty_data(reason='overwritten_by_state_switch')`
   - 任何搬运失败 → **不阻塞**，status 照常更新，file_state 视情况设为 missing / 保持 present
3. **若 `file_state!='present'`**：跳过搬运动作，直接 UPDATE status

### 销毁（手动，关键复合操作）

```
状态切换到 deleted + 触发"销毁"语义：

1. UPDATE status='deleted', file_state='absent_confirmed', updated_at=now
2. best-effort remove_file(last_seen_path)（不存在也算成功）
3. 写一条 scan_event (event_type='destroyed')
4. preview_cache.invalidate(id)
```

文件存在与否都成功。完成后：
- UI 默认 Library 列表不可见（deleted 默认隐藏）
- 可在 Library 过滤 "status=deleted" 时访问
- 可手动恢复回 in_library（file_state 保持 absent_confirmed，无文件复活）

### 恢复（手动）

任意 status 可手动切回 `in_library`。规则：

- 走"状态切换"标准流程
- 不动 file_state（不"恢复"文件）

### 扫描（自动，启动一次）

dirty_scanner 启动时跑一遍，扫 `identified/` / `archived/` / `will_delete/`（**不扫 deleted**——无对应目录）：

```
对每个文件 {path}：
  在 dirty_data 查 {file_path} → 存在则 skip
  在 doujinshi_file 查 last_seen_path={path}：
    - 命中 → 行 file_state='present', 更新 last_seen_path（如有漂移）
    - 未命中 → INSERT dirty_data(reason='orphan_file')

对每行 doujinshi_file（status IN in_library/archived/recycle）：
  推算 expected_dir = status 对应的目录
  if last_seen_path 指向 expected_dir 内：
    if file exists → file_state='present'
    else → file_state='missing' + dirty_data(reason='db_row_file_missing')
  else:
    candidate = expected_dir + basename(last_seen_path)
    if candidate.exists():
      last_seen_path=candidate, file_state='present'
      旧 last_seen_path → dirty_data(reason='location_path_mismatch_resolved')
    else:
      file_state='missing' + dirty_data(reason='location_path_mismatch')
```

脏数据表 `dirty_data` 的 reason 扩展：
- 保留：`orphan_file`, `db_row_file_missing`, `location_path_mismatch`, `location_path_mismatch_resolved`
- 新增：`overwritten_by_state_switch`（状态切换时覆盖的孤儿文件；`detected_dir` 填"目标目录"，即 status 切换到的目录）

新增 reason 写入路径：
- `overwritten_by_state_switch` 由 `state_machine` 写，不在 dirty_scanner 里产生

## 冲突处理（保留 V3）

入库冲突走 `conflict` 表 + 用户四选一（`keep_a` / `replace_b` / `keep_both` / `skip`）。

**ReplaceB 改造**：
- A 行：`status='deleted'`, `file_state='absent_confirmed'`, best-effort remove A 文件
- B 走 identify_file 流程入库
- 不再使用 `permanently_deleted` 概念；`deleted` 只是普通 status，可恢复

**KeepA / Skip / KeepBoth**：不变。

## UI 规则

| status | Library 默认 | 可访问 | 可改 status | 可改元数据 | 可预览 |
|---|---|---|---|---|---|
| in_library | 显示 | 主列表 | ✓ | ✓ | 仅 file_state=present |
| archived | 显示 | 主列表 | ✓ | ✓ | 仅 file_state=present |
| recycle | 隐藏 | 专门回收站页（沿用 V3） | ✓ | ✓ | 仅 file_state=present |
| deleted | 隐藏 | Library 过滤 status=deleted | ✓ | ✓ | 仅 file_state=present |

**DetailView**：`file_state!='present'` 时显示"文件已丢失，无法预览"提示，不阻塞其他元数据展示。

**RecycleBinView**（沿用 V3）：显示 status='recycle' 的记录，支持取回 / 销毁。

**DeletedView**（**不新增独立页面**）：在 `LibraryView` 增加 status 过滤项（全部 / in_library / archived / recycle / deleted），用户切到 deleted 时 Library 列表只显示 deleted 记录，每条提供"恢复"按钮（→ in_library）。回收站页（RecycleBinView）仍仅显示 recycle。

## 数据库迁移（v8）

```sql
-- 1. 加 file_state 列（present/missing/absent_confirmed）
ALTER TABLE doujinshi_file ADD COLUMN file_state TEXT NOT NULL DEFAULT 'present';

-- 2. 字段重命名（SQLite 3.25+ 支持 RENAME COLUMN）
ALTER TABLE doujinshi_file RENAME COLUMN current_location TO status;
ALTER TABLE doujinshi_file RENAME COLUMN current_path TO last_seen_path;

-- 3. 数据迁移：permanently_deleted → deleted
UPDATE doujinshi_file SET status = 'deleted' WHERE status = 'permanently_deleted';

-- 4. 数据迁移：has_physical_file=0 → file_state='missing'
UPDATE doujinshi_file SET file_state = 'missing' WHERE has_physical_file = 0;

-- 5. has_physical_file 列保留作为冗余（不 drop，零成本回滚）
-- 后续版本可以 drop：

-- 6. schema_version 升级
UPDATE app_setting SET schema_version = 8;
```

迁移幂等性：
- `ALTER TABLE ADD COLUMN` 用 `pragma_table_info` 检查（沿用 V3 模式）
- `RENAME COLUMN` 用 `pragma_table_info` 检测重命名后字段不存在则跳过
- `UPDATE ... WHERE` 用 `WHERE` 条件天然幂等
- v8 标记写入 `app_setting.schema_version`

## 代码改造点

### 后端（Rust）

| 文件 | 改造 |
|---|---|
| `db/entities/doujinshi_file.rs` | 字段重命名 + `file_state` 列 |
| `db/migrations.rs` | 新增 v8 迁移 |
| `services/state_machine.rs` | 从"强一致"改成"DB 优先 + 文件 best-effort"；移除 `PermanentlyDelete`/`permanently_deleted` 路径；引入 `Transition` enum 的新语义 |
| `services/identifier.rs` | `reactivate_row` 适配新 status；collision check 排除 status='deleted' |
| `services/dirty_scanner.rs` | 4 状态目录扫描规则；新增 `overwritten_by_state_switch` reason |
| `commands/inbox.rs` | `resolve_conflict` ReplaceB 改用 `status='deleted'` + `file_state='absent_confirmed'` |
| `commands/recycle.rs` | `permanent_delete` 改写为"销毁"语义：status=deleted + file_state=absent_confirmed |
| `commands/library.rs` | `mark_for_delete` / `archive` / `restore` 用新 status 名；全部移除"源文件缺失则拒绝"护栏 |
| `models/file_summary.rs` | `current_location` → `status`；新增 `file_state` 字段 |
| `http/api.rs` | 路由语义调整；冲突处理响应；新增 status=deleted 列表端点（如有需要） |

### 前端（Vue + TS）

| 文件 | 改造 |
|---|---|
| `types/api.ts` | `FileSummary` 字段名调整；`current_location` → `status`；增加 `file_state`；新增 `ConflictAction` 不变 |
| `stores/index.ts` | `useLibraryStore` 增加 status 过滤（in_library/archived/recycle/deleted）；recycle 视图默认隐藏 recycle/deleted；library 显示默认 in_library + archived |
| `views/LibraryView.vue` | status 过滤 UI；deleted 状态以淡色显示 |
| `views/RecycleBinView.vue` | 沿用 V3，仅显示 status=recycle |
| `views/DetailView.vue` | `file_state!='present'` 时显示"文件已丢失"提示 |
| `views/InboxView.vue` | 不变（仍由后端 conflict 表驱动） |
| `views/ConflictView.vue` | 不变（仍走 keep_a/replace_b/keep_both/skip） |

## 测试

### 单元测试

- `state_machine::transition_with_dirs`：所有 status 转移在源文件缺失时仍成功（仅文件操作 no-op，DB status 仍更新）
- `state_machine::transition_with_dirs`：目标目录同名视孤儿覆盖 + dirty_data 写入
- `state_machine::transition_with_dirs`：跨设备 copy + remove 兜底
- `identifier::identify_file`：hash 命中流程（in_library/archived/recycle/deleted 四种 status 路径）
- `identifier::identify_file`：collision check 排除 status='deleted'
- `dirty_scanner`：4 状态目录扫描 + file_state 更新
- `conflict::resolve_conflict_inner`：ReplaceB 改写为 status=deleted

### 集成测试

- archive API 在源文件缺失时仍返 204 + status 更新
- recycle 销毁 → status=deleted + file_state=absent_confirmed + 文件不在
- deleted → 恢复 → status=in_library + file_state 保持 absent_confirmed
- conflict ReplaceB → A 行 status=deleted + B 入库成功
- 启动扫描：DB 行 file_state=present 但 last_seen_path 不存在 → file_state=missing

### 手动 E2E

- 入库 → 归档 → 拿走 archived 文件 → 重启 → file_state=missing + status 仍 archived + Library 仍可见 + 可改 status / 改元数据
- recycle → 销毁 → status=deleted + Library 过滤可见 + 恢复 → status=in_library + file_state=absent_confirmed
- inbox 同 hash 重新入库（archived / recycle / deleted 都试一遍）→ 复用行 + status=in_library + last_seen_path 更新
- 状态切换时目标目录有同名孤儿 → 自动覆盖 + dirty_data 新增 `overwritten_by_state_switch`
- 冲突 ReplaceB → A 变 deleted + A 文件不在 + B 入库

## 迁移策略

启动时跑 v8 迁移，幂等。`has_physical_file` 列保留作为冗余（不破坏回滚能力），后续版本可单独 drop。

用户**不需要**手动操作——`init_schema_versioned` 检测到旧库自动跑迁移。

## 风险

- **脏数据自愈时序**：dirty_scanner 启动扫描 + 用户同时操作 status 切换 → 极端时序下 file_state 可能短暂与磁盘不一致。单用户本地应用，可忽略。
- **状态切换过于自由**：用户在 UI 上乱点 status 会让 last_seen_path 频繁搬运 → IO 抖动。但 status 切换是用户主导的少量操作，无压力。
- **deleted 默认隐藏** → 用户可能"找不到"已删除记录。过滤可见性保证 + Library 加 status 过滤 UI 即可缓解。
- **`has_physical_file` 冗余**：保留作为迁移保险，但前端/后端都不再使用。可后续 drop。
- **HTTP API 字段语义变更（决策）**：本 spec **不保留向后兼容**——`/api/doujinshi/:id` 等端点的响应字段直接改为 `status` / `file_state` / `last_seen_path`；旧的 `current_location` / `has_physical_file` / `current_path` 字段名消失。浏览器扩展若有调用必须同步升级。如果未来要支持旧扩展兼容，应单独开一个 spec 讨论，本 spec 不涉及。

## 待澄清

无——所有决策已对齐。

## 历史决策锚点

- V3 spec 描述的 5 状态机 + 强一致转移 在本 spec 后作废
- `permanently_deleted` 终态语义在本 spec 后作废；新语义为 `deleted`（普通 status，可恢复）
- `has_physical_file` bool 字段在本 spec 后作废；新语义为 `file_state` 三态字段
- `current_path` 强约束语义在本 spec 后作废；新语义为 `last_seen_path`（历史值）