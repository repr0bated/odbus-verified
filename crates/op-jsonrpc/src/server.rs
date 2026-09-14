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
use std::path::Path;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream, UnixListener, UnixStream};
use tokio::sync::RwLock;
use tracing::{debug, error, info};

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
        let mut handles = Vec::new();

        // Start Unix socket server
        if let Some(ref socket_path) = self.config.unix_socket {
            let server = Arc::clone(&self);
            let path = socket_path.clone();
            handles.push(tokio::spawn(async move {
                if let Err(e) = server.run_unix(&path).await {
                    error!("Unix socket server error: {}", e);
                }
            }));
        }

        // Start TCP server
        if let Some(ref addr) = self.config.tcp_addr {
            let server = Arc::clone(&self);
            let addr = addr.clone();
            handles.push(tokio::spawn(async move {
                if let Err(e) = server.run_tcp(&addr).await {
                    error!("TCP server error: {}", e);
                }
            }));
        }

        // Wait for all servers
        for handle in handles {
            handle.await?;
        }

        Ok(())
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
            "ovsdb.list_dbs" | "ovsdb.get_schema" | "ovsdb.transact"
                if self.config.ovsdb_enabled =>
            {
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
            "ovsdb.transact" => {
                let params = request.params.as_array();
                if let Some(params) = params {
                    if params.len() < 2 {
                        return JsonRpcResponse::error(
                            request.id,
                            error_codes::INVALID_PARAMS,
                            "Missing database or operations",
                        );
                    }
                    let db = params[0].as_str().unwrap_or("Open_vSwitch");
                    let ops = json!(params[1..].to_vec());
                    match client.transact(db, ops).await {
                        Ok(result) => result,
                        Err(e) => {
                            return JsonRpcResponse::error(
                                request.id,
                                error_codes::DATABASE_ERROR,
                                e.to_string(),
                            )
                        }
                    }
                } else {
                    return JsonRpcResponse::error(
                        request.id,
                        error_codes::INVALID_PARAMS,
                        "Invalid params",
                    );
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
