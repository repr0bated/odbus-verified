//! json-render.dev declarative element spec.
//!
//! An `Element` is an immutable, versioned UI atom authored by `gemma_brain`.
//! The Rust interpreter in [`super::interpret`] is the only thing allowed to
//! materialise these into pixels — no per-element Rust, no WASM, no fallback.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Stable identifier minted by gemma_brain. Never reused.
pub type ElementId = String;

/// Monotonic per-element version. A new version means a behavioural change;
/// the previous version is still resolvable via the immutable store so views
/// pinned to an older version keep working until an operator accepts migration.
pub type ElementVersion = u32;

/// A view's binding target. Always pinned — unpinned references are a smell.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct PinnedRef {
    pub id: ElementId,
    pub version: ElementVersion,
}

/// Catalog membership classification.
///
/// `StableCore` elements are immune to gemma's mint-on-retire churn — they
/// are the primitives operators rely on (button, toggle, select, numeric
/// input, table row, status pill, log line, form field, ...). Of the 200
/// gallery slots, ~40 are reserved as stable core; the remaining ~160 are
/// gemma's novelty playground and may rotate.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Tier {
    StableCore,
    Novelty,
}

/// One catalog element. The `spec` field is the json-render.dev DSL payload
/// the interpreter walks. We keep it as `serde_json::Value` so the DSL can
/// evolve without forcing a `zeroclaw-gui` rebuild — the interpreter handles
/// unknown nodes by surfacing a `RenderError`, never by guessing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Element {
    pub id: ElementId,
    pub version: ElementVersion,
    pub tier: Tier,
    /// Human label for the gallery view. Not shown to operators by default.
    pub title: String,
    /// Optional accepted-schema fingerprint. Bindings whose schema does not
    /// match are rejected before render.
    pub accepts_schema_hash: Option<String>,
    /// json-render.dev DSL tree. Walked by [`super::interpret::render`].
    pub spec: Value,
    /// Lifecycle bookkeeping populated by `CatalogStore` (not by gemma).
    #[serde(default)]
    pub retired: bool,
}

impl Element {
    pub fn pin(&self) -> PinnedRef {
        PinnedRef { id: self.id.clone(), version: self.version }
    }
}
