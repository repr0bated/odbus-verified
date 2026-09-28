//! Mail server state plugin - manages Incus mail container and D-Bus registration.
//!
//! Tracks Postfix/Dovecot runtime state, Unix socket endpoints for Xray routing,
//! and exposes mail configuration as a D-Bus object via zbus.

#[path = "mail_server/methods.rs"]
pub mod methods;
#[path = "mail_server/native_owner.rs"]
pub mod native_owner;

use anyhow::{Context, Result};
use async_trait::async_trait;
use op_state::{ApplyResult, Checkpoint, DiffMetadata, PluginCapabilities, StateDiff, StatePlugin};
use op_state_store::{FieldSchema, PluginSchema};
use serde::{Deserialize, Serialize};
use simd_json::prelude::*;
use simd_json::{OwnedValue as Value, json};
use std::collections::HashMap;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Top-level state for the mail server plugin.
/// See: https://doc.dovecot.org/configuration_manual/references/
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema, Default)]
pub struct MailServerState {
    /// Incus container name running the mail stack
    pub container_name: String,
    /// Container status: "Running", "Stopped", "Frozen"
    pub container_status: String,
    /// Primary mail domain
    pub domain: String,
    /// Unix socket path for Xray naive routing integration
    pub xray_socket_path: String,
    /// D-Bus service name registered for this mail instance
    pub dbus_service_name: String,
    /// Active mail service endpoints
    pub endpoints: MailEndpoints,
    /// Container IPv4 address
    pub container_ip: Option<String>,
    /// Whether the mail stack is healthy
    pub healthy: bool,
    /// Last error message if unhealthy
    pub last_error: Option<String>,
    /// Additional container devices (unix socket mounts, etc.)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub devices: Option<HashMap<String, HashMap<String, String>>>,
    /// Administration state observed from the native Postfix/Dovecot owner.
    #[serde(default)]
    pub administration: MailAdministrationState,
}

/// Mail protocol endpoints.
/// See: https://doc.dovecot.org/configuration_manual/references/
#[derive(Debug, Clone, Serialize, Deserialize, Default, schemars::JsonSchema)]
pub struct MailEndpoints {
    /// SMTP submission port (587)
    pub smtp_submission: Option<String>,
    /// SMTP TLS port (465)
    pub smtp_tls: Option<String>,
    /// IMAP port (143)
    pub imap: Option<String>,
    /// IMAPS port (993)
    pub imaps: Option<String>,
    /// Dovecot LDA/LMTP unix socket inside container
    pub dovecot_lmtp: Option<String>,
    /// Postfix pickup unix socket inside container
    pub postfix_pickup: Option<String>,
}

pub struct MailServerPlugin;

impl MailServerPlugin {
    pub fn new() -> Self {
        Self
    }

    /// Default state for 3tched.com mail stack
    pub fn declared_state() -> MailServerState {
        MailServerState {
            container_name: "mail-3tched".to_string(),
            container_status: "Unknown".to_string(),
            domain: "3tched.com".to_string(),
            xray_socket_path: "/run/xray/mail-naive.sock".to_string(),
            dbus_service_name: "org.opdbus.MailServer.3tched".to_string(),
            endpoints: MailEndpoints {
                smtp_submission: Some("0.0.0.0:587".to_string()),
                smtp_tls: Some("0.0.0.0:465".to_string()),
                imap: Some("0.0.0.0:143".to_string()),
                imaps: Some("0.0.0.0:993".to_string()),
                dovecot_lmtp: Some("/var/spool/postfix/private/dovecot-lmtp".to_string()),
                postfix_pickup: Some("/var/spool/postfix/private/pickup".to_string()),
            },
            container_ip: None,
            healthy: false,
            last_error: None,
            devices: None,
            administration: MailAdministrationState::default(),
        }
    }

    /// Minimal HTTP-over-UnixSocket helper for Incus REST API
    async fn incus_api_get(path: &str) -> Result<simd_json::OwnedValue> {
        let socket_path = "/var/lib/incus/unix.socket";
        if !std::path::Path::new(socket_path).exists() {
            return Err(anyhow::anyhow!(
                "Incus Unix socket not found at {}",
                socket_path
            ));
        }

        let mut stream = tokio::net::UnixStream::connect(socket_path)
            .await
            .context("Failed to connect to Incus Unix socket")?;

        let request = format!(
            "GET {} HTTP/1.1\r\nHost: localhost\r\nAccept: application/json\r\n\r\n",
            path
        );
        stream.write_all(request.as_bytes()).await?;

        let mut response = Vec::new();
        let mut buf = [0u8; 4096];
        loop {
            match tokio::time::timeout(Duration::from_millis(500), stream.read(&mut buf)).await {
                Ok(Ok(0)) => break,
                Ok(Ok(n)) => response.extend_from_slice(&buf[..n]),
                Ok(Err(e)) => return Err(e.into()),
                Err(_) => break,
            }
        }

        let body_start = if let Some(idx) = response.windows(4).position(|w| w == b"\r\n\r\n") {
            idx + 4
        } else if let Some(idx) = response.windows(2).position(|w| w == b"\n\n") {
            idx + 2
        } else {
            return Ok(simd_json::json!({}));
        };

        let mut body = response[body_start..].to_vec();
        let headers = std::str::from_utf8(&response[..body_start]).unwrap_or("");
        if headers
            .to_lowercase()
            .contains("transfer-encoding: chunked")
        {
            let mut decoded = Vec::new();
            let mut pos = 0;
            while pos < body.len() {
                let mut line_end = pos;
                while line_end < body.len() && body[line_end] != b'\n' {
                    line_end += 1;
                }
                if line_end >= body.len() {
                    break;
                }
                let line = std::str::from_utf8(&body[pos..line_end])
                    .unwrap_or("")
                    .trim();
                let size = usize::from_str_radix(line.split(';').next().unwrap_or("0").trim(), 16)
                    .unwrap_or(0);
                if size == 0 {
                    break;
                }
                pos = line_end + 1;
                if pos < body.len() && body[pos] == b'\r' {
                    pos += 1;
                }
                decoded.extend_from_slice(&body[pos..pos + size]);
                pos += size;
                if pos < body.len() && body[pos] == b'\r' {
                    pos += 1;
                }
                if pos < body.len() && body[pos] == b'\n' {
                    pos += 1;
                }
            }
            body = decoded;
        }

        let mut raw = body;
        let val: simd_json::OwnedValue =
            simd_json::from_slice(&mut raw).context("Failed to parse Incus API response")?;
        let metadata = val.get("metadata").cloned().unwrap_or(simd_json::json!({}));
        Ok(metadata)
    }

    /// Query incus for container status via REST API (AGENTS.md §4: no subprocess bypasses)
    async fn query_container_status(&self, name: &str) -> Result<(String, Option<String>)> {
        let inst = Self::incus_api_get(&format!("/1.0/instances/{}?recursion=1", name)).await?;

        let status = inst
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown")
            .to_string();

        let ip = inst
            .get("state")
            .and_then(|s| s.get("network"))
            .and_then(|n| n.get("eth0"))
            .and_then(|e| e.get("addresses"))
            .and_then(|a| a.as_array())
            .and_then(|addrs| {
                addrs.iter().find_map(|addr| {
                    if addr.get("family")?.as_str()? == "inet" {
                        addr.get("address")?.as_str().map(String::from)
                    } else {
                        None
                    }
                })
            });

        Ok((status, ip))
    }

    /// Check if container is Running as a proxy for mail health
    /// (AGENTS.md §4: incus exec is a subprocess bypass; we use container state instead)
    async fn check_mail_health(&self, container: &str) -> (bool, Option<String>) {
        match Self::incus_api_get(&format!("/1.0/instances/{}?recursion=1", container)).await {
            Ok(inst) => {
                let status = inst
                    .get("status")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Unknown");
                if status == "Running" {
                    (true, None)
                } else {
                    let err = format!("container_status={}", status);
                    (false, Some(err))
                }
            }
            Err(e) => {
                let err = format!("failed to query container: {}", e);
                (false, Some(err))
            }
        }
    }
}

impl Default for MailServerPlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl StatePlugin for MailServerPlugin {
    fn metadata(&self) -> op_state::PluginMetadata {
        op_state::PluginMetadata {
            name: self.name().to_string(),
            version: self.version().to_string(),
            description: "Postfix/Dovecot state and native D-Bus administration for mail-3tched"
                .to_string(),
            author: None,
            license: None,
            dependencies: vec![
                "incus".to_string(),
                "postfix".to_string(),
                "dovecot".to_string(),
                "unix_socket".to_string(),
            ],
            dbus_services: vec!["org.opdbus.MailServer.3tched".to_string()],
            feature_schemas: vec![],
            object_schemas: std::collections::HashMap::new(),
        }
    }

    fn name(&self) -> &str {
        "mail_server"
    }

    fn version(&self) -> &str {
        "2.0.0"
    }

    fn schema(&self) -> Option<op_state_store::PluginSchema> {
        Some(mail_server_schema())
    }

    async fn calculate_diff(&self, _current: &Value, _desired: &Value) -> Result<StateDiff> {
        Ok(StateDiff {
            plugin: self.name().to_string(),
            actions: vec![],
            metadata: DiffMetadata {
                timestamp: chrono::Utc::now().timestamp(),
                current_hash: "unknown".to_string(),
                desired_hash: "unknown".to_string(),
            },
        })
    }

    async fn apply_state(&self, _diff: &StateDiff) -> Result<ApplyResult> {
        Ok(ApplyResult {
            success: true,
            changes_applied: vec![],
            errors: vec![],
            checkpoint: None,
        })
    }

    async fn verify_state(&self, _desired: &Value) -> Result<bool> {
        Ok(true)
    }

    async fn create_checkpoint(&self) -> Result<Checkpoint> {
        Ok(Checkpoint {
            id: uuid::Uuid::new_v4().to_string(),
            plugin: self.name().to_string(),
            timestamp: chrono::Utc::now().timestamp(),
            state_snapshot: Value::null(),
            backend_checkpoint: None,
        })
    }

    async fn rollback(&self, _checkpoint: &Checkpoint) -> Result<()> {
        Ok(())
    }

    fn capabilities(&self) -> PluginCapabilities {
        PluginCapabilities {
            supports_rollback: false,
            supports_checkpoints: true,
            supports_verification: true,
            atomic_operations: false,
        }
    }
}

pub(crate) fn mail_server_schema() -> PluginSchema {
    use op_state_store::FieldType;

    let endpoint_fields = {
        let mut fields = HashMap::new();
        fields.insert(
            "smtp_submission".to_string(),
            FieldSchema {
                field_type: FieldType::String,
                required: false,
                description: "SMTP submission endpoint (port 587)".to_string(),
                default: Some(json!("0.0.0.0:587")),
                example: None,
                constraints: Vec::new(),
                read_only: false,
                read_only_when: None,
            },
        );
        fields.insert(
            "smtp_tls".to_string(),
            FieldSchema {
                field_type: FieldType::String,
                required: false,
                description: "SMTP TLS endpoint (port 465)".to_string(),
                default: Some(json!("0.0.0.0:465")),
                example: None,
                constraints: Vec::new(),
                read_only: false,
                read_only_when: None,
            },
        );
        fields.insert(
            "imap".to_string(),
            FieldSchema {
                field_type: FieldType::String,
                required: false,
                description: "IMAP endpoint (port 143)".to_string(),
                default: Some(json!("0.0.0.0:143")),
                example: None,
                constraints: Vec::new(),
                read_only: false,
                read_only_when: None,
            },
        );
        fields.insert(
            "imaps".to_string(),
            FieldSchema {
                field_type: FieldType::String,
                required: false,
                description: "IMAPS endpoint (port 993)".to_string(),
                default: Some(json!("0.0.0.0:993")),
                example: None,
                constraints: Vec::new(),
                read_only: false,
                read_only_when: None,
            },
        );
        fields.insert(
            "dovecot_lmtp".to_string(),
            FieldSchema {
                field_type: FieldType::String,
                required: false,
                description: "Dovecot LMTP unix socket path inside container".to_string(),
                default: Some(json!("/var/spool/postfix/private/dovecot-lmtp")),
                example: None,
                constraints: Vec::new(),
                read_only: false,
                read_only_when: None,
            },
        );
        fields.insert(
            "postfix_pickup".to_string(),
            FieldSchema {
                field_type: FieldType::String,
                required: false,
                description: "Postfix pickup unix socket path inside container".to_string(),
                default: Some(json!("/var/spool/postfix/private/pickup")),
                example: None,
                constraints: Vec::new(),
                read_only: false,
                read_only_when: None,
            },
        );
        fields
    };

    let mut schema = PluginSchema::builder("mail_server")
        .category("service")
        .version("2.0.0")
        .description(
            "Postfix/Dovecot mail administration through the mail-3tched native D-Bus owner",
        )
        .dependency("incus")
        .dependency("unix_socket")
        .field(
            "container_name",
            FieldSchema {
                field_type: FieldType::String,
                required: true,
                description: "Incus container name running the mail stack".to_string(),
                default: Some(json!("mail-3tched")),
                example: None,
                constraints: Vec::new(),
                read_only: false,
                read_only_when: None,
            },
        )
        .field(
            "container_status",
            FieldSchema {
                field_type: FieldType::String,
                required: true,
                description: "Container runtime status".to_string(),
                default: Some(json!("Unknown")),
                example: None,
                constraints: Vec::new(),
                read_only: true,
                read_only_when: None,
            },
        )
        .field(
            "domain",
            FieldSchema {
                field_type: FieldType::String,
                required: true,
                description: "Primary mail domain".to_string(),
                default: Some(json!("3tched.com")),
                example: None,
                constraints: Vec::new(),
                read_only: false,
                read_only_when: None,
            },
        )
        .field(
            "xray_socket_path",
            FieldSchema {
                field_type: FieldType::String,
                required: true,
                description: "Unix socket path for Xray naive routing integration".to_string(),
                default: Some(json!("/run/xray/mail-naive.sock")),
                example: None,
                constraints: Vec::new(),
                read_only: false,
                read_only_when: None,
            },
        )
        .field(
            "dbus_service_name",
            FieldSchema {
                field_type: FieldType::String,
                required: true,
                description: "D-Bus service name registered for this mail instance".to_string(),
                default: Some(json!("org.opdbus.MailServer.3tched")),
                example: None,
                constraints: Vec::new(),
                read_only: false,
                read_only_when: None,
            },
        )
        .field(
            "endpoints",
            FieldSchema {
                field_type: FieldType::Object(endpoint_fields),
                required: true,
                description: "Active mail service endpoints".to_string(),
                default: Some(json!({})),
                example: None,
                constraints: Vec::new(),
                read_only: false,
                read_only_when: None,
            },
        )
        .field(
            "container_ip",
            FieldSchema {
                field_type: FieldType::String,
                required: false,
                description: "Container IPv4 address".to_string(),
                default: None,
                example: None,
                constraints: Vec::new(),
                read_only: true,
                read_only_when: None,
            },
        )
        .field(
            "healthy",
            FieldSchema {
                field_type: FieldType::Boolean,
                required: true,
                description: "Whether the mail stack is healthy".to_string(),
                default: Some(json!(false)),
                example: None,
                constraints: Vec::new(),
                read_only: true,
                read_only_when: None,
            },
        )
        .field(
            "last_error",
            FieldSchema {
                field_type: FieldType::String,
                required: false,
                description: "Last error message if unhealthy".to_string(),
                default: None,
                example: None,
                constraints: Vec::new(),
                read_only: true,
                read_only_when: None,
            },
        )
        .example(json!({
            "container_name": "mail-3tched",
            "container_status": "Running",
            "domain": "3tched.com",
            "xray_socket_path": "/run/xray/mail-naive.sock",
            "dbus_service_name": "org.opdbus.MailServer.3tched",
            "endpoints": {
                "smtp_submission": "0.0.0.0:587",
                "smtp_tls": "0.0.0.0:465",
                "imap": "0.0.0.0:143",
                "imaps": "0.0.0.0:993",
                "dovecot_lmtp": "/var/spool/postfix/private/dovecot-lmtp",
                "postfix_pickup": "/var/spool/postfix/private/pickup"
            },
            "container_ip": "10.200.0.2",
            "healthy": true,
            "last_error": null,
            "administration": {
                "revision": 0,
                "domains": [],
                "mailboxes": [],
                "aliases": [],
                "queue": {"messages": 0, "bytes": 0, "observed_at_unix": 0}
            }
        }))
        .build();

    let derived_state = super::schemars_adapter::plugin_schema_from_json(
        "mail_server",
        "2.0.0",
        "Postfix/Dovecot mail administration",
        &serde_json::to_value(schemars::schema_for!(MailServerState))
            .expect("mail state schema serializes"),
    );
    let mut administration = derived_state
        .fields
        .get("administration")
        .cloned()
        .expect("mail administration field is derived");
    administration.read_only = true;
    administration.description =
        "Native Postfix/Dovecot domains, mailboxes, aliases, quotas and queue summary".into();
    schema
        .fields
        .insert("administration".to_string(), administration);

    schema.methods.insert(
        "add_domain".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            DomainInput,
            DomainMutationOutput,
        >(
            "AddDomain",
            op_state_store::SideEffect::Mutation,
            false,
            "mail.write",
            "mut.service.mail.domain.add@v1",
        ),
    );
    schema.methods.insert(
        "remove_domain".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            DomainInput,
            DomainMutationOutput,
        >(
            "RemoveDomain",
            op_state_store::SideEffect::Mutation,
            false,
            "mail.write",
            "mut.service.mail.domain.remove@v1",
        ),
    );
    schema.methods.insert(
        "add_mailbox".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            AddMailboxInput,
            MailboxMutationOutput,
        >(
            "AddMailbox",
            op_state_store::SideEffect::Mutation,
            false,
            "mail.write",
            "mut.service.mail.mailbox.add@v1",
        ),
    );
    schema.methods.insert(
        "remove_mailbox".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            RemoveMailboxInput,
            MailboxMutationOutput,
        >(
            "RemoveMailbox",
            op_state_store::SideEffect::Mutation,
            false,
            "mail.write",
            "mut.service.mail.mailbox.remove@v1",
        ),
    );
    schema.methods.insert(
        "add_alias".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            AliasInput,
            AliasMutationOutput,
        >(
            "AddAlias",
            op_state_store::SideEffect::Mutation,
            false,
            "mail.write",
            "mut.service.mail.alias.add@v1",
        ),
    );
    schema.methods.insert(
        "remove_alias".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            RemoveAliasInput,
            AliasMutationOutput,
        >(
            "RemoveAlias",
            op_state_store::SideEffect::Mutation,
            false,
            "mail.write",
            "mut.service.mail.alias.remove@v1",
        ),
    );
    schema.methods.insert(
        "set_quota".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            QuotaInput,
            QuotaMutationOutput,
        >(
            "SetQuota",
            op_state_store::SideEffect::Mutation,
            false,
            "mail.write",
            "mut.service.mail.quota.set@v1",
        ),
    );
    schema.methods.insert(
        "get_queue".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            super::plugin_scaffold_helpers::EmptyInput,
            GetQueueOutput,
        >(
            "GetQueue",
            op_state_store::SideEffect::Read,
            true,
            "mail.read",
            "obs.service.mail.queue.get@v1",
        ),
    );
    schema.methods.insert(
        "flush_queue".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            super::plugin_scaffold_helpers::EmptyInput,
            FlushQueueOutput,
        >(
            "FlushQueue",
            op_state_store::SideEffect::Mutation,
            false,
            "mail.write",
            "mut.service.mail.queue.flush@v1",
        ),
    );
    schema.capabilities.insert(
        "mail.write".to_string(),
        op_state_store::CapabilityDecl {
            id: "mail.write".to_string(),
            description: "Grants: add_domain, remove_domain, add_mailbox, remove_mailbox, add_alias, remove_alias, set_quota, flush_queue.".to_string(),
        },
    );
    schema.capabilities.insert(
        "mail.read".to_string(),
        op_state_store::CapabilityDecl {
            id: "mail.read".to_string(),
            description: "Grants: get_queue, connect.".to_string(),
        },
    );

    schema
}

// Self-registration: the plugin registry discovers this via inventory
// (single source of the catalog; no central dispatch list).
inventory::submit! {
    crate::default_registry::PluginReg::new("mail_server", |_ctx| std::sync::Arc::new(MailServerPlugin::new()))
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct DomainInput {
    pub domain: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct AddMailboxInput {
    pub domain: String,
    pub user: String,
    /// Dovecot passwd-file value, including its password scheme prefix.
    /// The native owner never returns this credential.
    #[schemars(extend("x-secret" = true))]
    pub password_hash: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct AliasInput {
    pub domain: String,
    pub alias: String,
    pub destination: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct RemoveMailboxInput {
    pub domain: String,
    pub user: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct RemoveAliasInput {
    pub domain: String,
    pub alias: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct QuotaInput {
    pub domain: String,
    pub user: String,
    pub quota_mb: u32,
}

#[derive(
    Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize, schemars::JsonSchema,
)]
pub struct MailAdministrationState {
    pub revision: u64,
    pub domains: Vec<String>,
    pub mailboxes: Vec<MailMailbox>,
    pub aliases: Vec<MailAlias>,
    pub queue: MailQueueSummary,
}

#[derive(
    Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, schemars::JsonSchema,
)]
pub struct MailMailbox {
    pub address: String,
    pub maildir: String,
    pub quota_bytes: Option<u64>,
}

#[derive(
    Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, schemars::JsonSchema,
)]
pub struct MailAlias {
    pub address: String,
    pub destination: String,
}

#[derive(
    Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize, schemars::JsonSchema,
)]
pub struct MailQueueSummary {
    pub messages: u64,
    pub bytes: u64,
    pub observed_at_unix: i64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct DomainMutationOutput {
    pub domain: String,
    pub changed: bool,
    pub domains: Vec<String>,
    pub revision: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct MailboxMutationOutput {
    pub address: String,
    pub maildir: String,
    pub quota_bytes: Option<u64>,
    pub changed: bool,
    pub maildir_preserved: bool,
    pub revision: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct AliasMutationOutput {
    pub address: String,
    pub destination: Option<String>,
    pub changed: bool,
    pub revision: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct QuotaMutationOutput {
    pub address: String,
    pub quota_bytes: u64,
    pub revision: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct MailQueueRecipient {
    pub address: String,
    pub original_address: Option<String>,
    /// Postfix's reason class, such as `defer` or `bounce`.
    pub reason_kind: Option<String>,
    pub delay_reason: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct MailQueueEntry {
    pub queue_name: String,
    pub queue_id: String,
    pub arrival_time: i64,
    pub message_size: u64,
    pub forced_expire: bool,
    pub sender: String,
    pub recipients: Vec<MailQueueRecipient>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct GetQueueOutput {
    pub entries: Vec<MailQueueEntry>,
    pub summary: MailQueueSummary,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct FlushQueueOutput {
    /// True once Postfix accepted the at-least-once flush trigger. A lost
    /// transport reply can require readback and a repeat trigger.
    pub accepted: bool,
    pub before: MailQueueSummary,
    pub after: MailQueueSummary,
}
