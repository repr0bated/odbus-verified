//! Chat Loop with Forced Tool Execution
//!
//! This module implements the main chat loop that:
//! 1. Forces all LLM responses through tools (anti-hallucination)
//! 2. Verifies tool execution before accepting claims
//! 3. Accumulates responses via respond_to_user tool

use anyhow::Result;
use op_llm::{
    ChatMessage, ChatRequest, ChatResponse, LlmProvider, ToolChoice, ToolDefinition,
};
use simd_json::{json, OwnedValue as Value};
use std::sync::Arc;
use tracing::{debug, error, info, warn};

use crate::forced_execution::{
    ForcedExecutionOrchestrator, HallucinationCheck, ToolCall, parse_tool_calls,
    detect_raw_text_output,
};
use crate::tool_executor::TrackedToolExecutor;

/// Configuration for the chat loop
#[derive(Debug, Clone)]
pub struct ChatLoopConfig {
    /// Maximum tool calls per turn (prevent infinite loops)
    pub max_tool_calls_per_turn: usize,
    /// Whether to force tool usage (anti-hallucination)
    pub force_tool_use: bool,
    /// Whether to verify claims against executions
    pub verify_claims: bool,
    /// Model to use
    pub model: String,
}

impl Default for ChatLoopConfig {
    fn default() -> Self {
        Self {
            max_tool_calls_per_turn: 10,
            force_tool_use: true,
            verify_claims: true,
            model: "deepseek-ai/DeepSeek-V2.5".to_string(),
        }
    }
}

/// Chat loop that forces tool execution
pub struct ForcedToolChatLoop<P: LlmProvider> {
    provider: Arc<P>,
    orchestrator: Arc<ForcedExecutionOrchestrator>,
    config: ChatLoopConfig,
    /// Available tools (including response tools)
    tools: Vec<ToolDefinition>,
    /// Conversation history
    messages: Vec<ChatMessage>,
}

impl<P: LlmProvider> ForcedToolChatLoop<P> {
    pub fn new(
        provider: Arc<P>,
        executor: Arc<TrackedToolExecutor>,
        config: ChatLoopConfig,
    ) -> Self {
        let orchestrator = Arc::new(ForcedExecutionOrchestrator::new(executor));
        
        // Build tool definitions including response tools
        let tools = Self::build_tool_definitions();

        Self {
            provider,
            orchestrator,
            config,
            tools,
            messages: Vec::new(),
        }
    }

    /// Build tool definitions including mandatory response tools
    fn build_tool_definitions() -> Vec<ToolDefinition> {
        vec![
            // Response tools (REQUIRED for all communication)
            ToolDefinition {
                name: "respond_to_user".to_string(),
                description: "Send a message to the user. This is the ONLY way to communicate. \
                              You MUST call this tool to respond. After performing actions, \
                              use this to explain what was done.".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "message": {
                            "type": "string",
                            "description": "The message to send to the user"
                        },
                        "message_type": {
                            "type": "string",
                            "enum": ["info", "success", "warning", "error", "question"],
                            "default": "info"
                        },
                        "related_actions": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "Tool names that were called before this response"
                        }
                    },
                    "required": ["message"]
                }),
            },
            ToolDefinition {
                name: "cannot_perform".to_string(),
                description: "Use when you cannot perform a requested action. \
                              Explain why and suggest alternatives.".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "reason": {
                            "type": "string",
                            "description": "Why the action cannot be performed"
                        },
                        "attempted_action": {
                            "type": "string",
                            "description": "What action was requested"
                        },
                        "alternatives": {
                            "type": "array",
                            "items": { "type": "string" }
                        }
                    },
                    "required": ["reason", "attempted_action"]
                }),
            },
            ToolDefinition {
                name: "request_clarification".to_string(),
                description: "Ask the user for clarification when the request is ambiguous.".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "question": {
                            "type": "string",
                            "description": "The clarifying question"
                        },
                        "options": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "Possible options for the user"
                        }
                    },
                    "required": ["question"]
                }),
            },
            // OVS Tools
            ToolDefinition {
                name: "ovs_create_bridge".to_string(),
                description: "Create a new OVS bridge".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "name": {
                            "type": "string",
                            "description": "Name of the bridge to create"
                        }
                    },
                    "required": ["name"]
                }),
            },
            ToolDefinition {
                name: "ovs_list_bridges".to_string(),
                description: "List all OVS bridges".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {}
                }),
            },
            // NOTE: shell_execute is intentionally NOT included
            // All operations must use native protocol tools (D-Bus, OVSDB JSON-RPC, rtnetlink)
        ]
    }

    /// Validate response for forbidden CLI commands
    /// Returns sanitized response or error if critical violation detected
    fn validate_response_for_cli_commands(response: &str) -> Result<String, String> {
        const FORBIDDEN_CLI_PATTERNS: &[&str] = &[
            "ovs-vsctl",
            "ovs-ofctl",
            "ovs-dpctl",
            "ovs-appctl",
            "ovsdb-client",
            "systemctl ",
            "service ",
            "ip link",
            "ip addr",
            "ip route",
            "ifconfig",
            "nmcli",
            "brctl",
            "apt install",
            "apt update",
            "yum install",
            "dnf install",
            "sudo apt",
            "sudo yum",
            "sudo dnf",
        ];

        let lower = response.to_lowercase();
        let mut found_violations: Vec<&str> = Vec::new();

        for pattern in FORBIDDEN_CLI_PATTERNS {
            if lower.contains(&pattern.to_lowercase()) {
                found_violations.push(pattern);
            }
        }

        if !found_violations.is_empty() {
            warn!(
                "Response contains forbidden CLI commands: {:?}",
                found_violations
            );

            // Return sanitized response with warning
            let warning = format!(
                "⚠️ **CLI Commands Not Allowed**\n\n\
                The response contained forbidden CLI commands ({}).\n\n\
                This system uses native protocols only:\n\
                - OVS: Use `ovs_*` tools (OVSDB JSON-RPC)\n\
                - Systemd: Use D-Bus systemd1 interface\n\
                - Network: Use rtnetlink tools\n\n\
                Please use the appropriate native tools instead.",
                found_violations.join(", ")
            );

            Err(warning)
        } else {
            Ok(response.to_string())
        }
    }

    /// Add a system message
    pub fn add_system_message(&mut self, content: impl Into<String>) {
        self.messages.push(ChatMessage::system(content));
    }

    /// Process a user message and return the response
    pub async fn process_message(&mut self, user_message: &str) -> Result<ChatTurnResult> {
        info!("Processing user message: {}", user_message);

        // Start a new turn
        self.orchestrator.start_turn().await;

        // Add user message to history
        self.messages.push(ChatMessage::user(user_message));

        // Build the chat request with forced tool use
        let mut request = ChatRequest::new(self.messages.clone())
            .with_tools(self.tools.clone());

        if self.config.force_tool_use {
            request = request.force_tool_use();
        }

        // Send to LLM
        let response = self.provider
            .chat_with_request(&self.config.model, request)
            .await?;

        // Check for raw text output (hallucination indicator)
        if self.config.force_tool_use && response.is_raw_text() {
            warn!("LLM returned raw text instead of tool calls - potential hallucination");
            return Ok(ChatTurnResult {
                user_response: "⚠️ The AI attempted to respond without using tools. \
                               This is not allowed to prevent hallucinations.".to_string(),
                verification: HallucinationCheck {
                    verified: false,
                    issues: vec![crate::forced_execution::HallucinationIssue {
                        issue_type: crate::forced_execution::HallucinationType::RawTextOutput,
                        description: "LLM output raw text instead of using respond_to_user tool".to_string(),
                        severity: crate::forced_execution::IssueSeverity::Critical,
                    }],
                    executed_tools: vec![],
                    unverified_claims: vec![],
                },
                tool_calls_made: 0,
            });
        }

        // Execute tool calls
        let mut tool_calls_made = 0;
        let tool_calls = response.get_tool_calls();

        for tool_call in tool_calls.iter().take(self.config.max_tool_calls_per_turn) {
            info!("Executing tool: {} with args: {:?}", tool_call.name, tool_call.arguments);
            
            let result = self.orchestrator
                .execute_tool(&tool_call.name, tool_call.arguments.clone(), None)
                .await;

            match result {
                Ok(tool_result) => {
                    debug!("Tool {} succeeded: {:?}", tool_call.name, tool_result);
                    
                    // Add tool result to conversation
                    self.messages.push(ChatMessage::tool_result(
                        &tool_call.id,
                        simd_json::to_string(&tool_result).unwrap_or_default(),
                    ));
                }
                Err(e) => {
                    error!("Tool {} failed: {}", tool_call.name, e);
                    
                    // Add error result to conversation
                    self.messages.push(ChatMessage::tool_result(
                        &tool_call.id,
                        json!({ "error": e.to_string() }).to_string(),
                    ));
                }
            }

            tool_calls_made += 1;
        }

        // Verify the turn for hallucinations
        let verification = if self.config.verify_claims {
            self.orchestrator.verify_turn().await
        } else {
            HallucinationCheck {
                verified: true,
                issues: vec![],
                executed_tools: vec![],
                unverified_claims: vec![],
            }
        };

        // Get the accumulated user response
        let raw_response = self.orchestrator.get_user_response().await;

        // Validate response for forbidden CLI commands
        let user_response = match Self::validate_response_for_cli_commands(&raw_response) {
            Ok(validated) => validated,
            Err(warning) => {
                // Response contained forbidden CLI commands - return warning instead
                warn!("Blocked response containing CLI commands");
                warning
            }
        };

        // Add assistant message to history (with tool calls)
        self.messages.push(response.message.clone());

        Ok(ChatTurnResult {
            user_response,
            verification,
            tool_calls_made,
        })
    }

    /// Get conversation history
    pub fn history(&self) -> &[ChatMessage] {
        &self.messages
    }

    /// Clear conversation history
    pub fn clear_history(&mut self) {
        self.messages.clear();
    }
}

/// Result of a single chat turn
#[derive(Debug)]
pub struct ChatTurnResult {
    /// The response to show the user (accumulated from respond_to_user calls)
    pub user_response: String,
    /// Hallucination verification result
    pub verification: HallucinationCheck,
    /// Number of tool calls made
    pub tool_calls_made: usize,
}

impl ChatTurnResult {
    /// Check if the turn was verified (no hallucinations)
    pub fn is_verified(&self) -> bool {
        self.verification.verified
    }

    /// Get any issues found
    pub fn issues(&self) -> &[crate::forced_execution::HallucinationIssue] {
        &self.verification.issues
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_definitions() {
        let tools = ForcedToolChatLoop::<()>::build_tool_definitions();
        
        // Must have response tools
        assert!(tools.iter().any(|t| t.name == "respond_to_user"));
        assert!(tools.iter().any(|t| t.name == "cannot_perform"));
        assert!(tools.iter().any(|t| t.name == "request_clarification"));
    }
}
