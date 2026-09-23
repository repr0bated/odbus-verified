//! Network Engineer Agent

use async_trait::async_trait;
use std::net::ToSocketAddrs;

use crate::agents::base::{validation, AgentTask, AgentTrait, TaskResult};
use crate::security::SecurityProfile;

pub struct NetworkEngineerAgent {
    profile: SecurityProfile,
}

impl NetworkEngineerAgent {
    pub fn new(_agent_id: String) -> Self {
        Self {
            profile: SecurityProfile::read_only_analysis(
                "network-engineer",
                vec!["ip", "ss", "netstat", "dig", "ping", "traceroute"],
            ),
        }
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
        vec!["dns".to_string()]
    }

    fn security_profile(&self) -> &SecurityProfile {
        &self.profile
    }

    async fn execute(&self, task: AgentTask) -> Result<TaskResult, String> {
        let result = match task.operation.as_str() {
            "interfaces" | "routes" | "connections" => {
                Err("Network diagnostics require a canonical plugin projection".to_string())
            }
            "dns" => self.dns_lookup(task.path.as_deref()),
            _ => Err(format!("Unknown operation: {}", task.operation)),
        };

        match result {
            Ok(data) => Ok(TaskResult::success(&task.operation, data)),
            Err(e) => Ok(TaskResult::failure(&task.operation, e)),
        }
    }
}
