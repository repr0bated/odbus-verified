//! MCP Protocol Types
//!
//! JSON-RPC 2.0 protocol types for Model Context Protocol.

use serde::{Deserialize, Serialize};
use simd_json::OwnedValue as Value;

/// JSON-RPC 2.0 Request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpRequest {
    pub jsonrpc: String,
    #[serde(default)]
    pub id: Option<Value>,
    pub method: String,
    #[serde(default)]
    pub params: Option<Value>,
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

impl McpRequest {
    pub fn new(method: impl Into<String>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id: None,
            method: method.into(),
            params: None,
            meta: None,
        }
    }

    pub fn with_id(mut self, id: impl Into<Value>) -> Self {
        self.id = Some(id.into());
        self
    }

    pub fn with_params(mut self, params: Value) -> Self {
        self.params = Some(params);
        self
    }

    pub fn with_meta(mut self, meta: Value) -> Self {
        self.meta = Some(meta);
        self
    }
}

/// JSON-RPC 2.0 Response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpResponse {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

impl McpResponse {
    pub fn success(id: Option<Value>, result: Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: Some(result),
            error: None,
            meta: None,
        }
    }

    pub fn error(id: Option<Value>, error: JsonRpcError) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: None,
            error: Some(error),
            meta: None,
        }
    }

    pub fn with_meta(mut self, meta: Value) -> Self {
        self.meta = Some(meta);
        self
    }

    pub fn is_success(&self) -> bool {
        self.error.is_none()
    }
}

/// JSON-RPC 2.0 Error
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl JsonRpcError {
    pub fn new(code: i32, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }

    pub fn with_data(mut self, data: Value) -> Self {
        self.data = Some(data);
        self
    }

    // Standard JSON-RPC error codes
    pub fn parse_error(msg: impl Into<String>) -> Self {
        Self::new(-32700, msg)
    }

    pub fn invalid_request(msg: impl Into<String>) -> Self {
        Self::new(-32600, msg)
    }

    pub fn method_not_found(method: &str) -> Self {
        Self::new(-32601, format!("Method not found: {}", method))
    }

    pub fn invalid_params(msg: impl Into<String>) -> Self {
        Self::new(-32602, msg)
    }

    pub fn internal_error(msg: impl Into<String>) -> Self {
        Self::new(-32603, msg)
    }
}

/// Type alias for backward compatibility
pub type McpError = JsonRpcError;

#[cfg(test)]
mod tests {
    use super::*;
    use simd_json::json;
    use simd_json::prelude::{ValueAsScalar, ValueObjectAccess};

    #[test]
    fn test_request_serialization() {
        let req = McpRequest::new("tools/list")
            .with_id(json!(1))
            .with_params(json!({"limit": 10}));

        let json_str = simd_json::to_string(&req).unwrap();
        assert!(json_str.contains("tools/list"));
    }

    #[test]
    fn test_response_success() {
        let resp = McpResponse::success(Some(json!(1)), json!({"tools": []}));
        assert!(resp.is_success());
    }

    #[test]
    fn test_response_error() {
        let resp = McpResponse::error(Some(json!(1)), JsonRpcError::method_not_found("unknown"));
        assert!(!resp.is_success());
    }

    #[test]
    fn test_request_meta_round_trip() {
        let req = McpRequest::new("initialize")
            .with_id(json!("abc"))
            .with_meta(json!({"traceId": "trace-123"}));

        let json_str = simd_json::to_string(&req).unwrap();
        assert!(json_str.contains("\"_meta\""));

        let mut json_buf = json_str.clone();
        let parsed: McpRequest = unsafe { simd_json::from_str(&mut json_buf) }.unwrap();
        assert_eq!(
            parsed
                .meta
                .as_ref()
                .and_then(|meta| meta.get("traceId"))
                .and_then(|v| v.as_str()),
            Some("trace-123")
        );
    }

    #[test]
    fn test_response_meta_round_trip() {
        let resp = McpResponse::success(Some(json!(7)), json!({"ok": true}))
            .with_meta(json!({"progressToken": "tok-1"}));

        let json_str = simd_json::to_string(&resp).unwrap();
        assert!(json_str.contains("\"_meta\""));

        let mut json_buf = json_str.clone();
        let parsed: McpResponse = unsafe { simd_json::from_str(&mut json_buf) }.unwrap();
        assert_eq!(
            parsed
                .meta
                .as_ref()
                .and_then(|meta| meta.get("progressToken"))
                .and_then(|v| v.as_str()),
            Some("tok-1")
        );
    }
}
