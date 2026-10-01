//! Browser reachability for the existing, identity-gated EMQX wake doorbell.
//!
//! This is a byte relay on the already trusted :8090 TLS ingress. EMQX still
//! authenticates every MQTT CONNECT through ExHook and authorizes the exact
//! owner topic. The relay cannot select a broker, topic, or credential.

use std::{io::BufReader, net::Ipv4Addr, sync::Arc, time::Duration};

use axum::{
    extract::{
        Extension,
        ws::{Message as BrowserMessage, WebSocket, WebSocketUpgrade},
    },
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use futures::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio::sync::OnceCell;
use tokio_rustls::{TlsConnector, rustls};
use tokio_tungstenite::{WebSocketStream, tungstenite, tungstenite::protocol::WebSocketConfig};

pub(crate) const PUBLIC_LISTENER: &str = "wss://10.0.0.2:8090/mqtt";
const BROKER_HOST: &str = op_plugins::state_plugins::emqx::FABRIC_SVC0_IP;
const BROKER_PORT: u16 = 8084;
const BROKER_CA: &str = "/etc/op-dbus/tls/tonic-svc0-ca.crt";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const FIRST_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const IDLE_TIMEOUT: Duration = Duration::from_secs(90);
const SEND_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_FRAME_BYTES: usize = 64 * 1024;
static BROKER_TLS: OnceCell<Arc<rustls::ClientConfig>> = OnceCell::const_new();

fn allowed_origin(headers: &HeaderMap) -> bool {
    matches!(
        headers
            .get(header::ORIGIN)
            .and_then(|value| value.to_str().ok()),
        Some("http://dashboard.ghostbridge.tech:8080") | Some("http://10.0.0.2:8080")
    )
}

fn mqtt_subprotocol(headers: &HeaderMap) -> bool {
    headers
        .get(header::SEC_WEBSOCKET_PROTOCOL)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.split(',').any(|part| part.trim() == "mqtt"))
}

/// The browser authenticates to EMQX after this upgrade with its own SID1 or
/// OIA1 MQTT password. Never inspect or log MQTT frames here.
pub(crate) async fn upgrade(
    ws: WebSocketUpgrade,
    headers: HeaderMap,
    Extension(permit): Extension<Arc<tokio::sync::OwnedSemaphorePermit>>,
) -> Response {
    if !allowed_origin(&headers) || !mqtt_subprotocol(&headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let broker = match tokio::time::timeout(CONNECT_TIMEOUT, connect_broker()).await {
        Ok(Ok(broker)) => broker,
        _ => return StatusCode::BAD_GATEWAY.into_response(),
    };
    accept_upgrade(ws, broker, Some(permit))
}

fn accept_upgrade<S>(
    ws: WebSocketUpgrade,
    broker: WebSocketStream<S>,
    permit: Option<Arc<tokio::sync::OwnedSemaphorePermit>>,
) -> Response
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    ws.protocols(["mqtt"])
        .max_message_size(MAX_FRAME_BYTES)
        .max_frame_size(MAX_FRAME_BYTES)
        .on_upgrade(move |browser| async move {
            let _permit = permit;
            relay(browser, broker).await;
        })
        .into_response()
}

async fn connect_broker()
-> anyhow::Result<WebSocketStream<tokio_rustls::client::TlsStream<TcpStream>>> {
    let config = BROKER_TLS
        .get_or_try_init(|| async {
            let ca = tokio::fs::read(BROKER_CA).await?;
            let mut roots = rustls::RootCertStore::empty();
            for certificate in rustls_pemfile::certs(&mut BufReader::new(ca.as_slice())) {
                roots.add(certificate?)?;
            }
            anyhow::ensure!(!roots.is_empty(), "broker CA contains no certificates");
            let config = rustls::ClientConfig::builder_with_provider(Arc::new(
                rustls::crypto::aws_lc_rs::default_provider(),
            ))
            .with_safe_default_protocol_versions()?
            .with_root_certificates(roots)
            .with_no_client_auth();
            Ok::<_, anyhow::Error>(Arc::new(config))
        })
        .await?;
    let tcp = TcpStream::connect((BROKER_HOST, BROKER_PORT)).await?;
    let broker_ip: Ipv4Addr = BROKER_HOST.parse()?;
    let server_name = rustls::pki_types::ServerName::IpAddress(broker_ip.into());
    let tls = TlsConnector::from(config.clone())
        .connect(server_name, tcp)
        .await?;
    let mut request = format!("wss://{BROKER_HOST}:{BROKER_PORT}/mqtt").into_client_request()?;
    request.headers_mut().insert(
        header::SEC_WEBSOCKET_PROTOCOL,
        "mqtt".parse().expect("constant MQTT protocol header"),
    );
    let (socket, response) = tokio_tungstenite::client_async_with_config(
        request,
        tls,
        Some(WebSocketConfig {
            max_message_size: Some(MAX_FRAME_BYTES),
            max_frame_size: Some(MAX_FRAME_BYTES),
            ..Default::default()
        }),
    )
    .await?;
    anyhow::ensure!(
        response
            .headers()
            .get(header::SEC_WEBSOCKET_PROTOCOL)
            .is_some_and(|value| value == "mqtt"),
        "broker did not accept MQTT WebSocket protocol"
    );
    Ok(socket)
}

async fn relay<S>(browser: WebSocket, broker: WebSocketStream<S>)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let (mut browser_send, mut browser_recv) = browser.split();
    let (mut broker_send, mut broker_recv) = broker.split();
    let mut first_connect = true;
    loop {
        let activity = tokio::time::timeout(
            if first_connect {
                FIRST_CONNECT_TIMEOUT
            } else {
                IDLE_TIMEOUT
            },
            async {
                tokio::select! {
                    from_browser = browser_recv.next() => RelayEvent::Browser(from_browser),
                    from_broker = broker_recv.next() => RelayEvent::Broker(from_broker),
                }
            },
        )
        .await;
        match activity {
            Ok(RelayEvent::Browser(Some(Ok(BrowserMessage::Binary(bytes)))))
                if bytes.len() <= MAX_FRAME_BYTES
                    && (!first_connect || bytes.first().is_some_and(|b| b >> 4 == 1)) =>
            {
                first_connect = false;
                if !matches!(
                    tokio::time::timeout(
                        SEND_TIMEOUT,
                        broker_send.send(tungstenite::Message::Binary(bytes))
                    )
                    .await,
                    Ok(Ok(()))
                ) {
                    break;
                }
            }
            Ok(RelayEvent::Broker(Some(Ok(tungstenite::Message::Binary(bytes)))))
                if !first_connect && bytes.len() <= MAX_FRAME_BYTES =>
            {
                if !matches!(
                    tokio::time::timeout(
                        SEND_TIMEOUT,
                        browser_send.send(BrowserMessage::Binary(bytes))
                    )
                    .await,
                    Ok(Ok(()))
                ) {
                    break;
                }
            }
            Ok(RelayEvent::Browser(Some(Ok(BrowserMessage::Ping(_))))) if !first_connect => {}
            Ok(RelayEvent::Browser(Some(Ok(BrowserMessage::Pong(_))))) if !first_connect => {}
            Ok(RelayEvent::Broker(Some(Ok(tungstenite::Message::Ping(_))))) if !first_connect => {}
            Ok(RelayEvent::Broker(Some(Ok(tungstenite::Message::Pong(_))))) if !first_connect => {}
            _ => break,
        }
    }
    let _ =
        tokio::time::timeout(SEND_TIMEOUT, browser_send.send(BrowserMessage::Close(None))).await;
    let _ = tokio::time::timeout(
        SEND_TIMEOUT,
        broker_send.send(tungstenite::Message::Close(None)),
    )
    .await;
}

enum RelayEvent {
    Browser(Option<Result<BrowserMessage, axum::Error>>),
    Broker(Option<Result<tungstenite::Message, tungstenite::Error>>),
}

use tokio_tungstenite::tungstenite::client::IntoClientRequest;

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, routing::get};
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;

    #[test]
    fn public_listener_uses_the_canonical_browser_tls_door() {
        assert_eq!(PUBLIC_LISTENER, "wss://10.0.0.2:8090/mqtt");
    }

    #[test]
    fn only_dashboard_origins_and_mqtt_subprotocol_are_admitted() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::ORIGIN,
            "http://dashboard.ghostbridge.tech:8080".parse().unwrap(),
        );
        headers.insert(header::SEC_WEBSOCKET_PROTOCOL, "mqtt".parse().unwrap());
        assert!(allowed_origin(&headers));
        assert!(mqtt_subprotocol(&headers));
        headers.insert(header::ORIGIN, "https://evil.example".parse().unwrap());
        assert!(!allowed_origin(&headers));
        headers.insert(header::SEC_WEBSOCKET_PROTOCOL, "other".parse().unwrap());
        assert!(!mqtt_subprotocol(&headers));
    }

    #[tokio::test]
    async fn axum_upgrade_relays_binary_and_closes_both_ends() {
        let broker_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let broker_addr = broker_listener.local_addr().unwrap();
        let broker = tokio::spawn(async move {
            let (tcp, _) = broker_listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(tcp).await.unwrap();
            let connect = ws.next().await.unwrap().unwrap();
            assert_eq!(connect, tungstenite::Message::Binary(vec![0x10, 0x00]));
            ws.send(tungstenite::Message::Binary(vec![0x20, 0x02, 0, 0]))
                .await
                .unwrap();
            let close = ws.next().await.unwrap().unwrap();
            assert!(matches!(close, tungstenite::Message::Close(_)));
        });
        let app = Router::new().route(
            "/mqtt",
            get(move |ws: WebSocketUpgrade| async move {
                let (upstream, _) =
                    tokio_tungstenite::connect_async(format!("ws://{broker_addr}/mqtt"))
                        .await
                        .unwrap();
                accept_upgrade(ws, upstream, None)
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let certificate = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
        let identity = crate::server::PemIdentity::from_pem(
            certificate.cert.pem(),
            certificate.key_pair.serialize_pem(),
        );
        let server_tls = identity.server_config().unwrap();
        let server = tokio::spawn(async move {
            crate::server::serve_upgradeable_tcp(listener, server_tls, app)
                .await
                .unwrap();
        });
        let mut roots = rustls::RootCertStore::empty();
        roots.add(certificate.cert.der().clone()).unwrap();
        let client_tls = rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::aws_lc_rs::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_root_certificates(roots)
        .with_no_client_auth();
        let tcp = TcpStream::connect(addr).await.unwrap();
        let tls = TlsConnector::from(Arc::new(client_tls))
            .connect(
                rustls::pki_types::ServerName::try_from("localhost").unwrap(),
                tcp,
            )
            .await
            .unwrap();
        let mut request = format!("wss://localhost:{}/mqtt", addr.port())
            .into_client_request()
            .unwrap();
        request
            .headers_mut()
            .insert(header::SEC_WEBSOCKET_PROTOCOL, "mqtt".parse().unwrap());
        let (mut client, response) = tokio_tungstenite::client_async(request, tls).await.unwrap();
        assert_eq!(response.status(), StatusCode::SWITCHING_PROTOCOLS);
        client
            .send(tungstenite::Message::Binary(vec![0x10, 0x00]))
            .await
            .unwrap();
        assert_eq!(
            client.next().await.unwrap().unwrap(),
            tungstenite::Message::Binary(vec![0x20, 0x02, 0, 0])
        );
        client.close(None).await.unwrap();
        tokio::time::timeout(Duration::from_secs(3), broker)
            .await
            .unwrap()
            .unwrap();
        server.abort();
    }
}
