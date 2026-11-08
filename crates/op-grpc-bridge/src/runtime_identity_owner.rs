//! Fresh-runtime identity owners. Cozo owns revisions; identity alone publishes
//! the paired metadata/credential generation. Native observations and guards are
//! carried to the enclosing durable terminal, never synthesized from arguments.
use std::{collections::{HashMap, HashSet}, path::PathBuf, sync::{Arc, LazyLock, Mutex, Weak, atomic::{AtomicBool, Ordering}}};
use anyhow::{Context as _, ensure};
use base64::Engine as _;
use op_core::projection_shm::{self, CredentialPublication};
use op_cozo_store::{CozoGraphShuttle, GenesisInputsRecord, IdentityCompareExchange, IdentityIntent,
    IdentityRecoverySnapshot, IdentityRevision, IdentitySledRecord, SessionEventRecord};
use op_plugins::state_plugins::{identity_sled::{ContainerIdentitySled, IdentitySledState,
    RECORD_FORMAT, SCHEMA_CONTENT_HASH, SessionEvent}, incus::{IncusInstance, IncusPlugin,
    IncusRootRepairBackend, IncusState, IdentitySessionPowerState}};
use serde_json::Value as JsonValue;
use crate::{core::{BackendProjection, BackendStateUpdate, StateScope},
    runtime::{BackendContext, BackendOutcome, Runtime, RuntimeError, VerifiedCaller},
    identity_binding::{configured_chatbot_key, control_plane_chatbot_session_id,
        identity_instance_matches, identity_session_binding_matches, validate_sled,
        CONTROL_PLANE_CHATBOT_CONTAINER, RESERVED_CHATBOT_CONTAINER},
    runtime_identity::record_to_sled};

#[path = "runtime_identity_root.rs"] mod root_repair;
#[path = "runtime_identity_clean.rs"] mod clean_provision;
#[path = "runtime_identity_physical.rs"] mod physical_root;
#[path = "runtime_identity_seed.rs"] mod seed_provision;
#[path = "runtime_identity_delete.rs"] mod delete_provision;
#[path = "runtime_identity_human.rs"] mod human;
#[path = "runtime_identity_rename.rs"] mod rename;

const MAX_EVENTS_IN_STATE: usize = 256;
const MAX_HISTORY_LIMIT: usize = 4096;

pub(crate) fn supports(plugin: &str, method: &str) -> bool {
    match plugin {
        "identity_sled" => matches!(method, "prepare_container" | "rename_container" | "get_identity" | "get_own_identity"
            | "inspect_own_root_repair_preflight" | "write_identity" | "inspect_identity_inventory"
            | "refresh_credential" | "provision_container" | "inspect_provision_preflight"
            | "delete_provision_record" | "repair_own_root" | "migrate_own_physical_root"
            | "inspect_own_physical_root_migration_preflight" | "attach_btrfs_device"
            | "touch_session" | "record_session_event" | "get_session_history"),
        "human_principal" => matches!(method, "register_key" | "revoke_key" | "set_alias"
            | "resolve_key" | "get_principal" | "list_principals"),
        _ => false,
    }
}

#[derive(Default)]
struct NativeLifetime {
    effect: AtomicBool,
    guards: Mutex<Vec<Box<dyn Send + Sync>>>,
    observed: Mutex<Vec<BackendProjection>>,
    identity: Mutex<Option<projection_shm::ProjectionCommit>>,
}
struct IdentitySledStore {
    cozo: Arc<CozoGraphShuttle>,
    lifetime: Arc<NativeLifetime>,
    allow_pending_rename: bool,
}
impl IdentitySledStore {
    fn mark_effect(&self) { self.lifetime.effect.store(true, Ordering::Release); }
    async fn lifecycle_lock(&self, session: &str) -> Arc<tokio::sync::Mutex<()>> {
        static LOCKS: LazyLock<Mutex<HashMap<String, Weak<tokio::sync::Mutex<()>>>>> = LazyLock::new(Default::default);
        let mut locks = LOCKS.lock().expect("identity locks poisoned");
        locks.retain(|_, lock| lock.strong_count() != 0);
        if let Some(lock) = locks.get(session).and_then(Weak::upgrade) { return lock; }
        let lock = Arc::new(tokio::sync::Mutex::new(()));
        locks.insert(session.into(), Arc::downgrade(&lock));
        lock
    }
}
struct NativeOwner {
    runtime: Runtime,
    caller: VerifiedCaller,
    plugin: String,
    method: String,
    operation_id: String,
    request_event_id: u64,
    request_event_hash: String,
    lifetime: Arc<NativeLifetime>,
}
impl NativeOwner {
    fn retain(&self, guard: impl Send + Sync + 'static) {
        self.lifetime.guards.lock().expect("native guards poisoned").push(Box::new(guard));
    }
    fn require_actor(&self, principal: &str) -> anyhow::Result<()> {
        ensure!(principal == self.caller.principal_id, "identity caller mismatch");
        Ok(())
    }
    async fn revalidate(&self) -> anyhow::Result<()> {
        self.runtime.authorize_nested(&self.caller, &self.plugin, &self.method).await?;
        if self.plugin == "identity_sled" {
            let pinned = strict_identity_contract()?;
            let admitted = self.runtime.catalog().plugin("identity_sled").context("identity plugin absent")?;
            ensure!(pinned.schema_hash == admitted.schema_hash
                && admitted.sealed_schema_json.as_deref() == Some(pinned.blob.schema_json.as_str()),
                "identity native owner and runtime catalog differ");
        }
        Ok(())
    }
    async fn verify_own(&self, sled: &ContainerIdentitySled) -> anyhow::Result<()> {
        self.revalidate().await?;
        ensure!(sled.is_anchored() && sled.session_id == self.caller.session_id
            && sled.genesis.as_deref() == Some(self.caller.session_genesis.as_str())
            && sled.wireguard_pubkey == self.caller.claims().wireguard_pubkey
            && op_identity::session::derive_principal_id(&sled.wireguard_pubkey) == self.caller.principal_id
            && op_identity::session::derive_session_id(&sled.wireguard_pubkey) == sled.session_id,
            "verified caller does not own identity");
        let wire = self.runtime.identity_wire(&self.caller).await?;
        crate::sid1_checks::match_bound_session(&wire, sled, "grpc", now()).map_err(anyhow::Error::msg)?;
        ensure!(self.caller.sealed_identity.as_deref() == sled.sealed_id.as_deref(),
            "original caller credential no longer matches identity");
        // Inactive is power policy, not credential revocation.
        seed_provision::target_key_is_registered(&sled.wireguard_pubkey).await
    }
    async fn observe_incus(&self, snapshot: &IncusState) -> anyhow::Result<()> {
        let updates = project_native(&self.runtime, &self.caller, "incus", serde_json::to_value(snapshot)?)?;
        let mut observed = self.lifetime.observed.lock().expect("native observations poisoned");
        observed.retain(|projection| projection.plugin_id != "incus");
        observed.extend(updates);
        Ok(())
    }
    async fn commit_identity_intent<T, F>(&self, operation_id: String, commit: F)
        -> anyhow::Result<T>
    where T: Send + 'static,
        F: FnOnce(Option<&[u8]>) -> anyhow::Result<(Option<simd_json::OwnedValue>, T)> + Send + 'static,
    {
        self.revalidate().await?;
        let runtime = self.runtime.clone();
        let lifetime = self.lifetime.clone();
        tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
            let directory = runtime.catalog_dir();
            let pinned = crate::sealed_schema_reader::read_manifest_pinned_schema_document(directory, "identity_sled")?;
            crate::method_contract_admission::admit_compiled_state_contract(&pinned, "identity_sled")?;
            pinned.schema.validate_public_state_policy()?;
            let state_dir = projection_shm::shm_state_dir();
            ensure!(projection_shm::pending_identity_publication_in(&state_dir)?.is_none(),
                "identity publication requires exact startup recovery");
            let expected = state_value(&runtime.identity_store().read_identity_recovery_state(MAX_EVENTS_IN_STATE)?)?;
            let bytes = projection_shm::read_credential_projection_bytes_in(&state_dir, "identity_sled")?
                .context("identity credential projection absent")?;
            ensure!(serde_json::from_slice::<JsonValue>(&bytes)? == serde_json::to_value(&expected)?,
                "durable identity owner differs from committed credentials");
            let group = std::env::var("OP_IDENTITY_PROJECTION_GROUP").unwrap_or_else(|_| "secrets".into());
            let gid = nix::unistd::Group::from_name(&group)?.context("credential reader group absent")?.gid.as_raw();
            lifetime.effect.store(true, Ordering::Release);
            let publication = projection_shm::commit_identity_projection_durable_in(&state_dir, gid, &operation_id, |previous| {
                ensure!(previous == Some(bytes.as_slice()), "identity credential generation changed before CAS");
                let (state, durable) = commit(previous)?;
                let current = crate::sealed_schema_reader::read_manifest_pinned_schema_document(directory, "identity_sled")?;
                ensure!(current.schema_hash == pinned.schema_hash && current.blob.schema_json == pinned.blob.schema_json,
                    "identity seal changed during native publication");
                let pair = state.map(|private| -> anyhow::Result<_> {
                    let public = op_plugins::state_plugins::identity_sled::identity_metadata_projection(&private)?;
                    // Validate the wire projection separately; the scalar metadata owner remains unchanged.
                    pinned.schema.project_public_state(&public)?;
                    Ok((simd_json::to_vec(&public)?, simd_json::to_vec(&private)?))
                }).transpose()?;
                Ok((pair, durable))
            })?;
            match publication {
                CredentialPublication::Committed { projection, durable } => {
                    *lifetime.identity.lock().expect("identity receipt poisoned") = Some(projection);
                    Ok(durable)
                }
                CredentialPublication::Pending { operation_id, error, .. } =>
                    Err(error.context(format!("identity publication {operation_id} requires exact startup recovery"))),
            }
        }).await.context("identity publication owner failed")?
    }
    async fn mint_identity_genesis_stamp(&self, session_id: &str, public_key: &str, expires_at: i64)
        -> anyhow::Result<GenesisStamp> {
        ensure!(self.method == "provision_container", "credential minting requires explicit provision");
        let key = canonical_key(public_key)?;
        ensure!(op_identity::session::derive_session_id(public_key) == session_id && expires_at >= 0,
            "invalid provision identity anchor");
        let events = self.runtime.core().journal_events(&self.caller.private_scope(), self.request_event_id, self.request_event_id)?;
        let event = events.into_iter().next().context("durable provision attempt is absent")?;
        ensure!(event.event_id == self.request_event_id && event.event_hash == self.request_event_hash,
            "provision attempt receipt differs");
        let head_timestamp = event.timestamp.seconds;
        let chain_head = hex::decode(&event.event_hash)?;
        ensure!(matches!(chain_head.len(), 16 | 32), "invalid arrival chain digest");
        let catalog = crate::sealed_schema_reader::read_manifest_pinned_catalog(self.runtime.catalog_dir())?;
        ensure!(catalog.plugins.contains_key("identity_sled"), "identity plugin is not sealed");
        let catalog_hash: [u8;32] = hex::decode(&catalog.catalog_hash)?.try_into()
            .map_err(|_| anyhow::anyhow!("invalid identity arrival catalog hash"))?;
        let arrival = now();
        let genesis = op_identity::session_genesis::mint_genesis(&key, &chain_head, head_timestamp, &catalog_hash, arrival);
        let bindings = crate::identity_nextdns::NextDnsBindings::from_env()?;
        let claims = op_identity::sealed_id::SealedId {
            principal_id: op_identity::session::derive_principal_id(public_key),
            principal_kind: "wireguard-principal".into(), session_id: session_id.into(),
            wireguard_pubkey: public_key.into(), session_genesis: hex::encode(genesis),
            trace_id: hex::encode(uuid::Uuid::new_v4().as_bytes()), schema_version: RECORD_FORMAT,
            issued_at: arrival, expires_at, arrival_timestamp: arrival,
            chain_head_at_arrival: event.event_hash, catalog_hash_at_arrival: catalog.catalog_hash,
            head_timestamp_at_arrival: head_timestamp, transport_scope: "dbus,grpc,mcp,mqtt".into(),
            nextdns: bindings.get(public_key).cloned(),
        };
        Ok(GenesisStamp { sealed_id: claims.to_inline_ref().map_err(|error| anyhow::anyhow!("seal identity: {error}"))?, claims })
    }
}
struct GenesisStamp { sealed_id: String, claims: op_identity::sealed_id::SealedId }
fn apply_genesis_stamp(sled: &mut ContainerIdentitySled, stamp: &GenesisStamp) -> anyhow::Result<()> {
    ensure!(!sled.is_anchored() && sled.session_id == stamp.claims.session_id
        && sled.wireguard_pubkey == stamp.claims.wireguard_pubkey, "provision anchor cannot overwrite existing identity");
    ensure!(identity_session_binding_matches(sled)
        && stamp.claims.schema_version == sled.schema_version
        && stamp.claims.expires_at == sled.expires_at.unwrap_or(0),
        "provision credential requires the exact native binding and immutable term");
    sled.genesis = Some(stamp.claims.session_genesis.clone());
    sled.trace_id = stamp.claims.trace_id.clone();
    sled.sealed_id = Some(stamp.sealed_id.clone());
    sled.arrival_timestamp = stamp.claims.arrival_timestamp;
    sled.chain_head_at_arrival = stamp.claims.chain_head_at_arrival.clone();
    sled.catalog_hash_at_arrival = stamp.claims.catalog_hash_at_arrival.clone();
    sled.head_timestamp_at_arrival = stamp.claims.head_timestamp_at_arrival;
    Ok(())
}

fn project_native(runtime: &Runtime, caller: &VerifiedCaller, plugin: &str, state: JsonValue) -> anyhow::Result<Vec<BackendProjection>> {
    let pinned = crate::sealed_schema_reader::read_manifest_pinned_schema_document(runtime.catalog_dir(), plugin)?;
    let entry = runtime.catalog().plugin(plugin).context("native observation plugin absent")?;
    ensure!(entry.schema_hash == pinned.schema_hash, "native observation seal changed");
    pinned.schema.validate_public_state_policy()?;
    let public = pinned.schema.project_public_state(&simd_json::serde::to_owned_value(&state)?)?;
    let mut updates = Vec::with_capacity(2);
    if let Some(public) = public {
        updates.push(BackendProjection { plugin_id: plugin.into(), object_path: op_plugins::canonical::plugin_path(plugin),
            schema_hash: pinned.schema_hash.clone(), scope: StateScope::Public,
            state: BackendStateUpdate::Replace(serde_json::to_value(public)?) });
    }
    updates.push(BackendProjection { plugin_id: plugin.into(), object_path: op_plugins::canonical::plugin_path(plugin),
        schema_hash: pinned.schema_hash, scope: caller.private_scope(), state: BackendStateUpdate::Replace(state) });
    Ok(updates)
}

pub(crate) async fn dispatch(ctx: &BackendContext<'_>, plugin: &str, method: &str, args: &JsonValue)
    -> Result<BackendOutcome, RuntimeError> {
    if !supports(plugin, method) { return Err(RuntimeError::unsupported("unknown identity owner method")); }
    ctx.runtime.authorize_nested(ctx.caller, plugin, method).await?;
    let lifetime = Arc::new(NativeLifetime::default());
    let engine = NativeOwner { runtime: ctx.runtime.clone(), caller: ctx.caller.clone(), plugin: plugin.into(),
        method: method.into(), operation_id: ctx.operation_id.to_string(), request_event_id: ctx.request_event_id,
        request_event_hash: ctx.request_event_hash.clone(), lifetime: lifetime.clone() };
    let store = IdentitySledStore { cozo: ctx.runtime.identity_store().clone(), lifetime: lifetime.clone(),
        allow_pending_rename: matches!(method, "rename_container" | "delete_provision_record") };
    let result = if plugin == "human_principal" {
        human::dispatch(&engine, method, args).await
    } else {
        identity_method(&engine, &store, method, args).await
    };
    if result.is_err() && matches!(method, "repair_own_root" | "prepare_container" | "rename_container")
        && (lifetime.effect.load(Ordering::Acquire)
            || result.as_ref().err().and_then(|error| error.downcast_ref::<RuntimeError>())
                .is_some_and(RuntimeError::outcome_unknown)) {
        // Observe once after an uncertain native maintenance request. A failed
        // readback leaves the original error unknown, never an empty success.
        if let Ok(snapshot) = IncusRootRepairBackend::local().default_project_state().await {
            let _ = engine.observe_incus(&snapshot).await;
        }
    }
    let additional = std::mem::take(&mut *lifetime.observed.lock().expect("native observations poisoned"));
    let state = lifetime.identity.lock().expect("identity receipt poisoned").take()
        .map(BackendStateUpdate::IdentityPublished).unwrap_or(BackendStateUpdate::Unchanged);
    match result {
        Ok(result) => Ok(BackendOutcome { result, state,
            scope: ctx.caller.private_scope(), additional, guard: Some(Box::new(lifetime)) }),
        Err(error) => {
            let mut error = if lifetime.effect.load(Ordering::Acquire) { RuntimeError::unknown(error) }
                else { match error.downcast::<RuntimeError>() {
                    Ok(error) => error,
                    Err(error) => RuntimeError::before_effect(error),
                } };
            if !matches!(state, BackendStateUpdate::Unchanged) {
                error = error.with_state(state, ctx.caller.private_scope());
            }
            if !additional.is_empty() { error = error.with_additional(additional); }
            Err(error.with_guard(Box::new(lifetime)))
        }
    }
}

struct VerifiedIdentityActor<'a> { actor_id: &'a str, session_id: &'a str, genesis: &'a str }
#[derive(Debug)]
struct DurableResult<T> { value: T, replayed: bool }
fn now() -> i64 { chrono::Utc::now().timestamp() }
fn arg_str<'a>(args: &'a JsonValue, name: &str) -> &'a str { args.get(name).and_then(JsonValue::as_str).unwrap_or("") }
fn canonical_key(key: &str) -> anyhow::Result<[u8;32]> {
    let raw = base64::engine::general_purpose::STANDARD.decode(key)?;
    ensure!(base64::engine::general_purpose::STANDARD.encode(&raw) == key, "noncanonical identity key");
    raw.try_into().map_err(|_| anyhow::anyhow!("identity key must contain 32 bytes"))
}
fn sled_to_record(sled: &ContainerIdentitySled) -> anyhow::Result<IdentitySledRecord> {
    validate_sled(sled)?;
    Ok(IdentitySledRecord {
        session_id: sled.session_id.clone(),
        wireguard_pubkey: sled.wireguard_pubkey.clone(),
        interface: sled.interface.clone(),
        peer_ip: sled.peer_ip.clone().unwrap_or_default(),
        mutation_index: i64::try_from(sled.mutation_index)?,
        session_genesis: sled.genesis.clone().unwrap_or_default(),
        trace_id: sled.trace_id.clone(),
        schema_version: i64::from(sled.schema_version),
        vector_id: sled.vector_id.clone(),
        sealed_id: sled.sealed_id.clone().unwrap_or_default(),
        principal_kind: sled.principal_kind.clone(),
        btrfs_device_json: sled
            .btrfs_device
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?
            .unwrap_or_default(),
        instance_json: sled
            .instance
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?
            .unwrap_or_default(),
        session_started_at: sled.session_started_at,
        last_seen_at: sled.last_seen_at,
        active: sled.active,
        expires_at: sled.expires_at.unwrap_or(0),
    })
}

fn sled_to_genesis(sled: &ContainerIdentitySled) -> GenesisInputsRecord {
    GenesisInputsRecord {
        session_id: sled.session_id.clone(),
        arrival_timestamp: sled.arrival_timestamp,
        chain_head_at_arrival: sled.chain_head_at_arrival.clone(),
        catalog_hash_at_arrival: sled.catalog_hash_at_arrival.clone(),
        head_timestamp_at_arrival: sled.head_timestamp_at_arrival,
        schema_content_hash: SCHEMA_CONTENT_HASH.trim().to_string(),
    }
}
fn event_from_record(row: &SessionEventRecord) -> anyhow::Result<SessionEvent> {
    anyhow::ensure!(row.seq >= 0, "negative durable session event sequence");
    Ok(SessionEvent {
        session_id: row.session_id.clone(),
        seq: u64::try_from(row.seq)?,
        kind: row.kind.clone(),
        subid: row.subid.clone(),
        content: row.content.clone(),
        created_at: row.created_at,
    })
}
fn state_value(snapshot: &IdentityRecoverySnapshot) -> anyhow::Result<simd_json::OwnedValue> {
    let mut sessions = HashSet::with_capacity(snapshot.identities.len());
    let mut sleds = Vec::with_capacity(snapshot.identities.len());
    for revision in &snapshot.identities {
        ensure!(sessions.insert(revision.session_id.as_str()), "duplicate durable identity revision");
        ensure!(revision.record.as_ref().is_none_or(|record| record.session_id == revision.session_id)
            && revision.genesis.as_ref().is_none_or(|genesis| genesis.session_id == revision.session_id),
            "durable identity revision owner mismatch");
        if let Some(sled) = revision_sled(revision)? { sleds.push(sled); }
    }
    sleds.sort_unstable_by(|a,b| a.session_id.cmp(&b.session_id));
    let events = snapshot.events.iter().map(event_from_record).collect::<anyhow::Result<Vec<_>>>()?;
    Ok(simd_json::serde::to_owned_value(IdentitySledState { sleds, events })?)
}
type SledCacheState = IdentitySledState;
fn strict_identity_contract()
-> anyhow::Result<crate::sealed_schema_reader::ManifestPinnedSchemaDocument> {
    let pinned = crate::sealed_schema_reader::read_manifest_pinned_schema_document(
        &crate::sealed_schema_reader::blob_catalog_dir_from_env(),
        "identity_sled",
    )?;
    crate::method_contract_admission::admit_compiled_state_contract(&pinned, "identity_sled")?;
    Ok(pinned)
}

fn same_identity_contract(
    expected: &crate::sealed_schema_reader::ManifestPinnedSchemaDocument,
) -> anyhow::Result<()> {
    let current = strict_identity_contract()?;
    anyhow::ensure!(
        current.schema_hash == expected.schema_hash
            && current.blob.schema_json == expected.blob.schema_json,
        "identity plugin seal changed during credential state access"
    );
    Ok(())
}

fn strict_private_state() -> anyhow::Result<SledCacheState> {
    let contract = strict_identity_contract()?;
    let state_dir = op_core::projection_shm::shm_state_dir();
    let bytes =
        op_core::projection_shm::read_credential_projection_bytes_in(&state_dir, "identity_sled")?
            .ok_or_else(|| anyhow::anyhow!("committed identity credential projection is absent"))?;
    same_identity_contract(&contract)?;
    let value: JsonValue = serde_json::from_slice(&bytes)?;
    anyhow::ensure!(
        value.get("sleds").is_some_and(JsonValue::is_array),
        "identity sleds missing"
    );
    anyhow::ensure!(
        value.get("events").is_some_and(JsonValue::is_array),
        "identity events missing"
    );
    let state: SledCacheState = serde_json::from_value(value)?;
    let mut ids = HashSet::new();
    for sled in &state.sleds {
        anyhow::ensure!(
            ids.insert(sled.session_id.as_str()),
            "duplicate identity in SHM"
        );
        validate_sled(sled)?;
    }
    let mut events = HashSet::new();
    for event in &state.events {
        anyhow::ensure!(
            events.insert((event.session_id.as_str(), event.seq)),
            "duplicate event in SHM"
        );
    }
    Ok(state)
}

async fn preflight_native_identity_effect() -> anyhow::Result<()> {
    tokio::task::spawn_blocking(|| {
        let contract = strict_identity_contract()?;
        let state_dir = op_core::projection_shm::shm_state_dir();
        anyhow::ensure!(
            op_core::projection_shm::pending_identity_publication_in(&state_dir)?.is_none(),
            "identity publication is pending recovery; native lifecycle is blocked"
        );
        same_identity_contract(&contract)
    })
    .await?
}

async fn read_private_state() -> anyhow::Result<SledCacheState> {
    tokio::task::spawn_blocking(strict_private_state).await?
}
fn publication_value<T>(durable: DurableResult<T>) -> anyhow::Result<T> {
    tracing::debug!(replayed = durable.replayed, "identity native intent committed");
    Ok(durable.value)
}
async fn require_published_identity(store: &IdentitySledStore) -> anyhow::Result<()> {
    let cozo = store.cozo.clone();
    tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
        let published = strict_private_state()?;
        let actual = state_value(&cozo.read_identity_recovery_state(MAX_EVENTS_IN_STATE)?)?;
        ensure!(serde_json::to_value(published)? == serde_json::to_value(actual)?,
            "identity replay requires already committed native credentials");
        Ok(())
    }).await?
}
async fn commit_cas<T: Send + 'static>(
    engine: &NativeOwner,
    store: &IdentitySledStore,
    operation_id: String,
    session_id: String,
    expected: IdentityRevision,
    record: Option<IdentitySledRecord>,
    genesis: Option<GenesisInputsRecord>,
    value: T,
) -> anyhow::Result<T> {
    let cozo = store.cozo.clone();
    let publication = engine
        .commit_identity_intent(operation_id.clone(), move |_| {
            let committed = cozo.apply_identity_intent(
                &session_id,
                &operation_id,
                IdentityIntent::CompareExchange(Box::new(IdentityCompareExchange {
                    expected,
                    record,
                    genesis,
                })),
            )?;
            let snapshot = cozo.read_identity_recovery_state(MAX_EVENTS_IN_STATE)?;
            Ok((
                Some(state_value(&snapshot)?),
                DurableResult {
                    value,
                    replayed: committed.replayed,
                },
            ))
        })
        .await?;
    publication_value(publication)
}

async fn load_revision(
    store: &IdentitySledStore,
    session_id: &str,
) -> anyhow::Result<IdentityRevision> {
    let cozo = store.cozo.clone();
    let session_id = session_id.to_string();
    let allow_pending_rename = store.allow_pending_rename;
    tokio::task::spawn_blocking(move || {
        ensure!(allow_pending_rename || cozo.read_container_rename(&session_id)?
            .is_none_or(|journal| journal.committed_revision != 0),
            "identity has an unresolved native rename; reconcile its original operation before lifecycle effects");
        Ok::<_, anyhow::Error>(cozo.read_identity_revision(&session_id)?)
    })
    .await?
}

fn revision_sled(revision: &IdentityRevision) -> anyhow::Result<Option<ContainerIdentitySled>> {
    revision
        .record
        .as_ref()
        .map(|record| record_to_sled(record, revision.genesis.as_ref()))
        .transpose()
}
async fn commit_sled_update<T: Send + 'static>(
    engine: &NativeOwner,
    store: &IdentitySledStore,
    operation_id: String,
    expected: IdentityRevision,
    sled: ContainerIdentitySled,
    value: T,
) -> anyhow::Result<T> {
    let genesis = expected.genesis.clone();
    let record = sled_to_record(&sled)?;
    commit_cas(
        engine,
        store,
        operation_id,
        sled.session_id.clone(),
        expected,
        Some(record),
        genesis,
        value,
    )
    .await
}
fn redact_own_identity(identity: ContainerIdentitySled) -> anyhow::Result<JsonValue> {
    // Arbitrary Incus config, descriptions and device payloads may contain
    // credentials. Keep only the typed instance lifecycle/storage metadata.
    let instance = identity.instance.map(|instance| IncusInstance {
        name: instance.name,
        status: instance.status,
        instance_type: instance.instance_type,
        status_code: instance.status_code,
        storage_pool: instance.storage_pool,
        ..IncusInstance::default()
    });
    // An explicit allowlist also withholds future credential-bearing record
    // fields. This remains a projection of the existing typed identity output.
    Ok(serde_json::json!({"identity": {
        "session_id": identity.session_id,
        "wireguard_pubkey": identity.wireguard_pubkey,
        "principal_kind": identity.principal_kind,
        "mutation_index": identity.mutation_index,
        "genesis": identity.genesis,
        "trace_id": identity.trace_id,
        "schema_version": identity.schema_version,
        "btrfs_device": identity.btrfs_device,
        "instance": instance,
        "session_started_at": identity.session_started_at,
        "last_seen_at": identity.last_seen_at,
        "active": identity.active,
        "expires_at": identity.expires_at,
        "arrival_timestamp": identity.arrival_timestamp,
        "chain_head_at_arrival": identity.chain_head_at_arrival,
        "catalog_hash_at_arrival": identity.catalog_hash_at_arrival,
        "head_timestamp_at_arrival": identity.head_timestamp_at_arrival
    }}))
}
pub(crate) struct IdentityRootStartGuard {
    should_start: bool,
    _legacy: Option<root_repair::RootStartGuard>,
    _physical: Option<physical_root::PhysicalRootGuard>,
    _clean: Option<clean_provision::CleanRootGuard>,
}

impl From<root_repair::RootStartGuard> for IdentityRootStartGuard {
    fn from(guard: root_repair::RootStartGuard) -> Self {
        Self {
            should_start: guard.should_start,
            _legacy: Some(guard),
            _physical: None,
            _clean: None,
        }
    }
}
impl From<physical_root::PhysicalRootGuard> for IdentityRootStartGuard {
    fn from(guard: physical_root::PhysicalRootGuard) -> Self {
        Self {
            should_start: guard.should_start,
            _legacy: None,
            _physical: Some(guard),
            _clean: None,
        }
    }
}
impl From<clean_provision::CleanRootGuard> for IdentityRootStartGuard {
    fn from(guard: clean_provision::CleanRootGuard) -> Self {
        Self { should_start: guard.should_start, _legacy: None, _physical: None, _clean: Some(guard) }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum IdentityStorageAttachment {
    /// Verify externally mounted storage before marking it logically attached.
    Attached,
}

type IdentityStorageVerifier = Arc<
    dyn Fn(IdentityStorageAttachment, &std::path::Path, &std::path::Path, &str) -> anyhow::Result<()>
        + Send
        + Sync,
>;

fn native_identity_storage_verifier() -> IdentityStorageVerifier {
    Arc::new(|_effect, device, mount, uuid| {
        op_network::btrfs::verify_identity_mount(device, mount, uuid)
            .context("identity storage must already be mounted by the external host provisioner or Incus; the bridge does not mount storage")
    })
}
async fn apply_identity_btrfs(
    sled: &mut ContainerIdentitySled,
    effect: IdentityStorageAttachment,
    effector: &IdentityStorageVerifier,
) -> anyhow::Result<()> {
    let Some(device) = sled.btrfs_device.clone() else {
        return Ok(());
    };
    anyhow::ensure!(
        !device.device_path.is_empty()
            && !device.mount_point.is_empty()
            && !device.btrfs_uuid.is_empty(),
        "registered identity filesystem binding is incomplete"
    );
    let path = PathBuf::from(device.device_path);
    let mount = PathBuf::from(device.mount_point);
    let uuid = device.btrfs_uuid;
    let effector = effector.clone();
    tokio::task::spawn_blocking(move || effector(effect, &path, &mount, &uuid)).await??;
    if let Some(device) = sled.btrfs_device.as_mut() {
        device.attached = matches!(effect, IdentityStorageAttachment::Attached);
    }
    Ok(())
}
async fn prepare_identity_root_start(
    store: &IdentitySledStore,
    expected: &IdentityRevision,
    sled: &mut ContainerIdentitySled,
    effector: &IdentityStorageVerifier,
) -> anyhow::Result<Option<IdentityRootStartGuard>> {
    if let Some(guard) = clean_provision::guard_start(store, expected, sled).await? {
        if let Some(device) = sled.btrfs_device.as_mut() { device.attached = true; }
        return Ok(Some(guard.into()));
    }
    let block_backed = sled
        .btrfs_device
        .as_ref()
        .is_some_and(|device| std::path::Path::new(&device.device_path).starts_with("/dev"));
    if block_backed {
        // Physical root guards may require this configured mount to exist.
        apply_identity_btrfs(sled, IdentityStorageAttachment::Attached, effector).await?;
    } else if let Some(device) = sled.btrfs_device.as_mut() {
        // Historical image-root guards require logical attachment while
        // verifying an externally prepared sprout. This provisional flag
        // remains local until all storage verification and readback succeed.
        device.attached = true;
    }
    let guard = if sled.instance.is_none() {
        None
    } else {
        match physical_root::guard_start(store, expected, sled).await? {
            Some(guard) => Some(IdentityRootStartGuard::from(guard)),
            None => match root_repair::guard_own_root_start(store, expected, sled).await? {
                Some(guard) => Some(IdentityRootStartGuard::from(guard)),
                None => seed_provision::guard_start(store, expected, sled)
                    .await?
                    .map(IdentityRootStartGuard::from),
            },
        }
    };
    if !block_backed {
        apply_identity_btrfs(sled, IdentityStorageAttachment::Attached, effector).await?;
    }
    Ok(guard)
}

async fn stored_session(session_id: &str) -> anyhow::Result<Option<ContainerIdentitySled>> {
    Ok(read_private_state().await?.sleds.into_iter().find(|sled| sled.session_id == session_id))
}
fn disclosed_identity(mut sled: ContainerIdentitySled, caller: &VerifiedCaller) -> anyhow::Result<JsonValue> {
    if sled.session_id == caller.session_id && sled.genesis.as_deref() == Some(caller.session_genesis.as_str()) {
        return Ok(serde_json::json!({"identity": sled}));
    }
    sled.sealed_id = None;
    redact_own_identity(sled)
}
async fn identity_method(engine: &NativeOwner, store: &IdentitySledStore, method: &str, args: &JsonValue)
    -> anyhow::Result<JsonValue> {
    engine.revalidate().await?;
    require_published_identity(store).await?;
    let actor = VerifiedIdentityActor { actor_id: &engine.caller.principal_id,
        session_id: &engine.caller.session_id, genesis: &engine.caller.session_genesis };
    match method {
        "prepare_container" => prepare_container(engine, store, actor, args).await,
        "rename_container" => rename::rename(engine, store, serde_json::from_value(args.clone())?).await,
        "get_identity" | "get_own_identity" => {
            let target = if method == "get_own_identity" { engine.caller.session_id.clone() }
                else if arg_str(args, "session_id").is_empty() {
                    std::env::var("IDENTITY_SLED_HOST_SESSION_ID").ok().filter(|s| !s.trim().is_empty())
                        .context("host identity session is not configured")?.trim().to_owned()
                } else { arg_str(args, "session_id").into() };
            let sled = stored_session(&target).await?.context("identity session not found")?;
            engine.revalidate().await?;
            if method == "get_own_identity" {
                engine.verify_own(&sled).await?;
                redact_own_identity(sled)
            } else { disclosed_identity(sled, &engine.caller) }
        }
        "inspect_own_root_repair_preflight" => root_repair::inspect_own_root_repair_preflight(
            engine, store, actor.actor_id, actor.session_id, actor.genesis).await,
        "repair_own_root" => root_repair::repair_own_root(engine, store, actor,
            serde_json::from_value(args.clone())?, root_repair::capture_repair_seal()?, engine.operation_id.clone()).await,
        "migrate_own_physical_root" => physical_root::migrate(engine, store, actor, serde_json::from_value(args.clone())?).await,
        "inspect_own_physical_root_migration_preflight" => physical_root::preflight(engine, store, actor).await,
        "provision_container" | "inspect_provision_preflight" => {
            let input = seed_provision::ProvisionInput { wireguard_pubkey: arg_str(args, "wireguard_pubkey").into(),
                plan_sha256: arg_str(args, "plan_sha256").into() };
            let result = clean_provision::provision(engine, store, actor, input,
                seed_provision::capture_method_seal(method)?, &engine.operation_id, method == "inspect_provision_preflight").await?;
            if method == "provision_container" {
                let mut result = result;
                let sled: ContainerIdentitySled = serde_json::from_value(result["identity"].clone())?;
                result["identity"] = disclosed_identity(sled, &engine.caller)?["identity"].take();
                Ok(result)
            } else { Ok(result) }
        }
        "delete_provision_record" => delete_provision::delete(engine, store, actor,
            serde_json::from_value(args.clone())?, seed_provision::capture_method_seal(method)?).await,
        "inspect_identity_inventory" => {
            let query: op_cozo_store::IdentityInventoryQuery = serde_json::from_value(args.clone())?;
            let cozo = store.cozo.clone();
            let result = tokio::task::spawn_blocking(move || cozo.inspect_identity_inventory(query)).await??;
            engine.revalidate().await?;
            Ok(serde_json::to_value(result)?)
        }
        "refresh_credential" => refresh_credential(engine, store).await,
        "write_identity" => {
            let key = arg_str(args, "wireguard_pubkey");
            canonical_key(key)?;
            let session = op_identity::session::derive_session_id(key);
            engine.retain(store.lifecycle_lock(&session).await.lock_owned().await);
            let expected = load_revision(store, &session).await?;
            let mut sled = revision_sled(&expected)?.context("explicit provision is required first")?;
            ensure!(sled.wireguard_pubkey == key && sled.is_anchored() && sled.sealed_id.is_some(),
                "write_identity cannot mint or replace a missing identity anchor");
            if let Some(value) = args.get("btrfs_device") {
                let supplied: Option<op_plugins::state_plugins::identity_sled::SledBtrfsDevice> = serde_json::from_value(value.clone())?;
                ensure!(supplied == sled.btrfs_device, "storage binding changes require the explicit native maintenance owner");
            }
            if let Some(interface) = args.get("interface").and_then(JsonValue::as_str) { sled.interface = interface.into(); }
            if let Some(peer_ip) = args.get("peer_ip") { sled.peer_ip = serde_json::from_value(peer_ip.clone())?; }
            sled.last_seen_at = now();
            sled.mutation_index = sled.mutation_index.checked_add(1).context("identity mutation index overflow")?;
            engine.revalidate().await?;
            let sled = commit_sled_update(engine, store, engine.operation_id.clone(), expected, sled.clone(), sled).await?;
            disclosed_identity(sled, &engine.caller)
        }
        "attach_btrfs_device" | "touch_session" => {
            lifecycle(engine, store, arg_str(args, "session_id"), method == "touch_session").await
        }
        "record_session_event" => {
            let session = arg_str(args, "session_id").to_owned();
            let kind = arg_str(args, "kind").to_owned();
            ensure!(!session.is_empty() && !kind.is_empty(), "session event owner and kind are required");
            engine.retain(store.lifecycle_lock(&session).await.lock_owned().await);
            ensure!(load_revision(store, &session).await?.record.is_some(), "identity session is absent");
            let cozo = store.cozo.clone();
            let subid = arg_str(args,"subid").to_owned();
            let content = arg_str(args,"content").to_owned();
            let target = session.clone();
            let publication = engine.commit_identity_intent(engine.operation_id.clone(), move |_| {
                let seq = cozo.append_session_event(&target, &kind, &subid, &content, now())?;
                let snapshot = cozo.read_identity_recovery_state(MAX_EVENTS_IN_STATE)?;
                Ok((Some(state_value(&snapshot)?), DurableResult { value: u64::try_from(seq)?, replayed: false }))
            }).await?;
            Ok(serde_json::json!({"session_id":session,"seq":publication_value(publication)?}))
        }
        "get_session_history" => {
            let session = arg_str(args,"session_id").to_owned();
            ensure!(!session.is_empty(), "history session is required");
            let requested = usize::try_from(args.get("limit").and_then(JsonValue::as_u64).unwrap_or(0))?;
            let limit = if requested == 0 { 100 } else { requested };
            ensure!(limit <= MAX_HISTORY_LIMIT, "identity history limit exceeded");
            let cozo = store.cozo.clone();
            let rows = tokio::task::spawn_blocking(move || cozo.list_session_events(&session, limit)).await??;
            engine.revalidate().await?;
            Ok(serde_json::json!({"events":rows.iter().map(event_from_record).collect::<anyhow::Result<Vec<_>>>()?}))
        }
        _ => anyhow::bail!("identity owner method was not admitted"),
    }
}

async fn lifecycle(engine: &NativeOwner, store: &IdentitySledStore, session: &str, start: bool)
    -> anyhow::Result<JsonValue> {
    ensure!(!session.is_empty(), "identity lifecycle requires an exact target");
    engine.retain(store.lifecycle_lock(session).await.lock_owned().await);
    let expected = load_revision(store, session).await?;
    let mut sled = revision_sled(&expected)?.context("identity session is absent")?;
    ensure!(sled.is_anchored() && sled.sealed_id.is_some(), "identity lifecycle requires existing credentials");
    preflight_native_identity_effect().await?;
    let guard = prepare_identity_root_start(store, &expected, &mut sled, &native_identity_storage_verifier()).await?;
    let should_start = guard.as_ref().map(|g| g.should_start);
    if let Some(guard) = guard { engine.retain(guard); }
    ensure!(load_revision(store, session).await? == expected, "identity revision changed before lifecycle effect");
    engine.revalidate().await?;
    if start {
        if let Some(bound) = &sled.instance {
            ensure!(identity_instance_matches(&sled, bound), "native target differs from durable identity");
            let before = IncusRootRepairBackend::local().default_project_state().await?;
            let instance = before.instances.iter().find(|i| i.name == bound.name).context("native identity instance absent")?;
            ensure!(identity_instance_matches(&sled, instance), "native identity owner changed");
            let start_required = root_repair::decide_seeded_native_start(sled.active, &instance.status)?;
            ensure!(should_start.is_none_or(|expected| expected == start_required), "native lifecycle guard raced");
            let effect = if start_required {
                store.mark_effect();
                IncusPlugin::set_bound_identity_session_power_state(session, &bound.name, IdentitySessionPowerState::Running).await
            } else { Ok(()) };
            // A failed native request can still have taken effect. Observe once, never replay.
            let snapshot = IncusRootRepairBackend::local().default_project_state().await?;
            engine.observe_incus(&snapshot).await?;
            let observed = snapshot.instances.iter().find(|i| i.name == bound.name).context("native lifecycle readback absent")?;
            ensure!(identity_instance_matches(&sled, observed), "native identity owner changed during lifecycle");
            effect?;
            ensure!(observed.status == "Running", "native identity did not start");
            sled.active = true;
            // Re-verify the exact existing storage receipt, never invent a Running record.
            let verified = if let Some(instance) = clean_provision::observe_power(engine, store, &expected, &sled, "Running").await? {
                Some(instance)
            } else if let Some(instance) = physical_root::observe_power(engine, store, &expected, &sled, "Running").await? {
                Some(instance)
            } else if let Some(instance) = root_repair::observe_owned_power_effect(engine, store, &expected, &sled, "Running").await? {
                Some(instance)
            } else { seed_provision::observe_power(engine, store, &expected, &sled, "Running").await? };
            sled.instance = Some(verified.unwrap_or_else(|| observed.clone()));
        } else {
            ensure!(sled.principal_kind.as_deref() == Some("human"), "identity has no native or human binding");
            sled.active = true;
        }
    }
    sled.last_seen_at = now();
    sled.mutation_index = sled.mutation_index.checked_add(1).context("identity mutation index overflow")?;
    let result = commit_sled_update(engine, store, engine.operation_id.clone(), expected, sled.clone(), sled).await?;
    disclosed_identity(result, &engine.caller)
}

async fn refresh_credential(engine: &NativeOwner, store: &IdentitySledStore) -> anyhow::Result<JsonValue> {
    let session = &engine.caller.session_id;
    engine.retain(store.lifecycle_lock(session).await.lock_owned().await);
    let expected = load_revision(store, session).await?;
    let sled = revision_sled(&expected)?.context("refresh identity is absent")?;
    engine.verify_own(&sled).await?;
    ensure!(sled.principal_kind.as_deref() == Some("human") && sled.schema_version == RECORD_FORMAT
        && identity_session_binding_matches(&sled), "refresh requires a current anchored human identity");
    let old = sled.sealed_id.as_deref().context("authored credential is absent")?;
    let mut claims = op_identity::sealed_id::SealedId::from_inline_ref(old)
        .map_err(|error| anyhow::anyhow!("stored identity credential is invalid: {error}"))?;
    ensure!(matches!(claims.transport_scope.as_str(), "dbus,grpc,mcp" | "dbus,grpc,mcp,mqtt"),
        "stored transport scope is not refreshable");
    claims.transport_scope = "dbus,grpc,mcp,mqtt".into();
    claims.nextdns = crate::identity_nextdns::NextDnsBindings::from_env()?.get(&sled.wireguard_pubkey).cloned();
    crate::runtime_identity::validate_refresh_claims(engine.caller.claims(), &claims, now())?;
    let candidate = claims.to_inline_ref().map_err(|error| anyhow::anyhow!("seal identity: {error}"))?;
    if candidate == old {
        require_published_identity(store).await?;
        return Ok(serde_json::json!({"refreshed":false,"session_id":session,
            "transport_scope":claims.transport_scope,"sealed_id":old}));
    }
    let cozo = store.cozo.clone();
    let target = session.clone();
    let operation = engine.operation_id.clone();
    let expected_credential = candidate.clone();
    let publication = engine.commit_identity_intent(operation.clone(), move |_| {
        let committed = cozo.apply_identity_intent(&target, &operation,
            IdentityIntent::RefreshTransportCredential { expected: Box::new(expected), sealed_id: candidate })?;
        let snapshot = cozo.read_identity_recovery_state(MAX_EVENTS_IN_STATE)?;
        Ok((Some(state_value(&snapshot)?), DurableResult { value: (), replayed: committed.replayed }))
    }).await?;
    publication_value(publication)?;
    let published = stored_session(session).await?.context("refreshed credential publication is absent")?;
    ensure!(published.sealed_id.as_deref() == Some(expected_credential.as_str()) && published.genesis == sled.genesis,
        "refreshed credential does not match native publication");
    Ok(serde_json::json!({"refreshed":true,"session_id":session,
        "transport_scope":claims.transport_scope,"sealed_id":expected_credential}))
}

async fn prepare_container(engine: &NativeOwner, store: &IdentitySledStore,
    actor: VerifiedIdentityActor<'_>, args: &JsonValue) -> anyhow::Result<JsonValue> {
    let key = arg_str(args, "wireguard_pubkey");
    canonical_key(key)?;
    let session = op_identity::session::derive_session_id(key);
    ensure!(configured_chatbot_key().as_deref() == Some(key)
        && control_plane_chatbot_session_id().as_deref() == Some(session.as_str()),
        "prepare is restricted to the configured service identity");
    engine.retain(store.lifecycle_lock(&session).await.lock_owned().await);
    seed_provision::admit(engine, store, actor.actor_id, actor.session_id, actor.genesis,
        &seed_provision::capture_method_seal("prepare_container")?).await?;
    seed_provision::target_key_is_registered(key).await?;
    preflight_native_identity_effect().await?;
    let operation = arg_str(args, "operation_id").to_owned();
    let cozo = store.cozo.clone();
    let records = tokio::task::spawn_blocking(move || cozo.native_creation_records()).await??;
    let (body, reserved_name) = if let Some(record) = records.iter().find(|record| record.request.operation_id == operation) {
        ensure!(record.request.owner_kind == "identity" && record.request.logical_target == session
            && record.request.actor_id == actor.actor_id && record.request.caller_session == actor.session_id
            && record.request.caller_genesis == actor.genesis,
            "prepare operation belongs to a different caller or target");
        (serde_json::from_str(&record.request.request_json)?, record.request.reserved_name.clone())
    } else {
        let revision = load_revision(store, &session).await?;
        ensure!(revision.record.is_none() && revision.genesis.is_none(),
            "existing identity binding must be retained or explicitly retired");
        let native = IncusRootRepairBackend::local().default_project_state().await?;
        ensure!(!native.instances.iter().any(|instance| instance.name == session
            || instance.name == crate::identity_binding::CONTROL_PLANE_CHATBOT_CONTAINER
            || instance.name == crate::identity_binding::RESERVED_CHATBOT_CONTAINER
            || instance.config.as_ref().is_some_and(|config| config.get("user.opdbus.session_id") == Some(&session))),
            "existing native identity must be retained or explicitly retired");
        (op_plugins::state_plugins::incus::identity_creation_body(&session, key).await?,
            Some(crate::identity_binding::RESERVED_CHATBOT_CONTAINER.to_owned()))
    };
    let (record, snapshot, replayed) = crate::runtime_creation::create_identity_container(
        &engine.runtime, &engine.caller, &operation, &session, body, reserved_name).await?;
    store.mark_effect();
    engine.observe_incus(&snapshot).await?;
    Ok(serde_json::json!({"session_id":session,"name":record.name,"native_uuid":record.native_uuid,
        "operation_id":record.request.operation_id,"phase":record.phase,"replayed":replayed}))
}
