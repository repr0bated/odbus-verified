// 🟢 🛡️ The Tonic gRPC Gatekeeper (Middleware Interceptor)
// Sits on the primary gRPC ingress at port 8090. Intercepts Xray-injected headers,
// resolves the request's projected identity session, and either allows the gRPC
// payload through or drops the connection instantly.
//
// Operated by A.N.N.A. Scribe. No payload enters the system without a cryptographic
// "Snowball" session. No process-wide, last-writer-wins identity is consulted.
#![allow(clippy::items_after_test_module)]

use std::net::SocketAddr;
use std::sync::{Arc, RwLock};

use axum::extract::ConnectInfo as AxumConnectInfo;
use tonic::transport::server::{TcpConnectInfo, TlsConnectInfo};
use tonic::{Request, Status};

use crate::mutation_engine::MutationEngine;
use crate::oracle_assertion::{AssertionRejection, AssertionValidator, HumanPrincipalIdentity};

/// gRPC metadata key for the optional oracle identity assertion (OIA1 wire bytes).
pub const ASSERTION_METADATA_KEY: &str = "x-oracle-identity-assertion-bin";
/// gRPC metadata key for MutationEngine-authored SID1 (local identity).
pub const SEALED_ID_METADATA_KEY: &str = op_identity::sealed_id::HTTP_HEADER_NAME;

/// Identity extracted by the Ghostbridge interceptor and attached to each
/// accepted request for bridge-layer authorization.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GhostbridgeIdentity {
    /// Stable registered identity used for authorization and audit metadata.
    pub principal_id: String,
    pub session_id: String,
    /// Session arrival proof. Never used as an authorization key.
    pub session_genesis: String,
}

/// Process-wide handle to the engine, set once from `build_operation_routes`
/// (the single choke point all routes are built from) before any interceptor
/// can run. Lets the interceptor do a per-identity Cozo lookup
/// (`identity_sled_dispatch::get_identity`) without threading `Arc<MutationEngine>`
/// through every one of the ~10 `.with_interceptor()` call sites.
static ENGINE: RwLock<Option<Arc<MutationEngine>>> = RwLock::new(None);

pub fn set_engine(engine: Arc<MutationEngine>) {
    *ENGINE.write().expect("engine lock poisoned") = Some(engine);
}

/// Read-only handle to the registered engine, for sync code paths (e.g. the
/// UDS identity interceptor) that resolve a session record without owning the
/// engine. Returns `None` until `set_engine` has run.
pub(crate) fn engine_handle() -> Option<Arc<MutationEngine>> {
    ENGINE.read().expect("engine lock poisoned").clone()
}

/// Anchor a human session after the interceptor has already verified the
/// decoy assertion. The sled store stays on the engine; this policy does not.
pub(crate) async fn anchor_verified_human_session(
    engine: &MutationEngine,
    wireguard_pubkey: &str,
) -> Option<crate::mutation_engine::SessionContext> {
    let valid_wireguard_key = base64::Engine::decode(
        &base64::engine::general_purpose::STANDARD,
        wireguard_pubkey.trim(),
    )
    .is_ok_and(|raw| raw.len() == 32);
    if !valid_wireguard_key {
        return None;
    }
    let session_id = op_identity::session::derive_session_id(wireguard_pubkey);
    // Unavailable or malformed committed identity state must not be treated
    // as a first arrival eligible for minting.
    let record = crate::identity_sled_dispatch::stored_session(engine, &session_id)
        .await
        .ok()?;
    if let Some(record) = record {
        let now = chrono::Utc::now().timestamp();
        let current = record.is_anchored()
            && record.wireguard_pubkey == wireguard_pubkey
            && record
                .expires_at
                .is_none_or(|expires_at| expires_at == 0 || expires_at > now);
        if !current {
            tracing::warn!(
                %session_id,
                "verified identity has an expired or mismatched authoritative sled"
            );
            return None;
        }
        crate::identity_sled_dispatch::set_session_actor_class(engine, &session_id, "human")
            .await
            .ok()?;
        let context = crate::mutation_engine::SessionContext {
            genesis_hex: record.genesis.unwrap_or_default(),
            session_id: record.session_id,
            wireguard_pubkey: record.wireguard_pubkey,
            sealed_id_digest: crate::mutation_engine::sealed_id_digest(record.sealed_id.as_deref()),
        };
        engine.register_session_context(context.clone()).await;
        return Some(context);
    }
    match engine
        .mint_and_store_genesis(&session_id, wireguard_pubkey)
        .await
    {
        Ok(_) => {
            crate::identity_sled_dispatch::set_session_actor_class(engine, &session_id, "human")
                .await
                .ok()?;
            engine.session_context(&session_id).await
        }
        Err(error) => {
            tracing::warn!(
                %error,
                %session_id,
                "verified human arrival could not be anchored"
            );
            None
        }
    }
}

/// Per-serving-instance interceptor carrying its own assertion validator.
#[derive(Clone)]
pub struct GhostbridgeInterceptorWithValidator {
    validator: Arc<AssertionValidator>,
}

impl GhostbridgeInterceptorWithValidator {
    pub fn new(validator: Arc<AssertionValidator>) -> Self {
        Self { validator }
    }
}

impl tonic::service::Interceptor for GhostbridgeInterceptorWithValidator {
    fn call(&mut self, req: Request<()>) -> Result<Request<()>, Status> {
        ghostbridge_interceptor_with_validator(&self.validator, req)
    }
}

/// Build the per-serving-instance PluginService/generated-route interceptor.
/// Oracle assertion metadata is mandatory on this gate; legacy session-genesis
/// authentication remains available only through [`GhostbridgeInterceptor`]
/// on explicitly legacy domain services.
pub fn make_ghostbridge_interceptor(
    validator: Arc<AssertionValidator>,
) -> GhostbridgeInterceptorWithValidator {
    GhostbridgeInterceptorWithValidator::new(validator)
}

/// Registration bootstrap interceptor sharing the same validator-backed gate.
#[derive(Clone)]
pub struct RegistrationInterceptorWithValidator {
    gate: GhostbridgeInterceptorWithValidator,
}

impl RegistrationInterceptorWithValidator {
    pub fn new(validator: Arc<AssertionValidator>) -> Self {
        Self {
            gate: GhostbridgeInterceptorWithValidator::new(validator),
        }
    }
}

impl tonic::service::Interceptor for RegistrationInterceptorWithValidator {
    fn call(&mut self, req: Request<()>) -> Result<Request<()>, Status> {
        match req.extensions().get::<tonic::GrpcMethod<'_>>() {
            Some(method)
                if method.service() == "operation.registration.v1.RegistrationService"
                    && matches!(method.method(), "SendMagicLink" | "VerifyMagicLink") =>
            {
                Ok(req)
            }
            _ => self.gate.call(req),
        }
    }
}

pub fn make_registration_interceptor(
    validator: Arc<AssertionValidator>,
) -> RegistrationInterceptorWithValidator {
    RegistrationInterceptorWithValidator::new(validator)
}

/// Resolve the principal/session identity for capability gating. Human principal
/// identity shadows Ghostbridge when both are present.
pub fn bridge_capability_identity(extensions: &tonic::Extensions) -> Option<GhostbridgeIdentity> {
    if let Some(human) = extensions.get::<HumanPrincipalIdentity>() {
        return Some(GhostbridgeIdentity {
            principal_id: human.principal_id.clone(),
            session_id: human.session_id.clone(),
            session_genesis: human.session_genesis.clone(),
        });
    }
    extensions.get::<GhostbridgeIdentity>().cloned()
}

/// Shared peer-address reader for production serve builders and the E2E fixture.
pub fn connect_info_peer_addr(req: &Request<()>) -> Option<SocketAddr> {
    if let Some(info) = req.extensions().get::<TcpConnectInfo>()
        && let Some(addr) = info.remote_addr()
    {
        return Some(addr);
    }
    if let Some(info) = req.extensions().get::<TlsConnectInfo<TcpConnectInfo>>()
        && let Some(addr) = info.get_ref().remote_addr()
    {
        return Some(addr);
    }
    req.extensions()
        .get::<AxumConnectInfo<SocketAddr>>()
        .map(|ci| ci.0)
}

fn peer_socket_addr(req: &Request<()>) -> Option<SocketAddr> {
    connect_info_peer_addr(req)
}

#[allow(clippy::result_large_err)]
fn read_bin_metadata(req: &Request<()>, key: &str) -> Result<Option<Vec<u8>>, Status> {
    let values: Vec<_> = req.metadata().get_all_bin(key).into_iter().collect();
    if values.is_empty() {
        return Ok(None);
    }
    if values.len() > 1 {
        return Err(AssertionRejection::Malformed.into());
    }
    let wire = values[0]
        .to_bytes()
        .map_err(|_| AssertionRejection::Malformed)?;
    Ok(Some(wire.to_vec()))
}

#[allow(clippy::result_large_err)]
fn read_assertion_wire(req: &Request<()>) -> Result<Option<Vec<u8>>, Status> {
    read_bin_metadata(req, ASSERTION_METADATA_KEY)
}

#[allow(clippy::result_large_err)]
fn read_sealed_id_wire(req: &Request<()>) -> Result<Option<Vec<u8>>, Status> {
    read_bin_metadata(req, SEALED_ID_METADATA_KEY)
}

/// Why a decoy-minted identity credential could not be written onto request metadata.
#[derive(Debug)]
pub(crate) enum IdentityMetadataError {
    Multiple,
    Missing,
    InvalidSid1,
    InvalidOia1,
    EmptyCapability,
    InvalidCapability(String),
}

impl std::fmt::Display for IdentityMetadataError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Multiple => write!(formatter, "multiple identity credentials"),
            Self::Missing => write!(formatter, "missing identity credential"),
            Self::InvalidSid1 => write!(formatter, "invalid SID1 credential"),
            Self::InvalidOia1 => write!(formatter, "invalid OIA1 credential"),
            Self::EmptyCapability => write!(formatter, "capability metadata cannot be empty"),
            Self::InvalidCapability(error) => {
                write!(formatter, "Invalid capability metadata: {error}")
            }
        }
    }
}

/// Write exactly one decoy-minted identity credential into gRPC metadata.
///
/// The session anchor lives on this gate. Callers that still hold the wire
/// bytes use this same write; the bridge client does not keep a second copy.
pub(crate) fn insert_identity_metadata<T>(
    request: &mut Request<T>,
    sealed_id: Option<&[u8]>,
    oracle_assertion: Option<&[u8]>,
    capability_id: Option<&str>,
) -> Result<(), IdentityMetadataError> {
    let valid_wire =
        |wire: &[u8], magic: &[u8; 4]| wire.len() <= 16 * 1024 && wire.starts_with(magic);
    match (sealed_id, oracle_assertion) {
        (Some(_), Some(_)) => return Err(IdentityMetadataError::Multiple),
        (None, None) => return Err(IdentityMetadataError::Missing),
        (Some(wire), None) => {
            if !valid_wire(wire, b"SID1") {
                return Err(IdentityMetadataError::InvalidSid1);
            }
            request.metadata_mut().insert_bin(
                SEALED_ID_METADATA_KEY,
                tonic::metadata::MetadataValue::from_bytes(wire),
            );
        }
        (None, Some(wire)) => {
            if !valid_wire(wire, b"OIA1") {
                return Err(IdentityMetadataError::InvalidOia1);
            }
            request.metadata_mut().insert_bin(
                ASSERTION_METADATA_KEY,
                tonic::metadata::MetadataValue::from_bytes(wire),
            );
        }
    }
    if let Some(capability_id) = capability_id {
        if capability_id.is_empty() {
            return Err(IdentityMetadataError::EmptyCapability);
        }
        request.metadata_mut().insert(
            "x-opdbus-capability",
            tonic::metadata::MetadataValue::try_from(capability_id)
                .map_err(|error| IdentityMetadataError::InvalidCapability(error.to_string()))?,
        );
    }
    Ok(())
}

#[allow(clippy::result_large_err)]
fn authenticate_sid1_grpc(mut req: Request<()>, wire: &[u8]) -> Result<Request<()>, Status> {
    let engine =
        engine_handle().ok_or_else(|| Status::internal("MutationEngine Memory Unreachable"))?;
    let claims = tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current().block_on(
            crate::identity_sled_dispatch::authenticate_sid1_wire(engine.as_ref(), wire, "grpc"),
        )
    })
    .map_err(Status::unauthenticated)?;
    // SID1 proves the sled binding, not that its key remains authorized.
    // Match the registry/revocation gate used by OIA1, MCP and D-Bus.
    let principal_id = resolve_active_principal_id(&claims.wireguard_pubkey)?;
    if principal_id != claims.principal_id {
        return Err(Status::unauthenticated(
            "Sealed identity does not match the registered principal",
        ));
    }
    req.extensions_mut().insert(HumanPrincipalIdentity {
        principal_id: claims.principal_id,
        human_pubkey: claims.wireguard_pubkey,
        session_id: claims.session_id,
        expires_at: claims.expires_at,
        session_genesis: claims.session_genesis,
    });
    Ok(req)
}

#[allow(clippy::result_large_err)]
fn ghostbridge_interceptor_with_validator(
    validator: &AssertionValidator,
    mut req: Request<()>,
) -> Result<Request<()>, Status> {
    let assertion = read_assertion_wire(&req)?;
    let sealed = read_sealed_id_wire(&req)?;
    match (assertion.as_deref(), sealed.as_deref()) {
        (Some(_), Some(_)) => {
            return Err(Status::unauthenticated(
                "send exactly one identity credential, not both OIA1 and SID1",
            ));
        }
        (None, Some(wire)) => {
            insert_identity_metadata(&mut req, Some(wire), None, None)
                .map_err(|error| Status::unauthenticated(error.to_string()))?;
            return authenticate_sid1_grpc(req, wire);
        }
        (Some(_), None) => {
            // Bytes are already on the request. Re-running the writer helper
            // rejects a missing OIA1 magic as "invalid OIA1 credential" and
            // hides AssertionRejection::Malformed, which is the admission
            // result for undecodable assertion metadata.
        }
        (None, None) => {
            return Err(Status::unauthenticated("Missing Oracle identity assertion"));
        }
    }
    let wire = assertion.expect("OIA1 wire after match");
    let source = peer_socket_addr(&req);
    let now = chrono::Utc::now().timestamp();
    let registration_bootstrap = req
        .metadata()
        .get(crate::grpc_server::DECLARED_CAPABILITY_HEADER)
        .and_then(|v| v.to_str().ok())
        == Some("human_principal.write");
    let mut pending = validator
        .validate_pending_with_bootstrap(&wire, source, now, registration_bootstrap)
        .map_err(Status::from)?;
    let engine =
        engine_handle().ok_or_else(|| Status::internal("MutationEngine Memory Unreachable"))?;
    let session = tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current().block_on(anchor_verified_human_session(
            engine.as_ref(),
            &pending.identity().human_pubkey,
        ))
    })
    .ok_or_else(|| Status::unauthenticated("Assertion identity has no anchored session genesis"))?;
    pending.identity_mut().session_id = session.session_id;
    pending.identity_mut().session_genesis = session.genesis_hex;
    let identity = validator.consume_pending(pending).map_err(Status::from)?;
    req.extensions_mut().insert(identity);
    Ok(req)
}

/// Legacy/test entry: anchored-session path (assertion metadata absent).
#[allow(clippy::result_large_err)]
pub fn ghostbridge_interceptor(req: Request<()>) -> Result<Request<()>, Status> {
    ghostbridge_genesis_interceptor(req)
}

/// Look up the specific caller's identity sled and check its session genesis +
/// term (`expires_at`) - the per-identity path. Resolves either by a
/// WireGuard pubkey (host/xray-mesh callers, session_id derived from the
/// pubkey) or, when there's no pubkey, directly by the trace_id the caller
/// presents (vendor/partner identities like a browser frontend that has no
/// WireGuard identity of its own - e.g. Lovable). Returns `None` when
/// there's no engine registered yet, no identifying header, or no matching
/// record, so the caller can fall back to the shared host legacy sled.
#[allow(clippy::result_large_err)]
fn resolve_active_principal_id(human_pubkey: &str) -> Result<String, Status> {
    if human_pubkey.is_empty() {
        return Err(Status::unauthenticated(
            "Session has no registered human public key",
        ));
    }
    let record = tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current().block_on(
            crate::human_principal_dispatch::resolve_key_for_assertion(human_pubkey),
        )
    })
    .map_err(|_| Status::internal("Human principal registry unavailable"))?
    .ok_or_else(|| Status::unauthenticated("Session is not bound to a registered principal"))?;
    if record.revoked_at != 0 {
        return Err(Status::permission_denied("Registered principal is revoked"));
    }
    if record.principal_id.is_empty() {
        return Err(Status::unauthenticated(
            "Registered principal has no stable principal_id",
        ));
    }
    Ok(record.principal_id)
}

fn verify_per_identity(
    pubkey: Option<&str>,
    trace_id: Option<&str>,
    request_genesis: &str,
) -> Option<Result<GhostbridgeIdentity, Status>> {
    let engine = ENGINE.read().expect("engine lock poisoned").clone()?;
    let session_id = pubkey.map(op_identity::session::derive_session_id);
    let handle = match (pubkey, trace_id) {
        (Some(pubkey), _) => pubkey,
        (None, Some(trace)) => trace,
        (None, None) => return None,
    };

    let result = tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current().block_on(
            crate::identity_sled_dispatch::session_record_for_actor(engine.as_ref(), handle),
        )
    });

    let identity = match result {
        Ok(Some(value)) => serde_json::to_value(value).ok()?,
        Ok(None) => return None,
        Err(error) => {
            return Some(Err(Status::unavailable(format!(
                "identity credential projection unavailable: {error}"
            ))))
        }
    };
    // Session genesis is the authoritative arrival proof. Legacy hashed
    // identity values are deliberately not accepted as authorization input.
    let stored = identity
        .get("genesis")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let stored = stored?;
    let expires_at = identity.get("expires_at").and_then(|v| v.as_i64());

    if let Some(expires_at) = expires_at
        && expires_at != 0
        && expires_at <= chrono::Utc::now().timestamp()
    {
        return Some(Err(Status::permission_denied(
            "Identity term has expired. Re-authenticate to renew.",
        )));
    }

    if stored != request_genesis {
        return None;
    }

    let resolved_session_id = identity
        .get("session_id")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .or(session_id)?;
    let human_pubkey = identity
        .get("wireguard_pubkey")
        .and_then(|v| v.as_str())
        .filter(|value| !value.is_empty())
        .or(pubkey)?;
    let principal_id = match resolve_active_principal_id(human_pubkey) {
        Ok(value) => value,
        Err(error) => return Some(Err(error)),
    };
    Some(Ok(GhostbridgeIdentity {
        principal_id,
        session_id: resolved_session_id,
        session_genesis: request_genesis.to_string(),
    }))
}

/// THE GATEKEEPER: legacy Ghostbridge session-genesis path.
#[allow(clippy::result_large_err)]
fn ghostbridge_genesis_interceptor(mut req: Request<()>) -> Result<Request<()>, Status> {
    let genesis_value = req.metadata().get("x-ghostbridge-genesis").cloned();
    let trace_value = req
        .metadata()
        .get("x-ghostbridge-trace-id")
        .or_else(|| req.metadata().get("x-wireguard-pubkey"))
        .cloned();

    if genesis_value.is_none() || trace_value.is_none() {
        return Err(Status::unauthenticated(
            "Missing Ghostbridge session identity. Connection dropped.",
        ));
    }

    let request_genesis = genesis_value
        .as_ref()
        .unwrap()
        .to_str()
        .map_err(|_| Status::invalid_argument("Invalid genesis header encoding"))?;

    let pubkey_value = req.metadata().get("x-wireguard-pubkey").cloned();
    let raw_trace_value = req.metadata().get("x-ghostbridge-trace-id").cloned();
    let pubkey_str = pubkey_value
        .as_ref()
        .map(|v| v.to_str())
        .transpose()
        .map_err(|_| Status::invalid_argument("Invalid pubkey header encoding"))?;
    let trace_str = raw_trace_value
        .as_ref()
        .map(|v| v.to_str())
        .transpose()
        .map_err(|_| Status::invalid_argument("Invalid trace header encoding"))?;
    if (pubkey_str.is_some() || trace_str.is_some())
        && let Some(outcome) = verify_per_identity(pubkey_str, trace_str, request_genesis)
    {
        req.extensions_mut().insert(outcome?);
        return Ok(req);
    }

    // Fallback: no identifying header beyond the asserted session genesis, so
    // resolve the session record by it
    // and fail closed when no record matches — an unknown genesis is no
    // identity, not a licence to admit.
    let engine = ENGINE.read().expect("engine lock poisoned").clone();
    let engine = match engine {
        Some(engine) => engine,
        None => return Err(Status::internal("MutationEngine Memory Unreachable")),
    };
    let record = tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current().block_on(
            crate::identity_sled_dispatch::session_record_for_actor(
                engine.as_ref(),
                request_genesis,
            ),
        )
    });
    match record {
        Ok(Some(sled)) => {
            let principal_id = resolve_active_principal_id(&sled.wireguard_pubkey)?;
            req.extensions_mut().insert(GhostbridgeIdentity {
                principal_id,
                session_id: sled.session_id,
                session_genesis: request_genesis.to_string(),
            });
            Ok(req)
        }
        Ok(None) => Err(Status::permission_denied(
            "A.N.N.A. Scribe: Unknown session genesis. Connection Dropped.",
        )),
        Err(error) => Err(Status::unavailable(format!(
            "identity credential projection unavailable: {error}"
        ))),
    }
}

#[derive(Clone, Debug)]
pub struct GhostbridgeInterceptor;

impl tonic::service::Interceptor for GhostbridgeInterceptor {
    fn call(&mut self, req: Request<()>) -> Result<Request<()>, Status> {
        ghostbridge_interceptor(req)
    }
}

/// Permit only the two calls needed to establish an identity. Everything else
/// on RegistrationService still passes through the normal sled gate.
#[allow(clippy::result_large_err)]
pub fn registration_interceptor(req: Request<()>) -> Result<Request<()>, Status> {
    match req.extensions().get::<tonic::GrpcMethod<'_>>() {
        Some(method)
            if method.service() == "operation.registration.v1.RegistrationService"
                && matches!(method.method(), "SendMagicLink" | "VerifyMagicLink") =>
        {
            Ok(req)
        }
        _ => ghostbridge_interceptor(req),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::human_principal_dispatch::tests::{pk, register, temp_cozo, TempShm};
    use crate::identity_sled_dispatch::tests::{sled_engine, write_identity};
    use crate::oracle_assertion::tests::{
        signed_with_fields, source_at, test_ip, test_issuer, trust_store_for_issuer,
    };
    use crate::oracle_assertion::AssertionValidator;
    use base64::Engine as _;
    use op_identity::session::derive_principal_id;
    use tonic::metadata::MetadataValue;
    use tonic::service::Interceptor;
    use tonic::Code;

    fn install_grants(document: serde_json::Value) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("grants tempdir");
        let path = dir.path().join("capability-grants.json");
        std::fs::write(
            &path,
            serde_json::to_vec(&document).expect("serialize grants"),
        )
        .expect("write grants");
        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::set_var("OP_GRANTS_PATH", path) };
        dir
    }

    fn request_with_connect_info(ip: std::net::IpAddr) -> Request<()> {
        let mut req = Request::new(());
        req.extensions_mut().insert(source_at(ip));
        req.extensions_mut().insert(TcpConnectInfo {
            local_addr: Some(source_at(ip)),
            remote_addr: Some(source_at(ip)),
        });
        req
    }

    fn insert_assertion_metadata(req: &mut Request<()>, wire: &[u8]) {
        req.metadata_mut()
            .insert_bin(ASSERTION_METADATA_KEY, MetadataValue::from_bytes(wire));
    }

    fn insert_ghostbridge_headers(req: &mut Request<()>, genesis: &str, trace: &str) {
        req.metadata_mut().insert(
            "x-ghostbridge-genesis",
            MetadataValue::try_from(genesis).expect("genesis header"),
        );
        req.metadata_mut().insert(
            "x-ghostbridge-trace-id",
            MetadataValue::try_from(trace).expect("trace header"),
        );
    }

    fn fresh_signed(
        issuer: &op_identity::oracle_assertion::DecoyIssuer,
        pubkey: &str,
        nonce: [u8; 16],
    ) -> op_identity::oracle_assertion::SignedAssertion {
        let now = chrono::Utc::now().timestamp();
        signed_with_fields(issuer, pubkey, test_ip(), now - 5, now + 300, nonce, None)
    }

    fn validator_for_tests() -> Arc<AssertionValidator> {
        let issuer = test_issuer();
        Arc::new(AssertionValidator::new(trust_store_for_issuer(&issuer)))
    }

    async fn anchor_assertion_session(pubkey: &str) -> TempShm {
        let (engine, shm) = sled_engine();
        write_identity(engine.as_ref(), pubkey)
            .await
            .expect("write assertion session identity");
        set_engine(engine);
        shm
    }

    #[test]
    fn test_rejects_missing_genesis_header() {
        let req = Request::new(());
        let result = ghostbridge_interceptor(req);
        assert!(result.is_err());
        let status = result.unwrap_err();
        assert_eq!(status.code(), tonic::Code::Unauthenticated);
        assert!(status
            .message()
            .contains("Missing Ghostbridge session identity"));
    }

    #[test]
    fn registration_bootstrap_allows_magic_link_without_identity() {
        for method in ["SendMagicLink", "VerifyMagicLink"] {
            let mut req = Request::new(());
            req.extensions_mut().insert(tonic::GrpcMethod::new(
                "operation.registration.v1.RegistrationService",
                method,
            ));
            assert!(registration_interceptor(req).is_ok());
        }
    }

    #[test]
    fn registration_bootstrap_does_not_expose_admin_methods() {
        let mut req = Request::new(());
        req.extensions_mut().insert(tonic::GrpcMethod::new(
            "operation.registration.v1.RegistrationService",
            "ListUsers",
        ));
        assert_eq!(
            registration_interceptor(req).unwrap_err().code(),
            tonic::Code::Unauthenticated
        );
    }

    #[test]
    fn test_rejects_missing_trace_header() {
        let mut req = Request::new(());
        req.metadata_mut().insert(
            "x-ghostbridge-genesis",
            MetadataValue::from_static("deadbeef"),
        );
        let result = ghostbridge_interceptor(req);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code(), tonic::Code::Unauthenticated);
    }

    #[test]
    fn test_rejects_missing_genesis_with_trace_only() {
        let mut req = Request::new(());
        req.metadata_mut().insert(
            "x-ghostbridge-trace-id",
            MetadataValue::from_static("trace-abc"),
        );
        let result = ghostbridge_interceptor(req);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code(), tonic::Code::Unauthenticated);
    }

    #[test]
    fn test_genesis_mismatch_detection() {
        let sled_genesis: [u8; 32] = [0xAA; 32];
        let expected = hex::encode(sled_genesis);
        let request_genesis = "0000000000000000000000000000000000000000000000000000000000000000";
        assert_ne!(request_genesis, expected);
    }

    #[test]
    fn test_genesis_match_succeeds() {
        let sled_genesis: [u8; 32] = [0xBB; 32];
        let expected = hex::encode(sled_genesis);
        let request_genesis = hex::encode([0xBB; 32]);
        assert_eq!(request_genesis, expected);
    }

    #[test]
    fn test_mutation_engine_unreachable_returns_internal() {
        if engine_handle().is_some() {
            eprintln!("skipping: another interceptor test registered an engine");
            return;
        }
        let mut req = Request::new(());
        req.metadata_mut().insert(
            "x-ghostbridge-genesis",
            MetadataValue::from_static("aabbccdd"),
        );
        req.metadata_mut().insert(
            "x-ghostbridge-trace-id",
            MetadataValue::from_static("trace-aabbccdd"),
        );
        let result = ghostbridge_interceptor(req);
        assert!(result.is_err());
        let status = result.unwrap_err();
        assert_eq!(status.code(), tonic::Code::Internal);
        assert!(status
            .message()
            .contains("MutationEngine Memory Unreachable"));
    }

    /// VAL-BRIDGE-021
    pub async fn assertion_present_valid_inserts_human_principal_identity_impl() {
        let _cozo = temp_cozo();
        let issuer = test_issuer();
        let pubkey = pk(41);
        register(&pubkey, "interceptor").await.expect("register");
        let _shm = anchor_assertion_session(&pubkey).await;
        let mut gate = make_ghostbridge_interceptor(Arc::new(AssertionValidator::new(
            trust_store_for_issuer(&issuer),
        )));
        let signed = fresh_signed(&issuer, &pubkey, [0x41; 16]);
        let mut req = request_with_connect_info(test_ip());
        insert_assertion_metadata(&mut req, &signed.to_wire());
        let out = gate.call(req).expect("valid assertion");
        let identity = out
            .extensions()
            .get::<HumanPrincipalIdentity>()
            .expect("human identity inserted");
        assert_eq!(identity.human_pubkey, pubkey);
        assert_eq!(identity.principal_id, derive_principal_id(&pubkey));
        assert!(!identity.session_id.is_empty());
        assert!(!identity.session_genesis.is_empty());
    }

    /// VAL-BRIDGE-022
    pub async fn assertion_present_invalid_returns_unauthenticated_impl() {
        let mut gate = make_ghostbridge_interceptor(validator_for_tests());
        let mut req = request_with_connect_info(test_ip());
        insert_assertion_metadata(&mut req, b"not-an-oia1-envelope");
        let status = gate.call(req).unwrap_err();
        assert_eq!(status.code(), Code::Unauthenticated);
        assert!(status.message().contains("Malformed"));
    }

    /// VAL-BRIDGE-023
    pub async fn assertion_present_legacy_headers_not_consulted_impl() {
        let _cozo = temp_cozo();
        let issuer = test_issuer();
        let pubkey = pk(42);
        register(&pubkey, "shadow").await.expect("register");
        let _shm = anchor_assertion_session(&pubkey).await;
        let mut gate = make_ghostbridge_interceptor(Arc::new(AssertionValidator::new(
            trust_store_for_issuer(&issuer),
        )));
        let signed = fresh_signed(&issuer, &pubkey, [0x42; 16]);
        let mut req = request_with_connect_info(test_ip());
        insert_assertion_metadata(&mut req, &signed.to_wire());
        insert_ghostbridge_headers(
            &mut req,
            "0000000000000000000000000000000000000000000000000000000000000000",
            "wrong-trace",
        );
        let out = gate.call(req).expect("assertion governs");
        assert!(out.extensions().get::<GhostbridgeIdentity>().is_none());
        assert!(out.extensions().get::<HumanPrincipalIdentity>().is_some());
    }

    /// VAL-BRIDGE-024
    pub async fn assertion_absent_rejects_legacy_headers_impl() {
        let mut gate = make_ghostbridge_interceptor(validator_for_tests());
        let mut req = Request::new(());
        insert_ghostbridge_headers(&mut req, &"aa".repeat(32), "legacy-trace");
        let status = gate.call(req).unwrap_err();
        assert_eq!(status.code(), Code::Unauthenticated);
        assert_eq!(status.message(), "Missing Oracle identity assertion");
    }

    /// VAL-BRIDGE-025
    pub async fn duplicate_assertion_metadata_values_reject_malformed_impl() {
        let mut gate = make_ghostbridge_interceptor(validator_for_tests());
        let mut req = request_with_connect_info(test_ip());
        req.metadata_mut()
            .append_bin(ASSERTION_METADATA_KEY, MetadataValue::from_bytes(b"OIA1"));
        req.metadata_mut()
            .append_bin(ASSERTION_METADATA_KEY, MetadataValue::from_bytes(b"OIA1"));
        let status = gate.call(req).unwrap_err();
        assert_eq!(status.code(), Code::Unauthenticated);
        assert!(status.message().contains("Malformed"));
    }

    fn insert_sealed_id_metadata(req: &mut Request<()>, wire: &[u8]) {
        req.metadata_mut()
            .insert_bin(SEALED_ID_METADATA_KEY, MetadataValue::from_bytes(wire));
    }

    /// VAL-BRIDGE-026
    pub async fn assertion_without_connect_info_rejects_missing_connect_info_impl() {
        let _cozo = temp_cozo();
        let issuer = test_issuer();
        let pubkey = pk(43);
        register(&pubkey, "noci").await.expect("register");
        let mut gate = make_ghostbridge_interceptor(Arc::new(AssertionValidator::new(
            trust_store_for_issuer(&issuer),
        )));
        let signed = fresh_signed(&issuer, &pubkey, [0x43; 16]);
        let mut req = Request::new(());
        insert_assertion_metadata(&mut req, &signed.to_wire());
        let status = gate.call(req).unwrap_err();
        assert_eq!(status.code(), Code::Unauthenticated);
        assert!(status.message().contains("MissingConnectInfo"));
    }

    pub async fn sid1_parked_inserts_human_principal_identity_impl() {
        let _cozo = temp_cozo();
        let (engine, _shm) = sled_engine();
        let pubkey = pk(0x72);
        register(&pubkey, "parked-sid1").await.expect("register");
        let identity = write_identity(engine.as_ref(), &pubkey)
            .await
            .expect("write");
        crate::identity_sled_dispatch::tests::park_identity(engine.as_ref(), &identity.session_id)
            .await
            .expect("park");
        set_engine(engine);
        let sealed = identity.sealed_id.clone().expect("sid1");
        let encoded = sealed
            .strip_prefix(op_identity::sealed_id::INLINE_PREFIX)
            .expect("sid1 prefix");
        let wire = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(encoded.as_bytes())
            .expect("sid1 b64");
        let mut gate = make_ghostbridge_interceptor(validator_for_tests());
        let mut req = Request::new(());
        insert_sealed_id_metadata(&mut req, &wire);
        let out = gate.call(req).expect("parked sid1");
        let inserted = out
            .extensions()
            .get::<HumanPrincipalIdentity>()
            .expect("human identity inserted");
        assert_eq!(inserted.human_pubkey, pubkey);
        assert_eq!(inserted.session_id, identity.session_id);
        assert_eq!(
            inserted.session_genesis,
            identity.genesis.clone().expect("genesis")
        );
    }

    pub async fn sid1_and_oia1_together_rejected_impl() {
        let (engine, _shm) = sled_engine();
        let pubkey = pk(0x73);
        let identity = write_identity(engine.as_ref(), &pubkey)
            .await
            .expect("write");
        set_engine(engine);
        let sealed = identity.sealed_id.expect("sid1");
        let encoded = sealed
            .strip_prefix(op_identity::sealed_id::INLINE_PREFIX)
            .expect("sid1 prefix");
        let wire = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(encoded.as_bytes())
            .expect("sid1 b64");
        let issuer = test_issuer();
        let signed = fresh_signed(&issuer, &pubkey, [0x73; 16]);
        let mut gate = make_ghostbridge_interceptor(Arc::new(AssertionValidator::new(
            trust_store_for_issuer(&issuer),
        )));
        let mut req = request_with_connect_info(test_ip());
        insert_assertion_metadata(&mut req, &signed.to_wire());
        insert_sealed_id_metadata(&mut req, &wire);
        let status = gate.call(req).unwrap_err();
        assert_eq!(status.code(), Code::Unauthenticated);
        assert!(status.message().contains("exactly one identity credential"));
    }

    pub async fn sid1_requires_current_principal_registration_impl() {
        let _cozo = temp_cozo();
        let (engine, _shm) = sled_engine();
        let pubkey = pk(0x74);
        let identity = write_identity(engine.as_ref(), &pubkey)
            .await
            .expect("write");
        set_engine(engine);
        let sealed = identity.sealed_id.expect("sid1");
        let wire = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(
                sealed
                    .strip_prefix(op_identity::sealed_id::INLINE_PREFIX)
                    .unwrap(),
            )
            .expect("sid1 b64");
        let request = || {
            let mut req = Request::new(());
            insert_sealed_id_metadata(&mut req, &wire);
            req
        };
        let mut gate = make_ghostbridge_interceptor(validator_for_tests());
        assert_eq!(
            gate.call(request()).unwrap_err().code(),
            Code::Unauthenticated
        );
        register(&pubkey, "sid1-revocation")
            .await
            .expect("register");
        assert!(gate.call(request()).is_ok());
        crate::human_principal_dispatch::tests::revoke(&pubkey)
            .await
            .expect("revoke");
        assert_eq!(
            gate.call(request()).unwrap_err().code(),
            Code::PermissionDenied
        );
    }

    /// VAL-BRIDGE-027
    pub async fn human_principal_grant_allows_capability_gate_impl() {
        let _cozo = temp_cozo();
        let pubkey = pk(44);
        register(&pubkey, "grant").await.expect("register");
        let principal_id = derive_principal_id(&pubkey);
        let schema = serde_json::json!({
            "methods": {
                "resolve_key": { "required_capability": "human_principal.read" }
            },
            "capability_grants": {
                "*": ["human_principal.read"]
            }
        });
        let _grants = install_grants(serde_json::json!({
            principal_id.clone(): { "capabilities": ["human_principal.read"] }
        }));
        let identity = bridge_capability_identity(&{
            let mut ex = tonic::Extensions::new();
            ex.insert(HumanPrincipalIdentity {
                principal_id,
                human_pubkey: pubkey.clone(),
                session_id: "human-session".to_string(),
                session_genesis: "11".repeat(32),
                expires_at: 1_700_000_300,
            });
            ex
        })
        .expect("identity");
        assert!(crate::grpc_server::enforce_bridge_capability_with_schema(
            Some(&schema),
            "human_principal",
            "resolve_key",
            Some("human_principal.read"),
            Some(&identity),
        )
        .is_ok());
        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::remove_var("OP_GRANTS_PATH") };
    }

    /// VAL-BRIDGE-028
    pub async fn human_principal_missing_grant_denies_capability_gate_impl() {
        let _cozo = temp_cozo();
        let pubkey = pk(45);
        let principal_id = derive_principal_id(&pubkey);
        let schema = serde_json::json!({
            "methods": {
                "resolve_key": { "required_capability": "human_principal.read" }
            },
            "capability_grants": {
                principal_id.clone(): ["human_principal.read"]
            }
        });
        let _grants = install_grants(serde_json::json!({
            "*": { "capabilities": ["human_principal.read"] }
        }));
        let identity = GhostbridgeIdentity {
            principal_id,
            session_id: "human".to_string(),
            session_genesis: "33".repeat(32),
        };
        assert!(crate::grpc_server::enforce_bridge_capability_with_schema(
            Some(&schema),
            "human_principal",
            "resolve_key",
            Some("human_principal.read"),
            Some(&identity),
        )
        .is_err());
        assert!(crate::grpc_server::enforce_bridge_capability_with_schema(
            None,
            "human_principal",
            "resolve_key",
            Some("human_principal.read"),
            Some(&identity),
        )
        .is_err());
        assert!(crate::grpc_server::enforce_bridge_capability_with_schema(
            Some(&schema),
            "human_principal",
            "missing_method",
            Some("human_principal.read"),
            Some(&identity),
        )
        .is_err());
        let missing_metadata = serde_json::json!({
            "methods": { "resolve_key": { "required_capability": null } }
        });
        assert!(crate::grpc_server::enforce_bridge_capability_with_schema(
            Some(&missing_metadata),
            "human_principal",
            "resolve_key",
            Some("human_principal.read"),
            Some(&identity),
        )
        .is_err());
        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::remove_var("OP_GRANTS_PATH") };
    }

    /// VAL-BRIDGE-029
    pub async fn human_identity_shadows_ghostbridge_for_capability_gate_impl() {
        let _cozo = temp_cozo();
        let human_principal = "did:op:human:shadow".to_string();
        let ghost_principal = "did:op:service:ghost".to_string();
        let schema = serde_json::json!({
            "methods": {
                "resolve_key": { "required_capability": "human_principal.read" }
            },
            "capability_grants": {
                ghost_principal.clone(): ["human_principal.read"]
            }
        });
        let _grants = install_grants(serde_json::json!({
            human_principal.clone(): { "capabilities": ["human_principal.read"] }
        }));
        let mut ex = tonic::Extensions::new();
        ex.insert(HumanPrincipalIdentity {
            principal_id: human_principal.clone(),
            human_pubkey: "BwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwc=".to_string(),
            session_id: "human-session".to_string(),
            session_genesis: "22".repeat(32),
            expires_at: 1_700_000_300,
        });
        ex.insert(GhostbridgeIdentity {
            principal_id: ghost_principal,
            session_id: "ghost".to_string(),
            session_genesis: "aa".repeat(32),
        });
        let identity = bridge_capability_identity(&ex).expect("human wins");
        assert_eq!(identity.principal_id, human_principal);
        assert!(crate::grpc_server::enforce_bridge_capability_with_schema(
            Some(&schema),
            "human_principal",
            "resolve_key",
            Some("human_principal.read"),
            Some(&identity),
        )
        .is_ok());
        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::remove_var("OP_GRANTS_PATH") };
    }

    /// VAL-BRIDGE-036
    pub async fn registration_interceptor_factory_respects_bootstrap_impl() {
        let mut gate = make_registration_interceptor(validator_for_tests());
        let mut req = Request::new(());
        req.extensions_mut().insert(tonic::GrpcMethod::new(
            "operation.registration.v1.RegistrationService",
            "SendMagicLink",
        ));
        assert!(gate.call(req).is_ok());
    }

    /// VAL-BRIDGE-037
    pub async fn validator_per_instance_interceptor_isolation_impl() {
        let issuer1 = test_issuer();
        let issuer2 = op_identity::oracle_assertion::DecoyIssuer::new(
            ed25519_dalek::SigningKey::from_bytes(&[9u8; 32]),
            "decoy-key-2",
            std::time::Duration::from_secs(900),
        );
        let v1 = Arc::new(AssertionValidator::new(trust_store_for_issuer(&issuer1)));
        let v2 = Arc::new(AssertionValidator::new(trust_store_for_issuer(&issuer2)));
        assert_ne!(v1.trust_store().key_count(), 0);
        assert_eq!(v2.trust_store().key_count(), 1);
        assert!(v1.trust_store().contains_key("decoy-key-1"));
        assert!(v2.trust_store().contains_key("decoy-key-2"));
    }

    /// VAL-BRIDGE-038
    pub async fn bridge_capability_identity_prefers_human_impl() {
        let mut ex = tonic::Extensions::new();
        ex.insert(HumanPrincipalIdentity {
            principal_id: "did:op:human:pref".to_string(),
            human_pubkey: "pk".to_string(),
            session_id: "human-session".to_string(),
            session_genesis: "38".repeat(32),
            expires_at: 1,
        });
        ex.insert(GhostbridgeIdentity {
            principal_id: "did:op:service:ghost".to_string(),
            session_id: "ghost".to_string(),
            session_genesis: "bb".repeat(32),
        });
        let id = bridge_capability_identity(&ex).unwrap();
        assert_eq!(id.principal_id, "did:op:human:pref");
        assert_eq!(id.session_id, "human-session");
    }

    /// VAL-BRIDGE-039
    pub async fn assertion_bad_signature_does_not_insert_ghostbridge_impl() {
        let _cozo = temp_cozo();
        let issuer = test_issuer();
        let pubkey = pk(46);
        register(&pubkey, "sig").await.expect("register");
        let mut gate = make_ghostbridge_interceptor(Arc::new(AssertionValidator::new(
            trust_store_for_issuer(&issuer),
        )));
        let mut signed = fresh_signed(&issuer, &pubkey, [0x46; 16]);
        signed.signature[0] ^= 0x01;
        let mut req = request_with_connect_info(test_ip());
        insert_assertion_metadata(&mut req, &signed.to_wire());
        insert_ghostbridge_headers(&mut req, &"aa".repeat(32), "trace");
        let status = gate.call(req).unwrap_err();
        assert_eq!(status.code(), Code::Unauthenticated);
        assert!(status.message().contains("BadSignature"));
    }
}

/// Load the capabilities granted to one registered principal.
///
/// The JSON document is keyed by exact `principal_id` and each value owns a
/// `capabilities` array. Missing, malformed, or unreadable grant state fails
/// closed.
pub fn load_capability_grants(principal_id: &str) -> std::collections::HashSet<String> {
    let path = std::env::var("OP_GRANTS_PATH")
        .unwrap_or_else(|_| "/dev/shm/opdbus/capability-grants.json".to_string());
    let Ok(bytes) = std::fs::read(path) else {
        return std::collections::HashSet::new();
    };
    let Ok(document) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        return std::collections::HashSet::new();
    };
    capabilities_for_principal(&document, principal_id)
}

/// Parse one principal's grants from a fully principal-bound document.
///
/// A wildcard or legacy 64-hex footprint key invalidates the entire document.
/// This prevents a partially migrated authority file from appearing safe merely
/// because an individual lookup happened to use an exact principal ID.
pub(crate) fn capabilities_for_principal(
    document: &serde_json::Value,
    principal_id: &str,
) -> std::collections::HashSet<String> {
    if principal_id.is_empty() || principal_id == "*" {
        return std::collections::HashSet::new();
    }
    let Some(entries) = document.as_object() else {
        return std::collections::HashSet::new();
    };
    if entries.keys().any(|key| {
        key == "*"
            || (key.len() == 64 && key.as_bytes().iter().all(|byte| byte.is_ascii_hexdigit()))
    }) {
        return std::collections::HashSet::new();
    }
    document
        .get(principal_id)
        .and_then(|entry| entry.get("capabilities"))
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(serde_json::Value::as_str)
        .map(str::to_string)
        .collect()
}
