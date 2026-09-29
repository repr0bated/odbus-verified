//! Pure renderer over one exact pinned target schema and Canvas validator.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{ensure, Context, Result};
use op_state_store::PluginSchema;
use serde_json::{json, Value};

use super::{
    json_render::canvas_profile::PinnedCanvasValidator,
    schema_renderer::{
        EmptyInput, GalleryDescription, GalleryLookupInput, GetRenderingOutput,
        ListGalleriesOutput, RenderSchemaOutput, ValidateSchemaOutput,
    },
};

const MAX_SUMMARY_ITEMS: usize = 512;
const MAX_ID_BYTES: usize = 128;

pub const WORKSPACE_GALLERY_ID: &str = "workspace";

pub struct GalleryTarget {
    pub method: &'static str,
    pub capability: String,
    pub args: Value,
}

/// This facade owns no gallery store or owner selector. Its sole gallery is
/// the calling principal/session's existing admitted private Gemma workspace.
pub fn plan_gallery_method(method: &str, args: &Value) -> Result<GalleryTarget> {
    let (target, args) = match method {
        "list_galleries" => {
            let _: EmptyInput = serde_json::from_value(args.clone())?;
            ("get_ui_workspace", json!({}))
        }
        "get_rendering" => {
            let input: GalleryLookupInput = serde_json::from_value(args.clone())?;
            ensure!(
                input.gallery_id == WORKSPACE_GALLERY_ID,
                "unknown caller-owned gallery"
            );
            ensure!(
                !input.rendering_id.is_empty()
                    && input.rendering_id.len() <= MAX_ID_BYTES
                    && !input.rendering_id.chars().any(char::is_control),
                "invalid rendering id"
            );
            ("get_ui_spec", json!({"spec_id":input.rendering_id}))
        }
        _ => anyhow::bail!("unsupported gallery method"),
    };
    let owner = super::gemma_brain::gemma_brain_schema();
    let declaration = owner
        .methods
        .get(target)
        .context("workspace method is absent")?;
    Ok(GalleryTarget {
        method: target,
        capability: owner
            .resolve_method_capability(declaration)
            .capability()
            .to_owned(),
        args,
    })
}

pub fn complete_gallery_method(method: &str, output: Value) -> Result<Value> {
    use super::gemma_brain::workspace::{UiSpecOutput, UiWorkspaceOutput};
    match method {
        "list_galleries" => {
            let output: UiWorkspaceOutput = serde_json::from_value(output)?;
            let galleries = output
                .workspace
                .into_iter()
                .map(|workspace| GalleryDescription {
                    gallery_id: WORKSPACE_GALLERY_ID.to_owned(),
                    document_revision: workspace.document_revision,
                    renderer_schema_hash: workspace.projection.catalog_hash,
                    rendering_count: workspace.projection.specs.len() as u64,
                })
                .collect();
            Ok(serde_json::to_value(ListGalleriesOutput { galleries })?)
        }
        "get_rendering" => {
            let output: UiSpecOutput = serde_json::from_value(output)?;
            Ok(serde_json::to_value(GetRenderingOutput {
                gallery_id: WORKSPACE_GALLERY_ID.to_owned(),
                rendering: output.spec,
                render_admitted: output.render_admitted,
            })?)
        }
        _ => anyhow::bail!("unsupported gallery method"),
    }
}

pub struct SchemaRendererMethods {
    validator: PinnedCanvasValidator,
}

impl SchemaRendererMethods {
    pub fn new(validator: PinnedCanvasValidator) -> Result<Self> {
        let components: BTreeSet<_> = validator
            .profile()
            .components
            .iter()
            .map(|component| component.name.as_str())
            .collect();
        for required in ["container", "heading", "text"] {
            ensure!(
                components.contains(required),
                "pinned Canvas profile lacks summary component {required}"
            );
        }
        Ok(Self { validator })
    }

    pub fn dispatch(
        &self,
        method: &str,
        target: &PluginSchema,
        target_hash: &str,
    ) -> Result<Value> {
        validate_target_identity(target, target_hash)?;
        match method {
            "render_schema" => {
                serde_json::to_value(self.render(target, target_hash)?).map_err(Into::into)
            }
            "validate_schema" => {
                serde_json::to_value(self.validate(target, target_hash)).map_err(Into::into)
            }
            "list_galleries" | "get_rendering" => {
                anyhow::bail!("schema_renderer.{method} requires authenticated workspace dispatch")
            }
            other => anyhow::bail!("unsupported schema_renderer method: {other}"),
        }
    }

    fn render(&self, target: &PluginSchema, target_hash: &str) -> Result<RenderSchemaOutput> {
        let errors = contract_errors(target);
        ensure!(
            errors.is_empty(),
            "target PluginSchema is invalid: {}",
            errors.join("; ")
        );
        let rendering = build_summary(target)?;
        self.validator
            .validate(&rendering)
            .context("generated schema summary violates pinned Canvas profile")?;
        Ok(RenderSchemaOutput {
            plugin_id: target.name.clone(),
            schema_version: target.version.clone(),
            schema_hash: target_hash.to_owned(),
            renderer_schema_hash: self.validator.schema_hash().to_owned(),
            canvas_profile_id: self.validator.profile().id.clone(),
            rendering,
            field_count: target.fields.len() as u64,
            method_count: target.methods.len() as u64,
        })
    }

    fn validate(&self, target: &PluginSchema, target_hash: &str) -> ValidateSchemaOutput {
        let mut errors = contract_errors(target);
        match build_summary(target).and_then(|spec| self.validator.validate(&spec)) {
            Ok(()) => {}
            Err(error) => errors.push(error.to_string()),
        }
        errors.sort();
        errors.dedup();
        ValidateSchemaOutput {
            plugin_id: target.name.clone(),
            schema_version: target.version.clone(),
            schema_hash: target_hash.to_owned(),
            renderer_schema_hash: self.validator.schema_hash().to_owned(),
            valid: errors.is_empty(),
            errors,
            checked_fields: target.fields.len() as u64,
            checked_methods: target.methods.len() as u64,
        }
    }
}

fn validate_target_identity(target: &PluginSchema, target_hash: &str) -> Result<()> {
    ensure!(
        !target.name.is_empty() && target.name.len() <= MAX_ID_BYTES,
        "invalid target plugin id"
    );
    ensure!(
        target
            .name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.')),
        "invalid target plugin id"
    );
    ensure!(
        target_hash.len() == 64 && target_hash.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "invalid stored target schema hash"
    );
    Ok(())
}

fn contract_errors(schema: &PluginSchema) -> Vec<String> {
    let mut errors = Vec::new();
    if schema.version.is_empty() {
        errors.push("plugin version is empty".into());
    }
    for (key, method) in &schema.methods {
        if invalid_name(key) {
            errors.push(format!("invalid canonical method key '{key}'"));
        }
        if invalid_name(&method.name) {
            errors.push(format!("invalid transport method name '{}'", method.name));
        }
        if op_state_store::exact_schema::validate_exact_schema_document(
            &serde_json::to_value(&method.args).unwrap_or(Value::Null),
        )
        .is_err()
        {
            errors.push(format!("method '{key}' has an invalid argument schema"));
        }
        if method.returns.as_ref().is_some_and(|returns| {
            op_state_store::exact_schema::validate_exact_schema_document(
                &serde_json::to_value(returns).unwrap_or(Value::Null),
            )
            .is_err()
        }) {
            errors.push(format!("method '{key}' has an invalid return schema"));
        }
    }
    errors
}

fn invalid_name(value: &str) -> bool {
    value.is_empty()
        || value.len() > MAX_ID_BYTES
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
}

fn build_summary(schema: &PluginSchema) -> Result<Value> {
    ensure!(
        schema.fields.len().saturating_add(schema.methods.len()) <= MAX_SUMMARY_ITEMS,
        "PluginSchema exceeds Canvas summary item limit"
    );
    let mut elements = BTreeMap::new();
    let mut children = vec!["title".to_owned(), "version".to_owned()];
    elements.insert(
        "title".to_owned(),
        json!({"type":"heading","props":{"text":schema.display_name.as_deref().unwrap_or(&schema.name),"level":2},"children":[]}),
    );
    elements.insert(
        "version".to_owned(),
        json!({"type":"text","props":{"text":format!("Plugin {} · schema {}", schema.name, schema.version),"className":null},"children":[]}),
    );
    if !schema.fields.is_empty() {
        children.push("fields_heading".into());
        elements.insert("fields_heading".into(), json!({"type":"heading","props":{"text":format!("Fields ({})", schema.fields.len()),"level":3},"children":[]}));
        let mut fields: Vec<_> = schema.fields.iter().collect();
        fields.sort_by(|left, right| left.0.cmp(right.0));
        for (index, (name, field)) in fields.into_iter().enumerate() {
            let id = format!("field_{index}");
            children.push(id.clone());
            elements.insert(id, json!({"type":"text","props":{"text":format!("{name} · required={} · read_only={}", field.required, field.read_only),"className":null},"children":[]}));
        }
    }
    if !schema.methods.is_empty() {
        children.push("methods_heading".into());
        elements.insert("methods_heading".into(), json!({"type":"heading","props":{"text":format!("Methods ({})", schema.methods.len()),"level":3},"children":[]}));
        let mut methods: Vec<_> = schema.methods.iter().collect();
        methods.sort_by(|left, right| left.0.cmp(right.0));
        for (index, (key, method)) in methods.into_iter().enumerate() {
            let id = format!("method_{index}");
            children.push(id.clone());
            elements.insert(id, json!({"type":"text","props":{"text":format!("{key} · transport={} · capability={}", method.name, schema.resolve_method_capability(method).capability()),"className":null},"children":[]}));
        }
    }
    elements.insert(
        "root".to_owned(),
        json!({"type":"container","props":{"className":null},"children":children}),
    );
    Ok(json!({"root":"root","elements":elements}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gallery_targets_reject_foreign_owners_and_preserve_stale_artifact_admission() {
        let list = plan_gallery_method("list_galleries", &json!({})).unwrap();
        assert_eq!(list.method, "get_ui_workspace");
        assert_eq!(list.capability, "gemma.read");
        assert!(plan_gallery_method("list_galleries", &json!({"owner":"other"})).is_err());
        assert!(plan_gallery_method(
            "get_rendering",
            &json!({
                "gallery_id":"other", "rendering_id":"one"
            })
        )
        .is_err());
        let get = plan_gallery_method(
            "get_rendering",
            &json!({
                "gallery_id":"workspace", "rendering_id":"one"
            }),
        )
        .unwrap();
        assert_eq!(get.method, "get_ui_spec");
        assert_eq!(get.args, json!({"spec_id":"one"}));
        let missing = complete_gallery_method(
            "get_rendering",
            json!({
                "spec":null, "render_admitted":false
            }),
        )
        .unwrap();
        assert_eq!(
            missing,
            json!({
                "gallery_id":"workspace", "rendering":null, "render_admitted":false
            })
        );
        let stale = json!({
            "id":"one", "revision":1, "catalog_hash":"a".repeat(64),
            "spec":{"root":"text","elements":{}}, "tags":[],
            "created_at":1, "updated_at":1
        });
        let result = complete_gallery_method(
            "get_rendering",
            json!({
                "spec":stale, "render_admitted":false
            }),
        )
        .unwrap();
        assert_eq!(result["rendering"], stale);
        assert_eq!(result["render_admitted"], false);
    }

    fn validator() -> PinnedCanvasValidator {
        let schema = crate::state_plugins::json_render::json_render_schema();
        let json = serde_json::to_string(&schema).unwrap();
        PinnedCanvasValidator::from_manifest_pinned_schema(json.into(), "a".repeat(64)).unwrap()
    }

    #[test]
    fn renders_only_components_declared_by_the_pinned_canvas_profile() {
        let service = SchemaRendererMethods::new(validator()).unwrap();
        let target = crate::state_plugins::gemma_brain::gemma_brain_schema();
        let output = service
            .dispatch("render_schema", &target, &"b".repeat(64))
            .unwrap();
        let allowed: BTreeSet<_> = service
            .validator
            .profile()
            .components
            .iter()
            .map(|item| item.name.as_str())
            .collect();
        for element in output["rendering"]["elements"]
            .as_object()
            .unwrap()
            .values()
        {
            assert!(allowed.contains(element["type"].as_str().unwrap()));
        }
        assert_eq!(output["schema_hash"], "b".repeat(64));
        assert_eq!(output["renderer_schema_hash"], "a".repeat(64));
    }

    #[test]
    fn validates_transport_names_independently_and_requires_workspace_dispatch() {
        let service = SchemaRendererMethods::new(validator()).unwrap();
        let mut target = crate::state_plugins::gemma_brain::gemma_brain_schema();
        target.methods.get_mut("classify").unwrap().name = "bad/name".into();
        let output = service
            .dispatch("validate_schema", &target, &"b".repeat(64))
            .unwrap();
        assert_eq!(output["valid"], false);
        assert!(output["errors"][0]
            .as_str()
            .unwrap()
            .contains("transport method"));
        assert!(service
            .dispatch("list_galleries", &target, &"b".repeat(64))
            .unwrap_err()
            .to_string()
            .contains("authenticated workspace dispatch"));
    }

    #[test]
    fn profile_is_the_actual_static_canvas_not_reference_catalog() {
        let service = SchemaRendererMethods::new(validator()).unwrap();
        let names: BTreeSet<_> = service
            .validator
            .profile()
            .components
            .iter()
            .map(|item| item.name.as_str())
            .collect();
        assert!(names.contains("container"));
        assert!(names.contains("heading"));
        assert!(names.contains("text"));
        assert!(!names.contains("Card"));
        assert!(!names.contains("Table"));
    }
}
