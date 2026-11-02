//! Compile the authored wire contract and identity-reader compatibility evidence.
//! No live-host schema capture, generated plugin protos, or environment probing.

use op_state::StatePlugin;
use std::env;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out_dir = PathBuf::from(env::var("OUT_DIR")?);
    // Compatibility evidence for the compiled identity reader. Plugin existence
    // and current schema remain determined by the sealed runtime catalog.
    let identity = op_plugins::state_plugins::identity_sled::IdentitySledPlugin::new()
        .schema()
        .ok_or("identity_sled has no compiled schema")?;
    let contract = serde_json::json!({"version": identity.version, "fields": identity.fields});
    std::fs::write(
        out_dir.join("identity_state_contract.json"),
        serde_json::to_vec(&contract)?,
    )?;
    tonic_build::configure()
        .build_server(true)
        .build_client(true)
        .file_descriptor_set_path(out_dir.join("operation_descriptor.bin"))
        .compile_protos(&["proto/operation.proto"], &["proto"])?;
    println!("cargo:rerun-if-changed=proto/operation.proto");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../op-plugins/src/state_plugins");
    Ok(())
}
