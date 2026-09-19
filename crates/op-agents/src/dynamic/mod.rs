//! Dynamically assembled agents.
//!
//! Owner direction (2026-09-19): static agents are recreated dynamically, and
//! the pieces they were made of are shared across agents with similar
//! functions. A *piece* is one reusable fragment — a behaviour, a knowledge
//! block, an example, or an executor capability — tagged by domain. The
//! [`PieceLibrary`] is the shared pool; [`assemble`] composes a persona for one
//! task from the pieces that fit that task's context. Nothing here is a fixed
//! persona: `rust expert` and `D-Bus expert` are different selections from the
//! same pool, and a fact stored in memory becomes a piece every agent can use.
//!
//! Seeds for the pool:
//! - the unified static catalog (`GLOBAL_REGISTRY`): each agent's behaviour,
//!   knowledge sections, examples and capabilities become pieces;
//! - markdown agent definitions read through [`crate::generator::md_parser`];
//! - memory entries (namespace `agent_pieces`) merged at runtime by the bridge.
//!
//! The assembler is deterministic and offline: scoring is lexical over domain
//! tags and titles so it works with no vector store. When the caller already
//! has semantic context (RAG snippets, memory hits), it passes them in
//! [`TaskContext::snippets`] and they are rendered into the persona prompt.

pub mod assemble;
pub mod pieces;

pub use assemble::{assemble, AssembledPersona, TaskContext};
pub use pieces::{AgentPiece, PieceKind, PieceLibrary, PieceSource};
