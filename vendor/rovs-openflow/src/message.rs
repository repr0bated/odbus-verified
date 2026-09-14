//! OpenFlow message types.

use bytes::{Buf, BufMut, Bytes, BytesMut};

use crate::{Error, Result, Version};

/// OpenFlow message header (8 bytes).
#[derive(Debug, Clone, Copy)]
pub struct Header {
    /// Protocol version
    pub version: Version,
    /// Message type
    pub msg_type: MessageType,
    /// Total message length (including header)
    pub length: u16,
    /// Transaction ID
    pub xid: u32,
}

impl Header {
    /// Header size in bytes.
    pub const SIZE: usize = 8;

    /// Encode the header to bytes.
    pub fn encode(&self, buf: &mut BytesMut) {
        buf.put_u8(self.version.wire_version());
        buf.put_u8(self.msg_type as u8);
        buf.put_u16(self.length);
        buf.put_u32(self.xid);
    }

    /// Decode a header from bytes.
    pub fn decode(buf: &mut Bytes) -> Result<Self> {
        if buf.remaining() < Self::SIZE {
            return Err(Error::Parse("header too short".into()));
        }

        let version = Version::try_from(buf.get_u8())?;
        let msg_type = MessageType::try_from(buf.get_u8())?;
        let length = buf.get_u16();
        let xid = buf.get_u32();

        Ok(Self {
            version,
            msg_type,
            length,
            xid,
        })
    }
}

/// OpenFlow message types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum MessageType {
    // Symmetric messages
    /// Hello message
    Hello = 0,
    /// Error message
    Error = 1,
    /// Echo request
    EchoRequest = 2,
    /// Echo reply
    EchoReply = 3,
    /// Experimenter message
    Experimenter = 4,

    // Controller-to-switch messages
    /// Features request
    FeaturesRequest = 5,
    /// Features reply
    FeaturesReply = 6,
    /// Get config request
    GetConfigRequest = 7,
    /// Get config reply
    GetConfigReply = 8,
    /// Set config
    SetConfig = 9,

    // Async messages
    /// Packet in
    PacketIn = 10,
    /// Flow removed
    FlowRemoved = 11,
    /// Port status
    PortStatus = 12,

    // Controller-to-switch messages
    /// Packet out
    PacketOut = 13,
    /// Flow mod
    FlowMod = 14,
    /// Group mod (OF 1.1+)
    GroupMod = 15,
    /// Port mod
    PortMod = 16,
    /// Table mod (OF 1.1+)
    TableMod = 17,

    // Multipart messages (OF 1.3+)
    /// Multipart request
    MultipartRequest = 18,
    /// Multipart reply
    MultipartReply = 19,

    // Barrier
    /// Barrier request
    BarrierRequest = 20,
    /// Barrier reply
    BarrierReply = 21,

    // Controller role (OF 1.2+)
    /// Role request
    RoleRequest = 24,
    /// Role reply
    RoleReply = 25,

    // Async config (OF 1.3+)
    /// Get async request
    GetAsyncRequest = 26,
    /// Get async reply
    GetAsyncReply = 27,
    /// Set async
    SetAsync = 28,

    // Meters (OF 1.3+)
    /// Meter mod
    MeterMod = 29,

    // Bundle (OF 1.4+)
    /// Bundle control
    BundleControl = 33,
    /// Bundle add message
    BundleAddMessage = 34,
}

impl TryFrom<u8> for MessageType {
    type Error = Error;

    fn try_from(v: u8) -> Result<Self> {
        match v {
            0 => Ok(Self::Hello),
            1 => Ok(Self::Error),
            2 => Ok(Self::EchoRequest),
            3 => Ok(Self::EchoReply),
            4 => Ok(Self::Experimenter),
            5 => Ok(Self::FeaturesRequest),
            6 => Ok(Self::FeaturesReply),
            7 => Ok(Self::GetConfigRequest),
            8 => Ok(Self::GetConfigReply),
            9 => Ok(Self::SetConfig),
            10 => Ok(Self::PacketIn),
            11 => Ok(Self::FlowRemoved),
            12 => Ok(Self::PortStatus),
            13 => Ok(Self::PacketOut),
            14 => Ok(Self::FlowMod),
            15 => Ok(Self::GroupMod),
            16 => Ok(Self::PortMod),
            17 => Ok(Self::TableMod),
            18 => Ok(Self::MultipartRequest),
            19 => Ok(Self::MultipartReply),
            20 => Ok(Self::BarrierRequest),
            21 => Ok(Self::BarrierReply),
            24 => Ok(Self::RoleRequest),
            25 => Ok(Self::RoleReply),
            26 => Ok(Self::GetAsyncRequest),
            27 => Ok(Self::GetAsyncReply),
            28 => Ok(Self::SetAsync),
            29 => Ok(Self::MeterMod),
            33 => Ok(Self::BundleControl),
            34 => Ok(Self::BundleAddMessage),
            _ => Err(Error::InvalidMessage(format!("unknown message type: {v}"))),
        }
    }
}

/// A complete OpenFlow message.
#[derive(Debug)]
pub struct Message {
    /// Message header
    pub header: Header,
    /// Message body
    pub body: Bytes,
}

impl Message {
    /// Create a new message.
    pub fn new(version: Version, msg_type: MessageType, xid: u32, body: Bytes) -> Self {
        let length = (Header::SIZE + body.len()) as u16;
        Self {
            header: Header {
                version,
                msg_type,
                length,
                xid,
            },
            body,
        }
    }

    /// Encode the message to bytes.
    pub fn encode(&self) -> BytesMut {
        let mut buf = BytesMut::with_capacity(self.header.length as usize);
        self.header.encode(&mut buf);
        buf.extend_from_slice(&self.body);
        buf
    }
}
