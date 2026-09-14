//! Central HTTP/TLS Server Implementation
//!
//! Single server that handles all HTTP/HTTPS traffic for op-dbus.

use crate::middleware::{apply_middleware, MiddlewareConfig};
use crate::tls::TlsConfig;
use crate::{Result, ServerError};
use axum::Router;
use hyper::server::conn::http1;
use hyper_util::rt::TokioIo;
use hyper_util::service::TowerToHyperService;
use std::net::SocketAddr;
use tokio::net::TcpListener;
use tracing::info;

/// Server configuration
#[derive(Clone, Debug)]
pub struct ServerConfig {
    /// HTTP port
    pub http_port: u16,
    /// HTTPS port (if TLS enabled)
    pub https_port: u16,
    /// Bind host
    pub bind_host: String,
    /// Public hostname for logging/display
    pub public_host: String,
    /// TLS configuration
    pub tls: TlsConfig,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            http_port: 8080,
            https_port: 8443,
            bind_host: "0.0.0.0".to_string(),
            public_host: gethostname::gethostname().to_string_lossy().to_string(),
            tls: TlsConfig::default(),
        }
    }
}

/// Central HTTP Server
pub struct HttpServer {
    config: ServerConfig,
    router: Router,
}

impl HttpServer {
    /// Create a new server builder
    pub fn builder() -> HttpServerBuilder {
        HttpServerBuilder::new()
    }

    /// Get server configuration
    pub fn config(&self) -> &ServerConfig {
        &self.config
    }

    /// Start the server
    pub async fn serve(self) -> Result<()> {
        let http_addr: SocketAddr = format!("{}:{}", self.config.bind_host, self.config.http_port)
            .parse()
            .map_err(|_| {
                ServerError::BindError(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "Invalid HTTP bind address",
                ))
            })?;

        // Try to build TLS acceptor
        let tls_acceptor = self.config.tls.build_acceptor()?;

        if let Some(acceptor) = tls_acceptor {
            // HTTPS mode - serve on both HTTP and HTTPS
            let https_addr: SocketAddr =
                format!("{}:{}", self.config.bind_host, self.config.https_port)
                    .parse()
                    .map_err(|_| {
                        ServerError::BindError(std::io::Error::new(
                            std::io::ErrorKind::InvalidInput,
                            "Invalid HTTPS bind address",
                        ))
                    })?;

            // Start HTTP server in background
            let http_router = self.router.clone();
            tokio::spawn(async move {
                let listener = match TcpListener::bind(http_addr).await {
                    Ok(l) => l,
                    Err(e) => {
                        tracing::error!("Failed to bind HTTP: {}", e);
                        return;
                    }
                };
                info!("HTTP server listening on http://{}", http_addr);
                let _ = axum::serve(listener, http_router).await;
            });

            // Start HTTPS server (main thread)
            let listener = TcpListener::bind(https_addr)
                .await
                .map_err(ServerError::BindError)?;

            info!("HTTPS server listening on https://{}", https_addr);
            info!(
                "Public URL: https://{}:{}",
                self.config.public_host, self.config.https_port
            );

            loop {
                let (stream, peer_addr) =
                    listener.accept().await.map_err(ServerError::BindError)?;
                let acceptor = acceptor.clone();
                let router = self.router.clone();

                tokio::spawn(async move {
                    match acceptor.accept(stream).await {
                        Ok(tls_stream) => {
                            let io = TokioIo::new(tls_stream);
                            let service = TowerToHyperService::new(router);

                            if let Err(e) =
                                http1::Builder::new().serve_connection(io, service).await
                            {
                                tracing::debug!("Connection error from {}: {}", peer_addr, e);
                            }
                        }
                        Err(e) => {
                            tracing::debug!("TLS handshake error from {}: {}", peer_addr, e);
                        }
                    }
                });
            }
        } else {
            // HTTP only mode
            let listener = TcpListener::bind(http_addr)
                .await
                .map_err(ServerError::BindError)?;

            info!("HTTP server listening on http://{}", http_addr);
            info!(
                "Public URL: http://{}:{}",
                self.config.public_host, self.config.http_port
            );
            info!("TLS disabled - using HTTP only");

            axum::serve(listener, self.router)
                .await
                .map_err(|e| ServerError::BindError(std::io::Error::other(e)))?;
        }

        Ok(())
    }
}

/// Builder for HttpServer
pub struct HttpServerBuilder {
    bind_host: String,
    http_port: u16,
    https_port: u16,
    public_host: Option<String>,
    tls_config: TlsConfig,
    router: Option<Router>,
    middleware_config: MiddlewareConfig,
}

impl HttpServerBuilder {
    pub fn new() -> Self {
        Self {
            bind_host: "0.0.0.0".to_string(),
            http_port: 8080,
            https_port: 8443,
            public_host: None,
            tls_config: TlsConfig::default(),
            router: None,
            middleware_config: MiddlewareConfig::default(),
        }
    }

    /// Set bind address (host:port format or just port)
    pub fn bind(mut self, addr: impl Into<String>) -> Self {
        let addr = addr.into();
        if let Some((host, port)) = addr.split_once(':') {
            self.bind_host = host.to_string();
            if let Ok(p) = port.parse() {
                self.http_port = p;
            }
        } else if let Ok(p) = addr.parse::<u16>() {
            self.http_port = p;
        }
        self
    }

    /// Set HTTP port
    pub fn http_port(mut self, port: u16) -> Self {
        self.http_port = port;
        self
    }

    /// Set HTTPS port
    pub fn https_port(mut self, port: u16) -> Self {
        self.https_port = port;
        self
    }

    /// Set public hostname
    pub fn public_host(mut self, host: impl Into<String>) -> Self {
        self.public_host = Some(host.into());
        self
    }

    /// Enable HTTPS with explicit certificate paths
    pub fn https(mut self, cert_path: impl Into<String>, key_path: impl Into<String>) -> Self {
        self.tls_config = TlsConfig::with_certs(cert_path, key_path);
        self
    }

    /// Enable HTTPS with auto-detection
    pub fn https_auto(mut self) -> Self {
        self.tls_config = TlsConfig::auto();
        self
    }

    /// Disable HTTPS (HTTP only)
    pub fn http_only(mut self) -> Self {
        self.tls_config = TlsConfig::disabled();
        self
    }

    /// Set the router
    pub fn router(mut self, router: Router) -> Self {
        self.router = Some(router);
        self
    }

    /// Set middleware configuration
    pub fn middleware(mut self, config: MiddlewareConfig) -> Self {
        self.middleware_config = config;
        self
    }

    /// Enable/disable CORS
    pub fn cors(mut self, enabled: bool) -> Self {
        self.middleware_config.cors_enabled = enabled;
        self
    }

    /// Enable/disable tracing
    pub fn tracing(mut self, enabled: bool) -> Self {
        self.middleware_config.tracing_enabled = enabled;
        self
    }

    /// Enable/disable compression
    pub fn compression(mut self, enabled: bool) -> Self {
        self.middleware_config.compression_enabled = enabled;
        self
    }

    /// Build the server
    pub fn build(self) -> Result<HttpServer> {
        let router = self.router.unwrap_or_default();

        // Apply middleware stack
        let router = apply_middleware(router, self.middleware_config);

        let public_host = self
            .public_host
            .unwrap_or_else(|| gethostname::gethostname().to_string_lossy().to_string());

        let config = ServerConfig {
            http_port: self.http_port,
            https_port: self.https_port,
            bind_host: self.bind_host,
            public_host,
            tls: self.tls_config,
        };

        Ok(HttpServer { config, router })
    }
}

impl Default for HttpServerBuilder {
    fn default() -> Self {
        Self::new()
    }
}
