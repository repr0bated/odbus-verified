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

use std::{future::Future, pin::Pin, sync::Arc};

use anyhow::Context as _;
use op_agents::dynamic::{assemble, AssembledPersona, PieceLibrary, TaskContext};
use op_cognitive_mcp::context_awareness::ActivityType;
use op_cognitive_mcp::memory_store::EntryQuery;
use op_cognitive_mcp::CognitiveMcpServer;
use op_plugins::state_plugins::cognitive_mcp::{
    SpecialistInput, SpecialistOutput, SpecialistPersona,
};
use op_plugins::state_plugins::tched_router::{ChatInput, TchedChatMessage};
use op_state_store::PluginSchema;
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

struct SpecialistGenerationAdmission {
    engine: MutationEngine,
    schema: PluginSchema,
    actor: String,
    session: String,
    genesis: String,
    authority_plugin: &'static str,
    authority_method: String,
    capability: String,
    facade_origin: Option<crate::tched_router_admission::FacadeOrigin>,
    sealed_identity: String,
}

impl crate::tched_router_generation_native::NativeGenerationAdmission
    for SpecialistGenerationAdmission
{
    fn sealed_identity(&self) -> Option<&str> {
        Some(&self.sealed_identity)
    }

    fn check(
        &self,
    ) -> Pin<
        Box<
            dyn Future<
                    Output = Result<
                        (),
                        crate::tched_router_generation_native::NativeGenerationError,
                    >,
                > + Send
                + '_,
        >,
    > {
        Box::pin(async move {
            let caller = crate::tched_router_admission::RouterCaller {
                actor: self.actor.clone(),
                capability: Some(self.capability.clone()),
                session: Some(self.session.clone()),
                genesis: Some(self.genesis.clone()),
            };
            let check = async {
                crate::tched_router_admission::readmit_call(
                    &self.engine,
                    &self.schema,
                    self.authority_plugin,
                    &self.authority_method,
                    &caller,
                )
                .await?;
                if let Some(origin) = &self.facade_origin {
                    origin.readmit(&self.engine, &caller).await?;
                }
                anyhow::Ok(())
            };
            check.await.map_err(|_| {
                crate::tched_router_generation_native::NativeGenerationError::Terminal(
                    crate::tched_router_generation_native::NativeErrorCode::Auth,
                )
            })
        })
    }
}

pub(crate) fn generation_admission(
    engine: &MutationEngine,
    schema: &PluginSchema,
    caller: CognitiveCaller<'_>,
    authority_plugin: &'static str,
    authority_method: &str,
) -> anyhow::Result<Arc<dyn crate::tched_router_generation_native::NativeGenerationAdmission>> {
    generation_admission_with_origin(
        engine,
        schema,
        caller,
        authority_plugin,
        authority_method,
        None,
    )
}

pub(crate) fn generation_admission_with_origin(
    engine: &MutationEngine,
    schema: &PluginSchema,
    caller: CognitiveCaller<'_>,
    authority_plugin: &'static str,
    authority_method: &str,
    facade_origin: Option<crate::tched_router_admission::FacadeOrigin>,
) -> anyhow::Result<Arc<dyn crate::tched_router_generation_native::NativeGenerationAdmission>> {
    let capability = schema
        .methods
        .get(authority_method)
        .and_then(|method| method.required_capability.as_deref())
        .context("generation authority method has no exact capability")?;
    let session = caller.session_id.context("verified session is required")?;
    let genesis = caller
        .session_genesis
        .context("verified genesis is required")?;
    let sealed_identity =
        crate::native_sled_identity::credential_for(caller.actor_id, session, genesis)?;
    Ok(Arc::new(SpecialistGenerationAdmission {
        engine: engine.clone(),
        schema: schema.clone(),
        actor: caller.actor_id.to_owned(),
        session: session.to_owned(),
        genesis: genesis.to_owned(),
        authority_plugin,
        authority_method: authority_method.to_owned(),
        capability: capability.to_owned(),
        facade_origin,
        sealed_identity,
    }))
}

/// Shared pool: seeded from the static catalog once, refreshed with memory
/// pieces on every call (they are the freshest shared knowledge).
static LIBRARY: std::sync::LazyLock<Arc<RwLock<PieceLibrary>>> =
    std::sync::LazyLock::new(|| Arc::new(RwLock::new(PieceLibrary::seeded_from_catalog())));

pub async fn consult(
    engine: &MutationEngine,
    caller: CognitiveCaller<'_>,
    args: &serde_json::Value,
    admitted_schema: &PluginSchema,
    authority_method: &str,
) -> anyhow::Result<serde_json::Value> {
    let input: SpecialistInput =
        serde_json::from_value(args.clone()).context("specialist: invalid input")?;
    consult_input(
        engine,
        caller,
        input,
        None,
        None,
        admitted_schema,
        authority_method,
    )
    .await
}

/// Consult one catalog persona through the authenticated bridge and the model
/// selected by the chat UI. The agent contributes its role and knowledge to
/// the shared dynamic persona; it never owns a provider client or credentials.
// Preserve the existing caller contract for authenticated specialist routing.
#[allow(clippy::too_many_arguments)]
pub async fn consult_catalog_agent(
    engine: &MutationEngine,
    caller: CognitiveCaller<'_>,
    agent_type: &str,
    operation: &str,
    question: &str,
    path: Option<&str>,
    context: Option<&str>,
    admitted_schema: &PluginSchema,
    authority_method: &str,
) -> anyhow::Result<serde_json::Value> {
    anyhow::ensure!(
        !caller.actor_id.trim().is_empty()
            && caller
                .session_id
                .is_some_and(|value| !value.trim().is_empty())
            && caller
                .session_genesis
                .is_some_and(|value| !value.trim().is_empty()),
        "specialist: verified session and genesis are required"
    );
    let agent = op_agents::unified::GLOBAL_REGISTRY
        .get_normalized(agent_type)
        .ok_or_else(|| anyhow::anyhow!("specialist: unknown catalog agent '{agent_type}'"))?;
    anyhow::ensure!(
        agent.category() == op_agents::AgentCategory::Persona,
        "specialist: catalog agent '{agent_type}' is not a Persona agent"
    );
    anyhow::ensure!(
        agent.operations().contains(&operation),
        "specialist: Persona agent '{agent_type}' does not support operation '{operation}'"
    );
    let advisor_operation: op_plugins::state_plugins::cognitive_mcp::AdvisorOperation =
        serde_json::from_value(serde_json::Value::String(operation.to_string()))
            .with_context(|| format!("specialist: invalid advisor operation '{operation}'"))?;
    let question = question.trim();
    anyhow::ensure!(
        !question.is_empty(),
        "specialist: 'question' must not be empty"
    );

    consult_input(
        engine,
        caller,
        SpecialistInput {
            operation: advisor_operation,
            question: question.to_owned(),
            path: path.map(str::to_owned),
            domain_hints: vec![agent.id().to_owned(), agent.name().to_owned()],
        },
        Some((agent_type, agent.as_ref())),
        context,
        admitted_schema,
        authority_method,
    )
    .await
}

async fn consult_input(
    engine: &MutationEngine,
    caller: CognitiveCaller<'_>,
    input: SpecialistInput,
    catalog_agent: Option<(&str, &dyn op_agents::unified::UnifiedAgent)>,
    caller_context: Option<&str>,
    admitted_schema: &PluginSchema,
    authority_method: &str,
) -> anyhow::Result<serde_json::Value> {
    anyhow::ensure!(
        !caller.actor_id.trim().is_empty()
            && caller
                .session_id
                .is_some_and(|value| !value.trim().is_empty())
            && caller
                .session_genesis
                .is_some_and(|value| !value.trim().is_empty()),
        "specialist: verified session and genesis are required"
    );
    let question = input.question.trim();
    anyhow::ensure!(
        !question.is_empty(),
        "specialist: 'question' must not be empty"
    );

    let capability = admitted_schema
        .methods
        .get(authority_method)
        .and_then(|method| method.required_capability.as_deref())
        .context("specialist authority method has no exact capability")?;
    let _selection_guard =
        crate::tched_router_runtime::TchedRouterRuntimeClient::selection_guard().await;
    crate::tched_router_admission::readmit_call(
        engine,
        admitted_schema,
        "cognitive_mcp",
        authority_method,
        &crate::tched_router_admission::RouterCaller {
            actor: caller.actor_id.to_owned(),
            capability: Some(capability.to_owned()),
            session: caller.session_id.map(str::to_owned),
            genesis: caller.session_genesis.map(str::to_owned),
        },
    )
    .await?;
    // Resolve mode only after the guard/admission boundary, then retain that
    // exact context for memory and routing so a mode switch cannot split one
    // consultation across two privacy policies.
    let mut route = crate::chat_mode_routing::resolve(
        engine,
        caller,
        ChatInput {
            conversation_id: caller.session_id.map(str::to_owned),
            ..ChatInput::default()
        },
    )
    .await?;
    let runtime = engine.cognitive_mcp().await?;
    let scope = caller.scope().to_string();
    // The memory the specialist may draw on is the caller's selected mode.
    let context = route.context.clone();
    let namespaces = vec![context.memory_namespace.clone()];
    let has_memory = context.mode.has_memory();

    // 1. Task context from what the session already knows.
    let mut task = TaskContext {
        question: question.to_string(),
        path: input.path.clone(),
        domain_hints: input.domain_hints.clone(),
        activity: if has_memory {
            activity_lines(runtime, &scope).await
        } else {
            Vec::new()
        },
        snippets: Vec::new(),
        executors: EXECUTORS.iter().map(|s| s.to_string()).collect(),
    };
    if has_memory {
        task.snippets = memory_snippets(runtime, &namespaces, &task).await;
        task.snippets
            .extend(engine.take_pending_knowledge(&scope).await);
    }
    if let Some(context) = caller_context.map(str::trim).filter(|s| !s.is_empty()) {
        task.snippets.push(format!(
            "[verified caller context] {}",
            truncate(context, 8_000)
        ));
    }

    // 2. Refresh shared pieces from memory, then compose the persona.
    if has_memory {
        refresh_memory_pieces(runtime).await;
    }
    let persona: AssembledPersona = {
        if has_memory {
            let library = LIBRARY.read().await;
            assemble(&library, &task, PERSONA_BUDGET_CHARS)
        } else {
            // UI rendering must not reuse memory-refreshed pieces from a
            // prior private consultation in this process.
            let library = PieceLibrary::seeded_from_catalog();
            assemble(&library, &task, PERSONA_BUDGET_CHARS)
        }
    };

    // 3. Record the consultation as session activity before answering, so a
    //    failed completion still leaves the signal.
    if has_memory {
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
                    "memory_namespace": context.memory_namespace,
                }),
            )
            .await;
    }

    // 4. Complete through the guarded native router path.
    let mut system_prompt = persona.system_prompt.clone();
    let persona_name = if let Some((agent_type, agent)) = catalog_agent {
        system_prompt.push_str("\n\n## Requested catalog persona\n");
        system_prompt.push_str("Catalog id: ");
        system_prompt.push_str(agent_type);
        system_prompt.push_str("\nRole: ");
        system_prompt.push_str(agent.system_prompt().trim());
        if let Some(knowledge) = agent.knowledge_base() {
            system_prompt.push_str("\nKnowledge:\n");
            system_prompt.push_str(&truncate(knowledge, 8_000));
        }
        format!("{} ({})", agent.name(), persona.name)
    } else {
        persona.name.clone()
    };
    route.input.messages = vec![
        TchedChatMessage {
            role: "system".into(),
            content: system_prompt,
        },
        TchedChatMessage {
            role: "user".into(),
            content: format!("[{}] {question}", operation_name(&input)),
        },
    ];
    let before_prompt = generation_admission(
        engine,
        admitted_schema,
        caller,
        "cognitive_mcp",
        authority_method,
    )?;
    let output = if route.mode == op_plugins::state_plugins::project::ChatMode::ControlPlane {
        crate::control_plane_routing::complete(route.input, before_prompt, true).await
    } else {
        route
            .client
            .complete_under_guard(route.input, before_prompt)
            .await
    }
    .context("specialist: provider-only native completion failed")?;
    anyhow::ensure!(
        !output.content.trim().is_empty(),
        "specialist: native completion returned an empty reply"
    );

    let mut sources = persona
        .sources
        .iter()
        .map(|s| serde_json::to_string(s).unwrap_or_default())
        .collect::<Vec<_>>();
    if let Some((agent_type, _)) = catalog_agent {
        sources.push(format!("catalog_agent:{agent_type}"));
    }
    if let Some(routing) = output.usage.get("routing") {
        sources.push(format!("tched_router.routing:{routing}"));
    }
    let result = SpecialistOutput {
        answer: output.content,
        persona: SpecialistPersona {
            name: persona_name,
            domains: persona.domains,
            piece_ids: persona.piece_ids,
            sources,
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

/// Memory entries the caller may see, matched to the task: the bound
/// namespace(s) of the selected mode and the shared pieces namespace,
/// filtered by task terms.
pub(crate) async fn memory_snippets(
    runtime: &CognitiveMcpServer,
    namespaces: &[String],
    task: &TaskContext,
) -> Vec<String> {
    let store = runtime.memory_store();
    let terms = task.terms();
    let mut snippets = Vec::new();
    let pieces = PIECES_NAMESPACE.to_string();
    for namespace_name in namespaces.iter().chain(std::iter::once(&pieces)) {
        let namespace_name = namespace_name.as_str();
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
