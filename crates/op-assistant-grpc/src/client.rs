//! Assistant client wrapper. Thin convenience layer on top of [`Transport`]
//! that normalises Assistant JSON-RPC responses into a `serde_json::Value`.

use crate::error::{AssistantError, Result};
use crate::transport::{Transport, TransportConfig};
use serde_json::{json, Value};
use std::sync::Arc;

#[derive(Clone)]
pub struct AssistantClient {
    transport: Arc<Transport>,
}

impl AssistantClient {
    pub async fn new(cfg: TransportConfig) -> Result<Self> {
        let transport = Transport::new(cfg).await?;
        Ok(Self {
            transport: Arc::new(transport),
        })
    }

    pub fn from_transport(transport: Arc<Transport>) -> Self {
        Self { transport }
    }

    pub fn transport(&self) -> &Transport {
        &self.transport
    }

    /// Invoke a named Assistant method.
    pub async fn call(&self, method: &str, params: Value) -> Result<Value> {
        let envelope = json!({
            "jsonrpc": "2.0",
            "id": uuid::Uuid::new_v4().to_string(),
            "method": method,
            "params": params,
        });
        let raw = self.transport.call(method, envelope).await?;
        unwrap_jsonrpc(raw)
    }
}

/// Strip JSON-RPC envelope. Accepts both `{result: ...}` and `{error: ...}`
/// shapes; if neither is present the raw value is returned as-is.
pub fn unwrap_jsonrpc(value: Value) -> Result<Value> {
    if let Some(err) = value.get("error") {
        let code = err.get("code").and_then(|c| c.as_i64()).unwrap_or(-32000);
        let message = err
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("assistant error")
            .to_string();
        return Err(match code {
            -32601 | 404 => AssistantError::NotFound(message),
            -32602 | 400 => AssistantError::InvalidRequest(message),
            401 => AssistantError::Unauthenticated(message),
            403 => AssistantError::Forbidden(message),
            -32603 | 500 => AssistantError::Internal(message),
            _ => AssistantError::Unknown(message),
        });
    }
    if let Some(result) = value.get("result") {
        return Ok(result.clone());
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unwrap_jsonrpc_returns_result() {
        let v = json!({"jsonrpc": "2.0", "id": "1", "result": {"x": 1}});
        let out = unwrap_jsonrpc(v).unwrap();
        assert_eq!(out, json!({"x": 1}));
    }

    #[test]
    fn unwrap_jsonrpc_maps_not_found() {
        let v = json!({"error": {"code": 404, "message": "no such agent"}});
        let err = unwrap_jsonrpc(v).unwrap_err();
        assert!(matches!(err, AssistantError::NotFound(_)));
    }

    #[test]
    fn unwrap_jsonrpc_returns_raw_when_no_envelope() {
        let v = json!({"agents": []});
        assert_eq!(unwrap_jsonrpc(v.clone()).unwrap(), v);
    }
}
