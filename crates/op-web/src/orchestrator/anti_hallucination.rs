//! Anti-Hallucination Enforcement
//!
//! Detects and handles when the LLM suggests CLI commands instead of using tools.
//! This module provides:
//! - Detection of forbidden CLI patterns
//! - Correction message generation
//! - Retry logic for enforcement

use tracing::{warn, info};

/// Forbidden CLI command patterns
/// These should NEVER appear in chatbot responses
const FORBIDDEN_PATTERNS: &[(&str, &str)] = &[
    // OVS - use ovs_* tools
    ("ovs-vsctl", "ovs_list_bridges, ovs_create_bridge, ovs_add_port"),
    ("ovs-ofctl", "ovs_add_flow, ovs_list_flows"),
    ("ovs-dpctl", "ovs_* tools"),
    ("ovsdb-client", "ovs_* tools"),
    
    // Service manager — this host runs runit; use the sv_* tools.
    ("systemctl start", "sv_start_service"),
    ("systemctl stop", "sv_stop_service"),
    ("systemctl restart", "sv_restart_service"),
    ("systemctl status", "sv_service_status"),
    ("systemctl", "sv_* tools"),
    ("service ", "sv_* tools"),
    ("dinitctl", "sv_* tools"),
    ("s6-rc", "sv_* tools"),
    ("s6-svc", "sv_* tools"),
    ("s6-svstat", "sv_* tools"),
    ("service6", "sv_* tools"),
    ("runsvdir", "sv_* tools"),
    ("runsv ", "sv_* tools"),
    ("journalctl", "sv_service_status"),
    
    // Network - use rtnetlink/network tools
    ("ip addr", "list_network_interfaces, add_ip_address"),
    ("ip link", "list_network_interfaces, set_interface_up"),
    ("ip route", "list_routes, add_route"),
    ("ip a ", "list_network_interfaces"),
    ("ip r ", "list_routes"),
    ("ifconfig", "list_network_interfaces"),
    ("nmcli", "dbus_networkmanager_* tools"),
    ("netplan", "network configuration tools"),
    
    // Package management - use packagekit
    ("apt install", "dbus_packagekit_install_packages"),
    ("apt-get install", "dbus_packagekit_install_packages"),
    ("apt update", "dbus_packagekit_refresh_cache"),
    ("apt-get update", "dbus_packagekit_refresh_cache"),
    ("apt ", "dbus_packagekit_* tools"),
    ("apt-get", "dbus_packagekit_* tools"),
    ("yum ", "packagekit tools"),
    ("dnf ", "packagekit tools"),
    
    // Dangerous patterns
    ("sudo ", "tools run as root already"),
    ("su -", "tools run as root already"),
    ("> /etc/", "write_file tool"),
    ("rm -rf", "file deletion tools"),
    
    // Container CLIs (if using native tools)
    ("docker ", "container_* tools or native APIs"),
    ("podman ", "container_* tools or native APIs"),
    ("lxc ", "lxc_* tools"),
];

/// Suggestion patterns that indicate the LLM is not executing
const SUGGESTION_PATTERNS: &[&str] = &[
    "you can run",
    "you could run",
    "try running",
    "execute the command",
    "run the following",
    "use the command",
    "by running",
    "with the command",
    "here's the command",
    "here is the command",
    "the command is",
    "command to use",
    "terminal command",
    "shell command",
    "bash command",
    "run this",
    "execute this",
];

/// Result of checking for forbidden commands
#[derive(Debug, Clone)]
pub struct ForbiddenCommandCheck {
    /// List of detected forbidden commands
    pub detected: Vec<ForbiddenCommand>,
    /// Whether the response should be rejected
    pub should_reject: bool,
    /// Correction message to send back to LLM
    pub correction_message: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ForbiddenCommand {
    pub pattern: String,
    pub alternative: String,
    pub context: String,
}

/// Check response for forbidden CLI commands
pub fn check_for_forbidden_commands(content: &str) -> ForbiddenCommandCheck {
    let lower = content.to_lowercase();
    let mut detected = Vec::new();
    
    // Check for forbidden CLI patterns
    for (pattern, alternative) in FORBIDDEN_PATTERNS {
        if lower.contains(&pattern.to_lowercase()) {
            // Find context (surrounding text)
            let context = extract_context(&lower, pattern);
            detected.push(ForbiddenCommand {
                pattern: pattern.to_string(),
                alternative: alternative.to_string(),
                context,
            });
        }
    }
    
    // Check for suggestion patterns
    let has_suggestions = SUGGESTION_PATTERNS.iter().any(|p| lower.contains(p));
    
    // Should reject if we found CLI commands AND suggestion language
    let should_reject = !detected.is_empty() && has_suggestions;
    
    // Build correction message
    let correction_message = if should_reject {
        Some(build_correction_message(&detected))
    } else if !detected.is_empty() {
        // Log warning but don't reject if no suggestion language
        warn!("Response contains CLI commands but no suggestion language: {:?}", 
              detected.iter().map(|d| &d.pattern).collect::<Vec<_>>());
        None
    } else {
        None
    };
    
    ForbiddenCommandCheck {
        detected,
        should_reject,
        correction_message,
    }
}

/// Extract context around a pattern match
fn extract_context(content: &str, pattern: &str) -> String {
    let pattern_lower = pattern.to_lowercase();
    if let Some(pos) = content.find(&pattern_lower) {
        let start = pos.saturating_sub(30);
        let end = (pos + pattern.len() + 30).min(content.len());
        format!("...{}...", &content[start..end])
    } else {
        String::new()
    }
}

/// Build a correction message to send back to the LLM
fn build_correction_message(detected: &[ForbiddenCommand]) -> String {
    let mut msg = String::from(
        "⚠️ ANTI-HALLUCINATION CORRECTION REQUIRED\n\n\
         Your response suggested CLI commands instead of using tools. \
         This is NOT allowed. You MUST use the native tools.\n\n\
         Detected violations:\n"
    );
    
    for cmd in detected {
        msg.push_str(&format!(
            "- You suggested `{}` → USE `{}` instead\n",
            cmd.pattern, cmd.alternative
        ));
    }
    
    msg.push_str(
        "\nPlease RETRY your response:\n\
         1. Do NOT suggest any CLI commands\n\
         2. CALL the appropriate tool directly\n\
         3. Report the ACTUAL tool result\n\n\
         Remember: You have 200+ native tools. Use them!"
    );
    
    msg
}

/// Get a list of forbidden command patterns (for logging/debugging)
pub fn get_forbidden_patterns() -> Vec<(&'static str, &'static str)> {
    FORBIDDEN_PATTERNS.to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_detects_ovs_vsctl() {
        let content = "You can run `ovs-vsctl add-br br0` to create the bridge";
        let check = check_for_forbidden_commands(content);
        assert!(!check.detected.is_empty());
        assert!(check.should_reject);
    }
    
    #[test]
    fn test_detects_systemctl() {
        let content = "Try running systemctl restart gbr-xray";
        let check = check_for_forbidden_commands(content);
        assert!(!check.detected.is_empty());
        assert!(check.should_reject);
    }
    
    #[test]
    fn test_allows_tool_names() {
        let content = "I'll use ovs_create_bridge to create the bridge";
        let check = check_for_forbidden_commands(content);
        assert!(check.detected.is_empty());
    }
    
    #[test]
    fn test_detects_suggestion_language() {
        let content = "Here's the command to use: ip addr show";
        let check = check_for_forbidden_commands(content);
        assert!(check.should_reject);
    }
}
