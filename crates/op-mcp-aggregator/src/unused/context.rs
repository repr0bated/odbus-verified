//! Context-Aware Tool Loading
//!
//! Dynamically suggests and enables tool groups based on conversation context.
//! This bridges Compact Mode (lazy loading) with Tool Groups (domain organization).
//!
//! ## How It Works
//!
//! 1. **Analyze Context**: Extract signals from messages, files, commands
//! 2. **Match Groups**: Map context signals to relevant tool groups  
//! 3. **Suggest/Auto-Enable**: Recommend or auto-enable groups within limit
//!
//! ## Context Signals
//!
//! - File paths: `.service` → systemd, `.py` → python, `Dockerfile` → containers
//! - Keywords: "nginx" → services, "database" → db-read
//! - Commands: Recent `git` commands → git-read/git-write
//! - D-Bus paths: Specific services → dbus-intro
//! - Intent: "restart", "stop" → service-control

use crate::groups::{ToolGroups, ToolGroup, SecurityLevel, AccessZone};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use tracing::{debug, info};

/// Context signals extracted from conversation
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ConversationContext {
    /// File paths mentioned or being worked on
    pub files: Vec<String>,
    /// Keywords extracted from messages
    pub keywords: Vec<String>,
    /// Commands recently executed
    pub recent_commands: Vec<String>,
    /// D-Bus services mentioned
    pub dbus_services: Vec<String>,
    /// Detected intent (e.g., "read", "modify", "debug", "deploy")
    pub intent: Option<String>,
    /// Explicit domain request (e.g., user says "I'm working on networking")
    pub explicit_domain: Option<String>,
    /// Current working directory
    pub cwd: Option<String>,
    /// Open files in editor
    pub open_files: Vec<String>,
}

impl ConversationContext {
    pub fn new() -> Self {
        Self::default()
    }
    
    /// Add a file path to context
    pub fn with_file(mut self, path: &str) -> Self {
        self.files.push(path.to_string());
        self
    }
    
    /// Add keywords from a message
    pub fn with_keywords(mut self, keywords: Vec<&str>) -> Self {
        self.keywords.extend(keywords.into_iter().map(String::from));
        self
    }
    
    /// Add a recent command
    pub fn with_command(mut self, cmd: &str) -> Self {
        self.recent_commands.push(cmd.to_string());
        self
    }
    
    /// Set intent
    pub fn with_intent(mut self, intent: &str) -> Self {
        self.intent = Some(intent.to_string());
        self
    }
    
    /// Set explicit domain
    pub fn for_domain(mut self, domain: &str) -> Self {
        self.explicit_domain = Some(domain.to_string());
        self
    }
    
    /// Extract context from a user message
    pub fn from_message(message: &str) -> Self {
        let mut ctx = Self::new();
        let lower = message.to_lowercase();
        
        // Extract file paths
        for word in message.split_whitespace() {
            if word.contains('/') || word.contains('.') {
                if looks_like_path(word) {
                    ctx.files.push(word.trim_matches(|c| c == '"' || c == '\'').to_string());
                }
            }
        }
        
        // Extract keywords
        let keywords: Vec<&str> = CONTEXT_KEYWORDS.iter()
            .filter(|&&kw| lower.contains(kw))
            .copied()
            .collect();
        ctx.keywords = keywords.into_iter().map(String::from).collect();
        
        // Detect intent
        ctx.intent = detect_intent(&lower);
        
        // Detect explicit domain
        ctx.explicit_domain = detect_domain(&lower);
        
        ctx
    }
    
    /// Merge with another context
    pub fn merge(&mut self, other: &ConversationContext) {
        self.files.extend(other.files.clone());
        self.keywords.extend(other.keywords.clone());
        self.recent_commands.extend(other.recent_commands.clone());
        self.dbus_services.extend(other.dbus_services.clone());
        if other.intent.is_some() {
            self.intent = other.intent.clone();
        }
        if other.explicit_domain.is_some() {
            self.explicit_domain = other.explicit_domain.clone();
        }
    }
}

/// Keywords that signal certain domains
const CONTEXT_KEYWORDS: &[&str] = &[
    // Systemd
    "service", "systemd", "unit", "daemon", "journalctl", "systemctl",
    // Network
    "network", "ip", "interface", "bridge", "route", "dns", "firewall",
    // Git
    "git", "commit", "branch", "merge", "pull", "push",
    // Containers
    "docker", "container", "kubernetes", "k8s", "pod", "deployment",
    // Database
    "database", "sql", "query", "table", "postgresql", "mysql", "mongodb",
    // Files
    "file", "directory", "folder", "read", "write", "create", "delete",
    // Security
    "security", "auth", "password", "secret", "certificate", "ssl", "tls",
    // D-Bus
    "dbus", "bus", "introspect",
    // OVS
    "ovs", "openvswitch", "vswitch",
];

fn looks_like_path(s: &str) -> bool {
    let trimmed = s.trim_matches(|c| c == '"' || c == '\'' || c == '`');
    trimmed.starts_with('/') || 
    trimmed.starts_with("./") ||
    trimmed.starts_with("../") ||
    trimmed.starts_with("~") ||
    (trimmed.contains('.') && !trimmed.contains(' '))
}

fn detect_intent(message: &str) -> Option<String> {
    if message.contains("restart") || message.contains("stop") || message.contains("start") || message.contains("enable") {
        Some("control".to_string())
    } else if message.contains("deploy") || message.contains("release") || message.contains("rollback") {
        Some("deploy".to_string())
    } else if message.contains("debug") || message.contains("troubleshoot") || message.contains("investigate") {
        Some("debug".to_string())
    } else if message.contains("monitor") || message.contains("watch") || message.contains("track") {
        Some("monitor".to_string())
    } else if message.contains("configure") || message.contains("setup") || message.contains("install") {
        Some("configure".to_string())
    } else if message.contains("list") || message.contains("show") || message.contains("get") || message.contains("read") {
        Some("read".to_string())
    } else if message.contains("create") || message.contains("write") || message.contains("add") || message.contains("modify") {
        Some("write".to_string())
    } else {
        None
    }
}

fn detect_domain(message: &str) -> Option<String> {
    // Explicit domain mentions
    if message.contains("working on network") || message.contains("networking") {
        Some("network".to_string())
    } else if message.contains("working on systemd") || message.contains("services") {
        Some("systemd".to_string())
    } else if message.contains("working on database") || message.contains("sql") {
        Some("database".to_string())
    } else if message.contains("working on docker") || message.contains("containers") {
        Some("devops".to_string())
    } else if message.contains("working on security") {
        Some("security".to_string())
    } else if message.contains("working on git") {
        Some("git".to_string())
    } else {
        None
    }
}

/// Suggested groups based on context analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextSuggestion {
    /// Suggested group ID
    pub group_id: String,
    /// Group name for display
    pub group_name: String,
    /// Why this was suggested
    pub reason: String,
    /// Confidence score (0-100)
    pub confidence: u8,
    /// Tools this would add
    pub estimated_tools: usize,
    /// Auto-enable recommended?
    pub auto_enable: bool,
}

/// Context-aware tool manager
#[derive(Debug, Clone)]
pub struct ContextAwareTools {
    /// Accumulated context
    context: ConversationContext,
    /// File extension → group mapping
    file_mappings: HashMap<String, Vec<String>>,
    /// Keyword → group mapping
    keyword_mappings: HashMap<String, Vec<String>>,
    /// Intent → group mapping
    intent_mappings: HashMap<String, Vec<String>>,
    /// Maximum tools limit
    max_tools: usize,
    /// Currently enabled groups
    enabled: HashSet<String>,
}

impl ContextAwareTools {
    pub fn new(max_tools: usize) -> Self {
        Self {
            context: ConversationContext::new(),
            file_mappings: build_file_mappings(),
            keyword_mappings: build_keyword_mappings(),
            intent_mappings: build_intent_mappings(),
            max_tools,
            enabled: HashSet::new(),
        }
    }
    
    /// Update context from a message
    pub fn observe_message(&mut self, message: &str) {
        let new_ctx = ConversationContext::from_message(message);
        self.context.merge(&new_ctx);
        debug!("Updated context: {:?}", self.context);
    }
    
    /// Update context from file paths being edited
    pub fn observe_files(&mut self, files: &[String]) {
        self.context.files.extend(files.iter().cloned());
    }
    
    /// Update context from a command execution
    pub fn observe_command(&mut self, command: &str) {
        self.context.recent_commands.push(command.to_string());
        
        // Extract command type for keyword matching
        if let Some(cmd) = command.split_whitespace().next() {
            self.context.keywords.push(cmd.to_string());
        }
    }
    
    /// Suggest tool groups based on current context
    pub fn suggest_groups(&self, tool_groups: &ToolGroups) -> Vec<ContextSuggestion> {
        let mut suggestions: HashMap<String, ContextSuggestion> = HashMap::new();
        
        // 1. File-based suggestions
        for file in &self.context.files {
            let ext = file.rsplit('.').next().unwrap_or("");
            if let Some(groups) = self.file_mappings.get(ext) {
                for group_id in groups {
                    let entry = suggestions.entry(group_id.clone()).or_insert_with(|| {
                        ContextSuggestion {
                            group_id: group_id.clone(),
                            group_name: group_id.clone(), // Will be updated
                            reason: String::new(),
                            confidence: 0,
                            estimated_tools: 0,
                            auto_enable: false,
                        }
                    });
                    entry.confidence = entry.confidence.saturating_add(30);
                    if entry.reason.is_empty() {
                        entry.reason = format!("File '{}' suggests {}", file, group_id);
                    }
                }
            }
        }
        
        // 2. Keyword-based suggestions
        for keyword in &self.context.keywords {
            if let Some(groups) = self.keyword_mappings.get(keyword.to_lowercase().as_str()) {
                for group_id in groups {
                    let entry = suggestions.entry(group_id.clone()).or_insert_with(|| {
                        ContextSuggestion {
                            group_id: group_id.clone(),
                            group_name: group_id.clone(),
                            reason: String::new(),
                            confidence: 0,
                            estimated_tools: 0,
                            auto_enable: false,
                        }
                    });
                    entry.confidence = entry.confidence.saturating_add(25);
                    if entry.reason.is_empty() {
                        entry.reason = format!("Keyword '{}' suggests {}", keyword, group_id);
                    }
                }
            }
        }
        
        // 3. Intent-based suggestions
        if let Some(intent) = &self.context.intent {
            if let Some(groups) = self.intent_mappings.get(intent.as_str()) {
                for group_id in groups {
                    let entry = suggestions.entry(group_id.clone()).or_insert_with(|| {
                        ContextSuggestion {
                            group_id: group_id.clone(),
                            group_name: group_id.clone(),
                            reason: String::new(),
                            confidence: 0,
                            estimated_tools: 0,
                            auto_enable: false,
                        }
                    });
                    entry.confidence = entry.confidence.saturating_add(20);
                    if entry.reason.is_empty() {
                        entry.reason = format!("Intent '{}' suggests {}", intent, group_id);
                    }
                }
            }
        }
        
        // 4. Explicit domain request (highest confidence)
        if let Some(domain) = &self.context.explicit_domain {
            for group in tool_groups.list_by_domain(domain) {
                let entry = suggestions.entry(group.id.clone()).or_insert_with(|| {
                    ContextSuggestion {
                        group_id: group.id.clone(),
                        group_name: group.name.clone(),
                        reason: String::new(),
                        confidence: 0,
                        estimated_tools: 0,
                        auto_enable: false,
                    }
                });
                entry.confidence = entry.confidence.saturating_add(50);
                entry.auto_enable = true;
                if entry.reason.is_empty() {
                    entry.reason = format!("Working on {} domain", domain);
                }
            }
        }
        
        // Update group metadata and filter
        let mut result: Vec<_> = suggestions.into_iter()
            .filter_map(|(id, mut suggestion)| {
                // Get actual group info
                let all_groups = tool_groups.list_all();
                if let Some(status) = all_groups.iter().find(|g| g.id == id) {
                    suggestion.group_name = status.name.clone();
                    suggestion.estimated_tools = status.estimated_count;
                    
                    // Skip if already enabled
                    if status.enabled || self.enabled.contains(&id) {
                        return None;
                    }
                    
                    // Auto-enable if high confidence
                    suggestion.auto_enable = suggestion.confidence >= 70;
                    
                    Some(suggestion)
                } else {
                    None
                }
            })
            .collect();
        
        // Sort by confidence
        result.sort_by(|a, b| b.confidence.cmp(&a.confidence));
        result.truncate(10); // Top 10 suggestions
        
        result
    }
    
    /// Auto-enable groups based on context (respects tool limit)
    pub fn auto_enable(&mut self, tool_groups: &mut ToolGroups) -> Vec<String> {
        let suggestions = self.suggest_groups(tool_groups);
        let mut enabled = Vec::new();
        
        for suggestion in suggestions {
            if suggestion.auto_enable && tool_groups.remaining_capacity() >= suggestion.estimated_tools {
                if tool_groups.try_enable(&suggestion.group_id) {
                    self.enabled.insert(suggestion.group_id.clone());
                    enabled.push(suggestion.group_id);
                    info!("🧠 Auto-enabled '{}' based on context: {}", 
                          suggestion.group_name, suggestion.reason);
                }
            }
        }
        
        enabled
    }
    
    /// Get current context
    pub fn context(&self) -> &ConversationContext {
        &self.context
    }
    
    /// Clear context (e.g., new conversation)
    pub fn clear_context(&mut self) {
        self.context = ConversationContext::new();
        self.enabled.clear();
    }
}

fn build_file_mappings() -> HashMap<String, Vec<String>> {
    let mut m = HashMap::new();
    
    // Systemd
    m.insert("service".into(), vec!["services".into(), "service-control".into()]);
    m.insert("socket".into(), vec!["services".into()]);
    m.insert("timer".into(), vec!["services".into()]);
    m.insert("target".into(), vec!["services".into()]);
    
    // Git
    m.insert("gitignore".into(), vec!["git-read".into()]);
    
    // Shell
    m.insert("sh".into(), vec!["shell-safe".into()]);
    m.insert("bash".into(), vec!["shell-safe".into()]);
    
    // Config files
    m.insert("json".into(), vec!["read".into()]);
    m.insert("yaml".into(), vec!["read".into()]);
    m.insert("yml".into(), vec!["read".into()]);
    m.insert("toml".into(), vec!["read".into()]);
    m.insert("conf".into(), vec!["read".into()]);
    
    // Docker
    m.insert("Dockerfile".into(), vec!["containers".into()]);
    m.insert("dockerignore".into(), vec!["containers".into()]);
    
    // Kubernetes
    m.insert("k8s".into(), vec!["k8s-read".into()]);
    
    // SQL
    m.insert("sql".into(), vec!["db-read".into()]);
    
    // Network
    m.insert("network".into(), vec!["network-info".into()]);
    m.insert("firewall".into(), vec!["firewall".into()]);
    
    // Logs
    m.insert("log".into(), vec!["logs".into()]);
    
    m
}

fn build_keyword_mappings() -> HashMap<String, Vec<String>> {
    let mut m = HashMap::new();
    
    // Systemd
    m.insert("systemd".into(), vec!["services".into(), "journals".into()]);
    m.insert("service".into(), vec!["services".into()]);
    m.insert("systemctl".into(), vec!["services".into(), "service-control".into()]);
    m.insert("journalctl".into(), vec!["journals".into()]);
    
    // Network
    m.insert("network".into(), vec!["network-info".into()]);
    m.insert("interface".into(), vec!["network-info".into()]);
    m.insert("bridge".into(), vec!["network-info".into(), "ovs-info".into()]);
    m.insert("firewall".into(), vec!["firewall".into()]);
    m.insert("dns".into(), vec!["network-diag".into()]);
    
    // Git
    m.insert("git".into(), vec!["git-read".into()]);
    m.insert("commit".into(), vec!["git-write".into()]);
    m.insert("branch".into(), vec!["git-read".into(), "git-write".into()]);
    
    // Containers
    m.insert("docker".into(), vec!["containers".into()]);
    m.insert("container".into(), vec!["containers".into()]);
    m.insert("kubernetes".into(), vec!["k8s-read".into()]);
    m.insert("k8s".into(), vec!["k8s-read".into()]);
    m.insert("pod".into(), vec!["k8s-read".into()]);
    
    // Database
    m.insert("database".into(), vec!["db-read".into()]);
    m.insert("sql".into(), vec!["db-read".into()]);
    m.insert("query".into(), vec!["db-read".into()]);
    m.insert("postgresql".into(), vec!["db-read".into()]);
    m.insert("mysql".into(), vec!["db-read".into()]);
    
    // D-Bus
    m.insert("dbus".into(), vec!["dbus-intro".into()]);
    m.insert("bus".into(), vec!["dbus-intro".into()]);
    m.insert("introspect".into(), vec!["dbus-intro".into()]);
    
    // OVS
    m.insert("ovs".into(), vec!["ovs-info".into()]);
    m.insert("openvswitch".into(), vec!["ovs-info".into()]);
    
    // Security
    m.insert("security".into(), vec!["auth".into(), "audit".into()]);
    m.insert("auth".into(), vec!["auth".into()]);
    m.insert("password".into(), vec!["auth".into()]);
    m.insert("secret".into(), vec!["secrets".into()]);
    
    // Monitoring
    m.insert("monitor".into(), vec!["monitoring".into()]);
    m.insert("cpu".into(), vec!["monitoring".into()]);
    m.insert("memory".into(), vec!["monitoring".into()]);
    m.insert("disk".into(), vec!["monitoring".into()]);
    
    // Files
    m.insert("file".into(), vec!["read".into()]);
    m.insert("read".into(), vec!["read".into()]);
    m.insert("search".into(), vec!["search".into()]);
    
    m
}

fn build_intent_mappings() -> HashMap<String, Vec<String>> {
    let mut m = HashMap::new();
    
    // Read operations
    m.insert("read".into(), vec!["read".into(), "info".into()]);
    
    // Write operations
    m.insert("write".into(), vec!["write".into()]);
    
    // Control operations
    m.insert("control".into(), vec!["service-control".into(), "process-control".into()]);
    
    // Debug operations
    m.insert("debug".into(), vec!["logs".into(), "journals".into(), "monitoring".into()]);
    
    // Deploy operations
    m.insert("deploy".into(), vec!["deploy".into(), "containers".into()]);
    
    // Monitor operations
    m.insert("monitor".into(), vec!["monitoring".into(), "logs".into()]);
    
    // Configure operations
    m.insert("configure".into(), vec!["service-config".into(), "network-config".into()]);
    
    m
}

/// Response format for context-aware suggestions
#[derive(Debug, Serialize, Deserialize)]
pub struct ContextResponse {
    /// Current accumulated context
    pub context: ConversationContext,
    /// Suggested groups
    pub suggestions: Vec<ContextSuggestion>,
    /// Auto-enabled groups
    pub auto_enabled: Vec<String>,
    /// Current tool count
    pub current_tools: usize,
    /// Remaining capacity
    pub remaining_capacity: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::groups::ToolGroups;
    
    #[test]
    fn test_context_from_message() {
        let ctx = ConversationContext::from_message(
            "I need to restart the nginx service and check the logs"
        );
        
        assert!(ctx.keywords.contains(&"service".to_string()));
        assert_eq!(ctx.intent, Some("control".to_string()));
    }
    
    #[test]
    fn test_file_path_detection() {
        let ctx = ConversationContext::from_message(
            "Please edit /etc/systemd/system/myapp.service"
        );
        
        assert!(ctx.files.iter().any(|f| f.contains("myapp.service")));
    }
    
    #[test]
    fn test_context_suggestions() {
        let groups = ToolGroups::new();
        let mut ctx_tools = ContextAwareTools::new(40);
        
        ctx_tools.observe_message("I want to check the systemd services");
        let suggestions = ctx_tools.suggest_groups(&groups);
        
        assert!(!suggestions.is_empty());
        assert!(suggestions.iter().any(|s| s.group_id == "services"));
    }
    
    #[test]
    fn test_explicit_domain() {
        let groups = ToolGroups::new();
        let mut ctx_tools = ContextAwareTools::new(40);
        
        ctx_tools.observe_message("I'm working on networking today");
        let suggestions = ctx_tools.suggest_groups(&groups);
        
        // Should suggest network groups with high confidence
        assert!(suggestions.iter().any(|s| s.group_id == "network-info"));
    }
    
    #[test]
    fn test_auto_enable() {
        let mut groups = ToolGroups::new().with_limit(40).from_ip("127.0.0.1");
        let mut ctx_tools = ContextAwareTools::new(40);
        
        // Strong signal should auto-enable
        ctx_tools.context.explicit_domain = Some("systemd".to_string());
        ctx_tools.context.intent = Some("read".to_string());
        ctx_tools.context.keywords.push("service".to_string());
        ctx_tools.context.keywords.push("systemctl".to_string());
        
        let enabled = ctx_tools.auto_enable(&mut groups);
        
        // Should have auto-enabled some systemd groups
        assert!(!enabled.is_empty());
    }
}
