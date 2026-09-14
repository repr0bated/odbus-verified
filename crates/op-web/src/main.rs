//! op-web: Main Entry Point
//!
//! Unified web server for op-dbus-v2.

use std::net::SocketAddr;
use std::sync::Arc;
use tracing::info;
use tracing_subscriber::{EnvFilter, FmtSubscriber};

use op_web::routes;
use op_web::AppState;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging with environment filter (RUST_LOG)
    // Default to info if not set
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    let _ = FmtSubscriber::builder()
        .with_env_filter(filter)
        .with_target(false)
        .with_thread_ids(false)
        .compact()
        .try_init();

    info!("Starting op-web server...");
    info!("Version: {}", env!("CARGO_PKG_VERSION"));

    // Absolute Base: the sealed SHM blob catalog is the current schema
    // authority. `/dev/shm/live-schema.json` was the retired monolithic form.
    if let Err(error) = op_blob::catalog::BlobStore::open_default() {
        tracing::error!(
            path = op_blob::catalog::DEFAULT_SHM_DIR,
            %error,
            "FATAL: sealed schema catalog missing from shared memory"
        );
        std::process::exit(1);
    }
    info!(
        path = op_blob::catalog::DEFAULT_SHM_DIR,
        "Sealed schema catalog present"
    );

    let state = Arc::new(AppState::new().await?);
    state.clone().start_event_bridge();
    let app = routes::create_router(state);

    let port = std::env::var("PORT")
        .unwrap_or_else(|_| "8080".to_string())
        .parse::<u16>()
        .unwrap_or(8080);
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    info!("Listening on http://{}", addr);
    info!("WebSocket available at ws://{}/ws", addr);
    info!("API available at http://{}/api", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;

    Ok(())
}
