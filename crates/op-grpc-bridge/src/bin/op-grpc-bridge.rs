//! Consolidated Operation gRPC bridge.
//!
//! The former `op-grpc-bridge-zeroclaw` entry point is folded into this main
//! service. It exposes the complete Operation route set plus the 3tched Router
//! schema API on TCP port 8090 and the plugin Unix socket.

use op_grpc_bridge::model_providers_validation;
use op_grpc_bridge::server::{run_tched_router_server, ServerConfig};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .init();

    let config = ServerConfig::from_env();

    // Validate model-providers.toml at startup to catch config errors early
    if let Ok(env_str) = std::env::var("MODEL_PROVIDERS_PATH") {
        if let Ok(toml_str) = std::fs::read_to_string(&env_str) {
            if let Ok(providers) = toml::from_str::<
                op_plugins::state_plugins::model_registry::ProvidersFile,
            >(&toml_str)
            {
                if let Err(e) =
                    model_providers_validation::validate_providers_config(&providers.providers)
                {
                    tracing::error!(error = %e, "Model providers configuration validation failed");
                    return Err(e);
                }
            }
        }
    }

    tracing::info!(
        plugin_id = %config.plugin_id,
        schema_path = %config.schema_path.display(),
        unix_socket = %config.unix_socket.display(),
        shared_socket = %config.shared_socket.display(),
        bind_addr = %config.bind_addr,
        tls = config.tls_identity.is_some(),
        "Consolidated Operation gRPC bridge starting (tonic TLS MQTT/gRPC demux)"
    );
    run_tched_router_server(config).await
}
