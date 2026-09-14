//! Tool Registry
//!
//! Provides a simple registry for tools and their definitions.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use simd_json::OwnedValue as Value;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::debug;

use crate::tool::BoxedTool;

/// Tool definition metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
    #[serde(default)]
    pub schema_version: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub namespace: String,
}

impl Default for ToolDefinition {
    fn default() -> Self {
        Self {
            name: String::new(),
            description: String::new(),
            input_schema: simd_json::json!({}),
            schema_version: String::new(),
            category: String::new(),
            tags: Vec::new(),
            namespace: String::new(),
        }
    }
}

/// Statistics about the registry
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RegistryStats {
    pub total_registered: usize,
}

/// Tool Registry
pub struct ToolRegistry {
    /// Registered tools
    tools: RwLock<HashMap<Arc<str>, BoxedTool>>,
    /// Tool definitions
    definitions: RwLock<HashMap<Arc<str>, ToolDefinition>>,
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ToolRegistry {
    /// Create a new registry
    pub fn new() -> Self {
        Self {
            tools: RwLock::new(HashMap::new()),
            definitions: RwLock::new(HashMap::new()),
        }
    }

    /// Register a tool with its definition
    pub async fn register(
        &self,
        name: Arc<str>,
        tool: BoxedTool,
        definition: ToolDefinition,
    ) -> Result<()> {
        {
            let mut tools = self.tools.write().await;
            let mut definitions = self.definitions.write().await;

            tools.insert(name.clone(), tool);
            definitions.insert(name.clone(), definition);
        }

        debug!("Registered tool: {}", name);
        Ok(())
    }

    /// Helper to register a tool instance directly
    pub async fn register_tool(&self, tool: BoxedTool) -> Result<()> {
        let definition = ToolDefinition {
            name: tool.name().to_string(),
            description: tool.description().to_string(),
            input_schema: tool.input_schema(),
            schema_version: "https://json-schema.org/draft/next/schema".to_string(),
            category: "builtin".to_string(),
            tags: vec!["builtin".to_string()],
            namespace: tool.namespace().to_string(),
        };
        self.register(Arc::from(tool.name()), tool, definition)
            .await
    }

    /// Get a tool by name
    pub async fn get(&self, name: &str) -> Option<BoxedTool> {
        let tools = self.tools.read().await;
        tools.get(name).cloned()
    }

    /// Get tool definition
    pub async fn get_definition(&self, name: &str) -> Option<ToolDefinition> {
        let definitions = self.definitions.read().await;
        definitions.get(name).cloned()
    }

    /// List all registered tool definitions
    pub async fn list(&self) -> Vec<ToolDefinition> {
        let definitions = self.definitions.read().await;
        definitions.values().cloned().collect()
    }

    /// List currently loaded tools (same as list in simplified version)
    pub async fn list_loaded(&self) -> Vec<ToolDefinition> {
        self.list().await
    }

    /// Get registry statistics
    pub async fn stats(&self) -> RegistryStats {
        let tools = self.tools.read().await;
        RegistryStats {
            total_registered: tools.len(),
        }
    }

    /// Check if a tool is registered
    pub async fn is_loaded(&self, name: &str) -> bool {
        let tools = self.tools.read().await;
        tools.contains_key(name)
    }

    /// Number of tools in the registry
    pub async fn len(&self) -> usize {
        let tools = self.tools.read().await;
        tools.len()
    }

    /// Check if the registry is empty
    pub async fn is_empty(&self) -> bool {
        let tools = self.tools.read().await;
        tools.is_empty()
    }

    /// List all definitions (alias for list)
    pub async fn list_definitions(&self) -> Vec<ToolDefinition> {
        self.list().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::Tool;
    use async_trait::async_trait;

    struct TestTool {
        name: String,
    }

    #[async_trait]
    impl Tool for TestTool {
        fn name(&self) -> &str {
            &self.name
        }

        fn description(&self) -> &str {
            "Test tool"
        }

        fn input_schema(&self) -> Value {
            simd_json::json!({})
        }

        async fn execute(&self, _input: Value) -> Result<Value> {
            Ok(simd_json::json!({"result": "ok"}))
        }
    }

    #[tokio::test]
    async fn test_register_and_get() {
        let registry = ToolRegistry::new();
        let tool: BoxedTool = Arc::new(TestTool {
            name: "test".to_string(),
        });
        let definition = ToolDefinition {
            name: "test".to_string(),
            description: "Test".to_string(),
            input_schema: simd_json::json!({}),
            schema_version: "https://json-schema.org/draft/next/schema".to_string(),
            category: "test".to_string(),
            tags: vec![],
            namespace: "test".to_string(),
        };

        registry
            .register(Arc::from("test"), tool, definition)
            .await
            .unwrap();

        let retrieved = registry.get("test").await;
        assert!(retrieved.is_some());
    }

    #[tokio::test]
    async fn test_list_definitions() {
        let registry = ToolRegistry::new();
        let tool: BoxedTool = Arc::new(TestTool {
            name: "test".to_string(),
        });
        let definition = ToolDefinition {
            name: "test".to_string(),
            description: "Test".to_string(),
            input_schema: simd_json::json!({}),
            schema_version: "https://json-schema.org/draft/next/schema".to_string(),
            category: "test".to_string(),
            tags: vec![],
            namespace: "test".to_string(),
        };

        registry
            .register(Arc::from("test"), tool, definition)
            .await
            .unwrap();

        let definitions = registry.list().await;
        assert_eq!(definitions.len(), 1);
    }

    #[tokio::test]
    async fn test_stats() {
        let registry = ToolRegistry::new();
        let tool: BoxedTool = Arc::new(TestTool {
            name: "test".to_string(),
        });
        let definition = ToolDefinition {
            name: "test".to_string(),
            description: "Test".to_string(),
            input_schema: simd_json::json!({}),
            schema_version: "https://json-schema.org/draft/next/schema".to_string(),
            category: "test".to_string(),
            tags: vec![],
            namespace: "test".to_string(),
        };

        registry
            .register(Arc::from("test"), tool, definition)
            .await
            .unwrap();

        // Access the tool
        registry.get("test").await;
        registry.get("test").await;

        let stats = registry.stats().await;
        assert_eq!(stats.total_registered, 1);
    }
}
