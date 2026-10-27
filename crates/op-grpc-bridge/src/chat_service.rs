//! ChatService gRPC server implementation.
//!
//! Implements the `op_chat.chat.ChatService` trait from `chat.proto`.
//! Served on the op-grpc-bridge alongside StateSync, PluginService, etc.
//! for discovery through a single reflection endpoint.
#![allow(dead_code)] // many items are infrastructure for upcoming chat features
//!
//! Architecture:
//! - threetched owns provider/model routing (OD-28) — SendRequest carries them.
//! - gemma_brain routes to the selected model and applies compliance tags.
//! - The chatbot is a DELEGATOR — forced tool calling is mandatory; even user
//!   responses are emitted as tool calls (respond_to_user).
//! - Chat persistence flows through the memory loop → cognitive.
//! - The agent loop is bounded (≥50 steps) and cancellable.

use std::pin::Pin;
use std::str::FromStr;
use std::sync::Arc;

use anyhow::{Context, anyhow};
use op_llm::ProviderType;
use op_llm::chat::ChatManager;
use op_plugins::state_plugins::chat_router::{
    ChatInput, ChatOutput, ChatMessage, ChatRouterState,
};
use tokio::sync::Mutex;
use tokio_stream::Stream;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};
use tracing::{info, warn};

use crate::interceptor::{GhostbridgeIdentity, bridge_capability_identity};
use crate::mutation_engine::MutationEngine;
use crate::proto::chat::{
    ApproveRequest, ApproveResponse, CancelRequest, CancelResponse, ChatFrame, Heartbeat,
    SendRequest, StreamDone, StreamError, UiMessagePart, chat_frame,
    chat_service_server::ChatService,
};

const ROUTER_PLUGIN_ID: &str = "tched_router";
const ROUTER_CHAT_CAPABILITY: &str = "cap.software.3tched-router.chat@v1";

// Conversation/tool ids are client-controlled correlation labels, never an
// authorization boundary. Include both stable principal and verified session.
type ConversationKey = (String, String, String);
type ApprovalKey = (ConversationKey, String);

#[allow(clippy::result_large_err)]
fn conversation_key(identity: &GhostbridgeIdentity, id: &str) -> Result<ConversationKey, Status> {
    if id.is_empty() || id.len() > 128 || id.chars().any(char::is_control) {
        return Err(Status::invalid_argument("invalid conversation_id"));
    }
    Ok((
        identity.principal_id.clone(),
        identity.session_id.clone(),
        id.into(),
    ))
}

#[allow(clippy::result_large_err)]
fn chat_identity<T>(request: &Request<T>) -> Result<GhostbridgeIdentity, Status> {
    let identity = bridge_capability_identity(request.extensions())
        .ok_or_else(|| Status::unauthenticated("verified chat identity is required"))?;
    crate::grpc_server::authorize_schema_method(
        ROUTER_PLUGIN_ID,
        "Chat",
        Some(ROUTER_CHAT_CAPABILITY),
        Some(&identity),
    )?;
    Ok(identity)
}

#[allow(clippy::result_large_err)]
fn reserve_cancellation(
    cancellations: &mut std::collections::HashMap<
        ConversationKey,
        tokio::sync::watch::Sender<bool>,
    >,
    key: ConversationKey,
    cancel_tx: tokio::sync::watch::Sender<bool>,
) -> Result<(), Status> {
    if cancellations.contains_key(&key) {
        return Err(Status::already_exists(
            "conversation already has an active turn",
        ));
    }
    cancellations.insert(key, cancel_tx);
    Ok(())
}

#[allow(dead_code)]
#[derive(Clone, Debug)]
struct ResolvedExecutionRoute {
    provider: ProviderType,
    provider_id: String,
    model: String,
    declared_available: bool,
    status_reason: String,
}

/// Shared state for the ChatService.
pub struct ChatServiceImpl {
    /// Active conversation cursors for cancel support.
    cancellations:
        Arc<Mutex<std::collections::HashMap<ConversationKey, tokio::sync::watch::Sender<bool>>>>,
    /// Pending approvals are scoped to principal, session and conversation.
    approvals:
        Arc<Mutex<std::collections::HashMap<ApprovalKey, tokio::sync::oneshot::Sender<bool>>>>,
    /// The sole schema method/event-chain authority.
    engine: Arc<MutationEngine>,
}

impl ChatServiceImpl {
    pub fn new(engine: Arc<MutationEngine>) -> Self {
        Self {
            cancellations: Arc::new(Mutex::new(Default::default())),
            approvals: Arc::new(Mutex::new(Default::default())),
            engine,
        }
    }
}

type ChatStream = Pin<Box<dyn Stream<Item = Result<ChatFrame, Status>> + Send + 'static>>;

async fn dispatch_chat_method(
    engine: &MutationEngine,
    chat_args: &str,
    identity: &GhostbridgeIdentity,
) -> anyhow::Result<ChatOutput> {
    let result = engine
        .dispatch_method_call_with_identity(
            ROUTER_PLUGIN_ID,
            "Chat",
            chat_args,
            Some(ROUTER_CHAT_CAPABILITY),
            &identity.principal_id,
            Some(&identity.session_id),
            Some(&identity.session_genesis),
        )
        .await?;
    let payload = result
        .get("result")
        .cloned()
        .ok_or_else(|| anyhow!("chat_router.Chat returned no result payload"))?;
    serde_json::from_value::<ChatOutput>(payload).context("invalid chat_router.Chat result")
}

fn provider_names(state: &op_plugins::state_plugins::chat_router::ChatRouterState, requested: &str) -> Option<Vec<String>> {
    let requested = requested.trim();
    state
        .catalog
        .providers
        .iter()
        .find(|provider| {
            provider.id.eq_ignore_ascii_case(requested)
                || provider.route.eq_ignore_ascii_case(requested)
                || provider
                    .aliases
                    .iter()
                    .any(|alias| alias.eq_ignore_ascii_case(requested))
        })
        .map(|provider| {
            let mut names = vec![provider.id.clone(), provider.route.clone()];
            names.extend(provider.aliases.clone());
            names
        })
}

fn names_contain(names: &[String], candidate: &str) -> bool {
    names
        .iter()
        .any(|name| name.eq_ignore_ascii_case(candidate))
}

fn provider_type_for(state: &ChatRouterState, provider_id: &str) -> anyhow::Result<ProviderType> {
    let names = provider_names(state, provider_id)
        .ok_or_else(|| anyhow!("provider '{provider_id}' is not declared by threetched"))?;
    names
        .iter()
        .find_map(|name| ProviderType::from_str(name).ok())
        .ok_or_else(|| {
            anyhow!(
                "provider '{}' is declared but has no op-llm runtime adapter",
                names.first().map(String::as_str).unwrap_or(provider_id)
            )
        })
}

fn resolve_declared_route(
    state: &ChatRouterState,
    requested_provider: &str,
    requested_model: &str,
) -> anyhow::Result<ResolvedExecutionRoute> {
    let provider_request = if !requested_provider.trim().is_empty() {
        requested_provider.trim()
    } else {
        state.selected_provider.as_str()
    };
    let model_request = if !requested_model.trim().is_empty() {
        requested_model.trim()
    } else {
        state.selected_model.as_str()
    };

    let provider_aliases = provider_names(state, provider_request)
        .ok_or_else(|| anyhow!("provider '{provider_request}' is not declared by threetched"))?;

    let provider_matches =
        |route: &&op_plugins::state_plugins::common::llm_projection::ModelRoute| {
            names_contain(&provider_aliases, &route.provider)
                || names_contain(&provider_aliases, &route.upstream_provider)
        };
    let exact_model = |route: &&op_plugins::state_plugins::common::llm_projection::ModelRoute| {
        route.model.eq_ignore_ascii_case(model_request)
    };
    let hint = |route: &&op_plugins::state_plugins::common::llm_projection::ModelRoute| {
        route.hint.eq_ignore_ascii_case(model_request)
    };

    let route = state
        .catalog
        .model_routes
        .iter()
        .filter(provider_matches)
        .find(exact_model)
        .or_else(|| {
            state
                .catalog
                .model_routes
                .iter()
                .filter(provider_matches)
                .find(hint)
        })
        .ok_or_else(|| {
            anyhow!(
                "model or route hint '{model_request}' is not declared for provider '{provider_request}'"
            )
        })?;

    if !matches!(route.kind.as_str(), "chat" | "orchestrator") {
        return Err(anyhow!(
            "route '{}' is kind '{}' and cannot serve chat",
            route.model,
            route.kind
        ));
    }

    let execution_provider = if route.upstream_provider.is_empty() {
        route.provider.as_str()
    } else {
        route.upstream_provider.as_str()
    };

    Ok(ResolvedExecutionRoute {
        provider: provider_type_for(state, execution_provider)?,
        provider_id: execution_provider.to_string(),
        model: route.model.clone(),
        declared_available: route.available,
        status_reason: route.status_reason.clone(),
    })
}

async fn execute_chat(
    chat_manager: &ChatManager,
    state: &ChatRouterState,
    requested_provider: &str,
    requested_model: &str,
    messages: Vec<op_llm::ChatMessage>,
) -> anyhow::Result<(ResolvedExecutionRoute, op_llm::ChatResponse)> {
    let route = resolve_declared_route(state, requested_provider, requested_model)?;
    if !chat_manager.has_provider(&route.provider) {
        return Err(anyhow!(
            "provider '{}' is declared but not configured in the bridge runtime",
            route.provider_id
        ));
    }

    if !route.declared_available {
        let models = chat_manager
            .list_models_for_provider(&route.provider)
            .await
            .with_context(|| {
                format!(
                    "route '{}' is unavailable: {}",
                    route.model, route.status_reason
                )
            })?;
        if route.model != "auto" && !models.iter().any(|model| model.id == route.model) {
            return Err(anyhow!(
                "route '{}' is unavailable: {}",
                route.model,
                route.status_reason
            ));
        }
    }

    let response = chat_manager
        .chat_with(&route.provider, &route.model, messages)
        .await
        .with_context(|| {
            format!(
                "provider '{}' failed model '{}'",
                route.provider_id, route.model
            )
        })?;
    Ok((route, response))
}

/// Execute the schema-declared `chat_router.Chat` method after the mutation
/// engine has recorded the call. Provider/model selection remains owned by
/// the projected threetched schema; `ChatManager` only performs the resolved
/// upstream call.
pub(crate) async fn dispatch_schema_chat(
    chat_manager: &ChatManager,
    state: &ChatRouterState,
    input: ChatInput,
) -> anyhow::Result<ChatOutput> {
    let messages = if input.messages.is_empty() {
        if input.message.trim().is_empty() {
            return Err(anyhow!("chat_router.Chat requires message or messages"));
        }
        vec![op_llm::ChatMessage {
            role: "user".to_string(),
            content: input.message,
            tool_calls: None,
            tool_call_id: None,
        }]
    } else {
        input
            .messages
            .into_iter()
            .map(|message| op_llm::ChatMessage {
                role: message.role,
                content: message.content,
                tool_calls: None,
                tool_call_id: None,
            })
            .collect()
    };

    let (route, response) =
        execute_chat(chat_manager, state, &input.provider, &input.model, messages).await?;

    Ok(ChatOutput {
        content: response.message.content,
        provider: route.provider_id,
        model: route.model,
        finish_reason: response.finish_reason.unwrap_or_else(|| "stop".to_string()),
        usage: serde_json::to_value(response.usage).unwrap_or(serde_json::Value::Null),
    })
}

#[async_trait::async_trait]
impl ChatService for ChatServiceImpl {
    type SendStream = ChatStream;

    async fn send(
        &self,
        request: Request<SendRequest>,
    ) -> Result<Response<Self::SendStream>, Status> {
        let identity = chat_identity(&request)?;
        let req = request.into_inner();
        let conversation_id = req.conversation_id.clone();
        let key = conversation_key(&identity, &conversation_id)?;
        let provider = req.provider.clone();
        let model = req.model.clone();

        info!(
            conversation_id = %conversation_id,
            provider = %provider,
            model = %model,
            "ChatService.Send — streaming chat completion"
        );

        // Parse ui_messages from JSON bytes.
        if req.ui_messages.len() > 1024 * 1024 {
            return Err(Status::resource_exhausted("chat input exceeds 1 MiB"));
        }
        let ui_messages: Vec<serde_json::Value> = serde_json::from_slice(&req.ui_messages)
            .map_err(|e| Status::invalid_argument(format!("Invalid ui_messages JSON: {e}")))?;
        if ui_messages.is_empty()
            || ui_messages.len() > 256
            || ui_messages.iter().any(|message| {
                !matches!(
                    message.get("role").and_then(serde_json::Value::as_str),
                    Some("user" | "assistant")
                ) || message
                    .get("content")
                    .and_then(serde_json::Value::as_str)
                    .is_none()
            })
        {
            return Err(Status::invalid_argument(
                "expected 1–256 user/assistant text messages",
            ));
        }
        let chat_args = ChatInput {
            message: String::new(),
            messages: ui_messages
                .iter()
                .map(|message| ChatMessage {
                    role: message
                        .get("role")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("user")
                        .to_string(),
                    content: message
                        .get("content")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                })
                .collect(),
            provider,
            model,
            conversation_id: Some(conversation_id.clone()),
        };
        let chat_args = serde_json::to_string(&chat_args)
            .map_err(|error| Status::invalid_argument(error.to_string()))?;

        // Set up cancellation channel.
        let (cancel_tx, mut cancel_rx) = tokio::sync::watch::channel(false);
        {
            let mut cancellations = self.cancellations.lock().await;
            reserve_cancellation(&mut cancellations, key.clone(), cancel_tx)?;
        }

        // Cursor for monotonic frame ordering.
        let cursor = std::sync::atomic::AtomicU64::new(0);

        let (tx, rx) = tokio::sync::mpsc::channel::<Result<ChatFrame, Status>>(64);

        let cancellations = self.cancellations.clone();
        let engine = self.engine.clone();
        let conv_id = conversation_id.clone();

        tokio::spawn(async move {
            let bump = || cursor.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            // Emit heartbeat immediately so the client knows the stream is alive.
            let _ = tx
                .send(Ok(ChatFrame {
                    cursor: bump(),
                    body: Some(chat_frame::Body::Heartbeat(Heartbeat {
                        server_time_ms: chrono::Utc::now().timestamp_millis() as u64,
                    })),
                }))
                .await;

            let completion = tokio::select! {
                result = dispatch_chat_method(engine.as_ref(), &chat_args, &identity) => result,
                _ = tx.closed() => Err(anyhow!("chat cancelled: client disconnected")),
                changed = cancel_rx.changed() => {
                    match changed {
                        Ok(()) if *cancel_rx.borrow() => Err(anyhow!("chat cancelled")),
                        Ok(()) => Err(anyhow!("chat cancellation channel changed unexpectedly")),
                        Err(_) => Err(anyhow!("chat cancellation channel closed")),
                    }
                }
            };

            let total_parts = match completion {
                Ok(response) => {
                    let payload = serde_json::json!({
                        "type": "text",
                        "text": response.content,
                        "provider": response.provider,
                        "model": response.model,
                    });
                    let _ = tx
                        .send(Ok(ChatFrame {
                            cursor: bump(),
                            body: Some(chat_frame::Body::Part(UiMessagePart {
                                message_id: uuid::Uuid::new_v4().to_string(),
                                role: "assistant".to_string(),
                                kind: "text".to_string(),
                                payload: serde_json::to_vec(&payload).unwrap_or_default(),
                            })),
                        }))
                        .await;
                    1
                }
                Err(error) => {
                    let cancelled = error.to_string().contains("cancelled");
                    let _ = tx
                        .send(Ok(ChatFrame {
                            cursor: bump(),
                            body: Some(chat_frame::Body::Error(StreamError {
                                code: if cancelled {
                                    "cancelled".to_string()
                                } else {
                                    "route_unavailable".to_string()
                                },
                                message: error.to_string(),
                                retryable: false,
                                retry_after_ms: None,
                            })),
                        }))
                        .await;
                    0
                }
            };

            // Stream done.
            let _ = tx
                .send(Ok(ChatFrame {
                    cursor: bump(),
                    body: Some(chat_frame::Body::Done(StreamDone {
                        conversation_id: conv_id.clone(),
                        total_parts,
                    })),
                }))
                .await;

            // Cleanup cancellation registration.
            {
                let mut cancellations = cancellations.lock().await;
                cancellations.remove(&key);
            }
        });

        Ok(Response::new(Box::pin(ReceiverStream::new(rx))))
    }

    async fn approve(
        &self,
        request: Request<ApproveRequest>,
    ) -> Result<Response<ApproveResponse>, Status> {
        let identity = chat_identity(&request)?;
        let req = request.into_inner();
        let key = (
            conversation_key(&identity, &req.conversation_id)?,
            req.tool_call_id.clone(),
        );
        info!(
            conversation_id = %req.conversation_id,
            tool_call_id = %req.tool_call_id,
            approved = req.approved,
            "ChatService.Approve"
        );

        // Deliver the approval decision to the pending tool call.
        let mut approvals = self.approvals.lock().await;
        match approvals.remove(&key) {
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
        let identity = chat_identity(&request)?;
        let req = request.into_inner();
        let key = conversation_key(&identity, &req.conversation_id)?;
        info!(
            conversation_id = %req.conversation_id,
            "ChatService.Cancel"
        );

        let cancellations = self.cancellations.lock().await;
        // Keep the slot until the running task finishes. Removing it here
        // lets an overlapping Send install a slot that old cleanup erases.
        match cancellations.get(&key) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use op_plugins::state_plugins::chat_router::ChatRouterPlugin;

    fn identity(principal_id: &str, session_id: &str) -> GhostbridgeIdentity {
        GhostbridgeIdentity {
            principal_id: principal_id.into(),
            session_id: session_id.into(),
            session_genesis: "test-genesis".into(),
        }
    }

    fn test_service() -> ChatServiceImpl {
        let event_chain = Arc::new(tokio::sync::RwLock::new(op_state_store::EventChain::new(
            op_state_store::ChainConfig::default(),
        )));
        let ovsdb = Arc::new(op_network::rovs_proxy::OvsdbDbusClient::new());
        ChatServiceImpl::new(Arc::new(MutationEngine::new(event_chain, ovsdb)))
    }

    #[test]
    fn chat_service_binding_matches_router_schema() {
        let schema = op_plugins::state_plugins::chat_router::chat_router_plugin_schema();
        let chat = schema.methods.get("Chat").expect("Chat method");

        assert_eq!(schema.name, ROUTER_PLUGIN_ID);
        assert_eq!(
            chat.required_capability.as_deref(),
            Some(ROUTER_CHAT_CAPABILITY)
        );
    }

    #[tokio::test]
    async fn chat_send_requires_ghostbridge_identity() {
        let service = test_service();

        let status = service
            .send(Request::new(SendRequest::default()))
            .await
            .err()
            .expect("request without identity must be rejected");

        assert_eq!(status.code(), tonic::Code::Unauthenticated);
    }

    #[test]
    fn conversation_key_scopes_principal_and_session() {
        let conversation = "same-conversation";
        let first = conversation_key(&identity("principal-a", "session-a"), conversation)
            .expect("valid key");
        let other_principal =
            conversation_key(&identity("principal-b", "session-a"), conversation).unwrap();
        let other_session =
            conversation_key(&identity("principal-a", "session-b"), conversation).unwrap();
        assert_ne!(first, other_principal);
        assert_ne!(first, other_session);
        assert_ne!(other_principal, other_session);
    }

    #[tokio::test]
    async fn chat_approve_and_cancel_require_ghostbridge_identity() {
        let service = test_service();
        let approve = service
            .approve(Request::new(ApproveRequest {
                conversation_id: "conversation".into(),
                tool_call_id: "tool".into(),
                approved: true,
                operator_note: Some(String::new()),
            }))
            .await
            .expect_err("approve without identity must be rejected");
        assert_eq!(approve.code(), tonic::Code::Unauthenticated);

        let cancel = service
            .cancel(Request::new(CancelRequest {
                conversation_id: "conversation".into(),
            }))
            .await
            .expect_err("cancel without identity must be rejected");
        assert_eq!(cancel.code(), tonic::Code::Unauthenticated);
    }

    #[tokio::test]
    async fn cancellation_slots_are_keyed_and_duplicate_sends_do_not_replace_them() {
        let service = test_service();
        let first_key = conversation_key(&identity("principal-a", "session-a"), "conversation")
            .expect("valid key");
        let other_key = conversation_key(&identity("principal-b", "session-a"), "conversation")
            .expect("valid key");
        let (first_tx, _first_rx) = tokio::sync::watch::channel(false);
        let (replacement_tx, _replacement_rx) = tokio::sync::watch::channel(false);
        let mut cancellations = service.cancellations.lock().await;
        reserve_cancellation(&mut cancellations, first_key.clone(), first_tx)
            .expect("first send reserves its slot");
        let duplicate = reserve_cancellation(&mut cancellations, first_key.clone(), replacement_tx)
            .expect_err("duplicate send must not replace the active slot");
        assert_eq!(duplicate.code(), tonic::Code::AlreadyExists);
        assert!(!cancellations.contains_key(&other_key));
        drop(cancellations);

        let cancel = service
            .cancellations
            .lock()
            .await
            .get(&first_key)
            .expect("original slot remains")
            .clone();
        cancel.send(true).expect("cancel original slot");
        assert!(cancel.borrow().to_owned());
    }

    #[tokio::test]
    async fn approval_slots_cannot_cross_principal_or_session_boundaries() {
        let service = test_service();
        let first = (
            conversation_key(&identity("principal-a", "session-a"), "conversation")
                .expect("valid key"),
            "tool".to_string(),
        );
        let other = (
            conversation_key(&identity("principal-b", "session-a"), "conversation")
                .expect("valid key"),
            "tool".to_string(),
        );
        let (sender, receiver) = tokio::sync::oneshot::channel();
        service.approvals.lock().await.insert(first.clone(), sender);

        let mut approvals = service.approvals.lock().await;
        assert!(approvals.remove(&other).is_none());
        let sender = approvals.remove(&first).expect("original approval slot");
        sender.send(true).expect("deliver original approval");
        drop(approvals);
        assert!(receiver.await.expect("approval response"));
    }

    #[tokio::test]
    async fn chat_dispatch_reaches_the_chat_router_backend() {
        use crate::human_principal_dispatch::tests::{pk, register, temp_cozo};
        use crate::identity_sled_dispatch::{IdentitySledStore, tests::write_identity};
        use op_plugins::state_plugins::{
            human_principal::HumanPrincipalPlugin, identity_sled::IdentitySledPlugin,
        };
        use op_state::StatePlugin;

        let _cozo = temp_cozo();
        let router_schema = ChatRouterPlugin::new().schema().unwrap();
        let _catalog = crate::human_principal_dispatch::tests::temp_shm_with_schemas(&[
            HumanPrincipalPlugin::new().schema().unwrap(),
            IdentitySledPlugin::new().schema().unwrap(),
            op_plugins::state_plugins::ctl_plane_chatbot::CtlPlaneChatbotPlugin::new()
                .schema()
                .unwrap(),
            router_schema.clone(),
        ]);
        struct Restore(Vec<(&'static str, Option<std::ffi::OsString>)>);
        impl Drop for Restore {
            fn drop(&mut self) {
                for (key, value) in &self.0 {
                    match value {
                        // FIXME: Audit that the environment access only happens in single-threaded code.
                        Some(value) => unsafe { std::env::set_var(key, value) },
                        // FIXME: Audit that the environment access only happens in single-threaded code.
                        None => unsafe { std::env::remove_var(key) },
                    }
                }
            }
        }
        let _restore = Restore(
            [
                "TCHED_ROUTER_RUNTIME_ENDPOINT",
                "TCHED_ROUTER_RUNTIME_TOKEN",
                "OP_GRANTS_PATH",
            ]
            .into_iter()
            .map(|key| (key, std::env::var_os(key)))
            .collect(),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let app = axum::Router::new().route(
            "/health",
            axum::routing::get(|| async {
                axum::Json(serde_json::json!({"status": "fixture-offline"}))
            }),
        );
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::set_var("TCHED_ROUTER_RUNTIME_ENDPOINT", endpoint) };
        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::remove_var("TCHED_ROUTER_RUNTIME_TOKEN") };
        let event_chain = Arc::new(tokio::sync::RwLock::new(op_state_store::EventChain::new(
            op_state_store::ChainConfig::default(),
        )));
        let ovsdb = Arc::new(op_network::rovs_proxy::OvsdbDbusClient::new());
        let engine = MutationEngine::new(event_chain, ovsdb);
        engine
            .install_identity_sled_store_for_test(IdentitySledStore::new(
                op_cozo_store::CozoGraphShuttle::new_in_memory().unwrap(),
            ))
            .unwrap();
        let key = pk(0xC7);
        register(&key, "chat-router-fixture").await.unwrap();
        let sled = write_identity(&engine, &key).await.unwrap();
        let actor = op_identity::session::derive_principal_id(&key);
        let capability = router_schema.methods["Chat"]
            .required_capability
            .clone()
            .unwrap();
        let grants_dir = tempfile::tempdir().unwrap();
        engine
            .configure_chatbot_episode_store(Arc::new(
                op_cozo_store::CozoGraphShuttle::new_in_memory().unwrap(),
            ))
            .unwrap();
        engine
            .configure_chatbot_episode_projection(
                op_core::projection_shm::PrivateProjectionDirectory::prepare(
                    &grants_dir.path().join("episodes"),
                )
                .unwrap(),
                crate::sealed_schema_reader::blob_catalog_dir_from_env(),
            )
            .unwrap();
        let grants = grants_dir.path().join("grants.json");
        std::fs::write(
            &grants,
            serde_json::to_vec(&serde_json::json!({
                (actor.clone()): {"capabilities":[capability]}
            }))
            .unwrap(),
        )
        .unwrap();
        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::set_var("OP_GRANTS_PATH", grants) };
        let input = ChatInput {
            message: "hello".to_string(),
            provider: "salad".to_string(),
            model: "not-a-model".to_string(),
            ..Default::default()
        };
        let args = serde_json::to_string(&input).expect("chat input");

        let error = dispatch_chat_method(
            &engine,
            &args,
            &GhostbridgeIdentity {
                principal_id: actor,
                session_id: sled.session_id,
                session_genesis: sled.genesis.expect("identity genesis"),
            },
        )
        .await
        .expect_err("the backend fixture must reject chat while unhealthy");
        server.abort();

        let message = error.to_string();
        assert!(
            message.contains("chat_router health status is 'fixture-offline', expected 'ok'"),
            "{error}"
        );
    }

    #[test]
    fn explicit_provider_and_model_resolve_through_schema_catalog() {
        let mut state = ChatRouterPlugin::current_state();
        let route = state
            .catalog
            .model_routes
            .iter_mut()
            .find(|route| route.provider == "salad" && route.hint == "balanced")
            .expect("balanced Salad route");
        route.available = true;

        let resolved =
            resolve_declared_route(&state, "salad", "qwen3.6-27b").expect("route resolves");
        assert_eq!(resolved.provider, ProviderType::Salad);
        assert_eq!(resolved.provider_id, "salad");
        assert_eq!(resolved.model, "qwen3.6-27b");
        assert!(resolved.declared_available);
    }

    #[test]
    fn provider_alias_and_route_hint_resolve() {
        let state = ChatRouterPlugin::current_state();
        let resolved =
            resolve_declared_route(&state, "salad-ai", "fast").expect("alias and hint resolve");
        assert_eq!(resolved.provider, ProviderType::Salad);
        assert_eq!(resolved.model, "qwen3.5-9b");
    }

    #[test]
    fn selected_provider_and_model_are_the_default_route() {
        let mut state = ChatRouterPlugin::current_state();
        state.selected_provider = "salad".to_string();
        state.selected_model = "qwen3.6-35b-a3b".to_string();

        let resolved = resolve_declared_route(&state, "", "").expect("selected route resolves");
        assert_eq!(resolved.provider, ProviderType::Salad);
        assert_eq!(resolved.model, "qwen3.6-35b-a3b");
    }

    #[test]
    fn undeclared_model_fails_closed() {
        let state = ChatRouterPlugin::current_state();
        let error = resolve_declared_route(&state, "salad", "not-a-model").unwrap_err();
        assert!(error.to_string().contains("not declared"));
    }

    #[test]
    fn non_chat_routes_fail_closed() {
        let state = ChatRouterPlugin::current_state();
        let error = resolve_declared_route(&state, "oscal", "compliance").unwrap_err();
        assert!(error.to_string().contains("cannot serve chat"));
    }
}
