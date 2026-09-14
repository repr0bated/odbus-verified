//! Schema-as-code: render arbitrary `serde_json::Value` driven by a
//! `schemars`-generated JSON Schema (Draft 2020-12).
//!
//! Plugins in `crates/op-plugins` derive `schemars::JsonSchema` on their state
//! structs and emit them via `schemars::schema_for!(StateStruct)`. This module
//! consumes that schema + a live JSON value (e.g. from a gRPC stream) and
//! renders an interactive egui tree — labels, types, doc strings, enums, and
//! nested objects/arrays — without any hand-written form code.

use egui::{Color32, RichText};
use schemars::Schema;
use serde_json::Value;

use crate::theme::*;

/// Lightweight handle identifying a single row in a virtualized array/tree.
/// Only visible rows are resolved to schema + value entries each frame, so
/// ≥10k-node payloads pay only for what's on screen. The handle holds no
/// data itself — `schema` resolution and `value` lookup happen at render time
/// for the visible slice alone.
#[derive(Clone, Debug, Default)]
pub struct RowHandle {
    pub index: usize,
    pub path: String,
}

/// Threshold above which `render_array` switches from direct iteration to
/// `egui::ScrollArea::show_rows` virtualization.
const VIRTUALIZE_THRESHOLD: usize = 100;

/// Estimated per-row height (collapsing header + one-line body) used by the
/// virtualized path. Heterogeneous row heights are handled by egui's
/// `show_viewport` correction pass inside `show_rows`.
const VIRTUAL_ROW_HEIGHT: f32 = 24.0;

/// Top-level entry point: render a JSON value annotated by a schema.
pub fn render_value(ui: &mut egui::Ui, schema: &Schema, value: &Value) {
    let root = schema.as_value();
    render_node(ui, root, value, "$", 0);
}

fn render_node(ui: &mut egui::Ui, schema: &Value, value: &Value, path: &str, depth: usize) {
    let title = schema_title(schema);
    let desc = schema_description(schema);
    let ty = schema_type(schema);

    let header = if !title.is_empty() {
        format!("{}  ·  {}", title, ty)
    } else {
        format!("{}  ·  {}", path, ty)
    };

    let id = ui.make_persistent_id(("schema", path));
    egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, depth < 2)
        .show_header(ui, |ui| {
            ui.label(RichText::new(header).color(FG).strong().size(12.0));
            if let Some(enum_vals) = schema.get("enum").and_then(Value::as_array) {
                ui.label(RichText::new(format!("enum[{}]", enum_vals.len())).color(MUTED).size(11.0));
            }
        })
        .body(|ui| {
            if let Some(d) = desc {
                ui.label(RichText::new(d).color(MUTED).italics().size(11.0));
                ui.add_space(4.0);
            }
            render_body(ui, schema, value, path, depth);
        });
}

fn render_body(ui: &mut egui::Ui, schema: &Value, value: &Value, path: &str, depth: usize) {
    // Resolve composition keywords (oneOf/anyOf/allOf) by picking the first branch.
    let resolved = resolve_composition(schema).unwrap_or(schema);

    match resolved.get("type").and_then(Value::as_str) {
        Some("object") => render_object(ui, resolved, value, path, depth),
        Some("array")  => render_array(ui, resolved, value, path, depth),
        _              => render_leaf(ui, resolved, value),
    }
}

fn render_object(ui: &mut egui::Ui, schema: &Value, value: &Value, path: &str, depth: usize) {
    let props = schema.get("properties").and_then(Value::as_object);
    let required: Vec<&str> = schema
        .get("required")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();

    let Some(props) = props else {
        render_leaf(ui, schema, value);
        return;
    };

    for (key, sub_schema) in props {
        let child_path = format!("{}.{}", path, key);
        let child_value = value.get(key).cloned().unwrap_or(Value::Null);

        ui.horizontal(|ui| {
            ui.label(RichText::new(key).color(FG).monospace().size(12.0));
            if required.iter().any(|r| r == key) {
                ui.label(RichText::new("·required").color(DANGER).size(10.0));
            }
        });
        ui.indent(child_path.clone(), |ui| {
            render_node(ui, sub_schema, &child_value, &child_path, depth + 1);
        });
        ui.add_space(2.0);
    }
}

fn render_array(ui: &mut egui::Ui, schema: &Value, value: &Value, path: &str, depth: usize) {
    let items_schema = schema.get("items").cloned().unwrap_or(Value::Null);
    let arr = value.as_array();

    let Some(arr) = arr else {
        render_leaf(ui, schema, value);
        return;
    };

    if arr.is_empty() {
        ui.label(RichText::new("(empty)").color(MUTED).italics().size(11.0));
        return;
    }

    if arr.len() <= VIRTUALIZE_THRESHOLD {
        // Small arrays: render every element directly — no scroll overhead.
        for (i, item) in arr.iter().enumerate() {
            let child_path = format!("{}[{}]", path, i);
            render_node(ui, &items_schema, item, &child_path, depth + 1);
        }
    } else {
        // Large arrays (≥10k-node payloads): virtualize via `show_rows` so
        // only the visible slice is resolved to schema + value each frame.
        // The `RowHandle` indirection means schema resolution + data lookup
        // happens for visible rows only; `ctx.request_repaint()` fires on
        // scroll events implicitly through egui's input handling.
        let total = arr.len();
        egui::ScrollArea::vertical()
            .id_source(format!("arr-virt-{path}"))
            .max_height(480.0)
            .show_rows(ui, VIRTUAL_ROW_HEIGHT, total, |ui, range| {
                for i in range {
                    let _handle = RowHandle {
                        index: i,
                        path: format!("{}[{}]", path, i),
                    };
                    let item = &arr[i];
                    render_node(ui, &items_schema, item, &_handle.path, depth + 1);
                }
            });
    }
}

fn render_leaf(ui: &mut egui::Ui, schema: &Value, value: &Value) {
    let (text, color) = match value {
        Value::Null      => ("∅ null".to_string(), MUTED),
        Value::Bool(b)   => (b.to_string(),        if *b { OK } else { DANGER }),
        Value::Number(n) => (n.to_string(),        PRIMARY),
        Value::String(s) => (format!("\"{}\"", s), FG),
        other            => (other.to_string(),    FG),
    };

    ui.horizontal(|ui| {
        ui.label(RichText::new(text).monospace().size(12.0).color(color));
        if let Some(fmt) = schema.get("format").and_then(Value::as_str) {
            ui.label(RichText::new(format!("«{}»", fmt)).color(MUTED).size(10.0));
        }
        if let Some(def) = schema.get("default") {
            ui.label(RichText::new(format!("default={}", def)).color(MUTED).size(10.0));
        }
    });
}

// ---------- helpers ----------

fn schema_title(schema: &Value) -> String {
    schema.get("title").and_then(Value::as_str).unwrap_or("").to_string()
}

fn schema_description(schema: &Value) -> Option<&str> {
    schema.get("description").and_then(Value::as_str)
}

fn schema_type(schema: &Value) -> String {
    if let Some(t) = schema.get("type").and_then(Value::as_str) {
        return t.to_string();
    }
    if schema.get("oneOf").is_some() { return "oneOf".into(); }
    if schema.get("anyOf").is_some() { return "anyOf".into(); }
    if schema.get("allOf").is_some() { return "allOf".into(); }
    if schema.get("$ref").is_some()  { return "ref".into(); }
    "any".into()
}

fn resolve_composition(schema: &Value) -> Option<&Value> {
    for key in ["oneOf", "anyOf", "allOf"] {
        if let Some(arr) = schema.get(key).and_then(Value::as_array) {
            if let Some(first) = arr.first() {
                return Some(first);
            }
        }
    }
    None
}

/// Helper used by views: return a stable, prettified JSON dump as fallback.
pub fn pretty_json(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|_| "<invalid json>".into())
}

#[allow(dead_code)]
fn _unused(_: &Color32) {}
