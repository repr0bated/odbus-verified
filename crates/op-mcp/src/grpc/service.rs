//! MCP gRPC Service Implementation
#![allow(clippy::result_large_err)]

#[cfg(feature = "grpc")]
use crate::grpc::proto::mcp_service_server::McpService;
#[cfg(feature = "grpc")]
use crate::grpc::proto::*;
use crate::ServerMode;
use anyhow::Result;
use prost_types::{ListValue as ProstListValue, Struct as ProstStruct, Value as ProstValue};
use simd_json::prelude::*;
use simd_json::{json, OwnedValue as Value};
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{broadcast, mpsc, RwLock};
#[cfg(feature = "grpc")]
use tokio_stream::{wrappers::ReceiverStream, Stream, StreamExt};
#[cfg(feature = "grpc")]
use tonic::{Request, Response, Status};
use tracing::warn;
use uuid::Uuid;

const PROTOCOL_VERSION: &str = "2024-11-05";
const SERVER_NAME: &str = "op-mcp-grpc";
const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(feature = "grpc")]
type ResponseStream<T> = Pin<Box<dyn Stream<Item = Result<T, Status>> + Send>>;

#[allow(dead_code)]
struct Session {
    id: String,
    client_name: String,
    started_agents: Vec<String>,
    created_at: Instant,
}

/// Infrastructure integrations
#[derive(Default)]
pub struct GrpcInfrastructure {
    pub cache_path: Option<PathBuf>,
    pub state_db_path: Option<PathBuf>,
    pub snowball_path: Option<PathBuf>,
    pub tool_registry: Option<Arc<op_tools::ToolRegistry>>,
}

impl Clone for GrpcInfrastructure {
    fn clone(&self) -> Self {
        Self {
            cache_path: self.cache_path.clone(),
            state_db_path: self.state_db_path.clone(),
            snowball_path: self.snowball_path.clone(),
            tool_registry: self.tool_registry.clone(),
        }
    }
}

impl GrpcInfrastructure {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn from_paths(
        _cache_path: Option<PathBuf>,
        _state_db_path: Option<PathBuf>,
        _snowball_path: Option<PathBuf>,
    ) -> Result<Self> {
        Ok(Self::default())
    }

    pub fn with_tool_registry(mut self, registry: Arc<op_tools::ToolRegistry>) -> Self {
        self.tool_registry = Some(registry);
        self
    }
}

#[allow(dead_code)]
pub struct McpGrpcService {
    mode: ServerMode,
    sessions: RwLock<HashMap<String, Session>>,
    start_time: Instant,
    request_counter: AtomicU64,
    error_counter: AtomicU64,
    infrastructure: GrpcInfrastructure,
    /// Broadcast channel for subscription events (tool executions, state changes)
    event_tx: broadcast::Sender<McpEvent>,
}

impl McpGrpcService {
    pub fn new(mode: ServerMode) -> Self {
        let (event_tx, _) = broadcast::channel(256);
        Self {
            mode,
            sessions: RwLock::new(HashMap::new()),
            start_time: Instant::now(),
            request_counter: AtomicU64::new(0),
            error_counter: AtomicU64::new(0),
            infrastructure: GrpcInfrastructure::default(),
            event_tx,
        }
    }

    pub fn with_infrastructure(mode: ServerMode, infrastructure: GrpcInfrastructure) -> Self {
        let (event_tx, _) = broadcast::channel(256);
        Self {
            mode,
            sessions: RwLock::new(HashMap::new()),
            start_time: Instant::now(),
            request_counter: AtomicU64::new(0),
            error_counter: AtomicU64::new(0),
            infrastructure,
            event_tx,
        }
    }

    #[allow(dead_code)]
    async fn start_session_agents(&self, session_id: &str, client_name: &str) -> Vec<String> {
        let started = Vec::new();
        let session = Session {
            id: session_id.to_string(),
            client_name: client_name.to_string(),
            started_agents: started.clone(),
            created_at: Instant::now(),
        };
        self.sessions
            .write()
            .await
            .insert(session_id.to_string(), session);
        started
    }

    fn mode_to_proto(&self) -> i32 {
        match self.mode {
            ServerMode::Compact => 1,
            ServerMode::Agents => 2,
            ServerMode::Full => 3,
            ServerMode::Cognitive => 4,
            ServerMode::BlobSchema => 5,
        }
    }

    /// Build an McpServer instance using the current tool registry.
    fn build_mcp_server(&self) -> crate::server::McpServer {
        crate::server::McpServer::with_executor(
            crate::server::McpServerConfig::default(),
            Arc::new(crate::server::DefaultToolExecutor::new(
                self.infrastructure
                    .tool_registry
                    .clone()
                    .unwrap_or_else(|| Arc::new(op_tools::ToolRegistry::new())),
            )),
        )
    }

    /// Emit an event to all active subscribers. Failures (no receivers) are silently ignored.
    #[allow(dead_code)]
    fn emit_event(&self, event_type: &str, data_json: String) {
        let _ = self.event_tx.send(McpEvent {
            event_type: event_type.to_string(),
            data_json,
            timestamp: chrono::Utc::now().timestamp(),
            sequence: 0, // subscribers track their own sequence
        });
    }
}

// Helper: simd_json::Value -> prost_types::Value
fn simd_to_prost_value(value: &Value) -> ProstValue {
    use prost_types::value::Kind;
    match value {
        v if v.is_null() => ProstValue {
            kind: Some(Kind::NullValue(0)),
        },
        v if v.is_bool() => ProstValue {
            kind: Some(Kind::BoolValue(v.as_bool().unwrap())),
        },
        v if v.is_str() => ProstValue {
            kind: Some(Kind::StringValue(v.as_str().unwrap().to_string())),
        },
        v if v.is_f64() => ProstValue {
            kind: Some(Kind::NumberValue(v.as_f64().unwrap())),
        },
        v if v.is_i64() => ProstValue {
            kind: Some(Kind::NumberValue(v.as_i64().unwrap() as f64)),
        },
        v if v.is_u64() => ProstValue {
            kind: Some(Kind::NumberValue(v.as_u64().unwrap() as f64)),
        },
        v if v.is_array() => ProstValue {
            kind: Some(Kind::ListValue(ProstListValue {
                values: v
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(simd_to_prost_value)
                    .collect(),
            })),
        },
        v if v.is_object() => {
            let fields: BTreeMap<String, ProstValue> = v
                .as_object()
                .unwrap()
                .iter()
                .map(|(k, v)| (k.to_string(), simd_to_prost_value(v)))
                .collect();
            ProstValue {
                kind: Some(Kind::StructValue(ProstStruct { fields })),
            }
        }
        _ => ProstValue {
            kind: Some(Kind::NullValue(0)),
        },
    }
}

// Helper: prost_types::Value -> simd_json::Value
fn prost_to_simd_value(value: &ProstValue) -> Value {
    use prost_types::value::Kind;
    match &value.kind {
        Some(Kind::NullValue(_)) => Value::from(()),
        Some(Kind::NumberValue(f)) => Value::from(*f),
        Some(Kind::StringValue(s)) => Value::from(s.clone()),
        Some(Kind::BoolValue(b)) => Value::from(*b),
        Some(Kind::StructValue(s)) => {
            let obj: HashMap<String, Value> = s
                .fields
                .iter()
                .map(|(k, v)| (k.clone(), prost_to_simd_value(v)))
                .collect();
            Value::from(obj)
        }
        Some(Kind::ListValue(l)) => {
            let arr: Vec<Value> = l.values.iter().map(prost_to_simd_value).collect();
            Value::from(arr)
        }
        None => Value::from(()),
    }
}

fn simd_to_prost_struct(value: &Value) -> Result<ProstStruct, Status> {
    if let Some(obj) = value.as_object() {
        let fields: BTreeMap<String, ProstValue> = obj
            .iter()
            .map(|(k, v): (&String, &Value)| (k.clone(), simd_to_prost_value(v)))
            .collect();
        Ok(ProstStruct { fields })
    } else {
        Err(Status::invalid_argument("Value is not an object"))
    }
}

#[cfg(feature = "grpc")]
#[tonic::async_trait]
impl McpService for McpGrpcService {
    async fn call(&self, request: Request<McpRequest>) -> Result<Response<McpResponse>, Status> {
        self.request_counter.fetch_add(1, Ordering::Relaxed);
        let proto_req = request.into_inner();

        let params_simd = proto_req.params.map(|p| {
            let obj: HashMap<String, Value> = p
                .fields
                .into_iter()
                .map(|(k, v)| (k, prost_to_simd_value(&v)))
                .collect();
            Value::from(obj)
        });

        let internal_req = crate::protocol::McpRequest {
            jsonrpc: "2.0".to_string(),
            id: proto_req.id.as_ref().map(|v| simd_json::json!(v)),
            method: proto_req.method.clone(),
            params: params_simd,
            meta: None,
        };

        let server = crate::server::McpServer::with_executor(
            crate::server::McpServerConfig::default(),
            Arc::new(crate::server::DefaultToolExecutor::new(
                self.infrastructure
                    .tool_registry
                    .clone()
                    .unwrap_or_else(|| Arc::new(op_tools::ToolRegistry::new())),
            )),
        );

        let internal_resp = server.handle_request(internal_req).await;

        Ok(Response::new(McpResponse {
            jsonrpc: "2.0".to_string(),
            id: proto_req.id,
            result: internal_resp
                .result
                .and_then(|v| simd_to_prost_struct(&v).ok()),
            error: internal_resp.error.map(|e| McpError {
                code: e.code,
                message: e.message,
                data: e.data.and_then(|v| simd_to_prost_struct(&v).ok()),
            }),
        }))
    }

    type SubscribeStream = ResponseStream<McpEvent>;

    async fn subscribe(
        &self,
        request: Request<SubscribeRequest>,
    ) -> Result<Response<Self::SubscribeStream>, Status> {
        let req = request.into_inner();
        let _session_id = req.session_id.unwrap_or_else(|| Uuid::new_v4().to_string());
        let event_filter: Vec<String> = req.event_types;
        let (tx, rx) = mpsc::channel(32);
        let mut event_rx = self.event_tx.subscribe();

        tokio::spawn(async move {
            let mut sequence = 0u32;
            let mut heartbeat = tokio::time::interval(Duration::from_secs(30));

            loop {
                tokio::select! {
                    // Forward real events from the broadcast channel
                    recv_result = event_rx.recv() => {
                        match recv_result {
                            Ok(mut event) => {
                                // Filter by requested event_types (empty = accept all)
                                if !event_filter.is_empty()
                                    && !event_filter.iter().any(|t| t == &event.event_type)
                                {
                                    continue;
                                }
                                event.sequence = sequence;
                                sequence += 1;
                                if tx.send(Ok(event)).await.is_err() {
                                    break; // client disconnected
                                }
                            }
                            Err(broadcast::error::RecvError::Lagged(n)) => {
                                warn!("Subscription lagged, dropped {} events", n);
                                // Continue receiving, don't kill the stream
                            }
                            Err(broadcast::error::RecvError::Closed) => {
                                break; // sender side dropped
                            }
                        }
                    }
                    // Periodic heartbeat so the client knows the stream is alive
                    _ = heartbeat.tick() => {
                        let ping = McpEvent {
                            event_type: "ping".to_string(),
                            data_json: String::new(),
                            timestamp: chrono::Utc::now().timestamp(),
                            sequence,
                        };
                        sequence += 1;
                        if tx.send(Ok(ping)).await.is_err() {
                            break;
                        }
                    }
                }
            }
        });

        Ok(Response::new(
            Box::pin(ReceiverStream::new(rx)) as Self::SubscribeStream
        ))
    }

    type StreamStream = ResponseStream<McpResponse>;

    async fn stream(
        &self,
        request: Request<tonic::Streaming<McpRequest>>,
    ) -> Result<Response<Self::StreamStream>, Status> {
        let mut stream = request.into_inner();
        let (tx, rx) = mpsc::channel(32);
        let mcp_server = self.build_mcp_server();

        tokio::spawn(async move {
            while let Some(msg) = stream.next().await {
                let proto_req = match msg {
                    Ok(r) => r,
                    Err(e) => {
                        warn!("Stream recv error: {}", e);
                        break;
                    }
                };

                // Convert proto McpRequest -> internal McpRequest (same as `call`)
                let params_simd = proto_req.params.map(|p| {
                    let obj: HashMap<String, Value> = p
                        .fields
                        .into_iter()
                        .map(|(k, v)| (k, prost_to_simd_value(&v)))
                        .collect();
                    Value::from(obj)
                });

                let internal_req = crate::protocol::McpRequest {
                    jsonrpc: "2.0".to_string(),
                    id: proto_req.id.as_ref().map(|v| simd_json::json!(v)),
                    method: proto_req.method.clone(),
                    params: params_simd,
                    meta: None,
                };

                // Route through the MCP handler
                let internal_resp = mcp_server.handle_request(internal_req).await;

                let proto_resp = McpResponse {
                    jsonrpc: "2.0".to_string(),
                    id: proto_req.id,
                    result: internal_resp
                        .result
                        .and_then(|v| simd_to_prost_struct(&v).ok()),
                    error: internal_resp.error.map(|e| McpError {
                        code: e.code,
                        message: e.message,
                        data: e.data.and_then(|v| simd_to_prost_struct(&v).ok()),
                    }),
                };

                if tx.send(Ok(proto_resp)).await.is_err() {
                    break; // client disconnected
                }
            }
        });

        Ok(Response::new(
            Box::pin(ReceiverStream::new(rx)) as Self::StreamStream
        ))
    }

    async fn health(&self, _request: Request<()>) -> Result<Response<HealthResponse>, Status> {
        Ok(Response::new(HealthResponse {
            healthy: true,
            version: SERVER_VERSION.to_string(),
            server_name: SERVER_NAME.to_string(),
            mode: self.mode_to_proto(),
            connected_agents: vec![],
            uptime_secs: self.start_time.elapsed().as_secs(),
        }))
    }

    async fn initialize(
        &self,
        request: Request<InitializeRequest>,
    ) -> Result<Response<InitializeResponse>, Status> {
        let req = request.into_inner();
        let session_id = req.session_id.unwrap_or_else(|| Uuid::new_v4().to_string());
        Ok(Response::new(InitializeResponse {
            protocol_version: PROTOCOL_VERSION.to_string(),
            server_name: SERVER_NAME.to_string(),
            server_version: SERVER_VERSION.to_string(),
            capabilities: vec!["tools".to_string()],
            started_agents: vec![],
            session_id,
        }))
    }

    async fn list_tools(
        &self,
        _request: Request<ListToolsRequest>,
    ) -> Result<Response<ListToolsResponse>, Status> {
        let tools = if let Some(ref registry) = self.infrastructure.tool_registry {
            let all = registry.list().await;
            all.into_iter()
                .map(|t| ToolInfo {
                    name: t.name,
                    description: t.description,
                    input_schema: Some(convert_json_schema_to_tool_schema(&t.input_schema)),
                    category: if t.category.is_empty() {
                        None
                    } else {
                        Some(t.category)
                    },
                    tags: t.tags,
                })
                .collect()
        } else {
            vec![]
        };

        Ok(Response::new(ListToolsResponse {
            tools,
            total: 0,
            has_more: false,
        }))
    }

    async fn call_tool(
        &self,
        request: Request<CallToolRequest>,
    ) -> Result<Response<CallToolResponse>, Status> {
        let req = request.into_inner();
        let start = Instant::now();

        let arguments = if let Some(ToolArguments {
            args: Some(tool_arguments::Args::Generic(s)),
        }) = req.arguments
        {
            let obj: HashMap<String, Value> = s
                .fields
                .into_iter()
                .map(|(k, v)| (k, prost_to_simd_value(&v)))
                .collect();
            Value::from(obj)
        } else {
            json!({})
        };

        let registry = self
            .infrastructure
            .tool_registry
            .clone()
            .ok_or_else(|| Status::internal("No tool registry"))?;
        let tool = registry
            .get(&req.tool_name)
            .await
            .ok_or_else(|| Status::not_found("Tool not found"))?;

        let result = tool
            .execute(arguments)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        let result_struct = simd_to_prost_struct(&result).ok();

        Ok(Response::new(CallToolResponse {
            success: true,
            result: result_struct,
            error: None,
            duration_ms: start.elapsed().as_millis() as u64,
        }))
    }

    type CallToolStreamingStream = ResponseStream<ToolOutput>;

    async fn call_tool_streaming(
        &self,
        request: Request<CallToolRequest>,
    ) -> Result<Response<Self::CallToolStreamingStream>, Status> {
        let req = request.into_inner();
        let tool_name = req.tool_name.clone();

        let arguments = if let Some(ToolArguments {
            args: Some(tool_arguments::Args::Generic(s)),
        }) = req.arguments
        {
            let obj: HashMap<String, Value> = s
                .fields
                .into_iter()
                .map(|(k, v)| (k, prost_to_simd_value(&v)))
                .collect();
            Value::from(obj)
        } else {
            json!({})
        };

        let registry = self
            .infrastructure
            .tool_registry
            .clone()
            .ok_or_else(|| Status::internal("No tool registry"))?;
        let tool = registry
            .get(&tool_name)
            .await
            .ok_or_else(|| Status::not_found(format!("Tool not found: {}", tool_name)))?;

        let event_tx = self.event_tx.clone();
        let (tx, rx) = mpsc::channel(32);

        tokio::spawn(async move {
            // Send a progress message indicating execution has started
            let start_msg = ToolOutput {
                output_type: OutputType::Progress as i32,
                content: format!("Executing tool: {}", tool_name),
                sequence: 0,
                is_final: false,
                exit_code: None,
            };
            if tx.send(Ok(start_msg)).await.is_err() {
                return;
            }

            // Execute the tool
            match tool.execute(arguments).await {
                Ok(result) => {
                    let result_json =
                        simd_json::to_string_pretty(&result).unwrap_or_else(|_| "{}".to_string());

                    // Send the final result
                    let output = ToolOutput {
                        output_type: OutputType::Result as i32,
                        content: result_json.clone(),
                        sequence: 1,
                        is_final: true,
                        exit_code: Some(0),
                    };
                    let _ = tx.send(Ok(output)).await;

                    // Emit event for subscribers
                    let _ = event_tx.send(McpEvent {
                        event_type: "tool.completed".to_string(),
                        data_json: simd_json::to_string(&simd_json::json!({
                            "tool": tool_name,
                            "success": true
                        }))
                        .unwrap_or_default(),
                        timestamp: chrono::Utc::now().timestamp(),
                        sequence: 0,
                    });
                }
                Err(e) => {
                    let error_output = ToolOutput {
                        output_type: OutputType::Error as i32,
                        content: e.to_string(),
                        sequence: 1,
                        is_final: true,
                        exit_code: Some(1),
                    };
                    let _ = tx.send(Ok(error_output)).await;

                    // Emit failure event for subscribers
                    let _ = event_tx.send(McpEvent {
                        event_type: "tool.failed".to_string(),
                        data_json: simd_json::to_string(&simd_json::json!({
                            "tool": tool_name,
                            "error": e.to_string()
                        }))
                        .unwrap_or_default(),
                        timestamp: chrono::Utc::now().timestamp(),
                        sequence: 0,
                    });
                }
            }
        });

        Ok(Response::new(
            Box::pin(ReceiverStream::new(rx)) as Self::CallToolStreamingStream
        ))
    }

    async fn get_tool_schema(
        &self,
        request: Request<GetToolSchemaRequest>,
    ) -> Result<Response<GetToolSchemaResponse>, Status> {
        let req = request.into_inner();
        let registry = self
            .infrastructure
            .tool_registry
            .clone()
            .ok_or_else(|| Status::internal("No tool registry"))?;
        let def = registry
            .get_definition(&req.tool_name)
            .await
            .ok_or_else(|| Status::not_found("Tool not found"))?;

        Ok(Response::new(GetToolSchemaResponse {
            schema: Some(convert_json_schema_to_tool_schema(&def.input_schema)),
        }))
    }
}

fn convert_json_schema_to_tool_schema(schema: &Value) -> ToolSchema {
    let mut parameters = Vec::new();
    if let Some(props) = schema.get("properties").and_then(|v| v.as_object()) {
        for (name, prop) in props {
            let p_type = prop
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or("string");
            let param_type = match p_type {
                "string" => ParameterType::String,
                "integer" => ParameterType::Integer,
                "number" => ParameterType::Number,
                "boolean" => ParameterType::Boolean,
                "array" => ParameterType::Array,
                "object" => ParameterType::Object,
                _ => ParameterType::String,
            };
            parameters.push(ToolParameter {
                name: name.to_string(),
                r#type: param_type as i32,
                description: prop
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                default_value: None,
                enum_values: vec![],
            });
        }
    }
    ToolSchema {
        parameters,
        required: vec![],
    }
}
