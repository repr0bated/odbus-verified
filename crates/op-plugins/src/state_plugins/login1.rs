//! Login/session management through the upstream login1 D-Bus interface.

mod methods;
pub use methods::*;

use anyhow::Result;
use async_trait::async_trait;
use op_state::{
    ApplyResult, Checkpoint, DiffMetadata, PluginCapabilities, StateAction, StateDiff, StatePlugin,
};
use op_state_store::{FieldSchema, FieldType, PluginSchema};
use serde::{Deserialize, Serialize};
use simd_json::{OwnedValue as Value, json};
use zbus::zvariant::OwnedObjectPath;
use zbus::{Connection, Proxy};

/// Login1 state containing active sessions.
/// See: https://www.freedesktop.org/wiki/Software/systemd/Login1/
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Login1State {
    /// Active login sessions
    pub sessions: Vec<SessionInfo>,
}

/// Login1 session information.
/// See: https://www.freedesktop.org/wiki/Software/systemd/Login1/
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SessionInfo {
    /// Session ID
    pub id: String,
    /// User ID
    pub uid: u32,
    /// Username
    pub user: String,
    /// Seat name
    pub seat: String,
    /// Session object path
    pub path: String,
}

pub struct Login1Plugin;

impl Login1Plugin {
    pub fn new() -> Self {
        Self
    }

    async fn connect_manager(&self) -> Result<Proxy<'static>> {
        let conn = Connection::system().await?;
        let p = Proxy::new(
            &conn,
            "org.freedesktop.login1",
            "/org/freedesktop/login1",
            "org.freedesktop.login1.Manager",
        )
        .await?;
        Ok(p)
    }
}

impl Default for Login1Plugin {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl StatePlugin for Login1Plugin {
    fn name(&self) -> &str {
        "login1"
    }
    fn version(&self) -> &str {
        "2.0.0"
    }

    fn schema(&self) -> Option<PluginSchema> {
        Some(login1_schema())
    }

    async fn calculate_diff(&self, current: &Value, desired: &Value) -> Result<StateDiff> {
        let actions = if current != desired {
            vec![StateAction::Modify {
                resource: "login1".into(),
                changes: desired.clone(),
            }]
        } else {
            vec![]
        };
        Ok(StateDiff {
            plugin: self.name().to_string(),
            actions,
            metadata: DiffMetadata {
                timestamp: chrono::Utc::now().timestamp(),
                current_hash: format!("{:x}", md5::compute(simd_json::to_string(current)?)),
                desired_hash: format!("{:x}", md5::compute(simd_json::to_string(desired)?)),
            },
        })
    }

    async fn apply_state(&self, _diff: &StateDiff) -> Result<ApplyResult> {
        Ok(ApplyResult {
            success: true,
            changes_applied: vec!["read-only".into()],
            errors: vec![],
            checkpoint: None,
        })
    }

    async fn verify_state(&self, _desired: &Value) -> Result<bool> {
        Ok(true)
    }

    async fn create_checkpoint(&self) -> Result<Checkpoint> {
        Ok(Checkpoint {
            id: format!("login1-{}", chrono::Utc::now().timestamp()),
            plugin: self.name().into(),
            timestamp: chrono::Utc::now().timestamp(),
            state_snapshot: json!({}),
            backend_checkpoint: None,
        })
    }

    async fn rollback(&self, _checkpoint: &Checkpoint) -> Result<()> {
        Ok(())
    }

    fn capabilities(&self) -> PluginCapabilities {
        PluginCapabilities {
            supports_rollback: false,
            supports_checkpoints: false,
            supports_verification: false,
            atomic_operations: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct GetSessionInput {
    pub session_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct GetUserInput {
    pub uid: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct GetSeatInput {
    pub seat_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PowerOffInput {
    pub interactive: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CreateSessionInput {
    pub uid: u32,
    pub pid: u32,
    pub service: String,
    pub type_: String,
    pub class: String,
    pub desktop: String,
    pub seat_id: String,
    pub vtnr: u32,
    pub tty: String,
    pub display: String,
    pub remote: bool,
    pub remote_user: String,
    pub remote_host: String,
    #[serde(default)]
    pub properties: Vec<SessionProperty>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SessionIdInput {
    pub session_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct KillSessionInput {
    pub session_id: String,
    pub who: String,
    pub signal_number: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct UidInput {
    pub uid: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct KillUserInput {
    pub uid: u32,
    pub signal_number: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SetUserLingerInput {
    pub uid: u32,
    pub enable: bool,
    pub interactive: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SeatIdInput {
    pub seat_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AttachDeviceInput {
    pub seat_id: String,
    pub sysfs_path: String,
    pub interactive: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct InteractiveInput {
    pub interactive: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SleepInput {
    /// login1 Sleep flags, not an operation selector.
    pub flags: u64,
}

pub(crate) fn login1_schema() -> PluginSchema {
    let mut schema = PluginSchema::builder("login1")
        .category("service")
        .version("2.0.0")
        .description("Login sessions, users and seats through the upstream login1 D-Bus API")
        .field(
            "sessions",
            FieldSchema {
                field_type: FieldType::Any,
                required: true,
                description: "Active sessions".to_string(),
                default: Some(json!([])),
                example: None,
                constraints: Vec::new(),
                read_only: false,
                read_only_when: None,
            },
        )
        .build();

    // org.freedesktop.login1.Manager methods
    // Spec: https://www.freedesktop.org/software/systemd/man/latest/org.freedesktop.login1.html
    schema.methods.insert(
        "list_sessions".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            (),
            ListSessionsOutput,
        >(
            "ListSessions",
            op_state_store::SideEffect::Read,
            true,
            "login1.read",
            "obs.software.login1.sessions.list@v1",
        ),
    );
    schema.methods.insert(
        "list_users".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            (),
            ListUsersOutput,
        >(
            "ListUsers",
            op_state_store::SideEffect::Read,
            true,
            "login1.read",
            "obs.software.login1.users.list@v1",
        ),
    );
    schema.methods.insert(
        "list_seats".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            (),
            ListSeatsOutput,
        >(
            "ListSeats",
            op_state_store::SideEffect::Read,
            true,
            "login1.read",
            "obs.software.login1.seats.list@v1",
        ),
    );
    schema.methods.insert(
        "get_session".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            GetSessionInput,
            GetObjectOutput,
        >(
            "GetSession",
            op_state_store::SideEffect::Read,
            true,
            "login1.read",
            "obs.software.login1.session.get@v1",
        ),
    );
    schema.methods.insert(
        "get_user".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            GetUserInput,
            GetObjectOutput,
        >(
            "GetUser",
            op_state_store::SideEffect::Read,
            true,
            "login1.read",
            "obs.software.login1.user.get@v1",
        ),
    );
    schema.methods.insert(
        "get_seat".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            GetSeatInput,
            GetObjectOutput,
        >(
            "GetSeat",
            op_state_store::SideEffect::Read,
            true,
            "login1.read",
            "obs.software.login1.seat.get@v1",
        ),
    );
    schema.methods.insert(
        "power_off".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            PowerOffInput,
            LoginOperationOutput,
        >(
            "PowerOff",
            op_state_store::SideEffect::Mutation,
            false,
            "login1.admin",
            "mut.software.login1.system.poweroff@v1",
        ),
    );
    schema.methods.insert(
        "reboot".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            PowerOffInput,
            LoginOperationOutput,
        >(
            "Reboot",
            op_state_store::SideEffect::Mutation,
            false,
            "login1.admin",
            "mut.software.login1.system.reboot@v1",
        ),
    );
    schema.methods.insert(
        "create_session".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            CreateSessionInput,
            CreateSessionOutput,
        >(
            "CreateSession",
            op_state_store::SideEffect::Mutation,
            false,
            "login1.write",
            "mut.software.login1.session.create@v1",
        ),
    );
    schema.methods.insert(
        "release_session".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            SessionIdInput,
            LoginOperationOutput,
        >(
            "ReleaseSession",
            op_state_store::SideEffect::Mutation,
            true,
            "login1.write",
            "mut.software.login1.session.release@v1",
        ),
    );
    schema.methods.insert(
        "terminate_session".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            SessionIdInput,
            LoginOperationOutput,
        >(
            "TerminateSession",
            op_state_store::SideEffect::Mutation,
            true,
            "login1.write",
            "mut.software.login1.session.terminate@v1",
        ),
    );
    schema.methods.insert(
        "lock_session".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            SessionIdInput,
            LoginOperationOutput,
        >(
            "LockSession",
            op_state_store::SideEffect::Mutation,
            true,
            "login1.write",
            "mut.software.login1.session.lock@v1",
        ),
    );
    schema.methods.insert(
        "unlock_session".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            SessionIdInput,
            LoginOperationOutput,
        >(
            "UnlockSession",
            op_state_store::SideEffect::Mutation,
            true,
            "login1.write",
            "mut.software.login1.session.unlock@v1",
        ),
    );
    schema.methods.insert(
        "kill_session".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            KillSessionInput,
            LoginOperationOutput,
        >(
            "KillSession",
            op_state_store::SideEffect::Mutation,
            false,
            "login1.write",
            "mut.software.login1.session.kill@v1",
        ),
    );
    schema.methods.insert(
        "terminate_user".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            UidInput,
            LoginOperationOutput,
        >(
            "TerminateUser",
            op_state_store::SideEffect::Mutation,
            true,
            "login1.admin",
            "mut.software.login1.user.terminate@v1",
        ),
    );
    schema.methods.insert(
        "kill_user".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            KillUserInput,
            LoginOperationOutput,
        >(
            "KillUser",
            op_state_store::SideEffect::Mutation,
            false,
            "login1.admin",
            "mut.software.login1.user.kill@v1",
        ),
    );
    schema.methods.insert(
        "set_user_linger".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            SetUserLingerInput,
            LoginOperationOutput,
        >(
            "SetUserLinger",
            op_state_store::SideEffect::Mutation,
            true,
            "login1.admin",
            "mut.software.login1.user.linger.set@v1",
        ),
    );
    schema.methods.insert(
        "terminate_seat".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            SeatIdInput,
            LoginOperationOutput,
        >(
            "TerminateSeat",
            op_state_store::SideEffect::Mutation,
            true,
            "login1.admin",
            "mut.software.login1.seat.terminate@v1",
        ),
    );
    schema.methods.insert(
        "attach_device".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            AttachDeviceInput,
            LoginOperationOutput,
        >(
            "AttachDevice",
            op_state_store::SideEffect::Mutation,
            true,
            "login1.admin",
            "mut.software.login1.device.attach@v1",
        ),
    );
    schema.methods.insert(
        "flush_devices".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            InteractiveInput,
            LoginOperationOutput,
        >(
            "FlushDevices",
            op_state_store::SideEffect::Mutation,
            true,
            "login1.admin",
            "mut.software.login1.devices.flush@v1",
        ),
    );
    schema.methods.insert(
        "suspend".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            InteractiveInput,
            LoginOperationOutput,
        >(
            "Suspend",
            op_state_store::SideEffect::Mutation,
            false,
            "login1.admin",
            "mut.software.login1.system.suspend@v1",
        ),
    );
    schema.methods.insert(
        "hibernate".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            InteractiveInput,
            LoginOperationOutput,
        >(
            "Hibernate",
            op_state_store::SideEffect::Mutation,
            false,
            "login1.admin",
            "mut.software.login1.system.hibernate@v1",
        ),
    );
    schema.methods.insert(
        "sleep".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            SleepInput,
            LoginOperationOutput,
        >(
            "Sleep",
            op_state_store::SideEffect::Mutation,
            false,
            "login1.admin",
            "mut.software.login1.system.sleep@v1",
        ),
    );
    schema.capabilities.insert(
        "login1.read".to_string(),
        op_state_store::CapabilityDecl {
            id: "login1.read".to_string(),
            description:
                "Grants: list_sessions, list_users, list_seats, get_session, get_user, get_seat."
                    .to_string(),
        },
    );
    schema.capabilities.insert(
        "login1.admin".to_string(),
        op_state_store::CapabilityDecl {
            id: "login1.admin".to_string(),
            description: "Grants: power_off, reboot, suspend, hibernate, sleep, terminate_seat, attach_device, flush_devices, terminate_user, kill_user, set_user_linger.".to_string(),
        },
    );
    schema.capabilities.insert(
        "login1.write".to_string(),
        op_state_store::CapabilityDecl {
            id: "login1.write".to_string(),
            description: "Grants: create_session, release_session, terminate_session, lock_session, unlock_session, kill_session.".to_string(),
        },
    );

    schema
}

// Self-registration: the plugin registry discovers this via inventory
// (single source of the catalog; no central dispatch list).
inventory::submit! {
    crate::default_registry::PluginReg::new("login1", |_ctx| std::sync::Arc::new(Login1Plugin::new()))
}
