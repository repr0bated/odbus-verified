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

use async_trait::async_trait;
use base64::Engine as _;
use op_core::projection_shm::CredentialPublication;
use op_cozo_store::{
    CozoGraphShuttle, GenesisInputsRecord, IdentityCompareExchange, IdentityIntent,
    IdentityRecoverySnapshot, IdentityRevision, IdentitySledRecord, SessionEventRecord,
};
use op_plugins::state_plugins::{
    identity_sled::{
        ContainerIdentitySled, IdentitySledState, SessionEvent, RECORD_FORMAT, SCHEMA_CONTENT_HASH,
    },
    incus::{IdentitySessionPowerState, IncusInstance, IncusPlugin},
    incus_device::Device,
};
use serde_json::Value as JsonValue;

use crate::mutation_engine::{GenesisStamp, MutationEngine};

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
    async fn create(&self, instance: &IncusInstance) -> anyhow::Result<()>;
    async fn start(&self, session_id: &str) -> anyhow::Result<()>;
    async fn stop(&self, session_id: &str) -> anyhow::Result<()>;
}

#[derive(Clone)]
struct IncusIdentityContainerLifecycle;

#[async_trait]
impl IdentityContainerLifecycle for IncusIdentityContainerLifecycle {
    async fn create(&self, instance: &IncusInstance) -> anyhow::Result<()> {
        IncusPlugin::apply_create(instance).await.map(|_| ())
    }
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

fn strict_identity_contract(
) -> anyhow::Result<crate::sealed_schema_reader::ManifestPinnedSchemaDocument> {
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

async fn set_session_active_owned(
    engine: MutationEngine,
    store: IdentitySledStore,
    lifecycle: Arc<dyn IdentityContainerLifecycle>,
    session_id: String,
    active: bool,
) -> anyhow::Result<ContainerIdentitySled> {
    let lifecycle_lock = store.lifecycle_lock(&session_id).await;
    let _lifecycle_guard = lifecycle_lock.lock_owned().await;
    let expected = load_revision(&store, &session_id).await?;
    let mut sled =
        revision_sled(&expected)?.ok_or_else(|| anyhow::anyhow!("identity session not found"))?;
    let has_instance = sled.instance.is_some();
    if active && !sled.active && has_instance {
        preflight_native_identity_effect().await?;
        lifecycle.start(&session_id).await?;
    }
    if !active {
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
            lifecycle.stop(&session_id).await?;
            let expected = load_revision(&store, &session_id).await?;
            let mut stopped =
                revision_sled(&expected)?.ok_or_else(|| anyhow::anyhow!("identity disappeared"))?;
            if let Some(instance) = stopped.instance.as_mut() {
                instance.status = "Stopped".into();
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
    if let Some(instance) = sled.instance.as_mut() {
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
        if had_instance {
            preflight_native_identity_effect().await?;
            IncusIdentityContainerLifecycle.stop(&session_id).await?;
        }
        Ok(())
    })
    .await?
}

pub(crate) async fn park_orphaned_container_sessions(
    engine: &MutationEngine,
) -> anyhow::Result<usize> {
    let sessions: Vec<String> = read_private_state()
        .await?
        .sleds
        .into_iter()
        // Every process-local binding was lost. Stop every durable container,
        // including one whose native start completed before a failed CAS left
        // its durable status at Stopped.
        .filter(|sled| sled.instance.is_some())
        .map(|sled| sled.session_id)
        .collect();
    let mut failures = Vec::new();
    for session in &sessions {
        if let Err(error) = deactivate_session(engine, session).await {
            failures.push(format!("{session}: {error:#}"));
        }
        engine.forget_session_context(session).await;
    }
    if failures.is_empty() {
        Ok(sessions.len())
    } else {
        anyhow::bail!("failed to park orphaned sessions: {}", failures.join("; "))
    }
}

async fn dispatch_identity_sled_method_owned(
    engine: &MutationEngine,
    store: &IdentitySledStore,
    operation_id: String,
    method: &str,
    args: &JsonValue,
) -> anyhow::Result<JsonValue> {
    match method {
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
            anyhow::ensure!(
                args.get("sealed_id").is_none(),
                "sealed_id is identity-authored"
            );
            anyhow::ensure!(
                args.get("ttl_seconds").is_none(),
                "ttl_seconds is not part of the sealed provision_container contract"
            );
            let public_key = arg_str(args, "wireguard_pubkey");
            anyhow::ensure!(!public_key.is_empty(), "wireguard_pubkey is required");
            let session_id = derive_session_id(args, &public_key)?;
            let instance = match args.get("instance") {
                Some(value) => {
                    let mut value = value.clone();
                    if let Some(object) = value.as_object_mut() {
                        object.entry("name").or_insert(serde_json::json!(""));
                        object.entry("status").or_insert(serde_json::json!(""));
                        object.entry("type").or_insert(serde_json::json!(""));
                    }
                    serde_json::from_value(value)?
                }
                None => IncusInstance::default(),
            };
            let instance = prepare_parked_instance(&session_id, instance)?;
            let btrfs_device = args
                .get("btrfs_device")
                .cloned()
                .map(serde_json::from_value)
                .transpose()?;
            let expires_at = None;
            let lifecycle: Arc<dyn IdentityContainerLifecycle> =
                Arc::new(IncusIdentityContainerLifecycle);
            let owned_engine = engine.clone();
            let owned_store = store.clone();
            let owned_instance = instance.clone();
            let task = tokio::spawn(async move {
                let lifecycle_lock = owned_store.lifecycle_lock(&session_id).await;
                let _lifecycle_guard = lifecycle_lock.lock_owned().await;
                let expected = load_revision(&owned_store, &session_id).await?;
                anyhow::ensure!(expected.record.is_none(), "identity is already provisioned");
                let stamp = owned_engine
                    .mint_identity_genesis_stamp(&session_id, &public_key, 0)
                    .await?;
                preflight_native_identity_effect().await?;
                lifecycle.create(&owned_instance).await?;
                let timestamp = now();
                let mut sled = ContainerIdentitySled {
                    session_id: session_id.clone(),
                    wireguard_pubkey: public_key,
                    interface: String::new(),
                    peer_ip: None,
                    mutation_index: 0,
                    genesis: None,
                    trace_id: String::new(),
                    schema_version: RECORD_FORMAT,
                    vector_id: String::new(),
                    sealed_id: None,
                    principal_kind: None,
                    btrfs_device,
                    instance: Some(owned_instance),
                    session_started_at: timestamp,
                    last_seen_at: timestamp,
                    active: false,
                    expires_at,
                    arrival_timestamp: 0,
                    chain_head_at_arrival: String::new(),
                    catalog_hash_at_arrival: String::new(),
                    head_timestamp_at_arrival: 0,
                };
                apply_genesis_stamp(&mut sled, &stamp)?;
                commit_sled_update(
                    &owned_engine,
                    &owned_store,
                    operation_id,
                    expected,
                    sled.clone(),
                    sled,
                )
                .await
            });
            let sled = task.await??;
            Ok(serde_json::json!({"identity": sled}))
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
            tokio::task::spawn_blocking(move || {
                op_network::btrfs::verify_dedicated_mount(&path, &mount, &uuid)
            })
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
            let identity = activate_session(engine, &session_id).await?;
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
    use crate::human_principal_dispatch::tests::{pk, temp_shm, test_engine, TempShm};
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
        async fn create(&self, _instance: &IncusInstance) -> anyhow::Result<()> {
            Ok(())
        }
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
        async fn create(&self, instance: &IncusInstance) -> anyhow::Result<()> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("create:{}", instance.name));
            Ok(())
        }
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
        write_identity_term(engine, pubkey, 0).await
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

    async fn write_identity_term(
        engine: &MutationEngine,
        pubkey: &str,
        expires_at: i64,
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
        let stamp = engine
            .mint_identity_genesis_stamp(&session_id, pubkey, expires_at)
            .await?;
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
        assert!(authenticate_sid1_wire(&engine, &wire, "grpc")
            .await
            .unwrap_err()
            .contains("expired"));
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
        assert!(session_record_for_actor(&engine, "")
            .await
            .unwrap()
            .is_none());
    }
}
