//! Commit-then-wake delivery of the context engine's knowledge pushes.
//!
//! A push names a session. This consumer resolves that session to its
//! verified scope through the identity sled (no session → no delivery),
//! commits the push as a durable row and private SHM document through the
//! owned `EpisodePublisher` effect (so the mutation carries the scope's
//! identity and the door stream wakes), and only then rings the EMQX
//! doorbell with `{"seq": revision}` on the scope's owner-key topic.
//!
//! The doorbell is best effort: the committed SHM document is the truth and
//! the door stream already woke; a broker outage only delays the UI.

use std::sync::Arc;
use std::time::{Duration, Instant};

use op_cognitive::context_awareness::KnowledgePush;
use op_cozo_store::EpisodeScope;
use tokio::sync::broadcast;

use crate::mutation_engine::MutationEngine;

/// After a failed wake publish, skip publishing for this long so a broker
/// outage cannot stall the consumer on connect timeouts.
const WAKE_BACKOFF: Duration = Duration::from_secs(30);

/// Spawn the consumer. Returns immediately; the task ends with the channel.
pub(crate) fn spawn(engine: Arc<MutationEngine>, mut pushes: broadcast::Receiver<KnowledgePush>) {
    tokio::spawn(async move {
        let mut wake_muted_until: Option<Instant> = None;
        loop {
            let push = match pushes.recv().await {
                Ok(push) => push,
                Err(broadcast::error::RecvError::Lagged(skipped)) => {
                    tracing::warn!(skipped, "knowledge delivery lagged; pushes not committed");
                    continue;
                }
                Err(broadcast::error::RecvError::Closed) => break,
            };
            match deliver(&engine, &push).await {
                Ok(Some((scope, seq))) => {
                    if wake_muted_until.is_some_and(|until| Instant::now() < until) {
                        continue;
                    }
                    match ring(&scope, seq).await {
                        Err(error) => {
                            tracing::debug!(%error, "doorbell wake not published");
                            wake_muted_until = Some(Instant::now() + WAKE_BACKOFF);
                        }
                        _ => {
                            wake_muted_until = None;
                        }
                    }
                }
                Ok(None) => {}
                Err(error) => {
                    tracing::warn!(%error, push = %push.id, "knowledge push not committed");
                }
            }
        }
    });
}

/// Resolve the push's session to its verified scope.
pub(crate) async fn scope_for_session(
    engine: &MutationEngine,
    session_id: &str,
) -> anyhow::Result<Option<EpisodeScope>> {
    let Some(sled) = crate::identity_sled_dispatch::stored_session(engine, session_id).await?
    else {
        return Ok(None);
    };
    let Some(genesis) = sled.genesis.clone() else {
        return Ok(None);
    };
    if !sled.is_anchored() {
        return Ok(None);
    }
    Ok(Some(EpisodeScope {
        principal_id: op_identity::session::derive_principal_id(&sled.wireguard_pubkey),
        session_id: sled.session_id,
        session_genesis: genesis,
    }))
}

/// The engine's push as the plugin contract's document, without the fields
/// the store and the digest add (identity, `push_id`, `content_hash`).
pub(crate) fn push_document(push: &KnowledgePush) -> serde_json::Value {
    serde_json::json!({
        "source_id": push.id,
        "created_at": push.timestamp.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        "trigger": format!("{:?}", push.trigger),
        "trigger_reason": push.trigger_reason,
        "relevance_score": f64::from(push.relevance_score).clamp(0.0, 1.0),
        "content": serde_json::to_value(&push.content).unwrap_or(serde_json::Value::Null),
        "suggested_action": push.suggested_action,
    })
}

/// Commit one push. `Ok(None)` means there was nothing to deliver to (no
/// anchored session, or no private projection configured).
async fn deliver(
    engine: &MutationEngine,
    push: &KnowledgePush,
) -> anyhow::Result<Option<(EpisodeScope, u64)>> {
    let Some(scope) = scope_for_session(engine, &push.session_id).await? else {
        tracing::debug!(session = %push.session_id, "knowledge push for a session without an anchored sled; dropped");
        return Ok(None);
    };
    let Ok(publisher) = engine.chatbot_episode_publisher() else {
        return Ok(None);
    };
    let snapshot = publisher
        .clone()
        .commit_knowledge(engine, scope.clone(), push_document(push))
        .await?;
    Ok(Some((scope, snapshot.revision)))
}

/// Ring the doorbell through the `emqx` plugin method — the only path that
/// touches the broker, so it is a declared, admitted mutation.
async fn ring(scope: &EpisodeScope, seq: u64) -> anyhow::Result<()> {
    let args = serde_json::json!({
        "owner_key": crate::wake_doorbell::owner_key(scope),
        "seq": seq,
    });
    op_plugins::state_plugins::emqx::dispatch_emqx_method("publish_wake", &args).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use op_cognitive::context_awareness::{KnowledgeContent, PushTrigger};

    #[test]
    fn push_document_carries_only_contract_fields_the_store_does_not_add() {
        let push = KnowledgePush {
            id: "p-1".into(),
            session_id: "s".into(),
            trigger: PushTrigger::ErrorAssistance,
            trigger_reason: "repeated build error".into(),
            relevance_score: 1.7,
            timestamp: chrono::Utc::now(),
            content: KnowledgeContent::ToolGuidance {
                tool_name: "rust_pro".into(),
                usage_tips: vec!["check first".into()],
                example_usage: serde_json::Value::Null,
            },
            suggested_action: Some("run rust_pro check".into()),
            proactive: true,
        };
        let doc = push_document(&push);
        let keys: Vec<&str> = doc
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            [
                "content",
                "created_at",
                "relevance_score",
                "source_id",
                "suggested_action",
                "trigger",
                "trigger_reason"
            ]
        );
        assert_eq!(
            doc["relevance_score"], 1.0,
            "clamped into the contract range"
        );
        assert_eq!(doc["trigger"], "ErrorAssistance");
        chrono::DateTime::parse_from_rfc3339(doc["created_at"].as_str().unwrap()).unwrap();
    }
}
