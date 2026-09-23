//! Network-manager authority through the runit D-Bus lifecycle gate.
use anyhow::{ensure, Context, Result};
use zbus::{Connection, Proxy};

pub struct NetworkAuthority;

impl NetworkAuthority {
    const COMPETING_MANAGERS: &'static [&'static str] =
        &["NetworkManager", "connmand", "dhcpcd", "wicd"];

    async fn proxy(connection: &Connection) -> Result<Proxy<'_>> {
        Ok(Proxy::new(
            connection,
            "org.opdbus.v1.Runit.Systemctl",
            "/org/opdbus/v1/plugins/runit/systemctl",
            "org.opdbus.v1.Runit.Systemctl",
        )
        .await?)
    }

    /// Stop and disable competing managers through the owner service. The owner
    /// authenticates the D-Bus sender; denied or failed transitions stay errors.
    pub async fn enforce_authority() -> Result<()> {
        let connection = Connection::system()
            .await
            .context("Connect to runit owner")?;
        let proxy = Self::proxy(&connection).await?;
        for service in Self::COMPETING_MANAGERS {
            let (success, message): (bool, String) = proxy.call("Disable", &(service,)).await?;
            ensure!(success, "Failed to disable {service}: {message}");
        }
        Ok(())
    }

    /// Read manager status from the D-Bus owner, never from host processes/files.
    pub async fn check_authority() -> Result<Vec<String>> {
        let connection = Connection::system()
            .await
            .context("Connect to runit owner")?;
        let proxy = Self::proxy(&connection).await?;
        let mut violations = Vec::new();
        for service in Self::COMPETING_MANAGERS {
            let status: String = proxy.call("IsActive", &(service,)).await?;
            if status == "active" {
                violations.push(format!("{service} is active"));
            }
        }
        Ok(violations)
    }
}
