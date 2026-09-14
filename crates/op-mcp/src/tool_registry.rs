//! Tool Registry - All Tools Always Loaded
//!
//! This replaces lazy_tools.rs with a simple registry that:
//! - Loads ALL tools at startup
//! - Never evicts tools
//! - Provides fast lookup for execute_tool
//!
//! The compact mode meta-tools use this registry to:
//! - list_tools: Paginate through all registered tools
//! - search_tools: Filter by name/description/category
//! - get_tool_schema: Return input schema for a tool
//! - execute_tool: Look up and execute any tool

use anyhow::Result;
use async_trait::async_trait;
use simd_json::prelude::*;
use simd_json::OwnedValue as Value;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::debug;

use crate::server::{ToolExecutor, ToolInfo};
use op_core::ToolDefinition;

/// Server-side metadata candidate for one registered cognitive tool.
///
/// Authority is optional here because a runtime tool may not have a matching
/// sealed `PluginSchema::methods` declaration. The bridge must project these
/// fields through the sealed schema before using them for authorization; an
/// absent or non-matching declaration remains registered but is not callable.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ToolDescriptor {
    #[serde(flatten)]
    pub definition: ToolDefinition,
    pub required_capability: Option<String>,
    pub subid: Option<String>,
}

/// Tool trait - same as op_tools::Tool but standalone
#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn input_schema(&self) -> Value;
    /// Capability candidate; authoritative only when a sealed method matches.
    fn required_capability(&self) -> Option<&str> {
        None
    }
    /// Method subid candidate; authoritative only when a sealed method matches.
    fn subid(&self) -> Option<&str> {
        None
    }
    fn category(&self) -> &str {
        "general"
    }
    fn namespace(&self) -> &str {
        "system"
    }
    fn tags(&self) -> Vec<String> {
        vec![]
    }
    async fn execute(&self, input: Value) -> Result<Value>;
}

pub type BoxedTool = Arc<dyn Tool>;

/// Simple tool registry - NO eviction, all tools always available
pub struct ToolRegistry {
    tools: RwLock<HashMap<String, BoxedTool>>,
    descriptors: RwLock<HashMap<String, ToolDescriptor>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: RwLock::new(HashMap::new()),
            descriptors: RwLock::new(HashMap::new()),
        }
    }

    /// Register a tool (never evicted)
    pub async fn register(&self, tool: BoxedTool) -> Result<()> {
        let name = tool.name().trim().to_string();
        let required_capability = tool
            .required_capability()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        let subid = tool
            .subid()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        if name.is_empty() {
            anyhow::bail!("tool registration denied: name is empty");
        }
        let input_schema = tool.input_schema();
        if !input_schema.is_object() {
            anyhow::bail!("tool registration denied for '{name}': input schema is not an object");
        }
        let definition = ToolDefinition {
            name: name.clone(),
            description: tool.description().to_string(),
            input_schema,
            schema_version: String::new(),
            category: tool.category().to_string(),
            tags: tool.tags(),
            namespace: tool.namespace().to_string(),
        };
        let descriptor = ToolDescriptor {
            definition,
            required_capability,
            subid,
        };

        // Hold both write guards through the check+insert so concurrent
        // registration cannot silently replace either half of the entry.
        let mut tools = self.tools.write().await;
        if tools.contains_key(&name) {
            anyhow::bail!("duplicate tool registration denied: '{name}'");
        }
        let mut descriptors = self.descriptors.write().await;
        if descriptors.contains_key(&name) {
            anyhow::bail!("duplicate tool descriptor registration denied: '{name}'");
        }
        tools.insert(name.clone(), tool);
        descriptors.insert(name.clone(), descriptor);

        debug!("Registered tool: {}", name);
        Ok(())
    }

    /// Get a tool by name (instant lookup, no loading)
    pub async fn get(&self, name: &str) -> Option<BoxedTool> {
        self.tools.read().await.get(name).cloned()
    }

    /// Execute a tool by name
    pub async fn execute(&self, name: &str, input: Value) -> Result<Value> {
        let tool = self
            .get(name)
            .await
            .ok_or_else(|| anyhow::anyhow!("Tool not found: {}", name))?;
        tool.execute(input).await
    }

    /// Get tool definition
    pub async fn get_definition(&self, name: &str) -> Option<ToolDefinition> {
        self.get_descriptor(name)
            .await
            .map(|descriptor| descriptor.definition)
    }

    /// Resolve the authoritative descriptor for a named tool.
    pub async fn get_descriptor(&self, name: &str) -> Option<ToolDescriptor> {
        self.descriptors.read().await.get(name).cloned()
    }

    /// List all tools (paginated)
    pub async fn list(
        &self,
        offset: usize,
        limit: usize,
        category: Option<&str>,
    ) -> Vec<ToolDefinition> {
        let descriptors = self.descriptors.read().await;

        let filtered: Vec<_> = descriptors
            .values()
            .map(|descriptor| &descriptor.definition)
            .filter(|definition| category.is_none_or(|c| definition.category == c))
            .cloned()
            .collect();

        filtered.into_iter().skip(offset).take(limit).collect()
    }

    /// List authoritative descriptors for trusted bridge-side policy checks.
    pub async fn list_descriptors(&self) -> Vec<ToolDescriptor> {
        self.descriptors.read().await.values().cloned().collect()
    }

    /// Search tools by query
    pub async fn search(&self, query: &str) -> Vec<ToolDefinition> {
        let query_lower = query.to_lowercase();
        let descriptors = self.descriptors.read().await;

        descriptors
            .values()
            .map(|descriptor| &descriptor.definition)
            .filter(|definition| {
                definition.name.to_lowercase().contains(&query_lower)
                    || definition.description.to_lowercase().contains(&query_lower)
                    || definition.category.to_lowercase().contains(&query_lower)
                    || definition
                        .tags
                        .iter()
                        .any(|t| t.to_lowercase().contains(&query_lower))
            })
            .take(50) // Reasonable limit for search results
            .cloned()
            .collect()
    }

    /// Total tool count
    pub async fn count(&self) -> usize {
        self.tools.read().await.len()
    }

    /// Get all categories
    pub async fn categories(&self) -> Vec<String> {
        let descriptors = self.descriptors.read().await;
        let mut cats: Vec<String> = descriptors
            .values()
            .map(|descriptor| descriptor.definition.category.clone())
            .filter(|c| !c.is_empty())
            .collect();
        cats.sort();
        cats.dedup();
        cats
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Registry executor for CompactServer
pub struct RegistryExecutor {
    registry: Arc<ToolRegistry>,
}

impl RegistryExecutor {
    pub fn new(registry: Arc<ToolRegistry>) -> Self {
        Self { registry }
    }
}

#[async_trait]
impl ToolExecutor for RegistryExecutor {
    async fn execute_tool(&self, tool_name: &str, arguments: Value) -> Result<Value> {
        self.registry.execute(tool_name, arguments).await
    }

    async fn list_tools(&self) -> Result<Vec<ToolInfo>> {
        let tools = self.registry.list(0, 1000, None).await;
        Ok(tools
            .into_iter()
            .map(|t| ToolInfo {
                name: t.name,
                description: t.description,
                input_schema: t.input_schema,
                annotations: None,
            })
            .collect())
    }

    async fn get_tool_schema(&self, name: &str) -> Result<Option<Value>> {
        Ok(self
            .registry
            .get_definition(name)
            .await
            .map(|d| d.input_schema))
    }

    async fn search_tools(&self, query: &str, limit: usize) -> Result<Vec<ToolInfo>> {
        let tools = self.registry.search(query).await;
        Ok(tools
            .into_iter()
            .take(limit)
            .map(|t| ToolInfo {
                name: t.name,
                description: t.description,
                input_schema: t.input_schema,
                annotations: None,
            })
            .collect())
    }
}

#[cfg(test)]
mod descriptor_tests {
    use super::*;
    use simd_json::json;

    struct TestTool {
        name: &'static str,
        capability: &'static str,
        subid: &'static str,
    }

    #[async_trait]
    impl Tool for TestTool {
        fn name(&self) -> &str {
            self.name
        }

        fn description(&self) -> &str {
            "test tool"
        }

        fn input_schema(&self) -> Value {
            json!({"type": "object"})
        }

        fn required_capability(&self) -> Option<&str> {
            Some(self.capability)
        }

        fn subid(&self) -> Option<&str> {
            Some(self.subid)
        }

        async fn execute(&self, input: Value) -> Result<Value> {
            Ok(input)
        }
    }

    fn test_tool(name: &'static str, capability: &'static str, subid: &'static str) -> BoxedTool {
        Arc::new(TestTool {
            name,
            capability,
            subid,
        })
    }

    #[tokio::test]
    async fn duplicate_registration_is_rejected_without_replacement() {
        let registry = ToolRegistry::new();
        registry
            .register(test_tool(
                "safe_read",
                "cognitive_mcp.read",
                "obs.service.cognitive-mcp.test.read@v1",
            ))
            .await
            .unwrap();
        let error = registry
            .register(test_tool(
                "safe_read",
                "cognitive_mcp.invoke",
                "mut.service.cognitive-mcp.test.replace@v1",
            ))
            .await
            .unwrap_err();
        assert!(error.to_string().contains("duplicate"));
        let descriptor = registry.get_descriptor("safe_read").await.unwrap();
        assert_eq!(
            descriptor.required_capability.as_deref(),
            Some("cognitive_mcp.read")
        );
    }

    #[tokio::test]
    async fn missing_authority_is_recorded_as_absent_for_fail_closed_projection() {
        let registry = ToolRegistry::new();
        registry
            .register(test_tool(
                "missing_capability",
                "",
                "obs.service.cognitive-mcp.test.read@v1",
            ))
            .await
            .unwrap();

        registry
            .register(test_tool("missing_subid", "cognitive_mcp.read", ""))
            .await
            .unwrap();
        assert!(registry
            .get_descriptor("missing_capability")
            .await
            .unwrap()
            .required_capability
            .is_none());
        assert!(registry
            .get_descriptor("missing_subid")
            .await
            .unwrap()
            .subid
            .is_none());
        assert_eq!(registry.count().await, 2);
    }

    #[tokio::test]
    async fn safe_read_descriptor_registers_and_executes() {
        let registry = ToolRegistry::new();
        registry
            .register(test_tool(
                "safe_read",
                "cognitive_mcp.read",
                "obs.service.cognitive-mcp.test.read@v1",
            ))
            .await
            .unwrap();
        let descriptor = registry.get_descriptor("safe_read").await.unwrap();
        assert_eq!(
            descriptor.required_capability.as_deref(),
            Some("cognitive_mcp.read")
        );
        let serialized = simd_json::serde::to_owned_value(&descriptor).unwrap();
        assert_eq!(serialized["name"], "safe_read");
        assert_eq!(serialized["required_capability"], "cognitive_mcp.read");
        assert_eq!(
            serialized["subid"],
            "obs.service.cognitive-mcp.test.read@v1"
        );
        assert_eq!(
            registry
                .execute("safe_read", json!({"key": "status"}))
                .await
                .unwrap(),
            json!({"key": "status"})
        );
    }
}
