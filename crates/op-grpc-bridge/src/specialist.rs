//! `cognitive_mcp.specialist` — a persona assembled per call from the shared
//! piece pool and answered by the selected router model.
//!
//! Owner direction (2026-09-19): static agents are recreated dynamically from
//! pieces shared across agents with similar functions. The pool lives in
//! [`op_agents::dynamic::PieceLibrary`]; this module scopes it to the verified
//! caller, adds what the session already knows (namespace memory, activity
//! signals, pending knowledge pushes), composes the persona, and completes it
//! through `tched_router.Chat`'s guarded native path. The assembled persona
//! (piece ids and sources) is returned with the answer so the decision is
//! auditable, and the call is recorded as session activity so the context
//! engine can follow up.

use std::sync::Arc;

use anyhow::Context as _;
use op_agents::dynamic::{assemble, AssembledPersona, PieceLibrary, TaskContext};
use op_cognitive_mcp::context_awareness::ActivityType;
use op_cognitive_mcp::memory_store::EntryQuery;
use op_cognitive_mcp::CognitiveMcpServer;
use op_plugins::state_plugins::cognitive_mcp::{
    SpecialistInput, SpecialistOutput, SpecialistPersona,
};
use op_plugins::state_plugins::tched_router::{ChatInput, TchedChatMessage};
use tokio::sync::RwLock;

use crate::mutation_engine::MutationEngine;

/// Verified identity of the caller of a cognitive method.
#[derive(Debug, Clone, Copy)]
pub struct CognitiveCaller<'a> {
    pub actor_id: &'a str,
    pub session_id: Option<&'a str>,
    pub session_genesis: Option<&'a str>,
}

impl CognitiveCaller<'_> {
    /// The session the activity and memory are scoped to: the anchored session
    /// when the caller named one, else the actor itself.
    pub fn scope(&self) -> &str {
        self.session_id.unwrap_or(self.actor_id)
    }
}

/// Memory namespace whose entries are shared agent pieces. Entries are either
/// a plain string (a knowledge piece) or `{kind, domains, tags, title, body}`.
pub const PIECES_NAMESPACE: &str = "agent_pieces";

/// Prompt budget for the assembled persona (chars, before the question).
const PERSONA_BUDGET_CHARS: usize = 12_000;
/// Hot executors a specialist may direct. These are HOT `cognitive_mcp`
/// methods the caller can invoke next; the persona is told they exist.
const EXECUTORS: [&str; 3] = ["rust_pro", "python_pro", "typescript_pro"];

/// Shared pool: seeded from the static catalog once, refreshed with memory
/// pieces on every call (they are the freshest shared knowledge).
static LIBRARY: std::sync::LazyLock<Arc<RwLock<PieceLibrary>>> =
    std::sync::LazyLock::new(|| Arc::new(RwLock::new(PieceLibrary::seeded_from_catalog())));

pub async fn consult(
    engine: &MutationEngine,
    caller: CognitiveCaller<'_>,
    args: &serde_json::Value,
) -> anyhow::Result<serde_json::Value> {
    let input: SpecialistInput =
        serde_json::from_value(args.clone()).context("specialist: invalid input")?;
    let question = input.question.trim();
    anyhow::ensure!(
        !question.is_empty(),
        "specialist: 'question' must not be empty"
    );

    let runtime = engine.cognitive_mcp().await?;
    let scope = caller.scope().to_string();

    // 1. Task context from what the session already knows.
    let mut task = TaskContext {
        question: question.to_string(),
        path: input.path.clone(),
        domain_hints: input.domain_hints.clone(),
        activity: activity_lines(runtime, &scope).await,
        snippets: Vec::new(),
        executors: EXECUTORS.iter().map(|s| s.to_string()).collect(),
    };
    task.snippets = memory_snippets(runtime, &scope, &task).await;
    task.snippets
        .extend(engine.take_pending_knowledge(&scope).await);

    // 2. Refresh shared pieces from memory, then compose the persona.
    refresh_memory_pieces(runtime).await;
    let persona: AssembledPersona = {
        let library = LIBRARY.read().await;
        assemble(&library, &task, PERSONA_BUDGET_CHARS)
    };

    // 3. Record the consultation as session activity before answering, so a
    //    failed completion still leaves the signal.
    runtime
        .context_engine()
        .record_activity(
            &scope,
            ActivityType::Query,
            format!("specialist({}): {}", operation_name(&input), question),
            serde_json::json!({
                "persona": persona.name,
                "domains": persona.domains,
                "pieces": persona.piece_ids.len(),
                "path": input.path,
            }),
        )
        .await;

    // 4. Complete through the guarded native router path.
    let output = crate::tched_router_runtime::TchedRouterRuntimeClient::from_env()
        .chat_under_guard(ChatInput {
            message: String::new(),
            messages: vec![
                TchedChatMessage {
                    role: "system".into(),
                    content: persona.system_prompt.clone(),
                },
                TchedChatMessage {
                    role: "user".into(),
                    content: format!("[{}] {question}", operation_name(&input)),
                },
            ],
            provider: String::new(),
            model: String::new(),
            conversation_id: caller.session_id.map(str::to_owned),
        })
        .await
        .context("specialist: native completion failed")?;

    let result = SpecialistOutput {
        answer: output.content,
        persona: SpecialistPersona {
            name: persona.name,
            domains: persona.domains,
            piece_ids: persona.piece_ids,
            sources: persona
                .sources
                .iter()
                .map(|s| serde_json::to_string(s).unwrap_or_default())
                .collect(),
        },
        context_used: task.snippets.len() + task.activity.len(),
        provider: output.provider,
        model: output.model,
    };
    Ok(serde_json::to_value(result)?)
}

/// Compose a persona for `task` from the shared pool, refreshed with memory
/// pieces when the cognitive runtime is available (best effort). Used by
/// event-triggered runs, which have no caller session of their own.
pub(crate) async fn assemble_for_task(
    engine: &MutationEngine,
    task: &TaskContext,
) -> AssembledPersona {
    if let Ok(runtime) = engine.cognitive_mcp().await {
        refresh_memory_pieces(runtime).await;
    }
    let library = LIBRARY.read().await;
    assemble(&library, task, PERSONA_BUDGET_CHARS)
}

fn operation_name(input: &SpecialistInput) -> String {
    serde_json::to_value(&input.operation)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_else(|| "consult".into())
}

/// Recent session signals from the context engine, as short lines.
pub(crate) async fn activity_lines(runtime: &CognitiveMcpServer, scope: &str) -> Vec<String> {
    let Some(signals) = runtime.context_engine().get_session_signals(scope).await else {
        return Vec::new();
    };
    let mut lines = Vec::new();
    if let Some(topics) = signals.get("current_topics").and_then(|v| v.as_array()) {
        let topics: Vec<&str> = topics.iter().filter_map(|t| t.as_str()).collect();
        if !topics.is_empty() {
            lines.push(format!("current topics: {}", topics.join(", ")));
        }
    }
    if signals
        .get("recent_error_count")
        .and_then(|v| v.as_u64())
        .unwrap_or(0)
        > 0
    {
        lines.push(format!(
            "recent errors in this session: {}",
            signals["recent_error_count"]
        ));
    }
    if signals
        .get("is_stuck")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        lines.push("the session appears stuck on the same problem".into());
    }
    lines
}

/// Memory entries the caller may see, matched to the task: the scope's own
/// namespace(s) and the shared pieces namespace, filtered by task terms.
pub(crate) async fn memory_snippets(
    runtime: &CognitiveMcpServer,
    scope: &str,
    task: &TaskContext,
) -> Vec<String> {
    let store = runtime.memory_store();
    let terms = task.terms();
    let mut snippets = Vec::new();
    for namespace_name in [scope, PIECES_NAMESPACE] {
        let Ok(Some(_)) = store.get_namespace_by_name(namespace_name).await else {
            continue;
        };
        let Ok(entries) = store
            .query_entries(EntryQuery {
                // `namespace_id` is the namespace name (see EntryQuery).
                namespace_id: Some(namespace_name.to_string()),
                key_pattern: None,
                tags: None,
                limit: Some(200),
                offset: None,
            })
            .await
        else {
            continue;
        };
        for entry in entries {
            let text = match &entry.value {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            let haystack =
                format!("{} {} {}", entry.key, entry.tags.join(" "), text).to_ascii_lowercase();
            if terms.iter().any(|t| haystack.contains(t.as_str())) {
                snippets.push(format!(
                    "[{}/{}] {}",
                    namespace_name,
                    entry.key,
                    truncate(&text, 400)
                ));
            }
            if snippets.len() >= 12 {
                return snippets;
            }
        }
    }
    snippets
}

/// Merge the `agent_pieces` namespace into the shared pool.
async fn refresh_memory_pieces(runtime: &CognitiveMcpServer) {
    let store = runtime.memory_store();
    let Ok(Some(_)) = store.get_namespace_by_name(PIECES_NAMESPACE).await else {
        return;
    };
    let Ok(entries) = store
        .query_entries(EntryQuery {
            namespace_id: Some(PIECES_NAMESPACE.to_string()),
            key_pattern: None,
            tags: None,
            limit: Some(500),
            offset: None,
        })
        .await
    else {
        return;
    };
    let mut library = LIBRARY.write().await;
    for entry in entries {
        library.add_from_memory_entry(PIECES_NAMESPACE, &entry.key, &entry.value);
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
    fn scope_prefers_the_anchored_session() {
        let with_session = CognitiveCaller {
            actor_id: "actor",
            session_id: Some("sess"),
            session_genesis: None,
        };
        assert_eq!(with_session.scope(), "sess");
        let bare = CognitiveCaller {
            actor_id: "actor",
            session_id: None,
            session_genesis: None,
        };
        assert_eq!(bare.scope(), "actor");
    }

    #[test]
    fn shared_pool_is_seeded_once_from_the_catalog() {
        let library = LIBRARY.blocking_read();
        assert!(library.len() > 50);
    }
}
