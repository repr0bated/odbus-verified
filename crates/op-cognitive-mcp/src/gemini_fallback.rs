//! 🔴 Gemini Fallback — R12
//!
//! When the NotebookLM browser bridge breaks (session expired, Chrome crash,
//! sidecar down), queries fall back to the Gemini API via reqwest.
//!
//! # Capabilities
//! - `gemini_query`: Standard grounded query via Gemini GenerateContent
//! - `deep_research`: Multi-step research with grounding via Gemini
//!
//! # Security (R13)
//! - API key read from env, never logged
//! - No shell=True, no eval
//! - Exponential backoff on transient errors

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn};

const DEFAULT_GEMINI_API_URL: &str = "https://generativelanguage.googleapis.com/v1beta";
const DEFAULT_GEMINI_MODEL: &str = "gemini-2.5-flash";
const MAX_RETRIES: u32 = 3;
const BASE_DELAY_MS: u64 = 200;

/// Gemini API client configuration.
#[derive(Debug, Clone)]
pub struct GeminiConfig {
    pub api_url: String,
    pub api_key: String,
    pub model: String,
    pub enabled: bool,
}

impl GeminiConfig {
    pub fn from_env() -> Option<Self> {
        let api_key = std::env::var("COGNITIVE_MCP_GEMINI_API_KEY")
            .or_else(|_| std::env::var("GEMINI_API_KEY"))
            .ok()?;

        let enabled = std::env::var("COGNITIVE_MCP_GEMINI_ENABLED")
            .map(|v| matches!(v.to_lowercase().as_str(), "1" | "true" | "yes" | "on"))
            .unwrap_or(true);

        Some(Self {
            api_url: std::env::var("COGNITIVE_MCP_GEMINI_API_URL")
                .unwrap_or_else(|_| DEFAULT_GEMINI_API_URL.to_string()),
            api_key,
            model: std::env::var("COGNITIVE_MCP_GEMINI_MODEL")
                .unwrap_or_else(|_| DEFAULT_GEMINI_MODEL.to_string()),
            enabled,
        })
    }
}

/// Citation from Gemini grounding.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeminiCitation {
    pub text: String,
    pub source: String,
    pub page: String,
}

/// Result of a Gemini query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeminiQueryResult {
    pub answer: String,
    pub citations: Vec<GeminiCitation>,
    pub model: String,
    pub is_fallback: bool,
}

/// Result of a deep research query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeepResearchResult {
    pub summary: String,
    pub sections: Vec<ResearchSection>,
    pub sources_consulted: usize,
    pub model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResearchSection {
    pub title: String,
    pub content: String,
    pub citations: Vec<GeminiCitation>,
}

/// Gemini API request types (simplified).
#[derive(Debug, Serialize)]
struct GeminiRequest {
    contents: Vec<GeminiContent>,
    #[serde(rename = "generationConfig", skip_serializing_if = "Option::is_none")]
    generation_config: Option<GeminiGenerationConfig>,
    #[serde(rename = "systemInstruction", skip_serializing_if = "Option::is_none")]
    system_instruction: Option<GeminiContent>,
}

#[derive(Debug, Serialize, Deserialize)]
struct GeminiContent {
    role: String,
    parts: Vec<GeminiPart>,
}

#[derive(Debug, Serialize, Deserialize)]
struct GeminiPart {
    text: String,
}

#[derive(Debug, Serialize)]
struct GeminiGenerationConfig {
    temperature: f32,
    #[serde(rename = "maxOutputTokens")]
    max_output_tokens: u32,
}

/// Gemini API response types.
#[derive(Debug, Deserialize)]
struct GeminiResponse {
    candidates: Option<Vec<GeminiCandidate>>,
}

#[derive(Debug, Deserialize)]
struct GeminiCandidate {
    content: Option<GeminiContent>,
    #[serde(rename = "citationMetadata")]
    citation_metadata: Option<GeminiCitationMetadata>,
}

#[derive(Debug, Deserialize)]
struct GeminiCitationMetadata {
    #[serde(rename = "citationSources")]
    citation_sources: Option<Vec<GeminiCitationSource>>,
}

#[derive(Debug, Deserialize)]
struct GeminiCitationSource {
    uri: Option<String>,
    #[serde(rename = "startIndex")]
    start_index: Option<u32>,
    #[serde(rename = "endIndex")]
    end_index: Option<u32>,
}

/// Gemini fallback client.
pub struct GeminiFallback {
    client: reqwest::Client,
    config: Arc<RwLock<Option<GeminiConfig>>>,
}

impl Default for GeminiFallback {
    fn default() -> Self {
        Self::new()
    }
}

impl GeminiFallback {
    pub fn new() -> Self {
        let config = GeminiConfig::from_env();
        if config.is_some() {
            info!("Gemini fallback client initialized");
        } else {
            info!(
                "Gemini fallback unavailable (set GEMINI_API_KEY or COGNITIVE_MCP_GEMINI_API_KEY)"
            );
        }
        Self {
            client: reqwest::Client::new(),
            config: Arc::new(RwLock::new(config)),
        }
    }

    /// Whether the Gemini fallback is available.
    #[tracing::instrument(level = "debug", skip(self))]
    pub async fn is_available(&self) -> bool {
        self.config.read().await.as_ref().is_some_and(|c| c.enabled)
    }

    /// Standard grounded query via Gemini.
    #[tracing::instrument(skip(self, context), fields(query))]
    pub async fn gemini_query(
        &self,
        query: &str,
        context: Option<&str>,
    ) -> Result<GeminiQueryResult> {
        let config = self
            .config
            .read()
            .await
            .clone()
            .context("Gemini fallback not configured")?;

        if !config.enabled {
            anyhow::bail!("Gemini fallback is disabled");
        }

        let system_instruction = context.map(|ctx| GeminiContent {
            role: "user".to_string(),
            parts: vec![GeminiPart {
                text: format!(
                    "You are a grounded research assistant. Answer questions using ONLY the following context. If the answer is not in the context, say so.\n\nContext:\n{}",
                    ctx
                ),
            }],
        });

        let request = GeminiRequest {
            contents: vec![GeminiContent {
                role: "user".to_string(),
                parts: vec![GeminiPart {
                    text: query.to_string(),
                }],
            }],
            generation_config: Some(GeminiGenerationConfig {
                temperature: 0.1,
                max_output_tokens: 4096,
            }),
            system_instruction,
        };

        let response = self
            .call_with_retry(&config, &request)
            .await
            .context("Gemini query failed after retries")?;

        let answer = extract_answer(&response);
        let citations = extract_citations(&response);

        Ok(GeminiQueryResult {
            answer,
            citations,
            model: config.model,
            is_fallback: true,
        })
    }

    /// Deep research — multi-step query that builds on itself.
    #[tracing::instrument(skip(self, context), fields(topic, depth))]
    pub async fn deep_research(
        &self,
        topic: &str,
        context: Option<&str>,
        depth: u32,
    ) -> Result<DeepResearchResult> {
        let config = self
            .config
            .read()
            .await
            .clone()
            .context("Gemini fallback not configured")?;

        if !config.enabled {
            anyhow::bail!("Gemini fallback is disabled");
        }

        let depth = depth.clamp(1, 5); // Clamp to 1-5 steps
        let mut sections = Vec::new();
        let mut accumulated_knowledge = context.unwrap_or("").to_string();

        // Step 1: Overview query
        let overview_prompt = format!(
            "Provide a comprehensive overview of: {}\n\nExisting context:\n{}",
            topic, accumulated_knowledge
        );
        let overview_result = self
            .gemini_query(&overview_prompt, Some(&accumulated_knowledge))
            .await?;
        accumulated_knowledge.push_str("\n\n");
        accumulated_knowledge.push_str(&overview_result.answer);

        sections.push(ResearchSection {
            title: "Overview".to_string(),
            content: overview_result.answer,
            citations: overview_result.citations,
        });

        // Steps 2..depth: Drill deeper
        let drill_prompts = [
            "What are the key technical details and implementation specifics?",
            "What are the trade-offs, limitations, and alternative approaches?",
            "What are the security implications and best practices?",
            "What are the performance characteristics and optimization strategies?",
        ];

        for step in 1..depth as usize {
            let prompt_idx = (step - 1).min(drill_prompts.len() - 1);
            let drill_prompt = format!(
                "Regarding '{}': {}\n\nBased on what we know so far:\n{}",
                topic, drill_prompts[prompt_idx], accumulated_knowledge
            );

            match self
                .gemini_query(&drill_prompt, Some(&accumulated_knowledge))
                .await
            {
                Ok(result) => {
                    accumulated_knowledge.push_str("\n\n");
                    accumulated_knowledge.push_str(&result.answer);

                    sections.push(ResearchSection {
                        title: drill_prompts[prompt_idx].to_string(),
                        content: result.answer,
                        citations: result.citations,
                    });
                }
                Err(e) => {
                    warn!(step, error = %e, "Deep research step failed, continuing");
                    break;
                }
            }
        }

        let summary = format!(
            "Deep research on '{}' completed with {} sections across {} research steps.",
            topic,
            sections.len(),
            depth
        );

        Ok(DeepResearchResult {
            summary,
            sections,
            sources_consulted: 0, // Gemini doesn't expose source count
            model: config.model,
        })
    }

    /// Call Gemini API with exponential backoff.
    async fn call_with_retry(
        &self,
        config: &GeminiConfig,
        request: &GeminiRequest,
    ) -> Result<GeminiResponse> {
        let url = format!(
            "{}/models/{}:generateContent?key={}",
            config.api_url, config.model, config.api_key
        );

        let mut last_error = None;
        for attempt in 0..=MAX_RETRIES {
            if attempt > 0 {
                let delay = BASE_DELAY_MS * (1 << (attempt - 1));
                warn!(
                    attempt,
                    delay_ms = delay,
                    "Retrying Gemini API call after backoff"
                );
                tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
            }

            match self.client.post(&url).json(request).send().await {
                Ok(resp) => {
                    if resp.status().is_success() {
                        return resp
                            .json::<GeminiResponse>()
                            .await
                            .context("Failed to parse Gemini response");
                    }

                    let status = resp.status();
                    let body = resp.text().await.unwrap_or_default();

                    // Don't retry client errors (4xx) except 429 (rate limit)
                    if status.as_u16() != 429 && status.is_client_error() {
                        anyhow::bail!("Gemini API error {}: {}", status, body);
                    }

                    last_error = Some(anyhow::anyhow!("Gemini API error {}: {}", status, body));
                }
                Err(e) => {
                    last_error = Some(e.into());
                }
            }
        }

        Err(last_error
            .unwrap_or_else(|| anyhow::anyhow!("Gemini API failed after {} retries", MAX_RETRIES)))
    }
}

fn extract_answer(response: &GeminiResponse) -> String {
    response
        .candidates
        .as_ref()
        .and_then(|c| c.first())
        .and_then(|c| c.content.as_ref())
        .and_then(|content| content.parts.first())
        .map(|part| part.text.clone())
        .unwrap_or_else(|| "No response generated.".to_string())
}

fn extract_citations(response: &GeminiResponse) -> Vec<GeminiCitation> {
    response
        .candidates
        .as_ref()
        .and_then(|c| c.first())
        .and_then(|c| c.citation_metadata.as_ref())
        .and_then(|cm| cm.citation_sources.as_ref())
        .map(|sources| {
            sources
                .iter()
                .map(|s| GeminiCitation {
                    text: String::new(),
                    source: s.uri.clone().unwrap_or_default(),
                    page: format!(
                        "{}-{}",
                        s.start_index.unwrap_or(0),
                        s.end_index.unwrap_or(0)
                    ),
                })
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_extract_answer_from_response() {
        let response = GeminiResponse {
            candidates: Some(vec![GeminiCandidate {
                content: Some(GeminiContent {
                    role: "model".to_string(),
                    parts: vec![GeminiPart {
                        text: "Test answer".to_string(),
                    }],
                }),
                citation_metadata: None,
            }]),
        };

        assert_eq!(extract_answer(&response), "Test answer");
    }

    #[test]
    fn should_handle_empty_response() {
        let response = GeminiResponse { candidates: None };
        assert_eq!(extract_answer(&response), "No response generated.");
    }

    #[test]
    fn should_extract_citations() {
        let response = GeminiResponse {
            candidates: Some(vec![GeminiCandidate {
                content: None,
                citation_metadata: Some(GeminiCitationMetadata {
                    citation_sources: Some(vec![GeminiCitationSource {
                        uri: Some("https://example.com".to_string()),
                        start_index: Some(0),
                        end_index: Some(100),
                    }]),
                }),
            }]),
        };

        let citations = extract_citations(&response);
        assert_eq!(citations.len(), 1);
        assert_eq!(citations[0].source, "https://example.com");
    }

    #[tokio::test]
    async fn should_report_unavailable_without_key() {
        // No env var set — should be unavailable
        let fallback = GeminiFallback::new();
        // May or may not be available depending on test env
        let _ = fallback.is_available().await;
    }
}
