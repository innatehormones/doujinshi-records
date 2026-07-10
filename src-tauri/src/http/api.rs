use crate::db::entities::doujinshi_file;
use crate::http::ApiState;
use crate::models::file_summary;
use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder, QuerySelect, PaginatorTrait};
use serde::Deserialize;
use serde_json::json;

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
