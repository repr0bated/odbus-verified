use anyhow::{Context, Result, bail};
use serde::Serialize;
use serde_json::Value;
use zbus::Proxy;

use crate::model::SideEffect;
use crate::namespace::ProjectionTarget;
use crate::store::ObjectStore;

pub const DEFAULT_BUS_ADDRESS: &str = "unix:path=/run/opdbus/session-bus.sock";

#[derive(Debug, Serialize)]
pub struct CallReceipt {
    pub plugin_id: String,
    pub method: String,
    pub subid: String,
    pub required_capability: Option<String>,
    pub side_effect: SideEffect,
    pub idempotent: bool,
    pub result: Value,
}

pub async fn call(
    store: &ObjectStore,
    target: &ProjectionTarget,
    bus_address: &str,
    plugin_id: &str,
    method_name: &str,
    arguments: Value,
    confirm_side_effects: bool,
) -> Result<CallReceipt> {
    let object = match target {
        ProjectionTarget::View(id) => {
            let view = store.read_view(id)?;
            store.object_for_view(&view, plugin_id)?
        }
        ProjectionTarget::Workspace(id) => {
            let workspace = store.read_workspace(id)?;
            store.workspace_object(&workspace, plugin_id)?
        }
    };
    let method =
        object.schema.methods.get(method_name).with_context(|| {
            format!("method '{plugin_id}.{method_name}' is not schema-declared")
        })?;

    jsonschema::Validator::new(&method.args)
        .with_context(|| format!("compile argument schema for '{plugin_id}.{method_name}'"))?
        .validate(&arguments)
        .map_err(|error| anyhow::anyhow!("arguments failed schema validation: {error}"))?;

    if method.side_effect != SideEffect::Read && !confirm_side_effects {
        bail!(
            "'{plugin_id}.{method_name}' is a {:?} operation; pass --confirm-side-effects",
            method.side_effect
        );
    }

    let connection = zbus::connection::Builder::address(bus_address)?
        .build()
        .await
        .with_context(|| format!("connect to D-Bus at {bus_address}"))?;
    let proxy = Proxy::new(
        &connection,
        "org.opdbus.v1.plugins",
        format!("/org/opdbus/v1/plugins/{plugin_id}"),
        "org.opdbus.v1.PluginV1",
    )
    .await
    .with_context(|| format!("create plugin proxy for '{plugin_id}'"))?;
    let json_args = serde_json::to_string(&arguments)?;
    let response: String = proxy
        .call("Call", &(method_name.to_string(), json_args))
        .await
        .with_context(|| format!("dispatch '{plugin_id}.{method_name}'"))?;
    let result = serde_json::from_str(&response).unwrap_or(Value::String(response));

    Ok(CallReceipt {
        plugin_id: plugin_id.to_string(),
        method: method_name.to_string(),
        subid: method.subid.clone(),
        required_capability: method.required_capability.clone(),
        side_effect: method.side_effect,
        idempotent: method.idempotent,
        result,
    })
}
