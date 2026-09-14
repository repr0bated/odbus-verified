//! Deterministic schema summaries rendered through the pinned runnable Canvas profile.

use anyhow::Result;
use async_trait::async_trait;
use op_state::{ApplyResult, Checkpoint, DiffMetadata, PluginCapabilities, StateDiff, StatePlugin};
use op_state_store::{CapabilityDecl, PluginSchema, SideEffect};
use serde::{Deserialize, Serialize};
use simd_json::OwnedValue as Value;

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.software.plugin.schema-renderer.schema@v3"))]
pub struct SchemaRendererState {}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TargetPluginInput {
    pub plugin_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EmptyInput {}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GalleryLookupInput {
    pub gallery_id: String,
    pub rendering_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RenderSchemaOutput {
    pub plugin_id: String,
    pub schema_version: String,
    pub schema_hash: String,
    pub renderer_schema_hash: String,
    pub canvas_profile_id: String,
    pub rendering: serde_json::Value,
    pub field_count: u64,
    pub method_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ValidateSchemaOutput {
    pub plugin_id: String,
    pub schema_version: String,
    pub schema_hash: String,
    pub renderer_schema_hash: String,
    pub valid: bool,
    pub errors: Vec<String>,
    pub checked_fields: u64,
    pub checked_methods: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GalleryDescription {
    /// `workspace` always selects the verified caller's Gemma workspace.
    pub gallery_id: String,
    pub document_revision: u64,
    pub renderer_schema_hash: String,
    pub rendering_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListGalleriesOutput {
    pub galleries: Vec<GalleryDescription>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetRenderingOutput {
    pub gallery_id: String,
    pub rendering: Option<super::gemma_brain::workspace::UiSpecRecord>,
    /// Stale artifacts can be inspected but cannot be rendered or promoted.
    pub render_admitted: bool,
}

pub struct SchemaRendererPlugin;

impl Default for SchemaRendererPlugin {
    fn default() -> Self {
        Self
    }
}

impl SchemaRendererPlugin {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl StatePlugin for SchemaRendererPlugin {
    fn name(&self) -> &str {
        "schema_renderer"
    }
    fn version(&self) -> &str {
        "3.0.0"
    }
    fn schema(&self) -> Option<PluginSchema> {
        Some(schema_renderer_schema())
    }
    async fn calculate_diff(&self, _current: &Value, _desired: &Value) -> Result<StateDiff> {
        Ok(StateDiff {
            plugin: self.name().into(),
            actions: vec![],
            metadata: DiffMetadata {
                timestamp: chrono::Utc::now().timestamp(),
                current_hash: "stateless".into(),
                desired_hash: "stateless".into(),
            },
        })
    }
    async fn apply_state(&self, diff: &StateDiff) -> Result<ApplyResult> {
        anyhow::ensure!(
            diff.actions.is_empty(),
            "schema_renderer has no state mutation surface"
        );
        Ok(ApplyResult {
            success: true,
            changes_applied: vec![],
            errors: vec![],
            checkpoint: None,
        })
    }
    async fn verify_state(&self, desired: &Value) -> Result<bool> {
        Ok(desired == &simd_json::serde::to_owned_value(SchemaRendererState::default())?)
    }
    async fn create_checkpoint(&self) -> Result<Checkpoint> {
        Ok(Checkpoint {
            id: uuid::Uuid::new_v4().to_string(),
            plugin: self.name().into(),
            timestamp: chrono::Utc::now().timestamp(),
            state_snapshot: simd_json::serde::to_owned_value(SchemaRendererState::default())?,
            backend_checkpoint: None,
        })
    }
    async fn rollback(&self, _checkpoint: &Checkpoint) -> Result<()> {
        anyhow::bail!("schema_renderer has no rollback surface")
    }
    fn capabilities(&self) -> PluginCapabilities {
        PluginCapabilities {
            supports_rollback: false,
            supports_checkpoints: true,
            supports_verification: true,
            atomic_operations: false,
        }
    }
}

pub(crate) fn schema_renderer_schema() -> PluginSchema {
    let root = serde_json::to_value(schemars::schema_for!(SchemaRendererState))
        .expect("schema serializes");
    let mut schema = super::schemars_adapter::plugin_schema_from_json(
        "schema_renderer",
        "3.0.0",
        "Pinned PluginSchema rendering and caller-owned Gemma workspace gallery reads",
        &root,
    );
    use super::plugin_scaffold_helpers::method_decl_from_schemars_with_output;
    schema.methods.insert(
        "render_schema".into(),
        method_decl_from_schemars_with_output::<TargetPluginInput, RenderSchemaOutput>(
            "render_schema",
            SideEffect::Read,
            true,
            "schema_renderer.read",
            "obs.service.schema.renderer.render@v2",
        ),
    );
    schema.methods.insert(
        "validate_schema".into(),
        method_decl_from_schemars_with_output::<TargetPluginInput, ValidateSchemaOutput>(
            "validate_schema",
            SideEffect::Read,
            true,
            "schema_renderer.read",
            "obs.service.schema.validate@v2",
        ),
    );
    schema.methods.insert(
        "list_galleries".into(),
        method_decl_from_schemars_with_output::<EmptyInput, ListGalleriesOutput>(
            "list_galleries",
            SideEffect::Read,
            true,
            "schema_renderer.read",
            "obs.service.schema.gallery.list@v3",
        ),
    );
    schema.methods.insert(
        "get_rendering".into(),
        method_decl_from_schemars_with_output::<GalleryLookupInput, GetRenderingOutput>(
            "get_rendering",
            SideEffect::Read,
            true,
            "schema_renderer.read",
            "obs.service.schema.rendering.get@v3",
        ),
    );
    schema.capabilities.insert("schema_renderer.read".into(), CapabilityDecl { id: "schema_renderer.read".into(), description: "Grants deterministic schema reads and caller-owned gallery facades; gallery reads additionally require the exact Gemma workspace grant.".into() });
    schema.dependencies = vec!["json_render".to_owned(), "gemma_brain".to_owned()];
    schema
}

inventory::submit! {
    crate::default_registry::PluginReg::new("schema_renderer", |_ctx| std::sync::Arc::new(SchemaRendererPlugin::new()))
}
