//! Web UI Plugin - serves embedded React SPA
//!
//! Follows the 3-section plugin pattern:
//! - SECTION 1: Immutable Identity (set once, never changes)
//! - SECTION 2: Tunable Config (can change, snowball tracks all changes)
//! - SECTION 3: Capabilities (what this plugin can do)
//!
//! Uses op-identity crate for WireGuard-based authentication.

use anyhow::Result;
use async_trait::async_trait;
use op_snowball::PluginFootprint;
use op_state::{ApplyResult, Checkpoint, PluginCapabilities, StateDiff, StatePlugin};
use op_state_store::PluginSchema;
#[cfg(test)]
use op_state_store::{Constraint, FieldSchema, FieldType};
use serde::{Deserialize, Serialize};
#[cfg(test)]
use simd_json::json;
use simd_json::{prelude::*, OwnedValue as Value};
use std::collections::HashMap;

// ============================================================================
// SECTION 1: IMMUTABLE IDENTITY (set once, never changes)
// ============================================================================

/// Plugin identity - immutable after creation.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.software.plugin.web-ui.identity.schema@v1"))]
pub struct WebUiIdentity {
    /// Plugin name (immutable).
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.software.plugin.web-ui.identity.name@v1"))]
    pub name: String,
    /// Semantic version.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.software.plugin.web-ui.identity.version@v1"))]
    pub version: String,
    /// Plugin classification.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.software.plugin.web-ui.identity.plugin-type@v1"))]
    pub plugin_type: String,
    /// Asset serving driver.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.software.plugin.web-ui.identity.driver@v1"))]
    pub driver: String,
}

impl Default for WebUiIdentity {
    fn default() -> Self {
        Self {
            name: "web_ui".to_string(),
            version: "3.0.0".to_string(),
            plugin_type: "ui".to_string(),
            driver: "rust-embed".to_string(),
        }
    }
}

// ============================================================================
// SECTION 2: TUNABLE CONFIG (can change, snowball tracks all changes)
// ============================================================================

/// Tunable configuration - changes tracked in snowball.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.software.plugin.web-ui.tunables.schema@v1"))]
pub struct WebUiTunables {
    /// Whether UI serving is enabled.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.software.plugin.web-ui.tunables.enabled@v1"))]
    pub enabled: bool,
    /// CORS allowed origins.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.software.plugin.web-ui.tunables.cors-origins@v1"))]
    pub cors_origins: Vec<String>,
    /// Enable gzip/brotli compression.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.software.plugin.web-ui.tunables.compression@v1"))]
    pub compression: bool,
    /// Cache TTL for static assets (seconds).
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.software.plugin.web-ui.tunables.cache-ttl@v1"))]
    pub cache_ttl: u64,
    /// UI theme preference.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.software.plugin.web-ui.tunables.theme@v1"))]
    pub theme: String,
    /// Feature flags for progressive rollout.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.software.plugin.web-ui.tunables.feature-flags@v1"))]
    pub feature_flags: HashMap<String, bool>,
    /// WebSocket configuration.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.software.plugin.web-ui.tunables.websocket@v1"))]
    pub websocket: WebSocketConfig,
    /// API configuration.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.software.plugin.web-ui.tunables.api@v1"))]
    pub api: ApiConfig,
    /// Security configuration.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.software.plugin.web-ui.tunables.security@v1"))]
    pub security: SecurityConfig,
}

impl Default for WebUiTunables {
    fn default() -> Self {
        Self {
            enabled: true,
            cors_origins: vec!["*".to_string()],
            compression: true,
            cache_ttl: 86400,
            theme: "dark".to_string(),
            feature_flags: HashMap::new(),
            websocket: WebSocketConfig::default(),
            api: ApiConfig::default(),
            security: SecurityConfig::default(),
        }
    }
}

/// WebSocket runtime configuration.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.software.plugin.web-ui.websocket.schema@v1"))]
pub struct WebSocketConfig {
    /// Whether WebSocket is enabled.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.software.plugin.web-ui.websocket.enabled@v1"))]
    pub enabled: bool,
    /// Maximum concurrent WebSocket connections.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.software.plugin.web-ui.websocket.max-connections@v1"))]
    pub max_connections: u32,
    /// Ping interval in milliseconds.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.software.plugin.web-ui.websocket.ping-interval-ms@v1"))]
    pub ping_interval_ms: u64,
    /// Maximum inbound message size in bytes.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.software.plugin.web-ui.websocket.message-size-limit@v1"))]
    pub message_size_limit: usize,
}

impl Default for WebSocketConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_connections: 1000,
            ping_interval_ms: 30000,
            message_size_limit: 1024 * 1024,
        }
    }
}

/// API runtime configuration.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.software.plugin.web-ui.api.schema@v1"))]
pub struct ApiConfig {
    /// Rate limit in requests per second.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.software.plugin.web-ui.api.rate-limit-rps@v1"))]
    pub rate_limit_rps: u32,
    /// Request timeout in milliseconds.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.software.plugin.web-ui.api.timeout-ms@v1"))]
    pub timeout_ms: u64,
    /// Maximum request payload size in bytes.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.software.plugin.web-ui.api.max-payload-bytes@v1"))]
    pub max_payload_bytes: usize,
}

impl Default for ApiConfig {
    fn default() -> Self {
        Self {
            rate_limit_rps: 100,
            timeout_ms: 30000,
            max_payload_bytes: 10 * 1024 * 1024,
        }
    }
}

/// Security runtime configuration.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.software.plugin.web-ui.security.schema@v1"))]
pub struct SecurityConfig {
    /// Whether authentication is required.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.software.plugin.web-ui.security.require-auth@v1"))]
    pub require_auth: bool,
    /// Session TTL in seconds.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.software.plugin.web-ui.security.session-ttl-seconds@v1"))]
    pub session_ttl_seconds: u64,
    /// Whether CSRF protection is enabled.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.software.plugin.web-ui.security.csrf-enabled@v1"))]
    pub csrf_enabled: bool,
    /// Content-Security-Policy string.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.software.plugin.web-ui.security.csp-policy@v1"))]
    pub csp_policy: String,
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            require_auth: true,
            session_ttl_seconds: 3600,
            csrf_enabled: true,
            csp_policy: "default-src 'self'".to_string(),
        }
    }
}

// ============================================================================
// SECTION 3: CAPABILITIES (what this plugin can do)
// ============================================================================

/// Plugin capabilities - read-only.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.software.plugin.web-ui.capabilities.schema@v1"))]
pub struct WebUiCapabilities {
    /// Whether the plugin can serve static assets.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.software.plugin.web-ui.capabilities.can-serve-static@v1"))]
    pub can_serve_static: bool,
    /// Whether the plugin can proxy API requests.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.software.plugin.web-ui.capabilities.can-proxy-api@v1"))]
    pub can_proxy_api: bool,
    /// Whether the plugin supports WebSocket.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.software.plugin.web-ui.capabilities.can-websocket@v1"))]
    pub can_websocket: bool,
    /// Whether the plugin supports Server-Sent Events.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.software.plugin.web-ui.capabilities.can-sse@v1"))]
    pub can_sse: bool,
    /// Whether hot reload is supported (false for embedded UI).
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.software.plugin.web-ui.capabilities.supports-hot-reload@v1"))]
    pub supports_hot_reload: bool,
    /// Whether compression is supported.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.software.plugin.web-ui.capabilities.supports-compression@v1"))]
    pub supports_compression: bool,
    /// Whether the plugin requires root privileges.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.software.plugin.web-ui.capabilities.requires-root@v1"))]
    pub requires_root: bool,
    /// Supported platforms.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.software.plugin.web-ui.capabilities.supported-platforms@v1"))]
    pub supported_platforms: Vec<String>,
}

impl Default for WebUiCapabilities {
    fn default() -> Self {
        Self {
            can_serve_static: true,
            can_proxy_api: true,
            can_websocket: true,
            can_sse: true,
            supports_hot_reload: false,
            supports_compression: true,
            requires_root: false,
            supported_platforms: vec!["linux".to_string(), "macos".to_string()],
        }
    }
}

/// Top-level Web UI state exposed by the plugin.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.software.plugin.web-ui.schema@v1"))]
#[schemars(extend("x-oscal-category" = "ui"))]
pub struct WebUiState {
    /// Immutable identity.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "sch.software.plugin.web-ui.identity@v1"))]
    pub identity: WebUiIdentity,
    /// Tunable configuration.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.software.plugin.web-ui.tunables@v1"))]
    pub tunables: WebUiTunables,
    /// Read-only capabilities.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.software.plugin.web-ui.capabilities@v1"))]
    pub capabilities: WebUiCapabilities,
}

impl WebUiTunables {
    /// Property schema - tracks which tunable fields exist (append-only).
    pub fn property_schema() -> Vec<String> {
        vec![
            "enabled".to_string(),
            "cors_origins".to_string(),
            "compression".to_string(),
            "cache_ttl".to_string(),
            "theme".to_string(),
            "feature_flags".to_string(),
            "websocket".to_string(),
            "api".to_string(),
            "security".to_string(),
        ]
    }
}

// ============================================================================
// PLUGIN IMPLEMENTATION
// ============================================================================

/// Web UI State Plugin
pub struct WebUiPlugin {
    identity: WebUiIdentity,
    tunables: WebUiTunables,
    capabilities: WebUiCapabilities,
    #[allow(dead_code)]
    snowball_sender: Option<tokio::sync::mpsc::UnboundedSender<PluginFootprint>>,
}

impl WebUiPlugin {
    pub fn new() -> Self {
        Self {
            identity: WebUiIdentity::default(),
            tunables: WebUiTunables::default(),
            capabilities: WebUiCapabilities::default(),
            snowball_sender: None,
        }
    }

    pub fn with_snowball_sender(
        snowball_sender: tokio::sync::mpsc::UnboundedSender<PluginFootprint>,
    ) -> Self {
        Self {
            identity: WebUiIdentity::default(),
            tunables: WebUiTunables::default(),
            capabilities: WebUiCapabilities::default(),
            snowball_sender: Some(snowball_sender),
        }
    }

    /// Get identity.
    pub fn identity(&self) -> &WebUiIdentity {
        &self.identity
    }

    /// Get tunables.
    pub fn tunables(&self) -> &WebUiTunables {
        &self.tunables
    }

    /// Get capabilities.
    pub fn capabilities(&self) -> &WebUiCapabilities {
        &self.capabilities
    }

    /// Check if a path is immutable.
    pub fn is_path_immutable(path: &str) -> bool {
        let immutable_paths = [
            "/identity",
            "/identity/name",
            "/identity/plugin_type",
            "/identity/driver",
        ];
        immutable_paths.iter().any(|p| path.starts_with(p))
    }

    /// Validate tunables against schema.
    pub fn validate_tunables(tunables: &Value) -> Result<()> {
        if !tunables.is_object() {
            anyhow::bail!("Tunables must be an object");
        }

        if tunables.get("enabled").is_none() {
            anyhow::bail!("Missing required field: enabled");
        }

        // Validate theme enum.
        if let Some(theme) = tunables.get("theme") {
            if let Some(theme_str) = theme.as_str() {
                if !["dark", "light", "system"].contains(&theme_str) {
                    anyhow::bail!("Invalid theme: must be dark, light, or system");
                }
            }
        }

        Ok(())
    }

    pub(crate) fn current_state() -> WebUiState {
        WebUiState {
            identity: WebUiIdentity::default(),
            tunables: WebUiTunables::default(),
            capabilities: WebUiCapabilities::default(),
        }
    }
}

impl Default for WebUiPlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl StatePlugin for WebUiPlugin {
    fn name(&self) -> &str {
        &self.identity.name
    }

    fn version(&self) -> &str {
        &self.identity.version
    }

    fn schema(&self) -> Option<op_state_store::PluginSchema> {
        let mut schema = web_ui_schema();
        super::common::oscal::ensure_category_metadata_fields(&mut schema);
        Some(schema)
    }

    fn is_available(&self) -> bool {
        true // UI is always available (embedded).
    }

    fn unavailable_reason(&self) -> String {
        String::new() // Never unavailable.
    }

    async fn calculate_diff(&self, current: &Value, desired: &Value) -> Result<StateDiff> {
        let _ = (current, desired);
        anyhow::bail!(
            "generic web_ui state diff is unsupported; use its canonical principal/session-owned workspace methods"
        )
    }

    async fn apply_state(&self, diff: &StateDiff) -> Result<ApplyResult> {
        let _ = diff;
        anyhow::bail!(
            "generic web_ui state apply is unsupported; use its canonical principal/session-owned workspace methods"
        )
    }

    async fn verify_state(&self, desired: &Value) -> Result<bool> {
        let _ = desired;
        anyhow::bail!(
            "generic web_ui verification is unsupported; use a canonical workspace read receipt"
        )
    }

    async fn create_checkpoint(&self) -> Result<Checkpoint> {
        anyhow::bail!(
            "generic web_ui checkpoints are unsupported; workspace revisions are owned by the canonical durable document methods"
        )
    }

    async fn rollback(&self, checkpoint: &Checkpoint) -> Result<()> {
        let _ = checkpoint;
        anyhow::bail!(
            "generic web_ui rollback is unsupported; no canonical rollback method is declared"
        )
    }

    fn capabilities(&self) -> PluginCapabilities {
        PluginCapabilities {
            supports_rollback: false,
            supports_checkpoints: false,
            supports_verification: false,
            atomic_operations: false,
        }
    }
}

/// Derived `web_ui` schema from the typed [`WebUiState`] struct via schemars.
pub(crate) fn web_ui_schema() -> PluginSchema {
    let root = serde_json::to_value(schemars::schema_for!(WebUiState))
        .expect("schemars schema serializes to JSON");
    let mut schema = super::schemars_adapter::plugin_schema_from_json(
        "web_ui",
        "3.0.0",
        "Web UI facade over the principal/session-owned Gemma workspace; no independent page or component state",
        &root,
    );
    let state = simd_json::serde::to_owned_value(&WebUiState::default())
        .expect("WebUiState default serializes");
    super::schemars_adapter::apply_state_defaults(&mut schema, &state);

    super::web_ui_methods::declare_methods(&mut schema);
    schema.dependencies = vec!["gemma_brain".to_owned()];

    schema
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state_plugins::common::oscal::validate_subid;
    use crate::state_plugins::schemars_adapter::schema_diffs;
    use serde_json::Value as JVal;

    fn collect_subids(value: &JVal, out: &mut Vec<String>) {
        if let Some(obj) = value.as_object() {
            if let Some(JVal::String(subid)) = obj.get("x-oscal-subid") {
                out.push(subid.clone());
            }
            for v in obj.values() {
                collect_subids(v, out);
            }
        }
        if let Some(arr) = value.as_array() {
            for v in arr {
                collect_subids(v, out);
            }
        }
    }

    #[test]
    fn test_default_identity() {
        let identity = WebUiIdentity::default();
        assert_eq!(identity.name, "web_ui");
        assert_eq!(identity.version, "3.0.0");
        assert_eq!(identity.plugin_type, "ui");
        assert_eq!(identity.driver, "rust-embed");
    }

    #[test]
    fn test_default_tunables() {
        let tunables = WebUiTunables::default();
        assert!(tunables.enabled);
        assert!(tunables.compression);
        assert_eq!(tunables.theme, "dark");
        assert_eq!(tunables.cache_ttl, 86400);
    }

    #[test]
    fn test_default_capabilities() {
        let caps = WebUiCapabilities::default();
        assert!(caps.can_serve_static);
        assert!(caps.can_websocket);
        assert!(!caps.supports_hot_reload);
        assert!(!caps.requires_root);
    }

    #[test]
    fn test_immutable_paths() {
        assert!(WebUiPlugin::is_path_immutable("/identity/name"));
        assert!(WebUiPlugin::is_path_immutable("/identity/plugin_type"));
        assert!(!WebUiPlugin::is_path_immutable("/tunables/enabled"));
        assert!(!WebUiPlugin::is_path_immutable("/tunables/theme"));
    }

    #[test]
    fn test_property_schema() {
        let schema = WebUiTunables::property_schema();
        assert!(schema.contains(&"enabled".to_string()));
        assert!(schema.contains(&"theme".to_string()));
        assert!(schema.contains(&"websocket".to_string()));
    }

    #[tokio::test]
    async fn generic_lifecycle_hooks_never_claim_workspace_effects() {
        let plugin = WebUiPlugin::new();
        let current = simd_json::serde::to_owned_value(WebUiState::default()).unwrap();
        let mut desired_state = WebUiState::default();
        desired_state.tunables.theme = "light".into();
        let desired = simd_json::serde::to_owned_value(desired_state).unwrap();
        assert!(plugin.calculate_diff(&current, &desired).await.is_err());
        let diff = StateDiff {
            plugin: plugin.name().into(),
            actions: Vec::new(),
            metadata: op_state::DiffMetadata {
                timestamp: 0,
                current_hash: String::new(),
                desired_hash: String::new(),
            },
        };
        assert!(plugin.apply_state(&diff).await.is_err());
        assert!(plugin.verify_state(&desired).await.is_err());
        assert!(plugin.create_checkpoint().await.is_err());
        let checkpoint = Checkpoint {
            id: "untrusted-fixture".into(),
            plugin: plugin.name().into(),
            timestamp: 0,
            state_snapshot: simd_json::json!(null),
            backend_checkpoint: None,
        };
        assert!(plugin.rollback(&checkpoint).await.is_err());
        let capabilities = StatePlugin::capabilities(&plugin);
        assert!(!capabilities.supports_rollback);
        assert!(!capabilities.supports_checkpoints);
        assert!(!capabilities.supports_verification);
        assert!(!capabilities.atomic_operations);
    }

    #[test]
    fn all_subids_are_valid() {
        let root = serde_json::to_value(schemars::schema_for!(WebUiState))
            .expect("schemars schema serializes to JSON");
        let mut subids = Vec::new();
        collect_subids(&root, &mut subids);
        assert!(!subids.is_empty(), "expected at least one subid");
        for subid in subids {
            assert!(validate_subid(&subid).is_ok(), "invalid subid: {subid}");
        }
    }
}

// Self-registration: the plugin registry discovers this via inventory
// (single source of the catalog; no central dispatch list).
inventory::submit! {
    crate::default_registry::PluginReg::new("web_ui", |_ctx| std::sync::Arc::new(WebUiPlugin::new()))
}
