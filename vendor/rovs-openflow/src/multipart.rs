//! OpenFlow Multipart messages (OF 1.3+).
//!
//! Multipart messages are used for statistics, table features, and other
//! requests that may require multiple reply messages.

use bytes::Bytes;

use crate::flow::OFPP_ANY;
use crate::instruction::InstructionList;
use crate::message::{Message, MessageType};
use crate::{Match, Version};

/// Multipart message types (OF 1.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum MultipartType {
    /// Switch description
    Desc = 0,
    /// Individual flow statistics
    Flow = 1,
    /// Aggregate flow statistics
    Aggregate = 2,
    /// Flow table statistics
    Table = 3,
    /// Port statistics
    PortStats = 4,
    /// Queue statistics
    Queue = 5,
    /// Group statistics
    Group = 6,
    /// Group description
    GroupDesc = 7,
    /// Group features
    GroupFeatures = 8,
    /// Meter statistics
    Meter = 9,
    /// Meter configuration
    MeterConfig = 10,
    /// Meter features
    MeterFeatures = 11,
    /// Table features
    TableFeatures = 12,
    /// Port description
    PortDesc = 13,
    /// Experimenter extension
    Experimenter = 0xffff,
}

impl TryFrom<u16> for MultipartType {
    type Error = crate::Error;

    fn try_from(v: u16) -> Result<Self, Self::Error> {
        match v {
            0 => Ok(Self::Desc),
            1 => Ok(Self::Flow),
            2 => Ok(Self::Aggregate),
            3 => Ok(Self::Table),
            4 => Ok(Self::PortStats),
            5 => Ok(Self::Queue),
            6 => Ok(Self::Group),
            7 => Ok(Self::GroupDesc),
            8 => Ok(Self::GroupFeatures),
            9 => Ok(Self::Meter),
            10 => Ok(Self::MeterConfig),
            11 => Ok(Self::MeterFeatures),
            12 => Ok(Self::TableFeatures),
            13 => Ok(Self::PortDesc),
            0xffff => Ok(Self::Experimenter),
            _ => Err(crate::Error::Parse(format!(
                "unknown multipart type: {v}"
            ))),
        }
    }
}

/// Multipart request flags.
pub mod multipart_flags {
    /// More requests/replies to follow
    pub const MORE: u16 = 1 << 0;
}

/// Multipart request header (8 bytes).
///
/// ```text
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// |            type             |            flags              |
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// |                           pad (4)                           |
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// ```
#[derive(Debug, Clone)]
pub struct MultipartHeader {
    /// Multipart type
    pub mp_type: MultipartType,
    /// Flags (MORE = more messages follow)
    pub flags: u16,
}

impl MultipartHeader {
    /// Header size in bytes.
    pub const SIZE: usize = 8;

    /// Encode the header.
    pub fn encode(&self) -> [u8; 8] {
        let mut buf = [0u8; 8];
        buf[0..2].copy_from_slice(&(self.mp_type as u16).to_be_bytes());
        buf[2..4].copy_from_slice(&self.flags.to_be_bytes());
        // bytes 4-7 are padding (zeros)
        buf
    }

    /// Decode the header from bytes.
    pub fn decode(data: &[u8]) -> crate::Result<Self> {
        if data.len() < Self::SIZE {
            return Err(crate::Error::Parse("multipart header too short".into()));
        }

        let mp_type = u16::from_be_bytes([data[0], data[1]]);
        let flags = u16::from_be_bytes([data[2], data[3]]);

        Ok(Self {
            mp_type: MultipartType::try_from(mp_type)?,
            flags,
        })
    }

    /// Check if MORE flag is set.
    pub fn has_more(&self) -> bool {
        self.flags & multipart_flags::MORE != 0
    }
}

/// Flow stats request body (follows multipart header).
///
/// ```text
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// |   table_id  |   pad (3)                                      |
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// |                          out_port                            |
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// |                          out_group                           |
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// |                           pad (4)                            |
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// |                           cookie                             |
/// |                                                               |
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// |                        cookie_mask                           |
/// |                                                               |
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// |                       match (variable)                       |
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// ```
#[derive(Debug, Clone)]
pub struct FlowStatsRequest {
    /// Table ID (0xff for all tables)
    pub table_id: u8,
    /// Output port filter (OFPP_ANY for any)
    pub out_port: u32,
    /// Output group filter (OFPG_ANY for any)
    pub out_group: u32,
    /// Cookie filter
    pub cookie: u64,
    /// Cookie mask (0 to match all cookies)
    pub cookie_mask: u64,
    /// Match fields filter
    pub match_fields: Match,
}

impl Default for FlowStatsRequest {
    fn default() -> Self {
        Self::new()
    }
}

impl FlowStatsRequest {
    /// Create a new flow stats request that matches all flows.
    pub fn new() -> Self {
        Self {
            table_id: 0xff,   // All tables
            out_port: OFPP_ANY,
            out_group: OFPP_ANY, // OFPG_ANY has same value
            cookie: 0,
            cookie_mask: 0,   // Match all cookies
            match_fields: Match::new(),
        }
    }

    /// Filter by table ID.
    pub fn table(mut self, table_id: u8) -> Self {
        self.table_id = table_id;
        self
    }

    /// Filter by match fields.
    pub fn match_fields(mut self, m: Match) -> Self {
        self.match_fields = m;
        self
    }

    /// Filter by cookie.
    pub fn cookie(mut self, cookie: u64, mask: u64) -> Self {
        self.cookie = cookie;
        self.cookie_mask = mask;
        self
    }

    /// Encode the flow stats request body.
    pub fn encode(&self) -> Vec<u8> {
        let match_bytes = self.match_fields.encode();
        let mut buf = Vec::with_capacity(32 + match_bytes.len());

        // table_id (1) + pad (3)
        buf.push(self.table_id);
        buf.extend([0u8; 3]);

        // out_port (4)
        buf.extend(self.out_port.to_be_bytes());

        // out_group (4)
        buf.extend(self.out_group.to_be_bytes());

        // pad (4)
        buf.extend([0u8; 4]);

        // cookie (8)
        buf.extend(self.cookie.to_be_bytes());

        // cookie_mask (8)
        buf.extend(self.cookie_mask.to_be_bytes());

        // match (variable)
        buf.extend(match_bytes);

        buf
    }

    /// Create the complete multipart request message.
    pub fn to_message(&self, version: Version, xid: u32) -> Message {
        let header = MultipartHeader {
            mp_type: MultipartType::Flow,
            flags: 0,
        };

        let mut body = Vec::new();
        body.extend(header.encode());
        body.extend(self.encode());

        Message::new(version, MessageType::MultipartRequest, xid, Bytes::from(body))
    }
}

/// Individual flow statistics from a FlowStats reply.
///
/// ```text
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// |           length            |   table_id  |      pad        |
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// |                         duration_sec                         |
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// |                        duration_nsec                         |
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// |           priority          |         idle_timeout           |
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// |         hard_timeout        |            flags               |
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// |                           pad (4)                            |
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// |                           cookie                             |
/// |                                                               |
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// |                        packet_count                          |
/// |                                                               |
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// |                         byte_count                           |
/// |                                                               |
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// |                       match (variable)                       |
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// |                   instructions (variable)                    |
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// ```
#[derive(Debug, Clone)]
pub struct FlowStatsEntry {
    /// Table ID
    pub table_id: u8,
    /// Time flow has been alive (seconds)
    pub duration_sec: u32,
    /// Time flow has been alive (nanoseconds beyond duration_sec)
    pub duration_nsec: u32,
    /// Priority
    pub priority: u16,
    /// Idle timeout
    pub idle_timeout: u16,
    /// Hard timeout
    pub hard_timeout: u16,
    /// Flags
    pub flags: u16,
    /// Cookie
    pub cookie: u64,
    /// Packet count
    pub packet_count: u64,
    /// Byte count
    pub byte_count: u64,
    /// Match fields
    pub match_fields: Match,
    /// Instructions (raw bytes for now)
    pub instructions: Vec<u8>,
}

impl FlowStatsEntry {
    /// Fixed header size before match (48 bytes).
    pub const FIXED_SIZE: usize = 48;

    /// Parse a single flow stats entry from bytes.
    #[allow(clippy::similar_names)]
    pub fn decode(data: &[u8]) -> crate::Result<(Self, usize)> {
        if data.len() < Self::FIXED_SIZE {
            return Err(crate::Error::Parse("flow stats entry too short".into()));
        }

        let length = u16::from_be_bytes([data[0], data[1]]) as usize;
        if data.len() < length {
            return Err(crate::Error::Parse("flow stats entry truncated".into()));
        }

        let table_id = data[2];
        // data[3] is padding
        let duration_sec = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
        let duration_nsec = u32::from_be_bytes([data[8], data[9], data[10], data[11]]);
        let priority = u16::from_be_bytes([data[12], data[13]]);
        let idle_timeout = u16::from_be_bytes([data[14], data[15]]);
        let hard_timeout = u16::from_be_bytes([data[16], data[17]]);
        let flags = u16::from_be_bytes([data[18], data[19]]);
        // data[20..24] is padding
        let cookie = u64::from_be_bytes([
            data[24], data[25], data[26], data[27],
            data[28], data[29], data[30], data[31],
        ]);
        let packet_count = u64::from_be_bytes([
            data[32], data[33], data[34], data[35],
            data[36], data[37], data[38], data[39],
        ]);
        let byte_count = u64::from_be_bytes([
            data[40], data[41], data[42], data[43],
            data[44], data[45], data[46], data[47],
        ]);

        // Parse match
        let match_data = &data[Self::FIXED_SIZE..];
        let (match_fields, match_len) = Match::decode(match_data)?;

        // Instructions follow the match
        let instructions_start = Self::FIXED_SIZE + match_len;
        let instructions = data[instructions_start..length].to_vec();

        Ok((
            Self {
                table_id,
                duration_sec,
                duration_nsec,
                priority,
                idle_timeout,
                hard_timeout,
                flags,
                cookie,
                packet_count,
                byte_count,
                match_fields,
                instructions,
            },
            length,
        ))
    }

    /// Decode the instructions from the raw bytes.
    ///
    /// Returns the decoded instruction list. This is a separate method
    /// because decoding instructions may fail and is not always needed.
    pub fn decoded_instructions(&self) -> crate::Result<InstructionList> {
        InstructionList::decode(&self.instructions)
    }
}

/// Parse all flow stats entries from a multipart reply body.
pub fn parse_flow_stats_reply(body: &[u8]) -> crate::Result<(Vec<FlowStatsEntry>, bool)> {
    if body.len() < MultipartHeader::SIZE {
        return Err(crate::Error::Parse("multipart reply too short".into()));
    }

    let header = MultipartHeader::decode(body)?;
    if header.mp_type != MultipartType::Flow {
        return Err(crate::Error::Parse(format!(
            "expected Flow multipart type, got {:?}",
            header.mp_type
        )));
    }

    let mut entries = Vec::new();
    let mut offset = MultipartHeader::SIZE;

    while offset < body.len() {
        let remaining = &body[offset..];
        if remaining.len() < 4 {
            break; // Not enough data for another entry
        }

        let (entry, consumed) = FlowStatsEntry::decode(remaining)?;
        entries.push(entry);
        offset += consumed;
    }

    Ok((entries, header.has_more()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multipart_type_values() {
        assert_eq!(MultipartType::Desc as u16, 0);
        assert_eq!(MultipartType::Flow as u16, 1);
        assert_eq!(MultipartType::Aggregate as u16, 2);
        assert_eq!(MultipartType::Table as u16, 3);
        assert_eq!(MultipartType::PortStats as u16, 4);
    }

    #[test]
    fn multipart_header_encode() {
        let header = MultipartHeader {
            mp_type: MultipartType::Flow,
            flags: 0,
        };
        let bytes = header.encode();
        assert_eq!(bytes.len(), 8);
        assert_eq!(bytes[0..2], [0x00, 0x01]); // Flow = 1
        assert_eq!(bytes[2..4], [0x00, 0x00]); // flags = 0
        assert_eq!(bytes[4..8], [0x00, 0x00, 0x00, 0x00]); // padding
    }

    #[test]
    fn multipart_header_decode() {
        let data = [0x00, 0x01, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00];
        let header = MultipartHeader::decode(&data).unwrap();
        assert_eq!(header.mp_type, MultipartType::Flow);
        assert!(header.has_more());
    }

    #[test]
    fn flow_stats_request_default() {
        let req = FlowStatsRequest::new();
        assert_eq!(req.table_id, 0xff);
        assert_eq!(req.out_port, OFPP_ANY);
        assert_eq!(req.cookie, 0);
        assert_eq!(req.cookie_mask, 0);
    }

    #[test]
    fn flow_stats_request_encode() {
        let req = FlowStatsRequest::new().table(0);
        let bytes = req.encode();

        // 32 bytes fixed + 8 bytes empty match (type=1, len=4, pad=4)
        assert!(bytes.len() >= 32);
        assert_eq!(bytes[0], 0); // table_id
    }

    #[test]
    fn flow_stats_request_to_message() {
        let req = FlowStatsRequest::new();
        let msg = req.to_message(Version::Of13, 42);

        assert_eq!(msg.header.msg_type, MessageType::MultipartRequest);
        assert_eq!(msg.header.xid, 42);
        // Body should have multipart header (8) + request body (32+)
        assert!(msg.body.len() >= 40);
    }

    #[test]
    fn flow_stats_entry_decode() {
        // Minimal flow stats entry: 48 fixed + 8 empty match = 56 bytes
        let mut data = vec![0u8; 56];

        // length = 56
        data[0] = 0x00;
        data[1] = 0x38;

        // table_id = 0
        data[2] = 0x00;

        // priority = 100
        data[12] = 0x00;
        data[13] = 0x64;

        // cookie = 0x1234
        data[24] = 0x00;
        data[25] = 0x00;
        data[26] = 0x00;
        data[27] = 0x00;
        data[28] = 0x00;
        data[29] = 0x00;
        data[30] = 0x12;
        data[31] = 0x34;

        // packet_count = 1000
        data[32..40].copy_from_slice(&1000u64.to_be_bytes());

        // byte_count = 64000
        data[40..48].copy_from_slice(&64000u64.to_be_bytes());

        // Match: type=1 (OXM), length=4, no fields, 4 bytes padding
        data[48] = 0x00;
        data[49] = 0x01;
        data[50] = 0x00;
        data[51] = 0x04;
        // padding
        data[52..56].copy_from_slice(&[0u8; 4]);

        let (entry, consumed) = FlowStatsEntry::decode(&data).unwrap();

        assert_eq!(consumed, 56);
        assert_eq!(entry.table_id, 0);
        assert_eq!(entry.priority, 100);
        assert_eq!(entry.cookie, 0x1234);
        assert_eq!(entry.packet_count, 1000);
        assert_eq!(entry.byte_count, 64000);
    }
}
