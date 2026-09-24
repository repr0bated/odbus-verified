//! Admitted network operations cross a mutually authenticated native D-Bus
//! connection to the decoy. The server bridge never creates a WireGuard owner.
//! SID1 bytes come from the identity owner's existing private credential, not
//! reconstructed claims. The decoy requires both its private transport CA and
//! the configured bridge leaf-certificate SHA-256 pin; a SID1 integrity seal
//! alone is deliberately NOT remote authentication.
use anyhow::{ensure, Context, Result};
use async_trait::async_trait;
use base64::Engine as _;
use op_network::dbus_tls::TlsDbusSocket;
use op_state_store::PluginSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    net::SocketAddr,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::sync::Mutex;
use zbus::connection::AuthMechanism;

const INTERFACE: &str = "org.opdbus.v1.DecoyNative";
const OBJECT_PATH: &str = "/org/opdbus/v1/native/decoy";
const MAX_ARGUMENT_BYTES: usize = 1024 * 1024;

#[derive(Serialize, Deserialize)]
pub struct NativeNetworkReceipt {
    pub output: Value,
    pub states: BTreeMap<String, Value>,
    #[serde(default)]
    pub state_schema_hashes: BTreeMap<String, String>,
}

/// Transport metadata kept separate from schema-governed method arguments.
/// `sealed_id` is the exact identity-owner-authored SID1 byte string.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, zbus::zvariant::Type)]
pub struct DecoyCallMetadata {
    pub capability: String,
    pub schema_hash: String,
    pub sealed_id: Vec<u8>,
}

#[async_trait]
pub trait DecoyNetworkBackend: Send + Sync {
    async fn dispatch(
        &self,
        plugin: &str,
        method: &str,
        arguments: Value,
    ) -> Result<NativeNetworkReceipt>;
}

/// Only the decoy binary constructs this with native kernel/account owners.
/// TLS accept happens before the service is registered on a peer connection.
#[derive(Clone)]
pub struct DecoyNativeService {
    backend: Arc<dyn DecoyNetworkBackend>,
    operation: Arc<Mutex<()>>,
    catalog_dir: PathBuf,
}

impl DecoyNativeService {
    pub fn new(role: &str, backend: Arc<dyn DecoyNetworkBackend>) -> Result<Self> {
        ensure!(
            role == "decoy",
            "native WireGuard owner is restricted to the decoy role"
        );
        Ok(Self {
            backend,
            operation: Arc::new(Mutex::new(())),
            catalog_dir: crate::sealed_schema_reader::blob_catalog_dir_from_env(),
        })
    }

    #[cfg(test)]
    fn with_catalog(backend: Arc<dyn DecoyNetworkBackend>, catalog_dir: PathBuf) -> Self {
        Self {
            backend,
            operation: Arc::new(Mutex::new(())),
            catalog_dir,
        }
    }

    async fn call_owned(
        &self,
        plugin: String,
        method: String,
        arguments: String,
        metadata: DecoyCallMetadata,
    ) -> Result<String> {
        ensure!(
            matches!(plugin.as_str(), "wireguard" | "wg_opdbus" | "wgcf"),
            "plugin is not a decoy network capability"
        );
        ensure!(
            arguments.len() <= MAX_ARGUMENT_BYTES,
            "decoy method arguments exceed limit"
        );
        let _operation = self.operation.lock().await;
        let claims = op_identity::sealed_id::SealedId::open(&metadata.sealed_id)?;
        ensure!(
            claims.principal_kind == "wireguard-principal"
                && claims
                    .transport_scope
                    .split(',')
                    .any(|scope| scope.trim() == "dbus"),
            "identity does not permit D-Bus transport"
        );
        ensure!(
            claims.expires_at == 0 || claims.expires_at > chrono::Utc::now().timestamp(),
            "forwarded identity expired while queued"
        );
        ensure!(
            claims.session_id == op_identity::session::derive_session_id(&claims.wireguard_pubkey)
                && claims.principal_id
                    == op_identity::session::derive_principal_id(&claims.wireguard_pubkey),
            "forwarded identity binding is invalid"
        );
        let pinned = crate::sealed_schema_reader::read_manifest_pinned_schema_document(
            &self.catalog_dir,
            &plugin,
        )?;
        ensure!(
            pinned.schema_hash == metadata.schema_hash,
            "bridge and decoy plugin seals differ"
        );
        crate::method_contract_admission::admit_compiled_state_contract(&pinned, &plugin)?;
        let declaration = crate::method_contract_admission::admit_pinned_compiled_method(
            &pinned,
            &plugin,
            &method,
            Some(&metadata.capability),
        )?;
        let arguments: Value = serde_json::from_str(&arguments)?;
        crate::method_contract_admission::validate_admitted_arguments(&declaration, &arguments)?;
        let mut receipt = self.backend.dispatch(&plugin, &method, arguments).await?;
        ensure!(
            receipt.states.contains_key(&plugin),
            "native owner omitted the called plugin state"
        );
        ensure!(
            !contains_secret(&receipt.output)
                && receipt.states.values().all(|state| !contains_secret(state)),
            "native owner attempted to return credential material"
        );
        if let Some(output) = declaration.returns {
            jsonschema::Validator::new(&serde_json::to_value(output)?)?
                .validate(&receipt.output)
                .map_err(|_| {
                    anyhow::anyhow!("native method output does not satisfy the sealed contract")
                })?;
        }
        for state_plugin in receipt.states.keys() {
            ensure!(
                matches!(state_plugin.as_str(), "wireguard" | "wg_opdbus" | "wgcf"),
                "native owner returned an unrelated projection"
            );
            let pin = crate::sealed_schema_reader::read_manifest_pinned_schema_document(
                &self.catalog_dir,
                state_plugin,
            )?;
            crate::method_contract_admission::admit_compiled_state_contract(&pin, state_plugin)?;
            let state = receipt
                .states
                .get(state_plugin)
                .context("native owner state disappeared during validation")?;
            let state = simd_json::serde::to_owned_value(state)?;
            pin.schema
                .validate_exact_schema_fields(&state)
                .context("native owner state does not satisfy the sealed exact schema")?;
            receipt
                .state_schema_hashes
                .insert(state_plugin.clone(), pin.schema_hash);
        }
        // The trusted bridge remains the sole MutationEngine/SHM publisher.
        // The decoy returns observations, never a second schema/state catalog.
        serde_json::to_string(&receipt).map_err(Into::into)
    }
}

#[zbus::interface(name = "org.opdbus.v1.DecoyNative")]
impl DecoyNativeService {
    async fn call(
        &self,
        plugin: String,
        method: String,
        arguments: String,
        metadata: DecoyCallMetadata,
    ) -> zbus::fdo::Result<String> {
        let service = self.clone();
        // Keep native effects/readback alive when the calling transport closes.
        tokio::spawn(async move {
            service
                .call_owned(plugin, method, arguments, metadata)
                .await
        })
        .await
        .map_err(|_| zbus::fdo::Error::Failed("decoy operation owner failed".into()))?
        .map_err(|error| zbus::fdo::Error::Failed(error.to_string()))
    }
}

fn contains_secret(value: &Value) -> bool {
    match value {
        Value::Object(fields) => fields.iter().any(|(key, value)| {
            let normalized: String = key
                .chars()
                .filter(|character| character.is_ascii_alphanumeric())
                .flat_map(char::to_lowercase)
                .collect();
            matches!(
                normalized.as_str(),
                "privatekey"
                    | "presharedkey"
                    | "accesstoken"
                    | "refreshtoken"
                    | "token"
                    | "licensekey"
                    | "sealedid"
                    | "apikey"
                    | "clientsecret"
                    | "password"
                    | "authorization"
                    | "bearertoken"
                    | "credentialblob"
                    | "secretkey"
            ) || normalized.ends_with("password")
                || normalized.ends_with("secret")
                || contains_secret(value)
        }),
        Value::Array(values) => values.iter().any(contains_secret),
        _ => false,
    }
}

pub struct TransportFiles {
    pub ca: PathBuf,
    pub certificate: PathBuf,
    pub key: PathBuf,
}

impl TransportFiles {
    fn required(name: &str) -> Result<PathBuf> {
        std::env::var_os(name)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .with_context(|| format!("{name} is required for the decoy transport"))
    }

    pub fn from_env() -> Result<Self> {
        Ok(Self {
            ca: Self::required("OP_DECOY_NATIVE_CA_FILE")?,
            certificate: Self::required("OP_DECOY_NATIVE_CERT_FILE")?,
            key: Self::required("OP_DECOY_NATIVE_KEY_FILE")?,
        })
    }

    fn read_protected(
        path: &Path,
        max_len: u64,
        forbidden_mode: u32,
        purpose: &str,
    ) -> Result<Vec<u8>> {
        use std::os::unix::fs::OpenOptionsExt;
        let file = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(nix::libc::O_NOFOLLOW)
            .open(path)
            .with_context(|| format!("{purpose} is unavailable"))?;
        let metadata = file.metadata()?;
        ensure!(
            metadata.is_file()
                && metadata.nlink() == 1
                && metadata.uid() == nix::unistd::geteuid().as_raw()
                && metadata.mode() & forbidden_mode == 0
                && metadata.len() <= max_len,
            "{purpose} must be a service-owned, non-writable regular file within its size limit"
        );
        let mut bytes = Vec::with_capacity(metadata.len() as usize);
        std::io::Read::read_to_end(&mut std::io::Read::take(file, max_len + 1), &mut bytes)?;
        ensure!(
            bytes.len() as u64 <= max_len,
            "{purpose} exceeds its size limit"
        );
        Ok(bytes)
    }

    fn read_certificates(path: &Path) -> Result<Vec<rustls::pki_types::CertificateDer<'static>>> {
        let file = std::fs::File::open(path).context("decoy transport certificate unavailable")?;
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut std::io::Read::take(file, 1024 * 1024 + 1), &mut bytes)?;
        ensure!(
            bytes.len() <= 1024 * 1024,
            "decoy certificate file exceeds limit"
        );
        Ok(rustls_pemfile::certs(&mut bytes.as_slice())
            .collect::<std::result::Result<Vec<_>, _>>()?)
    }

    fn read_key(&self) -> Result<rustls::pki_types::PrivateKeyDer<'static>> {
        let bytes = Self::read_protected(&self.key, 65_536, 0o077, "decoy transport private key")?;
        rustls_pemfile::private_key(&mut bytes.as_slice())?
            .context("decoy key contains no private key")
    }

    /// SHA-256 pin of the one allowed bridge leaf certificate.
    pub fn bridge_certificate_pin() -> Result<[u8; 32]> {
        let path = Self::required("OP_DECOY_NATIVE_BRIDGE_CERT_FILE")?;
        let bytes = Self::read_protected(&path, 1024 * 1024, 0o022, "bridge pin certificate")?;
        let certificates = rustls_pemfile::certs(&mut bytes.as_slice())
            .collect::<std::result::Result<Vec<_>, _>>()?;
        ensure!(
            certificates.len() == 1,
            "bridge pin file must contain exactly one leaf certificate"
        );
        Ok(Sha256::digest(certificates[0].as_ref()).into())
    }

    pub fn client_config(&self) -> Result<Arc<rustls::ClientConfig>> {
        op_network::dbus_tls::client_config(
            Self::read_certificates(&self.ca)?,
            Self::read_certificates(&self.certificate)?,
            self.read_key()?,
        )
    }
    pub fn server_config(&self) -> Result<Arc<rustls::ServerConfig>> {
        op_network::dbus_tls::server_config(
            Self::read_certificates(&self.ca)?,
            Self::read_certificates(&self.certificate)?,
            self.read_key()?,
        )
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn dispatch(
    engine: &crate::mutation_engine::MutationEngine,
    plugin: &'static str,
    method: &str,
    arguments: &Value,
    admitted: &PluginSchema,
    caller: &crate::tched_router_admission::RouterCaller,
) -> Result<NativeNetworkReceipt> {
    let pinned =
        crate::tched_router_admission::readmit_call(engine, admitted, plugin, method, caller)
            .await?;
    let session = caller
        .session
        .as_deref()
        .context("decoy call requires verified session")?;
    let sled = crate::identity_sled_dispatch::stored_session(engine, session)
        .await?
        .context("identity owner has no session credential")?;
    let inline = sled
        .sealed_id
        .as_deref()
        .context("identity owner has not authored SID1")?;
    let encoded = inline
        .strip_prefix(op_identity::sealed_id::INLINE_PREFIX)
        .context("identity credential is not SID1")?;
    ensure!(encoded.len() <= 24 * 1024, "stored SID1 exceeds wire limit");
    let identity = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(encoded)?;
    ensure!(
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&identity) == encoded,
        "stored SID1 base64 is not canonical"
    );
    let claims = crate::identity_sled_dispatch::authenticate_sid1_wire(engine, &identity, "dbus")
        .await
        .map_err(anyhow::Error::msg)?;
    ensure!(
        claims.principal_id == caller.actor
            && claims.session_genesis == caller.genesis.as_deref().unwrap_or(""),
        "decoy metadata differs from the verified caller"
    );
    let address: SocketAddr = std::env::var("OP_DECOY_NATIVE_ADDRESS")
        .context("OP_DECOY_NATIVE_ADDRESS is required; server has no local WireGuard fallback")?
        .parse()?;
    let name = std::env::var("OP_DECOY_NATIVE_SERVER_NAME")
        .context("OP_DECOY_NATIVE_SERVER_NAME is required")?;
    let tls = tokio::task::spawn_blocking(|| TransportFiles::from_env()?.client_config()).await??;
    let socket = TlsDbusSocket::connect(address, name, tls).await?;
    let connection_builder = zbus::connection::Builder::socket(socket)
        .p2p()
        .auth_mechanism(AuthMechanism::Anonymous);
    let connection = tokio::time::timeout(Duration::from_secs(10), connection_builder.build())
        .await
        .context("decoy D-Bus negotiation timed out")??;
    let proxy = tokio::time::timeout(
        Duration::from_secs(10),
        zbus::Proxy::new(&connection, INTERFACE, OBJECT_PATH, INTERFACE),
    )
    .await
    .context("decoy D-Bus proxy negotiation timed out")??;
    let capability = caller
        .capability
        .as_deref()
        .context("decoy method capability is required")?;
    let result: String = tokio::time::timeout(
        Duration::from_secs(60),
        proxy.call(
            "Call",
            &(
                plugin,
                method,
                serde_json::to_string(arguments)?,
                DecoyCallMetadata {
                    capability: capability.to_string(),
                    schema_hash: pinned.schema_hash,
                    sealed_id: identity,
                },
            ),
        ),
    )
    .await
    .context("decoy method timed out; reconcile before retry")??;
    ensure!(
        result.len() <= 8 * 1024 * 1024,
        "decoy receipt exceeds limit"
    );
    let receipt: NativeNetworkReceipt =
        serde_json::from_str(&result).context("invalid native decoy receipt")?;
    ensure!(
        receipt.states.contains_key(plugin),
        "decoy omitted the called plugin state"
    );
    for state_plugin in receipt.states.keys() {
        ensure!(
            matches!(state_plugin.as_str(), "wireguard" | "wg_opdbus" | "wgcf"),
            "decoy returned an unrelated state"
        );
        let pin = crate::sealed_schema_reader::read_manifest_pinned_schema_document(
            &crate::sealed_schema_reader::blob_catalog_dir_from_env(),
            state_plugin,
        )?;
        crate::method_contract_admission::admit_compiled_state_contract(&pin, state_plugin)?;
        ensure!(
            receipt.state_schema_hashes.get(state_plugin) == Some(&pin.schema_hash),
            "decoy and bridge observation schemas differ"
        );
    }
    ensure!(
        !contains_secret(&receipt.output)
            && receipt.states.values().all(|value| !contains_secret(value)),
        "decoy receipt contains credential material"
    );
    Ok(receipt)
}

fn private_or_tunnel_address(address: SocketAddr) -> bool {
    match address.ip() {
        std::net::IpAddr::V4(ip) => {
            let octets = ip.octets();
            ip.is_private()
                || ip.is_loopback()
                || ip.is_link_local()
                || (octets[0] == 100 && (64..=127).contains(&octets[1]))
        }
        std::net::IpAddr::V6(ip) => {
            let first = ip.octets()[0];
            ip.is_loopback() || ip.is_unicast_link_local() || first & 0xfe == 0xfc
        }
    }
}

fn verify_bridge_leaf(socket: &TlsDbusSocket, expected_pin: &[u8; 32]) -> Result<()> {
    let actual: [u8; 32] = Sha256::digest(socket.peer_leaf_certificate()?).into();
    ensure!(
        &actual == expected_pin,
        "authenticated client certificate is not the pinned bridge leaf"
    );
    Ok(())
}

pub async fn serve(
    address: SocketAddr,
    config: Arc<rustls::ServerConfig>,
    bridge_certificate_pin: [u8; 32],
    service: DecoyNativeService,
) -> Result<()> {
    ensure!(
        private_or_tunnel_address(address),
        "decoy native listener requires an explicit private/tunnel address"
    );
    let listener = tokio::net::TcpListener::bind(address).await?;
    let clients = Arc::new(tokio::sync::Semaphore::new(16));
    loop {
        let permit = clients.clone().acquire_owned().await?;
        let (stream, _) = listener.accept().await?;
        let config = config.clone();
        let service = service.clone();
        tokio::spawn(async move {
            let _permit = permit;
            let outcome = async {
                let socket = TlsDbusSocket::accept(stream, config).await?;
                verify_bridge_leaf(&socket, &bridge_certificate_pin)?;
                let connection_builder = zbus::connection::Builder::socket(socket)
                    .server(zbus::Guid::generate())?
                    .p2p()
                    .auth_mechanism(AuthMechanism::Anonymous)
                    .serve_at(OBJECT_PATH, service)?;
                let connection =
                    tokio::time::timeout(Duration::from_secs(10), connection_builder.build())
                        .await
                        .context("bridge D-Bus negotiation timed out")??;
                connection.closed().await;
                Ok::<_, anyhow::Error>(())
            }
            .await;
            if outcome.is_err() {
                tracing::warn!("decoy native peer rejected or disconnected");
            }
        });
    }
}

struct ProductionBackend {
    wireguard: Arc<op_plugins::state_plugins::decoy_network::wireguard_owner::WireGuardOwner>,
    wgcf: op_plugins::state_plugins::decoy_network::wgcf_owner::WgcfOwner,
}

#[async_trait]
impl DecoyNetworkBackend for ProductionBackend {
    async fn dispatch(
        &self,
        plugin: &str,
        method: &str,
        arguments: Value,
    ) -> Result<NativeNetworkReceipt> {
        let mut states = BTreeMap::new();
        let output = if plugin == "wgcf" {
            let result = self.wgcf.dispatch(method, arguments).await?;
            states.insert("wgcf".into(), serde_json::to_value(result.wgcf_state)?);
            if let Some(state) = result.wireguard_state {
                states.insert("wireguard".into(), state);
            }
            if let Some(state) = result.wg_opdbus_state {
                states.insert("wg_opdbus".into(), state);
            }
            result.output
        } else {
            let result = self.wireguard.dispatch(plugin, method, arguments).await?;
            states.insert(
                "wireguard".into(),
                serde_json::to_value(result.wireguard_state)?,
            );
            if let Some(state) = result.wg_opdbus_state {
                states.insert("wg_opdbus".into(), serde_json::to_value(state)?);
            }
            result.output
        };
        Ok(NativeNetworkReceipt {
            output,
            states,
            state_schema_hashes: BTreeMap::new(),
        })
    }
}

/// Explicit decoy-only binary entrypoint. Normal bridge startup never calls
/// this function and has no fallback to local WireGuard administration.
pub async fn run_decoy() -> Result<()> {
    use op_plugins::state_plugins::{
        decoy_network::{
            wgcf_owner::WgcfOwner,
            wireguard_owner::{DecoyRole, LinuxKernelWireGuard, WireGuardOwner},
        },
        runtime_documents::RuntimeDocuments,
    };
    let role = std::env::var("OP_NETWORK_ROLE").unwrap_or_default();
    ensure!(
        role == "decoy",
        "op-decoy-network requires OP_NETWORK_ROLE=decoy"
    );
    let allowlist = std::env::var("OP_DECOY_NETWORK_INTERFACES")
        .context("OP_DECOY_NETWORK_INTERFACES must explicitly allow decoy interfaces")?;
    let interfaces: Vec<_> = allowlist
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .collect();
    let address: SocketAddr = std::env::var("OP_DECOY_NATIVE_LISTEN")
        .context("OP_DECOY_NATIVE_LISTEN is required")?
        .parse()?;
    let transport = TransportFiles::from_env()?;
    let bridge_certificate_pin = TransportFiles::bridge_certificate_pin()?;
    let tls = transport.server_config()?;
    let path = std::env::var_os("OP_DECOY_NETWORK_COZO_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|| "/var/lib/op-dbus/decoy-network-cozo".into());
    let store = tokio::task::spawn_blocking(move || {
        op_cozo_store::CozoGraphShuttle::new_persistent_private(path)
    })
    .await??;
    let wireguard = Arc::new(WireGuardOwner::new(
        DecoyRole::Decoy,
        interfaces,
        Arc::new(LinuxKernelWireGuard),
    )?);
    let wgcf = WgcfOwner::production(Arc::new(RuntimeDocuments::new(store)), wireguard.clone())?;
    let service = DecoyNativeService::new(&role, Arc::new(ProductionBackend { wireguard, wgcf }))?;
    serve(address, tls, bridge_certificate_pin, service).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use rcgen::{BasicConstraints, CertificateParams, IsCa, KeyPair};
    use rustls::pki_types::PrivateKeyDer;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use tokio::net::TcpListener;

    struct AdmissionFixture {
        effects: AtomicUsize,
        malformed_state: AtomicBool,
    }

    #[async_trait]
    impl DecoyNetworkBackend for AdmissionFixture {
        async fn dispatch(
            &self,
            plugin: &str,
            _method: &str,
            _arguments: Value,
        ) -> Result<NativeNetworkReceipt> {
            self.effects.fetch_add(1, Ordering::SeqCst);
            let state = if self.malformed_state.load(Ordering::SeqCst) {
                serde_json::json!({"interfaces":"not-an-array"})
            } else {
                serde_json::json!({"interfaces":[]})
            };
            Ok(NativeNetworkReceipt {
                output: serde_json::json!({"interface":null}),
                states: BTreeMap::from([(plugin.to_string(), state)]),
                state_schema_hashes: BTreeMap::new(),
            })
        }
    }

    fn sealed_wireguard_catalog() -> (tempfile::TempDir, String) {
        let directory = tempfile::tempdir().unwrap();
        let schema = op_plugins::state_plugins::wireguard::wireguard_schema();
        let blob = op_blob::blobify_plugin_schema("wireguard", schema);
        op_blob::BlobStore::open(directory.path())
            .unwrap()
            .write(&blob)
            .unwrap();
        (directory, blob.manifest.schema_hash)
    }

    fn fixture_metadata(schema_hash: String, capability: &str) -> DecoyCallMetadata {
        let wireguard_pubkey = "fixture-public-key";
        let now = chrono::Utc::now().timestamp();
        let sealed_id = op_identity::sealed_id::SealedId {
            principal_id: op_identity::session::derive_principal_id(wireguard_pubkey),
            principal_kind: "wireguard-principal".into(),
            session_id: op_identity::session::derive_session_id(wireguard_pubkey),
            wireguard_pubkey: wireguard_pubkey.into(),
            session_genesis: "ab".repeat(32),
            trace_id: "cd".repeat(16),
            schema_version: 3,
            issued_at: now,
            expires_at: now + 60,
            arrival_timestamp: now,
            chain_head_at_arrival: "ef".repeat(32),
            catalog_hash_at_arrival: "12".repeat(32),
            head_timestamp_at_arrival: now,
            transport_scope: "dbus".into(),
            nextdns: None,
        }
        .seal()
        .unwrap();
        DecoyCallMetadata {
            capability: capability.into(),
            schema_hash,
            sealed_id,
        }
    }

    #[tokio::test]
    async fn real_sealed_catalog_admits_valid_call_and_rejects_before_effect() {
        let (catalog, schema_hash) = sealed_wireguard_catalog();
        let backend = Arc::new(AdmissionFixture {
            effects: AtomicUsize::new(0),
            malformed_state: AtomicBool::new(false),
        });
        let service = DecoyNativeService::with_catalog(backend.clone(), catalog.path().into());
        let receipt = service
            .call_owned(
                "wireguard".into(),
                "get_device".into(),
                serde_json::json!({"interface":"wg-decoy"}).to_string(),
                fixture_metadata(schema_hash.clone(), "wireguard.read"),
            )
            .await
            .unwrap();
        let receipt: NativeNetworkReceipt = serde_json::from_str(&receipt).unwrap();
        assert_eq!(receipt.output, serde_json::json!({"interface":null}));
        assert_eq!(receipt.state_schema_hashes["wireguard"], schema_hash);
        assert_eq!(backend.effects.load(Ordering::SeqCst), 1);

        for (method, arguments, capability) in [
            (
                "get_device",
                serde_json::json!({"interface":"wg-decoy"}),
                "wireguard.write",
            ),
            (
                "get_device",
                serde_json::json!({"unknown":true}),
                "wireguard.read",
            ),
            (
                "missing_method",
                serde_json::json!({"interface":"wg-decoy"}),
                "wireguard.read",
            ),
        ] {
            assert!(service
                .call_owned(
                    "wireguard".into(),
                    method.into(),
                    arguments.to_string(),
                    fixture_metadata(schema_hash.clone(), capability),
                )
                .await
                .is_err());
        }
        assert_eq!(backend.effects.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn malformed_native_state_is_rejected_against_real_seal() {
        let (catalog, schema_hash) = sealed_wireguard_catalog();
        let backend = Arc::new(AdmissionFixture {
            effects: AtomicUsize::new(0),
            malformed_state: AtomicBool::new(true),
        });
        let service = DecoyNativeService::with_catalog(backend.clone(), catalog.path().into());
        let error = service
            .call_owned(
                "wireguard".into(),
                "get_device".into(),
                serde_json::json!({"interface":"wg-decoy"}).to_string(),
                fixture_metadata(schema_hash, "wireguard.read"),
            )
            .await
            .unwrap_err();
        assert!(error.to_string().contains("exact") || error.to_string().contains("schema"));
        assert_eq!(backend.effects.load(Ordering::SeqCst), 1);
    }

    fn pinned_configs() -> (
        Arc<rustls::ClientConfig>,
        Arc<rustls::ClientConfig>,
        Arc<rustls::ServerConfig>,
        [u8; 32],
    ) {
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
        let ca_key = KeyPair::generate().unwrap();
        let mut ca_params = CertificateParams::new(vec![]).unwrap();
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
        let other_key = KeyPair::generate().unwrap();
        let other = CertificateParams::new(vec!["other.fixture".into()])
            .unwrap()
            .signed_by(&other_key, &ca, &ca_key)
            .unwrap();

        let bridge_client = op_network::dbus_tls::client_config(
            vec![ca.der().clone()],
            vec![bridge.der().clone()],
            PrivateKeyDer::Pkcs8(bridge_key.serialize_der().into()),
        )
        .unwrap();
        let other_client = op_network::dbus_tls::client_config(
            vec![ca.der().clone()],
            vec![other.der().clone()],
            PrivateKeyDer::Pkcs8(other_key.serialize_der().into()),
        )
        .unwrap();
        let server_config = op_network::dbus_tls::server_config(
            vec![ca.der().clone()],
            vec![server.der().clone()],
            PrivateKeyDer::Pkcs8(server_key.serialize_der().into()),
        )
        .unwrap();
        let pin = Sha256::digest(bridge.der().as_ref()).into();
        (bridge_client, other_client, server_config, pin)
    }

    struct MetadataFixture;

    #[zbus::interface(name = "org.opdbus.DecoyMetadataFixture")]
    impl MetadataFixture {
        fn echo(&self, metadata: DecoyCallMetadata) -> DecoyCallMetadata {
            metadata
        }
    }

    #[tokio::test]
    async fn pinned_bridge_leaf_and_typed_sid1_metadata_cross_dbus_exactly() {
        let (client, _, server, pin) = pinned_configs();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server_task = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let socket = TlsDbusSocket::accept(stream, server).await.unwrap();
            verify_bridge_leaf(&socket, &pin).unwrap();
            zbus::connection::Builder::socket(socket)
                .server(zbus::Guid::generate())
                .unwrap()
                .p2p()
                .auth_mechanism(AuthMechanism::Anonymous)
                .serve_at("/org/opdbus/metadata", MetadataFixture)
                .unwrap()
                .build()
                .await
                .unwrap()
        });
        let socket = TlsDbusSocket::connect(address, "decoy.fixture".into(), client)
            .await
            .unwrap();
        let connection = zbus::connection::Builder::socket(socket)
            .p2p()
            .auth_mechanism(AuthMechanism::Anonymous)
            .build()
            .await
            .unwrap();
        let _server = server_task.await.unwrap();
        let proxy = zbus::Proxy::new(
            &connection,
            "org.opdbus.DecoyMetadataFixture",
            "/org/opdbus/metadata",
            "org.opdbus.DecoyMetadataFixture",
        )
        .await
        .unwrap();
        let metadata = DecoyCallMetadata {
            capability: "wireguard.read".into(),
            schema_hash: "ab".repeat(32),
            sealed_id: b"exact-identity-owner-sid1".to_vec(),
        };
        let received: DecoyCallMetadata = proxy.call("Echo", &(metadata.clone(),)).await.unwrap();
        assert_eq!(received, metadata);
    }

    #[tokio::test]
    async fn another_leaf_signed_by_the_same_ca_is_rejected_by_pin() {
        let (_, other_client, server, pin) = pinned_configs();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server_task = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let socket = TlsDbusSocket::accept(stream, server).await.unwrap();
            verify_bridge_leaf(&socket, &pin)
        });
        let _ = TlsDbusSocket::connect(address, "decoy.fixture".into(), other_client).await;
        assert!(server_task.await.unwrap().is_err());
    }

    #[test]
    fn listener_accepts_only_private_or_tunnel_ranges() {
        for accepted in [
            "127.0.0.1:1",
            "10.0.0.1:1",
            "100.64.0.1:1",
            "172.16.0.1:1",
            "192.168.0.1:1",
            "[fd00::1]:1",
            "[fe80::1]:1",
        ] {
            assert!(private_or_tunnel_address(accepted.parse().unwrap()));
        }
        for rejected in ["0.0.0.0:1", "8.8.8.8:1", "[::]:1", "[2001:4860::1]:1"] {
            assert!(!private_or_tunnel_address(rejected.parse().unwrap()));
        }
    }

    #[test]
    fn secondary_secret_guard_normalizes_common_spellings() {
        for key in [
            "api_key",
            "api-key",
            "apiKey",
            "client_secret",
            "privateKey",
            "adminPassword",
            "Authorization",
        ] {
            let mut value = serde_json::Map::new();
            value.insert(key.to_string(), Value::String("private".to_string()));
            assert!(contains_secret(&Value::Object(value)));
        }
        assert!(!contains_secret(&serde_json::json!({
            "token_count": 4,
            "public_key": "public"
        })));
    }
}

#[cfg(test)]
#[path = "decoy_canonical_tests.rs"]
mod canonical_tests;
