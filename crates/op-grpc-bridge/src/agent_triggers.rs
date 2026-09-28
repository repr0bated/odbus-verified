//! The exhook side of OD-63: match a published topic against the sealed
//! trigger table and enqueue a run. Match + enqueue only — never blocks the
//! broker's publish path, never changes its verdict, never reads a payload
//! that no trigger asked for.

use std::sync::Arc;
use std::time::{Duration, Instant};

use op_plugins::state_plugins::agent_triggers::{AgentTrigger, enabled_triggers, topic_matches};
use tokio::sync::Mutex;

use crate::agent_runs::{RunQueue, TriggerEvent};

/// How long a read of the trigger document is reused by the matcher.
const CACHE_TTL: Duration = Duration::from_secs(5);

#[derive(Default)]
struct Cache {
    read_at: Option<Instant>,
    triggers: Vec<AgentTrigger>,
}

/// Matcher with a short-lived cache of the enabled triggers.
#[derive(Clone)]
pub(crate) struct TriggerMatcher {
    cache: Arc<Mutex<Cache>>,
    queue: RunQueue,
}

impl TriggerMatcher {
    pub(crate) fn new(queue: RunQueue) -> Self {
        Self {
            cache: Arc::new(Mutex::new(Cache::default())),
            queue,
        }
    }

    async fn triggers(&self) -> Vec<AgentTrigger> {
        let mut cache = self.cache.lock().await;
        if cache.read_at.is_some_and(|at| at.elapsed() < CACHE_TTL) {
            return cache.triggers.clone();
        }
        // Fail closed: no document or a malformed one means no triggers.
        let triggers = match op_plugins::state_plugins::runtime_documents::runtime_documents().await
        {
            Ok(documents) => enabled_triggers(&documents).await.unwrap_or_else(|error| {
                tracing::warn!(%error, "agent triggers unreadable; none active");
                Vec::new()
            }),
            Err(error) => {
                tracing::debug!(%error, "runtime documents unavailable; no agent triggers");
                Vec::new()
            }
        };
        cache.read_at = Some(Instant::now());
        cache.triggers = triggers.clone();
        triggers
    }

    /// Enqueue a run for every enabled trigger whose filter matches `topic`.
    /// Returns how many runs were enqueued (dropped ones are counted in the
    /// queue's stats).
    pub(crate) async fn on_publish(&self, topic: &str, payload: &[u8]) -> usize {
        let triggers = self.triggers().await;
        let mut enqueued = 0;
        for trigger in triggers
            .into_iter()
            .filter(|t| topic_matches(&t.topic_filter, topic))
        {
            let limit = trigger.max_payload_bytes as usize;
            let bounded = payload[..payload.len().min(limit)].to_vec();
            if self.queue.try_enqueue(TriggerEvent {
                trigger,
                topic: topic.to_string(),
                payload: bounded,
                received: Instant::now(),
            }) {
                enqueued += 1;
            }
        }
        if enqueued > 0 {
            tracing::debug!(stats = %self.queue.stats.snapshot(), "agent run queue");
        }
        enqueued
    }

    /// Test seam: seed the cache without a runtime document.
    #[cfg(test)]
    pub(crate) async fn seed(&self, triggers: Vec<AgentTrigger>) {
        let mut cache = self.cache.lock().await;
        cache.read_at = Some(Instant::now());
        cache.triggers = triggers;
    }
}
