//! OpenFlow flow entries and modifications.

use bytes::Bytes;

use crate::instruction::{Instruction, InstructionList};
use crate::message::{Message, MessageType};
use crate::{ActionList, Match, Version};

/// Flow statistics read from the switch.
#[derive(Debug, Clone)]
pub struct FlowStats {
    /// Table ID
    pub table_id: u8,
    /// Priority (higher = more specific)
    pub priority: u16,
    /// Cookie (opaque identifier)
    pub cookie: u64,
    /// Match fields
    pub match_fields: Match,
    /// Actions to apply
    pub actions: ActionList,
    /// Idle timeout (seconds, 0 = no timeout)
    pub idle_timeout: u16,
    /// Hard timeout (seconds, 0 = no timeout)
    pub hard_timeout: u16,
    /// Packet count
    pub packet_count: u64,
    /// Byte count
    pub byte_count: u64,
}

/// Flow command type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum FlowCommand {
    /// Add a new flow
    Add = 0,
    /// Modify matching flows
    Modify = 1,
    /// Modify matching flows (strict match)
    ModifyStrict = 2,
    /// Delete matching flows
    Delete = 3,
    /// Delete matching flows (strict match)
    DeleteStrict = 4,
}

/// Flow flags.
#[derive(Debug, Clone, Copy, Default)]
pub struct FlowFlags {
    /// Send flow removed message
    pub send_flow_rem: bool,
    /// Check for overlapping entries
    pub check_overlap: bool,
    /// Reset flow counters
    pub reset_counts: bool,
    /// Don't keep track of packet count
    pub no_pkt_counts: bool,
    /// Don't keep track of byte count
    pub no_byte_counts: bool,
}

/// Flow flag bit values (OF 1.3).
pub mod flow_flags {
    /// Send flow removed message when flow expires
    pub const SEND_FLOW_REM: u16 = 1 << 0;
    /// Check for overlapping entries first
    pub const CHECK_OVERLAP: u16 = 1 << 1;
    /// Reset flow packet and byte counts
    pub const RESET_COUNTS: u16 = 1 << 2;
    /// Don't keep track of packet count
    pub const NO_PKT_COUNTS: u16 = 1 << 3;
    /// Don't keep track of byte count
    pub const NO_BYT_COUNTS: u16 = 1 << 4;
}

impl FlowFlags {
    /// Convert flags to wire format.
    pub fn to_wire(self) -> u16 {
        let mut flags = 0u16;
        if self.send_flow_rem {
            flags |= flow_flags::SEND_FLOW_REM;
        }
        if self.check_overlap {
            flags |= flow_flags::CHECK_OVERLAP;
        }
        if self.reset_counts {
            flags |= flow_flags::RESET_COUNTS;
        }
        if self.no_pkt_counts {
            flags |= flow_flags::NO_PKT_COUNTS;
        }
        if self.no_byte_counts {
            flags |= flow_flags::NO_BYT_COUNTS;
        }
        flags
    }
}

/// Special port value for "any" (used in delete commands).
pub const OFPP_ANY: u32 = 0xffff_ffff;
/// Special group value for "any" (used in delete commands).
pub const OFPG_ANY: u32 = 0xffff_ffff;
/// Special buffer ID for "no buffer".
pub const OFP_NO_BUFFER: u32 = 0xffff_ffff;

/// A flow entry to add, modify, or delete.
#[derive(Debug, Clone)]
pub struct Flow {
    /// Command (add, modify, delete)
    pub command: FlowCommand,
    /// Table ID (0xff for all tables on delete)
    pub table_id: u8,
    /// Priority
    pub priority: u16,
    /// Cookie
    pub cookie: u64,
    /// Cookie mask (for modify/delete)
    pub cookie_mask: u64,
    /// Match fields
    pub match_fields: Match,
    /// Actions (wrapped in ApplyActions instruction if instructions is empty)
    pub actions: ActionList,
    /// Instructions (OF 1.3+, takes precedence over actions if set)
    pub instructions: InstructionList,
    /// Idle timeout
    pub idle_timeout: u16,
    /// Hard timeout
    pub hard_timeout: u16,
    /// Flags
    pub flags: FlowFlags,
    /// Output port (for delete commands)
    pub out_port: Option<u32>,
    /// Output group (for delete commands)
    pub out_group: Option<u32>,
    /// Buffer ID (for packet-out)
    pub buffer_id: Option<u32>,
}

impl Flow {
    /// Create a new flow add command.
    pub fn add() -> Self {
        Self {
            command: FlowCommand::Add,
            table_id: 0,
            priority: 0,
            cookie: 0,
            cookie_mask: 0,
            match_fields: Match::new(),
            actions: ActionList::new(),
            instructions: InstructionList::new(),
            idle_timeout: 0,
            hard_timeout: 0,
            flags: FlowFlags::default(),
            out_port: None,
            out_group: None,
            buffer_id: None,
        }
    }

    /// Create a new flow delete command.
    pub fn delete() -> Self {
        Self {
            command: FlowCommand::Delete,
            table_id: 0xff, // All tables
            priority: 0,
            cookie: 0,
            cookie_mask: 0,
            match_fields: Match::new(),
            actions: ActionList::new(),
            instructions: InstructionList::new(),
            idle_timeout: 0,
            hard_timeout: 0,
            flags: FlowFlags::default(),
            out_port: None,
            out_group: None,
            buffer_id: None,
        }
    }

    /// Set the table ID.
    pub fn table(mut self, id: u8) -> Self {
        self.table_id = id;
        self
    }

    /// Set the priority.
    pub fn priority(mut self, priority: u16) -> Self {
        self.priority = priority;
        self
    }

    /// Set the cookie.
    pub fn cookie(mut self, cookie: u64) -> Self {
        self.cookie = cookie;
        self
    }

    /// Set the match fields.
    pub fn match_fields(mut self, m: Match) -> Self {
        self.match_fields = m;
        self
    }

    /// Set the actions (will be wrapped in ApplyActions instruction).
    pub fn actions(mut self, actions: ActionList) -> Self {
        self.actions = actions;
        self
    }

    /// Set the instructions directly (OF 1.3+).
    pub fn instructions(mut self, instructions: InstructionList) -> Self {
        self.instructions = instructions;
        self
    }

    /// Set the idle timeout.
    pub fn idle_timeout(mut self, timeout: u16) -> Self {
        self.idle_timeout = timeout;
        self
    }

    /// Set the hard timeout.
    pub fn hard_timeout(mut self, timeout: u16) -> Self {
        self.hard_timeout = timeout;
        self
    }

    /// Encode the FlowMod fixed fields (40 bytes).
    ///
    /// ```text
    /// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
    /// |                            cookie                             |
    /// |                                                               |
    /// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
    /// |                         cookie_mask                           |
    /// |                                                               |
    /// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
    /// |   table_id  |    command    |         idle_timeout            |
    /// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
    /// |         hard_timeout        |           priority              |
    /// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
    /// |                          buffer_id                            |
    /// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
    /// |                          out_port                             |
    /// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
    /// |                          out_group                            |
    /// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
    /// |            flags            |           pad (zeros)           |
    /// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
    /// ```
    fn encode_fixed(&self) -> [u8; 40] {
        let mut buf = [0u8; 40];

        // cookie (8 bytes)
        buf[0..8].copy_from_slice(&self.cookie.to_be_bytes());

        // cookie_mask (8 bytes)
        buf[8..16].copy_from_slice(&self.cookie_mask.to_be_bytes());

        // table_id (1 byte)
        buf[16] = self.table_id;

        // command (1 byte)
        buf[17] = self.command as u8;

        // idle_timeout (2 bytes)
        buf[18..20].copy_from_slice(&self.idle_timeout.to_be_bytes());

        // hard_timeout (2 bytes)
        buf[20..22].copy_from_slice(&self.hard_timeout.to_be_bytes());

        // priority (2 bytes)
        buf[22..24].copy_from_slice(&self.priority.to_be_bytes());

        // buffer_id (4 bytes)
        let buffer_id = self.buffer_id.unwrap_or(OFP_NO_BUFFER);
        buf[24..28].copy_from_slice(&buffer_id.to_be_bytes());

        // out_port (4 bytes)
        let out_port = self.out_port.unwrap_or(OFPP_ANY);
        buf[28..32].copy_from_slice(&out_port.to_be_bytes());

        // out_group (4 bytes)
        let out_group = self.out_group.unwrap_or(OFPG_ANY);
        buf[32..36].copy_from_slice(&out_group.to_be_bytes());

        // flags (2 bytes)
        buf[36..38].copy_from_slice(&self.flags.to_wire().to_be_bytes());

        // pad (2 bytes) - already zeroed

        buf
    }

    /// Encode the complete FlowMod body (without OF header).
    ///
    /// Returns: fixed fields (40) + match (variable) + instructions (variable)
    pub fn encode(&self) -> Vec<u8> {
        let fixed = self.encode_fixed();
        let match_bytes = self.match_fields.encode();

        // Build instructions: use explicit instructions if set, otherwise wrap actions
        let instruction_bytes = if self.instructions.is_empty() && !self.actions.is_empty() {
            // Wrap actions in ApplyActions instruction
            let inst = Instruction::ApplyActions(self.actions.clone());
            inst.encode()
        } else {
            self.instructions.encode()
        };

        let mut buf = Vec::with_capacity(40 + match_bytes.len() + instruction_bytes.len());
        buf.extend_from_slice(&fixed);
        buf.extend(match_bytes);
        buf.extend(instruction_bytes);
        buf
    }

    /// Create a complete OpenFlow message for this FlowMod.
    ///
    /// # Arguments
    /// * `version` - OpenFlow version (should be 1.3+)
    /// * `xid` - Transaction ID
    pub fn to_message(&self, version: Version, xid: u32) -> Message {
        let body = self.encode();
        Message::new(version, MessageType::FlowMod, xid, Bytes::from(body))
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::OutputPort;

    #[test]
    fn flow_flags_to_wire_empty() {
        let flags = FlowFlags::default();
        assert_eq!(flags.to_wire(), 0);
    }

    #[test]
    fn flow_flags_to_wire_all() {
        let flags = FlowFlags {
            send_flow_rem: true,
            check_overlap: true,
            reset_counts: true,
            no_pkt_counts: true,
            no_byte_counts: true,
        };
        assert_eq!(flags.to_wire(), 0b11111);
    }

    #[test]
    fn flow_flags_to_wire_send_flow_rem() {
        let flags = FlowFlags {
            send_flow_rem: true,
            ..Default::default()
        };
        assert_eq!(flags.to_wire(), flow_flags::SEND_FLOW_REM);
    }

    #[test]
    fn flow_command_wire_values() {
        assert_eq!(FlowCommand::Add as u8, 0);
        assert_eq!(FlowCommand::Modify as u8, 1);
        assert_eq!(FlowCommand::ModifyStrict as u8, 2);
        assert_eq!(FlowCommand::Delete as u8, 3);
        assert_eq!(FlowCommand::DeleteStrict as u8, 4);
    }

    #[test]
    fn encode_fixed_fields_add() {
        let flow = Flow::add()
            .table(1)
            .priority(100)
            .cookie(0x1234_5678_9abc_def0)
            .idle_timeout(60)
            .hard_timeout(120);

        let fixed = flow.encode_fixed();
        assert_eq!(fixed.len(), 40);

        // cookie
        assert_eq!(
            &fixed[0..8],
            &[0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0]
        );

        // cookie_mask = 0
        assert_eq!(&fixed[8..16], &[0, 0, 0, 0, 0, 0, 0, 0]);

        // table_id = 1
        assert_eq!(fixed[16], 1);

        // command = 0 (Add)
        assert_eq!(fixed[17], 0);

        // idle_timeout = 60
        assert_eq!(&fixed[18..20], &[0x00, 0x3c]);

        // hard_timeout = 120
        assert_eq!(&fixed[20..22], &[0x00, 0x78]);

        // priority = 100
        assert_eq!(&fixed[22..24], &[0x00, 0x64]);

        // buffer_id = OFP_NO_BUFFER
        assert_eq!(&fixed[24..28], &[0xff, 0xff, 0xff, 0xff]);

        // out_port = OFPP_ANY
        assert_eq!(&fixed[28..32], &[0xff, 0xff, 0xff, 0xff]);

        // out_group = OFPG_ANY
        assert_eq!(&fixed[32..36], &[0xff, 0xff, 0xff, 0xff]);

        // flags = 0
        assert_eq!(&fixed[36..38], &[0x00, 0x00]);

        // pad = 0
        assert_eq!(&fixed[38..40], &[0x00, 0x00]);
    }

    #[test]
    fn encode_fixed_fields_delete() {
        let flow = Flow::delete().table(0xff);

        let fixed = flow.encode_fixed();

        // table_id = 0xff (all tables)
        assert_eq!(fixed[16], 0xff);

        // command = 3 (Delete)
        assert_eq!(fixed[17], 3);
    }

    #[test]
    fn encode_flow_with_match() {
        let flow = Flow::add()
            .table(0)
            .priority(100)
            .match_fields(Match::new().in_port(1));

        let bytes = flow.encode();

        // Fixed (40) + Match (16 for in_port) = 56
        assert!(bytes.len() >= 56);

        // Verify fixed fields present
        assert_eq!(bytes[16], 0); // table_id
        assert_eq!(bytes[17], 0); // command = Add
    }

    #[test]
    fn encode_flow_with_actions() {
        let flow = Flow::add()
            .table(0)
            .priority(100)
            .match_fields(Match::new().in_port(1))
            .actions(ActionList::new().output(OutputPort::Port(2)));

        let bytes = flow.encode();

        // Fixed (40) + Match (16) + ApplyActions instruction (24) = 80
        assert_eq!(bytes.len(), 80);

        // Check that ApplyActions instruction is present after match
        // Match ends at offset 40 + 16 = 56
        // Instruction type = 4 (ApplyActions)
        assert_eq!(&bytes[56..58], &[0x00, 0x04]);
    }

    #[test]
    fn encode_flow_with_instructions() {
        let flow = Flow::add()
            .table(0)
            .priority(100)
            .match_fields(Match::new().in_port(1))
            .instructions(InstructionList::new().goto_table(5));

        let bytes = flow.encode();

        // Fixed (40) + Match (16) + GotoTable instruction (8) = 64
        assert_eq!(bytes.len(), 64);

        // GotoTable instruction at offset 56
        assert_eq!(&bytes[56..58], &[0x00, 0x01]); // type = 1
        assert_eq!(bytes[60], 5); // table_id = 5
    }

    #[test]
    fn to_message_creates_valid_header() {
        let flow = Flow::add()
            .table(0)
            .priority(100)
            .match_fields(Match::new().in_port(1));

        let msg = flow.to_message(Version::Of13, 0x1234);

        // Verify header
        assert_eq!(msg.header.version, Version::Of13);
        assert_eq!(msg.header.msg_type, MessageType::FlowMod);
        assert_eq!(msg.header.xid, 0x1234);

        // Length should be header (8) + body
        let expected_len = 8 + flow.encode().len();
        assert_eq!(msg.header.length as usize, expected_len);
    }

    #[test]
    fn to_message_encodes_correctly() {
        let flow = Flow::add()
            .table(0)
            .priority(100)
            .match_fields(Match::new().in_port(1))
            .actions(ActionList::new().output(OutputPort::Port(2)));

        let msg = flow.to_message(Version::Of13, 42);
        let encoded = msg.encode();

        // Check OF header
        assert_eq!(encoded[0], 0x04); // version = OF 1.3
        assert_eq!(encoded[1], 14); // type = FlowMod
        assert_eq!(encoded[6], 0); // xid high bytes
        assert_eq!(encoded[7], 42); // xid low byte

        // Body starts at offset 8
        // Fixed fields: table_id at offset 8+16=24
        assert_eq!(encoded[24], 0); // table_id = 0
    }

    #[test]
    fn flow_builder_chain() {
        let flow = Flow::add()
            .table(5)
            .priority(1000)
            .cookie(0xdead_beef)
            .idle_timeout(300)
            .hard_timeout(600)
            .match_fields(Match::new().eth_type(0x0800).ipv4_dst("10.0.0.0".parse().unwrap(), 24))
            .actions(ActionList::new().output(OutputPort::Port(1)));

        assert_eq!(flow.table_id, 5);
        assert_eq!(flow.priority, 1000);
        assert_eq!(flow.cookie, 0xdead_beef);
        assert_eq!(flow.idle_timeout, 300);
        assert_eq!(flow.hard_timeout, 600);
    }
}
