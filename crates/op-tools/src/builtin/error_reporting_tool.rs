//! Tool for reporting internal errors

use crate::Tool;
use async_trait::async_trait;
use simd_json::{json, OwnedValue as Value};
use anyhow::Result;

/// Tool to report an internal error to the user
pub struct ReportInternalErrorTool;

#[async_trait]
impl Tool for ReportInternalErrorTool {
    fn name(&self) -> &str {
        "report_internal_error"
    }

    fn description(&self) -> &str {
        "Report an internal error or unexpected state to the user. Use this when a request cannot be fulfilled due to an internal failure."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "error_message": {
                    "type": "string",
                    "description": "A detailed description of the internal error."
                },
                "failed_action": {
                    "type": "string",
                    "description": "The action that failed (e.g., the tool I was trying to use)."
                }
            },
            "required": ["error_message", "failed_action"]
        })
    }

    fn category(&self) -> &str {
        "chat"
    }

    fn tags(&self) -> Vec<String> {
        vec!["error".to_string(), "internal".to_string(), "meta".to_string()]
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let error_message = input.get("error_message").and_then(|v| v.as_str()).unwrap_or("Unknown error");
        let failed_action = input.get("failed_action").and_then(|v| v.as_str()).unwrap_or("Unknown action");

        // This tool doesn't actually *do* anything other than provide a structured way
        // for me to report that I've had an internal error. The chat orchestrator
        // will see that this tool was called and can then formulate a user-friendly
        // error message.

        Ok(json!({
            "success": true,
            "message": "Internal error reported.",
            "reported_error": {
                "failed_action": failed_action,
                "error_message": error_message
            }
        }))
    }
}
