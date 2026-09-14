//! Tool Groups - Granular, domain-specific tool sets
//!
//! Groups are designed to be ~5 tools each for flexibility.
//! Mix and match to create custom configurations under any limit.
//!
//! ## Security Levels
//!
//! | Level | Description | API Key Required |
//! |-------|-------------|------------------|
//! | public | Safe read-only tools | No |
//! | standard | Normal operations | No |
//! | elevated | System modifications | Optional |
//! | restricted | Dangerous commands | **YES** |
//!
//! ## Domain Groups
//!
//! - **Core**: Essential tools (respond, info)
//! - **DevOps**: Infrastructure, deployment
//! - **Security**: Auth, SSO, secrets
//! - **Business**: Marketing, HR, analytics
//! - **System**: Restricted admin commands

use op_core::security::{AccessZone, NetworkConfig, SecurityLevel};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use tracing::info;

/// A group of related tools (~5 tools each for granularity)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolGroup {
    /// Group identifier
    pub id: String,
    /// Human-readable name
    pub name: String,
    /// Description of what this group provides
    pub description: String,
    /// Domain category (core, devops, security, business, system)
    pub domain: String,
    /// Tool name patterns (exact or wildcard like "systemd_*")
    pub patterns: Vec<String>,
    /// Namespace filter
    pub namespace: Option<String>,
    /// Category filter
    pub category: Option<String>,
    /// Estimated tool count (~5 for granularity)
    pub estimated_count: usize,
    /// Priority (higher = more important)
    pub priority: i32,
    /// Dependencies (other groups required)
    #[serde(default)]
    pub dependencies: Vec<String>,
    /// Whether enabled by default
    #[serde(default)]
    pub default_enabled: bool,
    /// Security level
    #[serde(default)]
    pub security: SecurityLevel,
    /// Tags for filtering
    #[serde(default)]
    pub tags: Vec<String>,
}

impl ToolGroup {
    pub fn new(id: &str, name: &str, description: &str) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            description: description.to_string(),
            domain: "core".to_string(),
            patterns: vec![],
            namespace: None,
            category: None,
            estimated_count: 5,
            priority: 50,
            dependencies: vec![],
            default_enabled: false,
            security: SecurityLevel::Standard,
            tags: vec![],
        }
    }

    pub fn domain(mut self, domain: &str) -> Self {
        self.domain = domain.to_string();
        self
    }

    pub fn patterns(mut self, patterns: Vec<&str>) -> Self {
        self.patterns = patterns.into_iter().map(String::from).collect();
        self
    }

    pub fn namespace(mut self, ns: &str) -> Self {
        self.namespace = Some(ns.to_string());
        self
    }

    pub fn category(mut self, cat: &str) -> Self {
        self.category = Some(cat.to_string());
        self
    }

    pub fn count(mut self, count: usize) -> Self {
        self.estimated_count = count;
        self
    }

    pub fn priority(mut self, priority: i32) -> Self {
        self.priority = priority;
        self
    }

    pub fn depends_on(mut self, deps: Vec<&str>) -> Self {
        self.dependencies = deps.into_iter().map(String::from).collect();
        self
    }

    pub fn default_on(mut self) -> Self {
        self.default_enabled = true;
        self
    }

    pub fn security_level(mut self, level: SecurityLevel) -> Self {
        self.security = level;
        self
    }

    pub fn restricted(mut self) -> Self {
        self.security = SecurityLevel::Restricted;
        self
    }

    pub fn tags(mut self, tags: Vec<&str>) -> Self {
        self.tags = tags.into_iter().map(String::from).collect();
        self
    }

    /// Check if a tool matches this group
    pub fn matches_tool(
        &self,
        tool_name: &str,
        tool_namespace: Option<&str>,
        tool_category: Option<&str>,
    ) -> bool {
        if let Some(ns) = &self.namespace {
            if tool_namespace != Some(ns.as_str()) {
                return false;
            }
        }

        if let Some(cat) = &self.category {
            if tool_category != Some(cat.as_str()) {
                return false;
            }
        }

        if self.patterns.is_empty() {
            return self.namespace.is_some() || self.category.is_some();
        }

        for pattern in &self.patterns {
            if pattern.ends_with('*') {
                let prefix = &pattern[..pattern.len() - 1];
                if tool_name.starts_with(prefix) {
                    return true;
                }
            } else if tool_name == pattern {
                return true;
            }
        }

        false
    }
}

/// Manager for tool groups with IP-based access control
#[derive(Debug, Clone)]
pub struct ToolGroups {
    groups: HashMap<String, ToolGroup>,
    enabled: HashSet<String>,
    max_tools: usize,
    /// Client's access zone (based on IP)
    access_zone: AccessZone,
    /// Client IP address (for logging)
    client_ip: Option<String>,
    /// Network configuration for trusted ranges
    network_config: NetworkConfig,
}

impl ToolGroups {
    pub fn new() -> Self {
        let mut manager = Self {
            groups: HashMap::new(),
            enabled: HashSet::new(),
            max_tools: 40,
            access_zone: AccessZone::Localhost, // Default to localhost for CLI
            client_ip: None,
            network_config: NetworkConfig::default(),
        };

        for group in builtin_groups() {
            if group.default_enabled {
                manager.enabled.insert(group.id.clone());
            }
            manager.groups.insert(group.id.clone(), group);
        }

        manager
    }

    pub fn with_limit(mut self, max: usize) -> Self {
        self.max_tools = max;
        self
    }

    /// Set network configuration for trusted networks
    pub fn with_network_config(mut self, config: NetworkConfig) -> Self {
        self.network_config = config;
        self
    }

    /// Add trusted network prefix (e.g., "10.50." for Netmaker)
    pub fn trust_network(mut self, prefix: &str) -> Self {
        self.network_config = self.network_config.trust_prefix(prefix);
        self
    }

    /// Set access zone from client IP address
    pub fn from_ip(mut self, ip: &str) -> Self {
        self.access_zone = AccessZone::from_ip_with_config(ip, &self.network_config);
        self.client_ip = Some(ip.to_string());
        info!("🌐 Client IP: {} -> {}", ip, self.access_zone.description());
        self
    }

    /// Set access zone directly
    pub fn with_zone(mut self, zone: AccessZone) -> Self {
        self.access_zone = zone;
        self
    }

    /// Get current access zone
    pub fn access_zone(&self) -> AccessZone {
        self.access_zone
    }

    /// Check if client can access a security level
    pub fn can_access(&self, level: SecurityLevel) -> bool {
        self.access_zone.can_access(level)
    }

    /// Enable a group (checks IP-based security)
    pub fn enable(&mut self, group_id: &str) -> Result<(), String> {
        let group_info = match self.groups.get(group_id) {
            Some(g) => (
                g.estimated_count,
                g.dependencies.clone(),
                g.security,
                g.name.clone(),
            ),
            None => return Err(format!("Unknown group: {}", group_id)),
        };

        let (estimated_count, dependencies, security, name) = group_info;

        // Check IP-based access
        if !self.access_zone.can_access(security) {
            let required = match security {
                SecurityLevel::Restricted => "localhost (127.0.0.1)",
                SecurityLevel::Elevated => "localhost or private network",
                _ => "any",
            };
            return Err(format!(
                "Group '{}' ({:?}) requires {} access. Your zone: {}",
                name,
                security,
                required,
                self.access_zone.description()
            ));
        }

        let current_count = self.estimated_tool_count();
        if current_count + estimated_count > self.max_tools {
            return Err(format!(
                "Cannot enable '{}' ({} tools) - exceeds limit ({} + {} > {})",
                group_id, estimated_count, current_count, estimated_count, self.max_tools
            ));
        }

        // Enable dependencies first
        for dep in dependencies {
            if !self.enabled.contains(&dep) {
                self.enable(&dep)?;
            }
        }

        self.enabled.insert(group_id.to_string());
        info!(
            "✅ Enabled group '{}' (~{} tools)",
            group_id, estimated_count
        );
        Ok(())
    }

    /// Try to enable (returns bool instead of Result)
    pub fn try_enable(&mut self, group_id: &str) -> bool {
        self.enable(group_id).is_ok()
    }

    pub fn disable(&mut self, group_id: &str) {
        self.enabled.remove(group_id);
    }

    pub fn estimated_tool_count(&self) -> usize {
        self.enabled
            .iter()
            .filter_map(|id| self.groups.get(id))
            .map(|g| g.estimated_count)
            .sum()
    }

    pub fn remaining_capacity(&self) -> usize {
        self.max_tools.saturating_sub(self.estimated_tool_count())
    }

    pub fn should_include(
        &self,
        tool_name: &str,
        namespace: Option<&str>,
        category: Option<&str>,
    ) -> bool {
        for group_id in &self.enabled {
            if let Some(group) = self.groups.get(group_id) {
                if group.matches_tool(tool_name, namespace, category) {
                    return true;
                }
            }
        }
        false
    }

    /// List groups by domain
    pub fn list_by_domain(&self, domain: &str) -> Vec<&ToolGroup> {
        self.groups
            .values()
            .filter(|g| g.domain == domain)
            .collect()
    }

    /// List all groups with status
    pub fn list_all(&self) -> Vec<GroupStatus> {
        let mut result: Vec<_> = self
            .groups
            .values()
            .map(|g| GroupStatus {
                id: g.id.clone(),
                name: g.name.clone(),
                description: g.description.clone(),
                domain: g.domain.clone(),
                estimated_count: g.estimated_count,
                enabled: self.enabled.contains(&g.id),
                security: g.security,
                requires_trusted: matches!(g.security, SecurityLevel::Restricted),
            })
            .collect();
        result.sort_by(|a, b| a.domain.cmp(&b.domain).then(b.enabled.cmp(&a.enabled)));
        result
    }

    /// Apply a preset
    pub fn apply_preset(&mut self, preset: &str) -> Result<(), String> {
        self.enabled.clear();

        let groups = match preset {
            "minimal" => vec!["respond", "info"],
            "safe" => vec!["respond", "info", "read", "search"],
            "developer" => vec!["respond", "info", "read", "write", "shell-safe", "git-read"],
            "sysadmin" => vec![
                "respond",
                "info",
                "read",
                "services",
                "network-info",
                "logs",
            ],
            "architect" => vec!["respond", "info", "dbus-intro", "services", "network-info"],
            "security" => vec!["respond", "info", "auth", "secrets", "audit"],
            "devops" => vec!["respond", "info", "deploy", "containers", "monitoring"],
            "full-safe" => vec![
                "respond",
                "info",
                "read",
                "search",
                "services",
                "network-info",
                "dbus-intro",
                "monitoring",
                "logs",
            ],
            _ => return Err(format!("Unknown preset: {}", preset)),
        };

        for group in groups {
            self.enable(group)?;
        }

        Ok(())
    }

    pub fn add_group(&mut self, group: ToolGroup) {
        self.groups.insert(group.id.clone(), group);
    }
}

impl Default for ToolGroups {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupStatus {
    pub id: String,
    pub name: String,
    pub description: String,
    pub domain: String,
    pub estimated_count: usize,
    pub enabled: bool,
    pub security: SecurityLevel,
    /// True if this group requires localhost or trusted network
    pub requires_trusted: bool,
}

/// Built-in granular tool groups (~5 tools each)
pub fn builtin_groups() -> Vec<ToolGroup> {
    vec![
        // =====================================================================
        // CORE DOMAIN - Essential tools
        // =====================================================================
        ToolGroup::new(
            "respond",
            "Respond",
            "Response tools for user communication",
        )
        .domain("core")
        .patterns(vec!["respond", "respond_to_user", "reply", "answer"])
        .count(3)
        .priority(100)
        .default_on()
        .security_level(SecurityLevel::Public),
        ToolGroup::new(
            "info",
            "System Info",
            "Basic system information (read-only)",
        )
        .domain("core")
        .patterns(vec![
            "system_info",
            "get_info",
            "whoami",
            "hostname",
            "uname",
        ])
        .count(5)
        .priority(95)
        .default_on()
        .security_level(SecurityLevel::Public),
        ToolGroup::new("help", "Help & Docs", "Documentation and help tools")
            .domain("core")
            .patterns(vec!["help", "man", "docs", "explain", "describe"])
            .count(4)
            .priority(90)
            .security_level(SecurityLevel::Public),
        // =====================================================================
        // FILE DOMAIN - File operations (split by permission)
        // =====================================================================
        ToolGroup::new("read", "File Read", "Read files and directories (safe)")
            .domain("files")
            .patterns(vec![
                "read_file",
                "cat",
                "head",
                "tail",
                "list_dir",
                "ls",
                "find",
            ])
            .count(6)
            .priority(85)
            .security_level(SecurityLevel::Public),
        ToolGroup::new("write", "File Write", "Write and modify files")
            .domain("files")
            .patterns(vec!["write_file", "create_file", "append", "touch"])
            .count(4)
            .priority(75)
            .depends_on(vec!["read"])
            .security_level(SecurityLevel::Standard),
        ToolGroup::new("file-manage", "File Management", "Move, copy, delete files")
            .domain("files")
            .patterns(vec!["mv", "cp", "rm", "mkdir", "rmdir", "chmod", "chown"])
            .count(6)
            .priority(70)
            .depends_on(vec!["read"])
            .security_level(SecurityLevel::Elevated),
        ToolGroup::new("search", "Search", "Search files and content")
            .domain("files")
            .patterns(vec!["grep", "find_files", "search", "locate", "which"])
            .count(5)
            .priority(80)
            .security_level(SecurityLevel::Public),
        // =====================================================================
        // SHELL DOMAIN - Command execution
        // =====================================================================
        ToolGroup::new(
            "shell-safe",
            "Shell (Safe)",
            "Safe shell commands (read-only)",
        )
        .domain("shell")
        .patterns(vec!["shell_read", "echo", "pwd", "env", "date"])
        .count(5)
        .priority(70)
        .security_level(SecurityLevel::Standard),
        ToolGroup::new("shell-exec", "Shell (Execute)", "Execute shell commands")
            .domain("shell")
            .patterns(vec!["shell_exec", "run_command", "exec"])
            .count(3)
            .priority(60)
            .depends_on(vec!["shell-safe"])
            .security_level(SecurityLevel::Elevated),
        ToolGroup::new("shell-root", "Shell (Root)", "Root/sudo shell commands")
            .domain("shell")
            .patterns(vec!["sudo", "su", "shell_root"])
            .count(3)
            .priority(10)
            .depends_on(vec!["shell-exec"])
            .restricted()
            .tags(vec!["dangerous", "requires-key"]),
        // =====================================================================
        // SYSTEMD DOMAIN - Service management
        // =====================================================================
        ToolGroup::new("services", "Services", "List and query services")
            .domain("systemd")
            .patterns(vec!["systemd_list", "service_status", "unit_status"])
            .count(4)
            .priority(75)
            .security_level(SecurityLevel::Public),
        ToolGroup::new(
            "service-control",
            "Service Control",
            "Start/stop/restart services",
        )
        .domain("systemd")
        .patterns(vec![
            "systemd_start",
            "systemd_stop",
            "systemd_restart",
            "systemd_reload",
        ])
        .count(4)
        .priority(65)
        .depends_on(vec!["services"])
        .security_level(SecurityLevel::Elevated),
        ToolGroup::new(
            "service-config",
            "Service Config",
            "Enable/disable services",
        )
        .domain("systemd")
        .patterns(vec!["systemd_enable", "systemd_disable", "systemd_mask"])
        .count(4)
        .priority(55)
        .depends_on(vec!["services"])
        .restricted()
        .tags(vec!["system-config"]),
        ToolGroup::new("journals", "Journals", "View systemd logs")
            .domain("systemd")
            .patterns(vec!["journalctl", "logs", "systemd_logs"])
            .count(3)
            .priority(70)
            .security_level(SecurityLevel::Public),
        // =====================================================================
        // NETWORK DOMAIN
        // =====================================================================
        ToolGroup::new(
            "network-info",
            "Network Info",
            "Network information (read-only)",
        )
        .domain("network")
        .patterns(vec!["ip_addr", "ifconfig", "route", "netstat", "ss"])
        .count(5)
        .priority(75)
        .security_level(SecurityLevel::Public),
        ToolGroup::new(
            "network-diag",
            "Network Diagnostics",
            "Ping, traceroute, DNS",
        )
        .domain("network")
        .patterns(vec!["ping", "traceroute", "dig", "nslookup", "curl"])
        .count(5)
        .priority(70)
        .security_level(SecurityLevel::Standard),
        ToolGroup::new(
            "network-config",
            "Network Config",
            "Configure network interfaces",
        )
        .domain("network")
        .patterns(vec!["ip_link", "ip_route", "interface_*"])
        .count(5)
        .priority(50)
        .restricted()
        .tags(vec!["network-admin"]),
        ToolGroup::new("firewall", "Firewall", "Firewall rules and policies")
            .domain("network")
            .patterns(vec!["iptables", "nft", "firewall_*", "ufw"])
            .count(5)
            .priority(40)
            .restricted()
            .tags(vec!["security", "network-admin"]),
        // =====================================================================
        // DBUS DOMAIN
        // =====================================================================
        ToolGroup::new("dbus-intro", "D-Bus Introspect", "D-Bus service discovery")
            .domain("dbus")
            .patterns(vec!["dbus_list", "dbus_introspect", "bus_list"])
            .count(4)
            .priority(70)
            .security_level(SecurityLevel::Public),
        ToolGroup::new("dbus-call", "D-Bus Call", "Call D-Bus methods")
            .domain("dbus")
            .patterns(vec!["dbus_call", "dbus_method", "bus_call"])
            .count(4)
            .priority(60)
            .depends_on(vec!["dbus-intro"])
            .security_level(SecurityLevel::Elevated),
        ToolGroup::new("dbus-monitor", "D-Bus Monitor", "Monitor D-Bus signals")
            .domain("dbus")
            .patterns(vec!["dbus_monitor", "dbus_watch", "bus_monitor"])
            .count(3)
            .priority(55)
            .security_level(SecurityLevel::Standard),
        // =====================================================================
        // MONITORING DOMAIN
        // =====================================================================
        ToolGroup::new("monitoring", "System Monitoring", "CPU, memory, disk usage")
            .domain("monitoring")
            .patterns(vec!["top", "htop", "free", "df", "du", "uptime"])
            .count(6)
            .priority(75)
            .security_level(SecurityLevel::Public),
        ToolGroup::new(
            "processes",
            "Process Management",
            "List and manage processes",
        )
        .domain("monitoring")
        .patterns(vec!["ps", "pgrep", "process_*"])
        .count(4)
        .priority(70)
        .security_level(SecurityLevel::Public),
        ToolGroup::new(
            "process-control",
            "Process Control",
            "Kill and signal processes",
        )
        .domain("monitoring")
        .patterns(vec!["kill", "pkill", "killall", "nice", "renice"])
        .count(5)
        .priority(50)
        .depends_on(vec!["processes"])
        .security_level(SecurityLevel::Elevated),
        ToolGroup::new("logs", "Log Viewing", "View system and application logs")
            .domain("monitoring")
            .patterns(vec!["tail_log", "view_log", "log_*", "dmesg"])
            .count(4)
            .priority(70)
            .security_level(SecurityLevel::Public),
        // =====================================================================
        // GIT DOMAIN
        // =====================================================================
        ToolGroup::new("git-read", "Git Read", "Git status, log, diff (read-only)")
            .domain("git")
            .patterns(vec!["git_status", "git_log", "git_diff", "git_show"])
            .count(5)
            .priority(70)
            .security_level(SecurityLevel::Public),
        ToolGroup::new("git-write", "Git Write", "Git add, commit, branch")
            .domain("git")
            .patterns(vec!["git_add", "git_commit", "git_branch", "git_checkout"])
            .count(5)
            .priority(65)
            .depends_on(vec!["git-read"])
            .security_level(SecurityLevel::Standard),
        ToolGroup::new("git-remote", "Git Remote", "Git push, pull, fetch")
            .domain("git")
            .patterns(vec!["git_push", "git_pull", "git_fetch", "git_clone"])
            .count(4)
            .priority(60)
            .depends_on(vec!["git-read"])
            .security_level(SecurityLevel::Elevated),
        // =====================================================================
        // DEVOPS DOMAIN
        // =====================================================================
        ToolGroup::new("containers", "Containers", "Container management (read)")
            .domain("devops")
            .patterns(vec!["container_list", "container_inspect", "docker_ps"])
            .count(4)
            .priority(65)
            .security_level(SecurityLevel::Standard),
        ToolGroup::new(
            "container-control",
            "Container Control",
            "Start/stop containers",
        )
        .domain("devops")
        .patterns(vec!["container_start", "container_stop", "docker_*"])
        .count(5)
        .priority(55)
        .depends_on(vec!["containers"])
        .security_level(SecurityLevel::Elevated),
        ToolGroup::new("deploy", "Deployment", "Deployment and release tools")
            .domain("devops")
            .patterns(vec!["deploy_*", "release_*", "rollback"])
            .count(5)
            .priority(60)
            .security_level(SecurityLevel::Elevated),
        ToolGroup::new(
            "k8s-read",
            "Kubernetes Read",
            "K8s get, describe (read-only)",
        )
        .domain("devops")
        .patterns(vec!["kubectl_get", "kubectl_describe", "k8s_list"])
        .count(4)
        .priority(60)
        .security_level(SecurityLevel::Standard),
        // =====================================================================
        // SECURITY DOMAIN
        // =====================================================================
        ToolGroup::new("auth", "Authentication", "Auth and identity tools")
            .domain("security")
            .patterns(vec!["auth_*", "login", "logout", "session_*"])
            .count(5)
            .priority(70)
            .security_level(SecurityLevel::Standard),
        ToolGroup::new("sso", "SSO", "Single sign-on integration")
            .domain("security")
            .patterns(vec!["sso_*", "oauth_*", "saml_*", "oidc_*"])
            .count(5)
            .priority(65)
            .security_level(SecurityLevel::Elevated),
        ToolGroup::new("secrets", "Secrets", "Secret and credential management")
            .domain("security")
            .patterns(vec!["secret_*", "vault_*", "credential_*"])
            .count(5)
            .priority(60)
            .restricted()
            .tags(vec!["sensitive"]),
        ToolGroup::new("audit", "Audit", "Security audit and compliance")
            .domain("security")
            .patterns(vec!["audit_*", "compliance_*", "scan_*"])
            .count(4)
            .priority(65)
            .security_level(SecurityLevel::Standard),
        ToolGroup::new("crypto", "Cryptography", "Encryption and signing")
            .domain("security")
            .patterns(vec!["encrypt_*", "decrypt_*", "sign_*", "verify_*"])
            .count(5)
            .priority(55)
            .security_level(SecurityLevel::Elevated),
        // =====================================================================
        // BUSINESS DOMAIN
        // =====================================================================
        ToolGroup::new("analytics", "Analytics", "Data and analytics queries")
            .domain("business")
            .patterns(vec!["analytics_*", "report_*", "metrics_*"])
            .count(5)
            .priority(60)
            .security_level(SecurityLevel::Standard),
        ToolGroup::new("marketing", "Marketing", "Marketing automation tools")
            .domain("business")
            .patterns(vec!["marketing_*", "campaign_*", "email_*"])
            .count(5)
            .priority(50)
            .security_level(SecurityLevel::Standard),
        ToolGroup::new("hr", "HR", "Human resources tools")
            .domain("business")
            .patterns(vec!["hr_*", "employee_*", "payroll_*"])
            .count(5)
            .priority(50)
            .security_level(SecurityLevel::Elevated)
            .tags(vec!["pii", "sensitive"]),
        ToolGroup::new("crm", "CRM", "Customer relationship management")
            .domain("business")
            .patterns(vec!["crm_*", "customer_*", "contact_*"])
            .count(5)
            .priority(55)
            .security_level(SecurityLevel::Standard),
        ToolGroup::new("finance", "Finance", "Financial and billing tools")
            .domain("business")
            .patterns(vec!["finance_*", "billing_*", "invoice_*"])
            .count(5)
            .priority(50)
            .security_level(SecurityLevel::Elevated)
            .tags(vec!["sensitive"]),
        // =====================================================================
        // ARCHITECT DOMAIN
        // =====================================================================
        ToolGroup::new(
            "architect-view",
            "Architecture View",
            "View system architecture",
        )
        .domain("architect")
        .patterns(vec!["arch_*", "topology_*", "diagram_*"])
        .count(4)
        .priority(65)
        .security_level(SecurityLevel::Public),
        ToolGroup::new("dependencies", "Dependencies", "Dependency analysis")
            .domain("architect")
            .patterns(vec!["deps_*", "dependency_*", "import_*"])
            .count(4)
            .priority(60)
            .security_level(SecurityLevel::Public),
        ToolGroup::new("performance", "Performance", "Performance analysis tools")
            .domain("architect")
            .patterns(vec!["perf_*", "benchmark_*", "profile_*"])
            .count(5)
            .priority(60)
            .security_level(SecurityLevel::Standard),
        // =====================================================================
        // DATABASE DOMAIN
        // =====================================================================
        ToolGroup::new("db-read", "Database Read", "Query databases (read-only)")
            .domain("database")
            .patterns(vec!["db_query", "sql_select", "db_list"])
            .count(4)
            .priority(65)
            .security_level(SecurityLevel::Standard),
        ToolGroup::new("db-write", "Database Write", "Modify database data")
            .domain("database")
            .patterns(vec!["db_insert", "db_update", "db_delete", "sql_*"])
            .count(5)
            .priority(50)
            .depends_on(vec!["db-read"])
            .security_level(SecurityLevel::Elevated),
        ToolGroup::new("db-admin", "Database Admin", "Database administration")
            .domain("database")
            .patterns(vec!["db_create", "db_drop", "db_migrate", "db_backup"])
            .count(5)
            .priority(40)
            .depends_on(vec!["db-read"])
            .restricted()
            .tags(vec!["database-admin"]),
        // =====================================================================
        // SYSTEM/RESTRICTED DOMAIN - Dangerous commands requiring API key
        // =====================================================================
        ToolGroup::new("system-power", "System Power", "Reboot, shutdown, halt")
            .domain("system")
            .patterns(vec!["reboot", "shutdown", "halt", "poweroff"])
            .count(4)
            .priority(5)
            .restricted()
            .tags(vec!["dangerous", "system-critical"]),
        ToolGroup::new(
            "system-config",
            "System Config",
            "System configuration changes",
        )
        .domain("system")
        .patterns(vec!["sysctl", "modprobe", "system_config_*"])
        .count(5)
        .priority(5)
        .restricted()
        .tags(vec!["dangerous", "system-critical"]),
        ToolGroup::new(
            "disk-format",
            "Disk Format",
            "Disk partitioning and formatting",
        )
        .domain("system")
        .patterns(vec!["fdisk", "mkfs", "parted", "mount", "umount"])
        .count(5)
        .priority(5)
        .restricted()
        .tags(vec!["dangerous", "data-loss"]),
        ToolGroup::new("user-admin", "User Admin", "User and group management")
            .domain("system")
            .patterns(vec!["useradd", "userdel", "usermod", "groupadd", "passwd"])
            .count(5)
            .priority(10)
            .restricted()
            .tags(vec!["user-management"]),
        // =====================================================================
        // OVS DOMAIN (for your networking use case)
        // =====================================================================
        ToolGroup::new("ovs-info", "OVS Info", "OVS bridge and port information")
            .domain("ovs")
            .patterns(vec!["ovs_list", "ovs_show", "ovsdb_query"])
            .count(4)
            .priority(60)
            .security_level(SecurityLevel::Standard),
        ToolGroup::new(
            "ovs-config",
            "OVS Config",
            "Configure OVS bridges and ports",
        )
        .domain("ovs")
        .patterns(vec!["ovs_add_*", "ovs_del_*", "ovs_set_*"])
        .count(5)
        .priority(50)
        .depends_on(vec!["ovs-info"])
        .security_level(SecurityLevel::Elevated),
        // =====================================================================
        // AGENTS DOMAIN
        // =====================================================================
        ToolGroup::new("agents-safe", "Agents (Safe)", "Safe agent operations")
            .domain("agents")
            .patterns(vec!["agent_list", "agent_status", "agent_describe"])
            .count(4)
            .priority(55)
            .security_level(SecurityLevel::Standard),
        ToolGroup::new("agents-invoke", "Agents Invoke", "Invoke agent operations")
            .domain("agents")
            .patterns(vec!["invoke_agent", "agent_*"])
            .count(5)
            .priority(50)
            .depends_on(vec!["agents-safe"])
            .security_level(SecurityLevel::Elevated),
    ]
}

/// Built-in presets (curated group combinations)
pub fn builtin_presets() -> Vec<GroupPreset> {
    vec![
        GroupPreset {
            id: "minimal".into(),
            name: "Minimal".into(),
            description: "Only response tools (3 tools)".into(),
            groups: vec!["respond".into()],
            estimated_total: 3,
            requires_localhost: false,
        },
        GroupPreset {
            id: "safe".into(),
            name: "Safe".into(),
            description: "Read-only, no modifications (18 tools)".into(),
            groups: vec![
                "respond".into(),
                "info".into(),
                "read".into(),
                "search".into(),
            ],
            estimated_total: 18,
            requires_localhost: false,
        },
        GroupPreset {
            id: "developer".into(),
            name: "Developer".into(),
            description: "Developer workflow (28 tools)".into(),
            groups: vec![
                "respond".into(),
                "info".into(),
                "read".into(),
                "write".into(),
                "shell-safe".into(),
                "git-read".into(),
            ],
            estimated_total: 28,
            requires_localhost: false,
        },
        GroupPreset {
            id: "sysadmin".into(),
            name: "System Admin".into(),
            description: "System administration (32 tools)".into(),
            groups: vec![
                "respond".into(),
                "info".into(),
                "read".into(),
                "services".into(),
                "network-info".into(),
                "logs".into(),
                "monitoring".into(),
            ],
            estimated_total: 32,
            requires_localhost: false,
        },
        GroupPreset {
            id: "architect".into(),
            name: "Architect".into(),
            description: "Architecture analysis (26 tools)".into(),
            groups: vec![
                "respond".into(),
                "info".into(),
                "dbus-intro".into(),
                "services".into(),
                "network-info".into(),
                "architect-view".into(),
            ],
            estimated_total: 26,
            requires_localhost: false,
        },
        GroupPreset {
            id: "security".into(),
            name: "Security".into(),
            description: "Security operations (24 tools)".into(),
            groups: vec![
                "respond".into(),
                "info".into(),
                "auth".into(),
                "audit".into(),
                "logs".into(),
            ],
            estimated_total: 24,
            requires_localhost: false,
        },
        GroupPreset {
            id: "full-admin".into(),
            name: "Full Admin".into(),
            description: "Full admin - localhost or Netmaker/Tailscale only".into(),
            groups: vec![
                "respond".into(),
                "info".into(),
                "read".into(),
                "write".into(),
                "shell-exec".into(),
                "shell-root".into(),
                "service-control".into(),
                "service-config".into(),
                "network-config".into(),
            ],
            estimated_total: 40,
            requires_localhost: true,
        },
    ]
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupPreset {
    pub id: String,
    pub name: String,
    pub description: String,
    pub groups: Vec<String>,
    pub estimated_total: usize,
    /// Requires localhost or trusted mesh network
    pub requires_localhost: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_granular_groups() {
        let groups = builtin_groups();

        // Most groups should be ~5 tools
        for group in &groups {
            assert!(
                group.estimated_count <= 6,
                "Group '{}' has {} tools, should be <=6",
                group.id,
                group.estimated_count
            );
        }
    }

    #[test]
    fn test_restricted_requires_localhost() {
        // From public IP - should fail for restricted
        let mut groups = ToolGroups::new().with_limit(40).from_ip("8.8.8.8");
        let result = groups.enable("shell-root");
        assert!(result.is_err());

        // From localhost - should succeed
        let mut groups2 = ToolGroups::new().with_limit(40).from_ip("127.0.0.1");
        assert!(groups2.enable("shell-root").is_ok());
    }

    #[test]
    fn test_elevated_requires_private() {
        // From public IP - should fail for elevated
        let mut groups = ToolGroups::new().with_limit(40).from_ip("8.8.8.8");
        let result = groups.enable("shell-exec");
        assert!(result.is_err());

        // From private network - should succeed
        let mut groups2 = ToolGroups::new().with_limit(40).from_ip("192.168.1.100");
        assert!(groups2.enable("shell-exec").is_ok());
    }

    #[test]
    fn test_presets_under_limit() {
        for preset in builtin_presets() {
            if !preset.requires_localhost {
                assert!(
                    preset.estimated_total <= 40,
                    "Preset '{}' has {} tools, should be <=40",
                    preset.id,
                    preset.estimated_total
                );
            }
        }
    }

    #[test]
    fn test_domains() {
        let groups = ToolGroups::new();

        // Should have groups in each domain
        let domains = [
            "core", "files", "shell", "systemd", "network", "security", "business", "system",
        ];
        for domain in domains {
            let domain_groups = groups.list_by_domain(domain);
            assert!(!domain_groups.is_empty(), "No groups in domain: {}", domain);
        }
    }
}
