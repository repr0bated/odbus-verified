//! Safe OVS datapath attach helpers (plugin contract).
//!
//! Source of truth for assumptions:
//! <https://docs.openvswitch.org/en/latest/topics/design/>
//!
//! # Why attach without fallback black-holes the host
//!
//! ## Asynchronous messages
//!
//! A **service** controller never receives async messages unless it raises
//! `miss_send_len` from the service default of **0** via:
//! - `OFPT_SET_CONFIG` with nonzero `miss_send_len`, or
//! - any `NXT_SET_ASYNC_CONFIG` (side effect: `miss_send_len` → 128).
//!
//! `OFPT_FLOW_REMOVED` is generated only if the removed flow had
//! `OFPFF_SEND_FLOW_REM`.
//!
//! `OFPT_PACKET_IN` / `NXT_PACKET_IN` go only to the connection with the
//! correct controller ID (`NXAST_CONTROLLER` uses the action's ID; table-miss
//! and other reasons use **controller ID 0**). Secondary role suppresses
//! `OFPR_NO_MATCH` by default.
//!
//! Therefore host L3 (SSH/pub0) must **never** depend on
//! `OFPR_NO_MATCH` → PACKET_IN → controller flow install.
//!
//! ## `OFPT_FLOW_MOD` atomicity (and the delete-all race)
//!
//! OVS applies each `flow_mod` atomically, but **separate** `flow_mod`s are
//! not one transaction (OF1.4 bundles would be). `delete-all` then
//! `add NORMAL` is two mods: packets can observe an empty table between them.
//! Datapath cache revalidation is also deferred (~1s), so connectivity can
//! look fine briefly, then die. Contract: always keep `priority=0,actions=NORMAL`
//! via ofctl **before** `set-controller`, re-install NORMAL immediately after
//! any wipe, and roll back if health check fails.
//!
//! ## In-band control principle
//!
//! "An OpenFlow switch must recognize and switch control traffic without
//! involving the OpenFlow controller." In-band rules must work even while
//! connected, and must override controller flows — a controller "last resort
//! send-to-controller" otherwise isolates itself. Our priority=0 NORMAL is the
//! same principle for host/management traffic when the OF channel is local to
//! the bridge (out-of-band TCP to `10.200.0.1:6653`, but host L3 still traverses
//! the bridge pipeline).
//!
//! ## Echo for liveness
//!
//! Prefer `OFPT_ECHO_REQUEST` over bare TCP timeouts to detect a hung OF
//! session (kernel TCP can take many minutes).
//!
//! # Contract methods
//! - [`ensure_fallback_normal`]
//! - [`set_fail_mode`]
//! - [`del_controller`] / [`set_controller`]
//! - [`get_datapath_health`]
//! - [`attach_controller_safe`] (orchestrated; auto-rollback)

//! ## Flow cookies
//!
//! Tag the NORMAL fallback with a stable cookie (`FALLBACK_COOKIE`) and
//! controller-managed flows with `MANAGED_COOKIE`. Prefer deleting by managed
//! cookie instead of delete-all so the fallback survives reconnect wipes
//! (OF1.3 cookie match on DELETE).
//!
//! ## Tables
//!
//! Controllers must use tables 0–127 only; 128+ are reserved for the switch.
//!
//! ## Connection mode
//!
//! OF endpoint `tcp:10.200.0.1:6653` is on the bridge IP → keep
//! `connection_mode=in-band` (default). Do **not** set
//! `other_config:disable-in-band=true`. Host SSH still needs NORMAL; in-band
//! hidden flows only cover controller/manager remotes.
//!
//! ## Action reproduction
//!
//! Do not byte-compare dumped actions to expected bytes — OVS normalizes
//! instruction order and drops empty Apply/Write-Actions.
//!
use anyhow::{bail, Context, Result};
use serde::Serialize;
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;

/// Cookie for priority=0 NORMAL host-safety fallback (never delete-all this).
pub const FALLBACK_COOKIE: u64 = 0x3344_4348_0000_0001; // "3DCH"+1
/// Cookie for controller-installed pair / schema flows.
pub const MANAGED_COOKIE: u64 = 0x3344_4348_0000_0002;
const FALLBACK_PRIORITY: u16 = 0;
/// Datapath cache revalidation window (OVS design: usually within ~1s).
const DATAPATH_SETTLE: Duration = Duration::from_millis(1200);

fn fallback_flow_spec() -> String {
    format!(
        "cookie={FALLBACK_COOKIE:#x}/-1,priority={FALLBACK_PRIORITY},actions=NORMAL"
    )
}

async fn run_capture(cmd: &str, args: &[&str]) -> Result<(i32, String, String)> {
    let out = Command::new(cmd)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .with_context(|| format!("failed to spawn {cmd}"))?;
    let code = out.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    Ok((code, stdout, stderr))
}

async fn run_ok(cmd: &str, args: &[&str]) -> Result<String> {
    let (code, stdout, stderr) = run_capture(cmd, args).await?;
    if code != 0 {
        bail!("{cmd} {:?} failed (exit {code}): {stderr}{stdout}", args);
    }
    Ok(stdout)
}

/// Idempotently install cookied priority=0 NORMAL and verify it is present.
pub async fn ensure_fallback_normal(bridge: &str) -> Result<()> {
    let flow = fallback_flow_spec();
    let (_c, _o, _e) = run_capture("ovs-ofctl", &["add-flow", bridge, &flow]).await?;
    if !fallback_present(bridge).await? {
        bail!("EnsureFallbackNormal: NORMAL fallback not present on {bridge} after add-flow");
    }
    log::info!("EnsureFallbackNormal: {flow} present on {bridge}");
    Ok(())
}

pub async fn fallback_present(bridge: &str) -> Result<bool> {
    let dump = run_ok("ovs-ofctl", &["dump-flows", bridge]).await?;
    let cookie_hex = format!("{FALLBACK_COOKIE:x}");
    Ok(dump.lines().any(|l| {
        let lower = l.to_ascii_lowercase();
        lower.contains("priority=0")
            && lower.contains("actions=normal")
            && lower.contains(&cookie_hex)
    }))
        let lower = l.to_ascii_lowercase();
        lower.contains("priority=0") && lower.contains("actions=normal")
    }))
}

pub async fn set_fail_mode(bridge: &str, mode: &str) -> Result<()> {
    match mode {
        "standalone" | "secure" => {}
        other => bail!("fail_mode must be standalone|secure, got {other}"),
    }
    run_ok(
        "ovs-vsctl",
        &["--no-wait", "set", "Bridge", bridge, &format!("fail_mode={mode}")],
    )
    .await?;
    log::info!("SetFailMode: {bridge} fail_mode={mode}");
    Ok(())
}

/// Keep in-band for bridge-local OF endpoints (design: hidden remotes).
pub async fn ensure_controller_in_band(bridge: &str) -> Result<()> {
    // After set-controller, Controller rows exist; set connection_mode on each.
    let _ = run_capture(
        "ovs-vsctl",
        &[
            "--no-wait",
            "set",
            "Controller",
            &format!("{bridge}"), // may fail if name form unsupported
            "connection_mode=in-band",
        ],
    )
    .await;
    // Portable: find Controller UUIDs referenced by the bridge
    let uuids = run_ok(
        "ovs-vsctl",
        &["--bare", "--columns=_uuid", "find", "Controller", &format!("target!=\"\"")],
    )
    .await
    .unwrap_or_default();
    for uuid in uuids.lines().map(str::trim).filter(|s| !s.is_empty()) {
        let _ = run_capture(
            "ovs-vsctl",
            &["--no-wait", "set", "Controller", uuid, "connection_mode=in-band"],
        )
        .await;
    }
    // Never disable in-band globally on the bridge
    let _ = run_capture(
        "ovs-vsctl",
        &[
            "--no-wait",
            "remove",
            "Bridge",
            bridge,
            "other_config",
            "disable-in-band",
        ],
    )
    .await;
    Ok(())
}

pub async fn del_controller(bridge: &str) -> Result<()> {
    run_ok("ovs-vsctl", &["--no-wait", "del-controller", bridge]).await?;
    log::info!("DelController: {bridge}");
    Ok(())
}

pub async fn set_controller(bridge: &str, endpoint: &str) -> Result<()> {
    ensure_fallback_normal(bridge).await?;
    let ep = if endpoint.starts_with("tcp:") {
        endpoint.to_string()
    } else {
        format!("tcp:{endpoint}")
    };
    run_ok(
        "ovs-vsctl",
        &["--no-wait", "set-controller", bridge, &ep],
    )
    .await?;
    ensure_controller_in_band(bridge).await?;
    log::info!("SetController: {bridge} -> {ep} (connection_mode=in-band)");
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
pub struct DatapathHealth {
    pub bridge: String,
    pub fail_mode: String,
    pub controllers: Vec<String>,
    pub fallback_normal: bool,
    pub fallback_priority: u16,
    pub fallback_cookie: String,
    pub connection_mode: String,
    pub disable_in_band: bool,
}

pub async fn get_datapath_health(bridge: &str) -> Result<DatapathHealth> {
    let fail_mode = run_ok("ovs-vsctl", &["get", "Bridge", bridge, "fail_mode"])
        .await?
        .trim()
        .trim_matches('"')
        .to_string();

    let controllers_raw = run_ok("ovs-vsctl", &["get-controller", bridge])
        .await
        .unwrap_or_default();
    let controllers: Vec<String> = controllers_raw
        .lines()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    let fallback_normal = fallback_present(bridge).await?;

    let connection_mode = run_ok(
        "ovs-vsctl",
        &["--if-exists", "get", "Controller", ".", "connection_mode"],
    )
    .await
    .unwrap_or_else(|_| "n/a".into())
    .trim()
    .trim_matches('"')
    .to_string();

    let other = run_ok(
        "ovs-vsctl",
        &["get", "Bridge", bridge, "other_config"],
    )
    .await
    .unwrap_or_default();
    let disable_in_band = other.contains("disable-in-band") && other.contains("true");

    Ok(DatapathHealth {
        bridge: bridge.to_string(),
        fail_mode,
        controllers,
        fallback_normal,
        fallback_priority: FALLBACK_PRIORITY,
        fallback_cookie: format!("{FALLBACK_COOKIE:#x}"),
        connection_mode,
        disable_in_band,
    })
}

pub async fn attach_controller_safe(bridge: &str, endpoint: &str) -> Result<DatapathHealth> {
    set_fail_mode(bridge, "standalone").await?;
    ensure_fallback_normal(bridge).await?;

    if let Err(e) = set_controller(bridge, endpoint).await {
        let _ = del_controller(bridge).await;
        let _ = ensure_fallback_normal(bridge).await;
        return Err(e).context("AttachControllerSafe: set-controller failed; rolled back");
    }

    // Wait for datapath cache revalidation + controller connect wipe.
    tokio::time::sleep(DATAPATH_SETTLE).await;
    if let Err(e) = ensure_fallback_normal(bridge).await {
        let _ = del_controller(bridge).await;
        let _ = ensure_fallback_normal(bridge).await;
        return Err(e).context("AttachControllerSafe: fallback missing after settle; rolled back");
    }

    // Second check after another short window (cache lag).
    tokio::time::sleep(Duration::from_millis(400)).await;
    let health = get_datapath_health(bridge).await?;
    if !health.fallback_normal || health.fail_mode != "standalone" || health.disable_in_band {
        let _ = del_controller(bridge).await;
        let _ = ensure_fallback_normal(bridge).await;
        bail!(
            "AttachControllerSafe: health failed fallback={} fail_mode={} disable_in_band={}; rolled back",
            health.fallback_normal,
            health.fail_mode,
            health.disable_in_band
        );
    }
    log::info!(
        "AttachControllerSafe: ok bridge={} controllers={:?}",
        bridge,
        health.controllers
    );
    Ok(health)
}
