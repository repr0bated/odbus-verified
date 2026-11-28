//! Self-describing CBLK chunk format (Variant B) for the ublk Cozo block store.
//!
//! Every stored chunk — filesystem metadata, user files, personal data, or any
//! other bytes the ublk device carries — is wrapped identically. The block
//! layer does not sniff content or special-case image data.
//!
//! Layout (all multi-byte integers little-endian):
//!
//! ```text
//! offset  size  field
//! 0       4     magic = b"CBLK"
//! 4       2     version = 1
//! 6       2     flags (bit0 = zstd payload, bit1 = inline schema present,
//!               bit2 = tombstone: chunk discarded, no payload)
//! 8       2     header_len (byte offset from blob start to stored payload)
//! 10      2     device_id_len
//! 12      4     schema_id
//! 16      8     schema_hash (FNV-1a 64 of the schema description)
//! 24      8     chunk_idx
//! 32      8     generation (monotonic per device)
//! 40      4     chunk_size (= 65536)
//! 44      4     payload_len (uncompressed)
//! 48      4     stored_len
//! 52      4     crc32c (header with this field zeroed + device_id +
//!               optional inline schema + stored payload)
//! 56      …     device_id bytes
//! …       …     optional inline schema description
//! header_len …  stored payload (raw or zstd; empty for a tombstone)
//! ```
//!
//! Integrity is CRC32C only; no blake3 anywhere in the chunk path. Compression
//! happens only here (zstd level 1 per chunk, flagged in the header, raw when
//! zstd would not shrink it); the RocksDB options profile disables compression
//! at every level so stored bytes are exactly these envelopes.
//!
//! Tombstones: a discard or all-zero write leaves a payload-less envelope with
//! a fresh generation, so a raw scan picks "discarded" over any older data
//! version still lingering in WAL/SST bytes instead of resurrecting it.

use std::path::Path;
use std::{collections::BTreeMap, fs, io};

/// On-disk magic.
pub const CBLK_MAGIC: &[u8; 4] = b"CBLK";
/// Format version.
pub const CBLK_VERSION: u16 = 1;
/// Current schema identifier.
pub const CBLK_SCHEMA_ID: u32 = 1;
/// Logical chunk size carried in every header.
pub const CBLK_CHUNK_SIZE: u32 = 64 * 1024;
/// Fixed header size including the CRC field.
pub const CBLK_FIXED_HEADER_LEN: usize = 56;
/// zstd level for chunk payloads. Level 1 keeps the per-chunk encode cost low
/// on the ublk write path; decode is level-independent, so chunks written at
/// any earlier level stay readable.
pub const CBLK_ZSTD_LEVEL: i32 = 1;
/// Inline the schema description on the first write after open and every Nth write.
pub const CBLK_SCHEMA_INLINE_INTERVAL: u64 = 256;

/// Flag: stored payload is zstd-compressed.
pub const CBLK_FLAG_ZSTD: u16 = 1 << 0;
/// Flag: inline schema description follows the device id.
pub const CBLK_FLAG_INLINE_SCHEMA: u16 = 1 << 1;
/// Flag: tombstone (chunk discarded); `payload_len == stored_len == 0`.
pub const CBLK_FLAG_TOMBSTONE: u16 = 1 << 2;
const CBLK_KNOWN_FLAGS: u16 = CBLK_FLAG_ZSTD | CBLK_FLAG_INLINE_SCHEMA | CBLK_FLAG_TOMBSTONE;

/// Canonical schema description hashed into `schema_hash` and optionally inlined.
///
/// Content-agnostic: describes the chunk envelope, not any guest filesystem.
pub const CBLK_SCHEMA_DESCRIPTION: &[u8] = b"cblk/1;chunk_size=65536;checksum=crc32c;payload=raw|zstd;relation=blocks{device,idx=>blob};tombstones=tombstones{device,idx=>blob};content=opaque";

/// Encode/decode/CRC failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CblkError {
    Io(String),
    Format(String),
    CrcMismatch { expected: u32, computed: u32 },
    Decompress(String),
}

impl std::fmt::Display for CblkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(msg) | Self::Format(msg) | Self::Decompress(msg) => write!(f, "{msg}"),
            Self::CrcMismatch { expected, computed } => {
                write!(
                    f,
                    "cblk crc32c mismatch: stored={expected:#x} computed={computed:#x}"
                )
            }
        }
    }
}

impl std::error::Error for CblkError {}

impl From<CblkError> for crate::CozoError {
    fn from(value: CblkError) -> Self {
        crate::CozoError::Database(format!("cblk: {value}"))
    }
}

/// Parsed CBLK header and trailing regions (payload still stored-encoded).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CblkHeader {
    pub version: u16,
    pub flags: u16,
    pub header_len: u16,
    pub device_id_len: u16,
    pub schema_id: u32,
    pub schema_hash: [u8; 8],
    pub chunk_idx: u64,
    pub generation: u64,
    pub chunk_size: u32,
    pub payload_len: u32,
    pub stored_len: u32,
    pub crc32c: u32,
}

/// Fully decoded chunk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CblkChunk {
    pub header: CblkHeader,
    pub device_id: String,
    pub inline_schema: Option<Vec<u8>>,
    /// Uncompressed payload (`chunk_size` bytes).
    pub payload: Vec<u8>,
}

impl CblkHeader {
    /// Whether this envelope is a discard tombstone (no payload).
    pub fn is_tombstone(&self) -> bool {
        self.flags & CBLK_FLAG_TOMBSTONE != 0
    }
}

/// FNV-1a 64 of [`CBLK_SCHEMA_DESCRIPTION`] (an identifier, not an integrity
/// check: integrity is the CRC32C over the whole envelope).
pub fn schema_hash() -> [u8; 8] {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in CBLK_SCHEMA_DESCRIPTION {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash.to_le_bytes()
}

/// Whether this write should inline the schema description.
pub fn should_inline_schema(writes_since_open: u64) -> bool {
    writes_since_open == 1 || writes_since_open.is_multiple_of(CBLK_SCHEMA_INLINE_INTERVAL)
}

/// Encode one opaque chunk into a CBLK blob.
pub fn encode(
    device_id: &str,
    chunk_idx: u64,
    generation: u64,
    payload: &[u8],
    inline_schema: bool,
) -> Result<Vec<u8>, CblkError> {
    if payload.len() != CBLK_CHUNK_SIZE as usize {
        return Err(CblkError::Format(format!(
            "payload length {} != chunk_size {CBLK_CHUNK_SIZE}",
            payload.len()
        )));
    }
    if device_id.len() > u16::MAX as usize {
        return Err(CblkError::Format("device id too long".into()));
    }

    let mut flags = 0u16;
    let mut stored = payload.to_vec();
    if let Ok(compressed) = zstd::bulk::compress(payload, CBLK_ZSTD_LEVEL)
        && compressed.len() < payload.len()
    {
        stored = compressed;
        flags |= CBLK_FLAG_ZSTD;
    }
    seal(
        device_id,
        chunk_idx,
        generation,
        flags,
        payload.len(),
        &stored,
        inline_schema,
    )
}

/// Encode a tombstone for a discarded chunk: no payload, fresh generation.
pub fn encode_tombstone(
    device_id: &str,
    chunk_idx: u64,
    generation: u64,
) -> Result<Vec<u8>, CblkError> {
    if device_id.len() > u16::MAX as usize {
        return Err(CblkError::Format("device id too long".into()));
    }
    seal(
        device_id,
        chunk_idx,
        generation,
        CBLK_FLAG_TOMBSTONE,
        0,
        &[],
        false,
    )
}

fn seal(
    device_id: &str,
    chunk_idx: u64,
    generation: u64,
    mut flags: u16,
    payload_len: usize,
    stored: &[u8],
    inline_schema: bool,
) -> Result<Vec<u8>, CblkError> {
    let schema = if inline_schema {
        flags |= CBLK_FLAG_INLINE_SCHEMA;
        Some(CBLK_SCHEMA_DESCRIPTION)
    } else {
        None
    };
    let schema_len = schema.map(|s| s.len()).unwrap_or(0);
    let header_len = CBLK_FIXED_HEADER_LEN + device_id.len() + schema_len;
    if header_len > u16::MAX as usize || stored.len() > u32::MAX as usize {
        return Err(CblkError::Format("cblk blob too large".into()));
    }

    let mut blob = vec![0u8; header_len + stored.len()];
    blob[0..4].copy_from_slice(CBLK_MAGIC);
    write_u16(&mut blob, 4, CBLK_VERSION);
    write_u16(&mut blob, 6, flags);
    write_u16(&mut blob, 8, header_len as u16);
    write_u16(&mut blob, 10, device_id.len() as u16);
    write_u32(&mut blob, 12, CBLK_SCHEMA_ID);
    blob[16..24].copy_from_slice(&schema_hash());
    write_u64(&mut blob, 24, chunk_idx);
    write_u64(&mut blob, 32, generation);
    write_u32(&mut blob, 40, CBLK_CHUNK_SIZE);
    write_u32(&mut blob, 44, payload_len as u32);
    write_u32(&mut blob, 48, stored.len() as u32);
    // crc field left zero for the digest pass
    let device_off = CBLK_FIXED_HEADER_LEN;
    blob[device_off..device_off + device_id.len()].copy_from_slice(device_id.as_bytes());
    if let Some(schema) = schema {
        let schema_off = device_off + device_id.len();
        blob[schema_off..schema_off + schema.len()].copy_from_slice(schema);
    }
    blob[header_len..].copy_from_slice(stored);

    let crc = crc32c::crc32c(&blob);
    write_u32(&mut blob, 52, crc);
    Ok(blob)
}

/// Decode and CRC-verify a CBLK blob, returning the uncompressed payload.
pub fn decode(blob: &[u8]) -> Result<CblkChunk, CblkError> {
    let parts = parse_parts(blob)?;
    if parts.header.is_tombstone() {
        if parts.header.payload_len != 0
            || parts.header.stored_len != 0
            || parts.header.flags & CBLK_FLAG_ZSTD != 0
            || parts.header.chunk_size != CBLK_CHUNK_SIZE
        {
            return Err(CblkError::Format("malformed cblk tombstone".into()));
        }
        return Ok(CblkChunk {
            header: parts.header,
            device_id: parts.device_id,
            inline_schema: parts.inline_schema,
            payload: Vec::new(),
        });
    }
    let payload = if parts.header.flags & CBLK_FLAG_ZSTD != 0 {
        zstd::bulk::decompress(parts.stored, parts.header.payload_len as usize)
            .map_err(|e| CblkError::Decompress(format!("zstd decompress failed: {e}")))?
    } else {
        parts.stored.to_vec()
    };
    if payload.len() != parts.header.payload_len as usize {
        return Err(CblkError::Format(format!(
            "payload length {} != header payload_len {}",
            payload.len(),
            parts.header.payload_len
        )));
    }
    if parts.header.chunk_size != CBLK_CHUNK_SIZE || payload.len() != CBLK_CHUNK_SIZE as usize {
        return Err(CblkError::Format(format!(
            "unsupported chunk_size {} / payload {}",
            parts.header.chunk_size,
            payload.len()
        )));
    }
    Ok(CblkChunk {
        header: parts.header,
        device_id: parts.device_id,
        inline_schema: parts.inline_schema,
        payload,
    })
}

/// CRC-verified header regions with the still-encoded stored payload slice.
pub struct CblkParts<'a> {
    pub header: CblkHeader,
    pub device_id: String,
    pub inline_schema: Option<Vec<u8>>,
    pub stored: &'a [u8],
}

/// Parse header regions and verify CRC without decompressing.
pub fn parse_verified(blob: &[u8]) -> Result<CblkParts<'_>, CblkError> {
    parse_parts(blob)
}

fn parse_parts(blob: &[u8]) -> Result<CblkParts<'_>, CblkError> {
    if blob.len() < CBLK_FIXED_HEADER_LEN {
        return Err(CblkError::Format("blob shorter than fixed header".into()));
    }
    if &blob[0..4] != CBLK_MAGIC.as_slice() {
        return Err(CblkError::Format("bad cblk magic".into()));
    }
    let version = read_u16(blob, 4);
    let flags = read_u16(blob, 6);
    let header_len = read_u16(blob, 8) as usize;
    let device_id_len = read_u16(blob, 10) as usize;
    let schema_id = read_u32(blob, 12);
    let mut schema_hash = [0u8; 8];
    schema_hash.copy_from_slice(&blob[16..24]);
    let chunk_idx = read_u64(blob, 24);
    let generation = read_u64(blob, 32);
    let chunk_size = read_u32(blob, 40);
    let payload_len = read_u32(blob, 44);
    let stored_len = read_u32(blob, 48);
    let crc32c = read_u32(blob, 52);

    if flags & !CBLK_KNOWN_FLAGS != 0 {
        return Err(CblkError::Format(format!("unknown cblk flags {flags:#x}")));
    }
    if version != CBLK_VERSION {
        return Err(CblkError::Format(format!(
            "unsupported cblk version {version}"
        )));
    }
    if header_len < CBLK_FIXED_HEADER_LEN + device_id_len {
        return Err(CblkError::Format(
            "header_len too small for device id".into(),
        ));
    }
    let schema_len = header_len - CBLK_FIXED_HEADER_LEN - device_id_len;
    let inline = flags & CBLK_FLAG_INLINE_SCHEMA != 0;
    if inline && schema_len == 0 {
        return Err(CblkError::Format(
            "inline schema flag set but schema empty".into(),
        ));
    }
    if !inline && schema_len != 0 {
        return Err(CblkError::Format(
            "schema bytes present without inline flag".into(),
        ));
    }
    let total = header_len
        .checked_add(stored_len as usize)
        .ok_or_else(|| CblkError::Format("cblk length overflow".into()))?;
    if blob.len() < total {
        return Err(CblkError::Format(format!(
            "blob length {} < header_len+stored_len {total}",
            blob.len()
        )));
    }
    // Tolerate trailing bytes (RocksDB value framing) only if exact; callers
    // pass the Cozo Bytes value which is exact. Recovery may pass a slice.
    let body = &blob[..total];

    let mut for_crc = body.to_vec();
    write_u32(&mut for_crc, 52, 0);
    let computed = crc32c::crc32c(&for_crc);
    if computed != crc32c {
        return Err(CblkError::CrcMismatch {
            expected: crc32c,
            computed,
        });
    }

    let device_off = CBLK_FIXED_HEADER_LEN;
    let device_bytes = &body[device_off..device_off + device_id_len];
    let device_id = std::str::from_utf8(device_bytes)
        .map_err(|_| CblkError::Format("device id is not UTF-8".into()))?
        .to_string();
    let inline_schema = if inline {
        let schema_off = device_off + device_id_len;
        Some(body[schema_off..schema_off + schema_len].to_vec())
    } else {
        None
    };
    let stored = &body[header_len..header_len + stored_len as usize];
    let header = CblkHeader {
        version,
        flags,
        header_len: header_len as u16,
        device_id_len: device_id_len as u16,
        schema_id,
        schema_hash,
        chunk_idx,
        generation,
        chunk_size,
        payload_len,
        stored_len,
        crc32c,
    };
    Ok(CblkParts {
        header,
        device_id,
        inline_schema,
        stored,
    })
}

fn write_u16(buf: &mut [u8], off: usize, value: u16) {
    buf[off..off + 2].copy_from_slice(&value.to_le_bytes());
}
fn write_u32(buf: &mut [u8], off: usize, value: u32) {
    buf[off..off + 4].copy_from_slice(&value.to_le_bytes());
}
fn write_u64(buf: &mut [u8], off: usize, value: u64) {
    buf[off..off + 8].copy_from_slice(&value.to_le_bytes());
}
fn read_u16(buf: &[u8], off: usize) -> u16 {
    u16::from_le_bytes(buf[off..off + 2].try_into().unwrap())
}
fn read_u32(buf: &[u8], off: usize) -> u32 {
    u32::from_le_bytes(buf[off..off + 4].try_into().unwrap())
}
fn read_u64(buf: &[u8], off: usize) -> u64 {
    u64::from_le_bytes(buf[off..off + 8].try_into().unwrap())
}

/// Summary produced by a raw CBLK scan of RocksDB/Cozo files.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct CblkScanSummary {
    pub files_scanned: u64,
    pub candidates: u64,
    pub valid: u64,
    pub corrupt: u64,
    /// Valid tombstone envelopes seen (included in `valid`).
    pub tombstones: u64,
    pub device_ids: BTreeMap<String, u64>,
}

/// One winning chunk after highest-generation selection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CblkRecoveredChunk {
    pub device_id: String,
    pub chunk_idx: u64,
    pub generation: u64,
    /// The newest version is a discard tombstone: the chunk reads as zeros.
    pub tombstone: bool,
    /// Uncompressed payload; empty for a tombstone.
    pub payload: Vec<u8>,
}

/// Scan `dir` (and children) for raw `CBLK` magic in file bytes, CRC-validate,
/// and keep the highest generation per `(device, idx)`. Does not open Cozo or
/// RocksDB. Contiguous blob files / SST values are scanned raw; RocksDB WAL
/// `.log` files are reassembled first so fragmented write batches still yield
/// intact CBLK envelopes.
pub fn scan_directory(dir: &Path) -> Result<(Vec<CblkRecoveredChunk>, CblkScanSummary), CblkError> {
    let mut best: BTreeMap<(String, u64), CblkRecoveredChunk> = BTreeMap::new();
    let mut summary = CblkScanSummary::default();
    scan_tree(dir, &mut best, &mut summary)?;
    Ok((best.into_values().collect(), summary))
}

fn scan_tree(
    path: &Path,
    best: &mut BTreeMap<(String, u64), CblkRecoveredChunk>,
    summary: &mut CblkScanSummary,
) -> Result<(), CblkError> {
    let meta = fs::metadata(path).map_err(|e| CblkError::Io(format!("{}: {e}", path.display())))?;
    if meta.is_dir() {
        let entries = fs::read_dir(path)
            .map_err(|e| CblkError::Io(format!("read_dir {}: {e}", path.display())))?;
        for entry in entries {
            let entry =
                entry.map_err(|e| CblkError::Io(format!("read_dir {}: {e}", path.display())))?;
            scan_tree(&entry.path(), best, summary)?;
        }
        return Ok(());
    }
    if !meta.is_file() {
        return Ok(());
    }
    summary.files_scanned += 1;
    let bytes = fs::read(path).map_err(|e| CblkError::Io(format!("{}: {e}", path.display())))?;
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    if name.ends_with(".log") {
        // WAL records are 32 KiB-blocked and may fragment a 64 KiB+ CBLK across
        // First/Middle/Last fragments. Reassemble before scanning.
        for payload in wal_reassembled_payloads(&bytes) {
            scan_bytes(&payload, best, summary, true);
        }
    } else {
        scan_bytes(&bytes, best, summary, true);
    }
    Ok(())
}

/// Scan an arbitrary byte buffer for CBLK candidates (used by tests and recovery).
pub fn scan_bytes(
    bytes: &[u8],
    best: &mut BTreeMap<(String, u64), CblkRecoveredChunk>,
    summary: &mut CblkScanSummary,
    count_corrupt: bool,
) {
    let mut i = 0usize;
    while i + CBLK_FIXED_HEADER_LEN <= bytes.len() {
        if &bytes[i..i + 4] != CBLK_MAGIC.as_slice() {
            i += 1;
            continue;
        }
        summary.candidates += 1;
        match try_candidate(&bytes[i..]) {
            Ok(chunk) => {
                summary.valid += 1;
                if chunk.header.is_tombstone() {
                    summary.tombstones += 1;
                }
                *summary
                    .device_ids
                    .entry(chunk.device_id.clone())
                    .or_default() += 1;
                let advance = chunk.header.header_len as usize + chunk.header.stored_len as usize;
                let key = (chunk.device_id.clone(), chunk.header.chunk_idx);
                let replace = match best.get(&key) {
                    Some(existing) => chunk.header.generation >= existing.generation,
                    None => true,
                };
                if replace {
                    best.insert(
                        key,
                        CblkRecoveredChunk {
                            device_id: chunk.device_id,
                            chunk_idx: chunk.header.chunk_idx,
                            generation: chunk.header.generation,
                            tombstone: chunk.header.is_tombstone(),
                            payload: chunk.payload,
                        },
                    );
                }
                // Advance past the consumed blob when possible.
                i += advance.max(1);
            }
            Err(CblkError::CrcMismatch { .. })
            | Err(CblkError::Format(_))
            | Err(CblkError::Decompress(_))
            | Err(CblkError::Io(_)) => {
                if count_corrupt {
                    summary.corrupt += 1;
                }
                i += 1;
            }
        }
    }
}

/// RocksDB WAL block size.
const WAL_BLOCK_SIZE: usize = 32 * 1024;
const WAL_HEADER_LEGACY: usize = 7;
const WAL_HEADER_RECYCLABLE: usize = 11;

const WAL_FULL: u8 = 1;
const WAL_FIRST: u8 = 2;
const WAL_MIDDLE: u8 = 3;
const WAL_LAST: u8 = 4;
const WAL_RECYCLABLE_FULL: u8 = 5;
const WAL_RECYCLABLE_FIRST: u8 = 6;
const WAL_RECYCLABLE_MIDDLE: u8 = 7;
const WAL_RECYCLABLE_LAST: u8 = 8;

/// Reassemble RocksDB WAL record payloads (legacy + recyclable headers).
fn wal_reassembled_payloads(data: &[u8]) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let mut pending: Option<Vec<u8>> = None;
    let mut block_off = 0usize;
    while block_off < data.len() {
        let block_end = (block_off + WAL_BLOCK_SIZE).min(data.len());
        let mut off = block_off;
        while off + WAL_HEADER_LEGACY <= block_end {
            let length = u16::from_le_bytes([data[off + 4], data[off + 5]]) as usize;
            let type_byte = data[off + 6];
            let (header_len, rec_type) = match type_byte {
                WAL_FULL | WAL_FIRST | WAL_MIDDLE | WAL_LAST => (WAL_HEADER_LEGACY, type_byte),
                WAL_RECYCLABLE_FULL
                | WAL_RECYCLABLE_FIRST
                | WAL_RECYCLABLE_MIDDLE
                | WAL_RECYCLABLE_LAST => {
                    if off + WAL_HEADER_RECYCLABLE > block_end {
                        break;
                    }
                    (WAL_HEADER_RECYCLABLE, type_byte)
                }
                0 => break, // zero padding to end of block
                _ => {
                    // Unknown / corrupt; skip rest of this block.
                    break;
                }
            };
            let record_end = off + header_len + length;
            if record_end > block_end {
                break;
            }
            let fragment = &data[off + header_len..record_end];
            match rec_type {
                WAL_FULL | WAL_RECYCLABLE_FULL => {
                    pending = None;
                    out.push(fragment.to_vec());
                }
                WAL_FIRST | WAL_RECYCLABLE_FIRST => {
                    pending = Some(fragment.to_vec());
                }
                WAL_MIDDLE | WAL_RECYCLABLE_MIDDLE => {
                    if let Some(buf) = pending.as_mut() {
                        buf.extend_from_slice(fragment);
                    }
                }
                WAL_LAST | WAL_RECYCLABLE_LAST => {
                    if let Some(mut buf) = pending.take() {
                        buf.extend_from_slice(fragment);
                        out.push(buf);
                    }
                }
                _ => {}
            }
            off = record_end;
        }
        block_off += WAL_BLOCK_SIZE;
    }
    out
}

fn try_candidate(bytes: &[u8]) -> Result<CblkChunk, CblkError> {
    // Need header_len + stored_len; parse_parts validates bounds.
    if bytes.len() < CBLK_FIXED_HEADER_LEN {
        return Err(CblkError::Format("truncated".into()));
    }
    let header_len = read_u16(bytes, 8) as usize;
    let stored_len = read_u32(bytes, 48) as usize;
    let total = header_len
        .checked_add(stored_len)
        .ok_or_else(|| CblkError::Format("overflow".into()))?;
    if bytes.len() < total {
        return Err(CblkError::Format("truncated blob".into()));
    }
    decode(&bytes[..total])
}

/// Write a sparse disk image for `device_id` into `out_path`.
///
/// Missing chunks are left as holes (zeros via `seek` + no write) when the
/// filesystem supports sparse files; otherwise they remain zero-filled by the
/// initial truncate.
pub fn write_sparse_image(
    chunks: &[CblkRecoveredChunk],
    device_id: &str,
    size_bytes: u64,
    out_path: &Path,
) -> Result<u64, CblkError> {
    if size_bytes == 0 || !size_bytes.is_multiple_of(u64::from(CBLK_CHUNK_SIZE)) {
        return Err(CblkError::Format(format!(
            "image size {size_bytes} must be a positive multiple of {CBLK_CHUNK_SIZE}"
        )));
    }
    let mut file = fs::File::create(out_path)
        .map_err(|e| CblkError::Io(format!("create {}: {e}", out_path.display())))?;
    file.set_len(size_bytes)
        .map_err(|e| CblkError::Io(format!("truncate {}: {e}", out_path.display())))?;
    let chunk_size = u64::from(CBLK_CHUNK_SIZE);
    let mut written = 0u64;
    // Tombstones win over older data versions in the scan and stay holes here.
    for chunk in chunks
        .iter()
        .filter(|c| c.device_id == device_id && !c.tombstone)
    {
        let off = chunk
            .chunk_idx
            .checked_mul(chunk_size)
            .ok_or_else(|| CblkError::Format("chunk offset overflow".into()))?;
        if off.saturating_add(chunk_size) > size_bytes {
            return Err(CblkError::Format(format!(
                "chunk {} exceeds image size",
                chunk.chunk_idx
            )));
        }
        if chunk.payload.len() != CBLK_CHUNK_SIZE as usize {
            return Err(CblkError::Format(
                "recovered payload has wrong length".into(),
            ));
        }
        use io::{Seek, SeekFrom, Write};
        file.seek(SeekFrom::Start(off))
            .map_err(|e| CblkError::Io(format!("seek: {e}")))?;
        file.write_all(&chunk.payload)
            .map_err(|e| CblkError::Io(format!("write: {e}")))?;
        written += 1;
    }
    file.sync_all()
        .map_err(|e| CblkError::Io(format!("sync {}: {e}", out_path.display())))?;
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arbitrary_payload(seed: u64) -> Vec<u8> {
        // Content-agnostic: mix compressible runs with pseudo-random bytes so
        // compression on/off both exercise real paths.
        let mut out = vec![0u8; CBLK_CHUNK_SIZE as usize];
        let mut state = seed ^ 0x9e37_79b9_7f4a_7c15;
        for (i, byte) in out.iter_mut().enumerate() {
            state = state
                .wrapping_mul(0xbf58_476d_1ce4_e5b9)
                .wrapping_add(i as u64 + 1);
            if i % 97 < 40 {
                *byte = (state as u8).wrapping_add((i % 7) as u8);
            } else {
                *byte = b'A'.wrapping_add((i % 26) as u8);
            }
        }
        out
    }

    #[test]
    fn encode_decode_round_trip_arbitrary_payload() {
        let payload = arbitrary_payload(42);
        let blob = encode("dev-a", 7, 3, &payload, true).unwrap();
        assert_eq!(&blob[0..4], CBLK_MAGIC);
        let chunk = decode(&blob).unwrap();
        assert_eq!(chunk.device_id, "dev-a");
        assert_eq!(chunk.header.chunk_idx, 7);
        assert_eq!(chunk.header.generation, 3);
        assert_eq!(chunk.payload, payload);
        assert!(chunk.inline_schema.is_some());
        assert_eq!(
            chunk.inline_schema.as_deref(),
            Some(CBLK_SCHEMA_DESCRIPTION)
        );
        assert_eq!(chunk.header.schema_hash, schema_hash());
    }

    #[test]
    fn crc_corruption_is_detected() {
        let payload = arbitrary_payload(99);
        let mut blob = encode("dev-b", 1, 1, &payload, false).unwrap();
        let last = blob.len() - 1;
        blob[last] ^= 0x5a;
        match decode(&blob) {
            Err(CblkError::CrcMismatch { .. }) => {}
            other => panic!("expected crc mismatch, got {other:?}"),
        }
    }

    #[test]
    fn inline_schema_cadence() {
        assert!(should_inline_schema(1));
        assert!(!should_inline_schema(2));
        assert!(!should_inline_schema(255));
        assert!(should_inline_schema(256));
        assert!(!should_inline_schema(257));
        assert!(should_inline_schema(512));
    }

    #[test]
    fn compression_on_when_helpful_off_when_not() {
        let mut zeros = vec![0u8; CBLK_CHUNK_SIZE as usize];
        zeros[0] = 1;
        let compressible = encode("dev-c", 0, 1, &zeros, false).unwrap();
        let parts = parse_verified(&compressible).unwrap();
        assert_ne!(parts.header.flags & CBLK_FLAG_ZSTD, 0);
        assert!(parts.stored.len() < zeros.len());

        // High-entropy payload: compression must not enlarge the stored form.
        let mut entropy = arbitrary_payload(0xdead_beef);
        // overwrite with more entropy from a second stream
        for (i, b) in entropy.iter_mut().enumerate() {
            *b ^= (i.wrapping_mul(1103515245) + 12345) as u8;
        }
        let rawish = encode("dev-c", 1, 2, &entropy, false).unwrap();
        let header = decode(&rawish).unwrap().header;
        if header.flags & CBLK_FLAG_ZSTD != 0 {
            assert!(header.stored_len < header.payload_len);
        } else {
            assert_eq!(header.stored_len, header.payload_len);
        }
        assert_eq!(decode(&rawish).unwrap().payload, entropy);
    }

    #[test]
    fn scan_keeps_highest_generation() {
        let p1 = arbitrary_payload(1);
        let p2 = arbitrary_payload(2);
        let mut stream = Vec::new();
        stream.extend(encode("dev-a", 0, 1, &p1, false).unwrap());
        stream.extend(b"noise");
        stream.extend(encode("dev-a", 0, 5, &p2, true).unwrap());
        stream.extend(encode("dev-a", 0, 3, &p1, false).unwrap());
        let mut best = BTreeMap::new();
        let mut summary = CblkScanSummary::default();
        scan_bytes(&stream, &mut best, &mut summary, true);
        assert_eq!(summary.valid, 3);
        let won = best.get(&("dev-a".into(), 0)).unwrap();
        assert_eq!(won.generation, 5);
        assert_eq!(won.payload, p2);
    }

    #[test]
    fn tombstone_round_trip_and_flags() {
        let blob = encode_tombstone("dev-t", 9, 42).unwrap();
        let chunk = decode(&blob).unwrap();
        assert!(chunk.header.is_tombstone());
        assert_eq!(chunk.header.generation, 42);
        assert_eq!(chunk.header.chunk_idx, 9);
        assert_eq!(chunk.header.payload_len, 0);
        assert!(chunk.payload.is_empty());
        let mut bad = blob.clone();
        let last = bad.len() - 1;
        bad[last] ^= 0x01;
        assert!(matches!(decode(&bad), Err(CblkError::CrcMismatch { .. })));
    }

    #[test]
    fn scan_tombstone_beats_older_data_and_newer_data_beats_tombstone() {
        let p1 = arbitrary_payload(7);
        let p2 = arbitrary_payload(8);
        let mut stream = Vec::new();
        // idx 0: data gen 1, tombstone gen 2 (discarded) -> must stay a hole.
        stream.extend(encode("dev-a", 0, 1, &p1, false).unwrap());
        stream.extend(encode_tombstone("dev-a", 0, 2).unwrap());
        // idx 1: tombstone gen 3, rewritten gen 4 -> data wins.
        stream.extend(encode_tombstone("dev-a", 1, 3).unwrap());
        stream.extend(encode("dev-a", 1, 4, &p2, false).unwrap());
        let mut best = BTreeMap::new();
        let mut summary = CblkScanSummary::default();
        scan_bytes(&stream, &mut best, &mut summary, true);
        assert_eq!(summary.valid, 4);
        assert_eq!(summary.tombstones, 2);
        assert!(best.get(&("dev-a".into(), 0)).unwrap().tombstone);
        let one = best.get(&("dev-a".into(), 1)).unwrap();
        assert!(!one.tombstone);
        assert_eq!(one.payload, p2);

        let dir = tempfile::tempdir().unwrap();
        let image = dir.path().join("img");
        let chunks: Vec<_> = best.into_values().collect();
        let size = 2 * u64::from(CBLK_CHUNK_SIZE);
        assert_eq!(
            write_sparse_image(&chunks, "dev-a", size, &image).unwrap(),
            1
        );
        let bytes = std::fs::read(&image).unwrap();
        let chunk = CBLK_CHUNK_SIZE as usize;
        assert!(
            bytes[..chunk].iter().all(|b| *b == 0),
            "discarded chunk resurrected"
        );
        assert_eq!(&bytes[chunk..], p2.as_slice());
    }

    #[test]
    fn unknown_flags_are_rejected() {
        let mut blob = encode("dev-a", 0, 1, &arbitrary_payload(3), false).unwrap();
        let flags = u16::from_le_bytes([blob[6], blob[7]]) | 0x8000;
        blob[6..8].copy_from_slice(&flags.to_le_bytes());
        blob[52..56].fill(0);
        let crc = crc32c::crc32c(&blob);
        blob[52..56].copy_from_slice(&crc.to_le_bytes());
        assert!(matches!(decode(&blob), Err(CblkError::Format(_))));
    }
}
