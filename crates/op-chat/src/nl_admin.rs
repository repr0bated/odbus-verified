//! Natural Language Server Administration
//!
//! This is the CORE module that enables natural language server administration.
//! It coordinates between:
//! - LLM (understands user intent)
//! - Tool Registry (executes operations)
//! - Response formatting (user-friendly output)
//!
//! The key insight: We instruct the LLM via system prompt to ALWAYS use tools,
//! then parse and execute those tool calls.

use anyhow::{Context, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};
use simd_json::prelude::*;
use simd_json::OwnedValue as Value;
use std::sync::Arc;
use tracing::{debug, error, info, warn};

use op_llm::provider::{
    ChatMessage, ChatRequest, ChatResponse, LlmProvider, ToolChoice, ToolDefinition,
};
use op_tools::ToolRegistry;

/// Extracted tool call from LLM response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedToolCall {
    pub name: String,
    pub arguments: Value,
    pub source: ToolCallSource,
}

/// Where the tool call was extracted from
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ToolCallSource {
    /// Native tool_calls from LLM API
    Native,
    /// Parsed from <tool_call> tags in text
    XmlTags,
    /// Parsed from ```tool format in text
    CodeBlock,
    /// Parsed from JSON in text
    JsonInText,
}

/// Result of natural language admin operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NLAdminResult {
    /// User-friendly response message
    pub message: String,
    /// Whether operation was successful
    pub success: bool,
    /// Tools that were executed
    pub tools_executed: Vec<String>,
    /// Detailed results from each tool
    pub tool_results: Vec<ToolExecutionResult>,
    /// Raw LLM response (for debugging)
    pub llm_response: Option<String>,
}

/// Result of a single tool execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolExecutionResult {
    pub tool_name: String,
    pub success: bool,
    pub result: Option<Value>,
    pub error: Option<String>,
}

/// Tool call parser - extracts tool calls from LLM responses
pub struct ToolCallParser {
    /// Regex for <tool_call>name(args)</tool_call> format
    xml_tag_regex: Regex,
    /// Regex for ```tool\nname(args)\n``` format
    code_block_regex: Regex,
    /// Regex for function call format: tool_name({"arg": "value"})
    function_call_regex: Regex,
}

impl Default for ToolCallParser {
    fn default() -> Self {
        Self::new()
    }
}

impl ToolCallParser {
    pub fn new() -> Self {
        Self {
            // Matches: <tool_call>tool_name({"arg": "value"})</tool_call>
            // Also handles nested JSON with newlines using (?s) for DOTALL mode
            xml_tag_regex: Regex::new(
                r"(?s)<tool_call>\s*([a-zA-Z_][a-zA-Z0-9_]*)\s*\((.*?)\)\s*</tool_call>",
            )
            .expect("hardcoded regex pattern is valid"),
            // Matches: ```tool\ntool_name({"arg": "value"})\n```
            // Also handles ```tool_code format
            code_block_regex: Regex::new(r"(?s)```(?:tool|tool_code)\s*\n(.+?)\n```")
                .expect("hardcoded regex pattern is valid"),
            // Matches: tool_name({"arg": "value"}) or tool_name({multi-line json})
            function_call_regex: Regex::new(
                r"(?s)\b([a-zA-Z_][a-zA-Z0-9_]*)\s*\(\s*(\{.*?\})\s*\)",
            )
            .expect("hardcoded regex pattern is valid"),
        }
    }

    /// Extract tool calls from LLM response
    pub fn extract_tool_calls(
        &self,
        response: &ChatResponse,
        available_tools: &[String],
    ) -> Vec<ExtractedToolCall> {
        let mut calls = Vec::new();

        // 1. Check native tool_calls first (best case)
        if let Some(ref tool_calls) = response.tool_calls {
            for tc in tool_calls {
                calls.push(ExtractedToolCall {
                    name: tc.name.clone(),
                    arguments: tc.arguments.clone(),
                    source: ToolCallSource::Native,
                });
            }
        }

        // If we got native calls, use those
        if !calls.is_empty() {
            return calls;
        }

        // 2. Parse from text content
        let content = &response.message.content;

        // Try XML tags: <tool_call>name(args)</tool_call>
        calls.extend(self.extract_from_xml_tags(content, available_tools));

        if !calls.is_empty() {
            return calls;
        }

        // Try code blocks: ```tool\n...\n``` or ```tool_code\n...\n```
        // The content inside might be xml tags OR direct function calls
        for cap in self.code_block_regex.captures_iter(content) {
            if let Some(block_content) = cap.get(1) {
                let inner = block_content.as_str();
                debug!("Parsing code block content: {:?}", inner);

                // Try parsing XML tags from inside the code block
                let xml_calls = self.extract_from_xml_tags(inner, available_tools);
                if !xml_calls.is_empty() {
                    calls.extend(xml_calls);
                    continue;
                }

                // Try parsing direct function calls from inside the code block
                let func_calls = self.extract_from_function_calls(inner, available_tools);
                if !func_calls.is_empty() {
                    calls.extend(func_calls);
                }
            }
        }

        if !calls.is_empty() {
            return calls;
        }

        // Try function call format: tool_name({"arg": "value"})
        calls.extend(self.extract_from_function_calls(content, available_tools));

        calls
    }

    /// Extract tool calls from XML tags in content
    fn extract_from_xml_tags(
        &self,
        content: &str,
        available_tools: &[String],
    ) -> Vec<ExtractedToolCall> {
        let mut calls = Vec::new();
        for cap in self.xml_tag_regex.captures_iter(content) {
            if let (Some(name), Some(args)) = (cap.get(1), cap.get(2)) {
                let tool_name = name.as_str().to_string();
                let args_str = args.as_str().trim();

                if !available_tools.contains(&tool_name) {
                    warn!("Tool {} not in available tools", tool_name);
                    continue;
                }

                if let Ok(arguments) = serde_json::from_str::<Value>(args_str) {
                    calls.push(ExtractedToolCall {
                        name: tool_name,
                        arguments,
                        source: ToolCallSource::XmlTags,
                    });
                }
            }
        }
        calls
    }

    /// Extract tool calls from function call patterns in content
    fn extract_from_function_calls(
        &self,
        content: &str,
        available_tools: &[String],
    ) -> Vec<ExtractedToolCall> {
        let mut calls = Vec::new();
        for cap in self.function_call_regex.captures_iter(content) {
            if let (Some(name), Some(args)) = (cap.get(1), cap.get(2)) {
                let tool_name = name.as_str().to_string();
                let args_str = args.as_str().trim();

                if !available_tools.contains(&tool_name) {
                    warn!("Tool {} not in available tools", tool_name);
                    continue;
                }

                if let Ok(arguments) = serde_json::from_str::<Value>(args_str) {
                    info!("Extracted tool call: {}", tool_name);
                    calls.push(ExtractedToolCall {
                        name: tool_name,
                        arguments,
                        source: ToolCallSource::JsonInText,
                    });
                }
            }
        }
        calls
    }

    /// Check if response contains forbidden CLI commands
    pub fn contains_forbidden_commands(&self, content: &str) -> Vec<String> {
        let forbidden = [
            "ovs-vsctl",
            "ovs-ofctl",
            "ovs-dpctl",
            "ovsdb-client",
            "systemctl",
            "service ",
            "journalctl",
            "ip addr",
            "ip link",
            "ip route",
            "ifconfig",
            "nmcli",
            "apt ",
            "apt-get",
            "yum ",
            "dnf ",
            "pacman",
            "sudo ",
            "su -",
        ];

        let lower = content.to_lowercase();
        forbidden
            .iter()
            .filter(|cmd| lower.contains(*cmd))
            .map(|s| s.to_string())
            .collect()
    }
}

/// Natural Language Admin Orchestrator
///
/// This is the main entry point for natural language server administration.
pub struct NLAdminOrchestrator {
    tool_registry: Arc<ToolRegistry>,
    tool_parser: ToolCallParser,
}

impl NLAdminOrchestrator {
    pub fn new(tool_registry: Arc<ToolRegistry>) -> Self {
        Self {
            tool_registry,
            tool_parser: ToolCallParser::new(),
        }
    }

    /// Generate the system prompt that instructs the LLM to use tools
    pub async fn generate_system_prompt(&self) -> String {
        // Use the comprehensive system prompt from system_prompt.rs
        let base_prompt = crate::system_prompt::generate_system_prompt(None).await;
        let mut prompt = base_prompt.content;

        // Add dynamically generated tool list
        let tools = self.tool_registry.list().await;
        prompt.push_str("\n\n## DYNAMICALLY LOADED TOOLS\n\n");

        // Group tools by category
        let mut ovs_tools = Vec::new();
        let mut systemd_tools = Vec::new();
        let mut network_tools = Vec::new();
        let mut other_tools = Vec::new();

        for tool in &tools {
            let entry = format!("- **{}**: {}\n", tool.name, tool.description);
            if tool.name.starts_with("ovs_") {
                ovs_tools.push(entry);
            } else if tool.name.contains("systemd") {
                systemd_tools.push(entry);
            } else if tool.name.contains("network") || tool.name.contains("dbus_networkmanager") {
                network_tools.push(entry);
            } else {
                other_tools.push(entry);
            }
        }

        if !ovs_tools.is_empty() {
            prompt.push_str("### Open vSwitch (OVS) Tools\n");
            for t in ovs_tools {
                prompt.push_str(&t);
            }
            prompt.push('\n');
        }

        if !systemd_tools.is_empty() {
            prompt.push_str("### Systemd/Service Tools\n");
            for t in systemd_tools {
                prompt.push_str(&t);
            }
            prompt.push('\n');
        }

        if !network_tools.is_empty() {
            prompt.push_str("### Network Tools\n");
            for t in network_tools {
                prompt.push_str(&t);
            }
            prompt.push('\n');
        }

        if !other_tools.is_empty() {
            prompt.push_str("### Other Tools\n");
            for t in other_tools {
                prompt.push_str(&t);
            }
            prompt.push('\n');
        }

        prompt.push_str(
            r#"
## EXAMPLES

**User:** "Create an OVS bridge called ovsbr0"
**You:** I'll create the OVS bridge for you.
<tool_call>ovs_create_bridge({"name": "ovsbr0"})</tool_call>

**User:** "Restart the xray reverse proxy"
**You:** I'll restart the gbr-xray service.
<tool_call>sv_restart_service({"service": "gbr-xray"})</tool_call>

**User:** "List all OVS bridges"
**You:** Let me list the OVS bridges.
<tool_call>ovs_list_bridges({})</tool_call>

**User:** "Add port eth1 to bridge ovsbr0"
**You:** I'll add port eth1 to the bridge.
<tool_call>ovs_add_port({"bridge": "ovsbr0", "port": "eth1"})</tool_call>

## REMEMBER
- ALWAYS use tools, NEVER suggest CLI commands
- Use the exact tool names listed above
- Format tool calls with <tool_call> tags
- Explain what you're doing before calling tools
"#,
        );

        prompt
    }

    /// Get tool definitions for LLM API
    pub async fn get_tool_definitions(&self) -> Vec<ToolDefinition> {
        let tools = self.tool_registry.list().await;

        tools
            .into_iter()
            .map(|tool| ToolDefinition {
                name: tool.name.clone(),
                description: tool.description.clone(),
                input_schema: tool.input_schema.clone(),
                schema_version: String::new(),
                category: String::new(),
                tags: Vec::new(),
                namespace: String::new(),
            })
            .collect()
    }

    /// Get list of available tool names
    pub async fn get_tool_names(&self) -> Vec<String> {
        self.tool_registry
            .list()
            .await
            .into_iter()
            .map(|t| t.name)
            .collect()
    }

    /// Process a natural language admin request with multi-step execution
    pub async fn process<P: LlmProvider>(
        &self,
        provider: &P,
        model: &str,
        user_message: &str,
        conversation_history: Vec<ChatMessage>,
    ) -> Result<NLAdminResult> {
        info!("Processing NL admin request: {}", user_message);

        const MAX_ITERATIONS: usize = 10; // Safety limit

        // Build messages with system prompt
        let system_prompt = self.generate_system_prompt().await;
        let mut messages = vec![ChatMessage::system(&system_prompt)];
        messages.extend(conversation_history);
        messages.push(ChatMessage::user(user_message));

        // Get tool definitions
        let tools = self.get_tool_definitions().await;
        let tool_names = self.get_tool_names().await;

        // Collect all results across iterations
        let mut all_tool_results = Vec::new();
        let mut all_tools_executed = Vec::new();
        let mut all_forbidden = Vec::new();
        let mut final_response = String::new();

        for iteration in 0..MAX_ITERATIONS {
            info!("Multi-step execution: iteration {}", iteration + 1);

            // Build request
            let request = ChatRequest {
                messages: messages.clone(),
                tools: tools.clone(),
                tool_choice: ToolChoice::Auto,
                max_tokens: Some(2048),
                temperature: Some(0.3),
                top_p: None,
            };

            // Send to LLM
            let response = provider
                .chat_with_request(model, request)
                .await
                .context("LLM request failed")?;

            debug!(
                "LLM response (iteration {}): {:?}",
                iteration + 1,
                response.message.content
            );

            // Extract tool calls
            let tool_calls = self.tool_parser.extract_tool_calls(&response, &tool_names);

            // Check for forbidden commands
            let forbidden = self
                .tool_parser
                .contains_forbidden_commands(&response.message.content);
            if !forbidden.is_empty() {
                warn!("LLM suggested forbidden commands: {:?}", forbidden);
                all_forbidden.extend(forbidden);
            }

            // If no tool calls, we're done (LLM has finished or is just responding)
            if tool_calls.is_empty() {
                info!(
                    "No more tool calls - task complete after {} iterations",
                    iteration + 1
                );
                final_response = response.message.content.clone();
                break;
            }

            // Check if the only tool call is respond_to_user - that means we're done
            let is_final_response =
                tool_calls.len() == 1 && tool_calls[0].name == "respond_to_user";

            // Execute tool calls
            let mut iteration_results = Vec::new();
            for call in &tool_calls {
                info!(
                    "Executing tool: {} with args: {:?}",
                    call.name, call.arguments
                );
                all_tools_executed.push(call.name.clone());

                match self.execute_tool(&call.name, call.arguments.clone()).await {
                    Ok(result) => {
                        iteration_results.push(ToolExecutionResult {
                            tool_name: call.name.clone(),
                            success: true,
                            result: Some(result.clone()),
                            error: None,
                        });

                        // Add tool result to messages for context
                        messages.push(ChatMessage::assistant(format!(
                            "<tool_call>{}({})</tool_call>",
                            call.name,
                            simd_json::to_string(&call.arguments).unwrap_or_default()
                        )));
                        messages.push(ChatMessage::user(format!(
                            "Tool result for {}: {}",
                            call.name,
                            simd_json::to_string_pretty(&result).unwrap_or_default()
                        )));
                    }
                    Err(e) => {
                        error!("Tool execution failed: {}", e);
                        iteration_results.push(ToolExecutionResult {
                            tool_name: call.name.clone(),
                            success: false,
                            result: None,
                            error: Some(e.to_string()),
                        });

                        // Add error to messages
                        messages.push(ChatMessage::assistant(format!(
                            "<tool_call>{}({})</tool_call>",
                            call.name,
                            simd_json::to_string(&call.arguments).unwrap_or_default()
                        )));
                        messages.push(ChatMessage::user(format!(
                            "Tool {} failed: {}",
                            call.name, e
                        )));
                    }
                }
            }

            all_tool_results.extend(iteration_results);

            // If this was a respond_to_user call, we're done
            if is_final_response {
                info!("Final response received after {} iterations", iteration + 1);
                final_response = response.message.content.clone();
                break;
            }

            // Add continuation prompt to encourage completing the task
            messages.push(ChatMessage::user(
                "Continue with the next step. If all steps are complete, use respond_to_user to summarize the results."
            ));
        }

        // Generate user-friendly response
        let message = if !final_response.is_empty() {
            self.format_response(&final_response, &all_tool_results, &all_forbidden)
        } else {
            self.format_response(
                "Task execution completed.",
                &all_tool_results,
                &all_forbidden,
            )
        };

        let success = all_tool_results.iter().all(|r| r.success) && all_forbidden.is_empty();

        Ok(NLAdminResult {
            message,
            success,
            tools_executed: all_tools_executed,
            tool_results: all_tool_results,
            llm_response: Some(final_response),
        })
    }

    /// Execute a single tool
    async fn execute_tool(&self, name: &str, arguments: Value) -> Result<Value> {
        let tool = self
            .tool_registry
            .get(name)
            .await
            .ok_or_else(|| anyhow::anyhow!("Tool not found: {}", name))?;

        let result = tool.execute(arguments).await;

        match result {
            Ok(content) => Ok(content),
            Err(e) => Err(anyhow::anyhow!("Tool execution failed: {}", e)),
        }
    }

    /// Format the final response for the user
    fn format_response(
        &self,
        llm_response: &str,
        tool_results: &[ToolExecutionResult],
        forbidden_commands: &[String],
    ) -> String {
        let mut response = String::new();

        // Add warning if forbidden commands were detected
        if !forbidden_commands.is_empty() {
            response.push_str("⚠️ Note: The AI attempted to suggest CLI commands, but I executed the proper tools instead.\n\n");
        }

        // If no tools were executed, just return the LLM response (cleaned)
        if tool_results.is_empty() {
            // Remove tool_call tags from response
            let cleaned = self.clean_llm_response(llm_response);
            return cleaned;
        }

        // Format tool results as a summary section
        let success_count = tool_results.iter().filter(|r| r.success).count();
        let fail_count = tool_results.iter().filter(|r| !r.success).count();

        if tool_results.len() > 1 {
            response.push_str(&format!(
                "**Executed {} tools** ({} success, {} failed)\n\n",
                tool_results.len(),
                success_count,
                fail_count
            ));
        }

        for result in tool_results {
            if result.success {
                response.push_str(&format!("✅ **{}** ", result.tool_name));
                if let Some(ref data) = result.result {
                    // Brief summary for successful tools
                    if let Some(obj) = data.as_object() {
                        let key_count = obj
                            .keys()
                            .filter(|k: &&String| k.as_str() != "_internal")
                            .count();
                        response.push_str(&format!("({} fields)", key_count));
                    }
                }
                response.push('\n');
            } else {
                response.push_str(&format!("❌ **{}** failed", result.tool_name));
                if let Some(ref err) = result.error {
                    response.push_str(&format!(": {}", err));
                }
                response.push('\n');
            }
        }

        // Include the LLM's final analysis/summary
        let cleaned_llm = self.clean_llm_response(llm_response);
        if !cleaned_llm.is_empty() && cleaned_llm.len() > 10 {
            response.push_str("\n---\n\n");
            response.push_str(&cleaned_llm);
        }

        response
    }

    /// Clean tool_call tags from LLM response
    fn clean_llm_response(&self, response: &str) -> String {
        // Remove <tool_call>...</tool_call> tags (single line)
        let re =
            Regex::new(r"<tool_call>.*?</tool_call>").expect("hardcoded regex pattern is valid");
        let cleaned = re.replace_all(response, "");

        // Remove ```tool...``` blocks (single line)
        let re2 = Regex::new(r"```tool\s*\n.*?\n```").expect("hardcoded regex pattern is valid");
        let cleaned = re2.replace_all(&cleaned, "");

        // Remove ```tool_code...``` blocks (multiline)
        let re3 =
            Regex::new(r"(?s)```tool_code\s*\n.*?\n```").expect("hardcoded regex pattern is valid");
        let cleaned = re3.replace_all(&cleaned, "");

        // Remove tool call patterns like: ovs_list_bridges({})
        let re4 = Regex::new(r"\w+\(\s*\{\s*\}\s*\)").expect("hardcoded regex pattern is valid");
        let cleaned = re4.replace_all(&cleaned, "");

        // Clean up multiple blank lines
        let re5 = Regex::new(r"\n{3,}").expect("hardcoded regex pattern is valid");
        let cleaned = re5.replace_all(&cleaned, "\n\n");

        cleaned.trim().to_string()
    }
}

/// Format a JSON value for display
#[allow(dead_code)]
fn format_value(value: &Value) -> String {
    if let Some(s) = value.as_str() {
        return s.to_string();
    }
    if let Some(b) = value.as_bool() {
        return b.to_string();
    }
    if let Some(n) = value.as_i64() {
        return n.to_string();
    }
    if let Some(n) = value.as_u64() {
        return n.to_string();
    }
    if let Some(n) = value.as_f64() {
        return n.to_string();
    }
    if let Some(arr) = value.as_array() {
        if arr.len() <= 5 {
            return format!(
                "[{}]",
                arr.iter().map(format_value).collect::<Vec<_>>().join(", ")
            );
        }
        return format!("[{} items]", arr.len());
    }
    if value.as_object().is_some() {
        return simd_json::to_string_pretty(value).unwrap_or_else(|_| "[object]".to_string());
    }
    if value.is_null() {
        return "null".to_string();
    }
    "[unknown]".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_call_parser_xml_tags() {
        let parser = ToolCallParser::new();
        let content = r#"I'll create the bridge for you.
<tool_call>ovs_create_bridge({"name": "ovsbr0"})</tool_call>"#;

        // Create a mock response
        let response = ChatResponse {
            message: ChatMessage::assistant(content),
            model: "test".to_string(),
            provider: "test".to_string(),
            finish_reason: None,
            usage: None,
            tool_calls: None,
        };

        let available = vec!["ovs_create_bridge".to_string()];
        let calls = parser.extract_tool_calls(&response, &available);

        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "ovs_create_bridge");
        assert_eq!(calls[0].arguments["name"], "ovsbr0");
    }

    #[test]
    fn test_forbidden_command_detection() {
        let parser = ToolCallParser::new();

        let content = "You can use ovs-vsctl add-br ovsbr0 to create the bridge";
        let forbidden = parser.contains_forbidden_commands(content);
        assert!(forbidden.contains(&"ovs-vsctl".to_string()));

        let clean = "I'll create the bridge using the ovs_create_bridge tool";
        let forbidden = parser.contains_forbidden_commands(clean);
        assert!(forbidden.is_empty());
    }
}
