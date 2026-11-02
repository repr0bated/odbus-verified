//! Bind an OIA1 assertion to an existing, committed SID1 session.
//! This partial receiver never mints identity and does not dispatch requests.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::identity_receiver::VerifiedSid1;
use crate::oracle_assertion::AssertionValidator;

/// An assertion and its existing bound SID1 session. Browser-assigned oracle
/// proofs are not supported by this receiver and fail closed.
pub struct VerifiedOracle {
    identity: VerifiedSid1,
    assertion_expires_at: i64,
}

impl VerifiedOracle {
    pub fn identity(&self) -> &VerifiedSid1 {
        &self.identity
    }
    pub fn assertion_expires_at(&self) -> i64 {
        self.assertion_expires_at
    }
}

impl std::fmt::Debug for VerifiedOracle {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("VerifiedOracle([redacted])")
    }
}

async fn verify_in(
    validator: Arc<AssertionValidator>,
    wire: &[u8],
    source: Option<SocketAddr>,
    catalog_dir: &Path,
    state_dir: &str,
    now: i64,
) -> anyhow::Result<VerifiedOracle> {
    let assertion_wire = wire.to_vec();
    let validator_for_check = validator.clone();
    let pending = tokio::task::spawn_blocking(move || {
        validator_for_check.validate_pending_with_bootstrap(&assertion_wire, source, now, false)
    })
    .await??;
    anyhow::ensure!(
        pending.bound_oracle().is_none(),
        "assigned browser oracle is not supported by this receiver"
    );
    let assertion = pending.identity().clone();
    let key = assertion.human_pubkey.clone();
    let catalog_dir = catalog_dir.to_owned();
    let state_dir = state_dir.to_owned();
    let bound = tokio::task::spawn_blocking(move || {
        crate::identity_receiver::bound_session_for_key_in(&catalog_dir, &state_dir, &key, now)
    })
    .await??;
    anyhow::ensure!(
        bound.claims().principal_id == assertion.principal_id
            && bound.claims().wireguard_pubkey == assertion.human_pubkey,
        "assertion does not match the committed identity"
    );
    let identity = crate::identity_receiver::verify_bound_registry(bound).await?;
    anyhow::ensure!(
        assertion.expires_at > chrono::Utc::now().timestamp(),
        "assertion expired during verification"
    );
    let validator_for_consume = validator.clone();
    tokio::task::spawn_blocking(move || validator_for_consume.consume_pending(pending)).await??;
    Ok(VerifiedOracle {
        identity,
        assertion_expires_at: assertion.expires_at,
    })
}

/// Validate an OIA1 against the configured validator and an existing SID1.
/// `source` must be derived from the accepted transport, never caller metadata.
/// The caller must enforce grants, compiled handler compatibility, argument
/// validation and effect-boundary revalidation before dispatch.
pub async fn verify_assertion(
    validator: Arc<AssertionValidator>,
    wire: &[u8],
    source: Option<SocketAddr>,
) -> anyhow::Result<VerifiedOracle> {
    let catalog_dir = std::env::var_os("OP_BLOB_CATALOG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(op_blob::catalog::DEFAULT_SHM_DIR));
    let state_dir = op_core::projection_shm::shm_state_dir();
    verify_in(
        validator,
        wire,
        source,
        &catalog_dir,
        &state_dir,
        chrono::Utc::now().timestamp(),
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine as _;
    use op_identity::sealed_id::SealedId;
    use op_state::StatePlugin;

    #[tokio::test(flavor = "multi_thread")]
    async fn assertion_requires_existing_session_and_consumes_nonce_after_binding() {
        let issuer = crate::oracle_assertion::tests::test_issuer();
        let key = crate::oracle_test_support::pk(77);
        let (_registry, validator) =
            crate::oracle_assertion::tests::validator_with_registered(&issuer, &key).await;
        let validator = Arc::new(validator);
        let dir = tempfile::tempdir().unwrap();
        let catalog_dir = dir.path().join("catalog");
        let state_dir = dir.path().join("state");
        let state_path = state_dir.to_str().unwrap();
        let schema = op_plugins::state_plugins::identity_sled::IdentitySledPlugin::new()
            .schema()
            .unwrap();
        op_blob::BlobStore::open(&catalog_dir)
            .unwrap()
            .write(&op_blob::blobify_plugin_schema("identity_sled", schema))
            .unwrap();
        let now = chrono::Utc::now().timestamp();
        let source_ip = crate::oracle_assertion::tests::test_ip();
        let source = Some(crate::oracle_assertion::tests::source_at(source_ip));
        let assertion = crate::oracle_assertion::tests::signed_with_fields(
            &issuer,
            &key,
            source_ip,
            now - 1,
            now + 300,
            [77; 16],
            None,
        )
        .to_wire();
        assert!(
            verify_in(
                validator.clone(),
                &assertion,
                source,
                &catalog_dir,
                state_path,
                now
            )
            .await
            .is_err()
        );
        assert!(!state_dir.exists(), "missing session must not be minted");
        let claims = SealedId {
            principal_id: op_identity::session::derive_principal_id(&key),
            principal_kind: "wireguard-principal".into(),
            session_id: op_identity::session::derive_session_id(&key),
            wireguard_pubkey: key,
            session_genesis: "genesis".into(),
            trace_id: "trace".into(),
            schema_version: 1,
            issued_at: now - 10,
            expires_at: now + 600,
            arrival_timestamp: now - 10,
            chain_head_at_arrival: "chain".into(),
            catalog_hash_at_arrival: "catalog".into(),
            head_timestamp_at_arrival: now - 11,
            transport_scope: "grpc".into(),
            nextdns: None,
        };
        let wire = claims.seal().unwrap();
        let state = serde_json::json!({"sleds": [{
            "session_id": claims.session_id, "wireguard_pubkey": claims.wireguard_pubkey,
            "principal_kind": "human", "genesis": claims.session_genesis, "trace_id": claims.trace_id,
            "schema_version": claims.schema_version, "expires_at": claims.expires_at,
            "arrival_timestamp": claims.arrival_timestamp, "chain_head_at_arrival": claims.chain_head_at_arrival,
            "catalog_hash_at_arrival": claims.catalog_hash_at_arrival, "head_timestamp_at_arrival": claims.head_timestamp_at_arrival,
            "sealed_id": format!("{}{}", op_identity::sealed_id::INLINE_PREFIX,
                base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&wire))
        }], "events": []});
        op_core::projection_shm::update_projection_with_credentials_in(
            state_path,
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
        let verified = verify_in(
            validator.clone(),
            &assertion,
            source,
            &catalog_dir,
            state_path,
            now,
        )
        .await
        .unwrap();
        assert_eq!(verified.identity().claims().session_id, claims.session_id);
        assert!(
            verify_in(validator, &assertion, source, &catalog_dir, state_path, now)
                .await
                .is_err()
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn assigned_browser_assertion_fails_closed_until_binding_is_supported() {
        let issuer = crate::oracle_assertion::tests::test_issuer();
        let peer = crate::oracle_test_support::pk(78);
        let target = crate::oracle_test_support::pk(79);
        let (_registry, validator) =
            crate::oracle_assertion::tests::validator_with_registered(&issuer, &peer).await;
        crate::oracle_test_support::register(&target, "assigned")
            .await
            .unwrap();
        let validator = Arc::new(validator.with_browser_binding(
            &peer,
            &op_identity::session::derive_principal_id(&target),
            &op_identity::session::derive_session_id(&target),
        ));
        let now = chrono::Utc::now().timestamp();
        let ip = crate::oracle_assertion::tests::test_ip();
        let assertion = crate::oracle_assertion::tests::signed_with_fields(
            &issuer,
            &peer,
            ip,
            now - 1,
            now + 300,
            [78; 16],
            None,
        )
        .to_wire();
        let dir = tempfile::tempdir().unwrap();
        let error = verify_in(
            validator,
            &assertion,
            Some(crate::oracle_assertion::tests::source_at(ip)),
            dir.path(),
            dir.path().to_str().unwrap(),
            now,
        )
        .await
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("assigned browser oracle is not supported")
        );
    }
}
