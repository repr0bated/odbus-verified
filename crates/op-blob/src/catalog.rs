//! `ActiveReflectionCatalog` — the bridge-owned mutable layer that
//! tonic-reflection's immutable `Builder` cannot be (whitepaper: "Why
//! tonic-reflection Builder Is Not Enough").
//!
//! The catalog owns active plugin blobs and answers reflection queries:
//! `list_services` advertises only active blobs (no phantom services);
//! `file_by_filename` / `symbol_by_name` serve borrowed descriptor bytes
//! sliced straight out of the sealed blob images (zero-copy).
//!
//! Persistence is the per-object shm layout that replaces monolithic
//! `/dev/shm/live-schema.json`:
//!
//! ```text
//! /dev/shm/opdbus/plugin-blobs/<plugin_id>.<schema_hash16>.blob
//! ```

use crate::blob::{BlobManifest, BlobRef, PluginObjectBlob};
use crate::descriptor;
use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};

pub const DEFAULT_SHM_DIR: &str = "/dev/shm/opdbus/plugin-blobs";

/// Catalog manifest filename — the atomic commit point of the blob dir.
/// `{ catalog_hash, generation, plugins: { <plugin_id>: <schema_hash> } }`.
/// `catalog_hash` is a blake3 leaf-fold over the sorted per-blob schema
/// hashes; it is computed HERE and only here — consumers read it, never
/// re-hash (one-schema rule).
pub const MANIFEST_FILENAME: &str = ".manifest.json";

struct Entry {
    path: PathBuf,
    /// Sealed blob image; descriptor lookups return subslices of this.
    bytes: Vec<u8>,
    manifest: BlobManifest,
    /// file name -> (offset, len) of the FileDescriptorProto within `bytes`.
    files: HashMap<String, (usize, usize)>,
    /// fully-qualified symbol -> file name.
    symbols: HashMap<String, String>,
}

pub struct ActiveReflectionCatalog {
    dir: PathBuf,
    entries: HashMap<String, Entry>,
}

/// Durable blob store. It keeps the historical `BlobStore` name while allowing
/// typed blob families in one store namespace. Today the active-reflection
/// family is backed by `ActiveReflectionCatalog`.
pub struct BlobStore {
    active_reflection: ActiveReflectionCatalog,
}

impl BlobStore {
    pub fn open(dir: impl Into<PathBuf>) -> io::Result<Self> {
        Ok(Self {
            active_reflection: ActiveReflectionCatalog::open(dir)?,
        })
    }

    pub fn open_default() -> io::Result<Self> {
        Self::open(DEFAULT_SHM_DIR)
    }

    pub fn write(&mut self, blob: &PluginObjectBlob) -> io::Result<PathBuf> {
        match blob.manifest.blob_type.as_str() {
            "active_reflection" => self.active_reflection.upsert_blob(blob),
            other => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("unsupported blob type: {other}"),
            )),
        }
    }

    pub fn remove_blob(&mut self, plugin_id: &str) -> io::Result<bool> {
        self.active_reflection.remove_blob(plugin_id)
    }

    pub fn retain_plugins(
        &mut self,
        keep: &std::collections::HashSet<String>,
    ) -> io::Result<Vec<String>> {
        self.active_reflection.retain_plugins(keep)
    }

    pub fn catalog_hash(&self) -> String {
        self.active_reflection.catalog_hash()
    }

    pub fn list_services(&self) -> Vec<String> {
        self.active_reflection.list_services()
    }

    pub fn active_plugins(&self) -> Vec<String> {
        self.active_reflection.active_plugins()
    }

    pub fn manifest(&self, plugin_id: &str) -> Option<&BlobManifest> {
        self.active_reflection.manifest(plugin_id)
    }

    pub fn blob_paths(&self) -> Vec<PathBuf> {
        self.active_reflection.blob_paths()
    }

    pub fn plugin_object_blobs(&self) -> Vec<PluginObjectBlob> {
        self.active_reflection.plugin_object_blobs()
    }

    pub fn active_reflection(&self) -> &ActiveReflectionCatalog {
        &self.active_reflection
    }
}

impl ActiveReflectionCatalog {
    /// Open (and create) a catalog directory, loading any existing blobs.
    pub fn open(dir: impl Into<PathBuf>) -> io::Result<Self> {
        let dir = dir.into();
        std::fs::create_dir_all(&dir)?;
        let mut cat = Self {
            dir: dir.clone(),
            entries: HashMap::new(),
        };
        for e in std::fs::read_dir(&dir)? {
            let path = e?.path();
            if path.extension().and_then(|x| x.to_str()) == Some("blob")
                && let Err(err) = cat.load_file(&path)
            {
                // A corrupt blob must not take the catalog down; it is
                // simply not advertised.
                eprintln!("op-blob: skipping {}: {err}", path.display());
            }
        }
        Ok(cat)
    }

    /// Default runtime location on tmpfs (REQ-8.3: never Btrfs-backed I/O).
    pub fn open_shm() -> io::Result<Self> {
        Self::open(DEFAULT_SHM_DIR)
    }

    fn load_file(&mut self, path: &Path) -> Result<(), String> {
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        let entry = index_entry(path.to_path_buf(), bytes)?;
        self.entries.insert(entry.manifest.plugin_id.clone(), entry);
        Ok(())
    }

    /// Whitepaper lifecycle steps 10–12: persist the sealed blob, drop stale
    /// hash versions of the same plugin, rebuild the reflection index, and
    /// commit the catalog manifest.
    pub fn upsert_blob(&mut self, blob: &PluginObjectBlob) -> io::Result<PathBuf> {
        let path = blob.write_to_dir(&self.dir)?;
        // Remove superseded hash versions on disk.
        let prefix = format!("{}.", blob.manifest.plugin_id);
        for e in std::fs::read_dir(&self.dir)? {
            let p = e?.path();
            if p != path
                && let Some(name) = p.file_name().and_then(|n| n.to_str())
                && name.starts_with(&prefix)
                && name.ends_with(".blob")
            {
                let _ = std::fs::remove_file(&p);
            }
        }
        let bytes = std::fs::read(&path)?;
        let entry = index_entry(path.clone(), bytes)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        self.entries.insert(blob.manifest.plugin_id.clone(), entry);
        self.write_manifest()?;
        Ok(path)
    }

    /// Inverse lifecycle: deactivate the plugin object, delete the artifact,
    /// stop advertising its services, commit the catalog manifest.
    pub fn remove_blob(&mut self, plugin_id: &str) -> io::Result<bool> {
        match self.entries.remove(plugin_id) {
            Some(entry) => {
                if entry.path.exists() {
                    std::fs::remove_file(&entry.path)?;
                }
                self.write_manifest()?;
                Ok(true)
            }
            None => Ok(false),
        }
    }

    /// Drop every blob whose plugin id is NOT in `keep`. Returns the removed
    /// plugin ids. This is how a full re-seal deregisters plugins that no
    /// longer exist — a blob in the catalog IS the plugin, so a plugin that
    /// vanished from the build must vanish from the catalog.
    pub fn retain_plugins(
        &mut self,
        keep: &std::collections::HashSet<String>,
    ) -> io::Result<Vec<String>> {
        let stale: Vec<String> = self
            .entries
            .keys()
            .filter(|id| !keep.contains(*id))
            .cloned()
            .collect();
        for id in &stale {
            if let Some(entry) = self.entries.remove(id)
                && entry.path.exists()
            {
                std::fs::remove_file(&entry.path)?;
            }
        }
        if !stale.is_empty() {
            self.write_manifest()?;
        }
        Ok(stale)
    }

    /// Blake3 leaf-fold over the sorted `(plugin_id, schema_hash)` pairs —
    /// the single catalog identity value.
    pub fn catalog_hash(&self) -> String {
        let mut leaves: Vec<(&String, &String)> = self
            .entries
            .iter()
            .map(|(id, e)| (id, &e.manifest.schema_hash))
            .collect();
        leaves.sort();
        let mut h = blake3::Hasher::new();
        for (id, hash) in leaves {
            h.update(id.as_bytes());
            h.update(b"\0");
            h.update(hash.as_bytes());
            h.update(b"\0");
        }
        h.finalize().to_hex().to_string()
    }

    /// Write `.manifest.json` atomically (tmp + rename), LAST after blob
    /// mutations — the commit point consumers use for change detection.
    fn write_manifest(&self) -> io::Result<()> {
        let path = self.dir.join(MANIFEST_FILENAME);
        let generation = Self::read_manifest_generation(&path).wrapping_add(1);
        // The hash this write supersedes. A genesis minted moments before a
        // reseal binds it, and would otherwise name a catalog nothing publishes
        // any more — so the manifest carries a two-deep window and verification
        // accepts either. Deliberately a window, not a history: the durable
        // record of what a session ran against is the chain, not this file.
        let previous_catalog_hash = Self::read_catalog_hash(&self.dir).unwrap_or_default();
        let mut plugins = std::collections::BTreeMap::new();
        for (id, e) in &self.entries {
            plugins.insert(id.clone(), e.manifest.schema_hash.clone());
        }
        let manifest = serde_json::json!({
            "catalog_hash": self.catalog_hash(),
            "previous_catalog_hash": previous_catalog_hash,
            "generation": generation,
            "plugins": plugins,
        });
        let bytes = serde_json::to_vec_pretty(&manifest)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let tmp = self.dir.join(format!("{MANIFEST_FILENAME}.tmp"));
        std::fs::write(&tmp, &bytes)?;
        std::fs::rename(&tmp, &path)?;
        Ok(())
    }

    /// Current manifest `generation`, or 0 if absent/unparsable. The next
    /// write uses `generation + 1` so readers get a monotonic staleness marker.
    fn read_manifest_generation(path: &Path) -> u64 {
        std::fs::read(path)
            .ok()
            .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
            .and_then(|v| v.get("generation").and_then(serde_json::Value::as_u64))
            .unwrap_or(0)
    }

    /// Read the published `catalog_hash` from a catalog dir without loading
    /// any blobs. Consumers use this for arrival-triggered change detection —
    /// read, never re-hash.
    pub fn read_catalog_hash(dir: &Path) -> Option<String> {
        let bytes = std::fs::read(dir.join(MANIFEST_FILENAME)).ok()?;
        let v: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
        v.get("catalog_hash")
            .and_then(serde_json::Value::as_str)
            .map(String::from)
    }

    /// Reflection `ListServices`: only active blobs, sorted.
    pub fn list_services(&self) -> Vec<String> {
        let mut v: Vec<String> = self
            .entries
            .values()
            .flat_map(|e| e.manifest.grpc.services.iter().cloned())
            .collect();
        v.sort();
        v
    }

    pub fn active_plugins(&self) -> Vec<String> {
        let mut v: Vec<String> = self.entries.keys().cloned().collect();
        v.sort();
        v
    }

    /// Reflection `file_by_filename`: borrowed `FileDescriptorProto` bytes.
    pub fn file_by_filename(&self, filename: &str) -> Option<&[u8]> {
        self.entries.values().find_map(|e| {
            e.files
                .get(filename)
                .map(|(off, len)| &e.bytes[*off..*off + *len])
        })
    }

    /// Reflection `file_containing_symbol`.
    pub fn symbol_by_name(&self, symbol: &str) -> Option<&[u8]> {
        self.entries.values().find_map(|e| {
            e.symbols
                .get(symbol)
                .and_then(|file| e.files.get(file))
                .map(|(off, len)| &e.bytes[*off..*off + *len])
        })
    }

    pub fn manifest(&self, plugin_id: &str) -> Option<&BlobManifest> {
        self.entries.get(plugin_id).map(|e| &e.manifest)
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Paths of all active blob artifacts (input to btrfs sealing).
    pub fn blob_paths(&self) -> Vec<PathBuf> {
        let mut v: Vec<PathBuf> = self.entries.values().map(|e| e.path.clone()).collect();
        v.sort();
        v
    }

    /// Rehydrate every active blob as an owned `PluginObjectBlob`, sorted by
    /// plugin id — the startup-hydration input for in-memory reflection.
    pub fn plugin_object_blobs(&self) -> Vec<PluginObjectBlob> {
        let mut ids: Vec<&String> = self.entries.keys().collect();
        ids.sort();
        ids.iter()
            .filter_map(|id| PluginObjectBlob::from_sealed(&self.entries[*id].bytes).ok())
            .collect()
    }
}

/// Read ONE plugin's canonical schema straight from its sealed blob in `dir`
/// — the single-plugin read path (no catalog load, no monolith file). This is
/// how a component (including a plugin reading its own contract) asks "what
/// is plugin X": the blob answers, nothing else does.
pub fn read_plugin_state_store_schema(
    dir: &Path,
    plugin_id: &str,
) -> Option<op_state_store::PluginSchema> {
    if plugin_id.is_empty()
        || !plugin_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
    {
        return None;
    }
    let manifest_path = dir.join(MANIFEST_FILENAME);
    let committed = std::fs::read(&manifest_path).ok()?;
    let catalog: serde_json::Value = serde_json::from_slice(&committed).ok()?;
    let expected_hash = catalog
        .get("plugins")?
        .as_object()?
        .get(plugin_id)?
        .as_str()?;
    if expected_hash.len() != 64 || !expected_hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let path = dir.join(format!("{plugin_id}.{}.blob", &expected_hash[..16]));
    let bytes = std::fs::read(path).ok()?;
    // The authoritative sealer-owned manifest supplies the expected identity.
    // This consumer performs no digest computation, scan, reseal, or write.
    let blob = BlobRef::from_manifest_pinned(&bytes, expected_hash).ok()?;
    let manifest = blob.manifest().ok()?;
    if manifest.plugin_id != plugin_id
        || manifest.schema_hash != expected_hash
        || manifest.blob_type != "active_reflection"
    {
        return None;
    }
    let schema = blob.state_store_schema().ok()?;
    if schema.name != plugin_id
        || schema.version != manifest.schema_version
        || std::fs::read(manifest_path).ok()? != committed
    {
        return None;
    }
    Some(schema)
}

/// [`read_plugin_state_store_schema`] against the default SHM catalog.
pub fn read_plugin_schema_shm(plugin_id: &str) -> Option<op_state_store::PluginSchema> {
    read_plugin_state_store_schema(Path::new(DEFAULT_SHM_DIR), plugin_id)
}

/// The active plugin ids from the catalog manifest (sorted) — the cheap
/// "which plugins exist" read: one small JSON file, no blob loads.
pub fn read_manifest_plugin_ids(dir: &Path) -> Option<Vec<String>> {
    let bytes = std::fs::read(dir.join(MANIFEST_FILENAME)).ok()?;
    let v: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    let mut ids: Vec<String> = v.get("plugins")?.as_object()?.keys().cloned().collect();
    ids.sort();
    Some(ids)
}

/// [`read_manifest_plugin_ids`] against the default SHM catalog.
pub fn read_manifest_plugin_ids_shm() -> Option<Vec<String>> {
    read_manifest_plugin_ids(Path::new(DEFAULT_SHM_DIR))
}

fn index_entry(path: PathBuf, bytes: Vec<u8>) -> Result<Entry, String> {
    let (manifest, files) = {
        let blob = BlobRef::new(&bytes)?;
        let manifest = blob.manifest()?;
        let set = blob.descriptor_set();
        let set_start = set.as_ptr() as usize - bytes.as_ptr() as usize;
        let mut files = HashMap::new();
        for f in descriptor::decode_file_summaries(set)? {
            let off = f.bytes.as_ptr() as usize - set.as_ptr() as usize;
            files.insert(f.name.clone(), (set_start + off, f.bytes.len()));
        }
        (manifest, files)
    };
    let mut symbols = HashMap::new();
    for m in &manifest.methods {
        for s in &m.symbols {
            symbols.insert(s.clone(), m.grpc_file.clone());
        }
    }
    Ok(Entry {
        path,
        bytes,
        manifest,
        files,
        symbols,
    })
}
