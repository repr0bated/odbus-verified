//! Canonical bridge dispatch for the singleton control-plane chatbot config,
//! pure significance rule, and capability-preserving embedding facade.

use crate::{
    interceptor::load_capability_grants,
    method_contract_admission,
    mutation_engine::MutationEngine,
    project_dispatch,
    sealed_schema_reader::{blob_catalog_dir_from_env, read_manifest_pinned_schema_document},
};
use anyhow::{Context, Result};
use op_cozo_store::{EpisodeEmbeddingRecord, EpisodeScope, ScopedEpisodeRecord};
use op_plugins::state_plugins::{
    ctl_plane_chatbot::{InputType, OutputDtype, ReasoningEpisode},
    ctl_plane_chatbot_methods::{
        complete_vectorization, config_state_fields, cosine_similarity, plan_vectorization,
        validate_episode, ClassifySignificanceInput, ConfigPublication, ContextMatch,
        CtlPlaneChatbotMethods, GetConfigInput, GetDoorbellInput, GetDoorbellOutput,
        GetEpisodeInput, GetEpisodeOutput, ListEpisodesInput, ListEpisodesOutput,
        ListKnowledgeInput, ListKnowledgeOutput, QueryContextInput, QueryContextOutput,
        SetConfigInput, VectorizeInput,
    },
    embedding_model::{
        methods::{EmbedInput, EmbedOutput, EmbeddingInputType},
        EmbeddingModelPlugin,
    },
    runtime_documents::runtime_documents,
};
use op_state::StatePlugin;
use op_state_store::PluginSchema;

const PLUGIN: &str = "ctl_plane_chatbot";
const EMBEDDING_PLUGIN: &str = "embedding_model";
const EMBEDDING_METHOD: &str = "embed";
static CONFIG_OPERATION: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[derive(Clone)]
struct SealPin {
    schema_hash: String,
    schema_json: String,
}

fn pin_contract(admitted: &PluginSchema) -> Result<SealPin> {
    let pinned = read_manifest_pinned_schema_document(&blob_catalog_dir_from_env(), PLUGIN)?;
    method_contract_admission::admit_compiled_state_contract(&pinned, PLUGIN)?;
    anyhow::ensure!(
        serde_json::to_value(&pinned.schema)? == serde_json::to_value(admitted)?,
        "admitted control-plane chatbot schema differs from the current sealed plugin"
    );
    Ok(SealPin {
        schema_hash: pinned.schema_hash,
        schema_json: pinned.blob.schema_json,
    })
}

fn same_seal(expected: &SealPin, current_hash: &str, current_schema_json: &str) -> bool {
    current_hash == expected.schema_hash && current_schema_json == expected.schema_json
}

fn seal_is_unchanged(expected: &SealPin) -> Result<()> {
    let current = read_manifest_pinned_schema_document(&blob_catalog_dir_from_env(), PLUGIN)?;
    method_contract_admission::admit_compiled_state_contract(&current, PLUGIN)?;
    anyhow::ensure!(
        same_seal(expected, &current.schema_hash, &current.blob.schema_json),
        "control-plane chatbot plugin seal changed during operation"
    );
    Ok(())
}

fn required_capability<'a>(schema: &'a PluginSchema, method: &str) -> Result<&'a str> {
    schema
        .methods
        .get(method)
        .with_context(|| format!("admitted {PLUGIN}.{method} declaration is absent"))?
        .required_capability
        .as_deref()
        .context("admitted chatbot method has no exact capability")
}

fn grants_allow(grants: &std::collections::HashSet<String>, required: &str) -> bool {
    grants.contains(required)
}

fn embedding_capability() -> Result<String> {
    let schema = EmbeddingModelPlugin::new()
        .schema()
        .context("embedding_model schema is unavailable")?;
    let declaration = schema
        .methods
        .get(EMBEDDING_METHOD)
        .context("embedding_model does not declare embed")?;
    Ok(schema
        .resolve_method_capability(declaration)
        .capability()
        .to_string())
}

/// Revalidate the caller and the same sealed method at the final execution
/// boundary. Config callers invoke this after waiting for the singleton writer.
/// `CONFIG_OPERATION` only serializes this bridge process; RuntimeDocuments
/// revision CAS remains the cross-process guard.
async fn revalidate_method_boundary(
    engine: &MutationEngine,
    method: &str,
    actor: &str,
    session: Option<&str>,
    genesis: Option<&str>,
    admitted: &PluginSchema,
    pin: &SealPin,
) -> Result<()> {
    project_dispatch::verified_session(engine, actor, session, genesis).await?;
    let required = required_capability(admitted, method)?;
    let grants = load_capability_grants(actor);
    anyhow::ensure!(
        grants_allow(&grants, required),
        "current exact control-plane chatbot capability grant is required"
    );
    let expected = pin.clone();
    blocking(move || seal_is_unchanged(&expected)).await
}

async fn blocking<T: Send + 'static>(
    operation: impl FnOnce() -> Result<T> + Send + 'static,
) -> Result<T> {
    tokio::task::spawn_blocking(operation)
        .await
        .context("control-plane chatbot sealed-contract worker failed")?
}

fn validate_output(schema: &PluginSchema, method: &str, output: &serde_json::Value) -> Result<()> {
    let declaration = schema
        .methods
        .get(method)
        .with_context(|| format!("admitted {PLUGIN}.{method} declaration is absent"))?;
    let returns = declaration
        .returns
        .as_ref()
        .context("admitted chatbot method has no output schema")?;
    let validator = jsonschema::Validator::new(&serde_json::to_value(returns)?)
        .context("admitted chatbot output schema is invalid")?;
    validator.validate(output).map_err(|_| {
        anyhow::anyhow!("chatbot method output does not conform to the admitted sealed contract")
    })
}

fn mark_publication_pending(
    output: &mut op_plugins::state_plugins::ctl_plane_chatbot_methods::SetConfigOutput,
) {
    output.publication = ConfigPublication::Pending;
    output.diagnostic = Some(
        "chatbot config committed durably but its authoritative projection is pending recovery"
            .to_string(),
    );
}

fn decode_episode(row: &ScopedEpisodeRecord, scope: &EpisodeScope) -> Result<ReasoningEpisode> {
    anyhow::ensure!(
        &row.scope == scope,
        "episode owner returned a mismatched identity scope"
    );
    let episode: ReasoningEpisode = serde_json::from_value(row.episode.clone())
        .context("durable chatbot episode is invalid")?;
    validate_episode(&episode)?;
    anyhow::ensure!(
        episode.principal_id == scope.principal_id
            && episode.session_id == scope.session_id
            && episode.session_genesis == scope.session_genesis
            && episode.on_behalf_of == scope.principal_id
            && episode.episode_id == row.episode_id
            && episode.content_hash == row.content_hash,
        "durable chatbot episode identity or hash differs from its owner key"
    );
    Ok(episode)
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn dispatch(
    engine: &MutationEngine,
    method: &str,
    args: &serde_json::Value,
    actor: &str,
    session: Option<&str>,
    genesis: Option<&str>,
    admitted: &PluginSchema,
) -> Result<serde_json::Value> {
    let engine = engine.clone();
    let method = method.to_string();
    let args = args.clone();
    let actor = actor.to_string();
    let session = session.map(str::to_string);
    let genesis = genesis.map(str::to_string);
    let admitted = admitted.clone();
    // The task owns the durable CAS, readback, and SHM publication even if the
    // requesting transport is cancelled after dispatch.
    tokio::spawn(async move {
        dispatch_owned(
            &engine,
            &method,
            &args,
            &actor,
            session.as_deref(),
            genesis.as_deref(),
            &admitted,
        )
        .await
    })
    .await
    .context("control-plane chatbot operation owner failed")?
}

#[allow(clippy::too_many_arguments)]
async fn dispatch_owned(
    engine: &MutationEngine,
    method: &str,
    args: &serde_json::Value,
    actor: &str,
    session: Option<&str>,
    genesis: Option<&str>,
    admitted: &PluginSchema,
) -> Result<serde_json::Value> {
    let verified_session =
        project_dispatch::verified_session(engine, actor, session, genesis).await?;
    let scope = EpisodeScope {
        principal_id: actor.to_owned(),
        session_id: verified_session,
        session_genesis: genesis
            .context("verified chatbot session genesis is required")?
            .to_owned(),
    };
    let pin = blocking({
        let admitted = admitted.clone();
        move || pin_contract(&admitted)
    })
    .await?;
    match method {
        "classify_significance" => {
            revalidate_method_boundary(engine, method, actor, session, genesis, admitted, &pin)
                .await?;
            let input: ClassifySignificanceInput = serde_json::from_value(args.clone())?;
            let output =
                serde_json::to_value(CtlPlaneChatbotMethods::classify_significance(input)?)?;
            let expected = pin.clone();
            blocking(move || seal_is_unchanged(&expected)).await?;
            validate_output(admitted, method, &output)?;
            Ok(output)
        }
        "get_config" => {
            let _operation = CONFIG_OPERATION.lock().await;
            revalidate_method_boundary(engine, method, actor, session, genesis, admitted, &pin)
                .await?;
            let methods = CtlPlaneChatbotMethods::new(runtime_documents().await?);
            let input: GetConfigInput = serde_json::from_value(args.clone())?;
            let receipt = methods.get_config(input).await?;
            let expected = pin.clone();
            blocking(move || seal_is_unchanged(&expected)).await?;
            if let Some(config) = &receipt.config {
                engine
                    .persist_ctl_plane_chatbot_config_fields(
                        config_state_fields(config)?,
                        pin.schema_hash.clone(),
                        "ctl_plane_chatbot:get_config".to_string(),
                    )
                    .await
                    .context(
                        "chatbot config read succeeded but its projection publication failed",
                    )?;
            }
            let output = serde_json::to_value(receipt)?;
            validate_output(admitted, method, &output)?;
            Ok(output)
        }
        "list_episodes" => {
            revalidate_method_boundary(engine, method, actor, session, genesis, admitted, &pin)
                .await?;
            engine
                .chatbot_episode_publisher()?
                .recover(engine, scope.clone())
                .await?;
            let input: ListEpisodesInput =
                serde_json::from_value(args.clone()).context("invalid list_episodes arguments")?;
            anyhow::ensure!(
                input
                    .after_episode_id
                    .as_ref()
                    .is_none_or(|cursor| !cursor.is_empty() && cursor.len() <= 128),
                "episode cursor must be 1..=128 bytes when provided"
            );
            let limit = if input.limit == 0 { 50 } else { input.limit };
            anyhow::ensure!(limit <= 199, "episode list limit must be at most 199");
            let store = engine.ctl_plane_chatbot_episode_cozo().await?;
            let page_scope = scope.clone();
            let after = input.after_episode_id;
            let page = blocking(move || {
                store
                    .list_scoped_episode_page(&page_scope, after.as_deref(), limit as usize)
                    .map_err(Into::into)
            })
            .await?;
            let episodes = page
                .episodes
                .iter()
                .map(|row| decode_episode(row, &scope))
                .collect::<Result<Vec<_>>>()?;
            let output = serde_json::to_value(ListEpisodesOutput {
                episodes,
                next_episode_id: page.next_episode_id,
                episode_count: page.episode_count,
                episode_revision: page.revision,
                episodes_truncated: page.episode_count > page.episodes.len() as u64,
            })?;
            revalidate_method_boundary(engine, method, actor, session, genesis, admitted, &pin)
                .await?;
            validate_output(admitted, method, &output)?;
            Ok(output)
        }
        "list_knowledge" => {
            revalidate_method_boundary(engine, method, actor, session, genesis, admitted, &pin)
                .await?;
            engine
                .chatbot_episode_publisher()?
                .recover(engine, scope.clone())
                .await?;
            let input: ListKnowledgeInput =
                serde_json::from_value(args.clone()).context("invalid list_knowledge arguments")?;
            let limit = if input.limit == 0 { 32 } else { input.limit };
            anyhow::ensure!(limit <= 200, "knowledge list limit must be at most 200");
            let store = engine.ctl_plane_chatbot_episode_cozo().await?;
            let read_scope = scope.clone();
            let (rows, snapshot) = blocking(move || {
                let rows = store.list_scoped_knowledge(&read_scope, limit as usize)?;
                let snapshot = store.snapshot_scoped_episodes(&read_scope, 1)?;
                Ok::<_, anyhow::Error>((rows, snapshot))
            })
            .await?;
            let knowledge = rows
                .iter()
                .map(|row| {
                    anyhow::ensure!(row.scope == scope, "knowledge row crosses identity scope");
                    let push: op_plugins::state_plugins::ctl_plane_chatbot::KnowledgePush =
                        serde_json::from_value(row.push.clone())
                            .context("durable knowledge push is not the typed contract")?;
                    op_plugins::state_plugins::ctl_plane_chatbot_methods::validate_knowledge_push(
                        &push,
                    )?;
                    Ok(push)
                })
                .collect::<Result<Vec<_>>>()?;
            let output = serde_json::to_value(ListKnowledgeOutput {
                knowledge_truncated: snapshot.knowledge_count > rows.len() as u64,
                knowledge,
                knowledge_count: snapshot.knowledge_count,
                episode_revision: snapshot.revision,
            })?;
            revalidate_method_boundary(engine, method, actor, session, genesis, admitted, &pin)
                .await?;
            validate_output(admitted, method, &output)?;
            Ok(output)
        }
        "get_doorbell" => {
            revalidate_method_boundary(engine, method, actor, session, genesis, admitted, &pin)
                .await?;
            let _: GetDoorbellInput =
                serde_json::from_value(args.clone()).context("invalid get_doorbell arguments")?;
            let owner_key = crate::wake_doorbell::owner_key(&scope);
            let root = crate::state_stream::plugin_object_path(PLUGIN);
            let output = serde_json::to_value(GetDoorbellOutput {
                topic: op_plugins::state_plugins::emqx::wake_topic(&owner_key)?,
                owner_key,
                listener: crate::mqtt_doorbell_relay::PUBLIC_LISTENER.to_string(),
                password_prefixes: vec![
                    op_identity::sealed_id::INLINE_PREFIX.to_string(),
                    crate::wake_doorbell::OIA1_PASSWORD_PREFIX.to_string(),
                ],
                knowledge_paths: ["knowledge", "knowledge_count", "knowledge_truncated"]
                    .iter()
                    .map(|child| format!("{root}/{child}"))
                    .collect(),
            })?;
            revalidate_method_boundary(engine, method, actor, session, genesis, admitted, &pin)
                .await?;
            validate_output(admitted, method, &output)?;
            Ok(output)
        }
        "get_episode" => {
            revalidate_method_boundary(engine, method, actor, session, genesis, admitted, &pin)
                .await?;
            engine
                .chatbot_episode_publisher()?
                .recover(engine, scope.clone())
                .await?;
            let input: GetEpisodeInput =
                serde_json::from_value(args.clone()).context("invalid get_episode arguments")?;
            anyhow::ensure!(
                !input.episode_id.is_empty() && input.episode_id.len() <= 128,
                "episode_id must be 1..=128 bytes"
            );
            let store = engine.ctl_plane_chatbot_episode_cozo().await?;
            let read_scope = scope.clone();
            let episode_id = input.episode_id;
            let row = blocking(move || {
                store
                    .get_scoped_episode(&read_scope, &episode_id)
                    .map_err(Into::into)
            })
            .await?;
            let episode = row
                .as_ref()
                .map(|row| decode_episode(row, &scope))
                .transpose()?;
            let output = serde_json::to_value(GetEpisodeOutput { episode })?;
            revalidate_method_boundary(engine, method, actor, session, genesis, admitted, &pin)
                .await?;
            validate_output(admitted, method, &output)?;
            Ok(output)
        }
        "set_config" => {
            let _operation = CONFIG_OPERATION.lock().await;
            revalidate_method_boundary(engine, method, actor, session, genesis, admitted, &pin)
                .await?;
            let methods = CtlPlaneChatbotMethods::new(runtime_documents().await?);
            let input: SetConfigInput = serde_json::from_value(args.clone())?;
            let mut receipt = methods.set_config(input).await?;
            let expected = pin.clone();
            let seal = blocking(move || seal_is_unchanged(&expected)).await;
            let publication = match seal {
                Ok(()) => {
                    engine
                        .persist_ctl_plane_chatbot_config_fields(
                            receipt.represented_state,
                            pin.schema_hash,
                            "ctl_plane_chatbot:set_config".to_string(),
                        )
                        .await
                }
                Err(error) => Err(error),
            };
            if publication.is_err() {
                mark_publication_pending(&mut receipt.output);
            }
            let output = serde_json::to_value(receipt.output)?;
            validate_output(admitted, method, &output)?;
            Ok(output)
        }
        "vectorize" => {
            let _operation = CONFIG_OPERATION.lock().await;
            revalidate_method_boundary(engine, method, actor, session, genesis, admitted, &pin)
                .await?;
            let methods = CtlPlaneChatbotMethods::new(runtime_documents().await?);
            let config = methods
                .get_config(GetConfigInput {})
                .await?
                .config
                .unwrap_or_default();
            anyhow::ensure!(
                config.vectorization_enabled,
                "control-plane chatbot vectorization is disabled"
            );
            anyhow::ensure!(
                config.embedding_plugin == EMBEDDING_PLUGIN,
                "configured chatbot embedding owner is unavailable"
            );
            anyhow::ensure!(
                config.output_dtype == OutputDtype::Float
                    && config.input_type == InputType::Document,
                "configured chatbot vector format is unsupported by the canonical embedding owner"
            );
            let input: VectorizeInput = serde_json::from_value(args.clone())
                .context("invalid control-plane chatbot vectorize arguments")?;
            anyhow::ensure!(
                !input.episode_id.is_empty() && input.episode_id.len() <= 128,
                "episode_id must be 1..=128 bytes"
            );
            let episode_effect = engine
                .chatbot_episode_publisher()?
                .admit_effect(engine, scope.clone())
                .await?;
            let store = engine.ctl_plane_chatbot_episode_cozo().await?;
            let read_scope = scope.clone();
            let episode_id = input.episode_id;
            let row = blocking(move || {
                store
                    .get_scoped_episode(&read_scope, &episode_id)
                    .map_err(Into::into)
            })
            .await?
            .context("chatbot episode does not exist in the verified identity scope")?;
            let episode = decode_episode(&row, &scope)?;
            let target_input = serde_json::to_value(plan_vectorization(&episode)?)?;
            let target_capability = embedding_capability()?;
            let target = method_contract_admission::admit_compiled_method(
                EMBEDDING_PLUGIN,
                EMBEDDING_METHOD,
                Some(&target_capability),
            )?;
            method_contract_admission::validate_admitted_arguments(&target, &target_input)?;
            anyhow::ensure!(
                load_capability_grants(actor).contains(&target_capability),
                "exact nested embedding capability grant is required"
            );
            let verified_session =
                project_dispatch::verified_session(engine, actor, session, genesis).await?;
            let target_args = serde_json::to_string(&target_input)?;
            let origin = crate::tched_router_admission::FacadeOrigin {
                plugin: PLUGIN,
                method: method.to_owned(),
                target_plugin: EMBEDDING_PLUGIN,
                target_method: EMBEDDING_METHOD,
                admitted: admitted.clone(),
                capability: required_capability(admitted, method)?.to_owned(),
            };
            let target_engine = engine.clone();
            let target_actor = actor.to_owned();
            let target_genesis = genesis.map(str::to_owned);
            let target_result = tokio::spawn(async move {
                target_engine
                    .dispatch_method_call_with_facade_origin(
                        EMBEDDING_PLUGIN,
                        EMBEDDING_METHOD,
                        &target_args,
                        Some(&target_capability),
                        &target_actor,
                        Some(&verified_session),
                        target_genesis.as_deref(),
                        origin,
                    )
                    .await
            })
            .await
            .context("chatbot embedding target task failed")??;
            let embedded = serde_json::from_value::<EmbedOutput>(
                target_result
                    .get("result")
                    .cloned()
                    .context("embedding_model response is missing its domain result")?,
            )
            .context("embedding_model returned an invalid embed result")?;
            anyhow::ensure!(
                embedded.dimensions == config.vector_dims.dims(),
                "embedding result differs from the chatbot vector dimension contract"
            );
            let completed = complete_vectorization(&episode, embedded)?;
            let replacement = EpisodeEmbeddingRecord {
                vector: completed.vector.clone(),
                provider: serde_json::to_value(completed.provider)?
                    .as_str()
                    .context("embedding provider is not a string")?
                    .to_owned(),
                model: completed.model.clone(),
                provider_model: completed.provider_model.clone(),
                dimensions: completed.dimensions,
                config_revision: completed.config_revision,
            };
            engine
                .commit_chatbot_episode_embedding(
                    episode_effect,
                    scope,
                    episode.episode_id.clone(),
                    episode.content_hash.clone(),
                    row.embedding,
                    replacement,
                )
                .await?;
            let output = serde_json::to_value(completed)?;
            revalidate_method_boundary(engine, method, actor, session, genesis, admitted, &pin)
                .await?;
            validate_output(admitted, method, &output)?;
            Ok(output)
        }
        "query_context" => {
            let _operation = CONFIG_OPERATION.lock().await;
            revalidate_method_boundary(engine, method, actor, session, genesis, admitted, &pin)
                .await?;
            engine
                .chatbot_episode_publisher()?
                .preflight(engine, &scope)
                .await?;
            let input: QueryContextInput =
                serde_json::from_value(args.clone()).context("invalid query_context arguments")?;
            anyhow::ensure!(
                !input.query.trim().is_empty() && input.query.len() <= 64 * 1024,
                "context query must be 1..=65536 bytes"
            );
            anyhow::ensure!(
                input
                    .after_episode_id
                    .as_ref()
                    .is_none_or(|cursor| !cursor.is_empty() && cursor.len() <= 128),
                "episode cursor must be 1..=128 bytes when provided"
            );
            let limit = if input.limit == 0 { 20 } else { input.limit };
            let scan_limit = if input.scan_limit == 0 {
                199
            } else {
                input.scan_limit
            };
            anyhow::ensure!(limit <= 100, "context result limit must be at most 100");
            anyhow::ensure!(scan_limit <= 199, "context scan limit must be at most 199");
            let store = engine.ctl_plane_chatbot_episode_cozo().await?;
            let page_scope = scope.clone();
            let after = input.after_episode_id;
            let page = blocking(move || {
                store
                    .list_scoped_episode_page(&page_scope, after.as_deref(), scan_limit as usize)
                    .map_err(Into::into)
            })
            .await?;
            let scanned_episodes = u32::try_from(page.episodes.len())?;
            let indexed_episodes = u32::try_from(
                page.episodes
                    .iter()
                    .filter(|row| row.embedding.is_some())
                    .count(),
            )?;
            let unvectorized_episodes = scanned_episodes - indexed_episodes;
            if indexed_episodes == 0 {
                let output = serde_json::to_value(QueryContextOutput {
                    results: vec![],
                    next_episode_id: page.next_episode_id.clone(),
                    scanned_episodes,
                    indexed_episodes,
                    unvectorized_episodes,
                    incompatible_embeddings: 0,
                    more_episodes: page.next_episode_id.is_some(),
                })?;
                revalidate_method_boundary(engine, method, actor, session, genesis, admitted, &pin)
                    .await?;
                validate_output(admitted, method, &output)?;
                return Ok(output);
            }
            let config = CtlPlaneChatbotMethods::new(runtime_documents().await?)
                .get_config(GetConfigInput {})
                .await?
                .config
                .unwrap_or_default();
            anyhow::ensure!(
                config.vectorization_enabled
                    && config.embedding_plugin == EMBEDDING_PLUGIN
                    && config.output_dtype == OutputDtype::Float
                    && config.input_type == InputType::Document,
                "configured chatbot embedding owner is unavailable or unsupported"
            );
            let target_input = serde_json::to_value(EmbedInput {
                texts: vec![input.query],
                input_type: Some(EmbeddingInputType::Query),
                truncate: false,
            })?;
            let target_capability = embedding_capability()?;
            let target = method_contract_admission::admit_compiled_method(
                EMBEDDING_PLUGIN,
                EMBEDDING_METHOD,
                Some(&target_capability),
            )?;
            method_contract_admission::validate_admitted_arguments(&target, &target_input)?;
            anyhow::ensure!(
                load_capability_grants(actor).contains(&target_capability),
                "exact nested embedding capability grant is required"
            );
            let verified_session =
                project_dispatch::verified_session(engine, actor, session, genesis).await?;
            let target_args = serde_json::to_string(&target_input)?;
            let origin = crate::tched_router_admission::FacadeOrigin {
                plugin: PLUGIN,
                method: method.to_owned(),
                target_plugin: EMBEDDING_PLUGIN,
                target_method: EMBEDDING_METHOD,
                admitted: admitted.clone(),
                capability: required_capability(admitted, method)?.to_owned(),
            };
            let embedded_result = engine
                .dispatch_method_call_with_facade_origin(
                    EMBEDDING_PLUGIN,
                    EMBEDDING_METHOD,
                    &target_args,
                    Some(&target_capability),
                    actor,
                    Some(&verified_session),
                    genesis,
                    origin,
                )
                .await?;
            let mut embedded = serde_json::from_value::<EmbedOutput>(
                embedded_result
                    .get("result")
                    .cloned()
                    .context("embedding_model response is missing its domain result")?,
            )?;
            anyhow::ensure!(
                embedded.vectors.len() == 1 && embedded.dimensions == config.vector_dims.dims(),
                "embedding result differs from the chatbot query contract"
            );
            let query_vector = embedded.vectors.remove(0);
            anyhow::ensure!(
                query_vector.len() == embedded.dimensions as usize,
                "embedding owner returned an invalid query vector length"
            );
            let provider = serde_json::to_value(embedded.provider)?
                .as_str()
                .context("embedding provider is not a string")?
                .to_owned();
            let mut incompatible_embeddings = 0_u32;
            let mut results = Vec::new();
            for row in &page.episodes {
                let Some(stored) = &row.embedding else {
                    continue;
                };
                if stored.provider != provider
                    || stored.model != embedded.model
                    || stored.provider_model != embedded.provider_model
                    || stored.dimensions != embedded.dimensions
                    || stored.config_revision != embedded.config_revision
                    || stored.vector.len() != query_vector.len()
                {
                    incompatible_embeddings += 1;
                    continue;
                }
                results.push(ContextMatch {
                    episode: decode_episode(row, &scope)?,
                    score: cosine_similarity(&query_vector, &stored.vector)?,
                });
            }
            results.sort_by(|left, right| {
                right
                    .score
                    .total_cmp(&left.score)
                    .then_with(|| left.episode.episode_id.cmp(&right.episode.episode_id))
            });
            results.truncate(limit as usize);
            let output = serde_json::to_value(QueryContextOutput {
                results,
                next_episode_id: page.next_episode_id.clone(),
                scanned_episodes,
                indexed_episodes,
                unvectorized_episodes,
                incompatible_embeddings,
                more_episodes: page.next_episode_id.is_some(),
            })?;
            revalidate_method_boundary(engine, method, actor, session, genesis, admitted, &pin)
                .await?;
            validate_output(admitted, method, &output)?;
            Ok(output)
        }
        _ => anyhow::bail!("unsupported {PLUGIN} method: {method}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        human_principal_dispatch::tests::{
            pk, register, temp_cozo, temp_shm_with_schemas, test_engine,
        },
        identity_sled_dispatch::{tests::write_identity, IdentitySledStore},
    };
    use op_plugins::state_plugins::{
        ctl_plane_chatbot::{
            CtlPlaneChatbotConfig, CtlPlaneChatbotPlugin, ExitReason, OutcomeClass,
            ReasoningEpisode, Trigger,
        },
        ctl_plane_chatbot_methods::{SetConfigOutput, VectorizeInput},
        human_principal::HumanPrincipalPlugin,
        identity_sled::IdentitySledPlugin,
    };
    use op_state::StatePlugin;
    use std::collections::HashSet;
    use std::time::Duration;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    const TEST_LIMIT: Duration = Duration::from_secs(5);

    struct Environment(Vec<(&'static str, Option<std::ffi::OsString>)>);
    impl Environment {
        fn set(values: &[(&'static str, String)]) -> Self {
            let previous = values
                .iter()
                .map(|(key, _)| (*key, std::env::var_os(key)))
                .collect();
            for (key, value) in values {
                std::env::set_var(key, value);
            }
            Self(previous)
        }
    }
    impl Drop for Environment {
        fn drop(&mut self) {
            for (key, value) in self.0.drain(..) {
                match value {
                    Some(value) => std::env::set_var(key, value),
                    None => std::env::remove_var(key),
                }
            }
        }
    }

    struct NativeResponse {
        status: u16,
        body: serde_json::Value,
    }

    struct NativeEmbedding {
        endpoint: String,
        requests: tokio::sync::mpsc::Receiver<serde_json::Value>,
        responses: tokio::sync::mpsc::Sender<NativeResponse>,
        task: tokio::task::JoinHandle<()>,
    }
    impl Drop for NativeEmbedding {
        fn drop(&mut self) {
            self.task.abort();
        }
    }
    impl NativeEmbedding {
        async fn new() -> Self {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let endpoint = format!("http://{}", listener.local_addr().unwrap());
            let (request_tx, requests) = tokio::sync::mpsc::channel(8);
            let (responses, mut response_rx) = tokio::sync::mpsc::channel::<NativeResponse>(8);
            let task = tokio::spawn(async move {
                loop {
                    let Ok((mut stream, _)) = listener.accept().await else {
                        break;
                    };
                    let mut bytes = Vec::new();
                    let header_end = loop {
                        let mut chunk = [0_u8; 4096];
                        let count = stream.read(&mut chunk).await.unwrap();
                        assert!(count > 0, "native fixture request ended before headers");
                        bytes.extend_from_slice(&chunk[..count]);
                        if let Some(offset) = bytes.windows(4).position(|part| part == b"\r\n\r\n")
                        {
                            break offset + 4;
                        }
                        assert!(bytes.len() <= 64 * 1024);
                    };
                    let headers = String::from_utf8(bytes[..header_end].to_vec()).unwrap();
                    assert_eq!(headers.lines().next().unwrap(), "POST /api/embed HTTP/1.1");
                    let length = headers
                        .lines()
                        .find_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            name.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse::<usize>().unwrap())
                        })
                        .unwrap();
                    while bytes.len() < header_end + length {
                        let mut chunk = [0_u8; 4096];
                        let count = stream.read(&mut chunk).await.unwrap();
                        assert!(count > 0);
                        bytes.extend_from_slice(&chunk[..count]);
                    }
                    let body =
                        serde_json::from_slice(&bytes[header_end..header_end + length]).unwrap();
                    request_tx.send(body).await.unwrap();
                    let Some(response) = response_rx.recv().await else {
                        break;
                    };
                    let body = response.body.to_string();
                    let wire = format!(
                        "HTTP/1.1 {} fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        response.status,
                        body.len(),
                        body
                    );
                    stream.write_all(wire.as_bytes()).await.unwrap();
                }
            });
            Self {
                endpoint,
                requests,
                responses,
                task,
            }
        }

        async fn request(&mut self) -> serde_json::Value {
            tokio::time::timeout(TEST_LIMIT, self.requests.recv())
                .await
                .unwrap()
                .unwrap()
        }

        async fn respond(&self, status: u16, body: serde_json::Value) {
            self.responses
                .send(NativeResponse { status, body })
                .await
                .unwrap();
        }

        async fn assert_no_request(&mut self) {
            assert!(
                tokio::time::timeout(Duration::from_millis(100), self.requests.recv())
                    .await
                    .is_err()
            );
        }
    }

    fn episode(actor: &str, session: &str, genesis: &str) -> ReasoningEpisode {
        let mut episode = ReasoningEpisode {
            principal_id: actor.into(),
            session_id: session.into(),
            session_genesis: genesis.into(),
            on_behalf_of: actor.into(),
            episode_id: "01912abc-def0-7abc-8def-0123456789ab".into(),
            started_at: "2026-09-14T10:00:00Z".into(),
            ended_at: "2026-09-14T10:00:01Z".into(),
            duration_ms: 1_000,
            trigger: Trigger::Goal,
            exit_reason: ExitReason::ResponseEmitted,
            goal_text: Some("private goal marker".into()),
            reasoning_summary: Some("private reasoning marker".into()),
            tools_consulted: vec!["schema.read".into()],
            decision_output: Some("private decision marker".into()),
            outcome_class: OutcomeClass::GoalAchieved,
            confidence: Some(0.9),
            plugin_id: Some("ctl_plane_chatbot".into()),
            conversation_id: Some("fixture".into()),
            content_hash: String::new(),
            pii_flagged: true,
            run: None,
        };
        episode.content_hash =
            op_plugins::state_plugins::ctl_plane_chatbot_methods::episode_content_hash(&episode)
                .unwrap();
        episode
    }

    #[allow(clippy::too_many_arguments)]
    async fn canonical_call(
        engine: &MutationEngine,
        plugin: &str,
        method: &str,
        args: serde_json::Value,
        capability: &str,
        actor: &str,
        session: &str,
        genesis: &str,
    ) -> Result<serde_json::Value> {
        engine
            .dispatch_method_call_with_identity(
                plugin,
                method,
                &args.to_string(),
                Some(capability),
                actor,
                Some(session),
                Some(genesis),
            )
            .await
    }

    fn write_grants(path: &std::path::Path, actor: &str, capabilities: &[&str]) {
        std::fs::write(
            path,
            serde_json::to_vec(&serde_json::json!({
                (actor): { "capabilities": capabilities }
            }))
            .unwrap(),
        )
        .unwrap();
    }

    fn fixture_vector() -> Vec<f32> {
        vec![0.25; 1_024]
    }

    #[tokio::test]
    async fn canonical_vectorize_executes_native_embedding_and_rechecks_both_grants() {
        let _cozo = temp_cozo();
        let _shm = temp_shm_with_schemas(&[
            HumanPrincipalPlugin::new().schema().unwrap(),
            IdentitySledPlugin::new().schema().unwrap(),
            CtlPlaneChatbotPlugin::new().schema().unwrap(),
            EmbeddingModelPlugin::new().schema().unwrap(),
        ]);
        let runtime = tempfile::tempdir().unwrap();
        let grants_dir = tempfile::tempdir().unwrap();
        let grants_path = grants_dir.path().join("grants.json");
        let mut native = NativeEmbedding::new().await;
        let _environment = Environment::set(&[
            (
                "OP_PLUGIN_RUNTIME_COZO_PATH",
                runtime.path().join("runtime").display().to_string(),
            ),
            ("OP_GRANTS_PATH", grants_path.display().to_string()),
            ("EMBEDDING_PROVIDER", "ollama".into()),
            ("EMBEDDING_ENDPOINT", native.endpoint.clone()),
            ("EMBEDDING_MODEL_ID", "fixture-model".into()),
            ("EMBEDDING_DIMENSIONS", "1024".into()),
            ("EMBEDDING_REQUEST_DIMENSIONS", "true".into()),
            ("EMBEDDING_TIMEOUT_MS", "10000".into()),
            ("OLLAMA_API_KEY", String::new()),
        ]);

        let engine = test_engine();
        let cognitive =
            std::sync::Arc::new(op_cozo_store::CozoGraphShuttle::new_in_memory().unwrap());
        engine
            .configure_chatbot_episode_store(cognitive.clone())
            .unwrap();
        let private_root = tempfile::tempdir().unwrap();
        engine
            .configure_chatbot_episode_projection(
                op_core::projection_shm::PrivateProjectionDirectory::prepare(
                    &private_root.path().join("episodes"),
                )
                .unwrap(),
                std::env::var("OP_BLOB_CATALOG_DIR").unwrap().into(),
            )
            .unwrap();
        engine
            .install_identity_sled_store_for_test(IdentitySledStore::new(
                op_cozo_store::CozoGraphShuttle::new_in_memory().unwrap(),
            ))
            .unwrap();
        let key = pk(0xD8);
        register(&key, "chatbot-vector-fixture").await.unwrap();
        let sled = write_identity(&engine, &key).await.unwrap();
        let actor = op_identity::session::derive_principal_id(&key);
        let session = sled.session_id;
        let genesis = sled.genesis.unwrap();
        let episode = episode(&actor, &session, &genesis);
        engine
            .commit_chatbot_episode(ScopedEpisodeRecord {
                scope: EpisodeScope {
                    principal_id: actor.clone(),
                    session_id: session.clone(),
                    session_genesis: genesis.clone(),
                },
                episode_id: episode.episode_id.clone(),
                content_hash: episode.content_hash.clone(),
                episode: serde_json::to_value(&episode).unwrap(),
                embedding: None,
            })
            .await
            .unwrap();
        let vectorize_args = serde_json::to_value(VectorizeInput {
            episode_id: episode.episode_id.clone(),
        })
        .unwrap();

        write_grants(
            &grants_path,
            &actor,
            &["chatbot.invoke", "embedding.invoke"],
        );

        // Occupy embedding_model's own execution lock with a real native call.
        // The facade call can then be observed in the admitted queue before its
        // original outer grant is withdrawn.
        let holder_engine = engine.clone();
        let holder_actor = actor.clone();
        let holder_session = session.clone();
        let holder_genesis = genesis.clone();
        let holder = tokio::spawn(async move {
            canonical_call(
                &holder_engine,
                EMBEDDING_PLUGIN,
                EMBEDDING_METHOD,
                serde_json::json!({
                    "texts": ["projection lock holder"],
                    "truncate": false
                }),
                "embedding.invoke",
                &holder_actor,
                &holder_session,
                &holder_genesis,
            )
            .await
        });
        let holder_request = native.request().await;
        assert_eq!(
            holder_request["input"],
            serde_json::json!(["projection lock holder"])
        );

        let queued_engine = engine.clone();
        let queued_actor = actor.clone();
        let queued_session = session.clone();
        let queued_genesis = genesis.clone();
        let queued_args = vectorize_args.clone();
        let queued = tokio::spawn(async move {
            canonical_call(
                &queued_engine,
                PLUGIN,
                "vectorize",
                queued_args,
                "chatbot.invoke",
                &queued_actor,
                &queued_session,
                &queued_genesis,
            )
            .await
        });
        tokio::time::timeout(TEST_LIMIT, async {
            loop {
                if engine
                    .event_chain
                    .read()
                    .await
                    .events_for_plugin(EMBEDDING_PLUGIN)
                    .len()
                    >= 2
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("facade call reached the embedding execution queue");
        write_grants(&grants_path, &actor, &["embedding.invoke"]);
        native
            .respond(
                200,
                serde_json::json!({
                    "model": "holder-model",
                    "embeddings": [fixture_vector()]
                }),
            )
            .await;
        holder.await.unwrap().unwrap();
        let queued_error = queued.await.unwrap().unwrap_err().to_string();
        assert!(queued_error.contains("current exact backend capability grant is required"));
        native.assert_no_request().await;

        // The successful facade reaches the real Ollama adapter with the
        // deterministic PII-safe document and returns that adapter's vector.
        write_grants(
            &grants_path,
            &actor,
            &["chatbot.invoke", "embedding.invoke"],
        );
        let success_engine = engine.clone();
        let success_actor = actor.clone();
        let success_session = session.clone();
        let success_genesis = genesis.clone();
        let success_args = vectorize_args.clone();
        let success = tokio::spawn(async move {
            canonical_call(
                &success_engine,
                PLUGIN,
                "vectorize",
                success_args,
                "chatbot.invoke",
                &success_actor,
                &success_session,
                &success_genesis,
            )
            .await
        });
        let request = native.request().await;
        let expected_text = concat!(
            "episode_id: 01912abc-def0-7abc-8def-0123456789ab\n",
            "trigger: \"goal\"\n",
            "exit_reason: \"response_emitted\"\n",
            "outcome_class: \"goal_achieved\"\n",
            "tools_consulted: schema.read"
        );
        assert_eq!(request["model"], "fixture-model");
        assert_eq!(request["dimensions"], 1_024);
        assert_eq!(request["input"], serde_json::json!([expected_text]));
        assert!(request.get("input_type").is_none());
        let encoded = request.to_string();
        for private in [
            "private goal marker",
            "private reasoning marker",
            "private decision marker",
        ] {
            assert!(!encoded.contains(private));
        }
        let expected_vector = fixture_vector();
        native
            .respond(
                200,
                serde_json::json!({
                    "model": "provider-fixture",
                    "embeddings": [expected_vector.clone()]
                }),
            )
            .await;
        let success = success.await.unwrap().unwrap();
        let result = &success["result"];
        assert_eq!(result["episode_id"], episode.episode_id);
        assert_eq!(result["content_hash"], episode.content_hash);
        assert_eq!(result["dimensions"], 1_024);
        assert_eq!(result["model"], "fixture-model");
        assert_eq!(result["provider_model"], "provider-fixture");
        assert_eq!(result["provider"], "ollama");
        assert_eq!(
            serde_json::from_value::<Vec<f32>>(result["vector"].clone()).unwrap(),
            expected_vector
        );
        let persisted = cognitive
            .get_scoped_episode(
                &EpisodeScope {
                    principal_id: actor.clone(),
                    session_id: session.clone(),
                    session_genesis: genesis.clone(),
                },
                &episode.episode_id,
            )
            .unwrap()
            .unwrap()
            .embedding
            .unwrap();
        assert_eq!(persisted.vector, fixture_vector());
        assert_eq!(persisted.provider, "ollama");
        assert_eq!(persisted.model, "fixture-model");
        assert_eq!(
            persisted.provider_model.as_deref(),
            Some("provider-fixture")
        );

        // The same committed row is visible through each canonical read
        // method, and query_context delegates to the real embedding owner.
        write_grants(
            &grants_path,
            &actor,
            &["chatbot.read", "chatbot.invoke", "embedding.invoke"],
        );
        let listed = canonical_call(
            &engine,
            PLUGIN,
            "list_episodes",
            serde_json::to_value(ListEpisodesInput {
                after_episode_id: None,
                limit: 0,
            })
            .unwrap(),
            "chatbot.read",
            &actor,
            &session,
            &genesis,
        )
        .await
        .unwrap();
        assert_eq!(listed["result"]["episode_count"], 1);
        assert_eq!(listed["result"]["episode_revision"], 2);
        assert_eq!(
            listed["result"]["episodes"][0]["episode_id"],
            episode.episode_id
        );

        let doorbell = canonical_call(
            &engine,
            PLUGIN,
            "get_doorbell",
            serde_json::json!({}),
            "chatbot.read",
            &actor,
            &session,
            &genesis,
        )
        .await
        .unwrap();
        let owner_key = doorbell["result"]["owner_key"]
            .as_str()
            .unwrap()
            .to_string();
        assert_eq!(owner_key.len(), 64);
        assert_eq!(
            doorbell["result"]["topic"],
            format!("opdbus/wake/{owner_key}"),
            "the topic is derived from the owner key"
        );
        assert_eq!(
            doorbell["result"]["listener"],
            crate::mqtt_doorbell_relay::PUBLIC_LISTENER
        );
        assert!(doorbell["result"]["knowledge_paths"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p.as_str().unwrap().ends_with("/knowledge")));

        let empty = canonical_call(
            &engine,
            PLUGIN,
            "list_knowledge",
            serde_json::json!({}),
            "chatbot.read",
            &actor,
            &session,
            &genesis,
        )
        .await
        .unwrap();
        assert_eq!(empty["result"]["knowledge_count"], 0);
        assert_eq!(empty["result"]["knowledge"], serde_json::json!([]));
        assert_eq!(empty["result"]["episode_revision"], 2);

        let fetched = canonical_call(
            &engine,
            PLUGIN,
            "get_episode",
            serde_json::to_value(GetEpisodeInput {
                episode_id: episode.episode_id.clone(),
            })
            .unwrap(),
            "chatbot.read",
            &actor,
            &session,
            &genesis,
        )
        .await
        .unwrap();
        assert_eq!(
            fetched["result"]["episode"]["episode_id"],
            episode.episode_id
        );
        let query = tokio::spawn({
            let engine = engine.clone();
            let actor = actor.clone();
            let session = session.clone();
            let genesis = genesis.clone();
            async move {
                canonical_call(
                    &engine,
                    PLUGIN,
                    "query_context",
                    serde_json::to_value(QueryContextInput {
                        query: "context query".into(),
                        limit: 10,
                        scan_limit: 10,
                        after_episode_id: None,
                    })
                    .unwrap(),
                    "chatbot.read",
                    &actor,
                    &session,
                    &genesis,
                )
                .await
            }
        });
        let query_request = native.request().await;
        assert_eq!(query_request["input"], serde_json::json!(["context query"]));
        assert!(query_request.get("input_type").is_none());
        native
            .respond(
                200,
                serde_json::json!({
                    "model": "provider-fixture",
                    "embeddings": [fixture_vector()]
                }),
            )
            .await;
        let queried = query.await.unwrap().unwrap();
        let query_result = &queried["result"];
        assert_eq!(query_result["scanned_episodes"], 1);
        assert_eq!(query_result["indexed_episodes"], 1);
        assert_eq!(query_result["unvectorized_episodes"], 0);
        assert_eq!(query_result["incompatible_embeddings"], 0);
        assert_eq!(
            query_result["results"][0]["episode"]["episode_id"],
            episode.episode_id
        );
        assert!((query_result["results"][0]["score"].as_f64().unwrap() - 1.0).abs() < 1e-6);

        write_grants(&grants_path, &actor, &["embedding.invoke"]);
        let missing_read = canonical_call(
            &engine,
            PLUGIN,
            "query_context",
            serde_json::json!({"query": "must be denied", "limit": 1}),
            "chatbot.read",
            &actor,
            &session,
            &genesis,
        )
        .await
        .unwrap_err();
        assert!(missing_read
            .to_string()
            .contains("chatbot capability grant"));
        native.assert_no_request().await;

        write_grants(&grants_path, &actor, &["embedding.invoke"]);
        let missing_outer = canonical_call(
            &engine,
            PLUGIN,
            "vectorize",
            vectorize_args.clone(),
            "chatbot.invoke",
            &actor,
            &session,
            &genesis,
        )
        .await
        .unwrap_err();
        assert!(missing_outer
            .to_string()
            .contains("chatbot capability grant"));
        native.assert_no_request().await;

        write_grants(&grants_path, &actor, &["chatbot.invoke"]);
        let missing_target = canonical_call(
            &engine,
            PLUGIN,
            "vectorize",
            vectorize_args.clone(),
            "chatbot.invoke",
            &actor,
            &session,
            &genesis,
        )
        .await
        .unwrap_err();
        assert!(missing_target
            .to_string()
            .contains("nested embedding capability"));
        native.assert_no_request().await;

        write_grants(
            &grants_path,
            &actor,
            &["chatbot.invoke", "embedding.invoke"],
        );
        let dimension_engine = engine.clone();
        let dimension_actor = actor.clone();
        let dimension_session = session.clone();
        let dimension_genesis = genesis.clone();
        let dimension_args = vectorize_args.clone();
        let dimension = tokio::spawn(async move {
            canonical_call(
                &dimension_engine,
                PLUGIN,
                "vectorize",
                dimension_args,
                "chatbot.invoke",
                &dimension_actor,
                &dimension_session,
                &dimension_genesis,
            )
            .await
        });
        native.request().await;
        native
            .respond(200, serde_json::json!({"embeddings": [[0.25]]}))
            .await;
        assert!(dimension
            .await
            .unwrap()
            .unwrap_err()
            .to_string()
            .contains("dimension does not match"));

        let provider_engine = engine.clone();
        let provider_actor = actor.clone();
        let provider_session = session.clone();
        let provider_genesis = genesis.clone();
        let provider = tokio::spawn(async move {
            canonical_call(
                &provider_engine,
                PLUGIN,
                "vectorize",
                vectorize_args,
                "chatbot.invoke",
                &provider_actor,
                &provider_session,
                &provider_genesis,
            )
            .await
        });
        native.request().await;
        native
            .respond(
                500,
                serde_json::json!({"error": "private provider response marker"}),
            )
            .await;
        let provider_error = provider.await.unwrap().unwrap_err().to_string();
        assert!(provider_error.contains("HTTP 500"));
        assert!(!provider_error.contains("private provider response marker"));
    }

    #[test]
    fn committed_and_pending_durable_receipts_match_the_sealed_method_shape() {
        let schema = CtlPlaneChatbotPlugin::new().schema().unwrap();
        for publication in [ConfigPublication::Committed, ConfigPublication::Pending] {
            let output = serde_json::to_value(SetConfigOutput {
                config: CtlPlaneChatbotConfig::default(),
                revision: 7,
                changed: true,
                publication,
                diagnostic: Some("fixed non-secret diagnostic".into()),
            })
            .unwrap();
            validate_output(&schema, "set_config", &output).unwrap();
        }
    }

    #[test]
    fn final_boundary_derives_exact_grants_and_denies_withdrawal() {
        let schema = CtlPlaneChatbotPlugin::new().schema().unwrap();
        assert_eq!(
            required_capability(&schema, "get_config").unwrap(),
            "chatbot.read"
        );
        assert_eq!(
            required_capability(&schema, "set_config").unwrap(),
            "chatbot.invoke"
        );
        assert_eq!(
            required_capability(&schema, "classify_significance").unwrap(),
            "chatbot.invoke"
        );
        let withdrawn = HashSet::<String>::new();
        assert!(!grants_allow(
            &withdrawn,
            required_capability(&schema, "set_config").unwrap()
        ));
        let present = HashSet::from(["chatbot.invoke".to_string()]);
        assert!(grants_allow(
            &present,
            required_capability(&schema, "set_config").unwrap()
        ));
        assert!(grants_allow(
            &present,
            required_capability(&schema, "classify_significance").unwrap()
        ));
        assert!(required_capability(&schema, "unknown").is_err());
        assert_eq!(embedding_capability().unwrap(), "embedding.invoke");
        assert!(!present.contains(&embedding_capability().unwrap()));
    }

    #[test]
    fn final_boundary_rejects_seal_replacement() {
        let expected = SealPin {
            schema_hash: "a".repeat(64),
            schema_json: "{\"version\":2}".into(),
        };
        assert!(same_seal(&expected, &"a".repeat(64), "{\"version\":2}"));
        assert!(!same_seal(&expected, &"b".repeat(64), "{\"version\":2}"));
        assert!(!same_seal(&expected, &"a".repeat(64), "{\"version\":3}"));
    }

    #[test]
    fn projection_fault_preserves_durable_receipt_and_marks_recovery_pending() {
        let config = CtlPlaneChatbotConfig::default();
        let mut output = SetConfigOutput {
            config: config.clone(),
            revision: 11,
            changed: true,
            publication: ConfigPublication::Committed,
            diagnostic: None,
        };
        mark_publication_pending(&mut output);
        assert_eq!(output.config, config);
        assert_eq!(output.revision, 11);
        assert!(output.changed);
        assert_eq!(output.publication, ConfigPublication::Pending);
        assert!(output
            .diagnostic
            .as_deref()
            .unwrap()
            .contains("pending recovery"));
    }

    #[test]
    fn config_root_merge_preserves_unrelated_siblings() {
        let previous = simd_json::json!({
            "runtime_status": {"ready": true},
            "embedding_plugin": "old"
        });
        let fields = simd_json::serde::to_owned_value(
            config_state_fields(&CtlPlaneChatbotConfig::default()).unwrap(),
        )
        .unwrap();
        let crate::state_tree_fold::StateTreeFold::Present(merged) =
            crate::state_tree_fold::merge_plugin_object_fields(
                Some(&previous),
                PLUGIN,
                &op_plugins::canonical::plugin_path(PLUGIN),
                &fields,
            )
            .unwrap()
        else {
            panic!("expected a complete merged chatbot state")
        };
        assert_eq!(merged["runtime_status"], simd_json::json!({"ready": true}));
        assert_eq!(merged["embedding_plugin"], fields["embedding_plugin"]);
    }
}
