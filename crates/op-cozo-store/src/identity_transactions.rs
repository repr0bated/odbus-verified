//! Atomic identity row/genesis/revision operations for the single store owner.
//! This is private persistence metadata, not plugin discovery/schema authority.
//! Legacy writes are rejected after a session acquires a revision/tombstone.
use super::*;
use cozo::{MultiTransaction, TransactionPayload};
use nix::{fcntl::OFlag, sys::stat::Mode};
use std::{
    fs::File,
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::fs::{MetadataExt, PermissionsExt},
    },
    path::{Component, Path, PathBuf},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IdentityRevision {
    pub session_id: String,
    pub revision: u64,
    pub operation_id: String,
    pub record: Option<IdentitySledRecord>,
    pub genesis: Option<GenesisInputsRecord>,
}

pub struct IdentityCompareExchange {
    pub expected: IdentityRevision,
    pub record: Option<IdentitySledRecord>,
    pub genesis: Option<GenesisInputsRecord>,
}

pub enum IdentityIntent {
    CompareExchange(Box<IdentityCompareExchange>),
    /// A narrowly bounded migration authored by the identity owner. Ordinary
    /// compare/exchange continues to prohibit changes to anchored credentials.
    RefreshTransportCredential {
        expected: Box<IdentityRevision>,
        sealed_id: String,
    },
    AdvanceMutationIndex {
        event_id: u64,
    },
}

pub struct IdentityCommit {
    pub current: IdentityRevision,
    pub replayed: bool,
}

pub struct IdentityRecoverySnapshot {
    pub identities: Vec<IdentityRevision>,
    /// Newest first; ties use session_id ascending then seq descending.
    /// Historical events for deleted sessions remain part of the ledger.
    pub events: Vec<SessionEventRecord>,
}

#[derive(Default)]
struct RevisionMetadata {
    revision: u64,
    operation_id: String,
    kind: String,
    value: i64,
    principal_kind: Option<String>,
}

fn err(message: &str) -> CozoError {
    CozoError::Other(message.into())
}
fn native(error: impl std::fmt::Display) -> CozoError {
    CozoError::Other(format!("identity transaction: {error}"))
}
fn sid_params(sid: &str) -> Params {
    BTreeMap::from([("sid".into(), DataValue::Str(sid.into()))])
}

pub(super) fn open_persistent_directory(path: &Path) -> std::result::Result<File, CozoError> {
    open_directory(path, true)
}

fn open_directory(path: &Path, create: bool) -> std::result::Result<File, CozoError> {
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().map_err(native)?.join(path)
    };
    if path == Path::new("/") {
        return Err(err("database directory cannot be filesystem root"));
    }
    let mut current = File::open("/").map_err(native)?;
    for part in path.components() {
        let Component::Normal(name) = part else {
            if matches!(part, Component::RootDir) {
                continue;
            }
            return Err(err("database directory cannot contain dot components"));
        };
        let open = |directory: &File| {
            nix::fcntl::openat(
                Some(directory.as_raw_fd()),
                name,
                OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
                Mode::empty(),
            )
        };
        let raw = match open(&current) {
            Ok(raw) => raw,
            Err(nix::errno::Errno::ENOENT) if create => {
                match nix::sys::stat::mkdirat(
                    Some(current.as_raw_fd()),
                    name,
                    Mode::from_bits_truncate(0o700),
                ) {
                    Ok(()) | Err(nix::errno::Errno::EEXIST) => {}
                    Err(error) => return Err(native(error)),
                }
                open(&current).map_err(native)?
            }
            Err(error) => return Err(native(error)),
        };
        // SAFETY: nix0.28 returns a newly owned fd, transferred once to File.
        current = unsafe { File::from_raw_fd(raw) };
    }
    let metadata = current.metadata().map_err(native)?;
    if metadata.uid() != nix::unistd::geteuid().as_raw() || metadata.mode() & 0o022 != 0 {
        return Err(err(
            "persistent database directory must be owned by service and not group/world writable",
        ));
    }
    Ok(current)
}

pub(super) fn verify_persistent_directory(
    path: &Path,
    expected: &File,
) -> std::result::Result<(), CozoError> {
    let current = open_directory(path, false)?.metadata().map_err(native)?;
    let expected = expected.metadata().map_err(native)?;
    if current.dev() != expected.dev() || current.ino() != expected.ino() {
        return Err(err("persistent database directory changed while opening"));
    }
    Ok(())
}

/// Cozo0.7.6 stores its manifest in root and its default RocksDB/WAL in
/// root/data. Hold both actual directories, including separate mount points.
/// Custom root/options may redirect WAL/data paths; generic Cozo can open it,
/// but these stronger identity commits refuse that unverified layout.
pub(super) struct PersistentIdentityBoundary {
    path: PathBuf,
    root: File,
    data: File,
    custom_options: bool,
    private: bool,
}

fn has_custom_options(root: &File) -> std::result::Result<bool, CozoError> {
    match nix::sys::stat::fstatat(
        Some(root.as_raw_fd()),
        "options",
        nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW,
    ) {
        Ok(_) => Ok(true),
        Err(nix::errno::Errno::ENOENT) => Ok(false),
        Err(error) => Err(native(error)),
    }
}

impl PersistentIdentityBoundary {
    pub(super) fn open(path: &Path) -> std::result::Result<Self, CozoError> {
        Self::open_with_privacy(path, false)
    }

    pub(super) fn open_private(path: &Path) -> std::result::Result<Self, CozoError> {
        Self::open_with_privacy(path, true)
    }

    fn open_with_privacy(path: &Path, private: bool) -> std::result::Result<Self, CozoError> {
        let path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir().map_err(native)?.join(path)
        };
        let root = open_persistent_directory(&path)?;
        if private {
            restrict_directory(&root)?;
        }
        let data = open_persistent_directory(&path.join("data"))?;
        if private {
            restrict_directory(&data)?;
        }
        let custom_options = has_custom_options(&root)?;
        if private && custom_options {
            return Err(err("private identity store refuses custom Cozo options with unverified data/WAL locations"));
        }
        Ok(Self {
            path,
            root,
            data,
            custom_options,
            private,
        })
    }

    pub(super) fn verify(&self) -> std::result::Result<(), CozoError> {
        verify_persistent_directory(&self.path, &self.root)?;
        verify_persistent_directory(&self.path.join("data"), &self.data)?;
        if self.private
            && (self.root.metadata().map_err(native)?.mode() & 0o7777 != 0o700
                || self.data.metadata().map_err(native)?.mode() & 0o7777 != 0o700)
        {
            return Err(err(
                "private identity database directory permissions changed",
            ));
        }
        if has_custom_options(&self.root)? != self.custom_options {
            return Err(err("Cozo options layout changed while database was open"));
        }
        Ok(())
    }

    pub(super) fn require_default_layout(&self) -> std::result::Result<(), CozoError> {
        self.verify()?;
        if self.custom_options {
            return Err(err("identity durability requires default Cozo root/data layout; custom options WAL/data locations are unverified"));
        }
        Ok(())
    }

    pub(super) fn ensure_distinct_from(&self, other: &Self) -> std::result::Result<(), CozoError> {
        self.verify()?;
        other.verify()?;
        let left_root = self.root.metadata().map_err(native)?;
        let left_data = self.data.metadata().map_err(native)?;
        let right_root = other.root.metadata().map_err(native)?;
        let right_data = other.data.metadata().map_err(native)?;
        let aliases = |left: &std::fs::Metadata, right: &std::fs::Metadata| {
            left.dev() == right.dev() && left.ino() == right.ino()
        };
        if aliases(&left_root, &right_root)
            || aliases(&left_root, &right_data)
            || aliases(&left_data, &right_root)
            || aliases(&left_data, &right_data)
        {
            return Err(err("persistent Cozo stores must use distinct directories"));
        }
        Ok(())
    }

    pub(super) fn sync(&self) -> std::result::Result<(), CozoError> {
        self.require_default_layout()?;
        // Each barrier flushes an ENTIRE filesystem, not only this database.
        nix::unistd::syncfs(self.data.as_raw_fd()).map_err(native)?;
        if self.root.metadata().map_err(native)?.dev()
            != self.data.metadata().map_err(native)?.dev()
        {
            nix::unistd::syncfs(self.root.as_raw_fd()).map_err(native)?;
        }
        Ok(())
    }
}

fn restrict_directory(directory: &File) -> std::result::Result<(), CozoError> {
    // The fd was opened O_DIRECTORY|O_NOFOLLOW and its service ownership was
    // checked. The permission mutation cannot follow a replaced path/symlink.
    directory
        .set_permissions(std::fs::Permissions::from_mode(0o700))
        .map_err(native)?;
    directory.sync_all().map_err(native)?;
    if directory.metadata().map_err(native)?.mode() & 0o7777 != 0o700 {
        return Err(err("could not restrict identity database directory"));
    }
    Ok(())
}

// Inspect both transport and native commit results. cozo0.7.6's convenience
// MultiTransaction::commit ignores the inner native error; do not use it here.
pub(super) fn commit_checked(tx: &MultiTransaction) -> std::result::Result<(), CozoError> {
    tx.sender.send(TransactionPayload::Commit).map_err(native)?;
    unwrap_commit_reply(tx.receiver.recv())
}
fn unwrap_commit_reply<T, E: std::fmt::Display, C: std::fmt::Display>(
    reply: std::result::Result<std::result::Result<T, E>, C>,
) -> std::result::Result<(), CozoError> {
    reply.map_err(native)?.map_err(native)?;
    Ok(())
}
fn query(
    tx: &MultiTransaction,
    script: &str,
    params: Params,
) -> std::result::Result<NamedRows, CozoError> {
    tx.run_script(script, params).map_err(native)
}

impl CozoGraphShuttle {
    pub(super) fn legacy_identity_write_guard(
        &self,
        session_id: &str,
    ) -> std::result::Result<std::sync::MutexGuard<'_, ()>, CozoError> {
        let guard = self
            .identity_transaction
            .lock()
            .map_err(|_| err("identity transaction owner is poisoned"))?;
        let rows = cozo_run(
            &self.db,
            "?[session_id] := *identity_revisions{session_id}, session_id = $sid",
            sid_params(session_id),
        )
        .map_err(native)?;
        if !rows.rows.is_empty() {
            return Err(err(
                "versioned identity requires atomic intent; legacy mutation rejected",
            ));
        }
        Ok(guard)
    }

    fn sync_identity_store(&self) -> std::result::Result<(), CozoError> {
        #[cfg(test)]
        tests::fault("barrier")?;
        if let Some(directory) = &self.identity_directory {
            // Cozo's RocksDB transactions default to asynchronous WAL writes.
            // syncfs uses the held validated DB-directory fd and does not scan
            // database files. It flushes the ENTIRE filesystem: an intentional
            // cost of the identity durability boundary, not a cheap row flush.
            directory.sync()?;
        }
        // In-memory stores are test/ephemeral stores, never crash-durable.
        Ok(())
    }

    pub fn read_identity_revision(
        &self,
        session_id: &str,
    ) -> std::result::Result<IdentityRevision, CozoError> {
        let _guard = self
            .identity_transaction
            .lock()
            .map_err(|_| err("identity transaction owner is poisoned"))?;
        let tx = self.db.multi_transaction(false);
        let (state, _) = read_one(&tx, session_id)?;
        commit_checked(&tx)?;
        Ok(state)
    }

    /// Coherent committed row/genesis/revision snapshot for explicit recovery.
    /// No malformed row is omitted, converted to defaults, or deleted as a ghost.
    /// A syncfs failure prevents recovery from clearing its SHM pending marker.
    pub fn read_identity_recovery_snapshot(
        &self,
    ) -> std::result::Result<Vec<IdentityRevision>, CozoError> {
        Ok(self.read_identity_recovery_state(0)?.identities)
    }

    /// One coherent read transaction for identity rows and the bounded event
    /// window used to rebuild SHM. Zero excludes events; limits above4096 fail.
    pub fn read_identity_recovery_state(
        &self,
        event_limit: usize,
    ) -> std::result::Result<IdentityRecoverySnapshot, CozoError> {
        if event_limit > 4096 {
            return Err(err("identity recovery event limit exceeds4096"));
        }
        let _guard = self
            .identity_transaction
            .lock()
            .map_err(|_| err("identity transaction owner is poisoned"))?;
        self.sync_identity_store()?;
        let tx = self.db.multi_transaction(false);
        let rows = query(
            &tx,
            &select(ROW_FIELDS, "identity_sessions", false),
            BTreeMap::new(),
        )?;
        let genesis = query(
            &tx,
            &select(GENESIS_FIELDS, "identity_genesis", false),
            BTreeMap::new(),
        )?;
        let revisions = query(
            &tx,
            &select(REVISION_FIELDS, "identity_revisions", false),
            BTreeMap::new(),
        )?;
        let mut result: BTreeMap<String, IdentityRevision> = BTreeMap::new();
        for row in &rows.rows {
            let record = decode_record(row)?;
            let sid = record.session_id.clone();
            if result
                .insert(
                    sid.clone(),
                    IdentityRevision {
                        session_id: sid,
                        revision: 0,
                        operation_id: String::new(),
                        record: Some(record),
                        genesis: None,
                    },
                )
                .is_some()
            {
                return Err(err("duplicate durable identity row"));
            }
        }
        for row in &genesis.rows {
            let inputs = decode_genesis(row)?;
            let entry = result
                .get_mut(&inputs.session_id)
                .ok_or_else(|| err("orphan durable identity genesis"))?;
            if entry.genesis.replace(inputs).is_some() {
                return Err(err("duplicate durable identity genesis"));
            }
        }
        for row in &revisions.rows {
            let sid = string(row, 0)?;
            let meta = decode_revision(row)?;
            let entry = result.entry(sid.clone()).or_insert_with(|| absent(&sid));
            if entry.revision != 0 {
                return Err(err("duplicate durable identity revision"));
            }
            entry.revision = meta.revision;
            entry.operation_id = meta.operation_id;
            if let Some(record) = &mut entry.record {
                record.principal_kind = meta.principal_kind;
            }
        }
        for entry in result.values() {
            validate_pair(
                &entry.session_id,
                entry.record.as_ref(),
                entry.genesis.as_ref(),
            )?;
        }
        let events = if event_limit == 0 {
            Vec::new()
        } else {
            let rows = query(&tx, &format!("?[session_id, seq, kind, subid, content, created_at] := *session_events[session_id, seq, kind, subid, content, created_at] :order -created_at, session_id, -seq :limit {event_limit}"), BTreeMap::new())?;
            let mut bytes = 0usize;
            rows.rows
                .iter()
                .map(|row| {
                    let event = decode_event(row)?;
                    bytes = bytes
                        .checked_add(event.session_id.len())
                        .and_then(|size| size.checked_add(event.kind.len()))
                        .and_then(|size| size.checked_add(event.subid.len()))
                        .and_then(|size| size.checked_add(event.content.len()))
                        .ok_or_else(|| err("identity recovery event size overflow"))?;
                    if bytes > 16 * 1024 * 1024 {
                        return Err(err("identity recovery event window exceeds16MiB"));
                    }
                    Ok(event)
                })
                .collect::<std::result::Result<Vec<_>, CozoError>>()?
        };
        commit_checked(&tx)?;
        Ok(IdentityRecoverySnapshot {
            identities: result.into_values().collect(),
            events,
        })
    }

    pub(super) fn append_session_event_durable(
        &self,
        session_id: &str,
        kind: &str,
        subid: &str,
        content: &str,
        created_at: i64,
    ) -> std::result::Result<i64, CozoError> {
        if session_id.is_empty() || kind.is_empty() || created_at < 0 {
            return Err(err("invalid session event identity/kind/timestamp"));
        }
        let _guard = self
            .identity_transaction
            .lock()
            .map_err(|_| err("identity transaction owner is poisoned"))?;
        if let Some(directory) = &self.identity_directory {
            directory.require_default_layout()?;
        }
        let tx = self.db.multi_transaction(true);
        let max = query(&tx, "?[max(seq)] := *session_events[session_id, seq, kind, subid, content, created_at], session_id = $sid", sid_params(session_id))?;
        let seq = match max.rows.as_slice() {
            [] => 0,
            [row] if row.as_slice() == [DataValue::Null] => 0,
            [row] if row.len() == 1 => {
                let previous = integer(row, 0)?;
                if previous < 0 {
                    return Err(err("corrupt negative session event sequence"));
                }
                previous
                    .checked_add(1)
                    .ok_or_else(|| err("session event sequence overflow"))?
            }
            _ => return Err(err("corrupt session event sequence result")),
        };
        query(&tx, "?[session_id, seq, kind, subid, content, created_at] <- [[$sid,$seq,$kind,$subid,$content,$created]] :put session_events {session_id, seq => kind, subid, content, created_at}", BTreeMap::from([
            ("sid".into(),DataValue::Str(session_id.into())), ("seq".into(),dv_int(seq)),
            ("kind".into(),DataValue::Str(kind.into())), ("subid".into(),DataValue::Str(subid.into())),
            ("content".into(),DataValue::Str(content.into())), ("created".into(),dv_int(created_at)),
        ]))?;
        #[cfg(test)]
        tests::fault("before_commit")?;
        commit_checked(&tx)?;
        // Ambiguous failure must remain pending in the caller's SHM transaction;
        // appends have no operation key, so recovery must not blindly retry one.
        self.sync_identity_store()?;
        Ok(seq)
    }

    pub fn apply_identity_intent(
        &self,
        session_id: &str,
        operation_id: &str,
        intent: IdentityIntent,
    ) -> std::result::Result<IdentityCommit, CozoError> {
        if session_id.is_empty()
            || operation_id.is_empty()
            || operation_id.len() > 128
            || !operation_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
        {
            return Err(err("invalid identity/session operation identifier"));
        }
        let _guard = self
            .identity_transaction
            .lock()
            .map_err(|_| err("identity transaction owner is poisoned"))?;
        if let Some(directory) = &self.identity_directory {
            directory.require_default_layout()?;
        }
        let tx = self.db.multi_transaction(true);
        let (previous, metadata) = read_one(&tx, session_id)?;
        let (record, genesis, kind, value, replayed) = match intent {
            IdentityIntent::CompareExchange(exchange) => {
                let IdentityCompareExchange {
                    expected,
                    mut record,
                    genesis,
                } = *exchange;
                if expected.session_id != session_id {
                    return Err(err("identity CAS session mismatch"));
                }
                // An index is an advance-only field, even during a full record CAS.
                if let (Some(old), Some(new)) = (&expected.record, &mut record) {
                    new.mutation_index = new.mutation_index.max(old.mutation_index);
                }
                validate_pair(session_id, record.as_ref(), genesis.as_ref())?;
                let value = i64::try_from(expected.revision)
                    .map_err(|_| err("identity revision exceeds Cozo Int"))?;
                if previous.operation_id == operation_id {
                    if metadata.kind != "cas"
                        || metadata.value != value
                        || previous.record != record
                        || previous.genesis != genesis
                    {
                        return Err(err("identity operation id reused for a different intent"));
                    }
                    (record, genesis, "cas", value, true)
                } else {
                    if previous != expected {
                        return Err(err("identity compare/exchange conflict"));
                    }
                    validate_binding(&previous, record.as_ref(), genesis.as_ref())?;
                    (record, genesis, "cas", value, false)
                }
            }
            IdentityIntent::RefreshTransportCredential {
                expected,
                sealed_id,
            } => {
                if expected.session_id != session_id {
                    return Err(err("identity refresh session mismatch"));
                }
                let mut record = refresh_transport_record(&expected, sealed_id)?;
                record.mutation_index = record
                    .mutation_index
                    .checked_add(1)
                    .ok_or_else(|| err("identity index overflow"))?;
                let value = i64::try_from(expected.revision)
                    .map_err(|_| err("identity revision exceeds Cozo Int"))?;
                let genesis = expected.genesis.clone();
                let record = Some(record);
                let replayed = previous.operation_id == operation_id;
                if replayed {
                    if metadata.kind != "transport-refresh"
                        || metadata.value != value
                        || previous.record != record
                        || previous.genesis != genesis
                    {
                        return Err(err("identity operation id reused for a different intent"));
                    }
                } else if previous != *expected {
                    return Err(err("identity compare/exchange conflict"));
                }
                (record, genesis, "transport-refresh", value, replayed)
            }
            IdentityIntent::AdvanceMutationIndex { event_id } => {
                let value =
                    i64::try_from(event_id).map_err(|_| err("identity index exceeds Cozo Int"))?;
                let mut record = previous
                    .record
                    .clone()
                    .ok_or_else(|| err("identity index target is absent"))?;
                if previous.operation_id == operation_id
                    && (metadata.kind != "advance" || metadata.value != value)
                {
                    return Err(err("identity operation id reused for a different intent"));
                }
                record.mutation_index = record.mutation_index.max(value);
                (
                    Some(record),
                    previous.genesis.clone(),
                    "advance",
                    value,
                    previous.operation_id == operation_id,
                )
            }
        };
        if replayed {
            commit_checked(&tx)?;
            self.sync_identity_store()?;
            return Ok(IdentityCommit {
                current: previous,
                replayed: true,
            });
        }
        let revision = previous
            .revision
            .checked_add(1)
            .filter(|v| *v <= i64::MAX as u64)
            .ok_or_else(|| err("identity revision overflow"))?;
        put_pair(&tx, session_id, record.as_ref(), genesis.as_ref())?;
        #[cfg(test)]
        tests::fault("before_revision")?;
        let actor_class = match &record {
            Some(row) => row.principal_kind.as_deref(),
            None => metadata.principal_kind.as_deref(),
        }
        .unwrap_or("");
        query(&tx, "?[session_id, revision, operation_id, intent_kind, intent_value, principal_kind] <- [[$sid,$revision,$op,$kind,$value,$actor]] :put identity_revisions { session_id => revision, operation_id, intent_kind, intent_value, principal_kind }",
            BTreeMap::from([("sid".into(), DataValue::Str(session_id.into())), ("revision".into(), dv_int(revision as i64)), ("op".into(), DataValue::Str(operation_id.into())), ("kind".into(), DataValue::Str(kind.into())), ("value".into(), dv_int(value)),("actor".into(),DataValue::Str(actor_class.into()))]))?;
        #[cfg(test)]
        tests::fault("before_commit")?;
        commit_checked(&tx)?;
        self.sync_identity_store()?;
        Ok(IdentityCommit {
            current: IdentityRevision {
                session_id: session_id.into(),
                revision,
                operation_id: operation_id.into(),
                record,
                genesis,
            },
            replayed: false,
        })
    }
}

const ROW_FIELDS: &str = "session_id, wireguard_pubkey, interface, peer_ip, mutation_index, session_genesis, trace_id, schema_version, vector_id, sealed_id, btrfs_device_json, instance_json, session_started_at, last_seen_at, active, expires_at";
const GENESIS_FIELDS: &str = "session_id, arrival_timestamp, chain_head_at_arrival, catalog_hash_at_arrival, head_timestamp_at_arrival, schema_content_hash";
const REVISION_FIELDS: &str =
    "session_id, revision, operation_id, intent_kind, intent_value, principal_kind";
fn select(fields: &str, relation: &str, one: bool) -> String {
    format!(
        "?[{fields}] := *{relation}[{fields}]{}",
        if one { ", session_id = $sid" } else { "" }
    )
}
fn absent(sid: &str) -> IdentityRevision {
    IdentityRevision {
        session_id: sid.into(),
        revision: 0,
        operation_id: String::new(),
        record: None,
        genesis: None,
    }
}
fn read_one(
    tx: &MultiTransaction,
    sid: &str,
) -> std::result::Result<(IdentityRevision, RevisionMetadata), CozoError> {
    let row = query(
        tx,
        &select(ROW_FIELDS, "identity_sessions", true),
        sid_params(sid),
    )?;
    let genesis = query(
        tx,
        &select(GENESIS_FIELDS, "identity_genesis", true),
        sid_params(sid),
    )?;
    let meta = query(
        tx,
        &select(REVISION_FIELDS, "identity_revisions", true),
        sid_params(sid),
    )?;
    if row.rows.len() > 1 || genesis.rows.len() > 1 || meta.rows.len() > 1 {
        return Err(err("duplicate identity transaction record"));
    }
    let mut record = row.rows.first().map(|row| decode_record(row)).transpose()?;
    let genesis = genesis
        .rows
        .first()
        .map(|row| decode_genesis(row))
        .transpose()?;
    let meta = meta
        .rows
        .first()
        .map(|row| decode_revision(row))
        .transpose()?
        .unwrap_or_default();
    if let Some(record) = &mut record {
        record.principal_kind = meta.principal_kind.clone();
    }
    validate_pair(sid, record.as_ref(), genesis.as_ref())?;
    Ok((
        IdentityRevision {
            session_id: sid.into(),
            revision: meta.revision,
            operation_id: meta.operation_id.clone(),
            record,
            genesis,
        },
        meta,
    ))
}
fn string(row: &[DataValue], at: usize) -> std::result::Result<String, CozoError> {
    match row.get(at) {
        Some(DataValue::Str(value)) => Ok(value.to_string()),
        _ => Err(err("corrupt identity string column")),
    }
}
fn integer(row: &[DataValue], at: usize) -> std::result::Result<i64, CozoError> {
    match row.get(at) {
        Some(DataValue::Num(Num::Int(value))) => Ok(*value),
        _ => Err(err("corrupt identity integer column")),
    }
}
fn boolean(row: &[DataValue], at: usize) -> std::result::Result<bool, CozoError> {
    match row.get(at) {
        Some(DataValue::Bool(value)) => Ok(*value),
        _ => Err(err("corrupt identity boolean column")),
    }
}
fn decode_record(row: &[DataValue]) -> std::result::Result<IdentitySledRecord, CozoError> {
    if row.len() != 16 {
        return Err(err("corrupt identity row cardinality"));
    }
    Ok(IdentitySledRecord {
        session_id: string(row, 0)?,
        wireguard_pubkey: string(row, 1)?,
        interface: string(row, 2)?,
        peer_ip: string(row, 3)?,
        mutation_index: integer(row, 4)?,
        session_genesis: string(row, 5)?,
        trace_id: string(row, 6)?,
        schema_version: integer(row, 7)?,
        vector_id: string(row, 8)?,
        sealed_id: string(row, 9)?,
        principal_kind: None,
        btrfs_device_json: string(row, 10)?,
        instance_json: string(row, 11)?,
        session_started_at: integer(row, 12)?,
        last_seen_at: integer(row, 13)?,
        active: boolean(row, 14)?,
        expires_at: integer(row, 15)?,
    })
}
fn decode_genesis(row: &[DataValue]) -> std::result::Result<GenesisInputsRecord, CozoError> {
    if row.len() != 6 {
        return Err(err("corrupt identity genesis cardinality"));
    }
    Ok(GenesisInputsRecord {
        session_id: string(row, 0)?,
        arrival_timestamp: integer(row, 1)?,
        chain_head_at_arrival: string(row, 2)?,
        catalog_hash_at_arrival: string(row, 3)?,
        head_timestamp_at_arrival: integer(row, 4)?,
        schema_content_hash: string(row, 5)?,
    })
}
fn decode_revision(row: &[DataValue]) -> std::result::Result<RevisionMetadata, CozoError> {
    if row.len() != 6 {
        return Err(err("corrupt identity revision cardinality"));
    }
    let revision = integer(row, 1)?;
    let op = string(row, 2)?;
    let kind = string(row, 3)?;
    let value = integer(row, 4)?;
    let principal_kind = string(row, 5)?;
    if revision <= 0
        || op.is_empty()
        || op.len() > 128
        || !op
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
        || !matches!(kind.as_str(), "cas" | "advance" | "transport-refresh")
        || value < 0
        || !matches!(principal_kind.as_str(), "" | "human" | "service")
    {
        return Err(err("corrupt identity revision metadata"));
    }
    Ok(RevisionMetadata {
        revision: revision as u64,
        operation_id: op,
        kind,
        value,
        principal_kind: (!principal_kind.is_empty()).then_some(principal_kind),
    })
}

fn decode_event(row: &[DataValue]) -> std::result::Result<SessionEventRecord, CozoError> {
    if row.len() != 6 {
        return Err(err("corrupt session event cardinality"));
    }
    let event = SessionEventRecord {
        session_id: string(row, 0)?,
        seq: integer(row, 1)?,
        kind: string(row, 2)?,
        subid: string(row, 3)?,
        content: string(row, 4)?,
        created_at: integer(row, 5)?,
    };
    if event.session_id.is_empty() || event.kind.is_empty() || event.seq < 0 || event.created_at < 0
    {
        return Err(err("corrupt session event identity/kind/integer"));
    }
    Ok(event)
}
fn validate_pair(
    sid: &str,
    row: Option<&IdentitySledRecord>,
    genesis: Option<&GenesisInputsRecord>,
) -> std::result::Result<(), CozoError> {
    let Some(row) = row else {
        if genesis.is_some() {
            return Err(err("identity genesis has no row"));
        }
        return Ok(());
    };
    if row.session_id != sid
        || sid.is_empty()
        || row.wireguard_pubkey.is_empty()
        || row.mutation_index < 0
        || row.schema_version < 0
        || row.expires_at < 0
        || row
            .principal_kind
            .as_deref()
            .is_some_and(|kind| !matches!(kind, "human" | "service"))
    {
        return Err(err("corrupt identity record binding or integer"));
    }
    for json in [&row.instance_json, &row.btrfs_device_json] {
        if !json.is_empty() && !serde_json::from_str::<Value>(json)?.is_object() {
            return Err(err("identity embedded record must be a JSON object"));
        }
    }
    if let Some(genesis) = genesis {
        if genesis.session_id != sid
            || genesis.arrival_timestamp <= 0
            || genesis.chain_head_at_arrival.is_empty()
            || genesis.catalog_hash_at_arrival.is_empty()
            || genesis.schema_content_hash.is_empty()
        {
            return Err(err("corrupt identity genesis binding"));
        }
    }
    if !row.sealed_id.is_empty()
        && (genesis.is_none() || row.session_genesis.is_empty() || row.trace_id.is_empty())
    {
        return Err(err(
            "authored identity credential lacks durable genesis inputs",
        ));
    }
    Ok(())
}
fn refresh_transport_record(
    expected: &IdentityRevision,
    sealed_id: String,
) -> std::result::Result<IdentitySledRecord, CozoError> {
    let mut row = expected
        .record
        .clone()
        .ok_or_else(|| err("identity refresh target is absent"))?;
    let genesis = expected
        .genesis
        .as_ref()
        .ok_or_else(|| err("identity refresh genesis is absent"))?;
    validate_pair(&expected.session_id, Some(&row), Some(genesis))?;
    let mut old =
        op_identity::sealed_id::SealedId::from_inline_ref(&row.sealed_id).map_err(native)?;
    let new = op_identity::sealed_id::SealedId::from_inline_ref(&sealed_id).map_err(native)?;
    if row.principal_kind.as_deref() != Some("human")
        || old.principal_kind != "wireguard-principal"
        || old.principal_id != op_identity::session::derive_principal_id(&row.wireguard_pubkey)
        || old.session_id != row.session_id
        || old.session_id != op_identity::session::derive_session_id(&row.wireguard_pubkey)
        || old.wireguard_pubkey != row.wireguard_pubkey
        || old.session_genesis != row.session_genesis
        || old.trace_id != row.trace_id
        || i64::from(old.schema_version) != row.schema_version
        || old.expires_at != row.expires_at
        || old.issued_at != genesis.arrival_timestamp
        || old.arrival_timestamp != genesis.arrival_timestamp
        || old.chain_head_at_arrival != genesis.chain_head_at_arrival
        || old.catalog_hash_at_arrival != genesis.catalog_hash_at_arrival
        || old.head_timestamp_at_arrival != genesis.head_timestamp_at_arrival
        || old.transport_scope != "dbus,grpc,mcp"
        || new.transport_scope != "dbus,grpc,mcp,mqtt"
    {
        return Err(err(
            "identity transport refresh does not match anchored legacy claims",
        ));
    }
    old.transport_scope = "dbus,grpc,mcp,mqtt".into();
    if old != new {
        return Err(err(
            "identity transport refresh cannot replace anchored claims",
        ));
    }
    row.sealed_id = sealed_id;
    Ok(row)
}

fn validate_binding(
    previous: &IdentityRevision,
    row: Option<&IdentitySledRecord>,
    genesis: Option<&GenesisInputsRecord>,
) -> std::result::Result<(), CozoError> {
    if let (Some(old), Some(new)) = (&previous.record, row) {
        if old.principal_kind.is_some() && old.principal_kind != new.principal_kind {
            return Err(err("identity actor class is immutable once classified"));
        }
        if old.session_id != new.session_id || old.wireguard_pubkey != new.wireguard_pubkey {
            return Err(err("identity binding cannot be replaced"));
        }
        if !old.sealed_id.is_empty()
            && (old.sealed_id != new.sealed_id
                || old.session_genesis != new.session_genesis
                || old.trace_id != new.trace_id
                || old.schema_version != new.schema_version
                || old.expires_at != new.expires_at
                || previous.genesis.as_ref() != genesis)
        {
            return Err(err("anchored SID1/genesis fields are immutable"));
        }
    }
    Ok(())
}
fn put_pair(
    tx: &MultiTransaction,
    sid: &str,
    row: Option<&IdentitySledRecord>,
    genesis: Option<&GenesisInputsRecord>,
) -> std::result::Result<(), CozoError> {
    if let Some(row) = row {
        let values = vec![
            DataValue::Str(sid.into()),
            DataValue::Str(row.wireguard_pubkey.as_str().into()),
            DataValue::Str(row.interface.as_str().into()),
            DataValue::Str(row.peer_ip.as_str().into()),
            dv_int(row.mutation_index),
            DataValue::Str(row.session_genesis.as_str().into()),
            DataValue::Str(row.trace_id.as_str().into()),
            dv_int(row.schema_version),
            DataValue::Str(row.vector_id.as_str().into()),
            DataValue::Str(row.sealed_id.as_str().into()),
            DataValue::Str(row.btrfs_device_json.as_str().into()),
            DataValue::Str(row.instance_json.as_str().into()),
            dv_int(row.session_started_at),
            dv_int(row.last_seen_at),
            DataValue::Bool(row.active),
            dv_int(row.expires_at),
        ];
        query(
            tx,
            &format!(
                "?[{ROW_FIELDS}] <- $rows :put identity_sessions {{ session_id => {} }}",
                ROW_FIELDS.split_once(", ").unwrap().1
            ),
            BTreeMap::from([(
                "rows".into(),
                DataValue::List(vec![DataValue::List(values)]),
            )]),
        )?;
    } else {
        query(
            tx,
            "?[session_id] <- [[$sid]] :rm identity_sessions {session_id}",
            sid_params(sid),
        )?;
    }
    #[cfg(test)]
    tests::fault("after_row")?;
    if let Some(g) = genesis {
        let values = vec![
            DataValue::Str(sid.into()),
            dv_int(g.arrival_timestamp),
            DataValue::Str(g.chain_head_at_arrival.as_str().into()),
            DataValue::Str(g.catalog_hash_at_arrival.as_str().into()),
            dv_int(g.head_timestamp_at_arrival),
            DataValue::Str(g.schema_content_hash.as_str().into()),
        ];
        query(
            tx,
            &format!(
                "?[{GENESIS_FIELDS}] <- $rows :put identity_genesis {{session_id => {}}}",
                GENESIS_FIELDS.split_once(", ").unwrap().1
            ),
            BTreeMap::from([(
                "rows".into(),
                DataValue::List(vec![DataValue::List(values)]),
            )]),
        )?;
    } else {
        query(
            tx,
            "?[session_id] <- [[$sid]] :rm identity_genesis {session_id}",
            sid_params(sid),
        )?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "identity_transactions_tests.rs"]
mod tests;
