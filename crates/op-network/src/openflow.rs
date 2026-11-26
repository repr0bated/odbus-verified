//! OpenFlow protocol types backed by `rovs-openflow`.
//!
//! This module re-exports and wraps rovs-openflow types to preserve
//! the public API surface expected by callers while delegating all
//! wire-protocol work to the library.

use anyhow::{Context, Result};
use bytes::{Bytes, BytesMut};
use rovs_openflow::{Message, MessageType, Version};
use rovs_transport::Reconnect;
use std::collections::HashMap;
use std::net::SocketAddr;

/// The rovs OpenFlow crate this client speaks, re-exported so the bridge's
/// switch owner builds and compares flows with the same types and encoding.
pub use rovs_openflow;
use std::path::Path;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

// Re-export rovs-openflow types used directly by callers.
pub use rovs_openflow::Match as FlowMatch;

/// Flow match field (alias for rovs-openflow `Match`).
///
/// Callers that previously constructed `FlowMatch { in_port: Some(n), .. }` should
/// now use the builder API: `FlowMatch::new().in_port(n)`.
///
/// Flow action — a simplified action enum that covers what callers need.
#[derive(Debug, Clone)]
pub enum FlowAction {
    /// Output to a specific port number.
    Output { port: u32 },
    /// Drop the packet (no instructions).
    Drop,
}

/// A flow entry for OpenFlow operations.
///
/// Maps to `rovs_openflow::Flow` when sent to the switch.
#[derive(Debug, Clone)]
pub struct FlowEntry {
    pub priority: u16,
    pub match_fields: FlowMatch,
    pub actions: Vec<FlowAction>,
    pub idle_timeout: u16,
    pub hard_timeout: u16,
    pub cookie: u64,
}

impl FlowEntry {
    /// Convert to a `rovs_openflow::Flow` ADD command.
    pub fn to_rovs_flow(&self) -> rovs_openflow::Flow {
        let mut action_list = rovs_openflow::ActionList::new();
        for action in &self.actions {
            match action {
                FlowAction::Output { port } => {
                    action_list = action_list.output(rovs_openflow::OutputPort::Port(*port));
                }
                FlowAction::Drop => {
                    // No actions = drop
                }
            }
        }

        rovs_openflow::Flow::add()
            .priority(self.priority)
            .match_fields(self.match_fields.clone())
            .actions(action_list)
            .idle_timeout(self.idle_timeout)
            .hard_timeout(self.hard_timeout)
            .cookie(self.cookie)
    }
}

/// OpenFlow protocol versions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenFlowVersion {
    V1_0,
    V1_3,
    V1_5,
}

impl OpenFlowVersion {
    pub fn as_u8(self) -> u8 {
        match self {
            Self::V1_0 => 0x01,
            Self::V1_3 => 0x04,
            Self::V1_5 => 0x06,
        }
    }
}

impl From<OpenFlowVersion> for rovs_openflow::Version {
    fn from(v: OpenFlowVersion) -> Self {
        match v {
            OpenFlowVersion::V1_0 => rovs_openflow::Version::Of10,
            OpenFlowVersion::V1_3 => rovs_openflow::Version::Of13,
            OpenFlowVersion::V1_5 => rovs_openflow::Version::Of15,
        }
    }
}

/// Longest wait for one switch reply.
const REPLY_TIMEOUT: Duration = Duration::from_secs(5);
/// OpenFlow message size ceiling accepted from the switch (header says u16).
const HEADER_LEN: usize = 8;

/// OpenFlow client: connects actively to an OpenFlow switch at OpenFlow 1.5.
///
/// `rovs_openflow::VConn::connect` always negotiates 1.3, so this client runs
/// the hello itself over `rovs_transport::Stream` with rovs `Message`/`Header`
/// encoding. It carries a `Reconnect` state machine; `reconnect_now` re-dials.
pub struct OpenFlowClient {
    addr: rovs_transport::Address,
    stream: rovs_transport::Stream,
    version: Version,
    xid: u32,
    /// Reconnection state machine: tracks backoff for the caller.
    pub reconnect: Reconnect,
}

impl OpenFlowClient {
    /// Connect to a switch listening on TCP.
    pub async fn connect(addr: SocketAddr) -> Result<Self> {
        Self::connect_address(&rovs_transport::Address::Tcp {
            host: addr.ip().to_string(),
            port: addr.port(),
        })
        .await
    }

    /// Connect to OVS's own bridge socket, e.g. `/var/run/openvswitch/ovsbr0.mgmt`.
    pub async fn connect_unix(path: &Path) -> Result<Self> {
        Self::connect_address(&rovs_transport::Address::Unix(path.to_path_buf())).await
    }

    /// Connect to any rovs address and negotiate OpenFlow 1.5.
    pub async fn connect_address(addr: &rovs_transport::Address) -> Result<Self> {
        let mut reconnect = Reconnect::new();
        reconnect.set_max_backoff(Duration::from_secs(30));
        reconnect.connecting();
        match handshake(addr).await {
            Ok((stream, version)) => {
                reconnect.connected();
                Ok(Self {
                    addr: addr.clone(),
                    stream,
                    version,
                    xid: 1,
                    reconnect,
                })
            }
            Err(e) => {
                reconnect.disconnected();
                reconnect.increase_backoff();
                Err(e).with_context(|| format!("Failed to connect to OpenFlow switch at {addr:?}"))
            }
        }
    }

    /// Negotiated OpenFlow version.
    pub fn version(&self) -> Version {
        self.version
    }

    /// Drop the current connection and dial again with a fresh hello.
    pub async fn reconnect_now(&mut self) -> Result<()> {
        self.reconnect.disconnected();
        self.reconnect.connecting();
        match handshake(&self.addr).await {
            Ok((stream, version)) => {
                self.stream = stream;
                self.version = version;
                self.xid = 1;
                self.reconnect.connected();
                Ok(())
            }
            Err(e) => {
                self.reconnect.disconnected();
                self.reconnect.increase_backoff();
                Err(e).context("Failed to reconnect to OpenFlow switch")
            }
        }
    }

    fn next_xid(&mut self) -> u32 {
        self.xid = self.xid.wrapping_add(1);
        self.xid
    }

    async fn send(&mut self, msg: &Message) -> Result<()> {
        write_message(&mut self.stream, msg).await
    }

    /// Wait for the reply `want` to `xid`. Echo requests from the switch are
    /// answered on the way; an `Error` for `xid` is returned as an error.
    async fn await_reply(&mut self, xid: u32, want: MessageType) -> Result<Message> {
        loop {
            let msg = read_message(&mut self.stream).await?;
            match msg.header.msg_type {
                MessageType::EchoRequest => {
                    let reply = Message::new(
                        self.version,
                        MessageType::EchoReply,
                        msg.header.xid,
                        msg.body,
                    );
                    self.send(&reply).await?;
                }
                MessageType::Error if msg.header.xid == xid => {
                    anyhow::bail!(
                        "switch refused request: {:?}",
                        rovs_openflow::OfError::parse(&msg.body)
                    );
                }
                t if t == want && msg.header.xid == xid => return Ok(msg),
                _ => {} // async messages (port status, packet-in, ...) are not replies
            }
        }
    }

    /// Send a message that changes the switch, then a barrier; an `Error` for
    /// the message that arrives before the barrier reply is returned.
    async fn send_synced(&mut self, msg: &Message) -> Result<()> {
        let barrier_xid = self.next_xid();
        let barrier = Message::new(
            self.version,
            MessageType::BarrierRequest,
            barrier_xid,
            Bytes::new(),
        );
        self.send(msg).await?;
        self.send(&barrier).await?;
        loop {
            let reply = read_message(&mut self.stream).await?;
            match reply.header.msg_type {
                MessageType::EchoRequest => {
                    let r = Message::new(
                        self.version,
                        MessageType::EchoReply,
                        reply.header.xid,
                        reply.body,
                    );
                    self.send(&r).await?;
                }
                MessageType::Error if reply.header.xid == msg.header.xid => {
                    anyhow::bail!(
                        "switch refused request: {:?}",
                        rovs_openflow::OfError::parse(&reply.body)
                    );
                }
                MessageType::BarrierReply if reply.header.xid == barrier_xid => return Ok(()),
                _ => {}
            }
        }
    }

    /// Add a flow entry to the switch and wait for the barrier.
    pub async fn add_flow(&mut self, flow: &FlowEntry) -> Result<()> {
        let xid = self.next_xid();
        let msg = flow.to_rovs_flow().to_message(self.version, xid);
        self.send_synced(&msg)
            .await
            .context("Failed to install flow")?;
        self.reconnect.activity();
        Ok(())
    }

    /// Delete all flows on the switch (wildcard delete).
    pub async fn delete_all_flows(&mut self) -> Result<()> {
        let xid = self.next_xid();
        let msg = rovs_openflow::Flow::delete().to_message(self.version, xid);
        self.send_synced(&msg)
            .await
            .context("Failed to delete all flows")?;
        self.reconnect.activity();
        Ok(())
    }

    /// Barrier: returns when the switch has processed everything sent before.
    pub async fn barrier(&mut self) -> Result<()> {
        let xid = self.next_xid();
        let msg = Message::new(self.version, MessageType::BarrierRequest, xid, Bytes::new());
        self.send(&msg).await?;
        self.await_reply(xid, MessageType::BarrierReply)
            .await
            .context("Barrier failed")?;
        self.reconnect.activity();
        Ok(())
    }

    /// Send an echo request and wait for reply (keepalive).
    pub async fn echo(&mut self) -> Result<()> {
        let xid = self.next_xid();
        let msg = Message::new(self.version, MessageType::EchoRequest, xid, Bytes::new());
        self.send(&msg).await?;
        self.await_reply(xid, MessageType::EchoReply)
            .await
            .context("Echo request failed")?;
        self.reconnect.activity();
        Ok(())
    }

    /// Send a FeaturesRequest and wait for FeaturesReply (connectivity probe).
    pub async fn request_features(&mut self) -> Result<()> {
        let xid = self.next_xid();
        let msg = Message::new(
            self.version,
            MessageType::FeaturesRequest,
            xid,
            Bytes::new(),
        );
        self.send(&msg).await?;
        self.await_reply(xid, MessageType::FeaturesReply)
            .await
            .context("FeaturesReply not received")?;
        Ok(())
    }
}

/// `ofp_flow_desc` fixed part: length, pad, table, pad, priority, idle,
/// hard, flags, importance, cookie.
const FLOW_DESC_FIXED: usize = 24;
/// Multipart type of a flow description/stats request.
const OFPMP_FLOW_DESC: u16 = 1;
/// Multipart type of a port description request.
const OFPMP_PORT_DESC: u16 = 13;
/// Multipart reply flag: more replies follow.
const OFPMPF_REPLY_MORE: u16 = 1;
/// OXS class of the standard flow statistics.
const OXS_CLASS_OPENFLOW_BASIC: u16 = 0x8002;
const OXS_DURATION: u8 = 0;
const OXS_IDLE_TIME: u8 = 1;
const OXS_PACKET_COUNT: u8 = 4;
const OXS_BYTE_COUNT: u8 = 5;
/// Ceiling on one flow dump, so a broken peer cannot exhaust memory.
const MAX_DUMP_BYTES: usize = 8 * 1024 * 1024;

/// One flow as the switch reports it at OpenFlow 1.5 (`ofp_flow_desc`).
#[derive(Debug, Clone)]
pub struct SwitchFlow {
    pub table_id: u8,
    pub priority: u16,
    pub idle_timeout: u16,
    pub hard_timeout: u16,
    pub flags: u16,
    pub importance: u16,
    pub cookie: u64,
    pub match_fields: FlowMatch,
    pub instructions: rovs_openflow::InstructionList,
    pub duration_sec: u32,
    pub duration_nsec: u32,
    /// Seconds since the flow last matched, when the switch reports it.
    pub idle_time_sec: Option<u32>,
    pub packet_count: u64,
    pub byte_count: u64,
}

fn be16(b: &[u8], at: usize) -> u16 {
    u16::from_be_bytes([b[at], b[at + 1]])
}

fn be32(b: &[u8], at: usize) -> u32 {
    u32::from_be_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

fn be64(b: &[u8], at: usize) -> u64 {
    u64::from_be_bytes(b[at..at + 8].try_into().expect("8 bytes"))
}

fn pad8(n: usize) -> usize {
    n.div_ceil(8) * 8
}

/// Decode one OpenFlow 1.5 `ofp_flow_desc` entry; returns the flow and the
/// bytes it used. rovs 0.2.0 only decodes the 1.3 `ofp_flow_stats` layout, so
/// the framing and OXS statistics are read here and the match and
/// instructions are decoded by rovs.
pub fn decode_flow_desc(data: &[u8]) -> Result<(SwitchFlow, usize)> {
    anyhow::ensure!(
        data.len() >= FLOW_DESC_FIXED,
        "flow entry shorter than its fixed part"
    );
    let len = usize::from(be16(data, 0));
    anyhow::ensure!(
        len >= FLOW_DESC_FIXED,
        "flow entry length {len} below the fixed part"
    );
    anyhow::ensure!(
        data.len() >= len,
        "flow entry truncated: {} of {len} bytes",
        data.len()
    );
    let e = &data[..len];

    let (match_fields, match_used) = rovs_openflow::Match::decode(&e[FLOW_DESC_FIXED..])
        .map_err(|err| anyhow::anyhow!("flow match: {err}"))?;
    let stats_at = FLOW_DESC_FIXED + match_used;
    anyhow::ensure!(stats_at + 4 <= len, "flow entry ends before its statistics");
    let stats_len = usize::from(be16(e, stats_at + 2));
    anyhow::ensure!(
        stats_len >= 4,
        "statistics length {stats_len} below its header"
    );
    let stats_end = stats_at + stats_len;
    let instr_at = stats_at + pad8(stats_len);
    anyhow::ensure!(instr_at <= len, "statistics run past the flow entry");

    let mut flow = SwitchFlow {
        table_id: e[4],
        priority: be16(e, 6),
        idle_timeout: be16(e, 8),
        hard_timeout: be16(e, 10),
        flags: be16(e, 12),
        importance: be16(e, 14),
        cookie: be64(e, 16),
        match_fields,
        instructions: rovs_openflow::InstructionList::decode(&e[instr_at..])
            .map_err(|err| anyhow::anyhow!("flow instructions: {err}"))?,
        duration_sec: 0,
        duration_nsec: 0,
        idle_time_sec: None,
        packet_count: 0,
        byte_count: 0,
    };

    let mut at = stats_at + 4;
    while at + 4 <= stats_end {
        let class = be16(e, at);
        let field = e[at + 2] >> 1;
        let vlen = usize::from(e[at + 3]);
        let v = at + 4;
        anyhow::ensure!(
            v + vlen <= stats_end,
            "statistic field runs past the statistics"
        );
        if class == OXS_CLASS_OPENFLOW_BASIC {
            match (field, vlen) {
                (OXS_DURATION, 8) => {
                    flow.duration_sec = be32(e, v);
                    flow.duration_nsec = be32(e, v + 4);
                }
                (OXS_IDLE_TIME, 8) => flow.idle_time_sec = Some(be32(e, v)),
                (OXS_PACKET_COUNT, 8) => flow.packet_count = be64(e, v),
                (OXS_BYTE_COUNT, 8) => flow.byte_count = be64(e, v),
                _ => {} // other statistics (e.g. flow_count) are not used
            }
        }
        at = v + vlen;
    }
    Ok((flow, len))
}

impl OpenFlowClient {
    /// Send a multipart request of type `ty` and collect the bodies (after the
    /// 8-byte multipart header) of every reply, following REPLY_MORE. The
    /// total size is bounded.
    async fn multipart(&mut self, req: Message, ty: u16) -> Result<Vec<Bytes>> {
        let xid = req.header.xid;
        self.send(&req).await?;
        let mut parts = Vec::new();
        let mut total = 0usize;
        loop {
            let reply = self.await_reply(xid, MessageType::MultipartReply).await?;
            let b = reply.body;
            total += b.len();
            anyhow::ensure!(
                total <= MAX_DUMP_BYTES,
                "multipart reply larger than {MAX_DUMP_BYTES} bytes"
            );
            anyhow::ensure!(b.len() >= 8, "multipart reply shorter than its header");
            anyhow::ensure!(
                be16(&b, 0) == ty,
                "unexpected multipart type {} (wanted {ty})",
                be16(&b, 0)
            );
            let more = be16(&b, 2) & OFPMPF_REPLY_MORE != 0;
            parts.push(b.slice(8..));
            if !more {
                break;
            }
        }
        self.reconnect.activity();
        Ok(parts)
    }

    /// Read every flow on the switch (all tables), following multipart
    /// replies until the last one.
    pub async fn dump_flows(&mut self) -> Result<Vec<SwitchFlow>> {
        let xid = self.next_xid();
        let req = rovs_openflow::FlowStatsRequest::new().to_message(self.version, xid);
        let mut flows = Vec::new();
        for body in self.multipart(req, OFPMP_FLOW_DESC).await? {
            let mut at = 0;
            while at < body.len() {
                let (flow, used) = decode_flow_desc(&body[at..])?;
                flows.push(flow);
                at += used;
            }
        }
        Ok(flows)
    }

    /// Read the switch's ports (`OFPMP_PORT_DESC`): name to (port number, MAC).
    pub async fn port_desc(&mut self) -> Result<HashMap<String, (u32, [u8; 6])>> {
        let xid = self.next_xid();
        // Multipart request header: type, flags, 4 pad bytes; no body.
        let mut body = Vec::with_capacity(8);
        body.extend_from_slice(&OFPMP_PORT_DESC.to_be_bytes());
        body.extend_from_slice(&[0u8; 6]);
        let req = Message::new(
            self.version,
            MessageType::MultipartRequest,
            xid,
            Bytes::from(body),
        );
        let mut ports = HashMap::new();
        for part in self.multipart(req, OFPMP_PORT_DESC).await? {
            for p in decode_port_desc(&part)? {
                anyhow::ensure!(
                    ports
                        .insert(p.name.clone(), (p.port_no, p.hw_addr))
                        .is_none(),
                    "port {} reported twice",
                    p.name
                );
            }
        }
        Ok(ports)
    }

    /// Send one FLOW_MOD (any command) and a barrier; an `Error` for the
    /// FLOW_MOD is an error. No retry: the caller reads the table back.
    pub async fn flow_mod(&mut self, flow: &rovs_openflow::Flow) -> Result<()> {
        let xid = self.next_xid();
        let msg = flow.to_message(self.version, xid);
        self.send_synced(&msg)
            .await
            .context("Failed to send flow-mod")?;
        self.reconnect.activity();
        Ok(())
    }
}

/// One port of the switch (`ofp_port`, OpenFlow 1.4+ layout).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwitchPort {
    pub port_no: u32,
    pub hw_addr: [u8; 6],
    pub name: String,
}

/// `ofp_port` fixed part: port_no, length, pad2, hw_addr[6], pad2, name[16],
/// config, state.
const PORT_FIXED: usize = 40;

/// Decode the `ofp_port` entries of one PORT_DESC reply. Properties after the
/// fixed part are skipped using the entry's own length.
pub fn decode_port_desc(data: &[u8]) -> Result<Vec<SwitchPort>> {
    let mut ports: Vec<SwitchPort> = Vec::new();
    let mut at = 0;
    while at < data.len() {
        let e = &data[at..];
        anyhow::ensure!(
            e.len() >= PORT_FIXED,
            "port entry shorter than its fixed part"
        );
        let len = usize::from(be16(e, 4));
        anyhow::ensure!(
            len >= PORT_FIXED,
            "port entry length {len} below the fixed part"
        );
        anyhow::ensure!(
            len <= e.len(),
            "port entry truncated: {} of {len} bytes",
            e.len()
        );
        let name_raw = &e[16..32];
        let end = name_raw
            .iter()
            .position(|b| *b == 0)
            .unwrap_or(name_raw.len());
        let name = String::from_utf8_lossy(&name_raw[..end]).into_owned();
        anyhow::ensure!(
            !ports.iter().any(|p| p.name == name),
            "port {name} reported twice"
        );
        ports.push(SwitchPort {
            port_no: be32(e, 0),
            hw_addr: e[8..14].try_into().expect("6 bytes"),
            name,
        });
        at += len;
    }
    Ok(ports)
}

/// Dial `addr` and exchange hellos at OpenFlow 1.5; the result is the
/// lower of the two versions.
async fn handshake(addr: &rovs_transport::Address) -> Result<(rovs_transport::Stream, Version)> {
    let mut stream = rovs_transport::Stream::connect(addr)
        .await
        .context("dial")?;
    let hello = Message::new(Version::Of15, MessageType::Hello, 1, Bytes::new());
    write_message(&mut stream, &hello).await?;
    loop {
        let msg = read_message(&mut stream)
            .await
            .context("waiting for hello")?;
        if msg.header.msg_type == MessageType::Hello {
            return Ok((stream, msg.header.version.min(Version::Of15)));
        }
    }
}

async fn write_message(stream: &mut rovs_transport::Stream, msg: &Message) -> Result<()> {
    stream.write_all(&msg.encode()).await.context("write")?;
    stream.flush().await.context("flush")?;
    Ok(())
}

/// Read one whole message, with a timeout.
async fn read_message(stream: &mut rovs_transport::Stream) -> Result<Message> {
    tokio::time::timeout(REPLY_TIMEOUT, async {
        let mut head = [0u8; HEADER_LEN];
        stream.read_exact(&mut head).await.context("read header")?;
        let len = u16::from_be_bytes([head[2], head[3]]) as usize;
        anyhow::ensure!(len >= HEADER_LEN, "short OpenFlow message length {len}");
        let mut body = BytesMut::zeroed(len - HEADER_LEN);
        stream.read_exact(&mut body).await.context("read body")?;
        let header = rovs_openflow::Header::decode(&mut Bytes::copy_from_slice(&head))
            .map_err(|e| anyhow::anyhow!("bad OpenFlow header {head:02x?}: {e}"))?;
        Ok(Message {
            header,
            body: body.freeze(),
        })
    })
    .await
    .map_err(|_| anyhow::anyhow!("timed out waiting for the switch"))?
}

/// An in-process fake OpenFlow 1.5 switch with a real flow table, for tests
/// here and (through the `fake-switch` feature) in dependent crates. It
/// applies `Add`, `ModifyStrict` and `DeleteStrict` FLOW_MODs it receives and
/// answers flow-description and port-description multiparts from its state.
#[cfg(any(test, feature = "fake-switch"))]
pub mod fake_switch {
    use super::{Bytes, Message, MessageType, Version, be16, be64, pad8};
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{UnixListener, UnixStream};

    /// One flow of the fake table, kept in wire form.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct StoredFlow {
        pub table: u8,
        pub priority: u16,
        pub cookie: u64,
        pub idle_timeout: u16,
        pub hard_timeout: u16,
        /// `Match::encode()` bytes (padded).
        pub match_bytes: Vec<u8>,
        /// Encoded instruction list.
        pub instruction_bytes: Vec<u8>,
    }

    impl StoredFlow {
        pub fn from_flow(flow: &rovs_openflow::Flow) -> Self {
            let instruction_bytes = if flow.instructions.is_empty() && !flow.actions.is_empty() {
                rovs_openflow::Instruction::ApplyActions(flow.actions.clone()).encode()
            } else {
                flow.instructions.encode()
            };
            Self {
                table: flow.table_id,
                priority: flow.priority,
                cookie: flow.cookie,
                idle_timeout: flow.idle_timeout,
                hard_timeout: flow.hard_timeout,
                match_bytes: flow.match_fields.encode(),
                instruction_bytes,
            }
        }

        fn same_identity(&self, other: &StoredFlow) -> bool {
            (self.table, self.priority, &self.match_bytes)
                == (other.table, other.priority, &other.match_bytes)
        }

        /// One `ofp_flow_desc` entry (OpenFlow 1.5 layout, as captured from OVS).
        fn encode_desc(&self) -> Vec<u8> {
            let mut stats = vec![0u8, 0, 0, 52];
            for (field, value) in [(0u8, (7u64 << 32) | 5), (1, 3), (4, 11), (5, 1234)] {
                stats.extend_from_slice(&0x8002u16.to_be_bytes());
                stats.push(field << 1);
                stats.push(8);
                if field == 0 {
                    stats.extend_from_slice(&((value >> 32) as u32).to_be_bytes());
                    stats.extend_from_slice(&(value as u32).to_be_bytes());
                } else if field == 1 {
                    stats.extend_from_slice(&(value as u32).to_be_bytes());
                    stats.extend_from_slice(&0u32.to_be_bytes());
                } else {
                    stats.extend_from_slice(&value.to_be_bytes());
                }
            }
            assert_eq!(stats.len(), 52);
            stats.resize(pad8(stats.len()), 0);
            let len = 24 + self.match_bytes.len() + stats.len() + self.instruction_bytes.len();
            let mut e = Vec::with_capacity(len);
            e.extend_from_slice(&(len as u16).to_be_bytes());
            e.extend_from_slice(&[0, self.table, 0]);
            e.extend_from_slice(&self.priority.to_be_bytes());
            e.extend_from_slice(&self.idle_timeout.to_be_bytes());
            e.extend_from_slice(&self.hard_timeout.to_be_bytes());
            e.extend_from_slice(&[0, 0, 0, 0]); // flags, importance
            e.extend_from_slice(&self.cookie.to_be_bytes());
            e.extend_from_slice(&self.match_bytes);
            e.extend_from_slice(&stats);
            e.extend_from_slice(&self.instruction_bytes);
            e
        }
    }

    #[derive(Default)]
    struct State {
        flows: Mutex<Vec<StoredFlow>>,
        ports: Mutex<Vec<(String, u32, [u8; 6])>>,
        flow_mods: AtomicUsize,
        hellos: AtomicUsize,
        ignore_flow_mods: AtomicBool,
        close_after_hello: AtomicUsize,
    }

    /// Handle to a running fake switch.
    pub struct FakeSwitch {
        path: PathBuf,
        state: Arc<State>,
    }

    impl FakeSwitch {
        /// Listen on `<dir>/<bridge>.mgmt` and serve every connection.
        pub fn start(dir: &Path, bridge: &str, ports: &[(&str, u32, [u8; 6])]) -> Self {
            let path = dir.join(format!("{bridge}.mgmt"));
            let listener = UnixListener::bind(&path).expect("bind fake switch socket");
            let state = Arc::new(State::default());
            *state.ports.lock().unwrap() = ports
                .iter()
                .map(|(n, no, mac)| ((*n).to_owned(), *no, *mac))
                .collect();
            let task_state = state.clone();
            tokio::spawn(async move {
                while let Ok((stream, _)) = listener.accept().await {
                    tokio::spawn(serve(stream, task_state.clone()));
                }
            });
            Self { path, state }
        }

        pub fn path(&self) -> &Path {
            &self.path
        }

        /// The fake table as it is now.
        pub fn flows(&self) -> Vec<StoredFlow> {
            self.state.flows.lock().unwrap().clone()
        }

        /// Put a flow in the table without a FLOW_MOD (a flow that "was already there").
        pub fn seed(&self, flow: &rovs_openflow::Flow) {
            self.state
                .flows
                .lock()
                .unwrap()
                .push(StoredFlow::from_flow(flow));
        }

        /// FLOW_MOD messages received so far.
        pub fn flow_mods(&self) -> usize {
            self.state.flow_mods.load(Ordering::SeqCst)
        }

        /// Hello messages received so far (one per connection).
        pub fn hellos(&self) -> usize {
            self.state.hellos.load(Ordering::SeqCst)
        }

        /// When set, FLOW_MODs are received and barriered but not applied.
        pub fn set_ignore_flow_mods(&self, ignore: bool) {
            self.state.ignore_flow_mods.store(ignore, Ordering::SeqCst);
        }

        /// Close the next `n` connections right after the hello.
        pub fn close_next_connections(&self, n: usize) {
            self.state.close_after_hello.store(n, Ordering::SeqCst);
        }
    }

    async fn read(s: &mut UnixStream) -> Option<Message> {
        let mut h = [0u8; 8];
        s.read_exact(&mut h).await.ok()?;
        let len = u16::from_be_bytes([h[2], h[3]]) as usize;
        let mut body = vec![0u8; len.checked_sub(8)?];
        s.read_exact(&mut body).await.ok()?;
        let header = rovs_openflow::Header::decode(&mut Bytes::copy_from_slice(&h)).ok()?;
        Some(Message {
            header,
            body: Bytes::from(body),
        })
    }

    async fn send(s: &mut UnixStream, t: MessageType, xid: u32, body: Vec<u8>) {
        let _ = s
            .write_all(&Message::new(Version::Of15, t, xid, Bytes::from(body)).encode())
            .await;
    }

    /// Reply to a multipart request with one entry per reply, REPLY_MORE on all
    /// but the last (a single empty reply when there are no entries).
    async fn multipart(s: &mut UnixStream, xid: u32, ty: u16, entries: Vec<Vec<u8>>) {
        let n = entries.len().max(1);
        for i in 0..n {
            let more = u8::from(i + 1 < n);
            let mut body = vec![(ty >> 8) as u8, ty as u8, 0, more, 0, 0, 0, 0];
            if let Some(e) = entries.get(i) {
                body.extend_from_slice(e);
            }
            send(s, MessageType::MultipartReply, xid, body).await;
        }
    }

    async fn serve(mut s: UnixStream, state: Arc<State>) {
        let Some(hello) = read(&mut s).await else {
            return;
        };
        state.hellos.fetch_add(1, Ordering::SeqCst);
        send(&mut s, MessageType::Hello, hello.header.xid, Vec::new()).await;
        if state
            .close_after_hello
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
            .is_ok()
        {
            return;
        }
        while let Some(m) = read(&mut s).await {
            let xid = m.header.xid;
            match m.header.msg_type {
                MessageType::EchoRequest => {
                    send(&mut s, MessageType::EchoReply, xid, m.body.to_vec()).await
                }
                MessageType::BarrierRequest => {
                    send(&mut s, MessageType::BarrierReply, xid, Vec::new()).await
                }
                MessageType::FlowMod => {
                    state.flow_mods.fetch_add(1, Ordering::SeqCst);
                    if !state.ignore_flow_mods.load(Ordering::SeqCst) {
                        apply_flow_mod(&state, &m.body);
                    }
                }
                MessageType::MultipartRequest if m.body.len() >= 8 => match be16(&m.body, 0) {
                    1 => {
                        let entries = state
                            .flows
                            .lock()
                            .unwrap()
                            .iter()
                            .map(StoredFlow::encode_desc)
                            .collect();
                        multipart(&mut s, xid, 1, entries).await;
                    }
                    13 => {
                        let entries = state
                            .ports
                            .lock()
                            .unwrap()
                            .iter()
                            .map(|(name, no, mac)| encode_port(name, *no, *mac))
                            .collect();
                        multipart(&mut s, xid, 13, entries).await;
                    }
                    _ => {}
                },
                _ => {}
            }
        }
    }

    /// One OpenFlow 1.4+ `ofp_port`, no properties.
    fn encode_port(name: &str, no: u32, mac: [u8; 6]) -> Vec<u8> {
        let mut e = Vec::with_capacity(40);
        e.extend_from_slice(&no.to_be_bytes());
        e.extend_from_slice(&40u16.to_be_bytes());
        e.extend_from_slice(&[0, 0]);
        e.extend_from_slice(&mac);
        e.extend_from_slice(&[0, 0]);
        let mut n = [0u8; 16];
        n[..name.len()].copy_from_slice(name.as_bytes());
        e.extend_from_slice(&n);
        e.extend_from_slice(&[0u8; 8]); // config, state
        e
    }

    fn apply_flow_mod(state: &State, body: &[u8]) {
        if body.len() < 40 {
            return;
        }
        let Ok((_, used)) = rovs_openflow::Match::decode(&body[40..]) else {
            return;
        };
        let incoming = StoredFlow {
            table: body[16],
            priority: be16(body, 22),
            cookie: be64(body, 0),
            idle_timeout: be16(body, 18),
            hard_timeout: be16(body, 20),
            match_bytes: body[40..40 + used].to_vec(),
            instruction_bytes: body[40 + used..].to_vec(),
        };
        let mask = be64(body, 8);
        let cookie_ok = |f: &StoredFlow| f.cookie & mask == incoming.cookie & mask;
        let mut flows = state.flows.lock().unwrap();
        match body[17] {
            0 => {
                flows.retain(|f| !f.same_identity(&incoming));
                flows.push(incoming);
            }
            2 => {
                for f in flows.iter_mut() {
                    if f.same_identity(&incoming) && cookie_ok(f) {
                        f.idle_timeout = incoming.idle_timeout;
                        f.hard_timeout = incoming.hard_timeout;
                        f.instruction_bytes = incoming.instruction_bytes.clone();
                    }
                }
            }
            4 => flows.retain(|f| !(f.same_identity(&incoming) && cookie_ok(f))),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use tokio::net::{UnixListener, UnixStream};

    /// What the fake switch does after the hello.
    #[derive(Clone, Copy)]
    enum Script {
        /// Answer echo and barrier; record every other message.
        Normal,
        /// Ask the client for an echo before answering its barrier.
        EchoFirst,
        /// Close right after the hello.
        CloseAfterHello,
        /// Answer a flow-mod with an Error, then the barrier.
        RefuseFlowMod,
        /// Answer a flow multipart request with two replies (REPLY_MORE, then last),
        /// each holding the captured `ovsbr0` entry.
        FlowDump,
    }

    type Seen = Arc<Mutex<Vec<(MessageType, Version)>>>;

    async fn fake_read(s: &mut UnixStream) -> Option<Message> {
        let mut h = [0u8; 8];
        s.read_exact(&mut h).await.ok()?;
        let len = u16::from_be_bytes([h[2], h[3]]) as usize;
        let mut body = vec![0u8; len - 8];
        s.read_exact(&mut body).await.ok()?;
        let header = rovs_openflow::Header::decode(&mut Bytes::copy_from_slice(&h)).ok()?;
        Some(Message {
            header,
            body: Bytes::from(body),
        })
    }

    async fn fake_send(s: &mut UnixStream, v: Version, t: MessageType, xid: u32, body: Bytes) {
        s.write_all(&Message::new(v, t, xid, body).encode())
            .await
            .unwrap();
    }

    /// Serve every accepted connection with `script`; returns the socket path.
    fn fake_switch(dir: &Path, scripts: Vec<Script>, seen: Seen) -> std::path::PathBuf {
        let path = dir.join("ovsbr0.mgmt");
        let listener = UnixListener::bind(&path).unwrap();
        tokio::spawn(async move {
            for script in scripts {
                let (mut s, _) = listener.accept().await.unwrap();
                let hello = fake_read(&mut s).await.unwrap();
                seen.lock()
                    .unwrap()
                    .push((hello.header.msg_type, hello.header.version));
                fake_send(
                    &mut s,
                    Version::Of15,
                    MessageType::Hello,
                    hello.header.xid,
                    Bytes::new(),
                )
                .await;
                if matches!(script, Script::CloseAfterHello) {
                    continue;
                }
                let mut refuse_xid = None;
                while let Some(m) = fake_read(&mut s).await {
                    seen.lock()
                        .unwrap()
                        .push((m.header.msg_type, m.header.version));
                    let xid = m.header.xid;
                    match m.header.msg_type {
                        MessageType::EchoRequest => {
                            fake_send(&mut s, Version::Of15, MessageType::EchoReply, xid, m.body)
                                .await
                        }
                        MessageType::FlowMod if matches!(script, Script::RefuseFlowMod) => {
                            refuse_xid = Some(xid);
                        }
                        MessageType::MultipartRequest if matches!(script, Script::FlowDump) => {
                            for more in [true, false] {
                                let mut body = vec![0, 1, 0, u8::from(more), 0, 0, 0, 0];
                                body.extend_from_slice(&ovsbr0_normal_entry());
                                fake_send(
                                    &mut s,
                                    Version::Of15,
                                    MessageType::MultipartReply,
                                    xid,
                                    Bytes::from(body),
                                )
                                .await;
                            }
                        }
                        MessageType::BarrierRequest => {
                            if let Some(bad) = refuse_xid.take() {
                                fake_send(
                                    &mut s,
                                    Version::Of15,
                                    MessageType::Error,
                                    bad,
                                    Bytes::from_static(&[0, 5, 0, 0]),
                                )
                                .await;
                            }
                            if matches!(script, Script::EchoFirst) {
                                fake_send(
                                    &mut s,
                                    Version::Of15,
                                    MessageType::EchoRequest,
                                    900,
                                    Bytes::new(),
                                )
                                .await;
                                let r = fake_read(&mut s).await.unwrap();
                                seen.lock()
                                    .unwrap()
                                    .push((r.header.msg_type, r.header.version));
                            }
                            fake_send(
                                &mut s,
                                Version::Of15,
                                MessageType::BarrierReply,
                                xid,
                                Bytes::new(),
                            )
                            .await;
                        }
                        _ => {}
                    }
                }
            }
        });
        path
    }

    /// One `ofp_flow_desc` entry captured from `ovsbr0.mgmt` on 2026-11-26
    /// (read-only probe): the priority-0 NORMAL flow, cookie 0, duration
    /// 15031 s, 977674 packets, 0x355baca0 bytes.
    fn ovsbr0_normal_entry() -> Vec<u8> {
        const HEX: &str = "0070000000000000000000000000000000000000000000000001000400000000\
                           000000348002000800003ab710089d40800202080000000000000000800208080000\
                           0000000eeb0a80020a0800000000355baca000000000000400180000000000000010\
                           fffffffa0000000000000000";
        (0..HEX.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&HEX[i..i + 2], 16).unwrap())
            .collect()
    }

    #[test]
    fn decodes_the_captured_ovsbr0_flow_desc_entry() {
        let raw = ovsbr0_normal_entry();
        assert_eq!(raw.len(), 112);
        let (f, used) = decode_flow_desc(&raw).unwrap();
        assert_eq!(used, 112);
        assert_eq!((f.table_id, f.priority, f.cookie), (0, 0, 0));
        assert_eq!(
            (f.idle_timeout, f.hard_timeout, f.flags, f.importance),
            (0, 0, 0, 0)
        );
        assert_eq!(f.duration_sec, 0x3ab7);
        assert_eq!(f.duration_nsec, 0x1008_9d40);
        assert_eq!(f.idle_time_sec, Some(0));
        assert_eq!(f.packet_count, 977_674);
        assert_eq!(f.byte_count, 0x355b_aca0);
        assert_eq!(
            f.match_fields.encode(),
            [0, 1, 0, 4, 0, 0, 0, 0],
            "empty match"
        );
        // One apply-actions instruction holding output:NORMAL. (Not compared
        // byte for byte: rovs re-encodes output max_len as 0xffff, OVS sent 0.)
        match f.instructions.instructions() {
            [rovs_openflow::Instruction::ApplyActions(a)] => assert!(
                matches!(
                    a.actions(),
                    [rovs_openflow::Action::Output(
                        rovs_openflow::OutputPort::Normal
                    )]
                ),
                "{a:?}"
            ),
            other => panic!("expected one apply-actions instruction, got {other:?}"),
        }
    }

    #[test]
    fn flow_desc_decoder_rejects_truncation_and_bad_lengths() {
        let raw = ovsbr0_normal_entry();
        assert!(
            decode_flow_desc(&raw[..20]).is_err(),
            "shorter than the fixed part"
        );
        assert!(
            decode_flow_desc(&raw[..100]).is_err(),
            "shorter than its length field"
        );
        let mut bad = raw.clone();
        bad[0] = 0;
        bad[1] = 8;
        assert!(
            decode_flow_desc(&bad).is_err(),
            "length below the fixed part"
        );
        let mut bad_stats = raw.clone();
        bad_stats[35] = 0x60; // stats length runs past the entry
        assert!(decode_flow_desc(&bad_stats).is_err(), "stats overrun");
    }

    #[tokio::test]
    async fn dump_flows_reads_every_multipart_reply_at_of15() {
        let dir = tempfile::tempdir().unwrap();
        let seen: Seen = Default::default();
        let path = fake_switch(dir.path(), vec![Script::FlowDump], seen.clone());
        let mut c = OpenFlowClient::connect_unix(&path).await.unwrap();
        let flows = c.dump_flows().await.unwrap();
        assert_eq!(flows.len(), 2, "one entry per reply, REPLY_MORE followed");
        assert!(flows.iter().all(|f| f.packet_count == 977_674));
        assert!(
            seen.lock()
                .unwrap()
                .contains(&(MessageType::MultipartRequest, Version::Of15))
        );
    }

    fn entry() -> FlowEntry {
        FlowEntry {
            priority: 10,
            match_fields: FlowMatch::new().in_port(1),
            actions: vec![FlowAction::Output { port: 2 }],
            idle_timeout: 0,
            hard_timeout: 0,
            cookie: 0x3344_4348_0000_0001,
        }
    }

    #[tokio::test]
    async fn hello_is_sent_and_negotiated_at_of15() {
        let dir = tempfile::tempdir().unwrap();
        let seen: Seen = Default::default();
        let path = fake_switch(dir.path(), vec![Script::Normal], seen.clone());
        let c = OpenFlowClient::connect_unix(&path).await.unwrap();
        assert_eq!(c.version(), Version::Of15);
        assert!(c.reconnect.is_connected());
        assert_eq!(seen.lock().unwrap()[0], (MessageType::Hello, Version::Of15));
    }

    #[tokio::test]
    async fn echo_round_trips_at_of15() {
        let dir = tempfile::tempdir().unwrap();
        let seen: Seen = Default::default();
        let path = fake_switch(dir.path(), vec![Script::Normal], seen.clone());
        let mut c = OpenFlowClient::connect_unix(&path).await.unwrap();
        c.echo().await.unwrap();
        assert!(
            seen.lock()
                .unwrap()
                .contains(&(MessageType::EchoRequest, Version::Of15))
        );
    }

    #[tokio::test]
    async fn switch_echo_request_is_answered_while_waiting_for_barrier() {
        let dir = tempfile::tempdir().unwrap();
        let seen: Seen = Default::default();
        let path = fake_switch(dir.path(), vec![Script::EchoFirst], seen.clone());
        let mut c = OpenFlowClient::connect_unix(&path).await.unwrap();
        c.barrier().await.unwrap();
        assert!(
            seen.lock()
                .unwrap()
                .contains(&(MessageType::EchoReply, Version::Of15))
        );
    }

    #[tokio::test]
    async fn add_flow_sends_flow_mod_then_barrier_at_of15() {
        let dir = tempfile::tempdir().unwrap();
        let seen: Seen = Default::default();
        let path = fake_switch(dir.path(), vec![Script::Normal], seen.clone());
        let mut c = OpenFlowClient::connect_unix(&path).await.unwrap();
        c.add_flow(&entry()).await.unwrap();
        let s = seen.lock().unwrap().clone();
        assert_eq!(
            &s[1..],
            &[
                (MessageType::FlowMod, Version::Of15),
                (MessageType::BarrierRequest, Version::Of15)
            ]
        );
    }

    #[tokio::test]
    async fn refused_flow_mod_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let seen: Seen = Default::default();
        let path = fake_switch(dir.path(), vec![Script::RefuseFlowMod], seen);
        let mut c = OpenFlowClient::connect_unix(&path).await.unwrap();
        let err = c.add_flow(&entry()).await.unwrap_err();
        assert!(format!("{err:#}").contains("refused"), "{err:#}");
    }

    #[tokio::test]
    async fn reconnect_after_the_switch_closes_does_a_fresh_hello() {
        let dir = tempfile::tempdir().unwrap();
        let seen: Seen = Default::default();
        let path = fake_switch(
            dir.path(),
            vec![Script::CloseAfterHello, Script::Normal],
            seen.clone(),
        );
        let mut c = OpenFlowClient::connect_unix(&path).await.unwrap();
        assert!(
            c.echo().await.is_err(),
            "closed connection must fail the echo"
        );
        c.reconnect_now().await.unwrap();
        c.echo().await.unwrap();
        let hellos = seen
            .lock()
            .unwrap()
            .iter()
            .filter(|(t, _)| *t == MessageType::Hello)
            .count();
        assert_eq!(hellos, 2);
        assert!(c.reconnect.is_connected());
    }

    // ---- port description ------------------------------------------------

    /// One OpenFlow 1.4+ `ofp_port`, built by hand (not by the fake switch):
    /// port_no, length, pad2, hw_addr, pad2, name[16], config, state, properties.
    fn hand_port(no: u32, name: &str, mac: [u8; 6], props: &[u8]) -> Vec<u8> {
        let mut e = Vec::new();
        e.extend_from_slice(&no.to_be_bytes());
        e.extend_from_slice(&((40 + props.len()) as u16).to_be_bytes());
        e.extend_from_slice(&[0, 0]);
        e.extend_from_slice(&mac);
        e.extend_from_slice(&[0, 0]);
        let mut n = [0u8; 16];
        n[..name.len()].copy_from_slice(name.as_bytes());
        e.extend_from_slice(&n);
        e.extend_from_slice(&1u32.to_be_bytes()); // config
        e.extend_from_slice(&4u32.to_be_bytes()); // state
        e.extend_from_slice(props);
        e
    }

    const MAC_A: [u8; 6] = [0x02, 0, 0x64, 0x45, 0, 0x0a];
    const MAC_B: [u8; 6] = [0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff];

    #[test]
    fn decodes_hand_built_port_entries_including_local_and_properties() {
        let mut raw = hand_port(1, "eth0", MAC_A, &[]);
        // an ethernet property (type 0, length 8) after the fixed part
        raw.extend(hand_port(
            0xffff_fffe,
            "LOCAL",
            MAC_B,
            &[0, 0, 0, 8, 0, 0, 0, 0],
        ));
        let ports = decode_port_desc(&raw).unwrap();
        assert_eq!(ports.len(), 2);
        assert_eq!(
            (ports[0].name.as_str(), ports[0].port_no, ports[0].hw_addr),
            ("eth0", 1, MAC_A)
        );
        assert_eq!(
            (ports[1].name.as_str(), ports[1].port_no, ports[1].hw_addr),
            ("LOCAL", 0xffff_fffe, MAC_B)
        );
    }

    #[test]
    fn port_decoder_rejects_truncation_and_bad_lengths() {
        let raw = hand_port(1, "eth0", MAC_A, &[]);
        assert!(
            decode_port_desc(&raw[..39]).is_err(),
            "shorter than fixed part"
        );
        let mut short_len = raw.clone();
        short_len[4..6].copy_from_slice(&39u16.to_be_bytes());
        assert!(
            decode_port_desc(&short_len).is_err(),
            "length below the fixed part"
        );
        let mut long_len = raw.clone();
        long_len[4..6].copy_from_slice(&80u16.to_be_bytes());
        assert!(decode_port_desc(&long_len).is_err(), "length past the data");
        let mut dup = raw.clone();
        dup.extend(hand_port(2, "eth0", MAC_B, &[]));
        assert!(decode_port_desc(&dup).is_err(), "duplicate port name");
    }

    #[tokio::test]
    async fn port_desc_follows_reply_more_and_resolves_local() {
        use fake_switch::FakeSwitch;
        let dir = tempfile::tempdir().unwrap();
        let sw = FakeSwitch::start(
            dir.path(),
            "ovsbr0",
            &[
                ("eth0", 1, MAC_A),
                ("tap7", 5, MAC_B),
                ("LOCAL", 0xffff_fffe, MAC_A),
            ],
        );
        let mut c = OpenFlowClient::connect_unix(sw.path()).await.unwrap();
        let ports = c.port_desc().await.unwrap();
        assert_eq!(ports.len(), 3, "three replies, REPLY_MORE followed");
        assert_eq!(ports["eth0"], (1, MAC_A));
        assert_eq!(ports["tap7"], (5, MAC_B));
        assert_eq!(ports["LOCAL"].0, 0xffff_fffe);
    }

    #[tokio::test]
    async fn port_desc_sends_a_port_desc_multipart_at_of15() {
        let dir = tempfile::tempdir().unwrap();
        let seen: Seen = Default::default();
        let path = fake_switch(dir.path(), vec![Script::Normal], seen.clone());
        let mut c = OpenFlowClient::connect_unix(&path).await.unwrap();
        // The scripted switch never answers multiparts; only the request matters.
        assert!(c.port_desc().await.is_err());
        assert!(
            seen.lock()
                .unwrap()
                .contains(&(MessageType::MultipartRequest, Version::Of15))
        );
    }

    // ---- synced flow-mod on a fake switch with a real table ---------------

    fn rovs_add(cookie: u64, in_port: u32, out: u32) -> rovs_openflow::Flow {
        rovs_openflow::Flow::add()
            .priority(10)
            .cookie(cookie)
            .match_fields(FlowMatch::new().in_port(in_port))
            .actions(rovs_openflow::ActionList::new().output(rovs_openflow::OutputPort::Port(out)))
    }

    #[tokio::test]
    async fn flow_mod_changes_the_fake_table_and_dump_reads_it_back() {
        use fake_switch::FakeSwitch;
        let dir = tempfile::tempdir().unwrap();
        let sw = FakeSwitch::start(dir.path(), "ovsbr0", &[]);
        let mut c = OpenFlowClient::connect_unix(sw.path()).await.unwrap();
        assert!(c.dump_flows().await.unwrap().is_empty());

        c.flow_mod(&rovs_add(0x3344_4348_0000_0001, 1, 2))
            .await
            .unwrap();
        let flows = c.dump_flows().await.unwrap();
        assert_eq!(flows.len(), 1);
        assert_eq!(
            (flows[0].cookie, flows[0].priority, flows[0].table_id),
            (0x3344_4348_0000_0001, 10, 0)
        );
        assert_eq!(
            flows[0].match_fields.encode(),
            FlowMatch::new().in_port(1).encode()
        );

        let mut modify = rovs_add(0x3344_4348_0000_0001, 1, 3);
        modify.command = rovs_openflow::FlowCommand::ModifyStrict;
        modify.cookie_mask = u64::MAX;
        c.flow_mod(&modify).await.unwrap();
        let flows = c.dump_flows().await.unwrap();
        assert_eq!(flows.len(), 1);
        assert_eq!(
            flows[0].instructions.encode(),
            rovs_openflow::Instruction::ApplyActions(
                rovs_openflow::ActionList::new().output(rovs_openflow::OutputPort::Port(3))
            )
            .encode()
        );

        let mut delete = rovs_add(0x3344_4348_0000_0001, 1, 3);
        delete.command = rovs_openflow::FlowCommand::DeleteStrict;
        delete.cookie_mask = u64::MAX;
        c.flow_mod(&delete).await.unwrap();
        assert!(c.dump_flows().await.unwrap().is_empty());
        assert_eq!(sw.flow_mods(), 3);
    }

    #[tokio::test]
    async fn strict_delete_with_a_different_cookie_leaves_the_flow() {
        use fake_switch::FakeSwitch;
        let dir = tempfile::tempdir().unwrap();
        let sw = FakeSwitch::start(dir.path(), "ovsbr0", &[]);
        sw.seed(&rovs_add(0x3344_4348_0000_0001, 1, 2));
        let mut c = OpenFlowClient::connect_unix(sw.path()).await.unwrap();
        let mut delete = rovs_add(0x3344_4348_0000_0002, 1, 2);
        delete.command = rovs_openflow::FlowCommand::DeleteStrict;
        delete.cookie_mask = u64::MAX;
        c.flow_mod(&delete).await.unwrap();
        assert_eq!(c.dump_flows().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn flow_mod_refused_by_the_switch_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let seen: Seen = Default::default();
        let path = fake_switch(dir.path(), vec![Script::RefuseFlowMod], seen);
        let mut c = OpenFlowClient::connect_unix(&path).await.unwrap();
        let err = c.flow_mod(&rovs_add(1, 1, 2)).await.unwrap_err();
        assert!(format!("{err:#}").contains("refused"), "{err:#}");
    }
}
