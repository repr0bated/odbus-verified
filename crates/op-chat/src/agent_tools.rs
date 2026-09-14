//! Agent Tools - Register agent-based tools with the ToolRegistry
//!
//! This module provides tools that wrap agent operations, allowing
//! the LLM to interact with specialized agents for various tasks.

use anyhow::Result;
use simd_json::{json, OwnedValue as Value};
use std::collections::HashSet;
use std::sync::Arc;
use tracing::{debug, info, warn};

use op_agents::builtin_agent_descriptors;
use op_tools::builtin::create_agent_tool;
use op_tools::registry::{ToolDefinition, ToolRegistry};
use op_tools::tool::{BoxedTool, Tool};

/// Helper function to register a tool with the new 3-parameter API
async fn register_tool(registry: &ToolRegistry, tool: BoxedTool) -> Result<()> {
    let definition = ToolDefinition {
        name: tool.name().to_string(),
        description: tool.description().to_string(),
        input_schema: tool.input_schema(),
        schema_version: String::new(),
        category: tool.category().to_string(),
        tags: tool.tags(),
        namespace: tool.namespace().to_string(),
    };
    registry
        .register(Arc::from(tool.name()), tool, definition)
        .await
}

/// Agent info structure for registration
#[derive(Debug, Clone)]
pub struct AgentInfo {
    pub agent_type: String,
    pub name: String,
    pub description: String,
    pub operations: Vec<String>,
}

struct ContextRule {
    keywords: &'static [&'static str],
    agent_types: &'static [&'static str],
}

fn context_rules() -> Vec<ContextRule> {
    vec![
        ContextRule {
            keywords: &[
                "backend architect",
                "backend architecture",
                "microservices",
                "api design",
                "event-driven",
                "distributed systems",
            ],
            agent_types: &["backend-architect"],
        },
        ContextRule {
            keywords: &["frontend", "ui", "ux", "react", "vue", "svelte"],
            agent_types: &["frontend-developer"],
        },
        ContextRule {
            keywords: &["graphql"],
            agent_types: &["graphql-architect"],
        },
        ContextRule {
            keywords: &["infrastructure", "infra", "cloud", "aws", "gcp", "azure"],
            agent_types: &["cloud-architect"],
        },
        ContextRule {
            keywords: &["kubernetes", "k8s"],
            agent_types: &["kubernetes-architect"],
        },
        ContextRule {
            keywords: &["terraform"],
            agent_types: &["terraform-specialist"],
        },
        ContextRule {
            keywords: &["deployment", "ansible", "docker", "ci/cd", "cicd"],
            agent_types: &["deployment-engineer"],
        },
        ContextRule {
            keywords: &["devops", "ops", "operations", "sre"],
            agent_types: &["devops-troubleshooter"],
        },
        ContextRule {
            keywords: &["incident", "outage", "postmortem"],
            agent_types: &["incident-responder"],
        },
        ContextRule {
            keywords: &["observability", "monitoring", "logging", "tracing"],
            agent_types: &["observability-engineer"],
        },
        ContextRule {
            keywords: &["network", "routing", "firewall"],
            agent_types: &["network-engineer"],
        },
        ContextRule {
            keywords: &["rust", "cargo", "clippy", "borrow checker"],
            agent_types: &["rust-pro"],
        },
        ContextRule {
            keywords: &["python", "pytest", "mypy", "ruff"],
            agent_types: &["python-pro"],
        },
        ContextRule {
            keywords: &["javascript", "node", "npm"],
            agent_types: &["javascript-pro"],
        },
        ContextRule {
            keywords: &["typescript", "tsconfig"],
            agent_types: &["typescript-pro"],
        },
        ContextRule {
            keywords: &["golang", "go build", "go test"],
            agent_types: &["golang-pro"],
        },
        ContextRule {
            keywords: &["c++", "cpp"],
            agent_types: &["cpp-pro"],
        },
        ContextRule {
            keywords: &["c#", "dotnet"],
            agent_types: &["csharp-pro"],
        },
        ContextRule {
            keywords: &["java", "jvm", "maven", "gradle"],
            agent_types: &["java-pro"],
        },
        ContextRule {
            keywords: &["php", "laravel"],
            agent_types: &["php-pro"],
        },
        ContextRule {
            keywords: &["ruby", "rails"],
            agent_types: &["ruby-pro"],
        },
        ContextRule {
            keywords: &["scala", "sbt"],
            agent_types: &["scala-pro"],
        },
        ContextRule {
            keywords: &["elixir", "erlang", "beam"],
            agent_types: &["elixir-pro"],
        },
        ContextRule {
            keywords: &["julia"],
            agent_types: &["julia-pro"],
        },
        ContextRule {
            keywords: &["bash", "shell", "sh script"],
            agent_types: &["bash-pro"],
        },
        ContextRule {
            keywords: &["c language", "gcc", "clang"],
            agent_types: &["c-pro"],
        },
    ]
}

fn match_context_agents(message: &str) -> HashSet<String> {
    let mut matches = HashSet::new();
    let lowered = message.to_lowercase();

    for rule in context_rules() {
        if rule.keywords.iter().any(|kw| lowered.contains(kw)) {
            for agent_type in rule.agent_types {
                matches.insert(agent_type.to_string());
            }
        }
    }

    matches
}

/// Register agent tools based on user context keywords.
pub async fn register_context_agents(
    registry: &ToolRegistry,
    message: &str,
) -> Result<Vec<String>> {
    let matched = match_context_agents(message);
    if matched.is_empty() {
        return Ok(Vec::new());
    }

    let descriptors = builtin_agent_descriptors();
    let mut registered = Vec::new();

    for descriptor in descriptors {
        if !matched.contains(&descriptor.agent_type) {
            continue;
        }

        let tool_name = format!("agent_{}", descriptor.agent_type.replace('-', "_"));
        if registry.get_definition(&tool_name).await.is_some() {
            continue;
        }

        let tool = create_agent_tool(
            &descriptor.agent_type,
            &format!("{} - {}", descriptor.name, descriptor.description),
            &descriptor.operations,
            json!({ "agent_type": descriptor.agent_type }),
        )?;

        registry.register_tool(tool).await?;
        registered.push(tool_name);
    }

    Ok(registered)
}

/// Generic agent tool that wraps agent operations
pub struct AgentOperationTool {
    tool_name: String,
    agent_type: String,
    operation: String,
    description: String,
    input_schema: Value,
}

fn agent_namespace(agent_type: &str) -> &'static str {
    const CONTROL_AGENT_TYPES: [&str; 6] = [
        "executor",
        "file",
        "monitor",
        "network",
        "packagekit",
        "systemd",
    ];

    if CONTROL_AGENT_TYPES.contains(&agent_type) {
        "control-agent"
    } else {
        "agent"
    }
}

impl AgentOperationTool {
    pub fn new(
        agent_type: &str,
        operation: &str,
        description: &str,
        input_schema: Value,
    ) -> Arc<Self> {
        Arc::new(Self {
            tool_name: format!("agent_{}_{}", agent_type, operation),
            agent_type: agent_type.to_string(),
            operation: operation.to_string(),
            description: description.to_string(),
            input_schema,
        })
    }
}

#[async_trait::async_trait]
impl Tool for AgentOperationTool {
    fn name(&self) -> &str {
        &self.tool_name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> Value {
        self.input_schema.clone()
    }

    fn category(&self) -> &str {
        "agent"
    }

    fn namespace(&self) -> &str {
        agent_namespace(&self.agent_type)
    }

    fn tags(&self) -> Vec<String> {
        vec![
            "agent".to_string(),
            self.agent_type.clone(),
            self.operation.clone(),
        ]
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        // Build task for agent execution
        let task = json!({
            "type": self.agent_type,
            "operation": self.operation,
            "args": input
        });

        // In a real implementation, this would call the agent via D-Bus
        // For now, return a stub response
        info!(
            agent = %self.agent_type,
            operation = %self.operation,
            "Executing agent operation"
        );

        // TODO: Integrate with actual agent execution via D-Bus
        // let executor = DbusAgentExecutor::new();
        // executor.execute_operation(&self.agent_type, &self.operation, None, Some(input)).await

        Ok(json!({
            "success": true,
            "agent": self.agent_type,
            "operation": self.operation,
            "task": task,
            "message": "Agent operation queued (stub - integrate with D-Bus agent executor)"
        }))
    }
}

/// Get default agent definitions
pub fn get_default_agents() -> Vec<AgentInfo> {
    vec![
        AgentInfo {
            agent_type: "python_pro".to_string(),
            name: "Python Pro Agent".to_string(),
            description:
                "Expert Python development agent for code analysis, refactoring, and best practices"
                    .to_string(),
            operations: vec![
                "analyze".to_string(),
                "refactor".to_string(),
                "test".to_string(),
                "document".to_string(),
            ],
        },
        AgentInfo {
            agent_type: "rust_pro".to_string(),
            name: "Rust Pro Agent".to_string(),
            description: "Expert Rust development agent for systems programming".to_string(),
            operations: vec![
                "analyze".to_string(),
                "refactor".to_string(),
                "test".to_string(),
                "document".to_string(),
                "unsafe_audit".to_string(),
            ],
        },
        AgentInfo {
            agent_type: "devops".to_string(),
            name: "DevOps Agent".to_string(),
            description: "Infrastructure and deployment automation agent".to_string(),
            operations: vec![
                "deploy".to_string(),
                "rollback".to_string(),
                "scale".to_string(),
                "monitor".to_string(),
            ],
        },
        AgentInfo {
            agent_type: "security".to_string(),
            name: "Security Agent".to_string(),
            description: "Security analysis and vulnerability assessment agent".to_string(),
            operations: vec![
                "scan".to_string(),
                "audit".to_string(),
                "report".to_string(),
                "remediate".to_string(),
            ],
        },
        AgentInfo {
            agent_type: "database".to_string(),
            name: "Database Agent".to_string(),
            description: "Database management and optimization agent".to_string(),
            operations: vec![
                "query".to_string(),
                "optimize".to_string(),
                "backup".to_string(),
                "migrate".to_string(),
            ],
        },
        AgentInfo {
            agent_type: "network".to_string(),
            name: "Network Agent".to_string(),
            description: "Network configuration and troubleshooting agent".to_string(),
            operations: vec![
                "diagnose".to_string(),
                "configure".to_string(),
                "monitor".to_string(),
                "trace".to_string(),
            ],
        },
        AgentInfo {
            agent_type: "kubernetes".to_string(),
            name: "Kubernetes Agent".to_string(),
            description: "Kubernetes cluster management agent".to_string(),
            operations: vec![
                "deploy".to_string(),
                "scale".to_string(),
                "rollout".to_string(),
                "debug".to_string(),
            ],
        },
        AgentInfo {
            agent_type: "docker".to_string(),
            name: "Docker Agent".to_string(),
            description: "Docker container management agent".to_string(),
            operations: vec![
                "build".to_string(),
                "run".to_string(),
                "compose".to_string(),
                "prune".to_string(),
            ],
        },
        AgentInfo {
            agent_type: "git".to_string(),
            name: "Git Agent".to_string(),
            description: "Git repository management agent".to_string(),
            operations: vec![
                "commit".to_string(),
                "branch".to_string(),
                "merge".to_string(),
                "rebase".to_string(),
                "bisect".to_string(),
            ],
        },
        AgentInfo {
            agent_type: "testing".to_string(),
            name: "Testing Agent".to_string(),
            description: "Test generation and execution agent".to_string(),
            operations: vec![
                "generate".to_string(),
                "run".to_string(),
                "coverage".to_string(),
                "report".to_string(),
            ],
        },
    ]
}

/// Generate input schema for an agent operation
fn get_operation_schema(agent_type: &str, operation: &str) -> Value {
    match (agent_type, operation) {
        // Python Pro Agent
        ("python_pro", "analyze") => json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "Path to Python file or directory"},
                "check_types": {"type": "boolean", "default": true},
                "check_style": {"type": "boolean", "default": true}
            },
            "required": ["path"]
        }),
        ("python_pro", "refactor") => json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "Path to Python file"},
                "refactor_type": {"type": "string", "enum": ["extract_function", "rename", "inline", "move"]}
            },
            "required": ["path", "refactor_type"]
        }),

        // Rust Pro Agent
        ("rust_pro", "analyze") => json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "Path to Rust file or crate"},
                "clippy": {"type": "boolean", "default": true}
            },
            "required": ["path"]
        }),
        ("rust_pro", "unsafe_audit") => json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "Path to audit for unsafe code"}
            },
            "required": ["path"]
        }),

        // DevOps Agent
        ("devops", "deploy") => json!({
            "type": "object",
            "properties": {
                "environment": {"type": "string", "enum": ["dev", "staging", "production"]},
                "service": {"type": "string"},
                "version": {"type": "string"}
            },
            "required": ["environment", "service"]
        }),
        ("devops", "rollback") => json!({
            "type": "object",
            "properties": {
                "environment": {"type": "string"},
                "service": {"type": "string"},
                "to_version": {"type": "string"}
            },
            "required": ["environment", "service"]
        }),

        // Security Agent
        ("security", "scan") => json!({
            "type": "object",
            "properties": {
                "target": {"type": "string", "description": "Target to scan (path, URL, or IP)"},
                "scan_type": {"type": "string", "enum": ["vulnerability", "dependency", "secrets", "full"]}
            },
            "required": ["target"]
        }),

        // Database Agent
        ("database", "query") => json!({
            "type": "object",
            "properties": {
                "connection": {"type": "string", "description": "Database connection string or name"},
                "query": {"type": "string", "description": "SQL query to execute"},
                "explain": {"type": "boolean", "default": false}
            },
            "required": ["connection", "query"]
        }),

        // Kubernetes Agent
        ("kubernetes", "deploy") => json!({
            "type": "object",
            "properties": {
                "namespace": {"type": "string"},
                "manifest": {"type": "string", "description": "Path to manifest or manifest content"},
                "dry_run": {"type": "boolean", "default": false}
            },
            "required": ["manifest"]
        }),

        // Default schema for any operation
        _ => json!({
            "type": "object",
            "properties": {
                "args": {"type": "object", "description": "Operation arguments"}
            }
        }),
    }
}

/// Register all agent management tools
pub async fn register_agent_management_tools(registry: &ToolRegistry) -> Result<usize> {
    let mut count = 0;
    let agents = get_default_agents();

    info!("Registering agent tools for {} agents...", agents.len());

    for agent in &agents {
        for operation in &agent.operations {
            let description = format!(
                "{} - {} operation: {}",
                agent.name, operation, agent.description
            );

            let input_schema = get_operation_schema(&agent.agent_type, operation);

            let tool =
                AgentOperationTool::new(&agent.agent_type, operation, &description, input_schema);

            match register_tool(registry, tool).await {
                Ok(_) => {
                    debug!(
                        "Registered agent tool: agent_{}_{}",
                        agent.agent_type, operation
                    );
                    count += 1;
                }
                Err(e) => {
                    warn!(
                        "Failed to register agent tool agent_{}_{}: {}",
                        agent.agent_type, operation, e
                    );
                }
            }
        }
    }

    info!("✅ Registered {} agent tools", count);
    Ok(count)
}

/// List available agent types
pub struct ListAgentsTool;

impl ListAgentsTool {
    pub fn new() -> Arc<Self> {
        Arc::new(Self)
    }
}

#[async_trait::async_trait]
impl Tool for ListAgentsTool {
    fn name(&self) -> &str {
        "list_agents"
    }

    fn description(&self) -> &str {
        "List all available agent types and their operations."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "filter": {
                    "type": "string",
                    "description": "Optional filter by agent type"
                }
            }
        })
    }

    fn category(&self) -> &str {
        "agent"
    }

    fn tags(&self) -> Vec<String> {
        vec![
            "agent".to_string(),
            "list".to_string(),
            "discovery".to_string(),
        ]
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        use simd_json::prelude::*;
        let filter: Option<&str> = input.get_str("filter");
        let agents = get_default_agents();

        let filtered: Vec<_> = agents
            .iter()
            .filter(|a| match filter {
                Some(f) => {
                    a.agent_type.contains(f) || a.name.to_lowercase().contains(&f.to_lowercase())
                }
                None => true,
            })
            .map(|a| {
                json!({
                    "type": a.agent_type,
                    "name": a.name,
                    "description": a.description,
                    "operations": a.operations
                })
            })
            .collect();

        Ok(json!({
            "success": true,
            "agents": filtered,
            "count": filtered.len()
        }))
    }
}

/// Register the list_agents tool and all agent operation tools
pub async fn register_all_agent_tools(registry: &ToolRegistry) -> Result<usize> {
    let mut count = 0;

    // Register list_agents tool
    match register_tool(registry, ListAgentsTool::new()).await {
        Ok(_) => {
            count += 1;
            debug!("Registered list_agents tool");
        }
        Err(e) => warn!("Failed to register list_agents tool: {}", e),
    }

    // Register all agent operation tools
    count += register_agent_management_tools(registry).await?;

    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use simd_json::prelude::*;

    #[test]
    fn test_get_default_agents() {
        let agents = get_default_agents();
        assert!(!agents.is_empty());

        // Check some expected agents
        assert!(agents.iter().any(|a| a.agent_type == "python_pro"));
        assert!(agents.iter().any(|a| a.agent_type == "rust_pro"));
        assert!(agents.iter().any(|a| a.agent_type == "devops"));
    }

    #[tokio::test]
    async fn test_agent_operation_tool() {
        let tool = AgentOperationTool::new(
            "python_pro",
            "analyze",
            "Analyze Python code",
            json!({"type": "object"}),
        );

        assert_eq!(tool.name(), "agent_python_pro_analyze");
        assert_eq!(tool.category(), "agent");

        let result = tool.execute(json!({"path": "/tmp/test.py"})).await.unwrap();
        assert_eq!(result.get_bool("success"), Some(true));
    }

    #[tokio::test]
    async fn test_list_agents_tool() {
        let tool = ListAgentsTool::new();
        let result = tool.execute(json!({})).await.unwrap();

        assert_eq!(result.get_bool("success"), Some(true));
        assert!(result.get_u64("count").unwrap_or(0) > 0);
    }
}
