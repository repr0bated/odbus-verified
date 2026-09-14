//! JSON-RPC 2.0 protocol types

use serde::{Deserialize, Serialize};
use simd_json::prelude::*;
use simd_json::OwnedValue as Value;

/// JSON-RPC 2.0 request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub method: String,
    #[serde(default)]
    pub params: Value,
    pub id: Value,
}

impl JsonRpcRequest {
    /// Create a new JSON-RPC request
    pub fn new(method: impl Into<String>, params: Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            method: method.into(),
            params,
            id: Value::from(0),
        }
    }

    /// Create with a specific ID
    pub fn with_id(method: impl Into<String>, params: Value, id: Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            method: method.into(),
            params,
            id,
        }
    }
}

/// JSON-RPC 2.0 response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
    pub id: Value,
}

impl JsonRpcResponse {
    /// Create a success response
    pub fn success(id: Value, result: Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            result: Some(result),
            error: None,
            id,
        }
    }

    /// Create an error response
    pub fn error(id: Value, code: i32, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            result: None,
            error: Some(JsonRpcError {
                code,
                message: message.into(),
                data: None,
            }),
            id,
        }
    }

    /// Create an error response with data
    pub fn error_with_data(id: Value, code: i32, message: impl Into<String>, data: Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            result: None,
            error: Some(JsonRpcError {
                code,
                message: message.into(),
                data: Some(data),
            }),
            id,
        }
    }
}

/// JSON-RPC 2.0 error object
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

/// Standard JSON-RPC error codes
pub mod error_codes {
    pub const PARSE_ERROR: i32 = -32700;
    pub const INVALID_REQUEST: i32 = -32600;
    pub const METHOD_NOT_FOUND: i32 = -32601;
    pub const INVALID_PARAMS: i32 = -32602;
    pub const INTERNAL_ERROR: i32 = -32603;

    // Custom error codes (application-specific)
    pub const DATABASE_ERROR: i32 = -32000;
    pub const CONNECTION_ERROR: i32 = -32001;
    pub const NOT_FOUND: i32 = -32002;
    pub const PERMISSION_DENIED: i32 = -32003;
}

/// Parse a JSON-RPC request from a JSON value
#[allow(clippy::result_large_err)]
pub fn parse_request(value: Value) -> Result<JsonRpcRequest, JsonRpcResponse> {
    simd_json::serde::from_owned_value(value.clone()).map_err(|e| {
        JsonRpcResponse::error(
            value.get("id").cloned().unwrap_or(Value::null()),
            error_codes::INVALID_REQUEST,
            format!("Invalid request: {}", e),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_serialization() {
        let req = JsonRpcRequest::new("test", simd_json::json!(["arg1", "arg2"]));
        let json = simd_json::to_string(&req).unwrap();
        assert!(json.contains("\"method\":\"test\""));
    }

    #[test]
    fn test_response_serialization() {
        let resp = JsonRpcResponse::success(Value::from(1), simd_json::json!({"ok": true}));
        let json = simd_json::to_string(&resp).unwrap();
        assert!(json.contains("\"result\""));
        assert!(!json.contains("\"error\""));
    }
}
