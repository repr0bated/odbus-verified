//! Per-request access logging.
//!
//! `TraceLayer` emits at DEBUG while the subscriber defaults to `info`
//! (`main.rs`), so individual requests were invisible. This logs one INFO line
//! per request with the method, path, status, and latency.
//!
//! The content type is included because a JSON path that falls through to the
//! SPA fallback answers `200 text/html`, which is otherwise indistinguishable
//! from success in a log that only records the status.

use axum::{extract::Request, http::header::CONTENT_TYPE, middleware::Next, response::Response};
use std::time::Instant;

pub async fn access_log_middleware(req: Request, next: Next) -> Response {
    let method = req.method().clone();
    let path = req
        .uri()
        .path_and_query()
        .map(|p| p.as_str().to_string())
        .unwrap_or_else(|| req.uri().path().to_string());

    let started = Instant::now();
    let response = next.run(req).await;
    let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;

    let status = response.status().as_u16();
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("-");

    tracing::info!("{method} {path} -> {status} {content_type} ({elapsed_ms:.1}ms)");

    response
}
