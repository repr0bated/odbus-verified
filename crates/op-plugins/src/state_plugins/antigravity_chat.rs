use anyhow::Result;
use async_trait::async_trait;
use op_state::{ApplyResult, Checkpoint, PluginCapabilities, StateDiff, StatePlugin};
use op_state_store::PluginSchema;
use serde::{Deserialize, Serialize};
use simd_json::OwnedValue as Value;

/// Provider options read from the durable runtime document.
#[derive(Debug, Clone, Serialize, Deserialize, Default, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.software.antigravity-chat.config.schema@v1"))]
pub struct ChatConfig {
    /// Provider protocol selected by the durable runtime owner.
    #[serde(default = "default_provider_route")]
    #[schemars(extend("x-oscal-subid" = "mut.software.antigravity-chat.config-provider@v1"))]
    pub provider: String,
    /// Base URL consumed by native provider requests.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.software.antigravity-chat.config-base-url@v1"))]
    pub base_url: String,
}

/// Bounded native readback from an OpenAI-compatible `/v1/models` endpoint.
#[derive(Debug, Clone, Serialize, Deserialize, Default, schemars::JsonSchema)]
#[schemars(extend("x-opdbus-exact-json-schema" = true))]
pub struct NativeBridgeStatus {
    pub url: String,
    pub reachable: bool,
    pub http_status: u16,
    #[serde(default)]
    pub model_ids: Vec<String>,
    pub checked_at: String,
}

/// Sanitized receipt from the Google credential owner.
#[derive(Debug, Clone, Serialize, Deserialize, Default, schemars::JsonSchema)]
#[schemars(extend("x-opdbus-exact-json-schema" = true))]
pub struct TokenRefreshReceipt {
    pub credential_type: String,
    /// A valid access token was obtained and discarded without disclosure.
    pub token_available: bool,
    /// Time at which the native credential owner completed the observation.
    pub observed_at: String,
    #[serde(default)]
    pub scopes: Vec<String>,
}

/// Runtime options consumed by native Antigravity generation requests.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-opdbus-exact-json-schema" = true))]
pub struct ConfigureInput {
    /// Revision of the shared Antigravity runtime document; zero means absent.
    pub expected_revision: u64,
    /// `gemini` or `openai_compatible`.
    #[schemars(regex(pattern = "^(gemini|openai_compatible)$"))]
    pub provider: String,
    /// HTTPS provider base URL, or a loopback HTTP URL for a local bridge.
    pub base_url: String,
    /// Provider model used when a request does not explicitly select one.
    #[schemars(length(min = 1, max = 256), regex(pattern = "^[A-Za-z0-9._-]+$"))]
    pub selected_model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-opdbus-exact-json-schema" = true))]
pub struct ConfigureOutput {
    pub revision: u64,
    pub provider: String,
    pub base_url: String,
    pub selected_model: String,
}

fn default_llm_plugin() -> String {
    "large_language_model".to_string()
}

fn default_provider_route() -> String {
    "gemini".to_string()
}

/// Top-level Antigravity Chat state.
///
/// Gemini model catalog is **not** owned here — resolve via `llm_plugin` +
/// `provider_route` (default `large_language_model` / `gemini`). This plugin
/// holds the native bridge observation, sanitized auth, and selected model id.
#[derive(Debug, Clone, Serialize, Deserialize, Default, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.software.plugin.antigravity-chat.schema@v1"))]
#[schemars(extend("x-oscal-category" = "llm"))]
pub struct AntigravityChatState {
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.software.antigravity-chat.status@v1"))]
    pub status: String,
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.software.antigravity-chat.bridge@v1"))]
    pub bridge: NativeBridgeStatus,
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.software.antigravity-chat.auth@v1"))]
    pub auth: super::antigravity::AntigravityAuthStatus,
    /// Plugin that owns the model catalog (default large_language_model).
    #[serde(default = "default_llm_plugin")]
    #[schemars(
        description = "Generation/catalog plugin Antigravity delegates to for models (default large_language_model). Catalog lives there; this plugin holds no Gemini model list.",
        example = &"large_language_model",
        extend("x-oscal-subid" = "mut.software.antigravity-chat.llm-plugin@v1")
    )]
    pub llm_plugin: String,
    /// Provider route inside `llm_plugin` (default gemini).
    #[serde(default = "default_provider_route")]
    #[schemars(
        description = "Provider route on llm_plugin for the Gemini catalog (default gemini).",
        example = &"gemini",
        extend("x-oscal-subid" = "mut.software.antigravity-chat.provider-route@v1")
    )]
    pub provider_route: String,
    /// Session-selected model id from the delegated catalog; empty = provider default.
    #[serde(default)]
    #[schemars(
        description = "Selected model id from the delegated catalog. Empty falls back to the provider default. Not a local catalog entry.",
        extend("x-oscal-subid" = "mut.software.antigravity-chat.selected-model@v1")
    )]
    pub selected_model: String,
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "sch.software.antigravity-chat.config@v1"))]
    pub config: ChatConfig,
}

pub struct AntigravityChatPlugin;

impl Default for AntigravityChatPlugin {
    fn default() -> Self {
        Self
    }
}

impl AntigravityChatPlugin {
    pub fn new() -> Self {
        Self
    }

    pub fn current_state() -> AntigravityChatState {
        AntigravityChatState {
            status: "declared".to_string(),
            bridge: NativeBridgeStatus::default(),
            auth: super::antigravity::AntigravityAuthStatus::default(),
            llm_plugin: default_llm_plugin(),
            provider_route: default_provider_route(),
            selected_model: String::new(),
            config: ChatConfig {
                provider: "gemini".to_string(),
                base_url: "https://generativelanguage.googleapis.com/v1beta".to_string(),
            },
        }
    }
}

#[async_trait]
impl StatePlugin for AntigravityChatPlugin {
    fn name(&self) -> &str {
        "antigravity_chat"
    }
    fn version(&self) -> &str {
        "2.0.0"
    }
    fn schema(&self) -> Option<PluginSchema> {
        let mut schema = antigravity_chat_schema();
        super::common::oscal::ensure_category_metadata_fields(&mut schema);
        Some(schema)
    }
    async fn calculate_diff(&self, _current: &Value, _desired: &Value) -> Result<StateDiff> {
        anyhow::bail!("antigravity_chat reconciliation has no authoritative bridge backend")
    }
    async fn apply_state(&self, _diff: &StateDiff) -> Result<ApplyResult> {
        anyhow::bail!("antigravity_chat apply has no authoritative bridge backend")
    }
    async fn verify_state(&self, _desired: &Value) -> Result<bool> {
        anyhow::bail!("antigravity_chat verification requires bridge readback")
    }
    async fn create_checkpoint(&self) -> Result<Checkpoint> {
        anyhow::bail!("antigravity_chat has no authoritative checkpoint owner")
    }
    async fn rollback(&self, _checkpoint: &Checkpoint) -> Result<()> {
        anyhow::bail!("antigravity_chat has no authoritative rollback owner")
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

pub(crate) fn antigravity_chat_schema() -> PluginSchema {
    let root = serde_json::to_value(schemars::schema_for!(AntigravityChatState))
        .expect("schemars schema serializes to JSON");
    let mut schema = super::schemars_adapter::plugin_schema_from_json(
        "antigravity_chat",
        "2.0.0",
        "Antigravity Chat — native Google credential and OpenAI-compatible bridge control; model catalog delegated to large_language_model",
        &root,
    );

    use super::plugin_scaffold_helpers::{EmptyInput, method_decl_from_schemars_with_output};
    use op_state_store::SideEffect;

    #[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
    #[schemars(extend("x-opdbus-exact-json-schema" = true))]
    pub struct GetBridgeStatusOutput {
        pub bridge: NativeBridgeStatus,
    }
    #[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
    #[schemars(extend("x-opdbus-exact-json-schema" = true))]
    pub struct RefreshTokenOutput {
        pub token: TokenRefreshReceipt,
    }

    // Model catalog remains owned by llm_plugin/provider_route. These methods
    // control and observe only the concrete provider bridge and credential owner.
    schema.methods.insert(
        "get_bridge_status".to_string(),
        method_decl_from_schemars_with_output::<EmptyInput, GetBridgeStatusOutput>(
            "get_bridge_status",
            SideEffect::Read,
            true,
            "antigravity_chat.read",
            "obs.software.antigravity-chat.bridge.status.get@v1",
        ),
    );
    schema.methods.insert(
        "refresh_token".to_string(),
        method_decl_from_schemars_with_output::<EmptyInput, RefreshTokenOutput>(
            "refresh_token",
            SideEffect::Mutation,
            false,
            "antigravity_chat.write",
            "mut.software.antigravity-chat.auth.token.refresh@v1",
        ),
    );
    schema.methods.insert(
        "configure".to_string(),
        method_decl_from_schemars_with_output::<ConfigureInput, ConfigureOutput>(
            "configure",
            SideEffect::Mutation,
            false,
            "antigravity_chat.write",
            "mut.software.antigravity-chat.config.set@v1",
        ),
    );
    schema.capabilities.insert(
        "antigravity_chat.read".to_string(),
        op_state_store::CapabilityDecl {
            id: "antigravity_chat.read".to_string(),
            description: "Grants bounded native bridge model readback.".to_string(),
        },
    );
    schema.capabilities.insert(
        "antigravity_chat.write".to_string(),
        op_state_store::CapabilityDecl {
            id: "antigravity_chat.write".to_string(),
            description:
                "Grants revisioned provider configuration and native Google token refresh."
                    .to_string(),
        },
    );

    schema
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state_plugins::common::oscal::validate_subid;
    use serde_json::Value as JVal;

    #[test]
    fn schema_is_schemars_seeded_and_typed() {
        let schema = antigravity_chat_schema();
        assert_eq!(schema.name, "antigravity_chat");
        assert_eq!(schema.version, "2.0.0");
        assert!(schema.fields.contains_key("status"));
        assert!(schema.fields.contains_key("bridge"));
        assert!(schema.fields.contains_key("auth"));
        assert!(schema.fields.contains_key("llm_plugin"));
        assert!(schema.fields.contains_key("provider_route"));
        assert!(schema.fields.contains_key("selected_model"));
        assert!(schema.fields.contains_key("config"));
        assert!(!schema.fields.contains_key("models"));
        assert!(!schema.methods.contains_key("list_models"));
    }

    #[test]
    fn all_subids_are_valid() {
        let raw = serde_json::to_value(schemars::schema_for!(AntigravityChatState)).unwrap();
        let mut subids = Vec::new();
        collect_subids(&raw, &mut subids);
        assert!(
            !subids.is_empty(),
            "expected at least one x-oscal-subid in the derived schema"
        );
        for subid in subids {
            validate_subid(&subid).unwrap_or_else(|e| panic!("invalid subid {subid}: {e}"));
        }
    }

    #[tokio::test]
    async fn generic_lifecycle_never_returns_a_false_receipt() {
        let plugin = AntigravityChatPlugin::new();
        let state = simd_json::json!({});
        assert!(plugin.calculate_diff(&state, &state).await.is_err());
        let diff = StateDiff {
            plugin: plugin.name().to_owned(),
            actions: vec![],
            metadata: op_state::DiffMetadata {
                timestamp: 0,
                current_hash: String::new(),
                desired_hash: String::new(),
            },
        };
        assert!(plugin.apply_state(&diff).await.is_err());
        assert!(plugin.verify_state(&state).await.is_err());
        assert!(plugin.create_checkpoint().await.is_err());
        let checkpoint = Checkpoint {
            id: "fixture".into(),
            plugin: plugin.name().into(),
            timestamp: 0,
            state_snapshot: state,
            backend_checkpoint: None,
        };
        assert!(plugin.rollback(&checkpoint).await.is_err());
        let capabilities = plugin.capabilities();
        assert!(!capabilities.supports_verification);
        assert!(!capabilities.supports_checkpoints);
        assert!(!capabilities.supports_rollback);
        assert!(!capabilities.atomic_operations);
    }

    fn collect_subids(node: &JVal, out: &mut Vec<String>) {
        if let Some(subid) = node.get("x-oscal-subid").and_then(JVal::as_str) {
            out.push(subid.to_string());
        }
        if let Some(props) = node.get("properties").and_then(JVal::as_object) {
            for v in props.values() {
                collect_subids(v, out);
            }
        }
        if let Some(defs) = node
            .get("$defs")
            .or_else(|| node.get("definitions"))
            .and_then(JVal::as_object)
        {
            for v in defs.values() {
                collect_subids(v, out);
            }
        }
        if let Some(items) = node.get("items") {
            collect_subids(items, out);
        }
        if let Some(alternatives) = node
            .get("anyOf")
            .or_else(|| node.get("oneOf"))
            .and_then(JVal::as_array)
        {
            for v in alternatives {
                collect_subids(v, out);
            }
        }
    }
}

// Self-registration: the plugin registry discovers this via inventory
// (single source of the catalog; no central dispatch list).
inventory::submit! {
    crate::default_registry::PluginReg::new("antigravity_chat", |_ctx| std::sync::Arc::new(AntigravityChatPlugin::new()))
}
