//! OpenFlow instruction encoding (OF 1.3+).
//!
//! Instructions are the top-level processing directives in OpenFlow 1.1+.
//! They wrap actions and control table pipeline behavior.

use std::fmt;

use crate::action::ActionList;

/// OpenFlow instruction type wire values (OF 1.3+).
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum InstructionType {
    /// Go to specified table
    GotoTable = 1,
    /// Write metadata value
    WriteMetadata = 2,
    /// Write actions to action set
    WriteActions = 3,
    /// Apply actions immediately
    ApplyActions = 4,
    /// Clear action set
    ClearActions = 5,
    /// Apply meter
    Meter = 6,
}

impl TryFrom<u16> for InstructionType {
    type Error = crate::Error;

    fn try_from(v: u16) -> Result<Self, Self::Error> {
        match v {
            1 => Ok(Self::GotoTable),
            2 => Ok(Self::WriteMetadata),
            3 => Ok(Self::WriteActions),
            4 => Ok(Self::ApplyActions),
            5 => Ok(Self::ClearActions),
            6 => Ok(Self::Meter),
            _ => Err(crate::Error::Parse(format!(
                "unknown instruction type: {v}"
            ))),
        }
    }
}

/// An OpenFlow instruction.
#[derive(Debug, Clone)]
pub enum Instruction {
    /// Go to another table in the pipeline.
    GotoTable(u8),

    /// Write metadata value with optional mask.
    WriteMetadata { metadata: u64, mask: u64 },

    /// Apply actions immediately to the packet.
    ApplyActions(ActionList),

    /// Write actions to the action set (executed at end of pipeline).
    WriteActions(ActionList),

    /// Clear all actions in the action set.
    ClearActions,

    /// Apply a meter to the packet.
    Meter(u32),
}

impl fmt::Display for Instruction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::GotoTable(table) => write!(f, "goto_table:{table}"),
            Self::WriteMetadata { metadata, mask } => {
                write!(f, "write_metadata:0x{metadata:x}/0x{mask:x}")
            }
            Self::ApplyActions(actions) => write!(f, "apply_actions({actions})"),
            Self::WriteActions(actions) => write!(f, "write_actions({actions})"),
            Self::ClearActions => write!(f, "clear_actions"),
            Self::Meter(id) => write!(f, "meter:{id}"),
        }
    }
}

impl fmt::Display for InstructionList {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Common case: a single ApplyActions instruction unwraps to just the actions
        if self.instructions.len() == 1 {
            if let Instruction::ApplyActions(actions) = &self.instructions[0] {
                return write!(f, "{actions}");
            }
        }
        if self.instructions.is_empty() {
            return write!(f, "drop");
        }
        for (i, inst) in self.instructions.iter().enumerate() {
            if i > 0 {
                write!(f, ",")?;
            }
            write!(f, "{inst}")?;
        }
        Ok(())
    }
}

impl Instruction {
    /// Encode instruction to OpenFlow wire format.
    pub fn encode(&self) -> Vec<u8> {
        match self {
            Self::GotoTable(table_id) => encode_goto_table(*table_id),
            Self::WriteMetadata { metadata, mask } => encode_write_metadata(*metadata, *mask),
            Self::ApplyActions(actions) => encode_apply_actions(actions),
            Self::WriteActions(actions) => encode_write_actions(actions),
            Self::ClearActions => encode_clear_actions(),
            Self::Meter(meter_id) => encode_meter(*meter_id),
        }
    }

    /// Decode instruction from OpenFlow wire format.
    ///
    /// Returns the decoded instruction and the number of bytes consumed.
    pub fn decode(data: &[u8]) -> crate::Result<(Self, usize)> {
        if data.len() < 4 {
            return Err(crate::Error::Parse("instruction too short".into()));
        }

        let inst_type = u16::from_be_bytes([data[0], data[1]]);
        let length = u16::from_be_bytes([data[2], data[3]]) as usize;

        if data.len() < length {
            return Err(crate::Error::Parse("instruction truncated".into()));
        }

        let inst_type = InstructionType::try_from(inst_type)?;

        let instruction = match inst_type {
            InstructionType::GotoTable => {
                if length < 8 {
                    return Err(crate::Error::Parse("goto_table instruction too short".into()));
                }
                let table_id = data[4];
                Self::GotoTable(table_id)
            }
            InstructionType::WriteMetadata => {
                if length < 24 {
                    return Err(crate::Error::Parse(
                        "write_metadata instruction too short".into(),
                    ));
                }
                // data[4..8] is padding
                let metadata = u64::from_be_bytes([
                    data[8], data[9], data[10], data[11], data[12], data[13], data[14], data[15],
                ]);
                let mask = u64::from_be_bytes([
                    data[16], data[17], data[18], data[19], data[20], data[21], data[22], data[23],
                ]);
                Self::WriteMetadata { metadata, mask }
            }
            InstructionType::ApplyActions => {
                // data[4..8] is padding, actions start at offset 8
                let actions_data = &data[8..length];
                let actions = ActionList::decode(actions_data)?;
                Self::ApplyActions(actions)
            }
            InstructionType::WriteActions => {
                // data[4..8] is padding, actions start at offset 8
                let actions_data = &data[8..length];
                let actions = ActionList::decode(actions_data)?;
                Self::WriteActions(actions)
            }
            InstructionType::ClearActions => Self::ClearActions,
            InstructionType::Meter => {
                if length < 8 {
                    return Err(crate::Error::Parse("meter instruction too short".into()));
                }
                let meter_id = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
                Self::Meter(meter_id)
            }
        };

        Ok((instruction, length))
    }
}

/// A list of instructions.
#[derive(Debug, Clone, Default)]
pub struct InstructionList {
    instructions: Vec<Instruction>,
}

impl InstructionList {
    /// Create a new empty instruction list.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add an instruction to the list.
    pub fn push(&mut self, instruction: Instruction) {
        self.instructions.push(instruction);
    }

    /// Go to another table.
    pub fn goto_table(mut self, table_id: u8) -> Self {
        self.instructions.push(Instruction::GotoTable(table_id));
        self
    }

    /// Write metadata with mask.
    pub fn write_metadata(mut self, metadata: u64, mask: u64) -> Self {
        self.instructions
            .push(Instruction::WriteMetadata { metadata, mask });
        self
    }

    /// Apply actions immediately.
    pub fn apply_actions(mut self, actions: ActionList) -> Self {
        self.instructions.push(Instruction::ApplyActions(actions));
        self
    }

    /// Write actions to action set.
    pub fn write_actions(mut self, actions: ActionList) -> Self {
        self.instructions.push(Instruction::WriteActions(actions));
        self
    }

    /// Clear action set.
    pub fn clear_actions(mut self) -> Self {
        self.instructions.push(Instruction::ClearActions);
        self
    }

    /// Apply meter.
    pub fn meter(mut self, meter_id: u32) -> Self {
        self.instructions.push(Instruction::Meter(meter_id));
        self
    }

    /// Get the instructions.
    pub fn instructions(&self) -> &[Instruction] {
        &self.instructions
    }

    /// Check if the list is empty.
    pub fn is_empty(&self) -> bool {
        self.instructions.is_empty()
    }

    /// Get the number of instructions.
    pub fn len(&self) -> usize {
        self.instructions.len()
    }

    /// Encode all instructions to wire format.
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        for instruction in &self.instructions {
            buf.extend(instruction.encode());
        }
        buf
    }

    /// Decode all instructions from wire format.
    ///
    /// Reads instructions until the data is exhausted.
    pub fn decode(data: &[u8]) -> crate::Result<Self> {
        let mut instructions = Vec::new();
        let mut offset = 0;

        while offset < data.len() {
            // Need at least 4 bytes for instruction header
            if data.len() - offset < 4 {
                break;
            }

            // Check for zero-length padding at end
            let length = u16::from_be_bytes([data[offset + 2], data[offset + 3]]) as usize;
            if length == 0 {
                break;
            }

            let (instruction, consumed) = Instruction::decode(&data[offset..])?;
            instructions.push(instruction);
            offset += consumed;
        }

        Ok(Self { instructions })
    }
}

// ============================================================================
// Instruction Encoding Functions
// ============================================================================

/// Encode GotoTable instruction (8 bytes).
///
/// ```text
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// |         type (1)            |          length (8)             |
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// |  table_id   |                    pad (zeros)                  |
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// ```
fn encode_goto_table(table_id: u8) -> Vec<u8> {
    let mut buf = Vec::with_capacity(8);
    buf.extend((InstructionType::GotoTable as u16).to_be_bytes());
    buf.extend(8u16.to_be_bytes()); // length
    buf.push(table_id);
    buf.extend([0u8; 3]); // padding
    buf
}

/// Encode WriteMetadata instruction (24 bytes).
///
/// ```text
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// |         type (2)            |          length (24)            |
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// |                           pad (zeros)                         |
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// |                           metadata                            |
/// |                                                               |
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// |                         metadata_mask                         |
/// |                                                               |
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// ```
fn encode_write_metadata(metadata: u64, mask: u64) -> Vec<u8> {
    let mut buf = Vec::with_capacity(24);
    buf.extend((InstructionType::WriteMetadata as u16).to_be_bytes());
    buf.extend(24u16.to_be_bytes()); // length
    buf.extend([0u8; 4]); // padding
    buf.extend(metadata.to_be_bytes());
    buf.extend(mask.to_be_bytes());
    buf
}

/// Encode ApplyActions instruction (variable size).
///
/// ```text
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// |         type (4)            |          length                 |
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// |                           pad (zeros)                         |
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// |                       actions (variable)                      |
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// ```
fn encode_apply_actions(actions: &ActionList) -> Vec<u8> {
    let actions_bytes = actions.encode();
    let len = 8 + actions_bytes.len(); // header (4) + pad (4) + actions

    let mut buf = Vec::with_capacity(len);
    buf.extend((InstructionType::ApplyActions as u16).to_be_bytes());
    buf.extend((len as u16).to_be_bytes());
    buf.extend([0u8; 4]); // padding
    buf.extend(actions_bytes);
    buf
}

/// Encode WriteActions instruction (variable size).
///
/// Same format as ApplyActions but with type=3.
fn encode_write_actions(actions: &ActionList) -> Vec<u8> {
    let actions_bytes = actions.encode();
    let len = 8 + actions_bytes.len();

    let mut buf = Vec::with_capacity(len);
    buf.extend((InstructionType::WriteActions as u16).to_be_bytes());
    buf.extend((len as u16).to_be_bytes());
    buf.extend([0u8; 4]); // padding
    buf.extend(actions_bytes);
    buf
}

/// Encode ClearActions instruction (8 bytes).
///
/// ```text
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// |         type (5)            |          length (8)             |
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// |                           pad (zeros)                         |
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// ```
fn encode_clear_actions() -> Vec<u8> {
    let mut buf = Vec::with_capacity(8);
    buf.extend((InstructionType::ClearActions as u16).to_be_bytes());
    buf.extend(8u16.to_be_bytes()); // length
    buf.extend([0u8; 4]); // padding
    buf
}

/// Encode Meter instruction (8 bytes).
///
/// ```text
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// |         type (6)            |          length (8)             |
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// |                          meter_id                             |
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// ```
fn encode_meter(meter_id: u32) -> Vec<u8> {
    let mut buf = Vec::with_capacity(8);
    buf.extend((InstructionType::Meter as u16).to_be_bytes());
    buf.extend(8u16.to_be_bytes()); // length
    buf.extend(meter_id.to_be_bytes());
    buf
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::OutputPort;

    #[test]
    fn instruction_type_wire_values() {
        assert_eq!(InstructionType::GotoTable as u16, 1);
        assert_eq!(InstructionType::WriteMetadata as u16, 2);
        assert_eq!(InstructionType::WriteActions as u16, 3);
        assert_eq!(InstructionType::ApplyActions as u16, 4);
        assert_eq!(InstructionType::ClearActions as u16, 5);
        assert_eq!(InstructionType::Meter as u16, 6);
    }

    #[test]
    fn encode_goto_table_instruction() {
        let bytes = encode_goto_table(5);
        assert_eq!(bytes.len(), 8);
        // type = 1 (GotoTable)
        assert_eq!(&bytes[0..2], &[0x00, 0x01]);
        // length = 8
        assert_eq!(&bytes[2..4], &[0x00, 0x08]);
        // table_id = 5
        assert_eq!(bytes[4], 5);
        // padding
        assert_eq!(&bytes[5..8], &[0, 0, 0]);
    }

    #[test]
    fn encode_write_metadata_instruction() {
        let bytes = encode_write_metadata(0x1234_5678_9abc_def0, 0xffff_ffff_ffff_ffff);
        assert_eq!(bytes.len(), 24);
        // type = 2 (WriteMetadata)
        assert_eq!(&bytes[0..2], &[0x00, 0x02]);
        // length = 24
        assert_eq!(&bytes[2..4], &[0x00, 0x18]);
        // padding
        assert_eq!(&bytes[4..8], &[0, 0, 0, 0]);
        // metadata
        assert_eq!(
            &bytes[8..16],
            &[0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0]
        );
        // mask
        assert_eq!(
            &bytes[16..24],
            &[0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff]
        );
    }

    #[test]
    fn encode_apply_actions_instruction() {
        let actions = ActionList::new().output(OutputPort::Port(1));
        let bytes = encode_apply_actions(&actions);
        // Header (4) + pad (4) + Output action (16) = 24
        assert_eq!(bytes.len(), 24);
        // type = 4 (ApplyActions)
        assert_eq!(&bytes[0..2], &[0x00, 0x04]);
        // length = 24
        assert_eq!(&bytes[2..4], &[0x00, 0x18]);
        // padding
        assert_eq!(&bytes[4..8], &[0, 0, 0, 0]);
        // First action should be Output (type=0)
        assert_eq!(&bytes[8..10], &[0x00, 0x00]);
    }

    #[test]
    fn encode_apply_actions_empty() {
        let actions = ActionList::new();
        let bytes = encode_apply_actions(&actions);
        // Header (4) + pad (4) = 8
        assert_eq!(bytes.len(), 8);
        // type = 4
        assert_eq!(&bytes[0..2], &[0x00, 0x04]);
        // length = 8
        assert_eq!(&bytes[2..4], &[0x00, 0x08]);
    }

    #[test]
    fn encode_write_actions_instruction() {
        let actions = ActionList::new().output(OutputPort::Port(2));
        let bytes = encode_write_actions(&actions);
        // Header (4) + pad (4) + Output action (16) = 24
        assert_eq!(bytes.len(), 24);
        // type = 3 (WriteActions)
        assert_eq!(&bytes[0..2], &[0x00, 0x03]);
        // length = 24
        assert_eq!(&bytes[2..4], &[0x00, 0x18]);
    }

    #[test]
    fn encode_clear_actions_instruction() {
        let bytes = encode_clear_actions();
        assert_eq!(bytes.len(), 8);
        // type = 5 (ClearActions)
        assert_eq!(&bytes[0..2], &[0x00, 0x05]);
        // length = 8
        assert_eq!(&bytes[2..4], &[0x00, 0x08]);
        // padding
        assert_eq!(&bytes[4..8], &[0, 0, 0, 0]);
    }

    #[test]
    fn encode_meter_instruction() {
        let bytes = encode_meter(100);
        assert_eq!(bytes.len(), 8);
        // type = 6 (Meter)
        assert_eq!(&bytes[0..2], &[0x00, 0x06]);
        // length = 8
        assert_eq!(&bytes[2..4], &[0x00, 0x08]);
        // meter_id = 100
        assert_eq!(&bytes[4..8], &[0x00, 0x00, 0x00, 0x64]);
    }

    #[test]
    fn instruction_goto_table_encode() {
        let inst = Instruction::GotoTable(10);
        let bytes = inst.encode();
        assert_eq!(bytes.len(), 8);
        assert_eq!(bytes[4], 10);
    }

    #[test]
    fn instruction_write_metadata_encode() {
        let inst = Instruction::WriteMetadata {
            metadata: 0x1234,
            mask: 0xffff,
        };
        let bytes = inst.encode();
        assert_eq!(bytes.len(), 24);
    }

    #[test]
    fn instruction_apply_actions_encode() {
        let actions = ActionList::new().pop_vlan().output(OutputPort::Port(1));
        let inst = Instruction::ApplyActions(actions);
        let bytes = inst.encode();
        // Header (4) + pad (4) + PopVlan (8) + Output (16) = 32
        assert_eq!(bytes.len(), 32);
    }

    #[test]
    fn instruction_clear_actions_encode() {
        let inst = Instruction::ClearActions;
        let bytes = inst.encode();
        assert_eq!(bytes.len(), 8);
        assert_eq!(&bytes[0..2], &[0x00, 0x05]);
    }

    #[test]
    fn instruction_meter_encode() {
        let inst = Instruction::Meter(42);
        let bytes = inst.encode();
        assert_eq!(bytes.len(), 8);
        assert_eq!(&bytes[4..8], &[0x00, 0x00, 0x00, 0x2a]);
    }

    #[test]
    fn instruction_list_encode_empty() {
        let list = InstructionList::new();
        let bytes = list.encode();
        assert!(bytes.is_empty());
    }

    #[test]
    fn instruction_list_encode_single() {
        let list = InstructionList::new().goto_table(5);
        let bytes = list.encode();
        assert_eq!(bytes.len(), 8);
        assert_eq!(&bytes[0..2], &[0x00, 0x01]);
        assert_eq!(bytes[4], 5);
    }

    #[test]
    fn instruction_list_encode_multiple() {
        let actions = ActionList::new().output(OutputPort::Port(1));
        let list = InstructionList::new()
            .apply_actions(actions)
            .goto_table(10);
        let bytes = list.encode();
        // ApplyActions (24) + GotoTable (8) = 32
        assert_eq!(bytes.len(), 32);
        // First instruction: ApplyActions (type=4)
        assert_eq!(&bytes[0..2], &[0x00, 0x04]);
        // Second instruction: GotoTable (type=1)
        assert_eq!(&bytes[24..26], &[0x00, 0x01]);
    }

    #[test]
    fn instruction_list_builder_methods() {
        let actions = ActionList::new().output(OutputPort::Port(1));
        let list = InstructionList::new()
            .write_metadata(0x1234, 0xffff)
            .apply_actions(actions)
            .clear_actions()
            .meter(100)
            .goto_table(5);
        assert_eq!(list.len(), 5);
        assert!(!list.is_empty());
    }

    // ========================================================================
    // Decode tests
    // ========================================================================

    #[test]
    fn decode_goto_table_instruction() {
        let inst = Instruction::GotoTable(10);
        let encoded = inst.encode();
        let (decoded, len) = Instruction::decode(&encoded).unwrap();
        assert_eq!(len, 8);
        match decoded {
            Instruction::GotoTable(table_id) => assert_eq!(table_id, 10),
            _ => panic!("expected GotoTable instruction"),
        }
    }

    #[test]
    fn decode_write_metadata_instruction() {
        let inst = Instruction::WriteMetadata {
            metadata: 0x1234_5678_9abc_def0,
            mask: 0xffff_ffff_0000_0000,
        };
        let encoded = inst.encode();
        let (decoded, len) = Instruction::decode(&encoded).unwrap();
        assert_eq!(len, 24);
        match decoded {
            Instruction::WriteMetadata { metadata, mask } => {
                assert_eq!(metadata, 0x1234_5678_9abc_def0);
                assert_eq!(mask, 0xffff_ffff_0000_0000);
            }
            _ => panic!("expected WriteMetadata instruction"),
        }
    }

    #[test]
    fn decode_apply_actions_instruction() {
        let actions = ActionList::new()
            .pop_vlan()
            .output(OutputPort::Port(2));
        let inst = Instruction::ApplyActions(actions);
        let encoded = inst.encode();
        let (decoded, _) = Instruction::decode(&encoded).unwrap();
        match decoded {
            Instruction::ApplyActions(actions) => {
                assert_eq!(actions.len(), 2);
            }
            _ => panic!("expected ApplyActions instruction"),
        }
    }

    #[test]
    fn decode_write_actions_instruction() {
        let actions = ActionList::new().output(OutputPort::Port(1));
        let inst = Instruction::WriteActions(actions);
        let encoded = inst.encode();
        let (decoded, _) = Instruction::decode(&encoded).unwrap();
        match decoded {
            Instruction::WriteActions(actions) => {
                assert_eq!(actions.len(), 1);
            }
            _ => panic!("expected WriteActions instruction"),
        }
    }

    #[test]
    fn decode_clear_actions_instruction() {
        let inst = Instruction::ClearActions;
        let encoded = inst.encode();
        let (decoded, len) = Instruction::decode(&encoded).unwrap();
        assert_eq!(len, 8);
        assert!(matches!(decoded, Instruction::ClearActions));
    }

    #[test]
    fn decode_meter_instruction() {
        let inst = Instruction::Meter(42);
        let encoded = inst.encode();
        let (decoded, len) = Instruction::decode(&encoded).unwrap();
        assert_eq!(len, 8);
        match decoded {
            Instruction::Meter(meter_id) => assert_eq!(meter_id, 42),
            _ => panic!("expected Meter instruction"),
        }
    }

    #[test]
    fn decode_instruction_list_multiple() {
        let actions = ActionList::new().output(OutputPort::Port(1));
        let original = InstructionList::new()
            .apply_actions(actions)
            .goto_table(10);
        let encoded = original.encode();
        let decoded = InstructionList::decode(&encoded).unwrap();
        assert_eq!(decoded.len(), 2);
        assert!(matches!(decoded.instructions()[0], Instruction::ApplyActions(_)));
        assert!(matches!(decoded.instructions()[1], Instruction::GotoTable(10)));
    }

    #[test]
    fn decode_instruction_list_empty() {
        let original = InstructionList::new();
        let encoded = original.encode();
        let decoded = InstructionList::decode(&encoded).unwrap();
        assert!(decoded.is_empty());
    }

    #[test]
    fn roundtrip_instruction_list() {
        let actions = ActionList::new()
            .push_vlan(0x8100)
            .set_vlan_vid(100)
            .output(OutputPort::Port(3));
        let original = InstructionList::new()
            .write_metadata(0xabcd, 0xffff)
            .apply_actions(actions)
            .meter(50)
            .goto_table(5);

        let encoded = original.encode();
        let decoded = InstructionList::decode(&encoded).unwrap();

        assert_eq!(decoded.len(), 4);
        match &decoded.instructions()[0] {
            Instruction::WriteMetadata { metadata, mask } => {
                assert_eq!(*metadata, 0xabcd);
                assert_eq!(*mask, 0xffff);
            }
            _ => panic!("expected WriteMetadata"),
        }
        match &decoded.instructions()[1] {
            Instruction::ApplyActions(actions) => assert_eq!(actions.len(), 3),
            _ => panic!("expected ApplyActions"),
        }
        match &decoded.instructions()[2] {
            Instruction::Meter(meter_id) => assert_eq!(*meter_id, 50),
            _ => panic!("expected Meter"),
        }
        match &decoded.instructions()[3] {
            Instruction::GotoTable(table_id) => assert_eq!(*table_id, 5),
            _ => panic!("expected GotoTable"),
        }
    }

    #[test]
    fn instruction_type_try_from() {
        assert_eq!(InstructionType::try_from(1).unwrap(), InstructionType::GotoTable);
        assert_eq!(InstructionType::try_from(2).unwrap(), InstructionType::WriteMetadata);
        assert_eq!(InstructionType::try_from(3).unwrap(), InstructionType::WriteActions);
        assert_eq!(InstructionType::try_from(4).unwrap(), InstructionType::ApplyActions);
        assert_eq!(InstructionType::try_from(5).unwrap(), InstructionType::ClearActions);
        assert_eq!(InstructionType::try_from(6).unwrap(), InstructionType::Meter);
        assert!(InstructionType::try_from(99).is_err());
    }
}
