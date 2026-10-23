//! Standalone local EMQX broker plugin — GB.Emqx.
//!
//! This PluginSchema is the single typed present-state/control contract used
//! by D-Bus and generated/generic gRPC. EMQX ExHook callbacks remain a
//! separate `emqx.exhook.v3.HookProvider` protocol in `op-grpc-bridge`.
//! EMQX is not owned by, installed in, or coupled to Netmaker.

use super::plugin_scaffold_helpers::method_decl_from_schemars_with_output;
use anyhow::{Context, Result};
use async_trait::async_trait;
use op_state::{ApplyResult, Checkpoint, DiffMetadata, PluginCapabilities, StateDiff, StatePlugin};
use op_state_store::{CapabilityDecl, PluginSchema, SideEffect, SignalDecl};
use reqwest::Method;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use simd_json::OwnedValue as Value;
use std::collections::BTreeSet;
use std::net::SocketAddr;
use std::time::Duration;
use zbus::{Connection, Proxy};

const PLUGIN_NAME: &str = "emqx";
const PLUGIN_VERSION: &str = "2.1.0";
const PLUGIN_CATEGORY: &str = "net";
const PLUGIN_DESCRIPTION: &str =
    "Standalone local EMQX broker: status, listeners, hooks, and fixed-service lifecycle";
const PLUGIN_DISPLAY_NAME: &str = "GB.Emqx";

pub const EMQX_RELEASE_VERSION: &str = "6.2.2";
pub const EMQX_SERVICE: &str = "emqx";
pub const EMQX_DATA_DIR: &str = "/var/lib/emqx";
pub const EMQX_CONFIG_DIR: &str = "/etc/emqx";
pub const EMQX_LOG_DIR: &str = "/var/log/emqx";
pub const EMQX_DASHBOARD_URL: &str = "http://127.0.0.1:18083";
pub const EMQX_API_BASE: &str = "http://127.0.0.1:18083/api/v5";
pub const EMQX_MQTT_TCP_BIND: &str = "127.0.0.1:1883";
/// Private WSS backend for the identity-gated wake relay on `10.0.0.2:8090`.
pub const EMQX_MQTT_WSS_HOST: &str = "127.0.0.1";
pub const EMQX_MQTT_WSS_PORT: u16 = 8085;
pub const EMQX_MQTT_WSS_BIND: &str = "127.0.0.1:8085";
/// Content-free wake topics. `<owner-key>` is the private-view owner key
/// (`sha256(principal, session, genesis)`), so the broker never sees an
/// identity. Payload is `{"seq": <scope revision>}` only.
pub const WAKE_TOPIC_PREFIX: &str = "opdbus/wake/";
/// Transport scope a SID1 must carry to open the doorbell.
pub const WAKE_TRANSPORT_SCOPE: &str = "mqtt";

/// `opdbus/wake/<owner-key>` for a valid owner key.
pub fn wake_topic(owner_key: &str) -> Result<String> {
    anyhow::ensure!(
        valid_owner_key(owner_key),
        "wake owner key must be 64 lowercase hex characters"
    );
    Ok(format!("{WAKE_TOPIC_PREFIX}{owner_key}"))
}

/// Retained topic carrying the toolset manifest derived from the sealed
/// catalog. It holds names only: which tools a connection may call is still
/// decided per call by the identity's grants.
pub const TOOLSETS_TOPIC: &str = "opdbus/toolsets";
const MAX_TOOLSETS: usize = 512;
const MAX_ANNOUNCED_TOOLS: usize = 16_384;
const MAX_TOOLSETS_PAYLOAD: usize = 1024 * 1024;

fn valid_toolset_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
}

pub fn valid_owner_key(owner_key: &str) -> bool {
    owner_key.len() == 64
        && owner_key
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
pub const EXHOOK_PROTO: &str = "emqx.exhook.v3";
pub const EXHOOK_SERVICE: &str = "emqx.exhook.v3.HookProvider";
pub const EXHOOK_SERVER_NAME: &str = "opdbus";
pub const EXHOOK_TARGET: &str = "https://127.0.0.1:9000";

const API_KEY_FILE: &str = "/etc/opdbus/secrets/emqx-api-key";
const API_SECRET_FILE: &str = "/etc/opdbus/secrets/emqx-api-secret";
const RUNIT_BUS_NAME: &str = "org.opdbus.v1.plugins.runit";
const RUNIT_OBJECT_PATH: &str = "/org/opdbus/v1/plugins/runit";
const RUNIT_INTERFACE: &str = "org.opdbus.v1.plugins.runit";
const API_RESPONSE_LIMIT: usize = 1024 * 1024;

/// HookSpec names advertised by the bridge. Configuration accepts exactly this
/// set because the ExHook protocol does not support a caller-selected subset.
pub const REGISTERED_HOOKS: &[&str] = &[
    "client.connect",
    "client.connack",
    "client.connected",
    "client.disconnected",
    "client.authenticate",
    "client.authorize",
    "client.subscribe",
    "client.unsubscribe",
    "session.created",
    "session.subscribed",
    "session.unsubscribed",
    "session.resumed",
    "session.discarded",
    "session.takenover",
    "session.terminated",
    "message.publish",
    "message.delivered",
    "message.acked",
    "message.dropped",
];

/// Metadata-only observation of the authenticated broker's ExHook callback.
/// References are digests, not MQTT identities or reusable credentials. Broker
/// message bodies must never enter the globally visible accountability stream.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct BrokerEvent {
    pub event_type: String,
    pub client_ref: String,
    pub topic_ref: Option<String>,
    pub detail_ref: Option<String>,
}

pub const BROKER_EVENT_SIGNAL: &str = "broker_event";
pub const BROKER_EVENT_SUBID: &str = "evt.network.emqx.broker-event@v1";

fn typed_input<T: serde::de::DeserializeOwned>(args: &JsonValue) -> Result<T> {
    serde_json::from_value(if args.is_null() {
        JsonValue::Object(Default::default())
    } else {
        args.clone()
    })
    .map_err(|error| anyhow::anyhow!("emqx method input does not match its schema: {error}"))
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct EmqxDeclaredListener {
    pub id: String,
    pub r#type: String,
    pub bind: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct EmqxExhookDeclaration {
    pub proto: String,
    pub service: String,
    pub server_name: String,
    pub target: String,
    pub transport: String,
    pub registered_hooks: Vec<String>,
}

/// Deterministic declared state. No live probe or credential is allowed here;
/// schema generation and blob sealing must produce identical output offline.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[schemars(extend("x-oscal-subid" = "sch.software.plugin.emqx.schema@v3"))]
#[schemars(extend("x-oscal-category" = "network"))]
#[schemars(extend("x-immutable-paths" = ["software", "service_name", "data_dir", "config_dir", "log_dir", "api_base"]))]
pub struct EmqxState {
    pub software: String,
    pub plugin_version: String,
    pub broker_version: String,
    pub service_name: String,
    pub data_dir: String,
    pub config_dir: String,
    pub log_dir: String,
    pub dashboard_url: String,
    pub api_base: String,
    pub listeners: Vec<EmqxDeclaredListener>,
    pub exhook: EmqxExhookDeclaration,
    /// EMQX's MCP Bridge is intentionally not installed. The only external
    /// MCP endpoint is the bridge-owned `https://10.0.0.2:8090/mcp`.
    pub mcp_gateway_installed: bool,
}

impl EmqxState {
    pub fn declared() -> Self {
        Self {
            software: "emqx".to_string(),
            plugin_version: PLUGIN_VERSION.to_string(),
            broker_version: EMQX_RELEASE_VERSION.to_string(),
            service_name: EMQX_SERVICE.to_string(),
            data_dir: EMQX_DATA_DIR.to_string(),
            config_dir: EMQX_CONFIG_DIR.to_string(),
            log_dir: EMQX_LOG_DIR.to_string(),
            dashboard_url: EMQX_DASHBOARD_URL.to_string(),
            api_base: EMQX_API_BASE.to_string(),
            listeners: vec![
                EmqxDeclaredListener {
                    id: "tcp:default".to_string(),
                    r#type: "tcp".to_string(),
                    bind: EMQX_MQTT_TCP_BIND.to_string(),
                },
                EmqxDeclaredListener {
                    id: "wss:wake".to_string(),
                    r#type: "wss".to_string(),
                    bind: EMQX_MQTT_WSS_BIND.to_string(),
                },
            ],
            exhook: EmqxExhookDeclaration {
                proto: EXHOOK_PROTO.to_string(),
                service: EXHOOK_SERVICE.to_string(),
                server_name: EXHOOK_SERVER_NAME.to_string(),
                target: EXHOOK_TARGET.to_string(),
                transport: "loopback_mtls".to_string(),
                registered_hooks: REGISTERED_HOOKS
                    .iter()
                    .map(|hook| (*hook).to_string())
                    .collect(),
            },
            mcp_gateway_installed: false,
        }
    }
}

pub struct EmqxPlugin;

impl EmqxPlugin {
    pub fn new() -> Self {
        Self
    }

    pub fn current_state() -> EmqxState {
        EmqxState::declared()
    }
}

impl Default for EmqxPlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone)]
struct EmqxNativeTransport {
    runit_connection: Option<Connection>,
    api_base: String,
    api_credentials: Option<(String, String)>,
}

impl EmqxNativeTransport {
    fn production() -> Self {
        Self {
            runit_connection: None,
            api_base: EMQX_API_BASE.to_string(),
            api_credentials: None,
        }
    }

    async fn runit_connection(&self) -> Result<Connection> {
        match &self.runit_connection {
            Some(connection) => Ok(connection.clone()),
            None => Connection::system()
                .await
                .context("runit D-Bus boundary is unavailable"),
        }
    }

    fn credentials(&self) -> Result<(String, String)> {
        match &self.api_credentials {
            Some(credentials) => Ok(credentials.clone()),
            None => api_credentials_from_files(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct GetStatusInput {}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GetStatusOutput {
    pub running: bool,
    pub pid: Option<u32>,
    pub uptime_seconds: Option<u64>,
    pub version: String,
    pub node_name: String,
    pub cluster_status: String,
    pub data_dir: String,
    pub config_dir: String,
    pub listeners_active: usize,
    pub connections_current: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct GetMcpStatusInput {}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GetMcpStatusOutput {
    pub gateway_installed: bool,
    pub gateway_enabled: bool,
    pub gateway_version: Option<String>,
    pub topic_prefix: Option<String>,
    pub internal_transport_only: bool,
    pub external_mcp_endpoint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct EmqxListener {
    pub id: String,
    pub r#type: String,
    pub bind: String,
    pub running: bool,
    pub current_connections: u64,
    pub max_connections: u64,
    pub is_loopback_or_uds: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct ListListenersInput {}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ListListenersOutput {
    pub listeners: Vec<EmqxListener>,
    pub total_connections: u64,
    pub all_loopback_or_uds: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct EmqxExhookServer {
    pub name: String,
    pub url: String,
    pub enable: bool,
    pub status: String,
    pub hooks: Vec<String>,
    pub is_local: bool,
    pub transport_authenticated: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct ListHooksInput {}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ListHooksOutput {
    pub exhook_servers: Vec<EmqxExhookServer>,
    pub total_hooks: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GetHookStatusInput {
    pub server_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GetHookStatusOutput {
    pub server: Option<EmqxExhookServer>,
    pub found: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExhookTarget {
    /// The pinned EMQX release accepts only HTTP(S) ExHook URLs, so the local
    /// production transport is loopback TLS with a dedicated client identity.
    LoopbackMtls,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ConfigureHooksInput {
    pub server_name: String,
    pub target: ExhookTarget,
    pub enable: bool,
    pub hooks: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ConfigureHooksOutput {
    pub configured: bool,
    pub server_name: String,
    pub is_local: bool,
    pub message: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct StartInput {}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct StartOutput {
    pub started: bool,
    pub pid: Option<u32>,
    pub uptime_seconds: Option<u64>,
    pub node_name: String,
    pub message: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct StopInput {}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct StopOutput {
    pub stopped: bool,
    pub was_running: bool,
    pub message: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct RestartInput {}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RestartOutput {
    pub restarted: bool,
    pub pid: Option<u32>,
    pub uptime_seconds: Option<u64>,
    pub node_name: String,
    pub previous_uptime: Option<u64>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PublishWakeInput {
    /// Private-view owner key; the topic is derived from it, never chosen.
    pub owner_key: String,
    /// Scope revision that was committed to SHM before this wake.
    pub seq: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PublishWakeOutput {
    pub topic: String,
    pub published: bool,
    pub message_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AnnouncedToolset {
    /// Set id: `[A-Za-z0-9_.-]`, at most 128 bytes.
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    /// MCP tool names in this set.
    pub tools: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PublishToolsetsInput {
    /// Content hash of the captured view the sets were derived from.
    pub view_id: String,
    pub sets: Vec<AnnouncedToolset>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PublishToolsetsOutput {
    pub topic: String,
    pub published: bool,
    pub message_id: Option<String>,
    pub sets: usize,
    pub tools: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct KickClientInput {
    /// MQTT client id to disconnect (a revoked sled's doorbell connection).
    pub client_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct KickClientOutput {
    pub kicked: bool,
    pub was_connected: bool,
}

/// MQTT topic prefix for responder wakes (chatbot/agent turn notifications).
pub const RESPONDER_WAKE_TOPIC_PREFIX: &str = "opdbus/responder-wake/";

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PublishResponderWakeInput {
    /// Responder principal ID (the topic is derived from sha256 of this).
    pub responder_principal_id: String,
    /// The request/turn ID the responder should claim.
    pub request_id: String,
    /// Owner key of the requester (sha256 of their principal).
    pub owner_key: String,
    /// Principal ID of the requester (for response routing).
    pub owner_principal_id: String,
    /// Conversation ID (for chat turns).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<String>,
    /// Brief preview of the request content.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PublishResponderWakeOutput {
    pub topic: String,
    pub published: bool,
    pub message_id: Option<String>,
}

/// `opdbus/responder-wake/<sha256(responder_principal_id)>` for a responder.
pub fn responder_wake_topic(responder_principal_id: &str) -> String {
    use sha2::{Digest, Sha256};
    let key = format!("{:x}", Sha256::digest(responder_principal_id.as_bytes()));
    format!("{RESPONDER_WAKE_TOPIC_PREFIX}{key}")
}

#[derive(Debug, Clone, Default, Deserialize)]
struct RunitStatus {
    active_state: String,
    main_pid: Option<u32>,
    up_time: Option<String>,
}

impl RunitStatus {
    fn running(&self) -> bool {
        self.active_state == "active"
    }

    fn uptime_seconds(&self) -> Option<u64> {
        self.up_time.as_deref().and_then(parse_uptime_seconds)
    }
}

fn parse_uptime_seconds(value: &str) -> Option<u64> {
    let token = value.split_whitespace().next()?;
    token.trim_end_matches('s').parse().ok()
}

async fn runit_status(transport: &EmqxNativeTransport) -> Result<RunitStatus> {
    let connection = transport.runit_connection().await?;
    let proxy = Proxy::new(
        &connection,
        RUNIT_BUS_NAME,
        RUNIT_OBJECT_PATH,
        RUNIT_INTERFACE,
    )
    .await
    .context("runit D-Bus proxy creation failed")?;
    let encoded: String = proxy
        .call("Status", &(EMQX_SERVICE,))
        .await
        .context("runit D-Bus status call failed")?;
    serde_json::from_str(&encoded).context("runit D-Bus returned an invalid status document")
}

/// `action` is the D-Bus member name, which is CamelCase on the wire: the
/// runit service declares `async fn start/stop/restart`, and zbus's
/// `#[interface]` macro converts those to `Start`/`Stop`/`Restart`. `Proxy::call`
/// sends the literal member name it is given, so the caller must match the
/// converted spelling exactly or the call fails with UnknownMethod.
async fn runit_lifecycle(transport: &EmqxNativeTransport, action: &'static str) -> Result<String> {
    debug_assert!(matches!(action, "Start" | "Stop" | "Restart"));
    let connection = transport.runit_connection().await?;
    let proxy = Proxy::new(
        &connection,
        RUNIT_BUS_NAME,
        RUNIT_OBJECT_PATH,
        RUNIT_INTERFACE,
    )
    .await
    .context("runit D-Bus proxy creation failed")?;
    let (success, message): (bool, String) = proxy
        .call(action, &(EMQX_SERVICE,))
        .await
        .with_context(|| format!("runit D-Bus {action} call failed"))?;
    if !success {
        anyhow::bail!("runit D-Bus rejected EMQX {action}: {message}");
    }
    Ok(message)
}

fn api_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(2))
        .timeout(Duration::from_secs(5))
        .build()
        .context("failed to construct bounded EMQX API client")
}

fn api_credentials_from_files() -> Result<(String, String)> {
    let key =
        std::fs::read_to_string(API_KEY_FILE).context("EMQX management API key is unavailable")?;
    let secret = std::fs::read_to_string(API_SECRET_FILE)
        .context("EMQX management API secret is unavailable")?;
    let key = key.trim().to_string();
    let secret = secret.trim().to_string();
    if key.is_empty() || secret.is_empty() {
        anyhow::bail!("EMQX management API credential is empty");
    }
    Ok((key, secret))
}

async fn emqx_api(
    transport: &EmqxNativeTransport,
    method: Method,
    path: &str,
    body: Option<JsonValue>,
) -> Result<JsonValue> {
    let (api_key, api_secret) = transport.credentials()?;
    let url = format!("{}{path}", transport.api_base.trim_end_matches('/'));
    let mut request = api_client()?
        .request(method, &url)
        .basic_auth(api_key, Some(api_secret));
    if let Some(body) = body {
        request = request.json(&body);
    }
    let mut response = request
        .send()
        .await
        .with_context(|| format!("EMQX API request to {path} failed"))?;
    let status = response.status();
    if !status.is_success() {
        anyhow::bail!("EMQX API request to {path} returned HTTP {status}");
    }
    if response
        .content_length()
        .is_some_and(|length| length > API_RESPONSE_LIMIT as u64)
    {
        anyhow::bail!("EMQX API response from {path} exceeded the size limit");
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .with_context(|| format!("EMQX API response from {path} could not be read"))?
    {
        if chunk.len() > API_RESPONSE_LIMIT.saturating_sub(bytes.len()) {
            anyhow::bail!("EMQX API response from {path} exceeded the size limit");
        }
        bytes.extend_from_slice(&chunk);
    }
    if bytes.is_empty() {
        return Ok(serde_json::json!({}));
    }
    serde_json::from_slice(&bytes)
        .with_context(|| format!("EMQX API response from {path} was not valid JSON"))
}

fn response_array(value: &JsonValue) -> Result<&[JsonValue]> {
    value
        .as_array()
        .or_else(|| value.get("data").and_then(JsonValue::as_array))
        .map(Vec::as_slice)
        .ok_or_else(|| anyhow::anyhow!("EMQX API returned a non-array inventory"))
}

fn string_field(value: &JsonValue, names: &[&str]) -> Option<String> {
    names
        .iter()
        .find_map(|name| value.get(*name).and_then(JsonValue::as_str))
        .map(str::to_string)
}

fn u64_field(value: &JsonValue, names: &[&str]) -> u64 {
    names
        .iter()
        .find_map(|name| {
            value
                .get(*name)
                .and_then(|field| field.as_u64().or_else(|| field.as_str()?.parse().ok()))
        })
        .unwrap_or(0)
}

fn local_endpoint(value: &str) -> bool {
    if value.starts_with("unix:") || value.starts_with('/') {
        return true;
    }
    let address = value
        .strip_prefix("http://")
        .or_else(|| value.strip_prefix("https://"))
        .unwrap_or(value);
    address
        .parse::<SocketAddr>()
        .map(|address| address.ip().is_loopback())
        .unwrap_or(false)
}

fn listener_running(item: &JsonValue) -> bool {
    fn status_running(value: &JsonValue) -> Option<bool> {
        value
            .get("running")
            .and_then(JsonValue::as_bool)
            .or_else(|| {
                value.get("status").and_then(|status| {
                    status
                        .as_bool()
                        .or_else(|| status.as_str().map(|value| value == "running"))
                        .or_else(|| status.as_object().and_then(|_| status_running(status)))
                })
            })
    }
    let node_status_running = item
        .get("node_status")
        .and_then(JsonValue::as_array)
        .is_some_and(|nodes| nodes.iter().any(|node| status_running(node) == Some(true)));
    if node_status_running {
        return true;
    }
    item.get("running")
        .and_then(JsonValue::as_bool)
        .or_else(|| item.get("status").and_then(status_running))
        .or_else(|| item.get("enable").and_then(JsonValue::as_bool))
        .unwrap_or(false)
}

fn listener_status_field(item: &JsonValue, field_names: &[&str]) -> u64 {
    let direct = u64_field(item, field_names);
    if direct != 0 {
        return direct;
    }
    item.get("status")
        .map(|status| u64_field(status, field_names))
        .unwrap_or(0)
}

fn listener_current_connections(item: &JsonValue) -> u64 {
    let direct = listener_status_field(item, &["current_connections", "current_conns"]);
    if direct != 0 {
        return direct;
    }
    item.get("node_status")
        .and_then(JsonValue::as_array)
        .map(|nodes| {
            nodes
                .iter()
                .map(|node| listener_status_field(node, &["current_connections", "current_conns"]))
                .sum()
        })
        .unwrap_or(0)
}

fn listener_endpoint_allowed(item: &JsonValue, bind: &str, running: bool) -> bool {
    if bind == ":0" || bind.ends_with(":0") {
        return item.get("enable").and_then(JsonValue::as_bool) == Some(false) && !running;
    }
    local_endpoint(bind)
}

async fn publish_wake(
    transport: &EmqxNativeTransport,
    input: PublishWakeInput,
) -> Result<PublishWakeOutput> {
    let topic = wake_topic(&input.owner_key)?;
    let response = emqx_api(
        transport,
        Method::POST,
        "/publish",
        Some(serde_json::json!({
            "topic": topic,
            "payload": serde_json::json!({"seq": input.seq}).to_string(),
            "qos": 1,
            "retain": false,
        })),
    )
    .await
    .context("EMQX wake publish failed")?;
    Ok(PublishWakeOutput {
        topic,
        published: true,
        message_id: string_field(&response, &["id"]),
    })
}

async fn publish_responder_wake(
    transport: &EmqxNativeTransport,
    input: PublishResponderWakeInput,
) -> Result<PublishResponderWakeOutput> {
    let topic = responder_wake_topic(&input.responder_principal_id);
    let payload = serde_json::json!({
        "request_id": input.request_id,
        "owner_key": input.owner_key,
        "owner_principal_id": input.owner_principal_id,
        "conversation_id": input.conversation_id,
        "preview": input.preview,
    });
    let response = emqx_api(
        transport,
        Method::POST,
        "/publish",
        Some(serde_json::json!({
            "topic": topic,
            "payload": payload.to_string(),
            "qos": 1,
            "retain": false,
        })),
    )
    .await
    .context("EMQX responder wake publish failed")?;
    Ok(PublishResponderWakeOutput {
        topic,
        published: true,
        message_id: string_field(&response, &["id"]),
    })
}

/// The toolset manifest, bounded and name-checked before it reaches the
/// broker.
fn toolsets_payload(input: &PublishToolsetsInput) -> Result<(String, usize)> {
    anyhow::ensure!(
        input.view_id.len() == 64
            && input
                .view_id
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "view id must be 64 lowercase hex characters"
    );
    anyhow::ensure!(
        input.sets.len() <= MAX_TOOLSETS,
        "at most {MAX_TOOLSETS} toolsets"
    );
    let mut ids = std::collections::HashSet::new();
    let mut tools = 0usize;
    for set in &input.sets {
        anyhow::ensure!(valid_toolset_id(&set.id), "invalid toolset id {:?}", set.id);
        anyhow::ensure!(
            ids.insert(set.id.as_str()),
            "duplicate toolset id {}",
            set.id
        );
        anyhow::ensure!(
            set.title.len() <= 256 && set.description.len() <= 2048,
            "toolset {} title or description too long",
            set.id
        );
        for tool in &set.tools {
            anyhow::ensure!(valid_toolset_id(tool), "invalid tool name {tool:?}");
        }
        tools += set.tools.len();
    }
    anyhow::ensure!(
        tools <= MAX_ANNOUNCED_TOOLS,
        "at most {MAX_ANNOUNCED_TOOLS} announced tools"
    );
    let payload = serde_json::json!({
        "view_id": input.view_id,
        "sets": input.sets,
    })
    .to_string();
    anyhow::ensure!(
        payload.len() <= MAX_TOOLSETS_PAYLOAD,
        "toolset manifest exceeds {MAX_TOOLSETS_PAYLOAD} bytes"
    );
    Ok((payload, tools))
}

async fn publish_toolsets(
    transport: &EmqxNativeTransport,
    input: PublishToolsetsInput,
) -> Result<PublishToolsetsOutput> {
    let (payload, tools) = toolsets_payload(&input)?;
    let response = emqx_api(
        transport,
        Method::POST,
        "/publish",
        Some(serde_json::json!({
            "topic": TOOLSETS_TOPIC,
            "payload": payload,
            "qos": 1,
            "retain": true,
        })),
    )
    .await
    .context("EMQX toolset publish failed")?;
    Ok(PublishToolsetsOutput {
        topic: TOOLSETS_TOPIC.to_string(),
        published: true,
        message_id: string_field(&response, &["id"]),
        sets: input.sets.len(),
        tools,
    })
}

async fn kick_client(
    transport: &EmqxNativeTransport,
    input: KickClientInput,
) -> Result<KickClientOutput> {
    anyhow::ensure!(
        !input.client_id.is_empty()
            && input.client_id.len() <= 256
            && !input
                .client_id
                .chars()
                .any(|c| c.is_control() || matches!(c, '/' | '?' | '#' | '%')),
        "client id must be a bounded MQTT client id"
    );
    let path = format!("/clients/{}", input.client_id);
    match emqx_api(transport, Method::DELETE, &path, None).await {
        Ok(_) => Ok(KickClientOutput {
            kicked: true,
            was_connected: true,
        }),
        Err(error) if error.to_string().contains("HTTP 404") => Ok(KickClientOutput {
            kicked: false,
            was_connected: false,
        }),
        Err(error) => Err(error.context("EMQX client kick failed")),
    }
}

async fn query_listeners(transport: &EmqxNativeTransport) -> Result<ListListenersOutput> {
    let status = runit_status(transport).await?;
    if !status.running() {
        let listeners = EmqxState::declared()
            .listeners
            .into_iter()
            .map(|listener| EmqxListener {
                id: listener.id,
                r#type: listener.r#type,
                is_loopback_or_uds: local_endpoint(&listener.bind),
                bind: listener.bind,
                running: false,
                current_connections: 0,
                max_connections: 0,
            })
            .collect::<Vec<_>>();
        return Ok(ListListenersOutput {
            all_loopback_or_uds: listeners.iter().all(|listener| listener.is_loopback_or_uds),
            listeners,
            total_connections: 0,
        });
    }

    let payload = emqx_api(transport, Method::GET, "/listeners", None).await?;
    let listeners = response_array(&payload)?
        .iter()
        .map(|item| {
            let id = string_field(item, &["id", "name"]).unwrap_or_else(|| "unknown".into());
            let listener_type = string_field(item, &["type"])
                .or_else(|| id.split(':').next().map(str::to_string))
                .unwrap_or_else(|| "unknown".into());
            let bind = string_field(item, &["bind", "listen_on"]).unwrap_or_default();
            let running = listener_running(item);
            EmqxListener {
                id,
                r#type: listener_type,
                // `:0` is acceptable only when the API proves that it is not
                // running. An active port-zero listener is an ephemeral
                // socket and must fail the local-only policy.
                is_loopback_or_uds: listener_endpoint_allowed(item, &bind, running),
                bind,
                running,
                current_connections: listener_current_connections(item),
                max_connections: listener_status_field(item, &["max_connections", "max_conns"]),
            }
        })
        .collect::<Vec<_>>();
    let total_connections = listeners
        .iter()
        .map(|listener| listener.current_connections)
        .sum();
    let all_loopback_or_uds = listeners.iter().all(|listener| listener.is_loopback_or_uds);
    if !all_loopback_or_uds {
        anyhow::bail!("EMQX reported a listener outside loopback/UDS policy");
    }
    Ok(ListListenersOutput {
        listeners,
        total_connections,
        all_loopback_or_uds,
    })
}

fn exhook_from_json(item: &JsonValue) -> EmqxExhookServer {
    let url = string_field(item, &["url"]).unwrap_or_default();
    let ssl = &item["ssl"];
    let transport_authenticated = url == EXHOOK_TARGET
        && ssl["enable"].as_bool() == Some(true)
        && ssl["verify"].as_str() == Some("verify_peer")
        && ssl["cacertfile"].as_str() == Some("/etc/emqx/certs/opdbus-ca.crt")
        && ssl["certfile"].as_str() == Some("/etc/emqx/certs/emqx-exhook-client.crt")
        && ssl["keyfile"].as_str() == Some("/etc/emqx/certs/emqx-exhook-client.key");
    let status = item["node_status"]
        .as_array()
        .filter(|nodes| !nodes.is_empty())
        .map(|nodes| {
            if nodes
                .iter()
                .all(|node| node["status"].as_str() == Some("connected"))
            {
                "connected"
            } else {
                "disconnected"
            }
        })
        .unwrap_or("unknown");
    EmqxExhookServer {
        name: string_field(item, &["name"]).unwrap_or_default(),
        is_local: local_endpoint(&url),
        transport_authenticated,
        url,
        enable: item
            .get("enable")
            .and_then(JsonValue::as_bool)
            .unwrap_or(false),
        status: status.into(),
        hooks: item["hooks"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|hook| hook["name"].as_str().map(str::to_string))
            .collect(),
    }
}

async fn query_hooks(transport: &EmqxNativeTransport) -> Result<ListHooksOutput> {
    let status = runit_status(transport).await?;
    if !status.running() {
        return Ok(ListHooksOutput {
            exhook_servers: vec![],
            total_hooks: 0,
        });
    }
    let payload = emqx_api(transport, Method::GET, "/exhooks", None).await?;
    let servers = response_array(&payload)?
        .iter()
        .map(exhook_from_json)
        .collect::<Vec<_>>();
    if servers
        .iter()
        .any(|server| !server.is_local || !server.transport_authenticated)
    {
        anyhow::bail!("EMQX reported an unauthenticated or non-local ExHook target");
    }
    Ok(ListHooksOutput {
        total_hooks: servers.iter().map(|server| server.hooks.len()).sum(),
        exhook_servers: servers,
    })
}

fn validate_hook_selection(input: &ConfigureHooksInput) -> Result<()> {
    if input.server_name != EXHOOK_SERVER_NAME {
        anyhow::bail!("only the fixed 'opdbus' ExHook server may be configured");
    }
    if input.target != ExhookTarget::LoopbackMtls {
        anyhow::bail!("unsupported ExHook transport");
    }
    let requested = input.hooks.iter().cloned().collect::<BTreeSet<_>>();
    let allowed = REGISTERED_HOOKS
        .iter()
        .map(|hook| (*hook).to_string())
        .collect::<BTreeSet<_>>();
    if requested != allowed || requested.len() != input.hooks.len() {
        anyhow::bail!("hooks must be the exact supported HookProvider hook set");
    }
    Ok(())
}

async fn configure_hooks(
    transport: &EmqxNativeTransport,
    input: ConfigureHooksInput,
) -> Result<ConfigureHooksOutput> {
    validate_hook_selection(&input)?;
    let body = serde_json::json!({
        "name": EXHOOK_SERVER_NAME,
        "url": EXHOOK_TARGET,
        "enable": input.enable,
        "request_timeout": "5s",
        "failed_action": "ignore",
        "auto_reconnect": "5s",
        "pool_size": 4,
        "ssl": {
            "enable": true,
            "verify": "verify_peer",
            "cacertfile": "/etc/emqx/certs/opdbus-ca.crt",
            "certfile": "/etc/emqx/certs/emqx-exhook-client.crt",
            "keyfile": "/etc/emqx/certs/emqx-exhook-client.key"
        }
    });
    let _ = emqx_api(transport, Method::PUT, "/exhooks/opdbus", Some(body)).await?;
    let verified = query_hooks(transport)
        .await?
        .exhook_servers
        .into_iter()
        .find(|server| server.name == EXHOOK_SERVER_NAME)
        .ok_or_else(|| anyhow::anyhow!("EMQX did not return the configured ExHook server"))?;
    if verified.enable != input.enable || !verified.is_local || !verified.transport_authenticated {
        anyhow::bail!("EMQX ExHook verification did not match the requested local mTLS state");
    }
    Ok(ConfigureHooksOutput {
        configured: true,
        server_name: EXHOOK_SERVER_NAME.to_string(),
        is_local: true,
        message: "EMQX accepted and verified the fixed loopback-mTLS ExHook configuration"
            .to_string(),
    })
}

async fn query_status(transport: &EmqxNativeTransport) -> Result<GetStatusOutput> {
    let runit = runit_status(transport).await?;
    if !runit.running() {
        return Ok(GetStatusOutput {
            running: false,
            pid: None,
            uptime_seconds: runit.uptime_seconds(),
            version: EMQX_RELEASE_VERSION.to_string(),
            node_name: "emqx@127.0.0.1".to_string(),
            cluster_status: "stopped".to_string(),
            data_dir: EMQX_DATA_DIR.to_string(),
            config_dir: EMQX_CONFIG_DIR.to_string(),
            listeners_active: 0,
            connections_current: 0,
        });
    }
    // /status defaults to plain text in pinned EMQX 6.2.2. The health JSON
    // omits the version, which must come from the actual node inventory.
    let api = emqx_api(transport, Method::GET, "/status?format=json", None).await?;
    let nodes = emqx_api(transport, Method::GET, "/nodes", None).await?;
    let node_name = api["node_name"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("EMQX health response has no node_name"))?;
    let node = response_array(&nodes)?
        .iter()
        .find(|node| node["node"].as_str() == Some(node_name))
        .ok_or_else(|| anyhow::anyhow!("EMQX health node is absent from node inventory"))?;
    let version = node["version"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("EMQX node inventory has no version"))?;
    let app_status = api["app_status"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("EMQX health response has no app_status"))?;
    let listeners = query_listeners(transport).await?;
    Ok(GetStatusOutput {
        running: app_status == "running" && node["node_status"].as_str() == Some("running"),
        pid: runit.main_pid,
        uptime_seconds: runit.uptime_seconds(),
        version: version.into(),
        node_name: node_name.into(),
        cluster_status: app_status.into(),
        data_dir: EMQX_DATA_DIR.to_string(),
        config_dir: EMQX_CONFIG_DIR.to_string(),
        listeners_active: listeners
            .listeners
            .iter()
            .filter(|listener| listener.running)
            .count(),
        connections_current: listeners.total_connections,
    })
}

/// Typed standalone-EMQX dispatcher used by every projected transport.
pub async fn dispatch_emqx_method(method: &str, args: &JsonValue) -> Result<JsonValue> {
    dispatch_emqx_method_on(&EmqxNativeTransport::production(), method, args).await
}

async fn dispatch_emqx_method_on(
    transport: &EmqxNativeTransport,
    method: &str,
    args: &JsonValue,
) -> Result<JsonValue> {
    let output = match method {
        "get_status" => {
            let _: GetStatusInput = typed_input(args)?;
            serde_json::to_value(query_status(transport).await?)?
        }
        "get_mcp_status" => {
            let _: GetMcpStatusInput = typed_input(args)?;
            serde_json::to_value(GetMcpStatusOutput {
                gateway_installed: false,
                gateway_enabled: false,
                gateway_version: None,
                topic_prefix: None,
                internal_transport_only: true,
                external_mcp_endpoint: "https://10.0.0.2:8090/mcp".to_string(),
            })?
        }
        "list_listeners" => {
            let _: ListListenersInput = typed_input(args)?;
            serde_json::to_value(query_listeners(transport).await?)?
        }
        "list_hooks" => {
            let _: ListHooksInput = typed_input(args)?;
            serde_json::to_value(query_hooks(transport).await?)?
        }
        "get_hook_status" => {
            let input: GetHookStatusInput = typed_input(args)?;
            let server = query_hooks(transport)
                .await?
                .exhook_servers
                .into_iter()
                .find(|server| server.name == input.server_name);
            serde_json::to_value(GetHookStatusOutput {
                found: server.is_some(),
                server,
            })?
        }
        "configure_hooks" => {
            let input: ConfigureHooksInput = typed_input(args)?;
            serde_json::to_value(configure_hooks(transport, input).await?)?
        }
        "start" => {
            let _: StartInput = typed_input(args)?;
            let before = runit_status(transport).await?;
            let message = if before.running() {
                "EMQX was already running".to_string()
            } else {
                runit_lifecycle(transport, "Start").await?
            };
            let after = runit_status(transport).await?;
            let broker = query_status(transport)
                .await
                .context("EMQX Start was accepted but broker health readback failed")?;
            if !broker.running {
                anyhow::bail!("EMQX Start was accepted but broker did not report running");
            }
            serde_json::to_value(StartOutput {
                started: after.running(),
                pid: after.main_pid,
                uptime_seconds: after.uptime_seconds(),
                node_name: broker.node_name,
                message,
            })?
        }
        "stop" => {
            let _: StopInput = typed_input(args)?;
            let before = runit_status(transport).await?;
            let message = if before.running() {
                runit_lifecycle(transport, "Stop").await?
            } else {
                "EMQX was already stopped".to_string()
            };
            let after = runit_status(transport).await?;
            serde_json::to_value(StopOutput {
                stopped: !after.running(),
                was_running: before.running(),
                message,
            })?
        }
        "restart" => {
            let _: RestartInput = typed_input(args)?;
            let before = runit_status(transport).await?;
            let message = runit_lifecycle(transport, "Restart").await?;
            let after = runit_status(transport).await?;
            let broker = query_status(transport)
                .await
                .context("EMQX Restart was accepted but broker health readback failed")?;
            if !broker.running {
                anyhow::bail!("EMQX Restart was accepted but broker did not report running");
            }
            serde_json::to_value(RestartOutput {
                restarted: after.running(),
                pid: after.main_pid,
                uptime_seconds: after.uptime_seconds(),
                node_name: broker.node_name,
                previous_uptime: before.uptime_seconds(),
                message,
            })?
        }
        "publish_wake" => {
            let input: PublishWakeInput = typed_input(args)?;
            serde_json::to_value(publish_wake(transport, input).await?)?
        }
        "publish_responder_wake" => {
            let input: PublishResponderWakeInput = typed_input(args)?;
            serde_json::to_value(publish_responder_wake(transport, input).await?)?
        }
        "kick_client" => {
            let input: KickClientInput = typed_input(args)?;
            serde_json::to_value(kick_client(transport, input).await?)?
        }
        "publish_toolsets" => {
            let input: PublishToolsetsInput = typed_input(args)?;
            serde_json::to_value(publish_toolsets(transport, input).await?)?
        }
        other => anyhow::bail!("unknown emqx method: {other}"),
    };
    Ok(output)
}

#[async_trait]
impl StatePlugin for EmqxPlugin {
    fn name(&self) -> &str {
        PLUGIN_NAME
    }

    fn version(&self) -> &str {
        PLUGIN_VERSION
    }

    fn schema(&self) -> Option<PluginSchema> {
        Some(emqx_schema())
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

    async fn verify_state(&self, desired: &Value) -> Result<bool> {
        let declared = simd_json::serde::to_owned_value(EmqxState::declared())?;
        Ok(&declared == desired)
    }

    async fn create_checkpoint(&self) -> Result<Checkpoint> {
        Ok(Checkpoint {
            id: format!("{PLUGIN_NAME}-checkpoint"),
            plugin: PLUGIN_NAME.to_string(),
            timestamp: chrono::Utc::now().timestamp(),
            state_snapshot: simd_json::serde::to_owned_value(EmqxState::declared())?,
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
            atomic_operations: true,
        }
    }
}

fn add_method<I, O>(
    schema: &mut PluginSchema,
    name: &str,
    side_effect: SideEffect,
    idempotent: bool,
    capability: &str,
    subid: &str,
) where
    I: JsonSchema,
    O: JsonSchema,
{
    schema.methods.insert(
        name.to_string(),
        method_decl_from_schemars_with_output::<I, O>(
            name,
            side_effect,
            idempotent,
            capability,
            subid,
        ),
    );
    schema.capabilities.insert(
        capability.to_string(),
        CapabilityDecl {
            id: capability.to_string(),
            description: format!("Grants: {name}."),
        },
    );
}

pub(crate) fn emqx_schema() -> PluginSchema {
    let root = serde_json::to_value(schemars::schema_for!(EmqxState))
        .expect("EMQX state schema serializes");
    let mut schema = super::schemars_adapter::plugin_schema_from_json(
        PLUGIN_NAME,
        PLUGIN_VERSION,
        PLUGIN_DESCRIPTION,
        &root,
    );
    schema.category = PLUGIN_CATEGORY.to_string();
    schema.display_name = Some(PLUGIN_DISPLAY_NAME.to_string());
    let declared = simd_json::serde::to_owned_value(EmqxState::declared())
        .expect("declared EMQX state serializes");
    super::schemars_adapter::apply_state_defaults(&mut schema, &declared);
    schema.example = Some(declared);

    schema.signals.push(SignalDecl {
        name: BROKER_EVENT_SIGNAL.to_string(),
        payload: Some(
            simd_json::serde::to_owned_value(schemars::schema_for!(BrokerEvent))
                .expect("broker event schema serializes"),
        ),
        subid: BROKER_EVENT_SUBID.to_string(),
    });

    add_method::<GetStatusInput, GetStatusOutput>(
        &mut schema,
        "get_status",
        SideEffect::Read,
        true,
        "cap.network.emqx.status.get@v1",
        "obs.network.emqx.status.get@v2",
    );
    add_method::<GetMcpStatusInput, GetMcpStatusOutput>(
        &mut schema,
        "get_mcp_status",
        SideEffect::Read,
        true,
        "cap.network.emqx.mcp.status.get@v1",
        "obs.network.emqx.mcp.status.get@v1",
    );
    add_method::<ListListenersInput, ListListenersOutput>(
        &mut schema,
        "list_listeners",
        SideEffect::Read,
        true,
        "cap.network.emqx.listeners.list@v1",
        "obs.network.emqx.listeners.list@v1",
    );
    add_method::<ListHooksInput, ListHooksOutput>(
        &mut schema,
        "list_hooks",
        SideEffect::Read,
        true,
        "cap.network.emqx.hooks.list@v1",
        "obs.network.emqx.hooks.list@v1",
    );
    add_method::<GetHookStatusInput, GetHookStatusOutput>(
        &mut schema,
        "get_hook_status",
        SideEffect::Read,
        true,
        "cap.network.emqx.hooks.status.get@v1",
        "obs.network.emqx.hooks.status.get@v1",
    );
    add_method::<ConfigureHooksInput, ConfigureHooksOutput>(
        &mut schema,
        "configure_hooks",
        SideEffect::Mutation,
        true,
        "cap.network.emqx.hooks.configure@v1",
        "mut.network.emqx.hooks.configure@v1",
    );
    add_method::<StartInput, StartOutput>(
        &mut schema,
        "start",
        SideEffect::Mutation,
        true,
        "cap.network.emqx.lifecycle.start@v1",
        "mut.network.emqx.lifecycle.start@v1",
    );
    add_method::<StopInput, StopOutput>(
        &mut schema,
        "stop",
        SideEffect::Mutation,
        true,
        "cap.network.emqx.lifecycle.stop@v1",
        "mut.network.emqx.lifecycle.stop@v1",
    );
    add_method::<RestartInput, RestartOutput>(
        &mut schema,
        "restart",
        SideEffect::Mutation,
        false,
        "cap.network.emqx.lifecycle.restart@v1",
        "mut.network.emqx.lifecycle.restart@v1",
    );
    add_method::<PublishWakeInput, PublishWakeOutput>(
        &mut schema,
        "publish_wake",
        SideEffect::Mutation,
        true,
        "cap.network.emqx.wake.publish@v1",
        "mut.network.emqx.wake.publish@v1",
    );
    add_method::<KickClientInput, KickClientOutput>(
        &mut schema,
        "kick_client",
        SideEffect::Mutation,
        true,
        "cap.network.emqx.clients.kick@v1",
        "mut.network.emqx.clients.kick@v1",
    );
    add_method::<PublishToolsetsInput, PublishToolsetsOutput>(
        &mut schema,
        "publish_toolsets",
        SideEffect::Mutation,
        true,
        "cap.network.emqx.toolsets.publish@v1",
        "mut.network.emqx.toolsets.publish@v1",
    );

    schema
}

inventory::submit! {
    crate::default_registry::PluginReg::new(PLUGIN_NAME, |_ctx| std::sync::Arc::new(EmqxPlugin::new()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn schema_is_standalone_deterministic_and_complete() {
        let first = emqx_schema();
        let second = emqx_schema();
        assert_eq!(
            serde_json::to_value(&first).unwrap(),
            serde_json::to_value(&second).unwrap()
        );
        let expected = [
            "get_status",
            "get_mcp_status",
            "list_listeners",
            "list_hooks",
            "get_hook_status",
            "configure_hooks",
            "start",
            "stop",
            "restart",
            "publish_wake",
            "kick_client",
            "publish_toolsets",
        ];
        assert_eq!(first.methods.len(), expected.len());
        for method in expected {
            assert!(first.methods.contains_key(method), "missing {method}");
        }
        let encoded = serde_json::to_string(&first).unwrap().to_ascii_lowercase();
        assert!(!encoded.contains("netmaker"));
        assert!(!encoded.contains("broker.sock"));
        assert!(!encoded.contains("api.sock"));
        assert!(!encoded.contains("100.69.0.1"));
    }

    #[test]
    fn declared_endpoints_are_loopback_or_uds_and_mcp_gateway_is_absent() {
        let state = EmqxState::declared();
        assert!(state
            .listeners
            .iter()
            .all(|listener| local_endpoint(&listener.bind)));
        assert!(local_endpoint(&state.exhook.target));
        assert!(
            !local_endpoint("0.0.0.0:8083"),
            "wildcard binds are never declared"
        );
        assert!(
            !local_endpoint("129.80.64.127:8084"),
            "the decoy is not a listener host"
        );
        let wake = state
            .listeners
            .iter()
            .find(|l| l.id == "wss:wake")
            .expect("doorbell listener is declared");
        assert_eq!(
            (wake.r#type.as_str(), wake.bind.as_str()),
            ("wss", EMQX_MQTT_WSS_BIND)
        );
        assert!(!state.mcp_gateway_installed);
    }

    #[test]
    fn toolset_manifests_are_bounded_and_name_checked() {
        let set = |id: &str, tools: &[&str]| AnnouncedToolset {
            id: id.into(),
            title: id.into(),
            description: String::new(),
            tools: tools.iter().map(|tool| tool.to_string()).collect(),
        };
        let input = |sets| PublishToolsetsInput {
            view_id: "ab".repeat(32),
            sets,
        };
        let (payload, tools) =
            toolsets_payload(&input(vec![set("net", &["net__get_status", "net__set"])])).unwrap();
        assert_eq!(tools, 2);
        assert!(payload.contains("net__get_status"));
        for bad in [
            vec![set("net/#", &[])],
            vec![set("net", &["a b"])],
            vec![set("net", &[]), set("net", &[])],
        ] {
            assert!(toolsets_payload(&input(bad)).is_err());
        }
        let mut short = input(vec![]);
        short.view_id = "abc".into();
        assert!(toolsets_payload(&short).is_err());
    }

    #[test]
    fn wake_topics_are_derived_from_owner_keys_only() {
        let key = "ab".repeat(32);
        assert_eq!(wake_topic(&key).unwrap(), format!("opdbus/wake/{key}"));
        for bad in [
            "",
            "AB",
            &"ab".repeat(31),
            "opdbus/wake/#",
            &"zz".repeat(32),
        ] {
            assert!(wake_topic(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn hook_selection_rejects_partial_or_unknown_sets() {
        let partial = ConfigureHooksInput {
            server_name: EXHOOK_SERVER_NAME.to_string(),
            target: ExhookTarget::LoopbackMtls,
            enable: true,
            hooks: vec!["message.publish".to_string()],
        };
        assert!(validate_hook_selection(&partial).is_err());

        let exact = ConfigureHooksInput {
            server_name: EXHOOK_SERVER_NAME.to_string(),
            target: ExhookTarget::LoopbackMtls,
            enable: true,
            hooks: REGISTERED_HOOKS
                .iter()
                .map(|hook| (*hook).to_string())
                .collect(),
        };
        validate_hook_selection(&exact).expect("exact hook set");
    }

    #[tokio::test]
    async fn unknown_method_fails_without_fallback_echo() {
        let error = dispatch_emqx_method("not_a_method", &serde_json::json!({}))
            .await
            .expect_err("unknown method must fail");
        assert!(error.to_string().contains("unknown emqx method"));
    }

    #[test]
    fn lifecycle_inputs_cannot_select_a_service() {
        let schema = serde_json::to_value(schemars::schema_for!(StartInput)).unwrap();
        let encoded = schema.to_string();
        assert!(!encoded.contains("service_name"));
        assert!(!encoded.contains("service"));
    }

    #[test]
    fn secret_files_never_enter_declared_state() {
        let encoded = serde_json::to_string(&EmqxState::declared()).unwrap();
        assert!(!encoded.contains("password"));
        assert!(!encoded.contains("credential"));
        assert!(!encoded.contains(API_KEY_FILE));
        assert!(!encoded.contains(API_SECRET_FILE));
    }

    #[test]
    fn broker_event_signal_has_typed_metadata_payload_and_event_subid() {
        let schema = emqx_schema();
        let signal = schema
            .signals
            .iter()
            .find(|signal| signal.name == BROKER_EVENT_SIGNAL)
            .expect("broker event signal");
        assert_eq!(signal.subid, BROKER_EVENT_SUBID);
        let payload = signal.payload.as_ref().expect("typed payload");
        let payload_json = serde_json::to_value(payload).expect("payload JSON");
        let properties = payload_json.get("properties").expect("payload properties");
        for field in ["event_type", "client_ref", "topic_ref", "detail_ref"] {
            assert!(properties.get(field).is_some(), "missing {field}");
        }
        assert!(payload_json.get("required").is_some());
        assert!(!serde_json::to_string(&payload_json)
            .unwrap()
            .contains("payload"));
    }

    #[test]
    fn method_contracts_preserve_reads_writes_subids_and_idempotence() {
        let schema = emqx_schema();
        let expected = [
            (
                "get_status",
                SideEffect::Read,
                true,
                "obs.network.emqx.status.get@v2",
            ),
            (
                "get_mcp_status",
                SideEffect::Read,
                true,
                "obs.network.emqx.mcp.status.get@v1",
            ),
            (
                "list_listeners",
                SideEffect::Read,
                true,
                "obs.network.emqx.listeners.list@v1",
            ),
            (
                "list_hooks",
                SideEffect::Read,
                true,
                "obs.network.emqx.hooks.list@v1",
            ),
            (
                "get_hook_status",
                SideEffect::Read,
                true,
                "obs.network.emqx.hooks.status.get@v1",
            ),
            (
                "configure_hooks",
                SideEffect::Mutation,
                true,
                "mut.network.emqx.hooks.configure@v1",
            ),
            (
                "start",
                SideEffect::Mutation,
                true,
                "mut.network.emqx.lifecycle.start@v1",
            ),
            (
                "stop",
                SideEffect::Mutation,
                true,
                "mut.network.emqx.lifecycle.stop@v1",
            ),
            (
                "restart",
                SideEffect::Mutation,
                false,
                "mut.network.emqx.lifecycle.restart@v1",
            ),
            (
                "publish_wake",
                SideEffect::Mutation,
                true,
                "mut.network.emqx.wake.publish@v1",
            ),
            (
                "kick_client",
                SideEffect::Mutation,
                true,
                "mut.network.emqx.clients.kick@v1",
            ),
            (
                "publish_toolsets",
                SideEffect::Mutation,
                true,
                "mut.network.emqx.toolsets.publish@v1",
            ),
        ];
        for (name, effect, idempotent, subid) in expected {
            let method = schema.methods.get(name).expect(name);
            assert_eq!(method.side_effect, effect, "{name} side effect");
            assert_eq!(method.idempotent, idempotent, "{name} idempotence");
            assert_eq!(method.subid, subid, "{name} subid");
        }
    }

    #[test]
    fn exhook_parser_accepts_only_the_pinned_authenticated_shape() {
        let value = json!({
            "name": "opdbus",
            "url": EXHOOK_TARGET,
            "enable": true,
            "ssl": {
                "enable": true,
                "verify": "verify_peer",
                "cacertfile": "/etc/emqx/certs/opdbus-ca.crt",
                "certfile": "/etc/emqx/certs/emqx-exhook-client.crt",
                "keyfile": "/etc/emqx/certs/emqx-exhook-client.key"
            },
            "node_status": [{"node": "emqx@127.0.0.1", "status": "connected"}],
            "hooks": [{"name": "message.publish", "params": {"topics": ["#"]}}]
        });
        let parsed = exhook_from_json(&value);
        assert!(parsed.is_local);
        assert!(parsed.transport_authenticated);
        assert_eq!(parsed.status, "connected");
        assert_eq!(parsed.hooks, vec!["message.publish"]);
    }

    #[test]
    fn exhook_parser_rejects_https_without_exact_mtls_material() {
        let value = json!({
            "name": "opdbus",
            "url": "https://127.0.0.1:9000",
            "enable": true,
            "ssl": {"enable": true},
            "node_status": [{"status": "connected"}],
            "hooks": []
        });
        let parsed = exhook_from_json(&value);
        assert!(parsed.is_local);
        assert!(!parsed.transport_authenticated);
    }

    #[test]
    fn malformed_inventory_fails_closed_instead_of_becoming_empty() {
        assert!(response_array(&json!({"message": "not an inventory"})).is_err());
        assert!(response_array(&json!(null)).is_err());
        let parsed = exhook_from_json(&json!({"name": "opdbus", "hooks": []}));
        assert_eq!(parsed.status, "unknown");
        assert!(!parsed.transport_authenticated);
    }
}

#[cfg(test)]
#[path = "emqx_native_tests.rs"]
mod native_tests;
