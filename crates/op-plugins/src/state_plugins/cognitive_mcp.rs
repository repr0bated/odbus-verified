//! Cognitive MCP state plugin — GB.CognitiveMcp.
//!
//! Declares the cognitive tools compiled into `op-grpc-bridge`. The bridge owns
//! the runtime and publishes its read-only projection to D-Bus under
//! `/opdbus/v1/plugins/cognitive_mcp`; there is no separately supervised
//! cognitive service or plugin-owned listener configuration.
//!
//! The canonical schema (every gRPC method, every MCP tool, every
//! request/response field) lives in the `cognitive_mcp_schema()` function below.

use anyhow::Result;
use async_trait::async_trait;
use op_state::{ApplyResult, Checkpoint, DiffMetadata, PluginCapabilities, StateDiff, StatePlugin};
use op_state_store::PluginSchema;
use op_state_store::SideEffect;
use serde::{Deserialize, Serialize};
use simd_json::OwnedValue as Value;

use super::plugin_scaffold_helpers::method_decl_from_schemars_with_output;

// =============================================================================
// PLUGIN ENTRY: identity and typed schema seed
// =============================================================================

const PLUGIN_NAME: &str = "cognitive_mcp";
const PLUGIN_VERSION: &str = "2.0.0";
const PLUGIN_CATEGORY: &str = "service";
const PLUGIN_DESCRIPTION: &str = "Bridge-owned cognitive MCP tools — memory, code context, and grounded query capabilities compiled into op-grpc-bridge. THE PLUGIN IS THE SCHEMA: every method, tool, property, and field is declared here. Downstream inherits.";
const PLUGIN_DISPLAY_NAME: &str = "GB.CognitiveMcp";

// =============================================================================
// PLUGIN BODY: D-Bus-backed behavior only
// =============================================================================

pub struct CognitiveMcpPlugin;

impl CognitiveMcpPlugin {
    pub fn new() -> Self {
        Self
    }
}

impl Default for CognitiveMcpPlugin {
    fn default() -> Self {
        Self::new()
    }
}

// ── StatePlugin impl ─────────────────────────────────────────────────────────

#[async_trait]
impl StatePlugin for CognitiveMcpPlugin {
    fn name(&self) -> &str {
        PLUGIN_NAME
    }
    fn version(&self) -> &str {
        PLUGIN_VERSION
    }

    fn schema(&self) -> Option<PluginSchema> {
        let mut schema = cognitive_mcp_schema();
        super::common::oscal::ensure_category_metadata_fields(&mut schema);
        Some(schema)
    }

    /// The cognitive runtime is a compiled component of `op-grpc-bridge`.
    ///
    /// Availability is derived from inventory registration plus the bridge's
    /// compile-time dependency, never from a runit directory. This keeps schema
    /// consumers and reflection activation stable after the standalone service
    /// definition is removed.
    fn is_available(&self) -> bool {
        true
    }

    fn unavailable_reason(&self) -> String {
        "cognitive_mcp is compiled into op-grpc-bridge and has no external service dependency"
            .into()
    }

    async fn calculate_diff(&self, current: &Value, desired: &Value) -> Result<StateDiff> {
        // This StatePlugin publishes the bridge-owned projection only. Cognitive
        // mutations are the declared tool methods below and flow through the
        // MutationEngine; there is no declarative service configuration to apply.
        Ok(StateDiff {
            plugin: self.name().into(),
            actions: Vec::new(),
            metadata: DiffMetadata {
                timestamp: chrono::Utc::now().timestamp(),
                current_hash: format!("{:x}", md5::compute(simd_json::to_string(current)?)),
                desired_hash: format!("{:x}", md5::compute(simd_json::to_string(desired)?)),
            },
        })
    }

    async fn apply_state(&self, diff: &StateDiff) -> Result<ApplyResult> {
        let errors = (!diff.actions.is_empty())
            .then(|| {
                "cognitive_mcp state is bridge-owned and read-only; invoke a declared cognitive tool method instead"
                    .to_string()
            })
            .into_iter()
            .collect::<Vec<_>>();
        Ok(ApplyResult {
            success: errors.is_empty(),
            changes_applied: Vec::new(),
            errors,
            checkpoint: None,
        })
    }

    async fn verify_state(&self, _desired: &Value) -> Result<bool> {
        Ok(true)
    }

    async fn create_checkpoint(&self) -> Result<Checkpoint> {
        anyhow::bail!("cognitive_mcp has no plugin-owned mutable state to checkpoint")
    }

    async fn rollback(&self, _checkpoint: &Checkpoint) -> Result<()> {
        anyhow::bail!("cognitive_mcp has no plugin-owned mutable state to roll back")
    }

    fn capabilities(&self) -> PluginCapabilities {
        PluginCapabilities {
            supports_rollback: false,
            supports_checkpoints: false,
            supports_verification: true,
            atomic_operations: false,
        }
    }
}

// ── Schema-derived state for cognitive_mcp ─────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AuthStatus {
    None,
    ChromeProfile,
    Cookie,
    ApiKey,
}

impl Default for AuthStatus {
    fn default() -> Self {
        Self::None
    }
}

fn default_auth_status() -> AuthStatus {
    AuthStatus::default()
}

fn example_auth_status() -> AuthStatus {
    AuthStatus::ChromeProfile
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum GeminiMode {
    Query,
    DeepResearch,
}

impl Default for GeminiMode {
    fn default() -> Self {
        Self::Query
    }
}

fn default_gemini_mode() -> GeminiMode {
    GeminiMode::default()
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum MemoryOperation {
    Store,
    Retrieve,
    Query,
    Delete,
    ListNamespaces,
    Stats,
}

fn example_memory_operation() -> MemoryOperation {
    MemoryOperation::Store
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum NamespaceKind {
    Project,
    Session,
    Database,
    Workflow,
    Agent,
    Cron,
    Custom,
}

fn example_memory_namespace_kind() -> Option<NamespaceKind> {
    Some(NamespaceKind::Project)
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SourceType {
    Url,
    Text,
    File,
}

fn example_source_type() -> SourceType {
    SourceType::Url
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum FileType {
    Source,
    Test,
    Config,
    Docs,
    Build,
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ActivityType {
    ToolCall,
    Query,
    ContextSwitch,
    Error,
    Idle,
    ReturnFromIdle,
    FileOpened,
    EditApplied,
    BuildError,
    TestFailure,
    DiffViewed,
    SymbolNavigated,
}

impl Default for ActivityType {
    fn default() -> Self {
        Self::Query
    }
}

fn default_activity_type() -> ActivityType {
    ActivityType::default()
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum CodeIndexMode {
    Source,
    RepomixZip,
}

impl Default for CodeIndexMode {
    fn default() -> Self {
        Self::Source
    }
}

fn default_code_index_mode() -> CodeIndexMode {
    CodeIndexMode::default()
}

// Defaults matching the bridge-owned schema contract
fn default_bridge_owned() -> bool {
    true
}

fn default_true() -> bool {
    true
}

fn default_execution_model() -> String {
    "in_process".to_string()
}

fn default_queries_remaining() -> i64 {
    0
}

fn default_queries_limit() -> i64 {
    50
}

fn default_notebook_count() -> i64 {
    0
}

fn default_depth() -> u8 {
    3
}

fn default_memory_limit() -> i64 {
    50
}

fn default_code_search_limit() -> u8 {
    8
}

fn default_code_context_limit() -> u8 {
    6
}

fn default_session_id() -> String {
    "default".to_string()
}

fn example_memory_namespace() -> Option<String> {
    Some("project:op-dbus".to_string())
}

fn example_code_search_query() -> String {
    "how is wireguard identity verified".to_string()
}

fn example_code_search_language() -> Option<String> {
    Some("rust".to_string())
}

fn example_collection() -> Option<String> {
    Some("repomix_rag".to_string())
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Citation {
    #[schemars(description = "Cited text passage")]
    pub text: Option<String>,
    #[schemars(description = "Source document identifier")]
    pub source: Option<String>,
    #[schemars(description = "Page or location within source")]
    pub page: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SourceInfo {
    #[schemars(description = "Unique source identifier", extend("readOnly" = true))]
    pub id: String,
    #[schemars(description = "Source title")]
    pub title: Option<String>,
    #[schemars(description = "Source transport type", example = example_source_type())]
    pub source_type: SourceType,
    #[serde(default)]
    #[schemars(description = "Tags attached to this source")]
    pub tags: Vec<String>,
    #[schemars(description = "ISO-8601 creation timestamp", extend("readOnly" = true))]
    pub created_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct GeminiQueryRequest {
    #[schemars(description = "Natural-language query")]
    pub query: String,
    #[schemars(description = "Optional grounding context")]
    pub context: Option<String>,
    #[serde(default = "default_gemini_mode")]
    #[schemars(description = "Query mode")]
    pub mode: GeminiMode,
    #[serde(default = "default_depth")]
    #[schemars(
        range(min = 1, max = 5),
        description = "Deep-research depth (1-5, default 3)"
    )]
    pub depth: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct MemoryToolInput {
    #[schemars(
        description = "Memory operation to perform",
        example = example_memory_operation()
    )]
    pub operation: MemoryOperation,
    #[schemars(
        description = "Namespace name (e.g. project:op-dbus, session:abc)",
        example = example_memory_namespace()
    )]
    pub namespace: Option<String>,
    #[schemars(
        description = "Kind of namespace (used when creating)",
        example = example_memory_namespace_kind()
    )]
    pub namespace_kind: Option<NamespaceKind>,
    #[schemars(description = "Entry key within namespace")]
    pub key: Option<String>,
    #[schemars(description = "Value to store (any JSON)")]
    pub value: Option<serde_json::Value>,
    #[serde(default)]
    #[schemars(description = "Tags for the entry")]
    pub tags: Vec<String>,
    #[schemars(description = "Substring pattern for key search (used in query)")]
    pub key_pattern: Option<String>,
    #[serde(default = "default_memory_limit")]
    #[schemars(description = "Max results (default 50)")]
    pub limit: i64,
}

/// Stable HOT facade for durable memory recall. The operation is fixed by the
/// server and cannot be changed by caller input.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct MemoryRecallInput {
    #[schemars(description = "Optional durable memory namespace")]
    pub namespace: Option<String>,
    #[schemars(description = "Optional substring pattern for entry keys")]
    pub key_pattern: Option<String>,
    #[serde(default)]
    #[schemars(description = "Optional exact tag filters")]
    pub tags: Vec<String>,
    #[serde(default = "default_memory_limit")]
    #[schemars(range(min = 1, max = 200), description = "Maximum matches")]
    pub limit: i64,
}

/// Stable HOT facade for durable memory storage. The operation is fixed to
/// `store` after validation.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct MemoryStoreInput {
    #[schemars(description = "Durable memory namespace")]
    pub namespace: String,
    #[schemars(description = "Namespace kind, used when creating the namespace")]
    pub namespace_kind: Option<NamespaceKind>,
    #[schemars(description = "Entry key within the namespace")]
    pub key: String,
    #[schemars(description = "JSON value to store")]
    pub value: serde_json::Value,
    #[serde(default)]
    #[schemars(description = "Tags attached to the entry")]
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RustProOperation {
    Check,
    Build,
    Test,
    Clippy,
    Format,
    Run,
}

/// Always-loaded Rust workspace agent. The operation is explicit and validated
/// before the bridge dispatches to the eagerly registered Rust Pro executor.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct RustProInput {
    #[schemars(description = "Cargo operation to perform")]
    pub operation: RustProOperation,
    #[schemars(description = "Workspace or crate directory")]
    pub path: String,
    #[serde(default)]
    #[schemars(description = "Use a release profile for build or run")]
    pub release: bool,
}

/// Stable HOT workflow query.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct WorkflowQueryInput {
    #[schemars(description = "Optional exact workflow id")]
    pub workflow_id: Option<String>,
}

/// Stable HOT workflow enqueue request.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct WorkflowRunInput {
    #[schemars(description = "Exact id of a declared workflow")]
    pub workflow_id: String,
    #[serde(default)]
    #[schemars(description = "Typed workflow parameters")]
    pub params: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ToolsetsOperation {
    List,
    Select,
}

/// Stateless HOT toolset operation. Selection is returned to the client and
/// must be repeated in request `_meta`; it never changes server-side grants.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ToolsetsInput {
    pub operation: ToolsetsOperation,
    #[schemars(description = "Required only for the select operation")]
    pub toolset_id: Option<String>,
}

/// Read one exact manifest-pinned PluginSchema from the sealed blob store.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SchemaReadInput {
    #[schemars(
        description = "Sealed plugin id, for example openflow, oscal_subid_registry, or snowball"
    )]
    pub plugin_id: String,
}

/// Query OSCAL routing tags across the manifest-pinned sealed schemas.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct OscalSubidsInput {
    #[schemars(description = "Optional exact plugin id; omit to inspect every sealed plugin")]
    pub plugin_id: Option<String>,
    #[schemars(
        description = "Optional exact subid prefix, for example obs.service or mut.network"
    )]
    pub prefix: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CodeSearchInput {
    #[schemars(
        description = "Natural-language or code query",
        example = example_code_search_query()
    )]
    pub query: String,
    #[schemars(description = "Restrict to a repo name")]
    pub repo: Option<String>,
    #[schemars(
        description = "Restrict to a language (e.g. rust, typescript)",
        example = example_code_search_language()
    )]
    pub language: Option<String>,
    #[schemars(description = "Restrict to a file classification")]
    pub file_type: Option<FileType>,
    #[schemars(description = "Only files whose path contains this substring")]
    pub path_contains: Option<String>,
    #[schemars(description = "Only chunks whose symbols/path contain this substring")]
    pub symbol_contains: Option<String>,
    #[serde(default)]
    #[schemars(description = "Drop test files from results")]
    pub exclude_tests: bool,
    #[serde(default = "default_true")]
    #[schemars(description = "Fuse semantic+lexical scoring and dedup to one chunk per file")]
    pub fused: bool,
    #[schemars(
        description = "Override the Qdrant collection (see qdrant plugin collections) for this search",
        example = example_collection()
    )]
    pub collection: Option<String>,
    #[serde(default = "default_code_search_limit")]
    #[schemars(range(min = 1, max = 50), description = "Max results (default 8)")]
    pub limit: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CodeContextInput {
    #[schemars(description = "Current query / what the agent is working on")]
    pub query: String,
    #[serde(default = "default_session_id")]
    #[schemars(description = "Session identifier (default 'default')")]
    pub session_id: String,
    #[serde(default = "default_activity_type")]
    #[schemars(description = "Kind of activity being recorded (default 'query')")]
    pub activity_type: ActivityType,
    #[schemars(description = "Restrict retrieval to a repo")]
    pub repo: Option<String>,
    #[schemars(description = "Restrict retrieval to a language")]
    pub language: Option<String>,
    #[serde(default)]
    #[schemars(description = "Drop test files from results")]
    pub exclude_tests: bool,
    #[schemars(
        description = "Override the Qdrant collection (see qdrant plugin collections) for this context request",
        example = example_collection()
    )]
    pub collection: Option<String>,
    #[serde(default = "default_code_context_limit")]
    #[schemars(range(min = 1, max = 50), description = "Max results (default 6)")]
    pub limit: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CodeIndexInput {
    #[serde(default = "default_code_index_mode")]
    #[schemars(description = "Indexing mode (default 'source')")]
    pub mode: CodeIndexMode,
    #[schemars(description = "Repo name (source mode)")]
    pub repo: Option<String>,
    #[schemars(description = "File path within the repo (source mode)")]
    pub file_path: Option<String>,
    #[schemars(description = "Raw file content (source mode)")]
    pub content: Option<String>,
    #[schemars(description = "Path to repomix zip (repomix_zip mode)")]
    pub zip_path: Option<String>,
    #[schemars(description = "Entry name within the zip (repomix_zip mode)")]
    pub entry: Option<String>,
    #[schemars(description = "Override target collection")]
    pub collection: Option<String>,
}

/// Typed empty argument set for methods that take no parameters.
///
/// Distinct from `()`, which schemars renders as `{"type":"null"}` and therefore
/// rejects the `{}` that D-Bus/gRPC callers send for a no-argument method. This
/// renders as an object schema, so both a bare `{}` and an absent body validate.
///
/// OSCAL subid: sch.software.cognitive-mcp.no-args@v1
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct NoArgs {}

/// Input for the generic `invoke_tool` method.
///
/// OSCAL subid: sch.software.cognitive-mcp.invoke-tool-input@v1
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct InvokeToolInput {
    #[schemars(
        description = "Name of the tool to invoke; must exist in the cognitive_mcp ToolRegistry (e.g. cognitive_memory, search_blob_vectors)"
    )]
    pub tool_name: String,
    #[serde(default)]
    #[schemars(
        description = "Tool-specific arguments, passed verbatim to the tool executor. Shape is validated by the tool, not by the bridge arg gate."
    )]
    pub arguments: serde_json::Value,
}

/// Output for the generic `invoke_tool` method.
///
/// OSCAL subid: sch.software.cognitive-mcp.invoke-tool-output@v1
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct InvokeToolOutput {
    pub success: bool,
    pub tool_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Output for GetHealth method
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct GetHealthOutput {
    pub bridge_owned: bool,
    pub execution_model: String,
    pub auth_status: String,
    pub queries_remaining: i64,
    pub queries_limit: i64,
}

/// Output for ListTools method
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ListToolsOutput {
    pub tools: Vec<String>,
}

/// Output for RegisterTool method
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct RegisterToolOutput {
    pub success: bool,
    pub tool_name: String,
}

/// Output for MemoryStore method
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct MemoryStoreOutput {
    pub success: bool,
    pub key: String,
}

/// Output for the memory_recall HOT facade.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct MemoryRecallOutput {
    pub results: Vec<serde_json::Value>,
    pub count: usize,
}

/// Output for workflow_query. The concrete entries inherit the authoritative
/// workflow plugin contract.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct WorkflowQueryOutput {
    pub workflows: Vec<serde_json::Value>,
    pub count: usize,
}

/// Output for workflow_run.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct WorkflowRunOutput {
    pub accepted: bool,
    pub execution_id: String,
    pub workflow_id: String,
    pub status: String,
    pub accepted_at: String,
}

/// Public shape of the local toolset response. Detailed set entries and the
/// canonical selector are emitted by the bridge after MutationEngine admission.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ToolsetsOutput {
    pub operation: String,
    pub toolset_generation: u64,
    pub relist_required: bool,
    pub result: serde_json::Value,
}

/// Exact sealed schema document returned by the bridge-owned blob reader.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SchemaReadOutput {
    pub plugin_id: String,
    pub uri: String,
    pub schema_hash: String,
    pub oscal_subid_count: usize,
    pub oscal_subids: Vec<String>,
    pub schema: serde_json::Value,
}

/// One sealed schema in which an OSCAL subid occurs.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct OscalSubidSource {
    pub plugin_id: String,
    pub schema_hash: String,
}

/// Deduplicated OSCAL routing tag and its sealed-schema provenance.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct OscalSubidEntry {
    pub subid: String,
    pub category: String,
    pub sources: Vec<OscalSubidSource>,
}

/// Cross-blob OSCAL routing-tag index.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct OscalSubidsOutput {
    pub plugin_count: usize,
    pub subid_count: usize,
    pub subids: Vec<OscalSubidEntry>,
}

/// Result from the always-loaded Rust Pro executor.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct RustProOutput {
    pub success: bool,
    pub agent_type: String,
    pub agent_name: String,
    pub operation: String,
    pub data: String,
    pub metadata: serde_json::Value,
}

/// Output for MemoryRetrieve method
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct MemoryRetrieveOutput {
    pub success: bool,
    pub value: Option<String>,
    pub namespace: String,
}

/// Output for MemoryQuery method
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct MemoryQueryOutput {
    pub results: Vec<String>,
    pub count: usize,
}

/// Output for MemoryDelete method
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct MemoryDeleteOutput {
    pub success: bool,
    pub keys_deleted: usize,
}

/// Output for MemoryListNamespaces method
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct MemoryListNamespacesOutput {
    pub namespaces: Vec<String>,
}

/// Output for CodeSearch method
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CodeSearchOutput {
    pub results: Vec<String>,
    pub count: usize,
}

/// Output for CodeIndex method
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CodeIndexOutput {
    pub success: bool,
    pub files_indexed: usize,
}

/// Output for CodeContext method
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CodeContextOutput {
    pub context: String,
    pub sources: Vec<String>,
}

/// Output for GeminiQuery method
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct GeminiQueryOutput {
    pub response: String,
    pub citations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.software.plugin.cognitive-mcp.schema@v1"))]
#[schemars(extend("x-oscal-category" = "service"))]
pub struct CognitiveMcpState {
    #[serde(default = "default_bridge_owned")]
    #[schemars(description = "True because the cognitive runtime is compiled into and owned by op-grpc-bridge", extend("readOnly" = true), extend("x-oscal-subid" = "obs.software.plugin.cognitive-mcp.bridge-owned@v1"))]
    pub bridge_owned: bool,
    #[serde(default = "default_execution_model")]
    #[schemars(description = "Cognitive runtime execution model; always in_process for this plugin", extend("readOnly" = true), extend("x-oscal-subid" = "obs.software.plugin.cognitive-mcp.execution-model@v1"))]
    pub execution_model: String,
    #[serde(default = "default_auth_status")]
    #[schemars(description = "Current authentication method", example = example_auth_status(), extend("readOnly" = true), extend("x-oscal-subid" = "obs.software.plugin.cognitive-mcp.auth-status@v1"))]
    pub auth_status: AuthStatus,
    #[serde(default = "default_queries_remaining")]
    #[schemars(description = "Queries remaining in current quota period", extend("readOnly" = true), extend("x-oscal-subid" = "obs.software.plugin.cognitive-mcp.queries-remaining@v1"))]
    pub queries_remaining: i64,
    #[serde(default = "default_queries_limit")]
    #[schemars(description = "Total queries allowed per quota period", extend("readOnly" = true), extend("x-oscal-subid" = "obs.software.plugin.cognitive-mcp.queries-limit@v1"))]
    pub queries_limit: i64,
    #[serde(default = "default_notebook_count")]
    #[schemars(description = "Number of notebooks in the library", extend("readOnly" = true), extend("x-oscal-subid" = "obs.software.plugin.cognitive-mcp.notebook-count@v1"))]
    pub notebook_count: i64,
    #[schemars(description = "R12: Gemini fallback query (requires GEMINI_API_KEY)", extend("readOnly" = true), extend("x-oscal-subid" = "exp.software.plugin.cognitive-mcp.gemini-query-request@v1"))]
    pub gemini_query_request: Option<GeminiQueryRequest>,
    #[schemars(description = "MCP MemoryTool: key-value memory store with operations store/retrieve/query/delete/list_namespaces/stats", extend("readOnly" = true), extend("x-oscal-subid" = "exp.software.plugin.cognitive-mcp.memory-tool@v1"))]
    pub memory_tool: Option<MemoryToolInput>,
    #[schemars(description = "Citation sub-object: text, source, page. Inherited by grounded query responses.", extend("readOnly" = true), extend("x-oscal-subid" = "exp.software.plugin.cognitive-mcp.citation@v1"))]
    pub citation: Option<Citation>,
    #[schemars(description = "SourceInfo sub-object: id, title, source_type, tags, created_at. Inherited by source CRUD responses.", extend("readOnly" = true), extend("x-oscal-subid" = "exp.software.plugin.cognitive-mcp.source-info@v1"))]
    pub source_info: Option<SourceInfo>,
    #[schemars(description = "CodeSearchTool input: semantic+lexical search over the indexed code corpus.", extend("readOnly" = true), extend("x-oscal-subid" = "obs.service.code-rag.search@v1"))]
    pub code_search: Option<CodeSearchInput>,
    #[schemars(description = "CodeContextTool input: activity-aware context retrieval for the current session.", extend("readOnly" = true), extend("x-oscal-subid" = "exp.service.code-context.render@v1"))]
    pub code_context: Option<CodeContextInput>,
    #[schemars(description = "CodeIndexTool input: live single-file or repomix-zip indexing into the code corpus.", extend("readOnly" = true), extend("x-oscal-subid" = "src.software.workspace.index@v1"))]
    pub code_index: Option<CodeIndexInput>,
}

// =============================================================================
// PLUGIN EXIT: publish the single PluginSchema contract
// =============================================================================

// Handler audit — where each method is actually backed:
// get_health              → op-grpc-bridge/mutation_engine.rs bridge-owned projection
// list_tools              → op-grpc-bridge/mutation_engine.rs in-process ToolRegistry
// register_tool           → embedded op_cognitive_mcp::cognitive_tools::RegisterToolTool
//                           (stub — no dynamic tool-registration mechanism exists yet)
// memory_store            → embedded op_cognitive_mcp::cognitive_tools::MemoryTool::op_store
// memory_retrieve         → embedded op_cognitive_mcp::cognitive_tools::MemoryTool::op_retrieve
// memory_query            → embedded op_cognitive_mcp::cognitive_tools::MemoryTool::op_query
// memory_delete           → embedded op_cognitive_mcp::cognitive_tools::MemoryTool::op_delete
// memory_list_namespaces  → embedded op_cognitive_mcp::cognitive_tools::MemoryTool::op_list_namespaces
// code_search             → embedded op_cognitive_mcp::code_tools::CodeSearchTool
// code_index              → embedded op_cognitive_mcp::code_tools::CodeIndexTool
// code_context            → embedded op_cognitive_mcp::code_tools::CodeContextTool
// gemini_query            → embedded op_cognitive_mcp::cognitive_tools::AskQuestionTool
// invoke_tool             → op-grpc-bridge/mutation_engine.rs in-process ToolRegistry
// memory_recall           → cognitive_memory query operation (HOT facade)
// workflow_query          → workflows.query_workflows (HOT facade)
// workflow_run            → workflows.start_workflow (HOT facade)
// rust_pro                → eagerly registered agent_rust_pro_<operation> tool (HOT)
// toolsets                → bridge-local deterministic projection policy (HOT)
// schema_read             → exact manifest-pinned sealed PluginSchema (HOT)
// oscal_subids            → cross-blob OSCAL routing-tag index (HOT)
pub(crate) fn cognitive_mcp_schema() -> PluginSchema {
    let root = serde_json::to_value(schemars::schema_for!(CognitiveMcpState))
        .expect("schemars schema serializes to JSON");
    let mut schema = super::schemars_adapter::plugin_schema_from_json(
        PLUGIN_NAME,
        PLUGIN_VERSION,
        PLUGIN_DESCRIPTION,
        &root,
    );
    schema.category = PLUGIN_CATEGORY.to_string();
    schema.display_name = Some(PLUGIN_DISPLAY_NAME.to_string());

    // Add methods
    use super::plugin_scaffold_helpers::method_decl_from_schemars_with_output;

    schema.methods.insert(
        "get_health".to_string(),
        method_decl_from_schemars_with_output::<NoArgs, GetHealthOutput>(
            "get_health",
            SideEffect::Read,
            true,
            "cognitive_mcp.read",
            "obs.service.cognitive-mcp.health@v1",
        ),
    );
    schema.methods.insert(
        "list_tools".to_string(),
        method_decl_from_schemars_with_output::<NoArgs, ListToolsOutput>(
            "list_tools",
            SideEffect::Read,
            true,
            "cognitive_mcp.read",
            "obs.service.cognitive-mcp.tool.list@v1",
        ),
    );
    schema.methods.insert(
        "register_tool".to_string(),
        method_decl_from_schemars_with_output::<(), RegisterToolOutput>(
            "register_tool",
            SideEffect::Mutation,
            false,
            "cognitive_mcp.invoke",
            "mut.service.cognitive-mcp.tool.register@v1",
        ),
    );
    schema.methods.insert(
        "memory_store".to_string(),
        method_decl_from_schemars_with_output::<MemoryStoreInput, MemoryStoreOutput>(
            "memory_store",
            SideEffect::Mutation,
            false,
            "cognitive_mcp.memory.write",
            "mut.service.cognitive-mcp.memory.store@v1",
        ),
    );
    schema.methods.insert(
        "memory_recall".to_string(),
        method_decl_from_schemars_with_output::<MemoryRecallInput, MemoryRecallOutput>(
            "memory_recall",
            SideEffect::Read,
            true,
            "cognitive_mcp.read",
            "obs.service.cognitive-mcp.memory.recall@v1",
        ),
    );
    schema.methods.insert(
        "memory_retrieve".to_string(),
        method_decl_from_schemars_with_output::<(), MemoryRetrieveOutput>(
            "memory_retrieve",
            SideEffect::Read,
            true,
            "cognitive_mcp.read",
            "obs.service.cognitive-mcp.memory.retrieve@v1",
        ),
    );
    schema.methods.insert(
        "memory_query".to_string(),
        method_decl_from_schemars_with_output::<(), MemoryQueryOutput>(
            "memory_query",
            SideEffect::Read,
            true,
            "cognitive_mcp.read",
            "obs.service.cognitive-mcp.memory.query@v1",
        ),
    );
    schema.methods.insert(
        "memory_delete".to_string(),
        method_decl_from_schemars_with_output::<(), MemoryDeleteOutput>(
            "memory_delete",
            SideEffect::Mutation,
            true,
            "cognitive_mcp.invoke",
            "mut.service.cognitive-mcp.memory.delete@v1",
        ),
    );
    schema.methods.insert(
        "memory_list_namespaces".to_string(),
        method_decl_from_schemars_with_output::<(), MemoryListNamespacesOutput>(
            "memory_list_namespaces",
            SideEffect::Read,
            true,
            "cognitive_mcp.read",
            "obs.service.cognitive-mcp.memory.namespace.list@v1",
        ),
    );
    schema.methods.insert(
        "code_search".to_string(),
        method_decl_from_schemars_with_output::<CodeSearchInput, CodeSearchOutput>(
            "code_search",
            SideEffect::Read,
            true,
            "cognitive_mcp.read",
            "obs.service.code-rag.search@v1",
        ),
    );
    schema.methods.insert(
        "code_index".to_string(),
        method_decl_from_schemars_with_output::<CodeIndexInput, CodeIndexOutput>(
            "code_index",
            SideEffect::Mutation,
            false,
            "cognitive_mcp.code.write",
            "mut.service.code-rag.index@v1",
        ),
    );
    schema.methods.insert(
        "code_context".to_string(),
        method_decl_from_schemars_with_output::<CodeContextInput, CodeContextOutput>(
            "code_context",
            SideEffect::Read,
            true,
            "cognitive_mcp.read",
            "obs.service.code-context.get@v1",
        ),
    );
    schema.methods.insert(
        "rust_pro".to_string(),
        method_decl_from_schemars_with_output::<RustProInput, RustProOutput>(
            "rust_pro",
            SideEffect::Mutation,
            false,
            "cognitive_mcp.invoke",
            "mut.software.rust-pro.execute@v1",
        ),
    );
    schema.methods.insert(
        "workflow_query".to_string(),
        method_decl_from_schemars_with_output::<WorkflowQueryInput, WorkflowQueryOutput>(
            "workflow_query",
            SideEffect::Read,
            true,
            "workflows.read",
            "obs.service.cognitive-mcp.workflow.query@v1",
        ),
    );
    schema.methods.insert(
        "workflow_run".to_string(),
        method_decl_from_schemars_with_output::<WorkflowRunInput, WorkflowRunOutput>(
            "workflow_run",
            SideEffect::Mutation,
            false,
            "workflows.write",
            "mut.service.cognitive-mcp.workflow.run@v1",
        ),
    );
    schema.methods.insert(
        "toolsets".to_string(),
        method_decl_from_schemars_with_output::<ToolsetsInput, ToolsetsOutput>(
            "toolsets",
            SideEffect::Read,
            true,
            "cognitive_mcp.read",
            "obs.service.cognitive-mcp.toolset.project@v1",
        ),
    );
    schema.methods.insert(
        "schema_read".to_string(),
        method_decl_from_schemars_with_output::<SchemaReadInput, SchemaReadOutput>(
            "schema_read",
            SideEffect::Read,
            true,
            "cognitive_mcp.read",
            "obs.service.cognitive-mcp.schema.read@v1",
        ),
    );
    schema.methods.insert(
        "oscal_subids".to_string(),
        method_decl_from_schemars_with_output::<OscalSubidsInput, OscalSubidsOutput>(
            "oscal_subids",
            SideEffect::Read,
            true,
            "cognitive_mcp.read",
            "obs.standard.oscal-subid-registry.subid.list@v1",
        ),
    );
    schema.methods.insert(
        "gemini_query".to_string(),
        method_decl_from_schemars_with_output::<(), GeminiQueryOutput>(
            "gemini_query",
            SideEffect::Mutation,
            false,
            "cognitive_mcp.invoke",
            "mut.service.gemini.query@v1",
        ),
    );
    // Generic tool invocation. The 12 methods above are fixed at blob-seal time, but
    // the ToolRegistry is populated at runtime (406 tools today), so a per-tool method
    // is impossible without re-sealing on every registration. `invoke_tool` is the one
    // typed door to the whole registry: `tool_name` selects the tool and `arguments` is
    // passed verbatim for the tool itself to validate.
    schema.methods.insert(
        "invoke_tool".to_string(),
        method_decl_from_schemars_with_output::<InvokeToolInput, InvokeToolOutput>(
            "invoke_tool",
            SideEffect::Mutation,
            false,
            "cognitive_mcp.invoke",
            "mut.service.cognitive-mcp.tool.invoke@v1",
        ),
    );

    schema.capabilities.insert(
        "cognitive_mcp.read".to_string(),
        op_state_store::CapabilityDecl {
            id: "cognitive_mcp.read".to_string(),
            description: "Grants bridge-owned cognitive reads: get_health, list_tools, memory_retrieve, memory_query, memory_list_namespaces, code_search, code_context, schema_read, oscal_subids.".to_string(),
        },
    );
    schema.capabilities.insert(
        "cognitive_mcp.invoke".to_string(),
        op_state_store::CapabilityDecl {
            id: "cognitive_mcp.invoke".to_string(),
            description: "Grants declared bridge-owned cognitive and agent execution methods; it does not authorize the generic invoke_tool escape hatch on the MCP projection.".to_string(),
        },
    );
    schema.capabilities.insert(
        "cognitive_mcp.memory.write".to_string(),
        op_state_store::CapabilityDecl {
            id: "cognitive_mcp.memory.write".to_string(),
            description: "Grants the sealed HOT memory_store method.".to_string(),
        },
    );
    schema.capabilities.insert(
        "cognitive_mcp.code.write".to_string(),
        op_state_store::CapabilityDecl {
            id: "cognitive_mcp.code.write".to_string(),
            description: "Grants the sealed context-aware code_index method.".to_string(),
        },
    );
    schema.capabilities.insert(
        "workflows.read".to_string(),
        op_state_store::CapabilityDecl {
            id: "workflows.read".to_string(),
            description: "Grants the HOT workflow query facade.".to_string(),
        },
    );
    schema.capabilities.insert(
        "workflows.write".to_string(),
        op_state_store::CapabilityDecl {
            id: "workflows.write".to_string(),
            description: "Grants the HOT workflow enqueue facade.".to_string(),
        },
    );

    schema
}

#[cfg(test)]
mod tests {
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

    #[test]
    fn all_subids_are_valid() {
        let raw = serde_json::to_value(schemars::schema_for!(CognitiveMcpState)).unwrap();
        let mut subids = Vec::new();
        collect_subids(&raw, &mut subids);
        assert!(
            !subids.is_empty(),
            "expected at least one x-oscal-subid in the derived schema"
        );
        for subid in subids {
            validate_subid(&subid).expect("invalid subid: {subid}");
        }
    }

    #[test]
    fn bridge_owned_plugin_is_available_without_a_service_definition() {
        let plugin = CognitiveMcpPlugin::new();
        assert!(plugin.is_available());
        assert!(plugin
            .unavailable_reason()
            .contains("compiled into op-grpc-bridge"));
    }

    #[test]
    fn schema_excludes_retired_standalone_service_controls() {
        let schema = CognitiveMcpPlugin::new()
            .schema()
            .expect("cognitive_mcp must publish its bridge-owned schema");

        for method in ["get_config", "set_config", "restart_service"] {
            assert!(
                !schema.methods.contains_key(method),
                "retired standalone management method {method} leaked into the schema"
            );
        }
        for field in [
            "http",
            "grpc",
            "wg_interface",
            "http_enabled",
            "grpc_enabled",
            "dbus_enabled",
            "running",
        ] {
            assert!(
                !schema.fields.contains_key(field),
                "retired standalone transport field {field} leaked into the schema"
            );
        }

        let expected_methods = [
            "get_health",
            "list_tools",
            "register_tool",
            "memory_store",
            "memory_recall",
            "memory_retrieve",
            "memory_query",
            "memory_delete",
            "memory_list_namespaces",
            "code_search",
            "code_index",
            "code_context",
            "rust_pro",
            "workflow_query",
            "workflow_run",
            "toolsets",
            "schema_read",
            "oscal_subids",
            "gemini_query",
            "invoke_tool",
        ];
        assert_eq!(
            schema.methods.len(),
            expected_methods.len(),
            "sealed bridge-owned cognitive method inventory drifted"
        );
        for method in expected_methods {
            assert!(
                schema.methods.contains_key(method),
                "bridge-owned cognitive method {method} must remain sealed"
            );
        }

        let rust_pro = &schema.methods["rust_pro"];
        assert_eq!(
            rust_pro.required_capability.as_deref(),
            Some("cognitive_mcp.invoke")
        );
        assert_eq!(rust_pro.side_effect, SideEffect::Mutation);

        let schema_read = &schema.methods["schema_read"];
        assert_eq!(
            schema_read.required_capability.as_deref(),
            Some("cognitive_mcp.read")
        );
        assert_eq!(
            schema_read.subid,
            "obs.service.cognitive-mcp.schema.read@v1"
        );

        let oscal_subids = &schema.methods["oscal_subids"];
        assert_eq!(
            oscal_subids.subid,
            "obs.standard.oscal-subid-registry.subid.list@v1"
        );

        let encoded = serde_json::to_string(&schema).expect("schema serializes");
        for retired_signal in ["3003", "50052", "netmaker", "op-cognitive-mcp"] {
            assert!(
                !encoded.contains(retired_signal),
                "retired standalone signal {retired_signal} leaked into the schema"
            );
        }
    }

    #[tokio::test]
    async fn state_plugin_cannot_apply_service_configuration() {
        let plugin = CognitiveMcpPlugin::new();
        let diff = plugin
            .calculate_diff(
                &simd_json::json!({"bridge_owned": true}),
                &simd_json::json!({"http": "127.0.0.1:1"}),
            )
            .await
            .expect("read-only diff calculation succeeds");
        assert!(diff.actions.is_empty());

        let forged = StateDiff {
            plugin: PLUGIN_NAME.to_string(),
            actions: vec![op_state::StateAction::Modify {
                resource: "http".to_string(),
                changes: simd_json::json!("127.0.0.1:1"),
            }],
            metadata: diff.metadata,
        };
        let result = plugin
            .apply_state(&forged)
            .await
            .expect("read-only rejection is an ApplyResult");
        assert!(!result.success);
        assert!(result.changes_applied.is_empty());
        assert!(result.errors[0].contains("bridge-owned and read-only"));

        let capabilities = plugin.capabilities();
        assert!(!capabilities.supports_checkpoints);
        assert!(!capabilities.supports_rollback);
    }
}

// Self-registration: the plugin registry discovers this via inventory
// (single source of registered plugins; no central dispatch list).
inventory::submit! {
    crate::default_registry::PluginReg::new(PLUGIN_NAME, |_ctx| std::sync::Arc::new(CognitiveMcpPlugin::new()))
}
