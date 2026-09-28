//! json_render plugin — GB.JsonRender.
//!
//! Runtime projection for the json-render generative UI framework.

use anyhow::Result;
use async_trait::async_trait;
use op_state::{ApplyResult, Checkpoint, DiffMetadata, PluginCapabilities, StateDiff, StatePlugin};
use op_state_store::{PluginSchema, SideEffect};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use simd_json::OwnedValue as Value;

use super::plugin_scaffold_helpers::method_decl_from_schemars_with_output;

#[path = "json_render_canvas_profile.rs"]
pub mod canvas_profile;
#[path = "json_render_catalog_v019.rs"]
mod catalog_v019;
#[path = "json_render_chat_canvas.rs"]
pub mod chat_canvas;
#[path = "json_render_gallery.rs"]
pub mod gallery;
#[path = "json_render_methods.rs"]
pub mod methods;
#[path = "json_render_runtime_contract.rs"]
mod runtime_contract;
#[path = "json_render_validation_bounds.rs"]
pub mod validation_bounds;

// =============================================================================
// PLUGIN ENTRY: identity and typed schema seed
// =============================================================================

const PLUGIN_NAME: &str = "json_render";
const PLUGIN_VERSION: &str = "3.0.0";
const PLUGIN_CATEGORY: &str = "software";
const PLUGIN_DESCRIPTION: &str = "json-render catalog projection — schema, specs, catalogs, renderers, actions, validation, and upstream API inventory";
const PLUGIN_DISPLAY_NAME: &str = "GB.JsonRender";
const JSON_RENDER_SOURCE_COMMIT: &str = "e2d00faeaabe2871ca18a4594a9ec39a245f9b6c";

/// Runtime projection for the json-render generative UI framework.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.software.plugin.json-render.schema@v2"))]
#[schemars(extend("x-oscal-category" = "software"))]
pub struct JsonRenderState {
    /// Plugin-owned generated Canvas contract checked against the active UI
    /// source. Distinct from the upstream reference inventory; no deployment claim.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "exp.software.plugin.json-render.canvas-profile@v3"))]
    pub canvas_profile: canvas_profile::CanvasProfile,
    /// The 3tched chat Canvas contract imported from the UI's own export:
    /// what chat canvases, gallery candidates and promoted catalog entries
    /// are composed from. Static compositions only; no deployment claim.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "exp.software.plugin.json-render.chat-canvas-profile@v1"))]
    pub chat_canvas_profile: chat_canvas::ChatCanvasProfile,
    /// Operational status of the catalog projection.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.software.plugin.json-render.status@v2"))]
    pub status: String,
    /// Source authority used to define this projection.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "src.software.plugin.json-render.source@v2"))]
    pub source: JsonRenderSource,
    /// Local policy for exposing json-render through D-Bus projections.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "sch.software.plugin.json-render.config@v2"))]
    pub config: JsonRenderConfig,
    /// Flat UI tree contract used by json-render specs.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "sch.software.plugin.json-render.spec-contract@v2"))]
    pub spec_contract: SpecContract,
    /// Package inventory observed from the upstream workspace.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.software.plugin.json-render.packages@v2"))]
    pub packages: Vec<JsonRenderPackage>,
    /// Core exported functions and schemas available to hosts.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.software.plugin.json-render.core-exports@v2"))]
    pub core_exports: Vec<ApiExport>,
    /// Renderer packages and their host integration surfaces.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "exp.software.plugin.json-render.renderers@v2"))]
    pub renderers: Vec<RendererSurface>,
    /// Catalog component declarations from the shadcn catalog exemplar.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "exp.software.plugin.json-render.components@v2"))]
    pub components: Vec<ComponentDecl>,
    /// Built-in runtime actions exposed by the React action provider.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "exp.software.plugin.json-render.actions@v2"))]
    pub actions: Vec<ActionDecl>,
    /// Validation checks built into the core validation module.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.software.plugin.json-render.validation-checks@v2"))]
    pub validation_checks: Vec<NamedCapability>,
    /// Built-in and custom directive vocabulary.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.software.plugin.json-render.directives@v2"))]
    pub directives: Vec<NamedCapability>,
}

/// Source provenance for this plugin's curated json-render model.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "src.software.plugin.json-render.source.schema@v2"))]
pub struct JsonRenderSource {
    /// Documentation home page.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "src.software.plugin.json-render.source-docs@v2"))]
    pub docs_url: String,
    /// Upstream repository URL.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "src.software.plugin.json-render.source-repository@v2"))]
    pub repository_url: String,
    /// Upstream commit inspected for package and API inventory.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "src.software.plugin.json-render.source-commit@v2"))]
    pub commit: String,
    /// Short source summary.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.software.plugin.json-render.source-summary@v2"))]
    pub summary: String,
}

impl Default for JsonRenderSource {
    fn default() -> Self {
        Self {
            docs_url: "https://json-render.dev/docs".to_string(),
            repository_url: "https://github.com/vercel-labs/json-render".to_string(),
            commit: JSON_RENDER_SOURCE_COMMIT.to_string(),
            summary: "Manifest-pinned upstream reference catalog. Catalog presence does not claim that a component is implemented by the deployed UI.".to_string(),
        }
    }
}

/// Local D-Bus projection policy for json-render.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.software.plugin.json-render.config.schema@v2"))]
#[serde(deny_unknown_fields)]
pub struct JsonRenderConfig {
    /// Allow catalog metadata to be exposed through D-Bus.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.software.plugin.json-render.config-catalog-exposure@v2"))]
    pub expose_catalog: bool,
    /// Allow prompt-building metadata to be exposed through D-Bus.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.software.plugin.json-render.config-prompt-exposure@v2"))]
    pub expose_prompt_surface: bool,
    /// Default schema export mode.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.software.plugin.json-render.config-schema-mode@v2"))]
    pub schema_mode: JsonSchemaMode,
    /// Default renderer target for UI projection previews.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.software.plugin.json-render.config-renderer@v2"))]
    pub default_renderer: RendererTarget,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum JsonSchemaMode {
    Standard,
    Strict,
}

impl Default for JsonSchemaMode {
    fn default() -> Self {
        Self::Strict
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum RendererTarget {
    React,
    Next,
    Vue,
    Svelte,
    Solid,
    ReactNative,
    ReactPdf,
    ReactEmail,
    Image,
    Remotion,
    Ink,
    ReactThreeFiber,
}

impl Default for RendererTarget {
    fn default() -> Self {
        Self::React
    }
}

impl Default for JsonRenderConfig {
    fn default() -> Self {
        Self {
            expose_catalog: true,
            expose_prompt_surface: true,
            schema_mode: JsonSchemaMode::Strict,
            default_renderer: RendererTarget::React,
        }
    }
}

/// json-render spec grammar at the shape level.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.software.plugin.json-render.spec.schema@v2"))]
pub struct SpecContract {
    /// Required root element key.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "sch.software.plugin.json-render.spec-root@v2"))]
    pub root_field: String,
    /// Required elements map.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "sch.software.plugin.json-render.spec-elements@v2"))]
    pub elements_field: String,
    /// Optional initial state field.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "sch.software.plugin.json-render.spec-state@v2"))]
    pub state_field: String,
    /// UI element fields recognized by core.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "sch.software.plugin.json-render.element-fields@v2"))]
    pub element_fields: Vec<String>,
    /// Supported JSON Patch operations for streaming edits.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "sch.software.plugin.json-render.patch-ops@v2"))]
    pub patch_ops: Vec<String>,
}

impl Default for SpecContract {
    fn default() -> Self {
        Self {
            root_field: "root".to_string(),
            elements_field: "elements".to_string(),
            state_field: "state".to_string(),
            element_fields: vec![
                "type".to_string(),
                "props".to_string(),
                "children".to_string(),
                "visible".to_string(),
                "on".to_string(),
                "repeat".to_string(),
                "watch".to_string(),
            ],
            patch_ops: vec![
                "add".to_string(),
                "remove".to_string(),
                "replace".to_string(),
                "move".to_string(),
                "copy".to_string(),
                "test".to_string(),
            ],
        }
    }
}

/// Upstream package inventory item.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "obs.software.plugin.json-render.package.schema@v2"))]
pub struct JsonRenderPackage {
    /// Workspace folder name.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.software.plugin.json-render.package-folder@v2"))]
    pub folder: String,
    /// npm package name.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.software.plugin.json-render.package-name@v2"))]
    pub package_name: String,
    /// Package version observed upstream.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.software.plugin.json-render.package-version@v2"))]
    pub version: String,
    /// Package role in the json-render system.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.software.plugin.json-render.package-role@v2"))]
    pub role: String,
}

/// Public API export or host method.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "obs.software.plugin.json-render.api-export.schema@v2"))]
pub struct ApiExport {
    /// Exported symbol.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.software.plugin.json-render.api-export-name@v2"))]
    pub name: String,
    /// Package or module that owns this symbol.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.software.plugin.json-render.api-export-package@v2"))]
    pub package: String,
    /// Export kind.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.software.plugin.json-render.api-export-kind@v2"))]
    pub kind: String,
    /// Operational category.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.software.plugin.json-render.api-export-category@v2"))]
    pub category: String,
}

/// Renderer package surface.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "exp.software.plugin.json-render.renderer.schema@v2"))]
pub struct RendererSurface {
    /// Renderer identifier.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "exp.software.plugin.json-render.renderer-id@v2"))]
    pub id: String,
    /// Package providing this renderer.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "exp.software.plugin.json-render.renderer-package@v2"))]
    pub package_name: String,
    /// Render target.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "exp.software.plugin.json-render.renderer-target@v2"))]
    pub target: String,
    /// Important exported host methods.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "exp.software.plugin.json-render.renderer-exports@v2"))]
    pub exports: Vec<String>,
}

/// Catalog component declaration.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "exp.software.plugin.json-render.component.schema@v2"))]
pub struct ComponentDecl {
    /// Component name.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "exp.software.plugin.json-render.component-name@v2"))]
    pub name: String,
    /// Component category.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "exp.software.plugin.json-render.component-category@v2"))]
    pub category: String,
    /// Required or commonly used props.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "exp.software.plugin.json-render.component-props@v2"))]
    pub props: Vec<String>,
    /// Exact Draft 7 schema extracted once from the pinned upstream Zod
    /// declaration. This structured value is the validation/export authority.
    pub props_schema: JsonValue,
    /// Named child slots accepted by the component.
    #[serde(default)]
    pub slots: Vec<String>,
    /// Supported event names.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "exp.software.plugin.json-render.component-events@v2"))]
    pub events: Vec<String>,
    /// Human-readable description.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "exp.software.plugin.json-render.component-description@v2"))]
    pub description: String,
}

/// Runtime action declaration.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "exp.software.plugin.json-render.action.schema@v2"))]
pub struct ActionDecl {
    /// Action name.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "exp.software.plugin.json-render.action-name@v2"))]
    pub name: String,
    /// Action parameter names.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "exp.software.plugin.json-render.action-params@v2"))]
    pub params: Vec<String>,
    /// Exact parameter schema for this built-in action.
    pub params_schema: JsonValue,
    /// Renderer targets whose pinned provider implements this action.
    pub renderers: Vec<RendererTarget>,
    /// Whether the action mutates state.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "exp.software.plugin.json-render.action-mutates-state@v2"))]
    pub mutates_state: bool,
    /// Human-readable description.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "exp.software.plugin.json-render.action-description@v2"))]
    pub description: String,
}

/// Named capability for validation, directives, and feature inventories.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "obs.software.plugin.json-render.capability.schema@v2"))]
pub struct NamedCapability {
    /// Capability name.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.software.plugin.json-render.capability-name@v2"))]
    pub name: String,
    /// Capability category.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.software.plugin.json-render.capability-category@v2"))]
    pub category: String,
    /// Short description.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.software.plugin.json-render.capability-description@v2"))]
    pub description: String,
}

/// D-Bus method surface exposed by this state plugin.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "exp.software.plugin.json-render.method.schema@v2"))]
pub struct MethodSurface {
    /// D-Bus method name.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "exp.software.plugin.json-render.method-name@v2"))]
    pub name: String,
    /// Read or mutation side effect.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "exp.software.plugin.json-render.method-side-effect@v2"))]
    pub side_effect: String,
    /// Required capability.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "exp.software.plugin.json-render.method-capability@v2"))]
    pub required_capability: String,
    /// Stable OSCAL subid for the method.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "exp.software.plugin.json-render.method-subid@v2"))]
    pub subid: String,
    /// Whether current persisted policy permits this method's metadata surface.
    pub available: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EmptyJsonRenderInput {}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetJsonRenderConfigInput {
    pub config: JsonRenderConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ComponentLookupInput {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PackageLookupInput {
    pub package_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SpecValidationInput {
    pub spec: JsonValue,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SchemaExportInput {
    #[serde(default)]
    pub mode: Option<JsonSchemaMode>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PromptMode {
    Standalone,
    Inline,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PromptSurfaceInput {
    #[serde(default)]
    pub mode: Option<PromptMode>,
    #[serde(default)]
    pub custom_rules: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct HealthOutput {
    pub status: String,
    pub package_count: usize,
    pub component_count: usize,
    pub action_count: usize,
    pub source_commit: String,
    pub catalog_kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ConfigOutput {
    pub config: JsonRenderConfig,
    pub revision: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SetConfigOutput {
    pub config: JsonRenderConfig,
    pub changed: bool,
    pub revision: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ToolsOutput {
    pub tools: Vec<MethodSurface>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PackagesOutput {
    pub packages: Vec<JsonRenderPackage>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PackageOutput {
    pub package: Option<JsonRenderPackage>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ComponentsOutput {
    pub components: Vec<ComponentDecl>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ComponentOutput {
    pub component: Option<ComponentDecl>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ActionsOutput {
    pub actions: Vec<ActionDecl>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CoreExportsOutput {
    pub exports: Vec<ApiExport>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct RenderersOutput {
    pub renderers: Vec<RendererSurface>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SpecContractOutput {
    pub spec_contract: SpecContract,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ValidationResultOutput {
    pub valid: bool,
    pub issues: Vec<String>,
    pub checked_elements: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SchemaExportOutput {
    pub mode: JsonSchemaMode,
    pub schema: JsonValue,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PromptSurfaceOutput {
    pub mode: PromptMode,
    pub renderer: RendererTarget,
    pub prompt: String,
}

impl Default for JsonRenderState {
    fn default() -> Self {
        Self {
            canvas_profile: canvas_profile::CanvasProfile::default(),
            chat_canvas_profile: chat_canvas::ChatCanvasProfile::default(),
            status: "catalog_snapshot".to_string(),
            source: JsonRenderSource::default(),
            config: JsonRenderConfig::default(),
            spec_contract: SpecContract::default(),
            packages: default_packages(),
            core_exports: default_core_exports(),
            renderers: default_renderers(),
            components: default_components(),
            actions: default_actions(),
            validation_checks: default_validation_checks(),
            directives: default_directives(),
        }
    }
}

// =============================================================================
// PLUGIN BODY: D-Bus-backed behavior only
// =============================================================================

pub struct JsonRenderPlugin;

impl Default for JsonRenderPlugin {
    fn default() -> Self {
        Self
    }
}

impl JsonRenderPlugin {
    pub fn new() -> Self {
        Self
    }

    pub(crate) fn current_state() -> JsonRenderState {
        JsonRenderState::default()
    }
}

#[async_trait]
impl StatePlugin for JsonRenderPlugin {
    fn name(&self) -> &str {
        PLUGIN_NAME
    }

    fn version(&self) -> &str {
        PLUGIN_VERSION
    }

    fn schema(&self) -> Option<PluginSchema> {
        Some(json_render_schema())
    }

    async fn calculate_diff(&self, _current: &Value, _desired: &Value) -> Result<StateDiff> {
        Ok(StateDiff {
            plugin: PLUGIN_NAME.to_string(),
            actions: vec![],
            metadata: DiffMetadata {
                timestamp: chrono::Utc::now().timestamp(),
                current_hash: String::new(),
                desired_hash: String::new(),
            },
        })
    }

    async fn apply_state(&self, _diff: &StateDiff) -> Result<ApplyResult> {
        Ok(ApplyResult {
            success: true,
            changes_applied: vec![],
            errors: vec![],
            checkpoint: None,
        })
    }

    async fn verify_state(&self, _desired: &Value) -> Result<bool> {
        Ok(true)
    }

    async fn create_checkpoint(&self) -> Result<Checkpoint> {
        Ok(Checkpoint {
            id: uuid::Uuid::new_v4().to_string(),
            plugin: PLUGIN_NAME.to_string(),
            timestamp: chrono::Utc::now().timestamp(),
            state_snapshot: simd_json::serde::to_owned_value(Self::current_state())?,
            backend_checkpoint: None,
        })
    }

    async fn rollback(&self, _checkpoint: &Checkpoint) -> Result<()> {
        Ok(())
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

// =============================================================================
// PLUGIN EXIT: publish the single PluginSchema contract
// =============================================================================

/// Derived `json_render` schema from the typed [`JsonRenderState`] struct via schemars.
pub(crate) fn json_render_schema() -> PluginSchema {
    let root = serde_json::to_value(schemars::schema_for!(JsonRenderState))
        .expect("schemars schema serializes to JSON");
    let mut schema = super::schemars_adapter::plugin_schema_from_json(
        PLUGIN_NAME,
        PLUGIN_VERSION,
        PLUGIN_DESCRIPTION,
        &root,
    );
    schema.category = PLUGIN_CATEGORY.to_string();
    schema.display_name = Some(PLUGIN_DISPLAY_NAME.to_string());
    let state = simd_json::serde::to_owned_value(&JsonRenderState::default())
        .expect("JsonRenderState default serializes");
    super::schemars_adapter::apply_state_defaults(&mut schema, &state);

    // Add methods
    schema.methods.insert(
        "get_config".to_string(),
        method_decl_from_schemars_with_output::<EmptyJsonRenderInput, ConfigOutput>(
            "get_config",
            SideEffect::Read,
            true,
            "json_render.read",
            "obs.software.plugin.json-render.config.get@v2",
        ),
    );
    schema.methods.insert(
        "set_config".to_string(),
        method_decl_from_schemars_with_output::<SetJsonRenderConfigInput, SetConfigOutput>(
            "set_config",
            SideEffect::Mutation,
            false,
            "json_render.invoke",
            "mut.software.plugin.json-render.config.set@v2",
        ),
    );
    schema.methods.insert(
        "get_health".to_string(),
        method_decl_from_schemars_with_output::<EmptyJsonRenderInput, HealthOutput>(
            "get_health",
            SideEffect::Read,
            true,
            "json_render.read",
            "obs.software.plugin.json-render.health@v2",
        ),
    );
    schema.methods.insert(
        "list_tools".to_string(),
        method_decl_from_schemars_with_output::<EmptyJsonRenderInput, ToolsOutput>(
            "list_tools",
            SideEffect::Read,
            true,
            "json_render.read",
            "obs.software.plugin.json-render.tool.list@v2",
        ),
    );
    schema.methods.insert(
        "list_packages".to_string(),
        method_decl_from_schemars_with_output::<EmptyJsonRenderInput, PackagesOutput>(
            "list_packages",
            SideEffect::Read,
            true,
            "json_render.read",
            "obs.software.plugin.json-render.package.list@v2",
        ),
    );
    schema.methods.insert(
        "get_package".to_string(),
        method_decl_from_schemars_with_output::<PackageLookupInput, PackageOutput>(
            "get_package",
            SideEffect::Read,
            true,
            "json_render.read",
            "obs.software.plugin.json-render.package.get@v2",
        ),
    );
    schema.methods.insert(
        "list_components".to_string(),
        method_decl_from_schemars_with_output::<EmptyJsonRenderInput, ComponentsOutput>(
            "list_components",
            SideEffect::Read,
            true,
            "json_render.read",
            "obs.software.plugin.json-render.component.list@v2",
        ),
    );
    schema.methods.insert(
        "get_component_schema".to_string(),
        method_decl_from_schemars_with_output::<ComponentLookupInput, ComponentOutput>(
            "get_component_schema",
            SideEffect::Read,
            true,
            "json_render.read",
            "obs.software.plugin.json-render.component.get@v2",
        ),
    );
    schema.methods.insert(
        "list_actions".to_string(),
        method_decl_from_schemars_with_output::<EmptyJsonRenderInput, ActionsOutput>(
            "list_actions",
            SideEffect::Read,
            true,
            "json_render.read",
            "obs.software.plugin.json-render.action.list@v2",
        ),
    );
    schema.methods.insert(
        "list_core_exports".to_string(),
        method_decl_from_schemars_with_output::<EmptyJsonRenderInput, CoreExportsOutput>(
            "list_core_exports",
            SideEffect::Read,
            true,
            "json_render.read",
            "obs.software.plugin.json-render.core-export.list@v2",
        ),
    );
    schema.methods.insert(
        "list_renderers".to_string(),
        method_decl_from_schemars_with_output::<EmptyJsonRenderInput, RenderersOutput>(
            "list_renderers",
            SideEffect::Read,
            true,
            "json_render.read",
            "obs.software.plugin.json-render.renderer.list@v2",
        ),
    );
    schema.methods.insert(
        "get_spec_schema".to_string(),
        method_decl_from_schemars_with_output::<EmptyJsonRenderInput, SchemaExportOutput>(
            "get_spec_schema",
            SideEffect::Read,
            true,
            "json_render.read",
            "obs.software.plugin.json-render.spec.get@v2",
        ),
    );
    schema.methods.insert(
        "validate_spec".to_string(),
        method_decl_from_schemars_with_output::<SpecValidationInput, ValidationResultOutput>(
            "validate_spec",
            SideEffect::Read,
            true,
            "json_render.read",
            "obs.software.plugin.json-render.spec.validate@v2",
        ),
    );
    schema.methods.insert(
        "export_json_schema".to_string(),
        method_decl_from_schemars_with_output::<SchemaExportInput, SchemaExportOutput>(
            "export_json_schema",
            SideEffect::Read,
            true,
            "json_render.read",
            "obs.software.plugin.json-render.schema.export@v2",
        ),
    );
    schema.methods.insert(
        "build_prompt_surface".to_string(),
        method_decl_from_schemars_with_output::<PromptSurfaceInput, PromptSurfaceOutput>(
            "build_prompt_surface",
            SideEffect::Read,
            true,
            "json_render.read",
            "obs.software.plugin.json-render.prompt.build@v2",
        ),
    );

    gallery::declare_methods(&mut schema);
    schema.capabilities.insert(
        "json_render.read".to_string(),
        op_state_store::CapabilityDecl {
            id: "json_render.read".to_string(),
            description: "Grants: get_config, get_health, list_tools, list_packages, get_package, list_components, get_component_schema, list_actions, list_core_exports, list_renderers, get_spec_schema, validate_spec, export_json_schema, build_prompt_surface.".to_string(),
        },
    );
    schema.capabilities.insert(
        "json_render.invoke".to_string(),
        op_state_store::CapabilityDecl {
            id: "json_render.invoke".to_string(),
            description: "Grants: set_config.".to_string(),
        },
    );

    // The local exposure policy and source pin are themselves public
    // metadata. Catalog and prompt material remains behind the plugin's
    // runtime configuration and method surface rather than being streamed
    // unconditionally.
    schema.public_state.paths = vec![
        "/status".to_string(),
        "/source".to_string(),
        "/config".to_string(),
    ];

    schema
}

fn default_packages() -> Vec<JsonRenderPackage> {
    [
        (
            "core",
            "@json-render/core",
            "0.19.0",
            "schema, catalog, spec, validation, stream, diff, merge, prompt core",
        ),
        (
            "react",
            "@json-render/react",
            "0.19.0",
            "React renderer, registry, providers, hooks",
        ),
        (
            "next",
            "@json-render/next",
            "0.19.0",
            "Next.js app renderer, routes, layouts, metadata, SSR",
        ),
        (
            "shadcn",
            "@json-render/shadcn",
            "0.19.0",
            "React shadcn/ui catalog",
        ),
        (
            "shadcn-svelte",
            "@json-render/shadcn-svelte",
            "0.19.0",
            "Svelte shadcn catalog",
        ),
        ("vue", "@json-render/vue", "0.19.0", "Vue renderer"),
        ("svelte", "@json-render/svelte", "0.19.0", "Svelte renderer"),
        ("solid", "@json-render/solid", "0.19.0", "Solid renderer"),
        (
            "react-native",
            "@json-render/react-native",
            "0.19.0",
            "React Native renderer",
        ),
        (
            "react-pdf",
            "@json-render/react-pdf",
            "0.19.0",
            "PDF renderer",
        ),
        (
            "react-email",
            "@json-render/react-email",
            "0.19.0",
            "HTML email renderer",
        ),
        (
            "image",
            "@json-render/image",
            "0.19.0",
            "SVG and PNG image renderer via Satori",
        ),
        (
            "remotion",
            "@json-render/remotion",
            "0.19.0",
            "Video composition renderer",
        ),
        ("ink", "@json-render/ink", "0.19.0", "Terminal UI renderer"),
        (
            "react-three-fiber",
            "@json-render/react-three-fiber",
            "0.19.0",
            "3D scene renderer",
        ),
        (
            "directives",
            "@json-render/directives",
            "0.19.0",
            "pre-built directive pack",
        ),
        (
            "codegen",
            "@json-render/codegen",
            "0.19.0",
            "code generation utilities",
        ),
        (
            "devtools",
            "@json-render/devtools",
            "0.19.0",
            "framework-agnostic devtools",
        ),
        (
            "devtools-react",
            "@json-render/devtools-react",
            "0.19.0",
            "React devtools adapter",
        ),
        (
            "devtools-vue",
            "@json-render/devtools-vue",
            "0.19.0",
            "Vue devtools adapter",
        ),
        (
            "devtools-svelte",
            "@json-render/devtools-svelte",
            "0.19.0",
            "Svelte devtools adapter",
        ),
        (
            "devtools-solid",
            "@json-render/devtools-solid",
            "0.19.0",
            "Solid devtools adapter",
        ),
        ("mcp", "@json-render/mcp", "0.19.0", "MCP Apps integration"),
        (
            "yaml",
            "@json-render/yaml",
            "0.19.0",
            "YAML wire format and streaming edits",
        ),
        (
            "redux",
            "@json-render/redux",
            "0.19.0",
            "Redux state adapter",
        ),
        (
            "zustand",
            "@json-render/zustand",
            "0.19.0",
            "Zustand state adapter",
        ),
        (
            "jotai",
            "@json-render/jotai",
            "0.19.0",
            "Jotai state adapter",
        ),
        (
            "xstate",
            "@json-render/xstate",
            "0.19.0",
            "XState store adapter",
        ),
    ]
    .into_iter()
    .map(|(folder, package_name, version, role)| JsonRenderPackage {
        folder: folder.to_string(),
        package_name: package_name.to_string(),
        version: version.to_string(),
        role: role.to_string(),
    })
    .collect()
}

fn default_core_exports() -> Vec<ApiExport> {
    [
        ("defineSchema", "@json-render/core", "function", "schema"),
        ("defineCatalog", "@json-render/core", "function", "catalog"),
        ("buildUserPrompt", "@json-render/core", "function", "prompt"),
        (
            "resolveDynamicValue",
            "@json-render/core",
            "function",
            "state",
        ),
        ("getByPath", "@json-render/core", "function", "state"),
        ("setByPath", "@json-render/core", "function", "state"),
        ("addByPath", "@json-render/core", "function", "state"),
        ("removeByPath", "@json-render/core", "function", "state"),
        ("createStateStore", "@json-render/core", "function", "state"),
        (
            "evaluateVisibility",
            "@json-render/core",
            "function",
            "visibility",
        ),
        (
            "conditionUsesItemScope",
            "@json-render/core",
            "function",
            "visibility",
        ),
        (
            "splitRepeatVisibility",
            "@json-render/core",
            "function",
            "visibility",
        ),
        ("resolvePropValue", "@json-render/core", "function", "props"),
        (
            "resolveElementProps",
            "@json-render/core",
            "function",
            "props",
        ),
        ("resolveBindings", "@json-render/core", "function", "props"),
        (
            "resolveActionParam",
            "@json-render/core",
            "function",
            "props",
        ),
        (
            "defineDirective",
            "@json-render/core",
            "function",
            "directives",
        ),
        (
            "createDirectiveRegistry",
            "@json-render/core",
            "function",
            "directives",
        ),
        (
            "findDirective",
            "@json-render/core",
            "function",
            "directives",
        ),
        ("resolveAction", "@json-render/core", "function", "actions"),
        ("executeAction", "@json-render/core", "function", "actions"),
        (
            "interpolateString",
            "@json-render/core",
            "function",
            "actions",
        ),
        (
            "runValidationCheck",
            "@json-render/core",
            "function",
            "validation",
        ),
        (
            "runValidation",
            "@json-render/core",
            "function",
            "validation",
        ),
        (
            "validateSpec",
            "@json-render/core",
            "function",
            "spec-validation",
        ),
        (
            "autoFixSpec",
            "@json-render/core",
            "function",
            "spec-validation",
        ),
        (
            "formatSpecIssues",
            "@json-render/core",
            "function",
            "spec-validation",
        ),
        (
            "parseSpecStreamLine",
            "@json-render/core",
            "function",
            "streaming",
        ),
        (
            "applySpecStreamPatch",
            "@json-render/core",
            "function",
            "streaming",
        ),
        (
            "applySpecPatch",
            "@json-render/core",
            "function",
            "streaming",
        ),
        ("nestedToFlat", "@json-render/core", "function", "streaming"),
        (
            "compileSpecStream",
            "@json-render/core",
            "function",
            "streaming",
        ),
        (
            "createSpecStreamCompiler",
            "@json-render/core",
            "function",
            "streaming",
        ),
        (
            "createMixedStreamParser",
            "@json-render/core",
            "function",
            "streaming",
        ),
        (
            "createJsonRenderTransform",
            "@json-render/core",
            "function",
            "streaming",
        ),
        (
            "pipeJsonRender",
            "@json-render/core",
            "function",
            "streaming",
        ),
        ("deepMergeSpec", "@json-render/core", "function", "merge"),
        ("diffToPatches", "@json-render/core", "function", "diff"),
    ]
    .into_iter()
    .map(|(name, package, kind, category)| ApiExport {
        name: name.to_string(),
        package: package.to_string(),
        kind: kind.to_string(),
        category: category.to_string(),
    })
    .collect()
}

fn default_renderers() -> Vec<RendererSurface> {
    [
        (
            "react",
            "@json-render/react",
            "React",
            vec![
                "defineRegistry",
                "createRenderer",
                "Renderer",
                "JSONUIProvider",
                "useUIStream",
                "useChatUI",
            ],
        ),
        (
            "next",
            "@json-render/next",
            "Next.js",
            vec![
                "createApp",
                "createRouter",
                "PageRenderer",
                "renderMetadata",
            ],
        ),
        (
            "vue",
            "@json-render/vue",
            "Vue",
            vec!["Renderer", "defineRegistry", "useJsonRender"],
        ),
        (
            "svelte",
            "@json-render/svelte",
            "Svelte",
            vec!["Renderer", "createRegistry"],
        ),
        (
            "solid",
            "@json-render/solid",
            "Solid",
            vec!["Renderer", "defineRegistry"],
        ),
        (
            "react-native",
            "@json-render/react-native",
            "React Native",
            vec!["Renderer", "defineRegistry"],
        ),
        (
            "react-pdf",
            "@json-render/react-pdf",
            "PDF",
            vec!["Renderer", "renderToStream", "renderToBuffer"],
        ),
        (
            "react-email",
            "@json-render/react-email",
            "Email",
            vec!["Renderer", "render"],
        ),
        (
            "image",
            "@json-render/image",
            "Image",
            vec!["renderToSvg", "renderToPng"],
        ),
        (
            "remotion",
            "@json-render/remotion",
            "Video",
            vec!["Renderer", "renderComposition"],
        ),
        ("ink", "@json-render/ink", "Terminal", vec!["Renderer"]),
        (
            "react-three-fiber",
            "@json-render/react-three-fiber",
            "3D",
            vec!["Renderer", "catalog"],
        ),
    ]
    .into_iter()
    .map(|(id, package_name, target, exports)| RendererSurface {
        id: id.to_string(),
        package_name: package_name.to_string(),
        target: target.to_string(),
        exports: exports.into_iter().map(str::to_string).collect(),
    })
    .collect()
}

fn default_components() -> Vec<ComponentDecl> {
    catalog_v019::components()
}

fn default_actions() -> Vec<ActionDecl> {
    [
        (
            "setState",
            vec!["statePath", "value"],
            true,
            "Set a state value by JSON Pointer path.",
            vec![
                RendererTarget::React,
                RendererTarget::Vue,
                RendererTarget::Svelte,
                RendererTarget::Solid,
                RendererTarget::ReactNative,
                RendererTarget::Ink,
            ],
        ),
        (
            "pushState",
            vec!["statePath", "value", "clearStatePath"],
            true,
            "Append an item to an array in state.",
            vec![
                RendererTarget::React,
                RendererTarget::Vue,
                RendererTarget::Svelte,
                RendererTarget::Solid,
                RendererTarget::ReactNative,
                RendererTarget::Ink,
            ],
        ),
        (
            "removeState",
            vec!["statePath", "index"],
            true,
            "Remove an array item by index.",
            vec![
                RendererTarget::React,
                RendererTarget::Vue,
                RendererTarget::Svelte,
                RendererTarget::Solid,
                RendererTarget::ReactNative,
                RendererTarget::Ink,
            ],
        ),
        (
            "push",
            vec!["screen"],
            true,
            "Push a screen name onto navigation state.",
            vec![RendererTarget::ReactNative],
        ),
        (
            "pop",
            vec![],
            true,
            "Pop the previous screen from navigation state.",
            vec![RendererTarget::ReactNative],
        ),
        (
            "validateForm",
            vec!["statePath"],
            true,
            "Run registered validation and write the result to state.",
            vec![
                RendererTarget::React,
                RendererTarget::Vue,
                RendererTarget::Svelte,
                RendererTarget::Solid,
            ],
        ),
    ]
    .into_iter()
    .map(
        |(name, params, mutates_state, description, renderers)| ActionDecl {
            name: name.to_string(),
            params: params.into_iter().map(str::to_string).collect(),
            params_schema: runtime_contract::builtin_action_params_schema(name),
            renderers,
            mutates_state,
            description: description.to_string(),
        },
    )
    .collect()
}

fn default_validation_checks() -> Vec<NamedCapability> {
    [
        ("required", "validation", "Require non-empty values."),
        ("email", "validation", "Validate email address shape."),
        ("minLength", "validation", "Require minimum string length."),
        ("maxLength", "validation", "Require maximum string length."),
        (
            "pattern",
            "validation",
            "Validate against a regular expression.",
        ),
        ("min", "validation", "Require minimum numeric value."),
        ("max", "validation", "Require maximum numeric value."),
        ("numeric", "validation", "Require numeric input."),
        ("url", "validation", "Validate URL shape."),
        (
            "matches",
            "validation",
            "Require equality with another value.",
        ),
        ("equalTo", "validation", "Alias for cross-field equality."),
        (
            "lessThan",
            "validation",
            "Require value less than another value.",
        ),
        (
            "greaterThan",
            "validation",
            "Require value greater than another value.",
        ),
    ]
    .into_iter()
    .map(|(name, category, description)| NamedCapability {
        name: name.to_string(),
        category: category.to_string(),
        description: description.to_string(),
    })
    .collect()
}

fn default_directives() -> Vec<NamedCapability> {
    [
        ("$state", "built-in-prop", "Resolve a value from state."),
        (
            "$item",
            "built-in-prop",
            "Resolve a value from the current repeat item.",
        ),
        (
            "$index",
            "built-in-prop",
            "Resolve the current repeat index.",
        ),
        (
            "$bindState",
            "built-in-prop",
            "Create a two-way state binding.",
        ),
        (
            "$bindItem",
            "built-in-prop",
            "Create a repeat item binding.",
        ),
        ("$cond", "built-in-prop", "Conditional prop expression."),
        ("$computed", "built-in-prop", "Computed prop expression."),
        (
            "$template",
            "built-in-prop",
            "Template string prop expression.",
        ),
        ("$format", "directive-pack", "Locale-aware formatting."),
        ("$math", "directive-pack", "Math expression helper."),
        ("$concat", "directive-pack", "Concatenate values."),
        ("$count", "directive-pack", "Count collection values."),
        ("$truncate", "directive-pack", "Truncate text."),
        ("$pluralize", "directive-pack", "Pluralize labels."),
        ("$join", "directive-pack", "Join collections."),
        ("$t", "directive-pack", "i18n translation lookup."),
    ]
    .into_iter()
    .map(|(name, category, description)| NamedCapability {
        name: name.to_string(),
        category: category.to_string(),
        description: description.to_string(),
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state_plugins::common::oscal::validate_subid;
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
    fn all_subids_are_valid() {
        let root = serde_json::to_value(schemars::schema_for!(JsonRenderState))
            .expect("schemars schema serializes to JSON");
        let mut subids = Vec::new();
        collect_subids(&root, &mut subids);
        assert!(!subids.is_empty(), "expected at least one subid");
        for subid in subids {
            validate_subid(&subid).unwrap_or_else(|error| panic!("invalid subid {subid}: {error}"));
        }
    }

    #[test]
    fn schema_declares_expected_methods() {
        let schema = json_render_schema();
        for method in [
            "get_config",
            "set_config",
            "get_health",
            "list_tools",
            "list_packages",
            "get_package",
            "list_components",
            "get_component_schema",
            "list_actions",
            "list_core_exports",
            "list_renderers",
            "get_spec_schema",
            "validate_spec",
            "export_json_schema",
            "build_prompt_surface",
        ] {
            assert!(
                schema.methods.contains_key(method),
                "missing method {method}"
            );
        }
    }
}

inventory::submit! {
    crate::default_registry::PluginReg::new(PLUGIN_NAME, |_ctx| std::sync::Arc::new(JsonRenderPlugin::new()))
}
