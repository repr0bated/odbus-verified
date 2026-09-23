//! JSON-RPC server implementation
//!
//! Provides a unified JSON-RPC server that can handle multiple backends:
//! - NonNet database
//! - OVSDB proxy
//! - Custom handlers

use anyhow::{Context, Result};
use simd_json::prelude::*;
use simd_json::{json, OwnedValue as Value};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream, UnixListener, UnixStream};
use tokio::sync::RwLock;
use tracing::{debug, info};

use crate::protocol::{error_codes, JsonRpcRequest, JsonRpcResponse};
use crate::OvsdbDbusClient;

/// Handler function type
pub type HandlerFn = Box<dyn Fn(JsonRpcRequest) -> JsonRpcResponse + Send + Sync>;

/// JSON-RPC server configuration
#[derive(Clone)]
pub struct JsonRpcServerConfig {
    /// Unix socket path (optional)
    pub unix_socket: Option<String>,
    /// TCP address (optional)
    pub tcp_addr: Option<String>,
    /// Enable OVSDB proxy
    pub ovsdb_enabled: bool,
}

impl Default for JsonRpcServerConfig {
    fn default() -> Self {
        Self {
            unix_socket: Some("/var/run/op-dbus/jsonrpc.sock".to_string()),
            tcp_addr: None,
            ovsdb_enabled: true,
        }
    }
}

/// JSON-RPC server
pub struct JsonRpcServer {
    config: JsonRpcServerConfig,
    handlers: Arc<RwLock<HashMap<String, HandlerFn>>>,
}

impl JsonRpcServer {
    /// Create a new JSON-RPC server
    pub fn new(config: JsonRpcServerConfig) -> Self {
        Self {
            config,
            handlers: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create with default configuration
    pub fn with_defaults() -> Self {
        Self::new(JsonRpcServerConfig::default())
    }

    /// Register a custom handler
    pub async fn register_handler(&self, method: &str, handler: HandlerFn) {
        let mut handlers = self.handlers.write().await;
        handlers.insert(method.to_string(), handler);
    }

    /// Run the server
    pub async fn run(self: Arc<Self>) -> Result<()> {
        match (
            self.config.unix_socket.as_deref(),
            self.config.tcp_addr.as_deref(),
        ) {
            (Some(path), Some(addr)) => {
                tokio::try_join!(self.run_unix(path), self.run_tcp(addr))?;
                Ok(())
            }
            (Some(path), None) => self.run_unix(path).await,
            (None, Some(addr)) => self.run_tcp(addr).await,
            (None, None) => anyhow::bail!("JSON-RPC server has no configured listener"),
        }
    }

    /// Run Unix socket server
    async fn run_unix(&self, socket_path: &str) -> Result<()> {
        let path = Path::new(socket_path);

        if let Some(dir) = path.parent() {
            tokio::fs::create_dir_all(dir).await.ok();
        }

        if path.exists() {
            tokio::fs::remove_file(path).await.ok();
        }

        let listener = UnixListener::bind(path).context("Failed to bind Unix socket")?;

        info!("JSON-RPC server listening on unix:{}", socket_path);

        loop {
            let (stream, _) = listener.accept().await?;
            let server = self.clone_for_connection();

            tokio::spawn(async move {
                if let Err(e) = server.handle_unix_connection(stream).await {
                    debug!("Connection error: {}", e);
                }
            });
        }
    }

    /// Run TCP server
    async fn run_tcp(&self, addr: &str) -> Result<()> {
        let addr: SocketAddr = addr.parse().context("Invalid JSON-RPC TCP address")?;
        if !addr.ip().is_loopback() {
            anyhow::bail!("JSON-RPC TCP listener must bind to loopback");
        }
        let listener = TcpListener::bind(addr)
            .await
            .context("Failed to bind TCP socket")?;

        info!("JSON-RPC server listening on tcp:{}", addr);

        loop {
            let (stream, _) = listener.accept().await?;
            let server = self.clone_for_connection();

            tokio::spawn(async move {
                if let Err(e) = server.handle_tcp_connection(stream).await {
                    debug!("Connection error: {}", e);
                }
            });
        }
    }

    /// Clone server state for a new connection
    fn clone_for_connection(&self) -> JsonRpcServerConnection {
        JsonRpcServerConnection {
            config: self.config.clone(),
            handlers: Arc::clone(&self.handlers),
        }
    }
}

/// Server state for a single connection
struct JsonRpcServerConnection {
    config: JsonRpcServerConfig,
    handlers: Arc<RwLock<HashMap<String, HandlerFn>>>,
}

impl JsonRpcServerConnection {
    /// Handle Unix socket connection
    async fn handle_unix_connection(&self, stream: UnixStream) -> Result<()> {
        let (reader, mut writer) = stream.into_split();
        let mut reader = BufReader::new(reader);
        let mut line = String::new();

        while reader.read_line(&mut line).await? > 0 {
            let response = self.process_line(&mut line).await;
            let response_str = simd_json::to_string(&response)?;
            writer.write_all(response_str.as_bytes()).await?;
            writer.write_all(b"\n").await?;
            line.clear();
        }

        Ok(())
    }

    /// Handle TCP connection
    async fn handle_tcp_connection(&self, stream: TcpStream) -> Result<()> {
        let (reader, mut writer) = stream.into_split();
        let mut reader = BufReader::new(reader);
        let mut line = String::new();

        while reader.read_line(&mut line).await? > 0 {
            let response = self.process_line(&mut line).await;
            let response_str = simd_json::to_string(&response)?;
            writer.write_all(response_str.as_bytes()).await?;
            writer.write_all(b"\n").await?;
            line.clear();
        }

        Ok(())
    }

    /// Process a JSON-RPC request line
    async fn process_line(&self, line: &mut String) -> JsonRpcResponse {
        match unsafe { simd_json::from_str::<Value>(line.as_mut_str()) } {
            Ok(value) => {
                match simd_json::serde::from_owned_value::<JsonRpcRequest>(value.clone()) {
                    Ok(request) => self.handle_request(request).await,
                    Err(e) => JsonRpcResponse::error(
                        value.get("id").cloned().unwrap_or(Value::null()),
                        error_codes::INVALID_REQUEST,
                        format!("Invalid request: {}", e),
                    ),
                }
            }
            Err(e) => JsonRpcResponse::error(
                Value::null(),
                error_codes::PARSE_ERROR,
                format!("Parse error: {}", e),
            ),
        }
    }

    /// Handle a JSON-RPC request
    async fn handle_request(&self, request: JsonRpcRequest) -> JsonRpcResponse {
        let method = &request.method;

        // This ingress carries no verified principal or OVSDB mutation grant.
        // Check before custom handlers so they cannot shadow the reserved method.
        if method == "ovsdb.transact" {
            return JsonRpcResponse::error(
                request.id,
                error_codes::PERMISSION_DENIED,
                "OVSDB transactions require verified control-plane admission",
            );
        }

        // Check custom handlers first
        {
            let handlers = self.handlers.read().await;
            if let Some(handler) = handlers.get(method) {
                return handler(request);
            }
        }

        // Built-in methods
        match method.as_str() {
            // OVSDB proxy methods
            "ovsdb.list_dbs" | "ovsdb.get_schema" if self.config.ovsdb_enabled => {
                return self.handle_ovsdb_request(request).await;
            }

            // Server info
            "server.info" => {
                return JsonRpcResponse::success(
                    request.id,
                    json!({
                        "name": "op-dbus-v2 JSON-RPC Server",
                        "version": env!("CARGO_PKG_VERSION"),
                        "ovsdb_enabled": self.config.ovsdb_enabled,
                    }),
                );
            }

            // Echo for testing
            "echo" => {
                return JsonRpcResponse::success(request.id, request.params);
            }

            _ => {}
        }

        JsonRpcResponse::error(
            request.id,
            error_codes::METHOD_NOT_FOUND,
            format!("Unknown method: {}", method),
        )
    }

    /// Handle OVSDB proxy request
    async fn handle_ovsdb_request(&self, request: JsonRpcRequest) -> JsonRpcResponse {
        let client = OvsdbDbusClient::new();

        let result = match request.method.as_str() {
            "ovsdb.list_dbs" => match client.list_dbs().await {
                Ok(dbs) => json!(dbs),
                Err(e) => {
                    return JsonRpcResponse::error(
                        request.id,
                        error_codes::DATABASE_ERROR,
                        e.to_string(),
                    )
                }
            },
            "ovsdb.get_schema" => {
                let db = request
                    .params
                    .as_array()
                    .and_then(|a| a.get(0))
                    .and_then(|v| v.as_str())
                    .unwrap_or("Open_vSwitch");
                match client.get_schema(db).await {
                    Ok(schema) => schema,
                    Err(e) => {
                        return JsonRpcResponse::error(
                            request.id,
                            error_codes::DATABASE_ERROR,
                            e.to_string(),
                        )
                    }
                }
            }
            _ => {
                return JsonRpcResponse::error(
                    request.id,
                    error_codes::METHOD_NOT_FOUND,
                    format!("Unknown method: {}", request.method),
                );
            }
        };

        JsonRpcResponse::success(request.id, result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tcp_only(addr: String) -> JsonRpcServerConfig {
        JsonRpcServerConfig {
            unix_socket: None,
            tcp_addr: Some(addr),
            ovsdb_enabled: true,
        }
    }

    #[tokio::test]
    async fn credential_free_ovsdb_transaction_is_denied_before_custom_handlers() {
        let server = JsonRpcServer::new(tcp_only("127.0.0.1:0".into()));
        server
            .register_handler(
                "ovsdb.transact",
                Box::new(|request| {
                    JsonRpcResponse::success(request.id, json!({ "bypassed": true }))
                }),
            )
            .await;
        let connection = server.clone_for_connection();
        let mut line = String::from(
            r#"{"jsonrpc":"2.0","method":"ovsdb.transact","params":["Open_vSwitch",{"op":"delete","table":"Bridge","where":[]}],"id":9}"#,
        );

        let response = connection.process_line(&mut line).await;
        assert_eq!(response.id, Value::from(9));
        assert_eq!(
            response.error.as_ref().map(|error| error.code),
            Some(error_codes::PERMISSION_DENIED)
        );
        assert!(response.result.is_none());
    }

    #[tokio::test]
    async fn tcp_listener_start_failure_is_returned_to_caller() {
        let server = Arc::new(JsonRpcServer::new(tcp_only("127.0.0.1:not-a-port".into())));

        let error = server.run().await.unwrap_err();
        assert!(error.to_string().contains("Invalid JSON-RPC TCP address"));
    }

    #[tokio::test]
    async fn tcp_listener_rejects_non_loopback_address() {
        let server = Arc::new(JsonRpcServer::new(tcp_only("0.0.0.0:0".into())));
        let error = server.run().await.unwrap_err();
        assert!(error.to_string().contains("must bind to loopback"));
    }
}
