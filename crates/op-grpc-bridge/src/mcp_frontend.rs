//! Bridge-owned MCP Streamable HTTP frontend.
//!
//! This module contributes an Axum `/mcp` route to the existing tonic TLS
//! acceptor. It does not bind a socket and it never dispatches over HTTP: tool
//! discovery and execution use the same `MutationEngine` and in-process
//! cognitive tool registry as the generated plugin routes.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::convert::Infallible;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::State;
use axum::http::{HeaderMap, Request, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use base64::Engine as _;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::{RwLock, broadcast, mpsc};
use tonic::transport::server::{TcpConnectInfo, TlsConnectInfo};

use crate::grpc_server::DECLARED_CAPABILITY_HEADER;
#[cfg(test)]
use crate::interceptor::capabilities_for_principal;
use crate::interceptor::load_capability_grants;
use crate::mcp_policy::{
    HOT_TOOL_NAMES, McpPolicyOwner, McpProjectionPolicy, ToolTemperature, ToolsetDefinition,
};
use crate::mutation_engine::MutationEngine;
use crate::oracle_assertion::AssertionValidator;
use crate::sealed_schema_reader::{
    blob_catalog_dir_from_env, manifest_plugins, read_manifest_pinned_catalog,
    read_manifest_pinned_plugin_schema, valid_plugin_id,
};
#[cfg(test)]
use crate::sealed_schema_reader::{read_oscal_subids_result, read_sealed_schema_result};

pub const MCP_PATH: &str = "/mcp";
pub const MCP_PROTOCOL_VERSION: &str = "2026-07-28";
/// Official MCP spec dates this frontend can speak, plus the local canonical
/// revision. Codex's rmcp client sends `2025-06-18` and will disconnect if
/// initialize errors or returns a date it does not know (`2026-07-28`).
const SUPPORTED_PROTOCOL_VERSIONS: &[&str] = &[
    MCP_PROTOCOL_VERSION,
    "2025-06-18",
    "2025-03-26",
    "2024-11-05",
];
/// Latest official MCP spec date this frontend implements. Used when a client
/// requests an unknown revision so stock SDKs can continue.
const LATEST_OFFICIAL_PROTOCOL_VERSION: &str = "2025-06-18";
pub const MCP_VERSION_HEADER: &str = "mcp-protocol-version";
pub const MCP_SESSION_HEADER: &str = "mcp-session-id";
// Receiver-authored, nonsecret claims for the existing opaque SID1 verifier.
// These are response metadata, never client authority or MCP conversation IDs.
pub const VERIFIED_PRINCIPAL_HEADER: &str = "x-opdbus-verified-principal";
pub const VERIFIED_SESSION_HEADER: &str = "x-opdbus-verified-session";
pub const VERIFIED_GENESIS_HEADER: &str = "x-opdbus-verified-genesis";
pub const MCP_METHOD_HEADER: &str = "mcp-method";
pub const MCP_NAME_HEADER: &str = "mcp-name";
const COMPACT_MODE_QUERY: &str = "opdbus_mode=compact";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum McpView {
    Typed,
    Compact,
}

/// The `/mcp` query: the compact view flag and, for HTTP+SSE (2024-11-05)
/// clients, the `sessionId` of the endpoint their event stream announced.
#[derive(Debug, Default)]
struct McpQuery {
    view: Option<McpView>,
    sse_session: Option<String>,
}

fn parse_query(query: Option<&str>) -> Result<McpQuery, &'static str> {
    let mut parsed = McpQuery::default();
    for pair in query
        .unwrap_or_default()
        .split('&')
        .filter(|pair| !pair.is_empty())
    {
        match pair.split_once('=') {
            _ if pair == COMPACT_MODE_QUERY && parsed.view.is_none() => {
                parsed.view = Some(McpView::Compact);
            }
            Some(("sessionId", id))
                if parsed.sse_session.is_none() && uuid::Uuid::parse_str(id).is_ok() =>
            {
                parsed.sse_session = Some(id.to_string());
            }
            _ => return Err("unsupported MCP query"),
        }
    }
    Ok(parsed)
}

/// Raw HTTP uses the same canonical header name as gRPC metadata. Its value is
/// the OIA1 wire envelope encoded as unpadded canonical base64url.
pub const HTTP_ASSERTION_HEADER: &str = "x-oracle-identity-assertion-bin";
/// MutationEngine-authored SID1 envelope read from the caller's selected sled.
/// The HTTP spelling is canonical unpadded base64url; the bridge reconstructs
/// the inline `sid1:` value and exact-matches it against the authoritative
/// session record before accepting any claims.
pub const HTTP_SEALED_ID_HEADER: &str = op_identity::sealed_id::HTTP_HEADER_NAME;

const MAX_BODY_BYTES: usize = 1024 * 1024;
const MAX_ASSERTION_BYTES: usize = 16 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(90);
// Agent executors have their own bounded process deadlines (up to one hour).
// Keep the public RPC alive long enough to return that real terminal outcome.
const AGENT_REQUEST_TIMEOUT: Duration = Duration::from_secs(3660);
const NOTEBOOKLM_AUTH_REQUEST_TIMEOUT: Duration = Duration::from_secs(660);
const DEFAULT_PAGE_SIZE: usize = 100;
const MAX_PAGE_SIZE: usize = 500;
const MAX_ACTIVE_MCP_SESSIONS: usize = 4096;
const MCP_SESSION_MAX_AGE: Duration = Duration::from_secs(24 * 60 * 60);
const ALLOWED_ORIGINS_ENV: &str = "OP_MCP_ALLOWED_ORIGINS";
const RUST_PRO_CODEBASE_ENV: &str = "OP_MCP_RUST_PRO_CODEBASE";
const DEFAULT_RUST_PRO_CODEBASE: &str = "/srv/git/odbus";

#[derive(Clone, Debug)]
pub(crate) struct AuthenticatedCaller {
    principal_id: String,
    session_id: String,
    session_genesis: String,
    /// Receiver-derived scope of an external WireGuard caller, never HTTP Origin.
    wireguard_origin: Option<String>,
}

impl AuthenticatedCaller {
    pub(crate) fn identity_claims(&self) -> (&str, &str, &str, Option<&str>) {
        (
            &self.principal_id,
            &self.session_id,
            &self.session_genesis,
            self.wireguard_origin.as_deref(),
        )
    }
}

#[cfg(test)]
pub(crate) async fn authenticate_dbus_fixture(
    engine: Arc<MutationEngine>,
    validator: Arc<AssertionValidator>,
    headers: &HeaderMap,
) -> Result<AuthenticatedCaller, String> {
    OracleHttpAuthenticator { engine, validator }
        .authenticate(headers, None)
        .await
        .map_err(|e| e.0)
}

#[derive(Debug)]
struct McpAuthError(String);

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct ToolsetSelection {
    id: String,
    generation: u64,
}

#[derive(Debug)]
struct McpSession {
    principal_id: String,
    identity_session_id: String,
    session_genesis: String,
    wireguard_origin: Option<String>,
    view: McpView,
    selection: RwLock<Option<ToolsetSelection>>,
    events: broadcast::Sender<String>,
    event_stream_active: Arc<AtomicBool>,
    /// HTTP+SSE sessions answer on their event stream, not the POST body.
    sse_replies: Option<mpsc::Sender<String>>,
    created_at: Instant,
}

impl McpSession {
    fn new(caller: &AuthenticatedCaller, view: McpView) -> Self {
        let (events, _) = broadcast::channel(16);
        Self {
            principal_id: caller.principal_id.clone(),
            identity_session_id: caller.session_id.clone(),
            session_genesis: caller.session_genesis.clone(),
            wireguard_origin: caller.wireguard_origin.clone(),
            view,
            selection: RwLock::new(None),
            events,
            event_stream_active: Arc::new(AtomicBool::new(false)),
            sse_replies: None,
            created_at: Instant::now(),
        }
    }

    fn with_sse_replies(
        caller: &AuthenticatedCaller,
        view: McpView,
        replies: mpsc::Sender<String>,
    ) -> Self {
        Self {
            sse_replies: Some(replies),
            event_stream_active: Arc::new(AtomicBool::new(true)),
            ..Self::new(caller, view)
        }
    }

    fn belongs_to(&self, caller: &AuthenticatedCaller) -> bool {
        self.principal_id == caller.principal_id
            && self.identity_session_id == caller.session_id
            && self.session_genesis == caller.session_genesis
            && self.wireguard_origin == caller.wireguard_origin
    }

    fn caller(&self) -> AuthenticatedCaller {
        AuthenticatedCaller {
            principal_id: self.principal_id.clone(),
            session_id: self.identity_session_id.clone(),
            session_genesis: self.session_genesis.clone(),
            wireguard_origin: self.wireguard_origin.clone(),
        }
    }
}

struct EventStreamLease(Arc<AtomicBool>);

impl Drop for EventStreamLease {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

#[async_trait]
trait McpAuthenticator: Send + Sync {
    async fn authenticate(
        &self,
        headers: &HeaderMap,
        peer: Option<SocketAddr>,
    ) -> Result<AuthenticatedCaller, McpAuthError>;
}

#[derive(Clone)]
struct OracleHttpAuthenticator {
    validator: Arc<AssertionValidator>,
    engine: Arc<MutationEngine>,
}

#[async_trait]
impl McpAuthenticator for OracleHttpAuthenticator {
    async fn authenticate(
        &self,
        headers: &HeaderMap,
        peer: Option<SocketAddr>,
    ) -> Result<AuthenticatedCaller, McpAuthError> {
        let assertion =
            one_optional_header(headers, HTTP_ASSERTION_HEADER).map_err(McpAuthError)?;
        let sealed_id =
            one_optional_header(headers, HTTP_SEALED_ID_HEADER).map_err(McpAuthError)?;
        match (assertion, sealed_id) {
            (Some(_), Some(_)) => Err(McpAuthError(
                "send exactly one identity credential, not both OIA1 and SID1".into(),
            )),
            (Some(encoded), None) => self.authenticate_assertion(encoded, peer).await,
            (None, Some(encoded)) => self.authenticate_sealed_id(encoded).await,
            (None, None) => Err(McpAuthError(format!(
                "missing {HTTP_ASSERTION_HEADER} or {HTTP_SEALED_ID_HEADER} header"
            ))),
        }
    }
}

impl OracleHttpAuthenticator {
    async fn authenticate_assertion(
        &self,
        encoded: &str,
        peer: Option<SocketAddr>,
    ) -> Result<AuthenticatedCaller, McpAuthError> {
        if encoded.len() > encoded_len_upper_bound(MAX_ASSERTION_BYTES) {
            return Err(McpAuthError(
                "Oracle identity assertion is too large".into(),
            ));
        }
        let wire = decode_http_assertion(encoded)?;

        let mut pending = self
            .validator
            .validate_pending_for_mcp(&wire, peer, chrono::Utc::now().timestamp())
            .map_err(|error| McpAuthError(error.to_string()))?;
        // External humans have a durable, origin-scoped audit session, without
        // a container, SID1, or browser-to-chatbot identity assignment. Preserve
        // nonce consumption after successful durable admission.
        let wireguard_origin = pending.verified_origin().origin();
        let session =
            crate::external_mcp_session::anchor(self.engine.as_ref(), pending.verified_origin())
                .await
                .map_err(|error| McpAuthError(error.to_string()))?;
        pending.identity_mut().session_id = session.session_id.clone();
        pending.identity_mut().session_genesis = session.genesis_hex.clone();
        let identity = self
            .validator
            .consume_pending(pending)
            .map_err(|error| McpAuthError(error.to_string()))?;
        self.engine.register_external_mcp_context(session).await;
        Ok(AuthenticatedCaller {
            principal_id: identity.principal_id,
            session_id: identity.session_id,
            session_genesis: identity.session_genesis,
            wireguard_origin: Some(wireguard_origin),
        })
    }

    async fn authenticate_sealed_id(
        &self,
        encoded: &str,
    ) -> Result<AuthenticatedCaller, McpAuthError> {
        if encoded.len() > encoded_len_upper_bound(MAX_ASSERTION_BYTES) {
            return Err(McpAuthError("sealed identity is too large".into()));
        }
        let wire = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(encoded.as_bytes())
            .map_err(|_| McpAuthError("malformed sealed identity".into()))?;
        if wire.len() > MAX_ASSERTION_BYTES
            || base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&wire) != encoded
        {
            return Err(McpAuthError(
                "sealed identity is not canonical base64url".into(),
            ));
        }
        let claims = crate::identity_sled_dispatch::authenticate_sid1_wire(
            self.engine.as_ref(),
            &wire,
            "mcp",
        )
        .await
        .map_err(McpAuthError)?;

        // A local sled can exist without MCP authority.  Exact principal-only
        // grants remain the final admission source; no sealed ID/genesis/hash is
        // ever looked up as a grant key.
        if load_exact_capability_grants(&claims.principal_id).is_empty() {
            return Err(McpAuthError(
                "sealed identity principal has no MCP capabilities".into(),
            ));
        }
        match crate::human_principal_dispatch::resolve_key_for_assertion(&claims.wireguard_pubkey)
            .await
        {
            Ok(record) => validate_registered_sealed_id_principal(
                record.as_ref(),
                &claims.principal_id,
                &claims.wireguard_pubkey,
            )?,
            Err(error) => {
                return Err(McpAuthError(format!(
                    "principal registry unavailable: {error:?}"
                )));
            }
        }

        Ok(AuthenticatedCaller {
            principal_id: claims.principal_id,
            session_id: claims.session_id,
            session_genesis: claims.session_genesis,
            wireguard_origin: None,
        })
    }
}

/// SID1 is a possession credential for an already-registered principal, not
/// a registration mechanism.  Keep this check independent and testable so a
/// missing row can never drift back into a fail-open wildcard match.
fn validate_registered_sealed_id_principal(
    record: Option<&op_cozo_store::HumanPrincipalRecord>,
    expected_principal_id: &str,
    expected_wireguard_pubkey: &str,
) -> Result<(), McpAuthError> {
    let record =
        record.ok_or_else(|| McpAuthError("sealed identity principal is not registered".into()))?;
    if record.revoked_at != 0 {
        return Err(McpAuthError("sealed identity principal is revoked".into()));
    }
    if record.principal_id != expected_principal_id
        || record.human_pubkey != expected_wireguard_pubkey
    {
        return Err(McpAuthError(
            "sealed identity principal registry binding changed".into(),
        ));
    }
    Ok(())
}

#[async_trait]
trait McpBackend: Send + Sync {
    async fn list_tools(
        &self,
        caller: &AuthenticatedCaller,
        selection: Option<&ToolsetSelection>,
    ) -> anyhow::Result<Vec<Value>>;

    async fn call_tool(
        &self,
        caller: &AuthenticatedCaller,
        name: &str,
        arguments: Value,
        selection: Option<&ToolsetSelection>,
    ) -> anyhow::Result<Value>;

    async fn list_resources(&self, caller: &AuthenticatedCaller) -> anyhow::Result<Vec<Value>>;
    async fn read_resource(&self, caller: &AuthenticatedCaller, uri: &str)
    -> anyhow::Result<Value>;

    async fn session_closed(&self, _caller: &AuthenticatedCaller) -> anyhow::Result<()> {
        Ok(())
    }
}

#[derive(Clone)]
struct MutationEngineMcpBackend {
    engine: Arc<MutationEngine>,
    blob_catalog_dir: PathBuf,
    policy: Arc<McpPolicyOwner>,
    close_check_active: Arc<AtomicBool>,
    #[cfg(test)]
    #[allow(clippy::type_complexity)]
    test_grants: Option<Arc<std::sync::RwLock<HashMap<String, HashSet<String>>>>>,
}

/// Common canonical result for every transport, including identity-bound FUSE.
/// The installed policy owner and freshly loaded principal grants remain the
/// authority; selection returns a stateless selector, never a persistent grant.
pub(crate) async fn canonical_toolsets_result(
    engine: &MutationEngine,
    principal_id: &str,
    arguments: &Value,
) -> anyhow::Result<Value> {
    let backend =
        MutationEngineMcpBackend::new(Arc::new(engine.clone()), engine.mcp_policy_owner()?);
    let grants = backend.current_grants(principal_id);
    backend.toolsets_result(&grants, arguments).await
}

impl MutationEngineMcpBackend {
    fn new(engine: Arc<MutationEngine>, policy: Arc<McpPolicyOwner>) -> Self {
        let blob_catalog_dir = blob_catalog_dir_from_env();
        Self {
            engine,
            blob_catalog_dir,
            policy,
            close_check_active: Arc::new(AtomicBool::new(false)),
            #[cfg(test)]
            test_grants: None,
        }
    }

    fn current_grants(&self, principal_id: &str) -> HashSet<String> {
        #[cfg(test)]
        if let Some(grants) = &self.test_grants {
            return grants
                .read()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .get(principal_id)
                .cloned()
                .unwrap_or_default();
        }
        load_exact_capability_grants(principal_id)
    }

    fn authorize(
        &self,
        caller: &AuthenticatedCaller,
        required_capability: &str,
    ) -> anyhow::Result<()> {
        let grants = self.current_grants(&caller.principal_id);
        authorize_with_grants(&grants, required_capability)
    }

    async fn dispatch_plugin(
        &self,
        caller: &AuthenticatedCaller,
        plugin_id: &str,
        method: &str,
        arguments: Value,
        required_capability: &str,
    ) -> anyhow::Result<Value> {
        self.authorize(caller, required_capability)?;
        // Stable principal identity is the audit actor. Session identity stays
        // in the authenticated request context and never becomes authority.
        let result = crate::mcp_dbus_dispatch::dispatch(
            self.engine.clone(),
            caller.clone(),
            plugin_id,
            method,
            arguments,
        )
        .await?;
        Ok(result.get("result").cloned().unwrap_or(result))
    }

    async fn cognitive_admission(
        &self,
        caller: &AuthenticatedCaller,
        method: &str,
        arguments: Value,
        required_capability: &str,
    ) -> anyhow::Result<Value> {
        self.dispatch_plugin(
            caller,
            "cognitive_mcp",
            method,
            arguments,
            required_capability,
        )
        .await
    }

    fn provider_ready(set: &ToolsetDefinition) -> bool {
        !set.requires_provider_health
            || Path::new("/run/opdbus/runit-ready")
                .join(&set.provider)
                .is_file()
    }

    async fn typed_tool_descriptor(&self, public_name: &str) -> anyhow::Result<Value> {
        let (plugin_id, method_name) = parse_typed_tool_name(public_name)?;
        let dir = self.blob_catalog_dir.clone();
        let plugin_id_owned = plugin_id.to_string();
        let schema = tokio::task::spawn_blocking(move || {
            read_manifest_pinned_plugin_schema(&dir, &plugin_id_owned)
        })
        .await
        .map_err(|error| anyhow::anyhow!("sealed schema task failed: {error}"))??;
        method_descriptor(&schema, method_name, public_name)
    }

    async fn typed_tool_descriptor_cached(
        &self,
        public_name: &str,
        schemas: &mut HashMap<String, op_state_store::PluginSchema>,
    ) -> anyhow::Result<Value> {
        let (plugin_id, method_name) = parse_typed_tool_name(public_name)?;
        if !schemas.contains_key(plugin_id) {
            let dir = self.blob_catalog_dir.clone();
            let plugin_id_owned = plugin_id.to_string();
            let schema = tokio::task::spawn_blocking(move || {
                read_manifest_pinned_plugin_schema(&dir, &plugin_id_owned)
            })
            .await
            .map_err(|error| anyhow::anyhow!("sealed schema task failed: {error}"))??;
            schemas.insert(plugin_id.to_string(), schema);
        }
        let schema = schemas
            .get(plugin_id)
            .expect("cached plugin schema inserted above");
        method_descriptor(schema, method_name, public_name)
    }

    fn hot_tool_descriptor(public_name: &str) -> anyhow::Result<Value> {
        let schema = op_plugins::cognitive_mcp_plugin_schema();
        method_descriptor(&schema, public_name, public_name)
    }

    async fn authorized_hot_tools(&self, grants: &HashSet<String>) -> anyhow::Result<Vec<Value>> {
        let mut tools = Vec::with_capacity(HOT_TOOL_NAMES.len());
        for name in HOT_TOOL_NAMES {
            let descriptor = Self::hot_tool_descriptor(name)?;
            if descriptor_authority(&descriptor, Some(name))
                .is_ok_and(|(capability, _)| grants.contains(capability))
            {
                tools.push(descriptor);
            }
        }
        Ok(tools)
    }

    /// The singleton Control Plane uses the exact manifest-pinned typed
    /// surface admitted by its current principal grants. Broker toolsets and
    /// provider health remain projection policy for other identities only.
    async fn authorized_singleton_tools(
        &self,
        caller: &AuthenticatedCaller,
    ) -> anyhow::Result<Vec<Value>> {
        let grants = self.current_grants(&caller.principal_id);
        let policy = self.policy.snapshot();
        let dir = self.blob_catalog_dir.clone();
        let catalog = tokio::task::spawn_blocking(move || read_manifest_pinned_catalog(&dir))
            .await
            .map_err(|error| anyhow::anyhow!("sealed schema task failed: {error}"))??;
        let mut by_name = BTreeMap::new();
        for (plugin_id, document) in catalog.plugins {
            for method_name in document.schema.methods.keys() {
                let name = format!("plugin.{plugin_id}.{method_name}");
                if policy.denies_singleton_chatbot_direct_tool(&caller.principal_id, &name) {
                    continue;
                }
                let Ok(descriptor) = method_descriptor(&document.schema, method_name, &name) else {
                    // A missing capability or schema-closure gap is not an
                    // executable operation, even if a grant document names it.
                    continue;
                };
                let Ok((capability, _)) = descriptor_authority(&descriptor, Some(&name)) else {
                    continue;
                };
                if capability != "cognitive_mcp.invoke" && grants.contains(capability) {
                    by_name.insert(name, descriptor);
                }
            }
        }
        Ok(by_name.into_values().collect())
    }

    async fn authorized_singleton_tool(
        &self,
        caller: &AuthenticatedCaller,
        name: &str,
    ) -> anyhow::Result<Value> {
        let policy = self.policy.snapshot();
        if policy.denies_singleton_chatbot_direct_tool(&caller.principal_id, name) {
            anyhow::bail!(
                "AccessDenied: singleton chatbot cannot call direct-execution tool '{name}'"
            );
        }
        let denied = || {
            anyhow::anyhow!(
                "AccessDenied: tool '{name}' is not in the singleton principal's granted sealed projection"
            )
        };
        let descriptor = self
            .typed_tool_descriptor(name)
            .await
            .map_err(|_| denied())?;
        let (capability, _) =
            descriptor_authority(&descriptor, Some(name)).map_err(|_| denied())?;
        if capability == "cognitive_mcp.invoke" {
            return Err(denied());
        }
        authorize_with_grants(&self.current_grants(&caller.principal_id), capability)
            .map_err(|_| denied())?;
        Ok(descriptor)
    }

    async fn authorized_toolset_tools(
        &self,
        set: &ToolsetDefinition,
        grants: &HashSet<String>,
        schemas: &mut HashMap<String, op_state_store::PluginSchema>,
    ) -> anyhow::Result<Vec<Value>> {
        if !Self::provider_ready(set) {
            anyhow::bail!("provider_unavailable: {}", set.provider);
        }
        let mut tools = Vec::with_capacity(set.tools.len());
        for name in &set.tools {
            let descriptor = self.typed_tool_descriptor_cached(name, schemas).await?;
            if descriptor_authority(&descriptor, Some(name))
                .is_ok_and(|(capability, _)| grants.contains(capability))
            {
                tools.push(descriptor);
            }
        }
        Ok(tools)
    }

    fn selected_set<'a>(
        policy: &'a McpProjectionPolicy,
        selection: &ToolsetSelection,
    ) -> anyhow::Result<&'a ToolsetDefinition> {
        if selection.generation != policy.toolsets.generation {
            anyhow::bail!(
                "toolset_generation_changed: requested {}, current {}; relist required",
                selection.generation,
                policy.toolsets.generation
            );
        }
        policy
            .toolset(&selection.id)
            .ok_or_else(|| anyhow::anyhow!("unknown toolset '{}'", selection.id))
    }

    async fn selected_toolset_tools(
        &self,
        caller: &AuthenticatedCaller,
        selection: Option<&ToolsetSelection>,
    ) -> anyhow::Result<Vec<Value>> {
        let grants = self.current_grants(&caller.principal_id);
        let policy = self.policy.snapshot();
        let mut by_name = BTreeMap::new();
        let mut schemas = HashMap::new();
        for descriptor in self.authorized_hot_tools(&grants).await? {
            by_name.insert(tool_name(&descriptor).to_string(), descriptor);
        }
        for set in policy
            .toolsets
            .sets
            .iter()
            .filter(|set| set.temperature == ToolTemperature::Hot && Self::provider_ready(set))
        {
            for descriptor in self
                .authorized_toolset_tools(set, &grants, &mut schemas)
                .await?
            {
                by_name.insert(tool_name(&descriptor).to_string(), descriptor);
            }
        }
        if let Some(selection) = selection {
            let set = Self::selected_set(&policy, selection)?;
            for descriptor in self
                .authorized_toolset_tools(set, &grants, &mut schemas)
                .await?
            {
                by_name.insert(tool_name(&descriptor).to_string(), descriptor);
            }
        }
        Ok(by_name.into_values().collect())
    }

    /// Return the complete set of broker-curated toolset tools that this caller can
    /// actually use. MCP clients cache the result of their startup
    /// `tools/list`, and not every client re-lists after receiving
    /// `notifications/tools/list_changed`. Keep the tools bounded by the
    /// reviewed toolset manifest while exposing real typed methods up front;
    /// `toolsets(select)` remains the execution-admission boundary.
    async fn discoverable_toolset_tools(
        &self,
        caller: &AuthenticatedCaller,
    ) -> anyhow::Result<Vec<Value>> {
        let grants = self.current_grants(&caller.principal_id);
        let policy = self.policy.snapshot();
        let mut by_name = BTreeMap::new();
        let mut schemas = HashMap::new();
        for descriptor in self.authorized_hot_tools(&grants).await? {
            by_name.insert(tool_name(&descriptor).to_string(), descriptor);
        }
        for set in &policy.toolsets.sets {
            if !Self::provider_ready(set) {
                continue;
            }
            for descriptor in self
                .authorized_toolset_tools(set, &grants, &mut schemas)
                .await?
            {
                by_name.insert(tool_name(&descriptor).to_string(), descriptor);
            }
        }
        Ok(by_name.into_values().collect())
    }

    async fn toolsets_result(
        &self,
        grants: &HashSet<String>,
        arguments: &Value,
    ) -> anyhow::Result<Value> {
        let policy = self.policy.snapshot();
        let mut schemas = HashMap::new();
        let operation = arguments
            .get("operation")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("toolsets requires operation"))?;
        let mut projected = Vec::new();
        for set in &policy.toolsets.sets {
            let authorized_count = self
                .authorized_toolset_tools_without_health(set, grants, &mut schemas)
                .await?
                .len();
            if authorized_count == 0 {
                continue;
            }
            projected.push(json!({
                "id": set.id,
                "temperature": set.temperature,
                "provider": set.provider,
                "available": Self::provider_ready(set),
                "authorizedToolCount": authorized_count
            }));
        }

        match operation {
            "list" => Ok(json!({
                "operation": "list",
                "toolset_generation": policy.toolsets.generation,
                "relist_required": false,
                "result": {"sets": projected}
            })),
            "select" => {
                let id = arguments
                    .get("toolset_id")
                    .and_then(Value::as_str)
                    .ok_or_else(|| anyhow::anyhow!("toolsets select requires toolset_id"))?;
                let set = policy
                    .toolset(id)
                    .ok_or_else(|| anyhow::anyhow!("unknown toolset '{id}'"))?;
                if !Self::provider_ready(set) {
                    anyhow::bail!("provider_unavailable: {}", set.provider);
                }
                let tools = self
                    .authorized_toolset_tools(set, grants, &mut schemas)
                    .await?;
                if tools.is_empty() {
                    anyhow::bail!("AccessDenied: no authorized tools in toolset '{id}'");
                }
                Ok(json!({
                    "operation": "select",
                    "toolset_generation": policy.toolsets.generation,
                    "relist_required": false,
                    "result": {
                        "selector": {
                            "id": id,
                            "generation": policy.toolsets.generation
                        },
                        "selected_set": id,
                        "activated_tool_count": tools.len()
                    }
                }))
            }
            _ => anyhow::bail!("toolsets operation must be list or select"),
        }
    }

    async fn authorized_toolset_tools_without_health(
        &self,
        set: &ToolsetDefinition,
        grants: &HashSet<String>,
        schemas: &mut HashMap<String, op_state_store::PluginSchema>,
    ) -> anyhow::Result<Vec<Value>> {
        let mut tools = Vec::with_capacity(set.tools.len());
        for name in &set.tools {
            let descriptor = self.typed_tool_descriptor_cached(name, schemas).await?;
            if descriptor_authority(&descriptor, Some(name))
                .is_ok_and(|(capability, _)| grants.contains(capability))
            {
                tools.push(descriptor);
            }
        }
        Ok(tools)
    }

    async fn dispatch_projected_tool(
        &self,
        caller: &AuthenticatedCaller,
        descriptor: &Value,
        name: &str,
        arguments: Value,
    ) -> anyhow::Result<Value> {
        let grants = self.current_grants(&caller.principal_id);
        authorize_and_validate_tool_call(descriptor, name, &arguments, &grants)?;
        let required_capability = descriptor_authority(descriptor, Some(name))?.0;
        let (plugin_id, method_name) = if HOT_TOOL_NAMES.contains(&name) {
            ("cognitive_mcp", name)
        } else {
            parse_typed_tool_name(name)?
        };
        let admitted = self
            .dispatch_plugin(
                caller,
                plugin_id,
                method_name,
                arguments.clone(),
                required_capability,
            )
            .await?;
        Ok(admitted)
    }
}

#[async_trait]
impl McpBackend for MutationEngineMcpBackend {
    async fn list_tools(
        &self,
        caller: &AuthenticatedCaller,
        _selection: Option<&ToolsetSelection>,
    ) -> anyhow::Result<Vec<Value>> {
        let singleton = self
            .policy
            .snapshot()
            .is_singleton_chatbot(&caller.principal_id);
        self.cognitive_admission(caller, "list_tools", json!({}), "cognitive_mcp.read")
            .await?;
        if singleton {
            return Ok(self
                .authorized_singleton_tools(caller)
                .await?
                .into_iter()
                .map(normalize_tool_definition)
                .collect());
        }
        Ok(self
            .discoverable_toolset_tools(caller)
            .await?
            .into_iter()
            .map(normalize_tool_definition)
            .collect())
    }

    async fn call_tool(
        &self,
        caller: &AuthenticatedCaller,
        name: &str,
        arguments: Value,
        selection: Option<&ToolsetSelection>,
    ) -> anyhow::Result<Value> {
        let policy = self.policy.snapshot();
        if policy.denies_singleton_chatbot_direct_tool(&caller.principal_id, name) {
            anyhow::bail!(
                "AccessDenied: singleton chatbot cannot call direct-execution tool '{name}'"
            );
        }
        if policy.is_singleton_chatbot(&caller.principal_id) {
            let descriptor = self.authorized_singleton_tool(caller, name).await?;
            return self
                .dispatch_projected_tool(caller, &descriptor, name, arguments)
                .await
                .map(normalize_tool_result);
        }
        // Reject calls outside the selected projection before resolving any
        // catalog entries or provider readiness. The later projection still
        // enforces the caller's exact grants and the provider's health.
        if !policy.is_hot_tool(name)
            && let Some(selection) = selection
            && !Self::selected_set(&policy, selection)?
                .tools
                .iter()
                .any(|tool| tool == name)
        {
            anyhow::bail!(
                "AccessDenied: tool '{name}' is not in the current HOT/toolset projection"
            );
        }
        if !policy.is_hot_tool(name) && selection.is_none() {
            let sets: Vec<_> = policy
                .toolsets
                .sets
                .iter()
                .filter(|set| set.tools.iter().any(|tool| tool == name))
                .collect();
            if sets.is_empty() {
                anyhow::bail!(
                    "AccessDenied: tool '{name}' is not exposed by the configured MCP toolsets"
                );
            }
            let descriptor = self.typed_tool_descriptor(name).await?;
            let (capability, _) = descriptor_authority(&descriptor, Some(name))?;
            self.authorize(caller, capability)?;
            let available: Vec<_> = sets
                .iter()
                .filter(|set| Self::provider_ready(set))
                .map(|set| set.id.as_str())
                .collect();
            if available.is_empty() {
                anyhow::bail!("provider_unavailable: no ready toolset exposes '{name}'");
            }
            anyhow::bail!(
                "AccessDenied: select toolset {} before calling '{name}'",
                available.join(" or ")
            );
        }
        let descriptor = self
            .selected_toolset_tools(caller, selection)
            .await?
            .into_iter()
            .find(|tool| tool_name(tool) == name)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "AccessDenied: tool '{name}' is not in the current HOT/toolset projection"
                )
            })?;
        self.dispatch_projected_tool(caller, &descriptor, name, arguments)
            .await
            .map(normalize_tool_result)
    }

    async fn list_resources(&self, caller: &AuthenticatedCaller) -> anyhow::Result<Vec<Value>> {
        self.authorize(caller, "cognitive_mcp.read")?;
        let dir = self.blob_catalog_dir.clone();
        tokio::task::spawn_blocking(move || list_blob_resources(&dir))
            .await
            .map_err(|error| anyhow::anyhow!("blob catalog task failed: {error}"))?
    }

    async fn read_resource(
        &self,
        caller: &AuthenticatedCaller,
        uri: &str,
    ) -> anyhow::Result<Value> {
        self.authorize(caller, "cognitive_mcp.read")?;
        let dir = self.blob_catalog_dir.clone();
        let uri = uri.to_string();
        tokio::task::spawn_blocking(move || read_blob_resource(&dir, &uri))
            .await
            .map_err(|error| anyhow::anyhow!("blob resource task failed: {error}"))?
    }

    async fn session_closed(&self, caller: &AuthenticatedCaller) -> anyhow::Result<()> {
        if self
            .close_check_active
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            tracing::debug!("Rust Pro close check already active; coalescing session trigger");
            return Ok(());
        }

        let codebase = std::env::var(RUST_PRO_CODEBASE_ENV)
            .unwrap_or_else(|_| DEFAULT_RUST_PRO_CODEBASE.to_string());
        let result = self
            .call_tool(
                caller,
                "rust_pro",
                json!({"operation": "check", "path": codebase}),
                None,
            )
            .await;
        self.close_check_active.store(false, Ordering::Release);

        match result {
            Ok(_) => {
                tracing::info!(codebase, "Rust Pro session-close check completed");
                Ok(())
            }
            Err(error) => {
                tracing::warn!(codebase, %error, "Rust Pro session-close check failed");
                Err(error)
            }
        }
    }
}

fn schedule_session_close(backend: Arc<dyn McpBackend>, caller: AuthenticatedCaller) {
    tokio::spawn(async move {
        let _ = backend.session_closed(&caller).await;
    });
}

#[derive(Clone)]
struct McpFrontendState {
    authenticator: Arc<dyn McpAuthenticator>,
    backend: Arc<dyn McpBackend>,
    allowed_origins: Arc<HashSet<String>>,
    sessions: Arc<RwLock<HashMap<String, Arc<McpSession>>>>,
}

/// Build the raw HTTP projection. The returned router owns no listener.
pub fn build_mcp_router(engine: Arc<MutationEngine>, validator: Arc<AssertionValidator>) -> Router {
    let owner = McpPolicyOwner::from_env()
        .expect("protected MCP audience/toolset policy must be valid before binding :8090");
    build_mcp_router_with_owner(engine, validator, owner)
}

/// Build the raw HTTP projection with an already validated policy.
///
/// Production callers should use [`build_mcp_router`], which loads the
/// root-protected policy files from the configured environment.  This explicit
/// form is also used by isolated integration tests so they can provide a
/// deterministic policy without reading host configuration.
pub fn build_mcp_router_with_policy(
    engine: Arc<MutationEngine>,
    validator: Arc<AssertionValidator>,
    policy: McpProjectionPolicy,
) -> Router {
    let owner = McpPolicyOwner::for_fixture(policy);
    build_mcp_router_with_owner(engine, validator, owner)
}

fn build_mcp_router_with_owner(
    engine: Arc<MutationEngine>,
    validator: Arc<AssertionValidator>,
    owner: Arc<McpPolicyOwner>,
) -> Router {
    engine
        .install_mcp_policy_owner(owner.clone())
        .expect("MutationEngine MCP policy owner must be installed once");
    let state = McpFrontendState {
        authenticator: Arc::new(OracleHttpAuthenticator {
            validator,
            engine: engine.clone(),
        }),
        backend: Arc::new(MutationEngineMcpBackend::new(engine, owner)),
        allowed_origins: Arc::new(configured_allowed_origins()),
        sessions: Arc::new(RwLock::new(HashMap::new())),
    };
    router_with_state(state)
}

fn router_with_state(state: McpFrontendState) -> Router {
    Router::new()
        .route(
            MCP_PATH,
            post(handle_mcp)
                .get(handle_mcp_events)
                .delete(handle_mcp_delete),
        )
        .with_state(state)
}

#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    #[serde(default)]
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Value,
}

/// One endpoint for every MCP client, as the reference SDK servers behave:
/// Streamable HTTP (any revision, with or without `MCP-Protocol-Version`),
/// JSON-RPC batches, and HTTP+SSE clients posting to the `?sessionId=`
/// endpoint their GET stream announced. Authentication and admission are the
/// same for all of them.
async fn handle_mcp(State(state): State<McpFrontendState>, request: Request<Body>) -> Response {
    let peer = peer_addr(request.extensions());
    let headers = request.headers().clone();
    if [
        VERIFIED_PRINCIPAL_HEADER,
        VERIFIED_SESSION_HEADER,
        VERIFIED_GENESIS_HEADER,
    ]
    .iter()
    .any(|name| headers.contains_key(*name))
    {
        return jsonrpc_error(
            StatusCode::BAD_REQUEST,
            None,
            -32600,
            "verified identity metadata is receiver-authored and not accepted in requests",
        );
    }
    let query = match parse_query(request.uri().query()) {
        Ok(query) => query,
        Err(message) => return jsonrpc_error(StatusCode::BAD_REQUEST, None, -32600, message),
    };
    if let Err(error) = validate_origin(&headers, &state.allowed_origins) {
        return (
            StatusCode::FORBIDDEN,
            axum::Json(json!({"error": "origin_rejected", "message": error})),
        )
            .into_response();
    }
    let caller = match state.authenticator.authenticate(&headers, peer).await {
        Ok(caller) => caller,
        Err(error) => {
            return (
                StatusCode::UNAUTHORIZED,
                axum::Json(json!({"error": "unauthenticated", "message": error.0})),
            )
                .into_response();
        }
    };

    let bytes = match to_bytes(request.into_body(), MAX_BODY_BYTES).await {
        Ok(bytes) => bytes,
        Err(_) => {
            return jsonrpc_error(
                StatusCode::PAYLOAD_TOO_LARGE,
                None,
                -32600,
                "body too large",
            );
        }
    };
    let message: Value = match serde_json::from_slice(&bytes) {
        Ok(message) => message,
        Err(error) => {
            return jsonrpc_error(
                StatusCode::BAD_REQUEST,
                None,
                -32700,
                &format!("parse error: {error}"),
            );
        }
    };
    let response = match message {
        Value::Array(batch) => handle_batch(&state, &headers, &caller, &query, batch).await,
        message => handle_message(&state, &headers, &caller, &query, message).await,
    };
    match query.sse_session.as_deref() {
        Some(session_id) => reply_on_sse_stream(&state, &caller, session_id, response).await,
        None => response,
    }
}

const MAX_BATCH_MESSAGES: usize = 64;
const SSE_REPLY_QUEUE: usize = 64;
const SSE_REPLY_SEND_TIMEOUT: Duration = Duration::from_secs(5);

/// JSON-RPC batch (2025-03-26 clients). Each member is admitted exactly as a
/// single request; notifications and client replies produce no member.
async fn handle_batch(
    state: &McpFrontendState,
    headers: &HeaderMap,
    caller: &AuthenticatedCaller,
    query: &McpQuery,
    batch: Vec<Value>,
) -> Response {
    if batch.is_empty() || batch.len() > MAX_BATCH_MESSAGES {
        return jsonrpc_error(
            StatusCode::BAD_REQUEST,
            None,
            -32600,
            "a JSON-RPC batch must hold 1 to 64 messages",
        );
    }
    if batch
        .iter()
        .any(|message| message.get("method").and_then(Value::as_str) == Some("initialize"))
    {
        return jsonrpc_error(
            StatusCode::BAD_REQUEST,
            None,
            -32600,
            "initialize must not be part of a JSON-RPC batch",
        );
    }
    let mut replies = Vec::new();
    for message in batch {
        let response = handle_message(state, headers, caller, query, message).await;
        if let Some(reply) = jsonrpc_body(response).await {
            replies.push(reply);
        }
    }
    if replies.is_empty() {
        StatusCode::ACCEPTED.into_response()
    } else {
        (StatusCode::OK, axum::Json(Value::Array(replies))).into_response()
    }
}

/// The JSON-RPC message a response carries, if any.
async fn jsonrpc_body(response: Response) -> Option<Value> {
    if response.status() == StatusCode::ACCEPTED {
        return None;
    }
    let bytes = to_bytes(response.into_body(), usize::MAX).await.ok()?;
    serde_json::from_slice::<Value>(&bytes)
        .ok()
        .filter(|message| message.get("jsonrpc").is_some() || message.is_array())
}

async fn handle_message(
    state: &McpFrontendState,
    headers: &HeaderMap,
    caller: &AuthenticatedCaller,
    query: &McpQuery,
    message: Value,
) -> Response {
    if message.get("method").is_none()
        && (message.get("result").is_some() || message.get("error").is_some())
    {
        // A client's reply to a server request. This server sends none, so
        // there is nothing to route; acknowledge it as the SDK servers do.
        return StatusCode::ACCEPTED.into_response();
    }
    let rpc: JsonRpcRequest = match serde_json::from_value(message) {
        Ok(rpc) => rpc,
        Err(error) => {
            return jsonrpc_error(
                StatusCode::BAD_REQUEST,
                None,
                -32600,
                &format!("invalid request: {error}"),
            );
        }
    };
    process_rpc(state, headers, caller, query, rpc).await
}

/// HTTP+SSE clients get their reply as a `message` event on the stream that
/// announced this session; the POST itself is acknowledged with 202. A reply
/// is only ever delivered to the stream of the caller that owns the session.
async fn reply_on_sse_stream(
    state: &McpFrontendState,
    caller: &AuthenticatedCaller,
    session_id: &str,
    response: Response,
) -> Response {
    let replies = match state.sessions.read().await.get(session_id) {
        Some(session) if session.belongs_to(caller) => session.sse_replies.clone(),
        _ => None,
    };
    let Some(replies) = replies else {
        return response;
    };
    let status = response.status();
    if status == StatusCode::ACCEPTED {
        return response;
    }
    let Some(reply) = jsonrpc_body(response).await else {
        return status.into_response();
    };
    match tokio::time::timeout(SSE_REPLY_SEND_TIMEOUT, replies.send(reply.to_string())).await {
        Ok(Ok(())) => StatusCode::ACCEPTED.into_response(),
        Ok(Err(_)) => StatusCode::NOT_FOUND.into_response(),
        Err(_) => StatusCode::SERVICE_UNAVAILABLE.into_response(),
    }
}

/// Insert a new MCP session, sweeping expired ones. Returns false at
/// capacity.
async fn insert_session(
    state: &McpFrontendState,
    session_id: String,
    session: Arc<McpSession>,
) -> bool {
    let mut sessions = state.sessions.write().await;
    let expired_callers = sessions
        .values()
        .filter(|session| session.created_at.elapsed() >= MCP_SESSION_MAX_AGE)
        .map(|session| session.caller())
        .collect::<Vec<_>>();
    sessions.retain(|_, session| session.created_at.elapsed() < MCP_SESSION_MAX_AGE);
    let inserted = sessions.len() < MAX_ACTIVE_MCP_SESSIONS;
    if inserted {
        sessions.insert(session_id, session);
    }
    drop(sessions);
    for expired_caller in expired_callers {
        schedule_session_close(state.backend.clone(), expired_caller);
    }
    inserted
}

async fn process_rpc(
    state: &McpFrontendState,
    headers: &HeaderMap,
    caller: &AuthenticatedCaller,
    query: &McpQuery,
    rpc: JsonRpcRequest,
) -> Response {
    let requested_view = query.view;
    if rpc.jsonrpc != "2.0" {
        return jsonrpc_error(
            StatusCode::BAD_REQUEST,
            rpc.id,
            -32600,
            "jsonrpc must be 2.0",
        );
    }
    if let Err(error) = validate_protocol_headers(headers, &rpc) {
        return jsonrpc_error(StatusCode::BAD_REQUEST, rpc.id, -32600, &error);
    }
    match one_optional_header(headers, DECLARED_CAPABILITY_HEADER) {
        Ok(None) => {}
        Ok(Some(_)) => {
            return jsonrpc_error(
                StatusCode::BAD_REQUEST,
                rpc.id,
                -32600,
                "x-opdbus-capability is not accepted on MCP; authority is resolved server-side",
            );
        }
        Err(error) => return jsonrpc_error(StatusCode::BAD_REQUEST, rpc.id, -32600, &error),
    }

    let header_session_id = match one_optional_header(headers, MCP_SESSION_HEADER) {
        Ok(value) => value.map(str::to_string),
        Err(error) => return jsonrpc_error(StatusCode::BAD_REQUEST, rpc.id, -32600, &error),
    };
    let supplied_session_id = match (header_session_id, query.sse_session.as_deref()) {
        (Some(header), Some(sse)) if header != sse => {
            return jsonrpc_error(
                StatusCode::BAD_REQUEST,
                rpc.id,
                -32600,
                "Mcp-Session-Id differs from the sessionId query",
            );
        }
        (header, sse) => header.or_else(|| sse.map(str::to_string)),
    };
    // An HTTP+SSE session exists from its GET stream; the client still sends
    // initialize on it. Streamable HTTP sessions are created by initialize.
    if rpc.method == "initialize" && supplied_session_id.is_some() && query.sse_session.is_none() {
        return jsonrpc_error(
            StatusCode::BAD_REQUEST,
            rpc.id,
            -32600,
            "initialize must not carry an existing MCP session",
        );
    }
    let session = match supplied_session_id.as_deref() {
        Some(session_id) => {
            let session = state.sessions.read().await.get(session_id).cloned();
            match session {
                Some(session) if session.belongs_to(caller) => Some(session),
                _ => {
                    return jsonrpc_error(
                        StatusCode::NOT_FOUND,
                        rpc.id,
                        -32001,
                        "MCP session not found",
                    );
                }
            }
        }
        None => None,
    };
    let view = match &session {
        Some(session) => {
            if requested_view.is_some_and(|requested| requested != session.view) {
                return jsonrpc_error(
                    StatusCode::BAD_REQUEST,
                    rpc.id,
                    -32600,
                    "MCP view differs from initialized session",
                );
            }
            session.view
        }
        None => requested_view.unwrap_or(McpView::Typed),
    };
    if view == McpView::Compact && rpc.method != "initialize" && session.is_none() {
        return jsonrpc_error(
            StatusCode::BAD_REQUEST,
            rpc.id,
            -32600,
            "compact MCP view requires an initialized session",
        );
    }
    let session_selection = match &session {
        Some(session) => session.selection.read().await.clone(),
        None => None,
    };

    let id = rpc.id.clone();
    let request_timeout = request_timeout_for(&rpc.method, &rpc.params);
    let outcome = tokio::time::timeout(
        request_timeout,
        dispatch_rpc(
            state.backend.as_ref(),
            caller,
            &rpc.method,
            rpc.params,
            session_selection.as_ref(),
            view,
        ),
    )
    .await;

    match outcome {
        Ok(Ok(mut result)) => {
            if rpc.method == "initialize" {
                if let Some(origin) = &caller.wireguard_origin {
                    result["_meta"]["opdbus/wireguardOrigin"] = json!(origin);
                }
            }
            if let (Some(session), Some(selection)) =
                (session.as_ref(), selected_toolset_from_result(&result))
            {
                *session.selection.write().await = Some(selection);
            }

            match id {
                Some(id) => {
                    let mut response = (
                        StatusCode::OK,
                        axum::Json(json!({"jsonrpc": "2.0", "id": id.clone(), "result": result})),
                    )
                        .into_response();
                    // Native verification uses an ordinary authenticated SID1 ping.
                    // Never echo headers, emit claims for notifications/errors, or
                    // confuse the separate MCP conversation session with identity.
                    if rpc.method == "ping"
                        && matches!(
                            one_optional_header(headers, HTTP_SEALED_ID_HEADER),
                            Ok(Some(_))
                        )
                    {
                        for (name, value) in [
                            (VERIFIED_PRINCIPAL_HEADER, &caller.principal_id),
                            (VERIFIED_SESSION_HEADER, &caller.session_id),
                            (VERIFIED_GENESIS_HEADER, &caller.session_genesis),
                        ] {
                            let Ok(value) = value.parse() else {
                                return jsonrpc_error(
                                    StatusCode::INTERNAL_SERVER_ERROR,
                                    Some(id),
                                    -32603,
                                    "verified identity metadata unavailable",
                                );
                            };
                            response.headers_mut().insert(name, value);
                        }
                    }
                    if rpc.method == "initialize" && session.is_none() {
                        let session_id = uuid::Uuid::new_v4().to_string();
                        let created = Arc::new(McpSession::new(caller, view));
                        if !insert_session(state, session_id.clone(), created).await {
                            return jsonrpc_error(
                                StatusCode::SERVICE_UNAVAILABLE,
                                Some(id),
                                -32000,
                                "MCP session capacity reached",
                            );
                        }
                        response.headers_mut().insert(
                            MCP_SESSION_HEADER,
                            session_id
                                .parse()
                                .expect("UUID is a valid HTTP header value"),
                        );
                    }
                    response
                }
                None => StatusCode::ACCEPTED.into_response(),
            }
        }
        Ok(Err(error)) => jsonrpc_error(StatusCode::OK, id, error.code, &error.message),
        Err(_) => jsonrpc_error(StatusCode::REQUEST_TIMEOUT, id, -32000, "request timed out"),
    }
}

fn request_timeout_for(method: &str, params: &Value) -> Duration {
    if method == "tools/call" {
        let outer_name = params
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let name = if outer_name == "execute_tool" {
            params
                .get("arguments")
                .and_then(|arguments| arguments.get("tool_name"))
                .and_then(Value::as_str)
                .unwrap_or_default()
        } else {
            outer_name
        };
        if name == "plugin.notebooklm.setup_auth" {
            return NOTEBOOKLM_AUTH_REQUEST_TIMEOUT;
        }
        if matches!(
            name,
            "specialist"
                | "rust_pro"
                | "python_pro"
                | "typescript_pro"
                | "sequential_thinking"
                | "context_manager"
                | "code_review_orchestrator"
        ) || name
            .strip_prefix("plugin.cognitive_mcp.")
            .is_some_and(|method| {
                op_plugins::state_plugins::cognitive_agents::method_by_name(method).is_some()
            })
        {
            return AGENT_REQUEST_TIMEOUT;
        }
    }
    REQUEST_TIMEOUT
}

async fn handle_mcp_events(
    State(state): State<McpFrontendState>,
    request: Request<Body>,
) -> Response {
    let peer = peer_addr(request.extensions());
    let headers = request.headers().clone();
    let query = match parse_query(request.uri().query()) {
        Ok(query) if query.sse_session.is_none() => query,
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                axum::Json(json!({"error": "invalid_request", "message": "unsupported MCP query"})),
            )
                .into_response();
        }
    };
    if let Err(error) = validate_origin(&headers, &state.allowed_origins) {
        return (
            StatusCode::FORBIDDEN,
            axum::Json(json!({"error": "origin_rejected", "message": error})),
        )
            .into_response();
    }
    let caller = match state.authenticator.authenticate(&headers, peer).await {
        Ok(caller) => caller,
        Err(error) => {
            return (
                StatusCode::UNAUTHORIZED,
                axum::Json(json!({"error": "unauthenticated", "message": error.0})),
            )
                .into_response();
        }
    };
    if let Err(error) = validate_event_stream_headers(&headers) {
        return (
            StatusCode::BAD_REQUEST,
            axum::Json(json!({"error": "invalid_request", "message": error})),
        )
            .into_response();
    }
    let session_id = match one_optional_header(&headers, MCP_SESSION_HEADER) {
        Ok(Some(value)) => value,
        // No session: an HTTP+SSE client opening its transport stream.
        Ok(None) => return open_sse_transport(&state, caller, query.view).await,
        Err(error) => {
            return (
                StatusCode::BAD_REQUEST,
                axum::Json(json!({"error": "invalid_request", "message": error})),
            )
                .into_response();
        }
    };
    let Some(session) = state.sessions.read().await.get(session_id).cloned() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if !session.belongs_to(&caller) {
        return StatusCode::NOT_FOUND.into_response();
    }
    if session
        .event_stream_active
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return (
            StatusCode::CONFLICT,
            axum::Json(json!({
                "error": "event_stream_exists",
                "message": "this MCP session already has an active event stream"
            })),
        )
            .into_response();
    }

    let mut events = session.events.subscribe();
    let active = session.event_stream_active.clone();
    let lease = EventStreamLease(active);
    let stream = async_stream::stream! {
        let _lease = lease;
        loop {
            match events.recv().await {
                Ok(message) => yield Ok::<Event, Infallible>(Event::default().data(message)),
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    };
    Sse::new(stream)
        .keep_alive(
            KeepAlive::new()
                .interval(Duration::from_secs(15))
                .text("keepalive"),
        )
        .into_response()
}

/// HTTP+SSE transport (MCP 2024-11-05) on the same path: the stream first
/// announces the endpoint to POST to, then carries every reply and
/// notification for the session. Closing the stream ends the session.
async fn open_sse_transport(
    state: &McpFrontendState,
    caller: AuthenticatedCaller,
    view: Option<McpView>,
) -> Response {
    let view = view.unwrap_or(McpView::Typed);
    let session_id = uuid::Uuid::new_v4().to_string();
    let (replies, mut reply_rx) = mpsc::channel::<String>(SSE_REPLY_QUEUE);
    let session = Arc::new(McpSession::with_sse_replies(&caller, view, replies));
    let mut events = session.events.subscribe();
    if !insert_session(state, session_id.clone(), session).await {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            axum::Json(json!({"error": "capacity", "message": "MCP session capacity reached"})),
        )
            .into_response();
    }
    let endpoint = match view {
        McpView::Compact => format!("{MCP_PATH}?{COMPACT_MODE_QUERY}&sessionId={session_id}"),
        McpView::Typed => format!("{MCP_PATH}?sessionId={session_id}"),
    };
    let lease = SseSessionLease {
        sessions: state.sessions.clone(),
        backend: state.backend.clone(),
        session_id,
        caller,
    };
    let stream = async_stream::stream! {
        let _lease = lease;
        yield Ok::<Event, Infallible>(Event::default().event("endpoint").data(endpoint));
        loop {
            let message = tokio::select! {
                reply = reply_rx.recv() => reply,
                notification = next_notification(&mut events) => notification,
            };
            match message {
                Some(message) => yield Ok(Event::default().event("message").data(message)),
                None => break,
            }
        }
    };
    Sse::new(stream)
        .keep_alive(
            KeepAlive::new()
                .interval(Duration::from_secs(15))
                .text("keepalive"),
        )
        .into_response()
}

async fn next_notification(events: &mut broadcast::Receiver<String>) -> Option<String> {
    loop {
        match events.recv().await {
            Ok(message) => return Some(message),
            Err(broadcast::error::RecvError::Lagged(_)) => continue,
            Err(broadcast::error::RecvError::Closed) => return None,
        }
    }
}

/// Ends an HTTP+SSE session when its stream is dropped.
struct SseSessionLease {
    sessions: Arc<RwLock<HashMap<String, Arc<McpSession>>>>,
    backend: Arc<dyn McpBackend>,
    session_id: String,
    caller: AuthenticatedCaller,
}

impl Drop for SseSessionLease {
    fn drop(&mut self) {
        let sessions = self.sessions.clone();
        let backend = self.backend.clone();
        let session_id = std::mem::take(&mut self.session_id);
        let caller = self.caller.clone();
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move {
                if sessions.write().await.remove(&session_id).is_some() {
                    let _ = backend.session_closed(&caller).await;
                }
            });
        }
    }
}

async fn handle_mcp_delete(
    State(state): State<McpFrontendState>,
    request: Request<Body>,
) -> Response {
    let peer = peer_addr(request.extensions());
    let headers = request.headers().clone();
    if let Err(error) = validate_origin(&headers, &state.allowed_origins) {
        return (
            StatusCode::FORBIDDEN,
            axum::Json(json!({"error": "origin_rejected", "message": error})),
        )
            .into_response();
    }
    if let Err(error) = validate_protocol_version_header(&headers, false) {
        return (
            StatusCode::BAD_REQUEST,
            axum::Json(json!({"error": "invalid_request", "message": error})),
        )
            .into_response();
    }
    let caller = match state.authenticator.authenticate(&headers, peer).await {
        Ok(caller) => caller,
        Err(error) => {
            return (
                StatusCode::UNAUTHORIZED,
                axum::Json(json!({"error": "unauthenticated", "message": error.0})),
            )
                .into_response();
        }
    };
    let session_id = match one_required_header(&headers, MCP_SESSION_HEADER) {
        Ok(value) => value.to_string(),
        Err(error) => {
            return (
                StatusCode::BAD_REQUEST,
                axum::Json(json!({"error": "invalid_request", "message": error})),
            )
                .into_response();
        }
    };
    let session = state.sessions.read().await.get(&session_id).cloned();
    match session {
        Some(session) if session.belongs_to(&caller) => {
            state.sessions.write().await.remove(&session_id);
            schedule_session_close(state.backend.clone(), caller);
            StatusCode::NO_CONTENT.into_response()
        }
        _ => StatusCode::NOT_FOUND.into_response(),
    }
}

#[derive(Debug)]
struct RpcDispatchError {
    code: i64,
    message: String,
}

impl RpcDispatchError {
    fn method_not_found(method: &str) -> Self {
        Self {
            code: -32601,
            message: format!("method not found: {method}"),
        }
    }

    fn invalid_params(message: impl Into<String>) -> Self {
        Self {
            code: -32602,
            message: message.into(),
        }
    }

    fn internal(error: impl std::fmt::Display) -> Self {
        Self {
            code: -32603,
            message: error.to_string(),
        }
    }
}

fn compact_tool_definitions() -> Result<Vec<Value>, RpcDispatchError> {
    op_mcp::compact::compact_tools_schema()
        .into_iter()
        .map(|definition| serde_json::to_value(definition).map_err(RpcDispatchError::internal))
        .collect()
}

/// The compact view borrows only the already authenticated, grant-filtered
/// backend. It never owns an unscoped tool registry or caches a caller across
/// requests; the frontend reauthenticates before every invocation.
async fn compact_call(
    backend: &dyn McpBackend,
    caller: &AuthenticatedCaller,
    name: &str,
    arguments: Value,
    selection: Option<&ToolsetSelection>,
) -> Result<Value, RpcDispatchError> {
    let object = arguments
        .as_object()
        .ok_or_else(|| RpcDispatchError::invalid_params("compact arguments must be an object"))?;
    match name {
        "list_tools" | "search_tools" | "get_tool_schema" => {
            let mut tools = backend
                .list_tools(caller, selection)
                .await
                .map_err(RpcDispatchError::internal)?;
            tools.sort_by(|left, right| tool_name(left).cmp(tool_name(right)));
            let result = match name {
                "list_tools" => {
                    let category = object.get("category").and_then(Value::as_str);
                    let offset = object.get("offset").and_then(Value::as_u64).unwrap_or(0);
                    let limit = object.get("limit").and_then(Value::as_u64).unwrap_or(20);
                    if offset > 10_000 || !(1..=100).contains(&limit) {
                        return Err(RpcDispatchError::invalid_params(
                            "compact list offset or limit is out of range",
                        ));
                    }
                    let tools = tools
                        .into_iter()
                        .filter(|tool| {
                            category.is_none_or(|category| {
                                let category = category.to_lowercase();
                                tool_name(tool).to_lowercase().contains(&category)
                                    || tool
                                        .get("description")
                                        .and_then(Value::as_str)
                                        .unwrap_or("")
                                        .to_lowercase()
                                        .contains(&category)
                            })
                        })
                        .skip(offset as usize)
                        .take(limit as usize)
                        .map(|tool| {
                            json!({
                                "name": tool_name(&tool),
                                "description": tool.get("description").and_then(Value::as_str).unwrap_or("")
                            })
                        })
                        .collect::<Vec<_>>();
                    json!({"count": tools.len(), "tools": tools, "offset": offset, "limit": limit})
                }
                "search_tools" => {
                    let query = object
                        .get("query")
                        .and_then(Value::as_str)
                        .filter(|query| !query.trim().is_empty() && query.len() <= 256)
                        .ok_or_else(|| RpcDispatchError::invalid_params("invalid search query"))?
                        .to_lowercase();
                    let limit = object.get("limit").and_then(Value::as_u64).unwrap_or(10);
                    if !(1..=100).contains(&limit) {
                        return Err(RpcDispatchError::invalid_params(
                            "compact search limit is out of range",
                        ));
                    }
                    let tools = tools
                        .into_iter()
                        .filter(|tool| {
                            tool_name(tool).to_lowercase().contains(&query)
                                || tool
                                    .get("description")
                                    .and_then(Value::as_str)
                                    .unwrap_or("")
                                    .to_lowercase()
                                    .contains(&query)
                        })
                        .take(limit as usize)
                        .map(|tool| {
                            json!({
                                "name": tool_name(&tool),
                                "description": tool.get("description").and_then(Value::as_str).unwrap_or("")
                            })
                        })
                        .collect::<Vec<_>>();
                    json!({"count": tools.len(), "tools": tools})
                }
                _ => {
                    let target = object
                        .get("tool_name")
                        .and_then(Value::as_str)
                        .filter(|name| !name.is_empty())
                        .ok_or_else(|| RpcDispatchError::invalid_params("tool_name is required"))?;
                    let tool = tools
                        .into_iter()
                        .find(|tool| tool_name(tool) == target)
                        .ok_or_else(|| RpcDispatchError::invalid_params("tool unavailable"))?;
                    json!({"tool": target, "schema": tool.get("inputSchema").cloned().unwrap_or(Value::Null)})
                }
            };
            Ok(normalize_tool_result(result))
        }
        "execute_tool" => {
            let target = object
                .get("tool_name")
                .and_then(Value::as_str)
                .filter(|name| !name.is_empty())
                .ok_or_else(|| RpcDispatchError::invalid_params("tool_name is required"))?;
            let input = object
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
            if !input.is_object() {
                return Err(RpcDispatchError::invalid_params(
                    "nested tool arguments must be an object",
                ));
            }
            backend
                .call_tool(caller, target, input, selection)
                .await
                .map_err(RpcDispatchError::internal)
        }
        "respond" => {
            let message = object
                .get("message")
                .and_then(Value::as_str)
                .filter(|message| message.len() <= 4096)
                .ok_or_else(|| RpcDispatchError::invalid_params("invalid response message"))?;
            Ok(normalize_tool_result(json!({"message": message})))
        }
        _ => Err(RpcDispatchError::invalid_params(
            "unknown compact meta-tool",
        )),
    }
}

async fn dispatch_rpc(
    backend: &dyn McpBackend,
    caller: &AuthenticatedCaller,
    method: &str,
    params: Value,
    session_selection: Option<&ToolsetSelection>,
    view: McpView,
) -> Result<Value, RpcDispatchError> {
    match method {
        // The handshake establishes a Streamable HTTP session used only for
        // per-client tool projection and server notifications.
        "initialize" => {
            let requested = params
                .get("protocolVersion")
                .and_then(Value::as_str)
                .ok_or_else(|| RpcDispatchError::invalid_params("missing protocolVersion"))?;
            Ok(initialize_result(
                negotiate_protocol_version(requested),
                view,
            ))
        }
        "notifications/initialized" => Ok(json!({})),
        "ping" => Ok(json!({})),
        "server/discover" => Ok(discovery_result()),
        "tools/list" => {
            if view == McpView::Compact {
                return paginate("tools", compact_tool_definitions()?, &params);
            }
            let selection = selection_from_params(&params)?.or_else(|| session_selection.cloned());
            let mut tools = backend
                .list_tools(caller, selection.as_ref())
                .await
                .map_err(RpcDispatchError::internal)?;
            tools.sort_by(|left, right| tool_name(left).cmp(tool_name(right)));
            Ok(paginate("tools", tools, &params)?)
        }
        "tools/call" => {
            let name = required_string_param(&params, "name")?;
            let mut arguments = params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
            let selection = merged_toolset_selection(&params, &mut arguments)?
                .or_else(|| session_selection.cloned());
            if view == McpView::Compact {
                return compact_call(backend, caller, name, arguments, selection.as_ref()).await;
            }
            backend
                .call_tool(caller, name, arguments, selection.as_ref())
                .await
                .map_err(RpcDispatchError::internal)
        }
        "resources/list" => {
            let mut resources = backend
                .list_resources(caller)
                .await
                .map_err(RpcDispatchError::internal)?;
            resources.sort_by(|left, right| resource_uri(left).cmp(resource_uri(right)));
            Ok(paginate("resources", resources, &params)?)
        }
        "resources/read" => {
            let uri = required_string_param(&params, "uri")?;
            backend
                .read_resource(caller, uri)
                .await
                .map_err(RpcDispatchError::internal)
        }
        other => Err(RpcDispatchError::method_not_found(other)),
    }
}

fn negotiate_protocol_version(requested: &str) -> &'static str {
    SUPPORTED_PROTOCOL_VERSIONS
        .iter()
        .copied()
        .find(|version| *version == requested)
        .unwrap_or(LATEST_OFFICIAL_PROTOCOL_VERSION)
}

fn protocol_version_supported(version: &str) -> bool {
    SUPPORTED_PROTOCOL_VERSIONS.contains(&version)
}

fn initialize_result(protocol_version: &str, view: McpView) -> Value {
    let instructions = match view {
        McpView::Typed => {
            "Each request requires exactly one authenticated identity. tools/list returns the real typed tools declared by the broker-curated toolsets. HOT tools—including Rust Pro, memory/code context, and healthy NotebookLM providers—are immediately callable. Before calling a non-HOT tool, use toolsets to select the toolset containing it."
        }
        McpView::Compact => {
            "Five compact meta-tools expose only the current caller's broker-curated tools. Use list_tools or search_tools, then get_tool_schema and execute_tool. Select a non-HOT toolset through execute_tool(tool_name=toolsets) before calling its tools. Every call rechecks the caller's current identity and grants."
        }
    };
    json!({
        "protocolVersion": protocol_version,
        "capabilities": {
            "tools": {"listChanged": false},
            "resources": {"subscribe": false, "listChanged": false}
        },
        "serverInfo": {
            "name": "op-grpc-bridge",
            "version": env!("CARGO_PKG_VERSION")
        },
        "instructions": instructions
    })
}

fn discovery_result() -> Value {
    json!({
        "protocolVersions": SUPPORTED_PROTOCOL_VERSIONS,
        "endpoint": MCP_PATH,
        "transport": "streamable-http",
        "transports": ["streamable-http", "http+sse"],
        "batching": true,
        "sessionMode": "stateful-toolset-admission",
        "requiredHeaders": [],
        "defaultProtocolVersion": "2025-03-26",
        "identityHeaderAlternatives": [HTTP_ASSERTION_HEADER, HTTP_SEALED_ID_HEADER],
        "optionalIntegrityHeaders": ["Mcp-Method", "Mcp-Name"],
        "capabilities": initialize_result(MCP_PROTOCOL_VERSION, McpView::Typed)["capabilities"].clone()
    })
}

fn validate_protocol_headers(headers: &HeaderMap, rpc: &JsonRpcRequest) -> Result<(), String> {
    // The initialize request negotiates the protocol, so it may name any
    // revision. A later request that names one must name a revision this
    // frontend speaks. Without the header the spec has the server assume
    // 2025-03-26 (clients of that revision and HTTP+SSE clients never send
    // it), which this frontend speaks.
    validate_protocol_version_header(headers, rpc.method == "initialize")?;
    if one_optional_header(headers, MCP_METHOD_HEADER)?.is_some_and(|method| method != rpc.method) {
        return Err("Mcp-Method header does not match JSON-RPC method".into());
    }

    let expected_name = match rpc.method.as_str() {
        "tools/call" => {
            Some(required_string_param(&rpc.params, "name").map_err(|error| error.message)?)
        }
        "resources/read" => {
            Some(required_string_param(&rpc.params, "uri").map_err(|error| error.message)?)
        }
        _ => None,
    };
    match expected_name {
        Some(expected) => {
            if one_optional_header(headers, MCP_NAME_HEADER)?
                .is_some_and(|actual| actual != expected)
            {
                return Err("Mcp-Name header does not match JSON-RPC target".into());
            }
        }
        None => {
            if one_optional_header(headers, MCP_NAME_HEADER)?.is_some() {
                return Err("Mcp-Name is not valid for this method".into());
            }
        }
    }
    Ok(())
}

fn validate_protocol_version_header(headers: &HeaderMap, negotiating: bool) -> Result<(), String> {
    match one_optional_header(headers, MCP_VERSION_HEADER)? {
        Some(version) if !negotiating && !protocol_version_supported(version) => {
            Err(format!("unsupported MCP protocol version: {version}"))
        }
        _ => Ok(()),
    }
}

fn validate_event_stream_headers(headers: &HeaderMap) -> Result<(), String> {
    validate_protocol_version_header(headers, false)?;
    let accept = one_required_header(headers, axum::http::header::ACCEPT.as_str())?;
    if !accept
        .split(',')
        .map(str::trim)
        .any(|value| value.eq_ignore_ascii_case("text/event-stream"))
    {
        return Err("MCP event stream requires Accept: text/event-stream".into());
    }
    Ok(())
}

fn one_required_header<'a>(headers: &'a HeaderMap, name: &str) -> Result<&'a str, String> {
    one_optional_header(headers, name)?.ok_or_else(|| format!("missing {name} header"))
}

fn one_optional_header<'a>(headers: &'a HeaderMap, name: &str) -> Result<Option<&'a str>, String> {
    let mut values = headers.get_all(name).iter();
    let Some(first) = values.next() else {
        return Ok(None);
    };
    if values.next().is_some() {
        return Err(format!("duplicate {name} header"));
    }
    let value = first
        .to_str()
        .map_err(|_| format!("malformed {name} header"))?
        .trim();
    if value.is_empty() {
        return Err(format!("empty {name} header"));
    }
    Ok(Some(value))
}

/// Exact browser origin allowlist shared by MCP HTTP and gRPC-Web CORS.
/// Invalid entries are ignored, which fails closed for those origins.
pub(crate) fn configured_allowed_origins() -> HashSet<String> {
    std::env::var(ALLOWED_ORIGINS_ENV)
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|origin| valid_origin(origin))
        .map(str::to_string)
        .collect()
}

pub(crate) fn configured_allow_origin() -> tower_http::cors::AllowOrigin {
    let origins = configured_allowed_origins()
        .into_iter()
        .filter_map(|origin| origin.parse::<axum::http::HeaderValue>().ok())
        .collect::<Vec<_>>();
    tower_http::cors::AllowOrigin::list(origins)
}

fn valid_origin(origin: &str) -> bool {
    if origin.is_empty() || origin == "*" || origin.eq_ignore_ascii_case("null") {
        return false;
    }
    let Ok(uri) = origin.parse::<axum::http::Uri>() else {
        return false;
    };
    matches!(uri.scheme_str(), Some("https") | Some("http"))
        && uri.authority().is_some()
        && (uri.path().is_empty() || uri.path() == "/")
        && uri.query().is_none()
}

fn validate_origin(headers: &HeaderMap, allowed_origins: &HashSet<String>) -> Result<(), String> {
    let origin = one_optional_header(headers, axum::http::header::ORIGIN.as_str())?;
    if let Some(origin) = origin {
        if !valid_origin(origin) || !allowed_origins.contains(origin) {
            return Err("Origin is not on the exact MCP allowlist".into());
        }
        return Ok(());
    }

    let browser_marked = ["sec-fetch-mode", "sec-fetch-site", "sec-fetch-dest"]
        .iter()
        .any(|name| headers.contains_key(*name));
    if browser_marked {
        return Err("browser MCP requests require an Origin header".into());
    }
    Ok(())
}

fn required_string_param<'a>(params: &'a Value, name: &str) -> Result<&'a str, RpcDispatchError> {
    params
        .get(name)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| RpcDispatchError::invalid_params(format!("missing {name}")))
}

fn selection_from_params(params: &Value) -> Result<Option<ToolsetSelection>, RpcDispatchError> {
    selection_from_meta(params.get("_meta"))
}

fn merged_toolset_selection(
    params: &Value,
    arguments: &mut Value,
) -> Result<Option<ToolsetSelection>, RpcDispatchError> {
    let from_params = selection_from_params(params)?;
    let from_arguments = take_argument_toolset_selection(arguments)?;
    match (from_params, from_arguments) {
        (Some(left), Some(right)) if left != right => Err(RpcDispatchError::invalid_params(
            "conflicting _meta.opdbus_toolset selectors",
        )),
        (Some(selection), _) | (_, Some(selection)) => Ok(Some(selection)),
        (None, None) => Ok(None),
    }
}

fn selection_from_meta(meta: Option<&Value>) -> Result<Option<ToolsetSelection>, RpcDispatchError> {
    let Some(raw) = meta.and_then(|value| value.get("opdbus_toolset")) else {
        return Ok(None);
    };
    let selection: ToolsetSelection = serde_json::from_value(raw.clone()).map_err(|error| {
        RpcDispatchError::invalid_params(format!("invalid toolset selector: {error}"))
    })?;
    if selection.id.is_empty() || selection.generation == 0 {
        return Err(RpcDispatchError::invalid_params(
            "toolset selector requires a non-empty id and positive generation",
        ));
    }
    Ok(Some(selection))
}

fn take_argument_toolset_selection(
    arguments: &mut Value,
) -> Result<Option<ToolsetSelection>, RpcDispatchError> {
    let Some(object) = arguments.as_object_mut() else {
        return Ok(None);
    };
    let mut remove_meta = false;
    let raw = if let Some(meta) = object.get_mut("_meta").and_then(Value::as_object_mut) {
        let value = meta.remove("opdbus_toolset");
        remove_meta = meta.is_empty();
        value
    } else {
        None
    };
    if remove_meta {
        object.remove("_meta");
    }
    match raw {
        Some(value) => selection_from_meta(Some(&json!({"opdbus_toolset": value}))),
        None => Ok(None),
    }
}

fn selected_toolset_from_result(result: &Value) -> Option<ToolsetSelection> {
    let projected = result.get("structuredContent").unwrap_or(result);
    if projected.get("operation").and_then(Value::as_str) != Some("select") {
        return None;
    }
    serde_json::from_value(projected.get("result")?.get("selector")?.clone()).ok()
}

fn paginate(key: &str, values: Vec<Value>, params: &Value) -> Result<Value, RpcDispatchError> {
    let offset = match params.get("cursor").and_then(Value::as_str) {
        Some(cursor) => cursor
            .parse::<usize>()
            .map_err(|_| RpcDispatchError::invalid_params("invalid cursor"))?,
        None => 0,
    };
    let limit = params
        .get("limit")
        .and_then(Value::as_u64)
        .map(|value| value as usize)
        .unwrap_or(DEFAULT_PAGE_SIZE)
        .clamp(1, MAX_PAGE_SIZE);
    if offset > values.len() {
        return Err(RpcDispatchError::invalid_params(
            "cursor is past the result set",
        ));
    }
    let end = offset.saturating_add(limit).min(values.len());
    let mut result = serde_json::Map::new();
    result.insert(key.to_string(), Value::Array(values[offset..end].to_vec()));
    if end < values.len() {
        result.insert("nextCursor".into(), Value::String(end.to_string()));
    }
    Ok(Value::Object(result))
}

fn parse_typed_tool_name(name: &str) -> anyhow::Result<(&str, &str)> {
    let mut parts = name.split('.');
    if parts.next() != Some("plugin") {
        anyhow::bail!("AccessDenied: '{name}' is not a canonical typed plugin tool");
    }
    let plugin_id = parts
        .next()
        .filter(|part| !part.is_empty())
        .ok_or_else(|| anyhow::anyhow!("AccessDenied: '{name}' has no plugin id"))?;
    let method_name = parts
        .next()
        .filter(|part| !part.is_empty())
        .ok_or_else(|| anyhow::anyhow!("AccessDenied: '{name}' has no method"))?;
    if parts.next().is_some()
        || !plugin_id
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
        || !method_name
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_alphabetic())
        || !method_name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        anyhow::bail!("AccessDenied: '{name}' is not canonical");
    }
    Ok((plugin_id, method_name))
}

fn method_descriptor(
    schema: &op_state_store::PluginSchema,
    method_name: &str,
    public_name: &str,
) -> anyhow::Result<Value> {
    let method = schema.methods.get(method_name).ok_or_else(|| {
        anyhow::anyhow!(
            "AccessDenied: sealed plugin '{}' has no method '{}'",
            schema.name,
            method_name
        )
    })?;
    let required_capability = method.required_capability.as_deref().ok_or_else(|| {
        anyhow::anyhow!(
            "AccessDenied: {}.{} has no sealed capability",
            schema.name,
            method_name
        )
    })?;
    if !schema.capabilities.contains_key(required_capability) {
        anyhow::bail!(
            "AccessDenied: {}.{} capability '{}' is outside schema closure",
            schema.name,
            method_name,
            required_capability
        );
    }
    let input_schema = mcp_input_schema(serde_json::to_value(&method.args)?);
    let description = input_schema
        .get("description")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| format!("{} — {}", schema.description, method.name));
    let mut descriptor = json!({
        "name": public_name,
        "description": description,
        "inputSchema": input_schema,
        "required_capability": required_capability,
        "subid": method.subid,
        "authority_method": method.name
    });
    if let Some(output) = &method.returns {
        descriptor["outputSchema"] = serde_json::to_value(output)?;
    }
    Ok(descriptor)
}

fn mcp_input_schema(schema: Value) -> Value {
    if schema.get("type").and_then(Value::as_str) == Some("null") {
        json!({
            "type": "object",
            "properties": {},
            "additionalProperties": false
        })
    } else {
        schema
    }
}

fn normalize_tool_definition(mut tool: Value) -> Value {
    let Some(object) = tool.as_object_mut() else {
        return tool;
    };
    if let Some(schema) = object.remove("input_schema") {
        object.insert("inputSchema".into(), schema);
    }
    for internal in [
        "category",
        "namespace",
        "schema_version",
        "tags",
        "required_capability",
        "subid",
        "authority_method",
    ] {
        object.remove(internal);
    }
    tool
}

fn normalize_tool_result(result: Value) -> Value {
    if result.as_object().is_some_and(|object| {
        object.contains_key("content") || object.contains_key("structuredContent")
    }) {
        return result;
    }
    let text = serde_json::to_string(&result).unwrap_or_else(|_| "null".into());
    json!({
        "content": [{"type": "text", "text": text}],
        "structuredContent": result,
        "isError": false
    })
}

fn tool_name(value: &Value) -> &str {
    value.get("name").and_then(Value::as_str).unwrap_or("")
}

fn authorize_with_grants(
    grants: &HashSet<String>,
    required_capability: &str,
) -> anyhow::Result<()> {
    if grants.contains(required_capability) {
        Ok(())
    } else {
        anyhow::bail!("AccessDenied: principal lacks required capability {required_capability}")
    }
}

fn descriptor_authority<'a>(
    descriptor: &'a Value,
    expected_name: Option<&str>,
) -> anyhow::Result<(&'a str, &'a str)> {
    let name = descriptor
        .get("name")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| anyhow::anyhow!("AccessDenied: tool descriptor has no name"))?;
    if expected_name.is_some_and(|expected| expected != name) {
        anyhow::bail!("AccessDenied: resolved tool descriptor name mismatch");
    }
    let capability = descriptor
        .get("required_capability")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "AccessDenied: target tool '{name}' has no required_capability descriptor"
            )
        })?;
    let subid = descriptor
        .get("subid")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            anyhow::anyhow!("AccessDenied: target tool '{name}' has no subid descriptor")
        })?;
    Ok((capability, subid))
}

#[allow(dead_code)] // infrastructure for capability-gated tool filtering
fn filter_authorized_tools(tools: Vec<Value>, grants: &HashSet<String>) -> Vec<Value> {
    tools
        .into_iter()
        .filter(|tool| {
            descriptor_authority(tool, None).is_ok_and(|(capability, _)| {
                capability != "cognitive_mcp.invoke" && grants.contains(capability)
            })
        })
        .collect()
}

fn validate_tool_arguments(descriptor: &Value, arguments: &Value) -> anyhow::Result<()> {
    let name = descriptor
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("<unknown>");
    let schema = descriptor
        .get("input_schema")
        .or_else(|| descriptor.get("inputSchema"))
        .ok_or_else(|| anyhow::anyhow!("AccessDenied: target tool '{name}' has no input schema"))?;
    let validator = jsonschema::Validator::new(schema)
        .map_err(|error| anyhow::anyhow!("invalid input schema for tool '{name}': {error}"))?;
    validator
        .validate(arguments)
        .map_err(|error| anyhow::anyhow!("invalid arguments for tool '{name}': {error}"))
}

fn authorize_and_validate_tool_call(
    descriptor: &Value,
    expected_name: &str,
    arguments: &Value,
    grants: &HashSet<String>,
) -> anyhow::Result<()> {
    let (required_capability, _) = descriptor_authority(descriptor, Some(expected_name))?;
    if required_capability == "cognitive_mcp.invoke" {
        anyhow::bail!(
            "AccessDenied: target tool '{expected_name}' has no independent sealed capability"
        );
    }
    authorize_with_grants(grants, required_capability)?;
    validate_tool_arguments(descriptor, arguments)
}

fn resource_uri(value: &Value) -> &str {
    value.get("uri").and_then(Value::as_str).unwrap_or("")
}

fn load_exact_capability_grants(principal_id: &str) -> HashSet<String> {
    load_capability_grants(principal_id)
}

fn list_blob_resources(dir: &Path) -> anyhow::Result<Vec<Value>> {
    let entries = manifest_plugins(dir)?;
    Ok(entries
        .keys()
        .filter(|plugin_id| !op_plugins::default_registry::is_retired_plugin(plugin_id))
        .map(|plugin_id| {
            json!({
                "uri": format!("blob://{plugin_id}"),
                "name": format!("{plugin_id} schema"),
                "description": format!("Sanitized sealed PluginSchema for {plugin_id}"),
                "mimeType": "application/json"
            })
        })
        .collect())
}

fn read_blob_resource(dir: &Path, uri: &str) -> anyhow::Result<Value> {
    let plugin_id = uri
        .strip_prefix("blob://")
        .filter(|value| valid_plugin_id(value))
        .ok_or_else(|| anyhow::anyhow!("unsupported resource URI: {uri}"))?;
    if op_plugins::default_registry::is_retired_plugin(plugin_id) {
        anyhow::bail!("resource not found: {uri}");
    }
    let mut schema = serde_json::to_value(read_manifest_pinned_plugin_schema(dir, plugin_id)?)?;
    sanitize_schema(&mut schema);
    let text = serde_json::to_string(&schema)?;
    Ok(json!({
        "contents": [{"uri": uri, "mimeType": "application/json", "text": text}]
    }))
}

fn sanitize_schema(value: &mut Value) {
    match value {
        Value::Object(object) => {
            object.retain(|key, _| !private_projection_key(key));
            for nested in object.values_mut() {
                sanitize_schema(nested);
            }
        }
        Value::Array(values) => {
            for nested in values {
                sanitize_schema(nested);
            }
        }
        _ => {}
    }
}

fn private_projection_key(key: &str) -> bool {
    let normalized = key.to_ascii_lowercase().replace('-', "_");
    matches!(
        normalized.as_str(),
        "capability_grants"
            | "principal_membership"
            | "principal_memberships"
            | "authorized_principals"
            | "member_ids"
            | "internal_path"
            | "socket_path"
            | "db_path"
            | "key_path"
            | "cert_path"
            | "key_id"
            | "decoy_key_id"
            | "approval_verifier"
            | "approval_verifier_config"
    ) || normalized.ends_with("_secret")
        || normalized.ends_with("_token")
        || normalized.ends_with("_password")
        || normalized.ends_with("_private_key")
        || normalized.ends_with("_api_key")
        || normalized.ends_with("_key_id")
}

fn encoded_len_upper_bound(decoded_len: usize) -> usize {
    decoded_len.saturating_add(2) / 3 * 4
}

fn decode_http_assertion(encoded: &str) -> Result<Vec<u8>, McpAuthError> {
    let wire = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(encoded.as_bytes())
        .map_err(|_| McpAuthError("Malformed Oracle identity assertion".into()))?;
    if wire.len() > MAX_ASSERTION_BYTES
        || base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&wire) != encoded
    {
        return Err(McpAuthError(
            "Oracle identity assertion is not canonical base64url".into(),
        ));
    }
    Ok(wire)
}

fn peer_addr(extensions: &axum::http::Extensions) -> Option<SocketAddr> {
    if let Some(info) = extensions.get::<TcpConnectInfo>()
        && let Some(addr) = info.remote_addr()
    {
        return Some(addr);
    }
    extensions
        .get::<TlsConnectInfo<TcpConnectInfo>>()
        .and_then(|info| info.get_ref().remote_addr())
        .or_else(|| {
            extensions
                .get::<axum::extract::ConnectInfo<SocketAddr>>()
                .map(|info| info.0)
        })
}

fn jsonrpc_error(status: StatusCode, id: Option<Value>, code: i64, message: &str) -> Response {
    (
        status,
        axum::Json(json!({
            "jsonrpc": "2.0",
            "id": id.unwrap_or(Value::Null),
            "error": {"code": code, "message": message}
        })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::header::CONTENT_TYPE;
    use tower::ServiceExt;

    #[test]
    fn notebooklm_setup_auth_alone_gets_the_interactive_request_timeout() {
        assert_eq!(
            request_timeout_for(
                "tools/call",
                &json!({"name": "plugin.notebooklm.setup_auth", "arguments": {}})
            ),
            NOTEBOOKLM_AUTH_REQUEST_TIMEOUT
        );
        assert_eq!(
            request_timeout_for(
                "tools/call",
                &json!({"name": "plugin.notebooklm.get_health", "arguments": {}})
            ),
            REQUEST_TIMEOUT
        );
        assert_eq!(
            request_timeout_for("tools/list", &json!({})),
            REQUEST_TIMEOUT
        );
    }

    #[test]
    fn known_agent_catalog_calls_get_the_long_execution_timeout() {
        assert_eq!(
            request_timeout_for(
                "tools/call",
                &json!({"name": "plugin.cognitive_mcp.agent_python_executor_run", "arguments": {}})
            ),
            AGENT_REQUEST_TIMEOUT
        );
        assert_eq!(
            request_timeout_for(
                "tools/call",
                &json!({"name": "specialist", "arguments": {}})
            ),
            AGENT_REQUEST_TIMEOUT
        );
    }

    #[test]
    fn unknown_agent_names_keep_the_default_timeout() {
        assert_eq!(
            request_timeout_for(
                "tools/call",
                &json!({"name": "plugin.cognitive_mcp.agent_invented_run", "arguments": {}})
            ),
            REQUEST_TIMEOUT
        );
    }

    #[test]
    fn compact_execution_uses_the_target_tool_timeout() {
        assert_eq!(
            request_timeout_for(
                "tools/call",
                &json!({
                    "name": "execute_tool",
                    "arguments": {
                        "tool_name": "plugin.cognitive_mcp.agent_python_executor_run",
                        "arguments": {}
                    }
                })
            ),
            AGENT_REQUEST_TIMEOUT
        );
        assert_eq!(
            request_timeout_for(
                "tools/call",
                &json!({"name": "execute_tool", "arguments": {"tool_name": "echo"}})
            ),
            REQUEST_TIMEOUT
        );
    }

    #[derive(Clone)]
    struct TestAuthenticator;

    #[async_trait]
    impl McpAuthenticator for TestAuthenticator {
        async fn authenticate(
            &self,
            headers: &HeaderMap,
            _peer: Option<SocketAddr>,
        ) -> Result<AuthenticatedCaller, McpAuthError> {
            let identity = headers
                .get("x-test-auth")
                .and_then(|value| value.to_str().ok())
                .filter(|value| matches!(*value, "ok" | "other"));
            let Some(identity) = identity else {
                return Err(McpAuthError("missing test assertion".into()));
            };
            Ok(AuthenticatedCaller {
                principal_id: format!("test-principal-{identity}"),
                session_id: format!("test-session-{identity}"),
                session_genesis: "test-genesis".into(),
                wireguard_origin: None,
            })
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn external_mcp_http_accepts_one_wireguard_identity_and_preserves_admission() {
        use crate::human_principal_dispatch::tests::{pk, temp_shm, test_engine};
        use crate::oracle_assertion::AssertionSourceBinding;
        use crate::oracle_assertion::tests::{
            signed_with_fields, test_issuer, validator_with_registered,
        };

        struct RestoreGrants(Option<std::ffi::OsString>);
        impl Drop for RestoreGrants {
            fn drop(&mut self) {
                match self.0.take() {
                    Some(value) => unsafe { std::env::set_var("OP_GRANTS_PATH", value) },
                    None => unsafe { std::env::remove_var("OP_GRANTS_PATH") },
                }
            }
        }
        let issuer = test_issuer();
        let key = pk(208);
        let (_registry, validator) = validator_with_registered(&issuer, &key).await;
        let _shm = temp_shm();
        let engine = test_engine();
        let grants = tempfile::tempdir().unwrap();
        let grants_path = grants.path().join("grants.json");
        let _restore = RestoreGrants(std::env::var_os("OP_GRANTS_PATH"));
        unsafe { std::env::set_var("OP_GRANTS_PATH", &grants_path) };
        std::fs::write(&grants_path, "{}").unwrap();
        let principal = op_identity::session::derive_principal_id(&key);
        let router = router_with_state(McpFrontendState {
            authenticator: Arc::new(OracleHttpAuthenticator {
                validator: Arc::new(
                    validator.with_source_binding(AssertionSourceBinding::TrustedDecoySignature),
                ),
                engine: engine.clone(),
            }),
            backend: Arc::new(TestBackend::default()),
            allowed_origins: Arc::new(HashSet::new()),
            sessions: Arc::new(RwLock::new(HashMap::new())),
        });
        let now = chrono::Utc::now().timestamp();
        let request = |nonce: u8, ip: &str, session: Option<&str>| {
            let proof = signed_with_fields(
                &issuer,
                &key,
                ip.parse().unwrap(),
                now - 1,
                now + 300,
                [nonce; 16],
                None,
            );
            let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(proof.to_wire());
            let mut builder = Request::builder()
                .method("POST")
                .uri(MCP_PATH)
                .header(CONTENT_TYPE, "application/json")
                .header("accept", "application/json, text/event-stream")
                .header(HTTP_ASSERTION_HEADER, encoded);
            let body = if let Some(session) = session {
                builder = builder
                    .header(MCP_SESSION_HEADER, session)
                    .header("mcp-protocol-version", MCP_PROTOCOL_VERSION);
                json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}})
            } else {
                json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": MCP_PROTOCOL_VERSION}})
            };
            builder.body(Body::from(body.to_string())).unwrap()
        };
        let denied = router
            .clone()
            .oneshot(request(81, "10.0.0.5", None))
            .await
            .unwrap();
        assert_eq!(denied.status(), StatusCode::UNAUTHORIZED);
        assert!(engine.connected_session_ids().await.is_empty());
        std::fs::write(
            &grants_path,
            serde_json::to_vec(&json!({
                principal: {"capabilities": ["cognitive_mcp.read"]}
            }))
            .unwrap(),
        )
        .unwrap();
        // A failed admission did not consume the nonce. There is no SID1, no
        // browser Origin, and no identity container in this request or fixture.
        let initialized = router
            .clone()
            .oneshot(request(81, "10.0.0.5", None))
            .await
            .unwrap();
        assert_eq!(initialized.status(), StatusCode::OK);
        let session = initialized.headers()[MCP_SESSION_HEADER]
            .to_str()
            .unwrap()
            .to_string();
        let body: Value = serde_json::from_slice(
            &to_bytes(initialized.into_body(), MAX_BODY_BYTES)
                .await
                .unwrap(),
        )
        .unwrap();
        assert!(
            body["result"]["_meta"]["opdbus/wireguardOrigin"]
                .as_str()
                .unwrap()
                .starts_with("https://wg-")
        );
        let replay = router
            .clone()
            .oneshot(request(81, "10.0.0.5", None))
            .await
            .unwrap();
        assert_eq!(replay.status(), StatusCode::UNAUTHORIZED);
        let renewed = router
            .clone()
            .oneshot(request(82, "10.0.0.5", Some(&session)))
            .await
            .unwrap();
        assert_eq!(renewed.status(), StatusCode::OK);
        let moved = router
            .clone()
            .oneshot(request(83, "10.0.0.6", Some(&session)))
            .await
            .unwrap();
        assert_eq!(moved.status(), StatusCode::NOT_FOUND);
        std::fs::write(&grants_path, "{}").unwrap();
        let revoked = router
            .oneshot(request(84, "10.0.0.5", Some(&session)))
            .await
            .unwrap();
        assert_eq!(revoked.status(), StatusCode::UNAUTHORIZED);
        assert!(engine.logged_in_principal_sessions(300).await.is_empty());
    }

    #[derive(Clone, Default)]
    struct TestBackend {
        session_closed: Option<Arc<tokio::sync::Notify>>,
    }

    #[async_trait]
    impl McpBackend for TestBackend {
        async fn list_tools(
            &self,
            _caller: &AuthenticatedCaller,
            _selection: Option<&ToolsetSelection>,
        ) -> anyhow::Result<Vec<Value>> {
            Ok(vec![
                json!({
                    "name": "echo",
                    "description": "Echo input",
                    "inputSchema": {"type": "object"}
                }),
                json!({
                    "name": "plugin.cognitive_mcp.code_search",
                    "description": "Search code",
                    "inputSchema": {
                        "type": "object",
                        "properties": {"query": {"type": "string"}},
                        "required": ["query"]
                    }
                }),
            ])
        }

        async fn call_tool(
            &self,
            _caller: &AuthenticatedCaller,
            name: &str,
            arguments: Value,
            selection: Option<&ToolsetSelection>,
        ) -> anyhow::Result<Value> {
            if name == "denied"
                || matches!(
                    name,
                    "list_tools" | "search_tools" | "get_tool_schema" | "execute_tool" | "respond"
                )
            {
                anyhow::bail!("AccessDenied: test backend rejected '{name}'");
            }
            if name == "toolsets"
                && arguments.get("operation").and_then(Value::as_str) == Some("select")
            {
                let id = arguments
                    .get("toolset_id")
                    .and_then(Value::as_str)
                    .unwrap_or("context_code");
                let projected = json!({
                    "operation": "select",
                    "toolset_generation": 5,
                    "relist_required": false,
                    "result": {
                        "selector": {"id": id, "generation": 5},
                        "selected_set": id,
                        "activated_tool_count": 1
                    }
                });
                return Ok(normalize_tool_result(projected));
            }
            Ok(json!({
                "content": [{"type": "text", "text": arguments.to_string()}],
                "structuredContent": {
                    "tool": name,
                    "arguments": arguments,
                    "selection": selection.map(|selection| selection.id.as_str())
                },
                "isError": false
            }))
        }

        async fn list_resources(
            &self,
            _caller: &AuthenticatedCaller,
        ) -> anyhow::Result<Vec<Value>> {
            Ok(vec![json!({
                "uri": "blob://cognitive_mcp",
                "name": "cognitive_mcp schema",
                "mimeType": "application/json"
            })])
        }

        async fn read_resource(
            &self,
            _caller: &AuthenticatedCaller,
            uri: &str,
        ) -> anyhow::Result<Value> {
            Ok(json!({
                "contents": [{"uri": uri, "mimeType": "application/json", "text": "{}"}]
            }))
        }

        async fn session_closed(&self, _caller: &AuthenticatedCaller) -> anyhow::Result<()> {
            if let Some(notify) = &self.session_closed {
                notify.notify_one();
            }
            Ok(())
        }
    }

    fn test_router() -> Router {
        test_router_with_backend(TestBackend::default())
    }

    fn verified_ping_request(auth: Option<&str>) -> Request<Body> {
        let mut builder = Request::builder()
            .method("POST")
            .uri(MCP_PATH)
            .header(CONTENT_TYPE, "application/json")
            .header("accept", "application/json")
            .header(HTTP_SEALED_ID_HEADER, "opaque-test-credential");
        if let Some(auth) = auth {
            builder = builder.header("x-test-auth", auth);
        }
        builder
            .body(Body::from(
                r#"{"jsonrpc":"2.0","id":"sled-auth","method":"ping","params":{}}"#,
            ))
            .unwrap()
    }

    #[tokio::test]
    async fn verified_ping_metadata_is_receiver_authored_and_request_scoped() {
        let router = test_router();
        let (own, other) = tokio::join!(
            router.clone().oneshot(verified_ping_request(Some("ok"))),
            router.clone().oneshot(verified_ping_request(Some("other"))),
        );
        for (response, auth) in [(own.unwrap(), "ok"), (other.unwrap(), "other")] {
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(
                response.headers()[VERIFIED_PRINCIPAL_HEADER],
                format!("test-principal-{auth}")
            );
            assert_eq!(
                response.headers()[VERIFIED_SESSION_HEADER],
                format!("test-session-{auth}")
            );
            assert_eq!(response.headers()[VERIFIED_GENESIS_HEADER], "test-genesis");
            assert!(!response.headers().contains_key(MCP_SESSION_HEADER));
        }
    }

    #[tokio::test]
    async fn verified_ping_metadata_rejects_spoofed_requests_and_is_absent_on_errors() {
        let router = test_router();
        for name in [
            VERIFIED_PRINCIPAL_HEADER,
            VERIFIED_SESSION_HEADER,
            VERIFIED_GENESIS_HEADER,
        ] {
            let mut request = verified_ping_request(Some("ok"));
            request
                .headers_mut()
                .append(name, "foreign".parse().unwrap());
            request
                .headers_mut()
                .append(name, "ambiguous".parse().unwrap());
            let response = router.clone().oneshot(request).await.unwrap();
            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
            for reserved in [
                VERIFIED_PRINCIPAL_HEADER,
                VERIFIED_SESSION_HEADER,
                VERIFIED_GENESIS_HEADER,
            ] {
                assert!(!response.headers().contains_key(reserved));
            }
        }
        let response = router
            .clone()
            .oneshot(verified_ping_request(None))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert!(!response.headers().contains_key(VERIFIED_SESSION_HEADER));
        let mut request = verified_ping_request(Some("ok"));
        request
            .headers_mut()
            .insert(MCP_SESSION_HEADER, "nonexistent".parse().unwrap());
        let response = router.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert!(!response.headers().contains_key(VERIFIED_SESSION_HEADER));
        let mut request = verified_ping_request(Some("ok"));
        request.headers_mut().remove(HTTP_SEALED_ID_HEADER);
        let response = router.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(!response.headers().contains_key(VERIFIED_SESSION_HEADER));
    }

    fn test_router_with_backend(backend: TestBackend) -> Router {
        router_with_state(McpFrontendState {
            authenticator: Arc::new(TestAuthenticator),
            backend: Arc::new(backend),
            allowed_origins: Arc::new(HashSet::from(["https://dashboard.example".into()])),
            sessions: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    async fn initialize_session(router: &Router) -> String {
        initialize_session_at(router, MCP_PATH).await
    }

    async fn initialize_session_at(router: &Router, uri: &str) -> String {
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(uri)
                    .header(CONTENT_TYPE, "application/json")
                    .header("accept", "application/json, text/event-stream")
                    .header("x-test-auth", "ok")
                    .body(Body::from(
                        json!({
                            "jsonrpc": "2.0",
                            "id": 1,
                            "method": "initialize",
                            "params": {"protocolVersion": MCP_PROTOCOL_VERSION}
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        response
            .headers()
            .get(MCP_SESSION_HEADER)
            .and_then(|value| value.to_str().ok())
            .expect("initialize response carries an MCP session")
            .to_string()
    }

    async fn session_call(
        router: &Router,
        session_id: &str,
        method: &str,
        params: Value,
        name: Option<&str>,
    ) -> Response {
        session_call_at(router, session_id, MCP_PATH, "ok", method, params, name).await
    }

    async fn session_call_at(
        router: &Router,
        session_id: &str,
        uri: &str,
        auth: &str,
        method: &str,
        params: Value,
        name: Option<&str>,
    ) -> Response {
        let mut builder = Request::builder()
            .method("POST")
            .uri(uri)
            .header(CONTENT_TYPE, "application/json")
            .header("accept", "application/json, text/event-stream")
            .header("x-test-auth", auth)
            .header(MCP_VERSION_HEADER, MCP_PROTOCOL_VERSION)
            .header(MCP_SESSION_HEADER, session_id)
            .header(MCP_METHOD_HEADER, method);
        if let Some(name) = name {
            builder = builder.header(MCP_NAME_HEADER, name);
        }
        router
            .clone()
            .oneshot(
                builder
                    .body(Body::from(
                        json!({"jsonrpc": "2.0", "id": 2, "method": method, "params": params})
                            .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap()
    }

    async fn call(method: &str, params: Value, name: Option<&str>) -> Response {
        let mut builder = Request::builder()
            .method("POST")
            .uri(MCP_PATH)
            .header(CONTENT_TYPE, "application/json")
            .header("x-test-auth", "ok")
            .header(MCP_VERSION_HEADER, MCP_PROTOCOL_VERSION)
            .header(MCP_METHOD_HEADER, method);
        if let Some(name) = name {
            builder = builder.header(MCP_NAME_HEADER, name);
        }
        test_router()
            .oneshot(
                builder
                    .body(Body::from(
                        json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params})
                            .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap()
    }

    async fn response_json(response: Response) -> Value {
        let bytes = to_bytes(response.into_body(), MAX_BODY_BYTES)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[test]
    fn external_mcp_conversation_cannot_move_to_a_different_wireguard_origin() {
        let mut caller = AuthenticatedCaller {
            principal_id: "same-principal".into(),
            session_id: "same-session".into(),
            session_genesis: "same-genesis".into(),
            wireguard_origin: Some("https://wg-first.opdbus.invalid".into()),
        };
        let session = McpSession::new(&caller, McpView::Typed);
        assert!(session.belongs_to(&caller));
        assert_eq!(session.caller().wireguard_origin, caller.wireguard_origin);
        caller.wireguard_origin = Some("https://wg-other.opdbus.invalid".into());
        assert!(!session.belongs_to(&caller));
        caller.wireguard_origin = None;
        assert!(!session.belongs_to(&caller));
    }

    #[tokio::test]
    async fn native_initialize_succeeds_without_a_shim() {
        let response = call(
            "initialize",
            json!({"protocolVersion": MCP_PROTOCOL_VERSION}),
            None,
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(body["result"]["protocolVersion"], MCP_PROTOCOL_VERSION);
        assert_eq!(body["result"]["serverInfo"]["name"], "op-grpc-bridge");
        assert_eq!(
            body["result"]["capabilities"]["tools"]["listChanged"],
            false
        );
    }

    #[tokio::test]
    async fn initialize_establishes_streamable_http_session() {
        let router = test_router();
        let session_id = initialize_session(&router).await;
        assert!(uuid::Uuid::parse_str(&session_id).is_ok());
    }

    #[tokio::test]
    async fn deleting_session_triggers_background_close_work() {
        let closed = Arc::new(tokio::sync::Notify::new());
        let router = test_router_with_backend(TestBackend {
            session_closed: Some(closed.clone()),
        });
        let session_id = initialize_session(&router).await;

        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(MCP_PATH)
                    .header("x-test-auth", "ok")
                    .header(MCP_VERSION_HEADER, MCP_PROTOCOL_VERSION)
                    .header(MCP_SESSION_HEADER, &session_id)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        tokio::time::timeout(Duration::from_secs(1), closed.notified())
            .await
            .expect("session-close work was scheduled");
    }

    #[tokio::test]
    async fn typed_toolsets_are_listed_up_front_and_selection_admits_calls() {
        let router = test_router();
        let session_id = initialize_session(&router).await;

        let initial =
            response_json(session_call(&router, &session_id, "tools/list", json!({}), None).await)
                .await;
        let initial_names = initial["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|tool| tool.get("name").and_then(Value::as_str))
            .collect::<Vec<_>>();
        assert_eq!(
            initial_names,
            vec!["echo", "plugin.cognitive_mcp.code_search"]
        );

        let selected = response_json(
            session_call(
                &router,
                &session_id,
                "tools/call",
                json!({
                    "name": "toolsets",
                    "arguments": {"operation": "select", "toolset_id": "context_code"}
                }),
                Some("toolsets"),
            )
            .await,
        )
        .await;
        assert_eq!(
            selected["result"]["structuredContent"]["result"]["selected_set"],
            "context_code"
        );
        assert_eq!(
            selected["result"]["structuredContent"]["toolset_generation"],
            5
        );
        assert!(
            selected["result"]["structuredContent"]
                .get("catalog_generation")
                .is_none()
        );
        assert!(
            selected["result"]["structuredContent"]["result"]
                .get("tools")
                .is_none()
        );
        assert_eq!(
            selected["result"]["structuredContent"]["relist_required"],
            false
        );

        let refreshed =
            response_json(session_call(&router, &session_id, "tools/list", json!({}), None).await)
                .await;
        let names = refreshed["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|tool| tool.get("name").and_then(Value::as_str))
            .collect::<Vec<_>>();
        assert_eq!(names, vec!["echo", "plugin.cognitive_mcp.code_search"]);

        let called = response_json(
            session_call(
                &router,
                &session_id,
                "tools/call",
                json!({
                    "name": "plugin.cognitive_mcp.code_search",
                    "arguments": {"query": "sealed schema"}
                }),
                Some("plugin.cognitive_mcp.code_search"),
            )
            .await,
        )
        .await;
        assert_eq!(
            called["result"]["structuredContent"]["selection"],
            "context_code"
        );
    }

    #[tokio::test]
    async fn compact_view_lists_canonical_meta_tools_and_executes_only_through_caller_backend() {
        let router = test_router();
        let uri = "/mcp?opdbus_mode=compact";
        let session_id = initialize_session_at(&router, uri).await;
        let listed = response_json(
            session_call_at(
                &router,
                &session_id,
                uri,
                "ok",
                "tools/list",
                json!({}),
                None,
            )
            .await,
        )
        .await;
        let names = listed["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|tool| tool.get("name").and_then(Value::as_str))
            .collect::<Vec<_>>();
        assert_eq!(
            names,
            vec![
                "list_tools",
                "search_tools",
                "get_tool_schema",
                "execute_tool",
                "respond"
            ]
        );
        assert_eq!(
            listed["result"]["tools"],
            json!(compact_tool_definitions().unwrap())
        );

        let discovered = response_json(
            session_call_at(
                &router,
                &session_id,
                uri,
                "ok",
                "tools/call",
                json!({"name": "search_tools", "arguments": {"query": "code_search"}}),
                Some("search_tools"),
            )
            .await,
        )
        .await;
        assert_eq!(
            discovered["result"]["structuredContent"]["tools"][0]["name"],
            "plugin.cognitive_mcp.code_search"
        );
        let schema = response_json(
            session_call_at(
                &router,
                &session_id,
                uri,
                "ok",
                "tools/call",
                json!({"name": "get_tool_schema", "arguments": {"tool_name": "plugin.cognitive_mcp.code_search"}}),
                Some("get_tool_schema"),
            )
            .await,
        )
        .await;
        assert_eq!(
            schema["result"]["structuredContent"]["schema"]["required"],
            json!(["query"])
        );

        let selected = response_json(
            session_call_at(
                &router,
                &session_id,
                uri,
                "ok",
                "tools/call",
                json!({"name": "execute_tool", "arguments": {"tool_name": "toolsets", "arguments": {"operation": "select", "toolset_id": "context_code"}}}),
                Some("execute_tool"),
            )
            .await,
        )
        .await;
        assert_eq!(
            selected["result"]["structuredContent"]["result"]["selected_set"],
            "context_code"
        );
        let called = response_json(
            session_call_at(
                &router,
                &session_id,
                uri,
                "ok",
                "tools/call",
                json!({"name": "execute_tool", "arguments": {"tool_name": "plugin.cognitive_mcp.code_search", "arguments": {"query": "sealed schema"}}}),
                Some("execute_tool"),
            )
            .await,
        )
        .await;
        assert_eq!(
            called["result"]["structuredContent"]["selection"],
            "context_code"
        );
    }

    #[tokio::test]
    async fn compact_view_rejects_cross_identity_cross_mode_and_backend_denial() {
        let router = test_router();
        let uri = "/mcp?opdbus_mode=compact";
        let compact = initialize_session_at(&router, uri).await;
        let typed = initialize_session(&router).await;

        let other = session_call_at(
            &router,
            &compact,
            uri,
            "other",
            "tools/list",
            json!({}),
            None,
        )
        .await;
        assert_eq!(other.status(), StatusCode::NOT_FOUND);
        let mismatch =
            session_call_at(&router, &typed, uri, "ok", "tools/list", json!({}), None).await;
        assert_eq!(mismatch.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            response_json(mismatch).await["error"]["message"],
            "MCP view differs from initialized session"
        );
        let typed_list =
            response_json(session_call(&router, &typed, "tools/list", json!({}), None).await).await;
        assert_eq!(typed_list["result"]["tools"][0]["name"], "echo");

        let denied = response_json(
            session_call_at(
                &router,
                &compact,
                uri,
                "ok",
                "tools/call",
                json!({"name": "execute_tool", "arguments": {"tool_name": "denied", "arguments": {}}}),
                Some("execute_tool"),
            )
            .await,
        )
        .await;
        assert_eq!(denied["error"]["code"], -32603);
        assert!(
            denied["error"]["message"]
                .as_str()
                .unwrap()
                .contains("AccessDenied")
        );
        let missing = response_json(
            session_call_at(
                &router,
                &compact,
                uri,
                "ok",
                "tools/call",
                json!({"name": "get_tool_schema", "arguments": {"tool_name": "denied"}}),
                Some("get_tool_schema"),
            )
            .await,
        )
        .await;
        assert_eq!(missing["error"]["code"], -32602);
        assert_eq!(missing["error"]["message"], "tool unavailable");

        let unauthenticated = session_call_at(
            &router,
            &compact,
            uri,
            "invalid",
            "tools/list",
            json!({}),
            None,
        )
        .await;
        assert_eq!(unauthenticated.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn native_initialized_notification_is_accepted_without_a_shim() {
        let response = call("notifications/initialized", json!({}), None).await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(body["result"], json!({}));
    }

    #[tokio::test]
    async fn initialize_negotiates_before_protocol_header_exists() {
        let response = test_router()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(MCP_PATH)
                    .header(CONTENT_TYPE, "application/json")
                    .header("x-test-auth", "ok")
                    .body(Body::from(
                        json!({
                            "jsonrpc": "2.0",
                            "id": 1,
                            "method": "initialize",
                            "params": {"protocolVersion": MCP_PROTOCOL_VERSION}
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response_json(response).await["result"]["protocolVersion"],
            MCP_PROTOCOL_VERSION
        );
    }

    #[tokio::test]
    async fn initialize_echoes_codex_official_protocol_version() {
        let response = test_router()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(MCP_PATH)
                    .header(CONTENT_TYPE, "application/json")
                    .header("x-test-auth", "ok")
                    .body(Body::from(
                        json!({
                            "jsonrpc": "2.0",
                            "id": 1,
                            "method": "initialize",
                            "params": {"protocolVersion": "2025-06-18"}
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert!(
            body.get("error").is_none(),
            "Codex handshake must not fail: {body}"
        );
        assert_eq!(body["result"]["protocolVersion"], "2025-06-18");
    }

    #[tokio::test]
    async fn subsequent_request_accepts_codex_protocol_header() {
        let response = test_router()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(MCP_PATH)
                    .header(CONTENT_TYPE, "application/json")
                    .header("x-test-auth", "ok")
                    .header(MCP_VERSION_HEADER, "2025-06-18")
                    .header(MCP_METHOD_HEADER, "tools/list")
                    .body(Body::from(
                        json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert!(
            body.get("error").is_none(),
            "post-initialize Codex requests must be accepted: {body}"
        );
        assert_eq!(body["result"]["tools"][0]["name"], "echo");
    }

    #[tokio::test]
    async fn unknown_protocol_version_down_negotiates_instead_of_erroring() {
        let response = test_router()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(MCP_PATH)
                    .header(CONTENT_TYPE, "application/json")
                    .header("x-test-auth", "ok")
                    .body(Body::from(
                        json!({
                            "jsonrpc": "2.0",
                            "id": 1,
                            "method": "initialize",
                            "params": {"protocolVersion": "2025-11-25"}
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert!(
            body.get("error").is_none(),
            "MCP lifecycle requires down-negotiation, not -32602: {body}"
        );
        assert_eq!(body["result"]["protocolVersion"], "2025-06-18");
    }

    fn principal_record(
        principal_id: &str,
        pubkey: &str,
        revoked_at: i64,
    ) -> op_cozo_store::HumanPrincipalRecord {
        op_cozo_store::HumanPrincipalRecord {
            principal_id: principal_id.into(),
            human_pubkey: pubkey.into(),
            display_alias: String::new(),
            registered_at: 1,
            revoked_at,
        }
    }

    #[test]
    fn sealed_id_requires_an_exact_active_registry_binding() {
        assert!(validate_registered_sealed_id_principal(None, "p", "k").is_err());
        assert!(
            validate_registered_sealed_id_principal(Some(&principal_record("p", "k", 2)), "p", "k")
                .is_err()
        );
        assert!(
            validate_registered_sealed_id_principal(
                Some(&principal_record("other", "k", 0)),
                "p",
                "k"
            )
            .is_err()
        );
        assert!(
            validate_registered_sealed_id_principal(
                Some(&principal_record("p", "other", 0)),
                "p",
                "k"
            )
            .is_err()
        );
        assert!(
            validate_registered_sealed_id_principal(Some(&principal_record("p", "k", 0)), "p", "k")
                .is_ok()
        );
    }

    #[tokio::test]
    async fn tools_list_uses_shared_toolset_backend() {
        let body = response_json(call("tools/list", json!({}), None).await).await;
        assert_eq!(body["result"]["tools"][0]["name"], "echo");
    }

    #[tokio::test]
    async fn resources_list_exposes_blob_resources() {
        let body = response_json(call("resources/list", json!({}), None).await).await;
        assert_eq!(
            body["result"]["resources"][0]["uri"],
            "blob://cognitive_mcp"
        );
    }

    #[tokio::test]
    async fn resources_read_returns_content() {
        let uri = "blob://cognitive_mcp";
        let body =
            response_json(call("resources/read", json!({"uri": uri}), Some(uri)).await).await;
        assert_eq!(body["result"]["contents"][0]["uri"], uri);
    }

    #[tokio::test]
    async fn tools_call_dispatches_named_tool() {
        let body = response_json(
            call(
                "tools/call",
                json!({"name": "echo", "arguments": {"value": 7}}),
                Some("echo"),
            )
            .await,
        )
        .await;
        assert_eq!(body["result"]["structuredContent"]["tool"], "echo");
        assert_eq!(body["result"]["structuredContent"]["arguments"]["value"], 7);
    }

    #[test]
    fn argument_toolset_selector_is_stripped_before_schema_validation() {
        let mut arguments = json!({
            "query": "identity",
            "_meta": {
                "opdbus_toolset": {"id": "context_code", "generation": 3}
            }
        });
        let selection = merged_toolset_selection(&json!({}), &mut arguments)
            .unwrap()
            .unwrap();
        assert_eq!(selection.id, "context_code");
        assert_eq!(selection.generation, 3);
        assert_eq!(arguments, json!({"query": "identity"}));
    }

    #[tokio::test]
    async fn unauthenticated_request_is_rejected_before_dispatch() {
        let response = test_router()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(MCP_PATH)
                    .header(MCP_VERSION_HEADER, MCP_PROTOCOL_VERSION)
                    .header(MCP_METHOD_HEADER, "tools/list")
                    .body(Body::from(
                        json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn browser_origin_is_exact_allowlist_only() {
        let allowed_origin = Request::builder()
            .method("POST")
            .uri(MCP_PATH)
            .header("origin", "https://dashboard.example")
            .header("sec-fetch-mode", "cors")
            .header("x-test-auth", "ok")
            .header(MCP_VERSION_HEADER, MCP_PROTOCOL_VERSION)
            .header(MCP_METHOD_HEADER, "tools/list")
            .body(Body::from(
                json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}).to_string(),
            ))
            .unwrap();
        let response = test_router().oneshot(allowed_origin).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let wrong_origin = Request::builder()
            .method("POST")
            .uri(MCP_PATH)
            .header("origin", "https://evil.example")
            .header("x-test-auth", "ok")
            .body(Body::empty())
            .unwrap();
        let response = test_router().oneshot(wrong_origin).await.unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);

        let missing_browser_origin = Request::builder()
            .method("POST")
            .uri(MCP_PATH)
            .header("sec-fetch-mode", "cors")
            .header("x-test-auth", "ok")
            .body(Body::empty())
            .unwrap();
        let response = test_router().oneshot(missing_browser_origin).await.unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn caller_supplied_capability_header_is_rejected() {
        let response = test_router()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(MCP_PATH)
                    .header("x-test-auth", "ok")
                    .header(MCP_VERSION_HEADER, MCP_PROTOCOL_VERSION)
                    .header(MCP_METHOD_HEADER, "tools/list")
                    .header(DECLARED_CAPABILITY_HEADER, "cognitive_mcp.invoke")
                    .body(Body::from(
                        json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn target_tool_without_authority_metadata_is_fail_closed() {
        let grants = HashSet::from([
            "cognitive_mcp.invoke".to_string(),
            "cognitive_mcp.read".to_string(),
        ]);
        let missing_capability = json!({
            "name": "safe_read",
            "subid": "obs.service.cognitive-mcp.test.read@v1",
            "input_schema": {"type": "object"}
        });
        assert!(
            authorize_and_validate_tool_call(&missing_capability, "safe_read", &json!({}), &grants)
                .is_err()
        );

        let missing_subid = json!({
            "name": "safe_read",
            "required_capability": "cognitive_mcp.read",
            "input_schema": {"type": "object"}
        });
        assert!(
            authorize_and_validate_tool_call(&missing_subid, "safe_read", &json!({}), &grants)
                .is_err()
        );
    }

    #[test]
    fn nested_tool_arguments_are_validated_before_dispatch() {
        let descriptor = json!({
            "name": "safe_read",
            "required_capability": "cognitive_mcp.read",
            "subid": "obs.service.cognitive-mcp.test.read@v1",
            "input_schema": {
                "type": "object",
                "required": ["filter"],
                "properties": {
                    "filter": {
                        "type": "object",
                        "required": ["limit"],
                        "properties": {"limit": {"type": "integer", "minimum": 1}}
                    }
                }
            }
        });
        let grants = HashSet::from([
            "cognitive_mcp.invoke".to_string(),
            "cognitive_mcp.read".to_string(),
        ]);
        assert!(
            authorize_and_validate_tool_call(
                &descriptor,
                "safe_read",
                &json!({"filter": {"limit": 2}}),
                &grants
            )
            .is_ok()
        );
        assert!(
            authorize_and_validate_tool_call(
                &descriptor,
                "safe_read",
                &json!({"filter": {"limit": 0}}),
                &grants
            )
            .is_err()
        );
        assert!(
            authorize_and_validate_tool_call(
                &descriptor,
                "safe_read",
                &json!({"filter": {}}),
                &grants
            )
            .is_err()
        );
    }

    #[test]
    fn target_capability_is_required_in_addition_to_outer_invoke() {
        let descriptor = json!({
            "name": "safe_read",
            "required_capability": "cognitive_mcp.read",
            "subid": "obs.service.cognitive-mcp.test.read@v1",
            "input_schema": {"type": "object"}
        });
        let outer_only = HashSet::from(["cognitive_mcp.invoke".to_string()]);
        let error =
            authorize_and_validate_tool_call(&descriptor, "safe_read", &json!({}), &outer_only)
                .unwrap_err();
        assert!(error.to_string().contains("cognitive_mcp.read"));
    }

    #[test]
    fn tool_list_is_filtered_by_exact_target_capability() {
        let tools = vec![
            json!({
                "name": "safe_read",
                "required_capability": "cognitive_mcp.read",
                "subid": "obs.service.cognitive-mcp.test.read@v1"
            }),
            json!({
                "name": "mutate",
                "required_capability": "cognitive_mcp.invoke",
                "subid": "mut.service.cognitive-mcp.test.write@v1"
            }),
            json!({"name": "missing_descriptor"}),
        ];
        let grants = HashSet::from(["cognitive_mcp.read".to_string()]);
        let filtered = filter_authorized_tools(tools, &grants);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0]["name"], "safe_read");
    }

    #[test]
    fn broad_outer_invoke_is_not_accepted_as_target_authority() {
        let descriptor = json!({
            "name": "code_index",
            "required_capability": "cognitive_mcp.invoke",
            "subid": "mut.service.code-rag.index@v1",
            "input_schema": {"type": "object"}
        });
        let grants = HashSet::from(["cognitive_mcp.invoke".to_string()]);
        let error =
            authorize_and_validate_tool_call(&descriptor, "code_index", &json!({}), &grants)
                .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("no independent sealed capability")
        );
    }

    #[test]
    fn hot_agent_targets_require_their_exact_sealed_capability() {
        let targets = [
            ("rust_pro", "cognitive_mcp.rust_pro"),
            ("python_pro", "cognitive_mcp.python_pro"),
            ("typescript_pro", "cognitive_mcp.typescript_pro"),
            ("sequential_thinking", "cognitive_mcp.sequential_thinking"),
            ("context_manager", "cognitive_mcp.context_manager"),
            (
                "code_review_orchestrator",
                "cognitive_mcp.code_review_orchestrator",
            ),
        ];
        for (name, capability) in targets {
            let descriptor = json!({
                "name": name,
                "required_capability": capability,
                "subid": format!("mut.service.cognitive-mcp.{name}.execute@v1"),
                "input_schema": {"type": "object"}
            });
            let exact = HashSet::from([capability.to_string()]);
            assert!(
                authorize_and_validate_tool_call(&descriptor, name, &json!({}), &exact).is_ok()
            );
            let generic = HashSet::from(["cognitive_mcp.invoke".to_string()]);
            let error = authorize_and_validate_tool_call(&descriptor, name, &json!({}), &generic)
                .unwrap_err();
            assert!(error.to_string().contains(capability));
        }
    }

    #[test]
    fn public_tool_projection_strips_authority_metadata() {
        let projected = normalize_tool_definition(json!({
            "name": "code_search",
            "description": "search",
            "input_schema": {"type": "object"},
            "required_capability": "cognitive_mcp.read",
            "subid": "obs.service.code-rag.search@v1",
            "authority_method": "code_search"
        }));
        assert!(projected.get("inputSchema").is_some());
        for internal in ["required_capability", "subid", "authority_method"] {
            assert!(projected.get(internal).is_none());
        }
    }

    #[test]
    fn safe_read_tool_with_both_grants_passes_policy_and_schema() {
        let descriptor = json!({
            "name": "safe_read",
            "required_capability": "cognitive_mcp.read",
            "subid": "obs.service.cognitive-mcp.test.read@v1",
            "input_schema": {
                "type": "object",
                "required": ["key"],
                "properties": {"key": {"type": "string"}},
                "additionalProperties": false
            }
        });
        let grants = HashSet::from([
            "cognitive_mcp.invoke".to_string(),
            "cognitive_mcp.read".to_string(),
        ]);
        authorize_with_grants(&grants, "cognitive_mcp.invoke").unwrap();
        authorize_and_validate_tool_call(
            &descriptor,
            "safe_read",
            &json!({"key": "status"}),
            &grants,
        )
        .unwrap();
    }

    #[test]
    fn http_assertion_requires_canonical_unpadded_base64url() {
        // Eight bytes force standard base64 to append one `=` padding byte.
        let wire = b"OIA1wire";
        let canonical = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(wire);
        assert_eq!(decode_http_assertion(&canonical).unwrap(), wire);

        let padded = base64::engine::general_purpose::URL_SAFE.encode(wire);
        assert!(decode_http_assertion(&padded).is_err());
        assert!(decode_http_assertion("%%%not-base64url%%%").is_err());
    }

    #[test]
    fn wildcard_grant_never_authorizes_mcp() {
        let document = json!({
            "*": {"capabilities": ["cognitive_mcp.read", "cognitive_mcp.invoke"]},
            "known": {"capabilities": ["cognitive_mcp.read"]}
        });
        assert!(capabilities_for_principal(&document, "unknown").is_empty());
        assert!(capabilities_for_principal(&document, "known").is_empty());
    }

    #[test]
    fn group_membership_adds_capabilities_and_a_group_name_is_not_a_caller() {
        let document = json!({
            "$GUEST": {"capabilities": ["cap.data.cozo.system-graph.read@v1"]},
            "937d6d2b-ecae-ed53-f3a2-d7bd09f544ff": {
                "groups": ["$GUEST"],
                "capabilities": ["cognitive_mcp.read"]
            }
        });
        let granted = capabilities_for_principal(
            &document,
            "937d6d2b-ecae-ed53-f3a2-d7bd09f544ff",
        );
        assert!(granted.contains("cognitive_mcp.read"));
        assert!(granted.contains("cap.data.cozo.system-graph.read@v1"));
        assert!(capabilities_for_principal(&document, "$GUEST").is_empty());
    }

    #[test]
    fn unknown_group_key_invalidates_entire_grant_document() {
        let document = json!({
            "$NOPE": {"capabilities": ["cognitive_mcp.read"]},
            "937d6d2b-ecae-ed53-f3a2-d7bd09f544ff": {
                "capabilities": ["cognitive_mcp.read"]
            }
        });
        assert!(
            capabilities_for_principal(&document, "937d6d2b-ecae-ed53-f3a2-d7bd09f544ff")
                .is_empty()
        );
    }

    #[test]
    fn group_membership_adds_the_group_capabilities() {
        let document = json!({
            "$ROOT": {"capabilities": ["cap.data.cozo.system-graph.read@v1"]},
            "$SUPERADMIN": {"capabilities": ["human_principal.write"]},
            "1262e982-14c1-44d4-0771-7ce74213211b": {
                "groups": ["$ROOT"],
                "capabilities": ["chat_response.respond"]
            }
        });
        let chatbot = capabilities_for_principal(
            &document,
            "1262e982-14c1-44d4-0771-7ce74213211b",
        );
        assert!(chatbot.contains("cap.data.cozo.system-graph.read@v1"));
        assert!(chatbot.contains("chat_response.respond"));
        assert!(!chatbot.contains("human_principal.write"));
        assert!(capabilities_for_principal(&document, "$ROOT").is_empty());
    }

    #[test]
    fn legacy_footprint_key_invalidates_entire_grant_document() {
        let document = json!({
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef": {
                "capabilities": ["cognitive_mcp.invoke"]
            },
            "937d6d2b-ecae-ed53-f3a2-d7bd09f544ff": {
                "capabilities": ["cognitive_mcp.read"]
            }
        });
        assert!(
            capabilities_for_principal(&document, "937d6d2b-ecae-ed53-f3a2-d7bd09f544ff")
                .is_empty()
        );
    }

    #[test]
    fn schema_sanitizer_removes_embedded_identity_grants() {
        let mut schema = json!({
            "capability_grants": {"*": ["mcp.write"]},
            "principal_membership": ["did:op:human:secret"],
            "internal": {
                "socket_path": "/run/private.sock",
                "decoy_key_id": "issuer-1",
                "oauth_token": "secret",
                "approval_verifier_config": {"key": "value"}
            },
            "methods": {"read": {
                "required_capability": "mcp.read",
                "args": {"properties": {"path": {"type": "string"}}}
            }}
        });
        sanitize_schema(&mut schema);
        assert!(schema.get("capability_grants").is_none());
        assert!(schema.get("principal_membership").is_none());
        assert!(schema["internal"].get("socket_path").is_none());
        assert!(schema["internal"].get("decoy_key_id").is_none());
        assert!(schema["internal"].get("oauth_token").is_none());
        assert!(schema["internal"].get("approval_verifier_config").is_none());
        assert_eq!(schema["methods"]["read"]["required_capability"], "mcp.read");
        assert_eq!(
            schema["methods"]["read"]["args"]["properties"]["path"]["type"],
            "string"
        );
    }

    #[test]
    fn typed_methods_are_loaded_from_manifest_pinned_sealed_blob() {
        let dir = tempfile::tempdir().unwrap();
        let expected = op_plugins::cognitive_mcp_plugin_schema();
        let blob = op_blob::blobify_plugin_schema("cognitive_mcp", expected.clone());
        let mut store = op_blob::BlobStore::open(dir.path()).unwrap();
        store.write(&blob).unwrap();

        let restored = read_manifest_pinned_plugin_schema(dir.path(), "cognitive_mcp").unwrap();
        assert_eq!(restored.name, "cognitive_mcp");
        assert_eq!(
            restored.methods["toolsets"].subid,
            expected.methods["toolsets"].subid
        );
        let zero_arg = method_descriptor(
            &restored,
            "memory_list_namespaces",
            "plugin.cognitive_mcp.memory_list_namespaces",
        )
        .unwrap();
        assert_eq!(zero_arg["inputSchema"]["type"], "object");
        validate_tool_arguments(&zero_arg, &json!({})).unwrap();
        assert!(read_manifest_pinned_plugin_schema(dir.path(), "missing").is_err());
    }

    #[test]
    fn sealed_schema_reader_returns_raw_schema_and_cross_blob_oscal_tags() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = op_blob::BlobStore::open(dir.path()).unwrap();

        let mut snowball = op_plugins::cognitive_mcp_plugin_schema();
        snowball.name = "snowball".to_string();
        snowball.description = "test snowball schema".to_string();
        snowball.subids.clear();
        snowball.subids.insert(
            "archive_message".to_string(),
            "evt.service.snowball.message.archive@v1".to_string(),
        );
        let expected_snowball = serde_json::to_value(&snowball).unwrap();
        store
            .write(&op_blob::blobify_plugin_schema("snowball", snowball))
            .unwrap();

        let mut registry = op_plugins::cognitive_mcp_plugin_schema();
        registry.name = "oscal_subid_registry".to_string();
        registry.description = "test OSCAL registry schema".to_string();
        registry.subids.clear();
        registry.subids.insert(
            "route_message".to_string(),
            "obs.standard.oscal-subid-registry.route.message@v1".to_string(),
        );
        store
            .write(&op_blob::blobify_plugin_schema(
                "oscal_subid_registry",
                registry,
            ))
            .unwrap();

        let result = read_sealed_schema_result(dir.path(), "snowball").unwrap();
        assert_eq!(result["plugin_id"], "snowball");
        assert_eq!(result["uri"], "blob://snowball");
        assert_eq!(result["schema"], expected_snowball);
        assert_eq!(result["schema_hash"].as_str().unwrap().len(), 64);
        assert!(
            result["oscal_subids"]
                .as_array()
                .unwrap()
                .iter()
                .any(|value| value == "evt.service.snowball.message.archive@v1")
        );

        let all = read_oscal_subids_result(dir.path(), None, None).unwrap();
        assert_eq!(all["plugin_count"], 2);
        assert!(all["subids"].as_array().unwrap().iter().any(|entry| {
            entry["subid"] == "evt.service.snowball.message.archive@v1"
                && entry["sources"][0]["plugin_id"] == "snowball"
        }));
        assert!(all["subids"].as_array().unwrap().iter().any(|entry| {
            entry["subid"] == "obs.standard.oscal-subid-registry.route.message@v1"
                && entry["sources"][0]["plugin_id"] == "oscal_subid_registry"
        }));

        let filtered =
            read_oscal_subids_result(dir.path(), Some("snowball"), Some("evt.service.snowball"))
                .unwrap();
        assert_eq!(filtered["plugin_count"], 1);
        assert_eq!(filtered["subid_count"], 1);
        assert!(read_sealed_schema_result(dir.path(), "missing").is_err());
    }

    fn deployed_mcp_policy() -> McpProjectionPolicy {
        McpProjectionPolicy {
            audience: crate::mcp_policy::parse_audience_policy(include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../deploy/config/mcp-audience-policy.json"
            )))
            .unwrap(),
            toolsets: crate::mcp_policy::parse_toolset_manifest(include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../deploy/config/mcp-toolsets.json"
            )))
            .unwrap(),
        }
    }

    #[tokio::test]
    async fn non_singleton_identity_still_requires_hot_or_selected_toolset() {
        let mut backend = MutationEngineMcpBackend::new(
            crate::human_principal_dispatch::tests::test_engine(),
            McpPolicyOwner::for_fixture(deployed_mcp_policy()),
        );
        let catalog_root = tempfile::tempdir().unwrap();
        backend.blob_catalog_dir = catalog_root.path().join("catalog-does-not-exist");
        let policy = backend.policy.snapshot();
        let caller = AuthenticatedCaller {
            principal_id: "ordinary-principal".into(),
            session_id: "ordinary-session".into(),
            session_genesis: "ordinary-genesis".into(),
            wireguard_origin: None,
        };
        let unknown = "plugin.unlisted.operation";
        let error = backend
            .call_tool(&caller, unknown, json!({}), None)
            .await
            .unwrap_err();
        assert_eq!(
            error.to_string(),
            format!("AccessDenied: tool '{unknown}' is not exposed by the configured MCP toolsets")
        );
        for set in &policy.toolsets.sets {
            let selection = ToolsetSelection {
                id: set.id.clone(),
                generation: policy.toolsets.generation,
            };
            let outside = policy
                .toolsets
                .sets
                .iter()
                .flat_map(|other| other.tools.iter().map(String::as_str))
                .chain(std::iter::once(unknown))
                .filter(|name| {
                    !policy.is_hot_tool(name)
                        && !set.tools.iter().any(|tool| tool.as_str() == *name)
                });
            for name in outside {
                let error = backend
                    .call_tool(&caller, name, json!({}), Some(&selection))
                    .await
                    .unwrap_err();
                assert_eq!(
                    error.to_string(),
                    format!(
                        "AccessDenied: tool '{name}' is not in the current HOT/toolset projection"
                    ),
                    "selection {}",
                    set.id
                );
            }
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn singleton_projection_uses_exact_grants_and_sealed_typed_dispatch() {
        // A projection-only invented session cannot pass the real D-Bus owner.
        // Use a genuine verified receiver term while preserving singleton-role
        // projection assertions through an isolated fixture audience policy.
        let _registry = crate::human_principal_dispatch::tests::temp_cozo();
        let peer = crate::human_principal_dispatch::tests::pk(213);
        crate::human_principal_dispatch::tests::register(&peer, "singleton-read-fixture")
            .await
            .unwrap();
        let principal = op_identity::session::derive_principal_id(&peer);
        let mut policy = deployed_mcp_policy();
        policy.audience.singleton_chatbot_principal_id = principal.clone();
        let snowball_schema = op_state::StatePlugin::schema(
            &op_plugins::state_plugins::snowball_plugin::SnowballPlugin::default(),
        )
        .expect("Snowball schema");
        let mut gap_schema = snowball_schema.clone();
        gap_schema.name = "snowball_gap".into();
        gap_schema
            .methods
            .get_mut("query_events")
            .unwrap()
            .required_capability = Some("snowball_gap.unsealed".into());
        let router_schema = op_plugins::state_plugins::tched_router::tched_router_plugin_schema();
        let router_selection_capability = router_schema.methods["SetSelection"]
            .required_capability
            .clone()
            .unwrap();
        // This shared fixture redirects both the MCP frontend and MutationEngine
        // away from live SHM. Keep it for the entire test lifetime.
        let _shm = crate::human_principal_dispatch::tests::temp_shm_with_schemas(&[
            op_plugins::cognitive_mcp_plugin_schema(),
            snowball_schema,
            router_schema.clone(),
            gap_schema,
        ]);
        let engine = crate::human_principal_dispatch::tests::test_engine();
        let fixture = tempfile::tempdir().unwrap();
        let documents = Arc::new(
            op_plugins::state_plugins::runtime_documents::RuntimeDocuments::new(
                op_cozo_store::CozoGraphShuttle::new_persistent_private(
                    fixture.path().join("journal"),
                )
                .unwrap(),
            ),
        );
        engine
            .configure_method_journal_documents(documents)
            .unwrap();
        engine
            .configure_cognitive_runtime_fixture(Arc::new(
                op_cognitive_mcp::CognitiveMcpServer::new_in_memory()
                    .await
                    .unwrap(),
            ))
            .unwrap();
        engine
            .init_audit_durability_at(fixture.path().join("audit").to_str().unwrap())
            .await;
        let mut backend =
            MutationEngineMcpBackend::new(engine.clone(), McpPolicyOwner::for_fixture(policy));

        let grants = Arc::new(std::sync::RwLock::new(HashMap::from([(
            principal.clone(),
            HashSet::from([
                "cognitive_mcp.read".to_string(),
                "snowball.read".to_string(),
                "snowball_gap.unsealed".to_string(),
                router_selection_capability,
            ]),
        )])));
        backend.test_grants = Some(Arc::clone(&grants));

        struct GrantPath(Option<std::ffi::OsString>);
        impl Drop for GrantPath {
            fn drop(&mut self) {
                match self.0.take() {
                    Some(path) => unsafe { std::env::set_var("OP_GRANTS_PATH", path) },
                    None => unsafe { std::env::remove_var("OP_GRANTS_PATH") },
                }
            }
        }
        let _grant_path = GrantPath(std::env::var_os("OP_GRANTS_PATH"));
        let real_grants = fixture.path().join("grants.json");
        unsafe { std::env::set_var("OP_GRANTS_PATH", &real_grants) };
        let capability_list = grants.read().unwrap()[&principal]
            .iter()
            .cloned()
            .collect::<Vec<_>>();
        std::fs::write(
            &real_grants,
            serde_json::to_vec(&json!({
                principal.clone(): {"capabilities": capability_list}
            }))
            .unwrap(),
        )
        .unwrap();
        let issuer = crate::oracle_assertion::tests::test_issuer();
        let validator = Arc::new(
            AssertionValidator::new(crate::oracle_assertion::tests::trust_store_for_issuer(
                &issuer,
            ))
            .with_source_binding(
                crate::oracle_assertion::AssertionSourceBinding::TrustedDecoySignature,
            ),
        );
        let now = chrono::Utc::now().timestamp();
        let proof = crate::oracle_assertion::tests::signed_with_fields(
            &issuer,
            &peer,
            "10.0.0.5".parse().unwrap(),
            now - 1,
            now + 300,
            [82; 16],
            None,
        );
        let mut headers = HeaderMap::new();
        headers.insert(
            HTTP_ASSERTION_HEADER,
            base64::engine::general_purpose::URL_SAFE_NO_PAD
                .encode(proof.to_wire())
                .parse()
                .unwrap(),
        );
        let caller = authenticate_dbus_fixture(engine, validator, &headers)
            .await
            .unwrap();

        let tools = backend.list_tools(&caller, None).await.unwrap();
        let call_name = "plugin.snowball.verify_chain";
        assert!(
            !backend
                .policy
                .snapshot()
                .toolsets
                .sets
                .iter()
                .any(|set| set.tools.iter().any(|name| name == call_name)),
            "fixture operation must be outside every broker toolset"
        );
        assert!(tools.iter().any(|tool| tool_name(tool) == call_name));
        let selection_name = "plugin.tched_router.SetSelection";
        assert!(tools.iter().any(|tool| tool_name(tool) == selection_name));
        assert_eq!(
            parse_typed_tool_name(selection_name).unwrap(),
            ("tched_router", "SetSelection")
        );
        assert_eq!(
            backend.typed_tool_descriptor(selection_name).await.unwrap()["authority_method"],
            "SetSelection",
            "case-preserving schema resolution must reach dispatch metadata"
        );
        assert!(
            !tools
                .iter()
                .any(|tool| tool_name(tool) == "plugin.snowball_gap.query_events"),
            "schema-closure gap is not discoverable"
        );
        assert!(
            !tools.iter().any(|tool| descriptor_authority(tool, None)
                .is_ok_and(|(capability, _)| capability == "cognitive_mcp.invoke")),
            "generic invocation is never projected"
        );
        let called = backend
            .call_tool(&caller, call_name, json!({}), None)
            .await
            .unwrap();
        assert_ne!(called.get("isError"), Some(&Value::Bool(true)));

        // Revoking after discovery is effective both at projection lookup and
        // at the dispatch boundary, which reloads grants before admission.
        let descriptors = backend.authorized_singleton_tools(&caller).await.unwrap();
        let descriptor = descriptors
            .iter()
            .find(|tool| tool_name(tool) == call_name)
            .unwrap();
        grants.write().unwrap().clear();
        std::fs::write(&real_grants, "{}").unwrap();
        let error = backend
            .dispatch_projected_tool(&caller, descriptor, call_name, json!({}))
            .await
            .unwrap_err();
        assert!(error.to_string().contains("lacks required capability"));
        let error = backend
            .call_tool(&caller, call_name, json!({}), None)
            .await
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("not in the singleton principal's granted sealed projection")
        );

        // A wildcard is never an identity grant; only the caller's exact key
        // produces a projection.
        grants.write().unwrap().insert(
            caller.principal_id.clone(),
            HashSet::from(["cognitive_mcp.read".to_string()]),
        );
        grants
            .write()
            .unwrap()
            .insert("*".into(), HashSet::from(["snowball.read".to_string()]));
        let wildcard = backend.authorized_singleton_tools(&caller).await.unwrap();
        assert!(!wildcard.iter().any(|tool| tool_name(tool) == call_name));
        // The public path also rechecks the real owner, whose revoked grants
        // remain empty. Projection-only fixture entries cannot restore admission.
        let error = backend.list_tools(&caller, None).await.unwrap_err();
        assert!(error.to_string().contains("no capabilities"));
        let error = backend
            .call_tool(&caller, "plugin.snowball_gap.query_events", json!({}), None)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("granted sealed projection"));
        let error = backend
            .call_tool(&caller, "plugin.unsealed.nope", json!({}), None)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("granted sealed projection"));
    }

    #[tokio::test]
    async fn singleton_chatbot_direct_tools_are_denied_before_catalog_or_provider_work() {
        let mut backend = MutationEngineMcpBackend::new(
            crate::human_principal_dispatch::tests::test_engine(),
            McpPolicyOwner::for_fixture(deployed_mcp_policy()),
        );
        let catalog_root = tempfile::tempdir().unwrap();
        backend.blob_catalog_dir = catalog_root.path().join("catalog-does-not-exist");
        let policy = backend.policy.snapshot();
        let caller = AuthenticatedCaller {
            principal_id: policy.audience.singleton_chatbot_principal_id.clone(),
            session_id: "chatbot-session".into(),
            session_genesis: "chatbot-genesis".into(),
            wireguard_origin: None,
        };
        let names = [
            "shell_execute",
            "shell_read",
            "file_read",
            "file_write",
            "dbus_call",
            "plugin.example.shell_execute",
            "plugin.example.file_read",
            "plugin.example.dbus_call",
        ];
        let selections = policy
            .toolsets
            .sets
            .iter()
            .map(|set| ToolsetSelection {
                id: set.id.clone(),
                generation: policy.toolsets.generation,
            })
            .collect::<Vec<_>>();

        for name in names {
            let error = backend
                .call_tool(&caller, name, json!({}), None)
                .await
                .unwrap_err();
            let expected = format!(
                "AccessDenied: singleton chatbot cannot call direct-execution tool '{name}'"
            );
            assert_eq!(error.to_string(), expected);

            for selection in &selections {
                let error = backend
                    .call_tool(&caller, name, json!({}), Some(selection))
                    .await
                    .unwrap_err();
                assert_eq!(error.to_string(), expected, "selection {}", selection.id);
            }
        }
    }

    fn plain_post(uri: &str, session_id: Option<&str>, auth: &str, body: Value) -> Request<Body> {
        let mut builder = Request::builder()
            .method("POST")
            .uri(uri)
            .header(CONTENT_TYPE, "application/json")
            .header("x-test-auth", auth);
        if let Some(session_id) = session_id {
            builder = builder.header(MCP_SESSION_HEADER, session_id);
        }
        builder.body(Body::from(body.to_string())).unwrap()
    }

    /// Read SSE text until it contains `needle` (bounded wait).
    async fn read_sse_until(
        stream: &mut (impl futures::Stream<Item = Result<axum::body::Bytes, axum::Error>> + Unpin),
        seen: &mut String,
        needle: &str,
    ) {
        use futures::StreamExt;
        while !seen.contains(needle) {
            let chunk = tokio::time::timeout(Duration::from_secs(5), stream.next())
                .await
                .expect("SSE event within 5s")
                .expect("SSE stream open")
                .unwrap();
            seen.push_str(std::str::from_utf8(&chunk).unwrap());
        }
    }

    #[tokio::test]
    async fn requests_without_protocol_header_assume_the_default_revision() {
        let router = test_router();
        let session_id = initialize_session(&router).await;
        let response = router
            .clone()
            .oneshot(plain_post(
                MCP_PATH,
                Some(&session_id),
                "ok",
                json!({"jsonrpc": "2.0", "id": 7, "method": "ping"}),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response_json(response).await["id"], 7);
    }

    #[tokio::test]
    async fn batches_answer_each_request_and_skip_notifications() {
        let router = test_router();
        let session_id = initialize_session(&router).await;
        let response = router
            .clone()
            .oneshot(plain_post(
                MCP_PATH,
                Some(&session_id),
                "ok",
                json!([
                    {"jsonrpc": "2.0", "id": 1, "method": "ping"},
                    {"jsonrpc": "2.0", "method": "notifications/initialized"},
                    {"jsonrpc": "2.0", "id": 2, "method": "ping"}
                ]),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        let ids = body
            .as_array()
            .unwrap()
            .iter()
            .map(|reply| reply["id"].clone())
            .collect::<Vec<_>>();
        assert_eq!(ids, vec![json!(1), json!(2)]);

        let batched_initialize = router
            .clone()
            .oneshot(plain_post(
                MCP_PATH,
                None,
                "ok",
                json!([{"jsonrpc": "2.0", "id": 1, "method": "initialize",
                        "params": {"protocolVersion": "2025-03-26"}}]),
            ))
            .await
            .unwrap();
        assert_eq!(batched_initialize.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn client_replies_are_acknowledged() {
        let router = test_router();
        let session_id = initialize_session(&router).await;
        let response = router
            .clone()
            .oneshot(plain_post(
                MCP_PATH,
                Some(&session_id),
                "ok",
                json!({"jsonrpc": "2.0", "id": "s-1", "result": {}}),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::ACCEPTED);
    }

    #[tokio::test]
    async fn http_sse_clients_use_the_same_endpoint() {
        let router = test_router();
        let stream = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(MCP_PATH)
                    .header("accept", "text/event-stream")
                    .header("x-test-auth", "ok")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(stream.status(), StatusCode::OK);
        let mut events = stream.into_body().into_data_stream();
        let mut seen = String::new();
        read_sse_until(&mut events, &mut seen, "\n\n").await;
        assert!(seen.contains("event: endpoint"), "{seen}");
        let endpoint = seen
            .lines()
            .find_map(|line| line.strip_prefix("data: "))
            .expect("endpoint data")
            .trim()
            .to_string();
        assert!(endpoint.starts_with("/mcp?sessionId="), "{endpoint}");

        let initialize = router
            .clone()
            .oneshot(plain_post(
                &endpoint,
                None,
                "ok",
                json!({"jsonrpc": "2.0", "id": 1, "method": "initialize",
                       "params": {"protocolVersion": "2024-11-05"}}),
            ))
            .await
            .unwrap();
        assert_eq!(initialize.status(), StatusCode::ACCEPTED);
        read_sse_until(&mut events, &mut seen, "\"protocolVersion\"").await;
        assert!(seen.contains("event: message"), "{seen}");
        assert!(seen.contains("2024-11-05"), "{seen}");

        let ping = router
            .clone()
            .oneshot(plain_post(
                &endpoint,
                None,
                "ok",
                json!({"jsonrpc": "2.0", "id": 42, "method": "ping"}),
            ))
            .await
            .unwrap();
        assert_eq!(ping.status(), StatusCode::ACCEPTED);
        read_sse_until(&mut events, &mut seen, "\"id\":42").await;

        // Another identity cannot post into, or read from, this session.
        let foreign = router
            .clone()
            .oneshot(plain_post(
                &endpoint,
                None,
                "other",
                json!({"jsonrpc": "2.0", "id": 9, "method": "ping"}),
            ))
            .await
            .unwrap();
        assert_eq!(foreign.status(), StatusCode::NOT_FOUND);
    }
}
