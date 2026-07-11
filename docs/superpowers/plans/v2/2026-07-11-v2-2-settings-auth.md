# V2 Sub-Plan 2 — 设置页增强 + HTTP Token 鉴权

> **For agentic workers:** REQUIRED SUB-SKILL: `superpowers:subagent-driven-development`（推荐）或 `superpowers:executing-plans`。
> Implements umbrella candidate **#2**。

**Goal:** 把 HTTP API 从「CORS 全开、谁都行」升级到「带 Bearer token 鉴权」，端口占用时按退避重试；前端 SettingsView 暴露端口 / token / inbox 目录三项设置。

**Architecture:**
- **后端：**
  - `http::auth` 中间件读 `app_setting.auth_token`，校验 `Authorization: Bearer <token>` 头；缺失或错则 401
  - `/api/health` 显式豁免（健康检查不该卡 token）
  - `http::port_allocator` 重试：保留端口失败时按 100ms × N（默认 N=3）退避，超过 N 次才回退到随机
  - `app_setting` 表加 `auth_token` 列（schema 迁移 v3）；首次启动生成 32 字节随机 token（base64）
- **前端：**
  - SettingsView 3 个新区块：HTTP 端口、Token、Inbox 目录
  - 所有 HTTP 调用统一通过 `apiClient` 包装，自动注入 `Authorization: Bearer <token>` 头
  - 新增 `useApiClient()` composable 复用

**Tech Stack:** Axum middleware + Naive UI + Pinia + Vue 3。

**依赖：** #3 三个 V2 HTTP 路由要在 #2 完成后才能做（依赖 token 鉴权中间件覆盖）。

---

## Task 1: schema v3 迁移（auth_token 列）

**Files:**
- Modify: `src-tauri/src/db/migrations.rs`

- [ ] **Step 1: CURRENT_VERSION 升 3，加迁移函数**

```rust
pub const CURRENT_VERSION: i64 = 3;

// 在 migrations vec 里追加：
(3, "add app_setting.auth_token", Box::new(|c| Box::pin(async move { add_auth_token(c).await }))),

async fn add_auth_token(conn: &DatabaseConnection) -> Result<()> {
    let backend = conn.get_database_backend();
    let rows: Vec<(String,)> = conn.query_all(Statement::from_string(
        backend.clone(),
        "SELECT name FROM pragma_table_info('app_setting') WHERE name='auth_token'".to_string(),
    )).await?;
    if rows.is_empty() {
        conn.execute(Statement::from_string(
            backend.clone(),
            "ALTER TABLE app_setting ADD COLUMN auth_token TEXT".to_string(),
        )).await?;
    }
    Ok(())
}
```

- [ ] **Step 2: 跑 migrations 测试**

`cd src-tauri && cargo test migrations::` → 通过。

- [ ] **Step 3: 提交**

```bash
git add src-tauri/src/db/migrations.rs
git commit -m "feat(db): schema v3 — add app_setting.auth_token column"
```

---

## Task 2: token 生成 + 启动加载

**Files:**
- Modify: `src-tauri/src/lib.rs`（首次启动生成 token 写入 app_setting）
- Create: `src-tauri/src/http/auth_token.rs`

- [ ] **Step 1: 生成函数**

```rust
// http/auth_token.rs
use base64::{engine::general_purpose, Engine};
use rand::RngCore;

pub fn generate() -> String {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}
```

需要 `base64 = "0.22"` 和 `rand = "0.8"`（可能已是 transitive dep）。

- [ ] **Step 2: lib.rs 启动逻辑**

在 `init_schema` 之后、`build_router` 之前：

```rust
let auth_token = ensure_auth_token(&conn).await?;
tracing::info!("HTTP auth token: {}", &auth_token);  // 不脱敏；本地工具
```

`ensure_auth_token` 函数：
1. 查 `app_setting.auth_token`
2. 非空 → 返回
3. 空 → 生成新 token，写回，返回

- [ ] **Step 3: 让 `ApiState` 带 token**

```rust
// http/mod.rs
pub struct ApiState {
    pub conn: DatabaseConnection,
    pub covers_dir: Arc<std::path::PathBuf>,
    pub auth_token: Arc<String>,
}
```

- [ ] **Step 4: 提交**

```bash
git add src-tauri/src/http/auth_token.rs src-tauri/src/lib.rs src-tauri/src/http/mod.rs
git commit -m "feat(http): generate auth_token on first launch, expose via ApiState"
```

---

## Task 3: auth 中间件

**Files:**
- Create: `src-tauri/src/http/auth.rs`
- Modify: `src-tauri/src/http/mod.rs`（路由层挂中间件）

- [ ] **Step 1: 中间件实现**

```rust
// http/auth.rs
use axum::{
    extract::{Request, State},
    http::{header, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use crate::http::ApiState;

const ALLOW_PATHS: &[&str] = &["/api/health"];

pub async fn require_auth(
    State(state): State<ApiState>,
    req: Request,
    next: Next,
) -> Response {
    let path = req.uri().path().to_string();
    if ALLOW_PATHS.iter().any(|p| path == *p) {
        return next.run(req).await;
    }
    let header_val = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok());
    let expected = format!("Bearer {}", state.auth_token);
    match header_val {
        Some(h) if h == expected => next.run(req).await,
        Some(_) => (StatusCode::UNAUTHORIZED, "bad token").into_response(),
        None    => (StatusCode::UNAUTHORIZED, "missing Authorization header").into_response(),
    }
}
```

- [ ] **Step 2: 挂到 router**

`http/mod.rs::build_router` 里在所有路由之后、`.layer(cors)` 之前加 `.layer(axum::middleware::from_fn_with_state(state.clone(), auth::require_auth))`。

- [ ] **Step 3: 集成测试**

```rust
// tests/http_routes.rs
#[tokio::test]
async fn protected_route_returns_401_without_token() {
    let h = build_state_with_token("test-token-123").await;
    let resp = router(h.state.clone())
        .oneshot(Request::builder().uri("/api/doujinshi/search").body(Body::empty()).unwrap())
        .await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn protected_route_returns_401_with_wrong_token() {
    let h = build_state_with_token("test-token-123").await;
    let resp = router(h.state.clone())
        .oneshot(
            Request::builder()
                .uri("/api/doujinshi/search")
                .header("Authorization", "Bearer wrong-token")
                .body(Body::empty()).unwrap()
        ).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn protected_route_returns_200_with_correct_token() {
    let h = build_state_with_token("test-token-123").await;
    let resp = router(h.state.clone())
        .oneshot(
            Request::builder()
                .uri("/api/doujinshi/search")
                .header("Authorization", "Bearer test-token-123")
                .body(Body::empty()).unwrap()
        ).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn health_route_is_exempt_from_auth() {
    let h = build_state_with_token("test-token-123").await;
    let resp = router(h.state.clone())
        .oneshot(Request::builder().uri("/api/health").body(Body::empty()).unwrap())
        .await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}
```

`build_state_with_token` helper 在 `tests/common/mod.rs` 加：

```rust
pub async fn build_state_with_token(token: &str) -> Harness {
    let mut h = build_state().await;
    h.state.auth_token = Arc::new(token.to_string());
    h
}
```

- [ ] **Step 4: 跑**

`cd src-tauri && cargo test --test http_routes` → 全绿（包括旧 9 个 + 新 4 个 = 13 个）。

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/http/auth.rs src-tauri/src/http/mod.rs src-tauri/tests/
git commit -m "feat(http): bearer-token middleware (health exempt)"
```

---

## Task 4: port_allocator 重试

**Files:**
- Modify: `src-tauri/src/http/mod.rs`（`build_router` 内部）

- [ ] **Step 1: 抽 port_allocator 函数**

```rust
// http/mod.rs（或者新建 http/port_allocator.rs）
use std::net::TcpListener;
use std::time::Duration;

pub fn bind_with_retry(preferred: u16, max_attempts: u32) -> std::io::Result<(TcpListener, u16, bool)> {
    // 返回 (listener, actual_port, used_fallback)
    // 1. 尝试 preferred（如果非 0）
    // 2. 失败 → sleep 100ms × N 再试，最多 max_attempts 次
    // 3. 全部失败 → 回退到 127.0.0.1:0
    // 4. 全部失败（包括 fallback）→ 返回 last error
}
```

逻辑伪代码：

```rust
pub fn bind_with_retry(preferred: u16, max_attempts: u32) -> std::io::Result<(TcpListener, u16, bool)> {
    if preferred != 0 {
        for attempt in 0..max_attempts {
            match TcpListener::bind(("127.0.0.1", preferred)) {
                Ok(l) => return Ok((l, preferred, false)),
                Err(e) if attempt + 1 < max_attempts => {
                    tracing::warn!("port {} bind failed ({}), retry {}/{}", preferred, e, attempt + 1, max_attempts);
                    std::thread::sleep(Duration::from_millis(100 * (attempt as u64 + 1)));
                }
                Err(e) => {
                    tracing::warn!("port {} bind failed {} times, falling back to random: {}", preferred, max_attempts, e);
                    let l = TcpListener::bind(("127.0.0.1", 0))?;
                    return Ok((l, l.local_addr()?.port(), true));
                }
            }
        }
    }
    let l = TcpListener::bind(("127.0.0.1", 0))?;
    Ok((l, l.local_addr()?.port(), true))
}
```

- [ ] **Step 2: 替换 `build_router` 中的 bind 调用**

- [ ] **Step 3: 单测**

```rust
// http/port_allocator.rs tests
#[test]
fn bind_with_retry_returns_preferred_when_available() {
    let taken = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = taken.local_addr().unwrap().port();
    drop(taken);
    let (l, actual, used_fallback) = bind_with_retry(port, 3).unwrap();
    assert_eq!(actual, port);
    assert!(!used_fallback);
    drop(l);
}

#[test]
fn bind_with_retry_falls_back_when_preferred_taken() {
    let taken = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = taken.local_addr().unwrap().port();
    // 不 drop taken — 占用端口
    let (_l, actual, used_fallback) = bind_with_retry(port, 2).unwrap();
    assert_ne!(actual, port);
    assert!(used_fallback);
}
```

- [ ] **Step 4: 跑测试**

`cd src-tauri && cargo test port_allocator::` → 2 passed。

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/http/
git commit -m "feat(http): port_allocator retry with fallback to random"
```

---

## Task 5: 后端暴露 token / port 给前端

**Files:**
- Modify: `src-tauri/src/commands/settings.rs`
- Modify: `src-tauri/src/http/mod.rs`

- [ ] **Step 1: SettingsView 增字段**

```rust
#[derive(Debug, Serialize)]
pub struct SettingsView {
    // ... 既有字段
    pub auth_token: String,
    pub http_port: u16,
    pub http_port_locked: bool,  // 用户是否锁了固定端口
}
```

- [ ] **Step 2: 命令补齐**

```rust
#[tauri::command]
pub async fn regenerate_auth_token(state: State<'_, AppState>) -> AppResult<String> {
    let new = crate::http::auth_token::generate();
    crate::db::setting::set(&state.conn, "auth_token", &new).await?;
    // 通知 ApiState 更新内存中的 token（重启前不生效，因为 axum 用的还是旧 Arc<String>）
    Ok(new)
}

#[tauri::command]
pub async fn set_http_port(state: State<'_, AppState>, port: u16) -> AppResult<()> {
    crate::db::setting::set(&state.conn, "http_port", &port.to_string()).await?;
    Ok(())
}
```

- [ ] **Step 3: 提交**

```bash
git add src-tauri/src/commands/settings.rs
git commit -m "feat(settings): expose auth_token + http_port to frontend"
```

---

## Task 6: 前端 SettingsView 三区块

**Files:**
- Modify: `src/views/SettingsView.vue`
- Modify: `src/types/api.ts`

- [ ] **Step 1: types 增字段**

```typescript
export interface SettingsView {
  // ... 既有
  auth_token: string
  http_port: number
  http_port_locked: boolean
}
```

- [ ] **Step 2: SettingsView 增 3 个 n-card**

在「HTTP API」卡片之前插入：

```vue
<n-card title="HTTP 端口">
  <n-space align="center">
    <n-input-number
      v-model:value="portInput"
      :min="0"
      :max="65535"
      :disabled="!store.data?.http_port_locked"
      placeholder="0 = 随机"
      style="width: 140px"
    />
    <n-switch v-model:value="portLocked" />
    <span style="color: #aaa; font-size: 12px">{{ portLocked ? '固定端口' : '随机端口' }}</span>
    <n-button type="primary" size="small" :disabled="!store.data" @click="savePort">
      保存（重启后生效）
    </n-button>
  </n-space>
</n-card>

<n-card title="HTTP Token">
  <p style="color: #aaa; font-size: 12px">
    浏览器扩展和外部脚本调用 HTTP API 时需要带这个 Token。重新生成后老 Token 立刻失效。
  </p>
  <n-space align="center">
    <n-code :code="store.data?.auth_token ?? ''" style="flex: 1" />
    <n-button size="small" @click="copy(store.data?.auth_token ?? '')">复制</n-button>
    <n-button size="small" type="warning" @click="regenToken">重新生成</n-button>
  </n-space>
</n-card>

<n-card title="Inbox 目录">
  <n-input :value="store.data?.inbox_dir" readonly style="margin-bottom: 8px" />
  <p style="color: #aaa; font-size: 12px">
    改路径需要修改配置文件 <code>resources/_config.toml</code> 后重启应用。当前版本不支持热修改。
  </p>
</n-card>
```

Script 部分加：

```typescript
import { invoke } from "@tauri-apps/api/core"

const portInput = ref<number>(0)
const portLocked = ref(false)
watch(() => store.data, (d) => {
  if (d) {
    portInput.value = d.http_port
    portLocked.value = d.http_port_locked
  }
})

async function savePort() {
  await invoke("set_http_port", { port: portInput.value })
  message.success("已保存，重启后生效")
}

async function regenToken() {
  const newToken = await invoke<string>("regenerate_auth_token")
  await store.load()
  message.success("Token 已重新生成")
  await copy(newToken)
}
```

- [ ] **Step 3: 提交**

```bash
git add src/views/SettingsView.vue src/types/api.ts
git commit -m "feat(settings): 3 new cards (port / token / inbox dir)"
```

---

## Task 7: apiClient 统一注入 token

**Files:**
- Create: `src/api/http.ts`（新的 HTTP client，区别于 `api/tauri.ts`）
- Modify: `src/views/LibraryView.vue`、`InboxView.vue`、`SettingsView.vue`、`ConflictView.vue`（#1）所有用 `fetch(apiBase + ...)` 的地方

- [ ] **Step 1: apiClient**

```typescript
// src/api/http.ts
import { useSettingsStore } from "@/stores"

export async function apiGet<T>(path: string): Promise<T> {
  const settings = useSettingsStore()
  await settings.load()  // 确保 token 已加载
  const resp = await fetch(settings.apiBase + path, {
    headers: {
      "Authorization": `Bearer ${settings.data?.auth_token ?? ""}`,
    },
  })
  if (!resp.ok) throw new Error(`HTTP ${resp.status}: ${await resp.text()}`)
  return await resp.json() as T
}

export async function apiPost(path: string): Promise<Response> {
  const settings = useSettingsStore()
  await settings.load()
  return await fetch(settings.apiBase + path, {
    method: "POST",
    headers: {
      "Authorization": `Bearer ${settings.data?.auth_token ?? ""}`,
    },
  })
}
```

- [ ] **Step 2: 替换所有裸 fetch**

```typescript
// 旧：
const resp = await fetch(settings.apiBase + "/api/doujinshi/search?q=" + q)
// 新：
const data = await apiGet<SearchResult>(`/api/doujinshi/search?q=${encodeURIComponent(q)}`)
```

- [ ] **Step 3: 提交**

```bash
git add src/api/http.ts src/views/*.vue src/stores/*.ts
git commit -m "refactor(frontend): unified apiClient with bearer token"
```

---

## Task 8: E2E + 回归

- [ ] **Step 1:** `cd src-tauri && cargo test` 全绿
- [ ] **Step 2:** `pnpm lint && pnpm build` 全绿
- [ ] **Step 3:** E2E：启动 app → SettingsView 看到 token 卡片显示 token → 浏览器扩展调用 `curl -H "Authorization: Bearer <token>" http://127.0.0.1:<port>/api/doujinshi/search` 返回 200 → 不带 header 调返 401 → /api/health 不带 header 也返 200
- [ ] **Step 4:** E2E：端口被占 → 看日志有 retry 警告，第三次后 fallback 到随机端口

---

## Self-review

- [ ] 所有非 `/api/health` 路由都 401（无 token），200（有正确 token）
- [ ] 端口占用重试 3 次后 fallback
- [ ] Token 重新生成后立刻生效（注意：当前实现是重启后才生效；要看是否需要热更新）
- [ ] SettingsView 三区块都能保存
- [ ] 前端所有 fetch 走 apiClient，自动带 token
- [ ] 没有破坏现有 `api/tauri.ts`（Tauri 命令不走 HTTP，仍用 invoke）