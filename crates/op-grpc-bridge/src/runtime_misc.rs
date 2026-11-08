//! Small native owners which do not belong to identity, chat or networking.
//! Runtime alone journals effects and publishes their observations. Descriptor
//! reads do not assert mount readiness; absent owners never become empty state.
use anyhow::{Context, ensure};
use op_plugins::state_plugins::{chat_fs, ghostbridge};
use serde_json::Value;

use crate::{
    core::{BackendStateUpdate, StateScope},
    runtime::{BackendContext, BackendOutcome, RuntimeError},
};

#[path = "runtime_misc_cozo.rs"]
mod cozo;
#[path = "runtime_misc_freedesktop.rs"]
mod freedesktop;
#[path = "runtime_misc_snowball.rs"]
mod snowball;

pub(crate) fn supports(plugin: &str, method: &str) -> bool {
    match plugin {
        "cozo" => cozo::supports(method),
        "freedesktop" => matches!(method, "GetManagedObjects" | "Get" | "GetAll" | "Set" | "Introspect" | "Ping" | "GetMachineId"),
        "tched_fs" => matches!(method, "get_protocol" | "get_contract" | "get_receipt"),
        "ghostbridge" => matches!(method, "GetState" | "GetIdentity" | "ListEndpoints" | "GetGhostrunner"),
        "snowball" => snowball::supports(method),
        _ => false,
    }
}

pub(crate) async fn dispatch(
    ctx: &BackendContext<'_>, plugin: &str, method: &str, args: &Value,
) -> Result<BackendOutcome, RuntimeError> {
    if !supports(plugin, method) {
        return Err(RuntimeError::unsupported(format!("no native adapter for {plugin}.{method}")));
    }
    revalidate(ctx).await?;
    match plugin {
        "cozo" => cozo::dispatch(ctx, method, args).await,
        "freedesktop" => freedesktop::dispatch(ctx, method, args).await,
        "tched_fs" => filesystem(ctx, method, args).await,
        "ghostbridge" => ghostbridge(ctx, method).await,
        "snowball" => snowball::dispatch(ctx, method, args).await,
        _ => unreachable!("supported plugin checked above"),
    }
}

pub(super) async fn revalidate(ctx: &BackendContext<'_>) -> Result<(), RuntimeError> {
    ctx.caller.authority.revalidate().await
        .map_err(|_| RuntimeError::denied("identity authority is no longer valid"))
}

pub(super) fn private_result(
    ctx: &BackendContext<'_>, result: Value, state: BackendStateUpdate,
) -> BackendOutcome {
    BackendOutcome { result, state, scope: ctx.caller.private_scope(), additional: Vec::new(), guard: None }
}

async fn filesystem(
    ctx: &BackendContext<'_>, method: &str, args: &Value,
) -> Result<BackendOutcome, RuntimeError> {
    let result = if method == "get_receipt" {
        let input: op_state_store::method_receipt::GetMethodReceiptInput =
            serde_json::from_value(args.clone()).map_err(|error| RuntimeError::before_effect(error.into()))?;
        let runtime = ctx.runtime.clone();
        let caller = ctx.caller.clone();
        let receipt = tokio::task::spawn_blocking(move || {
            runtime.core().recover_operation(&input.operation_id, &caller.principal_id, &caller.session_id)
        }).await.map_err(|error| RuntimeError::backend(error.into()))?
            .map_err(RuntimeError::backend)?;
        serde_json::to_value(receipt)?
    } else {
        // These are the actual immutable descriptor methods. The sealed
        // tched_fs contract has no mount, unmount, read-file or proxy method.
        // The existing host/guest mount owners and Runtime lungs keep those
        // namespaces; this adapter neither selects nor creates a host mount.
        chat_fs::dispatch(method, args).map_err(RuntimeError::before_effect)?
    };
    revalidate(ctx).await?;
    // Only the public protocol is global. The recovery result stays in the
    // caller-scoped method journal, never in this public state document.
    Ok(BackendOutcome {
        result,
        state: BackendStateUpdate::Replace(serde_json::to_value(chat_fs::protocol_value()?)?),
        scope: StateScope::Public,
        additional: Vec::new(),
        guard: None,
    })
}

async fn ghostbridge(
    ctx: &BackendContext<'_>, method: &str,
) -> Result<BackendOutcome, RuntimeError> {
    let runtime = ctx.runtime.clone();
    let scope = ctx.caller.private_scope();
    let snapshot = tokio::task::spawn_blocking(move || {
        let path = op_plugins::canonical::plugin_path("ghostbridge");
        let own = runtime.core().committed_snapshot(&scope, "ghostbridge", &path)?;
        match own {
            Some(snapshot) => Ok::<_, anyhow::Error>(Some(snapshot)),
            None => runtime.core().committed_snapshot(&StateScope::Public, "ghostbridge", &path),
        }
    }).await.map_err(|error| RuntimeError::backend(error.into()))?
        .map_err(RuntimeError::backend)?
        .ok_or_else(|| RuntimeError::unsupported(
            "ghostbridge has no committed native observation; TLS/endpoint/Ghostrunner owner publication is required",
        ))?;
    // The retained schema exemplar is declared environment configuration,
    // not an observed TLS certificate, endpoint list or running UI. Do not use
    // GhostbridgePlugin::current_state or serde defaults to manufacture these.
    let observed = (|| -> anyhow::Result<ghostbridge::GhostbridgeState> {
        let object = snapshot.state.as_object().context("Ghostbridge observation is not an object")?;
        for field in ["status", "bridge_identity", "ghostrunner", "endpoints"] {
            ensure!(object.contains_key(field), "Ghostbridge native observation is incomplete");
        }
        ensure!(object["status"].as_str().is_some_and(|status| !status.is_empty() && status != "declared"),
            "Ghostbridge state contains declaration defaults rather than a native observation");
        for (field, members) in [
            ("bridge_identity", &["wireguard_pubkey", "tls_subject", "tls_sans", "tls_expires", "tls_self_signed"][..]),
            ("ghostrunner", &["port", "bind_addr", "path", "enabled"][..]),
        ] {
            let value = object[field].as_object().context("Ghostbridge native observation has invalid shape")?;
            ensure!(members.iter().all(|member| value.contains_key(*member)), "Ghostbridge native observation is incomplete");
        }
        let endpoints = object["endpoints"].as_array().context("Ghostbridge endpoints are invalid")?;
        for endpoint in endpoints {
            let endpoint = endpoint.as_object().context("Ghostbridge endpoint is invalid")?;
            ensure!(["id", "address", "status", "peer_pubkey", "uptime_secs"].iter().all(|field| endpoint.contains_key(*field)),
                "Ghostbridge native endpoint observation is incomplete");
        }
        Ok(serde_json::from_value(snapshot.state)?)
    })().map_err(RuntimeError::backend)?;
    revalidate(ctx).await?;
    let result = ghostbridge::dispatch_ghostbridge_method(method, &observed).map_err(RuntimeError::backend)?;
    // Read the existing authoritative observation without turning a read into
    // a new native observation, or copying a private document into public SHM.
    Ok(private_result(ctx, result, BackendStateUpdate::Unchanged))
}
