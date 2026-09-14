//! json-render.dev DSL → egui interpreter.
//!
//! This is the **only** code path allowed to draw widgets in the operator
//! console. Every view binds via [`render`] with a `PinnedRef` resolved from
//! the live [`CatalogSnapshot`]. Unknown ids, missing versions, or unknown
//! DSL nodes surface as [`RenderError`] — no silent fallbacks, no raw JSON.
//!
//! The seed primitive set below is the minimum needed for gemma_brain to
//! generate against. Adding a new primitive is a deliberate act: append a
//! match arm here and document the DSL key in the json-render.dev catalog
//! contract. Never extend by accepting unknown node shapes.

use egui::{RichText, Ui};
use serde_json::Value;
use thiserror::Error as ThisError;

use super::dsl::PinnedRef;
use super::store::CatalogSnapshot;
use crate::theme::*;

#[derive(Debug, ThisError)]
pub enum RenderError {
    #[error("catalog element {id}@{version} not found")]
    UnknownElement { id: String, version: u32 },
    #[error("catalog spec missing or malformed `kind` field")]
    MalformedSpec,
    #[error("unknown DSL node kind `{0}` — not in json-render.dev catalog")]
    UnknownKind(String),
    #[error("schema/value mismatch for element {0}")]
    SchemaMismatch(String),
}

/// Render a bound element. `value` is the live JSON payload (typically from
/// `grpc::decode_to_json`); `snapshot` is the current `CatalogStore` view.
pub fn render(
    ui: &mut Ui,
    snapshot: &CatalogSnapshot,
    pin: &PinnedRef,
    value: &Value,
) -> Result<(), RenderError> {
    let element = snapshot.resolve(pin).ok_or_else(|| RenderError::UnknownElement {
        id: pin.id.clone(),
        version: pin.version,
    })?;
    walk(ui, &element.spec, value)
}

fn walk(ui: &mut Ui, spec: &Value, value: &Value) -> Result<(), RenderError> {
    let kind = spec.get("kind").and_then(Value::as_str).ok_or(RenderError::MalformedSpec)?;
    match kind {
        // ---------- layout primitives ----------
        "stack" => {
            let dir = spec.get("dir").and_then(Value::as_str).unwrap_or("v");
            let children = spec.get("children").and_then(Value::as_array).cloned().unwrap_or_default();
            let render_children = |ui: &mut Ui| -> Result<(), RenderError> {
                for child in &children { walk(ui, child, value)?; }
                Ok(())
            };
            let mut out = Ok(());
            if dir == "h" {
                ui.horizontal(|ui| out = render_children(ui));
            } else {
                ui.vertical(|ui| out = render_children(ui));
            }
            out
        }

        // ---------- text ----------
        "label" => {
            let bind = spec.get("bind").and_then(Value::as_str);
            let text = match bind {
                Some(path) => json_pointer(value, path)
                    .and_then(|v| v.as_str().map(|s| s.to_string()))
                    .unwrap_or_default(),
                None => spec.get("text").and_then(Value::as_str).unwrap_or("").to_string(),
            };
            ui.label(RichText::new(text).color(FG).size(12.0));
            Ok(())
        }

        // ---------- status pill (stable-core) ----------
        "status_pill" => {
            let bind = spec.get("bind").and_then(Value::as_str).ok_or(RenderError::MalformedSpec)?;
            let v = json_pointer(value, bind).cloned().unwrap_or(Value::Null);
            let (label, color) = match v.as_str().unwrap_or("") {
                "ok" | "up" | "healthy"   => ("OK", OK),
                "warn" | "degraded"       => ("WARN", WARN),
                "err" | "down" | "failed" => ("ERR", DANGER),
                other if !other.is_empty() => (other, MUTED),
                _                          => ("—", MUTED),
            };
            ui.label(RichText::new(label).strong().monospace().size(11.0).color(color));
            Ok(())
        }

        // ---------- action button (stable-core) ----------
        "button" => {
            let label = spec.get("label").and_then(Value::as_str).unwrap_or("action");
            // Mutation dispatch lands when InvokeHandle integration is wired.
            let _ = ui.button(label);
            Ok(())
        }

        "repeat" => {
            let bind = spec
                .get("bind")
                .and_then(Value::as_str)
                .ok_or(RenderError::MalformedSpec)?;
            let child = spec.get("child").ok_or(RenderError::MalformedSpec)?;
            let items = json_pointer(value, bind)
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            for item in &items {
                walk(ui, child, item)?;
            }
            Ok(())
        }

        other => Err(RenderError::UnknownKind(other.to_string())),
}
}

/// Minimal `/foo/bar/0` JSON pointer lookup used by `bind`.
fn json_pointer<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    if path.is_empty() || path == "/" { return Some(value); }
    let normalized = if path.starts_with('/') { path.to_string() } else { format!("/{path}") };
    value.pointer(&normalized)
}
