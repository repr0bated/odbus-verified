//! Per-task sudo elevation, enforced at MutationEngine's dispatch choke point.
//!
//! A protected call (the `elevation` plugin's policy) needs, besides the
//! caller's verified session and exact grant, a one-time ticket issued by
//! `elevation.authorize_task` for that exact call: same principal, session,
//! plugin, method and canonical arguments. Issuing a ticket checks the
//! caller's current sudo password: the principal's mapped Unix account must
//! be in `wheel` or `sudo` and PAM service `op-dbus-elevate` must accept the
//! password. Tickets live in memory, are single-use and expire after
//! `TICKET_TTL_SECONDS`; a bridge restart drops them. Repeated failures lock
//! the principal out for a while. The password is never stored or logged,
//! and `authorize_task` arguments are redacted from the chain record.
//!
//! The check sits behind [`SudoAuthority`] so a polkit-backed authority can
//! replace the PAM one without touching the protected methods.

use std::{
    collections::HashMap,
    path::PathBuf,
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

use crate::{
    interceptor::load_capability_grants, mutation_engine::MutationEngine, project_dispatch,
};

pub(crate) const PAM_SERVICE: &str = "op-dbus-elevate";
const TICKET_DOMAIN: &str = "opdbus.elevation.ticket.v1";
const MAX_FAILURES: u32 = 5;
const LOCKOUT_MINUTES: i64 = 15;
const MAX_TICKETS_PER_PRINCIPAL: usize = 16;

/// Decides whether `password` is the current sudo password of Unix `user`.
pub(crate) trait SudoAuthority: Send + Sync {
    fn verify(&self, user: &str, password: &str) -> Result<bool>;
}

/// Production authority: a sudoer (`wheel`/`sudo`) whose password PAM
/// service `op-dbus-elevate` accepts.
pub(crate) struct PamSudoAuthority;

impl SudoAuthority for PamSudoAuthority {
    fn verify(&self, user: &str, password: &str) -> Result<bool> {
        if !crate::pam_auth::is_sudoer(user)? {
            return Ok(false);
        }
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

/// The Unix account a principal elevates as, from the root-owned map
/// `{"<principal_id>": {"unix_user": "<name>"}}`.
fn unix_user(principal_id: &str) -> Result<String> {
    let path = std::env::var_os("OP_ELEVATION_USERS_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/etc/op-dbus/elevation-users.json"));
    let bytes = std::fs::read(&path).context("elevation user map is unavailable")?;
    let document: Value =
        serde_json::from_slice(&bytes).context("elevation user map is not JSON")?;
    let user = document
        .get(principal_id)
        .and_then(|entry| entry.get("unix_user"))
        .and_then(Value::as_str)
        .context("this principal has no elevation account")?;
    ensure!(
        !user.is_empty()
            && user.len() <= 32
            && user
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.')),
        "invalid elevation account name"
    );
    Ok(user.to_string())
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
        let Some(action) = protected_action(plugin, method, args) else {
            return Ok(());
        };
        let session = session_id
            .filter(|session| !session.is_empty())
            .with_context(|| format!("protected task {action} requires a verified session"))?;
        let key = ticket_key(principal_id, session, plugin, method, args);
        let now = Utc::now();
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

pub(crate) async fn dispatch(
    engine: &MutationEngine,
    method: &str,
    args: &Value,
    actor: &str,
    session: Option<&str>,
    genesis: Option<&str>,
) -> Result<Value> {
    match method {
        "list_protected_actions" => Ok(serde_json::to_value(ListProtectedActionsOutput {
            actions: protected_actions(),
            ticket_ttl_seconds: TICKET_TTL_SECONDS,
        })?),
        "authorize_task" => authorize(engine, args, actor, session, genesis).await,
        other => bail!("unknown elevation method {other}"),
    }
}

async fn authorize(
    engine: &MutationEngine,
    args: &Value,
    actor: &str,
    session: Option<&str>,
    genesis: Option<&str>,
) -> Result<Value> {
    project_dispatch::verified_session(engine, actor, session, genesis).await?;
    let session = session.context("verified session is required")?.to_string();
    ensure!(
        load_capability_grants(actor).contains(AUTHORIZE_CAPABILITY),
        "current exact {AUTHORIZE_CAPABILITY} grant is required"
    );
    let mut input: AuthorizeTaskInput =
        serde_json::from_value(args.clone()).context("invalid authorize_task arguments")?;
    let action = validate_authorize_input(&input)?;
    let elevation = engine.task_elevation();
    let now = Utc::now();
    elevation.check_lockout(actor, now)?;
    let user = unix_user(actor)?;
    let password = zeroize::Zeroizing::new(std::mem::take(&mut input.password));
    let authority = elevation.authority();
    let accepted = tokio::task::spawn_blocking(move || authority.verify(&user, &password))
        .await
        .context("sudo check worker failed")??;
    if !accepted {
        elevation.record_failure(actor, now);
        bail!("sudo password was not accepted");
    }
    lock(&elevation.failures).remove(actor);
    let expires_at = elevation.issue(actor, &session, &input, now)?;
    Ok(serde_json::to_value(AuthorizeTaskOutput {
        action_id: action.to_string(),
        expires_at: expires_at.to_rfc3339(),
    })?)
}

#[cfg(test)]
#[path = "task_elevation_tests.rs"]
mod tests;
