//! Compatibility LLM surface backed exclusively by the canonical tched_router.
//!
//! This plugin owns only its facade method schema. Provider/model discovery,
//! selection, configuration, inference, and committed state remain owned by
//! tched_router.

use anyhow::Result;
use async_trait::async_trait;
use op_state::{ApplyResult, Checkpoint, DiffMetadata, PluginCapabilities, StateDiff, StatePlugin};
use op_state_store::{CapabilityDecl, PluginSchema, SideEffect};
use serde::{Deserialize, Serialize};
use simd_json::OwnedValue as Value;

use super::large_language_model_methods::{EmptyInput, GetConfigOutput, GetProviderOutput};
use super::tched_router::{
    ChatInput, ChatOutput, ListModelsInput, ListModelsOutput, ListProvidersOutput,
    SetProviderInput, SetProviderOutput, SetSelectionInput, SetSelectionOutput,
};

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("x-oscal-subid" = "sch.software.plugin.large-language-model.schema@v3"))]
#[schemars(extend("x-oscal-category" = "llm"))]
pub struct LargeLanguageModelState {}

pub struct LargeLanguageModelPlugin;

impl Default for LargeLanguageModelPlugin {
    fn default() -> Self {
        Self
    }
}

impl LargeLanguageModelPlugin {
    pub fn new() -> Self {
        Self
    }
    fn state() -> LargeLanguageModelState {
        LargeLanguageModelState {}
    }
}

#[async_trait]
impl StatePlugin for LargeLanguageModelPlugin {
    fn name(&self) -> &str {
        "large_language_model"
    }
    fn version(&self) -> &str {
        "3.0.0"
    }
    fn schema(&self) -> Option<PluginSchema> {
        let mut schema = large_language_model_schema();
        super::common::oscal::ensure_category_metadata_fields(&mut schema);
        Some(schema)
    }
    async fn calculate_diff(&self, _current: &Value, _desired: &Value) -> Result<StateDiff> {
        Ok(StateDiff {
            plugin: self.name().to_string(),
            actions: vec![],
            metadata: DiffMetadata {
                timestamp: chrono::Utc::now().timestamp(),
                current_hash: "facade".into(),
                desired_hash: "facade".into(),
            },
        })
    }
    async fn apply_state(&self, diff: &StateDiff) -> Result<ApplyResult> {
        anyhow::ensure!(
            diff.actions.is_empty(),
            "large_language_model has no independent state mutation surface"
        );
        Ok(ApplyResult {
            success: true,
            changes_applied: vec![],
            errors: vec![],
            checkpoint: None,
        })
    }
    async fn verify_state(&self, desired: &Value) -> Result<bool> {
        Ok(desired == &simd_json::serde::to_owned_value(Self::state())?)
    }
    async fn create_checkpoint(&self) -> Result<Checkpoint> {
        Ok(Checkpoint {
            id: uuid::Uuid::new_v4().to_string(),
            plugin: self.name().to_string(),
            timestamp: chrono::Utc::now().timestamp(),
            state_snapshot: simd_json::serde::to_owned_value(Self::state())?,
            backend_checkpoint: None,
        })
    }
    async fn rollback(&self, _checkpoint: &Checkpoint) -> Result<()> {
        anyhow::bail!("large_language_model has no independent rollback surface")
    }
    fn capabilities(&self) -> PluginCapabilities {
        PluginCapabilities {
            supports_rollback: false,
            supports_checkpoints: true,
            supports_verification: true,
            atomic_operations: false,
        }
    }
}

pub(crate) fn add_forwarded_method<Input: schemars::JsonSchema, Output: schemars::JsonSchema>(
    schema: &mut PluginSchema,
    target_schema: &PluginSchema,
    facade_name: &str,
    target_name: &str,
    subid: &str,
) {
    let target = target_schema
        .methods
        .get(target_name)
        .unwrap_or_else(|| panic!("tched_router must declare {target_name}"));
    let capability = target_schema
        .resolve_method_capability(target)
        .capability()
        .to_string();
    let capability_decl = target_schema
        .capabilities
        .get(&capability)
        .unwrap_or_else(|| panic!("tched_router must declare capability {capability}"))
        .clone();
    let mut declaration =
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<Input, Output>(
            facade_name,
            target.side_effect.clone(),
            target.idempotent,
            &capability,
            subid,
        );
    // Some reused upstream input types predate deny_unknown_fields. The facade
    // rejects undeclared keys before producing a target plan, and its sealed
    // method schema must describe the same admission rule.
    let mut args: serde_json::Value = serde_json::from_str(
        &simd_json::to_string(&declaration.args).expect("method arguments serialize"),
    )
    .expect("method arguments are JSON");
    args.as_object_mut()
        .expect("method arguments schema is an object")
        .insert(
            "additionalProperties".to_string(),
            serde_json::Value::Bool(false),
        );
    declaration.args = simd_json::serde::to_owned_value(args).expect("method arguments convert");
    schema.methods.insert(facade_name.to_string(), declaration);
    schema.capabilities.insert(capability, capability_decl);
}

pub(crate) fn large_language_model_schema() -> PluginSchema {
    let root = serde_json::to_value(schemars::schema_for!(LargeLanguageModelState))
        .expect("schema serializes");
    let mut schema = super::schemars_adapter::plugin_schema_from_json(
        "large_language_model", "3.0.0",
        "Compatibility LLM method surface; tched_router exclusively owns provider, model, configuration, inference, and state", &root);
    let state = simd_json::serde::to_owned_value(LargeLanguageModelPlugin::state())
        .expect("state serializes");
    super::schemars_adapter::apply_state_defaults(&mut schema, &state);
    schema.example = Some(state);
    schema.dependencies = vec!["tched_router".to_string()];
    let target = super::tched_router::tched_router_schema();

    add_forwarded_method::<EmptyInput, ListProvidersOutput>(
        &mut schema,
        &target,
        "list_providers",
        "ListProviders",
        "obs.service.llm.provider.list@v1",
    );
    add_forwarded_method::<EmptyInput, GetProviderOutput>(
        &mut schema,
        &target,
        "get_provider",
        "GetState",
        "obs.service.llm.provider.get@v1",
    );
    add_forwarded_method::<SetProviderInput, SetProviderOutput>(
        &mut schema,
        &target,
        "set_provider",
        "SetProvider",
        "mut.service.llm.provider.set@v1",
    );
    add_forwarded_method::<ChatInput, ChatOutput>(
        &mut schema,
        &target,
        "generate",
        "Chat",
        "mut.service.llm.generate@v1",
    );
    add_forwarded_method::<ListModelsInput, ListModelsOutput>(
        &mut schema,
        &target,
        "list_models",
        "ListModels",
        "obs.service.llm.model.list@v1",
    );
    add_forwarded_method::<EmptyInput, GetConfigOutput>(
        &mut schema,
        &target,
        "get_config",
        "GetState",
        "obs.service.llm.config.get@v1",
    );
    add_forwarded_method::<SetSelectionInput, SetSelectionOutput>(
        &mut schema,
        &target,
        "update_config",
        "SetSelection",
        "mut.service.llm.config.update@v1",
    );

    // The facade starts one canonical private tched_router generation job.
    // Existing StateSync subscriptions read committed job children.
    schema.methods.insert(
        "stream_generate".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            super::tched_router::generation::StartGenerationInput,
            super::tched_router::generation::GenerationMutationOutput,
        >(
            "stream_generate",
            SideEffect::Mutation,
            false,
            "llm.invoke",
            "mut.service.llm.stream@v1",
        ),
    );
    schema.capabilities.insert(
        "llm.invoke".to_string(),
        CapabilityDecl {
            id: "llm.invoke".to_string(),
            description:
                "Invoke the generation facade; the exact nested tched_router start grant is also required."
                    .to_string(),
        },
    );
    schema
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forwarded_capabilities_are_derived_from_target_methods() {
        let facade = large_language_model_schema();
        let target = super::super::tched_router::tched_router_schema();
        for (facade_name, target_name) in [
            ("list_providers", "ListProviders"),
            ("get_provider", "GetState"),
            ("set_provider", "SetProvider"),
            ("generate", "Chat"),
            ("list_models", "ListModels"),
            ("get_config", "GetState"),
            ("update_config", "SetSelection"),
        ] {
            let expected = target
                .resolve_method_capability(&target.methods[target_name])
                .capability()
                .to_string();
            assert_eq!(
                facade.methods[facade_name].required_capability.as_deref(),
                Some(expected.as_str())
            );
            assert!(facade.capabilities.contains_key(&expected));
            assert_eq!(
                facade.methods[facade_name].args["additionalProperties"],
                simd_json::json!(false)
            );
        }
    }

    #[test]
    fn facade_has_no_provider_or_model_state_authority() {
        let schema = large_language_model_schema();
        assert_eq!(schema.version, "3.0.0");
        assert!(schema.fields.is_empty());
        assert_eq!(schema.dependencies, vec!["tched_router"]);
    }

    #[test]
    fn stream_resolves_to_the_canonical_private_job_owner() {
        assert!(
            super::super::large_language_model_methods::target_method_for("stream_generate")
                == Some("start_generation")
        );
    }
}

inventory::submit! {
    crate::default_registry::PluginReg::new("large_language_model", |_ctx| std::sync::Arc::new(LargeLanguageModelPlugin::new()))
}
