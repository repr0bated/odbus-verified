use anyhow::{Context, Result, anyhow};
use serde_json::Value;

pub const TARGET_PLUGIN_ID: &str = "chat_router";

#[derive(Debug, Clone, PartialEq)]
pub struct TargetPlan {
    pub plugin_id: &'static str,
    pub method: &'static str,
    pub required_capability: String,
    pub args: Value,
}

/// Resolve the target capability from the owning schema. No capability string
/// is copied into this compatibility module.
pub fn target_capability(target_method: &str) -> Result<String> {
    let schema = super::threetched::sealed_schema("chat_router")?;
    let declaration = schema.methods.get(target_method).ok_or_else(|| {
        anyhow!("chat_router schema does not declare target method '{target_method}'")
    })?;
    Ok(schema
        .resolve_method_capability(declaration)
        .capability()
        .to_string())
}

fn decode<T: serde::de::DeserializeOwned>(args: &Value, method: &str) -> Result<T> {
    serde_json::from_value(args.clone())
        .with_context(|| format!("invalid large_language_model.{method} arguments"))
}

/// Validate a compatibility call and identify the exact canonical chat_router
/// operation and capability it requires.
///
/// The caller MUST run target-method admission against the same pinned
/// chat_router PluginSchema and verified identity before invoking this plan.
/// Possession of the outer large_language_model capability is not sufficient.
pub fn plan_method(method: &str, args: &Value) -> Result<TargetPlan> {
    let (target_method, target_args) = match method {
        "stream_generate" => {
            let input: super::chat_router::generation::StartGenerationInput =
                decode(args, method)?;
            input.validate()?;
            ("start_generation", serde_json::to_value(input)?)
        }
        other => return Err(anyhow!("unknown large_language_model method: {other}")),
    };
    Ok(TargetPlan {
        plugin_id: TARGET_PLUGIN_ID,
        method: target_method,
        required_capability: target_capability(target_method)?,
        args: target_args,
    })
}

/// Validate and shape the canonical target result for the compatibility caller.
/// This function has no state or side effects.
pub fn complete_method(method: &str, target_result: Value) -> Result<Value> {
    match method {
        "stream_generate" => Ok(serde_json::to_value(
            serde_json::from_value::<super::chat_router::generation::GenerationMutationOutput>(
                target_result,
            )
            .context("chat_router generation returned an invalid receipt")?,
        )?),
        other => Err(anyhow!("unknown large_language_model method: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stream_generate_rejects_missing_required_input() {
        assert!(plan_method("stream_generate", &serde_json::json!({})).is_err());
    }

    #[test]
    fn malformed_target_results_fail_closed() {
        assert!(complete_method("stream_generate", serde_json::json!({"accepted":true})).is_err());
    }
}
