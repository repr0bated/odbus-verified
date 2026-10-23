//! Secret classification from the exact manifest-pinned config schema.

use std::sync::Arc;

use anyhow::{Context, anyhow};
use serde_json::Value;

use crate::tched_router_runtime::ConfigSecretPolicy;

pub(crate) struct PinnedConfigSecretPolicy {
    document: Arc<Value>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Classification {
    Applicable(bool),
    NotApplicable,
}

impl PinnedConfigSecretPolicy {
    pub(crate) fn new(document: Arc<Value>) -> anyhow::Result<Self> {
        let policy = Self { document };
        let root = policy
            .resolve_one_ref(&policy.document)?
            .unwrap_or(&policy.document);
        anyhow::ensure!(
            root.get("type").and_then(Value::as_str) == Some("object"),
            "pinned tched_router config contract is not an object schema"
        );
        anyhow::ensure!(
            root.get("properties").and_then(Value::as_object).is_some(),
            "pinned tched_router config contract has no properties"
        );
        Ok(policy)
    }

    fn resolve_one_ref<'a>(&'a self, schema: &'a Value) -> anyhow::Result<Option<&'a Value>> {
        let Some(reference) = schema.get("$ref").and_then(Value::as_str) else {
            return Ok(None);
        };
        let pointer = reference
            .strip_prefix('#')
            .context("config contract contains a non-local reference")?;
        self.document
            .pointer(pointer)
            .map(Some)
            .with_context(|| format!("config contract reference '{reference}' is unresolved"))
    }

    fn classify(
        &self,
        schema: &Value,
        segments: &[String],
        depth: usize,
    ) -> anyhow::Result<Classification> {
        anyhow::ensure!(depth <= 64, "config contract nesting exceeds 64 levels");
        if schema.get("x-secret").and_then(Value::as_bool) == Some(true) {
            return Ok(Classification::Applicable(true));
        }

        let mut branch_applied = false;
        for combinator in ["allOf", "anyOf", "oneOf"] {
            if let Some(branches) = schema.get(combinator).and_then(Value::as_array) {
                for branch in branches {
                    match self.classify(branch, segments, depth + 1)? {
                        Classification::Applicable(true) => {
                            return Ok(Classification::Applicable(true));
                        }
                        Classification::Applicable(false) => branch_applied = true,
                        Classification::NotApplicable => {}
                    }
                }
            }
        }

        if let Some(referenced) = self.resolve_one_ref(schema)? {
            match self.classify(referenced, segments, depth + 1)? {
                Classification::Applicable(true) => {
                    return Ok(Classification::Applicable(true));
                }
                Classification::Applicable(false) => branch_applied = true,
                Classification::NotApplicable => {}
            }
        }
        if segments.is_empty() {
            return Ok(Classification::Applicable(false));
        }

        let (head, tail) = segments.split_first().expect("checked nonempty");
        if let Some(property) = schema.get("properties").and_then(|v| v.get(head)) {
            return self.classify(property, tail, depth + 1);
        }
        if let Some(additional) = schema.get("additionalProperties") {
            if additional == &Value::Bool(false) {
                return Ok(if branch_applied {
                    Classification::Applicable(false)
                } else {
                    Classification::NotApplicable
                });
            }
            if additional.is_object() {
                return self.classify(additional, tail, depth + 1);
            }
        }
        if let Some(items) = schema.get("items").filter(|items| items.is_object()) {
            // Native Configurable arrays may route elements by natural key
            // (for example mcp.servers.<name>). Every element has the same
            // item contract, whether addressed by name or by its JSON index.
            // This classifies sensitivity only; native property lookup still
            // decides whether the supplied address exists and is writable.
            return self.classify(items, tail, depth + 1);
        }
        Ok(if branch_applied {
            Classification::Applicable(false)
        } else {
            Classification::NotApplicable
        })
    }
}

impl ConfigSecretPolicy for PinnedConfigSecretPolicy {
    fn is_secret(&self, path: &str) -> anyhow::Result<bool> {
        anyhow::ensure!(!path.is_empty(), "config path is empty");
        match self.classify(
            &self.document,
            &path.split('.').map(str::to_string).collect::<Vec<_>>(),
            0,
        )? {
            Classification::Applicable(secret) => Ok(secret),
            Classification::NotApplicable => {
                Err(anyhow!("config path is absent from pinned contract"))
            }
        }
    }

    fn redact_config(&self, config: &mut Value) -> anyhow::Result<()> {
        fn redact_secret(value: &mut Value) -> anyhow::Result<()> {
            match value {
                Value::String(text) => *text = "***MASKED***".to_owned(),
                Value::Null => {}
                Value::Object(fields) => {
                    for child in fields.values_mut() {
                        redact_secret(child)?;
                    }
                }
                Value::Array(items) => {
                    for child in items {
                        redact_secret(child)?;
                    }
                }
                _ => anyhow::bail!("secret config value has invalid type"),
            }
            Ok(())
        }
        fn walk(
            policy: &PinnedConfigSecretPolicy,
            value: &mut Value,
            path: &mut Vec<String>,
        ) -> anyhow::Result<()> {
            if !path.is_empty() {
                match policy.classify(&policy.document, path, 0) {
                    Ok(Classification::Applicable(true)) => {
                        // x-secret also annotates typed maps (MCP env/headers).
                        // Preserve their schema shape and mask all descendants.
                        return redact_secret(value);
                    }
                    Ok(Classification::Applicable(false)) => {}
                    Ok(Classification::NotApplicable) | Err(_)
                        if value.is_object() || value.is_array() => {}
                    Ok(Classification::NotApplicable) | Err(_) => {
                        // Opaque JSON options can contain scalars of any type.
                        // None is proven public: mask them just like unknown
                        // strings rather than reject the entire read snapshot.
                        // This affects only the returned projection; property
                        // access and mutation still require is_secret to resolve.
                        *value = Value::String("***MASKED***".to_owned());
                        return Ok(());
                    }
                }
            }
            match value {
                Value::Object(object) => {
                    for (key, nested) in object {
                        path.push(key.clone());
                        walk(policy, nested, path)?;
                        path.pop();
                    }
                }
                Value::Array(items) => {
                    for (index, nested) in items.iter_mut().enumerate() {
                        path.push(index.to_string());
                        walk(policy, nested, path)?;
                        path.pop();
                    }
                }
                _ => {}
            }
            Ok(())
        }
        anyhow::ensure!(config.is_object(), "config response is not an object");
        walk(self, config, &mut Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn policy() -> PinnedConfigSecretPolicy {
        PinnedConfigSecretPolicy::new(Arc::new(json!({
            "type":"object",
            "properties": {
                "public": {"type":"string"},
                "providers": {"$ref":"#/$defs/Providers"}
            },
            "$defs": {
                "Providers": {"type":"object", "additionalProperties": {"$ref":"#/$defs/Profile"}},
                "Profile": {"type":"object", "properties": {
                    "model":{"type":"string"},
                    "api_key":{"type":["string","null"], "x-secret":true}
                }}
            }
        })))
        .unwrap()
    }

    #[test]
    fn resolves_dynamic_map_secret_from_pinned_contract() {
        let policy = policy();
        assert!(!policy.is_secret("public").unwrap());
        assert!(!policy.is_secret("providers.work.model").unwrap());
        assert!(policy.is_secret("providers.work.api_key").unwrap());
        assert!(policy.is_secret("providers.work.missing").is_err());
    }

    #[test]
    fn redacts_declared_and_unknown_string_values_fail_closed() {
        let policy = policy();
        let mut config = json!({
            "public":"visible",
            "providers":{"work":{"model":"m", "api_key":"secret", "opaque":"unknown"}}
        });
        policy.redact_config(&mut config).unwrap();
        assert_eq!(config["public"], "visible");
        assert_eq!(config["providers"]["work"]["model"], "m");
        assert_eq!(config["providers"]["work"]["api_key"], "***MASKED***");
        assert_eq!(config["providers"]["work"]["opaque"], "***MASKED***");
    }

    #[test]
    fn resolves_secrets_in_the_pinned_v084_contract() {
        op_plugins::state_plugins::threetched::prepare_schema_capture().expect("explicit schema capture");
        let document = op_plugins::state_plugins::tched_router_contract::document().clone();
        let policy = PinnedConfigSecretPolicy::new(Arc::new(document)).unwrap();
        assert!(
            policy
                .is_secret("providers.models.openai.main.api_key")
                .unwrap()
        );
        assert!(
            !policy
                .is_secret("providers.models.openai.main.model")
                .unwrap()
        );
        assert!(!policy.is_secret("mcp.servers.docs.url").unwrap());
        assert!(!policy.is_secret("mcp.servers.0.url").unwrap());
        assert!(
            policy
                .is_secret("mcp.servers.docs.headers.Authorization")
                .unwrap()
        );
        assert!(policy.is_secret("mcp.servers.docs.env.API_TOKEN").unwrap());
        assert!(
            policy
                .is_secret("mcp.servers.docs.unknown_property")
                .is_err()
        );
        let mut config = json!({"mcp":{"servers":[{
            "name":"docs", "url":"https://docs.example.test/mcp",
            "headers":{"Authorization":"Bearer private"}, "env":{}
        }]}});
        policy.redact_config(&mut config).unwrap();
        assert_eq!(
            config["mcp"]["servers"][0]["headers"]["Authorization"],
            "***MASKED***"
        );
        assert_eq!(config["mcp"]["servers"][0]["env"], json!({}));
        assert_eq!(config["mcp"]["servers"][0]["name"], "docs");
    }

    #[test]
    fn natural_key_arrays_preserve_item_secret_classification_and_redaction() {
        let policy = PinnedConfigSecretPolicy::new(Arc::new(json!({
            "type":"object", "properties":{"servers":{
                "type":"array", "items":{"$ref":"#/$defs/Server"}
            }},
            "$defs":{"Server":{"type":"object", "properties":{
                "name":{"type":"string"},
                "token":{"type":"string", "x-secret":true}
            }}}
        })))
        .unwrap();
        assert!(policy.is_secret("servers.docs.token").unwrap());
        assert!(policy.is_secret("servers.0.token").unwrap());
        assert!(policy.is_secret("servers.docs.unknown").is_err());
        let mut config = json!({"servers":[{"name":"docs", "token":"private"}]});
        policy.redact_config(&mut config).unwrap();
        assert_eq!(config["servers"][0]["name"], "docs");
        assert_eq!(config["servers"][0]["token"], "***MASKED***");
    }

    #[test]
    fn reference_siblings_and_target_combinators_cannot_hide_secrets() {
        let policy = PinnedConfigSecretPolicy::new(Arc::new(json!({
            "type":"object",
            "properties": {
                "sibling": {"$ref":"#/$defs/Text", "x-secret":true},
                "branch": {"oneOf":[
                    {"type":"string"},
                    {"type":"string", "x-secret":true}
                ]}
            },
            "$defs":{"Text":{"type":"string"}}
        })))
        .unwrap();
        assert!(policy.is_secret("sibling").unwrap());
        assert!(policy.is_secret("branch").unwrap());
    }

    #[test]
    fn broken_combinator_reference_fails_closed() {
        let policy = PinnedConfigSecretPolicy::new(Arc::new(json!({
            "type":"object",
            "properties": {
                "ambiguous": {"anyOf":[
                    {"type":"string"},
                    {"$ref":"#/$defs/Missing"}
                ]}
            },
            "$defs":{}
        })))
        .unwrap();
        assert!(policy.is_secret("ambiguous").is_err());
        let mut config = json!({"ambiguous":"must-not-leave"});
        policy.redact_config(&mut config).unwrap();
        assert_eq!(config["ambiguous"], "***MASKED***");
    }

    #[test]
    fn nullable_nonapplicable_branch_does_not_hide_typed_children() {
        let policy = PinnedConfigSecretPolicy::new(Arc::new(json!({
            "type":"object",
            "properties": {
                "optional": {"anyOf":[
                    {"type":"null"},
                    {"type":"object", "properties": {
                        "count":{"type":"integer"},
                        "token":{"type":["string","null"], "x-secret":true}
                    }, "additionalProperties":false}
                ]}
            }
        })))
        .unwrap();
        assert!(!policy.is_secret("optional.count").unwrap());
        assert!(policy.is_secret("optional.token").unwrap());
        let mut config = json!({"optional":{"count":7,"token":"secret"}});
        policy.redact_config(&mut config).unwrap();
        assert_eq!(config["optional"]["count"], 7);
        assert_eq!(config["optional"]["token"], "***MASKED***");
    }

    #[test]
    fn intermediate_reference_secret_sibling_is_retained() {
        let policy = PinnedConfigSecretPolicy::new(Arc::new(json!({
            "type":"object",
            "properties":{"credential":{"$ref":"#/$defs/A"}},
            "$defs": {
                "A":{"$ref":"#/$defs/B", "x-secret":true},
                "B":{"type":"string"}
            }
        })))
        .unwrap();
        assert!(policy.is_secret("credential").unwrap());
    }

    #[test]
    fn local_references_use_json_pointer_escaping() {
        let policy = PinnedConfigSecretPolicy::new(Arc::new(json!({
            "type":"object",
            "properties":{"credential":{"$ref":"#/$defs/A~1B~0C"}},
            "$defs":{"A/B~C":{"type":"string", "x-secret":true}}
        })))
        .unwrap();
        assert!(policy.is_secret("credential").unwrap());
    }

    #[test]
    fn root_local_reference_keeps_the_exact_self_contained_contract() {
        let policy = PinnedConfigSecretPolicy::new(Arc::new(json!({
            "$ref":"#/$defs/Root",
            "$defs":{
                "Root":{"type":"object","properties":{
                    "credential":{"type":"string","x-secret":true}
                }}
            }
        })))
        .unwrap();
        assert!(policy.is_secret("credential").unwrap());
    }

    #[test]
    fn unknown_scalars_are_masked_without_becoming_accessible() {
        let policy = policy();
        let mut config = json!({
            "public":"visible",
            "unknown":[42, true, false, null, "private", {"nested":17}]
        });
        policy.redact_config(&mut config).unwrap();
        assert_eq!(config["public"], "visible");
        assert_eq!(
            config["unknown"],
            json!([
                "***MASKED***", "***MASKED***", "***MASKED***", "***MASKED***",
                "***MASKED***", {"nested":"***MASKED***"}
            ])
        );
        assert!(policy.is_secret("unknown.0").is_err());
        assert!(policy.is_secret("unknown.5.nested").is_err());
    }

    #[test]
    fn opaque_provider_options_do_not_block_catalog_reads_or_expose_values() {
        op_plugins::state_plugins::threetched::prepare_schema_capture().expect("explicit schema capture");
        let document = op_plugins::state_plugins::tched_router_contract::document().clone();
        let policy = PinnedConfigSecretPolicy::new(Arc::new(document)).unwrap();
        for enabled in [false, true] {
            let mut config = json!({"providers":{"models":{"custom":{"salad":{
                "model":"example-model", "api_key":"private",
                "provider_extra":{"chat_template_kwargs":{
                    "enable_thinking":enabled, "token":"private", "limit":123
                }}
            }}}}});
            policy.redact_config(&mut config).unwrap();
            let model = &config["providers"]["models"]["custom"]["salad"];
            assert_eq!(model["model"], "example-model");
            assert_eq!(model["api_key"], "***MASKED***");
            assert_eq!(
                model["provider_extra"]["chat_template_kwargs"],
                json!({
                    "enable_thinking":"***MASKED***", "token":"***MASKED***",
                    "limit":"***MASKED***"
                })
            );
        }
        assert!(policy
            .is_secret(
                "providers.models.custom.salad.provider_extra.chat_template_kwargs.enable_thinking"
            )
            .is_err());
    }

    #[test]
    fn unresolved_schema_scalars_are_masked_and_declared_invalid_secrets_still_fail() {
        let policy = PinnedConfigSecretPolicy::new(Arc::new(json!({
            "type":"object", "properties":{
                "broken":{"$ref":"#/$defs/Missing"},
                "secret":{"type":"string", "x-secret":true}
            }
        })))
        .unwrap();
        let mut config = json!({"broken":42});
        policy.redact_config(&mut config).unwrap();
        assert_eq!(config["broken"], "***MASKED***");
        assert!(policy.is_secret("broken").is_err());
        assert!(policy.redact_config(&mut json!({"secret":42})).is_err());
    }

    #[test]
    fn redaction_preserves_literal_dynamic_key_segments() {
        let policy = policy();
        let mut config = json!({
            "providers":{"work.profile":{"api_key":"secret", "model":"m"}}
        });
        policy.redact_config(&mut config).unwrap();
        assert_eq!(
            config["providers"]["work.profile"]["api_key"],
            "***MASKED***"
        );
        assert_eq!(config["providers"]["work.profile"]["model"], "m");
    }
}
