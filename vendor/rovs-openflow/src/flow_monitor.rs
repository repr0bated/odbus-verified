//! Nicira Flow Monitor (NXST_FLOW_MONITOR).
//!
//! Provides event-driven flow table monitoring via the Nicira vendor extension.
//! A controller can register interest in flow table changes and receive
//! asynchronous notifications as flows are added, modified, or deleted.
//!
//! # Protocol
//!
//! The flow monitor uses the Multipart Experimenter framework:
//! - Request: Multipart Request with Nicira vendor header and NXST_FLOW_MONITOR subtype
//! - Reply: Continuous stream of Multipart Experimenter replies with flow update entries
//!
//! # Usage
//!
//! Open a dedicated VConn for monitoring (standard OVS practice):
//!
//! ```ignore
//! # use rovs_openflow::*;
//! # async fn example() -> Result<()> {
//! # let addr = rovs_transport::Address::Tcp("127.0.0.1:6653".parse().unwrap());
//! let mut mon = VConn::connect(&addr).await?;
//!
//! // Register monitor for all flow changes
//! let request = FlowMonitorRequest::all_changes(1);
//! let initial = mon.monitor_flows(request).await?;
//!
//! // Receive ongoing updates
//! loop {
//!     let updates = mon.recv_flow_updates().await?;
//!     for update in &updates {
//!         match update {
//!             FlowUpdate::Full(f) => println!("{:?}: table={} pri={}",
//!                 f.event, f.table_id, f.priority),
//!             FlowUpdate::Abbrev { xid } => println!("own change (xid={xid})"),
//!         }
//!     }
//! }
//! # }
//! ```

use bytes::Bytes;

use crate::action::NICIRA_VENDOR_ID;
use crate::multipart::{MultipartHeader, MultipartType};
use crate::message::{Message, MessageType};
use crate::{Match, Version};

/// Nicira stats subtype for flow monitor.
const NXST_FLOW_MONITOR: u32 = 2;

/// Flow monitor request flags (NXFMF_*).
pub mod monitor_flags {
    /// Send existing flows as initial ADDED events.
    pub const INITIAL: u16 = 1 << 0;
    /// Notify when a new flow is added.
    pub const ADD: u16 = 1 << 1;
    /// Notify when a flow is deleted.
    pub const DELETE: u16 = 1 << 2;
    /// Notify when a flow is modified.
    pub const MODIFY: u16 = 1 << 3;
    /// Include actions in update entries.
    pub const ACTIONS: u16 = 1 << 4;
    /// Include updates caused by the caller's own messages.
    pub const OWN: u16 = 1 << 5;
}

/// Flow update event type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlowUpdateEvent {
    /// Flow was added (or initial snapshot entry).
    Added,
    /// Flow was deleted.
    Deleted,
    /// Flow was modified.
    Modified,
}

impl FlowUpdateEvent {
    fn from_u16(v: u16) -> crate::Result<Self> {
        match v {
            0 => Ok(Self::Added),
            1 => Ok(Self::Deleted),
            2 => Ok(Self::Modified),
            _ => Err(crate::Error::Parse(format!("unknown flow update event: {v}"))),
        }
    }
}

/// A flow update received from the monitor.
#[derive(Debug)]
pub enum FlowUpdate {
    /// A flow was added, deleted, or modified.
    Full(Box<FlowUpdateFull>),
    /// Abbreviated notification for the caller's own changes.
    Abbrev {
        /// The xid of the message that caused the update.
        xid: u32,
    },
}

/// Full flow update entry (ADDED, DELETED, or MODIFIED).
#[derive(Debug)]
pub struct FlowUpdateFull {
    /// The type of event.
    pub event: FlowUpdateEvent,
    /// Reason for deletion (OFPRR_* value). Only meaningful for Deleted events.
    pub reason: u16,
    /// Flow priority.
    pub priority: u16,
    /// Idle timeout (seconds).
    pub idle_timeout: u16,
    /// Hard timeout (seconds).
    pub hard_timeout: u16,
    /// Table ID.
    pub table_id: u8,
    /// Flow cookie.
    pub cookie: u64,
    /// Match fields.
    pub match_fields: Match,
    /// Actions (raw bytes). Only present if NXFMF_ACTIONS was set.
    pub actions: Vec<u8>,
}

/// Builder for a flow monitor request.
///
/// # Wire Format
///
/// ```text
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// |                          monitor_id                           |
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// |            flags            |           out_port              |
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// |          match_len          |   table_id  |     pad           |
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// |                    match (NXM/OXM TLVs)                       |
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// ```
#[derive(Debug, Clone)]
pub struct FlowMonitorRequest {
    /// Controller-assigned monitor ID.
    pub id: u32,
    /// Monitor flags (NXFMF_*).
    pub flags: u16,
    /// Required output port filter, or 0xffff for any.
    pub out_port: u16,
    /// Table to monitor, or 0xff for all tables.
    pub table_id: u8,
    /// Match fields filter.
    pub match_fields: Match,
}

impl FlowMonitorRequest {
    /// Create a new flow monitor request with the given ID.
    pub fn new(id: u32) -> Self {
        Self {
            id,
            flags: 0,
            out_port: 0xffff, // OFPP_NONE (16-bit)
            table_id: 0xff,   // All tables
            match_fields: Match::new(),
        }
    }

    /// Create a monitor for all flow changes in all tables.
    ///
    /// Enables INITIAL, ADD, DELETE, MODIFY, and ACTIONS flags.
    pub fn all_changes(id: u32) -> Self {
        Self::new(id).flags(
            monitor_flags::INITIAL
                | monitor_flags::ADD
                | monitor_flags::DELETE
                | monitor_flags::MODIFY
                | monitor_flags::ACTIONS,
        )
    }

    /// Set monitor flags.
    pub fn flags(mut self, flags: u16) -> Self {
        self.flags = flags;
        self
    }

    /// Filter by table ID (0xff for all tables).
    pub fn table(mut self, table_id: u8) -> Self {
        self.table_id = table_id;
        self
    }

    /// Filter by match fields.
    pub fn match_fields(mut self, m: Match) -> Self {
        self.match_fields = m;
        self
    }

    /// Filter by output port.
    pub fn out_port(mut self, port: u16) -> Self {
        self.out_port = port;
        self
    }

    /// Encode the monitor request body (without multipart/vendor headers).
    fn encode_body(&self) -> Vec<u8> {
        let match_bytes = self.match_fields.encode_oxm_fields();
        let match_len = match_bytes.len() as u16;

        let mut buf = Vec::with_capacity(12 + match_bytes.len());

        // id (4)
        buf.extend(self.id.to_be_bytes());
        // flags (2)
        buf.extend(self.flags.to_be_bytes());
        // out_port (2)
        buf.extend(self.out_port.to_be_bytes());
        // match_len (2)
        buf.extend(match_len.to_be_bytes());
        // table_id (1)
        buf.push(self.table_id);
        // pad (1)
        buf.push(0);
        // match fields (variable, NXM/OXM TLVs, no match header)
        buf.extend(match_bytes);

        buf
    }

    /// Create the complete multipart request message.
    pub fn to_message(&self, version: Version, xid: u32) -> Message {
        let mp_header = MultipartHeader {
            mp_type: MultipartType::Experimenter,
            flags: 0,
        };

        let mut body = Vec::new();
        // Multipart header (8 bytes)
        body.extend(mp_header.encode());
        // Experimenter header: vendor (4) + subtype (4)
        body.extend(NICIRA_VENDOR_ID.to_be_bytes());
        body.extend(NXST_FLOW_MONITOR.to_be_bytes());
        // Monitor request body
        body.extend(self.encode_body());

        Message::new(version, MessageType::MultipartRequest, xid, Bytes::from(body))
    }
}

/// Parse flow monitor update entries from a multipart experimenter reply body.
///
/// Returns the parsed updates and whether the MORE flag is set.
pub fn parse_flow_monitor_reply(body: &[u8]) -> crate::Result<(Vec<FlowUpdate>, bool)> {
    if body.len() < MultipartHeader::SIZE + 8 {
        return Err(crate::Error::Parse(
            "flow monitor reply too short".into(),
        ));
    }

    // Multipart header
    let mp_header = MultipartHeader::decode(body)?;
    if mp_header.mp_type != MultipartType::Experimenter {
        return Err(crate::Error::Parse(format!(
            "expected Experimenter multipart type, got {:?}",
            mp_header.mp_type
        )));
    }

    let offset = MultipartHeader::SIZE;

    // Vendor header
    let vendor = u32::from_be_bytes([
        body[offset], body[offset + 1], body[offset + 2], body[offset + 3],
    ]);
    let subtype = u32::from_be_bytes([
        body[offset + 4], body[offset + 5], body[offset + 6], body[offset + 7],
    ]);

    if vendor != NICIRA_VENDOR_ID {
        return Err(crate::Error::Parse(format!(
            "expected Nicira vendor ID 0x{NICIRA_VENDOR_ID:08x}, got 0x{vendor:08x}"
        )));
    }
    if subtype != NXST_FLOW_MONITOR {
        return Err(crate::Error::Parse(format!(
            "expected NXST_FLOW_MONITOR subtype {NXST_FLOW_MONITOR}, got {subtype}"
        )));
    }

    // Parse flow update entries
    let mut updates = Vec::new();
    let mut pos = offset + 8; // After vendor header

    while pos + 4 <= body.len() {
        let entry_len = u16::from_be_bytes([body[pos], body[pos + 1]]) as usize;
        let event_code = u16::from_be_bytes([body[pos + 2], body[pos + 3]]);

        if entry_len < 4 || pos + entry_len > body.len() {
            break;
        }

        let update = if event_code == 3 {
            // ABBREV
            if entry_len < 8 {
                return Err(crate::Error::Parse("flow update abbrev too short".into()));
            }
            let xid = u32::from_be_bytes([
                body[pos + 4], body[pos + 5], body[pos + 6], body[pos + 7],
            ]);
            FlowUpdate::Abbrev { xid }
        } else {
            // ADDED (0), DELETED (1), MODIFIED (2)
            parse_flow_update_full(&body[pos..pos + entry_len], event_code)?
        };

        updates.push(update);
        pos += entry_len;
    }

    Ok((updates, mp_header.has_more()))
}

/// Parse a full flow update entry.
///
/// Wire format:
/// ```text
/// length(2) + event(2) + reason(2) + priority(2) +
/// idle_timeout(2) + hard_timeout(2) + match_len(2) +
/// table_id(1) + pad(1) + cookie(8) = 24 bytes fixed
/// + match NXM TLVs (match_len bytes)
/// + actions (remaining bytes)
/// ```
fn parse_flow_update_full(data: &[u8], event_code: u16) -> crate::Result<FlowUpdate> {
    const FIXED_SIZE: usize = 24;

    if data.len() < FIXED_SIZE {
        return Err(crate::Error::Parse("flow update entry too short".into()));
    }

    let entry_len = u16::from_be_bytes([data[0], data[1]]) as usize;
    let event = FlowUpdateEvent::from_u16(event_code)?;
    let reason = u16::from_be_bytes([data[4], data[5]]);
    let priority = u16::from_be_bytes([data[6], data[7]]);
    let idle_timeout = u16::from_be_bytes([data[8], data[9]]);
    let hard_timeout = u16::from_be_bytes([data[10], data[11]]);
    let match_len = u16::from_be_bytes([data[12], data[13]]) as usize;
    let table_id = data[14];
    // data[15] is padding
    let cookie = u64::from_be_bytes([
        data[16], data[17], data[18], data[19],
        data[20], data[21], data[22], data[23],
    ]);

    // Match fields (NXM/OXM TLVs, no match header)
    let match_end = FIXED_SIZE + match_len;
    if match_end > entry_len {
        return Err(crate::Error::Parse("flow update match truncated".into()));
    }
    let match_fields = Match::decode_oxm(&data[FIXED_SIZE..match_end])?;

    // Actions (remaining bytes after match)
    let actions = if match_end < entry_len {
        data[match_end..entry_len].to_vec()
    } else {
        Vec::new()
    };

    Ok(FlowUpdate::Full(Box::new(FlowUpdateFull {
        event,
        reason,
        priority,
        idle_timeout,
        hard_timeout,
        table_id,
        cookie,
        match_fields,
        actions,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn monitor_request_defaults() {
        let req = FlowMonitorRequest::new(1);
        assert_eq!(req.id, 1);
        assert_eq!(req.flags, 0);
        assert_eq!(req.out_port, 0xffff);
        assert_eq!(req.table_id, 0xff);
    }

    #[test]
    fn monitor_request_all_changes() {
        let req = FlowMonitorRequest::all_changes(42);
        assert_eq!(req.id, 42);
        assert_eq!(
            req.flags,
            monitor_flags::INITIAL
                | monitor_flags::ADD
                | monitor_flags::DELETE
                | monitor_flags::MODIFY
                | monitor_flags::ACTIONS
        );
    }

    #[test]
    fn monitor_request_builder() {
        let req = FlowMonitorRequest::new(1)
            .flags(monitor_flags::ADD | monitor_flags::DELETE)
            .table(0)
            .out_port(1);

        assert_eq!(req.table_id, 0);
        assert_eq!(req.out_port, 1);
        assert_eq!(req.flags, monitor_flags::ADD | monitor_flags::DELETE);
    }

    #[test]
    fn monitor_request_encode_body() {
        let req = FlowMonitorRequest::new(1)
            .flags(monitor_flags::ADD);
        let body = req.encode_body();

        // id (4) + flags (2) + out_port (2) + match_len (2) + table_id (1) + pad (1) = 12
        // + 0 match bytes (empty match)
        assert_eq!(body.len(), 12);

        // id = 1
        assert_eq!(body[0..4], [0, 0, 0, 1]);
        // flags = ADD (2)
        assert_eq!(body[4..6], [0, 2]);
        // out_port = 0xffff
        assert_eq!(body[6..8], [0xff, 0xff]);
        // match_len = 0
        assert_eq!(body[8..10], [0, 0]);
        // table_id = 0xff
        assert_eq!(body[10], 0xff);
        // pad
        assert_eq!(body[11], 0);
    }

    #[test]
    fn monitor_request_to_message() {
        let req = FlowMonitorRequest::all_changes(1);
        let msg = req.to_message(Version::Of13, 10);

        assert_eq!(msg.header.msg_type, MessageType::MultipartRequest);
        assert_eq!(msg.header.xid, 10);

        // Body: multipart header (8) + vendor header (8) + request body (12+)
        assert!(msg.body.len() >= 28);

        // Check multipart type = Experimenter (0xffff)
        assert_eq!(msg.body[0..2], [0xff, 0xff]);
        // Check vendor ID
        assert_eq!(msg.body[8..12], NICIRA_VENDOR_ID.to_be_bytes());
        // Check subtype = NXST_FLOW_MONITOR (2)
        assert_eq!(msg.body[12..16], 2u32.to_be_bytes());
    }

    #[test]
    fn parse_flow_update_added() {
        // Build a minimal flow monitor reply
        let mut body = Vec::new();

        // Multipart header: type=Experimenter, flags=0, pad=0
        body.extend([0xff, 0xff]); // type
        body.extend([0x00, 0x00]); // flags
        body.extend([0x00, 0x00, 0x00, 0x00]); // pad

        // Vendor header
        body.extend(NICIRA_VENDOR_ID.to_be_bytes());
        body.extend(NXST_FLOW_MONITOR.to_be_bytes());

        // Flow update entry: ADDED, priority=100, table=0, no match, no actions
        let entry_len: u16 = 24; // fixed size, no match, no actions
        body.extend(entry_len.to_be_bytes()); // length
        body.extend(0u16.to_be_bytes());      // event = ADDED
        body.extend(0u16.to_be_bytes());      // reason
        body.extend(100u16.to_be_bytes());    // priority
        body.extend(0u16.to_be_bytes());      // idle_timeout
        body.extend(0u16.to_be_bytes());      // hard_timeout
        body.extend(0u16.to_be_bytes());      // match_len = 0
        body.push(0);                         // table_id
        body.push(0);                         // pad
        body.extend(0x1234u64.to_be_bytes()); // cookie

        let (updates, has_more) = parse_flow_monitor_reply(&body).unwrap();
        assert!(!has_more);
        assert_eq!(updates.len(), 1);

        match &updates[0] {
            FlowUpdate::Full(f) => {
                assert_eq!(f.event, FlowUpdateEvent::Added);
                assert_eq!(f.priority, 100);
                assert_eq!(f.table_id, 0);
                assert_eq!(f.cookie, 0x1234);
                assert!(f.actions.is_empty());
            }
            FlowUpdate::Abbrev { .. } => panic!("expected Full update"),
        }
    }

    #[test]
    fn parse_flow_update_abbrev() {
        let mut body = Vec::new();

        // Multipart header
        body.extend([0xff, 0xff, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]);
        // Vendor header
        body.extend(NICIRA_VENDOR_ID.to_be_bytes());
        body.extend(NXST_FLOW_MONITOR.to_be_bytes());

        // ABBREV entry: length=8, event=3, xid=42
        body.extend(8u16.to_be_bytes());  // length
        body.extend(3u16.to_be_bytes());  // event = ABBREV
        body.extend(42u32.to_be_bytes()); // xid

        let (updates, _) = parse_flow_monitor_reply(&body).unwrap();
        assert_eq!(updates.len(), 1);

        match &updates[0] {
            FlowUpdate::Abbrev { xid } => assert_eq!(*xid, 42),
            FlowUpdate::Full(_) => panic!("expected Abbrev update"),
        }
    }

    #[test]
    fn parse_multiple_updates() {
        let mut body = Vec::new();

        // Multipart header with MORE flag
        body.extend([0xff, 0xff]); // type = Experimenter
        body.extend([0x00, 0x01]); // flags = MORE
        body.extend([0x00, 0x00, 0x00, 0x00]); // pad

        // Vendor header
        body.extend(NICIRA_VENDOR_ID.to_be_bytes());
        body.extend(NXST_FLOW_MONITOR.to_be_bytes());

        // First entry: ADDED
        body.extend(24u16.to_be_bytes());     // length
        body.extend(0u16.to_be_bytes());      // event = ADDED
        body.extend(0u16.to_be_bytes());      // reason
        body.extend(200u16.to_be_bytes());    // priority
        body.extend(0u16.to_be_bytes());      // idle_timeout
        body.extend(0u16.to_be_bytes());      // hard_timeout
        body.extend(0u16.to_be_bytes());      // match_len
        body.push(1);                         // table_id
        body.push(0);                         // pad
        body.extend(0xABCDu64.to_be_bytes()); // cookie

        // Second entry: DELETED
        body.extend(24u16.to_be_bytes());     // length
        body.extend(1u16.to_be_bytes());      // event = DELETED
        body.extend(3u16.to_be_bytes());      // reason = OFPRR_DELETE
        body.extend(50u16.to_be_bytes());     // priority
        body.extend(10u16.to_be_bytes());     // idle_timeout
        body.extend(0u16.to_be_bytes());      // hard_timeout
        body.extend(0u16.to_be_bytes());      // match_len
        body.push(0);                         // table_id
        body.push(0);                         // pad
        body.extend(0u64.to_be_bytes());      // cookie

        let (updates, has_more) = parse_flow_monitor_reply(&body).unwrap();
        assert!(has_more);
        assert_eq!(updates.len(), 2);

        match &updates[0] {
            FlowUpdate::Full(f) => {
                assert_eq!(f.event, FlowUpdateEvent::Added);
                assert_eq!(f.priority, 200);
                assert_eq!(f.table_id, 1);
            }
            _ => panic!("expected Full"),
        }

        match &updates[1] {
            FlowUpdate::Full(f) => {
                assert_eq!(f.event, FlowUpdateEvent::Deleted);
                assert_eq!(f.reason, 3);
                assert_eq!(f.priority, 50);
            }
            _ => panic!("expected Full"),
        }
    }

    #[test]
    fn parse_wrong_vendor_id() {
        let mut body = Vec::new();
        body.extend([0xff, 0xff, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]);
        body.extend(0xDEAD_BEEFu32.to_be_bytes()); // wrong vendor
        body.extend(NXST_FLOW_MONITOR.to_be_bytes());

        assert!(parse_flow_monitor_reply(&body).is_err());
    }

    #[test]
    fn flow_update_event_values() {
        assert_eq!(FlowUpdateEvent::from_u16(0).unwrap(), FlowUpdateEvent::Added);
        assert_eq!(FlowUpdateEvent::from_u16(1).unwrap(), FlowUpdateEvent::Deleted);
        assert_eq!(FlowUpdateEvent::from_u16(2).unwrap(), FlowUpdateEvent::Modified);
        assert!(FlowUpdateEvent::from_u16(4).is_err());
    }
}
