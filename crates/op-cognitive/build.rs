fn main() -> Result<(), Box<dyn std::error::Error>> {
    let proto_file = "proto/cognitive.proto";

    tonic_build::configure()
        .build_server(true)
        .build_client(true)
        .file_descriptor_set_path(
            std::path::PathBuf::from(std::env::var("OUT_DIR")?).join("cognitive_descriptor.bin"),
        )
        .compile_protos(&[proto_file], &["proto/"])?;

    println!("cargo:rerun-if-changed={}", proto_file);
    Ok(())
}
