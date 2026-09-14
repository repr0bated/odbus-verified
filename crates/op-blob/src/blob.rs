//! `PluginObjectBlob` — the sealed, self-describing projection of one
//! `PluginSchema` into one runtime identity (whitepaper "Target Data Model").
//!
//! The blob couples the four identities that must not drift:
//! schema identity (canonical JSON + sha256), D-Bus identity, gRPC identity,
//! and the reflection descriptor identity. It is deliberately redundant at the
//! blob boundary: schema stays authoritative, but the blob is self-describing
//! enough that reflection, GUI, compliance, and dispatch reason about the same
//! object without reassembling identity from scattered files.
//!
//! ## Sealed byte format (zero-copy)
//!
//! ```text
//! offset  size  content
//! 0       8     magic "OPBLOB01"
//! 8       2     format version (u16 LE) = 1
//! 10      2     section count (u16 LE)
//! 12      4     reserved
//! 16      32    sha256 of the SCHEMA_JSON section (the schema hash)
//! 48      16    reserved
//! 64      24*n  section table: tag u32, reserved u32, offset u64, len u64
//! ...           8-byte-aligned section payloads
//! ```
//!
//! Sections: 1 = canonical `PluginSchema` JSON, 2 = `BlobManifest` JSON,
//! 3 = protobuf `FileDescriptorSet`, 4 = compliance/extra metadata JSON.
//!
//! `BlobRef` verifies magic + hash once, then hands out borrowed slices —
//! reads never copy or re-parse the payload. Sealed bytes are fully
//! deterministic for the same schema (sorted maps, no timestamps), so the
//! schema hash doubles as the artifact identity in the filename:
//! `<plugin_id>.<hash16>.blob`.

use crate::descriptor;
use crate::schema::{hex, PluginSchema, SideEffect};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io;
use std::path::{Path, PathBuf};

pub const MAGIC: &[u8; 8] = b"OPBLOB01";
pub const FORMAT_VERSION: u16 = 1;

pub const SECTION_SCHEMA_JSON: u32 = 1;
pub const SECTION_MANIFEST_JSON: u32 = 2;
pub const SECTION_DESCRIPTOR_SET: u32 = 3;
pub const SECTION_META_JSON: u32 = 4;

const HEADER_LEN: usize = 64;
const SECTION_ENTRY_LEN: usize = 24;

/// D-Bus identity carried by the blob (REQ-7.1/7.2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DbusIdentity {
    pub bus_name: String,
    pub object_path: String,
    pub interface_name: String,
}

/// gRPC identity: per-method packages/services synthesized from the schema.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GrpcIdentity {
    /// Fully-qualified service names, sorted.
    pub services: Vec<String>,
    /// Descriptor file names, sorted (parallel to services).
    pub files: Vec<String>,
}

/// Per-method manifest entry copied from `PluginSchema.methods` plus the
/// synthesized gRPC identity — the compliance/audit unit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MethodManifest {
    pub schema_name: String,
    pub grpc_name: String,
    pub grpc_path: String,
    pub grpc_service: String,
    pub grpc_file: String,
    pub input_message: String,
    pub output_message: String,
    pub subid: String,
    pub required_capability: Option<String>,
    pub side_effect: SideEffect,
    pub idempotent: bool,
    /// Fully-qualified symbols the method's descriptor file defines.
    pub symbols: Vec<String>,
}

/// The blob's self-describing metadata (section 2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BlobManifest {
    /// Runtime blob class. Serialized as `type` so the store can hold active
    /// reflection blobs now and other blob families later without guessing.
    #[serde(rename = "type", default = "default_blob_type")]
    pub blob_type: String,
    pub plugin_id: String,
    pub schema_version: String,
    pub schema_hash: String,
    pub dbus: DbusIdentity,
    pub grpc: GrpcIdentity,
    /// WireGuard-keypair identity of the publishing account (public half
    /// only); its session persists for the life of the account.
    #[serde(default)]
    pub identity: Option<crate::identity::BlobIdentity>,
    pub methods: Vec<MethodManifest>,
}

fn default_blob_type() -> String {
    "active_reflection".to_string()
}

/// Owned, unsealed blob. `seal()` produces the immutable byte image.
#[derive(Debug, Clone)]
pub struct PluginObjectBlob {
    pub manifest: BlobManifest,
    /// Canonical `PluginSchema` JSON (the authoritative contract bytes).
    pub schema_json: String,
    /// Encoded `google.protobuf.FileDescriptorSet` for all plugin methods.
    pub descriptor_set: Vec<u8>,
    /// Compliance / extra metadata (subids, immutable paths, dialect …).
    pub meta_json: String,
}

/// Canonical D-Bus path form: lowercase, hyphens -> underscores.
pub fn canonical_plugin_id(name: &str) -> String {
    name.to_lowercase().replace('-', "_")
}

/// Freeze one `PluginSchema` into a `PluginObjectBlob` (whitepaper "Blob
/// Creation Pipeline" steps 3–9). The schema is the input and stays the
/// authority; nothing here invents contract data.
pub fn blobify(schema: &PluginSchema) -> PluginObjectBlob {
    blobify_with_identity(schema, None)
}

/// `blobify`, stamping the publishing account's WireGuard-keypair identity
/// (public half only) into the manifest.
pub fn blobify_with_identity(
    schema: &PluginSchema,
    identity: Option<crate::identity::BlobIdentity>,
) -> PluginObjectBlob {
    blobify_canonical(schema, schema.canonical_json(), identity)
}

/// `blobify` with the sealed canonical JSON supplied by the caller.
///
/// This is the lossless path for the workspace's authoritative
/// `op_state_store::PluginSchema`: the caller serializes the FULL canonical
/// type (signals, guarantees, category, …) and `schema` is only the reduced
/// working view this crate needs for descriptor synthesis. The sealed
/// SCHEMA_JSON section — and therefore the schema hash — covers the complete
/// contract, never a reduced projection of it.
pub fn blobify_canonical(
    schema: &PluginSchema,
    schema_json: String,
    identity: Option<crate::identity::BlobIdentity>,
) -> PluginObjectBlob {
    let plugin_id = canonical_plugin_id(&schema.name);
    let schema_hash = {
        let mut h = Sha256::new();
        h.update(schema_json.as_bytes());
        hex(&h.finalize())
    };

    let (descriptor_set, method_infos) = descriptor::descriptor_set_for_plugin(&plugin_id, schema);

    let mut methods: Vec<MethodManifest> = method_infos
        .iter()
        .map(|d| {
            let decl = schema
                .methods
                .get(&d.schema_name)
                .expect("descriptor method must come from PluginSchema.methods");
            MethodManifest {
                schema_name: d.schema_name.clone(),
                grpc_name: descriptor::pascal_case(&decl.name),
                grpc_path: d.grpc_path.clone(),
                grpc_service: d.service_full.clone(),
                grpc_file: d.file_name.clone(),
                input_message: d.input_full.clone(),
                output_message: d.output_full.clone(),
                subid: decl.subid.clone(),
                required_capability: decl.required_capability.clone(),
                side_effect: decl.side_effect,
                idempotent: decl.idempotent,
                symbols: d.symbols.clone(),
            }
        })
        .collect();
    methods.sort_by(|a, b| a.schema_name.cmp(&b.schema_name));

    let mut services: Vec<String> = methods.iter().map(|m| m.grpc_service.clone()).collect();
    services.sort();
    let mut files: Vec<String> = methods.iter().map(|m| m.grpc_file.clone()).collect();
    files.sort();

    let manifest = BlobManifest {
        blob_type: default_blob_type(),
        plugin_id: plugin_id.clone(),
        schema_version: schema.version.clone(),
        schema_hash,
        dbus: DbusIdentity {
            bus_name: "org.opdbus.v1.plugins".to_string(),
            object_path: format!("/org/opdbus/v1/plugins/{plugin_id}"),
            interface_name: "org.opdbus.v1.PluginV1".to_string(),
        },
        grpc: GrpcIdentity { services, files },
        identity,
        methods,
    };

    let meta = serde_json::json!({
        "blob_format": FORMAT_VERSION,
        "dialect": schema.dialect,
        "subids": schema.subids,
        "immutable_paths": schema.immutable_paths,
        "json_schema": schema.to_json_schema(),
    });

    PluginObjectBlob {
        manifest,
        schema_json,
        descriptor_set,
        meta_json: serde_json::to_string(&meta).expect("meta serializes"),
    }
}

impl PluginObjectBlob {
    /// Rehydrate an owned blob from sealed bytes (inverse of `seal`).
    /// `schema_json` is copied verbatim, so `from_sealed(b).seal() == b`
    /// and the schema hash is preserved.
    pub fn from_sealed(bytes: &[u8]) -> Result<Self, String> {
        let r = BlobRef::new(bytes)?;
        Ok(Self {
            manifest: r.manifest()?,
            schema_json: r.schema_json().to_string(),
            descriptor_set: r.descriptor_set().to_vec(),
            meta_json: r.meta_json().unwrap_or("{}").to_string(),
        })
    }

    /// Seal into the immutable byte image. Deterministic: same schema, same bytes.
    pub fn seal(&self) -> Vec<u8> {
        let manifest_json =
            serde_json::to_string(&serde_json::to_value(&self.manifest).unwrap()).unwrap();
        let sections: [(u32, &[u8]); 4] = [
            (SECTION_SCHEMA_JSON, self.schema_json.as_bytes()),
            (SECTION_MANIFEST_JSON, manifest_json.as_bytes()),
            (SECTION_DESCRIPTOR_SET, &self.descriptor_set),
            (SECTION_META_JSON, self.meta_json.as_bytes()),
        ];

        let table_len = sections.len() * SECTION_ENTRY_LEN;
        let mut offset = align8(HEADER_LEN + table_len);
        let mut entries = Vec::with_capacity(sections.len());
        for (tag, bytes) in &sections {
            entries.push((*tag, offset as u64, bytes.len() as u64));
            offset = align8(offset + bytes.len());
        }

        let mut out = Vec::with_capacity(offset);
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
        out.extend_from_slice(&(sections.len() as u16).to_le_bytes());
        out.extend_from_slice(&[0u8; 4]);
        let mut h = Sha256::new();
        h.update(self.schema_json.as_bytes());
        out.extend_from_slice(&h.finalize());
        out.extend_from_slice(&[0u8; 16]);
        debug_assert_eq!(out.len(), HEADER_LEN);

        for (tag, off, len) in &entries {
            out.extend_from_slice(&tag.to_le_bytes());
            out.extend_from_slice(&[0u8; 4]);
            out.extend_from_slice(&off.to_le_bytes());
            out.extend_from_slice(&len.to_le_bytes());
        }

        for ((_, bytes), (_, off, _)) in sections.iter().zip(&entries) {
            while out.len() < *off as usize {
                out.push(0);
            }
            out.extend_from_slice(bytes);
        }
        out
    }

    /// `<plugin_id>.<hash16>.blob` — hash-addressed filename (shm layout).
    pub fn file_name(&self) -> String {
        format!(
            "{}.{}.blob",
            self.manifest.plugin_id,
            &self.manifest.schema_hash[..16]
        )
    }

    /// Atomic write: seal to `<name>.blob.tmp`, then rename over the final
    /// path. Readers scanning the directory never observe a partial blob
    /// (`.tmp` fails the `.blob` extension filter; rename(2) is atomic on
    /// the same filesystem).
    pub fn write_to_dir(&self, dir: &Path) -> io::Result<PathBuf> {
        std::fs::create_dir_all(dir)?;
        let path = dir.join(self.file_name());
        let tmp = dir.join(format!("{}.tmp", self.file_name()));
        std::fs::write(&tmp, self.seal())?;
        std::fs::rename(&tmp, &path)?;
        Ok(path)
    }
}

fn align8(n: usize) -> usize {
    (n + 7) & !7
}

/// Zero-copy view over sealed blob bytes. Construction verifies magic,
/// bounds, and the schema hash once; accessors return borrowed slices.
pub struct BlobRef<'a> {
    bytes: &'a [u8],
    sections: Vec<(u32, usize, usize)>,
}

impl<'a> BlobRef<'a> {
    pub fn new(bytes: &'a [u8]) -> Result<Self, String> {
        let blob = Self::parse_structure(bytes)?;
        let mut hash = Sha256::new();
        hash.update(blob.schema_json().as_bytes());
        if hash.finalize().as_slice() != &bytes[16..48] {
            return Err("schema hash mismatch: blob is corrupt or tampered".into());
        }
        Ok(blob)
    }

    /// Borrow an already sealed, manifest-pinned catalog artifact without
    /// recomputing its digest. The expected hash MUST come from the trusted
    /// sealer-owned catalog, never an untrusted request. This validates format,
    /// bounds, UTF-8 and stored header identity; it does not authenticate bytes.
    /// Use `new` for untrusted imports and producer-side integrity checks.
    pub fn from_manifest_pinned(bytes: &'a [u8], expected_hash: &str) -> Result<Self, String> {
        let blob = Self::parse_structure(bytes)?;
        if expected_hash.len() != 64
            || !expected_hash.bytes().all(|byte| byte.is_ascii_hexdigit())
            || blob.schema_hash_hex() != expected_hash
        {
            return Err("manifest/blob stored schema hash mismatch".into());
        }
        Ok(blob)
    }

    fn parse_structure(bytes: &'a [u8]) -> Result<Self, String> {
        if bytes.len() < HEADER_LEN || &bytes[..8] != MAGIC {
            return Err("not an OPBLOB01 blob".into());
        }
        let version = u16::from_le_bytes([bytes[8], bytes[9]]);
        if version != FORMAT_VERSION {
            return Err(format!("unsupported blob format version {version}"));
        }
        let count = u16::from_le_bytes([bytes[10], bytes[11]]) as usize;
        let table_end = HEADER_LEN + count * SECTION_ENTRY_LEN;
        if bytes.len() < table_end {
            return Err("section table overruns blob".into());
        }
        let mut sections: Vec<(u32, usize, usize)> = Vec::with_capacity(count);
        for i in 0..count {
            let e = HEADER_LEN + i * SECTION_ENTRY_LEN;
            let tag = u32::from_le_bytes(bytes[e..e + 4].try_into().unwrap());
            let off = usize::try_from(u64::from_le_bytes(bytes[e + 8..e + 16].try_into().unwrap()))
                .map_err(|_| format!("section {tag} offset overflows address space"))?;
            let len = usize::try_from(u64::from_le_bytes(
                bytes[e + 16..e + 24].try_into().unwrap(),
            ))
            .map_err(|_| format!("section {tag} length overflows address space"))?;
            if off < table_end || off.checked_add(len).is_none_or(|end| end > bytes.len()) {
                return Err(format!("section {tag} overruns blob"));
            }
            if sections
                .iter()
                .any(|(previous_tag, previous_offset, previous_length)| {
                    *previous_tag == tag
                        || (len > 0
                            && *previous_length > 0
                            && off < previous_offset + previous_length
                            && *previous_offset < off + len)
                })
            {
                return Err(format!("duplicate or overlapping section {tag}"));
            }
            sections.push((tag, off, len));
        }
        let blob = Self { bytes, sections };

        // Verify schema and manifest sections and UTF-8 validity.
        let schema = blob
            .section(SECTION_SCHEMA_JSON)
            .ok_or("missing schema section")?;
        std::str::from_utf8(schema)
            .map_err(|e| format!("schema section is not valid UTF-8: {e}"))?;

        let manifest = blob
            .section(SECTION_MANIFEST_JSON)
            .ok_or("missing manifest section")?;
        std::str::from_utf8(manifest)
            .map_err(|e| format!("manifest section is not valid UTF-8: {e}"))?;

        Ok(blob)
    }

    fn section(&self, tag: u32) -> Option<&'a [u8]> {
        self.sections
            .iter()
            .find(|(t, _, _)| *t == tag)
            .map(|(_, off, len)| &self.bytes[*off..*off + *len])
    }

    /// Canonical `PluginSchema` JSON — borrowed, zero-copy.
    pub fn schema_json(&self) -> &'a str {
        self.section(SECTION_SCHEMA_JSON)
            .and_then(|b| std::str::from_utf8(b).ok())
            .unwrap_or("")
    }

    pub fn manifest_json(&self) -> &'a str {
        self.section(SECTION_MANIFEST_JSON)
            .and_then(|b| std::str::from_utf8(b).ok())
            .unwrap_or("")
    }

    /// Encoded `FileDescriptorSet` — borrowed, zero-copy; slices of this can
    /// be served directly to reflection clients.
    pub fn descriptor_set(&self) -> &'a [u8] {
        self.section(SECTION_DESCRIPTOR_SET).unwrap_or(&[])
    }

    pub fn meta_json(&self) -> Option<&'a str> {
        self.section(SECTION_META_JSON)
            .and_then(|b| std::str::from_utf8(b).ok())
    }

    pub fn schema_hash_hex(&self) -> String {
        hex(&self.bytes[16..48])
    }

    /// Parse the manifest (the one intentional allocation on the read path;
    /// everything else stays borrowed).
    pub fn manifest(&self) -> Result<BlobManifest, String> {
        serde_json::from_str(self.manifest_json()).map_err(|e| format!("manifest parse: {e}"))
    }

    /// Reconstruct the authoritative `PluginSchema` from the sealed bytes.
    ///
    /// This is the reduced working view; blobs sealed via
    /// `blobify_plugin_schema` carry the full workspace schema — use
    /// [`Self::state_store_schema`] to read it without loss.
    pub fn plugin_schema(&self) -> Result<PluginSchema, String> {
        serde_json::from_str(self.schema_json()).map_err(|e| format!("schema parse: {e}"))
    }

    /// Reconstruct the FULL canonical `op_state_store::PluginSchema`
    /// (signals, guarantees, category, …) from the sealed bytes. The plugin
    /// IS this schema; consumers must read it from here, never from a
    /// registry or a monolith file.
    pub fn state_store_schema(&self) -> Result<op_state_store::PluginSchema, String> {
        serde_json::from_str(self.schema_json()).map_err(|e| format!("schema parse: {e}"))
    }
}
