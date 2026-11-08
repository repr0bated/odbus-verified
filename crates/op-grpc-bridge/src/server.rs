//! Core gRPC services, with an explicit TLS-only production door.

use crate::core::BridgeCore;
use crate::proto::event_chain_service_server::EventChainServiceServer;
use crate::proto::plugin_service_server::PluginServiceServer;
use crate::proto::state_sync_server::StateSyncServer;
use crate::proto::FILE_DESCRIPTOR_SET;
use crate::service::BridgeService;
use crate::tls_door::{PemIdentity, serve_upgradeable_tcp_with_shutdown};
use axum::http::{HeaderName, HeaderValue, Method, StatusCode, Uri};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio_stream::wrappers::TcpListenerStream;
use tokio_rustls::rustls;
use tower::Layer;
use tower_http::cors::CorsLayer;

/// Serve an isolated in-memory fixture on an explicitly bound listener.
/// Production binaries must use `run_from_env` and its TLS/bootstrap owners.
pub async fn serve_with_listener(listener: TcpListener, core: BridgeCore) -> anyhow::Result<()> {
    let service = BridgeService::new(core);
    let reflection = tonic_reflection::server::Builder::configure()
        .register_encoded_file_descriptor_set(FILE_DESCRIPTOR_SET)
        .build_v1()?;
    tonic::transport::Server::builder()
        .add_service(StateSyncServer::new(service.clone()))
        .add_service(PluginServiceServer::new(service.clone()))
        .add_service(EventChainServiceServer::new(service))
        .add_service(reflection)
        .serve_with_incoming(TcpListenerStream::new(listener))
        .await?;
    Ok(())
}

/// A fully configured, bound production door. Binding does not publish
/// readiness: the caller must also have initialized runtime, lungs and ExHook.
pub struct ProductionServer {
    listener: TcpListener,
    addr: SocketAddr,
    tls: Arc<rustls::ServerConfig>,
    app: axum::Router,
}

impl ProductionServer {
    pub fn local_addr(&self) -> SocketAddr {
        self.addr
    }

    /// Stop accepting on shutdown, then drain active RPCs and MQTT upgrades.
    pub async fn serve<F>(self, shutdown: F) -> anyhow::Result<()>
    where
        F: Future<Output = ()>,
    {
        serve_upgradeable_tcp_with_shutdown(self.listener, self.tls, self.app, shutdown).await
    }
}

/// Bind the one production TLS listener using the caller's authenticated
/// service implementation. No plaintext or secondary public listener exists.
pub async fn bind_production(
    addr: SocketAddr,
    service: BridgeService,
    identity: PemIdentity,
) -> anyhow::Result<ProductionServer> {
    let tls = identity.server_config()?;
    // This deployed setting's historical name does not enable an MCP route.
    let origins = parse_allowed_origins(
        &std::env::var("OP_MCP_ALLOWED_ORIGINS").unwrap_or_default(),
    )?;
    let reflection = tonic_reflection::server::Builder::configure()
        .register_encoded_file_descriptor_set(FILE_DESCRIPTOR_SET)
        .build_v1()?;
    let web = tonic_web::GrpcWebLayer::new();
    let app = tonic::service::Routes::new(web.layer(StateSyncServer::new(service.clone())))
        .add_service(web.layer(PluginServiceServer::new(service.clone())))
        .add_service(web.layer(EventChainServiceServer::new(service)))
        .add_service(web.layer(reflection))
        .prepare()
        .into_axum_router()
        .layer(grpc_cors(origins))
        .route("/mqtt", axum::routing::get(crate::mqtt_doorbell_relay::upgrade))
        .fallback(|| async { StatusCode::NOT_FOUND });
    let listener = TcpListener::bind(addr).await?;
    let addr = listener.local_addr()?;
    Ok(ProductionServer { listener, addr, tls, app })
}

fn parse_allowed_origins(configured: &str) -> anyhow::Result<Vec<HeaderValue>> {
    configured
        .split(',')
        .map(str::trim)
        .filter(|origin| !origin.is_empty())
        .map(|origin| {
            let uri: Uri = origin.parse()?;
            anyhow::ensure!(
                matches!(uri.scheme_str(), Some("http") | Some("https"))
                    && uri.authority().is_some_and(|authority| !authority.as_str().contains('@'))
                    && (uri.path().is_empty() || uri.path() == "/")
                    && uri.query().is_none(),
                "OP_MCP_ALLOWED_ORIGINS must contain exact HTTP(S) origins",
            );
            Ok(HeaderValue::from_str(origin)?)
        })
        .collect()
}

fn grpc_cors(origins: Vec<HeaderValue>) -> CorsLayer {
    CorsLayer::new()
        .allow_origin(origins)
        .allow_methods([Method::POST, Method::OPTIONS])
        .allow_credentials(true)
        .max_age(Duration::from_secs(86400))
        .allow_headers([
            HeaderName::from_static("x-grpc-web"),
            HeaderName::from_static("content-type"),
            HeaderName::from_static("x-user-agent"),
            HeaderName::from_static("grpc-timeout"),
            HeaderName::from_static(op_identity::sealed_id::HTTP_HEADER_NAME),
            HeaderName::from_static("x-oracle-identity-assertion-bin"),
            HeaderName::from_static("x-opdbus-capability"),
            HeaderName::from_static("x-opdbus-operation-id"),
        ])
        .expose_headers([
            HeaderName::from_static("grpc-status"),
            HeaderName::from_static("grpc-message"),
            HeaderName::from_static("grpc-status-details-bin"),
            HeaderName::from_static("x-opdbus-identity-mode"),
            HeaderName::from_static("x-opdbus-identity-session"),
            HeaderName::from_static("x-opdbus-operation-id"),
        ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{header, Request, StatusCode};
    use tower::ServiceExt;

    #[tokio::test]
    async fn cors_preserves_exact_origins_identity_and_operation_headers() {
        let app = axum::Router::new()
            .route("/operation.v1.PluginService/CallMethod", axum::routing::post(|| async {}))
            .layer(grpc_cors(
                parse_allowed_origins("http://10.0.0.2:8080").unwrap(),
            ));
        for (origin, allowed) in [
            ("http://10.0.0.2:8080", true),
            ("http://10.0.0.2:8081", false),
            ("https://10.0.0.2:8080", false),
            ("http://10.0.0.2:8080.evil.example", false),
        ] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("OPTIONS")
                        .uri("/operation.v1.PluginService/CallMethod")
                        .header(header::ORIGIN, origin)
                        .header(header::ACCESS_CONTROL_REQUEST_METHOD, "POST")
                        .header(
                            header::ACCESS_CONTROL_REQUEST_HEADERS,
                            "content-type,x-opdbus-sealed-id-bin,x-oracle-identity-assertion-bin,x-opdbus-capability,x-opdbus-operation-id",
                        )
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(
                response.headers().get(header::ACCESS_CONTROL_ALLOW_ORIGIN).is_some(),
                allowed,
            );
            if allowed {
                assert_eq!(response.headers()[header::ACCESS_CONTROL_ALLOW_ORIGIN], origin);
                let headers = response.headers()[header::ACCESS_CONTROL_ALLOW_HEADERS]
                    .to_str()
                    .unwrap();
                for expected in [
                    "x-opdbus-sealed-id-bin",
                    "x-oracle-identity-assertion-bin",
                    "x-opdbus-capability",
                    "x-opdbus-operation-id",
                ] {
                    assert!(headers.split(',').any(|header| header.trim() == expected));
                }
            }
        }
    }

    #[tokio::test]
    async fn cors_exposes_operation_receipts_and_identity_metadata() {
        let app = axum::Router::new()
            .route(
                "/operation.v1.PluginService/CallMethod",
                axum::routing::post(|| async {
                    ([("x-opdbus-operation-id", "operation-123")], ())
                }),
            )
            .layer(grpc_cors(
                parse_allowed_origins("http://10.0.0.2:8080").unwrap(),
            ));
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/operation.v1.PluginService/CallMethod")
                    .header(header::ORIGIN, "http://10.0.0.2:8080")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.headers()["x-opdbus-operation-id"], "operation-123");
        assert_eq!(response.headers()[header::ACCESS_CONTROL_ALLOW_CREDENTIALS], "true");
        let exposed = response.headers()[header::ACCESS_CONTROL_EXPOSE_HEADERS]
            .to_str()
            .unwrap();
        for expected in [
            "x-opdbus-operation-id",
            "x-opdbus-identity-mode",
            "x-opdbus-identity-session",
            "grpc-status",
            "grpc-message",
            "grpc-status-details-bin",
        ] {
            assert!(exposed.split(',').any(|header| header.trim() == expected));
        }
    }

    #[test]
    fn cors_rejects_wildcards_and_non_origin_urls() {
        for origin in ["*", "null", "file:///tmp/ui", "https://ui.example/path", "https://ui.example?x=1"] {
            assert!(parse_allowed_origins(origin).is_err(), "{origin}");
        }
        assert!(parse_allowed_origins("").unwrap().is_empty());
    }
}
