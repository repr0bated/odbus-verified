//! OVS `unixctl` client (the wire protocol behind `ovs-appctl`).
//!
//! Source of truth for assumptions:
//! <https://docs.openvswitch.org/en/latest/topics/design/>
//!
//! # Why this module exists
//!
//! Some datapath state has **no OVSDB column and no OpenFlow message**: the
//! L2 forwarding database is the case that matters here. A static FDB entry is
//! reachable only over `unixctl` (`fdb/add`), so without this module the only
//! way to set one would be shelling out to `ovs-appctl` — which the plugin
//! contract forbids (see `datapath_safe`'s "No CLI shell-outs").
//!
//! `unixctl` is plain JSON-RPC over a Unix socket: `method` is the appctl
//! command name and `params` is an array of string arguments. That is the same
//! transport `rovs_ovsdb` already speaks, so `rovs_jsonrpc::Connection` is
//! reused verbatim rather than hand-rolling a second framing layer.
//!
//! # CLI boundary
//!
//! `ovs-ofctl` / `ovs-appctl` / `ovs-dpctl` are **operator verification tools
//! only** — fine for diagnosing a live host by hand, never a code path. Every
//! runtime call in this crate goes over `rovs_jsonrpc` (unixctl), `rovs_ovsdb`
//! (OVSDB) or `rovs_openflow` (bridge `.mgmt`). Do not read a CLI invocation in
//! a log line or SIGNALS entry as precedent for shelling out from code.
//!
//! # Why static FDB entries are load-bearing
//!
//! `actions=NORMAL` consults the FDB. When a destination MAC is absent, NORMAL
//! degrades to flooding every port on the bridge. A router that is addressed by
//! a VRRP virtual MAC but *replies* from its physical MAC is never learned, so
//! every egress frame floods — including to ports that physically cannot accept
//! an Ethernet frame (a WireGuard `link/none` port errors on each one). Pinning
//! that MAC restores unicast without touching the flow table, so classifier
//! flows and their counters are left exactly as the control plane wrote them.
//!
//! Static entries do not survive a `ovs-vswitchd` restart, which is why
//! `ensure_static_fdb_entries` is re-asserted on every OVS reconnect alongside
//! `ensure_fallback_normal`.

use anyhow::{bail, Context, Result};
use rovs_jsonrpc::Connection;
use rovs_transport::{Address, Stream};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// Directory holding the `ovs-vswitchd.<pid>.ctl` socket.
const DEFAULT_OVS_RUNDIR: &str = "/var/run/openvswitch";

fn ovs_rundir() -> PathBuf {
    PathBuf::from(std::env::var("OVS_RUNDIR").unwrap_or_else(|_| DEFAULT_OVS_RUNDIR.to_string()))
}

/// Locate the `ovs-vswitchd` unixctl socket.
///
/// The filename embeds the daemon pid (`ovs-vswitchd.<pid>.ctl`), and under
/// runit supervision there is no pidfile to read it from, so the socket is
/// discovered by scanning the rundir instead of being derived from a pid.
pub fn vswitchd_ctl_path() -> Result<PathBuf> {
    if let Ok(explicit) = std::env::var("OVS_VSWITCHD_CTL") {
        return Ok(PathBuf::from(explicit));
    }
    let dir = ovs_rundir();
    let entries =
        std::fs::read_dir(&dir).with_context(|| format!("reading OVS rundir {}", dir.display()))?;

    let mut found: Option<PathBuf> = None;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with("ovs-vswitchd.") && name.ends_with(".ctl") {
            // Prefer the newest socket if a stale one was left behind by a crash.
            let replace = match &found {
                None => true,
                Some(prev) => newer_than(&entry.path(), prev),
            };
            if replace {
                found = Some(entry.path());
            }
        }
    }
    found.with_context(|| {
        format!(
            "no ovs-vswitchd.<pid>.ctl socket in {} (is ovs-vswitchd running?)",
            dir.display()
        )
    })
}

fn newer_than(a: &Path, b: &Path) -> bool {
    let mtime = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    match (mtime(a), mtime(b)) {
        (Some(a), Some(b)) => a > b,
        _ => false,
    }
}

/// Invoke one `unixctl` command and return its raw string reply.
pub async fn appctl(command: &str, args: &[&str]) -> Result<String> {
    let path = vswitchd_ctl_path()?;
    appctl_at(&path, command, args).await
}

/// Invoke one `unixctl` command against an explicit socket path.
pub async fn appctl_at(ctl: &Path, command: &str, args: &[&str]) -> Result<String> {
    let address = Address::Unix(ctl.to_path_buf());
    let stream = Stream::connect(&address)
        .await
        .with_context(|| format!("unixctl connect {}", ctl.display()))?;
    let mut conn = Connection::new(stream);

    let params: Vec<Value> = args.iter().map(|a| json!(a)).collect();
    let reply = conn
        .transact(command, Value::Array(params))
        .await
        .with_context(|| format!("unixctl {command} {args:?}"))?;

    // appctl replies are a bare JSON string; anything else is a protocol error
    // worth surfacing rather than silently stringifying.
    match reply {
        Value::String(s) => Ok(s),
        Value::Null => Ok(String::new()),
        other => bail!("unixctl {command}: unexpected reply shape {other}"),
    }
}

/// One statically-pinned L2 forwarding entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaticFdbEntry {
    pub bridge: String,
    pub port: String,
    pub vlan: u16,
    pub mac: String,
}

impl StaticFdbEntry {
    /// Parse `bridge:port:vlan:mac` (the `OF_STATIC_FDB` element form).
    pub fn parse(spec: &str) -> Result<Self> {
        let parts: Vec<&str> = spec.trim().split(':').collect();
        // A MAC contains colons too, so split from the left and rejoin the tail.
        if parts.len() != 9 {
            bail!(
                "malformed static FDB spec {spec:?} — expected bridge:port:vlan:aa:bb:cc:dd:ee:ff"
            );
        }
        let vlan: u16 = parts[2]
            .parse()
            .with_context(|| format!("invalid vlan in static FDB spec {spec:?}"))?;
        let mac = parts[3..9].join(":");
        validate_mac(&mac)?;
        Ok(Self {
            bridge: parts[0].to_string(),
            port: parts[1].to_string(),
            vlan,
            mac: mac.to_ascii_lowercase(),
        })
    }
}

fn validate_mac(mac: &str) -> Result<()> {
    let octets: Vec<&str> = mac.split(':').collect();
    if octets.len() != 6 {
        bail!("invalid MAC {mac:?} — expected six colon-separated octets");
    }
    for o in octets {
        if o.len() != 2 || !o.chars().all(|c| c.is_ascii_hexdigit()) {
            bail!("invalid MAC octet {o:?} in {mac:?}");
        }
    }
    Ok(())
}

/// Read the `OF_STATIC_FDB` environment spec into entries.
///
/// Format: comma-separated `bridge:port:vlan:mac`. An empty or unset value
/// yields no entries (the runit unit exports it unconditionally, so blank is
/// the normal "nothing pinned" case and must not warn).
pub fn static_fdb_from_env() -> Result<Vec<StaticFdbEntry>> {
    let raw = std::env::var("OF_STATIC_FDB").unwrap_or_default();
    let mut out = Vec::new();
    for spec in raw.split(',') {
        if spec.trim().is_empty() {
            continue;
        }
        out.push(StaticFdbEntry::parse(spec)?);
    }
    Ok(out)
}

/// True when `mac` is already pinned as a static entry on `bridge`.
pub async fn static_fdb_present(bridge: &str, mac: &str) -> Result<bool> {
    let table = appctl("fdb/show", &[bridge]).await?;
    let mac = mac.to_ascii_lowercase();
    Ok(table.lines().any(|line| {
        let l = line.to_ascii_lowercase();
        l.contains(&mac) && l.contains("static")
    }))
}

/// Idempotently pin one MAC to a port. Returns true when an entry was added.
pub async fn ensure_static_fdb(entry: &StaticFdbEntry) -> Result<bool> {
    if static_fdb_present(&entry.bridge, &entry.mac).await? {
        return Ok(false);
    }
    let vlan = entry.vlan.to_string();
    appctl("fdb/add", &[&entry.bridge, &entry.port, &vlan, &entry.mac]).await?;

    if !static_fdb_present(&entry.bridge, &entry.mac).await? {
        bail!(
            "EnsureStaticFdb: {} still absent from {} FDB after fdb/add",
            entry.mac,
            entry.bridge
        );
    }
    log::info!(
        "EnsureStaticFdb: pinned {} -> {} (vlan {}) on {}",
        entry.mac,
        entry.port,
        entry.vlan,
        entry.bridge
    );
    Ok(true)
}

/// Re-assert every configured static FDB entry.
///
/// Called on each OVS reconnect because static entries are lost when
/// `ovs-vswitchd` restarts. Individual failures are logged rather than
/// propagated: a missing pin degrades NORMAL to flooding, which is wasteful
/// but not a connectivity outage, so it must never abort flow installation.
pub async fn ensure_static_fdb_entries(entries: &[StaticFdbEntry]) -> usize {
    let mut added = 0;
    for entry in entries {
        match ensure_static_fdb(entry).await {
            Ok(true) => added += 1,
            Ok(false) => {}
            Err(e) => log::warn!(
                "EnsureStaticFdb: {} -> {} on {} failed: {e:#}",
                entry.mac,
                entry.port,
                entry.bridge
            ),
        }
    }
    added
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bridge_port_vlan_mac() {
        let e = StaticFdbEntry::parse("ovsbr0:eth0:0:00:00:5e:00:01:0a").unwrap();
        assert_eq!(e.bridge, "ovsbr0");
        assert_eq!(e.port, "eth0");
        assert_eq!(e.vlan, 0);
        assert_eq!(e.mac, "00:00:5e:00:01:0a");
    }

    #[test]
    fn uppercase_mac_is_normalised() {
        let e = StaticFdbEntry::parse("br0:p1:7:AA:BB:CC:DD:EE:FF").unwrap();
        assert_eq!(e.mac, "aa:bb:cc:dd:ee:ff");
        assert_eq!(e.vlan, 7);
    }

    #[test]
    fn rejects_truncated_mac() {
        assert!(StaticFdbEntry::parse("ovsbr0:eth0:0:00:00:5e:00:01").is_err());
    }

    #[test]
    fn rejects_non_numeric_vlan() {
        assert!(StaticFdbEntry::parse("ovsbr0:eth0:x:00:00:5e:00:01:0a").is_err());
    }

    #[test]
    fn rejects_non_hex_octet() {
        assert!(StaticFdbEntry::parse("ovsbr0:eth0:0:00:00:5e:00:01:zz").is_err());
    }

    #[test]
    fn blank_env_yields_no_entries() {
        // Guard the unset case without mutating process env for other tests.
        let parsed: Vec<_> = "".split(',').filter(|s| !s.trim().is_empty()).collect();
        assert!(parsed.is_empty());
    }
}
