use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::common::llm_projection::Provider;
use super::tched_router::{
    ChatInput, ChatOutput, GetStateOutput, ListModelsInput, ListModelsOutput, ListProvidersOutput,
    SetProviderInput, SetProviderOutput, SetSelectionInput, SetSelectionOutput,
};

pub const TARGET_PLUGIN_ID: &str = "tched_router";

#[derive(Debug, Clone, PartialEq)]
pub struct TargetPlan {
    pub plugin_id: &'static str,
    pub method: &'static str,
    pub required_capability: String,
    pub args: Value,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EmptyInput {}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetProviderOutput {
    pub selected_provider: String,
    pub provider: Provider,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SelectionConfig {
    pub selected_provider: String,
    pub selected_model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetConfigOutput {
    pub config: SelectionConfig,
}

pub fn target_method_for(method: &str) -> Option<&'static str> {
    match method {
        "list_providers" => Some("ListProviders"),
        "get_provider" | "get_config" => Some("GetState"),
        "set_provider" => Some("SetProvider"),
        "generate" => Some("Chat"),
        "stream_generate" => Some("start_generation"),
        "list_models" => Some("ListModels"),
        "update_config" => Some("SetSelection"),
        _ => None,
    }
}

/// Resolve the target capability from the owning schema. No capability string
/// is copied into this compatibility module.
pub fn target_capability(target_method: &str) -> Result<String> {
    let schema = super::tched_router::tched_router_schema();
    let declaration = schema.methods.get(target_method).ok_or_else(|| {
        anyhow!("tched_router schema does not declare target method '{target_method}'")
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

fn reject_extra_keys(args: &Value, allowed: &[&str], method: &str) -> Result<()> {
    let object = args
        .as_object()
        .ok_or_else(|| anyhow!("large_language_model.{method} arguments must be an object"))?;
    if let Some(key) = object.keys().find(|key| !allowed.contains(&key.as_str())) {
        return Err(anyhow!(
            "large_language_model.{method} does not accept field '{key}'"
        ));
    }
    Ok(())
}

/// Validate a compatibility call and identify the exact canonical tched_router
/// operation and capability it requires.
///
/// The caller MUST run target-method admission against the same pinned
/// tched_router PluginSchema and verified identity before invoking this plan.
/// Possession of the outer large_language_model capability is not sufficient.
pub fn plan_method(method: &str, args: &Value) -> Result<TargetPlan> {
    let (target_method, target_args) = match method {
        "list_providers" => {
            reject_extra_keys(args, &[], method)?;
            let _: EmptyInput = decode(args, method)?;
            ("ListProviders", serde_json::json!({}))
        }
        "get_provider" => {
            reject_extra_keys(args, &[], method)?;
            let _: EmptyInput = decode(args, method)?;
            ("GetState", serde_json::json!({}))
        }
        "set_provider" => {
            reject_extra_keys(args, &["provider_id"], method)?;
            let input: SetProviderInput = decode(args, method)?;
            ("SetProvider", serde_json::to_value(input)?)
        }
        "generate" => {
            reject_extra_keys(args, &["message", "messages", "provider", "model"], method)?;
            let input: ChatInput = decode(args, method)?;
            ("Chat", serde_json::to_value(input)?)
        }
        "list_models" => {
            reject_extra_keys(args, &["provider"], method)?;
            let input: ListModelsInput = decode(args, method)?;
            ("ListModels", serde_json::to_value(input)?)
        }
        "get_config" => {
            reject_extra_keys(args, &[], method)?;
            let _: EmptyInput = decode(args, method)?;
            ("GetState", serde_json::json!({}))
        }
        "update_config" => {
            reject_extra_keys(args, &["provider_id", "model_id"], method)?;
            let input: SetSelectionInput = decode(args, method)?;
            ("SetSelection", serde_json::to_value(input)?)
        }
        "stream_generate" => {
            let input: super::tched_router::generation::StartGenerationInput =
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
        "list_providers" => Ok(serde_json::to_value(
            serde_json::from_value::<ListProvidersOutput>(target_result)
                .context("tched_router.ListProviders returned an invalid result")?,
        )?),
        "get_provider" => {
            let state = serde_json::from_value::<GetStateOutput>(target_result)
                .context("tched_router.GetState returned an invalid result")?
                .state;
            let selected_provider = state.selected_provider;
            let mut matches = state
                .providers
                .into_iter()
                .filter(|provider| provider.id == selected_provider);
            let provider = matches.next().ok_or_else(|| {
                anyhow!(
                    "tched_router selected provider '{}' is absent from its observed catalog",
                    selected_provider
                )
            })?;
            if matches.next().is_some() {
                return Err(anyhow!(
                    "tched_router observed catalog contains duplicate provider id '{}'",
                    selected_provider
                ));
            }
            Ok(serde_json::to_value(GetProviderOutput {
                selected_provider,
                provider,
            })?)
        }
        "set_provider" => Ok(serde_json::to_value(
            serde_json::from_value::<SetProviderOutput>(target_result)
                .context("tched_router.SetProvider returned an invalid result")?,
        )?),
        "generate" => Ok(serde_json::to_value(
            serde_json::from_value::<ChatOutput>(target_result)
                .context("tched_router.Chat returned an invalid result")?,
        )?),
        "list_models" => Ok(serde_json::to_value(
            serde_json::from_value::<ListModelsOutput>(target_result)
                .context("tched_router.ListModels returned an invalid result")?,
        )?),
        "get_config" => {
            let state = serde_json::from_value::<GetStateOutput>(target_result)
                .context("tched_router.GetState returned an invalid result")?
                .state;
            Ok(serde_json::to_value(GetConfigOutput {
                config: SelectionConfig {
                    selected_provider: state.selected_provider,
                    selected_model: state.selected_model,
                },
            })?)
        }
        "update_config" => Ok(serde_json::to_value(
            serde_json::from_value::<SetSelectionOutput>(target_result)
                .context("tched_router.SetSelection returned an invalid result")?,
        )?),
        "stream_generate" => Ok(serde_json::to_value(
            serde_json::from_value::<super::tched_router::generation::GenerationMutationOutput>(
                target_result,
            )
            .context("tched_router generation returned an invalid receipt")?,
        )?),
        other => Err(anyhow!("unknown large_language_model method: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state_plugins::tched_router::TchedRouterPlugin;

    #[test]
    fn plans_exact_target_methods_and_nested_capabilities() {
        let cases = [
            ("list_providers", serde_json::json!({}), "ListProviders"),
            ("get_provider", serde_json::json!({}), "GetState"),
            (
                "set_provider",
                serde_json::json!({"provider_id":"opencode"}),
                "SetProvider",
            ),
            ("generate", serde_json::json!({"message":"hello"}), "Chat"),
            (
                "list_models",
                serde_json::json!({"provider":"opencode"}),
                "ListModels",
            ),
            ("get_config", serde_json::json!({}), "GetState"),
            (
                "update_config",
                serde_json::json!({"provider_id":"opencode","model_id":"model"}),
                "SetSelection",
            ),
        ];
        for (method, args, target) in cases {
            let plan = plan_method(method, &args).unwrap();
            assert_eq!(plan.plugin_id, "tched_router");
            assert_eq!(plan.method, target);
            assert_eq!(plan.required_capability, target_capability(target).unwrap());
        }
    }

    #[test]
    fn malformed_or_extra_inputs_never_produce_a_target_plan() {
        for (method, args) in [
            ("list_providers", serde_json::json!({"unexpected":true})),
            ("set_provider", serde_json::json!({})),
            (
                "set_provider",
                serde_json::json!({"provider_id":"x","extra":1}),
            ),
            ("generate", serde_json::json!({"messages":"not-an-array"})),
            ("update_config", serde_json::json!({"provider_id":"x"})),
        ] {
            assert!(
                plan_method(method, &args).is_err(),
                "{method} accepted {args}"
            );
        }
    }

    #[test]
    fn stream_generate_requires_real_typed_input_and_nested_start_grant() {
        assert!(plan_method("stream_generate", &serde_json::json!({})).is_err());
        let plan = plan_method(
            "stream_generate",
            &serde_json::json!({"agent_alias":"fixture", "prompt":"hello"}),
        )
        .unwrap();
        assert_eq!(plan.method, "start_generation");
        assert_eq!(
            plan.required_capability,
            super::super::tched_router::generation::START_CAPABILITY
        );
    }

    #[test]
    fn derived_reads_use_the_target_state_without_an_llm_state_store() {
        let state = TchedRouterPlugin::current_state();
        let selected = state.selected_provider.clone();
        let raw = serde_json::json!({"state": {
            "status": state.status,
            "selected_provider": state.selected_provider,
            "selected_model": state.selected_model,
            "providers": state.catalog.providers,
            "model_routes": state.catalog.model_routes,
        }});
        let provider = complete_method("get_provider", raw.clone()).unwrap();
        assert_eq!(provider["selected_provider"], selected);
        assert_eq!(provider["provider"]["id"], selected);

        let config = complete_method("get_config", raw).unwrap();
        assert_eq!(config["config"]["selected_provider"], selected);
        assert!(config["config"]["selected_model"].is_string());
    }

    #[test]
    fn malformed_target_results_fail_closed() {
        assert!(complete_method("list_providers", serde_json::json!({"providers":"bad"})).is_err());
        assert!(complete_method("get_provider", serde_json::json!({"state":{}})).is_err());
        assert!(complete_method("set_provider", serde_json::json!({"accepted":true})).is_err());
    }
}
