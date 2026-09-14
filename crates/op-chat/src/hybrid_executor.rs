//! Hybrid Executor: Intent-First with LLM Fallback
//!
//! This module combines:
//! 1. IntentExecutor for deterministic system operations
//! 2. LLM for complex queries, explanations, and general chat
//!
//! The flow is:
//! - User input → IntentExecutor.is_system_operation()?
//!   - YES → Execute tool directly (no LLM)
//!   - NO → Send to LLM for response

use anyhow::Result;
use simd_json::{json, OwnedValue as Value};
use std::sync::Arc;
use tracing::{debug, info, warn};

use op_llm::provider::{ChatMessage, ChatResponse, LlmProvider};
use op_tools::ToolRegistry;

use crate::intent_executor::{IntentExecutor, IntentExecutionResult};

/// Result from hybrid execution
#[derive(Debug)]
pub struct HybridResult {
    /// The response to show the user
    pub response: String,
    /// Whether a tool was executed
    pub tool_executed: bool,
    /// Name of executed tool (if any)
    pub tool_name: Option<String>,
    /// Whether this was handled by LLM
    pub llm_handled: bool,
    /// Execution details
    pub details: HybridDetails,
}

/// Details about hybrid execution
#[derive(Debug)]
pub enum HybridDetails {
    /// Tool was executed directly
    ToolExecution(IntentExecutionResult),
    /// LLM generated response
    LlmResponse(ChatResponse),
    /// Error occurred
    Error(String),
}

/// Hybrid executor combining intent-based and LLM-based execution
pub struct HybridExecutor {
    intent_executor: IntentExecutor,
    tool_registry: Arc<ToolRegistry>,
}

impl HybridExecutor {
    /// Create new hybrid executor
    pub fn new(tool_registry: Arc<ToolRegistry>) -> Self {
        Self {
            intent_executor: IntentExecutor::new(tool_registry.clone()),
            tool_registry,
        }
    }

    /// Process user input with hybrid approach
    ///
    /// 1. Check if input is a system operation
    /// 2. If yes, execute tool directly
    /// 3. If no, use LLM for response
    pub async fn process<P: LlmProvider>(
        &self,
        input: &str,
        provider: &P,
        model: &str,
        context: Vec<ChatMessage>,
    ) -> Result<HybridResult> {
        // Check if this is a system operation
        if self.intent_executor.is_system_operation(input) {
            info!("Detected system operation, executing directly");
            return self.execute_tool_directly(input).await;
        }

        // Check for explicit tool invocation
        if let Some(tool_invocation) = self.parse_explicit_tool_invocation(input) {
            info!("Explicit tool invocation: {:?}", tool_invocation);
            return self.execute_explicit_tool(tool_invocation).await;
        }

        // Fall back to LLM
        info!("Not a system operation, using LLM");
        self.execute_via_llm(input, provider, model, context).await
    }

    /// Execute tool directly based on intent
    async fn execute_tool_directly(&self, input: &str) -> Result<HybridResult> {
        let result = self.intent_executor.execute(input).await?;

        Ok(HybridResult {
            response: result.response.clone(),
            tool_executed: result.executed_tool.is_some(),
            tool_name: result.executed_tool.clone(),
            llm_handled: false,
            details: HybridDetails::ToolExecution(result),
        })
    }

    /// Parse explicit tool invocation like "@tool_name {args}"
    fn parse_explicit_tool_invocation(&self, input: &str) -> Option<(String, Value)> {
        // Pattern: @tool_name {"arg": "value"}
        if !input.starts_with('@') {
            return None;
        }

        let parts: Vec<&str> = input[1..].splitn(2, ' ').collect();
        if parts.is_empty() {
            return None;
        }

        let tool_name = parts[0].to_string();
        if parts.len() > 1 && parts[1].trim().starts_with('{') {
            serde_json::from_str(parts[1]).unwrap_or(json!({}))
        } else {
            json!({})
        };

        Some((tool_name, args))
    }

    /// Execute explicitly invoked tool
    async fn execute_explicit_tool(&self, (tool_name, args): (String, Value)) -> Result<HybridResult> {
        let tool = match self.tool_registry.get(&tool_name).await {
            Some(t) => t,
            None => {
                return Ok(HybridResult {
                    response: format!("Tool '{}' not found", tool_name),
                    tool_executed: false,
                    tool_name: Some(tool_name),
                    llm_handled: false,
                    details: HybridDetails::Error("Tool not found".to_string()),
                });
            }
        };

        let request = op_core::ToolRequest {
            id: uuid::Uuid::new_v4().to_string(),
            tool_name: tool_name.clone(),
            arguments: args,
            timeout_ms: Some(30000),
        };

        let result = tool.execute(request).await;

        let response = if result.success {
            format!(
                "✅ Tool '{}' executed successfully:\n{}",
                tool_name,
                simd_json::to_string_pretty(&result.content).unwrap_or_default()
            )
        } else {
            format!(
                "❌ Tool '{}' failed:\n{}",
                tool_name,
                simd_json::to_string_pretty(&result.content).unwrap_or_default()
            )
        };

        Ok(HybridResult {
            response,
            tool_executed: true,
            tool_name: Some(tool_name),
            llm_handled: false,
            details: HybridDetails::Error("Direct execution".to_string()),
        })
    }

    /// Execute via LLM (for non-system operations)
    async fn execute_via_llm<P: LlmProvider>(
        &self,
        input: &str,
        provider: &P,
        model: &str,
        mut context: Vec<ChatMessage>,
    ) -> Result<HybridResult> {
        // Add user message
        context.push(ChatMessage::user(input));

        // Call LLM
        let response = provider.chat(model, context).await?;

        Ok(HybridResult {
            response: response.message.content.clone(),
            tool_executed: false,
            tool_name: None,
            llm_handled: true,
            details: HybridDetails::LlmResponse(response),
        })
    }

    /// Get the intent executor for direct access
    pub fn intent_executor(&self) -> &IntentExecutor {
        &self.intent_executor
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_explicit_tool_invocation() {
        let registry = Arc::new(ToolRegistry::new());
        let executor = HybridExecutor::new(registry);

        let result = executor.parse_explicit_tool_invocation("@ovs_list_bridges {}");
        assert!(result.is_some());
        let (name, _args) = result.unwrap();
        assert_eq!(name, "ovs_list_bridges");

        let result = executor.parse_explicit_tool_invocation("not a tool");
        assert!(result.is_none());
    }
}
