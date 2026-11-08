//! FreeDesktop facade over pinned contracts and committed public state.
//! Mirrors the retained discovery helper's public policy and field-write rules,
//! but has no dependency on its retired execution owner or raw SHM read path.
use std::{collections::BTreeMap, time::Duration};

use anyhow::{Context, Result, bail, ensure};
use op_plugins::{canonical, state_plugins::freedesktop::*};
use op_state_store::{FieldType, PluginSchema, SideEffect};
use serde_json::{Value, json};
use tokio::sync::OnceCell;

use crate::{
    core::{BackendStateUpdate, StateScope},
    runtime::{BackendContext, BackendOutcome, RuntimeError},
    sealed_schema_reader::read_manifest_pinned_schema_document,
};

struct PresentPlugin {
    schema: PluginSchema,
    raw_schema: Value,
    // Raw guards are admitted only for native Set authorization; discovery
    // retains public-only values and cannot become a hidden-guard oracle.
    guard_state: Option<Value>,
    raw_guards: bool,
    object: ManagedPluginObject,
}

struct WritePlan {
    plugin: String,
    property: String,
    method: String,
    arguments: Value,
}

pub(super) async fn dispatch(
    ctx: &BackendContext<'_>, method: &str, args: &Value,
) -> Result<BackendOutcome, RuntimeError> {
    let result = match method {
        "GetManagedObjects" => {
            serde_json::from_value::<GetManagedObjectsInput>(args.clone())
                .map_err(|error| RuntimeError::before_effect(error.into()))?;
            let runtime = ctx.runtime.clone();
            let objects = tokio::task::spawn_blocking(move || {
                runtime.catalog().plugins().iter().map(|entry| {
                    present(&runtime, &entry.id).map(|present| present.object)
                }).collect::<Result<Vec<_>>>()
            }).await.map_err(|error| RuntimeError::backend(error.into()))?
                .map_err(RuntimeError::backend)?;
            serde_json::to_value(GetManagedObjectsOutput { objects })?
        }
        "Get" => {
            let input: GetPropertyInput = decode(args)?;
            check_interface(&input.interface_name).map_err(RuntimeError::before_effect)?;
            let present = load(ctx, &input.plugin_id).await?;
            ensure_declared_property(&present, &input.property_name).map_err(RuntimeError::before_effect)?;
            let value = present.object.properties.get(&input.property_name).cloned()
                .ok_or_else(|| RuntimeError::unsupported("declared property has no committed public value"))?;
            let read_only = !writable(&present, &input.property_name);
            serde_json::to_value(GetPropertyOutput {
                plugin_id: input.plugin_id, property_name: input.property_name, value,
                read_only, schema_hash: present.object.schema_hash,
            })?
        }
        "GetAll" => {
            let input: GetAllPropertiesInput = decode(args)?;
            check_interface(&input.interface_name).map_err(RuntimeError::before_effect)?;
            serde_json::to_value(GetAllPropertiesOutput {
                object: load(ctx, &input.plugin_id).await?.object,
            })?
        }
        "Introspect" => {
            let input: IntrospectInput = decode(args)?;
            let present = load(ctx, &input.plugin_id).await?;
            serde_json::to_value(IntrospectOutput {
                xml: introspection_xml(&present).map_err(RuntimeError::backend)?,
                plugin_id: input.plugin_id,
                object_path: present.object.object_path,
                schema_hash: present.object.schema_hash,
            })?
        }
        "Set" => return set(ctx, decode(args)?).await,
        "Ping" | "GetMachineId" => {
            let plugin = if method == "Ping" {
                decode::<PingInput>(args)?.plugin_id
            } else {
                decode::<GetMachineIdInput>(args)?.plugin_id
            };
            let present = load(ctx, &plugin).await?;
            super::revalidate(ctx).await?;
            peer(&present.object, method).await.map_err(RuntimeError::backend)?
        }
        _ => return Err(RuntimeError::unsupported("undeclared FreeDesktop method")),
    };
    super::revalidate(ctx).await?;
    // Discovery replies are not a second state authority or a private-data
    // cache. Their source remains each target's committed projection.
    Ok(super::private_result(ctx, result, BackendStateUpdate::Unchanged))
}

fn decode<T: serde::de::DeserializeOwned>(args: &Value) -> Result<T, RuntimeError> {
    serde_json::from_value(args.clone()).map_err(|error| RuntimeError::before_effect(error.into()))
}

fn check_interface(interface: &str) -> Result<()> {
    ensure!(interface == canonical::PLUGIN_INTERFACE, "only canonical PluginV1 properties are supported");
    Ok(())
}

fn ensure_declared_property(present: &PresentPlugin, field: &str) -> Result<()> {
    ensure!(present.schema.fields.contains_key(field), "property is absent from sealed plugin");
    Ok(())
}

async fn load(ctx: &BackendContext<'_>, plugin: &str) -> Result<PresentPlugin, RuntimeError> {
    let runtime = ctx.runtime.clone();
    let plugin = plugin.to_owned();
    tokio::task::spawn_blocking(move || present(&runtime, &plugin))
        .await.map_err(|error| RuntimeError::backend(error.into()))?
        .map_err(RuntimeError::backend)
}

async fn load_for_write(ctx: &BackendContext<'_>, plugin: &str) -> Result<PresentPlugin, RuntimeError> {
    super::revalidate(ctx).await?;
    let runtime = ctx.runtime.clone();
    let plugin = plugin.to_owned();
    let present = tokio::task::spawn_blocking(move || -> Result<PresentPlugin> {
        let mut present = present(&runtime, &plugin)?;
        // Shared native guards belong to the global committed object, never
        // a potentially stale caller-private observation. Core checks the
        // durable head and sealed schema before releasing this internal value.
        let snapshot = runtime.core().backend_snapshot(
            &StateScope::Public, &plugin, &present.object.object_path,
        )?;
        if let Some(snapshot) = &snapshot {
            ensure!(snapshot.schema_hash == present.object.schema_hash, "native guard schema pin changed");
        }
        present.guard_state = snapshot.map(|snapshot| snapshot.state);
        present.raw_guards = true;
        Ok(present)
    }).await.map_err(|error| RuntimeError::before_effect(error.into()))?
        .map_err(RuntimeError::before_effect)?;
    super::revalidate(ctx).await?;
    Ok(present)
}

fn present(runtime: &crate::runtime::Runtime, plugin: &str) -> Result<PresentPlugin> {
    let expected = runtime.catalog().plugin(plugin).context("plugin is absent from pinned runtime catalog")?;
    let document = read_manifest_pinned_schema_document(runtime.catalog_dir(), plugin)?;
    ensure!(document.schema_hash == expected.schema_hash, "sealed plugin changed; reopen the runtime");
    let raw_schema = serde_json::from_str(&document.blob.schema_json)?;
    let path = canonical::plugin_path(plugin);
    // committed_snapshot validates the durable head and exact sealed exposure
    // policy. Never read raw global state and assume an outer grant exposes it.
    let public = runtime.core().committed_snapshot(&StateScope::Public, plugin, &path)?;
    let available = public.is_some();
    let mut public = public.map(|snapshot| snapshot.state);
    if let Some(value) = &mut public { redact_credentials(value); }
    let properties = public.as_ref().map(|value| {
        value.as_object().context("committed public state must be an object")?
            .iter().map(|(key, value)| Ok((key.clone(), value.clone())))
            .collect::<Result<BTreeMap<_, _>>>()
    }).transpose()?.unwrap_or_default();
    let guard_state = public;
    Ok(PresentPlugin {
        schema: document.schema, raw_schema, guard_state, raw_guards: false,
        object: ManagedPluginObject {
            plugin_id: plugin.to_owned(), object_path: path,
            interface_name: canonical::PLUGIN_INTERFACE.to_owned(),
            schema_hash: document.schema_hash, properties, projection_available: available,
        },
    })
}

fn redact_credentials(value: &mut Value) {
    match value {
        Value::Object(object) => {
            object.remove("sealed_id");
            for child in object.values_mut() { redact_credentials(child); }
        }
        Value::Array(values) => for child in values { redact_credentials(child); },
        _ => {}
    }
}

fn protected(field: &FieldType) -> bool {
    match field {
        FieldType::Object(fields) => fields.values().any(|field|
            field.read_only || field.read_only_when.is_some() || protected(&field.field_type)),
        FieldType::Array(inner) | FieldType::Map(inner) => protected(inner),
        FieldType::OneOf(branches) => branches.iter().any(protected),
        FieldType::JsonSchema(_) => true,
        _ => false,
    }
}

fn writable(present: &PresentPlugin, property: &str) -> bool {
    let Some(field) = present.schema.fields.get(property) else { return false; };
    let Some(binding) = present.schema.field_writes.get(property) else { return false; };
    let Some(method) = present.schema.methods.get(&binding.method) else { return false; };
    if method.name != binding.method || method.side_effect != SideEffect::Mutation
        || !method.required_capability.as_ref().is_some_and(|cap|
            !cap.is_empty() && present.schema.capabilities.contains_key(cap))
        || (present.schema.name == "freedesktop" && binding.method == "Set")
        || present.schema.tags.iter().any(|tag| tag == "immutable")
        || field.read_only || protected(&field.field_type) {
        return false;
    }
    let pointer = format!("/{}", property.replace('~', "~0").replace('/', "~1"));
    if present.schema.immutable_paths.iter().any(|path|
        path.is_empty() || path == "/" || path == &pointer || path.starts_with(&format!("{pointer}/"))) {
        return false;
    }
    if let Some(condition) = &field.read_only_when {
        let guard_path = format!("/{}", condition.property.replace('~', "~0").replace('/', "~1"));
        if !present.raw_guards && !present.schema.public_state.paths.contains(&guard_path) {
            return false; // Public discovery must not reveal a hidden guard.
        }
        let Some(value) = present.guard_state.as_ref().and_then(|state| state.get(&condition.property)) else {
            return false; // An unavailable guard is not unlocked by a schema default.
        };
        if !present.schema.fields.contains_key(&condition.property) { return false; }
        let mut schema = present.schema.clone();
        schema.fields.retain(|key, _| key == &condition.property);
        let Ok(state) = serde_json::from_value(json!({(condition.property.clone()): value})) else { return false; };
        if !schema.validate(&state).valid { return false; }
        if value.as_str().map_or_else(|| value.to_string() == condition.value, |value| value == condition.value) {
            return false;
        }
    }
    true
}

fn write_plan(present: &PresentPlugin, input: SetPropertyInput) -> Result<WritePlan> {
    check_interface(&input.interface_name)?;
    ensure!(input.plugin_id == present.schema.name && input.plugin_id == present.object.plugin_id,
        "property target differs from sealed plugin");
    ensure!(writable(present, &input.property_name), "property is read-only through generic Set");
    let binding = &present.schema.field_writes[&input.property_name];
    ensure!(!binding.argument.is_empty(), "field write argument is empty");
    let method = &present.schema.methods[&binding.method];
    let mut field_schema = present.schema.clone();
    field_schema.fields.retain(|name, _| name == &input.property_name);
    let state = serde_json::from_value(json!({(input.property_name.clone()): &input.value}))?;
    ensure!(field_schema.validate(&state).valid, "property value fails sealed field validation");
    let arguments = json!({(binding.argument.clone()): input.value});
    let args_schema = serde_json::to_value(&method.args)?;
    ensure!(args_schema["properties"].get(&binding.argument).is_some(), "mapped argument is not declared");
    crate::method_contract_admission::validate_admitted_arguments(method, &arguments)?;
    Ok(WritePlan {
        plugin: input.plugin_id, property: input.property_name,
        method: binding.method.clone(), arguments,
    })
}

async fn set(ctx: &BackendContext<'_>, input: SetPropertyInput) -> Result<BackendOutcome, RuntimeError> {
    let present = load(ctx, &input.plugin_id).await?;
    check_interface(&input.interface_name).map_err(RuntimeError::before_effect)?;
    let method = present.schema.field_writes.get(&input.property_name)
        .map(|binding| binding.method.as_str())
        .ok_or_else(|| RuntimeError::denied("property has no sealed native write binding"))?;
    // freedesktop.write conveys no authority over the target's mutation.
    // Admit the exact original caller before observing hidden native guards.
    ctx.runtime.authorize_nested(ctx.caller, &input.plugin_id, method).await?;
    let current = load_for_write(ctx, &input.plugin_id).await?;
    if current.object.schema_hash != present.object.schema_hash {
        return Err(RuntimeError::denied("target field contract changed before execution"));
    }
    let plan = write_plan(&current, input).map_err(RuntimeError::before_effect)?;
    let result = ctx.runtime.call_nested(ctx.caller, &plan.plugin, &plan.method, plan.arguments).await?;
    result.receipt.validate_attribution(&plan.plugin, &plan.method)
        .map_err(RuntimeError::unknown)?;
    super::revalidate(ctx).await.map_err(|error| RuntimeError::unknown(error.into()))?;
    let event_id = result.receipt.event_id;
    let event_hash = result.receipt.event_hash.clone();
    let result = serde_json::to_value(SetPropertyOutput {
        plugin_id: plan.plugin, property_name: plan.property, method: plan.method,
        result: serde_json::to_value(result.receipt).map_err(|error| RuntimeError::unknown(error.into()))?,
        event_id, event_hash,
    }).map_err(|error| RuntimeError::unknown(error.into()))?;
    Ok(super::private_result(ctx, result, BackendStateUpdate::Unchanged))
}

async fn peer(object: &ManagedPluginObject, method: &str) -> Result<Value> {
    static CONNECTION: OnceCell<zbus::Connection> = OnceCell::const_new();
    tokio::time::timeout(Duration::from_secs(5), async {
        let connection = CONNECTION.get_or_try_init(|| async {
            let address = std::env::var("DBUS_SESSION_BUS_ADDRESS")
                .unwrap_or_else(|_| op_core::config::SESSION_BUS_ADDRESS.to_owned());
            Ok::<_, anyhow::Error>(zbus::connection::Builder::address(address.as_str())?.build().await?)
        }).await?;
        let proxy = zbus::fdo::PeerProxy::builder(connection)
            .destination(canonical::BASE_SERVICE_NAME)?.path(object.object_path.as_str())?
            .build().await?;
        match method {
            "Ping" => {
                proxy.ping().await?;
                Ok(serde_json::to_value(PingOutput {
                    plugin_id: object.plugin_id.clone(), object_path: object.object_path.clone(),
                    schema_hash: object.schema_hash.clone(),
                })?)
            }
            "GetMachineId" => {
                let machine_id = proxy.get_machine_id().await?;
                ensure!(machine_id.len() == 32 && machine_id.bytes().all(|byte| byte.is_ascii_hexdigit()),
                    "canonical peer returned an invalid machine ID");
                Ok(serde_json::to_value(GetMachineIdOutput { plugin_id: object.plugin_id.clone(), machine_id })?)
            }
            _ => bail!("unknown Peer operation"),
        }
    }).await.context("canonical D-Bus peer request timed out")?
}

fn introspection_xml(present: &PresentPlugin) -> Result<String> {
    let metadata = xml_escape(&serde_json::to_string(&present.raw_schema)?);
    let path = xml_escape(&present.object.object_path);
    let hash = xml_escape(&present.object.schema_hash);
    Ok(format!(r#"<node name="{path}">
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
</node>"#))
}

fn xml_escape(value: &str) -> String {
    value.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
        .replace('"', "&quot;").replace('\'', "&apos;")
}
