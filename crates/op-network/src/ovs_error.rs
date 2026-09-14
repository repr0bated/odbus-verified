//! OVS-specific error types for better error handling and debugging
//!
//! This module provides detailed error types for OVS operations,
//! making it easier to diagnose issues and provide helpful feedback.

use thiserror::Error;

/// OVS-specific errors
#[derive(Error, Debug)]
pub enum OvsError {
    // ========================================================================
    // Socket/Connection Errors
    // ========================================================================
    #[error("Failed to create netlink socket: {0}")]
    SocketCreation(#[source] std::io::Error),

    #[error("Failed to bind netlink socket: {0}")]
    SocketBind(#[source] std::io::Error),

    #[error("Failed to send netlink message: {0}")]
    SocketSend(#[source] std::io::Error),

    #[error("Failed to receive netlink message: {0}")]
    SocketRecv(#[source] std::io::Error),

    #[error("OVSDB socket not found at {0}")]
    OvsdbSocketNotFound(String),

    #[error("Failed to connect to OVSDB: {0}")]
    OvsdbConnection(String),

    // ========================================================================
    // Family Resolution Errors
    // ========================================================================
    #[error(
        "OVS Generic Netlink family '{0}' not found - is the openvswitch kernel module loaded?"
    )]
    FamilyNotFound(String),

    #[error("Failed to resolve Generic Netlink family: {0}")]
    FamilyResolution(String),

    // ========================================================================
    // Datapath Errors
    // ========================================================================
    #[error("Datapath '{0}' not found")]
    DatapathNotFound(String),

    #[error("Failed to create datapath '{0}': {1}")]
    DatapathCreation(String, String),

    #[error("Failed to delete datapath '{0}': {1}")]
    DatapathDeletion(String, String),

    #[error("Datapath name too long (max 16 chars): {0}")]
    DatapathNameTooLong(String),

    // ========================================================================
    // Vport Errors
    // ========================================================================
    #[error("Vport '{0}' not found on datapath '{1}'")]
    VportNotFound(String, String),

    #[error("Failed to create vport '{0}': {1}")]
    VportCreation(String, String),

    #[error("Failed to delete vport '{0}': {1}")]
    VportDeletion(String, String),

    #[error("Invalid vport type: {0}")]
    InvalidVportType(u32),

    // ========================================================================
    // Flow Errors
    // ========================================================================
    #[error("Failed to dump flows for datapath '{0}': {1}")]
    FlowDump(String, String),

    #[error("Failed to add flow: {0}")]
    FlowAdd(String),

    #[error("Failed to delete flow: {0}")]
    FlowDelete(String),

    #[error("Invalid flow key format: {0}")]
    InvalidFlowKey(String),

    #[error("Invalid flow action format: {0}")]
    InvalidFlowAction(String),

    // ========================================================================
    // Netlink Protocol Errors
    // ========================================================================
    #[error("Netlink error code {0}: {1}")]
    NetlinkError(i32, String),

    #[error("Failed to parse netlink message: {0}")]
    NetlinkParse(String),

    #[error("Unexpected netlink message type: {0}")]
    UnexpectedMessageType(u16),

    // ========================================================================
    // Permission Errors
    // ========================================================================
    #[error("Operation requires root privileges")]
    NotRoot,

    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    #[error("CAP_NET_ADMIN capability required for this operation")]
    MissingCapNetAdmin,

    // ========================================================================
    // Configuration Errors
    // ========================================================================
    #[error("OVS not running or not installed")]
    OvsNotRunning,

    #[error("openvswitch kernel module not loaded")]
    KernelModuleNotLoaded,

    // ========================================================================
    // Generic Errors
    // ========================================================================
    #[error("Operation not implemented: {0}")]
    NotImplemented(String),

    #[error("Timeout waiting for response")]
    Timeout,

    #[error("Internal error: {0}")]
    Internal(String),
}

impl OvsError {
    /// Get a helpful suggestion for resolving this error
    pub fn suggestion(&self) -> &'static str {
        match self {
            OvsError::SocketCreation(_) => "Ensure you have the netlink-sys crate properly linked",
            OvsError::SocketBind(_) => "Another process may be using the netlink socket",
            OvsError::FamilyNotFound(_) => "Try: sudo modprobe openvswitch",
            OvsError::DatapathNotFound(_) => "List available datapaths with ovs_list_datapaths",
            OvsError::VportNotFound(_, _) => "List available vports with ovs_list_vports",
            OvsError::NotRoot => "Run as root or with sudo",
            OvsError::MissingCapNetAdmin => {
                "Run with CAP_NET_ADMIN: sudo setcap cap_net_admin+ep <binary>"
            }
            OvsError::OvsNotRunning => "Start OVS: sudo sv start ovs-vswitchd",
            OvsError::KernelModuleNotLoaded => "Load module: sudo modprobe openvswitch",
            OvsError::OvsdbSocketNotFound(_) => {
                "Check if OVS is installed: apt install openvswitch-switch"
            }
            OvsError::Timeout => "Increase timeout or check system load",
            _ => "Check system logs for more details",
        }
    }

    /// Returns true if this error might be resolved by running as root
    pub fn needs_root(&self) -> bool {
        matches!(
            self,
            OvsError::NotRoot | OvsError::MissingCapNetAdmin | OvsError::PermissionDenied(_)
        )
    }

    /// Returns true if OVS components need to be installed/started
    pub fn needs_ovs(&self) -> bool {
        matches!(
            self,
            OvsError::FamilyNotFound(_)
                | OvsError::OvsNotRunning
                | OvsError::KernelModuleNotLoaded
                | OvsError::OvsdbSocketNotFound(_)
        )
    }
}

/// Map netlink error codes to descriptive messages
pub fn netlink_error_message(code: i32) -> &'static str {
    match code {
        -1 => "Operation not permitted (EPERM)",
        -2 => "No such file or directory (ENOENT)",
        -12 => "Out of memory (ENOMEM)",
        -13 => "Permission denied (EACCES)",
        -17 => "File exists (EEXIST)",
        -19 => "No such device (ENODEV)",
        -22 => "Invalid argument (EINVAL)",
        -95 => "Operation not supported (ENOTSUP)",
        _ => "Unknown error",
    }
}

/// Convert a netlink error code to an OvsError
pub fn from_netlink_error(code: i32) -> OvsError {
    let msg = netlink_error_message(code);
    match code {
        -1 | -13 => OvsError::PermissionDenied(msg.to_string()),
        -2 | -19 => OvsError::DatapathNotFound(msg.to_string()),
        -22 => OvsError::NetlinkError(code, msg.to_string()),
        _ => OvsError::NetlinkError(code, msg.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_suggestions() {
        let err = OvsError::NotRoot;
        assert!(!err.suggestion().is_empty());
        assert!(err.needs_root());
        assert!(!err.needs_ovs());
    }

    #[test]
    fn test_needs_ovs() {
        let err = OvsError::KernelModuleNotLoaded;
        assert!(err.needs_ovs());
        assert!(!err.needs_root());
    }

    #[test]
    fn test_netlink_error_message() {
        assert!(netlink_error_message(-1).contains("EPERM"));
        assert!(netlink_error_message(-22).contains("EINVAL"));
    }

    #[test]
    fn test_from_netlink_error() {
        let err = from_netlink_error(-13);
        assert!(matches!(err, OvsError::PermissionDenied(_)));
    }
}
