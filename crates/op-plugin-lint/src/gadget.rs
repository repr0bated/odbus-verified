//! Inspector-Gadget-compatible JSON introspection for element coverage.
//!
//! Default build embeds the JSON walk used by
//! `op_inspector::introspective_gadget::JsonParser` — but **recurse into nested
//! objects/arrays** (upstream `analyze_json_value` leaves `nested_schema: None`,
//! so a straight call would miss deep elements).
//!
//! With `--features inspector-gadget`, also invoke the live
//! `IntrospectiveGadget::inspect_object` and merge its top-level property names.

use anyhow::{Context, Result};
use serde_json::Value;
use std::collections::BTreeSet;

/// Flatten a JSON value into dotted element paths.
/// Arrays use `[*]` for the item shape (first element, or empty marker).
pub fn introspect_json_paths(value: &Value) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    walk(value, "", &mut out);
    out
}

fn walk(value: &Value, prefix: &str, out: &mut BTreeSet<String>) {
    match value {
        Value::Object(map) => {
            if !prefix.is_empty() {
                out.insert(prefix.to_string());
            }
            for (k, v) in map {
                let path = if prefix.is_empty() {
                    k.clone()
                } else {
                    format!("{prefix}.{k}")
                };
                out.insert(path.clone());
                walk(v, &path, out);
            }
        }
        Value::Array(items) => {
            if !prefix.is_empty() {
                out.insert(prefix.to_string());
            }
            let item_prefix = if prefix.is_empty() {
                "[*]".to_string()
            } else {
                format!("{prefix}[*]")
            };
            out.insert(item_prefix.clone());
            if let Some(first) = items.first() {
                walk(first, &item_prefix, out);
            }
        }
        _ => {
            if !prefix.is_empty() {
                out.insert(prefix.to_string());
            }
        }
    }
}

/// Parse JSON text and return discovered element paths.
pub fn introspect_json_text(text: &str) -> Result<BTreeSet<String>> {
    let value: Value = serde_json::from_str(text).context("parse instance/schema JSON")?;
    Ok(introspect_json_paths(&value))
}

/// Flatten a sealed `PluginSchema`-shaped JSON (`{ "fields": { ... } }` or the
/// ui-model wrapper `{ "schema": { "fields": ... } }`) into dotted field paths.
pub fn paths_from_sealed_schema(text: &str) -> Result<BTreeSet<String>> {
    let value: Value = serde_json::from_str(text).context("parse sealed schema JSON")?;
    let fields = value
        .pointer("/schema/fields")
        .or_else(|| value.pointer("/fields"))
        .and_then(|v| v.as_object())
        .context("sealed schema missing fields object")?;

    let mut out = BTreeSet::new();
    for (name, field) in fields {
        out.insert(name.clone());
        walk_field_schema(field, name, &mut out);
    }
    Ok(out)
}

fn walk_field_schema(field: &Value, prefix: &str, out: &mut BTreeSet<String>) {
    // FieldSchema wire: { "field_type": "string" | { "object": {..FieldSchema} } | { "array": ... } }
    let Some(ft) = field.get("field_type") else {
        return;
    };
    if let Value::Object(m) = ft {
        if let Some(obj) = m.get("object").and_then(|v| v.as_object()) {
            for (k, child) in obj {
                let path = format!("{prefix}.{k}");
                out.insert(path.clone());
                walk_field_schema(child, &path, out);
            }
        } else if let Some(inner) = m.get("array") {
            let path = format!("{prefix}[*]");
            out.insert(path.clone());
            // array item may be `{ "object": {...} }` or a nested FieldType
            if let Some(obj) = inner.get("object").and_then(|v| v.as_object()) {
                for (k, child) in obj {
                    let p = format!("{path}.{k}");
                    out.insert(p.clone());
                    walk_field_schema(child, &p, out);
                }
            } else if inner.get("field_type").is_some() {
                walk_field_schema(inner, &path, out);
            }
        }
    }
}

/// Collect declared field paths from plugin source via syn (nested structs).
pub fn declared_field_paths(source: &str) -> Result<BTreeSet<String>> {
    declared_field_paths_multi(&[source])
}

/// Same as [`declared_field_paths`], but merge struct definitions from extra
/// crates/modules (e.g. `common/llm_projection.rs` for `#[serde(flatten)]`).
pub fn declared_field_paths_multi(sources: &[&str]) -> Result<BTreeSet<String>> {
    use std::collections::HashMap;
    use syn::{parse_file, Fields, Item};

    // (field_name, type_name, flattened?)
    let mut structs: HashMap<String, Vec<(String, String, bool)>> = HashMap::new();
    let mut root_candidates: Vec<String> = Vec::new();

    for (i, source) in sources.iter().enumerate() {
        let file =
            parse_file(source).with_context(|| format!("parse source #{i} for field paths"))?;
        for item in &file.items {
            let Item::Struct(st) = item else { continue };
            if !st.attrs.iter().any(|a| {
                a.path().is_ident("derive")
                    && a.meta
                        .to_token_stream_display()
                        .to_string()
                        .contains("JsonSchema")
            }) {
                continue;
            }
            let Fields::Named(fields) = &st.fields else {
                continue;
            };
            let mut entries = Vec::new();
            for f in &fields.named {
                let Some(ident) = &f.ident else { continue };
                let flat = f.attrs.iter().any(|a| {
                    let s = a.meta.to_token_stream_display().to_string();
                    a.path().is_ident("serde") && s.contains("flatten")
                });
                entries.push((ident.to_string(), type_name(&f.ty), flat));
            }
            let name = st.ident.to_string();
            // Primary source (index 0) owns *State roots.
            if i == 0 && name.ends_with("State") {
                root_candidates.push(name.clone());
            }
            structs.insert(name, entries);
        }
    }

    let roots: Vec<String> = if root_candidates.is_empty() {
        structs
            .keys()
            .filter(|k| k.ends_with("State"))
            .cloned()
            .collect()
    } else {
        root_candidates
    };
    let roots = if roots.is_empty() {
        structs.keys().cloned().collect()
    } else {
        roots
    };

    let mut out = BTreeSet::new();
    for root in roots {
        expand(&root, "", &structs, &mut out, 0);
    }
    Ok(out)
}

fn expand(
    struct_name: &str,
    prefix: &str,
    structs: &std::collections::HashMap<String, Vec<(String, String, bool)>>,
    out: &mut BTreeSet<String>,
    depth: usize,
) {
    if depth > 8 {
        return;
    }
    let Some(fields) = structs.get(struct_name) else {
        return;
    };
    for (fname, ty, flat) in fields {
        // #[serde(flatten)] — children hoist to parent path (no fname segment).
        if *flat {
            let hoist = prefix.to_string();
            if let Some(inner) = ty.strip_prefix("Option<").and_then(|s| s.strip_suffix('>')) {
                expand(inner, &hoist, structs, out, depth + 1);
            } else if structs.contains_key(ty) {
                expand(ty, &hoist, structs, out, depth + 1);
            }
            continue;
        }

        let path = if prefix.is_empty() {
            fname.clone()
        } else {
            format!("{prefix}.{fname}")
        };
        out.insert(path.clone());

        if let Some(inner) = ty.strip_prefix("Vec<").and_then(|s| s.strip_suffix('>')) {
            let item_path = format!("{path}[*]");
            out.insert(item_path.clone());
            if structs.contains_key(inner) {
                expand(inner, &item_path, structs, out, depth + 1);
            }
        } else if let Some(inner) = ty.strip_prefix("Option<").and_then(|s| s.strip_suffix('>')) {
            if structs.contains_key(inner) {
                expand(inner, &path, structs, out, depth + 1);
            }
        } else if structs.contains_key(ty) {
            expand(ty, &path, structs, out, depth + 1);
        }
    }
}

fn type_name(ty: &syn::Type) -> String {
    match ty {
        syn::Type::Path(p) => {
            let seg = match p.path.segments.last() {
                Some(s) => s,
                None => return String::new(),
            };
            let name = seg.ident.to_string();
            match &seg.arguments {
                syn::PathArguments::AngleBracketed(args) => {
                    let inners: Vec<String> = args
                        .args
                        .iter()
                        .filter_map(|a| match a {
                            syn::GenericArgument::Type(t) => Some(type_name(t)),
                            _ => None,
                        })
                        .collect();
                    if inners.is_empty() {
                        name
                    } else {
                        format!("{name}<{}>", inners.join(","))
                    }
                }
                _ => name,
            }
        }
        _ => String::new(),
    }
}

trait MetaDisplay {
    fn to_token_stream_display(&self) -> String;
}
impl MetaDisplay for syn::Meta {
    fn to_token_stream_display(&self) -> String {
        match self {
            syn::Meta::List(l) => format!("{}({})", path_str(&l.path), l.tokens),
            syn::Meta::Path(p) => path_str(p),
            syn::Meta::NameValue(nv) => format!("{}=...", path_str(&nv.path)),
        }
    }
}
fn path_str(p: &syn::Path) -> String {
    p.segments
        .iter()
        .map(|s| s.ident.to_string())
        .collect::<Vec<_>>()
        .join("::")
}

/// Compare declared paths vs introspected paths.
pub struct CoverageDiff {
    pub missing_in_plugin: Vec<String>,
    pub extra_in_plugin: Vec<String>,
    pub introspected_count: usize,
    pub declared_count: usize,
}

pub fn diff_coverage(declared: &BTreeSet<String>, introspected: &BTreeSet<String>) -> CoverageDiff {
    let missing: Vec<String> = introspected.difference(declared).cloned().collect();
    let extra: Vec<String> = declared.difference(introspected).cloned().collect();
    CoverageDiff {
        missing_in_plugin: missing,
        extra_in_plugin: extra,
        introspected_count: introspected.len(),
        declared_count: declared.len(),
    }
}

#[cfg(feature = "inspector-gadget")]
pub async fn introspect_with_live_gadget(json_text: &str) -> Result<BTreeSet<String>> {
    use op_inspector::{InspectionInput, InspectionSource, IntrospectiveGadget, KnowledgeBase};
    use std::collections::HashMap;
    use std::sync::Arc;

    let kb = Arc::new(tokio::sync::RwLock::new(KnowledgeBase::default()));
    let gadget = IntrospectiveGadget::new(kb).await?;
    let input = InspectionInput {
        source: InspectionSource::RawData {
            format_hint: Some("json".into()),
            description: "op-plugin-lint coverage".into(),
        },
        data: Some(json_text.to_string()),
        metadata: HashMap::new(),
    };
    let result = gadget.inspect_object(input).await?;
    // Deep walk the original JSON (gadget's SchemaProperty.nested_schema is often None).
    let mut paths = introspect_json_text(json_text)?;
    for key in result.schema.properties.keys() {
        paths.insert(key.clone());
    }
    Ok(paths)
}
