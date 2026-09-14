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
use op_core::BusType;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info};
use zbus::{connection::Builder, interface, object_server::SignalEmitter, Connection};

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
