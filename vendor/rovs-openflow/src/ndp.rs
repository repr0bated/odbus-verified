//! NDP (Neighbor Discovery Protocol) packet parsing and construction.
//!
//! This module provides utilities for parsing ICMPv6 Neighbor Solicitation
//! messages and constructing Neighbor Advertisement replies, enabling
//! NDP proxy functionality in OpenFlow controllers.

use std::net::Ipv6Addr;

/// Ethernet header size.
pub const ETH_HEADER_LEN: usize = 14;
/// IPv6 header size (fixed part).
pub const IPV6_HEADER_LEN: usize = 40;
/// ICMPv6 header size (type + code + checksum).
pub const ICMPV6_HEADER_LEN: usize = 4;
/// Neighbor Solicitation/Advertisement body size (reserved + target).
pub const ND_BODY_LEN: usize = 20;

/// Ethertype for IPv6.
pub const ETHERTYPE_IPV6: u16 = 0x86dd;
/// IPv6 next header value for ICMPv6.
pub const IPPROTO_ICMPV6: u8 = 58;
/// ICMPv6 type for Neighbor Solicitation.
pub const ICMPV6_NEIGHBOR_SOLICITATION: u8 = 135;
/// ICMPv6 type for Neighbor Advertisement.
pub const ICMPV6_NEIGHBOR_ADVERTISEMENT: u8 = 136;
/// NDP option type for Source Link-Layer Address.
pub const NDP_OPT_SOURCE_LL_ADDR: u8 = 1;
/// NDP option type for Target Link-Layer Address.
pub const NDP_OPT_TARGET_LL_ADDR: u8 = 2;

/// Parsed Ethernet frame.
#[derive(Debug, Clone)]
pub struct EthernetFrame {
    /// Destination MAC address.
    pub dst_mac: [u8; 6],
    /// Source MAC address.
    pub src_mac: [u8; 6],
    /// Ethertype.
    pub ethertype: u16,
    /// Payload offset in original packet.
    pub payload_offset: usize,
}

impl EthernetFrame {
    /// Parse an Ethernet frame header.
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < ETH_HEADER_LEN {
            return None;
        }

        let dst_mac: [u8; 6] = data[0..6].try_into().ok()?;
        let src_mac: [u8; 6] = data[6..12].try_into().ok()?;
        let ethertype = u16::from_be_bytes([data[12], data[13]]);

        Some(Self {
            dst_mac,
            src_mac,
            ethertype,
            payload_offset: ETH_HEADER_LEN,
        })
    }

    /// Encode an Ethernet frame header.
    pub fn encode(&self, buf: &mut Vec<u8>) {
        buf.extend_from_slice(&self.dst_mac);
        buf.extend_from_slice(&self.src_mac);
        buf.extend_from_slice(&self.ethertype.to_be_bytes());
    }
}

/// Parsed IPv6 header.
#[derive(Debug, Clone)]
pub struct Ipv6Header {
    /// Traffic class.
    pub traffic_class: u8,
    /// Flow label.
    pub flow_label: u32,
    /// Payload length.
    pub payload_len: u16,
    /// Next header (protocol).
    pub next_header: u8,
    /// Hop limit.
    pub hop_limit: u8,
    /// Source address.
    pub src_addr: Ipv6Addr,
    /// Destination address.
    pub dst_addr: Ipv6Addr,
}

impl Ipv6Header {
    /// Parse an IPv6 header.
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < IPV6_HEADER_LEN {
            return None;
        }

        // Version (4 bits) + Traffic Class (8 bits) + Flow Label (20 bits)
        let version = (data[0] >> 4) & 0x0f;
        if version != 6 {
            return None;
        }

        let traffic_class = ((data[0] & 0x0f) << 4) | ((data[1] >> 4) & 0x0f);
        let flow_label =
            ((data[1] as u32 & 0x0f) << 16) | ((data[2] as u32) << 8) | (data[3] as u32);
        let payload_len = u16::from_be_bytes([data[4], data[5]]);
        let next_header = data[6];
        let hop_limit = data[7];

        let src_bytes: [u8; 16] = data[8..24].try_into().ok()?;
        let dst_bytes: [u8; 16] = data[24..40].try_into().ok()?;

        Some(Self {
            traffic_class,
            flow_label,
            payload_len,
            next_header,
            hop_limit,
            src_addr: Ipv6Addr::from(src_bytes),
            dst_addr: Ipv6Addr::from(dst_bytes),
        })
    }

    /// Encode an IPv6 header.
    pub fn encode(&self, buf: &mut Vec<u8>) {
        // Version (6) + Traffic Class high 4 bits
        buf.push(0x60 | ((self.traffic_class >> 4) & 0x0f));
        // Traffic Class low 4 bits + Flow Label high 4 bits
        buf.push(((self.traffic_class & 0x0f) << 4) | ((self.flow_label >> 16) as u8 & 0x0f));
        // Flow Label middle 8 bits
        buf.push((self.flow_label >> 8) as u8);
        // Flow Label low 8 bits
        buf.push(self.flow_label as u8);
        // Payload length
        buf.extend_from_slice(&self.payload_len.to_be_bytes());
        // Next header
        buf.push(self.next_header);
        // Hop limit
        buf.push(self.hop_limit);
        // Source address
        buf.extend_from_slice(&self.src_addr.octets());
        // Destination address
        buf.extend_from_slice(&self.dst_addr.octets());
    }
}

/// Parsed ICMPv6 Neighbor Solicitation.
#[derive(Debug, Clone)]
pub struct NeighborSolicitation {
    /// Target IPv6 address being queried.
    pub target_addr: Ipv6Addr,
    /// Source link-layer address option (if present).
    pub source_ll_addr: Option<[u8; 6]>,
}

impl NeighborSolicitation {
    /// Parse an ICMPv6 Neighbor Solicitation from the ICMPv6 payload.
    /// Expects data starting at the ICMPv6 header (type, code, checksum).
    pub fn parse(data: &[u8]) -> Option<Self> {
        // Minimum: ICMPv6 header (4) + reserved (4) + target (16) = 24 bytes
        if data.len() < ICMPV6_HEADER_LEN + ND_BODY_LEN {
            return None;
        }

        let icmp_type = data[0];
        if icmp_type != ICMPV6_NEIGHBOR_SOLICITATION {
            return None;
        }

        // Skip: type(1) + code(1) + checksum(2) + reserved(4) = 8 bytes
        let target_bytes: [u8; 16] = data[8..24].try_into().ok()?;
        let target_addr = Ipv6Addr::from(target_bytes);

        // Parse options (if any)
        let mut source_ll_addr = None;
        let mut offset = 24;

        while offset + 2 <= data.len() {
            let opt_type = data[offset];
            let opt_len = data[offset + 1] as usize * 8; // Length in units of 8 bytes

            if opt_len == 0 {
                break; // Invalid option length
            }

            if offset + opt_len > data.len() {
                break; // Truncated option
            }

            if opt_type == NDP_OPT_SOURCE_LL_ADDR && opt_len >= 8 {
                // Source Link-Layer Address option
                source_ll_addr = Some(data[offset + 2..offset + 8].try_into().ok()?);
            }

            offset += opt_len;
        }

        Some(Self {
            target_addr,
            source_ll_addr,
        })
    }
}

/// Neighbor Advertisement builder.
#[derive(Debug, Clone)]
pub struct NeighborAdvertisement {
    /// Target IPv6 address.
    pub target_addr: Ipv6Addr,
    /// Target link-layer address to include in option.
    pub target_ll_addr: [u8; 6],
    /// Router flag.
    pub is_router: bool,
    /// Solicited flag (response to NS).
    pub is_solicited: bool,
    /// Override flag.
    pub is_override: bool,
}

impl NeighborAdvertisement {
    /// Create a new Neighbor Advertisement.
    pub fn new(target_addr: Ipv6Addr, target_ll_addr: [u8; 6]) -> Self {
        Self {
            target_addr,
            target_ll_addr,
            is_router: false,
            is_solicited: true,
            is_override: true,
        }
    }

    /// Set the router flag.
    pub fn router(mut self, is_router: bool) -> Self {
        self.is_router = is_router;
        self
    }

    /// Set the solicited flag.
    pub fn solicited(mut self, is_solicited: bool) -> Self {
        self.is_solicited = is_solicited;
        self
    }

    /// Set the override flag.
    pub fn override_flag(mut self, is_override: bool) -> Self {
        self.is_override = is_override;
        self
    }

    /// Encode the ICMPv6 Neighbor Advertisement (without checksum).
    /// Returns the ICMPv6 message body that needs checksum calculation.
    pub fn encode_icmpv6(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(32);

        // ICMPv6 type
        buf.push(ICMPV6_NEIGHBOR_ADVERTISEMENT);
        // Code
        buf.push(0);
        // Checksum placeholder (will be filled in later)
        buf.push(0);
        buf.push(0);

        // Flags: R(1) + S(1) + O(1) + reserved(29 bits)
        let mut flags: u32 = 0;
        if self.is_router {
            flags |= 0x8000_0000;
        }
        if self.is_solicited {
            flags |= 0x4000_0000;
        }
        if self.is_override {
            flags |= 0x2000_0000;
        }
        buf.extend_from_slice(&flags.to_be_bytes());

        // Target address
        buf.extend_from_slice(&self.target_addr.octets());

        // Target Link-Layer Address option
        buf.push(NDP_OPT_TARGET_LL_ADDR); // Type
        buf.push(1); // Length (in units of 8 bytes)
        buf.extend_from_slice(&self.target_ll_addr);

        buf
    }
}

/// Calculate ICMPv6 checksum.
///
/// The checksum is computed over a pseudo-header and the ICMPv6 message.
pub fn icmpv6_checksum(src: &Ipv6Addr, dst: &Ipv6Addr, icmpv6_data: &[u8]) -> u16 {
    let mut sum: u32 = 0;

    // Pseudo-header
    for chunk in src.octets().chunks(2) {
        sum += u16::from_be_bytes([chunk[0], chunk[1]]) as u32;
    }
    for chunk in dst.octets().chunks(2) {
        sum += u16::from_be_bytes([chunk[0], chunk[1]]) as u32;
    }
    // Upper-layer packet length
    sum += icmpv6_data.len() as u32;
    // Next header (ICMPv6 = 58)
    sum += IPPROTO_ICMPV6 as u32;

    // ICMPv6 data
    let mut i = 0;
    while i + 1 < icmpv6_data.len() {
        sum += u16::from_be_bytes([icmpv6_data[i], icmpv6_data[i + 1]]) as u32;
        i += 2;
    }
    if i < icmpv6_data.len() {
        sum += (icmpv6_data[i] as u32) << 8;
    }

    // Fold 32-bit sum to 16 bits
    while sum >> 16 != 0 {
        sum = (sum & 0xffff) + (sum >> 16);
    }

    !sum as u16
}

/// Build a complete Neighbor Advertisement reply packet.
///
/// Given a parsed Neighbor Solicitation and the MAC/IPv6 to advertise,
/// constructs a complete Ethernet + IPv6 + ICMPv6 NA packet.
#[allow(clippy::missing_panics_doc)]
pub fn build_na_reply(
    ns_eth: &EthernetFrame,
    ns_ipv6: &Ipv6Header,
    ns: &NeighborSolicitation,
    our_mac: [u8; 6],
    our_ipv6: Ipv6Addr,
) -> Vec<u8> {
    // Determine destination: unicast to NS sender, or multicast if unspecified
    let dst_mac = ns.source_ll_addr.unwrap_or(ns_eth.src_mac);
    let dst_ipv6 = if ns_ipv6.src_addr.is_unspecified() {
        // All-nodes multicast
        "ff02::1".parse().unwrap()
    } else {
        ns_ipv6.src_addr
    };

    // Build NA
    let na = NeighborAdvertisement::new(our_ipv6, our_mac)
        .solicited(!ns_ipv6.src_addr.is_unspecified());

    let mut icmpv6_data = na.encode_icmpv6();

    // Calculate and insert checksum
    let checksum = icmpv6_checksum(&our_ipv6, &dst_ipv6, &icmpv6_data);
    icmpv6_data[2] = (checksum >> 8) as u8;
    icmpv6_data[3] = checksum as u8;

    // Build IPv6 header
    let ipv6 = Ipv6Header {
        traffic_class: 0,
        flow_label: 0,
        payload_len: icmpv6_data.len() as u16,
        next_header: IPPROTO_ICMPV6,
        hop_limit: 255,
        src_addr: our_ipv6,
        dst_addr: dst_ipv6,
    };

    // Build Ethernet header
    let eth = EthernetFrame {
        dst_mac,
        src_mac: our_mac,
        ethertype: ETHERTYPE_IPV6,
        payload_offset: 0,
    };

    // Assemble packet
    let mut packet = Vec::with_capacity(ETH_HEADER_LEN + IPV6_HEADER_LEN + icmpv6_data.len());
    eth.encode(&mut packet);
    ipv6.encode(&mut packet);
    packet.extend_from_slice(&icmpv6_data);

    packet
}

/// Parse a potential NDP Neighbor Solicitation from raw packet data.
///
/// Returns parsed components if the packet is a valid NS, None otherwise.
pub fn parse_neighbor_solicitation(
    data: &[u8],
) -> Option<(EthernetFrame, Ipv6Header, NeighborSolicitation)> {
    let eth = EthernetFrame::parse(data)?;
    if eth.ethertype != ETHERTYPE_IPV6 {
        return None;
    }

    let ipv6 = Ipv6Header::parse(&data[eth.payload_offset..])?;
    if ipv6.next_header != IPPROTO_ICMPV6 {
        return None;
    }

    let icmpv6_offset = eth.payload_offset + IPV6_HEADER_LEN;
    let ns = NeighborSolicitation::parse(&data[icmpv6_offset..])?;

    Some((eth, ipv6, ns))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_ethernet_frame() {
        let data = [
            0x33, 0x33, 0xff, 0x00, 0x01, 0x00, // dst MAC (solicited-node multicast)
            0x02, 0x00, 0x00, 0x00, 0x01, 0x00, // src MAC
            0x86, 0xdd, // ethertype (IPv6)
            0x00, // payload start
        ];

        let eth = EthernetFrame::parse(&data).unwrap();
        assert_eq!(eth.ethertype, ETHERTYPE_IPV6);
        assert_eq!(eth.src_mac, [0x02, 0x00, 0x00, 0x00, 0x01, 0x00]);
    }

    #[test]
    fn icmpv6_checksum_calculation() {
        // Simple test vector
        let src: Ipv6Addr = "fe80::1".parse().unwrap();
        let dst: Ipv6Addr = "fe80::2".parse().unwrap();
        let data = [136, 0, 0, 0, 0x60, 0, 0, 0]; // NA with flags

        let cksum = icmpv6_checksum(&src, &dst, &data);
        // Just verify it produces a non-zero result
        assert_ne!(cksum, 0);
    }

    #[test]
    fn build_neighbor_advertisement() {
        let na = NeighborAdvertisement::new(
            "fd00::100".parse().unwrap(),
            [0x02, 0x00, 0x00, 0x00, 0x99, 0x00],
        );

        let icmpv6 = na.encode_icmpv6();
        assert_eq!(icmpv6[0], ICMPV6_NEIGHBOR_ADVERTISEMENT);
        assert_eq!(icmpv6.len(), 32); // 4 header + 4 flags + 16 target + 8 option
    }
}
