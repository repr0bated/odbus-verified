/// Network Authority Enforcement
///
/// This module ensures the plugin system remains the ultimate authoritative source
/// for all network configuration, preventing interference from legacy systems.
use anyhow::Result;
use std::process::Command;

pub struct NetworkAuthority;

impl NetworkAuthority {
    /// Services that would fight the plugin system for control of the network.
    /// Named as runit services (`/etc/runit/sv/<name>`).
    const COMPETING_MANAGERS: &'static [&'static str] =
        &["NetworkManager", "connmand", "dhcpcd", "wicd"];

    /// Ensure no competing network managers are active.
    ///
    /// This host runs runit, so control is `sv down` plus removal of the
    /// boot-enablement symlink; there is no systemd and no `systemctl`.
    /// Services that are not defined on this host are simply absent, and `sv`
    /// failing for them is expected and ignored.
    pub fn enforce_authority() -> Result<()> {
        for service in Self::COMPETING_MANAGERS {
            // Stop it now.
            let _ = Command::new(op_core::runit::SV_BIN)
                .args(["down", service])
                .output();
            // Keep it from coming back at boot: drop the runlevel symlink and
            // leave a `down` marker in the definition.
            let _ = std::fs::remove_file(op_core::runit::enabled_path(service));
            let definition = op_core::runit::definition_path(service);
            if std::path::Path::new(&definition).is_dir() {
                let _ = std::fs::write(format!("{definition}/down"), "");
            }
        }

        log::info!("Network authority enforced - plugin system is sole controller");
        Ok(())
    }

    /// Check for authority violations.
    pub fn check_authority() -> Result<Vec<String>> {
        let mut violations = Vec::new();

        for service in Self::COMPETING_MANAGERS {
            if let Ok(output) = Command::new(op_core::runit::SV_BIN)
                .args(["status", service])
                .output()
            {
                // `sv status` prints e.g. "run: NetworkManager: (pid 123) 4s".
                let status = String::from_utf8_lossy(&output.stdout);
                if status.trim_start().starts_with("run:") {
                    violations.push(format!("{service} is active"));
                }
            }
        }

        Ok(violations)
    }
}
