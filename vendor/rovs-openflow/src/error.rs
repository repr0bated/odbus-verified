//! OpenFlow error types.

use thiserror::Error;

/// Errors that can occur in OpenFlow operations.
#[derive(Debug, Error)]
pub enum Error {
    /// Transport error
    #[error("transport error: {0}")]
    Transport(#[from] rovs_transport::Error),

    /// I/O error
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Unsupported protocol version
    #[error("unsupported OpenFlow version: {0}")]
    UnsupportedVersion(u8),

    /// Message parsing error
    #[error("parse error: {0}")]
    Parse(String),

    /// Invalid message
    #[error("invalid message: {0}")]
    InvalidMessage(String),

    /// Connection closed
    #[error("connection closed")]
    ConnectionClosed,

    /// Timeout
    #[error("timeout")]
    Timeout,

    /// OpenFlow error from switch
    #[error("{0}")]
    OfError(OfError),
}

/// OpenFlow error message from the switch.
#[derive(Debug, Clone)]
pub struct OfError {
    /// Error type
    pub error_type: OfErrorType,
    /// Error code (type-specific)
    pub code: u16,
    /// Original request data (up to 64 bytes)
    pub data: Vec<u8>,
}

impl std::fmt::Display for OfError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: ", self.error_type)?;
        match self.error_type {
            OfErrorType::HelloFailed => write!(f, "{}", HelloFailedCode::from(self.code)),
            OfErrorType::BadRequest => write!(f, "{}", BadRequestCode::from(self.code)),
            OfErrorType::BadAction => write!(f, "{}", BadActionCode::from(self.code)),
            OfErrorType::BadInstruction => write!(f, "{}", BadInstructionCode::from(self.code)),
            OfErrorType::BadMatch => write!(f, "{}", BadMatchCode::from(self.code)),
            OfErrorType::FlowModFailed => write!(f, "{}", FlowModFailedCode::from(self.code)),
            OfErrorType::GroupModFailed => write!(f, "{}", GroupModFailedCode::from(self.code)),
            OfErrorType::PortModFailed => write!(f, "{}", PortModFailedCode::from(self.code)),
            OfErrorType::TableModFailed => write!(f, "{}", TableModFailedCode::from(self.code)),
            OfErrorType::MeterModFailed => write!(f, "{}", MeterModFailedCode::from(self.code)),
            OfErrorType::TableFeaturesFailed => {
                write!(f, "{}", TableFeaturesFailedCode::from(self.code))
            }
            OfErrorType::Unknown(_) => write!(f, "code {}", self.code),
        }
    }
}

impl OfError {
    /// Parse an OpenFlow error from message body.
    pub fn parse(body: &[u8]) -> Result<Self, Error> {
        if body.len() < 4 {
            return Err(Error::Parse("error message too short".into()));
        }

        let error_type = u16::from_be_bytes([body[0], body[1]]);
        let code = u16::from_be_bytes([body[2], body[3]]);
        let data = body.get(4..).unwrap_or(&[]).to_vec();

        Ok(Self {
            error_type: OfErrorType::from(error_type),
            code,
            data,
        })
    }
}

/// OpenFlow error types (OF 1.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OfErrorType {
    /// Hello protocol failed
    HelloFailed,
    /// Request was not understood
    BadRequest,
    /// Error in action description
    BadAction,
    /// Error in instruction list
    BadInstruction,
    /// Error in match
    BadMatch,
    /// Problem modifying flow entry
    FlowModFailed,
    /// Problem modifying group entry
    GroupModFailed,
    /// Port mod request failed
    PortModFailed,
    /// Table mod request failed
    TableModFailed,
    /// Meter mod request failed
    MeterModFailed,
    /// Table features request failed
    TableFeaturesFailed,
    /// Unknown error type
    Unknown(u16),
}

impl From<u16> for OfErrorType {
    fn from(v: u16) -> Self {
        match v {
            0 => Self::HelloFailed,
            1 => Self::BadRequest,
            2 => Self::BadAction,
            3 => Self::BadInstruction,
            4 => Self::BadMatch,
            5 => Self::FlowModFailed,
            6 => Self::GroupModFailed,
            7 => Self::PortModFailed,
            8 => Self::TableModFailed,
            12 => Self::MeterModFailed,
            13 => Self::TableFeaturesFailed,
            _ => Self::Unknown(v),
        }
    }
}

impl std::fmt::Display for OfErrorType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::HelloFailed => write!(f, "HELLO_FAILED"),
            Self::BadRequest => write!(f, "BAD_REQUEST"),
            Self::BadAction => write!(f, "BAD_ACTION"),
            Self::BadInstruction => write!(f, "BAD_INSTRUCTION"),
            Self::BadMatch => write!(f, "BAD_MATCH"),
            Self::FlowModFailed => write!(f, "FLOW_MOD_FAILED"),
            Self::GroupModFailed => write!(f, "GROUP_MOD_FAILED"),
            Self::PortModFailed => write!(f, "PORT_MOD_FAILED"),
            Self::TableModFailed => write!(f, "TABLE_MOD_FAILED"),
            Self::MeterModFailed => write!(f, "METER_MOD_FAILED"),
            Self::TableFeaturesFailed => write!(f, "TABLE_FEATURES_FAILED"),
            Self::Unknown(v) => write!(f, "UNKNOWN({v})"),
        }
    }
}

/// Hello failed error codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HelloFailedCode {
    /// No compatible version
    Incompatible,
    /// Permissions error
    EpermError,
    /// Unknown code
    Unknown(u16),
}

impl From<u16> for HelloFailedCode {
    fn from(v: u16) -> Self {
        match v {
            0 => Self::Incompatible,
            1 => Self::EpermError,
            _ => Self::Unknown(v),
        }
    }
}

impl std::fmt::Display for HelloFailedCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Incompatible => write!(f, "incompatible version"),
            Self::EpermError => write!(f, "permissions error"),
            Self::Unknown(v) => write!(f, "unknown code {v}"),
        }
    }
}

/// Bad request error codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BadRequestCode {
    /// Unknown or unsupported version
    BadVersion,
    /// Unknown or unsupported message type
    BadType,
    /// Unknown or unsupported multipart type
    BadMultipart,
    /// Unknown or unsupported experimenter ID
    BadExperimenter,
    /// Unknown or unsupported experimenter type
    BadExpType,
    /// Permissions error
    Eperm,
    /// Wrong request length
    BadLen,
    /// Specified buffer does not exist
    BufferEmpty,
    /// Specified buffer already used
    BufferUnknown,
    /// Specified table-id invalid
    BadTableId,
    /// Denied because controller is slave
    IsSlave,
    /// Invalid port
    BadPort,
    /// Invalid packet in buffer-id
    BadPacket,
    /// Multipart request overflowed
    MultipartBufferOverflow,
    /// Unknown code
    Unknown(u16),
}

impl From<u16> for BadRequestCode {
    fn from(v: u16) -> Self {
        match v {
            0 => Self::BadVersion,
            1 => Self::BadType,
            2 => Self::BadMultipart,
            3 => Self::BadExperimenter,
            4 => Self::BadExpType,
            5 => Self::Eperm,
            6 => Self::BadLen,
            7 => Self::BufferEmpty,
            8 => Self::BufferUnknown,
            9 => Self::BadTableId,
            10 => Self::IsSlave,
            11 => Self::BadPort,
            12 => Self::BadPacket,
            13 => Self::MultipartBufferOverflow,
            _ => Self::Unknown(v),
        }
    }
}

impl std::fmt::Display for BadRequestCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BadVersion => write!(f, "unknown version"),
            Self::BadType => write!(f, "unknown message type"),
            Self::BadMultipart => write!(f, "unknown multipart type"),
            Self::BadExperimenter => write!(f, "unknown experimenter ID"),
            Self::BadExpType => write!(f, "unknown experimenter type"),
            Self::Eperm => write!(f, "permissions error"),
            Self::BadLen => write!(f, "wrong request length"),
            Self::BufferEmpty => write!(f, "buffer does not exist"),
            Self::BufferUnknown => write!(f, "buffer already used"),
            Self::BadTableId => write!(f, "invalid table ID"),
            Self::IsSlave => write!(f, "controller is slave"),
            Self::BadPort => write!(f, "invalid port"),
            Self::BadPacket => write!(f, "invalid packet in buffer-id"),
            Self::MultipartBufferOverflow => write!(f, "multipart request overflowed"),
            Self::Unknown(v) => write!(f, "unknown code {v}"),
        }
    }
}

/// Bad action error codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BadActionCode {
    /// Unknown action type
    BadType,
    /// Length problem in actions
    BadLen,
    /// Unknown experimenter ID
    BadExperimenter,
    /// Unknown action for experimenter ID
    BadExpType,
    /// Problem validating output port
    BadOutPort,
    /// Bad action argument
    BadArgument,
    /// Permissions error
    Eperm,
    /// Too many actions
    TooMany,
    /// Problem validating output queue
    BadQueue,
    /// Invalid group ID
    BadOutGroup,
    /// Action can't apply - match inconsistent
    MatchInconsistent,
    /// Action order unsupported for Apply-Actions
    UnsupportedOrder,
    /// Actions use unsupported tag/encap
    BadTag,
    /// Unsupported type in SET_FIELD
    BadSetType,
    /// Length problem in SET_FIELD
    BadSetLen,
    /// Bad argument in SET_FIELD
    BadSetArgument,
    /// Unknown code
    Unknown(u16),
}

impl From<u16> for BadActionCode {
    fn from(v: u16) -> Self {
        match v {
            0 => Self::BadType,
            1 => Self::BadLen,
            2 => Self::BadExperimenter,
            3 => Self::BadExpType,
            4 => Self::BadOutPort,
            5 => Self::BadArgument,
            6 => Self::Eperm,
            7 => Self::TooMany,
            8 => Self::BadQueue,
            9 => Self::BadOutGroup,
            10 => Self::MatchInconsistent,
            11 => Self::UnsupportedOrder,
            12 => Self::BadTag,
            13 => Self::BadSetType,
            14 => Self::BadSetLen,
            15 => Self::BadSetArgument,
            _ => Self::Unknown(v),
        }
    }
}

impl std::fmt::Display for BadActionCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BadType => write!(f, "unknown action type"),
            Self::BadLen => write!(f, "length problem in actions"),
            Self::BadExperimenter => write!(f, "unknown experimenter ID"),
            Self::BadExpType => write!(f, "unknown action for experimenter"),
            Self::BadOutPort => write!(f, "problem validating output port"),
            Self::BadArgument => write!(f, "bad action argument"),
            Self::Eperm => write!(f, "permissions error"),
            Self::TooMany => write!(f, "too many actions"),
            Self::BadQueue => write!(f, "problem validating output queue"),
            Self::BadOutGroup => write!(f, "invalid group ID"),
            Self::MatchInconsistent => write!(f, "action can't apply for this match"),
            Self::UnsupportedOrder => write!(f, "action order unsupported"),
            Self::BadTag => write!(f, "unsupported tag/encap"),
            Self::BadSetType => write!(f, "unsupported SET_FIELD type"),
            Self::BadSetLen => write!(f, "length problem in SET_FIELD"),
            Self::BadSetArgument => write!(f, "bad argument in SET_FIELD"),
            Self::Unknown(v) => write!(f, "unknown code {v}"),
        }
    }
}

/// Bad instruction error codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BadInstructionCode {
    /// Unknown instruction type
    UnknownInst,
    /// Switch does not support some prerequisite
    UnsupInst,
    /// Invalid table ID
    BadTableId,
    /// Metadata mask value unsupported
    UnsupMetadata,
    /// Metadata mask value unsupported in write
    UnsupMetadataMask,
    /// Unknown experimenter ID
    BadExperimenter,
    /// Unknown instruction for experimenter
    BadExpType,
    /// Length problem in instructions
    BadLen,
    /// Permissions error
    Eperm,
    /// Unknown code
    Unknown(u16),
}

impl From<u16> for BadInstructionCode {
    fn from(v: u16) -> Self {
        match v {
            0 => Self::UnknownInst,
            1 => Self::UnsupInst,
            2 => Self::BadTableId,
            3 => Self::UnsupMetadata,
            4 => Self::UnsupMetadataMask,
            5 => Self::BadExperimenter,
            6 => Self::BadExpType,
            7 => Self::BadLen,
            8 => Self::Eperm,
            _ => Self::Unknown(v),
        }
    }
}

impl std::fmt::Display for BadInstructionCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownInst => write!(f, "unknown instruction type"),
            Self::UnsupInst => write!(f, "unsupported instruction"),
            Self::BadTableId => write!(f, "invalid table ID"),
            Self::UnsupMetadata => write!(f, "metadata mask unsupported"),
            Self::UnsupMetadataMask => write!(f, "metadata mask unsupported in write"),
            Self::BadExperimenter => write!(f, "unknown experimenter ID"),
            Self::BadExpType => write!(f, "unknown instruction for experimenter"),
            Self::BadLen => write!(f, "length problem in instructions"),
            Self::Eperm => write!(f, "permissions error"),
            Self::Unknown(v) => write!(f, "unknown code {v}"),
        }
    }
}

/// Bad match error codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BadMatchCode {
    /// Unsupported match type
    BadType,
    /// Length problem in match
    BadLen,
    /// Match uses unsupported tag/encap
    BadTag,
    /// Unsupported datalink addr mask
    BadDlAddrMask,
    /// Unsupported network addr mask
    BadNwAddrMask,
    /// Unsupported combination of fields
    BadWildcards,
    /// Unsupported field type
    BadField,
    /// Unsupported value in a match field
    BadValue,
    /// Unsupported mask specified
    BadMask,
    /// A prerequisite was not met
    BadPrereq,
    /// A field appeared more than once
    DupField,
    /// Permissions error
    Eperm,
    /// Unknown code
    Unknown(u16),
}

impl From<u16> for BadMatchCode {
    fn from(v: u16) -> Self {
        match v {
            0 => Self::BadType,
            1 => Self::BadLen,
            2 => Self::BadTag,
            3 => Self::BadDlAddrMask,
            4 => Self::BadNwAddrMask,
            5 => Self::BadWildcards,
            6 => Self::BadField,
            7 => Self::BadValue,
            8 => Self::BadMask,
            9 => Self::BadPrereq,
            10 => Self::DupField,
            11 => Self::Eperm,
            _ => Self::Unknown(v),
        }
    }
}

impl std::fmt::Display for BadMatchCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BadType => write!(f, "unsupported match type"),
            Self::BadLen => write!(f, "length problem in match"),
            Self::BadTag => write!(f, "unsupported tag/encap"),
            Self::BadDlAddrMask => write!(f, "unsupported datalink address mask"),
            Self::BadNwAddrMask => write!(f, "unsupported network address mask"),
            Self::BadWildcards => write!(f, "unsupported field combination"),
            Self::BadField => write!(f, "unsupported field type"),
            Self::BadValue => write!(f, "unsupported value in match field"),
            Self::BadMask => write!(f, "unsupported mask specified"),
            Self::BadPrereq => write!(f, "prerequisite not met"),
            Self::DupField => write!(f, "field appeared more than once"),
            Self::Eperm => write!(f, "permissions error"),
            Self::Unknown(v) => write!(f, "unknown code {v}"),
        }
    }
}

/// Flow mod failed error codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlowModFailedCode {
    /// Unspecified error
    Unknown,
    /// Flow not added - table full
    TableFull,
    /// Table does not exist
    BadTableId,
    /// Attempted to add overlapping flow
    Overlap,
    /// Permissions error
    Eperm,
    /// Flow not added - idle/hard timeout
    BadTimeout,
    /// Unsupported or unknown command
    BadCommand,
    /// Unsupported or unknown flags
    BadFlags,
    /// Unknown code
    UnknownCode(u16),
}

impl From<u16> for FlowModFailedCode {
    fn from(v: u16) -> Self {
        match v {
            0 => Self::Unknown,
            1 => Self::TableFull,
            2 => Self::BadTableId,
            3 => Self::Overlap,
            4 => Self::Eperm,
            5 => Self::BadTimeout,
            6 => Self::BadCommand,
            7 => Self::BadFlags,
            _ => Self::UnknownCode(v),
        }
    }
}

impl std::fmt::Display for FlowModFailedCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unknown => write!(f, "unspecified error"),
            Self::TableFull => write!(f, "table full"),
            Self::BadTableId => write!(f, "table does not exist"),
            Self::Overlap => write!(f, "overlapping flow entry"),
            Self::Eperm => write!(f, "permissions error"),
            Self::BadTimeout => write!(f, "bad timeout"),
            Self::BadCommand => write!(f, "unsupported command"),
            Self::BadFlags => write!(f, "unsupported flags"),
            Self::UnknownCode(v) => write!(f, "unknown code {v}"),
        }
    }
}

/// Group mod failed error codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupModFailedCode {
    /// Group not added - already exists
    GroupExists,
    /// Group not added - invalid group
    InvalidGroup,
    /// Group not modified - does not exist
    UnknownGroup,
    /// Bucket weight unsupported
    WeightUnsupported,
    /// Group bucket maximum exceeded
    OutOfBuckets,
    /// Action bucket maximum exceeded
    OutOfGroups,
    /// Chaining not supported
    ChainingUnsupported,
    /// Watch bucket value unsupported
    WatchUnsupported,
    /// Group table full
    Loop,
    /// Group table full
    ChainsNotSupported,
    /// Unknown code
    Unknown(u16),
}

impl From<u16> for GroupModFailedCode {
    fn from(v: u16) -> Self {
        match v {
            0 => Self::GroupExists,
            1 => Self::InvalidGroup,
            2 => Self::WeightUnsupported,
            3 => Self::OutOfGroups,
            4 => Self::OutOfBuckets,
            5 => Self::ChainingUnsupported,
            6 => Self::WatchUnsupported,
            7 => Self::Loop,
            8 => Self::UnknownGroup,
            9 => Self::ChainsNotSupported,
            _ => Self::Unknown(v),
        }
    }
}

impl std::fmt::Display for GroupModFailedCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::GroupExists => write!(f, "group already exists"),
            Self::InvalidGroup => write!(f, "invalid group"),
            Self::UnknownGroup => write!(f, "group does not exist"),
            Self::WeightUnsupported => write!(f, "bucket weight unsupported"),
            Self::OutOfBuckets => write!(f, "bucket maximum exceeded"),
            Self::OutOfGroups => write!(f, "group maximum exceeded"),
            Self::ChainingUnsupported => write!(f, "chaining unsupported"),
            Self::WatchUnsupported => write!(f, "watch bucket unsupported"),
            Self::Loop => write!(f, "loop in group"),
            Self::ChainsNotSupported => write!(f, "chains not supported"),
            Self::Unknown(v) => write!(f, "unknown code {v}"),
        }
    }
}

/// Port mod failed error codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortModFailedCode {
    /// Specified port does not exist
    BadPort,
    /// Specified hardware address mismatch
    BadHwAddr,
    /// Specified config is invalid
    BadConfig,
    /// Specified advertise is invalid
    BadAdvertise,
    /// Permissions error
    Eperm,
    /// Unknown code
    Unknown(u16),
}

impl From<u16> for PortModFailedCode {
    fn from(v: u16) -> Self {
        match v {
            0 => Self::BadPort,
            1 => Self::BadHwAddr,
            2 => Self::BadConfig,
            3 => Self::BadAdvertise,
            4 => Self::Eperm,
            _ => Self::Unknown(v),
        }
    }
}

impl std::fmt::Display for PortModFailedCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BadPort => write!(f, "port does not exist"),
            Self::BadHwAddr => write!(f, "hardware address mismatch"),
            Self::BadConfig => write!(f, "invalid config"),
            Self::BadAdvertise => write!(f, "invalid advertise"),
            Self::Eperm => write!(f, "permissions error"),
            Self::Unknown(v) => write!(f, "unknown code {v}"),
        }
    }
}

/// Table mod failed error codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableModFailedCode {
    /// Specified table does not exist
    BadTable,
    /// Specified config is invalid
    BadConfig,
    /// Permissions error
    Eperm,
    /// Unknown code
    Unknown(u16),
}

impl From<u16> for TableModFailedCode {
    fn from(v: u16) -> Self {
        match v {
            0 => Self::BadTable,
            1 => Self::BadConfig,
            2 => Self::Eperm,
            _ => Self::Unknown(v),
        }
    }
}

impl std::fmt::Display for TableModFailedCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BadTable => write!(f, "table does not exist"),
            Self::BadConfig => write!(f, "invalid config"),
            Self::Eperm => write!(f, "permissions error"),
            Self::Unknown(v) => write!(f, "unknown code {v}"),
        }
    }
}

/// Meter mod failed error codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeterModFailedCode {
    /// Unspecified error
    Unknown,
    /// Meter not added - already exists
    MeterExists,
    /// Meter not added - invalid meter
    InvalidMeter,
    /// Meter not modified - does not exist
    UnknownMeter,
    /// Unsupported or unknown command
    BadCommand,
    /// Flag configuration unsupported
    BadFlags,
    /// Rate unsupported
    BadRate,
    /// Burst size unsupported
    BadBurst,
    /// Band unsupported
    BadBand,
    /// Band value unsupported
    BadBandValue,
    /// Meter maximum exceeded
    OutOfMeters,
    /// Maximum bands exceeded
    OutOfBands,
    /// Unknown code
    UnknownCode(u16),
}

impl From<u16> for MeterModFailedCode {
    fn from(v: u16) -> Self {
        match v {
            0 => Self::Unknown,
            1 => Self::MeterExists,
            2 => Self::InvalidMeter,
            3 => Self::UnknownMeter,
            4 => Self::BadCommand,
            5 => Self::BadFlags,
            6 => Self::BadRate,
            7 => Self::BadBurst,
            8 => Self::BadBand,
            9 => Self::BadBandValue,
            10 => Self::OutOfMeters,
            11 => Self::OutOfBands,
            _ => Self::UnknownCode(v),
        }
    }
}

impl std::fmt::Display for MeterModFailedCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unknown => write!(f, "unspecified error"),
            Self::MeterExists => write!(f, "meter already exists"),
            Self::InvalidMeter => write!(f, "invalid meter"),
            Self::UnknownMeter => write!(f, "meter does not exist"),
            Self::BadCommand => write!(f, "unsupported command"),
            Self::BadFlags => write!(f, "unsupported flags"),
            Self::BadRate => write!(f, "rate unsupported"),
            Self::BadBurst => write!(f, "burst size unsupported"),
            Self::BadBand => write!(f, "band unsupported"),
            Self::BadBandValue => write!(f, "band value unsupported"),
            Self::OutOfMeters => write!(f, "meter maximum exceeded"),
            Self::OutOfBands => write!(f, "maximum bands exceeded"),
            Self::UnknownCode(v) => write!(f, "unknown code {v}"),
        }
    }
}

/// Table features failed error codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableFeaturesFailedCode {
    /// Specified table does not exist
    BadTable,
    /// Invalid metadata mask
    BadMetadata,
    /// Unknown property type
    BadType,
    /// Length problem
    BadLen,
    /// Unsupported property value
    BadArgument,
    /// Permissions error
    Eperm,
    /// Unknown code
    Unknown(u16),
}

impl From<u16> for TableFeaturesFailedCode {
    fn from(v: u16) -> Self {
        match v {
            0 => Self::BadTable,
            1 => Self::BadMetadata,
            2 => Self::BadType,
            3 => Self::BadLen,
            4 => Self::BadArgument,
            5 => Self::Eperm,
            _ => Self::Unknown(v),
        }
    }
}

impl std::fmt::Display for TableFeaturesFailedCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BadTable => write!(f, "table does not exist"),
            Self::BadMetadata => write!(f, "invalid metadata mask"),
            Self::BadType => write!(f, "unknown property type"),
            Self::BadLen => write!(f, "length problem"),
            Self::BadArgument => write!(f, "unsupported property value"),
            Self::Eperm => write!(f, "permissions error"),
            Self::Unknown(v) => write!(f, "unknown code {v}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_of_error_flow_mod_failed() {
        // Error type=5 (FLOW_MOD_FAILED), code=1 (TABLE_FULL)
        let body = [0x00, 0x05, 0x00, 0x01, 0xde, 0xad, 0xbe, 0xef];
        let err = OfError::parse(&body).unwrap();

        assert_eq!(err.error_type, OfErrorType::FlowModFailed);
        assert_eq!(err.code, 1);
        assert_eq!(err.data, vec![0xde, 0xad, 0xbe, 0xef]);

        let msg = err.to_string();
        assert!(msg.contains("FLOW_MOD_FAILED"));
        assert!(msg.contains("table full"));
    }

    #[test]
    fn parse_of_error_bad_match() {
        // Error type=4 (BAD_MATCH), code=9 (BAD_PREREQ)
        let body = [0x00, 0x04, 0x00, 0x09];
        let err = OfError::parse(&body).unwrap();

        assert_eq!(err.error_type, OfErrorType::BadMatch);
        assert_eq!(err.code, 9);
        assert!(err.data.is_empty());

        let msg = err.to_string();
        assert!(msg.contains("BAD_MATCH"));
        assert!(msg.contains("prerequisite not met"));
    }

    #[test]
    fn parse_of_error_too_short() {
        let body = [0x00, 0x01];
        assert!(OfError::parse(&body).is_err());
    }

    #[test]
    fn of_error_type_from_u16() {
        assert_eq!(OfErrorType::from(0), OfErrorType::HelloFailed);
        assert_eq!(OfErrorType::from(1), OfErrorType::BadRequest);
        assert_eq!(OfErrorType::from(5), OfErrorType::FlowModFailed);
        assert_eq!(OfErrorType::from(99), OfErrorType::Unknown(99));
    }

    #[test]
    fn flow_mod_failed_codes() {
        assert_eq!(
            FlowModFailedCode::from(1).to_string(),
            "table full"
        );
        assert_eq!(
            FlowModFailedCode::from(3).to_string(),
            "overlapping flow entry"
        );
        assert_eq!(
            FlowModFailedCode::from(4).to_string(),
            "permissions error"
        );
    }

    #[test]
    fn bad_action_codes() {
        assert_eq!(
            BadActionCode::from(0).to_string(),
            "unknown action type"
        );
        assert_eq!(
            BadActionCode::from(4).to_string(),
            "problem validating output port"
        );
    }

    #[test]
    fn bad_match_codes() {
        assert_eq!(
            BadMatchCode::from(9).to_string(),
            "prerequisite not met"
        );
        assert_eq!(
            BadMatchCode::from(10).to_string(),
            "field appeared more than once"
        );
    }
}
