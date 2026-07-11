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
        None => (StatusCode::UNAUTHORIZED, "missing Authorization header").into_response(),
    }
}