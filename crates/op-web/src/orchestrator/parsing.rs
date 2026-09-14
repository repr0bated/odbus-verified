use super::UnifiedOrchestrator;
use regex::Regex;
use simd_json::prelude::*;
use simd_json::{json, OwnedValue as Value};
use tracing::info;

impl UnifiedOrchestrator {
    /// Extract tool calls from text (for models without native tool calling)
    ///
    /// Supports:
    /// - XML tags: `<tool_call>name({args})</tool_call>`
    /// - Code blocks: ` ```tool ... ``` `
    /// - Direct calls: `tool_name({args})`
    #[allow(dead_code)]
    pub(crate) fn extract_tool_calls_from_text(
        &self,
        text: &str,
        available: &[String],
    ) -> Vec<(String, Value)> {
        let mut calls = Vec::new();

        // Pattern 1: <tool_call>name({"arg": "val"})</tool_call> (with multiline support)
        if let Ok(re) =
            Regex::new(r"(?s)<tool_call>\s*([a-zA-Z_][a-zA-Z0-9_]*)\s*\((.*?)\)\s*</tool_call>")
        {
            for cap in re.captures_iter(text) {
                if let (Some(name), Some(args)) = (cap.get(1), cap.get(2)) {
                    let tool_name = name.as_str().to_string();
                    if available.contains(&tool_name) {
                        let raw = args.as_str().trim().to_string();
                        if let Ok(parsed) = serde_json::from_str(&raw) {
                            info!("Extracted tool call from XML tags: {}", tool_name);
                            calls.push((tool_name, parsed));
                        }
                    }
                }
            }
        }

        // If we found XML tag calls, use those (preferred format)
        if !calls.is_empty() {
            return calls;
        }

        // Pattern 2: ```tool or ```tool_code blocks
        if let Ok(re) = Regex::new(r"(?s)```(?:tool|tool_code)\s*\n(.+?)\n```") {
            for cap in re.captures_iter(text) {
                if let Some(block) = cap.get(1) {
                    // Parse tool calls from inside the block
                    let inner_calls = self.parse_function_calls(block.as_str(), available);
                    for call in inner_calls {
                        if !calls.iter().any(|(n, _)| n == &call.0) {
                            calls.push(call);
                        }
                    }
                }
            }
        }

        if !calls.is_empty() {
            return calls;
        }

        // Pattern 3: tool_name({"arg": "val"}) - direct function call syntax
        calls.extend(self.parse_function_calls(text, available));

        calls
    }

    /// Parse function call patterns from text
    #[allow(dead_code)]
    pub(crate) fn parse_function_calls(
        &self,
        text: &str,
        available: &[String],
    ) -> Vec<(String, Value)> {
        let mut calls = Vec::new();

        // Match: tool_name({...}) with multiline JSON support
        if let Ok(re) = Regex::new(r"(?s)\b([a-zA-Z_][a-zA-Z0-9_]*)\s*\(\s*(\{.*?\})\s*") {
            for cap in re.captures_iter(text) {
                if let (Some(name), Some(args)) = (cap.get(1), cap.get(2)) {
                    let tool_name = name.as_str().to_string();
                    if available.contains(&tool_name) && !calls.iter().any(|(n, _)| n == &tool_name)
                    {
                        let raw = args.as_str().trim().to_string();
                        if let Ok(parsed) = serde_json::from_str(&raw) {
                            info!("Extracted tool call from function syntax: {}", tool_name);
                            calls.push((tool_name, parsed));
                        }
                    }
                }
            }
        }

        calls
    }

    /// Parse tool calls from LLM response (handling native + text fallback)
    pub(crate) fn parse_tool_calls(
        &self,
        content: &str,
        tool_calls: &Option<Vec<op_llm::provider::ToolCallInfo>>,
    ) -> Vec<(String, Value)> {
        let mut calls = Vec::new();

        // First, check native tool calls
        if let Some(ref tc) = tool_calls {
            for call in tc {
                let args: Value = if let Some(s) = call.arguments.as_str() {
                    serde_json::from_str(s).unwrap_or(json!({}))
                } else {
                    call.arguments.clone()
                };
                calls.push((call.name.clone(), args));
            }
        }

        // Also parse from content if it contains tool call patterns
        // (some models embed tool calls in text)
        if calls.is_empty() {
            // Try to parse JSON tool calls from content
            if let Some(start) = content.find("```json") {
                if let Some(end) = content[start..].find("```").map(|e| start + e + 3) {
                    let json_str = &content[start + 7..end - 3];
                    if let Ok(parsed) = serde_json::from_str::<Value>(json_str) {
                        if let Some(name) = parsed.get("tool").and_then(|v| v.as_str()) {
                            let args = parsed.get("arguments").cloned().unwrap_or(json!({}));
                            calls.push((name.to_string(), args));
                        }
                    }
                }
            }
        }

        calls
    }

    /// Detect forbidden CLI commands in response
    pub(crate) fn detect_forbidden_commands(&self, content: &str) -> Vec<String> {
        let forbidden = [
            "rm -rf",
            "mkfs",
            "dd if=",
            "> /dev/",
            "chmod 777",
            "curl | sh",
            "wget | sh",
            "eval $(",
            "`curl",
            "`wget",
            // OVS CLI - use ovs_* tools instead
            "ovs-vsctl",
            "ovs-ofctl",
            "ovs-dpctl",
            "ovsdb-client",
            // Systemd CLI - use dbus_systemd_* tools instead
            "systemctl",
            "service ",
            "journalctl",
            // Network CLI - use rtnetlink_* tools instead
            "ip addr",
            "ip link",
            "ip route",
            "ifconfig",
            "nmcli",
            // Package managers - not supported yet
            "apt ",
            "apt-get",
            "yum ",
            "dnf ",
            "pacman",
            // Container CLI - use lxc_* tools instead
            "docker ",
            "kubectl",
            "lxc ",
        ];

        forbidden
            .iter()
            .filter(|cmd| content.to_lowercase().contains(*cmd))
            .map(|s| s.to_string())
            .collect()
    }
}
