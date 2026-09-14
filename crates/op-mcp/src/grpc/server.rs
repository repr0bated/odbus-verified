//! gRPC Server Transport with Infrastructure Integration

#[cfg(feature = "grpc")]
use crate::grpc::proto::{mcp_service_server::McpServiceServer, FILE_DESCRIPTOR_SET};
#[cfg(feature = "grpc")]
use crate::grpc::service::{GrpcInfrastructure, McpGrpcService};
use crate::ServerMode; // Unified ServerMode from lib.rs
use anyhow::Result;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;
#[cfg(feature = "grpc")]
use tonic::transport::Server;
use tracing::{error, info};

/// gRPC transport configuration
#[derive(Debug, Clone)]
pub struct GrpcConfig {
    pub address: SocketAddr,
    pub mode: ServerMode,
    pub tls_enabled: bool,
    pub tls_cert_path: Option<String>,
    pub tls_key_path: Option<String>,
    pub request_timeout: Duration,
    pub max_message_size: usize,
    pub enable_reflection: bool,
    pub enable_health: bool,
    pub max_concurrent_streams: u32,
    pub keepalive_interval: Duration,
    pub keepalive_timeout: Duration,
    pub cache_path: Option<PathBuf>,
    pub state_db_path: Option<PathBuf>,
    pub snowball_path: Option<PathBuf>,
}

impl Default for GrpcConfig {
    fn default() -> Self {
        Self {
            address: "[::1]:50051".parse().unwrap(),
            mode: ServerMode::Compact,
            tls_enabled: false,
            tls_cert_path: None,
            tls_key_path: None,
            request_timeout: Duration::from_secs(30),
            max_message_size: 16 * 1024 * 1024,
            enable_reflection: true,
            enable_health: true,
            max_concurrent_streams: 100,
            keepalive_interval: Duration::from_secs(30),
            keepalive_timeout: Duration::from_secs(10),
            cache_path: Some(PathBuf::from("/var/lib/op-dbus/cache/grpc")),
            state_db_path: Some(PathBuf::from("/var/lib/op-dbus/state/grpc.db")),
            snowball_path: Some(PathBuf::from("/var/lib/op-dbus/snowball/grpc")),
        }
    }
}

impl GrpcConfig {
    pub fn with_address(mut self, addr: SocketAddr) -> Self {
        self.address = addr;
        self
    }

    pub fn with_mode(mut self, mode: ServerMode) -> Self {
        self.mode = mode;
        self
    }

    pub fn with_tls(mut self, cert_path: impl Into<String>, key_path: impl Into<String>) -> Self {
        self.tls_enabled = true;
        self.tls_cert_path = Some(cert_path.into());
        self.tls_key_path = Some(key_path.into());
        self
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.request_timeout = timeout;
        self
    }

    pub fn with_cache_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.cache_path = Some(path.into());
        self
    }

    pub fn with_state_db_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.state_db_path = Some(path.into());
        self
    }

    pub fn with_snowball_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.snowball_path = Some(path.into());
        self
    }

    pub fn without_infrastructure(mut self) -> Self {
        self.cache_path = None;
        self.state_db_path = None;
        self.snowball_path = None;
        self
    }
}

/// gRPC transport for MCP server
#[cfg(feature = "grpc")]
pub struct GrpcTransport {
    config: GrpcConfig,
    service: McpGrpcService,
}

#[cfg(feature = "grpc")]
impl GrpcTransport {
    pub async fn new(config: GrpcConfig) -> Result<Self> {
        let infrastructure = GrpcInfrastructure::from_paths(
            config.cache_path.clone(),
            config.state_db_path.clone(),
            config.snowball_path.clone(),
        )
        .await?;

        let service = McpGrpcService::with_infrastructure(config.mode, infrastructure);

        Ok(Self { config, service })
    }

    pub async fn with_infrastructure(
        config: GrpcConfig,
        infrastructure: GrpcInfrastructure,
    ) -> Result<Self> {
        let service = McpGrpcService::with_infrastructure(config.mode, infrastructure);
        Ok(Self { config, service })
    }

    pub async fn with_defaults() -> Result<Self> {
        Self::new(GrpcConfig::default()).await
    }

    pub async fn without_infrastructure() -> Result<Self> {
        let config = GrpcConfig::default().without_infrastructure();
        let service = McpGrpcService::new(config.mode);
        Ok(Self { config, service })
    }

    pub async fn serve(self) -> Result<()> {
        let addr = self.config.address;

        info!(
            address = %addr,
            mode = %self.config.mode,
            tls = %self.config.tls_enabled,
            reflection = %self.config.enable_reflection,
            health = %self.config.enable_health,
            "Starting gRPC MCP server"
        );

        let mcp_service = McpServiceServer::new(self.service)
            .max_decoding_message_size(self.config.max_message_size)
            .max_encoding_message_size(self.config.max_message_size);

        let mut builder = Server::builder()
            .accept_http1(true)
            .timeout(self.config.request_timeout)
            .max_concurrent_streams(self.config.max_concurrent_streams)
            .http2_keepalive_interval(Some(self.config.keepalive_interval))
            .http2_keepalive_timeout(Some(self.config.keepalive_timeout))
            .add_service(tonic_web::enable(mcp_service));

        if self.config.enable_reflection {
            let reflection = tonic_reflection::server::Builder::configure()
                .register_encoded_file_descriptor_set(FILE_DESCRIPTOR_SET)
                .build_v1()
                .map_err(|e| {
                    error!(error = %e, "reflection build failed");
                    anyhow::anyhow!("reflection build failed: {}", e)
                })?;
            builder = builder.add_service(tonic_web::enable(reflection));
        }

        if self.config.enable_health {
            let (mut health_reporter, health_service) = tonic_health::server::health_reporter();
            health_reporter
                .set_serving::<McpServiceServer<McpGrpcService>>()
                .await;
            builder = builder.add_service(tonic_web::enable(health_service));
        }

        builder.serve(addr).await.map_err(|e| {
            error!(error = %e, "gRPC server error");
            anyhow::anyhow!("gRPC server error: {}", e)
        })?;

        Ok(())
    }

    pub async fn serve_with_shutdown<F>(self, shutdown: F) -> Result<()>
    where
        F: std::future::Future<Output = ()>,
    {
        let addr = self.config.address;

        info!(
            address = %addr,
            reflection = %self.config.enable_reflection,
            health = %self.config.enable_health,
            "Starting gRPC MCP server with graceful shutdown"
        );

        let mcp_service = McpServiceServer::new(self.service)
            .max_decoding_message_size(self.config.max_message_size)
            .max_encoding_message_size(self.config.max_message_size);

        let mut builder = Server::builder()
            .accept_http1(true)
            .timeout(self.config.request_timeout)
            .add_service(tonic_web::enable(mcp_service));

        if self.config.enable_reflection {
            let reflection = tonic_reflection::server::Builder::configure()
                .register_encoded_file_descriptor_set(FILE_DESCRIPTOR_SET)
                .build_v1()
                .map_err(|e| {
                    error!(error = %e, "reflection build failed");
                    anyhow::anyhow!("reflection build failed: {}", e)
                })?;
            builder = builder.add_service(tonic_web::enable(reflection));
        }

        if self.config.enable_health {
            let (mut health_reporter, health_service) = tonic_health::server::health_reporter();
            health_reporter
                .set_serving::<McpServiceServer<McpGrpcService>>()
                .await;
            builder = builder.add_service(tonic_web::enable(health_service));
        }

        builder.serve_with_shutdown(addr, shutdown).await?;

        info!("gRPC server shut down gracefully");
        Ok(())
    }
}

#[cfg(feature = "grpc")]
#[allow(dead_code)]
pub async fn run_grpc_server(config: GrpcConfig) -> Result<()> {
    let transport = GrpcTransport::new(config).await?;
    transport.serve().await
}

#[cfg(feature = "grpc")]
#[allow(dead_code)]
pub async fn run_grpc_server_lightweight(address: SocketAddr, mode: ServerMode) -> Result<()> {
    let _config = GrpcConfig::default()
        .with_address(address)
        .with_mode(mode)
        .without_infrastructure();

    let service = McpGrpcService::new(mode);

    info!(address = %address, mode = %mode, "Starting lightweight gRPC server");

    Server::builder()
        .accept_http1(true)
        .add_service(tonic_web::enable(McpServiceServer::new(service)))
        .serve(address)
        .await?;

    Ok(())
}
