use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use zbus::{Connection, Proxy};

/// systemd-networkd integration for the network plugin
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemdNetworkdConfig {
    pub enabled: bool,
    pub networks: HashMap<String, NetworkConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    pub match_name: String,
    pub dhcp: Option<String>,
    pub address: Option<Vec<String>>,
    pub gateway: Option<String>,
    pub dns: Option<Vec<String>>,
    pub bridge: Option<String>,
    pub vlan: Option<u16>,
}

pub struct SystemdNetworkdManager {
    connection: Option<Connection>,
}

impl SystemdNetworkdManager {
    pub async fn new() -> Result<Self> {
        let connection = Connection::system().await.ok();
        Ok(Self { connection })
    }

    /// Generate .network files from plugin configuration
    pub fn generate_network_files(&self, config: &SystemdNetworkdConfig) -> Result<()> {
        if !config.enabled {
            return Ok(());
        }

        let network_dir = Path::new("/etc/systemd/network");
        fs::create_dir_all(network_dir)?;

        for (name, net_config) in &config.networks {
            let content = self.generate_network_file_content(net_config)?;
            let file_path = network_dir.join(format!("50-{}.network", name));
            fs::write(file_path, content)?;
        }

        Ok(())
    }

    fn generate_network_file_content(&self, config: &NetworkConfig) -> Result<String> {
        let mut content = String::new();

        // [Match] section
        content.push_str("[Match]\n");
        content.push_str(&format!("Name={}\n\n", config.match_name));

        // [Network] section
        content.push_str("[Network]\n");

        if let Some(dhcp) = &config.dhcp {
            content.push_str(&format!("DHCP={}\n", dhcp));
        }

        if let Some(bridge) = &config.bridge {
            content.push_str(&format!("Bridge={}\n", bridge));
        }

        if let Some(vlan) = config.vlan {
            content.push_str(&format!("VLAN={}\n", vlan));
        }

        if let Some(dns_servers) = &config.dns {
            for dns in dns_servers {
                content.push_str(&format!("DNS={}\n", dns));
            }
        }

        content.push('\n');

        // [Address] sections
        if let Some(addresses) = &config.address {
            for addr in addresses {
                content.push_str("[Address]\n");
                content.push_str(&format!("Address={}\n\n", addr));
            }
        }

        // [Route] section
        if let Some(gateway) = &config.gateway {
            content.push_str("[Route]\n");
            content.push_str(&format!("Gateway={}\n", gateway));
        }

        Ok(content)
    }

    /// Reload systemd-networkd configuration via D-Bus
    pub async fn reload_configuration(&self) -> Result<()> {
        if let Some(ref conn) = self.connection {
            let proxy = Proxy::new(
                conn,
                "org.freedesktop.systemd1",
                "/org/freedesktop/systemd1/unit/systemd_2dnetworkd_2eservice",
                "org.freedesktop.systemd1.Unit",
            )
            .await?;

            // Reload systemd-networkd
            let _: () = proxy.call("Reload", &("replace",)).await?;
            log::info!("systemd-networkd configuration reloaded");
        }
        Ok(())
    }

    /// Get network state from systemd-networkd via D-Bus
    pub async fn get_network_state(&self) -> Result<HashMap<String, String>> {
        let mut state = HashMap::new();

        if let Some(ref conn) = self.connection {
            // Try to connect to org.freedesktop.network1 if available
            if let Ok(proxy) = Proxy::new(
                conn,
                "org.freedesktop.network1",
                "/org/freedesktop/network1",
                "org.freedesktop.network1.Manager",
            )
            .await
            {
                // Get links
                if let Ok(links) = proxy.call("ListLinks", &()).await {
                    let links: Vec<(i32, String, String)> = links;
                    for (index, name, state_str) in links {
                        state.insert(format!("link.{}.{}", index, name), state_str);
                    }
                }
            }
        }

        Ok(state)
    }

    /// Start systemd-networkd if not running
    pub async fn ensure_running(&self) -> Result<()> {
        if let Some(ref conn) = self.connection {
            let proxy = Proxy::new(
                conn,
                "org.freedesktop.systemd1",
                "/org/freedesktop/systemd1",
                "org.freedesktop.systemd1.Manager",
            )
            .await?;

            // Start systemd-networkd
            let _: () = proxy
                .call("StartUnit", &("systemd-networkd.service", "replace"))
                .await?;
            log::info!("systemd-networkd started");
        }
        Ok(())
    }
}
