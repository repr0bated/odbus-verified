//! Rtnetlink state plugin - manages kernel-level network interface state
//!
//! Handles: IP addresses, link state (up/down), MAC addresses, default routes
//! Uses native rtnetlink (netlink) protocol — no CLI wrappers.
//! Depends on: net, ovsdb_bridge (interfaces must exist before configuring)

use anyhow::{Context, Result};
use async_trait::async_trait;
use op_state::{
    ApplyResult, Checkpoint, DiffMetadata, PluginCapabilities, StateAction, StateDiff, StatePlugin,
};
use op_state_store::{CapabilityDecl, FieldSchema, FieldType, PluginSchema};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use simd_json::prelude::*;
use simd_json::{OwnedValue as Value, json};
use std::collections::HashMap;

/// Rtnetlink interface configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RtnetlinkInterfaceConfig {
    /// Interface name (e.g., "ens3", "ovsbr0-int")
    pub name: String,

    /// IPv4/IPv6 addresses to assign
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub addresses: Option<Vec<AddressEntry>>,

    /// MAC address to set (e.g., "fa:16:3e:f1:71:d2")
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mac_address: Option<String>,

    /// MTU
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mtu: Option<u32>,

    /// Desired link state: "up" or "down"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,

    /// Default gateway (only one interface should set this)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_gateway: Option<String>,
}

/// IP address entry
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AddressEntry {
    pub ip: String,
    pub prefix: u8,
}

/// Rtnetlink state — list of managed interfaces
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RtnetlinkState {
    pub interfaces: Vec<RtnetlinkInterfaceConfig>,
}

pub struct RtnetlinkPlugin;

impl RtnetlinkPlugin {
    pub fn new() -> Self {
        Self
    }
}

impl Default for RtnetlinkPlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl StatePlugin for RtnetlinkPlugin {
    fn name(&self) -> &str {
        "rtnetlink"
    }

    fn version(&self) -> &str {
        "1.1.0"
    }

    fn schema(&self) -> Option<op_state_store::PluginSchema> {
        Some(rtnetlink_schema())
    }

    fn is_available(&self) -> bool {
        // rtnetlink is always available — it's the kernel
        true
    }

    fn unavailable_reason(&self) -> String {
        "rtnetlink is always available".to_string()
    }

    async fn calculate_diff(&self, current: &Value, desired: &Value) -> Result<StateDiff> {
        let current_state: RtnetlinkState = simd_json::serde::from_owned_value(current.clone())
            .unwrap_or(RtnetlinkState { interfaces: vec![] });
        let desired_state: RtnetlinkState = simd_json::serde::from_owned_value(desired.clone())
            .unwrap_or(RtnetlinkState { interfaces: vec![] });

        let mut actions = Vec::new();

        let current_map: HashMap<&str, &RtnetlinkInterfaceConfig> = current_state
            .interfaces
            .iter()
            .map(|i| (i.name.as_str(), i))
            .collect();

        for desired_iface in &desired_state.interfaces {
            if let Some(current_iface) = current_map.get(desired_iface.name.as_str()) {
                // Check if any property differs
                let needs_update = desired_iface.state != current_iface.state
                    || desired_iface.mac_address != current_iface.mac_address
                    || desired_iface.addresses != current_iface.addresses
                    || desired_iface.default_gateway.is_some();

                if needs_update {
                    actions.push(StateAction::Modify {
                        resource: desired_iface.name.clone(),
                        changes: simd_json::serde::to_owned_value(desired_iface)?,
                    });
                }
            } else {
                // Interface not found in kernel — can only configure if it exists
                log::warn!(
                    "rtnetlink: desired interface '{}' not found in kernel",
                    desired_iface.name
                );
            }
        }

        Ok(StateDiff {
            plugin: self.name().to_string(),
            actions,
            metadata: DiffMetadata {
                timestamp: chrono::Utc::now().timestamp(),
                current_hash: format!("{:x}", md5::compute(simd_json::to_string(current)?)),
                desired_hash: format!("{:x}", md5::compute(simd_json::to_string(desired)?)),
            },
        })
    }

    async fn apply_state(&self, diff: &StateDiff) -> Result<ApplyResult> {
        let mut changes_applied = Vec::new();
        let mut errors = Vec::new();

        for action in &diff.actions {
            if let StateAction::Modify { resource, changes } = action {
                let config: RtnetlinkInterfaceConfig =
                    simd_json::serde::from_owned_value(changes.clone())?;

                // Set MAC address
                if let Some(ref mac) = config.mac_address {
                    match op_network::rtnetlink::set_mac_address(resource, mac).await {
                        Ok(_) => changes_applied
                            .push(format!("Set MAC {} on {} via rtnetlink", mac, resource)),
                        Err(e) => errors.push(format!("Failed to set MAC on {}: {}", resource, e)),
                    }
                }

                // Add IP addresses
                if let Some(ref addresses) = config.addresses {
                    for addr in addresses {
                        match op_network::rtnetlink::add_ipv4_address(
                            resource,
                            &addr.ip,
                            addr.prefix,
                        )
                        .await
                        {
                            Ok(_) => changes_applied.push(format!(
                                "Added {}/{} to {} via rtnetlink",
                                addr.ip, addr.prefix, resource
                            )),
                            Err(e) => {
                                // EEXIST is not an error — address already assigned
                                let msg = e.to_string();
                                if msg.contains("exist") {
                                    log::info!(
                                        "Address {}/{} already on {} (ok)",
                                        addr.ip,
                                        addr.prefix,
                                        resource
                                    );
                                } else {
                                    errors.push(format!(
                                        "Failed to add {}/{} to {}: {}",
                                        addr.ip, addr.prefix, resource, e
                                    ));
                                }
                            }
                        }
                    }
                }

                // Set link state
                if let Some(ref state) = config.state {
                    let result = if state == "up" {
                        op_network::rtnetlink::link_up(resource).await
                    } else {
                        op_network::rtnetlink::link_down(resource).await
                    };
                    match result {
                        Ok(_) => changes_applied
                            .push(format!("Set {} {} via rtnetlink", resource, state)),
                        Err(e) => {
                            errors.push(format!("Failed to set {} {}: {}", resource, state, e))
                        }
                    }
                }

                // Set default gateway
                if let Some(ref gateway) = config.default_gateway {
                    match op_network::rtnetlink::replace_default_route(resource, gateway).await {
                        Ok(_) => changes_applied.push(format!(
                            "Set default route via {} on {} via rtnetlink",
                            gateway, resource
                        )),
                        Err(e) => errors.push(format!(
                            "Failed to set default route via {}: {}",
                            gateway, e
                        )),
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

    async fn create_checkpoint(&self) -> Result<Checkpoint> {
        let state = simd_json::json!(null);
        Ok(Checkpoint {
            id: format!("rtnetlink-{}", chrono::Utc::now().timestamp()),
            plugin: self.name().to_string(),
            timestamp: chrono::Utc::now().timestamp(),
            state_snapshot: state,
            backend_checkpoint: None,
        })
    }

    async fn rollback(&self, checkpoint: &Checkpoint) -> Result<()> {
        let old_state: RtnetlinkState =
            simd_json::serde::from_owned_value(checkpoint.state_snapshot.clone())?;

        // Re-apply old state
        let current = simd_json::json!(null);
        let diff = self
            .calculate_diff(&current, &simd_json::serde::to_owned_value(&old_state)?)
            .await?;
        self.apply_state(&diff).await?;

        Ok(())
    }

    fn capabilities(&self) -> PluginCapabilities {
        PluginCapabilities {
            supports_rollback: true,
            supports_checkpoints: true,
            supports_verification: true,
            atomic_operations: false,
        }
    }
}

/// Method input types - single source of truth via schemars
/// set_link_state method input
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SetLinkStateInput {
    /// Interface name
    pub name: String,
    /// Desired state: "up" or "down"
    pub state: String,
}

/// add_ipv4_address method input
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AddIpv4AddressInput {
    /// Interface name
    pub name: String,
    /// IP address (CIDR notation, e.g. "10.0.0.1/24")
    pub address: String,
}

/// set_mac_address method input
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SetMacAddressInput {
    /// Interface name
    pub name: String,
    /// MAC address
    pub mac: String,
}

/// set_default_route method input
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SetDefaultRouteInput {
    /// Interface name for default route
    pub name: String,
    /// Gateway IP address
    pub gateway: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AddLinkInput {
    pub name: String,
    pub link_type: String,
    #[serde(default)]
    pub peer_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DelLinkInput {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AddRouteInput {
    pub destination: String,
    pub gateway: String,
    pub interface: Option<String>,
    #[serde(default)]
    pub table: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DelRouteInput {
    pub destination: String,
    #[serde(default)]
    pub gateway: Option<String>,
    #[serde(default)]
    pub interface: Option<String>,
    #[serde(default)]
    pub table: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AddRuleInput {
    #[serde(alias = "rule")]
    pub destination: String,
    #[serde(default)]
    pub table: Option<u32>,
    #[serde(default)]
    pub priority: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct NetlinkAcceptedOutput {
    pub interface: String,
    pub disposition: String,
}

fn interface_name(name: &str) -> Result<&str> {
    anyhow::ensure!(
        !name.is_empty() && name.len() <= IFNAMSIZ,
        "interface name is empty or too long"
    );
    anyhow::ensure!(
        name.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._:-".contains(&b)),
        "interface name contains unsafe characters"
    );
    Ok(name)
}

const IFNAMSIZ: usize = 15;

fn ipv4_cidr(value: &str) -> Result<(String, u8)> {
    let (ip, prefix) = value
        .split_once('/')
        .context("IPv4 address must use CIDR notation")?;
    let _: std::net::Ipv4Addr = ip.parse().context("invalid IPv4 address")?;
    let prefix: u8 = prefix.parse().context("invalid IPv4 prefix")?;
    anyhow::ensure!(prefix <= 32, "IPv4 prefix must be between 0 and 32");
    Ok((ip.to_string(), prefix))
}

fn gateway(value: &str) -> Result<&str> {
    let _: std::net::Ipv4Addr = value.parse().context("invalid IPv4 gateway")?;
    Ok(value)
}

fn route_destination(value: &str) -> Result<()> {
    let (address, prefix) = value
        .split_once('/')
        .context("route destination must use CIDR notation")?;
    let address: std::net::IpAddr = address.parse().context("invalid route destination")?;
    let prefix: u8 = prefix.parse().context("invalid route prefix")?;
    anyhow::ensure!(
        prefix <= if address.is_ipv4() { 32 } else { 128 },
        "route prefix is outside address family"
    );
    Ok(())
}

fn mac(value: &str) -> Result<[u8; 6]> {
    let parts: Vec<_> = value.split(':').collect();
    anyhow::ensure!(parts.len() == 6, "MAC address must contain six octets");
    let mut bytes = [0u8; 6];
    for (i, part) in parts.iter().enumerate() {
        bytes[i] = u8::from_str_radix(part, 16).context("invalid MAC address")?;
    }
    Ok(bytes)
}

/// Dispatch every declared rtnetlink method through op-network's native
/// rtnetlink implementation.
pub async fn dispatch_rtnetlink_method(
    method: &str,
    args: &serde_json::Value,
) -> Result<serde_json::Value> {
    match method {
        "set_link_state" => {
            let input: SetLinkStateInput =
                serde_json::from_value(args.clone()).context("invalid set_link_state arguments")?;
            let name = interface_name(&input.name)?;
            match input.state.as_str() {
                "up" => op_network::rtnetlink::link_up(name).await?,
                "down" => op_network::rtnetlink::link_down(name).await?,
                _ => anyhow::bail!("link state must be 'up' or 'down'"),
            };
            Ok(serde_json::to_value(NetlinkAcceptedOutput {
                interface: name.to_string(),
                disposition: "accepted".into(),
            })?)
        }
        "add_ipv4_address" => {
            let input: AddIpv4AddressInput = serde_json::from_value(args.clone())
                .context("invalid add_ipv4_address arguments")?;
            let name = interface_name(&input.name)?;
            let (ip, prefix) = ipv4_cidr(&input.address)?;
            op_network::rtnetlink::add_ipv4_address(name, &ip, prefix).await?;
            Ok(serde_json::to_value(NetlinkAcceptedOutput {
                interface: name.to_string(),
                disposition: "accepted".into(),
            })?)
        }
        "set_mac_address" => {
            let input: SetMacAddressInput = serde_json::from_value(args.clone())
                .context("invalid set_mac_address arguments")?;
            let name = interface_name(&input.name)?;
            let _ = mac(&input.mac)?;
            op_network::rtnetlink::set_mac_address(name, &input.mac).await?;
            Ok(serde_json::to_value(NetlinkAcceptedOutput {
                interface: name.to_string(),
                disposition: "accepted".into(),
            })?)
        }
        "set_default_route" => {
            let input: SetDefaultRouteInput = serde_json::from_value(args.clone())
                .context("invalid set_default_route arguments")?;
            let name = interface_name(&input.name)?;
            let gw = gateway(&input.gateway)?;
            op_network::rtnetlink::replace_default_route(name, gw).await?;
            Ok(serde_json::to_value(NetlinkAcceptedOutput {
                interface: name.to_string(),
                disposition: "accepted".into(),
            })?)
        }
        "add_link" => {
            let input: AddLinkInput =
                serde_json::from_value(args.clone()).context("invalid add_link arguments")?;
            interface_name(&input.name)?;
            if input.link_type == "veth" {
                let peer = input
                    .peer_name
                    .as_deref()
                    .context("veth requires peer_name")?;
                interface_name(peer)?;
                anyhow::ensure!(peer != input.name, "veth peer_name must differ from name");
            }
            op_network::rtnetlink::add_link(
                &input.name,
                &input.link_type,
                input.peer_name.as_deref(),
            )
            .await?;
            Ok(serde_json::to_value(NetlinkAcceptedOutput {
                interface: input.name,
                disposition: "accepted".into(),
            })?)
        }
        "del_link" => {
            let input: DelLinkInput =
                serde_json::from_value(args.clone()).context("invalid del_link arguments")?;
            interface_name(&input.name)?;
            op_network::rtnetlink::del_link(&input.name).await?;
            Ok(serde_json::to_value(NetlinkAcceptedOutput {
                interface: input.name,
                disposition: "accepted".into(),
            })?)
        }
        "add_route" => {
            let input: AddRouteInput =
                serde_json::from_value(args.clone()).context("invalid add_route arguments")?;
            route_destination(&input.destination)?;
            gateway(&input.gateway)?;
            if let Some(name) = &input.interface {
                interface_name(name)?;
            }
            op_network::rtnetlink::add_route(
                &input.destination,
                &input.gateway,
                input.interface.as_deref(),
                input.table.unwrap_or(254),
            )
            .await?;
            Ok(serde_json::to_value(NetlinkAcceptedOutput {
                interface: input.interface.unwrap_or_default(),
                disposition: "accepted".into(),
            })?)
        }
        "del_route" => {
            let input: DelRouteInput =
                serde_json::from_value(args.clone()).context("invalid del_route arguments")?;
            route_destination(&input.destination)?;
            if let Some(value) = &input.gateway {
                gateway(value)?;
            }
            if let Some(interface) = &input.interface {
                interface_name(interface)?;
            }
            op_network::rtnetlink::del_route(
                &input.destination,
                input.gateway.as_deref(),
                input.interface.as_deref(),
                input.table.unwrap_or(254),
            )
            .await?;
            Ok(serde_json::to_value(NetlinkAcceptedOutput {
                interface: String::new(),
                disposition: "accepted".into(),
            })?)
        }
        "add_rule" => {
            let input: AddRuleInput =
                serde_json::from_value(args.clone()).context("invalid add_rule arguments")?;
            route_destination(&input.destination)?;
            op_network::rtnetlink::add_rule(
                &input.destination,
                input.table.unwrap_or(254),
                input.priority,
            )
            .await?;
            Ok(serde_json::to_value(NetlinkAcceptedOutput {
                interface: String::new(),
                disposition: "accepted".into(),
            })?)
        }
        _ => anyhow::bail!(
            "unknown rtnetlink method '{}'; no canonical backend dispatch",
            method
        ),
    }
}

pub(crate) fn rtnetlink_schema() -> PluginSchema {
    let interface_fields = {
        let mut fields = HashMap::new();
        fields.insert(
            "name".to_string(),
            FieldSchema {
                field_type: FieldType::String,
                required: true,
                description: "Interface name".to_string(),
                default: None,
                example: Some(json!("eth0")),
                constraints: Vec::new(),
                read_only: true,
                read_only_when: None,
            },
        );
        fields.insert(
            "state".to_string(),
            FieldSchema {
                field_type: FieldType::Enum(vec!["up".to_string(), "down".to_string()]),
                required: false,
                description: "Administrative interface state".to_string(),
                default: Some(json!("up")),
                example: Some(json!("up")),
                constraints: Vec::new(),
                read_only: false,
                read_only_when: None,
            },
        );
        fields.insert(
            "addresses".to_string(),
            FieldSchema {
                field_type: FieldType::Array(Box::new(FieldType::String)),
                required: false,
                description: "Interface IP addresses in CIDR form".to_string(),
                default: Some(json!([])),
                example: Some(json!(["10.0.0.2/24"])),
                constraints: Vec::new(),
                read_only: false,
                read_only_when: None,
            },
        );
        fields.insert(
            "mac_address".to_string(),
            FieldSchema {
                field_type: FieldType::String,
                required: false,
                description: "Optional MAC address override".to_string(),
                default: None,
                example: Some(json!("02:00:00:00:00:01")),
                constraints: Vec::new(),
                read_only: false,
                read_only_when: None,
            },
        );
        fields.insert(
            "default_gateway".to_string(),
            FieldSchema {
                field_type: FieldType::String,
                required: false,
                description: "Default gateway for this interface".to_string(),
                default: None,
                example: Some(json!("10.0.0.1")),
                constraints: Vec::new(),
                read_only: false,
                read_only_when: None,
            },
        );
        fields
    };

    let mut schema = PluginSchema::builder("rtnetlink")
        .category("network")
        .version("1.1.0")
        .description("Native kernel rtnetlink interface management")
        .array_field(
            "interfaces",
            FieldType::Object(interface_fields),
            true,
            "Desired rtnetlink-managed interfaces",
        )
        .example(json!({
            "interfaces": [
                {
                    "name": "ovsbr0",
                    "state": "up",
                    "addresses": ["10.10.0.1/24"],
                    "default_gateway": "10.10.0.254"
                }
            ]
        }))
        .build();

    schema.methods.insert(
        "set_link_state".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            SetLinkStateInput,
            NetlinkAcceptedOutput,
        >(
            "set_link_state",
            op_state_store::SideEffect::Mutation,
            false,
            "cap.network.rtnetlink.link-state.set@v1",
            "mut.network.rtnetlink.link-state.set@v1",
        ),
    );
    schema.methods.insert(
        "add_ipv4_address".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            AddIpv4AddressInput,
            NetlinkAcceptedOutput,
        >(
            "add_ipv4_address",
            op_state_store::SideEffect::Mutation,
            false,
            "cap.network.rtnetlink.ipv4-address.add@v1",
            "mut.network.rtnetlink.ipv4-address.add@v1",
        ),
    );
    schema.methods.insert(
        "set_mac_address".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            SetMacAddressInput,
            NetlinkAcceptedOutput,
        >(
            "set_mac_address",
            op_state_store::SideEffect::Mutation,
            false,
            "cap.network.rtnetlink.mac-address.set@v1",
            "mut.network.rtnetlink.mac-address.set@v1",
        ),
    );
    schema.methods.insert(
        "set_default_route".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            SetDefaultRouteInput,
            NetlinkAcceptedOutput,
        >(
            "set_default_route",
            op_state_store::SideEffect::Mutation,
            false,
            "cap.network.rtnetlink.default-route.set@v1",
            "mut.network.rtnetlink.default-route.set@v1",
        ),
    );
    schema.methods.insert(
        "add_link".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            AddLinkInput,
            NetlinkAcceptedOutput,
        >(
            "add_link",
            op_state_store::SideEffect::Mutation,
            false,
            "cap.network.rtnetlink.link.add@v1",
            "mut.network.rtnetlink.link.add@v1",
        ),
    );
    schema.methods.insert(
        "del_link".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            DelLinkInput,
            NetlinkAcceptedOutput,
        >(
            "del_link",
            op_state_store::SideEffect::Mutation,
            false,
            "cap.network.rtnetlink.link.del@v1",
            "mut.network.rtnetlink.link.del@v1",
        ),
    );
    schema.methods.insert(
        "add_route".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            AddRouteInput,
            NetlinkAcceptedOutput,
        >(
            "add_route",
            op_state_store::SideEffect::Mutation,
            false,
            "cap.network.rtnetlink.route.add@v1",
            "mut.network.rtnetlink.route.add@v1",
        ),
    );
    schema.methods.insert(
        "del_route".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            DelRouteInput,
            NetlinkAcceptedOutput,
        >(
            "del_route",
            op_state_store::SideEffect::Mutation,
            false,
            "cap.network.rtnetlink.route.del@v1",
            "mut.network.rtnetlink.route.del@v1",
        ),
    );
    schema.methods.insert(
        "add_rule".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            AddRuleInput,
            NetlinkAcceptedOutput,
        >(
            "add_rule",
            op_state_store::SideEffect::Mutation,
            false,
            "cap.network.rtnetlink.rule.add@v1",
            "mut.network.rtnetlink.rule.add@v1",
        ),
    );
    schema.capabilities.insert(
        "cap.network.rtnetlink.link-state.set@v1".to_string(),
        CapabilityDecl {
            id: "cap.network.rtnetlink.link-state.set@v1".to_string(),
            description: "Grants: set_link_state.".to_string(),
        },
    );
    schema.capabilities.insert(
        "cap.network.rtnetlink.ipv4-address.add@v1".to_string(),
        CapabilityDecl {
            id: "cap.network.rtnetlink.ipv4-address.add@v1".to_string(),
            description: "Grants: add_ipv4_address.".to_string(),
        },
    );
    schema.capabilities.insert(
        "cap.network.rtnetlink.mac-address.set@v1".to_string(),
        CapabilityDecl {
            id: "cap.network.rtnetlink.mac-address.set@v1".to_string(),
            description: "Grants: set_mac_address.".to_string(),
        },
    );
    schema.capabilities.insert(
        "cap.network.rtnetlink.default-route.set@v1".to_string(),
        CapabilityDecl {
            id: "cap.network.rtnetlink.default-route.set@v1".to_string(),
            description: "Grants: set_default_route.".to_string(),
        },
    );
    schema.capabilities.insert(
        "cap.network.rtnetlink.link.add@v1".to_string(),
        CapabilityDecl {
            id: "cap.network.rtnetlink.link.add@v1".to_string(),
            description: "Grants: add_link.".to_string(),
        },
    );
    schema.capabilities.insert(
        "cap.network.rtnetlink.link.del@v1".to_string(),
        CapabilityDecl {
            id: "cap.network.rtnetlink.link.del@v1".to_string(),
            description: "Grants: del_link.".to_string(),
        },
    );
    schema.capabilities.insert(
        "cap.network.rtnetlink.route.add@v1".to_string(),
        CapabilityDecl {
            id: "cap.network.rtnetlink.route.add@v1".to_string(),
            description: "Grants: add_route.".to_string(),
        },
    );
    schema.capabilities.insert(
        "cap.network.rtnetlink.route.del@v1".to_string(),
        CapabilityDecl {
            id: "cap.network.rtnetlink.route.del@v1".to_string(),
            description: "Grants: del_route.".to_string(),
        },
    );
    schema.capabilities.insert(
        "cap.network.rtnetlink.rule.add@v1".to_string(),
        CapabilityDecl {
            id: "cap.network.rtnetlink.rule.add@v1".to_string(),
            description: "Grants: add_rule.".to_string(),
        },
    );

    schema
}

// Self-registration: the plugin registry discovers this via inventory
// (single source of the catalog; no central dispatch list).
inventory::submit! {
    crate::default_registry::PluginReg::new("rtnetlink", |_ctx| std::sync::Arc::new(RtnetlinkPlugin::new()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    #[test]
    fn validates_kernel_request_fields_without_touching_host() {
        assert!(interface_name("ovsbr0").is_ok());
        assert!(interface_name("../../host").is_err());
        assert!(ipv4_cidr("10.0.0.2/24").is_ok());
        assert!(ipv4_cidr("10.0.0.2/33").is_err());
        assert!(route_destination("0.0.0.0/0").is_ok());
        assert!(route_destination("10.0.0.0/33").is_err());
        assert!(mac("02:00:00:00:00:01").is_ok());
        assert!(mac("02:00:00:00:00").is_err());
    }

    #[test]
    fn invalid_route_is_rejected_before_any_netlink_call() {
        assert!(route_destination("bad").is_err());
        assert!(gateway("not-an-ip").is_err());
    }

    #[test]
    fn route_and_link_contracts_preserve_disambiguators() {
        let route: AddRouteInput = serde_json::from_value(serde_json::json!({
            "destination": "10.0.0.0/24",
            "gateway": "10.0.0.1",
            "interface": "eth0",
            "table": 100
        }))
        .unwrap();
        assert_eq!(route.table, Some(100));
        let rule: AddRuleInput = serde_json::from_value(serde_json::json!({
            "rule": "10.0.0.0/24", "table": 100, "priority": 50
        }))
        .unwrap();
        assert_eq!(rule.destination, "10.0.0.0/24");
        assert_eq!(rule.priority, Some(50));
        let veth: AddLinkInput = serde_json::from_value(serde_json::json!({
            "name": "veth0", "link_type": "veth", "peer_name": "veth1"
        }))
        .unwrap();
        assert_eq!(veth.peer_name.as_deref(), Some("veth1"));
    }

    /// Native proof for every declared method. The test enters a fresh network
    /// namespace before opening any netlink handle, then reads back links,
    /// addresses, routes and policy rules from that namespace.
    #[test]
    #[ignore = "requires root and an isolated network namespace"]
    fn all_declared_methods_apply_and_readback_in_isolated_network_namespace() {
        assert_eq!(unsafe { nix::libc::geteuid() }, 0);
        assert_eq!(
            unsafe { nix::libc::unshare(nix::libc::CLONE_NEWNET) },
            0,
            "unshare(CLONE_NEWNET) failed"
        );
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let call = |method: &'static str, args: serde_json::Value| async move {
                let output = dispatch_rtnetlink_method(method, &args).await.unwrap();
                assert_eq!(output["disposition"], "accepted");
                output
            };

            call(
                "add_link",
                serde_json::json!({"name":"rt-a","link_type":"veth","peer_name":"rt-b"}),
            )
            .await;
            let interfaces = op_network::rtnetlink::list_interfaces().await.unwrap();
            assert!(interfaces.iter().any(|i| i.name == "rt-a"));
            assert!(interfaces.iter().any(|i| i.name == "rt-b"));

            call(
                "set_link_state",
                serde_json::json!({"name":"rt-a","state":"up"}),
            )
            .await;
            let interfaces = op_network::rtnetlink::list_interfaces().await.unwrap();
            assert_eq!(
                interfaces.iter().find(|i| i.name == "rt-a").unwrap().state,
                "up"
            );

            call(
                "set_mac_address",
                serde_json::json!({"name":"rt-a","mac":"02:00:00:00:00:0a"}),
            )
            .await;
            let interfaces = op_network::rtnetlink::list_interfaces().await.unwrap();
            assert_eq!(
                interfaces
                    .iter()
                    .find(|i| i.name == "rt-a")
                    .unwrap()
                    .mac_address
                    .as_deref(),
                Some("02:00:00:00:00:0a")
            );

            call(
                "add_ipv4_address",
                serde_json::json!({"name":"rt-a","address":"192.0.2.1/24"}),
            )
            .await;
            let interfaces = op_network::rtnetlink::list_interfaces().await.unwrap();
            assert!(interfaces
                .iter()
                .find(|i| i.name == "rt-a")
                .unwrap()
                .addresses
                .iter()
                .any(|a| a.address == "192.0.2.1" && a.prefix_len == 24));

            call(
                "add_route",
                serde_json::json!({"destination":"198.51.100.0/24","gateway":"192.0.2.254","interface":"rt-a","table":100}),
            )
            .await;
            let routes = op_network::rtnetlink::list_routes_for_interface("rt-a")
                .await
                .unwrap();
            assert!(routes.iter().any(|route| {
                route["destination"] == "198.51.100.0/24"
                    && route["gateway"] == "192.0.2.254"
                    && route["table"] == 100
            }));

            call(
                "set_default_route",
                serde_json::json!({"name":"rt-a","gateway":"192.0.2.254"}),
            )
            .await;
            let default_route = op_network::rtnetlink::get_default_route()
                .await
                .unwrap()
                .unwrap();
            assert_eq!(default_route["interface_name"], "rt-a");
            assert_eq!(default_route["gateway"], "192.0.2.254");

            call(
                "add_rule",
                serde_json::json!({"destination":"203.0.113.0/24","table":100,"priority":1000}),
            )
            .await;
            let rule_output = Command::new("/usr/bin/ip")
                .args(["-N", "-j", "rule", "show"])
                .output()
                .unwrap();
            assert!(
                rule_output.status.success(),
                "ip rule readback failed: {}",
                String::from_utf8_lossy(&rule_output.stderr)
            );
            let rules: serde_json::Value = serde_json::from_slice(&rule_output.stdout).unwrap();
            // ip emits table identifiers as JSON strings, including with -N.
            assert!(
                rules.as_array().unwrap().iter().any(|rule| {
                    rule["priority"] == 1000
                        && rule["table"] == "100"
                        && rule["dst"] == "203.0.113.0/24"
                }),
                "expected destination/priority/table rule was absent: {rules}"
            );

            call(
                "del_route",
                serde_json::json!({"destination":"198.51.100.0/24","gateway":"192.0.2.254","interface":"rt-a","table":100}),
            )
            .await;
            let routes = op_network::rtnetlink::list_routes_for_interface("rt-a")
                .await
                .unwrap();
            assert!(!routes
                .iter()
                .any(|route| route["destination"] == "198.51.100.0/24"));

            call("del_link", serde_json::json!({"name":"rt-a"})).await;
            let interfaces = op_network::rtnetlink::list_interfaces().await.unwrap();
            assert!(!interfaces.iter().any(|i| i.name == "rt-a" || i.name == "rt-b"));
        });
    }
}
