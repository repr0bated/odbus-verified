//! Thin tonic server: the three core services plus gRPC reflection.

use crate::core::BridgeCore;
use crate::proto::event_chain_service_server::EventChainServiceServer;
use crate::proto::plugin_service_server::PluginServiceServer;
use crate::proto::state_sync_server::StateSyncServer;
use crate::proto::FILE_DESCRIPTOR_SET;
use crate::service::BridgeService;
use std::net::SocketAddr;

/// Serve `StateSync`, `PluginService` and `EventChainService` (plus
/// reflection) on `addr`. Runs until the server errors.
pub async fn serve(addr: SocketAddr, core: BridgeCore) -> anyhow::Result<()> {
    let service = BridgeService::new(core);
    let reflection = tonic_reflection::server::Builder::configure()
        .register_encoded_file_descriptor_set(FILE_DESCRIPTOR_SET)
        .build_v1()?;
    tonic::transport::Server::builder()
        .add_service(StateSyncServer::new(service.clone()))
        .add_service(PluginServiceServer::new(service.clone()))
        .add_service(EventChainServiceServer::new(service))
        .add_service(reflection)
        .serve(addr)
        .await?;
    Ok(())
}
