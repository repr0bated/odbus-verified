//! Execution behind the sealed cognitive agent methods. The caller has passed
//! canonical method admission; the catalog selects an implementation, never
//! supplies authority or accepts a caller-chosen tool name.

use anyhow::{bail, Context, Result};
use op_plugins::state_plugins::cognitive_agents::{self, CatalogAgentMethod};
use serde_json::{json, Value};

use crate::{mutation_engine::MutationEngine, specialist::CognitiveCaller};

pub(crate) async fn dispatch(
    engine: &MutationEngine,
    caller: CognitiveCaller<'_>,
    method: &CatalogAgentMethod,
    arguments: &Value,
    admitted_schema: &op_state_store::PluginSchema,
    authority_method: &str,
) -> Result<Value> {
    crate::project_dispatch::verified_session(
        engine,
        caller.actor_id,
        caller.session_id,
        caller.session_genesis,
    )
    .await?;
    let required = admitted_schema
        .methods
        .get(authority_method)
        .and_then(|method| method.required_capability.as_deref())
        .context("agent invocation has no admitted capability")?;
    anyhow::ensure!(
        crate::interceptor::load_capability_grants(caller.actor_id).contains(required),
        "agent invocation requires its current exact capability grant"
    );
    let context =
        crate::memory_binding::context_for(engine, caller.actor_id, caller.session_id).await?;
    anyhow::ensure!(
        !context.read_only
            || method.category == "persona"
            || method.operation != "run_workflow" && method.category == "orchestration",
        "selected chat mode is read-only"
    );
    if context.mode == op_plugins::state_plugins::project::ChatMode::Project {
        anyhow::ensure!(
            context.allowed_agents.contains(&method.agent_type),
            "agent '{}' is not allowed in the selected project",
            method.agent_type
        );
        let capability = method
            .declaration
            .required_capability
            .as_deref()
            .context("catalog agent has no declared capability")?;
        anyhow::ensure!(
            context.effective_capabilities.contains(capability),
            "selected project does not grant {capability}"
        );
    }
    match method.category.as_str() {
        "persona" => {
            crate::specialist::consult_catalog_agent(
                engine,
                caller,
                &method.agent_type,
                &method.operation,
                arguments
                    .get("question")
                    .and_then(Value::as_str)
                    .context("question is required")?,
                arguments.get("path").and_then(Value::as_str),
                arguments.get("context").and_then(Value::as_str),
                admitted_schema,
                authority_method,
            )
            .await
        }
        "execution" => {
            let result = execute(caller, &method.agent_type, &method.operation, arguments).await;
            if context.mode.has_memory()
                && let Ok(runtime) = engine.cognitive_mcp().await
            {
                let successful = result
                    .as_ref()
                    .is_ok_and(|value| value["exit_code"].as_i64() == Some(0));
                let activity = match (successful, method.operation.as_str()) {
                    (false, "test") => {
                        op_cognitive_mcp::context_awareness::ActivityType::TestFailure
                    }
                    (false, _) => op_cognitive_mcp::context_awareness::ActivityType::BuildError,
                    (true, _) => op_cognitive_mcp::context_awareness::ActivityType::ToolCall,
                };
                let detail = match &result {
                    Ok(value) => value.to_string(),
                    Err(error) => error.to_string(),
                };
                runtime.context_engine().record_activity(
                        caller.scope(), activity,
                        detail.chars().take(4000).collect(),
                        json!({"tool":method.declaration.name,"success":successful,
                            "path":arguments.get("path"),"memory_namespace":context.memory_namespace}),
                    ).await;
            }
            result
        }
        "orchestration" => {
            let steps = arguments
                .get("steps")
                .and_then(Value::as_array)
                .map(Vec::as_slice);
            match method.operation.as_str() {
                "run_workflow" => {
                    crate::agent_workflow::dispatch(
                        engine,
                        caller,
                        &method.agent_type,
                        steps,
                        arguments.get("context").and_then(Value::as_str),
                    )
                    .await
                }
                "list_steps" => crate::agent_workflow::list_steps(&method.agent_type),
                "validate" => crate::agent_workflow::validate(&method.agent_type, steps),
                _ => bail!("unsupported catalog workflow operation"),
            }
        }
        _ => bail!("unsupported catalog agent category"),
    }
}

async fn execute(
    caller: CognitiveCaller<'_>,
    agent_type: &str,
    operation: &str,
    arguments: &Value,
) -> Result<Value> {
    // The sealed method already establishes existence and authority. Registry
    // metadata only resolves the implementation alias; execution stays on D-Bus.
    let implementation = op_agents::unified::GLOBAL_REGISTRY
        .get_normalized(agent_type)
        .context("admitted executor implementation is absent")?;
    let backend_agent_type = implementation.id();
    let request = json!({
        "agent_type": backend_agent_type,
        "operation": operation,
        "arguments": arguments,
        "actor_id": caller.actor_id,
        "session_id": caller.session_id.context("verified executor session is required")?,
        "session_genesis": caller.session_genesis.context("verified executor genesis is required")?,
    });
    let connection = zbus::connection::Builder::system()?
        .method_timeout(std::time::Duration::from_secs(3605))
        .build()
        .await
        .context("unified execution D-Bus connection unavailable")?;
    let proxy = zbus::Proxy::new(
        &connection,
        "org.dbusmcp.Agent.UnifiedExecution",
        "/org/dbusmcp/Agent/UnifiedExecution",
        "org.dbusmcp.Agent",
    )
    .await
    .context("unified execution D-Bus service unavailable")?;
    let encoded = serde_json::to_string(&request)?;
    let response: String = proxy
        .call("ExecuteCatalog", &(encoded,))
        .await
        .context("unified execution D-Bus call failed")?;
    let data: Value =
        serde_json::from_str(&response).context("unified execution returned invalid JSON")?;
    execution_result(agent_type, operation, &data)
}

fn execution_result(agent_type: &str, operation: &str, data: &Value) -> Result<Value> {
    let exit_code = data
        .get("exit_code")
        .and_then(Value::as_i64)
        .context("executor returned no process exit status")?;
    let stdout = data
        .get("stdout")
        .or_else(|| data.get("output"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let stderr = data
        .get("stderr")
        .or_else(|| data.get("errors"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let mut result = json!({
        "agent_type": agent_type,
        "operation": operation,
        "stdout": stdout,
        "stderr": stderr,
        "exit_code": exit_code,
    });
    if let Some(error) = data.get("error").and_then(Value::as_str) {
        result["error"] = Value::String(error.to_string());
    }
    Ok(result)
}

/// Normalize an existing HOT facade to its exact catalog method. HOT names
/// remain stable while the sealed per-operation contracts own the arguments.
pub(crate) fn hot_method(
    method: &str,
    arguments: &Value,
) -> Result<Option<(&'static CatalogAgentMethod, Value)>> {
    let agent = match method {
        "rust_pro" => "rust-pro",
        "python_pro" => "python-pro",
        "typescript_pro" => "typescript-pro",
        "sequential_thinking" => "sequential-thinking",
        "context_manager" => "context-manager",
        "code_review_orchestrator" => "code-review-orchestrator",
        _ => return Ok(None),
    };
    let operation = arguments
        .get("operation")
        .and_then(Value::as_str)
        .context("agent operation is required")?;
    let catalog = cognitive_agents::catalog_agent_method(agent, operation)
        .context("unsupported agent operation")?;
    let mut normalized = arguments
        .as_object()
        .context("agent arguments must be an object")?
        .clone();
    normalized.remove("operation");
    normalized.retain(|_, value| !value.is_null());
    if agent == "rust-pro" && !matches!(operation, "build" | "run") {
        anyhow::ensure!(
            normalized.get("release").and_then(Value::as_bool) != Some(true),
            "release is supported only for build and run"
        );
        normalized.remove("release");
    }
    Ok(Some((catalog, Value::Object(normalized))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn executor_status_and_diagnostics_are_preserved() {
        let result = execution_result(
            "python-pro",
            "lint",
            &json!({
                "output":"lint output", "errors":"bad input", "exit_code":2,
            }),
        )
        .unwrap();
        assert_eq!(result["exit_code"], 2);
        assert_eq!(result["stdout"], "lint output");
        assert_eq!(result["stderr"], "bad input");
        assert!(execution_result("python-pro", "lint", &json!({"success":true})).is_err());
    }
}
