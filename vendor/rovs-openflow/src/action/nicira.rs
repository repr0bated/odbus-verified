//! Nicira extension actions for OpenFlow.
//!
//! This module contains Nicira (now part of VMware/OVS) vendor extensions
//! including learn, resubmit, connection tracking, move, and reg_load actions.

use super::types::{NxActionSubtype, NICIRA_VENDOR_ID, ActionType};

/// Learn action flags.
#[allow(dead_code)]
pub mod learn_flags {
    /// Send flow removed message when learned flow expires
    pub const SEND_FLOW_REM: u16 = 1 << 0;
    /// Delete matching flows instead of adding
    pub const DELETE_LEARNED: u16 = 1 << 1;
    /// Write result to the action set (vs apply immediately)
    pub const WRITE_RESULT: u16 = 1 << 2;
}

/// NxLearn action (Nicira extension).
///
/// The learn action creates flows dynamically based on packet content.
/// This is commonly used for MAC learning in OVS.
#[derive(Debug, Clone, Default)]
pub struct NxLearn {
    /// Idle timeout for learned flows (0 = no timeout)
    pub idle_timeout: u16,
    /// Hard timeout for learned flows (0 = no timeout)
    pub hard_timeout: u16,
    /// Priority of learned flows
    pub priority: u16,
    /// Cookie for learned flows
    pub cookie: u64,
    /// Learn flags
    pub flags: u16,
    /// Table to install learned flows
    pub table_id: u8,
    /// Idle timeout when FIN received
    pub fin_idle_timeout: u16,
    /// Hard timeout when FIN received
    pub fin_hard_timeout: u16,
    /// Flow modification specs (match and action specifications)
    pub specs: Vec<LearnSpec>,
}

/// A single learn specification.
///
/// Learn specs define how to construct match fields and actions
/// in the learned flow.
#[derive(Debug, Clone)]
pub enum LearnSpec {
    /// Match: copy field from packet to match field
    MatchField {
        /// Source field
        src_field: u32,
        /// Destination field (in learned flow's match)
        dst_field: u32,
        /// Number of bits
        n_bits: u16,
    },
    /// Match: use immediate value
    MatchImmediate {
        /// Destination field
        dst_field: u32,
        /// Value to match
        value: Vec<u8>,
        /// Number of bits
        n_bits: u16,
    },
    /// Action: copy field from packet to action's field
    LoadField {
        /// Source field
        src_field: u32,
        /// Destination field (in learned flow's actions)
        dst_field: u32,
        /// Number of bits
        n_bits: u16,
    },
    /// Action: load immediate value
    LoadImmediate {
        /// Destination field
        dst_field: u32,
        /// Value to load
        value: Vec<u8>,
        /// Number of bits
        n_bits: u16,
    },
    /// Output to port from field
    OutputField {
        /// Source field containing port number
        src_field: u32,
        /// Number of bits
        n_bits: u16,
    },
}

impl NxLearn {
    /// Create a new learn action with defaults.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set idle timeout for learned flows.
    pub fn idle_timeout(mut self, timeout: u16) -> Self {
        self.idle_timeout = timeout;
        self
    }

    /// Set hard timeout for learned flows.
    pub fn hard_timeout(mut self, timeout: u16) -> Self {
        self.hard_timeout = timeout;
        self
    }

    /// Set priority for learned flows.
    pub fn priority(mut self, priority: u16) -> Self {
        self.priority = priority;
        self
    }

    /// Set cookie for learned flows.
    pub fn cookie(mut self, cookie: u64) -> Self {
        self.cookie = cookie;
        self
    }

    /// Set table for learned flows.
    pub fn table(mut self, table_id: u8) -> Self {
        self.table_id = table_id;
        self
    }

    /// Set flags.
    pub fn flags(mut self, flags: u16) -> Self {
        self.flags = flags;
        self
    }

    /// Add a spec to match a field from the packet.
    pub fn match_field(mut self, src_field: u32, dst_field: u32, n_bits: u16) -> Self {
        self.specs.push(LearnSpec::MatchField { src_field, dst_field, n_bits });
        self
    }

    /// Add a spec to match an immediate value.
    pub fn match_immediate(mut self, dst_field: u32, value: Vec<u8>, n_bits: u16) -> Self {
        self.specs.push(LearnSpec::MatchImmediate { dst_field, value, n_bits });
        self
    }

    /// Add a spec to load a field from packet into action.
    pub fn load_field(mut self, src_field: u32, dst_field: u32, n_bits: u16) -> Self {
        self.specs.push(LearnSpec::LoadField { src_field, dst_field, n_bits });
        self
    }

    /// Add a spec to load an immediate value into action.
    pub fn load_immediate(mut self, dst_field: u32, value: Vec<u8>, n_bits: u16) -> Self {
        self.specs.push(LearnSpec::LoadImmediate { dst_field, value, n_bits });
        self
    }

    /// Add a spec to output to port from field.
    pub fn output_field(mut self, src_field: u32, n_bits: u16) -> Self {
        self.specs.push(LearnSpec::OutputField { src_field, n_bits });
        self
    }
}

// ============================================================================
// Nicira Action Encoding
// ============================================================================

/// Encode Nicira action header.
pub(crate) fn encode_nx_header(subtype: NxActionSubtype, len: u16) -> Vec<u8> {
    let mut buf = Vec::with_capacity(16);
    buf.extend((ActionType::Experimenter as u16).to_be_bytes());
    buf.extend(len.to_be_bytes());
    buf.extend(NICIRA_VENDOR_ID.to_be_bytes());
    buf.extend((subtype as u16).to_be_bytes());
    buf
}

/// Encode SetTunnelId as Nicira reg_load2 action (24 bytes).
pub(crate) fn encode_set_tunnel_id(tun_id: u64) -> Vec<u8> {
    // Use NXM reg_load2 (subtype 33) for setting tunnel ID
    // Format: NX header (10) + OXM header (4) + value (8) + pad (2) = 24 bytes
    let mut buf = encode_nx_header(NxActionSubtype::RegLoad2, 24);

    // OXM header for tun_id: NXM_NX_TUN_ID (class=1, field=16, len=8)
    let oxm_header: u32 = (1 << 16) | (16 << 9) | 8;
    buf.extend(oxm_header.to_be_bytes());
    buf.extend(tun_id.to_be_bytes());
    buf.extend([0u8; 2]); // padding to 24 bytes
    buf
}

/// Encode NxResubmit action (16 bytes for extended resubmit).
pub(crate) fn encode_nx_resubmit(in_port: Option<u16>, table: Option<u8>) -> Vec<u8> {
    // Use extended resubmit (subtype 14) which supports table
    let mut buf = encode_nx_header(NxActionSubtype::ResubmitTable, 16);
    buf.extend(in_port.unwrap_or(0xfff8).to_be_bytes()); // OFPP_IN_PORT = 0xfff8 (16-bit)
    buf.push(table.unwrap_or(255)); // 255 = current table
    buf.extend([0u8; 3]); // padding
    buf
}

/// Encode NxCt (connection tracking) action.
pub(crate) fn encode_nx_ct(flags: u16, zone: u16, table: Option<u8>) -> Vec<u8> {
    // CT action format (24 bytes minimum):
    // NX header (10) + flags (2) + zone_src (4) + zone (2) + recirc_table (1) + pad (3) + alg (2)
    let mut buf = encode_nx_header(NxActionSubtype::Ct, 24);
    buf.extend(flags.to_be_bytes());
    buf.extend(0u32.to_be_bytes()); // zone_src (0 = use zone_imm field)
    buf.extend(zone.to_be_bytes()); // zone_imm
    buf.push(table.unwrap_or(255)); // recirc_table (255 = no recirculation)
    buf.extend([0u8; 3]); // pad (3 bytes, not 1)
    buf.extend(0u16.to_be_bytes()); // alg (0 = no ALG)
    // No nested actions for now
    buf
}

/// Encode NxCt with NAT (connection tracking with NAT) action.
pub(crate) fn encode_nx_ct_nat(
    flags: u16,
    zone: u16,
    table: Option<u8>,
    nat: &super::NatConfig,
) -> Vec<u8> {
    // First encode the nested NAT action
    let nat_action = encode_nx_nat(nat);

    // CT action format with nested actions:
    // NX header (10) + flags (2) + zone_src (4) + zone (2) + recirc_table (1) + pad (1) + alg (2) + nested_actions
    // Note: The length includes the nested actions
    let ct_header_len = 24; // Base CT action size
    let total_len = ct_header_len + nat_action.len();
    // Round up to 8-byte boundary
    let padded_len = (total_len + 7) & !7;

    let mut buf = Vec::with_capacity(padded_len);

    // Action header
    buf.extend((ActionType::Experimenter as u16).to_be_bytes());
    buf.extend((padded_len as u16).to_be_bytes());
    buf.extend(NICIRA_VENDOR_ID.to_be_bytes());
    buf.extend((NxActionSubtype::Ct as u16).to_be_bytes());

    // CT fields
    buf.extend(flags.to_be_bytes());
    buf.extend(0u32.to_be_bytes()); // zone_src (0 = use zone_imm field)
    buf.extend(zone.to_be_bytes()); // zone_imm
    buf.push(table.unwrap_or(255)); // recirc_table (255 = no recirculation)
    buf.extend([0u8; 3]); // pad
    buf.extend(0u16.to_be_bytes()); // alg (0 = no ALG)

    // Nested NAT action
    buf.extend(nat_action);

    // Pad to 8-byte boundary
    buf.resize(padded_len, 0);
    buf
}

/// Encode NxNat action (used as nested action in ct).
///
/// NAT action format:
/// NX header (10) + pad (2) + flags (2) + range_present (2) + [optional fields]
fn encode_nx_nat(nat: &super::NatConfig) -> Vec<u8> {
    let range_present = nat.range_present();

    // Calculate the size of optional fields
    let mut optional_len = 0;
    if nat.ipv4_min.is_some() {
        optional_len += 4;
    }
    if nat.ipv4_max.is_some() {
        optional_len += 4;
    }
    if nat.ipv6_min.is_some() {
        optional_len += 16;
    }
    if nat.ipv6_max.is_some() {
        optional_len += 16;
    }
    if nat.port_min.is_some() {
        optional_len += 2;
    }
    if nat.port_max.is_some() {
        optional_len += 2;
    }

    // NAT header: 10 (NX header) + 2 (pad) + 2 (flags) + 2 (range_present) = 16
    let header_len = 16;
    let total_len = header_len + optional_len;
    // Round up to 8-byte boundary
    let padded_len = (total_len + 7) & !7;

    let mut buf = encode_nx_header(NxActionSubtype::Nat, padded_len as u16);
    buf.extend([0u8; 2]); // pad
    buf.extend(nat.flags.to_be_bytes());
    buf.extend(range_present.to_be_bytes());

    // Optional fields in order: ipv4_min, ipv4_max, ipv6_min, ipv6_max, port_min, port_max
    if let Some(addr) = nat.ipv4_min {
        buf.extend(addr.octets());
    }
    if let Some(addr) = nat.ipv4_max {
        buf.extend(addr.octets());
    }
    if let Some(addr) = nat.ipv6_min {
        buf.extend(addr.octets());
    }
    if let Some(addr) = nat.ipv6_max {
        buf.extend(addr.octets());
    }
    if let Some(port) = nat.port_min {
        buf.extend(port.to_be_bytes());
    }
    if let Some(port) = nat.port_max {
        buf.extend(port.to_be_bytes());
    }

    // Pad to 8-byte boundary
    buf.resize(padded_len, 0);
    buf
}

/// Encode NxRegLoad action for loading immediate value into register.
///
/// Format: `load:value->NXM_NX_REGn[start..end]`
#[allow(dead_code)]
pub fn encode_nx_reg_load(reg_num: u8, value: u32, start_bit: u8, n_bits: u8) -> Vec<u8> {
    // reg_load uses subtype 7
    // Format: NX header (10) + ofs_nbits (2) + dst (4) + value (8)
    let mut buf = encode_nx_header(NxActionSubtype::RegLoad, 24);

    // ofs_nbits: (start_bit << 6) | (n_bits - 1)
    let ofs_nbits = ((start_bit as u16) << 6) | ((n_bits - 1) as u16);
    buf.extend(ofs_nbits.to_be_bytes());

    // dst: NXM header for register (class=1, field=reg_num, len=4)
    let dst_header: u32 = (1 << 16) | ((reg_num as u32) << 9) | 4;
    buf.extend(dst_header.to_be_bytes());

    // value: 64-bit value (upper bits zero)
    buf.extend((value as u64).to_be_bytes());
    buf
}

/// Encode NxRegLoad action with NXM header for loading immediate value into any field.
///
/// This is the more general form that accepts any NXM field header.
/// Format: `load:value->NXM_field[ofs..ofs+n_bits]`
pub(crate) fn encode_nx_reg_load_nxm(dst_field: u32, dst_ofs: u16, n_bits: u16, value: u64) -> Vec<u8> {
    // reg_load uses subtype 7
    // Format: NX header (10) + ofs_nbits (2) + dst (4) + value (8)
    let mut buf = encode_nx_header(NxActionSubtype::RegLoad, 24);

    // ofs_nbits: (offset << 6) | (n_bits - 1)
    let ofs_nbits = (dst_ofs << 6) | (n_bits - 1);
    buf.extend(ofs_nbits.to_be_bytes());

    // dst: NXM header for destination field
    buf.extend(dst_field.to_be_bytes());

    // value: 64-bit value
    buf.extend(value.to_be_bytes());
    buf
}

/// Encode NxMove action for copying bits between fields.
///
/// Format: `move:src[start..end]->dst[start..end]`
pub(crate) fn encode_nx_move(
    src_field: u32,
    dst_field: u32,
    n_bits: u16,
    src_ofs: u16,
    dst_ofs: u16,
) -> Vec<u8> {
    // move uses subtype 6
    // Format: NX header (10) + n_bits (2) + src_ofs (2) + dst_ofs (2) + src (4) + dst (4)
    let mut buf = encode_nx_header(NxActionSubtype::Move, 24);
    buf.extend(n_bits.to_be_bytes());
    buf.extend(src_ofs.to_be_bytes());
    buf.extend(dst_ofs.to_be_bytes());
    buf.extend(src_field.to_be_bytes());
    buf.extend(dst_field.to_be_bytes());
    buf
}

/// Encode NxLearn action for creating flows dynamically.
///
/// Wire format (variable length):
/// ```text
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// |         type (0xffff)       |         length                  |
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// |                      vendor (0x00002320)                      |
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// |       subtype (16)          |         idle_timeout            |
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// |       hard_timeout          |          priority               |
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// |                            cookie                             |
/// |                                                               |
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// |           flags             |  table_id   |       pad         |
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// |      fin_idle_timeout       |       fin_hard_timeout          |
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// |                   flow_mod_specs (variable)                   |
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// ```
pub(crate) fn encode_nx_learn(learn: &NxLearn) -> Vec<u8> {
    // Calculate specs size
    let specs_bytes = encode_learn_specs(&learn.specs);

    // Total length: NX header (10) + fields (22) + specs + padding
    let header_and_fields = 32; // 10 (header) + 22 (fixed fields)
    let total_len = header_and_fields + specs_bytes.len();
    // Pad to 8-byte boundary
    let padded_len = (total_len + 7) & !7;

    // Build action
    let mut buf = Vec::with_capacity(padded_len);

    // Action header
    buf.extend((ActionType::Experimenter as u16).to_be_bytes());
    buf.extend((padded_len as u16).to_be_bytes());
    buf.extend(NICIRA_VENDOR_ID.to_be_bytes());
    buf.extend((NxActionSubtype::Learn as u16).to_be_bytes());

    // Learn fields
    buf.extend(learn.idle_timeout.to_be_bytes());
    buf.extend(learn.hard_timeout.to_be_bytes());
    buf.extend(learn.priority.to_be_bytes());
    buf.extend(learn.cookie.to_be_bytes());
    buf.extend(learn.flags.to_be_bytes());
    buf.push(learn.table_id);
    buf.push(0); // pad
    buf.extend(learn.fin_idle_timeout.to_be_bytes());
    buf.extend(learn.fin_hard_timeout.to_be_bytes());

    // Specs
    buf.extend(specs_bytes);

    // Padding
    buf.resize(padded_len, 0);
    buf
}

/// Learn spec header bits.
mod learn_spec_header {
    /// Match from field (src = packet field, dst = match field)
    pub const SRC_FIELD: u16 = 0 << 13;
    /// Match from immediate value
    pub const SRC_IMMEDIATE: u16 = 1 << 13;
    /// Load from field to action field
    pub const DST_MATCH: u16 = 0 << 11;
    /// Load to output action
    pub const DST_LOAD: u16 = 1 << 11;
    /// Output to port
    pub const DST_OUTPUT: u16 = 2 << 11;
}

/// Encode a learn subfield to the wire format.
///
/// OVS learn subfield format (6 bytes):
/// - 4 bytes: NXM/OXM header (full header including length)
/// - 2 bytes: bit offset within the field
fn encode_learn_subfield(buf: &mut Vec<u8>, nxm_header: u32, ofs: u16) {
    buf.extend(nxm_header.to_be_bytes());
    buf.extend(ofs.to_be_bytes());
}

/// Encode learn specs to wire format.
fn encode_learn_specs(specs: &[LearnSpec]) -> Vec<u8> {
    let mut buf = Vec::new();

    for spec in specs {
        match spec {
            LearnSpec::MatchField { src_field, dst_field, n_bits } => {
                // Header: src=field, dst=match (bits 0-10 = n_bits - 1)
                let header = learn_spec_header::SRC_FIELD
                    | learn_spec_header::DST_MATCH
                    | (n_bits - 1);
                buf.extend(header.to_be_bytes());
                // Encode src and dst as learn subfields (6 bytes each: 4 header + 2 offset)
                encode_learn_subfield(&mut buf, *src_field, 0);
                encode_learn_subfield(&mut buf, *dst_field, 0);
            }
            LearnSpec::MatchImmediate { dst_field, value, n_bits } => {
                // Header: src=immediate, dst=match (bits 0-10 = n_bits - 1)
                let header = learn_spec_header::SRC_IMMEDIATE
                    | learn_spec_header::DST_MATCH
                    | (n_bits - 1);
                buf.extend(header.to_be_bytes());
                // Immediate value (padded to 2-byte chunks)
                let value_len = (*n_bits as usize).div_ceil(16) * 2;
                let mut padded_value = vec![0u8; value_len];
                let copy_len = value.len().min(value_len);
                padded_value[value_len - copy_len..].copy_from_slice(&value[..copy_len]);
                buf.extend(padded_value);
                // Encode dst as learn subfield (6 bytes: 4 header + 2 offset)
                encode_learn_subfield(&mut buf, *dst_field, 0);
            }
            LearnSpec::LoadField { src_field, dst_field, n_bits } => {
                // Header: src=field, dst=load (bits 0-10 = n_bits - 1)
                let header = learn_spec_header::SRC_FIELD
                    | learn_spec_header::DST_LOAD
                    | (n_bits - 1);
                buf.extend(header.to_be_bytes());
                // Encode src and dst as learn subfields (6 bytes each: 4 header + 2 offset)
                encode_learn_subfield(&mut buf, *src_field, 0);
                encode_learn_subfield(&mut buf, *dst_field, 0);
            }
            LearnSpec::LoadImmediate { dst_field, value, n_bits } => {
                // Header: src=immediate, dst=load (bits 0-10 = n_bits - 1)
                let header = learn_spec_header::SRC_IMMEDIATE
                    | learn_spec_header::DST_LOAD
                    | (n_bits - 1);
                buf.extend(header.to_be_bytes());
                // Immediate value
                let value_len = (*n_bits as usize).div_ceil(16) * 2;
                let mut padded_value = vec![0u8; value_len];
                let copy_len = value.len().min(value_len);
                padded_value[value_len - copy_len..].copy_from_slice(&value[..copy_len]);
                buf.extend(padded_value);
                // Encode dst as learn subfield (6 bytes: 4 header + 2 offset)
                encode_learn_subfield(&mut buf, *dst_field, 0);
            }
            LearnSpec::OutputField { src_field, n_bits } => {
                // Header: src=field, dst=output (bits 0-10 = n_bits - 1)
                let header = learn_spec_header::SRC_FIELD
                    | learn_spec_header::DST_OUTPUT
                    | (n_bits - 1);
                buf.extend(header.to_be_bytes());
                // Encode src as learn subfield (6 bytes: 4 header + 2 offset)
                encode_learn_subfield(&mut buf, *src_field, 0);
            }
        }
    }

    buf
}

// ============================================================================
// Nicira Action Decoding
// ============================================================================

use super::Action;
use crate::oxm::OxmClass;

/// Decode Nicira experimenter action.
///
/// The vendor ID has already been consumed. Data starts at subtype.
pub(crate) fn decode_nicira_action(data: &[u8]) -> crate::Result<Action> {
    if data.len() < 2 {
        return Err(crate::Error::Parse("nicira action too short".into()));
    }

    let subtype = u16::from_be_bytes([data[0], data[1]]);

    match subtype {
        s if s == NxActionSubtype::ResubmitTable as u16 => {
            // Resubmit: subtype (2) + in_port (2) + table (1) + pad (3)
            if data.len() < 6 {
                return Err(crate::Error::Parse("resubmit action too short".into()));
            }
            let in_port = u16::from_be_bytes([data[2], data[3]]);
            let table = data[4];
            let port = if in_port == 0xfff8 { None } else { Some(in_port) };
            let table = if table == 255 { None } else { Some(table) };
            Ok(Action::NxResubmit { port, table })
        }
        s if s == NxActionSubtype::Resubmit as u16 => {
            // Simple resubmit: subtype (2) + in_port (2)
            if data.len() < 4 {
                return Err(crate::Error::Parse("resubmit action too short".into()));
            }
            let in_port = u16::from_be_bytes([data[2], data[3]]);
            let port = if in_port == 0xfff8 { None } else { Some(in_port) };
            Ok(Action::NxResubmit { port, table: None })
        }
        s if s == NxActionSubtype::Ct as u16 => {
            // CT: subtype (2) + flags (2) + zone_src (4) + zone (2) + recirc_table (1) + ...
            if data.len() < 10 {
                return Err(crate::Error::Parse("ct action too short".into()));
            }
            let flags = u16::from_be_bytes([data[2], data[3]]);
            // zone_src at data[4..8]
            let zone = u16::from_be_bytes([data[8], data[9]]);
            let recirc_table = if data.len() > 10 { data[10] } else { 255 };
            let table = if recirc_table == 255 { None } else { Some(recirc_table) };
            Ok(Action::NxCt { flags, zone, table })
        }
        s if s == NxActionSubtype::RegLoad2 as u16 => {
            // RegLoad2: subtype (2) + OXM header (4) + value
            if data.len() < 6 {
                return Err(crate::Error::Parse("reg_load2 action too short".into()));
            }
            let oxm_header = u32::from_be_bytes([data[2], data[3], data[4], data[5]]);
            let oxm_class = (oxm_header >> 16) as u16;
            let field = ((oxm_header >> 9) & 0x7f) as u8;
            let length = (oxm_header & 0xff) as usize;

            if data.len() < 6 + length {
                return Err(crate::Error::Parse("reg_load2 value truncated".into()));
            }

            let value = &data[6..6 + length];

            // NXM1 class, field 16 = tunnel ID
            if oxm_class == OxmClass::Nxm1 as u16 && field == 16 && length >= 8 {
                let tun_id = u64::from_be_bytes([
                    value[0], value[1], value[2], value[3],
                    value[4], value[5], value[6], value[7],
                ]);
                Ok(Action::SetTunnelId(tun_id))
            } else {
                Ok(Action::Drop)
            }
        }
        s if s == NxActionSubtype::Learn as u16 => {
            // Learn: subtype (2) + idle_timeout (2) + hard_timeout (2) + priority (2)
            //        + cookie (8) + flags (2) + table_id (1) + pad (1)
            //        + fin_idle_timeout (2) + fin_hard_timeout (2) + specs (variable)
            if data.len() < 22 {
                return Err(crate::Error::Parse("learn action too short".into()));
            }
            let idle_timeout = u16::from_be_bytes([data[2], data[3]]);
            let hard_timeout = u16::from_be_bytes([data[4], data[5]]);
            let priority = u16::from_be_bytes([data[6], data[7]]);
            let cookie = u64::from_be_bytes([
                data[8], data[9], data[10], data[11],
                data[12], data[13], data[14], data[15],
            ]);
            let flags = u16::from_be_bytes([data[16], data[17]]);
            let table_id = data[18];
            // data[19] is padding
            let fin_idle_timeout = u16::from_be_bytes([data[20], data[21]]);
            let fin_hard_timeout = if data.len() > 23 {
                u16::from_be_bytes([data[22], data[23]])
            } else {
                0
            };

            // Decode specs (simplified - full decoding would parse the spec headers)
            let specs = if data.len() > 24 {
                decode_learn_specs(&data[24..])
            } else {
                Vec::new()
            };

            Ok(Action::NxLearn(NxLearn {
                idle_timeout,
                hard_timeout,
                priority,
                cookie,
                flags,
                table_id,
                fin_idle_timeout,
                fin_hard_timeout,
                specs,
            }))
        }
        _ => {
            // Unknown Nicira subtype
            Ok(Action::Drop)
        }
    }
}

/// Decode learn specs from wire format.
pub(crate) fn decode_learn_specs(data: &[u8]) -> Vec<LearnSpec> {
    let mut specs = Vec::new();
    let mut offset = 0;

    while offset + 2 <= data.len() {
        let header = u16::from_be_bytes([data[offset], data[offset + 1]]);
        if header == 0 {
            break; // End of specs
        }
        offset += 2;

        let n_bits = (header & 0x07ff) + 1; // Lower 11 bits store n_bits - 1
        let src_type = (header >> 13) & 0x01; // Bit 13: 0=field, 1=immediate
        let dst_type = (header >> 11) & 0x03; // Bits 11-12: 0=match, 1=load, 2=output

        match (src_type, dst_type) {
            (0, 0) => {
                // MatchField: src_subfield (6) + dst_subfield (6)
                // Subfield format: 4-byte NXM header + 2-byte offset
                if offset + 12 > data.len() {
                    break;
                }
                let src_field = u32::from_be_bytes([
                    data[offset], data[offset + 1], data[offset + 2], data[offset + 3],
                ]);
                // Skip 2-byte offset (currently unused in our API)
                let dst_field = u32::from_be_bytes([
                    data[offset + 6], data[offset + 7], data[offset + 8], data[offset + 9],
                ]);
                // Skip 2-byte offset
                offset += 12;
                specs.push(LearnSpec::MatchField { src_field, dst_field, n_bits });
            }
            (1, 0) => {
                // MatchImmediate: value (variable) + dst_subfield (6)
                let value_len = (n_bits as usize).div_ceil(16) * 2;
                if offset + value_len + 6 > data.len() {
                    break;
                }
                let value = data[offset..offset + value_len].to_vec();
                offset += value_len;
                let dst_field = u32::from_be_bytes([
                    data[offset], data[offset + 1], data[offset + 2], data[offset + 3],
                ]);
                offset += 6; // 4-byte header + 2-byte offset
                specs.push(LearnSpec::MatchImmediate { dst_field, value, n_bits });
            }
            (0, 1) => {
                // LoadField: src_subfield (6) + dst_subfield (6)
                if offset + 12 > data.len() {
                    break;
                }
                let src_field = u32::from_be_bytes([
                    data[offset], data[offset + 1], data[offset + 2], data[offset + 3],
                ]);
                let dst_field = u32::from_be_bytes([
                    data[offset + 6], data[offset + 7], data[offset + 8], data[offset + 9],
                ]);
                offset += 12;
                specs.push(LearnSpec::LoadField { src_field, dst_field, n_bits });
            }
            (1, 1) => {
                // LoadImmediate: value (variable) + dst_subfield (6)
                let value_len = (n_bits as usize).div_ceil(16) * 2;
                if offset + value_len + 6 > data.len() {
                    break;
                }
                let value = data[offset..offset + value_len].to_vec();
                offset += value_len;
                let dst_field = u32::from_be_bytes([
                    data[offset], data[offset + 1], data[offset + 2], data[offset + 3],
                ]);
                offset += 6; // 4-byte header + 2-byte offset
                specs.push(LearnSpec::LoadImmediate { dst_field, value, n_bits });
            }
            (0, 2) => {
                // OutputField: src_subfield (6)
                if offset + 6 > data.len() {
                    break;
                }
                let src_field = u32::from_be_bytes([
                    data[offset], data[offset + 1], data[offset + 2], data[offset + 3],
                ]);
                offset += 6; // 4-byte header + 2-byte offset
                specs.push(LearnSpec::OutputField { src_field, n_bits });
            }
            _ => {
                // Unknown spec type, skip
                break;
            }
        }
    }

    specs
}
