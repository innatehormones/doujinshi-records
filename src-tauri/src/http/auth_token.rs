//! HTTP auth token: generate + persist.
//!
//! On first launch we create a 32-byte random token, encode it as
//! URL-safe base64 (no padding), and write it to `app_setting` under
//! the `auth_token` key. Subsequent launches read it back unchanged.
//! The token is required on every non-exempt HTTP route as
//! `Authorization: Bearer <token>`. See `super::auth`.

use base64::{engine::general_purpose, Engine};
use rand::RngCore;

/// Generate a fresh 32-byte URL-safe base64 token (~43 chars).
pub fn generate() -> String {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}