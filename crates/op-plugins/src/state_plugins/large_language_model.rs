//! Streaming generation facade backed exclusively by the canonical tched_router.
//!
//! Only stream_generate remains: its llm.invoke admission and pinned generation
//! origin protect canonical private jobs. Provider/model discovery, selection,
//! configuration and synchronous inference use tched_router directly.

use anyhow::Result;
use async_trait::async_trait;
use op_state::{ApplyResult, Checkpoint, DiffMetadata, PluginCapabilities, StateDiff, StatePlugin};
use op_state_store::{CapabilityDecl, PluginSchema, SideEffect};
use serde::{Deserialize, Serialize};
use simd_json::OwnedValue as Value;

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

pub(crate) fn large_language_model_schema() -> PluginSchema {
    let root = serde_json::to_value(schemars::schema_for!(LargeLanguageModelState))
        .expect("schema serializes");
    let mut schema = super::schemars_adapter::plugin_schema_from_json(
        "large_language_model",
        "3.0.0",
        "Streaming generation facade; tched_router exclusively owns provider, model, configuration, inference, and state",
        &root,
    );
    let state = simd_json::serde::to_owned_value(LargeLanguageModelPlugin::state())
        .expect("state serializes");
    super::schemars_adapter::apply_state_defaults(&mut schema, &state);
    schema.example = Some(state);
    schema.dependencies = vec!["tched_router".to_string()];
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
    fn facade_has_no_provider_or_model_state_authority() {
        let schema = large_language_model_schema();
        assert!(schema.fields.is_empty());
    }
}

inventory::submit! {
    crate::default_registry::PluginReg::new("large_language_model", |_ctx| std::sync::Arc::new(LargeLanguageModelPlugin::new()))
}
