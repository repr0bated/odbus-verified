//! OpenFlow Packet-Out message encoding.
//!
//! Packet-Out messages are sent from the controller to the switch to inject
//! a packet into the datapath or to release a buffered packet.

use bytes::{BufMut, BytesMut};

use crate::action::ActionList;
use crate::message::{Header, Message, MessageType};
use crate::packet_in::OFP_NO_BUFFER;
use crate::Version;

/// Special input port value meaning "controller".
pub const OFPP_CONTROLLER: u32 = 0xffff_fffd;

/// Special input port value meaning "any" (no specific port).
pub const OFPP_ANY: u32 = 0xffff_ffff;

/// Packet-Out message builder.
#[derive(Debug, Clone)]
pub struct PacketOut {
    /// Buffer ID from Packet-In, or OFP_NO_BUFFER to send packet data.
    buffer_id: u32,
    /// Input port to use for action processing (affects output:IN_PORT).
    in_port: u32,
    /// Actions to apply to the packet.
    actions: ActionList,
    /// Packet data (required if buffer_id is OFP_NO_BUFFER).
    data: Vec<u8>,
}

impl PacketOut {
    /// Create a new Packet-Out with no buffer (packet data will be provided).
    pub fn new() -> Self {
        Self {
            buffer_id: OFP_NO_BUFFER,
            in_port: OFPP_CONTROLLER,
            actions: ActionList::new(),
            data: Vec::new(),
        }
    }

    /// Create a Packet-Out that releases a buffered packet.
    pub fn from_buffer(buffer_id: u32) -> Self {
        Self {
            buffer_id,
            in_port: OFPP_CONTROLLER,
            actions: ActionList::new(),
            data: Vec::new(),
        }
    }

    /// Set the buffer ID (use OFP_NO_BUFFER to include packet data).
    pub fn buffer_id(mut self, id: u32) -> Self {
        self.buffer_id = id;
        self
    }

    /// Set the input port for action context.
    pub fn in_port(mut self, port: u32) -> Self {
        self.in_port = port;
        self
    }

    /// Set the actions to apply to the packet.
    pub fn actions(mut self, actions: ActionList) -> Self {
        self.actions = actions;
        self
    }

    /// Set the packet data (only used if buffer_id is OFP_NO_BUFFER).
    pub fn data(mut self, data: Vec<u8>) -> Self {
        self.data = data;
        self
    }

    /// Encode the Packet-Out to an OpenFlow message.
    pub fn to_message(&self, version: Version, xid: u32) -> Message {
        let mut buf = BytesMut::new();

        // Encode actions
        let actions_bytes = self.actions.encode();
        let actions_len = actions_bytes.len() as u16;

        // Packet-Out body:
        // - buffer_id: 4 bytes
        // - in_port: 4 bytes
        // - actions_len: 2 bytes
        // - pad: 6 bytes
        // - actions: variable
        // - data: variable (if buffer_id == OFP_NO_BUFFER)

        buf.put_u32(self.buffer_id);
        buf.put_u32(self.in_port);
        buf.put_u16(actions_len);
        buf.put_slice(&[0u8; 6]); // padding

        buf.extend_from_slice(&actions_bytes);

        // Include packet data if not using a buffer
        if self.buffer_id == OFP_NO_BUFFER {
            buf.extend_from_slice(&self.data);
        }

        let total_len = (Header::SIZE + buf.len()) as u16;

        let header = Header {
            version,
            msg_type: MessageType::PacketOut,
            length: total_len,
            xid,
        };

        Message {
            header,
            body: buf.freeze(),
        }
    }
}

impl Default for PacketOut {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packet_out_basic() {
        let pkt = PacketOut::new()
            .in_port(1)
            .actions(ActionList::new().output(2))
            .data(vec![0x00, 0x01, 0x02, 0x03]);

        let msg = pkt.to_message(Version::Of13, 42);
        assert_eq!(msg.header.msg_type, MessageType::PacketOut);
        assert_eq!(msg.header.xid, 42);
    }

    #[test]
    fn packet_out_from_buffer() {
        let pkt = PacketOut::from_buffer(123)
            .in_port(5)
            .actions(ActionList::new().output(10));

        assert_eq!(pkt.buffer_id, 123);
        assert_eq!(pkt.in_port, 5);
    }
}
