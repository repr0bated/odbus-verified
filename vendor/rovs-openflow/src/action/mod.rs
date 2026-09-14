//! OpenFlow actions.
//!
//! This module provides types and functions for working with OpenFlow actions,
//! including standard actions and Nicira vendor extensions.

mod nicira;
pub mod nxm;
pub mod types;

#[cfg(test)]
mod tests;

use std::net::{Ipv4Addr, Ipv6Addr};

use std::fmt;

use crate::match_fields::MacAddr;
use crate::oxm::{OxmClass, OxmField};

// Re-export public types
#[allow(unused_imports)]
pub use nicira::{learn_flags, LearnSpec, NxLearn};
pub use types::{ct_flags, nat_flags, nat_range, port, NICIRA_VENDOR_ID};

// Use ActionType internally (NxActionSubtype is used by nicira module)
use types::ActionType;

// Import nicira encoding/decoding functions
use nicira::{
    decode_nicira_action, encode_nx_ct, encode_nx_ct_nat, encode_nx_learn, encode_nx_move,
    encode_nx_reg_load_nxm, encode_nx_resubmit, encode_set_tunnel_id,
};

/// CT commit flag (shorthand).
pub const CT_COMMIT: u16 = ct_flags::COMMIT;

/// NAT configuration for use with ct() action.
///
/// # Examples
///
/// ```ignore
/// // SNAT to a single IP
/// NatConfig::snat(Ipv4Addr::new(10, 0, 0, 1))
///
/// // SNAT with IP range
/// NatConfig::snat_range(
///     Ipv4Addr::new(10, 0, 0, 1),
///     Ipv4Addr::new(10, 0, 0, 10),
/// )
///
/// // DNAT with port
/// NatConfig::dnat(Ipv4Addr::new(192, 168, 1, 1)).port(8080)
///
/// // SNAT with port range and random selection
/// NatConfig::snat(Ipv4Addr::new(10, 0, 0, 1))
///     .port_range(5000, 6000)
///     .random()
/// ```
#[derive(Debug, Clone)]
pub struct NatConfig {
    /// NAT flags (SRC/DST, persistent, hash, random)
    pub flags: u16,
    /// Minimum IPv4 address (if IPv4 NAT)
    pub ipv4_min: Option<Ipv4Addr>,
    /// Maximum IPv4 address (if range)
    pub ipv4_max: Option<Ipv4Addr>,
    /// Minimum IPv6 address (if IPv6 NAT)
    pub ipv6_min: Option<Ipv6Addr>,
    /// Maximum IPv6 address (if range)
    pub ipv6_max: Option<Ipv6Addr>,
    /// Minimum port (if port NAT)
    pub port_min: Option<u16>,
    /// Maximum port (if port range)
    pub port_max: Option<u16>,
}

impl NatConfig {
    /// Create a SNAT configuration with a single IPv4 address.
    pub fn snat(addr: Ipv4Addr) -> Self {
        Self {
            flags: nat_flags::SRC,
            ipv4_min: Some(addr),
            ipv4_max: None,
            ipv6_min: None,
            ipv6_max: None,
            port_min: None,
            port_max: None,
        }
    }

    /// Create a SNAT configuration with an IPv4 address range.
    pub fn snat_range(min: Ipv4Addr, max: Ipv4Addr) -> Self {
        Self {
            flags: nat_flags::SRC,
            ipv4_min: Some(min),
            ipv4_max: Some(max),
            ipv6_min: None,
            ipv6_max: None,
            port_min: None,
            port_max: None,
        }
    }

    /// Create a DNAT configuration with a single IPv4 address.
    pub fn dnat(addr: Ipv4Addr) -> Self {
        Self {
            flags: nat_flags::DST,
            ipv4_min: Some(addr),
            ipv4_max: None,
            ipv6_min: None,
            ipv6_max: None,
            port_min: None,
            port_max: None,
        }
    }

    /// Create a DNAT configuration with an IPv4 address range.
    pub fn dnat_range(min: Ipv4Addr, max: Ipv4Addr) -> Self {
        Self {
            flags: nat_flags::DST,
            ipv4_min: Some(min),
            ipv4_max: Some(max),
            ipv6_min: None,
            ipv6_max: None,
            port_min: None,
            port_max: None,
        }
    }

    /// Create a SNAT configuration with a single IPv6 address.
    pub fn snat_v6(addr: Ipv6Addr) -> Self {
        Self {
            flags: nat_flags::SRC,
            ipv4_min: None,
            ipv4_max: None,
            ipv6_min: Some(addr),
            ipv6_max: None,
            port_min: None,
            port_max: None,
        }
    }

    /// Create a DNAT configuration with a single IPv6 address.
    pub fn dnat_v6(addr: Ipv6Addr) -> Self {
        Self {
            flags: nat_flags::DST,
            ipv4_min: None,
            ipv4_max: None,
            ipv6_min: Some(addr),
            ipv6_max: None,
            port_min: None,
            port_max: None,
        }
    }

    /// Create a SNAT configuration with an IPv6 address range.
    pub fn snat_v6_range(min: Ipv6Addr, max: Ipv6Addr) -> Self {
        Self {
            flags: nat_flags::SRC,
            ipv4_min: None,
            ipv4_max: None,
            ipv6_min: Some(min),
            ipv6_max: Some(max),
            port_min: None,
            port_max: None,
        }
    }

    /// Create a DNAT configuration with an IPv6 address range.
    pub fn dnat_v6_range(min: Ipv6Addr, max: Ipv6Addr) -> Self {
        Self {
            flags: nat_flags::DST,
            ipv4_min: None,
            ipv4_max: None,
            ipv6_min: Some(min),
            ipv6_max: Some(max),
            port_min: None,
            port_max: None,
        }
    }

    /// Set a single port for NAT.
    pub fn port(mut self, port: u16) -> Self {
        self.port_min = Some(port);
        self.port_max = None;
        self
    }

    /// Set a port range for NAT.
    pub fn port_range(mut self, min: u16, max: u16) -> Self {
        self.port_min = Some(min);
        self.port_max = Some(max);
        self
    }

    /// Use persistent NAT mapping (survives restarts).
    pub fn persistent(mut self) -> Self {
        self.flags |= nat_flags::PERSISTENT;
        self
    }

    /// Use hash-based port selection.
    pub fn hash(mut self) -> Self {
        self.flags |= nat_flags::PROTO_HASH;
        self.flags &= !nat_flags::PROTO_RANDOM;
        self
    }

    /// Use random port selection.
    pub fn random(mut self) -> Self {
        self.flags |= nat_flags::PROTO_RANDOM;
        self.flags &= !nat_flags::PROTO_HASH;
        self
    }

    /// Calculate the range_present flags for encoding.
    pub(crate) fn range_present(&self) -> u16 {
        let mut flags = 0u16;
        if self.ipv4_min.is_some() {
            flags |= nat_range::IPV4_MIN;
        }
        if self.ipv4_max.is_some() {
            flags |= nat_range::IPV4_MAX;
        }
        if self.ipv6_min.is_some() {
            flags |= nat_range::IPV6_MIN;
        }
        if self.ipv6_max.is_some() {
            flags |= nat_range::IPV6_MAX;
        }
        if self.port_min.is_some() {
            flags |= nat_range::PROTO_MIN;
        }
        if self.port_max.is_some() {
            flags |= nat_range::PROTO_MAX;
        }
        flags
    }
}

/// An OpenFlow action.
#[derive(Debug, Clone)]
pub enum Action {
    /// Output to a port
    Output(OutputPort),
    /// Encapsulate the packet (OF 1.5, OFPAT_ENCAP = 29).
    ///
    /// `new_header_len` is the outer header length including the 2-byte
    /// pad that precedes it: 16 for Ethernet (2 pad + 14 MAC header).
    Encap { new_header_len: u16 },
    /// Drop the packet (implicit, no action)
    Drop,
    /// Send to controller
    Controller { max_len: u16 },
    /// Set source MAC
    SetEthSrc(MacAddr),
    /// Set destination MAC
    SetEthDst(MacAddr),
    /// Set VLAN ID
    SetVlanVid(u16),
    /// Push VLAN tag
    PushVlan(u16),
    /// Pop VLAN tag
    PopVlan,
    /// Set IPv4 source
    SetIpv4Src(Ipv4Addr),
    /// Set IPv4 destination
    SetIpv4Dst(Ipv4Addr),
    /// Set TCP/UDP source port
    SetTpSrc(u16),
    /// Set TCP/UDP destination port
    SetTpDst(u16),
    /// Set IP TTL
    SetTtl(u8),
    /// Decrement IP TTL
    DecTtl,
    /// Go to table (OF 1.1+)
    GotoTable(u8),
    /// Write metadata
    WriteMetadata { metadata: u64, mask: u64 },
    /// Apply meter
    Meter(u32),
    /// Output to group
    Group(u32),
    /// Set tunnel ID
    SetTunnelId(u64),
    /// Resubmit to table (Nicira extension)
    NxResubmit {
        port: Option<u16>,
        table: Option<u8>,
    },
    /// Learn action (Nicira extension)
    NxLearn(NxLearn),
    /// Conntrack action (Nicira extension)
    NxCt {
        flags: u16,
        zone: u16,
        table: Option<u8>,
    },
    /// Conntrack with NAT action (Nicira extension)
    NxCtNat {
        flags: u16,
        zone: u16,
        table: Option<u8>,
        nat: NatConfig,
    },
    /// Move/copy bits between fields (Nicira extension)
    NxMove {
        /// Source NXM field header
        src_field: u32,
        /// Destination NXM field header
        dst_field: u32,
        /// Number of bits to copy
        n_bits: u16,
        /// Bit offset in source field
        src_ofs: u16,
        /// Bit offset in destination field
        dst_ofs: u16,
    },
    /// Load immediate value into field (Nicira extension)
    NxRegLoad {
        /// Destination NXM field header
        dst_field: u32,
        /// Bit offset in destination field
        dst_ofs: u16,
        /// Number of bits to load
        n_bits: u16,
        /// Value to load
        value: u64,
    },
}

/// Output port specification.
#[derive(Debug, Clone, Copy)]
pub enum OutputPort {
    /// Physical or logical port number
    Port(u32),
    /// Send to controller
    Controller,
    /// Flood (all ports except input)
    Flood,
    /// All ports except input
    All,
    /// Input port
    InPort,
    /// Local (management) port
    Local,
    /// Normal L2/L3 processing
    Normal,
    /// No output (drop)
    None,
}

impl From<u32> for OutputPort {
    fn from(port: u32) -> Self {
        Self::Port(port)
    }
}

/// A list of actions.
#[derive(Debug, Clone, Default)]
pub struct ActionList {
    actions: Vec<Action>,
}

impl ActionList {
    /// Create a new empty action list.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add an action to the list.
    pub fn push(&mut self, action: Action) {
        self.actions.push(action);
    }

    /// Encapsulate the packet (OF 1.5 OFPAT_ENCAP = 29).
    ///
    /// `new_header_len` includes the 2-byte pad preceding the outer header:
    /// 16 for Ethernet (2 pad + 14 MAC header). Outer header fields are
    /// populated by subsequent SetField actions on the same list.
    pub fn encap(mut self, new_header_len: u16) -> Self {
        self.actions.push(Action::Encap { new_header_len });
        self
    }

    /// Output to a port.
    pub fn output(mut self, port: impl Into<OutputPort>) -> Self {
        self.actions.push(Action::Output(port.into()));
        self
    }

    /// Send to controller.
    pub fn controller(mut self, max_len: u16) -> Self {
        self.actions.push(Action::Controller { max_len });
        self
    }

    /// Drop the packet.
    pub fn drop(mut self) -> Self {
        self.actions.push(Action::Drop);
        self
    }

    /// Set destination MAC.
    pub fn set_eth_dst(mut self, mac: MacAddr) -> Self {
        self.actions.push(Action::SetEthDst(mac));
        self
    }

    /// Set source MAC.
    pub fn set_eth_src(mut self, mac: MacAddr) -> Self {
        self.actions.push(Action::SetEthSrc(mac));
        self
    }

    /// Push VLAN tag.
    pub fn push_vlan(mut self, tpid: u16) -> Self {
        self.actions.push(Action::PushVlan(tpid));
        self
    }

    /// Pop VLAN tag.
    pub fn pop_vlan(mut self) -> Self {
        self.actions.push(Action::PopVlan);
        self
    }

    /// Set VLAN ID.
    pub fn set_vlan_vid(mut self, vid: u16) -> Self {
        self.actions.push(Action::SetVlanVid(vid));
        self
    }

    /// Go to another table.
    pub fn goto_table(mut self, table: u8) -> Self {
        self.actions.push(Action::GotoTable(table));
        self
    }

    /// Decrement TTL.
    pub fn dec_ttl(mut self) -> Self {
        self.actions.push(Action::DecTtl);
        self
    }

    /// Output to all ports except input port (flood).
    pub fn flood(mut self) -> Self {
        self.actions.push(Action::Output(OutputPort::Flood));
        self
    }

    /// Output to all ports.
    pub fn all(mut self) -> Self {
        self.actions.push(Action::Output(OutputPort::All));
        self
    }

    /// Output using normal L2/L3 switching.
    pub fn normal(mut self) -> Self {
        self.actions.push(Action::Output(OutputPort::Normal));
        self
    }

    /// Output to ingress port.
    pub fn in_port(mut self) -> Self {
        self.actions.push(Action::Output(OutputPort::InPort));
        self
    }

    /// Set tunnel ID (Nicira extension).
    pub fn set_tunnel_id(mut self, tunnel_id: u64) -> Self {
        self.actions.push(Action::SetTunnelId(tunnel_id));
        self
    }

    /// Output to group table.
    pub fn group(mut self, group_id: u32) -> Self {
        self.actions.push(Action::Group(group_id));
        self
    }

    /// Resubmit to another table (Nicira extension).
    ///
    /// - `port`: Input port to use (None = current input port)
    /// - `table`: Table to resubmit to (None = current table)
    pub fn resubmit(mut self, port: Option<u16>, table: Option<u8>) -> Self {
        self.actions.push(Action::NxResubmit { port, table });
        self
    }

    /// Resubmit to a specific table (Nicira extension).
    ///
    /// Convenience method for `resubmit(None, Some(table))`.
    pub fn resubmit_table(mut self, table: u8) -> Self {
        self.actions.push(Action::NxResubmit { port: None, table: Some(table) });
        self
    }

    /// Connection tracking action (Nicira extension).
    ///
    /// - `flags`: CT flags (commit, force, etc.)
    /// - `zone`: CT zone ID
    /// - `table`: Table to recirculate to after CT (None = no recirc)
    pub fn ct(mut self, flags: u16, zone: u16, table: Option<u8>) -> Self {
        self.actions.push(Action::NxCt { flags, zone, table });
        self
    }

    /// Connection tracking with commit (Nicira extension).
    ///
    /// Commits the connection to the connection tracking table.
    pub fn ct_commit(mut self, zone: u16) -> Self {
        self.actions.push(Action::NxCt { flags: CT_COMMIT, zone, table: None });
        self
    }

    /// Connection tracking with NAT (Nicira extension).
    ///
    /// Performs connection tracking with Network Address Translation.
    ///
    /// # Arguments
    ///
    /// - `flags`: CT flags (commit, force, etc.)
    /// - `zone`: CT zone ID
    /// - `table`: Table to recirculate to after CT (None = no recirc)
    /// - `nat`: NAT configuration (SNAT or DNAT)
    ///
    /// # Example
    ///
    /// ```ignore
    /// use rovs_openflow::{ActionList, NatConfig, CT_COMMIT};
    /// use std::net::Ipv4Addr;
    ///
    /// // SNAT to 10.0.0.1
    /// ActionList::new().ct_nat(
    ///     CT_COMMIT,
    ///     1,
    ///     Some(2),
    ///     NatConfig::snat(Ipv4Addr::new(10, 0, 0, 1)),
    /// )
    /// ```
    pub fn ct_nat(mut self, flags: u16, zone: u16, table: Option<u8>, nat: NatConfig) -> Self {
        self.actions.push(Action::NxCtNat { flags, zone, table, nat });
        self
    }

    /// Connection tracking with SNAT (Nicira extension).
    ///
    /// Source NAT: translates the source IP address.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use std::net::Ipv4Addr;
    ///
    /// // SNAT to 10.0.0.1, commit and recirculate to table 2
    /// ActionList::new().ct_snat(
    ///     1,                              // zone
    ///     Some(2),                        // recirc table
    ///     Ipv4Addr::new(10, 0, 0, 1),     // NAT address
    /// )
    /// ```
    pub fn ct_snat(mut self, zone: u16, table: Option<u8>, addr: Ipv4Addr) -> Self {
        self.actions.push(Action::NxCtNat {
            flags: CT_COMMIT,
            zone,
            table,
            nat: NatConfig::snat(addr),
        });
        self
    }

    /// Connection tracking with DNAT (Nicira extension).
    ///
    /// Destination NAT: translates the destination IP address.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use std::net::Ipv4Addr;
    ///
    /// // DNAT to 192.168.1.100:8080
    /// ActionList::new().ct_dnat(
    ///     1,
    ///     Some(2),
    ///     Ipv4Addr::new(192, 168, 1, 100),
    /// )
    /// ```
    pub fn ct_dnat(mut self, zone: u16, table: Option<u8>, addr: Ipv4Addr) -> Self {
        self.actions.push(Action::NxCtNat {
            flags: CT_COMMIT,
            zone,
            table,
            nat: NatConfig::dnat(addr),
        });
        self
    }

    /// Learn action (Nicira extension).
    ///
    /// Creates flows dynamically based on packet content.
    pub fn learn(mut self, learn: NxLearn) -> Self {
        self.actions.push(Action::NxLearn(learn));
        self
    }

    /// Move/copy bits between fields (Nicira extension).
    ///
    /// Copies `n_bits` bits from `src_field[src_ofs..]` to `dst_field[dst_ofs..]`.
    /// Use NXM constants from the `nxm` module for field headers.
    ///
    /// # Example
    /// ```ignore
    /// // Copy ARP source IP to ARP target IP
    /// actions.move_field(nxm::ARP_SPA, nxm::ARP_TPA, 32, 0, 0)
    /// ```
    pub fn move_field(
        mut self,
        src_field: u32,
        dst_field: u32,
        n_bits: u16,
        src_ofs: u16,
        dst_ofs: u16,
    ) -> Self {
        self.actions.push(Action::NxMove {
            src_field,
            dst_field,
            n_bits,
            src_ofs,
            dst_ofs,
        });
        self
    }

    /// Load immediate value into field (Nicira extension).
    ///
    /// Loads `value` into `dst_field[dst_ofs..dst_ofs+n_bits]`.
    /// Use NXM constants from the `nxm` module for field headers.
    ///
    /// # Example
    /// ```ignore
    /// // Set ARP opcode to 2 (reply)
    /// actions.load_field(nxm::ARP_OP, 0, 16, 2)
    /// ```
    pub fn load_field(mut self, dst_field: u32, dst_ofs: u16, n_bits: u16, value: u64) -> Self {
        self.actions.push(Action::NxRegLoad {
            dst_field,
            dst_ofs,
            n_bits,
            value,
        });
        self
    }

    /// Set ARP opcode (Nicira extension).
    ///
    /// Common values: 1 = request, 2 = reply
    pub fn set_arp_op(self, opcode: u16) -> Self {
        self.load_field(nxm::ARP_OP, 0, 16, opcode as u64)
    }

    /// Set ARP source protocol address (sender IP).
    pub fn set_arp_spa(self, ip: u32) -> Self {
        self.load_field(nxm::ARP_SPA, 0, 32, ip as u64)
    }

    /// Set ARP target protocol address (target IP).
    pub fn set_arp_tpa(self, ip: u32) -> Self {
        self.load_field(nxm::ARP_TPA, 0, 32, ip as u64)
    }

    /// Set ARP source hardware address (sender MAC).
    ///
    /// Note: MAC is passed as a u64 with the MAC in the lower 48 bits.
    pub fn set_arp_sha(self, mac: u64) -> Self {
        self.load_field(nxm::ARP_SHA, 0, 48, mac)
    }

    /// Set ARP target hardware address (target MAC).
    ///
    /// Note: MAC is passed as a u64 with the MAC in the lower 48 bits.
    pub fn set_arp_tha(self, mac: u64) -> Self {
        self.load_field(nxm::ARP_THA, 0, 48, mac)
    }

    /// Get the actions.
    pub fn actions(&self) -> &[Action] {
        &self.actions
    }

    /// Check if the list is empty.
    pub fn is_empty(&self) -> bool {
        self.actions.is_empty()
    }

    /// Get the number of actions.
    pub fn len(&self) -> usize {
        self.actions.len()
    }

    /// Encode all actions to wire format.
    ///
    /// Actions are concatenated and padded to 8-byte alignment.
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        for action in &self.actions {
            buf.extend(action.encode());
        }        // Pad to 8-byte boundary
        let padding = (8 - (buf.len() % 8)) % 8;
        buf.extend(std::iter::repeat_n(0u8, padding));
        buf
    }

    /// Decode all actions from wire format.
    ///
    /// Reads actions until the data is exhausted.
    pub fn decode(data: &[u8]) -> crate::Result<Self> {
        let mut actions = Vec::new();
        let mut offset = 0;

        while offset < data.len() {
            // Need at least 4 bytes for action header
            if data.len() - offset < 4 {
                break;
            }

            // Check for zero-length padding at end
            let length = u16::from_be_bytes([data[offset + 2], data[offset + 3]]) as usize;
            if length == 0 {
                break;
            }

            let (action, consumed) = Action::decode(&data[offset..])?;
            // Skip Drop actions (used as placeholder for unsupported types)
            if !matches!(action, Action::Drop) {
                actions.push(action);
            }
            offset += consumed;
        }

        Ok(Self { actions })
    }
}

impl OutputPort {
    /// Convert to wire format port number.
    pub const fn to_wire_port(self) -> u32 {
        match self {
            Self::Port(p) => p,
            Self::Controller => port::CONTROLLER,
            Self::Flood => port::FLOOD,
            Self::All => port::ALL,
            Self::InPort => port::IN_PORT,
            Self::Local => port::LOCAL,
            Self::Normal => port::NORMAL,
            Self::None => port::NONE,
        }
    }

    /// Create from wire format port number.
    pub const fn from_wire(port_num: u32) -> Self {
        match port_num {
            port::CONTROLLER => Self::Controller,
            port::FLOOD => Self::Flood,
            port::ALL => Self::All,
            port::IN_PORT => Self::InPort,
            port::LOCAL => Self::Local,
            port::NORMAL => Self::Normal,
            port::NONE => Self::None,
            p => Self::Port(p),
        }
    }
}

/// Format a MAC address as `aa:bb:cc:dd:ee:ff`.
fn format_mac(mac: MacAddr) -> String {
    format!(
        "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
        mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]
    )
}

impl fmt::Display for OutputPort {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Port(p) => write!(f, "{p}"),
            Self::Controller => write!(f, "CONTROLLER"),
            Self::Flood => write!(f, "FLOOD"),
            Self::All => write!(f, "ALL"),
            Self::InPort => write!(f, "IN_PORT"),
            Self::Local => write!(f, "LOCAL"),
            Self::Normal => write!(f, "NORMAL"),
            Self::None => write!(f, "NONE"),
        }
    }
}

impl fmt::Display for NatConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "nat(")?;
        if self.flags & nat_flags::SRC != 0 {
            write!(f, "src")?;
        } else if self.flags & nat_flags::DST != 0 {
            write!(f, "dst")?;
        }
        if let Some(ip) = self.ipv4_min {
            write!(f, "={ip}")?;
            if let Some(max) = self.ipv4_max {
                write!(f, "-{max}")?;
            }
        } else if let Some(ip) = self.ipv6_min {
            write!(f, "={ip}")?;
            if let Some(max) = self.ipv6_max {
                write!(f, "-{max}")?;
            }
        }
        if let Some(port) = self.port_min {
            write!(f, ":{port}")?;
            if let Some(max) = self.port_max {
                write!(f, "-{max}")?;
            }
        }
        write!(f, ")")
    }
}

impl fmt::Display for Action {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Output(OutputPort::Normal) => write!(f, "NORMAL"),
            Self::Output(OutputPort::Flood) => write!(f, "FLOOD"),
            Self::Output(OutputPort::All) => write!(f, "ALL"),
            Self::Output(OutputPort::InPort) => write!(f, "IN_PORT"),
            Self::Output(OutputPort::Local) => write!(f, "LOCAL"),
            Self::Output(port) => write!(f, "output:{port}"),
            Self::Encap { new_header_len } => write!(f, "encap:{new_header_len}"),
            Self::Drop => write!(f, "drop"),
            Self::Controller { max_len } => write!(f, "CONTROLLER:{max_len}"),
            Self::SetEthSrc(mac) => write!(f, "set_eth_src:{}", format_mac(*mac)),
            Self::SetEthDst(mac) => write!(f, "set_eth_dst:{}", format_mac(*mac)),
            Self::SetVlanVid(vid) => write!(f, "set_vlan_vid:{vid}"),
            Self::PushVlan(ethertype) => write!(f, "push_vlan:0x{ethertype:04x}"),
            Self::PopVlan => write!(f, "pop_vlan"),
            Self::SetIpv4Src(ip) => write!(f, "set_ipv4_src:{ip}"),
            Self::SetIpv4Dst(ip) => write!(f, "set_ipv4_dst:{ip}"),
            Self::SetTpSrc(port) => write!(f, "set_tp_src:{port}"),
            Self::SetTpDst(port) => write!(f, "set_tp_dst:{port}"),
            Self::SetTtl(ttl) => write!(f, "set_ttl:{ttl}"),
            Self::DecTtl => write!(f, "dec_ttl"),
            Self::GotoTable(table) => write!(f, "goto_table:{table}"),
            Self::WriteMetadata { metadata, mask } => {
                write!(f, "write_metadata:0x{metadata:x}/0x{mask:x}")
            }
            Self::Meter(id) => write!(f, "meter:{id}"),
            Self::Group(id) => write!(f, "group:{id}"),
            Self::SetTunnelId(id) => write!(f, "set_tunnel:{id:#x}"),
            Self::NxResubmit { port, table } => {
                let port_str = port.map_or(String::new(), |p| p.to_string());
                let table_str = table.map_or(String::new(), |t| t.to_string());
                write!(f, "resubmit({port_str},{table_str})")
            }
            Self::NxLearn(learn) => {
                write!(f, "learn(table={}", learn.table_id)?;
                if learn.idle_timeout > 0 {
                    write!(f, ",idle_timeout={}", learn.idle_timeout)?;
                }
                if learn.hard_timeout > 0 {
                    write!(f, ",hard_timeout={}", learn.hard_timeout)?;
                }
                if learn.priority > 0 {
                    write!(f, ",priority={}", learn.priority)?;
                }
                write!(f, ")")
            }
            Self::NxCt { flags, zone, table } => {
                write!(f, "ct(")?;
                let mut parts = Vec::new();
                if *flags & types::ct_flags::COMMIT != 0 {
                    parts.push("commit".to_string());
                }
                if *zone != 0 {
                    parts.push(format!("zone={zone}"));
                }
                if let Some(t) = table {
                    parts.push(format!("table={t}"));
                }
                write!(f, "{})", parts.join(","))
            }
            Self::NxCtNat { flags, zone, table, nat } => {
                write!(f, "ct(")?;
                let mut parts = Vec::new();
                if *flags & types::ct_flags::COMMIT != 0 {
                    parts.push("commit".to_string());
                }
                if *zone != 0 {
                    parts.push(format!("zone={zone}"));
                }
                if let Some(t) = table {
                    parts.push(format!("table={t}"));
                }
                parts.push(nat.to_string());
                write!(f, "{})", parts.join(","))
            }
            Self::NxMove { src_field, dst_field, n_bits, src_ofs, dst_ofs } => {
                write!(f, "move:NXM({src_field:#x})[{src_ofs}..{n_bits}]->NXM({dst_field:#x})[{dst_ofs}..{n_bits}]")
            }
            Self::NxRegLoad { dst_field, dst_ofs, n_bits, value } => {
                write!(f, "load:{value:#x}->NXM({dst_field:#x})[{dst_ofs}..{}]", dst_ofs + n_bits)
            }
        }
    }
}

impl fmt::Display for ActionList {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.actions.is_empty() {
            return write!(f, "drop");
        }
        for (i, action) in self.actions.iter().enumerate() {
            if i > 0 {
                write!(f, ",")?;
            }
            write!(f, "{action}")?;
        }
        Ok(())
    }
}

impl Action {
    /// Encode action to OpenFlow wire format.
    #[allow(clippy::match_same_arms)]
    pub fn encode(&self) -> Vec<u8> {
        match self {
            Self::Output(port) => encode_output(port.to_wire_port(), 0xffff),
            Self::Encap { new_header_len } => encode_encap(*new_header_len),
            Self::Drop => Vec::new(), // Drop is implicit (no actions)
            Self::Controller { max_len } => encode_output(port::CONTROLLER, *max_len),
            Self::SetEthSrc(mac) => encode_set_field_mac(OxmField::EthSrc, *mac),
            Self::SetEthDst(mac) => encode_set_field_mac(OxmField::EthDst, *mac),
            Self::SetVlanVid(vid) => encode_set_field_u16(OxmField::VlanVid, *vid | 0x1000),
            Self::PushVlan(ethertype) => encode_push_vlan(*ethertype),
            Self::PopVlan => encode_pop_vlan(),
            Self::SetIpv4Src(addr) => encode_set_field_u32(OxmField::Ipv4Src, (*addr).into()),
            Self::SetIpv4Dst(addr) => encode_set_field_u32(OxmField::Ipv4Dst, (*addr).into()),
            Self::SetTpSrc(port) => encode_set_field_u16(OxmField::TcpSrc, *port),
            Self::SetTpDst(port) => encode_set_field_u16(OxmField::TcpDst, *port),
            Self::SetTtl(ttl) => encode_set_nw_ttl(*ttl),
            Self::DecTtl => encode_dec_ttl(),
            Self::GotoTable(_) => Vec::new(), // GotoTable is an instruction, not action
            Self::WriteMetadata { .. } => Vec::new(), // WriteMetadata is an instruction
            Self::Meter(_) => Vec::new(), // Meter is an instruction
            Self::Group(group_id) => encode_group(*group_id),
            Self::SetTunnelId(tun_id) => encode_set_tunnel_id(*tun_id),
            Self::NxResubmit { port, table } => encode_nx_resubmit(*port, *table),
            Self::NxLearn(learn) => encode_nx_learn(learn),
            Self::NxCt { flags, zone, table } => encode_nx_ct(*flags, *zone, *table),
            Self::NxCtNat { flags, zone, table, nat } => {
                encode_nx_ct_nat(*flags, *zone, *table, nat)
            }
            Self::NxMove { src_field, dst_field, n_bits, src_ofs, dst_ofs } => {
                encode_nx_move(*src_field, *dst_field, *n_bits, *src_ofs, *dst_ofs)
            }
            Self::NxRegLoad { dst_field, dst_ofs, n_bits, value } => {
                encode_nx_reg_load_nxm(*dst_field, *dst_ofs, *n_bits, *value)
            }
        }
    }

    /// Decode action from OpenFlow wire format.
    ///
    /// Returns the decoded action and the number of bytes consumed.
    #[allow(clippy::too_many_lines)]
    pub fn decode(data: &[u8]) -> crate::Result<(Self, usize)> {
        if data.len() < 4 {
            return Err(crate::Error::Parse("action too short".into()));
        }

        let action_type = u16::from_be_bytes([data[0], data[1]]);
        let length = u16::from_be_bytes([data[2], data[3]]) as usize;

        if data.len() < length {
            return Err(crate::Error::Parse("action truncated".into()));
        }

        let action_type = ActionType::try_from(action_type)?;

        let action = match action_type {
            ActionType::Encap => {
                if length < 8 {
                    return Err(crate::Error::Parse("encap action too short".into()));
                }
                Self::Encap {
                    new_header_len: u16::from_be_bytes([data[4], data[5]]),
                }
            }
            ActionType::Output => {
                if length < 16 {
                    return Err(crate::Error::Parse("output action too short".into()));
                }
                let port_num = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
                let max_len = u16::from_be_bytes([data[8], data[9]]);
                let output_port = OutputPort::from_wire(port_num);
                if port_num == port::CONTROLLER {
                    Self::Controller { max_len }
                } else {
                    Self::Output(output_port)
                }
            }
            ActionType::PopVlan => Self::PopVlan,
            ActionType::PushVlan => {
                if length < 8 {
                    return Err(crate::Error::Parse("push_vlan action too short".into()));
                }
                let ethertype = u16::from_be_bytes([data[4], data[5]]);
                Self::PushVlan(ethertype)
            }
            ActionType::DecNwTtl => Self::DecTtl,
            ActionType::SetNwTtl => {
                if length < 8 {
                    return Err(crate::Error::Parse("set_nw_ttl action too short".into()));
                }
                Self::SetTtl(data[4])
            }
            ActionType::Group => {
                if length < 8 {
                    return Err(crate::Error::Parse("group action too short".into()));
                }
                let group_id = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
                Self::Group(group_id)
            }
            ActionType::SetField => {
                decode_set_field_action(&data[4..length])?
            }
            ActionType::Experimenter => {
                if length < 10 {
                    return Err(crate::Error::Parse("experimenter action too short".into()));
                }
                let vendor = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
                if vendor == NICIRA_VENDOR_ID {
                    decode_nicira_action(&data[8..length])?
                } else {
                    // Unknown vendor, skip
                    return Err(crate::Error::Parse(format!(
                        "unknown experimenter vendor: {vendor:#x}"
                    )));
                }
            }
            // Actions we don't fully decode yet - return a placeholder
            ActionType::CopyTtlOut
            | ActionType::CopyTtlIn
            | ActionType::SetMplsTtl
            | ActionType::DecMplsTtl
            | ActionType::PushMpls
            | ActionType::PopMpls
            | ActionType::SetQueue
            | ActionType::PushPbb
            | ActionType::PopPbb => {
                // Skip unsupported actions by returning Drop as a placeholder
                Self::Drop
            }
        };

        Ok((action, length))
    }
}

// ============================================================================
// Standard Action Encoding Functions
// ============================================================================

/// Encode Output action (16 bytes).
///
/// ```text
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// |         type (0)            |          length (16)            |
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// |                            port                               |
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// |          max_len            |           pad (zeros)           |
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// |                           pad (zeros)                         |
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// ```
fn encode_output(port_num: u32, max_len: u16) -> Vec<u8> {
    let mut buf = Vec::with_capacity(16);
    buf.extend((ActionType::Output as u16).to_be_bytes());
    buf.extend(16u16.to_be_bytes()); // length
    buf.extend(port_num.to_be_bytes());
    buf.extend(max_len.to_be_bytes());
    buf.extend([0u8; 6]); // padding
    buf
}

/// Encode Encap action (OF 1.5, OFPAT_ENCAP = 29, 8 bytes).
///
/// `new_header_len` includes the 2-byte pad that precedes the new outer
/// header: 16 for Ethernet (2 pad + 14 MAC header). The outer header
/// contents themselves are supplied by subsequent SetField actions.
///
/// ```text
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// |           class = 0           |   type = 29   |   len = 8     |
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// |  new_header_len (incl. pad)   |           pad (zeros)         |
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// ```
fn encode_encap(new_header_len: u16) -> Vec<u8> {
    let mut buf = Vec::with_capacity(8);
    buf.extend((ActionType::Encap as u16).to_be_bytes());
    buf.extend(8u16.to_be_bytes()); // length
    buf.extend(new_header_len.to_be_bytes());
    buf.extend([0u8; 2]); // padding
    buf
}

/// Encode PopVlan action (8 bytes).
fn encode_pop_vlan() -> Vec<u8> {
    let mut buf = Vec::with_capacity(8);
    buf.extend((ActionType::PopVlan as u16).to_be_bytes());
    buf.extend(8u16.to_be_bytes()); // length
    buf.extend([0u8; 4]); // padding
    buf
}

/// Encode PushVlan action (8 bytes).
fn encode_push_vlan(ethertype: u16) -> Vec<u8> {
    let mut buf = Vec::with_capacity(8);
    buf.extend((ActionType::PushVlan as u16).to_be_bytes());
    buf.extend(8u16.to_be_bytes()); // length
    buf.extend(ethertype.to_be_bytes());
    buf.extend([0u8; 2]); // padding
    buf
}

/// Encode DecNwTtl action (8 bytes).
fn encode_dec_ttl() -> Vec<u8> {
    let mut buf = Vec::with_capacity(8);
    buf.extend((ActionType::DecNwTtl as u16).to_be_bytes());
    buf.extend(8u16.to_be_bytes()); // length
    buf.extend([0u8; 4]); // padding
    buf
}

/// Encode SetNwTtl action (8 bytes).
fn encode_set_nw_ttl(ttl: u8) -> Vec<u8> {
    let mut buf = Vec::with_capacity(8);
    buf.extend((ActionType::SetNwTtl as u16).to_be_bytes());
    buf.extend(8u16.to_be_bytes()); // length
    buf.push(ttl);
    buf.extend([0u8; 3]); // padding
    buf
}

/// Encode Group action (8 bytes).
fn encode_group(group_id: u32) -> Vec<u8> {
    let mut buf = Vec::with_capacity(8);
    buf.extend((ActionType::Group as u16).to_be_bytes());
    buf.extend(8u16.to_be_bytes()); // length
    buf.extend(group_id.to_be_bytes());
    buf
}

// ============================================================================
// SetField Actions
// ============================================================================

/// Encode SetField action for MAC address (16 bytes).
///
/// SetField uses OXM format: action header + OXM header + value + padding.
fn encode_set_field_mac(field: OxmField, mac: [u8; 6]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(16);
    buf.extend((ActionType::SetField as u16).to_be_bytes());
    buf.extend(16u16.to_be_bytes()); // length

    // OXM header for MAC field: class=0x8000, field, has_mask=false, length=6
    let oxm_header =
        ((OxmClass::OpenflowBasic as u32) << 16) | ((field as u32) << 9) | 6;
    buf.extend(oxm_header.to_be_bytes());
    buf.extend(mac);
    buf.extend([0u8; 2]); // padding to 16 bytes
    buf
}

/// Encode SetField action for u16 value (16 bytes).
fn encode_set_field_u16(field: OxmField, value: u16) -> Vec<u8> {
    let mut buf = Vec::with_capacity(16);
    buf.extend((ActionType::SetField as u16).to_be_bytes());
    buf.extend(16u16.to_be_bytes()); // length

    // OXM header for u16 field: class=0x8000, field, has_mask=false, length=2
    let oxm_header =
        ((OxmClass::OpenflowBasic as u32) << 16) | ((field as u32) << 9) | 2;
    buf.extend(oxm_header.to_be_bytes());
    buf.extend(value.to_be_bytes());
    buf.extend([0u8; 6]); // padding to 16 bytes
    buf
}

/// Encode SetField action for u32 value (16 bytes).
fn encode_set_field_u32(field: OxmField, value: u32) -> Vec<u8> {
    let mut buf = Vec::with_capacity(16);
    buf.extend((ActionType::SetField as u16).to_be_bytes());
    buf.extend(16u16.to_be_bytes()); // length

    // OXM header for u32 field: class=0x8000, field, has_mask=false, length=4
    let oxm_header =
        ((OxmClass::OpenflowBasic as u32) << 16) | ((field as u32) << 9) | 4;
    buf.extend(oxm_header.to_be_bytes());
    buf.extend(value.to_be_bytes());
    buf.extend([0u8; 4]); // padding to 16 bytes
    buf
}

// ============================================================================
// Action Decoding Functions
// ============================================================================

/// Decode SetField action.
///
/// SetField uses OXM format: OXM header (4 bytes) + value.
fn decode_set_field_action(data: &[u8]) -> crate::Result<Action> {
    if data.len() < 4 {
        return Err(crate::Error::Parse("set_field action too short".into()));
    }

    let oxm_header = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
    let oxm_class = (oxm_header >> 16) as u16;
    let field = ((oxm_header >> 9) & 0x7f) as u8;
    let length = (oxm_header & 0xff) as usize;

    if data.len() < 4 + length {
        return Err(crate::Error::Parse("set_field value truncated".into()));
    }

    let value = &data[4..4 + length];

    // OpenFlow Basic class
    if oxm_class == OxmClass::OpenflowBasic as u16 {
        match field {
            f if f == OxmField::EthSrc as u8 && length >= 6 => {
                let mut mac = [0u8; 6];
                mac.copy_from_slice(&value[..6]);
                Ok(Action::SetEthSrc(mac))
            }
            f if f == OxmField::EthDst as u8 && length >= 6 => {
                let mut mac = [0u8; 6];
                mac.copy_from_slice(&value[..6]);
                Ok(Action::SetEthDst(mac))
            }
            f if f == OxmField::VlanVid as u8 && length >= 2 => {
                let vid = u16::from_be_bytes([value[0], value[1]]);
                // Remove CFI bit
                Ok(Action::SetVlanVid(vid & 0x0fff))
            }
            f if f == OxmField::Ipv4Src as u8 && length >= 4 => {
                let addr = Ipv4Addr::new(value[0], value[1], value[2], value[3]);
                Ok(Action::SetIpv4Src(addr))
            }
            f if f == OxmField::Ipv4Dst as u8 && length >= 4 => {
                let addr = Ipv4Addr::new(value[0], value[1], value[2], value[3]);
                Ok(Action::SetIpv4Dst(addr))
            }
            f if f == OxmField::TcpSrc as u8 && length >= 2 => {
                let port = u16::from_be_bytes([value[0], value[1]]);
                Ok(Action::SetTpSrc(port))
            }
            f if f == OxmField::TcpDst as u8 && length >= 2 => {
                let port = u16::from_be_bytes([value[0], value[1]]);
                Ok(Action::SetTpDst(port))
            }
            f if f == OxmField::UdpSrc as u8 && length >= 2 => {
                let port = u16::from_be_bytes([value[0], value[1]]);
                Ok(Action::SetTpSrc(port))
            }
            f if f == OxmField::UdpDst as u8 && length >= 2 => {
                let port = u16::from_be_bytes([value[0], value[1]]);
                Ok(Action::SetTpDst(port))
            }
            _ => {
                // Unknown field, return Drop as placeholder
                Ok(Action::Drop)
            }
        }
    } else if oxm_class == OxmClass::Nxm1 as u16 {
        // NXM1 class (Nicira extensions)
        // Field 16 is tunnel ID
        if field == 16 && length >= 8 {
            let tun_id = u64::from_be_bytes([
                value[0], value[1], value[2], value[3],
                value[4], value[5], value[6], value[7],
            ]);
            Ok(Action::SetTunnelId(tun_id))
        } else {
            Ok(Action::Drop)
        }
    } else {
        // Unknown class
        Ok(Action::Drop)
    }
}
