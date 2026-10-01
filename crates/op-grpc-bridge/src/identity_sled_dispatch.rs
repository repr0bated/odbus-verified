//! Atomic identity_sled dispatch over Cozo CAS plus the paired SHM credential commit.
//!
//! The identity subsystem remains the sole SID1 author. Cozo is the durable owner;
//! private SHM is the only runtime read authority. The MutationEngine wrapper owns
//! public redaction, schema validation, paired SHM publication, cache mirroring and
//! notification. No path in this module writes a legacy row or a whole stale array.

use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::Arc,
};

use anyhow::Context as _;
use async_trait::async_trait;
use base64::Engine as _;
use op_core::projection_shm::CredentialPublication;
use op_cozo_store::{
    CozoGraphShuttle, GenesisInputsRecord, IdentityCompareExchange, IdentityIntent,
    IdentityRecoverySnapshot, IdentityRevision, IdentitySledRecord, SessionEventRecord,
};
#[cfg(test)]
use op_plugins::state_plugins::incus_device::Device;
use op_plugins::state_plugins::{
    identity_sled::{
        ContainerIdentitySled, IdentitySledState, RECORD_FORMAT, SCHEMA_CONTENT_HASH, SessionEvent,
    },
    incus::{IdentitySessionPowerState, IncusInstance, IncusPlugin},
};
use serde_json::Value as JsonValue;

use crate::mutation_engine::{GenesisStamp, MutationEngine};

#[path = "identity_physical_root.rs"]
mod physical_root;
#[path = "identity_root_repair.rs"]
mod root_repair;
#[path = "identity_seed_provision.rs"]
mod seed_provision;

pub(crate) struct IdentityRootStartGuard {
    should_start: bool,
    _legacy: Option<root_repair::RootStartGuard>,
    _physical: Option<physical_root::PhysicalRootGuard>,
}

impl From<root_repair::RootStartGuard> for IdentityRootStartGuard {
    fn from(guard: root_repair::RootStartGuard) -> Self {
        Self {
            should_start: guard.should_start,
            _legacy: Some(guard),
            _physical: None,
        }
    }
}
impl From<physical_root::PhysicalRootGuard> for IdentityRootStartGuard {
    fn from(guard: physical_root::PhysicalRootGuard) -> Self {
        Self {
            should_start: guard.should_start,
            _legacy: None,
            _physical: Some(guard),
        }
    }
}

struct VerifiedIdentityActor<'a> {
    actor_id: &'a str,
    session_id: &'a str,
    genesis: &'a str,
}

const MAX_EVENTS_IN_STATE: usize = 256;
const MAX_HISTORY_LIMIT: usize = 4096;

#[derive(Debug)]
struct DurableResult<T> {
    value: T,
    replayed: bool,
}

/// A native persistence diagnostic can contain bound credentials or events.
/// Keep it out of transport errors, including alternate/debug source chains.
struct IdentityPublicationPending {
    operation_id: String,
    phase: &'static str,
    _private_cause: anyhow::Error,
}

impl std::fmt::Display for IdentityPublicationPending {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "identity operation {} pending recovery ({})",
            self.operation_id, self.phase
        )
    }
}

impl std::fmt::Debug for IdentityPublicationPending {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(self, f)
    }
}

impl std::error::Error for IdentityPublicationPending {}

/// Store ownership is injected once by MutationEngine startup. Opening failure is
/// returned by startup and can be retried by restarting startup; it is never cached
/// as `None` in a process-global OnceLock.
#[derive(Clone)]
pub(crate) struct IdentitySledStore {
    cozo: CozoGraphShuttle,
    lifecycle_locks:
        Arc<tokio::sync::Mutex<HashMap<String, std::sync::Weak<tokio::sync::Mutex<()>>>>>,
}

impl IdentitySledStore {
    pub(crate) fn new(cozo: CozoGraphShuttle) -> Self {
        Self {
            cozo,
            lifecycle_locks: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
        }
    }

    pub(crate) fn cozo(&self) -> &CozoGraphShuttle {
        &self.cozo
    }

    async fn lifecycle_lock(&self, session_id: &str) -> Arc<tokio::sync::Mutex<()>> {
        let mut locks = self.lifecycle_locks.lock().await;
        locks.retain(|_, lock| lock.strong_count() != 0);
        if let Some(lock) = locks.get(session_id).and_then(std::sync::Weak::upgrade) {
            return lock;
        }
        let lock = Arc::new(tokio::sync::Mutex::new(()));
        locks.insert(session_id.to_string(), Arc::downgrade(&lock));
        lock
    }
}

impl From<CozoGraphShuttle> for IdentitySledStore {
    fn from(cozo: CozoGraphShuttle) -> Self {
        Self::new(cozo)
    }
}

#[async_trait]
pub(crate) trait IdentityContainerLifecycle: Send + Sync {
    async fn start(&self, session_id: &str) -> anyhow::Result<()>;
    async fn stop(&self, session_id: &str) -> anyhow::Result<()>;
}

#[derive(Clone)]
struct IncusIdentityContainerLifecycle;

#[async_trait]
impl IdentityContainerLifecycle for IncusIdentityContainerLifecycle {
    async fn start(&self, session_id: &str) -> anyhow::Result<()> {
        IncusPlugin::set_identity_session_power_state(
            session_id,
            IdentitySessionPowerState::Running,
        )
        .await
    }
    async fn stop(&self, session_id: &str) -> anyhow::Result<()> {
        IncusPlugin::set_identity_session_power_state(
            session_id,
            IdentitySessionPowerState::Stopped,
        )
        .await
    }
}

type SledCacheState = IdentitySledState;
type BtrfsMountVerifier =
    Arc<dyn Fn(&std::path::Path, &std::path::Path, &str) -> anyhow::Result<()> + Send + Sync>;

fn native_btrfs_mount_verifier() -> BtrfsMountVerifier {
    Arc::new(op_network::btrfs::verify_dedicated_mount)
}

fn parse_json<T: serde::de::DeserializeOwned>(
    label: &str,
    session_id: &str,
    json: &str,
) -> anyhow::Result<Option<T>> {
    if json.is_empty() {
        return Ok(None);
    }
    serde_json::from_str(json)
        .map(Some)
        .map_err(|error| anyhow::anyhow!("invalid durable {label} for {session_id}: {error}"))
}

fn record_to_sled(
    record: &IdentitySledRecord,
    genesis: Option<&GenesisInputsRecord>,
) -> anyhow::Result<ContainerIdentitySled> {
    anyhow::ensure!(
        record.mutation_index >= 0,
        "negative durable mutation index"
    );
    anyhow::ensure!(
        record.schema_version >= 0,
        "negative durable schema version"
    );
    let mut sled = ContainerIdentitySled {
        session_id: record.session_id.clone(),
        wireguard_pubkey: record.wireguard_pubkey.clone(),
        interface: record.interface.clone(),
        peer_ip: (!record.peer_ip.is_empty()).then(|| record.peer_ip.clone()),
        mutation_index: u64::try_from(record.mutation_index)?,
        genesis: (!record.session_genesis.is_empty()).then(|| record.session_genesis.clone()),
        trace_id: record.trace_id.clone(),
        schema_version: u32::try_from(record.schema_version)?,
        vector_id: record.vector_id.clone(),
        sealed_id: (!record.sealed_id.is_empty()).then(|| record.sealed_id.clone()),
        principal_kind: record.principal_kind.clone(),
        btrfs_device: parse_json(
            "btrfs_device",
            &record.session_id,
            &record.btrfs_device_json,
        )?,
        instance: parse_json("instance", &record.session_id, &record.instance_json)?,
        session_started_at: record.session_started_at,
        last_seen_at: record.last_seen_at,
        active: record.active,
        expires_at: (record.expires_at != 0).then_some(record.expires_at),
        arrival_timestamp: 0,
        chain_head_at_arrival: String::new(),
        catalog_hash_at_arrival: String::new(),
        head_timestamp_at_arrival: 0,
    };
    if let Some(genesis) = genesis {
        anyhow::ensure!(
            genesis.schema_content_hash.is_empty()
                || genesis.schema_content_hash == SCHEMA_CONTENT_HASH.trim(),
            "identity schema content hash drift for {}",
            record.session_id
        );
        sled.arrival_timestamp = genesis.arrival_timestamp;
        sled.chain_head_at_arrival = genesis.chain_head_at_arrival.clone();
        sled.catalog_hash_at_arrival = genesis.catalog_hash_at_arrival.clone();
        sled.head_timestamp_at_arrival = genesis.head_timestamp_at_arrival;
    }
    validate_sled(&sled)?;
    Ok(sled)
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

fn validate_sled(sled: &ContainerIdentitySled) -> anyhow::Result<()> {
    anyhow::ensure!(!sled.session_id.is_empty(), "identity session id is empty");
    anyhow::ensure!(
        sled.mutation_index <= i64::MAX as u64,
        "mutation index overflow"
    );
    if let Some(instance) = &sled.instance {
        anyhow::ensure!(
            instance.name == sled.session_id && instance.instance_type == "container",
            "identity instance does not match session container"
        );
    }
    if sled.is_anchored() {
        anyhow::ensure!(
            sled.sealed_id
                .as_deref()
                .is_some_and(|value| !value.is_empty()),
            "anchored identity lacks authored SID1"
        );
    }
    Ok(())
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

fn recovery_to_state(snapshot: &IdentityRecoverySnapshot) -> anyhow::Result<SledCacheState> {
    let mut ids = HashSet::new();
    let mut sleds = Vec::new();
    for revision in &snapshot.identities {
        anyhow::ensure!(
            ids.insert(revision.session_id.as_str()),
            "duplicate identity revision"
        );
        let Some(record) = &revision.record else {
            continue;
        };
        anyhow::ensure!(
            record.session_id == revision.session_id,
            "revision row mismatch"
        );
        if let Some(genesis) = &revision.genesis {
            anyhow::ensure!(
                genesis.session_id == revision.session_id,
                "genesis row mismatch"
            );
        }
        sleds.push(record_to_sled(record, revision.genesis.as_ref())?);
    }
    sleds.sort_by(|left, right| left.session_id.cmp(&right.session_id));
    let events = snapshot
        .events
        .iter()
        .map(event_from_record)
        .collect::<anyhow::Result<Vec<_>>>()?;
    Ok(SledCacheState { sleds, events })
}

fn state_value(snapshot: &IdentityRecoverySnapshot) -> anyhow::Result<simd_json::OwnedValue> {
    Ok(simd_json::serde::to_owned_value(recovery_to_state(
        snapshot,
    )?)?)
}

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

fn publication_value<T>(publication: CredentialPublication<DurableResult<T>>) -> anyhow::Result<T> {
    match publication {
        CredentialPublication::Committed { durable, .. } => {
            tracing::debug!(replayed = durable.replayed, "identity intent committed");
            Ok(durable.value)
        }
        CredentialPublication::Pending {
            operation_id,
            durable,
            error,
        } => {
            let phase = if durable.is_some() {
                "durable intent may have committed"
            } else {
                "durable outcome is uncertain"
            };
            Err(IdentityPublicationPending {
                operation_id,
                phase,
                _private_cause: error,
            }
            .into())
        }
    }
}

// Keep operation/session identity and the full expected/candidate pair explicit
// at this single durable identity transaction boundary.
#[allow(clippy::too_many_arguments)]
async fn commit_cas<T: Send + 'static>(
    engine: &MutationEngine,
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
    tokio::task::spawn_blocking(move || {
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

pub(crate) async fn recover_identity_startup_with_store(
    engine: &MutationEngine,
    store: &IdentitySledStore,
) -> anyhow::Result<()> {
    let state_dir = op_core::projection_shm::shm_state_dir();
    let pending = tokio::task::spawn_blocking(move || {
        op_core::projection_shm::pending_identity_publication_in(&state_dir)
    })
    .await??;
    let cozo = store.cozo.clone();
    let publication = if let Some(pending) = pending {
        engine
            .recover_identity_projection(pending.operation_id, move || {
                let snapshot = cozo.read_identity_recovery_state(MAX_EVENTS_IN_STATE)?;
                Ok((
                    Some(state_value(&snapshot)?),
                    DurableResult {
                        value: (),
                        replayed: true,
                    },
                ))
            })
            .await?
    } else {
        let operation_id = format!("startup-{}", uuid::Uuid::new_v4().simple());
        engine
            .commit_identity_intent(operation_id, move |_| {
                let snapshot = cozo.read_identity_recovery_state(MAX_EVENTS_IN_STATE)?;
                Ok((
                    Some(state_value(&snapshot)?),
                    DurableResult {
                        value: (),
                        replayed: true,
                    },
                ))
            })
            .await?
    };
    publication_value(publication)
}

pub(crate) async fn recover_identity_startup(engine: &MutationEngine) -> anyhow::Result<()> {
    recover_identity_startup_with_store(engine, engine.identity_sled_store()?).await
}

pub(crate) async fn stored_session(
    _engine: &MutationEngine,
    session_id: &str,
) -> anyhow::Result<Option<ContainerIdentitySled>> {
    Ok(read_private_state()
        .await?
        .sleds
        .into_iter()
        .find(|sled| sled.session_id == session_id))
}

/// Every sled in the committed credential projection (doorbell revalidation).
pub(crate) async fn all_sessions(
    _engine: &MutationEngine,
) -> anyhow::Result<Vec<ContainerIdentitySled>> {
    Ok(read_private_state().await?.sleds)
}

pub(crate) async fn session_record_for_actor(
    _engine: &MutationEngine,
    actor_id: &str,
) -> anyhow::Result<Option<ContainerIdentitySled>> {
    if actor_id.is_empty() {
        return Ok(None);
    }
    Ok(read_private_state().await?.sleds.into_iter().find(|sled| {
        sled.session_id == actor_id
            || sled.wireguard_pubkey == actor_id
            // The principal id is the canonical actor of every dispatched
            // method; authoritative changes announced under it must stamp
            // the same session identity as the method call did.
            || op_identity::session::derive_principal_id(&sled.wireguard_pubkey) == actor_id
            || (!sled.trace_id.is_empty() && sled.trace_id == actor_id)
            || sled.genesis.as_deref() == Some(actor_id)
    }))
}

pub(crate) async fn authenticate_sid1_wire(
    engine: &MutationEngine,
    wire: &[u8],
    transport: &str,
) -> Result<op_identity::sealed_id::SealedId, String> {
    let claims = op_identity::sealed_id::SealedId::open(wire).map_err(|error| error.to_string())?;
    if claims.principal_kind != "wireguard-principal"
        || !claims
            .transport_scope
            .split(',')
            .any(|scope| scope.trim() == transport)
    {
        return Err(format!(
            "sealed identity is not valid for the {transport} transport"
        ));
    }
    if op_identity::session::derive_session_id(&claims.wireguard_pubkey) != claims.session_id
        || op_identity::session::derive_principal_id(&claims.wireguard_pubkey)
            != claims.principal_id
    {
        return Err(
            "sealed identity identifiers do not match its WARP-transported identity".into(),
        );
    }
    let sled = stored_session(engine, &claims.session_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "sealed identity session was not found".to_string())?;
    if !sled.instance.as_ref().is_some_and(|instance| {
        instance.name == sled.session_id && instance.instance_type == "container"
    }) {
        return Err("sealed identity has no matching provisioned container binding".into());
    }
    let now = chrono::Utc::now().timestamp();
    if !sled.is_anchored()
        || sled
            .expires_at
            .is_some_and(|expiry| expiry != 0 && expiry <= now)
    {
        return Err("sealed identity session is unanchored or expired".into());
    }
    let inline = format!(
        "{}{}",
        op_identity::sealed_id::INLINE_PREFIX,
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(wire)
    );
    if sled.sealed_id.as_deref() != Some(inline.as_str())
        || sled.wireguard_pubkey != claims.wireguard_pubkey
        || sled.genesis.as_deref() != Some(claims.session_genesis.as_str())
        || sled.trace_id != claims.trace_id
        || sled.schema_version != claims.schema_version
        || sled.expires_at.unwrap_or(0) != claims.expires_at
        || sled.arrival_timestamp != claims.arrival_timestamp
        || claims.issued_at != claims.arrival_timestamp
        || sled.chain_head_at_arrival != claims.chain_head_at_arrival
        || sled.catalog_hash_at_arrival != claims.catalog_hash_at_arrival
        || sled.head_timestamp_at_arrival != claims.head_timestamp_at_arrival
    {
        return Err("sealed identity does not match the authoritative sled".into());
    }
    // `active` is deliberately absent: it is Incus power policy, not SID1 revocation.
    Ok(claims)
}

fn apply_genesis_stamp(
    sled: &mut ContainerIdentitySled,
    stamp: &GenesisStamp,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        sled.instance.as_ref().is_some_and(|instance| {
            instance.name == sled.session_id && instance.instance_type == "container"
        }),
        "SID1 minting requires the exact provisioned identity container"
    );
    let claims = op_identity::sealed_id::SealedId::from_inline_ref(&stamp.sealed_id)
        .map_err(|error| anyhow::anyhow!("invalid authored SID1: {error}"))?;
    anyhow::ensure!(
        claims.session_id == sled.session_id && claims.session_id == stamp.session_id,
        "SID1 session mismatch"
    );
    anyhow::ensure!(
        claims.wireguard_pubkey == sled.wireguard_pubkey
            && claims.wireguard_pubkey == stamp.wireguard_pubkey,
        "SID1 key mismatch"
    );
    anyhow::ensure!(
        claims.session_genesis == stamp.genesis_hex,
        "SID1 genesis mismatch"
    );
    anyhow::ensure!(claims.trace_id == stamp.trace_id, "SID1 trace mismatch");
    anyhow::ensure!(
        claims.arrival_timestamp == stamp.arrival_timestamp
            && claims.chain_head_at_arrival == stamp.chain_head_at_arrival
            && claims.catalog_hash_at_arrival == stamp.catalog_hash_at_arrival
            && claims.head_timestamp_at_arrival == stamp.head_timestamp_at_arrival,
        "SID1 arrival metadata mismatch"
    );
    anyhow::ensure!(
        claims.expires_at == sled.expires_at.unwrap_or(0),
        "SID1 term mismatch"
    );
    sled.genesis = Some(stamp.genesis_hex.clone());
    sled.trace_id = stamp.trace_id.clone();
    sled.sealed_id = Some(stamp.sealed_id.clone());
    sled.arrival_timestamp = stamp.arrival_timestamp;
    sled.chain_head_at_arrival = stamp.chain_head_at_arrival.clone();
    sled.catalog_hash_at_arrival = stamp.catalog_hash_at_arrival.clone();
    sled.head_timestamp_at_arrival = stamp.head_timestamp_at_arrival;
    sled.schema_version = RECORD_FORMAT;
    Ok(())
}

pub(crate) async fn store_genesis_with_operation(
    engine: &MutationEngine,
    store: &IdentitySledStore,
    operation_id: String,
    stamp: &GenesisStamp,
) -> anyhow::Result<String> {
    let engine = engine.clone();
    let store = store.clone();
    let stamp = stamp.clone();
    tokio::spawn(async move { store_genesis_owned(engine, store, operation_id, stamp).await })
        .await?
}

async fn store_genesis_owned(
    engine: MutationEngine,
    store: IdentitySledStore,
    operation_id: String,
    stamp: GenesisStamp,
) -> anyhow::Result<String> {
    let lifecycle_lock = store.lifecycle_lock(&stamp.session_id).await;
    let _session_guard = lifecycle_lock.lock_owned().await;
    let expected = load_revision(&store, &stamp.session_id).await?;
    let mut sled = revision_sled(&expected)?
        .ok_or_else(|| anyhow::anyhow!("genesis requires a provisioned identity container"))?;
    anyhow::ensure!(
        sled.wireguard_pubkey == stamp.wireguard_pubkey,
        "identity key changed"
    );
    if sled.is_anchored() {
        anyhow::ensure!(
            sled.genesis.as_deref() == Some(stamp.genesis_hex.as_str()),
            "refusing re-anchor"
        );
        anyhow::ensure!(
            sled.sealed_id.as_deref() == Some(stamp.sealed_id.as_str()),
            "refusing SID1 replacement"
        );
        return Ok(stamp.genesis_hex.clone());
    }
    apply_genesis_stamp(&mut sled, &stamp)?;
    let record = sled_to_record(&sled)?;
    let genesis = sled_to_genesis(&sled);
    commit_cas(
        &engine,
        &store,
        operation_id,
        sled.session_id.clone(),
        expected,
        Some(record),
        Some(genesis),
        stamp.genesis_hex.clone(),
    )
    .await
}

pub(crate) async fn store_genesis(
    engine: &MutationEngine,
    stamp: &GenesisStamp,
) -> anyhow::Result<String> {
    store_genesis_with_operation(
        engine,
        engine.identity_sled_store()?,
        opid("genesis"),
        stamp,
    )
    .await
}

pub(crate) async fn advance_mutation_index_with_operation(
    engine: &MutationEngine,
    store: &IdentitySledStore,
    operation_id: String,
    session_id: &str,
    event_id: u64,
) -> anyhow::Result<()> {
    let engine = engine.clone();
    let store = store.clone();
    let session_id = session_id.to_string();
    tokio::spawn(async move {
        advance_mutation_index_owned(engine, store, operation_id, session_id, event_id).await
    })
    .await?
}

async fn advance_mutation_index_owned(
    engine: MutationEngine,
    store: IdentitySledStore,
    operation_id: String,
    session_id: String,
    event_id: u64,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        event_id <= i64::MAX as u64,
        "event index exceeds durable range"
    );
    let lifecycle_lock = store.lifecycle_lock(&session_id).await;
    let _session_guard = lifecycle_lock.lock_owned().await;
    let cozo = store.cozo.clone();
    let sid = session_id;
    let publication = engine
        .commit_identity_intent(operation_id.clone(), move |_| {
            let committed = cozo.apply_identity_intent(
                &sid,
                &operation_id,
                IdentityIntent::AdvanceMutationIndex { event_id },
            )?;
            let snapshot = cozo.read_identity_recovery_state(MAX_EVENTS_IN_STATE)?;
            Ok((
                Some(state_value(&snapshot)?),
                DurableResult {
                    value: (),
                    replayed: committed.replayed,
                },
            ))
        })
        .await?;
    publication_value(publication)
}

pub(crate) async fn advance_mutation_index(
    engine: &MutationEngine,
    session_id: &str,
    event_id: u64,
) -> anyhow::Result<()> {
    advance_mutation_index_with_operation(
        engine,
        engine.identity_sled_store()?,
        opid("event-index"),
        session_id,
        event_id,
    )
    .await
}

/// Assign the authorization-facing actor class exactly once. This value is
/// durable identity metadata and is deliberately separate from SID1's fixed
/// `wireguard-principal` envelope field.
pub(crate) async fn set_principal_kind(
    engine: &MutationEngine,
    store: &IdentitySledStore,
    operation_id: String,
    session_id: &str,
    principal_kind: &str,
) -> anyhow::Result<ContainerIdentitySled> {
    let engine = engine.clone();
    let store = store.clone();
    let session_id = session_id.to_string();
    let principal_kind = principal_kind.to_string();
    tokio::spawn(async move {
        set_principal_kind_owned(engine, store, operation_id, session_id, principal_kind).await
    })
    .await?
}

async fn set_principal_kind_owned(
    engine: MutationEngine,
    store: IdentitySledStore,
    operation_id: String,
    session_id: String,
    principal_kind: String,
) -> anyhow::Result<ContainerIdentitySled> {
    anyhow::ensure!(
        matches!(principal_kind.as_str(), "human" | "service"),
        "identity principal kind must be human or service"
    );
    let lifecycle_lock = store.lifecycle_lock(&session_id).await;
    let _session_guard = lifecycle_lock.lock_owned().await;
    let expected = load_revision(&store, &session_id).await?;
    let mut sled =
        revision_sled(&expected)?.ok_or_else(|| anyhow::anyhow!("identity session not found"))?;
    if let Some(existing) = sled.principal_kind.as_deref() {
        anyhow::ensure!(
            existing == principal_kind.as_str(),
            "identity principal kind is immutable"
        );
        return Ok(sled);
    }
    sled.principal_kind = Some(principal_kind);
    commit_sled_update(&engine, &store, operation_id, expected, sled.clone(), sled).await
}

pub(crate) async fn set_session_actor_class(
    engine: &MutationEngine,
    session_id: &str,
    principal_kind: &str,
) -> anyhow::Result<ContainerIdentitySled> {
    let store = engine.identity_sled_store()?;
    set_principal_kind(
        engine,
        store,
        opid("actor-class"),
        session_id,
        principal_kind,
    )
    .await
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}
fn opid(prefix: &str) -> String {
    format!("{prefix}-{}", uuid::Uuid::new_v4().simple())
}
fn arg_str(args: &JsonValue, key: &str) -> String {
    args.get(key)
        .and_then(JsonValue::as_str)
        .unwrap_or_default()
        .to_string()
}
fn derive_session_id(args: &JsonValue, public_key: &str) -> anyhow::Result<String> {
    anyhow::ensure!(
        args.get("psk").is_none(),
        "PSK identity derivation is retired"
    );
    Ok(op_identity::session::derive_session_id(public_key))
}

fn host_session_id() -> Option<String> {
    std::env::var("IDENTITY_SLED_HOST_SESSION_ID")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
fn prepare_parked_instance(
    session_id: &str,
    mut instance: IncusInstance,
) -> anyhow::Result<IncusInstance> {
    instance.name = session_id.to_string();
    instance.instance_type = "container".into();
    instance.status = "Stopped".into();
    if instance.profiles.is_empty() {
        instance.profiles = vec!["identity".into()];
    }
    instance
        .config
        .get_or_insert_with(Default::default)
        .insert("boot.autostart".into(), "false".into());
    anyhow::ensure!(
        !instance
            .devices
            .iter()
            .chain(instance.expanded_devices.iter())
            .any(|device| matches!(device.device, Device::Nic(_))),
        "identity containers cannot have NIC devices; identity is transported by WARP"
    );
    Ok(instance)
}

async fn commit_sled_update<T: Send + 'static>(
    engine: &MutationEngine,
    store: &IdentitySledStore,
    operation_id: String,
    expected: IdentityRevision,
    sled: ContainerIdentitySled,
    value: T,
) -> anyhow::Result<T> {
    let genesis = sled.is_anchored().then(|| sled_to_genesis(&sled));
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

/// Storage is prepared and mounted by its host owner. Guest power transitions
/// only verify the dedicated filesystem; they never change its device set.
async fn verify_identity_btrfs(
    sled: &mut ContainerIdentitySled,
    verifier: &BtrfsMountVerifier,
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
    let verifier = verifier.clone();
    tokio::task::spawn_blocking(move || verifier(&path, &mount, &uuid)).await??;
    if let Some(device) = sled.btrfs_device.as_mut() {
        device.attached = true;
    }
    Ok(())
}

async fn stop_native_identity(
    lifecycle: &dyn IdentityContainerLifecycle,
    session_id: &str,
    sled: &ContainerIdentitySled,
    verifier: &BtrfsMountVerifier,
) -> anyhow::Result<()> {
    if let Err(error) = lifecycle.stop(session_id).await {
        let detail = format!("{error:#}");
        if !detail.contains("already stopped") && !detail.contains("in Error status") {
            return Err(error);
        }
        tracing::warn!(
            session_id,
            %error,
            "identity container is already stopped; verifying its retained filesystem"
        );
    }
    verify_identity_btrfs(&mut sled.clone(), verifier)
        .await
        .context(
            "native identity stopped but dedicated filesystem readback requires reconciliation",
        )
}

async fn set_session_active_owned(
    engine: MutationEngine,
    store: IdentitySledStore,
    lifecycle: Arc<dyn IdentityContainerLifecycle>,
    session_id: String,
    active: bool,
) -> anyhow::Result<ContainerIdentitySled> {
    set_session_active_owned_with_verifier(
        engine,
        store,
        lifecycle,
        session_id,
        active,
        native_btrfs_mount_verifier(),
    )
    .await
}

async fn set_session_active_owned_with_verifier(
    engine: MutationEngine,
    store: IdentitySledStore,
    lifecycle: Arc<dyn IdentityContainerLifecycle>,
    session_id: String,
    active: bool,
    btrfs_mount_verifier: BtrfsMountVerifier,
) -> anyhow::Result<ContainerIdentitySled> {
    let lifecycle_lock = store.lifecycle_lock(&session_id).await;
    let _lifecycle_guard = lifecycle_lock.lock_owned().await;
    // Keep native power effects and their complete Incus readback ordered with
    // ordinary Incus mutations and the root repair owner.
    let _observation_guard = engine.lock_host_observation().await;
    let expected = load_revision(&store, &session_id).await?;
    let mut sled =
        revision_sled(&expected)?.ok_or_else(|| anyhow::anyhow!("identity session not found"))?;
    let has_instance = sled.instance.is_some();
    let mut seeded_root_guard = None;
    if active && has_instance {
        verify_identity_btrfs(&mut sled, &btrfs_mount_verifier).await?;
        let seeded = match physical_root::guard_start(&store, &expected, &sled).await? {
            Some(guard) => Some(IdentityRootStartGuard::from(guard)),
            None => match root_repair::guard_own_root_start(&store, &expected, &sled).await? {
                Some(guard) => Some(IdentityRootStartGuard::from(guard)),
                None => seed_provision::guard_start(&store, &expected, &sled)
                    .await?
                    .map(IdentityRootStartGuard::from),
            },
        };
        match seeded {
            Some(guard) => {
                anyhow::ensure!(
                    load_revision(&store, &session_id).await? == expected,
                    "identity revision changed before seeded native activation"
                );
                if guard.should_start {
                    lifecycle.start(&session_id).await?;
                }
                seeded_root_guard = Some(guard);
            }
            None if !sled.active => {
                preflight_native_identity_effect().await?;
                lifecycle.start(&session_id).await?;
            }
            None => {}
        }
    }
    if !active {
        if has_instance
            && (root_repair::repair_journal_incomplete(&store, &session_id).await?
                || seed_provision::journal_incomplete(&store, &session_id).await?)
        {
            // A pending repair fences Cozo mutations, but stopping the native
            // target remains available as the safe operator action.
            preflight_native_identity_effect().await?;
            stop_native_identity(
                lifecycle.as_ref(),
                &session_id,
                &sled,
                &btrfs_mount_verifier,
            )
            .await?;
            anyhow::bail!(
                "native identity stopped; incomplete root repair requires reconciliation"
            );
        }
        // Commit inactive power policy before stopping. This does not revoke SID1.
        sled.active = false;
        let pre_stop = commit_sled_update(
            &engine,
            &store,
            opid("park"),
            expected,
            sled.clone(),
            sled.clone(),
        )
        .await?;
        if has_instance {
            preflight_native_identity_effect().await?;
            stop_native_identity(
                lifecycle.as_ref(),
                &session_id,
                &sled,
                &btrfs_mount_verifier,
            )
            .await?;
            let expected = load_revision(&store, &session_id).await?;
            let mut stopped =
                revision_sled(&expected)?.ok_or_else(|| anyhow::anyhow!("identity disappeared"))?;
            let root_observed =
                match physical_root::observe_power(&engine, &store, &expected, &stopped, "Stopped")
                    .await?
                {
                    Some(observed) => Some(observed),
                    None => root_repair::observe_owned_power_effect(
                        &engine, &store, &expected, &stopped, "Stopped",
                    )
                    .await
                    .context(
                        "native stop completed but authoritative readback requires reconciliation",
                    )?,
                };
            let seeded_observed = if root_observed.is_some() {
                root_observed
            } else {
                seed_provision::observe_power(&engine, &store, &expected, &stopped, "Stopped")
                    .await
                    .context("seeded native stop readback requires reconciliation")?
            };
            if let Some(observed) = seeded_observed {
                stopped.instance = Some(observed);
            } else if let Some(instance) = stopped.instance.as_mut() {
                instance.status = "Stopped".into();
            }
            if let Some(device) = stopped.btrfs_device.as_mut() {
                // Guest stop leaves the host-owned dedicated root mounted.
                // The successful post-stop verifier read its actual backing.
                device.attached = true;
            }
            return commit_sled_update(
                &engine,
                &store,
                opid("parked"),
                expected,
                stopped.clone(),
                stopped,
            )
            .await;
        }
        return Ok(pre_stop);
    }
    sled.active = true;
    sled.last_seen_at = now();
    if seeded_root_guard.is_some() {
        let root_observed =
            match physical_root::observe_power(&engine, &store, &expected, &sled, "Running").await?
            {
                Some(observed) => Some(observed),
                None => root_repair::observe_owned_power_effect(
                    &engine, &store, &expected, &sled, "Running",
                )
                .await
                .context(
                    "native start completed but authoritative readback requires reconciliation",
                )?,
            };
        let observed = if let Some(observed) = root_observed {
            Some(observed)
        } else {
            seed_provision::observe_power(&engine, &store, &expected, &sled, "Running")
                .await
                .context("seeded native start readback requires reconciliation")?
        }
        .context("completed seeded root journal disappeared after start")?;
        sled.instance = Some(observed);
    } else if let Some(instance) = sled.instance.as_mut() {
        instance.status = "Running".into();
    }
    commit_sled_update(
        &engine,
        &store,
        opid("activate"),
        expected,
        sled.clone(),
        sled,
    )
    .await
}

/// Serialize canonical Incus effects on an identity target with its repair
/// owner. An incomplete journal fences every native effect after a crash;
/// ordinary exec and snapshot remain available after the completed root and
/// persist provenance is revalidated and held for the duration of the effect.
pub(crate) struct IdentityIncusTargetGuard {
    _lifecycle: tokio::sync::OwnedMutexGuard<()>,
    store: IdentitySledStore,
    session_id: String,
}

pub(crate) async fn guard_identity_incus_target(
    engine: &MutationEngine,
    session_id: &str,
) -> anyhow::Result<IdentityIncusTargetGuard> {
    let store = engine.identity_sled_store()?.clone();
    let lifecycle = store.lifecycle_lock(session_id).await.lock_owned().await;
    Ok(IdentityIncusTargetGuard {
        _lifecycle: lifecycle,
        store,
        session_id: session_id.to_string(),
    })
}

impl IdentityIncusTargetGuard {
    /// Call only after the host observation lock is held. The returned mount
    /// guard must be kept through the native effect and its SHM readback.
    pub(crate) async fn verify_under_host_lock(
        &self,
        method: &str,
    ) -> anyhow::Result<Option<IdentityRootStartGuard>> {
        let revision = load_revision(&self.store, &self.session_id).await?;
        if let Some(sled) = revision_sled(&revision)?
            && let Some(guard) = physical_root::guard_start(&self.store, &revision, &sled).await?
        {
            anyhow::ensure!(
                matches!(method, "exec_instance" | "snapshot_instance"),
                "physical identity provenance and lifecycle are identity-owned"
            );
            return Ok(Some(guard.into()));
        }
        let cozo = self.store.cozo.clone();
        let session = self.session_id.clone();
        let journal =
            tokio::task::spawn_blocking(move || cozo.read_root_repair_journal(&session)).await??;
        let cozo = self.store.cozo.clone();
        let session = self.session_id.clone();
        let provision =
            tokio::task::spawn_blocking(move || cozo.read_seed_provision_journal(&session))
                .await??;
        if journal.is_none()
            && let Some(provision) = provision
        {
            anyhow::ensure!(
                provision.phase == "identity-committed",
                "Incus target has incomplete seeded provision"
            );
            anyhow::ensure!(
                matches!(method, "exec_instance" | "snapshot_instance"),
                "seeded identity provenance and lifecycle are identity-owned"
            );
            let revision = load_revision(&self.store, &self.session_id).await?;
            let sled = revision_sled(&revision)?.context("seeded identity disappeared")?;
            return Ok(Some(
                seed_provision::guard_start(&self.store, &revision, &sled)
                    .await?
                    .context("completed seed provision provenance disappeared")?
                    .into(),
            ));
        }
        match journal {
            None => Ok(None),
            Some(journal)
                if journal.phase == "identity-committed"
                    && matches!(method, "exec_instance" | "snapshot_instance") =>
            {
                let revision = load_revision(&self.store, &self.session_id).await?;
                let sled = revision_sled(&revision)?
                    .ok_or_else(|| anyhow::anyhow!("repaired identity disappeared"))?;
                Ok(Some(
                    root_repair::guard_own_root_start(&self.store, &revision, &sled)
                        .await?
                        .ok_or_else(|| anyhow::anyhow!("completed repair provenance disappeared"))?
                        .into(),
                ))
            }
            Some(_) => {
                anyhow::bail!("Incus target is owned by identity root repair; use its owner route")
            }
        }
    }
}

/// Login starts the human container. Ordinary authenticated calls do not.
/// A service principal, including the control-plane chatbot, is left to the
/// startup path that keeps that container running.
pub(crate) async fn activate_human_session_on_login(
    engine: &MutationEngine,
    session_id: &str,
) -> anyhow::Result<()> {
    let Some(sled) = stored_session(engine, session_id).await? else {
        return Ok(());
    };
    if sled.principal_kind.as_deref() != Some("human") || sled.instance.is_none() || sled.active {
        return Ok(());
    }
    activate_session(engine, session_id).await?;
    tracing::info!(session_id, "started human identity container on login");
    Ok(())
}

/// How often to stop human containers that have no anchored connection.
/// The chatbot is not part of this pass.
pub(crate) const UNATTENDED_HUMAN_CONTAINER_SECS: i64 = 30 * 60;

/// Stop human containers whose interceptor session is gone.
/// The control-plane chatbot stays up. This stops the container only.
/// It does not remove the Cozo device or the shared base loop.
/// A stop that times out is left for the next pass.
pub(crate) async fn park_human_containers_without_login(
    engine: &MutationEngine,
) -> anyhow::Result<usize> {
    let connected = engine.connected_session_ids().await;
    let chatbot = control_plane_chatbot_session_id();
    let sessions: Vec<String> = read_private_state()
        .await?
        .sleds
        .into_iter()
        .filter(|sled| {
            sled.principal_kind.as_deref() == Some("human")
                && human_container_running(sled)
                && !connected.contains(&sled.session_id)
                && chatbot.as_deref() != Some(sled.session_id.as_str())
        })
        .map(|sled| sled.session_id)
        .collect();
    let mut stopped = 0usize;
    for session in &sessions {
        match tokio::time::timeout(
            std::time::Duration::from_secs(150),
            deactivate_session(engine, session),
        )
        .await
        {
            Ok(Ok(_)) => {
                engine.forget_session_context(session).await;
                stopped += 1;
                tracing::info!(
                    session,
                    "stopped human identity container with no logged-in principal"
                );
            }
            Ok(Err(error)) => {
                let detail = format!("{error:#}");
                if detail.contains("already stopped") || detail.contains("in Error status") {
                    engine.forget_session_context(session).await;
                    stopped += 1;
                } else {
                    tracing::warn!(
                        session,
                        %error,
                        "human identity container stop failed; the next pass will retry"
                    );
                }
            }
            Err(_) => {
                tracing::warn!(
                    session,
                    "human identity container stop timed out; the next pass will retry"
                );
            }
        }
    }
    Ok(stopped)
}

fn human_container_running(sled: &ContainerIdentitySled) -> bool {
    sled.instance.is_some()
        && (sled.active
            || sled
                .instance
                .as_ref()
                .is_some_and(|instance| instance.status.eq_ignore_ascii_case("Running")))
}

/// Session id of the control-plane chatbot, from
/// `OP_CONTROL_PLANE_CHATBOT_SESSION_ID`.
pub(crate) fn control_plane_chatbot_session_id() -> Option<String> {
    std::env::var("OP_CONTROL_PLANE_CHATBOT_SESSION_ID")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// Bring up the control-plane chatbot container named by
/// `OP_CONTROL_PLANE_CHATBOT_SESSION_ID`. Human containers start at login.
pub(crate) async fn start_service_identity_containers(
    engine: &MutationEngine,
) -> anyhow::Result<usize> {
    let Some(session_id) = control_plane_chatbot_session_id() else {
        return Ok(0);
    };
    let state = read_private_state().await?;
    let present = state
        .sleds
        .into_iter()
        .any(|sled| sled.session_id == session_id && sled.instance.is_some());
    if !present {
        tracing::warn!(
            %session_id,
            "control-plane chatbot session has no identity container"
        );
        return Ok(0);
    }
    activate_session(engine, &session_id).await?;
    Ok(1)
}

pub(crate) async fn activate_session(
    engine: &MutationEngine,
    session_id: &str,
) -> anyhow::Result<ContainerIdentitySled> {
    let store = engine.identity_sled_store()?.clone();
    let task = tokio::spawn(set_session_active_owned(
        engine.clone(),
        store,
        Arc::new(IncusIdentityContainerLifecycle),
        session_id.to_string(),
        true,
    ));
    task.await?
}

pub(crate) async fn deactivate_session(
    engine: &MutationEngine,
    session_id: &str,
) -> anyhow::Result<ContainerIdentitySled> {
    let store = engine.identity_sled_store()?.clone();
    let task = tokio::spawn(set_session_active_owned(
        engine.clone(),
        store,
        Arc::new(IncusIdentityContainerLifecycle),
        session_id.to_string(),
        false,
    ));
    task.await?
}

/// Revoke the durable identity before attempting the physical stop. A stop
/// failure never restores the old SID1-readable row. The tombstone remains a
/// valid expected revision for a later explicitly authorized reprovision.
#[allow(dead_code)]
pub(crate) async fn tombstone_session(
    engine: &MutationEngine,
    store: &IdentitySledStore,
    operation_id: String,
    session_id: &str,
) -> anyhow::Result<()> {
    let engine = engine.clone();
    let store = store.clone();
    let session_id = session_id.to_string();
    tokio::spawn(async move {
        let lifecycle_lock = store.lifecycle_lock(&session_id).await;
        let _lifecycle_guard = lifecycle_lock.lock_owned().await;
        let expected = load_revision(&store, &session_id).await?;
        let sled = revision_sled(&expected)?
            .ok_or_else(|| anyhow::anyhow!("identity session is already absent"))?;
        let had_instance = sled.instance.is_some();
        commit_cas(
            &engine,
            &store,
            operation_id,
            session_id.clone(),
            expected,
            None,
            None,
            (),
        )
        .await?;
        // The revoked sled's doorbell connection must not outlive the row.
        // Best effort: the next subscribe would be refused anyway.
        if let Some(owner_key) = crate::wake_doorbell::owner_key_for_sled(&sled) {
            let args = serde_json::json!({ "client_id": owner_key });
            if let Err(error) =
                op_plugins::state_plugins::emqx::dispatch_emqx_method("kick_client", &args).await
            {
                tracing::debug!(%error, "doorbell client not kicked");
            }
        }
        if had_instance {
            preflight_native_identity_effect().await?;
            stop_native_identity(
                &IncusIdentityContainerLifecycle,
                &session_id,
                &sled,
                &native_btrfs_mount_verifier(),
            )
            .await?;
        }
        Ok(())
    })
    .await?
}

async fn dispatch_identity_sled_method_owned(
    engine: &MutationEngine,
    store: &IdentitySledStore,
    operation_id: String,
    method: &str,
    args: &JsonValue,
) -> anyhow::Result<JsonValue> {
    dispatch_identity_sled_method_owned_with_fixtures(
        engine,
        store,
        operation_id,
        method,
        args,
        Arc::new(IncusIdentityContainerLifecycle),
        native_btrfs_mount_verifier(),
    )
    .await
}

async fn dispatch_identity_sled_method_owned_with_fixtures(
    engine: &MutationEngine,
    store: &IdentitySledStore,
    operation_id: String,
    method: &str,
    args: &JsonValue,
    lifecycle: Arc<dyn IdentityContainerLifecycle>,
    btrfs_mount_verifier: BtrfsMountVerifier,
) -> anyhow::Result<JsonValue> {
    match method {
        "inspect_identity_inventory" => {
            anyhow::bail!("identity inventory requires the verified maintenance route")
        }
        "repair_own_root" => {
            anyhow::bail!("root repair requires the verified own-session identity route")
        }
        "migrate_own_physical_root" => {
            anyhow::bail!("physical migration requires the verified human owner route")
        }
        "inspect_own_physical_root_migration_preflight" => {
            anyhow::bail!("physical migration preflight requires the verified human owner route")
        }
        "refresh_credential" => {
            anyhow::ensure!(
                args.as_object().is_some_and(|object| object.is_empty()),
                "refresh_credential accepts no arguments"
            );
            anyhow::bail!("refresh_credential requires verified session metadata")
        }
        "get_identity" => {
            let mut session_id = arg_str(args, "session_id");
            let trace_id = arg_str(args, "trace_id");
            if session_id.is_empty() && trace_id.is_empty() {
                session_id = host_session_id().ok_or_else(|| {
                    anyhow::anyhow!("host identity requires IDENTITY_SLED_HOST_SESSION_ID")
                })?;
            }
            let identity = read_private_state()
                .await?
                .sleds
                .into_iter()
                .find(|sled| {
                    (!session_id.is_empty() && sled.session_id == session_id)
                        || (!trace_id.is_empty() && sled.trace_id == trace_id)
                })
                .ok_or_else(|| anyhow::anyhow!("identity not found"))?;
            Ok(serde_json::json!({"identity": identity}))
        }
        "write_identity" => {
            anyhow::ensure!(
                args.get("sealed_id").is_none(),
                "sealed_id is identity-authored"
            );
            anyhow::ensure!(
                args.get("ttl_seconds").is_none(),
                "ttl_seconds is not part of the sealed write_identity contract"
            );
            let public_key = arg_str(args, "wireguard_pubkey");
            anyhow::ensure!(!public_key.is_empty(), "wireguard_pubkey is required");
            let session_id = derive_session_id(args, &public_key)?;
            let lifecycle_lock = store.lifecycle_lock(&session_id).await;
            let _session_guard = lifecycle_lock.lock_owned().await;
            let expected = load_revision(store, &session_id).await?;
            let mut sled = revision_sled(&expected)?
                .ok_or_else(|| anyhow::anyhow!("provision_container is required first"))?;
            anyhow::ensure!(
                sled.wireguard_pubkey == public_key,
                "identity key is immutable"
            );
            if let Some(interface) = args.get("interface").and_then(JsonValue::as_str) {
                sled.interface = interface.into();
            }
            if let Some(peer_ip) = args.get("peer_ip").and_then(JsonValue::as_str) {
                sled.peer_ip = Some(peer_ip.into());
            }
            if let Some(device) = args.get("btrfs_device") {
                sled.btrfs_device = Some(serde_json::from_value(device.clone())?);
            }
            sled.last_seen_at = now();
            sled.mutation_index = sled
                .mutation_index
                .checked_add(1)
                .ok_or_else(|| anyhow::anyhow!("mutation index overflow"))?;
            if !sled.is_anchored() {
                let stamp = engine
                    .mint_identity_genesis_stamp(
                        &sled.session_id,
                        &sled.wireguard_pubkey,
                        sled.expires_at.unwrap_or(0),
                    )
                    .await?;
                apply_genesis_stamp(&mut sled, &stamp)?;
            }
            let sled =
                commit_sled_update(engine, store, operation_id, expected, sled.clone(), sled)
                    .await?;
            Ok(serde_json::json!({"identity": sled}))
        }
        "provision_container" => {
            anyhow::bail!("provision_container requires the verified principal owner route")
        }
        "attach_btrfs_device" => {
            let session_id = arg_str(args, "session_id");
            let lifecycle_lock = store.lifecycle_lock(&session_id).await;
            let _session_guard = lifecycle_lock.lock_owned().await;
            let expected = load_revision(store, &session_id).await?;
            let mut sled =
                revision_sled(&expected)?.ok_or_else(|| anyhow::anyhow!("identity not found"))?;
            let device = sled
                .btrfs_device
                .clone()
                .ok_or_else(|| anyhow::anyhow!("no registered btrfs device"))?;
            anyhow::ensure!(!device.attached, "btrfs device is already attached");
            anyhow::ensure!(
                !device.device_path.is_empty() && !device.mount_point.is_empty(),
                "btrfs device path and mount are required"
            );
            let path = PathBuf::from(&device.device_path);
            let mount = PathBuf::from(&device.mount_point);
            let uuid = device.btrfs_uuid.clone();
            let btrfs_mount_verifier = btrfs_mount_verifier.clone();
            tokio::task::spawn_blocking(move || btrfs_mount_verifier(&path, &mount, &uuid))
                .await??;
            sled.btrfs_device.as_mut().expect("checked").attached = true;
            sled.last_seen_at = now();
            sled.mutation_index = sled
                .mutation_index
                .checked_add(1)
                .ok_or_else(|| anyhow::anyhow!("mutation index overflow"))?;
            let sled =
                commit_sled_update(engine, store, operation_id, expected, sled.clone(), sled)
                    .await?;
            Ok(serde_json::json!({"identity": sled}))
        }
        "touch_session" => {
            let session_id = arg_str(args, "session_id");
            let identity = set_session_active_owned_with_verifier(
                engine.clone(),
                store.clone(),
                lifecycle,
                session_id,
                true,
                btrfs_mount_verifier,
            )
            .await?;
            Ok(serde_json::json!({"identity": identity}))
        }
        "record_session_event" => {
            let session_id = arg_str(args, "session_id");
            let kind = arg_str(args, "kind");
            anyhow::ensure!(
                !session_id.is_empty() && !kind.is_empty(),
                "session_id and kind are required"
            );
            let lifecycle_lock = store.lifecycle_lock(&session_id).await;
            let _session_guard = lifecycle_lock.lock_owned().await;
            anyhow::ensure!(
                load_revision(store, &session_id).await?.record.is_some(),
                "session event requires an existing identity"
            );
            let subid = arg_str(args, "subid");
            let content = arg_str(args, "content");
            let cozo = store.cozo.clone();
            let callback_operation = operation_id.clone();
            let callback_session_id = session_id.clone();
            let publication = engine
                .commit_identity_intent(operation_id, move |_| {
                    let seq = cozo.append_session_event(
                        &callback_session_id,
                        &kind,
                        &subid,
                        &content,
                        now(),
                    )?;
                    let snapshot = cozo.read_identity_recovery_state(MAX_EVENTS_IN_STATE)?;
                    Ok((
                        Some(state_value(&snapshot)?),
                        DurableResult {
                            value: u64::try_from(seq)?,
                            replayed: false,
                        },
                    ))
                })
                .await?;
            let seq = publication_value(publication).map_err(|error| {
                anyhow::anyhow!(
                    "event {callback_operation} pending; do not retry blindly: {error:#}"
                )
            })?;
            Ok(serde_json::json!({"session_id": session_id, "seq": seq}))
        }
        "get_session_history" => {
            let session_id = arg_str(args, "session_id");
            anyhow::ensure!(!session_id.is_empty(), "session_id is required");
            let requested =
                usize::try_from(args.get("limit").and_then(JsonValue::as_u64).unwrap_or(100))?;
            let limit = if requested == 0 { 100 } else { requested };
            anyhow::ensure!(
                limit <= MAX_HISTORY_LIMIT,
                "history limit exceeds {MAX_HISTORY_LIMIT}"
            );
            let cozo = store.cozo.clone();
            let rows =
                tokio::task::spawn_blocking(move || cozo.list_session_events(&session_id, limit))
                    .await??;
            let events = rows
                .iter()
                .map(event_from_record)
                .collect::<anyhow::Result<Vec<_>>>()?;
            Ok(serde_json::json!({"events": events}))
        }
        other => anyhow::bail!("unknown identity_sled method '{other}'"),
    }
}

async fn refresh_credential_owned(
    engine: &MutationEngine,
    store: &IdentitySledStore,
    operation_id: String,
    actor_id: &str,
    session_id: &str,
    session_genesis: &str,
) -> anyhow::Result<JsonValue> {
    anyhow::ensure!(
        !actor_id.is_empty() && !session_id.is_empty() && !session_genesis.is_empty(),
        "credential refresh requires verified session metadata"
    );
    let lifecycle_lock = store.lifecycle_lock(session_id).await;
    let _session_guard = lifecycle_lock.lock_owned().await;
    crate::method_contract_admission::admit_compiled_method(
        "identity_sled",
        "refresh_credential",
        Some("identity_sled.credential.refresh"),
    )?;
    anyhow::ensure!(
        crate::interceptor::load_capability_grants(actor_id)
            .iter()
            .any(|capability| capability == "identity_sled.credential.refresh"),
        "credential refresh requires a current exact-principal grant"
    );
    let expected = load_revision(store, session_id).await?;
    let mut sled =
        revision_sled(&expected)?.ok_or_else(|| anyhow::anyhow!("identity session not found"))?;
    anyhow::ensure!(
        sled.is_anchored(),
        "credential refresh requires an anchored session"
    );
    anyhow::ensure!(
        sled.genesis.as_deref() == Some(session_genesis),
        "verified session genesis does not match identity"
    );
    anyhow::ensure!(
        op_identity::session::derive_session_id(&sled.wireguard_pubkey) == sled.session_id,
        "identity session does not match its key"
    );
    anyhow::ensure!(
        op_identity::session::derive_principal_id(&sled.wireguard_pubkey) == actor_id,
        "verified actor does not match identity"
    );
    anyhow::ensure!(
        sled.principal_kind.as_deref() == Some("human"),
        "credential refresh requires a human identity"
    );
    anyhow::ensure!(
        sled.instance
            .as_ref()
            .is_some_and(|instance| instance.name == sled.session_id
                && instance.instance_type == "container"),
        "identity has no matching provisioned container binding"
    );
    anyhow::ensure!(
        sled.schema_version == RECORD_FORMAT,
        "identity schema version is not current"
    );
    anyhow::ensure!(
        crate::human_principal_dispatch::resolve_key_for_assertion(&sled.wireguard_pubkey)
            .await
            .map_err(|_| anyhow::anyhow!("identity principal registry is unavailable"))?
            .is_some_and(|record| record.revoked_at == 0 && record.principal_id == actor_id),
        "identity principal is not currently registered"
    );
    anyhow::ensure!(
        sled.expires_at
            .is_none_or(|expiry| expiry == 0 || expiry > now()),
        "identity session is expired"
    );
    let old = sled
        .sealed_id
        .as_deref()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow::anyhow!("identity has no refreshable SID1 credential"))?;
    let old_claims = op_identity::sealed_id::SealedId::from_inline_ref(old)
        .map_err(|error| anyhow::anyhow!("stored SID1 is invalid: {error}"))?;
    anyhow::ensure!(
        old_claims.principal_id == actor_id
            && old_claims.principal_kind == "wireguard-principal"
            && old_claims.session_id == sled.session_id
            && old_claims.wireguard_pubkey == sled.wireguard_pubkey
            && old_claims.session_genesis == session_genesis
            && old_claims.trace_id == sled.trace_id
            && old_claims.schema_version == sled.schema_version
            && old_claims.arrival_timestamp == sled.arrival_timestamp
            && old_claims.issued_at == sled.arrival_timestamp
            && old_claims.expires_at == sled.expires_at.unwrap_or(0)
            && old_claims.chain_head_at_arrival == sled.chain_head_at_arrival
            && old_claims.catalog_hash_at_arrival == sled.catalog_hash_at_arrival
            && old_claims.head_timestamp_at_arrival == sled.head_timestamp_at_arrival,
        "stored SID1 claims do not match authoritative identity"
    );
    anyhow::ensure!(
        matches!(
            old_claims.transport_scope.as_str(),
            "dbus,grpc,mcp" | "dbus,grpc,mcp,mqtt"
        ),
        "stored SID1 transport scope is not refreshable"
    );
    // Recheck the seal and grant after asynchronous identity reads, before
    // publishing a replacement credential or returning an idempotent receipt.
    crate::method_contract_admission::admit_compiled_method(
        "identity_sled",
        "refresh_credential",
        Some("identity_sled.credential.refresh"),
    )?;
    anyhow::ensure!(
        crate::interceptor::load_capability_grants(actor_id)
            .iter()
            .any(|capability| capability == "identity_sled.credential.refresh"),
        "credential refresh requires a current exact-principal grant"
    );
    let sealed_id = engine.refresh_identity_sealed_id(&sled)?;
    if old == sealed_id {
        let published = stored_session(engine, session_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("identity credential publication is absent"))?;
        anyhow::ensure!(
            published.sealed_id == sled.sealed_id && published.genesis == sled.genesis,
            "identity credential publication requires recovery"
        );
        return Ok(
            serde_json::json!({"refreshed": false, "session_id": sled.session_id, "transport_scope": old_claims.transport_scope}),
        );
    }
    sled.sealed_id = Some(sealed_id);
    sled.mutation_index = sled
        .mutation_index
        .checked_add(1)
        .ok_or_else(|| anyhow::anyhow!("mutation index overflow"))?;
    let cozo = store.cozo.clone();
    let owned_session = session_id.to_string();
    let candidate = sled.sealed_id.clone().expect("credential authored above");
    let publication = engine
        .commit_identity_intent(operation_id.clone(), move |_| {
            let committed = cozo.apply_identity_intent(
                &owned_session,
                &operation_id,
                IdentityIntent::RefreshTransportCredential {
                    expected: Box::new(expected),
                    sealed_id: candidate,
                },
            )?;
            let snapshot = cozo.read_identity_recovery_state(MAX_EVENTS_IN_STATE)?;
            Ok((
                Some(state_value(&snapshot)?),
                DurableResult {
                    value: (),
                    replayed: committed.replayed,
                },
            ))
        })
        .await?;
    publication_value(publication)?;
    let published = stored_session(engine, session_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("identity credential publication is absent"))?;
    anyhow::ensure!(
        published.sealed_id == sled.sealed_id && published.genesis == sled.genesis,
        "identity credential publication requires recovery"
    );
    engine
        .register_session_context(crate::mutation_engine::SessionContext {
            genesis_hex: sled.genesis.clone().unwrap_or_default(),
            session_id: sled.session_id.clone(),
            wireguard_pubkey: sled.wireguard_pubkey.clone(),
            sealed_id_digest: crate::mutation_engine::sealed_id_digest(sled.sealed_id.as_deref()),
        })
        .await;
    Ok(
        serde_json::json!({"refreshed": true, "session_id": sled.session_id, "transport_scope": "dbus,grpc,mcp,mqtt"}),
    )
}

fn require_identity_inventory_capability(
    grants: &std::collections::HashSet<String>,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        grants.contains("identity_sled.inventory.read"),
        "identity inventory requires a current exact-principal inventory grant"
    );
    Ok(())
}

pub(crate) async fn dispatch_identity_sled_method_for_verified_session(
    engine: &MutationEngine,
    method: &str,
    args: &JsonValue,
    actor_id: &str,
    session_id: Option<&str>,
    session_genesis: Option<&str>,
) -> anyhow::Result<JsonValue> {
    if matches!(
        method,
        "migrate_own_physical_root" | "inspect_own_physical_root_migration_preflight"
    ) {
        let preflight = method == "inspect_own_physical_root_migration_preflight";
        let sid = session_id
            .filter(|value| !value.is_empty())
            .context("physical migration requires a verified session")?
            .to_string();
        let genesis = session_genesis
            .filter(|value| !value.is_empty())
            .context("physical migration requires a verified genesis")?
            .to_string();
        let actor = actor_id.to_string();
        let input = if preflight {
            anyhow::ensure!(
                args.as_object().is_some_and(|args| args.is_empty()),
                "physical migration preflight accepts no arguments"
            );
            None
        } else {
            Some(serde_json::from_value::<physical_root::MigrationInput>(
                args.clone(),
            )?)
        };
        let engine = engine.clone();
        let store = engine.identity_sled_store()?.clone();
        return tokio::spawn(async move {
            let caller = VerifiedIdentityActor {
                actor_id: &actor,
                session_id: &sid,
                genesis: &genesis,
            };
            match input {
                Some(input) => physical_root::migrate(&engine, &store, caller, input).await,
                None => physical_root::preflight(&engine, &store, caller).await,
            }
        })
        .await?;
    }
    if method == "inspect_identity_inventory" {
        anyhow::ensure!(
            session_id.is_some_and(|value| !value.is_empty())
                && session_genesis.is_some_and(|value| !value.is_empty()),
            "identity inventory requires verified anchored session metadata"
        );
        crate::method_contract_admission::admit_compiled_method(
            "identity_sled",
            "inspect_identity_inventory",
            Some("identity_sled.inventory.read"),
        )?;
        require_identity_inventory_capability(&crate::interceptor::load_capability_grants(
            actor_id,
        ))?;
        let input: op_cozo_store::IdentityInventoryQuery = serde_json::from_value(args.clone())?;
        let contract = strict_identity_contract()?;
        let cozo = engine.identity_sled_store()?.cozo.clone();
        let page =
            tokio::task::spawn_blocking(move || cozo.inspect_identity_inventory(input)).await??;
        same_identity_contract(&contract)?;
        crate::method_contract_admission::admit_compiled_method(
            "identity_sled",
            "inspect_identity_inventory",
            Some("identity_sled.inventory.read"),
        )?;
        require_identity_inventory_capability(&crate::interceptor::load_capability_grants(
            actor_id,
        ))?;
        return Ok(serde_json::to_value(page)?);
    }
    if method == "inspect_own_root_repair_preflight" {
        anyhow::ensure!(
            args.as_object().is_some_and(|object| object.is_empty()),
            "own root repair preflight accepts no arguments"
        );
        let own_session = session_id
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                anyhow::anyhow!("root repair preflight requires a verified SID1 session")
            })?;
        let own_genesis = session_genesis
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                anyhow::anyhow!("root repair preflight requires verified session genesis")
            })?;
        return root_repair::inspect_own_root_repair_preflight(
            engine,
            engine.identity_sled_store()?,
            actor_id,
            own_session,
            own_genesis,
        )
        .await;
    }
    if method == "provision_container" {
        let caller_session = session_id
            .filter(|value| !value.is_empty())
            .ok_or_else(|| anyhow::anyhow!("seed provision requires a verified actor session"))?;
        let caller_genesis = session_genesis
            .filter(|value| !value.is_empty())
            .ok_or_else(|| anyhow::anyhow!("seed provision requires verified actor genesis"))?;
        let input: seed_provision::ProvisionInput = serde_json::from_value(args.clone())?;
        let seal = seed_provision::capture_provision_seal()?;
        let owned_engine = engine.clone();
        let owned_store = engine.identity_sled_store()?.clone();
        let actor_id = actor_id.to_string();
        let caller_session = caller_session.to_string();
        let caller_genesis = caller_genesis.to_string();
        let operation_id = opid("seed-provision");
        let owned_operation_id = operation_id.clone();
        return tokio::spawn(async move {
            seed_provision::provision_container(
                &owned_engine,
                &owned_store,
                VerifiedIdentityActor {
                    actor_id: &actor_id,
                    session_id: &caller_session,
                    genesis: &caller_genesis,
                },
                input,
                seal,
                &owned_operation_id,
            )
            .await
        })
        .await
        .map_err(|error| {
            anyhow::anyhow!(
                "seed provision {} owner failed; reconciliation may be required: {}",
                operation_id,
                error
            )
        })?;
    }
    if method == "repair_own_root" {
        let session_id = session_id
            .filter(|value| !value.is_empty())
            .ok_or_else(|| anyhow::anyhow!("root repair requires a verified SID1 session"))?;
        let session_genesis = session_genesis
            .filter(|value| !value.is_empty())
            .ok_or_else(|| anyhow::anyhow!("root repair requires a verified session genesis"))?;
        let input: root_repair::RepairOwnRootInput = serde_json::from_value(args.clone())?;
        let original_seal = root_repair::capture_repair_seal()?;
        let owned_engine = engine.clone();
        let owned_store = engine.identity_sled_store()?.clone();
        let actor_id = actor_id.to_string();
        let session_id = session_id.to_string();
        let session_genesis = session_genesis.to_string();
        let operation_id = opid("root-repair");
        let owned_operation_id = operation_id.clone();
        return tokio::spawn(async move {
            root_repair::repair_own_root(
                &owned_engine,
                &owned_store,
                VerifiedIdentityActor {
                    actor_id: &actor_id,
                    session_id: &session_id,
                    genesis: &session_genesis,
                },
                input,
                original_seal,
                owned_operation_id,
            )
            .await
        })
        .await
        .map_err(|error| root_repair::owned_task_join_error(&operation_id, error))?;
    }
    if method == "refresh_credential" {
        anyhow::ensure!(
            args.as_object().is_some_and(|object| object.is_empty()),
            "refresh_credential accepts no arguments"
        );
        anyhow::ensure!(
            session_id.is_some_and(|value| !value.is_empty())
                && session_genesis.is_some_and(|value| !value.is_empty()),
            "credential refresh requires verified session metadata"
        );
        let trusted = engine
            .session_context_for_actor(actor_id)
            .await
            .ok_or_else(|| {
                anyhow::anyhow!("credential refresh requires a verified anchored session")
            })?;
        anyhow::ensure!(
            session_id == Some(trusted.session_id.as_str()),
            "requested session does not match verified actor"
        );
        anyhow::ensure!(
            session_genesis == Some(trusted.genesis_hex.as_str()),
            "requested genesis does not match verified session"
        );
        let owned_engine = engine.clone();
        let owned_store = engine.identity_sled_store()?.clone();
        let actor_id = actor_id.to_string();
        let session_id = trusted.session_id;
        let session_genesis = trusted.genesis_hex;
        return tokio::spawn(async move {
            refresh_credential_owned(
                &owned_engine,
                &owned_store,
                opid("credential-refresh"),
                &actor_id,
                &session_id,
                &session_genesis,
            )
            .await
        })
        .await?;
    }
    dispatch_identity_sled_method(engine, method, args).await
}

pub(crate) async fn dispatch_identity_sled_method_with_operation(
    engine: &MutationEngine,
    store: &IdentitySledStore,
    operation_id: String,
    method: &str,
    args: &JsonValue,
) -> anyhow::Result<JsonValue> {
    let engine = engine.clone();
    let store = store.clone();
    let method = method.to_string();
    let args = args.clone();
    tokio::spawn(async move {
        dispatch_identity_sled_method_owned(&engine, &store, operation_id, &method, &args).await
    })
    .await?
}

pub async fn dispatch_identity_sled_method(
    engine: &MutationEngine,
    method: &str,
    args: &JsonValue,
) -> anyhow::Result<JsonValue> {
    dispatch_identity_sled_method_with_operation(
        engine,
        engine.identity_sled_store()?,
        opid(method),
        method,
        args,
    )
    .await
}

pub async fn current_state(_engine: &MutationEngine) -> anyhow::Result<IdentitySledState> {
    read_private_state().await
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::human_principal_dispatch::tests::{TempShm, pk, temp_shm, test_engine};
    use std::sync::{Arc, Mutex};

    #[test]
    fn pending_publication_does_not_disclose_native_credential_diagnostics() {
        for durable in [
            None,
            Some(DurableResult {
                value: (),
                replayed: false,
            }),
        ] {
            let failure = publication_value(CredentialPublication::Pending {
                operation_id: "safe-operation-id".into(),
                durable,
                error: anyhow::anyhow!("native bound value SID1_PRIVATE_SENTINEL")
                    .context("private document query failed"),
            })
            .unwrap_err();
            for rendered in [
                failure.to_string(),
                format!("{failure:?}"),
                format!("{failure:#}"),
            ] {
                assert!(rendered.contains("safe-operation-id"));
                assert!(rendered.contains("pending recovery"));
                assert!(!rendered.contains("PRIVATE_SENTINEL"));
                assert!(!rendered.contains("private document"));
            }
        }
    }

    #[derive(Default)]
    struct RecordingLifecycle {
        calls: Mutex<Vec<String>>,
        fail_stop: bool,
    }

    #[derive(Default)]
    struct BlockingLifecycle {
        calls: Mutex<Vec<String>>,
        start_entered: tokio::sync::Notify,
        release_start: tokio::sync::Notify,
    }

    #[async_trait]
    impl IdentityContainerLifecycle for BlockingLifecycle {
        async fn start(&self, session_id: &str) -> anyhow::Result<()> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("start:{session_id}"));
            self.start_entered.notify_one();
            self.release_start.notified().await;
            Ok(())
        }
        async fn stop(&self, session_id: &str) -> anyhow::Result<()> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("stop:{session_id}"));
            Ok(())
        }
    }

    #[async_trait]
    impl IdentityContainerLifecycle for RecordingLifecycle {
        async fn start(&self, session_id: &str) -> anyhow::Result<()> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("start:{session_id}"));
            Ok(())
        }
        async fn stop(&self, session_id: &str) -> anyhow::Result<()> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("stop:{session_id}"));
            if self.fail_stop {
                anyhow::bail!("injected Incus stop failure");
            }
            Ok(())
        }
    }

    pub(crate) fn sled_engine() -> (Arc<MutationEngine>, TempShm) {
        let shm = temp_shm();
        let engine = test_engine();
        engine
            .install_identity_sled_store_for_test(IdentitySledStore::new(
                CozoGraphShuttle::new_in_memory().expect("identity Cozo"),
            ))
            .expect("install identity store");
        (engine, shm)
    }

    pub(crate) async fn write_identity(
        engine: &MutationEngine,
        pubkey: &str,
    ) -> anyhow::Result<ContainerIdentitySled> {
        write_identity_term_with_scope(engine, pubkey, 0, None).await
    }

    /// Exercise the real durable parking path with an isolated native lifecycle.
    pub(crate) async fn park_identity(
        engine: &MutationEngine,
        session_id: &str,
    ) -> anyhow::Result<ContainerIdentitySled> {
        let lifecycle = Arc::new(RecordingLifecycle::default());
        let parked = set_session_active_owned(
            engine.clone(),
            engine.identity_sled_store()?.clone(),
            lifecycle.clone(),
            session_id.to_string(),
            false,
        )
        .await?;
        assert_eq!(
            *lifecycle.calls.lock().unwrap(),
            vec![format!("stop:{session_id}")]
        );
        Ok(parked)
    }

    async fn dispatch_with_fixtures(
        engine: &Arc<MutationEngine>,
        lifecycle: Arc<dyn IdentityContainerLifecycle>,
        btrfs_mount_verifier: BtrfsMountVerifier,
        method: &str,
        args: JsonValue,
    ) -> anyhow::Result<JsonValue> {
        let store = engine.identity_sled_store()?.clone();
        dispatch_identity_sled_method_owned_with_fixtures(
            engine,
            &store,
            opid(method),
            method,
            &args,
            lifecycle,
            btrfs_mount_verifier,
        )
        .await
    }

    fn no_btrfs_mount_verifier() -> BtrfsMountVerifier {
        Arc::new(|_device, _mount, _uuid| Ok(()))
    }

    async fn assert_sibling_unchanged(engine: &MutationEngine, expected: &ContainerIdentitySled) {
        assert!(
            expected.is_anchored(),
            "sibling must come from the identity owner"
        );
        let store = engine
            .identity_sled_store()
            .expect("identity owner configured");
        let durable = revision_sled(
            &load_revision(store, &expected.session_id)
                .await
                .expect("read durable sibling"),
        )
        .expect("parse durable sibling")
        .expect("durable sibling exists");
        assert_eq!(
            serde_json::to_value(&durable).unwrap(),
            serde_json::to_value(expected).unwrap(),
            "unrelated durable sibling changed"
        );
        let private = strict_private_state().expect("read committed private SHM");
        let projected = private
            .sleds
            .iter()
            .find(|sled| sled.session_id == expected.session_id)
            .expect("committed private-SHM sibling exists");
        assert_eq!(
            serde_json::to_value(projected).unwrap(),
            serde_json::to_value(expected).unwrap(),
            "unrelated private-SHM sibling changed"
        );
    }

    /// A second anchored session for an already registered key, with its own
    /// session id and genesis: the provenance a later login leaves for owners
    /// scoped by principal alone. Fixture only; the production genesis path
    /// derives the session id from the key.
    pub(crate) async fn write_second_session(
        engine: &MutationEngine,
        pubkey: &str,
        session_id: &str,
    ) -> anyhow::Result<ContainerIdentitySled> {
        use sha2::Digest as _;
        let store = engine.identity_sled_store()?;
        let mut sled = write_identity(engine, pubkey).await?;
        anyhow::ensure!(sled.session_id != session_id, "fixture needs a new session");
        let expected = load_revision(store, session_id).await?;
        anyhow::ensure!(
            revision_sled(&expected)?.is_none(),
            "fixture session already exists"
        );
        sled.session_id = session_id.into();
        sled.instance = Some(prepare_parked_instance(
            session_id,
            IncusInstance::default(),
        )?);
        sled.genesis = Some(hex::encode(sha2::Sha256::digest(
            format!("{}:{session_id}", sled.genesis.clone().unwrap_or_default()).as_bytes(),
        )));
        commit_sled_update(engine, store, opid("fixture"), expected, sled.clone(), sled).await
    }

    async fn write_identity_term(
        engine: &MutationEngine,
        pubkey: &str,
        expires_at: i64,
    ) -> anyhow::Result<ContainerIdentitySled> {
        write_identity_term_with_scope(engine, pubkey, expires_at, None).await
    }

    async fn write_identity_term_with_scope(
        engine: &MutationEngine,
        pubkey: &str,
        expires_at: i64,
        transport_scope: Option<&str>,
    ) -> anyhow::Result<ContainerIdentitySled> {
        let store = engine.identity_sled_store()?;
        let session_id = op_identity::session::derive_session_id(pubkey);
        let expected = load_revision(store, &session_id).await?;
        if let Some(existing) = revision_sled(&expected)? {
            return Ok(existing);
        }
        let timestamp = now();
        let mut sled = ContainerIdentitySled {
            session_id: session_id.clone(),
            wireguard_pubkey: pubkey.into(),
            interface: String::new(),
            peer_ip: None,
            mutation_index: 0,
            genesis: None,
            trace_id: String::new(),
            schema_version: RECORD_FORMAT,
            vector_id: String::new(),
            sealed_id: None,
            principal_kind: Some("human".into()),
            btrfs_device: None,
            instance: Some(prepare_parked_instance(
                &session_id,
                IncusInstance::default(),
            )?),
            session_started_at: timestamp,
            last_seen_at: timestamp,
            active: false,
            expires_at: (expires_at != 0).then_some(expires_at),
            arrival_timestamp: 0,
            chain_head_at_arrival: String::new(),
            catalog_hash_at_arrival: String::new(),
            head_timestamp_at_arrival: 0,
        };
        let mut stamp = engine
            .mint_identity_genesis_stamp(&session_id, pubkey, expires_at)
            .await?;
        if let Some(scope) = transport_scope {
            let claims = op_identity::sealed_id::SealedId::from_inline_ref(&stamp.sealed_id)?;
            stamp.sealed_id = op_identity::sealed_id::SealedId {
                transport_scope: scope.into(),
                ..claims
            }
            .to_inline_ref()?;
        }
        apply_genesis_stamp(&mut sled, &stamp)?;
        commit_sled_update(engine, store, opid("fixture"), expected, sled.clone(), sled).await
    }

    #[test]
    fn provision_policy_forces_parked_non_autostart_container() {
        let session_id = op_identity::session::derive_session_id(&pk(0x60));
        let instance = IncusInstance {
            name: "caller-selected".into(),
            status: "Running".into(),
            instance_type: "virtual-machine".into(),
            config: Some(std::collections::HashMap::from([(
                "boot.autostart".into(),
                "true".into(),
            )])),
            ..Default::default()
        };
        let parked = prepare_parked_instance(&session_id, instance).unwrap();
        assert_eq!(parked.name, session_id);
        assert_eq!(parked.status, "Stopped");
        assert_eq!(parked.instance_type, "container");
        assert_eq!(parked.profiles, ["identity"]);
        assert_eq!(parked.config.unwrap()["boot.autostart"], "false");
    }

    #[test]
    fn malformed_durable_json_fails_closed() {
        let mut record = test_record("sid");
        record.btrfs_device_json = "{".into();
        assert!(record_to_sled(&record, None).is_err());
    }

    fn test_record(session_id: &str) -> IdentitySledRecord {
        IdentitySledRecord {
            session_id: session_id.into(),
            wireguard_pubkey: format!("key-{session_id}"),
            interface: String::new(),
            peer_ip: String::new(),
            mutation_index: 0,
            session_genesis: String::new(),
            trace_id: String::new(),
            schema_version: 3,
            vector_id: String::new(),
            sealed_id: String::new(),
            principal_kind: None,
            btrfs_device_json: String::new(),
            instance_json: String::new(),
            session_started_at: 0,
            last_seen_at: 0,
            active: false,
            expires_at: 0,
        }
    }

    #[test]
    fn recovery_preserves_siblings_tombstones_and_event_order() {
        let snapshot = IdentityRecoverySnapshot {
            identities: vec![
                IdentityRevision {
                    session_id: "deleted".into(),
                    revision: 3,
                    operation_id: "d3".into(),
                    record: None,
                    genesis: None,
                },
                IdentityRevision {
                    session_id: "b".into(),
                    revision: 1,
                    operation_id: "b1".into(),
                    record: Some(test_record("b")),
                    genesis: None,
                },
                IdentityRevision {
                    session_id: "a".into(),
                    revision: 2,
                    operation_id: "a2".into(),
                    record: Some(test_record("a")),
                    genesis: None,
                },
            ],
            events: vec![
                SessionEventRecord {
                    session_id: "a".into(),
                    seq: 4,
                    kind: "new".into(),
                    subid: String::new(),
                    content: String::new(),
                    created_at: 20,
                },
                SessionEventRecord {
                    session_id: "deleted".into(),
                    seq: 1,
                    kind: "old".into(),
                    subid: String::new(),
                    content: String::new(),
                    created_at: 10,
                },
            ],
        };
        let state = recovery_to_state(&snapshot).unwrap();
        assert_eq!(
            state
                .sleds
                .iter()
                .map(|s| s.session_id.as_str())
                .collect::<Vec<_>>(),
            vec!["a", "b"]
        );
        assert_eq!(
            state
                .events
                .iter()
                .map(|e| (e.created_at, e.session_id.as_str(), e.seq))
                .collect::<Vec<_>>(),
            vec![(20, "a", 4), (10, "deleted", 1)]
        );
    }

    fn inline_sid1_wire(sealed_id: &str) -> Vec<u8> {
        base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(
                sealed_id
                    .strip_prefix(op_identity::sealed_id::INLINE_PREFIX)
                    .unwrap(),
            )
            .unwrap()
    }

    async fn refresh_test_fixture(
        byte: u8,
    ) -> (
        Arc<MutationEngine>,
        TempShm,
        ContainerIdentitySled,
        tempfile::TempDir,
        Option<std::ffi::OsString>,
        crate::human_principal_dispatch::tests::TempCozo,
    ) {
        refresh_test_fixture_with_claims(byte, None, 0).await
    }

    async fn refresh_test_fixture_with_claims(
        byte: u8,
        transport_scope: Option<&str>,
        expires_at: i64,
    ) -> (
        Arc<MutationEngine>,
        TempShm,
        ContainerIdentitySled,
        tempfile::TempDir,
        Option<std::ffi::OsString>,
        crate::human_principal_dispatch::tests::TempCozo,
    ) {
        let registry = crate::human_principal_dispatch::tests::temp_cozo();
        let key = pk(byte);
        let (engine, shm) = sled_engine();
        crate::human_principal_dispatch::dispatch_human_principal_method(
            "register_key",
            &serde_json::json!({"human_pubkey": key, "display_alias": format!("refresh-{byte}")}),
        )
        .await
        .unwrap();
        let identity = write_identity_term_with_scope(&engine, &key, expires_at, transport_scope)
            .await
            .unwrap();
        set_session_actor_class(&engine, &identity.session_id, "human")
            .await
            .unwrap();
        let identity = stored_session(&engine, &identity.session_id)
            .await
            .unwrap()
            .unwrap();
        let principal = op_identity::session::derive_principal_id(&key);
        let grants_dir = tempfile::tempdir().unwrap();
        let grants_path = grants_dir.path().join("capability-grants.json");
        std::fs::write(
            &grants_path,
            serde_json::to_vec(&serde_json::json!({principal: {"capabilities": ["identity_sled.credential.refresh"]}})).unwrap(),
        )
        .unwrap();
        let previous = std::env::var_os("OP_GRANTS_PATH");
        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::set_var("OP_GRANTS_PATH", &grants_path) };
        (engine, shm, identity, grants_dir, previous, registry)
    }

    fn restore_grants_path(previous: Option<std::ffi::OsString>) {
        match previous {
            // FIXME: Audit that the environment access only happens in single-threaded code.
            Some(value) => unsafe { std::env::set_var("OP_GRANTS_PATH", value) },
            // FIXME: Audit that the environment access only happens in single-threaded code.
            None => unsafe { std::env::remove_var("OP_GRANTS_PATH") },
        }
    }

    fn refresh_test_lock() -> &'static tokio::sync::Mutex<()> {
        static LOCK: std::sync::OnceLock<tokio::sync::Mutex<()>> = std::sync::OnceLock::new();
        LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn refresh_migrates_legacy_sid1_and_replaces_transport_auth() {
        let _refresh_lock = refresh_test_lock().lock().await;
        let (engine, _shm, identity, _grants_dir, previous, _registry) =
            refresh_test_fixture_with_claims(0x81, Some("dbus,grpc,mcp"), 0).await;
        let key = identity.wireguard_pubkey.clone();
        let principal = op_identity::session::derive_principal_id(&key);
        let old = op_identity::sealed_id::SealedId::open(&inline_sid1_wire(
            identity.sealed_id.as_deref().unwrap(),
        ))
        .unwrap();
        let store = engine.identity_sled_store().unwrap().clone();
        let legacy = op_identity::sealed_id::SealedId {
            transport_scope: "dbus,grpc,mcp".into(),
            ..old.clone()
        }
        .to_inline_ref()
        .unwrap();
        let refreshed = dispatch_identity_sled_method_for_verified_session(
            &engine,
            "refresh_credential",
            &serde_json::json!({}),
            &principal,
            Some(&identity.session_id),
            identity.genesis.as_deref(),
        )
        .await
        .unwrap_or_else(|error| {
            if let Some(pending) = error.downcast_ref::<IdentityPublicationPending>() {
                panic!("refresh publication pending: {}", pending._private_cause);
            }
            panic!("refresh failed: {error:#}");
        });
        assert_eq!(refreshed["refreshed"], true);
        let current = revision_sled(&load_revision(&store, &identity.session_id).await.unwrap())
            .unwrap()
            .unwrap();
        let new_claims = op_identity::sealed_id::SealedId::open(&inline_sid1_wire(
            current.sealed_id.as_deref().unwrap(),
        ))
        .unwrap();
        assert_eq!(new_claims.principal_id, old.principal_id);
        assert_eq!(new_claims.session_id, old.session_id);
        assert_eq!(new_claims.wireguard_pubkey, old.wireguard_pubkey);
        assert_eq!(new_claims.session_genesis, old.session_genesis);
        assert_eq!(new_claims.trace_id, old.trace_id);
        assert_eq!(new_claims.schema_version, old.schema_version);
        assert_eq!(new_claims.arrival_timestamp, old.arrival_timestamp);
        assert_eq!(new_claims.expires_at, old.expires_at);
        assert_eq!(new_claims.transport_scope, "dbus,grpc,mcp,mqtt");
        assert!(
            authenticate_sid1_wire(&engine, &inline_sid1_wire(&legacy), "grpc")
                .await
                .is_err()
        );
        authenticate_sid1_wire(
            &engine,
            &inline_sid1_wire(current.sealed_id.as_deref().unwrap()),
            "mqtt",
        )
        .await
        .unwrap();
        let private = strict_private_state()
            .unwrap()
            .sleds
            .into_iter()
            .find(|sled| sled.session_id == identity.session_id)
            .unwrap();
        assert_eq!(private.sealed_id, current.sealed_id);
        restore_grants_path(previous);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn nextdns_refresh_uses_owned_assignment_and_invalidates_old_bytes() {
        let _refresh_lock = refresh_test_lock().lock().await;
        let (engine, _shm, identity, _grants_dir, previous, _registry) =
            refresh_test_fixture_with_claims(0x91, Some("dbus,grpc,mcp,mqtt"), 0).await;
        let assignment = serde_json::json!([{
            "wireguard_pubkey": identity.wireguard_pubkey,
            "profile_id": "689ec7", "device_name": "Ghostbridge-Incus"
        }]);
        engine
            .configure_nextdns_bindings(
                crate::identity_nextdns::NextDnsBindings::parse(&assignment.to_string()).unwrap(),
            )
            .unwrap();
        let principal = op_identity::session::derive_principal_id(&identity.wireguard_pubkey);
        let old_wire = inline_sid1_wire(identity.sealed_id.as_deref().unwrap());
        let mut expected = op_identity::sealed_id::SealedId::open(&old_wire).unwrap();
        assert!(expected.nextdns.is_none());
        authenticate_sid1_wire(&engine, &old_wire, "grpc")
            .await
            .unwrap();
        let result = dispatch_identity_sled_method_for_verified_session(
            &engine,
            "refresh_credential",
            &serde_json::json!({}),
            &principal,
            Some(&identity.session_id),
            identity.genesis.as_deref(),
        )
        .await
        .unwrap();
        assert_eq!(result["refreshed"], true);
        let current = stored_session(&engine, &identity.session_id)
            .await
            .unwrap()
            .unwrap();
        let wire = inline_sid1_wire(current.sealed_id.as_deref().unwrap());
        let claims = authenticate_sid1_wire(&engine, &wire, "grpc")
            .await
            .unwrap();
        expected.nextdns = Some(op_identity::sealed_id::NextDnsContext {
            profile_id: "689ec7".into(),
            device_name: "Ghostbridge-Incus".into(),
        });
        assert_eq!(claims, expected);
        assert!(
            authenticate_sid1_wire(&engine, &old_wire, "grpc")
                .await
                .is_err()
        );
        let mut forged = claims.clone();
        forged.nextdns.as_mut().unwrap().profile_id = "abcdef".into();
        assert!(
            authenticate_sid1_wire(&engine, &forged.seal().unwrap(), "grpc")
                .await
                .is_err()
        );
        let second = dispatch_identity_sled_method_for_verified_session(
            &engine,
            "refresh_credential",
            &serde_json::json!({}),
            &principal,
            Some(&identity.session_id),
            identity.genesis.as_deref(),
        )
        .await
        .unwrap();
        assert_eq!(second["refreshed"], false);
        // The same configured key receives its association at initial mint;
        // unrelated keys never inherit it.
        for (key, assigned) in [
            (identity.wireguard_pubkey.as_str(), true),
            (pk(0x92).as_str(), false),
        ] {
            let stamp = engine
                .mint_identity_genesis_stamp(&op_identity::session::derive_session_id(key), key, 0)
                .await
                .unwrap();
            let claims =
                op_identity::sealed_id::SealedId::from_inline_ref(&stamp.sealed_id).unwrap();
            assert_eq!(claims.nextdns.is_some(), assigned);
        }
        assert!(
            engine
                .configure_nextdns_bindings(crate::identity_nextdns::NextDnsBindings::default())
                .is_err()
        );
        restore_grants_path(previous);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn refresh_is_idempotent_and_rejects_unverified_inputs() {
        let _refresh_lock = refresh_test_lock().lock().await;
        let (engine, _shm, identity, _grants_dir, previous, _registry) =
            refresh_test_fixture(0x82).await;
        let actor = op_identity::session::derive_principal_id(&identity.wireguard_pubkey);
        let first = dispatch_identity_sled_method_for_verified_session(
            &engine,
            "refresh_credential",
            &serde_json::json!({}),
            &actor,
            Some(&identity.session_id),
            identity.genesis.as_deref(),
        )
        .await
        .unwrap();
        let current = revision_sled(
            &load_revision(engine.identity_sled_store().unwrap(), &identity.session_id)
                .await
                .unwrap(),
        )
        .unwrap()
        .unwrap();
        let second = dispatch_identity_sled_method_for_verified_session(
            &engine,
            "refresh_credential",
            &serde_json::json!({}),
            &actor,
            Some(&identity.session_id),
            current.genesis.as_deref(),
        )
        .await
        .unwrap();
        assert_eq!(first["refreshed"], false);
        assert_eq!(second["refreshed"], false);
        let error = dispatch_identity_sled_method_for_verified_session(
            &engine,
            "refresh_credential",
            &serde_json::json!({"session_id": identity.session_id}),
            &actor,
            Some(&identity.session_id),
            current.genesis.as_deref(),
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("accepts no arguments"));
        let error = dispatch_identity_sled_method_for_verified_session(
            &engine,
            "refresh_credential",
            &serde_json::json!({}),
            "wrong-actor",
            Some(&identity.session_id),
            current.genesis.as_deref(),
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("verified anchored session"));
        let error = dispatch_identity_sled_method_for_verified_session(
            &engine,
            "refresh_credential",
            &serde_json::json!({}),
            &actor,
            None,
            current.genesis.as_deref(),
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("verified session metadata"));
        assert!(
            dispatch_identity_sled_method_for_verified_session(
                &engine,
                "refresh_credential",
                &serde_json::json!({}),
                &actor,
                Some(&identity.session_id),
                Some("wrong-genesis")
            )
            .await
            .is_err()
        );
        assert!(
            dispatch_identity_sled_method_for_verified_session(
                &engine,
                "refresh_credential",
                &serde_json::json!({}),
                &actor,
                Some("wrong-session"),
                current.genesis.as_deref()
            )
            .await
            .is_err()
        );
        assert!(
            dispatch_with_fixtures(
                &engine,
                Arc::new(RecordingLifecycle::default()),
                no_btrfs_mount_verifier(),
                "refresh_credential",
                serde_json::json!({})
            )
            .await
            .is_err()
        );
        assert!(
            dispatch_identity_sled_method(&engine, "refresh_credential", &serde_json::json!({}))
                .await
                .is_err()
        );
        restore_grants_path(previous);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn refresh_rejects_invalid_scope_expiry_revocation_and_missing_grant() {
        let _refresh_lock = refresh_test_lock().lock().await;
        {
            let (bad_engine, _bad_shm, bad_identity, _bad_grants, bad_previous, _bad_registry) =
                refresh_test_fixture_with_claims(0x84, Some("dbus,mqtt"), 0).await;
            let bad_actor =
                op_identity::session::derive_principal_id(&bad_identity.wireguard_pubkey);
            assert!(
                dispatch_identity_sled_method_for_verified_session(
                    &bad_engine,
                    "refresh_credential",
                    &serde_json::json!({}),
                    &bad_actor,
                    Some(&bad_identity.session_id),
                    bad_identity.genesis.as_deref()
                )
                .await
                .is_err()
            );
            restore_grants_path(bad_previous);
        }
        {
            let (
                expired_engine,
                _expired_shm,
                expired_identity,
                _expired_grants,
                expired_previous,
                _expired_registry,
            ) = refresh_test_fixture_with_claims(0x85, None, now() - 1).await;
            let expired_actor =
                op_identity::session::derive_principal_id(&expired_identity.wireguard_pubkey);
            assert!(
                dispatch_identity_sled_method_for_verified_session(
                    &expired_engine,
                    "refresh_credential",
                    &serde_json::json!({}),
                    &expired_actor,
                    Some(&expired_identity.session_id),
                    expired_identity.genesis.as_deref()
                )
                .await
                .is_err()
            );
            restore_grants_path(expired_previous);
        }
        let (engine, _shm, identity, grants_dir, previous, _registry) =
            refresh_test_fixture(0x83).await;
        let actor = op_identity::session::derive_principal_id(&identity.wireguard_pubkey);
        std::fs::write(grants_dir.path().join("capability-grants.json"), b"{}").unwrap();
        assert!(
            dispatch_identity_sled_method_for_verified_session(
                &engine,
                "refresh_credential",
                &serde_json::json!({}),
                &actor,
                Some(&identity.session_id),
                identity.genesis.as_deref()
            )
            .await
            .is_err()
        );
        std::fs::write(
            grants_dir.path().join("capability-grants.json"),
            serde_json::to_vec(
                &serde_json::json!({actor.clone(): {"capabilities": ["identity_sled.credential.refresh"]}}),
            )
            .unwrap(),
        )
        .unwrap();
        crate::human_principal_dispatch::dispatch_human_principal_method(
            "revoke_key",
            &serde_json::json!({"human_pubkey": identity.wireguard_pubkey}),
        )
        .await
        .unwrap();
        assert!(
            dispatch_identity_sled_method_for_verified_session(
                &engine,
                "refresh_credential",
                &serde_json::json!({}),
                &actor,
                Some(&identity.session_id),
                identity.genesis.as_deref()
            )
            .await
            .is_err()
        );
        restore_grants_path(previous);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn parked_anchored_sled_authenticates_without_treating_active_as_revocation() {
        let (engine, _shm) = sled_engine();
        let identity = write_identity(&engine, &pk(0x71)).await.unwrap();
        assert!(!identity.active);
        let sealed = identity.sealed_id.clone().unwrap();
        let wire = inline_sid1_wire(&sealed);
        for transport in ["mcp", "dbus", "grpc"] {
            authenticate_sid1_wire(&engine, &wire, transport)
                .await
                .unwrap();
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn exact_authored_sid1_is_stable_and_expired_term_is_rejected() {
        let (engine, _shm) = sled_engine();
        let key = pk(0x72);
        let first = write_identity(&engine, &key).await.unwrap();
        let second = write_identity(&engine, &key).await.unwrap();
        assert_eq!(first.genesis, second.genesis);
        assert_eq!(first.sealed_id, second.sealed_id);

        let expired = write_identity_term(&engine, &pk(0x73), now() - 1)
            .await
            .unwrap();
        let wire = inline_sid1_wire(expired.sealed_id.as_deref().unwrap());
        assert!(
            authenticate_sid1_wire(&engine, &wire, "grpc")
                .await
                .unwrap_err()
                .contains("expired")
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn queued_repair_rechecks_expired_authored_sid1() {
        let (engine, _shm) = sled_engine();
        let expired = write_identity_term(&engine, &pk(0x74), now() - 1)
            .await
            .unwrap();
        let actor = op_identity::session::derive_principal_id(&expired.wireguard_pubkey);
        let error = root_repair::verify_current_repair_sid1(
            &engine,
            &expired,
            &actor,
            expired.genesis.as_deref().unwrap(),
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("expired"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn owned_lifecycle_preserves_anchor_and_starts_and_stops_once() {
        let (engine, _shm) = sled_engine();
        let identity = write_identity(&engine, &pk(0x61)).await.unwrap();
        let lifecycle = Arc::new(RecordingLifecycle::default());
        let store = engine.identity_sled_store().unwrap().clone();
        let active = set_session_active_owned(
            engine.as_ref().clone(),
            store.clone(),
            lifecycle.clone(),
            identity.session_id.clone(),
            true,
        )
        .await
        .unwrap();
        assert!(active.active);
        assert_eq!(active.genesis, identity.genesis);
        let repeat = set_session_active_owned(
            engine.as_ref().clone(),
            store.clone(),
            lifecycle.clone(),
            identity.session_id.clone(),
            true,
        )
        .await
        .unwrap();
        assert!(repeat.active);
        let parked = set_session_active_owned(
            engine.as_ref().clone(),
            store,
            lifecycle.clone(),
            identity.session_id.clone(),
            false,
        )
        .await
        .unwrap();
        assert!(!parked.active);
        assert_eq!(parked.sealed_id, identity.sealed_id);
        assert_eq!(
            *lifecycle.calls.lock().unwrap(),
            vec![
                format!("start:{}", identity.session_id),
                format!("stop:{}", identity.session_id)
            ]
        );
    }

    async fn identity_with_dedicated_filesystem(
        engine: &Arc<MutationEngine>,
        byte: u8,
    ) -> ContainerIdentitySled {
        let key = pk(byte);
        let identity = write_identity(engine, &key).await.unwrap();
        let result = dispatch_with_fixtures(
            engine,
            Arc::new(RecordingLifecycle::default()),
            no_btrfs_mount_verifier(),
            "write_identity",
            serde_json::json!({
                "wireguard_pubkey":key,
                "btrfs_device":{
                    "device_path":"/private/leaf.img",
                    "mount_point":"/private/root",
                    "btrfs_uuid":"806e2801-8be0-4b0d-919c-3e319452667b",
                    "cozo_id":identity.session_id,
                    "attached":false
                }
            }),
        )
        .await
        .unwrap();
        serde_json::from_value(result["identity"].clone()).unwrap()
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn dedicated_filesystem_remains_attached_after_guest_stop() {
        let (engine, _shm) = sled_engine();
        let identity = identity_with_dedicated_filesystem(&engine, 0xa1).await;
        let lifecycle = Arc::new(RecordingLifecycle::default());
        let checks = Arc::new(Mutex::new(Vec::new()));
        let observed = checks.clone();
        let verifier: BtrfsMountVerifier = Arc::new(move |device, mount, uuid| {
            observed.lock().unwrap().push((
                device.to_path_buf(),
                mount.to_path_buf(),
                uuid.to_string(),
            ));
            Ok(())
        });
        let store = engine.identity_sled_store().unwrap().clone();
        let active = set_session_active_owned_with_verifier(
            engine.as_ref().clone(),
            store.clone(),
            lifecycle.clone(),
            identity.session_id.clone(),
            true,
            verifier.clone(),
        )
        .await
        .unwrap();
        assert!(active.btrfs_device.as_ref().unwrap().attached);
        // An already-active identity still verifies its mounted storage,
        // without an extra guest start or a membership mutation.
        set_session_active_owned_with_verifier(
            engine.as_ref().clone(),
            store.clone(),
            lifecycle.clone(),
            identity.session_id.clone(),
            true,
            verifier.clone(),
        )
        .await
        .unwrap();
        let parked = set_session_active_owned_with_verifier(
            engine.as_ref().clone(),
            store,
            lifecycle.clone(),
            identity.session_id.clone(),
            false,
            verifier,
        )
        .await
        .unwrap();
        assert!(!parked.active);
        assert_eq!(parked.instance.as_ref().unwrap().status, "Stopped");
        assert!(parked.btrfs_device.as_ref().unwrap().attached);
        assert_eq!(parked.genesis, identity.genesis);
        assert_eq!(parked.sealed_id, identity.sealed_id);
        assert_eq!(parked.principal_kind, identity.principal_kind);
        assert_eq!(
            parked.btrfs_device.as_ref().unwrap().cozo_id,
            identity.session_id
        );
        let expected = (
            PathBuf::from("/private/leaf.img"),
            PathBuf::from("/private/root"),
            "806e2801-8be0-4b0d-919c-3e319452667b".to_string(),
        );
        assert_eq!(
            *checks.lock().unwrap(),
            vec![expected.clone(), expected.clone(), expected]
        );
        assert_eq!(
            *lifecycle.calls.lock().unwrap(),
            vec![
                format!("start:{}", identity.session_id),
                format!("stop:{}", identity.session_id)
            ]
        );
        assert_sibling_unchanged(&engine, &parked).await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn dedicated_filesystem_verification_failure_prevents_guest_start() {
        let (engine, _shm) = sled_engine();
        let identity = identity_with_dedicated_filesystem(&engine, 0xa2).await;
        let lifecycle = Arc::new(RecordingLifecycle::default());
        let result = set_session_active_owned_with_verifier(
            engine.as_ref().clone(),
            engine.identity_sled_store().unwrap().clone(),
            lifecycle.clone(),
            identity.session_id.clone(),
            true,
            Arc::new(|_, _, _| anyhow::bail!("registered mount is absent")),
        )
        .await;
        assert!(result.unwrap_err().to_string().contains("mount is absent"));
        assert!(lifecycle.calls.lock().unwrap().is_empty());
        assert_sibling_unchanged(&engine, &identity).await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn dedicated_filesystem_missing_after_stop_requires_reconciliation() {
        let (engine, _shm) = sled_engine();
        let identity = identity_with_dedicated_filesystem(&engine, 0xa3).await;
        let lifecycle = Arc::new(RecordingLifecycle::default());
        let store = engine.identity_sled_store().unwrap().clone();
        let active = set_session_active_owned_with_verifier(
            engine.as_ref().clone(),
            store.clone(),
            lifecycle.clone(),
            identity.session_id.clone(),
            true,
            no_btrfs_mount_verifier(),
        )
        .await
        .unwrap();
        let result = set_session_active_owned_with_verifier(
            engine.as_ref().clone(),
            store,
            lifecycle.clone(),
            identity.session_id.clone(),
            false,
            Arc::new(|_, _, _| anyhow::bail!("mount disappeared")),
        )
        .await;
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("requires reconciliation")
        );
        let durable = stored_session(&engine, &identity.session_id)
            .await
            .unwrap()
            .unwrap();
        assert!(!durable.active);
        // No successful post-stop readback was committed as observed state.
        assert_eq!(durable.instance.as_ref().unwrap().status, "Running");
        assert_eq!(durable.btrfs_device, active.btrfs_device);
        assert_eq!(durable.sealed_id, identity.sealed_id);
        assert_eq!(
            *lifecycle.calls.lock().unwrap(),
            vec![
                format!("start:{}", identity.session_id),
                format!("stop:{}", identity.session_id)
            ]
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn overlapping_activate_then_park_is_serialized_per_session() {
        let (engine, _shm) = sled_engine();
        let identity = write_identity(&engine, &pk(0x66)).await.unwrap();
        let lifecycle = Arc::new(BlockingLifecycle::default());
        let store = engine.identity_sled_store().unwrap().clone();
        let activate = tokio::spawn(set_session_active_owned(
            engine.as_ref().clone(),
            store.clone(),
            lifecycle.clone(),
            identity.session_id.clone(),
            true,
        ));
        lifecycle.start_entered.notified().await;
        let park = tokio::spawn(set_session_active_owned(
            engine.as_ref().clone(),
            store,
            lifecycle.clone(),
            identity.session_id.clone(),
            false,
        ));
        tokio::task::yield_now().await;
        assert!(
            !park.is_finished(),
            "park must wait for the same-session start"
        );
        lifecycle.release_start.notify_one();
        activate.await.unwrap().unwrap();
        let parked = park.await.unwrap().unwrap();
        assert!(!parked.active);
        assert_eq!(parked.instance.as_ref().unwrap().status, "Stopped");
        assert_eq!(
            *lifecycle.calls.lock().unwrap(),
            vec![
                format!("start:{}", identity.session_id),
                format!("stop:{}", identity.session_id),
            ]
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn mutation_index_queues_behind_native_lifecycle_and_preserves_result() {
        let (engine, _shm) = sled_engine();
        let identity = write_identity(&engine, &pk(0x67)).await.unwrap();
        let lifecycle = Arc::new(BlockingLifecycle::default());
        let store = engine.identity_sled_store().unwrap().clone();
        let activate = tokio::spawn(set_session_active_owned(
            engine.as_ref().clone(),
            store,
            lifecycle.clone(),
            identity.session_id.clone(),
            true,
        ));
        lifecycle.start_entered.notified().await;
        let index_engine = engine.clone();
        let index_session = identity.session_id.clone();
        let advance = tokio::spawn(async move {
            advance_mutation_index(&index_engine, &index_session, 700).await
        });
        tokio::task::yield_now().await;
        assert!(
            !advance.is_finished(),
            "revision mutator must wait for same-session native lifecycle"
        );
        lifecycle.release_start.notify_one();
        activate.await.unwrap().unwrap();
        advance.await.unwrap().unwrap();
        let current = stored_session(&engine, &identity.session_id)
            .await
            .unwrap()
            .unwrap();
        assert!(current.active);
        assert_eq!(current.instance.as_ref().unwrap().status, "Running");
        assert_eq!(current.mutation_index, 700);
        assert_eq!(current.sealed_id, identity.sealed_id);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn identity_inventory_requires_verified_route_and_exact_capability() {
        let (engine, _shm) = sled_engine();
        assert!(
            dispatch_identity_sled_method(
                &engine,
                "inspect_identity_inventory",
                &serde_json::json!({})
            )
            .await
            .is_err()
        );
        assert!(
            dispatch_identity_sled_method_for_verified_session(
                &engine,
                "inspect_identity_inventory",
                &serde_json::json!({}),
                "owner",
                None,
                None
            )
            .await
            .is_err()
        );
        for grants in [
            std::collections::HashSet::new(),
            std::collections::HashSet::from([
                "identity_sled.read".into(),
                "identity_sled.write".into(),
            ]),
            std::collections::HashSet::from(["identity_sled.inventory.read.*".into()]),
        ] {
            assert!(require_identity_inventory_capability(&grants).is_err());
        }
        assert!(
            require_identity_inventory_capability(&std::collections::HashSet::from([
                "identity_sled.inventory.read".into()
            ]))
            .is_ok()
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn unverified_provision_and_legacy_overrides_are_rejected() {
        let (engine, _shm) = sled_engine();
        let key = pk(0x7a);
        for args in [
            serde_json::json!({"wireguard_pubkey": key}),
            serde_json::json!({"wireguard_pubkey": key, "plan_sha256": "a".repeat(64),
                "instance": {"name":"caller-selected"}}),
            serde_json::json!({"wireguard_pubkey": key, "plan_sha256": "a".repeat(64),
                "btrfs_device": {"device_path":"/tmp/caller.img"}}),
        ] {
            assert!(
                dispatch_identity_sled_method(&engine, "provision_container", &args)
                    .await
                    .is_err()
            );
        }
        assert!(
            dispatch_identity_sled_method(
                &engine,
                "inspect_own_root_repair_preflight",
                &serde_json::json!({}),
            )
            .await
            .is_err()
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn attach_btrfs_device_verifies_isolated_mount_and_commits_readback() {
        let (engine, _shm) = sled_engine();
        let public_key = pk(0x7b);
        let sibling = write_identity(&engine, &pk(0x7d)).await.unwrap();
        assert_sibling_unchanged(&engine, &sibling).await;
        let session_id = op_identity::session::derive_session_id(&public_key);
        let anchored = write_identity(&engine, &public_key).await.unwrap();
        let provisioned = dispatch_with_fixtures(
            &engine,
            Arc::new(RecordingLifecycle::default()),
            no_btrfs_mount_verifier(),
            "write_identity",
            serde_json::json!({
                "wireguard_pubkey": public_key,
                "btrfs_device": {
                    "device_path": "/tmp/identity-fstorage.img",
                    "mount_point": "/tmp/identity-fstorage",
                    "btrfs_uuid": "00000000-0000-0000-0000-000000000002",
                    "cozo_id": "cozo-device-8",
                    "attached": false
                }
            }),
        )
        .await
        .unwrap();
        let before: ContainerIdentitySled =
            serde_json::from_value(provisioned["identity"].clone()).unwrap();
        assert_eq!(before.genesis, anchored.genesis);
        let calls = Arc::new(Mutex::new(Vec::<(String, String, String)>::new()));
        let observed_calls = calls.clone();
        let verifier: BtrfsMountVerifier = Arc::new(move |device, mount, uuid| {
            observed_calls.lock().unwrap().push((
                device.display().to_string(),
                mount.display().to_string(),
                uuid.to_string(),
            ));
            anyhow::ensure!(device == std::path::Path::new("/tmp/identity-fstorage.img"));
            anyhow::ensure!(mount == std::path::Path::new("/tmp/identity-fstorage"));
            anyhow::ensure!(uuid == "00000000-0000-0000-0000-000000000002");
            Ok(())
        });
        let output = dispatch_with_fixtures(
            &engine,
            Arc::new(RecordingLifecycle::default()),
            verifier,
            "attach_btrfs_device",
            serde_json::json!({"session_id": session_id}),
        )
        .await
        .unwrap();
        let attached: ContainerIdentitySled =
            serde_json::from_value(output["identity"].clone()).unwrap();
        assert_eq!(attached.session_id, before.session_id);
        assert_eq!(attached.mutation_index, before.mutation_index + 1);
        assert_eq!(attached.genesis, before.genesis);
        assert_eq!(attached.sealed_id, before.sealed_id);
        assert!(attached.btrfs_device.as_ref().unwrap().attached);
        assert_eq!(
            *calls.lock().unwrap(),
            vec![(
                "/tmp/identity-fstorage.img".into(),
                "/tmp/identity-fstorage".into(),
                "00000000-0000-0000-0000-000000000002".into(),
            )]
        );

        let store = engine.identity_sled_store().unwrap();
        let durable = revision_sled(&load_revision(store, &session_id).await.unwrap())
            .unwrap()
            .unwrap();
        assert!(durable.btrfs_device.as_ref().unwrap().attached);
        assert_eq!(durable.sealed_id, before.sealed_id);
        let private = strict_private_state().unwrap();
        let shm_identity = private
            .sleds
            .into_iter()
            .find(|sled| sled.session_id == session_id)
            .unwrap();
        assert!(shm_identity.btrfs_device.as_ref().unwrap().attached);
        assert_eq!(shm_identity.genesis, before.genesis);
        assert_sibling_unchanged(&engine, &sibling).await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn touch_session_starts_parked_container_and_preserves_identity_metadata() {
        let (engine, _shm) = sled_engine();
        let sibling = write_identity(&engine, &pk(0x7e)).await.unwrap();
        assert_sibling_unchanged(&engine, &sibling).await;
        let identity = write_identity(&engine, &pk(0x7c)).await.unwrap();
        assert_sibling_unchanged(&engine, &sibling).await;
        let lifecycle = Arc::new(RecordingLifecycle::default());
        let touched = dispatch_with_fixtures(
            &engine,
            lifecycle.clone(),
            no_btrfs_mount_verifier(),
            "touch_session",
            serde_json::json!({"session_id": identity.session_id}),
        )
        .await
        .unwrap();
        let touched: ContainerIdentitySled =
            serde_json::from_value(touched["identity"].clone()).unwrap();
        assert!(touched.active);
        assert_eq!(touched.instance.as_ref().unwrap().status, "Running");
        assert_eq!(touched.principal_kind.as_deref(), Some("human"));
        assert_eq!(touched.genesis, identity.genesis);
        assert_eq!(touched.sealed_id, identity.sealed_id);
        assert!(touched.last_seen_at >= identity.last_seen_at);
        assert_eq!(
            *lifecycle.calls.lock().unwrap(),
            vec![format!("start:{}", identity.session_id)]
        );

        let store = engine.identity_sled_store().unwrap();
        let durable = revision_sled(&load_revision(store, &identity.session_id).await.unwrap())
            .unwrap()
            .unwrap();
        assert!(durable.active);
        assert_eq!(durable.principal_kind.as_deref(), Some("human"));
        assert_eq!(durable.genesis, identity.genesis);
        assert_eq!(durable.sealed_id, identity.sealed_id);
        let private = strict_private_state().unwrap();
        let shm_identity = private
            .sleds
            .into_iter()
            .find(|sled| sled.session_id == identity.session_id)
            .unwrap();
        assert!(shm_identity.active);
        assert_eq!(shm_identity.instance.as_ref().unwrap().status, "Running");
        assert_eq!(shm_identity.principal_kind.as_deref(), Some("human"));
        assert_eq!(shm_identity.sealed_id, identity.sealed_id);
        assert_sibling_unchanged(&engine, &sibling).await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn stop_failure_keeps_committed_inactive_policy_without_rollback() {
        let (engine, _shm) = sled_engine();
        let identity = write_identity(&engine, &pk(0x62)).await.unwrap();
        let store = engine.identity_sled_store().unwrap().clone();
        set_session_active_owned(
            engine.as_ref().clone(),
            store.clone(),
            Arc::new(RecordingLifecycle::default()),
            identity.session_id.clone(),
            true,
        )
        .await
        .unwrap();
        let failing = Arc::new(RecordingLifecycle {
            calls: Mutex::new(Vec::new()),
            fail_stop: true,
        });
        let result = set_session_active_owned(
            engine.as_ref().clone(),
            store,
            failing,
            identity.session_id.clone(),
            false,
        )
        .await;
        assert!(result.is_err());
        let persisted = stored_session(&engine, &identity.session_id)
            .await
            .unwrap()
            .unwrap();
        assert!(!persisted.active);
        assert_eq!(persisted.instance.unwrap().status, "Running");
        assert_eq!(persisted.sealed_id, identity.sealed_id);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn caller_cannot_supply_sid1_or_legacy_psk() {
        let (engine, _shm) = sled_engine();
        let args = serde_json::json!({"wireguard_pubkey":pk(0x5a),"sealed_id":"sid1:caller"});
        assert!(
            dispatch_identity_sled_method(&engine, "write_identity", &args)
                .await
                .is_err()
        );
        assert!(
            dispatch_identity_sled_method(&engine, "provision_container", &args)
                .await
                .is_err()
        );
        assert!(derive_session_id(&serde_json::json!({"psk":"retired"}), &pk(0x5a)).is_err());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn genesis_sid1_and_actor_class_survive_mutation_index_advance() {
        let (engine, _shm) = sled_engine();
        let identity = write_identity(&engine, &pk(0x55)).await.unwrap();
        advance_mutation_index(&engine, &identity.session_id, 500)
            .await
            .unwrap();
        advance_mutation_index(&engine, &identity.session_id, 10)
            .await
            .unwrap();
        let after = stored_session(&engine, &identity.session_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(after.mutation_index, 500);
        assert_eq!(after.genesis, identity.genesis);
        assert_eq!(after.sealed_id, identity.sealed_id);
        assert_eq!(after.principal_kind.as_deref(), Some("human"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn event_receipt_and_bounded_history_come_from_durable_ledger() {
        let (engine, _shm) = sled_engine();
        let identity = write_identity(&engine, &pk(0x68)).await.unwrap();
        let store = engine.identity_sled_store().unwrap();
        for kind in ["first", "second"] {
            let output = dispatch_identity_sled_method_with_operation(
                &engine,
                store,
                opid("event"),
                "record_session_event",
                &serde_json::json!({"session_id":&identity.session_id,"kind":kind}),
            )
            .await
            .unwrap();
            assert_eq!(output["session_id"], identity.session_id.as_str());
            assert!(output["seq"].as_u64().is_some());
        }
        let history = dispatch_identity_sled_method(
            &engine,
            "get_session_history",
            &serde_json::json!({"session_id":&identity.session_id,"limit":0}),
        )
        .await
        .unwrap();
        let events = history["events"].as_array().unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0]["kind"], "second");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn session_record_resolves_every_authored_handle() {
        let (engine, _shm) = sled_engine();
        let pubkey = pk(0x56);
        let identity = write_identity(&engine, &pubkey).await.unwrap();
        for handle in [
            identity.session_id.as_str(),
            pubkey.as_str(),
            identity.trace_id.as_str(),
            identity.genesis.as_deref().unwrap(),
        ] {
            assert_eq!(
                session_record_for_actor(&engine, handle)
                    .await
                    .unwrap()
                    .unwrap()
                    .session_id,
                identity.session_id
            );
        }
        assert!(
            session_record_for_actor(&engine, "")
                .await
                .unwrap()
                .is_none()
        );
    }

    include!("identity_sled_private_fixture_tests.rs");
}
