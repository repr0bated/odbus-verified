//! Core runit service manager.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use tracing::{info, warn};

use crate::schema::{ManagerState, ServiceDef, ServiceName, ServiceStatus};
use crate::store::Store;

const RUNIT_SERVICE_DIR: &str = "/etc/runit/sv";
const RUNIT_ACTIVE_DIR: &str = "/etc/runit/runsvdir/default";

fn service_path(base: &str, name: &ServiceName) -> PathBuf {
    Path::new(base).join(name.as_str())
}

async fn sv(action: &str, name: &ServiceName) -> anyhow::Result<std::process::Output> {
    let active = service_path(RUNIT_ACTIVE_DIR, name);
    if !active.exists() {
        anyhow::bail!("runit service '{}' is not enabled", name);
    }
    tokio::process::Command::new("sv")
        .arg(action)
        .arg(&active)
        .output()
        .await
        .map_err(|e| anyhow::anyhow!("failed to run sv {action} {name}: {e}"))
}

fn parse_runit_pid(status: &str) -> Option<u32> {
    status
        .split("(pid ")
        .nth(1)
        .and_then(|tail| tail.split(')').next())
        .and_then(|pid| pid.parse().ok())
}

pub struct ServiceManager {
    store: Arc<Store>,
    statuses: Arc<RwLock<HashMap<ServiceName, ServiceStatus>>>,
    events: broadcast::Sender<ServiceEvent>,
}

#[derive(Debug, Clone)]
pub struct ServiceEvent {
    pub name: ServiceName,
    pub old_state: ManagerState,
    pub new_state: ManagerState,
}

impl ServiceManager {
    pub async fn new(store: Arc<Store>) -> anyhow::Result<Self> {
        if !Path::new(RUNIT_ACTIVE_DIR).is_dir() {
            anyhow::bail!("runit active service directory not found at {RUNIT_ACTIVE_DIR}");
        }
        info!("runit active service directory found at {RUNIT_ACTIVE_DIR}");

        let (events, _) = broadcast::channel(256);

        Ok(Self {
            store,
            statuses: Arc::new(RwLock::new(HashMap::new())),
            events,
        })
    }

    pub async fn start(&self, name: &ServiceName) -> anyhow::Result<ServiceStatus> {
        self.store
            .get_service(name)
            .await?
            .ok_or_else(|| anyhow::anyhow!("service not found: {}", name))?;

        self.set_state(name, ManagerState::Starting).await;

        let out = sv("start", name).await?;
        let result: anyhow::Result<u32> = if out.status.success() {
            let status = String::from_utf8_lossy(&out.stdout);
            Ok(parse_runit_pid(&status).unwrap_or(0))
        } else {
            let error = String::from_utf8_lossy(&out.stderr).trim().to_string();
            Err(anyhow::anyhow!("sv start {} failed: {}", name, error))
        };

        match result {
            Ok(pid) => {
                self.set_state_with_pid(name, ManagerState::Running, pid)
                    .await;
            }
            Err(e) => {
                self.set_state_with_error(name, ManagerState::Failed, e.to_string())
                    .await;
            }
        }

        self.get_status(name).await
    }

    pub async fn stop(&self, name: &ServiceName) -> anyhow::Result<ServiceStatus> {
        self.set_state(name, ManagerState::Stopping).await;

        let out = sv("stop", name).await?;
        let result: anyhow::Result<()> = if out.status.success() {
            Ok(())
        } else {
            let error = String::from_utf8_lossy(&out.stderr).trim().to_string();
            Err(anyhow::anyhow!("sv stop {} failed: {}", name, error))
        };

        match result {
            Ok(()) => self.set_state(name, ManagerState::Stopped).await,
            Err(e) => {
                self.set_state_with_error(name, ManagerState::Failed, e.to_string())
                    .await
            }
        }

        self.get_status(name).await
    }

    pub async fn restart(&self, name: &ServiceName) -> anyhow::Result<ServiceStatus> {
        let out = sv("restart", name).await?;
        if !out.status.success() {
            let error = String::from_utf8_lossy(&out.stderr).trim().to_string();
            self.set_state_with_error(name, ManagerState::Failed, error.clone())
                .await;
            anyhow::bail!("sv restart {} failed: {}", name, error);
        }
        let status = String::from_utf8_lossy(&out.stdout);
        self.set_state_with_pid(
            name,
            ManagerState::Running,
            parse_runit_pid(&status).unwrap_or(0),
        )
        .await;
        self.get_status(name).await
    }

    pub async fn get_status(&self, name: &ServiceName) -> anyhow::Result<ServiceStatus> {
        let active = service_path(RUNIT_ACTIVE_DIR, name);
        if !active.exists() {
            return Ok(ServiceStatus {
                name: name.clone(),
                state: ManagerState::Stopped,
                pid: None,
                error: Some("service is not enabled in runit".to_string()),
                started_at: None,
            });
        }
        let out = sv("status", name).await?;
        let status = String::from_utf8_lossy(&out.stdout).trim().to_string();
        let state = if status.starts_with("run:") {
            ManagerState::Running
        } else if status.starts_with("down:") {
            ManagerState::Stopped
        } else {
            ManagerState::Failed
        };
        Ok(ServiceStatus {
            name: name.clone(),
            state,
            pid: parse_runit_pid(&status),
            error: (!out.status.success()).then_some(status),
            started_at: None,
        })
    }

    pub async fn get(&self, name: &ServiceName) -> anyhow::Result<Option<ServiceDef>> {
        self.store.get_service(name).await
    }

    pub async fn create(&self, service: &ServiceDef) -> anyhow::Result<()> {
        // Persist to the store and install the runit run script.
        self.store.save_service(service).await?;
        if let Err(e) = service.install() {
            warn!(
                "Failed to install runit service files for {}: {}",
                service.name, e
            );
        }
        Ok(())
    }

    pub async fn delete(&self, name: &ServiceName) -> anyhow::Result<()> {
        // Best-effort stop before removal
        if let Err(e) = self.stop(name).await {
            warn!("Failed to stop service {} before deletion: {}", name, e);
        }

        // Remove from store
        self.store.delete_service(name).await?;

        let active = service_path(RUNIT_ACTIVE_DIR, name);
        if let Err(e) = tokio::fs::remove_file(&active).await {
            if e.kind() != std::io::ErrorKind::NotFound {
                warn!("Failed to disable runit service {}: {}", name, e);
            }
        }
        let definition = service_path(RUNIT_SERVICE_DIR, name);
        if let Err(e) = tokio::fs::remove_dir_all(&definition).await {
            if e.kind() != std::io::ErrorKind::NotFound {
                warn!(
                    "Failed to remove runit service directory {}: {}",
                    definition.display(),
                    e
                );
            }
        }

        // Clear runtime status
        let mut statuses = self.statuses.write().await;
        statuses.remove(name);

        Ok(())
    }

    pub async fn set_enabled(&self, name: &ServiceName, enabled: bool) -> anyhow::Result<()> {
        let mut service = self
            .store
            .get_service(name)
            .await?
            .ok_or_else(|| anyhow::anyhow!("service not found: {}", name))?;

        service.enabled = enabled;
        self.store.save_service(&service).await?;
        let active = service_path(RUNIT_ACTIVE_DIR, name);
        if enabled {
            service.install()?;
        } else {
            let _ = self.stop(name).await;
            if let Err(e) = tokio::fs::remove_file(&active).await {
                if e.kind() != std::io::ErrorKind::NotFound {
                    return Err(e.into());
                }
            }
        }
        Ok(())
    }

    pub async fn list(&self) -> anyhow::Result<Vec<ServiceDef>> {
        self.store.list_services().await
    }

    pub fn subscribe(&self) -> broadcast::Receiver<ServiceEvent> {
        self.events.subscribe()
    }

    async fn set_state(&self, name: &ServiceName, state: ManagerState) {
        let mut statuses = self.statuses.write().await;
        let old_state = statuses
            .get(name)
            .map(|s| s.state.clone())
            .unwrap_or(ManagerState::Stopped);

        let status = statuses
            .entry(name.clone())
            .or_insert_with(|| ServiceStatus {
                name: name.clone(),
                state: ManagerState::Stopped,
                pid: None,
                error: None,
                started_at: None,
            });
        status.state = state.clone();
        status.error = None;

        if matches!(state, ManagerState::Running) {
            status.started_at = Some(chrono::Utc::now());
        }

        let _ = self.events.send(ServiceEvent {
            name: name.clone(),
            old_state,
            new_state: state,
        });
    }

    async fn set_state_with_pid(&self, name: &ServiceName, state: ManagerState, pid: u32) {
        let mut statuses = self.statuses.write().await;
        let status = statuses
            .entry(name.clone())
            .or_insert_with(|| ServiceStatus {
                name: name.clone(),
                state: ManagerState::Stopped,
                pid: None,
                error: None,
                started_at: None,
            });
        status.state = state;
        status.pid = Some(pid);
        status.started_at = Some(chrono::Utc::now());
    }

    async fn set_state_with_error(&self, name: &ServiceName, state: ManagerState, error: String) {
        let mut statuses = self.statuses.write().await;
        let status = statuses
            .entry(name.clone())
            .or_insert_with(|| ServiceStatus {
                name: name.clone(),
                state: ManagerState::Stopped,
                pid: None,
                error: None,
                started_at: None,
            });
        status.state = state;
        status.error = Some(error);
    }
}

#[cfg(test)]
mod tests {
    use super::parse_runit_pid;

    #[test]
    fn parses_sv_status_pid() {
        assert_eq!(
            parse_runit_pid("run: /etc/runit/runsvdir/default/op-web: (pid 4242) 12s"),
            Some(4242)
        );
        assert_eq!(
            parse_runit_pid("down: /etc/runit/runsvdir/default/op-web: 2s"),
            None
        );
    }
}
