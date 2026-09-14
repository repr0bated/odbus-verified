//! Build script for op-mcp
//!
//! Compiles proto files when the grpc feature is enabled.

fn main() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(feature = "grpc")]
    {
        let proto_file = "proto/mcp.proto";

        // Check if proto file exists
        if std::path::Path::new(proto_file).exists() {
            println!("cargo:rerun-if-changed={}", proto_file);

            let out_dir = std::path::PathBuf::from(std::env::var("OUT_DIR")?);

            tonic_build::configure()
                .build_server(true)
                .build_client(true)
                .file_descriptor_set_path(out_dir.join("mcp_descriptor.bin"))
                .compile_protos(&[proto_file], &["proto"])?;

            println!("cargo:warning=gRPC proto compiled successfully");
        } else {
            println!("cargo:warning=Proto file not found: {}", proto_file);
        }
    }

    Ok(())
}
