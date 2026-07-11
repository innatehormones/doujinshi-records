# V2 Sub-Plan 5 — 详细观看页 DetailView

> **For agentic workers:** REQUIRED SUB-SKILL: `superpowers:subagent-driven-development`（推荐）或 `superpowers:executing-plans`。
> Implements umbrella candidate **#5**。

**Goal:** 在 LibraryView 点卡片进 DetailView，能翻看 zip 内所有图、编辑 7 个元数据字段、切换状态。

**Architecture:**
- **后端：**
  - `GET /api/doujinshi/:id/images` 返回 zip 内所有图片名 + base64（or 流式 URL）
  - `PATCH /api/doujinshi/:id` 接受部分字段更新（title/circle/series/translator/version/note/rating）
  - 新增 `commands::library::update_metadata(id, patch)` Tauri 命令
  - HTTP handler 桥接 PATCH
- **前端：**
  - `/library/:id` 路由 + `DetailView.vue`
  - 大图轮播（左侧 60%）+ 元数据编辑面板（右侧 40%）
  - LibraryView FileCard 点击 → 跳 DetailView

**Tech Stack:** Axum + Vue 3 + Naive UI（NCarousel for 图预览）。

**依赖：** 无强制依赖，但 #2 完成后再做（避免 auth 中间件来回改测试）。

---

## Task 1: 后端 `commands::library::update_metadata`

**Files:**
- Modify: `src-tauri/src/commands/library.rs`

- [ ] **Step 1: 加 update_metadata 命令**

```rust
use serde::Deserialize;

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct MetadataPatch {
    pub title: Option<String>,
    pub circle: Option<String>,
    pub series: Option<String>,
    pub translator: Option<String>,
    pub version: Option<String>,
    pub note: Option<String>,
    pub rating: Option<i32>,
}

#[tauri::command]
pub async fn update_metadata(
    state: State<'_, AppState>,
    id: i64,
    patch: MetadataPatch,
) -> AppResult<()> {
    use sea_orm::{EntityTrait, ActiveModelTrait, Set};
    let row = doujinshi_file::Entity::find_by_id(id)
        .one(&state.conn).await?
        .ok_or_else(|| AppError::Other(format!("file {} not found", id)))?;
    let mut am: doujinshi_file::ActiveModel = row.into();
    if let Some(v) = patch.title       { am.title       = Set(v); }
    if let Some(v) = patch.circle      { am.circle      = Set(Some(v)); }
    if let Some(v) = patch.series      { am.series      = Set(Some(v)); }
    if let Some(v) = patch.translator  { am.translator  = Set(Some(v)); }
    if let Some(v) = patch.version     { am.version     = Set(Some(v)); }
    if let Some(v) = patch.note        { am.note        = Set(Some(v)); }
    if let Some(v) = patch.rating      { am.rating      = Set(Some(v)); }
    am.updated_at = Set(chrono::Utc::now());
    am.update(&state.conn).await?;
    Ok(())
}
```

- [ ] **Step 2: lib.rs 注册命令**

在 `tauri::generate_handler!` 宏里加 `commands::library::update_metadata`。

- [ ] **Step 3: 单测**

```rust
// commands/library.rs 内的 #[cfg(test)]
#[tokio::test]
async fn update_metadata_changes_only_specified_fields() {
    // 准备一行 doujinshi_file（title="旧", circle=Some("旧"), note=None）
    // 调 update_metadata(id, MetadataPatch { title: Some("新"), note: Some("hi"), ..Default::default() })
    // 重新查行，断言 title="新", circle=Some("旧")（未动）, note=Some("hi")
}
```

- [ ] **Step 4: 提交**

```bash
git add src-tauri/src/commands/library.rs src-tauri/src/lib.rs
git commit -m "feat(library): update_metadata command (7-field patch)"
```

---

## Task 2: 后端 `GET /api/doujinshi/:id/images`

**Files:**
- Modify: `src-tauri/src/http/api.rs`
- Modify: `src-tauri/src/http/mod.rs`

- [ ] **Step 1: handler**

```rust
#[derive(Serialize)]
pub struct ImagesResponse {
    pub file_id: i64,
    pub images: Vec<ImageEntry>,  // base64 + name
    pub zip_missing: bool,
}

#[derive(Serialize)]
pub struct ImageEntry {
    pub name: String,
    /// base64-encoded bytes, prefixed with "data:image/jpeg;base64,"
    pub data_url: String,
}

pub async fn images(
    State(s): State<ApiState>,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    use sea_orm::EntityTrait;
    let row = match doujinshi_file::Entity::find_by_id(id).one(&s.conn).await {
        Ok(Some(r)) => r,
        Ok(None) => return (StatusCode::NOT_FOUND, "file not found").into_response(),
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };
    let path = std::path::Path::new(&row.current_path);
    if !path.exists() {
        return (StatusCode::OK, Json(json!(ImagesResponse {
            file_id: id, images: vec![], zip_missing: true,
        }))).into_response();
    }
    match crate::services::archive::list_images(path) {
        Ok(entries) => {
            let images = entries.into_iter().map(|e| {
                let b64 = base64::engine::general_purpose::STANDARD.encode(&e.data);
                ImageEntry {
                    name: e.name,
                    data_url: format!("data:image/{};base64,{}", guess_ext(&e.name), b64),
                }
            }).collect();
            (StatusCode::OK, Json(json!(ImagesResponse {
                file_id: id, images, zip_missing: false,
            }))).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

fn guess_ext(name: &str) -> &'static str {
    let lower = name.to_lowercase();
    if lower.ends_with(".png") { "png" }
    else if lower.ends_with(".webp") { "webp" }
    else { "jpeg" }
}
```

base64 crate 已在 #2 引入。

- [ ] **Step 2: 注册路由**

`http/mod.rs` 加 `.route("/api/doujinshi/:id/images", get(api::images))`，放在 `/api/doujinshi/:id` 之后。

- [ ] **Step 3: 集成测试**

```rust
#[tokio::test]
async fn images_returns_entries_when_zip_present() {
    // 准备 doujinshi_file 行 + zip 内 2 张假图（jgp/png）
    // 调 GET /api/doujinshi/{id}/images
    // 断言 images.len() == 2，每个 data_url 以 data:image/ 开头
}

#[tokio::test]
async fn images_returns_zip_missing_true_when_file_gone() { ... }

#[tokio::test]
async fn images_returns_404_when_id_missing() { ... }
```

- [ ] **Step 4: 提交**

```bash
git add src-tauri/src/http/api.rs src-tauri/src/http/mod.rs src-tauri/tests/
git commit -m "feat(http): GET /api/doujinshi/:id/images (base64 data urls)"
```

---

## Task 3: 后端 PATCH handler

**Files:**
- Modify: `src-tauri/src/http/api.rs`

- [ ] **Step 1: PATCH handler**

```rust
pub async fn patch_metadata(
    State(s): State<ApiState>,
    Path(id): Path<i64>,
    Json(patch): Json<crate::commands::library::MetadataPatch>,
) -> impl IntoResponse {
    use sea_orm::{EntityTrait, ActiveModelTrait, Set};
    let row = match doujinshi_file::Entity::find_by_id(id).one(&s.conn).await {
        Ok(Some(r)) => r,
        Ok(None) => return (StatusCode::NOT_FOUND, "file not found").into_response(),
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };
    let mut am: doujinshi_file::ActiveModel = row.into();
    if let Some(v) = patch.title       { am.title       = Set(v); }
    if let Some(v) = patch.circle      { am.circle      = Set(Some(v)); }
    if let Some(v) = patch.series      { am.series      = Set(Some(v)); }
    if let Some(v) = patch.translator  { am.translator  = Set(Some(v)); }
    if let Some(v) = patch.version     { am.version     = Set(Some(v)); }
    if let Some(v) = patch.note        { am.note        = Set(Some(v)); }
    if let Some(v) = patch.rating      { am.rating      = Set(Some(v)); }
    am.updated_at = Set(chrono::Utc::now());
    match am.update(&s.conn).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}
```

需要 `MetadataPatch` 在 `commands/library.rs` 里 `pub`，否则 API 模块访问不到。

- [ ] **Step 2: 注册路由**

`http/mod.rs` 加：

```rust
.route("/api/doujinshi/:id", get(api::by_id).patch(api::patch_metadata))
```

（axum 允许同一路径多个 method handler）

- [ ] **Step 3: 集成测试**

```rust
#[tokio::test]
async fn patch_updates_title_and_returns_204() { ... }
#[tokio::test]
async fn patch_with_empty_body_is_noop() { ... }
#[tokio::test]
async fn patch_unknown_id_returns_404() { ... }
```

- [ ] **Step 4: 提交**

```bash
git add src-tauri/src/http/api.rs src-tauri/src/http/mod.rs src-tauri/tests/
git commit -m "feat(http): PATCH /api/doujinshi/:id (metadata partial update)"
```

---

## Task 4: 前端 types + store

**Files:**
- Modify: `src/types/api.ts`
- Modify: `src/stores/library.ts`

- [ ] **Step 1: types**

```typescript
export interface MetadataPatch {
  title?: string
  circle?: string | null
  series?: string | null
  translator?: string | null
  version?: string | null
  note?: string | null
  rating?: number | null
}

export interface DetailImage {
  name: string
  data_url: string
}

export interface DetailImagesResponse {
  file_id: number
  images: DetailImage[]
  zip_missing: boolean
}
```

- [ ] **Step 2: store actions**

```typescript
// stores/library.ts
async function fetchDetailImages(id: number): Promise<DetailImagesResponse> {
  return await apiGet<DetailImagesResponse>(`/api/doujinshi/${id}/images`)
}

async function updateMetadata(id: number, patch: MetadataPatch): Promise<void> {
  await apiPatch(`/api/doujinshi/${id}`, patch)
}
```

`apiPatch` 加到 `api/http.ts`：

```typescript
export async function apiPatch(path: string, body: unknown): Promise<Response> {
  const settings = useSettingsStore()
  await settings.load()
  return await fetch(settings.apiBase + path, {
    method: "PATCH",
    headers: {
      "Authorization": `Bearer ${settings.data?.auth_token ?? ""}`,
      "Content-Type": "application/json",
    },
    body: JSON.stringify(body),
  })
}
```

- [ ] **Step 3: 提交**

```bash
git add src/types/api.ts src/stores/library.ts src/api/http.ts
git commit -m "feat(frontend): detail types + library store actions + apiPatch"
```

---

## Task 5: DetailView.vue

**Files:**
- Create: `src/views/DetailView.vue`
- Modify: `src/router.ts`
- Modify: `src/components/FileCard.vue`（卡片点击跳详情）

- [ ] **Step 1: 路由**

```typescript
{ path: '/library/:id', name: 'detail', component: () => import('./views/DetailView.vue') }
```

- [ ] **Step 2: FileCard.vue 加 click 事件**

在 `<article>` 上加 `@click="$emit('open', file.id)"`，emit 一个新事件 `open`。

`LibraryView.vue` 监听 `@open="onCardOpen"`：

```typescript
function onCardOpen(id: number) {
  router.push({ name: 'detail', params: { id } })
}
```

- [ ] **Step 3: DetailView.vue**

```vue
<script setup lang="ts">
import { ref, onMounted, computed, watch } from "vue"
import { useRoute, useRouter } from "vue-router"
import {
  NCard, NSpace, NButton, NSpin, NCarousel, NInput, NInputNumber,
  NSelect, NEmpty, NAlert, useMessage,
} from "naive-ui"
import { useLibraryStore, useSettingsStore } from "@/stores"
import { api } from "@/api/tauri"
import type { FileSummary, MetadataPatch } from "@/types/api"

const route = useRoute()
const router = useRouter()
const store = useLibraryStore()
const settings = useSettingsStore()
const message = useMessage()

const id = computed(() => Number(route.params.id))
const file = ref<FileSummary | null>(null)
const images = ref<{ name: string; data_url: string }[]>([])
const zipMissing = ref(false)
const loading = ref(false)
const saving = ref(false)

// 编辑表单
const editTitle = ref("")
const editCircle = ref("")
const editSeries = ref("")
const editTranslator = ref("")
const editVersion = ref("")
const editNote = ref("")
const editRating = ref<number | null>(null)

const ratingOptions = [
  { label: "★", value: 1 }, { label: "★★", value: 2 }, { label: "★★★", value: 3 },
  { label: "★★★★", value: 4 }, { label: "★★★★★", value: 5 },
]

async function load() {
  loading.value = true
  try {
    const f = store.items.find((x) => x.id === id.value) || await api.getById(id.value)
    file.value = f
    editTitle.value = f.title
    editCircle.value = f.circle ?? ""
    editSeries.value = ""
    editTranslator.value = ""
    editVersion.value = ""
    editNote.value = ""
    editRating.value = null
    const r = await store.fetchDetailImages(id.value)
    images.value = r.images
    zipMissing.value = r.zip_missing
  } catch (e) {
    message.error(String(e))
  } finally {
    loading.value = false
  }
}

onMounted(load)
watch(id, load)

async function save() {
  saving.value = true
  try {
    const patch: MetadataPatch = {
      title: editTitle.value,
      circle: editCircle.value || null,
      series: editSeries.value || null,
      translator: editTranslator.value || null,
      version: editVersion.value || null,
      note: editNote.value || null,
      rating: editRating.value,
    }
    await store.updateMetadata(id.value, patch)
    message.success("已保存")
    await store.load()  // 刷新列表
  } catch (e) {
    message.error(String(e))
  } finally {
    saving.value = false
  }
}

async function markViewed() {
  await api.markViewed(id.value)
  message.success("已标记已看")
  await store.load()
}

async function markDelete() {
  router.push({ name: 'library' })  // 复用 DeleteDialogA/B 流程
  // 实际：从 store 取 target，调 store.startDelete
}
</script>

<template>
  <div>
    <div class="page-header">
      <n-button text @click="router.back()">← 返回</n-button>
      <h1>{{ file?.title ?? `文件 #${id}` }}</h1>
    </div>
    <n-spin :show="loading || saving">
      <div v-if="file" class="detail-grid">
        <n-card title="图片预览" class="preview-pane">
          <n-alert v-if="zipMissing" type="warning" title="压缩包已不在磁盘" />
          <n-empty v-else-if="images.length === 0" description="zip 内无图片" />
          <n-carousel v-else show-arrow autoplay>
            <img v-for="img in images" :key="img.name" :src="img.data_url" :alt="img.name" />
          </n-carousel>
        </n-card>
        <n-card title="元数据" class="meta-pane">
          <n-space vertical>
            <n-input v-model:value="editTitle" placeholder="标题" />
            <n-input v-model:value="editCircle" placeholder="社团 (circle)" />
            <n-input v-model:value="editSeries" placeholder="系列 (series)" />
            <n-input v-model:value="editTranslator" placeholder="翻译 (translator)" />
            <n-input v-model:value="editVersion" placeholder="版本 (version)" />
            <n-input v-model:value="editNote" type="textarea" placeholder="备注" />
            <n-select v-model:value="editRating" :options="ratingOptions" placeholder="评分" clearable />
            <n-button type="primary" @click="save" :loading="saving">保存</n-button>
          </n-space>
        </n-card>
        <n-card title="操作" class="action-pane">
          <n-space vertical>
            <n-button :disabled="file.viewed" @click="markViewed">标记已看</n-button>
            <n-button :disabled="file.marked_for_delete" @click="markDelete">标记删除</n-button>
            <n-tag v-if="file.viewed">已看</n-tag>
            <n-tag v-if="file.marked_for_delete" type="warning">已标记删除</n-tag>
          </n-space>
        </n-card>
      </div>
    </n-spin>
  </div>
</template>

<style scoped>
.detail-grid { display: grid; grid-template-columns: 3fr 2fr; grid-template-rows: auto auto; gap: 16px; }
.preview-pane { grid-row: span 2; }
img { max-width: 100%; max-height: 80vh; object-fit: contain; }
</style>
```

- [ ] **Step 4: 提交**

```bash
git add src/views/DetailView.vue src/router.ts src/components/FileCard.vue src/views/LibraryView.vue
git commit -m "feat(frontend): DetailView + /library/:id route + FileCard click"
```

---

## Task 6: getById / markViewed Tauri 命令补充

**Files:**
- Modify: `src/api/tauri.ts`

- [ ] **Step 1: 检查现有 `api.ts` 是否有 `getById` 和 `markViewed`**

如果没有，加：

```typescript
// api/tauri.ts
async getById(id: number): Promise<FileSummary> {
  return await invoke("get_by_id", { id })
}
async markViewed(id: number): Promise<void> {
  return await invoke("mark_viewed", { id })
}
```

Tauri 侧命令 `get_by_id` 也需要；如果没有，在 `commands/library.rs` 加：

```rust
#[tauri::command]
pub async fn get_by_id(state: State<'_, AppState>, id: i64) -> AppResult<FileSummary> {
    use sea_orm::EntityTrait;
    let row = doujinshi_file::Entity::find_by_id(id)
        .one(&state.conn).await?
        .ok_or_else(|| AppError::Other(format!("file {} not found", id)))?;
    Ok(file_summary::from_model(&row))
}
```

- [ ] **Step 2: 提交**

```bash
git add src/api/tauri.ts src-tauri/src/commands/library.rs src-tauri/src/lib.rs
git commit -m "feat(library): get_by_id tauri command + api wrapper"
```

---

## Task 7: E2E + 回归

- [ ] **Step 1:** `cd src-tauri && cargo test` 全绿
- [ ] **Step 2:** `pnpm lint && pnpm build` 全绿
- [ ] **Step 3:** E2E：LibraryView 点卡片 → 进 DetailView → 翻图轮播正常 → 改 title → 保存 → 返回 Library → Library 卡片显示新 title
- [ ] **Step 4:** E2E：DetailView 点「标记已看」→ 返回 Library → 卡片显示已看 tag

---

## Self-review

- [ ] `GET /api/doujinshi/:id/images` 返回 base64 data_url（不要返回 raw bytes 数组，前端处理麻烦）
- [ ] `PATCH /api/doujinshi/:id` 只更新传入字段，未传入字段不动
- [ ] DetailView 的「标记删除」走原来 DeleteDialogA/B 流程（不在 DetailView 重复弹窗）
- [ ] 7 字段都有 UI 输入控件
- [ ] hash / path / created_at 等身份字段在 UI 不可改（DetailView 也不显示）
- [ ] FileCard 点击进 DetailView 而不是 modal（避免嵌套弹窗）