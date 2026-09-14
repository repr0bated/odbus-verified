//! Response Tool - Forces LLM to use tools for ALL interactions
//!
//! This tool makes responding to users an explicit action, preventing hallucination
//! by ensuring every LLM output goes through the tool execution pipeline.

use async_trait::async_trait;
use simd_json::json;

use crate::Tool;
use op_core::{ToolDefinition, ToolRequest, ToolResult};

/// Tool for the LLM to respond to users
///
/// By making "respond" a tool, we force the LLM to go through tool execution
/// for EVERY interaction - no more hallucinated claims without tool calls.
pub struct RespondToUserTool;

#[async_trait]
impl Tool for RespondToUserTool {
    fn name(&self) -> &str {
        "respond_to_user"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "respond_to_user".to_string(),
            description: "Send a response message to the user. Use this tool when you want to communicate information, ask questions, or provide explanations. DO NOT claim to have performed actions - use action tools first, then respond with their results.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "message": {
                        "type": "string",
                        "description": "The message to send to the user. Be clear and helpful."
                    },
                    "message_type": {
                        "type": "string",
                        "enum": ["info", "question", "explanation", "error", "success"],
                        "description": "Type of message: info (general information), question (asking user), explanation (detailed explanation), error (reporting a problem), success (confirming completed action)"
                    }
                },
                "required": ["message", "message_type"]
            }),
            category: Some("communication".to_string()),
            tags: vec!["response".to_string(), "communication".to_string(), "required".to_string()],
        }
    }

    async fn execute(&self, request: ToolRequest) -> ToolResult {
        let message = request
            .arguments
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("No message provided");

        let message_type = request
            .arguments
            .get("message_type")
            .and_then(|v| v.as_str())
            .unwrap_or("info");

        // Just return the message - the orchestrator will handle displaying it
        ToolResult::success(
            request.id,
            json!({
                "message": message,
                "message_type": message_type,
                "delivered": true
            }),
            1, // 1ms execution time
        )
    }
}

/// Tool for reporting that an action cannot be performed
pub struct CannotPerformTool;

#[async_trait]
impl Tool for CannotPerformTool {
    fn name(&self) -> &str {
        "cannot_perform"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "cannot_perform".to_string(),
            description: "Use this tool when you cannot perform a requested action. Explain why and suggest alternatives. NEVER claim you performed an action that you didn't do.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "reason": {
                        "type": "string",
                        "description": "Why you cannot perform the action"
                    },
                    "alternatives": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": "Alternative actions or suggestions"
                    }
                },
                "required": ["reason"]
            }),
            category: Some("communication".to_string()),
            tags: vec!["response".to_string(), "error".to_string()],
        }
    }

    async fn execute(&self, request: ToolRequest) -> ToolResult {
        let reason = request
            .arguments
            .get("reason")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown reason");

        let alternatives = request
            .arguments
            .get("alternatives")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str())
                    .map(String::from)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        ToolResult::success(
            request.id,
            json!({
                "status": "cannot_perform",
                "reason": reason,
                "alternatives": alternatives
            }),
            1,
        )
    }
}

/// Create response-related tools
pub fn create_response_tools() -> Vec<Box<dyn Tool>> {
    vec![Box::new(RespondToUserTool), Box::new(CannotPerformTool)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_respond_tool() {
        let tool = RespondToUserTool;
        let request = ToolRequest {
            id: "test-1".to_string(),
            tool_name: "respond_to_user".to_string(),
            arguments: json!({
                "message": "Hello, this is a test response",
                "message_type": "info"
            }),
            timeout_ms: None,
        };

        let result = tool.execute(request).await;
        assert!(result.success);
        assert!(result.content.get("message").is_some());
    }

    #[tokio::test]
    async fn test_cannot_perform_tool() {
        let tool = CannotPerformTool;
        let request = ToolRequest {
            id: "test-2".to_string(),
            tool_name: "cannot_perform".to_string(),
            arguments: json!({
                "reason": "Service not available",
                "alternatives": ["Try later", "Check connection"]
            }),
            timeout_ms: None,
        };

        let result = tool.execute(request).await;
        assert!(result.success);
        assert_eq!(result.content.get("status").unwrap(), "cannot_perform");
    }
}
