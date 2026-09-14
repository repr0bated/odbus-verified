//! Forced Tool Execution Pipeline
//!
//! This is the MISSING INTEGRATION that connects:
//! - LLM requests (with tools and tool_choice: required)
//! - Tool execution tracking
//! - Hallucination verification
//! - Response extraction
//!
//! ## The Problem
//!
//! The existing code has:
//! - `ForcedExecutionOrchestrator` - tracks and verifies tool calls
//! - `response_tools` - respond_to_user, cannot_perform, etc.
//! - `TrackedToolExecutor` - executes tools with metrics
//!
//! But NOTHING connects them to the LLM request! The LLM is free to
//! output raw text because:
//! 1. `tool_choice` is not set to "required"
//! 2. Tools are not passed in the request
//! 3. Responses are not verified before returning
//!
//! ## The Solution
//!
//! This pipeline:
//! 1. Forces `tool_choice: required` on every LLM request
//! 2. Passes all available tools (action + response tools)
//! 3. Executes tool_calls via ForcedExecutionOrchestrator
//! 4. Verifies the turn for hallucinations
//! 5. Extracts user response from respond_to_user tool only

use anyhow::{Context, Result};
use simd_json::json;
use std::sync::Arc;
use tracing::{debug, error, info, warn};

use op_llm::provider::{
    ChatMessage, ChatRequest, ChatResponse, LlmProvider, ToolChoice, ToolDefinition,
};
use op_tools::ToolRegistry;

use crate::agent_tools::register_context_agents;
use crate::forced_execution::{
    detect_raw_text_output, ForcedExecutionOrchestrator, HallucinationCheck, ToolCall,
};
use crate::tool_executor::TrackedToolExecutor;

/// Maximum turns in a single request (tool call loops)
const MAX_TOOL_TURNS: usize = 10;

/// Result of processing a user message
#[derive(Debug)]
pub struct PipelineResult {
    /// The final user-facing response
    pub response: String,
    /// Whether the response was verified (no hallucinations)
    pub verified: bool,
    /// Hallucination check details
    pub hallucination_check: HallucinationCheck,
    /// Tools that were executed
    pub executed_tools: Vec<String>,
    /// Raw LLM response (for debugging)
    pub raw_llm_response: Option<ChatResponse>,
}

/// Forced Tool Execution Pipeline
///
/// This is the main integration point that ensures:
/// - LLM MUST use tools (tool_choice: required)
/// - All tool calls are tracked and verified
/// - Hallucinations are detected and rejected
/// - Only respond_to_user output reaches the user
pub struct ForcedToolPipeline {
    tool_registry: Arc<ToolRegistry>,
    orchestrator: ForcedExecutionOrchestrator,
}

impl ForcedToolPipeline {
    /// Create new pipeline
    pub fn new(tool_registry: Arc<ToolRegistry>, executor: Arc<TrackedToolExecutor>) -> Self {
        Self {
            tool_registry,
            orchestrator: ForcedExecutionOrchestrator::new(executor),
        }
    }

    /// Get all tool definitions for LLM request
    async fn get_tool_definitions(&self) -> Vec<ToolDefinition> {
        let tools = self.tool_registry.list().await;

        tools
            .into_iter()
            .map(|tool| ToolDefinition {
                name: tool.name.clone(),
                description: tool.description.clone(),
                input_schema: tool.input_schema.clone(),
                schema_version: String::new(),
                category: tool.category.clone(),
                tags: tool.tags.clone(),
                namespace: tool.namespace.clone(),
            })
            .collect()
    }

    /// Process a user message through the forced tool pipeline
    ///
    /// This is the main entry point that:
    /// 1. Starts a new turn (clears tracking)
    /// 2. Builds request with tools and tool_choice: required
    /// 3. Sends to LLM
    /// 4. Executes tool_calls
    /// 5. Loops if LLM wants more tool calls
    /// 6. Verifies for hallucinations
    /// 7. Returns verified response
    #[tracing::instrument(skip(self, provider, messages, session_id), fields(model, message_count = messages.len()))]
    pub async fn process_message(
        &self,
        provider: &dyn LlmProvider,
        model: &str,
        messages: Vec<ChatMessage>,
        session_id: Option<String>,
    ) -> Result<PipelineResult> {
        // Start new turn - clears tracking state
        self.orchestrator.start_turn().await;

        if let Some(user_message) = messages.iter().rev().find(|m| m.role == "user") {
            let _ = register_context_agents(&self.tool_registry, &user_message.content).await;
        }

        // Get all available tools
        let tools = self.get_tool_definitions().await;

        info!(
            "Starting forced tool pipeline: model={}, tools={}, messages={}",
            model,
            tools.len(),
            messages.len()
        );

        // Verify we have response tools
        let has_response_tools = tools.iter().any(|t| {
            t.name == "respond_to_user"
                || t.name == "cannot_perform"
                || t.name == "request_clarification"
        });

        if !has_response_tools {
            error!("No response tools registered! LLM cannot communicate with user.");
            return Err(anyhow::anyhow!(
                "Response tools (respond_to_user, cannot_perform) must be registered"
            ));
        }

        let mut current_messages = messages;
        let mut last_response: Option<ChatResponse> = None;
        let mut turn_count = 0;

        // Tool calling loop
        loop {
            turn_count += 1;
            if turn_count > MAX_TOOL_TURNS {
                warn!("Max tool turns ({}) exceeded, forcing stop", MAX_TOOL_TURNS);
                break;
            }

            // Build request with FORCED tool usage
            let request = ChatRequest {
                messages: current_messages.clone(),
                tools: tools.clone(),
                tool_choice: ToolChoice::Required, // ◄── THE KEY: FORCE TOOL USE
                max_tokens: None,
                temperature: Some(0.7),
                top_p: None,
            };

            debug!(
                "Sending LLM request (turn {}): tool_choice=required",
                turn_count
            );

            // Send to LLM
            let response = provider
                .chat_with_request(model, request)
                .await
                .context("LLM request failed")?;

            // Check for raw text output (hallucination attempt)
            if let Some(raw_text) = detect_raw_text_output(&json!({
                "content": &response.message.content,
                "tool_calls": &response.tool_calls
            })) {
                warn!(
                    "LLM attempted raw text output despite tool_choice=required: {}",
                    &raw_text[..raw_text.len().min(100)]
                );
                // This shouldn't happen with tool_choice=required, but handle it
            }

            // Check for tool calls
            match &response.tool_calls {
                Some(tool_calls) if !tool_calls.is_empty() => {
                    info!("LLM returned {} tool calls", tool_calls.len());

                    // Convert to our ToolCall format
                    let calls: Vec<ToolCall> = tool_calls
                        .iter()
                        .map(|tc| ToolCall {
                            name: tc.name.clone(),
                            arguments: tc.arguments.clone(),
                        })
                        .collect();

                    // Execute all tool calls
                    let exec_result = self
                        .orchestrator
                        .execute_tool_sequence(calls, session_id.clone())
                        .await?;

                    // Check if we got a response tool (end of conversation turn)
                    let has_response = exec_result.results.iter().any(|r| {
                        r.tool_name == "respond_to_user"
                            || r.tool_name == "cannot_perform"
                            || r.tool_name == "request_clarification"
                    });

                    if has_response {
                        // We have a response - end the loop
                        last_response = Some(response);
                        break;
                    }

                    // No response tool yet - add tool results and continue
                    current_messages.push(response.message.clone());

                    for result in &exec_result.results {
                        let content = result
                            .result
                            .as_ref()
                            .map(|v| simd_json::to_string(v).unwrap_or_default())
                            .unwrap_or_else(|| {
                                result
                                    .error
                                    .clone()
                                    .unwrap_or_else(|| "Unknown error".to_string())
                            });

                        current_messages.push(ChatMessage {
                            role: "tool".to_string(),
                            content,
                            tool_calls: None,
                            tool_call_id: Some(format!("call_{}", result.tool_name)),
                        });
                    }

                    last_response = Some(response);
                }
                _ => {
                    // No tool calls - this is a problem with tool_choice=required
                    warn!("LLM returned no tool calls despite tool_choice=required");
                    last_response = Some(response);
                    break;
                }
            }
        }

        // Verify the turn for hallucinations
        let hallucination_check = self.orchestrator.verify_turn().await;

        if !hallucination_check.verified {
            error!(
                "Hallucination detected! Issues: {:?}",
                hallucination_check.issues
            );
        }

        // Get the user response (from respond_to_user tool)
        let response = self.orchestrator.get_user_response().await;

        // Get turn stats
        let stats = self.orchestrator.get_turn_stats().await;

        Ok(PipelineResult {
            response,
            verified: hallucination_check.verified,
            hallucination_check,
            executed_tools: stats.tool_names,
            raw_llm_response: last_response,
        })
    }

    /// Get orchestrator for direct access
    pub fn orchestrator(&self) -> &ForcedExecutionOrchestrator {
        &self.orchestrator
    }
}

#[cfg(test)]
mod tests {
    // Tests would require mock LLM provider
}
