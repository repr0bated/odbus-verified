//! Gemma brain — the umbrella cognitive plugin.
//!
//! Gemma does multiple things, so this is a "brain", not a "router": it owns the
//! tag-routing / subid-classification configuration, the four reasoning
//! perspectives, and the UI-spec gallery surface. It is the WHAT-to-decide
//! surface; its retained taxonomy and private UI workspace methods are local,
//! not inference wrappers. Model selection and inference belong to
//! [`super::tched_router`]; "gemma" is a brain brand, not a model binding.
//!
//! The MutationEngine owns the canonical private workspace methods and commits
//! their full state to owner-scoped SHM before streaming wakeups. The retained
//! legacy generator is not the authority for this plugin's existence or state.

#[path = "gemma_brain_methods.rs"]
pub mod methods;
#[path = "ui_workspace_methods.rs"]
pub mod workspace;

use anyhow::Result;
use async_trait::async_trait;
use op_state::{ApplyResult, Checkpoint, PluginCapabilities, StateDiff, StatePlugin};
use op_state_store::PluginSchema;
use serde::{Deserialize, Serialize};
use simd_json::OwnedValue as Value;

/// Tag-routing / subid-classification configuration owned by the brain.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.software.gemma-brain.routing.schema@v1"))]
pub struct GemmaRouting {
    /// Whether Gemma performs OSCAL subid classification.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.service.gemma-brain.subid-classification@v1"))]
    pub subid_classification: bool,
    /// Whether Gemma performs xray/OpenFlow tag routing.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.service.gemma-brain.tag-routing@v1"))]
    pub tag_routing: bool,
    /// xray config the routing decisions are written into.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "src.service.gemma-brain.xray-config@v1"))]
    pub xray_config_path: String,
}

/// UI-spec gallery surface (the artifact the lovable SPA renders).
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.software.gemma-brain.gallery.schema@v1"))]
pub struct GemmaGallery {
    /// Tmpfs path of the gallery JSON (formerly produced by the retired `op-gemma` binary).
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "exp.service.gemma-brain.gallery-path@v1"))]
    pub specs_path: String,
    /// Target number of specs the gallery is topped up to.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.service.gemma-brain.target-count@v1"))]
    pub target_count: u32,
    /// Current number of specs present (observed live).
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.software.gemma-brain.spec-count@v1"))]
    pub spec_count: u32,
}

/// Top-level state for the Gemma brain.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.software.plugin.gemma-brain.schema@v1"))]
#[schemars(extend("x-oscal-category" = "llm"))]
pub struct GemmaBrainState {
    /// Committed private workspace revision; not public_state-authorized.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(extend("x-oscal-subid" = "obs.service.gemma.ui-workspace.revision@v2"))]
    pub document_revision: Option<u64>,
    /// Stored json_render schema hash, not a recomputed/global catalog hash.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(extend("x-oscal-subid" = "src.service.gemma.ui-renderer.schema-hash@v2"))]
    pub catalog_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(extend("x-oscal-subid" = "obs.service.gemma.ui-workspace.jobs@v2"))]
    pub jobs: Option<std::collections::BTreeMap<String, workspace::UiGenerationJob>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(extend("x-oscal-subid" = "obs.service.gemma.ui-workspace.specs@v2"))]
    pub specs: Option<std::collections::BTreeMap<String, workspace::UiSpecRecord>>,
    /// Derived operational status (observed): `unavailable` (no gallery
    /// producer present), `empty`, `partial`, or `ready`.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.software.gemma-brain.status@v1"))]
    pub status: String,
    /// The generic LLM plugin this brain consumes for inference. Not a model —
    /// the model is selected inside that plugin.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.service.gemma-brain.llm-plugin@v1"))]
    pub llm_plugin: String,
    /// Enabled reasoning perspectives (the four axes).
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.service.gemma-brain.perspectives@v1"))]
    pub perspectives: Vec<String>,
    /// Registered model-agnostic routing tags in this private workspace.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tags: Option<std::collections::BTreeMap<String, methods::GemmaTag>>,
    /// Monotonic revision of the private tag taxonomy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub taxonomy_revision: Option<u64>,
    /// Routing / subid-classification configuration.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.service.gemma-brain.routing@v1"))]
    pub routing: GemmaRouting,
    /// UI-spec gallery surface.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "sch.software.gemma-brain.gallery@v1"))]
    pub gallery: GemmaGallery,
}

pub struct GemmaBrainPlugin;

impl Default for GemmaBrainPlugin {
    fn default() -> Self {
        Self
    }
}

impl GemmaBrainPlugin {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl StatePlugin for GemmaBrainPlugin {
    fn name(&self) -> &str {
        "gemma_brain"
    }

    fn version(&self) -> &str {
        "4.0.0"
    }

    fn schema(&self) -> Option<PluginSchema> {
        let mut schema = gemma_brain_schema();
        super::common::oscal::ensure_category_metadata_fields(&mut schema);
        Some(schema)
    }

    async fn calculate_diff(&self, _current: &Value, _desired: &Value) -> Result<StateDiff> {
        anyhow::bail!("Gemma reconciliation requires canonical owner-scoped method dispatch")
    }

    async fn apply_state(&self, _diff: &StateDiff) -> Result<ApplyResult> {
        anyhow::bail!("Gemma mutation requires canonical owner-scoped method dispatch")
    }

    async fn verify_state(&self, _desired: &Value) -> Result<bool> {
        anyhow::bail!("Gemma verification requires a committed owner-scoped projection")
    }

    async fn create_checkpoint(&self) -> Result<Checkpoint> {
        anyhow::bail!("Gemma has no generic checkpoint owner")
    }

    async fn rollback(&self, _checkpoint: &Checkpoint) -> Result<()> {
        anyhow::bail!("Gemma has no generic rollback owner")
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

/// Canonical `gemma_brain` schema derived from [`GemmaBrainState`] via schemars.
pub(crate) fn gemma_brain_schema() -> PluginSchema {
    let root = serde_json::to_value(schemars::schema_for!(GemmaBrainState))
        .expect("schemars schema serializes to JSON");
    let mut schema = super::schemars_adapter::plugin_schema_from_json(
        "gemma_brain",
        "4.0.0",
        "Gemma cognitive brain: local tag taxonomy, subid classification, reasoning perspectives, and private JSON-render UI workspace",
        &root,
    );

    // Add methods
    use super::plugin_scaffold_helpers::method_decl_from_schemars_with_output;
    use op_state_store::SideEffect;

    schema.methods.insert(
        "classify".to_string(),
        method_decl_from_schemars_with_output::<methods::ClassifyInput, methods::ClassifyOutput>(
            "classify",
            SideEffect::Read,
            true,
            "gemma.invoke",
            "mut.service.gemma.classify@v1",
        ),
    );
    schema.methods.insert(
        "list_perspectives".to_string(),
        method_decl_from_schemars_with_output::<
            methods::ListTaxonomyInput,
            methods::ListPerspectivesOutput,
        >(
            "list_perspectives",
            SideEffect::Read,
            true,
            "gemma.read",
            "obs.service.gemma.perspective.list@v1",
        ),
    );
    schema.methods.insert(
        "list_tags".to_string(),
        method_decl_from_schemars_with_output::<methods::ListTaxonomyInput, methods::ListTagsOutput>(
            "list_tags",
            SideEffect::Read,
            true,
            "gemma.read",
            "obs.service.gemma.tag.list@v1",
        ),
    );
    schema.methods.insert(
        "register_tag".to_string(),
        method_decl_from_schemars_with_output::<
            methods::RegisterTagInput,
            methods::RegisterTagOutput,
        >(
            "register_tag",
            SideEffect::Mutation,
            false,
            "gemma.invoke",
            "mut.service.gemma.tag.register@v1",
        ),
    );
    schema.methods.insert(
        "analyze_intent".to_string(),
        method_decl_from_schemars_with_output::<
            methods::AnalyzeIntentInput,
            methods::AnalyzeIntentOutput,
        >(
            "analyze_intent",
            SideEffect::Read,
            true,
            "gemma.invoke",
            "mut.service.gemma.intent.analyze@v1",
        ),
    );

    workspace::declare_methods(&mut schema);
    schema.dependencies = vec!["json_render".into()];

    schema.capabilities.insert(
        "gemma.invoke".to_string(),
        op_state_store::CapabilityDecl {
            id: "gemma.invoke".to_string(),
            description: "Grants: classify, register_tag, analyze_intent and canonical private UI workspace mutations/recovery.".to_string(),
        },
    );
    schema.capabilities.insert(
        "gemma.read".to_string(),
        op_state_store::CapabilityDecl {
            id: "gemma.read".to_string(),
            description: "Grants: list_perspectives, list_tags and canonical private UI workspace/spec/job reads.".to_string(),
        },
    );

    schema
}

// Self-registration: the plugin registry discovers this via inventory
// (single source of the catalog; no central dispatch list).
inventory::submit! {
    crate::default_registry::PluginReg::new("gemma_brain", |_ctx| std::sync::Arc::new(GemmaBrainPlugin::new()))
}
