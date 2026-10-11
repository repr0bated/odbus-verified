//! 3tched Router route surface plugin — GB.3tched Router.
//!
//! Publishes the Antigravity-facing model and CLI routing contract through
//! `PluginSchema` so the UI can render provider/model controls from D-Bus.

#[path = "tched_router_generation.rs"]
pub mod generation;

use super::common::errors::TchedRouterError;
use super::common::llm_projection::{
    ConfigSchema, LlmProjection, LlmTool, ModelRoute, Provider, Router, StructuredOutput, UiSurface,
};
use super::plugin_scaffold_helpers::method_decl_from_schemars_with_output;
use anyhow::Result;
use async_trait::async_trait;
use op_state::{ApplyResult, Checkpoint, DiffMetadata, PluginCapabilities, StateDiff, StatePlugin};
use op_state_store::PluginSchema;
use op_state_store::{CapabilityDecl, SideEffect};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use simd_json::OwnedValue as Value;

// =============================================================================
// PLUGIN ENTRY: identity and typed schema seed
// =============================================================================

const PLUGIN_NAME: &str = "tched_router";
const PLUGIN_VERSION: &str = "4.4.1";
const PLUGIN_CATEGORY: &str = "llm";
const PLUGIN_DESCRIPTION: &str = "3tched Router — schema/RPC-native model router for Antigravity UI, CLI providers, and structured JSON output";
const PLUGIN_DISPLAY_NAME: &str = "GB.3tchedRouter";

/// Existing native gateway wire bounds, shared with exact episode persistence.
pub const MAX_CHAT_REQUEST_BYTES: usize = 1024 * 1024;
pub const MAX_CHAT_RESPONSE_BYTES: usize = 8 * 1024 * 1024;

/// Transport layer metadata for the 3tched Router plugin.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.software.3tched-router-transport.schema@v1"))]
pub struct LlmTransport {
    /// D-Bus object path served by this plugin.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.service.3tched-router-transport.dbus-object@v1"))]
    pub dbus_object: String,
    /// gRPC upstream target.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.service.3tched-router-transport.grpc-target@v1"))]
    pub grpc_target: String,
    /// Incus / WireGuard container target for xray routing. Kept as a
    /// published-schema field for backward compatibility, but threetched's LLM
    /// transport now runs on the host (xray through its runit-managed service and
    /// the gRPC-bridge via `op-grpc-bridge`); there is no per-service
    /// incus container. Defaults to the `"host"` sentinel.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.service.3tched-router-transport.incus-container@v1"))]
    pub incus_container: String,
    /// Browser-facing surface description.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "exp.service.3tched-router-transport.browser-surface@v1"))]
    pub browser_surface: String,
    /// REST aliases exposed by the bridge.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "exp.service.3tched-router-transport.rest-aliases@v1"))]
    pub rest_aliases: Vec<String>,
    /// Canonical OSCAL/subid mapping authority.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "src.service.3tched-router-transport.policy-source@v1"))]
    pub policy_source: String,
}

/// Nested per-capability model assignments.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.software.3tched-router.model-assignments.schema@v1"))]
pub struct ModelAssignments {
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.service.3tched-router.model-assignments.ovs-routing@v1"))]
    pub ovs_routing: String,
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.service.3tched-router.model-assignments.obfuscation@v1"))]
    pub obfuscation: String,
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.service.3tched-router.model-assignments.vectorization@v1"))]
    pub vectorization: String,
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.service.3tched-router.model-assignments.qdrant-retrieval@v1"))]
    pub qdrant_retrieval: String,
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.service.3tched-router.model-assignments.cozo-retrieval@v1"))]
    pub cozo_retrieval: String,
}

/// Configurable options RPC contract extracted from `operation.registration.v1`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.service.3tched-router-options.rpc.schema@v1"))]
pub struct ConfigurableOptionRpc {
    /// RPC name from RegistrationService.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "sch.service.3tched-router-options.rpc.name@v1"))]
    pub name: String,
    /// Request message type.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "sch.service.3tched-router-options.rpc.request@v1"))]
    pub request_type: String,
    /// Response message type.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "sch.service.3tched-router-options.rpc.response@v1"))]
    pub response_type: String,
    /// Read or mutation side-effect classification.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "sch.service.3tched-router-options.rpc.side-effect@v1"))]
    pub side_effect: String,
    /// Required capability for this RPC.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "sch.service.3tched-router-options.rpc.capability@v1"))]
    pub capability_id: String,
    /// OSCAL operation taxonomy key.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "sch.service.3tched-router-options.rpc.subid@v1"))]
    pub subid: String,
}

/// Configurable options message contract extracted from registration.proto.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.service.3tched-router-options.message.schema@v1"))]
pub struct ConfigurableOptionMessage {
    /// Message type name.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "sch.service.3tched-router-options.message.name@v1"))]
    pub name: String,
    /// Message fields keyed by proto field name.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "sch.service.3tched-router-options.message.fields@v1"))]
    pub fields: Vec<ConfigurableOptionField>,
}

/// Field contract extracted from configurable options proto messages.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.service.3tched-router-options.field.schema@v1"))]
pub struct ConfigurableOptionField {
    /// Field name.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "sch.service.3tched-router-options.field.name@v1"))]
    pub name: String,
    /// Proto type, including optional/repeated marker when applicable.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "sch.service.3tched-router-options.field.proto-type@v1"))]
    pub proto_type: String,
    /// JSON/schema rendering type.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "sch.service.3tched-router-options.field.json-type@v1"))]
    pub json_type: String,
    /// True when the field contains sensitive material.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "sch.service.3tched-router-options.field.sensitive@v1"))]
    pub sensitive: bool,
    /// True when this option is required by the source contract.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "sch.service.3tched-router-options.field.required@v1"))]
    pub required: bool,
}

/// Registration error code contract.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.service.3tched-router-options.error-code.schema@v1"))]
pub struct ConfigurableOptionErrorCode {
    /// Symbolic proto enum name.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "sch.service.3tched-router-options.error-code.name@v1"))]
    pub name: String,
    /// Numeric proto value.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "sch.service.3tched-router-options.error-code.value@v1"))]
    pub value: i32,
}

/// Magic-link and WireGuard registration service schema.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.service.3tched-router-options.registration-service.schema@v1"))]
pub struct RegistrationServiceSchema {
    /// Proto package name.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "sch.service.3tched-router-options.registration-service.package@v1"))]
    pub package: String,
    /// Owning proto source path.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "src.software.3tched-router-options.registration-service.proto@v1"))]
    pub source_proto: String,
    /// Service name.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "sch.service.3tched-router-options.registration-service.name@v1"))]
    pub service: String,
    /// RPC methods exposed by the registration service.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "sch.service.3tched-router-options.registration-service.rpcs@v1"))]
    pub rpcs: Vec<ConfigurableOptionRpc>,
    /// Message schemas used by those RPCs.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "sch.service.3tched-router-options.registration-service.messages@v1"))]
    pub messages: Vec<ConfigurableOptionMessage>,
    /// Error enum values.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "sch.service.3tched-router-options.registration-service.errors@v1"))]
    pub error_codes: Vec<ConfigurableOptionErrorCode>,
}

/// Identity chain declared for user container options.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.software.3tched-router-options.identity-chain.schema@v1"))]
pub struct IdentityOptions {
    /// Hardware-bound seed accepted by the user container flow.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "src.hardware.3tched-router-options.identity.mac@v1"))]
    pub mac_address_key: String,
    /// Optional shared key accepted by the user container flow.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "src.policy.3tched-router-options.identity.psk@v1"))]
    pub pre_shared_key: String,
    /// WireGuard public key identity.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "src.network.3tched-router-options.identity.wireguard-pubkey@v1"))]
    pub wireguard_pubkey: String,
    /// MCP bearer token derivation rule.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "sch.software.3tched-router-options.identity.mcp-token@v1"))]
    pub mcp_token_derivation: String,
}

/// Namespace template declared for each user container.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.software.3tched-router-options.memory-namespace.schema@v1"))]
pub struct MemoryNamespaceOption {
    /// Namespace template.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "sch.software.3tched-router-options.memory-namespace.template@v1"))]
    pub namespace_template: String,
    /// Keys owned by this namespace.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "sch.software.3tched-router-options.memory-namespace.keys@v1"))]
    pub keys: Vec<String>,
    /// Human-readable purpose.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "sch.software.3tched-router-options.memory-namespace.purpose@v1"))]
    pub purpose: String,
    /// Identity key used to bind this namespace to the container/user identity.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "sch.software.3tched-router-options.memory-namespace.identity-link@v1"))]
    pub identity_link_key: String,
}

/// User container option contract exposed through 3tched Router.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.process-procedure.3tched-router-options.user-container.schema@v1"))]
pub struct UserContainerOptions {
    /// Source script that currently materializes the user container flow.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "src.process-procedure.3tched-router-options.user-container.script@v1"))]
    pub source_script: String,
    /// User-visible container arguments.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "sch.process-procedure.3tched-router-options.user-container.arguments@v1"))]
    pub arguments: Vec<ConfigurableOptionField>,
    /// Incus container name template.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "sch.service.3tched-router-options.user-container.container-template@v1"))]
    pub container_id_template: String,
    /// Default Incus image.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "sch.service.3tched-router-options.user-container.image@v1"))]
    pub image: String,
    /// Cognitive MCP endpoint used by the user container memory flow.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "exp.service.3tched-router-options.user-container.mcp-endpoint@v1"))]
    pub cognitive_mcp_endpoint: String,
    /// Feature flags declared for the user container flow.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "sch.policy.3tched-router-options.user-container.features@v1"))]
    pub feature_flags: Vec<String>,
}

/// Privacy rules for configurable options.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.policy.3tched-router-options.privacy.schema@v1"))]
pub struct PrivacyOptions {
    /// Whether email may be written to CozoDB.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "sch.policy.3tched-router-options.privacy.email-storage@v1"))]
    pub email_storage_rule: String,
    /// Sensitive fields that must not be rendered casually.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "sch.policy.3tched-router-options.privacy.sensitive-fields@v1"))]
    pub sensitive_fields: Vec<String>,
}

/// Complete configurable options schema surface owned by 3tched Router.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.service.3tched-router-options.schema@v1"))]
pub struct ConfigurableOptions {
    /// Magic-link and WireGuard registration RPC schema.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "sch.service.3tched-router-options.registration-service@v1"))]
    pub registration_service: RegistrationServiceSchema,
    /// User container configurable option schema.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "sch.process-procedure.3tched-router-options.user-container@v1"))]
    pub user_container: UserContainerOptions,
    /// Identity chain schema.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "sch.software.3tched-router-options.identity-chain@v1"))]
    pub identity_chain: IdentityOptions,
    /// Memory namespace templates.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "sch.software.3tched-router-options.memory-namespaces@v1"))]
    pub memory_namespaces: Vec<MemoryNamespaceOption>,
    /// Privacy handling rules.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "sch.policy.3tched-router-options.privacy@v1"))]
    pub privacy_policy: PrivacyOptions,
}

/// Top-level 3tched Router state.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.software.plugin.3tched-router.schema@v1"))]
#[schemars(extend("x-oscal-category" = "llm"))]
pub struct TchedRouterState {
    /// Narrow reflected model binding, published by authoritative native observation.
    /// Read-only and excluded from the public state projection.
    #[serde(default)]
    pub catalog_model_binding: Option<ResolveCatalogModelBindingOutput>,
    /// Owner-private generation document revision; excluded from public_state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(extend("x-oscal-subid" = "obs.software.tched-router.generation-revision@v1"))]
    pub generation_revision: Option<u64>,
    /// Native response text committed for one verified principal and session.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(extend("x-oscal-subid" = "obs.software.tched-router.generation-jobs@v1"))]
    pub generation_jobs: Option<std::collections::BTreeMap<String, generation::GenerationJob>>,
    /// Operational status.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.software.3tched-router.status@v1"))]
    pub status: String,
    /// Selected provider identifier.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.service.3tched-router.selected-provider@v1"))]
    pub selected_provider: String,
    /// Selected model identifier.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "exp.service.3tched-router.selected-model@v1"))]
    pub selected_model: String,
    /// Exact masked native v0.8.4 configuration observed through the loopback
    /// gateway. It remains in protected committed SHM and is never part of the
    /// shared public-state policy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(extend("x-oscal-subid" = "obs.software.3tched-router.native-config@v1"))]
    pub native_config: Option<JsonValue>,
    /// Per-capability model assignments.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.service.3tched-router.model-assignments@v1"))]
    pub model_assignments: ModelAssignments,
    /// Transport layer metadata.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.service.3tched-router.transport@v1"))]
    pub transport: LlmTransport,
    /// Magic-link and WireGuard registration RPC schema (a plain configuration).
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "sch.service.3tched-router.registration-service@v1"))]
    pub registration_service: RegistrationServiceSchema,
    /// User container configurable option schema (a plain configuration).
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "sch.process-procedure.3tched-router.user-container@v1"))]
    pub user_container: UserContainerOptions,
    /// Identity chain schema (a plain configuration).
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "sch.software.3tched-router.identity-chain@v1"))]
    pub identity_chain: IdentityOptions,
    /// Memory namespace templates (a plain configuration).
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "sch.software.3tched-router.memory-namespaces@v1"))]
    pub memory_namespaces: Vec<MemoryNamespaceOption>,
    /// Privacy handling rules (a plain configuration).
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "sch.policy.3tched-router.privacy@v1"))]
    pub privacy_policy: PrivacyOptions,
    /// Declared LLM catalog fields — flattened to the top level. This is
    /// DECLARED state, not a runtime projection: there is no projection.
    #[serde(flatten)]
    #[schemars(extend("x-oscal-subid" = "sch.software.3tched-router.llm-catalog@v1"))]
    pub catalog: LlmProjection,
    /// Pure selector weights and default effort class.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "sch.software.3tched-router.selector-policy@v1"))]
    pub selector_policy: super::common::llm_projection::SelectorPolicy,
}

/// Empty input for read-only 3tched Router methods.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct EmptyTchedInput {}

/// Read the dedicated route selected by the verified caller's persisted mode.
/// No agent, identity, profile or configuration path can be supplied.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetOwnAgentRouteInput {}

/// Credential-free native route observation for the caller's own selected role.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetOwnAgentRouteOutput {
    pub mode: super::project::ChatMode,
    pub reference: String,
    pub model: String,
    /// Explicit HTTPS inference endpoint without credentials, query or fragment.
    pub uri: String,
}

/// Resolve a catalog model against allowed native ordinary-selection profiles.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ResolveCatalogModelBindingInput {
    pub provider_id: String,
    pub model_id: String,
}

/// A verified credential-free native profile for one current catalog model.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ResolveCatalogModelBindingOutput {
    pub provider_id: String,
    pub model_id: String,
    pub reference: String,
    pub model: String,
    pub uri: String,
    /// Verified OpenAI subscription profile; its native auth owner chooses the
    /// inference origin. This is not inferred from an empty URI or catalog label.
    #[serde(default)]
    pub oauth: bool,
    pub catalog_revision: u64,
}

/// Input for resolving a model route by hint or model identifier.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ResolveRouteInput {
    /// Route hint or model identifier.
    pub hint: String,
}

/// A chat message carried by the schema-declared 3tched Router method.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct TchedChatMessage {
    /// OpenAI-compatible message role such as `system`, `user`, or `assistant`.
    pub role: String,
    /// Text content carried by this conversation turn.
    pub content: String,
}

/// Input for the bridge-owned 3tched Router chat dispatcher.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ChatInput {
    /// Compatibility form for callers sending a single user turn.
    #[serde(default)]
    pub message: String,
    /// Full ordered conversation. When non-empty this takes precedence over
    /// `message`.
    #[serde(default)]
    pub messages: Vec<TchedChatMessage>,
    /// Provider id, route, or alias. Empty uses `selected_provider`.
    #[serde(default)]
    pub provider: String,
    /// Model id or route hint. Empty uses `selected_model`.
    #[serde(default)]
    pub model: String,
    /// Optional caller correlation label. This is scoped by the verified
    /// principal/session at the ChatService boundary and is never identity.
    #[serde(default)]
    pub conversation_id: Option<String>,
}

/// Input for listing model routes, optionally filtered by provider.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ListModelsInput {
    /// Optional provider id, route, or alias used to filter the model catalog.
    #[serde(default)]
    pub provider: String,
}

/// Input for selecting a provider.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SetProviderInput {
    /// Provider identifier declared in the provider catalog.
    pub provider_id: String,
}

/// Input for selecting a model.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SetModelInput {
    /// Model identifier declared in the model route catalog.
    pub model_id: String,
}

/// Runtime fields observed together from the native tched_router backend.
///
/// Configuration-only fields in [`TchedRouterState`] are deliberately absent:
/// a runtime observation cannot prove their values, and a fresh read must not
/// substitute the plugin schema's example/default state.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("x-oscal-subid" = "obs.service.3tched-router.runtime-state@v2"))]
pub struct ObservedTchedRouterState {
    pub status: String,
    pub selected_provider: String,
    pub selected_model: String,
    pub providers: Vec<Provider>,
    pub model_routes: Vec<ModelRoute>,
}

impl From<&TchedRouterState> for ObservedTchedRouterState {
    fn from(state: &TchedRouterState) -> Self {
        Self {
            status: state.status.clone(),
            selected_provider: state.selected_provider.clone(),
            selected_model: state.selected_model.clone(),
            providers: state.catalog.providers.clone(),
            model_routes: state.catalog.model_routes.clone(),
        }
    }
}

/// Output for the complete native runtime observation.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("x-oscal-subid" = "obs.service.3tched-router.state.result@v2"))]
pub struct GetStateOutput {
    /// Complete set of fields read by the native runtime adapter.
    pub state: ObservedTchedRouterState,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "exp.service.3tched-router.model-routes.result@v1"))]
pub struct GetModelRoutesOutput {
    /// Declared model routes.
    pub model_routes: Vec<ModelRoute>,
}

/// Output for the provider catalog.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "exp.service.3tched-router.provider-catalog.result@v1"))]
pub struct GetProviderCatalogOutput {
    /// Declared providers.
    pub providers: Vec<Provider>,
}

/// Output for the declared tool catalog.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "exp.service.3tched-router.tools.result@v1"))]
pub struct GetToolsOutput {
    /// Declared LLM tools.
    pub tools: Vec<LlmTool>,
}

/// Output for the provider list accessor.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "obs.service.3tched-router.providers.result@v1"))]
pub struct ListProvidersOutput {
    pub providers: Vec<Provider>,
}

/// Output for the router accessor.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "obs.service.3tched-router.router.result@v1"))]
pub struct GetRouterOutput {
    pub router: Router,
}

/// Output for the config schema accessor.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "obs.service.3tched-router.config-schema.result@v1"))]
pub struct GetConfigSchemaOutput {
    /// Exact JSON Schema for the native configuration value, selected from
    /// this same manifest-pinned plugin contract.
    pub config_schema: JsonValue,
}

// Plugin-owned v2 contracts for the six retained config_* methods.

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ConfigListInput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefix: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ConfigListOutput {
    pub entries: Vec<ConfigListEntry>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub drifted: Vec<ConfigDriftEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ConfigListEntry {
    pub path: String,
    pub category: String,
    pub kind: String,
    pub type_hint: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    pub populated: bool,
    pub is_secret: bool,
    #[serde(default)]
    pub is_env_overridden: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub enum_variants: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section: Option<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub tab: String,
    #[serde(default)]
    pub multiline: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ConfigDriftEntry {
    pub path: String,
    #[serde(default)]
    pub secret: bool,
    pub drifted: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub in_memory_value: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_disk_value: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ConfigGetInput {
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ConfigPropertyOutput {
    pub kind: ConfigPropertyKind,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub populated: Option<bool>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<ConfigValidationWarning>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ConfigPropertyKind {
    Value,
    Secret,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ConfigValidationWarning {
    pub code: String,
    pub message: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ConfigSetInput {
    pub path: String,
    pub value: JsonValue,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "op", rename_all = "lowercase", deny_unknown_fields)]
pub enum ConfigPatchOperation {
    Add { path: String, value: JsonValue },
    Remove { path: String },
    Replace { path: String, value: JsonValue },
    Test { path: String, value: JsonValue },
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ConfigPatchInput {
    pub operations: Vec<ConfigPatchOperation>,
}

/// Output for `GetMcpConfig` — the current `mcp` configuration, observed
/// live from the pinned runtime. Secrets masked by dispatch; the schema
/// rides the sealed `native_config` field, not this receipt.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct GetMcpConfigOutput {
    /// Current `mcp` configuration.
    pub config: JsonValue,
}

/// Output for `GetConfig` — the full configuration, observed live from the
/// pinned runtime and committed under the exact seal. Secrets are masked by
/// dispatch; the config's schema rides the sealed `native_config` field, not
/// this receipt.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct GetConfigOutput {
    /// Full configuration.
    pub config: JsonValue,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ConfigPatchOutput {
    pub saved: bool,
    pub results: Vec<ConfigPatchResult>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<ConfigValidationWarning>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ConfigPatchResult {
    pub op: String,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<ConfigDisplayValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub populated: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(untagged)]
pub enum ConfigDisplayValue {
    Text(String),
    Reset(()),
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ConfigInitInput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ConfigInitOutput {
    pub initialized: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ConfigMigrateInput {}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ConfigMigrateOutput {
    pub migrated: bool,
    pub schema_version: u32,
}

/// Output for the UI surfaces accessor.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "obs.service.3tched-router.ui-surfaces.result@v1"))]
pub struct ListUiSurfacesOutput {
    pub ui_surfaces: Vec<UiSurface>,
}

/// Output for the structured output accessor.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "obs.service.3tched-router.structured-output.result@v1"))]
pub struct GetStructuredOutputOutput {
    pub structured_output: StructuredOutput,
}

/// Output for a resolved model route.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "exp.service.3tched-router.route.result@v1"))]
pub struct ResolveRouteOutput {
    /// Resolved route.
    pub route: ModelRoute,
}

/// Output from the bridge-owned 3tched Router chat dispatcher.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "exp.service.3tched-router.chat.result@v1"))]
pub struct ChatOutput {
    /// Assistant response text.
    pub content: String,
    /// Upstream provider selected when the A2A request was submitted.
    /// A2A does not report attribution for an upstream fallback.
    pub provider: String,
    /// Upstream model selected when the A2A request was submitted.
    pub model: String,
    /// Provider finish reason, or an empty string when none was supplied.
    pub finish_reason: String,
    /// Provider-specific token usage object.
    pub usage: JsonValue,
}

/// Schema-declared model listing output.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "obs.service.3tched-router.models.result@v1"))]
pub struct ListModelsOutput {
    /// Schema-declared model routes, optionally filtered by provider.
    pub model_routes: Vec<ModelRoute>,
}

/// Output for provider selection.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "mut.service.3tched-router.selected-provider.result@v1"))]
pub struct SetProviderOutput {
    /// Selected provider identifier.
    pub selected_provider: String,
}

/// Output for model selection.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "mut.service.3tched-router.selected-model.result@v1"))]
pub struct SetModelOutput {
    /// Selected model identifier.
    pub selected_model: String,
}

/// Input for selecting provider and model together (dashboard picker).
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SetSelectionInput {
    pub provider_id: String,
    pub model_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "mut.service.tched-router.selection.set.result@v1"))]
pub struct SetSelectionOutput {
    pub selected_provider: String,
    pub selected_model: String,
}

/// Input for setting an ovs routing model.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SetOvsRoutingModelInput {
    pub model_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SetOvsRoutingModelOutput {
    pub ovs_routing: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SetObfuscationModelInput {
    pub model_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SetObfuscationModelOutput {
    pub obfuscation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SetVectorizationModelInput {
    pub model_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SetVectorizationModelOutput {
    pub vectorization: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SetQdrantRetrievalModelInput {
    pub model_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SetQdrantRetrievalModelOutput {
    pub qdrant_retrieval: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SetCozoRetrievalModelInput {
    pub model_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SetCozoRetrievalModelOutput {
    pub cozo_retrieval: String,
}

// =============================================================================
// PLUGIN BODY: D-Bus-backed behavior only
// =============================================================================

pub struct TchedRouterPlugin;

impl Default for TchedRouterPlugin {
    fn default() -> Self {
        Self
    }
}

impl TchedRouterPlugin {
    const DBUS_OBJECT: &'static str = "/org/opdbus/v1/plugins/tched_router";
    const OSCAL_SUBID_REGISTRY_OBJECT: &'static str = "/org/opdbus/v1/plugins/oscal_subid_registry";
    const DEFAULT_CHAT_MODEL: &'static str = "x-preview-f-free";

    pub fn new() -> Self {
        Self
    }

    fn env_or(key: &str, fallback: &str) -> String {
        std::env::var(key).unwrap_or_else(|_| fallback.to_string())
    }

    pub fn current_state() -> TchedRouterState {
        let selected_provider = Self::env_or("LLM_PROVIDER", "opencode");
        let selected_model = Self::env_or("LLM_MODEL", Self::DEFAULT_CHAT_MODEL);
        let chat_model = selected_model.clone();
        let router_endpoint = Self::env_or("TCHED_ROUTER_ENDPOINT", "http://localhost:11434");
        let grpc_target = Self::env_or("TCHED_ROUTER_GRPC_TARGET", "https://127.0.0.1:8090");
        let grpc_target_for_provider = grpc_target.clone();

        let co = Self::configurable_options();
        TchedRouterState {
            generation_revision: None,
            generation_jobs: None,
            status: "declared".to_string(),
            selected_provider: selected_provider.clone(),
            selected_model,
            native_config: None,
            catalog_model_binding: None,
            model_assignments: ModelAssignments {
                ovs_routing: Self::DEFAULT_CHAT_MODEL.to_string(),
                obfuscation: Self::DEFAULT_CHAT_MODEL.to_string(),
                vectorization: "gemini-embedding-001".to_string(),
                qdrant_retrieval: "gemini-embedding-001".to_string(),
                cozo_retrieval: Self::DEFAULT_CHAT_MODEL.to_string(),
            },
            transport: LlmTransport {
                dbus_object: Self::DBUS_OBJECT.to_string(),
                grpc_target: grpc_target_for_provider,
                incus_container: "host".to_string(),
                browser_surface: "gRPC-Web through op-web".to_string(),
                rest_aliases: vec![
                    "/api/tched-router/chat".to_string(),
                    "/api/llm/chat".to_string(),
                ],
                policy_source: Self::OSCAL_SUBID_REGISTRY_OBJECT.to_string(),
            },
            registration_service: co.registration_service,
            user_container: co.user_container,
            identity_chain: co.identity_chain,
            memory_namespaces: co.memory_namespaces,
            privacy_policy: co.privacy_policy,
            catalog: LlmProjection {
                providers: vec![
                    Provider {
                        id: "factory".to_string(),
                        route: "factory".to_string(),
                        kind: "provider".to_string(),
                        ..Default::default()
                    },
                    Provider {
                        id: "codex".to_string(),
                        route: "openai-codex".to_string(),
                        kind: "provider".to_string(),
                        aliases: vec!["openai_codex".to_string(), "codex".to_string()],
                        ..Default::default()
                    },
                    Provider {
                        id: "ollama".to_string(),
                        route: "ollama".to_string(),
                        kind: "local".to_string(),
                        aliases: vec![
                            "gemma".to_string(),
                            "gemma3".to_string(),
                            "gemma3:4b".to_string(),
                            "gemma4".to_string(),
                        ],
                        ..Default::default()
                    },
                    Provider {
                        id: "antigravity".to_string(),
                        route: "antigravity".to_string(),
                        kind: "orchestrator".to_string(),
                        aliases: vec!["google.antigravity".to_string()],
                        ..Default::default()
                    },
                    Provider {
                        id: "google".to_string(),
                        route: "gemini".to_string(),
                        kind: "provider".to_string(),
                        aliases: vec!["gemini".to_string(), "google-gemini".to_string()],
                        ..Default::default()
                    },
                    Provider {
                        id: "opencode".to_string(),
                        route: "custom:opencode".to_string(),
                        kind: "cli".to_string(),
                        aliases: vec![
                            "opencode.go".to_string(),
                            "deepseek-v4-flash-free".to_string(),
                        ],
                        ..Default::default()
                    },
                    Provider {
                        id: "kilocode".to_string(),
                        route: "custom:kilocode".to_string(),
                        kind: "cli".to_string(),
                        aliases: vec!["kilo-code".to_string(), "kilo_code".to_string()],
                        ..Default::default()
                    },
                    Provider {
                        id: "openrouter".to_string(),
                        route: "openrouter".to_string(),
                        kind: "router".to_string(),
                        aliases: vec![],
                        ..Default::default()
                    },
                    Provider {
                        id: "salad".to_string(),
                        route: "salad".to_string(),
                        kind: "provider".to_string(),
                        aliases: vec!["salad-ai".to_string(), "salad_ai".to_string()],
                        endpoint: "https://ai.salad.cloud/v1".to_string(),
                        auth: "bearer".to_string(),
                        sdk: "openai-compatible".to_string(),
                        description: "Salad AI Gateway — managed OpenAI-compatible LLM API on SaladCloud GPUs.".to_string(),
                        ..Default::default()
                    },
                    Provider {
                        id: "anthropic".to_string(),
                        route: "anthropic".to_string(),
                        kind: "provider".to_string(),
                        aliases: vec![],
                        ..Default::default()
                    },
                    Provider {
                        id: "openai".to_string(),
                        route: "openai".to_string(),
                        kind: "provider".to_string(),
                        aliases: vec![],
                        ..Default::default()
                    },
                    Provider {
                        id: "gemini-cli".to_string(),
                        route: "gemini-cli".to_string(),
                        kind: "cli".to_string(),
                        aliases: vec![],
                        ..Default::default()
                    },
                    Provider {
                        id: "oscal".to_string(),
                        route: "oscal".to_string(),
                        kind: "policy".to_string(),
                        aliases: vec![
                            "compliance".to_string(),
                            "subid".to_string(),
                            "nist".to_string(),
                        ],
                        source: Self::OSCAL_SUBID_REGISTRY_OBJECT.to_string(),
                        ..Default::default()
                    },
                ],
                router: Router {
                    provider: selected_provider.clone(),
                    model: chat_model.clone(),
                    endpoint: router_endpoint,
                    scope: "all".to_string(),
                    role: "context_aware_allocator".to_string(),
                    emits: vec![
                        "provider".to_string(),
                        "model".to_string(),
                        "hint".to_string(),
                        "candidate_subids".to_string(),
                        "confidence".to_string(),
                        "thinking_budget".to_string(),
                        "reasoning_effort".to_string(),
                    ],
                    ..Default::default()
                },
                model_routes: vec![
                    ModelRoute {
                        hint: "code".to_string(),
                        provider: "openrouter".to_string(),
                        upstream_provider: "openrouter".to_string(),
                        transport: "direct".to_string(),
                        model: "anthropic/claude-sonnet-4.6".to_string(),
                        kind: "chat".to_string(),
                        status: "declared".to_string(),
                        available: false,
                        status_reason: "Declared route; backend availability must be projected before execution.".to_string(),
                        api_key: Some(JsonValue::Null),
                        ..Default::default()
                    },
                    ModelRoute {
                        hint: "fast".to_string(),
                        provider: "gemini".to_string(),
                        upstream_provider: "gemini".to_string(),
                        transport: "direct".to_string(),
                        model: "gemini-2.5-flash".to_string(),
                        kind: "chat".to_string(),
                        status: "declared".to_string(),
                        available: false,
                        status_reason: "Declared route; backend availability must be projected before execution.".to_string(),
                        api_key: Some(JsonValue::Null),
                        ..Default::default()
                    },
                    ModelRoute {
                        hint: "local".to_string(),
                        provider: "opencode".to_string(),
                        upstream_provider: "opencode".to_string(),
                        transport: "tched-router-loopback".to_string(),
                        model: chat_model.clone(),
                        kind: "router".to_string(),
                        status: "declared".to_string(),
                        available: false,
                        status_reason: format!(
                            "{chat_model} is the declared OpenCode chat model; availability is projected by the 3tched Router runtime."
                        ),
                        api_key: Some(JsonValue::Null),
                        ..Default::default()
                    },
                    ModelRoute {
                        hint: "reasoning".to_string(),
                        provider: "antigravity".to_string(),
                        upstream_provider: "antigravity".to_string(),
                        transport: "direct".to_string(),
                        model: "gemini-3-pro-preview".to_string(),
                        kind: "chat".to_string(),
                        status: "declared".to_string(),
                        available: false,
                        status_reason: "Declared reasoning route; backend availability must be projected before execution.".to_string(),
                        api_key: Some(JsonValue::Null),
                        ..Default::default()
                    },
                    ModelRoute {
                        hint: "compliance".to_string(),
                        provider: "oscal".to_string(),
                        upstream_provider: "oscal".to_string(),
                        transport: "dbus".to_string(),
                        model: "NIST_SP_800-53_Rev5".to_string(),
                        kind: "policy".to_string(),
                        status: "declared".to_string(),
                        available: false,
                        status_reason: "OSCAL policy provider is declared; unavailable until oscal_subid_registry is projected.".to_string(),
                        source: Self::OSCAL_SUBID_REGISTRY_OBJECT.to_string(),
                        api_key: Some(JsonValue::Null),
                        ..Default::default()
                    },
                    ModelRoute {
                        hint: "reasoning".to_string(),
                        provider: "salad".to_string(),
                        upstream_provider: "salad".to_string(),
                        transport: "direct".to_string(),
                        model: "qwen3.6-35b-a3b".to_string(),
                        kind: "chat".to_string(),
                        status: "declared".to_string(),
                        available: false,
                        status_reason: "Salad qwen3.6-35b-a3b; requires SALAD_API_KEY and backend projection.".to_string(),
                        api_key: Some(JsonValue::Null),
                        ..Default::default()
                    },
                    ModelRoute {
                        hint: "balanced".to_string(),
                        provider: "salad".to_string(),
                        upstream_provider: "salad".to_string(),
                        transport: "direct".to_string(),
                        model: "qwen3.6-27b".to_string(),
                        kind: "chat".to_string(),
                        status: "declared".to_string(),
                        available: false,
                        status_reason: "Salad qwen3.6-27b; requires SALAD_API_KEY and backend projection.".to_string(),
                        api_key: Some(JsonValue::Null),
                        ..Default::default()
                    },
                    ModelRoute {
                        hint: "fast".to_string(),
                        provider: "salad".to_string(),
                        upstream_provider: "salad".to_string(),
                        transport: "direct".to_string(),
                        model: "qwen3.5-9b".to_string(),
                        kind: "chat".to_string(),
                        status: "declared".to_string(),
                        available: false,
                        status_reason: "Salad qwen3.5-9b; requires SALAD_API_KEY and backend projection.".to_string(),
                        api_key: Some(JsonValue::Null),
                        ..Default::default()
                    },
                ],
                tools: vec![
                    LlmTool {
                        name: "3tched-router.chat".to_string(),
                        description: "Send an Antigravity/3tched Router chat turn and return structured JSON when response_schema is present.".to_string(),
                        parameters: serde_json::json!({
                            "type": "object",
                            "properties": {
                                "message": {"type": "string"},
                                "provider": {"type": "string"},
                                "model": {"type": "string"},
                                "response_schema": {"type": "object"}
                            },
                            "required": ["message"]
                        }),
                        ..Default::default()
                    },
                    LlmTool {
                        name: "3tched-router.models.list".to_string(),
                        description: "List cached or live models for a 3tched Router provider.".to_string(),
                        parameters: serde_json::json!({
                            "type": "object",
                            "properties": {"provider": {"type": "string"}},
                            "required": []
                        }),
                        ..Default::default()
                    },
                ],
                config_schema: ConfigSchema {
                    source: "threetched config schema".to_string(),
                    schema_crate: "schemars".to_string(),
                    native_type: format!("threetched config schema v{}", tched_router_binary_version()),
                    status: "available_via_cli_or_gateway".to_string(),
                    ..Default::default()
                },
                ui_surfaces: vec![
                    UiSurface {
                        path: "/grpc".to_string(),
                        name: "gRPC Diagnostics".to_string(),
                        schema: "plugin-service".to_string(),
                    },
                    UiSurface {
                        path: "/models".to_string(),
                        name: "Routable Models".to_string(),
                        schema: "tched_router.providers".to_string(),
                    },
                ],
                structured_output: StructuredOutput {
                    sdk: "google.antigravity".to_string(),
                    config_object: "LocalAgentConfig(response_schema=UiResponseSchema)".to_string(),
                    extractor: "response.structured_output()".to_string(),
                    response_schema: serde_json::json!({
                        "action": "",
                        "metadata": {},
                        "confidence_score": 0.0
                    }),
                    pydantic_source: vec![
                        "import pydantic".to_string(),
                        "class UiResponseSchema(pydantic.BaseModel):".to_string(),
                        "    action: str".to_string(),
                        "    metadata: dict[str, str]".to_string(),
                        "    confidence_score: float".to_string(),
                    ],
                    ui_renderer: "JsonRenderer".to_string(),
                    required: true,
                    ..Default::default()
                },
            },
            selector_policy: Default::default(),
        }
    }

    fn option_field(
        name: &str,
        proto_type: &str,
        json_type: &str,
        sensitive: bool,
        required: bool,
    ) -> ConfigurableOptionField {
        ConfigurableOptionField {
            name: name.to_string(),
            proto_type: proto_type.to_string(),
            json_type: json_type.to_string(),
            sensitive,
            required,
        }
    }

    fn option_message(
        name: &str,
        fields: Vec<ConfigurableOptionField>,
    ) -> ConfigurableOptionMessage {
        ConfigurableOptionMessage {
            name: name.to_string(),
            fields,
        }
    }

    fn option_rpc(
        name: &str,
        request_type: &str,
        response_type: &str,
        side_effect: &str,
        capability_id: &str,
        subid: &str,
    ) -> ConfigurableOptionRpc {
        ConfigurableOptionRpc {
            name: name.to_string(),
            request_type: request_type.to_string(),
            response_type: response_type.to_string(),
            side_effect: side_effect.to_string(),
            capability_id: capability_id.to_string(),
            subid: subid.to_string(),
        }
    }

    fn memory_namespace(
        namespace_template: &str,
        keys: &[&str],
        purpose: &str,
    ) -> MemoryNamespaceOption {
        MemoryNamespaceOption {
            namespace_template: namespace_template.to_string(),
            keys: keys.iter().map(|key| (*key).to_string()).collect(),
            purpose: purpose.to_string(),
            identity_link_key: "container:{container_id}:identity.wireguard_pubkey".to_string(),
        }
    }

    fn configurable_options() -> ConfigurableOptions {
        ConfigurableOptions {
            registration_service: RegistrationServiceSchema {
                package: "operation.registration.v1".to_string(),
                source_proto: "crates/op-grpc-bridge/proto/registration.proto".to_string(),
                service: "RegistrationService".to_string(),
                rpcs: vec![
                    Self::option_rpc(
                        "SendMagicLink",
                        "SendMagicLinkRequest",
                        "SendMagicLinkResponse",
                        "Mutation",
                        "cap.software.3tched-router.registration.magic-link.send@v1",
                        "mut.service.3tched-router.registration.magic-link.send@v1",
                    ),
                    Self::option_rpc(
                        "VerifyMagicLink",
                        "VerifyMagicLinkRequest",
                        "VerifyMagicLinkResponse",
                        "Mutation",
                        "cap.software.3tched-router.registration.magic-link.verify@v1",
                        "mut.service.3tched-router.registration.magic-link.verify@v1",
                    ),
                    Self::option_rpc(
                        "RegisterUser",
                        "RegisterUserRequest",
                        "RegisterUserResponse",
                        "Mutation",
                        "cap.software.3tched-router.registration.user.register@v1",
                        "mut.service.3tched-router.registration.user.register@v1",
                    ),
                    Self::option_rpc(
                        "GetUserStatus",
                        "GetUserStatusRequest",
                        "GetUserStatusResponse",
                        "Read",
                        "cap.software.3tched-router.registration.user-status.read@v1",
                        "obs.service.3tched-router.registration.user-status.get@v1",
                    ),
                    Self::option_rpc(
                        "ListUsers",
                        "ListUsersRequest",
                        "ListUsersResponse",
                        "Read",
                        "cap.software.3tched-router.registration.users.read@v1",
                        "obs.service.3tched-router.registration.users.list@v1",
                    ),
                    Self::option_rpc(
                        "GetWireGuardConfig",
                        "GetWireGuardConfigRequest",
                        "GetWireGuardConfigResponse",
                        "Read",
                        "cap.software.3tched-router.registration.wireguard-config.read@v1",
                        "obs.service.3tched-router.registration.wireguard-config.get@v1",
                    ),
                    Self::option_rpc(
                        "AdminUserAction",
                        "AdminUserActionRequest",
                        "AdminUserActionResponse",
                        "Mutation",
                        "cap.software.3tched-router.registration.admin-user-action.apply@v1",
                        "mut.service.3tched-router.registration.admin-user-action.apply@v1",
                    ),
                ],
                messages: vec![
                    Self::option_message(
                        "SendMagicLinkRequest",
                        vec![
                            Self::option_field("email", "string", "string", false, true),
                            Self::option_field("domain", "string", "string", false, true),
                            Self::option_field("is_admin", "bool", "boolean", false, true),
                            Self::option_field("custom_message", "optional string", "string", false, false),
                        ],
                    ),
                    Self::option_message(
                        "SendMagicLinkResponse",
                        vec![
                            Self::option_field("success", "bool", "boolean", false, false),
                            Self::option_field("message", "string", "string", false, false),
                            Self::option_field("token", "optional string", "string", true, false),
                            Self::option_field(
                                "expires_at",
                                "google.protobuf.Timestamp",
                                "string:date-time",
                                false,
                                false,
                            ),
                        ],
                    ),
                    Self::option_message(
                        "VerifyMagicLinkRequest",
                        vec![
                            Self::option_field("token", "string", "string", true, true),
                            Self::option_field("domain", "string", "string", false, true),
                        ],
                    ),
                    Self::option_message(
                        "VerifyMagicLinkResponse",
                        vec![
                            Self::option_field("success", "bool", "boolean", false, false),
                            Self::option_field("user_id", "string", "string", false, false),
                            Self::option_field("email", "string", "string", false, false),
                            Self::option_field("wireguard_public_key", "string", "string", true, false),
                            Self::option_field("assigned_ip", "string", "string", false, false),
                            Self::option_field("wireguard_config", "string", "string", true, false),
                            Self::option_field("message", "string", "string", false, false),
                            Self::option_field("is_admin", "bool", "boolean", false, false),
                            Self::option_field(
                                "verified_at",
                                "google.protobuf.Timestamp",
                                "string:date-time",
                                false,
                                false,
                            ),
                        ],
                    ),
                    Self::option_message(
                        "RegisterUserRequest",
                        vec![
                            Self::option_field("email", "string", "string", false, true),
                            Self::option_field("wireguard_public_key", "string", "string", true, true),
                            Self::option_field("domain", "string", "string", false, true),
                            Self::option_field("is_admin", "bool", "boolean", false, true),
                            Self::option_field(
                                "metadata",
                                "optional google.protobuf.Struct",
                                "object",
                                false,
                                false,
                            ),
                        ],
                    ),
                    Self::option_message(
                        "RegisterUserResponse",
                        vec![
                            Self::option_field("success", "bool", "boolean", false, false),
                            Self::option_field("user_id", "string", "string", false, false),
                            Self::option_field("message", "string", "string", false, false),
                            Self::option_field("assigned_ip", "string", "string", false, false),
                            Self::option_field("wireguard_config", "string", "string", true, false),
                            Self::option_field(
                                "registered_at",
                                "google.protobuf.Timestamp",
                                "string:date-time",
                                false,
                                false,
                            ),
                        ],
                    ),
                    Self::option_message(
                        "GetUserStatusRequest",
                        vec![
                            Self::option_field("email", "string", "string", false, false),
                            Self::option_field("user_id", "string", "string", false, false),
                            Self::option_field("domain", "string", "string", false, false),
                        ],
                    ),
                    Self::option_message(
                        "GetUserStatusResponse",
                        vec![
                            Self::option_field("registered", "bool", "boolean", false, false),
                            Self::option_field("user_id", "string", "string", false, false),
                            Self::option_field("email", "string", "string", false, false),
                            Self::option_field("email_verified", "bool", "boolean", false, false),
                            Self::option_field("wireguard_public_key", "string", "string", true, false),
                            Self::option_field("assigned_ip", "string", "string", false, false),
                            Self::option_field("is_admin", "bool", "boolean", false, false),
                            Self::option_field(
                                "registered_at",
                                "google.protobuf.Timestamp",
                                "string:date-time",
                                false,
                                false,
                            ),
                            Self::option_field(
                                "last_active",
                                "google.protobuf.Timestamp",
                                "string:date-time",
                                false,
                                false,
                            ),
                        ],
                    ),
                    Self::option_message(
                        "ListUsersRequest",
                        vec![
                            Self::option_field("limit", "uint32", "integer:uint32", false, false),
                            Self::option_field("offset", "uint32", "integer:uint32", false, false),
                            Self::option_field("include_admins_only", "bool", "boolean", false, false),
                            Self::option_field("domain_filter", "string", "string", false, false),
                        ],
                    ),
                    Self::option_message(
                        "ListUsersResponse",
                        vec![
                            Self::option_field("users", "repeated UserInfo", "array", false, false),
                            Self::option_field("total_count", "uint32", "integer:uint32", false, false),
                            Self::option_field("filtered_count", "uint32", "integer:uint32", false, false),
                        ],
                    ),
                    Self::option_message(
                        "UserInfo",
                        vec![
                            Self::option_field("user_id", "string", "string", false, false),
                            Self::option_field("email", "string", "string", false, false),
                            Self::option_field("email_verified", "bool", "boolean", false, false),
                            Self::option_field("wireguard_public_key", "string", "string", true, false),
                            Self::option_field("assigned_ip", "string", "string", false, false),
                            Self::option_field("is_admin", "bool", "boolean", false, false),
                            Self::option_field(
                                "registered_at",
                                "google.protobuf.Timestamp",
                                "string:date-time",
                                false,
                                false,
                            ),
                            Self::option_field(
                                "last_active",
                                "google.protobuf.Timestamp",
                                "string:date-time",
                                false,
                                false,
                            ),
                            Self::option_field(
                                "metadata",
                                "optional google.protobuf.Struct",
                                "object",
                                false,
                                false,
                            ),
                        ],
                    ),
                    Self::option_message(
                        "GetWireGuardConfigRequest",
                        vec![
                            Self::option_field("email", "string", "string", false, false),
                            Self::option_field("user_id", "string", "string", false, false),
                            Self::option_field("domain", "string", "string", false, false),
                        ],
                    ),
                    Self::option_message(
                        "GetWireGuardConfigResponse",
                        vec![
                            Self::option_field("success", "bool", "boolean", false, false),
                            Self::option_field("wireguard_config", "string", "string", true, false),
                            Self::option_field("public_key", "string", "string", true, false),
                            Self::option_field("assigned_ip", "string", "string", false, false),
                            Self::option_field("message", "string", "string", false, false),
                            Self::option_field(
                                "generated_at",
                                "google.protobuf.Timestamp",
                                "string:date-time",
                                false,
                                false,
                            ),
                        ],
                    ),
                    Self::option_message(
                        "AdminUserActionRequest",
                        vec![
                            Self::option_field("action", "string", "string", false, true),
                            Self::option_field("user_id", "string", "string", false, false),
                            Self::option_field("email", "string", "string", false, false),
                            Self::option_field(
                                "parameters",
                                "optional google.protobuf.Struct",
                                "object",
                                false,
                                false,
                            ),
                        ],
                    ),
                    Self::option_message(
                        "AdminUserActionResponse",
                        vec![
                            Self::option_field("success", "bool", "boolean", false, false),
                            Self::option_field("message", "string", "string", false, false),
                            Self::option_field("user_id", "string", "string", false, false),
                            Self::option_field(
                                "action_timestamp",
                                "google.protobuf.Timestamp",
                                "string:date-time",
                                false,
                                false,
                            ),
                        ],
                    ),
                    Self::option_message(
                        "RegistrationError",
                        vec![
                            Self::option_field("code", "int32", "integer:int32", false, false),
                            Self::option_field("message", "string", "string", false, false),
                            Self::option_field(
                                "details",
                                "optional google.protobuf.Struct",
                                "object",
                                false,
                                false,
                            ),
                        ],
                    ),
                ],
                error_codes: vec![
                    ConfigurableOptionErrorCode {
                        name: "REGISTRATION_ERROR_UNSPECIFIED".to_string(),
                        value: 0,
                    },
                    ConfigurableOptionErrorCode {
                        name: "REGISTRATION_ERROR_INVALID_EMAIL".to_string(),
                        value: 1,
                    },
                    ConfigurableOptionErrorCode {
                        name: "REGISTRATION_ERROR_INVALID_TOKEN".to_string(),
                        value: 2,
                    },
                    ConfigurableOptionErrorCode {
                        name: "REGISTRATION_ERROR_USER_EXISTS".to_string(),
                        value: 3,
                    },
                    ConfigurableOptionErrorCode {
                        name: "REGISTRATION_ERROR_WIREGUARD_KEY_INVALID".to_string(),
                        value: 4,
                    },
                    ConfigurableOptionErrorCode {
                        name: "REGISTRATION_ERROR_NETWORK_UNAVAILABLE".to_string(),
                        value: 5,
                    },
                    ConfigurableOptionErrorCode {
                        name: "REGISTRATION_ERROR_ADMIN_REQUIRED".to_string(),
                        value: 6,
                    },
                ],
            },
            user_container: UserContainerOptions {
                source_script: "deploy/scripts/provision-workspace-subscriber.sh".to_string(),
                arguments: vec![
                    Self::option_field("username", "positional string", "string", false, true),
                    Self::option_field("--mac", "option string", "string", true, false),
                    Self::option_field("--psk", "option string", "string", true, false),
                    Self::option_field("--email", "option string", "string", false, false),
                    Self::option_field("--ghostbridge", "flag bool", "boolean", false, false),
                    Self::option_field("--semantic", "flag bool", "boolean", false, false),
                ],
                container_id_template: "ws-{username}".to_string(),
                image: "images:debian/12".to_string(),
                cognitive_mcp_endpoint: "unix:path=/run/opdbus/session-bus.sock".to_string(),
                feature_flags: vec!["ghostbridge".to_string(), "semantic_search".to_string()],
            },
            identity_chain: IdentityOptions {
                mac_address_key: "--mac".to_string(),
                pre_shared_key: "--psk".to_string(),
                wireguard_pubkey: "container:{container_id}:identity/wireguard_pubkey".to_string(),
                mcp_token_derivation: "uuid_v5(wireguard_pubkey)".to_string(),
            },
            memory_namespaces: vec![
                Self::memory_namespace(
                    "container:{container_id}:identity",
                    &["wireguard_pubkey", "mcp_token", "mac_address", "psk", "email"],
                    "Container identity namespace. Email is present only when GhostBridge is disabled.",
                ),
                Self::memory_namespace(
                    "container:{container_id}:soul",
                    &["profile"],
                    "Container soul profile bound to the same container identity.",
                ),
                Self::memory_namespace(
                    "container:{container_id}:domain:work",
                    &["MEMORY_INDEX"],
                    "Work-domain namespace bound to the container identity.",
                ),
                Self::memory_namespace(
                    "container:{container_id}:domain:personal",
                    &["MEMORY_INDEX"],
                    "Personal-domain namespace bound to the container identity.",
                ),
                Self::memory_namespace(
                    "container:{container_id}:domain:home",
                    &["MEMORY_INDEX"],
                    "Home-domain namespace bound to the container identity.",
                ),
                Self::memory_namespace(
                    "container:{container_id}:index",
                    &["MEMORY_INDEX"],
                    "Namespace index for identity, soul, and domain namespaces.",
                ),
                Self::memory_namespace(
                    "container:{container_id}:features",
                    &["ghostbridge", "semantic_search"],
                    "Feature flags projected for the container identity.",
                ),
            ],
            privacy_policy: PrivacyOptions {
                email_storage_rule:
                    "Store email only when GhostBridge is disabled; GhostBridge users withhold email from CozoDB."
                        .to_string(),
                sensitive_fields: vec![
                    "token".to_string(),
                    "wireguard_public_key".to_string(),
                    "wireguard_config".to_string(),
                    "public_key".to_string(),
                    "mcp_token".to_string(),
                    "mac_address".to_string(),
                    "psk".to_string(),
                ],
            },
        }
    }
}

#[async_trait]
impl StatePlugin for TchedRouterPlugin {
    fn name(&self) -> &str {
        PLUGIN_NAME
    }

    fn version(&self) -> &str {
        PLUGIN_VERSION
    }

    fn schema(&self) -> Option<PluginSchema> {
        let mut schema = tched_router_schema();
        super::common::llm_projection::rewrite_projection_subids_for_plugin(
            &mut schema,
            "3tched-router",
        );
        super::common::oscal::ensure_category_metadata_fields(&mut schema);
        Some(schema)
    }

    async fn schema_live(&self) -> Option<PluginSchema> {
        // No live probes — there is no projection, only declared state.
        self.schema()
    }

    async fn calculate_diff(&self, _current: &Value, _desired: &Value) -> Result<StateDiff> {
        Ok(StateDiff {
            plugin: PLUGIN_NAME.to_string(),
            actions: vec![],
            metadata: DiffMetadata {
                timestamp: chrono::Utc::now().timestamp(),
                current_hash: "schema-declared".to_string(),
                desired_hash: "schema-declared".to_string(),
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

/// Canonical `tched_router` schema derived from [`TchedRouterState`] via schemars.
pub(crate) fn tched_router_schema() -> PluginSchema {
    tched_router_schema_from_state(TchedRouterPlugin::current_state())
}

fn tched_router_schema_from_state(state: TchedRouterState) -> PluginSchema {
    let root = serde_json::to_value(schemars::schema_for!(TchedRouterState))
        .expect("schemars schema serializes to JSON");
    let mut schema = super::schemars_adapter::plugin_schema_from_json(
        PLUGIN_NAME,
        PLUGIN_VERSION,
        PLUGIN_DESCRIPTION,
        &root,
    );
    schema
        .fields
        .get_mut("native_config")
        .expect("tched_router native_config field is present")
        .field_type = op_state_store::FieldType::exact_json_schema(
        super::tched_router_contract::document().clone(),
    )
    .expect("plugin-owned tched_router config contract is a valid exact field schema");
    schema.category = PLUGIN_CATEGORY.to_string();
    let binding_field = schema
        .fields
        .get_mut("catalog_model_binding")
        .expect("catalog binding field is present");
    binding_field.read_only = true;
    binding_field.field_type = op_state_store::FieldType::exact_json_schema(
        serde_json::to_value(schemars::schema_for!(
            Option<ResolveCatalogModelBindingOutput>
        ))
        .expect("catalog binding field schema serializes"),
    )
    .expect("catalog binding field has a valid exact nullable typed schema");
    schema.display_name = Some(PLUGIN_DISPLAY_NAME.to_string());

    if let Ok(state) = simd_json::serde::to_owned_value(state) {
        super::schemars_adapter::apply_state_defaults(&mut schema, &state);
        schema.example = Some(state);
    }

    schema.methods.insert(
        "GetState".to_string(),
        method_decl_from_schemars_with_output::<EmptyTchedInput, GetStateOutput>(
            "GetState",
            op_state_store::SideEffect::Read,
            true,
            "cap.software.3tched-router.state.read@v1",
            "obs.service.3tched-router.state.get@v1",
        ),
    );
    schema.methods.insert(
        "GetModelRoutes".to_string(),
        method_decl_from_schemars_with_output::<EmptyTchedInput, GetModelRoutesOutput>(
            "GetModelRoutes",
            op_state_store::SideEffect::Read,
            true,
            "cap.software.3tched-router.routes.read@v1",
            "obs.service.3tched-router.model-routes.list@v1",
        ),
    );
    schema.methods.insert(
        "ListProviders".to_string(),
        method_decl_from_schemars_with_output::<EmptyTchedInput, ListProvidersOutput>(
            "ListProviders",
            op_state_store::SideEffect::Read,
            true,
            "cap.software.3tched-router.providers.read@v1",
            "obs.service.3tched-router.providers.list@v1",
        ),
    );
    schema.methods.insert(
        "GetProviderCatalog".to_string(),
        method_decl_from_schemars_with_output::<EmptyTchedInput, GetProviderCatalogOutput>(
            "GetProviderCatalog",
            op_state_store::SideEffect::Read,
            true,
            "cap.software.3tched-router.providers.read@v1",
            "obs.service.3tched-router.provider-catalog.list@v1",
        ),
    );
    schema.methods.insert(
        "GetTools".to_string(),
        method_decl_from_schemars_with_output::<EmptyTchedInput, GetToolsOutput>(
            "GetTools",
            op_state_store::SideEffect::Read,
            true,
            "cap.software.3tched-router.tools.read@v1",
            "obs.service.3tched-router.tools.list@v1",
        ),
    );
    schema.methods.insert(
        "GetRouter".to_string(),
        method_decl_from_schemars_with_output::<EmptyTchedInput, GetRouterOutput>(
            "GetRouter",
            op_state_store::SideEffect::Read,
            true,
            "cap.software.3tched-router.router.read@v1",
            "obs.service.3tched-router.router.get@v1",
        ),
    );
    schema.methods.insert(
        "GetConfigSchema".to_string(),
        method_decl_from_schemars_with_output::<EmptyTchedInput, GetConfigSchemaOutput>(
            "GetConfigSchema",
            op_state_store::SideEffect::Read,
            true,
            "cap.software.3tched-router.config-schema.read@v1",
            "obs.service.3tched-router.config-schema.get@v1",
        ),
    );
    schema.methods.insert(
        "ListUiSurfaces".to_string(),
        method_decl_from_schemars_with_output::<EmptyTchedInput, ListUiSurfacesOutput>(
            "ListUiSurfaces",
            op_state_store::SideEffect::Read,
            true,
            "cap.software.3tched-router.ui-surfaces.read@v1",
            "obs.service.3tched-router.ui-surfaces.list@v1",
        ),
    );
    schema.methods.insert(
        "GetStructuredOutput".to_string(),
        method_decl_from_schemars_with_output::<EmptyTchedInput, GetStructuredOutputOutput>(
            "GetStructuredOutput",
            op_state_store::SideEffect::Read,
            true,
            "cap.software.3tched-router.structured-output.read@v1",
            "obs.service.3tched-router.structured-output.get@v1",
        ),
    );
    schema.methods.insert(
        "ResolveRoute".to_string(),
        method_decl_from_schemars_with_output::<ResolveRouteInput, ResolveRouteOutput>(
            "ResolveRoute",
            op_state_store::SideEffect::Read,
            true,
            "cap.software.3tched-router.route.resolve@v1",
            "obs.service.3tched-router.route.resolve@v1",
        ),
    );
    schema.methods.insert(
        "Chat".to_string(),
        method_decl_from_schemars_with_output::<ChatInput, ChatOutput>(
            "Chat",
            op_state_store::SideEffect::Read,
            false,
            "cap.software.3tched-router.chat@v1",
            "exp.service.3tched-router.chat@v1",
        ),
    );
    schema.methods.insert(
        "ListModels".to_string(),
        method_decl_from_schemars_with_output::<ListModelsInput, ListModelsOutput>(
            "ListModels",
            op_state_store::SideEffect::Read,
            true,
            "cap.software.3tched-router.models.read@v1",
            "obs.service.3tched-router.models.list@v1",
        ),
    );
    schema.methods.insert(
        "SetProvider".to_string(),
        method_decl_from_schemars_with_output::<SetProviderInput, SetProviderOutput>(
            "SetProvider",
            op_state_store::SideEffect::Mutation,
            false,
            "cap.software.3tched-router.provider.set@v1",
            "mut.service.3tched-router.provider.set@v1",
        ),
    );
    schema.methods.insert(
        "SetModel".to_string(),
        method_decl_from_schemars_with_output::<SetModelInput, SetModelOutput>(
            "SetModel",
            op_state_store::SideEffect::Mutation,
            false,
            "cap.software.3tched-router.model.set@v1",
            "mut.service.3tched-router.model.set@v1",
        ),
    );
    schema.methods.insert(
        "SetSelection".to_string(),
        method_decl_from_schemars_with_output::<SetSelectionInput, SetSelectionOutput>(
            "SetSelection",
            op_state_store::SideEffect::Mutation,
            false,
            "cap.software.tched-router.selection.set@v1",
            "mut.service.tched-router.selection.set@v1",
        ),
    );
    schema.methods.insert(
        "SetOvsRoutingModel".to_string(),
        method_decl_from_schemars_with_output::<SetOvsRoutingModelInput, SetOvsRoutingModelOutput>(
            "SetOvsRoutingModel",
            op_state_store::SideEffect::Mutation,
            false,
            "cap.software.3tched-router.model-assignments.ovs-routing.set@v1",
            "mut.service.3tched-router.model-assignments.ovs-routing.set@v1",
        ),
    );
    schema.methods.insert(
        "SetObfuscationModel".to_string(),
        method_decl_from_schemars_with_output::<SetObfuscationModelInput, SetObfuscationModelOutput>(
            "SetObfuscationModel",
            op_state_store::SideEffect::Mutation,
            false,
            "cap.software.3tched-router.model-assignments.obfuscation.set@v1",
            "mut.service.3tched-router.model-assignments.obfuscation.set@v1",
        ),
    );
    schema.methods.insert(
        "SetVectorizationModel".to_string(),
        method_decl_from_schemars_with_output::<
            SetVectorizationModelInput,
            SetVectorizationModelOutput,
        >(
            "SetVectorizationModel",
            op_state_store::SideEffect::Mutation,
            false,
            "cap.software.3tched-router.model-assignments.vectorization.set@v1",
            "mut.service.3tched-router.model-assignments.vectorization.set@v1",
        ),
    );
    schema.methods.insert(
        "SetQdrantRetrievalModel".to_string(),
        method_decl_from_schemars_with_output::<
            SetQdrantRetrievalModelInput,
            SetQdrantRetrievalModelOutput,
        >(
            "SetQdrantRetrievalModel",
            op_state_store::SideEffect::Mutation,
            false,
            "cap.software.3tched-router.model-assignments.qdrant-retrieval.set@v1",
            "mut.service.3tched-router.model-assignments.qdrant-retrieval.set@v1",
        ),
    );
    schema.methods.insert(
        "SetCozoRetrievalModel".to_string(),
        method_decl_from_schemars_with_output::<
            SetCozoRetrievalModelInput,
            SetCozoRetrievalModelOutput,
        >(
            "SetCozoRetrievalModel",
            op_state_store::SideEffect::Mutation,
            false,
            "cap.software.3tched-router.model-assignments.cozo-retrieval.set@v1",
            "mut.service.3tched-router.model-assignments.cozo-retrieval.set@v1",
        ),
    );

    // Declare every capability this plugin's methods gate on — the plugin is
    // the single declaration point (closure-clean under
    // `validate_capability_closure`).
    for (id, description) in CARRIED_CAPABILITIES {
        schema.capabilities.insert(
            id.to_string(),
            CapabilityDecl {
                id: id.to_string(),
                description: description.to_string(),
            },
        );
    }

    // Complete typed config surface, authored like every other plugin.
    // The former 87 generated per-section getters (and their import bin)
    // are retired; the two load-bearing operations remain, addressed by
    // the same capability ids the grant table carries.
    schema.methods.insert(
        "GetConfig".to_string(),
        method_decl_from_schemars_with_output::<EmptyTchedInput, GetConfigOutput>(
            "GetConfig",
            SideEffect::Read,
            true,
            "cap.software.3tched-router.config.read@v1",
            "obs.service.3tched-router.config.get@v1",
        ),
    );
    schema.capabilities.insert(
        "cap.software.3tched-router.config.read@v1".to_string(),
        CapabilityDecl {
            id: "cap.software.3tched-router.config.read@v1".to_string(),
            description: "Read the full 3tched Router configuration (secrets masked).".to_string(),
        },
    );
    schema.methods.insert(
        "PatchConfig".to_string(),
        method_decl_from_schemars_with_output::<ConfigPatchInput, ConfigPatchOutput>(
            "PatchConfig",
            SideEffect::Mutation,
            false,
            "cap.software.3tched-router.config.write@v1",
            "mut.service.3tched-router.config.patch@v2",
        ),
    );
    schema.capabilities.insert(
        "cap.software.3tched-router.config.write@v1".to_string(),
        CapabilityDecl {
            id: "cap.software.3tched-router.config.write@v1".to_string(),
            description: "Apply bounded typed 3tched Router configuration property operations."
                .to_string(),
        },
    );

    register_cli_config_methods(&mut schema);
    generation::declare_methods(&mut schema);

    schema.methods.insert(
        "GetMcpConfig".to_string(),
        method_decl_from_schemars_with_output::<EmptyTchedInput, GetMcpConfigOutput>(
            "GetMcpConfig",
            SideEffect::Read,
            true,
            "cap.software.3tched-router.mcp-config.read@v1",
            "obs.service.3tched-router.mcp-config.get@v1",
        ),
    );
    schema.capabilities.insert(
        "cap.software.3tched-router.mcp-config.read@v1".to_string(),
        CapabilityDecl {
            id: "cap.software.3tched-router.mcp-config.read@v1".to_string(),
            description: "Read the 3tched Router `mcp` configuration (secrets masked).".to_string(),
        },
    );
    // Generic property writes are aliases for these real native operations.
    // Their existing method capabilities still apply to the original caller.
    for (property, method, argument) in [
        ("selected_provider", "SetProvider", "provider_id"),
        ("selected_model", "SetModel", "model_id"),
    ] {
        schema.field_writes.insert(
            property.to_string(),
            op_state_store::plugin_schema::FieldWriteDecl {
                method: method.to_string(),
                argument: argument.to_string(),
            },
        );
    }

    // Publish current model selection and its typed allocation policy. The
    // provider catalog, endpoints, authentication placeholders, identity,
    // memory, privacy, and arbitrary upstream configuration remain internal.
    schema.public_state.paths = vec![
        "/status".to_string(),
        "/selected_provider".to_string(),
        "/selected_model".to_string(),
        "/model_assignments".to_string(),
        "/selector_policy".to_string(),
    ];

    super::binary_namespace::attach_model_routes(&mut schema);

    schema
}

/// Typed config methods authored by this plugin (the generated per-section
/// getters retired); the bridge routes these by the same names.
pub(crate) const NATIVE_CONFIG_METHODS: &[&str] = &["GetConfig", "PatchConfig"];

fn register_cli_config_methods(schema: &mut PluginSchema) {
    schema.methods.insert(
        "apply_catalog_model_binding".to_string(),
        method_decl_from_schemars_with_output::<
            ResolveCatalogModelBindingInput,
            ResolveCatalogModelBindingOutput,
        >(
            "apply_catalog_model_binding",
            SideEffect::Mutation,
            false,
            "cap.software.tched-router.catalog.selection.set@v1",
            "mut.software.tched-router.catalog-model-binding@v1",
        ),
    );
    schema.methods.insert(
        "resolve_catalog_model_binding".to_string(),
        method_decl_from_schemars_with_output::<
            ResolveCatalogModelBindingInput,
            ResolveCatalogModelBindingOutput,
        >(
            "resolve_catalog_model_binding",
            SideEffect::Read,
            true,
            "cap.software.tched-router.model.binding.read@v1",
            "obs.software.tched-router.catalog-model-binding@v1",
        ),
    );
    schema.methods.insert(
        "get_own_agent_route".to_string(),
        method_decl_from_schemars_with_output::<GetOwnAgentRouteInput, GetOwnAgentRouteOutput>(
            "get_own_agent_route",
            SideEffect::Read,
            true,
            "cap.software.tched-router.agent.route.self.read@v1",
            "obs.software.tched-router.agent-route-self@v1",
        ),
    );
    let read_capability = "cap.software.tched-router.router.read@v1";
    let write_capability = "cap.software.tched-router.router.write@v1";
    schema.methods.insert(
        "config_list".to_string(),
        method_decl_from_schemars_with_output::<ConfigListInput, ConfigListOutput>(
            "config_list",
            SideEffect::Read,
            true,
            read_capability,
            "obs.software.tched-router.config-list@v2",
        ),
    );
    schema.methods.insert(
        "config_get".to_string(),
        method_decl_from_schemars_with_output::<ConfigGetInput, ConfigPropertyOutput>(
            "config_get",
            SideEffect::Read,
            true,
            read_capability,
            "obs.software.tched-router.config-get@v2",
        ),
    );
    schema.methods.insert(
        "config_set".to_string(),
        method_decl_from_schemars_with_output::<ConfigSetInput, ConfigPropertyOutput>(
            "config_set",
            SideEffect::Mutation,
            false,
            write_capability,
            "mut.software.tched-router.config-set@v2",
        ),
    );
    schema.methods.insert(
        "config_patch".to_string(),
        method_decl_from_schemars_with_output::<ConfigPatchInput, ConfigPatchOutput>(
            "config_patch",
            SideEffect::Mutation,
            false,
            write_capability,
            "mut.software.tched-router.config-patch@v2",
        ),
    );
    schema.methods.insert(
        "config_init".to_string(),
        method_decl_from_schemars_with_output::<ConfigInitInput, ConfigInitOutput>(
            "config_init",
            SideEffect::Mutation,
            false,
            write_capability,
            "mut.software.tched-router.config-init@v2",
        ),
    );
    schema.methods.insert(
        "config_migrate".to_string(),
        method_decl_from_schemars_with_output::<ConfigMigrateInput, ConfigMigrateOutput>(
            "config_migrate",
            SideEffect::Mutation,
            false,
            write_capability,
            "mut.software.tched-router.config-migrate@v2",
        ),
    );
    schema.capabilities.insert(
        read_capability.to_string(),
        CapabilityDecl {
            id: read_capability.to_string(),
            description: "Read masked native tched_router configuration.".to_string(),
        },
    );
    schema.capabilities.insert(
        write_capability.to_string(),
        CapabilityDecl {
            id: write_capability.to_string(),
            description: "Mutate native tched_router configuration through its typed loopback API."
                .to_string(),
        },
    );
}

/// Capabilities gated by the hand-carried (non-generated) methods.
const CARRIED_CAPABILITIES: &[(&str, &str)] = &[
    (
        "cap.software.3tched-router.state.read@v1",
        "Read the full 3tched Router declared state.",
    ),
    (
        "cap.software.3tched-router.providers.read@v1",
        "Read the declared provider catalog.",
    ),
    (
        "cap.software.3tched-router.provider.set@v1",
        "Select the active provider.",
    ),
    (
        "cap.software.3tched-router.routes.read@v1",
        "Read declared model routes.",
    ),
    (
        "cap.software.3tched-router.models.read@v1",
        "List models, optionally filtered by provider.",
    ),
    (
        "cap.software.3tched-router.model.set@v1",
        "Select the active model.",
    ),
    (
        "cap.software.3tched-router.route.resolve@v1",
        "Resolve a route hint to a declared model route.",
    ),
    (
        "cap.software.3tched-router.router.read@v1",
        "Read the router configuration.",
    ),
    (
        "cap.software.3tched-router.tools.read@v1",
        "Read the declared LLM tool catalog.",
    ),
    (
        "cap.software.3tched-router.config-schema.read@v1",
        "Read the config schema descriptor.",
    ),
    (
        "cap.software.3tched-router.ui-surfaces.read@v1",
        "List registered UI surfaces.",
    ),
    (
        "cap.software.3tched-router.structured-output.read@v1",
        "Read the structured-output contract.",
    ),
    (
        "cap.software.3tched-router.chat@v1",
        "Send a chat turn through the bridge-owned dispatcher.",
    ),
    (
        "cap.software.tched-router.router.read@v1",
        "Read tched_router configuration via config_list/get.",
    ),
    (
        "cap.software.tched-router.agent.route.self.read@v1",
        "Read only the dedicated native route selected by the caller's persisted chat mode.",
    ),
    (
        "cap.software.tched-router.model.binding.read@v1",
        "Resolve one available catalog model to a verified ordinary native profile and safe endpoint.",
    ),
    (
        "cap.software.tched-router.catalog.selection.set@v1",
        "Apply one verified available catalog model only to an allowed ordinary native profile.",
    ),
    (
        "cap.software.tched-router.selection.set@v1",
        "Select the active provider and model together.",
    ),
    (
        "cap.software.tched-router.router.write@v1",
        "Mutate tched_router configuration via config_set/patch.",
    ),
    (
        "cap.software.3tched-router.model-assignments.ovs-routing.set@v1",
        "Set the OVS-routing model assignment.",
    ),
    (
        "cap.software.3tched-router.model-assignments.obfuscation.set@v1",
        "Set the obfuscation model assignment.",
    ),
    (
        "cap.software.3tched-router.model-assignments.vectorization.set@v1",
        "Set the vectorization model assignment.",
    ),
    (
        "cap.software.3tched-router.model-assignments.qdrant-retrieval.set@v1",
        "Set the Qdrant-retrieval model assignment.",
    ),
    (
        "cap.software.3tched-router.model-assignments.cozo-retrieval.set@v1",
        "Set the Cozo-retrieval model assignment.",
    ),
];

/// Public accessor for crates that embed the 3tched Router plugin contract.
pub fn tched_router_plugin_schema() -> PluginSchema {
    tched_router_schema()
}

/// A signal the bridge can emit after a successful plugin-owned dispatch.
#[derive(Debug, Clone, PartialEq)]
pub struct DispatchSignal {
    pub name: String,
    pub payload: JsonValue,
}

/// Outcome of a plugin-owned method dispatch.
#[derive(Debug, Clone)]
pub struct DispatchOutcome {
    pub result: JsonValue,
    pub signal: Option<DispatchSignal>,
}

impl DispatchOutcome {
    pub(crate) fn plain(result: JsonValue) -> Self {
        Self {
            result,
            signal: None,
        }
    }
}

/// Plugin-owned method dispatch for the 3tched Router D-Bus/gRPC surface.
pub fn dispatch_tched_router_method(
    method: &str,
    json_args: &str,
    state: &TchedRouterState,
) -> std::result::Result<DispatchOutcome, TchedRouterError> {
    if let Some(outcome) = super::binary_namespace::dispatch(method, json_args) {
        return outcome.map(DispatchOutcome::plain);
    }
    match method {
        "GetState" => Ok(DispatchOutcome::plain(serde_json::json!({
            "state": to_json(&ObservedTchedRouterState::from(state))
        }))),
        "GetModelRoutes" => Ok(DispatchOutcome::plain(
            serde_json::json!({ "model_routes": to_json(&state.catalog.model_routes) }),
        )),
        "GetProviderCatalog" => Ok(DispatchOutcome::plain(
            serde_json::json!({ "providers": to_json(&state.catalog.providers) }),
        )),
        "GetTools" => Ok(DispatchOutcome::plain(
            serde_json::json!({ "tools": to_json(&state.catalog.tools) }),
        )),
        "ListProviders" => Ok(DispatchOutcome::plain(
            serde_json::json!({ "providers": to_json(&state.catalog.providers) }),
        )),
        "GetRouter" => Ok(DispatchOutcome::plain(
            serde_json::json!({ "router": to_json(&state.catalog.router) }),
        )),
        "GetConfigSchema" => Ok(DispatchOutcome::plain(
            serde_json::json!({ "config_schema": to_json(&state.catalog.config_schema) }),
        )),
        "ListUiSurfaces" => Ok(DispatchOutcome::plain(
            serde_json::json!({ "ui_surfaces": to_json(&state.catalog.ui_surfaces) }),
        )),
        "GetStructuredOutput" => Ok(DispatchOutcome::plain(
            serde_json::json!({ "structured_output": to_json(&state.catalog.structured_output) }),
        )),
        "ResolveRoute" => resolve_route(json_args, state).map(DispatchOutcome::plain),
        "ListModels" => list_models(json_args, state).map(DispatchOutcome::plain),
        "SetProvider" => set_provider_handler(json_args, state),
        "SetModel" => set_model_handler(json_args, state),
        "SetSelection" => set_selection_handler(json_args, state),
        "SetOvsRoutingModel" => set_role_model_handler(json_args, state, "ovs_routing"),
        "SetObfuscationModel" => set_role_model_handler(json_args, state, "obfuscation"),
        "SetVectorizationModel" => set_role_model_handler(json_args, state, "vectorization"),
        "SetQdrantRetrievalModel" => set_role_model_handler(json_args, state, "qdrant_retrieval"),
        "SetCozoRetrievalModel" => set_role_model_handler(json_args, state, "cozo_retrieval"),
        other => {
            let native_method = NATIVE_CONFIG_METHODS.contains(&other)
                || generation::is_method(other)
                || matches!(
                    other,
                    "Chat"
                        | "get_own_agent_route"
                        | "resolve_catalog_model_binding"
                        | "apply_catalog_model_binding"
                        | "config_list"
                        | "config_get"
                        | "config_set"
                        | "config_patch"
                        | "config_init"
                        | "config_migrate"
                );
            Err(TchedRouterError::ExecutionDenied {
                reason: if native_method {
                    format!("method requires admitted native bridge dispatch: {other}")
                } else {
                    format!("undeclared method: {other}")
                },
            })
        }
    }
}

fn list_models(
    json_args: &str,
    state: &TchedRouterState,
) -> std::result::Result<JsonValue, TchedRouterError> {
    let args = parse_args("ListModels", json_args)?;
    let provider = args
        .get("provider")
        .and_then(JsonValue::as_str)
        .unwrap_or_default();
    let routes = state
        .catalog
        .model_routes
        .iter()
        .filter(|route| {
            provider.is_empty()
                || route.provider.eq_ignore_ascii_case(provider)
                || route.upstream_provider.eq_ignore_ascii_case(provider)
        })
        .cloned()
        .collect::<Vec<_>>();
    Ok(serde_json::json!({ "model_routes": to_json(&routes) }))
}

fn to_json<T: Serialize>(value: &T) -> JsonValue {
    serde_json::to_value(value).unwrap_or(JsonValue::Null)
}

fn parse_args(method: &str, json_args: &str) -> std::result::Result<JsonValue, TchedRouterError> {
    if json_args.trim().is_empty() {
        return Ok(serde_json::json!({}));
    }
    serde_json::from_str(json_args).map_err(|error| TchedRouterError::ExecutionDenied {
        reason: format!("{method} arguments are not valid JSON: {error}"),
    })
}

fn require_str(
    args: &JsonValue,
    field: &str,
    method: &str,
) -> std::result::Result<String, TchedRouterError> {
    args.get(field)
        .and_then(JsonValue::as_str)
        .map(str::to_string)
        .ok_or_else(|| TchedRouterError::ExecutionDenied {
            reason: format!("{method} requires string field '{field}'"),
        })
}

fn resolve_route(
    json_args: &str,
    state: &TchedRouterState,
) -> std::result::Result<JsonValue, TchedRouterError> {
    let args = parse_args("ResolveRoute", json_args)?;
    let hint = require_str(&args, "hint", "ResolveRoute")?;
    state
        .catalog
        .model_routes
        .iter()
        .find(|route| route.hint == hint || route.model == hint)
        .map(|route| serde_json::json!({ "route": to_json(route) }))
        .ok_or(TchedRouterError::RouteNotDeclared { hint })
}

fn set_provider_handler(
    json_args: &str,
    state: &TchedRouterState,
) -> std::result::Result<DispatchOutcome, TchedRouterError> {
    let args = parse_args("SetProvider", json_args)?;
    let provider_id = require_str(&args, "provider_id", "SetProvider")?;
    if !state.catalog.providers.iter().any(|p| p.id == provider_id) {
        return Err(TchedRouterError::ProviderNotDeclared {
            provider: provider_id,
        });
    }
    let old = state.selected_provider.clone();
    Ok(DispatchOutcome {
        result: serde_json::json!({ "selected_provider": provider_id }),
        signal: Some(DispatchSignal {
            name: "ProviderChanged".to_string(),
            payload: serde_json::json!({ "old": old, "new": provider_id, "reason": "explicit set" }),
        }),
    })
}

fn set_selection_handler(
    json_args: &str,
    state: &TchedRouterState,
) -> std::result::Result<DispatchOutcome, TchedRouterError> {
    let args = parse_args("SetSelection", json_args)?;
    let provider_id = args
        .get("provider_id")
        .or_else(|| args.get("providerId"))
        .and_then(JsonValue::as_str)
        .map(str::to_string)
        .ok_or_else(|| TchedRouterError::ExecutionDenied {
            reason: "SetSelection requires string field 'provider_id'".to_string(),
        })?;
    let model_id = args
        .get("model_id")
        .or_else(|| args.get("modelId"))
        .and_then(JsonValue::as_str)
        .map(str::to_string)
        .ok_or_else(|| TchedRouterError::ExecutionDenied {
            reason: "SetSelection requires string field 'model_id'".to_string(),
        })?;
    if !state.catalog.providers.iter().any(|p| p.id == provider_id) {
        return Err(TchedRouterError::ProviderNotDeclared {
            provider: provider_id,
        });
    }
    if !state
        .catalog
        .model_routes
        .iter()
        .any(|r| r.model == model_id)
    {
        return Err(TchedRouterError::ModelNotDeclared { model: model_id });
    }
    Ok(DispatchOutcome {
        result: serde_json::json!({
            "selected_provider": provider_id,
            "selected_model": model_id,
        }),
        signal: Some(DispatchSignal {
            name: "SelectionChanged".to_string(),
            payload: serde_json::json!({
                "provider": provider_id,
                "model": model_id,
            }),
        }),
    })
}

fn set_model_handler(
    json_args: &str,
    state: &TchedRouterState,
) -> std::result::Result<DispatchOutcome, TchedRouterError> {
    let args = parse_args("SetModel", json_args)?;
    let model_id = require_str(&args, "model_id", "SetModel")?;
    if !state
        .catalog
        .model_routes
        .iter()
        .any(|r| r.model == model_id)
    {
        return Err(TchedRouterError::ModelNotDeclared { model: model_id });
    }
    let old = state.selected_model.clone();
    Ok(DispatchOutcome {
        result: serde_json::json!({ "selected_model": model_id }),
        signal: Some(DispatchSignal {
            name: "ModelChanged".to_string(),
            payload: serde_json::json!({ "old": old, "new": model_id, "reason": "explicit set" }),
        }),
    })
}

fn set_role_model_handler(
    json_args: &str,
    state: &TchedRouterState,
    role: &str,
) -> std::result::Result<DispatchOutcome, TchedRouterError> {
    let args = parse_args("SetRoleModel", json_args)?;
    let model_id = require_str(&args, "model_id", "SetRoleModel")?;
    let result = match role {
        "ovs_routing" => serde_json::json!({ "ovs_routing": model_id }),
        "obfuscation" => serde_json::json!({ "obfuscation": model_id }),
        "vectorization" => serde_json::json!({ "vectorization": model_id }),
        "qdrant_retrieval" => serde_json::json!({ "qdrant_retrieval": model_id }),
        "cozo_retrieval" => serde_json::json!({ "cozo_retrieval": model_id }),
        _ => {
            return Err(TchedRouterError::ExecutionDenied {
                reason: format!("unknown model role: {role}"),
            });
        }
    };
    let signal = serde_json::json!({
        "role": role,
        "model_id": model_id,
        "selected_provider": state.selected_provider,
    });
    Ok(DispatchOutcome {
        result,
        signal: Some(DispatchSignal {
            name: "ModelRoleChanged".to_string(),
            payload: signal,
        }),
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn catalog_binding_schema_is_narrow_and_state_is_readonly() {
        let schema = super::tched_router_plugin_schema();
        let method = &schema.methods["resolve_catalog_model_binding"];
        let apply = &schema.methods["apply_catalog_model_binding"];
        assert_eq!(
            apply.required_capability.as_deref(),
            Some("cap.software.tched-router.catalog.selection.set@v1")
        );
        assert_ne!(method.required_capability, apply.required_capability);
        assert_eq!(method.side_effect, op_state_store::SideEffect::Read);
        assert_eq!(apply.side_effect, op_state_store::SideEffect::Mutation);
        assert!(method.idempotent);
        assert!(!apply.idempotent);
        assert_eq!(method.args, apply.args);
        assert_eq!(method.returns, apply.returns);
        assert_eq!(
            method.required_capability.as_deref(),
            Some("cap.software.tched-router.model.binding.read@v1")
        );
        assert!(schema.fields["catalog_model_binding"].read_only);
        let op_state_store::FieldType::JsonSchema(field_schema) =
            &schema.fields["catalog_model_binding"].field_type
        else {
            panic!("catalog binding requires its exact nullable typed schema");
        };
        let field_schema = serde_json::to_value(field_schema.as_ref()).unwrap();
        let field_validator = jsonschema::Validator::new(&field_schema).unwrap();
        assert!(field_validator.is_valid(&serde_json::Value::Null));
        assert!(!schema
            .public_state
            .paths
            .iter()
            .any(|p| p.starts_with("/catalog_model_binding")));
        for extra in ["reference", "path", "agent", "uri", "session_id"] {
            let mut args = serde_json::json!({"provider_id":"openai", "model_id":"test-model"});
            args[extra] = serde_json::json!("untrusted");
            assert!(
                serde_json::from_value::<super::ResolveCatalogModelBindingInput>(args).is_err()
            );
        }
        let returns = serde_json::to_value(method.returns.as_ref().unwrap()).unwrap();
        let validator = jsonschema::Validator::new(&returns).unwrap();
        let mut output = serde_json::json!({"provider_id":"openai", "model_id":"test-model", "reference":"openai.dashboard", "model":"test-model", "uri":"https://inference.example/v1", "catalog_revision":7});
        assert!(validator.is_valid(&output));
        assert!(field_validator.is_valid(&output));
        output["api_key"] = serde_json::json!("secret");
        assert!(!validator.is_valid(&output));
        assert!(!field_validator.is_valid(&output));
        let mut state = super::TchedRouterPlugin::current_state();
        state.catalog_model_binding = Some(super::ResolveCatalogModelBindingOutput {
            provider_id: "PRIVATE_PROVIDER".into(),
            model_id: "PRIVATE_MODEL".into(),
            reference: "openai.private".into(),
            model: "PRIVATE_MODEL".into(),
            uri: "https://private.example/v1".into(),
            oauth: false,
            catalog_revision: 7,
        });
        let observed = super::dispatch_tched_router_method("GetState", "{}", &state).unwrap();
        assert!(!observed.result.to_string().contains("PRIVATE_"));
    }

    #[test]
    fn own_agent_route_has_narrow_typed_admission() {
        let schema = super::tched_router_plugin_schema();
        let method = &schema.methods["get_own_agent_route"];
        assert_eq!(
            method.required_capability.as_deref(),
            Some("cap.software.tched-router.agent.route.self.read@v1")
        );
        let input =
            serde_json::to_value(schemars::schema_for!(super::GetOwnAgentRouteInput)).unwrap();
        assert_eq!(input["additionalProperties"], false);
        for args in [
            serde_json::json!({"agent":"accountability"}),
            serde_json::json!({"path":"providers"}),
            serde_json::json!({"session_id":"other"}),
        ] {
            assert!(serde_json::from_value::<super::GetOwnAgentRouteInput>(args).is_err());
        }
        let output =
            serde_json::to_value(schemars::schema_for!(super::GetOwnAgentRouteOutput)).unwrap();
        let fields = output["properties"].as_object().unwrap();
        assert_eq!(fields.len(), 4);
        for field in ["mode", "reference", "model", "uri"] {
            assert!(fields.contains_key(field));
        }
        let returns = serde_json::to_value(method.returns.as_ref().unwrap()).unwrap();
        let validator = jsonschema::Validator::new(&returns).unwrap();
        let mut observed = serde_json::json!({"mode":"control_plane", "reference":"custom.test", "model":"test-model", "uri":"https://inference.example/v1"});
        assert!(validator.is_valid(&observed));
        observed["api_key"] = serde_json::json!("must-not-be-an-output-field");
        assert!(!validator.is_valid(&observed));
    }

    use super::*;
    use crate::state_plugins::common::oscal::validate_subid;
    use serde_json::Value as JVal;

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
        if let Some(alts) = node
            .get("anyOf")
            .or_else(|| node.get("oneOf"))
            .and_then(JVal::as_array)
        {
            for v in alts {
                collect_subids(v, out);
            }
        }
    }

    #[test]
    fn all_subids_are_valid() {
        let raw = serde_json::to_value(schemars::schema_for!(TchedRouterState)).unwrap();
        let mut subids = Vec::new();
        collect_subids(&raw, &mut subids);
        assert!(!subids.is_empty(), "expected at least one x-oscal-subid");
        for subid in subids {
            validate_subid(&subid).expect("invalid subid: {subid}");
        }
    }

    #[test]
    fn declared_routes_do_not_invent_factory_routing_or_availability() {
        let state = TchedRouterPlugin::current_state();
        assert!(state
            .catalog
            .model_routes
            .iter()
            .all(|route| !route.available));
        assert!(state
            .catalog
            .model_routes
            .iter()
            .all(|route| route.provider != "factory"));
        let factory = state
            .catalog
            .providers
            .iter()
            .find(|provider| provider.id == "factory")
            .unwrap();
        assert_eq!(factory.kind, "provider");
        assert!(factory.aliases.is_empty());
    }

    #[test]
    fn public_schema_accessor_returns_tched_router_schema() {
        let schema = tched_router_plugin_schema();
        assert_eq!(schema.name, PLUGIN_NAME);
        assert_eq!(schema.version, PLUGIN_VERSION);
        assert_eq!(schema.display_name, Some(PLUGIN_DISPLAY_NAME.to_string()));
        assert!(
            schema.capability_grants.is_empty(),
            "plugin schemas declare capabilities but never grant principal authority"
        );
    }

    #[test]
    fn generated_method_docs_include_input_and_output_field_descriptions() {
        let schema = tched_router_plugin_schema();
        let chat = schema.methods.get("Chat").unwrap();
        let chat_returns = serde_json::to_value(chat.returns.as_ref().unwrap()).unwrap();
        assert_eq!(
            chat_returns
                .pointer("/properties/content/description")
                .and_then(JVal::as_str),
            Some("Assistant response text.")
        );

        let list_models = schema.methods.get("ListModels").unwrap();
        let list_args = serde_json::to_value(&list_models.args).unwrap();
        assert_eq!(
            list_args
                .pointer("/properties/provider/description")
                .and_then(JVal::as_str),
            Some("Optional provider id, route, or alias used to filter the model catalog.")
        );
    }

    #[test]
    fn native_config_uses_exact_private_imported_contract() {
        let schema = tched_router_schema();
        assert!(matches!(
            &schema.fields["native_config"].field_type,
            op_state_store::FieldType::JsonSchema(_)
        ));
        let op_state_store::FieldType::JsonSchema(exact) =
            &schema.fields["native_config"].field_type
        else {
            unreachable!();
        };
        assert_eq!(
            exact.as_ref(),
            &simd_json::serde::to_owned_value(
                crate::state_plugins::tched_router_contract::document().clone()
            )
            .unwrap()
        );
        assert!(!schema
            .public_state
            .paths
            .iter()
            .any(|path| path == "/native_config" || path.starts_with("/native_config/")));
    }

    #[test]
    fn every_declared_method_has_a_domain_dispatcher() {
        let schema = tched_router_plugin_schema();
        let state = TchedRouterPlugin::current_state();

        for method in schema.methods.keys() {
            if generation::is_method(method) {
                // MutationEngine owns authenticated private generation state;
                // the synchronous helper must not bypass that owner.
                let error = dispatch_tched_router_method(method, "{}", &state).unwrap_err();
                assert!(error
                    .to_string()
                    .contains("requires admitted native bridge dispatch"));
                continue;
            }
            if method == "Chat" {
                // Chat is declared here but executed by the bridge runtime,
                // which owns provider credentials and the event chain.
                continue;
            }
            if super::NATIVE_CONFIG_METHODS.contains(&method.as_str())
                || matches!(
                    method.as_str(),
                    "config_list"
                        | "get_own_agent_route"
                        | "resolve_catalog_model_binding"
                        | "apply_catalog_model_binding"
                        | "config_get"
                        | "config_set"
                        | "config_patch"
                        | "config_init"
                        | "config_migrate"
                )
            {
                // Config calls require async admitted bridge dispatch to the
                // native loopback API or exact committed protected state.
                continue;
            }
            let args = match method.as_str() {
                "ResolveRoute" => serde_json::json!({ "hint": "balanced" }),
                "ListModels" => serde_json::json!({ "provider": "salad" }),
                "SetProvider" => serde_json::json!({ "provider_id": "salad" }),
                "SetModel" => serde_json::json!({ "model_id": "qwen3.6-27b" }),
                "SetSelection" => serde_json::json!({
                    "provider_id": "salad",
                    "model_id": "qwen3.6-27b"
                }),
                name if name.starts_with("Set") => {
                    serde_json::json!({ "model_id": "qwen3.6-27b" })
                }
                _ => serde_json::json!({}),
            };
            match dispatch_tched_router_method(method, &args.to_string(), &state) {
                Ok(_) => {}
                Err(error) => {
                    let msg = error.to_string();
                    if msg.contains("config section missing")
                        || msg.contains("required field")
                        || msg.contains("unknown field")
                        || msg.contains("execution denied: read")
                    {
                        continue;
                    }
                    panic!("{method} is not executable: {error}");
                }
            }
        }
    }

    #[test]
    fn generic_selection_writes_bind_to_existing_capability_gated_methods() {
        let schema = tched_router_plugin_schema();
        for (field, method_name, argument) in [
            ("selected_provider", "SetProvider", "provider_id"),
            ("selected_model", "SetModel", "model_id"),
        ] {
            assert!(schema.fields.contains_key(field));
            let binding = &schema.field_writes[field];
            assert_eq!(binding.method, method_name);
            assert_eq!(binding.argument, argument);
            let method = &schema.methods[method_name];
            assert_eq!(method.side_effect, op_state_store::SideEffect::Mutation);
            assert!(schema
                .capabilities
                .contains_key(method.required_capability.as_ref().unwrap()));
            let args = serde_json::to_value(&method.args).unwrap();
            assert!(args["properties"].get(argument).is_some());
        }
        assert_eq!(schema.field_writes.len(), 2);
    }

    #[test]
    fn undeclared_method_is_rejected() {
        let error =
            dispatch_tched_router_method("NotDeclared", "{}", &TchedRouterPlugin::current_state())
                .unwrap_err();
        assert!(error.to_string().contains("undeclared method"));
    }
}

// Self-registration: the plugin registry discovers this via inventory
// (single source of the catalog; no central dispatch list).
inventory::submit! {
    crate::default_registry::PluginReg::new(PLUGIN_NAME, |_ctx| std::sync::Arc::new(TchedRouterPlugin::new()))
}

/// Imported upstream version recorded in the plugin-owned contract.
pub fn tched_router_binary_version() -> &'static str {
    super::tched_router_contract::BINARY_SCHEMA_VERSION
}

#[cfg(test)]
mod official_schema_tests {
    //! Guard against truncated upstream imports and missing generated method fields.

    use super::tched_router_binary_version;

    #[test]
    fn imported_contract_is_not_a_stub() {
        let value = super::super::tched_router_contract::document();
        let sections = value
            .get("properties")
            .and_then(|p| p.as_object())
            .expect("imported contract has properties");
        let defs = value
            .get("$defs")
            .and_then(|d| d.as_object())
            .expect("imported contract has $defs");
        assert!(
            sections.len() >= 32,
            "imported contract has only {} sections — import `threetched config schema` \
             into tched_router_contract.rs's UPSTREAM IMPORT block before rebuilding",
            sections.len()
        );
        assert!(
            defs.len() >= 64,
            "imported contract has only {} $defs — looks stubbed or truncated",
            defs.len()
        );

        let gateway = defs
            .get("GatewayConfig")
            .and_then(|g| g.get("properties"))
            .and_then(|p| p.as_object())
            .expect("GatewayConfig is defined with properties");
        for field in [
            "port",
            "host",
            "require_pairing",
            "paired_tokens",
            "session_ttl_hours",
        ] {
            assert!(gateway.contains_key(field), "GatewayConfig missing {field}");
        }
        assert_eq!(
            gateway
                .get("require_pairing")
                .and_then(|v| v.get("type"))
                .and_then(|t| t.as_str()),
            Some("boolean")
        );
    }

    #[test]
    fn imported_contract_records_the_binary_version() {
        let version = tched_router_binary_version();
        assert!(!version.is_empty());
        assert_ne!(
            version, "unknown",
            "record the upstream version during the plugin contract import"
        );
    }
}

#[cfg(test)]
mod config_v3_contract_tests {
    use super::*;

    #[test]
    fn patch_contract_accepts_only_supported_typed_property_operations() {
        let input: ConfigPatchInput = serde_json::from_value(serde_json::json!({
            "operations": [
                {"op":"test", "path":"/gateway/port", "value": 8080},
                {"op":"replace", "path":"/gateway/port", "value": 8081},
                {"op":"remove", "path":"/gateway/public_base_url"}
            ]
        }))
        .expect("supported operations");
        assert_eq!(input.operations.len(), 3);
        assert!(
            serde_json::from_value::<ConfigPatchInput>(serde_json::json!({
                "section": "gateway", "value": {}
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<ConfigPatchInput>(serde_json::json!({
                "operations": [{"op":"comment", "path":"/gateway/port", "comment":"x"}]
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<ConfigPatchInput>(serde_json::json!({
                "operations": [{"op":"move", "path":"/gateway/port"}]
            }))
            .is_err()
        );
    }

    #[test]
    fn property_contract_has_stable_protobuf_fields_for_both_variants() {
        let secret: ConfigPropertyOutput = serde_json::from_value(serde_json::json!({
            "kind": "secret",
            "path": "providers.models.openai.default.api_key",
            "populated": true
        }))
        .expect("populated-only secret");
        assert!(matches!(secret.kind, ConfigPropertyKind::Secret));
        let value: ConfigPropertyOutput = serde_json::from_value(serde_json::json!({
            "kind": "value", "path": "gateway.port", "value": "8080"
        }))
        .expect("display value");
        assert!(matches!(value.kind, ConfigPropertyKind::Value));

        let schema = serde_json::to_value(schemars::schema_for!(ConfigPropertyOutput)).unwrap();
        let properties = schema["properties"]
            .as_object()
            .expect("stable object schema");
        for field in ["kind", "path", "value", "populated", "warnings"] {
            assert!(properties.contains_key(field), "missing {field}");
        }
    }

    #[test]
    fn config_contract_break_bumps_plugin_version() {
        assert_eq!(tched_router_plugin_schema().version, "4.3.0");
    }
}
