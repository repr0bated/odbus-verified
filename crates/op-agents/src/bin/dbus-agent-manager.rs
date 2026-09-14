//! D-Bus Agent Manager
//!
//! Starts and manages all agents as D-Bus services.
//! Run this as a systemd service to have agents available.
//!
//! Each agent registers on D-Bus as:
//!   - Service: org.dbusmcp.Agent.{AgentType}
//!   - Path: /org/dbusmcp/Agent/{AgentType}
//!   - Interface: org.dbusmcp.Agent
//!
//! The ChatActor's tool_loader discovers these via introspection.

use anyhow::Result;
use op_agents::{
    create_agent,
    dbus_service::{start_agent, DbusAgentService},
};
use op_core::BusType;
use std::collections::HashMap;
use tokio::signal;
use tracing::{error, info};
use zbus::Connection;

/// Agent configuration
struct AgentConfig {
    agent_type: &'static str,
    auto_start: bool,
    priority: u8,
}

/// Agents to start (run-on-connection + on-demand)
const AGENTS: &[AgentConfig] = &[
    // Orchestration (Critical)
    AgentConfig {
        agent_type: "memory",
        auto_start: true,
        priority: 100,
    },
    AgentConfig {
        agent_type: "context-manager",
        auto_start: true,
        priority: 100,
    },
    AgentConfig {
        agent_type: "sequential-thinking",
        auto_start: true,
        priority: 100,
    },
    AgentConfig {
        agent_type: "dx-optimizer",
        auto_start: true,
        priority: 95,
    },
    AgentConfig {
        agent_type: "tdd-orchestrator",
        auto_start: true,
        priority: 95,
    },
    // Language & Architecture (High)
    AgentConfig {
        agent_type: "rust-pro",
        auto_start: true,
        priority: 90,
    },
    AgentConfig {
        agent_type: "python-pro",
        auto_start: true,
        priority: 90,
    },
    AgentConfig {
        agent_type: "backend-architect",
        auto_start: true,
        priority: 90,
    },
    AgentConfig {
        agent_type: "frontend-developer",
        auto_start: true,
        priority: 90,
    },
    AgentConfig {
        agent_type: "database-architect",
        auto_start: true,
        priority: 85,
    },
    AgentConfig {
        agent_type: "backend-security-coder",
        auto_start: true,
        priority: 85,
    },
    // Infrastructure & Ops (Medium)
    AgentConfig {
        agent_type: "network-engineer",
        auto_start: true,
        priority: 80,
    },
    AgentConfig {
        agent_type: "deployment",
        auto_start: true,
        priority: 80,
    },
    AgentConfig {
        agent_type: "devops-troubleshooter",
        auto_start: true,
        priority: 80,
    },
    // Analysis & Quality (Medium)
    AgentConfig {
        agent_type: "debugger",
        auto_start: true,
        priority: 75,
    },
    AgentConfig {
        agent_type: "code-reviewer",
        auto_start: true,
        priority: 75,
    },
    AgentConfig {
        agent_type: "search-specialist",
        auto_start: true,
        priority: 75,
    },
    AgentConfig {
        agent_type: "prompt-engineer",
        auto_start: true,
        priority: 70,
    },
    AgentConfig {
        agent_type: "docs-architect",
        auto_start: true,
        priority: 70,
    },
];

/// Agent Manager - starts and monitors D-Bus agent services
struct AgentManager {
    connections: HashMap<String, Connection>,
    bus_type: BusType,
}

impl AgentManager {
    fn new(bus_type: BusType) -> Self {
        Self {
            connections: HashMap::new(),
            bus_type,
        }
    }

    /// Start an agent as a D-Bus service
    async fn start_agent(&mut self, agent_type: &str) -> Result<()> {
        if self.connections.contains_key(agent_type) {
            info!("Agent {} already running", agent_type);
            return Ok(());
        }

        // Create the agent
        let agent_id = format!("{}-main", agent_type);
        let agent = create_agent(agent_type, agent_id.clone())
            .map_err(|e| anyhow::anyhow!("Failed to create agent {}: {}", agent_type, e))?;

        // Start as D-Bus service
        let connection = start_agent(agent, &agent_id, self.bus_type)
            .await
            .map_err(|e| {
                anyhow::anyhow!("Failed to start D-Bus service for {}: {}", agent_type, e)
            })?;

        let service_name = DbusAgentService::service_name(agent_type);
        info!("✓ Started D-Bus agent: {} at {}", agent_type, service_name);

        self.connections.insert(agent_type.to_string(), connection);
        Ok(())
    }

    /// Start all auto-start agents
    async fn start_auto_agents(&mut self) -> Result<()> {
        let mut started = 0;
        let mut failed = 0;

        // Sort by priority (highest first)
        let mut agents: Vec<_> = AGENTS.iter().filter(|a| a.auto_start).collect();
        agents.sort_by_key(|a| std::cmp::Reverse(a.priority));

        for config in agents {
            match self.start_agent(config.agent_type).await {
                Ok(_) => started += 1,
                Err(e) => {
                    error!("Failed to start {}: {}", config.agent_type, e);
                    failed += 1;
                }
            }
        }

        info!(
            "Agent startup complete: {} started, {} failed",
            started, failed
        );
        Ok(())
    }

    /// List running agents
    fn list_running(&self) -> Vec<&str> {
        self.connections.keys().map(|s| s.as_str()).collect()
    }

    /// Stop an agent
    async fn stop_agent(&mut self, agent_type: &str) -> Result<()> {
        if let Some(_conn) = self.connections.remove(agent_type) {
            info!("Stopped agent: {}", agent_type);
            // Connection drops, D-Bus service unregisters
        }
        Ok(())
    }

    /// Stop all agents
    async fn stop_all(&mut self) {
        let agents: Vec<_> = self.connections.keys().cloned().collect();
        for agent in agents {
            let _ = self.stop_agent(&agent).await;
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("op_agents=info".parse().unwrap()),
        )
        .init();

    info!("Starting op-dbus Agent Manager");

    // Determine bus type from environment
    let bus_type = if std::env::var("DBUS_AGENT_SESSION").is_ok() {
        info!("Using session bus");
        BusType::Session
    } else {
        info!("Using system bus");
        BusType::System
    };

    // Create manager and start agents
    let mut manager = AgentManager::new(bus_type);

    if let Err(e) = manager.start_auto_agents().await {
        error!("Failed to start agents: {}", e);
        return Err(e);
    }

    info!(
        "Agent Manager ready. Running agents: {:?}",
        manager.list_running()
    );
    info!("Press Ctrl+C to stop");

    // Wait for shutdown signal
    signal::ctrl_c().await?;

    info!("Shutting down Agent Manager...");
    manager.stop_all().await;

    info!("Agent Manager stopped");
    Ok(())
}
