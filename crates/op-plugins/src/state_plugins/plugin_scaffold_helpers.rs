//! Plugin scaffold helpers.
//!
//! THE PLUGIN IS THE SCHEMA. Every schema is defined in its own plugin file.
//! This module only provides shared helpers used by the ENTRY/EXIT scaffold:
//! projection seeds, method declarations, and legacy cross-plugin discovery.
//!
//! For the expected top-to-bottom plugin shape, start from
//! `plugin_scaffold.rs.template`. Do not add new plugin schema bodies here.

// ── Re-exports from plugin files ──────────────────────────────────────────────

pub(crate) use super::adc::adc_schema as adc_plugin_schema;
pub(crate) use super::agent_config::agent_config_schema as agent_config_plugin_schema;
pub(crate) use super::antigravity::antigravity_schema as antigravity_plugin_schema;
pub(crate) use super::antigravity_chat::antigravity_chat_schema as antigravity_chat_plugin_schema;
pub(crate) use super::btrfs_plugin::btrfs_schema as btrfs_plugin_schema;
pub(crate) use super::cognitive_mcp::cognitive_mcp_schema as cognitive_mcp_plugin_schema;
pub(crate) use super::cron::cron_schema as cron_plugin_schema;
pub(crate) use super::ctl_plane_chatbot::ctl_plane_chatbot_schema as ctl_plane_chatbot_plugin_schema;
pub(crate) use super::endpoint::endpoint_schema as endpoint_plugin_schema;
pub(crate) use super::fail2ban::fail2ban_schema as fail2ban_plugin_schema;
pub(crate) use super::gcloud_adc::gcloud_adc_schema as gcloud_adc_plugin_schema;
pub(crate) use super::hardware::hardware_schema as hardware_plugin_schema;
pub(crate) use super::human_principal::human_principal_schema as human_principal_plugin_schema;
pub(crate) use super::identity_sled::identity_sled_schema as identity_sled_plugin_schema;
pub(crate) use super::incus::incus_schema as incus_plugin_schema;
pub(crate) use super::json_render::json_render_schema as json_render_plugin_schema;
pub(crate) use super::keypair::keypair_schema as keypair_plugin_schema;
pub(crate) use super::mail_server::mail_server_schema as mail_server_plugin_schema;
pub(crate) use super::memory_plugin::memory_schema as memory_plugin_schema;
pub(crate) use super::net::net_schema as net_plugin_schema;
pub(crate) use super::netmaker::netmaker_schema as netmaker_plugin_schema;
pub(crate) use super::oci::oci_schema;
pub(crate) use super::openflow::openflow_schema as openflow_plugin_schema;
pub(crate) use super::oscal_subid_registry::oscal_subid_registry_schema;
pub(crate) use super::ovsdb_bridge::ovsdb_bridge_schema as ovsdb_bridge_plugin_schema;
pub(crate) use super::proxy_server::proxy_server_schema as proxy_server_plugin_schema;
pub(crate) use super::rovs_commands::rovs_commands_schema as rovs_commands_plugin_schema;
pub(crate) use super::rtnetlink::rtnetlink_schema as rtnetlink_plugin_schema;
pub(crate) use super::schema_renderer::schema_renderer_schema as schema_renderer_plugin_schema;
pub(crate) use super::service::service_schema as service_plugin_schema;
pub(crate) use super::sessdecl::sess_decl_schema as sess_decl_plugin_schema;
pub(crate) use super::software::software_schema as software_plugin_schema;
pub(crate) use super::tched_router::tched_router_plugin_schema;
pub(crate) use super::unix_socket::unix_socket_schema_derived as unix_socket_plugin_schema;
pub(crate) use super::users::users_schema as users_plugin_schema;
pub(crate) use super::web_ui::web_ui_schema as web_ui_plugin_schema;
pub(crate) use super::wg_opdbus::wg_opdbus_schema;
pub(crate) use super::wgcf::wgcf_schema as wgcf_plugin_schema;
pub(crate) use super::wireguard::wireguard_schema as wireguard_plugin_schema;
pub(crate) use super::workflows_plugin::workflows_schema as workflows_plugin_schema;
pub(crate) use super::xray::xray_schema as xray_plugin_schema;

// Added missing schema re-exports
pub(crate) use super::config::config_plugin_schema;
pub(crate) use super::cozo::cozo_schema as cozo_plugin_schema;
pub(crate) use super::datastore::datastore_schema as datastore_plugin_schema;
pub(crate) use super::dnsresolver::dnsresolver_schema as dnsresolver_plugin_schema;
pub(crate) use super::gemma_brain::gemma_brain_schema as gemma_brain_plugin_schema;
pub(crate) use super::large_language_model::large_language_model_schema as large_language_model_plugin_schema;
pub(crate) use super::login1::login1_schema as login1_plugin_schema;
pub(crate) use super::mcp::mcp_schema as mcp_plugin_schema;
pub(crate) use super::notebooklm::notebooklm_schema as notebooklm_plugin_schema;
pub(crate) use super::snowball_plugin::snowball_schema as snowball_plugin_schema;
// pub(crate) use super::packagekit::packagekit_schema as packagekit_plugin_schema;
pub(crate) use super::persona::persona_schema as persona_plugin_schema;
pub(crate) use super::procfs::procfs_schema as procfs_plugin_schema;
pub(crate) use super::qdrant::qdrant_schema as qdrant_plugin_schema;
pub(crate) use super::shared_unix_socket::shared_unix_socket_schema as shared_unix_socket_plugin_schema;

// ── Shared utility functions (not schemas) ─────────────────────────────────────

use op_state_store::{FieldSchema, FieldType, MethodDecl, PluginSchema, SideEffect};
use serde::{Deserialize, Serialize};
use simd_json::{json, OwnedValue as Value};

/// Materialize a deterministic initial live state from a plugin schema.
///
/// This is the schema-side bootstrap for D-Bus projection creation. It never
/// queries a plugin backend and never invents data outside the contract:
/// field defaults win, then examples, then an empty value matching the field
/// type. The MutationEngine writes this once when no projection exists yet;
/// later mutations replace it through the normal `/dev/shm/opdbus/state`
/// write door.
pub(crate) fn materialize_state_from_schema(schema: &PluginSchema) -> Value {
    if let Some(example) = &schema.example {
        return example.clone();
    }

    let mut state = simd_json::value::owned::Object::new();
    let mut fields = schema.fields.iter().collect::<Vec<_>>();
    fields.sort_by_key(|(a, _)| *a);

    for (name, field) in fields {
        state.insert(name.clone(), materialize_field(field));
    }

    Value::Object(Box::new(state))
}

fn materialize_field(field: &FieldSchema) -> Value {
    if let Some(value) = &field.default {
        return value.clone();
    }
    if let Some(value) = &field.example {
        return value.clone();
    }

    materialize_field_type(&field.field_type)
}

fn materialize_field_type(field_type: &FieldType) -> Value {
    match field_type {
        FieldType::String => json!(""),
        FieldType::Integer => json!(0),
        FieldType::Float => json!(0.0),
        FieldType::Boolean => json!(false),
        FieldType::Array(_) => json!([]),
        FieldType::Object(fields) => {
            let mut object = simd_json::value::owned::Object::new();
            let mut fields = fields.iter().collect::<Vec<_>>();
            fields.sort_by_key(|(a, _)| *a);
            for (name, field) in fields {
                object.insert(name.clone(), materialize_field(field));
            }
            Value::Object(Box::new(object))
        }
        FieldType::Enum(values) => values
            .first()
            .map(|value| json!(value))
            .unwrap_or_else(|| json!(null)),
        FieldType::OneOf(types) => types
            .first()
            .map(materialize_field_type)
            .unwrap_or_else(|| json!(null)),
        FieldType::Map(_) => json!({}),
        FieldType::JsonSchema(_) | FieldType::Any => json!(null),
    }
}

/// Format a plugin's live state into a `PluginSchema`.
///
/// This is a *formatter*, not a schema authority — nothing here is indexed. The
/// plugin's live state (its `*State` struct) is the single source
/// of truth; field types are inferred from that state, so the schema can never
/// drift from what the plugin actually reports. To add a field, add it to the
/// plugin state — never hand-write a field definition.
pub(crate) fn schema_from_state(
    name: &str,
    category: &str,
    version: &str,
    description: &str,
    state: &Value,
) -> PluginSchema {
    use simd_json::prelude::*;

    let mut builder = PluginSchema::builder(name)
        .version(version)
        .category(category)
        .description(description);

    if let Some(obj) = state.as_object() {
        for (key, value) in obj.iter() {
            builder = builder.field(&key.to_string(), field_from_value(value));
        }
    }

    builder.example(state.clone()).build()
}

/// Infer a `FieldSchema` (type + example) from a live state value.
pub(crate) fn field_from_value(value: &Value) -> FieldSchema {
    FieldSchema {
        field_type: infer_field_type(value),
        required: false,
        description: String::new(),
        default: None,
        example: Some(value.clone()),
        constraints: Vec::new(),
        read_only: false,
        read_only_when: None,
    }
}

/// Infer a `FieldType` from a live state value, recursing into arrays/objects.
pub(crate) fn infer_field_type(value: &Value) -> FieldType {
    use simd_json::prelude::*;

    if value.is_null() {
        FieldType::Any
    } else if value.as_bool().is_some() {
        FieldType::Boolean
    } else if value.as_i64().is_some() || value.as_u64().is_some() {
        FieldType::Integer
    } else if value.as_f64().is_some() {
        FieldType::Float
    } else if value.as_str().is_some() {
        FieldType::String
    } else if let Some(arr) = value.as_array() {
        let inner = arr.first().map(infer_field_type).unwrap_or(FieldType::Any);
        FieldType::Array(Box::new(inner))
    } else if let Some(obj) = value.as_object() {
        let fields = obj
            .iter()
            .map(|(k, v)| (k.to_string(), field_from_value(v)))
            .collect();
        FieldType::Object(fields)
    } else {
        FieldType::Any
    }
}

pub(crate) fn any_field(required: bool, description: &str, default: Option<Value>) -> FieldSchema {
    FieldSchema {
        field_type: FieldType::Any,
        required,
        description: description.to_string(),
        default,
        example: None,
        constraints: Vec::new(),
        read_only: false,
        read_only_when: None,
    }
}

pub(crate) fn cap_method(
    name: &str,
    side_effect: SideEffect,
    idempotent: bool,
    required_capability: &str,
    subid: &str,
) -> MethodDecl {
    MethodDecl {
        name: name.to_string(),
        args: json!({"type": "object", "additionalProperties": true}),
        returns: Some(json!({"type": "object", "additionalProperties": true})),
        side_effect,
        idempotent,
        required_capability: Some(required_capability.to_string()),
        subid: subid.to_string(),
    }
}

use super::common::oscal::is_component_type;
use std::collections::HashMap;

/// Category used when a plugin declares nothing to infer one from.
const DEFAULT_CATEGORY: &str = "software";

/// Build the schema-level subid for a plugin.
///
/// Declared in one place so every plugin gets one without copy-pasting a
/// `#[schemars(extend("x-oscal-subid" = ...))]` attribute onto its state struct
/// — that duplication is why 20 of 65 plugins ended up with no schema subid at
/// all, while their methods (which route through the helpers here) always had
/// theirs.
///
/// `declared_category` is the plugin stating what it is, and always wins — it is
/// the one fact here that cannot be derived from anything else. Failing that,
/// the category is inferred from the plugin's own field subids, so a plugin
/// cannot contradict itself. Ties break lexicographically: this value feeds the
/// schema hash, so it must not depend on `HashMap` iteration order.
///
/// The subject segment is kebab-cased — `SUBID_CATEGORIES`' grammar allows no
/// underscores, so `agent_config` has to become `agent-config`.
pub fn derive_schema_subid(
    plugin_name: &str,
    declared_category: Option<&str>,
    declared: &HashMap<String, String>,
) -> String {
    let inferred = || {
        let mut counts: HashMap<&str, usize> = HashMap::new();
        for subid in declared.values() {
            if let Some(category) = subid.split('.').nth(1) {
                if is_component_type(category) {
                    *counts.entry(category).or_default() += 1;
                }
            }
        }

        let mut ranked: Vec<(&str, usize)> = counts.into_iter().collect();
        ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
        ranked
            .first()
            .map(|(c, _)| c.to_string())
            .unwrap_or_else(|| DEFAULT_CATEGORY.to_string())
    };

    // An unusable declared category is ignored rather than trusted: emitting it
    // would produce a subid that fails validation at the gate.
    let category = declared_category
        .filter(|c| is_component_type(c))
        .map(str::to_string)
        .unwrap_or_else(inferred);

    format!(
        "sch.{category}.plugin.{}.schema@v1",
        plugin_name.replace('_', "-")
    )
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct EmptyInput {}

/// Create a MethodDecl from a schemars-typed input struct.
///
/// This is the canonical pattern for all plugins: define method inputs as
/// Rust structs with `#[derive(schemars::JsonSchema)]`, then use this function
/// to generate typed method declarations. The resulting schema drives D-Bus method
/// validation and gRPC proto generation.
///
/// For methods with typed outputs, use `method_decl_from_schemars_with_output`.
/// For methods with no meaningful return (just success/failure), use `AckOutput`.
#[deprecated(
    since = "0.2.0",
    note = "Use method_decl_from_schemars_with_output for typed returns"
)]
pub fn method_decl_from_schemars<Input: schemars::JsonSchema>(
    name: &str,
    side_effect: SideEffect,
    idempotent: bool,
    required_capability: &str,
    subid: &str,
) -> MethodDecl {
    method_decl_from_schemars_with_output::<Input, AckOutput>(
        name,
        side_effect,
        idempotent,
        required_capability,
        subid,
    )
}

/// Dual-generic helper that generates method declarations with typed input AND output schemas.
/// The returns schema is derived from `Output` via schemars - NO hardcoded `{"type": "object"}`.
pub fn method_decl_from_schemars_with_output<
    Input: schemars::JsonSchema,
    Output: schemars::JsonSchema,
>(
    name: &str,
    side_effect: SideEffect,
    idempotent: bool,
    required_capability: &str,
    subid: &str,
) -> MethodDecl {
    // Generate input schema
    let serde_input_schema = serde_json::to_value(schemars::schema_for!(Input)).unwrap();
    let input_bytes = serde_json::to_vec(&serde_input_schema).unwrap();
    let mut input_bytes = input_bytes;
    let args = simd_json::to_owned_value(&mut input_bytes).unwrap();

    // Generate output schema
    let serde_output_schema = serde_json::to_value(schemars::schema_for!(Output)).unwrap();
    let output_bytes = serde_json::to_vec(&serde_output_schema).unwrap();
    let mut output_bytes = output_bytes;
    let returns = simd_json::to_owned_value(&mut output_bytes).unwrap();

    MethodDecl {
        name: name.to_string(),
        args,
        returns: Some(returns),
        side_effect,
        idempotent,
        required_capability: Some(required_capability.to_string()),
        subid: subid.to_string(),
    }
}

/// Standard acknowledgment response for methods that only need success/failure.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AckOutput {
    pub success: bool,
}

pub(crate) fn simple_schema(
    name: &str,
    description: &str,
    dependencies: &[&str],
    fields: Vec<(&str, FieldSchema)>,
) -> PluginSchema {
    let mut builder = PluginSchema::builder(name)
        .version("1.0.0")
        .description(description);
    for dep in dependencies {
        builder = builder.dependency(dep);
    }
    for (field_name, schema) in fields {
        builder = builder.field(field_name, schema);
    }
    builder.build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn field(field_type: FieldType, default: Option<Value>, example: Option<Value>) -> FieldSchema {
        FieldSchema {
            field_type,
            required: false,
            description: String::new(),
            default,
            example,
            constraints: Vec::new(),
            read_only: false,
            read_only_when: None,
        }
    }

    #[test]
    fn materializes_seed_state_from_schema_defaults_examples_and_types() {
        let schema = PluginSchema::builder("example")
            .field(
                "enabled",
                field(FieldType::Boolean, Some(json!(true)), Some(json!(false))),
            )
            .field("label", field(FieldType::String, None, Some(json!("demo"))))
            .field(
                "mode",
                field(
                    FieldType::Enum(vec!["active".to_string(), "passive".to_string()]),
                    None,
                    None,
                ),
            )
            .field(
                "choice",
                field(
                    FieldType::OneOf(vec![FieldType::Integer, FieldType::String]),
                    None,
                    None,
                ),
            )
            .field(
                "nested",
                field(
                    FieldType::Object(HashMap::from([(
                        "count".to_string(),
                        field(FieldType::Integer, None, None),
                    )])),
                    None,
                    None,
                ),
            )
            .build();

        let state = materialize_state_from_schema(&schema);

        assert_eq!(state["enabled"], json!(true));
        assert_eq!(state["label"], json!("demo"));
        assert_eq!(state["mode"], json!("active"));
        assert_eq!(state["choice"], json!(0));
        assert_eq!(state["nested"]["count"], json!(0));
    }
}

#[cfg(test)]
mod derive_schema_subid_tests {
    use super::derive_schema_subid;
    use crate::state_plugins::common::oscal::validate_subid;
    use std::collections::HashMap;

    fn declared(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn takes_the_category_the_plugins_own_fields_use() {
        // procfs declares `service` on every field but `software` on its struct;
        // the fields are the plugin describing itself, so they win.
        let subids = declared(&[
            ("memory", "obs.service.procfs.memory.query@v2"),
            ("cpuinfo", "obs.service.procfs.cpuinfo.query@v1"),
        ]);
        let got = derive_schema_subid("procfs", None, &subids);
        assert_eq!(got, "sch.service.plugin.procfs.schema@v1");
        assert!(validate_subid(&got).is_ok(), "derived subid must validate");
    }

    #[test]
    fn kebab_cases_underscored_plugin_names() {
        // The grammar allows no underscores, so `agent_config` must not appear
        // verbatim or the result fails validation.
        let got = derive_schema_subid("agent_config", None, &declared(&[]));
        assert_eq!(got, "sch.software.plugin.agent-config.schema@v1");
        assert!(validate_subid(&got).is_ok());
    }

    #[test]
    fn accepts_internal_categories_not_just_oscal_ones() {
        let subids = declared(&[("subvolumes", "obs.storage.btrfs.subvolume.list@v1")]);
        let got = derive_schema_subid("btrfs", None, &subids);
        assert_eq!(got, "sch.storage.plugin.btrfs.schema@v1");
        assert!(validate_subid(&got).is_ok());
    }

    #[test]
    fn is_deterministic_because_the_result_feeds_the_schema_hash() {
        let subids = declared(&[
            ("a", "obs.service.x.a.query@v1"),
            ("b", "obs.network.x.b.query@v1"),
            ("c", "obs.network.x.c.query@v1"),
        ]);
        let first = derive_schema_subid("x", None, &subids);
        for _ in 0..50 {
            assert_eq!(derive_schema_subid("x", None, &subids), first);
        }
        assert_eq!(first, "sch.network.plugin.x.schema@v1");
    }
}

#[cfg(test)]
mod declared_category_tests {
    use super::derive_schema_subid;
    use crate::state_plugins::common::oscal::validate_subid;
    use std::collections::HashMap;

    #[test]
    fn a_declared_category_beats_inference() {
        // openflow's fields say nothing, but the plugin knows it is network.
        let got = derive_schema_subid("openflow", Some("network"), &HashMap::new());
        assert_eq!(got, "sch.network.plugin.openflow.schema@v1");
        assert!(validate_subid(&got).is_ok());
    }

    #[test]
    fn an_unusable_declared_category_is_ignored_not_emitted() {
        // Emitting an unknown category would fail the subid gate, so the
        // derivation must fall back rather than trust the declaration.
        for bogus in ["nonsense", "replace_me", ""] {
            let got = derive_schema_subid("x", Some(bogus), &HashMap::new());
            assert_eq!(got, "sch.software.plugin.x.schema@v1");
            assert!(validate_subid(&got).is_ok(), "{bogus} produced {got}");
        }
    }

    #[test]
    fn the_domain_terms_real_plugins_declare_are_usable() {
        // `llm` and `agents` are declared by live plugins. They are aliases, so
        // they survive into the subid verbatim and resolve to OSCAL elsewhere.
        for (category, plugin) in [("llm", "large-language-model"), ("agents", "agent-config")] {
            let got = derive_schema_subid(plugin, Some(category), &HashMap::new());
            assert_eq!(got, format!("sch.{category}.plugin.{plugin}.schema@v1"));
            assert!(validate_subid(&got).is_ok(), "{category} produced {got}");
        }
    }
}
