//! Translate a complete selected native IDL view into the plugin-owned state.
//! UUID relationships are authoritative; names never imply membership. Opaque
//! options and other_config are deliberately unobserved and omitted, not read
//! and later scrubbed. This is selected topology, not a full config backup.

use super::{BridgeConfig, InterfaceConfig, OvsBridgeState, PortConfig};
use anyhow::{Context, Result, ensure};
use op_network::rovs_proxy::monitor::{OvsdbRow, OvsdbSnapshot};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

pub fn project_topology(snapshot: &OvsdbSnapshot) -> Result<OvsBridgeState> {
    let roots = table(snapshot, "Open_vSwitch")?;
    let bridges = table(snapshot, "Bridge")?;
    let ports = table(snapshot, "Port")?;
    let interfaces = table(snapshot, "Interface")?;
    ensure!(
        roots.len() <= 1,
        "Open_vSwitch topology has multiple root rows"
    );
    ensure!(
        !roots.is_empty() || (bridges.is_empty() && ports.is_empty() && interfaces.is_empty()),
        "OVS topology rows exist without an Open_vSwitch root"
    );
    let mut result = Vec::new();
    let mut owned_ports = BTreeSet::new();
    let mut owned_interfaces = BTreeSet::new();
    for root in roots.values() {
        for id in references(column(root, "bridges")?)? {
            let row = bridges
                .get(&id)
                .context("Open_vSwitch references an absent Bridge row")?;
            let mut children = Vec::new();
            for id in references(column(row, "ports")?)? {
                ensure!(
                    owned_ports.insert(id),
                    "OVS Port belongs to multiple bridges"
                );
                let port = ports
                    .get(&id)
                    .context("Bridge references an absent Port row")?;
                let mut port_interfaces = Vec::new();
                for id in references(column(port, "interfaces")?)? {
                    ensure!(
                        owned_interfaces.insert(id),
                        "OVS Interface belongs to multiple ports"
                    );
                    let interface = interfaces
                        .get(&id)
                        .context("Port references an absent Interface row")?;
                    port_interfaces.push(InterfaceConfig {
                        name: name(interface)?,
                        iface_type: string(column(interface, "type")?)?,
                        mac_in_use: optional_string(column(interface, "mac_in_use")?)?,
                        mac: optional_string(column(interface, "mac")?)?,
                        admin_state: optional_string(column(interface, "admin_state")?)?,
                        link_state: optional_string(column(interface, "link_state")?)?,
                        options: Default::default(),
                    });
                }
                ensure!(
                    !port_interfaces.is_empty(),
                    "OVS Port has no Interface reference"
                );
                port_interfaces.sort_by(|left, right| left.name.cmp(&right.name));
                unique_names(
                    port_interfaces
                        .iter()
                        .map(|interface| interface.name.as_str()),
                )?;
                let mut trunks = members(column(port, "trunks")?)?
                    .into_iter()
                    .map(vlan)
                    .collect::<Result<Vec<_>>>()?;
                trunks.sort_unstable();
                ensure!(
                    trunks.windows(2).all(|pair| pair[0] != pair[1]),
                    "OVS trunks contain duplicate VLANs"
                );
                children.push(PortConfig {
                    name: name(port)?,
                    interfaces: port_interfaces,
                    tag: optional(column(port, "tag")?)?.map(vlan).transpose()?,
                    trunks,
                    vlan_mode: optional_string(column(port, "vlan_mode")?)?,
                    bond_mode: optional_string(column(port, "bond_mode")?)?,
                });
            }
            children.sort_by(|left, right| left.name.cmp(&right.name));
            unique_names(children.iter().map(|port| port.name.as_str()))?;
            result.push(BridgeConfig {
                name: name(row)?,
                ports: children,
                datapath_type: string(column(row, "datapath_type")?)?,
                fail_mode: optional_string(column(row, "fail_mode")?)?,
                stp_enable: column(row, "stp_enable")?
                    .as_bool()
                    .context("invalid OVS STP boolean")?,
                mcast_snooping_enable: column(row, "mcast_snooping_enable")?
                    .as_bool()
                    .context("invalid OVS multicast boolean")?,
                other_config: Default::default(),
            });
        }
    }
    result.sort_by(|left, right| left.name.cmp(&right.name));
    unique_names(result.iter().map(|bridge| bridge.name.as_str()))?;
    Ok(OvsBridgeState {
        bridges: result,
        monitors: Default::default(),
    })
}

fn table<'a>(snapshot: &'a OvsdbSnapshot, name: &str) -> Result<&'a BTreeMap<Uuid, OvsdbRow>> {
    snapshot
        .tables
        .get(name)
        .context("native topology snapshot omitted required table")
}
fn column<'a>(row: &'a OvsdbRow, name: &str) -> Result<&'a Value> {
    row.get(name)
        .context("native topology row omitted required column")
}
fn members(value: &Value) -> Result<Vec<&Value>> {
    if let Some(array) = value.as_array() {
        if array.first().and_then(Value::as_str) == Some("set") {
            ensure!(array.len() == 2, "invalid OVSDB set encoding");
            return Ok(array[1]
                .as_array()
                .context("invalid OVSDB set members")?
                .iter()
                .collect());
        }
    }
    // RFC 7047 permits a singleton set to be represented by its atom.
    Ok(vec![value])
}
fn references(value: &Value) -> Result<Vec<Uuid>> {
    let mut result = BTreeSet::new();
    for value in members(value)? {
        let atom = value
            .as_array()
            .context("OVSDB reference must be a UUID atom")?;
        ensure!(
            atom.len() == 2 && atom[0].as_str() == Some("uuid"),
            "invalid OVSDB UUID atom"
        );
        let id = atom[1]
            .as_str()
            .context("OVSDB UUID must be text")?
            .parse::<Uuid>()
            .context("invalid OVSDB UUID")?;
        ensure!(result.insert(id), "OVSDB set contains duplicate references");
    }
    Ok(result.into_iter().collect())
}
fn optional(value: &Value) -> Result<Option<&Value>> {
    let members = members(value)?;
    ensure!(
        members.len() <= 1,
        "OVSDB optional column has multiple values"
    );
    Ok(members.into_iter().next())
}
fn string(value: &Value) -> Result<String> {
    let value = value
        .as_str()
        .context("OVSDB string column has wrong type")?;
    ensure!(
        value.len() <= 4096 && !value.contains('\0'),
        "OVSDB string exceeds topology bounds"
    );
    Ok(value.to_owned())
}
fn optional_string(value: &Value) -> Result<Option<String>> {
    optional(value)?.map(string).transpose()
}
fn name(row: &OvsdbRow) -> Result<String> {
    let name = string(column(row, "name")?)?;
    ensure!(!name.is_empty(), "OVS topology row has an empty name");
    Ok(name)
}
fn vlan(value: &Value) -> Result<u16> {
    let value = value
        .as_u64()
        .context("OVS VLAN must be a nonnegative integer")?;
    ensure!(value <= 4095, "OVS VLAN exceeds 12-bit range");
    Ok(value as u16)
}
fn unique_names<'a>(names: impl Iterator<Item = &'a str>) -> Result<()> {
    let mut seen = BTreeSet::new();
    for name in names {
        ensure!(seen.insert(name), "OVS topology contains duplicate names");
    }
    Ok(())
}

#[cfg(test)]
#[path = "ovsdb_topology_tests.rs"]
mod tests;
