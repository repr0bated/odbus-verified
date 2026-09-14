//! Reviewable Rust augmentation emitted from Inspector Gadget + Repomix gaps.
//!
//! The original plugin is preserved verbatim. Inferred declarations are kept
//! in a dedicated module because Repomix proves that a surface exists, but it
//! cannot prove ownership, mutation semantics, defaults, or runtime dispatch.

use crate::emit::CompletePluginDocument;
use anyhow::{bail, Result};
use std::collections::BTreeSet;

pub fn emit_inspector_rust(source: &str, doc: &CompletePluginDocument) -> Result<String> {
    let intro = doc
        .introspect
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("--format rust requires --introspect REPOMIX_XML"))?;

    let fields = candidate_fields(
        &intro.gaps.missing_config_fields,
        &intro.gaps.missing_cli_commands,
    );
    let methods = candidate_methods(&intro.gaps.missing_cli_commands);
    if fields.is_empty() && methods.is_empty() {
        bail!("Inspector Gadget found no actionable config-field or command gaps");
    }

    let plugin = rust_ident(&doc.plugin.name);
    let plugin_slug = subid_slug(&plugin);
    let mut out = source.trim_end().to_string();
    out.push_str(
        "\n\n// ── Inspector Gadget + Repomix generated candidates ───────────────────────\n",
    );
    out.push_str("// Generated against PLUGIN-RENDER-CONTRACT.md. The original plugin above is\n");
    out.push_str("// preserved. Review ownership, concrete types, defaults, side effects, and\n");
    out.push_str(
        "// runtime dispatch before flattening these candidates into the live state/schema.\n",
    );
    out.push_str("#[allow(dead_code)]\nmod inspector_gadget_generated {\n");
    out.push_str("    use serde::{Deserialize, Serialize};\n\n");

    out.push_str("    /// Repomix-discovered fields not represented by the input plugin.\n");
    out.push_str(
        "    #[derive(Debug, Clone, Serialize, Deserialize, Default, schemars::JsonSchema)]\n",
    );
    out.push_str(&format!(
        "    #[schemars(extend(\"x-oscal-subid\" = \"sch.software.{plugin_slug}.inspector-candidates.schema@v1\"))]\n"
    ));
    out.push_str("    pub struct InspectorGadgetFields {\n");
    for (name, path) in &fields {
        let ty = infer_type(path);
        let subid_name = subid_slug(name);
        out.push_str(&format!(
            "        /// Discovered from Repomix path `{}`. Review before promotion.\n",
            clean_doc(path)
        ));
        out.push_str("        #[serde(default)]\n");
        out.push_str(&format!(
            "        #[schemars(extend(\"x-oscal-subid\" = \"obs.software.{plugin_slug}.{subid_name}@v1\"))]\n"
        ));
        out.push_str(&format!("        pub {name}: Option<{ty}>,\n\n"));
    }
    out.push_str("    }\n\n");

    out.push_str(
        "    /// Metadata needed when promoting a generated typed method into `schema.methods`.\n",
    );
    out.push_str("    pub struct MethodCandidate {\n");
    out.push_str("        pub name: &'static str,\n        pub side_effect: &'static str,\n");
    out.push_str("        pub idempotent: bool,\n        pub required_capability: &'static str,\n");
    out.push_str(
        "        pub subid: &'static str,\n        pub repomix_path: &'static str,\n        pub command: &'static [&'static str],\n    }\n\n",
    );

    for (name, path) in &methods {
        let pascal = pascal_case(name);
        out.push_str(&format!(
            "    /// Typed input candidate for `{name}` discovered at `{}`.\n",
            clean_doc(path)
        ));
        out.push_str(
            "    #[derive(Debug, Clone, Serialize, Deserialize, Default, schemars::JsonSchema)]\n",
        );
        out.push_str(&format!("    pub struct {pascal}Input {{\n"));
        out.push_str("        /// String-valued options discovered from the external surface.\n");
        out.push_str("        #[serde(default)]\n        pub options: std::collections::BTreeMap<String, String>,\n    }\n");
        out.push_str("    #[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]\n");
        out.push_str(&format!("    pub struct {pascal}Output {{\n"));
        out.push_str("        /// Human-readable operation result.\n");
        out.push_str("        pub message: String,\n        pub changed: bool,\n    }\n\n");
    }

    out.push_str("    pub const METHOD_CANDIDATES: &[MethodCandidate] = &[\n");
    for (name, path) in &methods {
        let subid_name = subid_slug(name);
        let (side_effect, idempotent, capability, category) = method_policy(name);
        let command = command_tokens(path);
        out.push_str("        MethodCandidate {\n");
        out.push_str(&format!("            name: \"{name}\",\n"));
        out.push_str(&format!("            side_effect: \"{side_effect}\",\n"));
        out.push_str(&format!("            idempotent: {idempotent},\n"));
        out.push_str(&format!(
            "            required_capability: \"{plugin}.{capability}\",\n"
        ));
        out.push_str(&format!(
            "            subid: \"{category}.software.{plugin_slug}.{subid_name}@v1\",\n"
        ));
        out.push_str(&format!(
            "            repomix_path: \"{}\",\n",
            escape_rust(path)
        ));
        out.push_str(&format!("            command: &{:?},\n", command));
        out.push_str("        },\n");
    }
    out.push_str("    ];\n\n");
    out.push_str("    /// Promote every generated method into the sealed plugin schema.\n");
    out.push_str(
        "    pub(super) fn register_methods(schema: &mut op_state_store::PluginSchema) {\n",
    );
    out.push_str("        use super::super::plugin_scaffold_helpers::method_decl_from_schemars_with_output;\n");
    for (name, path) in &methods {
        if command_tokens(path).is_empty() {
            continue;
        }
        let pascal = pascal_case(name);
        let subid_name = subid_slug(name);
        let (side_effect, idempotent, capability, category) = method_policy(name);
        let side_effect = if side_effect == "read" {
            "Read"
        } else {
            "Mutation"
        };
        out.push_str(&format!(
            "        schema.methods.insert(\"{name}\".to_string(), method_decl_from_schemars_with_output::<{pascal}Input, {pascal}Output>(\"{name}\", op_state_store::SideEffect::{side_effect}, {idempotent}, \"{plugin}.{capability}\", \"{category}.software.{plugin_slug}.{subid_name}@v1\"));\n"
        ));
    }
    out.push_str("    }\n}\n");
    out.push_str("\n// Promotion checklist (Fable contract):\n");
    out.push_str(
        "// 1. Move owned fields into the plugin State struct with concrete Rust types.\n",
    );
    out.push_str("// 2. Replace method placeholders with dedicated typed Input/Output fields.\n");
    out.push_str(
        "// 3. Register with method_decl_from_schemars_with_output and correct SideEffect.\n",
    );
    out.push_str("// 4. Register every subid, implement dispatch, and add schema/subid tests.\n");
    out.push_str("// 5. Re-run op-plugin-lint; only then replace the original plugin file.\n");
    syn::parse_file(&out)?;
    Ok(out)
}

fn candidate_fields(config_paths: &[String], cli_paths: &[String]) -> Vec<(String, String)> {
    let mut seen = BTreeSet::new();
    config_paths
        .iter()
        .chain(
            cli_paths
                .iter()
                .filter(|path| path.starts_with("flag.") && !is_action_flag(path)),
        )
        .filter_map(|path| {
            let leaf = path.rsplit('.').next().unwrap_or(path);
            let name = rust_ident(leaf);
            // Dedup on the protobuf JSON name, not the Rust ident: a type path
            // (`AliasSource`) and a field path (`ConfigFieldEntry.alias_source`)
            // yield distinct idents that collide once camel-cased, which makes
            // the whole descriptor pool unloadable. First path wins.
            if name.is_empty() || !seen.insert(json_name_key(&name)) {
                None
            } else {
                Some((name, path.clone()))
            }
        })
        .collect()
}

/// Collision key for a field name, matching how protobuf derives `json_name`:
/// camel-case, compared case-insensitively. `alias_source` and `aliassource`
/// are different Rust idents but the same key, and emitting both is what makes
/// `prost_reflect` reject the sealed blob with "camel-case name of field ...
/// conflicts with field ...".
fn json_name_key(ident: &str) -> String {
    ident
        .chars()
        .filter(|c| *c != '_')
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

fn candidate_methods(paths: &[String]) -> Vec<(String, String)> {
    let mut seen = BTreeSet::new();
    paths
        .iter()
        .filter_map(|path| {
            let raw = if let Some(command) = path.strip_prefix("cmd.") {
                command.to_string()
            } else if path.starts_with("flag.") && is_action_flag(path) {
                path.rsplit('.').next().unwrap_or(path).to_string()
            } else if let Some((owner, variant)) = command_enum_parts(path) {
                format!("{owner}_{variant}")
            } else if (path.starts_with("ts.")
                || path.starts_with("go.")
                || path.starts_with("xml."))
                && (path.contains(".method.") || path.contains(".function."))
            {
                path.rsplit('.').next().unwrap_or(path).to_string()
            } else {
                return None;
            };
            let name = rust_ident(&raw.replace('.', "_"));
            if name.is_empty() || name == "cmd" || !seen.insert(name.clone()) {
                None
            } else {
                Some((name, path.clone()))
            }
        })
        .collect()
}

fn command_enum_parts(path: &str) -> Option<(String, String)> {
    let path = path.strip_prefix("enum.")?;
    let (owner, variant) = path.rsplit_once('.')?;
    let owner = owner
        .rsplit('.')
        .next()
        .unwrap_or(owner)
        .trim_end_matches("Commands")
        .trim_end_matches("commands");
    if owner.is_empty() || variant.is_empty() {
        None
    } else {
        Some((owner.to_string(), variant.to_string()))
    }
}

fn command_tokens(path: &str) -> Vec<String> {
    if let Some(command) = path.strip_prefix("cmd.") {
        return command.split('.').map(cli_token).collect();
    }
    if path.starts_with("flag.") && is_action_flag(path) {
        let flag = path.rsplit('.').next().unwrap_or(path);
        return vec![format!("--{}", flag.replace('_', "-"))];
    }
    if let Some((owner, variant)) = command_enum_parts(path) {
        return vec![cli_token(&owner), cli_token(&variant)];
    }
    Vec::new()
}

fn cli_token(raw: &str) -> String {
    let mut out = String::new();
    for (index, ch) in raw.chars().enumerate() {
        if ch.is_ascii_uppercase() && index > 0 {
            out.push('-');
        }
        out.push(ch.to_ascii_lowercase());
    }
    out.replace('_', "-")
}

fn is_action_flag(path: &str) -> bool {
    let leaf = path.rsplit('.').next().unwrap_or(path).to_ascii_lowercase();
    [
        "add_",
        "install_",
        "list_",
        "remove_",
        "show_",
        "status",
        "sync",
        "telemetry",
        "uninstall_",
        "update_",
    ]
    .iter()
    .any(|verb| leaf == *verb || leaf.starts_with(verb))
}

fn method_policy(name: &str) -> (&'static str, bool, &'static str, &'static str) {
    let leaf = name.rsplit('_').next().unwrap_or(name);
    if matches!(
        leaf,
        "get" | "info" | "list" | "search" | "show" | "status" | "stats" | "version"
    ) || name.starts_with("get_")
        || name.starts_with("list_")
    {
        ("read", true, "read", "obs")
    } else {
        ("mutation", false, "write", "mut")
    }
}

fn infer_type(path: &str) -> &'static str {
    let p = path.to_ascii_lowercase();
    if p.contains("enabled") || p.contains("disabled") || p.ends_with(".debug") {
        "bool"
    } else if p.contains("port")
        || p.contains("timeout")
        || p.contains("limit")
        || p.contains("count")
    {
        "u64"
    } else if p.contains("paths") || p.contains("hosts") || p.contains("models") {
        "Vec<String>"
    } else {
        "String"
    }
}

fn subid_slug(name: &str) -> String {
    name.replace('_', "-")
}

fn rust_ident(raw: &str) -> String {
    let mut out = String::new();
    for c in raw.chars() {
        let c = c.to_ascii_lowercase();
        if c.is_ascii_alphanumeric() || c == '_' {
            out.push(c);
        } else if !out.ends_with('_') {
            out.push('_');
        }
    }
    while out.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        out.remove(0);
    }
    let out = out.trim_matches('_').to_string();
    if is_keyword(&out) {
        format!("{out}_field")
    } else {
        out
    }
}

fn pascal_case(s: &str) -> String {
    s.split('_')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect()
}

fn is_keyword(s: &str) -> bool {
    matches!(
        s,
        "as" | "async"
            | "abstract"
            | "await"
            | "become"
            | "box"
            | "break"
            | "const"
            | "continue"
            | "crate"
            | "dyn"
            | "do"
            | "else"
            | "enum"
            | "extern"
            | "false"
            | "final"
            | "fn"
            | "for"
            | "if"
            | "impl"
            | "in"
            | "let"
            | "loop"
            | "macro"
            | "match"
            | "mod"
            | "move"
            | "mut"
            | "override"
            | "priv"
            | "pub"
            | "ref"
            | "return"
            | "self"
            | "Self"
            | "static"
            | "struct"
            | "super"
            | "trait"
            | "try"
            | "true"
            | "type"
            | "typeof"
            | "unsafe"
            | "unsized"
            | "use"
            | "virtual"
            | "where"
            | "while"
            | "yield"
    )
}

fn clean_doc(s: &str) -> String {
    s.replace(['`', '\n', '\r'], " ")
}
fn escape_rust(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifiers_are_safe_and_deduplicated() {
        let paths = vec![
            "x.config.default-model".into(),
            "y.config.default-model".into(),
            "z.config.type".into(),
        ];
        let got = candidate_fields(&paths, &[]);
        assert_eq!(got[0].0, "default_model");
        assert_eq!(got[1].0, "type_field");
    }

    /// Regression: a type path and a field path that differ only by separators
    /// must not both be emitted — camel-cased they are one protobuf field, and
    /// the duplicate makes the sealed blob's descriptor pool fail to decode.
    #[test]
    fn camel_case_colliding_candidates_are_emitted_once() {
        let paths = vec![
            "enum.zeroclaw_config.AliasSource".into(),
            "struct.zeroclaw_config.ConfigFieldEntry.alias_source".into(),
            "struct.zeroclaw_config.Runtime.model_providers".into(),
            "enum.zeroclaw_config.ModelProviders".into(),
        ];
        let got = candidate_fields(&paths, &[]);
        let names: Vec<&str> = got.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, vec!["aliassource", "model_providers"]);

        let mut keys: Vec<String> = got.iter().map(|(n, _)| json_name_key(n)).collect();
        let before = keys.len();
        keys.sort();
        keys.dedup();
        assert_eq!(keys.len(), before, "emitted fields must have unique json names");
    }

    #[test]
    fn method_paths_become_typed_names() {
        let paths = vec![
            "cmd.plugin.install".into(),
            "cmd.models.list".into(),
            "enum.zeroclaw.PluginCommands.Remove".into(),
            "flag.root.disable_gpu".into(),
            "flag.root.install_extension".into(),
        ];
        let got = candidate_methods(&paths);
        assert_eq!(got[0].0, "plugin_install");
        assert_eq!(pascal_case(&got[0].0), "PluginInstall");
        assert_eq!(got[2].0, "plugin_remove");
        assert_eq!(got[3].0, "install_extension");
        assert_eq!(method_policy("models_list"), ("read", true, "read", "obs"));
    }
}
