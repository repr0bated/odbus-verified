//! Axum gRPC/gRPC-Web host for the tched_router plugin schema.
//!
//! Serves the plugin-owned schema JSON on:
//!   - host native gRPC over Unix socket `/run/opdbus/grpc.sock`
//!   - shared container UDS `/run/ghostbridge/container.sock` (same routes)
//!   - gRPC + gRPC-Web on TCP `0.0.0.0:8090` (configurable by environment)
//!
//! The schema itself is never generated here; it is read from the plugin's
//! sealed blob in the SHM catalog (`/dev/shm/opdbus/plugin-blobs`).

use std::io::BufReader;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use async_stream::stream;
use axum::Router;
use sha2::{Digest, Sha256};
use tonic::transport::server::{TcpConnectInfo, TlsConnectInfo};
use tonic::transport::{Certificate, Identity, ServerTlsConfig};
use tonic::{Request as TonicRequest, Response as TonicResponse, Status};
use tower_http::cors::CorsLayer;
use tracing::info;

use crate::grpc_server::{build_operation_routes_with_validator, OperationGrpcServer};
use crate::mutation_engine::MutationEngine;
use crate::oracle_assertion::{AssertionValidator, DecoyTrustStore};
use crate::schema_loader::{SchemaLoader, SchemaReloadEvent};
use crate::schema_router::SchemaRouter;
use crate::tracing::{GhostbridgeTraceLayer, TraceContext};

use crate::proto::tched_router::{
    tched_router_service_server::{TchedRouterService, TchedRouterServiceServer},
    GetSchemaRequest, SchemaEvent, SchemaResponse, WatchSchemaRequest,
};

/// Host-local native gRPC UDS (op-web, zbusctl, operators on the host).
const DEFAULT_UNIX_SOCKET: &str = "/run/opdbus/grpc.sock";
/// Shared container socket — bind-mounted into NIC-less CTs as `/run/ghostbridge`.
const DEFAULT_SHARED_SOCKET: &str = crate::shared_socket::DEFAULT_SOCKET_PATH;
const DEFAULT_SCHEMA_PLUGIN_ID: &str = "tched_router";
/// The authenticated shared TCP door. Cognitive methods are in-process routes
/// on this listener; they do not own a separate bind address.
const DEFAULT_BIND_ADDR: &str = "0.0.0.0:8090";
const DEFAULT_EMQX_EXHOOK_BIND_ADDR: &str = "127.0.0.1:9000";
const DEFAULT_EMQX_EXHOOK_SERVER_CERT: &str = "/etc/op-dbus/tls/tonic-svc0.crt";
const DEFAULT_EMQX_EXHOOK_SERVER_KEY: &str = "/etc/op-dbus/tls/tonic-svc0.key";
const DEFAULT_EMQX_EXHOOK_CLIENT_CA: &str = "/etc/op-dbus/tls/tonic-svc0-ca.crt";
const DEFAULT_EMQX_EXHOOK_CLIENT_CERT: &str = "/etc/op-dbus/tls/emqx-exhook-client.crt";
/// Default schema source: the sealed blob catalog dir. When `schema_path`
/// is a directory the loader reads the plugin's own blob from it (a blob in
/// the catalog IS the plugin); a file path is still accepted for tests and
/// plugin-owned schema files.
const DEFAULT_UI_WORKSPACE_SHM_DIR: &str = "/dev/shm/opdbus/ui-workspaces";
const DEFAULT_GENERATION_SHM_DIR: &str = "/dev/shm/opdbus/generation-jobs";

/// Runtime configuration for the tched_router Axum host.
#[derive(Clone, Debug)]
pub struct ServerConfig {
    pub plugin_id: String,
    pub schema_path: PathBuf,
    /// Held owner-only root for principal/session-private UI workspace projections.
    pub ui_workspace_shm_dir: PathBuf,
    /// Private generation documents are separate from public router observations.
    pub generation_shm_dir: PathBuf,
    /// Host-local gRPC UDS (`TCHED_ROUTER_UNIX_SOCKET`, default `/run/opdbus/grpc.sock`).
    pub unix_socket: PathBuf,
    /// Shared container UDS (`GHOSTBRIDGE_SOCKET_PATH`, default
    /// `/run/ghostbridge/container.sock`). Served with the same route set.
    /// When equal to `unix_socket`, only one listener is opened.
    pub shared_socket: PathBuf,
    /// TCP bind addresses (comma-separated). **Always TLS** — zero-trust transport
    /// policy forbids plaintext gRPC on TCP, even on loopback.
    /// Set via `TCHED_ROUTER_BIND_ADDR` or `GRPC_BIND`; default `0.0.0.0:8090`.
    pub bind_addr: String,
    /// TLS identity for the TCP door. `None` aborts startup with a clear error
    /// unless `TCHED_ROUTER_DEV_SELF_SIGNED=1` is set (dev/CI only — never production).
    pub tls_identity: Option<Identity>,
    /// Dedicated broker callback listener. It is deliberately separate from
    /// the client-facing gRPC/MCP door: EMQX authenticates with a pinned mTLS
    /// client certificate and can reach only HookProvider.
    pub emqx_exhook: EmqxExhookConfig,
}

#[derive(Clone, Debug)]
pub struct EmqxExhookConfig {
    pub enabled: bool,
    pub bind_addr: String,
    pub server_cert_path: PathBuf,
    pub server_key_path: PathBuf,
    pub client_ca_path: PathBuf,
    pub client_cert_path: PathBuf,
}

impl Default for EmqxExhookConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            bind_addr: DEFAULT_EMQX_EXHOOK_BIND_ADDR.to_string(),
            server_cert_path: PathBuf::from(DEFAULT_EMQX_EXHOOK_SERVER_CERT),
            server_key_path: PathBuf::from(DEFAULT_EMQX_EXHOOK_SERVER_KEY),
            client_ca_path: PathBuf::from(DEFAULT_EMQX_EXHOOK_CLIENT_CA),
            client_cert_path: PathBuf::from(DEFAULT_EMQX_EXHOOK_CLIENT_CERT),
        }
    }
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            plugin_id: DEFAULT_SCHEMA_PLUGIN_ID.to_string(),
            schema_path: crate::sealed_schema_reader::blob_catalog_dir_from_env(),
            ui_workspace_shm_dir: PathBuf::from(DEFAULT_UI_WORKSPACE_SHM_DIR),
            generation_shm_dir: PathBuf::from(DEFAULT_GENERATION_SHM_DIR),
            unix_socket: PathBuf::from(DEFAULT_UNIX_SOCKET),
            shared_socket: PathBuf::from(DEFAULT_SHARED_SOCKET),
            bind_addr: DEFAULT_BIND_ADDR.to_string(),
            tls_identity: None,
            emqx_exhook: EmqxExhookConfig::default(),
        }
    }
}

impl ServerConfig {
    /// Load configuration from environment variables, falling back to defaults.
    pub fn from_env() -> Self {
        Self {
            plugin_id: configured_schema_plugin_id(|key| std::env::var(key).ok()),
            schema_path: crate::sealed_schema_reader::blob_catalog_dir_from_env(),
            ui_workspace_shm_dir: PathBuf::from(
                std::env::var("OP_UI_WORKSPACE_SHM_DIR")
                    .unwrap_or_else(|_| DEFAULT_UI_WORKSPACE_SHM_DIR.to_string()),
            ),
            generation_shm_dir: PathBuf::from(
                std::env::var("OP_GENERATION_SHM_DIR")
                    .unwrap_or_else(|_| DEFAULT_GENERATION_SHM_DIR.to_string()),
            ),
            unix_socket: PathBuf::from(
                std::env::var("TCHED_ROUTER_UNIX_SOCKET")
                    .unwrap_or_else(|_| DEFAULT_UNIX_SOCKET.to_string()),
            ),
            shared_socket: PathBuf::from(
                std::env::var("GHOSTBRIDGE_SOCKET_PATH")
                    .unwrap_or_else(|_| DEFAULT_SHARED_SOCKET.to_string()),
            ),
            bind_addr: std::env::var("TCHED_ROUTER_BIND_ADDR")
                .or_else(|_| std::env::var("GRPC_BIND"))
                .unwrap_or_else(|_| DEFAULT_BIND_ADDR.to_string()),
            tls_identity: Self::load_tls_identity(),
            emqx_exhook: EmqxExhookConfig {
                enabled: env_flag("OP_EMQX_EXHOOK_ENABLE"),
                bind_addr: std::env::var("OP_EMQX_EXHOOK_BIND_ADDR")
                    .unwrap_or_else(|_| DEFAULT_EMQX_EXHOOK_BIND_ADDR.to_string()),
                server_cert_path: env_path(
                    "OP_EMQX_EXHOOK_SERVER_CERT_FILE",
                    DEFAULT_EMQX_EXHOOK_SERVER_CERT,
                ),
                server_key_path: env_path(
                    "OP_EMQX_EXHOOK_SERVER_KEY_FILE",
                    DEFAULT_EMQX_EXHOOK_SERVER_KEY,
                ),
                client_ca_path: env_path(
                    "OP_EMQX_EXHOOK_CLIENT_CA_FILE",
                    DEFAULT_EMQX_EXHOOK_CLIENT_CA,
                ),
                client_cert_path: env_path(
                    "OP_EMQX_EXHOOK_CLIENT_CERT_FILE",
                    DEFAULT_EMQX_EXHOOK_CLIENT_CERT,
                ),
            },
        }
    }

    /// Load TLS identity for the TCP door.
    ///
    /// Priority:
    ///   1. `TCHED_ROUTER_TLS_CERT` + `TCHED_ROUTER_TLS_KEY` env vars (PEM contents).
    ///   2. `TCHED_ROUTER_TLS_CERT_FILE` + `TCHED_ROUTER_TLS_KEY_FILE` env vars pointing
    ///      at PEM files on disk — this is what the runit run script exports
    ///      (`/etc/op-dbus/tls/tonic-svc0.crt` / `.key`). Dropping this branch is
    ///      how the TCP door has regressed repeatedly (2026-08-22, -08-23,
    ///      -08-24): the binary aborts with "TCP fabric requires TLS" and the
    ///      mesh :8090 never binds.
    ///   3. Self-signed via rcgen **only** if `TCHED_ROUTER_DEV_SELF_SIGNED=1` is set
    ///      (dev/CI use only — never set on production; op-web and mesh peers will
    ///      reject an unverified cert and connections will fail at the TLS handshake).
    ///   4. `None` — startup will abort with a clear error rather than silently
    ///      serving unencrypted gRPC (zero-trust: TLS is mandatory on TCP).
    fn load_tls_identity() -> Option<Identity> {
        let cert_pem = std::env::var("TCHED_ROUTER_TLS_CERT").ok();
        let key_pem = std::env::var("TCHED_ROUTER_TLS_KEY").ok();

        if let (Some(cert), Some(key)) = (cert_pem, key_pem) {
            tracing::info!("TLS identity loaded from TCHED_ROUTER_TLS_CERT/TCHED_ROUTER_TLS_KEY");
            return Some(Identity::from_pem(cert, key));
        }

        // Certificate *files* on disk (the deployment path). Both must be set;
        // a half-configured pair falls through to the loud error below instead
        // of guessing.
        let cert_file = std::env::var("TCHED_ROUTER_TLS_CERT_FILE").ok();
        let key_file = std::env::var("TCHED_ROUTER_TLS_KEY_FILE").ok();
        if let (Some(cert_path), Some(key_path)) = (cert_file, key_file) {
            match (
                std::fs::read_to_string(&cert_path),
                std::fs::read_to_string(&key_path),
            ) {
                (Ok(cert), Ok(key)) => {
                    tracing::info!(cert = %cert_path, key = %key_path,
                        "TLS identity loaded from certificate files");
                    return Some(Identity::from_pem(cert, key));
                }
                (cert_res, key_res) => {
                    tracing::error!(
                        cert_path = %cert_path,
                        key_path = %key_path,
                        cert_error = ?cert_res.err(),
                        key_error = ?key_res.err(),
                        "failed to read TCHED_ROUTER_TLS_CERT_FILE/TCHED_ROUTER_TLS_KEY_FILE"
                    );
                    return None;
                }
            }
        }

        // Dev/CI escape hatch: self-signed cert. Never set this in production.
        if std::env::var("TCHED_ROUTER_DEV_SELF_SIGNED").as_deref() == Ok("1") {
            match rcgen::generate_simple_self_signed(vec![
                "localhost".to_string(),
                "ghostbridge.tech".to_string(),
                "3tched.com".to_string(),
            ]) {
                Ok(ck) => {
                    tracing::warn!(
                        "TCHED_ROUTER_DEV_SELF_SIGNED=1: using ephemeral self-signed TLS cert. \
                         DO NOT USE IN PRODUCTION — peers will reject this cert."
                    );
                    return Some(Identity::from_pem(
                        ck.cert.pem(),
                        ck.key_pair.serialize_pem(),
                    ));
                }
                Err(e) => {
                    tracing::error!(error = %e, "failed to generate self-signed TLS cert");
                    return None;
                }
            }
        }

        tracing::error!(
            "No TLS identity configured for TCP door. \
             Set TCHED_ROUTER_TLS_CERT+TCHED_ROUTER_TLS_KEY (PEM), \
             TCHED_ROUTER_TLS_CERT_FILE+TCHED_ROUTER_TLS_KEY_FILE (paths), \
             or TCHED_ROUTER_DEV_SELF_SIGNED=1 (dev only). \
             TCP listeners will not start."
        );
        None
    }
}

fn env_path(key: &str, default: &str) -> PathBuf {
    PathBuf::from(std::env::var(key).unwrap_or_else(|_| default.to_string()))
}

fn env_flag(key: &str) -> bool {
    std::env::var(key)
        .map(|value| matches!(value.trim(), "1" | "true" | "yes" | "on"))
        .unwrap_or(false)
}

#[derive(Clone)]
struct EmqxExhookPeerGate {
    expected_cert_sha256: [u8; 32],
}

impl tonic::service::Interceptor for EmqxExhookPeerGate {
    fn call(&mut self, request: TonicRequest<()>) -> Result<TonicRequest<()>, Status> {
        let info = request
            .extensions()
            .get::<TlsConnectInfo<TcpConnectInfo>>()
            .ok_or_else(|| Status::unauthenticated("EMQX ExHook requires mutual TLS"))?;
        let peer = info
            .get_ref()
            .remote_addr()
            .ok_or_else(|| Status::permission_denied("EMQX ExHook peer address is unavailable"))?;
        if !peer.ip().is_loopback() {
            return Err(Status::permission_denied(
                "EMQX ExHook accepts loopback peers only",
            ));
        }
        let certs = info
            .peer_certs()
            .ok_or_else(|| Status::unauthenticated("EMQX ExHook client certificate is missing"))?;
        let leaf = certs
            .first()
            .ok_or_else(|| Status::unauthenticated("EMQX ExHook client certificate is empty"))?;
        let actual: [u8; 32] = Sha256::digest(leaf.as_ref()).into();
        if actual != self.expected_cert_sha256 {
            return Err(Status::permission_denied(
                "EMQX ExHook client certificate is not the pinned broker identity",
            ));
        }
        Ok(request)
    }
}

fn first_certificate_sha256(path: &std::path::Path) -> anyhow::Result<[u8; 32]> {
    let file = std::fs::File::open(path).map_err(|error| {
        anyhow::anyhow!(
            "cannot open pinned EMQX client certificate {}: {error}",
            path.display()
        )
    })?;
    let mut reader = BufReader::new(file);
    let certificate = rustls_pemfile::certs(&mut reader)
        .next()
        .transpose()
        .map_err(|error| {
            anyhow::anyhow!(
                "cannot parse pinned EMQX client certificate {}: {error}",
                path.display()
            )
        })?
        .ok_or_else(|| {
            anyhow::anyhow!(
                "pinned EMQX client certificate {} contains no certificate",
                path.display()
            )
        })?;
    Ok(Sha256::digest(certificate.as_ref()).into())
}

fn configured_schema_plugin_id(getenv: impl Fn(&str) -> Option<String>) -> String {
    getenv("OP_DBUS_SCHEMA_PLUGIN_ID")
        .or_else(|| getenv("TCHED_ROUTER_PLUGIN_ID"))
        .unwrap_or_else(|| DEFAULT_SCHEMA_PLUGIN_ID.to_string())
}

/// Shared state for the gRPC service handlers.
#[derive(Clone)]
struct TchedRouterGrpcService {
    loader: Arc<SchemaLoader>,
}

#[tonic::async_trait]
impl TchedRouterService for TchedRouterGrpcService {
    type WatchSchemaStream =
        Pin<Box<dyn tokio_stream::Stream<Item = Result<SchemaEvent, Status>> + Send>>;

    async fn get_schema(
        &self,
        request: TonicRequest<GetSchemaRequest>,
    ) -> Result<TonicResponse<SchemaResponse>, Status> {
        let context = request
            .extensions()
            .get::<TraceContext>()
            .cloned()
            .unwrap_or_else(|| TraceContext::from_tonic_metadata(request.metadata()));

        let schema_json = self.loader.get().await.to_string();
        let mut response = TonicResponse::new(SchemaResponse {
            schema_json,
            trace_id: context.trace_id.clone(),
            session_genesis: context.session_genesis.clone(),
        });
        inject_trace_metadata(&mut response, &context);
        Ok(response)
    }

    async fn watch_schema(
        &self,
        _request: TonicRequest<WatchSchemaRequest>,
    ) -> Result<TonicResponse<Self::WatchSchemaStream>, Status> {
        let mut reload_rx = self.loader.reload_tx().subscribe();
        let loader = self.loader.clone();

        let initial = loader.get().await;
        let stream = stream! {
            yield Ok(SchemaEvent {
                schema_json: initial.to_string(),
                event_type: "initial".to_string(),
            });

            while let Ok(SchemaReloadEvent { event_type }) = reload_rx.recv().await {
                let schema_json = loader.get().await.to_string();
                yield Ok(SchemaEvent {
                    schema_json,
                    event_type,
                });
            }
        };

        Ok(TonicResponse::new(Box::pin(stream)))
    }
}

fn inject_trace_metadata<T>(response: &mut TonicResponse<T>, context: &TraceContext) {
    if let Ok(trace_id) = context.trace_id.parse() {
        response
            .metadata_mut()
            .insert("x-ghostbridge-trace-id", trace_id);
    }
    if let Ok(session_genesis) = context
        .session_genesis
        .parse::<tonic::metadata::MetadataValue<tonic::metadata::Ascii>>()
    {
        response
            .metadata_mut()
            .insert("x-ghostbridge-genesis", session_genesis);
    }
}

/// Assemble the gRPC service set for the tched_router bridge.
///
/// The full backplane surface (StateSync, PluginService, DbusPassthrough,
/// OvsdbMirror, RuntimeMirror, EventChainService, ComponentRegistry, Mail,
/// Privacy, Registration, McpService, ChatService, reflection) comes from the
/// shared [`build_operation_routes`] — identical to what `run_grpc_server` mounts
/// on op-dbus :50051, so reflection never advertises an unmounted service. On top
/// of that the tched_router bridge adds its endpoint-specific `TchedRouterService`
/// (plugin-owned schema get/watch).
///
/// Every container shares this one surface over `container.sock`; the assistant
/// container is provisioned exactly like the others (its socket created via the
/// unix-socket plugin's createsocket through PluginService, not a raw Incus proxy
/// device).
fn build_routes(
    loader: Arc<SchemaLoader>,
    server: OperationGrpcServer,
    validator: Arc<AssertionValidator>,
) -> tonic::service::Routes {
    let tched_router_svc =
        crate::grpc_web::enable(TchedRouterServiceServer::new(TchedRouterGrpcService {
            loader,
        }));
    build_operation_routes_with_validator(server, validator).add_service(tched_router_svc)
}

/// Build the shared TCP ingress: native MCP HTTP plus gRPC and gRPC-Web.
/// The router owns no listener; production serves it through the existing
/// tonic TLS acceptor on `:8090`.
pub fn build_axum_app(loader: Arc<SchemaLoader>, server: OperationGrpcServer) -> Router {
    let validator = Arc::new(AssertionValidator::from_env(DecoyTrustStore::load()));
    build_axum_app_with_validator(loader, server, validator)
}

/// Build the shared ingress with an already validated MCP projection policy.
///
/// Production callers should use [`build_axum_app`], which keeps the
/// root-protected environment policy loading in the normal MCP constructor.
/// This explicit form is for isolated integration fixtures; it preserves the
/// same assertion validator, interceptor, and authorization routes.
pub fn build_axum_app_with_policy(
    loader: Arc<SchemaLoader>,
    server: OperationGrpcServer,
    policy: crate::mcp_policy::McpProjectionPolicy,
) -> Router {
    let validator = Arc::new(AssertionValidator::from_env(DecoyTrustStore::load()));
    let engine = server.mutation_engine();
    let mcp = crate::mcp_frontend::build_mcp_router_with_policy(engine, validator.clone(), policy);
    build_axum_app_with_mcp(loader, server, validator, mcp)
}

fn build_axum_app_with_validator(
    loader: Arc<SchemaLoader>,
    server: OperationGrpcServer,
    validator: Arc<AssertionValidator>,
) -> Router {
    let engine = server.mutation_engine();
    let mcp = crate::mcp_frontend::build_mcp_router(engine, validator.clone());
    build_axum_app_with_mcp(loader, server, validator, mcp)
}

fn build_axum_app_with_mcp(
    loader: Arc<SchemaLoader>,
    server: OperationGrpcServer,
    validator: Arc<AssertionValidator>,
    mcp: Router,
) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(crate::mcp_frontend::configured_allow_origin())
        .allow_credentials(true)
        .allow_methods([
            axum::http::Method::GET,
            axum::http::Method::POST,
            axum::http::Method::DELETE,
        ])
        .allow_headers([
            axum::http::header::ACCEPT,
            axum::http::header::CONTENT_TYPE,
            "x-grpc-web".parse().unwrap(),
            "x-user-agent".parse().unwrap(),
            "grpc-timeout".parse().unwrap(),
            crate::mcp_frontend::HTTP_ASSERTION_HEADER.parse().unwrap(),
            crate::mcp_frontend::HTTP_SEALED_ID_HEADER.parse().unwrap(),
            crate::mcp_frontend::MCP_VERSION_HEADER.parse().unwrap(),
            crate::mcp_frontend::MCP_SESSION_HEADER.parse().unwrap(),
            crate::mcp_frontend::MCP_METHOD_HEADER.parse().unwrap(),
            crate::mcp_frontend::MCP_NAME_HEADER.parse().unwrap(),
            crate::grpc_server::DECLARED_CAPABILITY_HEADER
                .parse()
                .unwrap(),
        ])
        .expose_headers([
            "grpc-status".parse().unwrap(),
            "grpc-message".parse().unwrap(),
            "grpc-status-details-bin".parse().unwrap(),
            crate::mcp_frontend::MCP_SESSION_HEADER.parse().unwrap(),
        ]);

    mcp.merge(build_routes(loader, server, validator).into_axum_router())
        .layer(cors)
        .layer(GhostbridgeTraceLayer::new())
}

/// Build the tonic `Routes` used for the native-gRPC Unix socket side.
fn build_tonic_routes(
    loader: Arc<SchemaLoader>,
    server: OperationGrpcServer,
    validator: Arc<AssertionValidator>,
) -> tonic::service::Routes {
    build_routes(loader, server, validator)
}

/// Run the tched_router Axum host.
///
/// Returns only when both listeners exit (which should not happen under normal
/// operation).
pub async fn run_tched_router_server(config: ServerConfig) -> anyhow::Result<()> {
    let catalog_dir = crate::sealed_schema_reader::blob_catalog_dir_from_env();
    let legacy_path = std::env::var_os("TCHED_ROUTER_SCHEMA_PATH").map(PathBuf::from);
    validate_catalog_selection(&config.schema_path, &catalog_dir, legacy_path.as_deref())?;
    let loader = Arc::new(SchemaLoader::new_for_plugin(
        &config.schema_path,
        config.plugin_id.clone(),
    )?);

    // SIGHUP reload watcher.
    let _sighup_handle = loader.clone().watch_sighup();

    // PluginService enters the canonical MutationEngine after request identity
    // and exact capability validation; omitted actors inherit no identity.
    let event_chain = Arc::new(tokio::sync::RwLock::new(op_state_store::EventChain::new(
        op_state_store::ChainConfig::default(),
    )));
    let ovsdb = Arc::new(op_network::rovs_proxy::OvsdbDbusClient::new());
    let mutation_engine = Arc::new(MutationEngine::new(event_chain, ovsdb));
    mutation_engine.configure_shared_socket_path(&config.shared_socket)?;
    // Prepare only the held owner directory. Per-owner namespaces and durable
    // workspace rows remain absent until an admitted explicit recovery/mutation.
    let ui_workspace_root =
        op_core::projection_shm::PrivateProjectionDirectory::prepare(&config.ui_workspace_shm_dir)?;
    mutation_engine.configure_chatbot_episode_projection(
        ui_workspace_root.prepare_child("chatbot-episodes")?,
        crate::sealed_schema_reader::blob_catalog_dir_from_env(),
    )?;
    mutation_engine.configure_ui_workspace(
        ui_workspace_root,
        op_plugins::state_plugins::runtime_documents::runtime_documents().await?,
        crate::sealed_schema_reader::blob_catalog_dir_from_env(),
    )?;
    let generation_root =
        op_core::projection_shm::PrivateProjectionDirectory::prepare(&config.generation_shm_dir)?;
    generation_root.ensure_distinct_from(&config.ui_workspace_shm_dir)?;
    mutation_engine.configure_generation_jobs(
        generation_root,
        op_plugins::state_plugins::runtime_documents::runtime_documents().await?,
        crate::sealed_schema_reader::blob_catalog_dir_from_env(),
        Arc::new(crate::tched_router_runtime::TchedRouterRuntimeClient::from_env()),
    )?;
    // No prompt replay or automatic activation: each verified owner must invoke
    // recover_generation_jobs. A compatible stateless native ACK is required
    // later for every start; preparing these directories proves no live support.
    // One validator is shared by every projection/listener so an assertion
    // consumed through MCP cannot be replayed through gRPC (or vice versa).
    let assertion_validator = Arc::new(AssertionValidator::from_env(DecoyTrustStore::load()));
    // Open the durable audit sink and replay the pre-restart trail before any
    // surface accepts traffic, so an immediate query sees prior history.
    let replayed = mutation_engine.init_audit_durability().await;
    if replayed > 0 {
        tracing::info!(replayed, "audit trail restored from durable storage");
    }
    mutation_engine.initialize_identity_sled_store().await?;
    crate::identity_sled_dispatch::recover_identity_startup(mutation_engine.as_ref()).await?;
    mutation_engine.seed_missing_plugin_projections().await?;
    // D-Bus bindings do not survive a bridge restart. Invalidate every
    // instance-backed sled that was durably active, then stop its container,
    // before MCP/gRPC/D-Bus routes can be built or listeners exposed. Host
    // identities have no Incus instance and remain untouched.
    let parked =
        crate::identity_sled_dispatch::park_orphaned_container_sessions(mutation_engine.as_ref())
            .await?;
    if parked > 0 {
        tracing::warn!(
            parked,
            "parked orphaned identity containers before exposing fabric routes"
        );
    }
    // The control-plane chatbot's service identity is release-configured and sealed
    // by the same MutationEngine that owns every other session arrival.  This
    // identity belongs only to that singleton service and must not be borrowed
    // by Codex or another local human client.
    mutation_engine
        .bootstrap_control_plane_chatbot_identity()
        .await?;
    // A configured local human uses the same WireGuard-derived principal on
    // every surface. Mint its SID1 into the human sled so local clients can
    // authenticate without inventing a per-client service identity.
    mutation_engine
        .bootstrap_configured_local_human_identity()
        .await?;
    let mutation_engine_for_dbus = mutation_engine.clone();
    mutation_engine.clone().start().await?;
    let operation_server = OperationGrpcServer::new(mutation_engine.clone());
    // The sealed SHM blob catalog IS the plugin set: hydrate reflection from
    // it so a bridge restart advertises every sealed plugin immediately.
    operation_server.hydrate_reflection_from_shm().await;

    // Freeze schema/authority metadata before constructing the server. This
    // registers descriptors, not RPC handlers: the generated add_routes mounts
    // the typed aggregate services. Reflection advertises that same route table,
    // restricted to plugins whose sealed blobs are present.
    operation_server.freeze_plugin_method_reflection().await;

    // ── D-Bus plugin object registration ──────────────────────────────────
    // Register all plugin objects from the SHM blob catalog on the session bus
    // so `busctl tree org.opdbus.v1.plugins` shows the full plugin tree.
    // SchemaRouter reads sealed blobs and creates SchemaBackedInterface objects
    // with real methods/properties matching each plugin's schema, dispatching
    // calls through the same MutationEngine as the gRPC path.
    let assertion_validator_for_dbus = assertion_validator.clone();
    let mut catalog_reload_rx = loader.reload_tx().subscribe();
    tokio::spawn(async move {
        let addr = std::env::var("DBUS_SESSION_BUS_ADDRESS")
            .unwrap_or_else(|_| op_core::config::SESSION_BUS_ADDRESS.to_string());
        let conn = match zbus::connection::Builder::address(addr.as_str()) {
            Ok(builder) => match builder.build().await {
                Ok(c) => c,
                Err(e) => {
                    tracing::error!(error = %e, "Failed to connect to session bus for D-Bus plugin objects");
                    return;
                }
            },
            Err(e) => {
                tracing::error!(error = %e, "Invalid session bus address for D-Bus plugin objects");
                return;
            }
        };

        let dbus_conn = Arc::new(tokio::sync::OnceCell::new());
        let _ = dbus_conn.set(conn.clone());
        let engine_for_signal = mutation_engine_for_dbus.clone();
        let identity_resolver = crate::dbus_identity::DbusIdentityResolver::new(
            conn.clone(),
            mutation_engine_for_dbus.clone(),
            assertion_validator_for_dbus,
        );
        if let Err(error) = identity_resolver.start_exit_monitor().await {
            tracing::error!(%error, "D-Bus identity exit monitor failed to start");
            return;
        }
        if let Err(error) = conn
            .object_server()
            .at(
                crate::dbus_identity::DBUS_IDENTITY_OBJECT_PATH,
                crate::dbus_identity::DbusIdentityBootstrap::new(identity_resolver.clone()),
            )
            .await
        {
            tracing::error!(%error, "D-Bus identity bootstrap interface failed to register");
            return;
        }
        let router = SchemaRouter::with_engine(dbus_conn, mutation_engine_for_dbus)
            .with_identity_resolver(identity_resolver);

        if let Err(e) = router.register_objects().await {
            tracing::error!(error = %e, "Failed to register authoritative D-Bus plugin objects");
        } else {
            // Store the signal bus on the MutationEngine so emit_updated_signal works.
            engine_for_signal.set_signal_bus(conn.clone());
            // Request the canonical bus name
            match conn
                .request_name(op_plugins::canonical::BASE_SERVICE_NAME)
                .await
            {
                Ok(_) => {
                    let count = router.list_plugin_ids().await.len();
                    tracing::info!(
                        count,
                        "D-Bus plugin objects registered under org.opdbus.v1.plugins"
                    );
                }
                Err(e) => {
                    tracing::warn!(error = %e, "D-Bus name request failed");
                }
            }
        }

        // The existing explicit reload signal also refreshes the canonical
        // D-Bus objects and wakes SHM subscribers, including schema removals.
        while let Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) =
            catalog_reload_rx.recv().await
        {
            if let Err(error) = router.reload().await {
                tracing::warn!(%error, "D-Bus catalog reload failed");
            }
            if let Err(error) = engine_for_signal.clone().start().await {
                tracing::warn!(%error, "Native observation restart failed after explicit reload");
            }
        }
    });
    // Attach the semantic shuttle for parity with run_grpc_server so
    // SearchSemanticTrace behaves identically on both endpoints (best-effort:
    // if Qdrant is unavailable the method returns failed_precondition).
    let operation_server = match op_cognitive_mcp::QdrantSemanticShuttle::new().await {
        Ok(shuttle) => operation_server.with_semantic_shuttle(Arc::new(shuttle)),
        Err(error) => {
            tracing::warn!(%error, "semantic shuttle unavailable; SearchSemanticTrace will return failed_precondition");
            operation_server
        }
    };
    match crate::sealed_schema_reader::read_manifest_pinned_schema_document(
        &crate::sealed_schema_reader::blob_catalog_dir_from_env(),
        &config.plugin_id,
    ) {
        Ok(pinned) => {
            let blob = pinned.blob;
            tracing::info!(
                plugin_id = %blob.manifest.plugin_id,
                schema_hash = %blob.manifest.schema_hash,
                methods = blob.manifest.methods.len(),
                dbus_path = %blob.manifest.dbus.object_path,
                grpc_service = ?blob.manifest.grpc.services,
                "loaded manifest-pinned tched_router object metadata"
            );
            if let Err(error) = operation_server
                .register_plugin_methods(config.plugin_id.clone(), &pinned.schema)
                .await
            {
                tracing::warn!(
                    plugin_id = %config.plugin_id,
                    %error,
                    "failed to register pinned tched_router method descriptors for gRPC reflection"
                );
            }
        }
        Err(error) => {
            tracing::warn!(
                plugin_id = %config.plugin_id,
                %error,
                "pinned tched_router blob unavailable for gRPC reflection"
            );
        }
    }

    // Broker callbacks have a different trust domain from human/model calls.
    // ExHook therefore receives a dedicated loopback-only mTLS listener with
    // one mounted service and an exact client-certificate pin. It is never
    // added to the shared :8090/container.sock route set.
    let emqx_exhook_server = if config.emqx_exhook.enabled {
        let exhook = &config.emqx_exhook;
        let bind_addr: SocketAddr = exhook.bind_addr.parse().map_err(|error| {
            anyhow::anyhow!(
                "invalid OP_EMQX_EXHOOK_BIND_ADDR '{}': {error}",
                exhook.bind_addr
            )
        })?;
        if !bind_addr.ip().is_loopback() {
            anyhow::bail!(
                "OP_EMQX_EXHOOK_BIND_ADDR must be loopback, got {}",
                bind_addr
            );
        }
        let server_cert = std::fs::read(&exhook.server_cert_path).map_err(|error| {
            anyhow::anyhow!(
                "cannot read EMQX ExHook server certificate {}: {error}",
                exhook.server_cert_path.display()
            )
        })?;
        let server_key = std::fs::read(&exhook.server_key_path).map_err(|error| {
            anyhow::anyhow!(
                "cannot read EMQX ExHook server key {}: {error}",
                exhook.server_key_path.display()
            )
        })?;
        let client_ca = std::fs::read(&exhook.client_ca_path).map_err(|error| {
            anyhow::anyhow!(
                "cannot read EMQX ExHook client CA {}: {error}",
                exhook.client_ca_path.display()
            )
        })?;
        let expected_cert_sha256 = first_certificate_sha256(&exhook.client_cert_path)?;
        let peer_gate = EmqxExhookPeerGate {
            expected_cert_sha256,
        };
        let hook_provider =
            crate::proto::emqx_exhook::hook_provider_server::HookProviderServer::new(
                crate::emqx_hook_provider::HookProviderService::new(mutation_engine.clone()),
            )
            .max_decoding_message_size(1024 * 1024)
            .max_encoding_message_size(1024 * 1024);
        let hook_provider =
            tonic::service::interceptor::InterceptedService::new(hook_provider, peer_gate);
        let tls = ServerTlsConfig::new()
            .identity(Identity::from_pem(server_cert, server_key))
            .client_ca_root(Certificate::from_pem(client_ca));
        let listener = tokio::net::TcpListener::bind(bind_addr)
            .await
            .map_err(|error| anyhow::anyhow!("cannot bind EMQX ExHook at {bind_addr}: {error}"))?;
        info!(
            addr = %bind_addr,
            client_cert = %exhook.client_cert_path.display(),
            "EMQX ExHook listening on dedicated loopback mTLS endpoint"
        );
        Some(
            tonic::transport::Server::builder()
                .timeout(Duration::from_secs(10))
                .concurrency_limit_per_connection(32)
                .tls_config(tls)
                .map_err(|error| anyhow::anyhow!("invalid EMQX ExHook TLS config: {error}"))?
                .add_service(hook_provider)
                .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener)),
        )
    } else {
        info!("EMQX ExHook listener disabled");
        None
    };

    // Controllers retain an exclusive ownership lease. An existing path is
    // never removed without matching prior ownership and proven staleness.
    let unix_socket = config.unix_socket.clone();
    let unix_controller =
        crate::shared_socket::SharedListenerController::open(unix_socket.clone())?;
    unix_controller.start(128)?;
    let unix_incoming = unix_controller.take_incoming()?;
    info!(path = %unix_socket.display(), "tched_router native gRPC listening on Unix socket");

    // Shared container socket — same gRPC surface, path bind-mounted into CTs.
    // Skip a second bind when env points both knobs at the same path.
    let shared_socket = config.shared_socket.clone();
    let shared_incoming = if shared_socket != unix_socket {
        let controller =
            crate::shared_socket::SharedListenerController::open(shared_socket.clone())?;
        controller.start(128)?;
        let incoming = controller.take_incoming()?;
        mutation_engine.set_shared_listener(controller)?;
        mutation_engine.publish_shared_listener_startup().await?;
        info!(
            path = %shared_socket.display(),
            "shared container.sock listening (ghostbridge UDS fabric)"
        );
        Some(incoming)
    } else {
        mutation_engine.set_shared_listener(unix_controller.clone())?;
        mutation_engine.publish_shared_listener_startup().await?;
        info!(
            path = %shared_socket.display(),
            "shared container socket coincides with host UDS; single listener"
        );
        None
    };

    // accept_http1(true): tonic-web serves gRPC-Web over HTTP/1.1, which is what
    // browsers (and xray's tls-h1 fallback forwarding to this socket) speak. Without
    // it the socket is h2-only and rejects browser gRPC-Web. Matches the TCP/axum side.
    let tonic_server = tonic::transport::Server::builder()
        .accept_http1(true)
        .add_routes(build_tonic_routes(
            loader.clone(),
            operation_server.clone(),
            assertion_validator.clone(),
        ))
        .serve_with_incoming(unix_incoming);

    let shared_server = shared_incoming.map(|incoming| {
        tonic::transport::Server::builder()
            .accept_http1(true)
            .add_routes(build_tonic_routes(
                loader.clone(),
                operation_server.clone(),
                assertion_validator.clone(),
            ))
            .serve_with_incoming(incoming)
    });

    // TCP door: always TLS (zero-trust transport policy).
    // bind_addr is comma-separated; each address gets its own TLS listener.
    let identity = config.tls_identity.ok_or_else(|| {
        anyhow::anyhow!(
            "TCP fabric requires TLS — set TCHED_ROUTER_TLS_CERT+TCHED_ROUTER_TLS_KEY, \
             TCHED_ROUTER_TLS_CERT_FILE+TCHED_ROUTER_TLS_KEY_FILE, \
             or TCHED_ROUTER_DEV_SELF_SIGNED=1 (dev only)"
        )
    })?;
    let bind_addrs: Vec<&str> = config
        .bind_addr
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    if bind_addrs.is_empty() {
        anyhow::bail!("TCHED_ROUTER_BIND_ADDR is empty — no TCP addresses to bind");
    }
    let mut tcp_tasks = Vec::new();
    // One MCP policy owner / axum app, cloned onto every TCP bind. Building
    // per-address panics: MutationEngine installs that owner exactly once.
    let ingress = build_axum_app_with_validator(
        loader.clone(),
        operation_server.clone(),
        assertion_validator.clone(),
    );
    for bind_addr_str in &bind_addrs {
        let bind_addr: SocketAddr = bind_addr_str.parse()?;
        let listener = tokio::net::TcpListener::bind(bind_addr)
            .await
            .map_err(|error| anyhow::anyhow!("cannot bind TCP fabric at {bind_addr}: {error}"))?;
        info!(addr = %bind_addr, "tched_router TLS gRPC/gRPC-Web listening on TCP");
        let tls_config = ServerTlsConfig::new().identity(identity.clone());
        let server = tonic::transport::Server::builder()
            .accept_http1(true)
            .tls_config(tls_config)
            .map_err(|e| anyhow::anyhow!("invalid TLS config for {bind_addr}: {e}"))?
            .add_routes(tonic::service::Routes::from(ingress.clone()))
            .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener));
        tcp_tasks.push(tokio::spawn(server));
    }

    // The service, not its launcher, owns readiness. At this point every UDS
    // and TCP listener has been bound successfully, so consumers cannot see a
    // stale marker for a process that never opened its sockets.
    if let Ok(ready_path) = std::env::var("OPDBUS_READY_PATH") {
        let ready_path = std::path::Path::new(&ready_path);
        if let Some(parent) = ready_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::write(ready_path, b"ready\n").await?;
    }

    // Drive all listeners concurrently.
    let shared_fut = async {
        match shared_server {
            Some(s) => s
                .await
                .map_err(|e| anyhow::anyhow!("Shared container.sock: {e}")),
            None => std::future::pending::<anyhow::Result<()>>().await,
        }
    };
    let unix_fut = async {
        tonic_server
            .await
            .map_err(|e| anyhow::anyhow!("Unix server error: {e}"))
    };
    let tcp_fut = async {
        let (result, _idx, _rest) = futures::future::select_all(tcp_tasks).await;
        match result {
            Ok(inner) => inner.map_err(|e| anyhow::anyhow!("TCP TLS server error: {e}")),
            Err(join_err) => Err(anyhow::anyhow!("TCP TLS server task panicked: {join_err}")),
        }
    };
    let emqx_exhook_fut = async {
        match emqx_exhook_server {
            Some(server) => server
                .await
                .map_err(|error| anyhow::anyhow!("EMQX ExHook server error: {error}")),
            None => std::future::pending::<anyhow::Result<()>>().await,
        }
    };

    // A listener exit is a process-level health failure. Returning here lets
    // runit restart the complete bridge instead of leaving a partial surface.
    tokio::select! {
        result = unix_fut => result,
        result = shared_fut => result,
        result = tcp_fut => result,
        result = emqx_exhook_fut => result,
    }
}

fn validate_catalog_selection(
    discovery: &std::path::Path,
    admission: &std::path::Path,
    legacy_override: Option<&std::path::Path>,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        discovery == admission,
        "schema discovery and method admission must use the same sealed catalog"
    );
    anyhow::ensure!(
        legacy_override.is_none_or(|path| path == admission),
        "TCHED_ROUTER_SCHEMA_PATH conflicts with OP_BLOB_CATALOG_DIR"
    );
    Ok(())
}

#[cfg(test)]
mod catalog_selection_tests {
    use super::validate_catalog_selection;
    use std::path::Path;

    #[test]
    fn discovery_rejects_conflicting_catalogs_before_server_start() {
        let canonical = Path::new("/tmp/sealed-a");
        let conflicting = Path::new("/tmp/sealed-b");
        assert!(validate_catalog_selection(canonical, canonical, None).is_ok());
        assert!(validate_catalog_selection(canonical, canonical, Some(canonical)).is_ok());
        assert!(validate_catalog_selection(conflicting, canonical, None).is_err());
        assert!(validate_catalog_selection(canonical, canonical, Some(conflicting)).is_err());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_server_loads_the_canonical_router_blob() {
        assert_eq!(ServerConfig::default().plugin_id, "tched_router");
    }

    #[test]
    fn env_free_server_startup_loads_the_canonical_router_blob() {
        assert_eq!(configured_schema_plugin_id(|_| None), "tched_router");
    }

    #[test]
    fn emqx_exhook_defaults_to_disabled_loopback() {
        let config = EmqxExhookConfig::default();
        assert!(!config.enabled);
        let address: SocketAddr = config.bind_addr.parse().expect("valid bind address");
        assert!(address.ip().is_loopback());
    }
}
