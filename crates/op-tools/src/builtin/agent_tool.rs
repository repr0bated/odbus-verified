//! Agent Tool - D-Bus Agent Registration and Runtime Controls
//!
//! This module creates agent tools that register as D-Bus services.
//!
//! Architecture:
//! 1. Agent catalog is discovered from `op-agents`
//! 2. Registration is configurable (include/autostart)
//! 3. Tool calls go through zbus::Proxy with lazy service startup

use anyhow::Result;
use async_trait::async_trait;
use op_agents::builtin_agent_descriptors;
use simd_json::prelude::*;
use simd_json::{json, OwnedValue as Value};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, OnceLock};
use tokio::sync::RwLock;
use tracing::{debug, info, warn};
use zbus::Connection;

use crate::tool::{BoxedTool, Tool};

// =============================================================================
// BUS TYPE
// =============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BusType {
    #[default]
    System,
    Session,
}

// =============================================================================
// AGENT CONNECTION REGISTRY
// =============================================================================

static AGENT_CONNECTIONS: OnceLock<Arc<AgentConnectionRegistry>> = OnceLock::new();
static AGENT_RUNTIME_CATALOG: OnceLock<HashMap<String, AgentRuntimeDescriptor>> = OnceLock::new();

#[derive(Debug, Clone)]
struct AgentRuntimeDescriptor {
    agent_type: String,
    name: String,
    description: String,
    operations: Vec<String>,
    category: String,
}

#[derive(Debug, Clone)]
struct AgentRegistrationConfig {
    include: Option<HashSet<String>>,
    autostart: HashSet<String>,
    autostart_all: bool,
}

pub struct AgentConnectionRegistry {
    connections: RwLock<HashMap<String, Connection>>,
    bus_type: BusType,
}

impl AgentConnectionRegistry {
    pub fn new(bus_type: BusType) -> Self {
        Self {
            connections: RwLock::new(HashMap::new()),
            bus_type,
        }
    }

    pub fn global() -> Arc<Self> {
        AGENT_CONNECTIONS
            .get_or_init(|| {
                let bus_type = default_agent_bus();
                info!("AgentConnectionRegistry: using {:?} bus", bus_type);
                Arc::new(Self::new(bus_type))
            })
            .clone()
    }

    /// Start an agent as a D-Bus service.
    pub async fn start_agent_service(
        &self,
        agent_type: &str,
        agent_name: &str,
        description: &str,
        operations: &[String],
    ) -> Result<()> {
        let canonical_type = normalize_agent_type(agent_type);

        // Check if already running
        {
            let connections = self.connections.read().await;
            if connections.contains_key(&canonical_type) {
                debug!(agent = %canonical_type, "Agent already running");
                return Ok(());
            }
        }

        info!(agent = %canonical_type, "Starting agent D-Bus service");

        // Build service name: rust-pro -> org.dbusmcp.Agent.RustPro
        let service_name = format!(
            "org.dbusmcp.Agent.{}",
            canonical_type
                .split('-')
                .map(capitalize_first)
                .collect::<String>()
        );

        let object_path = format!(
            "/org/dbusmcp/Agent/{}",
            canonical_type
                .split('-')
                .map(capitalize_first)
                .collect::<String>()
        );

        // Create the D-Bus service object
        let service = AgentDbusService {
            agent_type: agent_type.to_string(),
            agent_name: agent_name.to_string(),
            description: description.to_string(),
            operations: operations.to_vec(),
        };

        // Build connection and serve
        let connection = match self.bus_type {
            BusType::System => {
                zbus::connection::Builder::system()?
                    .name(service_name.as_str())?
                    .serve_at(object_path.as_str(), service)?
                    .build()
                    .await?
            }
            BusType::Session => {
                zbus::connection::Builder::session()?
                    .name(service_name.as_str())?
                    .serve_at(object_path.as_str(), service)?
                    .build()
                    .await?
            }
        };

        // Store connection to keep service alive
        {
            let mut connections = self.connections.write().await;
            connections.insert(canonical_type.clone(), connection);
        }

        info!(agent = %canonical_type, service = %service_name, "✓ Agent registered on D-Bus");
        Ok(())
    }

    pub async fn is_running(&self, agent_type: &str) -> bool {
        self.connections
            .read()
            .await
            .contains_key(&normalize_agent_type(agent_type))
    }

    pub async fn list_running(&self) -> Vec<String> {
        self.connections.read().await.keys().cloned().collect()
    }

    pub async fn stop_agent(&self, agent_type: &str) -> Result<()> {
        let normalized = normalize_agent_type(agent_type);
        if self.connections.write().await.remove(&normalized).is_some() {
            info!(agent = %normalized, "Agent stopped");
        }
        Ok(())
    }

    pub async fn stop_all(&self) {
        let count = self.connections.write().await.drain().count();
        info!("Stopped {} agent D-Bus services", count);
    }
}

impl AgentRegistrationConfig {
    fn from_env() -> Self {
        let include = parse_agent_set("OP_AGENT_INCLUDE");
        let autostart = parse_agent_set("OP_AGENT_AUTOSTART").unwrap_or_default();
        let autostart_all = std::env::var("OP_AGENT_AUTOSTART_ALL")
            .ok()
            .map(|value| {
                matches!(
                    value.to_ascii_lowercase().as_str(),
                    "1" | "true" | "yes" | "on"
                )
            })
            .unwrap_or(false);
        Self {
            include,
            autostart,
            autostart_all,
        }
    }

    fn should_register(&self, agent_type: &str) -> bool {
        let normalized = normalize_agent_type(agent_type);
        match &self.include {
            Some(include) => include.contains(&normalized),
            None => true,
        }
    }

    fn should_autostart(&self, agent_type: &str) -> bool {
        let normalized = normalize_agent_type(agent_type);
        self.autostart_all || self.autostart.contains(&normalized)
    }
}

fn parse_agent_set(var_name: &str) -> Option<HashSet<String>> {
    let raw = std::env::var(var_name).ok()?;
    let parsed: HashSet<String> = raw
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(normalize_agent_type)
        .collect();
    if parsed.is_empty() {
        None
    } else {
        Some(parsed)
    }
}

fn normalize_agent_type(raw: &str) -> String {
    raw.trim().replace('_', "-").to_ascii_lowercase()
}

fn default_agent_bus() -> BusType {
    std::env::var("OP_AGENT_BUS")
        .ok()
        .and_then(|v| match v.to_lowercase().as_str() {
            "session" => Some(BusType::Session),
            "system" => Some(BusType::System),
            _ => None,
        })
        .unwrap_or_else(|| {
            if std::env::var("DBUS_SESSION_BUS_ADDRESS").is_ok() {
                BusType::Session
            } else {
                BusType::System
            }
        })
}

fn runtime_catalog() -> &'static HashMap<String, AgentRuntimeDescriptor> {
    AGENT_RUNTIME_CATALOG.get_or_init(|| {
        let mut catalog = HashMap::new();

        for descriptor in builtin_agent_descriptors() {
            let key = normalize_agent_type(&descriptor.agent_type);
            let category = infer_agent_category(&descriptor.agent_type).to_string();
            catalog.insert(
                key,
                AgentRuntimeDescriptor {
                    agent_type: normalize_agent_type(&descriptor.agent_type),
                    name: descriptor.name,
                    description: descriptor.description,
                    operations: descriptor.operations,
                    category,
                },
            );
        }

        // Keep legacy statically defined agents as fallbacks.
        for def in AGENT_DEFINITIONS {
            let key = normalize_agent_type(def.agent_type);
            catalog
                .entry(key)
                .or_insert_with(|| AgentRuntimeDescriptor {
                    agent_type: normalize_agent_type(def.agent_type),
                    name: def.name.to_string(),
                    description: def.description.to_string(),
                    operations: def.operations.iter().map(|op| op.to_string()).collect(),
                    category: def.category.to_string(),
                });
        }

        catalog
    })
}

fn find_agent_descriptor(agent_name: &str) -> Option<&'static AgentRuntimeDescriptor> {
    runtime_catalog().get(&normalize_agent_type(agent_name))
}

fn capitalize_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => first.to_uppercase().chain(chars).collect(),
    }
}

// =============================================================================
// D-BUS SERVICE IMPLEMENTATION
// =============================================================================

/// D-Bus service implementing org.dbusmcp.Agent interface
struct AgentDbusService {
    agent_type: String,
    agent_name: String,
    description: String,
    operations: Vec<String>,
}

#[zbus::interface(name = "org.dbusmcp.Agent")]
impl AgentDbusService {
    fn name(&self) -> &str {
        &self.agent_name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn operations(&self) -> Vec<String> {
        self.operations.clone()
    }

    async fn execute(&self, task_json: &str) -> String {
        debug!(agent = %self.agent_type, task = %task_json, "Executing");

        let task: Value = match serde_json::from_str(task_json) {
            Ok(t) => t,
            Err(e) => {
                return json!({
                    "success": false,
                    "error": format!("Parse error: {}", e)
                })
                .to_string();
            }
        };

        let operation = task
            .get("operation")
            .and_then(|v| v.as_str())
            .unwrap_or("execute");

        let mut agent_task = op_agents::AgentTask::new(&self.agent_type, operation);
        if let Some(path) = task.get("path").and_then(|v| v.as_str()) {
            agent_task = agent_task.with_path(path);
        }
        if let Some(args) = task.get("args") {
            agent_task = agent_task.with_args(&args.to_string());
        } else if let Some(query) = task.get("query").and_then(|v| v.as_str()) {
            agent_task = agent_task.with_args(&serde_json::json!({ "query": query }).to_string());
        }

        match op_agents::create_agent(&self.agent_type, format!("{}-dbus", self.agent_name)) {
            Ok(agent) => match agent.execute(agent_task).await {
                Ok(result) => serde_json::to_string(&serde_json::json!({
                    "success": result.success,
                    "agent": self.agent_type,
                    "operation": result.operation,
                    "data": result.data,
                    "metadata": result.metadata,
                }))
                .unwrap_or_else(|e| json!({"success": false, "error": e.to_string()}).to_string()),
                Err(e) => json!({
                    "success": false,
                    "agent": self.agent_type,
                    "operation": operation,
                    "error": e
                })
                .to_string(),
            },
            Err(e) => json!({
                "success": false,
                "agent": self.agent_type,
                "error": e
            })
            .to_string(),
        }
    }
}

// =============================================================================
// D-BUS EXECUTOR
// =============================================================================

pub struct DbusAgentExecutor {
    bus_type: BusType,
}

impl DbusAgentExecutor {
    pub fn new() -> Self {
        let bus_type = default_agent_bus();
        Self { bus_type }
    }

    fn to_service_name(agent_name: &str) -> String {
        let pascal = agent_name
            .split('_')
            .map(capitalize_first)
            .collect::<String>();
        format!("org.dbusmcp.Agent.{}", pascal)
    }

    fn to_object_path(agent_name: &str) -> String {
        let pascal = agent_name
            .split('_')
            .map(capitalize_first)
            .collect::<String>();
        format!("/org/dbusmcp/Agent/{}", pascal)
    }

    fn is_service_unavailable(error: &zbus::Error) -> bool {
        let s = error.to_string().to_lowercase();
        s.contains("serviceunknown")
            || s.contains("name has no owner")
            || s.contains("not found")
            || s.contains("does not exist")
    }

    async fn ensure_agent_running(&self, agent_name: &str) -> Result<()> {
        let Some(descriptor) = find_agent_descriptor(agent_name) else {
            return Err(anyhow::anyhow!(
                "Agent '{}' not found in runtime catalog",
                agent_name
            ));
        };

        let conn_registry = AgentConnectionRegistry::global();
        conn_registry
            .start_agent_service(
                &descriptor.agent_type,
                &descriptor.name,
                &descriptor.description,
                &descriptor.operations,
            )
            .await
    }
}

impl Default for DbusAgentExecutor {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
pub trait AgentExecutor: Send + Sync {
    async fn execute_operation(
        &self,
        agent_name: &str,
        operation: &str,
        path: Option<&str>,
        args: Option<Value>,
    ) -> Result<Value>;
}

#[async_trait]
impl AgentExecutor for DbusAgentExecutor {
    async fn execute_operation(
        &self,
        agent_name: &str,
        operation: &str,
        path: Option<&str>,
        args: Option<Value>,
    ) -> Result<Value> {
        let normalized_agent = normalize_agent_type(agent_name).replace('-', "_");
        let service_name = Self::to_service_name(&normalized_agent);
        let object_path = Self::to_object_path(&normalized_agent);

        let args_str = args.and_then(|v| {
            if v.is_null() {
                None
            } else {
                simd_json::to_string(&v).ok()
            }
        });

        let task = json!({
            "type": normalized_agent.replace('_', "-"),
            "operation": operation,
            "path": path,
            "args": args_str
        });
        let task_json = simd_json::to_string(&task)?;

        debug!(agent = %agent_name, service = %service_name, "Calling D-Bus");

        let connection = match self.bus_type {
            BusType::System => Connection::system().await,
            BusType::Session => Connection::session().await,
        }
        .map_err(|e| anyhow::anyhow!("D-Bus connection failed: {}", e))?;

        let mut bootstrap_attempted = false;

        loop {
            let proxy: zbus::Proxy = match zbus::proxy::Builder::new(&connection)
                .destination(service_name.as_str())?
                .path(object_path.as_str())?
                .interface("org.dbusmcp.Agent")?
                .build()
                .await
            {
                Ok(proxy) => proxy,
                Err(e) if Self::is_service_unavailable(&e) && !bootstrap_attempted => {
                    bootstrap_attempted = true;
                    self.ensure_agent_running(&normalized_agent).await?;
                    continue;
                }
                Err(e) if Self::is_service_unavailable(&e) => {
                    return Err(anyhow::anyhow!(
                        "Agent '{}' not running on D-Bus",
                        normalized_agent
                    ));
                }
                Err(e) => return Err(anyhow::anyhow!("D-Bus proxy failed: {}", e)),
            };

            let result: String = match proxy.call("Execute", &(task_json.clone(),)).await {
                Ok(result) => result,
                Err(e) if Self::is_service_unavailable(&e) && !bootstrap_attempted => {
                    bootstrap_attempted = true;
                    self.ensure_agent_running(&normalized_agent).await?;
                    continue;
                }
                Err(e) if Self::is_service_unavailable(&e) => {
                    return Err(anyhow::anyhow!(
                        "Agent '{}' not available",
                        normalized_agent
                    ));
                }
                Err(e) => return Err(anyhow::anyhow!("D-Bus call failed: {}", e)),
            };

            let parsed: Value = serde_json::from_str(&result)?;
            info!(agent = %normalized_agent, operation = %operation, "Completed");
            return Ok(parsed);
        }
    }
}

// =============================================================================
// AGENT TOOL
// =============================================================================

pub struct AgentTool {
    name: String,
    _agent_name: String,
    description: String,
    operations: Vec<String>,
    role_category: String,
    executor: Arc<dyn AgentExecutor + Send + Sync>,
}

impl AgentTool {
    pub fn new(
        agent_name: &str,
        description: &str,
        operations: &[String],
        executor: Arc<dyn AgentExecutor + Send + Sync>,
    ) -> Self {
        Self {
            name: format!("agent_{}", agent_name.replace('-', "_")),
            _agent_name: agent_name.to_string(),
            description: description.to_string(),
            operations: operations.to_vec(),
            role_category: "agent".to_string(),
            executor,
        }
    }

    pub fn with_category(
        agent_name: &str,
        description: &str,
        operations: &[String],
        category: &str,
        executor: Arc<dyn AgentExecutor + Send + Sync>,
    ) -> Self {
        Self {
            name: format!("agent_{}", agent_name.replace('-', "_")),
            _agent_name: agent_name.to_string(),
            description: description.to_string(),
            operations: operations.to_vec(),
            role_category: category.to_string(),
            executor,
        }
    }
}

#[async_trait]
impl Tool for AgentTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> Value {
        if self.operations.is_empty() {
            json!({
                "type": "object",
                "properties": {
                    "operation": { "type": "string", "description": "Operation to perform" },
                    "path": { "type": "string", "description": "Optional path" },
                    "args": { "type": "object", "description": "Additional arguments" }
                },
                "required": ["operation"]
            })
        } else {
            json!({
                "type": "object",
                "properties": {
                    "operation": {
                        "type": "string",
                        "enum": self.operations,
                        "description": "Operation to perform"
                    },
                    "path": { "type": "string", "description": "Optional path" },
                    "args": { "type": "object", "description": "Additional arguments" }
                },
                "required": ["operation"]
            })
        }
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let operation = input
            .get("operation")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'operation'"))?;

        if !self.operations.is_empty() && !self.operations.contains(&operation.to_string()) {
            return Err(anyhow::anyhow!(
                "Unknown operation: {}. Valid: {:?}",
                operation,
                self.operations
            ));
        }

        let path = input.get("path").and_then(|v| v.as_str());
        let args = input.get("args").cloned();
        let agent = self.name.strip_prefix("agent_").unwrap_or(&self.name);

        self.executor
            .execute_operation(agent, operation, path, args)
            .await
    }

    fn category(&self) -> &str {
        &self.role_category
    }

    fn namespace(&self) -> &str {
        "agent"
    }

    fn tags(&self) -> Vec<String> {
        vec!["agent".to_string(), self.role_category.clone()]
    }
}

// =============================================================================
// STATIC AGENT DEFINITIONS
// =============================================================================

/// Agent definition - no factory function needed
#[derive(Clone)]
pub struct AgentDef {
    pub agent_type: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub operations: &'static [&'static str],
    pub category: &'static str,
}

/// All agent definitions (static, no create_agent() needed)
pub const AGENT_DEFINITIONS: &[AgentDef] = &[
    AgentDef {
        agent_type: "rust-pro",
        name: "Rust Pro",
        description: "Expert Rust development agent",
        operations: &[
            "check", "build", "test", "clippy", "format", "run", "doc", "analyze",
        ],
        category: "language",
    },
    AgentDef {
        agent_type: "python-pro",
        name: "Python Pro",
        description: "Expert Python development agent",
        operations: &["analyze", "format", "lint", "test", "run"],
        category: "language",
    },
    AgentDef {
        agent_type: "backend-architect",
        name: "Backend Architect",
        description: "Backend architecture design agent",
        operations: &["analyze", "design", "review", "suggest", "document"],
        category: "architecture",
    },
    AgentDef {
        agent_type: "network-engineer",
        name: "Network Engineer",
        description: "Network configuration agent",
        operations: &["analyze", "configure", "diagnose", "optimize"],
        category: "infrastructure",
    },
    AgentDef {
        agent_type: "sequential-thinking",
        name: "Sequential Thinking",
        description: "Step-by-step reasoning agent",
        operations: &["think", "plan", "analyze", "conclude", "reflect"],
        category: "orchestration",
    },
    AgentDef {
        agent_type: "memory",
        name: "Memory Agent",
        description: "Persistent memory and recall",
        operations: &["store", "recall", "list", "search", "forget"],
        category: "orchestration",
    },
    AgentDef {
        agent_type: "context-manager",
        name: "Context Manager",
        description: "Session context management",
        operations: &[
            "save", "load", "list", "delete", "export", "import", "clear",
        ],
        category: "orchestration",
    },
    AgentDef {
        agent_type: "search-specialist",
        name: "Search Specialist",
        description: "Search and discovery agent",
        operations: &["search", "analyze", "suggest"],
        category: "seo",
    },
    AgentDef {
        agent_type: "deployment",
        name: "Deployment Agent",
        description: "Deployment management agent",
        operations: &["plan", "deploy", "rollback", "status"],
        category: "infrastructure",
    },
    AgentDef {
        agent_type: "debugger",
        name: "Debugger Agent",
        description: "Debugging and troubleshooting",
        operations: &["analyze", "diagnose", "suggest", "trace"],
        category: "analysis",
    },
    AgentDef {
        agent_type: "prompt-engineer",
        name: "Prompt Engineer",
        description: "Prompt optimization agent",
        operations: &["analyze", "improve", "generate", "test"],
        category: "aiml",
    },
];

// =============================================================================
// REGISTRATION
// =============================================================================

fn infer_agent_category(agent_type: &str) -> &'static str {
    if agent_type.contains("security") || agent_type.contains("auditor") {
        "security"
    } else if agent_type.contains("architect") || agent_type.contains("developer") {
        "architecture"
    } else if agent_type.ends_with("-pro") || agent_type.ends_with("_pro") {
        "language"
    } else if agent_type.contains("engineer") || agent_type == "deployment" {
        "infrastructure"
    } else if agent_type.contains("memory")
        || agent_type.contains("context")
        || agent_type.contains("sequential")
        || agent_type.contains("orchestrator")
    {
        "orchestration"
    } else {
        "agent"
    }
}

async fn register_agent_tool_parts(
    registry: &crate::ToolRegistry,
    agent_type: &str,
    agent_name: &str,
    description: &str,
    operations: &[String],
    category: &str,
    autostart: bool,
) -> Result<()> {
    info!(agent = %agent_type, "Registering agent");

    // 1. Optionally start D-Bus service immediately.
    if autostart {
        let conn_registry = AgentConnectionRegistry::global();
        if let Err(e) = conn_registry
            .start_agent_service(agent_type, agent_name, description, operations)
            .await
        {
            warn!(
                agent = %agent_type,
                error = %e,
                "D-Bus service failed, tool still registered"
            );
        }
    }

    // 2. Create tool
    let executor = Arc::new(DbusAgentExecutor::new());
    let tool = AgentTool::with_category(agent_type, description, operations, category, executor);

    // 3. Register tool
    registry.register_tool(Arc::new(tool)).await?;

    info!(agent = %agent_type, "✓ Agent registered");
    Ok(())
}

/// Register a single static agent definition (starts D-Bus service + creates tool)
pub async fn register_agent_tool(registry: &crate::ToolRegistry, def: &AgentDef) -> Result<()> {
    let operations: Vec<String> = def.operations.iter().map(|s| s.to_string()).collect();
    register_agent_tool_parts(
        registry,
        def.agent_type,
        def.name,
        def.description,
        &operations,
        def.category,
        true,
    )
    .await
}

/// Register all agents
pub async fn register_all_agents(registry: &crate::ToolRegistry) -> Result<()> {
    let config = AgentRegistrationConfig::from_env();
    let mut success = 0;
    let mut failed = 0;
    let mut skipped = 0;

    // Use full built-in catalog from op-agents.
    for descriptor in runtime_catalog().values() {
        if !config.should_register(&descriptor.agent_type) {
            skipped += 1;
            continue;
        }

        let autostart = config.should_autostart(&descriptor.agent_type);
        match register_agent_tool_parts(
            registry,
            &descriptor.agent_type,
            &descriptor.name,
            &descriptor.description,
            &descriptor.operations,
            &descriptor.category,
            autostart,
        )
        .await
        {
            Ok(()) => success += 1,
            Err(e) => {
                warn!(agent = %descriptor.agent_type, error = %e, "Failed");
                failed += 1;
            }
        }
    }

    info!(
        "Registered {} agents ({} failed, {} skipped)",
        success, failed, skipped
    );

    let running = AgentConnectionRegistry::global().list_running().await;
    info!("Active D-Bus services: {:?}", running);

    Ok(())
}

// =============================================================================
// LEGACY HELPERS
// =============================================================================

pub fn create_agent_tool(
    agent_name: &str,
    description: &str,
    operations: &[String],
    _config: Value,
) -> Result<BoxedTool> {
    let executor = Arc::new(DbusAgentExecutor::new());
    Ok(Arc::new(AgentTool::new(
        agent_name,
        description,
        operations,
        executor,
    )))
}

pub fn create_agent_tool_with_executor(
    agent_name: &str,
    description: &str,
    operations: &[String],
    executor: Arc<dyn AgentExecutor + Send + Sync>,
) -> Result<BoxedTool> {
    Ok(Arc::new(AgentTool::new(
        agent_name,
        description,
        operations,
        executor,
    )))
}
