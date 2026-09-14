//! Emit a full contract-shaped plugin document from a plugin `.rs` source,
//! optionally including introspect gap findings.

use crate::audit::{audit_source_with_coverage, CoverageInputs};
use crate::gadget::declared_field_paths_multi;
use crate::gaps::{gaps_from_surface_json_for_plugin, IntrospectGaps};
use crate::report::Report;
use anyhow::{Context, Result};
use regex::Regex;
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use syn::{parse_file, Attribute, Fields, Item, Lit, Meta, Type};

#[derive(Debug, Clone, Serialize)]
pub struct CompletePluginDocument {
    pub contract: &'static str,
    pub source: String,
    pub plugin: PluginEmit,
    pub audit: AuditEmit,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub introspect: Option<IntrospectEmit>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PluginEmit {
    pub name: String,
    pub category: String,
    pub version: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    pub state_root: Option<String>,
    pub fields: BTreeMap<String, FieldEmit>,
    pub nested_types: BTreeMap<String, BTreeMap<String, FieldEmit>>,
    pub methods: BTreeMap<String, MethodEmit>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FieldEmit {
    pub rust_type: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subid: Option<String>,
    pub has_serde_default: bool,
    pub optional: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub json_type: Option<&'static str>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MethodEmit {
    pub name: String,
    pub input_type: String,
    pub output_type: String,
    pub side_effect: String,
    pub idempotent: bool,
    pub required_capability: String,
    pub subid: String,
    pub args: Value,
    pub returns: Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct AuditEmit {
    pub ok: bool,
    pub fail: usize,
    pub warn: usize,
    pub hint: usize,
    pub findings: Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct IntrospectEmit {
    pub surface_kind: Option<String>,
    pub surface_source: Option<String>,
    pub files_by_kind: Option<Value>,
    pub by_kind: Option<Value>,
    pub element_path_count: usize,
    pub gaps: IntrospectGaps,
}

pub fn emit_complete_plugin(
    source_name: &str,
    source: &str,
    coverage: &CoverageInputs,
    extra_sources: &[&str],
) -> Result<CompletePluginDocument> {
    let mut sources = vec![source];
    sources.extend_from_slice(extra_sources);

    let report = audit_source_with_coverage(source_name, source, coverage)?;
    let file = parse_file(source).context("parse plugin source for emit")?;

    let consts = extract_plugin_consts(source);
    let structs = collect_structs(&file);
    let methods = extract_methods(source, &structs);

    let state_root = structs
        .keys()
        .find(|k| k.ends_with("State"))
        .cloned()
        .or_else(|| structs.keys().next().cloned());

    let mut fields = BTreeMap::new();
    if let Some(root) = &state_root {
        if let Some(fs) = structs.get(root) {
            fields = fs.clone();
        }
    }

    let mut nested = BTreeMap::new();
    for (name, fs) in &structs {
        if Some(name) == state_root.as_ref() {
            continue;
        }
        // Keep Input/Output and nested schema types used by state/methods.
        nested.insert(name.clone(), fs.clone());
    }

    let declared = declared_field_paths_multi(&sources).unwrap_or_default();
    let field_leaves: BTreeSet<String> = declared
        .iter()
        .map(|p| p.rsplit('.').next().unwrap_or(p).to_ascii_lowercase())
        .collect();
    let method_names: BTreeSet<String> = methods.keys().cloned().collect();

    let plugin_name = consts
        .get("PLUGIN_NAME")
        .cloned()
        .unwrap_or_else(|| source_name.trim_end_matches(".rs").to_string());

    let introspect = coverage.binary_surface_json.as_ref().map(|surface| {
        let v: Value = serde_json::from_str(surface).unwrap_or(json!({}));
        let gaps =
            gaps_from_surface_json_for_plugin(surface, &field_leaves, &method_names, &plugin_name)
                .unwrap_or(IntrospectGaps {
                    surface_paths_total: 0,
                    signal_paths_considered: 0,
                    covered_approx: 0,
                    missing_from_plugin: 0,
                    missing_cli_commands: vec![],
                    missing_config_fields: vec![],
                    missing_by_group: BTreeMap::new(),
                    delegated_paths: vec![],
                    missing_paths_sample: vec![],
                    note: "failed to compute gaps".into(),
                });
        IntrospectEmit {
            surface_kind: v
                .get("kind")
                .and_then(|x| x.as_str())
                .map(|s| s.to_string()),
            surface_source: v
                .get("binary")
                .and_then(|x| x.as_str())
                .map(|s| s.to_string()),
            files_by_kind: v.get("files_by_kind").cloned(),
            by_kind: v.get("by_kind").cloned(),
            element_path_count: v
                .get("element_paths")
                .and_then(|a| a.as_array())
                .map(|a| a.len())
                .unwrap_or(0),
            gaps,
        }
    });

    Ok(CompletePluginDocument {
        contract: "PLUGIN-RENDER-CONTRACT.md",
        source: source_name.to_string(),
        plugin: PluginEmit {
            name: plugin_name,
            category: consts
                .get("PLUGIN_CATEGORY")
                .cloned()
                .unwrap_or_else(|| "software".into()),
            version: consts
                .get("PLUGIN_VERSION")
                .cloned()
                .unwrap_or_else(|| "0.0.0".into()),
            description: consts
                .get("PLUGIN_DESCRIPTION")
                .cloned()
                .unwrap_or_default(),
            display_name: consts.get("PLUGIN_DISPLAY_NAME").cloned(),
            state_root,
            fields,
            nested_types: nested,
            methods,
        },
        audit: audit_emit(&report),
        introspect,
    })
}

pub fn complete_to_json(doc: &CompletePluginDocument) -> Result<String> {
    Ok(serde_json::to_string_pretty(doc)?)
}

pub fn complete_to_markdown(doc: &CompletePluginDocument) -> String {
    let mut out = String::new();
    out.push_str(&format!("# Complete plugin: {}\n\n", doc.plugin.name));
    out.push_str(&format!("**Contract:** {}\n\n", doc.contract));
    out.push_str(&format!("**Source:** `{}`\n\n", doc.source));
    out.push_str(&format!(
        "**Identity:** {} {} ({}) — {}\n\n",
        doc.plugin.name, doc.plugin.version, doc.plugin.category, doc.plugin.description
    ));
    if let Some(root) = &doc.plugin.state_root {
        out.push_str(&format!("**State root:** `{root}`\n\n"));
    }

    out.push_str("## Fields\n\n");
    out.push_str("| Field | Rust type | Subid | Doc |\n|---|---|---|---|\n");
    for (name, f) in &doc.plugin.fields {
        out.push_str(&format!(
            "| `{name}` | `{}` | {} | {} |\n",
            f.rust_type,
            f.subid.as_deref().unwrap_or("—"),
            escape_md(&f.description)
        ));
    }

    out.push_str("\n## Methods (typed)\n\n");
    for (name, m) in &doc.plugin.methods {
        out.push_str(&format!(
            "### `{name}`\n\n- side_effect: `{}`\n- idempotent: {}\n- capability: `{}`\n- subid: `{}`\n- input: `{}` → output: `{}`\n\n",
            m.side_effect, m.idempotent, m.required_capability, m.subid, m.input_type, m.output_type
        ));
        out.push_str("**args**\n\n```json\n");
        out.push_str(&serde_json::to_string_pretty(&m.args).unwrap_or_else(|_| "{}".into()));
        out.push_str("\n```\n\n**returns**\n\n```json\n");
        out.push_str(&serde_json::to_string_pretty(&m.returns).unwrap_or_else(|_| "{}".into()));
        out.push_str("\n```\n\n");
    }

    out.push_str("## Audit\n\n");
    out.push_str(&format!(
        "Status: {} (fail={} warn={} hint={})\n\n",
        if doc.audit.ok { "PASS" } else { "FAIL" },
        doc.audit.fail,
        doc.audit.warn,
        doc.audit.hint
    ));

    if let Some(intro) = &doc.introspect {
        out.push_str("## Introspect findings (gaps vs plugin)\n\n");
        out.push_str(&format!(
            "- surface: {} ({})\n- element_paths: {}\n- missing_from_plugin: **{}**\n- missing_cli_commands: **{}**\n- missing_config_fields: **{}**\n- delegated_gemini: **{}**\n\n",
            intro.surface_source.as_deref().unwrap_or("?"),
            intro.surface_kind.as_deref().unwrap_or("?"),
            intro.element_path_count,
            intro.gaps.missing_from_plugin,
            intro.gaps.missing_cli_commands.len(),
            intro.gaps.missing_config_fields.len(),
            intro.gaps.delegated_paths.len(),
        ));
        out.push_str(&format!("{}\n\n", intro.gaps.note));
        out.push_str("### CLI commands not in plugin\n\n");
        for p in intro.gaps.missing_cli_commands.iter().take(120) {
            out.push_str(&format!("- `{p}`\n"));
        }
        if intro.gaps.missing_cli_commands.len() > 120 {
            out.push_str(&format!(
                "- … +{} more\n",
                intro.gaps.missing_cli_commands.len() - 120
            ));
        }
        if !intro.gaps.delegated_paths.is_empty() {
            out.push_str("\n### Delegated Gemini-model paths (not owned by this plugin)\n\n");
            for p in intro.gaps.delegated_paths.iter().take(80) {
                out.push_str(&format!("- `{p}`\n"));
            }
            if intro.gaps.delegated_paths.len() > 80 {
                out.push_str(&format!(
                    "- … +{} more\n",
                    intro.gaps.delegated_paths.len() - 80
                ));
            }
        }
        out.push_str("\n### Config / struct fields not in plugin (sample)\n\n");
        for p in intro.gaps.missing_config_fields.iter().take(80) {
            out.push_str(&format!("- `{p}`\n"));
        }
    }

    out
}

fn escape_md(s: &str) -> String {
    s.replace('|', "\\|").replace('\n', " ")
}

fn audit_emit(report: &Report) -> AuditEmit {
    let fail = report
        .findings
        .iter()
        .filter(|f| matches!(f.severity, crate::report::Severity::Fail))
        .count();
    let warn = report
        .findings
        .iter()
        .filter(|f| matches!(f.severity, crate::report::Severity::Warn))
        .count();
    let hint = report
        .findings
        .iter()
        .filter(|f| matches!(f.severity, crate::report::Severity::Hint))
        .count();
    let findings = serde_json::to_value(&report.findings).unwrap_or(json!([]));
    AuditEmit {
        ok: report.ok(),
        fail,
        warn,
        hint,
        findings,
    }
}

fn extract_plugin_consts(source: &str) -> BTreeMap<String, String> {
    let re = Regex::new(
        r#"const\s+(PLUGIN_(?:NAME|CATEGORY|VERSION|DESCRIPTION|DISPLAY_NAME))\s*:\s*&str\s*=\s*"([^"]*)""#,
    )
    .expect("regex");
    let mut out = BTreeMap::new();
    for cap in re.captures_iter(source) {
        out.insert(cap[1].to_string(), cap[2].to_string());
    }
    out
}

fn collect_structs(file: &syn::File) -> BTreeMap<String, BTreeMap<String, FieldEmit>> {
    let mut out = BTreeMap::new();
    for item in &file.items {
        let Item::Struct(st) = item else { continue };
        if !has_jsonschema(&st.attrs) {
            continue;
        }
        let Fields::Named(fields) = &st.fields else {
            continue;
        };
        let mut map = BTreeMap::new();
        for f in &fields.named {
            let Some(ident) = &f.ident else { continue };
            let ty = type_string(&f.ty);
            let optional = ty.starts_with("Option<");
            map.insert(
                ident.to_string(),
                FieldEmit {
                    rust_type: ty.clone(),
                    description: doc_comment(&f.attrs),
                    subid: schemars_subid(&f.attrs),
                    has_serde_default: has_serde_default(&f.attrs),
                    optional,
                    json_type: rust_to_json_type(&ty),
                },
            );
        }
        out.insert(st.ident.to_string(), map);
    }
    out
}

fn extract_methods(
    source: &str,
    structs: &BTreeMap<String, BTreeMap<String, FieldEmit>>,
) -> BTreeMap<String, MethodEmit> {
    // method_decl_from_schemars_with_output::<In, Out>(
    //     "Name",
    //     SideEffect::Read|Mutation,
    //     true|false,
    //     "cap...",
    //     "subid...",
    // )
    // Allow trailing commas in turbofish:::<In, Out,>(
    let re = Regex::new(
        r#"(?s)method_decl_from_schemars_with_output\s*::\s*<\s*([A-Za-z0-9_]+)\s*,\s*([A-Za-z0-9_]+)\s*,?\s*>\s*\(\s*"([^"]+)"\s*,\s*[\w:]*SideEffect::(Read|Mutation)\s*,\s*(true|false)\s*,\s*"([^"]*)"\s*,\s*"([^"]*)""#,
    )
    .expect("regex");

    let mut out = BTreeMap::new();
    for cap in re.captures_iter(source) {
        let input_ty = cap[1].to_string();
        let output_ty = cap[2].to_string();
        let name = cap[3].to_string();
        let side = cap[4].to_ascii_lowercase();
        let idempotent = &cap[5] == "true";
        let capab = cap[6].to_string();
        let subid = cap[7].to_string();
        let args = struct_to_json_schema(&input_ty, structs);
        let returns = struct_to_json_schema(&output_ty, structs);
        out.insert(
            name.clone(),
            MethodEmit {
                name,
                input_type: input_ty,
                output_type: output_ty,
                side_effect: side,
                idempotent,
                required_capability: capab,
                subid,
                args,
                returns,
            },
        );
    }
    out
}

fn struct_to_json_schema(
    type_name: &str,
    structs: &BTreeMap<String, BTreeMap<String, FieldEmit>>,
) -> Value {
    let Some(fields) = structs.get(type_name) else {
        return json!({
            "title": type_name,
            "type": "object",
            "description": format!("type `{type_name}` not found in this .rs (may live in common/)"),
            "properties": {}
        });
    };
    let mut properties = serde_json::Map::new();
    let mut required = Vec::new();
    for (name, f) in fields {
        let mut prop = serde_json::Map::new();
        if let Some(jt) = f.json_type {
            prop.insert("type".into(), json!(jt));
        } else {
            prop.insert("type".into(), json!("object"));
            prop.insert("x-rust-type".into(), json!(f.rust_type));
        }
        if !f.description.is_empty() {
            prop.insert("description".into(), json!(f.description));
        }
        if let Some(sub) = &f.subid {
            prop.insert("x-oscal-subid".into(), json!(sub));
        }
        properties.insert(name.clone(), Value::Object(prop));
        if !f.optional && !f.has_serde_default {
            required.push(name.clone());
        }
    }
    let mut obj = serde_json::Map::new();
    obj.insert("title".into(), json!(type_name));
    obj.insert("type".into(), json!("object"));
    obj.insert("properties".into(), Value::Object(properties));
    if !required.is_empty() {
        obj.insert("required".into(), json!(required));
    }
    Value::Object(obj)
}

fn has_jsonschema(attrs: &[Attribute]) -> bool {
    attrs
        .iter()
        .any(|a| meta_str(&a.meta).contains("JsonSchema"))
}

fn meta_str(meta: &Meta) -> String {
    match meta {
        Meta::List(l) => format!(
            "{}({})",
            l.path
                .segments
                .iter()
                .map(|s| s.ident.to_string())
                .collect::<Vec<_>>()
                .join("::"),
            l.tokens
        ),
        Meta::Path(p) => p
            .segments
            .iter()
            .map(|s| s.ident.to_string())
            .collect::<Vec<_>>()
            .join("::"),
        Meta::NameValue(nv) => format!(
            "{}=...",
            nv.path
                .segments
                .iter()
                .map(|s| s.ident.to_string())
                .collect::<Vec<_>>()
                .join("::")
        ),
    }
}

fn attrs_joined(attrs: &[Attribute]) -> String {
    attrs
        .iter()
        .map(|a| meta_str(&a.meta))
        .collect::<Vec<_>>()
        .join(" ")
}

fn doc_comment(attrs: &[Attribute]) -> String {
    let mut lines = Vec::new();
    for a in attrs {
        if !a.path().is_ident("doc") {
            continue;
        }
        if let Meta::NameValue(nv) = &a.meta {
            if let syn::Expr::Lit(syn::ExprLit {
                lit: Lit::Str(s), ..
            }) = &nv.value
            {
                lines.push(s.value().trim().to_string());
            }
        }
    }
    lines.join(" ")
}

fn schemars_subid(attrs: &[Attribute]) -> Option<String> {
    let text = attrs_joined(attrs);
    // x-oscal-subid" = "mut.service...."
    let re = Regex::new(r#"x-oscal-subid"\s*=\s*"([^"]+)""#).ok()?;
    re.captures(&text)
        .and_then(|c| c.get(1).map(|m| m.as_str().to_string()))
}

fn has_serde_default(attrs: &[Attribute]) -> bool {
    let t = attrs_joined(attrs);
    t.contains("serde(default") || t.contains("default)")
}

fn type_string(ty: &Type) -> String {
    match ty {
        Type::Path(p) => {
            let segs: Vec<String> = p
                .path
                .segments
                .iter()
                .map(|s| {
                    let ident = s.ident.to_string();
                    match &s.arguments {
                        syn::PathArguments::AngleBracketed(ab) => {
                            let args: Vec<String> = ab
                                .args
                                .iter()
                                .filter_map(|a| match a {
                                    syn::GenericArgument::Type(t) => Some(type_string(t)),
                                    _ => None,
                                })
                                .collect();
                            if args.is_empty() {
                                ident
                            } else {
                                format!("{ident}<{}>", args.join(", "))
                            }
                        }
                        _ => ident,
                    }
                })
                .collect();
            segs.join("::")
        }
        Type::Reference(r) => format!("&{}", type_string(&r.elem)),
        Type::Tuple(t) => {
            let parts: Vec<_> = t.elems.iter().map(type_string).collect();
            format!("({})", parts.join(", "))
        }
        _ => "unknown".into(),
    }
}

fn rust_to_json_type(ty: &str) -> Option<&'static str> {
    let t = ty
        .strip_prefix("Option<")
        .and_then(|s| s.strip_suffix('>'))
        .unwrap_or(ty);
    match t {
        "String" | "str" | "&str" => Some("string"),
        "bool" => Some("boolean"),
        "u8" | "u16" | "u32" | "u64" | "i8" | "i16" | "i32" | "i64" | "usize" | "isize" => {
            Some("integer")
        }
        "f32" | "f64" => Some("number"),
        _ if t.starts_with("Vec<") => Some("array"),
        _ if t.starts_with("HashMap<") || t.starts_with("BTreeMap<") => Some("object"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emits_methods_and_fields_from_mini_plugin() {
        let src = r#"
            const PLUGIN_NAME: &str = "demo";
            const PLUGIN_CATEGORY: &str = "software";
            const PLUGIN_VERSION: &str = "1.0.0";
            const PLUGIN_DESCRIPTION: &str = "demo plugin";

            #[derive(schemars::JsonSchema)]
            pub struct DemoState {
                /// Status line.
                #[serde(default)]
                #[schemars(extend("x-oscal-subid" = "obs.software.demo.status@v1"))]
                pub status: String,
            }

            #[derive(schemars::JsonSchema)]
            pub struct EmptyIn {}

            #[derive(schemars::JsonSchema)]
            pub struct GetOut {
                pub status: String,
            }

            fn demo_schema() {
                schema.methods.insert(
                    "GetState".to_string(),
                    method_decl_from_schemars_with_output::<EmptyIn, GetOut>(
                        "GetState",
                        op_state_store::SideEffect::Read,
                        true,
                        "cap.software.demo.read@v1",
                        "obs.service.demo.get@v1",
                    ),
                );
            }
        "#;
        let doc = emit_complete_plugin("demo.rs", src, &CoverageInputs::default(), &[]).unwrap();
        assert_eq!(doc.plugin.name, "demo");
        assert!(doc.plugin.fields.contains_key("status"));
        assert!(doc.plugin.methods.contains_key("GetState"));
        let m = &doc.plugin.methods["GetState"];
        assert_eq!(m.side_effect, "read");
        assert_eq!(m.input_type, "EmptyIn");
        assert_eq!(m.output_type, "GetOut");
    }
}
