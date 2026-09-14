//! Privacy router system fabric.
//!
//! This plugin owns the base privacy fabric as system-managed Incus containers and
//! bridge/OpenFlow policy, separate from per-user privacy containers.

use crate::state_plugins::incus::{IncusInstance, IncusPlugin, IncusState};
use crate::state_plugins::incus_device::{Device, NamedDevice, NicDevice};
use crate::state_plugins::openflow::{
    BridgeFlowConfig, FlowAction, FlowEntry, OpenFlowConfig, OpenFlowPlugin,
};
use crate::state_plugins::privacy_routes::{PrivacyRoute, PrivacyRoutesPlugin, PrivacyRoutesState};
use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use op_network::{openflow::OpenFlowClient, rovs_proxy::OvsdbDbusClient};
use op_state::{
    ApplyResult, Checkpoint, DiffMetadata, PluginCapabilities, StateAction, StateDiff, StatePlugin,
};
use op_state_store::{
    Constraint, FieldSchema, FieldType, PluginSchema, ReadOnlyCondition, SideEffect,
};
use serde::{Deserialize, Serialize};
use simd_json::{json, prelude::*, OwnedValue as Value};
use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::path::Path;

const DEFAULT_BRIDGE_NAME: &str = "ovsbr0";
const DEFAULT_UPLINK_PORT: &str = "ens3";
const DEFAULT_MGMT_PORT: &str = "ovsbr0-mgmt";
const DEFAULT_SOCKET_PORT: &str = "ovsbr0-sock";
const DEFAULT_GRPC_BRIDGE_PORT: &str = "grpc-bridge";
const DEFAULT_MGMT_CIDR: &str = "10.200.0.1/24";
const DEFAULT_OPENFLOW_CONTROLLER: &str = "10.200.0.1:6653";
const DEFAULT_DATAPATH_TYPE: &str = "system";
const DEFAULT_FAIL_MODE: &str = "secure";
const DEFAULT_WARP_INTERFACE: &str = "gbr_warp";
const DEFAULT_WARP_NETCLIENT_NETWORK: &str = "gbr_warp";
const SYSTEM_FLOW_COOKIE_PREFIX: u64 = 0x5053_0000_0000_0000;
const SYSTEM_FLOW_COOKIE_MASK: u64 = 0xFFFF_0000_0000_0000;

/// Privacy Router Tunnel Configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrivacyRouterConfig {
    /// OVS bridge name (shared by all components)
    pub bridge_name: String,

    /// WireGuard ingress container configuration
    pub wireguard: WireGuardConfig,

    /// WARP tunnel configuration
    pub warp: WarpConfig,

    /// XRay REALITY outbound client configuration
    pub xray: XRayConfig,

    /// VPS XRay server endpoint
    pub vps: VpsConfig,

    /// Socket networking configuration
    pub socket_networking: SocketNetworkingConfig,

    /// OpenFlow privacy flow configuration
    pub openflow: OpenFlowPrivacyConfig,

    /// Additional containers (vector DB, bucket storage, etc.)
    pub containers: Vec<ContainerConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WireGuardConfig {
    pub enabled: bool,
    pub container_id: u32,
    pub socket_port: String,
    pub zero_config: bool,
    pub listen_port: u16,
    pub resources: ContainerResources,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerResources {
    pub vcpus: u8,
    pub memory_mb: u32,
    pub disk_gb: u32,
    /// Incus image reference, e.g. images:debian/13
    pub os_template: String,
    pub swap_mb: u32,
    pub unprivileged: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WarpConfig {
    pub enabled: bool,
    pub bridge_interface: String,
    /// Netmaker network name for netclient-provisioned WARP tunnel (not a raw WG config file)
    pub netclient_network: String,
    pub warp_license: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XRayConfig {
    pub enabled: bool,
    pub container_id: u32,
    pub socket_port: String,
    pub socks_port: u16,
    pub vps_address: String,
    pub vps_port: u16,
    pub resources: ContainerResources,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VpsConfig {
    pub xray_server: String,
    pub xray_port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SocketNetworkingConfig {
    pub enabled: bool,
    pub privacy_sockets: Vec<PrivacySocketPort>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrivacySocketPort {
    pub name: String,
    pub container_id: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenFlowPrivacyConfig {
    pub enabled: bool,
    #[serde(default = "default_security_enabled")]
    pub enable_security_flows: bool,
    #[serde(default = "default_obfuscation_level")]
    pub obfuscation_level: u8,
    pub privacy_flows: Vec<PrivacyFlowRule>,
    pub function_routing: Vec<FunctionRoute>,
}

fn default_security_enabled() -> bool {
    true
}

fn default_obfuscation_level() -> u8 {
    2
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrivacyFlowRule {
    pub priority: u16,
    pub match_fields: HashMap<String, String>,
    pub actions: Vec<String>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionRoute {
    pub function: String,
    pub target_socket: String,
    pub match_fields: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerConfig {
    pub id: u32,
    pub name: String,
    pub container_type: String,
}

#[derive(Debug, Clone)]
struct PrivacyHostBootstrapConfig {
    bridge_name: String,
    uplink_port: String,
    attach_uplink_to_bridge: bool,
    management_port: String,
    socket_port: String,
    grpc_bridge_port: String,
    management_cidr: String,
    openflow_controller: String,
    datapath_type: String,
    fail_mode: String,
}

impl PrivacyHostBootstrapConfig {
    fn from_env(bridge_name: &str) -> Self {
        Self {
            bridge_name: std::env::var("PRIVACY_BRIDGE_NAME")
                .unwrap_or_else(|_| bridge_name.to_string()),
            uplink_port: std::env::var("PRIVACY_UPLINK_PORT")
                .unwrap_or_else(|_| DEFAULT_UPLINK_PORT.to_string()),
            attach_uplink_to_bridge: bool_env("PRIVACY_ATTACH_UPLINK_TO_BRIDGE", false),
            management_port: std::env::var("PRIVACY_MGMT_PORT")
                .unwrap_or_else(|_| DEFAULT_MGMT_PORT.to_string()),
            socket_port: std::env::var("PRIVACY_SOCKET_PORT")
                .unwrap_or_else(|_| DEFAULT_SOCKET_PORT.to_string()),
            grpc_bridge_port: std::env::var("PRIVACY_GRPC_BRIDGE_PORT")
                .unwrap_or_else(|_| DEFAULT_GRPC_BRIDGE_PORT.to_string()),
            management_cidr: std::env::var("PRIVACY_MGMT_CIDR")
                .unwrap_or_else(|_| DEFAULT_MGMT_CIDR.to_string()),
            openflow_controller: std::env::var("PRIVACY_OPENFLOW_CONTROLLER")
                .unwrap_or_else(|_| DEFAULT_OPENFLOW_CONTROLLER.to_string()),
            datapath_type: std::env::var("PRIVACY_DATAPATH_TYPE")
                .unwrap_or_else(|_| DEFAULT_DATAPATH_TYPE.to_string()),
            fail_mode: std::env::var("PRIVACY_FAIL_MODE")
                .unwrap_or_else(|_| DEFAULT_FAIL_MODE.to_string()),
        }
    }
}

impl Default for PrivacyRouterConfig {
    fn default() -> Self {
        Self {
            bridge_name: DEFAULT_BRIDGE_NAME.to_string(),
            wireguard: WireGuardConfig {
                enabled: true,
                container_id: 100,
                socket_port: "gbr_wg".to_string(),
                zero_config: true,
                listen_port: 51820,
                resources: default_resources(),
            },
            warp: WarpConfig {
                enabled: true,
                bridge_interface: DEFAULT_WARP_INTERFACE.to_string(),
                netclient_network: DEFAULT_WARP_NETCLIENT_NETWORK.to_string(),
                warp_license: None,
            },
            xray: XRayConfig {
                enabled: true,
                container_id: 101,
                socket_port: "gbr_xray".to_string(),
                socks_port: 1080,
                vps_address: "vps.example.com".to_string(),
                vps_port: 443,
                resources: default_resources(),
            },
            vps: VpsConfig {
                xray_server: "vps.example.com".to_string(),
                xray_port: 443,
            },
            socket_networking: SocketNetworkingConfig {
                enabled: true,
                privacy_sockets: vec![
                    PrivacySocketPort {
                        name: "gbr_wg".to_string(),
                        container_id: Some(100),
                    },
                    PrivacySocketPort {
                        name: "gbr_xray".to_string(),
                        container_id: Some(101),
                    },
                ],
            },
            openflow: OpenFlowPrivacyConfig {
                enabled: true,
                enable_security_flows: true,
                obfuscation_level: 2,
                privacy_flows: default_privacy_flows(),
                function_routing: vec![],
            },
            containers: vec![],
        }
    }
}

fn default_resources() -> ContainerResources {
    ContainerResources {
        vcpus: 1,
        memory_mb: 512,
        disk_gb: 4,
        os_template: "images:debian/13".to_string(),
        swap_mb: 0,
        unprivileged: false,
    }
}

fn bool_env(name: &str, default: bool) -> bool {
    std::env::var(name)
        .ok()
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(default)
}

fn default_privacy_flows() -> Vec<PrivacyFlowRule> {
    vec![
        PrivacyFlowRule {
            priority: 100,
            match_fields: HashMap::from([("in_port".to_string(), "gbr_wg".to_string())]),
            actions: vec!["output:gbr_warp".to_string()],
            description: Some("gbr_wg -> gbr_warp".to_string()),
        },
        PrivacyFlowRule {
            priority: 100,
            match_fields: HashMap::from([("in_port".to_string(), "gbr_warp".to_string())]),
            actions: vec!["output:gbr_xray".to_string()],
            description: Some("gbr_warp -> gbr_xray".to_string()),
        },
        PrivacyFlowRule {
            priority: 100,
            match_fields: HashMap::from([("in_port".to_string(), "gbr_xray".to_string())]),
            actions: vec!["output:gbr_warp".to_string()],
            description: Some("gbr_xray -> gbr_warp".to_string()),
        },
        PrivacyFlowRule {
            priority: 200,
            match_fields: HashMap::from([("dl_type".to_string(), "0x0806".to_string())]),
            actions: vec!["arp_responder".to_string()],
            description: Some("ARP Responder for Privacy Network".to_string()),
        },
    ]
}

pub struct PrivacyRouterPlugin {
    config: PrivacyRouterConfig,
    routes_store: PrivacyRoutesPlugin,
}

impl PrivacyRouterPlugin {
    pub fn new(config: PrivacyRouterConfig) -> Self {
        Self {
            config,
            routes_store: PrivacyRoutesPlugin::default(),
        }
    }

    async fn query_privacy_routes(&self) -> Result<PrivacyRoutesState> {
        let state = simd_json::json!(null);
        Ok(simd_json::serde::from_owned_value(state)?)
    }

    async fn query_incus_state(&self) -> Result<IncusState> {
        let state = simd_json::json!(null);
        Ok(simd_json::serde::from_owned_value(state)?)
    }

    async fn query_openflow_state(&self) -> Result<OpenFlowConfig> {
        let state = simd_json::json!(null);
        Ok(simd_json::serde::from_owned_value(state)?)
    }

    async fn query_bridge_ports(&self, bridge_name: &str) -> Result<Vec<String>> {
        OvsdbDbusClient::new()
            .list_bridge_ports(bridge_name)
            .await
            .with_context(|| format!("list ports on {}", bridge_name))
    }

    fn unique_ingress_ports(routes: &[PrivacyRoute]) -> Vec<String> {
        let mut ingress_ports: HashSet<String> = routes
            .iter()
            .map(|route| route.ingress_port.clone())
            .collect();
        let mut ingress_ports: Vec<String> = ingress_ports.drain().collect();
        ingress_ports.sort();
        ingress_ports
    }

    fn desired_config_from_diff(&self, diff: &StateDiff) -> Result<PrivacyRouterConfig> {
        let mut merged = simd_json::serde::to_owned_value(self.config.clone())?;
        for action in &diff.actions {
            if let StateAction::Modify { changes, .. } = action {
                if let Some(config) = changes.get("config") {
                    Self::deep_merge(&mut merged, config);
                } else {
                    Self::deep_merge(&mut merged, changes);
                }
            }
        }
        Ok(simd_json::serde::from_owned_value(merged)?)
    }

    fn expected_system_container_names(config: &PrivacyRouterConfig) -> Vec<&'static str> {
        let mut names = Vec::new();
        if config.wireguard.enabled {
            names.push("privacy-wireguard-ingress");
        }
        if config.xray.enabled {
            names.push("privacy-xray-egress");
        }
        names
    }

    fn actual_system_containers(
        &self,
        config: &PrivacyRouterConfig,
        incus: &IncusState,
    ) -> Vec<String> {
        let expected: HashSet<&str> = Self::expected_system_container_names(config)
            .into_iter()
            .collect();
        let mut containers = incus
            .instances
            .iter()
            .filter(|instance| {
                expected.contains(instance.name.as_str())
                    && instance.status.eq_ignore_ascii_case("running")
            })
            .map(|instance| instance.name.clone())
            .collect::<Vec<_>>();
        containers.sort();
        containers
    }

    fn required_system_flow_count(&self, config: &PrivacyRouterConfig) -> usize {
        self.chain_ports(config).windows(2).count() * 2
    }

    async fn runtime_needs_reconcile(&self, config: &PrivacyRouterConfig) -> Result<bool> {
        if config.warp.enabled {
            match self.query_bridge_ports(&config.bridge_name).await {
                Ok(ports) => {
                    if !ports
                        .iter()
                        .any(|port| port == &config.warp.bridge_interface)
                    {
                        return Ok(true);
                    }
                }
                Err(_) => {
                    // Treat a missing bridge as drift so apply_state can build it.
                    return Ok(true);
                }
            }
        }

        let incus_state = self.query_incus_state().await?;
        let actual_containers = self.actual_system_containers(config, &incus_state);
        if actual_containers.len() != Self::expected_system_container_names(config).len() {
            return Ok(true);
        }

        let openflow_state = self.query_openflow_state().await?;
        let actual_flow_count = openflow_state
            .bridges
            .iter()
            .find(|bridge| bridge.name == config.bridge_name)
            .map(|bridge| {
                bridge
                    .flows
                    .iter()
                    .filter(|flow| flow.cookie.is_some_and(is_system_cookie))
                    .count()
            })
            .unwrap_or_default();

        Ok(config.openflow.enabled && actual_flow_count < self.required_system_flow_count(config))
    }

    fn deep_merge(target: &mut Value, source: &Value) {
        match (target, source) {
            (Value::Object(target_obj), Value::Object(source_obj)) => {
                for (key, value) in source_obj.iter() {
                    match target_obj.get_mut(key) {
                        Some(existing) => Self::deep_merge(existing, value),
                        None => {
                            target_obj.insert(key.clone(), value.clone());
                        }
                    }
                }
            }
            (target_value, source_value) => {
                *target_value = source_value.clone();
            }
        }
    }

    async fn ensure_warp_interface_on_bridge(&self, config: &PrivacyRouterConfig) -> Result<()> {
        if !config.warp.enabled {
            return Ok(());
        }

        let ovs = op_network::rovs_proxy::OvsdbDbusClient::new();
        let ports = ovs
            .list_bridge_ports(&config.bridge_name)
            .await
            .with_context(|| format!("list ports on {}", config.bridge_name))?;
        if ports
            .iter()
            .any(|port| port == &config.warp.bridge_interface)
        {
            let _ = op_network::rtnetlink::link_up(&config.warp.bridge_interface).await;
            return Ok(());
        }

        let interfaces = op_network::rtnetlink::list_interfaces()
            .await
            .context("list interfaces for warp attach")?;
        if !interfaces
            .iter()
            .any(|iface| iface.name == config.warp.bridge_interface)
        {
            // D-Bus first: do NOT spawn wg-quick/ip subprocesses.
            // The WARP interface must be provisioned by netclient
            // (Netmaker mesh) before the privacy router can attach it
            // to the OVS bridge.
            bail!(
                "WARP interface '{}' not found on host. Provision it via netclient \
                 (e.g. netclient join -t <token> for network '{}') before enabling privacy router.",
                config.warp.bridge_interface,
                config.warp.netclient_network
            );
        }

        ovs.add_port(&config.bridge_name, &config.warp.bridge_interface)
            .await
            .with_context(|| {
                format!(
                    "attach '{}' to '{}'",
                    config.warp.bridge_interface, config.bridge_name
                )
            })?;
        op_network::rtnetlink::link_up(&config.warp.bridge_interface)
            .await
            .with_context(|| format!("bring '{}' up", config.warp.bridge_interface))?;
        Ok(())
    }

    async fn ensure_host_bridge_topology(&self, config: &PrivacyRouterConfig) -> Result<()> {
        let host = PrivacyHostBootstrapConfig::from_env(&config.bridge_name);
        let ovs = OvsdbDbusClient::new();

        ovs.list_dbs()
            .await
            .context("Open vSwitch DB is unavailable; cannot provision privacy bridge")?;

        if !ovs
            .bridge_exists(&host.bridge_name)
            .await
            .context("check privacy bridge existence")?
        {
            ovs.create_bridge(&host.bridge_name)
                .await
                .with_context(|| format!("create bridge '{}'", host.bridge_name))?;
        }

        log::info!(
            "privacy_router bridge policy: {} datapath_type={} fail_mode={}",
            host.bridge_name,
            host.datapath_type,
            host.fail_mode
        );
        ovs.set_bridge_property(&host.bridge_name, "datapath_type", &host.datapath_type)
            .await
            .with_context(|| format!("set bridge datapath_type={}", host.datapath_type))?;
        ovs.set_bridge_property(&host.bridge_name, "fail_mode", &host.fail_mode)
            .await
            .with_context(|| format!("set bridge fail_mode={}", host.fail_mode))?;

        let existing_ports = ovs
            .list_bridge_ports(&host.bridge_name)
            .await
            .with_context(|| format!("list bridge ports on '{}'", host.bridge_name))?;

        if !host.uplink_port.trim().is_empty() {
            let uplink_path = format!("/sys/class/net/{}", host.uplink_port);
            if !Path::new(&uplink_path).exists() {
                bail!(
                    "configured uplink '{}' not found on host ({})",
                    host.uplink_port,
                    uplink_path
                );
            }
            if host.attach_uplink_to_bridge
                && !existing_ports.iter().any(|port| port == &host.uplink_port)
            {
                ovs.add_port(&host.bridge_name, &host.uplink_port)
                    .await
                    .with_context(|| {
                        format!(
                            "attach uplink '{}' to '{}'",
                            host.uplink_port, host.bridge_name
                        )
                    })?;
            }
            op_network::rtnetlink::link_up(&host.uplink_port)
                .await
                .with_context(|| format!("bring standalone uplink '{}' up", host.uplink_port))?;
        }

        if !existing_ports
            .iter()
            .any(|port| port == &host.management_port)
        {
            ovs.add_port_with_type(&host.bridge_name, &host.management_port, Some("internal"))
                .await
                .with_context(|| {
                    format!(
                        "add management port '{}' to '{}'",
                        host.management_port, host.bridge_name
                    )
                })?;
        }

        if !existing_ports.iter().any(|port| port == &host.socket_port) {
            ovs.add_port_with_type(&host.bridge_name, &host.socket_port, Some("internal"))
                .await
                .with_context(|| {
                    format!(
                        "add socket port '{}' to '{}'",
                        host.socket_port, host.bridge_name
                    )
                })?;
        }

        if !existing_ports
            .iter()
            .any(|port| port == &host.grpc_bridge_port)
        {
            ovs.add_port_with_type(&host.bridge_name, &host.grpc_bridge_port, Some("internal"))
                .await
                .with_context(|| {
                    format!(
                        "add gRPC bridge port '{}' to '{}'",
                        host.grpc_bridge_port, host.bridge_name
                    )
                })?;
        }

        op_network::rtnetlink::link_up(&host.bridge_name)
            .await
            .with_context(|| format!("bring '{}' up", host.bridge_name))?;
        op_network::rtnetlink::link_up(&host.management_port)
            .await
            .with_context(|| format!("bring '{}' up", host.management_port))?;
        op_network::rtnetlink::link_up(&host.socket_port)
            .await
            .with_context(|| format!("bring '{}' up", host.socket_port))?;
        op_network::rtnetlink::link_up(&host.grpc_bridge_port)
            .await
            .with_context(|| format!("bring '{}' up", host.grpc_bridge_port))?;

        let (management_ip, management_prefix) = parse_cidr(&host.management_cidr)?;
        op_network::rtnetlink::flush_addresses(&host.management_port)
            .await
            .with_context(|| format!("flush addresses on '{}'", host.management_port))?;
        op_network::rtnetlink::add_ipv4_address(
            &host.management_port,
            &management_ip,
            management_prefix,
        )
        .await
        .with_context(|| {
            format!(
                "assign management CIDR '{}' to '{}'",
                host.management_cidr, host.management_port
            )
        })?;

        if let Ok(controller_addr) = host.openflow_controller.parse::<SocketAddr>() {
            match OpenFlowClient::connect(controller_addr).await {
                Ok(mut client) => {
                    if let Err(e) = client.request_features().await {
                        log::warn!(
                            "OpenFlow controller probe connected but feature request failed: {}",
                            e
                        );
                    }
                }
                Err(e) => {
                    log::warn!(
                        "OpenFlow controller '{}' is not reachable yet: {}",
                        host.openflow_controller,
                        e
                    );
                }
            }
        } else {
            log::warn!(
                "Invalid PRIVACY_OPENFLOW_CONTROLLER '{}'; skipping OpenFlow probe",
                host.openflow_controller
            );
        }

        Ok(())
    }

    fn system_container_specs<'a>(
        &'a self,
        config: &'a PrivacyRouterConfig,
    ) -> Vec<SystemContainerSpec<'a>> {
        let mut specs = Vec::new();
        if config.wireguard.enabled {
            specs.push(SystemContainerSpec {
                name: "privacy-wireguard-ingress",
                role: "wireguard_ingress",
                socket_port: &config.wireguard.socket_port,
                resources: &config.wireguard.resources,
            });
        }
        if config.xray.enabled {
            specs.push(SystemContainerSpec {
                name: "privacy-xray-egress",
                role: "xray_reality_client",
                socket_port: &config.xray.socket_port,
                resources: &config.xray.resources,
            });
        }
        specs
    }

    fn desired_system_instance(
        &self,
        config: &PrivacyRouterConfig,
        spec: &SystemContainerSpec<'_>,
    ) -> IncusInstance {
        let devices = vec![NamedDevice {
            name: "fabric0".to_string(),
            device: Device::Nic(NicDevice {
                nictype: Some("bridged".to_string()),
                parent: Some(config.bridge_name.clone()),
                name: Some("eth0".to_string()),
                host_name: Some(spec.socket_port.to_string()),
                ..Default::default()
            }),
        }];

        IncusInstance {
            name: spec.name.to_string(),
            status: "Running".to_string(),
            instance_type: "container".to_string(),
            image: Some(spec.resources.os_template.clone()),
            storage_pool: Some(
                std::env::var("PRIVACY_SYSTEM_STORAGE_POOL")
                    .or_else(|_| std::env::var("INCUS_STORAGE_POOL"))
                    .unwrap_or_else(|_| "default".to_string()),
            ),
            profiles: Vec::new(),
            config: Some(HashMap::from([
                ("boot.autostart".to_string(), "true".to_string()),
                ("security.nesting".to_string(), "true".to_string()),
                (
                    "security.privileged".to_string(),
                    (!spec.resources.unprivileged).to_string(),
                ),
                ("user.opdbus.scope".to_string(), "system".to_string()),
                (
                    "user.opdbus.component".to_string(),
                    "privacy_router".to_string(),
                ),
                ("user.opdbus.role".to_string(), spec.role.to_string()),
                (
                    "user.opdbus.host_port".to_string(),
                    spec.socket_port.to_string(),
                ),
            ])),
            devices,
            description: None,
            architecture: None,
            ephemeral: Some(false),
            stateful: None,
            created_at: None,
            last_used_at: None,
            location: None,
            project: None,
            status_code: None,
            expanded_config: None,
            expanded_devices: Vec::new(),
        }
    }

    fn upsert_instance(instances: &mut Vec<IncusInstance>, desired: IncusInstance) {
        match instances
            .iter_mut()
            .find(|existing| existing.name == desired.name)
        {
            Some(existing) => *existing = desired,
            None => instances.push(desired),
        }
        instances.sort_by(|a, b| a.name.cmp(&b.name));
    }

    async fn apply_incus_system_containers(
        &self,
        config: &PrivacyRouterConfig,
    ) -> Result<ApplyResult> {
        let plugin = IncusPlugin::new();
        let current_state = simd_json::json!(null);
        let mut desired_state: IncusState =
            simd_json::serde::from_owned_value(current_state.clone())
                .context("deserialize current incus state")?;

        for spec in self.system_container_specs(config) {
            Self::upsert_instance(
                &mut desired_state.instances,
                self.desired_system_instance(config, &spec),
            );
        }

        let desired_value = simd_json::serde::to_owned_value(desired_state)?;
        let diff = plugin
            .calculate_diff(&current_state, &desired_value)
            .await?;
        if diff.actions.is_empty() {
            return Ok(ApplyResult {
                success: true,
                changes_applied: vec!["System privacy containers already in sync".to_string()],
                errors: Vec::new(),
                checkpoint: None,
            });
        }
        plugin.apply_state(&diff).await
    }

    fn chain_ports(&self, config: &PrivacyRouterConfig) -> Vec<String> {
        let mut ports = Vec::new();
        if config.wireguard.enabled {
            ports.push(config.wireguard.socket_port.clone());
        }
        if config.warp.enabled {
            ports.push(config.warp.bridge_interface.clone());
        }
        if config.xray.enabled {
            ports.push(config.xray.socket_port.clone());
        }
        ports
    }

    fn merge_openflow_config(
        &self,
        mut current: OpenFlowConfig,
        config: &PrivacyRouterConfig,
    ) -> OpenFlowConfig {
        let bridge_index = current
            .bridges
            .iter()
            .position(|bridge| bridge.name == config.bridge_name);
        let mut bridge = bridge_index
            .map(|index| current.bridges.remove(index))
            .unwrap_or(BridgeFlowConfig {
                name: config.bridge_name.clone(),
                flows: Vec::new(),
                socket_ports: None,
            });

        bridge
            .flows
            .retain(|flow| !flow.cookie.is_some_and(is_system_cookie));

        let ports = self.chain_ports(config);
        for (index, path) in ports.windows(2).enumerate() {
            bridge.flows.push(chain_flow(index, &path[0], &path[1]));
            bridge
                .flows
                .push(chain_flow(index + 1000, &path[1], &path[0]));
        }

        // Include custom privacy flows from configuration
        for (index, rule) in config.openflow.privacy_flows.iter().enumerate() {
            let mut actions = Vec::new();
            for action_str in &rule.actions {
                if action_str.starts_with("output:") {
                    actions.push(FlowAction::Output {
                        port: action_str.strip_prefix("output:").unwrap().to_string(),
                    });
                } else if action_str == "arp_responder" {
                    // Default ARP responder for the bridge IP
                    actions.push(FlowAction::ArpResponder {
                        mac: "00:11:22:33:44:55".to_string(), // Simplified default
                        ip: "10.200.0.1".to_string(),
                    });
                } else if action_str == "drop" {
                    actions.push(FlowAction::Drop);
                }
            }

            bridge.flows.push(FlowEntry {
                table: 0,
                priority: rule.priority,
                match_fields: rule.match_fields.clone(),
                actions,
                cookie: Some(SYSTEM_FLOW_COOKIE_PREFIX | 0x2000 | index as u64),
                idle_timeout: 0,
                hard_timeout: 0,
            });
        }

        bridge.flows.sort_by_key(flow_sort_key);

        current.bridges.push(bridge);
        current.bridges.sort_by(|a, b| a.name.cmp(&b.name));
        current.enable_security_flows =
            current.enable_security_flows || config.openflow.enable_security_flows;
        current.obfuscation_level = current
            .obfuscation_level
            .max(config.openflow.obfuscation_level);
        current
    }

    async fn apply_openflow_system_chain(
        &self,
        config: &PrivacyRouterConfig,
    ) -> Result<ApplyResult> {
        let plugin = OpenFlowPlugin::new();
        let current_state = simd_json::json!(null);
        let current_config: OpenFlowConfig =
            simd_json::serde::from_owned_value(current_state.clone())?;
        let desired_config = self.merge_openflow_config(current_config, config);
        let desired_value = simd_json::serde::to_owned_value(desired_config)?;
        let diff = plugin
            .calculate_diff(&current_state, &desired_value)
            .await?;
        if diff.actions.is_empty() {
            return Ok(ApplyResult {
                success: true,
                changes_applied: vec!["Privacy router OpenFlow chain already in sync".to_string()],
                errors: Vec::new(),
                checkpoint: None,
            });
        }
        plugin.apply_state(&diff).await
    }
}

struct SystemContainerSpec<'a> {
    name: &'a str,
    role: &'a str,
    socket_port: &'a str,
    resources: &'a ContainerResources,
}

fn chain_flow(index: usize, in_port: &str, out_port: &str) -> FlowEntry {
    FlowEntry {
        table: 0,
        priority: 21000,
        match_fields: HashMap::from([
            ("in_port".to_string(), in_port.to_string()),
            ("ip".to_string(), "".to_string()),
        ]),
        actions: vec![FlowAction::Output {
            port: out_port.to_string(),
        }],
        cookie: Some(SYSTEM_FLOW_COOKIE_PREFIX | index as u64),
        idle_timeout: 0,
        hard_timeout: 0,
    }
}

fn is_system_cookie(cookie: u64) -> bool {
    cookie & SYSTEM_FLOW_COOKIE_MASK == SYSTEM_FLOW_COOKIE_PREFIX
}

fn flow_sort_key(flow: &FlowEntry) -> (u8, u16, u64) {
    (flow.table, flow.priority, flow.cookie.unwrap_or_default())
}

#[async_trait]
impl StatePlugin for PrivacyRouterPlugin {
    fn name(&self) -> &'static str {
        "privacy_router"
    }

    fn version(&self) -> &str {
        "1.2.0"
    }

    fn schema(&self) -> Option<op_state_store::PluginSchema> {
        Some(privacy_router_schema())
    }

    fn capabilities(&self) -> PluginCapabilities {
        PluginCapabilities {
            supports_rollback: false,
            supports_checkpoints: true,
            supports_verification: true,
            atomic_operations: false,
        }
    }

    async fn calculate_diff(&self, current: &Value, desired: &Value) -> Result<StateDiff> {
        let mut actions = Vec::new();
        let current_config = current.get("config").unwrap_or(current);
        let desired_config = desired.get("config").unwrap_or(desired);
        let desired_runtime: PrivacyRouterConfig =
            simd_json::serde::from_owned_value(desired_config.clone())?;

        if current_config != desired_config
            || self.runtime_needs_reconcile(&desired_runtime).await?
        {
            actions.push(StateAction::Modify {
                resource: "privacy_router_config".to_string(),
                changes: desired.clone(),
            });
        }

        Ok(StateDiff {
            plugin: self.name().to_string(),
            actions,
            metadata: DiffMetadata {
                timestamp: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)?
                    .as_secs() as i64,
                current_hash: format!("{:x}", md5::compute(simd_json::to_string(current)?)),
                desired_hash: format!("{:x}", md5::compute(simd_json::to_string(desired)?)),
            },
        })
    }

    async fn apply_state(&self, diff: &StateDiff) -> Result<ApplyResult> {
        let config = self.desired_config_from_diff(diff)?;
        let mut changes_applied = Vec::new();
        let mut errors = Vec::new();

        self.ensure_host_bridge_topology(&config).await?;
        self.ensure_warp_interface_on_bridge(&config).await?;

        let incus_result = self.apply_incus_system_containers(&config).await?;
        changes_applied.extend(incus_result.changes_applied);
        errors.extend(incus_result.errors);

        if !errors.is_empty() {
            return Ok(ApplyResult {
                success: false,
                changes_applied,
                errors,
                checkpoint: None,
            });
        }

        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

        let openflow_result = self.apply_openflow_system_chain(&config).await?;
        changes_applied.extend(openflow_result.changes_applied);
        errors.extend(openflow_result.errors);

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

    async fn create_checkpoint(&self) -> Result<Checkpoint> {
        let state = simd_json::json!(null);
        Ok(Checkpoint {
            id: format!(
                "privacy_router_{}",
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

    async fn rollback(&self, checkpoint: &Checkpoint) -> Result<()> {
        log::info!(
            "Rolling back privacy router to checkpoint: {}",
            checkpoint.id
        );
        Err(anyhow::anyhow!(
            "Privacy router rollback not yet implemented"
        ))
    }
}

fn parse_cidr(cidr: &str) -> Result<(String, u8)> {
    let mut parts = cidr.split('/');
    let ip = parts
        .next()
        .ok_or_else(|| anyhow::anyhow!("invalid CIDR '{}': missing IP", cidr))?;
    let prefix = parts
        .next()
        .ok_or_else(|| anyhow::anyhow!("invalid CIDR '{}': missing prefix", cidr))?
        .parse::<u8>()
        .with_context(|| format!("invalid CIDR prefix in '{}'", cidr))?;
    if parts.next().is_some() {
        bail!("invalid CIDR '{}': too many separators", cidr);
    }
    Ok((ip.to_string(), prefix))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desired_config_merges_partial_overlay() {
        let plugin = PrivacyRouterPlugin::new(PrivacyRouterConfig::default());
        let diff = StateDiff {
            plugin: "privacy_router".to_string(),
            actions: vec![StateAction::Modify {
                resource: "privacy_router_config".to_string(),
                changes: json!({
                    "xray": {
                        "vps_address": "xray.example.com"
                    }
                }),
            }],
            metadata: DiffMetadata {
                timestamp: 0,
                current_hash: String::new(),
                desired_hash: String::new(),
            },
        };

        let config = plugin.desired_config_from_diff(&diff).expect("config");
        assert_eq!(config.xray.vps_address, "xray.example.com");
        assert_eq!(config.bridge_name, DEFAULT_BRIDGE_NAME);
    }

    #[test]
    fn chain_ports_follow_enabled_system_components() {
        let plugin = PrivacyRouterPlugin::new(PrivacyRouterConfig::default());
        let config = PrivacyRouterConfig::default();
        assert_eq!(
            plugin.chain_ports(&config),
            vec![
                config.wireguard.socket_port.clone(),
                config.warp.bridge_interface.clone(),
                config.xray.socket_port.clone(),
            ]
        );
    }

    #[test]
    fn desired_system_instance_sets_privileged_system_container_flags() {
        let plugin = PrivacyRouterPlugin::new(PrivacyRouterConfig::default());
        let config = PrivacyRouterConfig::default();
        let spec = SystemContainerSpec {
            name: "privacy-wireguard-ingress",
            role: "wireguard_ingress",
            socket_port: &config.wireguard.socket_port,
            resources: &config.wireguard.resources,
        };

        let instance = plugin.desired_system_instance(&config, &spec);
        let config = instance.config.expect("instance config");

        assert_eq!(config.get("security.nesting"), Some(&"true".to_string()));
        assert_eq!(config.get("security.privileged"), Some(&"true".to_string()));
    }

    #[test]
    fn host_bootstrap_defaults_keep_uplink_standalone() {
        let host = PrivacyHostBootstrapConfig::from_env("ovsbr0");
        assert_eq!(host.uplink_port, "ens3");
        assert!(!host.attach_uplink_to_bridge);
        assert_eq!(host.grpc_bridge_port, "grpc-bridge");
    }

    #[test]
    fn bool_env_accepts_common_true_values() {
        std::env::set_var("PRIVACY_ATTACH_UPLINK_TO_BRIDGE", "yes");
        assert!(bool_env("PRIVACY_ATTACH_UPLINK_TO_BRIDGE", false));
        std::env::remove_var("PRIVACY_ATTACH_UPLINK_TO_BRIDGE");
    }

    #[test]
    fn actual_system_containers_require_running_status() {
        let plugin = PrivacyRouterPlugin::new(PrivacyRouterConfig::default());
        let config = PrivacyRouterConfig::default();
        let instances = vec![
            IncusInstance {
                name: "privacy-wireguard-ingress".to_string(),
                status: "Stopped".to_string(),
                instance_type: "container".to_string(),
                image: None,
                storage_pool: None,
                profiles: Vec::new(),
                config: None,
                ..Default::default()
            },
            IncusInstance {
                name: "privacy-xray-egress".to_string(),
                status: "Running".to_string(),
                instance_type: "container".to_string(),
                image: None,
                storage_pool: None,
                profiles: Vec::new(),
                config: None,
                ..Default::default()
            },
        ];

        let actual = plugin.actual_system_containers(&config, &IncusState { instances });
        assert_eq!(actual, vec!["privacy-xray-egress".to_string()]);
    }
}

pub(crate) fn privacy_router_schema() -> PluginSchema {
    let wireguard_fields = {
        let mut fields = HashMap::new();
        fields.insert(
            "enabled".to_string(),
            FieldSchema {
                field_type: FieldType::Boolean,
                required: true,
                description: "Enable WireGuard tunnel".to_string(),
                default: Some(json!(true)),
                example: Some(json!(true)),
                constraints: Vec::new(),
                read_only: false,
                read_only_when: None,
            },
        );
        fields.insert(
            "container_id".to_string(),
            FieldSchema {
                field_type: FieldType::Integer,
                required: false,
                description: "Container VMID for WireGuard".to_string(),
                default: Some(json!(100)),
                example: Some(json!(100)),
                constraints: Vec::new(),
                read_only: false,
                read_only_when: Some(ReadOnlyCondition {
                    property: "enabled".to_string(),
                    value: "true".to_string(),
                }),
            },
        );
        fields.insert(
            "listen_port".to_string(),
            FieldSchema {
                field_type: FieldType::Integer,
                required: false,
                description: "WireGuard listen port".to_string(),
                default: Some(json!(51820)),
                example: Some(json!(51820)),
                constraints: vec![
                    Constraint::Min { value: 1.0 },
                    Constraint::Max { value: 65535.0 },
                ],
                read_only: false,
                read_only_when: None,
            },
        );
        fields.insert(
            "socket_port".to_string(),
            FieldSchema {
                field_type: FieldType::String,
                required: false,
                description: "Host-side bridge port name for the WireGuard ingress container"
                    .to_string(),
                default: Some(json!("gbr_wg")),
                example: Some(json!("gbr_wg")),
                constraints: Vec::new(),
                read_only: false,
                read_only_when: None,
            },
        );
        fields
    };

    let warp_fields = {
        let mut fields = HashMap::new();
        fields.insert(
            "enabled".to_string(),
            FieldSchema {
                field_type: FieldType::Boolean,
                required: true,
                description: "Enable Cloudflare WARP tunnel".to_string(),
                default: Some(json!(true)),
                example: Some(json!(true)),
                constraints: Vec::new(),
                read_only: false,
                read_only_when: None,
            },
        );
        fields.insert(
            "bridge_interface".to_string(),
            FieldSchema {
                field_type: FieldType::String,
                required: false,
                description: "Host WireGuard interface bridged into OVS for WARP egress"
                    .to_string(),
                default: Some(json!("wgcf")),
                example: Some(json!("wgcf")),
                constraints: Vec::new(),
                read_only: false,
                read_only_when: None,
            },
        );
        fields.insert(
            "netclient_network".to_string(),
            FieldSchema {
                field_type: FieldType::String,
                required: false,
                description: "Netclient network name for the WARP egress interface".to_string(),
                default: Some(json!("gbr_warp")),
                example: Some(json!("gbr_warp")),
                constraints: Vec::new(),
                read_only: false,
                read_only_when: None,
            },
        );
        fields
    };

    let xray_fields = {
        let mut fields = HashMap::new();
        fields.insert(
            "enabled".to_string(),
            FieldSchema {
                field_type: FieldType::Boolean,
                required: true,
                description: "Enable system XRay client tunnel".to_string(),
                default: Some(json!(true)),
                example: Some(json!(true)),
                constraints: Vec::new(),
                read_only: false,
                read_only_when: None,
            },
        );
        fields.insert(
            "container_id".to_string(),
            FieldSchema {
                field_type: FieldType::Integer,
                required: false,
                description: "Container VMID for the local XRay client".to_string(),
                default: Some(json!(101)),
                example: Some(json!(101)),
                constraints: Vec::new(),
                read_only: false,
                read_only_when: Some(ReadOnlyCondition {
                    property: "enabled".to_string(),
                    value: "true".to_string(),
                }),
            },
        );
        fields.insert(
            "socket_port".to_string(),
            FieldSchema {
                field_type: FieldType::String,
                required: false,
                description: "Host-side bridge port for the local XRay client".to_string(),
                default: Some(json!("gbr_xray")),
                example: Some(json!("gbr_xray")),
                constraints: Vec::new(),
                read_only: false,
                read_only_when: None,
            },
        );
        fields.insert(
            "socks_port".to_string(),
            FieldSchema {
                field_type: FieldType::Integer,
                required: false,
                description: "SOCKS listener port exposed by the local XRay client".to_string(),
                default: Some(json!(1080)),
                example: Some(json!(1080)),
                constraints: vec![
                    Constraint::Min { value: 1.0 },
                    Constraint::Max { value: 65535.0 },
                ],
                read_only: false,
                read_only_when: None,
            },
        );
        fields.insert(
            "vps_address".to_string(),
            FieldSchema {
                field_type: FieldType::String,
                required: false,
                description: "Remote XRay server hostname or IP".to_string(),
                default: Some(json!("vps.example.com")),
                example: Some(json!("vps.example.com")),
                constraints: Vec::new(),
                read_only: false,
                read_only_when: None,
            },
        );
        fields.insert(
            "vps_port".to_string(),
            FieldSchema {
                field_type: FieldType::Integer,
                required: false,
                description: "Remote XRay server port".to_string(),
                default: Some(json!(443)),
                example: Some(json!(443)),
                constraints: vec![
                    Constraint::Min { value: 1.0 },
                    Constraint::Max { value: 65535.0 },
                ],
                read_only: false,
                read_only_when: None,
            },
        );
        fields
    };

    let vps_fields = {
        let mut fields = HashMap::new();
        fields.insert(
            "xray_server".to_string(),
            FieldSchema {
                field_type: FieldType::String,
                required: true,
                description: "Remote XRay server hostname or IP".to_string(),
                default: Some(json!("vps.example.com")),
                example: Some(json!("vps.example.com")),
                constraints: Vec::new(),
                read_only: false,
                read_only_when: None,
            },
        );
        fields.insert(
            "xray_port".to_string(),
            FieldSchema {
                field_type: FieldType::Integer,
                required: true,
                description: "Remote XRay server port".to_string(),
                default: Some(json!(443)),
                example: Some(json!(443)),
                constraints: vec![
                    Constraint::Min { value: 1.0 },
                    Constraint::Max { value: 65535.0 },
                ],
                read_only: false,
                read_only_when: None,
            },
        );
        fields
    };

    let mut schema = PluginSchema::builder("privacy_router")
        .category("network")
        .version("1.1.0")
        .description("System privacy fabric (WireGuard/XRay ingress, WARP bridge, XRay egress)")
        .dependency("incus")
        .dependency("openflow")
        .dependency("privacy_routes")
        .string_field("bridge_name", true, "OVS bridge for privacy network")
        .object_field(
            "wireguard",
            wireguard_fields,
            true,
            "WireGuard tunnel config",
        )
        .object_field("warp", warp_fields, true, "Cloudflare WARP bridge config")
        .object_field(
            "xray",
            xray_fields,
            true,
            "XRay REALITY egress client config",
        )
        .object_field(
            "vps",
            vps_fields,
            true,
            "Remote XRay server endpoint config",
        )
        .example(json!({
            "bridge_name": "ovsbr0",
            "wireguard": {
                "enabled": true,
                "container_id": 100,
                "socket_port": "gbr_wg",
                "listen_port": 51820
            },
            "warp": {
                "enabled": true,
                "bridge_interface": "wgcf",
                "netclient_network": "gbr_warp"
            },
            "xray": {
                "enabled": true,
                "container_id": 101,
                "socket_port": "gbr_xray",
                "socks_port": 1080,
                "vps_address": "vps.example.com",
                "vps_port": 443
            },
            "vps": {
                "xray_server": "vps.example.com",
                "xray_port": 443
            }
        }))
        .build();

    // Add D-Bus methods for privacy router
    schema.methods.insert(
        "enforce_policy".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<EnforcePolicyInput, super::plugin_scaffold_helpers::AckOutput>(
            "EnforcePolicy",
            SideEffect::Mutation,
            false,
            "privacy_router.write",
            "mut.network.privacy.policy.enforce@v1",
        ),
    );
    schema.methods.insert(
        "audit_request".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<AuditRequestInput, super::plugin_scaffold_helpers::AckOutput>(
            "AuditRequest",
            SideEffect::Read,
            true,
            "privacy_router.read",
            "obs.network.privacy.request.audit@v1",
        ),
    );
    schema.methods.insert(
        "log_decision".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<LogDecisionInput, super::plugin_scaffold_helpers::AckOutput>(
            "LogDecision",
            SideEffect::Mutation,
            false,
            "privacy_router.write",
            "mut.network.privacy.decision.log@v1",
        ),
    );

    schema
}

// Self-registration: the plugin registry discovers this via inventory
// (single source of the catalog; no central dispatch list).
inventory::submit! {
    crate::default_registry::PluginReg::new("privacy_router", |_ctx| std::sync::Arc::new(PrivacyRouterPlugin::new(PrivacyRouterConfig::default())))
}

/// Input struct for EnforcePolicy method
/// D-Bus method spec: Privacy policy enforcement
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct EnforcePolicyInput {
    /// Policy ID to enforce
    pub policy_id: String,
    /// Target (user, IP, network)
    pub target: String,
    /// Action (allow, deny, mask)
    pub action: String,
}

/// Input struct for AuditRequest method
/// D-Bus method spec: Privacy request auditing
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AuditRequestInput {
    /// Request ID to audit
    pub request_id: String,
    /// Include details
    pub include_details: Option<bool>,
}

/// Input struct for LogDecision method
/// D-Bus method spec: Privacy decision logging
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct LogDecisionInput {
    /// Request ID
    pub request_id: String,
    /// Decision made
    pub decision: String,
    /// Reason
    pub reason: Option<String>,
}
