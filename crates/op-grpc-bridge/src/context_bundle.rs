//! What the control-plane chatbot recalls before it answers.
//!
//! Before 2026-09-19 the production turn (`chat_router.Chat`) forwarded the
//! UI's messages verbatim: episodes were written to Cozo and vectorised, user
//! memory sat in namespaces, the context engine produced knowledge pushes —
//! and none of it reached a turn. The chatbot is the consumer now. This module
//! gathers, for the verified session scope only:
//!
//! 1. recent reasoning episodes of this principal/session (Cozo),
//! 2. namespace memory entries that match the query (Cozo),
//! 3. semantic recall through the cognitive memory tool when the Qdrant
//!    shuttle is configured (best-effort, bounded),
//! 4. session activity signals from the context engine,
//! 5. pending knowledge pushes for this session (drained on read),
//!
//! renders them into one bounded system message, and reports what it used so
//! the committed episode records the provenance of the answer.

use std::time::Duration;

use op_cognitive::CognitiveServer;
use op_cozo_store::EpisodeScope;

use crate::mutation_engine::MutationEngine;
use crate::specialist;

/// Characters of recalled context injected into one turn.
pub const TURN_BUDGET_CHARS: usize = 8_000;
/// Episodes scanned (most recent first) when looking for relevant ones.
const EPISODE_SCAN: usize = 120;
/// Episodes rendered when nothing matches the query lexically.
const EPISODE_FALLBACK: usize = 3;
/// Semantic recall must answer within this or is skipped for the turn.
const SEMANTIC_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug, Default, Clone)]
pub struct ContextBundle {
    pub episodes: Vec<String>,
    pub memories: Vec<String>,
    pub semantic: Vec<String>,
    pub activity: Vec<String>,
    pub pushes: Vec<String>,
}

impl ContextBundle {
    pub fn is_empty(&self) -> bool {
        self.episodes.is_empty()
            && self.memories.is_empty()
            && self.semantic.is_empty()
            && self.activity.is_empty()
            && self.pushes.is_empty()
    }

    /// Provenance labels for the episode's `tools_consulted`, e.g.
    /// `context:episodes=3`, so the audit record says what the answer drew on.
    pub fn provenance(&self) -> Vec<String> {
        let mut out = Vec::new();
        for (label, items) in [
            ("episodes", &self.episodes),
            ("memory", &self.memories),
            ("semantic", &self.semantic),
            ("activity", &self.activity),
            ("knowledge_pushes", &self.pushes),
        ] {
            if !items.is_empty() {
                out.push(format!("context:{label}={}", items.len()));
            }
        }
        out
    }

    /// One system message, within `budget_chars`, sections in priority order.
    pub fn render(&self, budget_chars: usize) -> Option<String> {
        if self.is_empty() {
            return None;
        }
        let mut text = String::from(
            "## Recalled for this session (ground your answer in it; do not repeat it verbatim)\n",
        );
        let mut spent = text.len();
        for (heading, items) in [
            ("Knowledge pushed for this session", &self.pushes),
            ("Session activity", &self.activity),
            ("Relevant memory", &self.memories),
            ("Semantic recall", &self.semantic),
            ("Recent episodes", &self.episodes),
        ] {
            if items.is_empty() {
                continue;
            }
            let header = format!("\n### {heading}\n");
            if spent + header.len() > budget_chars {
                break;
            }
            text.push_str(&header);
            spent += header.len();
            for item in items {
                let line = format!("- {}\n", item.trim());
                if spent + line.len() > budget_chars {
                    break;
                }
                text.push_str(&line);
                spent += line.len();
            }
        }
        Some(text)
    }
}

/// Gather the bundle for one turn of `scope` about `query`. Never fails the
/// turn: every source is best-effort and logged on error.
pub async fn assemble_for_turn(
    engine: &MutationEngine,
    scope: &EpisodeScope,
    query: &str,
) -> ContextBundle {
    let context = match crate::memory_binding::context_for(
        engine,
        &scope.principal_id,
        Some(&scope.session_id),
    )
    .await
    {
        Ok(context) => context,
        Err(error) => {
            tracing::warn!(%error, "context bundle: selected chat mode does not resolve; no memory recalled");
            return ContextBundle::default();
        }
    };
    assemble_for_context(engine, scope, query, &context).await
}

/// Assemble context using the already-admitted chat context. Callers that
/// resolved routing under the selection guard use this overload so memory
/// recall cannot observe a different mode after routing has been chosen.
pub async fn assemble_for_context(
    engine: &MutationEngine,
    scope: &EpisodeScope,
    query: &str,
    context: &op_plugins::state_plugins::project::ResolvedChatContext,
) -> ContextBundle {
    let mut bundle = ContextBundle::default();
    // The selected mode decides what memory a turn may recall (OD-65);
    // ui_render recalls nothing at all — not even pending pushes.
    if !context.mode.has_memory() {
        return bundle;
    }
    let namespaces = vec![context.memory_namespace.clone()];
    let terms = op_agents::dynamic::TaskContext {
        question: query.to_string(),
        ..Default::default()
    }
    .terms();

    // 1. Episodes, scoped to this principal/session.
    match engine.ctl_plane_chatbot_episode_cozo().await {
        Ok(store) => match store.list_scoped_episodes(scope, None, EPISODE_SCAN) {
            Ok(records) => {
                let mut recent: Vec<(String, String)> = Vec::new();
                for record in records.iter().rev() {
                    let goal = record.episode["goal_text"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string();
                    let decision = record.episode["decision_output"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string();
                    if goal.is_empty() && decision.is_empty() {
                        continue;
                    }
                    let started = record.episode["started_at"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string();
                    let line = format!(
                        "[{started}] asked: {} → answered: {}",
                        truncate(&goal, 200),
                        truncate(&decision, 300)
                    );
                    let haystack = format!("{goal} {decision}").to_ascii_lowercase();
                    let relevant = terms.iter().any(|t| haystack.contains(t.as_str()));
                    if relevant {
                        bundle.episodes.push(line.clone());
                    }
                    if recent.len() < EPISODE_FALLBACK {
                        recent.push((started, line));
                    }
                    if bundle.episodes.len() >= 6 {
                        break;
                    }
                }
                if bundle.episodes.is_empty() {
                    bundle.episodes = recent.into_iter().map(|(_, line)| line).collect();
                }
            }
            Err(error) => tracing::warn!(%error, "context bundle: episode recall failed"),
        },
        Err(error) => tracing::warn!(%error, "context bundle: episode store unavailable"),
    }

    let Ok(runtime) = engine.cognitive().await else {
        bundle.pushes = engine.take_pending_knowledge(&scope.session_id).await;
        return bundle;
    };

    // 2. Namespace memory (this session's namespace + shared pieces).
    bundle.memories = specialist::memory_snippets(
        runtime,
        &namespaces,
        &op_agents::dynamic::TaskContext {
            question: query.to_string(),
            ..Default::default()
        },
    )
    .await;

    // 3. Semantic recall, only when the shuttle exists, bounded in time.
    bundle.semantic = semantic_recall(runtime, &context.memory_namespace, query).await;

    // 4. Activity signals and 5. pending pushes.
    bundle.activity = specialist::activity_lines(runtime, &scope.session_id).await;
    bundle.pushes = engine.take_pending_knowledge(&scope.session_id).await;
    bundle
}

async fn semantic_recall(
    runtime: &CognitiveServer,
    namespace: &str,
    query: &str,
) -> Vec<String> {
    if runtime.qdrant_shuttle().is_none() {
        return Vec::new();
    }
    let input = simd_json::json!({
        "operation": "semantic_query",
        "query": query,
        "namespace": namespace,
        "container_id": namespace,
        "limit": 5,
    });
    let registry = runtime.tool_registry();
    let call = registry.execute("cognitive_memory", input);
    match tokio::time::timeout(SEMANTIC_TIMEOUT, call).await {
        Ok(Ok(result)) => {
            let value: serde_json::Value = serde_json::to_value(result).unwrap_or_default();
            value
                .get("results")
                .and_then(|r| r.as_array())
                .map(|items| {
                    items
                        .iter()
                        .take(5)
                        .map(|item| {
                            truncate(
                                &item
                                    .as_str()
                                    .map(str::to_owned)
                                    .unwrap_or_else(|| item.to_string()),
                                300,
                            )
                        })
                        .collect()
                })
                .unwrap_or_default()
        }
        Ok(Err(error)) => {
            tracing::debug!(%error, "context bundle: semantic recall unavailable");
            Vec::new()
        }
        Err(_) => {
            tracing::debug!("context bundle: semantic recall timed out");
            Vec::new()
        }
    }
}

fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_string()
    } else {
        let cut: String = text.chars().take(max).collect();
        format!("{cut}…")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_orders_by_priority_and_respects_budget() {
        let bundle = ContextBundle {
            episodes: vec!["ep1".into(), "ep2".into()],
            memories: vec!["mem1".into()],
            semantic: vec![],
            activity: vec!["act1".into()],
            pushes: vec!["push1".into()],
        };
        let text = bundle.render(TURN_BUDGET_CHARS).unwrap();
        let p = text.find("push1").unwrap();
        let a = text.find("act1").unwrap();
        let m = text.find("mem1").unwrap();
        let e = text.find("ep1").unwrap();
        assert!(p < a && a < m && m < e, "priority order: {text}");
        assert_eq!(
            bundle.provenance(),
            vec![
                "context:episodes=2",
                "context:memory=1",
                "context:activity=1",
                "context:knowledge_pushes=1"
            ]
        );

        let tiny = bundle.render(200).unwrap();
        assert!(tiny.contains("push1"));
        assert!(!tiny.contains("ep2"), "budget cut the tail: {tiny}");
        assert!(ContextBundle::default().render(1_000).is_none());
    }
}
