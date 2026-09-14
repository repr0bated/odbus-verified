//! NXM/OXM field header constants for use with Nicira extensions.
//!
//! Field header format: `(class << 16) | (field << 9) | length`
//!
//! Common classes:
//! - `0x0000` (NXM_OF_*): Legacy OpenFlow 1.0 compatible fields
//! - `0x0001` (NXM_NX_*): Nicira extension fields
//! - `0x8000` (OXM_OF_*): OpenFlow 1.3+ basic fields

// NXM_OF_* fields (class 0x0000) - Legacy OpenFlow fields

/// NXM_OF_IN_PORT: Ingress port (2 bytes)
pub const IN_PORT: u32 = 0x0000_0002;
/// NXM_OF_ETH_DST: Destination MAC address (6 bytes)
pub const ETH_DST: u32 = 0x0000_0206;
/// NXM_OF_ETH_SRC: Source MAC address (6 bytes)
pub const ETH_SRC: u32 = 0x0000_0406;
/// NXM_OF_ETH_TYPE: Ethertype (2 bytes)
pub const ETH_TYPE: u32 = 0x0000_0602;
/// NXM_OF_VLAN_TCI: VLAN tag control information (2 bytes)
pub const VLAN_TCI: u32 = 0x0000_0802;
/// NXM_OF_IP_PROTO: IP protocol (1 byte)
pub const IP_PROTO: u32 = 0x0000_0a01;
/// NXM_OF_IP_SRC: IPv4 source address (4 bytes)
pub const IP_SRC: u32 = 0x0000_0c04;
/// NXM_OF_IP_DST: IPv4 destination address (4 bytes)
pub const IP_DST: u32 = 0x0000_0e04;
/// NXM_OF_TCP_SRC: TCP source port (2 bytes)
pub const TCP_SRC: u32 = 0x0000_1002;
/// NXM_OF_TCP_DST: TCP destination port (2 bytes)
pub const TCP_DST: u32 = 0x0000_1202;
/// NXM_OF_UDP_SRC: UDP source port (2 bytes)
pub const UDP_SRC: u32 = 0x0000_1602;
/// NXM_OF_UDP_DST: UDP destination port (2 bytes)
pub const UDP_DST: u32 = 0x0000_1802;
/// NXM_OF_ARP_OP: ARP opcode (2 bytes)
pub const ARP_OP: u32 = 0x0000_1e02;
/// NXM_OF_ARP_SPA: ARP source IPv4 address (4 bytes)
pub const ARP_SPA: u32 = 0x0000_2004;
/// NXM_OF_ARP_TPA: ARP target IPv4 address (4 bytes)
pub const ARP_TPA: u32 = 0x0000_2204;

// NXM_NX_* fields (class 0x0001) - Nicira extensions

/// NXM_NX_ARP_SHA: ARP source hardware address (6 bytes) - field 17
pub const ARP_SHA: u32 = 0x0001_2206;
/// NXM_NX_ARP_THA: ARP target hardware address (6 bytes) - field 18
pub const ARP_THA: u32 = 0x0001_2406;
/// NXM_NX_REG0: General purpose register 0 (4 bytes)
pub const REG0: u32 = 0x0001_0004;
/// NXM_NX_REG1: General purpose register 1 (4 bytes)
pub const REG1: u32 = 0x0001_0204;
/// NXM_NX_REG2: General purpose register 2 (4 bytes)
pub const REG2: u32 = 0x0001_0404;
/// NXM_NX_TUN_ID: Tunnel ID (8 bytes)
pub const TUN_ID: u32 = 0x0001_2008;
/// NXM_NX_IPV6_SRC: IPv6 source address (16 bytes) — OXM class 0x8000, field 26
pub const IPV6_SRC: u32 = 0x8000_3410;
/// NXM_NX_IPV6_DST: IPv6 destination address (16 bytes) — OXM class 0x8000, field 27
pub const IPV6_DST: u32 = 0x8000_3610;

// OXM_OF_* fields (class 0x8000) - OpenFlow 1.3+

/// OXM_OF_IN_PORT: Ingress port (4 bytes)
pub const OXM_IN_PORT: u32 = 0x8000_0004;
/// OXM_OF_ETH_DST: Destination MAC address (6 bytes)
pub const OXM_ETH_DST: u32 = 0x8000_0606;
/// OXM_OF_ETH_SRC: Source MAC address (6 bytes)
pub const OXM_ETH_SRC: u32 = 0x8000_0806;
