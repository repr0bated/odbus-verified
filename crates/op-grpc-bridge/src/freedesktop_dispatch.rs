//! FreeDesktop operations over the canonical sealed plugin tree.
//!
//! Target selection never accepts a bus, destination, or arbitrary object path.
//! Property writes are plans for the authenticated MutationEngine dispatcher;
//! this module cannot mutate SHM or invoke an unadmitted backend operation.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail, ensure};
use op_plugins::canonical;
use op_plugins::state_plugins::freedesktop::*;
use op_state_store::{FieldType, PluginSchema, SideEffect};
use serde_json::{Value, json};

use crate::mutation_engine::MutationEngine;
use crate::sealed_schema_reader::{
    blob_catalog_dir_from_env, manifest_plugins, read_manifest_pinned_schema_document,
    valid_plugin_id,
};

#[derive(Debug)]
pub(crate) enum DispatchOutcome {
    Reply(Value),
    Write(FieldWritePlan),
}

/// This is a routing plan, never evidence of caller authorization. The engine
/// must admit the original verified identity against this target's current
/// sealed method contract and exact capability grants before dispatch.
#[derive(Debug, Clone)]
pub(crate) struct FieldWritePlan {
    pub target_plugin: String,
    pub property_name: String,
    pub method: String,
    pub args: Value,
    pub required_capability: String,
    pub schema_hash: String,
}

struct PresentPlugin {
    schema: PluginSchema,
    raw_schema: Value,
    // Committed internal state for mutation lock checks. This value is never
    // serialized as a public property or substituted for an exposure policy.
    raw_state: Option<Value>,
    object: ManagedPluginObject,
}

pub(crate) async fn prepare(
    engine: &MutationEngine,
    method: &str,
    args: &Value,
) -> Result<DispatchOutcome> {
    let catalog_dir = blob_catalog_dir_from_env();
    let state_dir = PathBuf::from(op_core::projection_shm::shm_state_dir());
    if matches!(method, "Ping" | "GetMachineId") {
        let plugin_id = if method == "Ping" {
            serde_json::from_value::<PingInput>(args.clone())?.plugin_id
        } else {
            serde_json::from_value::<GetMachineIdInput>(args.clone())?.plugin_id
        };
        let present = tokio::task::spawn_blocking(move || {
            read_present_plugin(&catalog_dir, &state_dir, &plugin_id)
        })
        .await
        .context("sealed plugin read task failed")??;
        // Both Peer operations are real bounded D-Bus round trips to this
        // canonical object. A bus GUID is not substituted for a machine ID.
        let reply = tokio::time::timeout(Duration::from_secs(5), async {
            let connection = engine.session_bus().await?;
            let proxy = zbus::fdo::PeerProxy::builder(&connection)
                .destination(canonical::BASE_SERVICE_NAME)?
                .path(present.object.object_path.as_str())?
                .build()
                .await?;
            if method == "Ping" {
                proxy.ping().await?;
                Ok::<_, anyhow::Error>(serde_json::to_value(PingOutput {
                    plugin_id: present.object.plugin_id.clone(),
                    object_path: present.object.object_path.clone(),
                    schema_hash: present.object.schema_hash.clone(),
                })?)
            } else {
                let machine_id = proxy.get_machine_id().await?;
                ensure!(
                    machine_id.len() == 32
                        && machine_id.bytes().all(|byte| byte.is_ascii_hexdigit()),
                    "canonical peer returned an invalid machine ID"
                );
                Ok(serde_json::to_value(GetMachineIdOutput {
                    plugin_id: present.object.plugin_id.clone(),
                    machine_id,
                })?)
            }
        })
        .await
        .context("canonical peer request timed out")??;
        return Ok(DispatchOutcome::Reply(reply));
    }
    let method = method.to_owned();
    let args = args.clone();
    tokio::task::spawn_blocking(move || prepare_local(&catalog_dir, &state_dir, &method, &args))
        .await
        .context("FreeDesktop catalog read task failed")?
}

fn prepare_local(
    catalog_dir: &Path,
    state_dir: &Path,
    method: &str,
    args: &Value,
) -> Result<DispatchOutcome> {
    let result = match method {
        "GetManagedObjects" => {
            serde_json::from_value::<GetManagedObjectsInput>(args.clone())?;
            let manifest = manifest_plugins(catalog_dir)?;
            let ids = manifest
                .keys()
                .filter(|id| !op_plugins::default_registry::is_retired_plugin(id))
                .cloned()
                .collect::<Vec<_>>();
            let objects = read_present_plugins(catalog_dir, state_dir, &ids)?
                .into_values()
                .map(|present| present.object)
                .collect();
            ensure!(
                manifest_plugins(catalog_dir)? == manifest,
                "sealed catalog changed during enumeration; retry the request"
            );
            serde_json::to_value(GetManagedObjectsOutput { objects })?
        }
        "Get" => {
            let input: GetPropertyInput = serde_json::from_value(args.clone())?;
            check_interface(&input.interface_name)?;
            let present = read_present_plugin(catalog_dir, state_dir, &input.plugin_id)?;
            ensure!(
                present.schema.fields.contains_key(&input.property_name),
                "property is not declared by the sealed plugin"
            );
            let value = present
                .object
                .properties
                .get(&input.property_name)
                .cloned()
                .context("declared property has no public present-state value")?;
            let read_only = public_field_is_read_only(&present, &input.property_name);
            serde_json::to_value(GetPropertyOutput {
                plugin_id: input.plugin_id,
                property_name: input.property_name,
                value,
                read_only,
                schema_hash: present.object.schema_hash,
            })?
        }
        "GetAll" => {
            let input: GetAllPropertiesInput = serde_json::from_value(args.clone())?;
            check_interface(&input.interface_name)?;
            let present = read_present_plugin(catalog_dir, state_dir, &input.plugin_id)?;
            serde_json::to_value(GetAllPropertiesOutput {
                object: present.object,
            })?
        }
        "Introspect" => {
            let input: IntrospectInput = serde_json::from_value(args.clone())?;
            let present = read_present_plugin(catalog_dir, state_dir, &input.plugin_id)?;
            serde_json::to_value(IntrospectOutput {
                xml: introspection_xml(&present)?,
                plugin_id: input.plugin_id,
                object_path: present.object.object_path,
                schema_hash: present.object.schema_hash,
            })?
        }
        "Set" => {
            let input: SetPropertyInput = serde_json::from_value(args.clone())?;
            check_interface(&input.interface_name)?;
            let present = read_present_plugin(catalog_dir, state_dir, &input.plugin_id)?;
            return Ok(DispatchOutcome::Write(resolve_write(&present, input)?));
        }
        _ => bail!("unknown FreeDesktop method: {method}"),
    };
    Ok(DispatchOutcome::Reply(result))
}

fn check_interface(interface: &str) -> Result<()> {
    ensure!(
        interface == canonical::PLUGIN_INTERFACE,
        "only the canonical PluginV1 interface is supported"
    );
    Ok(())
}

fn read_present_plugin(catalog: &Path, state: &Path, plugin_id: &str) -> Result<PresentPlugin> {
    read_present_plugins(catalog, state, &[plugin_id.to_owned()])?
        .remove(plugin_id)
        .context("sealed plugin disappeared during state read")
}

fn read_present_plugins(
    catalog: &Path,
    state: &Path,
    ids: &[String],
) -> Result<BTreeMap<String, PresentPlugin>> {
    let before = manifest_plugins(catalog)?;
    let mut documents = BTreeMap::new();
    for plugin_id in ids {
        ensure!(
            valid_plugin_id(plugin_id) && !matches!(plugin_id.as_str(), "." | ".."),
            "invalid canonical plugin ID"
        );
        let expected = before
            .get(plugin_id)
            .context("plugin is absent from the sealed catalog")?;
        let document = read_manifest_pinned_schema_document(catalog, plugin_id)?;
        ensure!(
            &document.schema_hash == expected,
            "sealed plugin changed before its state read"
        );
        documents.insert(plugin_id.clone(), document);
    }
    // One shared filesystem lock covers every selected object and generation.
    // The complete list cannot combine independently read state revisions.
    let snapshot = op_core::projection_shm::read_projection_snapshot_in(
        state.to_str().context("state directory is not UTF-8")?,
        ids,
    )?;
    ensure!(
        manifest_plugins(catalog)? == before,
        "sealed catalog changed during state read; retry the request"
    );
    let mut present = BTreeMap::new();
    for (plugin_id, document) in documents {
        let raw_schema = serde_json::from_str(&document.blob.schema_json)?;
        let schema = document.schema;
        let raw = snapshot
            .states
            .get(&plugin_id)
            .map(|bytes| {
                let mut bytes = bytes.clone();
                simd_json::to_owned_value(&mut bytes)
                    .context("invalid committed SHM projection JSON")
            })
            .transpose()?;
        if let Some(state) = &raw {
            ensure!(
                matches!(state, simd_json::OwnedValue::Object(_)),
                "committed plugin projection must be an object"
            );
        }
        schema.validate_public_state_policy()?;
        let projected = raw
            .as_ref()
            .map(|state| schema.project_public_state(state))
            .transpose()?
            .flatten();
        let projection_available = projected.is_some();
        let mut projection = projected
            .map(serde_json::to_value)
            .transpose()?
            .unwrap_or_else(|| json!({}));
        // Credential redaction is defense in depth, not the source of public
        // authority. Only the exact sealed policy selected these fields.
        redact_credentials(&mut projection);
        let properties = projection
            .as_object()
            .context("public projection must be an object")?
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
        present.insert(
            plugin_id.clone(),
            PresentPlugin {
                schema,
                raw_schema,
                raw_state: raw.map(serde_json::to_value).transpose()?,
                object: ManagedPluginObject {
                    plugin_id: plugin_id.clone(),
                    object_path: canonical::plugin_path(&plugin_id),
                    interface_name: canonical::PLUGIN_INTERFACE.to_owned(),
                    schema_hash: document.schema_hash,
                    properties,
                    projection_available,
                },
            },
        );
    }
    Ok(present)
}

fn redact_credentials(value: &mut Value) {
    match value {
        Value::Object(object) => {
            object.remove("sealed_id");
            for value in object.values_mut() {
                redact_credentials(value);
            }
        }
        Value::Array(values) => {
            for value in values {
                redact_credentials(value);
            }
        }
        _ => {}
    }
}

fn public_field_is_read_only(present: &PresentPlugin, property: &str) -> bool {
    let Some(field) = present.schema.fields.get(property) else {
        return true;
    };
    if let Some(condition) = &field.read_only_when {
        let public = present.object.properties.get(&condition.property);
        let raw = present
            .raw_state
            .as_ref()
            .and_then(|state| state.get(&condition.property));
        // Read metadata must not reveal a hidden guard's value. A partially
        // exposed object guard is hidden too; only Set evaluates raw guards.
        if public.is_none() || public != raw {
            return true;
        }
    }
    !field_is_writable(present, property)
}

fn field_is_writable(present: &PresentPlugin, property: &str) -> bool {
    let Some(field) = present.schema.fields.get(property) else {
        return false;
    };
    let Some(binding) = present.schema.field_writes.get(property) else {
        return false;
    };
    let Some(method) = present.schema.methods.get(&binding.method) else {
        return false;
    };
    if method.name != binding.method
        || method.side_effect != SideEffect::Mutation
        || !method
            .required_capability
            .as_ref()
            .is_some_and(|capability| {
                !capability.is_empty() && present.schema.capabilities.contains_key(capability)
            })
        || (present.schema.name == "freedesktop" && binding.method == "Set")
        || present.schema.tags.iter().any(|tag| tag == "immutable")
        || field.read_only
        || has_protected_nested_fields(&field.field_type)
    {
        return false;
    }
    let pointer = format!("/{}", property.replace('~', "~0").replace('/', "~1"));
    if present.schema.immutable_paths.iter().any(|path| {
        path.is_empty()
            || path == "/"
            || path == &pointer
            || path.starts_with(&format!("{pointer}/"))
    }) {
        return false;
    }
    if let Some(condition) = &field.read_only_when {
        let Some(value) = present
            .raw_state
            .as_ref()
            .and_then(|state| state.get(&condition.property))
        else {
            // Do not guess the current lock condition from defaults.
            return false;
        };
        if !present.schema.fields.contains_key(&condition.property) {
            return false;
        }
        let mut condition_schema = present.schema.clone();
        condition_schema
            .fields
            .retain(|key, _| key == &condition.property);
        let Ok(condition_state) =
            serde_json::from_value(json!({(condition.property.clone()): value}))
        else {
            return false;
        };
        if !condition_schema.validate(&condition_state).valid {
            return false;
        }
        if value
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| value.to_string())
            == condition.value
        {
            return false;
        }
    }
    true
}

// A whole-property Set cannot safely preserve protected nested fields. Such
// contracts retain their explicitly typed mutation methods, not generic Set.
fn has_protected_nested_fields(field_type: &FieldType) -> bool {
    match field_type {
        FieldType::Object(fields) => fields.values().any(|field| {
            field.read_only
                || field.read_only_when.is_some()
                || has_protected_nested_fields(&field.field_type)
        }),
        FieldType::Array(inner) | FieldType::Map(inner) => has_protected_nested_fields(inner),
        FieldType::OneOf(branches) => branches.iter().any(has_protected_nested_fields),
        FieldType::JsonSchema(_) => true, // only its explicit native mutation contract may write it
        _ => false,
    }
}

fn resolve_write(present: &PresentPlugin, input: SetPropertyInput) -> Result<FieldWritePlan> {
    ensure!(
        input.plugin_id == present.schema.name && input.plugin_id == present.object.plugin_id,
        "property target does not match the sealed plugin"
    );
    ensure!(
        field_is_writable(present, &input.property_name),
        "property is read-only through generic Set"
    );
    let binding = &present.schema.field_writes[&input.property_name];
    ensure!(
        !(input.plugin_id == "freedesktop" && binding.method == "Set"),
        "recursive FreeDesktop Set binding is forbidden"
    );
    let method = present
        .schema
        .methods
        .get(&binding.method)
        .context("field write references an undeclared method")?;
    ensure!(
        method.name == binding.method,
        "field write method identity mismatch"
    );
    ensure!(
        method.side_effect == SideEffect::Mutation,
        "field write must reference a mutation method"
    );
    let required_capability = method
        .required_capability
        .as_ref()
        .filter(|cap| !cap.is_empty())
        .context("field write mutation has no explicit capability")?;
    ensure!(
        present
            .schema
            .capabilities
            .contains_key(required_capability),
        "field write mutation capability is undeclared"
    );
    ensure!(
        !binding.argument.is_empty(),
        "field write argument is empty"
    );

    // Apply the plugin's field validation as well as the actual target method
    // schema. Building a one-field view avoids requiring unrelated state that
    // might not have a present projection yet; it does not mint a new schema.
    let mut field_schema = present.schema.clone();
    field_schema
        .fields
        .retain(|name, _| name == &input.property_name);
    let value = json!({ (input.property_name.clone()): input.value.clone() });
    let validation = field_schema.validate(&serde_json::from_value(value)?);
    ensure!(
        validation.valid,
        "invalid field value: {}",
        validation.errors.join("; ")
    );
    let args = json!({ (binding.argument.clone()): input.value });
    let args_schema = serde_json::to_value(&method.args)?;
    ensure!(
        args_schema["properties"].get(&binding.argument).is_some(),
        "field write argument is not declared by the method"
    );
    let validator = jsonschema::Validator::new(&args_schema)
        .map_err(|error| anyhow::anyhow!("invalid sealed mutation schema: {error}"))?;
    if let Err(error) = validator.validate(&args) {
        bail!("field write does not satisfy target method input: {error}");
    }
    Ok(FieldWritePlan {
        target_plugin: input.plugin_id,
        property_name: input.property_name,
        method: binding.method.clone(),
        args,
        required_capability: required_capability.clone(),
        schema_hash: present.object.schema_hash.clone(),
    })
}

/// Re-read the pinned catalog and current lock metadata before the engine
/// admits the target mutation. No source-registry lookup or consumer rehash.
pub(crate) fn revalidate_write(plan: &FieldWritePlan) -> Result<()> {
    revalidate_write_in(
        &blob_catalog_dir_from_env(),
        Path::new(&op_core::projection_shm::shm_state_dir()),
        plan,
    )
}

fn revalidate_write_in(catalog: &Path, state: &Path, plan: &FieldWritePlan) -> Result<()> {
    let present = read_present_plugin(catalog, state, &plan.target_plugin)?;
    ensure!(
        present.object.schema_hash == plan.schema_hash,
        "target plugin was resealed after the property write was prepared"
    );
    let binding = present
        .schema
        .field_writes
        .get(&plan.property_name)
        .context("target field write mapping was removed")?;
    let value = plan
        .args
        .get(&binding.argument)
        .cloned()
        .context("prepared property write is missing the mapped argument")?;
    let current = resolve_write(
        &present,
        SetPropertyInput {
            plugin_id: plan.target_plugin.clone(),
            interface_name: canonical::PLUGIN_INTERFACE.to_owned(),
            property_name: plan.property_name.clone(),
            value,
        },
    )?;
    ensure!(
        current.method == plan.method
            && current.args == plan.args
            && current.required_capability == plan.required_capability,
        "prepared property write no longer matches the sealed mutation contract"
    );
    Ok(())
}

/// Exact target-grant check, after SID1/principal verification and the sealed
/// revalidation above. The outer freedesktop.write grant conveys no target
/// authority, and a caller-supplied actor string is not identity evidence.
pub(crate) fn authorize_write(
    plan: &FieldWritePlan,
    actor_id: &str,
    grants: &HashSet<String>,
) -> Result<()> {
    ensure!(
        !actor_id.trim().is_empty(),
        "property write requires a verified principal"
    );
    ensure!(
        !plan.required_capability.is_empty() && grants.contains(&plan.required_capability),
        "principal {actor_id} lacks target mutation capability {}",
        plan.required_capability
    );
    Ok(())
}

/// Called only after the engine's independently admitted target mutation
/// succeeds. Preserve its actual result and audit receipt, not the Set input.
pub(crate) fn finish_write(plan: &FieldWritePlan, target_result: Value) -> Result<Value> {
    ensure!(
        target_result["success"] == true
            && target_result["plugin_id"] == plan.target_plugin
            && target_result["method"] == plan.method,
        "target mutation receipt does not match the admitted write"
    );
    let event_id = target_result["event_id"]
        .as_u64()
        .context("target mutation returned no event ID")?;
    let event_hash = target_result["event_hash"]
        .as_str()
        .context("target mutation returned no event hash")?
        .to_owned();
    ensure!(
        !event_hash.is_empty(),
        "target mutation returned an empty event hash"
    );
    Ok(serde_json::to_value(SetPropertyOutput {
        plugin_id: plan.target_plugin.clone(),
        property_name: plan.property_name.clone(),
        method: plan.method.clone(),
        result: target_result,
        event_id,
        event_hash,
    })?)
}

fn introspection_xml(present: &PresentPlugin) -> Result<String> {
    let metadata = xml_escape(&serde_json::to_string(&present.raw_schema)?);
    let path = xml_escape(&present.object.object_path);
    let hash = xml_escape(&present.object.schema_hash);
    Ok(format!(
        r#"<node name="{path}">
  <interface name="org.opdbus.v1.PluginV1">
    <annotation name="org.opdbus.v1.SchemaHash" value="{hash}"/>
    <annotation name="org.opdbus.v1.PluginSchema" value="{metadata}"/>
    <method name="Call"><arg name="method" type="s" direction="in"/><arg name="json_args" type="s" direction="in"/><arg type="s" direction="out"/></method>
    <method name="CallWithOperationId"><arg name="method" type="s" direction="in"/><arg name="json_args" type="s" direction="in"/><arg name="operation_id" type="s" direction="in"/><arg type="s" direction="out"/></method>
    <method name="GetProperty"><arg name="name" type="s" direction="in"/><arg type="s" direction="out"/></method>
    <method name="GetAllProperties"><arg type="s" direction="out"/></method>
    <method name="SetProperty"><arg name="name" type="s" direction="in"/><arg name="json_value" type="s" direction="in"/><annotation name="org.opdbus.v1.WriteRoute" value="freedesktop.Call(Set) with target mutation admission"/></method>
    <signal name="Updated"><arg name="data_json" type="s"/></signal>
  </interface>
  <interface name="org.freedesktop.DBus.Peer"><method name="Ping"/><method name="GetMachineId"><arg type="s" direction="out"/></method></interface>
  <interface name="org.freedesktop.DBus.Introspectable"><method name="Introspect"><arg type="s" direction="out"/></method></interface>
  <interface name="org.freedesktop.DBus.Properties">
    <method name="Get"><arg name="interface_name" type="s" direction="in"/><arg name="property_name" type="s" direction="in"/><arg type="v" direction="out"/></method>
    <method name="Set"><arg name="interface_name" type="s" direction="in"/><arg name="property_name" type="s" direction="in"/><arg name="value" type="v" direction="in"/></method>
    <method name="GetAll"><arg name="interface_name" type="s" direction="in"/><arg type="a{{sv}}" direction="out"/></method>
    <signal name="PropertiesChanged"><arg name="interface_name" type="s"/><arg name="changed_properties" type="a{{sv}}"/><arg name="invalidated_properties" type="as"/></signal>
  </interface>
</node>"#
    ))
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use op_state_store::{CapabilityDecl, FieldSchema, MethodDecl, ReadOnlyCondition};
    use std::process::Stdio;
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncBufReadExt, BufReader};

    struct PeerFixture;

    // zbus installs the real org.freedesktop.DBus.Peer implementation when an
    // application interface is mounted. Do not replace it with a mock Peer.
    #[zbus::interface(name = "org.opdbus.PeerFixture")]
    impl PeerFixture {
        fn fixture_name(&self) -> &str {
            "fixture_control"
        }
    }

    struct SelectionOwner {
        selected: Mutex<String>,
    }

    impl SelectionOwner {
        fn apply(&self, plan: &FieldWritePlan) -> Value {
            let selected = plan.args["selection"].as_str().unwrap().to_owned();
            *self.selected.lock().unwrap() = selected.clone();
            json!({
                "success": true,
                "plugin_id": plan.target_plugin,
                "method": plan.method,
                "event_id": 7,
                "event_hash": "fixture-selection-event",
                "result": {"selected": selected}
            })
        }
    }

    fn fixture_schema() -> PluginSchema {
        PluginSchema::builder("fixture_control")
            .description("An <isolated> & \"sealed\" fixture")
            .string_field("selected", true, "Current selection")
            .boolean_field("locked", false, "Selection lock")
            .public_state_path("/selected")
            .field_write("selected", "Select", "selection")
            .method(MethodDecl {
                name: "Select".into(),
                args: simd_json::json!({
                    "type": "object",
                    "properties": { "selection": { "type": "string", "enum": ["old", "new"] } },
                    "required": ["selection"],
                    "additionalProperties": false
                }),
                returns: None,
                side_effect: SideEffect::Mutation,
                idempotent: true,
                required_capability: Some("fixture_control.select".into()),
                subid: "mut.software.fixture-control.select@v1".into(),
            })
            .capability(CapabilityDecl {
                id: "fixture_control.select".into(),
                description: "Select a fixture value".into(),
            })
            .build()
    }

    fn seal(catalog: &Path, schema: PluginSchema) {
        let id = schema.name.clone();
        op_blob::BlobStore::open(catalog)
            .unwrap()
            .write(&op_blob::blobify_plugin_schema(&id, schema))
            .unwrap();
    }

    fn fixture() -> (tempfile::TempDir, tempfile::TempDir) {
        let catalog = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        seal(catalog.path(), fixture_schema());
        commit(
            state.path(),
            br#"{"selected":"old","locked":false,"undeclared":"hidden"}"#,
        );
        (catalog, state)
    }

    fn commit(state: &Path, bytes: &[u8]) {
        op_core::projection_shm::update_projection_in(
            state.to_str().unwrap(),
            "fixture_control",
            |_| Ok(Some(bytes.to_vec())),
        )
        .unwrap();
    }

    fn input() -> SetPropertyInput {
        serde_json::from_value(
            json!({"plugin_id":"fixture_control", "property_name":"selected", "value":"new"}),
        )
        .unwrap()
    }

    fn reply(outcome: DispatchOutcome) -> Value {
        match outcome {
            DispatchOutcome::Reply(value) => value,
            DispatchOutcome::Write(_) => panic!("expected a read result"),
        }
    }

    #[test]
    fn enumeration_requires_current_blob_and_filters_public_fields() {
        let (catalog, state) = fixture();
        std::fs::write(
            state.path().join("stale_only.json"),
            r#"{"selected":"stale"}"#,
        )
        .unwrap();
        let value = reply(
            prepare_local(
                catalog.path(),
                state.path(),
                "GetManagedObjects",
                &json!({}),
            )
            .unwrap(),
        );
        let objects = value["objects"].as_array().unwrap();
        assert_eq!(objects.len(), 1);
        assert_eq!(objects[0]["plugin_id"], "fixture_control");
        assert_eq!(objects[0]["properties"], json!({"selected":"old"}));
        assert_eq!(objects[0]["projection_available"], true);
        let object = reply(
            prepare_local(
                catalog.path(),
                state.path(),
                "GetAll",
                &json!({"plugin_id":"fixture_control"}),
            )
            .unwrap(),
        );
        assert_eq!(object["object"]["plugin_id"], "fixture_control");
        assert_eq!(object["object"]["properties"], json!({"selected":"old"}));
        assert!(
            prepare_local(
                catalog.path(),
                state.path(),
                "GetAll",
                &json!({"plugin_id":"stale_only"})
            )
            .is_err()
        );
    }

    #[test]
    fn missing_projection_has_no_default_or_example_state() {
        let (catalog, state) = fixture();
        op_core::projection_shm::update_projection_in(
            state.path().to_str().unwrap(),
            "fixture_control",
            |_| Ok(None),
        )
        .unwrap();
        let value = reply(
            prepare_local(
                catalog.path(),
                state.path(),
                "GetAll",
                &json!({"plugin_id":"fixture_control"}),
            )
            .unwrap(),
        );
        assert_eq!(value["object"]["projection_available"], false);
        assert_eq!(value["object"]["properties"], json!({}));
        assert!(
            prepare_local(
                catalog.path(),
                state.path(),
                "Get",
                &json!({"plugin_id":"fixture_control", "property_name":"selected"})
            )
            .is_err()
        );
        std::fs::write(state.path().join("fixture_control.json"), b"not JSON").unwrap();
        assert!(
            prepare_local(
                catalog.path(),
                state.path(),
                "GetAll",
                &json!({"plugin_id":"fixture_control"})
            )
            .is_err()
        );
    }

    #[test]
    fn credentials_are_redacted_even_from_declared_nested_fields() {
        let (catalog, state) = fixture();
        let mut schema = fixture_schema();
        let mut field: FieldSchema = schema.fields["selected"].clone();
        field.field_type = FieldType::Array(Box::new(FieldType::Object(
            [
                ("session".into(), schema.fields["selected"].clone()),
                ("sealed_id".into(), schema.fields["selected"].clone()),
            ]
            .into_iter()
            .collect(),
        )));
        schema.fields.insert("sleds".into(), field.clone());
        schema
            .fields
            .insert("sealed_id".into(), schema.fields["selected"].clone());
        schema.public_state.paths = vec!["/sleds".into(), "/sealed_id".into()];
        seal(catalog.path(), schema);
        commit(
            state.path(),
            br#"{"sealed_id":"private","sleds":[{"session":"public","sealed_id":"private"}]}"#,
        );
        let present = read_present_plugin(catalog.path(), state.path(), "fixture_control").unwrap();
        assert!(!present.object.properties.contains_key("sealed_id"));
        assert_eq!(
            present.object.properties["sleds"],
            json!([{"session":"public"}])
        );
    }

    #[test]
    fn external_targets_and_unknown_interfaces_are_rejected() {
        let (catalog, state) = fixture();
        for args in [
            json!({"plugin_id":"fixture_control", "destination":"org.freedesktop.NetworkManager"}),
            json!({"plugin_id":"fixture_control", "object_path":"/arbitrary"}),
            json!({"plugin_id":"fixture_control", "bus":"system"}),
            json!({"plugin_id":"fixture_control", "interface_name":"org.freedesktop.NetworkManager"}),
            json!({"plugin_id":"../outside"}),
        ] {
            assert!(
                prepare_local(catalog.path(), state.path(), "GetAll", &args).is_err(),
                "accepted {args}"
            );
        }
        assert!(
            serde_json::from_value::<GetMachineIdInput>(
                json!({"plugin_id":"fixture_control", "destination":"elsewhere"})
            )
            .is_err()
        );
        assert!(
            serde_json::from_value::<PingInput>(
                json!({"plugin_id":"fixture_control", "object_path":"/elsewhere"})
            )
            .is_err()
        );
    }

    #[test]
    fn write_plan_preserves_target_capability_and_does_not_mutate_projection() {
        let (catalog, state) = fixture();
        let present = read_present_plugin(catalog.path(), state.path(), "fixture_control").unwrap();
        let before = std::fs::read(state.path().join("fixture_control.json")).unwrap();
        let plan = resolve_write(&present, input()).unwrap();
        assert_eq!(plan.target_plugin, "fixture_control");
        assert_eq!(plan.method, "Select");
        assert_eq!(plan.args, json!({"selection":"new"}));
        assert_eq!(plan.required_capability, "fixture_control.select");
        assert_eq!(plan.schema_hash, present.object.schema_hash);
        assert_eq!(
            std::fs::read(state.path().join("fixture_control.json")).unwrap(),
            before
        );
        let receipt = json!({"success":true,"plugin_id":"fixture_control","method":"Select","event_id":42,"event_hash":"abc","result":{"selected":"actual"}});
        let result = finish_write(&plan, receipt.clone()).unwrap();
        assert_eq!(result["result"], receipt);
        assert_eq!(result["event_id"], 42);
        assert!(finish_write(&plan, json!({"success":true,"plugin_id":"other","method":"Select","event_id":42,"event_hash":"abc"})).is_err());
    }

    #[test]
    fn set_facade_applies_injected_owner_effect_and_reads_back_committed_state() {
        let (catalog, state) = fixture();
        let plan = match prepare_local(
            catalog.path(),
            state.path(),
            "Set",
            &json!({
                "plugin_id":"fixture_control",
                "property_name":"selected",
                "value":"new"
            }),
        )
        .unwrap()
        {
            DispatchOutcome::Write(plan) => plan,
            DispatchOutcome::Reply(_) => panic!("expected a write plan"),
        };
        revalidate_write_in(catalog.path(), state.path(), &plan).unwrap();
        authorize_write(
            &plan,
            "fixture-principal",
            &HashSet::from(["fixture_control.select".to_owned()]),
        )
        .unwrap();

        let owner = SelectionOwner {
            selected: Mutex::new("old".into()),
        };
        let receipt = owner.apply(&plan);
        assert_eq!(*owner.selected.lock().unwrap(), "new");
        commit(
            state.path(),
            br#"{"selected":"new","locked":false,"undeclared":"hidden"}"#,
        );
        let readback = read_present_plugin(catalog.path(), state.path(), "fixture_control")
            .unwrap()
            .object
            .properties["selected"]
            .clone();
        assert_eq!(readback, "new");

        let finished = finish_write(&plan, receipt).unwrap();
        assert_eq!(finished["property_name"], "selected");
        assert_eq!(finished["result"]["result"]["selected"], "new");
        assert_eq!(finished["event_id"], 7);
    }

    #[test]
    fn write_rejects_missing_mapping_capability_and_invalid_argument_contract() {
        let (catalog, state) = fixture();
        let mut present =
            read_present_plugin(catalog.path(), state.path(), "fixture_control").unwrap();
        let valid_schema = present.schema.clone();
        let mut bad_value = input();
        bad_value.value = json!(123);
        assert!(resolve_write(&present, bad_value).is_err());
        let mut bad_value = input();
        bad_value.value = json!("not-in-target-enum");
        assert!(resolve_write(&present, bad_value).is_err());
        present.schema.field_writes.clear();
        assert!(resolve_write(&present, input()).is_err());
        present.schema = valid_schema.clone();
        present.schema.capabilities.clear();
        assert!(resolve_write(&present, input()).is_err());
        present.schema = valid_schema.clone();
        present
            .schema
            .methods
            .get_mut("Select")
            .unwrap()
            .required_capability = None;
        assert!(resolve_write(&present, input()).is_err());
        present.schema = valid_schema.clone();
        present
            .schema
            .methods
            .get_mut("Select")
            .unwrap()
            .side_effect = SideEffect::Read;
        assert!(resolve_write(&present, input()).is_err());
        present.schema = valid_schema;
        present
            .schema
            .field_writes
            .get_mut("selected")
            .unwrap()
            .argument = "undeclared_argument".into();
        assert!(resolve_write(&present, input()).is_err());
    }

    #[test]
    fn outer_freedesktop_grant_cannot_authorize_target_mutation() {
        let (catalog, state) = fixture();
        let present = read_present_plugin(catalog.path(), state.path(), "fixture_control").unwrap();
        let plan = resolve_write(&present, input()).unwrap();
        let mut grants = HashSet::from(["freedesktop.write".into(), "*".into()]);
        assert!(authorize_write(&plan, "verified-principal", &grants).is_err());
        grants.insert("fixture_control.select".into());
        assert!(authorize_write(&plan, "verified-principal", &grants).is_ok());
        assert!(authorize_write(&plan, "", &grants).is_err());
    }

    #[test]
    fn revalidation_rejects_resealed_or_relocked_target() {
        let (catalog, state) = fixture();
        let mut schema = fixture_schema();
        schema.fields.get_mut("selected").unwrap().read_only_when = Some(ReadOnlyCondition {
            property: "locked".into(),
            value: "true".into(),
        });
        seal(catalog.path(), schema.clone());
        let present = read_present_plugin(catalog.path(), state.path(), "fixture_control").unwrap();
        let plan = resolve_write(&present, input()).unwrap();
        assert!(revalidate_write_in(catalog.path(), state.path(), &plan).is_ok());
        commit(state.path(), br#"{"selected":"old","locked":true}"#);
        assert!(revalidate_write_in(catalog.path(), state.path(), &plan).is_err());
        commit(state.path(), br#"{"selected":"old","locked":false}"#);
        schema.description = "A new seal".into();
        seal(catalog.path(), schema);
        assert!(revalidate_write_in(catalog.path(), state.path(), &plan).is_err());
    }

    #[test]
    fn write_respects_immutable_and_current_lock_metadata() {
        let (catalog, state) = fixture();
        let mut present =
            read_present_plugin(catalog.path(), state.path(), "fixture_control").unwrap();
        present.schema.fields.get_mut("selected").unwrap().read_only = true;
        assert!(resolve_write(&present, input()).is_err());
        present.schema.fields.get_mut("selected").unwrap().read_only = false;
        present.schema.immutable_paths.push("/selected".into());
        assert!(resolve_write(&present, input()).is_err());
        present.schema.immutable_paths.clear();
        present
            .schema
            .fields
            .get_mut("selected")
            .unwrap()
            .read_only_when = Some(ReadOnlyCondition {
            property: "locked".into(),
            value: "true".into(),
        });
        assert!(resolve_write(&present, input()).is_ok());
        assert!(!present.object.properties.contains_key("locked"));
        present.raw_state.as_mut().unwrap()["locked"] = json!(true);
        assert!(resolve_write(&present, input()).is_err());
        present
            .raw_state
            .as_mut()
            .unwrap()
            .as_object_mut()
            .unwrap()
            .remove("locked");
        assert!(resolve_write(&present, input()).is_err());
    }

    #[test]
    fn introspection_reports_wrapper_abi_and_exact_escaped_sealed_metadata() {
        let (catalog, state) = fixture();
        let value = reply(
            prepare_local(
                catalog.path(),
                state.path(),
                "Introspect",
                &json!({"plugin_id":"fixture_control"}),
            )
            .unwrap(),
        );
        let xml = value["xml"].as_str().unwrap();
        assert!(xml.contains("<method name=\"Call\">"));
        assert!(!xml.contains("<method name=\"Select\">"));
        assert!(xml.contains("org.opdbus.v1.PluginSchema"));
        assert!(xml.contains("&lt;isolated&gt; &amp;"));
        assert!(xml.contains("field_writes"));
        assert!(xml.contains(value["schema_hash"].as_str().unwrap()));
    }

    #[test]
    fn public_read_only_metadata_does_not_disclose_hidden_lock_state() {
        let (catalog, state) = fixture();
        let mut schema = fixture_schema();
        schema.fields.get_mut("selected").unwrap().read_only_when = Some(ReadOnlyCondition {
            property: "locked".into(),
            value: "true".into(),
        });
        seal(catalog.path(), schema.clone());
        let read = || {
            reply(
                prepare_local(
                    catalog.path(),
                    state.path(),
                    "Get",
                    &json!({
                        "plugin_id":"fixture_control", "property_name":"selected"
                    }),
                )
                .unwrap(),
            )
        };
        assert_eq!(read()["read_only"], true);
        let present = read_present_plugin(catalog.path(), state.path(), "fixture_control").unwrap();
        assert!(resolve_write(&present, input()).is_ok());
        commit(state.path(), br#"{"selected":"old","locked":true}"#);
        assert_eq!(read()["read_only"], true);
        let present = read_present_plugin(catalog.path(), state.path(), "fixture_control").unwrap();
        assert!(resolve_write(&present, input()).is_err());

        schema.public_state.paths.push("/locked".into());
        seal(catalog.path(), schema);
        commit(state.path(), br#"{"selected":"old","locked":false}"#);
        assert_eq!(read()["read_only"], false);
        commit(state.path(), br#"{"selected":"old","locked":true}"#);
        assert_eq!(read()["read_only"], true);
    }

    #[test]
    fn absent_policy_withholds_values_without_disabling_valid_target_writes() {
        let (catalog, state) = fixture();
        let mut schema = fixture_schema();
        schema.public_state.paths.clear();
        schema.fields.get_mut("selected").unwrap().read_only_when = Some(ReadOnlyCondition {
            property: "locked".into(),
            value: "true".into(),
        });
        seal(catalog.path(), schema);
        let present = read_present_plugin(catalog.path(), state.path(), "fixture_control").unwrap();
        assert!(!present.object.projection_available);
        assert!(present.object.properties.is_empty());
        assert!(resolve_write(&present, input()).is_ok());
        assert!(
            prepare_local(
                catalog.path(),
                state.path(),
                "Get",
                &json!({"plugin_id":"fixture_control","property_name":"selected"})
            )
            .is_err()
        );
        commit(state.path(), br#"{"selected":"old","locked":true}"#);
        let present = read_present_plugin(catalog.path(), state.path(), "fixture_control").unwrap();
        assert!(resolve_write(&present, input()).is_err());
        commit(state.path(), br#"{"selected":"old","locked":"false"}"#);
        let present = read_present_plugin(catalog.path(), state.path(), "fixture_control").unwrap();
        assert!(
            resolve_write(&present, input()).is_err(),
            "malformed hidden guard must not open the field"
        );
    }

    #[test]
    fn opaque_declared_fields_are_not_public_and_invalid_exposure_fails_closed() {
        let (catalog, state) = fixture();
        let mut schema = fixture_schema();
        let mut field = schema.fields["selected"].clone();
        field.field_type = FieldType::Any;
        schema.fields.insert("config".into(), field);
        seal(catalog.path(), schema.clone());
        commit(
            state.path(),
            br#"{"selected":"old","locked":false,"config":{"password":"hidden"}}"#,
        );
        let present = read_present_plugin(catalog.path(), state.path(), "fixture_control").unwrap();
        assert_eq!(
            present.object.properties,
            [("selected".into(), json!("old"))].into_iter().collect()
        );
        assert!(
            prepare_local(
                catalog.path(),
                state.path(),
                "Get",
                &json!({"plugin_id":"fixture_control","property_name":"config"})
            )
            .is_err()
        );
        schema.public_state.paths.push("/config".into());
        seal(catalog.path(), schema);
        assert!(read_present_plugin(catalog.path(), state.path(), "fixture_control").is_err());
    }

    #[test]
    fn incomplete_or_missing_manifest_never_exposes_raw_projection_files() {
        let (catalog, state) = fixture();
        let manifest_path = state.path().join(".manifest.json");
        let original = std::fs::read(&manifest_path).unwrap();
        std::fs::write(&manifest_path, br#"{"generation":1,"publishing":true}"#).unwrap();
        std::fs::write(
            state.path().join("fixture_control.json"),
            br#"{"selected":"uncommitted","locked":false}"#,
        )
        .unwrap();
        for method in ["GetAll", "GetManagedObjects", "Set"] {
            let args = match method {
                "GetManagedObjects" => json!({}),
                "Set" => {
                    json!({"plugin_id":"fixture_control","property_name":"selected","value":"new"})
                }
                _ => json!({"plugin_id":"fixture_control"}),
            };
            assert!(prepare_local(catalog.path(), state.path(), method, &args).is_err());
        }
        std::fs::write(
            state.path().join("fixture_control.json"),
            br#"{"selected":"old","locked":false}"#,
        )
        .unwrap();
        std::fs::write(&manifest_path, original).unwrap();
        assert_eq!(
            read_present_plugin(catalog.path(), state.path(), "fixture_control")
                .unwrap()
                .object
                .properties["selected"],
            "old"
        );
        std::fs::remove_file(manifest_path).unwrap();
        let present = read_present_plugin(catalog.path(), state.path(), "fixture_control").unwrap();
        assert!(present.object.properties.is_empty());
        assert!(present.raw_state.is_none());
    }

    #[test]
    fn freedesktop_reader_waits_for_committed_state_instead_of_sampling_raw_file() {
        let (catalog, state) = fixture();
        let state_path = state.path().to_owned();
        let (begun_tx, begun_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let writer = std::thread::spawn(move || {
            op_core::projection_shm::update_projection_in(
                state_path.to_str().unwrap(),
                "fixture_control",
                |_| {
                    begun_tx.send(()).unwrap();
                    release_rx.recv().unwrap();
                    Ok(Some(br#"{"selected":"new","locked":false}"#.to_vec()))
                },
            )
            .unwrap();
        });
        begun_rx.recv_timeout(Duration::from_secs(3)).unwrap();
        let (read_tx, read_rx) = std::sync::mpsc::channel();
        let catalog_path = catalog.path().to_owned();
        let state_path = state.path().to_owned();
        let reader = std::thread::spawn(move || {
            let result =
                read_present_plugin(&catalog_path, &state_path, "fixture_control").unwrap();
            read_tx
                .send(result.object.properties["selected"].clone())
                .unwrap();
        });
        assert!(read_rx.recv_timeout(Duration::from_millis(30)).is_err());
        release_tx.send(()).unwrap();
        assert_eq!(read_rx.recv_timeout(Duration::from_secs(3)).unwrap(), "new");
        writer.join().unwrap();
        reader.join().unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn peer_methods_cross_private_dbus_and_read_sealed_identity() {
        // This test changes the process catalog/state environment, just like
        // the identity and generation tests. Use their same serialization
        // guard and retain it until the private environment is restored.
        let _shm_guard = crate::human_principal_dispatch::tests::temp_shm_with_schemas(&[]);
        let (catalog, state) = fixture();
        let mut command = tokio::process::Command::new("dbus-daemon");
        command
            .args(["--session", "--nofork", "--nopidfile", "--print-address=1"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        let mut daemon = command.spawn().expect("spawn private dbus-daemon");
        let stdout = daemon.stdout.take().expect("private bus stdout");
        let mut lines = BufReader::new(stdout).lines();
        let address = tokio::time::timeout(Duration::from_secs(3), lines.next_line())
            .await
            .expect("private bus address timeout")
            .expect("read private bus address")
            .expect("private bus printed no address");

        let service = zbus::connection::Builder::address(address.as_str())
            .expect("service bus address")
            .build()
            .await
            .expect("connect service to private bus");
        assert!(
            service
                .object_server()
                .at(canonical::plugin_path("fixture_control"), PeerFixture)
                .await
                .expect("mount peer fixture")
        );
        service
            .request_name(canonical::BASE_SERVICE_NAME)
            .await
            .expect("request canonical plugin name");
        let reference_client = zbus::connection::Builder::address(address.as_str())
            .unwrap()
            .build()
            .await
            .unwrap();
        let reference_path = canonical::plugin_path("fixture_control");
        let reference_peer = zbus::fdo::PeerProxy::builder(&reference_client)
            .destination(canonical::BASE_SERVICE_NAME)
            .unwrap()
            .path(reference_path.as_str())
            .unwrap()
            .build()
            .await
            .unwrap();
        let expected_machine_id = reference_peer.get_machine_id().await.unwrap();

        struct Restore(Vec<(&'static str, Option<std::ffi::OsString>)>);
        impl Drop for Restore {
            fn drop(&mut self) {
                for (key, value) in &self.0 {
                    match value {
                        // FIXME: Audit that the environment access only happens in single-threaded code.
                        Some(value) => unsafe { std::env::set_var(key, value) },
                        // FIXME: Audit that the environment access only happens in single-threaded code.
                        None => unsafe { std::env::remove_var(key) },
                    }
                }
            }
        }
        let _restore = Restore(
            [
                "OP_BLOB_CATALOG_DIR",
                "OP_SHM_STATE_DIR",
                "DBUS_SESSION_BUS_ADDRESS",
            ]
            .into_iter()
            .map(|key| (key, std::env::var_os(key)))
            .collect(),
        );
        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::set_var("OP_BLOB_CATALOG_DIR", catalog.path()) };
        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::set_var("OP_SHM_STATE_DIR", state.path()) };
        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::set_var("DBUS_SESSION_BUS_ADDRESS", address) };

        let engine = MutationEngine::new(
            Arc::new(tokio::sync::RwLock::new(op_state_store::EventChain::new(
                op_state_store::ChainConfig::default(),
            ))),
            Arc::new(op_network::rovs_proxy::OvsdbDbusClient::new()),
        );
        let ping = reply(
            prepare(&engine, "Ping", &json!({"plugin_id":"fixture_control"}))
                .await
                .expect("peer ping"),
        );
        assert_eq!(ping["plugin_id"], "fixture_control");
        assert_eq!(
            ping["object_path"],
            canonical::plugin_path("fixture_control")
        );
        assert!(!ping["schema_hash"].as_str().unwrap().is_empty());

        let machine = reply(
            prepare(
                &engine,
                "GetMachineId",
                &json!({"plugin_id":"fixture_control"}),
            )
            .await
            .expect("peer machine id"),
        );
        assert_eq!(machine["plugin_id"], "fixture_control");
        assert_eq!(machine["machine_id"], expected_machine_id);
        assert_eq!(expected_machine_id.len(), 32);
        assert!(
            expected_machine_id
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        );
    }
}
