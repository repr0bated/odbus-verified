//! Native cognitive, private memory/session metadata, and NotebookLM adapters.
//!
//! The historical tool-registry crate is an internal implementation library,
//! never a transport. Every delegated operation returns through Runtime admission.
use std::{
    collections::{BTreeSet, HashMap},
    os::unix::fs::DirBuilderExt,
    path::PathBuf,
    sync::Arc,
    time::Duration,
};

use anyhow::{Context, anyhow, bail, ensure};
use op_agents::dynamic::{TaskContext, ContextBudget, assemble};
use op_cognitive::{
    CognitiveServer,
    cognitive_tools::MemoryTool,
    memory_store::{CognitiveMemoryStore, EntryQuery},
};
use op_mcp::tool_registry::Tool as _;
use op_cozo_store::CozoGraphShuttle;
use op_plugins::state_plugins::{
    cognitive::{SpecialistInput, SpecialistOutput, SpecialistPersona},
    cognitive_agents::{self, CatalogAgentMethod},
    memory_methods::{MemoryAccessContext, MemoryMethods},
    notebooklm::{NotebookLmPlugin, NotebookQueryInput, QueryStatusInput},
    project::{
        ChatMode, ResolvedChatContext, namespace_prefix_for_session, validate_personal_namespace,
    },
    runtime_documents::{RuntimeDocumentRejected, RuntimeDocumentUncertain, RuntimeDocuments},
    sessdecl::{SessDeclAccessContext, SessDeclMethods},
};
use op_state_store::SideEffect;
use serde_json::{Value, json};
use tokio::sync::OnceCell;

use crate::{
    core::{BackendStateUpdate, StateScope},
    runtime::{BackendContext, BackendOutcome, RuntimeError},
};

const MEMORY_METHODS: &[&str] = &[
    "create_namespace",
    "delete_namespace",
    "query_namespace",
    "insert_entry",
    "update_entry",
    "get_entry",
    "delete_entry",
    "clear_namespace",
    "get_stats",
    "compact",
];
const COGNITIVE_METHODS: &[&str] = &[
    "get_health",
    "list_tools",
    "memory_store",
    "memory_recall",
    "memory_retrieve",
    "memory_query",
    "memory_delete",
    "memory_list_namespaces",
    "memory_semantic_query",
    "code_search",
    "code_index",
    "code_context",
    "rust_pro",
    "python_pro",
    "typescript_pro",
    "sequential_thinking",
    "context_manager",
    "code_review_orchestrator",
    "specialist",
    "workflow_query",
    "workflow_run",
    "schema_read",
    "oscal_subids",
    "gemini_query",
    "invoke_tool",
];
const NOTEBOOK_METHODS: &[&str] = &[
    "notebook_list",
    "notebook_create",
    "notebook_get",
    "notebook_describe",
    "notebook_rename",
    "notebook_delete",
    "select_notebook",
    "notebook_query",
    "notebook_query_start",
    "notebook_query_status",
    "source_add",
    "source_list_drive",
    "source_sync_drive",
    "source_delete",
    "source_describe",
    "source_get_content",
    "source_rename",
    "chat_configure",
    "chat_list",
    "chat_get",
    "chat_export",
    "studio_create",
    "studio_status",
    "studio_delete",
    "studio_revise",
    "download_artifact",
    "download_all_artifacts",
    "export_artifact",
    "research_start",
    "research_status",
    "research_import",
    "note",
    "label",
    "notebook_share_status",
    "notebook_share_public",
    "notebook_share_invite",
    "notebook_share_batch",
    "refresh_auth",
    "save_auth_tokens",
    "server_info",
    "batch",
    "cross_notebook_query",
    "pipeline",
    "tag",
    "get_health",
    "setup_auth",
    "reauth",
];

pub(crate) fn supports(plugin: &str, method: &str) -> bool {
    match plugin {
        "memory" => MEMORY_METHODS.contains(&method),
        "sess_decl" => matches!(method, "declare_session" | "list_sessions"),
        "notebooklm" => NOTEBOOK_METHODS.contains(&method),
        "cognitive" => {
            COGNITIVE_METHODS.contains(&method)
                || cognitive_agents::method_by_name(method).is_some()
        }
        _ => false,
    }
}

pub(crate) struct CognitiveBackend {
    memory: MemoryMethods,
    sessions: SessDeclMethods,
    cognitive: OnceCell<Arc<CognitiveServer>>,
    identity_store: Arc<CozoGraphShuttle>,
    queries: crate::notebooklm_queries::NotebookQueries,
    /// Personal memory per identity session, in that identity's own pool
    /// volume (`identity-<session>/memory/cozo`, cognitive-memory-rocksdb).
    personal: tokio::sync::Mutex<HashMap<String, Arc<PersonalMemory>>>,
}

/// One identity's personal memory store. Never paired with Qdrant: personal
/// memory reaches no shared store or vector index (owner rule 2026-11-22).
struct PersonalMemory {
    shuttle: Arc<CozoGraphShuttle>,
    store: Arc<CognitiveMemoryStore>,
    tool: MemoryTool,
}

/// `<pool>/custom/default_identity-<session>/memory/cozo`: the existing
/// identity volume (attached to the container read-only at /agent/user and
/// written only by the host bridge), same contract as the sled ledger.
fn personal_store_path(session: &str) -> anyhow::Result<PathBuf> {
    let parsed = uuid::Uuid::parse_str(session).context("session id is not a UUID")?;
    ensure!(parsed.to_string() == session, "session id is not canonical");
    let pool = std::env::var("OP_CONTAINER_POOL").unwrap_or_else(|_| "3tched-pool".into());
    let volume = PathBuf::from(format!(
        "/var/lib/incus/storage-pools/{pool}/custom/default_identity-{session}"
    ));
    let stored = std::fs::read_to_string(volume.join("session_id")).with_context(|| {
        format!("identity volume identity-{session} is absent; personal memory has no store")
    })?;
    ensure!(
        stored.trim() == session,
        "identity volume session_id differs from its name"
    );
    let memory = volume.join("memory");
    std::fs::DirBuilder::new()
        .mode(0o700)
        .recursive(true)
        .create(&memory)?;
    // Kept apart from the sled: a real directory, never a link into ledger/.
    ensure!(
        std::fs::symlink_metadata(&memory)?.file_type().is_dir(),
        "personal memory path is not a plain directory"
    );
    Ok(memory.join("cozo"))
}

fn memory_operation(method: &str) -> &'static str {
    match method {
        "memory_store" => "store",
        "memory_delete" => "delete",
        "memory_retrieve" => "retrieve",
        "memory_recall" | "memory_query" => "query",
        "memory_semantic_query" => "semantic_query",
        "memory_list_namespaces" => "list_namespaces",
        _ => unreachable!(),
    }
}

impl CognitiveBackend {
    /// Retain the already-open identity owner separately from plugin documents.
    /// Notebook selections use that owner; memory/session metadata use documents.
    pub(crate) fn new(
        identity_store: Arc<CozoGraphShuttle>,
        documents: Arc<RuntimeDocuments>,
    ) -> Self {
        Self {
            memory: MemoryMethods::from_documents(documents.clone()),
            sessions: SessDeclMethods::new(documents),
            cognitive: OnceCell::new(),
            identity_store,
            queries: Default::default(),
            personal: Default::default(),
        }
    }

    pub(crate) async fn cognitive(&self) -> Result<&Arc<CognitiveServer>, RuntimeError> {
        self.cognitive
            .get_or_try_init(|| async {
                let path = std::env::var("COGNITIVE_DB_PATH")
                    .unwrap_or_else(|_| "/var/lib/op-dbus/cognitive-memory.db".into());
                let server = tokio::time::timeout(
                    Duration::from_secs(60),
                    CognitiveServer::new_durable(&path),
                )
                .await
                .map_err(|_| {
                    RuntimeError::backend(anyhow!("durable cognitive initialization timed out"))
                })?
                .map_err(|error| {
                    RuntimeError::backend(anyhow!("durable cognitive runtime unavailable: {error}"))
                })?;
                self.identity_store
                    .ensure_distinct_from(&server.cozo_shuttle())?;
                for personal in self.personal.lock().await.values() {
                    personal.shuttle.ensure_distinct_from(&server.cozo_shuttle())?;
                }
                Ok(Arc::new(server))
            })
            .await
    }

    pub(crate) async fn dispatch(
        &self,
        ctx: &BackendContext<'_>,
        plugin: &str,
        method: &str,
        args: &Value,
    ) -> Result<BackendOutcome, RuntimeError> {
        if !supports(plugin, method) {
            return Err(RuntimeError::unsupported(format!(
                "no native adapter for {plugin}.{method}"
            )));
        }
        revalidate(ctx).await?;
        match plugin {
            "memory" => {
                let context = MemoryAccessContext {
                    principal_id: ctx.caller.principal_id.clone(),
                    session_id: ctx.caller.session_id.clone(),
                };
                let result =
                    bounded_document(ctx, self.memory.dispatch(&context, method, args)).await?;
                // MemoryState declares backend/isolation only, not entry contents.
                // Durable methods already return the exact CAS-committed domain result.
                Ok(private_result(ctx, result, BackendStateUpdate::Unchanged))
            }
            "sess_decl" => {
                let context = SessDeclAccessContext {
                    principal_id: ctx.caller.principal_id.clone(),
                    session_id: ctx.caller.session_id.clone(),
                };
                let result =
                    bounded_document(ctx, self.sessions.dispatch(&context, method, args)).await?;
                let observed = bounded_document(
                    ctx,
                    self.sessions
                        .dispatch(&context, "list_sessions", &json!({})),
                )
                .await?;
                let sessions = observed
                    .get("sessions")
                    .cloned()
                    .context("session readback omitted sessions")?;
                Ok(private_result(
                    ctx,
                    result,
                    BackendStateUpdate::Replace(json!({"sessions": sessions})),
                ))
            }
            "notebooklm" => self.notebook(ctx, method, args).await,
            "cognitive" => self.dispatch_cognitive(ctx, method, args).await,
            _ => unreachable!("supported plugin was checked"),
        }
    }

    async fn dispatch_cognitive(
        &self,
        ctx: &BackendContext<'_>,
        method: &str,
        args: &Value,
    ) -> Result<BackendOutcome, RuntimeError> {
        if method == "invoke_tool" {
            let requested = required_str(args, "tool_name")?;
            let mut arguments = args.get("arguments").cloned().unwrap_or_else(|| json!({}));
            let target = generic_target(requested, &mut arguments)?;
            if !ctx.schema.methods.contains_key(target) || !supports("cognitive", target) {
                return Err(RuntimeError::denied(
                    "tool has no sealed cognitive method authority",
                ));
            }
            let result = nested(ctx, "cognitive", target, arguments).await?;
            return Ok(private_result(
                ctx,
                json!({"success":true,"tool_name":requested,"result":result}),
                BackendStateUpdate::Unchanged,
            ));
        }
        if let Some((target, arguments)) = hot_target(method, args)? {
            let result = nested(ctx, "cognitive", &target.declaration.name, arguments).await?;
            return Ok(private_result(ctx, result, BackendStateUpdate::Unchanged));
        }
        if let Some(agent) = cognitive_agents::method_by_name(method) {
            let context = selected_context(ctx).await?;
            admit_agent(ctx, &context, agent).await?;
            let result = match agent.category.as_str() {
                "persona" => self.specialist(ctx, args, Some(agent), context).await?,
                "execution" => execute_agent(ctx, agent, args).await?,
                "orchestration" => workflow(ctx, agent, args).await?,
                _ => return Err(RuntimeError::unsupported("unknown native agent category")),
            };
            return Ok(private_result(ctx, result, BackendStateUpdate::Unchanged));
        }
        match method {
            "workflow_query" | "workflow_run" => {
                let target = if method == "workflow_query" {
                    "query_workflows"
                } else {
                    "start_workflow"
                };
                let result = nested(ctx, "workflows", target, args.clone()).await?;
                return Ok(private_result(ctx, result, BackendStateUpdate::Unchanged));
            }
            "schema_read" | "oscal_subids" => {
                let dir = ctx.runtime.catalog_dir().to_path_buf();
                let args = args.clone();
                let schema_read = method == "schema_read";
                let result = tokio::task::spawn_blocking(move || {
                    if schema_read {
                        crate::sealed_schema_reader::read_sealed_schema_result(
                            &dir,
                            required_str(&args, "plugin_id")?,
                        )
                    } else {
                        crate::sealed_schema_reader::read_oscal_subids_result(
                            &dir,
                            args.get("plugin_id").and_then(Value::as_str),
                            args.get("prefix").and_then(Value::as_str),
                        )
                    }
                })
                .await
                .map_err(|error| RuntimeError::backend(error.into()))??;
                return Ok(private_result(ctx, result, BackendStateUpdate::Unchanged));
            }
            "specialist" => {
                let context = selected_context(ctx).await?;
                let result = self.specialist(ctx, args, None, context).await?;
                return Ok(private_result(ctx, result, BackendStateUpdate::Unchanged));
            }
            _ => {}
        }
        // Personal-mode memory goes to the identity's own volume store and
        // never opens the shared cognitive store.
        let memory_context = if is_memory_method(method) {
            let context = selected_context(ctx).await?;
            if matches!(context.mode, ChatMode::Personal) {
                return self.personal_memory_call(ctx, method, args, &context).await;
            }
            Some(context)
        } else {
            None
        };
        let runtime = self.cognitive().await?;
        revalidate(ctx).await?;
        if method == "get_health" {
            let (remaining, limit) = runtime.quota_manager().status().await;
            let state = json!({"execution_model":"in_process",
                "queries_remaining":remaining,"queries_limit":limit});
            let result = json!({"bridge_owned":true,"execution_model":"in-process","auth_status":"bridge",
                "queries_remaining":remaining,"queries_limit":limit});
            return Ok(private_result(
                ctx,
                result,
                BackendStateUpdate::Merge(state),
            ));
        }
        if method == "list_tools" {
            let grants = ctx.runtime.current_grants(ctx.caller).await?;
            let mut tools = Vec::new();
            for (name, declaration) in &ctx.schema.methods {
                if supports("cognitive", name) && name != "invoke_tool" {
                    if let Some(cap) = &declaration.required_capability {
                        if grants.contains(cap) {
                            tools.push(name.clone());
                        }
                    }
                }
            }
            tools.sort();
            return Ok(private_result(
                ctx,
                json!({"tools":tools}),
                BackendStateUpdate::Unchanged,
            ));
        }
        let mut bound = args.clone();
        let context = if let Some(context) = memory_context {
            Some(context)
        } else if matches!(method, "code_context" | "code_index") {
            Some(selected_context(ctx).await?)
        } else {
            None
        };
        if let Some(context) = &context {
            if is_memory_method(method) {
                bind_memory(method, context, &mut bound)
                    .map_err(|error| RuntimeError::denied(error.to_string()))?;
            }
            if method == "code_index" && context.read_only {
                return Err(RuntimeError::denied(
                    "selected chat mode does not permit indexing",
                ));
            }
            if method == "code_context" {
                let object = bound
                    .as_object_mut()
                    .context("code_context expects an object")?;
                object.insert("session_id".into(), ctx.caller.session_id.clone().into());
                object.insert(
                    "memory_namespace".into(),
                    if context.mode.has_memory() {
                        context.memory_namespace.clone().into()
                    } else {
                        Value::Null
                    },
                );
            }
        }
        let tool = if is_memory_method(method) {
            let operation = memory_operation(method);
            bound
                .as_object_mut()
                .context("memory arguments must be an object")?
                .insert("operation".into(), operation.into());
            "cognitive_memory"
        } else {
            method
        };
        let registry = runtime.tool_registry();
        if registry.get_definition(tool).await.is_none() {
            return Err(RuntimeError::backend(anyhow!(
                "native {tool} backend is unavailable; configure its retained provider"
            )));
        }
        revalidate(ctx).await?;
        let input = simd_json::serde::to_owned_value(&bound)?;
        let result = tokio::time::timeout(Duration::from_secs(120), registry.execute(tool, input))
            .await
            .map_err(|_| {
                RuntimeError::unknown(anyhow!(
                    "native cognitive operation timed out; do not retry effects"
                ))
            })?
            .map_err(|error| after_effect(ctx, error))?;
        let mut result = serde_json::to_value(result)?;
        if method == "memory_list_namespaces" {
            let context = context.as_ref().context("memory context is absent")?;
            filter_namespaces(context, &mut result);
        }
        if matches!(method, "memory_store" | "memory_delete") {
            let store = runtime.cozo_shuttle();
            tokio::task::spawn_blocking(move || store.sync_runtime_document_store())
                .await
                .map_err(|error| RuntimeError::unknown(error.into()))?
                .map_err(|error| RuntimeError::unknown(error.into()))?;
            let namespace = required_str(&bound, "namespace")?;
            let key = required_str(&bound, "key")?;
            let entry = runtime
                .memory_store()
                .retrieve_entry(namespace, key)
                .await
                .map_err(RuntimeError::unknown)?;
            let matches = if method == "memory_store" {
                entry.as_ref().is_some_and(|entry| {
                    Some(entry.id.as_str()) == result.get("id").and_then(Value::as_str)
                        && Some(&entry.value) == bound.get("value")
                })
            } else {
                entry.is_none()
            };
            if !matches {
                return Err(RuntimeError::unknown(anyhow!(
                    "durable cognitive memory readback differs from native result"
                )));
            }
        }
        // The sealed cognitive root exposes runtime health, not private memory
        // entries or code corpus contents. Their authority remains Cozo/Qdrant.
        Ok(private_result(ctx, result, BackendStateUpdate::Unchanged))
    }

    async fn personal_memory(&self, session: &str) -> Result<Arc<PersonalMemory>, RuntimeError> {
        let mut open = self.personal.lock().await;
        if let Some(memory) = open.get(session) {
            return Ok(memory.clone());
        }
        let path = personal_store_path(session).map_err(RuntimeError::before_effect)?;
        let shuttle = tokio::task::spawn_blocking(move || {
            CozoGraphShuttle::new_persistent_private(path)
        })
        .await
        .map_err(|error| RuntimeError::backend(error.into()))?
        .map_err(|error| {
            RuntimeError::backend(anyhow!("personal memory store unavailable: {error}"))
        })?;
        let shuttle = Arc::new(shuttle);
        self.identity_store.ensure_distinct_from(&shuttle)?;
        if let Some(shared) = self.cognitive.get() {
            shared.cozo_shuttle().ensure_distinct_from(&shuttle)?;
        }
        for other in open.values() {
            other.shuttle.ensure_distinct_from(&shuttle)?;
        }
        let store = Arc::new(
            CognitiveMemoryStore::new(shuttle.clone())
                .await
                .map_err(RuntimeError::backend)?,
        );
        let memory = Arc::new(PersonalMemory {
            tool: MemoryTool::new(store.clone(), None),
            store,
            shuttle,
        });
        open.insert(session.to_owned(), memory.clone());
        Ok(memory)
    }

    async fn personal_memory_call(
        &self,
        ctx: &BackendContext<'_>,
        method: &str,
        args: &Value,
        context: &ResolvedChatContext,
    ) -> Result<BackendOutcome, RuntimeError> {
        if method == "memory_semantic_query" {
            return Err(RuntimeError::unsupported(
                "personal memory has no vector index; use memory_query",
            ));
        }
        let mut bound = args.clone();
        bind_memory(method, context, &mut bound)
            .map_err(|error| RuntimeError::denied(error.to_string()))?;
        bound
            .as_object_mut()
            .context("memory arguments must be an object")?
            .insert("operation".into(), memory_operation(method).into());
        let memory = self.personal_memory(&context.session_id).await?;
        revalidate(ctx).await?;
        let input = simd_json::serde::to_owned_value(&bound)?;
        let output = tokio::time::timeout(Duration::from_secs(120), memory.tool.execute(input))
            .await
            .map_err(|_| {
                RuntimeError::unknown(anyhow!(
                    "personal memory operation timed out; do not retry effects"
                ))
            })?
            .map_err(|error| after_effect(ctx, error))?;
        let mut result = serde_json::to_value(output)?;
        if method == "memory_list_namespaces" {
            filter_namespaces(context, &mut result);
        }
        if matches!(method, "memory_store" | "memory_delete") {
            let shuttle = memory.shuttle.clone();
            tokio::task::spawn_blocking(move || shuttle.sync_runtime_document_store())
                .await
                .map_err(|error| RuntimeError::unknown(error.into()))?
                .map_err(|error| RuntimeError::unknown(error.into()))?;
            let namespace = required_str(&bound, "namespace")?;
            let key = required_str(&bound, "key")?;
            let entry = memory
                .store
                .retrieve_entry(namespace, key)
                .await
                .map_err(RuntimeError::unknown)?;
            let matches = if method == "memory_store" {
                entry.as_ref().is_some_and(|entry| {
                    Some(entry.id.as_str()) == result.get("id").and_then(Value::as_str)
                        && Some(&entry.value) == bound.get("value")
                })
            } else {
                entry.is_none()
            };
            if !matches {
                return Err(RuntimeError::unknown(anyhow!(
                    "personal memory readback differs from native result"
                )));
            }
        }
        Ok(private_result(ctx, result, BackendStateUpdate::Unchanged))
    }

    async fn specialist(
        &self,
        ctx: &BackendContext<'_>,
        args: &Value,
        catalog: Option<&CatalogAgentMethod>,
        context: ResolvedChatContext,
    ) -> Result<Value, RuntimeError> {
        let input = if let Some(catalog) = catalog {
            SpecialistInput {
                operation: serde_json::from_value(catalog.operation.clone().into())?,
                question: required_str(args, "question")?.into(),
                path: args.get("path").and_then(Value::as_str).map(str::to_owned),
                domain_hints: vec![catalog.agent_type.clone()],
            }
        } else {
            serde_json::from_value(args.clone())?
        };
        if input.question.trim().is_empty() {
            return Err(RuntimeError::backend(anyhow!(
                "specialist question is empty"
            )));
        }
        let operation = serde_json::to_value(&input.operation)?;
        let mut task = TaskContext {
            question: input.question.clone(),
            path: input.path,
            domain_hints: input.domain_hints,
            activity: Vec::new(),
            snippets: Vec::new(),
            executors: Vec::new(),
        };
        // Immutable shared seed content; private memory is only scoped task data.
        let library = &*op_agents::recipes::CATALOG_PIECES;
        if context.mode.has_memory() {
            let runtime = self.cognitive().await?;
            let store = runtime.memory_store();
            let terms = task.terms();
            for namespace in [context.memory_namespace.as_str(), "agent_pieces"] {
                let entries = store
                    .query_entries(EntryQuery {
                        namespace_id: Some(namespace.into()),
                        limit: Some(200),
                        ..Default::default()
                    })
                    .await?;
                for entry in entries {
                    if task.snippets.len() < 12 {
                        let text = match &entry.value {
                            Value::String(text) => text.clone(),
                            value => value.to_string(),
                        };
                        let searchable = format!("{} {} {text}", entry.key, entry.tags.join(" "))
                            .to_ascii_lowercase();
                        if terms.iter().any(|term| searchable.contains(term.as_str())) {
                            task.snippets.push(format!(
                                "[{namespace}/{}] {}",
                                entry.key,
                                text.chars().take(400).collect::<String>()
                            ));
                        }
                    }
                }
            }
            if let Some(signals) = runtime
                .context_engine()
                .get_session_signals(&ctx.caller.session_id)
                .await
            {
                task.activity.push(serde_json::to_string(&signals)?);
            }
        }
        if let Some(context) = args.get("context").and_then(Value::as_str) {
            task.snippets.push(context.chars().take(8000).collect());
        }
        let recipe_id = catalog.map(|binding| binding.agent_type.as_str())
            .or_else(|| task.domain_hints.iter().find(|id| op_agents::unified::GLOBAL_REGISTRY.get(id).is_some()).map(String::as_str))
            .context("specialist requires an exact recipe pin; semantic selection belongs to workstacks")?;
        let recipes = op_agents::recipes::NATIVE_RECIPES.as_ref().map_err(|error| anyhow!("{error}"))?;
        let recipe = recipes.get(recipe_id).context("recipe is absent from native registry")?;
        op_agents::recipes::validate_recipe_pieces(recipe, &library, &BTreeSet::from(["catalog_visible".into()]))?;
        let pins = recipe.piece_refs.iter().map(|piece| piece.piece_id.clone()).collect::<Vec<_>>();
        let persona = assemble(&library, &task, ContextBudget {
            total_bytes: 12_000, reserved_output_bytes: 2_000, reserved_tool_bytes: 0, framing_bytes: 512,
        }, &pins)?;
        let prompt = persona.system_prompt;
        let name = recipe.name.clone();
        let sources = persona.sources.iter().map(serde_json::to_string).collect::<Result<Vec<_>, _>>()?;
        let request = op_plugins::state_plugins::chat_router::ChatInput {
            messages: vec![
                op_plugins::state_plugins::chat_router::ChatMessage {
                    role: "system".into(),
                    content: prompt,
                },
                op_plugins::state_plugins::chat_router::ChatMessage {
                    role: "user".into(),
                    content: format!(
                        "[{}] {}",
                        operation.as_str().unwrap_or("consult"),
                        input.question
                    ),
                },
            ],
            conversation_id: Some(ctx.caller.session_id.clone()),
            ..Default::default()
        };
        revalidate(ctx).await?;
        let value = nested(ctx, "chat_router", "Chat", serde_json::to_value(request)?).await?;
        let output: op_plugins::state_plugins::chat_router::ChatOutput =
            serde_json::from_value(value)?;
        if output.content.trim().is_empty() {
            return Err(RuntimeError::unknown(anyhow!(
                "native specialist provider returned an empty answer"
            )));
        }
        Ok(serde_json::to_value(SpecialistOutput {
            answer: output.content,
            persona: SpecialistPersona {
                name,
                domains: persona.domains,
                piece_ids: persona.piece_ids,
                sources,
            },
            context_used: task.snippets.len() + task.activity.len(),
            provider: output.provider,
            model: output.model,
        })?)
    }

    async fn notebook(
        &self,
        ctx: &BackendContext<'_>,
        method: &str,
        args: &Value,
    ) -> Result<BackendOutcome, RuntimeError> {
        // Exact notebooklm.* grants authorize the configured shared provider
        // account. Selection and all returned state are private to this caller.
        if method == "notebook_query_status" {
            let input: QueryStatusInput = serde_json::from_value(args.clone())?;
            let result = self
                .queries
                .status(
                    &ctx.caller.principal_id,
                    &ctx.caller.session_id,
                    &input.query_id,
                )
                .await?;
            return Ok(private_result(
                ctx,
                serde_json::to_value(result)?,
                BackendStateUpdate::Unchanged,
            ));
        }
        let selected = selected_notebook(ctx).await?;
        if method == "notebook_query_start" {
            let input: NotebookQueryInput = serde_json::from_value(args.clone())?;
            let (notebook_id, timeout) =
                crate::notebooklm_queries::NotebookQueries::prepare(&input, selected.as_deref())?;
            let invocation = crate::nlm_cli::nlm_argv("notebook_query", args, Some(&notebook_id))?;
            revalidate(ctx).await?;
            let runtime = ctx.runtime.clone();
            let caller = ctx.caller.for_background();
            let capability = ctx
                .declaration
                .required_capability
                .clone()
                .context("query capability absent")?;
            let result = self
                .queries
                .start(
                    &ctx.caller.principal_id,
                    &ctx.caller.session_id,
                    notebook_id,
                    timeout,
                    async move {
                        runtime
                            .require_capability(&caller, &capability)
                            .await
                            .map_err(|error| anyhow!("{error}"))?;
                        let result = crate::nlm_cli::run_notebooklm_query(&invocation).await?;
                        runtime
                            .require_capability(&caller, &capability)
                            .await
                            .map_err(|error| anyhow!("{error}"))?;
                        Ok(result)
                    },
                )
                .await?;
            return Ok(private_result(
                ctx,
                serde_json::to_value(result)?,
                BackendStateUpdate::Unchanged,
            ));
        }
        let result = match crate::nlm_cli::notebooklm_dispatch(method, args, selected.as_deref())? {
            crate::nlm_cli::NotebooklmDispatch::LocalSelect { notebook_id } => {
                // Resolve the real provider notebook before committing selection.
                nested(
                    ctx,
                    "notebooklm",
                    "notebook_get",
                    json!({"notebook_id":notebook_id}),
                )
                .await?;
                revalidate(ctx).await?;
                let store = ctx.runtime.identity_store().clone();
                let session = ctx.caller.session_id.clone();
                let expected = notebook_id.clone();
                tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
                    store.run_query(
                        "?[session_id, notebook_id] <- [[$session, $notebook]] :put notebooklm_selection {session_id => notebook_id}",
                        Some(json!({"session":session,"notebook":notebook_id})),
                    )?;
                    store.sync_runtime_document_store()?;
                    Ok(())
                }).await.map_err(|error| RuntimeError::unknown(error.into()))?.map_err(RuntimeError::unknown)?;
                let readback = selected_notebook(ctx)
                    .await
                    .map_err(|error| {
                        RuntimeError::unknown(anyhow!(
                            "selection committed but readback failed: {error}"
                        ))
                    })?
                    .ok_or_else(|| {
                        RuntimeError::unknown(anyhow!("committed notebook selection is absent"))
                    })?;
                if readback != expected {
                    return Err(RuntimeError::unknown(anyhow!(
                        "durable notebook selection differs from the committed selection"
                    )));
                }
                json!({"selected_notebook_id":readback,"session_id":ctx.caller.session_id})
            }
            crate::nlm_cli::NotebooklmDispatch::Cli(invocation) => {
                revalidate(ctx).await?;
                crate::nlm_cli::run_notebooklm_method(method, &invocation)
                    .await
                    .map_err(|error| {
                        if notebook_has_effect(method, args) {
                            RuntimeError::unknown(error)
                        } else {
                            RuntimeError::backend(error)
                        }
                    })?
            }
        };
        let mut state = NotebookLmPlugin::current_state();
        state.selected_notebook_id = selected_notebook(ctx).await.map_err(|error| {
            RuntimeError::unknown(anyhow!(
                "NotebookLM readback failed after execution: {error}"
            ))
        })?;
        if let Some(status) = crate::nlm_cli::auth_status_from_value(&result) {
            state.status = if status == "configured" {
                "ready"
            } else {
                "authentication_required"
            }
            .into();
            state.auth.profile_configured = status == "configured";
            let action = crate::nlm_cli::ready_marker_action(&status);
            crate::nlm_cli::apply_ready_marker(crate::nlm_cli::NOTEBOOKLM_AUTH_READY, action)
                .map_err(|error| {
                    RuntimeError::unknown(anyhow!(
                        "NotebookLM auth ready marker update failed: {error}"
                    ))
                })?;
        }
        crate::nlm_cli::touch_bin_ready_marker(&crate::nlm_cli::nlm_bin()).map_err(|error| {
            RuntimeError::unknown(anyhow!(
                "NotebookLM binary ready marker update failed: {error}"
            ))
        })?;
        Ok(private_result(
            ctx,
            result,
            BackendStateUpdate::Replace(serde_json::to_value(state)?),
        ))
    }
}

fn private_result(
    ctx: &BackendContext<'_>,
    result: Value,
    state: BackendStateUpdate,
) -> BackendOutcome {
    BackendOutcome {
        result,
        state,
        scope: StateScope::Private {
            principal_id: ctx.caller.principal_id.clone(),
            session_id: ctx.caller.session_id.clone(),
        },
        additional: Vec::new(),
        guard: None,
    }
}

async fn bounded_document<F>(ctx: &BackendContext<'_>, operation: F) -> Result<Value, RuntimeError>
where
    F: Future<Output = anyhow::Result<Value>>,
{
    tokio::time::timeout(Duration::from_secs(60), operation)
        .await
        .map_err(|_| {
            after_effect(
                ctx,
                anyhow!("native document operation timed out; reconcile before another mutation"),
            )
        })?
        .map_err(|error| document_error(ctx, error))
}

async fn revalidate(ctx: &BackendContext<'_>) -> Result<(), RuntimeError> {
    let capability = ctx
        .declaration
        .required_capability
        .as_deref()
        .context("native operation lacks a sealed capability")?;
    ctx.runtime.require_capability(ctx.caller, capability).await
}

async fn nested(
    ctx: &BackendContext<'_>,
    plugin: &str,
    method: &str,
    args: Value,
) -> Result<Value, RuntimeError> {
    revalidate(ctx).await?;
    // Agent tool call: always through the execution tracker with a checkpoint.
    let mut caller = ctx.caller.clone();
    caller.require_execution_checkpoint();
    Ok(
        Box::pin(ctx.runtime.call_nested(&caller, plugin, method, args))
            .await?
            .result,
    )
}

async fn selected_context(ctx: &BackendContext<'_>) -> Result<ResolvedChatContext, RuntimeError> {
    let value = nested(ctx, "project", "get_context", json!({})).await?;
    let context: ResolvedChatContext = serde_json::from_value(value)?;
    if context.session_id != ctx.caller.session_id {
        return Err(RuntimeError::denied(
            "project context is not bound to the verified session",
        ));
    }
    Ok(context)
}

fn document_error(ctx: &BackendContext<'_>, error: anyhow::Error) -> RuntimeError {
    if error.downcast_ref::<RuntimeDocumentUncertain>().is_some() {
        RuntimeError::unknown(error)
    } else if error.downcast_ref::<RuntimeDocumentRejected>().is_some() {
        RuntimeError::backend(error)
    } else {
        after_effect(ctx, error)
    }
}

fn after_effect(ctx: &BackendContext<'_>, error: anyhow::Error) -> RuntimeError {
    if ctx.declaration.side_effect == SideEffect::Mutation {
        RuntimeError::unknown(error)
    } else {
        RuntimeError::backend(error)
    }
}

fn required_str<'a>(value: &'a Value, key: &str) -> anyhow::Result<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .with_context(|| format!("{key} is required"))
}

fn is_memory_method(method: &str) -> bool {
    matches!(
        method,
        "memory_store"
            | "memory_recall"
            | "memory_retrieve"
            | "memory_query"
            | "memory_delete"
            | "memory_list_namespaces"
            | "memory_semantic_query"
    )
}

fn bind_memory(
    method: &str,
    context: &ResolvedChatContext,
    args: &mut Value,
) -> anyhow::Result<()> {
    ensure!(
        context.mode.has_memory(),
        "memory is disabled in the selected mode"
    );
    let writes = matches!(method, "memory_store" | "memory_delete");
    ensure!(
        !writes || !context.read_only,
        "memory is read-only in this mode"
    );
    ensure!(
        !writes || args.get("key").and_then(Value::as_str) != Some("_identity"),
        "memory identity metadata is not caller writable"
    );
    let namespace = match (context.mode, args.get("namespace").and_then(Value::as_str)) {
        (ChatMode::Personal, Some(name)) if !name.contains('/') => {
            validate_personal_namespace(name)?;
            format!(
                "{}personal/{name}",
                namespace_prefix_for_session(&context.session_id)
            )
        }
        _ => context.memory_namespace.clone(),
    };
    let object = args
        .as_object_mut()
        .context("memory arguments must be an object")?;
    object.insert("namespace".into(), namespace.clone().into());
    object.insert("container_id".into(), namespace.into());
    object.insert("identity_id".into(), context.session_id.clone().into());
    object.remove("wireguard_pubkey");
    Ok(())
}

fn filter_namespaces(context: &ResolvedChatContext, result: &mut Value) {
    let prefix = namespace_prefix_for_session(&context.session_id);
    if let Some(namespaces) = result.get_mut("namespaces").and_then(Value::as_array_mut) {
        namespaces.retain(|entry| {
            entry
                .get("name")
                .and_then(Value::as_str)
                .is_some_and(|name| {
                    name == "agent_pieces"
                        || name == context.memory_namespace
                        || name.starts_with(&prefix)
                })
        });
        let count = namespaces.len();
        result["count"] = count.into();
    }
}

fn generic_target<'a>(requested: &'a str, arguments: &mut Value) -> anyhow::Result<&'a str> {
    ensure!(
        requested != "invoke_tool",
        "recursive generic invocation is forbidden"
    );
    if requested != "cognitive_memory" {
        return Ok(requested);
    }
    let target = match required_str(arguments, "operation")? {
        "store" => "memory_store",
        "retrieve" => "memory_retrieve",
        "query" => "memory_query",
        "semantic_query" => "memory_semantic_query",
        "delete" => "memory_delete",
        "list_namespaces" => "memory_list_namespaces",
        _ => bail!("memory operation has no sealed method authority"),
    };
    arguments
        .as_object_mut()
        .context("tool arguments must be an object")?
        .remove("operation");
    Ok(target)
}

fn hot_target(
    method: &str,
    args: &Value,
) -> anyhow::Result<Option<(&'static CatalogAgentMethod, Value)>> {
    let agent = match method {
        "rust_pro" => "rust-pro",
        "python_pro" => "python-pro",
        "typescript_pro" => "typescript-pro",
        "sequential_thinking" => "sequential-thinking",
        "context_manager" => "context-manager",
        "code_review_orchestrator" => "code-review-orchestrator",
        _ => return Ok(None),
    };
    let operation = required_str(args, "operation")?;
    let target = cognitive_agents::catalog_agent_method(agent, operation)
        .context("unsupported native agent operation")?;
    let mut normalized = args
        .as_object()
        .context("agent arguments must be an object")?
        .clone();
    normalized.remove("operation");
    normalized.retain(|_, value| !value.is_null());
    if agent == "rust-pro" && !matches!(operation, "build" | "run") {
        ensure!(
            normalized.get("release").and_then(Value::as_bool) != Some(true),
            "release is supported only for build and run"
        );
        normalized.remove("release");
    }
    Ok(Some((target, normalized.into())))
}

async fn admit_agent(
    ctx: &BackendContext<'_>,
    context: &ResolvedChatContext,
    agent: &CatalogAgentMethod,
) -> Result<(), RuntimeError> {
    if context.read_only
        && (agent.category == "execution"
            || matches!(agent.operation.as_str(), "run" | "run_workflow"))
    {
        return Err(RuntimeError::denied("selected chat mode is read-only"));
    }
    if context.mode == ChatMode::Project {
        let capability = agent
            .declaration
            .required_capability
            .as_deref()
            .context("catalog capability absent")?;
        if !context.allowed_agents.contains(&agent.agent_type)
            || !context.effective_capabilities.contains(capability)
        {
            return Err(RuntimeError::denied(
                "selected project does not allow this agent and capability",
            ));
        }
    }
    revalidate(ctx).await
}

async fn execute_agent(
    ctx: &BackendContext<'_>,
    agent: &CatalogAgentMethod,
    args: &Value,
) -> Result<Value, RuntimeError> {
    let implementation = op_agents::unified::GLOBAL_REGISTRY
        .get_normalized(&agent.agent_type)
        .context("native execution implementation is absent")?;
    let connection = zbus::connection::Builder::system()
        .map_err(anyhow::Error::from)?
        .method_timeout(Duration::from_secs(3605))
        .build()
        .await
        .map_err(anyhow::Error::from)?;
    let proxy = zbus::Proxy::new(
        &connection,
        "org.dbusmcp.Agent.UnifiedExecution",
        "/org/dbusmcp/Agent/UnifiedExecution",
        "org.dbusmcp.Agent",
    )
    .await
    .map_err(anyhow::Error::from)?;
    let request = serde_json::to_string(
        &json!({"agent_type":implementation.id(),"operation":agent.operation,
        "arguments":args,"actor_id":ctx.caller.principal_id,"session_id":ctx.caller.session_id,
        "session_genesis":ctx.caller.session_genesis}),
    )?;
    revalidate(ctx).await?;
    let response: String = proxy
        .call("ExecuteCatalog", &(request,))
        .await
        .map_err(|error| RuntimeError::unknown(error.into()))?;
    let value: Value =
        serde_json::from_str(&response).map_err(|error| RuntimeError::unknown(error.into()))?;
    let exit_code = value
        .get("exit_code")
        .and_then(Value::as_i64)
        .ok_or_else(|| {
            RuntimeError::unknown(anyhow!("native executor returned no process exit status"))
        })?;
    Ok(
        json!({"agent_type":agent.agent_type,"operation":agent.operation,"exit_code":exit_code,
        "stdout":value.get("stdout").or_else(|| value.get("output")).and_then(Value::as_str).unwrap_or_default(),
        "stderr":value.get("stderr").or_else(|| value.get("errors")).and_then(Value::as_str).unwrap_or_default()}),
    )
}

async fn selected_notebook(ctx: &BackendContext<'_>) -> Result<Option<String>, RuntimeError> {
    let store = ctx.runtime.identity_store().clone();
    let session = ctx.caller.session_id.clone();
    let rows = tokio::task::spawn_blocking(move || {
        store.run_query(
        "?[notebook_id] := *notebooklm_selection{session_id, notebook_id}, session_id = $session",
        Some(json!({"session":session})),
    )
    })
    .await
    .map_err(|error| RuntimeError::backend(error.into()))??;
    Ok(rows
        .as_array()
        .and_then(|rows| rows.first())
        .and_then(|row| row.get("notebook_id"))
        .and_then(Value::as_str)
        .map(str::to_owned))
}

fn notebook_has_effect(method: &str, args: &Value) -> bool {
    !matches!(
        method,
        "notebook_list"
            | "notebook_get"
            | "notebook_describe"
            | "source_list_drive"
            | "source_describe"
            | "source_get_content"
            | "chat_list"
            | "chat_get"
            | "studio_status"
            | "research_status"
            | "notebook_share_status"
            | "server_info"
            | "get_health"
    ) || args.get("output_path").and_then(Value::as_str).is_some()
}

async fn workflow(
    ctx: &BackendContext<'_>,
    agent: &CatalogAgentMethod,
    args: &Value,
) -> Result<Value, RuntimeError> {
    let owner = op_agents::unified::GLOBAL_REGISTRY
        .get_normalized(&agent.agent_type)
        .context("native orchestration implementation is absent")?;
    let defaults = || {
        owner
            .workflow_steps()
            .into_iter()
            .map(|step| {
                json!({
                    "name":step.name,"agent_type":step.agent_type,"operation":step.operation,
                    "arguments":step.args_template,"condition":step.condition,
                })
            })
            .collect::<Vec<_>>()
    };
    if matches!(agent.operation.as_str(), "list" | "list_steps") {
        return Ok(json!({"workflow_agent":agent.agent_type,"steps":defaults()}));
    }
    let steps = args
        .get("steps")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_else(defaults);
    let mut errors = Vec::new();
    if steps.is_empty() || steps.len() > 64 {
        errors.push(json!({"code":"invalid_size","message":"workflow requires 1 to 64 steps"}));
    }
    let allowed: BTreeSet<String> = owner
        .delegated_agents()
        .into_iter()
        .map(|name| name.replace('_', "-"))
        .collect();
    for (index, step) in steps.iter().enumerate() {
        if let Err(error) = validate_workflow_step(&allowed, step) {
            errors.push(json!({"index":index,"code":"invalid_step","message":error.to_string()}));
        }
    }
    if agent.operation == "validate" {
        return Ok(
            json!({"valid":errors.is_empty(),"workflow_agent":agent.agent_type,"errors":errors,"steps":steps}),
        );
    }
    if !matches!(agent.operation.as_str(), "run" | "run_workflow") {
        return Err(RuntimeError::unsupported(
            "unsupported orchestration operation",
        ));
    }
    if !errors.is_empty() {
        return Err(RuntimeError::backend(anyhow!(
            "workflow validation failed: {errors:?}"
        )));
    }
    // Admit every required child grant before the first effect, then re-admit
    // each step at its actual runtime execution boundary.
    for step in &steps {
        let target = step_target(step)?;
        ctx.runtime
            .require_capability(
                ctx.caller,
                target
                    .declaration
                    .required_capability
                    .as_deref()
                    .context("child capability absent")?,
            )
            .await?;
    }
    let mut results = Vec::with_capacity(steps.len());
    let mut previous: Option<Value> = None;
    for (index, step) in steps.iter().enumerate() {
        let target = step_target(step)?;
        let mut arguments = normalized_step_arguments(target, step)?;
        let schema = serde_json::to_value(&target.declaration.args)?;
        if schema
            .get("properties")
            .and_then(Value::as_object)
            .is_some_and(|properties| properties.contains_key("context"))
        {
            let object = arguments
                .as_object_mut()
                .context("workflow arguments must be an object")?;
            if !object.contains_key("context") {
                let mut context = args
                    .get("context")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                if let Some(previous) = &previous {
                    context.push_str("\nprevious_step_result: ");
                    context.push_str(&serde_json::to_string(previous)?);
                }
                if !context.is_empty() {
                    object.insert("context".into(), context.into());
                }
            }
        }
        let result = nested(ctx, "cognitive", &target.declaration.name, arguments)
            .await
            .map_err(|error| {
                if results.is_empty() {
                    error
                } else {
                    RuntimeError::unknown(anyhow!(
                        "workflow stopped after committed child effects: {error}"
                    ))
                }
            })?;
        let successful = if target.category == "execution" {
            result
                .get("exit_code")
                .and_then(Value::as_i64)
                .context("executor result omitted exit_code")?
                == 0
        } else {
            result
                .get("success")
                .and_then(Value::as_bool)
                .unwrap_or(true)
        };
        let expected = step
            .get("condition")
            .and_then(Value::as_str)
            .map(|condition| condition != "expect_failure")
            .unwrap_or(true);
        if successful != expected {
            return Err(RuntimeError::unknown(anyhow!(
                "workflow child {} violated its expected outcome after execution",
                target.declaration.name
            )));
        }
        previous = Some(result.clone());
        results.push(json!({"index":index,"agent_type":target.agent_type,"operation":target.operation,"success":true,"result":result}));
    }
    Ok(
        json!({"ok":true,"workflow_agent":agent.agent_type,"steps_completed":results.len(),"results":results,"context":args.get("context")}),
    )
}

fn step_target(step: &Value) -> anyhow::Result<&'static CatalogAgentMethod> {
    cognitive_agents::catalog_agent_method(
        required_str(step, "agent_type")?,
        required_str(step, "operation")?,
    )
    .context("workflow child has no sealed catalog declaration")
}

fn normalized_step_arguments(target: &CatalogAgentMethod, step: &Value) -> anyhow::Result<Value> {
    let mut arguments = step.get("arguments").cloned().unwrap_or_else(|| json!({}));
    let schema = serde_json::to_value(&target.declaration.args)?;
    let object = arguments
        .as_object_mut()
        .context("workflow arguments must be an object")?;
    if !object.contains_key("question")
        && schema
            .get("properties")
            .and_then(Value::as_object)
            .is_some_and(|properties| properties.contains_key("question"))
    {
        if let Some(question) = object.remove("query") {
            object.insert("question".into(), question);
        }
    }
    Ok(arguments)
}

fn validate_workflow_step(allowed: &BTreeSet<String>, step: &Value) -> anyhow::Result<()> {
    let target = step_target(step)?;
    ensure!(
        target.category != "orchestration",
        "nested orchestration is not permitted"
    );
    ensure!(
        allowed.contains(&target.agent_type.replace('_', "-")),
        "orchestration does not allow this child agent"
    );
    if let Some(condition) = step.get("condition").and_then(Value::as_str) {
        ensure!(
            matches!(condition, "expect_success" | "expect_failure"),
            "unsupported workflow condition"
        );
    }
    let arguments = normalized_step_arguments(target, step)?;
    jsonschema::validate(&serde_json::to_value(&target.declaration.args)?, &arguments)
        .map_err(|error| anyhow!("workflow child arguments violate schema: {error}"))
}

#[cfg(test)]
mod personal_memory_tests {
    use super::personal_store_path;

    #[test]
    fn personal_store_needs_a_canonical_session_and_its_identity_volume() {
        assert!(personal_store_path("not-a-uuid").is_err());
        assert!(personal_store_path("2A2D9AA8-5468-309C-2DB4-342221450DE8").is_err());
        // A canonical session without an identity volume has no personal store.
        assert!(personal_store_path("00000000-0000-4000-8000-000000000001").is_err());
    }

    #[test]
    fn personal_memory_is_never_paired_with_qdrant() {
        let source = include_str!("runtime_cognitive.rs");
        assert!(source.contains("tool: MemoryTool::new(store.clone(), None),"));
    }
}
