//! Bounded adapter for the pinned binary's stateless generation WebSocket.
//!
//! The native endpoint has no task identifier: MutationEngine owns the job identity,
//! while this adapter owns one native WebSocket/session at a time.

use std::{future::Future, pin::Pin, sync::Arc, time::Duration};

use futures::{Sink, SinkExt, Stream, StreamExt};
use serde_json::Value;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio_tungstenite::{
    connect_async_with_config,
    tungstenite::{Message, client::IntoClientRequest, protocol::WebSocketConfig},
};

const WS_PROTOCOL: &str = "tched_router.generation.v1";
const MAX_PROMPT_BYTES: usize = 256 * 1024;
// JSON escaping expands decoded text; bound allocation separately from output.
const MAX_FRAME_BYTES: usize = 2 * 1024 * 1024;
const MAX_DELTA_BYTES: usize = 16 * 1024;
const MAX_OUTPUT_BYTES: usize = 256 * 1024;
const MAX_EVENTS: usize = 4096;
const MAX_SESSION_ID_BYTES: usize = 256;
const MAX_AGENT_ALIAS_BYTES: usize = 128;
const MAX_TOKEN_BYTES: usize = 8 * 1024;
const TOTAL_TIMEOUT: Duration = Duration::from_secs(600);

#[derive(Clone)]
pub(crate) struct NativeGenerationRequest {
    pub(crate) endpoint: String,
    pub(crate) bearer_token: Option<String>,
    pub(crate) agent_alias: String,
    pub(crate) native_session_id: String,
    pub(crate) prompt: String,
    /// Optional extra selection constraints. The stateless ACK must always
    /// identify the native turn's provider and model before prompt release,
    /// and terminal attribution must match that ACK.
    pub(crate) expected_provider: Option<String>,
    pub(crate) expected_model: Option<String>,
    /// Projection-owned canonical SID1 payload, forwarded only as sensitive
    /// transport metadata and never included in the prompt or logs.
    pub(crate) sealed_identity: Option<String>,
    /// Recheck the owner's identity, grants and sealed contract before input.
    pub(crate) before_prompt: Arc<dyn NativeGenerationAdmission>,
}

pub(crate) trait NativeGenerationAdmission: Send + Sync {
    /// Canonical SID1 payload to forward in native transport metadata.
    /// Legacy test admissions return `None`; production admissions must carry
    /// the projection-owned credential.
    fn sealed_identity(&self) -> Option<&str> {
        None
    }

    fn check(&self)
    -> Pin<Box<dyn Future<Output = Result<(), NativeGenerationError>> + Send + '_>>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum NativeGenerationEvent {
    TextDelta { delta: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct NativeGenerationUsage {
    pub(crate) input_tokens: Option<u64>,
    pub(crate) output_tokens: Option<u64>,
    pub(crate) tokens_used: Option<u64>,
    pub(crate) cost_usd_micros: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct NativeGenerationDone {
    pub(crate) full_response: String,
    pub(crate) provider: Option<String>,
    pub(crate) model: Option<String>,
    pub(crate) usage: NativeGenerationUsage,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum NativeGenerationTerminal {
    Done(NativeGenerationDone),
    Cancelled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NativeErrorCode {
    Auth,
    Provider,
    ProviderHttp(u16),
    ProviderTransport,
    ProviderResponse,
    ProviderConfiguration,
    ApprovalRequired,
    Protocol,
    Bounds,
    Timeout,
}

impl std::fmt::Display for NativeErrorCode {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Provider => formatter.write_str("model provider failed to generate a response"),
            Self::ProviderHttp(status) => {
                write!(formatter, "model provider returned HTTP {status}")
            }
            Self::ProviderTransport => {
                formatter.write_str("connection to the model provider failed")
            }
            Self::ProviderResponse => {
                formatter.write_str("model provider returned an invalid response")
            }
            Self::ProviderConfiguration => {
                formatter.write_str("model provider configuration is invalid")
            }
            _ => write!(
                formatter,
                "native generation protocol rejected the request: {self:?}"
            ),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum NativeGenerationError {
    #[error("invalid native generation request: {0}")]
    InvalidRequest(&'static str),
    #[error("native generation transport failed")]
    Transport,
    #[error("native generation timed out")]
    Timeout,
    #[error("native generation was cancelled")]
    Cancelled,
    #[error("{0}")]
    Terminal(NativeErrorCode),
}

/// Owned native turn. Dropping it requests cancellation and aborts the task;
/// the owner must map that terminal condition into its private SHM job state.
pub(crate) struct NativeGenerationTask {
    cancel: Option<oneshot::Sender<()>>,
    events: mpsc::Receiver<NativeGenerationEvent>,
    join: Option<JoinHandle<Result<NativeGenerationTerminal, NativeGenerationError>>>,
}

impl NativeGenerationTask {
    pub(crate) fn spawn(request: NativeGenerationRequest) -> Result<Self, NativeGenerationError> {
        validate_request(&request)?;
        let (cancel, cancel_rx) = oneshot::channel();
        let (event_tx, events) = mpsc::channel(64);
        let join = tokio::spawn(run_native(request, cancel_rx, event_tx));
        Ok(Self {
            cancel: Some(cancel),
            events,
            join: Some(join),
        })
    }

    pub(crate) async fn recv_event(&mut self) -> Option<NativeGenerationEvent> {
        self.events.recv().await
    }

    /// Drain only an event already queued by the bounded native transport.
    /// Empty never waits for another fragment or for the terminal result.
    pub(crate) fn try_recv_event(
        &mut self,
    ) -> Result<NativeGenerationEvent, mpsc::error::TryRecvError> {
        self.events.try_recv()
    }

    pub(crate) async fn wait(mut self) -> Result<NativeGenerationTerminal, NativeGenerationError> {
        let join = self.join.take().ok_or(NativeGenerationError::Transport)?;
        let cancel_guard = self.cancel.take();
        let result = join.await.map_err(|_| NativeGenerationError::Transport)?;
        drop(cancel_guard);
        result
    }

    pub(crate) fn cancel(&mut self) {
        if let Some(cancel) = self.cancel.take() {
            let _ = cancel.send(());
        }
    }
}

impl Drop for NativeGenerationTask {
    fn drop(&mut self) {
        self.cancel();
    }
}

async fn run_native(
    request: NativeGenerationRequest,
    mut cancel_rx: oneshot::Receiver<()>,
    events_tx: mpsc::Sender<NativeGenerationEvent>,
) -> Result<NativeGenerationTerminal, NativeGenerationError> {
    let url = ws_url(&request)?;
    let mut handshake = url.into_client_request().map_err(|_| {
        NativeGenerationError::InvalidRequest("failed to construct WebSocket request")
    })?;
    handshake.headers_mut().insert(
        "Sec-WebSocket-Protocol",
        WS_PROTOCOL
            .parse()
            .map_err(|_| NativeGenerationError::InvalidRequest("invalid WebSocket protocol"))?,
    );
    if let Some(token) = request.bearer_token.as_deref() {
        let mut authorization =
            tokio_tungstenite::tungstenite::http::HeaderValue::from_str(&format!("Bearer {token}"))
                .map_err(|_| NativeGenerationError::InvalidRequest("invalid bearer token"))?;
        authorization.set_sensitive(true);
        handshake
            .headers_mut()
            .insert("Authorization", authorization);
    }
    if let Some(sealed_identity) = request.sealed_identity.as_deref() {
        let header = sid1_header_value(sealed_identity)?;
        handshake
            .headers_mut()
            .insert("x-opdbus-sealed-id-bin", header);
    }
    let config = WebSocketConfig {
        max_message_size: Some(MAX_FRAME_BYTES),
        max_frame_size: Some(MAX_FRAME_BYTES),
        ..Default::default()
    };
    let deadline = tokio::time::Instant::now() + TOTAL_TIMEOUT;
    let connect_deadline = std::cmp::min(
        deadline,
        tokio::time::Instant::now() + Duration::from_secs(15),
    );
    let (mut socket, response) = tokio::select! {
        _ = &mut cancel_rx => return Err(NativeGenerationError::Cancelled),
        result = tokio::time::timeout_at(connect_deadline, connect_async_with_config(handshake, Some(config), false)) => {
            result.map_err(|_| NativeGenerationError::Timeout)?
                .map_err(|error| {
                    log_threetched_connection_failure("handshake", &error);
                    NativeGenerationError::Transport
                })?
        }
    };
    let selected_protocol = response
        .headers()
        .get("Sec-WebSocket-Protocol")
        .and_then(|value| value.to_str().ok());
    if selected_protocol != Some(WS_PROTOCOL) {
        let _ = close_socket(&mut socket).await;
        return Err(NativeGenerationError::Terminal(NativeErrorCode::Protocol));
    }

    let connect = serde_json::json!({
        "type": "connect",
        "session_id": request.native_session_id,
    });
    send_json(&mut socket, connect, &mut cancel_rx, deadline).await?;
    let mut events = 0usize;
    let session_start = recv_message(&mut socket, &mut cancel_rx, deadline).await?;
    let session_start = parse_json_frame(session_start)?;
    if session_start.get("type").and_then(Value::as_str) != Some("session_start")
        || session_start.get("session_id").and_then(Value::as_str)
            != Some(request.native_session_id.as_str())
        || session_start.get("resumed").and_then(Value::as_bool) != Some(false)
        || session_start.get("message_count").and_then(Value::as_u64) != Some(0)
        || session_start
            .get("stateless_generation")
            .and_then(Value::as_u64)
            != Some(1)
    {
        let _ = close_socket(&mut socket).await;
        return Err(NativeGenerationError::Terminal(NativeErrorCode::Protocol));
    }
    let connected = parse_json_frame(recv_message(&mut socket, &mut cancel_rx, deadline).await?)?;
    if connected.get("type").and_then(Value::as_str) != Some("connected")
        || connected
            .get("stateless_generation")
            .and_then(Value::as_u64)
            != Some(1)
    {
        let _ = close_socket(&mut socket).await;
        return Err(NativeGenerationError::Terminal(NativeErrorCode::Protocol));
    }
    if !identity_ack_is_authenticated(request.sealed_identity.is_some(), &connected) {
        let _ = close_socket(&mut socket).await;
        return Err(NativeGenerationError::Terminal(NativeErrorCode::Auth));
    }
    let bound_provider = bounded_label(connected.get("provider"))
        .ok_or(NativeGenerationError::Terminal(NativeErrorCode::Protocol))?;
    let bound_model = bounded_label(connected.get("model"))
        .ok_or(NativeGenerationError::Terminal(NativeErrorCode::Protocol))?;
    verify_selection(&request, Some(&bound_provider), Some(&bound_model))?;
    tokio::select! {
        _ = &mut cancel_rx => {
            let _ = close_socket(&mut socket).await;
            return Err(NativeGenerationError::Cancelled);
        }
        _ = tokio::time::sleep_until(deadline) => {
            let _ = close_socket(&mut socket).await;
            return Err(NativeGenerationError::Timeout);
        }
        result = request.before_prompt.check() => result?,
    }
    send_json(
        &mut socket,
        serde_json::json!({
            "type": "message",
            "content": request.prompt,
        }),
        &mut cancel_rx,
        deadline,
    )
    .await?;
    let mut output = String::new();
    loop {
        let message = tokio::select! {
            _ = &mut cancel_rx => {
                let _ = close_socket(&mut socket).await;
                return Err(NativeGenerationError::Cancelled);
            }
            _ = tokio::time::sleep_until(deadline) => {
                let _ = close_socket(&mut socket).await;
                return Err(NativeGenerationError::Timeout);
            }
            message = socket.next() => message,
        };
        let Some(message) = message else {
            tracing::warn!(
                stage = "receive_output",
                kind = "ended",
                "threetched generation connection failed"
            );
            return Err(NativeGenerationError::Transport);
        };
        let message = message.map_err(|error| {
            log_threetched_connection_failure("receive_output", &error);
            NativeGenerationError::Transport
        })?;
        let text = match message {
            Message::Text(text) => text,
            Message::Binary(bytes) => {
                if bytes.len() > MAX_FRAME_BYTES {
                    let _ = close_socket(&mut socket).await;
                    return Err(NativeGenerationError::Terminal(NativeErrorCode::Bounds));
                }
                continue;
            }
            Message::Close(_) => {
                tracing::warn!(
                    stage = "receive_output",
                    kind = "closed",
                    "threetched generation connection failed"
                );
                return Err(NativeGenerationError::Transport);
            }
            Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => continue,
        };
        let bytes = text.as_bytes();
        if bytes.len() > MAX_FRAME_BYTES {
            let _ = close_socket(&mut socket).await;
            return Err(NativeGenerationError::Terminal(NativeErrorCode::Bounds));
        }
        events = events.saturating_add(1);
        if events > MAX_EVENTS {
            let _ = close_socket(&mut socket).await;
            return Err(NativeGenerationError::Terminal(NativeErrorCode::Bounds));
        }
        let value: Value = serde_json::from_str(text.as_ref())
            .map_err(|_| NativeGenerationError::Terminal(NativeErrorCode::Protocol))?;
        match value.get("type").and_then(Value::as_str).unwrap_or("") {
            "chunk" => {
                let delta = value
                    .get("content")
                    .and_then(Value::as_str)
                    .ok_or(NativeGenerationError::Terminal(NativeErrorCode::Protocol))?;
                if delta.len() > MAX_DELTA_BYTES {
                    let _ = close_socket(&mut socket).await;
                    return Err(NativeGenerationError::Terminal(NativeErrorCode::Bounds));
                }
                if output.len().saturating_add(delta.len()) > MAX_OUTPUT_BYTES {
                    let _ = close_socket(&mut socket).await;
                    return Err(NativeGenerationError::Terminal(NativeErrorCode::Bounds));
                }
                output.push_str(delta);
                tokio::select! {
                    _ = &mut cancel_rx => {
                        let _ = close_socket(&mut socket).await;
                        return Err(NativeGenerationError::Cancelled);
                    }
                    _ = tokio::time::sleep_until(deadline) => {
                        let _ = close_socket(&mut socket).await;
                        return Err(NativeGenerationError::Timeout);
                    }
                    result = events_tx.send(NativeGenerationEvent::TextDelta {
                        delta: delta.to_owned(),
                    }) => result.map_err(|_| NativeGenerationError::Cancelled)?,
                }
            }
            "thinking" | "plan" | "history_trimmed" => {}
            "tool_call" | "tool_result" | "session_start" => {
                let _ = close_socket(&mut socket).await;
                return Err(NativeGenerationError::Terminal(NativeErrorCode::Protocol));
            }
            "approval_request" => {
                let _ = close_socket(&mut socket).await;
                return Err(NativeGenerationError::Terminal(
                    NativeErrorCode::ApprovalRequired,
                ));
            }
            "done" => {
                let full_response = bounded_text(value.get("full_response"), MAX_OUTPUT_BYTES)
                    .ok_or(NativeGenerationError::Terminal(NativeErrorCode::Protocol))?;
                if full_response != output {
                    return Err(NativeGenerationError::Terminal(NativeErrorCode::Protocol));
                }
                let provider = bounded_label(value.get("provider"));
                let model = bounded_label(value.get("model"));
                verify_selection(&request, provider.as_deref(), model.as_deref())?;
                if provider.as_deref() != Some(bound_provider.as_str())
                    || model.as_deref() != Some(bound_model.as_str())
                {
                    return Err(NativeGenerationError::Terminal(NativeErrorCode::Protocol));
                }
                let usage = NativeGenerationUsage {
                    input_tokens: optional_u64(&value, "input_tokens")?,
                    output_tokens: optional_u64(&value, "output_tokens")?,
                    tokens_used: optional_u64(&value, "tokens_used")?,
                    cost_usd_micros: optional_cost(&value)?,
                };
                let _ = close_socket(&mut socket).await;
                return Ok(NativeGenerationTerminal::Done(NativeGenerationDone {
                    full_response,
                    provider,
                    model,
                    usage,
                }));
            }
            "aborted" => return Ok(NativeGenerationTerminal::Cancelled),
            "error" => {
                let code = match value.get("code").and_then(Value::as_str).unwrap_or("") {
                    "AUTH_ERROR" => NativeErrorCode::Auth,
                    "PROVIDER_ERROR" => provider_error_code(&value),
                    "TIMEOUT" => NativeErrorCode::Timeout,
                    "BOUNDS" => NativeErrorCode::Bounds,
                    _ => NativeErrorCode::Protocol,
                };
                let _ = close_socket(&mut socket).await;
                return Err(NativeGenerationError::Terminal(code));
            }
            _ => return Err(NativeGenerationError::Terminal(NativeErrorCode::Protocol)),
        }
    }
}

/// Deliberately excludes peer bodies, headers, URLs and arbitrary error text.
/// Only exact gateway-owned rejection codes may be retained from an HTTP body.
#[derive(Debug, PartialEq, Eq)]
struct ThreetchedConnectionFailure {
    kind: &'static str,
    http_status: Option<u16>,
    gateway_reason: &'static str,
}

fn log_threetched_connection_failure(
    stage: &'static str,
    error: &tokio_tungstenite::tungstenite::Error,
) {
    let failure = threetched_connection_failure(error);
    tracing::warn!(
        stage,
        kind = failure.kind,
        http_status = ?failure.http_status,
        gateway_reason = failure.gateway_reason,
        "threetched generation connection failed"
    );
}

fn threetched_connection_failure(
    error: &tokio_tungstenite::tungstenite::Error,
) -> ThreetchedConnectionFailure {
    use tokio_tungstenite::tungstenite::Error;
    let mut failure = ThreetchedConnectionFailure {
        kind: "other",
        http_status: None,
        gateway_reason: "unspecified",
    };
    failure.kind = match error {
        Error::Http(response) => {
            failure.http_status = Some(response.status().as_u16());
            failure.gateway_reason = match response.body().as_deref() {
                Some(b"stateless_generation_not_admitted") => "not_admitted",
                Some(b"stateless_generation_invalid_session") => "invalid_session",
                Some(b"stateless_generation_identity_rejected") => "identity_rejected",
                Some(b"stateless_generation_identity_required") => "identity_required",
                Some(b"stateless_generation_unavailable") => "generation_unavailable",
                _ => "unspecified",
            };
            "http"
        }
        Error::Io(_) => "io",
        Error::Tls(_) => "tls",
        Error::Protocol(_) => "protocol",
        Error::Capacity(_) => "capacity",
        Error::ConnectionClosed | Error::AlreadyClosed => "closed",
        _ => "other",
    };
    failure
}

/// Accept only the native protocol's bounded diagnostic vocabulary. Provider
/// response bodies, URLs, credentials and arbitrary error text never leave
/// this boundary, including when an older or malformed peer sends them.
fn provider_error_code(frame: &Value) -> NativeErrorCode {
    let Some(detail) = frame.get("provider_error").and_then(Value::as_object) else {
        return NativeErrorCode::Provider;
    };
    match detail.get("kind").and_then(Value::as_str) {
        Some("http") => detail
            .get("status")
            .and_then(Value::as_u64)
            .filter(|status| (400..=599).contains(status))
            .map(|status| NativeErrorCode::ProviderHttp(status as u16))
            .unwrap_or(NativeErrorCode::Provider),
        Some("transport") => NativeErrorCode::ProviderTransport,
        Some("response") => NativeErrorCode::ProviderResponse,
        Some("configuration") => NativeErrorCode::ProviderConfiguration,
        _ => NativeErrorCode::Provider,
    }
}

fn sid1_header_value(
    sealed_identity: &str,
) -> Result<tokio_tungstenite::tungstenite::http::HeaderValue, NativeGenerationError> {
    let mut header = tokio_tungstenite::tungstenite::http::HeaderValue::from_str(sealed_identity)
        .map_err(|_| NativeGenerationError::InvalidRequest("invalid SID1 metadata"))?;
    header.set_sensitive(true);
    Ok(header)
}

fn identity_ack_is_authenticated(identity_forwarded: bool, connected: &Value) -> bool {
    !identity_forwarded
        || connected
            .get("identity_authenticated")
            .and_then(Value::as_str)
            == Some("SID1")
}

async fn send_json<S>(
    socket: &mut S,
    value: Value,
    cancel_rx: &mut oneshot::Receiver<()>,
    deadline: tokio::time::Instant,
) -> Result<(), NativeGenerationError>
where
    S: Sink<Message> + Unpin,
{
    let text = serde_json::to_string(&value).map_err(|_| NativeGenerationError::Transport)?;
    if text.len() > MAX_FRAME_BYTES {
        return Err(NativeGenerationError::Terminal(NativeErrorCode::Bounds));
    }
    tokio::select! {
        _ = cancel_rx => Err(NativeGenerationError::Cancelled),
        _ = tokio::time::sleep_until(deadline) => Err(NativeGenerationError::Timeout),
        result = socket.send(Message::Text(text)) => result.map_err(|_| {
            tracing::warn!(stage = "send_frame", "threetched generation connection failed");
            NativeGenerationError::Transport
        }),
    }
}

async fn recv_message<S>(
    socket: &mut S,
    cancel_rx: &mut oneshot::Receiver<()>,
    deadline: tokio::time::Instant,
) -> Result<Message, NativeGenerationError>
where
    S: Stream<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
    tokio::select! {
        _ = cancel_rx => Err(NativeGenerationError::Cancelled),
        _ = tokio::time::sleep_until(deadline) => Err(NativeGenerationError::Timeout),
        result = socket.next() => {
            let message = result.ok_or_else(|| {
                tracing::warn!(stage = "receive_ack", kind = "ended", "threetched generation connection failed");
                NativeGenerationError::Transport
            })?;
            message.map_err(|error| {
                log_threetched_connection_failure("receive_ack", &error);
                NativeGenerationError::Transport
            })
        },
    }
}

fn parse_json_frame(message: Message) -> Result<Value, NativeGenerationError> {
    let text = match message {
        Message::Text(text) => text,
        Message::Binary(_)
        | Message::Ping(_)
        | Message::Pong(_)
        | Message::Frame(_)
        | Message::Close(_) => {
            return Err(NativeGenerationError::Terminal(NativeErrorCode::Protocol));
        }
    };
    if text.len() > MAX_FRAME_BYTES {
        return Err(NativeGenerationError::Terminal(NativeErrorCode::Bounds));
    }
    serde_json::from_str(text.as_ref())
        .map_err(|_| NativeGenerationError::Terminal(NativeErrorCode::Protocol))
}

async fn close_socket<S>(socket: &mut S) -> Result<(), ()>
where
    S: Sink<Message> + Unpin,
{
    tokio::time::timeout(Duration::from_secs(1), socket.close())
        .await
        .map_err(|_| ())?
        .map_err(|_| ())
}

fn validate_request(request: &NativeGenerationRequest) -> Result<(), NativeGenerationError> {
    let endpoint = reqwest::Url::parse(&request.endpoint)
        .map_err(|_| NativeGenerationError::InvalidRequest("endpoint is not a valid URL"))?;
    if endpoint.scheme() != "http"
        || !endpoint.username().is_empty()
        || endpoint.password().is_some()
        || endpoint.query().is_some()
        || endpoint.fragment().is_some()
        || !matches!(endpoint.path(), "" | "/")
    {
        return Err(NativeGenerationError::InvalidRequest(
            "endpoint must be a loopback origin",
        ));
    }
    let host = endpoint
        .host_str()
        .ok_or(NativeGenerationError::InvalidRequest(
            "endpoint has no host",
        ))?;
    let host_is_loopback = host
        .trim_start_matches('[')
        .trim_end_matches(']')
        .parse::<std::net::IpAddr>()
        .map(|ip| ip.is_loopback())
        .unwrap_or(false);
    if !host_is_loopback {
        return Err(NativeGenerationError::InvalidRequest(
            "endpoint must be loopback",
        ));
    }
    if request.agent_alias.is_empty()
        || request.agent_alias.len() > MAX_AGENT_ALIAS_BYTES
        || !request
            .agent_alias
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err(NativeGenerationError::InvalidRequest("invalid agent alias"));
    }
    if request.native_session_id.is_empty()
        || request.native_session_id.len() > MAX_SESSION_ID_BYTES
        || !request
            .native_session_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err(NativeGenerationError::InvalidRequest(
            "invalid native session id",
        ));
    }
    if request.prompt.is_empty() || request.prompt.len() > MAX_PROMPT_BYTES {
        return Err(NativeGenerationError::InvalidRequest(
            "prompt exceeds bound or is empty",
        ));
    }
    if request.bearer_token.as_ref().is_some_and(|token| {
        token.is_empty()
            || token.len() > MAX_TOKEN_BYTES
            || !token
                .bytes()
                .all(|b| b.is_ascii_graphic() && b != b'\r' && b != b'\n')
    }) {
        return Err(NativeGenerationError::InvalidRequest(
            "bearer token exceeds bound",
        ));
    }
    Ok(())
}

fn ws_url(request: &NativeGenerationRequest) -> Result<String, NativeGenerationError> {
    let mut endpoint = reqwest::Url::parse(&request.endpoint)
        .map_err(|_| NativeGenerationError::InvalidRequest("endpoint is not a valid URL"))?;
    let scheme = match endpoint.scheme() {
        "http" => "ws",
        _ => {
            return Err(NativeGenerationError::InvalidRequest(
                "endpoint must use HTTP",
            ));
        }
    };
    endpoint
        .set_scheme(scheme)
        .map_err(|_| NativeGenerationError::InvalidRequest("failed to set WebSocket scheme"))?;
    endpoint.set_path("/ws/chat");
    endpoint.set_query(Some(&format!(
        "agent_alias={}&session_id={}&stateless_generation=1",
        percent_encode(&request.agent_alias),
        percent_encode(&request.native_session_id),
    )));
    Ok(endpoint.to_string())
}

fn percent_encode(value: &str) -> String {
    value.bytes().fold(String::new(), |mut out, byte| {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
        out
    })
}

fn bounded_text(value: Option<&Value>, max: usize) -> Option<String> {
    let text = value?.as_str()?;
    (text.len() <= max).then(|| text.to_string())
}

fn bounded_label(value: Option<&Value>) -> Option<String> {
    bounded_text(value, 256).filter(|s| !s.is_empty())
}

fn optional_u64(value: &Value, key: &str) -> Result<Option<u64>, NativeGenerationError> {
    match value.get(key) {
        None => Ok(None),
        Some(raw) => raw
            .as_u64()
            .map(Some)
            .ok_or(NativeGenerationError::Terminal(NativeErrorCode::Protocol)),
    }
}

fn optional_cost(value: &Value) -> Result<Option<u64>, NativeGenerationError> {
    let Some(raw) = value.get("cost_usd") else {
        return Ok(None);
    };
    let cost = raw
        .as_f64()
        .ok_or(NativeGenerationError::Terminal(NativeErrorCode::Protocol))?;
    if !cost.is_finite() || cost < 0.0 || cost > (u64::MAX as f64) / 1_000_000.0 {
        return Err(NativeGenerationError::Terminal(NativeErrorCode::Bounds));
    }
    Ok(Some((cost * 1_000_000.0) as u64))
}

fn verify_selection(
    request: &NativeGenerationRequest,
    provider: Option<&str>,
    model: Option<&str>,
) -> Result<(), NativeGenerationError> {
    if request
        .expected_provider
        .as_deref()
        .is_some_and(|expected| provider != Some(expected))
        || request
            .expected_model
            .as_deref()
            .is_some_and(|expected| model != Some(expected))
    {
        return Err(NativeGenerationError::Terminal(NativeErrorCode::Protocol));
    }
    Ok(())
}

#[cfg(test)]
// Tungstenite fixes the handshake callback's error type to an HTTP response.
#[allow(clippy::result_large_err)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;
    use tokio_tungstenite::{
        accept_hdr_async,
        tungstenite::handshake::server::{Request, Response},
    };

    fn request(endpoint: String) -> NativeGenerationRequest {
        NativeGenerationRequest {
            endpoint,
            bearer_token: Some("fixture-token".to_string()),
            agent_alias: "fixture-agent".to_string(),
            native_session_id: "native-fixture-session".to_string(),
            prompt: "hello".to_string(),
            expected_provider: Some("fixture-provider".to_string()),
            expected_model: Some("fixture-model".to_string()),
            sealed_identity: None,
            before_prompt: Arc::new(AllowAdmission),
        }
    }

    struct AllowAdmission;
    impl NativeGenerationAdmission for AllowAdmission {
        fn check(
            &self,
        ) -> Pin<Box<dyn Future<Output = Result<(), NativeGenerationError>> + Send + '_>> {
            Box::pin(async { Ok(()) })
        }
    }

    #[test]
    fn handshake_diagnostics_exclude_untrusted_response_data() {
        for (body, expected_reason) in [
            (
                "stateless_generation_identity_rejected",
                "identity_rejected",
            ),
            ("stateless_generation_unavailable", "generation_unavailable"),
            (
                "stateless_generation_identity_rejected credential-secret",
                "unspecified",
            ),
            (
                "private-prompt https://private.example/ credential-secret",
                "unspecified",
            ),
        ] {
            let response = tokio_tungstenite::tungstenite::http::Response::builder()
                .status(401)
                .header("x-private", "credential-secret")
                .body(Some(body.as_bytes().to_vec()))
                .unwrap();
            let failure = threetched_connection_failure(
                &tokio_tungstenite::tungstenite::Error::Http(response),
            );
            assert_eq!(failure.kind, "http");
            assert_eq!(failure.http_status, Some(401));
            assert_eq!(failure.gateway_reason, expected_reason);
            let rendered = format!("{failure:?}");
            assert!(!rendered.contains("credential-secret"));
            assert!(!rendered.contains("private-prompt"));
            assert!(!rendered.contains("private.example"));
        }
        let error =
            tokio_tungstenite::tungstenite::Error::Io(std::io::Error::other("credential-secret"));
        let failure = threetched_connection_failure(&error);
        assert_eq!(failure.kind, "io");
        assert_eq!(failure.http_status, None);
        assert!(!format!("{failure:?}").contains("credential-secret"));
    }

    #[test]
    fn provider_diagnostics_accept_only_bounded_status_and_categories() {
        for (detail, expected) in [
            (
                serde_json::json!({"kind":"http","status":401}),
                NativeErrorCode::ProviderHttp(401),
            ),
            (
                serde_json::json!({"kind":"http","status":429}),
                NativeErrorCode::ProviderHttp(429),
            ),
            (
                serde_json::json!({"kind":"http","status":503}),
                NativeErrorCode::ProviderHttp(503),
            ),
            (
                serde_json::json!({"kind":"transport"}),
                NativeErrorCode::ProviderTransport,
            ),
            (
                serde_json::json!({"kind":"response"}),
                NativeErrorCode::ProviderResponse,
            ),
            (
                serde_json::json!({"kind":"configuration"}),
                NativeErrorCode::ProviderConfiguration,
            ),
            (
                serde_json::json!({"kind":"unknown"}),
                NativeErrorCode::Provider,
            ),
            (
                serde_json::json!({"kind":"http","status":200}),
                NativeErrorCode::Provider,
            ),
            (
                serde_json::json!({"kind":"http","status":600}),
                NativeErrorCode::Provider,
            ),
            (
                serde_json::json!({"kind":"http","status":-401}),
                NativeErrorCode::Provider,
            ),
            (
                serde_json::json!({"kind":"http","status":"401"}),
                NativeErrorCode::Provider,
            ),
            (
                serde_json::json!({"kind":"http","status":401.5}),
                NativeErrorCode::Provider,
            ),
            (
                serde_json::json!({"kind":"http"}),
                NativeErrorCode::Provider,
            ),
            (
                serde_json::json!({"kind":"credential-secret"}),
                NativeErrorCode::Provider,
            ),
            (
                serde_json::json!("credential-secret"),
                NativeErrorCode::Provider,
            ),
            (Value::Null, NativeErrorCode::Provider),
        ] {
            let frame = serde_json::json!({
                "type":"error", "code":"PROVIDER_ERROR", "provider_error":detail,
                "message":"credential-secret private-prompt https://private.example/",
            });
            let code = provider_error_code(&frame);
            assert_eq!(code, expected);
            let rendered = NativeGenerationError::Terminal(code).to_string();
            assert!(!rendered.contains("credential-secret"));
            assert!(!rendered.contains("private-prompt"));
            assert!(!rendered.contains("private.example"));
        }
        assert_eq!(
            provider_error_code(&serde_json::json!({"code":"PROVIDER_ERROR"})),
            NativeErrorCode::Provider
        );
        assert_eq!(
            NativeGenerationError::Terminal(NativeErrorCode::ProviderHttp(429)).to_string(),
            "model provider returned HTTP 429"
        );
    }

    #[tokio::test]
    async fn provider_http_failure_survives_native_transport_without_response_body() {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let address = listener.local_addr().unwrap();
        let server =
            tokio::spawn(async move {
                let (stream, _) = listener.accept().await.unwrap();
                let mut socket = accept_hdr_async(stream, |_: &Request, mut response: Response| {
                    response
                        .headers_mut()
                        .insert("Sec-WebSocket-Protocol", WS_PROTOCOL.parse().unwrap());
                    Ok(response)
                })
                .await
                .unwrap();
                let _ = socket.next().await;
                socket
                    .send(Message::Text(
                        serde_json::json!({
                            "type":"session_start", "session_id":"native-fixture-session",
                            "resumed":false, "message_count":0, "stateless_generation":1,
                        })
                        .to_string(),
                    ))
                    .await
                    .unwrap();
                socket
                    .send(Message::Text(
                        serde_json::json!({
                            "type":"connected", "stateless_generation":1,
                            "provider":"fixture-provider", "model":"fixture-model",
                        })
                        .to_string(),
                    ))
                    .await
                    .unwrap();
                let _ = socket.next().await;
                socket.send(Message::Text(serde_json::json!({
                "type":"error", "code":"PROVIDER_ERROR",
                "provider_error":{"kind":"http","status":429,"body":"credential-secret"},
                "message":"private-prompt https://private.example/",
            }).to_string())).await.unwrap();
            });
        let error = NativeGenerationTask::spawn(request(format!("http://{address}")))
            .unwrap()
            .wait()
            .await
            .unwrap_err();
        assert!(matches!(
            error,
            NativeGenerationError::Terminal(NativeErrorCode::ProviderHttp(429))
        ));
        assert_eq!(error.to_string(), "model provider returned HTTP 429");
        server.await.unwrap();
    }

    #[test]
    fn identity_ack_is_required_when_sid1_was_forwarded() {
        assert!(identity_ack_is_authenticated(
            true,
            &serde_json::json!({"identity_authenticated":"SID1"})
        ));
        assert!(!identity_ack_is_authenticated(
            true,
            &serde_json::json!({"identity_authenticated":"none"})
        ));
        assert!(!identity_ack_is_authenticated(true, &serde_json::json!({})));
        assert!(identity_ack_is_authenticated(false, &serde_json::json!({})));
    }

    #[test]
    fn sid1_header_value_is_sensitive_safe_metadata() {
        let header = sid1_header_value("AQID-_abc").unwrap();
        assert_eq!(header.to_str().unwrap(), "AQID-_abc");
        assert!(header.is_sensitive());
        assert!(sid1_header_value("\n").is_err());
    }

    #[tokio::test]
    async fn streams_only_visible_chunks_and_checks_handshake() {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let socket = accept_hdr_async(stream, |request: &Request, mut response: Response| {
                assert_eq!(request.uri().path(), "/ws/chat");
                assert!(
                    request
                        .uri()
                        .query()
                        .unwrap()
                        .contains("agent_alias=fixture-agent")
                );
                assert_eq!(
                    request.headers().get("authorization").unwrap(),
                    "Bearer fixture-token"
                );
                assert_eq!(
                    request.headers().get("sec-websocket-protocol").unwrap(),
                    WS_PROTOCOL
                );
                response
                    .headers_mut()
                    .insert("Sec-WebSocket-Protocol", WS_PROTOCOL.parse().unwrap());
                Ok(response)
            })
            .await
            .unwrap();
            let (mut sink, mut stream) = socket.split();
            assert!(matches!(
                stream.next().await.unwrap().unwrap(),
                Message::Text(_)
            ));
            sink.send(Message::Text(
                r#"{"type":"session_start","session_id":"native-fixture-session","resumed":false,"message_count":0,"stateless_generation":1}"#
                    .into(),
            ))
            .await
            .unwrap();
            sink.send(Message::Text(r#"{"type":"connected","stateless_generation":1,"provider":"fixture-provider","model":"fixture-model"}"#.into()))
                .await
                .unwrap();
            assert!(matches!(
                stream.next().await.unwrap().unwrap(),
                Message::Text(_)
            ));
            sink.send(Message::Text(
                r#"{"type":"thinking","content":"secret"}"#.into(),
            ))
            .await
            .unwrap();
            sink.send(Message::Text(r#"{"type":"chunk","content":"hel"}"#.into()))
                .await
                .unwrap();
            sink.send(Message::Text(r#"{"type":"chunk","content":"lo"}"#.into()))
                .await
                .unwrap();
            sink.send(Message::Text(r#"{"type":"done","full_response":"hello","provider":"fixture-provider","model":"fixture-model","input_tokens":2,"output_tokens":3,"tokens_used":5}"#.into())).await.unwrap();
        });
        let mut task = NativeGenerationTask::spawn(request(format!("http://{address}"))).unwrap();
        let mut text = String::new();
        while let Some(NativeGenerationEvent::TextDelta { delta }) = task.recv_event().await {
            text.push_str(&delta);
        }
        let terminal = task.wait().await.unwrap();
        assert_eq!(text, "hello");
        assert!(matches!(terminal, NativeGenerationTerminal::Done(_)));
        server.await.unwrap();
    }

    #[tokio::test]
    async fn rejects_approval_without_exposing_payload() {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let socket = accept_hdr_async(stream, |_: &Request, mut response: Response| {
                response
                    .headers_mut()
                    .insert("Sec-WebSocket-Protocol", WS_PROTOCOL.parse().unwrap());
                Ok(response)
            })
            .await
            .unwrap();
            let (mut sink, mut stream) = socket.split();
            let _ = stream.next().await;
            sink.send(Message::Text(
                r#"{"type":"session_start","session_id":"native-fixture-session","resumed":false,"message_count":0,"stateless_generation":1}"#
                    .into(),
            ))
            .await
            .unwrap();
            sink.send(Message::Text(r#"{"type":"connected","stateless_generation":1,"provider":"fixture-provider","model":"fixture-model"}"#.into()))
                .await
                .unwrap();
            let _ = stream.next().await;
            sink.send(Message::Text(r#"{"type":"approval_request","request_id":"secret","tool":"secret","arguments_summary":"secret"}"#.into())).await.unwrap();
        });
        let terminal = NativeGenerationTask::spawn(request(format!("http://{address}")))
            .unwrap()
            .wait()
            .await;
        assert!(matches!(
            terminal,
            Err(NativeGenerationError::Terminal(
                NativeErrorCode::ApprovalRequired
            ))
        ));
    }

    #[tokio::test]
    async fn rejects_malformed_chunk_content() {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let socket = accept_hdr_async(stream, |_: &Request, mut response: Response| {
                response
                    .headers_mut()
                    .insert("Sec-WebSocket-Protocol", WS_PROTOCOL.parse().unwrap());
                Ok(response)
            })
            .await
            .unwrap();
            let (mut sink, mut stream) = socket.split();
            let _ = stream.next().await;
            sink.send(Message::Text(r#"{"type":"session_start","session_id":"native-fixture-session","resumed":false,"message_count":0,"stateless_generation":1}"#.into())).await.unwrap();
            sink.send(Message::Text(r#"{"type":"connected","stateless_generation":1,"provider":"fixture-provider","model":"fixture-model"}"#.into()))
                .await
                .unwrap();
            let _ = stream.next().await;
            sink.send(Message::Text(r#"{"type":"chunk","content":42}"#.into()))
                .await
                .unwrap();
        });
        let terminal = NativeGenerationTask::spawn(request(format!("http://{address}")))
            .unwrap()
            .wait()
            .await;
        assert!(matches!(
            terminal,
            Err(NativeGenerationError::Terminal(NativeErrorCode::Protocol))
        ));
    }

    #[tokio::test]
    async fn cancellation_while_waiting_is_cooperative() {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let _socket = accept_hdr_async(stream, |_: &Request, mut response: Response| {
                response
                    .headers_mut()
                    .insert("Sec-WebSocket-Protocol", WS_PROTOCOL.parse().unwrap());
                Ok(response)
            })
            .await
            .unwrap();
            tokio::time::sleep(Duration::from_secs(2)).await;
        });
        let mut task = NativeGenerationTask::spawn(request(format!("http://{address}"))).unwrap();
        tokio::time::sleep(Duration::from_millis(20)).await;
        task.cancel();
        assert!(matches!(
            task.wait().await,
            Err(NativeGenerationError::Cancelled)
        ));
    }
    #[tokio::test]
    async fn rejects_old_native_without_stateless_marker_before_prompt() {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let socket = accept_hdr_async(stream, |_: &Request, mut response: Response| {
                response
                    .headers_mut()
                    .insert("Sec-WebSocket-Protocol", WS_PROTOCOL.parse().unwrap());
                Ok(response)
            })
            .await
            .unwrap();
            let (mut sink, mut stream) = socket.split();
            let _ = stream.next().await;
            sink.send(Message::Text(r#"{"type":"session_start","session_id":"native-fixture-session","resumed":false,"message_count":0}"#.into())).await.unwrap();
            assert!(
                tokio::time::timeout(Duration::from_millis(200), stream.next())
                    .await
                    .map(|item| !matches!(item, Some(Ok(Message::Text(_)))))
                    .unwrap_or(false)
            );
        });
        let terminal = NativeGenerationTask::spawn(request(format!("http://{address}")))
            .unwrap()
            .wait()
            .await;
        assert!(matches!(
            terminal,
            Err(NativeGenerationError::Terminal(NativeErrorCode::Protocol))
        ));
        server.await.unwrap();
    }

    #[tokio::test]
    async fn rejects_wrong_stateless_selection_before_prompt() {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let socket = accept_hdr_async(stream, |_: &Request, mut response: Response| {
                response
                    .headers_mut()
                    .insert("Sec-WebSocket-Protocol", WS_PROTOCOL.parse().unwrap());
                Ok(response)
            })
            .await
            .unwrap();
            let (mut sink, mut stream) = socket.split();
            let _ = stream.next().await;
            sink.send(Message::Text(r#"{"type":"session_start","session_id":"native-fixture-session","resumed":false,"message_count":0,"stateless_generation":1}"#.into())).await.unwrap();
            sink.send(Message::Text(r#"{"type":"connected","stateless_generation":1,"provider":"wrong","model":"fixture-model"}"#.into())).await.unwrap();
            assert!(
                tokio::time::timeout(Duration::from_millis(200), stream.next())
                    .await
                    .map(|item| !matches!(item, Some(Ok(Message::Text(_)))))
                    .unwrap_or(false)
            );
        });
        let terminal = NativeGenerationTask::spawn(request(format!("http://{address}")))
            .unwrap()
            .wait()
            .await;
        assert!(matches!(
            terminal,
            Err(NativeGenerationError::Terminal(NativeErrorCode::Protocol))
        ));
        server.await.unwrap();
    }

    #[tokio::test]
    async fn rejects_oversized_raw_text_frame() {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let socket = accept_hdr_async(stream, |_: &Request, mut response: Response| {
                response
                    .headers_mut()
                    .insert("Sec-WebSocket-Protocol", WS_PROTOCOL.parse().unwrap());
                Ok(response)
            })
            .await
            .unwrap();
            let (mut sink, mut stream) = socket.split();
            let _ = stream.next().await;
            let _ = sink
                .send(Message::Text(format!(
                    "{{\"type\":\"chunk\",\"content\":\"{}\"}}",
                    "x".repeat(MAX_FRAME_BYTES)
                )))
                .await;
        });
        let terminal = NativeGenerationTask::spawn(request(format!("http://{address}")))
            .unwrap()
            .wait()
            .await;
        assert!(matches!(
            terminal,
            Err(NativeGenerationError::Transport)
                | Err(NativeGenerationError::Terminal(NativeErrorCode::Bounds))
        ));
        server.await.unwrap();
    }

    #[tokio::test]
    async fn cancellation_while_event_channel_is_saturated_finishes_cancelled() {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let socket = accept_hdr_async(stream, |_: &Request, mut response: Response| {
                response
                    .headers_mut()
                    .insert("Sec-WebSocket-Protocol", WS_PROTOCOL.parse().unwrap());
                Ok(response)
            })
            .await
            .unwrap();
            let (mut sink, mut stream) = socket.split();
            let _ = stream.next().await;
            sink.send(Message::Text(r#"{"type":"session_start","session_id":"native-fixture-session","resumed":false,"message_count":0,"stateless_generation":1}"#.into())).await.unwrap();
            sink.send(Message::Text(r#"{"type":"connected","stateless_generation":1,"provider":"fixture-provider","model":"fixture-model"}"#.into())).await.unwrap();
            let _ = stream.next().await;
            for _ in 0..66 {
                sink.send(Message::Text(r#"{"type":"chunk","content":"x"}"#.into()))
                    .await
                    .unwrap();
            }
            let _ = tokio::time::timeout(Duration::from_secs(2), stream.next()).await;
        });
        let mut task = NativeGenerationTask::spawn(request(format!("http://{address}"))).unwrap();
        tokio::time::timeout(Duration::from_secs(2), async {
            while task.events.len() < 64 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        task.cancel();
        assert!(matches!(
            tokio::time::timeout(Duration::from_secs(2), task.wait()).await,
            Ok(Err(NativeGenerationError::Cancelled))
        ));
        server.await.unwrap();
    }

    #[tokio::test]
    async fn bounded_escaped_terminal_matches_committed_deltas() {
        for mismatch in [false, true] {
            let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
                .await
                .unwrap();
            let address = listener.local_addr().unwrap();
            let server = tokio::spawn(async move {
                let (stream, _) = listener.accept().await.unwrap();
                let mut socket = accept_hdr_async(stream, |_: &Request, mut response: Response| {
                    response
                        .headers_mut()
                        .insert("Sec-WebSocket-Protocol", WS_PROTOCOL.parse().unwrap());
                    Ok(response)
                })
                .await
                .unwrap();
                let _ = socket.next().await;
                for frame in [
                    serde_json::json!({"type":"session_start","session_id":"native-fixture-session","resumed":false,"message_count":0,"stateless_generation":1}),
                    serde_json::json!({"type":"connected","stateless_generation":1,"provider":"fixture-provider","model":"fixture-model"}),
                ] {
                    socket.send(Message::Text(frame.to_string())).await.unwrap();
                }
                let _ = socket.next().await;
                let delta = "\u{0000}".repeat(MAX_DELTA_BYTES);
                for _ in 0..(MAX_OUTPUT_BYTES / MAX_DELTA_BYTES) {
                    socket
                        .send(Message::Text(
                            serde_json::json!({"type":"chunk","content":delta}).to_string(),
                        ))
                        .await
                        .unwrap();
                }
                let response = if mismatch {
                    "different".into()
                } else {
                    "\u{0000}".repeat(MAX_OUTPUT_BYTES)
                };
                let done = serde_json::json!({"type":"done","full_response":response,"provider":"fixture-provider","model":"fixture-model"}).to_string();
                if !mismatch {
                    assert!(done.len() > MAX_OUTPUT_BYTES && done.len() < MAX_FRAME_BYTES);
                }
                socket.send(Message::Text(done)).await.unwrap();
            });
            let mut task =
                NativeGenerationTask::spawn(request(format!("http://{address}"))).unwrap();
            tokio::time::timeout(Duration::from_secs(5), async {
                let mut output = String::new();
                while let Some(NativeGenerationEvent::TextDelta { delta }) = task.recv_event().await {
                    output.push_str(&delta);
                }
                assert_eq!(output.len(), MAX_OUTPUT_BYTES);
                let result = task.wait().await;
                if mismatch {
                    assert!(matches!(result, Err(NativeGenerationError::Terminal(NativeErrorCode::Protocol))));
                } else {
                    assert!(matches!(result, Ok(NativeGenerationTerminal::Done(done)) if done.full_response == output));
                }
            }).await.unwrap();
            server.await.unwrap();
        }
    }
}
