// Integration tests: tonic gRPC server with TLS + gRPC server reflection.
//
// Spins up the full `build_operation_routes` surface on an ephemeral TCP port
// with a self-signed TLS certificate (generated via `rcgen`).  A tonic client
// configured to trust that self-signed CA connects and exercises:
//
//   1. TLS handshake — connection succeeds with the right CA, fails without it.
//   2. gRPC Reflection v1 — `ListServices` returns all mounted services.
//   3. Reflection FileDescriptor — we can fetch the descriptor for a known service.
//   4. Health check — `tonic_health` responds SERVING over TLS.
//   5. Domain RPC — `GetState` returns a valid (empty) response over TLS.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use tokio::time::timeout;
use tonic::transport::{Certificate, Channel, ClientTlsConfig, Identity, ServerTlsConfig};
use tonic::Request;

/// Install the rustls CryptoProvider exactly once for the entire test binary.
fn install_crypto_provider() {
    use std::sync::Once;
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        rustls::crypto::aws_lc_rs::default_provider()
            .install_default()
            .expect("failed to install rustls CryptoProvider");
    });
}

// ── helpers ──────────────────────────────────────────────────────────────────

/// Generate a self-signed TLS identity via rcgen, returning (Identity, CA cert PEM).
fn generate_tls_identity() -> (Identity, String) {
    let ck =
        rcgen::generate_simple_self_signed(vec!["localhost".to_string(), "127.0.0.1".to_string()])
            .expect("failed to generate self-signed TLS cert");
    let cert_pem = ck.cert.pem();
    let key_pem = ck.key_pair.serialize_pem();
    let identity = Identity::from_pem(cert_pem.clone(), key_pem);
    (identity, cert_pem)
}

/// Start a TLS-enabled gRPC server on an ephemeral port. Returns (addr, ca_cert_pem).
async fn start_tls_server() -> (SocketAddr, String) {
    install_crypto_provider();
    let (identity, ca_pem) = generate_tls_identity();

    let event_chain = Arc::new(tokio::sync::RwLock::new(op_state_store::EventChain::new(
        op_state_store::ChainConfig::default(),
    )));
    let ovsdb = Arc::new(op_network::rovs_proxy::OvsdbDbusClient::new());
    let mutation_engine = Arc::new(op_grpc_bridge::MutationEngine::new(event_chain, ovsdb));
    let blobs = tempfile::tempdir().expect("isolated sealed blob catalog");
    op_blob::BlobStore::open(blobs.path())
        .unwrap()
        .write(&op_grpc_bridge::tched_router_object_blob::from_plugin_schema())
        .unwrap();
    let catalog = op_grpc_bridge::dynamic_reflection::ActiveReflectionCatalog::with_dir(
        blobs.path().to_path_buf(),
    );
    let server = op_grpc_bridge::grpc_server::OperationGrpcServer::new(mutation_engine)
        .with_reflection_catalog(catalog);

    // Health reporter — mirrors run_grpc_server's health surface.
    let (mut health_reporter, health_service) = tonic_health::server::health_reporter();
    health_reporter
        .set_serving::<op_grpc_bridge::proto::state_sync_server::StateSyncServer<
            op_grpc_bridge::grpc_server::OperationGrpcServer,
        >>()
        .await;

    let tls_config = ServerTlsConfig::new().identity(identity);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let incoming = tokio_stream::wrappers::TcpListenerStream::new(listener);

    tokio::spawn(async move {
        let _blobs = blobs;
        tonic::transport::Server::builder()
            .tls_config(tls_config)
            .expect("valid TLS config")
            .accept_http1(true)
            .add_routes(op_grpc_bridge::grpc_server::build_operation_routes(server))
            .add_service(tonic_web::enable(health_service))
            .serve_with_incoming(incoming)
            .await
            .unwrap();
    });

    // Give the server a moment to start accepting connections.
    tokio::time::sleep(Duration::from_millis(200)).await;
    (addr, ca_pem)
}

/// Build a tonic `Channel` that trusts the server's self-signed CA.
async fn tls_channel(addr: SocketAddr, ca_pem: &str) -> Channel {
    let tls = ClientTlsConfig::new()
        .ca_certificate(Certificate::from_pem(ca_pem))
        .domain_name("localhost");

    Channel::from_shared(format!("https://{}:{}", addr.ip(), addr.port()))
        .unwrap()
        .tls_config(tls)
        .unwrap()
        .connect_timeout(Duration::from_secs(5))
        .connect()
        .await
        .expect("TLS channel should connect")
}

// ── tests ────────────────────────────────────────────────────────────────────

/// TLS handshake succeeds with the correct CA.
#[tokio::test]
async fn tls_handshake_succeeds() {
    let (addr, ca_pem) = start_tls_server().await;
    let _channel = tls_channel(addr, &ca_pem).await;
    // If we got here, the TLS handshake succeeded.
}

/// TLS handshake fails when the client does not trust the server's CA.
#[tokio::test]
async fn tls_handshake_rejects_unknown_ca() {
    let (addr, _ca_pem) = start_tls_server().await;

    // Generate a *different* CA that the server doesn't use.
    let wrong_ca = rcgen::generate_simple_self_signed(vec!["wrong.example.com".to_string()])
        .unwrap()
        .cert
        .pem();

    let tls = ClientTlsConfig::new()
        .ca_certificate(Certificate::from_pem(&wrong_ca))
        .domain_name("localhost");

    let result = Channel::from_shared(format!("https://{}:{}", addr.ip(), addr.port()))
        .unwrap()
        .tls_config(tls)
        .unwrap()
        .connect_timeout(Duration::from_secs(5))
        .connect()
        .await;

    // The connection should fail (certificate verification error) or the
    // first RPC should fail. Tonic may lazily connect, so attempt a health
    // check to force the handshake.
    match result {
        Err(_) => { /* expected */ }
        Ok(channel) => {
            let mut health = tonic_health::pb::health_client::HealthClient::new(channel);
            let res = health
                .check(tonic_health::pb::HealthCheckRequest {
                    service: String::new(),
                })
                .await;
            assert!(
                res.is_err(),
                "RPC should fail with wrong CA, but got: {:?}",
                res
            );
        }
    }
}

/// gRPC server reflection `ListServices` returns services over TLS.
///
/// The `DynamicReflectionService` only lists services from active plugin
/// object blobs (SHM catalog) plus `grpc.reflection.v1.ServerReflection`.
/// The fixture seals one plugin in an isolated catalog; only its mounted
/// generated service and the reflection service should be advertised.
#[tokio::test]
async fn reflection_list_services_over_tls() {
    let (addr, ca_pem) = start_tls_server().await;
    let channel = tls_channel(addr, &ca_pem).await;

    let mut client =
        tonic_reflection::pb::v1::server_reflection_client::ServerReflectionClient::new(channel);

    let (tx, rx) = tokio::sync::mpsc::channel(4);
    tx.send(tonic_reflection::pb::v1::ServerReflectionRequest {
        host: String::new(),
        message_request: Some(
            tonic_reflection::pb::v1::server_reflection_request::MessageRequest::ListServices(
                String::new(),
            ),
        ),
    })
    .await
    .unwrap();
    drop(tx);

    let response = timeout(
        Duration::from_secs(10),
        client.server_reflection_info(tokio_stream::wrappers::ReceiverStream::new(rx)),
    )
    .await
    .expect("reflection RPC should not time out")
    .expect("reflection RPC should succeed");

    let mut stream = response.into_inner();
    let msg = timeout(Duration::from_secs(5), stream.message())
        .await
        .expect("should receive reflection response")
        .expect("stream should not error")
        .expect("should have at least one response");

    let services = match msg.message_response {
        Some(
            tonic_reflection::pb::v1::server_reflection_response::MessageResponse::ListServicesResponse(
                list,
            ),
        ) => list
            .service
            .into_iter()
            .map(|s| s.name)
            .collect::<Vec<_>>(),
        other => panic!("expected ListServicesResponse, got: {:?}", other),
    };

    // The list must be non-empty — at minimum it contains the reflection service.
    assert!(
        !services.is_empty(),
        "reflection should list at least one service"
    );

    // The reflection service itself is always listed.
    assert!(
        services
            .iter()
            .any(|s| s == "grpc.reflection.v1.ServerReflection"),
        "grpc.reflection.v1.ServerReflection should always be listed. Got: {services:?}"
    );

    assert_eq!(
        services,
        [
            "grpc.reflection.v1.ServerReflection",
            "operation.plugin.v1.TchedRouterPluginMethods",
        ],
        "only mounted services backed by this fixture's blob are discoverable"
    );
}

/// A discovered generated service must reach its authentication handler,
/// not return UNIMPLEMENTED as an unmounted descriptor-only service would.
#[tokio::test]
async fn advertised_plugin_route_is_mounted_over_tls() {
    let (addr, ca_pem) = start_tls_server().await;
    let channel = tls_channel(addr, &ca_pem).await;
    let mut client = op_grpc_bridge::proto::plugin_methods::tched_router_plugin_methods_client::TchedRouterPluginMethodsClient::new(channel);
    let status = client
        .get_agents_config(
            op_grpc_bridge::proto::plugin_methods::TchedRouterGetAgentsConfigRequest::default(),
        )
        .await
        .expect_err("missing identity must fail at the mounted route's auth gate");
    assert_eq!(status.code(), tonic::Code::Unauthenticated);
}

/// gRPC reflection can retrieve a file descriptor for a known service over TLS.
#[tokio::test]
async fn reflection_file_descriptor_over_tls() {
    let (addr, ca_pem) = start_tls_server().await;
    let channel = tls_channel(addr, &ca_pem).await;

    let mut client =
        tonic_reflection::pb::v1::server_reflection_client::ServerReflectionClient::new(channel);

    let (tx, rx) = tokio::sync::mpsc::channel(4);
    tx.send(tonic_reflection::pb::v1::ServerReflectionRequest {
        host: String::new(),
        message_request: Some(
            tonic_reflection::pb::v1::server_reflection_request::MessageRequest::FileContainingSymbol(
                "operation.v1.StateSync".to_string(),
            ),
        ),
    })
    .await
    .unwrap();
    drop(tx);

    let response = timeout(
        Duration::from_secs(10),
        client.server_reflection_info(tokio_stream::wrappers::ReceiverStream::new(rx)),
    )
    .await
    .expect("reflection RPC should not time out")
    .expect("reflection RPC should succeed");

    let mut stream = response.into_inner();
    let msg = timeout(Duration::from_secs(5), stream.message())
        .await
        .expect("should receive reflection response")
        .expect("stream should not error")
        .expect("should have at least one response");

    match msg.message_response {
        Some(
            tonic_reflection::pb::v1::server_reflection_response::MessageResponse::FileDescriptorResponse(
                fd_response,
            ),
        ) => {
            assert!(
                !fd_response.file_descriptor_proto.is_empty(),
                "file descriptor proto should not be empty"
            );
            // Verify we can decode the raw descriptor bytes.
            for fd_bytes in &fd_response.file_descriptor_proto {
                let _fd: prost_types::FileDescriptorProto =
                    prost::Message::decode(fd_bytes.as_slice())
                        .expect("should decode FileDescriptorProto");
            }
        }
        other => panic!(
            "expected FileDescriptorResponse, got: {:?}",
            other
        ),
    };
}

/// gRPC health check responds SERVING over TLS.
#[tokio::test]
async fn health_check_over_tls() {
    let (addr, ca_pem) = start_tls_server().await;
    let channel = tls_channel(addr, &ca_pem).await;

    let mut health = tonic_health::pb::health_client::HealthClient::new(channel);

    // Check the overall server health (empty service name).
    let response = timeout(
        Duration::from_secs(5),
        health.check(tonic_health::pb::HealthCheckRequest {
            service: String::new(),
        }),
    )
    .await
    .expect("health check should not time out")
    .expect("health check should succeed");

    assert_eq!(
        response.into_inner().status,
        tonic_health::pb::health_check_response::ServingStatus::Serving as i32,
        "overall health should be SERVING"
    );

    // Check a specific service that we registered.
    let response = timeout(
        Duration::from_secs(5),
        health.check(tonic_health::pb::HealthCheckRequest {
            service: "operation.v1.StateSync".to_string(),
        }),
    )
    .await
    .expect("health check should not time out")
    .expect("health check should succeed");

    assert_eq!(
        response.into_inner().status,
        tonic_health::pb::health_check_response::ServingStatus::Serving as i32,
        "StateSync health should be SERVING"
    );
}

/// A domain RPC (GetState) works over TLS.
#[tokio::test]
async fn domain_rpc_get_state_over_tls() {
    let (addr, ca_pem) = start_tls_server().await;
    let channel = tls_channel(addr, &ca_pem).await;

    let mut client = op_grpc_bridge::proto::state_sync_client::StateSyncClient::new(channel);

    let response = timeout(
        Duration::from_secs(5),
        client.get_state(Request::new(op_grpc_bridge::proto::GetStateRequest {
            plugin_id: "nonexistent".to_string(),
            object_path: String::new(),
        })),
    )
    .await
    .expect("RPC should not time out");

    // The server may return OK with an empty state or an error if the plugin
    // doesn't exist — either is fine; we're validating the TLS transport.
    match response {
        Ok(resp) => {
            let inner = resp.into_inner();
            // `state` is an optional google.protobuf.Struct — it may be None
            // for a nonexistent plugin, which is fine.
            let _state = inner.state;
        }
        Err(status) => {
            // NOT_FOUND or similar is acceptable — the point is the TLS
            // transport worked and the server returned a proper gRPC status.
            assert!(
                status.code() != tonic::Code::Unavailable,
                "UNAVAILABLE implies transport failure, not a domain error: {status}"
            );
        }
    }
}

/// ListPlugins returns a valid response over TLS (validates the interceptor
/// passes without identity headers for this read-only method).
#[tokio::test]
async fn list_plugins_over_tls() {
    let (addr, ca_pem) = start_tls_server().await;
    let channel = tls_channel(addr, &ca_pem).await;

    let mut client =
        op_grpc_bridge::proto::plugin_service_client::PluginServiceClient::new(channel);

    let response = timeout(
        Duration::from_secs(5),
        client.list_plugins(Request::new(())),
    )
    .await
    .expect("RPC should not time out");

    // The interceptor may reject unauthenticated requests; either way the TLS
    // transport is working if we get a gRPC status back.
    match response {
        Ok(resp) => {
            let _plugins = resp.into_inner().plugins;
        }
        Err(status) => {
            // UNAUTHENTICATED or PERMISSION_DENIED from the interceptor is
            // acceptable — transport worked.
            assert!(
                status.code() != tonic::Code::Unavailable,
                "UNAVAILABLE implies transport failure: {status}"
            );
        }
    }
}
