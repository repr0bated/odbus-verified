//! Canonical `mail_server` method adapter.
//!
//! Generated gRPC enters MutationEngine and reaches this adapter only after the
//! sealed method, identity and capability checks. The adapter then uses the
//! private D-Bus peer owned by `op-mail-admin` inside `mail-3tched`; it never
//! reads Postfix/Dovecot files or starts their command-line tools itself.

use super::{
    AddMailboxInput, AliasInput, AliasMutationOutput, DomainInput, DomainMutationOutput,
    FlushQueueOutput, GetQueueOutput, MailAdministrationState, MailboxMutationOutput, QuotaInput,
    QuotaMutationOutput, RemoveAliasInput, RemoveMailboxInput,
};
use anyhow::{Context, Result};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;
use std::{path::Path, time::Duration};
use tokio::net::UnixStream;
use zbus::{Connection, Proxy};

pub const OBJECT_PATH: &str = "/org/opdbus/v1/plugins/mail_server";
pub const INTERFACE: &str = "org.opdbus.v1.plugins.mail_server.Native";
pub const DEFAULT_HOST_SOCKET: &str = "/run/ghostbridge/mail/admin.sock";
pub const DEFAULT_CONTAINER_SOCKET: &str = "/opt/run-mounts/ghostbridge/mail/admin.sock";
const CALL_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Debug, Clone)]
pub struct MailMethodReceipt {
    pub output: Value,
    /// Complete native administration readback after the operation.
    pub administration: MailAdministrationState,
}

pub async fn dispatch_mail_server_method(method: &str, args: &Value) -> Result<MailMethodReceipt> {
    let socket = std::env::var_os("OP_MAIL_ADMIN_SOCKET")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| DEFAULT_HOST_SOCKET.into());
    tokio::time::timeout(CALL_TIMEOUT, async {
        let stream = UnixStream::connect(&socket).await.with_context(|| {
            format!(
                "connect to mail administration D-Bus peer {}",
                socket.display()
            )
        })?;
        let connection = zbus::connection::Builder::unix_stream(stream)
            .p2p()
            .build()
            .await
            .context("authenticate mail administration D-Bus peer")?;
        dispatch_mail_server_method_on(&connection, method, args).await
    })
    .await
    .context("mail administration request timed out; read back native state before retrying")?
}

pub(crate) async fn dispatch_mail_server_method_on(
    connection: &Connection,
    method: &str,
    args: &Value,
) -> Result<MailMethodReceipt> {
    let proxy = Proxy::new(connection, INTERFACE, OBJECT_PATH, INTERFACE)
        .await
        .context("create mail administration D-Bus proxy")?;
    let reply: zbus::Result<String> = match method {
        "add_domain" => {
            let input: DomainInput = serde_json::from_value(args.clone())?;
            proxy.call("AddDomain", &(input.domain.as_str(),)).await
        }
        "remove_domain" => {
            let input: DomainInput = serde_json::from_value(args.clone())?;
            proxy.call("RemoveDomain", &(input.domain.as_str(),)).await
        }
        "add_mailbox" => {
            let input: AddMailboxInput = serde_json::from_value(args.clone())?;
            proxy
                .call(
                    "AddMailbox",
                    &(
                        input.domain.as_str(),
                        input.user.as_str(),
                        input.password_hash.as_str(),
                    ),
                )
                .await
        }
        "remove_mailbox" => {
            let input: RemoveMailboxInput = serde_json::from_value(args.clone())?;
            proxy
                .call(
                    "RemoveMailbox",
                    &(input.domain.as_str(), input.user.as_str()),
                )
                .await
        }
        "add_alias" => {
            let input: AliasInput = serde_json::from_value(args.clone())?;
            proxy
                .call(
                    "AddAlias",
                    &(
                        input.domain.as_str(),
                        input.alias.as_str(),
                        input.destination.as_str(),
                    ),
                )
                .await
        }
        "remove_alias" => {
            let input: RemoveAliasInput = serde_json::from_value(args.clone())?;
            proxy
                .call(
                    "RemoveAlias",
                    &(input.domain.as_str(), input.alias.as_str()),
                )
                .await
        }
        "set_quota" => {
            let input: QuotaInput = serde_json::from_value(args.clone())?;
            proxy
                .call(
                    "SetQuota",
                    &(input.domain.as_str(), input.user.as_str(), input.quota_mb),
                )
                .await
        }
        "get_queue" => {
            let _: super::super::plugin_scaffold_helpers::EmptyInput =
                serde_json::from_value(args.clone())?;
            proxy.call("GetQueue", &()).await
        }
        "flush_queue" => {
            let _: super::super::plugin_scaffold_helpers::EmptyInput =
                serde_json::from_value(args.clone())?;
            proxy.call("FlushQueue", &()).await
        }
        _ => return Err(anyhow::anyhow!("unknown mail_server method: {method}")),
    };
    let raw = reply.with_context(|| {
        if method == "get_queue" {
            "mail queue D-Bus reply unavailable; reconnect and read native state before retrying"
                .to_string()
        } else {
            format!(
                "mail_server.{method} D-Bus reply unavailable; the native effect may have completed; reconnect and read native state before retrying"
            )
        }
    })?;
    let output = match method {
        "add_domain" | "remove_domain" => typed_output::<DomainMutationOutput>(&raw, method)?,
        "add_mailbox" | "remove_mailbox" => typed_output::<MailboxMutationOutput>(&raw, method)?,
        "add_alias" | "remove_alias" => typed_output::<AliasMutationOutput>(&raw, method)?,
        "set_quota" => typed_output::<QuotaMutationOutput>(&raw, method)?,
        "get_queue" => typed_output::<GetQueueOutput>(&raw, method)?,
        "flush_queue" => typed_output::<FlushQueueOutput>(&raw, method)?,
        _ => unreachable!("method was rejected before native dispatch"),
    };
    let state_raw: String = proxy
        .call("GetState", &())
        .await
        .context("mail operation completed but native state readback failed")?;
    let administration = serde_json::from_str(&state_raw)
        .context("mail owner returned malformed administration state")?;
    Ok(MailMethodReceipt {
        output,
        administration,
    })
}

fn typed_output<T>(raw: &str, method: &str) -> Result<Value>
where
    T: DeserializeOwned + Serialize,
{
    let typed: T = serde_json::from_str(raw)
        .with_context(|| format!("mail owner returned malformed {method} receipt"))?;
    serde_json::to_value(typed).context("serialize typed mail owner receipt")
}

pub fn production_socket() -> &'static Path {
    Path::new(DEFAULT_HOST_SOCKET)
}
