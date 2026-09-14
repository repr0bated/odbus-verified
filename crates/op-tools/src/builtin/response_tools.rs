//! Response Tools - Force LLM to use tools for all responses
//!
//! These tools ensure the LLM cannot hallucinate by requiring all
//! communication to go through verifiable tool calls.
//!
//! ## How It Works
//!
//! When tool_choice is set to "required", the LLM MUST call a tool.
//! To communicate with the user, it must call `respond_to_user`.
//! This allows us to:
//! 1. Verify that claimed actions actually happened
//! 2. Track what the LLM is telling the user
//! 3. Reject hallucinated responses

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use simd_json::prelude::*;
use simd_json::{json, OwnedValue as Value};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;

use crate::tool::{BoxedTool, Tool};

// ============================================================================
// RESPONSE ACCUMULATOR
// ============================================================================

/// A single response from the LLM
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmResponse {
    pub message: String,
    pub message_type: String,
    pub related_tool_calls: Vec<String>,
    pub data: Option<Value>,
}

/// Accumulates responses from respond_to_user tool calls
#[derive(Debug, Default)]
pub struct ResponseAccumulator {
    responses: Vec<LlmResponse>,
}

impl ResponseAccumulator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&mut self, response: LlmResponse) {
        self.responses.push(response);
    }

    pub fn clear(&mut self) {
        self.responses.clear();
    }

    pub fn get_responses(&self) -> &[LlmResponse] {
        &self.responses
    }

    /// Convert all responses to a single user message
    pub fn to_user_message(&self) -> String {
        self.responses
            .iter()
            .map(|r| r.message.clone())
            .collect::<Vec<_>>()
            .join("\n\n")
    }
}

// Global response accumulator (initialized eagerly)
static RESPONSE_ACCUMULATOR: std::sync::OnceLock<Arc<RwLock<ResponseAccumulator>>> =
    std::sync::OnceLock::new();

/// Initialize the global response accumulator (call once at startup)
pub fn init_response_accumulator() {
    let _ = RESPONSE_ACCUMULATOR.set(Arc::new(RwLock::new(ResponseAccumulator::new())));
}

/// Get the global response accumulator
#[cfg(test)]
pub fn get_response_accumulator() -> Arc<RwLock<ResponseAccumulator>> {
    RESPONSE_ACCUMULATOR
        .get_or_init(|| Arc::new(RwLock::new(ResponseAccumulator::new())))
        .clone()
}

/// Get the global response accumulator
#[cfg(not(test))]
pub fn get_response_accumulator() -> Arc<RwLock<ResponseAccumulator>> {
    RESPONSE_ACCUMULATOR
        .get()
        .expect("Response accumulator not initialized")
        .clone()
}

// ============================================================================
// RESPOND TO USER TOOL
// ============================================================================

/// Tool: Respond to User
///
/// ALL LLM responses to the user MUST go through this tool.
/// This allows verification that claimed actions were actually performed.
pub struct RespondToUserTool;

impl RespondToUserTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for RespondToUserTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for RespondToUserTool {
    fn name(&self) -> &str {
        "respond_to_user"
    }

    fn description(&self) -> &str {
        "Send a response to the user. ALL responses MUST use this tool. \
         Include related_actions to declare which tools were used - \
         this will be verified against actual tool executions. \
         NEVER output text directly - always use this tool."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "message": {
                    "type": "string",
                    "description": "The message to send to the user"
                },
                "message_type": {
                    "type": "string",
                    "enum": ["success", "info", "warning", "error"],
                    "description": "Type of message",
                    "default": "info"
                },
                "related_actions": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "List of tool names that were called to produce this response. Will be verified against actual executions."
                },
                "data": {
                    "type": "object",
                    "description": "Optional structured data to include with response"
                }
            },
            "required": ["message"]
        })
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let message = input
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("No message provided")
            .to_string();

        let message_type = input
            .get("message_type")
            .and_then(|v| v.as_str())
            .unwrap_or("info")
            .to_string();

        let related_actions: Vec<String> = input
            .get("related_actions")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str())
                    .map(|s| s.to_string())
                    .collect()
            })
            .unwrap_or_default();

        let data = input.get("data").cloned();

        info!(
            message_type = %message_type,
            related_actions = ?related_actions,
            "User response generated via respond_to_user tool"
        );

        // Add to accumulator
        let response = LlmResponse {
            message: message.clone(),
            message_type: message_type.clone(),
            related_tool_calls: related_actions.clone(),
            data: data.clone(),
        };

        {
            let accumulator_arc = get_response_accumulator();
            let mut accumulator = accumulator_arc.write().await;
            accumulator.add(response);
        }

        Ok(json!({
            "tool": "respond_to_user",
            "message": message,
            "message_type": message_type,
            "related_actions": related_actions,
            "data": data,
            "_internal": {
                "is_response_tool": true,
                "requires_verification": !related_actions.is_empty()
            }
        }))
    }

    fn category(&self) -> &str {
        "response"
    }

    fn namespace(&self) -> &str {
        "chat"
    }

    fn tags(&self) -> Vec<String> {
        vec![
            "response".to_string(),
            "user".to_string(),
            "required".to_string(),
        ]
    }
}

// ============================================================================
// CANNOT PERFORM TOOL
// ============================================================================

/// Tool: Cannot Perform
///
/// Use when the LLM cannot or should not perform a requested action.
pub struct CannotPerformTool;

impl CannotPerformTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for CannotPerformTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for CannotPerformTool {
    fn name(&self) -> &str {
        "cannot_perform"
    }

    fn description(&self) -> &str {
        "Decline to perform a requested action. Use when: \
         1) Action would be dangerous or destructive \
         2) Action is outside allowed capabilities \
         3) Action requires information not available \
         4) Action violates system policy"
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "reason": {
                    "type": "string",
                    "description": "Why the action cannot be performed"
                },
                "category": {
                    "type": "string",
                    "enum": ["dangerous", "not_allowed", "missing_info", "policy_violation", "not_supported"],
                    "description": "Category of refusal"
                },
                "alternatives": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Alternative actions the user could take"
                }
            },
            "required": ["reason", "category"]
        })
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let reason = input
            .get("reason")
            .and_then(|v| v.as_str())
            .unwrap_or("Action cannot be performed")
            .to_string();

        let category = input
            .get("category")
            .and_then(|v| v.as_str())
            .unwrap_or("not_allowed")
            .to_string();

        let alternatives: Vec<String> = input
            .get("alternatives")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str())
                    .map(|s| s.to_string())
                    .collect()
            })
            .unwrap_or_default();

        info!(
            category = %category,
            reason = %reason,
            "Action declined via cannot_perform tool"
        );

        // Add to accumulator as a response
        let message = format!("Cannot perform action: {} ({})", reason, category);
        let response = LlmResponse {
            message: message.clone(),
            message_type: "error".to_string(),
            related_tool_calls: vec![],
            data: None,
        };

        {
            let accumulator_arc = get_response_accumulator();
            let mut accumulator = accumulator_arc.write().await;
            accumulator.add(response);
        }

        Ok(json!({
            "tool": "cannot_perform",
            "declined": true,
            "reason": reason,
            "category": category,
            "alternatives": alternatives,
            "_internal": {
                "is_response_tool": true,
                "requires_verification": false
            }
        }))
    }

    fn category(&self) -> &str {
        "response"
    }

    fn namespace(&self) -> &str {
        "chat"
    }

    fn tags(&self) -> Vec<String> {
        vec![
            "response".to_string(),
            "decline".to_string(),
            "safety".to_string(),
        ]
    }
}

// ============================================================================
// REQUEST CLARIFICATION TOOL
// ============================================================================

/// Tool: Request Clarification
///
/// Use when more information is needed from the user.
pub struct RequestClarificationTool;

impl RequestClarificationTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for RequestClarificationTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for RequestClarificationTool {
    fn name(&self) -> &str {
        "request_clarification"
    }

    fn description(&self) -> &str {
        "Request additional information from the user before proceeding. \
         Use when the request is ambiguous or missing required details."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "question": {
                    "type": "string",
                    "description": "The clarifying question to ask"
                },
                "context": {
                    "type": "string",
                    "description": "Why this clarification is needed"
                },
                "options": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Possible options for the user to choose from"
                },
                "required_fields": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "List of fields/information that is missing"
                }
            },
            "required": ["question"]
        })
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let question = input
            .get("question")
            .and_then(|v| v.as_str())
            .unwrap_or("Could you please provide more details?")
            .to_string();

        let context = input
            .get("context")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let options: Option<Vec<String>> =
            input.get("options").and_then(|v| v.as_array()).map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str())
                    .map(|s| s.to_string())
                    .collect()
            });

        let required_fields: Option<Vec<String>> = input
            .get("required_fields")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str())
                    .map(|s| s.to_string())
                    .collect()
            });

        info!(question = %question, "Requesting clarification via tool");

        // Add to accumulator
        let response = LlmResponse {
            message: question.clone(),
            message_type: "info".to_string(),
            related_tool_calls: vec![],
            data: None,
        };

        {
            let accumulator_arc = get_response_accumulator();
            let mut accumulator = accumulator_arc.write().await;
            accumulator.add(response);
        }

        Ok(json!({
            "tool": "request_clarification",
            "question": question,
            "context": context,
            "options": options,
            "required_fields": required_fields,
            "_internal": {
                "is_response_tool": true,
                "requires_verification": false,
                "awaiting_input": true
            }
        }))
    }

    fn category(&self) -> &str {
        "response"
    }

    fn namespace(&self) -> &str {
        "chat"
    }

    fn tags(&self) -> Vec<String> {
        vec![
            "response".to_string(),
            "clarification".to_string(),
            "input".to_string(),
        ]
    }
}

// ============================================================================
// TOOL CREATION
// ============================================================================

/// Create all response tools
pub fn create_response_tools() -> Vec<BoxedTool> {
    vec![
        Arc::new(RespondToUserTool::new()),
        Arc::new(CannotPerformTool::new()),
        Arc::new(RequestClarificationTool::new()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_respond_to_user() {
        init_response_accumulator();
        let tool = RespondToUserTool::new();
        let result = tool
            .execute(json!({
                "message": "Bridge created successfully",
                "message_type": "success",
                "related_actions": ["ovs_create_bridge"]
            }))
            .await
            .unwrap();

        assert_eq!(
            result.get("message").unwrap(),
            "Bridge created successfully"
        );
        assert!(result
            .get("_internal")
            .unwrap()
            .get("is_response_tool")
            .unwrap()
            .as_bool()
            .unwrap());

        // Check accumulator
        let acc_arc = get_response_accumulator();
        let acc = acc_arc.read().await;
        let found = acc
            .get_responses()
            .iter()
            .any(|resp| resp.message == "Bridge created successfully");
        assert!(found);
    }

    #[tokio::test]
    async fn test_cannot_perform() {
        let tool = CannotPerformTool::new();
        let result = tool
            .execute(json!({
                "reason": "Would delete all network interfaces",
                "category": "dangerous",
                "alternatives": ["Delete specific interface", "Disable interface"]
            }))
            .await
            .unwrap();

        assert!(result.get("declined").unwrap().as_bool().unwrap());
    }
}
