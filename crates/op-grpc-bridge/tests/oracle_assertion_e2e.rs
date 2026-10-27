//! E2E oracle identity assertion battery over real TLS (design.md section 4.4).
//!
//! The fixture serializes its process-wide environment and interceptor engine
//! state internally, so the default parallel Rust test harness is safe.

use std::collections::BTreeMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex, Once, OnceLock};
use std::time::Duration;

use axum::Router;
use axum::extract::ConnectInfo as AxumConnectInfo;
use axum::http::StatusCode;
use axum::routing::any;
use base64::Engine as _;
use ed25519_dalek::{Signer, SigningKey};
use op_cozo_store::{IdentityCompareExchange, IdentityIntent, IdentitySledRecord};
use op_grpc_bridge::grpc_server::{
    DECLARED_CAPABILITY_HEADER, OperationGrpcServer, build_operation_routes_with_validator,
};
use op_grpc_bridge::interceptor::{ASSERTION_METADATA_KEY, connect_info_peer_addr};
use op_grpc_bridge::mutation_engine::MutationEngine;
use op_grpc_bridge::oracle_assertion::{
    AssertionSourceBinding, AssertionValidator, DecoyTrustStore,
};
use op_grpc_bridge::proto::ovsdb_mirror_client::OvsdbMirrorClient;
use op_grpc_bridge::proto::plugin_methods::human_principal_plugin_methods_client::HumanPrincipalPluginMethodsClient;
use op_grpc_bridge::proto::plugin_methods::snowball_plugin_methods_client::SnowballPluginMethodsClient;
use op_grpc_bridge::proto::plugin_methods::{
    HumanPrincipalResolveKeyRequest, SnowballQueryEventsRequest,
};
use op_grpc_bridge::proto::plugin_service_client::PluginServiceClient;
use op_grpc_bridge::proto::state_sync_client::StateSyncClient;
use op_grpc_bridge::proto::{
    BatchMutateRequest, CallMethodRequest, ErrorCode as ProtoErrorCode, MutateRequest,
    OperationType, OvsdbTransactRequest, SetPropertyRequest,
};
use op_identity::oracle_assertion::{
    DecoyIssuer, OracleIdentityAssertion, SignedAssertion, verify_signature,
};
use op_identity::session::{derive_principal_id, derive_session_id};
use op_plugins::state_plugins::identity_sled::{
    ContainerIdentitySled, RECORD_FORMAT, SledBtrfsDevice,
};
use op_plugins::state_plugins::incus::IncusInstance;
use op_state::StatePlugin;
use op_state_store::{ChainConfig, EventChain};
use prost_types::Value as ProstValue;
use prost_types::value::Kind;
use tempfile::TempDir;
use tokio::net::TcpListener;
use tokio::sync::RwLock;
use tokio::task::JoinHandle;
use tonic::metadata::{MetadataMap, MetadataValue};
use tonic::transport::server::TcpConnectInfo;
use tonic::transport::{Certificate, Channel, ClientTlsConfig, Identity, ServerTlsConfig};
use tonic::{Code, Request, Status};
use tower::{Layer, Service};

/// Pinned battery size (VAL-E2E-023).
pub const EXPECTED_TEST_COUNT: usize = 41;

static CRYPTO: Once = Once::new();

fn install_crypto_provider() {
    CRYPTO.call_once(|| {
        rustls::crypto::aws_lc_rs::default_provider()
            .install_default()
            .expect("rustls CryptoProvider");
    });
}

fn test_ip() -> IpAddr {
    IpAddr::V4(Ipv4Addr::LOCALHOST)
}

fn alt_ip() -> IpAddr {
    IpAddr::V4(Ipv4Addr::new(10, 200, 0, 7))
}

fn pk(byte: u8) -> String {
    base64::engine::general_purpose::STANDARD.encode([byte; 32])
}

fn test_signing_key() -> SigningKey {
    SigningKey::from_bytes(&[7u8; 32])
}

fn test_issuer() -> DecoyIssuer {
    DecoyIssuer::new(test_signing_key(), "decoy-key-1", Duration::from_secs(900))
}

fn write_trust_store(dir: &TempDir, issuer: &DecoyIssuer) {
    let b64 = base64::engine::general_purpose::STANDARD.encode(issuer.verifying_key().to_bytes());
    let json = format!(
        "{{\"decoy_keys\": {{\"{}\": \"{}\"}}}}",
        issuer.key_id(),
        b64
    );
    let path = dir.path().join("decoy-trust.json");
    std::fs::write(&path, json).expect("trust store");
    // FIXME: Audit that the environment access only happens in single-threaded code.
    unsafe { std::env::set_var("OP_DECOY_TRUST_STORE", &path) };
}

fn write_grants(path: &std::path::Path, doc: &serde_json::Value) {
    std::fs::write(path, serde_json::to_vec(doc).unwrap()).expect("grants");
    // FIXME: Audit that the environment access only happens in single-threaded code.
    unsafe { std::env::set_var("OP_GRANTS_PATH", path) };
}

fn principal_id_for(pubkey: &str) -> String {
    derive_principal_id(pubkey)
}

fn grants_for(pubkey: &str, caps: &[&str]) -> serde_json::Value {
    let fp = principal_id_for(pubkey);
    let caps: Vec<_> = caps.iter().map(|c| (*c).to_string()).collect();
    serde_json::json!({ fp: { "capabilities": caps } })
}

fn grants_wildcard(caps: &[&str]) -> serde_json::Value {
    let caps: Vec<_> = caps.iter().map(|c| (*c).to_string()).collect();
    serde_json::json!({ "*": { "capabilities": caps } })
}

fn empty_grants() -> serde_json::Value {
    serde_json::json!({})
}

fn prost_str(s: &str) -> ProstValue {
    ProstValue {
        kind: Some(Kind::StringValue(s.to_string())),
    }
}

fn prost_struct(fields: BTreeMap<String, ProstValue>) -> ProstValue {
    ProstValue {
        kind: Some(Kind::StructValue(prost_types::Struct { fields })),
    }
}

fn resolve_key_mutation(pubkey: &str, actor_id: &str) -> MutateRequest {
    MutateRequest {
        plugin_id: "human_principal".to_string(),
        object_path: "/org/opdbus/v1/plugins/human_principal".to_string(),
        operation: OperationType::CallMethod as i32,
        member_name: "method:resolve_key".to_string(),
        value: Some(prost_struct(BTreeMap::from([(
            "human_pubkey".to_string(),
            prost_str(pubkey),
        )]))),
        actor_id: actor_id.to_string(),
        capability_id: "human_principal.read".to_string(),
        idempotency_key: String::new(),
    }
}

fn struct_fields(v: &ProstValue) -> &BTreeMap<String, ProstValue> {
    match &v.kind {
        Some(Kind::StructValue(s)) => &s.fields,
        _ => panic!("expected struct"),
    }
}

fn struct_field<'a>(v: &'a ProstValue, key: &str) -> &'a ProstValue {
    struct_fields(v)
        .get(key)
        .unwrap_or_else(|| panic!("missing field {key}"))
}

fn string_field(v: &ProstValue, key: &str) -> String {
    match &struct_field(v, key).kind {
        Some(Kind::StringValue(s)) => s.clone(),
        _ => panic!("expected string field {key}"),
    }
}

fn envelope_result(v: &ProstValue) -> &ProstValue {
    struct_field(v, "result")
}

fn principal_id_from(v: &ProstValue) -> String {
    string_field(
        struct_field(envelope_result(v), "principal"),
        "principal_id",
    )
}

fn human_pubkey_from(v: &ProstValue) -> String {
    string_field(
        struct_field(envelope_result(v), "principal"),
        "human_pubkey",
    )
}

fn signed_with_fields(
    issuer: &DecoyIssuer,
    human_pubkey: &str,
    inner_ip: IpAddr,
    issued_at: i64,
    expires_at: i64,
    nonce: [u8; 16],
    decoy_key_id: Option<&str>,
) -> SignedAssertion {
    let assertion = OracleIdentityAssertion {
        human_pubkey: human_pubkey.to_string(),
        issued_at,
        expires_at,
        nonce,
        tunnel_inner_ip: inner_ip,
        decoy_key_id: decoy_key_id.unwrap_or(issuer.key_id()).to_string(),
    };
    let signature = test_signing_key()
        .sign(&assertion.signing_bytes())
        .to_bytes();
    SignedAssertion {
        assertion,
        signature,
    }
}

fn fresh_signed(issuer: &DecoyIssuer, pubkey: &str, nonce: [u8; 16]) -> SignedAssertion {
    let now = chrono::Utc::now().timestamp();
    signed_with_fields(issuer, pubkey, test_ip(), now - 5, now + 300, nonce, None)
}

fn attach_assertion(meta: &mut MetadataMap, wire: &[u8]) {
    meta.insert_bin(ASSERTION_METADATA_KEY, MetadataValue::from_bytes(wire));
}

fn attach_capability(meta: &mut MetadataMap, cap: &str) {
    meta.insert(
        DECLARED_CAPABILITY_HEADER,
        MetadataValue::try_from(cap).expect("cap header"),
    );
}

fn attach_ghostbridge(meta: &mut MetadataMap, genesis: &str, trace: &str) {
    meta.insert(
        "x-ghostbridge-genesis",
        MetadataValue::try_from(genesis).expect("genesis"),
    );
    meta.insert(
        "x-ghostbridge-trace-id",
        MetadataValue::try_from(trace).expect("trace"),
    );
}

struct TestEnv {
    _isolation: tokio::sync::OwnedMutexGuard<()>,
    _root: TempDir,
    _cozo: TempDir,
    identity_cozo: op_cozo_store::CozoGraphShuttle,
    issuer: DecoyIssuer,
    engine: Mutex<Option<Arc<MutationEngine>>>,
}

impl TestEnv {
    async fn new() -> Self {
        static TEST_ISOLATION: OnceLock<Arc<tokio::sync::Mutex<()>>> = OnceLock::new();
        let isolation = TEST_ISOLATION
            .get_or_init(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone()
            .lock_owned()
            .await;
        let root = tempfile::tempdir().expect("root");
        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::set_var("OP_SHM_STATE_DIR", root.path().join("shm-state")) };
        let blob_dir = root.path().join("plugin-blobs");
        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::set_var("OP_BLOB_CATALOG_DIR", &blob_dir) };
        let mut blob_store = op_blob::catalog::BlobStore::open(&blob_dir).expect("blob store");
        for (plugin_id, schema) in [
            (
                "human_principal",
                op_plugins::state_plugins::human_principal::HumanPrincipalPlugin::new()
                    .schema()
                    .expect("human principal schema"),
            ),
            (
                "identity_sled",
                op_plugins::state_plugins::identity_sled::IdentitySledPlugin::new()
                    .schema()
                    .expect("identity sled schema"),
            ),
            (
                "snowball",
                op_plugins::state_plugins::snowball_plugin::SnowballPlugin::new()
                    .schema()
                    .expect("snowball schema"),
            ),
        ] {
            let blob = op_blob::blobify_plugin_schema(plugin_id, schema);
            blob_store.write(&blob).expect("seal E2E plugin schema");
        }
        let cozo = tempfile::tempdir().expect("cozo");
        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::set_var("OP_HUMAN_PRINCIPAL_COZO_DB_PATH", cozo.path().join("cozo")) };
        let identity_cozo =
            op_cozo_store::CozoGraphShuttle::new_in_memory().expect("identity Cozo");
        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::set_var("OP_AUTH_REPLAY_DB_PATH", root.path().join("auth-replay.db")) };
        let grants_path = root.path().join("capability-grants.json");
        write_grants(&grants_path, &empty_grants());
        let issuer = test_issuer();
        write_trust_store(&root, &issuer);
        Self {
            _isolation: isolation,
            _root: root,
            _cozo: cozo,
            identity_cozo,
            issuer,
            engine: Mutex::new(None),
        }
    }

    async fn bind_engine(&self, engine: Arc<MutationEngine>) {
        // A validator/listener restart can retain the same authoritative
        // engine. Its identity owner must not be configured a second time.
        if self
            .engine
            .lock()
            .expect("test engine lock")
            .as_ref()
            .is_some_and(|bound| Arc::ptr_eq(bound, &engine))
        {
            return;
        }
        engine
            .configure_identity_store(self.identity_cozo.clone())
            .expect("configure fixture identity store");
        engine
            .recover_configured_identity_store()
            .await
            .expect("recover fixture identity store");
        *self.engine.lock().expect("test engine lock") = Some(engine);
    }

    /// Seed the result of a successful identity-container provision through
    /// the same authoritative cache/Cozo/genesis path used by production.
    /// The E2E process does not create real Incus containers, so this supplies
    /// only the isolated provision record; authentication still exercises the
    /// production validator and SID1/genesis checks unchanged.
    async fn provision_identity(&self, pubkey: &str) {
        let engine = self
            .engine
            .lock()
            .expect("test engine lock")
            .clone()
            .expect("test engine bound");
        let session_id = derive_session_id(pubkey);
        if op_grpc_bridge::identity_sled_dispatch::dispatch_identity_sled_method(
            engine.as_ref(),
            "get_identity",
            &serde_json::json!({"session_id": &session_id}),
        )
        .await
        .is_ok()
        {
            return;
        }
        let now = chrono::Utc::now().timestamp();
        let btrfs_root = self._root.path().join("identity-btrfs");
        std::fs::create_dir_all(&btrfs_root).expect("identity btrfs fixture root");
        let sled = ContainerIdentitySled {
            session_id: session_id.clone(),
            wireguard_pubkey: pubkey.to_string(),
            interface: String::new(),
            peer_ip: None,
            mutation_index: 0,
            genesis: None,
            trace_id: String::new(),
            schema_version: RECORD_FORMAT,
            vector_id: String::new(),
            sealed_id: None,
            principal_kind: Some("human".to_string()),
            btrfs_device: Some(SledBtrfsDevice {
                device_path: btrfs_root.join("fstorage.img").display().to_string(),
                mount_point: btrfs_root.join("mount").display().to_string(),
                btrfs_uuid: "00000000-0000-0000-0000-000000000001".to_string(),
                cozo_id: session_id.clone(),
                attached: false,
            }),
            instance: Some(IncusInstance {
                name: session_id.clone(),
                status: "Stopped".to_string(),
                instance_type: "container".to_string(),
                profiles: vec!["identity".to_string()],
                config: Some(std::collections::HashMap::from([(
                    "boot.autostart".to_string(),
                    "false".to_string(),
                )])),
                ..Default::default()
            }),
            session_started_at: now,
            last_seen_at: now,
            active: false,
            expires_at: None,
            arrival_timestamp: 0,
            chain_head_at_arrival: String::new(),
            catalog_hash_at_arrival: String::new(),
            head_timestamp_at_arrival: 0,
        };
        let expected = self
            .identity_cozo
            .read_identity_revision(&session_id)
            .expect("read fixture identity revision");
        self.identity_cozo
            .apply_identity_intent(
                &session_id,
                &format!("fixture-provision-{}", uuid::Uuid::new_v4().simple()),
                IdentityIntent::CompareExchange(Box::new(IdentityCompareExchange {
                    expected,
                    record: Some(IdentitySledRecord {
                        session_id: sled.session_id.clone(),
                        wireguard_pubkey: sled.wireguard_pubkey.clone(),
                        interface: sled.interface.clone(),
                        peer_ip: sled.peer_ip.clone().unwrap_or_default(),
                        mutation_index: 0,
                        session_genesis: String::new(),
                        trace_id: String::new(),
                        schema_version: i64::from(RECORD_FORMAT),
                        vector_id: sled.vector_id.clone(),
                        sealed_id: String::new(),
                        principal_kind: sled.principal_kind.clone(),
                        btrfs_device_json: serde_json::to_string(
                            sled.btrfs_device.as_ref().expect("fixture btrfs device"),
                        )
                        .expect("serialize fixture btrfs device"),
                        instance_json: serde_json::to_string(
                            sled.instance.as_ref().expect("fixture instance"),
                        )
                        .expect("serialize fixture instance"),
                        session_started_at: sled.session_started_at,
                        last_seen_at: sled.last_seen_at,
                        active: false,
                        expires_at: 0,
                    }),
                    genesis: None,
                })),
            )
            .expect("commit fixture identity provision");
        engine
            .recover_configured_identity_store()
            .await
            .expect("publish fixture identity provision");
        engine
            .mint_and_store_genesis(&session_id, pubkey)
            .await
            .expect("fixture identity genesis");
        let anchored = op_grpc_bridge::identity_sled_dispatch::dispatch_identity_sled_method(
            engine.as_ref(),
            "get_identity",
            &serde_json::json!({"session_id": &session_id}),
        )
        .await
        .expect("read anchored fixture identity");
        let _: ContainerIdentitySled = serde_json::from_value(anchored["identity"].clone())
            .expect("anchored fixture identity type");
    }

    fn set_grants(&self, doc: &serde_json::Value) {
        let path = self._root.path().join("capability-grants.json");
        write_grants(&path, doc);
    }

    fn grant_human(&self, pubkey: &str, caps: &[&str]) {
        self.set_grants(&grants_for(pubkey, caps));
    }

    fn cozo_path(&self) -> std::path::PathBuf {
        self._cozo.path().join("cozo")
    }
}

struct RunningServer {
    addr: SocketAddr,
    ca_pem: String,
    engine: Arc<MutationEngine>,
    _task: tokio::task::JoinHandle<()>,
}

impl Drop for RunningServer {
    fn drop(&mut self) {
        self._task.abort();
    }
}

#[derive(Clone, Default)]
struct StripConnectInfoLayer;

impl<S> Layer<S> for StripConnectInfoLayer {
    type Service = StripConnectInfo<S>;
    fn layer(&self, inner: S) -> Self::Service {
        StripConnectInfo { inner }
    }
}

#[derive(Clone)]
struct StripConnectInfo<S> {
    inner: S,
}

impl<S, ReqBody, ResBody> Service<http::Request<ReqBody>> for StripConnectInfo<S>
where
    S: Service<http::Request<ReqBody>, Response = http::Response<ResBody>> + Clone + Send + 'static,
    S::Future: Send + 'static,
    S::Error: Into<Box<dyn std::error::Error + Send + Sync>> + Send,
    ReqBody: Send + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = S::Future;

    fn poll_ready(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: http::Request<ReqBody>) -> Self::Future {
        req.extensions_mut().remove::<TcpConnectInfo>();
        req.extensions_mut()
            .remove::<tonic::transport::server::TlsConnectInfo<TcpConnectInfo>>();
        req.extensions_mut().remove::<AxumConnectInfo<SocketAddr>>();
        self.inner.call(req)
    }
}

async fn start_server(strip_connect_info: bool) -> (RunningServer, TestEnv) {
    start_server_with_env(strip_connect_info, TestEnv::new().await).await
}

async fn start_assertion_issuer(body: String) -> (SocketAddr, JoinHandle<()>) {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .expect("assertion issuer listener");
    let addr = listener.local_addr().expect("assertion issuer address");
    let app = Router::new().route(
        "/assertion",
        any(move || {
            let body = body.clone();
            async move { (StatusCode::OK, body) }
        }),
    );
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.expect("assertion issuer");
    });
    (addr, task)
}

async fn start_server_with_env(strip_connect_info: bool, env: TestEnv) -> (RunningServer, TestEnv) {
    let event_chain = Arc::new(RwLock::new(EventChain::new(ChainConfig::default())));
    let ovsdb = Arc::new(op_network::rovs_proxy::OvsdbDbusClient::new());
    let engine = Arc::new(MutationEngine::new(event_chain, ovsdb));
    start_server_with_env_and_engine(strip_connect_info, env, engine).await
}

async fn start_server_with_env_and_engine(
    strip_connect_info: bool,
    env: TestEnv,
    engine: Arc<MutationEngine>,
) -> (RunningServer, TestEnv) {
    start_server_with_binding(strip_connect_info, env, engine, None).await
}

async fn start_server_legacy_peer_ip(
    strip_connect_info: bool,
    env: TestEnv,
) -> (RunningServer, TestEnv) {
    let event_chain = Arc::new(RwLock::new(EventChain::new(ChainConfig::default())));
    let ovsdb = Arc::new(op_network::rovs_proxy::OvsdbDbusClient::new());
    let engine = Arc::new(MutationEngine::new(event_chain, ovsdb));
    start_server_with_binding(
        strip_connect_info,
        env,
        engine,
        Some(AssertionSourceBinding::ExactPeerIp),
    )
    .await
}

async fn start_server_with_binding(
    strip_connect_info: bool,
    env: TestEnv,
    engine: Arc<MutationEngine>,
    source_binding: Option<AssertionSourceBinding>,
) -> (RunningServer, TestEnv) {
    install_crypto_provider();
    let ck =
        rcgen::generate_simple_self_signed(vec!["localhost".to_string(), "127.0.0.1".to_string()])
            .expect("tls cert");
    let ca_pem = ck.cert.pem();
    let identity = Identity::from_pem(ck.cert.pem(), ck.key_pair.serialize_pem());

    let server = OperationGrpcServer::new(engine.clone());
    env.bind_engine(engine.clone()).await;
    let mut validator = AssertionValidator::from_env(DecoyTrustStore::load());
    if let Some(binding) = source_binding {
        validator = validator.with_source_binding(binding);
    }
    let validator = Arc::new(validator);
    let routes = build_operation_routes_with_validator(server, validator);

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let incoming = tokio_stream::wrappers::TcpListenerStream::new(listener);
    let tls = ServerTlsConfig::new().identity(identity);

    let task = if strip_connect_info {
        let serve = tonic::transport::Server::builder()
            .tls_config(tls)
            .expect("tls")
            .accept_http1(true)
            .layer(StripConnectInfoLayer)
            .add_routes(routes)
            .serve_with_incoming(incoming);
        tokio::spawn(async move {
            serve.await.unwrap();
        })
    } else {
        let serve = tonic::transport::Server::builder()
            .tls_config(tls)
            .expect("tls")
            .accept_http1(true)
            .add_routes(routes)
            .serve_with_incoming(incoming);
        tokio::spawn(async move {
            serve.await.unwrap();
        })
    };
    tokio::time::sleep(Duration::from_millis(200)).await;

    (
        RunningServer {
            addr,
            ca_pem,
            engine,
            _task: task,
        },
        env,
    )
}

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
        .expect("connect")
}

struct CallOpts<'a> {
    plugin_id: &'a str,
    method: &'a str,
    capability: &'a str,
    args: ProstValue,
    assertion: Option<SignedAssertion>,
    ghostbridge: Option<(&'a str, &'a str)>,
    wireguard_pubkey: Option<&'a str>,
    extra_assertion: Option<SignedAssertion>,
}

enum CallOutcome {
    RpcErr(Status),
    GateDenied,
    Ok(prost_types::Value),
}

impl std::fmt::Debug for CallOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RpcErr(s) => write!(f, "RpcErr({s})"),
            Self::GateDenied => write!(f, "GateDenied"),
            Self::Ok(_) => write!(f, "Ok"),
        }
    }
}

async fn call_method(channel: Channel, opts: CallOpts<'_>) -> CallOutcome {
    let mut client = PluginServiceClient::new(channel);
    let mut req = Request::new(CallMethodRequest {
        plugin_id: opts.plugin_id.to_string(),
        object_path: String::new(),
        interface_name: String::new(),
        method_name: opts.method.to_string(),
        arguments: vec![opts.args],
        actor_id: "spoofed-caller-controlled-actor".to_string(),
        capability_id: opts.capability.to_string(),
    });
    if let Some(signed) = opts.assertion {
        attach_assertion(req.metadata_mut(), &signed.to_wire());
    }
    if let Some(extra) = opts.extra_assertion {
        req.metadata_mut().append_bin(
            ASSERTION_METADATA_KEY,
            MetadataValue::from_bytes(&extra.to_wire()),
        );
    }
    if let Some((fp, trace)) = opts.ghostbridge {
        attach_ghostbridge(req.metadata_mut(), fp, trace);
    }
    if let Some(pubkey) = opts.wireguard_pubkey {
        req.metadata_mut().insert(
            "x-wireguard-pubkey",
            MetadataValue::try_from(pubkey).expect("wg pubkey"),
        );
    }
    attach_capability(req.metadata_mut(), opts.capability);

    match client.call_method(req).await {
        Err(status) => CallOutcome::RpcErr(status),
        Ok(resp) => {
            let inner = resp.into_inner();
            if inner.success {
                CallOutcome::Ok(inner.result.unwrap_or_default())
            } else if inner
                .error
                .as_ref()
                .is_some_and(|e| e.code == ProtoErrorCode::PermissionDenied as i32)
            {
                CallOutcome::GateDenied
            } else {
                CallOutcome::RpcErr(Status::internal(format!(
                    "unexpected call failure: {:?}",
                    inner.error
                )))
            }
        }
    }
}

fn assert_unauthenticated(out: CallOutcome, reason: &str) {
    match out {
        CallOutcome::RpcErr(s) => {
            assert_eq!(s.code(), Code::Unauthenticated, "{s}");
            assert!(
                s.message().contains(reason),
                "expected {reason}, got {}",
                s.message()
            );
        }
        other => panic!("expected Unauthenticated({reason}), got {other:?}"),
    }
}

fn assert_ghostbridge_identity_rejected(out: CallOutcome) {
    match out {
        CallOutcome::RpcErr(s) => {
            assert!(
                s.code() == Code::Unauthenticated || s.code() == Code::PermissionDenied,
                "{s}"
            );
        }
        other => panic!("expected ghostbridge identity rejection, got {other:?}"),
    }
}

fn assert_permission_denied(out: CallOutcome) {
    match out {
        CallOutcome::GateDenied => {}
        other => panic!("expected PermissionDenied, got {other:?}"),
    }
}

fn assert_ok(out: CallOutcome) -> prost_types::Value {
    match out {
        CallOutcome::Ok(v) => v,
        other => panic!("expected Ok, got {other:?}"),
    }
}

async fn register_human(
    channel: Channel,
    env: &TestEnv,
    pubkey: &str,
    alias: &str,
    nonce: [u8; 16],
) {
    env.provision_identity(pubkey).await;
    env.grant_human(pubkey, &["human_principal.write", "human_principal.read"]);
    let args = prost_struct(BTreeMap::from([
        ("human_pubkey".to_string(), prost_str(pubkey)),
        ("display_alias".to_string(), prost_str(alias)),
    ]));
    let signed = fresh_signed(&env.issuer, pubkey, nonce);
    let out = call_method(
        channel,
        CallOpts {
            plugin_id: "human_principal",
            method: "register_key",
            capability: "human_principal.write",
            args,
            assertion: Some(signed),
            ghostbridge: None,
            wireguard_pubkey: None,
            extra_assertion: None,
        },
    )
    .await;
    assert_ok(out);
}

async fn resolve_human(channel: Channel, env: &TestEnv, pubkey: &str, nonce: [u8; 16]) {
    env.grant_human(pubkey, &["human_principal.read"]);
    let args = prost_struct(BTreeMap::from([(
        "human_pubkey".to_string(),
        prost_str(pubkey),
    )]));
    let signed = fresh_signed(&env.issuer, pubkey, nonce);
    let out = call_method(
        channel,
        CallOpts {
            plugin_id: "human_principal",
            method: "resolve_key",
            capability: "human_principal.read",
            args,
            assertion: Some(signed),
            ghostbridge: None,
            wireguard_pubkey: None,
            extra_assertion: None,
        },
    )
    .await;
    assert_ok(out);
}

async fn provision_container_identity(
    channel: Channel,
    env: &TestEnv,
    pubkey: &str,
    nonce: [u8; 16],
) -> (String, String) {
    env.provision_identity(pubkey).await;
    let operator = pk(240u8.wrapping_add(nonce[0]));
    register_human(channel.clone(), env, &operator, "identity-operator", nonce).await;
    env.grant_human(&operator, &["identity_sled.write"]);
    let mut write_nonce = nonce;
    write_nonce[15] ^= 0x80;
    let args = prost_struct(BTreeMap::from([(
        "wireguard_pubkey".to_string(),
        prost_str(pubkey),
    )]));
    let result = assert_ok(
        call_method(
            channel,
            CallOpts {
                plugin_id: "identity_sled",
                method: "write_identity",
                capability: "identity_sled.write",
                args,
                assertion: Some(fresh_signed(&env.issuer, &operator, write_nonce)),
                ghostbridge: None,
                wireguard_pubkey: None,
                extra_assertion: None,
            },
        )
        .await,
    );
    let identity = struct_field(envelope_result(&result), "identity");
    (
        string_field(identity, "genesis"),
        string_field(identity, "trace_id"),
    )
}

// ?? VAL-E2E-001 ?????????????????????????????????????????????????????????????

#[tokio::test(flavor = "multi_thread")]
async fn happy_path_registered_human_gated_call_over_tls() {
    let (srv, env) = start_server(false).await;
    let ch = tls_channel(srv.addr, &srv.ca_pem).await;
    let pubkey = pk(1);
    register_human(ch.clone(), &env, &pubkey, "alice", [0x01; 16]).await;
    env.grant_human(&pubkey, &["human_principal.read"]);
    let args = prost_struct(BTreeMap::from([(
        "human_pubkey".to_string(),
        prost_str(&pubkey),
    )]));
    let signed = fresh_signed(&env.issuer, &pubkey, [0x02; 16]);
    let out = call_method(
        ch.clone(),
        CallOpts {
            plugin_id: "human_principal",
            method: "resolve_key",
            capability: "human_principal.read",
            args,
            assertion: Some(signed),
            ghostbridge: None,
            wireguard_pubkey: None,
            extra_assertion: None,
        },
    )
    .await;
    assert_ok(out);
    let legacy_genesis = "aa".repeat(32);
    let no_meta = call_method(
        ch,
        CallOpts {
            plugin_id: "human_principal",
            method: "resolve_key",
            capability: "human_principal.read",
            args: prost_struct(BTreeMap::from([(
                "human_pubkey".to_string(),
                prost_str(&pubkey),
            )])),
            assertion: None,
            ghostbridge: Some((&legacy_genesis, "legacy-trace")),
            wireguard_pubkey: None,
            extra_assertion: None,
        },
    )
    .await;
    assert_unauthenticated(no_meta, "Missing Oracle identity assertion");
}

#[tokio::test(flavor = "multi_thread")]
async fn generated_plugin_route_authenticates_registered_human() {
    let (srv, env) = start_server(false).await;
    let ch = tls_channel(srv.addr, &srv.ca_pem).await;
    let pubkey = pk(65);
    register_human(ch.clone(), &env, &pubkey, "typed-route-user", [0x65; 16]).await;
    env.grant_human(&pubkey, &["human_principal.read"]);

    let mut client = HumanPrincipalPluginMethodsClient::new(ch);
    let mut request = Request::new(HumanPrincipalResolveKeyRequest {
        human_pubkey: Some(pubkey.clone()),
    });
    attach_capability(request.metadata_mut(), "human_principal.read");
    attach_assertion(
        request.metadata_mut(),
        &fresh_signed(&env.issuer, &pubkey, [0x66; 16]).to_wire(),
    );

    let response = client
        .resolve_key(request)
        .await
        .expect("generated route should authenticate the assertion")
        .into_inner();
    assert!(response.principal.is_some());

    let mut legacy_only = Request::new(HumanPrincipalResolveKeyRequest {
        human_pubkey: Some(pubkey),
    });
    attach_capability(legacy_only.metadata_mut(), "human_principal.read");
    attach_ghostbridge(
        legacy_only.metadata_mut(),
        &"bb".repeat(32),
        "legacy-generated-trace",
    );
    let status = client.resolve_key(legacy_only).await.unwrap_err();
    assert_eq!(status.code(), Code::Unauthenticated);
    assert_eq!(status.message(), "Missing Oracle identity assertion");
}

#[tokio::test(flavor = "multi_thread")]
async fn generated_plugin_route_preserves_integer_arguments() {
    let (srv, env) = start_server(false).await;
    let ch = tls_channel(srv.addr, &srv.ca_pem).await;
    let pubkey = pk(67);
    register_human(ch.clone(), &env, &pubkey, "typed-number-user", [0x67; 16]).await;
    env.grant_human(&pubkey, &["snowball.read"]);

    let mut client = SnowballPluginMethodsClient::new(ch);
    let mut request = Request::new(SnowballQueryEventsRequest {
        limit: Some(prost_types::Value {
            kind: Some(prost_types::value::Kind::NumberValue(1.0)),
        }),
        ..Default::default()
    });
    attach_capability(request.metadata_mut(), "snowball.read");
    attach_assertion(
        request.metadata_mut(),
        &fresh_signed(&env.issuer, &pubkey, [0x68; 16]).to_wire(),
    );

    let response = client
        .query_events(request)
        .await
        .expect("generated route should preserve numeric JSON values")
        .into_inner();
    assert_eq!(response.events.len(), 1);
    assert_eq!(response.has_more, Some(true));
    // Genesis anchoring records a session-arrival event before the
    // registration mutation, in addition to the chain's initial event.
    assert_eq!(response.total_in_chain, Some(3));
}

#[tokio::test(flavor = "multi_thread")]
async fn oracle_assertion_cannot_restore_retired_ovsdb_transact() {
    let (srv, env) = start_server(false).await;
    let ch = tls_channel(srv.addr, &srv.ca_pem).await;
    let pubkey = pk(61);
    register_human(ch.clone(), &env, &pubkey, "ovsdb-attacker", [0x61; 16]).await;

    let mut client = OvsdbMirrorClient::new(ch);
    let mut request = Request::new(OvsdbTransactRequest {
        database: "Open_vSwitch".to_string(),
        operations_json: "[]".to_string(),
        actor_id: "ungranted-human".to_string(),
    });
    attach_assertion(
        request.metadata_mut(),
        &fresh_signed(&env.issuer, &pubkey, [0x62; 16]).to_wire(),
    );

    let status = client.transact(request).await.unwrap_err();
    assert_eq!(status.code(), Code::Unimplemented);
    assert!(
        status
            .message()
            .contains("legacy domain RPC has no sealed method admission; use PluginService")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn oracle_assertion_cannot_write_unmapped_property() {
    let (srv, env) = start_server(false).await;
    let ch = tls_channel(srv.addr, &srv.ca_pem).await;
    let pubkey = pk(63);
    register_human(ch.clone(), &env, &pubkey, "property-attacker", [0x63; 16]).await;

    let mut client = PluginServiceClient::new(ch);
    let mut request = Request::new(SetPropertyRequest {
        plugin_id: "human_principal".to_string(),
        object_path: "/org/opdbus/v1/plugins/human_principal".to_string(),
        interface_name: "org.opdbus.v1.PluginV1".to_string(),
        property_name: "principals".to_string(),
        value: Some(prost_str("corrupt")),
        actor_id: "ungranted-human".to_string(),
        capability_id: String::new(),
    });
    attach_assertion(
        request.metadata_mut(),
        &fresh_signed(&env.issuer, &pubkey, [0x64; 16]).to_wire(),
    );

    let status = client.set_property(request).await.unwrap_err();
    assert_eq!(status.code(), Code::FailedPrecondition);
    assert!(
        status
            .message()
            .contains("property is read-only through generic Set")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn writable_property_set_without_target_capability_is_denied_before_effects() {
    let env = TestEnv::new().await;
    let schema = op_plugins::state_plugins::chat_router::chat_router_plugin_schema();
    let plugin_id = schema.name.clone();
    op_blob::catalog::BlobStore::open(env._root.path().join("plugin-blobs"))
        .expect("fixture blob store")
        .write(&op_blob::blobify_plugin_schema(&plugin_id, schema))
        .expect("seal writable fixture plugin");
    let (srv, env) = start_server_with_env(false, env).await;
    let channel = tls_channel(srv.addr, &srv.ca_pem).await;
    let pubkey = pk(65);
    register_human(
        channel.clone(),
        &env,
        &pubkey,
        "property-no-capability",
        [0x65; 16],
    )
    .await;
    let before = srv.engine.event_chain.read().await.events().len();

    let mut request = Request::new(SetPropertyRequest {
        plugin_id: "chat_router".to_string(),
        object_path: "/org/opdbus/v1/plugins/chat_router".to_string(),
        interface_name: "org.opdbus.v1.PluginV1".to_string(),
        property_name: "selected_provider".to_string(),
        value: Some(prost_str("salad")),
        actor_id: "spoofed-property-actor".to_string(),
        capability_id: String::new(),
    });
    attach_assertion(
        request.metadata_mut(),
        &fresh_signed(&env.issuer, &pubkey, [0x66; 16]).to_wire(),
    );

    let status = PluginServiceClient::new(channel)
        .set_property(request)
        .await
        .unwrap_err();
    assert_eq!(status.code(), Code::PermissionDenied);
    assert_eq!(
        status.message(),
        "property write requires its declared method capability"
    );
    assert_eq!(srv.engine.event_chain.read().await.events().len(), before);
}

// ?? VAL-E2E-002 ?????????????????????????????????????????????????????????????

#[tokio::test(flavor = "multi_thread")]
async fn rejects_unknown_human_key() {
    let (srv, env) = start_server(false).await;
    let ch = tls_channel(srv.addr, &srv.ca_pem).await;
    let pubkey = pk(2);
    env.grant_human(&pubkey, &["human_principal.read"]);
    let signed = fresh_signed(&env.issuer, &pubkey, [0x03; 16]);
    let out = call_method(
        ch,
        CallOpts {
            plugin_id: "human_principal",
            method: "resolve_key",
            capability: "human_principal.read",
            args: prost_struct(BTreeMap::from([(
                "human_pubkey".to_string(),
                prost_str(&pubkey),
            )])),
            assertion: Some(signed),
            ghostbridge: None,
            wireguard_pubkey: None,
            extra_assertion: None,
        },
    )
    .await;
    assert_unauthenticated(out, "UnknownPrincipal");
}

// ?? VAL-E2E-003 ?????????????????????????????????????????????????????????????

#[tokio::test(flavor = "multi_thread")]
async fn rejects_revoked_human_key() {
    let (srv, env) = start_server(false).await;
    let ch = tls_channel(srv.addr, &srv.ca_pem).await;
    let pubkey = pk(3);
    register_human(ch.clone(), &env, &pubkey, "revoke-me", [0x04; 16]).await;
    resolve_human(ch.clone(), &env, &pubkey, [0x05; 16]).await;
    env.grant_human(&pubkey, &["human_principal.write", "human_principal.read"]);
    let revoke_args = prost_struct(BTreeMap::from([(
        "human_pubkey".to_string(),
        prost_str(&pubkey),
    )]));
    assert_ok(
        call_method(
            ch.clone(),
            CallOpts {
                plugin_id: "human_principal",
                method: "revoke_key",
                capability: "human_principal.write",
                args: revoke_args,
                assertion: Some(fresh_signed(&env.issuer, &pubkey, [0x06; 16])),
                ghostbridge: None,
                wireguard_pubkey: None,
                extra_assertion: None,
            },
        )
        .await,
    );
    let out = call_method(
        ch.clone(),
        CallOpts {
            plugin_id: "human_principal",
            method: "resolve_key",
            capability: "human_principal.read",
            args: prost_struct(BTreeMap::from([(
                "human_pubkey".to_string(),
                prost_str(&pubkey),
            )])),
            assertion: Some(fresh_signed(&env.issuer, &pubkey, [0x07; 16])),
            ghostbridge: None,
            wireguard_pubkey: None,
            extra_assertion: None,
        },
    )
    .await;
    assert_unauthenticated(out, "RevokedPrincipal");
}

// ?? VAL-E2E-004 ?????????????????????????????????????????????????????????????

#[tokio::test(flavor = "multi_thread")]
async fn rejects_expired_assertion() {
    let (srv, env) = start_server(false).await;
    let ch = tls_channel(srv.addr, &srv.ca_pem).await;
    let pubkey = pk(4);
    register_human(ch.clone(), &env, &pubkey, "exp", [0x08; 16]).await;
    env.grant_human(&pubkey, &["human_principal.read"]);
    let now = chrono::Utc::now().timestamp();
    let signed = signed_with_fields(
        &env.issuer,
        &pubkey,
        test_ip(),
        now - 600,
        now - 60,
        [0x09; 16],
        None,
    );
    let out = call_method(
        ch,
        CallOpts {
            plugin_id: "human_principal",
            method: "resolve_key",
            capability: "human_principal.read",
            args: prost_struct(BTreeMap::from([(
                "human_pubkey".to_string(),
                prost_str(&pubkey),
            )])),
            assertion: Some(signed),
            ghostbridge: None,
            wireguard_pubkey: None,
            extra_assertion: None,
        },
    )
    .await;
    assert_unauthenticated(out, "Expired");
}

// ?? VAL-E2E-005 ?????????????????????????????????????????????????????????????

#[tokio::test(flavor = "multi_thread")]
async fn rejects_replayed_nonce() {
    let (srv, env) = start_server(false).await;
    let ch = tls_channel(srv.addr, &srv.ca_pem).await;
    let pubkey = pk(5);
    register_human(ch.clone(), &env, &pubkey, "replay", [0x0A; 16]).await;
    env.grant_human(&pubkey, &["human_principal.read"]);
    let signed = fresh_signed(&env.issuer, &pubkey, [0x0B; 16]);
    let args = prost_struct(BTreeMap::from([(
        "human_pubkey".to_string(),
        prost_str(&pubkey),
    )]));
    assert_ok(
        call_method(
            ch.clone(),
            CallOpts {
                plugin_id: "human_principal",
                method: "resolve_key",
                capability: "human_principal.read",
                args: args.clone(),
                assertion: Some(signed.clone()),
                ghostbridge: None,
                wireguard_pubkey: None,
                extra_assertion: None,
            },
        )
        .await,
    );
    let out = call_method(
        ch,
        CallOpts {
            plugin_id: "human_principal",
            method: "resolve_key",
            capability: "human_principal.read",
            args,
            assertion: Some(signed),
            ghostbridge: None,
            wireguard_pubkey: None,
            extra_assertion: None,
        },
    )
    .await;
    assert_unauthenticated(out, "Replay");
}

// ?? VAL-E2E-006 ?????????????????????????????????????????????????????????????

#[tokio::test(flavor = "multi_thread")]
async fn rejects_source_ip_substitution() {
    let (srv, env) = start_server_legacy_peer_ip(false, TestEnv::new().await).await;
    let ch = tls_channel(srv.addr, &srv.ca_pem).await;
    let pubkey = pk(6);
    register_human(ch.clone(), &env, &pubkey, "ip", [0x0C; 16]).await;
    env.grant_human(&pubkey, &["human_principal.read"]);
    let now = chrono::Utc::now().timestamp();
    let bad = signed_with_fields(
        &env.issuer,
        &pubkey,
        alt_ip(),
        now - 5,
        now + 300,
        [0x0D; 16],
        None,
    );
    let out = call_method(
        ch.clone(),
        CallOpts {
            plugin_id: "human_principal",
            method: "resolve_key",
            capability: "human_principal.read",
            args: prost_struct(BTreeMap::from([(
                "human_pubkey".to_string(),
                prost_str(&pubkey),
            )])),
            assertion: Some(bad),
            ghostbridge: None,
            wireguard_pubkey: None,
            extra_assertion: None,
        },
    )
    .await;
    assert_unauthenticated(out, "SourceIpMismatch");
    resolve_human(ch, &env, &pubkey, [0x0E; 16]).await;
}

// ?? VAL-E2E-007 ?????????????????????????????????????????????????????????????

#[tokio::test(flavor = "multi_thread")]
async fn rejects_alias_substitution() {
    let (srv, env) = start_server(false).await;
    let ch = tls_channel(srv.addr, &srv.ca_pem).await;
    let pubkey = pk(7);
    register_human(ch.clone(), &env, &pubkey, "alice", [0x0F; 16]).await;
    env.grant_human(&pubkey, &["human_principal.read"]);
    env.grant_human("alice", &["human_principal.read"]);
    let signed = fresh_signed(&env.issuer, "alice", [0x10; 16]);
    let out = call_method(
        ch,
        CallOpts {
            plugin_id: "human_principal",
            method: "resolve_key",
            capability: "human_principal.read",
            args: prost_struct(BTreeMap::from([(
                "human_pubkey".to_string(),
                prost_str("alice"),
            )])),
            assertion: Some(signed),
            ghostbridge: None,
            wireguard_pubkey: None,
            extra_assertion: None,
        },
    )
    .await;
    assert_unauthenticated(out, "UnknownPrincipal");
}

// ?? VAL-E2E-008 ?????????????????????????????????????????????????????????????

#[tokio::test(flavor = "multi_thread")]
async fn rejects_container_key_substitution() {
    let (srv, env) = start_server(false).await;
    let ch = tls_channel(srv.addr, &srv.ca_pem).await;
    let container_pk = pk(8);
    let (_fp, _trace) =
        provision_container_identity(ch.clone(), &env, &container_pk, [0x11; 16]).await;
    env.grant_human(&container_pk, &["human_principal.read"]);
    let signed = fresh_signed(&env.issuer, &container_pk, [0x12; 16]);
    let out = call_method(
        ch,
        CallOpts {
            plugin_id: "human_principal",
            method: "resolve_key",
            capability: "human_principal.read",
            args: prost_struct(BTreeMap::from([(
                "human_pubkey".to_string(),
                prost_str(&container_pk),
            )])),
            assertion: Some(signed),
            ghostbridge: None,
            wireguard_pubkey: None,
            extra_assertion: None,
        },
    )
    .await;
    assert_unauthenticated(out, "UnknownPrincipal");
}

// ?? VAL-E2E-009 ?????????????????????????????????????????????????????????????

#[tokio::test(flavor = "multi_thread")]
async fn rejects_over_long_ttl() {
    let (srv, env) = start_server(false).await;
    let ch = tls_channel(srv.addr, &srv.ca_pem).await;
    let pubkey = pk(9);
    register_human(ch.clone(), &env, &pubkey, "long", [0x13; 16]).await;
    env.grant_human(&pubkey, &["human_principal.read"]);
    let now = chrono::Utc::now().timestamp();
    let signed = signed_with_fields(
        &env.issuer,
        &pubkey,
        test_ip(),
        now,
        now + 901,
        [0x14; 16],
        None,
    );
    verify_signature(
        &signed.assertion,
        &signed.signature,
        env.issuer.verifying_key(),
    )
    .expect("sig ok");
    let out = call_method(
        ch,
        CallOpts {
            plugin_id: "human_principal",
            method: "resolve_key",
            capability: "human_principal.read",
            args: prost_struct(BTreeMap::from([(
                "human_pubkey".to_string(),
                prost_str(&pubkey),
            )])),
            assertion: Some(signed),
            ghostbridge: None,
            wireguard_pubkey: None,
            extra_assertion: None,
        },
    )
    .await;
    assert_unauthenticated(out, "LifetimeTooLong");
}

// ?? VAL-E2E-010 ?????????????????????????????????????????????????????????????

#[tokio::test(flavor = "multi_thread")]
async fn rejects_unknown_decoy_key() {
    let (srv, env) = start_server(false).await;
    let ch = tls_channel(srv.addr, &srv.ca_pem).await;
    let pubkey = pk(10);
    register_human(ch.clone(), &env, &pubkey, "decoy", [0x15; 16]).await;
    env.grant_human(&pubkey, &["human_principal.read"]);
    let other = DecoyIssuer::new(
        SigningKey::from_bytes(&[9u8; 32]),
        "decoy-key-2",
        Duration::from_secs(900),
    );
    let signed = signed_with_fields(
        &other,
        &pubkey,
        test_ip(),
        chrono::Utc::now().timestamp() - 5,
        chrono::Utc::now().timestamp() + 300,
        [0x16; 16],
        Some("decoy-key-2"),
    );
    let out = call_method(
        ch,
        CallOpts {
            plugin_id: "human_principal",
            method: "resolve_key",
            capability: "human_principal.read",
            args: prost_struct(BTreeMap::from([(
                "human_pubkey".to_string(),
                prost_str(&pubkey),
            )])),
            assertion: Some(signed),
            ghostbridge: None,
            wireguard_pubkey: None,
            extra_assertion: None,
        },
    )
    .await;
    assert_unauthenticated(out, "UnknownDecoyKey");
}

// ?? VAL-E2E-011 ?????????????????????????????????????????????????????????????

#[tokio::test(flavor = "multi_thread")]
async fn rejects_bad_signature() {
    let (srv, env) = start_server(false).await;
    let ch = tls_channel(srv.addr, &srv.ca_pem).await;
    let pubkey = pk(11);
    register_human(ch.clone(), &env, &pubkey, "sig", [0x17; 16]).await;
    env.grant_human(&pubkey, &["human_principal.read"]);
    let args = prost_struct(BTreeMap::from([(
        "human_pubkey".to_string(),
        prost_str(&pubkey),
    )]));
    let mut wire_tamper = fresh_signed(&env.issuer, &pubkey, [0x18; 16]).to_wire();
    wire_tamper[10] ^= 0x01;
    let mut client = PluginServiceClient::new(ch.clone());
    let mut req = Request::new(CallMethodRequest {
        plugin_id: "human_principal".to_string(),
        object_path: String::new(),
        interface_name: String::new(),
        method_name: "resolve_key".to_string(),
        arguments: vec![args.clone()],
        actor_id: "e2e".to_string(),
        capability_id: "human_principal.read".to_string(),
    });
    req.metadata_mut().insert_bin(
        ASSERTION_METADATA_KEY,
        MetadataValue::from_bytes(&wire_tamper),
    );
    attach_capability(req.metadata_mut(), "human_principal.read");
    let out0 = client.call_method(req).await.unwrap_err();
    assert_eq!(out0.code(), Code::Unauthenticated);
    assert!(out0.message().contains("BadSignature"));
    let mut tamper_sig = fresh_signed(&env.issuer, &pubkey, [0x19; 16]);
    tamper_sig.signature[0] ^= 0x01;
    let out1 = call_method(
        ch.clone(),
        CallOpts {
            plugin_id: "human_principal",
            method: "resolve_key",
            capability: "human_principal.read",
            args: args.clone(),
            assertion: Some(tamper_sig.clone()),
            ghostbridge: None,
            wireguard_pubkey: None,
            extra_assertion: None,
        },
    )
    .await;
    assert_unauthenticated(out1, "BadSignature");
}

// ?? VAL-E2E-012 ?????????????????????????????????????????????????????????????

#[tokio::test(flavor = "multi_thread")]
async fn denies_valid_assertion_without_capability_grant() {
    let (srv, env) = start_server(false).await;
    let ch = tls_channel(srv.addr, &srv.ca_pem).await;
    let pubkey = pk(12);
    register_human(ch.clone(), &env, &pubkey, "nogrant", [0x1A; 16]).await;
    env.set_grants(&empty_grants());
    let out = call_method(
        ch,
        CallOpts {
            plugin_id: "human_principal",
            method: "resolve_key",
            capability: "human_principal.read",
            args: prost_struct(BTreeMap::from([(
                "human_pubkey".to_string(),
                prost_str(&pubkey),
            )])),
            assertion: Some(fresh_signed(&env.issuer, &pubkey, [0x1B; 16])),
            ghostbridge: None,
            wireguard_pubkey: None,
            extra_assertion: None,
        },
    )
    .await;
    assert_permission_denied(out);
}

// ?? VAL-E2E-015 ?????????????????????????????????????????????????????????????

#[tokio::test(flavor = "multi_thread")]
async fn rejects_malformed_assertion_metadata() {
    let (srv, env) = start_server(false).await;
    let ch = tls_channel(srv.addr, &srv.ca_pem).await;
    let pubkey = pk(15);
    register_human(ch.clone(), &env, &pubkey, "mal", [0x1C; 16]).await;
    env.grant_human(&pubkey, &["human_principal.read"]);
    let args = prost_struct(BTreeMap::from([(
        "human_pubkey".to_string(),
        prost_str(&pubkey),
    )]));
    for (nonce_byte, wire) in [
        (0x1Du8, b"not-oia1".to_vec()),
        (0x1Eu8, {
            let mut w = fresh_signed(&env.issuer, &pubkey, [0x1E; 16]).to_wire();
            w.push(0xFF);
            w
        }),
        (0x1Fu8, b"OIA1".to_vec()),
    ] {
        let mut client = PluginServiceClient::new(ch.clone());
        let mut req = Request::new(CallMethodRequest {
            plugin_id: "human_principal".to_string(),
            object_path: String::new(),
            interface_name: String::new(),
            method_name: "resolve_key".to_string(),
            arguments: vec![args.clone()],
            actor_id: "e2e".to_string(),
            capability_id: "human_principal.read".to_string(),
        });
        req.metadata_mut()
            .insert_bin(ASSERTION_METADATA_KEY, MetadataValue::from_bytes(&wire));
        attach_capability(req.metadata_mut(), "human_principal.read");
        let status = client.call_method(req).await.unwrap_err();
        assert_eq!(status.code(), Code::Unauthenticated, "nonce {nonce_byte}");
        assert!(
            status.message().contains("Malformed"),
            "{}",
            status.message()
        );
    }
}

// ?? VAL-E2E-016 ?????????????????????????????????????????????????????????????

#[tokio::test(flavor = "multi_thread")]
async fn rejects_not_yet_valid_assertion() {
    let (srv, env) = start_server(false).await;
    let ch = tls_channel(srv.addr, &srv.ca_pem).await;
    let pubkey = pk(16);
    register_human(ch.clone(), &env, &pubkey, "future", [0x20; 16]).await;
    env.grant_human(&pubkey, &["human_principal.read"]);
    let now = chrono::Utc::now().timestamp();
    let signed = signed_with_fields(
        &env.issuer,
        &pubkey,
        test_ip(),
        now + 300,
        now + 900,
        [0x21; 16],
        None,
    );
    let out = call_method(
        ch,
        CallOpts {
            plugin_id: "human_principal",
            method: "resolve_key",
            capability: "human_principal.read",
            args: prost_struct(BTreeMap::from([(
                "human_pubkey".to_string(),
                prost_str(&pubkey),
            )])),
            assertion: Some(signed),
            ghostbridge: None,
            wireguard_pubkey: None,
            extra_assertion: None,
        },
    )
    .await;
    assert_unauthenticated(out, "NotYetValid");
}

// ?? VAL-E2E-019 ?????????????????????????????????????????????????????????????

#[tokio::test(flavor = "multi_thread")]
async fn accepts_within_leeway_expired_assertion() {
    let (srv, env) = start_server(false).await;
    let ch = tls_channel(srv.addr, &srv.ca_pem).await;
    let pubkey = pk(19);
    register_human(ch.clone(), &env, &pubkey, "leeway", [0x22; 16]).await;
    env.grant_human(&pubkey, &["human_principal.read"]);
    let now = chrono::Utc::now().timestamp();
    let signed = signed_with_fields(
        &env.issuer,
        &pubkey,
        test_ip(),
        now - 300,
        now - 10,
        [0x23; 16],
        None,
    );
    assert_ok(
        call_method(
            ch,
            CallOpts {
                plugin_id: "human_principal",
                method: "resolve_key",
                capability: "human_principal.read",
                args: prost_struct(BTreeMap::from([(
                    "human_pubkey".to_string(),
                    prost_str(&pubkey),
                )])),
                assertion: Some(signed),
                ghostbridge: None,
                wireguard_pubkey: None,
                extra_assertion: None,
            },
        )
        .await,
    );
}

// ?? VAL-E2E-020 ?????????????????????????????????????????????????????????????

#[tokio::test(flavor = "multi_thread")]
async fn accepts_lifetime_exactly_900s() {
    let (srv, env) = start_server(false).await;
    let ch = tls_channel(srv.addr, &srv.ca_pem).await;
    let pubkey = pk(20);
    register_human(ch.clone(), &env, &pubkey, "900", [0x24; 16]).await;
    env.grant_human(&pubkey, &["human_principal.read"]);
    let now = chrono::Utc::now().timestamp();
    let signed = signed_with_fields(
        &env.issuer,
        &pubkey,
        test_ip(),
        now,
        now + 900,
        [0x25; 16],
        None,
    );
    assert_ok(
        call_method(
            ch,
            CallOpts {
                plugin_id: "human_principal",
                method: "resolve_key",
                capability: "human_principal.read",
                args: prost_struct(BTreeMap::from([(
                    "human_pubkey".to_string(),
                    prost_str(&pubkey),
                )])),
                assertion: Some(signed),
                ghostbridge: None,
                wireguard_pubkey: None,
                extra_assertion: None,
            },
        )
        .await,
    );
}

// ?? VAL-E2E-021 ?????????????????????????????????????????????????????????????

#[tokio::test(flavor = "multi_thread")]
async fn registration_bootstrap_requires_grant() {
    let (srv, env) = start_server(false).await;
    let ch = tls_channel(srv.addr, &srv.ca_pem).await;
    let pubkey = pk(21);
    env.provision_identity(&pubkey).await;
    env.grant_human(&pubkey, &["human_principal.write"]);
    let args = prost_struct(BTreeMap::from([
        ("human_pubkey".to_string(), prost_str(&pubkey)),
        ("display_alias".to_string(), prost_str("boot")),
    ]));
    assert_ok(
        call_method(
            ch.clone(),
            CallOpts {
                plugin_id: "human_principal",
                method: "register_key",
                capability: "human_principal.write",
                args: args.clone(),
                assertion: Some(fresh_signed(&env.issuer, &pubkey, [0x26; 16])),
                ghostbridge: None,
                wireguard_pubkey: None,
                extra_assertion: None,
            },
        )
        .await,
    );
    let pubkey2 = pk(22);
    env.provision_identity(&pubkey2).await;
    env.set_grants(&empty_grants());
    assert_permission_denied(
        call_method(
            ch,
            CallOpts {
                plugin_id: "human_principal",
                method: "register_key",
                capability: "human_principal.write",
                args: prost_struct(BTreeMap::from([
                    ("human_pubkey".to_string(), prost_str(&pubkey2)),
                    ("display_alias".to_string(), prost_str("denied")),
                ])),
                assertion: Some(fresh_signed(&env.issuer, &pubkey2, [0x27; 16])),
                ghostbridge: None,
                wireguard_pubkey: None,
                extra_assertion: None,
            },
        )
        .await,
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn registration_bootstrap_cannot_register_another_humans_key() {
    let (srv, env) = start_server(false).await;
    let ch = tls_channel(srv.addr, &srv.ca_pem).await;
    let attacker = pk(61);
    let victim = pk(62);
    env.provision_identity(&attacker).await;
    env.grant_human(&attacker, &["human_principal.write"]);

    assert_permission_denied(
        call_method(
            ch,
            CallOpts {
                plugin_id: "human_principal",
                method: "register_key",
                capability: "human_principal.write",
                args: prost_struct(BTreeMap::from([
                    ("human_pubkey".to_string(), prost_str(&victim)),
                    ("display_alias".to_string(), prost_str("stolen")),
                ])),
                assertion: Some(fresh_signed(&env.issuer, &attacker, [0x61; 16])),
                ghostbridge: None,
                wireguard_pubkey: None,
                extra_assertion: None,
            },
        )
        .await,
    );
}

// ?? VAL-E2E-022 ?????????????????????????????????????????????????????????????

#[tokio::test(flavor = "multi_thread")]
async fn fixture_connect_info_matches_validator() {
    let (srv, env) = start_server(false).await;
    let ch = tls_channel(srv.addr, &srv.ca_pem).await;
    let pubkey = pk(22);
    register_human(ch.clone(), &env, &pubkey, "conn", [0x28; 16]).await;
    env.grant_human(&pubkey, &["human_principal.read"]);
    let signed = fresh_signed(&env.issuer, &pubkey, [0x29; 16]);
    let mut client = PluginServiceClient::new(ch);
    let mut req = Request::new(CallMethodRequest {
        plugin_id: "human_principal".to_string(),
        object_path: String::new(),
        interface_name: String::new(),
        method_name: "resolve_key".to_string(),
        arguments: vec![prost_struct(BTreeMap::from([(
            "human_pubkey".to_string(),
            prost_str(&pubkey),
        )]))],
        actor_id: "spoofed-caller-controlled-actor".to_string(),
        capability_id: "human_principal.read".to_string(),
    });
    attach_assertion(req.metadata_mut(), &signed.to_wire());
    attach_capability(req.metadata_mut(), "human_principal.read");
    let _ = client.call_method(req).await.expect("composed path ok");
    let chain = srv.engine.event_chain.read().await;
    let event = chain.events().last().expect("method event");
    assert_eq!(event.actor_id, derive_principal_id(&pubkey));
    assert_ne!(event.actor_id, "spoofed-caller-controlled-actor");
    drop(chain);
    let mut probe = Request::new(());
    probe.extensions_mut().insert(TcpConnectInfo {
        local_addr: Some(SocketAddr::new(test_ip(), 0)),
        remote_addr: Some(SocketAddr::new(test_ip(), 12345)),
    });
    assert_eq!(
        connect_info_peer_addr(&probe),
        Some(SocketAddr::new(test_ip(), 12345))
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn legacy_mutate_preserves_verified_identity_and_ignores_spoofed_actor() {
    let (srv, env) = start_server(false).await;
    let channel = tls_channel(srv.addr, &srv.ca_pem).await;
    let pubkey = pk(70);
    register_human(channel.clone(), &env, &pubkey, "mutate-route", [0x70; 16]).await;
    env.grant_human(&pubkey, &["human_principal.read"]);

    let mut request = Request::new(resolve_key_mutation(&pubkey, "spoofed-mutate-actor"));
    attach_assertion(
        request.metadata_mut(),
        &fresh_signed(&env.issuer, &pubkey, [0x71; 16]).to_wire(),
    );
    attach_capability(request.metadata_mut(), "human_principal.read");
    let response = StateSyncClient::new(channel)
        .mutate(request)
        .await
        .expect("canonical mutate route")
        .into_inner();
    assert!(response.success, "mutate result: {:?}", response.error);

    let chain = srv.engine.event_chain.read().await;
    let event = chain.events().last().expect("mutate audit event");
    let expected_actor = derive_principal_id(&pubkey);
    let expected_session = derive_session_id(&pubkey);
    assert_eq!(event.actor_id, expected_actor);
    assert_ne!(event.actor_id, "spoofed-mutate-actor");
    assert_eq!(event.capability_id.as_deref(), Some("human_principal.read"));
    assert_eq!(event.method_name.as_deref(), Some("resolve_key"));
    assert_eq!(event.session_id.as_deref(), Some(expected_session.as_str()));
    assert!(
        event
            .session_genesis
            .as_ref()
            .is_some_and(|value| !value.is_empty())
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn legacy_batch_preserves_verified_identity_for_every_nested_request() {
    let (srv, env) = start_server(false).await;
    let channel = tls_channel(srv.addr, &srv.ca_pem).await;
    let pubkey = pk(72);
    register_human(channel.clone(), &env, &pubkey, "batch-route", [0x72; 16]).await;
    env.grant_human(&pubkey, &["human_principal.read"]);
    let before = srv.engine.event_chain.read().await.events().len();

    let mut request = Request::new(BatchMutateRequest {
        mutations: vec![
            resolve_key_mutation(&pubkey, "spoofed-nested-one"),
            resolve_key_mutation(&pubkey, "spoofed-nested-two"),
        ],
        atomic: false,
        actor_id: "spoofed-batch-actor".to_string(),
    });
    attach_assertion(
        request.metadata_mut(),
        &fresh_signed(&env.issuer, &pubkey, [0x73; 16]).to_wire(),
    );
    attach_capability(request.metadata_mut(), "human_principal.read");
    let response = StateSyncClient::new(channel)
        .batch_mutate(request)
        .await
        .expect("canonical batch route")
        .into_inner();
    assert!(response.success);
    assert_eq!(response.results.len(), 2);
    assert!(response.results.iter().all(|result| result.success));

    let chain = srv.engine.event_chain.read().await;
    let events = &chain.events()[before..];
    let expected_actor = derive_principal_id(&pubkey);
    let expected_session = derive_session_id(&pubkey);
    assert_eq!(events.len(), 2);
    assert!(events.iter().all(|event| {
        event.actor_id == expected_actor
            && event.capability_id.as_deref() == Some("human_principal.read")
            && event.method_name.as_deref() == Some("resolve_key")
            && event.session_id.as_deref() == Some(expected_session.as_str())
            && event
                .session_genesis
                .as_ref()
                .is_some_and(|value| !value.is_empty())
    }));
}

#[tokio::test(flavor = "multi_thread")]
async fn raw_patch_and_atomic_batch_reject_before_any_effect() {
    let (srv, env) = start_server(false).await;
    let channel = tls_channel(srv.addr, &srv.ca_pem).await;
    let pubkey = pk(74);
    register_human(channel.clone(), &env, &pubkey, "rejected-route", [0x74; 16]).await;
    env.grant_human(&pubkey, &["human_principal.read"]);
    let before = srv.engine.event_chain.read().await.events().len();

    let mut raw = Request::new(MutateRequest {
        operation: OperationType::ApplyPatch as i32,
        value: Some(prost_struct(BTreeMap::from([(
            "status".to_string(),
            prost_str("forged"),
        )]))),
        ..resolve_key_mutation(&pubkey, "spoofed-raw-actor")
    });
    attach_assertion(
        raw.metadata_mut(),
        &fresh_signed(&env.issuer, &pubkey, [0x75; 16]).to_wire(),
    );
    attach_capability(raw.metadata_mut(), "human_principal.read");
    let status = StateSyncClient::new(channel.clone())
        .mutate(raw)
        .await
        .unwrap_err();
    assert_eq!(status.code(), Code::FailedPrecondition);
    assert_eq!(srv.engine.event_chain.read().await.events().len(), before);

    let mut atomic = Request::new(BatchMutateRequest {
        mutations: vec![resolve_key_mutation(&pubkey, "spoofed-atomic-actor")],
        atomic: true,
        actor_id: "spoofed-batch-actor".to_string(),
    });
    attach_assertion(
        atomic.metadata_mut(),
        &fresh_signed(&env.issuer, &pubkey, [0x76; 16]).to_wire(),
    );
    attach_capability(atomic.metadata_mut(), "human_principal.read");
    let status = StateSyncClient::new(channel)
        .batch_mutate(atomic)
        .await
        .unwrap_err();
    assert_eq!(status.code(), Code::Unimplemented);
    assert_eq!(srv.engine.event_chain.read().await.events().len(), before);
}

// ?? VAL-E2E-024 ?????????????????????????????????????????????????????????????

#[tokio::test(flavor = "multi_thread")]
async fn rejects_duplicate_assertion_metadata() {
    let (srv, env) = start_server(false).await;
    let ch = tls_channel(srv.addr, &srv.ca_pem).await;
    let pubkey = pk(24);
    register_human(ch.clone(), &env, &pubkey, "dup", [0x2A; 16]).await;
    env.grant_human(&pubkey, &["human_principal.read"]);
    let a = fresh_signed(&env.issuer, &pubkey, [0x2B; 16]);
    let b = fresh_signed(&env.issuer, &pubkey, [0x2C; 16]);
    let out = call_method(
        ch,
        CallOpts {
            plugin_id: "human_principal",
            method: "resolve_key",
            capability: "human_principal.read",
            args: prost_struct(BTreeMap::from([(
                "human_pubkey".to_string(),
                prost_str(&pubkey),
            )])),
            assertion: Some(a),
            ghostbridge: None,
            wireguard_pubkey: None,
            extra_assertion: Some(b),
        },
    )
    .await;
    assert_unauthenticated(out, "Malformed");
}

// ?? VAL-CROSS-001 ???????????????????????????????????????????????????????????

#[tokio::test(flavor = "multi_thread")]
async fn e2e_full_trust_chain_generated_surface() {
    let (srv, env) = start_server(false).await;
    let ch = tls_channel(srv.addr, &srv.ca_pem).await;
    let pubkey = pk(31);
    env.provision_identity(&pubkey).await;
    env.grant_human(&pubkey, &["human_principal.write", "human_principal.read"]);
    let reg_args = prost_struct(BTreeMap::from([
        ("human_pubkey".to_string(), prost_str(&pubkey)),
        ("display_alias".to_string(), prost_str("chain")),
    ]));
    let reg = assert_ok(
        call_method(
            ch.clone(),
            CallOpts {
                plugin_id: "human_principal",
                method: "register_key",
                capability: "human_principal.write",
                args: reg_args,
                assertion: Some(fresh_signed(&env.issuer, &pubkey, [0x31; 16])),
                ghostbridge: None,
                wireguard_pubkey: None,
                extra_assertion: None,
            },
        )
        .await,
    );
    let derived = derive_principal_id(&pubkey);
    let pid = principal_id_from(&reg);
    assert_eq!(pid, derived);
    env.grant_human(&pubkey, &["human_principal.write", "human_principal.read"]);
    assert_ok(
        call_method(
            ch.clone(),
            CallOpts {
                plugin_id: "human_principal",
                method: "set_alias",
                capability: "human_principal.write",
                args: prost_struct(BTreeMap::from([
                    ("principal_id".to_string(), prost_str(&derived)),
                    ("display_alias".to_string(), prost_str("renamed")),
                ])),
                assertion: Some(fresh_signed(&env.issuer, &pubkey, [0x32; 16])),
                ghostbridge: None,
                wireguard_pubkey: None,
                extra_assertion: None,
            },
        )
        .await,
    );
    let got = assert_ok(
        call_method(
            ch,
            CallOpts {
                plugin_id: "human_principal",
                method: "get_principal",
                capability: "human_principal.read",
                args: prost_struct(BTreeMap::from([(
                    "principal_id".to_string(),
                    prost_str(&derived),
                )])),
                assertion: Some(fresh_signed(&env.issuer, &pubkey, [0x33; 16])),
                ghostbridge: None,
                wireguard_pubkey: None,
                extra_assertion: None,
            },
        )
        .await,
    );
    let alias = string_field(
        struct_field(envelope_result(&got), "principal"),
        "display_alias",
    );
    assert_eq!(alias, "renamed");
}

// ?? VAL-CROSS-003 ???????????????????????????????????????????????????????????

#[tokio::test(flavor = "multi_thread")]
async fn e2e_assertion_precedence_over_legacy_session_headers() {
    let (srv, env) = start_server(false).await;
    let ch = tls_channel(srv.addr, &srv.ca_pem).await;
    let pubkey = pk(32);
    register_human(ch.clone(), &env, &pubkey, "prec", [0x34; 16]).await;
    env.grant_human(&pubkey, &["human_principal.read"]);
    let signed = fresh_signed(&env.issuer, &pubkey, [0x35; 16]);
    assert_ok(
        call_method(
            ch.clone(),
            CallOpts {
                plugin_id: "human_principal",
                method: "resolve_key",
                capability: "human_principal.read",
                args: prost_struct(BTreeMap::from([(
                    "human_pubkey".to_string(),
                    prost_str(&pubkey),
                )])),
                assertion: Some(signed),
                ghostbridge: Some(("00".repeat(32).as_str(), "bogus-trace")),
                wireguard_pubkey: None,
                extra_assertion: None,
            },
        )
        .await,
    );
    let mut bad = fresh_signed(&env.issuer, &pubkey, [0x36; 16]);
    bad.signature[0] ^= 0x01;
    let container_pk = pk(33);
    let (fp, trace) =
        provision_container_identity(ch.clone(), &env, &container_pk, [0x37; 16]).await;
    env.set_grants(
        &serde_json::json!({ fp.clone(): { "capabilities": ["human_principal.read"] } }),
    );
    let out = call_method(
        ch,
        CallOpts {
            plugin_id: "human_principal",
            method: "resolve_key",
            capability: "human_principal.read",
            args: prost_struct(BTreeMap::from([(
                "human_pubkey".to_string(),
                prost_str(&pubkey),
            )])),
            assertion: Some(bad),
            ghostbridge: Some((&fp, &trace)),
            wireguard_pubkey: None,
            extra_assertion: None,
        },
    )
    .await;
    assert_unauthenticated(out, "BadSignature");
}

// ?? VAL-CROSS-004 ???????????????????????????????????????????????????????????

#[tokio::test(flavor = "multi_thread")]
async fn e2e_container_and_human_never_cross_authenticate() {
    let (srv, env) = start_server(false).await;
    let ch = tls_channel(srv.addr, &srv.ca_pem).await;
    let container_pk = pk(34);
    provision_container_identity(ch.clone(), &env, &container_pk, [0x38; 16]).await;
    env.grant_human(&container_pk, &["human_principal.read"]);
    assert_unauthenticated(
        call_method(
            ch.clone(),
            CallOpts {
                plugin_id: "human_principal",
                method: "resolve_key",
                capability: "human_principal.read",
                args: prost_struct(BTreeMap::from([(
                    "human_pubkey".to_string(),
                    prost_str(&container_pk),
                )])),
                assertion: Some(fresh_signed(&env.issuer, &container_pk, [0x39; 16])),
                ghostbridge: None,
                wireguard_pubkey: None,
                extra_assertion: None,
            },
        )
        .await,
        "UnknownPrincipal",
    );
    let human_pk = pk(35);
    register_human(ch.clone(), &env, &human_pk, "human", [0x3A; 16]).await;
    let hfp = principal_id_for(&human_pk);
    env.set_grants(
        &serde_json::json!({ hfp.clone(): { "capabilities": ["human_principal.read"] } }),
    );
    assert_ghostbridge_identity_rejected(
        call_method(
            ch.clone(),
            CallOpts {
                plugin_id: "human_principal",
                method: "resolve_key",
                capability: "human_principal.read",
                args: prost_struct(BTreeMap::from([(
                    "human_pubkey".to_string(),
                    prost_str(&human_pk),
                )])),
                assertion: None,
                ghostbridge: Some((&hfp, "human-trace")),
                wireguard_pubkey: Some(&human_pk),
                extra_assertion: None,
            },
        )
        .await,
    );
    assert_ne!(derive_principal_id(&human_pk), derive_session_id(&human_pk));
}

// ?? VAL-CROSS-005 ???????????????????????????????????????????????????????????

#[tokio::test(flavor = "multi_thread")]
async fn e2e_human_grants_same_mechanism() {
    let (srv, env) = start_server(false).await;
    let ch = tls_channel(srv.addr, &srv.ca_pem).await;
    let pubkey = pk(36);
    register_human(ch.clone(), &env, &pubkey, "grants", [0x3B; 16]).await;
    let fp = principal_id_for(&pubkey);
    let args = prost_struct(BTreeMap::from([(
        "human_pubkey".to_string(),
        prost_str(&pubkey),
    )]));
    env.set_grants(&serde_json::json!({ fp: { "capabilities": ["human_principal.read"] } }));
    assert_ok(
        call_method(
            ch.clone(),
            CallOpts {
                plugin_id: "human_principal",
                method: "resolve_key",
                capability: "human_principal.read",
                args: args.clone(),
                assertion: Some(fresh_signed(&env.issuer, &pubkey, [0x3C; 16])),
                ghostbridge: None,
                wireguard_pubkey: None,
                extra_assertion: None,
            },
        )
        .await,
    );
    env.set_grants(&empty_grants());
    assert_permission_denied(
        call_method(
            ch.clone(),
            CallOpts {
                plugin_id: "human_principal",
                method: "resolve_key",
                capability: "human_principal.read",
                args: args.clone(),
                assertion: Some(fresh_signed(&env.issuer, &pubkey, [0x3D; 16])),
                ghostbridge: None,
                wireguard_pubkey: None,
                extra_assertion: None,
            },
        )
        .await,
    );
    env.set_grants(&grants_wildcard(&["human_principal.read"]));
    assert_permission_denied(
        call_method(
            ch.clone(),
            CallOpts {
                plugin_id: "human_principal",
                method: "resolve_key",
                capability: "human_principal.read",
                args: args.clone(),
                assertion: Some(fresh_signed(&env.issuer, &pubkey, [0x3E; 16])),
                ghostbridge: None,
                wireguard_pubkey: None,
                extra_assertion: None,
            },
        )
        .await,
    );
    env.set_grants(&empty_grants());
    assert_permission_denied(
        call_method(
            ch,
            CallOpts {
                plugin_id: "human_principal",
                method: "resolve_key",
                capability: "human_principal.read",
                args,
                assertion: Some(fresh_signed(&env.issuer, &pubkey, [0x3F; 16])),
                ghostbridge: None,
                wireguard_pubkey: None,
                extra_assertion: None,
            },
        )
        .await,
    );
}

// ?? VAL-CROSS-006 ???????????????????????????????????????????????????????????

#[tokio::test(flavor = "multi_thread")]
async fn e2e_freshness_lifecycle() {
    let (srv, env) = start_server(false).await;
    let ch = tls_channel(srv.addr, &srv.ca_pem).await;
    let pubkey = pk(37);
    register_human(ch.clone(), &env, &pubkey, "life", [0x40; 16]).await;
    env.grant_human(&pubkey, &["human_principal.read"]);
    let now = chrono::Utc::now().timestamp();
    assert_ok(
        call_method(
            ch.clone(),
            CallOpts {
                plugin_id: "human_principal",
                method: "resolve_key",
                capability: "human_principal.read",
                args: prost_struct(BTreeMap::from([(
                    "human_pubkey".to_string(),
                    prost_str(&pubkey),
                )])),
                assertion: Some(signed_with_fields(
                    &env.issuer,
                    &pubkey,
                    test_ip(),
                    now - 5,
                    now + 60,
                    [0x41; 16],
                    None,
                )),
                ghostbridge: None,
                wireguard_pubkey: None,
                extra_assertion: None,
            },
        )
        .await,
    );
    assert_unauthenticated(
        call_method(
            ch.clone(),
            CallOpts {
                plugin_id: "human_principal",
                method: "resolve_key",
                capability: "human_principal.read",
                args: prost_struct(BTreeMap::from([(
                    "human_pubkey".to_string(),
                    prost_str(&pubkey),
                )])),
                assertion: Some(signed_with_fields(
                    &env.issuer,
                    &pubkey,
                    test_ip(),
                    now - 600,
                    now - 60,
                    [0x42; 16],
                    None,
                )),
                ghostbridge: None,
                wireguard_pubkey: None,
                extra_assertion: None,
            },
        )
        .await,
        "Expired",
    );
    assert_ok(
        call_method(
            ch,
            CallOpts {
                plugin_id: "human_principal",
                method: "resolve_key",
                capability: "human_principal.read",
                args: prost_struct(BTreeMap::from([(
                    "human_pubkey".to_string(),
                    prost_str(&pubkey),
                )])),
                assertion: Some(fresh_signed(&env.issuer, &pubkey, [0x43; 16])),
                ghostbridge: None,
                wireguard_pubkey: None,
                extra_assertion: None,
            },
        )
        .await,
    );
}

// ?? VAL-CROSS-007 ???????????????????????????????????????????????????????????

#[tokio::test(flavor = "multi_thread")]
async fn e2e_empty_trust_store_fail_closed() {
    let env = TestEnv::new().await;
    let cozo = env.cozo_path();
    let grants_template = env._root.path().join("capability-grants.json");
    let (srv, env) = start_server_with_env(false, env).await;
    let ch = tls_channel(srv.addr, &srv.ca_pem).await;
    let pubkey = pk(38);
    register_human(ch.clone(), &env, &pubkey, "empty-trust", [0x44; 16]).await;
    let container_pk = pk(39);
    let (fp, trace) =
        provision_container_identity(ch.clone(), &env, &container_pk, [0x46; 16]).await;
    let engine = srv.engine.clone();
    drop(srv);
    drop(ch);
    // FIXME: Audit that the environment access only happens in single-threaded code.
    unsafe { std::env::set_var("OP_HUMAN_PRINCIPAL_COZO_DB_PATH", &cozo) };
    write_grants(
        &grants_template,
        &grants_for(&pubkey, &["human_principal.read"]),
    );
    let missing = tempfile::tempdir().expect("missing");
    // FIXME: Audit that the environment access only happens in single-threaded code.
    unsafe {
        std::env::set_var(
            "OP_DECOY_TRUST_STORE",
            missing.path().join("missing-trust.json"),
        )
    };
    // Reload the validator with an empty trust store while retaining the
    // already-authoritative session/genesis state. This test is about trust
    // fail-closure, not in-process restart hydration.
    let (srv2, env) = start_server_with_env_and_engine(false, env, engine).await;
    let ch = tls_channel(srv2.addr, &srv2.ca_pem).await;
    assert_unauthenticated(
        call_method(
            ch.clone(),
            CallOpts {
                plugin_id: "human_principal",
                method: "resolve_key",
                capability: "human_principal.read",
                args: prost_struct(BTreeMap::from([(
                    "human_pubkey".to_string(),
                    prost_str(&pubkey),
                )])),
                assertion: Some(fresh_signed(&env.issuer, &pubkey, [0x45; 16])),
                ghostbridge: None,
                wireguard_pubkey: None,
                extra_assertion: None,
            },
        )
        .await,
        "UnknownDecoyKey",
    );
    env.set_grants(&serde_json::json!({ fp.clone(): { "capabilities": ["identity_sled.read"] } }));
    let legacy_only = call_method(
        ch,
        CallOpts {
            plugin_id: "identity_sled",
            method: "get_identity",
            capability: "identity_sled.read",
            args: prost_struct(BTreeMap::from([(
                "session_id".to_string(),
                prost_str(&derive_session_id(&container_pk)),
            )])),
            assertion: None,
            ghostbridge: Some((&fp, &trace)),
            wireguard_pubkey: Some(&container_pk),
            extra_assertion: None,
        },
    )
    .await;
    assert_unauthenticated(legacy_only, "Missing Oracle identity assertion");
}

// ?? VAL-CROSS-010 ???????????????????????????????????????????????????????????

#[tokio::test(flavor = "multi_thread")]
async fn e2e_alias_mutation_has_no_auth_effect() {
    let (srv, env) = start_server(false).await;
    let ch = tls_channel(srv.addr, &srv.ca_pem).await;
    let pubkey = pk(40);
    register_human(ch.clone(), &env, &pubkey, "alice", [0x47; 16]).await;
    env.grant_human(&pubkey, &["human_principal.read", "human_principal.write"]);
    let before = assert_ok(
        call_method(
            ch.clone(),
            CallOpts {
                plugin_id: "human_principal",
                method: "resolve_key",
                capability: "human_principal.read",
                args: prost_struct(BTreeMap::from([(
                    "human_pubkey".to_string(),
                    prost_str(&pubkey),
                )])),
                assertion: Some(fresh_signed(&env.issuer, &pubkey, [0x48; 16])),
                ghostbridge: None,
                wireguard_pubkey: None,
                extra_assertion: None,
            },
        )
        .await,
    );
    let derived = derive_principal_id(&pubkey);
    assert_ok(
        call_method(
            ch.clone(),
            CallOpts {
                plugin_id: "human_principal",
                method: "set_alias",
                capability: "human_principal.write",
                args: prost_struct(BTreeMap::from([
                    ("principal_id".to_string(), prost_str(&derived)),
                    ("display_alias".to_string(), prost_str("bob")),
                ])),
                assertion: Some(fresh_signed(&env.issuer, &pubkey, [0x49; 16])),
                ghostbridge: None,
                wireguard_pubkey: None,
                extra_assertion: None,
            },
        )
        .await,
    );
    let after = assert_ok(
        call_method(
            ch,
            CallOpts {
                plugin_id: "human_principal",
                method: "resolve_key",
                capability: "human_principal.read",
                args: prost_struct(BTreeMap::from([(
                    "human_pubkey".to_string(),
                    prost_str(&pubkey),
                )])),
                assertion: Some(fresh_signed(&env.issuer, &pubkey, [0x4A; 16])),
                ghostbridge: None,
                wireguard_pubkey: None,
                extra_assertion: None,
            },
        )
        .await,
    );
    assert_eq!(principal_id_from(&before), principal_id_from(&after));
    assert_eq!(human_pubkey_from(&before), human_pubkey_from(&after));
    assert_eq!(principal_id_for(&pubkey), derive_principal_id(&pubkey));
}

// ?? VAL-CROSS-011 ???????????????????????????????????????????????????????????

#[tokio::test(flavor = "multi_thread")]
async fn e2e_per_principal_grant_isolation() {
    let (srv, env) = start_server(false).await;
    let ch = tls_channel(srv.addr, &srv.ca_pem).await;
    let a = pk(41);
    let b = pk(42);
    register_human(ch.clone(), &env, &a, "a", [0x4B; 16]).await;
    register_human(ch.clone(), &env, &b, "b", [0x4C; 16]).await;
    env.set_grants(&grants_for(&b, &["human_principal.read"]));
    assert_permission_denied(
        call_method(
            ch.clone(),
            CallOpts {
                plugin_id: "human_principal",
                method: "resolve_key",
                capability: "human_principal.read",
                args: prost_struct(BTreeMap::from([(
                    "human_pubkey".to_string(),
                    prost_str(&a),
                )])),
                assertion: Some(fresh_signed(&env.issuer, &a, [0x4D; 16])),
                ghostbridge: None,
                wireguard_pubkey: None,
                extra_assertion: None,
            },
        )
        .await,
    );
    env.set_grants(&grants_for(&a, &["human_principal.read"]));
    assert_ok(
        call_method(
            ch,
            CallOpts {
                plugin_id: "human_principal",
                method: "resolve_key",
                capability: "human_principal.read",
                args: prost_struct(BTreeMap::from([(
                    "human_pubkey".to_string(),
                    prost_str(&a),
                )])),
                assertion: Some(fresh_signed(&env.issuer, &a, [0x4E; 16])),
                ghostbridge: None,
                wireguard_pubkey: None,
                extra_assertion: None,
            },
        )
        .await,
    );
}

// ?? VAL-CROSS-012 ???????????????????????????????????????????????????????????

#[tokio::test(flavor = "multi_thread")]
async fn e2e_missing_connect_info_fails_closed() {
    let env = TestEnv::new().await;
    let (srv, env) = start_server_with_env(false, env).await;
    let ch_ok = tls_channel(srv.addr, &srv.ca_pem).await;
    let pubkey = pk(43);
    register_human(ch_ok, &env, &pubkey, "ci", [0x4F; 16]).await;
    drop(srv);
    let (srv2, env2) = start_server_legacy_peer_ip(true, env).await;
    let ch_bad = tls_channel(srv2.addr, &srv2.ca_pem).await;
    env2.grant_human(&pubkey, &["human_principal.read"]);
    assert_unauthenticated(
        call_method(
            ch_bad,
            CallOpts {
                plugin_id: "human_principal",
                method: "resolve_key",
                capability: "human_principal.read",
                args: prost_struct(BTreeMap::from([(
                    "human_pubkey".to_string(),
                    prost_str(&pubkey),
                )])),
                assertion: Some(fresh_signed(&env2.issuer, &pubkey, [0x50; 16])),
                ghostbridge: None,
                wireguard_pubkey: None,
                extra_assertion: None,
            },
        )
        .await,
        "MissingConnectInfo",
    );
}

// ?? VAL-CROSS-013 ???????????????????????????????????????????????????????????

#[tokio::test(flavor = "multi_thread")]
async fn e2e_revocation_durable_across_restart() {
    let env = TestEnv::new().await;
    let cozo = env.cozo_path();
    let (srv, env) = start_server_with_env(false, env).await;
    let ch = tls_channel(srv.addr, &srv.ca_pem).await;
    let k1 = pk(44);
    let k2 = pk(45);
    register_human(ch.clone(), &env, &k1, "k1", [0x51; 16]).await;
    register_human(ch.clone(), &env, &k2, "k2", [0x52; 16]).await;
    env.set_grants(&serde_json::json!({
        principal_id_for(&k1): { "capabilities": ["human_principal.read", "human_principal.write"] },
        principal_id_for(&k2): { "capabilities": ["human_principal.read"] },
    }));
    resolve_human(ch.clone(), &env, &k1, [0x53; 16]).await;
    resolve_human(ch.clone(), &env, &k2, [0x54; 16]).await;
    env.set_grants(&serde_json::json!({
        principal_id_for(&k1): { "capabilities": ["human_principal.read", "human_principal.write"] },
        principal_id_for(&k2): { "capabilities": ["human_principal.read"] },
    }));
    assert_ok(
        call_method(
            ch.clone(),
            CallOpts {
                plugin_id: "human_principal",
                method: "revoke_key",
                capability: "human_principal.write",
                args: prost_struct(BTreeMap::from([(
                    "human_pubkey".to_string(),
                    prost_str(&k1),
                )])),
                assertion: Some(fresh_signed(&env.issuer, &k1, [0x55; 16])),
                ghostbridge: None,
                wireguard_pubkey: None,
                extra_assertion: None,
            },
        )
        .await,
    );
    drop(srv);
    drop(ch);
    // FIXME: Audit that the environment access only happens in single-threaded code.
    unsafe { std::env::set_var("OP_HUMAN_PRINCIPAL_COZO_DB_PATH", &cozo) };
    let (srv2, env) = start_server_with_env(false, env).await;
    let ch2 = tls_channel(srv2.addr, &srv2.ca_pem).await;
    env.set_grants(&serde_json::json!({
        principal_id_for(&k1): { "capabilities": ["human_principal.read"] },
        principal_id_for(&k2): { "capabilities": ["human_principal.read"] },
    }));
    assert_unauthenticated(
        call_method(
            ch2.clone(),
            CallOpts {
                plugin_id: "human_principal",
                method: "resolve_key",
                capability: "human_principal.read",
                args: prost_struct(BTreeMap::from([(
                    "human_pubkey".to_string(),
                    prost_str(&k1),
                )])),
                assertion: Some(fresh_signed(&env.issuer, &k1, [0x56; 16])),
                ghostbridge: None,
                wireguard_pubkey: None,
                extra_assertion: None,
            },
        )
        .await,
        "RevokedPrincipal",
    );
    assert_ok(
        call_method(
            ch2,
            CallOpts {
                plugin_id: "human_principal",
                method: "resolve_key",
                capability: "human_principal.read",
                args: prost_struct(BTreeMap::from([(
                    "human_pubkey".to_string(),
                    prost_str(&k2),
                )])),
                assertion: Some(fresh_signed(&env.issuer, &k2, [0x57; 16])),
                ghostbridge: None,
                wireguard_pubkey: None,
                extra_assertion: None,
            },
        )
        .await,
    );
}

// ?? VAL-CROSS-014 ???????????????????????????????????????????????????????????

#[tokio::test(flavor = "multi_thread")]
async fn e2e_concurrent_multi_principal_interleaving() {
    let (srv, env) = start_server(false).await;
    let ch = tls_channel(srv.addr, &srv.ca_pem).await;
    let a = pk(46);
    let b = pk(47);
    register_human(ch.clone(), &env, &a, "A", [0x58; 16]).await;
    register_human(ch.clone(), &env, &b, "B", [0x59; 16]).await;
    env.set_grants(&serde_json::json!({
        principal_id_for(&a): { "capabilities": ["human_principal.read"] },
        principal_id_for(&b): { "capabilities": ["human_principal.read"] },
    }));
    let (ra, rb) = tokio::join!(
        call_method(
            ch.clone(),
            CallOpts {
                plugin_id: "human_principal",
                method: "resolve_key",
                capability: "human_principal.read",
                args: prost_struct(BTreeMap::from([(
                    "human_pubkey".to_string(),
                    prost_str(&a),
                )])),
                assertion: Some(fresh_signed(&env.issuer, &a, [0x5A; 16])),
                ghostbridge: None,
                wireguard_pubkey: None,
                extra_assertion: None,
            },
        ),
        call_method(
            ch.clone(),
            CallOpts {
                plugin_id: "human_principal",
                method: "resolve_key",
                capability: "human_principal.read",
                args: prost_struct(BTreeMap::from([(
                    "human_pubkey".to_string(),
                    prost_str(&b),
                )])),
                assertion: Some(fresh_signed(&env.issuer, &b, [0x5B; 16])),
                ghostbridge: None,
                wireguard_pubkey: None,
                extra_assertion: None,
            },
        ),
    );
    let va = assert_ok(ra);
    let vb = assert_ok(rb);
    let check_pid = |v: &ProstValue, pk: &str| {
        assert_eq!(principal_id_from(v), derive_principal_id(pk));
    };
    check_pid(&va, &a);
    check_pid(&vb, &b);
}

// ?? VAL-CROSS-018 ???????????????????????????????????????????????????????????

#[tokio::test(flavor = "multi_thread")]
async fn e2e_human_registration_anchors_session_genesis() {
    let (srv, env) = start_server(false).await;
    let ch = tls_channel(srv.addr, &srv.ca_pem).await;
    let pubkey = pk(48);
    register_human(ch, &env, &pubkey, "sidefx", [0x5C; 16]).await;
    let session = srv
        .engine
        .session_context_for_actor(&pubkey)
        .await
        .expect("verified registration anchors a session");
    assert_eq!(session.session_id, derive_session_id(&pubkey));
    assert_eq!(session.wireguard_pubkey, pubkey);
    assert_eq!(session.genesis_hex.len(), 64);
    assert!(session.genesis_hex.chars().all(|c| c.is_ascii_hexdigit()));
}

// ?? VAL-CROSS-019 ???????????????????????????????????????????????????????????

#[tokio::test(flavor = "multi_thread")]
async fn e2e_revocation_tombstone_end_to_end() {
    let (srv, env) = start_server(false).await;
    let ch = tls_channel(srv.addr, &srv.ca_pem).await;
    let pubkey = pk(49);
    register_human(ch.clone(), &env, &pubkey, "tomb", [0x5D; 16]).await;
    env.grant_human(&pubkey, &["human_principal.write", "human_principal.read"]);
    assert_ok(
        call_method(
            ch.clone(),
            CallOpts {
                plugin_id: "human_principal",
                method: "revoke_key",
                capability: "human_principal.write",
                args: prost_struct(BTreeMap::from([(
                    "human_pubkey".to_string(),
                    prost_str(&pubkey),
                )])),
                assertion: Some(fresh_signed(&env.issuer, &pubkey, [0x5E; 16])),
                ghostbridge: None,
                wireguard_pubkey: None,
                extra_assertion: None,
            },
        )
        .await,
    );
    let dup = call_method(
        ch.clone(),
        CallOpts {
            plugin_id: "human_principal",
            method: "register_key",
            capability: "human_principal.write",
            args: prost_struct(BTreeMap::from([
                ("human_pubkey".to_string(), prost_str(&pubkey)),
                ("display_alias".to_string(), prost_str("retry")),
            ])),
            assertion: Some(fresh_signed(&env.issuer, &pubkey, [0x5F; 16])),
            ghostbridge: None,
            wireguard_pubkey: None,
            extra_assertion: None,
        },
    )
    .await;
    match dup {
        CallOutcome::RpcErr(_) => {}
        CallOutcome::Ok(_) => {}
        CallOutcome::GateDenied => panic!("duplicate register should not be gate denied"),
    }
    assert_unauthenticated(
        call_method(
            ch,
            CallOpts {
                plugin_id: "human_principal",
                method: "resolve_key",
                capability: "human_principal.read",
                args: prost_struct(BTreeMap::from([(
                    "human_pubkey".to_string(),
                    prost_str(&pubkey),
                )])),
                assertion: Some(fresh_signed(&env.issuer, &pubkey, [0x60; 16])),
                ghostbridge: None,
                wireguard_pubkey: None,
                extra_assertion: None,
            },
        )
        .await,
        "RevokedPrincipal",
    );
}

// ?? VAL-CROSS-021 ???????????????????????????????????????????????????????????

#[tokio::test(flavor = "multi_thread")]
async fn e2e_replay_nonce_remains_consumed_after_server_restart() {
    let (srv, env) = start_server(false).await;
    let ch = tls_channel(srv.addr, &srv.ca_pem).await;
    let pubkey = pk(50);
    register_human(ch.clone(), &env, &pubkey, "restart", [0x61; 16]).await;
    env.grant_human(&pubkey, &["human_principal.read"]);
    let assertion = fresh_signed(&env.issuer, &pubkey, [0x62; 16]);
    assert_ok(
        call_method(
            ch.clone(),
            CallOpts {
                plugin_id: "human_principal",
                method: "resolve_key",
                capability: "human_principal.read",
                args: prost_struct(BTreeMap::from([(
                    "human_pubkey".to_string(),
                    prost_str(&pubkey),
                )])),
                assertion: Some(assertion.clone()),
                ghostbridge: None,
                wireguard_pubkey: None,
                extra_assertion: None,
            },
        )
        .await,
    );
    let engine = srv.engine.clone();
    drop(srv);
    drop(ch);
    let (srv2, _env) = start_server_with_env_and_engine(false, env, engine).await;
    let ch2 = tls_channel(srv2.addr, &srv2.ca_pem).await;
    assert_unauthenticated(
        call_method(
            ch2,
            CallOpts {
                plugin_id: "human_principal",
                method: "resolve_key",
                capability: "human_principal.read",
                args: prost_struct(BTreeMap::from([(
                    "human_pubkey".to_string(),
                    prost_str(&pubkey),
                )])),
                assertion: Some(assertion),
                ghostbridge: None,
                wireguard_pubkey: None,
                extra_assertion: None,
            },
        )
        .await,
        "Replay",
    );
}

// ?? VAL-E2E-023 (count pin) ?????????????????????????????????????????????????

#[test]
fn battery_test_count_is_pinned() {
    assert_eq!(EXPECTED_TEST_COUNT, 41);
}

/// Exercise the shipped client against the real TLS interceptor, identity sled
/// validator, grants and dispatcher. The identity subsystem authors the bytes.
#[tokio::test(flavor = "multi_thread")]
async fn built_in_client_forwards_authored_identity_and_preserves_grant_gate() {
    use op_grpc_bridge::{GhostbridgeCallMetadata, GrpcClientPool, RemoteOperationClient};

    struct RestoreEnvironment(Vec<(&'static str, Option<std::ffi::OsString>)>);
    impl Drop for RestoreEnvironment {
        fn drop(&mut self) {
            for (key, value) in &self.0 {
                if let Some(value) = value {
                    // FIXME: Audit that the environment access only happens in single-threaded code.
                    unsafe { std::env::set_var(key, value) };
                } else {
                    // FIXME: Audit that the environment access only happens in single-threaded code.
                    unsafe { std::env::remove_var(key) };
                }
            }
        }
    }

    let (srv, env) = start_server(false).await;
    let _restore = RestoreEnvironment(
        [
            "OP_DBUS_GRPC_CA_FILE",
            "OP_DBUS_GRPC_TLS_DOMAIN",
            "OP_IDENTITY_SESSION_ID",
        ]
        .into_iter()
        .map(|key| (key, std::env::var_os(key)))
        .collect(),
    );
    let ca = env._root.path().join("client-ca.pem");
    std::fs::write(&ca, &srv.ca_pem).unwrap();
    // FIXME: Audit that the environment access only happens in single-threaded code.
    unsafe { std::env::set_var("OP_DBUS_GRPC_CA_FILE", &ca) };
    // FIXME: Audit that the environment access only happens in single-threaded code.
    unsafe { std::env::set_var("OP_DBUS_GRPC_TLS_DOMAIN", "localhost") };
    let pubkey = pk(91);
    let channel = tls_channel(srv.addr, &srv.ca_pem).await;
    register_human(channel, &env, &pubkey, "built-in-client", [0x91; 16]).await;
    env.grant_human(&pubkey, &["human_principal.read"]);
    let session_id = derive_session_id(&pubkey);
    // FIXME: Audit that the environment access only happens in single-threaded code.
    unsafe { std::env::set_var("OP_IDENTITY_SESSION_ID", &session_id) };
    let projection = op_identity::resolve_identity_credential_session(Some(&session_id)).unwrap();
    let encoded = projection.sealed_id_header_for_transport("grpc").unwrap();
    let wire = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(encoded)
        .unwrap();
    let mut identity = GhostbridgeCallMetadata {
        sealed_id: Some(wire.clone()),
        oracle_assertion: None,
        session_genesis: None,
        trace_id: None,
        wireguard_pubkey: None,
    };
    let client = RemoteOperationClient::new(
        Arc::new(GrpcClientPool::new()),
        &format!("https://{}", srv.addr),
        "identity-test",
    );
    let args = || vec![simd_json::json!({"human_pubkey": pubkey.clone()})];
    async fn client_call(
        client: &RemoteOperationClient,
        pubkey: &str,
        identity: &GhostbridgeCallMetadata,
    ) -> Result<simd_json::OwnedValue, op_grpc_bridge::grpc_client::GrpcClientError> {
        client
            .call_method_with_metadata(
                "human_principal",
                "/org/opdbus/v1/plugins/human_principal",
                "org.opdbus.v1.PluginV1",
                "resolve_key",
                vec![simd_json::json!({"human_pubkey": pubkey})],
                "",
                "human_principal.read",
                identity,
            )
            .await
    }
    client_call(&client, &pubkey, &identity)
        .await
        .expect("exact authored SID1 passes the receiver");
    client
        .call_method(
            "human_principal",
            "/org/opdbus/v1/plugins/human_principal",
            "org.opdbus.v1.PluginV1",
            "resolve_key",
            args(),
            "",
            "human_principal.read",
        )
        .await
        .expect("native client reads the configured private credential");

    // Even a well-formed envelope with a recomputed integrity trailer cannot
    // replace the credential authored and stored by the identity subsystem.
    let mut forged = op_identity::sealed_id::SealedId::open(&wire).unwrap();
    forged.trace_id = "caller-reconstructed".into();
    identity.sealed_id = Some(forged.seal().unwrap());
    let error = client_call(&client, &pubkey, &identity)
        .await
        .unwrap_err()
        .to_string();
    assert!(error.to_lowercase().contains("unauthenticated"), "{error}");
    identity.sealed_id = Some(wire.clone());
    identity.oracle_assertion = Some(fresh_signed(&env.issuer, &pubkey, [0x92; 16]).to_wire());
    assert!(
        client_call(&client, &pubkey, &identity)
            .await
            .unwrap_err()
            .to_string()
            .contains("multiple identity")
    );
    identity.sealed_id = None;
    client_call(&client, &pubkey, &identity)
        .await
        .expect("exact caller OIA1 passes the same receiver");
    identity.oracle_assertion = None;
    assert!(
        client_call(&client, &pubkey, &identity)
            .await
            .unwrap_err()
            .to_string()
            .contains("missing identity")
    );

    identity.sealed_id = Some(wire);
    env.set_grants(&empty_grants());
    assert!(
        client_call(&client, &pubkey, &identity).await.is_err(),
        "valid identity cannot bypass principal grants"
    );
}

/// The explicit decoy path acquires one OIA1 wire envelope and exercises the
/// receiver's trust, registration, provisioning, signature and grant gates.
#[tokio::test(flavor = "multi_thread")]
async fn built_in_client_acquires_oia1_from_decoy() {
    use op_grpc_bridge::{DecoyAssertionClient, GrpcClientPool, RemoteOperationClient};

    struct RestoreEnvironment(Vec<(&'static str, Option<std::ffi::OsString>)>);
    impl Drop for RestoreEnvironment {
        fn drop(&mut self) {
            for (key, value) in &self.0 {
                if let Some(value) = value {
                    // FIXME: Audit that the environment access only happens in single-threaded code.
                    unsafe { std::env::set_var(key, value) };
                } else {
                    // FIXME: Audit that the environment access only happens in single-threaded code.
                    unsafe { std::env::remove_var(key) };
                }
            }
        }
    }

    let (srv, env) = start_server(false).await;
    let _restore = RestoreEnvironment(
        ["OP_DBUS_GRPC_CA_FILE", "OP_DBUS_GRPC_TLS_DOMAIN"]
            .into_iter()
            .map(|key| (key, std::env::var_os(key)))
            .collect(),
    );
    let ca = env._root.path().join("decoy-client-ca.pem");
    std::fs::write(&ca, &srv.ca_pem).expect("client CA");
    // FIXME: Audit that the environment access only happens in single-threaded code.
    unsafe { std::env::set_var("OP_DBUS_GRPC_CA_FILE", &ca) };
    // FIXME: Audit that the environment access only happens in single-threaded code.
    unsafe { std::env::set_var("OP_DBUS_GRPC_TLS_DOMAIN", "localhost") };

    let pubkey = pk(93);
    let channel = tls_channel(srv.addr, &srv.ca_pem).await;
    register_human(channel, &env, &pubkey, "decoy-client", [0x93; 16]).await;

    let signed = env
        .issuer
        .issue(&pubkey, test_ip(), Duration::from_secs(60))
        .expect("fresh decoy assertion");
    let encoded = base64::engine::general_purpose::STANDARD.encode(signed.to_wire());
    let (issuer_addr, issuer_task) = start_assertion_issuer(encoded).await;
    let issuer = DecoyAssertionClient::new(issuer_addr, test_ip()).expect("decoy client");
    let client = RemoteOperationClient::new(
        Arc::new(GrpcClientPool::new()),
        &format!("https://{}", srv.addr),
        "decoy-client",
    );
    let result = client
        .call_method_via_decoy(
            "human_principal",
            "/org/opdbus/v1/plugins/human_principal",
            "org.opdbus.v1.PluginV1",
            "resolve_key",
            vec![simd_json::json!({"human_pubkey": pubkey.clone()})],
            "",
            "human_principal.read",
            &issuer,
        )
        .await
        .expect("decoy OIA1 reaches canonical human resolve");
    let rendered = serde_json::to_string(&result).expect("render resolve result");
    assert!(
        rendered.contains(&pubkey),
        "canonical result omitted pubkey: {rendered}"
    );
    issuer_task.abort();

    // A validly framed assertion from a key absent from the receiver trust
    // store is acquired, then rejected by the server's native validator.
    let untrusted = DecoyIssuer::new(
        SigningKey::from_bytes(&[8u8; 32]),
        "untrusted-decoy",
        Duration::from_secs(900),
    );
    let forged = untrusted
        .issue(&pubkey, test_ip(), Duration::from_secs(60))
        .expect("fresh untrusted assertion");
    let forged_body = base64::engine::general_purpose::STANDARD.encode(forged.to_wire());
    let (forged_addr, forged_task) = start_assertion_issuer(forged_body).await;
    let forged_client = DecoyAssertionClient::new(forged_addr, test_ip()).expect("decoy client");
    let error = client
        .call_method_via_decoy(
            "human_principal",
            "/org/opdbus/v1/plugins/human_principal",
            "org.opdbus.v1.PluginV1",
            "resolve_key",
            vec![simd_json::json!({"human_pubkey": pubkey})],
            "",
            "human_principal.read",
            &forged_client,
        )
        .await
        .expect_err("untrusted issuer must be denied by receiver");
    assert!(error.to_string().contains("UnknownDecoyKey"), "{error}");
    forged_task.abort();
}

fn audit_request<T>(
    env: &TestEnv,
    pubkey: &str,
    nonce: u8,
    capability: Option<&str>,
    body: T,
) -> Request<T> {
    let mut request = Request::new(body);
    attach_assertion(
        request.metadata_mut(),
        &fresh_signed(&env.issuer, pubkey, [nonce; 16]).to_wire(),
    );
    if let Some(capability) = capability {
        attach_capability(request.metadata_mut(), capability);
    }
    request
}

#[tokio::test(flavor = "multi_thread")]
async fn event_chain_rpc_rejects_missing_grants_and_unbacked_private_aliases() {
    use op_grpc_bridge::proto::{event_chain_service_client::EventChainServiceClient, *};
    let (srv, env) = start_server(false).await;
    let channel = tls_channel(srv.addr, &srv.ca_pem).await;
    let pubkey = pk(202);
    register_human(channel.clone(), &env, &pubkey, "audit-reader", [0xc0; 16]).await;
    let mut client = EventChainServiceClient::new(channel);
    let status = client
        .get_events(Request::new(GetEventsRequest::default()))
        .await
        .unwrap_err();
    assert_eq!(status.code(), Code::Unauthenticated);
    let status = client
        .get_events(audit_request(
            &env,
            &pubkey,
            0xc1,
            Some("snowball.read"),
            GetEventsRequest::default(),
        ))
        .await
        .unwrap_err();
    assert_eq!(
        status.code(),
        Code::PermissionDenied,
        "transport identity is not an audit grant"
    );
    env.grant_human(&pubkey, &["snowball.read", "snowball.invoke"]);
    let status = client
        .get_events(audit_request(
            &env,
            &pubkey,
            0xc2,
            None,
            GetEventsRequest::default(),
        ))
        .await
        .unwrap_err();
    assert_eq!(status.code(), Code::PermissionDenied);
    let status = client
        .get_events(audit_request(
            &env,
            &pubkey,
            0xc3,
            Some("snowball.invoke"),
            GetEventsRequest::default(),
        ))
        .await
        .unwrap_err();
    assert_eq!(
        status.code(),
        Code::PermissionDenied,
        "capability must match the pinned query method"
    );
    let mut duplicate = audit_request(
        &env,
        &pubkey,
        0xc4,
        Some("snowball.read"),
        GetEventsRequest::default(),
    );
    duplicate.metadata_mut().append(
        DECLARED_CAPABILITY_HEADER,
        MetadataValue::from_static("snowball.read"),
    );
    assert_eq!(
        client.get_events(duplicate).await.unwrap_err().code(),
        Code::PermissionDenied
    );

    let page = client
        .get_events(audit_request(
            &env,
            &pubkey,
            0xc5,
            Some("snowball.read"),
            GetEventsRequest {
                limit: 1,
                ..Default::default()
            },
        ))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(page.events.len(), 1);
    assert!(page.has_more);
    let verification = client
        .verify_chain(audit_request(
            &env,
            &pubkey,
            0xc6,
            Some("snowball.read"),
            VerifyChainRequest::default(),
        ))
        .await
        .unwrap()
        .into_inner();
    assert!(verification.valid);
    let proof = client
        .get_proof(audit_request(
            &env,
            &pubkey,
            0xc7,
            Some("snowball.read"),
            GetProofRequest {
                event_id: page.events[0].event_id,
            },
        ))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(proof.event_hash, page.events[0].event_hash);
    assert!(proof.batch_first_event_id > 0);
    client
        .prove_tag_immutability(audit_request(
            &env,
            &pubkey,
            0xc8,
            Some("snowball.read"),
            ProveTagImmutabilityRequest {
                plugin_id: "snowball".into(),
                tag: "fixture-tag".into(),
            },
        ))
        .await
        .unwrap();

    let head = srv.engine.event_chain.read().await.last_hash().to_owned();
    assert_eq!(
        client
            .get_snapshot(audit_request(
                &env,
                &pubkey,
                0xc9,
                Some("snowball.read"),
                GetSnapshotRequest {
                    snapshot_id: "unbacked".into()
                }
            ))
            .await
            .unwrap_err()
            .code(),
        Code::FailedPrecondition
    );
    assert_eq!(
        client
            .create_snapshot(audit_request(
                &env,
                &pubkey,
                0xca,
                Some("snowball.read"),
                CreateSnapshotRequest {
                    plugin_id: "identity_sled".into()
                }
            ))
            .await
            .unwrap_err()
            .code(),
        Code::PermissionDenied
    );
    assert_eq!(
        client
            .create_snapshot(audit_request(
                &env,
                &pubkey,
                0xcb,
                Some("snowball.invoke"),
                CreateSnapshotRequest {
                    plugin_id: "identity_sled".into()
                }
            ))
            .await
            .unwrap_err()
            .code(),
        Code::FailedPrecondition
    );
    assert_eq!(
        client
            .search_semantic_trace(audit_request(
                &env,
                &pubkey,
                0xcc,
                None,
                SearchSemanticTraceRequest::default()
            ))
            .await
            .unwrap_err()
            .code(),
        Code::FailedPrecondition
    );
    assert_eq!(
        srv.engine.event_chain.read().await.last_hash(),
        head,
        "unsupported aliases must have no backend effects"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn event_chain_rpc_stream_uses_engine_replays_and_rechecks_revocation() {
    use op_grpc_bridge::proto::{
        SubscribeEventsRequest, event_chain_service_client::EventChainServiceClient,
    };
    let (srv, env) = start_server(false).await;
    let channel = tls_channel(srv.addr, &srv.ca_pem).await;
    let pubkey = pk(203);
    register_human(channel.clone(), &env, &pubkey, "audit-stream", [0xd0; 16]).await;
    env.grant_human(&pubkey, &["snowball.read"]);
    let mut client = EventChainServiceClient::new(channel);
    let mut stream = client
        .subscribe_events(audit_request(
            &env,
            &pubkey,
            0xd1,
            Some("snowball.read"),
            SubscribeEventsRequest::default(),
        ))
        .await
        .unwrap()
        .into_inner();
    // Real MutationEngine append, not a separately fabricated service channel.
    srv.engine
        .dispatch_method_call(
            "snowball",
            "verify_chain",
            "{}",
            Some("snowball.read"),
            &principal_id_for(&pubkey),
        )
        .await
        .unwrap();
    let observed = tokio::time::timeout(Duration::from_secs(5), stream.message())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(observed.plugin_id, "snowball");
    assert_eq!(
        observed.event_hash,
        srv.engine
            .event_chain
            .read()
            .await
            .events()
            .last()
            .unwrap()
            .event_hash
    );

    let mut replay = client
        .subscribe_events(audit_request(
            &env,
            &pubkey,
            0xd2,
            Some("snowball.read"),
            SubscribeEventsRequest {
                from_event_id: observed.event_id,
                ..Default::default()
            },
        ))
        .await
        .unwrap()
        .into_inner();
    let replayed = tokio::time::timeout(Duration::from_secs(5), replay.message())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(replayed.event_id, observed.event_id);
    assert_eq!(replayed.event_hash, observed.event_hash);
    let invalid_cursor = audit_request(
        &env,
        &pubkey,
        0xd3,
        Some("snowball.read"),
        SubscribeEventsRequest {
            from_event_id: u64::MAX,
            ..Default::default()
        },
    );
    assert_eq!(
        client
            .subscribe_events(invalid_cursor)
            .await
            .unwrap_err()
            .code(),
        Code::OutOfRange
    );

    env.set_grants(&empty_grants());
    srv.engine
        .dispatch_method_call(
            "snowball",
            "verify_chain",
            "{}",
            Some("snowball.read"),
            "internal-fixture",
        )
        .await
        .unwrap();
    let revoked = tokio::time::timeout(Duration::from_secs(5), stream.message())
        .await
        .unwrap()
        .unwrap_err();
    assert_eq!(revoked.code(), Code::PermissionDenied);
}
