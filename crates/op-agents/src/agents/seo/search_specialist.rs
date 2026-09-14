//! Search Specialist Agent

use crate::agents::base::{AgentTask, AgentTrait, TaskResult};
use crate::security::SecurityProfile;
use async_trait::async_trait;
use reqwest::Client;
use simd_json::prelude::*;
use simd_json::{json, OwnedValue as Value};

#[derive(Debug, Clone)]
struct WebResult {
    title: String,
    url: String,
    snippet: String,
    source: String,
}

pub struct SearchSpecialistAgent {
    agent_id: String,
    profile: SecurityProfile,
}

impl SearchSpecialistAgent {
    pub fn new(agent_id: String) -> Self {
        Self {
            profile: SecurityProfile::content_generation("search-specialist"),
            agent_id,
        }
    }

    fn analyze(&self, args: Option<&str>) -> Result<String, String> {
        let input = args.unwrap_or("").to_lowercase();
        let mut recommendations = Vec::new();

        if input.contains("technical") || input.contains("audit") {
            recommendations.push("Check crawlability with robots.txt and sitemap");
            recommendations.push("Analyze Core Web Vitals scores");
            recommendations.push("Fix broken links and redirect chains");
        }
        if input.contains("local") {
            recommendations.push("Optimize Google Business Profile");
            recommendations.push("Build local citations consistently");
            recommendations.push("Encourage and respond to reviews");
        }
        if recommendations.is_empty() {
            recommendations.push("Monitor search console for issues");
            recommendations.push("Build quality backlinks");
            recommendations.push("Create comprehensive topic clusters");
        }

        let result = json!({
            "analysis": { "input": args.unwrap_or(""), "recommendations": recommendations },
            "tools": ["Google Search Console", "Screaming Frog", "Ahrefs", "SEMrush"]
        });
        Ok(simd_json::to_string_pretty(&result).unwrap_or_default())
    }

    fn extract_query(task: &AgentTask) -> String {
        if let Some(raw) = task.args.as_deref() {
            if let Ok(parsed) = serde_json::from_str::<Value>(raw) {
                if let Some(query) = parsed.get("query").and_then(|v| v.as_str()) {
                    if !query.trim().is_empty() {
                        return query.trim().to_string();
                    }
                }
                if let Some(requested) = parsed.get("requested").and_then(|v| v.as_str()) {
                    if !requested.trim().is_empty() {
                        return requested.trim().to_string();
                    }
                }
                if let Some(object_type) = parsed.get("object_type").and_then(|v| v.as_str()) {
                    if !object_type.trim().is_empty() {
                        return object_type.trim().to_string();
                    }
                }
            }
            if !raw.trim().is_empty() {
                return raw.trim().to_string();
            }
        }
        "plugin required configuration fields".to_string()
    }

    async fn web_search(&self, query: &str, limit: usize) -> Result<Vec<WebResult>, String> {
        let client = Client::builder()
            .user_agent("op-dbus-search-specialist/1.0")
            .build()
            .map_err(|e| format!("build HTTP client failed: {}", e))?;

        if let Ok(results) = Self::brave_search(&client, query, limit).await {
            if !results.is_empty() {
                return Ok(results);
            }
        }

        Self::duckduckgo_search(&client, query, limit).await
    }

    async fn brave_search(
        client: &Client,
        query: &str,
        limit: usize,
    ) -> Result<Vec<WebResult>, String> {
        let api_key = std::env::var("BRAVE_SEARCH_API_KEY")
            .map_err(|_| "BRAVE_SEARCH_API_KEY not configured".to_string())?;
        let count = limit.clamp(1, 20);
        let response_text = client
            .get("https://api.search.brave.com/res/v1/web/search")
            .query(&[("q", query), ("count", &count.to_string())])
            .header("X-Subscription-Token", api_key)
            .send()
            .await
            .map_err(|e| format!("brave search request failed: {}", e))?
            .error_for_status()
            .map_err(|e| format!("brave search HTTP error: {}", e))?
            .text()
            .await
            .map_err(|e| format!("read brave response failed: {}", e))?;

        let mut bytes = response_text.into_bytes();
        let parsed = simd_json::to_owned_value(&mut bytes)
            .map_err(|e| format!("parse brave JSON failed: {}", e))?;
        let mut results = Vec::new();
        if let Some(items) = parsed
            .get("web")
            .and_then(|v| v.get("results"))
            .and_then(|v| v.as_array())
        {
            for item in items.iter().take(count) {
                let title = item
                    .get("title")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .trim()
                    .to_string();
                let url = item
                    .get("url")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .trim()
                    .to_string();
                let snippet = item
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .trim()
                    .to_string();
                if !url.is_empty() {
                    results.push(WebResult {
                        title,
                        url,
                        snippet,
                        source: "brave".to_string(),
                    });
                }
            }
        }
        Ok(results)
    }

    async fn duckduckgo_search(
        client: &Client,
        query: &str,
        limit: usize,
    ) -> Result<Vec<WebResult>, String> {
        let response_text = client
            .get("https://api.duckduckgo.com/")
            .query(&[
                ("q", query),
                ("format", "json"),
                ("no_html", "1"),
                ("skip_disambig", "1"),
            ])
            .send()
            .await
            .map_err(|e| format!("duckduckgo request failed: {}", e))?
            .error_for_status()
            .map_err(|e| format!("duckduckgo HTTP error: {}", e))?
            .text()
            .await
            .map_err(|e| format!("read duckduckgo response failed: {}", e))?;

        let mut bytes = response_text.into_bytes();
        let parsed = simd_json::to_owned_value(&mut bytes)
            .map_err(|e| format!("parse duckduckgo JSON failed: {}", e))?;
        let mut results = Vec::new();

        if let Some(url) = parsed.get("AbstractURL").and_then(|v| v.as_str()) {
            let snippet = parsed
                .get("AbstractText")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if !url.trim().is_empty() {
                results.push(WebResult {
                    title: parsed
                        .get("Heading")
                        .and_then(|v| v.as_str())
                        .unwrap_or(query)
                        .to_string(),
                    url: url.to_string(),
                    snippet,
                    source: "duckduckgo".to_string(),
                });
            }
        }

        if let Some(related) = parsed.get("RelatedTopics").and_then(|v| v.as_array()) {
            for topic in related {
                if results.len() >= limit {
                    break;
                }
                let topic_url = topic.get("FirstURL").and_then(|v| v.as_str()).unwrap_or("");
                let topic_text = topic.get("Text").and_then(|v| v.as_str()).unwrap_or("");
                if !topic_url.is_empty() || !topic_text.is_empty() {
                    results.push(WebResult {
                        title: topic_text.chars().take(120).collect(),
                        url: topic_url.to_string(),
                        snippet: topic_text.to_string(),
                        source: "duckduckgo".to_string(),
                    });
                }
            }
        }

        Ok(results)
    }

    fn derive_plugin_elements(results: &[WebResult]) -> Vec<String> {
        let mut fields = vec![
            "plugin_id".to_string(),
            "uuid".to_string(),
            "subid".to_string(),
            "dbus_path".to_string(),
            "schema_version".to_string(),
            "pending_human_review".to_string(),
            "review_reason".to_string(),
        ];

        let corpus = results
            .iter()
            .map(|r| format!("{} {}", r.title.to_lowercase(), r.snippet.to_lowercase()))
            .collect::<Vec<_>>()
            .join(" ");

        let candidates = [
            ("endpoint", "endpoint"),
            ("api key", "api_key_ref"),
            ("token", "token_ref"),
            ("credential", "credential_ref"),
            ("model", "model_id"),
            ("provider", "provider"),
            ("serial", "serial_number"),
            ("firmware", "firmware_version"),
            ("driver", "driver_name"),
            ("port", "port"),
            ("protocol", "protocol"),
            ("namespace", "namespace"),
            ("health", "health_status"),
            ("capability", "capabilities"),
        ];

        for (needle, field) in candidates {
            if corpus.contains(needle) && !fields.iter().any(|f| f == field) {
                fields.push(field.to_string());
            }
        }

        fields
    }

    async fn research_plugin_elements(&self, task: &AgentTask) -> Result<String, String> {
        let query = Self::extract_query(task);
        let web_results = self.web_search(&query, 8).await.unwrap_or_default();
        let recommended_fields = Self::derive_plugin_elements(&web_results);
        let pending_human_review = web_results.is_empty();
        let review_reason = if pending_human_review {
            "No web results available; create with requested info and require human review."
        } else {
            "Web results collected; still require human review before production activation."
        };

        let response = json!({
            "query": query,
            "web_results": web_results.iter().map(|r| json!({
                "title": r.title,
                "url": r.url,
                "snippet": r.snippet,
                "source": r.source,
            })).collect::<Vec<_>>(),
            "recommended_fields": recommended_fields,
            "pending_human_review": pending_human_review,
            "review_reason": review_reason,
        });
        Ok(simd_json::to_string_pretty(&response).unwrap_or_default())
    }
}

#[async_trait]
impl AgentTrait for SearchSpecialistAgent {
    fn agent_type(&self) -> &str {
        "search-specialist"
    }
    fn name(&self) -> &str {
        "Search Specialist"
    }
    fn description(&self) -> &str {
        "Technical SEO audits and search optimization."
    }
    fn operations(&self) -> Vec<String> {
        vec![
            "audit_site".to_string(),
            "fix_issues".to_string(),
            "analyze".to_string(),
            "web_research".to_string(),
            "research_plugin_elements".to_string(),
        ]
    }
    fn security_profile(&self) -> &SecurityProfile {
        &self.profile
    }
    fn get_status(&self) -> String {
        format!("Search Specialist agent {} is running", self.agent_id)
    }

    async fn execute(&self, task: AgentTask) -> Result<TaskResult, String> {
        if task.task_type != "search-specialist" {
            return Err(format!("Invalid task type: {}", task.task_type));
        }
        let outcome = match task.operation.as_str() {
            "web_research" | "research_plugin_elements" => {
                self.research_plugin_elements(&task).await
            }
            _ => self.analyze(task.args.as_deref()),
        };
        match outcome {
            Ok(data) => Ok(TaskResult::success(&task.operation, data)),
            Err(e) => Ok(TaskResult::failure(&task.operation, e)),
        }
    }
}
