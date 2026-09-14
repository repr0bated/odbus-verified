//! Core Tool trait and types
//!
//! Defines the fundamental interface for all tools in the system.

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use simd_json::OwnedValue as Value;
use std::sync::Arc;

/// Security level for tool operations
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SecurityLevel {
    /// Safe read-only operations
    #[default]
    ReadOnly,
    /// Operations that modify state but are reversible
    Modify,
    /// Operations that may have significant impact
    Elevated,
    /// Operations requiring explicit approval
    Critical,
}

/// Core trait for all tools
#[async_trait]
pub trait Tool: Send + Sync {
    /// Get the tool name (unique identifier)
    fn name(&self) -> &str;

    /// Get human-readable description
    fn description(&self) -> &str;

    /// Get JSON schema for input validation
    fn input_schema(&self) -> Value;

    /// Execute the tool with given input
    async fn execute(&self, input: Value) -> Result<Value>;

    /// Get the security level for this tool
    fn security_level(&self) -> SecurityLevel {
        SecurityLevel::ReadOnly
    }

    /// Get the category this tool belongs to
    fn category(&self) -> &str {
        "general"
    }

    /// Get the namespace for tool permission gating
    fn namespace(&self) -> &str {
        "system"
    }

    /// Get tags for tool discovery
    fn tags(&self) -> Vec<String> {
        vec![]
    }

    /// Check if tool is available (e.g., dependencies met)
    fn is_available(&self) -> bool {
        true
    }

    /// Estimated execution time in milliseconds
    fn estimated_duration_ms(&self) -> Option<u64> {
        None
    }
}

/// Type alias for boxed tools
pub type BoxedTool = Arc<dyn Tool>;

/// Simple tool implementation for testing
#[derive(Clone)]
pub struct SimpleTool {
    name: String,
    description: String,
    schema: Value,
    handler: Arc<dyn Fn(Value) -> Result<Value> + Send + Sync>,
}

impl SimpleTool {
    pub fn new<F>(name: &str, description: &str, schema: Value, handler: F) -> Self
    where
        F: Fn(Value) -> Result<Value> + Send + Sync + 'static,
    {
        Self {
            name: name.to_string(),
            description: description.to_string(),
            schema,
            handler: Arc::new(handler),
        }
    }
}

#[async_trait]
impl Tool for SimpleTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> Value {
        self.schema.clone()
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        (self.handler)(input)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_simple_tool() {
        let tool = SimpleTool::new(
            "echo",
            "Echo input back",
            simd_json::json!({"type": "object"}),
            Ok,
        );

        assert_eq!(tool.name(), "echo");
        assert_eq!(tool.description(), "Echo input back");

        let result = tool
            .execute(simd_json::json!({"msg": "hello"}))
            .await
            .unwrap();
        assert_eq!(result, simd_json::json!({"msg": "hello"}));
    }
}
