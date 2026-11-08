//! Per-task root-password elevation at the shared authenticated dispatch boundary.
//!
//! A protected call (the `elevation` plugin's policy) needs, besides the
//! caller's verified session and exact grant, a one-time ticket issued by
//! `elevation.authorize_task` for that exact call: same principal, session,
//! plugin, method and canonical arguments. Issuing a ticket checks the
//! root password through PAM service `op-dbus-elevate`. There is no
//! principal-to-Unix-account map. The authenticated caller remains the
//! principal; checking the password never switches it to root. Tickets live
//! in memory, are single-use and expire after
//! `TICKET_TTL_SECONDS`; a bridge restart drops them. Repeated failures lock
//! the principal out for a while. The password is never stored or logged,
//! and `authorize_task` arguments are redacted from the chain record.
//!
//! The check sits behind [`SudoAuthority`] so a polkit-backed authority can
//! replace the PAM one without touching the protected methods.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex, OnceLock},
};

use anyhow::{Context, Result, bail, ensure};
use chrono::{DateTime, Duration, Utc};
use op_plugins::state_plugins::{
    elevation::{
        AUTHORIZE_CAPABILITY, AuthorizeTaskInput, AuthorizeTaskOutput, ListProtectedActionsOutput,
        TICKET_TTL_SECONDS, protected_action, protected_actions, validate_authorize_input,
    },
    json_render::gallery::canonical_json,
};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::runtime::{Runtime, VerifiedCaller};

pub(crate) const PAM_SERVICE: &str = "op-dbus-elevate";
const TICKET_DOMAIN: &str = "opdbus.elevation.ticket.v1";
const MAX_FAILURES: u32 = 5;
const LOCKOUT_MINUTES: i64 = 15;
const MAX_TICKETS_PER_PRINCIPAL: usize = 16;

/// Checks a password against the fixed elevation account, not the caller.
pub(crate) trait SudoAuthority: Send + Sync {
    fn verify(&self, user: &str, password: &str) -> Result<bool>;
}

/// Production authority: PAM verifies the root password and account validity.
pub(crate) struct PamSudoAuthority;

impl SudoAuthority for PamSudoAuthority {
    fn verify(&self, user: &str, password: &str) -> Result<bool> {
        ensure!(user == "root", "elevation checks only the root password");
        crate::pam_auth::check_password(PAM_SERVICE, user, password)
    }
}

struct Ticket {
    principal_id: String,
    expires_at: DateTime<Utc>,
}

#[derive(Default)]
struct Failures {
    count: u32,
    locked_until: Option<DateTime<Utc>>,
}

#[derive(Default)]
pub(crate) struct TaskElevation {
    tickets: Mutex<HashMap<String, Ticket>>,
    failures: Mutex<HashMap<String, Failures>>,
    authority: OnceLock<Arc<dyn SudoAuthority>>,
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    // Every mutation is a single insert/remove/assignment; a poisoned map is
    // still consistent.
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn ticket_key(principal: &str, session: &str, plugin: &str, method: &str, args: &Value) -> String {
    let material = serde_json::json!([TICKET_DOMAIN, principal, session, plugin, method, args]);
    hex::encode(Sha256::digest(canonical_json(&material)))
}

impl TaskElevation {
    fn authority(&self) -> Arc<dyn SudoAuthority> {
        self.authority
            .get_or_init(|| Arc::new(PamSudoAuthority))
            .clone()
    }

    #[cfg(test)]
    pub(crate) fn set_authority_for_test(&self, authority: Arc<dyn SudoAuthority>) {
        let _ = self.authority.set(authority);
    }

    /// Consume the ticket for a protected call; unprotected calls pass.
    pub(crate) fn consume(
        &self,
        principal_id: &str,
        session_id: Option<&str>,
        plugin: &str,
        method: &str,
        args: &Value,
    ) -> Result<()> {
        self.consume_at(principal_id, session_id, plugin, method, args, Utc::now())
    }

    fn consume_at(
        &self,
        principal_id: &str,
        session_id: Option<&str>,
        plugin: &str,
        method: &str,
        args: &Value,
        now: DateTime<Utc>,
    ) -> Result<()> {
        let Some(action) = protected_action(plugin, method, args) else {
            return Ok(());
        };
        let session = session_id
            .filter(|session| !session.is_empty())
            .with_context(|| format!("protected task {action} requires a verified session"))?;
        let key = ticket_key(principal_id, session, plugin, method, args);
        let mut tickets = lock(&self.tickets);
        tickets.retain(|_, ticket| ticket.expires_at > now);
        match tickets.remove(&key) {
            Some(ticket) if ticket.principal_id == principal_id => Ok(()),
            _ => bail!(
                "protected task {action} needs sudo elevation for this exact call; authorize it with elevation.authorize_task"
            ),
        }
    }

    fn check_lockout(&self, principal_id: &str, now: DateTime<Utc>) -> Result<()> {
        let mut failures = lock(&self.failures);
        let entry = failures.entry(principal_id.to_string()).or_default();
        if let Some(until) = entry.locked_until {
            if until > now {
                bail!(
                    "too many failed sudo elevations; try again after {}",
                    until.to_rfc3339()
                );
            }
            *entry = Failures::default();
        }
        Ok(())
    }

    fn record_failure(&self, principal_id: &str, now: DateTime<Utc>) {
        let mut failures = lock(&self.failures);
        let entry = failures.entry(principal_id.to_string()).or_default();
        entry.count += 1;
        if entry.count >= MAX_FAILURES {
            entry.locked_until = Some(now + Duration::minutes(LOCKOUT_MINUTES));
        }
    }

    fn issue(
        &self,
        principal_id: &str,
        session: &str,
        input: &AuthorizeTaskInput,
        now: DateTime<Utc>,
    ) -> Result<DateTime<Utc>> {
        let expires_at = now + Duration::seconds(TICKET_TTL_SECONDS);
        let mut tickets = lock(&self.tickets);
        tickets.retain(|_, ticket| ticket.expires_at > now);
        ensure!(
            tickets
                .values()
                .filter(|ticket| ticket.principal_id == principal_id)
                .count()
                < MAX_TICKETS_PER_PRINCIPAL,
            "too many outstanding elevation tickets"
        );
        tickets.insert(
            ticket_key(
                principal_id,
                session,
                &input.plugin,
                &input.method,
                &input.args,
            ),
            Ticket {
                principal_id: principal_id.to_string(),
                expires_at,
            },
        );
        Ok(expires_at)
    }
}

/// Audit the declared target, never the password or arbitrary nested arguments.
pub(crate) fn audit_arguments(args: &Value) -> Value {
    serde_json::json!({
        "plugin": args.get("plugin").and_then(Value::as_str),
        "method": args.get("method").and_then(Value::as_str),
        "redacted": true,
    })
}

pub(crate) async fn dispatch(
    runtime: &Runtime,
    caller: &VerifiedCaller,
    method: &str,
    args: &Value,
) -> Result<Value> {
    match method {
        "list_protected_actions" => Ok(serde_json::to_value(ListProtectedActionsOutput {
            actions: protected_actions(),
            ticket_ttl_seconds: TICKET_TTL_SECONDS,
        })?),
        "authorize_task" => authorize(runtime, caller, args).await,
        other => bail!("unknown elevation method {other}"),
    }
}

async fn authorize(runtime: &Runtime, caller: &VerifiedCaller, args: &Value) -> Result<Value> {
    runtime.require_capability(caller, AUTHORIZE_CAPABILITY).await?;
    let mut input: AuthorizeTaskInput =
        serde_json::from_value(args.clone()).context("invalid authorize_task arguments")?;
    let action = validate_authorize_input(&input)?;
    // A password cannot grant an operation that the original principal lacks.
    runtime.authorize_nested(caller, &input.plugin, &input.method).await?;
    let elevation = runtime.task_elevation();
    let now = Utc::now();
    elevation.check_lockout(&caller.principal_id, now)?;
    let password = zeroize::Zeroizing::new(std::mem::take(&mut input.password));
    let authority = elevation.authority();
    let accepted = tokio::task::spawn_blocking(move || authority.verify("root", &password))
        .await.context("root password check worker failed")??;
    if !accepted {
        elevation.record_failure(&caller.principal_id, now);
        bail!("root password was not accepted");
    }
    runtime.require_capability(caller, AUTHORIZE_CAPABILITY).await?;
    runtime.authorize_nested(caller, &input.plugin, &input.method).await?;
    lock(&elevation.failures).remove(&caller.principal_id);
    let expires_at = elevation.issue(&caller.principal_id, &caller.session_id, &input, Utc::now())?;
    Ok(serde_json::to_value(AuthorizeTaskOutput {
        action_id: action.to_string(),
        expires_at: expires_at.to_rfc3339(),
    })?)
}

#[cfg(test)]
#[path = "task_elevation_tests.rs"]
mod tests;
