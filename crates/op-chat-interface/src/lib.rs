//! Identity-bound chat interface backend.
//!
//! Owner mapping (2026-11-09): everything server-side is Rust in this
//! crate; only the chat browser interface is TypeScript/Svelte, delivered
//! last (spec task 15) against the backend contract defined here.
//!
//! Spec: `.kiro/specs/identity-chat-interface/` (requirements-first).
//! Records: that spec's `records/` directory (baseline, runtime, blockers).
//!
//! Core rules carried from the spec:
//! - All op-dbus access is direct local file I/O on the identity's mounted
//!   3tchedFS views (a runtime mirror of SHM), never a subprocess CLI call
//!   and never another identity's mount.
//! - Credentials are obtained and used behind the FUSE operation surface.
//!   Raw credential material never enters this crate (Secret_Guard, task 3).
//! - The deployment container is still in development; live integration
//!   is the blocked task-19 group. Development uses synthetic fixtures
//!   and an in-process harness (tasks 10.3, 14.6).
//!
//! Tool_Registry is RETIRED (owner, 2026-11-09): there is no tool
//! registry and no registry pattern in this crate. A tool IS a plug-in —
//! when the identity-chat needs tools, each is implemented as a
//! one-file plugin (one .rs + one schema JSON, `*State` contract,
//! per-method `*Input`/`*Output` pairs, `dispatch_*_method`, `*_schema()`,
//! fail-closed), never as a registry, registry table or registry lookup.
//! `ToolKey` below names a plugin method (`plugin/method`); it is a
//! plugin identifier, not a registry key. The `registry` module and the
//! `RegistryEntry`/`RegistrySnapshot` types were removed with the
//! retirement; the credential registry in `guard` (Secret_Guard needles)
//! is a different component and stays.

pub mod errors;
pub mod gate;
pub mod guard;
pub mod profile;
pub mod state;
pub mod status;
pub mod types;
pub mod chain;
pub mod validation;

pub use errors::{ErrorKind, TypedError};
