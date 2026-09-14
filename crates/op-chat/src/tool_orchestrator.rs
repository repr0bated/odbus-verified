//! Tool Orchestrator - Manages LLM ↔ Tool communication loop
//!
//! This is the MISSING piece that connects:
//! - HuggingFace (or any LLM) tool_calls
//! - op-tools ToolRegistry execution
//! - Response accumulation back to LLM

use anyhow::{Context, Result};
use simd_json::{json, OwnedValue as Value};
use std::sync::Arc;
use tracing::{debug, info, warn};

use op_core::{ToolRequest, ToolResult};
use op_llm::provider::{
    ChatMessage, ChatRequest, ChatResponse, LlmProvider, ToolCallInfo, ToolChoice, ToolDefinition,
};
use op_tools::ToolRegistry;

use crate::tool_executor::TrackedToolExecutor;

/// Maximum tool call iterations to prevent infinite loops
const MAX_TOOL_ITERATIONS: usize = 10;

/// Tool Orchestrator - manages the LLM ↔ Tool loop
pub struct ToolOrchestrator {
    tool_registry: Arc<ToolRegistry>,
    tool_executor: Arc<TrackedToolExecutor>,
}

impl ToolOrchestrator {
    pub fn new(
        tool_registry: Arc<ToolRegistry>,
        tool_executor: Arc<TrackedToolExecutor>,
    ) -> Self {
        Self {
            tool_registry,
            tool_executor,
        }
    }

    /// Get all tools as ToolDefinition for LLM
    pub async fn get_tool_definitions(&self) -> Vec<ToolDefinition> {
        let tools = self.tool_registry.list().await;
        
        tools
            .into_iter()
            .map(|tool| ToolDefinition {
                name: tool.name().to_string(),
                description: tool.description().to_string(),
                parameters: tool.input_schema(),
            })
            .collect()
    }

    /// Execute a tool call from LLM
    async fn execute_tool_call(&self, tool_call: &ToolCallInfo) -> Result<Value> {
        info!("Executing tool: {} (id: {})", tool_call.name, tool_call.id);
        debug!("Tool arguments: {:?}", tool_call.arguments);

        let result = self
            .tool_executor
            .execute(&tool_call.name, tool_call.arguments.clone(), None)
            .await
            .context(format!("Failed to execute tool: {}", tool_call.name))?;

        if result.success() {
            Ok(result.result.result.unwrap_or(json!({"success": true})))
        } else {
            let error = result.error().cloned().unwrap_or_else(|| "Unknown error".to_string());
            Ok(json!({"error": error}))
        }
    }

    /// Run the full chat loop with tool execution
    ///
    /// This is the main orchestration function that:
    /// 1. Sends messages + tools to LLM
    /// 2. If LLM returns tool_calls, executes them
    /// 3. Sends results back to LLM
    /// 4. Repeats until LLM returns final text response
    pub async fn chat_with_tools<P: LlmProvider>(
        &self,
        provider: &P,
        model: &str,
        mut messages: Vec<ChatMessage>,
        tool_choice: ToolChoice,
    ) -> Result<ChatResponse> {
        let tools = self.get_tool_definitions().await;
        
        info!(
            "Starting tool-enabled chat: model={}, tools={}, tool_choice={:?}",
            model,
            tools.len(),
            tool_choice
        );

        let mut iterations = 0;

        loop {
            iterations += 1;
            if iterations > MAX_TOOL_ITERATIONS {
                warn!("Max tool iterations ({}) reached, forcing stop", MAX_TOOL_ITERATIONS);
                break;
            }

            // Build request with tools
            let request = ChatRequest {
                messages: messages.clone(),
                tools: tools.clone(),
                tool_choice: tool_choice.clone(),
                max_tokens: None,
                temperature: Some(0.7),
                top_p: None,
            };

            // Send to LLM
            let response = provider
                .chat_with_request(model, request)
                .await
                .context("LLM chat request failed")?;

            debug!(
                "LLM response: finish_reason={:?}, tool_calls={:?}",
                response.finish_reason,
                response.tool_calls.as_ref().map(|tc| tc.len())
            );

            // Check if LLM wants to call tools
            match &response.tool_calls {
                Some(tool_calls) if !tool_calls.is_empty() => {
                    info!("LLM requested {} tool calls", tool_calls.len());

                    // Add assistant message with tool calls
                    messages.push(response.message.clone());

                    // Execute each tool and add results
                    for tool_call in tool_calls {
                        let result = self.execute_tool_call(tool_call).await?;
                        
                        // Add tool result message
                        messages.push(ChatMessage {
                            role: "tool".to_string(),
                            content: simd_json::to_string(&result)?,
                            tool_calls: None,
                            tool_call_id: Some(tool_call.id.clone()),
                        });
                    }

                    // Continue loop to send results back to LLM
                    continue;
                }
                _ => {
                    // No tool calls, return final response
                    info!("LLM returned final response after {} iterations", iterations);
                    return Ok(response);
                }
            }
        }

        // Fallback: return last response if max iterations reached
        Err(anyhow::anyhow!("Tool orchestration exceeded max iterations"))
    }

    /// Simple chat without tool loop (single turn)
    pub async fn chat_single_turn<P: LlmProvider>(
        &self,
        provider: &P,
        model: &str,
        messages: Vec<ChatMessage>,
    ) -> Result<ChatResponse> {
        let tools = self.get_tool_definitions().await;
        
        let request = ChatRequest {
            messages,
            tools,
            tool_choice: ToolChoice::Auto,
            max_tokens: None,
            temperature: Some(0.7),
            top_p: None,
        };

        provider.chat_with_request(model, request).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_tool_definitions() {
        // Would need mock registry
    }
}
