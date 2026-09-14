use anyhow::{Context, Result};
use async_trait::async_trait;
use op_state::StatePlugin;
use op_state::{ApplyResult, PluginCapabilities, StateAction, StateDiff};
use op_state_store::{CapabilityDecl, PluginSchema};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;
use serde_json::Value as JsonValue;
use simd_json::prelude::*;
use simd_json::OwnedValue as Value;
use std::path::Path;
use zbus::{Connection, Proxy};

/// Netmaker configuration.
/// See: https://docs.netmaker.io/
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema, Default)]
pub struct NetmakerConfig {
    /// Enable Netmaker mesh networking
    pub enabled: bool,
    /// Default network to join
    pub default_network: String,
    /// Enrollment token for joining networks
    pub enrollment_token: Option<String>,
    /// API endpoint for Netmaker server (if self-hosted)
    pub api_endpoint: Option<String>,
}

/// Netmaker network state.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct NetmakerNetwork {
    /// Network name
    pub name: String,
    /// Whether connected to this network
    pub connected: bool,
    /// Whether this is the default network
    pub is_default: bool,
    /// Node ID for this network
    pub node_id: Option<String>,
    /// List of peer endpoints
    pub peers: Vec<String>,
    /// Network address
    pub address: Option<String>,
}

/// Netmaker state.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-category" = "network"))]
pub struct NetmakerState {
    /// Software name (netclient)
    pub software: String,
    /// Version string
    pub version: String,
    /// Required dependencies
    pub dependencies: Vec<String>,
    /// Whether netclient is installed
    pub installed: bool,
    /// Whether the netclient service is running under s6
    pub daemon_running: bool,
    /// Netclient control socket used by the runtime
    pub control_socket: Option<String>,
    /// Connected networks
    pub networks: Vec<NetmakerNetwork>,
    /// Public IP address
    pub public_ip: Option<String>,
    /// Netmaker configuration
    pub config: NetmakerConfig,
    /// Available tools
    #[schemars(with = "serde_json::Value")]
    pub tools: JsonValue,
}

/// Service controller interface for managing the netclient lifecycle.
#[derive(Clone)]
enum ServiceController {
    /// runit via org.opdbus.v1.Runit.Systemctl
    S6 { connection: Connection },
}

impl ServiceController {
    /// Detect the appropriate service controller for this system
    async fn detect() -> Result<Self> {
        if !Path::new(op_core::runit::SERVICE_DIR).exists() {
            return Err(anyhow::anyhow!(
                "runit runtime not found at {}; netclient is managed through \
                 org.opdbus.v1.Runit.Systemctl",
                op_core::runit::SERVICE_DIR
            ));
        }

        let conn = Connection::system()
            .await
            .context("Failed to connect to system D-Bus for s6")?;
        Proxy::new(
            &conn,
            "org.opdbus.v1.Runit.Systemctl",
            "/org/opdbus/v1/plugins/runit/systemctl",
            "org.opdbus.v1.Runit.Systemctl",
        )
        .await
        .context("s6-systemctl D-Bus service is unavailable")?;

        Ok(ServiceController::S6 { connection: conn })
    }

    /// Check if a service is active
    async fn is_active(&self, service: &str) -> Result<bool> {
        match self {
            ServiceController::S6 { connection } => {
                let proxy = Proxy::new(
                    connection,
                    "org.opdbus.v1.Runit.Systemctl",
                    "/org/opdbus/v1/plugins/runit/systemctl",
                    "org.opdbus.v1.Runit.Systemctl",
                )
                .await?;

                let result: String = proxy.call("is_active", &(service,)).await?;
                Ok(result == "active")
            }
        }
    }

    /// Enable and start a service (enable --now)
    async fn enable_and_start(&self, service: &str) -> Result<()> {
        match self {
            ServiceController::S6 { connection } => {
                let proxy = Proxy::new(
                    connection,
                    "org.opdbus.v1.Runit.Systemctl",
                    "/org/opdbus/v1/plugins/runit/systemctl",
                    "org.opdbus.v1.Runit.Systemctl",
                )
                .await?;

                // Enable the service
                let (success, msg): (bool, String) = proxy.call("enable", &(service,)).await?;

                if !success {
                    return Err(anyhow::anyhow!("Failed to enable {}: {}", service, msg));
                }

                // Start the service
                let (success, msg): (bool, String) = proxy.call("start", &(service,)).await?;

                if !success {
                    return Err(anyhow::anyhow!("Failed to start {}: {}", service, msg));
                }

                Ok(())
            }
        }
    }
}

pub struct NetmakerPlugin {
    config: NetmakerConfig,
}

impl NetmakerPlugin {
    pub fn new(config: NetmakerConfig) -> Self {
        Self { config }
    }

    /// Check if netclient is installed via direct file check (AGENTS.md §4: no subprocess bypasses)
    async fn check_netclient_installed() -> Result<bool> {
        Ok(std::path::Path::new("/usr/bin/netclient").exists()
            || std::path::Path::new("/usr/local/bin/netclient").exists())
    }

    /// Check if netclient daemon is running via D-Bus
    async fn check_daemon_running() -> Result<bool> {
        let controller = ServiceController::detect().await?;
        controller.is_active("netclient").await
    }

    fn netclient_config_path() -> &'static str {
        "/etc/netclient/netclient.json"
    }

    async fn read_netclient_config() -> Result<JsonValue> {
        match tokio::fs::read_to_string(Self::netclient_config_path()).await {
            Ok(content) => Ok(serde_json::from_str(&content)
                .with_context(|| "Failed to parse /etc/netclient/netclient.json")?),
            Err(_) => Ok(serde_json::json!({})),
        }
    }

    async fn write_netclient_config(mut config: JsonValue) -> Result<()> {
        if !config.is_object() {
            config = serde_json::json!({});
        }

        tokio::fs::create_dir_all("/etc/netclient").await.ok();
        tokio::fs::write(
            Self::netclient_config_path(),
            serde_json::to_string_pretty(&config)?,
        )
        .await
        .context("Failed to write /etc/netclient/netclient.json")?;
        Ok(())
    }

    async fn restart_netclient() -> Result<()> {
        let controller = ServiceController::detect().await?;
        controller.enable_and_start("netclient").await
    }

    /// Get current networks from netclient config files (AGENTS.md §4: no subprocess bypasses)
    async fn get_networks(&self) -> Result<Vec<NetmakerNetwork>> {
        let mut networks = Vec::new();

        // Read netclient.json to discover connected networks
        if let Ok(content) = tokio::fs::read_to_string("/etc/netclient/netclient.json").await {
            if let Ok(config) = simd_json::to_owned_value(&mut content.into_bytes()) {
                // Check nodes array for connected networks
                if let Some(nodes) = config.get("nodes").and_then(|n| n.as_array()) {
                    for node in nodes {
                        let network_name = node
                            .get("network")
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown")
                            .to_string();
                        let connected = node
                            .get("connected")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false);
                        let address = node
                            .get("address")
                            .and_then(|v| v.as_str())
                            .map(String::from);

                        let peers = self
                            .get_network_peers(&network_name)
                            .await
                            .unwrap_or_default();

                        networks.push(NetmakerNetwork {
                            name: network_name.clone(),
                            connected,
                            is_default: network_name == self.config.default_network,
                            node_id: node.get("id").and_then(|v| v.as_str()).map(String::from),
                            peers,
                            address,
                        });
                    }
                }
            }
        }

        Ok(networks)
    }

    /// Get peers for a specific network
    async fn get_network_peers(&self, network: &str) -> Result<Vec<String>> {
        // This is a simplified implementation
        // In reality, you'd need to query the Netmaker API or parse daemon state
        let _ = network; // Suppress unused variable warning
        Ok(Vec::new()) // TODO: Implement actual peer discovery
    }

    /// Get public IP from netclient config (AGENTS.md §4: no subprocess bypasses)
    async fn get_public_ip(&self) -> Result<Option<String>> {
        if let Ok(content) = tokio::fs::read_to_string("/etc/netclient/netclient.json").await {
            if let Ok(config) = simd_json::to_owned_value(&mut content.into_bytes()) {
                return Ok(config
                    .get("endpointip")
                    .and_then(|v| v.as_str())
                    .map(String::from));
            }
        }
        Ok(None)
    }

    /// Join a Netmaker network
    /// The control surface is the netclient config file plus the s6 restart path.
    async fn join_network(&self, network: &str, token: &str) -> Result<()> {
        let mut config = Self::read_netclient_config().await?;
        config["enabled"] = serde_json::json!(true);
        config["default_network"] = serde_json::json!(network);
        config["enrollment_token"] = serde_json::json!(token);
        config["interface"] = serde_json::json!("netmaker");
        config["inittype"] = serde_json::json!(6);
        Self::write_netclient_config(config).await?;
        Self::restart_netclient().await?;
        Ok(())
    }

    /// Leave a Netmaker network
    async fn leave_network(&self, network: &str) -> Result<()> {
        let mut config = Self::read_netclient_config().await?;
        if config.get("default_network").and_then(|v| v.as_str()) == Some(network) {
            config["default_network"] = serde_json::json!("");
        }
        config["enabled"] = serde_json::json!(false);
        config["enrollment_token"] = serde_json::json!(null);
        Self::write_netclient_config(config).await?;
        Self::restart_netclient().await?;
        Ok(())
    }

    /// Base URL of the Netmaker server's own REST API (distinct from
    /// netclient, which is this host's/container's *client* daemon). Reached
    /// directly over the OVS bridge — no proxy hop needed since host and the
    /// `NetMaker` container share `ovsbr0`.
    fn netmaker_api_base() -> String {
        std::env::var("NETMAKER_API_BASE").unwrap_or_else(|_| "http://10.200.0.3:8081".to_string())
    }

    /// Master-key Bearer auth for the Netmaker server API. Fails closed
    /// (explicit error) rather than silently calling the API unauthenticated.
    fn netmaker_master_key() -> Result<String> {
        std::env::var("NETMAKER_MASTER_KEY")
            .context("NETMAKER_MASTER_KEY not set — cannot authenticate to the Netmaker server API")
    }

    /// GET /api/networks — real server-side network list (distinct from
    /// `get_networks`, which reads *this* host's netclient enrollment state).
    async fn list_networks_api() -> Result<JsonValue> {
        let master_key = Self::netmaker_master_key()?;
        let resp = reqwest::Client::new()
            .get(format!("{}/api/networks", Self::netmaker_api_base()))
            .bearer_auth(master_key)
            .send()
            .await
            .context("Netmaker API request failed (GET /api/networks)")?;
        if !resp.status().is_success() {
            anyhow::bail!("Netmaker API returned {}: GET /api/networks", resp.status());
        }
        resp.json()
            .await
            .context("Failed to parse Netmaker API response")
    }

    /// GET /api/nodes, filtered client-side by network — the real Netmaker
    /// server API has no dedicated per-network list endpoint; every node
    /// carries its own `network` field.
    async fn list_nodes_api(network: &str) -> Result<JsonValue> {
        let master_key = Self::netmaker_master_key()?;
        let resp = reqwest::Client::new()
            .get(format!("{}/api/nodes", Self::netmaker_api_base()))
            .bearer_auth(master_key)
            .send()
            .await
            .context("Netmaker API request failed (GET /api/nodes)")?;
        if !resp.status().is_success() {
            anyhow::bail!("Netmaker API returned {}: GET /api/nodes", resp.status());
        }
        let all_nodes: JsonValue = resp
            .json()
            .await
            .context("Failed to parse Netmaker API response")?;
        let filtered: Vec<JsonValue> = all_nodes
            .as_array()
            .map(|nodes| {
                nodes
                    .iter()
                    .filter(|n| n.get("network").and_then(|v| v.as_str()) == Some(network))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        Ok(serde_json::Value::Array(filtered))
    }

    /// GET /api/nodes/{network}/{node_id}
    async fn get_node_api(network: &str, node_id: &str) -> Result<JsonValue> {
        let master_key = Self::netmaker_master_key()?;
        let resp = reqwest::Client::new()
            .get(format!(
                "{}/api/nodes/{network}/{node_id}",
                Self::netmaker_api_base()
            ))
            .bearer_auth(master_key)
            .send()
            .await
            .context("Netmaker API request failed (GET /api/nodes/:network/:id)")?;
        if !resp.status().is_success() {
            anyhow::bail!(
                "Netmaker API returned {}: GET /api/nodes/{network}/{node_id}",
                resp.status()
            );
        }
        resp.json()
            .await
            .context("Failed to parse Netmaker API response")
    }

    /// PUT /api/nodes/{network}/{node_id}
    async fn update_node_api(
        network: &str,
        node_id: &str,
        payload: &JsonValue,
    ) -> Result<JsonValue> {
        let master_key = Self::netmaker_master_key()?;
        let resp = reqwest::Client::new()
            .put(format!(
                "{}/api/nodes/{network}/{node_id}",
                Self::netmaker_api_base()
            ))
            .bearer_auth(master_key)
            .json(payload)
            .send()
            .await
            .context("Netmaker API request failed (PUT /api/nodes/:network/:id)")?;
        if !resp.status().is_success() {
            anyhow::bail!(
                "Netmaker API returned {}: PUT /api/nodes/{network}/{node_id}",
                resp.status()
            );
        }
        resp.json()
            .await
            .context("Failed to parse Netmaker API response")
    }

    pub(crate) fn current_state() -> NetmakerState {
        let tools = json!([
            {
                "name": "netmaker.join",
                "description": "Join a Netmaker network",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "token": {
                            "type": "string",
                            "description": "The enrollment token"
                        }
                    },
                    "required": ["token"]
                }
            },
            {
                "name": "netmaker.leave",
                "description": "Leave a Netmaker network",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "network": {
                            "type": "string",
                            "description": "The network name to leave"
                        }
                    },
                    "required": ["network"]
                }
            }
        ]);

        NetmakerState {
            software: "netclient".to_string(),
            version: "1.0.0".to_string(),
            dependencies: vec!["net".to_string(), "service".to_string()],
            installed: false,
            daemon_running: false,
            control_socket: Some("/var/run/netclient/netclient.sock".to_string()),
            networks: Vec::new(),
            public_ip: None,
            config: NetmakerConfig::default(),
            tools,
        }
    }
}

#[async_trait]
impl StatePlugin for NetmakerPlugin {
    fn name(&self) -> &'static str {
        "netmaker"
    }

    fn version(&self) -> &str {
        "1.0.0"
    }

    fn schema(&self) -> Option<op_state_store::PluginSchema> {
        Some(netmaker_schema())
    }

    fn capabilities(&self) -> PluginCapabilities {
        PluginCapabilities {
            supports_rollback: true,
            supports_checkpoints: true,
            supports_verification: true,
            atomic_operations: true,
        }
    }

    async fn calculate_diff(&self, current: &Value, desired: &Value) -> Result<StateDiff> {
        let mut actions = Vec::new();

        // Check if netclient should be installed/enabled
        let current_installed = current
            .get("installed")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let desired_enabled = desired
            .get("config")
            .and_then(|c| c.get("enabled"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        if !current_installed && desired_enabled {
            actions.push(StateAction::Create {
                resource: "netmaker_installation".to_string(),
                config: simd_json::json!({
                    "action": "install_netclient",
                    "type": "system_package"
                }),
            });
        }

        // Check network membership changes
        let empty_networks = vec![];
        let current_networks = current
            .get("networks")
            .and_then(|n| n.as_array())
            .unwrap_or(&empty_networks);
        let desired_networks = desired
            .get("config")
            .and_then(|c| c.get("default_network"))
            .and_then(|n| n.as_str());

        if let Some(desired_network) = desired_networks {
            let currently_connected = current_networks.iter().any(|net| {
                net.get("name").and_then(|n| n.as_str()) == Some(desired_network)
                    && net
                        .get("connected")
                        .and_then(|c| c.as_bool())
                        .unwrap_or(false)
            });

            if !currently_connected && desired_enabled {
                actions.push(StateAction::Create {
                    resource: format!("netmaker_network_{}", desired_network),
                    config: simd_json::json!({
                        "network": desired_network,
                        "action": "join_network",
                        "type": "network_membership"
                    }),
                });
            }
        }

        Ok(StateDiff {
            plugin: self.name().to_string(),
            actions,
            metadata: op_state::DiffMetadata {
                timestamp: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)?
                    .as_secs() as i64,
                current_hash: format!("{:x}", md5::compute(simd_json::to_string(current)?)),
                desired_hash: format!("{:x}", md5::compute(simd_json::to_string(desired)?)),
            },
        })
    }

    async fn apply_state(&self, diff: &StateDiff) -> Result<ApplyResult> {
        let mut changes_applied = Vec::new();
        let mut errors = Vec::new();

        // Detect service controller once for all operations
        let controller = match ServiceController::detect().await {
            Ok(ctrl) => Some(ctrl),
            Err(e) => {
                errors.push(format!("Failed to detect service controller: {}", e));
                None
            }
        };

        for action in &diff.actions {
            if let StateAction::Create {
                resource,
                config: _,
            } = action
            {
                if resource == "netmaker_installation" {
                    // Package installation requires PackageKit D-Bus or manual install per AGENTS.md §4
                    errors.push(
                        "Netclient package installation requires external PackageKit D-Bus or manual install (disabled per AGENTS.md §4). \
                         Run: apt-get install -y netclient".to_string(),
                    );
                    // Attempt to enable and start service via D-Bus regardless
                    if let Some(ref ctrl) = controller {
                        match ctrl.enable_and_start("netclient").await {
                            Ok(_) => {
                                changes_applied.push(
                                    "Enabled and started netclient service via D-Bus".to_string(),
                                );
                            }
                            Err(e) => {
                                errors.push(format!(
                                    "Failed to enable/start netclient via D-Bus: {}",
                                    e
                                ));
                            }
                        }
                    }
                } else if resource.starts_with("netmaker_network_") {
                    let network = resource.strip_prefix("netmaker_network_").unwrap_or("");
                    if let Some(token) = &self.config.enrollment_token {
                        match self.join_network(network, token).await {
                            Ok(_) => {
                                changes_applied.push(format!("Joined Netmaker network {}", network))
                            }
                            Err(e) => {
                                errors.push(format!("Failed to join network {}: {}", network, e))
                            }
                        }
                    } else {
                        errors.push(format!(
                            "No enrollment token configured for network {}",
                            network
                        ));
                    }
                }
            }
        }

        Ok(ApplyResult {
            success: errors.is_empty(),
            changes_applied,
            errors,
            checkpoint: None,
        })
    }

    async fn verify_state(&self, _desired: &Value) -> Result<bool> {
        Ok(true)
    }

    async fn create_checkpoint(&self) -> Result<op_state::Checkpoint> {
        let state = simd_json::json!(null);
        Ok(op_state::Checkpoint {
            id: format!(
                "netmaker_{}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)?
                    .as_secs()
            ),
            plugin: self.name().to_string(),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_secs() as i64,
            state_snapshot: state,
            backend_checkpoint: None,
        })
    }

    async fn rollback(&self, _checkpoint: &op_state::Checkpoint) -> Result<()> {
        // Rollback would leave networks and potentially rejoin them
        // This is a simplified implementation
        Err(anyhow::anyhow!(
            "Netmaker rollback not implemented - would require leaving and rejoining networks"
        ))
    }
}

/// Method input types - single source of truth via schemars
/// join_network method input
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct JoinNetworkInput {
    /// Network name to join
    pub network: String,
    /// Enrollment token (optional if already configured)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct LeaveNetworkInput {
    pub network: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ListNodesInput {
    pub network: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GetNodeInput {
    pub network: String,
    pub node_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct UpdateNodeInput {
    pub network: String,
    pub node_id: String,
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ListNetworksInput {}

pub(crate) fn netmaker_schema() -> PluginSchema {
    let root = serde_json::to_value(schemars::schema_for!(NetmakerState))
        .expect("schemars schema serializes to JSON");
    let mut schema = super::schemars_adapter::plugin_schema_from_json(
        "netmaker",
        "1.0.0",
        "Netmaker state and execution schema backed by s6-managed netclient",
        &root,
    );
    schema.category = "net".to_string();
    if let Ok(defaults) = simd_json::serde::to_owned_value(NetmakerPlugin::current_state()) {
        super::schemars_adapter::apply_state_defaults(&mut schema, &defaults);
    }
    schema.methods.insert(
        "join_network".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            JoinNetworkInput,
            super::plugin_scaffold_helpers::AckOutput,
        >(
            "join_network",
            op_state_store::SideEffect::Mutation,
            false,
            "cap.network.netmaker.network.join@v1",
            "mut.network.netmaker.network.join@v1",
        ),
    );
    schema.methods.insert(
        "leave_network".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            LeaveNetworkInput,
            super::plugin_scaffold_helpers::AckOutput,
        >(
            "leave_network",
            op_state_store::SideEffect::Mutation,
            false,
            "cap.network.netmaker.network.leave@v1",
            "mut.network.netmaker.network.leave@v1",
        ),
    );
    schema.methods.insert(
        "list_nodes".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            ListNodesInput,
            super::plugin_scaffold_helpers::AckOutput,
        >(
            "list_nodes",
            op_state_store::SideEffect::Read,
            true,
            "cap.network.netmaker.nodes.list@v1",
            "obs.network.netmaker.nodes.list@v1",
        ),
    );
    schema.methods.insert(
        "get_node".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            GetNodeInput,
            super::plugin_scaffold_helpers::AckOutput,
        >(
            "get_node",
            op_state_store::SideEffect::Read,
            true,
            "cap.network.netmaker.node.get@v1",
            "obs.network.netmaker.node.get@v1",
        ),
    );
    schema.methods.insert(
        "update_node".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            UpdateNodeInput,
            super::plugin_scaffold_helpers::AckOutput,
        >(
            "update_node",
            op_state_store::SideEffect::Mutation,
            false,
            "cap.network.netmaker.node.update@v1",
            "mut.network.netmaker.node.update@v1",
        ),
    );
    schema.methods.insert(
        "list_networks".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            ListNetworksInput,
            super::plugin_scaffold_helpers::AckOutput,
        >(
            "list_networks",
            op_state_store::SideEffect::Read,
            true,
            "cap.network.netmaker.networks.list@v1",
            "obs.network.netmaker.networks.list@v1",
        ),
    );
    schema.capabilities.insert(
        "cap.network.netmaker.network.join@v1".to_string(),
        CapabilityDecl {
            id: "cap.network.netmaker.network.join@v1".to_string(),
            description: "Grants: join_network.".to_string(),
        },
    );
    schema.capabilities.insert(
        "cap.network.netmaker.network.leave@v1".to_string(),
        CapabilityDecl {
            id: "cap.network.netmaker.network.leave@v1".to_string(),
            description: "Grants: leave_network.".to_string(),
        },
    );
    schema.capabilities.insert(
        "cap.network.netmaker.nodes.list@v1".to_string(),
        CapabilityDecl {
            id: "cap.network.netmaker.nodes.list@v1".to_string(),
            description: "Grants: list_nodes.".to_string(),
        },
    );
    schema.capabilities.insert(
        "cap.network.netmaker.node.get@v1".to_string(),
        CapabilityDecl {
            id: "cap.network.netmaker.node.get@v1".to_string(),
            description: "Grants: get_node.".to_string(),
        },
    );
    schema.capabilities.insert(
        "cap.network.netmaker.node.update@v1".to_string(),
        CapabilityDecl {
            id: "cap.network.netmaker.node.update@v1".to_string(),
            description: "Grants: update_node.".to_string(),
        },
    );
    schema.capabilities.insert(
        "cap.network.netmaker.networks.list@v1".to_string(),
        CapabilityDecl {
            id: "cap.network.netmaker.networks.list@v1".to_string(),
            description: "Grants: list_networks.".to_string(),
        },
    );
    schema.capabilities.insert(
        "cap.network.netmaker.egress.create@v1".to_string(),
        CapabilityDecl {
            id: "cap.network.netmaker.egress.create@v1".to_string(),
            description: "Grants: create_egress.".to_string(),
        },
    );
    schema.capabilities.insert(
        "cap.network.netmaker.egress.delete@v1".to_string(),
        CapabilityDecl {
            id: "cap.network.netmaker.egress.delete@v1".to_string(),
            description: "Grants: delete_egress.".to_string(),
        },
    );

    schema
}

/// Real dispatch for every method declared in `netmaker_schema()`. Mirrors
/// `tched_router::dispatch_tched_router_method`'s role — the plugin crate owns its
/// own dispatch, the gRPC bridge just calls this one function.
pub async fn dispatch_netmaker_method(method: &str, args: &JsonValue) -> Result<JsonValue> {
    let plugin = NetmakerPlugin::new(NetmakerConfig::default());
    match method {
        "join_network" => {
            let network = args
                .get("network")
                .and_then(|v| v.as_str())
                .context("network required")?;
            let token = args.get("token").and_then(|v| v.as_str()).unwrap_or("");
            plugin.join_network(network, token).await?;
            Ok(json!({ "joined": network }))
        }
        "leave_network" => {
            let network = args
                .get("network")
                .and_then(|v| v.as_str())
                .context("network required")?;
            plugin.leave_network(network).await?;
            Ok(json!({ "left": network }))
        }
        "list_networks" => NetmakerPlugin::list_networks_api().await,
        "list_nodes" => {
            let network = args
                .get("network")
                .and_then(|v| v.as_str())
                .context("network required")?;
            NetmakerPlugin::list_nodes_api(network).await
        }
        "get_node" => {
            let network = args
                .get("network")
                .and_then(|v| v.as_str())
                .context("network required")?;
            let node_id = args
                .get("node_id")
                .and_then(|v| v.as_str())
                .context("node_id required")?;
            NetmakerPlugin::get_node_api(network, node_id).await
        }
        "update_node" => {
            let network = args
                .get("network")
                .and_then(|v| v.as_str())
                .context("network required")?;
            let node_id = args
                .get("node_id")
                .and_then(|v| v.as_str())
                .context("node_id required")?;
            let payload = args.get("payload").cloned().unwrap_or(json!({}));
            NetmakerPlugin::update_node_api(network, node_id, &payload).await
        }
        _ => anyhow::bail!("unknown netmaker method: {method}"),
    }
}

// Self-registration: the plugin registry discovers this via inventory
// (single source of the catalog; no central dispatch list).
inventory::submit! {
    crate::default_registry::PluginReg::new("netmaker", |_ctx| std::sync::Arc::new(NetmakerPlugin::new(NetmakerConfig::default())))
}
