//! PlugTree - Hierarchical plugin pattern for managing collections of resources
//!
//! A PlugTree allows a plugin to manage multiple independent sub-resources (pluglets),
//! each with its own state and lifecycle.
//!
//! Example: LXC plugin manages multiple containers, each container is a pluglet
//!
//! Architecture:
//! ```text
//! Plugin (PlugTree)
//!  ├─ Pluglet:100 (individual container)
//!  ├─ Pluglet:101 (individual container)
//!  └─ Pluglet:102 (individual container)
//! ```

use anyhow::Result;
use async_trait::async_trait;
use simd_json::prelude::*;
use simd_json::OwnedValue as Value;

use super::plugin::ApplyResult;

/// Trait for plugins that manage collections of independent sub-resources
#[async_trait]
#[allow(dead_code)]
pub trait PlugTree: Send + Sync {
    /// Type name of the sub-resources (e.g., "container", "interface", "unit")
    fn pluglet_type(&self) -> &str;

    /// Get unique identifier field name (e.g., "id", "name", "interface")
    fn pluglet_id_field(&self) -> &str;

    /// Extract pluglet ID from a resource value
    fn extract_pluglet_id(&self, resource: &Value) -> Result<String>;

    /// Apply state to a single pluglet by ID
    async fn apply_pluglet(&self, pluglet_id: &str, desired: &Value) -> Result<ApplyResult>;

    /// Query state of a single pluglet by ID
    async fn query_pluglet(&self, pluglet_id: &str) -> Result<Option<Value>>;

    /// List all pluglet IDs currently managed
    async fn list_pluglet_ids(&self) -> Result<Vec<String>>;
}

/// Helper to extract pluglets from plugin state
#[allow(dead_code)]
pub fn extract_pluglets(plugin_state: &Value, collection_key: &str) -> Result<Vec<Value>> {
    plugin_state
        .get(collection_key)
        .and_then(|v| v.as_array())
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("No '{}' array in plugin state", collection_key))
}

/// Helper to find a specific pluglet by ID
#[allow(dead_code)]
pub fn find_pluglet_by_id(
    plugin_state: &Value,
    collection_key: &str,
    id_field: &str,
    target_id: &str,
) -> Result<Option<Value>> {
    let pluglets = extract_pluglets(plugin_state, collection_key)?;

    for pluglet in pluglets {
        if let Some(id_value) = pluglet.get(id_field) {
            if let Some(id_str) = id_value.as_str() {
                if id_str == target_id {
                    return Ok(Some(pluglet));
                }
            }
        }
    }

    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use simd_json::json;

    #[test]
    fn test_extract_pluglets() {
        let state = json!({
            "containers": [
                {"id": "100", "name": "test1"},
                {"id": "101", "name": "test2"}
            ]
        });

        let pluglets = extract_pluglets(&state, "containers").unwrap();
        assert_eq!(pluglets.len(), 2);
    }

    #[test]
    fn test_find_pluglet_by_id() {
        let state = json!({
            "containers": [
                {"id": "100", "name": "test1"},
                {"id": "101", "name": "test2"}
            ]
        });

        let pluglet = find_pluglet_by_id(&state, "containers", "id", "101").unwrap();
        assert!(pluglet.is_some());
        assert_eq!(pluglet.unwrap()["name"], "test2");

        let not_found = find_pluglet_by_id(&state, "containers", "id", "999").unwrap();
        assert!(not_found.is_none());
    }
}
