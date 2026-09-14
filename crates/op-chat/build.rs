//! Build script for op-chat
//!
//! Compiles protocol buffer files for gRPC services.

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());

    let proto_files = ["proto/orchestration.proto", "proto/chat.proto"];
    let mut any_compiled = false;

    for proto_path in &proto_files {
        if !std::path::Path::new(proto_path).exists() {
            continue;
        }
        any_compiled = true;
    }

    if !any_compiled {
        eprintln!("cargo:warning=No proto files found, skipping gRPC codegen");
        return Ok(());
    }

    let existing: Vec<&str> = proto_files
        .iter()
        .copied()
        .filter(|p| std::path::Path::new(p).exists())
        .collect();

    tonic_build::configure()
        .build_server(true)
        .build_client(true)
        .file_descriptor_set_path(out.join("op_chat_descriptor.bin"))
        .out_dir("src/orchestration/proto")
        .compile(&existing, &["proto"])?;

    for proto_path in &existing {
        println!("cargo:rerun-if-changed={}", proto_path);
    }

    Ok(())
}
