//! D-Bus service for systemctl-to-runit command mapping on Artix Linux.
//!
//! Implements the `org.opdbus.v1.Runit.Systemctl` interface, mapping
//! systemctl commands to runit operations for Artix Linux systems.
//!
//! ## Runit layout (Artix convention)
//!
//! | Path                                  | Meaning                                             |
//! |----------------------------------------|-----------------------------------------------------|
//! | `/etc/runit/sv/<service>`             | Service definition (`run`, optional `log/run`)      |
//! | `/run/runit/service/<service>`        | Symlink into the live/active runlevel (runsvdir scans this) |
//! | `/etc/runit/runsvdir/default/<service>` | Symlink for boot-persistent enablement of the `default` runlevel |
//! | `/etc/runit/sv/<service>/down`        | Marker file: service should not auto-start when supervised |
//!
//! `enable` = symlink into the persistent runlevel directory
//! (and remove any `down` marker). `disable` = `sv down` + remove both
//! symlinks. There is no compiled service database (unlike s6-rc); runsvdir
//! picks up directory changes automatically, so `daemon_reload` is a no-op.

use std::process::Command;
use tracing::{debug, error, info, warn};
use zbus::{interface, message::Header, Connection};

// Re-exported from `op_core::runit` so the layout is stated in exactly one
// place; the local names are kept for readability at the call sites below.
const RUNIT_SV_DIR: &str = op_core::runit::SV_DIR;
const RUNIT_SERVICE_DIR: &str = op_core::runit::SERVICE_DIR;
const RUNIT_RUNSVDIR_DEFAULT: &str = op_core::runit::RUNSVDIR_DEFAULT;

/// D-Bus service for runit systemctl-compatibility operations
pub struct RunitSystemctlService {
    /// Base directory for Artix runit service definitions.
    sv_dir: String,
    /// Live/active runlevel directory scanned by runsvdir.
    runtime_dir: String,
    /// Persistent boot-enable directory for the `default` runlevel.
    enable_dir: String,
}

impl RunitSystemctlService {
    pub fn new() -> Self {
        Self {
            sv_dir: RUNIT_SV_DIR.to_string(),
            runtime_dir: RUNIT_SERVICE_DIR.to_string(),
            enable_dir: RUNIT_RUNSVDIR_DEFAULT.to_string(),
        }
    }

    /// Check if runit's `sv` control tool is available.
    fn check_runit_available(&self) -> bool {
        Command::new("sv").output().is_ok()
    }

    fn def_path(&self, service: &str) -> String {
        format!("{}/{}", self.sv_dir, service)
    }

    fn live_path(&self, service: &str) -> String {
        format!("{}/{}", self.runtime_dir, service)
    }

    fn enable_link_path(&self, service: &str) -> String {
        format!("{}/{}", self.enable_dir, service)
    }

    /// Symlink a service definition into a target directory if not already present.
    fn ensure_symlink(&self, service: &str, target_dir: &str) -> Result<(), String> {
        let def = self.def_path(service);
        if !std::path::Path::new(&def).exists() {
            return Err(format!("service {} not found in {}", service, self.sv_dir));
        }
        if let Err(e) = std::fs::create_dir_all(target_dir) {
            return Err(format!("failed to create {}: {}", target_dir, e));
        }
        let link = format!("{}/{}", target_dir, service);
        if std::path::Path::new(&link).exists() {
            return Ok(());
        }
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&def, &link)
                .map_err(|e| format!("failed to symlink {} -> {}: {}", link, def, e))?;
        }
        Ok(())
    }

    fn remove_symlink(&self, service: &str, target_dir: &str) -> Result<(), String> {
        let link = format!("{}/{}", target_dir, service);
        match std::fs::remove_file(&link) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(format!("Failed to remove {link}: {error}")),
        }
    }

    fn validate_service_name(service: &str) -> Result<(), String> {
        if service.is_empty()
            || service.len() > 128
            || service == "."
            || service == ".."
            || !service.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b'@')
            })
        {
            return Err("invalid runit service name".to_string());
        }
        Ok(())
    }

    /// Service lifecycle is a root-only system-bus boundary. Higher-level
    /// identities and capabilities are checked by op-grpc-bridge; this second
    /// gate prevents a local unprivileged process from skipping that path and
    /// invoking runit directly over D-Bus.
    async fn require_root_caller(
        connection: &Connection,
        header: &Header<'_>,
    ) -> Result<(), String> {
        let sender = header
            .sender()
            .ok_or_else(|| "D-Bus caller has no unique sender".to_string())?;
        let proxy = zbus::fdo::DBusProxy::new(connection)
            .await
            .map_err(|error| format!("D-Bus credential lookup unavailable: {error}"))?;
        let uid = proxy
            .get_connection_unix_user(sender.clone().into())
            .await
            .map_err(|error| format!("D-Bus caller credential lookup failed: {error}"))?;
        if uid != 0 {
            return Err("permission denied: service lifecycle requires uid 0".to_string());
        }
        Ok(())
    }

    async fn authorize_lifecycle(
        service: &str,
        connection: &Connection,
        header: &Header<'_>,
    ) -> Result<(), String> {
        Self::validate_service_name(service)?;
        Self::require_root_caller(connection, header).await
    }

    /// Execute `sv <args>` and return (success, output)
    fn run_sv(&self, args: &[&str]) -> (bool, String) {
        if !self.check_runit_available() {
            return (
                false,
                "runit tools not available. Is runit installed?".to_string(),
            );
        }

        match Command::new("sv").args(args).output() {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();

                if output.status.success() {
                    let result = if stdout.is_empty() { stderr } else { stdout };
                    (true, result.trim().to_string())
                } else {
                    let error = if stderr.is_empty() { stdout } else { stderr };
                    (false, error.trim().to_string())
                }
            }
            Err(e) => (false, format!("Failed to execute sv: {}", e)),
        }
    }

    /// Execute `sv status <service>` and parse output
    fn run_sv_status(&self, service: &str) -> Result<ServiceStatus, String> {
        let path = self.live_path(service);
        match Command::new("sv").arg("status").arg(&path).output() {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                Ok(ServiceStatus::from_sv_status_output(&stdout, service))
            }
            Err(e) => Err(format!("Failed to execute sv status: {}", e)),
        }
    }

    /// Check if a service is enabled (symlinked into the persistent `default` runlevel)
    fn is_service_enabled(&self, service: &str) -> bool {
        std::path::Path::new(&self.enable_link_path(service)).exists()
    }
}

#[cfg(test)]
mod path_security_tests {
    use super::RunitSystemctlService;

    #[test]
    fn disable_unlink_failure_is_not_success() {
        let root = std::env::temp_dir().join(format!("runit-unlink-test-{}", std::process::id()));
        std::fs::create_dir_all(root.join("service")).unwrap();
        let service = RunitSystemctlService::new();
        assert!(service
            .remove_symlink("service", root.to_str().unwrap())
            .is_err());
        std::fs::remove_dir(root.join("service")).unwrap();
        assert!(service
            .remove_symlink("service", root.to_str().unwrap())
            .is_ok());
        std::fs::remove_dir(root).unwrap();
    }

    #[test]
    fn service_name_is_one_component() {
        for name in ["../secret", "a/b", "/absolute", "..", ".", ""] {
            assert!(RunitSystemctlService::validate_service_name(name).is_err());
        }
        assert!(RunitSystemctlService::validate_service_name("op-grpc-bridge").is_ok());
    }

    #[tokio::test]
    async fn read_only_methods_reject_traversal() {
        let service = RunitSystemctlService::new();
        assert!(service
            .show("../outside")
            .await
            .contains("invalid runit service name"));
        assert!(service
            .journalctl("../outside", 10)
            .await
            .contains("invalid runit service name"));
    }
}

impl Default for RunitSystemctlService {
    fn default() -> Self {
        Self::new()
    }
}

/// Service status parsed from `sv status` output
#[derive(Debug, serde::Serialize)]
struct ServiceStatus {
    name: String,
    active_state: String,
    sub_state: String,
    main_pid: Option<u32>,
    ready: bool,
    up_time: Option<String>,
}

impl ServiceStatus {
    fn from_sv_status_output(output: &str, name: &str) -> Self {
        // sv status output forms:
        //   "run: <path>: (pid 1234) 56s"
        //   "down: <path>: 12s, normally up"
        //   "down: <path>: 12s"
        //   "fail: <path>: unable to open supervise/ok: file does not exist"
        let output = output.trim();
        let mut parts = output.splitn(3, ": ");
        let state_word = parts.next().unwrap_or("");
        let _path = parts.next();
        let rest = parts.next().unwrap_or("");

        let (active_state, sub_state, main_pid, ready, up_time) = match state_word {
            "run" => {
                let pid = rest
                    .split("pid ")
                    .nth(1)
                    .and_then(|s| s.split(')').next())
                    .and_then(|p| p.parse::<u32>().ok());
                let up_time = rest
                    .rsplit_once(')')
                    .map(|(_, t)| t.trim().to_string())
                    .filter(|t| !t.is_empty());
                (
                    "active".to_string(),
                    "running".to_string(),
                    pid,
                    true,
                    up_time,
                )
            }
            "down" => {
                let up_time = rest
                    .split(',')
                    .next()
                    .map(|t| t.trim().to_string())
                    .filter(|t| !t.is_empty());
                (
                    "inactive".to_string(),
                    "dead".to_string(),
                    None,
                    false,
                    up_time,
                )
            }
            "fail" => (
                "unknown".to_string(),
                "not-found".to_string(),
                None,
                false,
                None,
            ),
            _ => (
                "unknown".to_string(),
                "unknown".to_string(),
                None,
                false,
                None,
            ),
        };

        Self {
            name: name.to_string(),
            active_state,
            sub_state,
            main_pid,
            ready,
            up_time,
        }
    }

    fn to_json(&self) -> String {
        match simd_json::to_string(self) {
            Ok(s) => s,
            Err(_) => format!(
                "{{\"name\":\"{}\",\"active_state\":\"{}\",\"sub_state\":\"{}\"}}",
                self.name, self.active_state, self.sub_state
            ),
        }
    }
}

#[interface(name = "org.opdbus.v1.Runit.Systemctl")]
impl RunitSystemctlService {
    /// Start a service (ensures the persistent runlevel entry, then `sv up`).
    async fn start(
        &self,
        service: &str,
        #[zbus(connection)] connection: &Connection,
        #[zbus(header)] header: Header<'_>,
    ) -> (bool, String) {
        if let Err(error) = Self::authorize_lifecycle(service, connection, &header).await {
            warn!(service, %error, "rejected runit start request");
            return (false, error);
        }
        debug!("Starting service: {}", service);
        info!(
            "systemctl start {} -> sv up {}/{}",
            service, self.runtime_dir, service
        );

        if let Err(e) = self.ensure_symlink(service, &self.enable_dir) {
            error!("Failed to start service {}: {}", service, e);
            return (false, e);
        }

        let result = self.run_sv(&["up", &self.live_path(service)]);
        if result.0 {
            info!("Service {} started successfully", service);
        } else {
            error!("Failed to start service {}: {}", service, result.1);
        }
        result
    }

    /// Stop a service (maps to: sv down <service>)
    async fn stop(
        &self,
        service: &str,
        #[zbus(connection)] connection: &Connection,
        #[zbus(header)] header: Header<'_>,
    ) -> (bool, String) {
        if let Err(error) = Self::authorize_lifecycle(service, connection, &header).await {
            warn!(service, %error, "rejected runit stop request");
            return (false, error);
        }
        debug!("Stopping service: {}", service);
        info!(
            "systemctl stop {} -> sv down {}/{}",
            service, self.runtime_dir, service
        );

        let live = self.live_path(service);
        if !std::path::Path::new(&live).exists() {
            return (
                true,
                format!("{} is not running (not in {})", service, self.runtime_dir),
            );
        }

        let result = self.run_sv(&["down", &live]);
        if result.0 {
            info!("Service {} stopped successfully", service);
        } else {
            error!("Failed to stop service {}: {}", service, result.1);
        }
        result
    }

    /// Stop with runit's bounded force-stop escalation, preserving the same
    /// root-only D-Bus admission as normal lifecycle operations.
    async fn force_stop(
        &self,
        service: &str,
        #[zbus(connection)] connection: &Connection,
        #[zbus(header)] header: Header<'_>,
    ) -> (bool, String) {
        if let Err(error) = Self::authorize_lifecycle(service, connection, &header).await {
            return (false, error);
        }
        self.run_sv(&["force-stop", &self.live_path(service)])
    }

    /// Restart a service (maps to: sv restart <service>)
    async fn restart(
        &self,
        service: &str,
        #[zbus(connection)] connection: &Connection,
        #[zbus(header)] header: Header<'_>,
    ) -> (bool, String) {
        if let Err(error) = Self::authorize_lifecycle(service, connection, &header).await {
            warn!(service, %error, "rejected runit restart request");
            return (false, error);
        }
        debug!("Restarting service: {}", service);
        info!(
            "systemctl restart {} -> sv restart {}/{}",
            service, self.runtime_dir, service
        );

        let live = self.live_path(service);
        if !std::path::Path::new(&live).exists() {
            if let Err(e) = self.ensure_symlink(service, &self.enable_dir) {
                return (false, e);
            }
            return self.run_sv(&["up", &self.live_path(service)]);
        }

        let result = self.run_sv(&["restart", &live]);
        if result.0 {
            info!("Service {} restarted successfully", service);
        } else {
            error!("Failed to restart service {}: {}", service, result.1);
        }
        result
    }

    /// Reload a service (maps to: sv hup <service>)
    async fn reload(
        &self,
        service: &str,
        #[zbus(connection)] connection: &Connection,
        #[zbus(header)] header: Header<'_>,
    ) -> (bool, String) {
        if let Err(error) = Self::authorize_lifecycle(service, connection, &header).await {
            warn!(service, %error, "rejected runit reload request");
            return (false, error);
        }
        debug!("Reloading service: {}", service);
        info!(
            "systemctl reload {} -> sv hup {}/{}",
            service, self.runtime_dir, service
        );

        let result = self.run_sv(&["hup", &self.live_path(service)]);
        if result.0 {
            info!("Service {} reloaded successfully", service);
        } else {
            error!("Failed to reload service {}: {}", service, result.1);
        }
        result
    }

    /// Enable a service in the persistent `default` runlevel.
    async fn enable(
        &self,
        service: &str,
        #[zbus(connection)] connection: &Connection,
        #[zbus(header)] header: Header<'_>,
    ) -> (bool, String) {
        if let Err(error) = Self::authorize_lifecycle(service, connection, &header).await {
            warn!(service, %error, "rejected runit enable request");
            return (false, error);
        }
        debug!("Enabling service: {}", service);
        info!(
            "runit enable {} -> symlink into {}",
            service, self.enable_dir
        );

        if let Err(e) = self.ensure_symlink(service, &self.enable_dir) {
            return (false, e);
        }

        // Remove the `down` marker (if any) so the service auto-starts.
        let down_file = format!("{}/down", self.def_path(service));
        let _ = std::fs::remove_file(&down_file);

        info!("Service {} enabled", service);
        (true, format!("Service {service} enabled"))
    }

    /// Disable a service (sv down + remove the persistent runlevel entry).
    async fn disable(
        &self,
        service: &str,
        #[zbus(connection)] connection: &Connection,
        #[zbus(header)] header: Header<'_>,
    ) -> (bool, String) {
        if let Err(error) = Self::authorize_lifecycle(service, connection, &header).await {
            warn!(service, %error, "rejected runit disable request");
            return (false, error);
        }
        debug!("Disabling service: {}", service);
        info!(
            "runit disable {} -> sv down + remove entry from {}",
            service, self.enable_dir
        );

        let live = self.live_path(service);
        if std::path::Path::new(&live).exists() {
            let result = self.run_sv(&["down", &live]);
            if !result.0 {
                return result;
            }
        }

        if let Err(error) = self.remove_symlink(service, &self.enable_dir) {
            return (false, error);
        }

        info!("Service {} disabled", service);
        (true, format!("Service {service} disabled"))
    }

    /// Get detailed status of a service (JSON format)
    async fn status(&self, service: &str) -> String {
        if let Err(error) = Self::validate_service_name(service) {
            return serde_json::json!({"error": error}).to_string();
        }
        debug!("Getting status for service: {}", service);

        match self.run_sv_status(service) {
            Ok(status) => status.to_json(),
            Err(e) => {
                warn!("Failed to get status for {}: {}", service, e);
                format!(
                    "{{\"name\":\"{}\",\"error\":\"{}\",\"active_state\":\"unknown\"}}",
                    service, e
                )
            }
        }
    }

    /// Check if a service is active (returns "active" or "inactive")
    async fn is_active(&self, service: &str) -> String {
        if let Err(error) = Self::validate_service_name(service) {
            return serde_json::json!({"error": error}).to_string();
        }
        match self.run_sv_status(service) {
            Ok(status) => {
                if status.active_state == "active" {
                    "active".to_string()
                } else {
                    "inactive".to_string()
                }
            }
            Err(_) => "inactive".to_string(),
        }
    }

    /// Check if a service is enabled (returns "enabled" or "disabled")
    async fn is_enabled(&self, service: &str) -> String {
        if let Err(error) = Self::validate_service_name(service) {
            return serde_json::json!({"error": error}).to_string();
        }
        if self.is_service_enabled(service) {
            "enabled".to_string()
        } else {
            "disabled".to_string()
        }
    }

    /// Compatibility alias used by existing generated zbus proxies.
    async fn is_enabled_method(&self, service: &str) -> String {
        self.is_enabled(service).await
    }

    /// Show service properties as JSON.
    async fn show(&self, service: &str) -> String {
        if let Err(error) = Self::validate_service_name(service) {
            return serde_json::json!({"error": error}).to_string();
        }
        let mut properties = serde_json::Map::new();
        properties.insert("Id".to_string(), serde_json::json!(service));
        properties.insert("Names".to_string(), serde_json::json!(service));
        properties.insert("LoadState".to_string(), serde_json::json!("loaded"));
        properties.insert(
            "UnitFileState".to_string(),
            serde_json::json!(self.is_enabled(service).await),
        );
        properties.insert(
            "Type".to_string(),
            serde_json::json!(self.get_unit_type(service).await),
        );

        match self.run_sv_status(service) {
            Ok(status) => {
                properties.insert(
                    "ActiveState".to_string(),
                    serde_json::json!(status.active_state),
                );
                properties.insert("SubState".to_string(), serde_json::json!(status.sub_state));
                properties.insert("MainPID".to_string(), serde_json::json!(status.main_pid));
                properties.insert("Ready".to_string(), serde_json::json!(status.ready));
                properties.insert("UpTime".to_string(), serde_json::json!(status.up_time));
            }
            Err(e) => {
                properties.insert("ActiveState".to_string(), serde_json::json!("unknown"));
                properties.insert("SubState".to_string(), serde_json::json!("error"));
                properties.insert("Error".to_string(), serde_json::json!(e));
            }
        }

        serde_json::Value::Object(properties).to_string()
    }

    /// List all active units (JSON array) — scans the live runlevel directory.
    async fn list_units(&self) -> String {
        debug!("Listing all units");

        let entries = match std::fs::read_dir(&self.runtime_dir) {
            Ok(entries) => entries,
            Err(e) => return format!("{{\"error\":\"Failed to list units: {}\"}}", e),
        };

        let mut units = Vec::new();
        for entry in entries.flatten() {
            let Some(name) = entry.file_name().to_str().map(str::to_string) else {
                continue;
            };
            if name.starts_with('.') {
                continue;
            }
            match self.run_sv_status(&name) {
                Ok(status) => units.push(status),
                Err(_) => {
                    units.push(ServiceStatus {
                        name: name.clone(),
                        active_state: "unknown".to_string(),
                        sub_state: "unknown".to_string(),
                        main_pid: None,
                        ready: false,
                        up_time: None,
                    });
                }
            }
        }

        match simd_json::to_string(&units) {
            Ok(json) => json,
            Err(e) => format!("{{\"error\":\"Failed to serialize: {}\"}}", e),
        }
    }

    /// List all available unit files from the runit service directory.
    async fn list_unit_files(&self) -> String {
        debug!("Listing all unit files");

        let entries = match std::fs::read_dir(&self.sv_dir) {
            Ok(entries) => entries,
            Err(e) => return format!("{{\"error\":\"Failed to list unit files: {}\"}}", e),
        };

        let mut units = Vec::new();
        for entry in entries.flatten() {
            let file_type = match entry.file_type() {
                Ok(file_type) => file_type,
                Err(_) => continue,
            };
            if !file_type.is_dir() && !file_type.is_symlink() {
                continue;
            }
            let Some(name) = entry.file_name().to_str().map(str::to_string) else {
                continue;
            };
            if name.starts_with('.') {
                continue;
            }
            units.push(serde_json::json!({
                "unit_file": format!("{name}.service"),
                "state": self.is_enabled(&name).await,
                "vendor_preset": "disabled"
            }));
        }

        units.sort_by(|a, b| {
            a.get("unit_file")
                .and_then(|v| v.as_str())
                .cmp(&b.get("unit_file").and_then(|v| v.as_str()))
        });

        serde_json::Value::Array(units).to_string()
    }

    /// Retrieve service logs (svlogd `current` file) with tail fallback.
    async fn journalctl(&self, service: &str, lines: u32) -> String {
        if let Err(error) = Self::validate_service_name(service) {
            return serde_json::json!({"error": error}).to_string();
        }
        let lines = lines.max(1).to_string();
        let log_candidates = [
            format!("/var/log/op-dbus/{service}/current"),
            format!("/var/log/{service}/current"),
            format!("/run/log/op-dbus/{service}/current"),
        ];
        let Some(log_path) = log_candidates
            .iter()
            .find(|path| std::path::Path::new(path.as_str()).exists())
        else {
            return format!("{{\"error\":\"No log file found for {}\"}}", service);
        };

        match Command::new("tail").args(["-n", &lines, log_path]).output() {
            Ok(output) if output.status.success() => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let entries: Vec<_> = stdout
                    .lines()
                    .map(|line| {
                        serde_json::json!({
                            "UNIT": service,
                            "LOG_PATH": log_path,
                            "MESSAGE": line
                        })
                    })
                    .collect();
                serde_json::Value::Array(entries).to_string()
            }
            Ok(output) => format!(
                "{{\"error\":\"{}\"}}",
                String::from_utf8_lossy(&output.stderr)
            ),
            Err(e) => format!("{{\"error\":\"Failed to read logs: {}\"}}", e),
        }
    }

    /// Runit has no compiled service database (unlike s6-rc); runsvdir picks
    /// up directory changes automatically, so this is a best-effort no-op.
    async fn daemon_reload(
        &self,
        #[zbus(connection)] connection: &Connection,
        #[zbus(header)] header: Header<'_>,
    ) -> (bool, String) {
        if let Err(error) = Self::require_root_caller(connection, &header).await {
            warn!(%error, "rejected runit daemon-reload request");
            return (false, error);
        }
        (
            true,
            "runit requires no database recompilation; changes take effect automatically"
                .to_string(),
        )
    }

    /// Get daemon/runit supervisor status
    async fn daemon_status(&self) -> String {
        if !self.check_runit_available() {
            return "not-available".to_string();
        }
        match Command::new("pgrep").arg("-x").arg("runsvdir").output() {
            Ok(output) if output.status.success() => "running".to_string(),
            _ => {
                if std::path::Path::new(&self.runtime_dir).exists() {
                    "running".to_string()
                } else {
                    "not-available".to_string()
                }
            }
        }
    }

    /// Return the best-known unit type from an optional `type` marker file
    /// (a repo convention; runit itself has no unit-type metadata).
    async fn get_unit_type(&self, service: &str) -> String {
        if Self::validate_service_name(service).is_err() {
            return "longrun".to_string();
        }
        let type_path = format!("{}/type", self.def_path(service));
        match std::fs::read_to_string(&type_path) {
            Ok(value) => value.trim().to_string(),
            Err(_) => "longrun".to_string(),
        }
    }
}
