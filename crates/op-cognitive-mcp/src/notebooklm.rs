//! NotebookLM MCP bridge for the cognitive server.
//!
//! Connects to the runit-supervised NotebookLM MCP provider over loopback
//! Streamable HTTP and re-exposes its tools through a local `ToolRegistry`.

use anyhow::Result;
use async_trait::async_trait;
use op_mcp::external_client::{
    ExternalMcpClient, ExternalMcpConfig, ExternalMcpTransport, ExternalTool,
};
use op_mcp::tool_registry::{BoxedTool, Tool, ToolRegistry};
use simd_json::OwnedValue as Value;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

const DEFAULT_NOTEBOOKLM_URL: &str = "http://127.0.0.1:3101/mcp";
const DEFAULT_NOTEBOOKLM_SERVER_NAME: &str = "notebooklm";

#[derive(Debug, Clone)]
struct NotebookLmConfig {
    enabled: bool,
    url: String,
    server_name: String,
    tool_prefix: Option<String>,
}

impl NotebookLmConfig {
    fn from_env() -> Self {
        Self {
            enabled: env_flag("COGNITIVE_MCP_NOTEBOOKLM_ENABLED", true),
            url: std::env::var("COGNITIVE_MCP_NOTEBOOKLM_URL")
                .unwrap_or_else(|_| DEFAULT_NOTEBOOKLM_URL.to_string()),
            server_name: std::env::var("COGNITIVE_MCP_NOTEBOOKLM_SERVER_NAME")
                .unwrap_or_else(|_| DEFAULT_NOTEBOOKLM_SERVER_NAME.to_string()),
            tool_prefix: std::env::var("COGNITIVE_MCP_NOTEBOOKLM_TOOL_PREFIX")
                .ok()
                .filter(|value| !value.trim().is_empty()),
        }
    }

    fn external_config(&self) -> ExternalMcpConfig {
        ExternalMcpConfig {
            name: self.server_name.clone(),
            command: String::new(),
            args: vec![],
            transport: ExternalMcpTransport::StreamableHttp,
            url: Some(self.url.clone()),
            env: HashMap::new(),
            api_key: None,
            api_key_env: "API_KEY".to_string(),
            auth_method: op_mcp::external_client::AuthMethod::None,
            headers: HashMap::new(),
        }
    }

    fn published_tool_name(&self, upstream_name: &str) -> String {
        let raw_name = upstream_name
            .split_once(':')
            .map(|(_, name)| name)
            .unwrap_or(upstream_name);

        let mut normalized = String::with_capacity(raw_name.len());
        let mut last_was_underscore = false;

        for ch in raw_name.chars() {
            let ch = ch.to_ascii_lowercase();
            if ch.is_ascii_alphanumeric() {
                normalized.push(ch);
                last_was_underscore = false;
            } else if !last_was_underscore {
                normalized.push('_');
                last_was_underscore = true;
            }
        }

        let normalized = normalized.trim_matches('_');
        let tool_name = if normalized.is_empty() {
            raw_name.to_string()
        } else {
            normalized.to_string()
        };

        if let Some(prefix) = &self.tool_prefix {
            let prefix = prefix.trim_matches('_');
            if prefix.is_empty() {
                tool_name
            } else {
                format!("{prefix}_{tool_name}")
            }
        } else {
            tool_name
        }
    }
}

pub async fn register_notebooklm_tools(registry: &ToolRegistry) -> Result<usize> {
    let config = NotebookLmConfig::from_env();
    if !config.enabled {
        tracing::info!("NotebookLM MCP bridge disabled");
        return Ok(0);
    }

    let mut client = ExternalMcpClient::new(config.external_config());
    if let Err(error) = client.start().await {
        tracing::warn!(
            error = %error,
            url = %config.url,
            "supervised NotebookLM MCP provider is unavailable; continuing without NotebookLM tools"
        );
        return Ok(0);
    }

    let upstream_tools = client.get_tools().await;
    if upstream_tools.is_empty() {
        tracing::warn!("NotebookLM MCP sidecar started but returned no tools");
        return Ok(0);
    }

    let shared_client = Arc::new(Mutex::new(client));
    let mut registered = 0usize;

    for tool in upstream_tools {
        let published_name = config.published_tool_name(&tool.name);
        let wrapper = NotebookLmTool::new(shared_client.clone(), tool, published_name);
        registry.register(Arc::new(wrapper) as BoxedTool).await?;
        registered += 1;
    }

    tracing::info!(registered, "Registered NotebookLM MCP tools");
    Ok(registered)
}

struct NotebookLmTool {
    client: Arc<Mutex<ExternalMcpClient>>,
    upstream_name: String,
    name: String,
    description: String,
    input_schema: Value,
}

impl NotebookLmTool {
    fn new(client: Arc<Mutex<ExternalMcpClient>>, tool: ExternalTool, name: String) -> Self {
        let upstream_name = tool
            .name
            .split_once(':')
            .map(|(_, raw_name)| raw_name.to_string())
            .unwrap_or_else(|| tool.name.clone());

        Self {
            client,
            upstream_name,
            name,
            description: format!("NotebookLM MCP: {}", tool.description),
            input_schema: tool.input_schema,
        }
    }
}

#[async_trait]
impl Tool for NotebookLmTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> Value {
        self.input_schema.clone()
    }

    fn category(&self) -> &str {
        "cognitive"
    }

    fn namespace(&self) -> &str {
        "notebooklm"
    }

    fn tags(&self) -> Vec<String> {
        vec![
            "notebooklm".to_string(),
            "rag".to_string(),
            "research".to_string(),
        ]
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        // Robustness: retry with exponential backoff + session rotation
        // per Operation_Dbus_Robustness_Recommendations.md
        const MAX_RETRIES: u32 = 3;
        const BASE_DELAY_MS: u64 = 100;

        let mut last_error = None;
        for attempt in 0..=MAX_RETRIES {
            if attempt > 0 {
                let delay = BASE_DELAY_MS * (1 << (attempt - 1));
                tracing::warn!(
                    tool = %self.name,
                    attempt,
                    delay_ms = delay,
                    "Retrying NotebookLM tool call after backoff"
                );
                tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
            }

            let result = {
                let mut client = self.client.lock().await;
                client.call_tool(&self.upstream_name, input.clone()).await
            };

            match result {
                Ok(value) => return Ok(value),
                Err(e) => {
                    tracing::warn!(
                        tool = %self.name,
                        attempt,
                        error = %e,
                        "NotebookLM tool call failed"
                    );
                    last_error = Some(e);
                }
            }
        }

        Err(last_error.unwrap_or_else(|| {
            anyhow::anyhow!(
                "NotebookLM tool '{}' failed after {} retries",
                self.name,
                MAX_RETRIES
            )
        }))
    }
}

fn env_flag(name: &str, default: bool) -> bool {
    std::env::var(name)
        .ok()
        .map(|value| {
            matches!(
                value.to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::{NotebookLmConfig, DEFAULT_NOTEBOOKLM_URL};

    #[test]
    fn should_normalize_notebooklm_tool_names() {
        let config = NotebookLmConfig {
            enabled: true,
            url: DEFAULT_NOTEBOOKLM_URL.to_string(),
            server_name: "notebooklm".to_string(),
            tool_prefix: None,
        };

        assert_eq!(
            config.published_tool_name("notebooklm:create-notebook"),
            "create_notebook"
        );
        assert_eq!(config.published_tool_name("ask question"), "ask_question");
    }

    #[test]
    fn should_prefix_zero_claw_notebooklm_tool_names() {
        let config = NotebookLmConfig {
            enabled: true,
            url: DEFAULT_NOTEBOOKLM_URL.to_string(),
            server_name: "zeroclaw-model-transcripts".to_string(),
            tool_prefix: Some("zeroclaw_model_transcript".to_string()),
        };

        assert_eq!(
            config.published_tool_name("notebooklm:ask-question"),
            "zeroclaw_model_transcript_ask_question"
        );
    }
}
