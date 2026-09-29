use anyhow::Result;
use async_trait::async_trait;
use op_state::{ApplyResult, Checkpoint, DiffMetadata, PluginCapabilities, StateDiff, StatePlugin};
use op_state_store::PluginSchema;
use serde::{Deserialize, Serialize};
use simd_json::prelude::*;
use simd_json::OwnedValue as Value;

const CLOUD_PLATFORM_SCOPE: &str = "https://www.googleapis.com/auth/cloud-platform";
const ADC_PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AdcValidationState {
    /// The SDK could not construct a usable ADC credential configuration.
    NotConfigured,
    ConfiguredUnverified,
    ProviderUnavailable,
    Rejected,
    Validated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AdcFailureKind {
    Configuration,
    ProviderUnavailable,
    ProviderRejected,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AdcProbeFailure {
    pub kind: AdcFailureKind,
    pub retryable: bool,
    pub message: String,
}

/// Provider-backed ADC status. Tokens and credential fields are intentionally
/// absent from this public contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AdcAuthStatus {
    pub provider: String,
    pub configured: bool,
    /// True only after Google issued an access token.
    pub authenticated: bool,
    pub validation: AdcValidationState,
    pub failure: Option<AdcProbeFailure>,
}

impl AdcAuthStatus {
    fn not_configured(failure: AdcProbeFailure) -> Self {
        Self {
            provider: "google_application_default_credentials".to_string(),
            configured: false,
            authenticated: false,
            validation: AdcValidationState::NotConfigured,
            failure: Some(failure),
        }
    }

    fn validation_failed(state: AdcValidationState, failure: AdcProbeFailure) -> Self {
        Self {
            provider: "google_application_default_credentials".to_string(),
            configured: true,
            authenticated: false,
            validation: state,
            failure: Some(failure),
        }
    }

    fn validated() -> Self {
        Self {
            provider: "google_application_default_credentials".to_string(),
            configured: true,
            authenticated: true,
            validation: AdcValidationState::Validated,
            failure: None,
        }
    }
}

pub(crate) fn provider_timeout_status() -> AdcAuthStatus {
    AdcAuthStatus::validation_failed(
        AdcValidationState::ProviderUnavailable,
        AdcProbeFailure {
            kind: AdcFailureKind::ProviderUnavailable,
            retryable: true,
            message: "Google credential validation timed out".to_string(),
        },
    )
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("x-oscal-subid" = "sch.software.plugin.gcloud-adc.schema@v2"))]
#[schemars(extend("x-oscal-category" = "software"))]
pub struct GcloudAdcState {
    pub status: AdcAuthStatus,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CheckAuthStatusInput {}

#[async_trait]
pub(crate) trait AdcStatusProvider: Send + Sync {
    async fn check_auth_status(&self) -> Result<AdcAuthStatus>;
}

pub(crate) struct GoogleCloudAuthProvider;

#[async_trait]
impl AdcStatusProvider for GoogleCloudAuthProvider {
    async fn check_auth_status(&self) -> Result<AdcAuthStatus> {
        use google_cloud_auth::credentials::Builder;

        // The official SDK owns ADC file and metadata-service discovery.
        let credentials = match Builder::default()
            .with_scopes([CLOUD_PLATFORM_SCOPE])
            .build_access_token_credentials()
        {
            Ok(credentials) => credentials,
            Err(error) => {
                let message = if error.is_loading() {
                    "ADC credentials could not be loaded"
                } else if error.is_parsing() {
                    "ADC credentials could not be parsed"
                } else if error.is_unknown_type() {
                    "ADC credential type is unknown"
                } else if error.is_missing_field() {
                    "ADC credentials are missing a required field"
                } else if error.is_not_supported() {
                    "ADC credential type is unsupported"
                } else {
                    "ADC credentials could not be configured"
                };
                return Ok(AdcAuthStatus::not_configured(AdcProbeFailure {
                    kind: AdcFailureKind::Configuration,
                    retryable: false,
                    message: message.to_string(),
                }));
            }
        };

        match credentials.access_token().await {
            Ok(token) if !token.token.is_empty() => {
                drop(token);
                Ok(AdcAuthStatus::validated())
            }
            Ok(token) => {
                drop(token);
                Ok(AdcAuthStatus::validation_failed(
                    AdcValidationState::Rejected,
                    AdcProbeFailure {
                        kind: AdcFailureKind::ProviderRejected,
                        retryable: false,
                        message: "Google returned an unusable credential response".to_string(),
                    },
                ))
            }
            Err(error) => {
                let retryable = error.is_transient();
                let (kind, message) = if retryable {
                    (
                        AdcFailureKind::ProviderUnavailable,
                        "Google credential validation is temporarily unavailable",
                    )
                } else {
                    (
                        AdcFailureKind::ProviderRejected,
                        "Google did not validate the configured ADC credential",
                    )
                };
                // SDK errors may contain response bodies. Never format them.
                drop(error);
                let state = if retryable {
                    AdcValidationState::ProviderUnavailable
                } else {
                    AdcValidationState::Rejected
                };
                Ok(AdcAuthStatus::validation_failed(
                    state,
                    AdcProbeFailure {
                        kind,
                        retryable,
                        message: message.to_string(),
                    },
                ))
            }
        }
    }
}

pub struct GcloudAdcPlugin;

impl Default for GcloudAdcPlugin {
    fn default() -> Self {
        Self
    }
}

impl GcloudAdcPlugin {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl StatePlugin for GcloudAdcPlugin {
    fn name(&self) -> &str {
        "gcloud_adc"
    }
    fn version(&self) -> &str {
        "2.0.0"
    }
    fn schema(&self) -> Option<PluginSchema> {
        Some(gcloud_adc_schema())
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

pub fn gcloud_adc_schema() -> PluginSchema {
    let root =
        serde_json::to_value(schemars::schema_for!(GcloudAdcState)).expect("schema serializes");
    let mut schema = super::schemars_adapter::plugin_schema_from_json(
        "gcloud_adc",
        "2.0.0",
        "Google Cloud ADC provider validation state",
        &root,
    );
    use super::plugin_scaffold_helpers::method_decl_from_schemars_with_output;
    use op_state_store::SideEffect;
    schema.methods.insert(
        "check_auth_status".to_string(),
        method_decl_from_schemars_with_output::<CheckAuthStatusInput, AdcAuthStatus>(
            "check_auth_status",
            SideEffect::Read,
            true,
            "gcloud_adc.read",
            "obs.software.plugin.gcloud-adc.auth-status.check@v2",
        ),
    );
    schema.capabilities.insert(
        "gcloud_adc.read".to_string(),
        op_state_store::CapabilityDecl {
            id: "gcloud_adc.read".to_string(),
            description: "Grants: check_auth_status.".to_string(),
        },
    );
    schema
}

pub(crate) async fn dispatch_gcloud_adc_with_provider(
    provider: &dyn AdcStatusProvider,
    method: &str,
    args: &serde_json::Value,
) -> Result<serde_json::Value> {
    dispatch_gcloud_adc_with_provider_timeout(provider, method, args, ADC_PROBE_TIMEOUT).await
}

async fn dispatch_gcloud_adc_with_provider_timeout(
    provider: &dyn AdcStatusProvider,
    method: &str,
    args: &serde_json::Value,
    timeout: std::time::Duration,
) -> Result<serde_json::Value> {
    match method {
        "check_auth_status" => {
            serde_json::from_value::<CheckAuthStatusInput>(args.clone())?;
            let status = match tokio::time::timeout(timeout, provider.check_auth_status()).await {
                Ok(result) => result?,
                Err(_) => provider_timeout_status(),
            };
            Ok(serde_json::to_value(status)?)
        }
        other => Err(anyhow::anyhow!("unknown gcloud_adc method: {other}")),
    }
}

pub async fn dispatch_gcloud_adc_method(
    method: &str,
    args: &serde_json::Value,
) -> Result<serde_json::Value> {
    dispatch_gcloud_adc_with_provider(&GoogleCloudAuthProvider, method, args).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state_plugins::common::oscal::validate_subid;

    struct FakeProvider(std::result::Result<AdcAuthStatus, &'static str>);
    #[async_trait]
    impl AdcStatusProvider for FakeProvider {
        async fn check_auth_status(&self) -> Result<AdcAuthStatus> {
            self.0.clone().map_err(anyhow::Error::msg)
        }
    }

    fn failure_status(kind: AdcFailureKind, retryable: bool) -> AdcAuthStatus {
        let failure = AdcProbeFailure {
            kind,
            retryable,
            message: "sanitized".to_string(),
        };
        if kind == AdcFailureKind::Configuration {
            AdcAuthStatus::not_configured(failure)
        } else {
            let state = if retryable {
                AdcValidationState::ProviderUnavailable
            } else {
                AdcValidationState::Rejected
            };
            AdcAuthStatus::validation_failed(state, failure)
        }
    }

    #[tokio::test]
    async fn validated_status_contains_no_credential_fields() {
        let value = dispatch_gcloud_adc_with_provider(
            &FakeProvider(Ok(AdcAuthStatus::validated())),
            "check_auth_status",
            &serde_json::json!({}),
        )
        .await
        .unwrap();
        assert_eq!(value["authenticated"], true);
        assert_eq!(value["validation"], "validated");
        let encoded = serde_json::to_string(&value).unwrap();
        for name in [
            "access_token",
            "refresh_token",
            "private_key",
            "client_secret",
        ] {
            assert!(!encoded.contains(name));
        }
    }

    #[tokio::test]
    async fn failure_classes_remain_distinct_and_unprobed() {
        for (kind, retryable) in [
            (AdcFailureKind::Configuration, false),
            (AdcFailureKind::ProviderUnavailable, true),
            (AdcFailureKind::ProviderRejected, false),
        ] {
            let value = dispatch_gcloud_adc_with_provider(
                &FakeProvider(Ok(failure_status(kind, retryable))),
                "check_auth_status",
                &serde_json::json!({}),
            )
            .await
            .unwrap();
            assert_eq!(value["authenticated"], false);
            let expected_validation = match kind {
                AdcFailureKind::Configuration => "not_configured",
                AdcFailureKind::ProviderUnavailable => "provider_unavailable",
                AdcFailureKind::ProviderRejected => "rejected",
            };
            assert_eq!(value["validation"], expected_validation);
            assert_eq!(value["failure"]["retryable"], retryable);
            assert_eq!(
                serde_json::from_value::<AdcFailureKind>(value["failure"]["kind"].clone()).unwrap(),
                kind
            );
        }
    }

    #[tokio::test]
    async fn input_and_provider_errors_are_reported() {
        let provider = FakeProvider(Err("offline provider failed"));
        let err = dispatch_gcloud_adc_with_provider(
            &provider,
            "check_auth_status",
            &serde_json::json!({"unexpected": true}),
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("unknown field"));
        let err = dispatch_gcloud_adc_with_provider(
            &provider,
            "check_auth_status",
            &serde_json::json!({}),
        )
        .await
        .unwrap_err();
        assert_eq!(err.to_string(), "offline provider failed");
    }

    struct PendingProvider;

    #[async_trait]
    impl AdcStatusProvider for PendingProvider {
        async fn check_auth_status(&self) -> Result<AdcAuthStatus> {
            std::future::pending().await
        }
    }

    #[tokio::test]
    async fn provider_probe_has_a_bounded_deadline() {
        let value = dispatch_gcloud_adc_with_provider_timeout(
            &PendingProvider,
            "check_auth_status",
            &serde_json::json!({}),
            std::time::Duration::from_millis(1),
        )
        .await
        .unwrap();
        assert_eq!(value["validation"], "provider_unavailable");
        assert_eq!(value["authenticated"], false);
        assert_eq!(value["failure"]["retryable"], true);
    }

    #[test]
    fn method_schema_has_typed_input_and_output() {
        let method = &gcloud_adc_schema().methods["check_auth_status"];
        assert_eq!(method.args["type"], "object");
        assert_eq!(method.returns.as_ref().unwrap()["type"], "object");
    }

    #[test]
    fn all_subids_are_valid() {
        let raw = serde_json::to_value(schemars::schema_for!(GcloudAdcState)).unwrap();
        let mut stack = vec![&raw];
        while let Some(node) = stack.pop() {
            if let Some(subid) = node.get("x-oscal-subid").and_then(|v| v.as_str()) {
                validate_subid(subid).unwrap();
            }
            if let Some(props) = node.get("properties").and_then(|v| v.as_object()) {
                stack.extend(props.values());
            }
            if let Some(defs) = node
                .get("$defs")
                .or_else(|| node.get("definitions"))
                .and_then(|v| v.as_object())
            {
                stack.extend(defs.values());
            }
        }
    }
}

inventory::submit! {
    crate::default_registry::PluginReg::new("gcloud_adc", |_ctx| std::sync::Arc::new(GcloudAdcPlugin::new()))
}
