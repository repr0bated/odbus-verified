//! Host gRPC-bridge endpoint + Xray schema-tag routing.
//!
//! Topology (current — xray + gRPC-bridge run on the host):
//!
//! - The `wg-xray` Incus container is DEPRECATED and STOPPED. Do not
//!   reference it.
//! - Xray runs on the HOST via the `gbr-xray` s6 service
//!   (canonical live config: `/dev/shm/xray_config.json`).
//! - The operation.v1 gRPC server (StateSync, etc., port 50051) runs on the
//!   HOST served by `op-dbus` at `10.200.0.2:50051` (the `grpc-uplink` veth
//!   IP). The old `10.200.0.1:50051` lived inside the wg-xray container and
//!   is dead.
//! - Xray applies OpenFlow + PluginSchema tags to route traffic to WireGuard
//!   peers / wgcf egress.
//!
//! Outbound RPC calls from this crate target the on-host operation.v1 endpoint
//! directly and carry the explicitly selected session's genesis and trace id.

/// Default on-host operation.v1 gRPC endpoint served by `op-dbus` at the
/// `grpc-uplink` veth IP `10.200.0.2:50051`.
///
/// (Renamed from `DEFAULT_WG_XRAY_ENDPOINT`; the wg-xray container is
/// deprecated. The alias is preserved for downstream callers.)
pub const DEFAULT_GRPC_ENDPOINT: &str = "http://10.200.0.2:50051";
/// Backwards-compatible alias for callers that still reference the old
/// `DEFAULT_WG_XRAY_ENDPOINT` name.
pub const DEFAULT_WG_XRAY_ENDPOINT: &str = DEFAULT_GRPC_ENDPOINT;
/// Xray SOCKS/MCP control plane (host-side proxy device).
pub const DEFAULT_XRAY_MCP_ENDPOINT: &str = "tcp://127.0.0.1:1081";

pub const ENV_RPC_ENDPOINT: &str = "OP_ASSISTANT_RPC_ENDPOINT";
pub const ENV_XRAY_MCP: &str = "OP_ASSISTANT_XRAY_MCP";
pub const HEADER_GENESIS: &str = "x-ghostbridge-genesis";
pub const HEADER_FOOTPRINT: &str = "x-ghostbridge-footprint";
pub const HEADER_TRACE_ID: &str = "x-ghostbridge-trace-id";

/// Session identity tags injected into outbound RPCs.
#[derive(Debug, Clone, Default)]
pub struct SchemaTags {
    pub footprint_hex: String,
    pub trace_id: String,
}

impl SchemaTags {
    /// Load the configured session from the authoritative identity projection.
    pub fn load() -> Self {
        let Ok(identity) = op_identity::configured_identity_session() else {
            return Self::default();
        };
        let Some(genesis) = identity.genesis.filter(|value| !value.is_empty()) else {
            return Self::default();
        };
        Self {
            footprint_hex: genesis,
            trace_id: identity.trace_id,
        }
    }

    pub fn is_valid(&self) -> bool {
        !self.footprint_hex.is_empty() && self.footprint_hex.chars().any(|c| c != '0')
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_default_invalid() {
        let t = SchemaTags::default();
        assert!(!t.is_valid());
    }
}
