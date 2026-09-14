//! OpenClaw LLM Provider
//!
//! Connects to the OpenClaw agent platform via its OpenAI-compatible
//! `/v1/chat/completions` endpoint over the trusted internal network.
//!
//! ## Configuration
//!
//! ```bash
//! OPENCLAW_BASE_URL=http://127.0.0.1:8090  # default
//! ```

use anyhow::{Context, Result};
use async_trait::async_trait;
use op_identity::{configured_identity_session, SessionIdentity};
use reqwest::Client;
use simd_json::prelude::*;
use simd_json::{json, OwnedValue as Value};
use std::time::Duration;
use tracing::{debug, info, warn};

use crate::provider::{
    ChatMessage, ChatRequest, ChatResponse, LlmProvider, ModelInfo, ProviderType, TokenUsage,
    ToolCallInfo,
};

const DEFAULT_BASE_URL: &str = "http://127.0.0.1:8090";
const DEFAULT_MODEL: &str = "openclaw:main";

pub struct OpenClawProvider {
    client: Client,
    base_url: String,
    default_model: String,
}

impl OpenClawProvider {
    pub fn new(base_url: Option<String>, default_model: Option<String>) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(180))
            .build()
            .expect("Failed to create HTTP client");

        Self {
            client,
            base_url: base_url
                .unwrap_or_else(|| DEFAULT_BASE_URL.to_string())
                .trim_end_matches('/')
                .to_string(),
            default_model: default_model.unwrap_or_else(|| DEFAULT_MODEL.to_string()),
        }
    }

    pub fn from_env() -> Result<Self> {
        let base_url = std::env::var("OPENCLAW_BASE_URL").ok();
        let default_model = std::env::var("OPENCLAW_DEFAULT_MODEL").ok();
        Ok(Self::new(base_url, default_model))
    }

    fn models_url(&self) -> String {
        format!("{}/v1/models", self.base_url)
    }

    fn chat_url(&self) -> String {
        format!("{}/v1/chat/completions", self.base_url)
    }

    fn resolve_model(&self, model: &str) -> String {
        if model.is_empty() {
            self.default_model.clone()
        } else {
            model.to_string()
        }
    }

    fn ghostbridge_identity_headers(identity: &SessionIdentity) -> Option<(String, String)> {
        identity.is_anchored().then(|| {
            (
                identity.genesis.clone().unwrap_or_default(),
                identity.trace_id.clone(),
            )
        })
    }

    fn api_request(&self, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        let builder = builder.header("Content-Type", "application/json");
        let Ok(identity) = configured_identity_session() else {
            return builder;
        };
        let headers = Self::ghostbridge_identity_headers(&identity);

        match headers {
            Some((genesis, trace_id)) => builder
                .header("x-ghostbridge-genesis", genesis.clone())
                .header("x-ghostbridge-footprint", genesis)
                .header("x-ghostbridge-trace-id", trace_id),
            None => builder,
        }
    }

    fn fallback_model_info(&self) -> ModelInfo {
        ModelInfo {
            id: self.default_model.clone(),
            name: self.default_model.clone(),
            description: Some(
                "Configured OpenClaw default route (OpenClaw selects the agent's configured model)"
                    .to_string(),
            ),
            parameters: None,
            available: true,
            tags: vec![
                "openclaw".to_string(),
                "default".to_string(),
                "agent-route".to_string(),
            ],
            downloads: None,
            updated_at: None,
        }
    }

    fn parse_models_response(response_text: &str) -> Result<Vec<ModelInfo>> {
        let mut response_text_mut = response_text.to_string();
        let response_json: Value = unsafe { simd_json::from_str(&mut response_text_mut) }
            .map_err(|e| anyhow::anyhow!("Failed to parse OpenClaw models response: {}", e))?;

        let models = response_json
            .get("data")
            .and_then(|v| v.as_array())
            .map(|entries| {
                entries
                    .iter()
                    .filter_map(|entry| {
                        let id = entry.get("id")?.as_str()?.to_string();
                        let owned_by = entry
                            .get("owned_by")
                            .and_then(|v| v.as_str())
                            .unwrap_or("openclaw")
                            .to_string();
                        let created = entry
                            .get("created")
                            .and_then(|v| v.as_i64())
                            .map(|ts| ts.to_string());

                        Some(ModelInfo {
                            id: id.clone(),
                            name: id,
                            description: Some(format!("OpenClaw model owned by {}", owned_by)),
                            parameters: None,
                            available: true,
                            tags: vec!["openclaw".to_string(), owned_by],
                            downloads: None,
                            updated_at: created,
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        Ok(models)
    }
}

#[async_trait]
impl LlmProvider for OpenClawProvider {
    fn provider_type(&self) -> ProviderType {
        ProviderType::OpenClaw
    }

    async fn list_models(&self) -> Result<Vec<ModelInfo>> {
        let response = self
            .api_request(self.client.get(self.models_url()))
            .send()
            .await
            .context("Failed to query OpenClaw models")?;

        let status = response.status();
        let response_text = response.text().await?;

        if !status.is_success() {
            warn!(
                "OpenClaw model listing failed ({}), falling back to configured default route",
                status
            );
            return Ok(vec![self.fallback_model_info()]);
        }

        let mut models = match Self::parse_models_response(&response_text) {
            Ok(models) => models,
            Err(err) => {
                warn!(
                    "OpenClaw /v1/models did not return a usable model list ({}), falling back to configured default route",
                    err
                );
                return Ok(vec![self.fallback_model_info()]);
            }
        };
        if models.is_empty() {
            models.push(self.fallback_model_info());
        }

        Ok(models)
    }

    async fn search_models(&self, query: &str, limit: usize) -> Result<Vec<ModelInfo>> {
        let models = self.list_models().await?;
        Ok(models
            .into_iter()
            .filter(|m| m.name.to_lowercase().contains(&query.to_lowercase()))
            .take(limit)
            .collect())
    }

    async fn get_model(&self, model_id: &str) -> Result<Option<ModelInfo>> {
        let models = self.list_models().await?;
        Ok(models.into_iter().find(|m| m.id == model_id))
    }

    async fn is_model_available(&self, model_id: &str) -> Result<bool> {
        Ok(self.get_model(model_id).await?.is_some())
    }

    async fn chat(&self, model: &str, messages: Vec<ChatMessage>) -> Result<ChatResponse> {
        warn!("Using chat() without tools - consider using chat_with_request()");
        let request = ChatRequest::new(messages);
        self.chat_with_request(model, request).await
    }

    async fn chat_with_request(&self, model: &str, request: ChatRequest) -> Result<ChatResponse> {
        let model = self.resolve_model(model);
        let url = self.chat_url();

        // Convert messages to OpenAI format
        let messages: Vec<Value> = request
            .messages
            .iter()
            .map(|m| {
                let mut msg = json!({
                    "role": m.role,
                    "content": m.content
                });

                if let Some(ref id) = m.tool_call_id {
                    msg["tool_call_id"] = json!(id);
                }

                if let Some(ref calls) = m.tool_calls {
                    msg["tool_calls"] = json!(calls.iter().map(|tc| {
                        json!({
                            "id": tc.id,
                            "type": "function",
                            "function": {
                                "name": tc.name,
                                "arguments": simd_json::to_string(&tc.arguments).unwrap_or_default()
                            }
                        })
                    }).collect::<Vec<_>>());
                }

                msg
            })
            .collect();

        let tools: Vec<Value> = request.tools.iter().map(|t| t.to_openai_format()).collect();

        let mut body = json!({
            "model": model,
            "messages": messages,
            "stream": false
        });
        let body_object = body
            .as_object_mut()
            .expect("openclaw request body should be an object");

        if !tools.is_empty() {
            body_object.insert("tools".into(), json!(tools));
            body_object.insert("tool_choice".into(), request.tool_choice.to_api_format());
            info!(
                "OpenClaw request with {} tools, tool_choice={:?}",
                tools.len(),
                request.tool_choice
            );
        }

        if let Some(max_tokens) = request.max_tokens {
            body_object.insert("max_tokens".into(), json!(max_tokens));
        }
        if let Some(temp) = request.temperature {
            body_object.insert("temperature".into(), json!(temp));
        }
        if let Some(top_p) = request.top_p {
            body_object.insert("top_p".into(), json!(top_p));
        }

        debug!(
            "OpenClaw request: {}",
            simd_json::to_string_pretty(&body).unwrap_or_default()
        );

        let response = self
            .api_request(self.client.post(&url))
            .json(&body)
            .send()
            .await
            .context("Failed to send request to OpenClaw")?;

        let status = response.status();
        let response_text = response.text().await?;

        debug!(
            "OpenClaw response ({}): {}",
            status,
            &response_text[..response_text.len().min(500)]
        );

        if !status.is_success() {
            return Err(anyhow::anyhow!(
                "OpenClaw API error ({}): {}",
                status,
                response_text
            ));
        }

        // Parse OpenAI-compatible response
        let mut response_text_mut = response_text;
        let response_json: Value =
            unsafe { simd_json::from_str(&mut response_text_mut) }.map_err(|e| {
                anyhow::anyhow!(
                    "Failed to parse OpenClaw response: {}. Body: {}",
                    e,
                    response_text_mut
                )
            })?;

        let choice = response_json
            .get("choices")
            .and_then(|c| c.as_array())
            .and_then(|a| a.first())
            .ok_or_else(|| anyhow::anyhow!("No choices returned from OpenClaw"))?;

        let message = choice
            .get("message")
            .ok_or_else(|| anyhow::anyhow!("No message in OpenClaw response"))?;

        let content = message
            .get("content")
            .and_then(|c| c.as_str())
            .unwrap_or("")
            .to_string();

        let role = message
            .get("role")
            .and_then(|r| r.as_str())
            .unwrap_or("assistant")
            .to_string();

        // Parse tool_calls
        let tool_calls: Option<Vec<ToolCallInfo>> = message
            .get("tool_calls")
            .and_then(|tc| tc.as_array())
            .map(|calls| {
                calls
                    .iter()
                    .filter_map(|call| {
                        let id = call.get("id")?.as_str()?.to_string();
                        let function = call.get("function")?;
                        let name = function.get("name")?.as_str()?.to_string();
                        let args_str = function.get("arguments")?.as_str()?;
                        let mut args_mut = args_str.to_string();
                        let arguments: Value =
                            unsafe { simd_json::from_str(&mut args_mut) }.ok()?;

                        Some(ToolCallInfo {
                            id,
                            name,
                            arguments,
                        })
                    })
                    .collect()
            });

        if let Some(ref calls) = tool_calls {
            info!("OpenClaw: parsed {} tool calls", calls.len());
        }

        let finish_reason = choice
            .get("finish_reason")
            .and_then(|f| f.as_str())
            .map(|s| s.to_string());

        let usage = response_json.get("usage").map(|u| TokenUsage {
            prompt_tokens: u.get("prompt_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
            completion_tokens: u
                .get("completion_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32,
            total_tokens: u.get("total_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
        });

        Ok(ChatResponse {
            message: ChatMessage {
                role,
                content,
                tool_calls: tool_calls.clone(),
                tool_call_id: None,
            },
            model,
            provider: "openclaw".to_string(),
            finish_reason,
            usage,
            tool_calls,
        })
    }

    async fn chat_stream(
        &self,
        model: &str,
        messages: Vec<ChatMessage>,
    ) -> Result<tokio::sync::mpsc::Receiver<Result<String>>> {
        let (tx, rx) = tokio::sync::mpsc::channel(1);
        let response = self.chat(model, messages).await?;
        let _ = tx.send(Ok(response.message.content)).await;
        Ok(rx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    type MockServer = (String, Arc<Mutex<Vec<u8>>>, tokio::task::JoinHandle<()>);

    fn spawn_test_server(response_status: &str, response_body: &str) -> Result<MockServer> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let addr = listener.local_addr()?;
        let request_bytes = Arc::new(Mutex::new(Vec::new()));
        let request_bytes_clone = request_bytes.clone();
        let response_status = response_status.to_string();
        let response_body = response_body.to_string();

        let handle = tokio::task::spawn_blocking(move || {
            listener
                .set_nonblocking(true)
                .expect("listener should support nonblocking mode");
            let deadline = Instant::now() + Duration::from_secs(5);

            while Instant::now() < deadline {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        stream
                            .set_read_timeout(Some(Duration::from_millis(250)))
                            .expect("stream should support read timeout");
                        let mut buffer = [0_u8; 16384];
                        loop {
                            match stream.read(&mut buffer) {
                                Ok(0) => break,
                                Ok(read) => {
                                    request_bytes_clone
                                        .lock()
                                        .expect("request bytes lock poisoned")
                                        .extend_from_slice(&buffer[..read]);
                                    if read < buffer.len() {
                                        break;
                                    }
                                }
                                Err(err)
                                    if err.kind() == std::io::ErrorKind::WouldBlock
                                        || err.kind() == std::io::ErrorKind::TimedOut =>
                                {
                                    break;
                                }
                                Err(_) => break,
                            }
                        }

                        let response = format!(
                            "HTTP/1.1 {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                            response_status,
                            response_body.len(),
                            response_body
                        );
                        let _ = stream.write_all(response.as_bytes());
                        let _ = stream.flush();
                        return;
                    }
                    Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(25));
                    }
                    Err(_) => return,
                }
            }
        });

        Ok((format!("http://{}", addr), request_bytes, handle))
    }

    #[test]
    fn parses_model_listing_response() {
        let models = OpenClawProvider::parse_models_response(
            r#"{"data":[{"id":"openclaw:main","owned_by":"openclaw","created":1710000000}]}"#,
        )
        .expect("model response should parse");

        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, "openclaw:main");
        assert!(models[0].tags.iter().any(|tag| tag == "openclaw"));
    }

    #[test]
    fn derives_ghostbridge_headers_only_from_an_anchored_session() {
        let identity = SessionIdentity {
            session_id: "session-a".into(),
            principal_kind: Some("human".into()),
            wireguard_pubkey: "pubkey".into(),
            mutation_index: 1,
            genesis: Some("ab".repeat(32)),
            trace_id: "cd".repeat(16),
            schema_version: 3,
            active: true,
            expires_at: None,
            arrival_timestamp: 1,
            chain_head_at_arrival: "ef".repeat(32),
            catalog_hash_at_arrival: String::new(),
            head_timestamp_at_arrival: 0,
            sealed_id: None,
        };
        let (genesis, trace_id) =
            OpenClawProvider::ghostbridge_identity_headers(&identity).expect("session anchored");

        assert_eq!(genesis, "ab".repeat(32));
        assert_eq!(trace_id, "cd".repeat(16));
    }

    #[tokio::test]
    async fn list_models_falls_back_to_default_when_endpoint_fails() {
        let (base_url, _request, handle) =
            spawn_test_server("500 Internal Server Error", r#"{"error":"boom"}"#)
                .expect("test server should start");

        let provider = OpenClawProvider::new(Some(base_url), Some("opencode/agent".to_string()));
        let models = provider
            .list_models()
            .await
            .expect("list_models should succeed");
        handle.await.expect("server should finish");

        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, "opencode/agent");
    }

    #[tokio::test]
    async fn list_models_falls_back_to_default_when_endpoint_returns_non_json() {
        let (base_url, _request, handle) =
            spawn_test_server("200 OK", "<html>OpenClaw Control UI</html>")
                .expect("test server should start");

        let provider =
            OpenClawProvider::new(Some(base_url), Some("openclaw:gemini3-adc".to_string()));
        let models = provider
            .list_models()
            .await
            .expect("list_models should succeed");
        handle.await.expect("server should finish");

        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, "openclaw:gemini3-adc");
        assert!(models[0].tags.iter().any(|tag| tag == "agent-route"));
    }

    #[tokio::test]
    async fn chat_with_request_serializes_tools_and_parses_tool_calls() {
        let response_body = r#"{
            "choices":[
                {
                    "message":{
                        "role":"assistant",
                        "content":"",
                        "tool_calls":[
                            {
                                "id":"call_1",
                                "type":"function",
                                "function":{
                                    "name":"tool.echo",
                                    "arguments":"{\"message\":\"hello\"}"
                                }
                            }
                        ]
                    },
                    "finish_reason":"tool_calls"
                }
            ],
            "usage":{"prompt_tokens":10,"completion_tokens":5,"total_tokens":15}
        }"#;
        let (base_url, request_bytes, handle) =
            spawn_test_server("200 OK", response_body).expect("test server should start");

        let provider = OpenClawProvider::new(Some(base_url), Some("openclaw:main".to_string()));

        let request = ChatRequest::new(vec![ChatMessage::user("test tool call")])
            .with_tools(vec![crate::provider::ToolDefinition {
                name: "tool.echo".to_string(),
                description: "Echo a message".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "message": {"type": "string"}
                    },
                    "required": ["message"]
                }),
                schema_version: "1".to_string(),
                category: "test".to_string(),
                tags: vec![],
                namespace: "tool".to_string(),
            }])
            .with_tool_choice(crate::provider::ToolChoice::Required);

        let response = tokio::time::timeout(
            Duration::from_secs(5),
            provider.chat_with_request("", request),
        )
        .await
        .expect("chat request should not hang")
        .expect("chat_with_request should succeed");
        handle.await.expect("server should finish");

        let request_text = String::from_utf8(
            request_bytes
                .lock()
                .expect("request bytes lock poisoned")
                .clone(),
        )
        .expect("request should be valid utf-8");
        let request_text_lower = request_text.to_lowercase();

        assert!(!request_text_lower.contains("authorization:"));
        assert!(request_text.contains("\"tool_choice\":\"required\""));
        assert!(request_text.contains("\"name\":\"tool.echo\""));
        assert_eq!(response.provider, "openclaw");
        assert_eq!(
            response
                .tool_calls
                .as_ref()
                .expect("tool calls should exist")[0]
                .name,
            "tool.echo"
        );
    }
}
