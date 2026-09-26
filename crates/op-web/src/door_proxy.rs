//! Same-origin connector from the page to the bridge door.
//!
//! The page sends gRPC-Web to op-web's own origin. op-web forwards each call
//! unchanged to the tonic door over TLS it already trusts, so the browser never
//! handles the door's certificate. Identity metadata (OIA1 or SID1) passes
//! through untouched and the door's interceptor verifies it; op-web never
//! reads, mints or checks a credential here.

use std::time::Duration;

use anyhow::Context;
use axum::{
    body::Body,
    extract::{Request, State},
    http::{header, HeaderMap, HeaderName, HeaderValue, Method, Uri, Version},
    middleware::Next,
    response::Response,
};
use tonic::transport::{Certificate, Channel, ClientTlsConfig, Endpoint};
use tower::ServiceExt;

/// Connection-scoped headers that must not cross to the HTTP/2 door.
const HOP_BY_HOP: [&str; 8] = [
    "connection",
    "keep-alive",
    "proxy-connection",
    "transfer-encoding",
    "upgrade",
    "te",
    "trailer",
    "host",
];

/// A lazily connected channel to the door. Calls carry no request timeout, so
/// server-streaming methods stay open for as long as the page keeps them.
#[derive(Clone)]
pub struct DoorProxy {
    channel: Channel,
}

impl DoorProxy {
    /// Build from the settings op-web's own door client uses:
    /// `OP_DBUS_GRPC_ADDR`, `OP_DBUS_GRPC_CA_FILE` and `OP_DBUS_GRPC_TLS_DOMAIN`.
    pub fn from_env() -> anyhow::Result<Self> {
        let address = crate::state::door_grpc_addr();
        let ca_pem = match std::env::var("OP_DBUS_GRPC_CA_FILE") {
            Ok(path) if !path.trim().is_empty() => Some(
                std::fs::read(path.trim())
                    .with_context(|| format!("reading OP_DBUS_GRPC_CA_FILE={path}"))?,
            ),
            _ => None,
        };
        let domain = std::env::var("OP_DBUS_GRPC_TLS_DOMAIN")
            .ok()
            .map(|domain| domain.trim().to_string())
            .filter(|domain| !domain.is_empty());
        Self::new(&address, ca_pem, domain)
    }

    pub fn new(
        address: &str,
        ca_pem: Option<Vec<u8>>,
        domain: Option<String>,
    ) -> anyhow::Result<Self> {
        let mut endpoint = Endpoint::from_shared(address.to_string())
            .with_context(|| format!("invalid door address {address}"))?
            .connect_timeout(Duration::from_secs(5));
        if address.starts_with("https://") {
            let mut tls = ClientTlsConfig::new().with_native_roots();
            if let Some(pem) = ca_pem {
                tls = tls.ca_certificate(Certificate::from_pem(pem));
            }
            if let Some(domain) = domain {
                tls = tls.domain_name(domain);
            }
            endpoint = endpoint
                .tls_config(tls)
                .with_context(|| format!("TLS config for {address}"))?;
        }
        Ok(Self {
            channel: endpoint.connect_lazy(),
        })
    }

    /// Forward one gRPC-Web call and return the door's response as it came.
    pub async fn forward(&self, request: Request) -> Response {
        let (mut parts, body) = request.into_parts();
        let client_version = parts.version;
        strip_hop_by_hop(&mut parts.headers);
        // The channel supplies scheme and authority; only the path travels.
        parts.uri = parts
            .uri
            .path_and_query()
            .cloned()
            .map(Uri::from)
            .unwrap_or_default();
        parts.version = Version::HTTP_2;
        let upstream = axum::http::Request::from_parts(parts, tonic::body::boxed(body));
        match self.channel.clone().oneshot(upstream).await {
            Ok(response) => {
                let (mut parts, body) = response.into_parts();
                strip_hop_by_hop(&mut parts.headers);
                parts.version = client_version;
                Response::from_parts(parts, Body::new(body))
            }
            Err(error) => {
                tracing::warn!(%error, "door connector could not reach the door");
                unavailable()
            }
        }
    }
}

/// Route gRPC-Web calls to the door; everything else continues to op-web.
pub async fn door_connector(
    State(proxy): State<DoorProxy>,
    request: Request,
    next: Next,
) -> Response {
    if is_grpc_web(request.method(), request.uri(), request.headers()) {
        proxy.forward(request).await
    } else {
        next.run(request).await
    }
}

/// A gRPC-Web call is `POST /<package.Service>/<Method>` with a gRPC-Web
/// content type.
pub fn is_grpc_web(method: &Method, uri: &Uri, headers: &HeaderMap) -> bool {
    let grpc_web = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.starts_with("application/grpc-web"));
    method == Method::POST && grpc_web && is_service_path(uri.path())
}

fn is_service_path(path: &str) -> bool {
    let mut segments = path.strip_prefix('/').unwrap_or(path).split('/');
    matches!(
        (segments.next(), segments.next(), segments.next()),
        (Some(service), Some(method), None)
            if service.contains('.') && !service.starts_with('.') && !method.is_empty()
    )
}

fn strip_hop_by_hop(headers: &mut HeaderMap) {
    // Names listed in `Connection` are connection-scoped as well.
    let listed: Vec<HeaderName> = headers
        .get_all(header::CONNECTION)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .filter_map(|name| HeaderName::from_bytes(name.trim().as_bytes()).ok())
        .collect();
    for name in listed {
        headers.remove(name);
    }
    for name in HOP_BY_HOP {
        headers.remove(name);
    }
}

/// A trailers-only gRPC-Web reply, so the page reports UNAVAILABLE.
fn unavailable() -> Response {
    let mut response = Response::new(Body::empty());
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/grpc-web+proto"),
    );
    headers.insert("grpc-status", HeaderValue::from_static("14"));
    headers.insert(
        "grpc-message",
        HeaderValue::from_static("door%20unavailable"),
    );
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Bytes, http::StatusCode, Router};

    const CALL: &str = "/operation.v1.PluginService/CallMethod";
    const GRPC_WEB: &str = "application/grpc-web+proto";

    fn grpc_headers() -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(GRPC_WEB));
        headers
    }

    #[test]
    fn only_grpc_web_service_calls_go_to_the_door() {
        let call: Uri = CALL.parse().unwrap();
        assert!(is_grpc_web(&Method::POST, &call, &grpc_headers()));
        assert!(!is_grpc_web(&Method::GET, &call, &grpc_headers()));
        assert!(!is_grpc_web(&Method::POST, &call, &HeaderMap::new()));
        for path in [
            "/api/health",
            "/a/b/c",
            "/NoPackage/Method",
            "/.x/Method",
            "/pkg.Svc/",
        ] {
            let uri: Uri = path.parse().unwrap();
            assert!(!is_grpc_web(&Method::POST, &uri, &grpc_headers()), "{path}");
        }
    }

    #[test]
    fn connection_headers_are_removed_and_identity_is_kept() {
        let mut headers = grpc_headers();
        headers.insert(header::HOST, HeaderValue::from_static("10.0.0.4"));
        headers.insert(
            header::CONNECTION,
            HeaderValue::from_static("keep-alive, x-hop"),
        );
        headers.insert("x-hop", HeaderValue::from_static("1"));
        headers.insert("keep-alive", HeaderValue::from_static("timeout=5"));
        headers.insert(
            "x-oracle-identity-assertion-bin",
            HeaderValue::from_static("T0lBMQ"),
        );
        strip_hop_by_hop(&mut headers);
        for gone in ["host", "connection", "x-hop", "keep-alive"] {
            assert!(!headers.contains_key(gone), "{gone}");
        }
        assert_eq!(headers["x-oracle-identity-assertion-bin"], "T0lBMQ");
        assert_eq!(headers[header::CONTENT_TYPE], GRPC_WEB);
    }

    /// Echo what the door received, so the test sees exactly what crossed.
    async fn door_echo(uri: Uri, headers: HeaderMap, body: Bytes) -> Response {
        let mut response = Response::new(Body::from(body));
        let echoed = response.headers_mut();
        echoed.insert(header::CONTENT_TYPE, HeaderValue::from_static(GRPC_WEB));
        echoed.insert("echo-path", uri.path().parse().unwrap());
        echoed.insert(
            "echo-has-host",
            HeaderValue::from(u16::from(headers.contains_key(header::HOST))),
        );
        if let Some(assertion) = headers.get("x-oracle-identity-assertion-bin") {
            echoed.insert("echo-assertion", assertion.clone());
        }
        response
    }

    fn connector(proxy: DoorProxy) -> Router {
        Router::new()
            .fallback(|| async { StatusCode::NOT_FOUND })
            .layer(axum::middleware::from_fn_with_state(proxy, door_connector))
    }

    fn call_request() -> Request {
        axum::http::Request::post(CALL)
            .header(header::CONTENT_TYPE, GRPC_WEB)
            .header(header::HOST, "10.0.0.4")
            .header(header::CONNECTION, "keep-alive")
            .header("x-oracle-identity-assertion-bin", "T0lBMQ")
            .body(Body::from("\0\0\0\0\x02hi"))
            .unwrap()
    }

    #[tokio::test]
    async fn a_call_reaches_the_door_unchanged() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, Router::new().fallback(door_echo))
                .await
                .unwrap();
        });
        let proxy = DoorProxy::new(&format!("http://{address}"), None, None).unwrap();

        let response = connector(proxy).oneshot(call_request()).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["echo-path"], CALL);
        assert_eq!(response.headers()["echo-assertion"], "T0lBMQ");
        assert_eq!(response.headers()["echo-has-host"], "0");
        let body = axum::body::to_bytes(response.into_body(), 1024)
            .await
            .unwrap();
        assert_eq!(&body[..], b"\0\0\0\0\x02hi");
    }

    #[tokio::test]
    async fn an_https_door_builds_its_tls_client() {
        crate::install_tls_crypto_provider();
        let ca = include_bytes!("../../../certs/op-dbus-ca.crt").to_vec();
        assert!(DoorProxy::new("https://10.0.0.2:8090", Some(ca), None).is_ok());
    }

    #[tokio::test]
    async fn other_requests_stay_on_op_web() {
        let proxy = DoorProxy::new("http://127.0.0.1:1", None, None).unwrap();
        let request = axum::http::Request::get("/api/health")
            .body(Body::empty())
            .unwrap();
        let response = connector(proxy).oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn an_unreachable_door_reports_unavailable() {
        let proxy = DoorProxy::new("http://127.0.0.1:1", None, None).unwrap();
        let response = connector(proxy).oneshot(call_request()).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["grpc-status"], "14");
    }
}
