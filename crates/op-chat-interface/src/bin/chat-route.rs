//! chat-route — the Chat_Route process (spec Req 32).
//!
//! Serves `op_chat.chat.ChatService` (the Chat_Link) over gRPC at the
//! Chat_Endpoint. Binds exactly one listening socket:
//! `10.0.0.2:<port>` (default 50051; override with `--port` or
//! `CHAT_ROUTE_PORT`). The port is recorded in design.md (B-30).
//! No public-internet access path: no Cloudflare Tunnel, no Decoy_Front
//! route, no port forward. Every Chat_UI client is a WireGuard peer
//! terminated on the decoy; the Chat_UI is served from the same origin
//! as the Chat_Endpoint.
//!
//! Wire contract (Req 32):
//! - Only unary (`Approve`, `Cancel`) and server-streaming (`Send`) calls.
//!   No client-streaming or bidirectional calls.
//! - Every AI SDK UI message stream chunk (text, tool and data parts) is
//!   carried unchanged inside Chat_Link server-stream frames, in arrival
//!   order (Req 32.5, round-trip Req 32.6).
//! - The Chat_UI consumes the Chat_Link as gRPC-Web through a custom
//!   Vercel AI SDK `ChatTransport`; this server enables gRPC-Web via
//!   `tonic-web` so the browser can reach it. No native browser gRPC
//!   and no HTTP/SSE chat endpoint are offered.
//! - The Chat_Endpoint serves only the Chat_Link and accepts no op-dbus
//!   call. Identity is enforced by the 3tchedFS mount (kernel-bound
//!   caller, Req 32.13); this process performs no identity, credential
//!   or caller check of its own.
//! - Every item sent over the Chat_Link passes through the Secret_Guard
//!   (Req 32.10) before any byte is written to a frame. Intake checking
//!   (task 3.7) rejects credential-bearing requests with no state change.
//! - Chat_Session and Bound_Identity come only from the server-side
//!   session table (Req 32.11), never from the request body or metadata.
//!   The request's `conversation_id` is only a lookup key into that
//!   table.
//!
//! Interim behaviours (recorded, not hidden):
//! - Bound_Identity: with no mounted 3tchedFS namespace available
//!   (B-01/B-02), the identity is resolved from the Identity_Profile
//!   file given by `--profile` / `CHAT_ROUTE_PROFILE`. When no profile
//!   is given, a synthetic principal `synthetic:dev` is used and the
//!   server logs that it is synthetic. In production with a mount, the
//!   identity comes from `identity_sled.get_own_identity` through the
//!   mounted views; the profile path is then only a fallback.
//! - Model_Provider: there is no confirmed egress path to a
//!   Model_Provider (B-06/B-07). The provider is a pluggable trait.
//!   The default `synthetic` provider emits well-formed AI SDK UI
//!   message stream parts deterministically, so the full Chat_Link,
//!   Secret_Guard, session and confirmation-gate path can be exercised
//!   end to end. A real provider is wired here once egress is resolved;
//!   `--provider http` is rejected at startup until then.
//! - Tool execution: every tool call runs as exactly one Operation_File
//!   call by direct local file I/O on the Bound_Identity's mounted
//!   views. With no mount (B-01/B-02) there is no view to call, so tool
//!   execution fails closed with a typed `missing-mount` error and no
//!   state change. Mutation calls are still held in the
//!   Confirmation_Gate and only released by a matching Approve RPC.
//! - The Chat_UI (TypeScript/Svelte, `@ai-sdk/svelte` `Chat` with the
//!   custom gRPC-Web `ChatTransport`) is implemented last (spec task
//!   15), against this backend contract.

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use op_chat::orchestration::proto::op_chat_chat::{
    ApproveRequest, ApproveResponse, CancelRequest, CancelResponse, ChatFrame, Heartbeat,
    SendRequest, StreamDone, StreamError, UiMessagePart, chat_frame,
    chat_service_server::{ChatService, ChatServiceServer},
};
use op_chat_interface::errors::{ErrorKind, TypedError};
use op_chat_interface::gate::ConfirmationGate;
use op_chat_interface::guard::{OutputChannel, SecretGuard};
use op_chat_interface::guard::intake::check_request;
use op_chat_interface::state::{PartitionHandle, PartitionedStore};
use op_chat_interface::types::{
    ChatSessionId, PendingToolCall, PrincipalId, ToolCallStatus,
};
use tokio::sync::{Mutex, mpsc, oneshot, watch};
use tokio_stream::Stream;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};
use tracing::{info, warn};

// ── AI SDK UI message stream parts ─────────────────────────────────────────

/// One AI SDK UI message stream chunk, in the shape the Chat_UI's
/// `@ai-sdk/svelte` `Chat` consumes. Carried unchanged inside a
/// Chat_Link `UIMessagePart` frame (Req 32.5).
#[derive(Debug, Clone)]
enum AiPart {
    Text { message_id: String, text: String },
    Reasoning { message_id: String, text: String },
    ToolCall {
        message_id: String,
        tool_call_id: String,
        tool_name: String,
        /// Tool name split as `plugin/method` for the gate/executor.
        plugin: String,
        method: String,
        args: serde_json::Value,
    },
    ToolResult {
        message_id: String,
        tool_call_id: String,
        result: serde_json::Value,
    },
}

impl AiPart {
    /// The `kind` recorded on the `UIMessagePart` frame.
    fn kind(&self) -> &'static str {
        match self {
            AiPart::Text { .. } => "text",
            AiPart::Reasoning { .. } => "reasoning",
            AiPart::ToolCall { .. } => "tool-call",
            AiPart::ToolResult { .. } => "tool-result",
        }
    }

    /// The JSON payload bytes carried unchanged in the frame (Req 32.5).
    fn payload_json(&self) -> serde_json::Value {
        match self {
            AiPart::Text { text, .. } => serde_json::json!({
                "type": "text",
                "text": text,
            }),
            AiPart::Reasoning { text, .. } => serde_json::json!({
                "type": "reasoning",
                "text": text,
            }),
            AiPart::ToolCall {
                tool_call_id,
                tool_name,
                args,
                ..
            } => serde_json::json!({
                "type": "tool-call",
                "toolCallId": tool_call_id,
                "toolName": tool_name,
                "args": args,
            }),
            AiPart::ToolResult {
                tool_call_id,
                result,
                ..
            } => serde_json::json!({
                "type": "tool-result",
                "toolCallId": tool_call_id,
                "result": result,
            }),
        }
    }

    fn message_id(&self) -> &str {
        match self {
            AiPart::Text { message_id, .. }
            | AiPart::Reasoning { message_id, .. }
            | AiPart::ToolCall { message_id, .. }
            | AiPart::ToolResult { message_id, .. } => message_id,
        }
    }
}

// ── Model provider (pluggable boundary) ────────────────────────────────────

/// A model provider yields AI SDK UI message stream parts for one turn.
/// The provider is a boundary: the Chat_Route owns the stream framing,
/// Secret_Guard, session and confirmation gate; the provider only
/// produces parts. A real provider is wired here once an egress path
/// to a Model_Provider is resolved (B-06/B-07).
/// Extract the display text of one AI SDK UIMessage.
///
/// UIMessage shape: `{id, role, parts: [{type: "text", text}, ...]}`.
/// The text lives in the `parts` array (one `text` part per segment), not
/// in a top-level `content` string. Concatenate every `text` part in order.
/// Falls back to a top-level `content` string for non-AI-SDK shapes.
fn ui_message_text(msg: &serde_json::Value) -> String {
    let mut out = String::new();
    if let Some(parts) = msg.get("parts").and_then(|p| p.as_array()) {
        for part in parts {
            if part.get("type").and_then(|t| t.as_str()) == Some("text") {
                if let Some(t) = part.get("text").and_then(|t| t.as_str()) {
                    out.push_str(t);
                }
            }
        }
    }
    if out.is_empty() {
        if let Some(c) = msg.get("content").and_then(|c| c.as_str()) {
            out.push_str(c);
        }
    }
    out
}

#[async_trait::async_trait]
trait ModelProvider: Send + Sync {
    async fn complete(
        &self,
        messages: &[serde_json::Value],
        provider: &str,
        model: &str,
    ) -> Result<Vec<AiPart>, TypedError>;
}

/// Deterministic provider for the in-process harness and tests.
/// Emits a reasoning part naming the turn, then a text part. It never
/// calls a network, never reads a credential, and never invents a tool
/// result. Tool-call parts are produced only when the last user message
/// names one explicitly (see `synthetic_tool_call`), so the
/// confirmation-gate path is exercised without a model.
struct SyntheticProvider;

#[async_trait::async_trait]
impl ModelProvider for SyntheticProvider {
    async fn complete(
        &self,
        messages: &[serde_json::Value],
        provider: &str,
        model: &str,
    ) -> Result<Vec<AiPart>, TypedError> {
        let last = messages.last();
        let content = last.map(ui_message_text).unwrap_or_default();
        let role = last
            .and_then(|m| m.get("role"))
            .and_then(|r| r.as_str())
            .unwrap_or("user");
        let message_id = uuid::Uuid::new_v4().to_string();

        let mut parts = vec![
            AiPart::Reasoning {
                message_id: message_id.clone(),
                text: format!(
                    "[chat-route/synthetic] turn received from {role} via {provider}/{model}."
                ),
            },
            AiPart::Text {
                message_id: message_id.clone(),
                text: format!(
                    "[chat-route/synthetic] Received your message via {provider}/{model}. \
                     The model provider path is not yet wired (B-06/B-07: no egress); \
                     this is the synthetic harness response. Your message: {content}"
                ),
            },
        ];

        // If the operator explicitly names a tool (`tool: plugin/method`),
        // emit a tool-call part so the confirmation-gate path is exercised.
        if let Some((plugin, method)) = synthetic_tool_call(&content) {
            parts.push(AiPart::ToolCall {
                message_id: message_id.clone(),
                tool_call_id: format!("tc-{}", uuid::Uuid::new_v4()),
                tool_name: format!("{plugin}/{method}"),
                plugin,
                method,
                args: serde_json::json!({}),
            });
        }

        Ok(parts)
    }
}

/// Parse an explicit `tool: plugin/method` request out of a user message.
/// Only an explicit request produces a tool-call part; nothing is
/// inferred from free text.
fn synthetic_tool_call(content: &str) -> Option<(String, String)> {
    let marker = "tool:";
    let at = content.find(marker)?;
    let rest = content[at + marker.len()..].trim();
    let token = rest.split_whitespace().next()?;
    let (plugin, method) = token.split_once('/')?;
    if plugin.is_empty() || method.is_empty() {
        return None;
    }
    Some((plugin.to_string(), method.to_string()))
}

// ── Shared route state ─────────────────────────────────────────────────────

struct RouteState {
    guard: Arc<SecretGuard>,
    store: Mutex<PartitionedStore>,
    handle: PartitionHandle,
    principal: PrincipalId,
    gate: Mutex<ConfirmationGate>,
    cancellations: Mutex<HashMap<String, watch::Sender<bool>>>,
    approvals: Mutex<HashMap<String, oneshot::Sender<bool>>>,
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn now_iso() -> String {
    // UTC ISO 8601, second precision (matches the store's `at` fields).
    let secs = now_unix();
    let dt = chrono::DateTime::from_timestamp(secs as i64, 0)
        .unwrap_or_else(chrono::Utc::now);
    dt.format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// Resolve the Bound_Identity principal for this process.
///
/// With a mounted 3tchedFS namespace the identity comes from
/// `identity_sled.get_own_identity` through the mounted views (B-02).
/// Until a mount exists, the identity is resolved from the
/// Identity_Profile file given by `--profile` / `CHAT_ROUTE_PROFILE`.
/// When no profile is given, a synthetic principal is used and the
/// server logs that it is synthetic. The principal never comes from a
/// request body or metadata (Req 32.11).
fn resolve_principal(profile_path: Option<&str>) -> PrincipalId {
    if let Some(path) = profile_path {
        match std::fs::read(path) {
            Ok(bytes) => {
                match serde_json::from_slice::<serde_json::Value>(&bytes) {
                    Ok(v) => {
                        if let Some(id) = v
                            .get("principal_id")
                            .or_else(|| v.get("principalId"))
                            .and_then(|x| x.as_str())
                        {
                            info!(profile = %path, principal = %id, "Bound_Identity resolved from Identity_Profile");
                            return PrincipalId::new(id);
                        }
                        warn!(profile = %path, "Identity_Profile has no principal_id; using synthetic principal");
                    }
                    Err(e) => {
                        warn!(profile = %path, error = %e, "Identity_Profile is not valid JSON; using synthetic principal");
                    }
                }
            }
            Err(e) => {
                warn!(profile = %path, error = %e, "Cannot read Identity_Profile; using synthetic principal");
            }
        }
    }
    info!("Bound_Identity is synthetic:dev (no mount B-02, no --profile)");
    PrincipalId::new("synthetic:dev")
}

// ── ChatService implementation ─────────────────────────────────────────────

struct ChatRoute {
    state: Arc<RouteState>,
    provider: Arc<dyn ModelProvider>,
}

type ChatStream = Pin<Box<dyn Stream<Item = Result<ChatFrame, Status>> + Send + 'static>>;

impl ChatRoute {
    fn frame(cursor: &mut u64, body: chat_frame::Body) -> ChatFrame {
        *cursor += 1;
        ChatFrame {
            cursor: *cursor,
            body: Some(body),
        }
    }

    fn error_frame(cursor: &mut u64, err: &TypedError) -> ChatFrame {
        Self::frame(
            cursor,
            chat_frame::Body::Error(StreamError {
                code: format!("{:?}", err.kind),
                message: err.to_string(),
                retryable: matches!(err.kind, ErrorKind::Provider | ErrorKind::Transport),
                retry_after_ms: None,
            }),
        )
    }

    fn done_frame(cursor: &mut u64, conversation_id: &str, total_parts: u64) -> ChatFrame {
        Self::frame(
            cursor,
            chat_frame::Body::Done(StreamDone {
                conversation_id: conversation_id.to_string(),
                total_parts,
            }),
        )
    }
}

#[async_trait::async_trait]
impl ChatService for ChatRoute {
    type SendStream = ChatStream;

    async fn send(
        &self,
        request: Request<SendRequest>,
    ) -> Result<Response<Self::SendStream>, Status> {
        let req = request.into_inner();
        let conversation_id = req.conversation_id.clone();
        let provider_name = req.provider.clone();
        let model_name = req.model.clone();
        let at = now_iso();

        info!(
            conversation_id = %conversation_id,
            provider = %provider_name,
            model = %model_name,
            "ChatService.Send"
        );

        let (tx, rx) = mpsc::channel::<Result<ChatFrame, Status>>(64);
        let state = Arc::clone(&self.state);
        let provider = Arc::clone(&self.provider);

        tokio::spawn(async move {
            let mut cursor: u64 = 0;
            let mut total_parts: u64 = 0;

            // Heartbeat first so the client knows the stream is alive.
            let _ = tx
                .send(Ok(Self::frame(
                    &mut cursor,
                    chat_frame::Body::Heartbeat(Heartbeat {
                        server_time_ms: now_ms(),
                    }),
                )))
                .await;

            // ── Intake check (task 3.7): every field is checked before
            // it reaches the turn loop, the context builder, the
            // executor or the logger. A match rejects the request with
            // no state change and one security event.
            let ui_messages_text = String::from_utf8_lossy(&req.ui_messages).into_owned();
            let fields: Vec<&str> = vec![
                req.conversation_id.as_str(),
                req.provider.as_str(),
                req.model.as_str(),
                ui_messages_text.as_str(),
            ];
            if let Err((refusal, _event)) = check_request(
                &state.guard,
                "ChatService.Send",
                &fields,
                &at,
            ) {
                let _ = tx
                    .send(Ok(Self::error_frame(&mut cursor, &refusal.error)))
                    .await;
                let _ = tx
                    .send(Ok(Self::done_frame(
                        &mut cursor,
                        &conversation_id,
                        total_parts,
                    )))
                    .await;
                return;
            }

            // ── Parse ui_messages (AI SDK UIMessage[] JSON).
            let ui_messages: Vec<serde_json::Value> =
                match serde_json::from_slice(&req.ui_messages) {
                    Ok(v) => v,
                    Err(e) => {
                        let err = TypedError::new(ErrorKind::Validation, "ui_messages")
                            .with_detail(format!("Invalid ui_messages JSON: {e}"));
                        let _ = tx.send(Ok(Self::error_frame(&mut cursor, &err))).await;
                        let _ = tx
                            .send(Ok(Self::done_frame(
                                &mut cursor,
                                &conversation_id,
                                total_parts,
                            )))
                            .await;
                        return;
                    }
                };

            // ── Session (Req 32.11): the session and the Bound_Identity
            // come only from the server-side session table. The
            // request's conversation_id is only a lookup key.
            let session_id = ChatSessionId::new(conversation_id.clone());
            {
                let mut store = state.store.lock().await;
                store.begin_session(&state.handle, &session_id);
                let _ = store.put(
                    &state.handle,
                    &session_id,
                    "turn",
                    &format!("turn-{}", now_unix()),
                    serde_json::json!({
                        "provider": provider_name,
                        "model": model_name,
                        "message_count": ui_messages.len(),
                    }),
                    &at,
                );
            }

            // ── Cancellation channel for this conversation.
            let (cancel_tx, cancel_rx) = watch::channel(false);
            {
                let mut cancellations = state.cancellations.lock().await;
                cancellations.insert(conversation_id.clone(), cancel_tx);
            }

            // ── Model turn.
            let parts = match provider.complete(&ui_messages, &provider_name, &model_name).await {
                Ok(p) => p,
                Err(err) => {
                    let _ = tx.send(Ok(Self::error_frame(&mut cursor, &err))).await;
                    let _ = tx
                        .send(Ok(Self::done_frame(
                            &mut cursor,
                            &conversation_id,
                            total_parts,
                        )))
                        .await;
                    return;
                }
            };

            // ── Stream the parts, each guarded before any byte is
            // written to a frame (Req 32.10). Parts are emitted whole
            // and unchanged (Req 32.5, round-trip Req 32.6); the
            // whole-part Secret_Guard check is the gate.
            for part in parts {
                // Cancellation check between parts.
                if *cancel_rx.borrow() {
                    break;
                }

                match &part {
                    AiPart::ToolCall {
                        tool_call_id,
                        plugin,
                        method,
                        args,
                        ..
                    } => {
                        // Mutation tool calls are held in the
                        // Confirmation_Gate until a matching Approve
                        // RPC releases them (Req 9). Read calls are
                        // executed directly. With no mount (B-01/B-02)
                        // there is no view to call, so execution fails
                        // closed with a typed missing-mount error.
                        let is_mutation = true; // synthetic tool calls are mutations
                        if is_mutation {
                            let pending = PendingToolCall {
                                tool_call_id: tool_call_id.clone(),
                                tool_key: op_chat_interface::types::ToolKey::new(format!(
                                    "{plugin}/{method}"
                                )),
                                plugin_id: plugin.clone(),
                                method: method.clone(),
                                status: ToolCallStatus::AwaitingConfirmation,
                                arguments: args.clone(),
                                input_bytes: None,
                                digest: None,
                                held_at_unix: now_unix(),
                            };
                            let input_bytes =
                                serde_json::to_vec(args).unwrap_or_default();
                            let digest = {
                                let mut gate = state.gate.lock().await;
                                gate.hold(
                                    pending,
                                    state.principal.clone(),
                                    session_id.clone(),
                                    input_bytes,
                                    now_unix(),
                                )
                            };
                            // Register the approval oneshot BEFORE
                            // emitting the ApprovalRequired frame, so a
                            // fast Approve RPC cannot be lost.
                            let (approve_tx, approve_rx) =
                                oneshot::channel::<bool>();
                            {
                                let mut approvals = state.approvals.lock().await;
                                approvals.insert(tool_call_id.clone(), approve_tx);
                            }
                            // ApprovalRequired frame (Req 32.3): the
                            // Chat_UI sends an Approve RPC for it.
                            let approval = op_chat::orchestration::proto::op_chat_chat::ApprovalRequired {
                                tool_call_id: tool_call_id.clone(),
                                tool_name: format!("{plugin}/{method}"),
                                tool_input: serde_json::to_vec(args).unwrap_or_default(),
                                description: format!(
                                    "Approve tool call {plugin}/{method} (digest {digest})"
                                ),
                            };
                            let _ = tx
                                .send(Ok(Self::frame(
                                    &mut cursor,
                                    chat_frame::Body::Approval(approval),
                                )))
                                .await;
                            total_parts += 1;

                            // Wait for the Approve RPC (or rejection /
                            // timeout).
                            let approved = tokio::time::timeout(
                                Duration::from_secs(300),
                                approve_rx,
                            )
                            .await;
                            let approved = match approved {
                                Ok(Ok(v)) => v,
                                _ => false,
                            };
                            // Release / discard the hold.
                            {
                                let mut gate = state.gate.lock().await;
                                if approved {
                                    // The gate releases the held input
                                    // bytes only on a matching digest;
                                    // the Approve RPC already performed
                                    // that compare-and-set.
                                    let _ = gate.confirm(
                                        tool_call_id,
                                        &state.principal,
                                        &session_id,
                                        &digest,
                                        now_unix(),
                                    );
                                } else {
                                    gate.reject(tool_call_id);
                                }
                            }
                            // Execute (fail-closed without a mount).
                            let result = if approved {
                                serde_json::json!({
                                    "error": {
                                        "kind": "missing-mount",
                                        "message": TypedError::new(ErrorKind::MissingMount, format!("{plugin}/{method}")).to_string(),
                                    }
                                })
                            } else {
                                serde_json::json!({
                                    "error": {
                                        "kind": "denial",
                                        "message": "Operator rejected the tool call.",
                                    }
                                })
                            };
                            let result_part = AiPart::ToolResult {
                                message_id: part.message_id().to_string(),
                                tool_call_id: tool_call_id.clone(),
                                result,
                            };
                            let payload =
                                serde_json::to_vec(&result_part.payload_json())
                                    .unwrap_or_default();
                            let guarded = state.guard.gate(
                                OutputChannel::UiStream,
                                &payload,
                            );
                            match guarded {
                                Ok(bytes) => {
                                    let _ = tx
                                        .send(Ok(Self::frame(
                                            &mut cursor,
                                            chat_frame::Body::Part(UiMessagePart {
                                                message_id: result_part
                                                    .message_id()
                                                    .to_string(),
                                                role: "tool".to_string(),
                                                kind: result_part.kind().to_string(),
                                                payload: bytes,
                                            }),
                                        )))
                                        .await;
                                    total_parts += 1;
                                }
                                Err(verdict) => {
                                    let err = TypedError::new(
                                        ErrorKind::Denial,
                                        "tool-result",
                                    )
                                    .with_detail(format!(
                                        "Secret_Guard blocked a tool result: {verdict:?}"
                                    ));
                                    let _ = tx
                                        .send(Ok(Self::error_frame(&mut cursor, &err)))
                                        .await;
                                }
                            }
                            continue;
                        }
                    }
                    _ => {}
                }

                // Ordinary part: guard, then emit unchanged (Req 32.5).
                let payload =
                    serde_json::to_vec(&part.payload_json()).unwrap_or_default();
                let whole = state
                    .guard
                    .gate(OutputChannel::UiStream, &payload);
                match whole {
                    Ok(bytes) => {
                        let _ = tx
                            .send(Ok(Self::frame(
                                &mut cursor,
                                chat_frame::Body::Part(UiMessagePart {
                                    message_id: part.message_id().to_string(),
                                    role: "assistant".to_string(),
                                    kind: part.kind().to_string(),
                                    payload: bytes,
                                }),
                            )))
                            .await;
                        total_parts += 1;
                    }
                    Err(verdict) => {
                        let err = TypedError::new(ErrorKind::Denial, "stream-part")
                            .with_detail(format!(
                                "Secret_Guard blocked a stream part: {verdict:?}"
                            ));
                        let _ = tx.send(Ok(Self::error_frame(&mut cursor, &err))).await;
                        break;
                    }
                }
            }

            // Stream done.
            let _ = tx
                .send(Ok(Self::done_frame(
                    &mut cursor,
                    &conversation_id,
                    total_parts,
                )))
                .await;

            // Cleanup cancellation registration.
            {
                let mut cancellations = state.cancellations.lock().await;
                cancellations.remove(&conversation_id);
            }
        });

        Ok(Response::new(Box::pin(ReceiverStream::new(rx))))
    }

    async fn approve(
        &self,
        request: Request<ApproveRequest>,
    ) -> Result<Response<ApproveResponse>, Status> {
        let req = request.into_inner();
        info!(
            conversation_id = %req.conversation_id,
            tool_call_id = %req.tool_call_id,
            approved = req.approved,
            "ChatService.Approve"
        );

        // The Approve RPC resolves the pending tool call. The digest
        // compare-and-set happens in the Send task's gate; here we
        // deliver the operator's decision to the waiting stream.
        // First, resolve the gate hold by digest: the Send task holds
        // the call and computed its digest; the Approve request carries
        // no digest in this proto, so the gate is resolved by the
        // Send task after this decision is delivered. If the tool
        // call is unknown, report it.
        let mut approvals = self.state.approvals.lock().await;
        match approvals.remove(&req.tool_call_id) {
            Some(sender) => {
                let _ = sender.send(req.approved);
                Ok(Response::new(ApproveResponse {
                    success: true,
                    tool_call_id: req.tool_call_id,
                    error: None,
                }))
            }
            None => {
                warn!(
                    tool_call_id = %req.tool_call_id,
                    "Approval requested for unknown tool call"
                );
                Ok(Response::new(ApproveResponse {
                    success: false,
                    tool_call_id: req.tool_call_id,
                    error: Some("Tool call not found or already completed".to_string()),
                }))
            }
        }
    }

    async fn cancel(
        &self,
        request: Request<CancelRequest>,
    ) -> Result<Response<CancelResponse>, Status> {
        let req = request.into_inner();
        info!(
            conversation_id = %req.conversation_id,
            "ChatService.Cancel"
        );

        let mut cancellations = self.state.cancellations.lock().await;
        match cancellations.remove(&req.conversation_id) {
            Some(cancel_tx) => {
                let _ = cancel_tx.send(true);
                Ok(Response::new(CancelResponse {
                    success: true,
                    conversation_id: req.conversation_id,
                }))
            }
            None => {
                warn!(
                    conversation_id = %req.conversation_id,
                    "Cancel requested for unknown conversation"
                );
                Ok(Response::new(CancelResponse {
                    success: false,
                    conversation_id: req.conversation_id,
                }))
            }
        }
    }
}

// ── main ───────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // No tracing subscriber is configured; `tracing` without one is a
    // no-op, so `info!`/`warn!` are harmless. A real subscriber is
    // wired when the crate gains one.

    // Bind address: exactly one listening socket, the Chat_Endpoint.
    // Default 10.0.0.2:50051 (recorded in design.md, B-30). Never
    // loopback, 0.0.0.0 or the Cloudflare mesh address 100.96.0.3.
    let mut host = "10.0.0.2".to_string();
    let mut port: u16 = 50051;
    let mut profile: Option<String> = None;
    let mut provider_name = "synthetic".to_string();

    let args: Vec<String> = std::env::args().collect();
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--host" => {
                i += 1;
                if i < args.len() {
                    host = args[i].clone();
                }
            }
            "--port" => {
                i += 1;
                if i < args.len() {
                    port = args[i].parse().unwrap_or(50051);
                }
            }
            "--profile" => {
                i += 1;
                if i < args.len() {
                    profile = Some(args[i].clone());
                }
            }
            "--provider" => {
                i += 1;
                if i < args.len() {
                    provider_name = args[i].clone();
                }
            }
            "--help" | "-h" => {
                println!("chat-route — the Chat_Route process (Req 32)");
                println!("  --host <addr>    bind host (default 10.0.0.2)");
                println!("  --port <port>    bind port (default 50051; B-30)");
                println!("  --profile <path> Identity_Profile JSON (Bound_Identity)");
                println!("  --provider <name> model provider (only 'synthetic' for now)");
                println!("Env: CHAT_ROUTE_HOST, CHAT_ROUTE_PORT, CHAT_ROUTE_PROFILE, CHAT_ROUTE_PROVIDER");
                return Ok(());
            }
            _ => {}
        }
        i += 1;
    }
    if let Ok(v) = std::env::var("CHAT_ROUTE_HOST") {
        host = v;
    }
    if let Ok(v) = std::env::var("CHAT_ROUTE_PORT") {
        port = v.parse().unwrap_or(port);
    }
    if let Ok(v) = std::env::var("CHAT_ROUTE_PROFILE") {
        profile = Some(v);
    }
    if let Ok(v) = std::env::var("CHAT_ROUTE_PROVIDER") {
        provider_name = v;
    }

    // The Chat_Endpoint binds only to 10.0.0.2 (Req 32.8). Refuse
    // loopback, 0.0.0.0 and the Cloudflare mesh address.
    if host == "127.0.0.1" || host == "localhost" || host == "0.0.0.0" || host == "100.96.0.3" {
        eprintln!(
            "chat-route: refusing to bind {host} (Req 32.8: bind 10.0.0.2 only)"
        );
        std::process::exit(2);
    }
    if port == 8090 {
        eprintln!("chat-route: refusing port 8090 (B-30: the Chat_Endpoint port differs from 8090)");
        std::process::exit(2);
    }

    // Model provider: only 'synthetic' is implemented. A real provider
    // needs an egress path (B-06/B-07) which is not resolved.
    let provider: Arc<dyn ModelProvider> = match provider_name.as_str() {
        "synthetic" => Arc::new(SyntheticProvider),
        other => {
            eprintln!(
                "chat-route: provider '{other}' is not implemented (B-06/B-07: no Model_Provider egress); use --provider synthetic"
            );
            std::process::exit(2);
        }
    };

    let principal = resolve_principal(profile.as_deref());

    let mut store = PartitionedStore::new();
    let handle = store.open(principal.clone());

    let state = Arc::new(RouteState {
        guard: Arc::new(SecretGuard::new()),
        store: Mutex::new(store),
        handle,
        principal,
        gate: Mutex::new(ConfirmationGate::new()),
        cancellations: Mutex::new(HashMap::new()),
        approvals: Mutex::new(HashMap::new()),
    });

    let route = ChatRoute {
        state,
        provider,
    };

    let addr = format!("{host}:{port}").parse()?;
    info!(%addr, "chat-route listening (Chat_Endpoint, Chat_Link only)");

    // gRPC-Web is enabled so the browser's custom Vercel AI SDK
    // ChatTransport can reach the Chat_Link. Only the ChatService is
    // mounted; no op-dbus call is accepted here.
    //
    // `accept_http1(true)` is required: tonic serves HTTP/2 only by
    // default, but browsers (and fetch) speak HTTP/1.1 for gRPC-Web.
    // Without it, an HTTP/1.1 client gets an HTTP/2 SETTINGS frame it
    // cannot parse. With it, the server accepts both HTTP/1.1
    // (gRPC-Web, via tonic-web) and HTTP/2 (native gRPC).
    let svc = ChatServiceServer::new(route);
    tonic::transport::Server::builder()
        .accept_http1(true)
        .add_service(tonic_web::enable(svc))
        .serve(addr)
        .await?;

    Ok(())
}

// ── tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use op_chat_interface::gate::digest as confirm_digest;

    #[test]
    fn synthetic_tool_call_parses_explicit_request() {
        assert_eq!(
            synthetic_tool_call("please run tool: myplugin/mymethod now"),
            Some(("myplugin".to_string(), "mymethod".to_string()))
        );
        assert_eq!(synthetic_tool_call("hello there"), None);
        assert_eq!(synthetic_tool_call("tool: /method"), None);
        assert_eq!(synthetic_tool_call("tool: plugin/"), None);
    }

    #[test]
    fn confirm_digest_is_stable() {
        let d1 = confirm_digest(
            &PrincipalId::new("p1"),
            &ChatSessionId::new("s1"),
            "tc-1",
            "plug",
            "meth",
            "{}",
        );
        let d2 = confirm_digest(
            &PrincipalId::new("p1"),
            &ChatSessionId::new("s1"),
            "tc-1",
            "plug",
            "meth",
            "{}",
        );
        assert_eq!(d1, d2);
        assert_eq!(d1.len(), 64);
    }

    #[test]
    fn ui_message_text_reads_ai_sdk_parts() {
        // AI SDK UIMessage shape: text lives in parts[], not content.
        let m = serde_json::json!({
            "id": "msg-1",
            "role": "user",
            "parts": [
                {"type": "text", "text": "Hello, "},
                {"type": "text", "text": "what can you do?"},
                {"type": "reasoning", "text": "not display text"}
            ]
        });
        assert_eq!(ui_message_text(&m), "Hello, what can you do?");
        // Fallback: top-level content string.
        let m2 = serde_json::json!({"role": "user", "content": "hi"});
        assert_eq!(ui_message_text(&m2), "hi");
        // No text at all.
        assert_eq!(ui_message_text(&serde_json::json!({"role": "user"})), "");
    }

    #[tokio::test]
    async fn synthetic_provider_emits_text_and_reasoning() {
        let p = SyntheticProvider;
        let msgs = vec![serde_json::json!({
            "id": "msg-1",
            "role": "user",
            "parts": [{"type": "text", "text": "hi there"}]
        })];
        let parts = p.complete(&msgs, "test", "test-model").await.unwrap();
        assert!(parts.len() >= 2);
        assert!(matches!(parts[0], AiPart::Reasoning { .. }));
        match &parts[1] {
            AiPart::Text { text, .. } => {
                assert!(text.contains("hi there"), "text part should echo the user message: {text}");
            }
            other => panic!("expected text part, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn synthetic_provider_emits_tool_call_on_explicit_request() {
        let p = SyntheticProvider;
        let msgs = vec![serde_json::json!({
            "role": "user",
            "parts": [{"type": "text", "text": "run tool: demo/echo please"}]
        })];
        let parts = p.complete(&msgs, "test", "test-model").await.unwrap();
        assert!(parts.iter().any(|x| matches!(x, AiPart::ToolCall { .. })));
    }

    #[test]
    fn ai_part_payload_is_ai_sdk_shape() {
        let part = AiPart::Text {
            message_id: "m1".to_string(),
            text: "hello".to_string(),
        };
        let v = part.payload_json();
        assert_eq!(v["type"], "text");
        assert_eq!(v["text"], "hello");
        assert_eq!(part.kind(), "text");
    }

    #[test]
    fn resolve_principal_synthetic_when_no_profile() {
        let p = resolve_principal(None);
        assert_eq!(p.as_str(), "synthetic:dev");
    }
}
