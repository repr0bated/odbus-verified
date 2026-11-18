//! Sparse, self-describing block store for the ublk device that persists the
//! btrfs container pool (and any other bytes written through the device).
//!
//! The pool's read-only seed is sprouted by ONE ublk device, added once on the
//! host with `btrfs device add`. That device's bytes live here, in a dedicated
//! Cozo/RocksDB database on the host's ext4 root. It must never sit inside
//! the btrfs pool it backs: the pool's persistence would then depend on itself.
//!
//! Relations:
//! - `devices { id => size_bytes, block_size, chunk_size, created_at, role, write_generation }`
//! - `blocks { device, idx => blob }`, where `blob` is a CBLK envelope (header +
//!   device id + optional inline schema + payload). Chunk `idx` holds device
//!   bytes `[idx * chunk_size, (idx + 1) * chunk_size)`. The chunk checksum is
//!   the CRC32C inside the envelope; there is no separate checksum column.
//! - `tombstones { device, idx => blob }`: a payload-less CBLK tombstone for
//!   every chunk that held data and was then discarded (or overwritten with
//!   zeros). It carries a fresh generation so offline raw-scan recovery treats
//!   the chunk as discarded instead of resurrecting an older data version that
//!   still lingers in WAL/SST/blob bytes. Rewriting the chunk removes it.
//!
//! Every mutation (data write or discard) puts its envelopes AND advances
//! `devices.write_generation` in ONE Cozo transaction (a chained script), so
//! the persisted generation is never behind a stored envelope.
//!
//! The block layer is content-agnostic: every chunk index is encoded the same
//! way. There is no special case for container images, filesystem metadata, or
//! user data.
//!
//! Sparse: a chunk with no `blocks` row reads as zeros, and writing an
//! all-zero chunk deletes its row (leaving a tombstone). A 500 GiB device costs nothing until btrfs writes to it,
//! and discard gives the space back.
//!
//! Durability: Cozo 0.7.6 commits with RocksDB's default `sync = false`, so a
//! returned `write_block` is in the WAL (it survives this process crashing)
//! but not yet on stable storage. [`CozoBlockStore::flush`] is the barrier: the
//! same held-directory `syncfs` that identity commits use. The ublk target must
//! call it for every FLUSH and FUA request before completing that request.
//!
//! RocksDB compression is disabled at every level (SST, per-level, bottommost,
//! blob files, WAL) via the root `options` file profile
//! (`CBLK_ROCKSDB_OPTIONS`); Cozo's JSON options cannot set it. The only
//! compression is zstd inside each CBLK envelope.
//!
//! Requests smaller than a chunk (a 4 KiB write inside a 64 KiB chunk) are
//! read-modify-write in the ublk target, which serialises I/O per chunk.

use crate::CozoError;
use crate::cblk::{self, CBLK_CHUNK_SIZE, should_inline_schema};
use crate::identity_transactions::PersistentIdentityBoundary;
use cozo::{DataValue, DbInstance, NamedRows, ScriptMutability};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

type Params = BTreeMap<String, DataValue>;

/// Default database directory: the host ext4 root, beside the other op-dbus
/// Cozo stores, never under `/var/lib/incus/storage-pools`.
pub const DEFAULT_UBLK_COZO_DB_PATH: &str = "/var/lib/op-dbus/ublk-cozo";
/// Overrides [`DEFAULT_UBLK_COZO_DB_PATH`] when set and non-empty.
pub const UBLK_COZO_DB_PATH_ENV: &str = "OP_UBLK_COZO_DB_PATH";

/// Logical block size the ublk device advertises (`logical_bs_shift = 12`).
pub const UBLK_LOGICAL_BLOCK_BYTES: u32 = 4096;

/// Storage granularity: one `blocks` row per 64 KiB of device space.
///
/// - btrfs writes 4 KiB data sectors and 16 KiB metadata nodes. Any write
///   smaller than a chunk is a read-modify-write of the whole chunk, so a
///   4 KiB write costs 64 KiB read plus 64 KiB written (16x). The `cozo-ublk`
///   prototype's 4 MiB chunks cost 1024x for the same write.
/// - Larger rows mean fewer Cozo queries per sequential request: a 512 KiB
///   request (the prototype's I/O buffer) is 8 rows here, 128 at 4 KiB.
/// - A fully written 500 GiB device is 8,192,000 rows; 4 KiB chunks would be
///   131,072,000 rows, each paying key and index overhead.
/// - Discard frees whole chunks only, so smaller chunks return more of what
///   btrfs trims.
///
/// 128 KiB would halve the row count but double small-write amplification,
/// and btrfs metadata writes (16 KiB nodes, the most frequent random writes)
/// would gain nothing from it.
pub const UBLK_CHUNK_BYTES: u32 = CBLK_CHUNK_SIZE;

/// The single persistence device for the container pool.
pub const POOL_DEFAULT_PERSIST_DEVICE: &str = "pool-default-persist";
/// Sparse virtual size of [`POOL_DEFAULT_PERSIST_DEVICE`]: 500 GiB.
pub const POOL_DEFAULT_PERSIST_BYTES: u64 = 500 * 1024 * 1024 * 1024;
/// Role recorded for a device that is `btrfs device add`ed to a pool.
pub const POOL_PERSIST_ROLE: &str = "btrfs-pool-persist";

/// Chunks discarded per transaction by [`CozoBlockStore::discard_blocks`].
const DISCARD_BATCH: usize = 1024;

const RELATIONS: [(&str, &str, &[&str]); 3] = [
    (
        "devices",
        ":create devices { id: String => size_bytes: Int, block_size: Int, chunk_size: Int, created_at: Int, role: String, write_generation: Int }",
        &[
            "id",
            "size_bytes",
            "block_size",
            "chunk_size",
            "created_at",
            "role",
            "write_generation",
        ],
    ),
    (
        "blocks",
        ":create blocks { device: String, idx: Int => blob: Bytes }",
        &["device", "idx", "blob"],
    ),
    (
        "tombstones",
        ":create tombstones { device: String, idx: Int => blob: Bytes }",
        &["device", "idx", "blob"],
    ),
];

/// Advance `devices.write_generation` to `$generation` inside a chained
/// transaction, then assert the device row now carries that generation. The
/// assertion runs last, so if it fails (device row missing) Cozo aborts the
/// WHOLE transaction, including the envelope puts chained before it.
const BUMP_GENERATION: &str = "{ ?[id, size_bytes, block_size, chunk_size, created_at, role, write_generation] := \
     *devices{id, size_bytes, block_size, chunk_size, created_at, role}, \
     id = $device, write_generation = $generation \
     :put devices { id => size_bytes, block_size, chunk_size, created_at, role, write_generation } } \
     { ?[id] := *devices{id, write_generation}, id = $device, write_generation = $generation \
     :assert some }";

/// Resolve the block store path: `$OP_UBLK_COZO_DB_PATH`, else the default.
pub fn ublk_cozo_db_path() -> PathBuf {
    std::env::var_os(UBLK_COZO_DB_PATH_ENV)
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_UBLK_COZO_DB_PATH))
}

/// One registered virtual block device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockDevice {
    pub id: String,
    pub size_bytes: u64,
    pub block_size: u32,
    pub chunk_size: u32,
    /// Unix seconds.
    pub created_at: i64,
    pub role: String,
    /// Last assigned CBLK generation for this device (persisted).
    pub write_generation: u64,
}

impl BlockDevice {
    /// Number of chunks; valid chunk indexes are `0..chunk_count()`.
    pub fn chunk_count(&self) -> u64 {
        self.size_bytes / u64::from(self.chunk_size)
    }
}

/// Row counts of the relations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockStoreCounts {
    pub devices: u64,
    pub blocks: u64,
    pub tombstones: u64,
}

/// Owner of the ublk block database. RocksDB's lock makes it single-process;
/// the ublk target is that process.
pub struct CozoBlockStore {
    db: DbInstance,
    directory: PersistentIdentityBoundary,
    path: PathBuf,
    registry: Mutex<()>,
    /// Monotonic per-device generation; seeded from `devices.write_generation` on open.
    generations: Mutex<BTreeMap<String, u64>>,
    /// Writes completed since this process opened the store (inline-schema cadence).
    writes_since_open: Mutex<u64>,
}

impl CozoBlockStore {
    /// Open the store at [`ublk_cozo_db_path`].
    pub fn open_default() -> Result<Self, CozoError> {
        Self::open(ublk_cozo_db_path())
    }

    /// Open the store at `path`, creating it if absent. Root and data
    /// directories are created 0700, must be owned by the calling user, and
    /// must keep Cozo's default root/data layout so that [`Self::flush`]
    /// covers the WAL. SST compression is disabled for CBLK recoverability.
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, CozoError> {
        let mut path = path.into();
        if path.is_relative() {
            path = std::env::current_dir()
                .map_err(|e| CozoError::Other(format!("ublk block store: {e}")))?
                .join(path);
        }
        let text = path
            .to_str()
            .ok_or_else(|| CozoError::Other("ublk block store path must be valid UTF-8".into()))?
            .to_string();
        let directory = PersistentIdentityBoundary::open_private_ublk(&path)?;
        directory.require_default_layout()?;
        let db = DbInstance::new("rocksdb", &text, Default::default())
            .map_err(|e| CozoError::Database(format!("open ublk block store at {text}: {e}")))?;
        directory.verify()?;
        let store = Self {
            db,
            directory,
            path,
            registry: Mutex::new(()),
            generations: Mutex::new(BTreeMap::new()),
            writes_since_open: Mutex::new(0),
        };
        store.ensure_schema()?;
        store.load_generations()?;
        Ok(store)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Register a device with the store's fixed geometry. Idempotent when the
    /// device already exists with the same size and role; any other existing
    /// registration is refused rather than resized or reinterpreted.
    pub fn register_device(
        &self,
        id: &str,
        size_bytes: u64,
        role: &str,
    ) -> Result<BlockDevice, CozoError> {
        validate_device_id(id)?;
        if role.is_empty() {
            return Err(invalid("device role must not be empty"));
        }
        let chunk = u64::from(UBLK_CHUNK_BYTES);
        if size_bytes == 0
            || !size_bytes.is_multiple_of(chunk)
            || i64::try_from(size_bytes).is_err()
        {
            return Err(invalid(&format!(
                "device size {size_bytes} must be a positive multiple of {chunk} bytes"
            )));
        }
        let _registry = self
            .registry
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(existing) = self.device(id)? {
            let same = existing.size_bytes == size_bytes
                && existing.block_size == UBLK_LOGICAL_BLOCK_BYTES
                && existing.chunk_size == UBLK_CHUNK_BYTES
                && existing.role == role;
            if same {
                return Ok(existing);
            }
            return Err(invalid(&format!(
                "device {id} is already registered as {existing:?}; refusing to change it"
            )));
        }
        let params = Params::from([
            ("id".into(), DataValue::from(id)),
            ("size_bytes".into(), DataValue::from(size_bytes as i64)),
            (
                "block_size".into(),
                DataValue::from(i64::from(UBLK_LOGICAL_BLOCK_BYTES)),
            ),
            (
                "chunk_size".into(),
                DataValue::from(i64::from(UBLK_CHUNK_BYTES)),
            ),
            ("created_at".into(), DataValue::from(now_unix()?)),
            ("role".into(), DataValue::from(role)),
            ("write_generation".into(), DataValue::from(0i64)),
        ]);
        self.run(
            "?[id, size_bytes, block_size, chunk_size, created_at, role, write_generation] <- \
             [[$id, $size_bytes, $block_size, $chunk_size, $created_at, $role, $write_generation]] \
             :put devices { id => size_bytes, block_size, chunk_size, created_at, role, write_generation }",
            params,
            ScriptMutability::Mutable,
        )?;
        {
            let mut gens = self
                .generations
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            gens.entry(id.to_string()).or_insert(0);
        }
        self.device(id)?
            .ok_or_else(|| CozoError::Database(format!("device {id} missing after registration")))
    }

    /// Look up a registered device.
    pub fn device(&self, id: &str) -> Result<Option<BlockDevice>, CozoError> {
        let rows = self.run(
            "?[size_bytes, block_size, chunk_size, created_at, role, write_generation] := \
             *devices{id: $id, size_bytes, block_size, chunk_size, created_at, role, write_generation}",
            Params::from([("id".into(), DataValue::from(id))]),
            ScriptMutability::Immutable,
        )?;
        let Some(row) = rows.rows.first() else {
            return Ok(None);
        };
        let int = |index: usize| {
            row.get(index).and_then(DataValue::get_int).ok_or_else(|| {
                CozoError::Database(format!("device {id}: column {index} is not an Int"))
            })
        };
        let size_bytes = u64::try_from(int(0)?).map_err(|_| corrupt(id, "negative size"))?;
        let block_size = u32::try_from(int(1)?).map_err(|_| corrupt(id, "bad block_size"))?;
        let chunk_size = u32::try_from(int(2)?).map_err(|_| corrupt(id, "bad chunk_size"))?;
        let created_at = int(3)?;
        let role = row
            .get(4)
            .and_then(DataValue::get_str)
            .ok_or_else(|| corrupt(id, "role is not a String"))?
            .to_string();
        let write_generation =
            u64::try_from(int(5)?).map_err(|_| corrupt(id, "negative write_generation"))?;
        if chunk_size == 0 || size_bytes % u64::from(chunk_size) != 0 {
            return Err(corrupt(id, "size is not a whole number of chunks"));
        }
        Ok(Some(BlockDevice {
            id: id.to_string(),
            size_bytes,
            block_size,
            chunk_size,
            created_at,
            role,
            write_generation,
        }))
    }

    /// Read chunk `idx` into `out` (exactly one chunk long). Returns `true` if
    /// the chunk is stored, `false` if it is sparse (and `out` is zeroed).
    /// A stored chunk whose CBLK CRC does not match is an error, never data.
    pub fn read_block(
        &self,
        device: &BlockDevice,
        idx: u64,
        out: &mut [u8],
    ) -> Result<bool, CozoError> {
        let params = block_key(device, idx, Some(out.len()))?;
        let rows = self.run(
            "?[blob] := *blocks{device: $device, idx: $idx, blob}",
            params,
            ScriptMutability::Immutable,
        )?;
        let Some(row) = rows.rows.first() else {
            out.fill(0);
            return Ok(false);
        };
        let Some(blob) = row.first().and_then(DataValue::get_bytes) else {
            return Err(CozoError::Database(format!(
                "block {}/{idx} blob column is not Bytes",
                device.id
            )));
        };
        let chunk = cblk::decode(blob)?;
        if chunk.device_id != device.id || chunk.header.chunk_idx != idx {
            return Err(CozoError::Database(format!(
                "block {}/{idx} CBLK identity mismatch (device={}, idx={})",
                device.id, chunk.device_id, chunk.header.chunk_idx
            )));
        }
        if chunk.header.is_tombstone() {
            return Err(CozoError::Database(format!(
                "block {}/{idx} holds a tombstone envelope in the blocks relation",
                device.id
            )));
        }
        if chunk.payload.len() != out.len() {
            return Err(CozoError::Database(format!(
                "block {}/{idx} payload length {}",
                device.id,
                chunk.payload.len()
            )));
        }
        out.copy_from_slice(&chunk.payload);
        Ok(true)
    }

    /// Store chunk `idx` (exactly one chunk long). An all-zero chunk is a
    /// discard: the data row is removed and a tombstone left. Payload bytes are
    /// opaque. The envelope (CRC32C, zstd when it shrinks the chunk) and the
    /// generation bump commit in one transaction.
    pub fn write_block(
        &self,
        device: &BlockDevice,
        idx: u64,
        data: &[u8],
    ) -> Result<(), CozoError> {
        let mut params = block_key(device, idx, Some(data.len()))?;
        if data.iter().all(|byte| *byte == 0) {
            return self.discard_block(device, idx);
        }
        // Serialize envelope allocation + transaction commit, so concurrent
        // writers cannot persist a smaller generation after a larger one.
        let _registry = self
            .registry
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let generation = self.next_generation(&device.id)?;
        let writes = {
            let mut w = self
                .writes_since_open
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            *w = w.saturating_add(1);
            *w
        };
        let blob = cblk::encode(
            &device.id,
            idx,
            generation,
            data,
            should_inline_schema(writes),
        )?;
        params.insert("blob".into(), DataValue::Bytes(blob));
        params.insert("generation".into(), generation_value(generation)?);
        self.run(
            &format!(
                "{{ ?[device, idx, blob] <- [[$device, $idx, $blob]] \
                 :put blocks {{ device, idx => blob }} }} \
                 {{ ?[device, idx] <- [[$device, $idx]] :rm tombstones {{ device, idx }} }} \
                 {BUMP_GENERATION}"
            ),
            params,
            ScriptMutability::Mutable,
        )?;
        Ok(())
    }

    /// Discard chunk `idx` so it reads as zeros (discard / write-zeroes). A
    /// chunk that holds data is replaced by a tombstone; a sparse chunk is a
    /// no-op (it has no data version a raw scan could resurrect).
    pub fn discard_block(&self, device: &BlockDevice, idx: u64) -> Result<(), CozoError> {
        let params = block_key(device, idx, None)?;
        // Serialize envelope allocation + transaction commit, so concurrent
        // writers cannot persist a smaller generation after a larger one.
        let _registry = self
            .registry
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let rows = self.run(
            "?[i] := i = $idx, *blocks{device: $device, idx: i}",
            params,
            ScriptMutability::Immutable,
        )?;
        if rows.rows.is_empty() {
            return Ok(());
        }
        self.tombstone_batch(device, &[idx])
    }

    /// Discard every stored chunk in `lo..hi`. Visits only existing rows, so
    /// discarding a whole sparse device is cheap; each batch of up to
    /// [`DISCARD_BATCH`] chunks is one transaction with its own generation.
    pub fn discard_blocks(&self, device: &BlockDevice, lo: u64, hi: u64) -> Result<(), CozoError> {
        if lo >= hi {
            return Ok(());
        }
        if hi > device.chunk_count() {
            return Err(invalid(&format!(
                "chunk range {lo}..{hi} is outside device {} ({} chunks)",
                device.id,
                device.chunk_count()
            )));
        }
        // Serialize envelope allocation + transaction commit, so concurrent
        // writers cannot persist a smaller generation after a larger one.
        let _registry = self
            .registry
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let rows = self.run(
            "?[idx] := *blocks{device, idx}, device = $device, idx >= $lo, idx < $hi",
            Params::from([
                ("device".into(), DataValue::from(device.id.as_str())),
                ("lo".into(), DataValue::from(lo as i64)),
                ("hi".into(), DataValue::from(hi as i64)),
            ]),
            ScriptMutability::Immutable,
        )?;
        let mut idxs = Vec::with_capacity(rows.rows.len());
        for row in &rows.rows {
            let n = row
                .first()
                .and_then(DataValue::get_int)
                .ok_or_else(|| corrupt(&device.id, "blocks idx is not an Int"))?;
            idxs.push(u64::try_from(n).map_err(|_| corrupt(&device.id, "negative idx"))?);
        }
        for batch in idxs.chunks(DISCARD_BATCH) {
            self.tombstone_batch(device, batch)?;
        }
        Ok(())
    }

    /// One transaction: put a tombstone for each chunk, drop its data row, and
    /// advance the device generation.
    fn tombstone_batch(&self, device: &BlockDevice, idxs: &[u64]) -> Result<(), CozoError> {
        if idxs.is_empty() {
            return Ok(());
        }
        let generation = self.next_generation(&device.id)?;
        let mut tombs = Vec::with_capacity(idxs.len());
        let mut keys = Vec::with_capacity(idxs.len());
        for &idx in idxs {
            let blob = cblk::encode_tombstone(&device.id, idx, generation)?;
            let idx = DataValue::from(
                i64::try_from(idx).map_err(|_| invalid("chunk index overflows i64"))?,
            );
            let dev = DataValue::from(device.id.as_str());
            tombs.push(DataValue::List(vec![
                dev.clone(),
                idx.clone(),
                DataValue::Bytes(blob),
            ]));
            keys.push(DataValue::List(vec![dev, idx]));
        }
        let params = Params::from([
            ("device".into(), DataValue::from(device.id.as_str())),
            ("tombs".into(), DataValue::List(tombs)),
            ("keys".into(), DataValue::List(keys)),
            ("generation".into(), generation_value(generation)?),
        ]);
        self.run(
            &format!(
                "{{ ?[device, idx, blob] <- $tombs :put tombstones {{ device, idx => blob }} }} \
                 {{ ?[device, idx] <- $keys :rm blocks {{ device, idx }} }} \
                 {BUMP_GENERATION}"
            ),
            params,
            ScriptMutability::Mutable,
        )?;
        Ok(())
    }

    /// Durability barrier: every write that returned before this call is on
    /// stable storage when it returns. Flushes the whole host filesystem that
    /// holds the database (and its root directory's, if different).
    pub fn flush(&self) -> Result<(), CozoError> {
        self.directory.sync()
    }

    /// Number of stored (non-sparse) chunks of one device.
    pub fn block_count(&self, device: &str) -> Result<u64, CozoError> {
        let rows = self.run(
            "?[count(idx)] := *blocks{device: $device, idx}",
            Params::from([("device".into(), DataValue::from(device))]),
            ScriptMutability::Immutable,
        )?;
        first_count(&rows)
    }

    /// Row counts of `devices` and `blocks`.
    pub fn counts(&self) -> Result<BlockStoreCounts, CozoError> {
        let devices = first_count(&self.run(
            "?[count(id)] := *devices{id}",
            Params::new(),
            ScriptMutability::Immutable,
        )?)?;
        // Group by device: idx is unique within a device, so each group's
        // count is exact, and the groups sum to the relation's row count.
        let per_device = self.run(
            "?[device, count(idx)] := *blocks{device, idx}",
            Params::new(),
            ScriptMutability::Immutable,
        )?;
        let mut blocks = 0u64;
        for row in &per_device.rows {
            let n = row.get(1).and_then(DataValue::get_int).unwrap_or(0);
            blocks += u64::try_from(n).unwrap_or(0);
        }
        let tombstones = first_count(&self.run(
            "?[count(idx)] := *tombstones{idx}",
            Params::new(),
            ScriptMutability::Immutable,
        )?)?;
        Ok(BlockStoreCounts {
            devices,
            blocks,
            tombstones,
        })
    }

    /// Current in-memory generation for `device` (tests / diagnostics).
    pub fn current_generation(&self, device: &str) -> u64 {
        self.generations
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(device)
            .copied()
            .unwrap_or(0)
    }

    fn next_generation(&self, device: &str) -> Result<u64, CozoError> {
        let mut gens = self
            .generations
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let entry = gens.entry(device.to_string()).or_insert(0);
        *entry = entry.saturating_add(1);
        Ok(*entry)
    }

    fn load_generations(&self) -> Result<(), CozoError> {
        let rows = self.run(
            "?[id, write_generation] := *devices{id, write_generation}",
            Params::new(),
            ScriptMutability::Immutable,
        )?;
        let mut gens = self
            .generations
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for row in &rows.rows {
            let Some(id) = row.first().and_then(DataValue::get_str) else {
                continue;
            };
            let write_gen = row
                .get(1)
                .and_then(DataValue::get_int)
                .and_then(|n| u64::try_from(n).ok())
                .unwrap_or(0);
            let entry = gens.entry(id.to_string()).or_insert(0);
            *entry = (*entry).max(write_gen);
        }
        Ok(())
    }

    fn ensure_schema(&self) -> Result<(), CozoError> {
        for (name, create, expected) in RELATIONS {
            if let Err(e) = self
                .db
                .run_script(create, Params::new(), ScriptMutability::Mutable)
            {
                let msg = e.to_string();
                if !(msg.contains("already exists")
                    || msg.contains("AlreadyExists")
                    || msg.contains("conflicts with an existing"))
                {
                    return Err(CozoError::Database(format!("create {name}: {msg}")));
                }
            }
            let columns = self.run(
                &format!("::columns {name}"),
                Params::new(),
                ScriptMutability::Immutable,
            )?;
            let found: Vec<&str> = columns
                .rows
                .iter()
                .filter_map(|row| row.first().and_then(DataValue::get_str))
                .collect();
            if found != expected {
                return Err(CozoError::Database(format!(
                    "relation {name} has columns {found:?}, expected {expected:?}"
                )));
            }
        }
        Ok(())
    }

    fn run(
        &self,
        script: &str,
        params: Params,
        mutability: ScriptMutability,
    ) -> Result<NamedRows, CozoError> {
        self.db
            .run_script(script, params, mutability)
            .map_err(|e| CozoError::Database(e.to_string()))
    }
}

fn block_key(device: &BlockDevice, idx: u64, len: Option<usize>) -> Result<Params, CozoError> {
    if idx >= device.chunk_count() {
        return Err(invalid(&format!(
            "chunk {idx} is outside device {} ({} chunks)",
            device.id,
            device.chunk_count()
        )));
    }
    if let Some(len) = len
        && len != device.chunk_size as usize
    {
        return Err(invalid(&format!(
            "buffer of {len} bytes for device {} with {}-byte chunks",
            device.id, device.chunk_size
        )));
    }
    Ok(Params::from([
        ("device".into(), DataValue::from(device.id.as_str())),
        ("idx".into(), DataValue::from(idx as i64)),
    ]))
}

fn generation_value(generation: u64) -> Result<DataValue, CozoError> {
    i64::try_from(generation)
        .map(DataValue::from)
        .map_err(|_| invalid("write generation overflows i64"))
}

fn first_count(rows: &NamedRows) -> Result<u64, CozoError> {
    // An aggregate over an empty relation may yield no row or a Null row.
    let n = rows
        .rows
        .first()
        .and_then(|row| row.first())
        .and_then(DataValue::get_int)
        .unwrap_or(0);
    u64::try_from(n).map_err(|_| CozoError::Database(format!("negative count {n}")))
}

fn validate_device_id(id: &str) -> Result<(), CozoError> {
    let valid = !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .next()
            .is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    if valid {
        Ok(())
    } else {
        Err(invalid(&format!(
            "device id {id:?} must be 1-64 chars of [a-z0-9-], starting with [a-z0-9]"
        )))
    }
}

fn now_unix() -> Result<i64, CozoError> {
    let since = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| CozoError::Other(format!("clock before Unix epoch: {e}")))?;
    i64::try_from(since.as_secs()).map_err(|_| CozoError::Other("clock overflow".into()))
}

fn invalid(message: &str) -> CozoError {
    CozoError::Other(format!("ublk block store: {message}"))
}

fn corrupt(id: &str, what: &str) -> CozoError {
    CozoError::Database(format!("device {id}: {what}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cblk::{self, CBLK_MAGIC, CblkError};

    const CHUNK: usize = UBLK_CHUNK_BYTES as usize;

    fn store() -> (tempfile::TempDir, CozoBlockStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = CozoBlockStore::open(dir.path().join("ublk-cozo")).unwrap();
        (dir, store)
    }

    /// Arbitrary opaque payload — not an image, not sniffed.
    fn pattern(seed: u8) -> Vec<u8> {
        (0..CHUNK)
            .map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed))
            .collect()
    }

    fn randomish(seed: u64) -> Vec<u8> {
        let mut out = vec![0u8; CHUNK];
        let mut state = seed ^ 0x1234_5678_9abc_def0;
        for (i, byte) in out.iter_mut().enumerate() {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(i as u64 + 1);
            *byte = (state >> 33) as u8;
        }
        out
    }

    #[test]
    fn registration_is_idempotent_and_refuses_changes() {
        let (_dir, store) = store();
        let size = 16 * u64::from(UBLK_CHUNK_BYTES);
        let first = store
            .register_device("dev-a", size, POOL_PERSIST_ROLE)
            .unwrap();
        assert_eq!(first.chunk_count(), 16);
        assert_eq!(first.block_size, UBLK_LOGICAL_BLOCK_BYTES);
        assert_eq!(first.write_generation, 0);
        let again = store
            .register_device("dev-a", size, POOL_PERSIST_ROLE)
            .unwrap();
        assert_eq!(first, again);
        assert!(
            store
                .register_device("dev-a", size * 2, POOL_PERSIST_ROLE)
                .is_err()
        );
        assert!(store.register_device("dev-a", size, "other").is_err());
        assert!(
            store
                .register_device("Bad_Id", size, POOL_PERSIST_ROLE)
                .is_err()
        );
        assert!(
            store
                .register_device("dev-b", size + 1, POOL_PERSIST_ROLE)
                .is_err()
        );
        assert_eq!(
            store.counts().unwrap(),
            BlockStoreCounts {
                devices: 1,
                blocks: 0,
                tombstones: 0
            }
        );
    }

    #[test]
    fn sparse_write_read_discard_round_trip() {
        let (_dir, store) = store();
        let dev = store
            .register_device("dev-a", 8 * u64::from(UBLK_CHUNK_BYTES), POOL_PERSIST_ROLE)
            .unwrap();
        let mut out = vec![0xAA; CHUNK];
        assert!(!store.read_block(&dev, 3, &mut out).unwrap());
        assert!(out.iter().all(|b| *b == 0));

        let data = pattern(7);
        store.write_block(&dev, 3, &data).unwrap();
        store.write_block(&dev, 7, &pattern(9)).unwrap();
        assert!(store.read_block(&dev, 3, &mut out).unwrap());
        assert_eq!(out, data);
        assert_eq!(store.block_count("dev-a").unwrap(), 2);

        store.write_block(&dev, 7, &vec![0; CHUNK]).unwrap();
        assert_eq!(store.block_count("dev-a").unwrap(), 1);
        store.discard_block(&dev, 3).unwrap();
        store.discard_block(&dev, 3).unwrap();
        assert!(!store.read_block(&dev, 3, &mut out).unwrap());
        assert!(out.iter().all(|b| *b == 0));
        assert_eq!(
            store.counts().unwrap(),
            BlockStoreCounts {
                devices: 1,
                blocks: 0,
                tombstones: 2
            }
        );
        store.flush().unwrap();

        for idx in 0..8 {
            store
                .write_block(&dev, idx, &pattern(idx as u8 + 1))
                .unwrap();
        }
        store.discard_blocks(&dev, 2, 6).unwrap();
        assert_eq!(store.block_count("dev-a").unwrap(), 4);
        assert!(store.read_block(&dev, 1, &mut out).unwrap());
        assert!(!store.read_block(&dev, 2, &mut out).unwrap());
        assert!(store.read_block(&dev, 6, &mut out).unwrap());
        assert!(store.discard_blocks(&dev, 0, 9).is_err());
        store.discard_blocks(&dev, 0, 8).unwrap();
        assert_eq!(store.block_count("dev-a").unwrap(), 0);
        assert_eq!(store.counts().unwrap().tombstones, 8);
    }

    #[test]
    fn rejects_out_of_range_and_wrong_length() {
        let (_dir, store) = store();
        let dev = store
            .register_device("dev-a", 4 * u64::from(UBLK_CHUNK_BYTES), POOL_PERSIST_ROLE)
            .unwrap();
        let mut out = vec![0; CHUNK];
        assert!(store.read_block(&dev, 4, &mut out).is_err());
        assert!(store.write_block(&dev, 4, &pattern(1)).is_err());
        assert!(store.discard_block(&dev, 4).is_err());
        assert!(store.write_block(&dev, 0, &[1; 4096]).is_err());
        assert!(store.read_block(&dev, 0, &mut [0; 4096]).is_err());
    }

    #[test]
    fn corrupted_chunk_crc_is_an_error() {
        let (_dir, store) = store();
        let dev = store
            .register_device("dev-a", 4 * u64::from(UBLK_CHUNK_BYTES), POOL_PERSIST_ROLE)
            .unwrap();
        store.write_block(&dev, 1, &randomish(3)).unwrap();
        let rows = store
            .run(
                "?[blob] := *blocks{device: 'dev-a', idx: 1, blob}",
                Params::new(),
                ScriptMutability::Immutable,
            )
            .unwrap();
        let mut blob = rows.rows[0][0].get_bytes().unwrap().to_vec();
        let last = blob.len() - 1;
        blob[last] ^= 0xa5;
        store
            .run(
                "?[device, idx, blob] <- [['dev-a', 1, $blob]] \
                 :put blocks { device, idx => blob }",
                Params::from([("blob".into(), DataValue::Bytes(blob))]),
                ScriptMutability::Mutable,
            )
            .unwrap();
        let mut out = vec![0; CHUNK];
        let err = store.read_block(&dev, 1, &mut out).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("crc32c") || msg.contains("cblk"),
            "unexpected error: {msg}"
        );
    }

    #[test]
    fn data_survives_reopen_and_generation_is_monotonic() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ublk-cozo");
        let first_gen;
        {
            let store = CozoBlockStore::open(&path).unwrap();
            let dev = store
                .register_device("dev-a", 4 * u64::from(UBLK_CHUNK_BYTES), POOL_PERSIST_ROLE)
                .unwrap();
            store.write_block(&dev, 2, &randomish(5)).unwrap();
            store.write_block(&dev, 0, &randomish(6)).unwrap();
            first_gen = store.current_generation("dev-a");
            assert!(first_gen >= 2);
            store.flush().unwrap();
        }
        let store = CozoBlockStore::open(&path).unwrap();
        let dev = store.device("dev-a").unwrap().unwrap();
        assert_eq!(dev.write_generation, first_gen);
        assert_eq!(store.current_generation("dev-a"), first_gen);
        let mut out = vec![0; CHUNK];
        assert!(store.read_block(&dev, 2, &mut out).unwrap());
        assert_eq!(out, randomish(5));
        store.write_block(&dev, 1, &randomish(7)).unwrap();
        assert!(store.current_generation("dev-a") > first_gen);
    }

    #[test]
    fn inline_schema_on_first_and_every_256th_write() {
        let (_dir, store) = store();
        let dev = store
            .register_device(
                "dev-a",
                512 * u64::from(UBLK_CHUNK_BYTES),
                POOL_PERSIST_ROLE,
            )
            .unwrap();
        // Write 256 non-zero chunks; inspect stored blobs for inline schema flag.
        for idx in 0..256u64 {
            store
                .write_block(&dev, idx, &pattern((idx % 251) as u8 + 1))
                .unwrap();
        }
        let rows = store
            .run(
                "?[idx, blob] := *blocks{device: 'dev-a', idx, blob}",
                Params::new(),
                ScriptMutability::Immutable,
            )
            .unwrap();
        let mut by_idx: BTreeMap<u64, Vec<u8>> = BTreeMap::new();
        for row in &rows.rows {
            let idx = row[0].get_int().unwrap() as u64;
            by_idx.insert(idx, row[1].get_bytes().unwrap().to_vec());
        }
        let first = cblk::decode(by_idx.get(&0).unwrap()).unwrap();
        assert!(first.inline_schema.is_some());
        let mid = cblk::decode(by_idx.get(&1).unwrap()).unwrap();
        assert!(mid.inline_schema.is_none());
        let every = cblk::decode(by_idx.get(&255).unwrap()).unwrap();
        assert!(every.inline_schema.is_some());
    }

    fn stored_blob(store: &CozoBlockStore, relation: &str, idx: u64) -> Option<Vec<u8>> {
        let rows = store
            .run(
                &format!("?[blob] := *{relation}{{device: 'dev-a', idx: $idx, blob}}"),
                Params::from([("idx".into(), DataValue::from(idx as i64))]),
                ScriptMutability::Immutable,
            )
            .unwrap();
        rows.rows
            .first()
            .map(|row| row[0].get_bytes().unwrap().to_vec())
    }

    fn persisted_generation(store: &CozoBlockStore) -> u64 {
        let rows = store
            .run(
                "?[g] := *devices{id: 'dev-a', write_generation: g}",
                Params::new(),
                ScriptMutability::Immutable,
            )
            .unwrap();
        rows.rows[0][0].get_int().unwrap() as u64
    }

    #[test]
    fn envelope_and_generation_commit_together() {
        let (_dir, store) = store();
        let dev = store
            .register_device("dev-a", 8 * u64::from(UBLK_CHUNK_BYTES), POOL_PERSIST_ROLE)
            .unwrap();
        for (i, idx) in [3u64, 1, 3, 6].iter().enumerate() {
            store
                .write_block(&dev, *idx, &randomish(i as u64 + 1))
                .unwrap();
            let blob = stored_blob(&store, "blocks", *idx).unwrap();
            let generation = cblk::decode(&blob).unwrap().header.generation;
            // The persisted counter is exactly the generation just stored.
            assert_eq!(persisted_generation(&store), generation);
            assert_eq!(store.current_generation("dev-a"), generation);
        }
        store.discard_block(&dev, 6).unwrap();
        let tomb = stored_blob(&store, "tombstones", 6).unwrap();
        let tomb_gen = cblk::decode(&tomb).unwrap().header.generation;
        assert_eq!(persisted_generation(&store), tomb_gen);

        // Remove the device row behind the store's back: the generation
        // assertion fails, and the WHOLE transaction (envelope put included)
        // must roll back.
        store
            .run(
                "?[id] <- [['dev-a']] :rm devices { id }",
                Params::new(),
                ScriptMutability::Mutable,
            )
            .unwrap();
        assert!(store.write_block(&dev, 5, &randomish(9)).is_err());
        assert!(
            stored_blob(&store, "blocks", 5).is_none(),
            "envelope committed without generation"
        );
        assert!(store.discard_block(&dev, 1).is_err());
        assert!(
            stored_blob(&store, "blocks", 1).is_some(),
            "discard half-applied"
        );
        assert!(
            stored_blob(&store, "tombstones", 1).is_none(),
            "tombstone committed without generation"
        );
    }

    #[test]
    fn discard_leaves_tombstone_and_rewrite_clears_it() {
        let (_dir, store) = store();
        let dev = store
            .register_device("dev-a", 4 * u64::from(UBLK_CHUNK_BYTES), POOL_PERSIST_ROLE)
            .unwrap();
        // Discarding a never-written chunk writes nothing.
        store.discard_block(&dev, 0).unwrap();
        assert_eq!(store.counts().unwrap().tombstones, 0);

        store.write_block(&dev, 2, &randomish(1)).unwrap();
        let data_gen = cblk::decode(&stored_blob(&store, "blocks", 2).unwrap())
            .unwrap()
            .header
            .generation;
        store.write_block(&dev, 2, &vec![0; CHUNK]).unwrap();
        assert!(stored_blob(&store, "blocks", 2).is_none());
        let tomb = cblk::decode(&stored_blob(&store, "tombstones", 2).unwrap()).unwrap();
        assert!(tomb.header.is_tombstone());
        assert!(tomb.header.generation > data_gen);
        let mut out = vec![0xAA; CHUNK];
        assert!(!store.read_block(&dev, 2, &mut out).unwrap());
        assert!(out.iter().all(|b| *b == 0));

        store.write_block(&dev, 2, &randomish(2)).unwrap();
        assert!(stored_blob(&store, "tombstones", 2).is_none());
        assert!(store.read_block(&dev, 2, &mut out).unwrap());
        assert_eq!(out, randomish(2));
    }

    #[test]
    fn recovery_does_not_resurrect_discarded_chunks() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ublk-cozo");
        let size = 4 * u64::from(UBLK_CHUNK_BYTES);
        {
            let store = CozoBlockStore::open(&path).unwrap();
            let dev = store
                .register_device("dev-a", size, POOL_PERSIST_ROLE)
                .unwrap();
            store.write_block(&dev, 0, &randomish(1)).unwrap();
            store.write_block(&dev, 1, &randomish(2)).unwrap();
            store.write_block(&dev, 2, &randomish(3)).unwrap();
            store.discard_block(&dev, 1).unwrap();
            store.write_block(&dev, 2, &vec![0; CHUNK]).unwrap();
            store.write_block(&dev, 3, &randomish(4)).unwrap();
            store.discard_blocks(&dev, 3, 4).unwrap();
            store.write_block(&dev, 3, &randomish(5)).unwrap();
            store.flush().unwrap();
        }
        let (chunks, summary) = cblk::scan_directory(&path).unwrap();
        assert!(summary.tombstones >= 3, "summary={summary:?}");
        let image = dir.path().join("recovered.img");
        let written = cblk::write_sparse_image(&chunks, "dev-a", size, &image).unwrap();
        assert_eq!(written, 2);
        let bytes = std::fs::read(&image).unwrap();
        assert_eq!(&bytes[0..CHUNK], randomish(1).as_slice());
        assert!(
            bytes[CHUNK..3 * CHUNK].iter().all(|b| *b == 0),
            "discarded chunk resurrected"
        );
        assert_eq!(&bytes[3 * CHUNK..], randomish(5).as_slice());
    }

    #[test]
    fn zstd_flagged_chunks_read_back_identically_after_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ublk-cozo");
        // Compressible payload: sparse-ish text-like bytes.
        let mut compressible = vec![0u8; CHUNK];
        for (i, b) in compressible.iter_mut().enumerate() {
            *b = b"opaque device bytes "[i % 20];
        }
        let incompressible = randomish(77);
        {
            let store = CozoBlockStore::open(&path).unwrap();
            let dev = store
                .register_device("dev-a", 4 * u64::from(UBLK_CHUNK_BYTES), POOL_PERSIST_ROLE)
                .unwrap();
            store.write_block(&dev, 0, &compressible).unwrap();
            store.write_block(&dev, 1, &incompressible).unwrap();
            let z = cblk::decode(&stored_blob(&store, "blocks", 0).unwrap()).unwrap();
            assert_ne!(z.header.flags & cblk::CBLK_FLAG_ZSTD, 0);
            assert!(z.header.stored_len < z.header.payload_len);
            let raw = cblk::decode(&stored_blob(&store, "blocks", 1).unwrap()).unwrap();
            assert_eq!(raw.header.flags & cblk::CBLK_FLAG_ZSTD, 0);
            store.flush().unwrap();
        }
        let store = CozoBlockStore::open(&path).unwrap();
        let dev = store.device("dev-a").unwrap().unwrap();
        let mut out = vec![0; CHUNK];
        assert!(store.read_block(&dev, 0, &mut out).unwrap());
        assert_eq!(out, compressible);
        assert!(store.read_block(&dev, 1, &mut out).unwrap());
        assert_eq!(out, incompressible);
    }

    /// The OPTIONS-* file RocksDB writes on open records what it actually
    /// loaded. Every compression knob must be off.
    #[test]
    fn loaded_rocksdb_options_have_no_compression_anywhere() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ublk-cozo");
        {
            let store = CozoBlockStore::open(&path).unwrap();
            let dev = store
                .register_device("dev-a", 4 * u64::from(UBLK_CHUNK_BYTES), POOL_PERSIST_ROLE)
                .unwrap();
            store.write_block(&dev, 0, &randomish(1)).unwrap();
            store.flush().unwrap();
        }
        let mut loaded: Vec<PathBuf> = walkdir_files(&path)
            .into_iter()
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("OPTIONS-"))
            })
            .collect();
        loaded.sort();
        let latest = loaded.last().expect("RocksDB wrote no OPTIONS-* file");
        let text = std::fs::read_to_string(latest).unwrap();
        let value = |key: &str| -> Vec<String> {
            text.lines()
                .map(str::trim)
                .filter_map(|line| line.strip_prefix(key).and_then(|r| r.strip_prefix('=')))
                .map(str::to_string)
                .collect()
        };
        for key in [
            "compression",
            "bottommost_compression",
            "blob_compression_type",
            "wal_compression",
        ] {
            let values = value(key);
            assert!(
                !values.is_empty(),
                "{key} missing from {}",
                latest.display()
            );
            for v in values {
                assert_eq!(v, "kNoCompression", "{key}={v} in {}", latest.display());
            }
        }
        let per_level = value("compression_per_level");
        assert!(!per_level.is_empty(), "compression_per_level missing");
        for v in per_level {
            assert!(
                v.split(':').all(|level| level == "kNoCompression"),
                "compression_per_level={v}"
            );
        }
        for codec in [
            "kSnappyCompression",
            "kZlibCompression",
            "kBZip2Compression",
            "kLZ4Compression",
            "kLZ4HCCompression",
            "kXpressCompression",
            "kZSTD",
        ] {
            assert!(
                !text.contains(codec),
                "{codec} present in {}",
                latest.display()
            );
        }
        assert!(value("enable_blob_files").iter().all(|v| v == "true"));
    }

    #[test]
    fn sst_options_disable_compression() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ublk-cozo");
        let _store = CozoBlockStore::open(&path).unwrap();
        let options = std::fs::read(path.join("options")).unwrap();
        let text = String::from_utf8(options).unwrap();
        assert!(text.contains("compression=kNoCompression"));
        assert!(text.contains("bottommost_compression=kNoCompression"));
        assert!(text.contains("enable_blob_files=true"));
        assert!(text.contains("blob_compression_type=kNoCompression"));
        assert!(text.contains("wal_compression=kNoCompression"));
        assert!(text.contains("compression_per_level=kNoCompression:"));
        assert!(!text.contains("kLZ4Compression"));
        assert!(!text.contains("kZSTD"));
    }

    #[test]
    fn recovery_scan_rebuilds_image_from_data_dir() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ublk-cozo");
        let size = 4 * u64::from(UBLK_CHUNK_BYTES);
        let payloads = [randomish(11), randomish(22), randomish(33)];
        {
            let store = CozoBlockStore::open(&path).unwrap();
            let dev = store
                .register_device("dev-a", size, POOL_PERSIST_ROLE)
                .unwrap();
            store.write_block(&dev, 0, &payloads[0]).unwrap();
            store.write_block(&dev, 2, &payloads[1]).unwrap();
            store.write_block(&dev, 3, &payloads[2]).unwrap();
            // chunk 1 left sparse
            store.flush().unwrap();
        }
        let (chunks, summary) = cblk::scan_directory(&path).unwrap();
        assert!(summary.valid >= 3, "summary={summary:?}");
        assert!(summary.device_ids.contains_key("dev-a"));
        let image = dir.path().join("recovered.img");
        let written = cblk::write_sparse_image(&chunks, "dev-a", size, &image).unwrap();
        assert_eq!(written, 3);
        let bytes = std::fs::read(&image).unwrap();
        assert_eq!(bytes.len(), size as usize);
        assert_eq!(&bytes[0..CHUNK], payloads[0].as_slice());
        assert!(bytes[CHUNK..2 * CHUNK].iter().all(|b| *b == 0));
        assert_eq!(&bytes[2 * CHUNK..3 * CHUNK], payloads[1].as_slice());
        assert_eq!(&bytes[3 * CHUNK..4 * CHUNK], payloads[2].as_slice());
        // Magic must appear in raw store files (SST compression off).
        let mut found_magic = false;
        for entry in walkdir_files(&path) {
            let data = std::fs::read(&entry).unwrap();
            if data.windows(4).any(|w| w == CBLK_MAGIC) {
                found_magic = true;
                break;
            }
        }
        assert!(found_magic, "CBLK magic not found in raw store files");
    }

    #[test]
    fn default_path_and_override() {
        // SAFETY: no other test in this module reads this variable.
        unsafe { std::env::remove_var(UBLK_COZO_DB_PATH_ENV) };
        assert_eq!(
            ublk_cozo_db_path(),
            PathBuf::from(DEFAULT_UBLK_COZO_DB_PATH)
        );
        unsafe { std::env::set_var(UBLK_COZO_DB_PATH_ENV, "/tmp/ublk-override") };
        assert_eq!(ublk_cozo_db_path(), PathBuf::from("/tmp/ublk-override"));
        unsafe { std::env::remove_var(UBLK_COZO_DB_PATH_ENV) };
    }

    fn walkdir_files(root: &Path) -> Vec<PathBuf> {
        let mut out = Vec::new();
        fn walk(path: &Path, out: &mut Vec<PathBuf>) {
            let Ok(meta) = std::fs::metadata(path) else {
                return;
            };
            if meta.is_file() {
                out.push(path.to_path_buf());
                return;
            }
            if meta.is_dir() {
                let Ok(rd) = std::fs::read_dir(path) else {
                    return;
                };
                for entry in rd.flatten() {
                    walk(&entry.path(), out);
                }
            }
        }
        walk(root, &mut out);
        out
    }

    #[test]
    fn encode_error_maps_to_cozo_error() {
        let err: CozoError = CblkError::Format("boom".into()).into();
        assert!(err.to_string().contains("cblk"));
    }
}
