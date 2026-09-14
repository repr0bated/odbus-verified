//! Chat module — Antigravity IDE interface for the delegator chatbot.
//!
//! The chatbot is a DELEGATOR, not an executor. It plans, suggests,
//! coordinates, orchestrates — always delegates to agents. Forced tool
//! calling: even user responses are tool calls. Conversations are threaded
//! (conversation_id groups episodes). Chat persistence is handled by the
//! memory loop + cognitive-mcp (NOT by the client — the client just
//! displays).
//!
//! Repaint strategy: `ctx.request_repaint()` on a throttle (16ms ≈ 60fps
//! cap), not per-token.
//!
//! Submodules:
//! - [`store`]     — append-only conversation store with imperative writes.
//! - [`view`]      — egui rendering: transcript, message bubbles, tool cards,
//!                   composer, thinking indicator.
//! - [`transport`] — gRPC transport for ChatService streaming.

pub mod store;
pub mod transport;
pub mod view;

pub use store::{ChatMessage, ChatStore};
pub use transport::{ChatFrameEvent, ChatTransport, SendRequestPayload};
