//! Block events and plugin footprints for the streaming snowball

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use simd_json::prelude::*;
use std::collections::HashMap;

/// A block event in the streaming snowball
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockEvent {
    pub timestamp: u64,
    pub category: String,
    pub action: String,
    pub data: simd_json::OwnedValue,
    pub hash: String,
    pub vector: Vec<f32>,
}

impl BlockEvent {
    /// Create a new block event
    pub fn new(
        category: impl Into<String>,
        action: impl Into<String>,
        data: simd_json::OwnedValue,
    ) -> Self {
        let timestamp = chrono::Utc::now().timestamp_millis() as u64;
        let category = category.into();
        let action = action.into();

        // Compute hash
        let hash_input = format!("{}:{}:{}:{}", timestamp, category, action, data);
        let mut hasher = Sha256::new();
        hasher.update(hash_input.as_bytes());
        let hash = format!("{:x}", hasher.finalize());

        Self {
            timestamp,
            category,
            action,
            data,
            hash,
            vector: Vec::new(), // Empty vector, can be populated by ML
        }
    }

    /// Create with a pre-computed vector
    pub fn with_vector(mut self, vector: Vec<f32>) -> Self {
        self.vector = vector;
        self
    }
}

/// A payload envelope carried from a plugin/sled into the timing and vector
/// projections. A footprint is deliberately not a digest: Snowball consumes
/// the payload directly, while the authoritative chain owns hashing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginFootprint {
    pub plugin_id: String,
    pub operation: String,
    pub timestamp: u64,
    pub payload: simd_json::OwnedValue,
    pub metadata: HashMap<String, simd_json::OwnedValue>,
    pub vector_features: Vec<f32>,
}

impl PluginFootprint {
    /// Create a new plugin footprint
    pub fn new(
        plugin_id: impl Into<String>,
        operation: impl Into<String>,
        data: &simd_json::OwnedValue,
    ) -> Self {
        let timestamp = chrono::Utc::now().timestamp_millis() as u64;

        Self {
            plugin_id: plugin_id.into(),
            operation: operation.into(),
            timestamp,
            payload: data.clone(),
            metadata: HashMap::new(),
            vector_features: Vec::new(),
        }
    }

    /// Add metadata
    pub fn with_metadata(mut self, key: impl Into<String>, value: simd_json::OwnedValue) -> Self {
        self.metadata.insert(key.into(), value);
        self
    }

    /// Convert to a BlockEvent
    /// NOTE: Vectors are dropped for plugin footprints - timing is authoritative,
    /// vectors are async projections handled separately via Voyage AI embedding pipeline
    pub fn to_block_event(&self) -> BlockEvent {
        let data = simd_json::json!({
            "plugin_id": self.plugin_id,
            "operation": self.operation,
            "payload": self.payload,
            "metadata": self.metadata
        });

        // A persisted ChainEvent already owns the Snowball chain hash. Reuse
        // it directly; never hash that hash through an intermediate footprint.
        if let Some(chain_hash) = self
            .payload
            .get("event_hash")
            .and_then(simd_json::prelude::ValueAsScalar::as_str)
        {
            return BlockEvent {
                timestamp: self.timestamp,
                category: self.plugin_id.clone(),
                action: self.operation.clone(),
                data,
                hash: chain_hash.to_string(),
                vector: Vec::new(),
            };
        }
        BlockEvent::new(self.plugin_id.clone(), self.operation.clone(), data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_block_event_creation() {
        let event = BlockEvent::new("test", "create", simd_json::json!({"key": "value"}));

        assert!(!event.hash.is_empty());
        assert_eq!(event.category, "test");
        assert_eq!(event.action, "create");
    }

    #[test]
    fn test_plugin_footprint_creation() {
        let footprint = PluginFootprint::new(
            "systemd",
            "unit_started",
            &simd_json::json!({"unit": "nginx.service"}),
        );

        assert_eq!(footprint.plugin_id, "systemd");
        assert_eq!(footprint.payload["unit"], "nginx.service");
    }

    #[test]
    fn chain_event_hash_is_carried_without_hashing_the_hash() {
        let expected = "ab".repeat(32);
        let footprint = PluginFootprint::new(
            "identity_sled",
            "session_arrival",
            &simd_json::json!({
                "event_hash": expected.clone(),
                "input_payload": {"session_id": "session-a"}
            }),
        );
        let event = footprint.to_block_event();
        assert_eq!(event.hash, expected);
        assert_eq!(
            event.data["payload"]["input_payload"]["session_id"],
            "session-a"
        );
    }
}
