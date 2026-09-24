//! Typed model of Incus instance **devices**, faithful to the official Incus
//! API (`shared/api`: `map[string]map[string]string`).
//!
//! Incus stores every device value as a string (`"nat": "true"`, `"uid": "0"`).
//! To present a typed, schema-renderable model while round-tripping losslessly
//! with the REST API, numeric/boolean keys use the [`StrBool`]/[`StrInt`]
//! coercion newtypes: they deserialize from either a JSON string or a native
//! value and **always serialize back to a string**, so a `Device` round-trips
//! to the all-string map Incus expects.
//!
//! [`Device`] is an internally-tagged enum (`#[serde(tag = "type")]`) with one
//! variant per device type. Through [`super::schemars_adapter`] it renders into
//! the plugin schema as a `FieldType::OneOf` discriminated union (the `oneOf`
//! emitted by schemars for the tagged enum). The device *name* (the Incus map
//! key) is carried alongside in [`NamedDevice`] so the `devices` field renders
//! as `Array<OneOf<…>>` rather than an opaque map.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use simd_json::prelude::*;
use std::collections::BTreeMap;

/// A boolean Incus value stored on the wire as the string `"true"`/`"false"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StrBool(pub bool);

impl Serialize for StrBool {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(if self.0 { "true" } else { "false" })
    }
}

impl<'de> Deserialize<'de> for StrBool {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl serde::de::Visitor<'_> for V {
            type Value = StrBool;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a boolean or a boolean-like string")
            }
            fn visit_bool<E: serde::de::Error>(self, v: bool) -> Result<StrBool, E> {
                Ok(StrBool(v))
            }
            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<StrBool, E> {
                match v.trim().to_ascii_lowercase().as_str() {
                    "true" | "1" | "yes" | "on" => Ok(StrBool(true)),
                    "false" | "0" | "no" | "off" | "" => Ok(StrBool(false)),
                    other => Err(E::custom(format!("invalid boolean string: {other}"))),
                }
            }
        }
        d.deserialize_any(V)
    }
}

/// An integer Incus value stored on the wire as a decimal string (e.g. `"0"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StrInt(pub i64);

impl Serialize for StrInt {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.0.to_string())
    }
}

impl<'de> Deserialize<'de> for StrInt {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl serde::de::Visitor<'_> for V {
            type Value = StrInt;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("an integer or an integer-like string")
            }
            fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<StrInt, E> {
                Ok(StrInt(v))
            }
            fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<StrInt, E> {
                Ok(StrInt(v as i64))
            }
            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<StrInt, E> {
                v.trim()
                    .parse::<i64>()
                    .map(StrInt)
                    .map_err(|_| E::custom(format!("invalid integer string: {v}")))
            }
        }
        d.deserialize_any(V)
    }
}

/// A device paired with its Incus device name (the map key).
///
/// Renders as an object `{ name, device: oneOf<…> }`; the `device` union is the
/// schema-visible discriminated type. The REST layer flattens a `Vec` of these
/// into Incus's `map[name] -> { type, … }` and back.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct NamedDevice {
    /// Incus device name (the map key, e.g. `"eth0"`, `"grpc-socket"`).
    pub name: String,
    /// The typed device definition.
    pub device: Device,
}

/// A single Incus device, tagged by its `type` key.
///
/// One variant per official Incus device type. `nic`, `gpu` and `infiniband`
/// carry their sub-type selector (`nictype`/`gputype`) as a field rather than a
/// nested union, matching the flat `map[string]string` the API uses.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Device {
    /// Network interface. `nictype` (or a managed `network`) selects the kind.
    Nic(NicDevice),
    /// Disk / filesystem / block volume mount.
    Disk(DiskDevice),
    /// Unix character device passthrough (containers only).
    UnixChar(UnixDevice),
    /// Unix block device passthrough (containers only).
    UnixBlock(UnixDevice),
    /// USB device passthrough.
    Usb(UsbDevice),
    /// GPU passthrough. `gputype` selects physical/mdev/mig/sriov.
    Gpu(GpuDevice),
    /// InfiniBand device. `nictype` selects physical/sriov.
    Infiniband(InfinibandDevice),
    /// Proxy device (port/socket forwarding between host and instance).
    Proxy(ProxyDevice),
    /// Hot-pluggable Unix device matched by vendor/product id.
    UnixHotplug(UnixHotplugDevice),
    /// Trusted Platform Module.
    Tpm(TpmDevice),
    /// Raw PCI passthrough (VMs only).
    Pci(PciDevice),
    /// Sentinel that removes an inherited device of the same name. No keys.
    None,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct NicDevice {
    /// Sub-type when not using a managed network: bridged, macvlan, sriov,
    /// physical, ipvlan, p2p, routed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nictype: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub network: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hwaddr: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<String>")]
    pub mtu: Option<StrInt>,
    #[serde(
        rename = "boot.priority",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    #[schemars(with = "Option<String>")]
    pub boot_priority: Option<StrInt>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<String>")]
    pub vlan: Option<StrInt>,
    #[serde(
        rename = "vlan.tagged",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub vlan_tagged: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(
        rename = "ipv4.address",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub ipv4_address: Option<String>,
    #[serde(
        rename = "ipv4.gateway",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub ipv4_gateway: Option<String>,
    #[serde(
        rename = "ipv4.routes",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub ipv4_routes: Option<String>,
    #[serde(
        rename = "ipv4.host_address",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub ipv4_host_address: Option<String>,
    #[serde(
        rename = "ipv4.host_tables",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub ipv4_host_tables: Option<String>,
    #[serde(
        rename = "ipv6.address",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub ipv6_address: Option<String>,
    #[serde(
        rename = "ipv6.gateway",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub ipv6_gateway: Option<String>,
    #[serde(
        rename = "ipv6.routes",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub ipv6_routes: Option<String>,
    #[serde(
        rename = "limits.egress",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub limits_egress: Option<String>,
    #[serde(
        rename = "limits.ingress",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub limits_ingress: Option<String>,
    #[serde(
        rename = "limits.max",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub limits_max: Option<String>,
    #[serde(
        rename = "limits.priority",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    #[schemars(with = "Option<String>")]
    pub limits_priority: Option<StrInt>,
    #[serde(
        rename = "queue.tx.length",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    #[schemars(with = "Option<String>")]
    pub queue_tx_length: Option<StrInt>,
    #[serde(rename = "io.bus", default, skip_serializing_if = "Option::is_none")]
    pub io_bus: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<String>")]
    pub gvrp: Option<StrBool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<String>")]
    pub connected: Option<StrBool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acceleration: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vrf: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pci: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vendorid: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub productid: Option<String>,
    #[serde(
        rename = "security.mac_filtering",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    #[schemars(with = "Option<String>")]
    pub security_mac_filtering: Option<StrBool>,
    #[serde(
        rename = "security.ipv4_filtering",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    #[schemars(with = "Option<String>")]
    pub security_ipv4_filtering: Option<StrBool>,
    #[serde(
        rename = "security.ipv6_filtering",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    #[schemars(with = "Option<String>")]
    pub security_ipv6_filtering: Option<StrBool>,
    #[serde(
        rename = "security.port_isolation",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    #[schemars(with = "Option<String>")]
    pub security_port_isolation: Option<StrBool>,
    #[serde(
        rename = "security.acls",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub security_acls: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct DiskDevice {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pool: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<String>")]
    pub readonly: Option<StrBool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<String>")]
    pub required: Option<StrBool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<String>")]
    pub recursive: Option<StrBool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<String>")]
    pub shift: Option<StrBool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<String>,
    #[serde(
        rename = "size.state",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub size_state: Option<String>,
    #[serde(
        rename = "boot.priority",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    #[schemars(with = "Option<String>")]
    pub boot_priority: Option<StrInt>,
    #[serde(
        rename = "limits.read",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub limits_read: Option<String>,
    #[serde(
        rename = "limits.write",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub limits_write: Option<String>,
    #[serde(
        rename = "limits.max",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub limits_max: Option<String>,
    #[serde(rename = "io.bus", default, skip_serializing_if = "Option::is_none")]
    pub io_bus: Option<String>,
    #[serde(rename = "io.cache", default, skip_serializing_if = "Option::is_none")]
    pub io_cache: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub propagation: Option<String>,
    #[serde(
        rename = "raw.mount.options",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub raw_mount_options: Option<String>,
    #[serde(
        rename = "ceph.cluster_name",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub ceph_cluster_name: Option<String>,
    #[serde(
        rename = "ceph.user_name",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub ceph_user_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wwn: Option<String>,
}

/// Shared by `unix-char` and `unix-block` (identical key set).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct UnixDevice {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<String>")]
    pub uid: Option<StrInt>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<String>")]
    pub gid: Option<StrInt>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<String>")]
    pub major: Option<StrInt>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<String>")]
    pub minor: Option<StrInt>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<String>")]
    pub required: Option<StrBool>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct UsbDevice {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vendorid: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub productid: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<String>")]
    pub busnum: Option<StrInt>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<String>")]
    pub devnum: Option<StrInt>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub serial: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<String>")]
    pub uid: Option<StrInt>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<String>")]
    pub gid: Option<StrInt>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<String>")]
    pub required: Option<StrBool>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct GpuDevice {
    /// Sub-type: physical (default), mdev, mig, sriov.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gputype: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pci: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vendorid: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub productid: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<String>")]
    pub uid: Option<StrInt>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<String>")]
    pub gid: Option<StrInt>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mdev: Option<String>,
    #[serde(rename = "mig.ci", default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<String>")]
    pub mig_ci: Option<StrInt>,
    #[serde(rename = "mig.gi", default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<String>")]
    pub mig_gi: Option<StrInt>,
    #[serde(rename = "mig.uuid", default, skip_serializing_if = "Option::is_none")]
    pub mig_uuid: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct InfinibandDevice {
    /// Sub-type: physical or sriov.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nictype: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hwaddr: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<String>")]
    pub mtu: Option<StrInt>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ProxyDevice {
    /// Required. Target the proxy connects to, e.g. `tcp:127.0.0.1:50051`.
    pub connect: String,
    /// Required. Address the proxy listens on, e.g. `unix:/run/assistant.sock`.
    pub listen: String,
    /// `host` (default) or `instance`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<String>")]
    pub nat: Option<StrBool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<String>")]
    pub proxy_protocol: Option<StrBool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<String>")]
    pub uid: Option<StrInt>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<String>")]
    pub gid: Option<StrInt>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(
        rename = "security.uid",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    #[schemars(with = "Option<String>")]
    pub security_uid: Option<StrInt>,
    #[serde(
        rename = "security.gid",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    #[schemars(with = "Option<String>")]
    pub security_gid: Option<StrInt>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct UnixHotplugDevice {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vendorid: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub productid: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pci: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<String>")]
    pub uid: Option<StrInt>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<String>")]
    pub gid: Option<StrInt>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<String>")]
    pub required: Option<StrBool>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct TpmDevice {
    /// Container TPM device path, e.g. `/dev/tpm0` (unused for VMs).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Container resource-manager path, e.g. `/dev/tpmrm0` (unused for VMs).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pathrm: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PciDevice {
    /// Required. PCI address of the device, e.g. `0000:00:1f.0`.
    pub address: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<String>")]
    pub firmware: Option<StrBool>,
}

impl Device {
    /// The Incus `type` string for this device (the discriminator value).
    pub fn type_str(&self) -> &'static str {
        match self {
            Device::Nic(_) => "nic",
            Device::Disk(_) => "disk",
            Device::UnixChar(_) => "unix-char",
            Device::UnixBlock(_) => "unix-block",
            Device::Usb(_) => "usb",
            Device::Gpu(_) => "gpu",
            Device::Infiniband(_) => "infiniband",
            Device::Proxy(_) => "proxy",
            Device::UnixHotplug(_) => "unix-hotplug",
            Device::Tpm(_) => "tpm",
            Device::Pci(_) => "pci",
            Device::None => "none",
        }
    }

    /// Convert to Incus's `map[string]string` form (every value a string),
    /// including the `type` key. This is exactly what the REST API expects.
    pub fn to_incus_map(&self) -> BTreeMap<String, String> {
        let value = simd_json::serde::to_owned_value(self).unwrap_or_else(|_| simd_json::json!({}));
        let mut map = BTreeMap::new();
        if let Some(obj) = value.as_object() {
            for (k, v) in obj.iter() {
                let s = match v {
                    simd_json::OwnedValue::String(s) => s.to_string(),
                    other => other.to_string(),
                };
                map.insert(k.to_string(), s);
            }
        }
        map
    }

    /// Build a `Device` from an Incus device map (all-string values). The map
    /// must contain a `type` key; coercion newtypes parse the string values.
    pub fn from_incus_map(map: &BTreeMap<String, String>) -> anyhow::Result<Self> {
        let value = simd_json::serde::to_owned_value(map)?;
        Ok(simd_json::serde::from_owned_value(value)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn proxy_round_trips_through_string_map() {
        let mut map = BTreeMap::new();
        map.insert("type".into(), "proxy".into());
        map.insert("connect".into(), "tcp:127.0.0.1:50051".into());
        map.insert("listen".into(), "unix:/run/assistant.sock".into());
        map.insert("nat".into(), "true".into());
        map.insert("uid".into(), "0".into());

        let dev = Device::from_incus_map(&map).expect("parse proxy");
        match &dev {
            Device::Proxy(p) => {
                assert_eq!(p.connect, "tcp:127.0.0.1:50051");
                assert_eq!(p.nat, Some(StrBool(true)));
                assert_eq!(p.uid, Some(StrInt(0)));
            }
            other => panic!("expected proxy, got {other:?}"),
        }

        // Serialized form keeps every value a string (incl. the bool/int).
        let back = dev.to_incus_map();
        assert_eq!(back.get("type").map(String::as_str), Some("proxy"));
        assert_eq!(back.get("nat").map(String::as_str), Some("true"));
        assert_eq!(back.get("uid").map(String::as_str), Some("0"));
    }

    #[test]
    fn nic_with_dotted_keys_round_trips() {
        let mut map = BTreeMap::new();
        map.insert("type".into(), "nic".into());
        map.insert("nictype".into(), "bridged".into());
        map.insert("parent".into(), "ovsbr0".into());
        map.insert("ipv4.address".into(), "10.0.0.5".into());
        map.insert("security.mac_filtering".into(), "true".into());

        let dev = Device::from_incus_map(&map).expect("parse nic");
        match &dev {
            Device::Nic(n) => {
                assert_eq!(n.nictype.as_deref(), Some("bridged"));
                assert_eq!(n.ipv4_address.as_deref(), Some("10.0.0.5"));
                assert_eq!(n.security_mac_filtering, Some(StrBool(true)));
            }
            other => panic!("expected nic, got {other:?}"),
        }
        let back = dev.to_incus_map();
        assert_eq!(
            back.get("ipv4.address").map(String::as_str),
            Some("10.0.0.5")
        );
        assert_eq!(
            back.get("security.mac_filtering").map(String::as_str),
            Some("true")
        );
    }

    #[test]
    fn none_device_is_keyless() {
        let mut map = BTreeMap::new();
        map.insert("type".into(), "none".into());
        let dev = Device::from_incus_map(&map).expect("parse none");
        assert_eq!(dev, Device::None);
        let back = dev.to_incus_map();
        assert_eq!(back.get("type").map(String::as_str), Some("none"));
        assert_eq!(back.len(), 1);
    }

    /// The schemars-derived `NamedDevice` schema must expose the device union as
    /// a `FieldType::OneOf` with one branch per device type.
    #[test]
    fn named_device_renders_device_as_oneof_union() {
        use op_state_store::FieldType;
        let root = serde_json::to_value(schemars::schema_for!(NamedDevice)).unwrap();
        let schema = crate::state_plugins::schemars_adapter::plugin_schema_from_json(
            "named_device",
            "1.0.0",
            "test",
            &root,
        );
        let device = &schema
            .fields
            .get("device")
            .expect("device field")
            .field_type;
        match device {
            FieldType::OneOf(branches) => {
                assert_eq!(branches.len(), 12, "one branch per device type");
                let mut tags = std::collections::BTreeSet::new();
                for branch in branches {
                    let FieldType::Object(fields) = branch else {
                        panic!("typed object branch");
                    };
                    let tag = &fields["type"];
                    assert!(tag.required, "native discriminant is required");
                    let FieldType::Enum(values) = &tag.field_type else {
                        panic!("string const must remain a single enum");
                    };
                    assert_eq!(values.len(), 1);
                    assert!(tags.insert(values[0].clone()));
                }
                assert!(tags.contains("proxy") && tags.contains("none"));
            }
            other => panic!("expected OneOf union, got {other:?}"),
        }
        for valid in [
            serde_json::json!({"name":"eth0","device":{"type":"nic","nictype":"bridged","parent":"ovsbr0"}}),
            serde_json::json!({"name":"gone","device":{"type":"none"}}),
        ] {
            assert!(
                schema
                    .validate(&simd_json::serde::to_owned_value(valid).unwrap())
                    .valid
            );
        }
        for invalid in [
            serde_json::json!({"name":"eth0","device":{"parent":"ovsbr0"}}),
            serde_json::json!({"name":"eth0","device":{"type":"invented-device"}}),
        ] {
            assert!(
                !schema
                    .validate(&simd_json::serde::to_owned_value(invalid).unwrap())
                    .valid
            );
        }
        let other =
            op_blob::adapter::try_plugin_schema_from_json("named_device", "1.0.0", "test", &root)
                .unwrap();
        assert_eq!(
            serde_json::to_value(&other.fields["device"].field_type).unwrap(),
            serde_json::to_value(device).unwrap()
        );
        let original = serde_json::to_value(schemars::schema_for!(NamedDevice)).unwrap();
        assert_eq!(
            root, original,
            "producer normalization cannot mutate authored schema"
        );
    }
}
