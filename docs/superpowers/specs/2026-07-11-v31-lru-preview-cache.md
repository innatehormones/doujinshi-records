# V3.1 Spec — LRU Preview Cache（HTTP images 端点响应体缓存）

> 日期：2026-07-11
> 状态：draft（待用户 review）
> 范围：**V3.1 第一步**——给 `GET /api/doujinshi/<id>/images` 加 LRU 磁盘 + 内存缓存
> 后续：V3.1 第二步是 gallery-style detail page，复用本 spec 的 cache 层做预加载

## 目标

V3 详情页打开一本 zip（如 200 页同人志）调用 `GET /api/doujinshi/<id>/images`，每次都重新解压 + base64 编码 ~200 张图，单次 50–200 ms；同一本 zip 在 session 内重复浏览累计耗时可见。V3.1 解决：

1. **响应体级缓存**——同一 zip 的 images 响应整体复用，不重复解压
2. **磁盘长缓存**——重启后不丢，下次打开秒级
3. **磁盘占空可控**——上限 200 MB（可调），LRU 自动 evict
4. **HTTP 友好**——配 ETag，让浏览器扩展能少传一次

## 非目标

- **gallery-style detail page**——V3.1 第二步；后端 cache 准备好前端不动
- **图片解码缩略图级缓存**——单个图片的 pre-resized 缩略图；本 spec 只缓存整个 images 响应
- **跨设备/跨进程缓存共享**——单进程 LRU，多进程同目录不会自动协调
- **缓存预热**——用户首次打开才解压进 cache；不主动后台跑

## 核心模型

### 缓存 key

`(i64 file_id, SystemTime zip_mtime_unix)`

- `file_id` 定位 doujinshi_file 行（与 zip 路径一一对应，状态转移由 state_machine 维护）
- `mtime` 兜底：用户手工替换 zip、移动后可能同名但 mtime 不同；mtime 一变 → 自动重建
- 不存 zip 内容 hash，避免每次缓存查找多读一遭 zip

### 缓存值

序列化后的 `ImagesResponse` JSON（与现在的端点返回同 schema）：

```
CacheEntry {
  key: (i64, SystemTime),
  body: Bytes,           // serde_json::to_vec(&ImagesResponse)
  size: u64,             // body.len()
  last_accessed: Instant,
}
```

只缓存解码后的 JSON，原始 zip 数据不持久化（避免缓存体积膨胀）。

### 失效

- 自动失效：cache key 中的 `mtime` 与磁盘 zip `metadata().modified()` 不一致 → 算 miss，重建
- 不需要主动调 invalidation：state_machine 转移文件时会改 mtime（文件被 rename，OS 给新 inode，新 mtime）
- LRU 容量超限时 evict 最久未访问

## 架构

### 模块

`src-tauri/src/services/preview_cache.rs`（新文件）

```
pub struct PreviewCache {
  inner: Mutex<LruCache<CacheKey, CacheEntry>>,
  max_bytes: AtomicU64,         // 默认 200 * 1024 * 1024
  bytes_in_cache: AtomicU64,
  dir: PathBuf,                // _preview_cache/
}

impl PreviewCache {
  pub fn new(dir: PathBuf, max_bytes: u64) -> Self
  pub async fn get_or_compute<F>(&self, key: CacheKey, compute: F) -> Result<Bytes>
    where F: FnOnce() -> Result<Bytes>
  pub fn peek(&self, key: &CacheKey) -> Option<&CacheEntry>
  pub fn invalidate(&self, id: i64)
  pub async fn gc(&self) -> Result<()>   // drain to 80% of max_bytes
  pub fn on_disk_total(&self) -> u64
}
```

`AppState` 持 `Arc<PreviewCache>`；构造函数从 `cfg.preview_cache_dir()` + 默认 200 MB 初始化。

### 启动时构建内存 LRU

`PreviewCache::new` 阶段：

1. 确保 `_preview_cache/` 目录存在
2. Walk 该目录：每个 `<file_id>-<mtime>.json` 文件 parse 文件名拿到 key
3. stat 文件得 size；读 body 入内存 LRU
4. 跳过损坏文件（parse 失败 → 删除磁盘文件）

启动 GC 不主动跑——只是把磁盘文件读进 LRU map，磁盘 inode + 文件 size 已是最权威账本；超限 evict 等下次插入或后台任务触发。

### HTTP 层接入

`GET /api/doujinshi/<id>/images` 改写：

```
handler(id):
  row = SELECT * FROM doujinshi_file WHERE id = ?
  if !row: 404
  zip_path = row.current_path
  if !zip_path.exists():
    return 200 { images: [], zip_missing: true }
  
  mtime = zip_path.metadata().modified()?
  etag = format!("\"{id}-{}\"", mtime_unix)
  
  if request.if_none_match == etag:
    return 304 Not Modified（空 body + 仍带 ETag header）
  
  key = (id, mtime)
  if let Some(entry) = cache.peek(&key):
    response: 200 + ETag header + entry.body bytes
  else:
    images = list_images(zip_path)?   // 现有 archive::list_images
    body = serde_json::to_vec(&ImagesResponse { id, images, zip_missing: false })
    cache.get_or_compute(key, || Ok(body))  // 触发 disk write + LRU insert
    return 200 + ETag header + body
```

`/api/covers/*` 路径不改（本 spec 只动 images 端点）。

### 磁盘布局

`resources/_preview_cache/`

- 单文件 per entry：`<file_id>-<mtime_unix>.json`（文件名约定，不放 metadata）
- body 与文件名 mtime 必须一致；构造 `CacheKey` 时校验
- 写入：tmp 临时文件名 + `std::fs::rename` 原子替换
- 清理：LRU evict 删对应文件

### 后台 GC

`tauri::async_runtime::spawn` 一个 task，每 30 秒跑一次：

```
loop {
  tokio::time::sleep(Duration::from_secs(30)).await;
  cache.gc().await.ok();
}
```

`gc()` 行为：
- 如果 `bytes_in_cache > max_bytes`：持续 pop LRU 最老 entry + 删磁盘文件，直到 < `0.8 * max_bytes`
- 否则 no-op

后台 GC 只在 Tauri runtime 持有；HTTP server 在独立线程（见 lib.rs `http::build_router`），它持有 `Arc<PreviewCache>` 可以直接调用 `get_or_compute` 触发读路径上的 inline evict。后台 GC 是兜底，不阻塞请求。

### 配置

`AppConfig` 增：

```
pub preview_cache_max_bytes: u64  // 200 * 1024 * 1024 默认
```

`AppConfig::load()` 不读 TOML——目前 AppConfig 都是程序内 hardcode；本字段也 hardcode 默认值，留接口供后续 settings 页调整。

### 前端

**不动**。V3.1 第二步（gallery detail）才会改动 DetailView.vue；本 spec 仅后端。

## 错误处理

| 情况 | 行为 |
|---|---|
| zip 路径不存在 / 已删除 | cache 不写盘；handler 返回 `zip_missing: true`（现状） |
| zip mtime 无法读取 | cache 不命中；`list_images` 失败也向上抛 HTTP 500（同 V2） |
| 磁盘 cache 写失败 | log warn + 当前请求仍然返回正确响应；下次会重新计算 |
| 磁盘 cache 文件损坏（parses failed） | 启动时删掉，不阻塞启动 |
| 启动时 `_preview_cache/` 不存在 | 视为首次运行，`new()` 时 mkdir |

## 测试

`src-tauri/tests/preview_cache.rs`（新文件）+ 模块内 `#[cfg(test)]` 单元测试：

| 场景 | 断言 |
|---|---|
| `get_or_compute` miss 触发 compute + 写盘 + 命中 | 文件存在 + body bytes 等长 |
| `peek` 命中读内存 | 不调 compute |
| mtime 变化后同一 file_id → miss | 重 compute |
| 容量超限 → 淘汰最老 entry（磁盘 + 内存都删） | bytes_in_cache < max_bytes |
| `invalidate(id)` | 删除所有 key 含此 id 的 entry |
| ETag 304：handler 收到 `If-None-Match: "<id>-<mtime>"` | 返回 304 空体 |
| 启动 scan 重建 LRU（写 2 个文件，`new()` 后 LRU 内有 2 项） | `peek` 都命中 |
| 损坏文件（手工写入垃圾）启动时不阻塞 | 损坏文件被删，good 仍 hit |

## 上线影响

- **磁盘占用**：200 MB 软上限，最坏情况同馆藏 zip 总和（大馆藏 > 200 MB 的话冷淘汰）
- **首次打开延迟不变**：仍要走 `list_images` 解压
- **重复打开延迟**：从 ~50–200 ms → ~5 ms（命中纯读 + axum JSON 序列化）
- **浏览器扩展**：ETag `If-None-Match` 304 后 0 字节，节省带宽
- **使用 `lru` crate**（Cargo 加一行 dep），不自己实现双链表 LRU
