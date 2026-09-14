//! Forced Tool Execution - Anti-Hallucination Architecture
//!
//! This module ensures the LLM CANNOT hallucinate by:
//! 1. Requiring all output to go through response tools
//! 2. Verifying claimed actions against execution log
//! 3. Detecting and flagging hallucination attempts
//!
//! ## How It Works
//!
//! ```text
//! User: "Create a bridge called br0"
//!         │
//!         ▼
//! ┌───────────────────────────────────────────────────────────┐
//! │ LLM Response (MUST contain tool_calls)                    │
//! │                                                           │
//! │ tool_calls: [                                             │
//! │   { name: "ovs_create_bridge", args: {name: "br0"} },    │
//! │   { name: "respond_to_user", args: {                      │
//! │       message: "Created bridge br0",                      │
//! │       message_type: "success",                            │
//! │       related_actions: ["ovs_create_bridge"]              │
//! │   }}                                                      │
//! │ ]                                                         │
//! └───────────────────────────────────────────────────────────┘
//!         │
//!         ▼
//! ┌───────────────────────────────────────────────────────────┐
//! │ Execution Verifier                                        │
//! │                                                           │
//! │ ✓ ovs_create_bridge was called                            │
//! │ ✓ respond_to_user references ovs_create_bridge            │
//! │ ✓ No raw text output (all via respond_to_user)            │
//! │                                                           │
//! │ Result: VERIFIED - No hallucination                       │
//! └───────────────────────────────────────────────────────────┘
//! ```

use anyhow::Result;
// use op_core::ExecutionTracker;
use serde::{Deserialize, Serialize};
use simd_json::prelude::*;
use simd_json::OwnedValue;
use simd_json::OwnedValue as Value;
use std::collections::HashSet;
use std::sync::Arc;
use tracing::error;

use crate::tool_executor::TrackedToolExecutor;
use op_tools::builtin::response_tools::get_response_accumulator;

/// Result of hallucination detection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HallucinationCheck {
    /// Whether the response passed verification
    pub verified: bool,
    /// Detected hallucination issues
    pub issues: Vec<HallucinationIssue>,
    /// Tools that were actually executed
    pub executed_tools: Vec<String>,
    /// Tools claimed in response but not executed
    pub unverified_claims: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HallucinationIssue {
    pub issue_type: HallucinationType,
    pub description: String,
    pub severity: IssueSeverity,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum HallucinationType {
    /// LLM output raw text without using respond_to_user
    RawTextOutput,
    /// Claimed to perform action without calling tool
    UnverifiedActionClaim,
    /// respond_to_user called without any action tools
    ResponseWithoutAction,
    /// Tool call failed but success was claimed
    FailedToolClaimedSuccess,
    /// No respond_to_user tool called
    NoResponseTool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum IssueSeverity {
    /// Informational - not necessarily wrong
    Info,
    /// Warning - suspicious but might be valid
    Warning,
    /// Error - definite hallucination
    Error,
    /// Critical - severe hallucination, response should be rejected
    Critical,
}

/// Forced execution orchestrator
///
/// Ensures all LLM interactions go through tools and verifies
/// that claimed actions actually occurred.
pub struct ForcedExecutionOrchestrator {
    executor: Arc<TrackedToolExecutor>,
    /// Track which tools were called in current turn
    current_turn_tools: Arc<tokio::sync::RwLock<Vec<String>>>,
}

impl ForcedExecutionOrchestrator {
    pub fn new(executor: Arc<TrackedToolExecutor>) -> Self {
        Self {
            executor,
            current_turn_tools: Arc::new(tokio::sync::RwLock::new(Vec::new())),
        }
    }

    /// Start a new turn - clears tracking state
    pub async fn start_turn(&self) {
        let mut tools = self.current_turn_tools.write().await;
        tools.clear();

        // Also clear the response accumulator
        let accumulator = get_response_accumulator();
        accumulator.write().await.clear();
    }

    /// Execute a tool and track it
    pub async fn execute_tool(
        &self,
        tool_name: &str,
        arguments: Value,
        session_id: Option<String>,
    ) -> Result<Value> {
        // Track the tool call
        {
            let mut tools = self.current_turn_tools.write().await;
            tools.push(tool_name.to_string());
        }

        // Execute via tracked executor
        let result = self
            .executor
            .execute(tool_name, arguments, session_id)
            .await?;

        Ok(result.result.result.clone().unwrap_or_else(Value::null))
    }

    /// Execute a sequence of tool calls from LLM response
    pub async fn execute_tool_sequence(
        &self,
        tool_calls: Vec<ToolCall>,
        session_id: Option<String>,
    ) -> Result<SequenceExecutionResult> {
        let mut results = Vec::new();
        let mut all_succeeded = true;

        for call in tool_calls {
            let result = self
                .execute_tool(&call.name, call.arguments.clone(), session_id.clone())
                .await;

            let success = result.is_ok();
            if !success {
                all_succeeded = false;
            }

            results.push(ToolCallResult {
                tool_name: call.name,
                arguments: call.arguments,
                success,
                result: result.ok(),
                error: None,
            });
        }

        Ok(SequenceExecutionResult {
            all_succeeded,
            results,
        })
    }

    /// Verify the turn for hallucinations
    pub async fn verify_turn(&self) -> HallucinationCheck {
        let executed_tools = self.current_turn_tools.read().await.clone();
        let accumulator = get_response_accumulator();
        let responses = accumulator.read().await;

        let mut issues = Vec::new();
        let mut unverified_claims = Vec::new();

        // Check 1: Was respond_to_user called?
        let has_response_tool = executed_tools.iter().any(|t| {
            t == "respond_to_user" || t == "cannot_perform" || t == "request_clarification"
        });

        if !has_response_tool {
            issues.push(HallucinationIssue {
                issue_type: HallucinationType::NoResponseTool,
                description: "LLM did not use respond_to_user tool - output may be raw text"
                    .to_string(),
                severity: IssueSeverity::Critical,
            });
        }

        // Check 2: Do claimed actions match executed tools?
        let executed_set: HashSet<_> = executed_tools.iter().collect();

        for response in responses.get_responses() {
            for claimed_tool in &response.related_tool_calls {
                if !executed_set.contains(claimed_tool) {
                    unverified_claims.push(claimed_tool.clone());
                    issues.push(HallucinationIssue {
                        issue_type: HallucinationType::UnverifiedActionClaim,
                        description: format!(
                            "Response claims '{}' was called but no execution record found",
                            claimed_tool
                        ),
                        severity: IssueSeverity::Error,
                    });
                }
            }
        }

        // Check 3: Was there a response without any action?
        // This is only a warning - sometimes the user just asks a question
        let action_tools: Vec<_> = executed_tools
            .iter()
            .filter(|t| {
                !matches!(
                    t.as_str(),
                    "respond_to_user" | "cannot_perform" | "request_clarification"
                )
            })
            .collect();

        if action_tools.is_empty() && has_response_tool {
            // Check if this is a "cannot perform" or "clarification" - those are OK without actions
            let only_declined = executed_tools
                .iter()
                .all(|t| t == "cannot_perform" || t == "request_clarification");

            if !only_declined {
                issues.push(HallucinationIssue {
                    issue_type: HallucinationType::ResponseWithoutAction,
                    description:
                        "Response given without any action tools - verify this is intentional"
                            .to_string(),
                    severity: IssueSeverity::Info,
                });
            }
        }

        let verified = !issues.iter().any(|i| i.severity >= IssueSeverity::Error);

        if !verified {
            error!(
                issues = ?issues,
                executed_tools = ?executed_tools,
                "Hallucination detected in LLM response"
            );
        }

        HallucinationCheck {
            verified,
            issues,
            executed_tools,
            unverified_claims,
        }
    }

    /// Get the final user message from accumulated responses
    pub async fn get_user_response(&self) -> String {
        let accumulator = get_response_accumulator();
        let responses = accumulator.read().await;
        responses.to_user_message()
    }

    /// Get execution stats for current turn
    pub async fn get_turn_stats(&self) -> TurnStats {
        let tools = self.current_turn_tools.read().await;
        let accumulator = get_response_accumulator();
        let responses = accumulator.read().await;

        TurnStats {
            tools_executed: tools.len(),
            response_messages: responses.get_responses().len(),
            tool_names: tools.clone(),
        }
    }
}

/// Tool call from LLM
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub name: String,
    pub arguments: Value,
}

/// Result of a single tool call
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallResult {
    pub tool_name: String,
    pub arguments: Value,
    pub success: bool,
    pub result: Option<Value>,
    pub error: Option<String>,
}

/// Result of executing a sequence of tools
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SequenceExecutionResult {
    pub all_succeeded: bool,
    pub results: Vec<ToolCallResult>,
}

/// Statistics for current turn
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnStats {
    pub tools_executed: usize,
    pub response_messages: usize,
    pub tool_names: Vec<String>,
}

/// Parse tool calls from LLM response
///
/// Handles various formats:
/// - OpenAI function calling format
/// - Anthropic tool_use format
/// - Generic JSON format
pub fn parse_tool_calls(llm_response: &Value) -> Vec<ToolCall> {
    let mut calls = Vec::new();

    // Try OpenAI format
    if let Some(tool_calls) = llm_response.get("tool_calls").and_then(|v| v.as_array()) {
        for call in tool_calls {
            if let (Some(name), Some(args)) = (
                call.get("function")
                    .and_then(|f| f.get("name"))
                    .and_then(|n| n.as_str()),
                call.get("function").and_then(|f| f.get("arguments")),
            ) {
                let arguments = if let Some(args_str) = args.as_str() {
                    serde_json::from_str(args_str).unwrap_or_else(|_| Value::null())
                } else {
                    args.clone()
                };

                calls.push(ToolCall {
                    name: name.to_string(),
                    arguments,
                });
            }
        }
    }

    // Try Anthropic format
    if let Some(content) = llm_response.get("content").and_then(|v| v.as_array()) {
        for block in content {
            if block.get("type").and_then(|t| t.as_str()) == Some("tool_use") {
                if let (Some(name), Some(input)) = (
                    block.get("name").and_then(|n: &OwnedValue| n.as_str()),
                    block.get("input"),
                ) {
                    calls.push(ToolCall {
                        name: name.to_string(),
                        arguments: input.clone(),
                    });
                }
            }
        }
    }

    // Try generic format
    if let Some(tools) = llm_response
        .get("tools")
        .and_then(|v: &OwnedValue| v.as_array())
    {
        for tool in tools {
            if let (Some(name), arguments) = (
                tool.get("name").and_then(|n: &OwnedValue| n.as_str()),
                tool.get("arguments").cloned().unwrap_or_else(Value::null),
            ) {
                calls.push(ToolCall {
                    name: name.to_string(),
                    arguments,
                });
            }
        }
    }

    calls
}

/// Check if LLM response contains raw text that should have gone through respond_to_user
pub fn detect_raw_text_output(llm_response: &Value) -> Option<String> {
    // Check for content that looks like direct user communication
    if let Some(content) = llm_response.get("content") {
        if let Some(text) = content.as_str() {
            // If there's text content but no tool calls, this is raw output
            let has_tool_calls = llm_response.get("tool_calls").is_some()
                || llm_response
                    .get("content")
                    .and_then(|c: &OwnedValue| c.as_array())
                    .map(|arr: &Vec<OwnedValue>| {
                        arr.iter().any(|b: &OwnedValue| {
                            b.get("type").and_then(|t: &OwnedValue| t.as_str()) == Some("tool_use")
                        })
                    })
                    .unwrap_or(false);

            if !has_tool_calls && !text.is_empty() {
                return Some(text.to_string());
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use simd_json::json;

    #[test]
    fn test_parse_openai_tool_calls() {
        let response = json!({
            "tool_calls": [{
                "id": "call_123",
                "type": "function",
                "function": {
                    "name": "ovs_create_bridge",
                    "arguments": "{\"name\": \"br0\"}"
                }
            }]
        });

        let calls = parse_tool_calls(&response);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "ovs_create_bridge");
    }

    #[test]
    fn test_parse_anthropic_tool_calls() {
        let response = json!({
            "content": [{
                "type": "tool_use",
                "id": "toolu_123",
                "name": "respond_to_user",
                "input": {
                    "message": "Hello!",
                    "message_type": "info"
                }
            }]
        });

        let calls = parse_tool_calls(&response);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "respond_to_user");
    }

    #[test]
    fn test_detect_raw_text() {
        // Raw text without tool calls - should be detected
        let raw = json!({
            "content": "I created the bridge for you!"
        });
        assert!(detect_raw_text_output(&raw).is_some());

        // With tool calls - should not be detected as raw
        let with_tools = json!({
            "tool_calls": [{
                "function": {
                    "name": "respond_to_user",
                    "arguments": {}
                }
            }]
        });
        assert!(detect_raw_text_output(&with_tools).is_none());
    }
}
