//! Catalog subsystem — the json-render.dev pipeline.
//!
//! Single source of truth for renderable UI: a 200-element rolling gallery
//! authored by `gemma_brain` and streamed over the existing tonic-web channel
//! (`CatalogService`). Anything not in the catalog is **not renderable** —
//! binding to an unknown id surfaces as a hard error, never a silent fallback.
//!
//! Submodules:
//! - [`dsl`]        — json-render.dev declarative element spec types.
//! - [`store`]      — versioned, immutable element store with stable-core
//!                    protection and last-known-good cache.
//! - [`client`]     — tonic-web `CatalogService` stream consumer (gemma_brain
//!                    is the sole writer; this end is read-only).
//! - [`interpret`]  — Rust DSL → egui interpreter. The ONLY renderer allowed
//!                    to draw widgets in the operator console.

pub mod client;
pub mod dsl;
pub mod interpret;
pub mod store;

pub use dsl::{Element, ElementId, ElementVersion, PinnedRef};
pub use interpret::{render, RenderError};
pub use store::{CatalogStore, CatalogSnapshot};
