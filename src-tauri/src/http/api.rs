use crate::db::entities::doujinshi_file;
use crate::http::ApiState;
use crate::models::file_summary;
use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use sea_orm::{ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder, QuerySelect};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::time::SystemTime;

pub async fn health() -> Json<serde_json::Value> {
    Json(json!({ "status": "ok", "version": "0.1.0" }))
}

#[derive(Deserialize)]
pub struct SearchParams {
    pub q: Option<String>,
    pub status: Option<String>,
    pub limit: Option<u64>,
    pub offset: Option<u64>,
}

pub async fn search(
    State(s): State<ApiState>,
    Query(p): Query<SearchParams>,
) -> Json<serde_json::Value> {
    let mut q = match p.status.as_deref() {
        // 物理删除记录：放行不隐。
        Some("physically_deleted") => doujinshi_file::Entity::find()
            .filter(doujinshi_file::Column::PhysicallyDeleted.eq(true)),
        _ => doujinshi_file::Entity::find()
            .filter(doujinshi_file::Column::PhysicallyDeleted.eq(false)),
    };
    if let Some(text) = p.q.as_deref().filter(|s| !s.is_empty()) {
        let pat = format!("%{}%", text);
        q = q.filter(
            doujinshi_file::Column::Title
                .like(&pat)
                .or(doujinshi_file::Column::Circle.like(&pat))
                .or(doujinshi_file::Column::Filename.like(&pat)),
        );
    }
    if let Some(st) = p.status.as_deref() {
        q = match st {
            "physically_deleted" => q, // 上面已用，不再叠加过滤
            "identified" | "will_delete" | "archived" => {
                q.filter(doujinshi_file::Column::CurrentLocation.eq(st))
            }
            _ => q,
        };
    }
    let limit = p.limit.unwrap_or(50);
    let offset = p.offset.unwrap_or(0);
    let total: u64 = q.clone().count(&s.conn).await.ok().unwrap_or(0);
    let rows = q
        .order_by_desc(doujinshi_file::Column::CreatedAt)
        .limit(limit)
        .offset(offset)
        .all(&s.conn)
        .await
        .unwrap_or_default();
    let ids: Vec<i64> = rows.iter().map(|m| m.id).collect();
    let conflict_map = file_summary::open_conflict_map(&s.conn, &ids).await;
    let items: Vec<file_summary::FileSummary> = rows
        .iter()
        .map(|m| {
            let has = conflict_map.get(&m.id).copied().unwrap_or(false);
            file_summary::from_model_with_conflict_state(m, has)
        })
        .collect();
    Json(json!({ "items": items, "total": total }))
}

pub async fn by_hash(
    State(s): State<ApiState>,
    Path(hash): Path<String>,
) -> Json<serde_json::Value> {
    let row = doujinshi_file::Entity::find()
        .filter(doujinshi_file::Column::Hash.eq(&hash))
        .one(&s.conn)
        .await
        .unwrap_or(None);
    match row {
        Some(m) => Json(json!(file_summary::from_model(&s.conn, &m).await)),
        None => Json(json!(null)),
    }
}

pub async fn by_id(State(s): State<ApiState>, Path(id): Path<i64>) -> impl IntoResponse {
    let row = doujinshi_file::Entity::find_by_id(id)
        .one(&s.conn)
        .await
        .unwrap_or(None);
    match row {
        Some(m) => (StatusCode::OK, Json(json!(file_summary::from_model(&s.conn, &m).await))).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

pub async fn cover(State(s): State<ApiState>, Path(hash): Path<String>) -> impl IntoResponse {
    let row = doujinshi_file::Entity::find()
        .filter(doujinshi_file::Column::Hash.eq(&hash))
        .one(&s.conn)
        .await
        .unwrap_or(None);
    let Some(m) = row else {
        return (StatusCode::NOT_FOUND, "no file").into_response();
    };
    let Some(rel) = m.cover_path.clone() else {
        return (StatusCode::NOT_FOUND, "no cover").into_response();
    };
    let candidates = [
        s.covers_dir.join(&rel),
        s.covers_dir.join(rel.trim_start_matches("covers/")),
        s.covers_dir.join(rel.trim_start_matches("/")),
    ];
    for abs in &candidates {
        if let Ok(bytes) = tokio::fs::read(abs).await {
            return ([(header::CONTENT_TYPE, cover_mime(&bytes))], bytes).into_response();
        }
    }
    // Row exists but the cover file is missing on disk — serve a
    // transparent PNG so the frontend <img> never gets an error event.
    crate::http::placeholder::placeholder_response().into_response()
}

// ===== V2 endpoints =====

#[derive(Deserialize)]
pub struct CheckParams {
    pub hash: String,
}

/// `GET /api/doujinshi/check?hash=<blake3>` — friendly alias for
/// `by-hash` exposed for browser-extension callers ("have I seen this
/// hash before?"). Returns the same shape: `FileSummary | null`.
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
        Some(m) => Json(json!(file_summary::from_model(&s.conn, &m).await)),
        None => Json(json!(null)),
    }
}

/// `GET /api/doujinshi/:id/images/:index/thumb` — frontend uploads a
/// pre-converted 800px webp to the LRU. Idempotent: 204 if the file
/// already exists on disk.

/// `GET /api/covers/by-hash/:hash` — same handler body as `/api/covers/:file_id`
/// but mounted at a hash-keyed path so the browser extension can fetch a
/// cover without first knowing the internal row id.
pub async fn cover_by_hash(
    State(s): State<ApiState>,
    Path(hash): Path<String>,
) -> impl IntoResponse {
    cover(State(s), Path(hash)).await
}

// ===== V2 Conflict Compare =====

#[derive(Serialize)]
pub struct CompareSide {
    pub file_id: i64,
    pub title: String,
    pub hash: Option<String>,
    pub cover_url: Option<String>,
    pub image_names: Vec<String>,
    /// Absolute path on disk. A side reads this from
    /// `doujinshi_file.current_path`; B side from `conflict.b_file_path`.
    pub file_path: String,
    /// True when the archive file no longer exists on disk.
    pub zip_missing: bool,
    /// Set when the archive is on disk but couldn't be parsed
    /// (e.g. corrupt zip, or non-zip extension in V1).
    pub zip_error: Option<String>,
}

#[derive(Serialize)]
pub struct ConflictCompare {
    pub conflict_id: i64,
    pub a: CompareSide,
    pub b: CompareSide,
}

/// `GET /api/conflicts/:id/compare` — return both sides of a
/// conflict: the already-identified row (A) and the new zip still
/// sitting in the inbox (B). Used by the ConflictView page so the
/// user can decide which copy to keep.
pub async fn compare(State(s): State<ApiState>, Path(conflict_id): Path<i64>) -> impl IntoResponse {
    use crate::db::entities::conflict::Entity as ConflictEntity;

    let row = match ConflictEntity::find_by_id(conflict_id).one(&s.conn).await {
        Ok(Some(r)) => r,
        Ok(None) => return (StatusCode::NOT_FOUND, "conflict not found").into_response(),
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };

    // A side: look up the already-identified file by a_file_id.
    let a_row = doujinshi_file::Entity::find_by_id(row.a_file_id)
        .one(&s.conn)
        .await
        .unwrap_or(None);
    let a = match a_row {
        Some(m) => {
            let (names, missing, err) = read_image_names(&m.current_path);
            let hash = m.hash.clone();
            CompareSide {
                file_id: m.id,
                title: m.title,
                hash: Some(hash.clone()),
                cover_url: cover_url_for(&hash),
                image_names: names,
                file_path: m.current_path.clone(),
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
            file_path: String::new(),
            zip_missing: false,
            zip_error: None,
        },
    };

    // B side: read the inbox file directly.
    let (names, missing, err) = read_image_names(&row.b_file_path);
    let b = CompareSide {
        file_id: 0,
        title: row.b_filename,
        hash: row.b_hash,
        cover_url: None,
        image_names: names,
        file_path: row.b_file_path.clone(),
        zip_missing: missing,
        zip_error: err,
    };

    (StatusCode::OK, Json(ConflictCompare { conflict_id, a, b })).into_response()
}

/// Returns `(names, missing, error_msg)`.
/// - `missing = true` → file not on disk
/// - `error_msg = Some(_)` → archive was present but could not be
///   parsed (corrupt, wrong format, etc.)
fn read_image_names(path: &str) -> (Vec<String>, bool, Option<String>) {
    let p = std::path::Path::new(path);
    if !p.exists() {
        return (vec![], true, None);
    }
    match crate::services::archive::list_image_names(p) {
        Ok(n) => (n, false, None),
        Err(e) => (vec![], false, Some(e.to_string())),
    }
}

/// Path-only cover URL — the SPA prepends `useSettingsStore.apiBase`.
fn cover_url_for(hash: &str) -> Option<String> {
    Some(format!("/api/covers/by-hash/{}", hash))
}

// ===== V2 DetailView =====

#[derive(Serialize)]
pub struct ImageEntry {
    pub name: String,
    /// Path-only image URL — frontend prepends `useSettingsStore.apiBase`.
    /// Individual bytes served by `GET /api/doujinshi/:id/images/:index`.
    pub url: String,
    /// True when `/images/:index` will hit preview_cache and return webp.
    pub thumb_cached: bool,
}

#[derive(Serialize)]
pub struct ImagesResponse {
    pub file_id: i64,
    pub images: Vec<ImageEntry>,
    /// `true` when the archive file no longer exists on disk — the
    /// SPA still gets a 200 so the carousel can render an alert
    /// instead of an error.
    pub zip_missing: bool,
}

/// `GET /api/doujinshi/:id/images` — return one URL per image inside
/// the archive. Bytes are served by the sibling
/// `/api/doujinshi/:id/images/:index` route so the SPA's `<img>`
/// tags can stream them directly instead of pulling the whole
/// archive in one base64 blob.
///
/// 响应体本身很小（几 KB），不做服务端缓存；扫 zip central directory
/// 拿文件名列表约 5ms。ETag 走 zip mtime 让浏览器可以 304 短路。
pub async fn images(
    State(s): State<ApiState>,
    Path(id): Path<i64>,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    use sea_orm::EntityTrait;
    let row = match doujinshi_file::Entity::find_by_id(id).one(&s.conn).await {
        Ok(Some(r)) => r,
        Ok(None) => return (StatusCode::NOT_FOUND, "no file").into_response(),
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };
    let path = std::path::Path::new(&row.current_path);
    if !path.exists() {
        return (
            StatusCode::OK,
            [(header::ETAG, format!("\"{}-missing\"", id))],
            Json(json!(ImagesResponse {
                file_id: id,
                images: vec![],
                zip_missing: true,
            })),
        )
            .into_response();
    }

    // mtime → ETag. Zip changed → mtime moved → cache miss; no manual invalidation needed.
    let mtime = match path.metadata().and_then(|m| m.modified()) {
        Ok(t) => t,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };
    let etag = format!(
        "\"{}-{}\"",
        id,
        mtime
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    );

    // If-None-Match → 304
    if let Some(if_none_match) = headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
    {
        if if_none_match == etag {
            return (StatusCode::NOT_MODIFIED, [(header::ETAG, etag.clone())]).into_response();
        }
    }

    // Compute: list names only (natural-sorted), build URL list. Bytes
    // are served by /api/doujinshi/:id/images/:index so the SPA can
    // stream them instead of pulling the whole archive in one base64
    // blob.  `list_image_names_sorted` is the public-listing sort so
    // the SPA's `images[i].name` matches the `i` it sends back to
    // `read_image_at`.
    let names = match crate::services::archive::list_image_names_sorted(path) {
        Ok(n) => n,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };
    let images: Vec<ImageEntry> = names
        .into_iter()
        .enumerate()
        .map(|(idx, name)| ImageEntry {
            url: format!("/api/doujinshi/{}/images/{}", id, idx),
            name,
            // LRU miss 但磁盘文件存在的情况：eviction 删盘失败 / 启动 reload
            // 期间文件被改 / 测试残留等都会让 LRU 与磁盘不一致。仅看 LRU 会
            // 让前端误以为未缓存，重复跑 Worker。
            thumb_cached: s.preview_cache.contains(&(id, idx))
                || s.preview_cache.is_on_disk(&(id, idx)),
        })
        .collect();
    let response = ImagesResponse {
        file_id: id,
        images,
        zip_missing: false,
    };

    (
        StatusCode::OK,
        [
            (header::ETAG, etag.as_str()),
            // thumb_cached 字段依赖磁盘文件存在与否（运行时变化），不能
            // 让浏览器缓存旧 body（304 短路会复用旧 thumb_cached 值，导致
            // 前端误判未缓存、重复跑 Worker）。
            (header::CACHE_CONTROL, "no-store"),
        ],
        Json(response),
    )
        .into_response()
}

/// `GET /api/doujinshi/:id/images/:index` — cache hit 直接吐 webp 缩略图；
/// cache miss 时不解码不转码，zip 解压原图直返（mime 按 magic bytes 探测）。
/// 缩略图 webp 由前端 canvas 转好后通过 `PUT .../thumb` 落 LRU。
/// Auth-exempt（`<img>` 直读）。
pub async fn image_at(
    State(s): State<ApiState>,
    Path((id, index)): Path<(i64, usize)>,
) -> impl IntoResponse {
    let key: crate::services::preview_cache::CacheKey = (id, index);
    let etag = format!("\"{}-{}\"", id, index);

    if let Some(bytes) = s.preview_cache.get(&key) {
        return webp_response(bytes, &etag);
    }

    match raw_image_response(&s, id, index).await {
        Ok(resp) => resp,
        Err(resp) => resp,
    }
}

async fn raw_image_response(
    s: &ApiState,
    id: i64,
    index: usize,
) -> Result<axum::response::Response, axum::response::Response> {
    use sea_orm::EntityTrait;
    let row = match doujinshi_file::Entity::find_by_id(id).one(&s.conn).await {
        Ok(Some(r)) => r,
        Ok(None) => return Err(StatusCode::NOT_FOUND.into_response()),
        Err(e) => return Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()),
    };
    let path = std::path::Path::new(&row.current_path);
    if !path.exists() {
        return Err(StatusCode::NOT_FOUND.into_response());
    }

    let path_owned = path.to_path_buf();
    let result = tokio::task::spawn_blocking(move || {
        crate::services::archive::read_image_at(&path_owned, index)
    })
    .await;

    let (_name, raw) = match result {
        Ok(Ok(v)) => v,
        Ok(Err(_)) => return Err(StatusCode::NOT_FOUND.into_response()),
        Err(e) => return Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()),
    };

    let mime = image_mime(&raw);
    Ok(([(header::CONTENT_TYPE, mime)], raw).into_response())
}

/// 按 magic bytes 探测图像 mime（zip 解出的原图）。仅覆盖 webp / png /
/// jpeg 三种——其他（gif/bmp 等）按 jpeg 处理，浏览器多能猜对。
fn image_mime(bytes: &[u8]) -> &'static str {
    if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        "image/webp"
    } else if bytes.len() >= 8 && bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        "image/png"
    } else {
        "image/jpeg"
    }
}

fn webp_response(bytes: Vec<u8>, etag: &str) -> axum::response::Response {
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "image/webp"), (header::ETAG, etag)],
        bytes,
    )
        .into_response()
}

/// `PUT /api/doujinshi/:id/images/:index/thumb` — 前端用 canvas 把原图转
/// 成 webp q=70 ≤1000px 后写入 preview_cache。后续 GET 命中直吐。
pub async fn put_image_thumb(
    State(s): State<ApiState>,
    Path((id, index)): Path<(i64, usize)>,
    body: Bytes,
) -> impl IntoResponse {
    if body.is_empty() {
        return (StatusCode::BAD_REQUEST, "empty body").into_response();
    }
    let mime = image_mime(&body);
    if mime != "image/webp" {
        return (StatusCode::BAD_REQUEST, "not webp").into_response();
    }
    let key: crate::services::preview_cache::CacheKey = (id, index);
    if s.preview_cache.contains(&key) {
        return StatusCode::NO_CONTENT.into_response();
    }
    match s.preview_cache.insert(key, body.to_vec()).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// 封面文件实际是 webp（V3+）或 jpg（V1/V2 旧数据）。按 magic bytes 探测
/// mime——磁盘文件扩展名不可靠（V3 写 webp bytes 但路径硬编码 .jpg）。
fn cover_mime(bytes: &[u8]) -> &'static str {
    if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        "image/webp"
    } else if bytes.len() >= 8 && bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        "image/png"
    } else {
        "image/jpeg"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cover_mime_detects_webp_magic() {
        // 12 字节头：RIFF + size + WEBP
        let mut bytes = vec![0u8; 12];
        bytes[0..4].copy_from_slice(b"RIFF");
        bytes[8..12].copy_from_slice(b"WEBP");
        assert_eq!(cover_mime(&bytes), "image/webp");
    }

    #[test]
    fn cover_mime_detects_png_magic() {
        let mut bytes = vec![0u8; 32];
        bytes[0..8].copy_from_slice(b"\x89PNG\r\n\x1a\n");
        assert_eq!(cover_mime(&bytes), "image/png");
    }

    #[test]
    fn cover_mime_falls_back_to_jpeg_for_unknown() {
        let bytes = b"\xff\xd8\xff\xe0xxxx"; // JPEG SOI + APP0 magic
        assert_eq!(cover_mime(bytes), "image/jpeg");
    }

    #[test]
    fn cover_mime_handles_truncated_input() {
        // < 12 字节但 ≥ 8：能识别 png；< 8：fallback。
        let mut png_short = vec![0u8; 8];
        png_short[0..8].copy_from_slice(b"\x89PNG\r\n\x1a\n");
        assert_eq!(cover_mime(&png_short), "image/png");
        assert_eq!(cover_mime(b"\x89"), "image/jpeg");
    }
}

/// `PATCH /api/doujinshi/:id` — partial metadata update. Body shape
/// is `MetadataPatch`; only fields present in the JSON are written.
pub async fn patch_metadata(
    State(s): State<ApiState>,
    Path(id): Path<i64>,
    Json(patch): Json<crate::commands::library::MetadataPatch>,
) -> impl IntoResponse {
    match crate::commands::library::apply_metadata_patch(&s.conn, id, patch).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(crate::error::AppError::Other(msg)) if msg.contains("not found") => {
            StatusCode::NOT_FOUND.into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

// ---------------------------------------------------------------------------
// V3 endpoints: archive / restore / list_dirty
// ---------------------------------------------------------------------------

pub async fn archive(State(s): State<ApiState>, Path(id): Path<i64>) -> impl IntoResponse {
    let r = crate::services::state_machine::transition_with_dirs(
        &s.conn,
        id,
        crate::services::state_machine::TransitionKind::Archive,
        &s.identified_dir,
        &s.will_delete_dir,
        &s.archived_dir,
    )
    .await;
    if r.is_ok() {
        s.preview_cache.invalidate(id);
    }
    state_transition_response(r)
}

pub async fn restore(State(s): State<ApiState>, Path(id): Path<i64>) -> impl IntoResponse {
    let r = crate::services::state_machine::transition_with_dirs(
        &s.conn,
        id,
        crate::services::state_machine::TransitionKind::Restore,
        &s.identified_dir,
        &s.will_delete_dir,
        &s.archived_dir,
    )
    .await;
    if r.is_ok() {
        s.preview_cache.invalidate(id);
    }
    state_transition_response(r)
}

fn state_transition_response(r: anyhow::Result<()>) -> axum::response::Response {
    match r {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("not found") {
                StatusCode::NOT_FOUND.into_response()
            } else if msg.contains("illegal") {
                (StatusCode::CONFLICT, msg).into_response()
            } else {
                (StatusCode::INTERNAL_SERVER_ERROR, msg).into_response()
            }
        }
    }
}

pub async fn list_dirty(
    State(s): State<ApiState>,
    Query(p): Query<DirtyListParams>,
) -> impl IntoResponse {
    use crate::db::entities::dirty_data::Entity as Dirty;
    use sea_orm::QuerySelect;
    let limit = p.limit.unwrap_or(50);
    let offset = p.offset.unwrap_or(0);
    let q = Dirty::find();
    let total = match q.clone().count(&s.conn).await {
        Ok(n) => n,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };
    match q.offset(offset).limit(limit).all(&s.conn).await {
        Ok(items) => Json(json!({ "items": items, "total": total })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

#[derive(Deserialize)]
pub struct DirtyListParams {
    pub limit: Option<u64>,
    pub offset: Option<u64>,
}
