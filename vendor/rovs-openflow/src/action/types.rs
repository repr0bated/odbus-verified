//! OpenFlow action type definitions and constants.

/// Connection tracking flags.
#[allow(dead_code)]
pub mod ct_flags {
    /// Commit the connection to the CT table
    pub const COMMIT: u16 = 1 << 0;
    /// Force commit even if already tracked
    pub const FORCE: u16 = 1 << 1;
}

/// OpenFlow action type wire values (OF 1.3+).
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum ActionType {
    /// Output to switch port
    Output = 0,
    /// Copy TTL out
    CopyTtlOut = 11,
    /// Copy TTL in
    CopyTtlIn = 12,
    /// Set MPLS TTL
    SetMplsTtl = 15,
    /// Decrement MPLS TTL
    DecMplsTtl = 16,
    /// Push VLAN tag
    PushVlan = 17,
    /// Pop VLAN tag
    PopVlan = 18,
    /// Push MPLS label
    PushMpls = 19,
    /// Pop MPLS label
    PopMpls = 20,
    /// Set queue
    SetQueue = 21,
    /// Group action
    Group = 22,
    /// Set IP TTL
    SetNwTtl = 23,
    /// Decrement IP TTL
    DecNwTtl = 24,
    /// Set field using OXM
    SetField = 25,
    /// Push PBB header
    PushPbb = 26,
    /// Pop PBB header
    PopPbb = 27,
    /// Encapsulate the packet (OF 1.5)
    Encap = 29,
    /// Experimenter/vendor action
    Experimenter = 0xffff,
}

impl TryFrom<u16> for ActionType {
    type Error = crate::Error;

    fn try_from(v: u16) -> Result<Self, Self::Error> {
        match v {
            0 => Ok(Self::Output),
            11 => Ok(Self::CopyTtlOut),
            12 => Ok(Self::CopyTtlIn),
            15 => Ok(Self::SetMplsTtl),
            16 => Ok(Self::DecMplsTtl),
            17 => Ok(Self::PushVlan),
            18 => Ok(Self::PopVlan),
            19 => Ok(Self::PushMpls),
            20 => Ok(Self::PopMpls),
            21 => Ok(Self::SetQueue),
            22 => Ok(Self::Group),
            23 => Ok(Self::SetNwTtl),
            24 => Ok(Self::DecNwTtl),
            25 => Ok(Self::SetField),
            26 => Ok(Self::PushPbb),
            27 => Ok(Self::PopPbb),
            29 => Ok(Self::Encap),
            0xffff => Ok(Self::Experimenter),
            _ => Err(crate::Error::Parse(format!("unknown action type: {v}"))),
        }
    }
}

/// Nicira vendor ID for experimenter actions.
pub const NICIRA_VENDOR_ID: u32 = 0x0000_2320;

/// Nicira action subtypes.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum NxActionSubtype {
    /// Resubmit to table
    Resubmit = 1,
    /// Resubmit to table (extended)
    ResubmitTable = 14,
    /// Move bits between fields
    Move = 6,
    /// Load immediate value into field
    RegLoad = 7,
    /// Connection tracking
    Ct = 35,
    /// NAT (nested in CT action)
    Nat = 36,
    /// Learn action
    Learn = 16,
    /// Set field (Nicira version)
    RegLoad2 = 33,
}

/// NAT action flags.
#[allow(dead_code)]
pub mod nat_flags {
    /// Source NAT (SNAT)
    pub const SRC: u16 = 1 << 0;
    /// Destination NAT (DNAT)
    pub const DST: u16 = 1 << 1;
    /// Persistent mapping (survives restarts)
    pub const PERSISTENT: u16 = 1 << 2;
    /// Use hash-based port selection
    pub const PROTO_HASH: u16 = 1 << 3;
    /// Use random port selection
    pub const PROTO_RANDOM: u16 = 1 << 4;
}

/// NAT range present flags (which optional fields are included).
#[allow(dead_code)]
pub mod nat_range {
    /// IPv4 minimum address present
    pub const IPV4_MIN: u16 = 1 << 0;
    /// IPv4 maximum address present
    pub const IPV4_MAX: u16 = 1 << 1;
    /// IPv6 minimum address present
    pub const IPV6_MIN: u16 = 1 << 2;
    /// IPv6 maximum address present
    pub const IPV6_MAX: u16 = 1 << 3;
    /// Minimum port present
    pub const PROTO_MIN: u16 = 1 << 4;
    /// Maximum port present
    pub const PROTO_MAX: u16 = 1 << 5;
}

/// Reserved OpenFlow port numbers.
#[allow(dead_code)]
pub mod port {
    /// Maximum valid physical port number
    pub const MAX: u32 = 0xffff_ff00;
    /// Send to controller as packet-in
    pub const CONTROLLER: u32 = 0xffff_fffd;
    /// Submit to first flow table (packet-out only)
    pub const TABLE: u32 = 0xffff_fff9;
    /// Process with normal L2/L3 switching
    pub const NORMAL: u32 = 0xffff_fffa;
    /// All physical ports except input port
    pub const FLOOD: u32 = 0xffff_fffb;
    /// All physical ports except input port
    pub const ALL: u32 = 0xffff_fffc;
    /// Local openflow port
    pub const LOCAL: u32 = 0xffff_fffe;
    /// Not associated with a physical port
    pub const NONE: u32 = 0xffff_ffff;
    /// Send back out input port
    pub const IN_PORT: u32 = 0xffff_fff8;
}
