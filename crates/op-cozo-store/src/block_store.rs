//! Sparse, self-describing block store for the ublk device that persists the
//! btrfs container pool (and any other bytes written through the device).
//!
//! The pool's read-only seed is sprouted by ONE ublk device, added once on the
//! host with `btrfs device add`. That device's bytes live here, in a dedicated
//! store on the host's ext4 root. It must never sit inside the btrfs pool it
//! backs: the pool's persistence would then depend on itself.
//!
//! Two keyspaces share one store root:
//!
//! - The Cozo database (`<root>/manifest`, `<root>/options`, `<root>/data`)
//!   holds the `devices` registry and, for stores written before the raw
//!   keyspace existed, the LEGACY chunk relations:
//!   - `devices { id => size_bytes, block_size, chunk_size, created_at, role, write_generation }`
//!   - `blocks { device, idx => blob }` (CBLK envelope per stored chunk)
//!   - `tombstones { device, idx => blob }` (payload-less CBLK tombstones)
//! - The RAW chunk keyspace (`<root>/cblk-raw`, a plain RocksDB opened through
//!   cozorocks with its own `<root>/cblk-raw.options`) serves every chunk read
//!   and write on the hot path with direct get / put / one write batch per
//!   commit, instead of a parsed and planned Datalog script per request:
//!   - `B | len(device) | device | idx (u64 BE)` => CBLK envelope (data or tombstone)
//!   - `G | device` => last committed generation (u64 LE)
//!   - `S` => durability-barrier counter
//!
//! Compatibility: a chunk with no raw key is read through to the legacy
//! `blocks` relation, so an existing store keeps every byte readable without a
//! migration. Legacy rows are never modified or deleted by the hot path; a
//! newer raw envelope (data or tombstone) simply shadows them. A raw tombstone
//! is written whenever a chunk that holds data (raw OR legacy) is discarded or
//! overwritten with zeros, so a discarded legacy chunk never reappears.
//! [`CozoBlockStore::migrate_legacy_blocks`] (`ublk-block-store migrate`) can
//! copy legacy envelopes into the raw keyspace offline; it never deletes them.
//!
//! Generations: every commit (a write batch, or a discard batch) allocates ONE
//! fresh per-device generation, stamps it into every envelope of the batch,
//! and writes `G|device` in the SAME RocksDB write batch, so the persisted
//! generation is never behind a stored envelope. Generations continue above
//! the legacy `devices.write_generation`, so offline raw-scan recovery (which
//! picks the highest generation per chunk across WAL/SST/blob bytes of both
//! keyspaces) prefers raw versions over lingering legacy ones.
//!
//! The block layer is content-agnostic: every chunk index is encoded the same
//! way. There is no special case for container images, filesystem metadata, or
//! user data.
//!
//! Sparse: a chunk with no row reads as zeros, and writing an all-zero chunk
//! that held data leaves a tombstone. A 500 GiB device costs nothing until
//! btrfs writes to it, and discard gives the space back.
//!
//! Durability: commits use RocksDB's `sync = false` unless the caller asks
//! for a synced batch, so a returned write is in the raw WAL (it survives this
//! process crashing) but not necessarily on stable storage.
//! [`CozoBlockStore::flush`] is the barrier: one synced RocksDB write, which
//! fdatasyncs the raw WAL (and every earlier unsynced WAL record with it). It
//! no longer `syncfs`es the whole host filesystem or re-walks the store path.
//!
//! RocksDB compression is disabled at every level in both keyspaces so raw
//! `CBLK` magic stays findable for offline recovery. The only compression is
//! zstd inside each CBLK envelope.
//!
//! Requests smaller than a chunk (a 4 KiB write inside a 64 KiB chunk) are
//! read-modify-written by the ublk target in its chunk cache.

use crate::CozoError;
use crate::cblk::{self, CBLK_CHUNK_SIZE, should_inline_schema};
use crate::identity_transactions::PersistentIdentityBoundary;
use cozo::{DataValue, DbInstance, NamedRows, ScriptMutability};
use cozorocks::{DbBuilder, RocksDb, RocksDbStatus};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{Read, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

type Params = BTreeMap<String, DataValue>;

/// Default database directory: the host ext4 root, beside the other op-dbus
/// Cozo stores, never under `/var/lib/incus/storage-pools`.
pub const DEFAULT_UBLK_COZO_DB_PATH: &str = "/var/lib/op-dbus/ublk-cozo";
/// Overrides [`DEFAULT_UBLK_COZO_DB_PATH`] when set and non-empty.
pub const UBLK_COZO_DB_PATH_ENV: &str = "OP_UBLK_COZO_DB_PATH";

/// Logical block size the ublk device advertises (`logical_bs_shift = 12`).
pub const UBLK_LOGICAL_BLOCK_BYTES: u32 = 4096;

/// Storage granularity: one stored envelope per 64 KiB of device space.
///
/// - btrfs writes 4 KiB data sectors and 16 KiB metadata nodes. Any write
///   smaller than a chunk is a read-modify-write of the whole chunk; the ublk
///   target's chunk cache absorbs that so repeated small writes to one chunk
///   cost one envelope per writeback, not one per request.
/// - Larger rows mean fewer keys per sequential request: a 512 KiB request
///   (the I/O buffer) is 8 keys here, 128 at 4 KiB.
/// - A fully written 500 GiB device is 8,192,000 keys; 4 KiB chunks would be
///   131,072,000 keys, each paying key and index overhead.
/// - Discard frees whole chunks only, so smaller chunks return more of what
///   btrfs trims.
pub const UBLK_CHUNK_BYTES: u32 = CBLK_CHUNK_SIZE;

/// The single persistence device for the container pool.
pub const POOL_DEFAULT_PERSIST_DEVICE: &str = "pool-default-persist";
/// Sparse virtual size of [`POOL_DEFAULT_PERSIST_DEVICE`]: 500 GiB.
pub const POOL_DEFAULT_PERSIST_BYTES: u64 = 500 * 1024 * 1024 * 1024;
/// Role recorded for a device that is `btrfs device add`ed to a pool.
pub const POOL_PERSIST_ROLE: &str = "btrfs-pool-persist";

/// Chunks discarded per transaction by [`CozoBlockStore::discard_blocks`].
const DISCARD_BATCH: usize = 1024;
/// Legacy chunk indexes scanned per query by [`CozoBlockStore::migrate_legacy_blocks`].
const MIGRATE_RANGE: u64 = 4096;

/// Raw keyspace directory (a RocksDB) under the store root.
const RAW_DIR: &str = "cblk-raw";
/// Options file for [`RAW_DIR`], beside it under the store root.
const RAW_OPTIONS_FILE: &str = "cblk-raw.options";
const RAW_BLOCK_PREFIX: u8 = b'B';
const RAW_GENERATION_PREFIX: u8 = b'G';
const RAW_BARRIER_KEY: &[u8] = b"S";

/// RocksDB options for the raw chunk keyspace. Compression is OFF at every
/// layer (SST, every level, bottommost, blob files, WAL) so raw `CBLK` magic
/// stays findable; envelopes are already zstd-compressed. Values of 4 KiB or
/// more live in blob files, and blob GC reclaims superseded chunk versions.
/// `info_log_level=INFO_LEVEL` keeps the RocksDB LOG quiet.
const RAW_ROCKSDB_OPTIONS: &[u8] = b"\
[Version]\n\
options_file_version=1.1\n\
[DBOptions]\n\
info_log_level=INFO_LEVEL\n\
max_log_file_size=16777216\n\
keep_log_file_num=5\n\
allow_fallocate=false\n\
max_background_jobs=6\n\
bytes_per_sync=1048576\n\
wal_compression=kNoCompression\n\
[CFOptions \"default\"]\n\
compression=kNoCompression\n\
bottommost_compression=kNoCompression\n\
compression_per_level=kNoCompression:kNoCompression:kNoCompression:kNoCompression:kNoCompression:kNoCompression:kNoCompression\n\
write_buffer_size=67108864\n\
enable_blob_files=true\n\
min_blob_size=4096\n\
blob_compression_type=kNoCompression\n\
enable_blob_garbage_collection=true\n\
level_compaction_dynamic_level_bytes=true\n\
compaction_pri=kMinOverlappingRatio\n";

/// Keys an existing Cozo root `options` file may carry and still be opened
/// for the legacy keyspace. None of them redirects data or WAL locations.
const LEGACY_OPTION_KEYS: &[&str] = &[
    "options_file_version",
    "info_log_level",
    "max_log_file_size",
    "keep_log_file_num",
    "allow_fallocate",
    "max_background_jobs",
    "bytes_per_sync",
    "wal_compression",
    "compression",
    "bottommost_compression",
    "compression_per_level",
    "enable_blob_files",
    "min_blob_size",
    "blob_compression_type",
    "enable_blob_garbage_collection",
    "level_compaction_dynamic_level_bytes",
    "compaction_pri",
];

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

/// Row counts of the relations (raw keyspace merged over legacy rows).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockStoreCounts {
    pub devices: u64,
    pub blocks: u64,
    pub tombstones: u64,
}

/// Owner of the ublk block database. RocksDB's locks make it single-process;
/// the ublk target is that process. Safe to share between queue threads.
pub struct CozoBlockStore {
    db: DbInstance,
    raw: RocksDb,
    /// Held store root, for the one-off legacy sync after registration.
    root: File,
    path: PathBuf,
    /// Serialises generation allocation + commit (one lock per BATCH, not per
    /// request). Maps device id to its last committed generation.
    generations: Mutex<BTreeMap<String, u64>>,
    /// Envelopes encoded since this process opened the store (inline-schema cadence).
    writes_since_open: AtomicU64,
    /// Durability-barrier counter value written by [`Self::flush`].
    barriers: AtomicU64,
}

impl CozoBlockStore {
    /// Open the store at [`ublk_cozo_db_path`].
    pub fn open_default() -> Result<Self, CozoError> {
        Self::open(ublk_cozo_db_path())
    }

    /// Open the store at `path`, creating it if absent. Root and data
    /// directories are 0700 and owned by the calling user; the store must
    /// live on ext4 (never btrfs). An existing store whose root `options`
    /// file predates the current profile is accepted when it carries only
    /// known keys and no compression (see [`LEGACY_OPTION_KEYS`]).
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
        open_legacy_layout(&path)?;
        let root = open_private_dir(&path)?;
        let _data = open_private_dir(&path.join("data"))?;
        guard_backing_filesystem(&path)?;
        let db = DbInstance::new("rocksdb", &text, Default::default())
            .map_err(|e| CozoError::Database(format!("open ublk block store at {text}: {e}")))?;
        same_directory(&path, &root)?;
        let raw = open_raw(&path, &root)?;
        let store = Self {
            db,
            raw,
            root,
            path,
            generations: Mutex::new(BTreeMap::new()),
            writes_since_open: AtomicU64::new(0),
            barriers: AtomicU64::new(0),
        };
        store.ensure_schema()?;
        store.load_generations()?;
        Ok(store)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Directory of the raw chunk keyspace.
    pub fn raw_path(&self) -> PathBuf {
        self.path.join(RAW_DIR)
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
        let mut gens = self.lock_generations();
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
        // Registration is the only write left in the Cozo keyspace and is
        // rare (setup tooling), so it keeps the filesystem-wide barrier.
        nix::unistd::syncfs(std::os::fd::AsRawFd::as_raw_fd(&self.root))
            .map_err(|e| CozoError::Database(format!("syncfs {}: {e}", self.path.display())))?;
        let raw_generation = self.raw_generation(id)?;
        gens.entry(id.to_string()).or_insert(raw_generation);
        drop(gens);
        self.device(id)?
            .ok_or_else(|| CozoError::Database(format!("device {id} missing after registration")))
    }

    /// Look up a registered device. `write_generation` is the larger of the
    /// legacy row's value and the raw keyspace's committed generation.
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
        let legacy_generation =
            u64::try_from(int(5)?).map_err(|_| corrupt(id, "negative write_generation"))?;
        if chunk_size == 0 || size_bytes % u64::from(chunk_size) != 0 {
            return Err(corrupt(id, "size is not a whole number of chunks"));
        }
        let write_generation = legacy_generation.max(self.raw_generation(id)?);
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
    /// the chunk is stored, `false` if it is sparse or discarded (and `out`
    /// is zeroed). The raw keyspace answers first; a chunk it has never seen
    /// is read through to the legacy `blocks` relation. A stored chunk whose
    /// CBLK CRC does not match is an error, never data.
    pub fn read_block(
        &self,
        device: &BlockDevice,
        idx: u64,
        out: &mut [u8],
    ) -> Result<bool, CozoError> {
        check_chunk(device, idx, Some(out.len()))?;
        if let Some(blob) = self.raw_get(&raw_block_key(&device.id, idx))? {
            let chunk = decode_envelope(device, idx, &blob)?;
            if chunk.header.is_tombstone() {
                out.fill(0);
                return Ok(false);
            }
            copy_payload(device, idx, &chunk.payload, out)?;
            return Ok(true);
        }
        self.legacy_read(device, idx, out)
    }

    /// Store chunk `idx` (exactly one chunk long) in its own commit. See
    /// [`Self::write_blocks`].
    pub fn write_block(
        &self,
        device: &BlockDevice,
        idx: u64,
        data: &[u8],
    ) -> Result<(), CozoError> {
        self.write_blocks(device, &[(idx, data)], false)
    }

    /// Store many chunks in ONE RocksDB write batch with ONE fresh generation.
    /// Each payload is exactly one chunk; an all-zero chunk that currently
    /// holds data (raw or legacy) becomes a tombstone, and an all-zero chunk
    /// that is already sparse writes nothing. With `sync`, the batch is a
    /// durability barrier: it and every earlier commit are on stable storage
    /// when this returns. Nothing is committed if any entry is invalid.
    pub fn write_blocks(
        &self,
        device: &BlockDevice,
        chunks: &[(u64, &[u8])],
        sync: bool,
    ) -> Result<(), CozoError> {
        for (idx, data) in chunks {
            check_chunk(device, *idx, Some(data.len()))?;
        }
        let mut gens = self.lock_generations();
        let generation = next_generation(&gens, &device.id)?;
        let mut tx = self.raw.transact().sync(sync).start();
        let mut puts = 0usize;
        for (idx, data) in chunks {
            let key = raw_block_key(&device.id, *idx);
            let blob = if data.iter().all(|byte| *byte == 0) {
                let current = tx.get(&key, false).map_err(raw_err)?;
                let has_data = match current {
                    Some(blob) => !cblk::parse_verified(&blob[..])?.header.is_tombstone(),
                    None => self.legacy_exists(device, *idx)?,
                };
                if !has_data {
                    continue;
                }
                cblk::encode_tombstone(&device.id, *idx, generation)?
            } else {
                let writes = self.writes_since_open.fetch_add(1, Ordering::Relaxed) + 1;
                cblk::encode(
                    &device.id,
                    *idx,
                    generation,
                    data,
                    should_inline_schema(writes),
                )?
            };
            tx.put(&key, &blob).map_err(raw_err)?;
            puts += 1;
        }
        if puts == 0 {
            drop(tx);
            return if sync { self.barrier() } else { Ok(()) };
        }
        tx.put(&raw_generation_key(&device.id), &generation.to_le_bytes())
            .map_err(raw_err)?;
        tx.commit().map_err(raw_err)?;
        gens.insert(device.id.clone(), generation);
        Ok(())
    }

    /// Discard chunk `idx` so it reads as zeros (discard / write-zeroes). A
    /// chunk that holds data is replaced by a tombstone; a sparse chunk is a
    /// no-op (it has no data version a raw scan could resurrect).
    pub fn discard_block(&self, device: &BlockDevice, idx: u64) -> Result<(), CozoError> {
        check_chunk(device, idx, None)?;
        self.discard_blocks(device, idx, idx + 1)
    }

    /// Discard every stored chunk in `lo..hi`. Visits only existing keys and
    /// rows, so discarding a whole sparse device is cheap; each batch of up to
    /// [`DISCARD_BATCH`] chunks is one commit with its own generation.
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
        let mut gens = self.lock_generations();
        let raw = self.raw_range(&device.id, lo, hi)?;
        let mut targets: BTreeSet<u64> = raw
            .iter()
            .filter(|(_, is_data)| **is_data)
            .map(|(idx, _)| *idx)
            .collect();
        for idx in self.legacy_indexes(&device.id, lo, hi)? {
            if !raw.contains_key(&idx) {
                targets.insert(idx);
            }
        }
        let targets: Vec<u64> = targets.into_iter().collect();
        for batch in targets.chunks(DISCARD_BATCH) {
            let generation = next_generation(&gens, &device.id)?;
            let mut tx = self.raw.transact().start();
            for &idx in batch {
                let blob = cblk::encode_tombstone(&device.id, idx, generation)?;
                tx.put(&raw_block_key(&device.id, idx), &blob)
                    .map_err(raw_err)?;
            }
            tx.put(&raw_generation_key(&device.id), &generation.to_le_bytes())
                .map_err(raw_err)?;
            tx.commit().map_err(raw_err)?;
            gens.insert(device.id.clone(), generation);
        }
        Ok(())
    }

    /// Durability barrier: every commit that returned before this call is on
    /// stable storage when it returns. One synced RocksDB write fdatasyncs the
    /// raw WAL; no filesystem-wide `syncfs`, no per-flush path re-validation.
    pub fn flush(&self) -> Result<(), CozoError> {
        let _gens = self.lock_generations();
        self.barrier()
    }

    /// Copy legacy `blocks` envelopes that have no raw key into the raw
    /// keyspace, unchanged (same bytes, same generation, CRC verified first).
    /// Offline, one-time, optional: read-through already serves legacy chunks.
    /// Never deletes or rewrites a legacy row and never overwrites a raw key.
    /// Returns `(copied, already_raw)`.
    pub fn migrate_legacy_blocks(&self, device: &BlockDevice) -> Result<(u64, u64), CozoError> {
        let _gens = self.lock_generations();
        let mut copied = 0u64;
        let mut already = 0u64;
        let mut lo = 0u64;
        while lo < device.chunk_count() {
            let hi = (lo + MIGRATE_RANGE).min(device.chunk_count());
            let rows = self.run(
                "?[idx, blob] := *blocks{device, idx, blob}, device = $device, idx >= $lo, idx < $hi",
                Params::from([
                    ("device".into(), DataValue::from(device.id.as_str())),
                    ("lo".into(), DataValue::from(lo as i64)),
                    ("hi".into(), DataValue::from(hi as i64)),
                ]),
                ScriptMutability::Immutable,
            )?;
            if !rows.rows.is_empty() {
                let mut tx = self.raw.transact().start();
                let mut puts = 0usize;
                for row in &rows.rows {
                    let idx = row
                        .first()
                        .and_then(DataValue::get_int)
                        .and_then(|n| u64::try_from(n).ok())
                        .ok_or_else(|| corrupt(&device.id, "blocks idx is not a valid Int"))?;
                    let blob = row.get(1).and_then(DataValue::get_bytes).ok_or_else(|| {
                        corrupt(&device.id, "blocks blob column is not Bytes")
                    })?;
                    let key = raw_block_key(&device.id, idx);
                    if tx.get(&key, false).map_err(raw_err)?.is_some() {
                        already += 1;
                        continue;
                    }
                    let chunk = decode_envelope(device, idx, blob)?;
                    if chunk.header.is_tombstone() {
                        return Err(corrupt(
                            &device.id,
                            &format!("legacy blocks row {idx} holds a tombstone"),
                        ));
                    }
                    tx.put(&key, blob).map_err(raw_err)?;
                    puts += 1;
                    copied += 1;
                }
                if puts > 0 {
                    tx.commit().map_err(raw_err)?;
                }
            }
            lo = hi;
        }
        self.barrier()?;
        Ok((copied, already))
    }

    /// Number of stored (non-sparse, non-discarded) chunks of one device.
    pub fn block_count(&self, device: &str) -> Result<u64, CozoError> {
        let (blocks, _) = self.merged_counts(device)?;
        Ok(blocks)
    }

    /// Device rows, plus stored chunks and tombstones with raw keys shadowing
    /// legacy rows of the same chunk.
    pub fn counts(&self) -> Result<BlockStoreCounts, CozoError> {
        let rows = self.run(
            "?[id] := *devices{id}",
            Params::new(),
            ScriptMutability::Immutable,
        )?;
        let mut blocks = 0u64;
        let mut tombstones = 0u64;
        for row in &rows.rows {
            let Some(id) = row.first().and_then(DataValue::get_str) else {
                continue;
            };
            let (b, t) = self.merged_counts(id)?;
            blocks += b;
            tombstones += t;
        }
        Ok(BlockStoreCounts {
            devices: rows.rows.len() as u64,
            blocks,
            tombstones,
        })
    }

    /// Current in-memory generation for `device` (tests / diagnostics).
    pub fn current_generation(&self, device: &str) -> u64 {
        self.lock_generations().get(device).copied().unwrap_or(0)
    }

    fn lock_generations(&self) -> std::sync::MutexGuard<'_, BTreeMap<String, u64>> {
        self.generations
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// One synced write: fsyncs the raw WAL including every earlier record.
    fn barrier(&self) -> Result<(), CozoError> {
        let n = self.barriers.fetch_add(1, Ordering::Relaxed) + 1;
        let mut tx = self.raw.transact().sync(true).start();
        tx.put(RAW_BARRIER_KEY, &n.to_le_bytes()).map_err(raw_err)?;
        tx.commit().map_err(raw_err)
    }

    fn raw_get(&self, key: &[u8]) -> Result<Option<Vec<u8>>, CozoError> {
        let tx = self.raw.transact().start();
        let value = tx.get(key, false).map_err(raw_err)?;
        Ok(value.map(|slice| slice.to_vec()))
    }

    fn raw_generation(&self, device: &str) -> Result<u64, CozoError> {
        match self.raw_get(&raw_generation_key(device))? {
            None => Ok(0),
            Some(bytes) => {
                let array: [u8; 8] = bytes
                    .as_slice()
                    .try_into()
                    .map_err(|_| corrupt(device, "raw generation is not 8 bytes"))?;
                Ok(u64::from_le_bytes(array))
            }
        }
    }

    /// Raw keys of `device` in `lo..hi`: chunk index => holds data (not a tombstone).
    fn raw_range(&self, device: &str, lo: u64, hi: u64) -> Result<BTreeMap<u64, bool>, CozoError> {
        let lower = raw_block_key(device, lo);
        let upper = raw_block_key(device, hi);
        let tx = self.raw.transact().start();
        let mut iter = tx
            .iterator()
            .lower_bound(&lower)
            .upper_bound(&upper)
            .start();
        iter.seek(&lower);
        let mut out = BTreeMap::new();
        loop {
            let (idx, is_data) = match iter.pair().map_err(raw_err)? {
                None => break,
                Some((key, _)) if key >= upper.as_slice() => break,
                Some((key, value)) => {
                    let idx = raw_block_idx(device, key)
                        .ok_or_else(|| corrupt(device, "malformed raw block key"))?;
                    (idx, !cblk::parse_verified(value)?.header.is_tombstone())
                }
            };
            out.insert(idx, is_data);
            iter.next();
        }
        drop(iter);
        drop(tx);
        Ok(out)
    }

    /// `(stored, tombstones)` for one device across both keyspaces.
    fn merged_counts(&self, device: &str) -> Result<(u64, u64), CozoError> {
        let raw = self.raw_range(device, 0, u64::MAX)?;
        let mut blocks = raw.values().filter(|is_data| **is_data).count() as u64;
        let mut tombstones = raw.len() as u64 - blocks;
        for idx in self.legacy_indexes(device, 0, i64::MAX as u64)? {
            if !raw.contains_key(&idx) {
                blocks += 1;
            }
        }
        let rows = self.run(
            "?[idx] := *tombstones{device, idx}, device = $device",
            Params::from([("device".into(), DataValue::from(device))]),
            ScriptMutability::Immutable,
        )?;
        for row in &rows.rows {
            let idx = row
                .first()
                .and_then(DataValue::get_int)
                .and_then(|n| u64::try_from(n).ok())
                .ok_or_else(|| corrupt(device, "tombstones idx is not a valid Int"))?;
            if !raw.contains_key(&idx) {
                tombstones += 1;
            }
        }
        Ok((blocks, tombstones))
    }

    fn legacy_read(
        &self,
        device: &BlockDevice,
        idx: u64,
        out: &mut [u8],
    ) -> Result<bool, CozoError> {
        let rows = self.run(
            "?[blob] := *blocks{device: $device, idx: $idx, blob}",
            legacy_key(device, idx),
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
        let chunk = decode_envelope(device, idx, blob)?;
        if chunk.header.is_tombstone() {
            return Err(CozoError::Database(format!(
                "block {}/{idx} holds a tombstone envelope in the blocks relation",
                device.id
            )));
        }
        copy_payload(device, idx, &chunk.payload, out)?;
        Ok(true)
    }

    fn legacy_exists(&self, device: &BlockDevice, idx: u64) -> Result<bool, CozoError> {
        let rows = self.run(
            "?[i] := i = $idx, *blocks{device: $device, idx: i}",
            legacy_key(device, idx),
            ScriptMutability::Immutable,
        )?;
        Ok(!rows.rows.is_empty())
    }

    fn legacy_indexes(&self, device: &str, lo: u64, hi: u64) -> Result<Vec<u64>, CozoError> {
        let hi = hi.min(i64::MAX as u64);
        let rows = self.run(
            "?[idx] := *blocks{device, idx}, device = $device, idx >= $lo, idx < $hi",
            Params::from([
                ("device".into(), DataValue::from(device)),
                ("lo".into(), DataValue::from(lo.min(i64::MAX as u64) as i64)),
                ("hi".into(), DataValue::from(hi as i64)),
            ]),
            ScriptMutability::Immutable,
        )?;
        let mut idxs = Vec::with_capacity(rows.rows.len());
        for row in &rows.rows {
            let n = row
                .first()
                .and_then(DataValue::get_int)
                .ok_or_else(|| corrupt(device, "blocks idx is not an Int"))?;
            idxs.push(u64::try_from(n).map_err(|_| corrupt(device, "negative idx"))?);
        }
        Ok(idxs)
    }

    fn load_generations(&self) -> Result<(), CozoError> {
        let rows = self.run(
            "?[id, write_generation] := *devices{id, write_generation}",
            Params::new(),
            ScriptMutability::Immutable,
        )?;
        let mut loaded = Vec::new();
        for row in &rows.rows {
            let Some(id) = row.first().and_then(DataValue::get_str) else {
                continue;
            };
            let legacy = row
                .get(1)
                .and_then(DataValue::get_int)
                .and_then(|n| u64::try_from(n).ok())
                .unwrap_or(0);
            loaded.push((id.to_string(), legacy.max(self.raw_generation(id)?)));
        }
        let mut gens = self.lock_generations();
        for (id, generation) in loaded {
            let entry = gens.entry(id).or_insert(0);
            *entry = (*entry).max(generation);
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

/// Create a fresh store with the current CBLK profile, or accept an existing
/// one: either it matches the current profile exactly, or its root `options`
/// is an older profile that only carries [`LEGACY_OPTION_KEYS`] with every
/// compression knob off (so data and WAL stay in the default `data/` layout).
fn open_legacy_layout(path: &Path) -> Result<(), CozoError> {
    match PersistentIdentityBoundary::open_private_ublk(path) {
        Ok(boundary) => boundary.require_default_layout(),
        Err(error) => match legacy_options_acceptable(&path.join("options")) {
            Ok(true) => Ok(()),
            Ok(false) => Err(error),
            Err(check) => Err(CozoError::Database(format!("{error}; {check}"))),
        },
    }
}

fn legacy_options_acceptable(file: &Path) -> Result<bool, CozoError> {
    let mut handle = match std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc_o_nofollow())
        .open(file)
    {
        Ok(handle) => handle,
        Err(_) => return Ok(false),
    };
    let meta = handle.metadata().map_err(io_err(file))?;
    if !meta.is_file()
        || meta.uid() != nix::unistd::geteuid().as_raw()
        || meta.mode() & 0o022 != 0
        || meta.nlink() != 1
        || meta.len() > 64 * 1024
    {
        return Ok(false);
    }
    let mut text = String::new();
    handle.read_to_string(&mut text).map_err(io_err(file))?;
    let mut sections = 0;
    for line in text.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') {
            if !matches!(line, "[Version]" | "[DBOptions]" | "[CFOptions \"default\"]") {
                return Ok(false);
            }
            sections += 1;
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Ok(false);
        };
        let (key, value) = (key.trim(), value.trim());
        if !LEGACY_OPTION_KEYS.contains(&key) {
            return Ok(false);
        }
        if key.contains("compression") && !value.split(':').all(|v| v == "kNoCompression") {
            return Ok(false);
        }
    }
    Ok(sections == 3)
}

fn libc_o_nofollow() -> i32 {
    nix::fcntl::OFlag::O_NOFOLLOW.bits()
}

fn io_err(path: &Path) -> impl Fn(std::io::Error) -> CozoError + '_ {
    move |e| CozoError::Database(format!("ublk block store {}: {e}", path.display()))
}

/// Open a directory that must already exist, be owned by us and be 0700.
fn open_private_dir(path: &Path) -> Result<File, CozoError> {
    let dir = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(nix::fcntl::OFlag::O_DIRECTORY.bits() | libc_o_nofollow())
        .open(path)
        .map_err(io_err(path))?;
    let meta = dir.metadata().map_err(io_err(path))?;
    if !meta.is_dir()
        || meta.uid() != nix::unistd::geteuid().as_raw()
        || meta.mode() & 0o7777 != 0o700
    {
        return Err(invalid(&format!(
            "{} must be a 0700 directory owned by the store user",
            path.display()
        )));
    }
    Ok(dir)
}

/// The store must never live on the btrfs pool it backs; in production it
/// must be on ext4 (the runit service enforces the same).
fn guard_backing_filesystem(path: &Path) -> Result<(), CozoError> {
    let fs = nix::sys::statfs::statfs(path)
        .map_err(|e| CozoError::Database(format!("statfs {}: {e}", path.display())))?;
    let kind = fs.filesystem_type();
    if kind == nix::sys::statfs::BTRFS_SUPER_MAGIC {
        return Err(invalid(&format!(
            "{} is on btrfs; the ublk block store must not live on the pool it backs",
            path.display()
        )));
    }
    if !cfg!(test) && kind != nix::sys::statfs::EXT4_SUPER_MAGIC {
        return Err(invalid(&format!("{} must be on ext4", path.display())));
    }
    Ok(())
}

fn same_directory(path: &Path, held: &File) -> Result<(), CozoError> {
    let now = std::fs::symlink_metadata(path).map_err(io_err(path))?;
    let held = held.metadata().map_err(io_err(path))?;
    if now.dev() != held.dev() || now.ino() != held.ino() {
        return Err(invalid("store directory changed while opening"));
    }
    Ok(())
}

/// Open (creating on first use) the raw chunk keyspace under the store root.
fn open_raw(root_path: &Path, root: &File) -> Result<RocksDb, CozoError> {
    let dir = root_path.join(RAW_DIR);
    let options = root_path.join(RAW_OPTIONS_FILE);
    let mut created = false;
    match std::fs::DirBuilder::new().mode(0o700).create(&dir) {
        Ok(()) => created = true,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(io_err(&dir)(e)),
    }
    open_private_dir(&dir)?;
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc_o_nofollow())
        .open(&options)
    {
        Ok(mut file) => {
            file.write_all(RAW_ROCKSDB_OPTIONS).map_err(io_err(&options))?;
            file.sync_all().map_err(io_err(&options))?;
            created = true;
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(io_err(&options)(e)),
    }
    if created {
        root.sync_all().map_err(io_err(root_path))?;
    }
    let mut existing = Vec::new();
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc_o_nofollow())
        .open(&options)
        .map_err(io_err(&options))?;
    let meta = file.metadata().map_err(io_err(&options))?;
    if !meta.is_file()
        || meta.uid() != nix::unistd::geteuid().as_raw()
        || meta.mode() & 0o022 != 0
        || meta.nlink() != 1
    {
        return Err(invalid(&format!("{} has unsafe ownership or mode", options.display())));
    }
    file.read_to_end(&mut existing).map_err(io_err(&options))?;
    if existing != RAW_ROCKSDB_OPTIONS {
        return Err(invalid(&format!(
            "{} differs from the raw keyspace profile; refusing unverified data/WAL locations",
            options.display()
        )));
    }
    DbBuilder::default()
        .path(&dir)
        .options_path(&options)
        .create_if_missing(true)
        .build()
        .map_err(raw_err)
}

fn raw_err(status: RocksDbStatus) -> CozoError {
    CozoError::Database(format!(
        "ublk raw store: {:?}/{:?}: {}",
        status.code, status.subcode, status.message
    ))
}

fn raw_block_key(device: &str, idx: u64) -> Vec<u8> {
    let mut key = Vec::with_capacity(2 + device.len() + 8);
    key.push(RAW_BLOCK_PREFIX);
    key.push(device.len() as u8);
    key.extend_from_slice(device.as_bytes());
    key.extend_from_slice(&idx.to_be_bytes());
    key
}

fn raw_block_idx(device: &str, key: &[u8]) -> Option<u64> {
    let head = 2 + device.len();
    if key.len() != head + 8
        || key[0] != RAW_BLOCK_PREFIX
        || usize::from(key[1]) != device.len()
        || &key[2..head] != device.as_bytes()
    {
        return None;
    }
    Some(u64::from_be_bytes(key[head..].try_into().ok()?))
}

fn raw_generation_key(device: &str) -> Vec<u8> {
    let mut key = Vec::with_capacity(1 + device.len());
    key.push(RAW_GENERATION_PREFIX);
    key.extend_from_slice(device.as_bytes());
    key
}

fn next_generation(gens: &BTreeMap<String, u64>, device: &str) -> Result<u64, CozoError> {
    let next = gens
        .get(device)
        .copied()
        .unwrap_or(0)
        .checked_add(1)
        .ok_or_else(|| invalid("write generation overflow"))?;
    if i64::try_from(next).is_err() {
        return Err(invalid("write generation overflows i64"));
    }
    Ok(next)
}

/// Decode a CBLK envelope and check it belongs to `device`/`idx`.
fn decode_envelope(
    device: &BlockDevice,
    idx: u64,
    blob: &[u8],
) -> Result<cblk::CblkChunk, CozoError> {
    let chunk = cblk::decode(blob)?;
    if chunk.device_id != device.id || chunk.header.chunk_idx != idx {
        return Err(CozoError::Database(format!(
            "block {}/{idx} CBLK identity mismatch (device={}, idx={})",
            device.id, chunk.device_id, chunk.header.chunk_idx
        )));
    }
    Ok(chunk)
}

fn copy_payload(
    device: &BlockDevice,
    idx: u64,
    payload: &[u8],
    out: &mut [u8],
) -> Result<(), CozoError> {
    if payload.len() != out.len() {
        return Err(CozoError::Database(format!(
            "block {}/{idx} payload length {}",
            device.id,
            payload.len()
        )));
    }
    out.copy_from_slice(payload);
    Ok(())
}

fn check_chunk(device: &BlockDevice, idx: u64, len: Option<usize>) -> Result<(), CozoError> {
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
    Ok(())
}

fn legacy_key(device: &BlockDevice, idx: u64) -> Params {
    Params::from([
        ("device".into(), DataValue::from(device.id.as_str())),
        ("idx".into(), DataValue::from(idx as i64)),
    ])
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
        let mut blob = raw_blob(&store, 1).unwrap();
        let last = blob.len() - 1;
        blob[last] ^= 0xa5;
        raw_put(&store, 1, &blob);
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
        let mut by_idx: BTreeMap<u64, Vec<u8>> = BTreeMap::new();
        for idx in 0..256u64 {
            by_idx.insert(idx, raw_blob(&store, idx).unwrap());
        }
        let first = cblk::decode(by_idx.get(&0).unwrap()).unwrap();
        assert!(first.inline_schema.is_some());
        let mid = cblk::decode(by_idx.get(&1).unwrap()).unwrap();
        assert!(mid.inline_schema.is_none());
        let every = cblk::decode(by_idx.get(&255).unwrap()).unwrap();
        assert!(every.inline_schema.is_some());
    }

    /// Raw-keyspace envelope of chunk `idx` of `dev-a`.
    fn raw_blob(store: &CozoBlockStore, idx: u64) -> Option<Vec<u8>> {
        store.raw_get(&raw_block_key("dev-a", idx)).unwrap()
    }

    fn raw_put(store: &CozoBlockStore, idx: u64, blob: &[u8]) {
        let mut tx = store.raw.transact().start();
        tx.put(&raw_block_key("dev-a", idx), blob).unwrap();
        tx.commit().unwrap();
    }

    fn persisted_generation(store: &CozoBlockStore) -> u64 {
        store.raw_generation("dev-a").unwrap()
    }

    fn legacy_put(store: &CozoBlockStore, idx: u64, blob: Vec<u8>) {
        store
            .run(
                "?[device, idx, blob] <- [['dev-a', $idx, $blob]] \
                 :put blocks { device, idx => blob }",
                Params::from([
                    ("idx".into(), DataValue::from(idx as i64)),
                    ("blob".into(), DataValue::Bytes(blob)),
                ]),
                ScriptMutability::Mutable,
            )
            .unwrap();
    }

    fn legacy_blob(store: &CozoBlockStore, idx: u64) -> Option<Vec<u8>> {
        let rows = store
            .run(
                "?[blob] := *blocks{device: 'dev-a', idx: $idx, blob}",
                Params::from([("idx".into(), DataValue::from(idx as i64))]),
                ScriptMutability::Immutable,
            )
            .unwrap();
        rows.rows
            .first()
            .map(|row| row[0].get_bytes().unwrap().to_vec())
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
            let blob = raw_blob(&store, *idx).unwrap();
            let generation = cblk::decode(&blob).unwrap().header.generation;
            // The persisted counter is exactly the generation just stored.
            assert_eq!(persisted_generation(&store), generation);
            assert_eq!(store.current_generation("dev-a"), generation);
        }
        store.discard_block(&dev, 6).unwrap();
        let tomb = cblk::decode(&raw_blob(&store, 6).unwrap()).unwrap();
        assert!(tomb.header.is_tombstone());
        assert_eq!(persisted_generation(&store), tomb.header.generation);

        // One batch, one generation shared by every envelope in it.
        let before = persisted_generation(&store);
        let (a, b) = (randomish(10), randomish(11));
        store
            .write_blocks(&dev, &[(0, a.as_slice()), (2, b.as_slice())], true)
            .unwrap();
        let g0 = cblk::decode(&raw_blob(&store, 0).unwrap()).unwrap().header.generation;
        let g2 = cblk::decode(&raw_blob(&store, 2).unwrap()).unwrap().header.generation;
        assert_eq!(g0, before + 1);
        assert_eq!(g2, g0);
        assert_eq!(persisted_generation(&store), g0);

        // An invalid entry rejects the WHOLE batch: nothing is committed.
        let c = randomish(12);
        assert!(
            store
                .write_blocks(&dev, &[(5, c.as_slice()), (8, c.as_slice())], false)
                .is_err()
        );
        assert!(raw_blob(&store, 5).is_none(), "batch half-applied");
        assert_eq!(persisted_generation(&store), g0);
    }

    #[test]
    fn discard_leaves_tombstone_and_rewrite_clears_it() {
        let (_dir, store) = store();
        let dev = store
            .register_device("dev-a", 4 * u64::from(UBLK_CHUNK_BYTES), POOL_PERSIST_ROLE)
            .unwrap();
        // Discarding (or zero-writing) a never-written chunk writes nothing.
        store.discard_block(&dev, 0).unwrap();
        store.write_block(&dev, 1, &vec![0; CHUNK]).unwrap();
        assert_eq!(store.counts().unwrap().tombstones, 0);
        assert!(raw_blob(&store, 0).is_none());
        assert!(raw_blob(&store, 1).is_none());

        store.write_block(&dev, 2, &randomish(1)).unwrap();
        let data_gen = cblk::decode(&raw_blob(&store, 2).unwrap())
            .unwrap()
            .header
            .generation;
        store.write_block(&dev, 2, &vec![0; CHUNK]).unwrap();
        let tomb = cblk::decode(&raw_blob(&store, 2).unwrap()).unwrap();
        assert!(tomb.header.is_tombstone());
        assert!(tomb.header.generation > data_gen);
        let mut out = vec![0xAA; CHUNK];
        assert!(!store.read_block(&dev, 2, &mut out).unwrap());
        assert!(out.iter().all(|b| *b == 0));

        store.write_block(&dev, 2, &randomish(2)).unwrap();
        let again = cblk::decode(&raw_blob(&store, 2).unwrap()).unwrap();
        assert!(!again.header.is_tombstone());
        assert!(store.read_block(&dev, 2, &mut out).unwrap());
        assert_eq!(out, randomish(2));
    }

    #[test]
    fn legacy_chunks_read_through_and_are_shadowed_not_deleted() {
        let (_dir, store) = store();
        let dev = store
            .register_device("dev-a", 8 * u64::from(UBLK_CHUNK_BYTES), POOL_PERSIST_ROLE)
            .unwrap();
        // Chunks written by the pre-raw store live only in Cozo `blocks`.
        let old3 = cblk::encode("dev-a", 3, 5, &randomish(30), false).unwrap();
        let old4 = cblk::encode("dev-a", 4, 6, &randomish(40), false).unwrap();
        legacy_put(&store, 3, old3.clone());
        legacy_put(&store, 4, old4.clone());
        let mut out = vec![0; CHUNK];
        assert!(store.read_block(&dev, 3, &mut out).unwrap());
        assert_eq!(out, randomish(30));
        assert_eq!(store.block_count("dev-a").unwrap(), 2);

        // A raw write shadows the legacy row; the legacy row is untouched.
        store.write_block(&dev, 4, &randomish(41)).unwrap();
        assert!(store.read_block(&dev, 4, &mut out).unwrap());
        assert_eq!(out, randomish(41));
        assert_eq!(legacy_blob(&store, 4).unwrap(), old4);

        // Discarding a legacy-only chunk leaves a raw tombstone over it.
        store.discard_block(&dev, 3).unwrap();
        assert!(!store.read_block(&dev, 3, &mut out).unwrap());
        assert!(out.iter().all(|b| *b == 0));
        assert_eq!(legacy_blob(&store, 3).unwrap(), old3);
        // Zero-writing a legacy-only chunk does the same.
        let old5 = cblk::encode("dev-a", 5, 7, &randomish(50), false).unwrap();
        legacy_put(&store, 5, old5);
        store.write_block(&dev, 5, &vec![0; CHUNK]).unwrap();
        assert!(!store.read_block(&dev, 5, &mut out).unwrap());
        assert_eq!(
            store.counts().unwrap(),
            BlockStoreCounts {
                devices: 1,
                blocks: 1,
                tombstones: 2
            }
        );
        // New generations continue above the legacy ones.
        assert!(store.current_generation("dev-a") >= 1);
    }

    #[test]
    fn migrate_copies_legacy_envelopes_without_deleting_them() {
        let (_dir, store) = store();
        let dev = store
            .register_device("dev-a", 8 * u64::from(UBLK_CHUNK_BYTES), POOL_PERSIST_ROLE)
            .unwrap();
        let old1 = cblk::encode("dev-a", 1, 3, &randomish(1), false).unwrap();
        let old2 = cblk::encode("dev-a", 2, 4, &randomish(2), false).unwrap();
        legacy_put(&store, 1, old1.clone());
        legacy_put(&store, 2, old2.clone());
        store.write_block(&dev, 2, &randomish(22)).unwrap();
        assert_eq!(store.migrate_legacy_blocks(&dev).unwrap(), (1, 1));
        assert_eq!(raw_blob(&store, 1).unwrap(), old1);
        assert_eq!(legacy_blob(&store, 1).unwrap(), old1);
        assert_eq!(legacy_blob(&store, 2).unwrap(), old2);
        let mut out = vec![0; CHUNK];
        assert!(store.read_block(&dev, 2, &mut out).unwrap());
        assert_eq!(out, randomish(22), "migration overwrote a newer raw chunk");
        assert_eq!(store.migrate_legacy_blocks(&dev).unwrap(), (0, 2));
    }

    #[test]
    fn older_compression_free_options_profile_is_accepted() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ublk-cozo");
        {
            let store = CozoBlockStore::open(&path).unwrap();
            store
                .register_device("dev-a", 4 * u64::from(UBLK_CHUNK_BYTES), POOL_PERSIST_ROLE)
                .unwrap();
        }
        // The profile the live 500 GiB store was created with.
        let older = "[Version]\noptions_file_version=1.1\n[DBOptions]\ninfo_log_level=DEBUG_LEVEL\n\
max_log_file_size=16777216\nkeep_log_file_num=5\nallow_fallocate=false\nmax_background_jobs=6\n\
bytes_per_sync=1048576\n[CFOptions \"default\"]\nbottommost_compression=kNoCompression\n\
compression=kNoCompression\nenable_blob_files=true\nmin_blob_size=4096\n\
blob_compression_type=kNoCompression\nlevel_compaction_dynamic_level_bytes=true\n\
compaction_pri=kMinOverlappingRatio\n";
        std::fs::write(path.join("options"), older).unwrap();
        assert!(CozoBlockStore::open(&path).is_ok());
        // Anything that could move data or WAL, or compress, is refused.
        std::fs::write(path.join("options"), format!("{older}wal_dir=/elsewhere\n")).unwrap();
        assert!(CozoBlockStore::open(&path).is_err());
        std::fs::write(
            path.join("options"),
            older.replace("compression=kNoCompression\nenable", "compression=kLZ4Compression\nenable"),
        )
        .unwrap();
        assert!(CozoBlockStore::open(&path).is_err());
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
            let z = cblk::decode(&raw_blob(&store, 0).unwrap()).unwrap();
            assert_ne!(z.header.flags & cblk::CBLK_FLAG_ZSTD, 0);
            assert!(z.header.stored_len < z.header.payload_len);
            let raw = cblk::decode(&raw_blob(&store, 1).unwrap()).unwrap();
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
        assert!(
            loaded.iter().any(|p| p.starts_with(path.join(RAW_DIR))),
            "raw keyspace wrote no OPTIONS-* file"
        );
        for latest in &loaded {
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
        let raw = std::fs::read_to_string(path.join(RAW_OPTIONS_FILE)).unwrap();
        assert!(raw.contains("info_log_level=INFO_LEVEL"));
        assert!(raw.contains("compression_per_level=kNoCompression:"));
        assert!(!raw.contains("kLZ4Compression") && !raw.contains("kZSTD"));
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
