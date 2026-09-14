use super::types::ToolResult;
use super::UnifiedOrchestrator;
use simd_json::prelude::*;
use simd_json::OwnedValue as Value;

impl UnifiedOrchestrator {
    /// Format results for display in the LLM context
    #[allow(dead_code)]
    pub(crate) fn format_results(
        &self,
        llm_text: &str,
        results: &[ToolResult],
        forbidden: &[String],
    ) -> String {
        let mut output = String::new();

        // Add warning if LLM suggested forbidden commands
        if !forbidden.is_empty() {
            output.push_str("⚠️ Note: The AI attempted to suggest CLI commands, but I executed the proper tools instead.\n\n");
        }

        // Summary for multiple tools
        let success_count = results.iter().filter(|r| r.success).count();
        let failed_count = results.iter().filter(|r| !r.success).count();

        if results.len() > 1 {
            output.push_str(&format!(
                "**Executed {} tools** ({} success, {} failed)\n\n",
                results.len(),
                success_count,
                failed_count
            ));
        }

        // Tool results with actual data
        for r in results {
            if r.success {
                output.push_str(&format!("✅ **{}**\n", r.name));
                if let Some(ref data) = r.result {
                    // Format the result data nicely
                    output.push_str(&self.format_tool_result(data));
                }
            } else {
                output.push_str(&format!(
                    "❌ **{}** failed: {}\n",
                    r.name,
                    r.error.as_ref().unwrap_or(&"Unknown".to_string())
                ));
            }
            output.push('\n');
        }

        // Add LLM commentary (cleaned) only if it adds value
        let cleaned = self.clean_llm_text(llm_text);
        if !cleaned.is_empty() && cleaned.len() > 20 {
            output.push_str("---\n\n");
            output.push_str(&cleaned);
        }

        output
    }

    /// Format a tool result for display
    #[allow(dead_code)]
    pub(crate) fn format_tool_result(&self, data: &Value) -> String {
        if let Some(obj) = data.as_object() {
            let mut result = String::new();
            for (key, value) in obj {
                if key.starts_with('_') {
                    continue;
                }
                if let Some(arr) = value.as_array() {
                    result.push_str(&format!("  • **{}**:\n", key));
                    result.push_str(&self.format_array(arr, 20));
                } else {
                    let formatted_value = self.format_value(value);
                    result.push_str(&format!("  • **{}**: {}\n", key, formatted_value));
                }
            }
            return result;
        }
        if let Some(arr) = data.as_array() {
            return self.format_array(arr, 20);
        }
        if let Some(s) = data.as_str() {
            return format!("  {}\n", s);
        }
        if let Some(b) = data.as_bool() {
            return format!("  {}\n", b);
        }
        if let Some(n) = data.as_i64() {
            return format!("  {}\n", n);
        }
        if let Some(n) = data.as_u64() {
            return format!("  {}\n", n);
        }
        if let Some(n) = data.as_f64() {
            return format!("  {}\n", n);
        }
        if data.as_null().is_some() {
            return "  *(null)*\n".to_string();
        }
        format!("  {}\n", self.format_value(data))
    }

    /// Format an array for display
    #[allow(dead_code)]
    fn format_array(&self, arr: &[Value], max_items: usize) -> String {
        if arr.is_empty() {
            return "    *(empty list)*\n".to_string();
        }

        let mut result = String::new();
        let show_count = arr.len().min(max_items);

        for item in arr.iter().take(show_count) {
            if let Some(obj) = item.as_object() {
                let summary = self.summarize_object(obj);
                result.push_str(&format!("    - {}\n", summary));
            } else if let Some(s) = item.as_str() {
                result.push_str(&format!("    - {}\n", s));
            } else {
                result.push_str(&format!("    - {}\n", self.format_value(item)));
            }
        }

        if arr.len() > max_items {
            result.push_str(&format!("    ... and {} more\n", arr.len() - max_items));
        }

        result
    }

    /// Summarize an object into a single line
    #[allow(dead_code)]
    fn summarize_object(&self, obj: &simd_json::value::owned::Object) -> String {
        // Look for common identifying fields
        let name_fields = [
            "name",
            "unit",
            "id",
            "path",
            "service",
            "interface",
            "bridge",
        ];
        let status_fields = ["state", "status", "active_state", "sub_state", "load_state"];

        let mut parts = Vec::new();

        // Get the name/id
        for field in name_fields {
            if let Some(value) = obj.get(field) {
                if let Some(v) = value.as_str() {
                    parts.push(v.to_string());
                    break;
                }
            }
        }

        // Get status if available
        for field in status_fields {
            if let Some(value) = obj.get(field) {
                if let Some(v) = value.as_str() {
                    parts.push(format!("({})", v));
                    break;
                }
            }
        }

        if parts.is_empty() {
            // Fallback: show first few fields
            let keys: Vec<String> = obj.keys().take(3).cloned().collect();
            format!("{{{}}}...", keys.join(", "))
        } else {
            parts.join(" ")
        }
    }

    /// Format a single value for display
    #[allow(dead_code)]
    fn format_value(&self, value: &Value) -> String {
        if let Some(s) = value.as_str() {
            if s.len() > 100 {
                return format!("{}...", &s[..100]);
            }
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
            if arr.is_empty() {
                return "[]".to_string();
            }
            if arr.len() <= 5 {
                let items: Vec<String> = arr.iter().map(|v| self.format_value(v)).collect();
                return format!("[{}]", items.join(", "));
            }
            return format!("[{} items]", arr.len());
        }
        if let Some(obj) = value.as_object() {
            if obj.is_empty() {
                return "{{}}".to_string();
            }
            return self.summarize_object(obj);
        }
        if value.as_null().is_some() {
            return "null".to_string();
        }
        simd_json::to_string(value).unwrap_or_default()
    }

    /// Clean tool call syntax from LLM text
    #[allow(dead_code)]
    pub(crate) fn clean_llm_text(&self, text: &str) -> String {
        let mut cleaned = text.to_string();

        // Remove <tool_call>...</tool_call>
        if let Ok(re) = regex::Regex::new(r"<tool_call>.*?</tool_call>") {
            cleaned = re.replace_all(&cleaned, "").to_string();
        }

        // Remove tool_name({...})
        if let Ok(re) = regex::Regex::new(r"\w+\(\s*\{[^}]*\}\s*\)") {
            cleaned = re.replace_all(&cleaned, "").to_string();
        }

        // Clean multiple newlines
        if let Ok(re) = regex::Regex::new(r"\n{3,}") {
            cleaned = re.replace_all(&cleaned, "\n\n").to_string();
        }

        cleaned.trim().to_string()
    }

    /// Generate a human-readable description of a tool call
    pub(crate) fn describe_tool_call(&self, name: &str, args: &Value) -> String {
        match name {
            "execute_tool" => {
                let tool_name = args
                    .get("tool_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                let inner_args = args
                    .get("arguments")
                    .cloned()
                    .unwrap_or(simd_json::json!({}));
                self.describe_actual_tool(tool_name, &inner_args)
            }
            "list_tools" => {
                let category = args
                    .get("category")
                    .and_then(|v| v.as_str())
                    .unwrap_or("all");
                format!("Listing available tools (category: {})", category)
            }
            "search_tools" => {
                let query = args.get("query").and_then(|v| v.as_str()).unwrap_or("");
                format!("Searching for tools matching \"{}\"", query)
            }
            "get_tool_schema" => {
                let tool = args
                    .get("tool_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                format!("Getting schema for tool: {}", tool)
            }
            "respond" => "Preparing final response".to_string(),
            _ => format!("Calling {}", name),
        }
    }

    /// Generate human-readable description for actual tool execution
    fn describe_actual_tool(&self, name: &str, args: &Value) -> String {
        match name {
            // OVS tools
            "ovs_list_bridges" => "Listing OVS bridges".to_string(),
            "ovs_create_bridge" => {
                let bridge = args.get("name").and_then(|v| v.as_str()).unwrap_or("?");
                format!("Creating OVS bridge '{}'", bridge)
            }
            "ovs_delete_bridge" => {
                let bridge = args.get("name").and_then(|v| v.as_str()).unwrap_or("?");
                format!("Deleting OVS bridge '{}'", bridge)
            }
            "ovs_add_port" => {
                let bridge = args.get("bridge").and_then(|v| v.as_str()).unwrap_or("?");
                let port = args.get("port").and_then(|v| v.as_str()).unwrap_or("?");
                let port_type = args
                    .get("type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("normal");
                format!(
                    "Adding {} port '{}' to bridge '{}'",
                    port_type, port, bridge
                )
            }
            "ovs_list_ports" => {
                let bridge = args.get("bridge").and_then(|v| v.as_str()).unwrap_or("?");
                format!("Listing ports on bridge '{}'", bridge)
            }
            // Systemd tools
            "dbus_systemd_restart_unit" => {
                let unit = args.get("unit").and_then(|v| v.as_str()).unwrap_or("?");
                format!("Restarting service '{}'", unit)
            }
            "dbus_systemd_start_unit" => {
                let unit = args.get("unit").and_then(|v| v.as_str()).unwrap_or("?");
                format!("Starting service '{}'", unit)
            }
            "dbus_systemd_stop_unit" => {
                let unit = args.get("unit").and_then(|v| v.as_str()).unwrap_or("?");
                format!("Stopping service '{}'", unit)
            }
            "dbus_systemd_get_unit_status" => {
                let unit = args.get("unit").and_then(|v| v.as_str()).unwrap_or("?");
                format!("Checking status of '{}'", unit)
            }
            "dbus_systemd_list_units" => "Listing systemd units".to_string(),
            // Network tools
            "rtnetlink_list_links" | "list_network_interfaces" => {
                "Listing network interfaces".to_string()
            }
            "rtnetlink_add_address" => {
                let iface = args
                    .get("interface")
                    .and_then(|v| v.as_str())
                    .unwrap_or("?");
                let addr = args.get("address").and_then(|v| v.as_str()).unwrap_or("?");
                format!("Adding IP address {} to interface '{}'", addr, iface)
            }
            "rtnetlink_link_up" => {
                let iface = args
                    .get("interface")
                    .and_then(|v| v.as_str())
                    .unwrap_or("?");
                format!("Bringing interface '{}' up", iface)
            }
            "rtnetlink_link_down" => {
                let iface = args
                    .get("interface")
                    .and_then(|v| v.as_str())
                    .unwrap_or("?");
                format!("Bringing interface '{}' down", iface)
            }
            // File tools
            "file_read" => {
                let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("?");
                format!("Reading file '{}'", path)
            }
            "file_write" => {
                let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("?");
                format!("Writing to file '{}'", path)
            }
            "file_list" => {
                let path = args.get("path").and_then(|v| v.as_str()).unwrap_or(".");
                format!("Listing files in '{}'", path)
            }
            // Shell tools
            "shell_exec" => {
                let cmd = args.get("command").and_then(|v| v.as_str()).unwrap_or("?");
                let cmd_preview = if cmd.len() > 50 {
                    format!("{}...", &cmd[..50])
                } else {
                    cmd.to_string()
                };
                format!("Running command: {}", cmd_preview)
            }
            // Agent tools
            "agent_sequential_thinking" => {
                if let Some(thought) = args.get("thought").and_then(|v| v.as_str()) {
                    let preview = if thought.len() > 50 {
                        format!("{}...", &thought[..50])
                    } else {
                        thought.to_string()
                    };
                    format!("Thinking: {}", preview)
                } else {
                    "Sequential thinking".to_string()
                }
            }
            // Default
            _ => format!("Executing {}", name),
        }
    }
}
