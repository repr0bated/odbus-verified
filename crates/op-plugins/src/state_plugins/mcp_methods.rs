//! Authenticated compatibility facade for the native tched_router MCP section.

use anyhow::{Context, Result, anyhow, ensure};
use serde_json::Value;

use crate::state_plugins::tched_router::{
    ConfigPatchInput, ConfigPatchOperation, ConfigPatchOutput,
};
use crate::state_plugins::tched_router::{EmptyTchedInput, GetMcpConfigOutput};

pub struct McpMethodPlan {
    pub target_method: &'static str,
    pub required_capability: String,
    pub args: Value,
}

fn target_capability(method: &str) -> Result<String> {
    let schema = crate::state_plugins::tched_router::tched_router_schema();
    let declaration = schema
        .methods
        .get(method)
        .ok_or_else(|| anyhow!("tched_router does not declare '{method}'"))?;
    Ok(schema
        .resolve_method_capability(declaration)
        .capability()
        .to_string())
}

fn valid_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
}

fn validate_server_path(path: &str) -> Result<()> {
    ensure!(
        path.starts_with('/'),
        "MCP server patch path must be a JSON pointer"
    );
    ensure!(path.len() <= 256, "MCP server patch path is too long");
    let segments: Vec<&str> = path[1..].split('/').collect();
    ensure!(
        segments.len() >= 4
            && segments[0] == "mcp"
            && segments[1] == "servers"
            && segments.iter().all(|segment| valid_segment(segment)),
        "MCP server patches must target /mcp/servers/<server> or a descendant"
    );
    Ok(())
}

fn patch_path(operation: &ConfigPatchOperation) -> &str {
    match operation {
        ConfigPatchOperation::Add { path, .. }
        | ConfigPatchOperation::Remove { path }
        | ConfigPatchOperation::Replace { path, .. }
        | ConfigPatchOperation::Test { path, .. } => path,
    }
}

pub fn plan_method(method: &str, args: &Value) -> Result<McpMethodPlan> {
    match method {
        "get_config" => {
            let _: EmptyTchedInput =
                serde_json::from_value(args.clone()).context("invalid mcp.get_config arguments")?;
            Ok(McpMethodPlan {
                target_method: "GetMcpConfig",
                required_capability: target_capability("GetMcpConfig")?,
                args: args.clone(),
            })
        }
        "configure_server" => {
            let input: ConfigPatchInput = serde_json::from_value(args.clone())
                .context("invalid mcp.configure_server arguments")?;
            ensure!(
                !input.operations.is_empty() && input.operations.len() <= 64,
                "mcp.configure_server requires 1..=64 operations"
            );
            for operation in &input.operations {
                validate_server_path(patch_path(operation))?;
            }
            Ok(McpMethodPlan {
                target_method: "PatchConfig",
                required_capability: target_capability("PatchConfig")?,
                args: args.clone(),
            })
        }
        "configure_tool_groups" => Err(anyhow!(
            "mcp.configure_tool_groups is owned by the bridge MCP projection policy"
        )),
        other => Err(anyhow!("unknown mcp method: {other}")),
    }
}

pub fn complete_method(method: &str, result: Value) -> Result<Value> {
    match method {
        "get_config" => Ok(serde_json::to_value(
            serde_json::from_value::<GetMcpConfigOutput>(result)
                .context("tched_router.GetMcpConfig returned an invalid result")?,
        )?),
        "configure_server" => Ok(serde_json::to_value(
            serde_json::from_value::<ConfigPatchOutput>(result)
                .context("tched_router.PatchConfig returned an invalid result")?,
        )?),
        "configure_tool_groups" => Err(anyhow!(
            "mcp.configure_tool_groups is completed by the bridge MCP projection policy"
        )),
        other => Err(anyhow!("unknown mcp method: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn patch(path: &str) -> Value {
        serde_json::json!({"operations": [{"op": "replace", "path": path, "value": "x"}]})
    }

    #[test]
    fn server_patch_targets_native_router_and_retains_args() {
        let args = patch("/mcp/servers/demo/command");
        let plan = plan_method("configure_server", &args).unwrap();
        assert_eq!(plan.target_method, "PatchConfig");
        assert_eq!(plan.args, args);
    }

    #[test]
    fn server_path_requires_exact_root_and_segments() {
        for path in [
            "/mcp/servers",
            "/mcp/servers/demo",
            "/fake/mcp/servers/demo/command",
            "/mcp/servers/demo/command/",
            "/mcp/servers//command",
            "/mcp/servers/demo/bad space",
            "/mcp/servers/demo/command.name",
        ] {
            assert!(
                plan_method("configure_server", &patch(path)).is_err(),
                "{path}"
            );
        }
    }

    #[test]
    fn server_property_is_allowed_and_unknown_fields_are_rejected() {
        assert!(plan_method("configure_server", &patch("/mcp/servers/demo/url")).is_ok());
        assert!(
            plan_method(
                "configure_server",
                &serde_json::json!({"operations": [], "extra": true})
            )
            .is_err()
        );
        assert!(plan_method(
            "configure_server",
            &serde_json::json!({"operations": [{"op": "replace", "path": "/mcp/servers_evil/demo", "value": "x"}]})
        )
        .is_err());
    }

    #[test]
    fn patch_count_is_bounded() {
        let args = serde_json::json!({"operations": []});
        assert!(plan_method("configure_server", &args).is_err());

        let operations = (0..65)
            .map(|_| serde_json::json!({"op": "remove", "path": "/mcp/servers/demo/url"}))
            .collect::<Vec<_>>();
        assert!(
            plan_method(
                "configure_server",
                &serde_json::json!({"operations": operations})
            )
            .is_err()
        );
    }
}
