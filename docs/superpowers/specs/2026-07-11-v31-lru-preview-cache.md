# V3.1 Spec — Preview Cache + Lazy Gallery + Fullscreen Preview

> 日期：2026-07-11（初稿）→ 2026-07-12（落地修订）
> 状态：已实现
> 范围：详情页（DetailView）从一次性渲染 200+ 张原图改成"按需取 + Worker 转码 + LRU 缓存 + 全屏预览"

## 目标

V3 详情页打开一本 zip（如 200 页同人志）一次性挂载 200+ 个 `<n-image>`，WebView2 并行解码原图 + n-image 内部 wrapper 直接拖垮 UI；同本 zip 重复打开累计耗时可观。V3.1 解决：

1. **按需取**——cell 进入视口才发请求；不可见不加载
2. **后端转 webp 缓存**——Worker 缩 800px webp → PUT 落 LRU；二次进入直接读 webp
3. **磁盘长缓存**——LRU 200 MiB 上限，重启不丢
4. **LRU/磁盘一致性**——`/images` 端点的 `thumb_cached` 字段以"磁盘有文件"为最终依据，不只看内存 LRU
5. **全屏预览**——自封装，不放大、不取原图，键盘 ← → Esc 翻页关闭

## 非目标

- **跨设备/跨进程缓存共享**——单进程 LRU，多进程同目录不会自动协调
- **缓存预热**——用户首次进入才生成缩略图；不主动后台跑
- **放大查看原图**——预览只展示 ≤800px webp，不再请求 `/original` 路由（路由已删）

## 核心模型

### 缓存 key

`(i64 file_id, usize image_index)`

- `file_id` 定位 doujinshi_file 行；zip 重识别换 file_id 即自动失效
- `image_index` 索引 zip 内图片（0-based）
- 文件命名 `<id>-<idx>.webp` 让 `reload_from_disk` 能反查 key 重建内存 LRU
- 不存 zip mtime：单个图片变更不影响其它图片

### 缓存值

`Vec<u8>`——单图转码后的 webp bytes（≤800px q=0.7，约 30–150 KB/张）

### 失效

- **重启**：`PreviewCache::new` 扫盘 → `reload_from_disk` 重建 LRU；损坏文件自动删除
- **Eviction**：`bytes_in_cache > max_bytes * 80%` 时 LRU 弹出 + 删磁盘文件
- **状态机转移**：`state_machine::transition_with_dirs` 后调用 `preview_cache.invalidate(id)`（rename 后该 id 旧图片可能不存在）
- **API 路径**：`/api/doujinshi/:id/images/:index/thumb`（PUT）幂等——已存在 key 时直接 NO_CONTENT 不覆盖（避免重 PUT 浪费 worker）

## 架构

### 后端模块

`src-tauri/src/services/preview_cache.rs`

```
pub struct PreviewCache {
  inner: Mutex<LruCache<CacheKey, CacheEntry>>,
  max_bytes: AtomicU64,           // 默认 200 * 1024 * 1024
  bytes_in_cache: AtomicU64,
  dir: PathBuf,                  // resources/_preview_cache/
}

impl PreviewCache {
  pub fn new(dir: &Path, max_bytes: u64) -> Result<Self>
  pub fn contains(&self, key: &CacheKey) -> bool         // 仅内存 LRU
  pub fn is_on_disk(&self, key: &CacheKey) -> bool       // 磁盘兜底
  pub fn get(&self, key: &CacheKey) -> Option<Vec<u8>>
  pub async fn insert(&self, key: CacheKey, body: Vec<u8>) -> Result<()>
  pub async fn gc(&self) -> Result<()>                   // 后台任务兜底
  pub fn invalidate(&self, id: i64)
}
```

`ApiState.preview_cache: Arc<PreviewCache>`；lib.rs 启动时 `PreviewCache::new(&cfg.preview_cache_dir(), cfg.preview_cache_max_bytes)`，每 30s 跑一次 `gc()`。

### HTTP 路由

| 路由 | 行为 |
|---|---|
| `GET /api/doujinshi/:id/images` | 列表。每项含 `thumb_cached = contains OR is_on_disk`；`Cache-Control: no-store` |
| `GET /api/doujinshi/:id/images/:index` | 单图。cache hit 吐 webp；miss 走 `raw_image_response` 直返原图（mime 按 magic bytes 探测）|
| `PUT /api/doujinshi/:id/images/:index/thumb` | 落盘。body 必须是 webp；空体 400；非 webp 400；LRU 已存在 → NO_CONTENT 幂等 |

### `is_on_disk` 的必要性

`/images` 端点只查 `contains`（内存 LRU）会出现"假 miss"——eviction 删盘失败、测试残留、启动 reload 期间文件被改等都会让 LRU 与磁盘不一致。前端据此判断"是否要跑 Worker"，假 miss 会重复跑 Worker 浪费带宽。`is_on_disk` 是兜底。

### 磁盘布局

`resources/_preview_cache/`

- 单文件 per entry：`<file_id>-<image_index>.webp`
- 写入：tmp 临时文件名 + `std::fs::rename` 原子替换
- 清理：LRU evict 删对应文件

### `/original` 路由删除

V3 spec 留的"点开看原图"路由（`/api/doujinshi/:id/images/:index/original`）已删除：
- 全屏预览不再放大，统一 800px webp
- `image_original_at` handler 删；router 两处注册删；`http_routes.rs` 测试删

## 前端

### 缩略图渲染管线

```
DetailView
  ├─ composables/useThumbnailPipeline.ts
  │    ├─ Worker 调度（queue / inFlight / 并发 2）
  │    ├─ blob URL 生命周期（createObjectURL / revoke）
  │    └─ PUT 落盘（putImageThumb）
  ├─ IntersectionObserver（rootMargin 420px 预读上下各 ~2 行）
  │    └─ 进入视口 → pipeline.request(index)
  └─ Template
       ├─ <div class="thumb-skeleton">       (永远渲染，占底)
       └─ <img :src="..." class="thumb-img" :class="{ 'thumb-img-loaded': loaded.has(idx) }">
```

`request(index)` 决策：

| `thumb_cached` | Worker 可用 | 行为 |
|---|---|---|
| true | * | 直挂 `apiBase + /api/doujinshi/:id/images/:idx` |
| false | true | 入队 Worker（fetch 原图 → OffscreenCanvas 缩放 → webp → PUT 落盘 → blob URL 展示） |
| false | false | 降级：直挂后端图（cache miss 时返原图 mime） |

### 防闪烁

`<img>` 挂载瞬间 src 未解码完 → 浏览器显示空 + 黑色背景 → 用户看到"灰骨架 → 黑 → 图"三段闪烁。

修法：骨架 div 永远占底，`<img>` `position:absolute` 覆盖，`opacity:0`；onLoad 触发 `markLoaded(idx)` → 切 `.thumb-img-loaded` class → opacity 过渡到 1。挂载到 onLoad 期间骨架一直可见，**没有黑色中断**。

### 全屏预览组件

`src/components/FullscreenPreview.vue`

- 弹层遮罩（`rgba(0,0,0,0.88)`）；右上 × 按钮关闭
- **点击遮罩空白不关闭**（避免误触；仅 Esc / × 关）
- 左右按钮 / 键盘 ←→ 翻页；Esc 关闭；输入框聚焦时 ←→ 让位给光标
- 计数器 `n / total`
- 预读左右各 1 张（隐藏 `<img>` 让浏览器提前建连/缓存）

每张图就是 `/api/doujinshi/:id/images/:idx`——cache hit 吐 webp，miss 吐原图 mime。不再单独请求 `/original`。

## 错误处理

| 情况 | 行为 |
|---|---|
| Worker fetch 失败 | 退回展示后端图（cache miss 走原图 mime） |
| Worker 编码失败 | 同上 |
| PUT 落盘失败 | 静默吞（`.catch(() => {})`），blob URL 仍可展示当前次 |
| 切走文件后 PUT 返回 | 不动 images 数组（避免视图抖动） |
| zip 路径不存在 | `/images` 返 `zip_missing: true`；前端展示 alert |

## 验证

后端：

```bash
cd src-tauri && cargo test --test http_routes
# 36/36 通过（含 thumb_cached 反映磁盘、Cache-Control: no-store、PUT 幂等）
```

前端：

```bash
pnpm exec vue-tsc --noEmit
# 0 error
```

手动：

1. 清空 `_preview_cache/` 后进入大图详情页——Network 应**不**同时加载 200+ 个 `<img>`
2. 滚动——新 cell 进入视口时渐进加载（IntersectionObserver 触发）
3. 点击缩略图——全屏预览，键盘 ←→ Esc 工作
4. 第二次进入同详情页——`/images` 返 `thumb_cached: true`，Worker 不重复转码
5. 切文件 / 退出——blob URL revoke、worker.terminate，无内存泄漏

## 文件清单

新增：
- `src/composables/useThumbnailPipeline.ts`（Worker 调度封装）
- `src/composables/usePreviewState.ts`（预览 open/index 状态机）
- `src/components/FullscreenPreview.vue`（全屏预览组件）
- `src/workers/previewThumb.worker.ts`（OffscreenCanvas 转码 Worker）

修改：
- `src-tauri/src/services/preview_cache.rs`（增 `is_on_disk`）
- `src-tauri/src/http/api.rs`（`/images` 用 `contains OR is_on_disk` + `Cache-Control: no-store`；删 `image_original_at`）
- `src-tauri/src/http/mod.rs`（删 `/original` 注册）
- `src-tauri/tests/http_routes.rs`（删 `/original` 测试）
- `src/views/DetailView.vue`（去 `<n-image>`、加 IO 懒加载、用 composable）

## 上线影响

- **磁盘占用**：200 MiB 软上限（约 2000–6000 张 800px webp）
- **首次进入延迟**：可见区 cell 立即请求原图 → 后端直返原图 mime；Worker 并发 2 转码 → 后台 PUT 落盘
- **重复进入延迟**：可见区 cell 立即返回 webp，浏览器瞬间解码
- **不再支持"放大查看原图"**——如果后续要回，加 `/original` 路由即可（handler / router 都很短）