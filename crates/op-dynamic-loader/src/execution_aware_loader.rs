use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;

use crate::{DynamicToolRegistry, SmartLoadingStrategy};
use op_execution_tracker::{ExecutionContext, ExecutionTracker};
use op_tools::ToolRegistry;

/// Execution-aware tool loader
pub struct ExecutionAwareLoader {
    /// Dynamic registry
    dynamic_registry: Arc<DynamicToolRegistry>,

    /// Execution tracker
    execution_tracker: Arc<ExecutionTracker>,
}

impl ExecutionAwareLoader {
    /// Create new execution-aware loader
    pub fn new(
        base_registry: Arc<ToolRegistry>,
        execution_tracker: Arc<ExecutionTracker>,
        max_cache_size: usize,
    ) -> Self {
        let loading_strategy = Arc::new(SmartLoadingStrategy::new(
            Arc::clone(&execution_tracker),
            300, // 5 minute base TTL
        ));

        let dynamic_registry = Arc::new(DynamicToolRegistry::new(
            base_registry,
            Arc::clone(&execution_tracker),
            loading_strategy,
            max_cache_size,
        ));

        Self {
            dynamic_registry,
            execution_tracker,
        }
    }

    /// Get tool with execution-aware loading
    pub async fn get_tool_with_context(
        &self,
        tool_name: &str,
        context: &ExecutionContext,
    ) -> Result<op_tools::BoxedTool> {
        self.dynamic_registry.get_tool(tool_name, context).await
    }

    /// Get tool with automatic context creation
    pub async fn get_tool(&self, tool_name: &str) -> Result<op_tools::BoxedTool> {
        let context = ExecutionContext::new(tool_name);
        self.get_tool_with_context(tool_name, &context).await
    }

    /// Get cache statistics
    pub async fn get_cache_stats(&self) -> (u64, u64) {
        self.dynamic_registry.get_cache_stats().await
    }

    /// Get current cache size
    pub async fn get_cache_size(&self) -> usize {
        self.dynamic_registry.get_cache_size().await
    }

    /// Get base registry (for compatibility)
    pub fn base_registry(&self) -> Arc<ToolRegistry> {
        self.dynamic_registry.base_registry()
    }

    /// Get execution tracker (for compatibility)
    pub fn execution_tracker(&self) -> Arc<ExecutionTracker> {
        Arc::clone(&self.execution_tracker)
    }
}

/// Execution-aware tool registry trait
#[async_trait]
pub trait ExecutionAwareToolRegistry: Send + Sync {
    /// Get tool with execution context
    async fn get_tool_with_context(
        &self,
        tool_name: &str,
        context: &ExecutionContext,
    ) -> Result<op_tools::BoxedTool>;

    /// Get tool with automatic context
    async fn get_tool(&self, tool_name: &str) -> Result<op_tools::BoxedTool>;

    /// Get cache statistics
    async fn get_cache_stats(&self) -> (u64, u64);
}

#[async_trait]
impl ExecutionAwareToolRegistry for ExecutionAwareLoader {
    async fn get_tool_with_context(
        &self,
        tool_name: &str,
        context: &ExecutionContext,
    ) -> Result<op_tools::BoxedTool> {
        self.get_tool_with_context(tool_name, context).await
    }

    async fn get_tool(&self, tool_name: &str) -> Result<op_tools::BoxedTool> {
        self.get_tool(tool_name).await
    }

    async fn get_cache_stats(&self) -> (u64, u64) {
        self.get_cache_stats().await
    }
}
