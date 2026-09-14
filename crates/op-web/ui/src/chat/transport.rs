//! gRPC transport for ChatService streaming.
//!
//! Connects to the ChatService gRPC-Web endpoint, sends chat requests,
//! and pumps incoming ChatFrame events into a channel that the egui
//! frame loop drains into the ChatStore.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tonic::transport::Channel;

use super::store::ChatMessage;

include!("../proto/op_chat.chat.rs");

/// Re-export the generated client module.
pub mod chat_client {
    pub use super::chat_service_client::ChatServiceClient;
}

/// Request to send to ChatService.Send (server-streaming).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendRequestPayload {
    pub conversation_id: String,
    pub ui_messages: Vec<serde_json::Value>,
    pub persona_id: Option<String>,
    pub provider: String,
    pub model: String,
}

/// A single frame from the ChatService stream, decoded for the view layer.
#[derive(Debug, Clone)]
pub enum ChatFrameEvent {
    Part(ChatMessage),
    Error(String),
    Heartbeat,
    Done,
    ApprovalRequired {
        tool_call_id: String,
        tool_name: String,
        tool_input: serde_json::Value,
        description: String,
    },
}

/// gRPC transport handle. Holds a tonic Channel to the ChatService.
/// Created once at startup; reused for the lifetime of the app.
pub struct ChatTransport {
    channel: Channel,
}

impl ChatTransport {
    /// Connect to a ChatService endpoint.
    pub async fn connect(endpoint: &str) -> Result<Self> {
        let channel = crate::conn::connect_channel(endpoint).await?;
        Ok(Self { channel })
    }

    /// Send a chat request and return a channel of decoded frame events.
    ///
    /// Spawns a background task that reads the server-streaming response
    /// and forwards decoded ChatFrameEvent items into the channel.
    pub async fn send(&self, payload: SendRequestPayload) -> Result<mpsc::Receiver<ChatFrameEvent>> {
        let mut client = chat_service_client::ChatServiceClient::new(self.channel.clone());

        let ui_messages_bytes = serde_json::to_vec(&payload.ui_messages)?;

        let req = SendRequest {
            conversation_id: payload.conversation_id,
            ui_messages: ui_messages_bytes,
            persona_id: payload.persona_id,
            provider: payload.provider,
            model: payload.model,
            resume_cursor: None,
        };

        let response = client.send(tonic::Request::new(req)).await?;
        let mut stream = response.into_inner();

        let (tx, rx) = mpsc::channel::<ChatFrameEvent>(64);

        tokio::spawn(async move {
            use chat_frame::Body;
            while let Ok(Some(frame)) = stream.message().await {
                let event = match frame.body {
                    Some(Body::Part(part)) => {
                        let payload_val = serde_json::from_slice::<serde_json::Value>(&part.payload)
                            .unwrap_or(serde_json::Value::Null);
                        let msg = ChatMessage {
                            id: part.message_id,
                            role: part.role,
                            kind: part.kind,
                            payload: payload_val,
                            timestamp: std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .map(|d| d.as_millis() as u64)
                                .unwrap_or(0),
                        };
                        ChatFrameEvent::Part(msg)
                    }
                    Some(Body::Error(e)) => ChatFrameEvent::Error(e.message),
                    Some(Body::Heartbeat(_)) => ChatFrameEvent::Heartbeat,
                    Some(Body::Done(_)) => ChatFrameEvent::Done,
                    Some(Body::Approval(a)) => {
                        let tool_input = serde_json::from_slice(&a.tool_input)
                            .unwrap_or(serde_json::Value::Null);
                        ChatFrameEvent::ApprovalRequired {
                            tool_call_id: a.tool_call_id,
                            tool_name: a.tool_name,
                            tool_input,
                            description: a.description,
                        }
                    }
                    None => continue,
                };

                if tx.send(event).await.is_err() {
                    break;
                }
            }
        });

        Ok(rx)
    }

    /// Send an approval decision for a pending tool call.
    pub async fn approve(
        &self,
        conversation_id: String,
        tool_call_id: String,
        approved: bool,
    ) -> Result<bool> {
        let mut client = chat_service_client::ChatServiceClient::new(self.channel.clone());
        let req = ApproveRequest {
            conversation_id,
            tool_call_id: tool_call_id.clone(),
            approved,
            operator_note: None,
        };
        let resp = client.approve(tonic::Request::new(req)).await?;
        Ok(resp.into_inner().success)
    }

    /// Cancel an in-flight conversation.
    pub async fn cancel(&self, conversation_id: String) -> Result<bool> {
        let mut client = chat_service_client::ChatServiceClient::new(self.channel.clone());
        let req = CancelRequest { conversation_id };
        let resp = client.cancel(tonic::Request::new(req)).await?;
        Ok(resp.into_inner().success)
    }
}
