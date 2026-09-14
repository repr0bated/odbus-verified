//! Append-only conversation store with imperative writes.
//!
//! This is NOT React-style state. There are no notifications, no re-render
//! triggers, no subscribers. The egui frame loop polls `should_repaint()`
//! each frame and the view reads the `Vec<ChatMessage>` directly.
//!
//! `parking_lot::RwLock` is intentionally avoided here: the store is owned
//! by `ExplorerState` which is already behind a single `&mut` in the egui
//! update loop. A plain `Vec` is correct and zero-overhead. If a future
//! background task needs to push messages, wrap the whole `ChatStore` in a
//! `parking_lot::Mutex` at the call site — do not add interior mutability
//! here.

use serde::{Deserialize, Serialize};
use std::time::Instant;
use tokio::sync::mpsc;

use super::transport::ChatFrameEvent;

/// A single chat message part. One logical "turn" may produce several parts
/// (e.g. an assistant reasoning part, then a tool-call part, then a text
/// part). The `kind` discriminator drives the renderer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    /// Stable unique id (uuid or nanoid from the server).
    pub id: String,
    /// `"user"` | `"assistant"` | `"tool"` | `"system"`.
    pub role: String,
    /// `"text"` | `"tool-call"` | `"tool-result"` | `"reasoning"`.
    pub kind: String,
    /// Free-form JSON payload — shape depends on `kind`.
    ///
    /// - `text`:      `{ "content": "…" }`
    /// - `tool-call`: `{ "tool": "name", "input": {…}, "tool_call_id": "…" }`
    /// - `tool-result`: `{ "tool_call_id": "…", "output": {…}, "status": "ok"|"error" }`
    /// - `reasoning`: `{ "content": "…", "plan": […] }`
    pub payload: serde_json::Value,
    /// Unix epoch milliseconds.
    pub timestamp: u64,
}

/// Append-only conversation store. Owned by `ExplorerState`.
#[derive(Debug)]
pub struct ChatStore {
    /// All message parts for the current conversation, in arrival order.
    pub messages: Vec<ChatMessage>,
    /// Conversation thread id. Groups episodes. New conversation = new id.
    pub conversation_id: String,
    /// True while we are waiting for the assistant to respond. Drives the
    /// "Thinking…" indicator and disables the submit button.
    pub submitted: bool,
    /// Last time the view requested a repaint. Used by `should_repaint()`
    /// to throttle to ~60fps.
    pub last_repaint: Instant,
    /// Set by the view when the user submits. The app layer picks this up,
    /// calls `ChatTransport::send()`, and stores the receiver in `frame_rx`.
    pub pending_send: Option<super::transport::SendRequestPayload>,
    /// Incoming frame channel from the gRPC stream. Drained by the app
    /// layer each egui frame.
    pub frame_rx: Option<mpsc::Receiver<ChatFrameEvent>>,
    /// Connection status for display.
    pub connected: bool,
}

impl Default for ChatStore {
    fn default() -> Self {
        Self {
            messages: Vec::new(),
            conversation_id: new_conversation_id(),
            submitted: false,
            last_repaint: Instant::now(),
            pending_send: None,
            frame_rx: None,
            connected: false,
        }
    }
}

impl ChatStore {
    /// Imperative append. No notification, no callback. The view will pick
    /// this up on the next frame poll.
    pub fn append_part(&mut self, msg: ChatMessage) {
        self.messages.push(msg);
    }

    /// Returns true if more than 16ms have elapsed since the last repaint
    /// request, and updates `last_repaint`. The view calls this each frame
    /// and calls `ctx.request_repaint()` when it returns true during a
    /// streaming response.
    pub fn should_repaint(&mut self) -> bool {
        let elapsed = self.last_repaint.elapsed();
        if elapsed.as_millis() >= 16 {
            self.last_repaint = Instant::now();
            true
        } else {
            false
        }
    }

    /// Reset for a new conversation. Generates a fresh conversation_id and
    /// clears all messages. Called by the "New Chat" button.
    pub fn clear(&mut self) {
        self.messages.clear();
        self.conversation_id = new_conversation_id();
        self.submitted = false;
        self.last_repaint = Instant::now();
    }

    /// Convenience: append a user text message and queue it for sending.
    pub fn append_user_text(&mut self, content: impl Into<String>) {
        let content = content.into();
        self.append_part(ChatMessage {
            id: nanoid(),
            role: "user".into(),
            kind: "text".into(),
            payload: serde_json::json!({ "content": content }),
            timestamp: now_ms(),
        });
        // Queue the send for the app layer to pick up.
        self.pending_send = Some(super::transport::SendRequestPayload {
            conversation_id: self.conversation_id.clone(),
            ui_messages: vec![serde_json::json!({
                "role": "user",
                "content": content,
            })],
            persona_id: None,
            provider: std::env::var("ZEROCLAW_LLM_PROVIDER").unwrap_or_else(|_| "local".into()),
            model: std::env::var("ZEROCLAW_LLM_MODEL").unwrap_or_else(|_| "gemma".into()),
        });
    }

    /// Convenience: append an assistant text message.
    pub fn append_assistant_text(&mut self, content: impl Into<String>) {
        let content = content.into();
        self.append_part(ChatMessage {
            id: nanoid(),
            role: "assistant".into(),
            kind: "text".into(),
            payload: serde_json::json!({ "content": content }),
            timestamp: now_ms(),
        });
    }

    /// Drain all pending frames from the gRPC stream into the store.
    /// Called by the app layer each egui frame. Returns true if any
    /// frames were drained (to trigger a repaint).
    pub fn drain_frames(&mut self) -> bool {
        let mut got_any = false;
        let mut rx_opt = self.frame_rx.take();
        if let Some(rx) = &mut rx_opt {
            while let Ok(event) = rx.try_recv() {
                got_any = true;
                match event {
                    ChatFrameEvent::Part(msg) => {
                        self.append_part(msg);
                    }
                    ChatFrameEvent::Error(msg) => {
                        self.append_part(ChatMessage {
                            id: nanoid(),
                            role: "system".into(),
                            kind: "text".into(),
                            payload: serde_json::json!({ "content": format!("Error: {msg}") }),
                            timestamp: now_ms(),
                        });
                        self.submitted = false;
                    }
                    ChatFrameEvent::Heartbeat => {
                        // Reset idle timeout; no rendering needed.
                    }
                    ChatFrameEvent::Done => {
                        self.submitted = false;
                        // Don't put the receiver back — stream is done.
                        rx_opt = None;
                        break;
                    }
                    ChatFrameEvent::ApprovalRequired {
                        tool_call_id,
                        tool_name,
                        tool_input,
                        description,
                    } => {
                        self.append_part(ChatMessage {
                            id: nanoid(),
                            role: "assistant".into(),
                            kind: "tool-call".into(),
                            payload: serde_json::json!({
                                "tool": tool_name,
                                "tool_call_id": tool_call_id,
                                "input": tool_input,
                                "description": description,
                                "needs_approval": true,
                            }),
                            timestamp: now_ms(),
                        });
                    }
                }
            }
        }
        // Put the receiver back (if still active).
        self.frame_rx = rx_opt;
        got_any
    }
}

// ------------------------------------------------------------------
// ID generation — simple, no external dep. Good enough for the client
// store; the server assigns canonical uuids.
// ------------------------------------------------------------------

fn new_conversation_id() -> String {
    format!("conv_{}", nanoid())
}

fn nanoid() -> String {
    nanoid_pub()
}

/// Public nanoid for use by app.rs when constructing messages outside the store.
pub fn nanoid_pub() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    format!("{:x}{:x}", ts, n)
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
