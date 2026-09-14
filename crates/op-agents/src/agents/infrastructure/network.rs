//! Network Engineer Agent

use async_trait::async_trait;
use std::fs;
use std::net::ToSocketAddrs;

use crate::agents::base::{validation, AgentTask, AgentTrait, TaskResult};
use crate::security::SecurityProfile;

pub struct NetworkEngineerAgent {
    agent_id: String,
    profile: SecurityProfile,
}

impl NetworkEngineerAgent {
    pub fn new(agent_id: String) -> Self {
        Self {
            agent_id,
            profile: SecurityProfile::read_only_analysis(
                "network-engineer",
                vec!["ip", "ss", "netstat", "dig", "ping", "traceroute"],
            ),
        }
    }

    fn show_interfaces(&self) -> Result<String, String> {
        let mut output = String::new();

        if let Ok(dev) = fs::read_to_string("/proc/net/dev") {
            output.push_str("--- /proc/net/dev ---\n");
            output.push_str(&dev);
        }

        if let Ok(if_inet6) = fs::read_to_string("/proc/net/if_inet6") {
            output.push_str("\n--- /proc/net/if_inet6 ---\n");
            output.push_str(&if_inet6);
        }

        if output.is_empty() {
            return Err("Failed to read network interface data from /proc/net".to_string());
        }

        Ok(format!("Network interfaces:\n{}", output))
    }

    fn show_routes(&self) -> Result<String, String> {
        let mut output = String::new();

        if let Ok(route) = fs::read_to_string("/proc/net/route") {
            output.push_str("--- /proc/net/route ---\n");
            output.push_str(&route);
        }

        if let Ok(ipv6_route) = fs::read_to_string("/proc/net/ipv6_route") {
            output.push_str("\n--- /proc/net/ipv6_route ---\n");
            output.push_str(&ipv6_route);
        }

        if output.is_empty() {
            return Err("Failed to read route data from /proc/net".to_string());
        }

        Ok(format!("Routes:\n{}", output))
    }

    fn show_connections(&self) -> Result<String, String> {
        let mut output = String::new();
        let files = [
            ("/proc/net/tcp", "TCP"),
            ("/proc/net/tcp6", "TCP6"),
            ("/proc/net/udp", "UDP"),
            ("/proc/net/udp6", "UDP6"),
        ];

        for (path, label) in &files {
            if let Ok(content) = fs::read_to_string(path) {
                output.push_str(&format!("--- {} ({}) ---\n{}\n", label, path, content));
            }
        }

        if output.is_empty() {
            return Err("Failed to read socket data from /proc/net".to_string());
        }

        Ok(format!("Active connections:\n{}", output))
    }

    fn dns_lookup(&self, host: Option<&str>) -> Result<String, String> {
        if let Some(h) = host {
            validation::validate_args(h)?;
            let addr = format!("{}:0", h);
            let resolved: Vec<String> = addr
                .to_socket_addrs()
                .map_err(|e| format!("DNS resolution failed: {}", e))?
                .map(|a| a.ip().to_string())
                .collect();

            if resolved.is_empty() {
                return Err(format!("No addresses found for {}", h));
            }

            Ok(format!("DNS lookup for {}:\n{}", h, resolved.join("\n")))
        } else {
            Err("Host required".to_string())
        }
    }
}

#[async_trait]
impl AgentTrait for NetworkEngineerAgent {
    fn agent_type(&self) -> &str {
        "network-engineer"
    }
    fn name(&self) -> &str {
        "Network Engineer"
    }
    fn description(&self) -> &str {
        "Network diagnostics and analysis"
    }

    fn operations(&self) -> Vec<String> {
        vec![
            "interfaces".to_string(),
            "routes".to_string(),
            "connections".to_string(),
            "dns".to_string(),
        ]
    }

    fn security_profile(&self) -> &SecurityProfile {
        &self.profile
    }

    async fn execute(&self, task: AgentTask) -> Result<TaskResult, String> {
        let result = match task.operation.as_str() {
            "interfaces" => self.show_interfaces(),
            "routes" => self.show_routes(),
            "connections" => self.show_connections(),
            "dns" => self.dns_lookup(task.path.as_deref()),
            _ => Err(format!("Unknown operation: {}", task.operation)),
        };

        match result {
            Ok(data) => Ok(TaskResult::success(&task.operation, data)),
            Err(e) => Ok(TaskResult::failure(&task.operation, e)),
        }
    }
}
