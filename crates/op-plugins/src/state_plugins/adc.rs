use anyhow::Result;
use async_trait::async_trait;
use op_state::{ApplyResult, Checkpoint, DiffMetadata, PluginCapabilities, StateDiff, StatePlugin};
use op_state_store::PluginSchema;
use serde::{Deserialize, Serialize};
use simd_json::OwnedValue as Value;
use simd_json::prelude::*;

const ADC_PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

/// ADC plugin state schema.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("x-oscal-subid" = "sch.software.plugin.adc.schema@v2"))]
#[schemars(extend("x-oscal-category" = "software"))]
pub struct AdcState {
    /// Whether ADC is configured.
    #[schemars(
        description = "Whether ADC is configured",
        extend("x-oscal-subid" = "exp.software.plugin.adc.configured.render@v1")
    )]
    pub configured: bool,
    /// Whether Google has validated the configured credential.
    pub authenticated: bool,
    /// Current provider validation state.
    pub validation: super::gcloud_adc::AdcValidationState,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetStatusInput {}

pub struct AdcPlugin;

impl Default for AdcPlugin {
    fn default() -> Self {
        Self
    }
}

impl AdcPlugin {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl StatePlugin for AdcPlugin {
    fn name(&self) -> &str {
        "adc"
    }

    fn version(&self) -> &str {
        "2.0.0"
    }

    fn schema(&self) -> Option<op_state_store::PluginSchema> {
        Some(adc_schema())
    }

    async fn calculate_diff(&self, _current: &Value, _desired: &Value) -> Result<StateDiff> {
        Ok(StateDiff {
            plugin: self.name().to_string(),
            actions: vec![],
            metadata: DiffMetadata {
                timestamp: chrono::Utc::now().timestamp(),
                current_hash: "unknown".to_string(),
                desired_hash: "unknown".to_string(),
            },
        })
    }

    async fn apply_state(&self, _diff: &StateDiff) -> Result<ApplyResult> {
        Ok(ApplyResult {
            success: true,
            changes_applied: vec![],
            errors: vec![],
            checkpoint: None,
        })
    }

    async fn verify_state(&self, _desired: &Value) -> Result<bool> {
        Ok(true)
    }

    async fn create_checkpoint(&self) -> Result<Checkpoint> {
        Ok(Checkpoint {
            id: uuid::Uuid::new_v4().to_string(),
            plugin: self.name().to_string(),
            timestamp: chrono::Utc::now().timestamp(),
            state_snapshot: Value::null(),
            backend_checkpoint: None,
        })
    }

    async fn rollback(&self, _checkpoint: &Checkpoint) -> Result<()> {
        Ok(())
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

/// Canonical `adc` schema, derived from the structs via schemars.
pub fn adc_schema() -> PluginSchema {
    let root = serde_json::to_value(schemars::schema_for!(AdcState))
        .expect("schemars schema serializes to JSON");
    let mut schema = super::schemars_adapter::plugin_schema_from_json(
        "adc",
        "2.0.0",
        "Application default credentials provider validation state",
        &root,
    );

    use super::plugin_scaffold_helpers::method_decl_from_schemars_with_output;
    use op_state_store::SideEffect;

    schema.methods.insert(
        "get_status".to_string(),
        method_decl_from_schemars_with_output::<GetStatusInput, super::gcloud_adc::AdcAuthStatus>(
            "get_status",
            SideEffect::Read,
            true,
            "adc.read",
            "obs.software.plugin.adc.status.get@v2",
        ),
    );

    schema.capabilities.insert(
        "adc.read".to_string(),
        op_state_store::CapabilityDecl {
            id: "adc.read".to_string(),
            description: "Grants: get_status.".to_string(),
        },
    );

    schema
}

pub(crate) async fn dispatch_adc_with_provider(
    provider: &dyn super::gcloud_adc::AdcStatusProvider,
    method: &str,
    args: &serde_json::Value,
) -> Result<serde_json::Value> {
    dispatch_adc_with_provider_timeout(provider, method, args, ADC_PROBE_TIMEOUT).await
}

async fn dispatch_adc_with_provider_timeout(
    provider: &dyn super::gcloud_adc::AdcStatusProvider,
    method: &str,
    args: &serde_json::Value,
    timeout: std::time::Duration,
) -> Result<serde_json::Value> {
    match method {
        "get_status" => {
            serde_json::from_value::<GetStatusInput>(args.clone())?;
            let status = match tokio::time::timeout(timeout, provider.check_auth_status()).await {
                Ok(result) => result?,
                Err(_) => super::gcloud_adc::provider_timeout_status(),
            };
            Ok(serde_json::to_value(status)?)
        }
        other => Err(anyhow::anyhow!("unknown adc method: {other}")),
    }
}

/// Compatibility ADC surface backed by the same official Google provider as
/// `gcloud_adc`; credential material remains inside that adapter.
pub async fn dispatch_adc_method(
    method: &str,
    args: &serde_json::Value,
) -> Result<serde_json::Value> {
    dispatch_adc_with_provider(&super::gcloud_adc::GoogleCloudAuthProvider, method, args).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state_plugins::common::oscal::validate_subid;

    struct FakeProvider(std::result::Result<super::super::gcloud_adc::AdcAuthStatus, &'static str>);

    #[async_trait]
    impl super::super::gcloud_adc::AdcStatusProvider for FakeProvider {
        async fn check_auth_status(&self) -> Result<super::super::gcloud_adc::AdcAuthStatus> {
            self.0.clone().map_err(anyhow::Error::msg)
        }
    }

    fn validated() -> super::super::gcloud_adc::AdcAuthStatus {
        super::super::gcloud_adc::AdcAuthStatus {
            provider: "google_application_default_credentials".to_string(),
            configured: true,
            authenticated: true,
            validation: super::super::gcloud_adc::AdcValidationState::Validated,
            failure: None,
        }
    }

    #[tokio::test]
    async fn compatibility_surface_returns_the_provider_validated_contract() {
        let value = dispatch_adc_with_provider(
            &FakeProvider(Ok(validated())),
            "get_status",
            &serde_json::json!({}),
        )
        .await
        .unwrap();
        assert_eq!(value["configured"], true);
        assert_eq!(value["authenticated"], true);
        assert_eq!(value["validation"], "validated");
    }

    #[tokio::test]
    async fn compatibility_surface_rejects_input_and_propagates_provider_errors() {
        let provider = FakeProvider(Err("provider adapter failed"));
        let error = dispatch_adc_with_provider(
            &provider,
            "get_status",
            &serde_json::json!({"unexpected": true}),
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("unknown field"));

        let error = dispatch_adc_with_provider(&provider, "get_status", &serde_json::json!({}))
            .await
            .unwrap_err();
        assert_eq!(error.to_string(), "provider adapter failed");
    }

    struct PendingProvider;

    #[async_trait]
    impl super::super::gcloud_adc::AdcStatusProvider for PendingProvider {
        async fn check_auth_status(&self) -> Result<super::super::gcloud_adc::AdcAuthStatus> {
            std::future::pending().await
        }
    }

    #[tokio::test]
    async fn compatibility_surface_has_a_bounded_provider_deadline() {
        let value = dispatch_adc_with_provider_timeout(
            &PendingProvider,
            "get_status",
            &serde_json::json!({}),
            std::time::Duration::from_millis(1),
        )
        .await
        .unwrap();
        assert_eq!(value["validation"], "provider_unavailable");
        assert_eq!(value["authenticated"], false);
    }

    #[test]
    fn method_schema_has_typed_input_and_output() {
        let method = &adc_schema().methods["get_status"];
        assert_eq!(method.args["type"], "object");
        assert_eq!(method.returns.as_ref().unwrap()["type"], "object");
    }

    #[test]
    fn all_subids_are_valid() {
        let raw = serde_json::to_value(schemars::schema_for!(AdcState)).unwrap();
        let mut stack = vec![&raw];
        while let Some(node) = stack.pop() {
            if let Some(subid) = node.get("x-oscal-subid").and_then(|v| v.as_str()) {
                validate_subid(subid).expect("invalid subid");
            }
            if let Some(props) = node.get("properties").and_then(|v| v.as_object()) {
                for (_, v) in props {
                    stack.push(v);
                }
            }
            if let Some(defs) = node
                .get("$defs")
                .or_else(|| node.get("definitions"))
                .and_then(|v| v.as_object())
            {
                for (_, v) in defs {
                    stack.push(v);
                }
            }
        }
    }
}

// Self-registration: the plugin registry discovers this via inventory
// (single source of the catalog; no central dispatch list).
inventory::submit! {
    crate::default_registry::PluginReg::new("adc", |_ctx| std::sync::Arc::new(AdcPlugin::new()))
}
