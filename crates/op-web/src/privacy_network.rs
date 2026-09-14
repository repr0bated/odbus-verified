//! Host-level privacy network provisioning — OVS switching fabric only.
//
// Current architecture (Artix Linux + s6 + Incus + rovs):
// - Host runs ovsbr0 via OVSDB (datapath=system)
// - All privacy services (Xray, mail) run on the HOST (xray via the
//   `gbr-xray` s6 service); the deprecated `wg-xray` Incus container is
//   stopped and no longer referenced.
// - Containers have NO host interface — they communicate via Unix sockets
//   through OVS internal ports using OpenFlow routing
// - Xray ingress now runs on the host (gbr-xray service), NOT inside a
//   container.
// - OpenFlow controller now runs on the host as well.
// - priv_* internal ports are created by ovs-attach-ports.sh

use anyhow::{Context, Result};
use op_network::{openflow::OpenFlowClient, rovs_proxy::OvsdbDbusClient as OvsdbClient};
use std::path::Path;
use tracing::{info, warn};

const DEFAULT_BRIDGE: &str = "ovsbr0";
const DEFAULT_PRIVACY_PORTS: &[&str] = &[
    "gbr_xray",
    "gbr_warp",
    "gbr_wg",
    "ovsbr0-mgmt",
    "ovsbr0-sock",
];
// TODO(host-xray): These constants still reference the old wg-xray container
// IP `10.200.0.1`. Xray ingress / OpenFlow controller now run on the host
// (the host grpc-uplink veth IP is `10.200.0.2`). Repoint these once the
// exact on-host xray ingress / OpenFlow controller addresses are confirmed.
const DEFAULT_MGMT_CIDR: &str = "10.200.0.1/24"; // Xray ingress IP (was inside wg-xray; now on host — TODO: confirm new value)
const DEFAULT_OPENFLOW_CONTROLLER: &str = "10.200.0.1:6653"; // Controller (was inside wg-xray; now on host — TODO: confirm new value)
const DEFAULT_DATAPATH_TYPE: &str = "system";
const DEFAULT_FAIL_MODE: &str = "standalone";

#[derive(Debug, Clone)]
pub struct PrivacyNetworkHostConfig {
    pub bridge_name: String,
    pub privacy_ports: Vec<String>,
    pub management_cidr: String,
    pub openflow_controller: String,
    pub xray_ingress_ip: String,
    pub datapath_type: String,
    pub fail_mode: String,
}

impl PrivacyNetworkHostConfig {
    pub fn from_env() -> Self {
        Self {
            bridge_name: std::env::var("PRIVACY_BRIDGE_NAME")
                .unwrap_or_else(|_| DEFAULT_BRIDGE.to_string()),
            privacy_ports: std::env::var("PRIVACY_PORTS")
                .map(|s| s.split(',').map(|s| s.trim().to_string()).collect())
                .unwrap_or_else(|_| {
                    DEFAULT_PRIVACY_PORTS
                        .iter()
                        .map(|&s| s.to_string())
                        .collect()
                }),
            management_cidr: std::env::var("PRIVACY_MGMT_CIDR")
                .unwrap_or_else(|_| DEFAULT_MGMT_CIDR.to_string()),
            openflow_controller: std::env::var("PRIVACY_OPENFLOW_CONTROLLER")
                .unwrap_or_else(|_| DEFAULT_OPENFLOW_CONTROLLER.to_string()),
            xray_ingress_ip: std::env::var("XRAY_INGRESS_IP")
                .unwrap_or_else(|_| "10.200.0.1".to_string()), // TODO(host-xray): was wg-xray container IP; xray now runs on host (gbr-xray). Confirm new value.
            datapath_type: std::env::var("PRIVACY_DATAPATH_TYPE")
                .unwrap_or_else(|_| DEFAULT_DATAPATH_TYPE.to_string()),
            fail_mode: std::env::var("PRIVACY_FAIL_MODE")
                .unwrap_or_else(|_| DEFAULT_FAIL_MODE.to_string()),
        }
    }
}

/// Ensure host privacy network topology exists for wgcf architecture.
///
/// This is called during magic link verification to ensure the network
/// is ready for new users.
pub async fn ensure_host_privacy_network() -> Result<()> {
    let cfg = PrivacyNetworkHostConfig::from_env();
    ensure_host_privacy_network_with_config(&cfg).await
}

async fn ensure_host_privacy_network_with_config(cfg: &PrivacyNetworkHostConfig) -> Result<()> {
    info!(
        "Ensuring OVS privacy fabric: bridge={} ports={:?}",
        cfg.bridge_name, cfg.privacy_ports
    );

    let ovs = OvsdbClient::new();

    // Verify OVSDB connectivity
    ovs.list_dbs()
        .await
        .context("Open vSwitch DB is unavailable; cannot provision privacy network")?;

    // The bridge should be created by netplan/op-ovsbr0-setup, but ensure it exists
    if !ovs
        .bridge_exists(&cfg.bridge_name)
        .await
        .context("Failed to check bridge existence")?
    {
        info!("Bridge {} not found, creating via OVSDB", cfg.bridge_name);
        ovs.create_bridge(&cfg.bridge_name)
            .await
            .with_context(|| format!("Failed to create OVS bridge '{}'", cfg.bridge_name))?;
    }

    // Configure bridge for controller-driven forwarding.
    info!(
        "Configuring {} datapath_type={} fail_mode={}",
        cfg.bridge_name, cfg.datapath_type, cfg.fail_mode
    );
    ovs.set_bridge_property(&cfg.bridge_name, "datapath_type", &cfg.datapath_type)
        .await
        .with_context(|| format!("Failed to set bridge datapath_type={}", cfg.datapath_type))?;
    ovs.set_bridge_property(&cfg.bridge_name, "fail_mode", &cfg.fail_mode)
        .await
        .with_context(|| format!("Failed to set bridge fail_mode={}", cfg.fail_mode))?;

    let existing_ports = ovs
        .list_bridge_ports(&cfg.bridge_name)
        .await
        .context("Failed to list bridge ports")?;

    // NOTE: wgcf/Xray now run on the host (gbr-xray s6 service), not as
    // container interfaces. The deprecated wg-xray container is no longer
    // referenced. Host traffic reaches the bridge via OVS internal ports.

    // Ensure all privacy internal ports exist (created by ovs-attach-ports.sh)
    for port in &cfg.privacy_ports {
        if !existing_ports.iter().any(|p| p == port) {
            ovs.add_port_with_type(&cfg.bridge_name, port, Some("internal"))
                .await
                .with_context(|| format!("Failed to add internal port '{}'", port))?;
            info!("Added internal port: {}", port);
        }
    }

    // Bring up host bridge only (containers have no host interfaces)
    if Path::new(&format!("/sys/class/net/{}", cfg.bridge_name)).exists() {
        op_network::rtnetlink::link_up(&cfg.bridge_name)
            .await
            .with_context(|| format!("Failed to bring up interface {}", cfg.bridge_name))?;
    }

    // Bring up privacy internal ports
    for port in &cfg.privacy_ports {
        if Path::new(&format!("/sys/class/net/{}", port)).exists() {
            op_network::rtnetlink::link_up(port)
                .await
                .with_context(|| format!("Failed to bring up port {}", port))?;
        }
    }

    // Verify Xray ingress is available on the management IP
    // NOTE: Xray now runs on the host via the gbr-xray s6 service (the
    // deprecated wg-xray container is gone). The management IP is reachable
    // through OVS internal ports + OpenFlow.
    // TODO(host-xray): confirm the on-host xray ingress management IP.
    info!(
        "Privacy network ready. Xray ingress (on host, gbr-xray) should be listening on {}",
        cfg.xray_ingress_ip
    );

    // Probe OpenFlow controller if available
    // NOTE: Controller now runs on the host, reachable via OVS tunnel.
    // TODO(host-xray): confirm the on-host OpenFlow controller address.
    if let Ok(controller_addr) = cfg.openflow_controller.parse::<std::net::SocketAddr>() {
        match OpenFlowClient::connect(controller_addr).await {
            Ok(_) => info!("OpenFlow controller reachable (via host path)"),
            Err(e) => warn!("OpenFlow controller not ready yet: {}", e),
        }
    }

    info!("OVS privacy fabric provisioning complete (host services via OVS internal ports)");
    Ok(())
}
