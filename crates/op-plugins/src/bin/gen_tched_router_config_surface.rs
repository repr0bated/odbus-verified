//! Derive tched_router config methods from the plugin-owned contract.
//! Upstream upgrades are explicit imports, never runtime schema lookups:
//! `cargo run -p op-plugins --bin gen_tched_router_config_surface -- --import-json /tmp/config.json --version VERSION`
//! Default invocation only regenerates derived Rust methods; no schema directory is used.

use op_plugins::state_plugins::tched_router_contract as contract;
use serde_json::Value;
use std::fmt::Write as _;
use std::path::PathBuf;

/// One config section discovered from the official schema document.
struct Section {
    /// serde key on the native config (e.g. "gateway", "channels").
    key: String,
}

/// Local (3tched Router-owned) configuration sections — plain configurations,
/// not a separate "options" projection. Types live in `super::tched_router`.
const LOCAL_SECTIONS: &[(&str, &str)] = &[
    ("model_assignments", "super::tched_router::ModelAssignments"),
    (
        "memory_namespaces",
        "Vec<super::tched_router::MemoryNamespaceOption>",
    ),
    (
        "registration_service",
        "super::tched_router::RegistrationServiceSchema",
    ),
    (
        "user_container",
        "super::tched_router::UserContainerOptions",
    ),
    ("identity_chain", "super::tched_router::IdentityOptions"),
    ("privacy_policy", "super::tched_router::PrivacyOptions"),
];

fn pascal(s: &str) -> String {
    s.split('_')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let mut c = p.chars();
            match c.next() {
                Some(f) => f.to_ascii_uppercase().to_string() + c.as_str(),
                None => String::new(),
            }
        })
        .collect()
}

fn kebab(s: &str) -> String {
    s.replace('_', "-")
}

/// `Get` + Pascal(section) + `Config`, without doubling a trailing `Config`.
fn method_name(section_key: &str) -> String {
    let p = pascal(section_key);
    if p.ends_with("Config") {
        format!("Get{p}")
    } else {
        format!("Get{p}Config")
    }
}

/// Extract sections from the official config schema document.
fn discover_sections(schema: &Value) -> Vec<Section> {
    let props = schema
        .get("properties")
        .and_then(Value::as_object)
        .expect("config schema has properties");
    let mut out = Vec::new();
    for (key, prop) in props {
        let kind = prop.get("type").and_then(Value::as_str);
        let has_ref = prop.get("$ref").is_some();
        // Scalars stay reachable through `GetConfig`; they get no method of
        // their own, matching the previous surface.
        if !has_ref && !matches!(kind, Some("array") | Some("object")) {
            continue;
        }
        out.push(Section { key: key.clone() });
    }
    out.sort_by(|a, b| a.key.cmp(&b.key));
    out
}

/// Rust fn name carrying a section's embedded schema.
fn schema_fn(key: &str) -> String {
    format!("{}_section_schema", key.replace('-', "_"))
}

fn main() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut args = std::env::args().skip(1);
    let mut import = None;
    let mut import_version = None;
    let mut out_path = manifest.join("src/state_plugins/tched_router_config_surface.rs");
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--import-json" => {
                import = Some(PathBuf::from(
                    args.next().expect("--import-json needs a path"),
                ))
            }
            "--version" => import_version = Some(args.next().expect("--version needs a version")),
            "--output" => out_path = PathBuf::from(args.next().expect("--output needs a path")),
            value if !value.starts_with('-') => out_path = PathBuf::from(value),
            _ => panic!("unknown argument {arg}"),
        }
    }
    assert_eq!(
        import.is_some(),
        import_version.is_some(),
        "imports require both --import-json and --version"
    );
    let document: Value = match &import {
        Some(path) => serde_json::from_str(&std::fs::read_to_string(path).expect("read import"))
            .expect("import is valid JSON"),
        None => contract::document().clone(),
    };
    let version = import_version
        .as_deref()
        .unwrap_or(contract::BINARY_SCHEMA_VERSION);

    let defs = document
        .get("$defs")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let prop_count = document
        .get("properties")
        .and_then(Value::as_object)
        .map(|m| m.len())
        .unwrap_or(0);

    // ── Stub guard ──────────────────────────────────────────────────────────
    // Refuse to emit from an empty document. A stubbed-out source silently
    // deletes the entire config surface, which is exactly how this file lost
    // its meaning before: `vendor/zeroclawlabs` declared 54 empty structs and
    // every generated method carried a payload with no fields.
    assert!(
        prop_count >= 32 && defs.len() >= 64,
        "REFUSING TO GENERATE: plugin contract has {prop_count} properties and {} definitions; refusing a stub or truncated import",
        defs.len(),
    );

    let upstream = discover_sections(&document);
    assert!(!upstream.is_empty(), "no config sections discovered");
    // Validate all references before changing the plugin contract.
    for section in &upstream {
        let _ = contract::section_from(&document, &section.key);
    }
    if import.is_some() {
        assert!(
            !version.trim().is_empty() && version != "unknown",
            "version is required"
        );
        let raw = serde_json::to_string_pretty(&document).expect("serialize import");
        assert!(!raw.contains("\"####"), "raw string delimiter collision");
        let path = manifest.join("src/state_plugins/tched_router_contract.rs");
        let source = std::fs::read_to_string(&path).expect("read plugin contract");
        let begin = source
            .find("// BEGIN UPSTREAM IMPORT")
            .expect("import start");
        let end = source.find("// END UPSTREAM IMPORT").expect("import end");
        let replacement = format!(
            "// BEGIN UPSTREAM IMPORT\npub const BINARY_SCHEMA_VERSION: &str = {version:?};\nconst CONFIG_SCHEMA: &str = r####\"{raw}\"####;\n"
        );
        let updated = format!("{}{replacement}{}", &source[..begin], &source[end..]);
        let temporary = path.with_extension("rs.tmp");
        std::fs::write(&temporary, updated).expect("write imported contract");
        std::fs::rename(temporary, path).expect("replace imported contract");
    }

    // ── Emit the Rust surface ───────────────────────────────────────────────
    let mut s = String::new();
    s.push_str("//! GENERATED by gen_tched_router_config_surface; do not hand-edit.\n//! All schemas derive from the tched_router plugin contract.\n\n");
    s.push_str("use super::plugin_scaffold_helpers::method_decl_from_schemars_with_output;\n");
    s.push_str("use super::tched_router::{ConfigPatchInput, ConfigPatchOutput};\n");
    s.push_str("use op_state_store::{CapabilityDecl, PluginSchema, SideEffect};\n");
    s.push_str("use serde::{Deserialize, Serialize};\n");
    s.push_str("use serde_json::Value as JsonValue;\n\n");

    s.push_str("use super::tched_router_contract as contract;\n\n");
    s.push_str("fn embedded(value: JsonValue) -> schemars::Schema {\n    schemars::Schema::try_from(value).expect(\"plugin config contract is a JSON Schema\")\n}\n\n");
    s.push_str("fn embedded_output_config(value: JsonValue) -> schemars::Schema {\n    embedded(op_state_store::exact_schema::embed_exact_schema(\n        &value,\n        \"/properties/config\",\n    ))\n}\n\n");
    // Shared input/output structs.
    s.push_str(
        "/// Empty input for config read methods.\n\
         #[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]\n\
         pub struct ConfigMethodInput {}\n\n",
    );
    s.push_str(
        "/// Full configuration schema, as printed by `zeroclaw config schema`.\n\
         pub fn full_config_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {\n\
         \x20   embedded_output_config(contract::document().clone())\n\
         }\n\n",
    );
    s.push_str(
        "/// Output for `GetConfig` — the full configuration, typed by the official\n\
         /// binary's own schema. Secrets are masked by dispatch.\n\
         #[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]\n\
         #[schemars(extend(\"x-oscal-subid\" = \"exp.service.3tched-router.config.result@v1\"))]\n\
         pub struct GetConfigOutput {\n\
         \x20   /// Full configuration.\n\
         \x20   #[schemars(schema_with = \"full_config_schema\")]\n\
         \x20   pub config: JsonValue,\n\
         }\n\n",
    );
    s.push_str(
        "/// `PatchConfig` uses the same bounded native property-operation contract as\n\
         /// `config_patch`; whole-section replacement is not a v0.8.4 operation.\n\
         pub type PatchConfigInput = ConfigPatchInput;\n\
         pub type PatchConfigOutput = ConfigPatchOutput;\n\n",
    );

    // Per-section schema accessors.
    s.push_str("// ── Section schemas (from the official binary) ──────────────────────────\n\n");
    for sec in &upstream {
        let f = schema_fn(&sec.key);
        let _ = write!(
            s,
            "/// JSON Schema for the `{}` config section.\n\
             pub fn {f}(_: &mut schemars::SchemaGenerator) -> schemars::Schema {{\n\
             \x20   embedded_output_config(contract::section(\"{}\"))\n\
             }}\n\n",
            sec.key, sec.key
        );
    }

    // Per-section output structs.
    let mut all: Vec<(String, Option<String>)> = Vec::new(); // (key, local rust type)
    for sec in &upstream {
        all.push((sec.key.clone(), None));
    }
    for (key, ty) in LOCAL_SECTIONS {
        all.push(((*key).to_string(), Some((*ty).to_string())));
    }

    s.push_str("// ── Per-section outputs ────────────────────────────────────────────────\n\n");
    for (key, local) in &all {
        let m = method_name(key);
        match local {
            Some(ty) => {
                let _ = write!(
                    s,
                    "/// Output for `{m}`.\n\
                     #[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]\n\
                     #[schemars(extend(\"x-oscal-subid\" = \"exp.service.3tched-router.{}-config.result@v1\"))]\n\
                     pub struct {m}Output {{\n\
                     \x20   /// Current `{key}` configuration.\n\
                     \x20   pub config: {ty},\n\
                     }}\n\n",
                    kebab(key)
                );
            }
            None => {
                let f = schema_fn(key);
                let _ = write!(
                    s,
                    "/// Output for `{m}`.\n\
                     #[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]\n\
                     #[schemars(extend(\"x-oscal-subid\" = \"exp.service.3tched-router.{}-config.result@v1\"))]\n\
                     pub struct {m}Output {{\n\
                     \x20   /// Current `{key}` configuration.\n\
                     \x20   #[schemars(schema_with = \"{f}\")]\n\
                     \x20   pub config: JsonValue,\n\
                     }}\n\n",
                    kebab(key)
                );
            }
        }
    }

    // Method table + registration.
    s.push_str("/// (method, section key, source) for every config read.\n");
    s.push_str("pub const SECTIONS: &[(&str, &str, SectionSource)] = &[\n");
    for (key, local) in &all {
        let src = if local.is_some() { "Local" } else { "Upstream" };
        let _ = writeln!(
            s,
            "    (\"{}\", \"{key}\", SectionSource::{src}),",
            method_name(key)
        );
    }
    s.push_str("];\n\n");
    s.push_str("/// Where a config section is read from.\n");
    s.push_str("#[derive(Debug, Clone, Copy, PartialEq, Eq)]\n");
    s.push_str("pub enum SectionSource {\n    /// The native tched_router configuration, described by the imported plugin-owned contract.\n    Upstream,\n    /// Local 3tched Router state (a plain configuration).\n    Local,\n}\n\n");

    s.push_str("/// Every method owned by this surface (for dispatch membership checks).\n");
    s.push_str(
        "pub const CONFIG_METHODS: &[&str] = &[\n    \"GetConfig\",\n    \"PatchConfig\",\n",
    );
    for (key, _local) in &all {
        let _ = writeln!(s, "    \"{}\",", method_name(key));
    }
    s.push_str("];\n\n");

    s.push_str(
        "/// Register every config method and its declared capability on the schema.\n\
         ///\n\
         /// The plugin is the single declaration point: each method's\n\
         /// `required_capability` has a matching `schema.capabilities` entry\n\
         /// (closure-clean under `validate_capability_closure`).\n\
         pub fn register_config_methods(schema: &mut PluginSchema) {\n",
    );
    s.push_str(
        "    schema.methods.insert(\n\
         \x20       \"GetConfig\".to_string(),\n\
         \x20       method_decl_from_schemars_with_output::<ConfigMethodInput, GetConfigOutput>(\n\
         \x20           \"GetConfig\",\n\
         \x20           SideEffect::Read,\n\
         \x20           true,\n\
         \x20           \"cap.software.3tched-router.config.read@v1\",\n\
         \x20           \"obs.service.3tched-router.config.get@v1\",\n\
         \x20       ),\n\
         \x20   );\n\
         \x20   schema.capabilities.insert(\n\
         \x20       \"cap.software.3tched-router.config.read@v1\".to_string(),\n\
         \x20       CapabilityDecl {\n\
         \x20           id: \"cap.software.3tched-router.config.read@v1\".to_string(),\n\
         \x20           description: \"Read the full 3tched Router configuration (secrets masked).\".to_string(),\n\
         \x20       },\n\
         \x20   );\n\
         \x20   schema.methods.insert(\n\
         \x20       \"PatchConfig\".to_string(),\n\
         \x20       method_decl_from_schemars_with_output::<PatchConfigInput, PatchConfigOutput>(\n\
         \x20           \"PatchConfig\",\n\
         \x20           SideEffect::Mutation,\n\
         \x20           false,\n\
         \x20           \"cap.software.3tched-router.config.write@v1\",\n\
         \x20           \"mut.service.3tched-router.config.patch@v2\",\n\
         \x20       ),\n\
         \x20   );\n\
         \x20   schema.capabilities.insert(\n\
         \x20       \"cap.software.3tched-router.config.write@v1\".to_string(),\n\
         \x20       CapabilityDecl {\n\
         \x20           id: \"cap.software.3tched-router.config.write@v1\".to_string(),\n\
         \x20           description: \"Apply bounded typed 3tched Router configuration property operations.\".to_string(),\n\
         \x20       },\n\
         \x20   );\n",
    );
    for (key, _local) in &all {
        let m = method_name(key);
        let k = kebab(key);
        let _ = writeln!(
            s,
            "    schema.methods.insert(\n\
             \x20       \"{m}\".to_string(),\n\
             \x20       method_decl_from_schemars_with_output::<ConfigMethodInput, {m}Output>(\n\
             \x20           \"{m}\",\n\
             \x20           SideEffect::Read,\n\
             \x20           true,\n\
             \x20           \"cap.software.3tched-router.{k}-config.read@v1\",\n\
             \x20           \"obs.service.3tched-router.{k}-config.get@v1\",\n\
             \x20       ),\n\
             \x20   );\n\
             \x20   schema.capabilities.insert(\n\
             \x20       \"cap.software.3tched-router.{k}-config.read@v1\".to_string(),\n\
             \x20       CapabilityDecl {{\n\
             \x20           id: \"cap.software.3tched-router.{k}-config.read@v1\".to_string(),\n\
             \x20           description: \"Read the 3tched Router `{key}` configuration.\".to_string(),\n\
             \x20       }},\n\
             \x20   );"
        );
    }
    s.push_str("}\n\n");

    s.push_str(include_str!("tched_router_config_surface.dispatch.inc"));

    std::fs::write(&out_path, s).unwrap_or_else(|e| panic!("write {}: {e}", out_path.display()));
    eprintln!(
        "wrote {} from plugin contract v{version} ({} upstream sections, {} local, {} config methods)",
        out_path.display(),
        upstream.len(),
        LOCAL_SECTIONS.len(),
        all.len() + 2,
    );
}
