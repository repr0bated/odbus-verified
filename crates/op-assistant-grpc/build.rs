use std::env;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    let protos = [
        "proto/assistant/common.proto",
        "proto/assistant/agent.proto",
        "proto/assistant/session.proto",
        "proto/assistant/task.proto",
        "proto/assistant/model.proto",
        "proto/assistant/cron.proto",
        "proto/assistant/soul.proto",
        "proto/assistant/namespace.proto",
        "proto/assistant/memory.proto",
    ];

    tonic_build::configure()
        .build_server(true)
        .build_client(true)
        .file_descriptor_set_path(out_dir.join("assistant_descriptor.bin"))
        .compile_protos(&protos, &["proto"])?;

    for p in &protos {
        println!("cargo:rerun-if-changed={}", p);
    }
    println!("cargo:rerun-if-changed=build.rs");

    Ok(())
}
