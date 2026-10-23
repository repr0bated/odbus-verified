//! Fixed private WireGuard virtual hosts for native agent interfaces.
//! Lifecycle stays with the existing service D-Bus owner. This is only native
//! HTTP/WebSocket data relay; credentials are neither minted nor injected.
use std::{pin::Pin, sync::Arc, task::{Context, Poll}, time::Duration};

use axum::{
    body::{Body, Bytes},
    extract::Request,
    http::{HeaderMap, HeaderName, HeaderValue, StatusCode, Uri, Version, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use hyper::body::{Body as HttpBody, Frame, Incoming, SizeHint};
use hyper_util::rt::TokioIo;
use tokio::{net::TcpStream, sync::OwnedSemaphorePermit, task::JoinHandle};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
// Native synchronous prompt endpoints may withhold response headers until the
// agent finishes. Match the door's 3660s agent bound plus 60s transport margin.
const HTTP_RESPONSE_HEADER_TIMEOUT: Duration = Duration::from_secs(3720);

#[derive(Clone, Copy)]
enum Agent {
    OpenCode,
    Paseo,
}
impl Agent {
    fn address(self) -> &'static str {
        match self { Self::OpenCode => "127.0.0.1:4096", Self::Paseo => "127.0.0.1:6767" }
    }
}

fn enabled() -> bool {
    matches!(std::env::var("OP_AGENT_UI_PROXY_ENABLED").as_deref(), Ok("1"))
}

/// Preserve the entire exact authority, including the door port.
fn host(request: &Request) -> Result<Option<(Agent, String)>, ()> {
    let hosts: Vec<_> = request.headers().get_all(header::HOST).iter().collect();
    if hosts.len() > 1 { return Err(()); }
    let host = hosts.first().map(|h| h.to_str().map_err(|_| ())).transpose()?;
    let authority = request.uri().authority().map(|a| a.as_str());
    if let (Some(host), Some(authority)) = (host, authority)
        && host != authority { return Err(()); }
    let Some(host) = host.or(authority) else { return Ok(None); };
    let agent = match host {
        "opencode.wg.internal:8090" => Agent::OpenCode,
        "paseo.wg.internal:8090" => Agent::Paseo,
        _ => return Ok(None),
    };
    Ok(Some((agent, host.into())))
}

fn websocket(headers: &HeaderMap) -> bool {
    headers.get(header::UPGRADE).and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.eq_ignore_ascii_case("websocket"))
}

fn browser_origin(headers: &HeaderMap, host: &str) -> bool {
    let origins: Vec<_> = headers.get_all(header::ORIGIN).iter().collect();
    match origins.as_slice() {
        [] => true, // Native client; authentication still belongs to the server.
        [origin] => origin.to_str().ok() == Some(format!("https://{host}").as_str()),
        _ => false,
    }
}

fn strip_forwarded(headers: &mut HeaderMap) {
    let names: Vec<_> = headers.keys().filter(|name| {
        name.as_str() == "forwarded" || name.as_str().starts_with("x-forwarded-")
            || name.as_str() == "x-real-ip"
    }).cloned().collect();
    for name in names { headers.remove(name); }
    headers.insert("x-forwarded-proto", HeaderValue::from_static("https"));
}

fn strip_hop(headers: &mut HeaderMap) {
    let nominated: Vec<HeaderName> = headers.get_all(header::CONNECTION).iter()
        .filter_map(|v| v.to_str().ok()).flat_map(|v| v.split(','))
        .filter_map(|name| HeaderName::from_bytes(name.trim().as_bytes()).ok()).collect();
    for name in nominated { headers.remove(name); }
    for name in ["connection", "keep-alive", "proxy-connection", "transfer-encoding",
        "upgrade", "te", "trailer", "proxy-authenticate", "proxy-authorization"] {
        headers.remove(name);
    }
}

struct Driver(JoinHandle<()>);
impl Drop for Driver { fn drop(&mut self) { self.0.abort(); } }

struct StreamingBody {
    inner: Incoming,
    _driver: Driver,
}
impl HttpBody for StreamingBody {
    type Data = Bytes;
    type Error = hyper::Error;
    fn poll_frame(mut self: Pin<&mut Self>, cx: &mut Context<'_>)
        -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        Pin::new(&mut self.inner).poll_frame(cx)
    }
    fn is_end_stream(&self) -> bool { self.inner.is_end_stream() }
    fn size_hint(&self) -> SizeHint { self.inner.size_hint() }
}

/// This layer runs before gRPC CORS so native UI responses/challenges are
/// untouched. Disabled and unknown virtual hosts keep the existing door.
pub(crate) async fn route(request: Request, next: Next) -> Response {
    if !enabled() { return next.run(request).await; }
    match host(&request) {
        Ok(Some((agent, original_host))) => forward(request, agent.address(), &original_host).await,
        Ok(None) => next.run(request).await,
        Err(()) => StatusCode::BAD_REQUEST.into_response(),
    }
}

async fn forward(mut request: Request, address: &str, original_host: &str) -> Response {
    let is_ws = websocket(request.headers());
    if is_ws && !browser_origin(request.headers(), original_host) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let downstream_upgrade = is_ws.then(|| hyper::upgrade::on(&mut request));
    let permit = request.extensions().get::<Arc<OwnedSemaphorePermit>>().cloned();
    let tcp = match tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect(address)).await {
        Ok(Ok(tcp)) => tcp,
        _ => return StatusCode::BAD_GATEWAY.into_response(),
    };
    let (mut sender, connection) = match tokio::time::timeout(
        HANDSHAKE_TIMEOUT, hyper::client::conn::http1::handshake(TokioIo::new(tcp)),
    ).await {
        Ok(Ok(pair)) => pair,
        _ => return StatusCode::BAD_GATEWAY.into_response(),
    };
    let driver = Driver(tokio::spawn(async move {
        let _ = connection.with_upgrades().await;
    }));
    let origin = request.headers().get(header::ORIGIN).cloned();
    if !is_ws { strip_hop(request.headers_mut()); }
    strip_forwarded(request.headers_mut());
    request.headers_mut().insert(header::HOST,
        HeaderValue::from_str(original_host).expect("validated original host"));
    if let Some(origin) = origin { request.headers_mut().insert(header::ORIGIN, origin); }
    // The connection is already fixed to the loopback server. Origin-form
    // paths preserve both native UIs' root routing and all query parameters.
    *request.uri_mut() = request.uri().path_and_query().cloned()
        .map(Uri::from).unwrap_or_else(|| Uri::from_static("/"));
    *request.version_mut() = Version::HTTP_11;
    let header_timeout = if is_ws { HANDSHAKE_TIMEOUT } else { HTTP_RESPONSE_HEADER_TIMEOUT };
    let mut response = match tokio::time::timeout(header_timeout, sender.send_request(request)).await {
        Ok(Ok(response)) => response,
        _ => return StatusCode::BAD_GATEWAY.into_response(),
    };
    if is_ws && response.status() == StatusCode::SWITCHING_PROTOCOLS {
        let upstream_upgrade = hyper::upgrade::on(&mut response);
        let downstream_upgrade = downstream_upgrade.expect("WebSocket request upgrade captured");
        tokio::spawn(async move {
            let _driver = driver;
            let _permit = permit;
            let upgrades = tokio::time::timeout(HANDSHAKE_TIMEOUT, async {
                tokio::try_join!(downstream_upgrade, upstream_upgrade)
            }).await;
            if let Ok(Ok((downstream, upstream))) = upgrades {
                let mut downstream = TokioIo::new(downstream);
                let mut upstream = TokioIo::new(upstream);
                let _ = tokio::io::copy_bidirectional(&mut downstream, &mut upstream).await;
            }
        });
        let (parts, _) = response.into_parts();
        return Response::from_parts(parts, Body::empty());
    }
    let (mut parts, body) = response.into_parts();
    strip_hop(&mut parts.headers);
    Response::from_parts(parts, Body::new(StreamingBody { inner: body, _driver: driver }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, extract::ws::{WebSocketUpgrade, Message}, routing::any};
    use futures::{SinkExt, StreamExt};
    use tokio::sync::{Notify, mpsc};
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;

    struct Server(JoinHandle<()>);
    impl Drop for Server { fn drop(&mut self) { self.0.abort(); } }
    async fn serve(router: Router) -> (String, Server) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap().to_string();
        let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap(); });
        (address, Server(task))
    }
    async fn proxy(address: String) -> (String, Server) {
        serve(Router::new().fallback(any(move |request: Request| {
            let address = address.clone();
            async move {
                match host(&request) {
                    Ok(Some((_, host))) => forward(request, &address, &host).await,
                    Ok(None) => StatusCode::NOT_FOUND.into_response(),
                    Err(()) => StatusCode::BAD_REQUEST.into_response(),
                }
            }
        }))).await
    }

    #[test]
    fn exact_private_host_port_and_browser_origin_are_required() {
        for host_value in ["opencode.wg.internal:8090", "paseo.wg.internal:8090"] {
            let request = Request::builder().uri("/").header(header::HOST, host_value).body(Body::empty()).unwrap();
            assert!(host(&request).unwrap().is_some());
            let mut headers = HeaderMap::new();
            assert!(browser_origin(&headers, host_value));
            headers.insert(header::ORIGIN, format!("https://{host_value}").parse().unwrap());
            assert!(browser_origin(&headers, host_value));
            headers.insert(header::ORIGIN, "https://attacker.invalid".parse().unwrap());
            assert!(!browser_origin(&headers, host_value));
        }
        for host_value in ["opencode.wg.internal", "opencode.wg.internal:4096",
            "opencode.wg.internal.evil:8090", "10.0.0.2:8090"] {
            let request = Request::builder().uri("/").header(header::HOST, host_value).body(Body::empty()).unwrap();
            assert!(host(&request).unwrap().is_none());
        }
        let conflict = Request::builder().uri("https://paseo.wg.internal:8090/")
            .header(header::HOST, "opencode.wg.internal:8090").body(Body::empty()).unwrap();
        assert!(host(&conflict).is_err());
        let mut duplicate = Request::builder().uri("/").header(header::HOST, "opencode.wg.internal:8090")
            .body(Body::empty()).unwrap();
        duplicate.headers_mut().append(header::HOST, HeaderValue::from_static("paseo.wg.internal:8090"));
        assert!(host(&duplicate).is_err());
    }

    #[tokio::test]
    async fn native_auth_challenge_and_credentials_are_forwarded_without_injection() {
        let (sent, mut received) = mpsc::channel(4);
        let (upstream, _upstream_task) = serve(Router::new().fallback(any(move |request: Request| {
            let sent = sent.clone();
            async move {
                let auth = request.headers().get(header::AUTHORIZATION).cloned();
                sent.send((request.uri().to_string(), request.headers().clone())).await.unwrap();
                if auth.is_none() {
                    (StatusCode::UNAUTHORIZED, [(header::WWW_AUTHENTICATE, "Basic realm=\"native\""),
                        (header::SET_COOKIE, "native=challenge; HttpOnly")], "native authentication required").into_response()
                } else {
                    (StatusCode::OK, [(header::SET_COOKIE, "native=accepted; HttpOnly")], "accepted").into_response()
                }
            }
        }))).await;
        let (front, _proxy_task) = proxy(upstream).await;
        let client = reqwest::Client::new();
        let denied = client.get(format!("http://{front}/health?native=1"))
            .header("host", "opencode.wg.internal:8090")
            .header("forwarded", "for=attacker;proto=http")
            .header("x-forwarded-host", "attacker.invalid").send().await.unwrap();
        assert_eq!(denied.status(), reqwest::StatusCode::UNAUTHORIZED);
        assert_eq!(denied.headers()["www-authenticate"], "Basic realm=\"native\"");
        assert_eq!(denied.headers()["set-cookie"], "native=challenge; HttpOnly");
        assert_eq!(denied.text().await.unwrap(), "native authentication required");
        let (uri, headers) = received.recv().await.unwrap();
        assert_eq!(uri, "/health?native=1");
        assert_eq!(headers[header::HOST], "opencode.wg.internal:8090");
        assert!(headers.get(header::AUTHORIZATION).is_none());
        assert!(headers.get("forwarded").is_none());
        assert!(headers.get("x-forwarded-host").is_none());
        assert_eq!(headers["x-forwarded-proto"], "https");
        let accepted = client.get(format!("http://{front}/"))
            .header("host", "paseo.wg.internal:8090")
            .header("authorization", "Basic fixture-only")
            .header("cookie", "native=fixture-only")
            .header("origin", "https://paseo.wg.internal:8090")
            .send().await.unwrap();
        assert_eq!(accepted.status(), reqwest::StatusCode::OK);
        assert_eq!(accepted.headers()["set-cookie"], "native=accepted; HttpOnly");
        let (_, headers) = received.recv().await.unwrap();
        assert_eq!(headers[header::AUTHORIZATION], "Basic fixture-only");
        assert_eq!(headers[header::COOKIE], "native=fixture-only");
        assert_eq!(headers[header::ORIGIN], "https://paseo.wg.internal:8090");
    }

    #[tokio::test]
    async fn sse_first_chunk_streams_before_native_completion_and_disconnect_cancels() {
        struct Cancel(Arc<Notify>);
        impl Drop for Cancel { fn drop(&mut self) { self.0.notify_one(); } }
        let completed = Arc::new(Notify::new());
        let cancelled = Arc::new(Notify::new());
        let first_gate = completed.clone();
        let cancel_gate = cancelled.clone();
        let (upstream, _upstream_task) = serve(Router::new().fallback(any(move || {
            let gate = first_gate.clone();
            let cancel = cancel_gate.clone();
            async move {
                let body = async_stream::stream! {
                    let _cancel = Cancel(cancel);
                    yield Ok::<_, std::convert::Infallible>(Bytes::from_static(b"data: first\n\n"));
                    gate.notified().await;
                    yield Ok(Bytes::from_static(b"data: final\n\n"));
                };
                ([(header::CONTENT_TYPE, "text/event-stream")], Body::from_stream(body))
            }
        }))).await;
        let (front, _proxy_task) = proxy(upstream).await;
        let response = reqwest::Client::new().get(format!("http://{front}/events"))
            .header("host", "opencode.wg.internal:8090").send().await.unwrap();
        let mut stream = response.bytes_stream();
        let first = tokio::time::timeout(Duration::from_secs(2), stream.next()).await.unwrap().unwrap().unwrap();
        assert_eq!(first, "data: first\n\n");
        drop(stream);
        tokio::time::timeout(Duration::from_secs(2), cancelled.notified()).await.unwrap();
        completed.notify_one();
    }

    #[tokio::test]
    async fn websocket_native_auth_origin_echo_and_close_survive_relay() {
        let (upstream, _upstream_task) = serve(Router::new().fallback(any(
            |ws: WebSocketUpgrade, headers: HeaderMap| async move {
                if headers.get(header::AUTHORIZATION).is_none() {
                    return (StatusCode::UNAUTHORIZED, [(header::WWW_AUTHENTICATE, "Basic realm=\"native\"")]).into_response();
                }
                ws.on_upgrade(|mut socket| async move {
                    while let Some(Ok(message)) = socket.next().await {
                        if matches!(message, Message::Close(_)) { break; }
                        if socket.send(message).await.is_err() { break; }
                    }
                }).into_response()
            }
        ))).await;
        let (front, _proxy_task) = proxy(upstream).await;
        let make_request = || {
            let mut request = format!("ws://{front}/native/ws").into_client_request().unwrap();
            request.headers_mut().insert(header::HOST, HeaderValue::from_static("paseo.wg.internal:8090"));
            request.headers_mut().insert(header::ORIGIN, HeaderValue::from_static("https://paseo.wg.internal:8090"));
            request
        };
        let denied = tokio_tungstenite::connect_async(make_request()).await.unwrap_err();
        match denied {
            tokio_tungstenite::tungstenite::Error::Http(response) => {
                assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
                assert_eq!(response.headers()[header::WWW_AUTHENTICATE], "Basic realm=\"native\"");
            }
            other => panic!("unexpected handshake error: {other}"),
        }
        let mut wrong_origin = make_request();
        wrong_origin.headers_mut().insert(header::AUTHORIZATION, HeaderValue::from_static("Basic fixture-only"));
        wrong_origin.headers_mut().insert(header::ORIGIN, HeaderValue::from_static("https://attacker.invalid"));
        let denied = tokio_tungstenite::connect_async(wrong_origin).await.unwrap_err();
        assert!(matches!(denied, tokio_tungstenite::tungstenite::Error::Http(ref response)
            if response.status() == StatusCode::FORBIDDEN));
        let mut authorized = make_request();
        authorized.headers_mut().insert(header::AUTHORIZATION, HeaderValue::from_static("Basic fixture-only"));
        let (mut socket, _) = tokio_tungstenite::connect_async(authorized).await.unwrap();
        socket.send(tokio_tungstenite::tungstenite::Message::Text("native echo".into())).await.unwrap();
        let reply = tokio::time::timeout(Duration::from_secs(2), socket.next()).await.unwrap().unwrap().unwrap();
        assert_eq!(reply.to_text().unwrap(), "native echo");
        socket.close(None).await.unwrap();
        let _ = tokio::time::timeout(Duration::from_secs(2), socket.next()).await.unwrap();
    }
}
