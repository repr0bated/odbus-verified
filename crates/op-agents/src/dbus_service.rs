//! D-Bus Service Wrapper for AgentTrait implementations
//!
//! Exposes agents via D-Bus with standard interface: org.dbusmcp.Agent
//! This allows agents to be discovered by the ChatActor's tool_loader
//! and registered as tools that the LLM can call.
//!
//! # Architecture Integration
//!
//! ```text
//! ChatActor (brain)
//!    └── ToolRegistry
//!           └── AgentTool (wraps D-Bus calls)
//!                  └── D-Bus Call to org.dbusmcp.Agent.{AgentType}
//!                         └── DbusAgentService (this module)
//!                                └── AgentTrait implementation
//! ```
//!
//! # Example
//!
//! ```rust,ignore
//! use op_agents::{create_agent, dbus_service};
//! use op_core::BusType;
//!
//! let agent = create_agent("python-pro", "python-1".to_string()).unwrap();
//! let connection = dbus_service::start_agent(agent, "python-1", BusType::Session).await?;
//! // Agent is now discoverable via D-Bus introspection
//! ```

use crate::agents::base::{AgentTask, AgentTrait};
use crate::unified::{AgentCategory, GLOBAL_REGISTRY, agent_trait::AgentRequest};
use op_core::BusType;
use serde::Deserialize;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info};
use zbus::{
    Connection, connection::Builder, fdo::DBusProxy, interface, message::Header,
    object_server::SignalEmitter,
};

/// Error type for D-Bus agent service operations
#[derive(Debug, thiserror::Error)]
pub enum DbusAgentError {
    #[error("D-Bus connection error: {0}")]
    Connection(#[from] zbus::Error),

    #[error("Agent execution error: {0}")]
    Execution(String),

    #[error("JSON serialization error: {0}")]
    Json(#[from] simd_json::Error),

    #[error("Invalid task: {0}")]
    InvalidTask(String),
}

/// D-Bus service that wraps an AgentTrait implementation
///
/// Exposes a standard interface that can be discovered and called
/// by the AgentTool in op-chat's tool registry.
pub struct DbusAgentService {
    agent: Arc<RwLock<Box<dyn AgentTrait>>>,
    agent_type: String,
    agent_id: String,
}

/// Authenticated request forwarded by the bridge to the supervised execution
/// backend.  Identity fields are attribution only; admission remains in the
/// bridge and this process never mints or grants credentials.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CatalogExecutionRequest {
    agent_type: String,
    operation: String,
    arguments: serde_json::Value,
    actor_id: String,
    session_id: String,
    session_genesis: String,
}

fn catalog_execution_result(
    agent_type: &str,
    operation: &str,
    stdout: &str,
    stderr: &str,
    exit_code: i64,
) -> serde_json::Value {
    let success = exit_code == 0;
    serde_json::json!({
        "agent_type": agent_type,
        "operation": operation,
        "stdout": stdout,
        "stderr": stderr,
        "exit_code": exit_code,
        "success": success,
        "error": (!success).then(|| format!("child exited with status {exit_code}")),
    })
}

pub struct UnifiedExecutionService;

impl UnifiedExecutionService {
    pub const SERVICE_NAME: &'static str = "org.dbusmcp.Agent.UnifiedExecution";
    pub const OBJECT_PATH: &'static str = "/org/dbusmcp/Agent/UnifiedExecution";

    async fn require_root(
        connection: &Connection,
        header: &Header<'_>,
    ) -> Result<(), zbus::fdo::Error> {
        let sender = header
            .sender()
            .ok_or_else(|| zbus::fdo::Error::AccessDenied("missing D-Bus sender".into()))?;
        let proxy = DBusProxy::new(connection)
            .await
            .map_err(|e| zbus::fdo::Error::Failed(format!("credential lookup unavailable: {e}")))?;
        let uid = proxy
            .get_connection_unix_user(sender.clone().into())
            .await
            .map_err(|e| zbus::fdo::Error::Failed(format!("credential lookup failed: {e}")))?;
        if uid != 0 {
            return Err(zbus::fdo::Error::AccessDenied(
                "unified execution requires the bridge uid".into(),
            ));
        }
        Ok(())
    }
}

#[interface(name = "org.dbusmcp.Agent")]
impl UnifiedExecutionService {
    async fn execute_catalog(
        &self,
        request_json: String,
        #[zbus(connection)] connection: &Connection,
        #[zbus(header)] header: Header<'_>,
    ) -> zbus::fdo::Result<String> {
        Self::require_root(connection, &header).await?;
        if request_json.len() > 256 * 1024 {
            return Err(zbus::fdo::Error::InvalidArgs(
                "catalog execution request exceeds 256KiB".into(),
            ));
        }
        let request: CatalogExecutionRequest = serde_json::from_str(&request_json)
            .map_err(|e| zbus::fdo::Error::InvalidArgs(format!("invalid catalog request: {e}")))?;
        if request.actor_id.is_empty()
            || request.session_id.is_empty()
            || request.session_genesis.is_empty()
            || !request.arguments.is_object()
        {
            return Err(zbus::fdo::Error::InvalidArgs(
                "catalog attribution and object arguments are required".into(),
            ));
        }
        if !matches!(
            request.agent_type.as_str(),
            "python-executor"
                | "rust-executor"
                | "javascript-executor"
                | "go-executor"
                | "shell-executor"
        ) {
            return Err(zbus::fdo::Error::InvalidArgs(
                "agent is outside the canonical execution catalog".into(),
            ));
        }
        let agent = GLOBAL_REGISTRY
            .get_normalized(&request.agent_type)
            .ok_or_else(|| zbus::fdo::Error::InvalidArgs("unknown execution agent".into()))?;
        if agent.category() != AgentCategory::Execution || !agent.can_handle(&request.operation) {
            return Err(zbus::fdo::Error::InvalidArgs(
                "unsupported execution agent operation".into(),
            ));
        }
        let response = agent
            .execute(AgentRequest {
                operation: request.operation.clone(),
                args: simd_json::serde::to_owned_value(&request.arguments).map_err(|e| {
                    zbus::fdo::Error::InvalidArgs(format!("invalid arguments: {e}"))
                })?,
                context: Some(format!(
                    "actor={} session={} genesis={}",
                    request.actor_id, request.session_id, request.session_genesis
                )),
                files: Vec::new(),
            })
            .await;
        if !response.success {
            return Err(zbus::fdo::Error::Failed(response.message));
        }
        let data = serde_json::to_value(response.data)
            .map_err(|e| zbus::fdo::Error::Failed(format!("invalid executor response: {e}")))?;
        let exit_code = data
            .get("exit_code")
            .and_then(serde_json::Value::as_i64)
            .ok_or_else(|| zbus::fdo::Error::Failed("executor returned no exit status".into()))?;
        let stdout = data
            .get("stdout")
            .or_else(|| data.get("output"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        let stderr = data
            .get("stderr")
            .or_else(|| data.get("errors"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        serde_json::to_string(&catalog_execution_result(
            &request.agent_type,
            &request.operation,
            stdout,
            stderr,
            exit_code,
        ))
        .map_err(|e| zbus::fdo::Error::Failed(format!("response serialization failed: {e}")))
    }
}

pub async fn start_unified_execution_service(
    bus_type: BusType,
) -> Result<Connection, DbusAgentError> {
    let connection = match bus_type {
        BusType::System => {
            Builder::system()?
                .name(UnifiedExecutionService::SERVICE_NAME)?
                .serve_at(
                    UnifiedExecutionService::OBJECT_PATH,
                    UnifiedExecutionService,
                )?
                .build()
                .await?
        }
        BusType::Session => {
            Builder::session()?
                .name(UnifiedExecutionService::SERVICE_NAME)?
                .serve_at(
                    UnifiedExecutionService::OBJECT_PATH,
                    UnifiedExecutionService,
                )?
                .build()
                .await?
        }
    };
    Ok(connection)
}

impl DbusAgentService {
    /// Create a new D-Bus service wrapper for an agent
    pub fn new(agent: Box<dyn AgentTrait>, agent_id: String) -> Self {
        let agent_type = agent.agent_type().to_string();
        Self {
            agent: Arc::new(RwLock::new(agent)),
            agent_type,
            agent_id,
        }
    }

    /// Get the D-Bus well-known name for this agent type
    /// e.g., "python-pro" -> "org.dbusmcp.Agent.PythonPro"
    pub fn service_name(agent_type: &str) -> String {
        format!("org.dbusmcp.Agent.{}", to_pascal_case(agent_type))
    }

    /// Get the D-Bus object path for this agent
    /// e.g., "python-pro" -> "/org/dbusmcp/Agent/PythonPro"
    pub fn object_path(agent_type: &str) -> String {
        format!("/org/dbusmcp/Agent/{}", to_pascal_case(agent_type))
    }

    fn require_execution_admission() -> zbus::fdo::Result<()> {
        // The legacy per-agent interface has no verified principal or exact
        // operation grant. Keep discovery available, but never dispatch work
        // through this compatibility path.
        Err(zbus::fdo::Error::AccessDenied(
            "per-agent execution requires canonical identity and operation-grant admission".into(),
        ))
    }
}

/// D-Bus interface: org.dbusmcp.Agent
///
/// This is the standard interface that all agents expose.
/// The AgentTool in op-chat will call these methods.
#[interface(name = "org.dbusmcp.Agent")]
impl DbusAgentService {
    //
    // === Core Execution Methods ===
    //

    /// Execute a task on the agent
    ///
    /// # Arguments
    /// * `task_json` - JSON-encoded AgentTask:
    ///   ```json
    ///   {
    ///     "type": "python-pro",
    ///     "operation": "test",
    ///     "path": "/home/user/project",
    ///     "args": "--verbose",
    ///     "config": {}
    ///   }
    ///   ```
    ///
    /// # Returns
    /// JSON-encoded TaskResult
    async fn execute(&self, task_json: String) -> Result<String, zbus::fdo::Error> {
        Self::require_execution_admission()?;
        debug!(
            "[{}] Execute called: {}",
            self.agent_id,
            &task_json[..task_json.len().min(200)]
        );

        let task: AgentTask = serde_json::from_str(&task_json).map_err(|e| {
            error!("[{}] Invalid task JSON: {}", self.agent_id, e);
            zbus::fdo::Error::InvalidArgs(format!("Invalid task JSON: {}", e))
        })?;

        let agent = self.agent.read().await;

        // Validate operation is supported
        if !agent.supports_operation(&task.operation) {
            return Err(zbus::fdo::Error::InvalidArgs(format!(
                "Unsupported operation '{}'. Supported: {:?}",
                task.operation,
                agent.operations()
            )));
        }

        let result = agent.execute(task).await.map_err(|e| {
            error!("[{}] Execution failed: {}", self.agent_id, e);
            zbus::fdo::Error::Failed(format!("Execution failed: {}", e))
        })?;

        simd_json::to_string(&result).map_err(|e| {
            error!("[{}] Serialization failed: {}", self.agent_id, e);
            zbus::fdo::Error::Failed(format!("Serialization failed: {}", e))
        })
    }

    /// Execute an operation directly (convenience method)
    ///
    /// Simpler than Execute - just pass operation name and path
    async fn run_operation(
        &self,
        operation: String,
        path: String,
        args: String,
    ) -> Result<String, zbus::fdo::Error> {
        Self::require_execution_admission()?;
        let task = AgentTask {
            task_type: self.agent_type.clone(),
            operation,
            path: if path.is_empty() { None } else { Some(path) },
            args: if args.is_empty() { None } else { Some(args) },
            config: std::collections::HashMap::new(),
        };

        let task_json = simd_json::to_string(&task)
            .map_err(|e| zbus::fdo::Error::Failed(format!("Failed to serialize task: {}", e)))?;

        self.execute(task_json).await
    }

    //
    // === Introspection Methods ===
    //

    /// Get the agent type identifier (e.g., "python-pro")
    fn agent_type(&self) -> &str {
        &self.agent_type
    }

    /// Get the agent instance ID
    fn agent_id(&self) -> &str {
        &self.agent_id
    }

    /// Get the agent's display name
    async fn name(&self) -> String {
        let agent = self.agent.read().await;
        agent.name().to_string()
    }

    /// Get the agent's description
    async fn description(&self) -> String {
        let agent = self.agent.read().await;
        agent.description().to_string()
    }

    /// List supported operations
    async fn operations(&self) -> Vec<String> {
        let agent = self.agent.read().await;
        agent.operations()
    }

    /// Check if a specific operation is supported
    async fn supports_operation(&self, operation: String) -> bool {
        let agent = self.agent.read().await;
        agent.supports_operation(&operation)
    }

    /// Get the agent's current status
    async fn status(&self) -> String {
        let agent = self.agent.read().await;
        agent.get_status()
    }

    /// Get the security profile as JSON
    async fn security_profile(&self) -> String {
        let agent = self.agent.read().await;
        let profile = agent.security_profile();
        simd_json::to_string(profile).unwrap_or_else(|_| "{}".to_string())
    }

    /// Get full agent metadata as JSON (for tool discovery)
    async fn metadata(&self) -> String {
        let agent = self.agent.read().await;
        let profile = agent.security_profile();

        simd_json::json!({
            "agent_type": self.agent_type,
            "agent_id": self.agent_id,
            "name": agent.name(),
            "description": agent.description(),
            "operations": agent.operations(),
            "status": agent.get_status(),
            "security": {
                "category": format!("{:?}", profile.config.category),
                "timeout_secs": profile.config.timeout_secs,
                "requires_root": profile.config.requires_root,
            }
        })
        .to_string()
    }

    /// Ping to check if agent is alive
    fn ping(&self) -> bool {
        true
    }

    //
    // === Signals ===
    //

    /// Signal emitted when a task completes
    #[zbus(signal)]
    async fn task_completed(
        signal_ctxt: &SignalEmitter<'_>,
        task_id: &str,
        success: bool,
        result_json: &str,
    ) -> zbus::Result<()>;

    /// Signal emitted when agent status changes
    #[zbus(signal)]
    async fn status_changed(signal_ctxt: &SignalEmitter<'_>, new_status: &str) -> zbus::Result<()>;
}

//
// === Public Functions ===
//

/// Start an agent as a D-Bus service
///
/// This registers the agent on the specified bus with a well-known name.
/// The agent can then be discovered via D-Bus introspection and called
/// by the AgentTool in the ChatActor's tool registry.
///
/// # Arguments
/// * `agent` - The agent to expose via D-Bus
/// * `agent_id` - Unique identifier for this agent instance
/// * `bus_type` - Which bus to register on (System or Session)
///
/// # Returns
/// The D-Bus connection (keeps the service alive as long as it's held)
pub async fn start_agent(
    agent: Box<dyn AgentTrait>,
    agent_id: &str,
    bus_type: BusType,
) -> Result<Connection, DbusAgentError> {
    tracing::info!("Starting D-Bus agent service");
    let agent_type = agent.agent_type().to_string();
    let service = DbusAgentService::new(agent, agent_id.to_string());

    let service_name = DbusAgentService::service_name(&agent_type);
    let object_path = DbusAgentService::object_path(&agent_type);

    info!(
        "Starting D-Bus agent: {} (id={}) at {} on {:?} bus",
        service_name, agent_id, object_path, bus_type
    );

    let connection = match bus_type {
        BusType::System => {
            Builder::system()?
                .name(service_name.as_str())?
                .serve_at(object_path.as_str(), service)?
                .build()
                .await?
        }
        BusType::Session => {
            Builder::session()?
                .name(service_name.as_str())?
                .serve_at(object_path.as_str(), service)?
                .build()
                .await?
        }
    };

    info!("D-Bus agent {} registered successfully", service_name);

    // Wait for the service to be ready
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    Ok(connection)
}

/// Start an agent with a custom instance suffix
///
/// Useful for running multiple instances of the same agent type.
/// Service name becomes: org.dbusmcp.Agent.{AgentType}.{InstanceId}
pub async fn start_agent_instance(
    agent: Box<dyn AgentTrait>,
    agent_id: &str,
    instance_suffix: &str,
    bus_type: BusType,
) -> Result<Connection, DbusAgentError> {
    tracing::info!("Starting D-Bus agent instance");
    let agent_type = agent.agent_type().to_string();
    let service = DbusAgentService::new(agent, agent_id.to_string());

    let base_name = DbusAgentService::service_name(&agent_type);
    let service_name = format!("{}.{}", base_name, instance_suffix);
    let base_path = DbusAgentService::object_path(&agent_type);
    let object_path = format!("{}/{}", base_path, instance_suffix);

    info!(
        "Starting D-Bus agent instance: {} at {} on {:?} bus",
        service_name, object_path, bus_type
    );

    let connection = match bus_type {
        BusType::System => {
            Builder::system()?
                .name(service_name.as_str())?
                .serve_at(object_path.as_str(), service)?
                .build()
                .await?
        }
        BusType::Session => {
            Builder::session()?
                .name(service_name.as_str())?
                .serve_at(object_path.as_str(), service)?
                .build()
                .await?
        }
    };

    info!(
        "D-Bus agent instance {} registered successfully",
        service_name
    );

    // Wait for the service to be ready
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    Ok(connection)
}

//
// === Helper Functions ===
//

/// Convert agent type to PascalCase for D-Bus naming
/// e.g., "python-pro" -> "PythonPro"
fn to_pascal_case(s: &str) -> String {
    s.split('-')
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().chain(chars).collect(),
            }
        })
        .collect()
}

/// Generate a unique agent ID
pub fn generate_agent_id(agent_type: &str) -> String {
    format!(
        "{}-{}",
        agent_type,
        uuid::Uuid::new_v4()
            .to_string()
            .split('-')
            .next()
            .unwrap_or("0000")
    )
}

/// Check if a D-Bus service name matches the agent pattern
pub fn is_agent_service(service_name: &str) -> bool {
    service_name.starts_with("org.dbusmcp.Agent.")
}

/// Extract agent type from service name
/// e.g., "org.dbusmcp.Agent.PythonPro" -> "python-pro"
pub fn service_name_to_agent_type(service_name: &str) -> Option<String> {
    if !is_agent_service(service_name) {
        return None;
    }

    let pascal = service_name.strip_prefix("org.dbusmcp.Agent.")?;
    // Handle instance suffixes (e.g., "PythonPro.instance1" -> "PythonPro")
    let pascal = pascal.split('.').next()?;
    Some(to_kebab_case(pascal))
}

/// Convert PascalCase to kebab-case
/// e.g., "PythonPro" -> "python-pro"
fn to_kebab_case(s: &str) -> String {
    let mut result = String::new();
    for (i, c) in s.chars().enumerate() {
        if c.is_uppercase() {
            if i > 0 {
                result.push('-');
            }
            result.push(c.to_ascii_lowercase());
        } else {
            result.push(c);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::base::TaskResult;
    use crate::security::SecurityProfile;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn catalog_result_uses_child_exit_status_and_keeps_diagnostics() {
        let failed = catalog_execution_result("shell-executor", "exec", "partial", "error", 7);
        assert_eq!(failed["exit_code"], 7);
        assert_eq!(failed["success"], false);
        assert_eq!(failed["stdout"], "partial");
        assert_eq!(failed["stderr"], "error");
        assert_eq!(failed["error"], "child exited with status 7");
        let passed = catalog_execution_result("shell-executor", "exec", "done", "", 0);
        assert_eq!(passed["success"], true);
        assert!(passed["error"].is_null());
    }

    struct RecordingAgent {
        executions: Arc<AtomicUsize>,
        profile: SecurityProfile,
    }

    #[async_trait::async_trait]
    impl AgentTrait for RecordingAgent {
        fn agent_type(&self) -> &str {
            "recording"
        }

        fn name(&self) -> &str {
            "Recording"
        }

        fn description(&self) -> &str {
            "Records execution attempts"
        }

        fn operations(&self) -> Vec<String> {
            vec!["run".into()]
        }

        fn security_profile(&self) -> &SecurityProfile {
            &self.profile
        }

        async fn execute(&self, task: AgentTask) -> Result<TaskResult, String> {
            self.executions.fetch_add(1, Ordering::SeqCst);
            Ok(TaskResult::success(&task.operation, String::new()))
        }
    }

    #[tokio::test]
    async fn legacy_execution_methods_deny_before_dispatch() {
        let executions = Arc::new(AtomicUsize::new(0));
        let service = DbusAgentService::new(
            Box::new(RecordingAgent {
                executions: executions.clone(),
                profile: SecurityProfile::code_execution("recording", vec!["cargo"]),
            }),
            "recording-1".into(),
        );

        // Even a supported operation and a well-formed task cannot reach the
        // agent through the unauthenticated compatibility interface.
        let task = simd_json::to_string(&AgentTask::new("recording", "run")).unwrap();
        assert!(matches!(
            service.execute(task).await,
            Err(zbus::fdo::Error::AccessDenied(_))
        ));
        assert!(matches!(
            service
                .run_operation("run".into(), "/tmp".into(), String::new())
                .await,
            Err(zbus::fdo::Error::AccessDenied(_))
        ));
        assert_eq!(executions.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn test_to_pascal_case() {
        assert_eq!(to_pascal_case("python-pro"), "PythonPro");
        assert_eq!(to_pascal_case("rust-pro"), "RustPro");
        assert_eq!(to_pascal_case("code-reviewer"), "CodeReviewer");
        assert_eq!(to_pascal_case("tdd-orchestrator"), "TddOrchestrator");
    }

    #[test]
    fn test_to_kebab_case() {
        assert_eq!(to_kebab_case("PythonPro"), "python-pro");
        assert_eq!(to_kebab_case("RustPro"), "rust-pro");
        assert_eq!(to_kebab_case("CodeReviewer"), "code-reviewer");
    }

    #[test]
    fn test_service_name() {
        assert_eq!(
            DbusAgentService::service_name("python-pro"),
            "org.dbusmcp.Agent.PythonPro"
        );
    }

    #[test]
    fn test_object_path() {
        assert_eq!(
            DbusAgentService::object_path("python-pro"),
            "/org/dbusmcp/Agent/PythonPro"
        );
    }

    #[test]
    fn test_is_agent_service() {
        assert!(is_agent_service("org.dbusmcp.Agent.PythonPro"));
        assert!(is_agent_service("org.dbusmcp.Agent.RustPro.instance1"));
        assert!(!is_agent_service("org.freedesktop.DBus"));
        assert!(!is_agent_service("org.dbusmcp.Orchestrator"));
    }

    #[test]
    fn test_service_name_to_agent_type() {
        assert_eq!(
            service_name_to_agent_type("org.dbusmcp.Agent.PythonPro"),
            Some("python-pro".to_string())
        );
        assert_eq!(
            service_name_to_agent_type("org.dbusmcp.Agent.PythonPro.instance1"),
            Some("python-pro".to_string())
        );
        assert_eq!(service_name_to_agent_type("org.freedesktop.DBus"), None);
    }

    #[test]
    fn test_generate_agent_id() {
        let id = generate_agent_id("python-pro");
        assert!(id.starts_with("python-pro-"));
    }
}
