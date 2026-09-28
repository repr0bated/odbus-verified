use std::path::Path;

use anyhow::{bail, Context, Result};
use serde::Serialize;
use serde_json::Value;
use zbus::Proxy;

use crate::model::{MethodDecl, SideEffect};
use crate::namespace::ProjectionTarget;
use crate::store::ObjectStore;

pub const DEFAULT_BUS_ADDRESS: &str = "unix:path=/run/opdbus/session-bus.sock";

const PLUGIN_BUS_NAME: &str = "org.opdbus.v1.plugins";
const PLUGIN_INTERFACE: &str = "org.opdbus.v1.PluginV1";
/// The bridge's D-Bus identity bootstrap. A sender that registers a SID1
/// here is admitted as that identity for every later call on the same
/// connection, subject to the identity's own grants.
const IDENTITY_OBJECT_PATH: &str = "/org/opdbus/v1/identity/dbus";
const IDENTITY_INTERFACE: &str = "org.opdbus.v1.DbusIdentityV1";
/// Mirrors the bridge's registration bound; larger files are not SID1.
const MAX_CREDENTIAL_BYTES: u64 = 16 * 1024;
const LUNGS_NEGOTIATION_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

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

/// Why a lungs call did not complete. Each stage maps to one errno so the
/// writer learns what failed from `close()`, and the full text goes to the
/// method's `error` file.
#[derive(Debug, thiserror::Error)]
pub enum LungsError {
    #[error("arguments are not a JSON value: {0}")]
    Json(String),
    #[error("arguments failed schema validation: {0}")]
    Arguments(String),
    #[error("no credential has been delivered to this identity: {0}")]
    Credential(String),
    #[error("identity registration refused: {0}")]
    Registration(String),
    #[error("call refused: {0}")]
    Refused(String),
    #[error("dispatch failed: {0}")]
    Transport(String),
}

impl LungsError {
    pub fn stage(&self) -> &'static str {
        match self {
            Self::Json(_) => "parse",
            Self::Arguments(_) => "validate",
            Self::Credential(_) => "credential",
            Self::Registration(_) => "register",
            Self::Refused(_) => "call",
            Self::Transport(_) => "transport",
        }
    }

    pub fn errno(&self) -> i32 {
        match self {
            Self::Json(_) | Self::Arguments(_) => libc::EINVAL,
            Self::Credential(_) | Self::Registration(_) | Self::Refused(_) => libc::EACCES,
            Self::Transport(_) => libc::EIO,
        }
    }
}

pub fn method_decl(
    store: &ObjectStore,
    target: &ProjectionTarget,
    plugin_id: &str,
    method_name: &str,
) -> Result<MethodDecl> {
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
    object
        .schema
        .methods
        .get(method_name)
        .cloned()
        .with_context(|| format!("method '{plugin_id}.{method_name}' is not schema-declared"))
}

pub fn validate_arguments(method: &MethodDecl, arguments: &Value) -> Result<(), LungsError> {
    jsonschema::Validator::new(&method.args)
        .map_err(|error| LungsError::Arguments(format!("sealed argument schema: {error}")))?
        .validate(arguments)
        .map_err(|error| LungsError::Arguments(error.to_string()))
}

/// Read the identity's SID1 from its own device. It is read per call, so a
/// delivered or refreshed credential takes effect without a remount.
pub fn read_credential(path: &Path) -> Result<Vec<u8>, LungsError> {
    let metadata = std::fs::metadata(path)
        .map_err(|error| LungsError::Credential(format!("{}: {error}", path.display())))?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_CREDENTIAL_BYTES {
        return Err(LungsError::Credential(format!(
            "{} is not a SID1 credential file",
            path.display()
        )));
    }
    let bytes = std::fs::read(path)
        .map_err(|error| LungsError::Credential(format!("{}: {error}", path.display())))?;
    if !bytes.starts_with(b"SID1") {
        return Err(LungsError::Credential(format!(
            "{} does not hold a SID1 envelope",
            path.display()
        )));
    }
    Ok(bytes)
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
    let method = method_decl(store, target, plugin_id, method_name)?;
    validate_arguments(&method, &arguments)?;

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
    let result = plugin_call(&connection, plugin_id, method_name, &arguments)
        .await
        .map_err(|error| anyhow::anyhow!("dispatch '{plugin_id}.{method_name}': {error}"))?;
    Ok(receipt(plugin_id, method_name, &method, result))
}

/// Dispatch as the identity that owns the lungs mount. Registration and the
/// call share one connection, so the bridge admits the call as that identity
/// and runs it through MutationEngine under the identity's own grants.
pub async fn call_as_identity(
    bus_address: &str,
    plugin_id: &str,
    method_name: &str,
    method: &MethodDecl,
    arguments: Value,
    credential: Vec<u8>,
) -> Result<CallReceipt, LungsError> {
    let connection = zbus::connection::Builder::address(bus_address)
        .map_err(|error| LungsError::Transport(format!("bus address {bus_address}: {error}")))?
        .build()
        .await
        .map_err(|error| LungsError::Transport(format!("connect to {bus_address}: {error}")))?;
    let bootstrap = Proxy::new(
        &connection,
        PLUGIN_BUS_NAME,
        IDENTITY_OBJECT_PATH,
        IDENTITY_INTERFACE,
    )
    .await
    .map_err(|error| LungsError::Transport(format!("identity bootstrap proxy: {error}")))?;
    let registered: bool = bootstrap
        .call("RegisterCurrentSender", &(credential,))
        .await
        .map_err(|error| match refusal(&error) {
            true => LungsError::Registration(error.to_string()),
            false => LungsError::Transport(format!("register: {error}")),
        })?;
    if !registered {
        return Err(LungsError::Registration(
            "bridge declined the credential".into(),
        ));
    }
    let result = plugin_call(&connection, plugin_id, method_name, &arguments)
        .await
        .map_err(|error| match refusal(&error) {
            true => LungsError::Refused(error.to_string()),
            false => LungsError::Transport(format!("call: {error}")),
        })?;
    Ok(receipt(plugin_id, method_name, method, result))
}

/// Dispatch over the identity's own lungs socket. The bridge binds that
/// socket to one session, so no credential crosses it: holding the path is
/// the proof, and the call runs through MutationEngine as that identity.
pub async fn call_over_socket(
    socket: &Path,
    plugin_id: &str,
    method_name: &str,
    method: &MethodDecl,
    arguments: Value,
) -> Result<CallReceipt, LungsError> {
    let stream = tokio::net::UnixStream::connect(socket)
        .await
        .map_err(|error| {
            LungsError::Transport(format!("connect to {}: {error}", socket.display()))
        })?;
    let connection = tokio::time::timeout(
        LUNGS_NEGOTIATION_TIMEOUT,
        zbus::connection::Builder::unix_stream(stream)
            .p2p()
            .auth_mechanism(zbus::connection::AuthMechanism::Anonymous)
            .build(),
    )
    .await
    .map_err(|_| LungsError::Transport("lungs socket negotiation timed out".into()))?
    .map_err(|error| LungsError::Transport(format!("lungs socket: {error}")))?;
    let result = plugin_call(&connection, plugin_id, method_name, &arguments)
        .await
        .map_err(|error| match refusal(&error) {
            true => LungsError::Refused(error.to_string()),
            false => LungsError::Transport(format!("call: {error}")),
        })?;
    Ok(receipt(plugin_id, method_name, method, result))
}

async fn plugin_call(
    connection: &zbus::Connection,
    plugin_id: &str,
    method_name: &str,
    arguments: &Value,
) -> zbus::Result<Value> {
    let proxy = Proxy::new(
        connection,
        PLUGIN_BUS_NAME,
        format!("/org/opdbus/v1/plugins/{plugin_id}"),
        PLUGIN_INTERFACE,
    )
    .await?;
    let json_args = serde_json::to_string(arguments)
        .map_err(|error| zbus::Error::Failure(format!("serialize arguments: {error}")))?;
    let response: String = proxy
        .call("Call", &(method_name.to_string(), json_args))
        .await?;
    Ok(serde_json::from_str(&response).unwrap_or(Value::String(response)))
}

fn refusal(error: &zbus::Error) -> bool {
    matches!(error, zbus::Error::MethodError(name, _, _)
        if name.as_str() == "org.freedesktop.DBus.Error.AccessDenied")
        || matches!(error, zbus::Error::FDO(fdo) if matches!(**fdo, zbus::fdo::Error::AccessDenied(_)))
}

fn receipt(plugin_id: &str, method_name: &str, method: &MethodDecl, result: Value) -> CallReceipt {
    CallReceipt {
        plugin_id: plugin_id.to_string(),
        method: method_name.to_string(),
        subid: method.subid.clone(),
        required_capability: method.required_capability.clone(),
        side_effect: method.side_effect,
        idempotent: method.idempotent,
        result,
    }
}
