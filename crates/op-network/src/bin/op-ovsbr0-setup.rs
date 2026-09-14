//! op-ovsbr0-setup — ensure ovsbr0 exists with datapath_type=system and add ports.
//!
//! Uses the rovs crate family to manipulate OVSDB directly and the vswitchd
//! unixctl socket to stop the daemon cleanly.  No ovs-vsctl/ovsdb-client shell
//! commands are used.  `ovs-dpctl del-dp` is used only while vswitchd is down
//! to clear stale kernel datapath state.
//!
//! Environment variables:
//!   BRIDGE         OVS bridge name          (default: ovsbr0)
//!   UPLINK         Physical NIC to enslave  (optional; enslaved in the SAME
//!                  OVSDB transact as bridge creation — uplink capture only
//!                  starts correctly when vswitchd first reads the bridge and
//!                  its ports together, so this must not be a second step)
//!   FAIL_MODE      OVS fail mode            (default: standalone)
//!   PUBLIC_PORT    Internal port carrying the public uplink IP
//!                  (default: pub0) — the uplink's MAC is pinned on THIS
//!                  port's Interface row, never on the bridge itself
//!   INTERNAL_PORTS Space-separated extra internal ports seeded with the
//!                  bridge (default: "svc0 grpc"; svc0 = grpc endpoint,
//!                  grpc = named spare for the container/grpc link)
//!   SHARED_MAC     PUBLIC_PORT MAC override (default: UPLINK's own MAC when
//!                  UPLINK is set, so the public port presents the same
//!                  address upstream as the NIC always did; fa:16:3e:f1:71:d2
//!                  only when there's no UPLINK to match). The bridge device
//!                  itself NEVER gets this MAC — it keeps a random one.
//!   OVSDB_SOCKET   Path to OVSDB socket     (default: auto-detect)
//!   VSWITCHD_SVC   s6 service NAME          (default: ovs-vswitchd; lifecycle
//!                  goes through the sanctioned `service6` wrapper only —
//!                  never raw s6-svc/s6-svstat)
//!   VSWITCHD_CTL   Glob for vswitchd unixctl socket
//!                  (default: /usr/local/var/run/openvswitch/ovs-vswitchd.*.ctl)
//!
//! Modes:
//!   --seed-only     Write OVSDB system bridge/port rows and exit.  This is
//!                   for the s6 ovs-vswitchd run script before vswitchd starts.

use anyhow::{bail, Context, Result};
use rovs_jsonrpc::Connection;
use rovs_ovsdb::{Client, Transaction};
use rovs_transport::{Address, Stream};
use serde_json::{json, Value};
use std::path::Path;
use std::process::Command;
use std::time::Duration;
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;
use uuid::Uuid;

struct Config {
    bridge: String,
    uplink: String,
    fail_mode: String,
    /// `PUBLIC_PORT` env var: internal port that carries the public uplink
    /// IP (and the uplink's MAC). The bridge device itself never gets the
    /// uplink MAC — pinning it on the bridge and later moving the IP off
    /// blackholes inbound traffic (the provider filters on this MAC).
    public_port: String,
    /// `INTERNAL_PORTS` env var: space-separated extra internal ports seeded
    /// alongside the bridge (e.g. "svc0 grpc").
    internal_ports: Vec<String>,
    /// `SHARED_MAC` env var, if explicitly set. When unset and `uplink` is
    /// configured, the public port's MAC is taken from the uplink NIC's real
    /// hardware address instead (see `resolve_shared_mac`) — a hosting
    /// provider's virtual switch typically filters on the MAC it originally
    /// handed the NIC, so presenting any other MAC on that link silently
    /// blackholes all traffic once the NIC is enslaved.
    shared_mac_override: Option<String>,
    ovsdb_socket: String,
    vswitchd_svc: String,
}

impl Config {
    fn from_env() -> Self {
        Config {
            bridge: std::env::var("BRIDGE").unwrap_or_else(|_| "ovsbr0".into()),
            uplink: std::env::var("UPLINK").unwrap_or_default(),
            fail_mode: std::env::var("FAIL_MODE").unwrap_or_else(|_| "standalone".into()),
            public_port: std::env::var("PUBLIC_PORT").unwrap_or_else(|_| "pub0".into()),
            internal_ports: std::env::var("INTERNAL_PORTS")
                .unwrap_or_else(|_| "svc0 grpc".into())
                .split_whitespace()
                .map(|s| s.to_string())
                .collect(),
            shared_mac_override: std::env::var("SHARED_MAC").ok(),
            ovsdb_socket: std::env::var("OVSDB_SOCKET").unwrap_or_else(|_| find_socket_path()),
            vswitchd_svc: std::env::var("VSWITCHD_SVC").unwrap_or_else(|_| "ovs-vswitchd".into()),
        }
    }
}

/// Read a network interface's real hardware address from sysfs.
fn read_iface_mac(iface: &str) -> Result<String> {
    let path = format!("/sys/class/net/{iface}/address");
    let mac = std::fs::read_to_string(&path)
        .with_context(|| format!("reading MAC address from {path}"))?;
    Ok(mac.trim().to_string())
}

/// Resolve the public port's MAC: an explicit `SHARED_MAC` override always
/// wins; otherwise, when an uplink is configured, use *its* real MAC so the
/// public port presents the same address upstream as the NIC always did
/// (enslaving a physical NIC changes nothing about which MAC the hosting
/// provider's virtual switch expects on that link). Only falls back to a
/// placeholder when there's no physical uplink to match at all. This MAC is
/// pinned on the PUBLIC_PORT interface row — never on the bridge row.
fn resolve_shared_mac(cfg: &Config) -> String {
    if let Some(mac) = &cfg.shared_mac_override {
        return mac.clone();
    }
    if !cfg.uplink.is_empty() {
        match read_iface_mac(&cfg.uplink) {
            Ok(mac) => {
                info!("using {}'s own MAC {} for the bridge", cfg.uplink, mac);
                return mac;
            }
            Err(e) => {
                warn!(
                    "could not read {}'s MAC ({}); falling back to placeholder — \
                     traffic on this uplink may be dropped upstream if the provider \
                     filters by MAC",
                    cfg.uplink, e
                );
            }
        }
    }
    "fa:16:3e:f1:71:d2".to_string()
}

/// Where the uplink's pre-enslavement IPv4 addresses/gateway are persisted
/// (a tmpfs path already created by the opdbus-rundirs oneshot).
const UPLINK_SNAPSHOT_PATH: &str = "/run/opdbus/uplink-migration.env";

/// Snapshot `uplink`'s current IPv4 addresses and default gateway to
/// `UPLINK_SNAPSHOT_PATH`, so a later step (after the uplink has been
/// enslaved into the bridge) can apply them to the bridge instead of
/// re-reading the uplink — the kernel strips all IPs from an interface the
/// instant it's captured into a bridge, so by the time anything downstream
/// looks, the uplink itself has nothing left to read.
///
/// Only overwrites the snapshot when the uplink currently *has* addresses;
/// if it's already been stripped (e.g. a later idempotent run), an existing
/// good snapshot from the real capture is left alone rather than clobbered
/// with an empty one.
fn snapshot_uplink_before_enslavement(uplink: &str) {
    if uplink.is_empty() {
        return;
    }
    let addrs = Command::new("ip")
        .args(["-4", "-o", "addr", "show", "dev", uplink, "scope", "global"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default();
    let addrs: Vec<String> = addrs
        .lines()
        .filter_map(|line| line.split_whitespace().nth(3))
        .map(|s| s.to_string())
        .collect();
    if addrs.is_empty() {
        info!(
            "{} has no IPv4 addresses right now — leaving any existing {} in place",
            uplink, UPLINK_SNAPSHOT_PATH
        );
        return;
    }

    let gw = Command::new("ip")
        .args(["-4", "route", "show", "default", "dev", uplink])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default()
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(2))
        .map(|s| s.to_string())
        .unwrap_or_default();

    let contents = format!(
        "UPLINK_ADDRS=\"{}\"\nUPLINK_GW=\"{}\"\n",
        addrs.join(" "),
        gw
    );
    match std::fs::write(UPLINK_SNAPSHOT_PATH, &contents) {
        Ok(()) => info!(
            "captured {} (gw {}) from {} to {} before enslavement",
            addrs.join(" "),
            if gw.is_empty() { "none" } else { &gw },
            uplink,
            UPLINK_SNAPSHOT_PATH
        ),
        Err(e) => warn!("failed to write {}: {}", UPLINK_SNAPSHOT_PATH, e),
    }
}

fn find_socket_path() -> String {
    let candidates = [
        "/usr/local/var/run/openvswitch/db.sock",
        "/run/openvswitch/db.sock",
        "/var/run/openvswitch/db.sock",
    ];
    candidates
        .iter()
        .find(|p| Path::new(p).exists())
        .unwrap_or(&"/usr/local/var/run/openvswitch/db.sock")
        .to_string()
}

/// Find vswitchd unixctl control socket (matches ovs-vswitchd.PID.ctl).
fn find_vswitchd_ctl() -> Option<String> {
    let dir = "/usr/local/var/run/openvswitch";
    std::fs::read_dir(dir).ok()?.find_map(|entry| {
        let e = entry.ok()?;
        let name = e.file_name().into_string().ok()?;
        if name.starts_with("ovs-vswitchd.") && name.ends_with(".ctl") {
            Some(format!("{}/{}", dir, name))
        } else {
            None
        }
    })
}

/// Send the `exit` command to vswitchd via its unixctl JSON-RPC socket.
/// vswitchd will shut down gracefully on receiving this.
async fn vswitchd_send_exit(ctl_path: &str) -> Result<()> {
    let addr: Address = format!("unix:{}", ctl_path)
        .parse()
        .context("parse vswitchd ctl address")?;
    let stream = Stream::connect(&addr)
        .await
        .context("connect to vswitchd unixctl")?;
    let mut conn = Connection::new(stream);

    // vswitchd unixctl speaks JSON-RPC: {"method":"exit","params":[],"id":0}
    let _ = conn.transact("exit", json!([])).await;
    drop(conn);
    info!("sent exit to vswitchd via unixctl {}", ctl_path);
    Ok(())
}

/// Stop vswitchd:
///   1. `service6 stop <name>` — the only sanctioned host service-manager
///      entry point (raw `runsv`/`runsvdir` are policy violations outside boot
///      supervisors and human console recovery)
///   2. Fall back to a graceful `exit` via unixctl if the wrapper failed
///   3. Wait for the unixctl socket to vanish (vswitchd removes it on exit)
///      and for kernel OVS interfaces to disappear
async fn stop_vswitchd(svc: &str, bridge: &str) -> Result<()> {
    let stopped = Command::new(op_core::runit::SV_BIN)
        .args(["stop", svc])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !stopped {
        warn!("sv stop {} failed — falling back to unixctl exit", svc);
        if let Some(ctl) = find_vswitchd_ctl() {
            let _ = vswitchd_send_exit(&ctl).await;
        }
    }

    // Wait for vswitchd to fully exit: its unixctl socket disappears with it.
    info!("waiting for vswitchd to exit...");
    for i in 0..50 {
        std::thread::sleep(Duration::from_millis(200));
        if find_vswitchd_ctl().is_none() {
            info!("vswitchd exited after {}ms", (i + 1) * 200);
            break;
        }
    }
    if let Some(ctl) = find_vswitchd_ctl() {
        // The supervisor did not take it down in time — last graceful nudge.
        let _ = vswitchd_send_exit(&ctl).await;
        std::thread::sleep(Duration::from_millis(500));
    }

    clear_kernel_datapath(bridge);

    // Wait for kernel OVS interfaces to be cleaned up by the kernel module.
    // When vswitchd exits it releases the kernel datapath; the kernel then
    // GC-removes the ovsbr0 interface (async).  A new vswitchd trying to
    // create an internal port named "ovsbr0" will get EINVAL if this
    // interface still exists in the kernel.
    let sysfs_path = format!("/sys/class/net/{}", bridge);
    info!("waiting for kernel interface {} to disappear...", bridge);
    for i in 0..50 {
        if !Path::new(&sysfs_path).exists() {
            info!("kernel interface {} is gone", bridge);
            break;
        }
        if i == 10 || i == 25 {
            clear_kernel_datapath(bridge);
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    if Path::new(&sysfs_path).exists() {
        bail!(
            "kernel interface {} still exists after stopping vswitchd; \
             refusing recreate until the stale interface is gone",
            bridge
        );
    }

    Ok(())
}

fn clear_kernel_datapath(bridge: &str) {
    info!("clearing stale kernel datapath state");
    let _ = Command::new("ovs-dpctl")
        .args(["del-dp", "system@ovs-system"])
        .status();
    let _ = Command::new("ip").args(["link", "del", bridge]).status();
}

/// Start vswitchd through the sanctioned `sv` wrapper.
fn start_vswitchd(svc: &str) {
    info!("starting vswitchd via sv: {}", svc);
    let _ = Command::new(op_core::runit::SV_BIN)
        .args(["start", svc])
        .status();
}

/// Wait for vswitchd to fully start: poll OVSDB until bridge row appears AND
/// the vswitchd unixctl socket is present (confirms vswitchd is fully up).
async fn wait_for_vswitchd_up(socket: &str, bridge: &str) -> Result<()> {
    let addr = format!("unix:{}", socket);
    info!("waiting for vswitchd to come up with bridge {}...", bridge);

    for i in 0..120 {
        tokio::time::sleep(Duration::from_millis(500)).await;

        // vswitchd control socket must be present (vswitchd fully initialized)
        let ctl_present = find_vswitchd_ctl().is_some();
        if !ctl_present {
            continue;
        }

        // Bridge must be in OVSDB
        let Ok(client) = Client::connect(&addr).await else {
            continue;
        };
        let found = client
            .idl()
            .rows("Bridge")
            .any(|r| r.get_string("name") == Some(bridge));
        if found {
            info!("vswitchd up after {}ms", (i + 1) * 500);
            return Ok(());
        }
    }
    bail!("vswitchd did not come up within 60s")
}

/// Wait for the OVSDB socket to appear.
async fn wait_for_socket(path: &str) -> Result<()> {
    for _ in 0..40 {
        if Path::new(path).exists() {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    bail!("OVSDB socket {} did not appear within 10s", path)
}

/// Extract UUIDs from OVSDB set/single value.
fn extract_uuids(val: &Value) -> Vec<Uuid> {
    let mut out = Vec::new();
    if let Some(arr) = val.as_array() {
        match arr.first().and_then(|v| v.as_str()) {
            Some("uuid") => {
                if let Some(s) = arr.get(1).and_then(|v| v.as_str()) {
                    if let Ok(u) = s.parse() {
                        out.push(u);
                    }
                }
            }
            Some("set") => {
                if let Some(items) = arr.get(1).and_then(|v| v.as_array()) {
                    for item in items {
                        out.extend(extract_uuids(item));
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// Commit a transaction, bailing if it fails.
async fn commit(client: &mut Client, txn: &mut Transaction, label: &str) -> Result<()> {
    let ok = client
        .commit(txn)
        .await
        .with_context(|| label.to_string())?;
    if !ok {
        bail!("transaction '{}' failed in OVSDB", label);
    }
    Ok(())
}

/// Delete the ovsbr0 bridge (and its ports/interfaces) from OVSDB.
async fn delete_bridge(client: &mut Client, bridge: &str) -> Result<()> {
    let br_row = client
        .idl()
        .rows("Bridge")
        .find(|r| r.get_string("name") == Some(bridge))
        .map(|r| (r.uuid, r.get("ports").cloned()));

    let Some((br_uuid, ports_val)) = br_row else {
        info!("bridge {} not in OVSDB", bridge);
        return Ok(());
    };

    let port_uuids: Vec<Uuid> = ports_val.as_ref().map(extract_uuids).unwrap_or_default();
    let mut iface_uuids: Vec<Uuid> = Vec::new();
    for p_uuid in &port_uuids {
        if let Some(row) = client.idl().row("Port", p_uuid) {
            if let Some(ifaces) = row.get("interfaces") {
                iface_uuids.extend(extract_uuids(ifaces));
            }
        }
    }

    info!(
        "deleting bridge {} ({} ports, {} ifaces)",
        bridge,
        port_uuids.len(),
        iface_uuids.len()
    );
    let mut txn = Transaction::new("Open_vSwitch");
    txn.delete_bridge_uuid(br_uuid, &port_uuids, &iface_uuids);
    commit(client, &mut txn, "delete bridge").await
}

/// Purge orphaned Interface/Port/Bridge rows by name (handles TOCTOU races).
async fn purge_by_name(client: &mut Client, name: &str) -> Result<()> {
    warn!(
        "purging any stale Bridge/Port/Interface rows named '{}'",
        name
    );

    let bridge_uuids: Vec<Uuid> = client
        .idl()
        .rows("Bridge")
        .filter(|r| r.get_string("name") == Some(name))
        .map(|r| r.uuid)
        .collect();
    for br_uuid in bridge_uuids {
        let mut port_uuids = Vec::new();
        let mut iface_uuids = Vec::new();
        if let Some(row) = client.idl().row("Bridge", &br_uuid) {
            if let Some(ports) = row.get("ports") {
                port_uuids.extend(extract_uuids(ports));
            }
        }
        for p_uuid in &port_uuids {
            if let Some(row) = client.idl().row("Port", p_uuid) {
                if let Some(ifaces) = row.get("interfaces") {
                    iface_uuids.extend(extract_uuids(ifaces));
                }
            }
        }
        let mut txn = Transaction::new("Open_vSwitch");
        txn.delete_bridge_uuid(br_uuid, &port_uuids, &iface_uuids);
        let _ = client.commit(&mut txn).await;
    }

    let mut port_txn = Transaction::new("Open_vSwitch");
    port_txn.delete_by_name("Port", name);
    let _ = client.commit(&mut port_txn).await;

    let mut iface_txn = Transaction::new("Open_vSwitch");
    iface_txn.delete_by_name("Interface", name);
    let _ = client.commit(&mut iface_txn).await;
    Ok(())
}

/// Create ovsbr0 with datapath_type=system.
///
/// All ports ride in the SAME atomic transact (RFC 7047, official
/// Open_vSwitch schema): vswitchd must first read the bridge and its ports
/// together or uplink capture does not start correctly.  Enslavement is
/// never a second transaction on the create path.
///
/// `mac` (the uplink's real MAC) is pinned on `public_port`'s Interface row
/// via other_config.hwaddr.  The Bridge row NEVER carries hwaddr: the bridge
/// device must keep a random MAC, because the public IP lives on
/// `public_port`, and the provider filters on the uplink MAC — if the bridge
/// device held that MAC and the IP later moved, inbound would blackhole.
async fn create_bridge_system(
    client: &mut Client,
    bridge: &str,
    fail_mode: &str,
    mac: &str,
    extra_ports: &[&str],
    public_port: &str,
    internal_ports: &[String],
) -> Result<()> {
    info!(
        "creating bridge {} with datapath_type=system fail_mode={} extra_ports={:?} \
         public_port={} internal_ports={:?}",
        bridge, fail_mode, extra_ports, public_port, internal_ports
    );
    purge_by_name(client, bridge).await?;

    let mut txn = Transaction::new("Open_vSwitch");

    let iface_ref = txn.insert("Interface", json!({"name": bridge, "type": "internal"}));
    let mut port_refs = vec![txn.insert(
        "Port",
        json!({"name": bridge, "interfaces": iface_ref.to_json()}),
    )];
    for name in extra_ports {
        let extra_iface_ref = txn.insert("Interface", json!({"name": name, "type": ""}));
        port_refs.push(txn.insert(
            "Port",
            json!({"name": name, "interfaces": extra_iface_ref.to_json()}),
        ));
    }
    // public port: internal, carries the uplink MAC (and later the public IP)
    let pub_iface_ref = txn.insert(
        "Interface",
        json!({
            "name": public_port,
            "type": "internal",
            "other_config": ["map", [["hwaddr", mac]]]
        }),
    );
    port_refs.push(txn.insert(
        "Port",
        json!({"name": public_port, "interfaces": pub_iface_ref.to_json()}),
    ));
    for name in internal_ports {
        let int_iface_ref = txn.insert("Interface", json!({"name": name, "type": "internal"}));
        port_refs.push(txn.insert(
            "Port",
            json!({"name": name, "interfaces": int_iface_ref.to_json()}),
        ));
    }
    let ports_json = if port_refs.len() == 1 {
        port_refs[0].to_json()
    } else {
        json!([
            "set",
            port_refs.iter().map(|p| p.to_json()).collect::<Vec<_>>()
        ])
    };
    let bridge_ref = txn.insert(
        "Bridge",
        json!({
            "name": bridge,
            "datapath_type": "system",
            "fail_mode": fail_mode,
            "ports": ports_json
        }),
    );
    txn.mutate_where(
        "Open_vSwitch",
        json!([]),
        vec![json!(["bridges", "insert", bridge_ref.to_json()])],
    );

    commit(client, &mut txn, "create bridge system").await
}

/// Add a port to the bridge if not already present.
async fn add_port(client: &mut Client, bridge: &str, port_name: &str) -> Result<()> {
    let bridge_ports: Vec<Uuid> = client
        .idl()
        .rows("Bridge")
        .find(|r| r.get_string("name") == Some(bridge))
        .and_then(|r| r.get("ports"))
        .map(extract_uuids)
        .unwrap_or_default();

    let already = bridge_ports.iter().any(|uuid| {
        client
            .idl()
            .row("Port", uuid)
            .and_then(|p| p.get_string("name"))
            == Some(port_name)
    });
    if already {
        info!("port {} already in bridge {}", port_name, bridge);
        return Ok(());
    }

    info!("adding port {} to bridge {}", port_name, bridge);
    let mut txn = Transaction::new("Open_vSwitch");
    let iface_ref = txn.insert("Interface", json!({"name": port_name, "type": ""}));
    let port_ref = txn.insert(
        "Port",
        json!({"name": port_name, "interfaces": iface_ref.to_json()}),
    );
    txn.mutate_by_name(
        "Bridge",
        bridge,
        vec![json!(["ports", "insert", port_ref.to_json()])],
    );
    commit(client, &mut txn, "add port").await
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::from_default_env()
                .add_directive("op_ovsbr0_setup=info".parse()?)
                .add_directive("rovs_ovsdb=warn".parse()?),
        )
        .init();

    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "-h" || arg == "--help") {
        print_help();
        return Ok(());
    }
    let seed_only = args
        .iter()
        .any(|arg| arg == "--seed-only" || arg == "seed-only");

    let cfg = Config::from_env();
    let addr = format!("unix:{}", cfg.ovsdb_socket);

    // ── 1. Wait for OVSDB socket ──────────────────────────────────────────────
    info!("waiting for OVSDB socket: {}", cfg.ovsdb_socket);
    wait_for_socket(&cfg.ovsdb_socket).await?;

    // ── 2. Connect and read current bridge state ──────────────────────────────
    let mut client = Client::connect(&addr)
        .await
        .context("connect to ovsdb-server")?;
    info!("connected to OVSDB");

    let br_state = client
        .idl()
        .rows("Bridge")
        .find(|r| r.get_string("name") == Some(&cfg.bridge))
        .map(|r| {
            (
                r.uuid,
                r.get_string("datapath_type").unwrap_or("").to_string(),
            )
        });

    match &br_state {
        Some((u, dt)) => info!("bridge {} uuid={} datapath_type={}", cfg.bridge, u, dt),
        None => info!("bridge {} not found in OVSDB", cfg.bridge),
    }

    // The physical uplink (when configured) rides in the same create
    // transact — see create_bridge_system.
    let mut seed_ports: Vec<&str> = Vec::new();
    if !cfg.uplink.is_empty() {
        seed_ports.push(cfg.uplink.as_str());
    }
    let shared_mac = resolve_shared_mac(&cfg);

    if seed_only {
        info!("seed-only mode: writing system OVSDB rows without starting vswitchd");
        // Must happen before create_bridge_system: once vswitchd (started by
        // the caller right after this process exits) reads this seed and
        // captures the uplink, the kernel strips its IPs immediately.
        snapshot_uplink_before_enslavement(&cfg.uplink);
        clear_kernel_datapath(&cfg.bridge);
        delete_bridge(&mut client, &cfg.bridge).await?;
        tokio::time::sleep(Duration::from_millis(300)).await;
        client = Client::connect(&addr)
            .await
            .context("reconnect after seed delete")?;
        create_bridge_system(
            &mut client,
            &cfg.bridge,
            &cfg.fail_mode,
            &shared_mac,
            &seed_ports,
            &cfg.public_port,
            &cfg.internal_ports,
        )
        .await?;
        info!("seed-only complete: {} datapath_type=system", cfg.bridge);
        return Ok(());
    }

    let needs_recreate = match &br_state {
        None => true,
        Some((_, dt)) => dt != "system",
    };

    // ── 3. If wrong datapath: stop vswitchd, rewrite OVSDB, restart ──────────
    if needs_recreate {
        warn!("bridge has wrong/missing datapath_type — stopping vswitchd");

        // Same reasoning as the seed-only path: capture before vswitchd
        // (re)captures the uplink and the kernel strips its IPs.
        snapshot_uplink_before_enslavement(&cfg.uplink);

        stop_vswitchd(&cfg.vswitchd_svc, &cfg.bridge).await?;

        // Reconnect (ovsdb-server stays up while vswitchd is down)
        client = Client::connect(&addr)
            .await
            .context("reconnect after stop")?;

        // Delete bridge (separate tx — OVS 3.x del+add in one tx silently rolls back)
        delete_bridge(&mut client, &cfg.bridge).await?;
        tokio::time::sleep(Duration::from_millis(300)).await;

        // Fresh IDL for create
        client = Client::connect(&addr)
            .await
            .context("reconnect after delete")?;

        // Create with system datapath (uplink enslaved in the same transact)
        create_bridge_system(
            &mut client,
            &cfg.bridge,
            &cfg.fail_mode,
            &shared_mac,
            &seed_ports,
            &cfg.public_port,
            &cfg.internal_ports,
        )
        .await?;
        info!("OVSDB updated: datapath_type=system");

        // Start vswitchd — it reads system from OVSDB and initializes the kernel dpif
        start_vswitchd(&cfg.vswitchd_svc);

        // Wait for vswitchd to be genuinely up (ctl socket + bridge in OVSDB)
        wait_for_vswitchd_up(&cfg.ovsdb_socket, &cfg.bridge).await?;

        // Verify datapath_type in live OVSDB (vswitchd may have reverted)
        client = Client::connect(&addr)
            .await
            .context("reconnect after restart")?;
        let live_dt = client
            .idl()
            .rows("Bridge")
            .find(|r| r.get_string("name") == Some(&cfg.bridge))
            .and_then(|r| r.get_string("datapath_type"))
            .unwrap_or("")
            .to_string();

        if live_dt != "system" {
            bail!(
                "vswitchd reset datapath_type to '{}' — expected system. \
                 Check /run/uncaught-logs/current for WARN messages from bridge module.",
                live_dt
            );
        }
        info!("confirmed: bridge {} datapath_type=system", cfg.bridge);
    } else {
        info!("bridge {} already system — no restart needed", cfg.bridge);
    }

    // ── 4. Idempotent port adds for a pre-existing bridge ────────────────────
    // (On the create path these are no-ops: the ports were enslaved inside
    // the create transact.)
    if !cfg.uplink.is_empty() {
        add_port(&mut client, &cfg.bridge, &cfg.uplink).await?;
    }

    // ── 5. Bring ports up (ip link is a network utility, not an OVS tool) ────
    if !cfg.uplink.is_empty() {
        let _ = Command::new("ip")
            .args(["link", "set", &cfg.uplink, "up"])
            .status();
    }
    info!("op-ovsbr0-setup: done");
    Ok(())
}

fn print_help() {
    println!(
        "Usage: op-ovsbr0-setup [--seed-only]\n\n\
         Ensures ovsbr0 exists with datapath_type=system, using rovs for OVSDB \
         changes and the service6 wrapper for ovs-vswitchd control.\n\n\
         Modes:\n\
           --seed-only Write OVSDB system rows and exit without starting vswitchd\n\n\
         Environment:\n\
           BRIDGE       bridge name (default: ovsbr0)\n\
           UPLINK       physical NIC enslaved in the same create transact\n\
                        (optional; atomic with bridge creation so capture\n\
                        starts correctly)\n\
            FAIL_MODE    bridge fail mode (default: standalone)\n\
            PUBLIC_PORT  internal port for the public IP (default: pub0; the\n\
                         uplink MAC is pinned here, never on the bridge)\n\
            INTERNAL_PORTS extra internal ports (default: \"svc0 grpc\")\n\
            SHARED_MAC   PUBLIC_PORT MAC override (default: UPLINK's own MAC)\n\
            OVSDB_SOCKET OVSDB socket path\n\
            VSWITCHD_SVC s6 service name for service6 (default: ovs-vswitchd)"
    );
}
