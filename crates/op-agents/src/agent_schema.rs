//! Per-agent / per-operation JSON Schema builders.
//!
//! These schemas are the contract for MCP `inputSchema`, the `persona` plugin
//! catalog, and (eventually) typed PluginSchema method args.

use crate::unified::AgentCategory;
use serde::{Deserialize, Serialize};
use simd_json::{json, OwnedValue as Value};

/// JSON Schema for a single agent operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentOperationSchema {
    pub name: String,
    pub description: String,
    /// JSON Schema object for operation arguments.
    pub input_schema: Value,
    /// JSON Schema object for operation results.
    pub output_schema: Value,
}

fn base_output_schema() -> Value {
    json!({
        "type": "object",
        "required": ["success", "operation", "data"],
        "properties": {
            "success": { "type": "boolean" },
            "operation": { "type": "string" },
            "data": {},
            "metadata": { "type": "object", "additionalProperties": true },
            "message": { "type": "string" }
        },
        "additionalProperties": true
    })
}

fn execution_input_schema(operation: &str) -> Value {
    let mut properties = json!({
        "path": {
            "type": "string",
            "description": "Working directory or target path"
        },
        "args": {
            "type": "object",
            "description": "Operation-specific arguments",
            "additionalProperties": true
        },
        "timeout": {
            "type": "integer",
            "description": "Optional timeout in seconds",
            "minimum": 1
        }
    });

    if matches!(operation, "run" | "exec") {
        if let Value::Object(ref mut map) = properties {
            map.insert(
                "code".into(),
                json!({
                    "type": "string",
                    "description": "Source code to execute (language runners)"
                }),
            );
            map.insert(
                "release".into(),
                json!({
                    "type": "boolean",
                    "description": "Build/run in release mode when applicable"
                }),
            );
        }
    }

    json!({
        "type": "object",
        "properties": properties,
        "additionalProperties": true
    })
}

fn persona_input_schema(_operation: &str) -> Value {
    json!({
        "type": "object",
        "properties": {
            "query": {
                "type": "string",
                "description": "User query / prompt to augment with persona expertise"
            },
            "context": {
                "type": "string",
                "description": "Optional conversation or session context"
            },
            "args": {
                "type": "object",
                "additionalProperties": true
            }
        },
        "additionalProperties": true
    })
}

fn orchestration_input_schema(operation: &str) -> Value {
    let mut properties = json!({
        "args": {
            "type": "object",
            "additionalProperties": true
        },
        "context": {
            "type": "string"
        }
    });
    if operation == "run_workflow" {
        if let Value::Object(ref mut map) = properties {
            map.insert(
                "workflow".into(),
                json!({
                    "type": "string",
                    "description": "Named workflow to run"
                }),
            );
        }
    }
    json!({
        "type": "object",
        "properties": properties,
        "additionalProperties": true
    })
}

/// Build the JSON Schema pair for one operation under a category.
pub fn schema_for_operation(
    category: AgentCategory,
    operation: &str,
    agent_description: &str,
) -> AgentOperationSchema {
    let (input_schema, desc_suffix) = match category {
        AgentCategory::Execution => (
            execution_input_schema(operation),
            "execution agent operation",
        ),
        AgentCategory::Persona => (persona_input_schema(operation), "persona consult/review"),
        AgentCategory::Orchestration => (
            orchestration_input_schema(operation),
            "orchestration workflow step",
        ),
    };

    AgentOperationSchema {
        name: operation.to_string(),
        description: format!("{agent_description} — {operation} ({desc_suffix})"),
        input_schema,
        output_schema: base_output_schema(),
    }
}

/// Build schemas for every operation an agent advertises.
pub fn schemas_for_operations(
    category: AgentCategory,
    operations: &[String],
    agent_description: &str,
) -> Vec<AgentOperationSchema> {
    operations
        .iter()
        .map(|op| schema_for_operation(category, op, agent_description))
        .collect()
}

/// Default category when an agent is not in the unified registry.
pub fn infer_category(agent_type: &str, operations: &[String]) -> AgentCategory {
    if agent_type.contains("orchestr")
        || matches!(
            agent_type,
            "memory"
                | "context-manager"
                | "sequential-thinking"
                | "dx-optimizer"
                | "tdd-orchestrator"
        )
    {
        return AgentCategory::Orchestration;
    }
    if operations.iter().any(|o| {
        matches!(
            o.as_str(),
            "run" | "build" | "test" | "check" | "clippy" | "format" | "lint" | "exec"
        )
    }) || agent_type.ends_with("-pro")
        || agent_type.ends_with("-executor")
    {
        return AgentCategory::Execution;
    }
    AgentCategory::Persona
}
