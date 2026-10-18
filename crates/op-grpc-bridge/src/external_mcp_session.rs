//! Receiver-authored human MCP sessions, independent of machine/container identity.
//! No SID1 is minted and no container login, storage, or grant is created here.
use anyhow::{Context, Result, ensure};
use base64::Engine as _;
use op_cozo_store::ExternalMcpSessionRecord;

use crate::mutation_engine::{MutationEngine, SessionContext};
use crate::oracle_assertion::VerifiedWireGuardOrigin;

#[derive(Clone)]
struct AuthoringScope {
    actor: String,
    context: SessionContext,
}

tokio::task_local! {
    static AUTHORING_SCOPE: Option<AuthoringScope>;
}

/// Only the canonical admitted method owner installs this scope. It is task
/// local, so other origins, native callers and background observers cannot
/// replace the current request's secondary event attribution.
pub(crate) async fn with_authoring_scope<F: std::future::Future>(
    engine: &MutationEngine,
    actor: &str,
    session: Option<&str>,
    genesis: Option<&str>,
    future: F,
) -> Result<F::Output> {
    // Native SID1 callers retain their authoritative sled resolver even when
    // their transport validated without populating the engine cache.
    let scope = if let (Some(session), Some(genesis)) = (session, genesis) {
        match engine.session_context(session).await {
            Some(cached) if cached.sealed_id_digest.is_none() => {
                if verify_connected_session(engine, actor, session, Some(genesis)).await?.is_some() {
                    Some(AuthoringScope { actor: actor.into(), context: cached })
                } else {
                    None
                }
            }
            _ => None,
        }
    } else {
        None
    };
    Ok(AUTHORING_SCOPE.scope(scope, future).await)
}

/// An explicit task scope with another actor never substitutes its identity.
pub(crate) fn scoped_context(actor: &str) -> Option<SessionContext> {
    AUTHORING_SCOPE
        .try_with(|scope| {
            scope
                .as_ref()
                .filter(|scope| scope.actor == actor)
                .map(|scope| scope.context.clone())
        })
        .ok()
        .flatten()
}

/// Generic human session validation. A durable anchor alone is not a
/// credential: the receiver must have registered this exact connected scope.
pub(crate) async fn verify_connected_session(
    engine: &MutationEngine,
    actor: &str,
    session: &str,
    genesis: Option<&str>,
) -> Result<Option<String>> {
    let Some(cached) = engine.session_context(session).await else {
        return Ok(None);
    };
    if cached.sealed_id_digest.is_some() {
        return Ok(None);
    }
    let Some(row) = lookup(session).await? else {
        return Ok(None);
    };
    ensure!(
        row.principal_id == actor
            && row.genesis == cached.genesis_hex
            && Some(row.genesis.as_str()) == genesis
            && row.peer_key == cached.wireguard_pubkey,
        "external MCP connected session binding is invalid"
    );
    ensure!(
        !crate::interceptor::load_capability_grants(actor).is_empty(),
        "external MCP principal has no capabilities"
    );
    let principal = crate::human_principal_dispatch::resolve_key_for_assertion(&row.peer_key)
        .await
        .map_err(|_| anyhow::anyhow!("external MCP principal registry unavailable"))?
        .context("external MCP principal is not registered")?;
    ensure!(
        principal.revoked_at == 0
            && principal.principal_id == actor
            && principal.human_pubkey == row.peer_key,
        "external MCP principal revoked or mismatched"
    );
    Ok(Some(row.session_id))
}

fn session_id(origin: &VerifiedWireGuardOrigin) -> String {
    ExternalMcpSessionRecord::canonical_session_id(
        origin.principal_id(),
        origin.peer_key(),
        origin.issuer_key_id(),
        &origin.allowed_ip_scope(),
    )
}

fn context(row: ExternalMcpSessionRecord) -> SessionContext {
    SessionContext {
        genesis_hex: row.genesis,
        session_id: row.session_id,
        wireguard_pubkey: row.peer_key,
        sealed_id_digest: None,
    }
}

/// Called only after signature/origin verification; nonce consumption remains
/// with the ingress validator. A stored anchor itself is never a credential.
pub(crate) async fn anchor(
    engine: &MutationEngine,
    origin: &VerifiedWireGuardOrigin,
) -> Result<SessionContext> {
    ensure!(
        origin.expires_at() >= chrono::Utc::now().timestamp(),
        "external MCP origin expired"
    );
    ensure!(
        !crate::interceptor::load_capability_grants(origin.principal_id()).is_empty(),
        "external MCP principal has no capabilities"
    );
    let principal = crate::human_principal_dispatch::resolve_key_for_assertion(origin.peer_key())
        .await
        .map_err(|_| anyhow::anyhow!("external MCP principal registry unavailable"))?
        .context("external MCP principal is not registered")?;
    ensure!(
        principal.revoked_at == 0
            && principal.principal_id == origin.principal_id()
            && principal.human_pubkey == origin.peer_key(),
        "external MCP principal revoked or mismatched"
    );
    let sid = session_id(origin);
    let store = crate::human_principal_dispatch::external_mcp_store()?;
    let read_store = store.clone();
    let read_sid = sid.clone();
    let existing =
        tokio::task::spawn_blocking(move || read_store.get_external_mcp_session(&read_sid))
            .await??;
    let proposed = if let Some(existing) = existing {
        ensure!(
            existing.principal_id == origin.principal_id()
                && existing.peer_key == origin.peer_key()
                && existing.issuer_key_id == origin.issuer_key_id()
                && existing.allowed_ip_scope == origin.allowed_ip_scope()
                && existing.origin == origin.origin(),
            "external MCP origin binding changed"
        );
        existing
    } else {
        let key = base64::engine::general_purpose::STANDARD.decode(origin.peer_key())?;
        let key: [u8; 32] = key
            .try_into()
            .map_err(|_| anyhow::anyhow!("external MCP peer key length invalid"))?;
        ensure!(
            base64::engine::general_purpose::STANDARD.encode(key) == origin.peer_key(),
            "external MCP peer key is not canonical"
        );
        let (chain_head, head_timestamp) = engine.external_mcp_chain_head().await;
        let chain_bytes = hex::decode(&chain_head)?;
        ensure!(
            matches!(chain_bytes.len(), 16 | 32),
            "external MCP chain digest width invalid"
        );
        let catalog_dir = crate::sealed_schema_reader::blob_catalog_dir_from_env();
        let catalog = tokio::task::spawn_blocking(move || {
            crate::sealed_schema_reader::read_manifest_pinned_catalog(&catalog_dir)
        })
        .await??;
        let catalog_bytes: [u8; 32] = hex::decode(&catalog.catalog_hash)?
            .try_into()
            .map_err(|_| anyhow::anyhow!("external MCP catalog hash invalid"))?;
        let created_at = chrono::Utc::now().timestamp();
        let genesis = op_identity::session_genesis::mint_genesis(
            &key,
            &chain_bytes,
            head_timestamp,
            &catalog_bytes,
            created_at,
        );
        ExternalMcpSessionRecord {
            session_id: sid,
            principal_id: origin.principal_id().into(),
            peer_key: origin.peer_key().into(),
            origin: origin.origin(),
            issuer_key_id: origin.issuer_key_id().into(),
            allowed_ip_scope: origin.allowed_ip_scope(),
            signed_inner_ip: origin.signed_inner_ip().to_string(),
            first_assertion_digest: origin.assertion_digest().into(),
            genesis: hex::encode(genesis),
            created_at,
            chain_head,
            catalog_hash: catalog.catalog_hash,
            head_timestamp,
        }
    };
    // The durable owner retains the first immutable genesis under concurrent
    // arrivals. Registry revocation is independently rechecked after persistence.
    let retained =
        tokio::task::spawn_blocking(move || store.get_or_create_external_mcp_session(&proposed))
            .await??;
    ensure!(
        !crate::interceptor::load_capability_grants(origin.principal_id()).is_empty(),
        "external MCP principal capabilities were revoked"
    );
    let current = crate::human_principal_dispatch::resolve_key_for_assertion(origin.peer_key())
        .await
        .map_err(|_| anyhow::anyhow!("external MCP principal registry unavailable"))?
        .context("external MCP principal is not registered")?;
    ensure!(
        current.revoked_at == 0
            && current.principal_id == origin.principal_id()
            && current.human_pubkey == origin.peer_key(),
        "external MCP principal revoked or mismatched"
    );
    // The ingress registers this context only after nonce consumption succeeds.
    Ok(context(retained))
}

pub(crate) async fn lookup(session_id: &str) -> Result<Option<ExternalMcpSessionRecord>> {
    let store = crate::human_principal_dispatch::external_mcp_store()?;
    let session = session_id.to_string();
    Ok(tokio::task::spawn_blocking(move || store.get_external_mcp_session(&session)).await??)
}

/// Resolve only a uniquely connected external scope. Never pick an arbitrary
/// origin when the same human has several active scopes.
pub(crate) async fn context_for_actor(
    engine: &MutationEngine,
    actor: &str,
) -> Option<SessionContext> {
    let mut matched = Vec::new();
    for session in engine.connected_session_ids().await {
        let cached = engine.session_context(&session).await?;
        if cached.sealed_id_digest.is_none()
            && op_identity::session::derive_principal_id(&cached.wireguard_pubkey) == actor
            && let Ok(Some(row)) = lookup(&session).await
            && row.principal_id == actor
            && row.genesis == cached.genesis_hex
        {
            let principal =
                crate::human_principal_dispatch::resolve_key_for_assertion(&row.peer_key)
                    .await
                    .ok()??;
            if principal.revoked_at == 0 && principal.principal_id == actor {
                matched.push(cached);
            }
        }
    }
    (matched.len() == 1).then(|| matched.remove(0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::human_principal_dispatch::tests::{pk, temp_shm, test_engine};
    use crate::oracle_assertion::AssertionSourceBinding;
    use crate::oracle_assertion::tests::{
        signed_with_fields, test_issuer, validator_with_registered,
    };

    struct GrantsEnvironment(Option<std::ffi::OsString>);
    impl Drop for GrantsEnvironment {
        fn drop(&mut self) {
            match self.0.take() {
                Some(value) => unsafe { std::env::set_var("OP_GRANTS_PATH", value) },
                None => unsafe { std::env::remove_var("OP_GRANTS_PATH") },
            }
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn verified_peer_anchor_is_container_free_and_denies_missing_grant_or_expiry() {
        let issuer = test_issuer();
        let key = pk(207);
        // This existing fixture holds the registry environment lock for the
        // complete test, including the shared strict principal store.
        let (_registry, validator) = validator_with_registered(&issuer, &key).await;
        let _shm = temp_shm();
        let validator =
            validator.with_source_binding(AssertionSourceBinding::TrustedDecoySignature);
        let engine = test_engine();
        let grants = tempfile::tempdir().unwrap();
        let grants_path = grants.path().join("grants.json");
        let _environment = GrantsEnvironment(std::env::var_os("OP_GRANTS_PATH"));
        unsafe { std::env::set_var("OP_GRANTS_PATH", &grants_path) };
        let now = chrono::Utc::now().timestamp();
        let proof = signed_with_fields(
            &issuer,
            &key,
            "10.0.0.5".parse().unwrap(),
            now - 1,
            now + 300,
            [73; 16],
            None,
        );
        let pending = validator
            .validate_pending_for_mcp(&proof.to_wire(), None, now)
            .unwrap();
        let origin = pending.verified_origin().clone();
        std::fs::write(&grants_path, "{}").unwrap();
        assert!(
            anchor(&engine, &origin)
                .await
                .unwrap_err()
                .to_string()
                .contains("no capabilities")
        );
        assert!(lookup(&session_id(&origin)).await.unwrap().is_none());
        std::fs::write(
            &grants_path,
            serde_json::to_vec(&serde_json::json!({
                origin.principal_id(): {"capabilities": ["cognitive.read"]}
            }))
            .unwrap(),
        )
        .unwrap();
        let anchored = anchor(&engine, &origin).await.unwrap();
        assert!(anchored.sealed_id_digest.is_none());
        assert_ne!(
            anchored.session_id,
            op_identity::session::derive_session_id(&key)
        );
        assert!(
            engine.session_context(&anchored.session_id).await.is_none(),
            "cache publication belongs after nonce consumption"
        );
        assert!(
            engine.logged_in_principal_sessions(300).await.is_empty(),
            "external admission must not mark a container login"
        );
        let row = lookup(&anchored.session_id).await.unwrap().unwrap();
        assert_eq!(row.genesis, anchored.genesis_hex);
        assert_eq!(row.signed_inner_ip, "10.0.0.5");
        assert_eq!(row.first_assertion_digest, origin.assertion_digest());
        assert!(
            verify_connected_session(
                &engine,
                origin.principal_id(),
                &anchored.session_id,
                Some(&anchored.genesis_hex)
            )
            .await
            .unwrap()
            .is_none(),
            "durable anchor alone is not a connected credential"
        );
        validator.consume_pending(pending).unwrap();
        engine.register_external_mcp_context(anchored.clone()).await;
        with_authoring_scope(&engine, origin.principal_id(), Some(&anchored.session_id),
            Some(&anchored.genesis_hex), async {
                assert_eq!(scoped_context(origin.principal_id()), Some(anchored.clone()));
                assert!(scoped_context("other-principal").is_none());
            }).await.unwrap();
        assert!(with_authoring_scope(&engine, origin.principal_id(), Some(&anchored.session_id),
            Some("wrong-genesis"), async {}).await.is_err());
        assert!(with_authoring_scope(&engine, "other-principal", Some(&anchored.session_id),
            Some(&anchored.genesis_hex), async {}).await.is_err());
        assert_eq!(
            crate::project_dispatch::verified_session(
                &engine,
                origin.principal_id(),
                Some(&anchored.session_id),
                Some(&anchored.genesis_hex)
            )
            .await
            .unwrap(),
            anchored.session_id
        );
        assert!(
            crate::project_dispatch::verified_session(
                &engine,
                "another-principal",
                Some(&anchored.session_id),
                Some(&anchored.genesis_hex)
            )
            .await
            .is_err()
        );
        assert!(
            crate::project_dispatch::verified_session(
                &engine,
                origin.principal_id(),
                Some(&anchored.session_id),
                Some("wrong-genesis")
            )
            .await
            .is_err()
        );
        let encoded = serde_json::to_value(&row).unwrap();
        for field in ["sealed_id", "instance", "btrfs_device", "active"] {
            assert!(encoded.get(field).is_none());
        }
        let expired = signed_with_fields(
            &issuer,
            &key,
            "10.0.0.5".parse().unwrap(),
            now - 10,
            now - 1,
            [74; 16],
            None,
        );
        // Validator's leeway can accept a just-expired signature; the anchor
        // refuses using it to establish or reuse an external session.
        let expired = validator
            .validate_pending_for_mcp(&expired.to_wire(), None, now - 2)
            .unwrap();
        assert!(
            anchor(&engine, expired.verified_origin())
                .await
                .unwrap_err()
                .to_string()
                .contains("expired")
        );
        std::fs::write(&grants_path, "{}").unwrap();
        assert!(
            anchor(&engine, &origin).await.is_err(),
            "existing anchor does not preserve revoked grants"
        );
        assert!(
            crate::project_dispatch::verified_session(
                &engine,
                origin.principal_id(),
                Some(&anchored.session_id),
                Some(&anchored.genesis_hex)
            )
            .await
            .is_err(),
            "connected session does not preserve revoked grants"
        );
    }
    #[tokio::test]
    async fn admitted_authoring_scope_separates_origins_and_native_context() {
        let engine = test_engine();
        let key = pk(209);
        let actor = op_identity::session::derive_principal_id(&key);
        let first = SessionContext {
            session_id: "external-scope-one".into(),
            genesis_hex: "one".into(),
            wireguard_pubkey: key.clone(),
            sealed_id_digest: None,
        };
        let second = SessionContext {
            session_id: "external-scope-two".into(),
            genesis_hex: "two".into(),
            wireguard_pubkey: key.clone(),
            sealed_id_digest: None,
        };
        let native = SessionContext {
            session_id: op_identity::session::derive_session_id(&key),
            genesis_hex: "native".into(),
            wireguard_pubkey: key,
            sealed_id_digest: Some("native-sid1-digest".into()),
        };
        for context in [&first, &second, &native] {
            engine.register_external_mcp_context(context.clone()).await;
        }
        let check = |expected: SessionContext| {
            let engine = &engine;
            let actor = &actor;
            async move {
                AUTHORING_SCOPE.scope(Some(AuthoringScope {
                    actor: actor.clone(), context: expected.clone(),
                }), async {
                    tokio::task::yield_now().await;
                    assert_eq!(engine.session_context_for_actor(actor).await, Some(expected.clone()));
                    assert!(scoped_context("other-principal").is_none());
                }).await;
            }
        };
        tokio::join!(check(first.clone()), check(second.clone()));
        assert!(scoped_context(&actor).is_none(), "completed task scopes never leak");
        assert_eq!(engine.session_context(&native.session_id).await, Some(native.clone()));
        with_authoring_scope(&engine, &actor, Some(&native.session_id), Some("native"),
            async { assert!(scoped_context(&actor).is_none()); }).await.unwrap();
        with_authoring_scope(&engine, &actor, Some("native-without-cache"), Some("genesis"),
            async { assert!(scoped_context(&actor).is_none()); }).await.unwrap();
    }
}
