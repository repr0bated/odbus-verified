//! Whole-application Svelte contract authored in the UI repository and imported verbatim.
//! Static gallery and legacy workspace profiles remain separate consumers.
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ApplicationProfile {
    pub format: String,
    pub limits: Limits,
    pub components: Vec<Component>,
    pub actions: Vec<Action>,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Limits { pub max_bytes: usize, pub max_elements: usize, pub max_depth: usize }
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Component {
    pub name: String,
    pub description: String,
    pub slots: Vec<String>,
    pub events: Vec<String>,
    pub props_schema: Value,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required_bindings: Vec<String>,
    /// UI-authored cross-field constraints, applied after resolving state bindings.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub string_item_fields: BTreeMap<String, Vec<String>>,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Action { pub name: String, pub description: String, pub params_schema: Value }
impl Default for ApplicationProfile {
    fn default() -> Self {
        serde_json::from_str(include_str!("json_render_application_profile.json"))
            .expect("UI-authored application profile must be valid")
    }
}

pub struct ApplicationValidator {
    profile: ApplicationProfile,
    props: BTreeMap<String, jsonschema::Validator>,
    actions: BTreeMap<String, jsonschema::Validator>,
}
fn compile(schema: &Value) -> Result<jsonschema::Validator> {
    super::validation_bounds::check_json(schema, super::validation_bounds::JsonLimits {
        max_depth: 32, max_nodes: 4096, max_bytes: 65_536,
    })?;
    let mut pending = vec![schema];
    while let Some(value) = pending.pop() {
        match value {
            Value::Object(object) => {
                ensure!(!object.contains_key("$ref") && !object.contains_key("$dynamicRef"), "application schemas must be inline");
                pending.extend(object.values());
            }
            Value::Array(array) => pending.extend(array),
            _ => {}
        }
    }
    ensure!(schema["additionalProperties"] == false, "application schemas must be closed");
    jsonschema::validator_for(schema).map_err(|_| anyhow::anyhow!("invalid application schema"))
}
impl ApplicationValidator {
    pub fn new(profile: ApplicationProfile) -> Result<Self> {
        ensure!(profile.format == "3tched.application-profile.v1", "unsupported application profile");
        ensure!((1..=200_000).contains(&profile.limits.max_bytes)
            && (1..=256).contains(&profile.limits.max_elements)
            && (1..=20).contains(&profile.limits.max_depth), "invalid application limits");
        ensure!(!profile.components.is_empty() && profile.components.len() <= 256, "invalid application component set");
        let mut props = BTreeMap::new();
        for component in &profile.components {
            ensure!(component.slots.iter().all(|slot| slot == "default"), "unsupported application slot");
            ensure!(props.insert(component.name.clone(), compile(&component.props_schema)?).is_none(), "duplicate application component");
        }
        let mut actions = BTreeMap::new();
        for action in &profile.actions {
            ensure!(matches!(action.name.as_str(), "setState" | "invokePlugin"), "unsupported application action");
            ensure!(actions.insert(action.name.clone(), compile(&action.params_schema)?).is_none(), "duplicate application action");
        }
        ensure!(actions.len() == 2, "application action contract incomplete");
        Ok(Self { profile, props, actions })
    }
    pub fn from_pinned_schema_json(json: &str) -> Result<Self> {
        let schema: op_state_store::PluginSchema = serde_json::from_str(json)?;
        ensure!(schema.name == "json_render", "pinned schema is not json_render");
        let field = schema.fields.get("application_profile").context("sealed application profile absent")?;
        Self::new(serde_json::from_value(serde_json::to_value(field.default.as_ref().context("application profile has no value")?)?)?)
    }
    pub fn profile(&self) -> &ApplicationProfile { &self.profile }
    pub fn validate(&self, spec: &Value) -> Result<()> {
        self.validate_with_targets(spec, &|_, _, _, _, _| anyhow::bail!("target contract resolver required"))
    }
    pub fn validate_with_targets(
        &self,
        spec: &Value,
        target: &impl Fn(&str, &str, &str, &str, &Value) -> Result<()>,
    ) -> Result<()> {
        super::validation_bounds::check_json(spec, super::validation_bounds::JsonLimits {
            max_depth: 64, max_nodes: 32_768, max_bytes: self.profile.limits.max_bytes,
        })?;
        let object = spec.as_object().context("application must be an object")?;
        ensure!(object.keys().all(|key| matches!(key.as_str(), "root" | "elements" | "state" | "target_contracts")), "unsupported application field");
        let root = spec["root"].as_str().context("application root missing")?;
        let elements = spec["elements"].as_object().context("application elements missing")?;
        ensure!(!elements.is_empty() && elements.len() <= self.profile.limits.max_elements, "application element bound exceeded");
        let empty = serde_json::json!({});
        let state = object.get("state").unwrap_or(&empty);
        ensure!(state.is_object(), "application state must be an object");
        if let Some(pins) = object.get("target_contracts") {
            let pins = pins.as_object().context("target pins must be an object")?;
            ensure!(pins.len() <= 256 && pins.iter().all(|(plugin, hash)| !plugin.is_empty()
                && hash.as_str().is_some_and(|hash| !hash.is_empty() && hash.len() <= 256)), "invalid target pins");
        }
        for (id, element) in elements {
            ensure!(!id.is_empty() && id.len() <= 80 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-'), "invalid application element id");
            let element = element.as_object().context("invalid application element")?;
            ensure!(element.keys().all(|key| matches!(key.as_str(), "type" | "props" | "children" | "on")), "unsupported application element field");
            let name = element.get("type").and_then(Value::as_str).context("component type missing")?;
            let declaration = self.profile.components.iter().find(|component| component.name == name).context("undeclared application component")?;
            let props = element.get("props").context("component props missing")?;
            let resolved = resolve(props, state)?;
            ensure!(props.is_object() && self.props[name].is_valid(&resolved), "application props rejected");
            for (items, selectors) in &declaration.string_item_fields {
                let items = resolved.get(items).and_then(Value::as_array).context("application option list missing")?;
                for selector in selectors {
                    let field = resolved.get(selector).and_then(Value::as_str).context("application option field missing")?;
                    ensure!(items.iter().all(|item| item.get(field).is_some_and(Value::is_string)),
                        "application options require the declared string fields");
                }
            }
            for property in &declaration.required_bindings {
                ensure!(props.get(property).and_then(Value::as_object).is_some_and(|binding|
                    binding.len() == 1 && binding.get("$bindState").is_some_and(Value::is_string)),
                    "interactive property requires a writable state binding");
            }
            if let Some(children) = element.get("children") {
                let children = children.as_array().context("children must be ids")?;
                ensure!(children.iter().all(Value::is_string) && (!declaration.slots.is_empty() || children.is_empty()), "component has no child slot");
            }
            if let Some(on) = element.get("on") {
                for (event, binding) in on.as_object().context("events must be an object")? {
                    ensure!(declaration.events.contains(event), "undeclared component event");
                    let binding = binding.as_object().context("invalid action binding")?;
                    ensure!(binding.len() == 2 && binding.contains_key("action") && binding.contains_key("params"), "invalid action binding fields");
                    let action = binding["action"].as_str().context("action name missing")?;
                    let params = resolve(&binding["params"], state)?;
                    ensure!(self.actions.get(action).is_some_and(|validator| validator.is_valid(&params)), "undeclared or invalid application action");
                    match action {
                        "setState" => pointer(params["statePath"].as_str().context("state path missing")?)?,
                        "invokePlugin" => {
                            let plugin = params["plugin"].as_str().context("target plugin missing")?;
                            let method = params["method"].as_str().context("target method missing")?;
                            let capability = params["capability"].as_str().context("target capability missing")?;
                            let hash = spec["target_contracts"].get(plugin).and_then(Value::as_str).context("target plugin pin missing")?;
                            target(plugin, method, hash, capability, &params["arguments"])?;
                            if let Some(path) = params.get("resultStatePath").and_then(Value::as_str) { pointer(path)?; }
                        }
                        _ => anyhow::bail!("unsupported application action"),
                    }
                }
            }
        }
        let mut seen = BTreeSet::new();
        let mut pending = vec![(root, 0)];
        while let Some((id, depth)) = pending.pop() {
            ensure!(depth <= self.profile.limits.max_depth && seen.insert(id) && elements.contains_key(id), "application graph is missing, cyclic, shared or too deep");
            if let Some(children) = elements[id].get("children").and_then(Value::as_array) {
                pending.extend(children.iter().map(|child| (child.as_str().expect("checked child"), depth + 1)));
            }
        }
        ensure!(seen.len() == elements.len(), "unreachable application elements");
        Ok(())
    }
}
fn pointer(path: &str) -> Result<()> {
    ensure!(path.starts_with('/') && path.len() <= 1024, "invalid application state pointer");
    for segment in path[1..].split('/') {
        let mut chars = segment.chars();
        while let Some(ch) = chars.next() {
            if ch == '~' { ensure!(matches!(chars.next(), Some('0' | '1')), "invalid state pointer escape"); }
        }
        let decoded = segment.replace("~1", "/").replace("~0", "~");
        ensure!(!decoded.is_empty() && !decoded.chars().any(char::is_control)
            && !matches!(decoded.as_str(), "__proto__" | "prototype" | "constructor"), "unsafe application state pointer");
    }
    Ok(())
}
fn resolve(value: &Value, state: &Value) -> Result<Value> {
    match value {
        Value::Object(object) => {
            if let Some(path) = object.get("$state").or_else(|| object.get("$bindState")) {
                ensure!(object.len() == 1, "invalid application binding");
                let path = path.as_str().context("binding path must be a string")?;
                pointer(path)?;
                return state.pointer(path).cloned().context("application binding state is absent");
            }
            ensure!(object.keys().all(|key| !key.starts_with('$')), "unsupported application expression");
            Ok(Value::Object(object.iter().map(|(key, value)| Ok((key.clone(), resolve(value, state)?))).collect::<Result<_>>()?))
        }
        Value::Array(values) => Ok(Value::Array(values.iter().map(|value| resolve(value, state)).collect::<Result<_>>()?)),
        _ => Ok(value.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn select_checks_named_string_fields_after_resolving_bindings() {
        let validator = ApplicationValidator::new(ApplicationProfile::default()).unwrap();
        let mut spec = json!({
            "root":"select",
            "elements":{"select":{"type":"applicationSelect","props":{
                "label":"Model","value":{"$bindState":"/selected"},
                "options":{"$state":"/options"},"valueField":"id","labelField":"name"
            }}},
            "state":{"selected":"m","options":[{"id":"m","name":"Model"}]}
        });
        assert!(validator.validate(&spec).is_ok());
        for options in [json!([{"id":"m"}]), json!([{"id":7,"name":"Model"}]), json!([{"id":"m","name":null}])] {
            spec["state"]["options"] = options;
            assert!(validator.validate(&spec).is_err());
        }
        spec["state"]["options"] = json!([]);
        assert!(validator.validate(&spec).is_ok());
    }
}
