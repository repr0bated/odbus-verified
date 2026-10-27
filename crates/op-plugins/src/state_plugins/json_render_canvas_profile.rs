//! Plugin-owned static Canvas contract, authored from the active UI checkout.
//! The component declarations below are the sole backend profile definition.
//! Source evidence describes inspected implementation bytes, not deployment.
use super::{
    ActionDecl, ComponentDecl, RendererTarget,
    validation_bounds::{GraphLimits, JsonLimits, check_children_graph, check_json},
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::BTreeMap, sync::Arc};

pub const PROFILE_ID: &str = "vercel-chat-canvas-static-v1";

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CanvasImplementationEvidence {
    pub repository: String,
    pub source_revision: String,
    pub catalog_sha256: String,
    pub registry_sha256: String,
    pub package_lock_sha256: String,
    pub core_version: String,
    pub react_version: String,
    /// Always source_checked: sealing is not a deployment attestation.
    pub assurance: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CanvasProfile {
    pub id: String,
    pub evidence: CanvasImplementationEvidence,
    /// Distinct from the upstream reference inventory in JsonRenderState.components.
    pub components: Vec<ComponentDecl>,
    /// Exact built-in actions implemented by the pinned renderer and admitted
    /// by the server-side event reducer.
    pub actions: Vec<ActionDecl>,
    /// Generated props remain literal; state is changed only through a
    /// validated action binding, never through prop expressions.
    pub literal_props_only: bool,
    pub json_limits: JsonLimits,
    pub graph_limits: GraphLimits,
}

impl Default for CanvasProfile {
    fn default() -> Self {
        Self {
            id: PROFILE_ID.into(),
            evidence: CanvasImplementationEvidence {
                repository: "vercel-json-render-ui".into(),
                source_revision: "e8aad07bf02fc16bbb45554fbc570b66513ec019".into(),
                catalog_sha256: "c0a273cd75243a3297624be9578a2f89540bb20c038e9c2e6be2f01aea2ed011"
                    .into(),
                registry_sha256: "52b254c56350d67c9334815c65c05072bf5f8169b5dee3a93cdb3ff5ea86e61b"
                    .into(),
                package_lock_sha256:
                    "851a6ec0f398cc13300e375aa31dec7d07150de5961dba60eb859d9995412201".into(),
                core_version: "0.20.0".into(),
                react_version: "0.20.0".into(),
                assurance: "source_checked".into(),
            },
            components: components(),
            actions: vec![ActionDecl {
                name: "setState".into(),
                params: vec!["statePath".into(), "value".into()],
                params_schema: serde_json::json!({
                    "$schema":"http://json-schema.org/draft-07/schema#",
                    "type":"object",
                    "properties":{"statePath":{"type":"string"},"value":{}},
                    "required":["statePath","value"],
                    "additionalProperties":false
                }),
                renderers: vec![RendererTarget::React],
                mutates_state: true,
                description: "Set a value at a JSON Pointer in the spec-owned state object.".into(),
            }],
            literal_props_only: true,
            json_limits: JsonLimits {
                max_depth: 64,
                max_nodes: 65_536,
                max_bytes: 1024 * 1024,
            },
            graph_limits: GraphLimits {
                max_elements: 4096,
                max_edges: 16_384,
                max_depth: 64,
                max_expanded_nodes: 4096,
            },
        }
    }
}

/// Immutable validator constructed from an ALREADY manifest-pinned blob's
/// original schema JSON and stored schema hash. The caller must establish
/// catalog membership and compiled-state compatibility before constructing it.
/// This module never hashes, seals, scans a catalog, or consults a registry.
pub struct PinnedCanvasValidator {
    schema_json: Arc<str>,
    schema_hash: String,
    profile: CanvasProfile,
    props: BTreeMap<String, jsonschema::Validator>,
}

impl PinnedCanvasValidator {
    pub fn from_manifest_pinned_schema(schema_json: Arc<str>, schema_hash: String) -> Result<Self> {
        ensure!(
            schema_json.len() <= 16 * 1024 * 1024,
            "renderer schema exceeds size limit"
        );
        ensure!(
            schema_hash.len() == 64
                && schema_hash
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "invalid stored renderer schema hash"
        );
        let schema: op_state_store::PluginSchema = serde_json::from_str(&schema_json)
            .map_err(|_| anyhow::anyhow!("invalid pinned renderer schema"))?;
        ensure!(
            schema.name == "json_render" && schema.version == super::PLUGIN_VERSION,
            "incompatible pinned renderer contract"
        );
        let field = schema
            .fields
            .get("canvas_profile")
            .context("sealed Canvas profile absent")?;
        let profile: CanvasProfile = serde_json::from_value(serde_json::to_value(
            field
                .default
                .as_ref()
                .context("sealed Canvas profile has no authored value")?,
        )?)
        .map_err(|_| anyhow::anyhow!("invalid sealed Canvas profile"))?;
        ensure!(
            profile.id == PROFILE_ID && profile.literal_props_only,
            "unsupported Canvas profile"
        );
        ensure!(
            profile.evidence.assurance == "source_checked",
            "invalid Canvas evidence kind"
        );
        let json = profile.json_limits;
        let graph = profile.graph_limits;
        ensure!(
            json.max_depth > 0
                && json.max_depth <= 64
                && json.max_nodes > 0
                && json.max_nodes <= 65_536
                && json.max_bytes > 0
                && json.max_bytes <= 1024 * 1024,
            "invalid Canvas JSON bounds"
        );
        ensure!(
            graph.max_elements > 0
                && graph.max_elements <= 4096
                && graph.max_edges <= 16_384
                && graph.max_depth > 0
                && graph.max_depth <= 64
                && graph.max_expanded_nodes > 0
                && graph.max_expanded_nodes <= 4096,
            "invalid Canvas graph bounds"
        );
        ensure!(
            profile.components.len() == 8,
            "incomplete static Canvas profile"
        );
        ensure!(
            profile.actions.len() == 1
                && profile.actions[0].name == "setState"
                && profile.actions[0].renderers == [RendererTarget::React]
                && profile.actions[0].mutates_state,
            "unsupported Canvas action profile"
        );
        let mut props = BTreeMap::new();
        for component in &profile.components {
            // The single profile declares props; this structural safety gate
            // prevents widening to hidden host controllers or runtime hooks.
            ensure!(
                !matches!(
                    component.name.as_str(),
                    "antigravityChatContainer" | "chatRouterPicker" | "pageHeader" | "chatMessage"
                ),
                "host shell component cannot appear in generated Canvas"
            );
            ensure!(
                component.slots.iter().all(|slot| slot == "default"),
                "unsupported Canvas slot"
            );
            check_json(
                &component.props_schema,
                JsonLimits {
                    max_depth: 32,
                    max_nodes: 4096,
                    max_bytes: 65_536,
                },
            )?;
            // This authored static profile has inline primitive prop schemas.
            // Never allow a schema retriever (or recursive reference expansion)
            // to become an I/O/work source while constructing the validator.
            let mut pending = vec![&component.props_schema];
            while let Some(value) = pending.pop() {
                match value {
                    Value::Object(object) => {
                        ensure!(
                            !object.contains_key("$ref") && !object.contains_key("$dynamicRef"),
                            "static Canvas props must not contain schema references"
                        );
                        pending.extend(object.values());
                    }
                    Value::Array(array) => pending.extend(array),
                    _ => {}
                }
            }
            ensure!(
                component.props_schema.get("additionalProperties") == Some(&Value::Bool(false)),
                "Canvas props must be closed"
            );
            let validator = jsonschema::validator_for(&component.props_schema)
                .map_err(|_| anyhow::anyhow!("invalid sealed Canvas props schema"))?;
            ensure!(
                props.insert(component.name.clone(), validator).is_none(),
                "duplicate Canvas component"
            );
        }
        Ok(Self {
            schema_json,
            schema_hash,
            profile,
            props,
        })
    }

    pub fn schema_hash(&self) -> &str {
        &self.schema_hash
    }
    pub fn original_schema_json(&self) -> &str {
        &self.schema_json
    }
    pub fn profile(&self) -> &CanvasProfile {
        &self.profile
    }

    pub fn validate(&self, spec: &Value) -> Result<()> {
        check_json(spec, self.profile.json_limits)?;
        let root_object = spec.as_object().context("Canvas spec must be an object")?;
        ensure!(
            root_object.len() <= 3
                && root_object.contains_key("root")
                && root_object.contains_key("elements")
                && root_object
                    .keys()
                    .all(|key| matches!(key.as_str(), "root" | "elements" | "state")),
            "Canvas permits only root, elements and spec-owned state"
        );
        if let Some(state) = root_object.get("state") {
            ensure!(state.is_object(), "Canvas state must be an object");
        }
        let root = root_object["root"]
            .as_str()
            .context("Canvas root must be a string")?;
        valid_id(root)?;
        let elements = root_object["elements"]
            .as_object()
            .context("Canvas elements must be an object")?;
        for (id, element) in elements {
            valid_id(id)?;
            let object = element
                .as_object()
                .context("Canvas element must be an object")?;
            ensure!(
                object.len() <= 4
                    && ["type", "props", "children"]
                        .iter()
                        .all(|name| object.contains_key(*name))
                    && object
                        .keys()
                        .all(|key| matches!(key.as_str(), "type" | "props" | "children" | "on")),
                "Canvas permits only type, literal props, children and event bindings"
            );
            let name = object["type"]
                .as_str()
                .context("Canvas component must be a string")?;
            let validator = self
                .props
                .get(name)
                .context("component is absent from static Canvas profile")?;
            // Do not expose schema-validator diagnostics containing private values.
            ensure!(
                validator.is_valid(&object["props"]),
                "Canvas component props violate sealed contract"
            );
            let component = self
                .profile
                .components
                .iter()
                .find(|component| component.name == name)
                .unwrap();
            if let Some(on) = object.get("on") {
                validate_bindings(on, component)?;
            }
            let children = object["children"]
                .as_array()
                .context("Canvas children must be an array")?;
            ensure!(
                !component.slots.is_empty() || children.is_empty(),
                "Canvas component has no child slot"
            );
            for child in children {
                valid_id(child.as_str().context("Canvas child must be a string")?)?;
            }
        }
        check_children_graph(root, elements, self.profile.graph_limits)
    }

    /// Resolve one caller-named element event through the sealed binding and
    /// apply its sole supported action to spec-owned state.
    pub fn apply_event(&self, spec: &Value, element_id: &str, event: &str) -> Result<Value> {
        self.validate(spec)?;
        valid_id(element_id)?;
        valid_id(event)?;
        let binding = spec["elements"]
            .get(element_id)
            .and_then(|element| element.get("on"))
            .and_then(|on| on.get(event))
            .context("Canvas event binding not found")?;
        let params = binding
            .get("params")
            .and_then(Value::as_object)
            .context("invalid Canvas setState parameters")?;
        let path = params
            .get("statePath")
            .and_then(Value::as_str)
            .context("Canvas setState path is missing")?;
        validate_state_pointer(path)?;
        let mut candidate = spec.clone();
        if candidate.get("state").is_none() {
            candidate["state"] = serde_json::json!({});
        }
        set_state_pointer(
            candidate.get_mut("state").unwrap(),
            path,
            params["value"].clone(),
        )?;
        self.validate(&candidate)?;
        Ok(candidate)
    }
}

fn validate_bindings(value: &Value, component: &ComponentDecl) -> Result<()> {
    let bindings = value
        .as_object()
        .context("Canvas event bindings must be an object")?;
    for (event, binding) in bindings {
        ensure!(
            component.events.contains(event),
            "event is absent from Canvas component contract"
        );
        let object = binding
            .as_object()
            .context("Canvas event binding must be an object")?;
        ensure!(
            object.len() == 2 && object["action"] == "setState" && object.contains_key("params"),
            "unsupported Canvas event action"
        );
        let params = object["params"]
            .as_object()
            .context("Canvas action params must be an object")?;
        ensure!(
            params.len() == 2 && params.contains_key("value"),
            "invalid Canvas setState parameters"
        );
        let path = params
            .get("statePath")
            .and_then(Value::as_str)
            .context("Canvas setState path is missing")?;
        validate_state_pointer(path)?;
    }
    Ok(())
}

fn set_state_pointer(state: &mut Value, pointer: &str, value: Value) -> Result<()> {
    let mut current = state;
    let segments: Vec<_> = pointer[1..].split('/').collect();
    ensure!(
        !segments.is_empty() && segments.len() <= 32,
        "invalid Canvas state path"
    );
    for (index, encoded) in segments.iter().enumerate() {
        ensure!(
            !encoded.is_empty() && encoded.len() <= 128,
            "invalid Canvas state path segment"
        );
        let segment = encoded.replace("~1", "/").replace("~0", "~");
        ensure!(
            !segment.chars().any(char::is_control),
            "invalid Canvas state path segment"
        );
        ensure!(
            !matches!(segment.as_str(), "__proto__" | "prototype" | "constructor"),
            "unsafe Canvas state path segment"
        );
        let object = current
            .as_object_mut()
            .context("Canvas state path crosses a non-object")?;
        if index + 1 == segments.len() {
            object.insert(segment, value);
            return Ok(());
        }
        current = object
            .entry(segment)
            .or_insert_with(|| serde_json::json!({}));
    }
    unreachable!()
}

fn validate_state_pointer(pointer: &str) -> Result<()> {
    ensure!(
        pointer.starts_with('/') && pointer.len() <= 512,
        "invalid Canvas state path"
    );
    let segments: Vec<_> = pointer[1..].split('/').collect();
    ensure!(
        !segments.is_empty() && segments.len() <= 32,
        "invalid Canvas state path"
    );
    for encoded in segments {
        ensure!(
            !encoded.is_empty() && encoded.len() <= 128,
            "invalid Canvas state path segment"
        );
        let mut chars = encoded.chars();
        while let Some(ch) = chars.next() {
            if ch == '~' {
                ensure!(
                    matches!(chars.next(), Some('0' | '1')),
                    "invalid Canvas state pointer escape"
                );
            }
        }
        let segment = encoded.replace("~1", "/").replace("~0", "~");
        ensure!(
            !segment.chars().any(char::is_control),
            "invalid Canvas state path segment"
        );
        ensure!(
            !matches!(segment.as_str(), "__proto__" | "prototype" | "constructor"),
            "unsafe Canvas state path segment"
        );
    }
    Ok(())
}

fn valid_id(id: &str) -> Result<()> {
    ensure!(
        !id.is_empty()
            && id.len() <= 128
            && id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-')),
        "invalid Canvas element identifier"
    );
    Ok(())
}

#[cfg(test)]
#[path = "json_render_canvas_profile_tests.rs"]
mod tests;

// Authored from the exact chatCatalog declarations checked against chatRegistry.
// This lives in the plugin definition, not a runtime schema capture.
fn components() -> Vec<ComponentDecl> {
    vec![
        ComponentDecl {
            name: "container".into(),
            category: "static_canvas".into(),
            props: vec!["className".into()],
            props_schema: serde_json::json!({"$schema":"http://json-schema.org/draft-07/schema#","type":"object","properties":{"className":{"anyOf":[{"type":"string"},{"type":"null"}]}},"required":["className"],"additionalProperties":false}),
            slots: vec!["default".into()],
            events: vec![],
            description: "Vertical stack of children.".into(),
        },
        ComponentDecl {
            name: "autoTile".into(),
            category: "static_canvas".into(),
            props: vec![
                "columns".into(),
                "gap".into(),
                "rowHeight".into(),
                "boardId".into(),
                "reorderable".into(),
                "fit".into(),
                "className".into(),
            ],
            props_schema: serde_json::json!({"$schema":"http://json-schema.org/draft-07/schema#","type":"object","properties":{"columns":{"anyOf":[{"type":"number"},{"type":"null"}]},"gap":{"anyOf":[{"type":"number"},{"type":"null"}]},"rowHeight":{"anyOf":[{"type":"number"},{"type":"null"}]},"boardId":{"anyOf":[{"type":"string"},{"type":"null"}]},"reorderable":{"anyOf":[{"type":"boolean"},{"type":"null"}]},"fit":{"anyOf":[{"type":"string","enum":["content","hint"]},{"type":"null"}]},"className":{"anyOf":[{"type":"string"},{"type":"null"}]}},"required":["columns","gap","rowHeight","boardId","reorderable","fit","className"],"additionalProperties":false}),
            slots: vec!["default".into()],
            events: vec!["reorder".into()],
            description:
                "Dense golden-ratio tile board with local pointer and keyboard reordering.".into(),
        },
        ComponentDecl {
            name: "autoTileItem".into(),
            category: "static_canvas".into(),
            props: vec![
                "size".into(),
                "contentType".into(),
                "tileId".into(),
                "className".into(),
            ],
            props_schema: serde_json::json!({"$schema":"http://json-schema.org/draft-07/schema#","type":"object","properties":{"size":{"anyOf":[{"type":"string","enum":["small","medium","large"]},{"type":"null"}]},"contentType":{"anyOf":[{"type":"string"},{"type":"null"}]},"tileId":{"anyOf":[{"type":"string"},{"type":"null"}]},"className":{"anyOf":[{"type":"string"},{"type":"null"}]}},"required":["size","contentType","tileId","className"],"additionalProperties":false}),
            slots: vec!["default".into()],
            events: vec!["resize".into()],
            description: "One deterministic Mondrian tile with a visible drag handle.".into(),
        },
        ComponentDecl {
            name: "heading".into(),
            category: "static_canvas".into(),
            props: vec!["text".into(), "level".into()],
            props_schema: serde_json::json!({"$schema":"http://json-schema.org/draft-07/schema#","type":"object","properties":{"text":{"type":"string"},"level":{"anyOf":[{"anyOf":[{"type":"number","const":1},{"type":"number","const":2},{"type":"number","const":3}]},{"type":"null"}]}},"required":["text","level"],"additionalProperties":false}),
            slots: vec![],
            events: vec![],
            description: "Canvas heading.".into(),
        },
        ComponentDecl {
            name: "text".into(),
            category: "static_canvas".into(),
            props: vec!["text".into(), "className".into()],
            props_schema: serde_json::json!({"$schema":"http://json-schema.org/draft-07/schema#","type":"object","properties":{"text":{"type":"string"},"className":{"anyOf":[{"type":"string"},{"type":"null"}]}},"required":["text","className"],"additionalProperties":false}),
            slots: vec![],
            events: vec![],
            description: "Canvas body text.".into(),
        },
        ComponentDecl {
            name: "badge".into(),
            category: "static_canvas".into(),
            props: vec!["text".into(), "label".into(), "tone".into()],
            props_schema: serde_json::json!({"$schema":"http://json-schema.org/draft-07/schema#","type":"object","properties":{"text":{"anyOf":[{"type":"string"},{"type":"null"}]},"label":{"anyOf":[{"type":"string"},{"type":"null"}]},"tone":{"anyOf":[{"type":"string","enum":["default","ok","warn","danger","info"]},{"type":"null"}]}},"required":["text","label","tone"],"additionalProperties":false}),
            slots: vec![],
            events: vec![],
            description: "Compact status marker.".into(),
        },
        ComponentDecl {
            name: "row".into(),
            category: "static_canvas".into(),
            props: vec!["className".into()],
            props_schema: serde_json::json!({"$schema":"http://json-schema.org/draft-07/schema#","type":"object","properties":{"className":{"anyOf":[{"type":"string"},{"type":"null"}]}},"required":["className"],"additionalProperties":false}),
            slots: vec!["default".into()],
            events: vec![],
            description: "Horizontal wrapping group.".into(),
        },
        ComponentDecl {
            name: "stat".into(),
            category: "static_canvas".into(),
            props: vec![
                "label".into(),
                "value".into(),
                "unit".into(),
                "trend".into(),
            ],
            props_schema: serde_json::json!({"$schema":"http://json-schema.org/draft-07/schema#","type":"object","properties":{"label":{"type":"string"},"value":{"type":"string"},"unit":{"anyOf":[{"type":"string"},{"type":"null"}]},"trend":{"anyOf":[{"type":"string","enum":["up","down","flat"]},{"type":"null"}]}},"required":["label","value","unit","trend"],"additionalProperties":false}),
            slots: vec![],
            events: vec![],
            description: "Compact Canvas metric.".into(),
        },
    ]
}
