//! Full canonical admission -> pinned mTLS D-Bus -> native owner -> SHM proof
//! for the sixteen decoy network methods. All native effects are isolated in
//! an in-memory kernel and a loopback Cloudflare protocol fixture.

use super::*;
use crate::human_principal_dispatch::tests::{
    pk, register, temp_cozo, temp_shm_with_schemas, test_engine,
};
use crate::identity_sled_dispatch::{tests::write_identity, IdentitySledStore};
use anyhow::{Context, Result};
use async_trait::async_trait;
use axum::{
    body::to_bytes,
    extract::{Request, State},
    http::{header::AUTHORIZATION, Method, StatusCode},
    response::IntoResponse,
    routing::any,
    Json, Router,
};
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use op_network::wireguard_kernel::{KernelDeviceConfig, KernelDeviceSnapshot, KernelPeerSnapshot};
use op_plugins::state_plugins::{
    decoy_network::{
        wgcf_owner::WgcfOwner,
        wireguard_owner::{
            DecoyRole, KernelWireGuard, LinuxKernelWireGuard, WireGuardKeyPair, WireGuardOwner,
        },
    },
    human_principal::HumanPrincipalPlugin,
    identity_sled::IdentitySledPlugin,
    runtime_documents::RuntimeDocuments,
    wg_opdbus::WgOpdbusPlugin,
    wgcf::{WgcfConfig, WgcfPlugin},
    wireguard::WireGuardPlugin,
};
use op_state::StatePlugin;
use rcgen::{BasicConstraints, CertificateParams, IsCa, KeyPair};
use rustls::pki_types::PrivateKeyDer;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    net::IpAddr,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};
use tokio::sync::Mutex;

const INTERFACE_NAME: &str = "wg-decoy";
const WGCF_INTERFACE: &str = "wgcf";
const FIXTURE_TOKEN: &str = "fixture-private-token";
const FIXTURE_LICENSE: &str = "fixture-private-license";

struct Environment {
    values: Vec<(&'static str, Option<std::ffi::OsString>)>,
}

impl Environment {
    fn capture(keys: impl IntoIterator<Item = &'static str>) -> Self {
        Self {
            values: keys
                .into_iter()
                .map(|key| (key, std::env::var_os(key)))
                .collect(),
        }
    }
}

impl Drop for Environment {
    fn drop(&mut self) {
        for (key, value) in &self.values {
            match value {
                // FIXME: Audit that the environment access only happens in single-threaded code.
                Some(value) => unsafe { std::env::set_var(key, value) },
                // FIXME: Audit that the environment access only happens in single-threaded code.
                None => unsafe { std::env::remove_var(key) },
            }
        }
    }
}

#[derive(Default)]
struct MemoryKernel {
    devices: Mutex<BTreeMap<String, KernelDeviceSnapshot>>,
    calls: AtomicUsize,
}

impl MemoryKernel {
    fn empty(name: &str) -> KernelDeviceSnapshot {
        KernelDeviceSnapshot {
            name: name.into(),
            public_key: None,
            listen_port: None,
            fwmark: None,
            peers: Vec::new(),
            addresses: Vec::new(),
            mtu: None,
            admin_up: false,
        }
    }

    fn count(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl KernelWireGuard for MemoryKernel {
    fn generate_keypair(&self) -> Result<WireGuardKeyPair> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        <LinuxKernelWireGuard as KernelWireGuard>::generate_keypair(&LinuxKernelWireGuard)
    }

    fn public_from_private(&self, private: &str) -> Result<String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        <LinuxKernelWireGuard as KernelWireGuard>::public_from_private(
            &LinuxKernelWireGuard,
            private,
        )
    }

    async fn apply_device(
        &self,
        name: &str,
        config: KernelDeviceConfig,
    ) -> Result<KernelDeviceSnapshot> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let mut devices = self.devices.lock().await;
        let device = devices
            .entry(name.into())
            .or_insert_with(|| Self::empty(name));
        if let Some(private) = config.private_key.as_deref() {
            device.public_key = Some(
                <LinuxKernelWireGuard as KernelWireGuard>::public_from_private(
                    &LinuxKernelWireGuard,
                    private,
                )?,
            );
        }
        if let Some(port) = config.listen_port {
            device.listen_port = Some(port);
        }
        if let Some(fwmark) = config.fwmark {
            device.fwmark = Some(fwmark);
        }
        if config.replace_peers {
            device.peers.clear();
        }
        for peer in config.peers {
            device
                .peers
                .retain(|present| present.public_key != peer.public_key);
            let preshared_key_sha256 = peer
                .preshared_key
                .as_deref()
                .map(|key| -> Result<[u8; 32]> {
                    let bytes = STANDARD.decode(key)?;
                    Ok(Sha256::digest(bytes).into())
                })
                .transpose()?;
            device.peers.push(KernelPeerSnapshot {
                public_key: peer.public_key,
                preshared_key_sha256,
                endpoint: peer.endpoint,
                persistent_keepalive: peer.persistent_keepalive,
                allowed_ips: peer.allowed_ips,
                latest_handshake_unix: None,
                rx_bytes: 0,
                tx_bytes: 0,
            });
        }
        Ok(device.clone())
    }

    async fn remove_peer(&self, name: &str, public_key: &str) -> Result<KernelDeviceSnapshot> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let mut devices = self.devices.lock().await;
        let device = devices
            .get_mut(name)
            .context("fixture interface is absent")?;
        device.peers.retain(|peer| peer.public_key != public_key);
        Ok(device.clone())
    }

    async fn configure_link(
        &self,
        name: &str,
        addresses: &[(IpAddr, u8)],
        mtu: Option<u32>,
    ) -> Result<KernelDeviceSnapshot> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let mut devices = self.devices.lock().await;
        let device = devices
            .get_mut(name)
            .context("fixture interface is absent")?;
        device.addresses = addresses.to_vec();
        device.mtu = mtu;
        device.admin_up = true;
        Ok(device.clone())
    }

    async fn get_device(&self, name: &str) -> Result<Option<KernelDeviceSnapshot>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(self.devices.lock().await.get(name).cloned())
    }
}

#[derive(Clone)]
struct CloudflareFixture {
    device: Arc<Mutex<Value>>,
    requests: Arc<AtomicUsize>,
}

impl CloudflareFixture {
    fn source(public_key: &str, peer_public_key: &str) -> Value {
        json!({
            "id":"device-1", "enabled":true, "name":"decoy", "model":"PC",
            "type":"Android", "created":"2026-01-01T00:00:00Z",
            "updated":"2026-01-01T00:00:00Z", "warp_enabled":true,
            "key":public_key,
            "account":{
                "id":"account-1", "account_type":"free", "role":"child",
                "warp_plus":false, "quota":0, "premium_data":0,
                "referral_count":0, "referral_renewal_countdown":0,
                "created":"2026-01-01T00:00:00Z",
                "updated":"2026-01-01T00:00:00Z", "license":FIXTURE_LICENSE
            },
            "config":{
                "client_id":"fixture-client",
                "interface":{"addresses":{"v4":"172.16.0.2","v6":"2606:4700::2"}},
                "peers":[{
                    "endpoint":{
                        "host":"engage.cloudflareclient.com:2408",
                        "v4":"162.159.192.1:2408",
                        "v6":"[2606:4700:d0::1]:2408"
                    },
                    "public_key":peer_public_key
                }]
            }
        })
    }
}

async fn cloudflare_gateway(
    State(state): State<CloudflareFixture>,
    request: Request,
) -> impl IntoResponse {
    state.requests.fetch_add(1, Ordering::SeqCst);
    let method = request.method().clone();
    let path = request.uri().path().to_owned();
    let authorized = request
        .headers()
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        == Some("Bearer fixture-private-token");
    let bytes = match to_bytes(request.into_body(), 1024 * 1024).await {
        Ok(bytes) => bytes,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error":"invalid body"})),
            )
        }
    };
    let body: Value = if bytes.is_empty() {
        json!({})
    } else {
        match serde_json::from_slice(&bytes) {
            Ok(body) => body,
            Err(_) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"error":"invalid json"})),
                )
            }
        }
    };

    let response = match (method, path.as_str()) {
        (Method::POST, "/v0a1922/reg") => {
            let Some(public_key) = body.get("key").and_then(Value::as_str) else {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"error":"missing key"})),
                );
            };
            let mut device = state.device.lock().await;
            device["key"] = json!(public_key);
            device["model"] = body.get("model").cloned().unwrap_or(json!("PC"));
            let mut registration = device.clone();
            registration["token"] = json!(FIXTURE_TOKEN);
            registration
        }
        (Method::GET, "/v0a1922/reg/device-1") if authorized => state.device.lock().await.clone(),
        (Method::PATCH, "/v0a1922/reg/device-1/account/reg/device-1") if authorized => {
            let Some(name) = body.get("name").and_then(Value::as_str) else {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"error":"missing name"})),
                );
            };
            state.device.lock().await["name"] = json!(name);
            json!([])
        }
        (Method::PUT, "/v0a1922/reg/device-1/account") if authorized => {
            let Some(license) = body.get("license").and_then(Value::as_str) else {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"error":"missing license"})),
                );
            };
            state.device.lock().await["account"]["license"] = json!(license);
            json!({})
        }
        (Method::PATCH, "/v0a1922/reg/device-1") if authorized => {
            let Some(public_key) = body.get("key").and_then(Value::as_str) else {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"error":"missing key"})),
                );
            };
            let mut device = state.device.lock().await;
            device["key"] = json!(public_key);
            device.clone()
        }
        _ => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error":"unadmitted fixture request"})),
            )
        }
    };
    (StatusCode::OK, Json(response))
}

struct TlsFixture {
    _root: tempfile::TempDir,
    ca: PathBuf,
    bridge_certificate: PathBuf,
    bridge_key: PathBuf,
    server: Arc<rustls::ServerConfig>,
    bridge_pin: [u8; 32],
}

fn write_file(path: &Path, contents: &str, mode: u32) {
    std::fs::write(path, contents).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
}

fn tls_fixture() -> TlsFixture {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    let root = tempfile::tempdir().unwrap();
    let ca_key = KeyPair::generate().unwrap();
    let mut ca_params = CertificateParams::new(Vec::<String>::new()).unwrap();
    ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    let ca = ca_params.self_signed(&ca_key).unwrap();
    let server_key = KeyPair::generate().unwrap();
    let server = CertificateParams::new(vec!["decoy.fixture".into()])
        .unwrap()
        .signed_by(&server_key, &ca, &ca_key)
        .unwrap();
    let bridge_key = KeyPair::generate().unwrap();
    let bridge = CertificateParams::new(vec!["bridge.fixture".into()])
        .unwrap()
        .signed_by(&bridge_key, &ca, &ca_key)
        .unwrap();
    let ca_path = root.path().join("ca.pem");
    let bridge_certificate = root.path().join("bridge.pem");
    let bridge_key_path = root.path().join("bridge-key.pem");
    write_file(&ca_path, &ca.pem(), 0o644);
    write_file(&bridge_certificate, &bridge.pem(), 0o644);
    write_file(&bridge_key_path, &bridge_key.serialize_pem(), 0o600);
    let server_config = op_network::dbus_tls::server_config(
        vec![ca.der().clone()],
        vec![server.der().clone()],
        PrivateKeyDer::Pkcs8(server_key.serialize_der().into()),
    )
    .unwrap();
    TlsFixture {
        _root: root,
        ca: ca_path,
        bridge_certificate,
        bridge_key: bridge_key_path,
        server: server_config,
        bridge_pin: Sha256::digest(bridge.der().as_ref()).into(),
    }
}

fn validate(schema: &impl serde::Serialize, value: &Value) {
    let schema = serde_json::to_value(schema).unwrap();
    jsonschema::Validator::new(&schema)
        .unwrap()
        .validate(value)
        .unwrap();
}

fn read_and_validate_state(schema: &PluginSchema) -> Value {
    let bytes = op_core::projection_shm::read_projection_bytes(&schema.name)
        .unwrap_or_else(|| panic!("{} SHM projection is absent", schema.name));
    let state: Value = serde_json::from_slice(&bytes).unwrap();
    let mut exact = bytes;
    schema
        .validate_exact_schema_fields(&simd_json::to_owned_value(&mut exact).unwrap())
        .unwrap();
    state
}

async fn call(
    engine: &crate::mutation_engine::MutationEngine,
    schema: &PluginSchema,
    method: &str,
    arguments: Value,
    principal: &str,
    session: &str,
    genesis: &str,
) -> Value {
    let declaration = &schema.methods[method];
    validate(&declaration.args, &arguments);
    let response = engine
        .dispatch_method_call_with_identity(
            &schema.name,
            method,
            &arguments.to_string(),
            declaration.required_capability.as_deref(),
            principal,
            Some(session),
            Some(genesis),
        )
        .await
        .unwrap_or_else(|error| panic!("{}.{method}: {error:#}", schema.name));
    validate(declaration.returns.as_ref().unwrap(), &response["result"]);
    response
}

#[tokio::test]
async fn all_sixteen_methods_cross_canonical_pinned_mtls_and_commit_complete_siblings() {
    let _cozo = temp_cozo();
    let wireguard_schema = WireGuardPlugin::new().schema().unwrap();
    let wg_opdbus_schema = WgOpdbusPlugin::new().schema().unwrap();
    let wgcf_schema = WgcfPlugin::new(WgcfConfig::default()).schema().unwrap();
    assert_eq!(
        wireguard_schema.methods.len() + wg_opdbus_schema.methods.len() + wgcf_schema.methods.len(),
        16
    );
    let _shm = temp_shm_with_schemas(&[
        HumanPrincipalPlugin::new().schema().unwrap(),
        IdentitySledPlugin::new().schema().unwrap(),
        wireguard_schema.clone(),
        wg_opdbus_schema.clone(),
        wgcf_schema.clone(),
    ]);
    let engine = test_engine();
    engine
        .install_identity_sled_store_for_test(IdentitySledStore::new(
            op_cozo_store::CozoGraphShuttle::new_in_memory().unwrap(),
        ))
        .unwrap();
    let public_key = pk(0xD3);
    register(&public_key, "decoy-canonical-fixture")
        .await
        .unwrap();
    let identity = write_identity(&engine, &public_key).await.unwrap();
    let principal = op_identity::session::derive_principal_id(&public_key);
    let genesis = identity.genesis.as_deref().unwrap().to_owned();
    let minted_sid1 = identity
        .sealed_id
        .clone()
        .expect("identity owner minted SID1");
    let minted_wire = URL_SAFE_NO_PAD
        .decode(
            minted_sid1
                .strip_prefix(op_identity::sealed_id::INLINE_PREFIX)
                .expect("identity owner stored an inline SID1"),
        )
        .unwrap();
    let minted_claims = op_identity::sealed_id::SealedId::open(&minted_wire).unwrap();
    assert_eq!(minted_claims.principal_id, principal);
    assert_eq!(minted_claims.session_id, identity.session_id);
    assert_eq!(minted_claims.session_genesis, genesis);

    let kernel = Arc::new(MemoryKernel::default());
    let wireguard = Arc::new(
        WireGuardOwner::new(
            DecoyRole::Decoy,
            [INTERFACE_NAME.to_owned(), WGCF_INTERFACE.to_owned()],
            kernel.clone(),
        )
        .unwrap(),
    );
    let peer =
        <LinuxKernelWireGuard as KernelWireGuard>::generate_keypair(&LinuxKernelWireGuard).unwrap();
    let cloudflare = CloudflareFixture {
        device: Arc::new(Mutex::new(CloudflareFixture::source(
            peer.public_key(),
            peer.public_key(),
        ))),
        requests: Arc::new(AtomicUsize::new(0)),
    };
    let http_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let http_endpoint = format!("http://{}", http_listener.local_addr().unwrap());
    let http_state = cloudflare.clone();
    let http_task = tokio::spawn(async move {
        axum::serve(
            http_listener,
            Router::new()
                .fallback(any(cloudflare_gateway))
                .with_state(http_state),
        )
        .await
        .unwrap();
    });
    let documents = Arc::new(RuntimeDocuments::new(
        op_cozo_store::CozoGraphShuttle::new_in_memory().unwrap(),
    ));
    let wgcf = WgcfOwner::new(documents, wireguard.clone(), http_endpoint, true).unwrap();
    let backend = Arc::new(ProductionBackend {
        wireguard: wireguard.clone(),
        wgcf,
    });
    let catalog = crate::sealed_schema_reader::blob_catalog_dir_from_env();
    let service = DecoyNativeService::with_catalog(backend, catalog);

    let tls = tls_fixture();
    let probe = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let decoy_address = probe.local_addr().unwrap();
    drop(probe);
    let decoy_task = tokio::spawn(serve(
        decoy_address,
        tls.server.clone(),
        tls.bridge_pin,
        service,
    ));
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            if tokio::net::TcpStream::connect(decoy_address).await.is_ok() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("decoy fixture listener became ready");

    let fixture_root = tempfile::tempdir().unwrap();
    let grants = fixture_root.path().join("grants.json");
    let _environment = Environment::capture([
        "OP_GRANTS_PATH",
        "OP_DECOY_NATIVE_ADDRESS",
        "OP_DECOY_NATIVE_SERVER_NAME",
        "OP_DECOY_NATIVE_CA_FILE",
        "OP_DECOY_NATIVE_CERT_FILE",
        "OP_DECOY_NATIVE_KEY_FILE",
        "OP_DECOY_NATIVE_BRIDGE_CERT_FILE",
    ]);
    // FIXME: Audit that the environment access only happens in single-threaded code.
    unsafe { std::env::set_var("OP_GRANTS_PATH", &grants) };
    // FIXME: Audit that the environment access only happens in single-threaded code.
    unsafe { std::env::set_var("OP_DECOY_NATIVE_ADDRESS", decoy_address.to_string()) };
    // FIXME: Audit that the environment access only happens in single-threaded code.
    unsafe { std::env::set_var("OP_DECOY_NATIVE_SERVER_NAME", "decoy.fixture") };
    // FIXME: Audit that the environment access only happens in single-threaded code.
    unsafe { std::env::set_var("OP_DECOY_NATIVE_CA_FILE", &tls.ca) };
    // FIXME: Audit that the environment access only happens in single-threaded code.
    unsafe { std::env::set_var("OP_DECOY_NATIVE_CERT_FILE", &tls.bridge_certificate) };
    // FIXME: Audit that the environment access only happens in single-threaded code.
    unsafe { std::env::set_var("OP_DECOY_NATIVE_KEY_FILE", &tls.bridge_key) };
    // FIXME: Audit that the environment access only happens in single-threaded code.
    unsafe { std::env::set_var("OP_DECOY_NATIVE_BRIDGE_CERT_FILE", &tls.bridge_certificate) };
    let capabilities = [&wireguard_schema, &wg_opdbus_schema, &wgcf_schema]
        .into_iter()
        .flat_map(|schema| schema.methods.values())
        .filter_map(|method| method.required_capability.clone())
        .collect::<BTreeSet<_>>();
    std::fs::write(
        &grants,
        serde_json::to_vec(&json!({
            (principal.clone()): {"capabilities": capabilities}
        }))
        .unwrap(),
    )
    .unwrap();

    let device_pair =
        <LinuxKernelWireGuard as KernelWireGuard>::generate_keypair(&LinuxKernelWireGuard).unwrap();
    let first_peer =
        <LinuxKernelWireGuard as KernelWireGuard>::generate_keypair(&LinuxKernelWireGuard).unwrap();
    let second_peer =
        <LinuxKernelWireGuard as KernelWireGuard>::generate_keypair(&LinuxKernelWireGuard).unwrap();
    let preshared =
        <LinuxKernelWireGuard as KernelWireGuard>::generate_keypair(&LinuxKernelWireGuard).unwrap();
    let secrets = [
        device_pair.private_key().to_owned(),
        preshared.private_key().to_owned(),
        FIXTURE_TOKEN.to_owned(),
        FIXTURE_LICENSE.to_owned(),
    ];

    let mut calls = 0usize;
    let wireguard_calls = [
        (
            "set_device",
            json!({
                "interface":INTERFACE_NAME, "private_key":device_pair.private_key(),
                "listen_port":51820, "fwmark":73
            }),
        ),
        ("get_device", json!({"interface":INTERFACE_NAME})),
        (
            "add_peer",
            json!({
                "interface":INTERFACE_NAME, "public_key":first_peer.public_key(),
                "preshared_key":preshared.private_key(), "endpoint":"192.0.2.9:51820",
                "allowed_ips":["10.8.0.9/32"], "persistent_keepalive":25
            }),
        ),
        ("list_peers", json!({"interface":INTERFACE_NAME})),
        (
            "set_config",
            json!({
                "interface":INTERFACE_NAME,
                "config":{
                    "private_key":device_pair.private_key(), "listen_port":51821,
                    "fwmark":74, "peers":[{
                        "public_key":second_peer.public_key(),
                        "allowed_ips":["2001:db8::9/128"]
                    }]
                }
            }),
        ),
        (
            "remove_peer",
            json!({"interface":INTERFACE_NAME,"public_key":second_peer.public_key()}),
        ),
    ];
    for (method, arguments) in wireguard_calls {
        let response = call(
            &engine,
            &wireguard_schema,
            method,
            arguments,
            &principal,
            &identity.session_id,
            &genesis,
        )
        .await;
        calls += 1;
        match method {
            "set_device" => {
                assert_eq!(response["result"]["interface"]["listen_port"], 51820);
                assert_eq!(response["result"]["interface"]["fwmark"], 73);
            }
            "get_device" => {
                assert_eq!(response["result"]["interface"]["listen_port"], 51820)
            }
            "add_peer" => {
                assert_eq!(response["result"]["changed"], true);
                assert_eq!(
                    response["result"]["peer"]["public_key"],
                    first_peer.public_key()
                );
            }
            "list_peers" => {
                assert_eq!(response["result"]["peers"].as_array().unwrap().len(), 1);
                assert_eq!(
                    response["result"]["peers"][0]["public_key"],
                    first_peer.public_key()
                );
            }
            "set_config" => {
                assert_eq!(response["result"]["interface"]["listen_port"], 51821);
                assert_eq!(
                    response["result"]["interface"]["peers"][0]["public_key"],
                    second_peer.public_key()
                );
            }
            "remove_peer" => {
                assert_eq!(response["result"]["changed"], true);
                assert!(response["result"]["interface"]["peers"]
                    .as_array()
                    .unwrap()
                    .is_empty());
            }
            _ => unreachable!("fixture contains all wireguard methods"),
        }
        let wire_state = read_and_validate_state(&wireguard_schema);
        let facade_state = read_and_validate_state(&wg_opdbus_schema);
        assert_eq!(facade_state["interfaces"].as_object().unwrap().len(), 2);
        assert!(facade_state["interfaces"].get(INTERFACE_NAME).is_some());
        assert!(facade_state["interfaces"].get(WGCF_INTERFACE).is_some());
        let public = format!("{response}{wire_state}{facade_state}");
        for secret in &secrets {
            assert!(!public.contains(secret));
        }
    }

    for (method, arguments) in [
        (
            "create_interface",
            json!({
                "interface_name":INTERFACE_NAME,
                "addresses":["10.8.0.1/24","2001:db8:8::1/64"], "mtu":1420,
                "device":{
                    "private_key":device_pair.private_key(), "listen_port":51822,
                    "fwmark":75, "peers":[]
                }
            }),
        ),
        ("get_status", json!({"interface_name":INTERFACE_NAME})),
    ] {
        let response = call(
            &engine,
            &wg_opdbus_schema,
            method,
            arguments,
            &principal,
            &identity.session_id,
            &genesis,
        )
        .await;
        calls += 1;
        if method == "create_interface" {
            assert_eq!(response["result"]["status"]["admin_up"], true);
            assert_eq!(response["result"]["status"]["mtu"], 1420);
            assert_eq!(
                response["result"]["status"]["addresses"],
                json!(["10.8.0.1/24", "2001:db8:8::1/64"])
            );
        } else {
            assert_eq!(response["result"]["admin_up"], true);
            assert_eq!(response["result"]["mtu"], 1420);
        }
        let wire_state = read_and_validate_state(&wireguard_schema);
        let facade_state = read_and_validate_state(&wg_opdbus_schema);
        assert_eq!(facade_state["interfaces"].as_object().unwrap().len(), 2);
        assert_eq!(
            facade_state["service_persona"]["principal_id"],
            op_plugins::state_plugins::wg_opdbus::ANNA_SCRIBE_PRINCIPAL_ID
        );
        let public = format!("{response}{wire_state}{facade_state}");
        for secret in &secrets {
            assert!(!public.contains(secret));
        }
    }

    let mut profile_id = None;
    let mut registered_wgcf_key = None;
    let wgcf_calls = [
        (
            "register",
            json!({"accept_tos":true,"device_model":"canonical-fixture"}),
        ),
        (
            "update",
            json!({"license":null,"device_name":"canonical-decoy"}),
        ),
        ("status", json!({})),
        ("rotate_keys", json!({})),
        ("set_endpoint", json!({"endpoint":"162.159.193.1:2408"})),
        (
            "generate_config",
            json!({"interface":WGCF_INTERFACE,"dns":[],"mtu":1280}),
        ),
    ];
    for (method, arguments) in wgcf_calls {
        let response = call(
            &engine,
            &wgcf_schema,
            method,
            arguments,
            &principal,
            &identity.session_id,
            &genesis,
        )
        .await;
        calls += 1;
        match method {
            "register" => {
                assert_eq!(response["result"]["device"]["name"], "decoy");
                registered_wgcf_key = response["result"]["device"]["public_key"]
                    .as_str()
                    .map(str::to_owned);
            }
            "update" | "status" => {
                assert_eq!(response["result"]["device"]["name"], "canonical-decoy")
            }
            "rotate_keys" => {
                assert_ne!(
                    response["result"]["device"]["public_key"].as_str(),
                    registered_wgcf_key.as_deref()
                );
            }
            "set_endpoint" => assert_eq!(
                response["result"]["profile"]["endpoint"],
                "162.159.193.1:2408"
            ),
            "generate_config" => {
                assert_eq!(response["result"]["profile"]["mtu"], 1280);
                assert_eq!(response["result"]["profile"]["interface"], WGCF_INTERFACE);
            }
            _ => unreachable!("fixture contains the first six wgcf methods"),
        }
        if matches!(method, "set_endpoint" | "generate_config") {
            profile_id = response["result"]["profile"]["profile_id"]
                .as_str()
                .map(str::to_owned);
        }
        let wgcf_state = read_and_validate_state(&wgcf_schema);
        let wire_state = read_and_validate_state(&wireguard_schema);
        let facade_state = read_and_validate_state(&wg_opdbus_schema);
        assert_eq!(facade_state["interfaces"].as_object().unwrap().len(), 2);
        assert!(wire_state["interfaces"].is_array());
        let public = format!("{response}{wgcf_state}{wire_state}{facade_state}");
        for secret in &secrets {
            assert!(!public.contains(secret));
        }
    }
    let profile_id = profile_id.expect("generated WGCF profile id");
    for (method, arguments) in [
        ("apply_config", json!({"profile_id":profile_id})),
        ("refresh", json!({"kind":"all"})),
    ] {
        let response = call(
            &engine,
            &wgcf_schema,
            method,
            arguments,
            &principal,
            &identity.session_id,
            &genesis,
        )
        .await;
        calls += 1;
        if method == "apply_config" {
            assert!(response["result"]["kernel"]["interfaces"]
                .as_array()
                .unwrap()
                .iter()
                .any(|interface| interface["name"] == WGCF_INTERFACE));
            assert_eq!(response["result"]["profile"]["profile_id"], profile_id);
        } else {
            assert_eq!(
                response["result"]["active_profile"]["profile_id"],
                profile_id
            );
            assert!(response["result"]["kernel"]["interfaces"].is_array());
        }
        let wgcf_state = read_and_validate_state(&wgcf_schema);
        let wire_state = read_and_validate_state(&wireguard_schema);
        let facade_state = read_and_validate_state(&wg_opdbus_schema);
        assert_eq!(facade_state["interfaces"].as_object().unwrap().len(), 2);
        assert!(facade_state["interfaces"][WGCF_INTERFACE]["interface"].is_object());
        assert!(wire_state["interfaces"]
            .as_array()
            .unwrap()
            .iter()
            .any(|interface| interface["name"] == WGCF_INTERFACE));
        let public = format!("{response}{wgcf_state}{wire_state}{facade_state}");
        for secret in &secrets {
            assert!(!public.contains(secret));
        }
    }
    assert_eq!(calls, 16);
    assert_eq!(cloudflare.requests.load(Ordering::SeqCst), 7);

    let before_kernel = kernel.count();
    let before_http = cloudflare.requests.load(Ordering::SeqCst);
    let before_state = op_core::projection_shm::read_projection_bytes("wireguard").unwrap();
    let denied = engine
        .dispatch_method_call_with_identity(
            "wireguard",
            "set_device",
            &json!({"interface":INTERFACE_NAME,"listen_port":51999}).to_string(),
            wireguard_schema.methods["set_device"]
                .required_capability
                .as_deref(),
            &principal,
            None,
            None,
        )
        .await;
    assert!(denied.is_err());
    assert_eq!(kernel.count(), before_kernel);
    assert_eq!(cloudflare.requests.load(Ordering::SeqCst), before_http);
    assert_eq!(
        op_core::projection_shm::read_projection_bytes("wireguard").unwrap(),
        before_state
    );
    let retained = crate::identity_sled_dispatch::stored_session(&engine, &identity.session_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(retained.sealed_id.as_deref(), Some(minted_sid1.as_str()));

    decoy_task.abort();
    http_task.abort();
}
