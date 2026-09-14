//! Shared LLM projection types used by `tched_router` and `antigravity`.
//!
//! These structs define the common schema surface that both plugins expose at
//! the top level: provider catalog, model routes, router metadata, tool
//! definitions, configuration schema, UI surfaces, and structured output
//! controls. Each struct derives `schemars::JsonSchema` so the D-Bus
//! projection is generated directly from the type.

use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

/// A single routable LLM provider.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.software.llm-provider.schema@v1"))]
pub struct Provider {
    /// Stable provider identifier.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "exp.software.llm-provider.id@v1"))]
    pub id: String,
    /// Routing tag used by the classifier.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.service.llm-provider.route@v1"))]
    pub route: String,
    /// Provider kind (local, provider, orchestrator, policy, virtual, cli, router).
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.software.llm-provider.kind@v1"))]
    pub kind: String,
    /// Recognized aliases for this provider.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "exp.software.llm-provider.aliases@v1"))]
    pub aliases: Vec<String>,
    /// Service endpoint URL, when the provider is remote.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.service.llm-provider.endpoint@v1"))]
    pub endpoint: String,
    /// Authentication method expected by the provider.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.service.llm-provider.auth@v1"))]
    pub auth: String,
    /// SDK or package name that implements this provider.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "exp.software.llm-provider.sdk@v1"))]
    pub sdk: String,
    /// Canonical OSCAL subid registry source for policy-kind providers.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "src.service.llm-provider.oscal-source@v1"))]
    pub oscal_source: String,
    /// Canonical OSCAL subid registry source for policy-kind providers.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "src.service.llm-provider.source@v1"))]
    pub source: String,
    /// Human-readable description.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "exp.software.llm-provider.description@v1"))]
    pub description: String,
}

/// A model route binds a hint to a provider and model.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.software.llm-model-route.schema@v1"))]
pub struct ModelRoute {
    /// Query hint this route satisfies (code, fast, local, reasoning, vision, ...).
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "exp.software.llm-model-route.hint@v1"))]
    pub hint: String,
    /// Selected provider for this route.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.service.llm-model-route.provider@v1"))]
    pub provider: String,
    /// Upstream provider, when the route is proxied through an orchestrator.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.service.llm-model-route.upstream-provider@v1"))]
    pub upstream_provider: String,
    /// Transport mechanism (direct, loopback, auto, dbus).
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.service.llm-model-route.transport@v1"))]
    pub transport: String,
    /// Target model identifier.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "exp.software.llm-model-route.model@v1"))]
    pub model: String,
    /// Route kind (chat, router, multimodal, embedding, policy, orchestrator).
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.software.llm-model-route.kind@v1"))]
    pub kind: String,
    /// Declared operational status.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.software.llm-model-route.status@v1"))]
    pub status: String,
    /// Whether the route is currently available.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.software.llm-model-route.available@v1"))]
    pub available: bool,
    /// Human-readable reason for the current availability status.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.software.llm-model-route.status-reason@v1"))]
    pub status_reason: String,
    /// API key placeholder, null when not configured.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.service.llm-model-route.api-key@v1"))]
    pub api_key: Option<JsonValue>,
    /// Canonical OSCAL subid registry source for policy-kind routes.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "src.service.llm-model-route.source@v1"))]
    pub source: String,
}

/// Local classifier/router configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.software.llm-router.schema@v1"))]
pub struct Router {
    /// Provider hosting the classifier.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.service.llm-router.provider@v1"))]
    pub provider: String,
    /// Classifier model.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "exp.software.llm-router.model@v1"))]
    pub model: String,
    /// Classifier endpoint URL.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.service.llm-router.endpoint@v1"))]
    pub endpoint: String,
    /// Scope of requests the router classifies.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.software.llm-router.scope@v1"))]
    pub scope: String,
    /// Router role (classifier, context_aware_allocator, ...).
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.software.llm-router.role@v1"))]
    pub role: String,
    /// Tags emitted by the router.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "exp.service.llm-router.emits@v1"))]
    pub emits: Vec<String>,
    /// Canonical OSCAL subid registry source.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "src.service.llm-router.oscal-source@v1"))]
    pub oscal_source: String,
    /// Optional classification rules mapping task kinds to routes.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "sch.software.llm-router.classification-rules@v1"))]
    pub classification_rules: JsonValue,
}

/// Declared tool exposed by the LLM surface.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.software.llm-tool.schema@v1"))]
pub struct LlmTool {
    /// Tool name, including namespace prefix.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "exp.software.llm-tool.name@v1"))]
    pub name: String,
    /// Human-readable tool description.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "exp.software.llm-tool.description@v1"))]
    pub description: String,
    /// JSON Schema for tool parameters.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "sch.standard.llm-tool.parameters@v1"))]
    pub parameters: JsonValue,
    /// Optional JSON Schema for the structured response.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "sch.standard.llm-tool.response-schema@v1"))]
    pub response_schema: JsonValue,
}

/// Configuration schema reference.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.software.llm-config-schema.schema@v1"))]
pub struct ConfigSchema {
    /// Source label for this configuration.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "src.software.llm-config-schema.source@v1"))]
    pub source: String,
    /// Crate or package that owns the native schema.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "exp.software.llm-config-schema.schema-crate@v1"))]
    pub schema_crate: String,
    /// Native Rust or Python type name.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "exp.software.llm-config-schema.native-type@v1"))]
    pub native_type: String,
    /// SDK package name (e.g. google-genai).
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "exp.software.llm-config-schema.sdk-package@v1"))]
    pub sdk_package: String,
    /// OpenAPI or schema URL.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "src.standard.llm-config-schema.openapi-url@v1"))]
    pub openapi_url: String,
    /// API version string.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.software.llm-config-schema.api-version@v1"))]
    pub api_version: String,
    /// Availability status of the configuration surface.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.software.llm-config-schema.status@v1"))]
    pub status: String,
    /// Environment variable map for the configuration.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.service.llm-config-schema.env-vars@v1"))]
    pub env_vars: JsonValue,
}

/// Weights and defaults for the pure model selector.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.software.llm-selector-policy.schema@v1"))]
pub struct SelectorPolicy {
    /// Default effort class when the caller does not request one.
    #[serde(default = "default_effort")]
    pub default_effort: String,
    /// Weight applied to effort-quality scoring.
    #[serde(default = "default_weight")]
    pub effort_weight: f64,
    /// Weight applied to cost-class scoring.
    #[serde(default = "default_weight")]
    pub cost_weight: f64,
    /// Weight applied to latency-class scoring.
    #[serde(default = "default_weight")]
    pub latency_weight: f64,
    /// Weight applied to declared route health.
    #[serde(default = "default_weight")]
    pub health_weight: f64,
}

fn default_effort() -> String {
    "medium".to_string()
}

fn default_weight() -> f64 {
    1.0
}

impl Default for SelectorPolicy {
    fn default() -> Self {
        Self {
            default_effort: default_effort(),
            effort_weight: default_weight(),
            cost_weight: default_weight(),
            latency_weight: default_weight(),
            health_weight: default_weight(),
        }
    }
}

/// A UI surface rendered from the schema.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.software.llm-ui-surface.schema@v1"))]
pub struct UiSurface {
    /// Route path.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "exp.service.llm-ui-surface.path@v1"))]
    pub path: String,
    /// Display name.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "exp.service.llm-ui-surface.name@v1"))]
    pub name: String,
    /// Schema reference.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "exp.service.llm-ui-surface.schema@v1"))]
    pub schema: String,
}

/// Rewrite shared `llm-projection` subids so they are plugin-specific.
///
/// Both `antigravity` and `tched_router` flatten the shared [`LlmProjection`] struct,
/// so without this rewrite the top-level projection subids collide. The rewrite
/// replaces the subject token `llm-projection` with `{plugin_name}-llm-projection` in
/// every top-level subid carried by the schema (nested `$defs` subids are not
/// stored in `PluginSchema.subids` and therefore are not touched here).
pub fn rewrite_projection_subids_for_plugin(
    schema: &mut op_state_store::PluginSchema,
    plugin_name: &str,
) {
    let replacement = format!("{plugin_name}-llm-projection");
    for subid in schema.subids.values_mut() {
        *subid = subid.replace("llm-projection", &replacement);
    }
}

/// Structured output configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.software.llm-structured-output.schema@v1"))]
pub struct StructuredOutput {
    /// SDK that defines the structured output contract.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "exp.software.llm-structured-output.sdk@v1"))]
    pub sdk: String,
    /// Config object signature.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "exp.software.llm-structured-output.config-object@v1"))]
    pub config_object: String,
    /// Response extractor expression.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.software.llm-structured-output.extractor@v1"))]
    pub extractor: String,
    /// Response schema object.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "sch.standard.llm-structured-output.response-schema@v1"))]
    pub response_schema: JsonValue,
    /// Pydantic source lines for the response schema.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "src.software.llm-structured-output.pydantic-source@v1"))]
    pub pydantic_source: Vec<String>,
    /// UI renderer name.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "exp.service.llm-structured-output.ui-renderer@v1"))]
    pub ui_renderer: String,
    /// Whether structured output is required.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.software.llm-structured-output.required@v1"))]
    pub required: bool,
    /// MIME type for the response.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.software.llm-structured-output.response-mime-type@v1"))]
    pub response_mime_type: String,
    /// Schema validation mode.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "sch.standard.llm-structured-output.schema-validation@v1"))]
    pub schema_validation: String,
    /// Whether refusals are returned as null.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.software.llm-structured-output.refusals-as-none@v1"))]
    pub refusals_as_none: bool,
}

/// Shared LLM projection embedded by both `tched_router` and `antigravity`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.software.llm-projection.schema@v1"))]
pub struct LlmProjection {
    /// Declared provider catalog.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.service.llm-projection.providers@v1"))]
    pub providers: Vec<Provider>,
    /// Declared model routes.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.service.llm-projection.model-routes@v1"))]
    pub model_routes: Vec<ModelRoute>,
    /// Local classifier/router.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.service.llm-projection.router@v1"))]
    pub router: Router,
    /// Declared tools.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "exp.service.llm-projection.tools@v1"))]
    pub tools: Vec<LlmTool>,
    /// Configuration schema reference.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "sch.software.llm-projection.config-schema@v1"))]
    pub config_schema: ConfigSchema,
    /// UI surfaces.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "exp.service.llm-projection.ui-surfaces@v1"))]
    pub ui_surfaces: Vec<UiSurface>,
    /// Structured output configuration.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "exp.service.llm-projection.structured-output@v1"))]
    pub structured_output: StructuredOutput,
}

#[cfg(test)]
pub mod schema_helpers {
    use crate::state_plugins::plugin_scaffold_helpers::schema_from_state;
    use crate::state_plugins::schemars_adapter::plugin_schema_from_json;
    use op_state_store::{FieldSchema, FieldType, PluginSchema};
    use serde_json::Value as JVal;
    use simd_json::OwnedValue;

    /// Build a golden `PluginSchema` from a default state value and its schemars
    /// schema JSON.
    ///
    /// The golden starts from the schemars-derived schema (so field types,
    /// descriptions, and subids match the derived contract), then overlays the
    /// default values from `schema_from_state` on the default state. This lets
    /// the golden reproduce the `schema_from_state` output shape while staying
    /// exactly equivalent to the derived schema.
    pub fn golden_from_state_and_schema(
        state: &OwnedValue,
        schema_json: &JVal,
        name: &str,
        category: &str,
        version: &str,
        description: &str,
    ) -> PluginSchema {
        // Base schema derived from schemars: correct types, descriptions, and
        // subids. The derived schema function uses the same path.
        let mut plugin_schema = plugin_schema_from_json(name, version, description, schema_json);
        plugin_schema.category = category.to_string();

        // Defaults come from `schema_from_state` on the default state. The
        // schema_from_state output carries them as examples; overlay them onto
        // the schemars base so the golden matches the derived defaults.
        let example_schema = schema_from_state(name, category, version, description, state);
        for (fname, field) in plugin_schema.fields.iter_mut() {
            if let Some(ex_field) = example_schema.fields.get(fname) {
                overlay_default(field, ex_field);
            }
        }

        // `schema_from_state` serializes default objects in struct field order;
        // the schemars-derived schema keeps properties alphabetically ordered.
        // Sort object keys in every default at every nesting level so the two
        // representations match.
        for field in plugin_schema.fields.values_mut() {
            sort_all_defaults(field);
        }

        plugin_schema
    }

    fn sort_all_defaults(field: &mut FieldSchema) {
        if let Some(default) = field.default.as_mut() {
            sort_object_keys(default);
        }
        match &mut field.field_type {
            FieldType::Object(map) => {
                for v in map.values_mut() {
                    sort_all_defaults(v);
                }
            }
            FieldType::Array(inner) => {
                sort_all_defaults_in_field_type(inner);
            }
            _ => {}
        }
    }

    fn sort_all_defaults_in_field_type(ft: &mut FieldType) {
        match ft {
            FieldType::Object(map) => {
                for v in map.values_mut() {
                    sort_all_defaults(v);
                }
            }
            FieldType::Array(inner) => {
                sort_all_defaults_in_field_type(inner);
            }
            _ => {}
        }
    }

    fn overlay_default(field: &mut FieldSchema, example_field: &FieldSchema) {
        if example_field.example.is_some() {
            field.default = example_field.example.clone();
        }
        match (&mut field.field_type, &example_field.field_type) {
            (FieldType::Object(map1), FieldType::Object(map2)) => {
                for (k, v1) in map1.iter_mut() {
                    if let Some(v2) = map2.get(k) {
                        overlay_default(v1, v2);
                    }
                }
            }
            (FieldType::Array(inner1), FieldType::Array(inner2)) => {
                overlay_default_in_field_type(inner1, inner2);
            }
            _ => {}
        }
    }

    fn overlay_default_in_field_type(ft1: &mut FieldType, ft2: &FieldType) {
        match (ft1, ft2) {
            (FieldType::Object(map1), FieldType::Object(map2)) => {
                for (k, v1) in map1.iter_mut() {
                    if let Some(v2) = map2.get(k) {
                        overlay_default(v1, v2);
                    }
                }
            }
            (FieldType::Array(inner1), FieldType::Array(inner2)) => {
                overlay_default_in_field_type(inner1, inner2);
            }
            _ => {}
        }
    }

    fn sort_object_keys(value: &mut OwnedValue) {
        // Convert to serde_json so we can sort object keys deterministically,
        // then parse back to simd_json. serde_json's Map preserves insertion
        // order, and simd_json's parser preserves JSON order, so the final
        // OwnedValue is sorted alphabetically at every nesting level.
        let json_str = simd_json::to_string(value).unwrap_or_default();
        let serde_value: serde_json::Value = serde_json::from_str(&json_str).unwrap_or_default();
        let sorted = sort_json_value_keys(serde_value);
        let mut bytes = serde_json::to_vec(&sorted).unwrap_or_default();
        *value = simd_json::to_owned_value(&mut bytes).unwrap_or_default();
    }

    fn sort_json_value_keys(value: serde_json::Value) -> serde_json::Value {
        match value {
            serde_json::Value::Object(map) => {
                let mut entries: Vec<(String, serde_json::Value)> = map.into_iter().collect();
                entries.sort_by(|a, b| a.0.cmp(&b.0));
                let sorted: serde_json::Map<String, serde_json::Value> = entries
                    .into_iter()
                    .map(|(k, v)| (k, sort_json_value_keys(v)))
                    .collect();
                serde_json::Value::Object(sorted)
            }
            serde_json::Value::Array(arr) => {
                serde_json::Value::Array(arr.into_iter().map(sort_json_value_keys).collect())
            }
            other => other,
        }
    }
}
