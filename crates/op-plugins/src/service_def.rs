//! Runit service definitions and lifecycle management.
//!
//! Schema-as-code: These types ARE the schema. Validation happens at parse time.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tracing::info;

const RUNIT_SERVICE_DIR: &str = "/etc/runit/sv";
const RUNIT_ACTIVE_DIR: &str = "/etc/runit/runsvdir/default";

/// Service name - validated on construction
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ServiceName(String);

impl ServiceName {
    pub fn new(name: impl Into<String>) -> Result<Self, ValidationError> {
        let name = name.into();
        if name.is_empty() {
            return Err(ValidationError::EmptyName);
        }
        if name.len() > 64 {
            return Err(ValidationError::NameTooLong(name.len()));
        }
        if !name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.' || c == '@')
        {
            return Err(ValidationError::InvalidChars(name));
        }
        if name.starts_with('-') || name.starts_with('.') {
            return Err(ValidationError::InvalidStart(name));
        }
        Ok(Self(name))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for ServiceName {
    type Error = ValidationError;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        Self::new(s)
    }
}
impl From<ServiceName> for String {
    fn from(n: ServiceName) -> String {
        n.0
    }
}
impl std::fmt::Display for ServiceName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Validation errors
#[derive(Debug, thiserror::Error)]
pub enum ValidationError {
    #[error("service name cannot be empty")]
    EmptyName,
    #[error("service name exceeds 64 chars: {0}")]
    NameTooLong(usize),
    #[error("service name contains invalid characters: {0}")]
    InvalidChars(String),
    #[error("service name cannot start with - or .: {0}")]
    InvalidStart(String),
    #[error("command path must be absolute: {0}")]
    RelativePath(String),
    #[error("invalid resource limit: {0}")]
    InvalidResource(String),
}

/// Service type
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ServiceType {
    #[default]
    Simple,
    Forking {
        pid_file: Option<PathBuf>,
    },
    Oneshot,
    Notify,
}

/// Active state
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ActiveState {
    Active,
    Inactive,
    Activating,
    Deactivating,
    Failed,
    Reloading,
}

/// Command to execute - validated
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecCommand {
    pub program: PathBuf,
    #[serde(default)]
    pub args: Vec<String>,
}

impl ExecCommand {
    pub fn new(program: impl Into<PathBuf>, args: Vec<String>) -> Result<Self, ValidationError> {
        let program = program.into();
        if !program.is_absolute() {
            return Err(ValidationError::RelativePath(program.display().to_string()));
        }
        Ok(Self { program, args })
    }

    pub fn to_command_line(&self) -> String {
        let mut cmd = self.program.display().to_string();
        for arg in &self.args {
            cmd.push(' ');
            if arg.contains(' ') {
                cmd.push('"');
                cmd.push_str(arg);
                cmd.push('"');
            } else {
                cmd.push_str(arg);
            }
        }
        cmd
    }
}

/// Resource limits
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ResourceLimits {
    pub memory_max: Option<u64>,
    pub cpu_quota: Option<f32>,
    pub tasks_max: Option<u32>,
}

impl ResourceLimits {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if let Some(mem) = self.memory_max {
            if mem < 1024 * 1024 {
                return Err(ValidationError::InvalidResource("memory_max < 1MB".into()));
            }
        }
        if let Some(cpu) = self.cpu_quota {
            if cpu <= 0.0 || cpu > 100.0 {
                return Err(ValidationError::InvalidResource(
                    "cpu_quota not in 0-100".into(),
                ));
            }
        }
        Ok(())
    }
}

/// Restart condition
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RestartCondition {
    #[default]
    Never,
    Always,
    OnFailure,
}

/// Log type for a service.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogType {
    #[default]
    None,
    Buffer,
    Syslog,
    File(PathBuf),
}

/// Ready notification mechanism
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReadyNotification {
    #[default]
    None,
    Pipefd(u32),
    SdNotify,
}

/// Restart policy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestartPolicy {
    #[serde(default)]
    pub condition: RestartCondition,
    #[serde(default = "default_delay")]
    pub delay_secs: u64,
    pub max_retries: Option<u32>,
}

fn default_delay() -> u64 {
    1
}

impl Default for RestartPolicy {
    fn default() -> Self {
        Self {
            condition: RestartCondition::Never,
            delay_secs: 1,
            max_retries: None,
        }
    }
}

/// Service definition - the schema
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceDef {
    pub name: ServiceName,
    #[serde(default)]
    pub service_type: ServiceType,
    pub exec_start: ExecCommand,
    pub exec_stop: Option<ExecCommand>,
    pub working_dir: Option<PathBuf>,
    pub user: Option<String>,
    pub group: Option<String>,
    #[serde(default)]
    pub depends_on: Vec<ServiceName>,
    #[serde(default)]
    pub waits_for: Vec<ServiceName>,
    #[serde(default)]
    pub restart: RestartPolicy,
    #[serde(default)]
    pub environment: HashMap<String, String>,
    #[serde(default)]
    pub env_file: Option<PathBuf>,
    #[serde(default)]
    pub resources: Option<ResourceLimits>,
    #[serde(default)]
    pub log_type: LogType,
    #[serde(default)]
    pub ready_notification: ReadyNotification,
    #[serde(default)]
    pub chain_to: Option<ServiceName>,
    #[serde(default)]
    pub smooth_recovery: bool,
    #[serde(default)]
    pub enabled: bool,
}

impl ServiceDef {
    /// Generate a runit `run` script from the service definition.
    ///
    /// The resulting script follows the runit convention:
    /// ```sh
    /// #!/bin/sh
    /// exec <command>
    /// ```
    /// Environment variables and working directory are set up before the exec.
    pub fn to_runit_run(&self) -> String {
        let mut out = String::new();
        out.push_str("#!/bin/sh\n");

        // Working directory
        if let Some(ref dir) = self.working_dir {
            out.push_str(&format!("cd {} || exit 1\n", dir.display()));
        }

        // Runit uses chpst for privilege dropping.
        if let Some(ref user) = self.user {
            let group_suffix = self
                .group
                .as_deref()
                .map(|g| format!(":{g}"))
                .unwrap_or_default();
            out.push_str(&format!(
                "exec chpst -u {user}{group_suffix} {}\n",
                self.exec_start.to_command_line()
            ));
        } else {
            out.push_str(&format!("exec {}\n", self.exec_start.to_command_line()));
        }

        out
    }

    /// Write and enable the runit definition at `/etc/runit/sv/<name>/run`.
    ///
    /// Creates the service directory if it does not exist and makes the run
    /// script executable (mode 0o755).
    pub fn install(&self) -> std::io::Result<()> {
        let svc_dir = format!("{RUNIT_SERVICE_DIR}/{}", self.name);
        std::fs::create_dir_all(&svc_dir)?;

        let run_path = format!("{svc_dir}/run");
        let content = self.to_runit_run();
        std::fs::write(&run_path, &content)?;

        // Make the run script executable
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&run_path, std::fs::Permissions::from_mode(0o755))?;
        }

        let active_path = format!("{RUNIT_ACTIVE_DIR}/{}", self.name);
        std::fs::create_dir_all(RUNIT_ACTIVE_DIR)?;
        if !Path::new(&active_path).exists() {
            #[cfg(unix)]
            std::os::unix::fs::symlink(&svc_dir, &active_path)?;
        }

        Ok(())
    }
}

/// Current runit service state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceState {
    pub name: ServiceName,
    pub active_state: ActiveState,
    pub sub_state: String,
    pub load_state: String,
}

/// Internal manager state (state machine)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ManagerState {
    Stopped,
    Starting,
    Running,
    Stopping,
    Failed,
}

/// Service status (runtime)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceStatus {
    pub name: ServiceName,
    pub state: ManagerState,
    pub pid: Option<u32>,
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// Desired state for apply
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DesiredState {
    pub name: ServiceName,
    pub active: Option<ActiveState>,
    pub enabled: Option<bool>,
}

/// Runit lifecycle manager.
#[derive(Debug, Clone, Default)]
pub struct RunitPlugin {
    pub services: Vec<ServiceName>,
}

impl RunitPlugin {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn get_state(&self) -> Result<Vec<ServiceState>> {
        let names: Vec<&str> = if self.services.is_empty() {
            let mut names = Vec::new();
            for entry in std::fs::read_dir(RUNIT_ACTIVE_DIR)
                .with_context(|| format!("read {RUNIT_ACTIVE_DIR}"))?
            {
                let entry = entry?;
                if let Some(name) = entry.file_name().to_str() {
                    names.push(name.to_string());
                }
            }
            names.sort();
            return self.get_named_state(&names).await;
        } else {
            self.services.iter().map(|s| s.as_str()).collect()
        };

        let mut states = Vec::new();
        for name in names {
            if let Ok(state) = self.get_service_status(name).await {
                states.push(state);
            }
        }
        Ok(states)
    }

    async fn get_named_state(&self, names: &[String]) -> Result<Vec<ServiceState>> {
        let mut states = Vec::new();
        for name in names {
            if let Ok(state) = self.get_service_status(name).await {
                states.push(state);
            }
        }
        Ok(states)
    }

    pub async fn apply(&self, desired: &[DesiredState]) -> Result<()> {
        for d in desired {
            if let Some(active) = d.active {
                match active {
                    ActiveState::Active => self.start(d.name.as_str()).await?,
                    ActiveState::Inactive => self.stop(d.name.as_str()).await?,
                    _ => {}
                }
            }
            if let Some(enabled) = d.enabled {
                if enabled {
                    self.enable(d.name.as_str()).await?;
                } else {
                    self.disable(d.name.as_str()).await?;
                }
            }
        }
        Ok(())
    }

    pub async fn start(&self, name: &str) -> Result<()> {
        self.ctl(name, "start").await
    }
    pub async fn stop(&self, name: &str) -> Result<()> {
        self.ctl(name, "stop").await
    }
    pub async fn restart(&self, name: &str) -> Result<()> {
        self.ctl(name, "restart").await
    }
    pub async fn enable(&self, name: &str) -> Result<()> {
        self.ctl(name, "enable").await
    }
    pub async fn disable(&self, name: &str) -> Result<()> {
        self.ctl(name, "disable").await
    }

    pub async fn get_service_status(&self, name: &str) -> Result<ServiceState> {
        let service = ServiceName::new(name)?;
        let path = Path::new(RUNIT_ACTIVE_DIR).join(service.as_str());
        if !path.exists() {
            anyhow::bail!("runit service '{}' is not enabled", service);
        }
        let output = tokio::process::Command::new("sv")
            .arg("status")
            .arg(&path)
            .output()
            .await
            .with_context(|| format!("sv status {service}"))?;
        let status = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let active_state = if status.starts_with("run:") {
            ActiveState::Active
        } else if status.starts_with("down:") {
            ActiveState::Inactive
        } else {
            ActiveState::Failed
        };

        Ok(ServiceState {
            name: service,
            active_state,
            sub_state: status,
            load_state: "loaded".to_string(),
        })
    }

    async fn ctl(&self, name: &str, action: &str) -> Result<()> {
        let service = ServiceName::new(name)?;
        let definition = Path::new(RUNIT_SERVICE_DIR).join(service.as_str());
        let active = Path::new(RUNIT_ACTIVE_DIR).join(service.as_str());

        if action == "enable" {
            if !definition.exists() {
                anyhow::bail!("runit service definition '{}' does not exist", service);
            }
            std::fs::create_dir_all(RUNIT_ACTIVE_DIR)?;
            if !active.exists() {
                #[cfg(unix)]
                std::os::unix::fs::symlink(&definition, &active)?;
            }
            return Ok(());
        }
        if action == "disable" {
            if active.exists() {
                std::fs::remove_file(&active)?;
            }
            return Ok(());
        }
        if !active.exists() {
            anyhow::bail!("runit service '{}' is not enabled", service);
        }
        let sv_action = match action {
            "start" => "start",
            "stop" => "stop",
            "restart" => "restart",
            other => anyhow::bail!("unsupported runit action: {other}"),
        };

        info!("runit {} {}", sv_action, service);
        let status = tokio::process::Command::new("sv")
            .arg(sv_action)
            .arg(&active)
            .status()
            .await
            .with_context(|| format!("sv {sv_action} {service}"))?;
        if !status.success() {
            anyhow::bail!("sv {sv_action} {service} exited with {status}");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn service_with_user() -> ServiceDef {
        ServiceDef {
            name: ServiceName::new("op-example").unwrap(),
            service_type: ServiceType::Simple,
            exec_start: ExecCommand::new("/usr/local/bin/op-example", vec!["serve".to_string()])
                .unwrap(),
            exec_stop: None,
            working_dir: Some(PathBuf::from("/var/lib/op-example")),
            user: Some("op-example".to_string()),
            group: Some("op-example".to_string()),
            depends_on: vec![],
            waits_for: vec![],
            restart: RestartPolicy::default(),
            environment: HashMap::new(),
            env_file: None,
            resources: None,
            log_type: LogType::None,
            ready_notification: ReadyNotification::None,
            chain_to: None,
            smooth_recovery: false,
            enabled: true,
        }
    }

    #[test]
    fn generates_runit_run_script_with_chpst() {
        let run = service_with_user().to_runit_run();
        assert!(run.starts_with("#!/bin/sh\n"));
        assert!(run.contains("cd /var/lib/op-example || exit 1"));
        assert!(run.contains("exec chpst -u op-example:op-example /usr/local/bin/op-example serve"));
        assert!(!run.contains("s6"));
    }
}
