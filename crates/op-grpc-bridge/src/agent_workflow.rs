//! Authenticated execution of the cognitive orchestration catalog.
//!
//! Workflow execution is deliberately kept in the bridge.  An orchestration
//! request may select steps, but every selected step is still a normal,
//! schema-backed cognitive method call carrying the original verified
//! identity.  This prevents a workflow from becoming an unaudited side door.

use anyhow::{Context, Result, anyhow, bail};
use serde::Serialize;
use serde_json::{Value, json};

use crate::mutation_engine::MutationEngine;
use crate::specialist::CognitiveCaller;
use op_agents::unified::GLOBAL_REGISTRY;
use op_plugins::state_plugins::cognitive_agents;

const COGNITIVE_PLUGIN: &str = "cognitive_mcp";

#[derive(Debug, Clone, Serialize)]
struct StepResult {
    index: usize,
    agent_type: String,
    operation: String,
    success: bool,
    result: Value,
}

/// Execute an authenticated workflow.  Empty workflows are rejected, nested
/// orchestration targets are rejected, and each child is dispatched through
/// the canonical MutationEngine path with the caller's exact identity.
pub async fn dispatch(
    engine: &MutationEngine,
    caller: CognitiveCaller<'_>,
    workflow_agent: &str,
    steps: Option<&[Value]>,
    context: Option<&str>,
) -> Result<Value> {
    let selected = if let Some(steps) = steps {
        steps.to_vec()
    } else {
        default_steps(workflow_agent)?
    };
    if selected.is_empty() {
        bail!("workflow '{workflow_agent}' must contain at least one step");
    }

    validate_steps(workflow_agent, &selected)?;
    validate_grants(caller.actor_id, &selected)?;
    let mut results = Vec::with_capacity(selected.len());
    let mut prior_result: Option<Value> = None;
    for (index, step) in selected.iter().enumerate() {
        let agent_type = step
            .get("agent_type")
            .and_then(Value::as_str)
            .context("workflow step requires agent_type")?;
        let operation = step
            .get("operation")
            .and_then(Value::as_str)
            .context("workflow step requires operation")?;
        let mut arguments = step.get("arguments").cloned().unwrap_or_else(|| json!({}));
        let method = cognitive_agents::catalog_agent_method(agent_type, operation)
            .ok_or_else(|| anyhow!("unsupported workflow target {agent_type}.{operation}"))?;
        let capability = method
            .declaration
            .required_capability
            .as_deref()
            .context("workflow target has no required capability")?;
        normalize_arguments(&mut arguments, &method.declaration.args)?;
        inject_context(
            &mut arguments,
            &method.declaration.args,
            context,
            prior_result.as_ref(),
        )?;
        validate_arguments(&method.declaration.args, &arguments)?;
        let method_name = cognitive_agents::agent_method_name(agent_type, operation);
        let serialized = serde_json::to_string(&arguments)?;
        let envelope = engine
            .dispatch_method_call_with_identity(
                COGNITIVE_PLUGIN,
                &method_name,
                &serialized,
                Some(capability),
                caller.actor_id,
                caller.session_id,
                caller.session_genesis,
            )
            .await?;
        let result = envelope.get("result").cloned().unwrap_or(envelope);
        let condition = step.get("condition").and_then(Value::as_str);
        let child_success = child_success(&method.category, &result)?;
        let expected = match condition {
            Some("expect_success") => Some(true),
            Some("expect_failure") => Some(false),
            Some(other) => bail!("unsupported workflow condition '{other}'"),
            None => None,
        };
        if let Some(expected) = expected {
            if child_success != expected {
                bail!("workflow step {agent_type}.{operation} violated {condition:?}");
            }
        } else if !child_success {
            bail!("workflow step {agent_type}.{operation} failed");
        }
        prior_result = Some(result.clone());
        results.push(StepResult {
            index,
            agent_type: agent_type.to_string(),
            operation: operation.to_string(),
            success: expected.map_or(child_success, |_| true),
            result,
        });
    }

    Ok(json!({
        "ok": true,
        "workflow_agent": workflow_agent,
        "steps_completed": results.len(),
        "results": results,
        "context": context,
    }))
}

/// Return the fixed catalog steps for an orchestration agent.
pub fn list_steps(workflow_agent: &str) -> Result<Value> {
    let agent = GLOBAL_REGISTRY
        .get_normalized(workflow_agent)
        .ok_or_else(|| anyhow!("unknown orchestration agent '{workflow_agent}'"))?;
    if agent.category() != op_agents::unified::AgentCategory::Orchestration {
        bail!("'{workflow_agent}' is not an orchestration agent");
    }
    let steps = agent
        .workflow_steps()
        .into_iter()
        .map(|step| {
            let method = cognitive_agents::catalog_agent_method(&step.agent_type, &step.operation);
            json!({
                "name": step.name,
                "agent_type": step.agent_type,
                "operation": step.operation,
                "condition": step.condition,
                "arguments": step.args_template,
                "method": method.map(|m| cognitive_agents::agent_method_name(&m.agent_type, &m.operation)),
                "required_capability": method.and_then(|m| m.declaration.required_capability.clone()),
            })
        })
        .collect::<Vec<_>>();
    Ok(json!({ "workflow_agent": workflow_agent, "steps": steps }))
}

/// Validate either explicit steps or the fixed default workflow without
/// executing any child operation.
pub fn validate(workflow_agent: &str, steps: Option<&[Value]>) -> Result<Value> {
    let selected = if let Some(steps) = steps {
        steps.to_vec()
    } else {
        default_steps(workflow_agent)?
    };
    let mut errors = Vec::new();
    if selected.is_empty() {
        errors.push(
            json!({"code":"empty_workflow","message":"workflow must contain at least one step"}),
        );
    }
    for (index, step) in selected.iter().enumerate() {
        if let Err(error) = validate_step(workflow_agent, step) {
            errors.push(json!({"index":index,"code":"invalid_step","message":error.to_string()}));
        }
    }
    Ok(json!({
        "valid": errors.is_empty(),
        "workflow_agent": workflow_agent,
        "errors": errors,
        "steps": selected,
    }))
}

fn default_steps(workflow_agent: &str) -> Result<Vec<Value>> {
    let agent = GLOBAL_REGISTRY
        .get_normalized(workflow_agent)
        .ok_or_else(|| anyhow!("unknown orchestration agent '{workflow_agent}'"))?;
    if agent.category() != op_agents::unified::AgentCategory::Orchestration {
        bail!("'{workflow_agent}' is not an orchestration agent");
    }
    Ok(agent
        .workflow_steps()
        .into_iter()
        .map(|step| {
            json!({ "agent_type": step.agent_type, "operation": step.operation, "arguments": step.args_template, "condition": step.condition })
        })
        .collect())
}

fn validate_steps(workflow_agent: &str, steps: &[Value]) -> Result<()> {
    for step in steps {
        validate_step(workflow_agent, step)?;
    }
    Ok(())
}

fn validate_grants(actor_id: &str, steps: &[Value]) -> Result<()> {
    let grants = crate::interceptor::load_capability_grants(actor_id);
    for step in steps {
        let agent_type = step
            .get("agent_type")
            .and_then(Value::as_str)
            .context("workflow step requires agent_type")?;
        let operation = step
            .get("operation")
            .and_then(Value::as_str)
            .context("workflow step requires operation")?;
        let method = cognitive_agents::catalog_agent_method(agent_type, operation)
            .ok_or_else(|| anyhow!("unsupported workflow target {agent_type}.{operation}"))?;
        let capability = method
            .declaration
            .required_capability
            .as_deref()
            .context("workflow target has no required capability")?;
        if !grants.contains(capability) {
            bail!("caller lacks child capability '{capability}'");
        }
    }
    Ok(())
}

fn validate_step(workflow_agent: &str, step: &Value) -> Result<()> {
    let agent_type = step
        .get("agent_type")
        .and_then(Value::as_str)
        .context("workflow step requires agent_type")?;
    let operation = step
        .get("operation")
        .and_then(Value::as_str)
        .context("workflow step requires operation")?;
    let target = cognitive_agents::catalog_agent_method(agent_type, operation)
        .ok_or_else(|| anyhow!("unsupported workflow target {agent_type}.{operation}"))?;
    if target.category == "orchestration" || agent_type == workflow_agent {
        bail!("nested orchestration is not permitted in workflow steps");
    }
    if let Some(condition) = step.get("condition").and_then(Value::as_str)
        && !matches!(condition, "expect_success" | "expect_failure")
    {
        bail!("unsupported workflow condition '{condition}'");
    }
    let mut arguments = step.get("arguments").cloned().unwrap_or_else(|| json!({}));
    normalize_arguments(&mut arguments, &target.declaration.args)?;
    validate_arguments(&target.declaration.args, &arguments)?;
    let owner = GLOBAL_REGISTRY
        .get_normalized(workflow_agent)
        .ok_or_else(|| anyhow!("unknown orchestration agent '{workflow_agent}'"))?;
    if !owner
        .delegated_agents()
        .iter()
        .any(|allowed| allowed == agent_type || allowed.replace('_', "-") == agent_type)
    {
        bail!("workflow agent '{workflow_agent}' does not allow '{agent_type}'");
    }
    Ok(())
}

fn normalize_arguments(arguments: &mut Value, schema: &impl serde::Serialize) -> Result<()> {
    let schema = serde_json::to_value(schema)?;
    let properties = schema.get("properties").and_then(Value::as_object);
    if let (Some(object), Some(properties)) = (arguments.as_object_mut(), properties) {
        // Older fixed workflow templates used `query`; public agent methods
        // use `question`. Translate only when the target schema says so.
        if !object.contains_key("question")
            && object.contains_key("query")
            && properties.contains_key("question")
            && let Some(query) = object.remove("query")
        {
            object.insert("question".into(), query);
        }
    }
    Ok(())
}

fn validate_arguments(schema: &impl serde::Serialize, arguments: &Value) -> Result<()> {
    let schema = serde_json::to_value(schema)?;
    jsonschema::validate(&schema, arguments)
        .map_err(|error| anyhow!("workflow child arguments violate schema: {error}"))
}

pub(crate) fn child_success(category: &str, result: &Value) -> Result<bool> {
    let exit_code = result.get("exit_code").and_then(Value::as_i64);
    if category == "execution" {
        return Ok(exit_code.context("execution workflow result omitted exit_code")? == 0);
    }
    Ok(exit_code
        .map(|code| code == 0)
        .or_else(|| result.get("success").and_then(Value::as_bool))
        .unwrap_or(true))
}

fn inject_context(
    arguments: &mut Value,
    schema: &impl serde::Serialize,
    workflow_context: Option<&str>,
    prior_result: Option<&Value>,
) -> Result<()> {
    let schema = serde_json::to_value(schema)?;
    let accepts_context = schema
        .get("properties")
        .and_then(Value::as_object)
        .is_some_and(|properties| properties.contains_key("context"));
    if !accepts_context {
        return Ok(());
    }
    let object = arguments
        .as_object_mut()
        .context("workflow child arguments must be an object")?;
    if !object.contains_key("context") {
        let mut context = workflow_context.unwrap_or_default().to_string();
        if let Some(previous) = prior_result {
            if !context.is_empty() {
                context.push('\n');
            }
            context.push_str("previous_step_result: ");
            context.push_str(&serde_json::to_string(previous)?);
        }
        if !context.is_empty() {
            object.insert("context".into(), Value::String(context));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{child_success, validate};
    use serde_json::json;

    #[test]
    fn execution_nonzero_is_failure_after_envelope_unwrap() {
        let envelope = json!({"success":true,"result":{"exit_code":7,"stderr":"failed"}});
        let domain = envelope.get("result").unwrap();
        assert!(!child_success("execution", domain).unwrap());
    }

    #[test]
    fn execution_requires_exit_code() {
        assert!(child_success("execution", &json!({"stdout":"ok"})).is_err());
    }

    #[test]
    fn non_execution_success_field_is_supported() {
        assert!(child_success("orchestration", &json!({"success":true})).unwrap());
        assert!(!child_success("orchestration", &json!({"success":false})).unwrap());
    }

    #[test]
    fn empty_workflow_is_invalid() {
        let steps = Vec::new();
        let report = validate("code-review-orchestrator", Some(&steps)).unwrap();
        assert_eq!(report["valid"], false);
        assert_eq!(report["errors"][0]["code"], "empty_workflow");
    }

    #[test]
    fn nested_workflow_target_is_rejected() {
        let steps = vec![json!({
            "agent_type": "code-review-orchestrator",
            "operation": "run_workflow",
            "arguments": {"steps": []}
        })];
        let error =
            validate("code-review-orchestrator", Some(&steps)).unwrap()["errors"][0]["message"]
                .as_str()
                .unwrap()
                .to_string();
        assert!(error.contains("nested orchestration"), "{error}");
    }

    #[test]
    fn forbidden_delegated_target_is_rejected() {
        let steps = vec![json!({
            "agent_type": "shell-executor",
            "operation": "exec",
            "arguments": {"command": "true", "cwd": ".", "timeout": 1}
        })];
        let error =
            validate("code-review-orchestrator", Some(&steps)).unwrap()["errors"][0]["message"]
                .as_str()
                .unwrap()
                .to_string();
        assert!(error.contains("does not allow"), "{error}");
    }

    #[test]
    fn valid_child_arguments_are_accepted() {
        let steps = vec![json!({
            "agent_type": "python-executor",
            "operation": "lint",
            "arguments": {"path": ".", "fix": false}
        })];
        let report = validate("code-review-orchestrator", Some(&steps)).unwrap();
        assert_eq!(report["valid"], true);
        assert!(report["errors"].as_array().unwrap().is_empty());
    }

    #[test]
    fn python_run_requires_nonempty_code() {
        let steps = vec![json!({
            "agent_type": "python-executor",
            "operation": "run",
            "arguments": {}
        })];
        let error =
            validate("code-review-orchestrator", Some(&steps)).unwrap()["errors"][0]["message"]
                .as_str()
                .unwrap()
                .to_string();
        assert!(error.contains("violate schema"), "{error}");
    }
}
