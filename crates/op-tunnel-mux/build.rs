fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR")?);
    tonic_build::configure()
        .build_server(true)
        .build_client(true)
        .file_descriptor_set_path(out.join("tunnel_descriptor.bin"))
        .compile_protos(&["proto/tunnel.proto"], &["proto"])?;
    println!("cargo:rerun-if-changed=proto/tunnel.proto");
    println!("cargo:rerun-if-changed=build.rs");
    Ok(())
}
