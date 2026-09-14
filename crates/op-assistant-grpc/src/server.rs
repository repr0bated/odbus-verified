//! Tonic gRPC server wiring. Registers every Assistant gateway service,
//! reflection, health checks, the WireGuard auth interceptor, and an optional
//! gRPC-Web layer for browser clients.
//!
//! Memory / soul / namespace services talk to cognitive-mcp over the session
//! bus (`PluginV1.Call`). This process never opens Cozo RocksDB.

use crate::agents::AgentServiceImpl;
use crate::auth::wireguard_auth_interceptor;
use crate::client::AssistantClient;
use crate::cognitive_client::{default_cognitive_bus_address, CognitivePluginClient};
use crate::cron::CronServiceImpl;
use crate::memory::MemoryServiceImpl;
use crate::models::ModelServiceImpl;
use crate::namespace::NamespaceMemoryServiceImpl;
use crate::proto::agent_service_server::AgentServiceServer;
use crate::proto::cron_service_server::CronServiceServer;
use crate::proto::memory_service_server::MemoryServiceServer;
use crate::proto::model_service_server::ModelServiceServer;
use crate::proto::namespace_memory_service_server::NamespaceMemoryServiceServer;
use crate::proto::session_service_server::SessionServiceServer;
use crate::proto::soul_service_server::SoulServiceServer;
use crate::proto::task_service_server::TaskServiceServer;
use crate::sessions::SessionServiceImpl;
use crate::soul::SoulServiceImpl;
use crate::tasks::TaskServiceImpl;
use crate::transport::TransportConfig;
use std::net::SocketAddr;
use std::sync::Arc;
use tonic::transport::Server;
use tracing::info;

pub const DEFAULT_GRPC_PORT: u16 = 50051;
pub const DEFAULT_GRPC_HOST: &str = "0.0.0.0";

#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub transport: TransportConfig,
    pub enable_grpc_web: bool,
    pub enable_reflection: bool,
    /// Session-bus address for `PluginV1.Call` on cognitive_mcp.
    /// Default: `DBUS_SESSION_BUS_ADDRESS` or `unix:path=/run/opdbus/session-bus.sock`.
    pub cognitive_bus_address: String,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: std::env::var("OP_ASSISTANT_GRPC_HOST")
                .unwrap_or_else(|_| DEFAULT_GRPC_HOST.to_string()),
            port: std::env::var("OP_ASSISTANT_GRPC_PORT")
                .ok()
                .and_then(|p| p.parse().ok())
                .unwrap_or(DEFAULT_GRPC_PORT),
            transport: TransportConfig::default(),
            enable_grpc_web: std::env::var("OP_ASSISTANT_GRPC_WEB")
                .map(|v| matches!(v.as_str(), "1" | "true" | "yes"))
                .unwrap_or(false),
            enable_reflection: true,
            cognitive_bus_address: default_cognitive_bus_address(),
        }
    }
}

pub struct AssistantGrpcServer {
    cfg: ServerConfig,
    client: AssistantClient,
    cognitive: Arc<CognitivePluginClient>,
}

impl AssistantGrpcServer {
    pub async fn new(cfg: ServerConfig) -> anyhow::Result<Self> {
        let client = AssistantClient::new(cfg.transport.clone()).await?;
        let cognitive = Arc::new(CognitivePluginClient::connect(&cfg.cognitive_bus_address).await?);

        Ok(Self {
            cfg,
            client,
            cognitive,
        })
    }

    pub fn client(&self) -> &AssistantClient {
        &self.client
    }

    pub async fn serve(self) -> anyhow::Result<()> {
        let addr: SocketAddr = format!("{}:{}", self.cfg.host, self.cfg.port).parse()?;
        info!(
            %addr,
            transport = ?self.client.transport().primary_kind(),
            cognitive_bus = %self.cfg.cognitive_bus_address,
            "starting op-assistant-grpc"
        );
        // Publish D-Bus interface so model calls work over the session bus.
        let _dbus_conn = crate::dbus_service::serve(Arc::new(self.client.clone())).await?;

        let agent = AgentServiceServer::with_interceptor(
            AgentServiceImpl::new(self.client.clone()),
            wireguard_auth_interceptor,
        );
        let session = SessionServiceServer::with_interceptor(
            SessionServiceImpl::new(self.client.clone()),
            wireguard_auth_interceptor,
        );
        let task = TaskServiceServer::with_interceptor(
            TaskServiceImpl::new(self.client.clone()),
            wireguard_auth_interceptor,
        );
        let model = ModelServiceServer::with_interceptor(
            ModelServiceImpl::new(self.client.clone()),
            wireguard_auth_interceptor,
        );
        let cron = CronServiceServer::with_interceptor(
            CronServiceImpl::new(self.client.clone()),
            wireguard_auth_interceptor,
        );
        let soul = SoulServiceServer::with_interceptor(
            SoulServiceImpl::from_client(self.cognitive.clone()),
            wireguard_auth_interceptor,
        );
        let namespace = NamespaceMemoryServiceServer::with_interceptor(
            NamespaceMemoryServiceImpl::from_client(self.cognitive.clone()),
            wireguard_auth_interceptor,
        );
        let memory = MemoryServiceServer::with_interceptor(
            MemoryServiceImpl::from_client(self.cognitive.clone()),
            wireguard_auth_interceptor,
        );

        let mut builder = Server::builder()
            .accept_http1(true)
            .add_service(tonic_web::enable(agent))
            .add_service(tonic_web::enable(session))
            .add_service(tonic_web::enable(task))
            .add_service(tonic_web::enable(model))
            .add_service(tonic_web::enable(cron))
            .add_service(tonic_web::enable(soul))
            .add_service(tonic_web::enable(namespace))
            .add_service(tonic_web::enable(memory));

        if self.cfg.enable_reflection {
            let reflection = tonic_reflection::server::Builder::configure()
                .register_encoded_file_descriptor_set(crate::proto::FILE_DESCRIPTOR_SET)
                .build_v1()?;
            builder = builder.add_service(tonic_web::enable(reflection));
        }

        let (mut health_reporter, health_service) = tonic_health::server::health_reporter();
        health_reporter
            .set_serving::<AgentServiceServer<AgentServiceImpl>>()
            .await;
        builder = builder.add_service(tonic_web::enable(health_service));

        builder.serve(addr).await?;
        Ok(())
    }
}

/// Convenience entry-point used by `op-dbus` and integration tests.
pub async fn run_grpc_server(cfg: ServerConfig) -> anyhow::Result<()> {
    AssistantGrpcServer::new(cfg).await?.serve().await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_defaults_populate() {
        let cfg = ServerConfig::default();
        assert!(cfg.port > 0);
        assert!(!cfg.host.is_empty());
        assert!(!cfg.cognitive_bus_address.is_empty());
        assert!(
            cfg.cognitive_bus_address.contains("unix:path=")
                || cfg.cognitive_bus_address.contains("unix:abstract=")
                || cfg.cognitive_bus_address.contains("tcp:"),
            "cognitive bus address should be a D-Bus socket, got {}",
            cfg.cognitive_bus_address
        );
    }
}
