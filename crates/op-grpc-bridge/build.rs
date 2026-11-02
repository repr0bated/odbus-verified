//! Fresh build: compile ONLY the authored wire contract.
//! No live-host schema capture, no generated plugin protos, no env probing.

use std::env;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out_dir = PathBuf::from(env::var("OUT_DIR")?);
    tonic_build::configure()
        .build_server(true)
        .build_client(true)
        .file_descriptor_set_path(out_dir.join("operation_descriptor.bin"))
        .compile_protos(&["proto/operation.proto"], &["proto"])?;
    println!("cargo:rerun-if-changed=proto/operation.proto");
    println!("cargo:rerun-if-changed=build.rs");
    Ok(())
}
