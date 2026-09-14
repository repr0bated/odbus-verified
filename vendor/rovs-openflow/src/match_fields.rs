//! OpenFlow match field builder.

use std::fmt;
use std::net::{Ipv4Addr, Ipv6Addr};

use crate::oxm::{self, ct_state, OxmClass, OxmField};

/// MAC address type.
pub type MacAddr = [u8; 6];

/// OpenFlow match type (OF 1.2+).
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum MatchType {
    /// Standard match (deprecated in OF 1.2+)
    Standard = 0,
    /// OXM (OpenFlow Extensible Match)
    Oxm = 1,
}

/// Match fields for flow matching.
///
/// Uses a builder pattern for ergonomic construction.
#[derive(Debug, Clone, Default)]
pub struct Match {
    /// Packet type (OF1.5): `(namespace, type)` — `(1, 0x800)` marks plain
    /// IPv4 arriving on an L3-only port (e.g. a WireGuard interface).
    /// OXM class 0x8000, field 44, 4-byte payload of two big-endian u16s.
    pub packet_type: Option<(u16, u16)>,
    /// Input port
    pub in_port: Option<u32>,
    /// Input physical port
    pub in_phy_port: Option<u32>,
    /// Metadata
    pub metadata: Option<u64>,
    /// Metadata mask
    pub metadata_mask: Option<u64>,

    // L2
    /// Source MAC address
    pub eth_src: Option<MacAddr>,
    /// Source MAC mask
    pub eth_src_mask: Option<MacAddr>,
    /// Destination MAC address
    pub eth_dst: Option<MacAddr>,
    /// Destination MAC mask
    pub eth_dst_mask: Option<MacAddr>,
    /// Ethernet type
    pub eth_type: Option<u16>,
    /// VLAN ID
    pub vlan_vid: Option<u16>,
    /// VLAN PCP
    pub vlan_pcp: Option<u8>,

    // L3 IPv4
    /// IPv4 source address
    pub ipv4_src: Option<Ipv4Addr>,
    /// IPv4 source mask (prefix length)
    pub ipv4_src_mask: Option<u8>,
    /// IPv4 destination address
    pub ipv4_dst: Option<Ipv4Addr>,
    /// IPv4 destination mask (prefix length)
    pub ipv4_dst_mask: Option<u8>,
    /// IP protocol
    pub ip_proto: Option<u8>,
    /// IP DSCP
    pub ip_dscp: Option<u8>,
    /// IP ECN
    pub ip_ecn: Option<u8>,

    // L3 IPv6
    /// IPv6 source address
    pub ipv6_src: Option<Ipv6Addr>,
    /// IPv6 source mask (prefix length)
    pub ipv6_src_mask: Option<u8>,
    /// IPv6 destination address
    pub ipv6_dst: Option<Ipv6Addr>,
    /// IPv6 destination mask (prefix length)
    pub ipv6_dst_mask: Option<u8>,
    /// IPv6 flow label
    pub ipv6_flabel: Option<u32>,

    // L4 TCP
    /// TCP source port
    pub tcp_src: Option<u16>,
    /// TCP destination port
    pub tcp_dst: Option<u16>,
    /// TCP flags
    pub tcp_flags: Option<u16>,

    // L4 UDP
    /// UDP source port
    pub udp_src: Option<u16>,
    /// UDP destination port
    pub udp_dst: Option<u16>,

    // L4 ICMPv4
    /// ICMPv4 type
    pub icmp_type: Option<u8>,
    /// ICMPv4 code
    pub icmp_code: Option<u8>,

    // L4 ICMPv6
    /// ICMPv6 type
    pub icmpv6_type: Option<u8>,
    /// ICMPv6 code
    pub icmpv6_code: Option<u8>,

    // ARP
    /// ARP opcode
    pub arp_op: Option<u16>,
    /// ARP source IPv4
    pub arp_spa: Option<Ipv4Addr>,
    /// ARP target IPv4
    pub arp_tpa: Option<Ipv4Addr>,
    /// ARP source MAC
    pub arp_sha: Option<MacAddr>,
    /// ARP target MAC
    pub arp_tha: Option<MacAddr>,

    // Tunnel
    /// Tunnel ID
    pub tunnel_id: Option<u64>,

    // Connection tracking (Nicira extensions)
    /// Connection tracking state (use ct_state:: constants)
    pub ct_state: Option<u32>,
    /// Connection tracking state mask
    pub ct_state_mask: Option<u32>,
    /// Connection tracking zone
    pub ct_zone: Option<u16>,
    /// Connection tracking mark
    pub ct_mark: Option<u32>,
    /// Connection tracking mark mask
    pub ct_mark_mask: Option<u32>,
}

impl Match {
    /// Create a new empty match (matches all packets).
    pub fn new() -> Self {
        Self::default()
    }

    /// Match on input port.
    pub fn in_port(mut self, port: u32) -> Self {
        self.in_port = Some(port);
        self
    }

    /// Match on source MAC address.
    pub fn eth_src(mut self, addr: MacAddr) -> Self {
        self.eth_src = Some(addr);
        self
    }

    /// Match on destination MAC address.
    pub fn eth_dst(mut self, addr: MacAddr) -> Self {
        self.eth_dst = Some(addr);
        self
    }

    /// Match on Ethernet type.
    pub fn eth_type(mut self, etype: u16) -> Self {
        self.eth_type = Some(etype);
        self
    }

    /// Match on VLAN ID.
    pub fn vlan_vid(mut self, vid: u16) -> Self {
        self.vlan_vid = Some(vid);
        self
    }

    /// Match on IPv4 source address with prefix length.
    pub fn ipv4_src(mut self, addr: Ipv4Addr, prefix_len: u8) -> Self {
        self.eth_type = Some(0x0800); // IP
        self.ipv4_src = Some(addr);
        self.ipv4_src_mask = Some(prefix_len);
        self
    }

    /// Match on IPv4 destination address with prefix length.
    pub fn ipv4_dst(mut self, addr: Ipv4Addr, prefix_len: u8) -> Self {
        self.eth_type = Some(0x0800); // IP
        self.ipv4_dst = Some(addr);
        self.ipv4_dst_mask = Some(prefix_len);
        self
    }

    /// Match on IPv6 source address with prefix length.
    pub fn ipv6_src(mut self, addr: Ipv6Addr, prefix_len: u8) -> Self {
        self.eth_type = Some(0x86dd); // IPv6
        self.ipv6_src = Some(addr);
        self.ipv6_src_mask = Some(prefix_len);
        self
    }

    /// Match on IPv6 destination address with prefix length.
    pub fn ipv6_dst(mut self, addr: Ipv6Addr, prefix_len: u8) -> Self {
        self.eth_type = Some(0x86dd); // IPv6
        self.ipv6_dst = Some(addr);
        self.ipv6_dst_mask = Some(prefix_len);
        self
    }

    /// Match on IP protocol.
    pub fn ip_proto(mut self, proto: u8) -> Self {
        self.ip_proto = Some(proto);
        self
    }

    /// Match on TCP source port.
    pub fn tcp_src(mut self, port: u16) -> Self {
        self.ip_proto = Some(6); // TCP
        self.tcp_src = Some(port);
        self
    }

    /// Match on TCP destination port.
    pub fn tcp_dst(mut self, port: u16) -> Self {
        self.ip_proto = Some(6); // TCP
        self.tcp_dst = Some(port);
        self
    }

    /// Match on UDP source port.
    pub fn udp_src(mut self, port: u16) -> Self {
        self.ip_proto = Some(17); // UDP
        self.udp_src = Some(port);
        self
    }

    /// Match on UDP destination port.
    pub fn udp_dst(mut self, port: u16) -> Self {
        self.ip_proto = Some(17); // UDP
        self.udp_dst = Some(port);
        self
    }

    /// Match on tunnel ID.
    pub fn tunnel_id(mut self, id: u64) -> Self {
        self.tunnel_id = Some(id);
        self
    }

    /// Match on connection tracking state (Nicira extension).
    ///
    /// Use constants from `oxm::ct_state` module, e.g.:
    /// - `ct_state::TRK` - packet has been tracked
    /// - `ct_state::NEW` - new connection
    /// - `ct_state::EST` - established connection
    /// - `ct_state::REL` - related connection
    /// - `ct_state::INV` - invalid connection
    ///
    /// # Example
    ///
    /// ```ignore
    /// use rovs_openflow::oxm::ct_state;
    ///
    /// // Match established connections
    /// Match::new().ct_state(ct_state::TRK | ct_state::EST)
    /// ```
    pub fn ct_state(mut self, state: u32) -> Self {
        self.ct_state = Some(state);
        self.ct_state_mask = Some(state); // Default mask = match all set bits
        self
    }

    /// Match on connection tracking state with explicit mask.
    ///
    /// The mask specifies which bits of the state to match.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use rovs_openflow::oxm::ct_state;
    ///
    /// // Match tracked + new, ignoring other flags
    /// Match::new().ct_state_masked(
    ///     ct_state::TRK | ct_state::NEW,
    ///     ct_state::TRK | ct_state::NEW
    /// )
    /// ```
    pub fn ct_state_masked(mut self, state: u32, mask: u32) -> Self {
        self.ct_state = Some(state);
        self.ct_state_mask = Some(mask);
        self
    }

    /// Match on connection tracking zone (Nicira extension).
    ///
    /// Zones allow multiple independent connection tracking tables.
    pub fn ct_zone(mut self, zone: u16) -> Self {
        self.ct_zone = Some(zone);
        self
    }

    /// Match on connection tracking mark (Nicira extension).
    ///
    /// The ct_mark is a 32-bit value that can be set by the ct action
    /// and matched on later.
    pub fn ct_mark(mut self, mark: u32) -> Self {
        self.ct_mark = Some(mark);
        self
    }

    /// Match on connection tracking mark with mask (Nicira extension).
    #[allow(clippy::similar_names)]
    pub fn ct_mark_masked(mut self, mark: u32, mask: u32) -> Self {
        self.ct_mark = Some(mark);
        self.ct_mark_mask = Some(mask);
        self
    }

    /// Match on ARP opcode.
    ///
    /// Common values: 1 = request, 2 = reply
    pub fn arp_op(mut self, opcode: u16) -> Self {
        self.eth_type = Some(0x0806); // ARP
        self.arp_op = Some(opcode);
        self
    }

    /// Match on ARP source protocol address (sender IP).
    pub fn arp_spa(mut self, addr: impl Into<Ipv4Addr>) -> Self {
        self.eth_type = Some(0x0806); // ARP
        self.arp_spa = Some(addr.into());
        self
    }

    /// Match on ARP target protocol address (target IP).
    pub fn arp_tpa(mut self, addr: impl Into<Ipv4Addr>) -> Self {
        self.eth_type = Some(0x0806); // ARP
        self.arp_tpa = Some(addr.into());
        self
    }

    /// Match on ARP source hardware address (sender MAC).
    pub fn arp_sha(mut self, addr: MacAddr) -> Self {
        self.eth_type = Some(0x0806); // ARP
        self.arp_sha = Some(addr);
        self
    }

    /// Match on ARP target hardware address (target MAC).
    pub fn arp_tha(mut self, addr: MacAddr) -> Self {
        self.eth_type = Some(0x0806); // ARP
        self.arp_tha = Some(addr);
        self
    }

    /// Match on ICMPv4 type.
    ///
    /// Common values: 0 = echo reply, 8 = echo request
    pub fn icmp_type(mut self, icmp_type: u8) -> Self {
        self.eth_type = Some(0x0800); // IPv4
        self.ip_proto = Some(1); // ICMP
        self.icmp_type = Some(icmp_type);
        self
    }

    /// Match on ICMPv4 code.
    pub fn icmp_code(mut self, code: u8) -> Self {
        self.eth_type = Some(0x0800); // IPv4
        self.ip_proto = Some(1); // ICMP
        self.icmp_code = Some(code);
        self
    }

    /// Match on ICMPv6 type.
    ///
    /// Common values: 128 = echo request, 129 = echo reply,
    /// 133 = router solicitation, 134 = router advertisement,
    /// 135 = neighbor solicitation, 136 = neighbor advertisement
    pub fn icmpv6_type(mut self, icmp_type: u8) -> Self {
        self.eth_type = Some(0x86dd); // IPv6
        self.ip_proto = Some(58); // ICMPv6
        self.icmpv6_type = Some(icmp_type);
        self
    }

    /// Match on ICMPv6 code.
    pub fn icmpv6_code(mut self, code: u8) -> Self {
        self.eth_type = Some(0x86dd); // IPv6
        self.ip_proto = Some(58); // ICMPv6
        self.icmpv6_code = Some(code);
        self
    }

    /// Check if this match is empty (matches all).
    pub fn is_empty(&self) -> bool {
        self.in_port.is_none()
            && self.eth_src.is_none()
            && self.eth_dst.is_none()
            && self.eth_type.is_none()
            && self.vlan_vid.is_none()
            && self.ipv4_src.is_none()
            && self.ipv4_dst.is_none()
            && self.ip_proto.is_none()
            && self.tcp_src.is_none()
            && self.tcp_dst.is_none()
            && self.udp_src.is_none()
            && self.udp_dst.is_none()
            && self.icmp_type.is_none()
            && self.icmp_code.is_none()
            && self.icmpv6_type.is_none()
            && self.icmpv6_code.is_none()
            && self.tunnel_id.is_none()
            && self.arp_op.is_none()
            && self.arp_spa.is_none()
            && self.arp_tpa.is_none()
            && self.arp_sha.is_none()
            && self.arp_tha.is_none()
            && self.ct_state.is_none()
            && self.ct_zone.is_none()
            && self.ct_mark.is_none()
    }
}

/// Format a MAC address as `aa:bb:cc:dd:ee:ff`.
fn format_mac(mac: MacAddr) -> String {
    format!(
        "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
        mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]
    )
}

/// Format ct_state flags using OVS `+flag` notation.
fn format_ct_state(state: u32) -> String {
    let mut parts = Vec::new();
    if state & ct_state::TRK != 0 { parts.push("+trk"); }
    if state & ct_state::NEW != 0 { parts.push("+new"); }
    if state & ct_state::EST != 0 { parts.push("+est"); }
    if state & ct_state::REL != 0 { parts.push("+rel"); }
    if state & ct_state::RPL != 0 { parts.push("+rpl"); }
    if state & ct_state::INV != 0 { parts.push("+inv"); }
    if state & ct_state::SNAT != 0 { parts.push("+snat"); }
    if state & ct_state::DNAT != 0 { parts.push("+dnat"); }
    if parts.is_empty() {
        format!("0x{state:x}")
    } else {
        parts.join("")
    }
}

impl fmt::Display for Match {
    #[allow(clippy::too_many_lines)]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_empty() {
            return write!(f, "*");
        }

        let mut parts: Vec<String> = Vec::new();

        if let Some(p) = self.in_port {
            parts.push(format!("in_port={p}"));
        }
        if let Some(p) = self.in_phy_port {
            parts.push(format!("in_phy_port={p}"));
        }
        if let Some(m) = self.metadata {
            if let Some(mask) = self.metadata_mask {
                if mask == u64::MAX {
                    parts.push(format!("metadata=0x{m:x}"));
                } else {
                    parts.push(format!("metadata=0x{m:x}/0x{mask:x}"));
                }
            } else {
                parts.push(format!("metadata=0x{m:x}"));
            }
        }
        if let Some(mac) = self.eth_src {
            let s = format_mac(mac);
            if let Some(mask) = self.eth_src_mask {
                if mask == [0xff; 6] {
                    parts.push(format!("eth_src={s}"));
                } else {
                    parts.push(format!("eth_src={s}/{}", format_mac(mask)));
                }
            } else {
                parts.push(format!("eth_src={s}"));
            }
        }
        if let Some(mac) = self.eth_dst {
            let s = format_mac(mac);
            if let Some(mask) = self.eth_dst_mask {
                if mask == [0xff; 6] {
                    parts.push(format!("eth_dst={s}"));
                } else {
                    parts.push(format!("eth_dst={s}/{}", format_mac(mask)));
                }
            } else {
                parts.push(format!("eth_dst={s}"));
            }
        }
        if let Some(et) = self.eth_type {
            parts.push(format!("eth_type=0x{et:04x}"));
        }
        if let Some(vid) = self.vlan_vid {
            parts.push(format!("vlan_vid={vid}"));
        }
        if let Some(pcp) = self.vlan_pcp {
            parts.push(format!("vlan_pcp={pcp}"));
        }
        if let Some(proto) = self.ip_proto {
            parts.push(format!("ip_proto={proto}"));
        }
        if let Some(dscp) = self.ip_dscp {
            parts.push(format!("ip_dscp={dscp}"));
        }
        if let Some(ecn) = self.ip_ecn {
            parts.push(format!("ip_ecn={ecn}"));
        }
        if let Some(ip) = self.ipv4_src {
            let prefix = self.ipv4_src_mask.unwrap_or(32);
            if prefix < 32 {
                parts.push(format!("ipv4_src={ip}/{prefix}"));
            } else {
                parts.push(format!("ipv4_src={ip}"));
            }
        }
        if let Some(ip) = self.ipv4_dst {
            let prefix = self.ipv4_dst_mask.unwrap_or(32);
            if prefix < 32 {
                parts.push(format!("ipv4_dst={ip}/{prefix}"));
            } else {
                parts.push(format!("ipv4_dst={ip}"));
            }
        }
        if let Some(ip) = self.ipv6_src {
            let prefix = self.ipv6_src_mask.unwrap_or(128);
            if prefix < 128 {
                parts.push(format!("ipv6_src={ip}/{prefix}"));
            } else {
                parts.push(format!("ipv6_src={ip}"));
            }
        }
        if let Some(ip) = self.ipv6_dst {
            let prefix = self.ipv6_dst_mask.unwrap_or(128);
            if prefix < 128 {
                parts.push(format!("ipv6_dst={ip}/{prefix}"));
            } else {
                parts.push(format!("ipv6_dst={ip}"));
            }
        }
        if let Some(fl) = self.ipv6_flabel {
            parts.push(format!("ipv6_flabel=0x{fl:x}"));
        }
        if let Some(p) = self.tcp_src {
            parts.push(format!("tcp_src={p}"));
        }
        if let Some(p) = self.tcp_dst {
            parts.push(format!("tcp_dst={p}"));
        }
        if let Some(flags) = self.tcp_flags {
            parts.push(format!("tcp_flags=0x{flags:x}"));
        }
        if let Some(p) = self.udp_src {
            parts.push(format!("udp_src={p}"));
        }
        if let Some(p) = self.udp_dst {
            parts.push(format!("udp_dst={p}"));
        }
        if let Some(t) = self.icmp_type {
            parts.push(format!("icmp_type={t}"));
        }
        if let Some(c) = self.icmp_code {
            parts.push(format!("icmp_code={c}"));
        }
        if let Some(t) = self.icmpv6_type {
            parts.push(format!("icmpv6_type={t}"));
        }
        if let Some(c) = self.icmpv6_code {
            parts.push(format!("icmpv6_code={c}"));
        }
        if let Some(op) = self.arp_op {
            parts.push(format!("arp_op={op}"));
        }
        if let Some(ip) = self.arp_spa {
            parts.push(format!("arp_spa={ip}"));
        }
        if let Some(ip) = self.arp_tpa {
            parts.push(format!("arp_tpa={ip}"));
        }
        if let Some(mac) = self.arp_sha {
            parts.push(format!("arp_sha={}", format_mac(mac)));
        }
        if let Some(mac) = self.arp_tha {
            parts.push(format!("arp_tha={}", format_mac(mac)));
        }
        if let Some(id) = self.tunnel_id {
            parts.push(format!("tunnel_id={id:#x}"));
        }
        if let Some(state) = self.ct_state {
            let s = format_ct_state(state);
            if let Some(mask) = self.ct_state_mask {
                if mask == state {
                    parts.push(format!("ct_state={s}"));
                } else {
                    parts.push(format!("ct_state={s}/{}", format_ct_state(mask)));
                }
            } else {
                parts.push(format!("ct_state={s}"));
            }
        }
        if let Some(z) = self.ct_zone {
            parts.push(format!("ct_zone={z}"));
        }
        if let Some(mark) = self.ct_mark {
            if let Some(mask) = self.ct_mark_mask {
                if mask == u32::MAX {
                    parts.push(format!("ct_mark=0x{mark:x}"));
                } else {
                    parts.push(format!("ct_mark=0x{mark:x}/0x{mask:x}"));
                }
            } else {
                parts.push(format!("ct_mark=0x{mark:x}"));
            }
        }

        write!(f, "{}", parts.join(","))
    }
}

impl Match {
    /// Decode OXM fields from raw bytes (without match header).
    ///
    /// This is useful for parsing match fields from Packet-In messages
    /// where the match header has already been processed.
    pub fn decode_oxm(oxm_data: &[u8]) -> crate::Result<Self> {
        let mut m = Match::new();
        let mut offset = 0;

        while offset + 4 <= oxm_data.len() {
            let header = u32::from_be_bytes([
                oxm_data[offset],
                oxm_data[offset + 1],
                oxm_data[offset + 2],
                oxm_data[offset + 3],
            ]);

            let oxm_class = (header >> 16) as u16;
            let field = ((header >> 9) & 0x7f) as u8;
            let has_mask = ((header >> 8) & 1) != 0;
            let length = (header & 0xff) as usize;

            offset += 4; // Skip header

            if offset + length > oxm_data.len() {
                break; // Not enough data
            }

            let value = &oxm_data[offset..offset + length];
            let value_len = if has_mask { length / 2 } else { length };

            // Decode based on class and field
            if oxm_class == OxmClass::OpenflowBasic as u16 {
                Self::decode_oxm_field(&mut m, field, has_mask, value, value_len);
            } else if oxm_class == OxmClass::Nxm1 as u16 {
                Self::decode_nxm_field(&mut m, field, has_mask, value, value_len);
            } else if oxm_class == OxmClass::Nxm0 as u16 {
                Self::decode_nxm0_field(&mut m, field, has_mask, value, value_len);
            }
            // Skip unknown classes

            offset += length;
        }

        Ok(m)
    }

    /// Decode a match from OpenFlow wire format.
    ///
    /// Returns the decoded match and the total number of bytes consumed
    /// (including padding to 8-byte boundary).
    #[allow(clippy::too_many_lines)]
    pub fn decode(data: &[u8]) -> crate::Result<(Self, usize)> {
        if data.len() < 4 {
            return Err(crate::Error::Parse("match header too short".into()));
        }

        let match_type = u16::from_be_bytes([data[0], data[1]]);
        let match_len = u16::from_be_bytes([data[2], data[3]]) as usize;

        if match_type != MatchType::Oxm as u16 {
            return Err(crate::Error::Parse(format!(
                "unsupported match type: {match_type}"
            )));
        }

        if data.len() < match_len {
            return Err(crate::Error::Parse("match data truncated".into()));
        }

        let mut m = Match::new();
        let oxm_data = &data[4..match_len];
        let mut offset = 0;

        while offset + 4 <= oxm_data.len() {
            let header = u32::from_be_bytes([
                oxm_data[offset],
                oxm_data[offset + 1],
                oxm_data[offset + 2],
                oxm_data[offset + 3],
            ]);

            let oxm_class = (header >> 16) as u16;
            let field = ((header >> 9) & 0x7f) as u8;
            let has_mask = ((header >> 8) & 1) != 0;
            let length = (header & 0xff) as usize;

            offset += 4; // Skip header

            if offset + length > oxm_data.len() {
                break; // Not enough data
            }

            let value = &oxm_data[offset..offset + length];
            let value_len = if has_mask { length / 2 } else { length };

            // Decode based on class and field
            if oxm_class == OxmClass::OpenflowBasic as u16 {
                Self::decode_oxm_field(&mut m, field, has_mask, value, value_len);
            } else if oxm_class == OxmClass::Nxm1 as u16 {
                Self::decode_nxm_field(&mut m, field, has_mask, value, value_len);
            } else if oxm_class == OxmClass::Nxm0 as u16 {
                Self::decode_nxm0_field(&mut m, field, has_mask, value, value_len);
            }
            // Skip unknown classes

            offset += length;
        }

        // Calculate padded length (8-byte boundary)
        let padded_len = (match_len + 7) & !7;

        Ok((m, padded_len))
    }

    /// Decode an OXM field (OpenFlow Basic class).
    #[allow(clippy::too_many_lines)]
    fn decode_oxm_field(
        m: &mut Match,
        field: u8,
        has_mask: bool,
        value: &[u8],
        value_len: usize,
    ) {
        match field {
            f if f == OxmField::PacketType as u8 => {
                if value_len >= 4 {
                    let raw = u32::from_be_bytes([value[0], value[1], value[2], value[3]]);
                    m.packet_type = Some(((raw >> 16) as u16, raw as u16));
                }
            }
            f if f == OxmField::InPort as u8 => {
                if value_len >= 4 {
                    m.in_port = Some(u32::from_be_bytes([value[0], value[1], value[2], value[3]]));
                }
            }
            f if f == OxmField::InPhyPort as u8 => {
                if value_len >= 4 {
                    m.in_phy_port = Some(u32::from_be_bytes([value[0], value[1], value[2], value[3]]));
                }
            }
            f if f == OxmField::Metadata as u8 => {
                if value_len >= 8 {
                    m.metadata = Some(u64::from_be_bytes([
                        value[0], value[1], value[2], value[3],
                        value[4], value[5], value[6], value[7],
                    ]));
                    if has_mask && value.len() >= 16 {
                        m.metadata_mask = Some(u64::from_be_bytes([
                            value[8], value[9], value[10], value[11],
                            value[12], value[13], value[14], value[15],
                        ]));
                    }
                }
            }
            f if f == OxmField::EthDst as u8 => {
                if value_len >= 6 {
                    let mut mac = [0u8; 6];
                    mac.copy_from_slice(&value[..6]);
                    m.eth_dst = Some(mac);
                    if has_mask && value.len() >= 12 {
                        let mut mask = [0u8; 6];
                        mask.copy_from_slice(&value[6..12]);
                        m.eth_dst_mask = Some(mask);
                    }
                }
            }
            f if f == OxmField::EthSrc as u8 => {
                if value_len >= 6 {
                    let mut mac = [0u8; 6];
                    mac.copy_from_slice(&value[..6]);
                    m.eth_src = Some(mac);
                    if has_mask && value.len() >= 12 {
                        let mut mask = [0u8; 6];
                        mask.copy_from_slice(&value[6..12]);
                        m.eth_src_mask = Some(mask);
                    }
                }
            }
            f if f == OxmField::EthType as u8 => {
                if value_len >= 2 {
                    m.eth_type = Some(u16::from_be_bytes([value[0], value[1]]));
                }
            }
            f if f == OxmField::VlanVid as u8 => {
                if value_len >= 2 {
                    let vid = u16::from_be_bytes([value[0], value[1]]);
                    // Remove CFI bit (0x1000)
                    m.vlan_vid = Some(vid & 0x0fff);
                }
            }
            f if f == OxmField::VlanPcp as u8 => {
                if value_len >= 1 {
                    m.vlan_pcp = Some(value[0]);
                }
            }
            f if f == OxmField::IpDscp as u8 => {
                if value_len >= 1 {
                    m.ip_dscp = Some(value[0]);
                }
            }
            f if f == OxmField::IpEcn as u8 => {
                if value_len >= 1 {
                    m.ip_ecn = Some(value[0]);
                }
            }
            f if f == OxmField::IpProto as u8 => {
                if value_len >= 1 {
                    m.ip_proto = Some(value[0]);
                }
            }
            f if f == OxmField::Ipv4Src as u8 => {
                if value_len >= 4 {
                    let addr = Ipv4Addr::new(value[0], value[1], value[2], value[3]);
                    m.ipv4_src = Some(addr);
                    if has_mask && value.len() >= 8 {
                        let mask = u32::from_be_bytes([value[4], value[5], value[6], value[7]]);
                        m.ipv4_src_mask = Some(mask_to_prefix(mask));
                    } else {
                        m.ipv4_src_mask = Some(32);
                    }
                }
            }
            f if f == OxmField::Ipv4Dst as u8 => {
                if value_len >= 4 {
                    let addr = Ipv4Addr::new(value[0], value[1], value[2], value[3]);
                    m.ipv4_dst = Some(addr);
                    if has_mask && value.len() >= 8 {
                        let mask = u32::from_be_bytes([value[4], value[5], value[6], value[7]]);
                        m.ipv4_dst_mask = Some(mask_to_prefix(mask));
                    } else {
                        m.ipv4_dst_mask = Some(32);
                    }
                }
            }
            f if f == OxmField::TcpSrc as u8 => {
                if value_len >= 2 {
                    m.tcp_src = Some(u16::from_be_bytes([value[0], value[1]]));
                }
            }
            f if f == OxmField::TcpDst as u8 => {
                if value_len >= 2 {
                    m.tcp_dst = Some(u16::from_be_bytes([value[0], value[1]]));
                }
            }
            f if f == OxmField::UdpSrc as u8 => {
                if value_len >= 2 {
                    m.udp_src = Some(u16::from_be_bytes([value[0], value[1]]));
                }
            }
            f if f == OxmField::UdpDst as u8 => {
                if value_len >= 2 {
                    m.udp_dst = Some(u16::from_be_bytes([value[0], value[1]]));
                }
            }
            f if f == OxmField::Icmpv4Type as u8 => {
                if value_len >= 1 {
                    m.icmp_type = Some(value[0]);
                }
            }
            f if f == OxmField::Icmpv4Code as u8 => {
                if value_len >= 1 {
                    m.icmp_code = Some(value[0]);
                }
            }
            f if f == OxmField::Icmpv6Type as u8 => {
                if value_len >= 1 {
                    m.icmpv6_type = Some(value[0]);
                }
            }
            f if f == OxmField::Icmpv6Code as u8 => {
                if value_len >= 1 {
                    m.icmpv6_code = Some(value[0]);
                }
            }
            f if f == OxmField::ArpOp as u8 => {
                if value_len >= 2 {
                    m.arp_op = Some(u16::from_be_bytes([value[0], value[1]]));
                }
            }
            f if f == OxmField::ArpSpa as u8 => {
                if value_len >= 4 {
                    m.arp_spa = Some(Ipv4Addr::new(value[0], value[1], value[2], value[3]));
                }
            }
            f if f == OxmField::ArpTpa as u8 => {
                if value_len >= 4 {
                    m.arp_tpa = Some(Ipv4Addr::new(value[0], value[1], value[2], value[3]));
                }
            }
            f if f == OxmField::ArpSha as u8 => {
                if value_len >= 6 {
                    let mut mac = [0u8; 6];
                    mac.copy_from_slice(&value[..6]);
                    m.arp_sha = Some(mac);
                }
            }
            f if f == OxmField::ArpTha as u8 => {
                if value_len >= 6 {
                    let mut mac = [0u8; 6];
                    mac.copy_from_slice(&value[..6]);
                    m.arp_tha = Some(mac);
                }
            }
            f if f == OxmField::Ipv6Src as u8 => {
                if value_len >= 16 {
                    let mut octets = [0u8; 16];
                    octets.copy_from_slice(&value[..16]);
                    m.ipv6_src = Some(Ipv6Addr::from(octets));
                    if has_mask && value.len() >= 32 {
                        let mut mask_bytes = [0u8; 16];
                        mask_bytes.copy_from_slice(&value[16..32]);
                        let mask = u128::from_be_bytes(mask_bytes);
                        m.ipv6_src_mask = Some(mask_to_prefix_v6(mask));
                    } else {
                        m.ipv6_src_mask = Some(128);
                    }
                }
            }
            f if f == OxmField::Ipv6Dst as u8 => {
                if value_len >= 16 {
                    let mut octets = [0u8; 16];
                    octets.copy_from_slice(&value[..16]);
                    m.ipv6_dst = Some(Ipv6Addr::from(octets));
                    if has_mask && value.len() >= 32 {
                        let mut mask_bytes = [0u8; 16];
                        mask_bytes.copy_from_slice(&value[16..32]);
                        let mask = u128::from_be_bytes(mask_bytes);
                        m.ipv6_dst_mask = Some(mask_to_prefix_v6(mask));
                    } else {
                        m.ipv6_dst_mask = Some(128);
                    }
                }
            }
            f if f == OxmField::Ipv6Flabel as u8 => {
                if value_len >= 4 {
                    m.ipv6_flabel = Some(u32::from_be_bytes([value[0], value[1], value[2], value[3]]));
                }
            }
            f if f == OxmField::TunnelId as u8 => {
                if value_len >= 8 {
                    m.tunnel_id = Some(u64::from_be_bytes([
                        value[0], value[1], value[2], value[3],
                        value[4], value[5], value[6], value[7],
                    ]));
                }
            }
            _ => {
                // Unknown field, skip
            }
        }
    }

    /// Decode an NXM field (Nicira extensions).
    /// Decode an NXM0 field (Nicira class 0x0000 — OpenFlow 1.0 compatible fields).
    ///
    /// NXM0 field numbering differs from OXM (OpenFlow Basic class):
    ///   0=IN_PORT, 1=ETH_DST, 2=ETH_SRC, 3=ETH_TYPE, 4=VLAN_TCI,
    ///   5=IP_TOS, 6=IP_PROTO, 7=IP_SRC, 8=IP_DST, 9=TCP_SRC,
    ///   10=TCP_DST, 11=UDP_SRC, 12=UDP_DST, 13=ICMP_TYPE, 14=ICMP_CODE,
    ///   15=ARP_OP, 16=ARP_SPA, 17=ARP_TPA
    fn decode_nxm0_field(
        m: &mut Match,
        field: u8,
        has_mask: bool,
        value: &[u8],
        value_len: usize,
    ) {
        match field {
            // NXM_OF_IN_PORT (2 bytes, stored as u32 in Match)
            0 if value_len >= 2 => {
                m.in_port = Some(u16::from_be_bytes([value[0], value[1]]) as u32);
            }
            // NXM_OF_ETH_DST (6 bytes)
            1 if value_len >= 6 => {
                let mut mac = [0u8; 6];
                mac.copy_from_slice(&value[..6]);
                m.eth_dst = Some(mac);
                if has_mask && value.len() >= 12 {
                    let mut mask = [0u8; 6];
                    mask.copy_from_slice(&value[6..12]);
                    m.eth_dst_mask = Some(mask);
                }
            }
            // NXM_OF_ETH_SRC (6 bytes)
            2 if value_len >= 6 => {
                let mut mac = [0u8; 6];
                mac.copy_from_slice(&value[..6]);
                m.eth_src = Some(mac);
                if has_mask && value.len() >= 12 {
                    let mut mask = [0u8; 6];
                    mask.copy_from_slice(&value[6..12]);
                    m.eth_src_mask = Some(mask);
                }
            }
            // NXM_OF_ETH_TYPE (2 bytes)
            3 if value_len >= 2 => {
                m.eth_type = Some(u16::from_be_bytes([value[0], value[1]]));
            }
            // NXM_OF_VLAN_TCI (2 bytes)
            4 if value_len >= 2 => {
                let tci = u16::from_be_bytes([value[0], value[1]]);
                // VLAN VID is the lower 12 bits with OFPVID_PRESENT (0x1000)
                m.vlan_vid = Some(tci);
            }
            // NXM_OF_IP_TOS (1 byte — contains DSCP in upper 6 bits)
            5 if value_len >= 1 => {
                m.ip_dscp = Some(value[0] >> 2);
            }
            // NXM_OF_IP_PROTO (1 byte)
            6 if value_len >= 1 => {
                m.ip_proto = Some(value[0]);
            }
            // NXM_OF_IP_SRC (4 bytes)
            7 if value_len >= 4 => {
                m.ipv4_src = Some(Ipv4Addr::new(value[0], value[1], value[2], value[3]));
                if has_mask && value.len() >= 8 {
                    let mask = u32::from_be_bytes([value[4], value[5], value[6], value[7]]);
                    m.ipv4_src_mask = Some(mask_to_prefix(mask));
                } else {
                    m.ipv4_src_mask = Some(32);
                }
            }
            // NXM_OF_IP_DST (4 bytes)
            8 if value_len >= 4 => {
                m.ipv4_dst = Some(Ipv4Addr::new(value[0], value[1], value[2], value[3]));
                if has_mask && value.len() >= 8 {
                    let mask = u32::from_be_bytes([value[4], value[5], value[6], value[7]]);
                    m.ipv4_dst_mask = Some(mask_to_prefix(mask));
                } else {
                    m.ipv4_dst_mask = Some(32);
                }
            }
            // NXM_OF_TCP_SRC (2 bytes)
            9 if value_len >= 2 => {
                m.tcp_src = Some(u16::from_be_bytes([value[0], value[1]]));
            }
            // NXM_OF_TCP_DST (2 bytes)
            10 if value_len >= 2 => {
                m.tcp_dst = Some(u16::from_be_bytes([value[0], value[1]]));
            }
            // NXM_OF_UDP_SRC (2 bytes)
            11 if value_len >= 2 => {
                m.udp_src = Some(u16::from_be_bytes([value[0], value[1]]));
            }
            // NXM_OF_UDP_DST (2 bytes)
            12 if value_len >= 2 => {
                m.udp_dst = Some(u16::from_be_bytes([value[0], value[1]]));
            }
            // NXM_OF_ICMP_TYPE (1 byte)
            13 if value_len >= 1 => {
                m.icmp_type = Some(value[0]);
            }
            // NXM_OF_ICMP_CODE (1 byte)
            14 if value_len >= 1 => {
                m.icmp_code = Some(value[0]);
            }
            // NXM_OF_ARP_OP (2 bytes)
            15 if value_len >= 2 => {
                m.arp_op = Some(u16::from_be_bytes([value[0], value[1]]));
            }
            // NXM_OF_ARP_SPA (4 bytes)
            16 if value_len >= 4 => {
                m.arp_spa = Some(Ipv4Addr::new(value[0], value[1], value[2], value[3]));
            }
            // NXM_OF_ARP_TPA (4 bytes)
            17 if value_len >= 4 => {
                m.arp_tpa = Some(Ipv4Addr::new(value[0], value[1], value[2], value[3]));
            }
            _ => {
                // Unknown NXM0 field, skip
            }
        }
    }

    fn decode_nxm_field(
        m: &mut Match,
        field: u8,
        has_mask: bool,
        value: &[u8],
        value_len: usize,
    ) {
        match field {
            // NXM1 field 16 is TUN_ID
            16 if value_len >= 8 => {
                m.tunnel_id = Some(u64::from_be_bytes([
                    value[0], value[1], value[2], value[3],
                    value[4], value[5], value[6], value[7],
                ]));
            }
            // CT_STATE = 105
            105 if value_len >= 4 => {
                m.ct_state = Some(u32::from_be_bytes([
                    value[0], value[1], value[2], value[3],
                ]));
                if has_mask && value.len() >= 8 {
                    m.ct_state_mask = Some(u32::from_be_bytes([
                        value[4], value[5], value[6], value[7],
                    ]));
                }
            }
            // CT_ZONE = 106
            106 if value_len >= 2 => {
                m.ct_zone = Some(u16::from_be_bytes([value[0], value[1]]));
            }
            // CT_MARK = 107
            107 if value_len >= 4 => {
                m.ct_mark = Some(u32::from_be_bytes([
                    value[0], value[1], value[2], value[3],
                ]));
                if has_mask && value.len() >= 8 {
                    m.ct_mark_mask = Some(u32::from_be_bytes([
                        value[4], value[5], value[6], value[7],
                    ]));
                }
            }
            _ => {
                // Unknown NXM field, skip
            }
        }
    }

    /// Encode the match to OpenFlow wire format (OXM).
    ///
    /// The match is encoded as:
    /// - Match header: type (2 bytes) + length (2 bytes)
    /// - OXM fields (variable)
    /// - Padding to 8-byte boundary
    ///
    /// Fields are encoded in OpenFlow-specified order with prerequisites
    /// automatically satisfied by the builder methods.
    #[allow(clippy::too_many_lines)]
    pub fn encode(&self) -> Vec<u8> {
        // Encode all OXM fields first
        let mut oxm_fields = Vec::new();

        // Encode fields in OpenFlow-specified order
        // This ordering ensures prerequisites come before dependent fields

        // Packet type (OF1.5) — outermost qualifier, precedes in_port.
        // Two BE u16s concatenate to one BE u32, so encode_u32 is exact.
        if let Some((ns, ty)) = self.packet_type {
            oxm_fields.extend(oxm::encode_u32(
                OxmClass::OpenflowBasic,
                OxmField::PacketType as u8,
                ((u32::from(ns)) << 16) | u32::from(ty),
            ));
        }

        // Port fields
        if let Some(port) = self.in_port {
            oxm_fields.extend(oxm::encode_u32(
                OxmClass::OpenflowBasic,
                OxmField::InPort as u8,
                port,
            ));
        }
        if let Some(port) = self.in_phy_port {
            oxm_fields.extend(oxm::encode_u32(
                OxmClass::OpenflowBasic,
                OxmField::InPhyPort as u8,
                port,
            ));
        }

        // Metadata
        if let Some(metadata) = self.metadata {
            if let Some(mask) = self.metadata_mask {
                oxm_fields.extend(oxm::encode_u64_masked(
                    OxmClass::OpenflowBasic,
                    OxmField::Metadata as u8,
                    metadata,
                    mask,
                ));
            } else {
                oxm_fields.extend(oxm::encode_u64(
                    OxmClass::OpenflowBasic,
                    OxmField::Metadata as u8,
                    metadata,
                ));
            }
        }

        // L2 fields
        if let Some(mac) = self.eth_dst {
            if let Some(mask) = self.eth_dst_mask {
                oxm_fields.extend(oxm::encode_mac_masked(
                    OxmClass::OpenflowBasic,
                    OxmField::EthDst as u8,
                    mac,
                    mask,
                ));
            } else {
                oxm_fields.extend(oxm::encode_mac(
                    OxmClass::OpenflowBasic,
                    OxmField::EthDst as u8,
                    mac,
                ));
            }
        }
        if let Some(mac) = self.eth_src {
            if let Some(mask) = self.eth_src_mask {
                oxm_fields.extend(oxm::encode_mac_masked(
                    OxmClass::OpenflowBasic,
                    OxmField::EthSrc as u8,
                    mac,
                    mask,
                ));
            } else {
                oxm_fields.extend(oxm::encode_mac(
                    OxmClass::OpenflowBasic,
                    OxmField::EthSrc as u8,
                    mac,
                ));
            }
        }
        if let Some(eth_type) = self.eth_type {
            oxm_fields.extend(oxm::encode_u16(
                OxmClass::OpenflowBasic,
                OxmField::EthType as u8,
                eth_type,
            ));
        }
        if let Some(vid) = self.vlan_vid {
            // VLAN VID has CFI bit (0x1000) set when present
            oxm_fields.extend(oxm::encode_u16(
                OxmClass::OpenflowBasic,
                OxmField::VlanVid as u8,
                vid | 0x1000,
            ));
        }
        if let Some(pcp) = self.vlan_pcp {
            oxm_fields.extend(oxm::encode_u8(
                OxmClass::OpenflowBasic,
                OxmField::VlanPcp as u8,
                pcp,
            ));
        }

        // L3 IPv4 fields
        if let Some(dscp) = self.ip_dscp {
            oxm_fields.extend(oxm::encode_u8(
                OxmClass::OpenflowBasic,
                OxmField::IpDscp as u8,
                dscp,
            ));
        }
        if let Some(ecn) = self.ip_ecn {
            oxm_fields.extend(oxm::encode_u8(
                OxmClass::OpenflowBasic,
                OxmField::IpEcn as u8,
                ecn,
            ));
        }
        if let Some(proto) = self.ip_proto {
            oxm_fields.extend(oxm::encode_u8(
                OxmClass::OpenflowBasic,
                OxmField::IpProto as u8,
                proto,
            ));
        }
        if let Some(addr) = self.ipv4_src {
            let addr_u32: u32 = addr.into();
            if let Some(prefix) = self.ipv4_src_mask {
                if prefix < 32 {
                    let mask = oxm::prefix_to_mask(prefix);
                    oxm_fields.extend(oxm::encode_u32_masked(
                        OxmClass::OpenflowBasic,
                        OxmField::Ipv4Src as u8,
                        addr_u32,
                        mask,
                    ));
                } else {
                    oxm_fields.extend(oxm::encode_u32(
                        OxmClass::OpenflowBasic,
                        OxmField::Ipv4Src as u8,
                        addr_u32,
                    ));
                }
            } else {
                oxm_fields.extend(oxm::encode_u32(
                    OxmClass::OpenflowBasic,
                    OxmField::Ipv4Src as u8,
                    addr_u32,
                ));
            }
        }
        if let Some(addr) = self.ipv4_dst {
            let addr_u32: u32 = addr.into();
            if let Some(prefix) = self.ipv4_dst_mask {
                if prefix < 32 {
                    let mask = oxm::prefix_to_mask(prefix);
                    oxm_fields.extend(oxm::encode_u32_masked(
                        OxmClass::OpenflowBasic,
                        OxmField::Ipv4Dst as u8,
                        addr_u32,
                        mask,
                    ));
                } else {
                    oxm_fields.extend(oxm::encode_u32(
                        OxmClass::OpenflowBasic,
                        OxmField::Ipv4Dst as u8,
                        addr_u32,
                    ));
                }
            } else {
                oxm_fields.extend(oxm::encode_u32(
                    OxmClass::OpenflowBasic,
                    OxmField::Ipv4Dst as u8,
                    addr_u32,
                ));
            }
        }

        // L4 TCP fields
        if let Some(port) = self.tcp_src {
            oxm_fields.extend(oxm::encode_u16(
                OxmClass::OpenflowBasic,
                OxmField::TcpSrc as u8,
                port,
            ));
        }
        if let Some(port) = self.tcp_dst {
            oxm_fields.extend(oxm::encode_u16(
                OxmClass::OpenflowBasic,
                OxmField::TcpDst as u8,
                port,
            ));
        }

        // L4 UDP fields
        if let Some(port) = self.udp_src {
            oxm_fields.extend(oxm::encode_u16(
                OxmClass::OpenflowBasic,
                OxmField::UdpSrc as u8,
                port,
            ));
        }
        if let Some(port) = self.udp_dst {
            oxm_fields.extend(oxm::encode_u16(
                OxmClass::OpenflowBasic,
                OxmField::UdpDst as u8,
                port,
            ));
        }

        // ICMPv4 fields
        if let Some(icmp_type) = self.icmp_type {
            oxm_fields.extend(oxm::encode_u8(
                OxmClass::OpenflowBasic,
                OxmField::Icmpv4Type as u8,
                icmp_type,
            ));
        }
        if let Some(icmp_code) = self.icmp_code {
            oxm_fields.extend(oxm::encode_u8(
                OxmClass::OpenflowBasic,
                OxmField::Icmpv4Code as u8,
                icmp_code,
            ));
        }

        // ICMPv6 fields
        if let Some(icmpv6_type) = self.icmpv6_type {
            oxm_fields.extend(oxm::encode_u8(
                OxmClass::OpenflowBasic,
                OxmField::Icmpv6Type as u8,
                icmpv6_type,
            ));
        }
        if let Some(icmpv6_code) = self.icmpv6_code {
            oxm_fields.extend(oxm::encode_u8(
                OxmClass::OpenflowBasic,
                OxmField::Icmpv6Code as u8,
                icmpv6_code,
            ));
        }

        // ARP fields
        if let Some(op) = self.arp_op {
            oxm_fields.extend(oxm::encode_u16(
                OxmClass::OpenflowBasic,
                OxmField::ArpOp as u8,
                op,
            ));
        }
        if let Some(addr) = self.arp_spa {
            oxm_fields.extend(oxm::encode_u32(
                OxmClass::OpenflowBasic,
                OxmField::ArpSpa as u8,
                addr.into(),
            ));
        }
        if let Some(addr) = self.arp_tpa {
            oxm_fields.extend(oxm::encode_u32(
                OxmClass::OpenflowBasic,
                OxmField::ArpTpa as u8,
                addr.into(),
            ));
        }
        if let Some(mac) = self.arp_sha {
            oxm_fields.extend(oxm::encode_mac(
                OxmClass::OpenflowBasic,
                OxmField::ArpSha as u8,
                mac,
            ));
        }
        if let Some(mac) = self.arp_tha {
            oxm_fields.extend(oxm::encode_mac(
                OxmClass::OpenflowBasic,
                OxmField::ArpTha as u8,
                mac,
            ));
        }

        // IPv6 fields
        if let Some(addr) = self.ipv6_src {
            let octets = addr.octets();
            let value = u128::from_be_bytes(octets);
            if let Some(prefix) = self.ipv6_src_mask {
                if prefix < 128 {
                    let mask = oxm::prefix_to_mask_v6(prefix);
                    oxm_fields.extend(encode_ipv6_masked(OxmField::Ipv6Src as u8, value, mask));
                } else {
                    oxm_fields.extend(encode_ipv6(OxmField::Ipv6Src as u8, value));
                }
            } else {
                oxm_fields.extend(encode_ipv6(OxmField::Ipv6Src as u8, value));
            }
        }
        if let Some(addr) = self.ipv6_dst {
            let octets = addr.octets();
            let value = u128::from_be_bytes(octets);
            if let Some(prefix) = self.ipv6_dst_mask {
                if prefix < 128 {
                    let mask = oxm::prefix_to_mask_v6(prefix);
                    oxm_fields.extend(encode_ipv6_masked(OxmField::Ipv6Dst as u8, value, mask));
                } else {
                    oxm_fields.extend(encode_ipv6(OxmField::Ipv6Dst as u8, value));
                }
            } else {
                oxm_fields.extend(encode_ipv6(OxmField::Ipv6Dst as u8, value));
            }
        }
        if let Some(flabel) = self.ipv6_flabel {
            oxm_fields.extend(oxm::encode_u32(
                OxmClass::OpenflowBasic,
                OxmField::Ipv6Flabel as u8,
                flabel,
            ));
        }

        // Tunnel ID (NXM field)
        if let Some(tun_id) = self.tunnel_id {
            oxm_fields.extend(oxm::encode_tun_id(tun_id));
        }

        // Connection tracking (NXM fields)
        if let Some(state) = self.ct_state {
            if let Some(mask) = self.ct_state_mask {
                if mask == 0xffff_ffff {
                    oxm_fields.extend(oxm::encode_ct_state(state));
                } else {
                    oxm_fields.extend(oxm::encode_ct_state_masked(state, mask));
                }
            } else {
                oxm_fields.extend(oxm::encode_ct_state(state));
            }
        }
        if let Some(zone) = self.ct_zone {
            oxm_fields.extend(oxm::encode_ct_zone(zone));
        }
        if let Some(mark) = self.ct_mark {
            if let Some(mask) = self.ct_mark_mask {
                oxm_fields.extend(oxm::encode_ct_mark_masked(mark, mask));
            } else {
                oxm_fields.extend(oxm::encode_ct_mark(mark));
            }
        }

        // Build match structure
        // Match header: type (2) + length (2) + OXM fields + padding
        // Length includes header (4 bytes) + OXM fields length
        let oxm_len = oxm_fields.len();
        let match_len = 4 + oxm_len; // header + OXM fields
        let padded_len = (match_len + 7) & !7; // Round up to 8-byte boundary
        let padding = padded_len - match_len;

        let mut buf = Vec::with_capacity(padded_len);
        buf.extend((MatchType::Oxm as u16).to_be_bytes()); // type = 1 (OXM)
        buf.extend((match_len as u16).to_be_bytes()); // length (includes header)
        buf.extend(oxm_fields);
        buf.extend(std::iter::repeat_n(0u8, padding));
        buf
    }

    /// Encode just the OXM/NXM field TLVs without the match header.
    ///
    /// This is used by protocols that embed raw OXM fields without the
    /// standard OpenFlow match header (e.g., Nicira flow monitor requests).
    pub fn encode_oxm_fields(&self) -> Vec<u8> {
        // Re-use the full encode, then strip the 4-byte header and padding
        let full = self.encode();
        if full.len() <= 4 {
            return Vec::new();
        }
        let match_len = u16::from_be_bytes([full[2], full[3]]) as usize;
        // OXM fields are bytes 4..match_len (before padding)
        full[4..match_len].to_vec()
    }
}

/// Convert a 32-bit network mask to prefix length.
fn mask_to_prefix(mask: u32) -> u8 {
    mask.leading_ones() as u8
}

/// Convert a 128-bit network mask to prefix length.
fn mask_to_prefix_v6(mask: u128) -> u8 {
    mask.leading_ones() as u8
}

// Helper functions for IPv6 encoding (16 bytes)
fn encode_ipv6(field: u8, value: u128) -> Vec<u8> {
    let mut buf = Vec::with_capacity(20);
    // OXM header: class=0x8000, field, has_mask=false, length=16
    let oxm_header = ((OxmClass::OpenflowBasic as u32) << 16) | ((field as u32) << 9) | 16;
    buf.extend(oxm_header.to_be_bytes());
    buf.extend(value.to_be_bytes());
    buf
}

fn encode_ipv6_masked(field: u8, value: u128, mask: u128) -> Vec<u8> {
    let mut buf = Vec::with_capacity(36);
    // OXM header: class=0x8000, field, has_mask=true, length=32
    let oxm_header = ((OxmClass::OpenflowBasic as u32) << 16) | ((field as u32) << 9) | (1 << 8) | 32;
    buf.extend(oxm_header.to_be_bytes());
    buf.extend(value.to_be_bytes());
    buf.extend(mask.to_be_bytes());
    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn match_type_values() {
        assert_eq!(MatchType::Standard as u16, 0);
        assert_eq!(MatchType::Oxm as u16, 1);
    }

    #[test]
    fn encode_empty_match() {
        let m = Match::new();
        let bytes = m.encode();
        // Empty match: header (4) + padding (4) = 8 bytes
        assert_eq!(bytes.len(), 8);
        // type = 1 (OXM)
        assert_eq!(&bytes[0..2], &[0x00, 0x01]);
        // length = 4 (just header, no fields)
        assert_eq!(&bytes[2..4], &[0x00, 0x04]);
        // padding to 8 bytes
        assert_eq!(&bytes[4..8], &[0, 0, 0, 0]);
    }

    #[test]
    fn encode_in_port_match() {
        let m = Match::new().in_port(1);
        let bytes = m.encode();
        // Header (4) + InPort OXM (4 header + 4 value) = 12, padded to 16
        assert_eq!(bytes.len(), 16);
        // type = 1 (OXM)
        assert_eq!(&bytes[0..2], &[0x00, 0x01]);
        // length = 12 (header + OXM)
        assert_eq!(&bytes[2..4], &[0x00, 0x0c]);
        // OXM header: class=0x8000, field=0 (InPort), has_mask=0, length=4
        let expected_oxm: u32 = (0x8000 << 16) | (0 << 9) | 4;
        assert_eq!(&bytes[4..8], &expected_oxm.to_be_bytes());
        // InPort value = 1
        assert_eq!(&bytes[8..12], &[0x00, 0x00, 0x00, 0x01]);
    }

    #[test]
    fn encode_eth_type_match() {
        let m = Match::new().eth_type(0x0800);
        let bytes = m.encode();
        // Header (4) + EthType OXM (4 header + 2 value) = 10, padded to 16
        assert_eq!(bytes.len(), 16);
        // length = 10
        assert_eq!(&bytes[2..4], &[0x00, 0x0a]);
        // OXM header: class=0x8000, field=5 (EthType), has_mask=0, length=2
        let expected_oxm: u32 = (0x8000 << 16) | (5 << 9) | 2;
        assert_eq!(&bytes[4..8], &expected_oxm.to_be_bytes());
        // EthType value = 0x0800
        assert_eq!(&bytes[8..10], &[0x08, 0x00]);
    }

    #[test]
    fn encode_ipv4_dst_with_prefix() {
        let m = Match::new().ipv4_dst("10.0.0.0".parse().unwrap(), 24);
        let bytes = m.encode();
        // Header (4) + EthType (6) + Ipv4Dst masked (12) = 22, padded to 24
        assert_eq!(bytes.len(), 24);
        // EthType should be auto-set to 0x0800
        // Check EthType OXM at offset 4
        let eth_type_oxm: u32 = (0x8000 << 16) | (5 << 9) | 2;
        assert_eq!(&bytes[4..8], &eth_type_oxm.to_be_bytes());
        assert_eq!(&bytes[8..10], &[0x08, 0x00]); // EthType = IPv4
    }

    #[test]
    fn encode_tcp_dst_match() {
        let m = Match::new().eth_type(0x0800).ip_proto(6).tcp_dst(80);
        let bytes = m.encode();
        // Header (4) + EthType (6) + IpProto (5) + TcpDst (6) = 21, padded to 24
        assert_eq!(bytes.len(), 24);
    }

    #[test]
    fn encode_eth_dst_match() {
        let mac: MacAddr = [0x00, 0x11, 0x22, 0x33, 0x44, 0x55];
        let m = Match::new().eth_dst(mac);
        let bytes = m.encode();
        // Header (4) + EthDst OXM (4 header + 6 value) = 14, padded to 16
        assert_eq!(bytes.len(), 16);
        // OXM header: class=0x8000, field=3 (EthDst), has_mask=0, length=6
        let expected_oxm: u32 = (0x8000 << 16) | (3 << 9) | 6;
        assert_eq!(&bytes[4..8], &expected_oxm.to_be_bytes());
        // MAC address
        assert_eq!(&bytes[8..14], &mac);
    }

    #[test]
    fn encode_vlan_vid_match() {
        let m = Match::new().vlan_vid(100);
        let bytes = m.encode();
        // Header (4) + VlanVid OXM (4 header + 2 value) = 10, padded to 16
        assert_eq!(bytes.len(), 16);
        // OXM header: class=0x8000, field=6 (VlanVid), has_mask=0, length=2
        let expected_oxm: u32 = (0x8000 << 16) | (6 << 9) | 2;
        assert_eq!(&bytes[4..8], &expected_oxm.to_be_bytes());
        // VLAN VID with CFI bit = 100 | 0x1000 = 0x1064
        assert_eq!(&bytes[8..10], &[0x10, 0x64]);
    }

    #[test]
    fn encode_tunnel_id_match() {
        let m = Match::new().tunnel_id(0x1234);
        let bytes = m.encode();
        // Header (4) + TunId NXM (4 header + 8 value) = 16, already aligned
        assert_eq!(bytes.len(), 16);
    }

    #[test]
    fn encode_multiple_fields() {
        let m = Match::new()
            .in_port(1)
            .eth_type(0x0800)
            .ipv4_dst("192.168.1.0".parse().unwrap(), 24);
        let bytes = m.encode();
        // Verify it's 8-byte aligned
        assert_eq!(bytes.len() % 8, 0);
        // Verify type = OXM
        assert_eq!(&bytes[0..2], &[0x00, 0x01]);
    }

    #[test]
    fn match_8_byte_alignment() {
        // Test various field combinations ensure 8-byte alignment
        let m1 = Match::new().in_port(1);
        assert_eq!(m1.encode().len() % 8, 0);

        let m2 = Match::new().eth_type(0x0800);
        assert_eq!(m2.encode().len() % 8, 0);

        let m3 = Match::new().ip_proto(6);
        assert_eq!(m3.encode().len() % 8, 0);

        let m4 = Match::new()
            .in_port(1)
            .eth_type(0x0800)
            .ip_proto(6)
            .tcp_dst(80);
        assert_eq!(m4.encode().len() % 8, 0);
    }

    // Decode tests

    #[test]
    fn decode_empty_match() {
        // Empty match: type=1 (OXM), length=4, padding
        let data = [0x00, 0x01, 0x00, 0x04, 0x00, 0x00, 0x00, 0x00];
        let (m, len) = Match::decode(&data).unwrap();
        assert_eq!(len, 8);
        assert!(m.is_empty());
    }

    #[test]
    fn decode_in_port() {
        let original = Match::new().in_port(42);
        let encoded = original.encode();
        let (decoded, _) = Match::decode(&encoded).unwrap();
        assert_eq!(decoded.in_port, Some(42));
    }

    #[test]
    fn decode_eth_type() {
        let original = Match::new().eth_type(0x0800);
        let encoded = original.encode();
        let (decoded, _) = Match::decode(&encoded).unwrap();
        assert_eq!(decoded.eth_type, Some(0x0800));
    }

    #[test]
    fn decode_vlan_vid() {
        let original = Match::new().vlan_vid(100);
        let encoded = original.encode();
        let (decoded, _) = Match::decode(&encoded).unwrap();
        assert_eq!(decoded.vlan_vid, Some(100));
    }

    #[test]
    fn decode_ipv4_dst() {
        let original = Match::new().ipv4_dst("10.0.0.0".parse().unwrap(), 24);
        let encoded = original.encode();
        let (decoded, _) = Match::decode(&encoded).unwrap();
        assert_eq!(decoded.ipv4_dst, Some("10.0.0.0".parse().unwrap()));
        assert_eq!(decoded.ipv4_dst_mask, Some(24));
    }

    #[test]
    fn decode_tcp_dst() {
        let original = Match::new().eth_type(0x0800).ip_proto(6).tcp_dst(80);
        let encoded = original.encode();
        let (decoded, _) = Match::decode(&encoded).unwrap();
        assert_eq!(decoded.eth_type, Some(0x0800));
        assert_eq!(decoded.ip_proto, Some(6));
        assert_eq!(decoded.tcp_dst, Some(80));
    }

    #[test]
    fn decode_eth_dst() {
        let mac: MacAddr = [0x00, 0x11, 0x22, 0x33, 0x44, 0x55];
        let original = Match::new().eth_dst(mac);
        let encoded = original.encode();
        let (decoded, _) = Match::decode(&encoded).unwrap();
        assert_eq!(decoded.eth_dst, Some(mac));
    }

    #[test]
    fn decode_multiple_fields() {
        let original = Match::new()
            .in_port(1)
            .eth_type(0x0800)
            .ip_proto(6)
            .tcp_dst(443);
        let encoded = original.encode();
        let (decoded, _) = Match::decode(&encoded).unwrap();
        assert_eq!(decoded.in_port, Some(1));
        assert_eq!(decoded.eth_type, Some(0x0800));
        assert_eq!(decoded.ip_proto, Some(6));
        assert_eq!(decoded.tcp_dst, Some(443));
    }

    #[test]
    fn roundtrip_encode_decode() {
        // Create a complex match and verify roundtrip
        let original = Match::new()
            .in_port(5)
            .eth_dst([0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff])
            .eth_type(0x0800)
            .ipv4_dst("192.168.1.0".parse().unwrap(), 24)
            .ip_proto(17)
            .udp_dst(53);

        let encoded = original.encode();
        let (decoded, len) = Match::decode(&encoded).unwrap();

        assert_eq!(len, encoded.len());
        assert_eq!(decoded.in_port, Some(5));
        assert_eq!(decoded.eth_dst, Some([0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff]));
        assert_eq!(decoded.eth_type, Some(0x0800));
        assert_eq!(decoded.ipv4_dst, Some("192.168.1.0".parse().unwrap()));
        assert_eq!(decoded.ipv4_dst_mask, Some(24));
        assert_eq!(decoded.ip_proto, Some(17));
        assert_eq!(decoded.udp_dst, Some(53));
    }

    #[test]
    fn mask_to_prefix_conversion() {
        assert_eq!(mask_to_prefix(0xffff_ffff), 32);
        assert_eq!(mask_to_prefix(0xffff_ff00), 24);
        assert_eq!(mask_to_prefix(0xffff_0000), 16);
        assert_eq!(mask_to_prefix(0xff00_0000), 8);
        assert_eq!(mask_to_prefix(0x0000_0000), 0);
    }

    #[test]
    fn mask_to_prefix_v6_conversion() {
        assert_eq!(mask_to_prefix_v6(u128::MAX), 128);
        assert_eq!(mask_to_prefix_v6(u128::MAX << 64), 64);
        assert_eq!(mask_to_prefix_v6(0), 0);
    }

    // Connection tracking tests

    #[test]
    fn encode_ct_state() {
        use crate::oxm::ct_state;
        let m = Match::new().ct_state(ct_state::TRK | ct_state::EST);
        let bytes = m.encode();
        // Should have match header + ct_state OXM
        assert!(bytes.len() >= 8);
        // Verify it's 8-byte aligned
        assert_eq!(bytes.len() % 8, 0);
    }

    #[test]
    fn encode_ct_state_masked() {
        use crate::oxm::ct_state;
        let m = Match::new().ct_state_masked(
            ct_state::TRK | ct_state::NEW,
            ct_state::TRK | ct_state::NEW,
        );
        let bytes = m.encode();
        assert!(bytes.len() >= 8);
        assert_eq!(bytes.len() % 8, 0);
    }

    #[test]
    fn encode_ct_zone() {
        let m = Match::new().ct_zone(100);
        let bytes = m.encode();
        assert!(bytes.len() >= 8);
        assert_eq!(bytes.len() % 8, 0);
    }

    #[test]
    fn encode_ct_mark() {
        let m = Match::new().ct_mark(0xaabbccdd);
        let bytes = m.encode();
        assert!(bytes.len() >= 8);
        assert_eq!(bytes.len() % 8, 0);
    }

    #[test]
    fn encode_ct_mark_masked() {
        let m = Match::new().ct_mark_masked(0xff000000, 0xff000000);
        let bytes = m.encode();
        assert!(bytes.len() >= 8);
        assert_eq!(bytes.len() % 8, 0);
    }

    #[test]
    fn roundtrip_ct_state() {
        use crate::oxm::ct_state;
        let state = ct_state::TRK | ct_state::EST;
        let original = Match::new().ct_state(state);
        let encoded = original.encode();
        let (decoded, _) = Match::decode(&encoded).unwrap();
        assert_eq!(decoded.ct_state, Some(state));
    }

    #[test]
    fn roundtrip_ct_zone() {
        let original = Match::new().ct_zone(42);
        let encoded = original.encode();
        let (decoded, _) = Match::decode(&encoded).unwrap();
        assert_eq!(decoded.ct_zone, Some(42));
    }

    #[test]
    fn roundtrip_ct_mark() {
        let original = Match::new().ct_mark(0x12345678);
        let encoded = original.encode();
        let (decoded, _) = Match::decode(&encoded).unwrap();
        assert_eq!(decoded.ct_mark, Some(0x12345678));
    }

    #[test]
    fn stateful_firewall_match() {
        use crate::oxm::ct_state;
        // Test a typical stateful firewall match pattern
        let m = Match::new()
            .eth_type(0x0800)
            .ct_state(ct_state::TRK | ct_state::EST);

        let bytes = m.encode();
        let (decoded, _) = Match::decode(&bytes).unwrap();

        assert_eq!(decoded.eth_type, Some(0x0800));
        assert_eq!(decoded.ct_state, Some(ct_state::TRK | ct_state::EST));
    }
}
