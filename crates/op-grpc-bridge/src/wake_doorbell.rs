//! The EMQX doorbell: sled-gated, content-free wakes for private views.
//!
//! Owner decisions (2026-09-19): no broker ACL tables, no MCP over MQTT, no
//! authoritative state on the broker; SHM is the one truth. So the broker
//! carries exactly one thing per commit — `{"seq": <revision>}` on
//! `opdbus/wake/<owner-key>` — and every verdict here is a *function of the
//! identity the client presented*, never a lookup in a policy table:
//!
//! * A doorbell client's MQTT client id **is** its owner key
//!   (`sha256(principal, session, genesis)`, the same key the private
//!   episode view uses). The broker never sees a principal, session, or key.
//! * `client.authenticate`: the password is one of the door's two
//!   credentials — the inline SID1 (`sid1:…`, provisioned container/agent
//!   sessions) or the OIA1 assertion the decoy issued (`oia1:<base64>`, the
//!   browser). Each is verified exactly as the door verifies it (sled match
//!   with the `mqtt` transport scope, or signature/lifetime/replay/source
//!   binding plus the anchored human session), must belong to an unrevoked
//!   registered principal, and must derive exactly the client id.
//! * `client.authorize`: a doorbell client may only SUBSCRIBE to its own
//!   topic, and only while some anchored, unexpired sled still derives its
//!   client id (re-checked on every subscribe). Every other client is denied
//!   the `opdbus/` namespace and otherwise left to the broker's own rules.
//!
//! Revocation: tombstoning a sled makes the next subscribe fail, and the
//! bridge kicks the owner key's connection through `emqx.kick_client`.

use std::net::SocketAddr;

use base64::Engine as _;
use op_cozo_store::EpisodeScope;

use crate::mutation_engine::MutationEngine;
use crate::oracle_assertion::AssertionValidator;

/// Password prefix for an OIA1 assertion (standard base64 of the wire, as
/// the decoy issuer hands it to the browser).
pub(crate) const OIA1_PASSWORD_PREFIX: &str = "oia1:";

/// Verdict for the broker: `Ignore` leaves the broker's own decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Verdict {
    Ignore,
    Allow,
    Deny,
}

/// Owner key for a verified scope — identical to the private view's key.
pub(crate) fn owner_key(scope: &EpisodeScope) -> String {
    crate::chatbot_episode_state::EpisodePublisher::key(scope)
}

/// Is this client id shaped like an owner key (and therefore a doorbell)?
pub(crate) fn is_doorbell_client(client_id: &str) -> bool {
    op_plugins::state_plugins::emqx::valid_owner_key(client_id)
}

fn scope_of(
    sled: &op_plugins::state_plugins::identity_sled::ContainerIdentitySled,
) -> Option<EpisodeScope> {
    Some(EpisodeScope {
        principal_id: op_identity::session::derive_principal_id(&sled.wireguard_pubkey),
        session_id: sled.session_id.clone(),
        session_genesis: sled.genesis.clone()?,
    })
}

/// `client.authenticate` for a doorbell client: the password is a door
/// credential and must derive exactly `client_id`.
pub(crate) async fn authenticate(
    engine: &MutationEngine,
    validator: &AssertionValidator,
    client_id: &str,
    password: &str,
    peer: Option<SocketAddr>,
) -> Verdict {
    if !is_doorbell_client(client_id) {
        return Verdict::Ignore;
    }
    let outcome = if password.starts_with(op_identity::sealed_id::INLINE_PREFIX) {
        authenticate_sid1(engine, client_id, password).await
    } else if let Some(encoded) = password.strip_prefix(OIA1_PASSWORD_PREFIX) {
        authenticate_oia1(engine, validator, client_id, encoded, peer).await
    } else {
        Err("password is neither an inline SID1 nor an OIA1 assertion".into())
    };
    match outcome {
        Ok(()) => Verdict::Allow,
        Err(reason) => {
            tracing::info!(reason, "doorbell authentication refused");
            Verdict::Deny
        }
    }
}

/// The browser's credential: the decoy-issued assertion, verified by the
/// same validator as the door (signature, lifetime, one-time nonce, source
/// binding), resolved to the anchored human session, then consumed.
async fn authenticate_oia1(
    engine: &MutationEngine,
    validator: &AssertionValidator,
    client_id: &str,
    encoded: &str,
    peer: Option<SocketAddr>,
) -> Result<(), String> {
    let wire = base64::engine::general_purpose::STANDARD
        .decode(encoded.trim())
        .map_err(|_| "OIA1 password is not base64".to_string())?;
    let now = chrono::Utc::now().timestamp();
    let mut pending = validator
        .validate_pending_with_bootstrap(&wire, peer, now, false)
        .map_err(|rejection| format!("assertion rejected: {rejection:?}"))?;
    let session = engine
        .ensure_verified_human_session_context(&pending.identity().human_pubkey)
        .await
        .ok_or_else(|| "assertion identity has no anchored session genesis".to_string())?;
    pending.identity_mut().session_id = session.session_id;
    pending.identity_mut().session_genesis = session.genesis_hex;
    let expected = owner_key(&EpisodeScope {
        principal_id: pending.identity().principal_id.clone(),
        session_id: pending.identity().session_id.clone(),
        session_genesis: pending.identity().session_genesis.clone(),
    });
    if expected != client_id {
        return Err("client id is not the owner key of the presented identity".into());
    }
    // Consume the nonce last: a refused connect must not burn the assertion.
    validator
        .consume_pending(pending)
        .map_err(|rejection| format!("assertion rejected: {rejection:?}"))?;
    Ok(())
}

async fn authenticate_sid1(
    engine: &MutationEngine,
    client_id: &str,
    password: &str,
) -> Result<(), String> {
    let claims = op_identity::sealed_id::SealedId::from_inline_ref(password)
        .map_err(|error| format!("password is not an inline SID1: {error}"))?;
    let wire = claims.seal().map_err(|error| error.to_string())?;
    let claims = crate::identity_sled_dispatch::authenticate_sid1_wire(
        engine,
        &wire,
        op_plugins::state_plugins::emqx::WAKE_TRANSPORT_SCOPE,
    )
    .await?;
    // SID1 proves the sled binding, not that its key remains authorized.
    let principal =
        crate::human_principal_dispatch::resolve_key_for_assertion(&claims.wireguard_pubkey)
            .await
            .map_err(|_| "principal registry is unavailable".to_string())?
            .ok_or_else(|| "sealed identity key is not registered".to_string())?;
    if principal.revoked_at != 0 || principal.principal_id != claims.principal_id {
        return Err("sealed identity principal is revoked or mismatched".into());
    }
    let expected = owner_key(&EpisodeScope {
        principal_id: claims.principal_id,
        session_id: claims.session_id,
        session_genesis: claims.session_genesis,
    });
    if expected != client_id {
        return Err("client id is not the owner key of the presented identity".into());
    }
    Ok(())
}

/// `client.authorize`: pure derivation from the client id and the topic,
/// plus a live sled check for doorbell subscriptions.
pub(crate) async fn authorize(
    engine: &MutationEngine,
    client_id: &str,
    subscribe: bool,
    topic: &str,
) -> Verdict {
    let namespace = op_plugins::state_plugins::emqx::WAKE_TOPIC_PREFIX
        .split('/')
        .next()
        .unwrap_or("opdbus");
    let in_namespace =
        topic == namespace || topic.starts_with(&format!("{namespace}/")) || topic.starts_with('$');
    if !is_doorbell_client(client_id) {
        // Devices and other broker clients never touch the wake namespace
        // (or broker-internal `$` topics through us); the rest is the
        // broker's own authorization.
        return if in_namespace && !topic.starts_with("$SYS") {
            Verdict::Deny
        } else {
            Verdict::Ignore
        };
    }
    let own_topic = match op_plugins::state_plugins::emqx::wake_topic(client_id) {
        Ok(topic) => topic,
        Err(_) => return Verdict::Deny,
    };
    if !subscribe || topic != own_topic {
        return Verdict::Deny;
    }
    match sled_still_derives(engine, client_id).await {
        Ok(true) => Verdict::Allow,
        Ok(false) => {
            tracing::info!("doorbell subscribe refused: no anchored sled derives this key");
            Verdict::Deny
        }
        Err(error) => {
            tracing::warn!(%error, "doorbell subscribe refused: sled projection unavailable");
            Verdict::Deny
        }
    }
}

/// Does any anchored, unexpired sled still derive `client_id`? No table:
/// the sleds are the projection the door itself authenticates against.
async fn sled_still_derives(engine: &MutationEngine, client_id: &str) -> anyhow::Result<bool> {
    let now = chrono::Utc::now().timestamp();
    let sleds = crate::identity_sled_dispatch::all_sessions(engine).await?;
    Ok(sleds.iter().any(|sled| {
        sled.is_anchored()
            && !sled
                .expires_at
                .is_some_and(|expiry| expiry != 0 && expiry <= now)
            && scope_of(sled).is_some_and(|scope| owner_key(&scope) == client_id)
    }))
}

/// Owner keys whose sleds no longer derive them: the connections to kick.
pub(crate) fn owner_key_for_sled(
    sled: &op_plugins::state_plugins::identity_sled::ContainerIdentitySled,
) -> Option<String> {
    scope_of(sled).map(|scope| owner_key(&scope))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The browser path end to end: a decoy-issued assertion for a
    /// registered key with an anchored sled opens the doorbell exactly once,
    /// only for the client id that is that identity's owner key, and the
    /// subscribe verdict follows the live sled.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn an_issued_assertion_opens_the_doorbell_once_for_its_own_owner_key() {
        use crate::human_principal_dispatch::tests::{
            pk, register, temp_cozo, temp_shm_with_schemas, test_engine,
        };
        use crate::identity_sled_dispatch::{tests::write_identity, IdentitySledStore};
        use crate::oracle_assertion::tests::{
            signed_with_fields, test_issuer, trust_store_for_issuer,
        };
        use op_state::StatePlugin as _;

        let _cozo = temp_cozo();
        let _shm = temp_shm_with_schemas(&[
            op_plugins::state_plugins::human_principal::HumanPrincipalPlugin::new()
                .schema()
                .unwrap(),
            op_plugins::state_plugins::identity_sled::IdentitySledPlugin::new()
                .schema()
                .unwrap(),
        ]);
        let engine = test_engine();
        engine
            .install_identity_sled_store_for_test(IdentitySledStore::new(
                op_cozo_store::CozoGraphShuttle::new_in_memory().unwrap(),
            ))
            .unwrap();
        let key = pk(0xD7);
        register(&key, "doorbell-fixture").await.unwrap();
        let sled = write_identity(&engine, &key).await.unwrap();
        let scope = EpisodeScope {
            principal_id: op_identity::session::derive_principal_id(&key),
            session_id: sled.session_id.clone(),
            session_genesis: sled.genesis.clone().unwrap(),
        };
        let client_id = owner_key(&scope);
        assert!(is_doorbell_client(&client_id));

        let issuer = test_issuer();
        let validator = AssertionValidator::new(trust_store_for_issuer(&issuer));
        let now = chrono::Utc::now().timestamp();
        let peer: SocketAddr = "127.0.0.1:40000".parse().unwrap();
        let password = |nonce: u8| {
            let signed = signed_with_fields(
                &issuer,
                &key,
                peer.ip(),
                now - 1,
                now + 300,
                [nonce; 16],
                None,
            );
            format!(
                "{OIA1_PASSWORD_PREFIX}{}",
                base64::engine::general_purpose::STANDARD.encode(signed.to_wire())
            )
        };

        // Wrong client id: refused, and the assertion is NOT consumed.
        let first = password(1);
        assert_eq!(
            authenticate(&engine, &validator, &"aa".repeat(32), &first, Some(peer)).await,
            Verdict::Deny
        );
        assert_eq!(
            authenticate(&engine, &validator, &client_id, &first, Some(peer)).await,
            Verdict::Allow,
            "same assertion, right owner key"
        );
        assert_eq!(
            authenticate(&engine, &validator, &client_id, &first, Some(peer)).await,
            Verdict::Deny,
            "one-time nonce: a replayed connect is refused"
        );
        assert_eq!(
            authenticate(&engine, &validator, &client_id, &password(2), Some(peer)).await,
            Verdict::Allow,
            "a fresh assertion opens it again"
        );

        // Subscribe: own topic only, and only while the sled derives the key.
        let topic = op_plugins::state_plugins::emqx::wake_topic(&client_id).unwrap();
        assert_eq!(
            authorize(&engine, &client_id, true, &topic).await,
            Verdict::Allow
        );
        assert_eq!(
            authorize(&engine, &client_id, false, &topic).await,
            Verdict::Deny
        );
        assert_eq!(
            owner_key_for_sled(&sled).as_deref(),
            Some(client_id.as_str()),
            "the kick targets the same derived key"
        );
    }

    #[test]
    fn only_owner_key_shaped_client_ids_are_doorbells() {
        assert!(is_doorbell_client(&"ab".repeat(32)));
        assert!(!is_doorbell_client("device-42"));
        assert!(!is_doorbell_client(&"AB".repeat(32)));
    }

    #[tokio::test]
    async fn non_doorbell_clients_are_kept_out_of_the_wake_namespace_only() {
        let engine = crate::human_principal_dispatch::tests::test_engine();
        assert_eq!(
            authorize(&engine, "device-42", true, "opdbus/wake/#").await,
            Verdict::Deny
        );
        assert_eq!(
            authorize(&engine, "device-42", false, "opdbus/anything").await,
            Verdict::Deny
        );
        assert_eq!(
            authorize(&engine, "device-42", true, "$share/g/opdbus/wake/x").await,
            Verdict::Deny
        );
        assert_eq!(
            authorize(&engine, "device-42", true, "factory/line1/telemetry").await,
            Verdict::Ignore
        );
        assert_eq!(
            authorize(&engine, "device-42", true, "$SYS/brokers").await,
            Verdict::Ignore
        );
    }

    #[tokio::test]
    async fn doorbell_clients_may_only_subscribe_to_their_own_topic() {
        let engine = crate::human_principal_dispatch::tests::test_engine();
        let key = "cd".repeat(32);
        let other = "ef".repeat(32);
        assert_eq!(
            authorize(&engine, &key, false, &format!("opdbus/wake/{key}")).await,
            Verdict::Deny,
            "doorbells never publish"
        );
        assert_eq!(
            authorize(&engine, &key, true, &format!("opdbus/wake/{other}")).await,
            Verdict::Deny
        );
        assert_eq!(
            authorize(&engine, &key, true, "opdbus/wake/+").await,
            Verdict::Deny
        );
        // Own topic, but no sled derives the key in this fixture.
        assert_eq!(
            authorize(&engine, &key, true, &format!("opdbus/wake/{key}")).await,
            Verdict::Deny
        );
        let validator =
            AssertionValidator::new(crate::oracle_assertion::DecoyTrustStore::parse_bytes(b"{}"));
        assert_eq!(
            authenticate(&engine, &validator, "device-42", "whatever", None).await,
            Verdict::Ignore
        );
        for password in [
            "not-a-credential",
            "sid1:!!",
            "oia1:not-base64!",
            "oia1:QUJDRA==",
        ] {
            assert_eq!(
                authenticate(&engine, &validator, &key, password, None).await,
                Verdict::Deny,
                "{password}"
            );
        }
    }
}
