//! Compatibility adapter for legacy contract-style plugin schemas.
//!
//! Dead-path note:
//! this module exists only to keep legacy contract consumers alive while the
//! workspace is migrated to canonical plugin documents plus a schema catalog.
//! It must not be treated as a source of schema truth.
//!
//! The canonical schema source of truth is the plugin-owned canonical document,
//! resolved through `op_state_store::SchemaCatalog`.
//! This module preserves the old `schema_for_plugin()` / `all_contract_schemas()`
//! API surface by wrapping catalog-resolved schemas in the legacy contract
//! envelope.

use op_state_store::SchemaCatalog;
use simd_json::OwnedValue as Value;
use std::collections::HashMap;

/// Get contract schema for a single plugin.
pub fn schema_for_plugin(catalog: &SchemaCatalog, plugin: &str) -> Option<Value> {
    catalog.export_contract_for(plugin)
}

/// Get all contract schemas keyed by canonical plugin name.
pub fn all_contract_schemas(catalog: &SchemaCatalog) -> HashMap<String, Value> {
    catalog.export_all_contract()
}

#[cfg(test)]
mod tests {
    use super::*;
    use simd_json::prelude::*;
    use std::collections::HashSet;

    fn test_catalog() -> SchemaCatalog {
        let mut catalog = SchemaCatalog::empty();
        catalog.register(crate::state_plugins::net::net_schema());
        catalog.register(crate::state_plugins::incus::incus_schema());
        catalog.register(crate::state_plugins::service::service_schema());
        catalog
    }

    #[test]
    fn test_all_plugins_have_contract_schema() {
        let catalog = test_catalog();
        let schemas = all_contract_schemas(&catalog);
        assert_eq!(schemas.len(), catalog.list().len());
    }

    #[test]
    fn test_contract_shape_has_required_sections() {
        let catalog = test_catalog();
        let schema = schema_for_plugin(&catalog, "net").expect("net schema");
        let required = schema
            .get("required")
            .and_then(|v| v.as_array())
            .expect("required array");

        let required_strings: Vec<&str> = required.iter().filter_map(|v| v.as_str()).collect();

        for field in [
            "stub",
            "immutable",
            "tunable",
            "observed",
            "meta",
            "semantic_index",
            "privacy_index",
        ] {
            assert!(required_strings.contains(&field));
        }
    }

    #[test]
    fn test_dependency_targets_are_known_plugins() {
        let catalog = test_catalog();
        let known: HashSet<String> = all_contract_schemas(&catalog).keys().cloned().collect();

        for (plugin, schema) in all_contract_schemas(&catalog) {
            let empty: Vec<Value> = Vec::new();
            let deps = schema
                .get("properties")
                .and_then(|v| v.get("meta"))
                .and_then(|v| v.get("properties"))
                .and_then(|v| v.get("dependencies"))
                .and_then(|v| v.get("default"))
                .and_then(|v| v.as_array())
                .unwrap_or(&empty);

            for dep in deps.iter().filter_map(|v| v.as_str()) {
                assert!(
                    known.contains(dep),
                    "plugin '{}' has unknown dependency '{}'",
                    plugin,
                    dep
                );
            }
        }
    }

    #[test]
    fn test_uniform_index_paths_use_absolute_json_paths() {
        let catalog = test_catalog();

        fn validate_path_array(paths: Option<&Vec<Value>>, context: &str) {
            if let Some(arr) = paths {
                for path in arr.iter().filter_map(|v| v.as_str()) {
                    assert!(
                        path.starts_with('/'),
                        "{} contains non-absolute path '{}'",
                        context,
                        path
                    );
                }
            }
        }

        for (plugin, schema) in all_contract_schemas(&catalog) {
            let semantic = schema
                .get("properties")
                .and_then(|v| v.get("semantic_index"))
                .and_then(|v| v.get("properties"));

            validate_path_array(
                semantic
                    .and_then(|v| v.get("include_paths"))
                    .and_then(|v| v.get("default"))
                    .and_then(|v| v.as_array()),
                &format!("{}.semantic_index.include_paths", plugin),
            );
            validate_path_array(
                semantic
                    .and_then(|v| v.get("exclude_paths"))
                    .and_then(|v| v.get("default"))
                    .and_then(|v| v.as_array()),
                &format!("{}.semantic_index.exclude_paths", plugin),
            );

            let redaction = schema
                .get("properties")
                .and_then(|v| v.get("privacy_index"))
                .and_then(|v| v.get("properties"))
                .and_then(|v| v.get("redaction"))
                .and_then(|v| v.get("properties"));

            validate_path_array(
                redaction
                    .and_then(|v| v.get("secret_paths"))
                    .and_then(|v| v.get("default"))
                    .and_then(|v| v.as_array()),
                &format!("{}.privacy_index.redaction.secret_paths", plugin),
            );
            validate_path_array(
                redaction
                    .and_then(|v| v.get("pii_paths"))
                    .and_then(|v| v.get("default"))
                    .and_then(|v| v.as_array()),
                &format!("{}.privacy_index.redaction.pii_paths", plugin),
            );
        }
    }

    #[test]
    fn test_recovery_priority_is_bounded() {
        let catalog = test_catalog();

        for (plugin, schema) in all_contract_schemas(&catalog) {
            let priority = schema
                .get("properties")
                .and_then(|v| v.get("meta"))
                .and_then(|v| v.get("properties"))
                .and_then(|v| v.get("recovery_priority"))
                .and_then(|v| v.get("default"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0);

            assert!(
                priority <= 100,
                "plugin '{}' has out-of-range recovery priority {}",
                plugin,
                priority
            );
        }
    }

    #[test]
    fn test_aliases_resolve_from_registry() {
        let catalog = test_catalog();

        assert!(schema_for_plugin(&catalog, "network").is_some());
        assert!(schema_for_plugin(&catalog, "incus").is_some());
    }
}
