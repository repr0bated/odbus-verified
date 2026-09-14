//! Plugin loader - discovers state plugins and instantiates them
//!
//! This module deliberately does not maintain a second plugin list. If a state
//! plugin exists in `state_plugins`, the registry discovers it and registers it.
//!
//! ## Canonical Path Enforcement
//!
//! All plugins must use canonical D-Bus paths:
//! - Path: `/org/opdbus/v1/plugins/{name}`
//! - Interface: `org.opdbus.v1.PluginV1`
//!
//! Legacy paths are deprecated and will be rejected.

use anyhow::{anyhow, Result};
use op_state_store::StateStore;
use simd_json::prelude::*;
use std::sync::Arc;

use crate::AutoPlugin;

/// Plugins deliberately removed from the live control plane.
///
/// A retired implementation may remain for recovery tooling or be deleted
/// entirely; either way, never publish or auto-create its id as a callable
/// plugin. The sealed blob catalog and generated gRPC routes are both derived
/// from [`DefaultPluginRegistry::available_plugins`], so this is the common
/// fail-closed boundary.
///
/// - `netmaker`: overlay/mesh is gone. Keep the id fail-closed so a leftover
///   blob cannot reappear as a callable plugin.
/// - `compact_mcp`: retired as a PluginV1 / MCP *plugin*. Compact *mode* for
///   the singleton control-plane chatbot is in-process in op-web, not a
///   reflected plugin and not a second MCP listener.
/// - `factory`: the former account/platform plugin is retired; Factory CLI/SSH
///   provider vocabulary remains in `op-llm`, while model routing stays in
///   `tched_router`.
pub const RETIRED_PLUGIN_IDS: &[&str] = &["compact_mcp", "factory", "netmaker"];

pub fn is_retired_plugin(plugin_id: &str) -> bool {
    RETIRED_PLUGIN_IDS.contains(&plugin_id)
}

/// Context handed to a plugin's registered constructor at load time. Wraps the
/// registry so co-located registrations can reach the few inputs a constructor
/// needs (state store, config-path resolution) without exposing private fields.
pub struct PluginCtx<'a> {
    reg: &'a DefaultPluginRegistry,
}

impl PluginCtx<'_> {
    /// Shared state store (used by e.g. the `mcp` plugin).
    pub fn state_store(&self) -> Arc<dyn StateStore> {
        self.reg.state_store.clone()
    }

    /// Resolve a plugin's configured `config_path`, falling back to `default`.
    pub fn config_path(&self, plugin_name: &str, default: &str) -> String {
        self.reg.get_plugin_config_path(plugin_name, default)
    }
}

/// A single plugin's self-registration: its canonical name plus a constructor.
///
/// Each plugin module emits one of these via [`inventory::submit!`], co-located
/// with the plugin's own definition. The registry iterates the collected set —
/// this is the **only** plugin catalog; there is no central dispatch list.
pub struct PluginReg {
    /// Canonical loader name (what `load_plugin` / `available_plugins` expose).
    pub name: &'static str,
    /// Constructor. `build` (not `ctor`/`actor`) to avoid any confusion with the
    /// MutationEngine `actor_id` identity concept.
    pub build: fn(&PluginCtx) -> Arc<dyn op_state::StatePlugin>,
}

impl PluginReg {
    pub const fn new(
        name: &'static str,
        build: fn(&PluginCtx) -> Arc<dyn op_state::StatePlugin>,
    ) -> Self {
        Self { name, build }
    }
}

inventory::collect!(PluginReg);

/// Default plugin loader configuration
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PluginRegistryConfig {
    /// Deprecated runtime policy field. Registration is discovery-based.
    #[serde(default = "default_auto_load")]
    pub auto_load: Vec<String>,

    /// Plugin-specific configurations
    #[serde(default)]
    pub plugin_configs: std::collections::HashMap<String, simd_json::OwnedValue>,
}

fn default_auto_load() -> Vec<String> {
    Vec::new()
}

impl Default for PluginRegistryConfig {
    fn default() -> Self {
        Self {
            auto_load: default_auto_load(),
            plugin_configs: std::collections::HashMap::new(),
        }
    }
}

/// Default plugin loader.
///
/// This is intentionally not the authoritative plugin catalog. Its job is to
/// instantiate the built-in plugin implementations so the real catalog path can
/// persist their canonical plugin documents during registration.
pub struct DefaultPluginRegistry {
    config: PluginRegistryConfig,
    state_store: Arc<dyn StateStore>,
}

impl DefaultPluginRegistry {
    /// Create a new plugin loader
    pub fn new(state_store: Arc<dyn StateStore>) -> Self {
        Self {
            config: PluginRegistryConfig::default(),
            state_store,
        }
    }

    /// Create with custom configuration
    pub fn with_config(state_store: Arc<dyn StateStore>, config: PluginRegistryConfig) -> Self {
        Self {
            config,
            state_store,
        }
    }

    /// Resolve user/request-facing plugin references into canonical loader names.
    ///
    /// Supports direct names, aliases, and projection paths like
    /// `/org/opdbus/v1/plugins/<plugin>/...`.
    ///
    /// The old `/org/opdbus/v1/plugin/plugins/...` spelling is accepted as a
    /// compatibility alias and normalized to the canonical plural base path.
    pub fn resolve_requested_plugin_name(requested: &str) -> Result<String> {
        let trimmed = requested.trim();
        if trimmed.is_empty() {
            return Err(anyhow!("Plugin identifier cannot be empty"));
        }

        let extracted = Self::extract_plugin_name_from_projection_path(trimmed);
        let from_path = extracted.as_deref().unwrap_or(trimmed);
        let normalized = from_path
            .trim()
            .trim_matches('/')
            .replace('-', "_")
            .to_lowercase();

        let canonical = match normalized.as_str() {
            "network" => "net",
            "systemd" | "dinit" => "service",
            "web_ui" | "webui" => "web_ui",
            "mail" | "mailserver" => "mail_server",
            "ovsbridge" => "ovsdb_bridge",
            "rtnet" => "rtnetlink",
            "sessdecl" => "sess_decl",
            other => other,
        };

        Ok(canonical.to_string())
    }

    fn extract_plugin_name_from_projection_path(requested: &str) -> Option<String> {
        // Normalize aliases first, then extract the top-level plugin name.
        let normalized = crate::canonical::normalize_plugin_path(requested)?;
        normalized
            .strip_prefix(crate::canonical::PLUGIN_BASE_PATH)?
            .trim_start_matches('/')
            .split('/')
            .find(|segment| !segment.is_empty())
            .map(|segment| segment.to_string())
    }

    /// Compatibility shim for older callers. Registration is discovery-based:
    /// if a state plugin exists, it is loaded.
    pub async fn load_default_plugins(&self) -> Result<Vec<Arc<dyn op_state::StatePlugin>>> {
        self.load_all_plugins().await
    }

    /// Load every built-in plugin implementation so the schema catalog can
    /// publish the full plugin contract set, not just runtime autoload state.
    pub async fn load_all_plugins(&self) -> Result<Vec<Arc<dyn op_state::StatePlugin>>> {
        let mut plugins: Vec<Arc<dyn op_state::StatePlugin>> = Vec::new();
        let mut plugin_names = Self::available_plugins();

        plugin_names.sort();
        plugin_names.dedup();

        for plugin_name in plugin_names {
            match self.load_plugin(&plugin_name).await {
                Ok(plugin) => {
                    if !plugin.is_available() {
                        tracing::info!(
                            "Loaded unavailable plugin schema {}: {}",
                            plugin_name,
                            plugin.unavailable_reason()
                        );
                    } else {
                        tracing::info!("Loaded plugin schema: {}", plugin_name);
                    }
                    plugins.push(plugin);
                }
                Err(e) => {
                    tracing::warn!("Failed to load plugin schema {}: {}", plugin_name, e);
                }
            }
        }

        tracing::info!("Loaded {} built-in plugin schemas", plugins.len());
        Ok(plugins)
    }

    /// Load a specific plugin by name.
    ///
    /// Resolves the request to a canonical name, then looks it up in the
    /// inventory of plugin self-registrations (each plugin module emits a
    /// [`PluginReg`] via `inventory::submit!`). Unknown names fall through to the
    /// auto-create review-required draft. There is no central dispatch list.
    pub async fn load_plugin(&self, name: &str) -> Result<Arc<dyn op_state::StatePlugin>> {
        let resolved_name = Self::resolve_requested_plugin_name(name)?;

        if is_retired_plugin(&resolved_name) {
            return Err(anyhow!(
                "plugin '{}' is retired and cannot be loaded or recreated",
                resolved_name
            ));
        }

        let ctx = PluginCtx { reg: self };
        for registration in inventory::iter::<PluginReg> {
            if registration.name == resolved_name {
                return Ok((registration.build)(&ctx));
            }
        }

        let requested_info = format!("requested='{}' resolved='{}'", name, resolved_name);
        tracing::warn!(
            "Unknown plugin '{}'; auto-creating review-required draft from requested info",
            name
        );
        Ok(Arc::new(
            AutoPlugin::create_from_requested_info(&resolved_name, &requested_info).await,
        ))
    }

    /// Get plugin-specific config value or default
    fn get_plugin_config_path(&self, plugin_name: &str, default: &str) -> String {
        self.config
            .plugin_configs
            .get(plugin_name)
            .and_then(|v| v.get("config_path"))
            .and_then(|v| v.as_str())
            .unwrap_or(default)
            .to_string()
    }

    /// The plugin catalog: every name self-registered via `inventory::submit!`.
    ///
    /// This is intentionally not a hand-maintained list. Each plugin module is
    /// the single source of its own registration; this just collects them.
    /// Unknown subjects still flow through `load_plugin()` and the auto-create
    /// fallback.
    pub fn available_plugins() -> Vec<String> {
        let mut plugins: Vec<String> = inventory::iter::<PluginReg>
            .into_iter()
            .map(|registration| registration.name.to_string())
            .filter(|plugin_id| !is_retired_plugin(plugin_id))
            .collect();

        plugins.sort();
        plugins.dedup();
        plugins
    }

    /// Check if a plugin is enabled for auto-load
    pub fn is_auto_load(&self, plugin_name: &str) -> bool {
        self.config.auto_load.contains(&plugin_name.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use op_state_store::MemoryStore;

    #[tokio::test]
    async fn test_default_plugin_registry() {
        let store = Arc::new(MemoryStore::new());
        let registry = DefaultPluginRegistry::new(store);

        let plugins = registry.load_default_plugins().await.unwrap();
        assert!(!plugins.is_empty());
        assert!(!plugins.iter().any(|plugin| plugin.name() == "factory"));
        assert!(plugins.iter().any(|plugin| plugin.name() == "mail_server"));
        assert!(plugins.iter().any(|plugin| plugin.name() == "tched_router"));
    }

    #[tokio::test]
    async fn test_discovered_plugins_publish_schema() {
        let store = Arc::new(MemoryStore::new());
        let registry = DefaultPluginRegistry::new(store);

        let plugins = registry.load_all_plugins().await.unwrap();
        let missing: Vec<String> = plugins
            .iter()
            .filter(|plugin| {
                plugin
                    .schema()
                    .map(|schema| schema.name != plugin.name())
                    .unwrap_or(true)
            })
            .map(|plugin| plugin.name().to_string())
            .collect();

        assert!(
            missing.is_empty(),
            "discovered plugins missing plugin-owned schema: {:?}",
            missing
        );
    }

    #[tokio::test]
    async fn test_retired_plugins_are_not_loadable_or_advertised() {
        let store = Arc::new(MemoryStore::new());
        let registry = DefaultPluginRegistry::new(store);

        let advertised = DefaultPluginRegistry::available_plugins();
        for plugin_id in RETIRED_PLUGIN_IDS {
            assert!(!advertised.iter().any(|candidate| candidate == *plugin_id));

            let error = registry
                .load_plugin(plugin_id)
                .await
                .err()
                .expect("retired plugin must fail closed");
            assert!(error.to_string().contains("retired"));
        }
    }

    /// Every plugin with canonical-form capability ids on its methods MUST
    /// declare them in `schema.capabilities` (the plugin is the single
    /// declaration point). Legacy short-form and derived ids are tolerated
    /// until the capability refactor completes — only `missing` fails.
    #[tokio::test]
    async fn test_plugins_have_capability_closure() {
        let store = Arc::new(MemoryStore::new());
        let registry = DefaultPluginRegistry::new(store);

        let plugins = registry.load_all_plugins().await.unwrap();
        let mut failures: Vec<String> = Vec::new();
        let mut legacy_total = 0usize;
        let mut derived_total = 0usize;
        for plugin in &plugins {
            let Some(schema) = plugin.schema() else {
                continue;
            };
            let closure = schema.validate_capability_closure();
            legacy_total += closure.legacy.len();
            derived_total += closure.derived.len();
            if !closure.missing.is_empty() {
                failures.push(format!(
                    "{}: undeclared canonical capabilities {:?}",
                    schema.name, closure.missing
                ));
            }
        }
        eprintln!(
            "capability closure: {} plugins, legacy tolerated: {}, derived tolerated: {}",
            plugins.len(),
            legacy_total,
            derived_total
        );
        assert!(
            failures.is_empty(),
            "plugins with undeclared canonical capabilities:\n{}",
            failures.join("\n")
        );
    }

    #[tokio::test]
    async fn test_loadable_plugins_publish_schema() {
        let store = Arc::new(MemoryStore::new());
        let registry = DefaultPluginRegistry::new(store);
        let plugin_names = vec![
            "mcp",
            "config",
            "systemd",
            "dinit",
            "incus",
            "net",
            "openflow",
            "hardware",
            "software",
            "users",
            "gcloud_adc",
            "keypair",
            "service",
            "wireguard",
            "agent_config",
            "ovsdb_bridge",
            "procfs",
            "rtnetlink",
            "sess_decl",
            "adc",
            "endpoint",
            "proxy_server",
            "web_ui",
        ];

        let mut missing = Vec::new();
        for plugin_name in plugin_names {
            let plugin = registry
                .load_plugin(plugin_name)
                .await
                .unwrap_or_else(|error| panic!("failed to load {}: {}", plugin_name, error));
            if plugin.schema().is_none() {
                missing.push(plugin_name.to_string());
            }
        }

        assert!(
            missing.is_empty(),
            "loadable plugins missing schema(): {:?}",
            missing
        );
    }

    #[tokio::test]
    async fn test_auto_load_config_does_not_control_registration() {
        let store = Arc::new(MemoryStore::new());

        let config = PluginRegistryConfig {
            auto_load: vec!["service".to_string()],
            plugin_configs: std::collections::HashMap::new(),
        };

        let registry = DefaultPluginRegistry::with_config(store, config);

        assert!(registry.is_auto_load("service"));
        assert!(!registry.is_auto_load("mcp"));
        assert!(!registry.is_auto_load("config"));

        let plugins = registry.load_default_plugins().await.unwrap();
        assert!(plugins.iter().any(|plugin| plugin.name() == "mcp"));
        assert!(plugins.iter().any(|plugin| plugin.name() == "config"));
    }

    #[test]
    fn test_resolve_requested_plugin_name() {
        use crate::canonical;

        // Canonical path format (REQUIRED)
        assert_eq!(
            DefaultPluginRegistry::resolve_requested_plugin_name(&format!(
                "{}/procfs/memory",
                canonical::PLUGIN_BASE_PATH
            ))
            .unwrap(),
            "procfs"
        );

        // Direct plugin name
        assert_eq!(
            DefaultPluginRegistry::resolve_requested_plugin_name("systemd").unwrap(),
            "service"
        );
        assert_eq!(
            DefaultPluginRegistry::resolve_requested_plugin_name("web-ui").unwrap(),
            "web_ui"
        );
    }

    #[tokio::test]
    async fn test_load_plugin_from_canonical_projection_path() {
        use crate::canonical;
        let store = Arc::new(MemoryStore::new());
        let registry = DefaultPluginRegistry::new(store);

        // Use canonical path format
        let path = format!("{}/procfs/memory", canonical::PLUGIN_BASE_PATH);
        let procfs = registry.load_plugin(&path).await.unwrap();
        assert_eq!(procfs.name(), "procfs");
    }

    #[test]
    fn test_alias_paths_are_resolved() {
        assert_eq!(
            DefaultPluginRegistry::resolve_requested_plugin_name("/opdbus/v1/plugins/procfs")
                .unwrap(),
            "procfs"
        );
        assert_eq!(
            DefaultPluginRegistry::resolve_requested_plugin_name(
                "/org/opdbus/v1/plugin/plugins/procfs"
            )
            .unwrap(),
            "procfs"
        );
        assert_eq!(
            DefaultPluginRegistry::resolve_requested_plugin_name("/org/opdbus/v1/plugins/procfs")
                .unwrap(),
            "procfs"
        );
    }

    #[tokio::test]
    async fn test_unknown_plugin_auto_creates_review_draft() {
        let store = Arc::new(MemoryStore::new());
        let registry = DefaultPluginRegistry::new(store);

        let plugin = registry.load_plugin("new_future_plugin").await.unwrap();
        assert_eq!(plugin.name(), "new_future_plugin");
    }

    #[tokio::test]
    async fn all_plugin_subids_are_valid_and_unique() {
        use crate::state_plugins::common::oscal::{category_required_fields, validate_subid};
        use std::collections::HashMap;

        let store = Arc::new(MemoryStore::new());
        let registry = DefaultPluginRegistry::new(store);
        let plugins = registry.load_all_plugins().await.unwrap();

        let mut all_subids: Vec<(String, String, String)> = Vec::new(); // schema plugin, key, subid
        let mut declaration_subids: Vec<(String, String, String)> = Vec::new();
        let mut errors: Vec<String> = Vec::new();

        for plugin in &plugins {
            let name = plugin.name().to_string();
            let Some(schema) = plugin.schema() else {
                continue;
            };
            for (key, subid) in &schema.subids {
                all_subids.push((name.clone(), key.clone(), subid.clone()));
            }
            for (key, method) in &schema.methods {
                declaration_subids.push((
                    name.clone(),
                    format!("method:{key}"),
                    method.subid.clone(),
                ));
            }
            for signal in &schema.signals {
                declaration_subids.push((
                    name.clone(),
                    format!("signal:{}", signal.name),
                    signal.subid.clone(),
                ));
            }
        }

        // 1. Every schema annotation, method and signal subid must match the
        // canonical OSCAL grammar. Method/signal bindings may legitimately
        // repeat an annotation owned by the same plugin, so this adds no new
        // cross-surface uniqueness claim.
        for (plugin_name, key, subid) in all_subids.iter().chain(&declaration_subids) {
            if let Err(e) = validate_subid(subid) {
                errors.push(format!(
                    "plugin '{plugin_name}' key '{key}' has invalid subid '{subid}': {e}"
                ));
            }
        }

        // 2. No subid value may appear more than once across all plugins.
        let mut seen: HashMap<String, (String, String)> = HashMap::new();
        for (plugin_name, key, subid) in &all_subids {
            if let Some((prev_plugin, prev_key)) = seen.get(subid) {
                errors.push(format!(
                    "duplicate subid '{subid}' in plugin '{plugin_name}' key '{key}' (first seen in plugin '{prev_plugin}' key '{prev_key}')"
                ));
            } else {
                seen.insert(subid.clone(), (plugin_name.clone(), key.clone()));
            }
        }

        // 3. Category-specific required metadata fields must be present in the plugin schema.
        for (plugin_name, key, subid) in &all_subids {
            let Some(schema) = plugins
                .iter()
                .find(|p| p.name() == plugin_name)
                .and_then(|p| p.schema())
            else {
                continue;
            };
            let category = subid.split('.').next().unwrap_or("");
            let required_fields = category_required_fields(category);
            if category == "evt" {
                // evt.* requires at least one of event_id or event_hash.
                if !required_fields
                    .iter()
                    .any(|field| schema.fields.contains_key(*field))
                {
                    errors.push(format!(
                        "plugin '{plugin_name}' subid '{subid}' (key '{key}') is evt.* and requires one of {:?} fields, but none found in schema fields",
                        required_fields
                    ));
                }
            } else if !required_fields.is_empty() {
                for field in required_fields {
                    if !schema.fields.contains_key(*field) {
                        errors.push(format!(
                            "plugin '{plugin_name}' subid '{subid}' (key '{key}') is {category}.* and requires field '{field}', but it is not present in schema fields"
                        ));
                    }
                }
            }
        }

        assert!(
            errors.is_empty(),
            "OSCAL subid gate failures:\n{}",
            errors.join("\n")
        );
    }
}
