//! OpenFlow 1.5 controller server (passive mode)
//!
//! Listens for OVS to connect (passive mode), performs the OpenFlow handshake,
//! enables async delivery (`OFPT_SET_CONFIG` miss_send_len=128), discovers
//! ports, clears flows, immediately re-installs priority=0 NORMAL, then
//! installs configured forwarding rules.
//!
//! Design constraints (OVS design doc):
//! <https://docs.openvswitch.org/en/latest/topics/design/>
//! - Async: service controllers get no PACKET_IN until miss_send_len > 0;
//!   table-miss PACKET_IN uses controller ID 0; never rely on PACKET_IN for
//!   host L3 survival — NORMAL fallback is mandatory after every wipe.
//! - FLOW_MOD: each mod is atomic; delete-all + add-NORMAL is two mods (race).
//!   Minimize the gap; `AttachControllerSafe` pre-seeds NORMAL via ofctl.
//! - In-band principle: essential traffic must forward without the controller.
//! - Echo: reply to OFPT_ECHO_REQUEST; prefer echo for session liveness.
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

use crate::datapath_safe::{FALLBACK_COOKIE, MANAGED_COOKIE};
use crate::openflow_translate::{json_flow_to_add, json_flow_to_delete};
use crate::unixctl::{ensure_static_fdb_entries, static_fdb_from_env, StaticFdbEntry};

#[path = "controller_flow_verify.rs"]
mod flow_verify;

#[cfg(test)]
#[path = "controller_native_tests.rs"]
mod native_tests;

/// A request to install or delete a schema-driven flow on the currently
/// connected switch, submitted via `OpenFlowControllerHandle::send_flow`.
struct FlowRequest {
    flow_json: String,
    delete: bool,
    compare: bool,
    verify_only: bool,
    expected: Option<String>,
    bridge: Option<String>,
    reply: oneshot::Sender<Result<String>>,
}

// ── OpenFlow constants ─────────────────────────────────────────────────────────

/// Wire version used for every message this controller emits.
///
/// OF1.5 is required, not cosmetic: `packet_type` matches and the
/// `encap(ethernet)` action that give the L3-only `netmaker` WireGuard port an
/// Ethernet header exist only from OF1.5 onwards. It is deliberately a single
/// constant so the whole controller can be moved back to `Version::Of13` with
/// one edit if a bridge is pinned to an older protocol set.
const OF_VERSION: Version = Version::Of15;

/// Multipart type: port description.
const OFPMP_PORT_DESC: u16 = 13;
/// "All" output port — used when out_port is not restricted.
const OFPP_ANY: u32 = 0xFFFF_FFFF;
/// Fixed part of `ofp_port` (OF1.4+): port_no(4) length(2) pad(2) hw_addr(6)
/// pad(2) name(16) config(4) state(4). Port properties follow.
const OFP_PORT_HDR_LEN: usize = 40;

// ── Wire helpers ──────────────────────────────────────────────────────────────

/// Build a raw OpenFlow message with an 8-byte header and `body`.
fn build_raw_msg(msg_type: MessageType, xid: u32, body: &[u8]) -> Vec<u8> {
    let msg = Message::new(OF_VERSION, msg_type, xid, Bytes::copy_from_slice(body));
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

/// OF1.3 `OFPT_SET_CONFIG`: flags=0, miss_send_len=`OFP_DEFAULT_MISS_SEND_LEN` (128).
///
/// Required so a **service** controller can receive async messages (PACKET_IN).
/// Primary controllers already get OFPR_NO_MATCH by default, but setting this
/// explicitly documents intent and matches OVS design guidance.
fn build_set_config(xid: u32) -> Vec<u8> {
    // struct ofp_switch_config { uint16_t flags; uint16_t miss_send_len; }
    let mut body = [0u8; 4];
    body[0..2].copy_from_slice(&0u16.to_be_bytes()); // flags
    body[2..4].copy_from_slice(&128u16.to_be_bytes()); // miss_send_len
    build_raw_msg(MessageType::SetConfig, xid, &body)
}

/// Build a PortDesc multipart request.
///
/// Body: `ofp_multipart_request` type(2) + flags(2) + pad(4), followed by the
/// OF1.5-only `ofp15_port_desc_request` port_no(4) + pad(4) = 16 bytes
/// (24-byte message).
///
/// OF1.5 added the `port_no` body that OF1.3 does not have (`openflow-1.5.h`:
/// "All ports if OFPP_ANY"). Leaving it zeroed asks for port 0, which is not a
/// valid port: OVS answers with an empty list and **no error**, so every
/// symbolic port name silently fails to resolve, every configured flow is
/// skipped, and `actions=NORMAL` floods gateway-bound frames to every port.
fn build_port_desc_request(xid: u32) -> Vec<u8> {
    let mut body = [0u8; 16];
    body[0..2].copy_from_slice(&OFPMP_PORT_DESC.to_be_bytes());
    body[8..12].copy_from_slice(&OFPP_ANY.to_be_bytes());
    build_raw_msg(MessageType::MultipartRequest, xid, &body)
}

/// Build a BarrierRequest — forces the switch to finish (and error on) every
/// preceding FlowMod before it replies, so flow acceptance is deterministic
/// instead of "we wrote bytes to a socket".
fn build_barrier_request(xid: u32) -> Vec<u8> {
    build_raw_msg(MessageType::BarrierRequest, xid, &[])
}

/// Build an EchoReply that mirrors the request payload.
fn build_echo_reply(xid: u32, payload: &[u8]) -> Vec<u8> {
    build_raw_msg(MessageType::EchoReply, xid, payload)
}

/// Build an OF1.3 EchoRequest (design: detect hung OF sessions faster than TCP).
fn build_echo_request(xid: u32) -> Vec<u8> {
    build_raw_msg(MessageType::EchoRequest, xid, &[])
}

/// Delete only controller-managed flows (MANAGED_COOKIE), preserving NORMAL fallback.
fn build_flow_mod_delete_managed(xid: u32) -> Vec<u8> {
    let mut flow = Flow::delete();
    flow.cookie = MANAGED_COOKIE;
    flow.cookie_mask = u64::MAX; // match this cookie exactly
    let msg = flow.to_message(OF_VERSION, xid);
    msg.encode().to_vec()
}

/// Legacy wipe of all tables (avoid on live host bridges).
#[allow(dead_code)] // kept for unit-test coverage of wire encoding
fn build_flow_mod_delete_all(xid: u32) -> Vec<u8> {
    let flow = Flow::delete();
    let msg = flow.to_message(OF_VERSION, xid);
    msg.encode().to_vec()
}

/// Build an OF1.3 FlowMod ADD: match `in_port`, output to `out_port`.
pub fn build_flow_mod_add(in_port: u32, out_port: u32, priority: u16, xid: u32) -> Vec<u8> {
    let match_fields = Match::new().in_port(in_port);
    let actions = ActionList::new().output(OutputPort::Port(out_port));
    let flow = Flow::add()
        .priority(priority)
        .cookie(MANAGED_COOKIE)
        .match_fields(match_fields)
        .actions(actions);
    let msg = flow.to_message(OF_VERSION, xid);
    msg.encode().to_vec()
}

/// Build an OF1.3 FlowMod ADD: table-miss style fallback `actions=NORMAL`.
///
/// Must be re-installed after any delete-all so unmatched host traffic does not
/// depend solely on async packet_in → controller flow install.
pub fn build_flow_mod_normal(priority: u16, xid: u32) -> Vec<u8> {
    let match_fields = Match::new();
    let actions = ActionList::new().normal();
    let mut flow = Flow::add()
        .priority(priority)
        .cookie(FALLBACK_COOKIE)
        .match_fields(match_fields)
        .actions(actions);
    // Host-safety fallback: skip per-flow counters (OF1.3 OFPFF_NO_*).
    flow.flags.no_pkt_counts = true;
    flow.flags.no_byte_counts = true;
    let msg = flow.to_message(OF_VERSION, xid);
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
    let mut first = [0];
    stream
        .read_exact(&mut first)
        .await
        .context("reading OF header")?;
    recv_msg_after_first(stream, first[0]).await
}

async fn recv_msg_after_first(stream: &mut TcpStream, first: u8) -> Result<RawMsg> {
    let mut hdr = [0u8; 8];
    hdr[0] = first;
    stream
        .read_exact(&mut hdr[1..])
        .await
        .context("reading OF header")?;

    let msg_type = hdr[1];
    let length = u16::from_be_bytes([hdr[2], hdr[3]]) as usize;
    anyhow::ensure!(
        hdr[0] == OF_VERSION as u8 && length >= 8,
        "invalid OpenFlow frame header"
    );
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

/// Send a PortDesc request and parse all replies into `{port_name → ofport_no}`
/// plus `{port_name → hw_addr}`.
///
/// The MAC map is what lets a static flow say `"eth_dst": "port_mac:ovsbr0"`
/// instead of hard-coding the bridge's MAC in config — it comes free with the
/// reply we already have to parse, with no OVSDB round trip.
async fn discover_ports(
    stream: &mut TcpStream,
    xid: u32,
) -> Result<(HashMap<String, u32>, HashMap<String, [u8; 6]>)> {
    send_msg(stream, &build_port_desc_request(xid)).await?;

    let mut ports: HashMap<String, u32> = HashMap::new();
    let mut port_macs: HashMap<String, [u8; 6]> = HashMap::new();

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
                    // From OF1.4 `ofp_port` is variable length — a 40-byte
                    // fixed part followed by port properties — so stride on the
                    // entry's own `length` field instead of the OF1.3 64-byte
                    // stride, which would mis-slice every port after the first.
                    // port_no(4) length(2) pad(2) hw_addr(6) pad(2) name(16)
                    // config(4) state(4) properties...
                    let body = &msg.payload[8..];
                    let mut off = 0usize;
                    while off + OFP_PORT_HDR_LEN <= body.len() {
                        let entry = &body[off..];
                        let port_no =
                            u32::from_be_bytes([entry[0], entry[1], entry[2], entry[3]]);
                        let entry_len = u16::from_be_bytes([entry[4], entry[5]]) as usize;
                        let name = String::from_utf8_lossy(&entry[16..32])
                            .trim_end_matches('\0')
                            .to_string();
                        if !name.is_empty() && port_no < OFPP_ANY {
                            let mut hw_addr = [0u8; 6];
                            hw_addr.copy_from_slice(&entry[8..14]);
                            ports.insert(name.clone(), port_no);
                            port_macs.insert(name, hw_addr);
                        }
                        // A length that cannot advance us would spin forever.
                        if entry_len < OFP_PORT_HDR_LEN {
                            log::warn!(
                                "OF controller: malformed ofp_port length {entry_len} at offset {off}, \
                                 stopping port parse"
                            );
                            break;
                        }
                        off += entry_len;
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

    Ok((ports, port_macs))
}

// ── Connection handler ────────────────────────────────────────────────────────

/// Handle one inbound OVS connection: handshake → port discovery → flow install → keepalive.
async fn handle_connection(
    mut stream: TcpStream,
    flows: Arc<Vec<(String, String, u16)>>,
    static_flows: Arc<Vec<String>>,
    static_fdb: Arc<Vec<StaticFdbEntry>>,
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

    // 3b. Enable async delivery (service-controller miss_send_len gate).
    send_msg(&mut stream, &build_set_config(xid)).await?;
    xid += 1;
    log::info!("OF controller: SET_CONFIG miss_send_len=128 (async PACKET_IN enabled)");

    // 4. Discover ports via PortDesc multipart.
    let (port_map, port_macs) = discover_ports(&mut stream, xid).await?;
    xid += 1;

    log::info!(
        "OF controller: discovered {} ports: {:?}",
        port_map.len(),
        port_map.keys().collect::<Vec<_>>()
    );

    // 5. Install cookied NORMAL FIRST (in-band principle), then delete only
    // MANAGED_COOKIE flows — never delete-all (avoids empty-table race).
    send_msg(&mut stream, &build_flow_mod_normal(0, xid)).await?;
    xid += 1;
    log::info!("OF controller: ensured cookie={FALLBACK_COOKIE:#x} priority=0 NORMAL");

    send_msg(&mut stream, &build_flow_mod_delete_managed(xid)).await?;
    xid += 1;

    // Re-assert NORMAL after managed delete (idempotent ADD).
    send_msg(&mut stream, &build_flow_mod_normal(0, xid)).await?;
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

    // 5b. Re-assert static FDB pins BEFORE any symbolic-port work. These live
    // in ovs-vswitchd's L2 table, not in OVSDB or the flow table, so a vswitchd
    // restart silently drops them — and the ISP filters VRRP off customer
    // ports, so the gateway's virtual MAC can never be learned. Without the pin
    // `actions=NORMAL` floods every gateway-bound frame to every port. L2
    // reachability must never be gated on a static flow resolving a port name,
    // which is why this runs ahead of the loop below and its barrier.
    if !static_fdb.is_empty() {
        let added = ensure_static_fdb_entries(&static_fdb).await;
        log::info!(
            "OF controller: {} static FDB pin(s) configured, {} (re)added",
            static_fdb.len(),
            added
        );
    }

    // 6b. Install durable static flows (rich match) — reinstalled every
    // reconnect, after delete_managed so the fallback/managed set is clean.
    //
    // A flow that cannot be translated (typically an `in_port` naming a port
    // that is not currently attached to the bridge — `netmaker` while
    // netmaker-ovs-attach is broken) is skipped, not fatal: the plugin declares
    // `atomic_operations: false`, so one unresolvable flow must not tear down
    // the connection and take every other flow, the NORMAL fallback and the FDB
    // pins with it.
    let mut static_installed = 0u32;
    let mut static_skipped = 0u32;
    for flow_json in static_flows.iter() {
        match push_flow_add(&mut stream, flow_json, &port_map, &port_macs, &mut xid).await {
            Ok(_) => static_installed += 1,
            Err(e) => {
                static_skipped += 1;
                log::warn!("OF controller: skipping static flow ({e:#}): {flow_json}");
            }
        }
    }
    if static_installed > 0 || static_skipped > 0 {
        log::info!(
            "OF controller: {static_installed} static flow(s) installed, {static_skipped} skipped"
        );
    }

    // 6c. A barrier makes static-flow acceptance deterministic: the switch must
    // process every preceding FlowMod (and report any it rejected) before it
    // replies. Errors are reported, never fatal — one rejected FlowMod must not
    // cost us the fallback, the pins and every other flow. Three exits so a
    // switch that never sends BarrierReply cannot hang the session: the reply,
    // an error carrying our own barrier xid, and a hard timeout.
    let barrier_xid = xid;
    send_msg(&mut stream, &build_barrier_request(barrier_xid)).await?;
    xid += 1;
    loop {
        let msg = match tokio::time::timeout(Duration::from_secs(10), recv_msg(&mut stream)).await {
            Ok(msg) => msg?,
            Err(_) => {
                log::warn!(
                    "OF controller: no BarrierReply within 10s (xid={barrier_xid}); \
                     continuing to keepalive"
                );
                break;
            }
        };
        match msg.msg_type {
            21 /* BarrierReply */ => break,
            1 /* Error */ => {
                let error_type = msg
                    .payload
                    .get(0..2)
                    .map(|v| u16::from_be_bytes([v[0], v[1]]));
                let error_code = msg
                    .payload
                    .get(2..4)
                    .map(|v| u16::from_be_bytes([v[0], v[1]]));
                log::warn!(
                    "OF controller: switch rejected OpenFlow programming: \
                     type={error_type:?} code={error_code:?} xid={}",
                    msg.xid
                );
                // If the barrier itself errored there will be no reply at all.
                if msg.xid == barrier_xid {
                    break;
                }
            }
            2 /* EchoRequest */ => {
                send_msg(&mut stream, &build_echo_reply(msg.xid, &msg.payload)).await?;
            }
            _ => {}
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

    let mut echo_tick = tokio::time::interval(Duration::from_secs(5));
    echo_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let result = loop {
        tokio::select! {
            _ = echo_tick.tick() => {
                // Proactive echo (design: do not rely on kernel TCP RTO alone).
                if let Err(e) = send_msg(&mut stream, &build_echo_request(xid)).await {
                    break Err(e);
                }
                xid = xid.wrapping_add(1);
            }
            ready = stream.readable() => {
                if let Err(error) = ready { break Err(error.into()); }
                // Readiness can be stale after a prior successful read. Clear
                // it with try_read before committing to a whole-frame wait.
                let mut first = [0];
                match stream.try_read(&mut first) {
                    Ok(0) => break Err(anyhow::anyhow!("OpenFlow peer closed")),
                    Ok(_) => {},
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => continue,
                    Err(error) => break Err(error.into()),
                }
                // Readiness is cancellation safe. Never cancel recv_msg after
                // it has consumed part of a frame to handle another command.
                let msg = match tokio::time::timeout(Duration::from_secs(10), recv_msg_after_first(&mut stream, first[0])).await {
                    Ok(message) => message,
                    Err(error) => break Err(error.into()),
                };
                match msg {
                    Ok(msg) if msg.msg_type == 2 /* EchoRequest */ => {
                        if let Err(e) = send_msg(&mut stream, &build_echo_reply(msg.xid, &msg.payload)).await {
                            break Err(e);
                        }
                    }
                    Ok(msg) if msg.msg_type == 3 /* EchoReply */ => {
                        log::trace!("OF controller: echo reply xid={}", msg.xid);
                    }
                    Ok(msg) if msg.msg_type == 10 /* PacketIn */ => {
                        // Reactive path optional; host L3 must already hit NORMAL.
                        log::debug!(
                            "OF controller: PACKET_IN xid={} len={} (not required for host L3)",
                            msg.xid,
                            msg.payload.len()
                        );
                    }
                    Ok(_) => {}
                    Err(e) => break Err(e),
                }
            }
            Some(req) = cmd_rx.recv() => {
                let outcome = if req.verify_only {
                    verify_owned_flow(&mut stream, &req, &port_map, &port_macs, &mut xid).await
                } else if req.compare {
                    compare_exchange_flow(&mut stream, &req, &port_map, &port_macs, &mut xid).await
                } else if req.delete {
                    push_flow_delete(&mut stream, &req.flow_json, &port_map, &port_macs, &mut xid).await
                } else {
                    push_flow_add(&mut stream, &req.flow_json, &port_map, &port_macs, &mut xid).await
                };
                let disconnected = outcome.as_ref().err().is_some_and(|error| {
                    error.downcast_ref::<std::io::Error>().is_some()
                        || error.downcast_ref::<tokio::time::error::Elapsed>().is_some()
                });
                let _ = req.reply.send(outcome);
                // A timed-out partial frame cannot be reused safely.
                if disconnected { break Err(anyhow::anyhow!("OpenFlow command transport lost framing; reconnect required")); }
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
    port_macs: &HashMap<String, [u8; 6]>,
    xid: &mut u32,
) -> Result<String> {
    let flow = json_flow_to_add(flow_json, port_map, port_macs)?;
    flow_verify::apply(stream, &flow, xid).await?;
    let entries = flow_verify::query(stream, &flow, xid).await?;
    flow_verify::verify(&entries, Some(&flow), &flow)?;
    Ok(serde_json::json!({"ok": true, "action": "add", "verified": true}).to_string())
}

/// Translate and push one schema-driven flow DELETE to the connected switch.
async fn push_flow_delete(
    stream: &mut TcpStream,
    flow_json: &str,
    port_map: &HashMap<String, u32>,
    port_macs: &HashMap<String, [u8; 6]>,
    xid: &mut u32,
) -> Result<String> {
    let flow = json_flow_to_delete(flow_json, port_map, port_macs)?;
    flow_verify::apply(stream, &flow, xid).await?;
    let entries = flow_verify::query(stream, &flow, xid).await?;
    flow_verify::verify(&entries, None, &flow)?;
    Ok(serde_json::json!({"ok": true, "action": "delete", "verified": true}).to_string())
}

async fn compare_exchange_flow(
    stream: &mut TcpStream,
    request: &FlowRequest,
    ports: &HashMap<String, u32>,
    macs: &HashMap<String, [u8; 6]>,
    xid: &mut u32,
) -> Result<String> {
    let bridge = request
        .bridge
        .as_deref()
        .context("managed flow requires bridge")?;
    anyhow::ensure!(
        ports.get(bridge) == Some(&0xffff_fffe),
        "requested bridge does not match the connected switch LOCAL port"
    );
    let desired = json_flow_to_add(&request.flow_json, ports, macs)?;
    let expected = request
        .expected
        .as_deref()
        .map(|json| json_flow_to_add(json, ports, macs))
        .transpose()?;
    if let Some(expected) = &expected {
        anyhow::ensure!(
            expected.table_id == desired.table_id
                && expected.priority == desired.priority
                && expected.match_fields.encode() == desired.match_fields.encode(),
            "flow compare/exchange cannot change entry identity"
        );
    }
    let entries = flow_verify::query(stream, &desired, xid).await?;
    flow_verify::verify(&entries, expected.as_ref(), &desired)?;
    if request.delete {
        push_flow_delete(stream, &request.flow_json, ports, macs, xid).await?;
    } else {
        push_flow_add(stream, &request.flow_json, ports, macs, xid).await?;
    }
    Ok(serde_json::json!({"verified":true,"action":if request.delete {"delete"} else {"add"},"flow":serde_json::from_str::<serde_json::Value>(&request.flow_json)?}).to_string())
}

async fn verify_owned_flow(
    stream: &mut TcpStream,
    request: &FlowRequest,
    ports: &HashMap<String, u32>,
    macs: &HashMap<String, [u8; 6]>,
    xid: &mut u32,
) -> Result<String> {
    let bridge = request
        .bridge
        .as_deref()
        .context("flow verification requires bridge")?;
    anyhow::ensure!(
        ports.get(bridge) == Some(&0xffff_fffe),
        "requested bridge does not match the connected switch LOCAL port"
    );
    let flow = json_flow_to_add(&request.flow_json, ports, macs)?;
    let entries = flow_verify::query(stream, &flow, xid).await?;
    let matches = flow_verify::verify(
        &entries,
        if request.delete { None } else { Some(&flow) },
        &flow,
    )
    .is_ok();
    Ok(serde_json::json!({"matches":matches}).to_string())
}

// ── Public API ────────────────────────────────────────────────────────────────

/// OpenFlow 1.3 controller — accepts connections from OVS and installs flows.
///
/// OVS connects *to* the controller (not the other way around).
/// Configure OVS with: `attach-controller-safe ovsbr0 tcp:<listen_addr>`
/// (or D-Bus `AttachControllerSafe`) — never bare `ovs-vsctl set-controller`.
pub struct OpenFlowController {
    listen_addr: SocketAddr,
    flows: Vec<(String, String, u16)>,
    /// Durable schema-driven flows (JSON FlowEntry), reinstalled on every
    /// OVS reconnect via the same translator as SendFlow.
    static_flows: Vec<String>,
    /// Static FDB pins (`bridge:port:vlan:mac`) re-asserted on every reconnect
    /// via unixctl, sourced from `OF_STATIC_FDB`.
    static_fdb: Vec<StaticFdbEntry>,
    active_conn: Arc<Mutex<Option<mpsc::UnboundedSender<FlowRequest>>>>,
    installed_flows: Arc<Mutex<Vec<serde_json::Value>>>,
}

impl OpenFlowController {
    /// Create a new controller that will listen on `listen_addr`.
    pub fn new(listen_addr: SocketAddr) -> Self {
        Self {
            listen_addr,
            flows: Vec::new(),
            static_flows: Vec::new(),
            static_fdb: static_fdb_from_env().unwrap_or_else(|e| {
                log::warn!("OF controller: OF_STATIC_FDB parse failed, no pins: {e:#}");
                Vec::new()
            }),
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

    /// Add a durable static flow (JSON ) reinstalled on every OVS
    /// reconnect. Unlike , this carries the full match/action
    /// set (e.g. /) via the same translator as .
    pub fn add_static_flow(mut self, flow_json: &str) -> Self {
        self.static_flows.push(flow_json.to_string());
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
        let static_flows = Arc::new(self.static_flows);
        let static_fdb = Arc::new(self.static_fdb);
        let active_conn = self.active_conn;

        loop {
            let (stream, peer) = listener.accept().await?;
            let flows = flows.clone();
            let static_flows = static_flows.clone();
            let static_fdb = static_fdb.clone();
            let active_conn = active_conn.clone();
            log::info!("OpenFlow controller: OVS connected from {}", peer);

            tokio::spawn(async move {
                // Per-connection reconnection tracker.  OVS is the active side so
                // we don't drive the reconnect loop ourselves — we just record state
                // and log when failures come in rapidly so operators see backoff hints.
                let mut reconnect = Reconnect::new();
                reconnect.set_max_backoff(Duration::from_secs(30));
                reconnect.connecting();

                match handle_connection(stream, flows, static_flows, static_fdb, active_conn).await
                {
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
                    compare: false,
                    verify_only: false,
                    expected: None,
                    bridge: None,
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

    /// Serialized native compare/read/modify/barrier/readback, restricted to
    /// the named connected bridge. No caller can wildcard-delete other flows.
    pub async fn compare_exchange_flow(
        &self,
        bridge: String,
        flow_json: String,
        expected: Option<String>,
        delete: bool,
    ) -> Result<String> {
        anyhow::ensure!(
            !delete || expected.as_ref() == Some(&flow_json),
            "delete requires the exact expected flow"
        );
        let (tx, rx) = oneshot::channel();
        self.active_conn
            .lock()
            .unwrap()
            .as_ref()
            .context("no connected OpenFlow switch")?
            .send(FlowRequest {
                flow_json,
                delete,
                compare: true,
                verify_only: false,
                expected,
                bridge: Some(bridge),
                reply: tx,
            })
            .map_err(|_| anyhow::anyhow!("OpenFlow owner disconnected"))?;
        rx.await
            .context("OpenFlow compare/exchange owner dropped result; reconcile outcome")?
    }

    /// Native readback for recovery of a previously attempted exact flow.
    /// This does not mutate the switch or consult the installed-flow cache.
    pub async fn verify_flow(
        &self,
        bridge: String,
        flow_json: String,
        absent: bool,
    ) -> Result<bool> {
        let (tx, rx) = oneshot::channel();
        self.active_conn
            .lock()
            .unwrap()
            .as_ref()
            .context("no connected OpenFlow switch")?
            .send(FlowRequest {
                flow_json,
                delete: absent,
                compare: false,
                verify_only: true,
                expected: None,
                bridge: Some(bridge),
                reply: tx,
            })
            .map_err(|_| anyhow::anyhow!("OpenFlow owner disconnected"))?;
        let value: serde_json::Value =
            serde_json::from_str(&rx.await.context("flow verification owner disconnected")??)?;
        value
            .get("matches")
            .and_then(serde_json::Value::as_bool)
            .context("invalid verification result")
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
        // OpenFlow version byte (OF1.5)
        assert_eq!(msg[0], 0x06);
        // FlowMod type = 14
        assert_eq!(msg[1], 14);
    }

    /// OF1.5 PORT_DESC carries a request body; `port_no` must be `OFPP_ANY` to
    /// mean "all ports". A zeroed body asks for port 0 and OVS answers with an
    /// empty list and no error, which silently breaks all name resolution and
    /// leaves every frame on `actions=NORMAL` flooding.
    #[test]
    fn test_port_desc_request_asks_for_all_ports() {
        let msg = build_port_desc_request(4);
        assert_eq!(msg.len(), 24, "OF1.5 PORT_DESC request is 8 + 16 bytes");
        let declared_len = u16::from_be_bytes([msg[2], msg[3]]) as usize;
        assert_eq!(declared_len, msg.len(), "declared length must match actual");
        assert_eq!(msg[0], 0x06); // OpenFlow 1.5
        assert_eq!(msg[1], 18); // MultipartRequest

        // ofp_multipart_request: type(2) + flags(2) + pad(4)
        let body = &msg[8..];
        assert_eq!(u16::from_be_bytes([body[0], body[1]]), OFPMP_PORT_DESC);
        assert_eq!(u16::from_be_bytes([body[2], body[3]]), 0, "flags");

        // ofp15_port_desc_request: port_no(4) + pad(4)
        let port_no = u32::from_be_bytes([body[8], body[9], body[10], body[11]]);
        assert_eq!(
            port_no, OFPP_ANY,
            "port_no must be OFPP_ANY, not 0 (port 0 returns an empty reply)"
        );
    }

    #[test]
    fn test_flow_mod_delete_all() {
        let msg = build_flow_mod_delete_all(1);
        let declared_len = u16::from_be_bytes([msg[2], msg[3]]) as usize;
        assert_eq!(declared_len, msg.len(), "declared length must match actual");
        assert_eq!(msg[0], 0x06); // OF1.5
        assert_eq!(msg[1], 14); // FlowMod
    }
}
