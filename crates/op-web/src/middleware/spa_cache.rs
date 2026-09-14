//! Cache headers for the single-page dashboard.
//!
//! The SPA is served with content-hashed asset filenames (`index-BHDV-J-S.js`),
//! and `index.html` is the only thing that says which hash is current. Serving
//! both with no `Cache-Control` — which is what happened before this — lets the
//! browser apply its own heuristic freshness to the HTML, so it keeps loading a
//! previous bundle and every deploy looks like it silently did nothing.
//!
//! The two files want opposite policies:
//!
//! * `index.html` — never trust a cached copy. It is the pointer; a stale
//!   pointer pins the whole app to an old build.
//! * `/assets/*` — cache hard. The filename contains a content hash, so a
//!   changed file is a different URL and can never be stale.

use axum::{extract::Request, http::header, middleware::Next, response::Response};

/// Hashed assets are immutable: a new build emits a new filename.
const IMMUTABLE: &str = "public, max-age=31536000, immutable";

/// The bundle pointer must be revalidated on every load.
const NO_CACHE: &str = "no-cache, must-revalidate";

pub async fn spa_cache_middleware(req: Request, next: Next) -> Response {
    let path = req.uri().path().to_string();
    let mut response = next.run(req).await;

    // Only touch what this SPA serves; API and MCP routes set their own policy.
    let is_hashed_asset = path.starts_with("/assets/");
    let is_document = !is_hashed_asset
        && response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|ct| ct.starts_with("text/html"));

    let value = if is_hashed_asset {
        IMMUTABLE
    } else if is_document {
        NO_CACHE
    } else {
        return response;
    };

    if let Ok(header_value) = header::HeaderValue::from_str(value) {
        response
            .headers_mut()
            .insert(header::CACHE_CONTROL, header_value);
    }
    response
}
