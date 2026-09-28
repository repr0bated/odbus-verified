//! Control-plane chat follows the dedicated native agent's configured model.
//!
//! This module observes the `control_plane` agent. It never changes the
//! dashboard selection or accepts a request-level provider/model override.

use std::sync::Arc;

use anyhow::{Context, ensure};
use op_plugins::state_plugins::tched_router::{ChatInput, ChatOutput};
use serde::Serialize;

use crate::chat_mode_routing::CONTROL_PLANE_AGENT;
use crate::tched_router_generation_native::NativeGenerationAdmission;
use crate::tched_router_runtime::{RuntimeSelection, TchedRouterRuntimeClient};

#[derive(Debug, Serialize)]
struct RouteDecision<'a> {
    native_alias: &'a str,
    provider: &'a str,
    model: &'a str,
    source: &'a str,
}

/// Pin the observed native selection into the request. Caller-supplied
/// provider/model fields are not control-plane configuration authority.
fn apply_native_selection(
    input: &mut ChatInput,
    selection: &RuntimeSelection,
) -> anyhow::Result<()> {
    ensure!(
        !selection.provider.trim().is_empty() && !selection.model.trim().is_empty(),
        "control-plane native agent has no configured provider/model"
    );
    input.provider.clone_from(&selection.provider);
    input.model.clone_from(&selection.model);
    Ok(())
}

/// Execute under the caller's existing native admission guard. Provider-only
/// callers use stateless generation; chat callers use the native A2A agent.
pub(crate) async fn complete(
    input: ChatInput,
    before_prompt: Arc<dyn NativeGenerationAdmission>,
    provider_only: bool,
) -> anyhow::Result<ChatOutput> {
    let client = TchedRouterRuntimeClient::for_agent_alias(CONTROL_PLANE_AGENT)
        .with_sealed_identity(before_prompt.sealed_identity());
    complete_with_client(input, before_prompt, provider_only, client).await
}

async fn complete_with_client(
    mut input: ChatInput,
    before_prompt: Arc<dyn NativeGenerationAdmission>,
    provider_only: bool,
    client: TchedRouterRuntimeClient,
) -> anyhow::Result<ChatOutput> {
    before_prompt
        .check()
        .await
        .map_err(|error| anyhow::anyhow!("control-plane routing admission failed: {error}"))?;
    let selection = client
        .current_selection()
        .await
        .context("control-plane native agent selection is unavailable")?;
    apply_native_selection(&mut input, &selection)?;
    before_prompt
        .check()
        .await
        .map_err(|error| anyhow::anyhow!("control-plane execution admission failed: {error}"))?;
    let mut output = if provider_only {
        client.complete_under_guard(input, before_prompt).await?
    } else {
        client.chat_under_guard(input).await?
    };
    ensure!(
        output.provider == selection.provider && output.model == selection.model,
        "control-plane native execution did not retain its observed provider/model"
    );
    if !output.usage.is_object() {
        output.usage = serde_json::json!({});
    }
    output.usage["routing"] = serde_json::to_value(RouteDecision {
        native_alias: CONTROL_PLANE_AGENT,
        provider: &output.provider,
        model: &output.model,
        source: "native_agent",
    })?;
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        Json, Router,
        extract::Query,
        http::{HeaderMap, HeaderValue},
        response::IntoResponse,
        routing::{get, post},
    };
    use serde_json::{Value, json};
    use std::{
        collections::HashMap,
        sync::atomic::{AtomicUsize, Ordering},
    };

    struct Admission(AtomicUsize);

    impl NativeGenerationAdmission for Admission {
        fn sealed_identity(&self) -> Option<&str> {
            Some("canonical-fixture-sid1")
        }

        fn check(
            &self,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<
                        Output = Result<
                            (),
                            crate::tched_router_generation_native::NativeGenerationError,
                        >,
                    > + Send
                    + '_,
            >,
        > {
            self.0.fetch_add(1, Ordering::SeqCst);
            Box::pin(async { Ok(()) })
        }
    }

    fn selection(provider: &str, model: &str) -> RuntimeSelection {
        RuntimeSelection {
            provider: provider.into(),
            model: model.into(),
        }
    }

    #[test]
    fn dedicated_agent_accepts_any_configured_provider_and_ignores_request_override() {
        for configured in [
            selection("openai", "gpt-6-astra"),
            selection("anthropic", "claude-fable-5-1"),
            selection("custom", "local/model"),
        ] {
            let mut input = ChatInput {
                message: "hello".into(),
                provider: "dashboard-override".into(),
                model: "unrelated-model".into(),
                ..ChatInput::default()
            };
            apply_native_selection(&mut input, &configured).unwrap();
            assert_eq!(input.provider, configured.provider);
            assert_eq!(input.model, configured.model);
            assert_eq!(input.message, "hello");
        }
    }

    #[test]
    fn missing_native_selection_fails_closed() {
        let mut input = ChatInput::default();
        assert!(apply_native_selection(&mut input, &selection("", "model")).is_err());
        assert!(apply_native_selection(&mut input, &selection("openai", "")).is_err());
    }

    #[test]
    fn route_audit_names_the_native_agent_and_executed_provider() {
        let audit = serde_json::to_value(RouteDecision {
            native_alias: CONTROL_PLANE_AGENT,
            provider: "anthropic",
            model: "claude-fable-5-1",
            source: "native_agent",
        })
        .unwrap();
        assert_eq!(audit["native_alias"], "control_plane");
        assert_eq!(audit["provider"], "anthropic");
        assert_eq!(audit["model"], "claude-fable-5-1");
        assert_eq!(audit["source"], "native_agent");
    }

    #[tokio::test]
    async fn native_anthropic_route_ignores_opencode_override_and_requires_sid1_ack() {
        async fn health() -> Json<Value> {
            Json(json!({"status":"ok"}))
        }
        async fn prop(Query(query): Query<HashMap<String, String>>) -> Json<Value> {
            let value = match query.get("path").map(String::as_str) {
                Some("agents.control_plane.model_provider") => "anthropic.main",
                Some("providers.models.anthropic.main.model") => "claude-fable-5-1",
                other => panic!("unexpected native property: {other:?}"),
            };
            Json(json!({"value":value}))
        }
        async fn list() -> Json<Value> {
            Json(json!({"entries":[
                {"path":"providers.models.anthropic.main.model","value":"claude-fable-5-1"}
            ]}))
        }
        async fn chat(headers: HeaderMap, Json(body): Json<Value>) -> impl IntoResponse {
            assert_eq!(
                headers
                    .get("x-opdbus-sealed-id-bin")
                    .and_then(|v| v.to_str().ok()),
                Some("canonical-fixture-sid1")
            );
            assert_eq!(body["method"], "message/send");
            let mut response = Json(json!({"result":{"artifacts":[{"parts":[
                {"kind":"text","text":"configured native answer"}
            ]}]}}))
            .into_response();
            response.headers_mut().insert(
                "x-opdbus-identity-authenticated",
                HeaderValue::from_static("SID1"),
            );
            response
        }
        // No OpenCode inventory endpoint: calling it makes the route fail.
        let app = Router::new()
            .route("/health", get(health))
            .route("/api/config/prop", get(prop))
            .route("/api/config/list", get(list))
            .route("/a2a/control_plane", post(chat));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client = TchedRouterRuntimeClient::new(endpoint, CONTROL_PLANE_AGENT.into(), None)
            .with_sealed_identity(Some("canonical-fixture-sid1"));
        let admission = Arc::new(Admission(AtomicUsize::new(0)));
        let result = complete_with_client(
            ChatInput {
                message: "hello".into(),
                provider: "custom".into(),
                model: "opencode/glm-5.3-flash".into(),
                ..ChatInput::default()
            },
            admission.clone(),
            false,
            client,
        )
        .await
        .unwrap();
        assert_eq!(result.content, "configured native answer");
        assert_eq!(result.provider, "anthropic");
        assert_eq!(result.model, "claude-fable-5-1");
        assert_eq!(result.usage["routing"]["native_alias"], "control_plane");
        assert_eq!(result.usage["routing"]["provider"], "anthropic");
        assert!(admission.0.load(Ordering::SeqCst) >= 2);
        server.abort();
    }
}
