//! Syn-based static walk of a plugin `.rs` source against the render contract.

use crate::report::Report;
use crate::subid::{validate_subid, KNOWN_X_KEYS};
use anyhow::{Context, Result};
use syn::{parse_file, Attribute, Expr, Fields, Item, ItemStruct, Lit, Meta, MetaNameValue};

/// Optional Inspector-Gadget coverage inputs.
#[derive(Debug, Clone, Default)]
pub struct CoverageInputs {
    /// External surface JSON (binary help-walk instance paths, SDK dump, …).
    pub instance_json: Option<String>,
    /// Sealed PluginSchema JSON — only for drift checks, NOT as discovery source.
    pub sealed_schema_json: Option<String>,
    /// Extra Rust sources for nested/flattened types (e.g. llm_projection.rs).
    pub extra_rust_sources: Vec<String>,
    /// Pretty JSON of a [`crate::BinarySurface`] when `--introspect` hit a binary.
    pub binary_surface_json: Option<String>,
}

pub fn audit_source(source_name: &str, source: &str) -> Result<Report> {
    audit_source_with_coverage(source_name, source, &CoverageInputs::default())
}

pub fn audit_source_with_coverage(
    source_name: &str,
    source: &str,
    coverage: &CoverageInputs,
) -> Result<Report> {
    let mut report = Report {
        source: source_name.to_string(),
        findings: Vec::new(),
    };

    let file = parse_file(source).context("failed to parse Rust source")?;

    // Source-text scans (reliable even when Debug formatting of stmts is noisy).
    let used_typed_helper = source.contains("method_decl_from_schemars_with_output");
    let used_deprecated_helper = source_uses_deprecated_method_helper(source);
    for subid in extract_string_lits_matching_subid_shape(source) {
        if let Err(e) = validate_subid(&subid) {
            report.fail("invalid_subid", e, None);
        }
    }

    let mut has_inventory = false;
    let mut has_schema_fn = false;
    let mut has_category = false;
    let mut has_schema_seeded_test = false;
    let mut has_subid_test = false;
    let mut schemars_structs = 0usize;

    for item in &file.items {
        match item {
            Item::Macro(m) => {
                let path = m
                    .mac
                    .path
                    .segments
                    .iter()
                    .map(|s| s.ident.to_string())
                    .collect::<Vec<_>>()
                    .join("::");
                if path.contains("inventory") && path.contains("submit") {
                    has_inventory = true;
                }
            }
            Item::Fn(f) => {
                let name = f.sig.ident.to_string();
                if name.ends_with("_schema") || name == "schema" {
                    has_schema_fn = true;
                }
            }
            Item::Struct(st) => {
                if has_schemars_derive(&st.attrs) {
                    schemars_structs += 1;
                    audit_struct(&mut report, st, &mut has_category);
                }
            }
            Item::Mod(m) if m.ident == "tests" => {
                if let Some((_, items)) = &m.content {
                    for it in items {
                        if let Item::Fn(tf) = it {
                            let n = tf.sig.ident.to_string();
                            if n.contains("schema_is_schemars") || n.contains("schemars_seeded") {
                                has_schema_seeded_test = true;
                            }
                            if n.contains("subid") {
                                has_subid_test = true;
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    // inventory::submit! appears as Item::Macro with path inventory::submit
    if source.contains("inventory::submit!") {
        has_inventory = true;
    }

    if schemars_structs == 0 {
        report.fail(
            "missing_schemars_struct",
            "no #[derive(schemars::JsonSchema)] structs found — the struct IS the schema",
            None,
        );
    }

    if !has_category {
        report.fail(
            "missing_x_oscal_category",
            "no #[schemars(extend(\"x-oscal-category\" = \"…\"))] on a state struct — declare what the plugin IS",
            None,
        );
    }

    if !has_inventory {
        report.fail(
            "missing_inventory_submit",
            "missing inventory::submit! PluginReg self-registration",
            None,
        );
    }

    if !has_schema_fn {
        report.warn(
            "missing_schema_fn",
            "no `*_schema()` function spotted — expected plugin_schema_from_json wrapper",
            None,
        );
    }

    if used_deprecated_helper {
        report.fail(
            "deprecated_method_helper",
            "uses method_decl_from_schemars (deprecated) — switch to method_decl_from_schemars_with_output with a typed Output",
            None,
        );
    }

    if source.contains("AckOutput") {
        report.warn(
            "ack_output_used",
            "AckOutput is a legacy shortcut — prefer a dedicated typed Output struct for new methods",
            None,
        );
    }

    if used_typed_helper {
        // good signal; no finding
    } else if source.contains("schema.methods") || source.contains("methods.insert") {
        report.warn(
            "untyped_or_missing_method_helper",
            "methods.insert present but method_decl_from_schemars_with_output not found",
            None,
        );
    }

    if !has_schema_seeded_test {
        report.warn(
            "missing_schema_seeded_test",
            "add #[test] fn schema_is_schemars_seeded_and_typed (see antigravity_chat.rs)",
            None,
        );
    }
    if !has_subid_test {
        report.warn(
            "missing_subid_validity_test",
            "add #[test] fn all_subids_are_valid (see antigravity_chat.rs)",
            None,
        );
    }

    // Unknown x-* keys (typo trap: x-oscal-subld)
    for key in extract_x_keys(source) {
        if !KNOWN_X_KEYS.contains(&key.as_str()) && key.starts_with("x-oscal") {
            report.fail(
                "unknown_x_oscal_key",
                format!(
                    "unknown extension key `{key}` — did you mean one of: {}?",
                    KNOWN_X_KEYS.join(", ")
                ),
                None,
            );
        }
    }

    apply_gadget_coverage(&mut report, source, coverage)?;

    Ok(report)
}

fn apply_gadget_coverage(
    report: &mut Report,
    source: &str,
    coverage: &CoverageInputs,
) -> Result<()> {
    use crate::gadget::{
        declared_field_paths_multi, diff_coverage, introspect_json_text, paths_from_sealed_schema,
    };
    use std::collections::BTreeSet;

    if coverage.instance_json.is_none() && coverage.sealed_schema_json.is_none() {
        return Ok(());
    }

    let mut sources: Vec<&str> = vec![source];
    for extra in &coverage.extra_rust_sources {
        sources.push(extra.as_str());
    }
    let declared = declared_field_paths_multi(&sources)?;
    let mut introspected = BTreeSet::new();

    let from_external_surface = coverage.binary_surface_json.is_some();
    // Cap per-path WARNs so 10k+ element catalogs stay readable; full set is in --surface-out.
    const SURFACE_WARN_CAP: usize = 40;

    if let Some(surface) = &coverage.binary_surface_json {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(surface) {
            let n_nodes = v.get("nodes").and_then(|n| n.as_array()).map(|a| a.len());
            let n_el = v
                .get("element_paths")
                .and_then(|n| n.as_array())
                .map(|a| a.len());
            let kind = v.get("kind").and_then(|k| k.as_str()).unwrap_or("cli");
            let label = if kind == "repomix" {
                "external repomix source pack (universal)"
            } else {
                "external CLI help-walk"
            };
            report.hint(
                "external_surface",
                format!(
                    "{label}: source={} version={} nodes={} elements={} — see --surface-out for full catalog",
                    v.get("binary").and_then(|b| b.as_str()).unwrap_or("?"),
                    v.get("version")
                        .and_then(|b| b.as_str())
                        .unwrap_or("unknown"),
                    n_nodes.unwrap_or(0),
                    n_el.unwrap_or(0)
                ),
                None,
            );
            if let Some(arr) = v.get("element_paths").and_then(|a| a.as_array()) {
                for p in arr {
                    if let Some(s) = p.as_str() {
                        introspected.insert(s.to_string());
                    }
                }
            }
        }
    } else if let Some(instance) = &coverage.instance_json {
        let paths = introspect_json_text(instance)?;
        report.hint(
            "gadget_instance_elements",
            format!(
                "external introspect discovered {} element path(s)",
                paths.len()
            ),
            None,
        );
        introspected.extend(paths);
    }

    if let Some(sealed) = &coverage.sealed_schema_json {
        let paths = paths_from_sealed_schema(sealed)?;
        report.hint(
            "gadget_sealed_elements",
            format!(
                "sealed schema contributed {} field path(s) for drift check",
                paths.len()
            ),
            None,
        );
        introspected.extend(paths);
    }

    let diff = diff_coverage(&declared, &introspected);
    report.hint(
        "gadget_coverage_summary",
        format!(
            "declared={} introspected={} missing_in_plugin={} extra_in_plugin={}",
            diff.declared_count,
            diff.introspected_count,
            diff.missing_in_plugin.len(),
            diff.extra_in_plugin.len()
        ),
        None,
    );

    let mut surface_warns = 0usize;
    let mut surface_warns_omitted = 0usize;
    for path in &diff.missing_in_plugin {
        if is_auto_accountability_field(path) {
            continue;
        }
        // External vocab (struct.*/enum.*/cmd.*) ≠ schemars fields until mapped.
        if from_external_surface {
            if surface_warns < SURFACE_WARN_CAP {
                report.warn(
                    "surface_element_not_in_plugin",
                    format!(
                        "external surface has `{path}` — not mirrored as a schemars field (expected until mapped)"
                    ),
                    Some(path.clone()),
                );
                surface_warns += 1;
            } else {
                surface_warns_omitted += 1;
            }
        } else {
            report.fail(
                "gadget_missing_element",
                format!(
                    "introspected element `{path}` is not declared on a schemars state/nested struct — add the field (or nested type) so the UI can render it"
                ),
                Some(path.clone()),
            );
        }
    }
    if surface_warns_omitted > 0 {
        report.hint(
            "surface_warn_cap",
            format!(
                "omitted {surface_warns_omitted} further surface_element_not_in_plugin WARNs (cap={SURFACE_WARN_CAP}); full catalog in --surface-out"
            ),
            None,
        );
    }

    for path in &diff.extra_in_plugin {
        if is_auto_accountability_field(path) {
            continue;
        }
        if from_external_surface {
            // Plugin field names won't match external struct.*/cmd.* paths — skip noise.
            continue;
        }
        report.warn(
            "gadget_undeclared_in_instance",
            format!(
                "plugin declares `{path}` but introspection did not see it — check sample completeness"
            ),
            Some(path.clone()),
        );
    }

    Ok(())
}

fn is_auto_accountability_field(path: &str) -> bool {
    matches!(
        path,
        "actor_id"
            | "capability_id"
            | "source_system"
            | "source_locator"
            | "event_id"
            | "event_hash"
    )
}

fn audit_struct(report: &mut Report, st: &ItemStruct, has_category: &mut bool) {
    let name = st.ident.to_string();
    let loc = format!("struct {name}");

    let attrs_text = attrs_to_string(&st.attrs);
    if attrs_text.contains("x-oscal-category") {
        *has_category = true;
    }
    if !attrs_text.contains("x-oscal-subid") {
        // Nested helpers often have subids; top-level state should too.
        // Only FAIL for public structs that look like state (name ends with State)
        // or WARN otherwise.
        if name.ends_with("State") {
            report.fail(
                "missing_struct_subid",
                "state struct missing #[schemars(extend(\"x-oscal-subid\" = \"sch.…@v1\"))]",
                Some(loc.clone()),
            );
        } else {
            report.warn(
                "missing_struct_subid",
                "schemars struct missing x-oscal-subid — nested schemas need it for UI audit binding",
                Some(loc.clone()),
            );
        }
    } else {
        for subid in extract_extend_string_values(&st.attrs, "x-oscal-subid") {
            if let Err(e) = validate_subid(&subid) {
                report.fail("invalid_subid", e, Some(loc.clone()));
            }
        }
    }

    if !has_doc_comment(&st.attrs) {
        report.warn(
            "missing_struct_doc",
            "struct has no /// doc comment — renders as blank section description",
            Some(loc.clone()),
        );
    }

    let Fields::Named(fields) = &st.fields else {
        return;
    };

    for field in &fields.named {
        let Some(ident) = &field.ident else { continue };
        let fname = ident.to_string();
        let floc = format!("{name}.{fname}");
        let fattrs = attrs_to_string(&field.attrs);

        if !fattrs.contains("x-oscal-subid") {
            report.fail(
                "missing_field_subid",
                format!("field `{fname}` missing x-oscal-subid"),
                Some(floc.clone()),
            );
        } else {
            for subid in extract_extend_string_values(&field.attrs, "x-oscal-subid") {
                if let Err(e) = validate_subid(&subid) {
                    report.fail("invalid_subid", e, Some(floc.clone()));
                }
            }
        }

        if !has_doc_comment(&field.attrs) {
            report.warn(
                "missing_field_doc",
                format!("field `{fname}` has no /// doc — UI label/tooltip will be empty"),
                Some(floc.clone()),
            );
        }

        let has_serde_default =
            fattrs.contains("serde(default") || fattrs.contains("serde(default)");
        // syn pretty may normalize differently; also check attribute path
        let serde_default = field.attrs.iter().any(|a| {
            let s = attr_tokens(a);
            s.contains("default") && (s.contains("serde") || a.path().is_ident("serde"))
        });
        if !serde_default && !has_serde_default {
            // Option fields often omit it; still hint
            let ty = type_string(&field.ty);
            if !ty.starts_with("Option") {
                report.hint(
                    "missing_serde_default",
                    format!("consider #[serde(default)] on `{fname}` so defaults round-trip"),
                    Some(floc.clone()),
                );
            }
        }

        // Enhancement: port-like integer without range
        let ty = type_string(&field.ty);
        let looks_port = fname.contains("port") || fname.ends_with("_port");
        let is_int = matches!(
            ty.as_str(),
            "u16" | "u32" | "i32" | "i64" | "u64" | "usize" | "isize"
        );
        if looks_port && is_int && !fattrs.contains("range") {
            report.hint(
                "suggest_port_range",
                format!(
                    "field `{fname}` looks like a port — add #[schemars(range(min = 1, max = 65535))]"
                ),
                Some(floc.clone()),
            );
        }

        // Enhancement: obs.* fields often should be readOnly
        let field_subids = extract_extend_string_values(&field.attrs, "x-oscal-subid");
        if field_subids.iter().any(|s| s.starts_with("obs.")) && !fattrs.contains("readOnly") {
            report.hint(
                "suggest_readonly_obs",
                format!(
                    "`{fname}` is obs.* — consider #[schemars(extend(\"readOnly\" = true))] if display-only"
                ),
                Some(floc.clone()),
            );
        }
    }
}

fn source_uses_deprecated_method_helper(source: &str) -> bool {
    // Match `method_decl_from_schemars` but not `…_with_output`.
    let mut rest = source;
    while let Some(idx) = rest.find("method_decl_from_schemars") {
        let after = &rest[idx + "method_decl_from_schemars".len()..];
        if after.starts_with("_with_output") {
            rest = after;
            continue;
        }
        return true;
    }
    false
}

fn has_schemars_derive(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|a| {
        if !a.path().is_ident("derive") {
            return false;
        }
        attr_tokens(a).contains("JsonSchema")
    })
}

fn has_doc_comment(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|a| a.path().is_ident("doc"))
}

fn attrs_to_string(attrs: &[Attribute]) -> String {
    attrs.iter().map(attr_tokens).collect::<Vec<_>>().join(" ")
}

fn attr_tokens(attr: &Attribute) -> String {
    attr.to_token_stream_string()
}

trait AttrExt {
    fn to_token_stream_string(&self) -> String;
}

impl AttrExt for Attribute {
    fn to_token_stream_string(&self) -> String {
        // Avoid quote dependency: use Debug-ish via parse tree stringification
        self.meta.to_token_stream_display().to_string()
    }
}

trait MetaDisplay {
    fn to_token_stream_display(&self) -> String;
}

impl MetaDisplay for Meta {
    fn to_token_stream_display(&self) -> String {
        match self {
            Meta::Path(p) => p
                .segments
                .iter()
                .map(|s| s.ident.to_string())
                .collect::<Vec<_>>()
                .join("::"),
            Meta::List(l) => {
                let path = l
                    .path
                    .segments
                    .iter()
                    .map(|s| s.ident.to_string())
                    .collect::<Vec<_>>()
                    .join("::");
                format!("{path}({})", l.tokens)
            }
            Meta::NameValue(MetaNameValue { path, value, .. }) => {
                let p = path
                    .segments
                    .iter()
                    .map(|s| s.ident.to_string())
                    .collect::<Vec<_>>()
                    .join("::");
                format!("{p} = {}", expr_string(value))
            }
        }
    }
}

fn expr_string(expr: &Expr) -> String {
    match expr {
        Expr::Lit(l) => match &l.lit {
            Lit::Str(s) => format!("{:?}", s.value()),
            Lit::Int(i) => i.to_string(),
            Lit::Bool(b) => b.value.to_string(),
            _ => format!("{expr:?}"),
        },
        _ => format!("{expr:?}"),
    }
}

fn extract_extend_string_values(attrs: &[Attribute], key: &str) -> Vec<String> {
    let mut out = Vec::new();
    let needle = format!("\"{key}\"");
    for a in attrs {
        let s = attr_tokens(a);
        if !s.contains(&needle) && !s.contains(key) {
            continue;
        }
        // crude: find key = "value" patterns inside extend(...)
        out.extend(pull_assigned_strings(&s, key));
    }
    out
}

fn pull_assigned_strings(s: &str, key: &str) -> Vec<String> {
    let mut out = Vec::new();
    // Prefer the canonical schemars extend form: "x-oscal-…" = "value"
    let pat = format!("\"{key}\" = \"");
    let mut rest = s;
    while let Some(idx) = rest.find(&pat) {
        let after = &rest[idx + pat.len()..];
        if let Some(end) = after.find('"') {
            out.push(after[..end].to_string());
            rest = &after[end + 1..];
        } else {
            break;
        }
    }
    out.sort();
    out.dedup();
    out
}

fn extract_x_keys(source: &str) -> Vec<String> {
    let mut keys = Vec::new();
    let mut rest = source;
    while let Some(idx) = rest.find("\"x-") {
        let after = &rest[idx + 1..];
        if let Some(end) = after.find('"') {
            let key = &after[..end];
            if key.starts_with("x-") {
                keys.push(key.to_string());
            }
            rest = &after[end + 1..];
        } else {
            break;
        }
    }
    keys.sort();
    keys.dedup();
    keys
}

fn extract_string_lits_matching_subid_shape(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = body;
    while let Some(idx) = rest.find('"') {
        let after = &rest[idx + 1..];
        if let Some(end) = after.find('"') {
            let lit = &after[..end];
            // Must start with a taxonomy category — avoids URLs, versions, paths.
            if looks_like_subid(lit) {
                out.push(lit.to_string());
            }
            rest = &after[end + 1..];
        } else {
            break;
        }
    }
    out.sort();
    out.dedup();
    out
}

fn looks_like_subid(lit: &str) -> bool {
    const CATS: &[&str] = &["src.", "prj.", "sch.", "mut.", "obs.", "evt.", "exp."];
    if !CATS.iter().any(|c| lit.starts_with(c)) {
        return false;
    }
    // category.component.subject.verb[+…] — at least 3 dots
    lit.matches('.').count() >= 3 && !lit.contains("://") && !lit.contains('/')
}

fn type_string(ty: &syn::Type) -> String {
    match ty {
        syn::Type::Path(p) => p
            .path
            .segments
            .last()
            .map(|s| s.ident.to_string())
            .unwrap_or_default(),
        _ => String::new(),
    }
}
