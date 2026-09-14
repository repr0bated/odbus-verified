//! gRPC server setup and configuration

use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Result;
use tonic::transport::Server;
use tonic_web::enable as tw_enable;
use tracing::info;

use super::agent_service::AgentServiceImpl;
use super::cache_service::CacheServiceImpl;
use super::mcp_service::McpServiceImpl;
use super::orchestrator_service::OrchestratorServiceImpl;
use super::proto::{
    agent_service_server::AgentServiceServer, cache_service_server::CacheServiceServer,
    mcp_service_server::McpServiceServer, orchestrator_service_server::OrchestratorServiceServer,
    FILE_DESCRIPTOR_SET,
};

/// Server configuration
#[derive(Debug, Clone)]
pub struct GrpcServerConfig {
    pub listen_addr: SocketAddr,
    pub workstack_threshold: usize,
    pub enable_caching: bool,
    pub promotion_threshold: u32,
    pub default_cache_ttl_secs: i64,
}

impl Default for GrpcServerConfig {
    fn default() -> Self {
        Self {
            listen_addr: "[::1]:50051".parse().unwrap(),
            workstack_threshold: 2,
            enable_caching: true,
            promotion_threshold: 3,
            default_cache_ttl_secs: 3600,
        }
    }
}

/// gRPC server builder
pub struct GrpcServer {
    config: GrpcServerConfig,
    agent_service: Arc<AgentServiceImpl>,
    cache_service: Arc<CacheServiceImpl>,
    orchestrator_service: Arc<OrchestratorServiceImpl>,
    mcp_service: Arc<McpServiceImpl>,
}

impl GrpcServer {
    /// Create new gRPC server with default configuration
    pub fn new() -> Self {
        Self::with_config(GrpcServerConfig::default())
    }

    /// Create new gRPC server with custom configuration
    pub fn with_config(config: GrpcServerConfig) -> Self {
        let agent_service = Arc::new(AgentServiceImpl::new());
        let cache_service = Arc::new(CacheServiceImpl::with_ttl(config.default_cache_ttl_secs));
        let orchestrator_service = Arc::new(OrchestratorServiceImpl::with_config(
            agent_service.clone(),
            cache_service.clone(),
            config.workstack_threshold,
            config.enable_caching,
            config.promotion_threshold,
        ));
        let mcp_service = Arc::new(McpServiceImpl::new(
            agent_service.clone(),
            orchestrator_service.clone(),
        ));

        Self {
            config,
            agent_service,
            cache_service,
            orchestrator_service,
            mcp_service,
        }
    }

    /// Get agent service for local registration
    pub fn agent_service(&self) -> Arc<AgentServiceImpl> {
        self.agent_service.clone()
    }

    /// Get orchestrator service
    pub fn orchestrator_service(&self) -> Arc<OrchestratorServiceImpl> {
        self.orchestrator_service.clone()
    }

    /// Get cache service
    pub fn cache_service(&self) -> Arc<CacheServiceImpl> {
        self.cache_service.clone()
    }

    /// Get MCP service
    pub fn mcp_service(&self) -> Arc<McpServiceImpl> {
        self.mcp_service.clone()
    }

    /// Start the gRPC server (with gRPC-Web, reflection, and health)
    pub async fn serve(self) -> Result<()> {
        let addr = self.config.listen_addr;

        info!("Starting gRPC server on {}", addr);

        let reflection = tonic_reflection::server::Builder::configure()
            .register_encoded_file_descriptor_set(FILE_DESCRIPTOR_SET)
            .build_v1()
            .expect("failed to build cache reflection service");

        let (mut health_reporter, health_service) = tonic_health::server::health_reporter();
        health_reporter
            .set_serving::<AgentServiceServer<AgentServiceImpl>>()
            .await;
        health_reporter
            .set_serving::<CacheServiceServer<CacheServiceImpl>>()
            .await;
        health_reporter
            .set_serving::<OrchestratorServiceServer<OrchestratorServiceImpl>>()
            .await;
        health_reporter
            .set_serving::<McpServiceServer<McpServiceImpl>>()
            .await;

        Server::builder()
            .accept_http1(true)
            .add_service(tw_enable(AgentServiceServer::from_arc(self.agent_service)))
            .add_service(tw_enable(CacheServiceServer::from_arc(self.cache_service)))
            .add_service(tw_enable(OrchestratorServiceServer::from_arc(
                self.orchestrator_service,
            )))
            .add_service(tw_enable(McpServiceServer::from_arc(self.mcp_service)))
            .add_service(tw_enable(reflection))
            .add_service(tw_enable(health_service))
            .serve(addr)
            .await?;

        Ok(())
    }

    /// Serve with graceful shutdown (with gRPC-Web, reflection, and health)
    pub async fn serve_with_shutdown(
        self,
        shutdown: impl std::future::Future<Output = ()>,
    ) -> Result<()> {
        let addr = self.config.listen_addr;

        info!("Starting gRPC server on {} (with graceful shutdown)", addr);

        let reflection = tonic_reflection::server::Builder::configure()
            .register_encoded_file_descriptor_set(FILE_DESCRIPTOR_SET)
            .build_v1()
            .expect("failed to build cache reflection service");

        let (mut health_reporter, health_service) = tonic_health::server::health_reporter();
        health_reporter
            .set_serving::<AgentServiceServer<AgentServiceImpl>>()
            .await;
        health_reporter
            .set_serving::<CacheServiceServer<CacheServiceImpl>>()
            .await;
        health_reporter
            .set_serving::<OrchestratorServiceServer<OrchestratorServiceImpl>>()
            .await;
        health_reporter
            .set_serving::<McpServiceServer<McpServiceImpl>>()
            .await;

        Server::builder()
            .accept_http1(true)
            .add_service(tw_enable(AgentServiceServer::from_arc(self.agent_service)))
            .add_service(tw_enable(CacheServiceServer::from_arc(self.cache_service)))
            .add_service(tw_enable(OrchestratorServiceServer::from_arc(
                self.orchestrator_service,
            )))
            .add_service(tw_enable(McpServiceServer::from_arc(self.mcp_service)))
            .add_service(tw_enable(reflection))
            .add_service(tw_enable(health_service))
            .serve_with_shutdown(addr, shutdown)
            .await?;

        Ok(())
    }
}

impl Default for GrpcServer {
    fn default() -> Self {
        Self::new()
    }
}
