//! op-web: Main Entry Point
//!
//! Unified web server for op-dbus-v2.

use anyhow::Context;
use std::net::SocketAddr;
use std::sync::Arc;
use tracing::info;
use tracing_subscriber::{EnvFilter, FmtSubscriber};

use op_web::routes;
use op_web::AppState;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    op_web::install_tls_crypto_provider();

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
    // Tonic address on rtly-lungs. Never bind 0.0.0.0. The old page address
    // 10.0.0.4 is not an address on this interface.
    let bind_hosts =
        std::env::var("OP_WEB_BIND_ADDR").unwrap_or_else(|_| "10.0.0.2".to_string());
    let mut listeners = Vec::new();
    for bind_host in bind_hosts
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        let bind_ip: std::net::IpAddr = bind_host
            .parse()
            .with_context(|| format!("parsing OP_WEB_BIND_ADDR entry={bind_host}"))?;
        let addr = SocketAddr::new(bind_ip, port);
        info!("Listening on http://{}", addr);
        info!("WebSocket available at ws://{}/ws", addr);
        info!("API available at http://{}/api", addr);
        listeners.push(tokio::net::TcpListener::bind(addr).await?);
    }
    anyhow::ensure!(
        !listeners.is_empty(),
        "OP_WEB_BIND_ADDR must contain at least one HTTP bind address"
    );
    let http_servers = async {
        let mut set = tokio::task::JoinSet::new();
        for listener in listeners {
            let app = app.clone();
            set.spawn(async move {
                axum::serve(
                    listener,
                    app.into_make_service_with_connect_info::<SocketAddr>(),
                )
                .await
            });
        }
        while let Some(result) = set.join_next().await {
            result??;
        }
        Ok::<(), anyhow::Error>(())
    };

    // Optional HTTPS listener for the same router, such as the page address
    // 10.0.0.4:443 with its internal-CA certificate. A configured bind without
    // a readable certificate pair stops startup.
    let Some(tls_bind) = std::env::var("OP_WEB_TLS_BIND")
        .ok()
        .filter(|value| !value.trim().is_empty())
    else {
        http_servers.await?;
        return Ok(());
    };
    let tls_addr: SocketAddr = tls_bind
        .trim()
        .parse()
        .with_context(|| format!("parsing OP_WEB_TLS_BIND={tls_bind}"))?;
    let cert_path = std::env::var("OP_WEB_TLS_CERT_FILE")
        .context("OP_WEB_TLS_BIND requires OP_WEB_TLS_CERT_FILE")?;
    let key_path = std::env::var("OP_WEB_TLS_KEY_FILE")
        .context("OP_WEB_TLS_BIND requires OP_WEB_TLS_KEY_FILE")?;
    let tls = op_grpc_bridge::PemIdentity::from_pem(
        std::fs::read(&cert_path).with_context(|| format!("reading {cert_path}"))?,
        std::fs::read(&key_path).with_context(|| format!("reading {key_path}"))?,
    )
    .server_config()?;
    let tls_listener = tokio::net::TcpListener::bind(tls_addr)
        .await
        .with_context(|| format!("binding {tls_addr}"))?;
    info!("Listening on https://{}", tls_addr);

    tokio::select! {
        result = http_servers => result?,
        result = op_grpc_bridge::serve_upgradeable_tcp(tls_listener, tls, app.clone()) => result?,
    }
    Ok(())
}
