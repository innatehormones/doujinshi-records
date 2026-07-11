//! HTTP bearer-token middleware.
//!
//! Every route other than `/api/health` requires
//! `Authorization: Bearer <token>`. The expected token is the
//! per-install secret persisted in `app_setting.auth_token` and
//! mirrored into `ApiState.auth_token` at startup. Wrong / missing
//! header → 401.

use axum::{
    extract::{Request, State},
    http::{header, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};

use crate::http::ApiState;

/// Paths that bypass auth. Keep this list short — anything new here
/// is a publicly-readable endpoint on a local loopback port.
///
/// `/api/covers/*` is exempt so browser `<img src="...">` tags can
/// fetch covers without having to inject an Authorization header.
/// Cover blobs are static image bytes — no sensitive data.
fn is_exempt(path: &str) -> bool {
    path == "/api/health" || path.starts_with("/api/covers/")
}

pub async fn require_auth(
    State(state): State<ApiState>,
    req: Request,
    next: Next,
) -> Response {
    let path = req.uri().path().to_string();
    if is_exempt(&path) {
        return next.run(req).await;
    }
    let header_val = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok());
    // Snapshot the live token so a concurrent regeneration doesn't
    // race the comparison. A failed read is treated as an internal
    // error and yields 500 rather than silently granting access.
    let token_snapshot = match state.auth_token.read() {
        Ok(t) => t.clone(),
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "token lock poisoned").into_response(),
    };
    let expected = format!("Bearer {}", token_snapshot);
    match header_val {
        Some(h) if h == expected => next.run(req).await,
        Some(_) => (StatusCode::UNAUTHORIZED, "bad token").into_response(),
        None => (StatusCode::UNAUTHORIZED, "missing Authorization header").into_response(),
    }
}