//! The 3tched chat Canvas contract: the components a chat reply's canvas, a
//! saved gallery candidate and a promoted catalog entry are composed from.
//!
//! The UI owns its Canvas catalog and exports it; the importer embeds that
//! exact export in `json_render_chat_canvas_contract.rs`, and this module
//! turns it into the sealed `chat_canvas_profile` field and its validator.
//! It is distinct from `canvas_profile`, the interactive Gemma workspace
//! profile, and it admits no actions, bindings or state: a chat canvas is a
//! static composition.
use super::ComponentDecl;
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

#[path = "json_render_chat_canvas_contract.rs"]
mod contract;

pub const PROFILE_ID: &str = "3tched-chat-canvas-v1";
/// Host controls that never appear inside a generated canvas.
const HOST_COMPONENTS: [&str; 3] = [
    "antigravityChatContainer",
    "chatRouterPicker",
    "chatMessage",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ChatCanvasEvidence {
    pub repository: String,
    pub source_revision: String,
    /// sha256 of the imported export document.
    pub profile_sha256: String,
    /// Always source_checked: sealing is not a deployment attestation.
    pub assurance: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ChatCanvasProfile {
    pub id: String,
    pub evidence: ChatCanvasEvidence,
    pub components: Vec<ComponentDecl>,
    /// Largest serialized canvas.
    pub max_bytes: u64,
    pub max_elements: u64,
    /// Deepest element nesting below the root.
    pub max_depth: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExportDocument {
    format: String,
    limits: ExportLimits,
    components: Vec<ExportComponent>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExportLimits {
    max_bytes: u64,
    max_elements: u64,
    max_depth: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExportComponent {
    name: String,
    description: String,
    slots: Vec<String>,
    props_schema: Value,
}

impl ChatCanvasProfile {
    fn imported() -> Result<Self> {
        let document: ExportDocument = serde_json::from_str(contract::PROFILE_JSON)
            .context("imported chat Canvas export is not a canvas profile document")?;
        ensure!(
            document.format == "3tched.canvas-profile.v1",
            "unsupported chat Canvas export format"
        );
        let components = document
            .components
            .into_iter()
            .map(|component| ComponentDecl {
                props: component
                    .props_schema
                    .get("properties")
                    .and_then(Value::as_object)
                    .map(|properties| properties.keys().cloned().collect())
                    .unwrap_or_default(),
                name: component.name,
                category: "chat_canvas".into(),
                props_schema: component.props_schema,
                slots: component.slots,
                events: vec![],
                description: component.description,
            })
            .collect();
        Ok(Self {
            id: PROFILE_ID.into(),
            evidence: ChatCanvasEvidence {
                repository: contract::SOURCE_REPOSITORY.into(),
                source_revision: contract::SOURCE_REVISION.into(),
                profile_sha256: contract::PROFILE_SHA256.into(),
                assurance: "source_checked".into(),
            },
            components,
            max_bytes: document.limits.max_bytes,
            max_elements: document.limits.max_elements,
            max_depth: document.limits.max_depth,
        })
    }
}

impl Default for ChatCanvasProfile {
    fn default() -> Self {
        Self::imported().expect("the imported chat Canvas export is a valid profile")
    }
}

/// Immutable validator for one chat Canvas profile, normally the one sealed
/// in the manifest-pinned json_render blob.
pub struct ChatCanvasValidator {
    profile: ChatCanvasProfile,
    props: BTreeMap<String, jsonschema::Validator>,
}

impl ChatCanvasValidator {
    pub fn new(profile: ChatCanvasProfile) -> Result<Self> {
        ensure!(profile.id == PROFILE_ID, "unsupported chat Canvas profile");
        ensure!(
            profile.evidence.assurance == "source_checked",
            "invalid chat Canvas evidence kind"
        );
        ensure!(
            (1..=1024 * 1024).contains(&profile.max_bytes)
                && (1..=4096).contains(&profile.max_elements)
                && (1..=64).contains(&profile.max_depth),
            "invalid chat Canvas bounds"
        );
        ensure!(
            !profile.components.is_empty() && profile.components.len() <= 256,
            "invalid chat Canvas component set"
        );
        let mut props = BTreeMap::new();
        for component in &profile.components {
            ensure!(
                !HOST_COMPONENTS.contains(&component.name.as_str()),
                "host control cannot appear in a chat Canvas"
            );
            ensure!(
                component.events.is_empty(),
                "chat Canvas components carry no events"
            );
            ensure!(
                component.slots.iter().all(|slot| slot == "default"),
                "unsupported chat Canvas slot"
            );
            super::validation_bounds::check_json(
                &component.props_schema,
                super::validation_bounds::JsonLimits {
                    max_depth: 32,
                    max_nodes: 4096,
                    max_bytes: 65_536,
                },
            )?;
            // Inline schemas only: no reference may become an I/O or
            // expansion source while the validator is built.
            let mut pending = vec![&component.props_schema];
            while let Some(value) = pending.pop() {
                match value {
                    Value::Object(object) => {
                        ensure!(
                            !object.contains_key("$ref") && !object.contains_key("$dynamicRef"),
                            "chat Canvas props must not contain schema references"
                        );
                        pending.extend(object.values());
                    }
                    Value::Array(array) => pending.extend(array),
                    _ => {}
                }
            }
            ensure!(
                component.props_schema.get("additionalProperties") == Some(&Value::Bool(false)),
                "chat Canvas props must be closed"
            );
            let validator = jsonschema::validator_for(&component.props_schema)
                .map_err(|_| anyhow::anyhow!("invalid chat Canvas props schema"))?;
            ensure!(
                props.insert(component.name.clone(), validator).is_none(),
                "duplicate chat Canvas component"
            );
        }
        Ok(Self { profile, props })
    }

    /// The validator for the profile sealed in a manifest-pinned json_render
    /// schema. The caller establishes catalog membership first.
    pub fn from_pinned_schema_json(schema_json: &str) -> Result<Self> {
        let schema: op_state_store::PluginSchema = serde_json::from_str(schema_json)
            .map_err(|_| anyhow::anyhow!("invalid pinned renderer schema"))?;
        ensure!(
            schema.name == "json_render",
            "pinned schema is not json_render"
        );
        let field = schema
            .fields
            .get("chat_canvas_profile")
            .context("sealed chat Canvas profile absent")?;
        let profile: ChatCanvasProfile = serde_json::from_value(serde_json::to_value(
            field
                .default
                .as_ref()
                .context("sealed chat Canvas profile has no authored value")?,
        )?)
        .map_err(|_| anyhow::anyhow!("invalid sealed chat Canvas profile"))?;
        Self::new(profile)
    }

    pub fn profile(&self) -> &ChatCanvasProfile {
        &self.profile
    }

    /// The UI's own admission rule: exactly `{root, elements}`, flat element
    /// IDs, closed declared props, children by ID, and one tree reachable
    /// from the root with no cycle, shared child or orphan.
    pub fn validate(&self, spec: &Value) -> Result<()> {
        let root_object = spec.as_object().context("Canvas must be an object")?;
        ensure!(
            root_object.len() == 2
                && root_object.contains_key("root")
                && root_object.contains_key("elements"),
            "Canvas must contain only root and elements"
        );
        ensure!(
            serde_json::to_vec(spec)?.len() as u64 <= self.profile.max_bytes,
            "Canvas exceeds its size limit"
        );
        let root = root_object["root"]
            .as_str()
            .context("Canvas root must be a string")?;
        let elements = root_object["elements"]
            .as_object()
            .context("Canvas elements must be an object")?;
        ensure!(
            !elements.is_empty() && elements.len() as u64 <= self.profile.max_elements,
            "Canvas exceeds its size limit"
        );
        for (id, element) in elements {
            valid_id(id)?;
            let object = element
                .as_object()
                .context("Canvas elements must be static presentation elements")?;
            ensure!(
                object.contains_key("type")
                    && object.contains_key("props")
                    && object
                        .keys()
                        .all(|key| matches!(key.as_str(), "type" | "props" | "children")),
                "Canvas elements must be static presentation elements"
            );
            let name = object["type"]
                .as_str()
                .context("Canvas elements must be static presentation elements")?;
            let validator = self
                .props
                .get(name)
                .with_context(|| format!("Invalid Canvas component: {name}"))?;
            ensure!(
                object["props"].is_object() && validator.is_valid(&object["props"]),
                "Invalid Canvas component: {name}"
            );
            if let Some(children) = object.get("children") {
                let children = children
                    .as_array()
                    .context("Canvas children must be element IDs")?;
                ensure!(
                    children.iter().all(Value::is_string),
                    "Canvas children must be element IDs"
                );
            }
        }
        let mut seen = BTreeSet::new();
        let mut pending = vec![(root, 0u64)];
        while let Some((id, depth)) = pending.pop() {
            ensure!(
                depth <= self.profile.max_depth && seen.insert(id) && elements.contains_key(id),
                "Canvas graph is missing a child, cyclic, shared, or too deep"
            );
            if let Some(children) = elements[id].get("children").and_then(Value::as_array) {
                pending.extend(
                    children
                        .iter()
                        .filter_map(Value::as_str)
                        .map(|child| (child, depth + 1)),
                );
            }
        }
        ensure!(
            seen.len() == elements.len(),
            "Canvas has unreachable elements"
        );
        Ok(())
    }
}

fn valid_id(id: &str) -> Result<()> {
    ensure!(
        !id.is_empty()
            && id.len() <= 80
            && id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-')),
        "Invalid Canvas element ID"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn validator() -> ChatCanvasValidator {
        ChatCanvasValidator::new(ChatCanvasProfile::default()).unwrap()
    }

    fn sample() -> Value {
        json!({
            "root": "page",
            "elements": {
                "page": {"type": "container", "props": {"className": null}, "children": ["title", "metric"]},
                "title": {"type": "heading", "props": {"text": "Proposal", "level": 2}},
                "metric": {"type": "stat", "props": {"label": "Nodes", "value": "4", "unit": null, "trend": null}}
            }
        })
    }

    #[test]
    fn imported_profile_is_the_ui_export() {
        let profile = ChatCanvasProfile::default();
        assert_eq!(profile.id, PROFILE_ID);
        assert_eq!(profile.evidence.profile_sha256.len(), 64);
        assert!(!profile.evidence.source_revision.contains("modified"));
        for name in [
            "container",
            "heading",
            "stat",
            "dashboardPanel",
            "dashboardTable",
        ] {
            assert!(
                profile
                    .components
                    .iter()
                    .any(|component| component.name == name),
                "{name} is in the chat Canvas"
            );
        }
        use sha2::Digest;
        assert_eq!(
            format!(
                "{:x}",
                sha2::Sha256::digest(contract::PROFILE_JSON.as_bytes())
            ),
            contract::PROFILE_SHA256
        );
    }

    #[test]
    fn accepts_a_catalog_composition() {
        validator().validate(&sample()).unwrap();
    }

    #[test]
    fn rejects_what_the_ui_rejects() {
        let validator = validator();
        let mut cases = Vec::new();
        let mut extra = sample();
        extra["state"] = json!({});
        cases.push(extra);
        let mut action = sample();
        action["elements"]["title"]["on"] = json!({"press": {"action": "setState"}});
        cases.push(action);
        let mut undeclared = sample();
        undeclared["elements"]["title"]["props"]["html"] = json!("<script>");
        cases.push(undeclared);
        let mut unknown = sample();
        unknown["elements"]["title"]["type"] = json!("chatRouterPicker");
        cases.push(unknown);
        let mut orphan = sample();
        orphan["elements"]["loose"] =
            json!({"type": "text", "props": {"text": "x", "className": null}});
        cases.push(orphan);
        let mut cycle = sample();
        cycle["elements"]["metric"]["children"] = json!(["page"]);
        cases.push(cycle);
        let mut missing = sample();
        missing["elements"]["page"]["children"] = json!(["title", "absent"]);
        cases.push(missing);
        let mut bad_id = sample();
        bad_id["root"] = json!("page one");
        cases.push(bad_id);
        for case in cases {
            assert!(validator.validate(&case).is_err(), "{case}");
        }
    }

    #[test]
    fn host_controls_and_open_props_are_refused() {
        let mut profile = ChatCanvasProfile::default();
        profile.components[0].name = "chatRouterPicker".into();
        assert!(ChatCanvasValidator::new(profile).is_err());
        let mut profile = ChatCanvasProfile::default();
        profile.components[0].props_schema["additionalProperties"] = json!(true);
        assert!(ChatCanvasValidator::new(profile).is_err());
    }
}
