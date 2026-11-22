//! Door Axum `/compliance` static files and `/compliance/api/*` proxy.
//!
//! Served on the TLS door (`op-grpc-bridge`), not op-web `:8080`.
//! `/compliance/api/...` is rewritten to `/api/...` on `CC_COZO_UPSTREAM`
//! (default `http://127.0.0.1:8787`). The proxy forwards the method, body,
//! and content-type only.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

use axum::{
    body::Body,
    extract::{OriginalUri, Request},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use tower_http::services::{ServeDir, ServeFile};

const MAX_REQUEST_BODY: usize = 32 * 1024 * 1024;
const DEFAULT_UPSTREAM: &str = "http://127.0.0.1:8787";

/// Map a browser path `/compliance/api/...` to the Cozo path `/api/...`.
///
/// Paths outside that prefix, and paths whose decoded segments are `.` or
/// `..`, return `None` so they are not forwarded.
pub fn rewrite_compliance_api(path: &str) -> Option<String> {
    let (path, query) = split_path_query(path);
    let rest = path.strip_prefix("/compliance")?;
    if rest != "/api" && !rest.starts_with("/api/") {
        return None;
    }
    if path_has_dot_segment(rest) {
        return None;
    }
    if let Some(query) = query {
        if query_is_unsafe(query) {
            return None;
        }
        Some(format!("{rest}?{query}"))
    } else {
        Some(rest.to_string())
    }
}

/// Resolve the upstream path from the original request URI and the URI a
/// nested router sees after `/compliance/api` is stripped.
pub(crate) fn compliance_upstream_path(original: &str, nested: &str) -> Option<String> {
    if let Some(rewritten) = rewrite_compliance_api(original) {
        return Some(rewritten);
    }
    let (nested_path, nested_query) = split_path_query(nested);
    let suffix = nested_path.trim_start_matches('/');
    let mut reconstructed = if suffix.is_empty() {
        "/compliance/api".to_string()
    } else {
        format!("/compliance/api/{suffix}")
    };
    if let Some(query) = nested_query {
        reconstructed.push('?');
        reconstructed.push_str(query);
    }
    rewrite_compliance_api(&reconstructed)
}

fn split_path_query(value: &str) -> (&str, Option<&str>) {
    match value.split_once('?') {
        Some((path, query)) => (path, Some(query)),
        None => (value, None),
    }
}

fn path_has_dot_segment(path: &str) -> bool {
    let mut current = path.to_string();
    for _ in 0..3 {
        if segment_is_dot(&current) || has_control_or_separator(&current) {
            return true;
        }
        let Some(decoded) = percent_decode(&current) else {
            return true;
        };
        if decoded == current {
            return false;
        }
        current = decoded;
    }
    segment_is_dot(&current) || has_control_or_separator(&current)
}

fn segment_is_dot(path: &str) -> bool {
    path.split('/')
        .any(|segment| segment == "." || segment == "..")
}

fn has_control_or_separator(path: &str) -> bool {
    path.bytes()
        .any(|byte| byte < 0x20 || byte == b'\\' || byte == b'#')
}

fn query_is_unsafe(query: &str) -> bool {
    query
        .bytes()
        .any(|byte| byte < 0x20 || byte == b'\\' || byte == b'#')
}

fn percent_decode(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len() {
                return None;
            }
            let hi = hex_value(bytes[index + 1])?;
            let lo = hex_value(bytes[index + 2])?;
            out.push((hi << 4) | lo);
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(out).ok()
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// `CC_COMPLIANCE_STATIC_DIR`, or `{dashboard_static_dir}/compliance`.
pub fn compliance_static_dir(dashboard_static_dir: &str) -> PathBuf {
    let configured = std::env::var("CC_COMPLIANCE_STATIC_DIR").ok();
    resolve_compliance_static_dir(configured.as_deref(), dashboard_static_dir)
}

pub(crate) fn resolve_compliance_static_dir(
    configured: Option<&str>,
    dashboard_static_dir: &str,
) -> PathBuf {
    if let Some(configured) = configured.map(str::trim).filter(|value| !value.is_empty()) {
        return PathBuf::from(configured);
    }
    PathBuf::from(dashboard_static_dir.trim_end_matches('/')).join("compliance")
}

/// Static compliance dist with `index.html` fallback for client routes.
pub fn compliance_static_service(dir: &Path) -> ServeDir<ServeFile> {
    ServeDir::new(dir).fallback(ServeFile::new(dir.join("index.html")))
}

/// Proxy `/compliance/api/*` to the Cozo origin from `CC_COZO_UPSTREAM`.
pub async fn compliance_api_proxy(request: Request) -> Response {
    proxy_request(request, &cozo_upstream()).await
}

/// Owner decision 2026-11-22: the `/compliance/api` proxy requires the same
/// verified door identity as every other door call (exactly one SID1 or oracle
/// assertion, `BridgeService::authenticate`). Unauthenticated requests get 401
/// and never reach the upstream. It stays an accepted untracked proxy (the
/// compliance app is not a sealed plugin; no new subid or reseal).
pub(crate) async fn authenticated_compliance_api_proxy(
    service: crate::service::BridgeService,
    request: Request,
) -> Response {
    authenticated_proxy(&service, request, &cozo_upstream()).await
}

async fn authenticated_proxy(
    service: &crate::service::BridgeService,
    request: Request,
    upstream: &str,
) -> Response {
    let mut probe = tonic::Request::new(());
    *probe.metadata_mut() = tonic::metadata::MetadataMap::from_headers(request.headers().clone());
    if service.authenticate(&probe).await.is_err() {
        return json_error(StatusCode::UNAUTHORIZED, "verified door identity required");
    }
    proxy_request(request, upstream).await
}

fn cozo_upstream() -> String {
    std::env::var("CC_COZO_UPSTREAM")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| DEFAULT_UPSTREAM.to_string())
}

async fn proxy_request(request: Request, upstream: &str) -> Response {
    let nested = uri_string(request.uri());
    let original = request
        .extensions()
        .get::<OriginalUri>()
        .map(|OriginalUri(uri)| uri_string(uri))
        .unwrap_or_else(|| nested.clone());
    let Some(rewritten) = compliance_upstream_path(&original, &nested) else {
        return json_error(StatusCode::NOT_FOUND, "not found");
    };
    let Some(url) = upstream_url(upstream, &rewritten) else {
        tracing::warn!("compliance upstream URL rejected");
        return json_error(StatusCode::BAD_GATEWAY, "compliance upstream unavailable");
    };
    let Some(method) = reqwest_method(request.method()) else {
        return json_error(StatusCode::BAD_REQUEST, "unsupported method");
    };
    let content_type = copy_request_header(request.headers(), header::CONTENT_TYPE);
    let body = match axum::body::to_bytes(request.into_body(), MAX_REQUEST_BODY).await {
        Ok(bytes) => bytes,
        Err(error) => {
            tracing::warn!(%error, "compliance request body rejected");
            return json_error(
                StatusCode::PAYLOAD_TOO_LARGE,
                "compliance request body is too large",
            );
        }
    };

    let Ok(client) = http_client() else {
        return json_error(StatusCode::BAD_GATEWAY, "compliance upstream unavailable");
    };
    let mut builder = client.request(method, url);
    if let Some(content_type) = content_type {
        builder = builder.header(reqwest::header::CONTENT_TYPE, content_type);
    }
    if !body.is_empty() {
        builder = builder.body(body.to_vec());
    }

    let upstream_response = match builder.send().await {
        Ok(response) => response,
        Err(error) => {
            tracing::warn!(%error, "compliance upstream unavailable");
            return json_error(StatusCode::BAD_GATEWAY, "compliance upstream unavailable");
        }
    };

    let status = StatusCode::from_u16(upstream_response.status().as_u16())
        .unwrap_or(StatusCode::BAD_GATEWAY);
    let upstream_headers = upstream_response.headers().clone();
    let upstream_bytes = match upstream_response.bytes().await {
        Ok(bytes) => bytes,
        Err(error) => {
            tracing::warn!(%error, "compliance upstream body failed");
            return json_error(StatusCode::BAD_GATEWAY, "compliance upstream unavailable");
        }
    };
    let mut response = Response::new(Body::from(upstream_bytes.to_vec()));
    *response.status_mut() = status;
    copy_response_header(
        response.headers_mut(),
        header::CONTENT_TYPE,
        upstream_headers.get(reqwest::header::CONTENT_TYPE),
    );
    copy_response_header(
        response.headers_mut(),
        header::CONTENT_DISPOSITION,
        upstream_headers.get(reqwest::header::CONTENT_DISPOSITION),
    );
    copy_response_header(
        response.headers_mut(),
        header::CACHE_CONTROL,
        upstream_headers.get(reqwest::header::CACHE_CONTROL),
    );
    response
}

fn uri_string(uri: &axum::http::Uri) -> String {
    uri.path_and_query()
        .map(|value| value.as_str().to_string())
        .unwrap_or_else(|| uri.path().to_string())
}

fn upstream_url(base: &str, rewritten: &str) -> Option<reqwest::Url> {
    let base_url = reqwest::Url::parse(base.trim()).ok()?;
    if base_url.scheme() != "http" && base_url.scheme() != "https" {
        return None;
    }
    if !base_url.username().is_empty() || base_url.password().is_some() {
        return None;
    }
    let host = base_url.host_str()?.to_string();
    let port = base_url.port_or_known_default();
    let mut origin = base_url;
    origin.set_path("");
    origin.set_query(None);
    origin.set_fragment(None);
    let origin = origin.as_str().trim_end_matches('/');
    let url = reqwest::Url::parse(&format!("{origin}{rewritten}")).ok()?;
    if url.host_str() != Some(host.as_str()) || url.port_or_known_default() != port {
        return None;
    }
    let path = url.path();
    if path != "/api" && !path.starts_with("/api/") {
        return None;
    }
    Some(url)
}

fn reqwest_method(method: &axum::http::Method) -> Option<reqwest::Method> {
    Some(match method.as_str() {
        "GET" => reqwest::Method::GET,
        "POST" => reqwest::Method::POST,
        "PUT" => reqwest::Method::PUT,
        "DELETE" => reqwest::Method::DELETE,
        "HEAD" => reqwest::Method::HEAD,
        "OPTIONS" => reqwest::Method::OPTIONS,
        "PATCH" => reqwest::Method::PATCH,
        "TRACE" => reqwest::Method::TRACE,
        other => reqwest::Method::from_bytes(other.as_bytes()).ok()?,
    })
}

fn copy_request_header(
    headers: &HeaderMap,
    name: axum::http::HeaderName,
) -> Option<reqwest::header::HeaderValue> {
    let value = headers.get(name)?;
    reqwest::header::HeaderValue::from_bytes(value.as_bytes()).ok()
}

fn copy_response_header(
    headers: &mut HeaderMap,
    name: axum::http::HeaderName,
    value: Option<&reqwest::header::HeaderValue>,
) {
    let Some(value) = value else {
        return;
    };
    if let Ok(value) = HeaderValue::from_bytes(value.as_bytes()) {
        headers.insert(name, value);
    }
}

fn http_client() -> Result<&'static reqwest::Client, ()> {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    if let Some(client) = CLIENT.get() {
        return Ok(client);
    }
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|error| {
            tracing::error!(%error, "compliance proxy HTTP client");
        })?;
    Ok(CLIENT.get_or_init(|| client))
}

fn json_error(status: StatusCode, message: &'static str) -> Response {
    (
        status,
        [(header::CONTENT_TYPE, "application/json")],
        format!(r#"{{"error":"{message}"}}"#),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, body::Bytes};
    use std::path::PathBuf;

    #[test]
    fn strips_compliance_prefix_for_upstream() {
        assert_eq!(
            rewrite_compliance_api("/compliance/api/health"),
            Some("/api/health".to_string())
        );
        assert_eq!(
            rewrite_compliance_api("/compliance/api/query"),
            Some("/api/query".to_string())
        );
        assert_eq!(rewrite_compliance_api("/api/health"), None);
    }

    #[test]
    fn keeps_query_and_rejects_dot_segments() {
        assert_eq!(
            rewrite_compliance_api("/compliance/api/storage/a%20b?x=1"),
            Some("/api/storage/a%20b?x=1".to_string())
        );
        assert_eq!(rewrite_compliance_api("/compliance/api/../health"), None);
        assert_eq!(
            rewrite_compliance_api("/compliance/api/%2e%2e/health"),
            None
        );
        assert_eq!(
            rewrite_compliance_api("/compliance/api/%252e%252e/health"),
            None
        );
        assert_eq!(rewrite_compliance_api("/compliance"), None);
    }

    #[test]
    fn nested_suffix_rewrites_to_api() {
        assert_eq!(
            compliance_upstream_path("/compliance/api/health", "/health"),
            Some("/api/health".to_string())
        );
        assert_eq!(
            compliance_upstream_path("/health?x=1", "/health?x=1"),
            Some("/api/health?x=1".to_string())
        );
        assert_eq!(
            compliance_upstream_path("/compliance/api", "/"),
            Some("/api".to_string())
        );
    }

    #[test]
    fn static_dir_defaults_under_dashboard_dir() {
        assert_eq!(
            resolve_compliance_static_dir(None, "/var/www/dashboard"),
            PathBuf::from("/var/www/dashboard/compliance")
        );
        assert_eq!(
            resolve_compliance_static_dir(None, "/var/www/dashboard/"),
            PathBuf::from("/var/www/dashboard/compliance")
        );
        assert_eq!(
            resolve_compliance_static_dir(Some("  /opt/cc  "), "/var/www/dashboard"),
            PathBuf::from("/opt/cc")
        );
        assert_eq!(
            resolve_compliance_static_dir(Some("   "), "/var/www/dashboard"),
            PathBuf::from("/var/www/dashboard/compliance")
        );
    }

    #[tokio::test]
    async fn api_proxy_rejects_unauthenticated_requests_before_the_upstream() {
        let core = crate::BridgeCore::new(crate::Catalog::from_entries(Vec::new()).unwrap());
        let service = crate::service::BridgeService::new(core);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let upstream = format!("http://{}", listener.local_addr().unwrap());
        for request in [
            Request::builder()
                .method("POST")
                .uri("/compliance/api/query")
                .body(Body::from("{}"))
                .unwrap(),
            Request::builder()
                .method("GET")
                .uri("/compliance/api/query")
                .header(op_identity::sealed_id::HTTP_HEADER_NAME, "AAAA")
                .body(Body::empty())
                .unwrap(),
        ] {
            let response = authenticated_proxy(&service, request, &upstream).await;
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        }
        assert!(
            tokio::time::timeout(Duration::from_millis(200), listener.accept())
                .await
                .is_err(),
            "an unauthenticated request must never reach the compliance upstream"
        );
    }

    #[tokio::test]
    async fn proxy_forwards_method_body_and_content_type_only() {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<(String, String, String, bool, Vec<u8>)>(1);
        let app = Router::new().fallback(move |request: Request| {
            let tx = tx.clone();
            async move {
                let method = request.method().to_string();
                let path = request
                    .uri()
                    .path_and_query()
                    .map(|value| value.as_str().to_string())
                    .unwrap_or_default();
                let content_type = request
                    .headers()
                    .get(header::CONTENT_TYPE)
                    .and_then(|value| value.to_str().ok())
                    .unwrap_or("")
                    .to_string();
                let saw_identity = request.headers().contains_key(header::AUTHORIZATION)
                    || request.headers().contains_key(header::COOKIE);
                let body = axum::body::to_bytes(request.into_body(), 1024 * 1024)
                    .await
                    .unwrap_or_default()
                    .to_vec();
                let _ = tx
                    .send((method, path, content_type, saw_identity, body))
                    .await;
                (
                    StatusCode::OK,
                    [(header::CONTENT_TYPE, "application/json")],
                    r#"{"ok":true}"#,
                )
            }
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let response = proxy_request(
            Request::builder()
                .method("POST")
                .uri("/compliance/api/query?limit=1")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::AUTHORIZATION, "Bearer secret")
                .header(header::COOKIE, "sid=secret")
                .body(Body::from(r#"{"q":"x"}"#))
                .unwrap(),
            &format!("http://{address}"),
        )
        .await;

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            "application/json"
        );
        let bytes = axum::body::to_bytes(response.into_body(), 1024)
            .await
            .unwrap();
        assert_eq!(bytes, Bytes::from_static(br#"{"ok":true}"#));
        let (method, path, content_type, saw_identity, body) = rx.recv().await.unwrap();
        assert_eq!(method, "POST");
        assert_eq!(path, "/api/query?limit=1");
        assert_eq!(content_type, "application/json");
        assert!(!saw_identity);
        assert_eq!(body, br#"{"q":"x"}"#);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let closed = listener.local_addr().unwrap();
        drop(listener);
        let down = proxy_request(
            Request::builder()
                .uri("/compliance/api/health")
                .body(Body::empty())
                .unwrap(),
            &format!("http://{closed}"),
        )
        .await;
        assert_eq!(down.status(), StatusCode::BAD_GATEWAY);
        assert_eq!(
            down.headers().get(header::CONTENT_TYPE).unwrap(),
            "application/json"
        );
    }
}
