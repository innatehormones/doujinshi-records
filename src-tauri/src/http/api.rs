use crate::db::entities::doujinshi_file;
use crate::http::ApiState;
use crate::models::file_summary;
use base64::Engine;
use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder, QuerySelect, PaginatorTrait};
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
    let mut q = doujinshi_file::Entity::find()
        .filter(doujinshi_file::Column::PhysicallyDeleted.eq(false));
    if let Some(text) = p.q.as_deref().filter(|s| !s.is_empty()) {
        let pat = format!("%{}%", text);
        q = q.filter(
            doujinshi_file::Column::Title.like(&pat)
                .or(doujinshi_file::Column::Circle.like(&pat))
                .or(doujinshi_file::Column::Filename.like(&pat)),
        );
    }
    if let Some(st) = p.status.as_deref() {
        q = match st {
            "viewed" => q.filter(doujinshi_file::Column::Viewed.eq(true)),
            "not_viewed" => q.filter(doujinshi_file::Column::Viewed.eq(false)),
            "marked" => q.filter(doujinshi_file::Column::MarkedForDelete.eq(true)),
            _ => q,
        };
    }
    let limit = p.limit.unwrap_or(50);
    let offset = p.offset.unwrap_or(0);
    let total: u64 = q
        .clone()
        .count(&s.conn)
        .await
        .ok()
        .unwrap_or(0);
    let rows = q
        .order_by_desc(doujinshi_file::Column::CreatedAt)
        .limit(limit)
        .offset(offset)
        .all(&s.conn)
        .await
        .unwrap_or_default();
    let items: Vec<file_summary::FileSummary> =
        rows.iter().map(file_summary::from_model).collect();
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
        Some(m) => Json(json!(file_summary::from_model(&m))),
        None => Json(json!(null)),
    }
}

pub async fn by_id(
    State(s): State<ApiState>,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    let row = doujinshi_file::Entity::find_by_id(id)
        .one(&s.conn)
        .await
        .unwrap_or(None);
    match row {
        Some(m) => (StatusCode::OK, Json(json!(file_summary::from_model(&m)))).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

pub async fn cover(
    State(s): State<ApiState>,
    Path(hash): Path<String>,
) -> impl IntoResponse {
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
            return ([(header::CONTENT_TYPE, "image/jpeg")], bytes).into_response();
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
        Some(m) => Json(json!(file_summary::from_model(&m))),
        None => Json(json!(null)),
    }
}

/// `POST /api/doujinshi/:id/viewed` — mark a single file as viewed.
/// Returns 204 on success, 404 when the id does not exist.
pub async fn mark_viewed_http(
    State(s): State<ApiState>,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    use sea_orm::{ActiveModelTrait, EntityTrait, Set};
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
pub async fn compare(
    State(s): State<ApiState>,
    Path(conflict_id): Path<i64>,
) -> impl IntoResponse {
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
    /// `data:image/{ext};base64,...` — directly usable in `<img src>`.
    pub data_url: String,
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

/// `GET /api/doujinshi/:id/images` — return every image inside the
/// archive as base64 `data:` URLs so the SPA can render them
/// without setting up its own file-serving endpoint.
///
/// V3.1: response body is LRU-cached on disk + memory, keyed by
/// `(file_id, zip_mtime)`. ETag mirrors the key so the browser can
/// short-circuit with `If-None-Match: 304`.
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
        mtime.duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    );

    // If-None-Match → 304
    if let Some(if_none_match) = headers.get(header::IF_NONE_MATCH).and_then(|v| v.to_str().ok()) {
        if if_none_match == etag {
            return (
                StatusCode::NOT_MODIFIED,
                [(header::ETAG, etag.clone())],
            )
                .into_response();
        }
    }

    let key: crate::services::preview_cache::CacheKey = (id, mtime);

    // Try cache.
    if let Some(body) = s.preview_cache.get(&key) {
        return (
            StatusCode::OK,
            [
                (header::CONTENT_TYPE, "application/json"),
                (header::ETAG, etag.as_str()),
            ],
            body,
        )
            .into_response();
    }

    // Compute.
    let entries = match crate::services::archive::list_images(path) {
        Ok(e) => e,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };
    let images: Vec<ImageEntry> = entries
        .into_iter()
        .map(|e| {
            let b64 = base64::engine::general_purpose::STANDARD.encode(&e.data);
            ImageEntry {
                data_url: format!("data:image/{};base64,{}", guess_image_ext(&e.name), b64),
                name: e.name,
            }
        })
        .collect();
    let response = ImagesResponse { file_id: id, images, zip_missing: false };
    let body = match serde_json::to_vec(&response) {
        Ok(b) => b,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };

    // Store (async — fire and forget on tokio runtime).
    let cache_for_write = s.preview_cache.clone();
    let body_for_write = body.clone();
    let key_for_write = key;
    tokio::spawn(async move {
        if let Err(e) = cache_for_write
            .get_or_compute(key_for_write, || async { Ok::<_, anyhow::Error>(body_for_write) })
            .await
        {
            eprintln!("preview_cache write failed: {:?}", e);
        }
    });

    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "application/json"),
            (header::ETAG, etag.as_str()),
        ],
        body,
    )
        .into_response()
}

fn guess_image_ext(name: &str) -> &'static str {
    let lower = name.to_lowercase();
    if lower.ends_with(".png") {
        "png"
    } else if lower.ends_with(".webp") {
        "webp"
    } else {
        "jpeg"
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

pub async fn list_dirty(State(s): State<ApiState>) -> impl IntoResponse {
    use crate::db::entities::dirty_data::Entity as Dirty;
    match Dirty::find().all(&s.conn).await {
        Ok(rows) => Json(rows).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}
