//! System Information Tools

use crate::tool_registry::{Tool, ToolRegistry};
use anyhow::Result;
use async_trait::async_trait;
use simd_json::{json, OwnedValue as Value};
use std::sync::Arc;

pub async fn register_all(registry: &ToolRegistry) -> Result<usize> {
    registry.register(Arc::new(ListNetworkInterfacesTool)).await?;
    Ok(1)
}

pub struct ListNetworkInterfacesTool;

#[async_trait]
impl Tool for ListNetworkInterfacesTool {
    fn name(&self) -> &str { "list_network_interfaces" }
    fn description(&self) -> &str { "List all network interfaces." }
    fn category(&self) -> &str { "network" }
    fn tags(&self) -> Vec<String> { vec!["network".into(), "interfaces".into()] }
    
    fn input_schema(&self) -> Value {
        json!({"type": "object", "properties": {}})
    }

    async fn execute(&self, _input: Value) -> Result<Value> {
        let mut interfaces = Vec::new();
        let mut dir = tokio::fs::read_dir("/sys/class/net").await?;
        
        while let Some(entry) = dir.next_entry().await? {
            let name = entry.file_name().to_string_lossy().to_string();
            let state = tokio::fs::read_to_string(format!("/sys/class/net/{}/operstate", name))
                .await.unwrap_or_else(|_| "unknown".into()).trim().to_string();
            let mac = tokio::fs::read_to_string(format!("/sys/class/net/{}/address", name))
                .await.unwrap_or_else(|_| "unknown".into()).trim().to_string();
            
            interfaces.push(json!({"name": name, "state": state, "mac": mac}));
        }
        
        Ok(json!({"success": true, "interfaces": interfaces}))
    }
}
