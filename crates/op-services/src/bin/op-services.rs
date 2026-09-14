//! op-services daemon

use std::sync::Arc;
use tonic::transport::Server;
use tracing::info;
use tracing_subscriber::EnvFilter;

use op_services::grpc::proto::service_manager_server::ServiceManagerServer;
use op_services::grpc::server::GrpcServer;
use op_services::manager::ServiceManager;
use op_services::store::Store;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("op_services=info".parse()?))
        .init();

    info!("Starting op-services daemon");

    // Initialize store — JSON flat file, no SQLite, no drift.
    let store = Arc::new(Store::default_store().await?);

    // Initialize service manager
    let manager = Arc::new(ServiceManager::new(store).await?);

    // D-Bus projection is owned exclusively by op-grpc-bridge at
    // org.opdbus.v1.plugins. This daemon exposes only its gRPC compatibility
    // transport.
    let grpc_server = GrpcServer::new(manager);
    let addr = std::env::var("OP_SERVICES_GRPC_ADDR")
        .unwrap_or_else(|_| "[::]:50053".to_string())
        .parse()?;

    info!("gRPC server listening on {}", addr);

    Server::builder()
        .add_service(ServiceManagerServer::new(grpc_server))
        .serve(addr)
        .await?;

    Ok(())
}
