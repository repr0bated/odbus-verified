//! Context-Aware Code Search Integration
//!
//! Automatically injects relevant code from indexed repositories
//! into tool execution context for smart suggestions and debugging.

use anyhow::Result;
use serde_json;
use simd_json::prelude::ValueAsContainer;
use simd_json::OwnedValue as Value;
use tracing::debug;

/// Code context extracted from indexed repos
#[derive(Debug, Clone, Default)]
pub struct CodeContext {
    pub relevant_code: Vec<CodeSnippet>,
    pub suggestions: Vec<String>,
    pub debugging_hints: Vec<String>,
}

impl CodeContext {
    pub fn is_empty(&self) -> bool {
        self.relevant_code.is_empty()
            && self.suggestions.is_empty()
            && self.debugging_hints.is_empty()
    }

    pub fn to_json(&self) -> Value {
        use simd_json::ObjectHasher;

        // Build relevant_code array
        let relevant_code = Value::Array(
            self.relevant_code
                .iter()
                .map(|s| {
                    let mut obj = simd_json::owned::Object::with_hasher(ObjectHasher::default());
                    obj.insert("file".into(), Value::String(s.file.clone()));
                    obj.insert(
                        "function".into(),
                        Value::String(s.function.clone().unwrap_or_default()),
                    );
                    obj.insert("language".into(), Value::String(s.language.clone()));
                    obj.insert("code".into(), Value::String(s.code.clone()));
                    obj.insert(
                        "similarity".into(),
                        Value::Static(simd_json::prelude::StaticNode::F64(s.similarity)),
                    );
                    Value::Object(Box::new(obj))
                })
                .collect(),
        );

        // Build the main object
        let mut result = simd_json::owned::Object::with_hasher(ObjectHasher::default());
        result.insert("relevant_code".into(), relevant_code);
        result.insert(
            "suggestions".into(),
            Value::Array(
                self.suggestions
                    .iter()
                    .map(|s| Value::String(s.clone()))
                    .collect(),
            ),
        );
        result.insert(
            "debugging_hints".into(),
            Value::Array(
                self.debugging_hints
                    .iter()
                    .map(|h| Value::String(h.clone()))
                    .collect(),
            ),
        );

        Value::Object(Box::new(result))
    }
}

#[derive(Debug, Clone)]
pub struct CodeSnippet {
    pub file: String,
    pub function: Option<String>,
    pub language: String,
    pub code: String,
    pub similarity: f64,
}

/// Inject code context into tool execution
pub async fn inject_code_context(
    tool_name: &str,
    arguments: &Value,
    current_file: Option<&str>,
) -> CodeContext {
    let mut context = CodeContext::default();

    // Build search query from tool context
    let query = build_context_query(tool_name, arguments, current_file);
    if query.is_empty() {
        return context;
    }

    // Search indexed code
    if let Ok(results) = search_code(&query, current_file, 5).await {
        context.relevant_code = results;
    }

    // Generate suggestions based on tool type
    context.suggestions = generate_suggestions(tool_name, &context.relevant_code);

    // Generate debugging hints for mutation tools
    if is_mutation_tool(tool_name) {
        context.debugging_hints = generate_debugging_hints(tool_name, &context.relevant_code);
    }

    debug!(
        "Injected {} code snippets for tool {}",
        context.relevant_code.len(),
        tool_name
    );
    context
}

fn build_context_query(tool_name: &str, arguments: &Value, current_file: Option<&str>) -> String {
    let mut parts: Vec<String> = Vec::new();

    // Tool category gives context
    if tool_name.contains("file") || tool_name.contains("write") {
        parts.push("file operations".to_string());
    } else if tool_name.contains("network") || tool_name.contains("ovs") {
        parts.push("network configuration".to_string());
    } else if tool_name.contains("service") || tool_name.contains("systemd") {
        parts.push("service management".to_string());
    } else if tool_name.contains("shell") || tool_name.contains("exec") {
        parts.push("shell scripting".to_string());
    }

    // Current file path
    if let Some(f) = current_file {
        parts.push(f.to_string());
    }

    // Arguments hint at intent
    if let Some(obj) = arguments.as_object() {
        for (k, v) in obj {
            parts.push(format!("{}: {}", k, v));
        }
    }

    parts.join(" ")
}

fn is_mutation_tool(name: &str) -> bool {
    name.contains("create")
        || name.contains("delete")
        || name.contains("update")
        || name.contains("modify")
        || name.contains("write")
        || name.contains("apply")
}

async fn search_code(query: &str, _repo: Option<&str>, limit: usize) -> Result<Vec<CodeSnippet>> {
    // Call the existing code search via HTTP to Qdrant
    let client = reqwest::Client::new();
    let qdrant_url =
        std::env::var("QDRANT_URL").unwrap_or_else(|_| "http://127.0.0.1:6333".to_string());

    // Embed query (simplified - would use HF API in production)
    let embedding = embed_text(query).await?;

    // Search Qdrant
    let url = format!("{}/collections/code_chunks/points/query", qdrant_url);
    let response = client
        .post(&url)
        .json(&serde_json::json!({
            "query": embedding,
            "limit": limit,
            "with_payload": true
        }))
        .send()
        .await
        .ok();

    let mut snippets = Vec::new();
    if let Some(r) = response {
        if let Ok(json) = r.json::<serde_json::Value>().await {
            if let Some(results) = json.pointer("/result/points").and_then(|p| p.as_array()) {
                for point in results {
                    if let Some(payload) = point.pointer("/payload") {
                        snippets.push(CodeSnippet {
                            file: payload
                                .pointer("/file")
                                .and_then(|f| f.as_str())
                                .unwrap_or("")
                                .to_string(),
                            function: payload
                                .pointer("/function")
                                .and_then(|f| f.as_str())
                                .map(|s| s.to_string()),
                            language: payload
                                .pointer("/language")
                                .and_then(|l| l.as_str())
                                .unwrap_or("")
                                .to_string(),
                            code: payload
                                .pointer("/code")
                                .and_then(|c| c.as_str())
                                .unwrap_or("")
                                .to_string(),
                            similarity: point
                                .pointer("/score")
                                .and_then(|s| s.as_f64())
                                .unwrap_or(0.0),
                        });
                    }
                }
            }
        }
    }

    Ok(snippets)
}

async fn embed_text(_text: &str) -> Result<Vec<f64>> {
    // Simplified - in production use HF API
    Ok(vec![0.0; 384])
}

fn generate_suggestions(_tool_name: &str, code: &[CodeSnippet]) -> Vec<String> {
    let mut suggestions = Vec::new();

    if let Some(snippet) = code.first() {
        suggestions.push(format!(
            "Similar pattern found in {}: {}",
            snippet.file,
            snippet.function.clone().unwrap_or_default()
        ));
    }

    suggestions
}

fn generate_debugging_hints(tool_name: &str, _code: &[CodeSnippet]) -> Vec<String> {
    let mut hints = Vec::new();

    if tool_name.contains("delete") || tool_name.contains("remove") {
        hints.push("Consider checking dependencies before deletion".to_string());
        hints.push("Verify no services depend on this resource".to_string());
    }

    if tool_name.contains("network") || tool_name.contains("ovs") {
        hints.push("Check OVS service is running before modifications".to_string());
        hints.push("Consider backing up current configuration".to_string());
    }

    hints
}

/// Add code context to tool arguments
pub fn augment_arguments_with_context(mut arguments: Value, context: &CodeContext) -> Value {
    if !context.is_empty() {
        arguments["_code_context"] = context.to_json();
    }
    arguments
}
