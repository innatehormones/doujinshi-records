# V2 Sub-Plan 1 — 冲突对比页 ConflictView

> **For agentic workers:** REQUIRED SUB-SKILL: `superpowers:subagent-driven-development`（推荐）或 `superpowers:executing-plans`。
> Implements umbrella candidate **#1**。

**Goal:** 把 `InboxView` 上的死按钮「V2: 内容比对」变成真能用的对比页。拖两个同名不同 hash 的 zip 进 inbox 后，能在对比页看到两侧文件清单，按 4 个动作按钮之一处理冲突。

**Architecture:**
- **后端：** 新增 `GET /api/conflicts/:id/compare` 返回两侧 zip 内图片文件名清单 + 封面 URL（**不读完整 data，只读 entry name**）；复用 `services::archive::list_image_names`（新增函数，复用 `list_images` 的过滤逻辑但跳过 `std::io::copy`）。
- **前端：** 新增 `/inbox/compare/:id` 路由 + `ConflictView.vue` 左右两栏布局 + 4 个动作按钮（保留 A / 替换为 B / 都保留 / 都跳过）；`InboxView` 把 n-tag 改成 `<router-link>` 跳到对比页。

**Tech Stack:** Axum 0.7（handler）+ Vue 3 + Pinia + Naive UI + Vue Router。

**依赖：** 独立，可在 #2/#3 之前或之后做（不走新 HTTP 路由的 token 鉴权路径，但本 plan 加的 `/api/conflicts/:id/compare` 仍受 #2 token 中间件保护）。

---

## Task 1: `archive::list_image_names` 服务函数

**Files:**
- Modify: `src-tauri/src/services/archive.rs`

- [ ] **Step 1: 新增只读文件名清单函数**

当前 `list_images` 把整个文件内容读进 `Vec<u8>`，对比页只要文件名不要 data，新增一个轻量版：

```rust
/// Like `list_images` but only returns entry names — used by the
/// conflict compare endpoint which never needs the file bytes.
pub fn list_image_names(path: &Path) -> Result<Vec<String>> {
    if path.extension().and_then(|e| e.to_str()) != Some("zip") {
        return Err(anyhow!("unsupported archive format (zip only for V1)"));
    }
    let f = std::fs::File::open(path)?;
    let mut zip = zip::ZipArchive::new(f)?;
    let mut names = Vec::new();
    for i in 0..zip.len() {
        let entry = zip.by_index(i)?;
        if !entry.is_file() { continue; }
        let name = entry.name().to_string();
        let lower = name.to_lowercase();
        if IMG_EXTS.iter().any(|e| lower.ends_with(&format!(".{}", e))) {
            names.push(name);
        }
    }
    Ok(names)
}
```

注意：故意保留「只支持 zip」的硬限制——RAR 对比等到 #7 完成后再做。

- [ ] **Step 2: 单测**

在 `archive.rs` 的 `#[cfg(test)]` 加：

```rust
#[test]
fn list_image_names_skips_directories_and_non_images() {
    // 构造一个最小 zip 包含：images/01.jpg, images/02.png, readme.txt, subdir/
    let zip_bytes = build_test_zip(&[
        ("images/01.jpg", b"fake-jpg-data"),
        ("images/02.png", b"fake-png-data"),
        ("readme.txt",   b"hello"),
        ("subdir/",      b""),
    ]);
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("t.zip");
    std::fs::write(&p, zip_bytes).unwrap();
    let names = list_image_names(&p).unwrap();
    assert_eq!(names, vec!["images/01.jpg", "images/02.png"]);
}

#[test]
fn list_image_names_rejects_rar() {
    let p = std::path::Path::new("foo.rar");
    assert!(list_image_names(p).is_err());
}
```

`build_test_zip` helper 用 `zip::write::ZipWriter` 写到 `Vec<u8>` 即可。

- [ ] **Step 3: 跑测试**

`cd src-tauri && cargo test archive::` → 2 passed。

- [ ] **Step 4: 提交**

```bash
git add src-tauri/src/services/archive.rs
git commit -m "feat(archive): list_image_names — name-only list for conflict compare"
```

---

## Task 2: 后端 compare handler

**Files:**
- Modify: `src-tauri/src/http/api.rs`
- Modify: `src-tauri/src/http/mod.rs`

- [ ] **Step 1: 加 `ConflictCompare` 类型 + handler**

```rust
#[derive(Serialize)]
pub struct CompareSide {
    pub file_id: i64,
    pub title: String,
    pub hash: Option<String>,        // A 侧有，B 侧没有（还没入库）
    pub cover_url: Option<String>,
    pub image_names: Vec<String>,
    pub zip_missing: bool,           // 文件不在磁盘上时为 true
    pub zip_error: Option<String>,   // 解压失败时的错误信息
}

#[derive(Serialize)]
pub struct ConflictCompare {
    pub conflict_id: i64,
    pub a: CompareSide,
    pub b: CompareSide,
}

pub async fn compare(
    State(s): State<ApiState>,
    Path(conflict_id): Path<i64>,
) -> impl IntoResponse {
    use crate::db::entities::conflict::Entity as ConflictEntity;
    use sea_orm::EntityTrait;

    // 1. 查 conflict 行
    let row = match ConflictEntity::find_by_id(conflict_id)
        .one(&s.conn).await
    {
        Ok(Some(r)) => r,
        Ok(None)    => return (StatusCode::NOT_FOUND, "conflict not found").into_response(),
        Err(e)      => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };

    // 2. A 侧：a_file_id 查 doujinshi_file
    let a_row = doujinshi_file::Entity::find_by_id(row.a_file_id)
        .one(&s.conn).await.unwrap_or(None);
    let a = match a_row {
        Some(m) => {
            let (names, missing, err) = read_image_names(&m.current_path);
            CompareSide {
                file_id: m.id,
                title: m.title,
                hash: Some(m.hash),
                cover_url: build_cover_url(&m.hash, m.cover_path.as_deref()),
                image_names: names,
                zip_missing: missing,
                zip_error: err,
            }
        }
        None => CompareSide {
            file_id: row.a_file_id,
            title: format!("(文件 {} 已不存在)", row.a_file_id),
            hash: None,
            cover_url: None,
            image_names: vec![],
            zip_missing: false,
            zip_error: None,
        },
    };

    // 3. B 侧：b_file_path 直接读
    let b_path = std::path::PathBuf::from(&row.b_file_path);
    let (names, missing, err) = read_image_names(&b_path);
    let b = CompareSide {
        file_id: 0,
        title: row.b_filename,
        hash: None,
        cover_url: None,
        image_names: names,
        zip_missing: missing,
        zip_error: err,
    };

    (StatusCode::OK, Json(json!(ConflictCompare {
        conflict_id, a, b,
    }))).into_response()
}

/// Returns (names, missing, error_msg).
/// - missing=true 表示文件不在磁盘
/// - error_msg=Some 表示解压失败（非 zip / 损坏）
fn read_image_names(path: &str) -> (Vec<String>, bool, Option<String>) {
    let p = std::path::Path::new(path);
    if !p.exists() { return (vec![], true, None); }
    match crate::services::archive::list_image_names(p) {
        Ok(n)  => (n, false, None),
        Err(e) => (vec![], false, Some(e.to_string())),
    }
}

fn build_cover_url(hash: &str, cover_path: Option<&str>) -> Option<String> {
    cover_path.as_ref().map(|_| format!("/api/covers/by-hash/{}", hash))
}
```

注意：`build_cover_url` 路径前缀不带 host，前端 `useSettingsStore.apiBase` 拼。

- [ ] **Step 2: 注册路由**

`http/mod.rs` 加 `.route("/api/conflicts/:id/compare", get(api::compare))`。

注意路由位置：放在 `/api/doujinshi/:id/viewed`（#3）之后、`/api/covers/...` 之前即可，没有前缀冲突。

- [ ] **Step 3: 提交**

```bash
git add src-tauri/src/http/api.rs src-tauri/src/http/mod.rs
git commit -m "feat(http): GET /api/conflicts/:id/compare"
```

---

## Task 3: 后端 resolve 命令扩展（4 个动作）

**Files:**
- Modify: `src-tauri/src/commands/inbox.rs`（现有 `resolve` 是「跳过」，扩展为 4 选 1）

- [ ] **Step 1: 改命令签名**

现有 `resolve(conflict_id)` 把 conflict 标记为 resolved 并删除 inbox 文件。本 plan 加 4 个动作：

```rust
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictAction {
    KeepA,        // A 不动，删 B（inbox 文件）
    ReplaceB,     // B 替换 A：删 A 的 zip 文件，B 走完整入库流程
    KeepBoth,     // 两个都留：A 不动，B 入库但改文件名加后缀
    Skip,         // 等同于现有 resolve：保留 inbox 不动
}

#[tauri::command]
pub async fn resolve_conflict(
    state: State<'_, AppState>,
    conflict_id: i64,
    action: ConflictAction,
) -> AppResult<()> {
    // 读 conflict 行
    // 分发到 4 个分支：
    //   KeepA    → 删 b_file_path，UPDATE conflict SET resolved=1, action='keep_a'
    //   ReplaceB → 删 a_file_path，trigger B 重扫（直接调 identifier::identify_file），conflict resolved
    //   KeepBoth → B 重扫但 identifier 时把 filename 加后缀 " (copy)"，A 不动
    //   Skip     → 等同旧 resolve
}
```

具体业务逻辑：
- `KeepA`：直接 `std::fs::remove_file(b_path)`，写 `conflict.resolved_at = now`
- `ReplaceB`：`std::fs::remove_file(a_path)`；调用 `identifier::identify_file(&state, &b_path)` 走完整流程；冲突自动消失（因为 filename_alias 表会更新）
- `KeepBoth`：调 `identifier::identify_file` 但传 `force_rename = Some("(copy)")` 参数（需要在 identifier 加这个参数）；冲突消失
- `Skip`：旧逻辑

- [ ] **Step 2: identifier 加 `force_rename` 支持**

`services/identifier.rs` 的 `identify_file` 函数签名加 `Option<String>` 参数，内部在 `std::fs::rename` 之前用这个后缀重命名。

- [ ] **Step 3: 提交**

```bash
git add src-tauri/src/commands/inbox.rs src-tauri/src/services/identifier.rs
git commit -m "feat(inbox): resolve_conflict with 4 actions (keep_a/replace_b/keep_both/skip)"
```

---

## Task 4: 集成测试

**Files:**
- Modify: `src-tauri/tests/http_routes.rs`
- Create: `src-tauri/tests/inbox_resolve.rs`（或追加到 http_routes）

- [ ] **Step 1: compare endpoint 测试（4 个）**

```rust
#[tokio::test]
async fn compare_returns_both_sides_with_image_names() {
    // 构造冲突：A 已有 doujinshi_file 行 + zip；B 有 zip 在 inbox 目录
    // 调 GET /api/conflicts/{id}/compare
    // 断言 a.image_names / b.image_names 都非空，hash 字段 A 有 B 无
}

#[tokio::test]
async fn compare_returns_404_when_conflict_missing() { ... }

#[tokio::test]
async fn compare_returns_zip_missing_true_when_a_file_gone() { ... }

#[tokio::test]
async fn compare_returns_zip_error_when_b_is_not_zip() {
    // B 是个 .txt 文件，list_image_names 会返错
    // 断言 b.zip_error = Some("unsupported archive format...")
}
```

- [ ] **Step 2: resolve 命令测试（4 个）**

```rust
#[tokio::test]
async fn resolve_keep_a_deletes_b_file() {
    // 准备 A、B 两份 zip；调 resolve_conflict(id, KeepA)
    // 断言 b_path 不存在，conflict.resolved_at 非空
}

#[tokio::test]
async fn resolve_replace_b_promotes_b_to_library() {
    // 准备 A、B 两份 zip；调 resolve_conflict(id, ReplaceB)
    // 断言 a_path 不存在，b_path 移到 identified_dir，doujinshi_file 表有 B 行
}

#[tokio::test]
async fn resolve_keep_both_inserts_b_with_copy_suffix() {
    // 同上，预期 B 的 filename 末尾带 " (copy)"
}

#[tokio::test]
async fn resolve_skip_leaves_b_in_inbox() {
    // 旧逻辑，b_path 仍存在，conflict 未 resolved
}
```

- [ ] **Step 3: 跑**

`cd src-tauri && cargo test` → 全绿。

- [ ] **Step 4: 提交**

```bash
git add src-tauri/tests/
git commit -m "test(conflict): compare + resolve action coverage"
```

---

## Task 5: 前端 types + store

**Files:**
- Modify: `src/types/api.ts`
- Modify: `src/stores/inbox.ts`

- [ ] **Step 1: types 加 `ConflictCompare`**

```typescript
export interface ConflictCompare {
  conflict_id: number
  a: ConflictCompareSide
  b: ConflictCompareSide
}

export interface ConflictCompareSide {
  file_id: number
  title: string
  hash: string | null
  cover_url: string | null
  image_names: string[]
  zip_missing: boolean
  zip_error: string | null
}

export type ConflictAction = "keep_a" | "replace_b" | "keep_both" | "skip"
```

- [ ] **Step 2: store 加 actions**

```typescript
// stores/inbox.ts
async function fetchCompare(id: number): Promise<ConflictCompare> {
  return await invoke<ConflictCompare>("fetch_compare", { conflictId: id })
  // 注意：HTTP 走 settings.apiBase，不走 invoke
  // → 实际是 httpFetch(`GET ${apiBase}/api/conflicts/${id}/compare`)
}

async function resolveConflict(id: number, action: ConflictAction): Promise<void> {
  return await invoke("resolve_conflict", { conflictId: id, action })
}
```

前端 HTTP 调用通过 `apiClient`（#2 引入）发 `GET /api/conflicts/.../compare`，Tauri 命令只走 `resolve_conflict`。

- [ ] **Step 3: 提交**

```bash
git add src/types/api.ts src/stores/inbox.ts
git commit -m "feat(frontend): ConflictCompare types + inbox store actions"
```

---

## Task 6: ConflictView.vue + 路由

**Files:**
- Create: `src/views/ConflictView.vue`
- Modify: `src/router.ts`

- [ ] **Step 1: 路由**

```typescript
{ path: '/inbox/compare/:id', name: 'compare', component: () => import('./views/ConflictView.vue') }
```

- [ ] **Step 2: ConflictView.vue**

左右两栏，每栏顶部封面 + 标题 + 哈希，中间文件清单，底部 4 个按钮。

```vue
<script setup lang="ts">
import { ref, onMounted, computed } from "vue"
import { useRoute, useRouter } from "vue-router"
import { NCard, NSpace, NButton, NSpin, NList, NListItem, NEmpty, useMessage, NAlert } from "naive-ui"
import { useInboxStore, useSettingsStore } from "@/stores"
import type { ConflictCompare, ConflictAction } from "@/types/api"

const route = useRoute()
const router = useRouter()
const inbox = useInboxStore()
const settings = useSettingsStore()
const message = useMessage()

const data = ref<ConflictCompare | null>(null)
const loading = ref(false)
const acting = ref(false)

const conflictId = computed(() => Number(route.params.id))

onMounted(async () => {
  await settings.load()
  loading.value = true
  try {
    data.value = await inbox.fetchCompare(conflictId.value)
  } catch (e) {
    message.error(String(e))
  } finally {
    loading.value = false
  }
})

async function act(action: ConflictAction) {
  acting.value = true
  try {
    await inbox.resolveConflict(conflictId.value, action)
    message.success("已处理")
    router.push({ name: "inbox" })
  } catch (e) {
    message.error(String(e))
  } finally {
    acting.value = false
  }
}
</script>

<template>
  <div>
    <div class="page-header">
      <h1>冲突对比</h1>
      <span class="count">conflict #{{ conflictId }}</span>
    </div>
    <n-spin :show="loading || acting">
      <div v-if="data" class="compare-grid">
        <n-card title="A · 已识别">
          <img v-if="data.a.cover_url" :src="settings.apiBase + data.a.cover_url" style="max-width: 200px" />
          <div>标题: {{ data.a.title }}</div>
          <div v-if="data.a.hash" style="font-family: monospace; font-size: 12px">
            哈希: {{ data.a.hash.slice(0, 16) }}…
          </div>
          <n-alert v-if="data.a.zip_missing" type="warning" title="A 文件已不在磁盘" style="margin: 8px 0" />
          <n-alert v-if="data.a.zip_error" type="error" :title="data.a.zip_error" style="margin: 8px 0" />
          <h4>文件列表 ({{ data.a.image_names.length }})</h4>
          <n-list bordered>
            <n-list-item v-for="n in data.a.image_names" :key="n">{{ n }}</n-list-item>
          </n-list>
        </n-card>
        <n-card title="B · inbox 待处理">
          <div>文件名: {{ data.b.title }}</div>
          <n-alert v-if="data.b.zip_missing" type="warning" title="B 文件已不在磁盘" style="margin: 8px 0" />
          <n-alert v-if="data.b.zip_error" type="error" :title="data.b.zip_error" style="margin: 8px 0" />
          <h4>文件列表 ({{ data.b.image_names.length }})</h4>
          <n-list bordered>
            <n-list-item v-for="n in data.b.image_names" :key="n">{{ n }}</n-list-item>
          </n-list>
        </n-card>
      </div>
      <n-space v-if="data" style="margin-top: 16px" justify="end">
        <n-button @click="act('keep_a')">保留 A（删 B）</n-button>
        <n-button type="warning" @click="act('replace_b')">替换为 B（删 A）</n-button>
        <n-button @click="act('keep_both')">都保留（A 不动，B 加后缀入库）</n-button>
        <n-button @click="act('skip')">都跳过（保留 B 在 inbox）</n-button>
      </n-space>
    </n-spin>
  </div>
</template>

<style scoped>
.compare-grid { display: grid; grid-template-columns: 1fr 1fr; gap: 16px; }
img { border: 1px solid var(--surface-border); border-radius: 4px; }
</style>
```

- [ ] **Step 3: 提交**

```bash
git add src/views/ConflictView.vue src/router.ts
git commit -m "feat(frontend): ConflictView page + /inbox/compare/:id route"
```

---

## Task 7: InboxView 把死按钮改 router-link

**Files:**
- Modify: `src/views/InboxView.vue`

- [ ] **Step 1: 替换 n-tag**

```vue
<template #suffix>
  <n-space>
    <router-link
      :to="{ name: 'compare', params: { id: c.id } }"
      custom
      v-slot="{ navigate }"
    >
      <n-button size="small" type="primary" @click="navigate">
        内容比对
      </n-button>
    </router-link>
    <n-button size="small" @click="store.resolve(c.id)">
      跳过
    </n-button>
  </n-space>
</template>
```

注意：旧的 `store.resolve(c.id)` 等同于新的 `resolveConflict(id, 'skip')`。本 plan 不动 store.resolve 的实现，保留向后兼容。

- [ ] **Step 2: 提交**

```bash
git add src/views/InboxView.vue
git commit -m "feat(inbox): turn 'V2 内容比对' tag into a real button"
```

---

## Task 8: 回归 + E2E

- [ ] **Step 1:** `cd src-tauri && cargo test` 全绿
- [ ] **Step 2:** `pnpm lint && pnpm build` 全绿
- [ ] **Step 3:** E2E 流程：拖两个同名不同 hash 的 zip 进 `resources/doujinshi/` → 触发 conflict → 进 InboxView 点「内容比对」→ 进入 ConflictView 看到两侧文件清单 → 点「保留 A」→ 跳回 InboxView，B 不见了，A 仍在 Library

---

## Self-review

- [ ] `/api/conflicts/:id/compare` 在 4 种情况（都有、都没、A 缺、B 非 zip）下都返回正确 JSON
- [ ] 4 个 resolve 动作都能跑通且副作用符合预期
- [ ] InboxView 的死按钮变真按钮，点击跳转到对比页
- [ ] 对比页的 4 个按钮都能把状态改对（DeleteDialogA/B 不复用）
- [ ] 没有破坏旧 `store.resolve(c.id)`「跳过」行为