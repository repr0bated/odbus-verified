//! D-Bus interface for the Cognitive MCP tool registry.
//!
//! service:   org.opdbus.v1.plugins
//! object:    /org/opdbus/v1/plugins/cognitive_mcp
//! interface: org.opdbus.v1.plugins.CognitiveMcp
//!
//! Methods:
//!   ListTools() -> s                  JSON array [{name, description, category}]
//!   GetToolSchema(s name) -> s        JSON input schema, or "null"
//!   CallTool(s name, s args_json) -> s  JSON result, or {"error":"..."}

use op_mcp::tool_registry::ToolRegistry;
use simd_json::prelude::*;
use std::sync::Arc;
use zbus::interface;

pub struct CognitiveMcpInterface {
    registry: Arc<ToolRegistry>,
}

impl CognitiveMcpInterface {
    pub fn new(registry: Arc<ToolRegistry>) -> Self {
        Self { registry }
    }
}

#[interface(name = "org.opdbus.v1.plugins.CognitiveMcp")]
impl CognitiveMcpInterface {
    async fn list_tools(&self) -> zbus::fdo::Result<String> {
        let defs = self.registry.list(0, usize::MAX, None).await;
        let arr: Vec<serde_json::Value> = defs
            .iter()
            .map(|d| serde_json::json!({ "name": d.name, "description": d.description, "category": d.category }))
            .collect();
        serde_json::to_string(&arr).map_err(|e| zbus::fdo::Error::Failed(e.to_string()))
    }

    async fn get_tool_schema(&self, name: String) -> zbus::fdo::Result<String> {
        match self.registry.get_definition(&name).await {
            Some(def) => simd_json::to_string(&def.input_schema)
                .map_err(|e| zbus::fdo::Error::Failed(e.to_string())),
            None => Ok("null".to_string()),
        }
    }

    async fn call_tool(&self, name: String, args_json: String) -> String {
        let args = match parse_simd(&args_json) {
            Ok(v) => v,
            Err(e) => return err_json(&e),
        };
        match self.registry.execute(&name, args).await {
            Ok(result) => simd_json::to_string(&result)
                .unwrap_or_else(|_| r#"{"error":"serialize failed"}"#.to_string()),
            Err(e) => err_json(&e.to_string()),
        }
    }
}

fn parse_simd(s: &str) -> Result<simd_json::OwnedValue, String> {
    let mut buf = s.as_bytes().to_vec();
    simd_json::from_slice(&mut buf).map_err(|e| e.to_string())
}

fn err_json(msg: &str) -> String {
    serde_json::json!({ "error": msg }).to_string()
}
