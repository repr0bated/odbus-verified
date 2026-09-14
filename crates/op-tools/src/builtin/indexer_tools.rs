use anyhow::{anyhow, Result};
use async_trait::async_trait;
use simd_json::{json, prelude::*, OwnedValue as Value};
use std::{process::Command, sync::Arc};
use tracing::{error, info};

use crate::tool::{SecurityLevel, Tool};

pub struct IndexerSearchTool;

#[async_trait]
impl Tool for IndexerSearchTool {
    fn name(&self) -> &str {
        "indexer_search"
    }

    fn description(&self) -> &str {
        "Searches the OpenClaw code index semantically for relevant code snippets."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "The semantic query string to search for."
                },
                "repo": {
                    "type": "string",
                    "description": "Optional: Filter search results by repository name."
                },
                "language": {
                    "type": "string",
                    "description": "Optional: Filter search results by programming language."
                },
                "limit": {
                    "type": "number", // Assuming number for now, can be changed to integer if needed
                    "description": "Optional: Maximum number of results to return (default: 5)."
                }
            },
            "required": ["query"]
        })
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        info!("Executing openclaw_search with input: {:?}", input);

        let query = input.get("query").and_then(Value::as_str).ok_or_else(|| anyhow!("Missing 'query' argument"))?;

        let mut command = Command::new("bash");
        command.arg("openclaw-indexer/run.sh").arg("search").arg(query);

        if let Some(repo) = input.get("repo").and_then(Value::as_str) {
            command.arg("--repo").arg(repo);
        }
        if let Some(language) = input.get("language").and_then(Value::as_str) {
            command.arg("--language").arg(language);
        }
        if let Some(limit) = input.get("limit").and_then(Value::as_u64) {
            command.arg("--limit").arg(limit.to_string());
        }

        let output = command.output().map_err(|e| anyhow!("Failed to execute command: {}", e))?;

        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            info!("OpenClaw search successful.");

            let mut results = Vec::new();
            let mut current_result: Option<Value> = None;

            for line in stdout.lines() {
                if line.starts_with("#") {
                    // New result block
                    if let Some(res) = current_result.take() {
                        results.push(res);
                    }
                    let parts: Vec<&str> = line.splitn(3, ' ').collect();
                    if parts.len() >= 3 {
                        let score_str = parts[1].trim_start_matches("(score: ").trim_end_matches(")");
                        if let Ok(score) = score_str.parse::<f64>() {
                            current_result = Some(json!({
                                "score": score,
                                "name": parts[2].trim(),
                            }));
                        }
                    }
                } else if line.trim().starts_with("operation-dbus/") {
                    // Location line
                    if let Some(res) = current_result.as_mut() {
                        let loc_parts: Vec<&str> = line.trim().split(':').collect();
                        if loc_parts.len() >= 4 {
                            res["repo"] = json!(loc_parts[0]);
                            res["file_path"] = json!(loc_parts[1]);
                            let line_range: Vec<&str> = loc_parts[2].split('-').collect();
                            if line_range.len() == 2 {
                                if let (Ok(start), Ok(end)) = (line_range[0].parse::<u64>(), line_range[1].parse::<u64>()) {
                                    res["line_start"] = json!(start);
                                    res["line_end"] = json!(end);
                                }
                            }
                        }
                    }
                } else if line.trim().starts_with("pub ") || line.trim().starts_with("impl ") || line.trim().starts_with("```") || line.trim().starts_with("struct ") {
                    // Content preview - simple heuristic
                    if let Some(res) = current_result.as_mut() {
                        let current_content = res["content_preview"].as_str().unwrap_or("").to_string();
                        res["content_preview"] = json!(format!("{}{}\n", current_content, line.trim()));
                    }
                }
            }
            if let Some(res) = current_result.take() {
                results.push(res);
            }

            Ok(json!(results))
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            error!("OpenClaw search failed: {}", stderr);
            Err(anyhow!("OpenClaw search failed: {}", stderr))
        }
    }

    fn security_level(&self) -> SecurityLevel {
        SecurityLevel::ReadOnly
    }

    fn category(&self) -> &str {
        "code_search"
    }

    fn tags(&self) -> Vec<String> {
        vec!["openclaw".to_string(), "indexer".to_string(), "code".to_string(), "semantic_search".to_string()]
    }
}

pub fn create_indexer_tools() -> Vec<Arc<dyn Tool>> {
    vec![
        Arc::new(IndexerSearchTool),
    ]
}
