//! gRPC Agent Client — reflection-driven dispatcher
//!
//! All agent operations route through a single op-dbus gRPC endpoint via
//! PluginService.CallMethod. The available methods are discovered at connect
//! time via gRPC server reflection so no stubs or per-agent port tables are
//! needed. Adding a new agent or service requires no code change here — it
//! self-registers with ComponentRegistry and appears automatically.

use anyhow::{anyhow, Context, Result};
use prost_types::FileDescriptorSet;
use simd_json::OwnedValue as Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tonic::transport::{Channel, Endpoint};
use tracing::{debug, error, info, warn};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct AgentClientConfig {
    /// Address of the op-dbus gRPC server.
    /// Env: OP_DBUS_GRPC_ADDR (default: http://10.200.0.2:50051)
    pub address: String,
    pub connect_timeout: Duration,
    pub request_timeout: Duration,
    pub max_retries: u32,
}

impl Default for AgentClientConfig {
    fn default() -> Self {
        Self {
            address: std::env::var("OP_DBUS_GRPC_ADDR")
                .unwrap_or_else(|_| "http://10.200.0.2:50051".to_string()),
            connect_timeout: Duration::from_secs(5),
            request_timeout: Duration::from_secs(30),
            max_retries: 3,
        }
    }
}

// ---------------------------------------------------------------------------
// Discovered method index (built from reflection at connect time)
// ---------------------------------------------------------------------------

/// A gRPC method discovered via reflection.
#[derive(Debug, Clone)]
pub struct DiscoveredMethod {
    pub service: String,   // e.g. "operation.v1.PluginService"
    pub method: String,    // e.g. "CallMethod"
    pub input_type: String,
    pub output_type: String,
    pub server_streaming: bool,
}

// ---------------------------------------------------------------------------
// Active session state
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
struct SessionState {
    started_agents: Vec<String>,
    started_at: Option<std::time::Instant>,
}

// ---------------------------------------------------------------------------
// GrpcAgentClient
// ---------------------------------------------------------------------------

pub struct GrpcAgentClient {
    config: AgentClientConfig,
    channel: RwLock<Option<Channel>>,
    /// Method index keyed by "ServiceName/MethodName"
    methods: RwLock<HashMap<String, DiscoveredMethod>>,
    sessions: RwLock<HashMap<String, SessionState>>,
}

impl GrpcAgentClient {
    pub fn new(config: AgentClientConfig) -> Self {
        Self {
            config,
            channel: RwLock::new(None),
            methods: RwLock::new(HashMap::new()),
            sessions: RwLock::new(HashMap::new()),
        }
    }

    pub fn with_default_config() -> Self {
        Self::new(AgentClientConfig::default())
    }

    // -----------------------------------------------------------------------
    // Connect + reflection discovery
    // -----------------------------------------------------------------------
/// Connect to op-dbus and discover all available methods via reflection.
/// Must be called before any execute() calls.
pub async fn connect(&self) -> Result<()> {
    info!(address = %self.config.address, "Connecting to op-dbus gRPC");

    let addrs: Vec<&str> = self.config.address.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()).collect();

    let channel = if addrs.len() > 1 {
        // Native Tonic Load Balancing
        let endpoints = addrs.into_iter().map(|addr| {
            Endpoint::from_shared(addr.to_string())
                .map(|e| e.connect_timeout(self.config.connect_timeout)
                          .timeout(self.config.request_timeout))
        }).collect::<Result<Vec<_>, _>>()
        .context("invalid gRPC address in list")?;

        Channel::balance_list(endpoints.into_iter())
    } else {
        let endpoint = Endpoint::from_shared(self.config.address.clone())
            .context("invalid gRPC address")?
            .connect_timeout(self.config.connect_timeout)
            .timeout(self.config.request_timeout);

        endpoint
            .connect()
            .await
            .context("failed to connect to op-dbus")?
    };

    // Get clients
    let plugin_client = PluginServiceClient::new(channel.clone());

        // Discover methods via gRPC server reflection
        match self.discover_methods(channel).await {
            Ok(count) => info!(methods = count, "gRPC method discovery complete"),
            Err(e) => {
                // Non-fatal: reflection may be temporarily unavailable.
                // CallMethod dispatch will still work for known paths.
                warn!(error = %e, "gRPC reflection unavailable — method index empty");
            }
        }

        info!("Connected to op-dbus gRPC");
        Ok(())
    }

    /// Query reflection service and build the method index.
    async fn discover_methods(&self, channel: Channel) -> Result<usize> {
        use tonic_reflection::pb::grpc_reflection_v1::{
            server_reflection_client::ServerReflectionClient,
            server_reflection_request::MessageRequest,
            server_reflection_response::MessageResponse,
            ServerReflectionRequest,
        };

        let mut client = ServerReflectionClient::new(channel);

        // Open the bidirectional reflection stream
        let (tx, rx) = tokio::sync::mpsc::channel(16);
        let request_stream = tokio_stream::wrappers::ReceiverStream::new(rx);
        let mut stream = client
            .server_reflection_info(tonic::Request::new(request_stream))
            .await
            .context("reflection stream open failed")?
            .into_inner();

        // 1. Request service list
        tx.send(ServerReflectionRequest {
            host: String::new(),
            message_request: Some(MessageRequest::ListServices(String::new())),
        })
        .await?;

        let service_names: Vec<String> = if let Some(Ok(resp)) = stream.message().await? {
            match resp.message_response {
                Some(MessageResponse::ListServicesResponse(r)) => {
                    r.service.into_iter().map(|s| s.name).collect()
                }
                _ => return Err(anyhow!("unexpected reflection response for ListServices")),
            }
        } else {
            return Err(anyhow!("no response from reflection service"));
        };

        debug!(services = ?service_names, "Discovered gRPC services");

        // 2. For each service, request its FileDescriptorProto
        let mut discovered: HashMap<String, DiscoveredMethod> = HashMap::new();

        for svc in &service_names {
            // Skip the reflection service itself
            if svc.starts_with("grpc.reflection") || svc.starts_with("grpc.health") {
                continue;
            }

            tx.send(ServerReflectionRequest {
                host: String::new(),
                message_request: Some(MessageRequest::FileContainingSymbol(svc.clone())),
            })
            .await?;

            if let Some(Ok(resp)) = stream.message().await? {
                if let Some(MessageResponse::FileDescriptorResponse(fdr)) = resp.message_response {
                    for proto_bytes in fdr.file_descriptor_proto {
                        let fds = FileDescriptorSet {
                            file: vec![prost::Message::decode(proto_bytes.as_slice())
                                .context("decode FileDescriptorProto")?],
                        };
                        index_methods_from_descriptor(&fds, &mut discovered);
                    }
                }
            }
        }

        let count = discovered.len();
        *self.methods.write().await = discovered;
        Ok(count)
    }

    // -----------------------------------------------------------------------
    // Session management
    // -----------------------------------------------------------------------

    pub async fn start_session(
        &self,
        session_id: &str,
        client_name: &str,
    ) -> Result<Vec<String>> {
        info!(session = %session_id, client = %client_name, "Starting agent session");

        // Agents are discovered from ComponentRegistry at runtime.
        // For now, return the known run-on-connection set from env or defaults.
        let agents: Vec<String> = std::env::var("OP_RUN_ON_CONNECTION_AGENTS")
            .unwrap_or_else(|_| {
                "rust_pro,backend_architect,sequential_thinking,memory,context_manager"
                    .to_string()
            })
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        self.sessions.write().await.insert(
            session_id.to_string(),
            SessionState {
                started_agents: agents.clone(),
                started_at: Some(std::time::Instant::now()),
            },
        );

        info!(session = %session_id, agents = ?agents, "Session started");
        Ok(agents)
    }

    pub async fn end_session(&self, session_id: &str) -> Result<()> {
        info!(session = %session_id, "Ending agent session");
        self.sessions.write().await.remove(session_id);
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Dispatch — routes through PluginService.CallMethod on op-dbus
    // -----------------------------------------------------------------------

    /// Execute an operation on a named agent/plugin via PluginService.CallMethod.
    ///
    /// agent_id maps to plugin_id in op-dbus.
    /// operation maps to method_name.
    /// Arguments are serialized as a JSON array of prost Values.
    pub async fn execute(
        &self,
        session_id: &str,
        agent_id: &str,
        operation: &str,
        arguments: Value,
    ) -> Result<Value> {
        debug!(
            session = %session_id,
            agent = %agent_id,
            operation = %operation,
            "Dispatching agent operation via PluginService.CallMethod"
        );

        let channel = self
            .channel
            .read()
            .await
            .clone()
            .ok_or_else(|| anyhow!("not connected — call connect() first"))?;

        // Build the CallMethodRequest using op-grpc-bridge's generated types
        use op_grpc_bridge::proto::{
            plugin_service_client::PluginServiceClient, CallMethodRequest,
        };

        let args_json = simd_json::to_string(&arguments)
            .context("serialize arguments")?;

        // Convert JSON args array to prost Values
        let prost_args = json_to_prost_values(&arguments);

        let request = tonic::Request::new(CallMethodRequest {
            plugin_id: agent_id.to_string(),
            object_path: format!("/org/opdbus/agents/{}", agent_id),
            interface_name: "org.opdbus.AgentV1".to_string(),
            method_name: operation.to_string(),
            arguments: prost_args,
            actor_id: session_id.to_string(),
            capability_id: String::new(),
        });

        let mut client = PluginServiceClient::new(channel);

        let response = client
            .call_method(request)
            .await
            .context("PluginService.CallMethod failed")?
            .into_inner();

        if !response.success {
            if let Some(err) = response.error {
                return Err(anyhow!("agent error [{}]: {}", err.code, err.message));
            }
            return Err(anyhow!("agent operation failed (no error detail)"));
        }

        let result = response
            .result
            .map(prost_value_to_simd)
            .unwrap_or(simd_json::json!({}));

        Ok(result)
    }

    /// Execute with streaming response via StateSync.Subscribe.
    pub async fn execute_stream(
        &self,
        session_id: &str,
        agent_id: &str,
        operation: &str,
        arguments: Value,
        mut on_chunk: impl FnMut(StreamChunk) + Send,
    ) -> Result<Value> {
        debug!(
            session = %session_id,
            agent = %agent_id,
            operation = %operation,
            "Dispatching streaming agent operation"
        );

        // Non-streaming operations fall through to execute()
        // Streaming operations use StateSync.Subscribe with a path filter
        use op_grpc_bridge::proto::{
            state_sync_client::StateSyncClient, SubscribeRequest,
        };

        let channel = self
            .channel
            .read()
            .await
            .clone()
            .ok_or_else(|| anyhow!("not connected — call connect() first"))?;

        let mut client = StateSyncClient::new(channel.clone());

        let request = tonic::Request::new(SubscribeRequest {
            plugin_ids: vec![agent_id.to_string()],
            path_patterns: vec![format!("/agents/{}/{}", agent_id, operation)],
            tags: vec![format!("session:{}", session_id)],
            include_initial_state: false,
        });

        let mut stream = client
            .subscribe(request)
            .await
            .context("StateSync.Subscribe failed")?
            .into_inner();

        use tokio_stream::StreamExt;
        while let Some(change) = stream.next().await {
            match change {
                Ok(c) => {
                    let content = simd_json::to_string(
                        &c.new_value.map(prost_value_to_simd).unwrap_or_default(),
                    )
                    .unwrap_or_default();
                    let is_final = c.member_name == "complete";
                    on_chunk(StreamChunk {
                        content,
                        stream_type: StreamType::Stdout,
                        is_final,
                    });
                    if is_final {
                        break;
                    }
                }
                Err(e) => {
                    error!(error = %e, "stream error");
                    break;
                }
            }
        }

        // Fire-and-forget the actual execution after stream is set up
        self.execute(session_id, agent_id, operation, arguments)
            .await
    }

    /// Batch execute — parallel or sequential.
    pub async fn batch_execute(
        &self,
        session_id: &str,
        operations: Vec<(String, String, Value)>,
        parallel: bool,
    ) -> Result<Vec<Result<Value>>> {
        info!(
            session = %session_id,
            count = operations.len(),
            parallel = %parallel,
            "Batch executing agent operations"
        );

        if parallel {
            let futures: Vec<_> = operations
                .into_iter()
                .map(|(agent, op, args)| {
                    let session = session_id.to_string();
                    async move { self.execute(&session, &agent, &op, args).await }
                })
                .collect();
            Ok(futures::future::join_all(futures).await)
        } else {
            let mut results = Vec::new();
            for (agent, op, args) in operations {
                results.push(self.execute(session_id, &agent, &op, args).await);
            }
            Ok(results)
        }
    }

    // -----------------------------------------------------------------------
    // Convenience methods — all route through execute()
    // -----------------------------------------------------------------------

    pub async fn memory_remember(&self, session_id: &str, key: &str, value: &str) -> Result<()> {
        self.execute(
            session_id,
            "memory",
            "remember",
            simd_json::json!({ "key": key, "value": value }),
        )
        .await?;
        Ok(())
    }

    pub async fn memory_recall(&self, session_id: &str, key: &str) -> Result<Option<String>> {
        let result = self
            .execute(
                session_id,
                "memory",
                "recall",
                simd_json::json!({ "key": key }),
            )
            .await?;
        Ok(result
            .get("value")
            .and_then(|v| v.as_str())
            .map(String::from))
    }

    pub async fn think_start(
        &self,
        session_id: &str,
        problem: &str,
        max_steps: i32,
    ) -> Result<String> {
        let result = self
            .execute(
                session_id,
                "sequential_thinking",
                "start_chain",
                simd_json::json!({ "problem": problem, "max_steps": max_steps }),
            )
            .await?;
        Ok(result
            .get("chain_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string())
    }

    pub async fn think(
        &self,
        session_id: &str,
        chain_id: &str,
        thought: &str,
        step: i32,
    ) -> Result<Value> {
        self.execute(
            session_id,
            "sequential_thinking",
            "think",
            simd_json::json!({
                "chain_id": chain_id,
                "thought": thought,
                "step": step,
            }),
        )
        .await
    }

    pub async fn cargo_check(
        &self,
        session_id: &str,
        path: &str,
        on_output: impl FnMut(StreamChunk) + Send,
    ) -> Result<Value> {
        self.execute_stream(
            session_id,
            "rust_pro",
            "check",
            simd_json::json!({ "path": path }),
            on_output,
        )
        .await
    }

    pub async fn cargo_build(
        &self,
        session_id: &str,
        path: &str,
        release: bool,
        on_output: impl FnMut(StreamChunk) + Send,
    ) -> Result<Value> {
        self.execute_stream(
            session_id,
            "rust_pro",
            "build",
            simd_json::json!({ "path": path, "release": release }),
            on_output,
        )
        .await
    }

    pub async fn context_save(
        &self,
        session_id: &str,
        name: &str,
        content: &str,
        tags: Vec<String>,
    ) -> Result<()> {
        self.execute(
            session_id,
            "context_manager",
            "save",
            simd_json::json!({ "name": name, "content": content, "tags": tags }),
        )
        .await?;
        Ok(())
    }

    pub async fn context_load(&self, session_id: &str, name: &str) -> Result<Option<String>> {
        let result = self
            .execute(
                session_id,
                "context_manager",
                "load",
                simd_json::json!({ "name": name }),
            )
            .await?;
        if result
            .get("found")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            Ok(result
                .get("content")
                .and_then(|v| v.as_str())
                .map(String::from))
        } else {
            Ok(None)
        }
    }

    // -----------------------------------------------------------------------
    // Status
    // -----------------------------------------------------------------------

    pub async fn is_connected(&self) -> bool {
        self.channel.read().await.is_some()
    }

    pub async fn session_count(&self) -> usize {
        self.sessions.read().await.len()
    }

    pub async fn discovered_method_count(&self) -> usize {
        self.methods.read().await.len()
    }
}

// ---------------------------------------------------------------------------
// Stream types (unchanged interface for callers)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct StreamChunk {
    pub content: String,
    pub stream_type: StreamType,
    pub is_final: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamType {
    Stdout,
    Stderr,
    Progress,
    Result,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Walk a FileDescriptorSet and insert every method into the index.
fn index_methods_from_descriptor(
    fds: &FileDescriptorSet,
    index: &mut HashMap<String, DiscoveredMethod>,
) {
    for file in &fds.file {
        let pkg = file.package.as_deref().unwrap_or("");
        for svc in &file.service {
            let svc_name = svc.name.as_deref().unwrap_or("");
            let full_svc = if pkg.is_empty() {
                svc_name.to_string()
            } else {
                format!("{}.{}", pkg, svc_name)
            };
            for method in &svc.method {
                let method_name = method.name.as_deref().unwrap_or("");
                let key = format!("{}/{}", full_svc, method_name);
                index.insert(
                    key,
                    DiscoveredMethod {
                        service: full_svc.clone(),
                        method: method_name.to_string(),
                        input_type: method.input_type.as_deref().unwrap_or("").to_string(),
                        output_type: method.output_type.as_deref().unwrap_or("").to_string(),
                        server_streaming: method.server_streaming.unwrap_or(false),
                    },
                );
            }
        }
    }
}

fn prost_value_to_simd(v: prost_types::Value) -> Value {
    use prost_types::value::Kind;
    match v.kind {
        Some(Kind::NullValue(_)) => Value::Static(simd_json::StaticNode::Null),
        Some(Kind::BoolValue(b)) => Value::Static(simd_json::StaticNode::Bool(b)),
        Some(Kind::NumberValue(n)) => simd_json::json!(n),
        Some(Kind::StringValue(s)) => Value::String(s.into()),
        Some(Kind::ListValue(l)) => {
            Value::Array(l.values.into_iter().map(prost_value_to_simd).collect())
        }
        Some(Kind::StructValue(s)) => Value::Object(Box::new(
            s.fields
                .into_iter()
                .map(|(k, v)| (k.into(), prost_value_to_simd(v)))
                .collect(),
        )),
        None => Value::Static(simd_json::StaticNode::Null),
    }
}

fn simd_to_prost_value(v: &Value) -> prost_types::Value {
    use prost_types::value::Kind;
    use simd_json::prelude::*;
    let kind = match v {
        Value::Static(simd_json::StaticNode::Null) => Kind::NullValue(0),
        Value::Static(simd_json::StaticNode::Bool(b)) => Kind::BoolValue(*b),
        Value::Static(n) => Kind::NumberValue(n.as_f64().unwrap_or(0.0)),
        Value::String(s) => Kind::StringValue(s.to_string()),
        Value::Array(arr) => Kind::ListValue(prost_types::ListValue {
            values: arr.iter().map(simd_to_prost_value).collect(),
        }),
        Value::Object(obj) => Kind::StructValue(prost_types::Struct {
            fields: obj
                .iter()
                .map(|(k, v)| (k.to_string(), simd_to_prost_value(v)))
                .collect(),
        }),
    };
    prost_types::Value { kind: Some(kind) }
}

fn json_to_prost_values(v: &Value) -> Vec<prost_types::Value> {
    use simd_json::prelude::*;
    match v {
        Value::Array(arr) => arr.iter().map(simd_to_prost_value).collect(),
        Value::Object(_) => vec![simd_to_prost_value(v)],
        _ => vec![simd_to_prost_value(v)],
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_session_lifecycle() {
        let client = GrpcAgentClient::with_default_config();
        assert_eq!(client.session_count().await, 0);
        let agents = client.start_session("t1", "test").await.unwrap();
        assert!(!agents.is_empty());
        assert_eq!(client.session_count().await, 1);
        client.end_session("t1").await.unwrap();
        assert_eq!(client.session_count().await, 0);
    }

    #[test]
    fn test_method_indexing() {
        let mut index = HashMap::new();
        let fds = FileDescriptorSet { file: vec![] };
        index_methods_from_descriptor(&fds, &mut index);
        assert!(index.is_empty());
    }
}
