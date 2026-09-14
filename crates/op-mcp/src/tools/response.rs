//! Response Tools - Communication with User

use crate::tool_registry::{BoxedTool, Tool, ToolRegistry};
use anyhow::Result;
use async_trait::async_trait;
use simd_json::{json, OwnedValue as Value};
use std::sync::Arc;

pub async fn register_all(registry: &ToolRegistry) -> Result<usize> {
    registry.register(Arc::new(RespondToUserTool)).await?;
    registry.register(Arc::new(CannotPerformTool)).await?;
    registry.register(Arc::new(RequestClarificationTool)).await?;
    Ok(3)
}

pub struct RespondToUserTool;

#[async_trait]
impl Tool for RespondToUserTool {
    fn name(&self) -> &str { "respond_to_user" }
    fn description(&self) -> &str { "Send a response message to the user." }
    fn category(&self) -> &str { "response" }
    fn tags(&self) -> Vec<String> { vec!["response".into(), "essential".into()] }
    
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "message": {"type": "string", "description": "The message to send"}
            },
            "required": ["message"]
        })
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let message = input.get("message").and_then(|v| v.as_str()).unwrap_or("");
        Ok(json!({"success": true, "message": message, "delivered": true}))
    }
}

pub struct CannotPerformTool;

#[async_trait]
impl Tool for CannotPerformTool {
    fn name(&self) -> &str { "cannot_perform" }
    fn description(&self) -> &str { "Indicate that a requested action cannot be performed." }
    fn category(&self) -> &str { "response" }
    fn tags(&self) -> Vec<String> { vec!["response".into(), "error".into()] }
    
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "reason": {"type": "string", "description": "Why the action cannot be performed"}
            },
            "required": ["reason"]
        })
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let reason = input.get("reason").and_then(|v| v.as_str()).unwrap_or("Unknown");
        Ok(json!({"success": true, "cannot_perform": true, "reason": reason}))
    }
}

pub struct RequestClarificationTool;

#[async_trait]
impl Tool for RequestClarificationTool {
    fn name(&self) -> &str { "request_clarification" }
    fn description(&self) -> &str { "Ask the user for clarification." }
    fn category(&self) -> &str { "response" }
    fn tags(&self) -> Vec<String> { vec!["response".into(), "clarification".into()] }
    
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "question": {"type": "string", "description": "The clarification question"}
            },
            "required": ["question"]
        })
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let question = input.get("question").and_then(|v| v.as_str()).unwrap_or("");
        Ok(json!({"success": true, "needs_clarification": true, "question": question}))
    }
}
