//! Control-plane chatbot methods over the retained durable domain owners.
//! Provider calls and nested embeddings always retain canonical caller admission.

use std::{collections::BTreeMap, sync::Arc};
use anyhow::{Context, Result, ensure};
use op_cozo_store::{CozoGraphShuttle, EpisodeScope, EpisodeSnapshot, ScopedEpisodeRecord};
use op_plugins::state_plugins::{
    ctl_plane_chatbot::{InputType, OutputDtype, ReasoningEpisode},
    ctl_plane_chatbot_methods::*,
    embedding_model::methods::{EmbedInput, EmbedOutput, EmbeddingInputType},
    runtime_documents::RuntimeDocuments,
};
use serde_json::{Value, json};
use crate::{core::{BackendStateUpdate, StateScope}, runtime::{BackendContext, BackendOutcome, RuntimeError}};
use super::{blocking, decode, private_scope};

pub(super) struct ChatbotBackend {
    pub(super) store: Arc<CozoGraphShuttle>,
    methods: CtlPlaneChatbotMethods,
    pub(super) operation: Arc<tokio::sync::Mutex<()>>,
}

impl ChatbotBackend {
    pub(super) fn new(store: Arc<CozoGraphShuttle>, documents: Arc<RuntimeDocuments>) -> Self {
        Self { store, methods: CtlPlaneChatbotMethods::new(documents), operation: Arc::new(tokio::sync::Mutex::new(())) }
    }

    pub(super) async fn snapshot(&self, scope: EpisodeScope) -> Result<Value> {
        let store = self.store.clone();
        let snapshot = blocking(move || Ok(store.snapshot_scoped_episodes(&scope, 32)?)).await?;
        episode_state(snapshot)
    }

    pub(super) async fn dispatch(&self, ctx: &BackendContext<'_>, method: &str, args: &Value) -> Result<BackendOutcome, RuntimeError> {
        match self.dispatch_inner(ctx, method, args).await {
            Ok(outcome) => Ok(outcome),
            Err(error) => {
                let mut error = RuntimeError::from(error);
                if matches!(method, "set_config" | "vectorize") {
                    let guard = self.operation.clone().lock_owned().await;
                    let readback = if method == "set_config" {
                        self.methods.get_config(GetConfigInput {}).await.and_then(|output| output.config.context("chatbot config absent after mutation")).and_then(|config| config_state_fields(&config)).map(|state| (state, StateScope::Public))
                    } else {
                        self.snapshot(EpisodeScope { principal_id: ctx.caller.principal_id.clone(), session_id: ctx.caller.session_id.clone(), session_genesis: ctx.caller.session_genesis.clone() }).await.map(|state| (state, private_scope(ctx.caller)))
                    };
                    if let Ok((state, scope)) = readback { error = error.with_state(BackendStateUpdate::Fields(state), scope); }
                    error = error.with_guard(Box::new(guard));
                }
                Err(error)
            }
        }
    }

    async fn dispatch_inner(&self, ctx: &BackendContext<'_>, method: &str, args: &Value) -> Result<BackendOutcome> {
        if method == "Chat" {
            let result = ctx.runtime.call_nested(ctx.caller, "chat_router", "Chat", args.clone()).await?;
            return Ok(super::unchanged(ctx, result.result));
        }
        let guard = self.operation.clone().lock_owned().await;
        ctx.runtime.authorize_nested(ctx.caller, "ctl_plane_chatbot", method).await?;
        let scope = EpisodeScope { principal_id: ctx.caller.principal_id.clone(), session_id: ctx.caller.session_id.clone(), session_genesis: ctx.caller.session_genesis.clone() };
        let mut publication = BackendStateUpdate::Unchanged;
        let mut output_scope = private_scope(ctx.caller);
        let result = match method {
            "classify_significance" => serde_json::to_value(CtlPlaneChatbotMethods::classify_significance(decode(args)?)?)?,
            "get_config" => {
                let result = self.methods.get_config(decode(args)?).await?;
                if let Some(config) = &result.config { publication = BackendStateUpdate::Fields(config_state_fields(config)?); }
                output_scope = StateScope::Public;
                serde_json::to_value(result)?
            }
            "set_config" => {
                let committed = self.methods.set_config(decode(args)?).await.map_err(RuntimeError::unknown)?;
                publication = BackendStateUpdate::Fields(committed.represented_state);
                output_scope = StateScope::Public;
                serde_json::to_value(committed.output)?
            }
            "list_episodes" => {
                let input: ListEpisodesInput = decode(args)?;
                let limit = bounded_limit(input.limit, 32, 199)?;
                let store = self.store.clone();
                let owner = scope.clone();
                let page = blocking(move || Ok(store.list_scoped_episode_page(&owner, input.after_episode_id.as_deref(), limit)?)).await?;
                let episodes = page.episodes.iter().map(|row| decode_episode(row, &scope)).collect::<Result<Vec<_>>>()?;
                serde_json::to_value(ListEpisodesOutput { episodes_truncated: page.next_episode_id.is_some(), episodes, next_episode_id: page.next_episode_id, episode_count: page.episode_count, episode_revision: page.revision })?
            }
            "get_episode" => {
                let input: GetEpisodeInput = decode(args)?;
                let store = self.store.clone();
                let owner = scope.clone();
                let row = blocking(move || Ok(store.get_scoped_episode(&owner, &input.episode_id)?)).await?;
                serde_json::to_value(GetEpisodeOutput { episode: row.as_ref().map(|row| decode_episode(row, &scope)).transpose()? })?
            }
            "list_knowledge" => {
                let input: ListKnowledgeInput = decode(args)?;
                let limit = bounded_limit(input.limit, 32, 200)?;
                let store = self.store.clone();
                let owner = scope.clone();
                let snapshot = blocking(move || Ok(store.snapshot_scoped_episodes(&owner, limit)?)).await?;
                let mut knowledge = Vec::with_capacity(snapshot.knowledge.len());
                for row in &snapshot.knowledge {
                    ensure!(row.scope == scope, "knowledge owner differs from verified caller");
                    let push = serde_json::from_value(row.push.clone())?;
                    validate_knowledge_push(&push)?;
                    knowledge.push(push);
                }
                serde_json::to_value(ListKnowledgeOutput { knowledge, knowledge_count: snapshot.knowledge_count, episode_revision: snapshot.revision, knowledge_truncated: snapshot.knowledge_truncated })?
            }
            "get_doorbell" => super::doorbell(ctx, "ctl_plane_chatbot")?,
            "vectorize" => {
                let input: VectorizeInput = decode(args)?;
                let config = self.embedding_config().await?;
                let store = self.store.clone();
                let owner = scope.clone();
                let row = blocking(move || Ok(store.get_scoped_episode(&owner, &input.episode_id)?)).await?.context("episode does not exist in caller scope")?;
                let episode = decode_episode(&row, &scope)?;
                let nested = ctx.runtime.call_nested(ctx.caller, "embedding_model", "embed", serde_json::to_value(plan_vectorization(&episode)?)?).await?;
                let embedded: EmbedOutput = decode(&nested.result)?;
                ensure!(embedded.dimensions == config.vector_dims.dims(), "embedding dimensions differ from chatbot contract");
                let completed = complete_vectorization(&episode, embedded)?;
                let replacement = op_cozo_store::EpisodeEmbeddingRecord {
                    vector: completed.vector.clone(), provider: serde_json::to_value(completed.provider)?.as_str().context("embedding provider is not a string")?.to_owned(),
                    model: completed.model.clone(), provider_model: completed.provider_model.clone(), dimensions: completed.dimensions, config_revision: completed.config_revision,
                };
                ctx.caller.authority.revalidate().await?;
                let store = self.store.clone();
                let owner = scope.clone();
                let snapshot = blocking(move || Ok(store.compare_exchange_episode_embedding(&owner, &episode.episode_id, &episode.content_hash, row.embedding.as_ref(), &replacement, 32)?)).await.map_err(RuntimeError::unknown)?;
                publication = BackendStateUpdate::Fields(episode_state(snapshot)?);
                serde_json::to_value(completed)?
            }
            "query_context" => {
                let input: QueryContextInput = decode(args)?;
                ensure!(!input.query.trim().is_empty() && input.query.len() <= 64 * 1024, "query must contain 1..=65536 bytes");
                let limit = bounded_limit(input.limit, 20, 100)?;
                let scan_limit = bounded_limit(input.scan_limit, 199, 199)?;
                let store = self.store.clone();
                let owner = scope.clone();
                let page = blocking(move || Ok(store.list_scoped_episode_page(&owner, input.after_episode_id.as_deref(), scan_limit)?)).await?;
                let scanned = page.episodes.len() as u32;
                let indexed = page.episodes.iter().filter(|row| row.embedding.is_some()).count() as u32;
                let mut results = Vec::new();
                let mut incompatible = 0;
                if indexed != 0 {
                    let config = self.embedding_config().await?;
                    let nested = ctx.runtime.call_nested(ctx.caller, "embedding_model", "embed", serde_json::to_value(EmbedInput { texts: vec![input.query], input_type: Some(EmbeddingInputType::Query), truncate: false })?).await?;
                    let mut embedded: EmbedOutput = decode(&nested.result)?;
                    ensure!(embedded.vectors.len() == 1 && embedded.dimensions == config.vector_dims.dims(), "query embedding differs from chatbot contract");
                    let vector = embedded.vectors.remove(0);
                    let provider = serde_json::to_value(embedded.provider)?.as_str().context("embedding provider is not a string")?.to_owned();
                    for row in &page.episodes {
                        let Some(stored) = &row.embedding else { continue; };
                        if stored.provider != provider || stored.model != embedded.model || stored.provider_model != embedded.provider_model || stored.dimensions != embedded.dimensions || stored.config_revision != embedded.config_revision || stored.vector.len() != vector.len() {
                            incompatible += 1;
                            continue;
                        }
                        results.push(ContextMatch { episode: decode_episode(row, &scope)?, score: cosine_similarity(&vector, &stored.vector)? });
                    }
                    results.sort_by(|a,b| b.score.total_cmp(&a.score).then_with(|| a.episode.episode_id.cmp(&b.episode.episode_id)));
                    results.truncate(limit);
                }
                serde_json::to_value(QueryContextOutput { results, more_episodes: page.next_episode_id.is_some(), next_episode_id: page.next_episode_id, scanned_episodes: scanned, indexed_episodes: indexed, unvectorized_episodes: scanned - indexed, incompatible_embeddings: incompatible })?
            }
            _ => return Err(RuntimeError::unsupported(format!("undeclared ctl_plane_chatbot method {method}")).into()),
        };
        if matches!(publication, BackendStateUpdate::Unchanged) && !matches!(method, "get_config" | "classify_significance" | "get_doorbell") {
            publication = BackendStateUpdate::Fields(self.snapshot(scope).await?);
        }
        Ok(BackendOutcome { result, state: publication, scope: output_scope, additional: Vec::new(), guard: Some(Box::new(guard)) })
    }

    async fn embedding_config(&self) -> Result<op_plugins::state_plugins::ctl_plane_chatbot::CtlPlaneChatbotConfig> {
        let config = self.methods.get_config(GetConfigInput {}).await?.config.unwrap_or_default();
        ensure!(config.vectorization_enabled && config.embedding_plugin == "embedding_model" && config.output_dtype == OutputDtype::Float && config.input_type == InputType::Document, "configured chatbot embedding owner is disabled or incompatible");
        Ok(config)
    }
}

fn bounded_limit(value: u32, default: usize, max: usize) -> Result<usize> {
    let value = if value == 0 { default } else { value as usize };
    ensure!(value <= max, "requested page exceeds {max} entries");
    Ok(value)
}

fn decode_episode(row: &ScopedEpisodeRecord, scope: &EpisodeScope) -> Result<ReasoningEpisode> {
    ensure!(&row.scope == scope, "episode owner differs from verified caller");
    let episode: ReasoningEpisode = serde_json::from_value(row.episode.clone())?;
    validate_episode(&episode)?;
    ensure!(episode.principal_id == scope.principal_id && episode.session_id == scope.session_id && episode.session_genesis == scope.session_genesis && episode.on_behalf_of == scope.principal_id && episode.episode_id == row.episode_id && episode.content_hash == row.content_hash, "episode document differs from durable owner key");
    Ok(episode)
}

pub(super) fn episode_state(snapshot: EpisodeSnapshot) -> Result<Value> {
    let mut episodes = BTreeMap::new();
    for row in &snapshot.episodes { episodes.insert(row.episode_id.clone(), decode_episode(row, &row.scope)?); }
    let mut knowledge = BTreeMap::new();
    for row in &snapshot.knowledge {
        let push = serde_json::from_value(row.push.clone())?;
        validate_knowledge_push(&push)?;
        knowledge.insert(row.push_id.clone(), push);
    }
    Ok(json!({"episodes": episodes, "episode_count": snapshot.episode_count, "episode_revision": snapshot.revision, "episodes_truncated": snapshot.truncated, "knowledge": knowledge, "knowledge_count": snapshot.knowledge_count, "knowledge_truncated": snapshot.knowledge_truncated}))
}
