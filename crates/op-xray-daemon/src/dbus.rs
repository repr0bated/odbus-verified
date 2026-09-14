//! D-Bus service implementation for Xray daemon management
//!
//! Provides the `org.opdbus.v1.Xray` interface for controlling
//! xray process lifecycle via D-Bus.
//!
//! Production xray runs under the `xray` container's own systemd
//! (`xray.service`), not spawned by this daemon — `start()` here predates
//! that and is effectively unsupported now (see its doc comment). Liveness
//! is detected by scanning `/proc` for the real process, not by tracking a
//! child handle this daemon itself spawned — the previous in-memory
//! `Option<Child>` tracking lost all state across a daemon restart and could
//! never see an externally-started xray process at all, which is the
//! process this daemon actually needs to observe in practice.

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};
use zbus::interface;

/// Status information for the Xray daemon
#[derive(Debug, Clone, Serialize, Deserialize)]
struct XrayStatus {
    running: bool,
    pid: Option<u32>,
    config_path: Option<String>,
}

/// Find running `xray` process PIDs by scanning `/proc` directly — no
/// `pgrep`/`pkill` subprocess spawning (CLAUDE.md: no `Command::new(...)`
/// subprocesses in plugin/service code).
fn find_xray_pids() -> Vec<nix::unistd::Pid> {
    let mut pids = Vec::new();
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return pids;
    };
    for entry in entries.flatten() {
        let Ok(pid) = entry.file_name().to_string_lossy().parse::<i32>() else {
            continue;
        };
        if let Ok(comm) = std::fs::read_to_string(entry.path().join("comm")) {
            if comm.trim() == "xray" {
                pids.push(nix::unistd::Pid::from_raw(pid));
            }
        }
    }
    pids
}

/// Last config path this daemon was told about via `start()`/`restart()`
/// (informational only — a real xray process is detected live via `/proc`,
/// this is just the daemon's own last-instruction memory).
pub struct XrayProcessState {
    config_path: Option<String>,
}

impl XrayProcessState {
    fn new() -> Self {
        Self { config_path: None }
    }
}

/// D-Bus service for managing Xray daemon lifecycle
pub struct XrayService {
    state: Arc<Mutex<XrayProcessState>>,
}

impl XrayService {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(XrayProcessState::new())),
        }
    }

    /// Create a status snapshot for external reporting, from real `/proc`
    /// state — not this daemon's own (possibly stale) memory of it.
    async fn get_status_snapshot(&self) -> XrayStatus {
        let pids = find_xray_pids();
        let state = self.state.lock().await;
        XrayStatus {
            running: !pids.is_empty(),
            pid: pids.first().map(|p| p.as_raw() as u32),
            config_path: state.config_path.clone(),
        }
    }
}

impl Default for XrayService {
    fn default() -> Self {
        Self::new()
    }
}

/// D-Bus interface implementation for org.opdbus.v1.Xray
#[interface(name = "org.opdbus.v1.Xray")]
impl XrayService {
    /// Start xray with the specified config path.
    ///
    /// NOTE: production xray runs under the `xray` container's own systemd
    /// unit (`xray.service`), started independently of this daemon. This
    /// method cannot launch a new process without a `Command::new(...)`
    /// subprocess spawn (prohibited — see module docs), so it only verifies
    /// an instance is already running rather than starting one. Use the
    /// container's systemd unit to actually start xray.
    async fn start(&mut self, config_path: &str) -> (bool, String) {
        info!("Xray start requested with config: {}", config_path);

        if !find_xray_pids().is_empty() {
            let mut state = self.state.lock().await;
            state.config_path = Some(config_path.to_string());
            return (
                true,
                "xray is already running (started by the container's systemd unit, \
                 not this daemon)"
                    .to_string(),
            );
        }

        (
            false,
            "xray is not running and this daemon cannot start it directly (no subprocess \
             spawning) — start it via the `xray` container's systemd unit (`xray.service`)"
                .to_string(),
        )
    }

    /// Stop the running xray process (SIGTERM — the container's systemd will
    /// typically restart it, since it's a supervised unit; use this for a
    /// deliberate restart-in-place, not to permanently take xray down).
    /// Returns (success, message)
    async fn stop(&mut self) -> (bool, String) {
        info!("Xray stop requested");

        let pids = find_xray_pids();
        if pids.is_empty() {
            warn!("Xray is not running");
            return (false, "Xray is not running".to_string());
        }

        let mut errors = Vec::new();
        for pid in &pids {
            if let Err(e) = nix::sys::signal::kill(*pid, nix::sys::signal::Signal::SIGTERM) {
                errors.push(format!("pid {pid}: {e}"));
            }
        }

        if errors.is_empty() {
            info!("Xray process(es) terminated: {:?}", pids);
            (true, "Xray stopped successfully".to_string())
        } else {
            error!("Failed to stop xray: {:?}", errors);
            (false, format!("Failed to stop xray: {}", errors.join(", ")))
        }
    }

    /// Reload xray configuration by sending SIGHUP to the real running
    /// process(es). Restart with a config path just updates this daemon's
    /// own memory of the config path and reloads — it does not actually
    /// swap xray's config file on disk (that lives elsewhere, per the
    /// container's own deploy).
    /// Returns (success, message)
    async fn restart(&mut self, config_path: &str) -> (bool, String) {
        info!("Xray restart (reload) requested");
        let mut state = self.state.lock().await;
        state.config_path = Some(config_path.to_string());
        drop(state);
        self.reload().await
    }

    /// Get current xray status as JSON string
    async fn status(&self) -> String {
        let status = self.get_status_snapshot().await;
        match serde_json::to_string(&status) {
            Ok(json) => {
                debug!("Xray status: {}", json);
                json
            }
            Err(e) => {
                error!("Failed to serialize status: {}", e);
                r#"{"running":false,"error":"serialization failed"}"#.to_string()
            }
        }
    }

    /// Reload xray configuration by sending SIGHUP
    /// Returns (success, message)
    async fn reload(&self) -> (bool, String) {
        info!("Xray reload requested");

        let pids = find_xray_pids();
        if pids.is_empty() {
            warn!("Cannot reload: Xray is not running");
            return (false, "Xray is not running".to_string());
        }

        let mut errors = Vec::new();
        for pid in &pids {
            if let Err(e) = nix::sys::signal::kill(*pid, nix::sys::signal::Signal::SIGHUP) {
                errors.push(format!("pid {pid}: {e}"));
            }
        }

        if errors.is_empty() {
            info!("Sent SIGHUP to xray {:?}", pids);
            (
                true,
                format!("Reload signal sent to xray (PIDs: {:?})", pids),
            )
        } else {
            error!("Failed to send SIGHUP: {:?}", errors);
            (
                false,
                format!("Failed to send reload signal: {}", errors.join(", ")),
            )
        }
    }

    /// Get current config path
    async fn get_config(&self) -> String {
        let state = self.state.lock().await;
        state.config_path.clone().unwrap_or_default()
    }

    /// Query xray-core's own StatsService.QueryStats over the commander UDS
    /// (`crate::commander_client::DEFAULT_API_SOCKET`). Returns a JSON array
    /// of `{name, value}`, or a JSON `{"error": ...}` object on failure —
    /// e.g. if xray's `api`/`stats` blocks aren't configured, or the socket
    /// isn't up yet.
    async fn query_stats(&self, pattern: &str, reset: bool) -> String {
        let result: anyhow::Result<String> = async {
            let mut client =
                crate::commander_client::stats_client(crate::commander_client::DEFAULT_API_SOCKET)
                    .await?;
            let resp = client
                .query_stats(crate::commander_client::stats::QueryStatsRequest {
                    pattern: pattern.to_string(),
                    reset,
                })
                .await?
                .into_inner();
            let stats: Vec<serde_json::Value> = resp
                .stat
                .into_iter()
                .map(|s| serde_json::json!({ "name": s.name, "value": s.value }))
                .collect();
            serde_json::to_string(&stats).map_err(anyhow::Error::from)
        }
        .await;

        match result {
            Ok(json) => json,
            Err(e) => {
                warn!("QueryStats failed: {e:#}");
                serde_json::json!({ "error": e.to_string() }).to_string()
            }
        }
    }

    /// xray-core's own Go runtime health (StatsService.GetSysStats) —
    /// goroutines, GC cycles, memory, uptime. Genuine process telemetry.
    async fn get_sys_stats(&self) -> String {
        let result: anyhow::Result<String> = async {
            let mut client =
                crate::commander_client::stats_client(crate::commander_client::DEFAULT_API_SOCKET)
                    .await?;
            let resp = client
                .get_sys_stats(crate::commander_client::stats::SysStatsRequest {})
                .await?
                .into_inner();
            let json = serde_json::json!({
                "num_goroutine": resp.num_goroutine,
                "num_gc": resp.num_gc,
                "alloc": resp.alloc,
                "total_alloc": resp.total_alloc,
                "sys": resp.sys,
                "mallocs": resp.mallocs,
                "frees": resp.frees,
                "live_objects": resp.live_objects,
                "pause_total_ns": resp.pause_total_ns,
                "uptime": resp.uptime,
            });
            Ok(json.to_string())
        }
        .await;

        match result {
            Ok(json) => json,
            Err(e) => {
                warn!("GetSysStats failed: {e:#}");
                serde_json::json!({ "error": e.to_string() }).to_string()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_xray_status_serialization() {
        let status = XrayStatus {
            running: true,
            pid: Some(1234),
            config_path: Some("/etc/xray/xray_config.json".to_string()),
        };

        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains("\"running\":true"));
        assert!(json.contains("\"pid\":1234"));
        assert!(json.contains("\"config_path\":\"/etc/xray/xray_config.json\""));
    }

    #[tokio::test]
    async fn test_xray_service_creation() {
        // Real /proc-based detection now, so this reflects actual system
        // state rather than an isolated fake — just check it doesn't panic
        // and config_path starts empty (this daemon's own memory, distinct
        // from whether a real xray process happens to be running).
        let service = XrayService::new();
        let status = service.get_status_snapshot().await;
        assert_eq!(status.config_path, None);
    }
}
