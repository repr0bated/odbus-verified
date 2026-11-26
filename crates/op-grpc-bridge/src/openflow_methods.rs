//! The `openflow` plugin's flow members, written to OVS by this process.
//!
//! The bridge owns one OpenFlow 1.5 connection per managed bridge, over
//! OVS's own socket `<run dir>/<bridge>.mgmt` (rovs). Each `add_flow`,
//! `modify_flow` and `delete_flow` is one admitted compare-and-exchange under
//! that bridge's lock: read ports and flows, compare with the request, run the
//! call's final admission, send one strict FLOW_MOD plus barrier, read back and
//! verify. There is no controller process, no D-Bus hop and no permit.
//! A write is never retried: an unverified outcome is reported as unknown.

use crate::admission::FinalAdmission;
use crate::core::{BackendProjection, BackendStateUpdate, StateScope};
use anyhow::{Context, Result, anyhow, bail, ensure};
use op_network::openflow::{OpenFlowClient, SwitchFlow, rovs_openflow};
use op_network::openflow_cookie::{FlowIdentity, is_plugin_cookie};
use op_network::openflow_translate::{json_flow_to_add, json_flow_to_delete, json_flow_to_modify};
use op_plugins::state_plugins::openflow::{
    AddFlowInput, DeleteFlowInput, FlowAction, FlowChild, FlowEntry, FlowMutationKind,
    FlowMutationOutput, ModifyFlowInput,
};
use rovs_openflow::{
    Action, ActionList, Flow, FlowCommand, Instruction, InstructionList, Match, OutputPort,
    Version, nxm,
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;

const MAX_FLOW_JSON: usize = 64 * 1024;
/// Where OVS puts each bridge's `<bridge>.mgmt` socket.
pub(crate) const OVS_RUN_DIR: &str = "/var/run/openvswitch";
/// Whole-call ceiling; each switch reply has its own shorter timeout.
const CALL_TIMEOUT: Duration = Duration::from_secs(45);
/// Upper bound on distinct bridges this owner keeps a connection slot for.
const MAX_BRIDGES: usize = 64;

#[derive(Debug, Clone)]
pub struct OpenFlowMethodReceipt {
    pub output: Value,
    /// Exact member known to be live immediately before the operation.
    pub before: Option<FlowEntry>,
    /// Exact member confirmed live after the operation; absent for delete.
    pub after: Option<FlowEntry>,
    pub bridge: String,
    /// The bridge's flow children rebuilt from the read-back after the write:
    /// every plugin-cookie flow on the switch, keyed by hex cookie. Commit it
    /// with [`flow_children_projection`].
    pub children: BTreeMap<String, FlowChild>,
}

/// Merge only the native observation member. Authored bridge configuration and
/// observations for other bridges remain owned by the canonical state tree.
///
/// The flow children are not in this patch: a merge can add or update a
/// child but never remove one, so a flow deleted on the switch would stay.
/// They are committed by [`flow_children_projection`], which replaces the
/// whole `flows/<bridge>` subtree.
pub(crate) fn projection_patch(receipt: &OpenFlowMethodReceipt) -> Result<Value> {
    let observed = receipt
        .after
        .as_ref()
        .or(receipt.before.as_ref())
        .context("OpenFlow receipt omitted both member revisions")?;
    Ok(serde_json::json!({
        "observed_flows": {
            (receipt.bridge.as_str()): {
                (canonical_flow_identity(observed)): {
                    "flow": observed,
                    "installed": receipt.after.is_some()
                }
            }
        }
    }))
}

/// The update that commits one bridge's flow children: `Replace` of the
/// subtree `flows/<bridge>` of the plugin's committed state. A flow gone from
/// the switch is gone from the state; other bridges' children and every other
/// field stay as they are. The children come from a read of the switch
/// (`receipt.children` after a write, [`SwitchView::children`] on a read), so
/// nothing is kept between reads and a reconnect rebuilds them.
pub(crate) fn flow_children_projection(
    bridge: &str,
    children: &BTreeMap<String, FlowChild>,
    schema_hash: &str,
) -> Result<BackendProjection> {
    validate_bridge(bridge)?;
    Ok(BackendProjection {
        plugin_id: "openflow".to_owned(),
        object_path: format!(
            "{}/flows/{bridge}",
            op_plugins::canonical::plugin_path("openflow")
        ),
        schema_hash: schema_hash.to_owned(),
        scope: StateScope::Public,
        state: BackendStateUpdate::Replace(serde_json::to_value(children)?),
    })
}

/// The flow children of `bridge`, rebuilt from one read of its switch: every
/// flow carrying the plugin's cookie prefix, keyed by its cookie in hex.
/// Foreign flows are not children (requirement 2.8). Counters are those of
/// this read; OVS has no flow monitor on `.mgmt`, so they refresh on each read.
fn flow_children(
    bridge: &str,
    ports: &HashMap<String, (u32, [u8; 6])>,
    flows: &[SwitchFlow],
) -> Result<BTreeMap<String, FlowChild>> {
    let port_map: HashMap<String, u32> = ports
        .iter()
        .map(|(name, (no, _))| (name.clone(), *no))
        .collect();
    let port_macs: HashMap<String, [u8; 6]> = ports
        .iter()
        .map(|(name, (_, mac))| (name.clone(), *mac))
        .collect();
    let mut sorted: Vec<(&String, u32)> = port_map.iter().map(|(n, no)| (n, *no)).collect();
    sorted.sort_unstable();
    let mut names: HashMap<u32, String> = HashMap::new();
    for (name, no) in sorted {
        names.entry(no).or_insert_with(|| name.clone());
    }
    let mut children = BTreeMap::new();
    for flow in flows.iter().filter(|flow| is_plugin_cookie(flow.cookie)) {
        let child = flow_child(bridge, flow, &names, &port_map, &port_macs)
            .with_context(|| format!("flow {:#018x} on {bridge}", flow.cookie))?;
        let cookie = child.cookie.clone();
        ensure!(
            children.insert(cookie.clone(), child).is_none(),
            "flow {cookie} appears more than once on {bridge}; one child cannot name two flows"
        );
    }
    Ok(children)
}

/// One switch flow in the plugin's own vocabulary. The child must name
/// exactly the live flow: translated forward again it gives the same
/// canonical revision, so it can be passed back as `expected` to
/// `modify_flow` or `delete_flow`.
fn flow_child(
    bridge: &str,
    flow: &SwitchFlow,
    names: &HashMap<u32, String>,
    port_map: &HashMap<String, u32>,
    port_macs: &HashMap<String, [u8; 6]>,
) -> Result<FlowChild> {
    let match_fields = match_to_fields(&flow.match_fields, names)?;
    let actions = instructions_to_actions(&flow.instructions, names)?;
    let entry = FlowEntry {
        table: flow.table_id,
        priority: flow.priority,
        match_fields: match_fields.clone(),
        actions: actions.clone(),
        cookie: Some(flow.cookie),
        idle_timeout: flow.idle_timeout,
        hard_timeout: flow.hard_timeout,
    };
    let forward = json_flow_to_add(&serde_json::to_string(&entry)?, port_map, port_macs)
        .context("the plugin's spelling of the live flow does not translate back")?;
    ensure!(
        canon_flow(&forward)? == canon_switch(flow)?,
        "the live flow uses fields or actions the plugin's FlowEntry cannot spell"
    );
    Ok(FlowChild {
        bridge: bridge.to_owned(),
        table: flow.table_id,
        priority: flow.priority,
        cookie: format!("{:#018x}", flow.cookie),
        match_fields,
        actions,
        flags: flow_flag_names(flow.flags),
        idle_timeout: flow.idle_timeout,
        hard_timeout: flow.hard_timeout,
        packet_count: flow.packet_count,
        byte_count: flow.byte_count,
        duration_sec: flow.duration_sec,
    })
}

/// OpenFlow `ofp_flow_mod_flags`, spelled as OVS spells them.
fn flow_flag_names(flags: u16) -> Vec<String> {
    const NAMES: [(u16, &str); 5] = [
        (1 << 0, "send_flow_rem"),
        (1 << 1, "check_overlap"),
        (1 << 2, "reset_counts"),
        (1 << 3, "no_packet_counts"),
        (1 << 4, "no_byte_counts"),
    ];
    let mut names: Vec<String> = NAMES
        .iter()
        .filter(|(bit, _)| flags & bit != 0)
        .map(|(_, name)| (*name).to_owned())
        .collect();
    let known = NAMES.iter().fold(0u16, |all, (bit, _)| all | bit);
    for bit in (0..16).map(|shift| 1u16 << shift) {
        if flags & bit != 0 && known & bit == 0 {
            names.push(format!("{bit:#06x}"));
        }
    }
    names
}

/// A port number as the plugin writes it: `LOCAL`, the switch's port name,
/// or the bare number when the switch reports no name for it.
fn port_token(port: u32, names: &HashMap<u32, String>) -> String {
    if port == 0xffff_fffe {
        return "LOCAL".to_owned();
    }
    names
        .get(&port)
        .cloned()
        .unwrap_or_else(|| port.to_string())
}

fn mac_text(mac: &[u8; 6]) -> String {
    mac.iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .join(":")
}

fn cidr_text(addr: std::net::Ipv4Addr, prefix: Option<u8>) -> String {
    match prefix {
        None | Some(32) => addr.to_string(),
        Some(prefix) => format!("{addr}/{prefix}"),
    }
}

/// The reverse of `openflow_translate`'s match parsing: a rovs `Match` as the
/// plugin's `match_fields`. A field the plugin cannot spell is an error, never
/// dropped, so a child is never looser than the flow it names.
fn match_to_fields(m: &Match, names: &HashMap<u32, String>) -> Result<HashMap<String, String>> {
    let unspellable = [
        ("in_phy_port", m.in_phy_port.is_some()),
        (
            "metadata",
            m.metadata.is_some() || m.metadata_mask.is_some(),
        ),
        ("eth_src mask", m.eth_src_mask.is_some()),
        ("eth_dst", m.eth_dst.is_some() || m.eth_dst_mask.is_some()),
        ("vlan_pcp", m.vlan_pcp.is_some()),
        ("ip_dscp", m.ip_dscp.is_some()),
        ("ip_ecn", m.ip_ecn.is_some()),
        (
            "ipv6",
            m.ipv6_src.is_some() || m.ipv6_dst.is_some() || m.ipv6_flabel.is_some(),
        ),
        (
            "icmp type/code",
            m.icmp_type.is_some() || m.icmp_code.is_some(),
        ),
        (
            "icmpv6 type/code",
            m.icmpv6_type.is_some() || m.icmpv6_code.is_some(),
        ),
        (
            "arp fields",
            m.arp_op.is_some()
                || m.arp_spa.is_some()
                || m.arp_tpa.is_some()
                || m.arp_sha.is_some()
                || m.arp_tha.is_some(),
        ),
        ("tunnel_id", m.tunnel_id.is_some()),
        ("ct_zone", m.ct_zone.is_some()),
        ("ct_mark", m.ct_mark.is_some() || m.ct_mark_mask.is_some()),
    ];
    if let Some((field, _)) = unspellable.iter().find(|(_, set)| *set) {
        bail!("match field {field} has no match_fields spelling");
    }
    let mut fields = HashMap::new();
    let mut put = |key: &str, value: String| {
        fields.insert(key.to_owned(), value);
    };
    if let Some(port) = m.in_port {
        put("in_port", port_token(port, names));
    }
    match (m.eth_type, m.ip_proto) {
        (None, None) => {}
        (Some(0x0800), None) => put("ip", String::new()),
        (Some(0x0800), Some(6)) => put("tcp", String::new()),
        (Some(0x0800), Some(17)) => put("udp", String::new()),
        (Some(0x0800), Some(1)) => put("icmp", String::new()),
        (Some(0x86dd), Some(58)) => put("icmp6", String::new()),
        (Some(0x0806), None) => put("arp", String::new()),
        (Some(eth_type), None) => put("dl_type", format!("{eth_type:#06x}")),
        (eth_type, Some(proto)) => {
            bail!("ip_proto {proto} with eth_type {eth_type:?} has no match_fields spelling")
        }
    }
    if let Some(vid) = m.vlan_vid {
        put("dl_vlan", vid.to_string());
    }
    if let Some(mac) = &m.eth_src {
        put("dl_src", mac_text(mac));
    }
    if let Some(flags) = m.tcp_flags {
        put("tcp_flags", format!("{flags:#x}"));
    }
    if let Some(port) = m.tcp_src.or(m.udp_src) {
        put("tp_src", port.to_string());
    }
    if let Some(port) = m.tcp_dst.or(m.udp_dst) {
        put("tp_dst", port.to_string());
    }
    if let Some(addr) = m.ipv4_src {
        put("nw_src", cidr_text(addr, m.ipv4_src_mask));
    }
    if let Some(addr) = m.ipv4_dst {
        put("nw_dst", cidr_text(addr, m.ipv4_dst_mask));
    }
    if m.ct_state.is_some() || m.ct_state_mask.is_some() {
        let state = m.ct_state.unwrap_or_default();
        ensure!(
            m.ct_state_mask.unwrap_or(state) == state,
            "ct_state with a mask other than its own bits has no match_fields spelling"
        );
        const CT: [(u32, &str); 8] = [
            (0x01, "new"),
            (0x02, "est"),
            (0x04, "rel"),
            (0x08, "rpl"),
            (0x10, "inv"),
            (0x20, "trk"),
            (0x40, "snat"),
            (0x80, "dnat"),
        ];
        let known = CT.iter().fold(0u32, |all, (bit, _)| all | bit);
        ensure!(
            state & !known == 0,
            "ct_state bits {:#x} have no match_fields spelling",
            state & !known
        );
        let tokens: String = CT
            .iter()
            .filter(|(bit, _)| state & bit != 0)
            .map(|(_, name)| format!("+{name}"))
            .collect();
        put("ct_state", tokens);
    }
    Ok(fields)
}

/// The reverse of `openflow_translate`'s action building: the switch's
/// instructions as the plugin's `FlowAction` list. No instructions (or an
/// empty apply-actions) is a drop flow.
fn instructions_to_actions(
    list: &InstructionList,
    names: &HashMap<u32, String>,
) -> Result<Vec<FlowAction>> {
    let mut actions = Vec::new();
    for instruction in list.instructions() {
        match instruction {
            Instruction::ApplyActions(applied) => {
                let applied = applied.actions();
                let mut at = 0;
                while at < applied.len() {
                    if let Some(responder) = arp_responder(&applied[at..]) {
                        actions.push(responder);
                        at += ARP_RESPONDER_LEN;
                        continue;
                    }
                    actions.push(action_to_flow(&applied[at], names)?);
                    at += 1;
                }
            }
            other => bail!("instruction {other:?} has no FlowAction spelling"),
        }
    }
    if actions.is_empty() {
        actions.push(FlowAction::Drop);
    }
    Ok(actions)
}

fn action_to_flow(action: &Action, names: &HashMap<u32, String>) -> Result<FlowAction> {
    let set = |field: &str, value: String| FlowAction::SetField {
        field: field.to_owned(),
        value,
    };
    let output = |port: &str| FlowAction::Output {
        port: port.to_owned(),
    };
    Ok(match action {
        Action::Output(OutputPort::Port(port)) => FlowAction::Output {
            port: port_token(*port, names),
        },
        Action::Output(OutputPort::Local) => output("LOCAL"),
        Action::Output(OutputPort::Flood) => output("FLOOD"),
        Action::Output(OutputPort::All) => output("ALL"),
        Action::Output(OutputPort::InPort) => output("IN_PORT"),
        Action::Output(OutputPort::Controller) => output("CONTROLLER"),
        Action::Output(OutputPort::Normal) => FlowAction::Normal,
        Action::Controller { max_len } => FlowAction::Controller {
            max_len: Some(*max_len),
        },
        Action::SetEthSrc(mac) => set("eth_src", mac_text(mac)),
        Action::SetEthDst(mac) => set("eth_dst", mac_text(mac)),
        Action::SetVlanVid(vid) => set("vlan_vid", vid.to_string()),
        Action::SetTtl(ttl) => set("nw_ttl", ttl.to_string()),
        Action::SetTunnelId(id) => set("tunnel_id", id.to_string()),
        Action::SetTpSrc(port) => set("tp_src", port.to_string()),
        Action::SetTpDst(port) => set("tp_dst", port.to_string()),
        Action::NxResubmit {
            port: None,
            table: Some(table),
        } => FlowAction::Resubmit { table: *table },
        Action::NxRegLoad {
            dst_field,
            dst_ofs: 0,
            n_bits,
            value,
        } => match (*dst_field, *n_bits) {
            (field, 32) if field == nxm::REG0 => FlowAction::LoadRegister {
                register: 0,
                value: *value,
            },
            (field, 32) if field == nxm::REG1 => FlowAction::LoadRegister {
                register: 1,
                value: *value,
            },
            (field, 32) if field == nxm::REG2 => FlowAction::LoadRegister {
                register: 2,
                value: *value,
            },
            (field, 16) if field == nxm::TCP_SRC || field == nxm::UDP_SRC => {
                set("tp_src", value.to_string())
            }
            (field, 16) if field == nxm::TCP_DST || field == nxm::UDP_DST => {
                set("tp_dst", value.to_string())
            }
            (field, bits) => {
                bail!("load of {bits} bits into NXM field {field:#010x} has no FlowAction spelling")
            }
        },
        // Drop encodes to nothing, so a decoded Drop is rovs' stand-in for
        // an action it could not decode (e.g. a Nicira reg_load or move).
        Action::Drop => bail!("the switch reported an action rovs-openflow cannot decode"),
        other => bail!("action {other:?} has no FlowAction spelling"),
    })
}

/// Number of switch actions `FlowAction::ArpResponder` translates into.
const ARP_RESPONDER_LEN: usize = 8;

/// Recognize the exact action chain `openflow_translate` builds for
/// `ArpResponder { mac, ip }` at the start of `actions`.
fn arp_responder(actions: &[Action]) -> Option<FlowAction> {
    let moves = |action: &Action, src: u32, dst: u32, bits: u16| {
        matches!(
            action,
            Action::NxMove { src_field, dst_field, n_bits, src_ofs: 0, dst_ofs: 0 }
                if *src_field == src && *dst_field == dst && *n_bits == bits
        )
    };
    let loads = |action: &Action, field: u32, bits: u16| match action {
        Action::NxRegLoad {
            dst_field,
            dst_ofs: 0,
            n_bits,
            value,
        } if *dst_field == field && *n_bits == bits => Some(*value),
        _ => None,
    };
    let [
        m1,
        Action::SetEthSrc(mac),
        op,
        m2,
        sha,
        m3,
        spa,
        Action::Output(OutputPort::InPort),
    ] = actions.get(..ARP_RESPONDER_LEN)?
    else {
        return None;
    };
    let sha = loads(sha, nxm::ARP_SHA, 48)?;
    let spa = loads(spa, nxm::ARP_SPA, 32)?;
    let mac_value = mac
        .iter()
        .fold(0u64, |acc, byte| (acc << 8) | u64::from(*byte));
    let well_formed = moves(m1, nxm::ETH_SRC, nxm::ETH_DST, 48)
        && loads(op, nxm::ARP_OP, 16) == Some(2)
        && moves(m2, nxm::ARP_SHA, nxm::ARP_THA, 48)
        && sha == mac_value
        && moves(m3, nxm::ARP_SPA, nxm::ARP_TPA, 32)
        && spa <= u64::from(u32::MAX);
    well_formed.then(|| FlowAction::ArpResponder {
        mac: mac_text(mac),
        ip: std::net::Ipv4Addr::from(spa as u32).to_string(),
    })
}

impl SwitchView {
    /// This read's flow children of `bridge`, for [`flow_children_projection`].
    /// A read after a reconnect rebuilds them from the switch.
    pub(crate) fn children(&self, bridge: &str) -> Result<BTreeMap<String, FlowChild>> {
        validate_bridge(bridge)?;
        flow_children(bridge, &self.ports, &self.flows)
    }
}

fn canonical_flow_identity(flow: &FlowEntry) -> String {
    let mut digest = Sha256::new();
    digest.update([flow.table]);
    digest.update(flow.priority.to_be_bytes());
    let mut fields = flow.match_fields.iter().collect::<Vec<_>>();
    fields.sort_unstable();
    for (key, value) in fields {
        digest.update((key.len() as u64).to_be_bytes());
        digest.update(key.as_bytes());
        digest.update((value.len() as u64).to_be_bytes());
        digest.update(value.as_bytes());
    }
    hex::encode(digest.finalize())
}

type Slot = Arc<Mutex<Option<OpenFlowClient>>>;

/// In-process owner of the bridges' OpenFlow connections. Each bridge has one
/// lock; holding it makes this call the only writer to that switch.
pub(crate) struct OpenFlowSwitches {
    dir: PathBuf,
    bridges: StdMutex<HashMap<String, Slot>>,
}

impl OpenFlowSwitches {
    /// `dir` holds the `<bridge>.mgmt` sockets ([`OVS_RUN_DIR`] in production).
    pub(crate) fn new(dir: &Path) -> Self {
        Self {
            dir: dir.to_path_buf(),
            bridges: StdMutex::new(HashMap::new()),
        }
    }

    fn slot(&self, bridge: &str) -> Result<Slot> {
        let mut bridges = self
            .bridges
            .lock()
            .map_err(|_| anyhow!("OpenFlow switch table lock poisoned"))?;
        if let Some(slot) = bridges.get(bridge) {
            return Ok(slot.clone());
        }
        ensure!(
            bridges.len() < MAX_BRIDGES,
            "more than {MAX_BRIDGES} OpenFlow bridges requested"
        );
        let slot: Slot = Arc::new(Mutex::new(None));
        bridges.insert(bridge.to_owned(), slot.clone());
        Ok(slot)
    }

    /// One admitted compare-and-exchange for `add_flow`, `modify_flow` or
    /// `delete_flow` on `bridge`. Requests are validated before any switch
    /// I/O; `admit` runs once, after the compare and immediately before the
    /// write.
    pub(crate) async fn compare_and_exchange(
        &self,
        bridge: &str,
        method: &str,
        args: &Value,
        admit: FinalAdmission,
    ) -> Result<OpenFlowMethodReceipt> {
        let request = cas_request(method, args)?;
        ensure!(
            request.bridge == bridge,
            "bridge argument differs from the flow request's bridge"
        );
        let slot = self.slot(bridge)?;
        let path = self.dir.join(format!("{bridge}.mgmt"));
        tokio::time::timeout(CALL_TIMEOUT, async move {
            let mut guard = slot.lock().await;
            // Taken out of the slot for the call: if this future is dropped
            // (timeout) or the link fails, the connection goes with it and the
            // next call starts with a fresh hello.
            let mut client = match guard.take() {
                Some(client) => client,
                None => match connect(&path).await {
                    Ok(client) => client,
                    Err(error) => return Err::<OpenFlowMethodReceipt, anyhow::Error>(error),
                },
            };
            match exchange(&mut client, &request, &admit).await {
                Ok(receipt) => {
                    *guard = Some(client);
                    Ok(receipt)
                }
                Err(Failure::Refused(error)) => {
                    *guard = Some(client);
                    Err(error)
                }
                Err(Failure::Link(error)) => Err(error),
            }
        })
        .await
        .context("OpenFlow compare/exchange timed out; outcome unknown, not retried")?
    }
}

async fn connect(path: &Path) -> Result<OpenFlowClient> {
    let client = OpenFlowClient::connect_unix(path)
        .await
        .with_context(|| format!("OpenFlow switch socket {}", path.display()))?;
    ensure!(
        client.version() == Version::Of15,
        "switch negotiated {:?}, OpenFlow 1.5 is required",
        client.version()
    );
    Ok(client)
}

/// Why a call failed, and whether the connection is still sound.
enum Failure {
    /// The request was refused or the read-back differed; the link is sound.
    Refused(anyhow::Error),
    /// The switch link failed or answered out of turn; drop the connection.
    Link(anyhow::Error),
}

async fn exchange(
    client: &mut OpenFlowClient,
    request: &CasRequest,
    admit: &FinalAdmission,
) -> std::result::Result<OpenFlowMethodReceipt, Failure> {
    let ports = client.port_desc().await.map_err(Failure::Link)?;
    let port_map: HashMap<String, u32> = ports
        .iter()
        .map(|(name, (no, _))| (name.clone(), *no))
        .collect();
    let port_macs: HashMap<String, [u8; 6]> = ports
        .iter()
        .map(|(name, (_, mac))| (name.clone(), *mac))
        .collect();
    let plan = plan(request, &port_map, &port_macs).map_err(Failure::Refused)?;
    let live = client.dump_flows().await.map_err(Failure::Link)?;
    compare(&plan, &live).map_err(Failure::Refused)?;
    admit().await.map_err(Failure::Refused)?;
    client.flow_mod(&plan.write).await.map_err(|error| {
        Failure::Link(error.context("flow-mod not confirmed; outcome unknown, not retried"))
    })?;
    let after = client.dump_flows().await.map_err(|error| {
        Failure::Link(
            error.context("read-back failed after the write; outcome unknown, not retried"),
        )
    })?;
    verify(&plan, &after).map_err(Failure::Refused)?;
    // The write is verified; the children are this read-back, never a copy.
    let children = flow_children(&request.bridge, &ports, &after).map_err(|error| {
        Failure::Refused(error.context(
            "flow written and verified, but the bridge's flow children cannot be projected",
        ))
    })?;
    let output = serde_json::to_value(FlowMutationOutput {
        bridge: request.bridge.clone(),
        operation: request.operation.clone(),
        before: plan.before.clone(),
        after: plan.after.clone(),
        verified: true,
        observed_at_unix: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64,
    })
    .map_err(|error| Failure::Refused(error.into()))?;
    Ok(OpenFlowMethodReceipt {
        output,
        before: plan.before,
        after: plan.after,
        bridge: request.bridge.clone(),
        children,
    })
}

/// What one call will do, fully resolved against the switch's ports.
struct Plan {
    bridge: String,
    identity: FlowIdentity,
    cookie: u64,
    /// Live revision required before the write (modify, delete).
    expected: Option<Canon>,
    /// Revision required after the write (add, modify); `None` means absent.
    desired: Option<Canon>,
    write: Flow,
    before: Option<FlowEntry>,
    after: Option<FlowEntry>,
}

fn plan(
    request: &CasRequest,
    port_map: &HashMap<String, u32>,
    port_macs: &HashMap<String, [u8; 6]>,
) -> Result<Plan> {
    if request.delete && is_exempt_normal(&request.desired) {
        // The one foreign flow that may be deleted: the switch's own
        // `priority=0 actions=NORMAL` with cookie 0 (requirement 2.7).
        let identity = FlowIdentity::new(&request.bridge, 0, 0, &Match::new())?;
        let mut write = Flow::delete()
            .table(0)
            .priority(0)
            .match_fields(Match::new());
        write.command = FlowCommand::DeleteStrict;
        write.cookie = 0;
        write.cookie_mask = u64::MAX;
        let shape = Flow::add()
            .table(0)
            .priority(0)
            .cookie(0)
            .match_fields(Match::new())
            .actions(ActionList::new().normal());
        let mut before = request.desired.clone();
        before.cookie = Some(0);
        return Ok(Plan {
            bridge: request.bridge.clone(),
            identity,
            cookie: 0,
            expected: Some(canon_flow(&shape)?),
            desired: None,
            write,
            before: Some(before),
            after: None,
        });
    }

    let probe = json_flow_to_add(
        &serde_json::to_string(&request.desired)?,
        port_map,
        port_macs,
    )?;
    let identity = FlowIdentity::new(
        &request.bridge,
        probe.table_id,
        probe.priority,
        &probe.match_fields,
    )?;
    let cookie = identity.cookie();
    for flow in std::iter::once(&request.desired).chain(request.expected.iter()) {
        if let Some(given) = flow.cookie {
            ensure!(
                given == cookie,
                "cookie {given:#x} is not the derived cookie {cookie:#x} for this flow"
            );
        }
    }
    let with_cookie = |flow: &FlowEntry| {
        let mut flow = flow.clone();
        flow.cookie = Some(cookie);
        flow
    };
    let desired = with_cookie(&request.desired);
    let desired_json = serde_json::to_string(&desired)?;
    let expected = request.expected.as_ref().map(with_cookie);
    let expected_canon = match &expected {
        Some(flow) => {
            let live = json_flow_to_add(&serde_json::to_string(flow)?, port_map, port_macs)?;
            ensure!(
                (live.table_id, live.priority, live.match_fields.encode())
                    == (probe.table_id, probe.priority, probe.match_fields.encode()),
                "expected and requested flow have different identities"
            );
            Some(canon_flow(&live)?)
        }
        None => None,
    };
    let (write, desired_canon, after) = match request.operation {
        FlowMutationKind::Added => {
            let write = json_flow_to_add(&desired_json, port_map, port_macs)?;
            refuse_normal(&write)?;
            let canon = canon_flow(&write)?;
            (write, Some(canon), Some(desired))
        }
        FlowMutationKind::Modified => {
            let write = json_flow_to_modify(&desired_json, port_map, port_macs)?;
            refuse_normal(&write)?;
            let canon = canon_flow(&write)?;
            (write, Some(canon), Some(desired))
        }
        FlowMutationKind::Deleted => {
            let write = json_flow_to_delete(&desired_json, port_map, port_macs)?;
            (write, None, None)
        }
    };
    ensure!(
        write.cookie == cookie && (request.operation_is_add() || write.cookie_mask == u64::MAX),
        "flow-mod is not scoped to the derived cookie"
    );
    Ok(Plan {
        bridge: request.bridge.clone(),
        identity,
        cookie,
        expected: expected_canon,
        desired: desired_canon,
        write,
        before: expected,
        after,
    })
}

fn same_identity(flow: &SwitchFlow, identity: &FlowIdentity) -> bool {
    flow.table_id == identity.table
        && flow.priority == identity.priority
        && flow.match_fields.encode() == identity.match_bytes
}

/// Refuse unless the live table is exactly what the request expects.
fn compare(plan: &Plan, live: &[SwitchFlow]) -> Result<()> {
    if plan.cookie != 0 {
        for holder in live.iter().filter(|flow| flow.cookie == plan.cookie) {
            let held = FlowIdentity::new(
                &plan.bridge,
                holder.table_id,
                holder.priority,
                &holder.match_fields,
            )?;
            if plan.identity.check_cookie_holder(Some(&held))? && plan.expected.is_none() {
                bail!(
                    "flow {:#x} is already present on {}",
                    plan.cookie,
                    plan.bridge
                );
            }
        }
    }
    let same: Vec<&SwitchFlow> = live
        .iter()
        .filter(|flow| same_identity(flow, &plan.identity))
        .collect();
    match &plan.expected {
        None => {
            if let Some(other) = same.first() {
                bail!(
                    "a flow with the same identity (table, priority, match) is already on {} with cookie {:#x}",
                    plan.bridge,
                    other.cookie
                );
            }
        }
        Some(expected) => {
            let mut current = same
                .iter()
                .copied()
                .filter(|flow| flow.cookie == plan.cookie);
            let current = match (current.next(), current.next()) {
                (None, _) => bail!("flow {:#x} is absent on {}", plan.cookie, plan.bridge),
                (Some(one), None) => one,
                (Some(_), Some(_)) => bail!(
                    "flow {:#x} appears more than once on {}",
                    plan.cookie,
                    plan.bridge
                ),
            };
            ensure!(
                &canon_switch(current)? == expected,
                "live flow {:#x} on {} differs from the expected revision",
                plan.cookie,
                plan.bridge
            );
        }
    }
    Ok(())
}

/// Check the read-back against the write; a mismatch is an unknown outcome.
fn verify(plan: &Plan, after: &[SwitchFlow]) -> Result<()> {
    let same: Vec<&SwitchFlow> = after
        .iter()
        .filter(|flow| same_identity(flow, &plan.identity))
        .collect();
    match &plan.desired {
        Some(desired) => {
            let confirmed = match same.as_slice() {
                [one] => one.cookie == plan.cookie && &canon_switch(one)? == desired,
                _ => false,
            };
            ensure!(
                confirmed,
                "read-back of flow {:#x} on {} differs from the written flow; outcome unknown, not retried",
                plan.cookie,
                plan.bridge
            );
        }
        None => ensure!(
            !same.iter().any(|flow| flow.cookie == plan.cookie),
            "read-back still shows flow {:#x} on {} after delete; outcome unknown, not retried",
            plan.cookie,
            plan.bridge
        ),
    }
    Ok(())
}

/// The revision of a flow as compared: identity, cookie and instructions,
/// with the instructions re-encoded through rovs so equivalent spellings
/// (actions vs apply-actions, output `max_len`) compare equal. Counters,
/// timeouts and flags are not part of it.
#[derive(Debug, PartialEq, Eq)]
struct Canon {
    table: u8,
    priority: u16,
    cookie: u64,
    match_bytes: Vec<u8>,
    instructions: Vec<u8>,
}

fn canon_switch(flow: &SwitchFlow) -> Result<Canon> {
    Ok(Canon {
        table: flow.table_id,
        priority: flow.priority,
        cookie: flow.cookie,
        match_bytes: flow.match_fields.encode(),
        instructions: canon_instructions(&flow.instructions.encode())?,
    })
}

fn canon_flow(flow: &Flow) -> Result<Canon> {
    let raw = if flow.instructions.is_empty() && !flow.actions.is_empty() {
        Instruction::ApplyActions(flow.actions.clone()).encode()
    } else {
        flow.instructions.encode()
    };
    Ok(Canon {
        table: flow.table_id,
        priority: flow.priority,
        cookie: flow.cookie,
        match_bytes: flow.match_fields.encode(),
        instructions: canon_instructions(&raw)?,
    })
}

/// Decode and re-encode; an apply-actions with no actions (drop) is the same
/// as no instruction, which is how the switch reports a drop flow.
fn canon_instructions(raw: &[u8]) -> Result<Vec<u8>> {
    let decoded = InstructionList::decode(raw)
        .map_err(|error| anyhow!("undecodable instructions: {error}"))?;
    let mut canonical = InstructionList::new();
    for instruction in decoded.instructions() {
        if let Instruction::ApplyActions(actions) = instruction
            && actions.encode().is_empty()
        {
            continue;
        }
        canonical.push(instruction.clone());
    }
    Ok(canonical.encode())
}

fn refuse_normal(flow: &Flow) -> Result<()> {
    ensure!(
        !flow
            .actions
            .actions()
            .iter()
            .any(|action| matches!(action, Action::Output(OutputPort::Normal))),
        "NORMAL is not allowed: every path on a managed bridge is an explicit flow"
    );
    Ok(())
}

/// The switch's own `priority=0 actions=NORMAL` flow: table 0, empty match,
/// no plugin cookie, no timeouts.
fn is_exempt_normal(flow: &FlowEntry) -> bool {
    flow.table == 0
        && flow.priority == 0
        && flow.match_fields.is_empty()
        && flow.actions == [FlowAction::Normal]
        && matches!(flow.cookie, None | Some(0))
        && flow.idle_timeout == 0
        && flow.hard_timeout == 0
}

fn sends_to_normal(flow: &FlowEntry) -> bool {
    flow.actions.iter().any(|action| match action {
        FlowAction::Normal => true,
        FlowAction::Output { port } => port.eq_ignore_ascii_case("NORMAL"),
        _ => false,
    })
}

struct CasRequest {
    bridge: String,
    desired: FlowEntry,
    expected: Option<FlowEntry>,
    delete: bool,
    operation: FlowMutationKind,
}

impl CasRequest {
    fn operation_is_add(&self) -> bool {
        matches!(self.operation, FlowMutationKind::Added)
    }
}

fn cas_request(method: &str, args: &Value) -> Result<CasRequest> {
    let request = match method {
        "add_flow" => {
            let input: AddFlowInput = serde_json::from_value(args.clone())?;
            CasRequest {
                bridge: input.bridge,
                desired: input.flow,
                expected: None,
                delete: false,
                operation: FlowMutationKind::Added,
            }
        }
        "delete_flow" => {
            let input: DeleteFlowInput = serde_json::from_value(args.clone())?;
            CasRequest {
                bridge: input.bridge,
                desired: input.expected.clone(),
                expected: Some(input.expected),
                delete: true,
                operation: FlowMutationKind::Deleted,
            }
        }
        "modify_flow" => {
            let input: ModifyFlowInput = serde_json::from_value(args.clone())?;
            ensure!(
                flow_identity(&input.expected) == flow_identity(&input.replacement),
                "modify_flow cannot change table, priority or match_fields"
            );
            CasRequest {
                bridge: input.bridge,
                desired: input.replacement,
                expected: Some(input.expected),
                delete: false,
                operation: FlowMutationKind::Modified,
            }
        }
        _ => bail!("unknown OpenFlow member method: {method}"),
    };
    validate_bridge(&request.bridge)?;
    for flow in std::iter::once(&request.desired).chain(request.expected.iter()) {
        ensure!(
            serde_json::to_vec(flow)?.len() <= MAX_FLOW_JSON,
            "flow JSON is larger than {MAX_FLOW_JSON} bytes"
        );
    }
    if request.delete && is_exempt_normal(&request.desired) {
        return Ok(request);
    }
    validate_flow(&request.desired)?;
    if let Some(expected) = &request.expected {
        validate_flow(expected)?;
    }
    if !request.delete {
        ensure!(
            !sends_to_normal(&request.desired),
            "NORMAL is not allowed: every path on a managed bridge is an explicit flow"
        );
    }
    Ok(request)
}

fn flow_identity(flow: &FlowEntry) -> (u8, u16, Vec<(&str, &str)>) {
    let mut fields = flow
        .match_fields
        .iter()
        .map(|(key, value)| (key.as_str(), value.as_str()))
        .collect::<Vec<_>>();
    fields.sort_unstable();
    (flow.table, flow.priority, fields)
}

fn validate_bridge(bridge: &str) -> Result<()> {
    ensure!(
        !bridge.is_empty()
            && bridge.len() <= 15
            && bridge
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.')),
        "invalid OpenFlow bridge name"
    );
    Ok(())
}

fn validate_flow(flow: &FlowEntry) -> Result<()> {
    ensure!(flow.table < u8::MAX, "OpenFlow table 255 is reserved");
    ensure!(
        flow.priority > 0,
        "priority 0 is reserved: only the switch's own NORMAL flow may be deleted there"
    );
    ensure!(!flow.actions.is_empty(), "flow actions must be explicit");
    ensure!(
        !flow
            .actions
            .iter()
            .any(|action| matches!(action, FlowAction::Drop))
            || (flow.actions.len() == 1 && matches!(flow.actions.first(), Some(FlowAction::Drop))),
        "drop must be the only flow action"
    );
    ensure!(
        flow.match_fields.iter().all(|(key, value)| {
            !key.is_empty()
                && key.len() <= 64
                && value.len() <= 256
                && !key.chars().any(char::is_control)
                && !value.chars().any(char::is_control)
        }),
        "invalid OpenFlow match field"
    );
    for action in &flow.actions {
        match action {
            FlowAction::Output { port } => ensure!(!port.is_empty(), "output port is empty"),
            FlowAction::SetField { field, value } => ensure!(
                !field.is_empty() && !value.is_empty(),
                "set_field requires field and value"
            ),
            FlowAction::ArpResponder { mac, ip } => {
                ensure!(mac.split(':').count() == 6, "invalid ARP responder MAC");
                ip.parse::<std::net::Ipv4Addr>()
                    .context("invalid ARP responder IPv4 address")?;
            }
            _ => {}
        }
    }
    Ok(())
}

/// One read of a bridge's switch: its ports and its whole flow table, taken
/// under the bridge's lock. Reading writes nothing.
pub(crate) struct SwitchView {
    ports: HashMap<String, (u32, [u8; 6])>,
    pub(crate) flows: Vec<SwitchFlow>,
}

impl OpenFlowSwitches {
    /// Read ports and flows of `bridge` over the same per-bridge connection
    /// the writes use. A failed read drops the connection; nothing is retried.
    pub(crate) async fn observe(&self, bridge: &str) -> Result<SwitchView> {
        validate_bridge(bridge)?;
        let slot = self.slot(bridge)?;
        let path = self.dir.join(format!("{bridge}.mgmt"));
        tokio::time::timeout(CALL_TIMEOUT, async move {
            let mut guard = slot.lock().await;
            let mut client = match guard.take() {
                Some(client) => client,
                None => match connect(&path).await {
                    Ok(client) => client,
                    Err(error) => return Err::<SwitchView, anyhow::Error>(error),
                },
            };
            let view = read_switch(&mut client).await?;
            *guard = Some(client);
            Ok(view)
        })
        .await
        .context("OpenFlow switch read timed out")?
    }
}

async fn read_switch(client: &mut OpenFlowClient) -> Result<SwitchView> {
    let ports = client.port_desc().await?;
    let flows = client.dump_flows().await?;
    Ok(SwitchView { ports, flows })
}

impl SwitchView {
    pub(crate) fn has_port(&self, name: &str) -> bool {
        self.ports.contains_key(name)
    }

    /// The plan for adding `flow`, resolved against this switch's ports: its
    /// identity, derived cookie and canonical revision. A `flow` cookie, when
    /// present, must be the derived one.
    fn plan_for(&self, bridge: &str, flow: &FlowEntry) -> Result<Plan> {
        validate_bridge(bridge)?;
        let port_map: HashMap<String, u32> = self
            .ports
            .iter()
            .map(|(name, (no, _))| (name.clone(), *no))
            .collect();
        let port_macs: HashMap<String, [u8; 6]> = self
            .ports
            .iter()
            .map(|(name, (_, mac))| (name.clone(), *mac))
            .collect();
        let request = CasRequest {
            bridge: bridge.to_owned(),
            desired: flow.clone(),
            expected: None,
            delete: false,
            operation: FlowMutationKind::Added,
        };
        plan(&request, &port_map, &port_macs)
    }

    /// True when exactly `flow` is live: one flow with its identity, the
    /// derived cookie and the same instructions.
    pub(crate) fn holds(&self, bridge: &str, flow: &FlowEntry) -> Result<bool> {
        let plan = self.plan_for(bridge, flow)?;
        let same: Vec<&SwitchFlow> = self
            .flows
            .iter()
            .filter(|live| same_identity(live, &plan.identity))
            .collect();
        Ok(match (same.as_slice(), &plan.desired) {
            ([one], Some(desired)) => one.cookie == plan.cookie && &canon_switch(one)? == desired,
            _ => false,
        })
    }

    /// True when no flow with `flow`'s identity (table, priority, match) is
    /// live, whatever its cookie.
    pub(crate) fn lacks(&self, bridge: &str, flow: &FlowEntry) -> Result<bool> {
        let plan = self.plan_for(bridge, flow)?;
        Ok(!self
            .flows
            .iter()
            .any(|live| same_identity(live, &plan.identity)))
    }

    /// True when the switch's own `priority=0 actions=NORMAL` flow (table 0,
    /// empty match, cookie 0) is still installed.
    pub(crate) fn normal_present(&self) -> Result<bool> {
        let shape = canon_flow(
            &Flow::add()
                .table(0)
                .priority(0)
                .cookie(0)
                .match_fields(Match::new())
                .actions(ActionList::new().normal()),
        )?;
        for flow in &self.flows {
            if flow.table_id == 0
                && flow.priority == 0
                && flow.cookie == 0
                && canon_switch(flow)? == shape
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Flows without the plugin's cookie prefix. They are reported, never
    /// adopted (requirement 2.8).
    pub(crate) fn foreign(&self) -> impl Iterator<Item = &SwitchFlow> {
        self.flows
            .iter()
            .filter(|flow| !op_network::openflow_cookie::is_plugin_cookie(flow.cookie))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use op_network::openflow::fake_switch::FakeSwitch;
    use op_network::openflow_cookie::FlowIdentity;
    use rovs_openflow::{ActionList, Flow, FlowCommand, Match, OutputPort};
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    const MAC_ETH0: [u8; 6] = [0x52, 0x54, 0, 0, 0, 1];
    const MAC_BR: [u8; 6] = [0x02, 0, 0x64, 0x45, 0, 0x0a];

    fn flow(actions: Vec<FlowAction>) -> FlowEntry {
        FlowEntry {
            table: 4,
            priority: 200,
            match_fields: HashMap::from([("in_port".into(), "uplink".into())]),
            actions,
            cookie: Some(42),
            idle_timeout: 0,
            hard_timeout: 0,
        }
    }

    fn output(port: &str) -> FlowAction {
        FlowAction::Output { port: port.into() }
    }

    /// `in_port=<in_port> actions=output:<out>` at table 0, priority 10, no cookie.
    fn entry(in_port: &str, out: &str) -> FlowEntry {
        FlowEntry {
            table: 0,
            priority: 10,
            match_fields: HashMap::from([("in_port".into(), in_port.into())]),
            actions: vec![output(out)],
            cookie: None,
            idle_timeout: 0,
            hard_timeout: 0,
        }
    }

    /// The cookie the plugin derives for `in_port=<n>` on table 0, priority 10.
    fn derived(in_port: u32) -> u64 {
        FlowIdentity::new("ovsbr0", 0, 10, &Match::new().in_port(in_port))
            .unwrap()
            .cookie()
    }

    struct Rig {
        _dir: tempfile::TempDir,
        switch: FakeSwitch,
        switches: OpenFlowSwitches,
    }

    fn rig() -> Rig {
        let dir = tempfile::tempdir().unwrap();
        let switch = FakeSwitch::start(
            dir.path(),
            "ovsbr0",
            &[
                ("eth0", 1, MAC_ETH0),
                ("tap7", 7, MAC_ETH0),
                ("ovsbr0", 0xffff_fffe, MAC_BR),
            ],
        );
        let switches = OpenFlowSwitches::new(dir.path());
        Rig {
            _dir: dir,
            switch,
            switches,
        }
    }

    fn admit_ok() -> (FinalAdmission, Arc<AtomicUsize>) {
        let calls = Arc::new(AtomicUsize::new(0));
        let seen = calls.clone();
        let admit: FinalAdmission = Arc::new(move || {
            seen.fetch_add(1, Ordering::SeqCst);
            Box::pin(async { Ok(()) })
        });
        (admit, calls)
    }

    fn admit_denied() -> (FinalAdmission, Arc<AtomicUsize>) {
        let calls = Arc::new(AtomicUsize::new(0));
        let seen = calls.clone();
        let admit: FinalAdmission = Arc::new(move || {
            seen.fetch_add(1, Ordering::SeqCst);
            Box::pin(async { Err(anyhow::anyhow!("identity not admitted")) })
        });
        (admit, calls)
    }

    impl Rig {
        async fn call(&self, method: &str, args: Value) -> Result<OpenFlowMethodReceipt> {
            let (admit, _) = admit_ok();
            self.switches
                .compare_and_exchange("ovsbr0", method, &args, admit)
                .await
        }

        async fn add(&self, flow: &FlowEntry) -> Result<OpenFlowMethodReceipt> {
            self.call(
                "add_flow",
                serde_json::json!({"bridge":"ovsbr0","flow":flow}),
            )
            .await
        }
    }

    fn seed_flow(in_port: u32, out: u32, cookie: u64) -> Flow {
        Flow::add()
            .priority(10)
            .cookie(cookie)
            .match_fields(Match::new().in_port(in_port))
            .actions(ActionList::new().output(OutputPort::Port(out)))
    }

    // ---- request building and bounds -------------------------------------

    #[test]
    fn three_methods_build_exact_compare_exchange_requests() {
        let before = flow(vec![output("LOCAL")]);
        let after = flow(vec![output("uplink")]);
        let add = cas_request(
            "add_flow",
            &serde_json::json!({"bridge":"ovsbr0","flow":before.clone()}),
        )
        .unwrap();
        assert!(add.expected.is_none() && !add.delete);
        let delete = cas_request(
            "delete_flow",
            &serde_json::json!({"bridge":"ovsbr0","expected":before.clone()}),
        )
        .unwrap();
        assert_eq!(delete.expected.as_ref(), Some(&delete.desired));
        assert!(delete.delete);
        let modify = cas_request(
            "modify_flow",
            &serde_json::json!({"bridge":"ovsbr0","expected":before,"replacement":after}),
        )
        .unwrap();
        assert!(modify.expected.is_some() && !modify.delete);
    }

    #[test]
    fn wildcard_delete_identity_change_and_priority_zero_are_unrepresentable() {
        assert!(
            cas_request(
                "delete_flow",
                &serde_json::json!({"bridge":"ovsbr0","match_fields":{}})
            )
            .is_err()
        );
        let expected = flow(vec![output("LOCAL")]);
        let mut changed = expected.clone();
        changed.priority += 1;
        assert!(
            cas_request(
                "modify_flow",
                &serde_json::json!({"bridge":"ovsbr0","expected":expected,"replacement":changed})
            )
            .is_err()
        );
        let mut zero = flow(vec![output("LOCAL")]);
        zero.priority = 0;
        for method in ["add_flow"] {
            assert!(
                cas_request(method, &serde_json::json!({"bridge":"ovsbr0","flow":zero})).is_err()
            );
        }
        assert!(
            cas_request(
                "modify_flow",
                &serde_json::json!({"bridge":"ovsbr0","expected":zero.clone(),"replacement":zero})
            )
            .is_err()
        );
        assert!(
            cas_request(
                "delete_flow",
                &serde_json::json!({"bridge":"ovsbr0","expected":zero})
            )
            .is_err(),
            "priority 0 delete is only for the exact foreign NORMAL flow"
        );
    }

    #[test]
    fn normal_actions_are_refused_for_add_and_modify_but_not_delete() {
        let normal = flow(vec![FlowAction::Normal]);
        let other = flow(vec![output("LOCAL")]);
        assert!(
            cas_request(
                "add_flow",
                &serde_json::json!({"bridge":"ovsbr0","flow":normal})
            )
            .is_err()
        );
        assert!(
            cas_request(
                "modify_flow",
                &serde_json::json!({"bridge":"ovsbr0","expected":other,"replacement":normal})
            )
            .is_err()
        );
        assert!(
            cas_request(
                "delete_flow",
                &serde_json::json!({"bridge":"ovsbr0","expected":normal})
            )
            .is_ok(),
            "a NORMAL flow that exists must still be deletable"
        );
    }

    #[test]
    fn bridge_name_and_flow_size_bounds() {
        let ok = flow(vec![output("LOCAL")]);
        assert!(
            cas_request(
                "add_flow",
                &serde_json::json!({"bridge":"ovsbr0123456789a","flow":ok.clone()})
            )
            .is_err(),
            "16 bytes is over the bridge name bound"
        );
        assert!(
            cas_request(
                "add_flow",
                &serde_json::json!({"bridge":"ovsbr0123456789","flow":ok.clone()})
            )
            .is_ok(),
            "15 bytes is allowed"
        );
        let mut big = ok;
        for i in 0..400 {
            big.match_fields.insert(format!("k{i:03}"), "v".repeat(200));
        }
        assert!(serde_json::to_vec(&big).unwrap().len() > MAX_FLOW_JSON);
        assert!(
            cas_request(
                "add_flow",
                &serde_json::json!({"bridge":"ovsbr0","flow":big})
            )
            .is_err()
        );
    }

    // ---- compare and exchange against a fake switch with a real table ----

    #[tokio::test]
    async fn add_creates_the_flow_with_the_derived_cookie_and_verifies_it() {
        let rig = rig();
        let receipt = rig.add(&entry("eth0", "LOCAL")).await.unwrap();

        let table = rig.switch.flows();
        assert_eq!(table.len(), 1, "the fake switch's table changed");
        assert_eq!(table[0].cookie, derived(1));
        assert_eq!(table[0].priority, 10);
        assert_eq!(rig.switch.flow_mods(), 1);

        assert!(receipt.before.is_none());
        assert_eq!(receipt.after.as_ref().unwrap().cookie, Some(derived(1)));
        assert_eq!(receipt.output["verified"], true);
        assert_eq!(receipt.output["operation"], "added");
        assert_eq!(receipt.output["after"]["cookie"], derived(1));
        projection_patch(&receipt).unwrap();
    }

    #[tokio::test]
    async fn uplink_pair_resolves_local_from_the_port_table() {
        let rig = rig();
        rig.add(&entry("eth0", "LOCAL")).await.unwrap();
        rig.add(&entry("LOCAL", "eth0")).await.unwrap();
        let table = rig.switch.flows();
        assert_eq!(table.len(), 2);
        let local = table.iter().find(|f| f.cookie == derived(0xffff_fffe));
        assert!(local.is_some(), "in_port=LOCAL resolved to 0xfffffffe");
    }

    #[tokio::test]
    async fn add_with_the_cookie_already_present_is_refused_without_a_flow_mod() {
        let rig = rig();
        rig.add(&entry("eth0", "LOCAL")).await.unwrap();
        assert_eq!(rig.switch.flow_mods(), 1);
        let err = rig.add(&entry("eth0", "LOCAL")).await.unwrap_err();
        assert!(format!("{err:#}").contains("already present"), "{err:#}");
        assert_eq!(rig.switch.flow_mods(), 1, "no FLOW_MOD was sent");
    }

    #[tokio::test]
    async fn add_over_a_foreign_flow_with_the_same_identity_is_refused() {
        let rig = rig();
        rig.switch.seed(&seed_flow(1, 2, 0));
        let err = rig.add(&entry("eth0", "LOCAL")).await.unwrap_err();
        assert!(format!("{err:#}").contains("identity"), "{err:#}");
        assert_eq!(rig.switch.flow_mods(), 0);
        assert_eq!(rig.switch.flows()[0].cookie, 0, "foreign flow untouched");
    }

    #[tokio::test]
    async fn add_whose_cookie_is_held_by_a_different_flow_is_a_collision() {
        let rig = rig();
        // a different identity (in_port 9) already holds the cookie in_port 1 derives
        rig.switch.seed(&seed_flow(9, 2, derived(1)));
        let err = rig.add(&entry("eth0", "LOCAL")).await.unwrap_err();
        assert!(format!("{err:#}").contains("collision"), "{err:#}");
        assert_eq!(rig.switch.flow_mods(), 0);
    }

    #[tokio::test]
    async fn input_cookie_that_is_not_the_derived_one_is_refused() {
        let rig = rig();
        let mut wrong = entry("eth0", "LOCAL");
        wrong.cookie = Some(derived(1) ^ 1);
        let err = rig.add(&wrong).await.unwrap_err();
        assert!(format!("{err:#}").contains("derived"), "{err:#}");
        assert_eq!(rig.switch.flow_mods(), 0);

        let mut right = entry("eth0", "LOCAL");
        right.cookie = Some(derived(1));
        rig.add(&right).await.unwrap();
        assert_eq!(rig.switch.flows()[0].cookie, derived(1));
    }

    #[tokio::test]
    async fn add_of_a_normal_action_is_refused_before_any_io() {
        let rig = rig();
        let mut normal = entry("eth0", "LOCAL");
        normal.actions = vec![FlowAction::Normal];
        assert!(rig.add(&normal).await.is_err());
        assert_eq!(rig.switch.flow_mods(), 0);
        assert_eq!(rig.switch.hellos(), 0, "refused before connecting");
    }

    #[tokio::test]
    async fn modify_replaces_the_actions_when_expected_matches() {
        let rig = rig();
        rig.add(&entry("eth0", "LOCAL")).await.unwrap();
        let receipt = rig
            .call(
                "modify_flow",
                serde_json::json!({
                    "bridge":"ovsbr0",
                    "expected": entry("eth0", "LOCAL"),
                    "replacement": entry("eth0", "tap7"),
                }),
            )
            .await
            .unwrap();
        assert_eq!(rig.switch.flow_mods(), 2);
        let table = rig.switch.flows();
        assert_eq!(table.len(), 1);
        assert_eq!(table[0].cookie, derived(1), "identity and cookie unchanged");
        let want =
            rovs_openflow::Instruction::ApplyActions(ActionList::new().output(OutputPort::Port(7)))
                .encode();
        assert_eq!(table[0].instruction_bytes, want);
        assert_eq!(
            receipt.before.as_ref().unwrap().actions,
            vec![output("LOCAL")]
        );
        assert_eq!(
            receipt.after.as_ref().unwrap().actions,
            vec![output("tap7")]
        );
        assert_eq!(receipt.output["operation"], "modified");
    }

    #[tokio::test]
    async fn modify_with_a_stale_expected_is_a_cas_conflict_and_writes_nothing() {
        let rig = rig();
        rig.add(&entry("eth0", "LOCAL")).await.unwrap();
        let err = rig
            .call(
                "modify_flow",
                serde_json::json!({
                    "bridge":"ovsbr0",
                    "expected": entry("eth0", "tap7"),
                    "replacement": entry("eth0", "eth0"),
                }),
            )
            .await
            .unwrap_err();
        assert!(format!("{err:#}").contains("differs"), "{err:#}");
        assert_eq!(rig.switch.flow_mods(), 1, "only the original add");
    }

    #[tokio::test]
    async fn delete_removes_the_flow_and_verifies_absence() {
        let rig = rig();
        rig.add(&entry("eth0", "LOCAL")).await.unwrap();
        let receipt = rig
            .call(
                "delete_flow",
                serde_json::json!({"bridge":"ovsbr0","expected":entry("eth0","LOCAL")}),
            )
            .await
            .unwrap();
        assert!(rig.switch.flows().is_empty());
        assert!(receipt.after.is_none());
        assert_eq!(receipt.before.as_ref().unwrap().cookie, Some(derived(1)));
        assert_eq!(receipt.output["operation"], "deleted");
        projection_patch(&receipt).unwrap();
    }

    #[tokio::test]
    async fn delete_with_a_stale_expected_or_an_absent_flow_writes_nothing() {
        let rig = rig();
        let err = rig
            .call(
                "delete_flow",
                serde_json::json!({"bridge":"ovsbr0","expected":entry("eth0","LOCAL")}),
            )
            .await
            .unwrap_err();
        assert!(format!("{err:#}").contains("absent"), "{err:#}");

        rig.add(&entry("eth0", "LOCAL")).await.unwrap();
        let err = rig
            .call(
                "delete_flow",
                serde_json::json!({"bridge":"ovsbr0","expected":entry("eth0","tap7")}),
            )
            .await
            .unwrap_err();
        assert!(format!("{err:#}").contains("differs"), "{err:#}");
        assert_eq!(rig.switch.flow_mods(), 1, "only the original add");
        assert_eq!(rig.switch.flows().len(), 1);
    }

    #[tokio::test]
    async fn denied_final_admission_refuses_before_any_flow_mod() {
        let rig = rig();
        let (admit, calls) = admit_denied();
        let args = serde_json::json!({"bridge":"ovsbr0","flow":entry("eth0","LOCAL")});
        let err = rig
            .switches
            .compare_and_exchange("ovsbr0", "add_flow", &args, admit)
            .await
            .unwrap_err();
        assert!(
            format!("{err:#}").contains("identity not admitted"),
            "{err:#}"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1, "admission ran once");
        assert_eq!(rig.switch.flow_mods(), 0);
        assert!(rig.switch.flows().is_empty());
    }

    #[tokio::test]
    async fn final_admission_runs_after_the_compare_and_only_once_per_write() {
        let rig = rig();
        let (admit, calls) = admit_ok();
        let stale = serde_json::json!({
            "bridge":"ovsbr0","expected":entry("eth0","LOCAL")
        });
        assert!(
            rig.switches
                .compare_and_exchange("ovsbr0", "delete_flow", &stale, admit.clone())
                .await
                .is_err()
        );
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "CAS refusal precedes admission"
        );
        let add = serde_json::json!({"bridge":"ovsbr0","flow":entry("eth0","LOCAL")});
        rig.switches
            .compare_and_exchange("ovsbr0", "add_flow", &add, admit)
            .await
            .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn read_back_mismatch_is_reported_as_an_unknown_outcome() {
        let rig = rig();
        rig.switch.set_ignore_flow_mods(true);
        let err = rig.add(&entry("eth0", "LOCAL")).await.unwrap_err();
        let text = format!("{err:#}");
        assert!(text.contains("outcome unknown"), "{text}");
        assert_eq!(rig.switch.flow_mods(), 1, "written once, not retried");
    }

    #[tokio::test]
    async fn delete_of_the_exact_foreign_normal_flow_is_allowed() {
        let rig = rig();
        rig.switch.seed(
            &Flow::add()
                .priority(0)
                .cookie(0)
                .actions(ActionList::new().output(OutputPort::Normal)),
        );
        rig.switch.seed(&seed_flow(1, 2, derived(1)));
        let normal = FlowEntry {
            table: 0,
            priority: 0,
            match_fields: HashMap::new(),
            actions: vec![FlowAction::Normal],
            cookie: None,
            idle_timeout: 0,
            hard_timeout: 0,
        };
        let receipt = rig
            .call(
                "delete_flow",
                serde_json::json!({"bridge":"ovsbr0","expected":normal}),
            )
            .await
            .unwrap();
        let table = rig.switch.flows();
        assert_eq!(table.len(), 1, "only the NORMAL flow went");
        assert_eq!(table[0].cookie, derived(1));
        assert_eq!(receipt.before.as_ref().unwrap().cookie, Some(0));
        assert!(receipt.after.is_none());
    }

    #[tokio::test]
    async fn delete_of_another_foreign_flow_is_refused() {
        let rig = rig();
        rig.switch.seed(&seed_flow(1, 2, 0)); // cookie 0, not NORMAL
        for cookie in [None, Some(0)] {
            let mut foreign = entry("eth0", "tap7");
            foreign.actions = vec![output("2")];
            foreign.cookie = cookie;
            let err = rig
                .call(
                    "delete_flow",
                    serde_json::json!({"bridge":"ovsbr0","expected":foreign}),
                )
                .await
                .unwrap_err();
            assert!(!format!("{err:#}").is_empty());
        }
        assert_eq!(rig.switch.flow_mods(), 0);
        assert_eq!(rig.switch.flows().len(), 1);
    }

    #[tokio::test]
    async fn a_normal_flow_with_a_priority_or_match_of_its_own_is_not_the_exempt_one() {
        let rig = rig();
        rig.switch.seed(
            &Flow::add()
                .priority(5)
                .cookie(0)
                .actions(ActionList::new().output(OutputPort::Normal)),
        );
        let mut not_exact = FlowEntry {
            table: 0,
            priority: 5,
            match_fields: HashMap::new(),
            actions: vec![FlowAction::Normal],
            cookie: Some(0),
            idle_timeout: 0,
            hard_timeout: 0,
        };
        assert!(
            rig.call(
                "delete_flow",
                serde_json::json!({"bridge":"ovsbr0","expected":not_exact.clone()})
            )
            .await
            .is_err()
        );
        not_exact.priority = 0;
        not_exact.table = 3;
        assert!(
            rig.call(
                "delete_flow",
                serde_json::json!({"bridge":"ovsbr0","expected":not_exact})
            )
            .await
            .is_err()
        );
        assert_eq!(rig.switch.flow_mods(), 0);
    }

    // ---- connection ownership --------------------------------------------

    #[tokio::test]
    async fn one_connection_is_reused_and_an_io_error_drops_it_for_a_fresh_hello() {
        let rig = rig();
        rig.add(&entry("eth0", "LOCAL")).await.unwrap();
        rig.add(&entry("LOCAL", "eth0")).await.unwrap();
        assert_eq!(rig.switch.hellos(), 1, "lazy connect once, then reused");

        // The switch closes the next connection after the hello, but the
        // current one stays up; kill it by closing the socket from the
        // client side: simulate with a fresh owner on the same socket dir.
        rig.switch.close_next_connections(1);
        let fresh = OpenFlowSwitches::new(rig._dir.path());
        let (admit, _) = admit_ok();
        let args = serde_json::json!({"bridge":"ovsbr0","flow":entry("tap7","LOCAL")});
        assert!(
            fresh
                .compare_and_exchange("ovsbr0", "add_flow", &args, admit.clone())
                .await
                .is_err(),
            "the closed connection fails the call"
        );
        assert_eq!(rig.switch.flow_mods(), 2, "nothing was written or retried");
        fresh
            .compare_and_exchange("ovsbr0", "add_flow", &args, admit)
            .await
            .unwrap();
        assert_eq!(rig.switch.hellos(), 3, "the next call reconnected");
        assert_eq!(rig.switch.flow_mods(), 3);
    }

    #[tokio::test]
    async fn bridge_argument_must_match_the_request() {
        let rig = rig();
        let (admit, _) = admit_ok();
        let args = serde_json::json!({"bridge":"other","flow":entry("eth0","LOCAL")});
        assert!(
            rig.switches
                .compare_and_exchange("ovsbr0", "add_flow", &args, admit)
                .await
                .is_err()
        );
        assert_eq!(rig.switch.hellos(), 0);
    }

    // ---- canonical comparison --------------------------------------------

    #[test]
    fn canonical_form_ignores_output_max_len_and_the_actions_vs_instructions_spelling() {
        // OVS reports output max_len 0 (apply-actions, type 4, length 24).
        let mut ovs = vec![0, 4, 0, 24, 0, 0, 0, 0, 0, 0, 0, 16];
        ovs.extend_from_slice(&7u32.to_be_bytes());
        ovs.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0]);
        let switch_flow = op_network::openflow::SwitchFlow {
            table_id: 0,
            priority: 10,
            idle_timeout: 0,
            hard_timeout: 0,
            flags: 0,
            importance: 0,
            cookie: 5,
            match_fields: Match::new().in_port(1),
            instructions: rovs_openflow::InstructionList::decode(&ovs).unwrap(),
            duration_sec: 9,
            duration_nsec: 0,
            idle_time_sec: None,
            packet_count: 100,
            byte_count: 100,
        };
        let desired = seed_flow(1, 7, 5);
        let mut via_instructions = desired.clone();
        via_instructions.instructions = rovs_openflow::InstructionList::new()
            .apply_actions(ActionList::new().output(OutputPort::Port(7)));
        via_instructions.actions = ActionList::new();
        let a = canon_switch(&switch_flow).unwrap();
        assert_eq!(a, canon_flow(&desired).unwrap());
        assert_eq!(a, canon_flow(&via_instructions).unwrap());
        // counters are not part of the identity of the revision
        let mut busier = switch_flow.clone();
        busier.packet_count += 1;
        assert_eq!(a, canon_switch(&busier).unwrap());
        // but the cookie and the actions are
        let mut other = desired.clone();
        other.cookie = 6;
        assert_ne!(a, canon_flow(&other).unwrap());
        assert_ne!(a, canon_flow(&seed_flow(1, 8, 5)).unwrap());
        assert_eq!(desired.command, FlowCommand::Add);
    }

    #[test]
    fn observed_member_patch_never_replaces_authored_configuration() {
        let flow = flow(vec![output("LOCAL")]);
        let identity = canonical_flow_identity(&flow);
        let patch = projection_patch(&OpenFlowMethodReceipt {
            output: Value::Null,
            before: None,
            after: Some(flow.clone()),
            bridge: "ovsbr0".into(),
            children: BTreeMap::new(),
        })
        .unwrap();
        assert_eq!(patch.as_object().unwrap().len(), 1);
        assert_eq!(patch["observed_flows"].as_object().unwrap().len(), 1);
        assert_eq!(
            patch["observed_flows"]["ovsbr0"][&identity]["installed"],
            true
        );
        let removed = projection_patch(&OpenFlowMethodReceipt {
            output: Value::Null,
            before: Some(flow.clone()),
            after: None,
            bridge: "ovsbr0".into(),
            children: BTreeMap::new(),
        })
        .unwrap();
        assert_eq!(
            removed["observed_flows"]["ovsbr0"][&identity]["installed"],
            false
        );
        assert_eq!(
            removed["observed_flows"]["ovsbr0"][&identity]["flow"],
            serde_json::to_value(flow).unwrap()
        );
        assert!(removed.get("bridges").is_none());
    }
}
