//! DBus connection management

use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};
use zbus::Connection;

use crate::error::{Error, Result};
use crate::types::BusType;

/// Configuration for DBus connections
#[derive(Debug, Clone)]
pub struct ConnectionConfig {
    /// Whether to auto-reconnect on connection loss
    pub auto_reconnect: bool,
    /// Connection timeout in milliseconds
    pub timeout_ms: u64,
    /// Maximum retry attempts for connection
    pub max_retries: u32,
}

impl Default for ConnectionConfig {
    fn default() -> Self {
        Self {
            auto_reconnect: true,
            timeout_ms: 30000,
            max_retries: 3,
        }
    }
}

/// DBus connection manager
///
/// Manages connections to both system and session buses with
/// automatic reconnection support.
pub struct DbusConnection {
    system: Arc<RwLock<Option<Connection>>>,
    session: Arc<RwLock<Option<Connection>>>,
    config: ConnectionConfig,
}

impl DbusConnection {
    /// Create a new DBus connection manager
    pub fn new(config: ConnectionConfig) -> Self {
        Self {
            system: Arc::new(RwLock::new(None)),
            session: Arc::new(RwLock::new(None)),
            config,
        }
    }

    /// Create with default configuration
    pub fn with_defaults() -> Self {
        Self::new(ConnectionConfig::default())
    }

    /// Connect to the system bus
    pub async fn connect_system(&self) -> Result<Connection> {
        let mut conn = self.system.write().await;

        if let Some(ref existing) = *conn {
            // Check if connection is still valid by attempting to get the unique name
            if existing.unique_name().is_some() {
                debug!("Reusing existing system bus connection");
                return Ok(existing.clone());
            }
            warn!("System bus connection lost, reconnecting...");
        }

        let new_conn = self.try_connect(BusType::System).await?;
        *conn = Some(new_conn.clone());
        info!("Connected to system bus");
        Ok(new_conn)
    }

    /// Connect to the session bus
    pub async fn connect_session(&self) -> Result<Connection> {
        let mut conn = self.session.write().await;

        if let Some(ref existing) = *conn {
            // Check if connection is still valid by attempting to get the unique name
            if existing.unique_name().is_some() {
                debug!("Reusing existing session bus connection");
                return Ok(existing.clone());
            }
            warn!("Session bus connection lost, reconnecting...");
        }

        let new_conn = self.try_connect(BusType::Session).await?;
        *conn = Some(new_conn.clone());
        info!("Connected to session bus");
        Ok(new_conn)
    }

    /// Get connection for specified bus type
    pub async fn get(&self, bus_type: BusType) -> Result<Connection> {
        match bus_type {
            BusType::System => self.connect_system().await,
            BusType::Session => self.connect_session().await,
        }
    }

    /// Check if system bus is connected
    pub async fn is_system_connected(&self) -> bool {
        let conn = self.system.read().await;
        conn.as_ref().is_some_and(|c| c.unique_name().is_some())
    }

    /// Check if session bus is connected
    pub async fn is_session_connected(&self) -> bool {
        let conn = self.session.read().await;
        conn.as_ref().is_some_and(|c| c.unique_name().is_some())
    }

    /// Disconnect from all buses
    pub async fn disconnect(&self) {
        let mut system = self.system.write().await;
        let mut session = self.session.write().await;

        *system = None;
        *session = None;
        info!("Disconnected from all DBus connections");
    }

    /// Try to establish connection with retries
    async fn try_connect(&self, bus_type: BusType) -> Result<Connection> {
        let mut last_error = None;

        for attempt in 1..=self.config.max_retries {
            debug!("Connection attempt {} for {:?} bus", attempt, bus_type);

            let result = match bus_type {
                BusType::System => Connection::system().await,
                BusType::Session => Connection::session().await,
            };

            match result {
                Ok(conn) => return Ok(conn),
                Err(e) => {
                    warn!("Connection attempt {} failed: {}", attempt, e);
                    last_error = Some(e);

                    if attempt < self.config.max_retries {
                        // Exponential backoff
                        let delay = std::time::Duration::from_millis(100 * 2u64.pow(attempt - 1));
                        tokio::time::sleep(delay).await;
                    }
                }
            }
        }

        Err(Error::Connection(format!(
            "Failed to connect to {:?} bus after {} attempts: {:?}",
            bus_type, self.config.max_retries, last_error
        )))
    }
}

impl Default for DbusConnection {
    fn default() -> Self {
        Self::with_defaults()
    }
}

impl Clone for DbusConnection {
    fn clone(&self) -> Self {
        Self {
            system: Arc::clone(&self.system),
            session: Arc::clone(&self.session),
            config: self.config.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_connection_config_default() {
        let config = ConnectionConfig::default();
        assert!(config.auto_reconnect);
        assert_eq!(config.timeout_ms, 30000);
        assert_eq!(config.max_retries, 3);
    }
}
