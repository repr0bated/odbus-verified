//! Scoped metadata-only notifications. The shared bridge core is the sole
//! producer; its notices are emitted after the durable state and SHM commit.

use std::future::Future;
use std::sync::Arc;

use anyhow::{Context, Result, bail, ensure};
use tokio::sync::broadcast;

use crate::core::{BridgeCore, StateNotice, StateScope};
use super::auth::{Session, Sessions};

/// Uses the same transient authenticated connection evidence as ExHook.
/// It owns neither state nor a second identity/grant registry.
#[derive(Clone)]
pub struct WakePublisher {
    sessions: Arc<Sessions>,
}

impl WakePublisher {
    pub(super) fn new(sessions: Arc<Sessions>) -> Self { Self { sessions } }

    /// Supervise alongside the listener. No broker availability check precedes
    /// binding or readiness. Native publication is attempted once per committed
    /// notice; a broker failure is reported without replaying any effect.
    pub async fn run<F>(self, core: BridgeCore, shutdown: F) -> Result<()>
    where F: Future<Output = ()> + Send,
    {
        let mut notices = core.subscribe_commits();
        tokio::pin!(shutdown);
        loop {
            let notice = tokio::select! {
                () = &mut shutdown => return Ok(()),
                result = notices.recv() => match result {
                    Ok(notice) => notice,
                    Err(broadcast::error::RecvError::Closed) => bail!("committed wake source closed"),
                    Err(broadcast::error::RecvError::Lagged(count)) => {
                        // Do not manufacture a sequence or silently report a
                        // healthy delivery task after losing committed notices.
                        bail!("committed wake source lagged by {count}; consumers must refresh SHM");
                    }
                },
            };
            if !notice.state_committed { continue; }
            ensure!(notice.event_id != 0, "committed wake has no durable event ID");
            tokio::select! {
                () = &mut shutdown => return Ok(()),
                result = self.publish_notice(&notice) => result?,
            }
        }
    }

    async fn publish_notice(&self, notice: &StateNotice) -> Result<()> {
        for session in self.sessions.connected().await {
            // On identity/registry changes all cached connections are checked,
            // including subscribers whose own projection did not change.
            if session.caller.authority.revalidate().await.is_err() {
                if let Err(error) = self.sessions.revoke(&session).await {
                    tracing::warn!(%error, "revoked MQTT connection kick failed; no wake was sent");
                }
                continue;
            }
            if !session.subscribed() || !scope_matches(&notice.scope, &session) { continue; }
            let readable = self.sessions.runtime.can_read_plugin(&session.caller, &notice.plugin_id).await;
            match readable {
                Ok(true) => {},
                Ok(false) => continue,
                Err(error) => {
                    tracing::warn!(%error, "committed MQTT wake refused by shared read admission");
                    continue;
                }
            }
            if !self.sessions.is_current(&session).await { continue; }
            // Nothing from the notice becomes state. The only broker payload
            // is the durable sequence; authorized consumers reread committed
            // SHM through the existing authenticated plugin door.
            if session.caller.authority.revalidate().await.is_err() {
                if let Err(error) = self.sessions.revoke(&session).await {
                    tracing::warn!(%error, "MQTT identity changed before wake; kick failed");
                }
                continue;
            }
            let mut effect = PublicationAttempt { in_flight: true };
            let result = op_plugins::state_plugins::emqx::dispatch_emqx_method(
                "publish_wake", &serde_json::json!({
                    "owner_key": session.owner_key,
                    "seq": notice.event_id,
                }),
            ).await.context("authenticated native EMQX wake publication failed");
            effect.in_flight = false;
            match result {
                Ok(response) => {
                    let response: op_plugins::state_plugins::emqx::PublishWakeOutput =
                        serde_json::from_value(response).context("invalid native wake acknowledgement")?;
                    ensure!(response.published && response.topic == session.topic,
                        "native wake acknowledgement did not match the admitted topic");
                }
                Err(error) => {
                    // The broker may have accepted the request before a lost
                    // response. Never retry this commit or claim non-execution.
                    tracing::warn!(%error, "MQTT wake outcome unknown; no retry; SHM remains authoritative");
                }
            }
        }
        Ok(())
    }
}

fn scope_matches(scope: &StateScope, session: &Session) -> bool {
    match scope {
        StateScope::Public => true,
        StateScope::Private { principal_id, session_id } =>
            principal_id == &session.caller.principal_id && session_id == &session.caller.session_id,
    }
}

struct PublicationAttempt { in_flight: bool }

impl Drop for PublicationAttempt {
    fn drop(&mut self) {
        if self.in_flight {
            tracing::warn!("MQTT wake cancelled after dispatch; outcome unknown; not retried");
        }
    }
}
