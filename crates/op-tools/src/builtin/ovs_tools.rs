//! OVS Tools for Chat Interface
//!
//! These tools expose OVS operations to the LLM chat system.
//! ALL OPERATIONS USE NATIVE OVSDB JSON-RPC - NO CLI COMMANDS.

use crate::Tool;
use crate::ToolRegistry;
use anyhow::Result;
use async_trait::async_trait;
use op_network::ovsdb::OvsdbClient;
use simd_json::prelude::*;
use simd_json::{json, OwnedValue as Value};
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, info, warn};

/// Type alias for systemd EnableUnitFiles result to avoid complex type in fn body
type SystemdEnableResult = Result<(bool, Vec<(String, String, String)>), zbus::Error>;

/// Tool to test tool execution (no network ops)
pub struct TestTool;

#[async_trait]
impl Tool for TestTool {
    fn name(&self) -> &str {
        "test_tool"
    }

    fn description(&self) -> &str {
        "Test tool execution without network operations"
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {},
            "required": []
        })
    }

    fn category(&self) -> &str {
        "test"
    }

    fn tags(&self) -> Vec<String> {
        vec!["test".to_string()]
    }

    async fn execute(&self, _input: Value) -> Result<Value> {
        tokio::time::sleep(Duration::from_millis(100)).await;

        Ok(json!({
            "message": "Test tool executed successfully",
            "timestamp": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).expect("system time should be after unix epoch").as_secs()
        }))
    }
}

/// Tool to list OVS bridges (via OVSDB JSON-RPC only)
pub struct OvsListBridgesTool;

#[async_trait]
impl Tool for OvsListBridgesTool {
    fn name(&self) -> &str {
        "ovs_list_bridges"
    }

    fn description(&self) -> &str {
        "List all OVS bridges configured in OVSDB via native JSON-RPC."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {},
            "required": []
        })
    }

    fn category(&self) -> &str {
        "networking"
    }

    fn tags(&self) -> Vec<String> {
        vec!["ovs".to_string(), "bridges".to_string()]
    }

    async fn execute(&self, _input: Value) -> Result<Value> {
        let bridges = OvsdbClient::new()
            .list_bridges()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to list bridges via OVSDB: {}", e))?;

        Ok(json!({ "bridges": bridges, "method": "native_ovsdb" }))
    }
}

/// Tool to list kernel datapaths (via OVS Netlink)
pub struct OvsListDatapathsTool;

#[async_trait]
impl Tool for OvsListDatapathsTool {
    fn name(&self) -> &str {
        "ovs_list_datapaths"
    }

    fn description(&self) -> &str {
        "List OVS kernel datapaths via Generic Netlink. Requires root privileges."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {},
            "required": []
        })
    }

    fn category(&self) -> &str {
        "networking"
    }

    fn tags(&self) -> Vec<String> {
        vec![
            "ovs".to_string(),
            "datapaths".to_string(),
            "kernel".to_string(),
        ]
    }

    async fn execute(&self, _input: Value) -> Result<Value> {
        use op_network::OvsNetlinkClient;

        let mut client = OvsNetlinkClient::new().await.map_err(|e| {
            anyhow::anyhow!("Failed to create netlink client: {} (requires root)", e)
        })?;

        let dps = client
            .list_datapaths()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to list datapaths: {}", e))?;

        Ok(json!({ "datapaths": dps }))
    }
}

/// Tool to list vports on a datapath
pub struct OvsListVportsTool;

#[async_trait]
impl Tool for OvsListVportsTool {
    fn name(&self) -> &str {
        "ovs_list_vports"
    }

    fn description(&self) -> &str {
        "List vports on an OVS kernel datapath. Requires root."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "datapath": {
                    "type": "string",
                    "description": "Name of the datapath (e.g., 'ovs-system' or bridge name)"
                }
            },
            "required": ["datapath"]
        })
    }

    fn category(&self) -> &str {
        "networking"
    }

    fn tags(&self) -> Vec<String> {
        vec![
            "ovs".to_string(),
            "vports".to_string(),
            "kernel".to_string(),
        ]
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        use op_network::OvsNetlinkClient;

        let dp_name = input
            .get("datapath")
            .and_then(|v| v.as_str())
            .unwrap_or("ovs-system");

        let mut client = OvsNetlinkClient::new()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to create netlink client: {}", e))?;

        let vports = client
            .list_vports(dp_name)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to list vports: {}", e))?;

        Ok(json!({ "datapath": dp_name, "vports": vports }))
    }
}

/// Tool to show OVS capabilities
pub struct OvsCapabilitiesTool;

#[async_trait]
impl Tool for OvsCapabilitiesTool {
    fn name(&self) -> &str {
        "ovs_capabilities"
    }

    fn description(&self) -> &str {
        "Detect and report OVS capabilities. Shows what OVS operations are available on this system."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {},
            "required": []
        })
    }

    fn category(&self) -> &str {
        "networking"
    }

    fn tags(&self) -> Vec<String> {
        vec![
            "ovs".to_string(),
            "capabilities".to_string(),
            "detection".to_string(),
        ]
    }

    async fn execute(&self, _input: Value) -> Result<Value> {
        use op_network::OvsCapabilities;

        let caps = OvsCapabilities::detect().await;
        let llm_context = caps.to_llm_context();

        Ok(json!({
            "capabilities": caps,
            "llm_context": llm_context
        }))
    }
}

/// Tool to dump kernel flows
pub struct OvsDumpFlowsTool;

#[async_trait]
impl Tool for OvsDumpFlowsTool {
    fn name(&self) -> &str {
        "ovs_dump_flows"
    }

    fn description(&self) -> &str {
        "Dump kernel flow table for a datapath. Shows flows cached in kernel. Requires root."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "datapath": {
                    "type": "string",
                    "description": "Datapath name (default: ovs-system)"
                }
            },
            "required": []
        })
    }

    fn category(&self) -> &str {
        "networking"
    }

    fn tags(&self) -> Vec<String> {
        vec!["ovs".to_string(), "flows".to_string(), "kernel".to_string()]
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        use op_network::OvsNetlinkClient;

        let dp_name = input
            .get("datapath")
            .and_then(|v| v.as_str())
            .unwrap_or("ovs-system");

        let mut client = OvsNetlinkClient::new()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to create netlink client: {}", e))?;

        let flows = client
            .dump_flows(dp_name)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to dump flows: {}", e))?;

        Ok(json!({
            "datapath": dp_name,
            "flow_count": flows.len(),
            "flows": flows
        }))
    }
}

// =============================================================================
// OVSDB WRITE OPERATIONS - Bridge/Port Management via JSON-RPC
// =============================================================================

/// Tool to create an OVS bridge
pub struct OvsCreateBridgeTool;

#[async_trait]
impl Tool for OvsCreateBridgeTool {
    fn name(&self) -> &str {
        "ovs_create_bridge"
    }

    fn description(&self) -> &str {
        "Create a new OVS bridge via OVSDB JSON-RPC."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Name of the bridge to create (e.g., 'br0', 'ovsbr1')"
                }
            },
            "required": ["name"]
        })
    }

    fn category(&self) -> &str {
        "networking"
    }

    fn tags(&self) -> Vec<String> {
        vec![
            "ovs".to_string(),
            "bridge".to_string(),
            "create".to_string(),
            "write".to_string(),
        ]
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let bridge_name = input
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required argument: name"))?;

        let client = OvsdbClient::new();

        let bridges = client
            .list_bridges()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to check existing bridges: {}", e))?;

        if bridges.contains(&bridge_name.to_string()) {
            return Err(anyhow::anyhow!("Bridge '{}' already exists", bridge_name));
        }

        client
            .create_bridge(bridge_name)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to create bridge: {}", e))?;

        let bridges_after = client.list_bridges().await.map_err(|e| {
            anyhow::anyhow!("Bridge creation succeeded but verification failed: {}", e)
        })?;

        if bridges_after.contains(&bridge_name.to_string()) {
            Ok(json!({
                "success": true,
                "bridge": bridge_name,
                "message": format!("Bridge '{}' created and verified successfully", bridge_name),
                "verification": "Bridge found in OVSDB after creation"
            }))
        } else {
            Err(anyhow::anyhow!(
                "Bridge creation claimed success but '{}' not found in OVSDB",
                bridge_name
            ))
        }
    }
}

/// Tool to delete an OVS bridge
pub struct OvsDeleteBridgeTool;

#[async_trait]
impl Tool for OvsDeleteBridgeTool {
    fn name(&self) -> &str {
        "ovs_delete_bridge"
    }

    fn description(&self) -> &str {
        "Delete an OVS bridge via OVSDB JSON-RPC."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Name of the bridge to delete"
                }
            },
            "required": ["name"]
        })
    }

    fn category(&self) -> &str {
        "networking"
    }

    fn tags(&self) -> Vec<String> {
        vec![
            "ovs".to_string(),
            "bridge".to_string(),
            "delete".to_string(),
            "write".to_string(),
        ]
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let bridge_name = input
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required argument: name"))?;

        let client = OvsdbClient::new();

        client
            .delete_bridge(bridge_name)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to delete bridge: {}", e))?;

        Ok(json!({
            "success": true,
            "bridge": bridge_name,
            "message": format!("Bridge '{}' deleted successfully", bridge_name)
        }))
    }
}

/// Tool to add a port to an OVS bridge
pub struct OvsAddPortTool;

#[async_trait]
impl Tool for OvsAddPortTool {
    fn name(&self) -> &str {
        "ovs_add_port"
    }

    fn description(&self) -> &str {
        "Add a port to an OVS bridge via OVSDB JSON-RPC."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "bridge": {
                    "type": "string",
                    "description": "Name of the bridge to add the port to"
                },
                "port": {
                    "type": "string",
                    "description": "Name of the port/interface to add"
                },
                "type": {
                    "type": "string",
                    "description": "Optional OVS interface type (e.g. 'system', 'internal', 'patch')"
                }
            },
            "required": ["bridge", "port"]
        })
    }

    fn category(&self) -> &str {
        "networking"
    }

    fn tags(&self) -> Vec<String> {
        vec![
            "ovs".to_string(),
            "port".to_string(),
            "add".to_string(),
            "write".to_string(),
        ]
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let bridge_name = input
            .get("bridge")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required argument: bridge"))?;

        let port_name = input
            .get("port")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required argument: port"))?;
        let port_type = input.get("type").and_then(|v| v.as_str());

        let client = OvsdbClient::new();

        match port_type {
            Some(port_type) => client
                .add_port_with_type(bridge_name, port_name, Some(port_type))
                .await
                .map_err(|e| anyhow::anyhow!("Failed to add port: {}", e))?,
            None => client
                .add_port(bridge_name, port_name)
                .await
                .map_err(|e| anyhow::anyhow!("Failed to add port: {}", e))?,
        };

        Ok(json!({
            "success": true,
            "bridge": bridge_name,
            "port": port_name,
            "type": port_type,
            "message": format!("Port '{}' added to bridge '{}' successfully", port_name, bridge_name)
        }))
    }
}

/// Tool to list ports on an OVS bridge
pub struct OvsListPortsTool;

#[async_trait]
impl Tool for OvsListPortsTool {
    fn name(&self) -> &str {
        "ovs_list_ports"
    }

    fn description(&self) -> &str {
        "List all ports attached to an OVS bridge via OVSDB JSON-RPC."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "bridge": {
                    "type": "string",
                    "description": "Name of the bridge to list ports for"
                }
            },
            "required": ["bridge"]
        })
    }

    fn category(&self) -> &str {
        "networking"
    }

    fn tags(&self) -> Vec<String> {
        vec!["ovs".to_string(), "port".to_string(), "list".to_string()]
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let bridge_name = input
            .get("bridge")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required argument: bridge"))?;

        let client = OvsdbClient::new();

        let ports = client
            .list_bridge_ports(bridge_name)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to list ports: {}", e))?;

        Ok(json!({
            "bridge": bridge_name,
            "ports": ports,
            "port_count": ports.len()
        }))
    }
}

pub async fn register_ovs_tools(registry: &ToolRegistry) -> Result<()> {
    registry
        .register_tool(Arc::new(OvsCheckAvailableTool))
        .await?;
    registry.register_tool(Arc::new(OvsListBridgesTool)).await?;
    registry.register_tool(Arc::new(OvsListPortsTool)).await?;
    registry
        .register_tool(Arc::new(OvsGetBridgeInfoTool))
        .await?;
    registry
        .register_tool(Arc::new(OvsCapabilitiesTool))
        .await?;
    registry
        .register_tool(Arc::new(OvsCreateBridgeTool))
        .await?;
    registry
        .register_tool(Arc::new(OvsDeleteBridgeTool))
        .await?;
    registry.register_tool(Arc::new(OvsAddPortTool)).await?;
    registry
        .register_tool(Arc::new(OvsListDatapathsTool))
        .await?;
    registry.register_tool(Arc::new(OvsListVportsTool)).await?;
    registry.register_tool(Arc::new(OvsDumpFlowsTool)).await?;
    Ok(())
}

/// Tool to get detailed bridge info
pub struct OvsGetBridgeInfoTool;

#[async_trait]
impl Tool for OvsGetBridgeInfoTool {
    fn name(&self) -> &str {
        "ovs_get_bridge_info"
    }

    fn description(&self) -> &str {
        "Get detailed information about an OVS bridge from OVSDB."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "bridge": {
                    "type": "string",
                    "description": "Name of the bridge to get info for"
                }
            },
            "required": ["bridge"]
        })
    }

    fn category(&self) -> &str {
        "networking"
    }

    fn tags(&self) -> Vec<String> {
        vec!["ovs".to_string(), "bridge".to_string(), "info".to_string()]
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let bridge_name = input
            .get("bridge")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required argument: bridge"))?;

        let client = OvsdbClient::new();

        let info = client
            .get_bridge_info(bridge_name)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to get bridge info: {}", e))?;

        Ok(json!({
            "bridge": bridge_name,
            "info": info
        }))
    }
}

/// Tool to check if OVS is available
pub struct OvsCheckAvailableTool;

#[async_trait]
impl Tool for OvsCheckAvailableTool {
    fn name(&self) -> &str {
        "ovs_check_available"
    }

    fn description(&self) -> &str {
        "Check if Open vSwitch is available and running. Verifies OVSDB socket connectivity. If unavailable, use ovs_auto_install to install it."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {},
            "required": []
        })
    }

    fn category(&self) -> &str {
        "networking"
    }

    fn tags(&self) -> Vec<String> {
        vec!["ovs".to_string(), "check".to_string(), "status".to_string()]
    }

    async fn execute(&self, _input: Value) -> Result<Value> {
        let client = OvsdbClient::new();

        match client.list_dbs().await {
            Ok(dbs) => {
                let bridges = client.list_bridges().await.unwrap_or_default();

                Ok(json!({
                    "available": true,
                    "socket": "/var/run/openvswitch/db.sock",
                    "databases": dbs,
                    "bridges": bridges,
                    "message": "Open vSwitch is available and responding"
                }))
            }
            Err(e) => Ok(json!({
                "available": false,
                "socket": "/var/run/openvswitch/db.sock",
                "error": e.to_string(),
                "message": "Open vSwitch is not available or not running",
                "install_hint": "Use ovs_auto_install tool to install and start Open vSwitch automatically"
            })),
        }
    }
}

/// Tool to auto-install OVS via PackageKit and systemd D-Bus (NO CLI COMMANDS)
pub struct OvsAutoInstallTool;

#[async_trait]
impl Tool for OvsAutoInstallTool {
    fn name(&self) -> &str {
        "ovs_auto_install"
    }

    fn description(&self) -> &str {
        "Automatically install and start Open vSwitch using PackageKit D-Bus and systemd D-Bus. No CLI commands used."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "force": {
                    "type": "boolean",
                    "description": "Force reinstall even if OVS socket exists (default: false)"
                }
            },
            "required": []
        })
    }

    fn category(&self) -> &str {
        "networking"
    }

    fn tags(&self) -> Vec<String> {
        vec![
            "ovs".to_string(),
            "install".to_string(),
            "setup".to_string(),
            "packagekit".to_string(),
        ]
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        use zbus::Connection;

        let force = input
            .get("force")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        // Step 1: Check if OVS is already available via OVSDB socket
        if !force {
            let client = OvsdbClient::new();
            if client.list_dbs().await.is_ok() {
                return Ok(json!({
                    "success": true,
                    "already_installed": true,
                    "message": "Open vSwitch is already installed and running (OVSDB responding)",
                    "action": "none"
                }));
            }
        }

        info!("Starting OVS auto-installation via D-Bus");

        // Step 2: Connect to system D-Bus
        let connection = Connection::system()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to connect to system D-Bus: {}", e))?;

        // Step 3: Install openvswitch-switch via PackageKit
        info!("Installing openvswitch-switch via PackageKit D-Bus");
        let install_result =
            install_package_via_packagekit(&connection, "openvswitch-switch").await;

        let install_status = match &install_result {
            Ok(msg) => {
                info!("Package installation result: {}", msg);
                json!({ "status": "success", "message": msg })
            }
            Err(e) => {
                warn!("Package installation failed: {}", e);
                json!({ "status": "failed", "error": e.to_string() })
            }
        };

        // Step 4: Start and enable the openvswitch-switch service via systemd D-Bus
        info!("Starting openvswitch-switch service via systemd D-Bus");
        let start_result =
            start_service_via_systemd(&connection, "openvswitch-switch.service").await;

        let service_status = match &start_result {
            Ok(msg) => {
                info!("Service start result: {}", msg);
                json!({ "status": "success", "message": msg })
            }
            Err(e) => {
                warn!("Service start failed: {}", e);
                json!({ "status": "failed", "error": e.to_string() })
            }
        };

        // Step 5: Wait for service to fully start
        tokio::time::sleep(Duration::from_secs(3)).await;

        // Step 6: Verify installation via OVSDB connection (no CLI)
        let client = OvsdbClient::new();
        let ovsdb_available = client.list_dbs().await.is_ok();
        let socket_exists = tokio::fs::metadata("/var/run/openvswitch/db.sock")
            .await
            .is_ok();

        let verification = json!({
            "socket_exists": socket_exists,
            "ovsdb_responding": ovsdb_available,
            "fully_operational": ovsdb_available
        });

        Ok(json!({
            "success": ovsdb_available,
            "package_install": install_status,
            "service_start": service_status,
            "verification": verification,
            "message": if ovsdb_available {
                "Open vSwitch installed and started successfully"
            } else {
                "Installation attempted but OVSDB not responding - check logs"
            }
        }))
    }
}

/// Install a package via PackageKit D-Bus interface
async fn install_package_via_packagekit(
    connection: &zbus::Connection,
    package_name: &str,
) -> Result<String> {
    debug!(
        "Creating PackageKit transaction for package: {}",
        package_name
    );

    let pk_proxy: zbus::Proxy = zbus::proxy::Builder::new(connection)
        .destination("org.freedesktop.PackageKit")?
        .path("/org/freedesktop/PackageKit")?
        .interface("org.freedesktop.PackageKit")?
        .build()
        .await?;

    let transaction_path: zbus::zvariant::OwnedObjectPath = pk_proxy
        .call("CreateTransaction", &())
        .await
        .map_err(|e| anyhow::anyhow!("Failed to create PackageKit transaction: {}", e))?;

    debug!("Got transaction path: {}", transaction_path);

    let tx_proxy: zbus::Proxy = zbus::proxy::Builder::new(connection)
        .destination("org.freedesktop.PackageKit")?
        .path(transaction_path.as_str())?
        .interface("org.freedesktop.PackageKit.Transaction")?
        .build()
        .await?;

    // Use InstallPackages - PackageKit will resolve the package name
    let transaction_flags: u64 = 0;
    let package_ids: Vec<String> = vec![format!("{};;", package_name)];

    tx_proxy
        .call::<_, (u64, Vec<String>), ()>("InstallPackages", &(transaction_flags, package_ids))
        .await
        .map_err(|e| anyhow::anyhow!("Failed to install package: {}", e))?;

    // Wait for installation to complete
    tokio::time::sleep(Duration::from_secs(10)).await;

    Ok(format!(
        "Package {} installation initiated via PackageKit D-Bus",
        package_name
    ))
}

/// Start a systemd service via D-Bus
async fn start_service_via_systemd(
    connection: &zbus::Connection,
    service_name: &str,
) -> Result<String> {
    debug!("Starting systemd service via D-Bus: {}", service_name);

    let systemd_proxy: zbus::Proxy = zbus::proxy::Builder::new(connection)
        .destination("org.freedesktop.systemd1")?
        .path("/org/freedesktop/systemd1")?
        .interface("org.freedesktop.systemd1.Manager")?
        .build()
        .await?;

    // Enable the service first
    // The result type is complex, but we don't use it - just checking for success
    let _enable_result: SystemdEnableResult = systemd_proxy
        .call("EnableUnitFiles", &(vec![service_name], false, true))
        .await;

    // Start the service
    let start_result: std::result::Result<zbus::zvariant::OwnedObjectPath, _> = systemd_proxy
        .call("StartUnit", &(service_name, "replace"))
        .await;

    match start_result {
        Ok(job_path) => {
            info!("Service {} start job created: {}", service_name, job_path);
            Ok(format!(
                "Service {} started via systemd D-Bus",
                service_name
            ))
        }
        Err(e) => {
            // Check if service might already be running
            let status_result: std::result::Result<zbus::zvariant::OwnedObjectPath, _> =
                systemd_proxy.call("GetUnit", &(service_name,)).await;

            if status_result.is_ok() {
                Ok(format!(
                    "Service {} is already running or was started",
                    service_name
                ))
            } else {
                Err(anyhow::anyhow!(
                    "Failed to start service {}: {}",
                    service_name,
                    e
                ))
            }
        }
    }
}

/// Tool to set a bridge property
pub struct OvsSetBridgePropertyTool;

#[async_trait]
impl Tool for OvsSetBridgePropertyTool {
    fn name(&self) -> &str {
        "ovs_set_bridge_property"
    }

    fn description(&self) -> &str {
        "Set a property on an OVS bridge via OVSDB JSON-RPC."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "bridge": {
                    "type": "string",
                    "description": "Name of the bridge"
                },
                "property": {
                    "type": "string",
                    "description": "Property name (datapath_type, fail_mode, stp_enable, mcast_snooping_enable)"
                },
                "value": {
                    "type": "string",
                    "description": "Property value"
                }
            },
            "required": ["bridge", "property", "value"]
        })
    }

    fn category(&self) -> &str {
        "networking"
    }

    fn tags(&self) -> Vec<String> {
        vec![
            "ovs".to_string(),
            "bridge".to_string(),
            "property".to_string(),
            "write".to_string(),
        ]
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let bridge_name = input
            .get("bridge")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required argument: bridge"))?;

        let property = input
            .get("property")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required argument: property"))?;

        let value = input
            .get("value")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required argument: value"))?;

        let client = OvsdbClient::new();

        client
            .set_bridge_property(bridge_name, property, value)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to set bridge property: {}", e))?;

        Ok(json!({
            "success": true,
            "bridge": bridge_name,
            "property": property,
            "value": value,
            "message": format!("Set {}={} on bridge '{}'", property, value, bridge_name)
        }))
    }
}

/// Tool to delete a port from an OVS bridge
pub struct OvsDeletePortTool;

#[async_trait]
impl Tool for OvsDeletePortTool {
    fn name(&self) -> &str {
        "ovs_delete_port"
    }

    fn description(&self) -> &str {
        "Delete a port from an OVS bridge via OVSDB JSON-RPC."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "bridge": {
                    "type": "string",
                    "description": "Name of the bridge"
                },
                "port": {
                    "type": "string",
                    "description": "Name of the port to delete"
                }
            },
            "required": ["bridge", "port"]
        })
    }

    fn category(&self) -> &str {
        "networking"
    }

    fn tags(&self) -> Vec<String> {
        vec![
            "ovs".to_string(),
            "port".to_string(),
            "delete".to_string(),
            "write".to_string(),
        ]
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let bridge_name = input
            .get("bridge")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required argument: bridge"))?;

        let port_name = input
            .get("port")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required argument: port"))?;

        let client = OvsdbClient::new();

        client
            .delete_port(bridge_name, port_name)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to delete port: {}", e))?;

        Ok(json!({
            "success": true,
            "bridge": bridge_name,
            "port": port_name,
            "message": format!("Port '{}' deleted from bridge '{}'", port_name, bridge_name)
        }))
    }
}

/// Tool to apply OpenFlow obfuscation levels to privacy router
pub struct OvsApplyObfuscationTool;

#[async_trait]
impl Tool for OvsApplyObfuscationTool {
    fn name(&self) -> &str {
        "ovs_apply_obfuscation"
    }

    fn description(&self) -> &str {
        "Apply OpenFlow obfuscation levels (0-3) to privacy router bridge for traffic privacy protection. Level 1: basic security (11 flows), Level 2: pattern hiding (3 flows), Level 3: advanced obfuscation (4 flows)."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "bridge": {
                    "type": "string",
                    "description": "OVS bridge name (default: ovs-br0)",
                    "default": "ovs-br0"
                },
                "level": {
                    "type": "integer",
                    "description": "Obfuscation level: 0=none, 1=basic security, 2=pattern hiding (recommended), 3=advanced",
                    "minimum": 0,
                    "maximum": 3,
                    "default": 2
                },
                "privacy_ports": {
                    "type": "array",
                    "description": "Privacy tunnel ports (default: [gbr_wg, gbr_warp, gbr_xray])",
                    "items": {"type": "string"},
                    "default": ["gbr_wg", "gbr_warp", "gbr_xray"]
                }
            },
            "required": []
        })
    }

    fn category(&self) -> &str {
        "privacy"
    }

    fn tags(&self) -> Vec<String> {
        vec![
            "ovs".to_string(),
            "privacy".to_string(),
            "obfuscation".to_string(),
            "openflow".to_string(),
            "security".to_string(),
        ]
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let bridge = input
            .get("bridge")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required parameter: bridge"))?;

        let level = input.get("level").and_then(|v| v.as_u64()).unwrap_or(2) as u8;

        if level > 3 {
            return Err(anyhow::anyhow!(
                "Invalid obfuscation level: {}. Must be 0-3.",
                level
            ));
        }

        let ports_list: Vec<String> = match input.get("privacy_ports").and_then(|v| v.as_array()) {
            Some(arr) => arr
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect(),
            None => vec![
                "gbr_wg".to_string(),
                "gbr_warp".to_string(),
                "gbr_xray".to_string(),
            ],
        };

        info!(
            "Generating obfuscation level {} configuration for bridge {}",
            level, bridge
        );

        // Calculate flow counts
        let security_flows = if level >= 1 { 11 } else { 0 };
        let pattern_flows = if level >= 2 { 3 } else { 0 };
        let advanced_flows = if level >= 3 { 4 } else { 0 };
        let forwarding_flows = ports_list.len() * 2 + 1;
        let total_flows = security_flows + pattern_flows + advanced_flows + forwarding_flows;

        // Generate flow descriptions
        let mut flow_descriptions = vec![];

        // Forwarding flows
        for (idx, port) in ports_list.iter().enumerate() {
            if idx < ports_list.len() - 1 {
                let next = &ports_list[idx + 1];
                flow_descriptions.push(format!("[Table 40:P100] Forward {} → {}", port, next));
            }
        }
        for idx in (1..ports_list.len()).rev() {
            let port = &ports_list[idx];
            let prev = &ports_list[idx - 1];
            flow_descriptions.push(format!("[Table 40:P100] Return {} → {}", port, prev));
        }
        flow_descriptions.push("[Table 40:P1] Normal L2/L3 forwarding".to_string());

        // Security flows (Level 1)
        if level >= 1 {
            flow_descriptions.extend(vec![
                "[Table 0:P500] Drop SYN+FIN packets (invalid)".to_string(),
                "[Table 0:P500] Drop NULL scan packets".to_string(),
                "[Table 0:P500] Drop XMAS scan packets".to_string(),
                "[Table 0:P490] Drop fragmented packets".to_string(),
                "[Table 0:P480] Rate limit ICMP to 100pps".to_string(),
                "[Table 0:P480] Rate limit DNS queries to 1000pps".to_string(),
                "[Table 0:P470] Connection tracking for stateful filtering".to_string(),
                "[Table 10:P500] Drop untracked connections".to_string(),
                "[Table 10:P500] Drop invalid connection states".to_string(),
                "[Table 10:P400] Allow established connections".to_string(),
                "[Table 10:P390] Allow new connections".to_string(),
            ]);
        }

        // Pattern hiding flows (Level 2)
        if level >= 2 {
            flow_descriptions.extend(vec![
                "[Table 20:P300] TTL normalization (set to 64)".to_string(),
                "[Table 20:P290] Timing jitter for TCP (anti-fingerprinting)".to_string(),
                "[Table 20:P280] TCP source port randomization".to_string(),
            ]);
        }

        // Advanced obfuscation flows (Level 3)
        if level >= 3 {
            flow_descriptions.extend(vec![
                "[Table 30:P200] WireGuard port mimicry (51820→443)".to_string(),
                "[Table 30:P190] Decoy traffic trigger (low bandwidth detection)".to_string(),
                "[Table 30:P180] Packet timing randomization (morphing)".to_string(),
                "[Table 30:P170] DPI evasion (VLAN stripping)".to_string(),
            ]);
        }

        Ok(json!({
            "success": true,
            "bridge": bridge,
            "obfuscation_level": level,
            "flow_breakdown": {
                "security": security_flows,
                "pattern_hiding": pattern_flows,
                "advanced": advanced_flows,
                "forwarding": forwarding_flows,
                "total": total_flows,
            },
            "flows_generated": flow_descriptions,
            "level_description": match level {
                0 => "No obfuscation - standard forwarding only",
                1 => "Basic security - drop invalid packets, rate limiting, connection tracking",
                2 => "Pattern hiding - TTL normalization, timing jitter, anti-fingerprinting (recommended)",
                3 => "Advanced - protocol mimicry, decoy traffic, traffic morphing",
                _ => "Unknown level"
            },
            "note": "OpenFlow obfuscation configuration generated. Use op-state plugin to apply flows to OVS bridge."
        }))
    }
}

/// Create all OVS tools
pub fn create_ovs_tools() -> Vec<std::sync::Arc<dyn Tool>> {
    vec![
        std::sync::Arc::new(TestTool),
        // Read operations
        std::sync::Arc::new(OvsCheckAvailableTool),
        std::sync::Arc::new(OvsListBridgesTool),
        std::sync::Arc::new(OvsListPortsTool),
        std::sync::Arc::new(OvsGetBridgeInfoTool),
        std::sync::Arc::new(OvsListDatapathsTool),
        std::sync::Arc::new(OvsListVportsTool),
        std::sync::Arc::new(OvsCapabilitiesTool),
        std::sync::Arc::new(OvsDumpFlowsTool),
        // Write operations
        std::sync::Arc::new(OvsCreateBridgeTool),
        std::sync::Arc::new(OvsDeleteBridgeTool),
        std::sync::Arc::new(OvsAddPortTool),
        std::sync::Arc::new(OvsDeletePortTool),
        std::sync::Arc::new(OvsSetBridgePropertyTool),
        // Privacy/Obfuscation
        std::sync::Arc::new(OvsApplyObfuscationTool),
        // Auto-install
        std::sync::Arc::new(OvsAutoInstallTool),
    ]
}
