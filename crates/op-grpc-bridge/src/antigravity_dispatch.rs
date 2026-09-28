//! Canonical method owner for the `antigravity` and `antigravity_chat` plugins.
//!
//! D-Bus/PluginSchema admission happens in `MutationEngine`. This module owns
//! native provider observations/effects and returns a complete authoritative
//! state snapshot for MutationEngine to commit to SHM before notification.

use anyhow::{Context, Result};
use op_llm::antigravity::{
    GoogleCredentialOwner, auth_status_for_config, configure_runtime_with_documents,
    configure_safety_with_documents, last_bridge_status_with_documents, load_runtime_config_from,
    observe_bridge_with_documents, usage_report_with_documents,
};
use op_plugins::state_plugins::{
    antigravity::{AntigravityState, ConfigureSafetyInput, GetUsageReportInput, LocalUsageReport},
    antigravity_chat::{AntigravityChatState, ChatConfig, ConfigureInput, NativeBridgeStatus},
    runtime_documents::RuntimeDocuments,
};
use serde_json::{Value, json};
use std::sync::Arc;

pub(crate) struct DispatchOutcome {
    pub output: Value,
    pub state: Value,
}

async fn observe_antigravity(
    documents: &RuntimeDocuments,
    credentials: &GoogleCredentialOwner,
) -> Result<AntigravityState> {
    let (_, runtime) = load_runtime_config_from(documents).await?;
    let auth = auth_status_for_config(credentials, &runtime).await;
    let usage = usage_report_with_documents(documents, None).await?;
    Ok(AntigravityState {
        status: "configured".to_string(),
        auth,
        llm_plugin: "large_language_model".to_string(),
        provider_route: runtime.provider.clone(),
        selected_model: runtime.selected_model.clone(),
        provider: runtime.provider,
        base_url: runtime.base_url,
        safety_settings: runtime.safety_settings,
        usage,
    })
}

async fn observe_antigravity_chat(
    documents: &RuntimeDocuments,
    credentials: &GoogleCredentialOwner,
) -> Result<AntigravityChatState> {
    let (_, runtime) = load_runtime_config_from(documents).await?;
    let auth = auth_status_for_config(credentials, &runtime).await;
    let bridge = last_bridge_status_with_documents(documents)
        .await?
        .filter(|status| runtime.provider == "openai_compatible" && status.url == runtime.base_url)
        .unwrap_or_else(|| NativeBridgeStatus {
            url: if runtime.provider == "openai_compatible" {
                runtime.base_url.clone()
            } else {
                String::new()
            },
            ..Default::default()
        });
    let status = if bridge.checked_at.is_empty() {
        "configured"
    } else if bridge.reachable {
        "online"
    } else {
        "unreachable"
    };
    Ok(AntigravityChatState {
        status: status.to_string(),
        bridge,
        auth,
        llm_plugin: "large_language_model".to_string(),
        provider_route: runtime.provider.clone(),
        selected_model: runtime.selected_model,
        config: ChatConfig {
            provider: runtime.provider,
            base_url: runtime.base_url,
        },
    })
}

async fn antigravity(
    method: &str,
    args: &Value,
    documents: Arc<RuntimeDocuments>,
    credentials: &GoogleCredentialOwner,
) -> Result<DispatchOutcome> {
    let output = match method {
        "get_auth_status" => {
            let state = observe_antigravity(&documents, credentials).await?;
            return Ok(DispatchOutcome {
                output: json!({"auth": &state.auth}),
                state: serde_json::to_value(state)?,
            });
        }
        "get_usage_report" => {
            let input: GetUsageReportInput = serde_json::from_value(args.clone())?;
            if let Some(model) = input.model.as_deref() {
                anyhow::ensure!(
                    !model.is_empty()
                        && model.len() <= 256
                        && model.bytes().all(|byte| byte.is_ascii_alphanumeric()
                            || matches!(byte, b'.' | b'_' | b'-')),
                    "usage model filter is invalid"
                );
            }
            let usage: LocalUsageReport =
                usage_report_with_documents(&documents, input.model.as_deref()).await?;
            json!({"usage": usage})
        }
        "configure_safety" => {
            let input: ConfigureSafetyInput = serde_json::from_value(args.clone())?;
            serde_json::to_value(configure_safety_with_documents(documents.clone(), input).await?)?
        }
        _ => anyhow::bail!("unsupported antigravity method '{method}'"),
    };
    let state = serde_json::to_value(observe_antigravity(&documents, credentials).await?)?;
    Ok(DispatchOutcome { output, state })
}

async fn antigravity_chat(
    method: &str,
    args: &Value,
    documents: Arc<RuntimeDocuments>,
    credentials: &GoogleCredentialOwner,
) -> Result<DispatchOutcome> {
    let output = match method {
        "get_bridge_status" => {
            let (_, runtime) = load_runtime_config_from(&documents).await?;
            let bridge = observe_bridge_with_documents(documents.clone(), &runtime).await?;
            json!({"bridge": bridge})
        }
        "refresh_token" => {
            let (_, runtime) = load_runtime_config_from(&documents).await?;
            anyhow::ensure!(
                runtime.provider == "gemini",
                "refresh_token requires the Gemini provider"
            );
            let token = credentials
                .refresh()
                .await
                .context("native Google credential refresh failed")?;
            json!({"token": token})
        }
        "configure" => {
            let input: ConfigureInput = serde_json::from_value(args.clone())?;
            serde_json::to_value(configure_runtime_with_documents(documents.clone(), input).await?)?
        }
        _ => anyhow::bail!("unsupported antigravity_chat method '{method}'"),
    };
    let state = serde_json::to_value(observe_antigravity_chat(&documents, credentials).await?)?;
    Ok(DispatchOutcome { output, state })
}

pub(crate) async fn dispatch(
    plugin: &str,
    method: &str,
    args: &Value,
    documents: Arc<RuntimeDocuments>,
    credentials: Arc<GoogleCredentialOwner>,
) -> Result<DispatchOutcome> {
    match plugin {
        "antigravity" => antigravity(method, args, documents, &credentials).await,
        "antigravity_chat" => antigravity_chat(method, args, documents, &credentials).await,
        _ => anyhow::bail!("unsupported Antigravity plugin '{plugin}'"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use op_cozo_store::CozoGraphShuttle;
    use op_llm::antigravity::{
        google_credential_owner, record_usage_with_documents, usage_report_with_documents,
    };
    use op_llm::provider::TokenUsage;

    fn documents() -> Arc<RuntimeDocuments> {
        Arc::new(RuntimeDocuments::new(
            CozoGraphShuttle::new_in_memory().unwrap(),
        ))
    }

    #[tokio::test]
    async fn configure_and_safety_preserve_the_complete_observed_state() {
        let documents = documents();
        let configured = dispatch(
            "antigravity_chat",
            "configure",
            &json!({
                "expected_revision": 0,
                "provider": "gemini",
                "base_url": "https://generativelanguage.googleapis.com/v1beta",
                "selected_model": "gemini-test"
            }),
            documents.clone(),
            google_credential_owner(),
        )
        .await
        .unwrap();
        assert_eq!(configured.output["revision"], 1);
        assert_eq!(configured.state["config"]["provider"], "gemini");
        assert_eq!(configured.state["selected_model"], "gemini-test");

        let safety = dispatch(
            "antigravity",
            "configure_safety",
            &json!({
                "expected_revision": 1,
                "settings": [{
                    "category": "HARM_CATEGORY_HARASSMENT",
                    "threshold": "BLOCK_ONLY_HIGH"
                }]
            }),
            documents.clone(),
            google_credential_owner(),
        )
        .await
        .unwrap();
        assert_eq!(safety.output["revision"], 2);
        assert_eq!(safety.state["selected_model"], "gemini-test");
        assert_eq!(
            safety.state["safety_settings"][0]["threshold"],
            "BLOCK_ONLY_HIGH"
        );
        let row = documents
            .read("antigravity.runtime.v1")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.document["selected_model"], "gemini-test");
        assert!(
            dispatch(
                "antigravity_chat",
                "configure",
                &json!({
                    "expected_revision": 1,
                    "provider": "gemini",
                    "base_url": "https://generativelanguage.googleapis.com/v1beta",
                    "selected_model": "stale"
                }),
                documents,
                google_credential_owner(),
            )
            .await
            .is_err()
        );
    }

    #[tokio::test]
    async fn usage_filter_does_not_replace_the_complete_state_ledger() {
        let documents = documents();
        for model in ["model-a", "model-b"] {
            record_usage_with_documents(
                documents.clone(),
                model.to_string(),
                TokenUsage {
                    prompt_tokens: 9,
                    completion_tokens: 4,
                    total_tokens: 13,
                },
            )
            .await
            .unwrap();
        }
        let direct = usage_report_with_documents(&documents, Some("model-a"))
            .await
            .unwrap();
        assert_eq!(direct.total_tokens, 13);
        let outcome = dispatch(
            "antigravity",
            "get_usage_report",
            &json!({"model":"model-a"}),
            documents,
            google_credential_owner(),
        )
        .await
        .unwrap();
        assert_eq!(outcome.output["usage"]["requests"], 1);
        assert_eq!(outcome.state["usage"]["requests"], 2);
        assert!(outcome.state["usage"]["models"].get("model-b").is_some());
        assert!(outcome.output["usage"].get("quota").is_none());
        assert!(outcome.output["usage"].get("billing").is_none());
    }
}
