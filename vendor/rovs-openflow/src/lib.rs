// OpenFlow implementation is work-in-progress
#![allow(clippy::must_use_candidate)]
#![allow(clippy::return_self_not_must_use)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::struct_excessive_bools)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::unused_async)]
#![allow(clippy::bool_to_int_with_if)]
#![allow(clippy::cast_lossless)]

//! `OpenFlow` protocol implementation for OVS.
//!
//! Provides:
//! - `OpenFlow` message encoding/decoding
//! - Match field builder
//! - Action types
//! - Flow modification
//! - Virtual connection (`VConn`) abstraction

mod action;
mod error;
mod flow;
mod flow_monitor;
mod instruction;
mod match_fields;
mod message;
mod multipart;
pub mod ndp;
pub mod oxm;
mod packet_in;
mod packet_out;
mod vconn;

pub use action::{nxm, Action, ActionList, LearnSpec, NatConfig, NxLearn, OutputPort, CT_COMMIT};
pub use error::{Error, OfError, OfErrorType};
pub use flow::{Flow, FlowCommand, FlowFlags, FlowStats};
pub use flow_monitor::{monitor_flags, FlowMonitorRequest, FlowUpdate, FlowUpdateEvent, FlowUpdateFull};
pub use instruction::{Instruction, InstructionList};
pub use match_fields::Match;
pub use message::{Header, Message, MessageType};
pub use multipart::{FlowStatsEntry, FlowStatsRequest, MultipartType};
pub use packet_in::{PacketIn, PacketInReason, OFP_NO_BUFFER};
pub use packet_out::{PacketOut, OFPP_CONTROLLER, OFPP_ANY};
pub use vconn::VConn;

pub type Result<T> = std::result::Result<T, Error>;

/// OpenFlow protocol versions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum Version {
    /// OpenFlow 1.0
    Of10 = 0x01,
    /// OpenFlow 1.1
    Of11 = 0x02,
    /// OpenFlow 1.2
    Of12 = 0x03,
    /// OpenFlow 1.3
    Of13 = 0x04,
    /// OpenFlow 1.4
    Of14 = 0x05,
    /// OpenFlow 1.5
    Of15 = 0x06,
}

impl Version {
    /// Get the wire format version number.
    pub fn wire_version(self) -> u8 {
        self as u8
    }
}

impl TryFrom<u8> for Version {
    type Error = Error;

    fn try_from(v: u8) -> Result<Self> {
        match v {
            0x01 => Ok(Self::Of10),
            0x02 => Ok(Self::Of11),
            0x03 => Ok(Self::Of12),
            0x04 => Ok(Self::Of13),
            0x05 => Ok(Self::Of14),
            0x06 => Ok(Self::Of15),
            _ => Err(Error::UnsupportedVersion(v)),
        }
    }
}
