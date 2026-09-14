//! ChatService gRPC server implementation.
//!
//! Implements the `op_chat.chat.ChatService` trait from `chat.proto`.
//!
//! Architecture:
//! - zeroclaw owns provider/model routing (OD-28) — SendRequest carries them.
//! - gemma_brain routes to the selected model and applies compliance tags.
//! - The chatbot is a DELEGATOR — forced tool calling is mandatory; even user
//!   responses are emitted as tool calls (respond_to_user).
//! - Chat persistence flows through the memory loop → cognitive-mcp.
//! - The agent loop is bounded (≥50 steps) and cancellable.

use std::pin::Pin;
use std::sync::Arc;

use tokio::sync::Mutex;
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::Stream;
use tonic::{Request, Response, Status};
use tracing::{info, warn};

use crate::orchestration::proto::op_chat_chat::{
    chat_frame, chat_service_server::ChatService, ApproveRequest, ApproveResponse, CancelRequest,
    CancelResponse, ChatFrame, Heartbeat, SendRequest, StreamDone, UiMessagePart,
};

/// Shared state for the ChatService.
pub struct ChatServiceImpl {
    /// Active conversation cursors for cancel support.
    cancellations: Arc<Mutex<std::collections::HashMap<String, tokio::sync::watch::Sender<bool>>>>,
    /// Pending approvals: tool_call_id -> oneshot sender.
    approvals: Arc<Mutex<std::collections::HashMap<String, tokio::sync::oneshot::Sender<bool>>>>,
}

impl ChatServiceImpl {
    pub fn new() -> Self {
        Self {
            cancellations: Arc::new(Mutex::new(Default::default())),
            approvals: Arc::new(Mutex::new(Default::default())),
        }
    }
}

impl Default for ChatServiceImpl {
    fn default() -> Self {
        Self::new()
    }
}

type ChatStream = Pin<Box<dyn Stream<Item = Result<ChatFrame, Status>> + Send + 'static>>;

#[async_trait::async_trait]
impl ChatService for ChatServiceImpl {
    type SendStream = ChatStream;

    async fn send(
        &self,
        request: Request<SendRequest>,
    ) -> Result<Response<Self::SendStream>, Status> {
        let req = request.into_inner();
        let conversation_id = req.conversation_id.clone();
        let provider = req.provider.clone();
        let model = req.model.clone();

        info!(
            conversation_id = %conversation_id,
            provider = %provider,
            model = %model,
            "ChatService.Send — streaming chat completion"
        );

        // Parse ui_messages from JSON bytes.
        let ui_messages: Vec<serde_json::Value> = serde_json::from_slice(&req.ui_messages)
            .map_err(|e| Status::invalid_argument(format!("Invalid ui_messages JSON: {e}")))?;

        // Set up cancellation channel.
        let (cancel_tx, _cancel_rx) = tokio::sync::watch::channel(false);
        {
            let mut cancellations = self.cancellations.lock().await;
            cancellations.insert(conversation_id.clone(), cancel_tx);
        }

        // Cursor for monotonic frame ordering.
        let cursor = std::sync::atomic::AtomicU64::new(0);

        let (tx, rx) = tokio::sync::mpsc::channel::<Result<ChatFrame, Status>>(64);

        let cancellations = self.cancellations.clone();
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

            // TODO: Wire to gemma_brain routing + ForcedToolPipeline.
            //
            // The pipeline will:
            // 1. Build ChatRequest with tool_choice: Required
            // 2. Route through gemma_brain to the selected provider/model
            // 3. For each ModelPart yielded:
            //    - Text → ChatFrame::Part(text)
            //    - ToolCall → ChatFrame::Part(tool-call) + ApprovalRequired if needed
            //    - ToolResult → ChatFrame::Part(tool-result)
            //    - Reasoning → ChatFrame::Part(reasoning)
            // 4. Feed completed transcript to MemoryLoop for persistence
            // 5. Bounded at ≥50 steps, cancellable via cancel_rx

            // Placeholder: echo back the last user message as a reasoning part
            // so the client can verify the stream is working.
            if let Some(last_msg) = ui_messages.last() {
                let role = last_msg
                    .get("role")
                    .and_then(|v| v.as_str())
                    .unwrap_or("user");
                let content = last_msg
                    .get("content")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                let payload = serde_json::json!({
                    "type": "text",
                    "text": format!(
                        "[ChatService stub] Received message from {role} via {provider}/{model}. \
                         Delegation pipeline not yet wired. Message: {content}"
                    )
                });

                let _ = tx
                    .send(Ok(ChatFrame {
                        cursor: bump(),
                        body: Some(chat_frame::Body::Part(UiMessagePart {
                            message_id: uuid::Uuid::new_v4().to_string(),
                            role: "assistant".to_string(),
                            kind: "reasoning".to_string(),
                            payload: serde_json::to_vec(&payload).unwrap_or_default(),
                        })),
                    }))
                    .await;
            }

            // Stream done.
            let _ = tx
                .send(Ok(ChatFrame {
                    cursor: bump(),
                    body: Some(chat_frame::Body::Done(StreamDone {
                        conversation_id: conv_id.clone(),
                        total_parts: 1,
                    })),
                }))
                .await;

            // Cleanup cancellation registration.
            {
                let mut cancellations = cancellations.lock().await;
                cancellations.remove(&conv_id);
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

        // Deliver the approval decision to the pending tool call.
        let mut approvals = self.approvals.lock().await;
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

        let mut cancellations = self.cancellations.lock().await;
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
