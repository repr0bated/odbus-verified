//! Dump every proto available to the system into a target directory.
//!
//! Combines:
//!   1. Static `.proto` files from every `crates/*/proto/` tree.
//!   2. Dynamically-generated plugin method protos reconstructed from the
//!      sealed blobs in the SHM catalog (`/dev/shm/opdbus/plugin-blobs`).
//!
//! Output is plain `.proto` text files so external tooling (e.g. Lovable)
//! can consume them without touching D-Bus, gRPC reflection, or SHM.
//!
//! Usage:
//!   cargo run -p op-grpc-bridge --bin dump-protos -- <output-dir>
//!   # default output: <workspace_root>/operation-dashboard-ui-07/proto

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use op_blob::catalog::{read_manifest_plugin_ids_shm, read_plugin_schema_shm, DEFAULT_SHM_DIR};
use op_grpc_bridge::plugin_grpc_gen::generate_plugin_method_protos;
use op_grpc_bridge::proto_gen::{ProtoGenConfig, ProtoGenerator};
use op_state_store::{PluginSchema, SchemaCatalog};

fn main() -> Result<()> {
    // Anchor on the workspace root so behavior doesn't depend on CWD.
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("workspace root is two levels above the crate dir")
        .to_path_buf();

    let out_dir: PathBuf = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace_root.join("operation-dashboard-ui-07/proto"));

    fs::create_dir_all(&out_dir)
        .with_context(|| format!("create output dir {}", out_dir.display()))?;

    let mut static_count = 0;
    let mut dynamic_count = 0;

    // ── 1. Static protos ────────────────────────────────────────────────
    // Walk every crate's `proto/` dir and copy `.proto` files through.
    let crates_dir = workspace_root.join("crates");
    if crates_dir.is_dir() {
        for entry in fs::read_dir(&crates_dir)? {
            let crate_dir = entry?.path();
            let proto_dir = crate_dir.join("proto");
            if !proto_dir.is_dir() {
                continue;
            }
            static_count += copy_proto_tree(&proto_dir, &out_dir)?;
        }
    }

    // ── 2. Dynamic plugin method protos from SHM blob catalog ───────────
    let dyn_dir = out_dir.join("plugin_methods");
    fs::create_dir_all(&dyn_dir)?;

    let plugin_ids = read_manifest_plugin_ids_shm().unwrap_or_default();
    if plugin_ids.is_empty() {
        eprintln!(
            "dump-protos: no active blobs in {} — skipping dynamic protos \
             (is the bridge running?)",
            DEFAULT_SHM_DIR
        );
    }

    let mut catalog = SchemaCatalog::new();
    let gen = ProtoGenerator::new(ProtoGenConfig::default());

    for plugin_id in &plugin_ids {
        let schema: PluginSchema = match read_plugin_schema_shm(plugin_id) {
            Some(s) => s,
            None => {
                eprintln!("dump-protos: could not read schema for {plugin_id}, skipping");
                continue;
            }
        };

        // Per-plugin typed method services (one service per method).
        let per_plugin_proto = generate_plugin_method_protos(plugin_id, &schema);
        let dest = dyn_dir.join(format!("{plugin_id}.proto"));
        fs::write(&dest, &per_plugin_proto).with_context(|| format!("write {}", dest.display()))?;
        dynamic_count += 1;

        // Also feed the unified catalog so the combined proto is available.
        catalog.register(schema);
    }

    if !plugin_ids.is_empty() {
        let unified = gen.generate_for_catalog(&catalog);
        let dest = out_dir.join("plugin_methods_unified.proto");
        fs::write(&dest, &unified).with_context(|| format!("write {}", dest.display()))?;
        dynamic_count += 1;
    }

    eprintln!(
        "dump-protos: wrote {static_count} static + {dynamic_count} dynamic proto files to {}",
        out_dir.display()
    );
    Ok(())
}

/// Recursively copy every `.proto` file from `src` into `dst`, preserving
/// relative paths. Returns the number of files copied.
fn copy_proto_tree(src: &Path, dst: &Path) -> Result<usize> {
    let mut count = 0;
    for entry in fs::read_dir(src)? {
        let path = entry?.path();
        if path.is_dir() {
            let rel = path.strip_prefix(src).unwrap();
            let sub_dst = dst.join(rel);
            fs::create_dir_all(&sub_dst)?;
            count += copy_proto_tree(&path, &sub_dst)?;
        } else if path.extension().and_then(|e| e.to_str()) == Some("proto") {
            let rel = path.strip_prefix(src).unwrap();
            let dest = dst.join(rel);
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent)?;
            }
            if dest.exists() {
                eprintln!(
                    "dump-protos: WARNING: {} already written by another crate — overwriting",
                    dest.display()
                );
            }
            fs::copy(&path, &dest)
                .with_context(|| format!("copy {} -> {}", path.display(), dest.display()))?;
            count += 1;
        }
    }
    Ok(count)
}
