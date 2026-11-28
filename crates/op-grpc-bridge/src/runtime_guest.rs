//! Guest-local native effects enter Runtime and its execution tracker through
//! owned-container exec. No caller-selected container, bus or host fallback.
use anyhow::Context as _;
use op_plugins::state_plugins::{identity_sled::ExecuteInOwnedContainerOutput, packagekit::PackageKitResult};
use op_state_store::SideEffect;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    core::BackendStateUpdate,
    runtime::{BackendContext, BackendOutcome, RuntimeError, subsidiary::Scope},
};

const RUNNER: &str = "/usr/local/libexec/op-dbus/op-guest-backend";

pub(crate) fn supports(plugin: &str) -> bool {
    matches!(plugin, "users" | "software" | "packagekit" | "rtnetlink" | "service" | "btrfs")
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GuestOutput {
    result: Value,
    observation: Value,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NetworkObservation {
    interfaces: Vec<op_network::rtnetlink::NetworkInterface>,
    default_route: Option<Value>,
}

pub(crate) async fn dispatch(
    ctx: &BackendContext<'_>, scope: &Scope, plugin: &str, method: &str, args: &Value,
) -> Result<BackendOutcome, RuntimeError> {
    if !supports(plugin) {
        return Err(RuntimeError::unsupported("guest native adapter is absent"));
    }
    let contract = serde_json::to_string(&json!({
        "version": ctx.schema.version, "declaration": ctx.declaration,
    })).map_err(|error| RuntimeError::before_effect(error.into()))?;
    let args = serde_json::to_string(args).map_err(|error| RuntimeError::before_effect(error.into()))?;
    let operation_id = uuid::Uuid::new_v4().to_string();
    let result = ctx.runtime.call_nested(ctx.caller, "identity_sled", "execute_in_owned_container", json!({
        "operation_id": operation_id,
        "session_id": ctx.caller.session_id,
        "command": [RUNNER, ctx.caller.session_id.as_str(), plugin, method, contract.as_str(), args.as_str()],
    })).await?;
    let output: ExecuteInOwnedContainerOutput = serde_json::from_value(result.result)
        .map_err(|error| RuntimeError::unknown(error.into()))?;
    if output.operation_id != operation_id || output.session_id != ctx.caller.session_id || output.native_uuid != scope.native_uuid()
        || output.stdout_truncated || output.stderr_truncated {
        return Err(RuntimeError::unknown(anyhow::anyhow!("guest result operation, identity, native UUID or output bound changed")));
    }
    if output.exit_code != 0 {
        // A native error may follow a partial effect. Its tracked exec receipt
        // retains diagnostics; never retry or fall back to a host provider.
        return Err(RuntimeError::unknown(anyhow::anyhow!("guest native operation failed; inspect its nested execution receipt")));
    }
    let output: GuestOutput = serde_json::from_str(&output.stdout).context("guest native result is not JSON")
        .map_err(RuntimeError::unknown)?;
    let private_scope = ctx.caller.private_scope();
    let state = match plugin {
        "software" | "packagekit" if ctx.declaration.side_effect == SideEffect::Mutation => {
            let inventory: PackageKitResult = serde_json::from_value(output.observation)
                .map_err(|error| RuntimeError::unknown(error.into()))?;
            crate::runtime_platform::package_state(ctx, plugin, &private_scope, inventory)
                .map_err(RuntimeError::unknown)?
        }
        "rtnetlink" => {
            let observed: NetworkObservation = serde_json::from_value(output.observation)
                .map_err(|error| RuntimeError::unknown(error.into()))?;
            crate::runtime_platform::rtnetlink_observation(ctx, &private_scope, observed.interfaces, observed.default_route)
                .map_err(RuntimeError::unknown)?
        }
        "service" => crate::runtime_platform::service_state(ctx, &private_scope, method, &output.result)
            .map_err(RuntimeError::unknown)?,
        _ => BackendStateUpdate::Unchanged,
    };
    Ok(BackendOutcome {
        result: output.result, state, scope: private_scope,
        additional: Vec::new(), guard: None,
    })
}
