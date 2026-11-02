//! SID1 receiver over the committed private identity projection.
//! This does not admit an RPC, evaluate grants, or handle OIA1 assertions.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use base64::Engine as _;
use op_identity::sealed_id::SealedId;
use op_plugins::state_plugins::identity_sled::{ContainerIdentitySled, IdentitySledState};
use op_state_store::FieldSchema;
use serde::Deserialize;

use crate::catalog::Catalog;

/// SID1 verified against committed state, its binding and the active principal registry.
#[derive(Clone)]
pub struct VerifiedSid1(SealedId);

pub(crate) struct BoundSid1(SealedId);

impl BoundSid1 {
    pub(crate) fn claims(&self) -> &SealedId {
        &self.0
    }
}

impl std::fmt::Debug for VerifiedSid1 {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("VerifiedSid1([redacted])")
    }
}

impl VerifiedSid1 {
    pub fn claims(&self) -> &SealedId {
        &self.0
    }
}

#[derive(Deserialize)]
struct CompiledIdentityContract {
    version: String,
    fields: HashMap<String, FieldSchema>,
}

fn pinned_identity(dir: &Path) -> anyhow::Result<crate::catalog::PluginEntry> {
    let pinned = Catalog::sealed_plugin(dir, "identity_sled")?;
    let schema: op_state_store::PluginSchema = serde_json::from_value(
        pinned
            .schema
            .clone()
            .ok_or_else(|| anyhow::anyhow!("sealed identity schema absent"))?,
    )?;
    let compiled: CompiledIdentityContract = serde_json::from_str(include_str!(concat!(
        env!("OUT_DIR"),
        "/identity_state_contract.json"
    )))?;
    anyhow::ensure!(
        schema.version == compiled.version && schema.fields == compiled.fields,
        "compiled and sealed identity state contracts differ"
    );
    Ok(pinned)
}

fn committed_session(
    catalog_dir: &Path,
    state_dir: &str,
    session_id: &str,
) -> anyhow::Result<Option<ContainerIdentitySled>> {
    let before = pinned_identity(catalog_dir)?;
    let bytes =
        op_core::projection_shm::read_credential_projection_bytes_in(state_dir, "identity_sled")?
            .ok_or_else(|| anyhow::anyhow!("committed identity credential projection is absent"))?;
    let after = pinned_identity(catalog_dir)?;
    anyhow::ensure!(
        before.schema_hash == after.schema_hash
            && before.sealed_schema_json == after.sealed_schema_json,
        "identity plugin seal changed during credential state access"
    );
    let value: serde_json::Value = serde_json::from_slice(&bytes)?;
    anyhow::ensure!(
        value.get("sleds").is_some_and(serde_json::Value::is_array),
        "identity sleds missing"
    );
    anyhow::ensure!(
        value.get("events").is_some_and(serde_json::Value::is_array),
        "identity events missing"
    );
    let state: IdentitySledState = serde_json::from_value(value)?;
    let mut ids = HashSet::new();
    for sled in &state.sleds {
        anyhow::ensure!(
            ids.insert(sled.session_id.as_str()),
            "duplicate identity in SHM"
        );
        crate::identity_binding::validate_sled(sled)?;
    }
    let mut events = HashSet::new();
    for event in &state.events {
        anyhow::ensure!(
            events.insert((event.session_id.as_str(), event.seq)),
            "duplicate identity event in SHM"
        );
    }
    Ok(state
        .sleds
        .into_iter()
        .find(|sled| sled.session_id == session_id))
}

pub(crate) fn verify_in(
    catalog_dir: &Path,
    state_dir: &str,
    wire: &[u8],
    transport: &str,
    now: i64,
) -> anyhow::Result<BoundSid1> {
    let claims =
        crate::sid1_checks::parse_for_transport(wire, transport).map_err(anyhow::Error::msg)?;
    let sled = committed_session(catalog_dir, state_dir, &claims.session_id)?
        .ok_or_else(|| anyhow::anyhow!("sealed identity session was not found"))?;
    let claims = crate::sid1_checks::match_bound_session(wire, &sled, transport, now)
        .map_err(anyhow::Error::msg)?;
    Ok(BoundSid1(claims))
}

/// Find the authored SID1 for an already anchored assertion key. This never
/// mints or changes an identity, and reads only the committed private projection.
pub(crate) fn bound_session_for_key_in(
    catalog_dir: &Path,
    state_dir: &str,
    key: &str,
    now: i64,
) -> anyhow::Result<BoundSid1> {
    let session_id = op_identity::session::derive_session_id(key);
    let sled = committed_session(catalog_dir, state_dir, &session_id)?
        .ok_or_else(|| anyhow::anyhow!("assertion has no committed identity session"))?;
    let encoded = sled
        .sealed_id
        .as_deref()
        .and_then(|value| value.strip_prefix(op_identity::sealed_id::INLINE_PREFIX))
        .ok_or_else(|| anyhow::anyhow!("committed identity credential is unavailable"))?;
    let wire = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(encoded)?;
    let claims = crate::sid1_checks::match_bound_session(&wire, &sled, "grpc", now)
        .map_err(anyhow::Error::msg)?;
    anyhow::ensure!(
        claims.wireguard_pubkey == key,
        "assertion key does not match committed identity"
    );
    Ok(BoundSid1(claims))
}

fn verify_registered(
    bound: BoundSid1,
    record: Option<op_cozo_store::HumanPrincipalRecord>,
) -> anyhow::Result<VerifiedSid1> {
    let record = record.ok_or_else(|| anyhow::anyhow!("session has no registered principal"))?;
    anyhow::ensure!(record.revoked_at == 0, "registered principal is revoked");
    anyhow::ensure!(
        !record.principal_id.is_empty()
            && record.principal_id == bound.0.principal_id
            && record.human_pubkey == bound.0.wireguard_pubkey,
        "sealed identity does not match the registered principal"
    );
    Ok(VerifiedSid1(bound.0))
}

pub(crate) async fn verify_bound_registry(bound: BoundSid1) -> anyhow::Result<VerifiedSid1> {
    let record =
        crate::human_principal_registry::resolve_key_for_assertion(&bound.0.wireguard_pubkey)
            .await
            .map_err(|_| anyhow::anyhow!("human principal registry unavailable"))?;
    verify_registered(bound, record)
}

#[cfg(test)]
pub(crate) fn verify_registered_for_test(
    bound: BoundSid1,
    record: Option<op_cozo_store::HumanPrincipalRecord>,
) -> anyhow::Result<VerifiedSid1> {
    verify_registered(bound, record)
}

/// Verify SID1 against the sealed contract, private SHM, and active principal registry.
/// Callers must still check the grant for each requested method and revalidate
/// long-lived subscriptions as appropriate.
pub async fn verify_sid1(wire: &[u8], transport: &str) -> anyhow::Result<VerifiedSid1> {
    let catalog_dir = std::env::var_os("OP_BLOB_CATALOG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(op_blob::catalog::DEFAULT_SHM_DIR));
    let state_dir = op_core::projection_shm::shm_state_dir();
    let wire = wire.to_vec();
    let transport = transport.to_owned();
    let bound = tokio::task::spawn_blocking(move || {
        verify_in(
            &catalog_dir,
            &state_dir,
            &wire,
            &transport,
            chrono::Utc::now().timestamp(),
        )
    })
    .await??;
    verify_bound_registry(bound).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine as _;
    use op_state::StatePlugin;

    #[test]
    fn receiver_requires_pinned_contract_and_committed_bound_identity() {
        let dir = tempfile::tempdir().unwrap();
        let catalog_dir = dir.path().join("catalog");
        let state_dir = dir.path().join("state");
        let state_dir = state_dir.to_str().unwrap();
        let key = "fixture-key".to_string();
        let claims = SealedId {
            principal_id: op_identity::session::derive_principal_id(&key),
            principal_kind: "wireguard-principal".into(),
            session_id: op_identity::session::derive_session_id(&key),
            wireguard_pubkey: key,
            session_genesis: "genesis".into(),
            trace_id: "trace".into(),
            schema_version: 1,
            issued_at: 100,
            expires_at: 2_000_000_000,
            arrival_timestamp: 100,
            chain_head_at_arrival: "chain".into(),
            catalog_hash_at_arrival: "catalog".into(),
            head_timestamp_at_arrival: 99,
            transport_scope: "grpc".into(),
            nextdns: None,
        };
        let wire = claims.seal().unwrap();
        assert!(verify_in(&catalog_dir, state_dir, &wire, "grpc", 150).is_err());
        let mut schema = op_plugins::state_plugins::identity_sled::IdentitySledPlugin::new()
            .schema()
            .unwrap();
        let mut catalog = op_blob::BlobStore::open(&catalog_dir).unwrap();
        catalog
            .write(&op_blob::blobify_plugin_schema(
                "identity_sled",
                schema.clone(),
            ))
            .unwrap();
        assert!(verify_in(&catalog_dir, state_dir, &wire, "grpc", 150).is_err());
        let mut state = serde_json::json!({"sleds": [{
            "session_id": claims.session_id, "wireguard_pubkey": claims.wireguard_pubkey,
            "principal_kind": "human", "genesis": claims.session_genesis, "trace_id": claims.trace_id,
            "schema_version": claims.schema_version, "expires_at": claims.expires_at,
            "arrival_timestamp": claims.arrival_timestamp, "chain_head_at_arrival": claims.chain_head_at_arrival,
            "catalog_hash_at_arrival": claims.catalog_hash_at_arrival, "head_timestamp_at_arrival": claims.head_timestamp_at_arrival,
            "sealed_id": format!("{}{}", op_identity::sealed_id::INLINE_PREFIX,
                base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&wire))
        }], "events": []});
        op_core::projection_shm::update_projection_with_credentials_in(
            state_dir,
            "identity_sled",
            nix::unistd::getgid().as_raw(),
            |_| {
                Ok(Some((
                    br#"{"sleds":[],"events":[]}"#.to_vec(),
                    serde_json::to_vec(&state)?,
                )))
            },
        )
        .unwrap();
        let bound = verify_in(&catalog_dir, state_dir, &wire, "grpc", 150).unwrap();
        let record = op_cozo_store::HumanPrincipalRecord {
            principal_id: bound.0.principal_id.clone(),
            human_pubkey: bound.0.wireguard_pubkey.clone(),
            display_alias: String::new(),
            registered_at: 1,
            revoked_at: 0,
        };
        assert!(
            verify_registered(
                verify_in(&catalog_dir, state_dir, &wire, "grpc", 150).unwrap(),
                None
            )
            .is_err()
        );
        let mut revoked = record.clone();
        revoked.revoked_at = 1;
        assert!(
            verify_registered(
                verify_in(&catalog_dir, state_dir, &wire, "grpc", 150).unwrap(),
                Some(revoked)
            )
            .is_err()
        );
        let mut mismatch = record.clone();
        mismatch.principal_id = "wrong-principal".into();
        assert!(
            verify_registered(
                verify_in(&catalog_dir, state_dir, &wire, "grpc", 150).unwrap(),
                Some(mismatch)
            )
            .is_err()
        );
        let registry = op_cozo_store::CozoGraphShuttle::new_in_memory().unwrap();
        registry.put_human_principal(&record).unwrap();
        let registered = registry
            .get_human_principal_by_pubkey(&record.human_pubkey)
            .unwrap();
        let verified = verify_registered(bound, registered).unwrap();
        let method_schema = op_state_store::PluginSchema::builder("fixture")
            .method(op_state_store::MethodDecl {
                name: "Read".into(),
                args: simd_json::json!({"type":"object"}),
                returns: None,
                side_effect: op_state_store::SideEffect::Read,
                idempotent: true,
                required_capability: Some("cap.fixture.read@v1".into()),
                subid: "obs.service.fixture.read@v1".into(),
            })
            .build();
        catalog
            .write(&op_blob::blobify_plugin_schema("fixture", method_schema))
            .unwrap();
        let grants = serde_json::json!({(verified.claims().principal_id.clone()): {"capabilities": ["cap.fixture.read@v1"]}});
        assert!(
            crate::admission::require_method_grant(
                &verified,
                &catalog_dir,
                "fixture",
                "Read",
                Some("cap.fixture.read@v1"),
                &grants
            )
            .is_ok()
        );
        assert!(
            crate::admission::require_method_grant(
                &verified,
                &catalog_dir,
                "fixture",
                "Read",
                Some("cap.other@v1"),
                &grants
            )
            .is_err()
        );
        assert!(
            crate::admission::require_method_grant(
                &verified,
                &catalog_dir,
                "fixture",
                "Read",
                Some("cap.fixture.read@v1"),
                &serde_json::json!({})
            )
            .is_err()
        );
        assert!(
            crate::admission::require_method_grant(
                &verified,
                &catalog_dir,
                "fixture",
                "Read",
                Some("cap.fixture.read@v1"),
                &serde_json::json!({"*": {"capabilities": ["cap.fixture.read@v1"]}})
            )
            .is_err()
        );
        assert!(
            crate::admission::require_method_grant(
                &verified,
                &catalog_dir,
                "fixture",
                "Missing",
                Some("cap.fixture.read@v1"),
                &grants
            )
            .is_err()
        );
        let grants_path = dir.path().join("grants.json");
        std::fs::write(&grants_path, serde_json::to_vec(&grants).unwrap()).unwrap();
        assert!(
            crate::admission::check_sid1_method_in(
                &catalog_dir,
                state_dir,
                &grants_path,
                &wire,
                "fixture",
                "Read",
                Some("cap.fixture.read@v1"),
                150,
                Some(record.clone()),
            )
            .is_ok()
        );
        assert!(
            crate::admission::check_sid1_method_in(
                &catalog_dir,
                state_dir,
                &grants_path,
                &wire,
                "fixture",
                "Read",
                Some("cap.other@v1"),
                150,
                Some(record.clone()),
            )
            .is_err()
        );
        assert!(verify_in(&catalog_dir, state_dir, &wire, "grpc", 2_000_000_000).is_err());
        assert!(verify_in(&catalog_dir, state_dir, &wire, "mcp", 150).is_err());
        state["sleds"][0]["principal_kind"] = serde_json::json!("service");
        op_core::projection_shm::update_projection_with_credentials_in(
            state_dir,
            "identity_sled",
            nix::unistd::getgid().as_raw(),
            |_| {
                Ok(Some((
                    br#"{"sleds":[],"events":[]}"#.to_vec(),
                    serde_json::to_vec(&state)?,
                )))
            },
        )
        .unwrap();
        assert!(verify_in(&catalog_dir, state_dir, &wire, "grpc", 150).is_err());
        assert!(
            crate::admission::check_sid1_method_in(
                &catalog_dir,
                state_dir,
                &grants_path,
                &wire,
                "fixture",
                "Read",
                Some("cap.fixture.read@v1"),
                150,
                Some(record.clone()),
            )
            .is_err()
        );
        state["sleds"][0]["principal_kind"] = serde_json::json!("human");
        op_core::projection_shm::update_projection_with_credentials_in(
            state_dir,
            "identity_sled",
            nix::unistd::getgid().as_raw(),
            |_| {
                Ok(Some((
                    br#"{"sleds":[],"events":[]}"#.to_vec(),
                    serde_json::to_vec(&state)?,
                )))
            },
        )
        .unwrap();
        assert!(verify_in(&catalog_dir, state_dir, &wire, "grpc", 150).is_ok());
        schema.version.push_str("-drift");
        catalog
            .write(&op_blob::blobify_plugin_schema("identity_sled", schema))
            .unwrap();
        assert!(verify_in(&catalog_dir, state_dir, &wire, "grpc", 150).is_err());
    }
}
