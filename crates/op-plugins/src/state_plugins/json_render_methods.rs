//! Durable method adapter for the manifest-pinned json-render reference catalog.

use std::collections::BTreeSet;
use std::sync::Arc;

use anyhow::{Context, Result, bail, ensure};
use op_state_store::{PluginSchema, SideEffect};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::runtime_contract;
use super::{
    ActionsOutput, ComponentDecl, ComponentLookupInput, ComponentOutput, ComponentsOutput,
    ConfigOutput, CoreExportsOutput, EmptyJsonRenderInput, HealthOutput, JsonRenderConfig,
    JsonRenderState, JsonSchemaMode, MethodSurface, PLUGIN_NAME, PackageLookupInput, PackageOutput,
    PackagesOutput, PromptMode, PromptSurfaceInput, PromptSurfaceOutput, RenderersOutput,
    SchemaExportInput, SchemaExportOutput, SetConfigOutput, SetJsonRenderConfigInput,
    SpecValidationInput, ToolsOutput, ValidationResultOutput,
};
use crate::state_plugins::runtime_documents::RuntimeDocuments;

const MAX_SPEC_BYTES: usize = 4 * 1024 * 1024;
const MAX_ELEMENTS: usize = 4096;
const MAX_GRAPH_EDGES: usize = 16_384;
const MAX_GRAPH_DEPTH: usize = 128;
const MAX_EXPANDED_RENDER_NODES: usize = 65_536;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredPolicy {
    config: JsonRenderConfig,
}

pub struct JsonRenderMethods {
    snapshot: Arc<JsonRenderState>,
    schema: Arc<PluginSchema>,
    documents: Arc<RuntimeDocuments>,
}

impl JsonRenderMethods {
    /// The bridge must pass the state defaults and method schema read from the
    /// same manifest-pinned plugin blob. Runtime code never rebuilds either.
    pub fn new(
        snapshot: JsonRenderState,
        schema: PluginSchema,
        documents: Arc<RuntimeDocuments>,
    ) -> Result<Self> {
        ensure!(
            schema.name == PLUGIN_NAME,
            "json_render schema owner mismatch"
        );
        ensure!(
            snapshot.source.commit == super::JSON_RENDER_SOURCE_COMMIT,
            "json_render snapshot source commit mismatch"
        );
        ensure!(
            schema.methods.len() == 15 + super::gallery::METHODS.len(),
            "json_render method snapshot is incomplete"
        );
        for (key, method) in &schema.methods {
            ensure!(key == &method.name, "json_render method key/name mismatch");
        }
        let mut component_names = BTreeSet::new();
        for component in &snapshot.components {
            ensure!(
                component_names.insert(component.name.as_str()),
                "duplicate json_render component"
            );
            jsonschema::validator_for(&component.props_schema)
                .with_context(|| format!("invalid {} props schema", component.name))?;
        }
        drop(component_names);
        let renderer_ids: BTreeSet<_> = snapshot
            .renderers
            .iter()
            .map(|item| item.id.as_str())
            .collect();
        let mut action_names = BTreeSet::new();
        for action in &snapshot.actions {
            ensure!(
                action_names.insert(action.name.as_str()),
                "duplicate json_render action"
            );
            jsonschema::validator_for(&action.params_schema)
                .with_context(|| format!("invalid {} parameter schema", action.name))?;
            ensure!(
                !action.renderers.is_empty(),
                "action has no backing renderer"
            );
            for renderer in &action.renderers {
                let renderer = serde_json::to_value(renderer)?;
                ensure!(
                    renderer_ids.contains(renderer.as_str().context("invalid renderer")?),
                    "action references an absent renderer"
                );
            }
        }
        drop((renderer_ids, action_names));
        Ok(Self {
            snapshot: Arc::new(snapshot),
            schema: Arc::new(schema),
            documents,
        })
    }

    pub async fn dispatch(&self, method: &str, args: &Value) -> Result<Value> {
        match method {
            "get_config" => {
                parse::<EmptyJsonRenderInput>(args, method)?;
                let (config, revision) = self.config().await?;
                encode(ConfigOutput { config, revision })
            }
            "set_config" => {
                let input = parse::<SetJsonRenderConfigInput>(args, method)?;
                self.set_config(input.config).await
            }
            "get_health" => {
                parse::<EmptyJsonRenderInput>(args, method)?;
                encode(HealthOutput {
                    status: self.snapshot.status.clone(),
                    package_count: self.snapshot.packages.len(),
                    component_count: self.snapshot.components.len(),
                    action_count: self.snapshot.actions.len(),
                    source_commit: self.snapshot.source.commit.clone(),
                    catalog_kind: "manifest_pinned_upstream_reference".into(),
                })
            }
            "list_tools" => {
                parse::<EmptyJsonRenderInput>(args, method)?;
                let (config, _) = self.config().await?;
                encode(ToolsOutput {
                    tools: self.tools(&config),
                })
            }
            "list_packages" => {
                parse::<EmptyJsonRenderInput>(args, method)?;
                self.require_catalog().await?;
                encode(PackagesOutput {
                    packages: self.snapshot.packages.clone(),
                })
            }
            "get_package" => {
                let input = parse::<PackageLookupInput>(args, method)?;
                validate_lookup("package_name", &input.package_name)?;
                self.require_catalog().await?;
                encode(PackageOutput {
                    package: self
                        .snapshot
                        .packages
                        .iter()
                        .find(|item| item.package_name == input.package_name)
                        .cloned(),
                })
            }
            "list_components" => {
                parse::<EmptyJsonRenderInput>(args, method)?;
                self.require_catalog().await?;
                encode(ComponentsOutput {
                    components: self.snapshot.components.clone(),
                })
            }
            "get_component_schema" => {
                let input = parse::<ComponentLookupInput>(args, method)?;
                validate_lookup("component name", &input.name)?;
                self.require_catalog().await?;
                encode(ComponentOutput {
                    component: self.component(&input.name).cloned(),
                })
            }
            "list_actions" => {
                parse::<EmptyJsonRenderInput>(args, method)?;
                self.require_catalog().await?;
                let (config, _) = self.config().await?;
                encode(ActionsOutput {
                    actions: runtime_contract::renderer_actions(
                        &self.snapshot.actions,
                        config.default_renderer,
                    )
                    .into_iter()
                    .cloned()
                    .collect(),
                })
            }
            "list_core_exports" => {
                parse::<EmptyJsonRenderInput>(args, method)?;
                self.require_catalog().await?;
                encode(CoreExportsOutput {
                    exports: self.snapshot.core_exports.clone(),
                })
            }
            "list_renderers" => {
                parse::<EmptyJsonRenderInput>(args, method)?;
                self.require_catalog().await?;
                encode(RenderersOutput {
                    renderers: self.snapshot.renderers.clone(),
                })
            }
            "get_spec_schema" => {
                parse::<EmptyJsonRenderInput>(args, method)?;
                let (config, _) = self.config().await?;
                ensure!(config.expose_catalog, "catalog exposure is disabled");
                encode(SchemaExportOutput {
                    mode: config.schema_mode,
                    schema: self.export_schema(config.schema_mode, config.default_renderer),
                })
            }
            "validate_spec" => {
                let input = parse::<SpecValidationInput>(args, method)?;
                let (config, _) = self.config().await?;
                ensure!(config.expose_catalog, "catalog exposure is disabled");
                self.validate_spec(&input.spec, config.schema_mode, config.default_renderer)
            }
            "export_json_schema" => {
                let input = parse::<SchemaExportInput>(args, method)?;
                let (config, _) = self.config().await?;
                ensure!(config.expose_catalog, "catalog exposure is disabled");
                let mode = input.mode.unwrap_or(config.schema_mode);
                encode(SchemaExportOutput {
                    mode,
                    schema: self.export_schema(mode, config.default_renderer),
                })
            }
            "build_prompt_surface" => {
                let input = parse::<PromptSurfaceInput>(args, method)?;
                self.build_prompt(input).await
            }
            _ => bail!("unsupported json_render method"),
        }
    }

    /// Materialize the immutable manifest snapshot with the currently committed
    /// policy. The bridge publishes this value as observed plugin state after a
    /// successful method invocation.
    pub async fn projection_state(&self) -> Result<JsonRenderState> {
        let (config, _) = self.config().await?;
        let mut state = self.snapshot.as_ref().clone();
        state.config = config;
        Ok(state)
    }

    async fn config(&self) -> Result<(JsonRenderConfig, u64)> {
        match self.documents.read(PLUGIN_NAME).await? {
            None => Ok((self.snapshot.config.clone(), 0)),
            Some(row) => {
                let policy: StoredPolicy = serde_json::from_value(row.document)
                    .context("invalid stored json_render policy")?;
                self.validate_config(&policy.config)?;
                Ok((policy.config, row.revision))
            }
        }
    }

    async fn set_config(&self, config: JsonRenderConfig) -> Result<Value> {
        self.validate_config(&config)?;
        let fallback = self.snapshot.config.clone();
        let stored = config.clone();
        let (row, changed) = self
            .documents
            .update_with_result(PLUGIN_NAME, move |old| {
                let previous = old
                    .map(serde_json::from_value::<StoredPolicy>)
                    .transpose()
                    .context("invalid stored json_render policy")?
                    .map_or(fallback, |policy| policy.config);
                let changed = previous != stored;
                Ok((
                    serde_json::to_value(StoredPolicy { config: stored })?,
                    changed,
                ))
            })
            .await?;
        let observed: StoredPolicy = serde_json::from_value(row.document)
            .context("committed json_render policy is unreadable")?;
        ensure!(
            observed.config == config,
            "json_render policy readback mismatch"
        );
        encode(SetConfigOutput {
            config,
            changed,
            revision: row.revision,
        })
    }

    fn validate_config(&self, config: &JsonRenderConfig) -> Result<()> {
        let selected = serde_json::to_value(config.default_renderer)?;
        let selected = selected.as_str().context("invalid renderer target")?;
        ensure!(
            self.snapshot
                .renderers
                .iter()
                .any(|item| item.id == selected),
            "renderer is absent from the pinned catalog"
        );
        Ok(())
    }

    async fn require_catalog(&self) -> Result<()> {
        let (config, _) = self.config().await?;
        ensure!(config.expose_catalog, "catalog exposure is disabled");
        Ok(())
    }

    fn component(&self, name: &str) -> Option<&ComponentDecl> {
        self.snapshot
            .components
            .iter()
            .find(|item| item.name == name)
    }

    fn tools(&self, config: &JsonRenderConfig) -> Vec<MethodSurface> {
        let mut tools: Vec<_> = self
            .schema
            .methods
            .values()
            .map(|method| MethodSurface {
                name: method.name.clone(),
                side_effect: match method.side_effect {
                    SideEffect::Read => "read",
                    SideEffect::Mutation => "mutation",
                }
                .into(),
                required_capability: method.required_capability.clone().unwrap_or_default(),
                subid: method.subid.clone(),
                available: method_available(&method.name, config),
            })
            .collect();
        tools.sort_by(|left, right| left.name.cmp(&right.name));
        tools
    }

    fn validate_spec(
        &self,
        spec: &Value,
        mode: JsonSchemaMode,
        renderer: super::RendererTarget,
    ) -> Result<Value> {
        super::validation_bounds::check_json(
            spec,
            super::validation_bounds::JsonLimits {
                max_depth: 128,
                max_nodes: 262_144,
                max_bytes: MAX_SPEC_BYTES,
            },
        )?;
        let mut issues = Vec::new();
        let runtime_schema = self.export_schema(mode, renderer);
        if let Err(error) = jsonschema::validate(&runtime_schema, spec) {
            issues.push(format!("runtime schema: {error}"));
        }
        let Some(object) = spec.as_object() else {
            return encode(ValidationResultOutput {
                valid: false,
                issues: vec!["spec must be an object".into()],
                checked_elements: 0,
            });
        };
        if matches!(mode, JsonSchemaMode::Strict) {
            let allowed: BTreeSet<_> = ["root", "elements", "state"].into_iter().collect();
            for field in object.keys().filter(|key| !allowed.contains(key.as_str())) {
                issues.push(format!("unsupported top-level field '{field}'"));
            }
        }
        let root = object.get("root").and_then(Value::as_str);
        if root.is_none_or(str::is_empty) {
            issues.push("root must be a non-empty string".into());
        }
        let Some(elements) = object.get("elements").and_then(Value::as_object) else {
            issues.push("elements must be an object".into());
            return encode(ValidationResultOutput {
                valid: false,
                issues,
                checked_elements: 0,
            });
        };
        ensure!(elements.len() <= MAX_ELEMENTS, "spec exceeds 4096 elements");
        if let Some(root) = root {
            if !elements.contains_key(root) {
                issues.push(format!("root element '{root}' does not exist"));
            }
        }
        for (key, element) in elements {
            self.validate_element(key, element, elements, &mut issues);
        }
        self.validate_graph(root, elements, &mut issues);
        issues.sort();
        issues.dedup();
        encode(ValidationResultOutput {
            valid: issues.is_empty(),
            issues,
            checked_elements: elements.len(),
        })
    }

    fn validate_element(
        &self,
        key: &str,
        element: &Value,
        elements: &serde_json::Map<String, Value>,
        issues: &mut Vec<String>,
    ) {
        let Some(object) = element.as_object() else {
            issues.push(format!("element '{key}' must be an object"));
            return;
        };
        let Some(kind) = object.get("type").and_then(Value::as_str) else {
            issues.push(format!("element '{key}' type must be a string"));
            return;
        };
        let Some(component) = self.component(kind) else {
            issues.push(format!("element '{key}' uses unknown component '{kind}'"));
            return;
        };
        match object.get("children") {
            Some(Value::Array(children)) => {
                if component.slots.is_empty() && !children.is_empty() {
                    issues.push(format!(
                        "element '{key}' component '{kind}' has no child slot"
                    ));
                }
                for child in children {
                    match child.as_str() {
                        Some(child) if elements.contains_key(child) => {}
                        Some(child) => issues.push(format!(
                            "element '{key}' references missing child '{child}'"
                        )),
                        None => issues.push(format!(
                            "element '{key}' children must contain only strings"
                        )),
                    }
                }
            }
            Some(_) => issues.push(format!("element '{key}' children must be an array")),
            None => issues.push(format!("element '{key}' is missing children")),
        }
    }

    /// Validate the exact flat `children` graph accepted by the pinned runtime
    /// schema. Named `slots` are not part of that sealed element contract and
    /// therefore are neither accepted nor interpreted as hidden edges here.
    fn validate_graph(
        &self,
        root: Option<&str>,
        elements: &serde_json::Map<String, Value>,
        issues: &mut Vec<String>,
    ) {
        let Some(root) = root else {
            return;
        };
        if let Err(error) = super::validation_bounds::check_children_graph(
            root,
            elements,
            super::validation_bounds::GraphLimits {
                max_elements: MAX_ELEMENTS,
                max_edges: MAX_GRAPH_EDGES,
                max_depth: MAX_GRAPH_DEPTH,
                max_expanded_nodes: MAX_EXPANDED_RENDER_NODES,
            },
        ) {
            issues.push(error.to_string());
        }
    }

    fn export_schema(&self, mode: JsonSchemaMode, renderer: super::RendererTarget) -> Value {
        runtime_contract::spec_schema(
            &self.snapshot.components,
            &self.snapshot.actions,
            renderer,
            mode,
        )
    }

    async fn build_prompt(&self, input: PromptSurfaceInput) -> Result<Value> {
        let (config, _) = self.config().await?;
        ensure!(config.expose_catalog, "catalog exposure is disabled");
        ensure!(
            config.expose_prompt_surface,
            "prompt surface exposure is disabled"
        );
        ensure!(
            input.custom_rules.len() <= 32,
            "at most 32 custom rules are allowed"
        );
        for rule in &input.custom_rules {
            ensure!(!rule.trim().is_empty(), "custom rules cannot be empty");
            ensure!(rule.len() <= 2048, "custom rule exceeds 2048 bytes");
        }
        let mode = input.mode.unwrap_or(PromptMode::Standalone);
        let mut lines = vec![
            "You are a UI generator for the manifest-pinned json-render reference catalog.".to_string(),
            format!("Renderer target: {}.", serde_json::to_value(config.default_renderer)?.as_str().unwrap_or("unknown")),
            match mode {
                PromptMode::Standalone => "Return RFC 6902 JSON Patch as JSONL without prose.".into(),
                PromptMode::Inline => "Respond briefly, then place RFC 6902 JSON Patch JSONL in a spec code fence.".into(),
            },
            "Every element requires type, props, and children. Every root and child key must resolve.".into(),
            "Use only the following component declarations and exact prop schemas:".into(),
        ];
        for component in &self.snapshot.components {
            lines.push(format!(
                "- {}: props={} slots={} events={}",
                component.name,
                serde_json::to_string(&component.props_schema)?,
                serde_json::to_string(&component.slots)?,
                serde_json::to_string(&component.events)?
            ));
        }
        lines.push("Available actions:".into());
        for action in
            runtime_contract::renderer_actions(&self.snapshot.actions, config.default_renderer)
        {
            lines.push(format!(
                "- {}({}): {}",
                action.name,
                action.params.join(", "),
                action.description
            ));
        }
        lines.push(format!(
            "Runtime JSON Schema: {}",
            serde_json::to_string(
                &self.export_schema(config.schema_mode, config.default_renderer)
            )?
        ));
        lines.extend(input.custom_rules);
        encode(PromptSurfaceOutput {
            mode,
            renderer: config.default_renderer,
            prompt: lines.join("\n"),
        })
    }
}

fn method_available(method: &str, config: &JsonRenderConfig) -> bool {
    match method {
        "get_config" | "set_config" | "get_health" | "list_tools" => true,
        "build_prompt_surface" => config.expose_catalog && config.expose_prompt_surface,
        _ => config.expose_catalog,
    }
}

fn parse<T: serde::de::DeserializeOwned>(args: &Value, method: &str) -> Result<T> {
    serde_json::from_value(args.clone()).with_context(|| format!("invalid {method} arguments"))
}

fn encode<T: Serialize>(output: T) -> Result<Value> {
    serde_json::to_value(output).map_err(Into::into)
}

fn validate_lookup(field: &str, value: &str) -> Result<()> {
    ensure!(!value.trim().is_empty(), "{field} is required");
    ensure!(value.len() <= 256, "{field} is too long");
    ensure!(
        !value.chars().any(char::is_control),
        "{field} contains control characters"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state_plugins::json_render::json_render_schema;
    use op_cozo_store::CozoGraphShuttle;

    fn service(store: CozoGraphShuttle) -> JsonRenderMethods {
        JsonRenderMethods::new(
            JsonRenderState::default(),
            json_render_schema(),
            Arc::new(RuntimeDocuments::new(store)),
        )
        .unwrap()
    }

    fn card(children: Vec<String>) -> Value {
        json!({
            "type":"Card",
            "props":{
                "title":null,"description":null,"maxWidth":null,
                "centered":null,"className":null
            },
            "children":children
        })
    }

    fn button() -> Value {
        json!({
            "type":"Button",
            "props":{"label":"leaf","variant":null,"disabled":null},
            "children":[]
        })
    }

    async fn validate_elements(
        service: &JsonRenderMethods,
        root: &str,
        elements: serde_json::Map<String, Value>,
    ) -> Value {
        service
            .dispatch(
                "validate_spec",
                &json!({"spec":{"root":root,"elements":elements}}),
            )
            .await
            .unwrap()
    }

    fn has_issue(output: &Value, needle: &str) -> bool {
        output["issues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue.as_str().is_some_and(|issue| issue.contains(needle)))
    }

    #[tokio::test]
    async fn all_methods_dispatch_and_tools_derive_from_schema() {
        let service = service(CozoGraphShuttle::new_in_memory().unwrap());
        let cases = [
            ("get_config", json!({})),
            (
                "set_config",
                json!({"config":{"expose_catalog":true,"expose_prompt_surface":true,"schema_mode":"strict","default_renderer":"react"}}),
            ),
            ("get_health", json!({})),
            ("list_tools", json!({})),
            ("list_packages", json!({})),
            ("get_package", json!({"package_name":"@json-render/core"})),
            ("list_components", json!({})),
            ("get_component_schema", json!({"name":"Button"})),
            ("list_actions", json!({})),
            ("list_core_exports", json!({})),
            ("list_renderers", json!({})),
            ("get_spec_schema", json!({})),
            (
                "validate_spec",
                json!({"spec":{"root":"x","elements":{"x":{"type":"Button","props":{"label":"Go","variant":null,"disabled":null},"children":[]}}}}),
            ),
            ("export_json_schema", json!({})),
            ("build_prompt_surface", json!({})),
        ];
        assert_eq!(cases.len(), 15);
        let mut outputs = std::collections::BTreeMap::new();
        for (method, args) in cases {
            outputs.insert(method, service.dispatch(method, &args).await.unwrap());
        }
        // A successful return alone does not prove these read methods. Check
        // independently useful catalog facts and consistency between public
        // lookup, listing, health, prompt, validation and schema-export methods.
        let packages = outputs["list_packages"]["packages"].as_array().unwrap();
        let core = packages
            .iter()
            .find(|p| p["package_name"] == "@json-render/core")
            .unwrap();
        assert_eq!(&outputs["get_package"]["package"], core);
        assert!(!core["version"].as_str().unwrap().is_empty());
        let components = outputs["list_components"]["components"].as_array().unwrap();
        let button = components.iter().find(|c| c["name"] == "Button").unwrap();
        assert_eq!(&outputs["get_component_schema"]["component"], button);
        let valid_props = json!({"label":"Go","variant":null,"disabled":null});
        jsonschema::validate(&button["props_schema"], &valid_props).unwrap();
        assert!(jsonschema::validate(&button["props_schema"], &json!({"label":42})).is_err());
        let actions = outputs["list_actions"]["actions"].as_array().unwrap();
        assert!(!actions.is_empty());
        assert!(
            outputs["list_core_exports"]["exports"]
                .as_array()
                .unwrap()
                .iter()
                .any(|item| item["name"] == "defineCatalog"
                    && item["package"] == "@json-render/core")
        );
        assert!(
            outputs["list_renderers"]["renderers"]
                .as_array()
                .unwrap()
                .iter()
                .any(|renderer| renderer["id"] == "react"
                    && renderer["package_name"] == "@json-render/react")
        );
        let health = &outputs["get_health"];
        assert_eq!(health["status"], "catalog_snapshot");
        assert_eq!(health["package_count"], packages.len());
        assert_eq!(health["component_count"], components.len());
        assert_eq!(health["catalog_kind"], "manifest_pinned_upstream_reference");
        assert_eq!(outputs["validate_spec"]["valid"], true);
        assert_eq!(outputs["validate_spec"]["checked_elements"], 1);
        assert_eq!(outputs["validate_spec"]["issues"], json!([]));
        let valid_spec = json!({"root":"x","elements":{"x":{
            "type":"Button","props":valid_props,"children":[]
        }}});
        for method in ["get_spec_schema", "export_json_schema"] {
            jsonschema::validate(&outputs[method]["schema"], &valid_spec).unwrap();
            let mut invalid = valid_spec.clone();
            invalid["elements"]["x"]["props"]["label"] = json!(42);
            assert!(
                jsonschema::validate(&outputs[method]["schema"], &invalid).is_err(),
                "{method}"
            );
        }
        let prompt = outputs["build_prompt_surface"]["prompt"].as_str().unwrap();
        assert_eq!(outputs["build_prompt_surface"]["renderer"], "react");
        assert!(prompt.contains("Renderer target: react."));
        assert!(prompt.contains("Button: props="));
        assert!(prompt.contains("Runtime JSON Schema:"));
        for action in actions {
            assert!(prompt.contains(&format!("- {}(", action["name"].as_str().unwrap())));
        }
        let readback = service.dispatch("get_config", &json!({})).await.unwrap();
        assert_eq!(readback["config"], outputs["set_config"]["config"]);
        assert_eq!(readback["revision"], outputs["set_config"]["revision"]);
        let tools = service.dispatch("list_tools", &json!({})).await.unwrap();
        let names: BTreeSet<_> = tools["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|tool| tool["name"].as_str().unwrap())
            .collect();
        let declared: BTreeSet<_> = service.schema.methods.keys().map(String::as_str).collect();
        assert_eq!(names, declared);
    }

    #[tokio::test]
    async fn exact_component_schema_and_graph_integrity_are_enforced() {
        let service = service(CozoGraphShuttle::new_in_memory().unwrap());
        let output = service.dispatch("validate_spec", &json!({"spec":{
            "root":"root","elements":{
                "root":{"type":"Button","props":{"label":7,"variant":null,"disabled":null,"invented":true},"children":["missing"]}
            }
        }})).await.unwrap();
        assert_eq!(output["valid"], false);
        let issues = output["issues"].as_array().unwrap();
        assert!(
            issues
                .iter()
                .any(|issue| issue.as_str().unwrap().contains("props"))
        );
        assert!(
            issues
                .iter()
                .any(|issue| issue.as_str().unwrap().contains("missing child"))
        );
    }

    #[tokio::test]
    async fn graph_rejects_self_cycles_and_disconnected_multi_node_cycles() {
        let service = service(CozoGraphShuttle::new_in_memory().unwrap());
        let self_cycle = serde_json::Map::from_iter([("root".into(), card(vec!["root".into()]))]);
        let output = validate_elements(&service, "root", self_cycle).await;
        assert_eq!(output["valid"], false);
        assert!(has_issue(&output, "contains a cycle"));

        let disconnected = serde_json::Map::from_iter([
            ("root".into(), button()),
            ("a".into(), card(vec!["b".into()])),
            ("b".into(), card(vec!["a".into()])),
        ]);
        let output = validate_elements(&service, "root", disconnected).await;
        assert_eq!(output["valid"], false);
        assert!(has_issue(&output, "disconnected cycle"));
    }

    #[tokio::test]
    async fn graph_rejects_excessive_depth_and_edge_count_iteratively() {
        let service = service(CozoGraphShuttle::new_in_memory().unwrap());
        let mut deep = serde_json::Map::new();
        for index in 0..=MAX_GRAPH_DEPTH {
            let key = format!("n{index}");
            let element = if index == MAX_GRAPH_DEPTH {
                button()
            } else {
                card(vec![format!("n{}", index + 1)])
            };
            deep.insert(key, element);
        }
        let output = validate_elements(&service, "n0", deep).await;
        assert_eq!(output["valid"], false);
        assert!(has_issue(&output, "maximum depth"));

        let too_many_edges = serde_json::Map::from_iter([
            (
                "root".into(),
                card(vec!["leaf".into(); MAX_GRAPH_EDGES + 1]),
            ),
            ("leaf".into(), button()),
        ]);
        let output = validate_elements(&service, "root", too_many_edges).await;
        assert_eq!(output["valid"], false);
        assert!(has_issue(&output, "child edges"));
    }

    #[tokio::test]
    async fn graph_bounds_expanded_dag_cost_but_allows_bounded_shared_children() {
        let service = service(CozoGraphShuttle::new_in_memory().unwrap());
        let shared = serde_json::Map::from_iter([
            ("root".into(), card(vec!["left".into(), "right".into()])),
            ("left".into(), card(vec!["leaf".into()])),
            ("right".into(), card(vec!["leaf".into()])),
            ("leaf".into(), button()),
        ]);
        let output = validate_elements(&service, "root", shared).await;
        assert_eq!(output["valid"], true);

        // Each pair points to both nodes in the next layer. The serialized DAG
        // stays tiny while the number of render occurrences doubles per layer.
        let mut exponential = serde_json::Map::new();
        exponential.insert("root".into(), card(vec!["a0".into(), "b0".into()]));
        for layer in 0..16 {
            let children = vec![format!("a{}", layer + 1), format!("b{}", layer + 1)];
            exponential.insert(format!("a{layer}"), card(children.clone()));
            exponential.insert(format!("b{layer}"), card(children));
        }
        exponential.insert("a16".into(), button());
        exponential.insert("b16".into(), button());
        let output = validate_elements(&service, "root", exponential).await;
        assert_eq!(output["valid"], false);
        assert!(has_issue(&output, "expanded render graph"));
    }

    #[tokio::test]
    async fn runtime_fields_share_exported_validation_and_renderer_actions() {
        let service = service(CozoGraphShuttle::new_in_memory().unwrap());
        let valid = json!({"spec":{
            "root":"root","state":{"saving":false},"elements":{
                "root":{
                    "type":"Button",
                    "props":{"label":{"$template":"Save ${/name}"},"variant":null,"disabled":{"$state":"/saving"}},
                    "children":[],
                    "visible":{"$state":"/allowed","eq":true},
                    "on":{"press":{"action":"setState","params":{"statePath":"/saving","value":true}}},
                    "watch":{"/saving":{"action":"validateForm"}}
                }
            }
        }});
        assert_eq!(
            service.dispatch("validate_spec", &valid).await.unwrap()["valid"],
            true
        );

        for invalid in [
            json!({"spec":{"root":"root","elements":{"root":{"type":"Button","props":{"label":"Save","variant":null,"disabled":null},"children":[],"on":{"change":{"action":"setState","params":{"statePath":"/x","value":1}}}}}}}),
            json!({"spec":{"root":"root","elements":{"root":{"type":"Button","props":{"label":"Save","variant":null,"disabled":null},"children":[],"on":{"press":{"action":"push","params":{"screen":"detail"}}}}}}}),
            json!({"spec":{"root":"root","elements":{"root":{"type":"Button","props":{"label":"Save","variant":null,"disabled":null},"children":[],"visible":{"$state":"/x","$item":"x"}}}}}),
        ] {
            assert_eq!(
                service.dispatch("validate_spec", &invalid).await.unwrap()["valid"],
                false
            );
        }

        service
            .dispatch(
                "set_config",
                &json!({"config":{
                    "expose_catalog":true,"expose_prompt_surface":true,
                    "schema_mode":"strict","default_renderer":"react-native"
                }}),
            )
            .await
            .unwrap();
        let native = json!({"spec":{"root":"root","elements":{"root":{
            "type":"Button","props":{"label":"Open","variant":null,"disabled":null},
            "children":[],"on":{"press":{"action":"push","params":{"screen":"detail"}}}
        }}}});
        assert_eq!(
            service.dispatch("validate_spec", &native).await.unwrap()["valid"],
            true
        );
    }

    #[tokio::test]
    async fn policy_persists_reopens_and_gates_catalog_and_prompt() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("runtime");
        {
            let service = service(CozoGraphShuttle::new_persistent(path.clone()).unwrap());
            let result = service
                .dispatch(
                    "set_config",
                    &json!({"config":{
                        "expose_catalog":false,"expose_prompt_surface":false,
                        "schema_mode":"standard","default_renderer":"react"
                    }}),
                )
                .await
                .unwrap();
            assert_eq!(result["changed"], true);
            assert!(
                service
                    .dispatch("list_components", &json!({}))
                    .await
                    .is_err()
            );
            let tools = service.dispatch("list_tools", &json!({})).await.unwrap();
            assert_eq!(
                tools["tools"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|tool| tool["name"] == "build_prompt_surface")
                    .unwrap()["available"],
                false
            );
        }
        // The dropped handle releases RocksDB's LOCK asynchronously; under a
        // loaded parallel test run the reopen can race it. Production never
        // reopens a path in-process, so the test waits rather than the code.
        let reopened = {
            let mut attempt = 0;
            loop {
                match CozoGraphShuttle::new_persistent(path.clone()) {
                    Ok(store) => break store,
                    Err(error) if attempt < 50 && error.to_string().contains("LOCK") => {
                        attempt += 1;
                        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    }
                    Err(error) => panic!("reopen runtime store: {error}"),
                }
            }
        };
        let service = service(reopened);
        let config = service.dispatch("get_config", &json!({})).await.unwrap();
        assert_eq!(config["config"]["expose_catalog"], false);
        assert!(
            service
                .dispatch("build_prompt_surface", &json!({}))
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn malformed_inputs_and_unsupported_renderer_fail_before_write() {
        let service = service(CozoGraphShuttle::new_in_memory().unwrap());
        assert!(
            service
                .dispatch("get_config", &json!({"extra":true}))
                .await
                .is_err()
        );
        assert!(
            service
                .dispatch("get_component_schema", &json!({"name":"\n"}))
                .await
                .is_err()
        );
        assert!(
            service
                .dispatch("build_prompt_surface", &json!({"mode":"invented"}))
                .await
                .is_err()
        );
        assert!(
            service
                .dispatch(
                    "set_config",
                    &json!({"config":{
                        "expose_catalog":true,"expose_prompt_surface":true,
                        "schema_mode":"strict","default_renderer":"react-pdf","extra":true
                    }})
                )
                .await
                .is_err()
        );
        assert_eq!(
            service.dispatch("get_config", &json!({})).await.unwrap()["revision"],
            0
        );
    }

    #[test]
    fn malformed_injected_snapshot_is_rejected() {
        let documents = Arc::new(RuntimeDocuments::new(
            CozoGraphShuttle::new_in_memory().unwrap(),
        ));
        let mut snapshot = JsonRenderState::default();
        snapshot.components[0].props_schema = json!({"type":"invented"});
        assert!(JsonRenderMethods::new(snapshot, json_render_schema(), documents).is_err());
    }

    #[test]
    fn sealed_projection_seed_reconstructs_the_manifest_snapshot() {
        let schema = json_render_schema();
        let seed = crate::projection_seed_state_from_schema(&schema);
        let snapshot: JsonRenderState = simd_json::serde::from_owned_value(seed).unwrap();
        assert_eq!(
            snapshot.source.commit,
            super::super::JSON_RENDER_SOURCE_COMMIT
        );
        assert_eq!(snapshot.components.len(), 36);
        let button = snapshot
            .components
            .iter()
            .find(|item| item.name == "Button")
            .unwrap();
        assert_eq!(button.props_schema["additionalProperties"], false);
        assert_eq!(
            button.props_schema["required"],
            json!(["label", "variant", "disabled"])
        );
        JsonRenderMethods::new(
            snapshot,
            schema,
            Arc::new(RuntimeDocuments::new(
                CozoGraphShuttle::new_in_memory().unwrap(),
            )),
        )
        .unwrap();
    }

    #[tokio::test]
    async fn projection_state_merges_the_committed_policy() {
        let service = service(CozoGraphShuttle::new_in_memory().unwrap());
        service
            .dispatch(
                "set_config",
                &json!({"config":{
                    "expose_catalog":false,"expose_prompt_surface":false,
                    "schema_mode":"standard","default_renderer":"react"
                }}),
            )
            .await
            .unwrap();
        let projected = service.projection_state().await.unwrap();
        assert!(!projected.config.expose_catalog);
        assert_eq!(projected.components.len(), 36);
        assert_eq!(
            projected.source.commit,
            super::super::JSON_RENDER_SOURCE_COMMIT
        );
    }
}
