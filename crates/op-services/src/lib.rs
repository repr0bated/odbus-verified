//! op-services: System-wide runit service manager.

pub mod grpc;
pub mod manager;
pub mod schema;
pub mod store;

pub use manager::{ServiceEvent, ServiceManager};
pub use schema::{
    ActiveState, DesiredState, ExecCommand, LogType, ManagerState, ReadyNotification,
    ResourceLimits, RestartCondition, RestartPolicy, RunitPlugin, ServiceDef, ServiceName,
    ServiceState, ServiceStatus, ServiceType, ValidationError,
};
pub use store::Store;
