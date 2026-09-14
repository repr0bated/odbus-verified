//! Agent registry for dynamic agent management
//!
//! This eliminates tight coupling by allowing agents to be registered
//! dynamically without hardcoding in the orchestrator.

use anyhow::{Context, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use simd_json::OwnedValue;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Agent specification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSpec {
    /// Unique agent type identifier
    pub agent_type: String,

    /// Human-readable name
    pub name: String,

    /// Description of agent functionality
    pub description: String,

    /// Command to execute the agent
    pub command: String,

    /// Arguments to pass to the command
    #[serde(default)]
    pub args: Vec<String>,

    /// Environment variables
    #[serde(default)]
    pub env: HashMap<String, String>,

    /// Working directory
    pub working_dir: Option<PathBuf>,

    /// Capabilities this agent provides
    pub capabilities: Vec<String>,

    /// Whether this agent requires root privileges
    #[serde(default)]
    pub requires_root: bool,

    /// Maximum number of instances
    #[serde(default = "default_max_instances")]
    pub max_instances: usize,

    /// Restart policy
    #[serde(default)]
    pub restart_policy: RestartPolicy,

    /// Health check configuration
    pub health_check: Option<HealthCheck>,
}

fn default_max_instances() -> usize {
    5
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RestartPolicy {
    #[default]
    Never,
    Always,
    OnFailure {
        max_retries: u32,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheck {
    /// D-Bus method to call for health check
    pub method: String,

    /// Interval in seconds
    pub interval_secs: u64,

    /// Timeout in seconds
    pub timeout_secs: u64,

    /// Number of consecutive failures before marking unhealthy
    pub unhealthy_threshold: u32,
}

/// Agent instance information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentInstance {
    pub id: String,
    pub agent_type: String,
    pub pid: Option<u32>,
    pub status: AgentStatus,
    pub started_at: chrono::DateTime<chrono::Utc>,
    pub last_health_check: Option<chrono::DateTime<chrono::Utc>>,
    pub restart_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentStatus {
    Starting,
    Running,
    Healthy,
    Unhealthy,
    Stopping,
    Stopped,
    Failed { reason: String },
}

/// Agent factory for creating agents
#[async_trait]
pub trait AgentFactory: Send + Sync {
    /// Create a new agent instance
    async fn create_agent(&self, spec: &AgentSpec, instance_id: &str) -> Result<AgentHandle>;

    /// Check if an agent type is supported
    fn supports(&self, agent_type: &str) -> bool;
}

/// Handle to a running agent (process or in-process trait object)
pub enum AgentBackend {
    Process(tokio::process::Child),
    InProcess(std::sync::Arc<dyn crate::AgentTrait + Send + Sync>),
}

pub struct AgentHandle {
    pub id: String,
    pub backend: AgentBackend,
    pub spec: AgentSpec,
}

impl AgentHandle {
    pub fn pid(&self) -> Option<u32> {
        match &self.backend {
            AgentBackend::Process(child) => child.id(),
            AgentBackend::InProcess(_) => None,
        }
    }

    pub async fn execute_task(&self, task: crate::AgentTask) -> Result<crate::TaskResult> {
        match &self.backend {
            AgentBackend::InProcess(agent) => {
                agent.execute(task).await.map_err(|e| anyhow::anyhow!(e))
            }
            AgentBackend::Process(_) => Err(anyhow::anyhow!(
                "Process-backed agents do not support in-process task execution"
            )),
        }
    }
}

/// In-process factory backed by `create_agent` / unified registry.
pub struct InProcessAgentFactory;

#[async_trait]
impl AgentFactory for InProcessAgentFactory {
    async fn create_agent(&self, spec: &AgentSpec, instance_id: &str) -> Result<AgentHandle> {
        let agent = crate::create_agent(&spec.agent_type, instance_id.to_string())
            .map_err(|e| anyhow::anyhow!(e))?;
        Ok(AgentHandle {
            id: instance_id.to_string(),
            backend: AgentBackend::InProcess(std::sync::Arc::from(agent)),
            spec: spec.clone(),
        })
    }

    fn supports(&self, agent_type: &str) -> bool {
        crate::create_agent(agent_type, "probe".to_string()).is_ok()
    }
}

/// Default agent factory that spawns processes
pub struct ProcessAgentFactory;

#[async_trait]
impl AgentFactory for ProcessAgentFactory {
    async fn create_agent(&self, spec: &AgentSpec, instance_id: &str) -> Result<AgentHandle> {
        let mut cmd = tokio::process::Command::new(&spec.command);

        // Add arguments
        cmd.args(&spec.args);

        // Add instance ID as environment variable
        cmd.env("AGENT_ID", instance_id);
        cmd.env("AGENT_TYPE", &spec.agent_type);

        // Add custom environment variables
        for (key, value) in &spec.env {
            cmd.env(key, value);
        }

        // Set working directory
        if let Some(dir) = &spec.working_dir {
            cmd.current_dir(dir);
        }

        // Spawn the process
        let process = cmd
            .spawn()
            .context(format!("Failed to spawn agent: {}", spec.command))?;

        Ok(AgentHandle {
            id: instance_id.to_string(),
            backend: AgentBackend::Process(process),
            spec: spec.clone(),
        })
    }

    fn supports(&self, agent_type: &str) -> bool {
        // Only for explicit external commands — in-process factory takes precedence.
        let _ = agent_type;
        false
    }
}

/// Agent registry for managing agent specifications and instances
pub struct AgentRegistry {
    /// Registered agent specifications
    specs: Arc<RwLock<HashMap<String, AgentSpec>>>,

    /// Running agent instances
    instances: Arc<RwLock<HashMap<String, AgentInstance>>>,

    /// Agent factories
    factories: Arc<RwLock<Vec<Box<dyn AgentFactory>>>>,

    /// Agent handles
    handles: Arc<RwLock<HashMap<String, AgentHandle>>>,
}

impl Default for AgentRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentRegistry {
    pub fn new() -> Self {
        Self {
            specs: Arc::new(RwLock::new(HashMap::new())),
            instances: Arc::new(RwLock::new(HashMap::new())),
            factories: Arc::new(RwLock::new(vec![
                Box::new(InProcessAgentFactory),
                Box::new(ProcessAgentFactory),
            ])),
            handles: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register an agent specification
    pub async fn register_spec(&self, spec: AgentSpec) -> Result<()> {
        let mut specs = self.specs.write().await;

        if specs.contains_key(&spec.agent_type) {
            return Err(anyhow::anyhow!(
                "Agent type '{}' is already registered",
                spec.agent_type
            ));
        }

        specs.insert(spec.agent_type.clone(), spec);
        Ok(())
    }

    /// Load specifications from a configuration file
    pub async fn load_specs_from_file(&self, path: &PathBuf) -> Result<()> {
        let content = tokio::fs::read_to_string(path)
            .await
            .context("Failed to read agent specifications file")?;

        let specs: Vec<AgentSpec> =
            serde_json::from_str(&content).context("Failed to parse agent specifications")?;

        for spec in specs {
            self.register_spec(spec).await?;
        }

        Ok(())
    }

    /// Load specifications from a directory
    pub async fn load_specs_from_directory(&self, dir: &PathBuf) -> Result<()> {
        let mut entries = tokio::fs::read_dir(dir)
            .await
            .context("Failed to read specifications directory")?;

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();

            if path.extension().and_then(|s| s.to_str()) == Some("json") {
                if let Err(e) = self.load_specs_from_file(&path).await {
                    tracing::warn!("Failed to load spec from {:?}: {}", path, e);
                }
            }
        }

        Ok(())
    }

    /// Register a custom agent factory
    pub async fn register_factory(&self, factory: Box<dyn AgentFactory>) {
        let mut factories = self.factories.write().await;
        factories.push(factory);
    }

    /// Spawn an agent instance
    pub async fn spawn_agent(
        &self,
        agent_type: &str,
        _config: Option<OwnedValue>,
    ) -> Result<String> {
        // Get the specification
        let specs = self.specs.read().await;
        let spec = specs
            .get(agent_type)
            .ok_or_else(|| anyhow::anyhow!("Unknown agent type: {}", agent_type))?
            .clone();

        // Check max instances
        let instances = self.instances.read().await;
        let current_count = instances
            .values()
            .filter(|i| {
                i.agent_type == agent_type
                    && matches!(i.status, AgentStatus::Running | AgentStatus::Healthy)
            })
            .count();

        if current_count >= spec.max_instances {
            return Err(anyhow::anyhow!(
                "Maximum instances ({}) reached for agent type '{}'",
                spec.max_instances,
                agent_type
            ));
        }
        drop(instances);

        // Generate instance ID
        let instance_id = format!("{}-{}", agent_type, uuid::Uuid::new_v4());

        // Find suitable factory
        let factories = self.factories.read().await;
        let factory = factories
            .iter()
            .find(|f| f.supports(agent_type))
            .ok_or_else(|| anyhow::anyhow!("No factory supports agent type: {}", agent_type))?;

        // Create the agent
        let handle = factory.create_agent(&spec, &instance_id).await?;
        let pid = handle.pid();

        // Store the handle
        let mut handles = self.handles.write().await;
        handles.insert(instance_id.clone(), handle);

        // Create instance record
        let instance = AgentInstance {
            id: instance_id.clone(),
            agent_type: agent_type.to_string(),
            pid,
            status: AgentStatus::Starting,
            started_at: chrono::Utc::now(),
            last_health_check: None,
            restart_count: 0,
        };

        // Store instance
        let mut instances = self.instances.write().await;
        instances.insert(instance_id.clone(), instance);

        // TODO: Start health check task if configured

        Ok(instance_id)
    }

    /// Kill an agent instance
    pub async fn kill_agent(&self, instance_id: &str) -> Result<()> {
        let mut handles = self.handles.write().await;

        if let Some(mut handle) = handles.remove(instance_id) {
            match &mut handle.backend {
                AgentBackend::Process(process) => {
                    process
                        .kill()
                        .await
                        .context("Failed to kill agent process")?;
                }
                AgentBackend::InProcess(_) => {}
            }

            // Update instance status
            let mut instances = self.instances.write().await;
            if let Some(instance) = instances.get_mut(instance_id) {
                instance.status = AgentStatus::Stopped;
            }

            Ok(())
        } else {
            Err(anyhow::anyhow!(
                "Agent instance '{}' not found",
                instance_id
            ))
        }
    }

    /// Execute a task on a running in-process agent instance
    pub async fn execute_instance_task(
        &self,
        instance_id: &str,
        task: crate::AgentTask,
    ) -> Result<crate::TaskResult> {
        let handles = self.handles.read().await;
        let handle = handles
            .get(instance_id)
            .ok_or_else(|| anyhow::anyhow!("Agent instance '{}' not found", instance_id))?;
        handle.execute_task(task).await
    }

    /// Get agent instance status
    pub async fn get_instance_status(&self, instance_id: &str) -> Result<AgentInstance> {
        let instances = self.instances.read().await;
        instances
            .get(instance_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Agent instance '{}' not found", instance_id))
    }

    /// List all agent instances
    pub async fn list_instances(&self) -> Vec<AgentInstance> {
        let instances = self.instances.read().await;
        instances.values().cloned().collect()
    }

    /// List all registered agent types
    pub async fn list_agent_types(&self) -> Vec<String> {
        let specs = self.specs.read().await;
        specs.keys().cloned().collect()
    }

    /// Get agent specification
    pub async fn get_spec(&self, agent_type: &str) -> Option<AgentSpec> {
        let specs = self.specs.read().await;
        specs.get(agent_type).cloned()
    }
}

/// Load catalog agent specifications (in-process unified agents).
///
/// Idempotent: skips types already registered so spawn/reload paths are safe.
pub async fn load_default_specs(registry: &AgentRegistry) -> Result<()> {
    for desc in crate::builtin_agent_descriptors() {
        if registry.get_spec(&desc.agent_type).await.is_some() {
            continue;
        }
        let spec = AgentSpec {
            agent_type: desc.agent_type.clone(),
            name: desc.name.clone(),
            description: desc.description.clone(),
            command: "in-process".to_string(),
            args: vec![],
            env: HashMap::new(),
            working_dir: None,
            capabilities: desc.operations.clone(),
            requires_root: false,
            max_instances: 8,
            restart_policy: RestartPolicy::Never,
            health_check: None,
        };
        registry.register_spec(spec).await?;
    }
    Ok(())
}
