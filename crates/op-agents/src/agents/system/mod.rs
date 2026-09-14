//! System agents - D-Bus based system operations
//!
//! These agents interact with system services via D-Bus:
//! - executor: Command execution
//! - file: File operations
//! - monitor: System monitoring
//! - network: Network configuration
//! - packagekit: Package management
//! - systemd: Systemd service control

pub mod executor;
pub mod file;
pub mod monitor;
pub mod network;
pub mod packagekit;
pub mod systemd;

pub use executor::ExecutorAgent;
pub use file::FileAgent;
pub use monitor::MonitorAgent;
pub use network::NetworkAgent;
pub use packagekit::PackageKitAgent;
pub use systemd::SystemdAgent;
