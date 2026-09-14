//! PackageKit D-Bus tools (native, no CLI fallbacks).
//!
//! These tools use org.freedesktop.PackageKit over D-Bus via zbus.

use anyhow::Result;
use async_trait::async_trait;
use simd_json::{json, OwnedValue as Value};
use std::sync::Arc;
use zbus::Connection;

use crate::{Tool, ToolRegistry};

pub struct DbusPackageKitInstallTool;

#[async_trait]
impl Tool for DbusPackageKitInstallTool {
    fn name(&self) -> &str {
        "dbus_packagekit_install_packages"
    }

    fn description(&self) -> &str {
        "Install packages via PackageKit D-Bus (no CLI)"
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "packages": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Package IDs (e.g., name;version;arch;repo)"
                },
                "transaction_flags": {
                    "type": "integer",
                    "description": "PackageKit transaction flags",
                    "default": 0
                }
            },
            "required": ["packages"]
        })
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let packages = input
            .get("packages")
            .and_then(|v| v.as_array())
            .ok_or_else(|| anyhow::anyhow!("Missing required parameter: packages"))?
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect::<Vec<String>>();

        if packages.is_empty() {
            return Err(anyhow::anyhow!("packages must be a non-empty array of strings"));
        }

        let flags = input
            .get("transaction_flags")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);

        let tx_path = create_transaction().await?;
        install_packages(&tx_path, flags, &packages).await?;

        Ok(json!({
            "installed": packages,
            "transaction": tx_path,
            "protocol": "D-Bus"
        }))
    }

    fn category(&self) -> &str {
        "packagekit"
    }
}

pub struct DbusPackageKitRemoveTool;

#[async_trait]
impl Tool for DbusPackageKitRemoveTool {
    fn name(&self) -> &str {
        "dbus_packagekit_remove_packages"
    }

    fn description(&self) -> &str {
        "Remove packages via PackageKit D-Bus (no CLI)"
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "packages": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Package IDs (e.g., name;version;arch;repo)"
                },
                "transaction_flags": {
                    "type": "integer",
                    "description": "PackageKit transaction flags",
                    "default": 0
                },
                "allow_deps": {
                    "type": "boolean",
                    "description": "Allow removing dependent packages",
                    "default": true
                },
                "autoremove": {
                    "type": "boolean",
                    "description": "Auto-remove unused dependencies",
                    "default": false
                }
            },
            "required": ["packages"]
        })
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let packages = input
            .get("packages")
            .and_then(|v| v.as_array())
            .ok_or_else(|| anyhow::anyhow!("Missing required parameter: packages"))?
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect::<Vec<String>>();

        if packages.is_empty() {
            return Err(anyhow::anyhow!("packages must be a non-empty array of strings"));
        }

        let flags = input
            .get("transaction_flags")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let allow_deps = input
            .get("allow_deps")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        let autoremove = input
            .get("autoremove")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let tx_path = create_transaction().await?;
        remove_packages(&tx_path, flags, &packages, allow_deps, autoremove).await?;

        Ok(json!({
            "removed": packages,
            "transaction": tx_path,
            "protocol": "D-Bus"
        }))
    }

    fn category(&self) -> &str {
        "packagekit"
    }
}

async fn create_transaction() -> Result<String> {
    let connection = Connection::system().await?;
    let proxy = zbus::Proxy::new(
        &connection,
        "org.freedesktop.PackageKit",
        "/org/freedesktop/PackageKit",
        "org.freedesktop.PackageKit",
    )
    .await?;

    let path: zbus::zvariant::OwnedObjectPath =
        proxy.call("CreateTransaction", &()).await?;
    Ok(path.to_string())
}

async fn install_packages(tx_path: &str, flags: u64, packages: &[String]) -> Result<()> {
    let connection = Connection::system().await?;
    let proxy = zbus::Proxy::new(
        &connection,
        "org.freedesktop.PackageKit",
        tx_path,
        "org.freedesktop.PackageKit.Transaction",
    )
    .await?;

    let _: () = proxy
        .call("InstallPackages", &(flags, packages.to_vec()))
        .await?;
    Ok(())
}

async fn remove_packages(
    tx_path: &str,
    flags: u64,
    packages: &[String],
    allow_deps: bool,
    autoremove: bool,
) -> Result<()> {
    let connection = Connection::system().await?;
    let proxy = zbus::Proxy::new(
        &connection,
        "org.freedesktop.PackageKit",
        tx_path,
        "org.freedesktop.PackageKit.Transaction",
    )
    .await?;

    let _: () = proxy
        .call(
            "RemovePackages",
            &(flags, packages.to_vec(), allow_deps, autoremove),
        )
        .await?;
    Ok(())
}

/// Register PackageKit tools.
pub async fn register_packagekit_tools(registry: &ToolRegistry) -> Result<()> {
    registry.register_tool(Arc::new(DbusPackageKitInstallTool)).await?;
    registry.register_tool(Arc::new(DbusPackageKitRemoveTool)).await?;
    Ok(())
}
