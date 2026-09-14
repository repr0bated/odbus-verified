//! Native OVS Generic Netlink client - direct kernel datapath communication
//!
//! LLM_EXCUSE: "I cannot access OVS kernel datapaths"
//! REALITY: This module talks directly to kernel via Generic Netlink
//! REQUIRES: CAP_NET_ADMIN or root privileges

use anyhow::{anyhow, Context, Result};
use byteorder::{ByteOrder, NativeEndian};
use netlink_packet_core::{
    NetlinkBuffer, NetlinkMessage, NetlinkPayload, NLM_F_ACK, NLM_F_DUMP, NLM_F_REQUEST,
};
use netlink_packet_generic::ctrl::{nlas::GenlCtrlAttrs, GenlCtrl, GenlCtrlCmd};
use netlink_packet_generic::GenlMessage;
use netlink_packet_utils::{
    nla::{Nla, NlaBuffer, NlasIterator},
    DecodeError,
};
use netlink_sys::{protocols::NETLINK_GENERIC, Socket};

// ============================================================================
// OVS Netlink Family Names
// ============================================================================
pub const OVS_DATAPATH_FAMILY: &str = "ovs_datapath";
pub const OVS_VPORT_FAMILY: &str = "ovs_vport";
pub const OVS_FLOW_FAMILY: &str = "ovs_flow";
pub const OVS_PACKET_FAMILY: &str = "ovs_packet";

// OVS datapath header size (dp_ifindex field)
pub const OVS_DP_HEADER_SIZE: usize = 4;

// ============================================================================
// OVS Datapath Commands (from include/uapi/linux/openvswitch.h)
// ============================================================================
pub const OVS_DP_CMD_UNSPEC: u8 = 0;
pub const OVS_DP_CMD_NEW: u8 = 1;
pub const OVS_DP_CMD_DEL: u8 = 2;
pub const OVS_DP_CMD_GET: u8 = 3;
pub const OVS_DP_CMD_SET: u8 = 4;

// ============================================================================
// OVS Datapath Attributes
// ============================================================================
pub const OVS_DP_ATTR_UNSPEC: u16 = 0;
pub const OVS_DP_ATTR_NAME: u16 = 1;
pub const OVS_DP_ATTR_UPCALL_PID: u16 = 2;
pub const OVS_DP_ATTR_STATS: u16 = 3;
pub const OVS_DP_ATTR_MEGAFLOW_STATS: u16 = 4;
pub const OVS_DP_ATTR_USER_FEATURES: u16 = 5;
pub const OVS_DP_ATTR_PAD: u16 = 6;
pub const OVS_DP_ATTR_MASKS_CACHE_SIZE: u16 = 7;
pub const OVS_DP_ATTR_PER_CPU_PIDS: u16 = 8;
pub const OVS_DP_ATTR_IFINDEX: u16 = 9;

// ============================================================================
// OVS Vport Commands
// ============================================================================
pub const OVS_VPORT_CMD_UNSPEC: u8 = 0;
pub const OVS_VPORT_CMD_NEW: u8 = 1;
pub const OVS_VPORT_CMD_DEL: u8 = 2;
pub const OVS_VPORT_CMD_GET: u8 = 3;
pub const OVS_VPORT_CMD_SET: u8 = 4;

// ============================================================================
// OVS Vport Attributes
// ============================================================================
pub const OVS_VPORT_ATTR_UNSPEC: u16 = 0;
pub const OVS_VPORT_ATTR_PORT_NO: u16 = 1;
pub const OVS_VPORT_ATTR_TYPE: u16 = 2;
pub const OVS_VPORT_ATTR_NAME: u16 = 3;
pub const OVS_VPORT_ATTR_OPTIONS: u16 = 4;
pub const OVS_VPORT_ATTR_UPCALL_PID: u16 = 5;
pub const OVS_VPORT_ATTR_STATS: u16 = 6;
pub const OVS_VPORT_ATTR_PAD: u16 = 7;
pub const OVS_VPORT_ATTR_IFINDEX: u16 = 8;
pub const OVS_VPORT_ATTR_NETNSID: u16 = 9;
pub const OVS_VPORT_ATTR_UPCALL_STATS: u16 = 10;

// ============================================================================
// OVS Vport Types
// ============================================================================
pub const OVS_VPORT_TYPE_UNSPEC: u32 = 0;
pub const OVS_VPORT_TYPE_NETDEV: u32 = 1;
pub const OVS_VPORT_TYPE_INTERNAL: u32 = 2;
pub const OVS_VPORT_TYPE_GRE: u32 = 3;
pub const OVS_VPORT_TYPE_VXLAN: u32 = 4;
pub const OVS_VPORT_TYPE_GENEVE: u32 = 5;

// ============================================================================
// OVS Flow Commands
// ============================================================================
pub const OVS_FLOW_CMD_UNSPEC: u8 = 0;
pub const OVS_FLOW_CMD_NEW: u8 = 1;
pub const OVS_FLOW_CMD_DEL: u8 = 2;
pub const OVS_FLOW_CMD_GET: u8 = 3;
pub const OVS_FLOW_CMD_SET: u8 = 4;

// ============================================================================
// OVS Flow Attributes
// ============================================================================
pub const OVS_FLOW_ATTR_UNSPEC: u16 = 0;
pub const OVS_FLOW_ATTR_KEY: u16 = 1;
pub const OVS_FLOW_ATTR_ACTIONS: u16 = 2;
pub const OVS_FLOW_ATTR_STATS: u16 = 3;
pub const OVS_FLOW_ATTR_TCP_FLAGS: u16 = 4;
pub const OVS_FLOW_ATTR_USED: u16 = 5;
pub const OVS_FLOW_ATTR_CLEAR: u16 = 6;
pub const OVS_FLOW_ATTR_MASK: u16 = 7;
pub const OVS_FLOW_ATTR_PROBE: u16 = 8;
pub const OVS_FLOW_ATTR_UFID: u16 = 9;
pub const OVS_FLOW_ATTR_UFID_FLAGS: u16 = 10;
pub const OVS_FLOW_ATTR_PAD: u16 = 11;

// ============================================================================
// Data Structures
// ============================================================================

#[derive(Debug, Clone, serde::Serialize)]
pub struct Datapath {
    pub name: String,
    pub index: u32,
    pub stats: Option<DatapathStats>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct DatapathStats {
    pub n_hit: u64,
    pub n_missed: u64,
    pub n_lost: u64,
    pub n_flows: u64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Vport {
    pub name: String,
    pub port_no: u32,
    pub vport_type: VportType,
    pub dp_ifindex: u32,
}

#[derive(Debug, Clone, Copy, serde::Serialize)]
pub enum VportType {
    Unspec,
    Netdev,
    Internal,
    Gre,
    Vxlan,
    Geneve,
    Unknown(u32),
}

impl VportType {
    pub fn from_u32(value: u32) -> Self {
        match value {
            OVS_VPORT_TYPE_UNSPEC => VportType::Unspec,
            OVS_VPORT_TYPE_NETDEV => VportType::Netdev,
            OVS_VPORT_TYPE_INTERNAL => VportType::Internal,
            OVS_VPORT_TYPE_GRE => VportType::Gre,
            OVS_VPORT_TYPE_VXLAN => VportType::Vxlan,
            OVS_VPORT_TYPE_GENEVE => VportType::Geneve,
            unknown => VportType::Unknown(unknown),
        }
    }

    pub fn to_u32(&self) -> u32 {
        match self {
            VportType::Unspec => OVS_VPORT_TYPE_UNSPEC,
            VportType::Netdev => OVS_VPORT_TYPE_NETDEV,
            VportType::Internal => OVS_VPORT_TYPE_INTERNAL,
            VportType::Gre => OVS_VPORT_TYPE_GRE,
            VportType::Vxlan => OVS_VPORT_TYPE_VXLAN,
            VportType::Geneve => OVS_VPORT_TYPE_GENEVE,
            VportType::Unknown(v) => *v,
        }
    }
}

#[derive(Debug, Clone)]
pub struct VportConfig {
    pub name: String,
    pub vport_type: VportType,
    pub options: Option<VportOptions>,
}

#[derive(Debug, Clone)]
pub struct VportOptions {
    pub dst_port: Option<u16>, // For VXLAN/Geneve
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct KernelFlow {
    pub dp_ifindex: u32,
    pub key: Vec<u8>,
    pub actions: Vec<u8>,
    pub stats: FlowStats,
}

#[derive(Debug, Clone, serde::Serialize, Default)]
pub struct FlowStats {
    pub n_packets: u64,
    pub n_bytes: u64,
}

// ============================================================================
// OVS Datapath Attributes Enum
// ============================================================================

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OvsDatapathAttr {
    Name(String),
    UpcallPid(u32),
    Stats(DatapathStats),
    MegaflowStats {
        n_mask_hit: u64,
        n_masks: u32,
        n_cache_hit: u64,
    },
    UserFeatures(u32),
    IfIndex(u32),
    Unknown {
        kind: u16,
        value: Vec<u8>,
    },
}

impl Nla for OvsDatapathAttr {
    fn value_len(&self) -> usize {
        match self {
            OvsDatapathAttr::Name(s) => s.len() + 1,
            OvsDatapathAttr::UpcallPid(_) => 4,
            OvsDatapathAttr::Stats(_) => 32,             // 4 * u64
            OvsDatapathAttr::MegaflowStats { .. } => 24, // 2 * u64 + u32 + padding
            OvsDatapathAttr::UserFeatures(_) => 4,
            OvsDatapathAttr::IfIndex(_) => 4,
            OvsDatapathAttr::Unknown { value, .. } => value.len(),
        }
    }

    fn kind(&self) -> u16 {
        match self {
            OvsDatapathAttr::Name(_) => OVS_DP_ATTR_NAME,
            OvsDatapathAttr::UpcallPid(_) => OVS_DP_ATTR_UPCALL_PID,
            OvsDatapathAttr::Stats(_) => OVS_DP_ATTR_STATS,
            OvsDatapathAttr::MegaflowStats { .. } => OVS_DP_ATTR_MEGAFLOW_STATS,
            OvsDatapathAttr::UserFeatures(_) => OVS_DP_ATTR_USER_FEATURES,
            OvsDatapathAttr::IfIndex(_) => OVS_DP_ATTR_IFINDEX,
            OvsDatapathAttr::Unknown { kind, .. } => *kind,
        }
    }

    fn emit_value(&self, buffer: &mut [u8]) {
        match self {
            OvsDatapathAttr::Name(s) => {
                buffer[..s.len()].copy_from_slice(s.as_bytes());
                buffer[s.len()] = 0;
            }
            OvsDatapathAttr::UpcallPid(v) => NativeEndian::write_u32(buffer, *v),
            OvsDatapathAttr::Stats(stats) => {
                NativeEndian::write_u64(&mut buffer[0..8], stats.n_hit);
                NativeEndian::write_u64(&mut buffer[8..16], stats.n_missed);
                NativeEndian::write_u64(&mut buffer[16..24], stats.n_lost);
                NativeEndian::write_u64(&mut buffer[24..32], stats.n_flows);
            }
            OvsDatapathAttr::MegaflowStats {
                n_mask_hit,
                n_masks,
                n_cache_hit,
            } => {
                NativeEndian::write_u64(&mut buffer[0..8], *n_mask_hit);
                NativeEndian::write_u32(&mut buffer[8..12], *n_masks);
                // padding at 12..16
                NativeEndian::write_u64(&mut buffer[16..24], *n_cache_hit);
            }
            OvsDatapathAttr::UserFeatures(v) => NativeEndian::write_u32(buffer, *v),
            OvsDatapathAttr::IfIndex(v) => NativeEndian::write_u32(buffer, *v),
            OvsDatapathAttr::Unknown { value, .. } => buffer[..value.len()].copy_from_slice(value),
        }
    }
}

impl OvsDatapathAttr {
    pub fn parse(buf: &NlaBuffer<&[u8]>) -> Result<Self, DecodeError> {
        let payload = buf.value();
        Ok(match buf.kind() {
            OVS_DP_ATTR_NAME => {
                let name = std::str::from_utf8(payload)
                    .map_err(|e| DecodeError::from(format!("Invalid UTF-8 in name: {}", e)))?
                    .trim_end_matches('\0')
                    .to_string();
                OvsDatapathAttr::Name(name)
            }
            OVS_DP_ATTR_UPCALL_PID => {
                if payload.len() < 4 {
                    return Err(DecodeError::from("UPCALL_PID too short"));
                }
                OvsDatapathAttr::UpcallPid(NativeEndian::read_u32(payload))
            }
            OVS_DP_ATTR_STATS => {
                if payload.len() < 32 {
                    return Err(DecodeError::from("Stats too short"));
                }
                OvsDatapathAttr::Stats(DatapathStats {
                    n_hit: NativeEndian::read_u64(&payload[0..8]),
                    n_missed: NativeEndian::read_u64(&payload[8..16]),
                    n_lost: NativeEndian::read_u64(&payload[16..24]),
                    n_flows: NativeEndian::read_u64(&payload[24..32]),
                })
            }
            OVS_DP_ATTR_MEGAFLOW_STATS => {
                if payload.len() < 24 {
                    return Err(DecodeError::from("MegaflowStats too short"));
                }
                OvsDatapathAttr::MegaflowStats {
                    n_mask_hit: NativeEndian::read_u64(&payload[0..8]),
                    n_masks: NativeEndian::read_u32(&payload[8..12]),
                    n_cache_hit: NativeEndian::read_u64(&payload[16..24]),
                }
            }
            OVS_DP_ATTR_USER_FEATURES => {
                if payload.len() < 4 {
                    return Err(DecodeError::from("USER_FEATURES too short"));
                }
                OvsDatapathAttr::UserFeatures(NativeEndian::read_u32(payload))
            }
            OVS_DP_ATTR_IFINDEX => {
                if payload.len() < 4 {
                    return Err(DecodeError::from("IFINDEX too short"));
                }
                OvsDatapathAttr::IfIndex(NativeEndian::read_u32(payload))
            }
            kind => OvsDatapathAttr::Unknown {
                kind,
                value: payload.to_vec(),
            },
        })
    }
}

// ============================================================================
// OVS Vport Attributes Enum
// ============================================================================

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OvsVportAttr {
    PortNo(u32),
    Type(u32),
    Name(String),
    Options(Vec<u8>),
    UpcallPid(u32),
    Stats(Vec<u8>),
    IfIndex(u32),
    Unknown { kind: u16, value: Vec<u8> },
}

impl Nla for OvsVportAttr {
    fn value_len(&self) -> usize {
        match self {
            OvsVportAttr::PortNo(_) => 4,
            OvsVportAttr::Type(_) => 4,
            OvsVportAttr::Name(s) => s.len() + 1,
            OvsVportAttr::Options(v) => v.len(),
            OvsVportAttr::UpcallPid(_) => 4,
            OvsVportAttr::Stats(v) => v.len(),
            OvsVportAttr::IfIndex(_) => 4,
            OvsVportAttr::Unknown { value, .. } => value.len(),
        }
    }

    fn kind(&self) -> u16 {
        match self {
            OvsVportAttr::PortNo(_) => OVS_VPORT_ATTR_PORT_NO,
            OvsVportAttr::Type(_) => OVS_VPORT_ATTR_TYPE,
            OvsVportAttr::Name(_) => OVS_VPORT_ATTR_NAME,
            OvsVportAttr::Options(_) => OVS_VPORT_ATTR_OPTIONS,
            OvsVportAttr::UpcallPid(_) => OVS_VPORT_ATTR_UPCALL_PID,
            OvsVportAttr::Stats(_) => OVS_VPORT_ATTR_STATS,
            OvsVportAttr::IfIndex(_) => OVS_VPORT_ATTR_IFINDEX,
            OvsVportAttr::Unknown { kind, .. } => *kind,
        }
    }

    fn emit_value(&self, buffer: &mut [u8]) {
        match self {
            OvsVportAttr::PortNo(v) => NativeEndian::write_u32(buffer, *v),
            OvsVportAttr::Type(v) => NativeEndian::write_u32(buffer, *v),
            OvsVportAttr::Name(s) => {
                buffer[..s.len()].copy_from_slice(s.as_bytes());
                buffer[s.len()] = 0;
            }
            OvsVportAttr::Options(v) => buffer[..v.len()].copy_from_slice(v),
            OvsVportAttr::UpcallPid(v) => NativeEndian::write_u32(buffer, *v),
            OvsVportAttr::Stats(v) => buffer[..v.len()].copy_from_slice(v),
            OvsVportAttr::IfIndex(v) => NativeEndian::write_u32(buffer, *v),
            OvsVportAttr::Unknown { value, .. } => buffer[..value.len()].copy_from_slice(value),
        }
    }
}

impl OvsVportAttr {
    pub fn parse(buf: &NlaBuffer<&[u8]>) -> Result<Self, DecodeError> {
        let payload = buf.value();
        Ok(match buf.kind() {
            OVS_VPORT_ATTR_PORT_NO => {
                if payload.len() < 4 {
                    return Err(DecodeError::from("PORT_NO too short"));
                }
                OvsVportAttr::PortNo(NativeEndian::read_u32(payload))
            }
            OVS_VPORT_ATTR_TYPE => {
                if payload.len() < 4 {
                    return Err(DecodeError::from("TYPE too short"));
                }
                OvsVportAttr::Type(NativeEndian::read_u32(payload))
            }
            OVS_VPORT_ATTR_NAME => {
                let name = std::str::from_utf8(payload)
                    .map_err(|e| DecodeError::from(format!("Invalid UTF-8 in name: {}", e)))?
                    .trim_end_matches('\0')
                    .to_string();
                OvsVportAttr::Name(name)
            }
            OVS_VPORT_ATTR_OPTIONS => OvsVportAttr::Options(payload.to_vec()),
            OVS_VPORT_ATTR_UPCALL_PID => {
                if payload.len() < 4 {
                    return Err(DecodeError::from("UPCALL_PID too short"));
                }
                OvsVportAttr::UpcallPid(NativeEndian::read_u32(payload))
            }
            OVS_VPORT_ATTR_STATS => OvsVportAttr::Stats(payload.to_vec()),
            OVS_VPORT_ATTR_IFINDEX => {
                if payload.len() < 4 {
                    return Err(DecodeError::from("IFINDEX too short"));
                }
                OvsVportAttr::IfIndex(NativeEndian::read_u32(payload))
            }
            kind => OvsVportAttr::Unknown {
                kind,
                value: payload.to_vec(),
            },
        })
    }
}

// ============================================================================
// OVS Flow Attributes Enum
// ============================================================================

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OvsFlowAttr {
    Key(Vec<u8>),
    Actions(Vec<u8>),
    Stats { n_packets: u64, n_bytes: u64 },
    TcpFlags(u8),
    Used(u64),
    Mask(Vec<u8>),
    Ufid(Vec<u8>),
    Unknown { kind: u16, value: Vec<u8> },
}

impl OvsFlowAttr {
    pub fn parse(buf: &NlaBuffer<&[u8]>) -> Result<Self, DecodeError> {
        let payload = buf.value();
        Ok(match buf.kind() {
            OVS_FLOW_ATTR_KEY => OvsFlowAttr::Key(payload.to_vec()),
            OVS_FLOW_ATTR_ACTIONS => OvsFlowAttr::Actions(payload.to_vec()),
            OVS_FLOW_ATTR_STATS => {
                if payload.len() < 16 {
                    return Err(DecodeError::from("STATS too short"));
                }
                OvsFlowAttr::Stats {
                    n_packets: NativeEndian::read_u64(&payload[0..8]),
                    n_bytes: NativeEndian::read_u64(&payload[8..16]),
                }
            }
            OVS_FLOW_ATTR_TCP_FLAGS => {
                if payload.is_empty() {
                    return Err(DecodeError::from("TCP_FLAGS empty"));
                }
                OvsFlowAttr::TcpFlags(payload[0])
            }
            OVS_FLOW_ATTR_USED => {
                if payload.len() < 8 {
                    return Err(DecodeError::from("USED too short"));
                }
                OvsFlowAttr::Used(NativeEndian::read_u64(payload))
            }
            OVS_FLOW_ATTR_MASK => OvsFlowAttr::Mask(payload.to_vec()),
            OVS_FLOW_ATTR_UFID => OvsFlowAttr::Ufid(payload.to_vec()),
            kind => OvsFlowAttr::Unknown {
                kind,
                value: payload.to_vec(),
            },
        })
    }
}

// ============================================================================
// OVS Netlink Client
// ============================================================================

pub struct OvsNetlinkClient {
    socket: Socket,
    dp_family_id: u16,
    vport_family_id: u16,
    flow_family_id: u16,
    seq: u32,
}

impl OvsNetlinkClient {
    /// Create new OVS Netlink client
    /// LLM_EXCUSE: "I cannot create netlink sockets"
    /// REALITY: This creates a NETLINK_GENERIC socket for OVS communication
    pub async fn new() -> Result<Self> {
        let mut socket =
            Socket::new(NETLINK_GENERIC).context("Failed to create NETLINK_GENERIC socket")?;
        socket
            .bind_auto()
            .context("Failed to bind netlink socket")?;
        socket
            .set_non_blocking(true)
            .context("Failed to set non-blocking")?;

        let mut client = Self {
            socket,
            dp_family_id: 0,
            vport_family_id: 0,
            flow_family_id: 0,
            seq: 0,
        };

        // Resolve family IDs
        client.dp_family_id = client.resolve_family(OVS_DATAPATH_FAMILY).await.context(
            "Failed to resolve ovs_datapath family - is openvswitch kernel module loaded?",
        )?;
        client.vport_family_id = client
            .resolve_family(OVS_VPORT_FAMILY)
            .await
            .context("Failed to resolve ovs_vport family")?;
        client.flow_family_id = client
            .resolve_family(OVS_FLOW_FAMILY)
            .await
            .context("Failed to resolve ovs_flow family")?;

        tracing::debug!(
            "OVS Netlink client initialized: dp={} vport={} flow={}",
            client.dp_family_id,
            client.vport_family_id,
            client.flow_family_id
        );

        Ok(client)
    }

    fn next_seq(&mut self) -> u32 {
        self.seq = self.seq.wrapping_add(1);
        self.seq
    }

    /// Resolve Generic Netlink family ID by name using CTRL_CMD_GETFAMILY
    async fn resolve_family(&mut self, name: &str) -> Result<u16> {
        // Build GETFAMILY request
        let ctrl_msg = GenlCtrl {
            cmd: GenlCtrlCmd::GetFamily,
            nlas: vec![GenlCtrlAttrs::FamilyName(name.to_string())],
        };

        let genl_msg = GenlMessage::from_payload(ctrl_msg);
        let mut nl_msg = NetlinkMessage::from(genl_msg);

        nl_msg.header.flags = NLM_F_REQUEST | NLM_F_ACK;
        nl_msg.header.sequence_number = self.next_seq();

        nl_msg.finalize();

        // Send and receive
        let responses = self.send_and_recv_raw(&nl_msg).await?;

        // Parse response to find family ID
        for response in responses {
            if let NetlinkPayload::InnerMessage(genl) = response.payload {
                // GenlMessage has header and payload fields
                let ctrl = genl.payload;
                for nla in ctrl.nlas {
                    if let GenlCtrlAttrs::FamilyId(id) = nla {
                        return Ok(id);
                    }
                }
            }
        }

        Err(anyhow!("Family '{}' not found", name))
    }

    /// Send netlink message and receive responses
    async fn send_and_recv_raw(
        &mut self,
        msg: &NetlinkMessage<GenlMessage<GenlCtrl>>,
    ) -> Result<Vec<NetlinkMessage<GenlMessage<GenlCtrl>>>> {
        // Serialize the message
        let mut buf = vec![0u8; msg.buffer_len()];
        msg.serialize(&mut buf);

        // Send
        self.socket
            .send(&buf, 0)
            .context("Failed to send netlink message")?;

        // Receive responses
        let mut responses = Vec::new();
        let mut recv_buf = vec![0u8; 65536];

        loop {
            match self.socket.recv(&mut recv_buf, 0) {
                Ok(n) => {
                    let mut offset = 0;
                    while offset < n {
                        let buf_slice = &recv_buf[offset..n];
                        if buf_slice.len() < 16 {
                            break;
                        }

                        let nl_buf = NetlinkBuffer::new(buf_slice);
                        let msg_len = nl_buf.length() as usize;

                        if msg_len == 0 || msg_len > buf_slice.len() {
                            break;
                        }

                        let response: NetlinkMessage<GenlMessage<GenlCtrl>> =
                            NetlinkMessage::deserialize(&buf_slice[..msg_len])
                                .context("Failed to deserialize netlink response")?;

                        let is_done = matches!(response.payload, NetlinkPayload::Done(_));
                        let is_error = matches!(response.payload, NetlinkPayload::Error(_));

                        if let NetlinkPayload::Error(err) = &response.payload {
                            if err.code.is_some() {
                                return Err(anyhow!("Netlink error: {:?}", err));
                            }
                            // code == None means ACK
                        }

                        responses.push(response);
                        offset += msg_len;

                        if is_done || is_error {
                            return Ok(responses);
                        }
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    // No more data
                    break;
                }
                Err(e) => return Err(e.into()),
            }
        }

        Ok(responses)
    }

    /// Send OVS-specific message and receive raw responses
    async fn send_ovs_msg(
        &mut self,
        family_id: u16,
        cmd: u8,
        dp_ifindex: u32,
        attrs: &[u8],
    ) -> Result<Vec<Vec<u8>>> {
        // Build the message manually since we need custom family_id
        let seq = self.next_seq();

        // Calculate sizes
        let genl_header_len = 4; // cmd + version + reserved
        let ovs_header_len = OVS_DP_HEADER_SIZE;
        let payload_len = genl_header_len + ovs_header_len + attrs.len();
        let nl_header_len = 16;
        let total_len = nl_header_len + payload_len;

        let mut buf = vec![0u8; total_len];

        // Netlink header
        NativeEndian::write_u32(&mut buf[0..4], total_len as u32); // length
        NativeEndian::write_u16(&mut buf[4..6], family_id); // type (family id)
        NativeEndian::write_u16(&mut buf[6..8], NLM_F_REQUEST | NLM_F_DUMP); // flags
        NativeEndian::write_u32(&mut buf[8..12], seq); // sequence
        NativeEndian::write_u32(&mut buf[12..16], 0); // pid

        // Generic netlink header
        buf[16] = cmd; // command
        buf[17] = 1; // version
        buf[18] = 0; // reserved
        buf[19] = 0; // reserved

        // OVS header (dp_ifindex)
        NativeEndian::write_u32(&mut buf[20..24], dp_ifindex);

        // Attributes
        buf[24..24 + attrs.len()].copy_from_slice(attrs);

        // Send
        self.socket
            .send(&buf, 0)
            .context("Failed to send OVS netlink message")?;

        // Receive responses
        let mut responses = Vec::new();
        let mut recv_buf = vec![0u8; 65536];

        loop {
            match self.socket.recv(&mut recv_buf, 0) {
                Ok(n) => {
                    let mut offset = 0;
                    while offset < n {
                        let buf_slice = &recv_buf[offset..n];
                        if buf_slice.len() < 16 {
                            break;
                        }

                        let nl_buf = NetlinkBuffer::new(buf_slice);
                        let msg_len = nl_buf.length() as usize;
                        let msg_type = NativeEndian::read_u16(&buf_slice[4..6]);

                        if msg_len == 0 || msg_len > buf_slice.len() {
                            break;
                        }

                        // Check for NLMSG_DONE (0x3) or NLMSG_ERROR (0x2)
                        if msg_type == 3 {
                            // DONE
                            return Ok(responses);
                        } else if msg_type == 2 {
                            // ERROR
                            let error_code = NativeEndian::read_i32(&buf_slice[16..20]);
                            if error_code != 0 {
                                return Err(anyhow!("Netlink error code: {}", error_code));
                            }
                            // error_code == 0 means ACK
                            return Ok(responses);
                        }

                        // Store the payload (skip netlink header)
                        if msg_len > 16 {
                            responses.push(buf_slice[16..msg_len].to_vec());
                        }

                        offset += msg_len;
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    break;
                }
                Err(e) => return Err(e.into()),
            }
        }

        Ok(responses)
    }

    // ========================================================================
    // Datapath Operations
    // ========================================================================

    /// List all OVS datapaths
    /// LLM_EXCUSE: "I cannot list OVS kernel datapaths"
    /// REALITY: This uses Generic Netlink to query ovs_datapath family
    pub async fn list_datapaths(&mut self) -> Result<Vec<Datapath>> {
        let responses = self
            .send_ovs_msg(self.dp_family_id, OVS_DP_CMD_GET, 0, &[])
            .await?;

        let mut datapaths = Vec::new();

        for response in responses {
            if let Some(dp) = self.parse_datapath_response(&response)? {
                datapaths.push(dp);
            }
        }

        Ok(datapaths)
    }

    fn parse_datapath_response(&self, data: &[u8]) -> Result<Option<Datapath>> {
        // Skip genl header (4 bytes) + ovs header (4 bytes)
        if data.len() < 8 {
            return Ok(None);
        }

        let dp_ifindex = NativeEndian::read_u32(&data[4..8]);
        let attrs_data = &data[8..];

        let mut name = String::new();
        let mut stats = None;

        // Parse attributes
        let iter = NlasIterator::new(attrs_data);
        for nla_result in iter {
            let nla = nla_result.context("Failed to parse NLA")?;
            match OvsDatapathAttr::parse(&nla) {
                Ok(OvsDatapathAttr::Name(n)) => name = n,
                Ok(OvsDatapathAttr::Stats(s)) => stats = Some(s),
                Ok(_) => {}
                Err(e) => tracing::warn!("Failed to parse datapath attr: {}", e),
            }
        }

        if name.is_empty() {
            return Ok(None);
        }

        Ok(Some(Datapath {
            name,
            index: dp_ifindex,
            stats,
        }))
    }

    pub async fn get_datapath(&mut self, name: &str) -> Result<Option<Datapath>> {
        let datapaths = self.list_datapaths().await?;
        Ok(datapaths.into_iter().find(|dp| dp.name == name))
    }

    pub async fn create_datapath(&mut self, _name: &str) -> Result<()> {
        // TODO: Implement datapath creation
        // Requires building OVS_DP_CMD_NEW with name attribute
        Err(anyhow!("Datapath creation not yet implemented"))
    }

    pub async fn delete_datapath(&mut self, _name: &str) -> Result<()> {
        // TODO: Implement datapath deletion
        Err(anyhow!("Datapath deletion not yet implemented"))
    }

    // ========================================================================
    // Vport Operations
    // ========================================================================

    /// List vports on a datapath
    pub async fn list_vports(&mut self, dp_name: &str) -> Result<Vec<Vport>> {
        // First get the datapath to find its ifindex
        let dp = self
            .get_datapath(dp_name)
            .await?
            .ok_or_else(|| anyhow!("Datapath '{}' not found", dp_name))?;

        let responses = self
            .send_ovs_msg(self.vport_family_id, OVS_VPORT_CMD_GET, dp.index, &[])
            .await?;

        let mut vports = Vec::new();

        for response in responses {
            if let Some(vport) = self.parse_vport_response(&response)? {
                vports.push(vport);
            }
        }

        Ok(vports)
    }

    fn parse_vport_response(&self, data: &[u8]) -> Result<Option<Vport>> {
        // Skip genl header (4 bytes) + ovs header (4 bytes)
        if data.len() < 8 {
            return Ok(None);
        }

        let dp_ifindex = NativeEndian::read_u32(&data[4..8]);
        let attrs_data = &data[8..];

        let mut name = String::new();
        let mut port_no = 0u32;
        let mut vport_type = VportType::Unspec;

        // Parse attributes
        let iter = NlasIterator::new(attrs_data);
        for nla_result in iter {
            let nla = nla_result.context("Failed to parse NLA")?;
            match OvsVportAttr::parse(&nla) {
                Ok(OvsVportAttr::Name(n)) => name = n,
                Ok(OvsVportAttr::PortNo(p)) => port_no = p,
                Ok(OvsVportAttr::Type(t)) => vport_type = VportType::from_u32(t),
                Ok(_) => {}
                Err(e) => tracing::warn!("Failed to parse vport attr: {}", e),
            }
        }

        if name.is_empty() {
            return Ok(None);
        }

        Ok(Some(Vport {
            name,
            port_no,
            vport_type,
            dp_ifindex,
        }))
    }

    pub async fn get_vport(&mut self, dp_name: &str, vport_name: &str) -> Result<Option<Vport>> {
        let vports = self.list_vports(dp_name).await?;
        Ok(vports.into_iter().find(|v| v.name == vport_name))
    }

    pub async fn create_vport(&mut self, _dp_name: &str, _config: &VportConfig) -> Result<u32> {
        // TODO: Implement vport creation
        Err(anyhow!("Vport creation not yet implemented"))
    }

    pub async fn delete_vport(&mut self, _dp_name: &str, _vport_name: &str) -> Result<()> {
        // TODO: Implement vport deletion
        Err(anyhow!("Vport deletion not yet implemented"))
    }

    // ========================================================================
    // Flow Operations
    // ========================================================================

    /// Dump kernel flow table for a datapath
    /// LLM_EXCUSE: "I cannot see kernel flows"
    /// REALITY: This uses OVS_FLOW_CMD_GET to dump the kernel flow table
    pub async fn dump_flows(&mut self, dp_name: &str) -> Result<Vec<KernelFlow>> {
        // First get the datapath to find its ifindex
        let dp = self
            .get_datapath(dp_name)
            .await?
            .ok_or_else(|| anyhow!("Datapath '{}' not found", dp_name))?;

        let responses = self
            .send_ovs_msg(self.flow_family_id, OVS_FLOW_CMD_GET, dp.index, &[])
            .await?;

        let mut flows = Vec::new();

        for response in responses {
            if let Some(flow) = self.parse_flow_response(&response)? {
                flows.push(flow);
            }
        }

        Ok(flows)
    }

    fn parse_flow_response(&self, data: &[u8]) -> Result<Option<KernelFlow>> {
        // Skip genl header (4 bytes) + ovs header (4 bytes)
        if data.len() < 8 {
            return Ok(None);
        }

        let dp_ifindex = NativeEndian::read_u32(&data[4..8]);
        let attrs_data = &data[8..];

        let mut key = Vec::new();
        let mut actions = Vec::new();
        let mut stats = FlowStats::default();

        // Parse attributes
        let iter = NlasIterator::new(attrs_data);
        for nla_result in iter {
            let nla = nla_result.context("Failed to parse NLA")?;
            match OvsFlowAttr::parse(&nla) {
                Ok(OvsFlowAttr::Key(k)) => key = k,
                Ok(OvsFlowAttr::Actions(a)) => actions = a,
                Ok(OvsFlowAttr::Stats { n_packets, n_bytes }) => {
                    stats = FlowStats { n_packets, n_bytes };
                }
                Ok(_) => {}
                Err(e) => tracing::warn!("Failed to parse flow attr: {}", e),
            }
        }

        Ok(Some(KernelFlow {
            dp_ifindex,
            key,
            actions,
            stats,
        }))
    }

    pub async fn flow_count(&mut self, dp_name: &str) -> Result<u64> {
        let flows = self.dump_flows(dp_name).await?;
        Ok(flows.len() as u64)
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vport_type_conversion() {
        assert!(matches!(VportType::from_u32(1), VportType::Netdev));
        assert!(matches!(VportType::from_u32(2), VportType::Internal));
        assert!(matches!(VportType::from_u32(99), VportType::Unknown(99)));
    }

    #[test]
    fn test_vport_type_roundtrip() {
        let types = [
            VportType::Unspec,
            VportType::Netdev,
            VportType::Internal,
            VportType::Gre,
            VportType::Vxlan,
            VportType::Geneve,
        ];
        for vt in types {
            assert_eq!(VportType::from_u32(vt.to_u32()).to_u32(), vt.to_u32());
        }
    }

    #[tokio::test]
    #[ignore] // Run only when OVS is installed and as root
    async fn test_list_datapaths() {
        let mut client = OvsNetlinkClient::new()
            .await
            .expect("Failed to create client");
        let dps = client
            .list_datapaths()
            .await
            .expect("Failed to list datapaths");
        println!("Datapaths: {:?}", dps);
    }

    #[tokio::test]
    #[ignore] // Run only when OVS is installed and as root
    async fn test_list_vports() {
        let mut client = OvsNetlinkClient::new()
            .await
            .expect("Failed to create client");
        let dps = client
            .list_datapaths()
            .await
            .expect("Failed to list datapaths");
        if let Some(dp) = dps.first() {
            let vports = client
                .list_vports(&dp.name)
                .await
                .expect("Failed to list vports");
            println!("Vports on {}: {:?}", dp.name, vports);
        }
    }

    #[tokio::test]
    #[ignore] // Run only when OVS is installed and as root
    async fn test_dump_flows() {
        let mut client = OvsNetlinkClient::new()
            .await
            .expect("Failed to create client");
        let dps = client
            .list_datapaths()
            .await
            .expect("Failed to list datapaths");
        if let Some(dp) = dps.first() {
            let flows = client
                .dump_flows(&dp.name)
                .await
                .expect("Failed to dump flows");
            println!("Flows on {}: {} flows found", dp.name, flows.len());
        }
    }
}
