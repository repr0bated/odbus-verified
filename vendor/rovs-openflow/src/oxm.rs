//! `OpenFlow` Extensible Match (OXM) encoding.
//!
//! OXM is used in `OpenFlow` 1.2+ for flexible match field encoding.
//!
//! # Wire Format
//!
//! The OXM header is 4 bytes in network byte order:
//! ```text
//!  0                   1                   2                   3
//!  0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
//! +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//! |           class             |field|M|        length           |
//! +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//! ```
//!
//! - `class`: 16 bits - OXM class (e.g., OpenFlow Basic = 0x8000)
//! - `field`: 7 bits - Field identifier within the class
//! - `M` (hasmask): 1 bit - Whether a mask follows the value
//! - `length`: 8 bits - Length of value bytes (doubled if masked)

/// OXM class identifiers.
#[allow(dead_code)] // Reserved for future OpenFlow implementation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum OxmClass {
    /// Basic OpenFlow match fields
    OpenflowBasic = 0x8000,
    /// Experimenter match fields
    Experimenter = 0xffff,
    /// Nicira extensions
    Nxm0 = 0x0000,
    /// Nicira extensions (class 1)
    Nxm1 = 0x0001,
}

/// OXM field identifiers for `OpenFlow` Basic class.
#[allow(dead_code)] // Reserved for future OpenFlow implementation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum OxmField {
    /// Input port
    InPort = 0,
    /// Physical input port
    InPhyPort = 1,
    /// Metadata
    Metadata = 2,
    /// Ethernet destination
    EthDst = 3,
    /// Ethernet source
    EthSrc = 4,
    /// Ethernet type
    EthType = 5,
    /// VLAN ID
    VlanVid = 6,
    /// VLAN PCP
    VlanPcp = 7,
    /// IP DSCP
    IpDscp = 8,
    /// IP ECN
    IpEcn = 9,
    /// IP protocol
    IpProto = 10,
    /// IPv4 source
    Ipv4Src = 11,
    /// IPv4 destination
    Ipv4Dst = 12,
    /// TCP source port
    TcpSrc = 13,
    /// TCP destination port
    TcpDst = 14,
    /// Packet type (OF1.5): `(namespace, type)` pair, e.g. `(1, 0x800)`
    /// for plain IPv4 in the non-Ethernet namespace.
    PacketType = 44,
    /// UDP source port
    UdpSrc = 15,
    /// UDP destination port
    UdpDst = 16,
    /// SCTP source port
    SctpSrc = 17,
    /// SCTP destination port
    SctpDst = 18,
    /// ICMP type
    Icmpv4Type = 19,
    /// ICMP code
    Icmpv4Code = 20,
    /// ARP opcode
    ArpOp = 21,
    /// ARP source IPv4
    ArpSpa = 22,
    /// ARP target IPv4
    ArpTpa = 23,
    /// ARP source MAC
    ArpSha = 24,
    /// ARP target MAC
    ArpTha = 25,
    /// IPv6 source
    Ipv6Src = 26,
    /// IPv6 destination
    Ipv6Dst = 27,
    /// IPv6 flow label
    Ipv6Flabel = 28,
    /// ICMPv6 type
    Icmpv6Type = 29,
    /// ICMPv6 code
    Icmpv6Code = 30,
    /// Tunnel ID
    TunnelId = 38,
}

// =============================================================================
// NXM Field Identifiers (Phase 1.4)
// =============================================================================

/// NXM (Nicira Extended Match) field identifiers.
///
/// NXM fields use classes `Nxm0` (0x0000) for OpenFlow-compatible fields
/// and `Nxm1` (0x0001) for Nicira extensions.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum NxmField {
    // -------------------------------------------------------------------------
    // Registers (class 0x0001, fields 0-15, 4 bytes each)
    // -------------------------------------------------------------------------
    /// General purpose register 0
    Reg0 = 0,
    /// General purpose register 1
    Reg1 = 1,
    /// General purpose register 2
    Reg2 = 2,
    /// General purpose register 3
    Reg3 = 3,
    /// General purpose register 4
    Reg4 = 4,
    /// General purpose register 5
    Reg5 = 5,
    /// General purpose register 6
    Reg6 = 6,
    /// General purpose register 7
    Reg7 = 7,
    /// General purpose register 8
    Reg8 = 8,
    /// General purpose register 9
    Reg9 = 9,
    /// General purpose register 10
    Reg10 = 10,
    /// General purpose register 11
    Reg11 = 11,
    /// General purpose register 12
    Reg12 = 12,
    /// General purpose register 13
    Reg13 = 13,
    /// General purpose register 14
    Reg14 = 14,
    /// General purpose register 15
    Reg15 = 15,

    // -------------------------------------------------------------------------
    // Tunnel fields (class 0x0001)
    // -------------------------------------------------------------------------
    /// Tunnel ID (field 16, 8 bytes)
    TunId = 16,
    /// Tunnel IPv4 source address (field 31, 4 bytes)
    TunIpv4Src = 31,
    /// Tunnel IPv4 destination address (field 32, 4 bytes)
    TunIpv4Dst = 32,
    /// Packet mark (field 33, 4 bytes)
    PktMark = 33,

    // -------------------------------------------------------------------------
    // Connection tracking fields (class 0x0001)
    // -------------------------------------------------------------------------
    /// Connection tracking state (field 105, 4 bytes)
    CtState = 105,
    /// Connection tracking zone (field 106, 2 bytes)
    CtZone = 106,
    /// Connection tracking mark (field 107, 4 bytes)
    CtMark = 107,
    /// Connection tracking label (field 108, 16 bytes)
    CtLabel = 108,

    // -------------------------------------------------------------------------
    // Extended registers (class 0x0001, fields 111-114, 16 bytes each)
    // -------------------------------------------------------------------------
    /// 128-bit extended register 0
    XxReg0 = 111,
    /// 128-bit extended register 1
    XxReg1 = 112,
    /// 128-bit extended register 2
    XxReg2 = 113,
    /// 128-bit extended register 3
    XxReg3 = 114,
}

/// Connection tracking state flags for `NXM_NX_CT_STATE`.
///
/// These flags can be combined with bitwise OR.
#[allow(dead_code)]
pub mod ct_state {
    /// Packet is tracked (ct has been executed)
    pub const TRK: u32 = 0x20;
    /// New connection
    pub const NEW: u32 = 0x01;
    /// Established connection
    pub const EST: u32 = 0x02;
    /// Related connection (e.g., FTP data)
    pub const REL: u32 = 0x04;
    /// Reply direction
    pub const RPL: u32 = 0x08;
    /// Invalid connection
    pub const INV: u32 = 0x10;
    /// Source NAT applied
    pub const SNAT: u32 = 0x40;
    /// Destination NAT applied
    pub const DNAT: u32 = 0x80;
}

/// OXM header containing class, field, mask flag, and length.
///
/// This struct represents the 4-byte header that precedes OXM field values
/// in OpenFlow 1.2+ match encoding.
#[allow(dead_code)] // Foundation for OpenFlow wire encoding (Phase 1)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OxmHeader {
    /// OXM class (e.g., `OpenflowBasic`, `Nxm0`, `Nxm1`)
    pub class: OxmClass,
    /// Field identifier within the class (7 bits, 0-127)
    pub field: u8,
    /// Whether this field has a mask following the value
    pub has_mask: bool,
    /// Length of the value bytes (or value + mask if `has_mask` is true)
    pub length: u8,
}

#[allow(dead_code)] // Foundation for OpenFlow wire encoding (Phase 1)
impl OxmHeader {
    /// Create a new OXM header with raw field value.
    #[must_use]
    pub const fn new(class: OxmClass, field: u8, has_mask: bool, length: u8) -> Self {
        Self {
            class,
            field,
            has_mask,
            length,
        }
    }

    /// Create an OXM header for an OpenFlow Basic field.
    #[must_use]
    pub const fn openflow_basic(field: OxmField, has_mask: bool, length: u8) -> Self {
        Self {
            class: OxmClass::OpenflowBasic,
            field: field as u8,
            has_mask,
            length,
        }
    }

    /// Encode the header to 4 bytes in network byte order.
    ///
    /// # Wire Format
    ///
    /// ```text
    /// Byte 0: class high byte
    /// Byte 1: class low byte
    /// Byte 2: (field << 1) | has_mask
    /// Byte 3: length
    /// ```
    #[must_use]
    pub const fn encode(self) -> [u8; 4] {
        let class_val = self.class as u16;
        let field_and_mask = (self.field << 1) | (self.has_mask as u8);

        [
            (class_val >> 8) as u8, // class high byte
            class_val as u8,        // class low byte
            field_and_mask,         // field (7 bits) | hasmask (1 bit)
            self.length,            // length
        ]
    }

    /// Get the encoded header as a big-endian u32.
    #[must_use]
    pub const fn as_u32(self) -> u32 {
        let bytes = self.encode();
        u32::from_be_bytes(bytes)
    }
}

// =============================================================================
// Fixed-Size Field Encoding (Phase 1.2)
// =============================================================================

/// Encode a 1-byte OXM field value (e.g., `IpProto`, `IpDscp`, `IpEcn`).
///
/// Returns 5 bytes: 4-byte header + 1-byte value.
#[allow(dead_code)]
#[must_use]
pub fn encode_u8(class: OxmClass, field: u8, value: u8) -> Vec<u8> {
    let header = OxmHeader::new(class, field, false, 1);
    let mut buf = Vec::with_capacity(5);
    buf.extend_from_slice(&header.encode());
    buf.push(value);
    buf
}

/// Encode a 1-byte OpenFlow Basic field value.
#[allow(dead_code)]
#[must_use]
pub fn encode_u8_field(field: OxmField, value: u8) -> Vec<u8> {
    encode_u8(OxmClass::OpenflowBasic, field as u8, value)
}

/// Encode a 2-byte OXM field value (e.g., `EthType`, `TcpSrc`, `TcpDst`, `VlanVid`).
///
/// Returns 6 bytes: 4-byte header + 2-byte value (big-endian).
#[allow(dead_code)]
#[must_use]
pub fn encode_u16(class: OxmClass, field: u8, value: u16) -> Vec<u8> {
    let header = OxmHeader::new(class, field, false, 2);
    let mut buf = Vec::with_capacity(6);
    buf.extend_from_slice(&header.encode());
    buf.extend_from_slice(&value.to_be_bytes());
    buf
}

/// Encode a 2-byte OpenFlow Basic field value.
#[allow(dead_code)]
#[must_use]
pub fn encode_u16_field(field: OxmField, value: u16) -> Vec<u8> {
    encode_u16(OxmClass::OpenflowBasic, field as u8, value)
}

/// Encode a masked 2-byte OXM field value (e.g., `VlanVid` with mask).
///
/// Returns 8 bytes: 4-byte header + 2-byte value + 2-byte mask (big-endian).
#[allow(dead_code)]
#[must_use]
pub fn encode_u16_masked(class: OxmClass, field: u8, value: u16, mask: u16) -> Vec<u8> {
    let header = OxmHeader::new(class, field, true, 4);
    let mut buf = Vec::with_capacity(8);
    buf.extend_from_slice(&header.encode());
    buf.extend_from_slice(&value.to_be_bytes());
    buf.extend_from_slice(&mask.to_be_bytes());
    buf
}

/// Encode a 4-byte OXM field value (e.g., `InPort`, `Ipv4Src`, `Ipv4Dst`).
///
/// Returns 8 bytes: 4-byte header + 4-byte value (big-endian).
#[allow(dead_code)]
#[must_use]
pub fn encode_u32(class: OxmClass, field: u8, value: u32) -> Vec<u8> {
    let header = OxmHeader::new(class, field, false, 4);
    let mut buf = Vec::with_capacity(8);
    buf.extend_from_slice(&header.encode());
    buf.extend_from_slice(&value.to_be_bytes());
    buf
}

/// Encode a 4-byte OpenFlow Basic field value.
#[allow(dead_code)]
#[must_use]
pub fn encode_u32_field(field: OxmField, value: u32) -> Vec<u8> {
    encode_u32(OxmClass::OpenflowBasic, field as u8, value)
}

/// Encode an 8-byte OXM field value (e.g., `Metadata`, `TunnelId`).
///
/// Returns 12 bytes: 4-byte header + 8-byte value (big-endian).
#[allow(dead_code)]
#[must_use]
pub fn encode_u64(class: OxmClass, field: u8, value: u64) -> Vec<u8> {
    let header = OxmHeader::new(class, field, false, 8);
    let mut buf = Vec::with_capacity(12);
    buf.extend_from_slice(&header.encode());
    buf.extend_from_slice(&value.to_be_bytes());
    buf
}

/// Encode an 8-byte OpenFlow Basic field value.
#[allow(dead_code)]
#[must_use]
pub fn encode_u64_field(field: OxmField, value: u64) -> Vec<u8> {
    encode_u64(OxmClass::OpenflowBasic, field as u8, value)
}

/// Encode a 6-byte MAC address OXM field (e.g., `EthSrc`, `EthDst`).
///
/// Returns 10 bytes: 4-byte header + 6-byte MAC address.
#[allow(dead_code)]
#[must_use]
pub fn encode_mac(class: OxmClass, field: u8, value: [u8; 6]) -> Vec<u8> {
    let header = OxmHeader::new(class, field, false, 6);
    let mut buf = Vec::with_capacity(10);
    buf.extend_from_slice(&header.encode());
    buf.extend_from_slice(&value);
    buf
}

/// Encode a 6-byte MAC address OpenFlow Basic field.
#[allow(dead_code)]
#[must_use]
pub fn encode_mac_field(field: OxmField, value: [u8; 6]) -> Vec<u8> {
    encode_mac(OxmClass::OpenflowBasic, field as u8, value)
}

// =============================================================================
// Masked Field Encoding (Phase 1.3)
// =============================================================================

/// Encode a masked 4-byte OXM field value (e.g., `Ipv4Src/Dst` with subnet).
///
/// Returns 12 bytes: 4-byte header + 4-byte value + 4-byte mask.
/// The header length field is set to 8 (value + mask bytes).
#[allow(dead_code)]
#[must_use]
pub fn encode_u32_masked(class: OxmClass, field: u8, value: u32, mask: u32) -> Vec<u8> {
    let header = OxmHeader::new(class, field, true, 8);
    let mut buf = Vec::with_capacity(12);
    buf.extend_from_slice(&header.encode());
    buf.extend_from_slice(&value.to_be_bytes());
    buf.extend_from_slice(&mask.to_be_bytes());
    buf
}

/// Encode a masked 4-byte OpenFlow Basic field value.
#[allow(dead_code)]
#[must_use]
pub fn encode_u32_masked_field(field: OxmField, value: u32, mask: u32) -> Vec<u8> {
    encode_u32_masked(OxmClass::OpenflowBasic, field as u8, value, mask)
}

/// Encode a masked 6-byte MAC address OXM field.
///
/// Returns 16 bytes: 4-byte header + 6-byte MAC + 6-byte mask.
/// The header length field is set to 12 (value + mask bytes).
#[allow(dead_code)]
#[must_use]
pub fn encode_mac_masked(class: OxmClass, field: u8, value: [u8; 6], mask: [u8; 6]) -> Vec<u8> {
    let header = OxmHeader::new(class, field, true, 12);
    let mut buf = Vec::with_capacity(16);
    buf.extend_from_slice(&header.encode());
    buf.extend_from_slice(&value);
    buf.extend_from_slice(&mask);
    buf
}

/// Encode a masked 6-byte MAC address OpenFlow Basic field.
#[allow(dead_code)]
#[must_use]
pub fn encode_mac_masked_field(field: OxmField, value: [u8; 6], mask: [u8; 6]) -> Vec<u8> {
    encode_mac_masked(OxmClass::OpenflowBasic, field as u8, value, mask)
}

/// Encode a masked 8-byte OXM field value (e.g., `Metadata`, `TunnelId`).
///
/// Returns 20 bytes: 4-byte header + 8-byte value + 8-byte mask.
/// The header length field is set to 16 (value + mask bytes).
#[allow(dead_code)]
#[must_use]
pub fn encode_u64_masked(class: OxmClass, field: u8, value: u64, mask: u64) -> Vec<u8> {
    let header = OxmHeader::new(class, field, true, 16);
    let mut buf = Vec::with_capacity(20);
    buf.extend_from_slice(&header.encode());
    buf.extend_from_slice(&value.to_be_bytes());
    buf.extend_from_slice(&mask.to_be_bytes());
    buf
}

/// Encode a masked 8-byte OpenFlow Basic field value.
#[allow(dead_code)]
#[must_use]
pub fn encode_u64_masked_field(field: OxmField, value: u64, mask: u64) -> Vec<u8> {
    encode_u64_masked(OxmClass::OpenflowBasic, field as u8, value, mask)
}

// =============================================================================
// NXM Field Encoding (Phase 1.4)
// =============================================================================

/// Encode an NXM register value (REG0-REG15).
///
/// Returns 8 bytes: 4-byte header + 4-byte value.
/// All registers use class `Nxm1` (0x0001).
#[allow(dead_code, clippy::missing_panics_doc)]
#[must_use]
pub fn encode_reg(reg_num: u8, value: u32) -> Vec<u8> {
    assert!(reg_num <= 15, "Register number must be 0-15");
    encode_u32(OxmClass::Nxm1, reg_num, value)
}

/// Encode a masked NXM register value.
///
/// Returns 12 bytes: 4-byte header + 4-byte value + 4-byte mask.
#[allow(dead_code, clippy::missing_panics_doc)]
#[must_use]
pub fn encode_reg_masked(reg_num: u8, value: u32, mask: u32) -> Vec<u8> {
    assert!(reg_num <= 15, "Register number must be 0-15");
    encode_u32_masked(OxmClass::Nxm1, reg_num, value, mask)
}

/// Encode an NXM field value using the `NxmField` enum.
#[allow(dead_code)]
#[must_use]
pub fn encode_nxm_u32(field: NxmField, value: u32) -> Vec<u8> {
    encode_u32(OxmClass::Nxm1, field as u8, value)
}

/// Encode a masked NXM field value using the `NxmField` enum.
#[allow(dead_code)]
#[must_use]
pub fn encode_nxm_u32_masked(field: NxmField, value: u32, mask: u32) -> Vec<u8> {
    encode_u32_masked(OxmClass::Nxm1, field as u8, value, mask)
}

/// Encode NXM tunnel ID (`NXM_NX_TUN_ID`).
///
/// Returns 12 bytes: 4-byte header + 8-byte value.
#[allow(dead_code)]
#[must_use]
pub fn encode_tun_id(value: u64) -> Vec<u8> {
    encode_u64(OxmClass::Nxm1, NxmField::TunId as u8, value)
}

/// Encode masked NXM tunnel ID.
///
/// Returns 20 bytes: 4-byte header + 8-byte value + 8-byte mask.
#[allow(dead_code)]
#[must_use]
pub fn encode_tun_id_masked(value: u64, mask: u64) -> Vec<u8> {
    encode_u64_masked(OxmClass::Nxm1, NxmField::TunId as u8, value, mask)
}

/// Encode NXM tunnel IPv4 source address (`NXM_NX_TUN_IPV4_SRC`).
///
/// Returns 8 bytes: 4-byte header + 4-byte IPv4 address.
#[allow(dead_code)]
#[must_use]
pub fn encode_tun_ipv4_src(addr: u32) -> Vec<u8> {
    encode_u32(OxmClass::Nxm1, NxmField::TunIpv4Src as u8, addr)
}

/// Encode NXM tunnel IPv4 destination address (`NXM_NX_TUN_IPV4_DST`).
///
/// Returns 8 bytes: 4-byte header + 4-byte IPv4 address.
#[allow(dead_code)]
#[must_use]
pub fn encode_tun_ipv4_dst(addr: u32) -> Vec<u8> {
    encode_u32(OxmClass::Nxm1, NxmField::TunIpv4Dst as u8, addr)
}

/// Encode NXM packet mark (`NXM_NX_PKT_MARK`).
///
/// Returns 8 bytes: 4-byte header + 4-byte mark value.
#[allow(dead_code)]
#[must_use]
pub fn encode_pkt_mark(value: u32) -> Vec<u8> {
    encode_u32(OxmClass::Nxm1, NxmField::PktMark as u8, value)
}

/// Encode masked NXM packet mark.
///
/// Returns 12 bytes: 4-byte header + 4-byte value + 4-byte mask.
#[allow(dead_code)]
#[must_use]
pub fn encode_pkt_mark_masked(value: u32, mask: u32) -> Vec<u8> {
    encode_u32_masked(OxmClass::Nxm1, NxmField::PktMark as u8, value, mask)
}

/// Encode connection tracking state (`NXM_NX_CT_STATE`).
///
/// Use constants from `ct_state` module (e.g., `ct_state::TRK | ct_state::EST`).
///
/// Returns 8 bytes: 4-byte header + 4-byte state flags.
#[allow(dead_code)]
#[must_use]
pub fn encode_ct_state(state: u32) -> Vec<u8> {
    encode_u32(OxmClass::Nxm1, NxmField::CtState as u8, state)
}

/// Encode masked connection tracking state.
///
/// Returns 12 bytes: 4-byte header + 4-byte value + 4-byte mask.
#[allow(dead_code)]
#[must_use]
pub fn encode_ct_state_masked(state: u32, mask: u32) -> Vec<u8> {
    encode_u32_masked(OxmClass::Nxm1, NxmField::CtState as u8, state, mask)
}

/// Encode connection tracking zone (`NXM_NX_CT_ZONE`).
///
/// Returns 6 bytes: 4-byte header + 2-byte zone ID.
#[allow(dead_code)]
#[must_use]
pub fn encode_ct_zone(zone: u16) -> Vec<u8> {
    encode_u16(OxmClass::Nxm1, NxmField::CtZone as u8, zone)
}

/// Encode connection tracking mark (`NXM_NX_CT_MARK`).
///
/// Returns 8 bytes: 4-byte header + 4-byte mark value.
#[allow(dead_code)]
#[must_use]
pub fn encode_ct_mark(mark: u32) -> Vec<u8> {
    encode_u32(OxmClass::Nxm1, NxmField::CtMark as u8, mark)
}

/// Encode masked connection tracking mark.
///
/// Returns 12 bytes: 4-byte header + 4-byte value + 4-byte mask.
#[allow(dead_code, clippy::similar_names)]
#[must_use]
pub fn encode_ct_mark_masked(mark: u32, mask: u32) -> Vec<u8> {
    encode_u32_masked(OxmClass::Nxm1, NxmField::CtMark as u8, mark, mask)
}

/// Encode connection tracking label (`NXM_NX_CT_LABEL`).
///
/// Returns 20 bytes: 4-byte header + 16-byte label.
#[allow(dead_code)]
#[must_use]
pub fn encode_ct_label(label: u128) -> Vec<u8> {
    let header = OxmHeader::new(OxmClass::Nxm1, NxmField::CtLabel as u8, false, 16);
    let mut buf = Vec::with_capacity(20);
    buf.extend_from_slice(&header.encode());
    buf.extend_from_slice(&label.to_be_bytes());
    buf
}

/// Encode masked connection tracking label.
///
/// Returns 36 bytes: 4-byte header + 16-byte value + 16-byte mask.
#[allow(dead_code)]
#[must_use]
pub fn encode_ct_label_masked(label: u128, mask: u128) -> Vec<u8> {
    let header = OxmHeader::new(OxmClass::Nxm1, NxmField::CtLabel as u8, true, 32);
    let mut buf = Vec::with_capacity(36);
    buf.extend_from_slice(&header.encode());
    buf.extend_from_slice(&label.to_be_bytes());
    buf.extend_from_slice(&mask.to_be_bytes());
    buf
}

// =============================================================================
// Extended Register Encoding (Phase 1.5)
// =============================================================================

/// Encode a 128-bit extended register (`NXM_NX_XXREG0-3`).
///
/// Extended registers are 128-bit (16-byte) registers useful for storing
/// IPv6 addresses or large metadata values.
///
/// Returns 20 bytes: 4-byte header + 16-byte value.
///
/// # Panics
///
/// Panics if `reg_num` is greater than 3.
#[allow(dead_code)]
#[must_use]
pub fn encode_xxreg(reg_num: u8, value: u128) -> Vec<u8> {
    assert!(reg_num <= 3, "Extended register number must be 0-3");
    let field = NxmField::XxReg0 as u8 + reg_num;
    let header = OxmHeader::new(OxmClass::Nxm1, field, false, 16);
    let mut buf = Vec::with_capacity(20);
    buf.extend_from_slice(&header.encode());
    buf.extend_from_slice(&value.to_be_bytes());
    buf
}

/// Encode a masked 128-bit extended register.
///
/// Returns 36 bytes: 4-byte header + 16-byte value + 16-byte mask.
///
/// # Panics
///
/// Panics if `reg_num` is greater than 3.
#[allow(dead_code)]
#[must_use]
pub fn encode_xxreg_masked(reg_num: u8, value: u128, mask: u128) -> Vec<u8> {
    assert!(reg_num <= 3, "Extended register number must be 0-3");
    let field = NxmField::XxReg0 as u8 + reg_num;
    let header = OxmHeader::new(OxmClass::Nxm1, field, true, 32);
    let mut buf = Vec::with_capacity(36);
    buf.extend_from_slice(&header.encode());
    buf.extend_from_slice(&value.to_be_bytes());
    buf.extend_from_slice(&mask.to_be_bytes());
    buf
}

/// Encode an IPv6 address in an extended register.
///
/// This is a convenience function for storing IPv6 addresses in xxreg registers.
///
/// Returns 20 bytes: 4-byte header + 16-byte IPv6 address.
#[allow(dead_code)]
#[must_use]
pub fn encode_xxreg_ipv6(reg_num: u8, addr: std::net::Ipv6Addr) -> Vec<u8> {
    encode_xxreg(reg_num, u128::from(addr))
}

/// Encode a masked IPv6 address in an extended register.
///
/// Returns 36 bytes: 4-byte header + 16-byte address + 16-byte mask.
#[allow(dead_code)]
#[must_use]
pub fn encode_xxreg_ipv6_masked(reg_num: u8, addr: std::net::Ipv6Addr, mask: std::net::Ipv6Addr) -> Vec<u8> {
    encode_xxreg_masked(reg_num, u128::from(addr), u128::from(mask))
}

/// Convert an IPv6 prefix length to a 128-bit mask.
///
/// # Examples
///
/// ```ignore
/// use rovs_openflow::oxm::prefix_to_mask_v6;
///
/// assert_eq!(prefix_to_mask_v6(64), 0xffffffff_ffffffff_00000000_00000000);
/// assert_eq!(prefix_to_mask_v6(128), u128::MAX);
/// assert_eq!(prefix_to_mask_v6(0), 0);
/// ```
#[allow(dead_code)]
#[must_use]
pub const fn prefix_to_mask_v6(prefix_len: u8) -> u128 {
    if prefix_len == 0 {
        0
    } else if prefix_len >= 128 {
        u128::MAX
    } else {
        u128::MAX << (128 - prefix_len)
    }
}

// =============================================================================
// Helper Functions
// =============================================================================

/// Convert an IPv4 prefix length to a subnet mask.
///
/// # Examples
///
/// ```ignore
/// use rovs_openflow::oxm::prefix_to_mask;
///
/// assert_eq!(prefix_to_mask(24), 0xffffff00); // 255.255.255.0
/// assert_eq!(prefix_to_mask(16), 0xffff0000); // 255.255.0.0
/// assert_eq!(prefix_to_mask(32), 0xffffffff); // 255.255.255.255
/// assert_eq!(prefix_to_mask(0), 0x00000000);  // 0.0.0.0
/// ```
#[allow(dead_code)]
#[must_use]
pub const fn prefix_to_mask(prefix_len: u8) -> u32 {
    if prefix_len == 0 {
        0
    } else if prefix_len >= 32 {
        0xffff_ffff
    } else {
        // Shift 1s into the high bits
        0xffff_ffff << (32 - prefix_len)
    }
}

/// Encode an IPv4 address with prefix length as a masked OXM field.
///
/// Convenience function that combines address and prefix into value/mask.
///
/// # Examples
///
/// ```ignore
/// // Match 10.0.0.0/24
/// let bytes = encode_ipv4_prefix(OxmField::Ipv4Dst, 0x0a000000, 24);
/// ```
#[allow(dead_code)]
#[must_use]
pub fn encode_ipv4_prefix(field: OxmField, addr: u32, prefix_len: u8) -> Vec<u8> {
    let mask = prefix_to_mask(prefix_len);
    encode_u32_masked_field(field, addr & mask, mask)
}

// =============================================================================
// OxmEncode Trait (Phase 1.6)
// =============================================================================

/// A trait for types that can be encoded to OXM/NXM wire format.
///
/// This provides a unified interface for encoding match fields regardless
/// of whether they are OpenFlow Basic (OXM) or Nicira Extension (NXM) fields.
pub trait OxmEncode {
    /// Encode this field to OXM wire format.
    ///
    /// Returns a `Vec<u8>` containing the 4-byte OXM header followed by
    /// the field value (and mask, if applicable).
    fn encode(&self) -> Vec<u8>;
}

/// OpenFlow Basic match field with value.
///
/// This enum represents all standard OpenFlow 1.3+ match fields from the
/// `OFPXMC_OPENFLOW_BASIC` class (0x8000).
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OxmMatchField {
    /// Switch input port (4 bytes)
    InPort(u32),
    /// Switch physical input port (4 bytes)
    InPhyPort(u32),
    /// Metadata (8 bytes)
    Metadata(u64),
    /// Metadata with mask (8+8 bytes)
    MetadataMasked(u64, u64),
    /// Ethernet destination address (6 bytes)
    EthDst([u8; 6]),
    /// Ethernet destination with mask (6+6 bytes)
    EthDstMasked([u8; 6], [u8; 6]),
    /// Ethernet source address (6 bytes)
    EthSrc([u8; 6]),
    /// Ethernet source with mask (6+6 bytes)
    EthSrcMasked([u8; 6], [u8; 6]),
    /// Ethernet type (2 bytes)
    EthType(u16),
    /// VLAN ID (2 bytes)
    VlanVid(u16),
    /// VLAN ID with mask (2+2 bytes)
    VlanVidMasked(u16, u16),
    /// VLAN priority (1 byte)
    VlanPcp(u8),
    /// IP DSCP (1 byte)
    IpDscp(u8),
    /// IP ECN (1 byte)
    IpEcn(u8),
    /// IP protocol (1 byte)
    IpProto(u8),
    /// IPv4 source address (4 bytes)
    Ipv4Src(u32),
    /// IPv4 source with mask (4+4 bytes)
    Ipv4SrcMasked(u32, u32),
    /// IPv4 destination address (4 bytes)
    Ipv4Dst(u32),
    /// IPv4 destination with mask (4+4 bytes)
    Ipv4DstMasked(u32, u32),
    /// TCP source port (2 bytes)
    TcpSrc(u16),
    /// TCP destination port (2 bytes)
    TcpDst(u16),
    /// UDP source port (2 bytes)
    UdpSrc(u16),
    /// UDP destination port (2 bytes)
    UdpDst(u16),
    /// SCTP source port (2 bytes)
    SctpSrc(u16),
    /// SCTP destination port (2 bytes)
    SctpDst(u16),
    /// ICMPv4 type (1 byte)
    Icmpv4Type(u8),
    /// ICMPv4 code (1 byte)
    Icmpv4Code(u8),
    /// ARP opcode (2 bytes)
    ArpOp(u16),
    /// ARP source IPv4 (4 bytes)
    ArpSpa(u32),
    /// ARP source IPv4 with mask (4+4 bytes)
    ArpSpaMasked(u32, u32),
    /// ARP target IPv4 (4 bytes)
    ArpTpa(u32),
    /// ARP target IPv4 with mask (4+4 bytes)
    ArpTpaMasked(u32, u32),
    /// ARP source MAC (6 bytes)
    ArpSha([u8; 6]),
    /// ARP target MAC (6 bytes)
    ArpTha([u8; 6]),
    /// Tunnel ID (8 bytes)
    TunnelId(u64),
    /// Tunnel ID with mask (8+8 bytes)
    TunnelIdMasked(u64, u64),
}

#[allow(dead_code)]
impl OxmEncode for OxmMatchField {
    fn encode(&self) -> Vec<u8> {
        match self {
            Self::InPort(v) => encode_u32_field(OxmField::InPort, *v),
            Self::InPhyPort(v) => encode_u32_field(OxmField::InPhyPort, *v),
            Self::Metadata(v) => encode_u64_field(OxmField::Metadata, *v),
            Self::MetadataMasked(v, m) => encode_u64_masked_field(OxmField::Metadata, *v, *m),
            Self::EthDst(v) => encode_mac_field(OxmField::EthDst, *v),
            Self::EthDstMasked(v, m) => encode_mac_masked_field(OxmField::EthDst, *v, *m),
            Self::EthSrc(v) => encode_mac_field(OxmField::EthSrc, *v),
            Self::EthSrcMasked(v, m) => encode_mac_masked_field(OxmField::EthSrc, *v, *m),
            Self::EthType(v) => encode_u16_field(OxmField::EthType, *v),
            Self::VlanVid(v) => encode_u16_field(OxmField::VlanVid, *v),
            Self::VlanVidMasked(v, m) => {
                encode_u16_masked(OxmClass::OpenflowBasic, OxmField::VlanVid as u8, *v, *m)
            }
            Self::VlanPcp(v) => encode_u8_field(OxmField::VlanPcp, *v),
            Self::IpDscp(v) => encode_u8_field(OxmField::IpDscp, *v),
            Self::IpEcn(v) => encode_u8_field(OxmField::IpEcn, *v),
            Self::IpProto(v) => encode_u8_field(OxmField::IpProto, *v),
            Self::Ipv4Src(v) => encode_u32_field(OxmField::Ipv4Src, *v),
            Self::Ipv4SrcMasked(v, m) => encode_u32_masked_field(OxmField::Ipv4Src, *v, *m),
            Self::Ipv4Dst(v) => encode_u32_field(OxmField::Ipv4Dst, *v),
            Self::Ipv4DstMasked(v, m) => encode_u32_masked_field(OxmField::Ipv4Dst, *v, *m),
            Self::TcpSrc(v) => encode_u16_field(OxmField::TcpSrc, *v),
            Self::TcpDst(v) => encode_u16_field(OxmField::TcpDst, *v),
            Self::UdpSrc(v) => encode_u16_field(OxmField::UdpSrc, *v),
            Self::UdpDst(v) => encode_u16_field(OxmField::UdpDst, *v),
            Self::SctpSrc(v) => encode_u16_field(OxmField::SctpSrc, *v),
            Self::SctpDst(v) => encode_u16_field(OxmField::SctpDst, *v),
            Self::Icmpv4Type(v) => encode_u8_field(OxmField::Icmpv4Type, *v),
            Self::Icmpv4Code(v) => encode_u8_field(OxmField::Icmpv4Code, *v),
            Self::ArpOp(v) => encode_u16_field(OxmField::ArpOp, *v),
            Self::ArpSpa(v) => encode_u32_field(OxmField::ArpSpa, *v),
            Self::ArpSpaMasked(v, m) => encode_u32_masked_field(OxmField::ArpSpa, *v, *m),
            Self::ArpTpa(v) => encode_u32_field(OxmField::ArpTpa, *v),
            Self::ArpTpaMasked(v, m) => encode_u32_masked_field(OxmField::ArpTpa, *v, *m),
            Self::ArpSha(v) => encode_mac_field(OxmField::ArpSha, *v),
            Self::ArpTha(v) => encode_mac_field(OxmField::ArpTha, *v),
            Self::TunnelId(v) => encode_u64_field(OxmField::TunnelId, *v),
            Self::TunnelIdMasked(v, m) => encode_u64_masked_field(OxmField::TunnelId, *v, *m),
        }
    }
}

/// Nicira Extension (NXM) match field with value.
///
/// This enum represents Nicira-specific match fields from the `NXM_NX_*`
/// class (0x0001).
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NxmMatchField {
    /// General purpose register 0-15 (4 bytes)
    Reg(u8, u32),
    /// General purpose register with mask (4+4 bytes)
    RegMasked(u8, u32, u32),
    /// Extended 128-bit register 0-3 (16 bytes)
    XxReg(u8, u128),
    /// Extended register with mask (16+16 bytes)
    XxRegMasked(u8, u128, u128),
    /// Tunnel ID (8 bytes)
    TunId(u64),
    /// Tunnel ID with mask (8+8 bytes)
    TunIdMasked(u64, u64),
    /// Tunnel IPv4 source (4 bytes)
    TunIpv4Src(u32),
    /// Tunnel IPv4 destination (4 bytes)
    TunIpv4Dst(u32),
    /// Packet mark (4 bytes)
    PktMark(u32),
    /// Packet mark with mask (4+4 bytes)
    PktMarkMasked(u32, u32),
    /// Connection tracking state (4 bytes)
    CtState(u32),
    /// Connection tracking state with mask (4+4 bytes)
    CtStateMasked(u32, u32),
    /// Connection tracking zone (2 bytes)
    CtZone(u16),
    /// Connection tracking mark (4 bytes)
    CtMark(u32),
    /// Connection tracking mark with mask (4+4 bytes)
    CtMarkMasked(u32, u32),
    /// Connection tracking label (16 bytes)
    CtLabel(u128),
    /// Connection tracking label with mask (16+16 bytes)
    CtLabelMasked(u128, u128),
}

#[allow(dead_code)]
impl OxmEncode for NxmMatchField {
    fn encode(&self) -> Vec<u8> {
        match self {
            Self::Reg(n, v) => encode_reg(*n, *v),
            Self::RegMasked(n, v, m) => encode_reg_masked(*n, *v, *m),
            Self::XxReg(n, v) => encode_xxreg(*n, *v),
            Self::XxRegMasked(n, v, m) => encode_xxreg_masked(*n, *v, *m),
            Self::TunId(v) => encode_tun_id(*v),
            Self::TunIdMasked(v, m) => encode_tun_id_masked(*v, *m),
            Self::TunIpv4Src(v) => encode_tun_ipv4_src(*v),
            Self::TunIpv4Dst(v) => encode_tun_ipv4_dst(*v),
            Self::PktMark(v) => encode_pkt_mark(*v),
            Self::PktMarkMasked(v, m) => encode_pkt_mark_masked(*v, *m),
            Self::CtState(v) => encode_ct_state(*v),
            Self::CtStateMasked(v, m) => encode_ct_state_masked(*v, *m),
            Self::CtZone(v) => encode_ct_zone(*v),
            Self::CtMark(v) => encode_ct_mark(*v),
            Self::CtMarkMasked(v, m) => encode_ct_mark_masked(*v, *m),
            Self::CtLabel(v) => encode_ct_label(*v),
            Self::CtLabelMasked(v, m) => encode_ct_label_masked(*v, *m),
        }
    }
}

/// A unified match field that can be either OXM or NXM.
///
/// This enum provides a single type for all match fields, useful when
/// building match lists that may contain both standard and extension fields.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MatchField {
    /// OpenFlow Basic match field
    Oxm(OxmMatchField),
    /// Nicira Extension match field
    Nxm(NxmMatchField),
}

#[allow(dead_code)]
impl OxmEncode for MatchField {
    fn encode(&self) -> Vec<u8> {
        match self {
            Self::Oxm(f) => f.encode(),
            Self::Nxm(f) => f.encode(),
        }
    }
}

// Convenience From implementations
impl From<OxmMatchField> for MatchField {
    fn from(f: OxmMatchField) -> Self {
        Self::Oxm(f)
    }
}

impl From<NxmMatchField> for MatchField {
    fn from(f: NxmMatchField) -> Self {
        Self::Nxm(f)
    }
}

/// Build an OXM header.
///
/// Format: class (16 bits) | field (7 bits) | hasmask (1 bit) | length (8 bits)
#[allow(dead_code)] // Reserved for future OpenFlow implementation
pub fn oxm_header(class: OxmClass, field: OxmField, has_mask: bool, length: u8) -> u32 {
    let class_val = class as u32;
    let field_val = (field as u32) << 1;
    let mask_val = if has_mask { 1u32 } else { 0u32 };
    let len_val = length as u32;

    (class_val << 16) | (field_val << 8) | (mask_val << 8) | len_val
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oxm_header_in_port() {
        // InPort: class=0x8000, field=0, no mask, length=4
        let header = OxmHeader::openflow_basic(OxmField::InPort, false, 4);
        let bytes = header.encode();

        // Expected: 0x80000004
        // byte 0: 0x80 (class high)
        // byte 1: 0x00 (class low)
        // byte 2: (0 << 1) | 0 = 0x00
        // byte 3: 4
        assert_eq!(bytes, [0x80, 0x00, 0x00, 0x04]);
        assert_eq!(header.as_u32(), 0x8000_0004);
    }

    #[test]
    fn oxm_header_eth_type() {
        // EthType: class=0x8000, field=5, no mask, length=2
        let header = OxmHeader::openflow_basic(OxmField::EthType, false, 2);
        let bytes = header.encode();

        // byte 2: (5 << 1) | 0 = 0x0a
        assert_eq!(bytes, [0x80, 0x00, 0x0a, 0x02]);
        assert_eq!(header.as_u32(), 0x8000_0a02);
    }

    #[test]
    fn oxm_header_eth_dst_masked() {
        // EthDst with mask: class=0x8000, field=3, mask=true, length=12
        let header = OxmHeader::openflow_basic(OxmField::EthDst, true, 12);
        let bytes = header.encode();

        // byte 2: (3 << 1) | 1 = 0x07
        assert_eq!(bytes, [0x80, 0x00, 0x07, 0x0c]);
        assert_eq!(header.as_u32(), 0x8000_070c);
    }

    #[test]
    fn oxm_header_ipv4_src_no_mask() {
        // Ipv4Src: class=0x8000, field=11, no mask, length=4
        let header = OxmHeader::openflow_basic(OxmField::Ipv4Src, false, 4);
        let bytes = header.encode();

        // byte 2: (11 << 1) | 0 = 0x16
        assert_eq!(bytes, [0x80, 0x00, 0x16, 0x04]);
        assert_eq!(header.as_u32(), 0x8000_1604);
    }

    #[test]
    fn oxm_header_ipv4_src_masked() {
        // Ipv4Src with mask: class=0x8000, field=11, mask=true, length=8
        let header = OxmHeader::openflow_basic(OxmField::Ipv4Src, true, 8);
        let bytes = header.encode();

        // byte 2: (11 << 1) | 1 = 0x17
        assert_eq!(bytes, [0x80, 0x00, 0x17, 0x08]);
        assert_eq!(header.as_u32(), 0x8000_1708);
    }

    #[test]
    fn oxm_header_tcp_dst() {
        // TcpDst: class=0x8000, field=14, no mask, length=2
        let header = OxmHeader::openflow_basic(OxmField::TcpDst, false, 2);
        let bytes = header.encode();

        // byte 2: (14 << 1) | 0 = 0x1c
        assert_eq!(bytes, [0x80, 0x00, 0x1c, 0x02]);
    }

    #[test]
    fn oxm_header_metadata_masked() {
        // Metadata with mask: class=0x8000, field=2, mask=true, length=16
        let header = OxmHeader::openflow_basic(OxmField::Metadata, true, 16);
        let bytes = header.encode();

        // byte 2: (2 << 1) | 1 = 0x05
        assert_eq!(bytes, [0x80, 0x00, 0x05, 0x10]);
    }

    #[test]
    fn oxm_header_nxm_register() {
        // NXM register: class=0x0001 (Nxm1), field=0 (REG0), no mask, length=4
        let header = OxmHeader::new(OxmClass::Nxm1, 0, false, 4);
        let bytes = header.encode();

        assert_eq!(bytes, [0x00, 0x01, 0x00, 0x04]);
        assert_eq!(header.as_u32(), 0x0001_0004);
    }

    #[test]
    fn oxm_header_nxm_register_masked() {
        // NXM register with mask: class=0x0001, field=0 (REG0), mask=true, length=8
        let header = OxmHeader::new(OxmClass::Nxm1, 0, true, 8);
        let bytes = header.encode();

        // byte 2: (0 << 1) | 1 = 0x01
        assert_eq!(bytes, [0x00, 0x01, 0x01, 0x08]);
    }

    #[test]
    fn oxm_header_tunnel_id() {
        // TunnelId: class=0x8000, field=38, no mask, length=8
        let header = OxmHeader::openflow_basic(OxmField::TunnelId, false, 8);
        let bytes = header.encode();

        // byte 2: (38 << 1) | 0 = 0x4c
        assert_eq!(bytes, [0x80, 0x00, 0x4c, 0x08]);
    }

    #[test]
    fn oxm_header_compatible_with_legacy_function() {
        // Verify OxmHeader.as_u32() matches legacy oxm_header() function
        let header = OxmHeader::openflow_basic(OxmField::EthType, false, 2);
        let legacy = oxm_header(OxmClass::OpenflowBasic, OxmField::EthType, false, 2);

        assert_eq!(header.as_u32(), legacy);
    }

    // =========================================================================
    // Fixed-Size Encoding Tests (Phase 1.2)
    // =========================================================================

    #[test]
    fn encode_u8_ip_proto_tcp() {
        // IP protocol = 6 (TCP)
        // IpProto: field=10
        let bytes = encode_u8_field(OxmField::IpProto, 6);

        assert_eq!(bytes.len(), 5);
        // Header: class=0x8000, field=10, no mask, length=1
        // byte 2: (10 << 1) | 0 = 0x14
        assert_eq!(&bytes[0..4], &[0x80, 0x00, 0x14, 0x01]);
        // Value
        assert_eq!(bytes[4], 6);
    }

    #[test]
    fn encode_u8_ip_dscp() {
        // IP DSCP = 46 (EF/Expedited Forwarding)
        // IpDscp: field=8
        let bytes = encode_u8_field(OxmField::IpDscp, 46);

        assert_eq!(bytes.len(), 5);
        // byte 2: (8 << 1) | 0 = 0x10
        assert_eq!(&bytes[0..4], &[0x80, 0x00, 0x10, 0x01]);
        assert_eq!(bytes[4], 46);
    }

    #[test]
    fn encode_u16_eth_type_ipv4() {
        // EthType = 0x0800 (IPv4)
        // EthType: field=5
        let bytes = encode_u16_field(OxmField::EthType, 0x0800);

        assert_eq!(bytes.len(), 6);
        // byte 2: (5 << 1) | 0 = 0x0a
        assert_eq!(&bytes[0..4], &[0x80, 0x00, 0x0a, 0x02]);
        // Value in big-endian
        assert_eq!(&bytes[4..6], &[0x08, 0x00]);
    }

    #[test]
    fn encode_u16_tcp_dst_http() {
        // TCP dst port = 80 (HTTP)
        // TcpDst: field=14
        let bytes = encode_u16_field(OxmField::TcpDst, 80);

        assert_eq!(bytes.len(), 6);
        // byte 2: (14 << 1) | 0 = 0x1c
        assert_eq!(&bytes[0..4], &[0x80, 0x00, 0x1c, 0x02]);
        // 80 = 0x0050 in big-endian
        assert_eq!(&bytes[4..6], &[0x00, 0x50]);
    }

    #[test]
    fn encode_u16_vlan_vid() {
        // VLAN ID = 100
        // VlanVid: field=6
        let bytes = encode_u16_field(OxmField::VlanVid, 100);

        assert_eq!(bytes.len(), 6);
        // byte 2: (6 << 1) | 0 = 0x0c
        assert_eq!(&bytes[0..4], &[0x80, 0x00, 0x0c, 0x02]);
        // 100 = 0x0064 in big-endian
        assert_eq!(&bytes[4..6], &[0x00, 0x64]);
    }

    #[test]
    fn encode_u32_in_port() {
        // InPort = 1
        // InPort: field=0
        let bytes = encode_u32_field(OxmField::InPort, 1);

        assert_eq!(bytes.len(), 8);
        // byte 2: (0 << 1) | 0 = 0x00
        assert_eq!(&bytes[0..4], &[0x80, 0x00, 0x00, 0x04]);
        // 1 in big-endian
        assert_eq!(&bytes[4..8], &[0x00, 0x00, 0x00, 0x01]);
    }

    #[test]
    fn encode_u32_ipv4_src() {
        // IPv4 src = 10.0.0.1 = 0x0a000001
        // Ipv4Src: field=11
        let ip: u32 = (10 << 24) | (0 << 16) | (0 << 8) | 1;
        let bytes = encode_u32_field(OxmField::Ipv4Src, ip);

        assert_eq!(bytes.len(), 8);
        // byte 2: (11 << 1) | 0 = 0x16
        assert_eq!(&bytes[0..4], &[0x80, 0x00, 0x16, 0x04]);
        // IP address bytes
        assert_eq!(&bytes[4..8], &[10, 0, 0, 1]);
    }

    #[test]
    fn encode_u32_ipv4_dst() {
        // IPv4 dst = 192.168.1.1
        // Ipv4Dst: field=12
        let ip: u32 = (192 << 24) | (168 << 16) | (1 << 8) | 1;
        let bytes = encode_u32_field(OxmField::Ipv4Dst, ip);

        assert_eq!(bytes.len(), 8);
        // byte 2: (12 << 1) | 0 = 0x18
        assert_eq!(&bytes[0..4], &[0x80, 0x00, 0x18, 0x04]);
        assert_eq!(&bytes[4..8], &[192, 168, 1, 1]);
    }

    #[test]
    fn encode_u64_metadata() {
        // Metadata = 0x123456789ABCDEF0
        // Metadata: field=2
        let bytes = encode_u64_field(OxmField::Metadata, 0x123456789ABCDEF0);

        assert_eq!(bytes.len(), 12);
        // byte 2: (2 << 1) | 0 = 0x04
        assert_eq!(&bytes[0..4], &[0x80, 0x00, 0x04, 0x08]);
        // Value in big-endian
        assert_eq!(
            &bytes[4..12],
            &[0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC, 0xDE, 0xF0]
        );
    }

    #[test]
    fn encode_u64_tunnel_id() {
        // TunnelId = 42
        // TunnelId: field=38
        let bytes = encode_u64_field(OxmField::TunnelId, 42);

        assert_eq!(bytes.len(), 12);
        // byte 2: (38 << 1) | 0 = 0x4c
        assert_eq!(&bytes[0..4], &[0x80, 0x00, 0x4c, 0x08]);
        // 42 in big-endian (8 bytes)
        assert_eq!(&bytes[4..12], &[0, 0, 0, 0, 0, 0, 0, 42]);
    }

    #[test]
    fn encode_mac_eth_src() {
        // EthSrc = 00:11:22:33:44:55
        // EthSrc: field=4
        let mac = [0x00, 0x11, 0x22, 0x33, 0x44, 0x55];
        let bytes = encode_mac_field(OxmField::EthSrc, mac);

        assert_eq!(bytes.len(), 10);
        // byte 2: (4 << 1) | 0 = 0x08
        assert_eq!(&bytes[0..4], &[0x80, 0x00, 0x08, 0x06]);
        assert_eq!(&bytes[4..10], &mac);
    }

    #[test]
    fn encode_mac_eth_dst() {
        // EthDst = ff:ff:ff:ff:ff:ff (broadcast)
        // EthDst: field=3
        let mac = [0xff, 0xff, 0xff, 0xff, 0xff, 0xff];
        let bytes = encode_mac_field(OxmField::EthDst, mac);

        assert_eq!(bytes.len(), 10);
        // byte 2: (3 << 1) | 0 = 0x06
        assert_eq!(&bytes[0..4], &[0x80, 0x00, 0x06, 0x06]);
        assert_eq!(&bytes[4..10], &mac);
    }

    #[test]
    fn encode_mac_arp_sha() {
        // ARP source hardware address
        // ArpSha: field=24
        let mac = [0xde, 0xad, 0xbe, 0xef, 0xca, 0xfe];
        let bytes = encode_mac_field(OxmField::ArpSha, mac);

        assert_eq!(bytes.len(), 10);
        // byte 2: (24 << 1) | 0 = 0x30
        assert_eq!(&bytes[0..4], &[0x80, 0x00, 0x30, 0x06]);
        assert_eq!(&bytes[4..10], &mac);
    }

    #[test]
    fn encode_nxm_register_raw() {
        // NXM_NX_REG0 (class=0x0001, field=0) with value 0x12345678
        let bytes = encode_u32(OxmClass::Nxm1, 0, 0x12345678);

        assert_eq!(bytes.len(), 8);
        assert_eq!(&bytes[0..4], &[0x00, 0x01, 0x00, 0x04]);
        assert_eq!(&bytes[4..8], &[0x12, 0x34, 0x56, 0x78]);
    }

    // =========================================================================
    // Masked Field Encoding Tests (Phase 1.3)
    // =========================================================================

    #[test]
    fn prefix_to_mask_24() {
        // /24 = 255.255.255.0 = 0xffffff00
        assert_eq!(prefix_to_mask(24), 0xffff_ff00);
    }

    #[test]
    fn prefix_to_mask_16() {
        // /16 = 255.255.0.0 = 0xffff0000
        assert_eq!(prefix_to_mask(16), 0xffff_0000);
    }

    #[test]
    fn prefix_to_mask_8() {
        // /8 = 255.0.0.0 = 0xff000000
        assert_eq!(prefix_to_mask(8), 0xff00_0000);
    }

    #[test]
    fn prefix_to_mask_32() {
        // /32 = 255.255.255.255 (host route)
        assert_eq!(prefix_to_mask(32), 0xffff_ffff);
    }

    #[test]
    fn prefix_to_mask_0() {
        // /0 = 0.0.0.0 (default route)
        assert_eq!(prefix_to_mask(0), 0x0000_0000);
    }

    #[test]
    fn prefix_to_mask_various() {
        assert_eq!(prefix_to_mask(1), 0x8000_0000);
        assert_eq!(prefix_to_mask(25), 0xffff_ff80);
        assert_eq!(prefix_to_mask(30), 0xffff_fffc); // /30 point-to-point
        assert_eq!(prefix_to_mask(31), 0xffff_fffe); // /31 point-to-point (RFC 3021)
    }

    #[test]
    fn encode_u32_masked_ipv4_24() {
        // IPv4 dst = 10.0.0.0/24
        // Ipv4Dst: field=12
        let addr: u32 = 10 << 24; // 10.0.0.0
        let mask: u32 = 0xffff_ff00; // /24
        let bytes = encode_u32_masked_field(OxmField::Ipv4Dst, addr, mask);

        assert_eq!(bytes.len(), 12);
        // Header: class=0x8000, field=12, has_mask=1, length=8
        // byte 2: (12 << 1) | 1 = 0x19
        assert_eq!(&bytes[0..4], &[0x80, 0x00, 0x19, 0x08]);
        // Value: 10.0.0.0
        assert_eq!(&bytes[4..8], &[10, 0, 0, 0]);
        // Mask: 255.255.255.0
        assert_eq!(&bytes[8..12], &[255, 255, 255, 0]);
    }

    #[test]
    fn encode_u32_masked_ipv4_16() {
        // IPv4 src = 192.168.0.0/16
        // Ipv4Src: field=11
        let addr: u32 = (192 << 24) | (168 << 16); // 192.168.0.0
        let mask: u32 = 0xffff_0000; // /16
        let bytes = encode_u32_masked_field(OxmField::Ipv4Src, addr, mask);

        assert_eq!(bytes.len(), 12);
        // byte 2: (11 << 1) | 1 = 0x17
        assert_eq!(&bytes[0..4], &[0x80, 0x00, 0x17, 0x08]);
        assert_eq!(&bytes[4..8], &[192, 168, 0, 0]);
        assert_eq!(&bytes[8..12], &[255, 255, 0, 0]);
    }

    #[test]
    fn encode_ipv4_prefix_convenience() {
        // Test the convenience function
        // 10.0.0.0/24
        let addr: u32 = (10 << 24) | (0 << 16) | (0 << 8) | 1; // 10.0.0.1
        let bytes = encode_ipv4_prefix(OxmField::Ipv4Dst, addr, 24);

        assert_eq!(bytes.len(), 12);
        // Value should be masked: 10.0.0.1 & 255.255.255.0 = 10.0.0.0
        assert_eq!(&bytes[4..8], &[10, 0, 0, 0]);
        assert_eq!(&bytes[8..12], &[255, 255, 255, 0]);
    }

    #[test]
    fn encode_mac_masked_oui() {
        // Match by OUI (first 3 bytes)
        // EthSrc: field=4
        let mac = [0x00, 0x11, 0x22, 0x00, 0x00, 0x00];
        let mask = [0xff, 0xff, 0xff, 0x00, 0x00, 0x00];
        let bytes = encode_mac_masked_field(OxmField::EthSrc, mac, mask);

        assert_eq!(bytes.len(), 16);
        // Header: class=0x8000, field=4, has_mask=1, length=12
        // byte 2: (4 << 1) | 1 = 0x09
        assert_eq!(&bytes[0..4], &[0x80, 0x00, 0x09, 0x0c]);
        assert_eq!(&bytes[4..10], &mac);
        assert_eq!(&bytes[10..16], &mask);
    }

    #[test]
    fn encode_mac_masked_multicast() {
        // Match multicast bit (LSB of first byte = 1)
        // EthDst: field=3
        let mac = [0x01, 0x00, 0x00, 0x00, 0x00, 0x00];
        let mask = [0x01, 0x00, 0x00, 0x00, 0x00, 0x00];
        let bytes = encode_mac_masked_field(OxmField::EthDst, mac, mask);

        assert_eq!(bytes.len(), 16);
        // byte 2: (3 << 1) | 1 = 0x07
        assert_eq!(&bytes[0..4], &[0x80, 0x00, 0x07, 0x0c]);
        assert_eq!(&bytes[4..10], &mac);
        assert_eq!(&bytes[10..16], &mask);
    }

    #[test]
    fn encode_u64_masked_metadata() {
        // Metadata with partial mask
        // Metadata: field=2
        let value: u64 = 0x1234_0000_0000_0000;
        let mask: u64 = 0xffff_0000_0000_0000;
        let bytes = encode_u64_masked_field(OxmField::Metadata, value, mask);

        assert_eq!(bytes.len(), 20);
        // Header: class=0x8000, field=2, has_mask=1, length=16
        // byte 2: (2 << 1) | 1 = 0x05
        assert_eq!(&bytes[0..4], &[0x80, 0x00, 0x05, 0x10]);
        assert_eq!(&bytes[4..12], &[0x12, 0x34, 0, 0, 0, 0, 0, 0]);
        assert_eq!(&bytes[12..20], &[0xff, 0xff, 0, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn encode_nxm_register_masked() {
        // NXM_NX_REG0 with mask (match only high 16 bits)
        let bytes = encode_u32_masked(OxmClass::Nxm1, 0, 0xabcd_0000, 0xffff_0000);

        assert_eq!(bytes.len(), 12);
        // Header: class=0x0001, field=0, has_mask=1, length=8
        // byte 2: (0 << 1) | 1 = 0x01
        assert_eq!(&bytes[0..4], &[0x00, 0x01, 0x01, 0x08]);
        assert_eq!(&bytes[4..8], &[0xab, 0xcd, 0x00, 0x00]);
        assert_eq!(&bytes[8..12], &[0xff, 0xff, 0x00, 0x00]);
    }

    // =========================================================================
    // NXM Field Encoding Tests (Phase 1.4)
    // =========================================================================

    #[test]
    fn encode_reg_0() {
        // NXM_NX_REG0 = 0x12345678
        let bytes = encode_reg(0, 0x12345678);

        assert_eq!(bytes.len(), 8);
        // Header: class=0x0001, field=0, no mask, length=4
        assert_eq!(&bytes[0..4], &[0x00, 0x01, 0x00, 0x04]);
        assert_eq!(&bytes[4..8], &[0x12, 0x34, 0x56, 0x78]);
    }

    #[test]
    fn encode_reg_15() {
        // NXM_NX_REG15 = 0xdeadbeef
        let bytes = encode_reg(15, 0xdeadbeef);

        assert_eq!(bytes.len(), 8);
        // Header: class=0x0001, field=15, no mask, length=4
        // byte 2: (15 << 1) | 0 = 0x1e
        assert_eq!(&bytes[0..4], &[0x00, 0x01, 0x1e, 0x04]);
        assert_eq!(&bytes[4..8], &[0xde, 0xad, 0xbe, 0xef]);
    }

    #[test]
    fn encode_reg_masked_partial() {
        // NXM_NX_REG5 with mask (match only low 8 bits)
        let bytes = encode_reg_masked(5, 0x42, 0x000000ff);

        assert_eq!(bytes.len(), 12);
        // Header: class=0x0001, field=5, has_mask=1, length=8
        // byte 2: (5 << 1) | 1 = 0x0b
        assert_eq!(&bytes[0..4], &[0x00, 0x01, 0x0b, 0x08]);
        assert_eq!(&bytes[4..8], &[0x00, 0x00, 0x00, 0x42]);
        assert_eq!(&bytes[8..12], &[0x00, 0x00, 0x00, 0xff]);
    }

    #[test]
    fn encode_tun_id_value() {
        // NXM_NX_TUN_ID = 1000
        let bytes = encode_tun_id(1000);

        assert_eq!(bytes.len(), 12);
        // Header: class=0x0001, field=16, no mask, length=8
        // byte 2: (16 << 1) | 0 = 0x20
        assert_eq!(&bytes[0..4], &[0x00, 0x01, 0x20, 0x08]);
        // 1000 = 0x3e8 in big-endian 8 bytes
        assert_eq!(&bytes[4..12], &[0, 0, 0, 0, 0, 0, 0x03, 0xe8]);
    }

    #[test]
    fn encode_tun_id_masked_value() {
        // NXM_NX_TUN_ID with high 32-bit mask
        let bytes = encode_tun_id_masked(0x12345678_00000000, 0xffffffff_00000000);

        assert_eq!(bytes.len(), 20);
        // Header: class=0x0001, field=16, has_mask=1, length=16
        // byte 2: (16 << 1) | 1 = 0x21
        assert_eq!(&bytes[0..4], &[0x00, 0x01, 0x21, 0x10]);
    }

    #[test]
    fn encode_tun_ipv4_src_value() {
        // NXM_NX_TUN_IPV4_SRC = 10.0.0.1
        let addr: u32 = (10 << 24) | 1;
        let bytes = encode_tun_ipv4_src(addr);

        assert_eq!(bytes.len(), 8);
        // Header: class=0x0001, field=31, no mask, length=4
        // byte 2: (31 << 1) | 0 = 0x3e
        assert_eq!(&bytes[0..4], &[0x00, 0x01, 0x3e, 0x04]);
        assert_eq!(&bytes[4..8], &[10, 0, 0, 1]);
    }

    #[test]
    fn encode_tun_ipv4_dst_value() {
        // NXM_NX_TUN_IPV4_DST = 192.168.1.1
        let addr: u32 = (192 << 24) | (168 << 16) | (1 << 8) | 1;
        let bytes = encode_tun_ipv4_dst(addr);

        assert_eq!(bytes.len(), 8);
        // Header: class=0x0001, field=32, no mask, length=4
        // byte 2: (32 << 1) | 0 = 0x40
        assert_eq!(&bytes[0..4], &[0x00, 0x01, 0x40, 0x04]);
        assert_eq!(&bytes[4..8], &[192, 168, 1, 1]);
    }

    #[test]
    fn encode_pkt_mark_value() {
        // NXM_NX_PKT_MARK = 0x100
        let bytes = encode_pkt_mark(0x100);

        assert_eq!(bytes.len(), 8);
        // Header: class=0x0001, field=33, no mask, length=4
        // byte 2: (33 << 1) | 0 = 0x42
        assert_eq!(&bytes[0..4], &[0x00, 0x01, 0x42, 0x04]);
        assert_eq!(&bytes[4..8], &[0x00, 0x00, 0x01, 0x00]);
    }

    #[test]
    fn encode_pkt_mark_masked_value() {
        // NXM_NX_PKT_MARK with partial mask
        let bytes = encode_pkt_mark_masked(0xff00, 0xff00);

        assert_eq!(bytes.len(), 12);
        // byte 2: (33 << 1) | 1 = 0x43
        assert_eq!(&bytes[0..4], &[0x00, 0x01, 0x43, 0x08]);
    }

    #[test]
    fn encode_ct_state_tracked_established() {
        // ct_state=+trk+est
        let state = ct_state::TRK | ct_state::EST;
        let bytes = encode_ct_state(state);

        assert_eq!(bytes.len(), 8);
        // Header: class=0x0001, field=105, no mask, length=4
        // byte 2: (105 << 1) | 0 = 0xd2
        assert_eq!(&bytes[0..4], &[0x00, 0x01, 0xd2, 0x04]);
        // TRK (0x20) | EST (0x02) = 0x22
        assert_eq!(&bytes[4..8], &[0x00, 0x00, 0x00, 0x22]);
    }

    #[test]
    fn encode_ct_state_masked_new() {
        // ct_state=+trk+new with mask
        let state = ct_state::TRK | ct_state::NEW;
        let mask = ct_state::TRK | ct_state::NEW;
        let bytes = encode_ct_state_masked(state, mask);

        assert_eq!(bytes.len(), 12);
        // byte 2: (105 << 1) | 1 = 0xd3
        assert_eq!(&bytes[0..4], &[0x00, 0x01, 0xd3, 0x08]);
        // TRK (0x20) | NEW (0x01) = 0x21
        assert_eq!(&bytes[4..8], &[0x00, 0x00, 0x00, 0x21]);
        assert_eq!(&bytes[8..12], &[0x00, 0x00, 0x00, 0x21]);
    }

    #[test]
    fn encode_ct_zone_value() {
        // NXM_NX_CT_ZONE = 100
        let bytes = encode_ct_zone(100);

        assert_eq!(bytes.len(), 6);
        // Header: class=0x0001, field=106, no mask, length=2
        // byte 2: (106 << 1) | 0 = 0xd4
        assert_eq!(&bytes[0..4], &[0x00, 0x01, 0xd4, 0x02]);
        // 100 = 0x0064 in big-endian
        assert_eq!(&bytes[4..6], &[0x00, 0x64]);
    }

    #[test]
    fn encode_ct_mark_value() {
        // NXM_NX_CT_MARK = 0xaabbccdd
        let bytes = encode_ct_mark(0xaabbccdd);

        assert_eq!(bytes.len(), 8);
        // Header: class=0x0001, field=107, no mask, length=4
        // byte 2: (107 << 1) | 0 = 0xd6
        assert_eq!(&bytes[0..4], &[0x00, 0x01, 0xd6, 0x04]);
        assert_eq!(&bytes[4..8], &[0xaa, 0xbb, 0xcc, 0xdd]);
    }

    #[test]
    fn encode_ct_mark_masked_value() {
        // NXM_NX_CT_MARK with high byte mask
        let bytes = encode_ct_mark_masked(0xff000000, 0xff000000);

        assert_eq!(bytes.len(), 12);
        // byte 2: (107 << 1) | 1 = 0xd7
        assert_eq!(&bytes[0..4], &[0x00, 0x01, 0xd7, 0x08]);
        assert_eq!(&bytes[4..8], &[0xff, 0x00, 0x00, 0x00]);
        assert_eq!(&bytes[8..12], &[0xff, 0x00, 0x00, 0x00]);
    }

    #[test]
    fn encode_ct_label_value() {
        // NXM_NX_CT_LABEL = 0x12345678_9abcdef0_12345678_9abcdef0
        let label: u128 = 0x123456789abcdef0_123456789abcdef0;
        let bytes = encode_ct_label(label);

        assert_eq!(bytes.len(), 20);
        // Header: class=0x0001, field=108, no mask, length=16
        // byte 2: (108 << 1) | 0 = 0xd8
        assert_eq!(&bytes[0..4], &[0x00, 0x01, 0xd8, 0x10]);
        assert_eq!(
            &bytes[4..20],
            &[
                0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0, 0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc,
                0xde, 0xf0
            ]
        );
    }

    #[test]
    fn encode_ct_label_masked_value() {
        // NXM_NX_CT_LABEL with high 64-bit mask
        let label: u128 = 0xdeadbeef_00000000_00000000_00000000;
        let mask: u128 = 0xffffffff_00000000_00000000_00000000;
        let bytes = encode_ct_label_masked(label, mask);

        assert_eq!(bytes.len(), 36);
        // Header: class=0x0001, field=108, has_mask=1, length=32
        // byte 2: (108 << 1) | 1 = 0xd9
        assert_eq!(&bytes[0..4], &[0x00, 0x01, 0xd9, 0x20]);
    }

    #[test]
    fn encode_nxm_u32_via_enum() {
        // Test encode_nxm_u32 helper with NxmField enum
        let bytes = encode_nxm_u32(NxmField::PktMark, 0x42);

        assert_eq!(bytes.len(), 8);
        // Same as encode_pkt_mark
        assert_eq!(&bytes[0..4], &[0x00, 0x01, 0x42, 0x04]);
        assert_eq!(&bytes[4..8], &[0x00, 0x00, 0x00, 0x42]);
    }

    #[test]
    fn ct_state_flags_correct() {
        // Verify ct_state flag values match OVS definitions
        assert_eq!(ct_state::NEW, 0x01);
        assert_eq!(ct_state::EST, 0x02);
        assert_eq!(ct_state::REL, 0x04);
        assert_eq!(ct_state::RPL, 0x08);
        assert_eq!(ct_state::INV, 0x10);
        assert_eq!(ct_state::TRK, 0x20);
        assert_eq!(ct_state::SNAT, 0x40);
        assert_eq!(ct_state::DNAT, 0x80);
    }

    // =========================================================================
    // Extended Register Encoding Tests (Phase 1.5)
    // =========================================================================

    #[test]
    fn encode_xxreg_0() {
        // NXM_NX_XXREG0 = 0x0123456789abcdef_fedcba9876543210
        let value: u128 = 0x0123456789abcdef_fedcba9876543210;
        let bytes = encode_xxreg(0, value);

        assert_eq!(bytes.len(), 20);
        // Header: class=0x0001, field=111, no mask, length=16
        // byte 2: (111 << 1) | 0 = 0xde
        assert_eq!(&bytes[0..4], &[0x00, 0x01, 0xde, 0x10]);
        assert_eq!(
            &bytes[4..20],
            &[
                0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0xfe, 0xdc, 0xba, 0x98, 0x76, 0x54,
                0x32, 0x10
            ]
        );
    }

    #[test]
    fn encode_xxreg_3() {
        // NXM_NX_XXREG3
        let value: u128 = 0xdeadbeef_cafebabe_12345678_9abcdef0;
        let bytes = encode_xxreg(3, value);

        assert_eq!(bytes.len(), 20);
        // Header: class=0x0001, field=114, no mask, length=16
        // byte 2: (114 << 1) | 0 = 0xe4
        assert_eq!(&bytes[0..4], &[0x00, 0x01, 0xe4, 0x10]);
    }

    #[test]
    fn encode_xxreg_masked_high64() {
        // NXM_NX_XXREG1 with high 64-bit mask
        let value: u128 = 0xaabbccdd_eeff0011_00000000_00000000;
        let mask: u128 = 0xffffffff_ffffffff_00000000_00000000;
        let bytes = encode_xxreg_masked(1, value, mask);

        assert_eq!(bytes.len(), 36);
        // Header: class=0x0001, field=112, has_mask=1, length=32
        // byte 2: (112 << 1) | 1 = 0xe1
        assert_eq!(&bytes[0..4], &[0x00, 0x01, 0xe1, 0x20]);
    }

    #[test]
    fn encode_xxreg_ipv6_address() {
        use std::net::Ipv6Addr;

        // Store IPv6 address 2001:db8::1 in XXREG0
        let addr: Ipv6Addr = "2001:db8::1".parse().unwrap();
        let bytes = encode_xxreg_ipv6(0, addr);

        assert_eq!(bytes.len(), 20);
        // Header: field=111
        assert_eq!(&bytes[0..4], &[0x00, 0x01, 0xde, 0x10]);
        // IPv6 address bytes: 2001:0db8:0000:0000:0000:0000:0000:0001
        assert_eq!(
            &bytes[4..20],
            &[0x20, 0x01, 0x0d, 0xb8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x01]
        );
    }

    #[test]
    fn encode_xxreg_ipv6_masked_prefix() {
        use std::net::Ipv6Addr;

        // Store IPv6 prefix 2001:db8::/32 in XXREG2
        let addr: Ipv6Addr = "2001:db8::".parse().unwrap();
        let mask: Ipv6Addr = "ffff:ffff::".parse().unwrap();
        let bytes = encode_xxreg_ipv6_masked(2, addr, mask);

        assert_eq!(bytes.len(), 36);
        // Header: class=0x0001, field=113, has_mask=1, length=32
        // byte 2: (113 << 1) | 1 = 0xe3
        assert_eq!(&bytes[0..4], &[0x00, 0x01, 0xe3, 0x20]);
    }

    #[test]
    fn prefix_to_mask_v6_64() {
        // /64 prefix
        assert_eq!(
            prefix_to_mask_v6(64),
            0xffffffff_ffffffff_00000000_00000000
        );
    }

    #[test]
    fn prefix_to_mask_v6_128() {
        // /128 = full match
        assert_eq!(prefix_to_mask_v6(128), u128::MAX);
    }

    #[test]
    fn prefix_to_mask_v6_0() {
        // /0 = match all
        assert_eq!(prefix_to_mask_v6(0), 0);
    }

    #[test]
    fn prefix_to_mask_v6_various() {
        assert_eq!(prefix_to_mask_v6(32), 0xffffffff_00000000_00000000_00000000);
        assert_eq!(prefix_to_mask_v6(48), 0xffffffff_ffff0000_00000000_00000000);
        assert_eq!(
            prefix_to_mask_v6(96),
            0xffffffff_ffffffff_ffffffff_00000000
        );
        assert_eq!(
            prefix_to_mask_v6(120),
            0xffffffff_ffffffff_ffffffff_ffffff00
        );
    }

    #[test]
    fn xxreg_field_numbers() {
        // Verify field numbers for xxreg0-3
        assert_eq!(NxmField::XxReg0 as u8, 111);
        assert_eq!(NxmField::XxReg1 as u8, 112);
        assert_eq!(NxmField::XxReg2 as u8, 113);
        assert_eq!(NxmField::XxReg3 as u8, 114);
    }

    // =========================================================================
    // OxmEncode Trait Tests (Phase 1.6)
    // =========================================================================

    #[test]
    fn trait_oxm_in_port() {
        let field = OxmMatchField::InPort(1);
        let bytes = field.encode();

        // Should match encode_u32_field(OxmField::InPort, 1)
        assert_eq!(bytes, encode_u32_field(OxmField::InPort, 1));
        assert_eq!(bytes.len(), 8);
        assert_eq!(&bytes[0..4], &[0x80, 0x00, 0x00, 0x04]);
    }

    #[test]
    fn trait_oxm_eth_type() {
        let field = OxmMatchField::EthType(0x0800);
        let bytes = field.encode();

        assert_eq!(bytes, encode_u16_field(OxmField::EthType, 0x0800));
        assert_eq!(bytes.len(), 6);
    }

    #[test]
    fn trait_oxm_ipv4_src_masked() {
        // 10.0.0.0/24
        let field = OxmMatchField::Ipv4SrcMasked(0x0a000000, 0xffffff00);
        let bytes = field.encode();

        assert_eq!(
            bytes,
            encode_u32_masked_field(OxmField::Ipv4Src, 0x0a000000, 0xffffff00)
        );
        assert_eq!(bytes.len(), 12);
    }

    #[test]
    fn trait_oxm_eth_dst() {
        let mac = [0x00, 0x11, 0x22, 0x33, 0x44, 0x55];
        let field = OxmMatchField::EthDst(mac);
        let bytes = field.encode();

        assert_eq!(bytes, encode_mac_field(OxmField::EthDst, mac));
        assert_eq!(bytes.len(), 10);
    }

    #[test]
    fn trait_oxm_eth_src_masked() {
        // Match by OUI
        let mac = [0x00, 0x11, 0x22, 0x00, 0x00, 0x00];
        let mask = [0xff, 0xff, 0xff, 0x00, 0x00, 0x00];
        let field = OxmMatchField::EthSrcMasked(mac, mask);
        let bytes = field.encode();

        assert_eq!(bytes, encode_mac_masked_field(OxmField::EthSrc, mac, mask));
        assert_eq!(bytes.len(), 16);
    }

    #[test]
    fn trait_oxm_tcp_dst() {
        let field = OxmMatchField::TcpDst(80);
        let bytes = field.encode();

        assert_eq!(bytes, encode_u16_field(OxmField::TcpDst, 80));
    }

    #[test]
    fn trait_oxm_ip_proto() {
        let field = OxmMatchField::IpProto(6); // TCP
        let bytes = field.encode();

        assert_eq!(bytes, encode_u8_field(OxmField::IpProto, 6));
        assert_eq!(bytes.len(), 5);
    }

    #[test]
    fn trait_nxm_reg() {
        let field = NxmMatchField::Reg(0, 0x12345678);
        let bytes = field.encode();

        assert_eq!(bytes, encode_reg(0, 0x12345678));
        assert_eq!(bytes.len(), 8);
    }

    #[test]
    fn trait_nxm_reg_masked() {
        let field = NxmMatchField::RegMasked(5, 0xff00, 0xff00);
        let bytes = field.encode();

        assert_eq!(bytes, encode_reg_masked(5, 0xff00, 0xff00));
        assert_eq!(bytes.len(), 12);
    }

    #[test]
    fn trait_nxm_tun_id() {
        let field = NxmMatchField::TunId(1000);
        let bytes = field.encode();

        assert_eq!(bytes, encode_tun_id(1000));
        assert_eq!(bytes.len(), 12);
    }

    #[test]
    fn trait_nxm_ct_state() {
        let state = ct_state::TRK | ct_state::EST;
        let field = NxmMatchField::CtState(state);
        let bytes = field.encode();

        assert_eq!(bytes, encode_ct_state(state));
    }

    #[test]
    fn trait_nxm_ct_zone() {
        let field = NxmMatchField::CtZone(100);
        let bytes = field.encode();

        assert_eq!(bytes, encode_ct_zone(100));
        assert_eq!(bytes.len(), 6);
    }

    #[test]
    fn trait_nxm_xxreg() {
        let field = NxmMatchField::XxReg(0, 0x123456789abcdef0_fedcba9876543210);
        let bytes = field.encode();

        assert_eq!(
            bytes,
            encode_xxreg(0, 0x123456789abcdef0_fedcba9876543210)
        );
        assert_eq!(bytes.len(), 20);
    }

    #[test]
    fn trait_unified_match_field_oxm() {
        let field: MatchField = OxmMatchField::EthType(0x0800).into();
        let bytes = field.encode();

        assert_eq!(bytes, encode_u16_field(OxmField::EthType, 0x0800));
    }

    #[test]
    fn trait_unified_match_field_nxm() {
        let field: MatchField = NxmMatchField::Reg(0, 42).into();
        let bytes = field.encode();

        assert_eq!(bytes, encode_reg(0, 42));
    }

    #[test]
    fn trait_match_list_encoding() {
        // Test encoding a list of mixed fields
        let fields: Vec<MatchField> = vec![
            OxmMatchField::EthType(0x0800).into(),
            OxmMatchField::IpProto(6).into(),
            OxmMatchField::TcpDst(80).into(),
            NxmMatchField::Reg(0, 1).into(),
        ];

        let mut encoded = Vec::new();
        for field in &fields {
            encoded.extend(field.encode());
        }

        // Verify total length: 6 + 5 + 6 + 8 = 25 bytes
        assert_eq!(encoded.len(), 25);
    }

    #[test]
    fn trait_oxm_vlan_vid_masked() {
        // VLAN ID with CFI bit mask
        let field = OxmMatchField::VlanVidMasked(0x1064, 0x1fff); // VID=100 with CFI
        let bytes = field.encode();

        assert_eq!(bytes.len(), 8);
        // Header: field=6, has_mask=1, length=4
        // byte 2: (6 << 1) | 1 = 0x0d
        assert_eq!(&bytes[0..4], &[0x80, 0x00, 0x0d, 0x04]);
    }

    #[test]
    fn trait_oxm_metadata_masked() {
        let field = OxmMatchField::MetadataMasked(0xff00_0000_0000_0000, 0xff00_0000_0000_0000);
        let bytes = field.encode();

        assert_eq!(bytes.len(), 20);
    }

    #[test]
    fn trait_nxm_ct_label() {
        let label: u128 = 0x12345678_9abcdef0_12345678_9abcdef0;
        let field = NxmMatchField::CtLabel(label);
        let bytes = field.encode();

        assert_eq!(bytes, encode_ct_label(label));
        assert_eq!(bytes.len(), 20);
    }
}
