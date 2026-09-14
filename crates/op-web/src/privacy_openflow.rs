use anyhow::Result;
use serde::{Deserialize, Serialize};
use simd_json::OwnedValue as Value;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};

use crate::privacy_routes::{PrivacyRoute, PrivacyRoutesState};

const DEFAULT_BRIDGE_NAME: &str = "ovsbr0";
const PRIVACY_FLOW_COOKIE_PREFIX: u64 = 0x5052_0000_0000_0000;
const PRIVACY_FLOW_COOKIE_MASK: u64 = 0xFFFF_0000_0000_0000;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OpenFlowConfig {
    pub bridges: Vec<BridgeFlowConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub controller_endpoint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flow_policies: Option<Vec<Value>>,
    pub auto_discover_containers: bool,
    pub enable_security_flows: bool,
    pub obfuscation_level: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BridgeFlowConfig {
    pub name: String,
    pub flows: Vec<FlowEntry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub socket_ports: Option<Vec<SocketPort>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FlowEntry {
    pub table: u8,
    pub priority: u16,
    pub match_fields: HashMap<String, String>,
    pub actions: Vec<FlowAction>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cookie: Option<u64>,
    #[serde(default)]
    pub idle_timeout: u16,
    #[serde(default)]
    pub hard_timeout: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FlowAction {
    Output { port: String },
    LoadRegister { register: u8, value: u64 },
    Resubmit { table: u8 },
    SetField { field: String, value: String },
    Drop,
    Normal,
    Controller { max_len: Option<u16> },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SocketPort {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub container_name: Option<String>,
    pub port_type: SocketPortType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ofport: Option<u16>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SocketPortType {
    Privacy,
    SharedIngress,
    Container,
}

pub async fn publish_openflow_for_privacy_routes() -> Result<()> {
    let routes =
        crate::state_manager_client::query_plugin_state::<PrivacyRoutesState>("privacy_routes")
            .await?
            .unwrap_or(PrivacyRoutesState { routes: Vec::new() });
    let existing = crate::state_manager_client::query_plugin_state::<OpenFlowConfig>("openflow")
        .await?
        .unwrap_or_else(empty_openflow_config);

    let desired = merge_privacy_routes(existing, &routes);
    crate::state_manager_client::apply_plugin_state("openflow", &desired).await?;
    Ok(())
}

fn empty_openflow_config() -> OpenFlowConfig {
    OpenFlowConfig {
        bridges: Vec::new(),
        controller_endpoint: None,
        flow_policies: None,
        auto_discover_containers: false,
        enable_security_flows: false,
        obfuscation_level: 0,
    }
}

fn merge_privacy_routes(
    mut existing: OpenFlowConfig,
    routes: &PrivacyRoutesState,
) -> OpenFlowConfig {
    let mut routes_by_bridge: HashMap<String, Vec<&PrivacyRoute>> = HashMap::new();
    for route in routes.routes.iter().filter(|route| route.enabled) {
        routes_by_bridge
            .entry(bridge_name_for_route(route))
            .or_default()
            .push(route);
    }

    let managed_bridges: HashSet<String> = existing
        .bridges
        .iter()
        .filter(|bridge| {
            bridge
                .flows
                .iter()
                .any(|flow| flow.cookie.is_some_and(is_privacy_managed_cookie))
                || bridge
                    .socket_ports
                    .as_ref()
                    .is_some_and(|ports| ports.iter().any(is_shared_ingress_socket))
        })
        .map(|bridge| bridge.name.clone())
        .collect();

    let mut bridges = Vec::new();
    for bridge in existing.bridges.drain(..) {
        let bridge_routes = routes_by_bridge.get(&bridge.name);
        if bridge_routes.is_none() && !managed_bridges.contains(&bridge.name) {
            bridges.push(bridge);
            continue;
        }

        bridges.push(merge_bridge_config(
            bridge,
            bridge_routes.cloned().unwrap_or_default(),
        ));
    }

    for (bridge_name, bridge_routes) in routes_by_bridge {
        if bridges.iter().any(|bridge| bridge.name == bridge_name) {
            continue;
        }
        bridges.push(merge_bridge_config(
            BridgeFlowConfig {
                name: bridge_name,
                flows: Vec::new(),
                socket_ports: None,
            },
            bridge_routes,
        ));
    }

    bridges.sort_by(|a, b| a.name.cmp(&b.name));
    existing.bridges = bridges;
    existing.auto_discover_containers = false;
    existing.flow_policies = None;
    existing
}

fn merge_bridge_config(
    mut bridge: BridgeFlowConfig,
    routes: Vec<&PrivacyRoute>,
) -> BridgeFlowConfig {
    bridge
        .flows
        .retain(|flow| !flow.cookie.is_some_and(is_privacy_managed_cookie));

    if let Some(socket_ports) = bridge.socket_ports.as_mut() {
        socket_ports.retain(|port| !is_shared_ingress_socket(port));
    }

    let mut shared_ports: HashMap<String, SocketPort> = HashMap::new();
    for route in routes {
        bridge.flows.push(route_forward_flow(route));
        bridge.flows.push(route_return_flow(route));
        shared_ports
            .entry(route.ingress_port.clone())
            .or_insert_with(|| SocketPort {
                name: route.ingress_port.clone(),
                container_name: None,
                port_type: SocketPortType::SharedIngress,
                ofport: None,
            });
    }

    bridge.flows.sort_by_key(flow_sort_key);
    if shared_ports.is_empty() {
        if bridge
            .socket_ports
            .as_ref()
            .is_some_and(|ports| ports.is_empty())
        {
            bridge.socket_ports = None;
        }
    } else {
        let socket_ports = bridge.socket_ports.get_or_insert_with(Vec::new);
        socket_ports.extend(shared_ports.into_values());
        socket_ports.sort_by(|a, b| a.name.cmp(&b.name));
        socket_ports.dedup_by(|a, b| a.name == b.name);
    }
    bridge
}

fn flow_sort_key(flow: &FlowEntry) -> (u8, u16, u64) {
    (flow.table, flow.priority, flow.cookie.unwrap_or_default())
}

fn bridge_name_for_route(route: &PrivacyRoute) -> String {
    route
        .ingress_port
        .strip_suffix("-sock")
        .filter(|name| !name.is_empty())
        .unwrap_or(DEFAULT_BRIDGE_NAME)
        .to_string()
}

fn route_forward_flow(route: &PrivacyRoute) -> FlowEntry {
    FlowEntry {
        table: 0,
        priority: 22000,
        match_fields: HashMap::from([
            ("in_port".to_string(), route.ingress_port.clone()),
            ("ip".to_string(), "".to_string()),
            ("nw_src".to_string(), route.selector_ip.clone()),
        ]),
        actions: vec![FlowAction::Output {
            port: route.next_hop.clone(),
        }],
        cookie: Some(privacy_flow_cookie(&route.route_id, false)),
        idle_timeout: 0,
        hard_timeout: 0,
    }
}

fn route_return_flow(route: &PrivacyRoute) -> FlowEntry {
    FlowEntry {
        table: 0,
        priority: 22000,
        match_fields: HashMap::from([
            ("in_port".to_string(), route.next_hop.clone()),
            ("ip".to_string(), "".to_string()),
            ("nw_dst".to_string(), route.selector_ip.clone()),
        ]),
        actions: vec![FlowAction::Output {
            port: route.ingress_port.clone(),
        }],
        cookie: Some(privacy_flow_cookie(&route.route_id, true)),
        idle_timeout: 0,
        hard_timeout: 0,
    }
}

fn privacy_flow_cookie(route_id: &str, return_path: bool) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    route_id.hash(&mut hasher);
    return_path.hash(&mut hasher);
    PRIVACY_FLOW_COOKIE_PREFIX | (hasher.finish() & !PRIVACY_FLOW_COOKIE_MASK)
}

fn is_privacy_managed_cookie(cookie: u64) -> bool {
    cookie & PRIVACY_FLOW_COOKIE_MASK == PRIVACY_FLOW_COOKIE_PREFIX
}

fn is_shared_ingress_socket(port: &SocketPort) -> bool {
    port.port_type == SocketPortType::SharedIngress
}

#[cfg(test)]
mod tests {
    use super::*;

    fn route(route_id: &str, selector_ip: &str) -> PrivacyRoute {
        PrivacyRoute {
            name: route_id.to_string(),
            route_id: route_id.to_string(),
            user_id: route_id.to_string(),
            email: format!("{route_id}@example.com"),
            wireguard_public_key: "pubkey".to_string(),
            assigned_ip: format!("{selector_ip}/32"),
            selector_ip: selector_ip.to_string(),
            container_name: Some(format!("privacy-user-{route_id}")),
            ingress_port: "ovsbr0-sock".to_string(),
            next_hop: "gbr_wg".to_string(),
            enabled: true,
            created_at: "2026-01-01T00:00:00Z".to_string(),
            updated_at: "2026-01-01T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn merge_routes_keeps_non_privacy_flows_and_replaces_managed_ones() {
        let existing = OpenFlowConfig {
            bridges: vec![BridgeFlowConfig {
                name: "ovsbr0".to_string(),
                flows: vec![
                    FlowEntry {
                        table: 0,
                        priority: 1,
                        match_fields: HashMap::new(),
                        actions: vec![FlowAction::Normal],
                        cookie: Some(1),
                        idle_timeout: 0,
                        hard_timeout: 0,
                    },
                    route_forward_flow(&route("stale", "10.0.0.2")),
                ],
                socket_ports: Some(vec![SocketPort {
                    name: "ovsbr0-sock".to_string(),
                    container_name: None,
                    port_type: SocketPortType::SharedIngress,
                    ofport: None,
                }]),
            }],
            ..empty_openflow_config()
        };
        let routes = PrivacyRoutesState {
            routes: vec![route("fresh", "10.0.0.3")],
        };

        let merged = merge_privacy_routes(existing, &routes);
        let bridge = &merged.bridges[0];
        assert_eq!(bridge.flows.len(), 3);
        assert!(bridge.flows.iter().any(|flow| flow.cookie == Some(1)));
        assert!(bridge.flows.iter().any(|flow| {
            flow.cookie == Some(privacy_flow_cookie("fresh", false))
                && flow.match_fields.get("nw_src") == Some(&"10.0.0.3".to_string())
        }));
    }

    #[test]
    fn bridge_name_is_derived_from_shared_ingress_port() {
        let route = PrivacyRoute {
            ingress_port: "edge0-sock".to_string(),
            ..route("route-a", "10.0.0.2")
        };
        assert_eq!(bridge_name_for_route(&route), "edge0");
    }
}
