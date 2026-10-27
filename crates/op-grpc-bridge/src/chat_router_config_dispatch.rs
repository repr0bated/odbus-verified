//! Prior name: tched_config_dispatch.
//! Native read adapter for the chat_router v0.8.4 config surface.
//!
//! MutationEngine performs sealed admission and pins/rechecks the exact plugin
//! schema around this helper. This module only maps an already-admitted method
//! to the shared loopback runtime; it is not plugin-existence authority.

use anyhow::Context;
use op_plugins::state_plugins::chat_router::{
    ConfigGetInput, ConfigInitInput, ConfigListInput, ConfigMigrateInput, ConfigPatchInput,
    ConfigSetInput,
};
use serde_json::Value;

use crate::chat_router_runtime::{
    ConfigSecretPolicy, NativeConfigMutation, ChatRouterRuntimeClient,
};

/// Backing for the authored typed config methods. The generated per-section
/// getters retired; only the two load-bearing operations remain, routed by
/// the same names the plugin declares (`NATIVE_CONFIG_METHODS`).
pub enum ConfigMethodBacking {
    /// Masked `GET /api/config` from the pinned loopback gateway.
    WholeNativeConfig,
    /// Bounded typed property operations sent to native `PATCH /api/config`.
    NativePropertyPatch,
    /// The named native configuration section, masked and committed under
    /// the same seal pin (the MCP facade's narrow read).
    NativeSection(&'static str),
}

/// Resolve a canonical method name to its backing without reading state or
/// consulting the Rust registry for plugin existence.
pub fn config_method_backing(method: &str) -> Option<ConfigMethodBacking> {
    match method {
        "GetConfig" => Some(ConfigMethodBacking::WholeNativeConfig),
        "PatchConfig" => Some(ConfigMethodBacking::NativePropertyPatch),
        "GetMcpConfig" => Some(ConfigMethodBacking::NativeSection("mcp")),
        _ => None,
    }
}

pub(crate) enum ConfigReadDispatch {
    Handled(Value),
    /// MutationEngine must read, commit, and return from one masked native
    /// configuration image under the exact seal pin.
    NativeConfig {
        section: Option<&'static str>,
    },
    NotAConfigRead,
}

pub(crate) async fn dispatch_config_mutation(
    runtime: &ChatRouterRuntimeClient,
    method: &str,
    args: &Value,
    secrets: &dyn ConfigSecretPolicy,
) -> anyhow::Result<Option<NativeConfigMutation>> {
    let mutation = match method {
        "PatchConfig" | "config_patch" => {
            let input: ConfigPatchInput = serde_json::from_value(args.clone())
                .context("decode admitted config patch input")?;
            runtime.patch_config(&input, secrets).await?
        }
        "config_set" => {
            let input: ConfigSetInput =
                serde_json::from_value(args.clone()).context("decode admitted config_set input")?;
            runtime.set_config_property(&input, secrets).await?
        }
        "config_init" => {
            let input: ConfigInitInput = serde_json::from_value(args.clone())
                .context("decode admitted config_init input")?;
            runtime.init_config(&input, secrets).await?
        }
        "config_migrate" => {
            let input: ConfigMigrateInput = serde_json::from_value(args.clone())
                .context("decode admitted config_migrate input")?;
            runtime.migrate_config(&input, secrets).await?
        }
        _ => return Ok(None),
    };
    Ok(Some(mutation))
}

pub(crate) async fn dispatch_config_read(
    runtime: &ChatRouterRuntimeClient,
    method: &str,
    args: &Value,
    secrets: &dyn ConfigSecretPolicy,
) -> anyhow::Result<ConfigReadDispatch> {
    match method {
        "config_list" => {
            let input: ConfigListInput = serde_json::from_value(args.clone())
                .context("decode admitted config_list input")?;
            let value = runtime
                .read_config_list(input.prefix.as_deref(), secrets)
                .await?;
            Ok(ConfigReadDispatch::Handled(value))
        }
        "config_get" => {
            let input: ConfigGetInput =
                serde_json::from_value(args.clone()).context("decode admitted config_get input")?;
            let value = runtime.read_config_property(&input.path, secrets).await?;
            Ok(ConfigReadDispatch::Handled(value))
        }
        other => match config_method_backing(other) {
            Some(ConfigMethodBacking::WholeNativeConfig) => {
                Ok(ConfigReadDispatch::NativeConfig { section: None })
            }
            Some(ConfigMethodBacking::NativeSection(section)) => {
                Ok(ConfigReadDispatch::NativeConfig {
                    section: Some(section),
                })
            }
            // The generated per-section getters retired; anything else is
            // not a config read (PatchConfig is a mutation, handled later).
            _ => Ok(ConfigReadDispatch::NotAConfigRead),
        },
    }
}
