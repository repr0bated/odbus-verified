//! Mutation Engine - The Authoritative Source for State and Schema DNA
//!
//! The Mutation Engine is the central coordinator that:
//! - Authoritatively routes all mutations (gRPC and D-Bus)
//! - Ensures all state changes are strictly recorded in the Event Chain (Audit Log)
//! - Broadcasts authoritative state changes to gRPC subscribers
//! - Directly manages authoritative RCP stores (OVSDB, NonNet, SQLite)

#[path = "identity_publication.rs"]
mod identity_publication;
#[path = "method_receipt_audit.rs"]
mod method_receipt_audit;
#[path = "owned_method_task.rs"]
mod owned_method_task;

#[cfg(test)]
#[path = "proxy_canonical_tests.rs"]
mod proxy_canonical_tests;

#[cfg(test)]
#[path = "antigravity_canonical_tests.rs"]
mod antigravity_canonical_tests;
#[cfg(test)]
#[path = "mail_canonical_tests.rs"]
mod mail_canonical_tests;
#[cfg(test)]
#[path = "obfuscation_canonical_tests.rs"]
mod obfuscation_canonical_tests;

use anyhow::Context as _;
use async_trait::async_trait;
use serde_json;
use simd_json::prelude::{ValueAsContainer, ValueAsScalar, ValueObjectAccess};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::{Mutex, OnceCell, RwLock, Semaphore, broadcast};
use zbus::zvariant::OwnedValue as ZOwnedValue;
use zbus::{Connection, Proxy};

use op_cognitive_mcp::CognitiveMcpServer;
use op_llm::chat::ChatManager;
use op_network::rovs_proxy::OvsdbDbusClient;
use op_plugins::state_plugins::snowball_plugin::{
    AuditEventRecord, QueryEventsInput, QueryEventsOutput, VerifyChainInput, VerifyChainOutput,
};
use op_snowball::{OptimizedSnowball, PluginFootprint, StreamingSnowball};
use op_state_store::{ChainEvent, Decision, EventChain, OperationType};

/// Default on-disk location of the streaming snowball that backs the durable
/// audit trail, when `$OPDBUS_SNOWBALL_PATH` is unset. Matches
/// `snowball_plugin::DEFAULT_BASE_PATH` so both read the same chain.
const DEFAULT_SNOWBALL_PATH: &str = "/var/lib/opdbus/snowball";
const DEFAULT_CACHE_PATH: &str = "/var/lib/opdbus/cache";

fn completed_effect_time(
    started_at: chrono::DateTime<chrono::Utc>,
    elapsed: std::time::Duration,
) -> chrono::DateTime<chrono::Utc> {
    let elapsed = chrono::Duration::from_std(elapsed).unwrap_or_default();
    started_at.checked_add_signed(elapsed).unwrap_or(started_at)
}

#[cfg(test)]
mod completed_effect_time_tests {
    use super::completed_effect_time;

    #[test]
    fn completion_time_is_derived_from_monotonic_elapsed_time() {
        let started = chrono::DateTime::parse_from_rfc3339("2026-09-14T12:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        assert_eq!(
            completed_effect_time(started, std::time::Duration::from_millis(1_250)),
            started + chrono::Duration::milliseconds(1_250)
        );
        assert_eq!(
            completed_effect_time(
                chrono::DateTime::<chrono::Utc>::MAX_UTC,
                std::time::Duration::from_secs(1)
            ),
            chrono::DateTime::<chrono::Utc>::MAX_UTC
        );
    }
}

/// A state change projected from the authoritative system bus
#[derive(Debug, Clone)]
pub struct StateChange {
    pub change_id: String,
    pub event_id: u64,
    pub plugin_id: String,
    pub object_path: String,
    pub change_type: ChangeType,
    pub member_name: Option<String>,
    pub old_value: Option<simd_json::OwnedValue>,
    pub new_value: simd_json::OwnedValue,
    pub tags_touched: Vec<String>,
    pub event_hash: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub actor_id: String,
    pub source: ChangeSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeType {
    PropertySet,
    PropertyDelete,
    MethodCall,
    Signal,
    ObjectAdded,
    ObjectRemoved,
    SchemaMigration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeSource {
    DBus,
    Grpc,
    Internal,
}

// This record is constructed only after the canonical compiled/sealed method
// and argument admission. It carries the exact previously verified identity
// metadata; it does not mint identity or grant a capability. Never derive Debug
// over its opaque request arguments.
struct AdmittedMethodCall {
    plugin_id: String,
    method: String,
    json_args: String,
    capability_id: Option<String>,
    actor_id: String,
    session_id: Option<String>,
    session_genesis: Option<String>,
    parsed_value: simd_json::OwnedValue,
    admitted_schema: op_state_store::PluginSchema,
    generation_origin: Option<crate::generation_admission::GenerationOrigin>,
    facade_origin: Option<crate::tched_router_admission::FacadeOrigin>,
    bound_oracle: Option<crate::oracle_assertion::BoundOracleProof>,
    change: StateChange,
}

fn ensure_own_route_context_unchanged(
    before: &op_plugins::state_plugins::project::ResolvedChatContext,
    after: &op_plugins::state_plugins::project::ResolvedChatContext,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        serde_json::to_value(before)? == serde_json::to_value(after)?,
        "own route caller context changed during observation"
    );
    Ok(())
}

fn require_bound_oracle_grant(actor: &str, capability: Option<&str>) -> anyhow::Result<()> {
    let capability = capability.context("bound oracle method requires its declared capability")?;
    anyhow::ensure!(
        crate::interceptor::load_capability_grants(actor).contains(capability),
        "bound oracle method capability is no longer granted"
    );
    Ok(())
}

fn audit_method_arguments(
    plugin_id: &str,
    method: &str,
    json_args: &str,
    args_schema: Option<&serde_json::Value>,
) -> String {
    if plugin_id == "elevation" && method == "authorize_task" {
        // Even a digest of this request would let audit readers test password
        // guesses offline. Retain event attribution and the action result,
        // but neither the password nor any password-derived request hash.
        return serde_json::json!({ "arguments_redacted": true }).to_string();
    }
    // OD-91: when the sealed method args schema is available, drop `x-secret`
    // and replace `x-opdbus-private` leaves with blake3 digests. Fall back to
    // the plugin allowlist when the schema has no private markers (or is absent).
    if let Some(schema) = args_schema
        && let Ok(value) = serde_json::from_str::<serde_json::Value>(json_args)
    {
        let digested =
            op_state_store::schema_projection::commit_with_private_digests(schema, &value);
        let digested_str = digested.to_string();
        let value_str = value.to_string();
        if digested_str != value_str {
            return digested_str;
        }
    }
    let private = matches!(
        plugin_id,
        "project"
            | "cognitive_mcp"
            | "workflows"
            | "persona"
            | "memory"
            | "cozo"
            | "sess_decl"
            | "notebooklm"
            | "cron"
            | "web_ui"
            | "ctl_plane_chatbot"
            | "chat_response"
            | "elevation"
            | "mail_server"
            | "wireguard"
            | "wg_opdbus"
            | "wgcf"
            | "antigravity"
            | "antigravity_chat"
    ) || (plugin_id == "xray" && matches!(method, "add_user" | "add_inbound"))
        || (plugin_id == "tched_router"
            && (matches!(
                method,
                "Chat" | "PatchConfig" | "config_set" | "config_patch"
            ) || crate::generation_jobs_dispatch::is_generation_method(method)))
        || (plugin_id == "large_language_model" && method == "stream_generate")
        || (plugin_id == "embedding_model" && method == "embed")
        || (plugin_id == "mcp" && method == "configure_server")
        || (plugin_id == "gemma_brain"
            && crate::ui_workspace_dispatch::is_ui_workspace_method(method))
        || (plugin_id == "json_render"
            && crate::json_render_gallery_dispatch::is_gallery_method(method));
    if private {
        serde_json::json!({
            "arguments_redacted": true,
            "request_hash": blake3::hash(json_args.as_bytes()).to_hex().to_string()
        })
        .to_string()
    } else {
        json_args.to_string()
    }
}

// Clones are handles to the same engine: every mutable owner, lock and lazy
// service below is shared. Owned backend work can outlive its requesting RPC.
#[derive(Clone)]
pub struct MutationEngine {
    /// Authoritative Event Chain
    pub event_chain: Arc<RwLock<EventChain>>,
    /// Real-time change projection channel
    change_tx: broadcast::Sender<StateChange>,
    /// Exact appended audit events for EventChainService subscribers.
    chain_tx: broadcast::Sender<ChainEvent>,
    /// Working state mirror, updated only after an SHM commit.
    state_cache: Arc<RwLock<HashMap<String, simd_json::OwnedValue>>>,
    /// Keep local commit, mirror update and notification ordered, including
    /// when the requesting future is cancelled during filesystem work.
    state_publication: Arc<Mutex<()>>,
    identity_sled_store: Arc<OnceCell<crate::identity_sled_dispatch::IdentitySledStore>>,
    nextdns_bindings: Arc<OnceCell<crate::identity_nextdns::NextDnsBindings>>,
    /// Serialize native host observation and its commit so an earlier slow
    /// read cannot overwrite a later host snapshot.
    host_observation: Arc<Mutex<()>>,
    observers_started: Arc<std::sync::atomic::AtomicBool>,
    incus_observer_started: Arc<std::sync::atomic::AtomicBool>,
    cron_observer_started: Arc<std::sync::atomic::AtomicBool>,
    /// System D-Bus connection authority
    pub dbus_connection: Arc<OnceCell<Connection>>,
    /// Session bus connection for projection tree introspection
    session_bus: Arc<OnceCell<Connection>>,
    /// Session bus connection with registered PluginV1 interfaces for signal emission.
    /// Set after SchemaRouter registers objects (server.rs spawned task).
    signal_bus: Arc<OnceCell<Connection>>,
    /// Resource limiter for D-Bus operations
    #[allow(dead_code)]
    dbus_call_limiter: Arc<Semaphore>,

    /// Durable audit sink: the streaming snowball's `timing_subvol` holds one
    /// JSON record per event chain event, so the trail survives a restart. The
    /// optimized form also mirrors footprints into the NUMA-aware Btrfs cache;
    /// cache initialization is best-effort and never gates the timing trail.
    /// Empty until [`MutationEngine::init_audit_durability`] runs; a missing
    /// sink prevents admitted methods from executing; legacy non-method audit
    /// paths retain their existing best-effort persistence behavior.
    audit_sink: Arc<OnceCell<AuditSink>>,
    /// An uncertain durable append forbids further method effects until restart
    /// replays the actual timing trail. No speculative chain-head recovery.
    method_audit_uncertain: Arc<std::sync::atomic::AtomicBool>,
    method_journal_documents:
        Arc<OnceCell<Arc<op_plugins::state_plugins::runtime_documents::RuntimeDocuments>>>,
    /// Vectorize on mutation (OD-64): woken after every audit write.
    vector_projector: Arc<OnceCell<crate::chain_vector_projector::ChainVectorProjector>>,

    /// Authoritative RCP stores
    pub ovsdb: Arc<OvsdbDbusClient>,
    /// Forwarders from caller-selected native monitor sessions into protected
    /// SHM children, keyed by an opaque caller/monitor digest.
    ovsdb_monitor_tasks: Arc<Mutex<HashMap<String, tokio::task::JoinHandle<()>>>>,
    ovsdb_monitor_state: Arc<Mutex<()>>,
    /// Provider runtime used only after ZeroClaw resolves a schema-declared route.
    #[allow(dead_code)] // wired for chat dispatch; not yet called from dispatch loop
    chat_manager: Arc<ChatManager>,
    /// Lazily initialized cognitive runtime.  It is owned by the bridge and is
    /// reached only after a call has entered PluginService/MutationEngine; it
    /// never opens a second listener or becomes a parallel control plane.
    cognitive_mcp: Arc<OnceCell<Arc<CognitiveMcpServer>>>,
    /// Knowledge pushes produced by the context engine, waiting to be
    /// consumed by the next chatbot turn or `specialist` call of that session.
    knowledge_inbox: crate::knowledge_inbox::KnowledgeInbox,
    chatbot_episode_cozo: Arc<OnceCell<Arc<op_cozo_store::CozoGraphShuttle>>>,
    /// The control plane's json_render UI gallery and active catalog owner:
    /// one system database, read and written only through grants.
    ui_gallery_cozo: Arc<OnceCell<Arc<op_cozo_store::CozoGraphShuttle>>>,
    /// The system-wide model registry: providers' refresh status and the
    /// models each one lists, one system database behind grants.
    model_registry_cozo: Arc<OnceCell<Arc<op_cozo_store::CozoGraphShuttle>>>,
    /// One-time sudo elevation tickets for protected tasks.
    task_elevation: Arc<crate::task_elevation::TaskElevation>,
    /// Private memory methods share one writer lock and the existing Cozo
    /// handle; request context determines their principal/session namespace.
    memory_methods: Arc<OnceCell<op_plugins::state_plugins::memory_methods::MemoryMethods>>,
    sess_decl_methods: Arc<OnceCell<op_plugins::state_plugins::sessdecl::SessDeclMethods>>,
    notebook_queries: Arc<crate::notebooklm_queries::NotebookQueries>,
    snowball_methods: Arc<OnceCell<op_plugins::state_plugins::snowball_methods::SnowballMethods>>,
    /// One process-shared durable/private publication owner. It has no global
    /// state cache or broadcast path.
    ui_workspace: Arc<OnceCell<Arc<crate::ui_workspace_dispatch::UiWorkspacePublisher>>>,
    chatbot_episodes: Arc<OnceCell<Arc<crate::chatbot_episode_state::EpisodePublisher>>>,
    /// Private principal-scoped chat_response conversations: their dedicated
    /// Cozo owner, private SHM publisher, and the bridge's turn registry.
    chat_response_cozo: Arc<OnceCell<Arc<op_cozo_store::CozoGraphShuttle>>>,
    chat_responses: Arc<OnceCell<Arc<crate::chat_response_state::ChatResponsePublisher>>>,
    chat_turns: Arc<crate::chat_response_turns::TurnRegistry>,
    generation_jobs: Arc<OnceCell<Arc<crate::generation_jobs_dispatch::GenerationPublisher>>>,
    /// Supplied by the serving bridge at startup; metadata registrations never
    /// choose or bind their own listener path.
    shared_socket_path: Arc<OnceCell<String>>,
    /// The same listener controller consumed by tonic, installed by the bridge.
    shared_listener: Arc<OnceCell<Arc<crate::shared_socket::SharedListenerController>>>,
    shared_listener_projection: Arc<tokio::sync::Mutex<()>>,
    proxy_methods: Arc<OnceCell<Arc<crate::proxy_methods::ProxyMethods>>>,
    proxy_projection: Arc<Mutex<()>>,
    obfuscation_methods: Arc<OnceCell<Arc<crate::obfuscation_methods::ObfuscationMethods>>>,
    obfuscation_projection: Arc<Mutex<()>>,
    openflow_projection: Arc<Mutex<()>>,
    /// Isolated tests may supply a peer D-Bus connection to the canonical
    /// OpenFlow owner. Production leaves this empty and uses the system bus.
    openflow_method_connection: Arc<OnceCell<Connection>>,
    mcp_policy_owner: Arc<OnceCell<Arc<crate::mcp_policy::McpPolicyOwner>>>,
    antigravity_documents:
        Arc<OnceCell<Arc<op_plugins::state_plugins::runtime_documents::RuntimeDocuments>>>,
    antigravity_credentials: Arc<OnceCell<Arc<op_llm::antigravity::GoogleCredentialOwner>>>,
    antigravity_projection: Arc<Mutex<()>>,
    mail_projection: Arc<Mutex<()>>,
    decoy_network_projection: Arc<Mutex<()>>,
    /// Verified session identities, keyed by session_id. Projection of the
    /// session records for the mutation path — not a second store.
    sessions: Arc<RwLock<HashMap<String, SessionContext>>>,
    /// Last time a principal presented its own credential. Host-service calls
    /// do not update this. A disconnect that hangs leaves the stamp behind,
    /// and the unattended pass uses its age.
    principal_logins: Arc<RwLock<HashMap<String, i64>>>,
}

enum AuditSink {
    Optimized(Arc<OptimizedSnowball>),
    TimingOnly(Arc<StreamingSnowball>),
}

impl AuditSink {
    fn streaming_snowball(&self) -> Arc<StreamingSnowball> {
        match self {
            Self::Optimized(sink) => sink.snowball().clone(),
            Self::TimingOnly(sink) => sink.clone(),
        }
    }

    async fn add_footprint(&self, footprint: PluginFootprint) -> anyhow::Result<String> {
        match self {
            Self::Optimized(sink) => sink.add_footprint(footprint).await,
            Self::TimingOnly(sink) => sink.add_footprint(footprint).await,
        }
    }
}

impl std::fmt::Debug for MutationEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MutationEngine").finish()
    }
}

#[async_trait]
impl op_core::state_publisher::StatePublisher for MutationEngine {
    async fn publish_change(
        &self,
        plugin_id: String,
        path: String,
        change_type: op_core::state_publisher::ChangeType,
        property: Option<String>,
        old_value: Option<simd_json::OwnedValue>,
        new_value: simd_json::OwnedValue,
        tags: Vec<String>,
        source: String,
    ) -> anyhow::Result<()> {
        let internal_type = match change_type {
            op_core::state_publisher::ChangeType::PropertySet => ChangeType::PropertySet,
            op_core::state_publisher::ChangeType::Signal => ChangeType::Signal,
            op_core::state_publisher::ChangeType::Deleted if property.is_some() => {
                ChangeType::PropertyDelete
            }
            op_core::state_publisher::ChangeType::Deleted => ChangeType::ObjectRemoved,
        };

        self.process_authoritative_change(
            plugin_id,
            path,
            internal_type,
            property,
            old_value,
            new_value,
            tags,
            source,
            None,
            ChangeSource::Internal,
        )
        .await
        .map(|_| ())
        .map_err(|e| anyhow::anyhow!(e))
    }
}

/// Session genesis stamp used by identity_sled_dispatch.
#[derive(Debug, Clone, Default)]
pub struct GenesisStamp {
    pub session_id: String,
    pub wireguard_pubkey: String,
    pub genesis_hex: String,
    pub trace_id: String,
    /// Complete MutationEngine-authored SID1 envelope, embedded inline in the
    /// sled's `sealed_id` and forwarded unchanged by SID1-aware clients.
    pub sealed_id: String,
    pub arrival_timestamp: i64,
    pub chain_head_at_arrival: String,
    pub catalog_hash_at_arrival: String,
    pub head_timestamp_at_arrival: i64,
}

/// Verified session identity stamped on the mutation path.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionContext {
    pub genesis_hex: String,
    pub session_id: String,
    pub wireguard_pubkey: String,
    /// `blake3` hex of the sled's SID1 envelope; stamped on every mutation
    /// this session authors. Never the credential itself.
    pub sealed_id_digest: Option<String>,
}

/// Commitment to a SID1 envelope for audit records: the credential's blake3
/// digest, or `None` when the sled carries no sealed id.
pub fn sealed_id_digest(sealed_id: Option<&str>) -> Option<String> {
    sealed_id
        .filter(|s| !s.is_empty())
        .map(|s| blake3::hash(s.as_bytes()).to_hex().to_string())
}

const ANONYMOUS_ACTOR: &str = "anonymous";
const IDENTITY_PROJECTION_GROUP_ENV: &str = "OP_IDENTITY_PROJECTION_GROUP";

/// Remove possession material from any identity value that can leave the
/// MutationEngine through a generic state surface. The full value remains in
/// the authoritative cache and private credential projection only.
fn redact_identity_credentials(value: &mut simd_json::OwnedValue) {
    match value {
        simd_json::OwnedValue::Object(object) => {
            object.remove("sealed_id");
            for nested in object.values_mut() {
                redact_identity_credentials(nested);
            }
        }
        simd_json::OwnedValue::Array(values) => {
            for nested in values {
                redact_identity_credentials(nested);
            }
        }
        _ => {}
    }
}

fn contains_identity_credential_field(value: &simd_json::OwnedValue) -> bool {
    match value {
        simd_json::OwnedValue::Object(object) => {
            object.contains_key("sealed_id")
                || object.values().any(contains_identity_credential_field)
        }
        simd_json::OwnedValue::Array(values) => {
            values.iter().any(contains_identity_credential_field)
        }
        _ => false,
    }
}

fn restored_runtime_projection(
    plugin_id: &str,
    row: &op_plugins::state_plugins::runtime_documents::RuntimeDocument,
) -> anyhow::Result<simd_json::OwnedValue> {
    use op_plugins::state_plugins::{
        agent_config::AgentConfigState, config::ConfigStoreState, endpoint::EndpointState,
    };
    let document = row.document.clone();
    let projected = match plugin_id {
        "config" => {
            anyhow::ensure!(
                document
                    .get("configs")
                    .is_some_and(serde_json::Value::is_object),
                "invalid stored config document"
            );
            serde_json::to_value(serde_json::from_value::<ConfigStoreState>(document)?)?
        }
        "endpoint" => {
            anyhow::ensure!(
                document
                    .get("endpoints")
                    .is_some_and(serde_json::Value::is_array),
                "invalid stored endpoint document"
            );
            serde_json::to_value(serde_json::from_value::<EndpointState>(document)?)?
        }
        "agent_config" => {
            anyhow::ensure!(
                document
                    .get("agents")
                    .is_some_and(serde_json::Value::is_array),
                "invalid stored agent config document"
            );
            serde_json::to_value(serde_json::from_value::<AgentConfigState>(document)?)?
        }
        "embedding_model" => serde_json::to_value(
            op_plugins::state_plugins::embedding_model::restored_embedding_state(row)?,
        )?,
        "shared_unix_socket" => serde_json::to_value(
            op_plugins::state_plugins::shared_unix_socket_methods::restored_shared_registration_state(row.clone())?,
        )?,
        _ => anyhow::bail!("plugin has no public runtime-document projection"),
    };
    Ok(simd_json::serde::to_owned_value(projected)?)
}

impl MutationEngine {
    /// OD-93: fold durable active-catalog names into the observable json_render
    /// projection without depending on gallery dispatch internals.
    async fn enrich_json_render_catalog(
        &self,
        mut state: op_plugins::state_plugins::json_render::JsonRenderState,
    ) -> op_plugins::state_plugins::json_render::JsonRenderState {
        let Ok(gallery) = self.ui_gallery_cozo().await else {
            return state;
        };
        match gallery.list_ui_catalog(None, 64, false) {
            Ok(page) => {
                state.catalog_revision = page.revision;
                state.active_catalog_entry_names =
                    page.entries.into_iter().map(|entry| entry.name).collect();
            }
            Err(error) => {
                tracing::debug!(
                    error = %error,
                    "json_render active catalog projection skipped"
                );
            }
        }
        state
    }

    async fn json_render_methods_enriched(
        &self,
        schema: op_state_store::PluginSchema,
        documents: Arc<op_plugins::state_plugins::runtime_documents::RuntimeDocuments>,
    ) -> anyhow::Result<op_plugins::state_plugins::json_render::methods::JsonRenderMethods> {
        let defaults = op_plugins::projection_seed_state_from_schema(&schema);
        let snapshot: op_plugins::state_plugins::json_render::JsonRenderState =
            serde_json::from_value(serde_json::to_value(defaults)?)
                .context("invalid json_render state defaults in admitted plugin")?;
        let snapshot = self.enrich_json_render_catalog(snapshot).await;
        op_plugins::state_plugins::json_render::methods::JsonRenderMethods::new(
            snapshot, schema, documents,
        )
    }
}

pub(crate) fn public_plugin_value(
    plugin_id: &str,
    value: &simd_json::OwnedValue,
) -> simd_json::OwnedValue {
    let mut public = value.clone();
    if plugin_id == "identity_sled" {
        redact_identity_credentials(&mut public);
    }
    public
}

fn redact_identity_credentials_json(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(object) => {
            object.remove("sealed_id");
            for nested in object.values_mut() {
                redact_identity_credentials_json(nested);
            }
        }
        serde_json::Value::Array(values) => {
            for nested in values {
                redact_identity_credentials_json(nested);
            }
        }
        _ => {}
    }
}

/// The authenticated owner's get_identity reply is the credential handoff.
/// All generic/public state and every other identity in the reply stay redacted.
fn redact_identity_method_credentials(
    method: &str,
    verified_session: Option<&str>,
    value: &mut serde_json::Value,
) {
    let own_credential = verified_session
        .filter(|session| !session.is_empty() && method == "get_identity")
        .and_then(|session| {
            let identity = value.get("identity")?;
            (identity.get("session_id")?.as_str()? == session)
                .then(|| identity.get("sealed_id").filter(|v| v.is_string()).cloned())
                .flatten()
        });
    redact_identity_credentials_json(value);
    if let Some(credential) = own_credential
        && let Some(identity) = value.get_mut("identity").and_then(|v| v.as_object_mut())
    {
        identity.insert("sealed_id".to_owned(), credential);
    }
}

/// Generic state publication excludes credentials; identity has a typed durable owner.
fn write_plugin_projection(plugin_id: &str, json: &[u8]) -> anyhow::Result<()> {
    anyhow::ensure!(
        plugin_id != "identity_sled",
        "identity state requires its durable intent owner"
    );
    op_core::projection_shm::write_projection(plugin_id, json)
}

fn identity_projection_gid(state_dir: &str) -> anyhow::Result<nix::unistd::Gid> {
    let group_name =
        std::env::var(IDENTITY_PROJECTION_GROUP_ENV).unwrap_or_else(|_| "secrets".to_string());
    let group_gid = match nix::unistd::Group::from_name(&group_name)
        .map_err(|error| anyhow::anyhow!("resolve identity projection group: {error}"))?
    {
        Some(group) => group.gid,
        None if state_dir != op_core::projection_shm::SHM_STATE_DIR => {
            // Sandboxed test/dev projection roots are already private temp
            // directories and need not depend on a host-specific group.
            nix::unistd::Gid::current()
        }
        None => anyhow::bail!("identity projection group '{group_name}' does not exist"),
    };
    Ok(group_gid)
}

/// Seal the immutable session metadata once, in the same component that owns
/// genesis minting.  This helper does not touch Snowball footprints or event
/// hashes; the returned SID1 value is an identity projection only.
fn sealed_sealed_id(
    stamp: &GenesisStamp,
    schema_version: u32,
    expires_at: i64,
    nextdns: Option<op_identity::sealed_id::NextDnsContext>,
) -> anyhow::Result<String> {
    op_identity::sealed_id::SealedId {
        principal_id: op_identity::session::derive_principal_id(&stamp.wireguard_pubkey),
        principal_kind: "wireguard-principal".to_string(),
        session_id: stamp.session_id.clone(),
        wireguard_pubkey: stamp.wireguard_pubkey.clone(),
        session_genesis: stamp.genesis_hex.clone(),
        trace_id: stamp.trace_id.clone(),
        schema_version,
        issued_at: stamp.arrival_timestamp,
        expires_at,
        arrival_timestamp: stamp.arrival_timestamp,
        chain_head_at_arrival: stamp.chain_head_at_arrival.clone(),
        catalog_hash_at_arrival: stamp.catalog_hash_at_arrival.clone(),
        head_timestamp_at_arrival: stamp.head_timestamp_at_arrival,
        transport_scope: "dbus,grpc,mcp,mqtt".to_string(),
        nextdns,
    }
    .to_inline_ref()
    .map_err(|error| anyhow::anyhow!("seal session sealed ID: {error}"))
}

impl MutationEngine {
    pub(crate) fn install_mcp_policy_owner(
        &self,
        owner: Arc<crate::mcp_policy::McpPolicyOwner>,
    ) -> anyhow::Result<()> {
        if let Some(existing) = self.mcp_policy_owner.get() {
            anyhow::ensure!(
                Arc::ptr_eq(existing, &owner),
                "MCP gateway policy owner is already installed"
            );
            return Ok(());
        }
        self.mcp_policy_owner
            .set(owner)
            .map_err(|_| anyhow::anyhow!("MCP gateway policy owner installation raced"))
    }

    pub(crate) fn mcp_policy_owner(
        &self,
    ) -> anyhow::Result<Arc<crate::mcp_policy::McpPolicyOwner>> {
        self.mcp_policy_owner
            .get()
            .cloned()
            .context("MCP gateway has not installed its live policy owner")
    }

    pub(crate) async fn publish_mcp_toolsets(
        &self,
        output: &op_plugins::state_plugins::mcp::ConfigureToolGroupsOutput,
        expected_schema_hash: &str,
    ) -> anyhow::Result<()> {
        self.commit_state_update(
            StateChange {
                change_id: uuid::Uuid::new_v4().to_string(),
                event_id: 0,
                plugin_id: "mcp".into(),
                object_path: op_plugins::canonical::plugin_path("mcp"),
                change_type: ChangeType::PropertySet,
                member_name: Some("toolsets".into()),
                old_value: None,
                new_value: simd_json::serde::to_owned_value(output)?,
                tags_touched: Vec::new(),
                event_hash: String::new(),
                timestamp: chrono::Utc::now(),
                actor_id: "mcp_policy_owner".into(),
                source: ChangeSource::Internal,
            },
            Some(expected_schema_hash.to_owned()),
            false,
        )
        .await
        .map(|_| ())
    }

    /// Materialize the control-plane chatbot service identity at bridge startup.
    /// The public key is not a credential; it selects the already configured
    /// WireGuard account, while both derived IDs are checked against the
    /// release-owned expectations before the MutationEngine mints anything.
    pub async fn bootstrap_control_plane_chatbot_identity(
        &self,
    ) -> anyhow::Result<Option<SessionContext>> {
        let Ok(wireguard_pubkey) = std::env::var("OP_CONTROL_PLANE_CHATBOT_WIREGUARD_PUBKEY")
        else {
            return Ok(None);
        };
        let wireguard_pubkey = wireguard_pubkey.trim();
        if wireguard_pubkey.is_empty() {
            anyhow::bail!("OP_CONTROL_PLANE_CHATBOT_WIREGUARD_PUBKEY is empty");
        }
        let session_id = op_identity::session::derive_session_id(wireguard_pubkey);
        let principal_id = op_identity::session::derive_principal_id(wireguard_pubkey);
        if let Ok(expected) = std::env::var("OP_CONTROL_PLANE_CHATBOT_SESSION_ID")
            && expected.trim() != session_id
        {
            anyhow::bail!(
                "configured control-plane chatbot session_id does not match its WireGuard key"
            );
        }
        if let Ok(expected) = std::env::var("OP_CONTROL_PLANE_CHATBOT_PRINCIPAL_ID")
            && expected.trim() != principal_id
        {
            anyhow::bail!(
                "configured control-plane chatbot principal_id does not match its WireGuard key"
            );
        }
        match crate::human_principal_dispatch::resolve_key_for_assertion(wireguard_pubkey).await {
            Ok(Some(record)) if record.revoked_at != 0 => {
                anyhow::bail!("configured control-plane chatbot identity is revoked")
            }
            Ok(Some(record))
                if record.principal_id != principal_id
                    || record.human_pubkey != wireguard_pubkey =>
            {
                anyhow::bail!("configured control-plane chatbot registry binding changed")
            }
            Ok(Some(_)) => {}
            Ok(None) => {
                // This is a release-owned singleton identity, so its first
                // registration is itself a MutationEngine event. No direct
                // Cozo bootstrap or unaudited startup write is permitted.
                let args = simd_json::serde::to_owned_value(serde_json::json!({
                    "human_pubkey": wireguard_pubkey,
                    "display_alias": "control-plane-chatbot"
                }))?;
                self.mutate(
                    "human_principal".to_string(),
                    "/org/opdbus/v1/plugins/human_principal".to_string(),
                    ChangeType::MethodCall,
                    Some("register_key".to_string()),
                    args,
                    "op-grpc-bridge.bootstrap".to_string(),
                    Some("human_principal.write".to_string()),
                )
                .await?;
            }
            Err(error) => {
                anyhow::bail!("configured control-plane chatbot registry unavailable: {error:?}")
            }
        }
        if crate::identity_sled_dispatch::chatbot_awaits_explicit_reprovision(self, &session_id)
            .await?
        {
            tracing::info!(%session_id, "configured chatbot record was explicitly deleted; awaiting provisioning");
            return Ok(None);
        }
        self.mint_and_store_genesis(&session_id, wireguard_pubkey)
            .await?;
        crate::identity_sled_dispatch::set_session_actor_class(self, &session_id, "service")
            .await?;
        let context = self.session_context(&session_id).await.ok_or_else(|| {
            anyhow::anyhow!("configured control-plane chatbot identity was not cached after mint")
        })?;
        tracing::info!(
            %principal_id,
            %session_id,
            "control-plane chatbot identity sealed into its sled"
        );
        Ok(Some(context))
    }

    /// Materialize the operator-configured local human identity at startup.
    ///
    /// This is the same WireGuard-derived principal used by the human's other
    /// authenticated surfaces. It exists so local clients such as Codex can
    /// forward the MutationEngine-authored SID1 from that human sled; it does
    /// not create a Codex daemon or service principal.
    pub async fn bootstrap_configured_local_human_identity(
        &self,
    ) -> anyhow::Result<Option<SessionContext>> {
        let Ok(wireguard_pubkey) = std::env::var("OP_LOCAL_HUMAN_WIREGUARD_PUBKEY") else {
            return Ok(None);
        };
        let wireguard_pubkey = wireguard_pubkey.trim();
        if wireguard_pubkey.is_empty() {
            anyhow::bail!("OP_LOCAL_HUMAN_WIREGUARD_PUBKEY is empty");
        }
        let session_id = op_identity::session::derive_session_id(wireguard_pubkey);
        let principal_id = op_identity::session::derive_principal_id(wireguard_pubkey);
        if let Ok(expected) = std::env::var("OP_LOCAL_HUMAN_SESSION_ID")
            && expected.trim() != session_id
        {
            anyhow::bail!("configured local human session_id does not match its WireGuard key");
        }
        if let Ok(expected) = std::env::var("OP_LOCAL_HUMAN_PRINCIPAL_ID")
            && expected.trim() != principal_id
        {
            anyhow::bail!("configured local human principal_id does not match its WireGuard key");
        }
        match crate::human_principal_dispatch::resolve_key_for_assertion(wireguard_pubkey).await {
            Ok(Some(record)) if record.revoked_at != 0 => {
                anyhow::bail!("configured local human identity is revoked")
            }
            Ok(Some(record))
                if record.principal_id != principal_id
                    || record.human_pubkey != wireguard_pubkey =>
            {
                anyhow::bail!("configured local human registry binding changed")
            }
            Ok(Some(_)) => {}
            Ok(None) => {
                let display_alias = std::env::var("OP_LOCAL_HUMAN_DISPLAY_ALIAS")
                    .unwrap_or_else(|_| "local-human".to_string());
                let args = simd_json::serde::to_owned_value(serde_json::json!({
                    "human_pubkey": wireguard_pubkey,
                    "display_alias": display_alias
                }))?;
                self.mutate(
                    "human_principal".to_string(),
                    "/org/opdbus/v1/plugins/human_principal".to_string(),
                    ChangeType::MethodCall,
                    Some("register_key".to_string()),
                    args,
                    "op-grpc-bridge.bootstrap-local-human".to_string(),
                    Some("human_principal.write".to_string()),
                )
                .await?;
            }
            Err(error) => {
                anyhow::bail!("configured local human registry unavailable: {error:?}")
            }
        }
        self.mint_and_store_genesis(&session_id, wireguard_pubkey)
            .await?;
        crate::identity_sled_dispatch::set_session_actor_class(self, &session_id, "human").await?;
        let context = self.session_context(&session_id).await.ok_or_else(|| {
            anyhow::anyhow!("configured local human identity was not cached after mint")
        })?;
        tracing::info!(
            %principal_id,
            %session_id,
            "local human identity sealed into its sled"
        );
        Ok(Some(context))
    }

    pub async fn session_context(&self, session_id: &str) -> Option<SessionContext> {
        self.sessions.read().await.get(session_id).cloned()
    }

    /// Sessions the interceptor has anchored and not yet released.
    pub async fn connected_session_ids(&self) -> HashSet<String> {
        self.sessions.read().await.keys().cloned().collect()
    }

    pub async fn session_context_for_actor(&self, actor_id: &str) -> Option<SessionContext> {
        if actor_id.is_empty() || actor_id == ANONYMOUS_ACTOR {
            return None;
        }
        if let Some(context) = crate::external_mcp_session::scoped_context(actor_id) {
            return Some(context);
        }
        let record = crate::identity_sled_dispatch::session_record_for_actor(self, actor_id)
            .await
            .ok()?;
        let Some(record) = record else {
            return crate::external_mcp_session::context_for_actor(self, actor_id).await;
        };
        let genesis_hex = record.genesis.clone().filter(|g| !g.is_empty())?;
        let context = SessionContext {
            genesis_hex,
            session_id: record.session_id,
            wireguard_pubkey: record.wireguard_pubkey,
            sealed_id_digest: sealed_id_digest(record.sealed_id.as_deref()),
        };
        self.register_session_context(context.clone()).await;
        Some(context)
    }

    pub async fn ensure_session_context(&self, actor_id: &str) -> Option<SessionContext> {
        if actor_id.is_empty() || actor_id == ANONYMOUS_ACTOR {
            return None;
        }
        if let Some(found) = self.session_context_for_actor(actor_id).await {
            return Some(found);
        }
        let record = crate::identity_sled_dispatch::session_record_for_actor(self, actor_id)
            .await
            .ok()??;
        match self
            .mint_and_store_genesis(&record.session_id, &record.wireguard_pubkey)
            .await
        {
            Ok(_) => self.session_context(&record.session_id).await,
            Err(error) => {
                tracing::warn!(
                    %error,
                    session_id = %record.session_id,
                    "session arrival could not be anchored"
                );
                None
            }
        }
    }

    /// External human transport admission must not count as a container login.
    pub(crate) async fn register_external_mcp_context(&self, context: SessionContext) {
        self.sessions.write().await.insert(context.session_id.clone(), context);
    }

    pub(crate) async fn external_mcp_chain_head(&self) -> (String, i64) {
        let chain = self.event_chain.read().await;
        (chain.last_hash().to_string(),
         chain.events().last().map(|event| event.timestamp.timestamp()).unwrap_or(0))
    }

    pub async fn register_session_context(&self, context: SessionContext) {
        self.note_principal_login(&context.session_id).await;
        self.sessions
            .write()
            .await
            .insert(context.session_id.clone(), context);
    }

    pub async fn forget_session_context(&self, session_id: &str) {
        self.principal_logins.write().await.remove(session_id);
        self.sessions.write().await.remove(session_id);
    }

    /// Record that this principal presented its own credential just now.
    pub async fn note_principal_login(&self, session_id: &str) {
        if session_id.is_empty() {
            return;
        }
        self.principal_logins
            .write()
            .await
            .insert(session_id.to_string(), chrono::Utc::now().timestamp());
    }

    /// Sessions whose own credential was accepted inside `max_age_secs`.
    pub async fn logged_in_principal_sessions(&self, max_age_secs: i64) -> HashSet<String> {
        let now = chrono::Utc::now().timestamp();
        self.principal_logins
            .read()
            .await
            .iter()
            .filter(|(_, seen)| now.saturating_sub(**seen) < max_age_secs)
            .map(|(session_id, _)| session_id.clone())
            .collect()
    }

    /// Mint this session's genesis once at arrival (FR-1).
    pub async fn mint_and_store_genesis(
        &self,
        session_id: &str,
        wireguard_pubkey: &str,
    ) -> anyhow::Result<String> {
        if session_id.is_empty() {
            anyhow::bail!("session genesis requires a session_id");
        }
        anyhow::ensure!(
            !crate::identity_sled_dispatch::chatbot_awaits_explicit_reprovision(self, session_id)
                .await?,
            "explicitly deleted identity requires supported provisioning before minting"
        );
        let existing_record =
            crate::identity_sled_dispatch::stored_session(self, session_id).await?;
        if let Some(existing) = existing_record
            .as_ref()
            .filter(|record| record.is_anchored())
        {
            if existing.wireguard_pubkey != wireguard_pubkey {
                anyhow::bail!(
                    "session '{}' is already anchored to a different WireGuard identity",
                    session_id
                );
            }
            let existing_genesis = existing.genesis.clone().unwrap_or_default();
            let mut stamp = GenesisStamp {
                session_id: existing.session_id.clone(),
                wireguard_pubkey: existing.wireguard_pubkey.clone(),
                genesis_hex: existing_genesis.clone(),
                trace_id: existing.trace_id.clone(),
                sealed_id: String::new(),
                arrival_timestamp: existing.arrival_timestamp,
                chain_head_at_arrival: existing.chain_head_at_arrival.clone(),
                catalog_hash_at_arrival: existing.catalog_hash_at_arrival.clone(),
                head_timestamp_at_arrival: existing.head_timestamp_at_arrival,
            };
            stamp.sealed_id = match existing
                .sealed_id
                .as_deref()
                .filter(|sealed| !sealed.is_empty())
            {
                // Authored once: preserve the exact stored bytes. store_genesis
                // validates every claim and fails closed if they were altered.
                Some(sealed) => sealed.to_string(),
                // One-way migration for a legacy anchored record that predates
                // SID1. This is the only anchored-session sealing path.
                None => sealed_sealed_id(
                    &stamp,
                    existing.schema_version,
                    existing.expires_at.unwrap_or(0),
                    self.assigned_nextdns(&stamp.wireguard_pubkey),
                )?,
            };
            crate::identity_sled_dispatch::store_genesis(self, &stamp).await?;
            self.register_session_context(SessionContext {
                genesis_hex: existing_genesis.clone(),
                session_id: session_id.to_string(),
                wireguard_pubkey: wireguard_pubkey.to_string(),
                sealed_id_digest: sealed_id_digest(Some(&stamp.sealed_id)),
            })
            .await;
            return Ok(existing_genesis);
        }

        let stamp = self
            .mint_identity_genesis_stamp(
                session_id,
                wireguard_pubkey,
                existing_record
                    .as_ref()
                    .and_then(|record| record.expires_at)
                    .unwrap_or(0),
            )
            .await?;

        let stored = crate::identity_sled_dispatch::store_genesis(self, &stamp).await?;
        self.register_session_context(SessionContext {
            genesis_hex: stored.clone(),
            session_id: session_id.to_string(),
            wireguard_pubkey: wireguard_pubkey.to_string(),
            sealed_id_digest: sealed_id_digest(Some(&stamp.sealed_id)),
        })
        .await;
        self.record_session_arrival(session_id).await;
        Ok(stored)
    }

    async fn record_session_arrival(&self, session_id: &str) {
        let args = serde_json::json!({ "session_id": session_id }).to_string();
        let event = {
            let mut chain = self.event_chain.write().await;
            let event = chain
                .record_method_call(
                    session_id.to_string(),
                    "identity_sled".to_string(),
                    "session_arrival".to_string(),
                    Some("identity_sled.write".to_string()),
                    &args,
                )
                .clone();
            // Serialize timing persistence with every chain append, including
            // observations interleaved with an admitted method's two proofs.
            self.persist_audit_event(&event).await;
            event
        };
        let _ = self.chain_tx.send(event);
    }

    /// Create a new authoritative Mutation Engine
    pub fn new(event_chain: Arc<RwLock<EventChain>>, ovsdb: Arc<OvsdbDbusClient>) -> Self {
        let (change_tx, _) = broadcast::channel(1024);
        let (chain_tx, _) = broadcast::channel(1024);
        Self {
            event_chain,
            change_tx,
            chain_tx,
            state_cache: Arc::new(RwLock::new(HashMap::new())),
            state_publication: Arc::new(Mutex::new(())),
            identity_sled_store: Arc::new(OnceCell::new()),
            nextdns_bindings: Arc::new(OnceCell::new()),
            host_observation: Arc::new(Mutex::new(())),
            observers_started: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            incus_observer_started: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            cron_observer_started: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            dbus_connection: Arc::new(OnceCell::new()),
            session_bus: Arc::new(OnceCell::new()),
            signal_bus: Arc::new(OnceCell::new()),
            dbus_call_limiter: Arc::new(Semaphore::new(32)),
            audit_sink: Arc::new(OnceCell::new()),
            method_audit_uncertain: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            method_journal_documents: Arc::new(OnceCell::new()),
            ovsdb,
            ovsdb_monitor_tasks: Arc::new(Mutex::new(HashMap::new())),
            ovsdb_monitor_state: Arc::new(Mutex::new(())),
            chat_manager: Arc::new(ChatManager::new()),
            cognitive_mcp: Arc::new(OnceCell::new()),
            knowledge_inbox: crate::knowledge_inbox::KnowledgeInbox::default(),
            vector_projector: Arc::new(OnceCell::new()),
            chatbot_episode_cozo: Arc::new(OnceCell::new()),
            ui_gallery_cozo: Arc::new(OnceCell::new()),
            model_registry_cozo: Arc::new(OnceCell::new()),
            task_elevation: Arc::new(crate::task_elevation::TaskElevation::default()),
            memory_methods: Arc::new(OnceCell::new()),
            sess_decl_methods: Arc::new(OnceCell::new()),
            notebook_queries: Arc::new(crate::notebooklm_queries::NotebookQueries::default()),
            snowball_methods: Arc::new(OnceCell::new()),
            ui_workspace: Arc::new(OnceCell::new()),
            chatbot_episodes: Arc::new(OnceCell::new()),
            chat_response_cozo: Arc::new(OnceCell::new()),
            chat_responses: Arc::new(OnceCell::new()),
            chat_turns: Arc::new(crate::chat_response_turns::TurnRegistry::default()),
            generation_jobs: Arc::new(OnceCell::new()),
            shared_socket_path: Arc::new(OnceCell::new()),
            shared_listener: Arc::new(OnceCell::new()),
            shared_listener_projection: Arc::new(tokio::sync::Mutex::new(())),
            proxy_methods: Arc::new(OnceCell::new()),
            proxy_projection: Arc::new(Mutex::new(())),
            obfuscation_methods: Arc::new(OnceCell::new()),
            obfuscation_projection: Arc::new(Mutex::new(())),
            openflow_projection: Arc::new(Mutex::new(())),
            openflow_method_connection: Arc::new(OnceCell::new()),
            mcp_policy_owner: Arc::new(OnceCell::new()),
            antigravity_documents: Arc::new(OnceCell::new()),
            antigravity_credentials: Arc::new(OnceCell::new()),
            antigravity_projection: Arc::new(Mutex::new(())),
            mail_projection: Arc::new(Mutex::new(())),
            decoy_network_projection: Arc::new(Mutex::new(())),
            sessions: Arc::new(RwLock::new(HashMap::new())),
            principal_logins: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn chain_tx(&self) -> &broadcast::Sender<ChainEvent> {
        &self.chain_tx
    }

    #[cfg(test)]
    pub(crate) fn install_openflow_method_connection_for_test(
        &self,
        connection: Connection,
    ) -> anyhow::Result<()> {
        self.openflow_method_connection
            .set(connection)
            .map_err(|_| anyhow::anyhow!("OpenFlow method connection is already installed"))
    }

    pub fn configure_ui_workspace(
        &self,
        private_root: op_core::projection_shm::PrivateProjectionDirectory,
        documents: Arc<op_plugins::state_plugins::runtime_documents::RuntimeDocuments>,
        catalog_dir: std::path::PathBuf,
    ) -> anyhow::Result<()> {
        self.ui_workspace
            .set(Arc::new(
                crate::ui_workspace_dispatch::UiWorkspacePublisher::new(
                    private_root,
                    documents,
                    catalog_dir,
                ),
            ))
            .map_err(|_| anyhow::anyhow!("UI workspace owner is already configured"))
    }

    pub fn configure_chatbot_episode_projection(
        &self,
        root: op_core::projection_shm::PrivateProjectionDirectory,
        catalog: std::path::PathBuf,
    ) -> anyhow::Result<()> {
        self.chatbot_episodes
            .set(Arc::new(
                crate::chatbot_episode_state::EpisodePublisher::new(root, catalog),
            ))
            .map_err(|_| anyhow::anyhow!("chatbot episode projection is already configured"))
    }

    pub(crate) fn chatbot_episode_publisher(
        &self,
    ) -> anyhow::Result<&Arc<crate::chatbot_episode_state::EpisodePublisher>> {
        self.chatbot_episodes
            .get()
            .context("chatbot episode projection is not configured")
    }

    pub(crate) async fn ctl_plane_chatbot_episode_cozo(
        &self,
    ) -> anyhow::Result<Arc<op_cozo_store::CozoGraphShuttle>> {
        self.chatbot_episode_cozo
            .get_or_try_init(|| async {
                let path = std::env::var("CTL_PLANE_CHATBOT_EPISODE_DB_PATH")
                    .unwrap_or_else(|_| "/var/lib/op-dbus/chatbot-episodes.db".to_string());
                anyhow::ensure!(
                    !path.is_empty() && path != ":memory:" && path != "memory",
                    "chatbot episode store requires a durable path"
                );
                let general = self.cognitive_mcp.get().map(|server| server.cozo_shuttle());
                let conversations = std::env::var("CHAT_RESPONSE_DB_PATH")
                    .unwrap_or_else(|_| "/var/lib/op-dbus/chat-responses.db".to_string());
                let conversation_store = self.chat_response_cozo.get().cloned();
                tokio::task::spawn_blocking(move || {
                    crate::chatbot_episode_state::require_distinct_database_paths(&path)?;
                    crate::chatbot_episode_state::distinct_databases(
                        std::path::Path::new(&path),
                        std::path::Path::new(&conversations),
                    )?;
                    let store = op_cozo_store::CozoGraphShuttle::new_persistent_private(
                        std::path::PathBuf::from(path),
                    )?;
                    if let Some(general) = general {
                        store.ensure_distinct_from(&general)?;
                    }
                    if let Some(conversations) = conversation_store {
                        store.ensure_distinct_from(&conversations)?;
                    }
                    store.initialize_chatbot_episode_store()?;
                    Ok::<_, anyhow::Error>(Arc::new(store))
                })
                .await
                .context("chatbot episode store initialization worker failed")?
            })
            .await
            .cloned()
    }

    /// The json_render UI gallery owner. Its database is distinct from the
    /// generic cognitive database and the chatbot's episode database.
    pub(crate) async fn ui_gallery_cozo(
        &self,
    ) -> anyhow::Result<Arc<op_cozo_store::CozoGraphShuttle>> {
        self.ui_gallery_cozo
            .get_or_try_init(|| async {
                let path = std::env::var("UI_GALLERY_DB_PATH")
                    .unwrap_or_else(|_| "/var/lib/op-dbus/ui-gallery.db".to_string());
                anyhow::ensure!(
                    !path.is_empty() && path != ":memory:" && path != "memory",
                    "ui gallery store requires a durable path"
                );
                let episodes = std::env::var("CTL_PLANE_CHATBOT_EPISODE_DB_PATH")
                    .unwrap_or_else(|_| "/var/lib/op-dbus/chatbot-episodes.db".to_string());
                let conversations = std::env::var("CHAT_RESPONSE_DB_PATH")
                    .unwrap_or_else(|_| "/var/lib/op-dbus/chat-responses.db".to_string());
                let general = self.cognitive_mcp.get().map(|server| server.cozo_shuttle());
                let episode_store = self.chatbot_episode_cozo.get().cloned();
                let conversation_store = self.chat_response_cozo.get().cloned();
                tokio::task::spawn_blocking(move || {
                    crate::chatbot_episode_state::require_distinct_database_paths(&path)?;
                    crate::chatbot_episode_state::distinct_databases(
                        std::path::Path::new(&path),
                        std::path::Path::new(&episodes),
                    )?;
                    crate::chatbot_episode_state::distinct_databases(
                        std::path::Path::new(&path),
                        std::path::Path::new(&conversations),
                    )?;
                    let store = op_cozo_store::CozoGraphShuttle::new_persistent_private(
                        std::path::PathBuf::from(path),
                    )?;
                    if let Some(general) = general {
                        store.ensure_distinct_from(&general)?;
                    }
                    if let Some(episodes) = episode_store {
                        store.ensure_distinct_from(&episodes)?;
                    }
                    if let Some(conversations) = conversation_store {
                        store.ensure_distinct_from(&conversations)?;
                    }
                    store.initialize_ui_gallery_store()?;
                    Ok::<_, anyhow::Error>(Arc::new(store))
                })
                .await
                .context("ui gallery store initialization worker failed")?
            })
            .await
            .cloned()
    }

    /// The model registry owner. Its database is distinct from the generic
    /// cognitive database, the chatbot's episodes and the UI gallery.
    pub(crate) async fn model_registry_cozo(
        &self,
    ) -> anyhow::Result<Arc<op_cozo_store::CozoGraphShuttle>> {
        self.model_registry_cozo
            .get_or_try_init(|| async {
                let path = std::env::var("MODEL_REGISTRY_DB_PATH")
                    .unwrap_or_else(|_| "/var/lib/op-dbus/model-registry.db".to_string());
                anyhow::ensure!(
                    !path.is_empty() && path != ":memory:" && path != "memory",
                    "model registry store requires a durable path"
                );
                let episodes = std::env::var("CTL_PLANE_CHATBOT_EPISODE_DB_PATH")
                    .unwrap_or_else(|_| "/var/lib/op-dbus/chatbot-episodes.db".to_string());
                let gallery = std::env::var("UI_GALLERY_DB_PATH")
                    .unwrap_or_else(|_| "/var/lib/op-dbus/ui-gallery.db".to_string());
                let conversations = std::env::var("CHAT_RESPONSE_DB_PATH")
                    .unwrap_or_else(|_| "/var/lib/op-dbus/chat-responses.db".to_string());
                let others: Vec<Arc<op_cozo_store::CozoGraphShuttle>> = self
                    .cognitive_mcp
                    .get()
                    .map(|server| server.cozo_shuttle())
                    .into_iter()
                    .chain(self.chatbot_episode_cozo.get().cloned())
                    .chain(self.ui_gallery_cozo.get().cloned())
                    .chain(self.chat_response_cozo.get().cloned())
                    .collect();
                tokio::task::spawn_blocking(move || {
                    crate::chatbot_episode_state::require_distinct_database_paths(&path)?;
                    for other in [&episodes, &gallery, &conversations] {
                        crate::chatbot_episode_state::distinct_databases(
                            std::path::Path::new(&path),
                            std::path::Path::new(other),
                        )?;
                    }
                    let store = op_cozo_store::CozoGraphShuttle::new_persistent_private(
                        std::path::PathBuf::from(path),
                    )?;
                    for other in &others {
                        store.ensure_distinct_from(other)?;
                    }
                    store.initialize_model_registry_store()?;
                    Ok::<_, anyhow::Error>(Arc::new(store))
                })
                .await
                .context("model registry store initialization worker failed")?
            })
            .await
            .cloned()
    }

    pub fn configure_model_registry_store(
        &self,
        store: Arc<op_cozo_store::CozoGraphShuttle>,
    ) -> anyhow::Result<()> {
        store.initialize_model_registry_store()?;
        self.model_registry_cozo
            .set(store)
            .map_err(|_| anyhow::anyhow!("model registry Cozo owner is already configured"))
    }

    pub(crate) fn task_elevation(&self) -> &Arc<crate::task_elevation::TaskElevation> {
        &self.task_elevation
    }

    pub fn configure_ui_gallery_store(
        &self,
        store: Arc<op_cozo_store::CozoGraphShuttle>,
    ) -> anyhow::Result<()> {
        if let Some(general) = self.cognitive_mcp.get() {
            store.ensure_distinct_from(&general.cozo_shuttle())?;
        }
        if let Some(episodes) = self.chatbot_episode_cozo.get() {
            store.ensure_distinct_from(episodes)?;
        }
        if let Some(conversations) = self.chat_response_cozo.get() {
            store.ensure_distinct_from(conversations)?;
        }
        store.initialize_ui_gallery_store()?;
        self.ui_gallery_cozo
            .set(store)
            .map_err(|_| anyhow::anyhow!("ui gallery Cozo owner is already configured"))
    }

    pub fn configure_chatbot_episode_store(
        &self,
        store: Arc<op_cozo_store::CozoGraphShuttle>,
    ) -> anyhow::Result<()> {
        if let Some(general) = self.cognitive_mcp.get() {
            store.ensure_distinct_from(&general.cozo_shuttle())?;
        }
        if let Some(conversations) = self.chat_response_cozo.get() {
            store.ensure_distinct_from(conversations)?;
        }
        store.initialize_chatbot_episode_store()?;
        self.chatbot_episode_cozo
            .set(store)
            .map_err(|_| anyhow::anyhow!("chatbot episode Cozo owner is already configured"))
    }

    pub fn configure_chat_response_projection(
        &self,
        root: op_core::projection_shm::PrivateProjectionDirectory,
        catalog: std::path::PathBuf,
    ) -> anyhow::Result<()> {
        self.chat_responses
            .set(Arc::new(
                crate::chat_response_state::ChatResponsePublisher::new(root, catalog),
            ))
            .map_err(|_| anyhow::anyhow!("chat_response projection is already configured"))
    }

    pub(crate) fn chat_response_publisher(
        &self,
    ) -> anyhow::Result<&Arc<crate::chat_response_state::ChatResponsePublisher>> {
        self.chat_responses
            .get()
            .context("chat_response projection is not configured")
    }

    /// The private chat_response Cozo owner. Its database must be distinct
    /// from the generic cognitive database, the chatbot's episode database,
    /// the UI gallery and the model registry: conversations belong to the
    /// humans who asked.
    pub(crate) async fn chat_response_cozo(
        &self,
    ) -> anyhow::Result<Arc<op_cozo_store::CozoGraphShuttle>> {
        self.chat_response_cozo
            .get_or_try_init(|| async {
                let path = std::env::var("CHAT_RESPONSE_DB_PATH")
                    .unwrap_or_else(|_| "/var/lib/op-dbus/chat-responses.db".to_string());
                anyhow::ensure!(
                    !path.is_empty() && path != ":memory:" && path != "memory",
                    "chat_response store requires a durable path"
                );
                let episodes = std::env::var("CTL_PLANE_CHATBOT_EPISODE_DB_PATH")
                    .unwrap_or_else(|_| "/var/lib/op-dbus/chatbot-episodes.db".to_string());
                let gallery = std::env::var("UI_GALLERY_DB_PATH")
                    .unwrap_or_else(|_| "/var/lib/op-dbus/ui-gallery.db".to_string());
                let registry = std::env::var("MODEL_REGISTRY_DB_PATH")
                    .unwrap_or_else(|_| "/var/lib/op-dbus/model-registry.db".to_string());
                let general = self.cognitive_mcp.get().map(|server| server.cozo_shuttle());
                let others: Vec<Arc<op_cozo_store::CozoGraphShuttle>> = self
                    .chatbot_episode_cozo
                    .get()
                    .cloned()
                    .into_iter()
                    .chain(self.ui_gallery_cozo.get().cloned())
                    .chain(self.model_registry_cozo.get().cloned())
                    .collect();
                tokio::task::spawn_blocking(move || {
                    crate::chatbot_episode_state::require_distinct_database_paths(&path)?;
                    for other in [&episodes, &gallery, &registry] {
                        crate::chatbot_episode_state::distinct_databases(
                            std::path::Path::new(&path),
                            std::path::Path::new(other),
                        )?;
                    }
                    let store = op_cozo_store::CozoGraphShuttle::new_persistent_private(
                        std::path::PathBuf::from(path),
                    )?;
                    if let Some(general) = general {
                        store.ensure_distinct_from(&general)?;
                    }
                    for other in &others {
                        store.ensure_distinct_from(other)?;
                    }
                    store.initialize_chat_response_store()?;
                    Ok::<_, anyhow::Error>(Arc::new(store))
                })
                .await
                .context("chat_response store initialization worker failed")?
            })
            .await
            .cloned()
    }

    pub fn configure_chat_response_store(
        &self,
        store: Arc<op_cozo_store::CozoGraphShuttle>,
    ) -> anyhow::Result<()> {
        if let Some(general) = self.cognitive_mcp.get() {
            store.ensure_distinct_from(&general.cozo_shuttle())?;
        }
        if let Some(episodes) = self.chatbot_episode_cozo.get() {
            store.ensure_distinct_from(episodes)?;
        }
        if let Some(gallery) = self.ui_gallery_cozo.get() {
            store.ensure_distinct_from(gallery)?;
        }
        if let Some(registry) = self.model_registry_cozo.get() {
            store.ensure_distinct_from(registry)?;
        }
        store.initialize_chat_response_store()?;
        self.chat_response_cozo
            .set(store)
            .map_err(|_| anyhow::anyhow!("chat_response Cozo owner is already configured"))
    }

    pub(crate) fn chat_turns(&self) -> &Arc<crate::chat_response_turns::TurnRegistry> {
        &self.chat_turns
    }

    /// Record a verified human's message as the next turn of one of their
    /// conversations, then open the following turn for the agent principal
    /// that may answer it. Both sides land in the same conversation chain:
    /// the response's `prev_turn_hash` is the human turn's hash. Internal
    /// bridge API: the door's chat operation calls it, with its own admitted
    /// chat request as `request`; no transport or model can mint a turn.
    /// The answer is a `chat_response` write on the responder identity's
    /// 3tchedFS mount.
    pub(crate) async fn open_chat_turn(
        &self,
        owner: crate::chat_response_turns::TurnOwner,
        conversation_id: &str,
        message: String,
        request: crate::chat_response_dispatch::RequestEvent,
        responder_principal_id: &str,
    ) -> anyhow::Result<(
        op_plugins::state_plugins::chat_response::ChatTurn,
        crate::chat_response_turns::PendingTurn,
    )> {
        crate::project_dispatch::verified_session(
            self,
            &owner.principal_id,
            Some(&owner.session_id),
            Some(&owner.session_genesis),
        )
        .await
        .context("a chat turn requires the asking human's verified session")?;
        op_plugins::state_plugins::chat_response_methods::validate_uuid(
            "conversation_id",
            conversation_id,
        )?;
        anyhow::ensure!(
            crate::interceptor::load_capability_grants(responder_principal_id)
                .contains(op_plugins::state_plugins::chat_response::RESPOND_CAPABILITY),
            "the designated responder must hold chat_response.respond"
        );
        let store = self.chat_response_cozo().await?;
        let principal = owner.principal_id.clone();
        let conversation = conversation_id.to_string();
        let head = tokio::task::spawn_blocking(move || {
            store.get_chat_conversation(&principal, &conversation)
        })
        .await
        .context("chat_response conversation lookup worker failed")??
        .context("conversation does not exist in the asking principal's scope")?;
        // Reserve the answer's turn first: the registry allows one open turn
        // per conversation, so this also serializes the human message below,
        // and a refused reservation records nothing.
        let pending = self.chat_turns.open(
            owner.clone(),
            conversation_id,
            head.turn_count + 1,
            responder_principal_id,
        )?;

        // Notify the responder that a turn awaits their response (best-effort,
        // off the commit path). The responder can also poll list_pending_for_responder.
        {
            let wake = crate::responder_wake::chat_turn_wake(&pending, Some(&message));
            let responder = responder_principal_id.to_string();
            tokio::spawn(async move {
                crate::responder_wake::publish_responder_wake(wake, &responder).await;
            });
        }

        let human = match crate::chat_response_dispatch::commit_human_turn(
            self,
            &owner,
            conversation_id,
            message,
            request,
        )
        .await
        {
            Ok(human) if human.turn_index + 1 == pending.turn_index() => human,
            Ok(human) => {
                self.chat_turns.cancel(pending.turn_id());
                anyhow::bail!(
                    "human message landed at turn {} but the answer was reserved at {}",
                    human.turn_index,
                    pending.turn_index()
                );
            }
            Err(error) => {
                self.chat_turns.cancel(pending.turn_id());
                return Err(error);
            }
        };
        Ok((human, pending))
    }

    /// When this Chat names a conversation the asking human already started,
    /// record their message and open the answer turn for the control-plane
    /// chatbot. The reply stays that identity's `respond_to_user`,
    /// `request_clarification` or `cannot_perform` write. A correlation label
    /// that is not one of their conversations leaves this chat unchanged.
    async fn attach_chat_conversation_turn(
        &self,
        scope: &op_cozo_store::EpisodeScope,
        conversation_id: Option<&str>,
        message: &str,
        request: crate::chat_response_dispatch::RequestEvent,
    ) -> anyhow::Result<()> {
        if self.chat_responses.get().is_none() {
            return Ok(());
        }
        let Some(conversation_id) = conversation_id else {
            return Ok(());
        };
        if op_plugins::state_plugins::chat_response_methods::validate_uuid(
            "conversation_id",
            conversation_id,
        )
        .is_err()
        {
            return Ok(());
        }
        let store = self.chat_response_cozo().await?;
        let principal = scope.principal_id.clone();
        let conversation = conversation_id.to_string();
        let exists = tokio::task::spawn_blocking(move || {
            store.get_chat_conversation(&principal, &conversation)
        })
        .await
        .context("chat_response conversation lookup worker failed")??
        .is_some();
        if !exists {
            return Ok(());
        }
        let responder = std::env::var("OP_CONTROL_PLANE_CHATBOT_PRINCIPAL_ID")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .context(
                "this conversation's answer turn needs OP_CONTROL_PLANE_CHATBOT_PRINCIPAL_ID",
            )?;
        anyhow::ensure!(
            !message.is_empty(),
            "a conversation turn requires the human message"
        );
        self.open_chat_turn(
            crate::chat_response_turns::TurnOwner {
                principal_id: scope.principal_id.clone(),
                session_id: scope.session_id.clone(),
                session_genesis: scope.session_genesis.clone(),
            },
            conversation_id,
            message.to_string(),
            request,
            &responder,
        )
        .await?;
        Ok(())
    }

    /// Admit and commit an episode in one step. The chat path admits before
    /// its native effect (it must not lose the receipt of a completed
    /// completion); an event-triggered run's actions are each admitted
    /// mutations of their own, so its record is admitted at the end — a
    /// sled revoked mid-run then refuses the record, not the fail-closed
    /// actions.
    pub(crate) async fn commit_chatbot_episode(
        &self,
        row: op_cozo_store::ScopedEpisodeRecord,
    ) -> anyhow::Result<op_cozo_store::EpisodeSnapshot> {
        let effect = self
            .chatbot_episode_publisher()?
            .admit_effect(self, row.scope.clone())
            .await?;
        self.commit_chatbot_episode_effect(effect, row).await
    }

    pub(crate) async fn commit_chatbot_episode_effect(
        &self,
        effect: crate::chatbot_episode_state::EpisodeEffect,
        row: op_cozo_store::ScopedEpisodeRecord,
    ) -> anyhow::Result<op_cozo_store::EpisodeSnapshot> {
        effect.require_scope(&row.scope)?;
        let episode: op_plugins::state_plugins::ctl_plane_chatbot::ReasoningEpisode =
            serde_json::from_value(row.episode.clone())?;
        op_plugins::state_plugins::ctl_plane_chatbot_methods::validate_episode(&episode)?;
        anyhow::ensure!(
            episode.episode_id == row.episode_id
                && episode.content_hash == row.content_hash
                && episode.principal_id == row.scope.principal_id
                && episode.on_behalf_of == row.scope.principal_id
                && episode.session_id == row.scope.session_id
                && episode.session_genesis == row.scope.session_genesis,
            "episode document differs from durable identity key"
        );
        let store = self.ctl_plane_chatbot_episode_cozo().await?;
        let announced = row.clone();
        let snapshot = self
            .chatbot_episode_publisher()?
            .commit_effect(effect, move || {
                store
                    .insert_scoped_episode_and_snapshot(
                        &row,
                        crate::chatbot_episode_state::SNAPSHOT_LIMIT,
                    )
                    .map_err(Into::into)
            })
            .await?;
        self.announce_chatbot_write("episode_committed", &announced.scope, &announced.episode)
            .await;
        Ok(snapshot)
    }

    /// Record a durable chatbot write as a chain event under the scope's
    /// principal, so the audit trail and the vector projection (OD-64) see
    /// the committed document through its declared signal schema. Best
    /// effort: the Cozo row and the private SHM document are already
    /// committed; a failed announcement is logged, never unwound.
    pub(crate) async fn announce_chatbot_write(
        &self,
        signal: &str,
        scope: &op_cozo_store::EpisodeScope,
        document: &serde_json::Value,
    ) {
        let value = match simd_json::serde::to_owned_value(document) {
            Ok(value) => value,
            Err(error) => {
                tracing::warn!(%error, signal, "chatbot write not announced");
                return;
            }
        };
        if let Err(error) = self
            .process_authoritative_change(
                crate::chatbot_episode_state::PLUGIN.to_string(),
                crate::state_stream::plugin_object_path(crate::chatbot_episode_state::PLUGIN),
                ChangeType::Signal,
                Some(signal.to_string()),
                None,
                value,
                vec!["state".into(), crate::chatbot_episode_state::PLUGIN.into()],
                scope.principal_id.clone(),
                None,
                ChangeSource::Internal,
            )
            .await
        {
            tracing::warn!(%error, signal, "chatbot write not announced");
        }
    }

    pub(crate) async fn commit_chatbot_episode_embedding(
        &self,
        effect: crate::chatbot_episode_state::EpisodeEffect,
        scope: op_cozo_store::EpisodeScope,
        id: String,
        hash: String,
        expected: Option<op_cozo_store::EpisodeEmbeddingRecord>,
        replacement: op_cozo_store::EpisodeEmbeddingRecord,
    ) -> anyhow::Result<op_cozo_store::EpisodeSnapshot> {
        effect.require_scope(&scope)?;
        let store = self.ctl_plane_chatbot_episode_cozo().await?;
        let owned_scope = scope.clone();
        self.chatbot_episode_publisher()?
            .commit_effect(effect, move || {
                store
                    .compare_exchange_episode_embedding(
                        &owned_scope,
                        &id,
                        &hash,
                        expected.as_ref(),
                        &replacement,
                        crate::chatbot_episode_state::SNAPSHOT_LIMIT,
                    )
                    .map_err(Into::into)
            })
            .await
    }

    pub(crate) fn ui_workspace(
        &self,
    ) -> anyhow::Result<&Arc<crate::ui_workspace_dispatch::UiWorkspacePublisher>> {
        self.ui_workspace
            .get()
            .context("UI workspace owner is not configured")
    }

    pub(crate) fn configure_generation_jobs(
        &self,
        private_root: op_core::projection_shm::PrivateProjectionDirectory,
        documents: Arc<op_plugins::state_plugins::runtime_documents::RuntimeDocuments>,
        catalog_dir: std::path::PathBuf,
        backend: Arc<dyn crate::generation_jobs_dispatch::GenerationBackend>,
    ) -> anyhow::Result<()> {
        self.generation_jobs
            .set(Arc::new(
                crate::generation_jobs_dispatch::GenerationPublisher::new(
                    private_root,
                    documents,
                    catalog_dir,
                    backend,
                ),
            ))
            .map_err(|_| anyhow::anyhow!("generation owner is already configured"))
    }

    pub(crate) fn generation_jobs(
        &self,
    ) -> anyhow::Result<&Arc<crate::generation_jobs_dispatch::GenerationPublisher>> {
        self.generation_jobs
            .get()
            .context("generation owner is not configured")
    }

    pub fn configure_shared_socket_path(&self, path: &std::path::Path) -> anyhow::Result<()> {
        anyhow::ensure!(path.is_absolute(), "shared socket path must be absolute");
        let path = path.to_str().context("shared socket path must be UTF-8")?;
        if let Some(configured) = self.shared_socket_path.get() {
            anyhow::ensure!(
                configured == path,
                "shared socket owner path cannot change after startup"
            );
            return Ok(());
        }
        self.shared_socket_path
            .set(path.to_owned())
            .map_err(|_| anyhow::anyhow!("shared socket owner path already configured"))
    }

    pub fn set_shared_listener(
        &self,
        listener: Arc<crate::shared_socket::SharedListenerController>,
    ) -> anyhow::Result<()> {
        self.configure_shared_socket_path(listener.path())?;
        if let Some(current) = self.shared_listener.get() {
            anyhow::ensure!(
                Arc::ptr_eq(current, &listener),
                "shared listener owner cannot change after startup"
            );
            return Ok(());
        }
        self.shared_listener
            .set(listener)
            .map_err(|_| anyhow::anyhow!("shared listener owner already installed"))
    }

    pub(crate) async fn publish_shared_listener_startup(&self) -> anyhow::Result<()> {
        let _guard = self.shared_listener_projection.lock().await;
        let catalog = crate::sealed_schema_reader::blob_catalog_dir_from_env();
        if !crate::sealed_schema_reader::manifest_plugins(&catalog)?.contains_key("unix_socket") {
            return Ok(());
        }
        crate::sealed_schema_reader::read_manifest_pinned_plugin_schema(&catalog, "unix_socket")?;
        let listener = self
            .shared_listener
            .get()
            .context("shared listener owner is not installed")?;
        self.publish_plugin_projection_state(
            "unix_socket",
            simd_json::serde::to_owned_value(listener.snapshot()?)?,
            ChangeType::PropertySet,
            true,
        )
        .await
    }

    /// Explicit ephemeral owner for isolated transport tests; production keeps
    /// its mandatory durable Cozo/vector initialization.
    #[cfg(test)]
    pub(crate) fn configure_cognitive_runtime_fixture(
        &self, runtime: Arc<CognitiveMcpServer>,
    ) -> anyhow::Result<()> {
        self.cognitive_mcp
            .set(runtime)
            .map_err(|_| anyhow::anyhow!("cognitive fixture owner already configured"))
    }

    pub(crate) async fn cognitive_mcp(&self) -> anyhow::Result<&Arc<CognitiveMcpServer>> {
        self.cognitive_mcp
            .get_or_try_init(|| async {
                let path = std::env::var("COGNITIVE_MCP_DB_PATH")
                    .unwrap_or_else(|_| "/var/lib/op-dbus/cognitive-memory.db".to_string());
                crate::chatbot_episode_state::require_distinct_database_paths(
                    &std::env::var("CTL_PLANE_CHATBOT_EPISODE_DB_PATH")
                        .unwrap_or_else(|_| "/var/lib/op-dbus/chatbot-episodes.db".to_string()),
                )?;
                let server = CognitiveMcpServer::new_durable(&path)
                    .await
                    .map_err(|error| {
                        anyhow::anyhow!("durable cognitive runtime unavailable: {error}")
                    })?;
                if let Some(episodes) = self.chatbot_episode_cozo.get() {
                    episodes.ensure_distinct_from(&server.cozo_shuttle())?;
                }
                if let Some(conversations) = self.chat_response_cozo.get() {
                    conversations.ensure_distinct_from(&server.cozo_shuttle())?;
                }
                // The control-plane chatbot is the consumer of the context
                // engine's knowledge pushes: drain them into the per-session
                // inbox that the next turn / `specialist` call reads.
                self.knowledge_inbox
                    .consume(server.context_engine().subscribe_pushes());
                // ...and delivered to the UI: committed to the session's
                // private SHM document, then the EMQX doorbell rings.
                if let Some(engine) = crate::interceptor::engine_handle() {
                    crate::knowledge_delivery::spawn(
                        engine,
                        server.context_engine().subscribe_pushes(),
                    );
                }
                Ok(Arc::new(server))
            })
            .await
    }

    /// Drain the knowledge pushes waiting for `scope`, oldest first, as
    /// rendered context lines. Expired pushes are dropped.
    pub(crate) async fn take_pending_knowledge(&self, scope: &str) -> Vec<String> {
        self.knowledge_inbox.take(scope).await
    }

    /// Open the durable audit sink and rebuild the in-memory event chain from it.
    ///
    /// Called once at startup (before the gRPC/D-Bus surfaces accept traffic) so
    /// that a query issued immediately after a restart sees the pre-restart
    /// history. Idempotent: the sink is behind a `OnceCell`, and the rebuild is
    /// skipped when the chain already holds events.
    ///
    /// Returns the number of events replayed from disk. A sink that cannot be
    /// opened (no Btrfs, no permission, path absent) is not an error: the engine
    /// keeps recording to RAM and logs at `warn!`.
    ///
    /// OSCAL subid: `src.service.event-chain.rebuild@v1`
    pub async fn init_audit_durability(&self) -> usize {
        let base_path = std::env::var("OPDBUS_SNOWBALL_PATH")
            .unwrap_or_else(|_| DEFAULT_SNOWBALL_PATH.to_string());
        let cache_path =
            std::env::var("OP_DBUS_CACHE_DIR").unwrap_or_else(|_| DEFAULT_CACHE_PATH.to_string());
        self.init_audit_durability_with_cache_at(&base_path, &cache_path)
            .await
    }

    /// [`init_audit_durability`](Self::init_audit_durability) against an explicit
    /// chain path, bypassing `$OPDBUS_SNOWBALL_PATH`.
    pub async fn init_audit_durability_at(&self, base_path: &str) -> usize {
        let base = std::path::Path::new(base_path);
        let cache_name = format!(
            "{}-cache",
            base.file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("snowball")
        );
        let cache_path = base
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .join(cache_name);
        self.init_audit_durability_with_cache_at(base_path, &cache_path.to_string_lossy())
            .await
    }

    async fn init_audit_durability_with_cache_at(
        &self,
        base_path: &str,
        cache_path: &str,
    ) -> usize {
        if self.audit_sink.get().is_some() {
            return 0;
        }

        let chain_store = match OptimizedSnowball::new(base_path, cache_path).await {
            Ok(chain) => {
                tracing::info!(
                    path = %base_path,
                    cache_path = %cache_path,
                    "durable audit timing, Btrfs cache, and NUMA topology initialized"
                );
                AuditSink::Optimized(Arc::new(chain))
            }
            Err(cache_error) => match StreamingSnowball::new(base_path).await {
                Ok(chain) => {
                    tracing::warn!(
                        error = %cache_error,
                        path = %base_path,
                        cache_path = %cache_path,
                        "NUMA-aware Btrfs cache unavailable; durable timing remains active"
                    );
                    AuditSink::TimingOnly(Arc::new(chain))
                }
                Err(error) => {
                    tracing::warn!(
                        %error,
                        path = %base_path,
                        "durable audit sink unavailable; event chain stays in memory only"
                    );
                    // A previously persisted trail is still replayable even
                    // when the current host cannot reopen its Btrfs sink.
                    return self
                        .rebuild_chain_from_disk(&std::path::Path::new(base_path).join("timing"))
                        .await;
                }
            },
        };

        let timing_dir = std::path::Path::new(base_path).join("timing");
        let replayed = self.rebuild_chain_from_disk(&timing_dir).await;
        let _ = self.audit_sink.set(chain_store);
        // The trail is durable; from here every block gets its vector.
        self.start_vector_projection();
        replayed
    }

    /// Replay persisted audit events from the `timing_subvol` into the chain.
    ///
    /// Records are sorted by their persisted `event_id` so hash linkage is
    /// restored in the original order. Malformed or non-audit files (the
    /// timing directory also holds footprints written by other producers) are
    /// skipped with a `warn!` rather than aborting the rebuild.
    async fn rebuild_chain_from_disk(&self, timing_dir: &std::path::Path) -> usize {
        {
            let chain = self.event_chain.read().await;
            if !chain.events().is_empty() {
                tracing::warn!(
                    events = chain.events().len(),
                    "event chain already populated; skipping disk rebuild"
                );
                return 0;
            }
        }

        let mut dir = match tokio::fs::read_dir(timing_dir).await {
            Ok(dir) => dir,
            Err(error) => {
                tracing::info!(
                    %error,
                    path = %timing_dir.display(),
                    "no audit trail on disk yet; starting with an empty chain"
                );
                return 0;
            }
        };

        let max_replay: usize = std::env::var("OPDBUS_AUDIT_REPLAY_LIMIT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(10_000);

        let mut candidate_paths = Vec::new();
        while let Ok(Some(entry)) = dir.next_entry().await {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let block_num = path
                .file_name()
                .and_then(|n| n.to_str())
                .and_then(op_snowball::parse_block_number);
            candidate_paths.push((block_num, path));
        }

        candidate_paths.sort_by(|(a_num, a_path), (b_num, b_path)| match (a_num, b_num) {
            (Some(a), Some(b)) => a.cmp(b),
            (Some(_), None) => std::cmp::Ordering::Greater,
            (None, Some(_)) => std::cmp::Ordering::Less,
            (None, None) => a_path.cmp(b_path),
        });

        let total_candidates = candidate_paths.len();
        let target_paths = if total_candidates > max_replay {
            &candidate_paths[total_candidates - max_replay..]
        } else {
            &candidate_paths[..]
        };

        // (event_id, event_json) — sorted before replay so linkage is exact.
        let mut records: Vec<(u64, serde_json::Value)> = Vec::new();
        let mut skipped = 0usize;

        for (_, path) in target_paths {
            let bytes = match tokio::fs::read(path).await {
                Ok(bytes) => bytes,
                Err(error) => {
                    tracing::warn!(%error, path = %path.display(), "unreadable audit record");
                    skipped += 1;
                    continue;
                }
            };
            let value: serde_json::Value = match serde_json::from_slice(&bytes) {
                Ok(value) => value,
                Err(error) => {
                    tracing::warn!(%error, path = %path.display(), "malformed audit record");
                    skipped += 1;
                    continue;
                }
            };
            // Audit footprints store the lossless ChainEvent in `data.payload`.
            // Accept the older metadata envelope too, so trails written before
            // the optimized sink remain restart-compatible.
            let Some(event) = value
                .get("data")
                .and_then(|d| d.get("metadata"))
                .and_then(|m| m.get("audit_event"))
                .or_else(|| value.get("data").and_then(|d| d.get("payload")))
            else {
                continue;
            };
            // Replay the validated event itself.  The state-store accepts a
            // bare ChainEvent and this avoids coupling recovery to the outer
            // Snowball envelope shape.
            let event_value = event.clone();
            let Ok(event_record) = serde_json::from_value::<ChainEvent>(event_value.clone()) else {
                tracing::warn!(path = %path.display(), "audit record is not a complete ChainEvent");
                skipped += 1;
                continue;
            };
            records.push((event_record.event_id, event_value));
        }

        records.sort_by_key(|(event_id, _)| *event_id);

        let mut replayed = 0usize;
        {
            let mut chain = self.event_chain.write().await;
            for (event_id, value) in &records {
                match chain.replay_from_footprint(value) {
                    Ok(_) => replayed += 1,
                    Err(error) => {
                        tracing::warn!(%error, event_id, "audit record rejected during replay");
                        skipped += 1;
                    }
                }
            }
        }

        if replayed > 0 || skipped > 0 {
            tracing::info!(
                replayed,
                skipped,
                "event chain rebuilt from durable audit trail"
            );
        }
        replayed
    }

    /// Persist one chain event to the durable audit sink.
    ///
    /// Runs inline on the dispatch path (NFR-4: not deferred to a background
    /// task). A write failure is logged and swallowed — the event is already in
    /// the in-memory chain. A failure blocks subsequent method completion
    /// proofs until restart/replay; observations keep their existing API.
    ///
    /// OSCAL subid: `evt.service.event-chain.persist@v1`
    async fn persist_audit_event(&self, event: &ChainEvent) {
        let Some(sink) = self.audit_sink.get() else {
            self.method_audit_uncertain
                .store(true, std::sync::atomic::Ordering::Release);
            return;
        };
        match sink.add_footprint(event_to_footprint(event)).await {
            Ok(_) => {
                if let Some(projector) = self.vector_projector.get() {
                    projector.notify();
                }
            }
            Err(error) => {
                self.method_audit_uncertain
                    .store(true, std::sync::atomic::Ordering::Release);
                tracing::warn!(
                    %error,
                    event_id = event.event_id,
                    "audit durability write failed; event retained in memory only"
                );
            }
        }
    }

    /// Start vectorizing chain blocks as they land (OD-64). Idempotent.
    pub fn start_vector_projection(&self) {
        if self.vector_projector.get().is_some() {
            return;
        }
        let policy = Arc::new(crate::chain_vector_projector::SchemaProjectionPolicy::new(
            crate::sealed_schema_reader::blob_catalog_dir_from_env(),
        ));
        let _ =
            self.vector_projector
                .set(crate::chain_vector_projector::ChainVectorProjector::spawn(
                    policy,
                ));
    }

    /// Store the session bus connection used for `Updated` signal emission.
    /// Called from server.rs after SchemaRouter registers its interfaces.
    pub fn set_signal_bus(&self, conn: Connection) {
        let _ = self.signal_bus.set(conn);
    }

    /// Top-level keys of a whole-state value, for batch `Updated` payloads.
    fn state_keys_owned(value: &simd_json::OwnedValue) -> Option<Vec<String>> {
        value
            .as_object()
            .map(|obj| obj.keys().map(|k| k.to_string()).collect())
    }

    /// Emit the `Updated` signal on `org.opdbus.v1.PluginV1` for a given plugin.
    ///
    /// `key` is the mutated member when a single key changed; `keys` lists the
    /// top-level subtrees affected by a whole-state write. The payload always
    /// identifies what changed (`{"plugin","key"}` or `{"plugin","keys"}`) so
    /// subscribers can act on it without a follow-up query (REQ-1.5).
    ///
    /// Best-effort: if the signal bus is not yet available (early boot) or the
    /// interface is not registered for this plugin, the write still succeeds.
    /// The signal is for reactivity; SHM is the source of truth.
    async fn emit_updated_signal(
        &self,
        plugin_id: &str,
        key: Option<&str>,
        keys: Option<Vec<String>>,
    ) {
        Self::emit_updated_signal_with_bus(
            self.signal_bus.clone(),
            plugin_id.to_string(),
            key.map(str::to_string),
            keys,
        )
        .await;
    }

    async fn emit_updated_signal_with_bus(
        bus: Arc<OnceCell<Connection>>,
        plugin_id: String,
        key: Option<String>,
        keys: Option<Vec<String>>,
    ) {
        let conn = match bus.get() {
            Some(c) => c,
            None => {
                tracing::debug!(
                    plugin_id,
                    "signal_bus not yet available; skipping Updated signal"
                );
                return;
            }
        };
        let path = format!("/org/opdbus/v1/plugins/{plugin_id}");
        let iface_ref = match conn
            .object_server()
            .interface::<_, crate::schema_router::SchemaBackedInterface>(&*path)
            .await
        {
            Ok(r) => r,
            Err(_) => {
                tracing::debug!(
                    plugin_id,
                    "PluginV1 interface not registered at path; skipping signal"
                );
                return;
            }
        };
        let payload = match (key, keys) {
            (Some(k), _) => serde_json::json!({"plugin": plugin_id, "key": k}),
            (None, Some(ks)) => serde_json::json!({"plugin": plugin_id, "keys": ks}),
            (None, None) => serde_json::json!({"plugin": plugin_id}),
        }
        .to_string();
        if let Err(e) = crate::schema_router::SchemaBackedInterface::updated(
            iface_ref.signal_emitter(),
            &payload,
        )
        .await
        {
            tracing::debug!(plugin_id, error = %e, "Failed to emit Updated signal");
        }
    }

    /// Restore committed state for explicitly sealed plugins. Schema examples
    /// and field defaults describe a contract, not observed runtime state.
    /// Startup never creates, refreshes, or removes plugin blobs.
    pub async fn seed_missing_plugin_projections(&self) -> anyhow::Result<usize> {
        self.seed_plugin_projections_with_documents(None).await
    }

    /// Bootstrap with an injected durable document store. Used by embedders and
    /// isolated recovery tests; plugin presence still comes only from the blob
    /// manifest. Committed domain data supersedes stale or missing projections.
    pub async fn seed_plugin_projections_with_documents(
        &self,
        mut documents: Option<Arc<op_plugins::state_plugins::runtime_documents::RuntimeDocuments>>,
    ) -> anyhow::Result<usize> {
        let state_dir = op_core::projection_shm::shm_state_dir();
        let prepare_dir = state_dir.clone();
        tokio::task::spawn_blocking(move || {
            op_core::projection_shm::prepare_projection_directory_in(&prepare_dir)
        })
        .await??;
        let catalog = crate::sealed_schema_reader::blob_catalog_dir_from_env();
        let plugins = crate::sealed_schema_reader::manifest_plugins(&catalog)?;
        let mut seeded = 0usize;
        for plugin_id in plugins.keys() {
            // Identity recovery owns its durable rows and paired projections.
            if plugin_id == "identity_sled" {
                continue;
            }
            let schema = crate::sealed_schema_reader::read_manifest_pinned_plugin_schema(
                &catalog, plugin_id,
            )?;
            anyhow::ensure!(
                schema.name == *plugin_id,
                "sealed schema owner mismatch for {plugin_id}"
            );
            let read_id = plugin_id.clone();
            let read_dir = state_dir.clone();
            let public_projection = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
                if read_id != "identity_sled" {
                    op_core::projection_shm::restrict_projection_file_in(&read_dir, &read_id)?;
                }
                Ok(op_core::projection_shm::read_projection_snapshot_in(
                    &read_dir,
                    std::slice::from_ref(&read_id),
                )?
                .states
                .remove(&read_id))
            })
            .await??;
            if matches!(
                plugin_id.as_str(),
                "config"
                    | "endpoint"
                    | "agent_config"
                    | "embedding_model"
                    | "shared_unix_socket"
                    | "json_render"
            ) {
                let store = match documents.as_ref() {
                    Some(store) => store.clone(),
                    None => {
                        let store =
                            op_plugins::state_plugins::runtime_documents::runtime_documents()
                                .await?;
                        documents = Some(store.clone());
                        store
                    }
                };
                if let Some(row) = store.read(plugin_id).await? {
                    let state = if plugin_id == "json_render" {
                        let projected = self
                            .json_render_methods_enriched(schema.clone(), store.clone())
                            .await?
                            .projection_state()
                            .await?;
                        simd_json::serde::to_owned_value(projected)?
                    } else {
                        restored_runtime_projection(plugin_id, &row)?
                    };
                    let bytes = simd_json::to_vec(&state)?;
                    let changed = public_projection.as_deref() != Some(bytes.as_slice());
                    if changed {
                        write_plugin_projection(plugin_id, &bytes)?;
                        self.emit_updated_signal(plugin_id, None, Self::state_keys_owned(&state))
                            .await;
                        seeded += 1;
                    }
                    self.update_state_cache(plugin_id.clone(), state).await;
                    continue;
                }
            }
            let existing_projection = public_projection;
            if let Some(mut bytes) = existing_projection {
                match simd_json::to_owned_value(&mut bytes) {
                    Ok(projected_state) => {
                        self.update_state_cache(plugin_id.clone(), projected_state)
                            .await;
                    }
                    Err(error) => {
                        tracing::warn!(
                            plugin_id = %plugin_id,
                            %error,
                            "existing plugin projection is invalid JSON; leaving it authoritative but uncached"
                        );
                    }
                }
                continue;
            }

            // Presence belongs to the blob catalog. Without a committed
            // observation or durable owner document there is no runtime state
            // to publish yet; examples/defaults must not become fake telemetry.
        }

        tracing::info!(
            projections = seeded,
            "Restored committed state projections for sealed plugins"
        );
        Ok(seeded)
    }

    /// Authoritative D-Bus connection getter
    pub async fn dbus_connection(&self) -> anyhow::Result<Connection> {
        self.dbus_connection
            .get_or_try_init(|| async { Connection::system().await })
            .await
            .cloned()
            .map_err(|e| anyhow::anyhow!(e))
    }

    /// Get or create the session bus connection (for projection tree introspection)
    pub(crate) async fn session_bus(&self) -> anyhow::Result<Connection> {
        self.session_bus
            .get_or_try_init(|| async {
                let addr = std::env::var("DBUS_SESSION_BUS_ADDRESS")
                    .unwrap_or_else(|_| op_core::config::SESSION_BUS_ADDRESS.to_string());
                zbus::connection::Builder::address(addr.as_str())
                    .map_err(|e| anyhow::anyhow!("Builder::address: {}", e))?
                    .build()
                    .await
                    .map_err(|e| anyhow::anyhow!("Session bus connect: {}", e))
            })
            .await
            .cloned()
            .map_err(|e| anyhow::anyhow!(e))
    }

    fn compute_tags(&self, plugin_id: &str, object_path: &str) -> Vec<String> {
        let mut tags = Vec::new();
        if plugin_id == "net" || object_path.contains("/ovsdb/") {
            tags.push("network".to_string());
            tags.push("ovsdb".to_string());
        } else {
            tags.push("state".to_string());
            tags.push(plugin_id.to_string());
        }
        tags
    }

    /// Process a change that has already happened in an authoritative store.
    /// This records the change in the event chain and broadcasts it to gRPC.
    #[allow(clippy::too_many_arguments)]
    pub async fn process_authoritative_change(
        &self,
        plugin_id: String,
        object_path: String,
        change_type: ChangeType,
        member_name: Option<String>,
        old_value: Option<simd_json::OwnedValue>,
        new_value: simd_json::OwnedValue,
        tags: Vec<String>,
        actor_id: String,
        capability_id: Option<String>,
        source: ChangeSource,
    ) -> Result<StateChange, String> {
        self.process_authoritative_change_inner(
            plugin_id,
            object_path,
            change_type,
            member_name,
            old_value,
            new_value,
            tags,
            actor_id,
            capability_id,
            source,
            None,
        )
        .await
    }

    /// Publish a backend observation against the schema used to validate it.
    /// The commit worker checks this pin immediately before folding. Catalog
    /// resealing is separate from the SHM transaction and must be coordinated
    /// by release tooling; this is not a cross-catalog transaction guarantee.
    pub(crate) async fn process_observed_state(
        &self,
        plugin_id: &str,
        state: simd_json::OwnedValue,
        expected_schema_hash: String,
    ) -> Result<StateChange, String> {
        self.process_authoritative_change_inner(
            plugin_id.to_string(),
            op_plugins::canonical::plugin_path(plugin_id),
            ChangeType::PropertySet,
            None,
            None,
            state,
            Vec::new(),
            "backend-observer".to_string(),
            None,
            ChangeSource::Internal,
            Some(expected_schema_hash),
        )
        .await
    }

    /// Replace one root property while retaining every other committed field.
    /// Array-valued properties are not whole D-Bus child objects.
    pub(crate) async fn process_observed_property(
        &self,
        plugin_id: &str,
        member: &str,
        value: simd_json::OwnedValue,
        expected_schema_hash: String,
    ) -> Result<StateChange, String> {
        self.process_authoritative_change_inner(
            plugin_id.to_string(),
            op_plugins::canonical::plugin_path(plugin_id),
            ChangeType::PropertySet,
            Some(member.to_owned()),
            None,
            value,
            Vec::new(),
            "backend-observer".to_string(),
            None,
            ChangeSource::Internal,
            Some(expected_schema_hash),
        )
        .await
    }

    fn ovsdb_owner_scope(
        actor_id: &str,
        session_id: Option<&str>,
        session_genesis: Option<&str>,
    ) -> String {
        format!(
            "{actor_id}\0{}\0{}",
            session_id.unwrap_or_default(),
            session_genesis.unwrap_or_default()
        )
    }

    fn ovsdb_monitor_projection_key(owner: &str, monitor_id: &str) -> String {
        let mut material = String::with_capacity(owner.len() + monitor_id.len() + 1);
        material.push_str(owner);
        material.push('\0');
        material.push_str(monitor_id);
        blake3::hash(material.as_bytes()).to_hex()[..32].to_owned()
    }

    async fn publish_ovsdb_topology(&self, expected_schema_hash: &str) -> anyhow::Result<()> {
        let state = op_plugins::state_plugins::ovsdb_bridge::OvsBridgePlugin::new()
            .query_bridges()
            .await?;
        self.process_observed_property(
            "ovsdb_bridge",
            "bridges",
            simd_json::serde::to_owned_value(state.bridges)?,
            expected_schema_hash.to_owned(),
        )
        .await
        .map(|_| ())
        .map_err(anyhow::Error::msg)
    }

    async fn publish_ovsdb_monitor_snapshot(
        &self,
        projection_key: &str,
        snapshot: &op_network::rovs_proxy::monitor::ProtocolSnapshot,
        active: bool,
        error: Option<String>,
        expected_schema_hash: &str,
    ) -> anyhow::Result<()> {
        let state = op_plugins::state_plugins::ovsdb_bridge::OvsdbMonitorState {
            projection_key: projection_key.to_owned(),
            monitor_id: snapshot.monitor_id.clone(),
            method: snapshot.method.clone(),
            sequence: snapshot.sequence,
            active,
            error,
            tables: op_plugins::state_plugins::ovsdb_bridge::materialize_monitor_tables(
                &snapshot.tables,
            ),
        };
        self.publish_ovsdb_monitor_states(projection_key, Some(state), expected_schema_hash)
            .await
    }

    async fn publish_ovsdb_monitor_states(
        &self,
        projection_key: &str,
        replacement: Option<op_plugins::state_plugins::ovsdb_bridge::OvsdbMonitorState>,
        expected_schema_hash: &str,
    ) -> anyhow::Result<()> {
        let _state = self.ovsdb_monitor_state.lock().await;
        let mut monitors = match op_core::projection_shm::read_projection_bytes("ovsdb_bridge") {
            Some(bytes) => serde_json::from_slice::<serde_json::Value>(&bytes)?
                .get("monitors")
                .cloned()
                .map(serde_json::from_value)
                .transpose()?
                .unwrap_or_default(),
            None => Vec::new(),
        };
        monitors.retain(
            |monitor: &op_plugins::state_plugins::ovsdb_bridge::OvsdbMonitorState| {
                monitor.projection_key != projection_key
            },
        );
        if let Some(replacement) = replacement {
            monitors.push(replacement);
            monitors.sort_by(|left, right| left.projection_key.cmp(&right.projection_key));
        }
        self.process_observed_property(
            "ovsdb_bridge",
            "monitors",
            simd_json::serde::to_owned_value(monitors)?,
            expected_schema_hash.to_owned(),
        )
        .await
        .map(|_| ())
        .map_err(anyhow::Error::msg)
    }

    async fn start_ovsdb_protocol_monitor(
        &self,
        owner: &str,
        database: &str,
        monitor_id: &str,
        method: &str,
        requests: serde_json::Value,
        expected_schema_hash: &str,
    ) -> anyhow::Result<serde_json::Value> {
        let projection_key = Self::ovsdb_monitor_projection_key(owner, monitor_id);
        {
            let mut tasks = self.ovsdb_monitor_tasks.lock().await;
            tasks.retain(|_, task| !task.is_finished());
            anyhow::ensure!(
                !tasks.contains_key(&projection_key),
                "OVSDB monitor projection is still draining for this caller and ID"
            );
        }
        let start = self
            .ovsdb
            .start_protocol_monitor(owner, database, monitor_id, method, requests)
            .await?;
        // Establish the required root before adding a protected monitor child.
        // A filtered result never becomes the canonical topology authority.
        if let Err(error) = self.publish_ovsdb_topology(expected_schema_hash).await {
            let _ = self.ovsdb.cancel_monitor(owner, monitor_id).await;
            return Err(error.context("publish canonical OVSDB topology before monitor state"));
        }
        if let Err(error) = self
            .publish_ovsdb_monitor_snapshot(
                &projection_key,
                &start.initial,
                true,
                None,
                expected_schema_hash,
            )
            .await
        {
            let _ = self.ovsdb.cancel_monitor(owner, monitor_id).await;
            return Err(error.context("publish initial OVSDB monitor state"));
        }
        let output = serde_json::json!({
            "monitor_id": start.initial.monitor_id.clone(),
            "method": start.initial.method.clone(),
            "sequence": start.initial.sequence,
            "tables": op_plugins::state_plugins::ovsdb_bridge::materialize_monitor_tables(
                &start.initial.tables,
            ),
        });

        let mut receiver = start.receiver;
        receiver.borrow_and_update();
        let mut last_snapshot = start.initial.clone();
        let engine = self.clone();
        let task_key = projection_key.clone();
        let task_owner = owner.to_owned();
        let task_monitor_id = monitor_id.to_owned();
        let task_schema_hash = expected_schema_hash.to_owned();
        let handle = tokio::spawn(async move {
            loop {
                if receiver.changed().await.is_err() {
                    return;
                }
                let observation = receiver.borrow_and_update().clone();
                match observation {
                    op_network::rovs_proxy::monitor::ProtocolObservation::Snapshot(snapshot) => {
                        last_snapshot = snapshot.clone();
                        if let Err(error) = engine
                            .publish_ovsdb_monitor_snapshot(
                                &task_key,
                                &snapshot,
                                true,
                                None,
                                &task_schema_hash,
                            )
                            .await
                        {
                            tracing::warn!(%error, "OVSDB monitor SHM publication failed");
                            let _ = engine
                                .ovsdb
                                .cancel_monitor(&task_owner, &task_monitor_id)
                                .await;
                            return;
                        }
                    }
                    op_network::rovs_proxy::monitor::ProtocolObservation::Unavailable(reason) => {
                        let _ = engine
                            .publish_ovsdb_monitor_snapshot(
                                &task_key,
                                &last_snapshot,
                                false,
                                Some(reason.to_string()),
                                &task_schema_hash,
                            )
                            .await;
                        return;
                    }
                }
            }
        });
        let mut tasks = self.ovsdb_monitor_tasks.lock().await;
        anyhow::ensure!(
            tasks.insert(projection_key, handle).is_none(),
            "OVSDB monitor projection task already exists"
        );
        Ok(output)
    }

    async fn cancel_ovsdb_protocol_monitor(
        &self,
        owner: &str,
        monitor_id: &str,
        expected_schema_hash: &str,
    ) -> anyhow::Result<serde_json::Value> {
        self.ovsdb.cancel_monitor(owner, monitor_id).await?;
        let projection_key = Self::ovsdb_monitor_projection_key(owner, monitor_id);
        if let Some(handle) = self
            .ovsdb_monitor_tasks
            .lock()
            .await
            .remove(&projection_key)
        {
            handle
                .await
                .context("OVSDB monitor projection task failed")?;
        }
        self.publish_ovsdb_monitor_states(&projection_key, None, expected_schema_hash)
            .await?;
        Ok(serde_json::json!({"monitor_id": monitor_id, "canceled": true}))
    }

    pub(crate) async fn lock_host_observation(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.host_observation.lock().await
    }

    #[allow(clippy::too_many_arguments)]
    async fn process_authoritative_change_inner(
        &self,
        plugin_id: String,
        object_path: String,
        change_type: ChangeType,
        member_name: Option<String>,
        old_value: Option<simd_json::OwnedValue>,
        new_value: simd_json::OwnedValue,
        mut tags: Vec<String>,
        actor_id: String,
        capability_id: Option<String>,
        source: ChangeSource,
        mut expected_schema_hash: Option<String>,
    ) -> Result<StateChange, String> {
        if plugin_id == "identity_sled"
            && !matches!(change_type, ChangeType::MethodCall | ChangeType::Signal)
        {
            return Err("identity state requires its durable intent owner".into());
        }
        if tags.is_empty() {
            tags = self.compute_tags(&plugin_id, &object_path);
        }

        // State observation payloads obey the same sealed public policy as
        // notifications. Fold a descendant into a partial root before applying
        // the policy: a child's field names are not top-level policy paths.
        let public_old_value = old_value
            .as_ref()
            .map(|value| public_plugin_value(&plugin_id, value));
        let public_new_value = if matches!(change_type, ChangeType::MethodCall | ChangeType::Signal)
        {
            public_plugin_value(&plugin_id, &new_value)
        } else {
            let catalog_dir = crate::sealed_schema_reader::blob_catalog_dir_from_env();
            let plugin = plugin_id.clone();
            let path = object_path.clone();
            let member = member_name.clone();
            let value = new_value.clone();
            let expected = expected_schema_hash.clone();
            let (payload, pin) = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
                let document = crate::sealed_schema_reader::read_manifest_pinned_schema_document(
                    &catalog_dir,
                    &plugin,
                )?;
                if let Some(expected) = expected {
                    anyhow::ensure!(
                        document.schema_hash == expected,
                        "sealed observation schema changed before audit admission"
                    );
                }
                let partial = crate::state_tree_fold::fold_plugin_state(
                    None,
                    &plugin,
                    &path,
                    change_type,
                    member.as_deref(),
                    &value,
                )?;
                let public = match partial {
                    crate::state_tree_fold::StateTreeFold::Present(state) => document
                        .schema
                        .project_public_state(&public_plugin_value(&plugin, &state))?,
                    _ => None,
                };
                Ok((
                    public.unwrap_or(simd_json::json!(null)),
                    document.schema_hash,
                ))
            })
            .await
            .map_err(|error| error.to_string())?
            .map_err(|error| error.to_string())?;
            expected_schema_hash = Some(pin);
            payload
        };

        // Every recorded mutation carries the verified session identity of
        // its actor: the SID1-backed session id and its genesis, resolved from
        // the same projection the interceptor authenticated against. Owner
        // rule (2026-09-19): identity is transported in metadata and must be
        // embedded in every mutation's metadata, not only authenticated at the
        // door. Resolved before the chain lock so the lookup cannot hold it.
        let session_stamp = self
            .session_context_for_actor(&actor_id)
            .await
            .map(|context| {
                (
                    context.session_id,
                    context.genesis_hex,
                    context.sealed_id_digest,
                )
            });
        if session_stamp.is_none() && actor_id != ANONYMOUS_ACTOR {
            tracing::warn!(
                actor_id = %actor_id,
                plugin_id = %plugin_id,
                "mutation recorded without a resolvable session identity"
            );
        }

        let event = {
            let mut chain = self.event_chain.write().await;
            let op = match change_type {
                ChangeType::PropertySet => OperationType::PropertySet,
                ChangeType::ObjectRemoved => OperationType::Custom("delete".to_string()),
                _ => OperationType::EmitSignal,
            };
            let mut event = ChainEvent::new(
                chain.next_event_id(),
                chain.last_hash().to_string(),
                actor_id.clone(),
                plugin_id.clone(),
                "1.0.0".to_string(),
                op,
                object_path.clone(),
                tags.clone(),
                Decision::Allow,
                &public_new_value,
            );
            if let Some((session_id, session_genesis, sealed_id_digest)) = session_stamp {
                event.session_id = Some(session_id);
                event.session_genesis = Some(session_genesis);
                event.sealed_id_digest = sealed_id_digest;
            }
            if let Some(capability) = capability_id {
                event.capability_id = Some(capability);
            }
            // The member (signal or method name) is the event's operation; the
            // audit footprint and the vector projection key on it to find the
            // declaring schema (OD-64).
            if matches!(change_type, ChangeType::Signal | ChangeType::MethodCall) {
                event.method_name = member_name.clone();
            }
            // Hash covers the identity and capability fields set above.
            event.event_hash = event.compute_hash();
            let event = chain.append(event).clone();
            // Keep persistence ordered with the chain head. Otherwise a
            // method proof could overtake a not-yet-durable state event.
            self.persist_audit_event(&event).await;
            event
        };
        let _ = self.chain_tx.send(event.clone());

        let change = StateChange {
            change_id: uuid::Uuid::new_v4().to_string(),
            event_id: event.event_id,
            plugin_id,
            object_path,
            change_type,
            member_name,
            old_value: public_old_value,
            new_value,
            tags_touched: tags,
            event_hash: event.event_hash.clone(),
            timestamp: event.timestamp,
            actor_id,
            source,
        };

        self.commit_state_change_pinned(change, expected_schema_hash)
            .await
            .map_err(|error| error.to_string())
    }

    /// Start native observations once for this engine. The backend supplies
    /// events; no timer polls or synthetic schema state are used.
    pub async fn start(self: Arc<Self>) -> anyhow::Result<()> {
        if let Err(error) = crate::nlm_cli::touch_bin_ready_marker(&crate::nlm_cli::nlm_bin()) {
            tracing::warn!(%error, "could not update NotebookLM binary readiness marker");
        }
        if !self
            .incus_observer_started
            .swap(true, std::sync::atomic::Ordering::AcqRel)
        {
            let engine = self.clone();
            let started = self.incus_observer_started.clone();
            tokio::spawn(async move {
                if let Err(error) = crate::incus_observer::observe(engine).await {
                    tracing::warn!(%error, "Incus lifecycle observation unavailable; last committed state retained");
                }
                started.store(false, std::sync::atomic::Ordering::Release);
            });
        }
        if !self
            .cron_observer_started
            .swap(true, std::sync::atomic::Ordering::AcqRel)
        {
            let engine = self.clone();
            let started = self.cron_observer_started.clone();
            tokio::spawn(async move {
                if let Err(error) = crate::cron_observer::observe(engine).await {
                    tracing::warn!(%error, "native Cron event observation unavailable; last committed state retained");
                }
                // A later explicit start may retry after the observer exhausts
                // its finite disconnect budget; there is no timer restart.
                started.store(false, std::sync::atomic::Ordering::Release);
            });
        }
        if self
            .observers_started
            .swap(true, std::sync::atomic::Ordering::AcqRel)
        {
            return Ok(());
        }
        let engine = self.clone();
        let client = (*self.ovsdb).clone();
        let started = self.observers_started.clone();
        tokio::spawn(async move {
            if let Err(error) = crate::ovsdb_observer::observe(engine, client).await {
                tracing::warn!(%error, "OVS topology observation unavailable; last committed state retained");
            }
            // A later explicit start/reload may retry; there is no timer retry.
            started.store(false, std::sync::atomic::Ordering::Release);
        });
        Ok(())
    }

    /// Unified mutation entry point. Writes to authoritative RCP stores and
    /// triggers the event recording/broadcast pipeline.
    #[allow(clippy::too_many_arguments)]
    pub async fn mutate(
        &self,
        plugin_id: String,
        object_path: String,
        change_type: ChangeType,
        member_name: Option<String>,
        value: simd_json::OwnedValue,
        actor_id: String,
        capability_id: Option<String>,
    ) -> anyhow::Result<MutationResult> {
        anyhow::ensure!(
            change_type == ChangeType::MethodCall,
            "raw state writes are not backend mutations; use a schema-declared plugin method"
        );
        if change_type == ChangeType::MethodCall {
            let declaration = crate::method_contract_admission::admit_compiled_method(
                &plugin_id,
                member_name
                    .as_deref()
                    .context("method mutation requires a method name")?,
                capability_id.as_deref(),
            )?;
            let mut arguments = serde_json::to_value(&value)?;
            // Legacy identity callers may wrap the object in one positional
            // argument. This is the same normalization the identity adapters
            // below consume; no other plugin gains an argument alias.
            if matches!(plugin_id.as_str(), "identity_sled" | "human_principal")
                && let serde_json::Value::Array(items) = &arguments
            {
                anyhow::ensure!(
                    items.len() == 1,
                    "identity method requires one argument object"
                );
                arguments = items[0].clone();
            }
            crate::method_contract_admission::validate_admitted_arguments(
                &declaration,
                &arguments,
            )?;
        }
        let mut old_value = None;
        let mut authoritative_value = value.clone();
        let mut caller_result = None;
        // Empty falls back to compute_tags in process_authoritative_change;
        // the human_principal arm sets this to the method's subid so the
        // event carries it in tags_touched (VAL-CROSS-020).
        let mut event_tags: Vec<String> = Vec::new();

        // Identity is request/session scoped. An omitted actor never inherits a
        // process-wide identity from compatibility state.
        let actor_id = if actor_id.is_empty() {
            ANONYMOUS_ACTOR.to_string()
        } else {
            actor_id
        };
        let session_to_advance = self.session_context_for_actor(&actor_id).await;

        // 1. Write to authoritative RCP store
        if plugin_id == "net" || object_path.contains("/ovsdb/") {
            // OVSDB Authoritative Path
            if change_type == ChangeType::MethodCall {
                if let Some(method) = &member_name {
                    match method.as_str() {
                        "create_bridge" => {
                            if let Some(name) = value.as_str() {
                                self.ovsdb.create_bridge(name).await?;
                            }
                        }
                        "add_port" => {
                            if let Some(args) = value.as_array()
                                && args.len() >= 2
                                && let (Some(br), Some(port)) = (args[0].as_str(), args[1].as_str())
                            {
                                self.ovsdb.add_port(br, port).await?;
                            }
                        }
                        _ => {
                            // Fallback to generic D-Bus call if it's a known service
                            let _ = self
                                .call_dbus_method(
                                    &format!("org.opdbus.{}.v1", plugin_id),
                                    &object_path,
                                    "org.opdbus.OvsdbV1",
                                    method,
                                    vec![value.clone()],
                                    &actor_id,
                                    &capability_id,
                                )
                                .await?;
                        }
                    }
                }
            } else if change_type == ChangeType::PropertySet
                && let Some(prop) = &member_name
            {
                // Extract bridge name from path if possible
                // Path format: /org/opdbus/v1/ovsdb/Bridge/bridge_name
                let parts: Vec<&str> = object_path.split('/').collect();
                if parts.len() >= 6 && parts[4] == "Bridge" {
                    let br_name = parts[5].replace('_', "-");
                    if let Some(val_str) = value.as_str() {
                        self.ovsdb
                            .set_bridge_property(&br_name, prop, val_str)
                            .await?;
                    }
                }
            }
        } else if plugin_id == "unix_socket" && change_type == ChangeType::MethodCall {
            anyhow::bail!(
                "socket lifecycle requires the authenticated PluginV1 method path; service metadata is registered through shared_unix_socket.create_unix_socket"
            );
        } else if plugin_id == "identity_sled" && change_type == ChangeType::MethodCall {
            // PluginService.CallMethod wraps object arguments in a one-element
            // array. The identity dispatcher owns provisioning, durable Cozo
            // persistence, and the native Btrfs attach operation.
            if let Some(method) = &member_name {
                let mut args_json = serde_json::to_value(&value)?;
                if let serde_json::Value::Array(items) = &args_json {
                    args_json = items
                        .first()
                        .cloned()
                        .unwrap_or_else(|| serde_json::Value::Object(Default::default()));
                }
                // This compatibility entry has no verified transport session.
                // Credential refresh is accepted only by canonical dispatch.
                anyhow::ensure!(
                    method != "refresh_credential",
                    "credential refresh requires verified session metadata"
                );
                let domain = crate::identity_sled_dispatch::dispatch_identity_sled_method(
                    self, method, &args_json,
                )
                .await?;
                caller_result = Some(simd_json::serde::to_owned_value(&domain)?);
            }
        } else if plugin_id == "human_principal" && change_type == ChangeType::MethodCall {
            // PluginService.CallMethod wraps object arguments in a one-element
            // array (identity_sled convention above). The dispatcher owns Cozo
            // durability and registry policy; the authoritative present state
            // is re-read from Cozo after the mutation, and the event carries
            // the method's subid in tags_touched (VAL-CROSS-020).
            if let Some(method) = &member_name {
                let mut args_json = serde_json::to_value(&value)?;
                if let serde_json::Value::Array(items) = &args_json {
                    args_json = items
                        .first()
                        .cloned()
                        .unwrap_or_else(|| serde_json::Value::Object(Default::default()));
                }
                let domain = crate::human_principal_dispatch::dispatch_human_principal_method(
                    method, &args_json,
                )
                .await?;
                let state = crate::human_principal_dispatch::current_state().await;
                authoritative_value = simd_json::serde::to_owned_value(&state)?;
                self.publish_plugin_projection_state(
                    "human_principal",
                    authoritative_value.clone(),
                    ChangeType::PropertySet,
                    true,
                )
                .await?;
                caller_result = Some(simd_json::serde::to_owned_value(&domain)?);
                event_tags = crate::human_principal_dispatch::method_subid(method)
                    .into_iter()
                    .collect();
            }
        } else if plugin_id == "xray" && change_type == ChangeType::MethodCall {
            // PluginService.CallMethod wraps object arguments in a
            // one-element array, matching identity_sled's convention above.
            if let Some(method) = &member_name {
                let mut args_json = serde_json::to_value(&value)?;
                if let serde_json::Value::Array(items) = &args_json {
                    args_json = items
                        .first()
                        .cloned()
                        .unwrap_or_else(|| serde_json::Value::Object(Default::default()));
                }
                let result = dispatch_xray_method(method, &args_json).await?;
                caller_result = Some(simd_json::serde::to_owned_value(&result)?);
            }
        } else if plugin_id == "emqx" && change_type == ChangeType::MethodCall {
            // EMQX owns a typed standalone dispatcher. Keep the original
            // caller arguments as the canonical event payload and return the
            // domain result separately; lifecycle selection is intentionally
            // fixed inside the plugin and cannot be supplied by the caller.
            if let Some(method) = &member_name {
                let mut args_json = serde_json::to_value(&value)?;
                if let serde_json::Value::Array(items) = &args_json {
                    args_json = items
                        .first()
                        .cloned()
                        .unwrap_or_else(|| serde_json::Value::Object(Default::default()));
                }
                let result =
                    op_plugins::state_plugins::emqx::dispatch_emqx_method(method, &args_json)
                        .await?;
                caller_result = Some(simd_json::serde::to_owned_value(&result)?);
            }
        } else if plugin_id == "netmaker" && change_type == ChangeType::MethodCall {
            anyhow::bail!("plugin 'netmaker' is retired and cannot be invoked");
        } else {
            // NonNet / Generic Plugin Path
            if change_type == ChangeType::PropertySet {
                // Get old value for the footprint before update from cache
                old_value = self.get_state(&plugin_id).await.and_then(|v| {
                    if let Some(prop) = &member_name {
                        v.get(prop).cloned()
                    } else {
                        Some(v)
                    }
                });
            }
        }

        let raw_result = caller_result.unwrap_or_else(|| authoritative_value.clone());
        let public_result = public_plugin_value(&plugin_id, &raw_result);

        // 2. Record and broadcast change
        let change = self
            .process_authoritative_change(
                plugin_id.clone(),
                object_path,
                change_type,
                member_name,
                old_value,
                authoritative_value.clone(),
                event_tags, // Empty falls back to compute_tags in process_authoritative_change
                actor_id,
                capability_id,
                ChangeSource::Grpc,
            )
            .await
            .map_err(|e| anyhow::anyhow!(e))?;

        if let Some(session) = session_to_advance
            && (session.sealed_id_digest.is_some()
                || crate::external_mcp_session::lookup(&session.session_id).await?.is_none())
        {
            crate::identity_sled_dispatch::advance_mutation_index(
                self,
                &session.session_id,
                change.event_id,
            )
            .await.context("method completed but its identity mutation index could not be published; recovery may be required")?;
        }

        Ok(MutationResult {
            success: true,
            event_id: change.event_id,
            event_hash: change.event_hash,
            result: Some(public_result),
            error: None,
        })
    }

    /// Backward-compatible wrapper for gRPC Mutations.
    #[allow(clippy::too_many_arguments)]
    pub async fn process_grpc_mutation(
        &self,
        plugin_id: String,
        object_path: String,
        change_type: ChangeType,
        member_name: Option<String>,
        value: simd_json::OwnedValue,
        actor_id: String,
        capability_id: Option<String>,
    ) -> anyhow::Result<MutationResult> {
        self.mutate(
            plugin_id,
            object_path,
            change_type,
            member_name,
            value,
            actor_id,
            capability_id,
        )
        .await
    }

    /// Dispatch a method call from the schema-backed bridge interface.
    ///
    /// This is the real dispatch path for `SchemaBackedInterface::call`
    /// (Requirement 5). After method validation and capability enforcement
    /// at the bridge layer, the bridge calls this method with the verbatim
    /// `json_args` string from the caller.
    ///
    /// The method:
    /// 1. Records an immutable event in the event chain via
    ///    [`EventChain::record_method_call`] including `actor_id`,
    ///    `plugin_id`, `method_name`, `capability_id`, and a Blake3
    ///    bounded canonical `json_args` payload (Requirement 5.5 / VAL-DISPATCH-005).
    /// 2. The append occurs **before** the call returns success
    ///    (NFR-003 / VAL-NFR-003).
    /// 3. Broadcasts the change to subscribers.
    /// 4. Returns the result as a `serde_json::Value`.
    ///
    /// Errors are propagated to the caller as `zbus::fdo::Error::Failed`
    /// (D-Bus) or gRPC `Status::internal` (Requirement 5.3 /
    /// VAL-DISPATCH-003) by the calling bridge interface.
    pub async fn dispatch_method_call(
        &self,
        plugin_id: &str,
        method: &str,
        json_args: &str,
        capability_id: Option<&str>,
        actor_id: &str,
    ) -> anyhow::Result<serde_json::Value> {
        self.dispatch_method_call_with_identity(
            plugin_id,
            method,
            json_args,
            capability_id,
            actor_id,
            None,
            None,
        )
        .await
    }

    /// Dispatch with the verified session metadata attached to the canonical
    /// event payload. Session values are correlation fields only; `actor_id`
    /// is the exact stable principal used for authorization and audit identity.
    #[allow(clippy::too_many_arguments)]
    pub fn dispatch_method_call_with_identity<'a>(
        &'a self,
        plugin_id: &'a str,
        method: &'a str,
        json_args: &'a str,
        capability_id: Option<&'a str>,
        actor_id: &'a str,
        session_id: Option<&'a str>,
        session_genesis: Option<&'a str>,
    ) -> futures::future::BoxFuture<'a, anyhow::Result<serde_json::Value>> {
        self.dispatch_method_call_with_generation_origin(
            plugin_id,
            method,
            json_args,
            capability_id,
            actor_id,
            session_id,
            session_genesis,
            None,
        )
    }

    /// Caller-chosen correlation only; identity and grants still come solely
    /// from the receiver. Reusing an admitted ID cannot execute another effect.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn dispatch_method_call_with_operation_id<'a>(
        &'a self,
        plugin_id: &'a str,
        method: &'a str,
        json_args: &'a str,
        capability_id: Option<&'a str>,
        actor_id: &'a str,
        session_id: Option<&'a str>,
        session_genesis: Option<&'a str>,
        operation_id: &'a str,
    ) -> futures::future::BoxFuture<'a, anyhow::Result<serde_json::Value>> {
        self.dispatch_method_call_with_origins(
            plugin_id,
            method,
            json_args,
            capability_id,
            actor_id,
            session_id,
            session_genesis,
            None,
            None,
            None,
            Some(operation_id),
        )
    }

    /// Internal facade evidence is supplied only by the admitted LLM wrapper.
    /// It conveys no grant and does not bypass the same canonical admission.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn dispatch_method_call_with_generation_origin<'a>(
        &'a self,
        plugin_id: &'a str,
        method: &'a str,
        json_args: &'a str,
        capability_id: Option<&'a str>,
        actor_id: &'a str,
        session_id: Option<&'a str>,
        session_genesis: Option<&'a str>,
        generation_origin: Option<crate::generation_admission::GenerationOrigin>,
    ) -> futures::future::BoxFuture<'a, anyhow::Result<serde_json::Value>> {
        self.dispatch_method_call_with_origins(
            plugin_id,
            method,
            json_args,
            capability_id,
            actor_id,
            session_id,
            session_genesis,
            generation_origin,
            None,
            None,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn dispatch_method_call_with_facade_origin<'a>(
        &'a self,
        plugin_id: &'a str,
        method: &'a str,
        json_args: &'a str,
        capability_id: Option<&'a str>,
        actor_id: &'a str,
        session_id: Option<&'a str>,
        session_genesis: Option<&'a str>,
        origin: crate::tched_router_admission::FacadeOrigin,
    ) -> futures::future::BoxFuture<'a, anyhow::Result<serde_json::Value>> {
        self.dispatch_method_call_with_origins(
            plugin_id,
            method,
            json_args,
            capability_id,
            actor_id,
            session_id,
            session_genesis,
            None,
            Some(origin),
            None,
            None,
        )
    }

    /// Only the receiver can supply this typed proof; plugin arguments never
    /// select an actor or manufacture transport attribution.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn dispatch_method_call_with_bound_oracle<'a>(
        &'a self,
        plugin_id: &'a str,
        method: &'a str,
        json_args: &'a str,
        capability_id: Option<&'a str>,
        actor_id: &'a str,
        session_id: Option<&'a str>,
        session_genesis: Option<&'a str>,
        proof: Option<crate::oracle_assertion::BoundOracleProof>,
    ) -> futures::future::BoxFuture<'a, anyhow::Result<serde_json::Value>> {
        self.dispatch_method_call_with_origins(
            plugin_id,
            method,
            json_args,
            capability_id,
            actor_id,
            session_id,
            session_genesis,
            None,
            None,
            proof,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn dispatch_method_call_with_origins<'a>(
        &'a self,
        plugin_id: &'a str,
        method: &'a str,
        json_args: &'a str,
        capability_id: Option<&'a str>,
        actor_id: &'a str,
        session_id: Option<&'a str>,
        session_genesis: Option<&'a str>,
        generation_origin: Option<crate::generation_admission::GenerationOrigin>,
        facade_origin: Option<crate::tched_router_admission::FacadeOrigin>,
        bound_oracle: Option<crate::oracle_assertion::BoundOracleProof>,
        operation_id: Option<&'a str>,
    ) -> futures::future::BoxFuture<'a, anyhow::Result<serde_json::Value>> {
        // Explicit Send erasure breaks recursive facade/property dispatch's
        // future-type cycle while preserving all pre-task admission below.
        Box::pin(async move {
            if let Some(operation_id) = operation_id {
                let parsed = uuid::Uuid::parse_str(operation_id)?;
                anyhow::ensure!(
                    parsed.to_string() == operation_id,
                    "noncanonical operation id"
                );
            }
            anyhow::ensure!(
                json_args.len() <= op_state_store::method_receipt::MAX_METHOD_ARGUMENT_BYTES,
                "method arguments exceed 1 MiB"
            );
            if let Some(proof) = &bound_oracle {
                crate::interceptor::revalidate_bound_oracle(
                    self,
                    proof,
                    actor_id,
                    session_id,
                    session_genesis,
                    true,
                )
                .await?;
                require_bound_oracle_grant(actor_id, capability_id)?;
            }
            if let Some(origin) = &facade_origin {
                origin.validate_target(plugin_id, method)?;
            }
            // Test-only: ForceDispatchError triggers an error for VAL-DISPATCH-003 testing.
            // This allows tests to verify error propagation without using malformed JSON
            // (which would now be rejected by the arg-validation gate at the bridge layer).
            if method == "ForceDispatchError" {
                return Err(anyhow::anyhow!(
                    "dispatch error: method explicitly triggered failure"
                ));
            }

            // Parse the verbatim json_args string. If parsing fails, propagate
            // the error — the bridge converts it to fdo::Error::Failed.
            let parsed_value: simd_json::OwnedValue = {
                let mut bytes = json_args.as_bytes().to_vec();
                simd_json::to_owned_value(&mut bytes).map_err(|e| {
                    anyhow::anyhow!("invalid json_args for method '{}': {}", method, e)
                })?
            };
            if plugin_id == "identity_sled" && contains_identity_credential_field(&parsed_value) {
                anyhow::bail!("sealed_id is MutationEngine-authored and cannot be supplied");
            }

            // Admit the sealed method before notarizing arguments so OD-91 can
            // digest `x-opdbus-private` fields from the method's args schema.
            let (declaration, admitted_schema) =
                crate::method_contract_admission::admit_compiled_invocation(
                    plugin_id,
                    method,
                    capability_id,
                )?;
            anyhow::ensure!(declaration.returns.is_some(), "missing_result_contract");
            let return_contract = serde_json::to_value(
                declaration
                    .returns
                    .as_ref()
                    .expect("checked result contract"),
            )?;
            jsonschema::validator_for(&return_contract)
                .context("invalid admitted result contract")?;
            let mut canonical_args = serde_json::to_value(&parsed_value)?;
            crate::method_contract_admission::validate_admitted_arguments(
                &declaration,
                &canonical_args,
            )?;
            // Keep the exact admitted arguments used to issue a one-use
            // elevation. Consumption belongs to the owned task, after all
            // cancellable admission awaits and capacity acquisition.
            let elevation_arguments = canonical_args.clone();
            crate::schema_integer_arguments::normalize_declared_integers(
                &serde_json::to_value(&declaration.args)?,
                &mut canonical_args,
            )?;
            let json_args = serde_json::to_string(&canonical_args)?;
            let parsed_value = simd_json::serde::to_owned_value(canonical_args.clone())?;
            let args_schema: serde_json::Value = serde_json::to_value(&declaration.args)?;

            // Compliance graph deny rules (no-op when the cognitive Cozo store
            // is absent or has no Deny rows).
            if let Ok(mcp) = self.cognitive_mcp().await {
                let verdict = mcp.cozo_shuttle().evaluate_mutation(plugin_id, method);
                if !verdict.allow {
                    anyhow::bail!(
                        "compliance policy denied {}.{}: {}",
                        plugin_id,
                        method,
                        verdict.reason
                    );
                }
            }

            // Tenant data must not leak through the global chain/change streams.
            // Notarize digests / redaction, while dispatch still receives originals.
            let audit_json =
                audit_method_arguments(plugin_id, method, &json_args, Some(&args_schema));
            // Commitment to the authoring SID1 for the audit record: from the
            // anchored session when the caller named one, else from the actor's
            // own sled. Resolved before the chain lock is taken.
            let method_sealed_id_digest = match session_id {
                Some(sid) => self
                    .session_context(sid)
                    .await
                    .and_then(|context| context.sealed_id_digest),
                None => self
                    .session_context_for_actor(actor_id)
                    .await
                    .and_then(|context| context.sealed_id_digest),
            };
            let audit_value = simd_json::serde::to_owned_value(serde_json::from_str::<
                serde_json::Value,
            >(&audit_json)?)?;

            // The owner task encloses persistence too: dropping the caller while
            // the durable writer runs cannot leave a committed, untracked attempt.
            let change = StateChange {
                change_id: operation_id
                    .map(str::to_owned)
                    .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
                event_id: 0,
                plugin_id: plugin_id.to_string(),
                object_path: format!("/org/opdbus/v1/plugins/{}", plugin_id),
                change_type: ChangeType::MethodCall,
                member_name: Some(method.to_string()),
                old_value: None,
                new_value: audit_value,
                tags_touched: vec![],
                event_hash: String::new(),
                timestamp: chrono::Utc::now(),
                actor_id: actor_id.to_string(),
                source: ChangeSource::DBus,
            };
            let operation = owned_method_task::OperationMetadata {
                operation_id: change.change_id.clone(),
                event_id: change.event_id,
                plugin_id: plugin_id.to_string(),
                method: method.to_string(),
            };
            let call = AdmittedMethodCall {
                plugin_id: plugin_id.to_string(),
                method: method.to_string(),
                json_args,
                capability_id: capability_id.map(str::to_owned),
                actor_id: actor_id.to_string(),
                session_id: session_id.map(str::to_owned),
                session_genesis: session_genesis.map(str::to_owned),
                parsed_value,
                admitted_schema,
                generation_origin,
                facade_origin,
                bound_oracle,
                change,
            };
            let engine = self.clone();
            let permit = self
                .dbus_call_limiter
                .clone()
                .try_acquire_owned()
                .map_err(|_| anyhow::anyhow!("admitted_method_capacity_exhausted"))?;
            // Transport identity/grants precede engine entry; compiled contract and
            // input admission above precede this spawn. Backend-specific identity
            // checks and independently admitted nested target calls stay intact.
            owned_method_task::spawn(operation, async move {
                let _permit = permit;
                engine.task_elevation.consume(
                    &call.actor_id,
                    call.session_id.as_deref(),
                    &call.plugin_id,
                    &call.method,
                    &elevation_arguments,
                )?;
                drop(elevation_arguments);
                let scoped_actor = call.actor_id.clone();
                let scoped_session = call.session_id.clone();
                let scoped_genesis = call.session_genesis.clone();
                crate::external_mcp_session::with_authoring_scope(
                    &engine, &scoped_actor, scoped_session.as_deref(), scoped_genesis.as_deref(),
                    // Keep the large audit future out of task-local wrapper frames.
                    Box::pin(engine.audit_and_dispatch_admitted(call, method_sealed_id_digest)),
                ).await?
            })
            .await
            .map_err(|_| anyhow::anyhow!("admitted_method_owner_failed"))?
        })
    }

    async fn dispatch_admitted_method(
        &self,
        call: AdmittedMethodCall,
    ) -> anyhow::Result<serde_json::Value> {
        let AdmittedMethodCall {
            plugin_id,
            method,
            json_args,
            capability_id,
            actor_id,
            session_id,
            session_genesis,
            parsed_value,
            admitted_schema,
            generation_origin,
            facade_origin,
            bound_oracle,
            change,
        } = call;
        let plugin_id = plugin_id.as_str();
        let method = method.as_str();
        let json_args = json_args.as_str();
        let capability_id = capability_id.as_deref();
        let actor_id = actor_id.as_str();
        let session_id = session_id.as_deref();
        let session_genesis = session_genesis.as_deref();
        if let Some(proof) = &bound_oracle {
            crate::interceptor::revalidate_bound_oracle(
                self,
                proof,
                actor_id,
                session_id,
                session_genesis,
                true,
            )
            .await?;
            require_bound_oracle_grant(actor_id, capability_id)?;
        }
        let mut method_result: serde_json::Value = match plugin_id {
            "mcp" => {
                crate::mcp_config_dispatch::dispatch(
                    self,
                    method,
                    &serde_json::to_value(&parsed_value)?,
                    actor_id,
                    session_id,
                    session_genesis,
                    &admitted_schema,
                )
                .await?
            }
            "elevation" => {
                crate::task_elevation::dispatch(
                    self,
                    method,
                    &serde_json::to_value(&parsed_value)?,
                    actor_id,
                    session_id,
                    session_genesis,
                )
                .await?
            }
            "model_registry" => {
                crate::model_registry_dispatch::dispatch(
                    self,
                    method,
                    &serde_json::to_value(&parsed_value)?,
                    actor_id,
                    session_id,
                    &admitted_schema,
                )
                .await?
            }
            "json_render" if crate::json_render_gallery_dispatch::is_gallery_method(method) => {
                let result = crate::json_render_gallery_dispatch::dispatch(
                    self,
                    method,
                    &serde_json::to_value(&parsed_value)?,
                    actor_id,
                    session_id,
                    session_genesis,
                    &admitted_schema,
                )
                .await?;
                // Refresh active-catalog names into observable plugin state after
                // gallery mutations so normal modes can subscribe (OD-93).
                if let Ok(documents) =
                    op_plugins::state_plugins::runtime_documents::runtime_documents().await
                    && let Ok(service) = self
                        .json_render_methods_enriched(admitted_schema.clone(), documents)
                        .await
                {
                    let enriched = service.projection_state().await?;
                    if let Err(error) = self
                        .publish_plugin_projection_state(
                            plugin_id,
                            simd_json::serde::to_owned_value(enriched)?,
                            ChangeType::PropertySet,
                            true,
                        )
                        .await
                    {
                        tracing::warn!(
                            error = %format!("{error:#}"),
                            "json_render catalog projection refresh failed after gallery method"
                        );
                    }
                }
                result
            }
            "json_render" => {
                static PROJECTION_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
                let _guard = PROJECTION_LOCK.lock().await;
                let store =
                    op_plugins::state_plugins::runtime_documents::runtime_documents().await?;
                let service = self
                    .json_render_methods_enriched(admitted_schema, store)
                    .await?;
                let result = service
                    .dispatch(method, &serde_json::to_value(&parsed_value)?)
                    .await?;
                if method == "set_config" {
                    let enriched = service.projection_state().await?;
                    self.publish_plugin_projection_state(
                        plugin_id,
                        simd_json::serde::to_owned_value(enriched)?,
                        ChangeType::PropertySet,
                        true,
                    ).await.context("json_render policy committed but projection publication failed; retry to reconcile")?;
                }
                result
            }
            "schema_renderer" => {
                crate::schema_renderer_dispatch::dispatch(
                    self,
                    method,
                    &serde_json::to_value(&parsed_value)?,
                    actor_id,
                    session_id,
                    session_genesis,
                    &admitted_schema,
                )
                .await?
            }
            "gemma_brain" if method == "route" => {
                crate::gemma_brain_dispatch::dispatch(
                    self,
                    &serde_json::to_value(&parsed_value)?,
                    actor_id,
                    session_id,
                    session_genesis,
                    &admitted_schema,
                )
                .await?
            }
            "gemma_brain" if crate::ui_workspace_dispatch::is_ui_workspace_method(method) => {
                self.ui_workspace()?
                    .dispatch(
                        self,
                        &admitted_schema,
                        method,
                        &serde_json::to_value(&parsed_value)?,
                        actor_id,
                        session_id,
                        session_genesis,
                        &change.change_id,
                        facade_origin,
                    )
                    .await?
            }
            "rovs_commands" => {
                dispatch_rovs_commands_method(&self.ovsdb, method, &parsed_value).await?
            }
            "ovsdb_bridge" => {
                static OVSDB_PROJECTION_LOCK: tokio::sync::Mutex<()> =
                    tokio::sync::Mutex::const_new(());
                let _operation = OVSDB_PROJECTION_LOCK.lock().await;
                let caller = crate::tched_router_admission::RouterCaller {
                    actor: actor_id.into(),
                    capability: capability_id.map(str::to_owned),
                    session: session_id.map(str::to_owned),
                    genesis: session_genesis.map(str::to_owned),
                };
                let pinned = crate::tched_router_admission::readmit_call(
                    self,
                    &admitted_schema,
                    "ovsdb_bridge",
                    method,
                    &caller,
                )
                .await?;
                let owner = Self::ovsdb_owner_scope(actor_id, session_id, session_genesis);
                let result = match method {
                    "monitor" => {
                        let input: op_plugins::state_plugins::ovsdb_bridge::MonitorInput =
                            serde_json::from_value(serde_json::to_value(&parsed_value)?)?;
                        anyhow::ensure!(
                            input.db_name == "Open_vSwitch",
                            "unsupported OVSDB database"
                        );
                        self.start_ovsdb_protocol_monitor(
                            &owner,
                            &input.db_name,
                            &input.monitor_id,
                            "monitor",
                            serde_json::to_value(input.monitor_requests)?,
                            &pinned.schema_hash,
                        )
                        .await?
                    }
                    "monitor_cond" => {
                        let input: op_plugins::state_plugins::ovsdb_bridge::MonitorCondInput =
                            serde_json::from_value(serde_json::to_value(&parsed_value)?)?;
                        anyhow::ensure!(
                            input.db_name == "Open_vSwitch",
                            "unsupported OVSDB database"
                        );
                        self.start_ovsdb_protocol_monitor(
                            &owner,
                            &input.db_name,
                            &input.monitor_id,
                            "monitor_cond",
                            serde_json::to_value(input.monitor_cond_requests)?,
                            &pinned.schema_hash,
                        )
                        .await?
                    }
                    "cancel" => {
                        let input: op_plugins::state_plugins::ovsdb_bridge::CancelInput =
                            serde_json::from_value(serde_json::to_value(&parsed_value)?)?;
                        self.cancel_ovsdb_protocol_monitor(&owner, &input.id, &pinned.schema_hash)
                            .await?
                    }
                    _ => {
                        dispatch_ovsdb_bridge_method(&self.ovsdb, &owner, method, &parsed_value)
                            .await?
                    }
                };
                if matches!(
                    method,
                    "create_bridge" | "delete_bridge" | "add_port" | "remove_port" | "transact"
                ) {
                    self.publish_ovsdb_topology(&pinned.schema_hash).await.context(
                        "OVSDB operation completed but topology publication failed; reconcile before retry",
                    )?;
                }
                result
            }
            "xray" => {
                let args = serde_json::to_value(&parsed_value)?;
                dispatch_xray_method(method, &args).await?
            }
            "emqx" => {
                let args = serde_json::to_value(&parsed_value)?;
                op_plugins::state_plugins::emqx::dispatch_emqx_method(method, &args).await?
            }
            "workflows" => {
                let args = serde_json::to_value(&parsed_value)?;
                crate::workstack_runner::dispatch(
                    self,
                    method,
                    &args,
                    actor_id,
                    session_id,
                    session_genesis,
                )
                .await?
            }
            "identity_sled" => {
                let args = serde_json::to_value(&parsed_value)?;
                crate::identity_sled_dispatch::dispatch_identity_sled_method_for_verified_session(
                    self,
                    method,
                    &args,
                    actor_id,
                    session_id,
                    session_genesis,
                )
                .await?
            }
            "cron" => {
                crate::cron_dispatch::dispatch(
                    self,
                    method,
                    &serde_json::to_value(&parsed_value)?,
                    actor_id,
                    capability_id,
                    session_id,
                    session_genesis,
                    &admitted_schema,
                )
                .await?
            }
            "ctl_plane_chatbot" => {
                crate::ctl_plane_chatbot_dispatch::dispatch(
                    self,
                    method,
                    &serde_json::to_value(&parsed_value)?,
                    actor_id,
                    session_id,
                    session_genesis,
                    &admitted_schema,
                )
                .await?
            }
            "chat_response" => {
                crate::chat_response_dispatch::dispatch(
                    self,
                    method,
                    &serde_json::to_value(&parsed_value)?,
                    actor_id,
                    session_id,
                    session_genesis,
                    &admitted_schema,
                    crate::chat_response_dispatch::RequestEvent {
                        event_id: change.event_id,
                        event_hash: change.event_hash.clone(),
                    },
                )
                .await?
            }
            "human_principal" => {
                let args = serde_json::to_value(&parsed_value)?;
                let result =
                    crate::human_principal_dispatch::dispatch_human_principal_method(method, &args)
                        .await?;
                let state = crate::human_principal_dispatch::current_state().await;
                self.publish_plugin_projection_state(
                    plugin_id,
                    simd_json::serde::to_owned_value(state)?,
                    ChangeType::PropertySet,
                    true,
                )
                .await?;
                result
            }
            "project" => {
                crate::project_dispatch::dispatch(
                    self,
                    method,
                    &serde_json::to_value(&parsed_value)?,
                    actor_id,
                    session_id,
                    session_genesis,
                )
                .await?
            }
            "persona" => {
                let args = serde_json::to_value(&parsed_value)?;
                if method == "execute" {
                    let input: op_plugins::state_plugins::persona::ExecuteInput =
                        serde_json::from_value(args)?;
                    let target = op_plugins::state_plugins::cognitive_agents::catalog_agent_method(
                        &input.agent_type,
                        &input.operation,
                    )
                    .context("unknown agent operation")?;
                    let required = target
                        .declaration
                        .required_capability
                        .as_deref()
                        .context("agent requires exact capability")?;
                    anyhow::ensure!(
                        crate::interceptor::load_capability_grants(actor_id).contains(required),
                        "persona execution requires the target agent capability"
                    );
                    let mut arguments = input.args.unwrap_or_else(|| serde_json::json!({}));
                    let object = arguments
                        .as_object_mut()
                        .context("agent arguments must be an object")?;
                    if let Some(question) = input.query {
                        object.insert("question".into(), question.into());
                    }
                    if let Some(path) = input.path {
                        object.insert("path".into(), path.into());
                    }
                    let envelope = self
                        .dispatch_method_call_with_identity(
                            "cognitive_mcp",
                            &target.declaration.name,
                            &serde_json::to_string(&arguments)?,
                            Some(required),
                            actor_id,
                            session_id,
                            session_genesis,
                        )
                        .await?;
                    serde_json::json!({"success":true,"agent_type":input.agent_type,
                        "operation":input.operation,"data":envelope.get("result").unwrap_or(&envelope)})
                } else {
                    op_plugins::state_plugins::persona::dispatch_persona_method(method, &args)
                        .await?
                }
            }
            "gcloud_adc" => {
                let args = serde_json::to_value(&parsed_value)?;
                op_plugins::state_plugins::gcloud_adc::dispatch_gcloud_adc_method(method, &args)
                    .await?
            }
            "adc" => {
                let args = serde_json::to_value(&parsed_value)?;
                op_plugins::state_plugins::adc::dispatch_adc_method(method, &args).await?
            }
            "full_system" => {
                let args = serde_json::to_value(&parsed_value)?;
                op_plugins::state_plugins::full_system::dispatch_full_system_method(method, &args)
                    .await?
            }
            "config" | "endpoint" | "agent_config" => {
                // Hold across commit and publication so concurrent method
                // calls cannot publish an older durable revision last.
                static PROJECTION_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
                let _guard = PROJECTION_LOCK.lock().await;
                let store =
                    op_plugins::state_plugins::runtime_documents::runtime_documents().await?;
                let args = serde_json::to_value(&parsed_value)?;
                let result = match plugin_id {
                    "config" => {
                        op_plugins::state_plugins::config::dispatch_config_with_store(
                            store.clone(),
                            method,
                            &args,
                        )
                        .await?
                    }
                    "endpoint" => {
                        op_plugins::state_plugins::endpoint::dispatch_endpoint_with_store(
                            &store, method, &args,
                        )
                        .await?
                    }
                    _ => {
                        op_plugins::state_plugins::agent_config::dispatch_agent_config_with_store(
                            store.clone(),
                            method,
                            &args,
                        )
                        .await?
                    }
                };
                if matches!(
                    method,
                    "set_value"
                        | "delete_key"
                        | "add_endpoint"
                        | "remove_endpoint"
                        | "update_config"
                        | "register_tool"
                        | "reset_config"
                ) {
                    let row = store
                        .read(plugin_id)
                        .await?
                        .context("committed plugin state is missing")?;
                    self.publish_plugin_projection_state(plugin_id, simd_json::serde::to_owned_value(row.document)?, ChangeType::PropertySet, true).await
                        .context("plugin state committed but projection publication failed; retry to reconcile")?;
                }
                result
            }
            "qdrant" => {
                let args = serde_json::to_value(&parsed_value)?;
                let dispatch_result =
                    op_plugins::state_plugins::qdrant::dispatch_qdrant_method(method, &args)
                        .await?;
                // Same gap as snowball: this plugin is dispatched only through
                // the typed interface here, which never commits to the SHM
                // projection on its own.
                match op_plugins::state_plugins::qdrant::current_state().await {
                    Ok(state) => {
                        if let Err(error) = self
                            .publish_plugin_projection_state(
                                "qdrant",
                                simd_json::serde::to_owned_value(state)?,
                                ChangeType::PropertySet,
                                true,
                            )
                            .await
                        {
                            tracing::warn!(%error, "qdrant state read but projection publish failed");
                        }
                    }
                    Err(error) => {
                        tracing::warn!(%error, "qdrant method completed but current state could not be read for projection")
                    }
                }
                dispatch_result
            }
            "cozo" => {
                let args = serde_json::to_value(&parsed_value)?;
                if method == "refresh_system_graph" {
                    let input: op_plugins::state_plugins::cozo::RefreshSystemGraphInput =
                        serde_json::from_value(args)?;
                    serde_json::to_value(self.refresh_system_graph(input.expected_revision).await?)?
                } else {
                    let store = self.cognitive_mcp().await?.cozo_shuttle();
                    op_plugins::state_plugins::cozo::dispatch_cozo_method(store, method, &args)
                        .await?
                }
            }
            "memory" => {
                use op_plugins::state_plugins::memory_methods::{
                    MemoryAccessContext, MemoryMethods,
                };
                let session = crate::project_dispatch::verified_session(
                    self,
                    actor_id,
                    session_id,
                    session_genesis,
                )
                .await?;
                let service = self
                    .memory_methods
                    .get_or_try_init(|| async {
                        let store = self.cognitive_mcp().await?.cozo_shuttle();
                        Ok::<_, anyhow::Error>(MemoryMethods::from_shared(store))
                    })
                    .await?;
                let context = MemoryAccessContext {
                    principal_id: actor_id.to_owned(),
                    session_id: session,
                };
                let args = serde_json::to_value(&parsed_value)?;
                service.dispatch(&context, method, &args).await?
            }
            "embedding_model" => {
                static PROJECTION_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
                let _guard = PROJECTION_LOCK.lock().await;
                let args = serde_json::to_value(&parsed_value)?;
                let caller = crate::tched_router_admission::RouterCaller {
                    actor: actor_id.to_owned(),
                    capability: capability_id.map(str::to_owned),
                    session: session_id.map(str::to_owned),
                    genesis: session_genesis.map(str::to_owned),
                };
                let pinned = crate::tched_router_admission::readmit_call(
                    self,
                    &admitted_schema,
                    "embedding_model",
                    method,
                    &caller,
                )
                .await?;
                let declaration = pinned
                    .schema
                    .methods
                    .get(method)
                    .context("admitted embedding method is absent")?;
                crate::method_contract_admission::validate_admitted_arguments(declaration, &args)?;
                if let Some(origin) = facade_origin {
                    origin.readmit(self, &caller).await?;
                }
                let result =
                    op_plugins::state_plugins::embedding_model::dispatch_embedding_model_method(
                        method, &args,
                    )
                    .await?;
                if matches!(method, "set_model" | "update_config") {
                    let state =
                        op_plugins::state_plugins::embedding_model::embedding_model_state().await?;
                    self.publish_plugin_projection_state("embedding_model", simd_json::serde::to_owned_value(state)?, ChangeType::PropertySet, true).await
                        .context("embedding config committed but projection publication failed; retry to reconcile")?;
                }
                result
            }
            "rtnetlink" => {
                let args = serde_json::to_value(&parsed_value)?;
                op_plugins::state_plugins::rtnetlink::dispatch_rtnetlink_method(method, &args)
                    .await?
            }
            "net" => {
                let args = serde_json::to_value(&parsed_value)?;
                op_plugins::state_plugins::net::dispatch_net_method(method, &args).await?
            }
            "hardware" => {
                let _observation = self.lock_host_observation().await;
                let args = serde_json::to_value(&parsed_value)?;
                let method = method.to_owned();
                let observed = tokio::task::spawn_blocking(move || {
                    op_plugins::state_plugins::hardware::HardwarePlugin::dispatch_hardware_method_observed(
                        &method, &args,
                    )
                })
                .await??;
                self.publish_plugin_projection_state(
                    plugin_id,
                    simd_json::serde::to_owned_value(observed.state)?,
                    ChangeType::PropertySet,
                    true,
                )
                .await?;
                observed.output
            }
            "pcidecl" => {
                let args = serde_json::to_value(&parsed_value)?;
                op_plugins::state_plugins::pcidecl::dispatch_pcidecl_method(method, &args).await?
            }
            "software" => {
                let args = serde_json::to_value(&parsed_value)?;
                op_plugins::state_plugins::software::dispatch_software_method(method, &args).await?
            }
            "threetched_fs" => {
                let args = serde_json::to_value(&parsed_value)?;
                let output = if method == "get_receipt" {
                    let request: op_state_store::method_receipt::GetMethodReceiptInput =
                        serde_json::from_value(args)?;
                    serde_json::to_value(
                        self.recover_method_receipt(&request.operation_id, actor_id, session_id)
                            .await?,
                    )?
                } else {
                    op_plugins::state_plugins::threetched_fs::dispatch(method, &args)?
                };
                self.publish_plugin_projection_state(
                    plugin_id,
                    op_plugins::state_plugins::threetched_fs::protocol_value()?,
                    ChangeType::PropertySet,
                    true,
                )
                .await?;
                output
            }
            "procfs" => {
                let _observation = self.lock_host_observation().await;
                let args = serde_json::to_value(&parsed_value)?;
                let method = method.to_owned();
                let observed = tokio::task::spawn_blocking(move || {
                    op_plugins::state_plugins::procfs::dispatch_procfs_method_observed(
                        &method, &args,
                    )
                })
                .await??;
                self.publish_plugin_projection_state(
                    plugin_id,
                    simd_json::serde::to_owned_value(observed.state)?,
                    ChangeType::PropertySet,
                    true,
                )
                .await?;
                observed.output
            }
            "wireguard" | "wg_opdbus" | "wgcf" => {
                let _operation = self.decoy_network_projection.lock().await;
                let plugin = match plugin_id {
                    "wireguard" => "wireguard",
                    "wg_opdbus" => "wg_opdbus",
                    _ => "wgcf",
                };
                let caller = crate::tched_router_admission::RouterCaller {
                    actor: actor_id.into(),
                    capability: capability_id.map(str::to_owned),
                    session: session_id.map(str::to_owned),
                    genesis: session_genesis.map(str::to_owned),
                };
                let receipt = crate::decoy_network::dispatch(
                    self,
                    plugin,
                    method,
                    &serde_json::to_value(&parsed_value)?,
                    &admitted_schema,
                    &caller,
                )
                .await?;
                for (state_plugin, state) in receipt.states {
                    let expected = receipt
                        .state_schema_hashes
                        .get(&state_plugin)
                        .context("decoy observation omitted its schema pin")?;
                    self.process_observed_state(&state_plugin, simd_json::serde::to_owned_value(state)?, expected.clone()).await
                        .map_err(anyhow::Error::msg).context("decoy operation completed but SHM publication failed; reconcile before retry")?;
                }
                receipt.output
            }
            "mail_server" => {
                let _operation = self.mail_projection.lock().await;
                let caller = crate::tched_router_admission::RouterCaller {
                    actor: actor_id.into(),
                    capability: capability_id.map(str::to_owned),
                    session: session_id.map(str::to_owned),
                    genesis: session_genesis.map(str::to_owned),
                };
                let pinned = crate::tched_router_admission::readmit_call(
                    self,
                    &admitted_schema,
                    "mail_server",
                    method,
                    &caller,
                )
                .await?;
                let receipt =
                    op_plugins::state_plugins::mail_server::methods::dispatch_mail_server_method(
                        method,
                        &serde_json::to_value(&parsed_value)?,
                    )
                    .await?;
                self.commit_state_update(StateChange {
                    change_id: uuid::Uuid::new_v4().to_string(), event_id: 0,
                    plugin_id: "mail_server".into(), object_path: op_plugins::canonical::plugin_path("mail_server"),
                    change_type: ChangeType::PropertySet, member_name: Some("administration".into()),
                    old_value: None, new_value: simd_json::serde::to_owned_value(receipt.administration)?,
                    tags_touched: Vec::new(), event_hash: String::new(), timestamp: chrono::Utc::now(),
                    actor_id: actor_id.into(), source: ChangeSource::Internal,
                }, Some(pinned.schema_hash), false).await
                    .context("native mail operation completed but SHM publication failed; reconcile before retry")?;
                receipt.output
            }
            "antigravity" | "antigravity_chat" => {
                let _operation = self.antigravity_projection.lock().await;
                let plugin = if plugin_id == "antigravity" {
                    "antigravity"
                } else {
                    "antigravity_chat"
                };
                let caller = crate::tched_router_admission::RouterCaller {
                    actor: actor_id.into(),
                    capability: capability_id.map(str::to_owned),
                    session: session_id.map(str::to_owned),
                    genesis: session_genesis.map(str::to_owned),
                };
                let pinned = crate::tched_router_admission::readmit_call(
                    self,
                    &admitted_schema,
                    plugin,
                    method,
                    &caller,
                )
                .await?;
                let outcome = crate::antigravity_dispatch::dispatch(
                    plugin,
                    method,
                    &serde_json::to_value(&parsed_value)?,
                    self.antigravity_documents
                        .get_or_try_init(|| {
                            op_plugins::state_plugins::runtime_documents::runtime_documents()
                        })
                        .await?
                        .clone(),
                    self.antigravity_credentials
                        .get_or_init(|| async { op_llm::antigravity::google_credential_owner() })
                        .await
                        .clone(),
                )
                .await?;
                self.process_observed_state(plugin, simd_json::serde::to_owned_value(outcome.state)?, pinned.schema_hash)
                    .await.map_err(anyhow::Error::msg)
                    .context("Antigravity native result completed but SHM publication failed; reconcile before retry")?;
                outcome.output
            }
            "proxy_server" => {
                let _operation = self.proxy_projection.lock().await;
                let caller = crate::tched_router_admission::RouterCaller {
                    actor: actor_id.into(),
                    capability: capability_id.map(str::to_owned),
                    session: session_id.map(str::to_owned),
                    genesis: session_genesis.map(str::to_owned),
                };
                let pinned = crate::tched_router_admission::readmit_call(
                    self,
                    &admitted_schema,
                    "proxy_server",
                    method,
                    &caller,
                )
                .await?;
                let service = self.proxy_methods.get_or_try_init(|| async {
                    let service = Arc::new(crate::proxy_methods::ProxyMethods::new(
                        op_plugins::state_plugins::runtime_documents::runtime_documents().await?,
                    ));
                    let observed = Arc::downgrade(&service);
                    let changed = service.changed.clone();
                    let engine = self.clone();
                    tokio::spawn(async move {
                        loop {
                            changed.notified().await;
                            let Some(service) = observed.upgrade() else { break };
                            let _operation = engine.proxy_projection.lock().await;
                            let result = async {
                                let pinned = crate::sealed_schema_reader::read_manifest_pinned_schema_document(
                                    &crate::sealed_schema_reader::blob_catalog_dir_from_env(), "proxy_server",
                                )?;
                                crate::method_contract_admission::admit_compiled_state_contract(&pinned, "proxy_server")?;
                                let state = service.snapshot().await?;
                                engine.process_observed_state("proxy_server", simd_json::serde::to_owned_value(state)?, pinned.schema_hash)
                                    .await.map_err(anyhow::Error::msg)?;
                                Ok::<_, anyhow::Error>(())
                            }.await;
                            if result.is_err() { tracing::error!("proxy terminal-state SHM publication failed"); }
                        }
                    });
                    Ok::<_, anyhow::Error>(service)
                }).await?;
                let (output, state) = service
                    .dispatch(method, serde_json::to_value(&parsed_value)?)
                    .await?;
                self.process_observed_state(
                    "proxy_server",
                    simd_json::serde::to_owned_value(state)?,
                    pinned.schema_hash,
                )
                .await
                .map_err(anyhow::Error::msg)
                .context(
                    "proxy operation completed but SHM publication failed; reconcile before retry",
                )?;
                output
            }
            "service" => {
                let args = serde_json::to_value(&parsed_value)?;
                op_plugins::state_plugins::service::dispatch_service_method(method, &args).await?
            }
            "users" => {
                let args = serde_json::to_value(&parsed_value)?;
                op_plugins::state_plugins::users::dispatch_users_method(method, &args).await?
            }
            "oci" => {
                let args = serde_json::to_value(&parsed_value)?;
                op_plugins::state_plugins::oci::dispatch_oci_method(method, &args).await?
            }
            "fail2ban" => {
                let socket = std::env::var_os("OP_FAIL2BAN_SOCKET")
                    .map(std::path::PathBuf::from)
                    .unwrap_or_else(|| "/run/fail2ban/fail2ban.sock".into());
                let service = op_plugins::state_plugins::fail2ban_methods::Fail2banMethods::new(
                    socket,
                    std::time::Duration::from_secs(15),
                )?;
                let args = serde_json::to_value(&parsed_value)?;
                service.dispatch(method, &args).await?
            }
            "btrfs" => {
                let args = serde_json::to_value(&parsed_value)?;
                op_plugins::state_plugins::btrfs_plugin::dispatch_btrfs_method(method, &args)
                    .await?
            }
            "unix_socket" => {
                let _guard = if matches!(method, "bind" | "listen" | "close") {
                    Some(self.shared_listener_projection.lock().await)
                } else {
                    None
                };
                let listener = self.shared_listener.get().context(
                    "the serving bridge has not installed its shared listener controller",
                )?;
                let args = serde_json::to_value(&parsed_value)?;
                let result = listener.dispatch(method, &args).await?;
                if matches!(method, "bind" | "listen" | "close") {
                    self.publish_plugin_projection_state(
                        "unix_socket", simd_json::serde::to_owned_value(listener.snapshot()?)?,
                        ChangeType::PropertySet, true,
                    ).await.context("listener operation completed but projection publication failed; query the listener before retrying")?;
                }
                result
            }
            "shared_unix_socket" => {
                use op_plugins::state_plugins::shared_unix_socket::{
                    ListRegistrationsOutput, SharedUnixSocketState,
                };
                use op_plugins::state_plugins::shared_unix_socket_methods::SharedUnixSocketMethods;
                static PROJECTION_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
                let _guard = PROJECTION_LOCK.lock().await;
                let path = self
                    .shared_socket_path
                    .get()
                    .context("shared socket owner has not configured its path")?;
                let service = SharedUnixSocketMethods::new(
                    op_plugins::state_plugins::runtime_documents::runtime_documents().await?,
                    path,
                )?;
                let args = serde_json::to_value(&parsed_value)?;
                let result = service.dispatch(method, &args).await?;
                if method == "create_unix_socket" {
                    let listed: ListRegistrationsOutput = serde_json::from_value(
                        service
                            .dispatch("list_registrations", &serde_json::json!({}))
                            .await?,
                    )?;
                    let state = SharedUnixSocketState {
                        registrations: listed.registrations,
                    };
                    self.publish_plugin_projection_state("shared_unix_socket", simd_json::serde::to_owned_value(state)?, ChangeType::PropertySet, true).await
                        .context("registration committed but projection publication failed; retry to reconcile")?;
                }
                result
            }
            "packagekit" => {
                let args = serde_json::to_value(&parsed_value)?;
                op_plugins::state_plugins::packagekit::dispatch_packagekit_method(method, &args)
                    .await?
            }
            "freedesktop" => {
                use crate::freedesktop_dispatch::{self, DispatchOutcome};
                let args = serde_json::to_value(&parsed_value)?;
                match freedesktop_dispatch::prepare(self, method, &args).await? {
                    DispatchOutcome::Reply(reply) => reply,
                    DispatchOutcome::Write(plan) => {
                        crate::project_dispatch::verified_session(
                            self,
                            actor_id,
                            session_id,
                            session_genesis,
                        )
                        .await?;
                        freedesktop_dispatch::revalidate_write(&plan)?;
                        let grants = crate::interceptor::load_capability_grants(actor_id);
                        freedesktop_dispatch::authorize_write(&plan, actor_id, &grants)?;
                        let target_args = serde_json::to_string(&plan.args)?;
                        let result = Box::pin(self.dispatch_method_call_with_identity(
                            &plan.target_plugin,
                            &plan.method,
                            &target_args,
                            Some(&plan.required_capability),
                            actor_id,
                            session_id,
                            session_genesis,
                        ))
                        .await?;
                        freedesktop_dispatch::finish_write(&plan, result)?
                    }
                }
            }
            "keyring" => {
                let args = serde_json::to_value(&parsed_value)?;
                op_plugins::state_plugins::keyring::dispatch_keyring_method(method, &args).await?
            }
            "keypair" => {
                let args = serde_json::to_value(&parsed_value)?;
                op_plugins::state_plugins::keypair::dispatch_keypair_method(method, &args).await?
            }
            "login1" => {
                let args = serde_json::to_value(&parsed_value)?;
                op_plugins::state_plugins::login1::dispatch_login1_method(method, &args).await?
            }
            "dnsresolver" => {
                let args = serde_json::to_value(&parsed_value)?;
                op_plugins::state_plugins::dnsresolver::dispatch_dnsresolver_method(method, &args)
                    .await?
            }
            "openflow_obfuscation" => {
                let _operation = self.obfuscation_projection.lock().await;
                let caller = crate::tched_router_admission::RouterCaller {
                    actor: actor_id.into(),
                    capability: capability_id.map(str::to_owned),
                    session: session_id.map(str::to_owned),
                    genesis: session_genesis.map(str::to_owned),
                };
                let pinned = crate::tched_router_admission::readmit_call(
                    self,
                    &admitted_schema,
                    "openflow_obfuscation",
                    method,
                    &caller,
                )
                .await?;
                let service = self
                    .obfuscation_methods
                    .get_or_try_init(|| async {
                        Ok::<_, anyhow::Error>(Arc::new(
                            crate::obfuscation_methods::ObfuscationMethods::new(
                                op_plugins::state_plugins::runtime_documents::runtime_documents()
                                    .await?,
                                Arc::new(crate::obfuscation_methods::NativeFlowOwner),
                            ),
                        ))
                    })
                    .await?;
                let (output, state) = service
                    .dispatch(method, serde_json::to_value(&parsed_value)?)
                    .await?;
                self.process_observed_state(
                    "openflow_obfuscation",
                    simd_json::serde::to_owned_value(state)?,
                    pinned.schema_hash,
                )
                .await
                .map_err(anyhow::Error::msg)
                .context(
                    "flow operation completed but SHM publication failed; reconcile before retry",
                )?;
                output
            }
            "openflow" => {
                let args = serde_json::to_value(&parsed_value)?;
                if !matches!(method, "add_flow" | "delete_flow" | "modify_flow") {
                    let _operation = self.openflow_projection.lock().await;
                    let caller = crate::tched_router_admission::RouterCaller {
                        actor: actor_id.into(),
                        capability: capability_id.map(str::to_owned),
                        session: session_id.map(str::to_owned),
                        genesis: session_genesis.map(str::to_owned),
                    };
                    let _pinned = crate::tched_router_admission::readmit_call(
                        self,
                        &admitted_schema,
                        "openflow",
                        method,
                        &caller,
                    )
                    .await?;
                    op_plugins::state_plugins::openflow::dispatch_openflow_method(method, &args)
                        .await?
                } else {
                    let _operation = self.openflow_projection.lock().await;
                    let existing = tokio::task::spawn_blocking(|| {
                        op_core::projection_shm::read_projection_bytes("openflow")
                    })
                    .await?;
                    if let Some(mut existing) = existing {
                        let existing = simd_json::to_owned_value(&mut existing)?;
                        admitted_schema
                            .validate_exact_schema_fields(&existing)
                            .context("OpenFlow projection does not satisfy the admitted schema")?;
                    }
                    let caller = crate::tched_router_admission::RouterCaller {
                        actor: actor_id.into(),
                        capability: capability_id.map(str::to_owned),
                        session: session_id.map(str::to_owned),
                        genesis: session_genesis.map(str::to_owned),
                    };
                    let pinned = crate::tched_router_admission::readmit_call(
                        self,
                        &admitted_schema,
                        "openflow",
                        method,
                        &caller,
                    )
                    .await?;
                    let receipt = if let Some(connection) = self.openflow_method_connection.get() {
                        crate::openflow_methods::dispatch_openflow_flow_method_on(
                            connection, method, &args,
                        )
                        .await?
                    } else {
                        crate::openflow_methods::dispatch_openflow_flow_method(method, &args)
                            .await?
                    };
                    let projection = crate::openflow_methods::projection_change(&receipt)?;
                    self.commit_state_update(
                        StateChange {
                            change_id: uuid::Uuid::new_v4().to_string(),
                            event_id: 0,
                            plugin_id: "openflow".into(),
                            object_path: projection.object_path,
                            change_type: projection.change_type,
                            member_name: None,
                            old_value: projection
                                .before
                                .map(simd_json::serde::to_owned_value)
                                .transpose()?,
                            new_value: simd_json::serde::to_owned_value(projection.value)?,
                            tags_touched: Vec::new(),
                            event_hash: String::new(),
                            timestamp: chrono::Utc::now(),
                            actor_id: actor_id.into(),
                            source: ChangeSource::Internal,
                        },
                        Some(pinned.schema_hash),
                        false,
                    )
                    .await
                    .context(
                        "OpenFlow native operation completed but SHM publication failed; reconcile the exact member before retry",
                    )?;
                    receipt.output
                }
            }
            "incus" => {
                let args = serde_json::to_value(&parsed_value)?;
                crate::incus_observer::dispatch_method(self, method, &args).await?
            }
            "privacy_routes" => {
                // Keep effect and publication in the same order. Otherwise a
                // slower earlier delete could republish a removed sibling.
                static CALLS: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
                let _owner = CALLS.lock().await;
                let caller = crate::tched_router_admission::RouterCaller {
                    actor: actor_id.into(),
                    capability: capability_id.map(str::to_owned),
                    session: session_id.map(str::to_owned),
                    genesis: session_genesis.map(str::to_owned),
                };
                let pinned = crate::tched_router_admission::readmit_call(
                    self,
                    &admitted_schema,
                    "privacy_routes",
                    method,
                    &caller,
                )
                .await?;
                let args = serde_json::to_value(&parsed_value)?;
                let result =
                    op_plugins::state_plugins::privacy_routes::dispatch_privacy_routes_method(
                        method, &args,
                    )
                    .await?;
                let state = simd_json::serde::to_owned_value(result.clone())?;
                self.process_observed_state("privacy_routes", state, pinned.schema_hash)
                    .await
                    .map_err(anyhow::Error::msg)
                    .context(
                        "privacy_routes operation completed but authoritative SHM publication failed; reconcile before retry",
                    )?;
                result
            }
            "web_ui" => {
                let args = serde_json::to_value(&parsed_value)?;
                crate::web_ui_dispatch::dispatch(
                    self,
                    method,
                    &args,
                    actor_id,
                    session_id,
                    session_genesis,
                    &admitted_schema,
                )
                .await?
            }
            "large_language_model" => {
                let args = serde_json::to_value(&parsed_value)?;
                crate::large_language_model_dispatch::dispatch(
                    self,
                    method,
                    &args,
                    actor_id,
                    session_id,
                    session_genesis,
                    Some(&admitted_schema),
                )
                .await?
            }
            "tched_router" if crate::generation_jobs_dispatch::is_generation_method(method) => {
                self.generation_jobs()?
                    .dispatch(
                        self,
                        &admitted_schema,
                        method,
                        &serde_json::to_value(&parsed_value)?,
                        actor_id,
                        session_id,
                        session_genesis,
                        generation_origin,
                    )
                    .await?
            }
            "tched_router" => {
                let engine = self.clone();
                let method = method.to_string();
                let json_args = json_args.to_string();
                let args = serde_json::to_value(&parsed_value)?;
                let admitted_schema = admitted_schema.clone();
                let change = change.clone();
                let caller = crate::tched_router_admission::RouterCaller {
                    actor: actor_id.to_owned(),
                    capability: capability_id.map(str::to_owned),
                    session: session_id.map(str::to_owned),
                    genesis: session_genesis.map(str::to_owned),
                };
                let owned_bound_oracle = bound_oracle.clone();
                tokio::spawn(async move {
                    engine
                        .dispatch_tched_router_owned(
                            &method,
                            &json_args,
                            args,
                            admitted_schema,
                            change,
                            caller,
                            facade_origin,
                            owned_bound_oracle,
                        )
                        .await
                })
                .await
                .map_err(|error| anyhow::anyhow!("tched_router owned task failed: {error}"))??
            }
            "ghostbridge" => {
                let state = self
                    .projected_state::<op_plugins::state_plugins::ghostbridge::GhostbridgeState>(
                        "ghostbridge",
                    )
                    .await?
                    .unwrap_or_else(
                        op_plugins::state_plugins::ghostbridge::GhostbridgePlugin::current_state,
                    );
                op_plugins::state_plugins::ghostbridge::dispatch_ghostbridge_method(method, &state)?
            }
            "cognitive_mcp" => {
                let args = serde_json::to_value(&parsed_value)?;
                let result = self
                    .dispatch_cognitive_mcp_method(
                        crate::specialist::CognitiveCaller {
                            actor_id,
                            session_id,
                            session_genesis,
                        },
                        method,
                        &args,
                        &admitted_schema,
                    )
                    .await?;
                let declaration = admitted_schema
                    .methods
                    .get(method)
                    .context("admitted cognitive method is absent")?;
                crate::method_contract_admission::validate_admitted_output(
                    declaration,
                    &result,
                )?;
                result
            }
            "oscal_subid_registry" => {
                crate::oscal_dispatch::dispatch(
                    method,
                    &serde_json::to_value(&parsed_value)?,
                    actor_id,
                    capability_id,
                )
                .await?
            }
            "datastore" => {
                crate::datastore_methods::DatastoreMethods::production()
                    .dispatch(method, &serde_json::to_value(&parsed_value)?)
                    .await?
            }
            "notebooklm" => {
                let verified_session = crate::project_dispatch::verified_session(
                    self,
                    actor_id,
                    session_id,
                    session_genesis,
                )
                .await?;
                self.dispatch_notebooklm_method(method, &parsed_value, actor_id, &verified_session)
                    .await?
            }
            "sess_decl" => {
                let verified = crate::project_dispatch::verified_session(
                    self,
                    actor_id,
                    session_id,
                    session_genesis,
                )
                .await?;
                let service = self
                    .sess_decl_methods
                    .get_or_try_init(|| async {
                        let documents =
                            op_plugins::state_plugins::runtime_documents::runtime_documents()
                                .await?;
                        Ok::<_, anyhow::Error>(
                            op_plugins::state_plugins::sessdecl::SessDeclMethods::new(documents),
                        )
                    })
                    .await?;
                let context = op_plugins::state_plugins::sessdecl::SessDeclAccessContext {
                    principal_id: actor_id.to_owned(),
                    session_id: verified,
                };
                service
                    .dispatch(&context, method, &serde_json::to_value(&parsed_value)?)
                    .await?
            }
            // The same initialized chain owns both audit persistence and the
            // Snowball data snapshots. Never open an empty replacement store.
            "snowball" => match method {
                "query_events" => {
                    dispatch_snowball_query_events(&self.event_chain, &parsed_value).await?
                }
                "verify_chain" => {
                    dispatch_snowball_verify_chain(&self.event_chain, &parsed_value).await?
                }
                "get_projector_stats" => {
                    let stats = match self.vector_projector.get() {
                        Some(projector) => projector.stats().await,
                        None => crate::chain_vector_projector::ProjectorStats::default(),
                    };
                    serde_json::to_value(&stats)?
                }
                "create_snapshot" | "get_current_state" | "get_snapshot" | "get_stats"
                | "list_snapshots" | "rollback" | "set_retention" => {
                    let sink = self
                        .audit_sink
                        .get()
                        .context("durable Snowball backend is not initialized")?;
                    let methods = self.snowball_methods.get_or_init(|| async {
                        op_plugins::state_plugins::snowball_methods::SnowballMethods::from_shared(sink.streaming_snowball())
                    }).await;
                    let dispatch_result = methods
                        .dispatch(method, &serde_json::to_value(&parsed_value)?)
                        .await?;
                    // This plugin never gets a mutate()-path special case (it's
                    // dispatched here, through the typed interface, which does
                    // not otherwise commit to the SHM projection), so it would
                    // never appear as observed-runtime state without this.
                    match op_plugins::state_plugins::snowball_plugin::current_state().await {
                        Ok(state) => {
                            if let Err(error) = self
                                .publish_plugin_projection_state(
                                    "snowball",
                                    simd_json::serde::to_owned_value(state)?,
                                    ChangeType::PropertySet,
                                    true,
                                )
                                .await
                            {
                                tracing::warn!(%error, "snowball state read but projection publish failed");
                            }
                        }
                        Err(error) => {
                            tracing::warn!(%error, "snowball method completed but current state could not be read for projection")
                        }
                    }
                    dispatch_result
                }
                _ => anyhow::bail!(
                    "unsupported plugin method '{}.{}': no canonical backend dispatch",
                    plugin_id,
                    method
                ),
            },
            _ => anyhow::bail!(
                "unsupported plugin method '{}.{}': no canonical backend dispatch",
                plugin_id,
                method
            ),
        };
        if plugin_id == "identity_sled" {
            // The metadata was admitted before dispatch; recheck the durable
            // session/principal binding before releasing possession material.
            let own_session = if method == "get_identity" && session_id.is_some() {
                Some(
                    crate::project_dispatch::verified_session(
                        self,
                        actor_id,
                        session_id,
                        session_genesis,
                    )
                    .await?,
                )
            } else {
                None
            };
            if method == "get_identity"
                && let Some(session) = own_session.as_deref()
            {
                crate::identity_sled_dispatch::activate_human_session_on_login(self, session)
                    .await?;
            }
            redact_identity_method_credentials(method, own_session.as_deref(), &mut method_result);
        }
        if let Some(proof) = &bound_oracle {
            crate::interceptor::revalidate_bound_oracle(
                self,
                proof,
                actor_id,
                session_id,
                session_genesis,
                false,
            )
            .await?;
            require_bound_oracle_grant(actor_id, capability_id)?;
            // The browser transport does not export the target's authored SID1,
            // even if a future explicit grant permits an administrative read.
            redact_identity_credentials_json(&mut method_result);
        }

        // Return a JSON result carrying the event accountability proof and the
        // method's domain result. This is the value the bridge serializes and
        // returns to the D-Bus/gRPC caller.
        let result = serde_json::json!({
            "success": true,
            "event_id": change.event_id,
            "event_hash": change.event_hash,
            "plugin_id": plugin_id,
            "method": method,
            "result": method_result,
        });
        Ok(result)
    }

    #[allow(clippy::too_many_arguments)]
    async fn dispatch_tched_router_owned(
        &self,
        method: &str,
        json_args: &str,
        parsed_value: serde_json::Value,
        admitted_schema: op_state_store::PluginSchema,
        change: StateChange,
        caller: crate::tched_router_admission::RouterCaller,
        facade_origin: Option<crate::tched_router_admission::FacadeOrigin>,
        bound_oracle: Option<crate::oracle_assertion::BoundOracleProof>,
    ) -> anyhow::Result<serde_json::Value> {
        let guarded =
            crate::tched_router_admission::admit_guarded(self, &admitted_schema, method, &caller)
                .await?;
        if let Some(proof) = &bound_oracle {
            crate::interceptor::revalidate_bound_oracle(
                self,
                proof,
                &caller.actor,
                caller.session.as_deref(),
                caller.genesis.as_deref(),
                true,
            )
            .await?;
        }
        if let Some(origin) = &facade_origin {
            // The native operation guard is retained while rechecking the
            // original facade seal and current caller grant after any queue wait.
            origin.readmit(self, &caller).await?;
        }
        let pinned = &guarded.pinned;
        let sealed_identity = crate::native_sled_identity::credential_for(
            &caller.actor,
            caller
                .session
                .as_deref()
                .context("verified session is required")?,
            caller
                .genesis
                .as_deref()
                .context("verified genesis is required")?,
        )?;
        if method == "get_own_agent_route" {
            let _: op_plugins::state_plugins::tched_router::GetOwnAgentRouteInput =
                serde_json::from_value(parsed_value)?;
            let session = caller
                .session
                .as_deref()
                .context("verified session is required")?;
            let grants = crate::interceptor::load_capability_grants(&caller.actor)
                .into_iter()
                .collect();
            let before =
                crate::project_dispatch::current_chat_context(self, session, &grants).await?;
            anyhow::ensure!(
                crate::chat_mode_routing::dedicated_alias(before.mode).is_some(),
                "current chat mode has no dedicated native route"
            );
            let (_, represented) =
                crate::tched_config_engine::coherent_observation(pinned, &sealed_identity).await?;
            self.persist_tched_router_config_observation(method, &represented, &pinned.schema_hash)
                .await?;
            let committed = self
                .read_tched_router_config_field("native_config", &pinned.schema_hash)
                .await?;
            let output = serde_json::to_value(crate::tched_own_agent_route::project(
                before.mode,
                &committed,
            )?)?;
            crate::tched_router_admission::readmit_call(
                self,
                &admitted_schema,
                "tched_router",
                method,
                &caller,
            )
            .await?;
            let grants = crate::interceptor::load_capability_grants(&caller.actor)
                .into_iter()
                .collect();
            let after =
                crate::project_dispatch::current_chat_context(self, session, &grants).await?;
            ensure_own_route_context_unchanged(&before, &after)?;
            if let Some(proof) = &bound_oracle {
                crate::interceptor::revalidate_bound_oracle(
                    self,
                    proof,
                    &caller.actor,
                    caller.session.as_deref(),
                    caller.genesis.as_deref(),
                    false,
                )
                .await?;
            }
            crate::method_contract_admission::validate_admitted_output(
                admitted_schema
                    .methods
                    .get(method)
                    .context("admitted own route method absent")?,
                &output,
            )?;
            return Ok(output);
        }
        if matches!(
            method,
            "resolve_catalog_model_binding" | "apply_catalog_model_binding"
        ) {
            let applying = method == "apply_catalog_model_binding";
            let input: op_plugins::state_plugins::tched_router::ResolveCatalogModelBindingInput =
                serde_json::from_value(parsed_value)?;
            let context = if applying {
                let grants = crate::interceptor::load_capability_grants(&caller.actor)
                    .into_iter()
                    .collect();
                let context = crate::project_dispatch::current_chat_context(
                    self,
                    caller
                        .session
                        .as_deref()
                        .context("verified session is required")?,
                    &grants,
                )
                .await?;
                anyhow::ensure!(
                    crate::chat_mode_routing::dedicated_alias(context.mode).is_none(),
                    "dedicated agent model changes require their protected operation"
                );
                Some(context)
            } else {
                None
            };
            let target = crate::model_registry_dispatch::catalog_binding_target(
                self,
                &input.provider_id,
                &input.model_id,
            )
            .await?;
            // The catalog lookup may have waited for Cozo. Readmit this exact
            // reflected read before inspecting the native router configuration.
            crate::tched_router_admission::readmit_call(
                self,
                &admitted_schema,
                "tched_router",
                method,
                &caller,
            )
            .await?;
            if let Some(proof) = &bound_oracle {
                crate::interceptor::revalidate_bound_oracle(
                    self,
                    proof,
                    &caller.actor,
                    caller.session.as_deref(),
                    caller.genesis.as_deref(),
                    true,
                )
                .await?;
            }
            if applying && target.ordinary_reference.is_some() {
                let env = crate::model_registry_dispatch::Environment::from_env();
                let file = crate::model_registry_dispatch::load_providers(&env)?;
                if let Some(provider) = file
                    .providers
                    .iter()
                    .find(|provider| provider.id == input.provider_id)
                {
                    let secrets = crate::tched_config_engine::pinned_config_secrets(pinned)?;
                    let runtime =
                        crate::tched_router_runtime::TchedRouterRuntimeClient::for_agent_alias(
                            "dashboard",
                        )
                        .with_sealed_identity(Some(&sealed_identity));
                    crate::catalog_ordinary_profile::ensure_ordinary_profile(
                        &env,
                        &runtime,
                        provider,
                        &target,
                        &input.model_id,
                        &secrets,
                    )
                    .await?;
                }
            }
            let mut observation = crate::tched_config_engine::observe_catalog_binding(
                pinned,
                &sealed_identity,
                &target,
            )
            .await?;
            let after = crate::model_registry_dispatch::catalog_binding_target(
                self,
                &input.provider_id,
                &input.model_id,
            )
            .await?;
            anyhow::ensure!(
                target == after,
                "catalog authority changed during native binding reflection"
            );
            let mut represented = serde_json::json!({});
            if applying {
                // This method owns only a catalog-verified ordinary selection.
                // It never forwards to the broad SetSelection/config facade.
                crate::tched_router_admission::readmit_call(
                    self,
                    &admitted_schema,
                    "tched_router",
                    method,
                    &caller,
                )
                .await?;
                let grants = crate::interceptor::load_capability_grants(&caller.actor)
                    .into_iter()
                    .collect();
                let current = crate::project_dispatch::current_chat_context(
                    self,
                    caller
                        .session
                        .as_deref()
                        .context("verified session is required")?,
                    &grants,
                )
                .await?;
                ensure_own_route_context_unchanged(
                    context.as_ref().context("selection context absent")?,
                    &current,
                )?;
                if let Some(proof) = &bound_oracle {
                    crate::interceptor::revalidate_bound_oracle(
                        self,
                        proof,
                        &caller.actor,
                        caller.session.as_deref(),
                        caller.genesis.as_deref(),
                        true,
                    )
                    .await?;
                }
                let expected = observation.binding.clone();
                let runtime =
                    crate::tched_router_runtime::TchedRouterRuntimeClient::for_agent_alias(
                        "dashboard",
                    )
                    .with_sealed_identity(Some(&sealed_identity));
                runtime
                    .set_selection(&expected.reference, &expected.model)
                    .await?;
                let fields = crate::tched_config_engine::coherent_catalog_selection_observation(pinned, &sealed_identity)
                    .await.context("native catalog selection may have completed but coherent readback failed; reconcile before retry")?;
                observation = crate::tched_config_engine::observe_catalog_binding(pinned, &sealed_identity, &target)
                    .await.context("native catalog selection may have completed but binding readback failed; reconcile before retry")?;
                anyhow::ensure!(
                    observation.binding == expected
                        && fields["native_config"] == observation.native_config,
                    "native catalog selection may have completed but route readback changed; reconcile before retry"
                );
                let (family, alias) = expected
                    .reference
                    .split_once('.')
                    .context("resolved native reference is invalid")?;
                anyhow::ensure!(
                    fields["native_config"]["agents"]["dashboard"]["model_provider"].as_str()
                        == Some(expected.reference.as_str())
                        && fields["native_config"]["providers"]["models"][family][alias]["model"]
                            .as_str()
                            == Some(expected.model.as_str()),
                    "native catalog selection may have completed but exact selection readback differs; reconcile before retry"
                );
                represented = fields;
            }
            let binding = serde_json::to_value(observation.binding)?;
            represented["native_config"] = observation.native_config;
            represented["catalog_model_binding"] = binding.clone();
            self.persist_tched_router_config_observation(method, &represented, &pinned.schema_hash)
                .await?;
            let committed_config = self
                .read_tched_router_config_field("native_config", &pinned.schema_hash)
                .await?;
            let committed = self
                .read_tched_router_config_field("catalog_model_binding", &pinned.schema_hash)
                .await?;
            anyhow::ensure!(
                committed_config == represented["native_config"] && committed == binding,
                "native binding observation changed after publication"
            );
            let after = crate::model_registry_dispatch::catalog_binding_target(
                self,
                &input.provider_id,
                &input.model_id,
            )
            .await?;
            anyhow::ensure!(
                target == after,
                "catalog authority changed after binding publication"
            );
            crate::tched_router_admission::readmit_call(
                self,
                &admitted_schema,
                "tched_router",
                method,
                &caller,
            )
            .await?;
            if let Some(before) = context {
                let grants = crate::interceptor::load_capability_grants(&caller.actor)
                    .into_iter()
                    .collect();
                let after = crate::project_dispatch::current_chat_context(
                    self,
                    caller
                        .session
                        .as_deref()
                        .context("verified session is required")?,
                    &grants,
                )
                .await?;
                ensure_own_route_context_unchanged(&before, &after)
                    .context("native catalog selection may have completed but caller context changed; reconcile before retry")?;
            }
            if let Some(proof) = &bound_oracle {
                crate::interceptor::revalidate_bound_oracle(
                    self,
                    proof,
                    &caller.actor,
                    caller.session.as_deref(),
                    caller.genesis.as_deref(),
                    false,
                )
                .await?;
            }
            crate::method_contract_admission::validate_admitted_output(
                admitted_schema
                    .methods
                    .get(method)
                    .context("admitted binding reflection method absent")?,
                &committed,
            )?;
            return Ok(committed);
        }
        if let Some(result) = crate::tched_config_engine::dispatch(
            self,
            pinned,
            method,
            &parsed_value,
            &sealed_identity,
        )
        .await?
        {
            return Ok(result);
        }
        if matches!(method, "GetState" | "ListProviders" | "ListModels") {
            let observed = crate::large_language_model_dispatch::observed_tched_router_state(
                self,
                pinned,
                &sealed_identity,
            )
            .await?;
            return op_plugins::state_plugins::tched_router::dispatch_tched_router_method(
                method, json_args, &observed,
            )
            .map(|outcome| outcome.result)
            .map_err(|error| {
                anyhow::anyhow!(
                    "tched_router observed read dispatch error for '{}': {}",
                    method,
                    error
                )
            });
        }

        if method == "Chat" {
            let args =
                serde_json::from_value::<op_plugins::state_plugins::tched_router::ChatInput>(
                    parsed_value,
                )
                .context("invalid tched_router.Chat arguments")?;
            if let Some(id) = &args.conversation_id {
                anyhow::ensure!(
                    !id.is_empty() && id.len() <= 128 && !id.chars().any(char::is_control),
                    "invalid chat conversation_id"
                );
            }
            let scope = op_cozo_store::EpisodeScope {
                principal_id: caller.actor.clone(),
                session_id: caller
                    .session
                    .clone()
                    .context("chat episode requires verified session")?,
                session_genesis: caller
                    .genesis
                    .clone()
                    .context("chat episode requires verified genesis")?,
            };
            let route = crate::chat_mode_routing::resolve(
                self,
                crate::specialist::CognitiveCaller {
                    actor_id: &caller.actor,
                    session_id: caller.session.as_deref(),
                    session_genesis: caller.genesis.as_deref(),
                },
                args,
            )
            .await?;
            let has_memory = route.mode.has_memory();
            let args = route.input;
            let episode_effect = if has_memory {
                self.ctl_plane_chatbot_episode_cozo().await?;
                Some(
                    self.chatbot_episode_publisher()?
                        .admit_chat_effect(self, scope.clone())
                        .await?,
                )
            } else {
                None
            };
            let conversation_id = args.conversation_id.clone();
            let goal_text = if args.messages.is_empty() {
                Some(args.message.clone())
            } else {
                args.messages
                    .iter()
                    .rev()
                    .find(|message| message.role == "user")
                    .map(|message| message.content.clone())
            };
            let started_at = chrono::Utc::now();
            let started_clock = std::time::Instant::now();
            self.attach_chat_conversation_turn(
                &scope,
                conversation_id.as_deref(),
                goal_text.as_deref().unwrap_or(""),
                crate::chat_response_dispatch::RequestEvent {
                    event_id: change.event_id,
                    event_hash: change.event_hash.clone(),
                },
            )
            .await
            .context("human message was not recorded on the conversation")?;
            crate::tched_router_admission::readmit_call(
                self,
                &admitted_schema,
                "tched_router",
                method,
                &caller,
            )
            .await?;
            if let Some(origin) = &facade_origin {
                origin.readmit(self, &caller).await?;
            }
            // The control-plane chatbot consumes what the session already
            // knows: recent episodes, namespace memory, semantic recall,
            // activity signals and pending knowledge pushes, scoped to this
            // principal/session and bounded. Injected as the first system
            // message; the episode records what was drawn on.
            let turn_query = goal_text.clone().unwrap_or_default();
            let bundle = crate::context_bundle::assemble_for_context(
                self,
                &scope,
                &turn_query,
                &route.context,
            )
            .await;
            let mut args = args;
            if let Some(recalled) = bundle.render(crate::context_bundle::TURN_BUDGET_CHARS) {
                if args.messages.is_empty() && !args.message.trim().is_empty() {
                    let message = std::mem::take(&mut args.message);
                    args.messages
                        .push(op_plugins::state_plugins::tched_router::TchedChatMessage {
                            role: "user".into(),
                            content: message,
                        });
                }
                args.messages.insert(
                    0,
                    op_plugins::state_plugins::tched_router::TchedChatMessage {
                        role: "system".into(),
                        content: recalled,
                    },
                );
            }
            let provenance = bundle.provenance();
            // The turn itself is session activity for the context engine.
            if has_memory && let Ok(runtime) = self.cognitive_mcp().await {
                runtime
                    .context_engine()
                    .record_activity(
                        &scope.session_id,
                        op_cognitive_mcp::context_awareness::ActivityType::Query,
                        turn_query.clone(),
                        serde_json::json!({ "source": "tched_router.chat", "context": provenance }),
                    )
                    .await;
            }
            // Selected model on tched-router (:8084). Tools are compact MCP on
            // that agent; the bridge does not own a second provider state.
            let output = if route.mode == op_plugins::state_plugins::project::ChatMode::ControlPlane
            {
                let before_prompt = crate::specialist::generation_admission_with_origin(
                    self,
                    &admitted_schema,
                    crate::specialist::CognitiveCaller {
                        actor_id: &caller.actor,
                        session_id: caller.session.as_deref(),
                        session_genesis: caller.genesis.as_deref(),
                    },
                    "tched_router",
                    method,
                    facade_origin.clone(),
                )?;
                crate::control_plane_routing::complete(args, before_prompt, false).await?
            } else {
                let before_prompt = crate::specialist::generation_admission_with_origin(
                    self,
                    &admitted_schema,
                    crate::specialist::CognitiveCaller {
                        actor_id: &caller.actor,
                        session_id: caller.session.as_deref(),
                        session_genesis: caller.genesis.as_deref(),
                    },
                    "tched_router",
                    method,
                    facade_origin.clone(),
                )?;
                route
                    .client
                    .complete_under_guard(args, before_prompt)
                    .await?
            };
            if let Some(episode_effect) = episode_effect {
                let mut provenance = provenance;
                if let Some(routing) = output.usage.get("routing") {
                    provenance.push(format!("tched_router.routing:{routing}"));
                }
                let episode =
                op_plugins::state_plugins::ctl_plane_chatbot_methods::build_completed_chat_episode(
                    op_plugins::state_plugins::ctl_plane_chatbot_methods::CompletedChatEpisode {
                        scope,
                        conversation_id,
                        goal_text,
                        decision_output: output.content.clone(),
                        tools: provenance,
                        started_at,
                        ended_at: completed_effect_time(started_at, started_clock.elapsed()),
                    },
                )
                .context("native chat completed but episode construction failed")?;
                self.commit_chatbot_episode_effect(episode_effect, episode)
                .await
                .context(
                    "native chat completed but episode publication failed; recover before retry",
                )?;
            }
            crate::tched_router_admission::readmit_call(
                self,
                &admitted_schema,
                "tched_router",
                method,
                &caller,
            )
            .await?;
            if let Some(origin) = &facade_origin {
                origin.readmit(self, &caller).await?;
            }
            return serde_json::to_value(output).map_err(Into::into);
        }

        if matches!(
            method,
            "GetModelRoutes"
                | "GetProviderCatalog"
                | "GetTools"
                | "ResolveRoute"
                | "SetOvsRoutingModel"
                | "SetObfuscationModel"
                | "SetVectorizationModel"
                | "SetQdrantRetrievalModel"
                | "SetCozoRetrievalModel"
        ) {
            // One process-wide owner serializes native catalog observation,
            // durable role CAS/readback, and authoritative SHM publication.
            let documents = if method == "GetTools" {
                None
            } else {
                Some(op_plugins::state_plugins::runtime_documents::runtime_documents().await?)
            };
            let receipt = crate::tched_router_legacy_dispatch::dispatch(
                pinned,
                method,
                &parsed_value,
                documents,
                &sealed_identity,
            )
            .await?;
            self.persist_tched_router_fields(
                &receipt.represented_state,
                receipt.schema_hash,
                format!("tched_router_legacy:{method}"),
            )
            .await
            .with_context(|| {
                format!(
                    "tched_router.{method} completed but its authoritative observation could not be published"
                )
            })?;
            return Ok(receipt.output);
        }

        if matches!(method, "SetProvider" | "SetModel" | "SetSelection") {
            let expected_schema_hash = pinned.schema_hash.clone();
            // Reject an unreadable committed projection before any native effect.
            // Absence is valid for a router that has not published state yet.
            let _ = self
                .projected_state::<serde_json::Value>("tched_router")
                .await?;
            let runtime = crate::tched_router_runtime::TchedRouterRuntimeClient::from_env()
                .with_sealed_identity(Some(&sealed_identity));
            let prior = runtime.current_selection().await?;
            let selected = match method {
                "SetProvider" => {
                    let input: op_plugins::state_plugins::tched_router::SetProviderInput =
                        serde_json::from_value(parsed_value)?;
                    runtime.set_provider(&input.provider_id).await?
                }
                "SetModel" => {
                    let input: op_plugins::state_plugins::tched_router::SetModelInput =
                        serde_json::from_value(parsed_value)?;
                    runtime.set_model(&input.model_id).await?
                }
                _ => {
                    let input: op_plugins::state_plugins::tched_router::SetSelectionInput =
                        serde_json::from_value(parsed_value)?;
                    runtime
                        .set_selection(&input.provider_id, &input.model_id)
                        .await?
                }
            };
            let snapshot = serde_json::json!({
                "selected_provider": selected.provider.clone(),
                "selected_model": selected.model.clone(),
            });
            let (readback, represented) = crate::tched_config_engine::coherent_observation(pinned, &sealed_identity)
                .await
                .context(
                    "native tched_router selection completed but coherent runtime readback failed; outcome requires reconciliation before retry",
                )?;
            anyhow::ensure!(
                readback.selected_provider == selected.provider
                    && readback.selected_model == selected.model,
                "native tched_router selection completed but provider response diverges from authoritative readback; outcome requires reconciliation before retry"
            );
            self.persist_tched_router_observation(&represented, &expected_schema_hash)
                .await
                .context(
                    "native tched_router selection completed but its authoritative observation could not be published; outcome requires reconciliation before retry",
                )?;
            let result = match method {
                "SetProvider" => {
                    serde_json::json!({ "selected_provider": selected.provider.clone() })
                }
                "SetModel" => {
                    serde_json::json!({ "selected_model": selected.model.clone() })
                }
                _ => snapshot,
            };
            let (signal_name, signal_payload) = match method {
                "SetProvider" => (
                    "ProviderChanged",
                    serde_json::json!({ "old": prior.provider, "new": selected.provider, "reason": "explicit set" }),
                ),
                "SetModel" => (
                    "ModelChanged",
                    serde_json::json!({ "old": prior.model, "new": selected.model, "reason": "explicit set" }),
                ),
                _ => (
                    "SelectionChanged",
                    serde_json::json!({ "provider": selected.provider, "model": selected.model }),
                ),
            };
            self.broadcast_method_signal(&change, signal_name, &signal_payload);
            return Ok(result);
        }

        let stored = self
            .projected_state::<serde_json::Value>("tched_router")
            .await?
            .context("tched_router legacy state-backed method has no committed complete state")?;
        let object = stored
            .as_object()
            .context("committed tched_router state is not an object")?;
        for required in [
            "status",
            "selected_provider",
            "selected_model",
            "model_assignments",
            "transport",
            "registration_service",
            "user_container",
            "identity_chain",
            "memory_namespaces",
            "privacy_policy",
            "providers",
            "model_routes",
            "router",
            "tools",
            "config_schema",
            "ui_surfaces",
            "structured_output",
            "selector_policy",
        ] {
            anyhow::ensure!(
                object.contains_key(required),
                "tched_router legacy state-backed method requires observed/stored field '{required}'"
            );
        }
        let state = serde_json::from_value::<
            op_plugins::state_plugins::tched_router::TchedRouterState,
        >(stored)
        .context("committed complete tched_router state does not match its typed contract")?;
        match op_plugins::state_plugins::tched_router::dispatch_tched_router_method(
            method, json_args, &state,
        ) {
            Ok(outcome) => {
                if method.starts_with("Set") {
                    anyhow::bail!(
                        "tched_router.{method} has no canonical native/durable mutation adapter"
                    );
                }
                if let Some(signal) = &outcome.signal {
                    self.broadcast_method_signal(&change, &signal.name, &signal.payload);
                }
                Ok(outcome.result)
            }
            Err(error) => Err(anyhow::anyhow!(
                "tched_router dispatch error for '{}': {}",
                method,
                error
            )),
        }
    }

    async fn projected_state<T>(&self, plugin_id: &str) -> anyhow::Result<Option<T>>
    where
        T: serde::de::DeserializeOwned,
    {
        anyhow::ensure!(
            plugin_id != "identity_sled",
            "identity requires its private credential reader"
        );
        let id = plugin_id.to_string();
        let bytes = tokio::task::spawn_blocking(move || -> anyhow::Result<Option<Vec<u8>>> {
            let catalog = crate::sealed_schema_reader::blob_catalog_dir_from_env();
            let before =
                crate::sealed_schema_reader::read_manifest_pinned_schema_document(&catalog, &id)?;
            let mut snapshot =
                op_core::projection_shm::read_projection_snapshot(std::slice::from_ref(&id))
                    .context("committed plugin state is unavailable")?;
            let after =
                crate::sealed_schema_reader::read_manifest_pinned_schema_document(&catalog, &id)?;
            anyhow::ensure!(
                before.schema_hash == after.schema_hash,
                "plugin seal changed during state read"
            );
            Ok(snapshot.states.remove(&id))
        })
        .await??;
        bytes
            .map(|bytes| {
                serde_json::from_slice(&bytes)
                    .context("committed plugin state does not match its runtime type")
            })
            .transpose()
    }

    fn broadcast_method_signal(
        &self,
        method_change: &StateChange,
        signal_name: &str,
        payload: &serde_json::Value,
    ) {
        let mut bytes = serde_json::to_vec(payload).unwrap_or_default();
        let payload =
            simd_json::to_owned_value(&mut bytes).unwrap_or_else(|_| simd_json::json!(null));
        let signal = StateChange {
            change_id: uuid::Uuid::new_v4().to_string(),
            event_id: method_change.event_id,
            plugin_id: method_change.plugin_id.clone(),
            object_path: method_change.object_path.clone(),
            change_type: ChangeType::Signal,
            member_name: Some(signal_name.to_string()),
            old_value: None,
            new_value: payload,
            tags_touched: method_change.tags_touched.clone(),
            event_hash: method_change.event_hash.clone(),
            timestamp: method_change.timestamp,
            actor_id: method_change.actor_id.clone(),
            source: method_change.source,
        };
        let _ = self.change_tx.send(signal);
    }

    async fn dispatch_notebooklm_method(
        &self,
        method: &str,
        parsed_value: &simd_json::OwnedValue,
        principal_id: &str,
        session_id: &str,
    ) -> anyhow::Result<serde_json::Value> {
        let args = serde_json::to_value(parsed_value)?;
        if method == "notebook_query_status" {
            let input: op_plugins::state_plugins::notebooklm::QueryStatusInput =
                serde_json::from_value(args)?;
            return Ok(serde_json::to_value(
                self.notebook_queries
                    .status(principal_id, session_id, &input.query_id)
                    .await?,
            )?);
        }
        let store = self.identity_sled_store()?.cozo().clone();
        let selection_store = store.clone();
        let selector = session_id.to_string();
        let rows = tokio::task::spawn_blocking(move || selection_store.run_query(
            "?[notebook_id] := *notebooklm_selection{session_id, notebook_id}, session_id = $session",
            Some(serde_json::json!({"session":selector})))).await??;
        let selected = rows
            .as_array()
            .and_then(|rows| rows.first())
            .and_then(|row| row["notebook_id"].as_str())
            .map(str::to_string);
        if method == "notebook_query_start" {
            let input: op_plugins::state_plugins::notebooklm::NotebookQueryInput =
                serde_json::from_value(args.clone())?;
            let (notebook_id, timeout) =
                crate::notebooklm_queries::NotebookQueries::prepare(&input, selected.as_deref())?;
            let invocation = crate::nlm_cli::nlm_argv("notebook_query", &args, Some(&notebook_id))?;
            let receipt = self
                .notebook_queries
                .start(principal_id, session_id, notebook_id, timeout, async move {
                    crate::nlm_cli::run_notebooklm_query(&invocation).await
                })
                .await?;
            return Ok(serde_json::to_value(receipt)?);
        }
        match crate::nlm_cli::notebooklm_dispatch(method, &args, selected.as_deref())? {
            crate::nlm_cli::NotebooklmDispatch::LocalSelect { notebook_id } => {
                let selector = session_id.to_string();
                let selected_id = notebook_id.clone();
                tokio::task::spawn_blocking(move || store.run_query(
                    "?[session_id, notebook_id] <- [[$session, $notebook]] :put notebooklm_selection {session_id => notebook_id}",
                    Some(serde_json::json!({"session":selector, "notebook":selected_id})))).await??;
                Ok(serde_json::json!({
                    "selected_notebook_id": notebook_id,
                    "session_id": session_id
                }))
            }
            crate::nlm_cli::NotebooklmDispatch::Cli(invocation) => {
                let mut result = crate::nlm_cli::run_notebooklm_method(method, &invocation).await?;
                if matches!(
                    method,
                    "get_health"
                        | "setup_auth"
                        | "reauth"
                        | "refresh_auth"
                        | "save_auth_tokens"
                        | "server_info"
                ) && let Some(status) = crate::nlm_cli::auth_status_from_value(&result)
                {
                    if let Err(error) = crate::nlm_cli::apply_ready_marker(
                        crate::nlm_cli::NOTEBOOKLM_AUTH_READY,
                        crate::nlm_cli::ready_marker_action(&status),
                    ) {
                        tracing::warn!(
                            %error,
                            "could not update NotebookLM authentication readiness"
                        );
                    }
                    if let Some(object) = result.as_object_mut() {
                        object.insert(
                            "authenticated".into(),
                            serde_json::Value::Bool(status == "configured"),
                        );
                        if status == "stale" {
                            object.insert(
                                "hint".into(),
                                serde_json::Value::String(
                                    "auth_status is stale; run setup_auth".into(),
                                ),
                            );
                        }
                    }
                }
                Ok(result)
            }
        }
    }

    /// Merge the exact durable singleton config fields into the chatbot root.
    /// No episode/runtime child can be overwritten through this narrow seam.
    pub(crate) async fn persist_ctl_plane_chatbot_config_fields(
        &self,
        fields: serde_json::Value,
        expected_schema_hash: String,
        actor_id: String,
    ) -> anyhow::Result<()> {
        let object = fields
            .as_object()
            .context("control-plane chatbot config fields must be an object")?;
        const CONFIG_FIELDS: [&str; 9] = [
            "embedding_plugin",
            "qdrant_collection",
            "vector_dims",
            "output_dtype",
            "input_type",
            "dedup_window_hrs",
            "queue_alert_threshold",
            "nesting_policy",
            "vectorization_enabled",
        ];
        anyhow::ensure!(
            object.len() == CONFIG_FIELDS.len()
                && CONFIG_FIELDS
                    .iter()
                    .all(|field| object.contains_key(*field)),
            "control-plane chatbot publication must contain the complete config field set"
        );
        self.commit_state_update(
            StateChange {
                change_id: uuid::Uuid::new_v4().to_string(),
                event_id: 0,
                plugin_id: "ctl_plane_chatbot".to_string(),
                object_path: op_plugins::canonical::plugin_path("ctl_plane_chatbot"),
                change_type: ChangeType::PropertySet,
                member_name: None,
                old_value: None,
                new_value: simd_json::serde::to_owned_value(fields)?,
                tags_touched: Vec::new(),
                event_hash: String::new(),
                timestamp: chrono::Utc::now(),
                actor_id,
                source: ChangeSource::Internal,
            },
            Some(expected_schema_hash),
            true,
        )
        .await
        .map(|_| ())
    }

    pub(crate) async fn persist_tched_router_config_observation(
        &self,
        method: &str,
        represented: &serde_json::Value,
        expected_schema_hash: &str,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            represented.is_object(),
            "tched_router config observation must be an object"
        );
        self.persist_tched_router_fields(
            represented,
            expected_schema_hash.to_string(),
            format!("tched_router_config:{method}"),
        )
        .await
    }

    pub(crate) async fn read_tched_router_config_field(
        &self,
        field: &str,
        expected_schema_hash: &str,
    ) -> anyhow::Result<serde_json::Value> {
        anyhow::ensure!(
            matches!(
                field,
                "native_config"
                    | "catalog_model_binding"
                    | "model_assignments"
                    | "memory_namespaces"
                    | "registration_service"
                    | "user_container"
                    | "identity_chain"
                    | "privacy_policy"
            ),
            "field is not owned by the tched_router config facade"
        );
        let field = field.to_string();
        let expected = expected_schema_hash.to_string();
        tokio::task::spawn_blocking(move || -> anyhow::Result<serde_json::Value> {
            let catalog = crate::sealed_schema_reader::blob_catalog_dir_from_env();
            let before = crate::sealed_schema_reader::read_manifest_pinned_schema_document(
                &catalog,
                "tched_router",
            )?;
            anyhow::ensure!(
                before.schema_hash == expected,
                "tched_router seal changed before committed config read"
            );
            let mut snapshot =
                op_core::projection_shm::read_projection_snapshot(&["tched_router".to_string()])?;
            let bytes = snapshot
                .states
                .remove("tched_router")
                .context("tched_router has no committed state")?;
            let state: serde_json::Value = serde_json::from_slice(&bytes)
                .context("committed tched_router state is invalid JSON")?;
            let value = state
                .get(&field)
                .cloned()
                .with_context(|| format!("committed tched_router field '{field}' is absent"))?;
            let after = crate::sealed_schema_reader::read_manifest_pinned_schema_document(
                &catalog,
                "tched_router",
            )?;
            anyhow::ensure!(
                after.schema_hash == expected,
                "tched_router seal changed during committed config read"
            );
            Ok(value)
        })
        .await?
    }

    async fn persist_tched_router_fields(
        &self,
        fields: &serde_json::Value,
        expected_schema_hash: String,
        actor_id: String,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            fields.is_object(),
            "tched_router observed fields must be an object"
        );
        self.commit_state_update(
            StateChange {
                change_id: uuid::Uuid::new_v4().to_string(),
                event_id: 0,
                plugin_id: "tched_router".to_string(),
                object_path: op_plugins::canonical::plugin_path("tched_router"),
                change_type: ChangeType::PropertySet,
                member_name: None,
                old_value: None,
                new_value: simd_json::serde::to_owned_value(fields)?,
                tags_touched: Vec::new(),
                event_hash: String::new(),
                timestamp: chrono::Utc::now(),
                actor_id,
                source: ChangeSource::Internal,
            },
            Some(expected_schema_hash),
            true,
        )
        .await
        .map(|_| ())
    }

    pub(crate) async fn persist_tched_router_observation(
        &self,
        represented: &serde_json::Value,
        expected_schema_hash: &str,
    ) -> anyhow::Result<()> {
        let documents = op_plugins::state_plugins::runtime_documents::runtime_documents().await?;
        let methods =
            op_plugins::state_plugins::tched_router_legacy_methods::TchedRouterLegacyMethods::new(
                documents,
            );
        let (_, assignments) = methods.assignments().await?;
        let mut complete = represented.clone();
        let object = complete
            .as_object_mut()
            .context("tched_router runtime observation must be an object")?;
        object.insert(
            "model_assignments".into(),
            serde_json::to_value(assignments)?,
        );
        self.persist_tched_router_fields(
            &complete,
            expected_schema_hash.to_string(),
            "tched_router_selection".to_string(),
        )
        .await
        .map_err(|error| {
            anyhow::anyhow!(
                "tched_router native read completed but observation publication failed: {error}"
            )
        })
    }

    /// Read the internal working mirror used by domain dispatchers. Outward
    /// readers must use committed SHM, even while a dispatcher prepares state.
    pub async fn get_state(&self, plugin_id: &str) -> Option<simd_json::OwnedValue> {
        let cache = self.state_cache.read().await;
        cache.get(plugin_id).cloned()
    }

    /// Read public state from a committed SHM snapshot of a sealed plugin.
    /// Credential files are never opened by this reader.
    pub async fn get_public_state(&self, plugin_id: &str) -> Option<simd_json::OwnedValue> {
        let plugin_id = plugin_id.to_string();
        tokio::task::spawn_blocking(move || -> anyhow::Result<Option<simd_json::OwnedValue>> {
            let catalog = crate::sealed_schema_reader::blob_catalog_dir_from_env();
            let before = crate::sealed_schema_reader::manifest_plugins(&catalog)?;
            anyhow::ensure!(before.contains_key(&plugin_id), "plugin is not sealed");
            let document = crate::sealed_schema_reader::read_manifest_pinned_schema_document(
                &catalog, &plugin_id,
            )?;
            let mut snapshot = op_core::projection_shm::read_projection_snapshot_in(
                &op_core::projection_shm::shm_state_dir(),
                std::slice::from_ref(&plugin_id),
            )?;
            anyhow::ensure!(
                crate::sealed_schema_reader::manifest_plugins(&catalog)? == before,
                "sealed catalog changed during state read"
            );
            let Some(mut bytes) = snapshot.states.remove(&plugin_id) else {
                return Ok(None);
            };
            let state = simd_json::to_owned_value(&mut bytes)?;
            Ok(document
                .schema
                .project_public_state(&public_plugin_value(&plugin_id, &state))?)
        })
        .await
        .ok()?
        .ok()?
    }

    /// Prepare internal working state; this does not publish state or notify readers.
    pub async fn update_state_cache(&self, plugin_id: String, state: simd_json::OwnedValue) {
        let mut cache = self.state_cache.write().await;
        cache.insert(plugin_id, state);
    }

    /// Publish the current authoritative cache entry into the plugin's SHM
    /// projection after a domain dispatcher has completed its mutation.
    pub async fn publish_plugin_projection_from_cache(
        &self,
        plugin_id: &str,
        change_type: ChangeType,
    ) -> anyhow::Result<()> {
        let Some(state) = self.get_state(plugin_id).await else {
            return Ok(());
        };
        self.publish_plugin_projection_state(plugin_id, state, change_type, false)
            .await
    }

    /// Publish a prepared state; transactional callers commit the cache only
    /// after the SHM write succeeds, before announcing the update.
    pub(crate) async fn publish_plugin_projection_state(
        &self,
        plugin_id: &str,
        state: simd_json::OwnedValue,
        change_type: ChangeType,
        _commit_cache: bool,
    ) -> anyhow::Result<()> {
        self.commit_state_change(StateChange {
            change_id: uuid::Uuid::new_v4().to_string(),
            event_id: 0,
            plugin_id: plugin_id.to_string(),
            object_path: op_plugins::canonical::plugin_path(plugin_id),
            change_type,
            member_name: None,
            old_value: None,
            new_value: state,
            tags_touched: vec![],
            event_hash: String::new(),
            timestamp: chrono::Utc::now(),
            actor_id: format!("{plugin_id}_dispatch"),
            source: ChangeSource::Internal,
        })
        .await?;
        Ok(())
    }

    /// Fold against committed SHM while holding its cross-process transaction
    /// lock. Only the completed tree is mirrored and announced. Method inputs
    /// and observations belong to the audit/signal path, never to state.
    async fn commit_state_change(&self, change: StateChange) -> anyhow::Result<StateChange> {
        self.commit_state_change_pinned(change, None).await
    }

    async fn commit_state_change_pinned(
        &self,
        change: StateChange,
        expected_schema_hash: Option<String>,
    ) -> anyhow::Result<StateChange> {
        self.commit_state_update(change, expected_schema_hash, false)
            .await
    }

    async fn commit_state_update(
        &self,
        change: StateChange,
        expected_schema_hash: Option<String>,
        merge_fields: bool,
    ) -> anyhow::Result<StateChange> {
        self.commit_state_update_in(
            change,
            expected_schema_hash,
            merge_fields,
            op_core::projection_shm::shm_state_dir(),
            crate::sealed_schema_reader::blob_catalog_dir_from_env(),
        )
        .await
    }

    // The production path resolves its existing directories above. This
    // private seam lets owned-publication fixtures avoid mutable process env.
    async fn commit_state_update_in(
        &self,
        mut change: StateChange,
        expected_schema_hash: Option<String>,
        merge_fields: bool,
        state_dir: String,
        catalog_dir: std::path::PathBuf,
    ) -> anyhow::Result<StateChange> {
        anyhow::ensure!(
            !merge_fields
                || (change.change_type == ChangeType::PropertySet && change.member_name.is_none()),
            "field updates require a property-set object target"
        );
        if matches!(
            change.change_type,
            ChangeType::MethodCall | ChangeType::Signal
        ) {
            change.new_value = public_plugin_value(&change.plugin_id, &change.new_value);
            change.old_value = change
                .old_value
                .as_ref()
                .map(|value| public_plugin_value(&change.plugin_id, value));
            if change.change_type == ChangeType::Signal {
                let _ = self.change_tx.send(change.clone());
            }
            return Ok(change);
        }
        anyhow::ensure!(
            change.plugin_id != "identity_sled",
            "identity state requires its durable intent owner"
        );
        let guard = self.state_publication.clone().lock_owned().await;
        let cache = self.state_cache.clone();
        let tx = self.change_tx.clone();
        let signal_bus = self.signal_bus.clone();
        let runtime = tokio::runtime::Handle::current();
        tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
            let _guard = guard;
            let document = crate::sealed_schema_reader::read_manifest_pinned_schema_document(
                &catalog_dir,
                &change.plugin_id,
            )?;
            if let Some(expected) = expected_schema_hash {
                anyhow::ensure!(
                    document.blob.manifest.schema_hash == expected,
                    "sealed plugin changed before observation publication"
                );
            }
            let mut folded_state = None;
            let mut projected_previous = None;
            let mut projected_current = None;
            let fold = |previous: Option<&[u8]>| -> anyhow::Result<Option<Vec<u8>>> {
                let previous_state = previous
                    .map(|bytes| {
                        let mut bytes = bytes.to_vec();
                        simd_json::to_owned_value(&mut bytes)
                    })
                    .transpose()?;
                projected_previous = previous_state
                    .as_ref()
                    .map(|state| {
                        document
                            .schema
                            .project_public_state(&public_plugin_value(&change.plugin_id, state))
                    })
                    .transpose()?
                    .flatten();
                let folded = if merge_fields {
                    crate::state_tree_fold::merge_plugin_object_fields(
                        previous_state.as_ref(),
                        &change.plugin_id,
                        &change.object_path,
                        &change.new_value,
                    )?
                } else {
                    crate::state_tree_fold::fold_plugin_state(
                        previous_state.as_ref(),
                        &change.plugin_id,
                        &change.object_path,
                        change.change_type,
                        change.member_name.as_deref(),
                        &change.new_value,
                    )?
                };
                folded_state = match folded {
                    crate::state_tree_fold::StateTreeFold::Unchanged => previous_state,
                    crate::state_tree_fold::StateTreeFold::Present(state) => Some(state),
                    crate::state_tree_fold::StateTreeFold::Removed => None,
                };
                // Validate whole protected boundaries after folding children;
                // an empty public policy is not state validation.
                if let Some(state) = folded_state.as_ref() {
                    document.schema.validate_exact_schema_fields(state)?;
                }
                // Validate and derive the outbound view before committing any
                // bytes. Full backend state remains in the protected SHM root.
                projected_current = folded_state
                    .as_ref()
                    .map(|state| {
                        document
                            .schema
                            .project_public_state(&public_plugin_value(&change.plugin_id, state))
                    })
                    .transpose()?
                    .flatten();
                folded_state
                    .as_ref()
                    .map(simd_json::to_vec)
                    .transpose()
                    .map_err(Into::into)
            };
            let receipt =
                op_core::projection_shm::update_projection_in(&state_dir, &change.plugin_id, fold)?;
            {
                let mut cache = cache.blocking_write();
                if let Some(state) = folded_state {
                    cache.insert(change.plugin_id.clone(), state);
                } else {
                    cache.remove(&change.plugin_id);
                }
            }
            let committed = receipt.previous != receipt.current;
            if committed {
                let removed = receipt.current.is_none();
                let mut notification = change.clone();
                notification.object_path = op_plugins::canonical::plugin_path(&change.plugin_id);
                notification.member_name = None;
                notification.old_value = projected_previous.clone();
                notification.new_value =
                    projected_current.clone().unwrap_or(simd_json::json!(null));
                notification.change_type = if removed {
                    ChangeType::ObjectRemoved
                } else {
                    ChangeType::PropertySet
                };
                let _ = tx.send(notification);
                let plugin_id = change.plugin_id.clone();
                // This task owns notification delivery even if the original
                // RPC is cancelled after the SHM transaction commits.
                runtime.spawn(async move {
                    Self::emit_updated_signal_with_bus(signal_bus, plugin_id, None, None).await;
                });
            }
            change.object_path = op_plugins::canonical::plugin_path(&change.plugin_id);
            change.member_name = None;
            change.new_value = projected_current.unwrap_or(simd_json::json!(null));
            change.old_value = projected_previous;
            Ok(change)
        })
        .await?
    }

    /// Route a D-Bus method call through the authoritative bridge
    #[allow(clippy::too_many_arguments)]
    pub async fn call_dbus_method(
        &self,
        bus_name: &str,
        path: &str,
        interface: &str,
        method: &str,
        _args: Vec<simd_json::OwnedValue>,
        _actor_id: &str,
        capability_id: &Option<String>,
    ) -> anyhow::Result<simd_json::OwnedValue> {
        tracing::debug!(
            bus_name,
            path,
            interface,
            method,
            capability_id = ?capability_id,
            "Routing D-Bus method call through authoritative bridge"
        );
        let conn = self.dbus_connection().await?;
        let proxy = Proxy::new(&conn, bus_name, path, interface).await?;
        let result: ZOwnedValue = proxy.call(method, &()).await?;
        simd_json::serde::to_owned_value(&result).map_err(|e| anyhow::anyhow!(e))
    }

    pub fn change_tx(&self) -> broadcast::Sender<StateChange> {
        self.change_tx.clone()
    }

    /// Wake state consumers after the existing catalog reload path observes a
    /// new sealed catalog. No schema/state payload is authoritative here: each
    /// reader must pin the catalog and read committed SHM again.
    pub(crate) fn notify_catalog_changed(&self) {
        let _ = self.change_tx.send(StateChange {
            change_id: uuid::Uuid::new_v4().to_string(),
            event_id: 0,
            plugin_id: String::new(),
            object_path: op_plugins::canonical::DBUS_ROOT_PATH.to_string(),
            change_type: ChangeType::SchemaMigration,
            member_name: None,
            old_value: None,
            new_value: simd_json::OwnedValue::Static(simd_json::StaticNode::Null),
            tags_touched: Vec::new(),
            event_hash: String::new(),
            timestamp: chrono::Utc::now(),
            actor_id: "sealed-catalog".to_string(),
            source: ChangeSource::Internal,
        });
    }
}

/// Result of an authoritative mutation
#[derive(Debug, Clone)]
pub struct MutationResult {
    pub success: bool,
    pub event_id: u64,
    pub event_hash: String,
    pub result: Option<simd_json::OwnedValue>,
    pub error: Option<MutationError>,
}

#[derive(Debug, Clone)]
pub struct MutationError {
    pub code: ErrorCode,
    pub message: String,
}

#[derive(Debug, Clone, Copy)]
pub enum ErrorCode {
    NotFound,
    PermissionDenied,
    ValidationFailed,
    ReadOnly,
    Internal,
}

// ── Audit trail: durable persistence + query dispatch ────────────────────────

/// Convert a chain event into a durable footprint for the `timing_subvol`.
///
/// The full event is the footprint payload so startup rebuild can restore it
/// losslessly, including the stored `event_hash` and `prev_hash` that make the
/// chain tamper-evident. Flat metadata keys beside it exist for operators and
/// tools grepping the timing directory directly.
///
/// OSCAL subid: `evt.service.event-chain.persist@v1`
fn event_to_footprint(event: &ChainEvent) -> PluginFootprint {
    let mut metadata: HashMap<String, simd_json::OwnedValue> = HashMap::new();
    metadata.insert(
        "actor_id".to_string(),
        simd_json::OwnedValue::from(event.actor_id.as_str()),
    );
    if let Some(session_id) = event.session_id.as_deref() {
        metadata.insert(
            "session_id".to_string(),
            simd_json::OwnedValue::from(session_id),
        );
    }
    if let Some(session_genesis) = event.session_genesis.as_deref() {
        metadata.insert(
            "session_genesis".to_string(),
            simd_json::OwnedValue::from(session_genesis),
        );
    }
    if let Some(sealed_id_digest) = event.sealed_id_digest.as_deref() {
        metadata.insert(
            "sealed_id_digest".to_string(),
            simd_json::OwnedValue::from(sealed_id_digest),
        );
    }
    metadata.insert(
        "capability_id".to_string(),
        simd_json::OwnedValue::from(event.capability_id.clone().unwrap_or_default()),
    );
    metadata.insert(
        "method_name".to_string(),
        simd_json::OwnedValue::from(event.method_name.clone().unwrap_or_default()),
    );
    metadata.insert(
        "event_id".to_string(),
        simd_json::OwnedValue::from(event.event_id),
    );
    metadata.insert(
        "event_hash".to_string(),
        simd_json::OwnedValue::from(event.event_hash.as_str()),
    );
    metadata.insert(
        "decision".to_string(),
        simd_json::OwnedValue::from(format!("{:?}", event.decision)),
    );
    // Lossless payload for replay, hashing, and vectorization. The footprint
    // transports this value; it does not replace it with another digest.
    let payload = simd_json::serde::to_owned_value(event).unwrap_or_else(|error| {
        tracing::warn!(%error, event_id = event.event_id, "audit event not serializable");
        simd_json::json!({ "event_id": event.event_id, "serialization_error": true })
    });

    PluginFootprint {
        plugin_id: event.plugin_id.clone(),
        operation: event
            .method_name
            .clone()
            .unwrap_or_else(|| format!("{:?}", event.op)),
        timestamp: event.timestamp.timestamp_millis() as u64,
        payload,
        metadata,
        vector_features: vec![],
    }
}

/// Flatten a chain event into the plugin-surface record shape.
fn chain_event_to_record(event: &ChainEvent) -> AuditEventRecord {
    AuditEventRecord {
        event_id: event.event_id,
        event_hash: event.event_hash.clone(),
        prev_hash: event.prev_hash.clone(),
        timestamp: event.timestamp.to_rfc3339(),
        actor_id: event.actor_id.clone(),
        capability_id: event.capability_id.clone().unwrap_or_default(),
        plugin_id: event.plugin_id.clone(),
        method_name: event.method_name.clone().unwrap_or_default(),
        operation_type: format!("{:?}", event.op),
        target: event.target.clone(),
        tags_touched: event.tags_touched.clone(),
        decision: format!("{:?}", event.decision),
        input_patch_hash: event.input_patch_hash.clone(),
        result_effective_hash: event.result_effective_hash.clone().unwrap_or_default(),
    }
}

/// `snowball.query_events` — paginated, filtered read of the audit trail.
///
/// Reads the same `EventChain` the gRPC `EventChainService.GetEvents` serves, so
/// the D-Bus/MCP path and the GUI path can never disagree.
///
/// OSCAL subid: `obs.service.snowball.events.query@v1`
async fn dispatch_snowball_query_events(
    event_chain: &Arc<RwLock<EventChain>>,
    args: &simd_json::OwnedValue,
) -> anyhow::Result<serde_json::Value> {
    let input: QueryEventsInput = serde_json::from_value(serde_json::to_value(args)?)
        .context("invalid Snowball query arguments")?;

    // Default 50, hard ceiling 100 — clamped silently, never unbounded (FR-4).
    let limit = input.limit.unwrap_or(50).clamp(1, 100) as usize;

    let chain = event_chain.read().await;
    let total_in_chain = chain.events().len() as u64;

    let decision_filter = input.decision.as_deref().map(str::to_ascii_lowercase);

    let mut matched: Vec<AuditEventRecord> = chain
        .events()
        .iter()
        .filter(|e| input.from_event_id.is_none_or(|id| e.event_id >= id))
        .filter(|e| input.to_event_id.is_none_or(|id| e.event_id <= id))
        .filter(|e| {
            input
                .plugin_id
                .as_ref()
                .is_none_or(|p| p.is_empty() || e.plugin_id == *p)
        })
        .filter(|e| match decision_filter.as_deref() {
            Some("allow") => e.decision == Decision::Allow,
            Some("deny") => e.decision == Decision::Deny,
            _ => true,
        })
        // One extra row reveals whether another page exists.
        .take(limit + 1)
        .map(chain_event_to_record)
        .collect();

    let has_more = matched.len() > limit;
    matched.truncate(limit);

    Ok(serde_json::to_value(QueryEventsOutput {
        events: matched,
        has_more,
        total_in_chain,
    })?)
}

/// `snowball.verify_chain` — hash-chain integrity check over a range.
///
/// OSCAL subid: `obs.service.snowball.chain.verify@v1`
async fn dispatch_snowball_verify_chain(
    event_chain: &Arc<RwLock<EventChain>>,
    args: &simd_json::OwnedValue,
) -> anyhow::Result<serde_json::Value> {
    let input: VerifyChainInput = serde_json::from_value(serde_json::to_value(args)?)
        .context("invalid Snowball verification arguments")?;

    let chain = event_chain.read().await;
    let result = chain.verify_range(
        input.from_event_id.unwrap_or(0),
        input.to_event_id.unwrap_or(0),
    );

    Ok(serde_json::to_value(VerifyChainOutput {
        valid: result.valid,
        events_verified: result.events_verified as u64,
        errors: result.errors,
    })?)
}

/// Execute rovs_commands methods via OVSDB proxy
async fn dispatch_rovs_commands_method(
    ovsdb: &op_network::rovs_proxy::OvsdbDbusClient,
    method: &str,
    args: &simd_json::OwnedValue,
) -> anyhow::Result<serde_json::Value> {
    match method {
        "create_bridge" => {
            let bridge_name = args
                .get("bridge_name")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("bridge_name required"))?;
            ovsdb.create_bridge(bridge_name).await?;
            Ok(serde_json::json!({"created": bridge_name}))
        }
        "delete_bridge" => {
            let bridge_name = args
                .get("bridge_name")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("bridge_name required"))?;
            ovsdb.delete_bridge(bridge_name).await?;
            Ok(serde_json::json!({"deleted": bridge_name}))
        }
        "add_port" => {
            let bridge_name = args
                .get("bridge_name")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("bridge_name required"))?;
            let port_name = args
                .get("port_name")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("port_name required"))?;
            let interface_type = args
                .get("interface_type")
                .and_then(|v| v.as_str())
                .unwrap_or("internal");
            ovsdb
                .add_port_with_type(bridge_name, port_name, Some(interface_type))
                .await?;
            Ok(serde_json::json!({
                "added": port_name,
                "to": bridge_name,
                "interface_type": interface_type
            }))
        }
        "ensure_internal_port" => {
            let bridge_name = args
                .get("bridge_name")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("bridge_name required"))?;
            let port_name = args
                .get("port_name")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("port_name required"))?;
            let created = ovsdb.ensure_internal_port(bridge_name, port_name).await?;
            Ok(serde_json::json!({
                "bridge_name": bridge_name,
                "port_name": port_name,
                "interface_type": "internal",
                "created": created
            }))
        }
        "remove_port" => {
            let bridge_name = args
                .get("bridge_name")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("bridge_name required"))?;
            let port_name = args
                .get("port_name")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("port_name required"))?;
            ovsdb.delete_port(bridge_name, port_name).await?;
            Ok(serde_json::json!({"removed": port_name, "from": bridge_name}))
        }
        "list_bridges" => {
            let bridges = ovsdb.list_bridges().await?;
            Ok(serde_json::json!(bridges))
        }
        "list_ports" => {
            let bridge_name = args
                .get("bridge_name")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("bridge_name required"))?;
            let ports = ovsdb.list_bridge_ports(bridge_name).await?;
            Ok(serde_json::json!(ports))
        }
        "list_dbs" => {
            let dbs = ovsdb.list_dbs().await?;
            Ok(serde_json::json!(dbs))
        }
        _ => Err(anyhow::anyhow!("unknown rovs_commands method: {}", method)),
    }
}

/// Execute the schema-declared OVSDB bridge methods used by canonical
/// `org.opdbus.v1.plugins` compatibility clients.
async fn dispatch_ovsdb_bridge_method(
    ovsdb: &op_network::rovs_proxy::OvsdbDbusClient,
    owner: &str,
    method: &str,
    args: &simd_json::OwnedValue,
) -> anyhow::Result<serde_json::Value> {
    let args = serde_json::to_value(args)?;
    match method {
        "list_dbs" => Ok(serde_json::json!({"databases": ovsdb.list_dbs().await?})),
        "get_schema" => {
            let input: op_plugins::state_plugins::ovsdb_bridge::GetSchemaInput =
                serde_json::from_value(args)?;
            anyhow::ensure!(
                input.db_name == "Open_vSwitch",
                "unsupported OVSDB database"
            );
            Ok(serde_json::json!({"schema": ovsdb.get_schema().await?}))
        }
        "transact" => {
            let input: op_plugins::state_plugins::ovsdb_bridge::TransactInput =
                serde_json::from_value(args)?;
            anyhow::ensure!(
                input.db_name == "Open_vSwitch",
                "unsupported OVSDB database"
            );
            let operations = simd_json::serde::to_owned_value(input.operations)?;
            let results = ovsdb.transact_simd(operations).await?;
            anyhow::ensure!(
                results.is_array(),
                "OVSDB transact returned a non-array result"
            );
            Ok(serde_json::json!({"results": results}))
        }
        "create_bridge" => {
            let input: op_plugins::state_plugins::ovsdb_bridge::CreateBridgeInput =
                serde_json::from_value(args)?;
            ovsdb.create_bridge(&input.name).await?;
            if let Some(value) = input.datapath_type {
                ovsdb
                    .set_bridge_property(&input.name, "datapath_type", &value)
                    .await?;
            }
            if let Some(value) = input.fail_mode {
                ovsdb
                    .set_bridge_property(&input.name, "fail_mode", &value)
                    .await?;
            }
            if let Some(value) = input.stp_enable {
                ovsdb.set_bridge_stp(&input.name, value).await?;
            }
            Ok(serde_json::json!({"bridge": input.name, "changed": true}))
        }
        "delete_bridge" => {
            let input: op_plugins::state_plugins::ovsdb_bridge::DeleteBridgeInput =
                serde_json::from_value(args)?;
            ovsdb.delete_bridge(&input.name).await?;
            Ok(serde_json::json!({"bridge": input.name, "changed": true}))
        }
        "add_port" => {
            let input: op_plugins::state_plugins::ovsdb_bridge::AddPortInput =
                serde_json::from_value(args)?;
            ovsdb
                .add_port_with_options(
                    &input.bridge,
                    &input.port,
                    input.interface.as_deref(),
                    input.tag,
                )
                .await?;
            Ok(serde_json::json!({"bridge": input.bridge, "port": input.port, "changed": true}))
        }
        "remove_port" => {
            let input: op_plugins::state_plugins::ovsdb_bridge::RemovePortInput =
                serde_json::from_value(args)?;
            ovsdb.delete_port(&input.bridge, &input.port).await?;
            Ok(serde_json::json!({"bridge": input.bridge, "port": input.port, "changed": true}))
        }
        "echo" => {
            let input: op_plugins::state_plugins::ovsdb_bridge::EchoInput =
                serde_json::from_value(args)?;
            let echoed = ovsdb
                .native_rpc("echo", serde_json::json!(input.params))
                .await?;
            let params = echoed
                .as_array()
                .context("OVSDB echo returned a non-array result")?;
            Ok(serde_json::json!({"params": params}))
        }
        "lock" => {
            let input: op_plugins::state_plugins::ovsdb_bridge::LockInput =
                serde_json::from_value(args)?;
            let receipt = ovsdb.lock(owner, &input.lock_id).await?;
            Ok(serde_json::json!({
                "lock_id": receipt.lock_id,
                "locked": receipt.locked,
                "queued": receipt.queued,
            }))
        }
        "steal" => {
            let input: op_plugins::state_plugins::ovsdb_bridge::StealInput =
                serde_json::from_value(args)?;
            let receipt = ovsdb.steal(owner, &input.lock_id).await?;
            Ok(serde_json::json!({
                "lock_id": receipt.lock_id,
                "locked": receipt.locked,
                "queued": receipt.queued,
            }))
        }
        "unlock" => {
            let input: op_plugins::state_plugins::ovsdb_bridge::UnlockInput =
                serde_json::from_value(args)?;
            let receipt = ovsdb.unlock(owner, &input.lock_id).await?;
            Ok(serde_json::json!({
                "lock_id": receipt.lock_id,
                "unlocked": receipt.unlocked,
                "acquired_while_queued": receipt.acquired_while_queued,
                "stolen_before_unlock": receipt.stolen_before_unlock,
            }))
        }
        "monitor" | "monitor_cond" | "cancel" => {
            anyhow::bail!("OVSDB monitor lifecycle must use the MutationEngine SHM owner")
        }
        _ => Err(anyhow::anyhow!("unknown ovsdb_bridge method: {method}")),
    }
}

/// Dispatch the `xray` plugin's schema-declared methods through xray-core's
/// commander UDS. Process reload and trace ingestion are deliberately rejected:
/// the pinned commander protocol has no operations with those semantics.
async fn dispatch_xray_method(
    method: &str,
    args: &serde_json::Value,
) -> anyhow::Result<serde_json::Value> {
    match method {
        "get_stats" => crate::xray_methods::dispatch_stats_method(args).await,
        "add_user" | "remove_user" | "add_inbound" => {
            crate::xray_methods::dispatch_handler_method(method, args).await
        }
        "restart" | "start_trace" | "end_trace" | "record_span" => Err(anyhow::anyhow!(
            "xray.{method} is unavailable: the pinned xray-core commander API has no matching operation"
        )),
        _ => Err(anyhow::anyhow!("unknown xray method: {}", method)),
    }
}

#[cfg(test)]
mod xray_unavailable_method_tests {
    use super::dispatch_xray_method;

    #[tokio::test]
    async fn reload_and_trace_methods_fail_before_any_provider_io() {
        for method in ["restart", "start_trace", "end_trace", "record_span"] {
            let error = dispatch_xray_method(method, &serde_json::json!({}))
                .await
                .expect_err("method without a native protocol operation must fail closed");
            assert_eq!(
                error.to_string(),
                format!(
                    "xray.{method} is unavailable: the pinned xray-core commander API has no matching operation"
                )
            );
        }
    }
}

/// Dispatch a `cognitive_mcp` schema method to the cognitive tool registry.
///
/// OSCAL subid: mut.service.cognitive-mcp.dispatch@v1
///
/// The runtime is in-process, but this function is reached only after the call has
/// passed through the canonical `PluginService`/`MutationEngine` method gate.  The
/// registry is therefore an implementation detail behind D-Bus, not a second
/// listener or control plane.
impl MutationEngine {
    async fn dispatch_cognitive_mcp_method(
        &self,
        caller: crate::specialist::CognitiveCaller<'_>,
        method: &str,
        args: &serde_json::Value,
        admitted_schema: &op_state_store::PluginSchema,
    ) -> anyhow::Result<serde_json::Value> {
        if method == "invoke_tool" {
            let requested = args
                .get("tool_name")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            let mut arguments = args
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| serde_json::json!({}));
            let target = canonical_generic_cognitive_target(requested, &mut arguments)?;
            anyhow::ensure!(
                requested != "cognitive_memory" || admitted_schema.methods.contains_key(target),
                "generic memory target is absent from the admitted schema"
            );
            if let Some(declaration) = admitted_schema.methods.get(target) {
                let required = declaration
                    .required_capability
                    .as_deref()
                    .context("target requires exact capability")?;
                anyhow::ensure!(
                    crate::interceptor::load_capability_grants(caller.actor_id).contains(required),
                    "generic invocation does not grant the target capability"
                );
                let envelope = self
                    .dispatch_method_call_with_identity(
                        "cognitive_mcp",
                        target,
                        &serde_json::to_string(&arguments)?,
                        Some(required),
                        caller.actor_id,
                        caller.session_id,
                        caller.session_genesis,
                    )
                    .await?;
                return Ok(
                    serde_json::json!({"success":true,"tool_name":requested,"result":envelope.get("result").unwrap_or(&envelope)}),
                );
            }
        }
        if let Some(catalog) = op_plugins::state_plugins::cognitive_agents::method_by_name(method) {
            return crate::agent_catalog_dispatch::dispatch(
                self,
                caller,
                catalog,
                args,
                admitted_schema,
                method,
            )
            .await;
        }
        if let Some((catalog, normalized)) =
            crate::agent_catalog_dispatch::hot_method(method, args)?
        {
            crate::method_contract_admission::validate_admitted_arguments(
                &catalog.declaration,
                &normalized,
            )?;
            return crate::agent_catalog_dispatch::dispatch(
                self,
                caller,
                catalog,
                &normalized,
                admitted_schema,
                method,
            )
            .await;
        }
        if method == "specialist" {
            return crate::specialist::consult(self, caller, args, admitted_schema, method).await;
        }
        // Health answers from the in-process runtime itself. It used to read a
        // `cognitive_mcp` SHM projection that nothing ever published, so it
        // failed on every bridge. Starting the runtime is the health check:
        // it fails exactly when every other cognitive operation would.
        if method == "get_health" {
            let runtime = self.cognitive_mcp().await?;
            let (remaining, limit) = runtime.quota_manager().status().await;
            return Ok(serde_json::json!({
                "bridge_owned": true,
                "execution_model": "in-process",
                // Callers are authenticated by the bridge before dispatch.
                "auth_status": "bridge",
                "queries_remaining": i64::from(remaining),
                "queries_limit": i64::from(limit),
            }));
        }

        // `list_tools` is an MCP *protocol* method (`tools/list`), not an entry in the
        // tool registry, so it cannot go through `tools/call`.
        if method == "list_tools" {
            let runtime = self.cognitive_mcp().await?;
            let descriptors = runtime
                .tool_registry()
                .list_descriptors()
                .await
                .into_iter()
                .filter_map(|descriptor| serde_json::to_value(descriptor).ok())
                .filter_map(project_sealed_cognitive_tool_descriptor)
                .filter_map(|descriptor| {
                    descriptor
                        .get("name")
                        .and_then(|name| name.as_str())
                        .map(str::to_owned)
                })
                .collect::<Vec<_>>();
            return Ok(serde_json::json!({ "tools": descriptors }));
        }

        if method == "schema_read" {
            let plugin_id = args
                .get("plugin_id")
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| anyhow::anyhow!("schema_read requires plugin_id"))?
                .to_string();
            let dir = crate::sealed_schema_reader::blob_catalog_dir_from_env();
            return tokio::task::spawn_blocking(move || {
                crate::sealed_schema_reader::read_sealed_schema_result(&dir, &plugin_id)
            })
            .await
            .map_err(|error| anyhow::anyhow!("sealed schema task failed: {error}"))?;
        }

        if method == "oscal_subids" {
            let plugin_id = args
                .get("plugin_id")
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string);
            let prefix = args
                .get("prefix")
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string);
            let dir = crate::sealed_schema_reader::blob_catalog_dir_from_env();
            return tokio::task::spawn_blocking(move || {
                crate::sealed_schema_reader::read_oscal_subids_result(
                    &dir,
                    plugin_id.as_deref(),
                    prefix.as_deref(),
                )
            })
            .await
            .map_err(|error| anyhow::anyhow!("OSCAL subid task failed: {error}"))?;
        }

        // Admission and the actual policy result share this boundary for every
        // transport. The stateless selector is never identity/grant authority.
        if method == "toolsets" {
            return crate::mcp_frontend::canonical_toolsets_result(self, caller.actor_id, args)
                .await;
        }

        if method == "workflow_query" {
            return crate::workstack_runner::dispatch(
                self,
                "query_workflows",
                args,
                caller.actor_id,
                caller.session_id,
                caller.session_genesis,
            )
            .await;
        }
        if method == "workflow_run" {
            return crate::workstack_runner::dispatch(
                self,
                "start_workflow",
                args,
                caller.actor_id,
                caller.session_id,
                caller.session_genesis,
            )
            .await;
        }

        // Memory is bound to the principal's selected mode (OD-65): the
        // namespace comes from the selection, never from the caller.
        let mut bound_args = args.clone();
        if method == "code_context" {
            let session = crate::project_dispatch::verified_session(
                self,
                caller.actor_id,
                caller.session_id,
                caller.session_genesis,
            )
            .await?;
            let context =
                crate::memory_binding::context_for(self, caller.actor_id, Some(&session)).await?;
            let object = bound_args
                .as_object_mut()
                .context("code context requires an argument object")?;
            object.insert("session_id".into(), session.into());
            object.insert(
                "memory_namespace".into(),
                if context.mode.has_memory() {
                    context.memory_namespace.into()
                } else {
                    serde_json::Value::Null
                },
            );
        }
        let memory_context = if crate::memory_binding::is_memory_method(method) {
            let context =
                crate::memory_binding::context_for(self, caller.actor_id, caller.session_id)
                    .await?;
            crate::memory_binding::bind_args(method, &context, &mut bound_args)?;
            Some(context)
        } else {
            None
        };
        let (tool_name, tool_args) = map_schema_method_to_tool(method, &bound_args)?;
        let runtime = self.cognitive_mcp().await?;
        let input = simd_json::serde::to_owned_value(&tool_args)?;
        let result = runtime.tool_registry().execute(&tool_name, input).await?;
        let mut value = serde_json::to_value(result)?;
        if method == "memory_list_namespaces"
            && let Some(context) = &memory_context
        {
            crate::memory_binding::filter_listing(context, &mut value);
        }
        // Executor outcomes are session activity: a failing build or test is
        // exactly what the context engine's `build_error`/`test_failure`
        // triggers exist for, and the resulting knowledge push reaches the
        // chatbot's next turn (context bundle) or `specialist` call.
        if tool_name.starts_with("agent_") {
            let operation = args
                .get("operation")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let success = value
                .get("success")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            let activity = match (success, operation) {
                (false, "test") => op_cognitive_mcp::context_awareness::ActivityType::TestFailure,
                (false, _) => op_cognitive_mcp::context_awareness::ActivityType::BuildError,
                (true, _) => op_cognitive_mcp::context_awareness::ActivityType::ToolCall,
            };
            let data = value
                .get("data")
                .map(|d| match d {
                    serde_json::Value::String(text) => text.clone(),
                    other => other.to_string(),
                })
                .unwrap_or_default();
            let content: String = data.chars().take(4_000).collect();
            runtime
                .context_engine()
                .record_activity(
                    caller.scope(),
                    activity,
                    format!("{method}({operation}): {content}"),
                    serde_json::json!({
                        "tool": tool_name,
                        "success": success,
                        "path": args.get("path"),
                    }),
                )
                .await;
        }
        if method == "invoke_tool" {
            return Ok(serde_json::json!({
                "success": true,
                "tool_name": args.get("tool_name").and_then(|name| name.as_str()).unwrap_or_default(),
                "result": value
            }));
        }
        Ok(value)
    }
}

/// Normalize implementation-only memory operations into admitted public methods.
fn canonical_generic_cognitive_target<'a>(
    requested: &'a str,
    arguments: &mut serde_json::Value,
) -> anyhow::Result<&'a str> {
    anyhow::ensure!(
        requested != "invoke_tool",
        "recursive generic invocation is forbidden"
    );
    if requested != "cognitive_memory" {
        return Ok(requested);
    }
    let operation = arguments
        .get("operation")
        .and_then(|v| v.as_str())
        .context("generic memory invocation requires operation")?;
    let target = match operation {
        "store" => "memory_store",
        "retrieve" => "memory_retrieve",
        "query" => "memory_query",
        "semantic_query" => "memory_semantic_query",
        "delete" => "memory_delete",
        "list_namespaces" => "memory_list_namespaces",
        _ => anyhow::bail!("generic memory operation has no admitted public method"),
    };
    if let Some(object) = arguments.as_object_mut() {
        object.remove("operation");
    }
    Ok(target)
}

/// Project a runtime tool candidate through the sealed cognitive PluginSchema.
/// Implementation-only tools remain internal unless their exact authority matches.
fn project_sealed_cognitive_tool_descriptor(
    mut candidate: serde_json::Value,
) -> Option<serde_json::Value> {
    let name = candidate.get("name")?.as_str()?.to_string();
    let candidate_capability = candidate.get("required_capability")?.as_str()?;
    let candidate_subid = candidate.get("subid")?.as_str()?;
    if let Some(agent) = op_plugins::state_plugins::cognitive_agents::method_by_name(&name) {
        let declaration = &agent.declaration;
        if Some(candidate_capability) != declaration.required_capability.as_deref()
            || candidate_subid != declaration.subid
        {
            return None;
        }
        candidate
            .as_object_mut()?
            .insert("authority_method".into(), name.into());
        return Some(candidate);
    }
    let schema = op_plugins::cognitive_mcp_plugin_schema();
    let method = schema.methods.get(&name)?;
    let sealed_capability = method.required_capability.as_deref()?;
    if candidate_capability != sealed_capability || candidate_subid != method.subid {
        return None;
    }
    if !schema.capabilities.contains_key(sealed_capability) {
        return None;
    }
    let object = candidate.as_object_mut()?;
    object.insert(
        "required_capability".into(),
        serde_json::Value::String(sealed_capability.to_string()),
    );
    object.insert(
        "subid".into(),
        serde_json::Value::String(method.subid.clone()),
    );
    object.insert(
        "authority_method".into(),
        serde_json::Value::String(method.name.clone()),
    );
    Some(candidate)
}

/// Translate a `cognitive_mcp` schema method name into a live tool-registry call.
///
/// The schema method names do not map one-to-one onto registry tool names: the
/// registry exposes a single `cognitive_memory` tool selected by an `operation`
/// field, so the memory methods inject that field rather than existing as separate
/// tools. `invoke_tool` bypasses the table entirely and names its tool directly.
fn map_schema_method_to_tool(
    method: &str,
    args: &serde_json::Value,
) -> anyhow::Result<(String, serde_json::Value)> {
    if let Some((catalog, normalized)) = crate::agent_catalog_dispatch::hot_method(method, args)? {
        crate::method_contract_admission::validate_admitted_arguments(
            &catalog.declaration,
            &normalized,
        )?;
        return Ok((catalog.declaration.name.clone(), normalized));
    }
    let base = if args.is_object() {
        args.clone()
    } else {
        serde_json::json!({})
    };

    let with_field = |key: &str, value: &str| -> serde_json::Value {
        let mut v = base.clone();
        if let Some(obj) = v.as_object_mut() {
            obj.insert(
                key.to_string(),
                serde_json::Value::String(value.to_string()),
            );
        }
        v
    };

    match method {
        // Memory methods all resolve to the one `cognitive_memory` tool.
        "memory_store" => Ok(("cognitive_memory".into(), with_field("operation", "store"))),
        "memory_recall" => Ok(("cognitive_memory".into(), with_field("operation", "query"))),
        "memory_query" => Ok(("cognitive_memory".into(), with_field("operation", "query"))),
        "memory_retrieve" => Ok((
            "cognitive_memory".into(),
            with_field("operation", "retrieve"),
        )),
        "memory_delete" => Ok(("cognitive_memory".into(), with_field("operation", "delete"))),
        "memory_list_namespaces" => Ok((
            "cognitive_memory".into(),
            with_field("operation", "list_namespaces"),
        )),
        "memory_semantic_query" => Ok((
            "cognitive_memory".into(),
            with_field("operation", "semantic_query"),
        )),
        // Code retrieval / indexing.
        "code_search" => Ok(("code_search".into(), base)),
        "code_index" => Ok(("code_index".into(), base)),
        "code_context" => Ok(("code_context".into(), base)),
        "gemini_query" => Ok(("gemini_query".into(), base)),
        // Generic door: the caller names the tool.
        "invoke_tool" => {
            let tool_name = args
                .get("tool_name")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("invoke_tool: missing required field 'tool_name'"))?
                .to_string();
            if tool_name.is_empty() {
                return Err(anyhow::anyhow!(
                    "invoke_tool: 'tool_name' must not be empty"
                ));
            }
            let tool_args = args
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| serde_json::json!({}));
            Ok((tool_name, tool_args))
        }
        other => Err(anyhow::anyhow!(
            "cognitive_mcp method '{other}' has no tool-registry mapping"
        )),
    }
}

#[cfg(test)]
mod cognitive_dispatch_tests {
    use super::{map_schema_method_to_tool, project_sealed_cognitive_tool_descriptor};

    #[test]
    fn generic_memory_routes_through_public_methods_and_rejects_unbound_operations() {
        for (operation, target) in [
            ("store", "memory_store"),
            ("delete", "memory_delete"),
            ("query", "memory_query"),
            ("semantic_query", "memory_semantic_query"),
            ("retrieve", "memory_retrieve"),
            ("list_namespaces", "memory_list_namespaces"),
        ] {
            let mut args = serde_json::json!({"operation":operation,"key":"k"});
            assert_eq!(
                super::canonical_generic_cognitive_target("cognitive_memory", &mut args).unwrap(),
                target
            );
            assert!(args.get("operation").is_none());
        }
        for operation in ["ensure_soul", "stats", "unknown"] {
            assert!(
                super::canonical_generic_cognitive_target(
                    "cognitive_memory",
                    &mut serde_json::json!({"operation":operation})
                )
                .is_err()
            );
        }
        assert!(
            super::canonical_generic_cognitive_target("invoke_tool", &mut serde_json::json!({}))
                .is_err()
        );
    }

    #[test]
    fn cognitive_discovery_contracts_reject_admission_placeholders_and_descriptor_objects() {
        let schema = op_plugins::cognitive_mcp_plugin_schema();
        let validate = crate::method_contract_admission::validate_admitted_output;
        assert!(
            validate(
                &schema.methods["toolsets"],
                &serde_json::json!({"admitted":true})
            )
            .is_err()
        );
        assert!(
            validate(
                &schema.methods["list_tools"],
                &serde_json::json!({"tools":[{"name":"gemini_query"}]})
            )
            .is_err()
        );
        validate(
            &schema.methods["list_tools"],
            &serde_json::json!({"tools":["gemini_query"]}),
        )
        .unwrap();
        validate(
            &schema.methods["toolsets"],
            &serde_json::json!({
                "operation":"list", "toolset_generation":1, "relist_required":false,
                "result":{"sets":[]}
            }),
        )
        .unwrap();
    }

    #[test]
    fn tool_authority_is_projected_only_from_an_exact_sealed_method() {
        let projected = project_sealed_cognitive_tool_descriptor(serde_json::json!({
            "name": "code_search",
            "required_capability": "cognitive_mcp.read",
            "subid": "obs.service.code-rag.search@v1",
            "input_schema": {"type": "object"}
        }))
        .expect("code_search has an exact sealed MethodDecl");
        assert_eq!(projected["authority_method"], "code_search");
        assert_eq!(projected["required_capability"], "cognitive_mcp.read");

        assert!(
            project_sealed_cognitive_tool_descriptor(serde_json::json!({
                "name": "search_blob_vectors",
                "required_capability": "cognitive_mcp.read",
                "subid": "obs.service.cognitive-mcp.blob-vectors.search@v1",
                "input_schema": {"type": "object"}
            }))
            .is_none()
        );
        assert!(
            project_sealed_cognitive_tool_descriptor(serde_json::json!({
                "name": "code_index",
                "required_capability": "cognitive_mcp.read",
                "subid": "mut.service.code-rag.index@v1",
                "input_schema": {"type": "object"}
            }))
            .is_none()
        );
    }

    #[test]
    fn code_methods_route_to_the_real_code_tools() {
        let args = serde_json::json!({ "query": "MutationEngine" });
        for (method, expected) in [
            ("code_search", "code_search"),
            ("code_context", "code_context"),
            ("code_index", "code_index"),
        ] {
            let (actual, forwarded) =
                map_schema_method_to_tool(method, &args).expect("mapping must exist");
            assert_eq!(actual, expected);
            assert_eq!(forwarded, args);
        }
    }

    #[test]
    fn rust_pro_routes_to_the_eager_agent_operation() {
        let (tool, args) = map_schema_method_to_tool(
            "rust_pro",
            &serde_json::json!({
                "operation": "check",
                "path": "/srv/git/odbus",
                "release": false
            }),
        )
        .expect("Rust Pro mapping must exist");
        assert_eq!(tool, "agent_rust_pro_check");
        assert_eq!(args["path"], "/srv/git/odbus");
        assert!(args.get("release").is_none());

        assert!(
            map_schema_method_to_tool(
                "rust_pro",
                &serde_json::json!({"operation": "shell", "path": "/tmp"})
            )
            .is_err()
        );
    }

    #[test]
    fn memory_methods_select_the_canonical_memory_operation() {
        let (tool, args) = map_schema_method_to_tool(
            "memory_store",
            &serde_json::json!({ "content": "remember this" }),
        )
        .expect("mapping must exist");
        assert_eq!(tool, "cognitive_memory");
        assert_eq!(args["operation"], "store");
    }

    #[test]
    fn hot_agents_map_to_their_registered_executor_tools() {
        let (tool, args) = map_schema_method_to_tool(
            "python_pro",
            &serde_json::json!({"operation": "test", "path": "/srv/git/x", "args": ["-k", "smoke"]}),
        )
        .unwrap();
        assert_eq!(tool, "agent_python_pro_test");
        assert_eq!(args["path"], "/srv/git/x");
        assert_eq!(args["args"], serde_json::json!(["-k", "smoke"]));

        let (tool, args) = map_schema_method_to_tool(
            "sequential_thinking",
            &serde_json::json!({"operation": "run_workflow", "context": "plan the migration"}),
        )
        .unwrap();
        assert_eq!(tool, "agent_sequential_thinking_run_workflow");
        assert_eq!(args["context"], "plan the migration");

        // Operations outside the agent's catalog are refused; static advisor
        // personas are not registry tools any more (see `specialist`).
        assert!(
            map_schema_method_to_tool(
                "python_pro",
                &serde_json::json!({"operation": "clippy", "path": "/x"})
            )
            .is_err()
        );
        assert!(
            map_schema_method_to_tool(
                "dbus_expert",
                &serde_json::json!({"operation": "explain", "args": "q"})
            )
            .is_err()
        );
        let (tool, args) = map_schema_method_to_tool(
            "memory_semantic_query",
            &serde_json::json!({"query": "what did we decide about ledger ids?"}),
        )
        .unwrap();
        assert_eq!(tool, "cognitive_memory");
        assert_eq!(args["operation"], "semantic_query");
        assert_eq!(args["query"], "what did we decide about ledger ids?");
    }

    #[test]
    fn invoke_tool_uses_the_named_registry_target_and_nested_arguments() {
        let nested = serde_json::json!({"filter": {"limit": 2}});
        let (tool, args) = map_schema_method_to_tool(
            "invoke_tool",
            &serde_json::json!({"tool_name": "safe_read", "arguments": nested}),
        )
        .expect("canonical invoke_tool mapping must exist");
        assert_eq!(tool, "safe_read");
        assert_eq!(args, serde_json::json!({"filter": {"limit": 2}}));
    }

    #[test]
    fn retired_standalone_management_methods_have_no_dispatch_path() {
        for method in ["get_config", "set_config", "restart_service"] {
            let error = map_schema_method_to_tool(method, &serde_json::json!({}))
                .expect_err("retired standalone management must not reach a handler");
            assert!(error.to_string().contains("has no tool-registry mapping"));
        }
    }
}

#[cfg(test)]
mod private_method_audit_tests {
    use super::audit_method_arguments;

    #[test]
    fn elevation_audit_contains_no_password_verifier() {
        let first = r#"{"plugin":"tched_router","method":"config_set","args":{"path":"agents.control_plane.model_provider","value":"custom.route"},"password":"first-password"}"#;
        let second = first.replace("first-password", "different-password");
        let audit = audit_method_arguments("elevation", "authorize_task", first, None);
        assert_eq!(
            audit,
            audit_method_arguments("elevation", "authorize_task", &second, None)
        );
        assert_eq!(audit, r#"{"arguments_redacted":true}"#);
        assert!(!audit.contains("request_hash"));
    }

    #[test]
    fn nested_embedding_text_and_mcp_credentials_are_digest_only() {
        for (plugin, method, input) in [
            (
                "embedding_model",
                "embed",
                r#"{"input":["private episode"]}"#,
            ),
            (
                "mcp",
                "configure_server",
                r#"{"operations":[{"value":"private credential"}]}"#,
            ),
            (
                "cognitive_mcp",
                "agent_backend_architect_consult",
                r#"{"question":"private code"}"#,
            ),
            ("persona", "execute", r#"{"query":"private code"}"#),
            (
                "workflows",
                "start_workflow",
                r#"{"workflow_id":"deploy","params":{"steps":["agent.operation|{private credentials}"]}}"#,
            ),
            ("tched_router", "Chat", r#"{"message":"private message"}"#),
        ] {
            let audit = audit_method_arguments(plugin, method, input, None);
            assert!(audit.contains("arguments_redacted"));
            assert!(audit.contains("request_hash"));
            assert!(!audit.contains("private"));
        }
    }

    #[test]
    fn chatbot_reasoning_arguments_are_digest_only() {
        let private = r#"{"episode":{"reasoning_summary":"never-in-audit","goal_text":"secret"}}"#;
        let audit =
            audit_method_arguments("ctl_plane_chatbot", "classify_significance", private, None);
        assert!(audit.contains("arguments_redacted"));
        assert!(audit.contains("request_hash"));
        assert!(!audit.contains("never-in-audit"));
        assert!(!audit.contains("secret"));
        assert_eq!(
            audit,
            audit_method_arguments("ctl_plane_chatbot", "set_config", private, None)
        );
    }

    #[test]
    fn ordinary_public_arguments_remain_auditable() {
        let input = r#"{"selector":"public"}"#;
        assert_eq!(
            audit_method_arguments("procfs", "get_memory", input, None),
            input
        );
    }

    #[test]
    fn schema_private_fields_become_blake3_digests_in_audit() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "outcome": {"type": "string"},
                "goal_text": {"type": "string", "x-opdbus-private": true}
            }
        });
        let input = r#"{"outcome":"ok","goal_text":"secret prompt"}"#;
        let audit = audit_method_arguments(
            "ctl_plane_chatbot",
            "classify_significance",
            input,
            Some(&schema),
        );
        assert!(audit.contains("\"outcome\":\"ok\""));
        assert!(audit.contains("blake3:"));
        assert!(!audit.contains("secret prompt"));
        assert!(!audit.contains("arguments_redacted"));
    }
}

#[cfg(test)]
mod identity_redaction_tests {
    use super::{public_plugin_value, redact_identity_method_credentials};

    #[test]
    fn credential_handoff_returns_only_the_verified_sessions_credential() {
        let original = serde_json::json!({
            "identity":{"session_id":"mine", "sealed_id":"sid1:owned",
                "nested":{"sealed_id":"sid1:other"}},
            "sealed_id":"sid1:root", "other":{"sealed_id":"sid1:elsewhere"}
        });
        let mut own = original.clone();
        redact_identity_method_credentials("get_identity", Some("mine"), &mut own);
        assert_eq!(own["identity"]["sealed_id"], "sid1:owned");
        assert!(own["identity"]["nested"].get("sealed_id").is_none());
        assert!(own.get("sealed_id").is_none());
        assert!(own["other"].get("sealed_id").is_none());
        for (method, session) in [
            ("get_identity", Some("someone-else")),
            ("get_identity", None),
            ("get_identity", Some("")),
            ("write_identity", Some("mine")),
            ("list_identities", Some("mine")),
        ] {
            let mut result = original.clone();
            redact_identity_method_credentials(method, session, &mut result);
            assert!(!result.to_string().contains("sealed_id"));
        }
    }

    #[test]
    fn public_identity_value_is_recursive_and_other_plugins_are_unchanged() {
        let state = simd_json::json!({
            "sleds": [{
                "session_id": "one",
                "sealed_id": "sid1:secret",
                "nested": { "sealed_id": "sid1:nested", "keep": true }
            }]
        });
        let public = public_plugin_value("identity_sled", &state);
        let encoded = simd_json::to_string(&public).unwrap();
        assert!(!encoded.contains("sealed_id"));
        assert!(!encoded.contains("sid1:"));
        assert!(encoded.contains("session_id"));
        assert!(encoded.contains("keep"));
        assert_eq!(public_plugin_value("another_plugin", &state), state);
    }
}

#[cfg(test)]
mod own_route_context_tests {
    use super::ensure_own_route_context_unchanged;
    use op_plugins::state_plugins::project::{ChatMode, ResolvedChatContext};

    fn context() -> ResolvedChatContext {
        ResolvedChatContext {
            mode: ChatMode::ControlPlane,
            session_id: "own-session".into(),
            project_id: None,
            memory_namespace: "session/own-session/control_plane".into(),
            read_only: false,
            effective_capabilities: ["chat.control_plane".to_owned()].into_iter().collect(),
            allowed_agents: Default::default(),
            selected_agent: None,
        }
    }

    #[test]
    fn own_route_context_rechecks_session_mode_and_revoked_context_grants() {
        let before = context();
        assert!(ensure_own_route_context_unchanged(&before, &context()).is_ok());
        let mut after = context();
        after.mode = ChatMode::Personal;
        assert!(ensure_own_route_context_unchanged(&before, &after).is_err());
        let mut after = context();
        after.session_id = "foreign-session".into();
        assert!(ensure_own_route_context_unchanged(&before, &after).is_err());
        let mut after = context();
        after.effective_capabilities.clear();
        assert!(ensure_own_route_context_unchanged(&before, &after).is_err());
    }

    #[test]
    fn bound_oracle_result_redaction_withholds_authored_credentials_recursively() {
        let mut result = serde_json::json!({"identity":{"sealed_id":"sid1:private", "session_id":"own"},
            "nested":[{"sealed_id":"sid1:other"}]});
        super::redact_identity_credentials_json(&mut result);
        assert!(!result.to_string().contains("sid1:"));
        assert_eq!(result["identity"]["session_id"], "own");
    }
}
