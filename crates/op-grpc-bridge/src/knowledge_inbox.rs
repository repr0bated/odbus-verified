//! Per-session inbox for the context engine's knowledge pushes.
//!
//! The context engine publishes [`KnowledgePush`]es on a broadcast channel.
//! Nothing consumed them before; the control-plane chatbot is the consumer
//! now. A background task drains the channel into this inbox, keyed by the
//! push's session, and the next chatbot turn or `specialist` call for that
//! session takes them as context lines. Bounded per session and expiring, so
//! a session that never returns cannot grow the inbox.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};

use op_cognitive_mcp::context_awareness::KnowledgePush;
use tokio::sync::{Mutex, broadcast};

/// Pushes kept per session.
const MAX_PER_SESSION: usize = 32;
/// A push older than this is stale by the time a turn reads it.
const TTL: Duration = Duration::from_secs(15 * 60);

/// Rendered pushes per session, oldest first.
type Queues = HashMap<String, VecDeque<(Instant, String)>>;

#[derive(Debug, Clone, Default)]
pub struct KnowledgeInbox {
    inner: Arc<Mutex<Queues>>,
}

impl KnowledgeInbox {
    /// Spawn the consumer for `pushes`. Idempotent per receiver; calling it
    /// twice with two receivers duplicates delivery, so the engine calls it
    /// once when the cognitive runtime is created.
    pub fn consume(&self, mut pushes: broadcast::Receiver<KnowledgePush>) {
        let inner = Arc::clone(&self.inner);
        tokio::spawn(async move {
            loop {
                match pushes.recv().await {
                    Ok(push) => {
                        let line = render(&push);
                        let mut map = inner.lock().await;
                        let queue = map.entry(push.session_id.clone()).or_default();
                        queue.push_back((Instant::now(), line));
                        while queue.len() > MAX_PER_SESSION {
                            queue.pop_front();
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(skipped)) => {
                        tracing::warn!(skipped, "knowledge inbox lagged; pushes dropped");
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        });
    }

    /// Store one push directly (tests and in-process producers).
    #[cfg(test)]
    pub async fn offer(&self, push: &KnowledgePush) {
        let mut map = self.inner.lock().await;
        let queue = map.entry(push.session_id.clone()).or_default();
        queue.push_back((Instant::now(), render(push)));
        while queue.len() > MAX_PER_SESSION {
            queue.pop_front();
        }
    }

    /// Drain the pushes for `scope`, oldest first, dropping expired ones.
    pub async fn take(&self, scope: &str) -> Vec<String> {
        let mut map = self.inner.lock().await;
        let Some(queue) = map.remove(scope) else {
            return Vec::new();
        };
        let now = Instant::now();
        queue
            .into_iter()
            .filter(|(at, _)| now.duration_since(*at) <= TTL)
            .map(|(_, line)| line)
            .collect()
    }

    /// Pending pushes for `scope` without draining (diagnostics).
    #[cfg(test)]
    pub async fn pending(&self, scope: &str) -> usize {
        self.inner
            .lock()
            .await
            .get(scope)
            .map(|q| q.len())
            .unwrap_or(0)
    }
}

/// One context line per push: trigger, why, and the knowledge itself.
fn render(push: &KnowledgePush) -> String {
    let mut line = format!(
        "[knowledge:{:?} score={:.2}] {}",
        push.trigger, push.relevance_score, push.trigger_reason
    );
    let body = serde_json::to_string(&push.content).unwrap_or_default();
    if !body.is_empty() && body != "null" {
        line.push_str(" — ");
        line.push_str(&body);
    }
    if let Some(action) = push.suggested_action.as_deref() {
        line.push_str(" (suggested: ");
        line.push_str(action);
        line.push(')');
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;
    use op_cognitive_mcp::context_awareness::{KnowledgeContent, PushTrigger};

    fn push(session: &str, reason: &str) -> KnowledgePush {
        KnowledgePush {
            id: format!("p-{reason}"),
            session_id: session.into(),
            trigger: PushTrigger::ErrorAssistance,
            trigger_reason: reason.into(),
            relevance_score: 0.9,
            timestamp: chrono::Utc::now(),
            content: KnowledgeContent::ToolGuidance {
                tool_name: "rust_pro".into(),
                usage_tips: vec!["run check before build".into()],
                example_usage: serde_json::Value::Null,
            },
            suggested_action: Some("run rust_pro check".into()),
            proactive: true,
        }
    }

    #[tokio::test]
    async fn pushes_are_scoped_bounded_and_drained_once() {
        let inbox = KnowledgeInbox::default();
        for i in 0..40 {
            inbox.offer(&push("s1", &format!("r{i}"))).await;
        }
        inbox.offer(&push("s2", "other")).await;
        assert_eq!(inbox.pending("s1").await, MAX_PER_SESSION);
        let lines = inbox.take("s1").await;
        assert_eq!(lines.len(), MAX_PER_SESSION);
        assert!(lines[0].contains("r8"), "oldest kept first: {}", lines[0]);
        assert!(
            lines
                .iter()
                .all(|l| l.contains("suggested: run rust_pro check"))
        );
        assert!(inbox.take("s1").await.is_empty(), "drained");
        assert_eq!(inbox.take("s2").await.len(), 1);
    }

    #[tokio::test]
    async fn consumer_delivers_broadcast_pushes_to_the_session() {
        let inbox = KnowledgeInbox::default();
        let (tx, rx) = broadcast::channel(8);
        inbox.consume(rx);
        tx.send(push("s9", "from-engine")).unwrap();
        for _ in 0..50 {
            if inbox.pending("s9").await == 1 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(inbox.take("s9").await.len(), 1);
    }
}
