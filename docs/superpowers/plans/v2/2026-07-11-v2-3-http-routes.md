# V2 Sub-Plan 3 — 三个 V2 HTTP 路由

> **For agentic workers:** REQUIRED SUB-SKILL: `superpowers:subagent-driven-development`（推荐）或 `superpowers:executing-plans`。
> Implements umbrella candidate **#3**。

**Goal:** 给浏览器扩展加 3 个新 HTTP 端点：`/api/doujinshi/check`、`/api/doujinshi/:id/viewed`（POST）、`/api/covers/by-hash/:hash`。

**Architecture:** 复用现有 `by_hash` / `cover` handler 的查询逻辑；新增 `mark_viewed` 命令的 HTTP 桥接；token 鉴权已在 #2 完成，本 plan 只接受带 Bearer 头的请求。

**Tech Stack:** Axum 0.7 + SeaORM 1.1 + JSON。

**依赖：** 必须在 #2（token 鉴权中间件）之后跑，否则测试无法覆盖 401 路径。

---

## Task 1: `/api/doujinshi/check?hash=<blake3>` — 复用的 by-hash 别名

**Files:**
- Modify: `src-tauri/src/http/api.rs`
- Modify: `src-tauri/src/http/mod.rs`（路由注册）

- [ ] **Step 1: 在 api.rs 加 check handler**

`check` 是 `by_hash` 的语义化别名（浏览器扩展侧更直观），返回完全相同的 JSON：找到返回 FileSummary，没找到返回 `null`。

```rust
pub async fn check(
    State(s): State<ApiState>,
    Query(p): Query<CheckParams>,
) -> Json<serde_json::Value> {
    let row = doujinshi_file::Entity::find()
        .filter(doujinshi_file::Column::Hash.eq(&p.hash))
        .one(&s.conn)
        .await
        .unwrap_or(None);
    match row {
        Some(m) => Json(json!(file_summary::from_model(&m))),
        None => Json(json!(null)),
    }
}

#[derive(Deserialize)]
pub struct CheckParams { pub hash: String }
```

- [ ] **Step 2: 注册路由**

在 `http/mod.rs` 的 `build_router` 里 `.route("/api/doujinshi/check", get(api::check))`，**位置必须在 `/api/doujinshi/by-hash/:hash` 之前**（否则会被 `:hash` 吞掉）。

- [ ] **Step 3: 跑现有 http_routes 测试**

`cd src-tauri && cargo test --test http_routes` 应仍然通过。

- [ ] **Step 4: 提交**

```bash
git add src-tauri/src/http/api.rs src-tauri/src/http/mod.rs
git commit -m "feat(http): GET /api/doujinshi/check?hash= alias for by-hash"
```

---

## Task 2: `/api/doujinshi/:id/viewed`（POST）— 标记已看

**Files:**
- Modify: `src-tauri/src/commands/library.rs`（无新命令，直接复用 `mark_viewed`）
- Modify: `src-tauri/src/http/api.rs`（新增 `mark_viewed_http` 桥接 handler）

- [ ] **Step 1: HTTP 桥接 handler**

HTTP 层不能直接调 `#[tauri::command]`（没有 State extractor），要写一个轻量 handler 包同样的逻辑：

```rust
pub async fn mark_viewed_http(
    State(s): State<ApiState>,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    use sea_orm::{EntityTrait, ActiveModelTrait, Set};
    let row = match doujinshi_file::Entity::find_by_id(id).one(&s.conn).await {
        Ok(Some(r)) => r,
        Ok(None) => return (StatusCode::NOT_FOUND, "no file").into_response(),
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };
    let mut am: doujinshi_file::ActiveModel = row.into();
    am.viewed = Set(true);
    am.updated_at = Set(chrono::Utc::now());
    match am.update(&s.conn).await {
        Ok(_) => (StatusCode::NO_CONTENT, "").into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}
```

- [ ] **Step 2: 注册路由**

`http/mod.rs` 加 `.route("/api/doujinshi/:id/viewed", post(api::mark_viewed_http))`。

- [ ] **Step 3: 提交**

```bash
git add src-tauri/src/http/api.rs src-tauri/src/http/mod.rs
git commit -m "feat(http): POST /api/doujinshi/:id/viewed marks viewed"
```

---

## Task 3: `/api/covers/by-hash/:hash` — 按 hash 直接拿封面

**Files:**
- Modify: `src-tauri/src/http/api.rs`

- [ ] **Step 1: 新 handler**

复用现有 `cover` handler 的查询与读盘逻辑：

```rust
pub async fn cover_by_hash(
    State(s): State<ApiState>,
    Path(hash): Path<String>,
) -> impl IntoResponse {
    cover(State(s), Path(hash)).await
}
```

或者直接把 `cover` 的实现抽成私有函数 `serve_cover(state, hash)`，让 `cover` 和 `cover_by_hash` 都调它（更清晰）。

- [ ] **Step 2: 注册路由**

`http/mod.rs` 加 `.route("/api/covers/by-hash/:hash", get(api::cover_by_hash))`。

注意：`/api/covers/:file_id` 已有，**新路由放在它之前**避免冲突。

- [ ] **Step 3: 提交**

```bash
git add src-tauri/src/http/api.rs src-tauri/src/http/mod.rs
git commit -m "feat(http): GET /api/covers/by-hash/:hash (explicit alias)"
```

---

## Task 4: 集成测试（必须在 #2 token 鉴权完成后跑）

**Files:**
- Modify: `src-tauri/tests/http_routes.rs`

- [ ] **Step 1: 加 6 个 test**

```rust
// check
#[tokio::test]
async fn check_returns_summary_when_hash_known() { ... }
#[tokio::test]
async fn check_returns_null_when_hash_unknown() { ... }

// mark_viewed
#[tokio::test]
async fn mark_viewed_http_sets_flag_to_true() { ... }
#[tokio::test]
async fn mark_viewed_http_returns_404_when_id_missing() { ... }

// cover_by_hash
#[tokio::test]
async fn cover_by_hash_returns_jpeg_when_present() { ... }
#[tokio::test]
async fn cover_by_hash_returns_404_when_disk_missing() { ... }
```

每个 test 都要带 `Authorization: Bearer <token>` 头（token 来自测试 helper，#2 已生成）。

- [ ] **Step 2: 跑**

`cd src-tauri && cargo test --test http_routes` → 6 passed。

- [ ] **Step 3: 提交**

```bash
git add src-tauri/tests/http_routes.rs
git commit -m "test(http): cover three new V2 routes + auth header"
```

---

## Task 5: 前端 SettingsView 加新路由到 apiLines

**Files:**
- Modify: `src/views/SettingsView.vue`

- [ ] **Step 1: 扩充 apiLines**

```typescript
const apiLines = computed(() => [
  "GET  " + store.apiBase + "/api/health              健康检查",
  "GET  " + store.apiBase + "/api/doujinshi/search?q=关键词",
  "GET  " + store.apiBase + "/api/doujinshi/check?hash=<blake3>   检查哈希是否在库",
  "GET  " + store.apiBase + "/api/doujinshi/by-hash/<hash>",
  "GET  " + store.apiBase + "/api/doujinshi/<id>",
  "POST " + store.apiBase + "/api/doujinshi/<id>/viewed   标记已看",
  "GET  " + store.apiBase + "/api/covers/by-hash/<hash>   按哈希取封面",
  "GET  " + store.apiBase + "/api/covers/<file_id>",
])
```

- [ ] **Step 2: 提交**

```bash
git add src/views/SettingsView.vue
git commit -m "docs(settings): list three new V2 HTTP routes"
```

---

## Task 6: 回归

- [ ] `cd src-tauri && cargo test` 全绿
- [ ] `pnpm lint && pnpm build` 全绿

---

## Self-review

- [ ] `/api/doujinshi/check`、`/api/doujinshi/:id/viewed`、`/api/covers/by-hash/:hash` 三个端点都有集成测试
- [ ] 路由注册顺序正确：`check` 在 `by-hash/:hash` 前，`by-hash` 在 `:id` 前，`cover/by-hash/:hash` 在 `cover/:file_id` 前
- [ ] 没有 401（鉴权由 #2 中间件统一覆盖，不在每个 handler 重复）
- [ ] SettingsView apiLines 反映三个新路由