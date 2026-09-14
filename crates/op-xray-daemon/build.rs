//! Build script for op-xray-daemon.
//!
//! Compiles the vendored xray-core commander protos (StatsService,
//! RoutingService, LoggerService, HandlerService and its typed payloads — see
//! proto/VENDORED.md for provenance). These are upstream backing protocols,
//! not a separately authored OP-DBUS plugin service.

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let proto_files = [
        "proto/app/stats/command/command.proto",
        "proto/app/router/command/command.proto",
        "proto/app/log/command/config.proto",
        "proto/app/proxyman/command/command.proto",
        "proto/app/proxyman/config.proto",
        "proto/proxy/vless/account.proto",
        "proto/proxy/vless/inbound/config.proto",
        "proto/proxy/vmess/account.proto",
        "proto/proxy/vmess/inbound/config.proto",
        "proto/proxy/trojan/config.proto",
        "proto/transport/internet/tls/config.proto",
    ];

    tonic_build::configure()
        // The server types support isolated mock-transport tests. No upstream
        // HandlerService is registered on the public OP-DBUS listener.
        .build_server(true)
        .build_client(true)
        .compile_protos(&proto_files, &["proto"])?;

    println!("cargo:rerun-if-changed=proto");

    Ok(())
}
