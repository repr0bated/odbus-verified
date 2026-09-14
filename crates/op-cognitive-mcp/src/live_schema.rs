//! Push-fed mirror of the running plugin contracts.
//!
//! The sealed blob catalog in `/dev/shm/opdbus/plugin-blobs/` remains the
//! durable source of truth. This is its in-process twin: the mutation engine
//! publishes here from the same value it seals and puts on the StateSync
//! stream, so a model calling `blob_catalog` and a UI subscribed to the stream
//! reason about byte-identical contracts rather than two independent reads.
//!
//! It is a mirror, never an authority. When nothing has published — the MCP
//! running standalone over stdio, or a test harness — the catalog is empty and
//! callers fall back to reading SHM, which is why every accessor is written to
//! make emptiness obvious rather than to fake a hit.
//!
//! The publisher lives in `op-grpc-bridge`, which depends on this crate; the
//! dependency cannot run the other way, so the handoff is a process-global
//! rather than a constructor argument. The cognitive tool registry is already a
//! per-process singleton, so this adds no new sharing that did not exist.

use std::collections::BTreeMap;
use std::sync::{Arc, OnceLock, RwLock};

/// One plugin's contract as it was sealed.
#[derive(Debug, Clone)]
pub struct LiveSchema {
    /// Hash of the canonical SCHEMA_JSON bytes below — the same hash that names
    /// the blob file in the SHM catalog.
    pub schema_hash: String,
    /// The sealed `PluginSchema` JSON, parsed. Not re-serialized from a struct:
    /// these are the bytes the hash covers.
    pub schema: serde_json::Value,
}

/// In-process mirror of the sealed contracts, keyed by canonical plugin id.
#[derive(Debug, Default)]
pub struct LiveSchemaCatalog {
    entries: RwLock<BTreeMap<String, LiveSchema>>,
}

impl LiveSchemaCatalog {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record (or replace) one plugin's contract. Replacing rather than
    /// accumulating is deliberate: only one contract is ever current, and a
    /// reseal supersedes what came before it.
    pub fn publish(&self, plugin_id: &str, schema_hash: String, schema: serde_json::Value) {
        if let Ok(mut entries) = self.entries.write() {
            entries.insert(
                plugin_id.to_string(),
                LiveSchema {
                    schema_hash,
                    schema,
                },
            );
        }
    }

    pub fn get(&self, plugin_id: &str) -> Option<LiveSchema> {
        self.entries.read().ok()?.get(plugin_id).cloned()
    }

    /// Canonical plugin ids currently mirrored, sorted. Empty means "nothing
    /// published" — callers must treat that as "ask SHM", not "no plugins".
    pub fn plugin_ids(&self) -> Vec<String> {
        self.entries
            .read()
            .map(|entries| entries.keys().cloned().collect())
            .unwrap_or_default()
    }

    pub fn is_empty(&self) -> bool {
        self.entries
            .read()
            .map(|entries| entries.is_empty())
            .unwrap_or(true)
    }

    pub fn len(&self) -> usize {
        self.entries
            .read()
            .map(|entries| entries.len())
            .unwrap_or(0)
    }
}

/// The process-wide mirror. Created on first touch so neither the publisher nor
/// the readers need an initialization step.
pub fn global() -> &'static Arc<LiveSchemaCatalog> {
    static GLOBAL: OnceLock<Arc<LiveSchemaCatalog>> = OnceLock::new();
    GLOBAL.get_or_init(|| Arc::new(LiveSchemaCatalog::new()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn empty_catalog_reports_itself_as_empty() {
        let catalog = LiveSchemaCatalog::new();
        assert!(catalog.is_empty());
        assert!(catalog.plugin_ids().is_empty());
        assert!(catalog.get("network").is_none());
    }

    #[test]
    fn reseal_supersedes_rather_than_accumulates() {
        let catalog = LiveSchemaCatalog::new();
        catalog.publish("network", "aaaa".into(), json!({ "name": "network" }));
        catalog.publish(
            "network",
            "bbbb".into(),
            json!({ "name": "network", "methods": { "Up": {} } }),
        );

        assert_eq!(catalog.len(), 1);
        let live = catalog.get("network").expect("published");
        assert_eq!(live.schema_hash, "bbbb");
        assert!(live.schema.get("methods").is_some());
    }

    #[test]
    fn plugin_ids_come_back_sorted() {
        let catalog = LiveSchemaCatalog::new();
        catalog.publish("zeroclaw", "1".into(), json!({}));
        catalog.publish("adc", "2".into(), json!({}));
        catalog.publish("network", "3".into(), json!({}));

        assert_eq!(catalog.plugin_ids(), vec!["adc", "network", "zeroclaw"]);
    }
}
