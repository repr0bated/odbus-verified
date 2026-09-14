//! OpenFlow 1.3 controller server (passive mode)
//!
//! Listens for OVS to connect (passive mode), performs the OF1.3 handshake,
//! discovers port numbers by name via a PortDesc multipart request, clears all
//! existing flows, and installs the configured forwarding rules.
//!
//! Wire-protocol encoding is delegated to `rovs_openflow` types wherever
//! possible; the TCP listener and passive handshake are implemented here
//! because `rovs_openflow::VConn` only supports active (outbound) connections.

use anyhow::{Context, Result};
use bytes::Bytes;
use rovs_openflow::{ActionList, Flow, Match, Message, MessageType, OutputPort, Version};
use rovs_transport::Reconnect;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, oneshot};

use crate::openflow_translate::{json_flow_to_add, json_flow_to_delete};

/// A request to install or delete a schema-driven flow on the currently
/// connected switch, submitted via `OpenFlowControllerHandle::send_flow`.
struct FlowRequest {
    flow_json: String,
    delete: bool,
    reply: oneshot::Sender<Result<String>>,
}

// ── OF1.3 constants ────────────────────────────────────────────────────────────

/// Multipart type: port description.
const OFPMP_PORT_DESC: u16 = 13;
/// "All" output port — used when out_port is not restricted.
const OFPP_ANY: u32 = 0xFFFF_FFFF;

// ── Wire helpers ──────────────────────────────────────────────────────────────

/// Build a raw OF1.3 message with an 8-byte header and `body`.
fn build_raw_msg(msg_type: MessageType, xid: u32, body: &[u8]) -> Vec<u8> {
    let msg = Message::new(Version::Of13, msg_type, xid, Bytes::copy_from_slice(body));
    msg.encode().to_vec()
}

/// Build an OF1.3 Hello message.
fn build_hello(xid: u32) -> Vec<u8> {
    build_raw_msg(MessageType::Hello, xid, &[])
}

/// Build an OF1.3 FeaturesRequest message.
fn build_features_request(xid: u32) -> Vec<u8> {
    build_raw_msg(MessageType::FeaturesRequest, xid, &[])
}

/// Build an OF1.3 PortDesc multipart request.
///
/// Body: type(2) + flags(2) + pad(4) = 8 bytes.
fn build_port_desc_request(xid: u32) -> Vec<u8> {
    let mut body = [0u8; 8];
    body[0..2].copy_from_slice(&OFPMP_PORT_DESC.to_be_bytes());
    build_raw_msg(MessageType::MultipartRequest, xid, &body)
}

/// Build an OF1.3 EchoReply that mirrors the request payload.
fn build_echo_reply(xid: u32, payload: &[u8]) -> Vec<u8> {
    build_raw_msg(MessageType::EchoReply, xid, payload)
}

/// Build an OF1.3 FlowMod DELETE ALL (wildcard match, all tables).
fn build_flow_mod_delete_all(xid: u32) -> Vec<u8> {
    let flow = Flow::delete();
    let msg = flow.to_message(Version::Of13, xid);
    msg.encode().to_vec()
}

/// Build an OF1.3 FlowMod ADD: match `in_port`, output to `out_port`.
pub fn build_flow_mod_add(in_port: u32, out_port: u32, priority: u16, xid: u32) -> Vec<u8> {
    let match_fields = Match::new().in_port(in_port);
    let actions = ActionList::new().output(OutputPort::Port(out_port));
    let flow = Flow::add()
        .priority(priority)
        .match_fields(match_fields)
        .actions(actions);
    let msg = flow.to_message(Version::Of13, xid);
    msg.encode().to_vec()
}

// ── Raw message receive ───────────────────────────────────────────────────────

/// A raw inbound OpenFlow message (header parsed, body buffered).
struct RawMsg {
    msg_type: u8,
    xid: u32,
    payload: Vec<u8>,
}

/// Read one complete OpenFlow message from `stream`.
async fn recv_msg(stream: &mut TcpStream) -> Result<RawMsg> {
    let mut hdr = [0u8; 8];
    stream
        .read_exact(&mut hdr)
        .await
        .context("reading OF header")?;

    let msg_type = hdr[1];
    let length = u16::from_be_bytes([hdr[2], hdr[3]]) as usize;
    let xid = u32::from_be_bytes([hdr[4], hdr[5], hdr[6], hdr[7]]);
    let payload_len = length.saturating_sub(8);

    let mut payload = vec![0u8; payload_len];
    if payload_len > 0 {
        stream
            .read_exact(&mut payload)
            .await
            .context("reading OF payload")?;
    }

    Ok(RawMsg {
        msg_type,
        xid,
        payload,
    })
}

/// Write raw bytes to `stream`.
async fn send_msg(stream: &mut TcpStream, bytes: &[u8]) -> Result<()> {
    stream.write_all(bytes).await.context("sending OF message")
}

// ── Port discovery ─────────────────────────────────────────────────────────────

/// Send a PortDesc request and parse all replies into `{port_name → ofport_no}`.
async fn discover_ports(stream: &mut TcpStream, xid: u32) -> Result<HashMap<String, u32>> {
    send_msg(stream, &build_port_desc_request(xid)).await?;

    let mut ports: HashMap<String, u32> = HashMap::new();

    loop {
        let msg = recv_msg(stream).await?;
        match msg.msg_type {
            // Echo request — must reply to stay alive during discovery.
            2 /* EchoRequest */ => {
                send_msg(stream, &build_echo_reply(msg.xid, &msg.payload)).await?;
            }
            // MultipartReply (19)
            19 => {
                if msg.payload.len() < 8 {
                    break;
                }
                let reply_type = u16::from_be_bytes([msg.payload[0], msg.payload[1]]);
                let flags = u16::from_be_bytes([msg.payload[2], msg.payload[3]]);

                if reply_type == OFPMP_PORT_DESC {
                    // OF1.3 ofp_port = 64 bytes:
                    // port_no(4) pad(4) hw_addr(6) pad(2) name(16) config(4) state(4)
                    // curr(4) advertised(4) supported(4) peer(4) curr_speed(4) max_speed(4)
                    let body = &msg.payload[8..];
                    for chunk in body.chunks_exact(64) {
                        let port_no =
                            u32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
                        let name = String::from_utf8_lossy(&chunk[16..32])
                            .trim_end_matches('\0')
                            .to_string();
                        if !name.is_empty() && port_no < OFPP_ANY {
                            ports.insert(name, port_no);
                        }
                    }
                    // bit 0 of flags = OFPMPF_REPLY_MORE
                    if flags & 1 == 0 {
                        break;
                    }
                } else {
                    break;
                }
            }
            _ => break,
        }
    }

    Ok(ports)
}

// ── Connection handler ────────────────────────────────────────────────────────

/// Handle one inbound OVS connection: handshake → port discovery → flow install → keepalive.
async fn handle_connection(
    mut stream: TcpStream,
    flows: Arc<Vec<(String, String, u16)>>,
    active_conn: Arc<Mutex<Option<mpsc::UnboundedSender<FlowRequest>>>>,
) -> Result<()> {
    let mut xid: u32 = 1;

    // 1. Receive Hello from OVS.
    let hello = recv_msg(&mut stream).await?;
    if hello.msg_type != 0 {
        anyhow::bail!("expected Hello (type 0), got msg_type={}", hello.msg_type);
    }

    // 2. Send Hello.
    send_msg(&mut stream, &build_hello(xid)).await?;
    xid += 1;

    // 3. Send FeaturesRequest; wait for FeaturesReply (type 6), echo any pings.
    send_msg(&mut stream, &build_features_request(xid)).await?;
    xid += 1;

    loop {
        let msg = recv_msg(&mut stream).await?;
        match msg.msg_type {
            2 /* EchoRequest */ => {
                send_msg(&mut stream, &build_echo_reply(msg.xid, &msg.payload)).await?;
            }
            6 /* FeaturesReply */ => break,
            _ => {}
        }
    }

    // 4. Discover ports via PortDesc multipart.
    let port_map = discover_ports(&mut stream, xid).await?;
    xid += 1;

    log::info!(
        "OF controller: discovered {} ports: {:?}",
        port_map.len(),
        port_map.keys().collect::<Vec<_>>()
    );

    // 5. Delete all existing flows.
    send_msg(&mut stream, &build_flow_mod_delete_all(xid)).await?;
    xid += 1;

    // 6. Install configured flows.
    let mut installed = 0u32;
    for (in_name, out_name, priority) in flows.iter() {
        match (
            port_map.get(in_name.as_str()),
            port_map.get(out_name.as_str()),
        ) {
            (Some(&in_port), Some(&out_port)) => {
                send_msg(
                    &mut stream,
                    &build_flow_mod_add(in_port, out_port, *priority, xid),
                )
                .await?;
                xid += 1;
                installed += 1;
                log::info!(
                    "OF controller: installed flow {} (port {}) → {} (port {}), priority={}",
                    in_name,
                    in_port,
                    out_name,
                    out_port,
                    priority
                );
            }
            _ => {
                log::warn!(
                    "OF controller: port not found for flow {} → {} (known: {:?})",
                    in_name,
                    out_name,
                    port_map.keys().collect::<Vec<_>>()
                );
            }
        }
    }

    log::info!(
        "OF controller: {} flows installed; entering keepalive loop",
        installed
    );

    // 7. Register this connection's command channel so
    // `OpenFlowControllerHandle::send_flow` can reach the live switch, then
    // keepalive: reply to Echo requests and service schema-driven flow
    // requests for as long as this connection lasts.
    let (cmd_tx, mut cmd_rx) = mpsc::unbounded_channel::<FlowRequest>();
    *active_conn.lock().unwrap() = Some(cmd_tx);

    let result = loop {
        tokio::select! {
            msg = recv_msg(&mut stream) => {
                match msg {
                    Ok(msg) if msg.msg_type == 2 /* EchoRequest */ => {
                        if let Err(e) = send_msg(&mut stream, &build_echo_reply(msg.xid, &msg.payload)).await {
                            break Err(e);
                        }
                    }
                    Ok(_) => {}
                    Err(e) => break Err(e),
                }
            }
            Some(req) = cmd_rx.recv() => {
                let outcome = if req.delete {
                    push_flow_delete(&mut stream, &req.flow_json, &port_map, &mut xid).await
                } else {
                    push_flow_add(&mut stream, &req.flow_json, &port_map, &mut xid).await
                };
                let _ = req.reply.send(outcome);
            }
        }
    };

    *active_conn.lock().unwrap() = None;
    result
}

/// Translate and push one schema-driven flow ADD to the connected switch.
async fn push_flow_add(
    stream: &mut TcpStream,
    flow_json: &str,
    port_map: &HashMap<String, u32>,
    xid: &mut u32,
) -> Result<String> {
    let flow = json_flow_to_add(flow_json, port_map)?;
    let msg = flow.to_message(Version::Of13, *xid);
    *xid += 1;
    send_msg(stream, &msg.encode().to_vec()).await?;
    Ok(serde_json::json!({"ok": true, "action": "add"}).to_string())
}

/// Translate and push one schema-driven flow DELETE to the connected switch.
async fn push_flow_delete(
    stream: &mut TcpStream,
    flow_json: &str,
    port_map: &HashMap<String, u32>,
    xid: &mut u32,
) -> Result<String> {
    let flow = json_flow_to_delete(flow_json, port_map)?;
    let msg = flow.to_message(Version::Of13, *xid);
    *xid += 1;
    send_msg(stream, &msg.encode().to_vec()).await?;
    Ok(serde_json::json!({"ok": true, "action": "delete"}).to_string())
}

// ── Public API ────────────────────────────────────────────────────────────────

/// OpenFlow 1.3 controller — accepts connections from OVS and installs flows.
///
/// OVS connects *to* the controller (not the other way around).
/// Configure OVS with: `ovs-vsctl set-controller ovsbr0 tcp:<listen_addr>`
pub struct OpenFlowController {
    listen_addr: SocketAddr,
    flows: Vec<(String, String, u16)>,
    active_conn: Arc<Mutex<Option<mpsc::UnboundedSender<FlowRequest>>>>,
    installed_flows: Arc<Mutex<Vec<serde_json::Value>>>,
}

impl OpenFlowController {
    /// Create a new controller that will listen on `listen_addr`.
    pub fn new(listen_addr: SocketAddr) -> Self {
        Self {
            listen_addr,
            flows: Vec::new(),
            active_conn: Arc::new(Mutex::new(None)),
            installed_flows: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Get a cloneable handle for pushing schema-driven flows while the
    /// controller runs. Must be called before `run()` consumes `self`.
    pub fn handle(&self) -> OpenFlowControllerHandle {
        OpenFlowControllerHandle {
            active_conn: self.active_conn.clone(),
            installed_flows: self.installed_flows.clone(),
        }
    }

    /// Add a bidirectional forwarding pair (installs two flows: A→B and B→A).
    pub fn add_port_pair(mut self, port_a: &str, port_b: &str, priority: u16) -> Self {
        self.flows
            .push((port_a.to_string(), port_b.to_string(), priority));
        self.flows
            .push((port_b.to_string(), port_a.to_string(), priority));
        self
    }

    /// Add a single directed flow (in_port → out_port).
    pub fn add_flow(mut self, in_port: &str, out_port: &str, priority: u16) -> Self {
        self.flows
            .push((in_port.to_string(), out_port.to_string(), priority));
        self
    }

    /// Run the controller — listens for OVS connections and re-programs flows on each reconnect.
    ///
    /// Each spawned connection handler maintains its own `Reconnect` state machine so
    /// that rapid repeated failures (e.g. OVS flapping) are logged with backoff information
    /// rather than silently spinning.
    pub async fn run(self) -> Result<()> {
        let listener = TcpListener::bind(self.listen_addr)
            .await
            .with_context(|| format!("binding OpenFlow controller on {}", self.listen_addr))?;

        log::info!("OpenFlow controller listening on {}", self.listen_addr);

        let flows = Arc::new(self.flows);
        let active_conn = self.active_conn;

        loop {
            let (stream, peer) = listener.accept().await?;
            let flows = flows.clone();
            let active_conn = active_conn.clone();
            log::info!("OpenFlow controller: OVS connected from {}", peer);

            tokio::spawn(async move {
                // Per-connection reconnection tracker.  OVS is the active side so
                // we don't drive the reconnect loop ourselves — we just record state
                // and log when failures come in rapidly so operators see backoff hints.
                let mut reconnect = Reconnect::new();
                reconnect.set_max_backoff(Duration::from_secs(30));
                reconnect.connecting();

                match handle_connection(stream, flows, active_conn).await {
                    Ok(()) => {
                        // Clean close — mark disconnected so next accept starts fresh.
                        reconnect.disconnected();
                        log::info!("OF controller: connection from {} closed cleanly", peer);
                    }
                    Err(e) => {
                        reconnect.disconnected();
                        reconnect.increase_backoff();
                        log::warn!(
                            "OF controller: connection from {} ended with error \
                             (next OVS reconnect backoff hint: {:?}): {:#}",
                            peer,
                            reconnect.current_backoff(),
                            e
                        );
                    }
                }
            });
        }
    }
}

/// A cloneable handle for pushing schema-driven flows to whichever switch is
/// currently connected to an `OpenFlowController`, obtained via
/// `OpenFlowController::handle()` before calling `run()`.
///
/// This is what backs the `org.opdbus.v1.plugins.openflow` D-Bus interface
/// exposed by the `op-of-controller` binary — the openflow plugin's
/// `install_flow`/`delete_flow`/`query_flows` call through it instead of the
/// old (broken, now-removed) `op-openvswitch-daemon` passthrough.
#[derive(Clone)]
pub struct OpenFlowControllerHandle {
    active_conn: Arc<Mutex<Option<mpsc::UnboundedSender<FlowRequest>>>>,
    installed_flows: Arc<Mutex<Vec<serde_json::Value>>>,
}

impl OpenFlowControllerHandle {
    /// Install or delete one schema-driven `FlowEntry` (JSON, matching the
    /// openflow plugin's shape) on the currently connected switch.
    pub async fn send_flow(&self, flow_json: String, delete: bool) -> Result<String> {
        let value: serde_json::Value =
            serde_json::from_str(&flow_json).context("invalid flow JSON")?;

        let (tx, rx) = oneshot::channel();
        {
            let guard = self.active_conn.lock().unwrap();
            let sender = guard
                .as_ref()
                .context("no OVS switch currently connected to the OpenFlow controller")?;
            sender
                .send(FlowRequest {
                    flow_json,
                    delete,
                    reply: tx,
                })
                .map_err(|_| anyhow::anyhow!("controller connection task is no longer running"))?;
        }
        let result: Result<String> = rx.await.context("controller dropped the reply channel")?;
        let result = result?;

        let mut flows = self.installed_flows.lock().unwrap();
        flows.retain(|f| f != &value);
        if !delete {
            flows.push(value);
        }
        Ok(result)
    }

    /// Return the flows this handle believes are currently installed
    /// (tracked in-memory since they were pushed through `send_flow` —
    /// this is not a live re-query of the switch's flow table).
    pub fn dump_flows(&self) -> Vec<String> {
        self.installed_flows
            .lock()
            .unwrap()
            .iter()
            .map(|v| v.to_string())
            .collect()
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flow_mod_add_length() {
        let msg = build_flow_mod_add(6, 7, 100, 1);
        let declared_len = u16::from_be_bytes([msg[2], msg[3]]) as usize;
        assert_eq!(declared_len, msg.len(), "declared length must match actual");
        // OF1.3 version byte
        assert_eq!(msg[0], 0x04);
        // FlowMod type = 14
        assert_eq!(msg[1], 14);
    }

    #[test]
    fn test_flow_mod_delete_all() {
        let msg = build_flow_mod_delete_all(1);
        let declared_len = u16::from_be_bytes([msg[2], msg[3]]) as usize;
        assert_eq!(declared_len, msg.len(), "declared length must match actual");
        assert_eq!(msg[0], 0x04); // OF1.3
        assert_eq!(msg[1], 14); // FlowMod
    }
}
