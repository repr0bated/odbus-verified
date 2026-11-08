//! Native chat/provider adapters for the fresh bridge.
//!
//! The pinned threetched runtime owns inference and provider selection. This
//! module owns no provider mapping, credential mint, or unscoped host tool set.
//! Durable conversations, episodes and generation documents retain their existing
//! owners; the parent Runtime commits their observations before returning receipts.

use std::{path::PathBuf, sync::Arc};
use anyhow::{Context, Result, ensure};
use op_cozo_store::CozoGraphShuttle;
use op_plugins::state_plugins::{
    chat_router::{self, ChatInput, ChatOutput, ObservedChatRouterState},
    chat_router_legacy_methods::{ChatRouterLegacyMethods, ModelRole},
    model_registry as registry,
    runtime_documents::{RuntimeDocuments, runtime_documents},
};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use crate::{
    chat_router_config_secret_policy::PinnedConfigSecretPolicy,
    chat_router_runtime::ChatRouterRuntimeClient,
    core::{BackendStateUpdate, StateScope},
    runtime::{BackendContext, BackendOutcome, Runtime, RuntimeError, VerifiedCaller},
};

#[path = "runtime_chat_ctl.rs"]
mod ctl;
#[path = "runtime_chat_conversations.rs"]
mod conversations;
#[path = "runtime_chat_generations.rs"]
mod generations;

pub(crate) struct ChatRuntime {
    documents: Arc<RuntimeDocuments>,
    registry: Arc<CozoGraphShuttle>,
    chatbot: ctl::ChatbotBackend,
    conversations: conversations::ConversationBackend,
    generations: generations::GenerationBackend,
}

impl ChatRuntime {
    pub(crate) async fn open() -> Result<Self> {
        let documents = runtime_documents().await?;
        let shared = documents.shared_store();
        let (registry, episodes, conversations) = blocking(move || {
            let registry = Arc::new(open_store("MODEL_REGISTRY_DB_PATH", "/var/lib/op-dbus/model-registry.db")?);
            let episodes = Arc::new(open_store("CTL_PLANE_CHATBOT_EPISODE_DB_PATH", "/var/lib/op-dbus/chatbot-episodes.db")?);
            let conversations = Arc::new(open_store("CHAT_RESPONSE_DB_PATH", "/var/lib/op-dbus/chat-responses.db")?);
            registry.ensure_distinct_from(&episodes)?;
            registry.ensure_distinct_from(&conversations)?;
            episodes.ensure_distinct_from(&conversations)?;
            registry.ensure_distinct_from(&shared)?;
            episodes.ensure_distinct_from(&shared)?;
            conversations.ensure_distinct_from(&shared)?;
            registry.initialize_model_registry_store()?;
            episodes.initialize_chatbot_episode_store()?;
            conversations.initialize_chat_response_store()?;
            Ok((registry, episodes, conversations))
        }).await?;
        Ok(Self {
            chatbot: ctl::ChatbotBackend::new(episodes, documents.clone()),
            conversations: conversations::ConversationBackend::new(conversations),
            generations: generations::GenerationBackend::new(documents.clone()),
            registry,
            documents,
        })
    }

    pub(crate) fn episode_store(&self) -> Arc<CozoGraphShuttle> { self.chatbot.store.clone() }

    pub(crate) fn ensure_distinct_from(&self, store: &CozoGraphShuttle) -> Result<()> {
        self.registry.ensure_distinct_from(store)?;
        self.chatbot.store.ensure_distinct_from(store)?;
        self.conversations.store.ensure_distinct_from(store)?;
        Ok(())
    }

    pub(crate) async fn commit_episode(&self, runtime: &Runtime, caller: &VerifiedCaller, episode: op_cozo_store::ScopedEpisodeRecord) -> Result<(), RuntimeError> {
        let _guard = self.chatbot.operation.clone().lock_owned().await;
        caller.authority.revalidate().await?;
        if episode.scope.principal_id != caller.principal_id || episode.scope.session_id != caller.session_id || episode.scope.session_genesis != caller.session_genesis {
            return Err(RuntimeError::denied("episode owner differs from the verified caller"));
        }
        let store = self.episode_store();
        let snapshot = blocking(move || Ok(store.insert_scoped_episode_and_snapshot(&episode, 32)?)).await.map_err(RuntimeError::unknown)?;
        runtime.publish_backend_state(caller, "ctl_plane_chatbot", BackendStateUpdate::Fields(ctl::episode_state(snapshot)?), private_scope(caller)).await
    }

    pub(crate) async fn dispatch(&self, ctx: &BackendContext<'_>, plugin: &str, method: &str, args: &Value) -> Result<BackendOutcome, RuntimeError> {
        match plugin {
            "chat_router" if chat_router::generation::is_method(method) => self.generations.dispatch(ctx, method, args).await,
            "chat_router" => self.router(ctx, method, args).await,
            "chat_response" => self.conversations.dispatch(ctx, method, args).await,
            "ctl_plane_chatbot" => self.chatbot.dispatch(ctx, method, args).await,
            "gemma_brain" => crate::runtime_workspace::dispatch_gemma_workspace(ctx, method, args).await,
            "model_registry" => self.models(ctx, method, args).await.map_err(RuntimeError::from),
            "large_language_model" if method == "stream_generate" => {
                let result = ctx.runtime.call_nested(ctx.caller, "chat_router", "start_generation", args.clone()).await?;
                Ok(unchanged(ctx, result.result))
            }
            "persona" => self.persona(ctx, method, args).await.map_err(RuntimeError::from),
            "threetched" => {
                let source = ctx.schema.example.as_ref().context("sealed threetched discovery descriptor is absent")?;
                let source = serde_json::from_value(serde_json::to_value(source)?)?;
                let guard = Arc::new(ChatRouterRuntimeClient::selection_guard().await);
                ctx.runtime.authorize_nested(ctx.caller, "threetched", method).await?;
                let result = op_plugins::state_plugins::threetched::dispatch(method, args, &source, guard.clone()).await.map_err(|error| RuntimeError::unknown(error).with_guard(Box::new(guard.clone())))?;
                let mut outcome = unchanged(ctx, result);
                outcome.guard = Some(Box::new(guard));
                Ok(outcome)
            }
            _ => Err(RuntimeError::unsupported(format!("unknown chat backend {plugin}.{method}"))),
        }
    }

    async fn models(&self, ctx: &BackendContext<'_>, method: &str, args: &Value) -> Result<BackendOutcome> {
        use crate::model_registry_dispatch as backend;
        ctx.caller.authority.revalidate().await?;
        let env = backend::Environment::from_env();
        let result = match method {
            "list_providers" => serde_json::to_value(backend::list_providers(&env, self.registry.clone()).await?)?,
            "list_models" => serde_json::to_value(backend::list_models(&env, self.registry.clone(), decode(args)?).await?)?,
            "refresh" => serde_json::to_value(backend::refresh(&env, &backend::HttpLister, self.registry.clone(), decode(args)?).await.map_err(RuntimeError::unknown)?)?,
            "set_model_profile" => serde_json::to_value(backend::set_model_profile(&env, self.registry.clone(), decode(args)?).await.map_err(RuntimeError::unknown)?)?,
            "maintain_credentials" => serde_json::to_value(backend::maintain_credentials(&env, decode(args)?).await.map_err(RuntimeError::unknown)?)?,
            "native_list_providers" => {
                let schema = sealed_schema(ctx, "chat_router")?;
                let client = native_client(ctx, "dashboard").await?;
                let providers = client.native_providers_inventory(&config_secrets(&schema)?).await?;
                serde_json::to_value(registry::NativeListProvidersOutput { providers })?
            }
            _ => anyhow::bail!("unknown model_registry method {method}"),
        };
        // Models are durable rows returned by the paged methods, not fields in
        // ModelRegistryState. Only its declared configuration is projected.
        let state = serde_json::to_value(registry::ModelRegistryState {
            providers_path: env.providers_path.to_str().context("provider path is not UTF-8")?.to_owned(),
            secrets_dir: env.secrets_dir.to_str().context("secrets path is not UTF-8")?.to_owned(),
            ..registry::ModelRegistryState::default()
        })?;
        Ok(BackendOutcome { result, state: BackendStateUpdate::Fields(state), scope: StateScope::Public, additional: Vec::new(), guard: None })
    }

    async fn router(&self, ctx: &BackendContext<'_>, method: &str, args: &Value) -> Result<BackendOutcome, RuntimeError> {
        if method == "Chat" {
            return self.complete(ctx, decode(args)?).await.and_then(|output| Ok(unchanged(ctx, serde_json::to_value(output)?))).map_err(RuntimeError::from);
        }
        let guard = Arc::new(ChatRouterRuntimeClient::selection_guard().await);
        match self.router_inner(ctx, method, args).await {
            Ok(mut outcome) => { outcome.guard = Some(Box::new(guard)); Ok(outcome) }
            Err(error) => {
                let mut error = if ctx.declaration.side_effect == op_state_store::SideEffect::Mutation { RuntimeError::unknown(error) } else { RuntimeError::from(error) };
                if ctx.declaration.side_effect == op_state_store::SideEffect::Mutation {
                    if let (Ok(client), Ok(secrets)) = (native_client(ctx, "dashboard").await, config_secrets(ctx.schema)) {
                        if let Ok(observation) = client.observe_config_and_state(&secrets).await {
                            if let Ok((_, fields)) = observation.into_state_and_fields() {
                                error = error.with_state(BackendStateUpdate::Fields(fields), private_scope(ctx.caller));
                            }
                        }
                    }
                }
                Err(error.with_guard(Box::new(guard)))
            }
        }
    }

    async fn router_inner(&self, ctx: &BackendContext<'_>, method: &str, args: &Value) -> Result<BackendOutcome> {
        // No MCP tool set is exported or activated. Retained read-only config
        // inspection does not create an MCP endpoint.
        ctx.runtime.authorize_nested(ctx.caller, "chat_router", method).await?;
        if let Some(field) = match method { "GetRouter" => Some("router"), "ListUiSurfaces" => Some("ui_surfaces"), "GetStructuredOutput" => Some("structured_output"), _ => None } {
            let value = ctx.schema.fields.get(field).and_then(|field| field.default.as_ref()).context("sealed router descriptor is absent")?;
            return Ok(unchanged(ctx, json!({(field): serde_json::to_value(value)?})));
        }
        if method == "GetTools" {
            return Ok(unchanged(ctx, json!({"tools": visible_tools(ctx).await?})));
        }
        let client = native_client(ctx, "dashboard").await?;
        let secrets = config_secrets(ctx.schema)?;
        match crate::chat_router_config_dispatch::dispatch_config_read(&client, method, args, &secrets).await? {
            crate::chat_router_config_dispatch::ConfigReadDispatch::Handled(result) => return Ok(unchanged(ctx, result)),
            crate::chat_router_config_dispatch::ConfigReadDispatch::NativeConfig { section } => {
                let observation = client.observe_config_and_state(&secrets).await?;
                let config = match section { Some(section) => observation.native_config.get(section).cloned().context("native config section is absent")?, None => observation.native_config.clone() };
                let (_, fields) = observation.into_state_and_fields()?;
                return Ok(observed(ctx, json!({"config": config}), fields));
            }
            crate::chat_router_config_dispatch::ConfigReadDispatch::NotAConfigRead => {}
        }
        if let Some(mutation) = crate::chat_router_config_dispatch::dispatch_config_mutation(&client, method, args, &secrets).await.map_err(RuntimeError::unknown)? {
            let (_, fields) = client.observe_config_and_state(&secrets).await.map_err(RuntimeError::unknown)?.into_state_and_fields()?;
            return Ok(observed(ctx, mutation.result, fields));
        }
        match method {
            "SetProvider" => { client.set_provider(&decode::<chat_router::SetProviderInput>(args)?.provider_id).await.map_err(RuntimeError::unknown)?; }
            "SetModel" => { client.set_model(&decode::<chat_router::SetModelInput>(args)?.model_id).await.map_err(RuntimeError::unknown)?; }
            "SetSelection" => { let input: chat_router::SetSelectionInput = decode(args)?; client.set_selection(&input.provider_id, &input.model_id).await.map_err(RuntimeError::unknown)?; }
            "resolve_catalog_model_binding" | "apply_catalog_model_binding" => {
                let input: chat_router::ResolveCatalogModelBindingInput = decode(args)?;
                let prior_context = if method == "apply_catalog_model_binding" {
                    let context = ctx.runtime.call_nested(ctx.caller, "project", "get_chat_context", json!({})).await?.result;
                    let mode = context.get("context").unwrap_or(&context)["mode"].as_str().context("committed chat mode is absent")?;
                    ensure!(!matches!(mode, "control_plane" | "accountability"), "dedicated agent model changes require their protected operation");
                    Some(context)
                } else { None };
                let target = crate::model_registry_dispatch::binding_target_from(&crate::model_registry_dispatch::Environment::from_env(), self.registry.clone(), &input.provider_id, &input.model_id).await?;
                let binding = client.observe_catalog_binding(&target, &secrets).await?;
                let after = crate::model_registry_dispatch::binding_target_from(&crate::model_registry_dispatch::Environment::from_env(), self.registry.clone(), &input.provider_id, &input.model_id).await?;
                ensure!(target == after, "catalog authority changed during native binding observation");
                if method == "apply_catalog_model_binding" {
                    client.guard_ordinary_profile(&binding.binding.reference, &secrets).await?;
                    ctx.runtime.authorize_nested(ctx.caller, "chat_router", method).await?;
                    let current = ctx.runtime.call_nested(ctx.caller, "project", "get_chat_context", json!({})).await?.result;
                    ensure!(prior_context.as_ref() == Some(&current), "committed chat context changed during model selection");
                    client.set_selection(&binding.binding.reference, &binding.binding.model).await.map_err(RuntimeError::unknown)?;
                }
                let current = client.observe_catalog_binding(&target, &secrets).await?;
                ensure!(binding.binding == current.binding, "native model binding changed during selection");
                let result = serde_json::to_value(&current.binding)?;
                let (_, mut fields) = client.observe_config_and_state(&secrets).await?.into_state_and_fields()?;
                fields.as_object_mut().context("native observation is not an object")?.insert("catalog_model_binding".into(), result.clone());
                return Ok(observed(ctx, result, fields));
            }
            "get_own_agent_route" => {
                let context = ctx.runtime.call_nested(ctx.caller, "project", "get_chat_context", json!({})).await?;
                let mode = serde_json::from_value(context.result.get("context").unwrap_or(&context.result)["mode"].clone())?;
                let observation = client.observe_config_and_state(&secrets).await?;
                let result = serde_json::to_value(crate::chat_router_agent_route::project(mode, &observation.native_config)?)?;
                return Ok(observed(ctx, result, observation.into_state_and_fields()?.1));
            }
            _ => {}
        }
        let observation = client.observe_config_and_state(&secrets).await?;
        let native = ObservedChatRouterState::from(&observation.state);
        let (state, mut fields) = observation.into_state_and_fields()?;
        let methods = ChatRouterLegacyMethods::new(self.documents.clone());
        if let Some(role) = role(method) {
            let model = args["model_id"].as_str().context("role assignment requires model_id")?;
            ctx.caller.authority.revalidate().await?;
            let (receipt, represented) = methods.assign(role, model, &native).await.map_err(RuntimeError::unknown)?;
            merge_fields(&mut fields, represented)?;
            return Ok(observed(ctx, json!({(receipt.role): receipt.model_id}), fields));
        }
        let result = match method {
            "GetModelRoutes" | "ResolveRoute" => {
                let output = methods.observe(method, args, &native).await?;
                merge_fields(&mut fields, output.represented_state)?;
                output.output
            }
            "SetProvider" => json!({"selected_provider": state.selected_provider}),
            "SetModel" => json!({"selected_model": state.selected_model}),
            "SetSelection" => json!({"selected_provider": state.selected_provider, "selected_model": state.selected_model}),
            "GetState" | "ListModels" | "ListProviders" => chat_router::dispatch_chat_router_method(method, &serde_json::to_string(args)?, &state)?.result,
            _ => anyhow::bail!("unknown chat_router method {method}"),
        };
        Ok(observed(ctx, result, fields))
    }

    pub(super) async fn complete(&self, ctx: &BackendContext<'_>, input: ChatInput) -> Result<ChatOutput> {
        // Persisted mode, not request-supplied role names, selects dedicated routes.
        let context = ctx.runtime.call_nested(ctx.caller, "project", "get_chat_context", json!({})).await?;
        let context = context.result.get("context").unwrap_or(&context.result);
        let mode: op_plugins::state_plugins::project::ChatMode = serde_json::from_value(context["mode"].clone()).context("committed chat mode is invalid")?;
        let alias = match mode { op_plugins::state_plugins::project::ChatMode::ControlPlane => "control_plane", op_plugins::state_plugins::project::ChatMode::Accountability => "accountability", _ => "dashboard" };
        let mut input = input;
        if alias != "dashboard" { input.provider.clear(); input.model.clear(); }
        let client = native_client(ctx, alias).await?;
        let admission = generation_admission(ctx).await?;
        let _guard = ChatRouterRuntimeClient::selection_guard().await;
        ctx.caller.authority.revalidate().await?;
        client.complete_under_guard(input, admission).await.map_err(|error| RuntimeError::unknown(error).into())
    }

    async fn persona(&self, ctx: &BackendContext<'_>, method: &str, args: &Value) -> Result<BackendOutcome> {
        if method == "execute" {
            let input: op_plugins::state_plugins::persona::ExecuteInput = decode(args)?;
            let declaration = op_plugins::state_plugins::cognitive_agents::catalog_agent_method(&input.agent_type, &input.operation).context("persona operation is not in the native agent catalog")?;
            let mut arguments = match input.args { Some(Value::Object(arguments)) => arguments, Some(_) => anyhow::bail!("persona arguments must be an object matching the declared cognitive method"), None => serde_json::Map::new() };
            if let Some(query) = input.query {
                let schema = serde_json::to_value(&declaration.declaration.args)?;
                let key = if schema["properties"].get("question").is_some() { "question" } else { "query" };
                arguments.insert(key.into(), Value::String(query));
            }
            if let Some(path) = input.path { arguments.insert("path".into(), Value::String(path)); }
            let result = ctx.runtime.call_nested(ctx.caller, "cognitive", &declaration.declaration.name, Value::Object(arguments)).await?;
            let success = result.result.get("success").and_then(Value::as_bool)
                .or_else(|| result.result.get("ok").and_then(Value::as_bool))
                .or_else(|| result.result.get("valid").and_then(Value::as_bool))
                .or_else(|| result.result.get("exit_code").and_then(Value::as_i64).map(|code| code == 0))
                .unwrap_or(true);
            return Ok(unchanged(ctx, serde_json::to_value(op_plugins::state_plugins::persona::ExecuteOutput { success, agent_type: input.agent_type, operation: input.operation, data: result.result, message: None, metadata: None })?));
        }
        // The catalog is deliberately code-defined; set_identity fails honestly.
        // authenticate has no external authentication backend and must not pretend.
        if method == "authenticate" { return Err(RuntimeError::unsupported("persona.authenticate has no retained credential verification owner; use the existing SID1/OIA1 admission surface").into()); }
        let result = op_plugins::state_plugins::persona::dispatch_persona_method(method, args).await?;
        Ok(unchanged(ctx, result))
    }
}

pub(crate) fn supports(plugin: &str, method: &str) -> bool {
    match plugin {
        "chat_router" => chat_router::generation::is_method(method) || matches!(method, "GetState" | "GetModelRoutes" | "GetTools" | "ListProviders" | "GetRouter" | "ListUiSurfaces" | "GetStructuredOutput" | "ResolveRoute" | "Chat" | "ListModels" | "SetProvider" | "SetModel" | "SetSelection" | "SetOvsRoutingModel" | "SetObfuscationModel" | "SetVectorizationModel" | "SetQdrantRetrievalModel" | "SetCozoRetrievalModel" | "GetConfig" | "PatchConfig" | "GetMcpConfig" | "get_own_agent_route" | "resolve_catalog_model_binding" | "apply_catalog_model_binding" | "config_list" | "config_get" | "config_set" | "config_patch" | "config_init" | "config_migrate"),
        "chat_response" => matches!(method, "start_conversation" | "submit_turn" | "respond_to_user" | "request_clarification" | "cannot_perform" | "get_turn_response" | "list_conversations" | "get_conversation" | "list_pending_turns"),
        "ctl_plane_chatbot" => matches!(method, "get_config" | "set_config" | "classify_significance" | "vectorize" | "query_context" | "list_episodes" | "get_episode" | "list_knowledge" | "get_doorbell" | "Chat"),
        "model_registry" => matches!(method, "list_providers" | "list_models" | "refresh" | "set_model_profile" | "native_list_providers" | "maintain_credentials"),
        "gemma_brain" => crate::runtime_workspace::supports_gemma_workspace(method),
        "large_language_model" => method == "stream_generate",
        "persona" => matches!(method, "get_identity" | "set_identity" | "authenticate" | "execute" | "list_triggers" | "set_trigger" | "delete_trigger"),
        "threetched" => true, // Parent admits only the manifest-pinned discovered command.
        _ => false,
    }
}

fn open_store(variable: &str, default: &str) -> Result<CozoGraphShuttle> {
    let path = PathBuf::from(std::env::var_os(variable).unwrap_or_else(|| default.into()));
    ensure!(path.is_absolute(), "{variable} must name a persistent absolute database path");
    Ok(CozoGraphShuttle::new_persistent_private(path)?)
}

pub(super) async fn blocking<T: Send + 'static>(operation: impl FnOnce() -> Result<T> + Send + 'static) -> Result<T> {
    tokio::task::spawn_blocking(operation).await.context("native chat owner worker failed")?
}
pub(super) fn decode<T: DeserializeOwned>(args: &Value) -> Result<T> { serde_json::from_value(args.clone()).context("invalid admitted chat arguments") }
pub(super) fn private_scope(caller: &VerifiedCaller) -> StateScope { StateScope::Private { principal_id: caller.principal_id.clone(), session_id: caller.session_id.clone() } }
fn unchanged(ctx: &BackendContext<'_>, result: Value) -> BackendOutcome { BackendOutcome { result, state: BackendStateUpdate::Unchanged, scope: private_scope(ctx.caller), additional: Vec::new(), guard: None } }
fn observed(ctx: &BackendContext<'_>, result: Value, state: Value) -> BackendOutcome { BackendOutcome { result, state: BackendStateUpdate::Fields(state), scope: private_scope(ctx.caller), additional: Vec::new(), guard: None } }
fn merge_fields(target: &mut Value, source: Value) -> Result<()> { target.as_object_mut().context("observation is not an object")?.extend(source.as_object().context("observation fields are not an object")?.clone()); Ok(()) }
fn role(method: &str) -> Option<ModelRole> { Some(match method { "SetOvsRoutingModel" => ModelRole::OvsRouting, "SetObfuscationModel" => ModelRole::Obfuscation, "SetVectorizationModel" => ModelRole::Vectorization, "SetQdrantRetrievalModel" => ModelRole::QdrantRetrieval, "SetCozoRetrievalModel" => ModelRole::CozoRetrieval, _ => return None }) }
fn sealed_schema(ctx: &BackendContext<'_>, plugin: &str) -> Result<op_state_store::PluginSchema> {
    let pinned = crate::sealed_schema_reader::read_manifest_pinned_schema_document(ctx.runtime.catalog_dir(), plugin)?;
    ensure!(ctx.runtime.catalog().plugin(plugin).map(|entry| entry.schema_hash.as_str()) == Some(pinned.schema_hash.as_str()), "sealed native dependency changed; reopen the runtime");
    Ok(pinned.schema)
}
fn config_secrets(schema: &op_state_store::PluginSchema) -> Result<PinnedConfigSecretPolicy> {
    let field = schema.fields.get("native_config").context("sealed native_config schema is absent")?;
    let op_state_store::FieldType::JsonSchema(document) = &field.field_type else { anyhow::bail!("native_config is not an exact schema"); };
    PinnedConfigSecretPolicy::new(Arc::new(serde_json::to_value(document.as_ref())?))
}
pub(super) async fn native_client(ctx: &BackendContext<'_>, alias: &str) -> Result<ChatRouterRuntimeClient> {
    ctx.caller.authority.revalidate().await?;
    Ok(ChatRouterRuntimeClient::for_agent_alias(alias).with_sealed_identity(Some(original_payload(ctx.caller)?)).with_trace_id(&ctx.caller.trace_id))
}
struct GenerationAdmission { caller: VerifiedCaller, credential: String, runtime: Runtime, method: String }
impl crate::chat_router_generation::NativeGenerationAdmission for GenerationAdmission {
    fn sealed_identity(&self) -> Option<&str> { Some(&self.credential) }
    fn trace_id(&self) -> Option<&str> { Some(&self.caller.trace_id) }
    fn check(&self) -> std::pin::Pin<Box<dyn Future<Output = Result<(), crate::chat_router_generation::NativeGenerationError>> + Send + '_>> {
        Box::pin(async { self.runtime.authorize_nested(&self.caller, "chat_router", &self.method).await.map_err(|_| crate::chat_router_generation::NativeGenerationError::Terminal(crate::chat_router_generation::NativeErrorCode::Auth)) })
    }
}
pub(super) async fn generation_admission(ctx: &BackendContext<'_>) -> Result<Arc<dyn crate::chat_router_generation::NativeGenerationAdmission>> {
    Ok(Arc::new(GenerationAdmission { caller: ctx.caller.for_background(), credential: original_payload(ctx.caller)?.to_owned(), runtime: ctx.runtime.clone(), method: ctx.declaration.name.clone() }))
}
fn original_payload(caller: &VerifiedCaller) -> Result<&str> {
    caller.sealed_identity.as_deref().and_then(|identity| identity.strip_prefix(op_identity::sealed_id::INLINE_PREFIX)).context("original authored SID1 transport payload is absent")
}

async fn visible_tools(ctx: &BackendContext<'_>) -> Result<Vec<op_plugins::state_plugins::common::llm_projection::LlmTool>> {
    let grants = ctx.runtime.current_grants(ctx.caller).await?;
    let mut tools = Vec::new();
    for plugin in ctx.runtime.catalog().plugins() {
        let schema = sealed_schema(ctx, &plugin.id)?;
        for declaration in schema.methods.values() {
            if declaration.required_capability.as_ref().is_some_and(|cap| grants.contains(cap)) && plugin.id != "mcp" {
                tools.push(op_plugins::state_plugins::common::llm_projection::LlmTool { name: format!("/operations/{}/{}/call", tched_fs::namespace::encode_component(&plugin.id), tched_fs::namespace::encode_component(&declaration.name)), description: format!("{}: {}", plugin.name, declaration.name), parameters: serde_json::to_value(&declaration.args)?, response_schema: serde_json::to_value(&declaration.returns)? });
            }
        }
    }
    Ok(tools)
}

pub(super) fn doorbell(ctx: &BackendContext<'_>, plugin: &str) -> Result<Value> {
    let key = crate::runtime_mqtt::owner_key_for_claims(ctx.caller.claims())?;
    let root = op_plugins::canonical::plugin_path(plugin);
    let knowledge_paths: Vec<String> = ["knowledge", "knowledge_count", "knowledge_truncated"]
        .iter().map(|child| format!("{root}/{child}")).collect();
    Ok(json!({"owner_key": key, "topic": op_plugins::state_plugins::emqx::wake_topic(&key)?, "listener": "wss://10.0.0.2:8090/mqtt", "password_prefixes": ["sid1:", "oia1:"], "knowledge_paths": knowledge_paths}))
}
