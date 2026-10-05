use std::io::Read;
use std::path::Path;

use anyhow::{Context, Result, bail};
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
/// The bridge's lungs bootstrap: a connection binds to one session here first.
const LUNGS_OBJECT_PATH: &str = "/org/opdbus/v1/lungs";
const LUNGS_INTERFACE: &str = "org.opdbus.v1.LungsV1";
const LUNGS_NEGOTIATION_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
const METHOD_RESPONSE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

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
    #[error("operation outcome is unknown; do not retry automatically: {0}")]
    OutcomeUnknown(String),
    #[error("operation contract unavailable: {0}")]
    Contract(String),
}

impl LungsError {
    /// Correlation is retained even when the reply is lost. It never selects
    /// an identity or grants permission to the declared recovery operation.
    pub fn operation_id(&self) -> Option<&str> {
        let Self::OutcomeUnknown(message) = self else {
            return None;
        };
        let id = message
            .split_once("operation_id=")?
            .1
            .split(|character: char| character == ';' || character.is_whitespace())
            .next()?;
        uuid::Uuid::parse_str(id).ok()?;
        Some(id)
    }

    pub fn stage(&self) -> &'static str {
        match self {
            Self::Json(_) => "parse",
            Self::Arguments(_) => "validate",
            Self::Credential(_) => "credential",
            Self::Registration(_) => "register",
            Self::Refused(_) => "call",
            Self::Transport(_) => "transport",
            Self::OutcomeUnknown(_) => "outcome_unknown",
            Self::Contract(_) => "contract",
        }
    }

    pub fn errno(&self) -> i32 {
        match self {
            Self::Json(_) | Self::Arguments(_) => libc::EINVAL,
            Self::Credential(_) | Self::Registration(_) | Self::Refused(_) => libc::EACCES,
            Self::Transport(_) | Self::OutcomeUnknown(_) => libc::EIO,
            Self::Contract(_) => libc::EPROTO,
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
    if method.returns.is_none() {
        return Err(LungsError::Contract("missing_result_contract".into()));
    }
    op_state_store::method_receipt::validate_json_size(
        arguments,
        op_state_store::method_receipt::MAX_METHOD_ARGUMENT_BYTES,
    )
    .map_err(|error| LungsError::Arguments(error.to_string()))?;
    jsonschema::Validator::new(&method.args)
        .map_err(|error| LungsError::Arguments(format!("sealed argument schema: {error}")))?
        .validate(arguments)
        .map_err(|error| LungsError::Arguments(error.to_string()))
}

/// Read the identity's SID1 from its own device. It is read per call, so a
/// delivered or refreshed credential takes effect without a remount.
pub fn read_credential(path: &Path) -> Result<Vec<u8>, LungsError> {
    let file = std::fs::File::open(path)
        .map_err(|error| LungsError::Credential(format!("{}: {error}", path.display())))?;
    let metadata = file
        .metadata()
        .map_err(|error| LungsError::Credential(format!("{}: {error}", path.display())))?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_CREDENTIAL_BYTES {
        return Err(LungsError::Credential(format!(
            "{} is not a SID1 credential file",
            path.display()
        )));
    }
    let mut bytes = Vec::new();
    file.take(MAX_CREDENTIAL_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| LungsError::Credential(format!("{}: {error}", path.display())))?;
    if bytes.is_empty() || bytes.len() as u64 > MAX_CREDENTIAL_BYTES {
        return Err(LungsError::Credential(
            "credential exceeds SID1 byte bound".into(),
        ));
    }
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
    let operation_id = uuid::Uuid::new_v4().to_string();

    if method.side_effect != SideEffect::Read && !confirm_side_effects {
        bail!(
            "'{plugin_id}.{method_name}' is a {:?} operation; pass --confirm-side-effects",
            method.side_effect
        );
    }

    let connection = connect_bus(bus_address).await?;
    let result = plugin_call(
        &connection,
        plugin_id,
        method_name,
        &arguments,
        &operation_id,
    )
    .await
    .map_err(|error| anyhow::anyhow!("dispatch '{plugin_id}.{method_name}': {error}"))?;
    Ok(receipt(
        plugin_id,
        method_name,
        &method,
        result,
        &operation_id,
    )?)
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
    validate_arguments(method, &arguments)?;
    let operation_id = uuid::Uuid::new_v4().to_string();
    let connection = connect_bus(bus_address).await?;
    let bootstrap = tokio::time::timeout(
        LUNGS_NEGOTIATION_TIMEOUT,
        Proxy::new(
            &connection,
            PLUGIN_BUS_NAME,
            IDENTITY_OBJECT_PATH,
            IDENTITY_INTERFACE,
        ),
    )
    .await
    .map_err(|_| LungsError::Transport("identity bootstrap proxy timed out".into()))?
    .map_err(|error| LungsError::Transport(format!("identity bootstrap proxy: {error}")))?;
    let registered: bool = tokio::time::timeout(
        LUNGS_NEGOTIATION_TIMEOUT,
        bootstrap.call("RegisterCurrentSender", &(credential,)),
    )
    .await
    .map_err(|_| LungsError::Transport("identity registration timed out".into()))?
    .map_err(|error| match refusal(&error) {
        true => LungsError::Registration(error.to_string()),
        false => LungsError::Transport(format!("register: {error}")),
    })?;
    if !registered {
        return Err(LungsError::Registration(
            "bridge declined the credential".into(),
        ));
    }
    let result = plugin_call(
        &connection,
        plugin_id,
        method_name,
        &arguments,
        &operation_id,
    )
    .await?;
    receipt(plugin_id, method_name, method, result, &operation_id)
}

/// Dispatch over the bridge's shared lungs socket. The connection first binds
/// to this identity's session, so no credential crosses it. An empty session
/// calls the no-argument container bootstrap; the bridge derives identity
/// from its kernel peer and native Incus binding. A named session uses the
/// host-root-only bootstrap. Both run through the identity's own grants.
pub async fn call_over_socket(
    socket: &Path,
    session: &str,
    plugin_id: &str,
    method_name: &str,
    method: &MethodDecl,
    arguments: Value,
) -> Result<CallReceipt, LungsError> {
    validate_arguments(method, &arguments)?;
    let operation_id = uuid::Uuid::new_v4().to_string();
    let stream = tokio::time::timeout(
        LUNGS_NEGOTIATION_TIMEOUT,
        tokio::net::UnixStream::connect(socket),
    )
    .await
    .map_err(|_| LungsError::Transport("lungs socket connection timed out".into()))?
    .map_err(|error| LungsError::Transport(format!("connect to {}: {error}", socket.display())))?;
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
    let lungs = tokio::time::timeout(
        LUNGS_NEGOTIATION_TIMEOUT,
        Proxy::new(
            &connection,
            PLUGIN_BUS_NAME,
            LUNGS_OBJECT_PATH,
            LUNGS_INTERFACE,
        ),
    )
    .await
    .map_err(|_| LungsError::Transport("lungs bind proxy timed out".into()))?
    .map_err(|error| LungsError::Transport(format!("lungs bind proxy: {error}")))?;
    let binding = tokio::time::timeout(LUNGS_NEGOTIATION_TIMEOUT, async {
        if session.is_empty() {
            lungs.call("BindCurrentContainer", &()).await
        } else {
            lungs.call("Bind", &(session,)).await
        }
    })
    .await
    .map_err(|_| LungsError::Transport("lungs identity bind timed out".into()))?;
    let bound: bool = binding.map_err(|error| match refusal(&error) {
        true => LungsError::Registration(error.to_string()),
        false => LungsError::Transport(format!("bind: {error}")),
    })?;
    if !bound {
        return Err(LungsError::Registration(
            "bridge declined to bind this session".into(),
        ));
    }
    let result = plugin_call(
        &connection,
        plugin_id,
        method_name,
        &arguments,
        &operation_id,
    )
    .await?;
    receipt(plugin_id, method_name, method, result, &operation_id)
}

async fn connect_bus(bus_address: &str) -> Result<zbus::Connection, LungsError> {
    let builder = zbus::connection::Builder::address(bus_address)
        .map_err(|error| LungsError::Transport(format!("bus address {bus_address}: {error}")))?;
    tokio::time::timeout(LUNGS_NEGOTIATION_TIMEOUT, builder.build())
        .await
        .map_err(|_| LungsError::Transport("D-Bus connection timed out".into()))?
        .map_err(|error| LungsError::Transport(format!("connect to {bus_address}: {error}")))
}

async fn plugin_call(
    connection: &zbus::Connection,
    plugin_id: &str,
    method_name: &str,
    arguments: &Value,
    operation_id: &str,
) -> Result<Value, LungsError> {
    let proxy = tokio::time::timeout(
        LUNGS_NEGOTIATION_TIMEOUT,
        Proxy::new(
            connection,
            PLUGIN_BUS_NAME,
            format!("/org/opdbus/v1/plugins/{plugin_id}"),
            PLUGIN_INTERFACE,
        ),
    )
    .await
    .map_err(|_| LungsError::Transport("plugin proxy timed out".into()))?
    .map_err(|error| LungsError::Transport(format!("plugin proxy: {error}")))?;
    let json_args = serde_json::to_string(arguments)
        .map_err(|error| LungsError::Arguments(format!("serialize arguments: {error}")))?;
    let response: String = tokio::time::timeout(
        METHOD_RESPONSE_TIMEOUT,
        proxy.call(
            "CallWithOperationId",
            &(method_name.to_string(), json_args, operation_id.to_owned()),
        ),
    )
    .await
    .map_err(|_| {
        LungsError::OutcomeUnknown(format!(
            "operation_id={operation_id}; method reply timed out; admitted work may continue"
        ))
    })?
    .map_err(|error| {
        if refusal(&error) {
            LungsError::Refused(error.to_string())
        } else {
            LungsError::OutcomeUnknown(format!(
                "operation_id={operation_id}; method reply unavailable: {error}"
            ))
        }
    })?;
    if response.len() > op_state_store::method_receipt::MAX_METHOD_RECEIPT_BYTES {
        return Err(LungsError::OutcomeUnknown(format!(
            "operation_id={operation_id}; method reply exceeds 16 MiB"
        )));
    }
    serde_json::from_str(&response).map_err(|error| {
        LungsError::OutcomeUnknown(format!(
            "operation_id={operation_id}; invalid method reply JSON: {error}"
        ))
    })
}

fn refusal(error: &zbus::Error) -> bool {
    matches!(error, zbus::Error::MethodError(name, _, _)
        if name.as_str() == "org.freedesktop.DBus.Error.AccessDenied")
        || matches!(error, zbus::Error::FDO(fdo) if matches!(**fdo, zbus::fdo::Error::AccessDenied(_)))
}

pub fn validate_engine_receipt(
    plugin_id: &str,
    method_name: &str,
    method: &MethodDecl,
    value: &Value,
) -> Result<(), LungsError> {
    let check = || -> anyhow::Result<()> {
        op_state_store::method_receipt::validate_json_size(
            value,
            op_state_store::method_receipt::MAX_METHOD_RECEIPT_BYTES,
        )?;
        let receipt: op_state_store::method_receipt::MethodExecutionReceipt =
            serde_json::from_value(value.clone())?;
        receipt.validate_attribution(plugin_id, method_name)?;
        op_state_store::method_receipt::validate_method_result(
            method.returns.as_ref(),
            &receipt.result,
        )
    };
    check().map_err(|error| {
        LungsError::OutcomeUnknown(format!("unverifiable method receipt: {error}"))
    })
}

fn receipt(
    plugin_id: &str,
    method_name: &str,
    method: &MethodDecl,
    result: Value,
    operation_id: &str,
) -> Result<CallReceipt, LungsError> {
    validate_engine_receipt(plugin_id, method_name, method, &result).map_err(|error| {
        LungsError::OutcomeUnknown(format!("operation_id={operation_id}; {error}"))
    })?;
    if result["operation_id"] != operation_id {
        return Err(LungsError::OutcomeUnknown(format!(
            "operation_id={operation_id}; reply operation id mismatch"
        )));
    }
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
