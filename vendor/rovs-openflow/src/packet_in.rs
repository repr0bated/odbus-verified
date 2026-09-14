//! OpenFlow Packet-In message parsing.
//!
//! Packet-In messages are sent from the switch to the controller when a packet
//! matches a flow with `actions=CONTROLLER` or when there's a table miss.

use bytes::{Buf, Bytes};

use crate::match_fields::Match;
use crate::{Error, Result};

/// Buffer ID indicating no buffering (packet data is in the message).
pub const OFP_NO_BUFFER: u32 = 0xffff_ffff;

/// Reason codes for Packet-In messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PacketInReason {
    /// No matching flow (table-miss).
    NoMatch = 0,
    /// Action explicitly output to controller.
    Action = 1,
    /// Packet has invalid TTL.
    InvalidTtl = 2,
    /// Action set explicitly output to controller (OF 1.4+).
    ActionSet = 3,
    /// Group bucket explicitly output to controller (OF 1.4+).
    Group = 4,
    /// Packet sent for controller processing (OF 1.4+).
    PacketOut = 5,
}

impl TryFrom<u8> for PacketInReason {
    type Error = Error;

    fn try_from(v: u8) -> Result<Self> {
        match v {
            0 => Ok(Self::NoMatch),
            1 => Ok(Self::Action),
            2 => Ok(Self::InvalidTtl),
            3 => Ok(Self::ActionSet),
            4 => Ok(Self::Group),
            5 => Ok(Self::PacketOut),
            _ => Err(Error::Parse(format!("unknown packet-in reason: {v}"))),
        }
    }
}

/// Parsed Packet-In message.
#[derive(Debug, Clone)]
pub struct PacketIn {
    /// Buffer ID assigned by the switch, or `OFP_NO_BUFFER` if not buffered.
    pub buffer_id: u32,
    /// Total length of the packet (before truncation).
    pub total_len: u16,
    /// Reason the packet was sent to the controller.
    pub reason: PacketInReason,
    /// ID of the table that generated the Packet-In.
    pub table_id: u8,
    /// Cookie of the flow entry that caused the Packet-In.
    pub cookie: u64,
    /// Match fields from the packet.
    pub match_fields: Match,
    /// Packet data (may be truncated based on controller max_len).
    pub data: Vec<u8>,
}

impl PacketIn {
    /// Parse a Packet-In message from raw bytes (after the OpenFlow header).
    pub fn parse(mut buf: Bytes) -> Result<Self> {
        // Minimum size: buffer_id(4) + total_len(2) + reason(1) + table_id(1)
        //              + cookie(8) + match_type(2) + match_len(2) = 20 bytes
        if buf.remaining() < 20 {
            return Err(Error::Parse("packet-in too short".into()));
        }

        let buffer_id = buf.get_u32();
        let total_len = buf.get_u16();
        let reason = PacketInReason::try_from(buf.get_u8())?;
        let table_id = buf.get_u8();
        let cookie = buf.get_u64();

        // Parse match (OXM format)
        // Match header: type(2) + length(2)
        let match_type = buf.get_u16();
        let match_len = buf.get_u16();

        if match_type != 1 {
            // OFPMT_OXM = 1
            return Err(Error::Parse(format!(
                "unsupported match type: {match_type}",
            )));
        }

        // match_len includes the 4-byte header, OXM fields follow
        let oxm_len = match_len.saturating_sub(4) as usize;
        if buf.remaining() < oxm_len {
            return Err(Error::Parse("packet-in match truncated".into()));
        }

        let oxm_bytes = buf.copy_to_bytes(oxm_len);
        let match_fields = Match::decode_oxm(&oxm_bytes)?;

        // Skip padding to 8-byte alignment
        // Total match size with header is match_len, padded to 8 bytes
        let padded_match_len = (match_len as usize + 7) & !7;
        let padding = padded_match_len - match_len as usize;
        if buf.remaining() < padding {
            return Err(Error::Parse("packet-in padding missing".into()));
        }
        buf.advance(padding);

        // Skip 2 bytes of padding before data
        if buf.remaining() < 2 {
            return Err(Error::Parse("packet-in data padding missing".into()));
        }
        buf.advance(2);

        // Remaining bytes are packet data
        let data = buf.to_vec();

        Ok(Self {
            buffer_id,
            total_len,
            reason,
            table_id,
            cookie,
            match_fields,
            data,
        })
    }

    /// Check if the packet is buffered on the switch.
    pub fn is_buffered(&self) -> bool {
        self.buffer_id != OFP_NO_BUFFER
    }

    /// Get the input port from the match fields.
    pub fn in_port(&self) -> u32 {
        self.match_fields.in_port.unwrap_or(0)
    }

    /// Get the buffer ID, or None if not buffered.
    pub fn buffer_id(&self) -> Option<u32> {
        if self.buffer_id == OFP_NO_BUFFER {
            None
        } else {
            Some(self.buffer_id)
        }
    }

    /// Get the packet data.
    pub fn data(&self) -> &[u8] {
        &self.data
    }

    /// Create a PacketIn for testing.
    #[cfg(any(test, feature = "test-support"))]
    pub fn new_for_test(buffer_id: u32, in_port: u32, data: Vec<u8>) -> Self {
        Self {
            buffer_id,
            total_len: data.len() as u16,
            reason: PacketInReason::Action,
            table_id: 0,
            cookie: 0,
            match_fields: Match::new().in_port(in_port),
            data,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_packet_in_reason() {
        assert_eq!(PacketInReason::try_from(0).unwrap(), PacketInReason::NoMatch);
        assert_eq!(PacketInReason::try_from(1).unwrap(), PacketInReason::Action);
        assert!(PacketInReason::try_from(99).is_err());
    }
}
