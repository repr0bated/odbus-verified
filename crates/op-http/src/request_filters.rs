//! HTTP Request Filters
//!
//! Native request filtering and processing for security, logging, rate limiting, etc.

use axum::{
    extract::Request,
    http::{HeaderMap, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use std::time::Instant;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

/// Security middleware - adds security headers
pub async fn security_headers(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;

    let headers = response.headers_mut();
    headers.insert("X-Content-Type-Options", "nosniff".parse().unwrap());
    headers.insert("X-Frame-Options", "DENY".parse().unwrap());
    headers.insert("X-XSS-Protection", "1; mode=block".parse().unwrap());
    headers.insert("Referrer-Policy", "strict-origin-when-cross-origin".parse().unwrap());

    // Add HSTS for HTTPS
    if let Some(host) = headers.get("host") {
        if let Ok(host_str) = host.to_str() {
            if host_str.contains(":443") || host_str.starts_with("https://") {
                headers.insert("Strict-Transport-Security",
                    "max-age=31536000; includeSubDomains".parse().unwrap());
            }
        }
    }

    response
}

/// Request logging middleware
pub async fn request_logger(request: Request, next: Next) -> Response {
    let start = Instant::now();
    let method = request.method().clone();
    let uri = request.uri().clone();
    let version = request.version();

    let response = next.run(request).await;

    let duration = start.elapsed();
    let status = response.status();

    tracing::info!(
        "{} {} {:?} {} - {}ms",
        method,
        uri,
        version,
        status,
        duration.as_millis()
    );

    response
}

/// API key authentication middleware
pub async fn api_key_auth(
    headers: HeaderMap,
    request: Request,
    next: Next,
) -> Response {
    // Check for API key in various headers
    let api_key = headers.get("x-api-key")
        .or_else(|| headers.get("authorization"))
        .or_else(|| headers.get("x-password"))
        .and_then(|h| h.to_str().ok())
        .map(|s| s.trim_start_matches("Bearer ").trim());

    // For now, allow all requests (authentication is optional)
    // In production, you would validate the API key here
    if let Some(key) = api_key {
        if !key.is_empty() {
            tracing::debug!("API key provided: {}", if key.len() > 8 {
                format!("{}...{}", &key[..4], &key[key.len()-4..])
            } else {
                "***".to_string()
            });
        }
    }

    next.run(request).await
}

/// Rate limiting middleware (basic implementation)
pub async fn rate_limit(
    request: Request,
    next: Next,
) -> Response {
    // Simple rate limiting based on IP
    // In production, you'd want a more sophisticated solution
    let client_ip = request.headers()
        .get("x-forwarded-for")
        .or_else(|| request.headers().get("x-real-ip"))
        .and_then(|h| h.to_str().ok())
        .unwrap_or("unknown");

    // For now, just log and allow all requests
    tracing::debug!("Request from IP: {}", client_ip);

    next.run(request).await
}

/// Compression middleware
pub async fn compression(
    request: Request,
    next: Next,
) -> Response {
    // Add compression headers
    let mut response = next.run(request).await;

    let headers = response.headers_mut();
    headers.insert("Content-Encoding", "gzip".parse().unwrap());

    response
}

/// Create default CORS layer
pub fn default_cors() -> CorsLayer {
    CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any)
}

use tower_http::classify::SharedClassifier;
use tower_http::trace::DefaultOnRequest;
use tower_http::trace::DefaultOnResponse;
use tower_http::trace::DefaultOnBodyChunk;
use tower_http::trace::DefaultOnEos;
use tower_http::trace::DefaultOnFailure;
use tower_http::classify::ServerErrorsAsFailures;

/// Create default trace layer
pub fn default_trace() -> TraceLayer<
    SharedClassifier<ServerErrorsAsFailures>,
    tower_http::trace::DefaultMakeSpan,
    DefaultOnRequest,
    DefaultOnResponse,
    DefaultOnBodyChunk,
    DefaultOnEos,
    DefaultOnFailure,
> {
    TraceLayer::new_for_http()
}

/// Error handling middleware
pub async fn error_handler(
    err: Box<dyn std::error::Error + Send + Sync>,
) -> impl IntoResponse {
    tracing::error!("Request error: {}", err);
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        "Internal Server Error"
    )
}

/// Timeout middleware
pub async fn timeout(
    request: Request,
    next: Next,
) -> Response {
    // Set a reasonable timeout for requests
    match tokio::time::timeout(
        std::time::Duration::from_secs(30),
        next.run(request)
    ).await {
        Ok(response) => response,
        Err(_) => (
            StatusCode::REQUEST_TIMEOUT,
            "Request timeout"
        ).into_response(),
    }
}
