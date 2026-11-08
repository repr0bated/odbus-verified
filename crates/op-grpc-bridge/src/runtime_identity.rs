//! Identity publication and configured service-role authority for the fresh runtime.
//! Recovery rereads the durable identity owner under the existing pending token;
//! it never clears a marker, mints a replacement identity, or adopts storage.
use std::{collections::HashSet, path::PathBuf, sync::Arc};
use anyhow::{Context as _, ensure};
use op_core::projection_shm::{self, CredentialPublication};
use op_cozo_store::{CozoGraphShuttle, IdentityRecoverySnapshot, IdentitySledRecord, GenesisInputsRecord};
use op_plugins::state_plugins::identity_sled::{ContainerIdentitySled, IdentitySledState, SessionEvent};
use crate::{identity_receiver::VerifiedSid1, runtime::{CallerAuthority, Runtime, RuntimeError, VerifiedCaller}};

/// Constructed only from a completed native refresh whose paired publication
/// was accepted by the canonical operation owner. These are exact authored
/// replacement bytes, never a credential reconstructed from claims.
pub(crate) struct CredentialRefresh {
    previous: op_identity::sealed_id::SealedId,
    identity: VerifiedSid1,
    wire: Vec<u8>,
}

impl CredentialRefresh {
    pub(crate) fn previous(&self) -> &op_identity::sealed_id::SealedId { &self.previous }
    pub(crate) fn identity(&self) -> &VerifiedSid1 { &self.identity }
    pub(crate) fn wire(&self) -> &[u8] { &self.wire }
}

pub(crate) fn validate_refresh_claims(
    previous: &op_identity::sealed_id::SealedId,
    replacement: &op_identity::sealed_id::SealedId,
    now: i64,
) -> anyhow::Result<()> {
    // Exhaustive destructuring makes a future identity claim an explicit
    // decision here instead of silently permitting it to change.
    let op_identity::sealed_id::SealedId {
        principal_id, principal_kind, session_id, wireguard_pubkey, session_genesis,
        trace_id, schema_version, issued_at, expires_at, arrival_timestamp,
        chain_head_at_arrival, catalog_hash_at_arrival, head_timestamp_at_arrival,
        transport_scope, nextdns: _,
    } = previous;
    ensure!(principal_id == &replacement.principal_id
        && principal_kind == &replacement.principal_kind
        && session_id == &replacement.session_id
        && wireguard_pubkey == &replacement.wireguard_pubkey
        && session_genesis == &replacement.session_genesis
        && trace_id == &replacement.trace_id
        && schema_version == &replacement.schema_version
        && issued_at == &replacement.issued_at
        && expires_at == &replacement.expires_at
        && arrival_timestamp == &replacement.arrival_timestamp
        && chain_head_at_arrival == &replacement.chain_head_at_arrival
        && catalog_hash_at_arrival == &replacement.catalog_hash_at_arrival
        && head_timestamp_at_arrival == &replacement.head_timestamp_at_arrival,
        "credential refresh changed an immutable identity claim");
    ensure!(matches!(transport_scope.as_str(), "dbus,grpc,mcp" | "dbus,grpc,mcp,mqtt")
        && replacement.transport_scope == "dbus,grpc,mcp,mqtt",
        "credential refresh changed an unauthorized transport scope");
    ensure!(*expires_at == 0 || *expires_at > now, "credential refresh identity has expired");
    Ok(())
}

pub(crate) async fn refreshed_caller(
    caller: &VerifiedCaller,
    receipt: &op_state_store::method_receipt::MethodExecutionReceipt,
) -> anyhow::Result<VerifiedCaller> {
    receipt.validate_attribution("identity_sled", "refresh_credential")?;
    let result = &receipt.result;
    ensure!(result.get("refreshed").and_then(serde_json::Value::as_bool) == Some(true)
        && result.get("session_id").and_then(serde_json::Value::as_str) == Some(caller.session_id.as_str()),
        "completed refresh does not belong to the original caller");
    let inline = result.get("sealed_id").and_then(serde_json::Value::as_str)
        .context("completed refresh has no authored credential")?;
    let encoded = inline.strip_prefix(op_identity::sealed_id::INLINE_PREFIX)
        .context("completed refresh credential is not SID1")?;
    let wire = base64::Engine::decode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, encoded)?;
    // Receiver compares these bytes with the private committed credential and
    // rechecks its binding and the current principal registry.
    let identity = crate::identity_receiver::verify_sid1(&wire, "grpc").await?;
    validate_refresh_claims(caller.claims(), identity.claims(), chrono::Utc::now().timestamp())?;
    ensure!(result.get("transport_scope").and_then(serde_json::Value::as_str)
        == Some(identity.claims().transport_scope.as_str()), "refresh result scope differs from authored credential");
    let refresh = CredentialRefresh { previous: caller.claims().clone(), identity, wire };
    let authority = caller.authority.clone().refreshed(&refresh).await?;
    authority.revalidate().await?;
    let mut refreshed = VerifiedCaller::new(refresh.identity(), authority);
    refreshed.sealed_identity = Some(inline.to_owned());
    Ok(refreshed)
}

fn stored_json<T: serde::de::DeserializeOwned>(value: &str) -> anyhow::Result<Option<T>> {
    if value.is_empty() { Ok(None) } else { Ok(Some(serde_json::from_str(value)?)) }
}

pub(crate) fn record_to_sled(record: &IdentitySledRecord, genesis: Option<&GenesisInputsRecord>) -> anyhow::Result<ContainerIdentitySled> {
    let mut sled = ContainerIdentitySled {
        session_id: record.session_id.clone(), wireguard_pubkey: record.wireguard_pubkey.clone(),
        interface: record.interface.clone(), peer_ip: (!record.peer_ip.is_empty()).then(|| record.peer_ip.clone()),
        mutation_index: u64::try_from(record.mutation_index)?,
        genesis: (!record.session_genesis.is_empty()).then(|| record.session_genesis.clone()),
        trace_id: record.trace_id.clone(), schema_version: u32::try_from(record.schema_version)?,
        vector_id: record.vector_id.clone(), sealed_id: (!record.sealed_id.is_empty()).then(|| record.sealed_id.clone()),
        principal_kind: record.principal_kind.clone(), btrfs_device: stored_json(&record.btrfs_device_json)?,
        instance: stored_json(&record.instance_json)?, session_started_at: record.session_started_at,
        last_seen_at: record.last_seen_at, active: record.active,
        expires_at: (record.expires_at != 0).then_some(record.expires_at), arrival_timestamp: 0,
        chain_head_at_arrival: String::new(), catalog_hash_at_arrival: String::new(), head_timestamp_at_arrival: 0,
    };
    if let Some(genesis) = genesis {
        ensure!(genesis.session_id == record.session_id, "durable identity genesis owner mismatch");
        sled.arrival_timestamp = genesis.arrival_timestamp;
        sled.chain_head_at_arrival = genesis.chain_head_at_arrival.clone();
        sled.catalog_hash_at_arrival = genesis.catalog_hash_at_arrival.clone();
        sled.head_timestamp_at_arrival = genesis.head_timestamp_at_arrival;
    }
    crate::identity_binding::validate_sled(&sled)?;
    Ok(sled)
}

pub(crate) fn recovery_state(snapshot: IdentityRecoverySnapshot) -> anyhow::Result<IdentitySledState> {
    let mut ids = HashSet::new();
    let mut sleds = Vec::with_capacity(snapshot.identities.len());
    for revision in snapshot.identities {
        ensure!(ids.insert(revision.session_id.clone()), "duplicate durable identity revision");
        if let Some(record) = revision.record {
            ensure!(record.session_id == revision.session_id, "durable identity owner mismatch");
            sleds.push(record_to_sled(&record, revision.genesis.as_ref())?);
        }
    }
    sleds.sort_unstable_by(|a, b| a.session_id.cmp(&b.session_id));
    let events = snapshot.events.into_iter().map(|row| -> anyhow::Result<_> {
        Ok(SessionEvent { session_id: row.session_id, seq: u64::try_from(row.seq)?, kind: row.kind,
            subid: row.subid, content: row.content, created_at: row.created_at })
    }).collect::<anyhow::Result<Vec<_>>>()?;
    Ok(IdentitySledState { sleds, events })
}

pub(crate) async fn recover_startup(store: Arc<CozoGraphShuttle>, catalog_dir: PathBuf) -> anyhow::Result<()> {
    tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
        let pinned = crate::sealed_schema_reader::read_manifest_pinned_schema_document(&catalog_dir, "identity_sled")?;
        crate::method_contract_admission::admit_compiled_state_contract(&pinned, "identity_sled")?;
        pinned.schema.validate_public_state_policy()?;
        let state_dir = projection_shm::shm_state_dir();
        let group_name = std::env::var("OP_IDENTITY_PROJECTION_GROUP").unwrap_or_else(|_| "secrets".into());
        let gid = nix::unistd::Group::from_name(&group_name)?.context("configured credential reader group is absent")?.gid.as_raw();
        let pending = projection_shm::pending_identity_publication_in(&state_dir)?;
        let load = || -> anyhow::Result<_> {
            // This API includes the owner's durability barrier, not only a visibility read.
            let state = recovery_state(store.read_identity_recovery_state(256)?)?;
            let private = simd_json::serde::to_owned_value(state)?;
            let public = op_plugins::state_plugins::identity_sled::identity_metadata_projection(&private)?;
            let current = crate::sealed_schema_reader::read_manifest_pinned_schema_document(&catalog_dir, "identity_sled")?;
            ensure!(current.schema_hash == pinned.schema_hash && current.blob.schema_json == pinned.blob.schema_json,
                "identity seal changed during durable recovery");
            Ok((Some((simd_json::to_vec(&public)?, simd_json::to_vec(&private)?)), ()))
        };
        let publication = if let Some(pending) = pending {
            projection_shm::recover_identity_projection_in(&state_dir, gid, &pending.operation_id, load)?
        } else {
            let operation = format!("startup-{}", uuid::Uuid::new_v4().simple());
            projection_shm::commit_identity_projection_durable_in(&state_dir, gid, &operation, |_| load())?
        };
        match publication {
            CredentialPublication::Committed { .. } => Ok(()),
            CredentialPublication::Pending { operation_id, .. } => anyhow::bail!("identity publication remains pending: {operation_id}"),
        }
    }).await.context("identity recovery owner failed")?
}

struct ResponderAuthority { identity: VerifiedSid1, principal: String }
#[async_trait::async_trait]
impl CallerAuthority for ResponderAuthority {
    async fn revalidate(&self) -> anyhow::Result<()> {
        ensure!(std::env::var("OP_CONTROL_PLANE_CHATBOT_PRINCIPAL_ID").ok().as_deref() == Some(self.principal.as_str()),
            "configured responder owner changed");
        crate::identity_receiver::authored_transport_wire(&self.identity).await?;
        Ok(())
    }
}

pub(crate) async fn configured_responder(runtime: &Runtime, requester: &VerifiedCaller) -> Result<VerifiedCaller, RuntimeError> {
    requester.authority.revalidate().await?;
    let principal = std::env::var("OP_CONTROL_PLANE_CHATBOT_PRINCIPAL_ID")
        .map_err(|_| RuntimeError::denied("designated responder principal is not configured"))?;
    let catalog = runtime.catalog_dir().to_owned();
    let expected = principal.clone();
    let bound = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        let state_dir = projection_shm::shm_state_dir();
        let rows = crate::identity_receiver::committed_sessions_in(&catalog, &state_dir)?;
        let mut matching = rows.into_iter().filter(|row| op_identity::session::derive_principal_id(&row.wireguard_pubkey) == expected);
        let row = matching.next().context("configured responder has no authored session")?;
        ensure!(matching.next().is_none(), "configured responder session is ambiguous");
        ensure!(row.principal_kind.as_deref() == Some("service"), "designated responder is not a service identity");
        crate::identity_receiver::bound_session_for_id_in(&catalog, &state_dir, &row.session_id, chrono::Utc::now().timestamp())
    }).await??;
    let identity = crate::identity_receiver::verify_bound_registry(bound).await?;
    let authority = Arc::new(ResponderAuthority { identity: identity.clone(), principal });
    let mut caller = VerifiedCaller::new(&identity, authority);
    caller.inherit_execution(requester);
    runtime.authorize_nested(&caller, "chat_response", "respond_to_user").await?;
    caller.sealed_identity = Some(format!("{}{}", op_identity::sealed_id::INLINE_PREFIX,
        base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, runtime.identity_wire(&caller).await?)));
    Ok(caller)
}

pub(crate) async fn authorize_protected(runtime: &Runtime, caller: &VerifiedCaller, plugin: &str, method: &str, args: &serde_json::Value) -> Result<(), RuntimeError> {
    runtime.task_elevation().consume(&caller.principal_id, Some(&caller.session_id), plugin, method, args)
        .map_err(|_| RuntimeError::denied("protected action requires a current exact-call elevation ticket"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use op_identity::sealed_id::{NextDnsContext, SealedId};

    fn claims() -> SealedId {
        SealedId {
            principal_id: "principal".into(), principal_kind: "wireguard-principal".into(),
            session_id: "session".into(), wireguard_pubkey: "key".into(),
            session_genesis: "genesis".into(), trace_id: "trace".into(), schema_version: 3,
            issued_at: 10, expires_at: 100, arrival_timestamp: 10,
            chain_head_at_arrival: "chain".into(), catalog_hash_at_arrival: "catalog".into(),
            head_timestamp_at_arrival: 9, transport_scope: "dbus,grpc,mcp".into(), nextdns: None,
        }
    }

    #[test]
    fn refresh_changes_only_transport_and_native_nextdns() {
        let previous = claims();
        let mut replacement = previous.clone();
        replacement.transport_scope = "dbus,grpc,mcp,mqtt".into();
        replacement.nextdns = Some(NextDnsContext { profile_id: "abcdef".into(), device_name: "own".into() });
        validate_refresh_claims(&previous, &replacement, 50).unwrap();
        assert!(validate_refresh_claims(&previous, &replacement, 100).is_err());
        replacement.transport_scope = "grpc".into();
        assert!(validate_refresh_claims(&previous, &replacement, 50).is_err());
    }

    #[test]
    fn refresh_rejects_every_immutable_identity_claim_change() {
        let previous = claims();
        let changes: &[fn(&mut SealedId)] = &[
            |c| c.principal_id.push('x'), |c| c.principal_kind.push('x'),
            |c| c.session_id.push('x'), |c| c.wireguard_pubkey.push('x'),
            |c| c.session_genesis.push('x'), |c| c.trace_id.push('x'),
            |c| c.schema_version += 1, |c| c.issued_at += 1, |c| c.expires_at += 1,
            |c| c.arrival_timestamp += 1, |c| c.chain_head_at_arrival.push('x'),
            |c| c.catalog_hash_at_arrival.push('x'), |c| c.head_timestamp_at_arrival += 1,
        ];
        for change in changes {
            let mut replacement = previous.clone();
            replacement.transport_scope = "dbus,grpc,mcp,mqtt".into();
            change(&mut replacement);
            assert!(validate_refresh_claims(&previous, &replacement, 50).is_err());
        }
    }
}
