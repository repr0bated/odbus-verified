//! op-network: Network Management with OpenFlow, OVSDB, and Container Networking
//!
//! This crate provides:
//! - Native OpenFlow protocol implementation (all versions, pure Rust)
//! - OVSDB JSON-RPC client for OVS bridge management
//! - Network plugin for OVS/OVSDB persistence
//! - Socket networking support
//! - Container networking with OpenFlow routing
//! - Native Proxmox API client for LXC container management

#![deny(rustdoc::broken_intra_doc_links)]

pub mod btrfs;
pub mod controller;
pub mod openflow;
pub mod openflow_translate;
pub mod ovs_capabilities;
pub mod ovs_error;
pub mod ovs_netlink;
pub mod ovsdb;
pub mod plugin;
pub mod proxmox;
pub mod rovs_proxy;
pub mod rtnetlink;

pub use controller::{OpenFlowController, OpenFlowControllerHandle};
pub use openflow::{FlowAction, FlowEntry, FlowMatch, OpenFlowClient, OpenFlowVersion};
pub use ovs_capabilities::{counter_excuses, excuses_to_llm_context, OvsCapabilities};
pub use ovs_error::OvsError;
pub use ovs_netlink::{Datapath, KernelFlow, OvsNetlinkClient, Vport, VportConfig, VportType};
pub use ovsdb::OvsdbClient;
pub use plugin::{NetworkInterface, NetworkPlugin, OpenFlowConfig, OvsBridge, OvsdbConfig};
pub use proxmox::{
    ContainerStatus, CreateContainerRequest, LxcContainer, ProxmoxClient, ProxmoxToken,
};

/// Prelude for convenient imports
pub mod prelude {
    pub use super::openflow::{FlowAction, FlowEntry, FlowMatch, OpenFlowClient, OpenFlowVersion};
    pub use super::ovs_capabilities::OvsCapabilities;
    pub use super::ovs_netlink::{Datapath, OvsNetlinkClient, Vport};
    pub use super::plugin::{NetworkInterface, NetworkPlugin, OvsBridge};
    pub use super::proxmox::{CreateContainerRequest, LxcContainer, ProxmoxClient};
}
