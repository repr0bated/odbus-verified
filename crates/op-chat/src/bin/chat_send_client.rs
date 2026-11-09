//! Minimal end-to-end test client for the Chat_Route (ChatService).
//!
//! Connects to the Chat_Endpoint, sends one user message, and prints every
//! ChatFrame received. Used to verify the chat-route binary serves gRPC
//! (and gRPC-Web via tonic-web) on 10.0.0.2:50051.
//!
//! Usage: `chat-send-client [endpoint] [message]`
//! Defaults: endpoint http://10.0.0.2:50051, message "Hello, what can you do?"

use anyhow::Result;
use op_chat::orchestration::proto::op_chat_chat::{
    ApproveRequest, SendRequest, chat_frame, chat_service_client::ChatServiceClient,
};
use tokio_stream::StreamExt;

#[tokio::main]
async fn main() -> Result<()> {
    let endpoint = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "http://10.0.0.2:50051".to_string());
    let text = std::env::args()
        .nth(2)
        .unwrap_or_else(|| "Hello, what can you do?".to_string());
    // Third arg: "approve" => auto-approve any ApprovalRequired frame.
    let auto_approve = std::env::args().nth(3).as_deref() == Some("approve");

    println!("connecting to {endpoint}");
    let mut client = ChatServiceClient::connect(endpoint).await?;
    println!("connected");

    // AI SDK UIMessage[] JSON: one user message with a text part.
    let ui_messages = serde_json::json!([
        {
            "id": "msg-1",
            "role": "user",
            "parts": [{ "type": "text", "text": text }]
        }
    ]);
    let conversation_id = "test-conv-1".to_string();
    let req = SendRequest {
        conversation_id: conversation_id.clone(),
        ui_messages: serde_json::to_vec(&ui_messages)?,
        persona_id: None,
        provider: "synthetic".to_string(),
        model: "synthetic".to_string(),
        resume_cursor: None,
    };

    println!("sending Send request (conversation_id={conversation_id})");
    let mut stream = client.send(req).await?.into_inner();

    let mut frames = 0u64;
    while let Some(frame) = stream.next().await {
        let frame = frame?;
        frames += 1;
        match &frame.body {
            Some(chat_frame::Body::Part(p)) => {
                let payload = String::from_utf8_lossy(&p.payload);
                println!(
                    "frame cursor={} part message_id={} role={} kind={} payload={}",
                    frame.cursor, p.message_id, p.role, p.kind, payload
                );
            }
            Some(chat_frame::Body::Error(e)) => {
                println!("frame cursor={} ERROR code={} message={} retryable={}", frame.cursor, e.code, e.message, e.retryable);
            }
            Some(chat_frame::Body::Heartbeat(h)) => {
                println!("frame cursor={} heartbeat server_time_ms={}", frame.cursor, h.server_time_ms);
            }
            Some(chat_frame::Body::Done(d)) => {
                println!("frame cursor={} DONE conversation_id={} total_parts={}", frame.cursor, d.conversation_id, d.total_parts);
            }
            Some(chat_frame::Body::Approval(a)) => {
                println!("frame cursor={} APPROVAL tool_call_id={} tool_name={} description={}", frame.cursor, a.tool_call_id, a.tool_name, a.description);
                if auto_approve {
                    println!("auto-approving tool_call_id={}", a.tool_call_id);
                    let resp = client
                        .approve(ApproveRequest {
                            conversation_id: conversation_id.clone(),
                            tool_call_id: a.tool_call_id.clone(),
                            approved: true,
                            operator_note: Some("auto-approved by chat-send-client".to_string()),
                        })
                        .await?
                        .into_inner();
                    println!("approve response: success={} tool_call_id={} error={:?}", resp.success, resp.tool_call_id, resp.error);
                }
            }
            None => {
                println!("frame cursor={} (empty body)", frame.cursor);
            }
        }
    }
    println!("stream closed after {frames} frames");
    Ok(())
}
