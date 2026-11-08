//! Compile wire protocols and native-handler compatibility evidence.
//! Plugin definitions remain the source; runtime never performs schema capture.

use op_state::StatePlugin;
use std::env;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Capture selectors shape the embedded fields as well as the methods.
    // Changing from a scratch capture to the release paths must rerun this build.
    for name in [
        "THREETCHED_BIN",
        "THREETCHED_MANIFEST",
        "TCHED_ROUTER_CONFIG_DIR",
        "THREETCHED_CAPTURE_BIN",
        "THREETCHED_CAPTURE_MANIFEST",
    ] {
        println!("cargo:rerun-if-env-changed={name}");
    }
    for name in ["THREETCHED_CAPTURE_BIN", "THREETCHED_CAPTURE_MANIFEST"] {
        if let Some(path) = env::var_os(name) {
            println!("cargo:rerun-if-changed={}", PathBuf::from(path).display());
        }
    }
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
    // Dynamic declarations use the sealer's verified, isolated capture, never
    // a runtime catalog pretending to be this binary's compiled contract.
    op_plugins::state_plugins::threetched::prepare_schema_capture()?;
    let registry = op_plugins::DefaultPluginRegistry::new(std::sync::Arc::new(
        op_state_store::MemoryStore::new(),
    ));
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
    let plugins = runtime.block_on(registry.load_all_plugins())?;
    let mut contracts = std::collections::BTreeMap::new();
    for plugin in plugins {
        let schema = plugin.schema().ok_or("native plugin has no compatibility contract")?;
        contracts.insert(schema.name.clone(), serde_json::json!({
            "version": schema.version, "fields": schema.fields, "methods": schema.methods,
        }));
    }
    std::fs::write(
        out_dir.join("compiled_method_contracts.json"),
        serde_json::to_vec(&contracts)?,
    )?;
    tonic_build::configure()
        .build_server(true)
        .build_client(true)
        .file_descriptor_set_path(out_dir.join("operation_descriptor.bin"))
        .compile_protos(&["proto/operation.proto"], &["proto"])?;
    // The broker's private protocol is not a public plugin service and must
    // not enter the door's reflection descriptor set.
    tonic_build::configure()
        .build_server(true)
        .build_client(false)
        .file_descriptor_set_path(out_dir.join("exhook_descriptor.bin"))
        .compile_protos(&["proto/emqx_exhook_v3.proto"], &["proto"])?;
    println!("cargo:rerun-if-changed=proto/operation.proto");
    println!("cargo:rerun-if-changed=proto/emqx_exhook_v3.proto");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../op-plugins/src/state_plugins");
    Ok(())
}
