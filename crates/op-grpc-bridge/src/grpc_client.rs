//! gRPC Client - For D-Bus → remote gRPC calls
//!
//! Allows local D-Bus services to call remote gRPC endpoints,
//! enabling distributed operation-dbus deployments.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use base64::Engine as _;
use prost_types::{Struct as ProstStruct, Value as ProstValue, value::Kind as ProstKind};
use tokio::sync::RwLock;
use tonic::Request;
use tonic::metadata::MetadataValue;
use tonic::transport::{Certificate, Channel, ClientTlsConfig, Endpoint};
use tracing::info;

use crate::oracle_assertion_client::DecoyAssertionClient;
use crate::proto::{
    CallMethodRequest, GetStateRequest, GetStateResponse, MutateRequest,
    OperationType as ProtoOperationType, SubscribeEventsRequest, SubscribeRequest,
    event_chain_service_client::EventChainServiceClient,
    plugin_service_client::PluginServiceClient, state_sync_client::StateSyncClient,
};

/// Bounded response envelope for complete plugin documents and SHM state.
/// Native chat can exceed tonic's 4 MiB default; identity admission is unchanged.
pub const MAX_PLUGIN_STATE_RESPONSE_BYTES: usize = 64 * 1024 * 1024;

#[cfg(test)]
#[path = "grpc_client_large_response_tests.rs"]
mod large_response_tests;

/// Configuration for a remote gRPC endpoint
#[derive(Debug, Clone)]
pub struct RemoteEndpoint {
    pub address: String,
    pub tls_enabled: bool,
    pub connect_timeout: Duration,
    pub request_timeout: Duration,
}

/// Caller-supplied Ghostbridge identity forwarded by an HTTP compatibility
/// adapter into the canonical gRPC method call.
#[derive(Clone)]
pub struct GhostbridgeCallMetadata {
    /// Exact caller-supplied SID1 wire bytes, if this is a local identity call.
    pub sealed_id: Option<Vec<u8>>,
    /// Exact caller-supplied OIA1 wire bytes, if this is a decoy assertion call.
    pub oracle_assertion: Option<Vec<u8>>,
    /// Legacy fields retained for non-plugin Ghostbridge services.
    pub session_genesis: Option<String>,
    pub trace_id: Option<String>,
    pub wireguard_pubkey: Option<String>,
}

impl Default for RemoteEndpoint {
    fn default() -> Self {
        Self {
            address: "http://127.0.0.1:8090".to_string(),
            tls_enabled: false,
            connect_timeout: Duration::from_secs(5),
            request_timeout: Duration::from_secs(30),
        }
    }
}

/// gRPC client pool for connecting to remote Operation services
pub struct GrpcClientPool {
    /// Map of endpoint address to channel
    channels: RwLock<HashMap<String, Channel>>,
    /// Default endpoint configuration
    default_config: RemoteEndpoint,
}

impl Default for GrpcClientPool {
    fn default() -> Self {
        Self::new()
    }
}

impl GrpcClientPool {
    pub fn new() -> Self {
        Self {
            channels: RwLock::new(HashMap::new()),
            default_config: RemoteEndpoint::default(),
        }
    }

    pub fn with_default_config(config: RemoteEndpoint) -> Self {
        Self {
            channels: RwLock::new(HashMap::new()),
            default_config: config,
        }
    }

    /// Apply transport settings (timeouts + TLS) to an endpoint.
    ///
    /// TLS is driven by the URI scheme: `https://` gets a tonic
    /// `ClientTlsConfig`. This mirrors `examples/repro_subscribe.rs`, which is
    /// the reference client path. Trust anchors, in priority order:
    ///   1. `OP_DBUS_GRPC_CA_FILE` — PEM file to pin (what op-web's run script
    ///      exports; may point at the leaf cert itself since it is its own CA).
    ///   2. Native system roots (`with_native_roots`).
    ///
    /// `OP_DBUS_GRPC_TLS_DOMAIN` overrides the SNI/verification name when the
    /// dial address is an IP or differs from the certificate SAN.
    fn configure_endpoint(
        &self,
        endpoint: Endpoint,
        addr: &str,
    ) -> Result<Endpoint, GrpcClientError> {
        let endpoint = endpoint
            .connect_timeout(self.default_config.connect_timeout)
            .timeout(self.default_config.request_timeout);

        if !addr.starts_with("https://") {
            return Ok(endpoint);
        }

        let mut tls = ClientTlsConfig::new().with_native_roots();
        if let Ok(path) = std::env::var("OP_DBUS_GRPC_CA_FILE")
            && !path.trim().is_empty()
        {
            let pem = std::fs::read(path.trim()).map_err(|e| {
                GrpcClientError::ConnectionFailed(format!(
                    "failed to read OP_DBUS_GRPC_CA_FILE={path}: {e}"
                ))
            })?;
            tls = tls.ca_certificate(Certificate::from_pem(pem));
        }
        if let Ok(domain) = std::env::var("OP_DBUS_GRPC_TLS_DOMAIN")
            && !domain.trim().is_empty()
        {
            tls = tls.domain_name(domain.trim().to_string());
        }
        endpoint
            .tls_config(tls)
            .map_err(|e| GrpcClientError::ConnectionFailed(format!("TLS config for {addr}: {e}")))
    }

    /// Get or create a channel to the specified address (supports comma-separated endpoints for load balancing)
    async fn get_channel(&self, address: &str) -> Result<Channel, GrpcClientError> {
        {
            let channels = self.channels.read().await;
            if let Some(channel) = channels.get(address) {
                return Ok(channel.clone());
            }
        }

        let addrs: Vec<&str> = address
            .split(',')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect();

        let channel = if addrs.len() > 1 {
            // Native Tonic Load Balancing
            let endpoints = addrs
                .into_iter()
                .map(|addr| {
                    Endpoint::from_shared(addr.to_string())
                        .map_err(|e| {
                            GrpcClientError::ConnectionFailed(format!(
                                "Invalid endpoint {addr}: {e}"
                            ))
                        })
                        .and_then(|e| self.configure_endpoint(e, addr))
                })
                .collect::<Result<Vec<_>, _>>()?;

            Channel::balance_list(endpoints.into_iter())
        } else {
            // Single endpoint
            let endpoint = Endpoint::from_shared(address.to_string())
                .map_err(|e| GrpcClientError::ConnectionFailed(e.to_string()))?;
            let endpoint = self.configure_endpoint(endpoint, address)?;

            endpoint
                .connect()
                .await
                .map_err(|e| GrpcClientError::ConnectionFailed(e.to_string()))?
        };

        {
            let mut channels = self.channels.write().await;
            channels.insert(address.to_string(), channel.clone());
        }

        info!("Connected to remote gRPC endpoint(s): {}", address);
        Ok(channel)
    }

    /// Get a Plugin service client
    pub async fn plugin_service_client(
        &self,
        address: &str,
    ) -> Result<PluginServiceClient<Channel>, GrpcClientError> {
        let channel = self.get_channel(address).await?;
        Ok(PluginServiceClient::new(channel)
            .max_decoding_message_size(MAX_PLUGIN_STATE_RESPONSE_BYTES))
    }

    /// Get a StateSync service client
    pub async fn state_sync_client(
        &self,
        address: &str,
    ) -> Result<StateSyncClient<Channel>, GrpcClientError> {
        let channel = self.get_channel(address).await?;
        Ok(
            StateSyncClient::new(channel)
                .max_decoding_message_size(MAX_PLUGIN_STATE_RESPONSE_BYTES),
        )
    }

    /// Get an EventChain service client
    pub async fn event_chain_client(
        &self,
        address: &str,
    ) -> Result<EventChainServiceClient<Channel>, GrpcClientError> {
        let channel = self.get_channel(address).await?;
        Ok(EventChainServiceClient::new(channel))
    }

    /// Close all connections
    pub async fn close_all(&self) {
        let mut channels = self.channels.write().await;
        channels.clear();
        info!("Closed all gRPC client connections");
    }
}

/// High-level client for remote Operation services
#[allow(dead_code)]
pub struct RemoteOperationClient {
    pool: Arc<GrpcClientPool>,
    default_address: String,
    client_id: String,
}

impl RemoteOperationClient {
    pub fn new(pool: Arc<GrpcClientPool>, address: &str, client_id: &str) -> Self {
        Self {
            pool,
            default_address: address.to_string(),
            client_id: client_id.to_string(),
        }
    }

    /// Get state from a remote endpoint
    pub async fn get_state(
        &self,
        plugin_id: &str,
        object_path: &str,
    ) -> Result<simd_json::OwnedValue, GrpcClientError> {
        self.get_state_with_identity(plugin_id, object_path, &private_plugin_identity()?, None)
            .await
    }

    /// Get public state, or a private workspace descendant when the caller
    /// supplies its authored credential and the exact declared capability.
    /// This path never substitutes a process/service identity.
    pub async fn get_state_with_identity(
        &self,
        plugin_id: &str,
        object_path: &str,
        identity: &GhostbridgeCallMetadata,
        capability_id: Option<&str>,
    ) -> Result<simd_json::OwnedValue, GrpcClientError> {
        let mut client = self.pool.state_sync_client(&self.default_address).await?;

        let mut request = Request::new(GetStateRequest {
            plugin_id: plugin_id.to_string(),
            object_path: object_path.to_string(),
        });
        attach_identity_metadata(&mut request, identity, capability_id)?;

        let response = client
            .get_state(request)
            .await
            .map_err(|e| GrpcClientError::RequestFailed(e.to_string()))?;

        decode_get_state_response(response.into_inner())
    }

    /// Set state on a remote endpoint (apply patch)
    pub async fn set_state(
        &self,
        plugin_id: &str,
        object_path: &str,
        state: simd_json::OwnedValue,
        actor_id: &str,
        capability_id: &str,
    ) -> Result<SetStateResult, GrpcClientError> {
        self.set_state_with_identity(
            plugin_id,
            object_path,
            state,
            actor_id,
            capability_id,
            &private_plugin_identity()?,
        )
        .await
    }

    /// Apply state with the exact caller-supplied SID1/OIA1 credential and
    /// capability. The request actor remains the caller's asserted actor; the
    /// server validates that it matches the credential.
    #[allow(clippy::too_many_arguments)]
    pub async fn set_state_with_identity(
        &self,
        plugin_id: &str,
        object_path: &str,
        state: simd_json::OwnedValue,
        actor_id: &str,
        capability_id: &str,
        identity: &GhostbridgeCallMetadata,
    ) -> Result<SetStateResult, GrpcClientError> {
        let mut client = self.pool.state_sync_client(&self.default_address).await?;

        let mut request = Request::new(MutateRequest {
            plugin_id: plugin_id.to_string(),
            object_path: object_path.to_string(),
            operation: ProtoOperationType::ApplyPatch as i32,
            member_name: String::new(),
            value: Some(simd_to_prost_value(&state)),
            actor_id: actor_id.to_string(),
            capability_id: capability_id.to_string(),
            idempotency_key: uuid::Uuid::new_v4().to_string(),
        });
        attach_identity_metadata(&mut request, identity, Some(capability_id))?;

        let response = client
            .mutate(request)
            .await
            .map_err(|e| GrpcClientError::RequestFailed(e.to_string()))?;

        let resp = response.into_inner();
        if !resp.success {
            if let Some(err) = resp.error {
                return Err(GrpcClientError::RemoteError {
                    code: format!("{}", err.code),
                    message: err.message,
                });
            }
            return Err(GrpcClientError::RemoteError {
                code: "UNKNOWN".to_string(),
                message: "mutation failed".to_string(),
            });
        }

        Ok(SetStateResult {
            event_id: resp.event_id,
            effective_hash: resp.effective_hash,
        })
    }

    /// Call a method on a remote endpoint
    #[allow(clippy::too_many_arguments)]
    pub async fn call_method(
        &self,
        plugin_id: &str,
        object_path: &str,
        interface_name: &str,
        method_name: &str,
        arguments: Vec<simd_json::OwnedValue>,
        actor_id: &str,
        capability_id: &str,
    ) -> Result<simd_json::OwnedValue, GrpcClientError> {
        let mut client = self
            .pool
            .plugin_service_client(&self.default_address)
            .await?;

        let arguments = arguments
            .iter()
            .map(simd_to_prost_value)
            .collect::<Vec<_>>();

        let mut request = Request::new(CallMethodRequest {
            plugin_id: plugin_id.to_string(),
            object_path: object_path.to_string(),
            interface_name: interface_name.to_string(),
            method_name: method_name.to_string(),
            arguments,
            actor_id: actor_id.to_string(),
            capability_id: capability_id.to_string(),
        });
        attach_plugin_identity_metadata(&mut request, &private_plugin_identity()?, capability_id)?;

        let response = client
            .call_method(request)
            .await
            .map_err(|e| GrpcClientError::RequestFailed(e.to_string()))?;

        let resp = response.into_inner();
        if !resp.success {
            if let Some(err) = resp.error {
                return Err(GrpcClientError::RemoteError {
                    code: format!("{}", err.code),
                    message: err.message,
                });
            }
            return Err(GrpcClientError::RemoteError {
                code: "UNKNOWN".to_string(),
                message: "call failed".to_string(),
            });
        }

        if let Some(result) = resp.result {
            Ok(prost_value_to_simd(&result))
        } else {
            Ok(simd_json::json!(null))
        }
    }

    /// Call a method while preserving the Ghostbridge identity supplied by an
    /// outer transport adapter. This keeps HTTP on op-web while capability
    /// enforcement remains in the bridge's canonical gRPC method pipeline.
    #[allow(clippy::too_many_arguments)]
    pub async fn call_method_with_metadata(
        &self,
        plugin_id: &str,
        object_path: &str,
        interface_name: &str,
        method_name: &str,
        arguments: Vec<simd_json::OwnedValue>,
        actor_id: &str,
        capability_id: &str,
        identity: &GhostbridgeCallMetadata,
    ) -> Result<simd_json::OwnedValue, GrpcClientError> {
        let mut client = self
            .pool
            .plugin_service_client(&self.default_address)
            .await?;

        let arguments = arguments
            .iter()
            .map(simd_to_prost_value)
            .collect::<Vec<_>>();

        let mut request = Request::new(CallMethodRequest {
            plugin_id: plugin_id.to_string(),
            object_path: object_path.to_string(),
            interface_name: interface_name.to_string(),
            method_name: method_name.to_string(),
            arguments,
            actor_id: actor_id.to_string(),
            capability_id: capability_id.to_string(),
        });
        attach_plugin_identity_metadata(&mut request, identity, capability_id)?;

        let response = client
            .call_method(request)
            .await
            .map_err(|e| GrpcClientError::RequestFailed(e.to_string()))?;

        let resp = response.into_inner();
        if !resp.success {
            if let Some(err) = resp.error {
                return Err(GrpcClientError::RemoteError {
                    code: format!("{}", err.code),
                    message: err.message,
                });
            }
            return Err(GrpcClientError::RemoteError {
                code: "UNKNOWN".to_string(),
                message: "call failed".to_string(),
            });
        }

        if let Some(result) = resp.result {
            Ok(prost_value_to_simd(&result))
        } else {
            Ok(simd_json::json!(null))
        }
    }

    /// Acquire one fresh OIA1 assertion from the caller's explicit decoy
    /// issuer and forward it through the canonical metadata path.
    #[allow(clippy::too_many_arguments)]
    pub async fn call_method_via_decoy(
        &self,
        plugin_id: &str,
        object_path: &str,
        interface_name: &str,
        method_name: &str,
        arguments: Vec<simd_json::OwnedValue>,
        actor_id: &str,
        capability_id: &str,
        issuer: &DecoyAssertionClient,
    ) -> Result<simd_json::OwnedValue, GrpcClientError> {
        let identity = issuer.acquire().await.map_err(|error| {
            GrpcClientError::RequestFailed(format!("OIA1 assertion acquisition failed: {error:#}"))
        })?;
        self.call_method_with_metadata(
            plugin_id,
            object_path,
            interface_name,
            method_name,
            arguments,
            actor_id,
            capability_id,
            &identity,
        )
        .await
    }

    /// Subscribe to state updates from a remote endpoint
    pub async fn subscribe(
        &self,
        plugin_filters: Vec<String>,
        path_filters: Vec<String>,
        tag_filters: Vec<String>,
    ) -> Result<
        impl tokio_stream::Stream<Item = Result<StateUpdateMessage, GrpcClientError>> + use<>,
        GrpcClientError,
    > {
        self.subscribe_with_identity(
            plugin_filters,
            path_filters,
            tag_filters,
            &private_plugin_identity()?,
            None,
        )
        .await
    }

    /// Subscribe using one explicit authored caller credential. Private
    /// workspace subscriptions pass their exact capability; public projection
    /// subscriptions omit it. No service identity is substituted.
    pub async fn subscribe_with_identity(
        &self,
        plugin_filters: Vec<String>,
        path_filters: Vec<String>,
        tag_filters: Vec<String>,
        identity: &GhostbridgeCallMetadata,
        capability_id: Option<&str>,
    ) -> Result<
        impl tokio_stream::Stream<Item = Result<StateUpdateMessage, GrpcClientError>> + use<>,
        GrpcClientError,
    > {
        let mut client = self.pool.state_sync_client(&self.default_address).await?;

        let mut request = Request::new(SubscribeRequest {
            plugin_ids: plugin_filters,
            path_patterns: path_filters,
            tags: tag_filters,
            include_initial_state: true,
            include_schema: true,
        });
        attach_identity_metadata(&mut request, identity, capability_id)?;

        let response = client
            .subscribe(request)
            .await
            .map_err(|e| GrpcClientError::RequestFailed(e.to_string()))?;

        let stream = response.into_inner();

        Ok(tokio_stream::StreamExt::map(stream, |result| {
            result
                .map(state_update_message)
                .map_err(|e| GrpcClientError::StreamError(e.to_string()))
        }))
    }

    /// Subscribe to privileged audit metadata using this explicitly configured
    /// session's authored credential and existing snowball.read grant.
    pub async fn stream_events(
        &self,
        from_event_id: Option<u64>,
        plugin_filters: Vec<String>,
        tag_filters: Vec<String>,
    ) -> Result<
        impl tokio_stream::Stream<Item = Result<ChainEventMessage, GrpcClientError>> + use<>,
        GrpcClientError,
    > {
        self.stream_events_with_identity(
            from_event_id,
            plugin_filters,
            tag_filters,
            &private_plugin_identity()?,
        )
        .await
    }

    /// Forward an explicit caller credential; never substitute the service
    /// identity when adapting an authenticated outer request.
    pub async fn stream_events_with_identity(
        &self,
        from_event_id: Option<u64>,
        plugin_filters: Vec<String>,
        tag_filters: Vec<String>,
        identity: &GhostbridgeCallMetadata,
    ) -> Result<
        impl tokio_stream::Stream<Item = Result<ChainEventMessage, GrpcClientError>> + use<>,
        GrpcClientError,
    > {
        if plugin_filters.len() > 1 {
            return Err(GrpcClientError::RequestFailed(
                "audit subscription supports at most one plugin filter".into(),
            ));
        }
        let mut request = Request::new(SubscribeEventsRequest {
            from_event_id: from_event_id.unwrap_or_default(),
            plugin_id: plugin_filters.first().cloned().unwrap_or_default(),
            tags: tag_filters,
        });
        attach_plugin_identity_metadata(&mut request, identity, "snowball.read")?;

        let mut client = self.pool.event_chain_client(&self.default_address).await?;

        let response = client
            .subscribe_events(request)
            .await
            .map_err(|e| GrpcClientError::RequestFailed(e.to_string()))?;

        let stream = response.into_inner();

        Ok(tokio_stream::StreamExt::map(stream, |result| {
            result
                .map(|event| ChainEventMessage {
                    event_id: event.event_id.to_string(),
                    event_hash: event.event_hash,
                    prev_hash: event.prev_hash,
                    plugin_id: event.plugin_id,
                    operation_type: event.operation_type,
                    target: event.target,
                    decision: event.decision.to_string(),
                    tags_touched: event.tags_touched,
                })
                .map_err(|e| GrpcClientError::StreamError(e.to_string()))
        }))
    }
}

/// Result of a set state operation
#[derive(Debug, Clone)]
pub struct SetStateResult {
    pub event_id: u64,
    pub effective_hash: String,
}

/// State update message from subscription
#[derive(Debug, Clone)]
pub struct StateUpdateMessage {
    pub plugin_id: String,
    pub object_path: String,
    pub property_name: Option<String>,
    pub new_value: Option<simd_json::OwnedValue>,
    pub event_id: String,
    pub tags_touched: Vec<String>,
    pub change_type: i32,
    pub frame_kind: i32,
    pub schema_hash: String,
    pub catalog_hash: String,
    pub state_generation: u64,
}

fn state_update_message(update: crate::proto::StateChange) -> StateUpdateMessage {
    StateUpdateMessage {
        plugin_id: update.plugin_id,
        object_path: update.object_path,
        property_name: if update.member_name.is_empty() {
            None
        } else {
            Some(update.member_name)
        },
        new_value: update.new_value.as_ref().map(prost_value_to_simd),
        event_id: update.event_id.to_string(),
        tags_touched: update.tags_touched,
        change_type: update.change_type,
        frame_kind: update.frame_kind,
        schema_hash: update.schema_hash,
        catalog_hash: update.catalog_hash,
        state_generation: update.state_generation,
    }
}

/// Chain event message from event stream
#[derive(Debug, Clone)]
pub struct ChainEventMessage {
    pub event_id: String,
    pub event_hash: String,
    pub prev_hash: String,
    pub plugin_id: String,
    pub operation_type: String,
    pub target: String,
    pub decision: String,
    pub tags_touched: Vec<String>,
}

/// Errors that can occur in gRPC client operations
#[derive(Debug, Clone)]
pub enum GrpcClientError {
    ConnectionFailed(String),
    RequestFailed(String),
    StreamError(String),
    ParseError(String),
    RemoteError { code: String, message: String },
}

impl std::fmt::Display for GrpcClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ConnectionFailed(msg) => write!(f, "Connection failed: {}", msg),
            Self::RequestFailed(msg) => write!(f, "Request failed: {}", msg),
            Self::StreamError(msg) => write!(f, "Stream error: {}", msg),
            Self::ParseError(msg) => write!(f, "Parse error: {}", msg),
            Self::RemoteError { code, message } => {
                write!(f, "Remote error [{}]: {}", code, message)
            }
        }
    }
}

impl std::error::Error for GrpcClientError {}

/// Identity for a host service call.
///
/// A named `OP_IDENTITY_SESSION_ID` still wins. Otherwise a single human
/// session whose container is running is the logged-in caller. When no human
/// is logged in, the control-plane chatbot is the caller. That service sled
/// is never handed out by the general implicit resolver.
fn host_service_session() -> Result<op_identity::SessionIdentity, GrpcClientError> {
    let selector = std::env::var(op_identity::SESSION_SELECTOR_ENV)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            std::env::var("IDENTITY_SLED_HOST_SESSION_ID")
                .ok()
                .filter(|value| !value.trim().is_empty())
        });
    if selector.is_some() {
        return op_identity::resolve_identity_credential_session(selector.as_deref()).map_err(
            |error| {
                GrpcClientError::RequestFailed(format!("private identity unavailable: {error}"))
            },
        );
    }

    let sessions = op_identity::read_identity_credential_sessions().map_err(|error| {
        GrpcClientError::RequestFailed(format!("private identity unavailable: {error}"))
    })?;
    let mut logged_in_humans: Vec<_> = sessions
        .iter()
        .filter(|session| {
            session.is_current() && session.may_resolve_implicitly() && session.active
        })
        .cloned()
        .collect();
    if logged_in_humans.len() == 1 {
        return Ok(logged_in_humans.pop().expect("one logged-in human"));
    }
    if logged_in_humans.len() > 1 {
        return Err(GrpcClientError::RequestFailed(format!(
            "private identity unavailable: {}",
            op_identity::SessionProjectionError::Ambiguous(logged_in_humans.len())
        )));
    }

    let mut chatbots: Vec<_> = sessions
        .into_iter()
        .filter(|session| session.is_current() && session.is_service_principal())
        .collect();
    if chatbots.len() == 1 {
        return Ok(chatbots.pop().expect("one chatbot session"));
    }
    if chatbots.is_empty() {
        return Err(GrpcClientError::RequestFailed(
            "private identity unavailable: no logged-in human and no control-plane chatbot session"
                .into(),
        ));
    }
    Err(GrpcClientError::RequestFailed(format!(
        "private identity unavailable: {}",
        op_identity::SessionProjectionError::Ambiguous(chatbots.len())
    )))
}

fn private_plugin_identity() -> Result<GhostbridgeCallMetadata, GrpcClientError> {
    let identity = host_service_session()?;
    let encoded = identity
        .sealed_id_header_for_transport("grpc")
        .map_err(|e| GrpcClientError::RequestFailed(format!("private SID1 unavailable: {e}")))?;
    let wire = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(encoded)
        .map_err(|e| {
            GrpcClientError::RequestFailed(format!("private SID1 is not canonical: {e}"))
        })?;
    Ok(GhostbridgeCallMetadata {
        sealed_id: Some(wire),
        oracle_assertion: None,
        session_genesis: None,
        trace_id: None,
        wireguard_pubkey: None,
    })
}

pub(crate) const HOST_SERVICE_LOGIN_HEADER: &str = "x-opdbus-host-service";

fn attach_plugin_identity_metadata<T>(
    request: &mut Request<T>,
    identity: &GhostbridgeCallMetadata,
    capability_id: &str,
) -> Result<(), GrpcClientError> {
    attach_identity_metadata(request, identity, Some(capability_id))?;
    request
        .metadata_mut()
        .insert(HOST_SERVICE_LOGIN_HEADER, MetadataValue::from_static("1"));
    Ok(())
}

fn attach_identity_metadata<T>(
    request: &mut Request<T>,
    identity: &GhostbridgeCallMetadata,
    capability_id: Option<&str>,
) -> Result<(), GrpcClientError> {
    crate::interceptor::insert_identity_metadata(
        request,
        identity.sealed_id.as_deref(),
        identity.oracle_assertion.as_deref(),
        capability_id,
    )
    .map_err(|error| GrpcClientError::RequestFailed(error.to_string()))
}

fn decode_get_state_response(
    response: GetStateResponse,
) -> Result<simd_json::OwnedValue, GrpcClientError> {
    if let Some(value) = response.value {
        validate_prost_value(&value)?;
        return Ok(prost_value_to_simd(&value));
    }
    if let Some(state) = response.state {
        for value in state.fields.values() {
            validate_prost_value(value)?;
        }
        return Ok(prost_struct_to_simd(&state));
    }
    Err(GrpcClientError::ParseError(
        "GetStateResponse omitted both value and legacy state payloads".into(),
    ))
}

fn validate_prost_value(value: &ProstValue) -> Result<(), GrpcClientError> {
    match value.kind.as_ref() {
        None => Err(GrpcClientError::ParseError(
            "protobuf value has no kind".into(),
        )),
        Some(ProstKind::NumberValue(number)) if !number.is_finite() => Err(
            GrpcClientError::ParseError("protobuf value contains a non-finite number".into()),
        ),
        Some(ProstKind::StructValue(object)) => {
            for nested in object.fields.values() {
                validate_prost_value(nested)?;
            }
            Ok(())
        }
        Some(ProstKind::ListValue(list)) => {
            for nested in &list.values {
                validate_prost_value(nested)?;
            }
            Ok(())
        }
        Some(_) => Ok(()),
    }
}

fn prost_value_to_simd(value: &ProstValue) -> simd_json::OwnedValue {
    let serde_value = prost_value_to_serde(value);
    simd_json::serde::to_owned_value(&serde_value).unwrap_or_else(|_| simd_json::json!(null))
}

fn prost_struct_to_simd(value: &ProstStruct) -> simd_json::OwnedValue {
    let serde_value = serde_json::Value::Object(
        value
            .fields
            .iter()
            .map(|(k, v)| (k.clone(), prost_value_to_serde(v)))
            .collect(),
    );
    simd_json::serde::to_owned_value(&serde_value).unwrap_or_else(|_| simd_json::json!(null))
}

fn prost_value_to_serde(value: &ProstValue) -> serde_json::Value {
    match &value.kind {
        None => serde_json::Value::Null,
        Some(ProstKind::NullValue(_)) => serde_json::Value::Null,
        Some(ProstKind::BoolValue(b)) => serde_json::Value::Bool(*b),
        Some(ProstKind::NumberValue(n)) => serde_json::Number::from_f64(*n)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
        Some(ProstKind::StringValue(s)) => serde_json::Value::String(s.clone()),
        Some(ProstKind::StructValue(s)) => serde_json::Value::Object(
            s.fields
                .iter()
                .map(|(k, v)| (k.clone(), prost_value_to_serde(v)))
                .collect(),
        ),
        Some(ProstKind::ListValue(l)) => {
            serde_json::Value::Array(l.values.iter().map(prost_value_to_serde).collect())
        }
    }
}

fn simd_to_prost_value(value: &simd_json::OwnedValue) -> ProstValue {
    let json = simd_json::to_string(value).unwrap_or_else(|_| "null".to_string());
    let serde_value: serde_json::Value =
        serde_json::from_str(&json).unwrap_or(serde_json::Value::Null);
    serde_to_prost_value(&serde_value)
}

fn serde_to_prost_value(value: &serde_json::Value) -> ProstValue {
    match value {
        serde_json::Value::Null => ProstValue {
            kind: Some(ProstKind::NullValue(0)),
        },
        serde_json::Value::Bool(b) => ProstValue {
            kind: Some(ProstKind::BoolValue(*b)),
        },
        serde_json::Value::Number(n) => ProstValue {
            kind: Some(ProstKind::NumberValue(n.as_f64().unwrap_or(0.0))),
        },
        serde_json::Value::String(s) => ProstValue {
            kind: Some(ProstKind::StringValue(s.clone())),
        },
        serde_json::Value::Array(arr) => ProstValue {
            kind: Some(ProstKind::ListValue(prost_types::ListValue {
                values: arr.iter().map(serde_to_prost_value).collect(),
            })),
        },
        serde_json::Value::Object(map) => ProstValue {
            kind: Some(ProstKind::StructValue(ProstStruct {
                fields: map
                    .iter()
                    .map(|(k, v)| (k.clone(), serde_to_prost_value(v)))
                    .collect(),
            })),
        },
    }
}

#[cfg(test)]
mod identity_metadata_tests {
    use super::*;

    #[test]
    fn plugin_metadata_forwards_exact_sid1_and_capability() {
        let wire = b"SID1-authored".to_vec();
        let identity = GhostbridgeCallMetadata {
            sealed_id: Some(wire.clone()),
            oracle_assertion: None,
            session_genesis: None,
            trace_id: None,
            wireguard_pubkey: None,
        };
        let mut request = Request::new(());
        attach_plugin_identity_metadata(&mut request, &identity, "router.invoke").unwrap();
        assert_eq!(
            request
                .metadata()
                .get_bin(op_identity::sealed_id::HTTP_HEADER_NAME)
                .unwrap()
                .to_bytes()
                .unwrap(),
            wire.as_slice()
        );
        assert_eq!(
            request.metadata().get("x-opdbus-capability").unwrap(),
            "router.invoke"
        );
    }

    #[test]
    fn plugin_metadata_rejects_missing_or_dual_credentials() {
        for identity in [
            GhostbridgeCallMetadata {
                sealed_id: None,
                oracle_assertion: None,
                session_genesis: None,
                trace_id: None,
                wireguard_pubkey: None,
            },
            GhostbridgeCallMetadata {
                sealed_id: Some(vec![1]),
                oracle_assertion: Some(vec![2]),
                session_genesis: None,
                trace_id: None,
                wireguard_pubkey: None,
            },
        ] {
            let mut request = Request::new(());
            assert!(attach_plugin_identity_metadata(&mut request, &identity, "x").is_err());
        }
    }

    #[test]
    fn state_metadata_accepts_explicit_sid1_with_optional_exact_capability() {
        let identity = GhostbridgeCallMetadata {
            sealed_id: Some(b"SID1-authored-state-reader".to_vec()),
            oracle_assertion: None,
            session_genesis: None,
            trace_id: None,
            wireguard_pubkey: None,
        };
        let mut public = Request::new(GetStateRequest::default());
        attach_identity_metadata(&mut public, &identity, None).unwrap();
        assert!(
            public
                .metadata()
                .get_bin(op_identity::sealed_id::HTTP_HEADER_NAME)
                .is_some()
        );
        assert!(public.metadata().get("x-opdbus-capability").is_none());

        let mut private = Request::new(SubscribeRequest::default());
        attach_identity_metadata(&mut private, &identity, Some("gemma.read")).unwrap();
        assert_eq!(
            private.metadata().get("x-opdbus-capability").unwrap(),
            "gemma.read"
        );
    }

    #[test]
    fn state_metadata_rejects_malformed_multiple_and_empty_capability() {
        let malformed = GhostbridgeCallMetadata {
            sealed_id: Some(b"not-sid1".to_vec()),
            oracle_assertion: None,
            session_genesis: None,
            trace_id: None,
            wireguard_pubkey: None,
        };
        let mut request = Request::new(());
        assert!(attach_identity_metadata(&mut request, &malformed, None).is_err());

        let multiple = GhostbridgeCallMetadata {
            sealed_id: Some(b"SID1-one".to_vec()),
            oracle_assertion: Some(b"OIA1-two".to_vec()),
            session_genesis: None,
            trace_id: None,
            wireguard_pubkey: None,
        };
        let mut request = Request::new(());
        assert!(attach_identity_metadata(&mut request, &multiple, None).is_err());

        let valid = GhostbridgeCallMetadata {
            sealed_id: Some(b"SID1-one".to_vec()),
            oracle_assertion: None,
            session_genesis: None,
            trace_id: None,
            wireguard_pubkey: None,
        };
        let mut request = Request::new(());
        assert!(attach_identity_metadata(&mut request, &valid, Some("")).is_err());
    }

    #[test]
    fn get_state_prefers_exact_value_and_falls_back_to_legacy_object() {
        let scalar = GetStateResponse {
            value: Some(serde_to_prost_value(&serde_json::json!("leaf"))),
            state: Some(ProstStruct {
                fields: [(
                    "legacy".into(),
                    serde_to_prost_value(&serde_json::json!(true)),
                )]
                .into_iter()
                .collect(),
            }),
            ..Default::default()
        };
        assert_eq!(
            decode_get_state_response(scalar).unwrap(),
            simd_json::json!("leaf")
        );

        let array = GetStateResponse {
            value: Some(serde_to_prost_value(&serde_json::json!([1, 2, 3]))),
            ..Default::default()
        };
        assert_eq!(
            decode_get_state_response(array).unwrap(),
            // google.protobuf.Value numbers use the double-precision wire form.
            simd_json::json!([1.0, 2.0, 3.0])
        );

        let legacy = GetStateResponse {
            state: Some(ProstStruct {
                fields: [(
                    "legacy".into(),
                    serde_to_prost_value(&serde_json::json!(true)),
                )]
                .into_iter()
                .collect(),
            }),
            ..Default::default()
        };
        assert_eq!(
            decode_get_state_response(legacy).unwrap(),
            simd_json::json!({"legacy": true})
        );
    }

    #[test]
    fn get_state_rejects_missing_or_malformed_payloads() {
        assert!(decode_get_state_response(GetStateResponse::default()).is_err());
        assert!(
            decode_get_state_response(GetStateResponse {
                value: Some(ProstValue { kind: None }),
                ..Default::default()
            })
            .is_err()
        );
        assert!(
            decode_get_state_response(GetStateResponse {
                value: Some(ProstValue {
                    kind: Some(ProstKind::NumberValue(f64::NAN)),
                }),
                ..Default::default()
            })
            .is_err()
        );
    }

    #[test]
    fn subscription_conversion_preserves_state_and_contract_generations() {
        let message = state_update_message(crate::proto::StateChange {
            event_id: 17,
            plugin_id: "web_ui".into(),
            object_path: "/org/opdbus/v1/plugins/web_ui/page".into(),
            member_name: "spec".into(),
            new_value: Some(serde_to_prost_value(&serde_json::json!(["child"]))),
            tags_touched: vec!["ui".into()],
            change_type: 6,
            frame_kind: 2,
            schema_hash: "ab".repeat(32),
            catalog_hash: "cd".repeat(32),
            state_generation: 42,
            ..Default::default()
        });
        assert_eq!(message.event_id, "17");
        assert_eq!(message.property_name.as_deref(), Some("spec"));
        assert_eq!(message.new_value, Some(simd_json::json!(["child"])));
        assert_eq!(message.frame_kind, 2);
        assert_eq!(message.schema_hash, "ab".repeat(32));
        assert_eq!(message.catalog_hash, "cd".repeat(32));
        assert_eq!(message.state_generation, 42);
    }
}
