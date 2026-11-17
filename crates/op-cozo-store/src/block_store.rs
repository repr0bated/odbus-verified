//! Sparse, checksummed block store for the ublk device that persists the
//! btrfs container pool.
//!
//! The pool's read-only seed is sprouted by ONE ublk device, added once on the
//! host with `btrfs device add`. That device's bytes live here, in a dedicated
//! Cozo/RocksDB database on the host's ext4 root. It must never sit inside
//! the btrfs pool it backs: the pool's persistence would then depend on itself.
//!
//! Relations:
//! - `devices { id => size_bytes, block_size, chunk_size, created_at, role }`
//! - `blocks { device, idx => data, blake3 }`, where chunk `idx` holds device
//!   bytes `[idx * chunk_size, (idx + 1) * chunk_size)`.
//!
//! Sparse: a chunk with no row reads as zeros, and writing an all-zero chunk
//! deletes its row. A 500 GiB device costs nothing until btrfs writes to it,
//! and discard gives the space back.
//!
//! Durability: Cozo 0.7.6 commits with RocksDB's default `sync = false`, so a
//! returned `write_block` is in the WAL (it survives this process crashing)
//! but not yet on stable storage. [`CozoBlockStore::flush`] is the barrier: the
//! same held-directory `syncfs` that identity commits use. The ublk target must
//! call it for every FLUSH and FUA request before completing that request.
//!
//! Requests smaller than a chunk (a 4 KiB write inside a 64 KiB chunk) are
//! read-modify-write in the ublk target, which serialises I/O per chunk.

use crate::CozoError;
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
pub const UBLK_CHUNK_BYTES: u32 = 64 * 1024;

/// The single persistence device for the container pool.
pub const POOL_DEFAULT_PERSIST_DEVICE: &str = "pool-default-persist";
/// Sparse virtual size of [`POOL_DEFAULT_PERSIST_DEVICE`]: 500 GiB.
pub const POOL_DEFAULT_PERSIST_BYTES: u64 = 500 * 1024 * 1024 * 1024;
/// Role recorded for a device that is `btrfs device add`ed to a pool.
pub const POOL_PERSIST_ROLE: &str = "btrfs-pool-persist";

const RELATIONS: [(&str, &str, &[&str]); 2] = [
    (
        "devices",
        ":create devices { id: String => size_bytes: Int, block_size: Int, chunk_size: Int, created_at: Int, role: String }",
        &["id", "size_bytes", "block_size", "chunk_size", "created_at", "role"],
    ),
    (
        "blocks",
        ":create blocks { device: String, idx: Int => data: Bytes, blake3: Bytes }",
        &["device", "idx", "data", "blake3"],
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
}

impl BlockDevice {
    /// Number of chunks; valid chunk indexes are `0..chunk_count()`.
    pub fn chunk_count(&self) -> u64 {
        self.size_bytes / u64::from(self.chunk_size)
    }
}

/// Row counts of the two relations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockStoreCounts {
    pub devices: u64,
    pub blocks: u64,
}

/// Owner of the ublk block database. RocksDB's lock makes it single-process;
/// the ublk target is that process.
pub struct CozoBlockStore {
    db: DbInstance,
    directory: PersistentIdentityBoundary,
    path: PathBuf,
    registry: Mutex<()>,
}

impl CozoBlockStore {
    /// Open the store at [`ublk_cozo_db_path`].
    pub fn open_default() -> Result<Self, CozoError> {
        Self::open(ublk_cozo_db_path())
    }

    /// Open the store at `path`, creating it if absent. Root and data
    /// directories are created 0700, must be owned by the calling user, and
    /// must keep Cozo's default root/data layout so that [`Self::flush`]
    /// covers the WAL.
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
        let directory = PersistentIdentityBoundary::open_private(&path)?;
        directory.require_default_layout()?;
        let db = DbInstance::new("rocksdb", &text, Default::default())
            .map_err(|e| CozoError::Database(format!("open ublk block store at {text}: {e}")))?;
        directory.verify()?;
        let store = Self {
            db,
            directory,
            path,
            registry: Mutex::new(()),
        };
        store.ensure_schema()?;
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
        if size_bytes == 0 || size_bytes % chunk != 0 || i64::try_from(size_bytes).is_err() {
            return Err(invalid(&format!(
                "device size {size_bytes} must be a positive multiple of {chunk} bytes"
            )));
        }
        let _registry = self.registry.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
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
            ("block_size".into(), DataValue::from(i64::from(UBLK_LOGICAL_BLOCK_BYTES))),
            ("chunk_size".into(), DataValue::from(i64::from(UBLK_CHUNK_BYTES))),
            ("created_at".into(), DataValue::from(now_unix()?)),
            ("role".into(), DataValue::from(role)),
        ]);
        self.run(
            "?[id, size_bytes, block_size, chunk_size, created_at, role] <- \
             [[$id, $size_bytes, $block_size, $chunk_size, $created_at, $role]] \
             :put devices { id => size_bytes, block_size, chunk_size, created_at, role }",
            params,
            ScriptMutability::Mutable,
        )?;
        self.device(id)?
            .ok_or_else(|| CozoError::Database(format!("device {id} missing after registration")))
    }

    /// Look up a registered device.
    pub fn device(&self, id: &str) -> Result<Option<BlockDevice>, CozoError> {
        let rows = self.run(
            "?[size_bytes, block_size, chunk_size, created_at, role] := \
             *devices{id: $id, size_bytes, block_size, chunk_size, created_at, role}",
            Params::from([("id".into(), DataValue::from(id))]),
            ScriptMutability::Immutable,
        )?;
        let Some(row) = rows.rows.first() else {
            return Ok(None);
        };
        let int = |index: usize| {
            row.get(index)
                .and_then(DataValue::get_int)
                .ok_or_else(|| CozoError::Database(format!("device {id}: column {index} is not an Int")))
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
        }))
    }

    /// Read chunk `idx` into `out` (exactly one chunk long). Returns `true` if
    /// the chunk is stored, `false` if it is sparse (and `out` is zeroed).
    /// A stored chunk whose blake3 does not match is an error, never data.
    pub fn read_block(
        &self,
        device: &BlockDevice,
        idx: u64,
        out: &mut [u8],
    ) -> Result<bool, CozoError> {
        let params = block_key(device, idx, Some(out.len()))?;
        let rows = self.run(
            "?[data, blake3] := *blocks{device: $device, idx: $idx, data, blake3}",
            params,
            ScriptMutability::Immutable,
        )?;
        let Some(row) = rows.rows.first() else {
            out.fill(0);
            return Ok(false);
        };
        let data = row.first().and_then(DataValue::get_bytes);
        let digest = row.get(1).and_then(DataValue::get_bytes);
        match (data, digest) {
            (Some(data), Some(digest))
                if data.len() == out.len() && blake3::hash(data).as_bytes().as_slice() == digest =>
            {
                out.copy_from_slice(data);
                Ok(true)
            }
            _ => Err(CozoError::Database(format!(
                "block {}/{idx} failed length or blake3 verification",
                device.id
            ))),
        }
    }

    /// Store chunk `idx` (exactly one chunk long). An all-zero chunk deletes
    /// the row instead, keeping the device sparse.
    pub fn write_block(&self, device: &BlockDevice, idx: u64, data: &[u8]) -> Result<(), CozoError> {
        let mut params = block_key(device, idx, Some(data.len()))?;
        if data.iter().all(|byte| *byte == 0) {
            return self.remove(params);
        }
        params.insert("data".into(), DataValue::Bytes(data.to_vec()));
        params.insert(
            "blake3".into(),
            DataValue::Bytes(blake3::hash(data).as_bytes().to_vec()),
        );
        self.run(
            "?[device, idx, data, blake3] <- [[$device, $idx, $data, $blake3]] \
             :put blocks { device, idx => data, blake3 }",
            params,
            ScriptMutability::Mutable,
        )?;
        Ok(())
    }

    /// Drop chunk `idx` so it reads as zeros (discard / write-zeroes).
    pub fn discard_block(&self, device: &BlockDevice, idx: u64) -> Result<(), CozoError> {
        self.remove(block_key(device, idx, None)?)
    }

    /// Drop every stored chunk in `lo..hi` with one query that visits only
    /// existing rows, so discarding a whole sparse device is cheap.
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
        self.run(
            "?[device, idx] := *blocks{device, idx}, device = $device, idx >= $lo, idx < $hi \
             :rm blocks { device, idx }",
            Params::from([
                ("device".into(), DataValue::from(device.id.as_str())),
                ("lo".into(), DataValue::from(lo as i64)),
                ("hi".into(), DataValue::from(hi as i64)),
            ]),
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
        Ok(BlockStoreCounts { devices, blocks })
    }

    fn remove(&self, params: Params) -> Result<(), CozoError> {
        self.run(
            "?[device, idx] <- [[$device, $idx]] :rm blocks { device, idx }",
            params,
            ScriptMutability::Mutable,
        )?;
        Ok(())
    }

    fn ensure_schema(&self) -> Result<(), CozoError> {
        for (name, create, expected) in RELATIONS {
            if let Err(e) = self.db.run_script(create, Params::new(), ScriptMutability::Mutable) {
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
        && id.bytes().next().is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
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

    const CHUNK: usize = UBLK_CHUNK_BYTES as usize;

    fn store() -> (tempfile::TempDir, CozoBlockStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = CozoBlockStore::open(dir.path().join("ublk-cozo")).unwrap();
        (dir, store)
    }

    fn pattern(seed: u8) -> Vec<u8> {
        (0..CHUNK).map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed)).collect()
    }

    #[test]
    fn registration_is_idempotent_and_refuses_changes() {
        let (_dir, store) = store();
        let size = 16 * u64::from(UBLK_CHUNK_BYTES);
        let first = store.register_device("dev-a", size, POOL_PERSIST_ROLE).unwrap();
        assert_eq!(first.chunk_count(), 16);
        assert_eq!(first.block_size, UBLK_LOGICAL_BLOCK_BYTES);
        let again = store.register_device("dev-a", size, POOL_PERSIST_ROLE).unwrap();
        assert_eq!(first, again);
        assert!(store.register_device("dev-a", size * 2, POOL_PERSIST_ROLE).is_err());
        assert!(store.register_device("dev-a", size, "other").is_err());
        assert!(store.register_device("Bad_Id", size, POOL_PERSIST_ROLE).is_err());
        assert!(store.register_device("dev-b", size + 1, POOL_PERSIST_ROLE).is_err());
        assert_eq!(store.counts().unwrap(), BlockStoreCounts { devices: 1, blocks: 0 });
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
        assert_eq!(store.counts().unwrap(), BlockStoreCounts { devices: 1, blocks: 0 });
        store.flush().unwrap();

        for idx in 0..8 {
            store.write_block(&dev, idx, &pattern(idx as u8 + 1)).unwrap();
        }
        store.discard_blocks(&dev, 2, 6).unwrap();
        assert_eq!(store.block_count("dev-a").unwrap(), 4);
        assert!(store.read_block(&dev, 1, &mut out).unwrap());
        assert!(!store.read_block(&dev, 2, &mut out).unwrap());
        assert!(store.read_block(&dev, 6, &mut out).unwrap());
        assert!(store.discard_blocks(&dev, 0, 9).is_err());
        store.discard_blocks(&dev, 0, 8).unwrap();
        assert_eq!(store.block_count("dev-a").unwrap(), 0);
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
    fn corrupted_chunk_is_an_error() {
        let (_dir, store) = store();
        let dev = store
            .register_device("dev-a", 4 * u64::from(UBLK_CHUNK_BYTES), POOL_PERSIST_ROLE)
            .unwrap();
        store.write_block(&dev, 1, &pattern(3)).unwrap();
        let params = Params::from([
            ("data".into(), DataValue::Bytes(pattern(4))),
            ("blake3".into(), DataValue::Bytes(blake3::hash(&pattern(3)).as_bytes().to_vec())),
        ]);
        store
            .run(
                "?[device, idx, data, blake3] <- [['dev-a', 1, $data, $blake3]] \
                 :put blocks { device, idx => data, blake3 }",
                params,
                ScriptMutability::Mutable,
            )
            .unwrap();
        let mut out = vec![0; CHUNK];
        assert!(store.read_block(&dev, 1, &mut out).is_err());
    }

    #[test]
    fn data_survives_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ublk-cozo");
        {
            let store = CozoBlockStore::open(&path).unwrap();
            let dev = store
                .register_device("dev-a", 4 * u64::from(UBLK_CHUNK_BYTES), POOL_PERSIST_ROLE)
                .unwrap();
            store.write_block(&dev, 2, &pattern(5)).unwrap();
            store.flush().unwrap();
        }
        let store = CozoBlockStore::open(&path).unwrap();
        let dev = store.device("dev-a").unwrap().unwrap();
        let mut out = vec![0; CHUNK];
        assert!(store.read_block(&dev, 2, &mut out).unwrap());
        assert_eq!(out, pattern(5));
    }

    #[test]
    fn default_path_and_override() {
        // SAFETY: no other test in this module reads this variable.
        unsafe { std::env::remove_var(UBLK_COZO_DB_PATH_ENV) };
        assert_eq!(ublk_cozo_db_path(), PathBuf::from(DEFAULT_UBLK_COZO_DB_PATH));
        unsafe { std::env::set_var(UBLK_COZO_DB_PATH_ENV, "/tmp/ublk-override") };
        assert_eq!(ublk_cozo_db_path(), PathBuf::from("/tmp/ublk-override"));
        unsafe { std::env::remove_var(UBLK_COZO_DB_PATH_ENV) };
    }
}
