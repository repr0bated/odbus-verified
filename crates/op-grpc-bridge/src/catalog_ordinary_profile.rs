//! Materialize missing ordinary native profiles from `model-providers.toml` metadata
//! before catalog binding reflection runs.

use anyhow::{Context, Result, ensure};
use op_plugins::state_plugins::model_registry::{ModelListFormat, ProviderConfig};
use op_plugins::state_plugins::tched_router::{ConfigPatchInput, ConfigPatchOperation};
use serde_json::{Value, json};

use crate::model_registry_dispatch::{Environment, read_provider_key};
use crate::tched_router_runtime::{ConfigSecretPolicy, TchedRouterRuntimeClient};

fn profile_section(reference: &str) -> String {
    format!("providers.models.{reference}")
}

fn slash_path(reference: &str, field: &str) -> String {
    format!(
        "/providers/models/{}/{}",
        reference.replace('.', "/"),
        field
    )
}

fn wire_api(format: ModelListFormat) -> &'static str {
    match format {
        ModelListFormat::Openai => "chat_completions",
        ModelListFormat::Anthropic => "chat_completions",
    }
}

/// Create the ordinary profile section when absent, using provider metadata only.
pub(crate) async fn ensure_ordinary_profile(
    env: &Environment,
    runtime: &TchedRouterRuntimeClient,
    provider: &ProviderConfig,
    reference: &str,
    model_id: &str,
    secrets: &dyn ConfigSecretPolicy,
) -> Result<()> {
    ensure!(provider.enabled, "catalog provider is disabled");
    // Subscription-native profiles (`requires_openai_auth`) are provisioned
    // by the operator through the native auth CLI; admission only keeps
    // their model current. No key material and no HTTP endpoint are ever
    // written for them, and the provider file's key_file is not required.
    let auth_flag_path = format!("{}.requires_openai_auth", profile_section(reference));
    let oauth_profile = matches!(
        runtime
            .read_config_property(&auth_flag_path, secrets)
            .await
            .ok()
            .and_then(|value| value.get("value").and_then(Value::as_str).map(String::from)),
        Some(ref value) if value == "true"
    );
    let key_file = if oauth_profile {
        None
    } else {
        Some(
            provider
                .key_file
                .as_ref()
                .context("catalog provider has no key_file for native provisioning")?,
        )
    };
    let uri_path = format!("{}.uri", profile_section(reference));
    let model_path = format!("{}.model", profile_section(reference));
    if runtime
        .read_config_property(&uri_path, secrets)
        .await
        .is_ok()
    {
        let current = runtime
            .read_config_property(&model_path, secrets)
            .await
            .ok()
            .and_then(|value| {
                value
                    .get("value")
                    .and_then(|v| v.as_str())
                    .map(str::to_string)
            });
        if current.as_deref() == Some(model_id) {
            return Ok(());
        }
        runtime
            .patch_config(
                &ConfigPatchInput {
                    operations: vec![ConfigPatchOperation::Replace {
                        path: slash_path(reference, "model"),
                        value: json!(model_id),
                    }],
                },
                secrets,
            )
            .await
            .with_context(|| format!("update native profile {reference} model"))?;
        return Ok(());
    }
    let uri = if oauth_profile {
        // Subscription profiles are served from the native auth store's own
        // backend; no HTTP endpoint is provisioned into the profile.
        None
    } else {
        Some(
            provider
                .inference_url_prefix
                .as_deref()
                .context("catalog provider has no inference endpoint authority")?,
        )
    };
    runtime
        .init_provider_model_reference(reference, secrets)
        .await
        .with_context(|| format!("initialize native profile {reference}"))?;
    if oauth_profile {
        let patch = ConfigPatchInput {
            operations: vec![
                ConfigPatchOperation::Add {
                    path: slash_path(reference, "model"),
                    value: json!(model_id),
                },
                ConfigPatchOperation::Add {
                    path: slash_path(reference, "wire_api"),
                    value: json!(wire_api(provider.format)),
                },
                ConfigPatchOperation::Add {
                    path: slash_path(reference, "requires_openai_auth"),
                    value: json!(true),
                },
                ConfigPatchOperation::Add {
                    path: slash_path(reference, "native_tools"),
                    value: json!(false),
                },
            ],
        };
        runtime
            .patch_config(&patch, secrets)
            .await
            .with_context(|| format!("provision subscription-native profile {reference}"))?;
        return Ok(());
    }
    let api_key = read_provider_key(
        env,
        key_file.expect("keyed provisioning requires a key_file"),
    )?;
    let patch = ConfigPatchInput {
        operations: vec![
            ConfigPatchOperation::Add {
                path: slash_path(reference, "model"),
                value: json!(model_id),
            },
            ConfigPatchOperation::Add {
                path: slash_path(reference, "uri"),
                value: json!(uri.context("keyed provisioning requires an endpoint authority")?),
            },
            ConfigPatchOperation::Add {
                path: slash_path(reference, "wire_api"),
                value: json!(wire_api(provider.format)),
            },
            ConfigPatchOperation::Add {
                path: slash_path(reference, "native_tools"),
                value: json!(false),
            },
            ConfigPatchOperation::Add {
                path: slash_path(reference, "api_key"),
                value: json!(api_key),
            },
        ],
    };
    runtime
        .patch_config(&patch, secrets)
        .await
        .with_context(|| format!("provision native profile {reference}"))?;
    Ok(())
}
