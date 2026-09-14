//! Reference plugins proving "the plugin is the schema pipeline" end-to-end.
//!
//! `unix_socket` mirrors the spec's canonical Tier-A pattern
//! (`op-plugins/src/state_plugins/unix_socket.rs`): the state struct and every
//! method input/output struct derive `schemars::JsonSchema`; the schema
//! function is `plugin_schema_from_json` + `apply_state_defaults` + typed
//! method inserts. `wireguard` is the identity-bearing first-pass plugin from
//! the whitepaper.

use crate::adapter::{
    apply_state_defaults, method_decl_from_schemars_with_output, plugin_schema_from_json, AckOutput,
};
use crate::schema::{PluginSchema, SideEffect};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// unix_socket — canonical Tier-A pattern
// ---------------------------------------------------------------------------

/// One declared socket endpoint.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SocketEndpoint {
    /// Filesystem path of the unix socket
    pub path: String,
    /// Abstract port tag for socket multiplexing
    #[schemars(range(min = 1, max = 65535))]
    pub port: u16,
}

/// The state struct IS the schema (spec Layer 0).
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.network.unix-socket.describe@v1"))]
pub struct UnixSocketState {
    /// Declared socket endpoints
    #[schemars(extend("x-oscal-subid" = "obs.network.unix-socket.endpoints@v1"))]
    pub sockets: Vec<SocketEndpoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct BindInput {
    /// Socket path to bind
    pub path: String,
    /// Port tag to register
    pub port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CloseInput {
    /// Socket path to close
    pub path: String,
}

/// Canonical schema function: derivation, defaults, then typed methods.
pub fn unix_socket_schema() -> PluginSchema {
    let root = serde_json::to_value(schemars::schema_for!(UnixSocketState))
        .expect("schemars serializes cleanly");
    let mut schema = plugin_schema_from_json(
        "unix_socket",
        "1.0.0",
        "Unix domain socket ownership and registration",
        &root,
    );
    let defaults = serde_json::to_value(UnixSocketState::default()).expect("default serializes");
    apply_state_defaults(&mut schema, &defaults);

    // Method-level subid with no struct field: the one sanctioned imperative
    // insert (REQ-3.4).
    schema.subids.insert(
        "bind".to_string(),
        "mut.network.unix-socket.bind@v1".to_string(),
    );

    schema.methods.insert(
        "bind".to_string(),
        method_decl_from_schemars_with_output::<BindInput, AckOutput>(
            "bind",
            SideEffect::Mutation,
            false,
            "unix_socket.write",
            "mut.network.unix-socket.bind@v1",
        ),
    );
    schema.methods.insert(
        "close".to_string(),
        method_decl_from_schemars_with_output::<CloseInput, AckOutput>(
            "close",
            SideEffect::Mutation,
            true,
            "unix_socket.write",
            "mut.network.unix-socket.close@v1",
        ),
    );
    schema
}

// ---------------------------------------------------------------------------
// wireguard — identity-bearing first-pass plugin (Task 1.1 shape)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct WireGuardPeer {
    /// Peer public key (base64) — the peer's identity
    pub public_key: String,
    /// Allowed IPs in CIDR form
    pub allowed_ips: Vec<String>,
    /// Optional endpoint host:port
    pub endpoint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct WireGuardInterface {
    /// Interface name
    pub name: String,
    /// Interface public key (base64) — the node identity
    #[schemars(extend("x-oscal-subid" = "obs.network.wireguard.identity@v1"))]
    pub public_key: String,
    /// UDP listen port
    #[schemars(range(min = 1, max = 65535))]
    pub listen_port: Option<u16>,
    /// Configured peers
    pub peers: Vec<WireGuardPeer>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.network.plugin.wireguard.schema@v1"))]
pub struct WireGuardState {
    /// WireGuard interfaces
    #[schemars(extend("x-oscal-subid" = "obs.network.wireguard.interfaces@v1"))]
    pub interfaces: Vec<WireGuardInterface>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SetDeviceInput {
    /// Interface to configure
    pub interface: String,
    /// Private key (base64); the derived public key becomes the identity
    pub private_key: String,
    /// UDP listen port
    pub listen_port: Option<u16>,
    /// Firewall mark
    pub fwmark: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AddPeerInput {
    /// Interface to add the peer to
    pub interface: String,
    /// Peer public key (base64)
    pub public_key: String,
    /// Allowed IPs in CIDR form
    pub allowed_ips: Vec<String>,
}

pub fn wireguard_schema() -> PluginSchema {
    let root = serde_json::to_value(schemars::schema_for!(WireGuardState))
        .expect("schemars serializes cleanly");
    let mut schema =
        plugin_schema_from_json("wireguard", "1.0.0", "WireGuard interface state", &root);
    let defaults = serde_json::to_value(WireGuardState::default()).expect("default serializes");
    apply_state_defaults(&mut schema, &defaults);

    schema.methods.insert(
        "set_device".to_string(),
        method_decl_from_schemars_with_output::<SetDeviceInput, AckOutput>(
            "set_device",
            SideEffect::Mutation,
            false,
            "wireguard.write",
            "mut.network.wireguard.set-device@v1",
        ),
    );
    schema.methods.insert(
        "add_peer".to_string(),
        method_decl_from_schemars_with_output::<AddPeerInput, AckOutput>(
            "add_peer",
            SideEffect::Mutation,
            false,
            "wireguard.write",
            "mut.network.wireguard.peer-add@v1",
        ),
    );
    schema
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::FieldType;
    use crate::subid::validate_subid;

    /// REQ-2.5 drift-guard shape for the demo plugins.
    #[test]
    fn derived_schema_matches_declared_contract() {
        let schema = unix_socket_schema();
        let sockets = schema.fields.get("sockets").expect("sockets field");
        assert!(matches!(sockets.field_type, FieldType::Array(_)));
        assert_eq!(
            schema.subids.get("__schema__").map(String::as_str),
            Some("sch.network.unix-socket.describe@v1")
        );
        assert_eq!(
            schema.subids.get("sockets").map(String::as_str),
            Some("obs.network.unix-socket.endpoints@v1")
        );
        // REQ-4.4: every method has a returns contract.
        for m in schema.methods.values() {
            assert!(m.returns.is_some(), "{} has no returns", m.name);
        }
    }

    /// Subid gate (REQ-4.5 / Task 4.3): all subids valid; method subids are
    /// mut/obs/exp; schema-level subid present when methods exist.
    #[test]
    fn all_demo_subids_are_valid_and_complete() {
        for schema in [unix_socket_schema(), wireguard_schema()] {
            for (key, subid) in &schema.subids {
                validate_subid(subid).unwrap_or_else(|e| panic!("{key}: {e}"));
            }
            for m in schema.methods.values() {
                validate_subid(&m.subid).unwrap_or_else(|e| panic!("{}: {e}", m.name));
                let cat = m.subid.split('.').next().unwrap();
                assert!(
                    matches!(cat, "mut" | "obs" | "exp"),
                    "method {} subid category {cat} must be mut/obs/exp",
                    m.name
                );
            }
            if !schema.methods.is_empty() {
                assert!(
                    schema.subids.contains_key("__schema__"),
                    "{}: schema-level subid missing",
                    schema.name
                );
            }
        }
    }

    #[test]
    fn wireguard_port_constraints_survive_derivation() {
        let schema = wireguard_schema();
        let ifaces = schema.fields.get("interfaces").unwrap();
        let FieldType::Array(inner) = &ifaces.field_type else {
            panic!("interfaces must be an array");
        };
        let FieldType::Object(props) = inner.as_ref() else {
            panic!("interface items must be objects");
        };
        assert_eq!(props.get("listen_port").unwrap().constraints.len(), 2);
        assert!(props.contains_key("peers"));
    }
}
