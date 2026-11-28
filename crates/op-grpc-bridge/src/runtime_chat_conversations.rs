//! Principal-private durable conversations and the native responder continuation.
//! A submit commits the human turn before inference, and the real configured
//! responder's authority is checked again before committing its answer.

use super::{blocking, decode, observed, private_scope, unchanged};
use crate::{
    chat_response_turns::{PendingTurn, TurnOwner, TurnRegistry},
    core::BackendStateUpdate,
    runtime::{BackendContext, BackendOutcome, RuntimeError, VerifiedCaller},
};
use anyhow::{Context, Result, ensure};
use op_cozo_store::{
    ChatConversationRecord, ChatResponseSnapshot, ChatTurnDraft, CozoError, CozoGraphShuttle,
};
use op_plugins::state_plugins::{
    chat_response::{ChatTurn, ConversationSummary, MessageType, ResponseContent},
    chat_response_methods::*,
    chat_router::{ChatInput, ChatMessage, ChatOutput},
    json_render::chat_canvas::ChatCanvasValidator,
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashMap},
    sync::{Arc, Mutex},
};

#[derive(Clone)]
struct PendingContinuation {
    request: PendingTurnRequest,
    generation: Option<(uuid::Uuid, ChatOutput)>,
    generating: bool,
    steps: u8,
    claimed: bool,
    actions: Vec<String>,
    evidence: Vec<Value>,
}
struct PendingGenerationAdmission {
    owner: VerifiedCaller,
    responder: VerifiedCaller,
    runtime: crate::runtime::Runtime,
    credential: String,
}

impl PendingGenerationAdmission {
    async fn for_turn(ctx: &BackendContext<'_>, owner: VerifiedCaller) -> Result<Arc<Self>> {
        // The FUSE caller is authenticated by Lungs, so it has no inbound
        // gRPC identity header. Forward its own authored credential to native
        // generation after revalidating the mounted caller.
        let wire = ctx.runtime.identity_wire(ctx.caller).await?;
        let credential = base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, wire);
        Ok(Arc::new(Self {
            owner,
            responder: ctx.caller.for_background(),
            runtime: ctx.runtime.clone(),
            credential,
        }))
    }
}

impl crate::chat_router_generation::NativeGenerationAdmission for PendingGenerationAdmission {
    fn sealed_identity(&self) -> Option<&str> { Some(&self.credential) }
    fn trace_id(&self) -> Option<&str> { Some(&self.owner.trace_id) }
    fn check(&self) -> std::pin::Pin<Box<dyn Future<Output =
        Result<(), crate::chat_router_generation::NativeGenerationError>> + Send + '_>> {
        Box::pin(async {
            use crate::chat_router_generation::{NativeGenerationError, NativeErrorCode};
            self.runtime.authorize_nested(&self.owner, "chat_router", "Chat").await
                .map_err(|_| NativeGenerationError::Terminal(NativeErrorCode::Auth))?;
            self.runtime.authorize_nested(&self.responder, "chat_response", "generate_pending_turn").await
                .map_err(|_| NativeGenerationError::Terminal(NativeErrorCode::Auth))
        })
    }
}

impl crate::chat_router_generation::NativeProviderAdmission for PendingGenerationAdmission {
    fn check_profile<'a>(&'a self, profile: &'a op_agents::inference::NativeProviderProfile)
        -> std::pin::Pin<Box<dyn Future<Output = Result<(), crate::chat_router_generation::NativeGenerationError>> + Send + 'a>> {
        Box::pin(async move {
            if profile.model.is_empty() || !crate::chat_router_catalog_binding::endpoint_matches(
                "https://api.typesafe.ai/v1/systemone", &profile.http_uri).unwrap_or(false) {
                return Err(crate::chat_router_generation::NativeGenerationError::Terminal(
                    crate::chat_router_generation::NativeErrorCode::Auth));
            }
            Ok(())
        })
    }
}

pub(super) struct ConversationBackend {
    pub(super) store: Arc<CozoGraphShuttle>,
    turns: TurnRegistry,
    owners: Mutex<HashMap<String, (VerifiedCaller, chrono::DateTime<chrono::Utc>)>>,
    operation: tokio::sync::Mutex<()>,
    continuations: Mutex<HashMap<String, PendingContinuation>>,
    committed: Mutex<HashMap<String, (VerifiedCaller, ChatTurn)>>,
}

impl ConversationBackend {
    pub(super) fn new(store: Arc<CozoGraphShuttle>) -> Self {
        Self {
            store,
            turns: TurnRegistry::default(),
            owners: Mutex::new(HashMap::new()),
            operation: tokio::sync::Mutex::new(()),
            committed: Mutex::new(HashMap::new()),
            continuations: Mutex::new(HashMap::new()),
        }
    }

    pub(super) async fn dispatch(
        &self,
        ctx: &BackendContext<'_>,
        method: &str,
        args: &Value,
    ) -> Result<BackendOutcome, RuntimeError> {
        let result = self.dispatch_inner(ctx, method, args).await;
        match result {
            Ok(outcome) => Ok(outcome),
            Err(error)
                if matches!(
                    method,
                    "submit_turn"
                        | "start_conversation"
                        | "respond_to_user"
                        | "request_clarification"
                        | "cannot_perform"
                ) =>
            {
                let state = self.snapshot(&ctx.caller.principal_id).await;
                let error = RuntimeError::from(error);
                match state {
                    Ok(state) => Err(error
                        .with_state(BackendStateUpdate::Fields(state), private_scope(ctx.caller))),
                    Err(_) => Err(error),
                }
            }
            Err(error) => Err(RuntimeError::from(error)),
        }
    }

    async fn dispatch_inner(
        &self,
        ctx: &BackendContext<'_>,
        method: &str,
        args: &Value,
    ) -> Result<BackendOutcome> {
        ctx.caller.authority.revalidate().await?;
        let result = match method {
            "start_conversation" => {
                let input: StartConversationInput = decode(args)?;
                let _guard = self.operation.lock().await;
                ctx.caller.authority.revalidate().await?;
                let caller = ctx.caller.clone();
                let store = self.store.clone();
                let (summary, snapshot) = blocking(move || {
                    ensure!(
                        store.count_principal_conversations(&caller.principal_id)? < 10000,
                        "conversation quota exceeded"
                    );
                    let summary = build_conversation(NewConversation {
                        principal_id: caller.principal_id,
                        session_id: caller.session_id,
                        session_genesis: caller.session_genesis,
                        conversation_id: uuid::Uuid::now_v7().to_string(),
                        title: input.title,
                        started_at: now(),
                    })?;
                    let record = ChatConversationRecord {
                        principal_id: summary.principal_id.clone(),
                        conversation_id: summary.conversation_id.clone(),
                        turn_count: 0,
                        head_turn_hash: summary.head_turn_hash.clone(),
                        revision: 0,
                        conversation: serde_json::to_value(&summary)?,
                    };
                    let (_, snapshot) =
                        store.insert_chat_conversation_and_snapshot(&record, 32, 64)?;
                    Ok((summary, snapshot))
                })
                .await
                .map_err(RuntimeError::unknown)?;
                return Ok(observed(
                    ctx,
                    serde_json::to_value(StartConversationOutput {
                        conversation_id: summary.conversation_id,
                        started_at: summary.started_at,
                    })?,
                    snapshot_state(snapshot, &ctx.caller.principal_id)?,
                ));
            }
            "submit_turn" => return self.submit(ctx, decode(args)?).await,
            "get_pending_turn" => {
                let input: GetPendingTurnInput = decode(args)?;
                let continuation = self.pending(ctx, &input.turn_id).await?;
                return Ok(unchanged(ctx, serde_json::to_value(continuation.request)?));
            }
            "generate_pending_turn" => return self.generate(ctx, decode(args)?).await,
            "propose_pending_tool" | "get_pending_tool" | "approve_pending_tool" | "consume_pending_tool" =>
                return self.pending_tool(ctx, method, args).await,
            "record_pending_tool" => return self.record_tool(ctx, decode(args)?).await,
            "claim_pending_turn" => {
                let input: GetPendingTurnInput = decode(args)?;
                self.pending(ctx, &input.turn_id).await?;
                let mut continuations = self.continuations.lock()
                    .map_err(|_| anyhow::anyhow!("pending continuation lock poisoned"))?;
                let current = continuations.get_mut(&input.turn_id).context("pending request absent")?;
                ensure!(!current.claimed, "turn processing already claimed; do not replay");
                current.claimed = true;
                return Ok(unchanged(ctx, serde_json::to_value(PendingToolOutput { proposal: None })?));
            }
            "get_response_commit" => {
                let input: GetPendingTurnInput = decode(args)?;
                let (owner, turn) = self.committed.lock()
                    .map_err(|_| anyhow::anyhow!("response commit lock poisoned"))?
                    .get(&input.turn_id).cloned().context("response commit absent")?;
                ensure!(turn.responder_principal_id == ctx.caller.principal_id,
                    "response commit belongs to another responder");
                owner.authority.revalidate().await?;
                let store = self.store.clone();
                let principal = owner.principal_id.clone();
                let turn_id = input.turn_id;
                let row = blocking(move || Ok(store.get_chat_turn(&principal, &turn_id)?))
                    .await?.context("committed response row is absent")?;
                let reread = decode_turn(row, &owner.principal_id)?;
                ensure!(reread.turn_hash == turn.turn_hash && reread.response_id == turn.response_id,
                    "durable response differs from the committed response receipt");
                return Ok(unchanged(ctx, serde_json::to_value(GetTurnResponseOutput { turn: Some(reread) })?));
            }
            "get_turn_response" => {
                let input: GetTurnResponseInput = decode(args)?;
                validate_uuid("turn_id", &input.turn_id)?;
                let principal = ctx.caller.principal_id.clone();
                let store = self.store.clone();
                let row =
                    blocking(move || Ok(store.get_chat_turn(&principal, &input.turn_id)?)).await?;
                let turn = row
                    .map(|row| decode_turn(row, &ctx.caller.principal_id))
                    .transpose()?;
                serde_json::to_value(GetTurnResponseOutput { turn })?
            }
            "list_conversations" => {
                let store = self.store.clone();
                let principal = ctx.caller.principal_id.clone();
                let page =
                    blocking(move || Ok(store.list_chat_conversations(&principal, 100)?)).await?;
                let conversations = page
                    .conversations
                    .iter()
                    .map(|row| decode_conversation(row, &ctx.caller.principal_id))
                    .collect::<Result<Vec<_>>>()?;
                serde_json::to_value(ListConversationsOutput {
                    conversations_truncated: page.conversation_count > conversations.len() as u64,
                    conversations,
                    conversation_count: page.conversation_count,
                    revision: page.revision,
                })?
            }
            "get_conversation" => {
                let input: GetConversationInput = decode(args)?;
                validate_uuid("conversation_id", &input.conversation_id)?;
                let store = self.store.clone();
                let principal = ctx.caller.principal_id.clone();
                let (row, page) = blocking(move || {
                    Ok(store.get_chat_conversation_with_turns(
                        &principal,
                        &input.conversation_id,
                        100,
                    )?)
                })
                .await?
                .context("conversation is absent from verified principal scope")?;
                let conversation = decode_conversation(&row, &ctx.caller.principal_id)?;
                let turns = page
                    .turns
                    .into_iter()
                    .map(|row| decode_turn(row, &ctx.caller.principal_id))
                    .collect::<Result<Vec<_>>>()?;
                let first_turn_index = turns
                    .first()
                    .map_or(conversation.turn_count, |turn| turn.turn_index);
                verify_turn_chain(&conversation, &turns, first_turn_index)?;
                serde_json::to_value(GetConversationOutput {
                    conversation,
                    turns,
                    first_turn_index,
                    turns_truncated: page.truncated,
                })?
            }
            "respond_to_user" | "request_clarification" | "cannot_perform" => {
                let (id, content) = match method {
                    "respond_to_user" => decode::<RespondToUserInput>(args)?.into_content()?,
                    "request_clarification" => {
                        decode::<RequestClarificationInput>(args)?.into_content()?
                    }
                    _ => decode::<CannotPerformInput>(args)?.into_content()?,
                };
                let _operation = self.operation.lock().await;
                if method == "respond_to_user" {
                    self.validate_generated_response(ctx, &id, &content).await?;
                }
                let claim = self.turns.claim(&id, &ctx.caller.principal_id)?;
                let (owner, _) = match self
                    .owners
                    .lock()
                    .map_err(|_| anyhow::anyhow!("pending chat owner lock poisoned"))?
                    .get(&id)
                    .cloned()
                    .context("pending chat owner admission is absent")
                {
                    Ok(owner) => owner,
                    Err(error) => {
                        claim.release();
                        return Err(error);
                    }
                };
                if let Err(error) = owner.authority.revalidate().await {
                    claim.release();
                    return Err(error.into());
                }
                if let Err(error) = ctx.caller.authority.revalidate().await {
                    claim.release();
                    return Err(error.into());
                }
                let canvas_hash = match canvas_hash(ctx, &content) {
                    Ok(hash) => hash,
                    Err(error) => {
                        claim.release();
                        return Err(error);
                    }
                };
                let episode_output = serde_json::to_string(&content).unwrap_or_default();
                let (turn, snapshot) = match self
                    .commit(
                        ctx,
                        claim.turn().clone(),
                        &ctx.caller.principal_id,
                        content,
                        canvas_hash,
                    )
                    .await
                {
                    Ok(committed) => committed,
                    Err(error) => {
                        claim.release();
                        return Err(error);
                    }
                };
                claim.answered();
                {
                    let mut committed = self.committed.lock()
                        .map_err(|_| anyhow::anyhow!("response commit lock poisoned"))?;
                    committed.retain(|_, (_, turn)| chrono::DateTime::parse_from_rfc3339(&turn.committed_at)
                        .is_ok_and(|at| at > chrono::Utc::now() - chrono::Duration::seconds(300)));
                    committed.insert(id.clone(), (owner.clone(), turn.clone()));
                }
                self.owners
                    .lock()
                    .map_err(|_| anyhow::anyhow!("pending chat owner lock poisoned"))?
                    .remove(&id);
                let revision = snapshot.revision;
                // The asking session, captured when the turn opened, is the only
                // authority for its private projection; never use response args.
                ctx.runtime
                    .publish_backend_state(
                        &owner,
                        "chat_response",
                        BackendStateUpdate::Fields(snapshot_state(snapshot, &owner.principal_id)?),
                        private_scope(&owner),
                    )
                    .await
                    .map_err(|error| RuntimeError::unknown(error.into()))?;
                // Task 27.4: a control-plane turn of the chatbot or a subsidiary is
                // also a ctl_plane_chatbot episode in the asking session's scope.
                // The answer is already committed; a failure here is logged only.
                if let Err(error) = self.commit_control_plane_episode(ctx, &id, &owner, episode_output).await {
                    tracing::warn!(%error, turn_id = %id, "control-plane chat episode not committed");
                }
                let result = if method == "respond_to_user" {
                    serde_json::to_value(RespondToUserOutput {
                        response_id: turn.response_id,
                        conversation_id: turn.conversation_id,
                        turn_index: turn.turn_index,
                        turn_hash: turn.turn_hash,
                        revision,
                        canvas_schema_hash: turn.canvas_schema_hash,
                    })?
                } else {
                    serde_json::to_value(ResponseCommitOutput {
                        response_id: turn.response_id,
                        conversation_id: turn.conversation_id,
                        turn_index: turn.turn_index,
                        turn_hash: turn.turn_hash,
                        revision,
                    })?
                };
                return Ok(unchanged(ctx, result));
            }
            "list_pending_turns" => {
                let pending_turns = self
                    .turns
                    .list_pending_for_responder(&ctx.caller.principal_id)
                    .into_iter()
                    .map(|turn| PendingTurnInfo {
                        turn_id: turn.request_id,
                        conversation_id: turn.context.conversation_id,
                        turn_index: turn.context.turn_index,
                        owner_key: principal_key(&turn.owner.principal_id),
                        owner_principal_id: turn.owner.principal_id,
                        opened_at: turn.opened_at.to_rfc3339(),
                    })
                    .collect::<Vec<_>>();
                serde_json::to_value(ListPendingTurnsOutput {
                    pending_count: pending_turns.len() as u64,
                    pending_turns,
                })?
            }
            _ => anyhow::bail!("unknown chat_response method {method}"),
        };
        ctx.caller.authority.revalidate().await?;
        Ok(observed(
            ctx,
            result,
            self.snapshot(&ctx.caller.principal_id).await?,
        ))
    }

    /// Owner rule (2026-11-22): the control-plane chatbot keeps its chat history
    /// in its own `ctl_plane_chatbot` plugin; a subsidiary uses the same plugin
    /// under `<subsidiary name>::ctrlplane-chat` and owns only that prefix.
    /// Only ControlPlane turns qualify, so personal content is never an episode.
    async fn commit_control_plane_episode(
        &self,
        ctx: &BackendContext<'_>,
        turn_id: &str,
        owner: &VerifiedCaller,
        decision_output: String,
    ) -> Result<()> {
        use op_plugins::state_plugins::ctl_plane_chatbot_methods::{
            CompletedChatEpisode, build_completed_chat_episode, owns_conversation,
            subsidiary_conversation_prefix,
        };
        let Some(continuation) = self.continuations.lock()
            .map_err(|_| anyhow::anyhow!("pending continuation lock poisoned"))?
            .get(turn_id).cloned() else { return Ok(()) };
        let request = continuation.request;
        if request.context.mode != op_plugins::state_plugins::project::ChatMode::ControlPlane {
            return Ok(());
        }
        let store = ctx.runtime.identity_store().clone();
        let session = owner.session_id.clone();
        let (kind, name) = blocking(move || crate::chat_mode_policy::session_identity(&store, &session)).await?;
        let conversation_id = match kind {
            crate::chat_mode_policy::SessionKind::Identity => return Ok(()),
            crate::chat_mode_policy::SessionKind::ControlPlaneChatbot => request.conversation_id.clone(),
            crate::chat_mode_policy::SessionKind::Subsidiary => {
                let name = name.context("subsidiary native instance name absent")?;
                let id = format!("{}::{}", subsidiary_conversation_prefix(&name)?, request.conversation_id);
                ensure!(owns_conversation(Some(&name), &id), "subsidiary conversation prefix not owned");
                id
            }
        };
        if kind == crate::chat_mode_policy::SessionKind::ControlPlaneChatbot {
            ensure!(owns_conversation(None, &conversation_id), "chatbot cannot write a subsidiary prefix");
        }
        let goal_text = request.messages.iter().rev()
            .find(|message| message.role == "user")
            .map(|message| message.content.clone());
        let now = chrono::Utc::now();
        let episode = build_completed_chat_episode(CompletedChatEpisode {
            scope: op_cozo_store::EpisodeScope {
                principal_id: owner.principal_id.clone(),
                session_id: owner.session_id.clone(),
                session_genesis: owner.session_genesis.clone(),
            },
            conversation_id: Some(conversation_id),
            goal_text,
            decision_output,
            tools: Vec::new(),
            started_at: now,
            ended_at: now,
        })?;
        ctx.runtime.chat().commit_episode(ctx.runtime, owner, episode).await?;
        Ok(())
    }

    async fn submit(
        &self,
        ctx: &BackendContext<'_>,
        input: SubmitTurnInput,
    ) -> Result<BackendOutcome> {
        validate_uuid("conversation_id", &input.conversation_id)?;
        ensure!(
            input.response_mode == "text" || input.response_mode == "application",
            "response_mode must be text or application"
        );
        validate_content(&ResponseContent::HumanMessage {
            message: input.message.clone(),
        })?;
        ctx.runtime
            .authorize_nested(ctx.caller, "chat_router", "Chat")
            .await?;
        let responder = ctx.runtime.configured_responder(ctx.caller).await?;
        ctx.runtime
            .authorize_nested(&responder, "chat_response", "respond_to_user")
            .await?;
        ensure!(
            responder.principal_id != ctx.caller.principal_id,
            "chat response requires a distinct registered responder"
        );
        // UI selections are captured from the verified asking session, never
        // from responder defaults or a mode/provider supplied by a model.
        let context = crate::runtime_workspace::project::current_chat_context(ctx).await?;
        ctx.runtime.authorize_nested(ctx.caller, "chat_router", "GetState").await?;
        let _selection = crate::chat_router_runtime::ChatRouterRuntimeClient::selection_guard().await;
        let native_agent_alias = super::chat_agent_alias(ctx, context.mode).await?;
        let client = super::native_client(ctx, &native_agent_alias).await?;
        let router_schema = super::sealed_schema(ctx, "chat_router")?;
        let observation = client.observe_config_and_state(&super::config_secrets(&router_schema)?).await?;
        let exact = client.current_exact_selection().await?;
        let provider = exact.provider;
        let model = exact.model;
        let native_config_digest = crate::core::BridgeCore::method_value_digest(&observation.native_config)?;
        ensure!(!provider.is_empty() && !model.is_empty(), "selected model is unconfirmed");
        let (human, pending, history) = {
            let _guard = self.operation.lock().await;
            ctx.caller.authority.revalidate().await?;
            responder.authority.revalidate().await?;
            let principal = ctx.caller.principal_id.clone();
            let conversation = input.conversation_id.clone();
            let store = self.store.clone();
            let (head, page) = blocking(move || {
                Ok(store.get_chat_conversation_with_turns(&principal, &conversation, 64)?)
            })
            .await?
            .context("conversation is absent from verified principal scope")?;
            let owner = turn_owner(ctx.caller);
            let pending = self.turns.open(
                owner.clone(),
                &input.conversation_id,
                head.turn_count
                    .checked_add(1)
                    .context("turn index exhausted")?,
                &responder.principal_id,
            )?;
            let human_pending =
                PendingTurn::for_human_message(owner, &input.conversation_id, head.turn_count);
            let committed = self
                .commit(
                    ctx,
                    human_pending,
                    &ctx.caller.principal_id,
                    ResponseContent::HumanMessage {
                        message: input.message.clone(),
                    },
                    None,
                )
                .await;
            let (human, snapshot) = match committed {
                Ok(value) => value,
                Err(error) => {
                    self.turns.cancel(pending.turn_id());
                    return Err(RuntimeError::unknown(error).into());
                }
            };
            {
                let mut owners = self
                    .owners
                    .lock()
                    .map_err(|_| anyhow::anyhow!("pending chat owner lock poisoned"))?;
                owners.retain(|_, (_, expires)| *expires > chrono::Utc::now());
                owners.insert(
                    pending.turn_id().to_owned(),
                    (ctx.caller.for_background(), pending.expires_at),
                );
            }
            ctx.runtime
                .publish_backend_state(
                    ctx.caller,
                    "chat_response",
                    BackendStateUpdate::Fields(snapshot_state(snapshot, &ctx.caller.principal_id)?),
                    private_scope(ctx.caller),
                )
                .await?;
            (human, pending, page.turns)
        };
        let mut messages = Vec::new();
        for row in history {
            let turn = decode_turn(row, &ctx.caller.principal_id)?;
            let role = if turn.content.is_human_message() {
                "user"
            } else {
                "assistant"
            };
            messages.push(ChatMessage {
                role: role.into(),
                content: serde_json::to_string(&turn.content)?,
            });
        }
        messages.push(ChatMessage {
            role: "user".into(),
            content: input.message,
        });
        let offered_operations = super::offered_pending_operations(ctx, &responder, &context).await?;
        self.continuations.lock().map_err(|_| anyhow::anyhow!("pending continuation lock poisoned"))?
            .insert(pending.turn_id().to_owned(), PendingContinuation {
                request: PendingTurnRequest {
                    turn_id: pending.turn_id().to_owned(),
                    conversation_id: input.conversation_id,
                    response_mode: input.response_mode,
                    context,
                    provider,
                    model,
                    native_agent_alias,
                    native_config_digest,
                    messages,
                    pending_tool: None,
                    offered_operations,
                },
                generation: None,
                generating: false,
                steps: 0,
                claimed: false,
                actions: Vec::new(),
                evidence: Vec::new(),
            });
        // The human message is already committed. A wake failure is an unknown
        // delivery outcome, never permission to replay submit_turn.
        crate::responder_wake::publish_responder_wake(
            crate::responder_wake::chat_turn_wake(&pending, None),
            responder.claims(),
        ).await.map_err(RuntimeError::unknown)?;
        Ok(unchanged(ctx, serde_json::to_value(SubmitTurnOutput {
            conversation_id: human.conversation_id,
            turn_id: human.turn_id,
            turn_index: human.turn_index,
            response_turn_id: pending.turn_id().to_owned(),
            response_turn_index: pending.turn_index(),
        })?))
    }

    async fn record_tool(&self, ctx: &BackendContext<'_>, input: RecordPendingToolInput)
        -> Result<BackendOutcome>
    {
        let continuation = self.pending(ctx, &input.turn_id).await?;
        let receipt: op_state_store::method_receipt::MethodExecutionReceipt =
            serde_json::from_value(input.receipt)?;
        receipt.validate_attribution(&receipt.plugin_id, &receipt.method)?;
        let path = format!("plugins/{}/operations/{}/call",
            _3tched_fs::namespace::encode_component(&receipt.plugin_id),
            _3tched_fs::namespace::encode_component(&receipt.method));
        ensure!(continuation.request.offered_operations.contains(&path), "tool was not offered for this turn");
        let (generation_id, generated) = continuation.generation.context("tool has no generation evidence")?;
        let generation_receipt = ctx.runtime.core().recover_operation(&generation_id.to_string(),
            &ctx.caller.principal_id, &ctx.caller.session_id)?;
        ensure!(generation_receipt.status == op_state_store::method_receipt::MethodRecoveryStatus::Completed
            && generation_receipt.terminal.as_ref().is_some_and(|terminal| receipt.event_id > terminal.event_id),
            "tool receipt predates this turn's generation");
        let ModelAction::Call { path: proposed, args } = serde_json::from_str(&generated.content)?
            else { anyhow::bail!("latest generation did not propose a tool call"); };
        ensure!(proposed == path && args == input.arguments, "tool differs from current generation proposal");
        let (schema, declaration) = ctx.runtime.pinned_method(&receipt.plugin_id, &receipt.method)?;
        if declaration.side_effect == op_state_store::SideEffect::Mutation {
            let proposal = continuation.request.pending_tool.context("mutation approval absent")?;
            ensure!(proposal.decision == "consumed" && proposal.plugin_id == receipt.plugin_id
                && proposal.method == receipt.method && proposal.arguments == input.arguments,
                "mutation differs from consumed human approval");
        }
        ctx.runtime.core().verify_operation_binding(&receipt.operation_id,
            &ctx.caller.principal_id, &ctx.caller.session_id, &receipt.plugin_id, &receipt.method,
            &schema.schema_hash, &input.arguments, None)?;
        ctx.runtime.core().verify_operation_result(&receipt.operation_id,
            &ctx.caller.principal_id, &ctx.caller.session_id, &receipt.result)?;
        let action = format!("{}.{}#{}", receipt.plugin_id, receipt.method, receipt.operation_id);
        let mut continuations = self.continuations.lock()
            .map_err(|_| anyhow::anyhow!("pending continuation lock poisoned"))?;
        let current = continuations.get_mut(&input.turn_id).context("pending request absent")?;
        ensure!(!current.actions.contains(&action), "tool receipt already recorded");
        current.actions.push(action);
        current.evidence.push(json!({"plugin":receipt.plugin_id,"method":receipt.method,
            "operation_id":receipt.operation_id,"result":receipt.result}));
        Ok(unchanged(ctx, serde_json::to_value(PendingToolOutput { proposal: None })?))
    }

    async fn pending_tool(&self, ctx: &BackendContext<'_>, method: &str, args: &Value)
        -> Result<BackendOutcome>
    {
        use sha2::{Digest, Sha256};
        let id = args["turn_id"].as_str().context("turn_id is required")?;
        validate_uuid("turn_id", id)?;
        if method == "propose_pending_tool" {
            let input: ProposePendingToolInput = decode(args)?;
            let continuation = self.pending(ctx, id).await?;
            let (_, generated) = continuation.generation.as_ref().context("tool has no generation evidence")?;
            let ModelAction::Call { path, args } = serde_json::from_str(&generated.content)?
                else { anyhow::bail!("latest generation did not propose a mutation"); };
            ensure!(path == format!("plugins/{}/operations/{}/call",
                _3tched_fs::namespace::encode_component(&input.plugin_id),
                _3tched_fs::namespace::encode_component(&input.method)) && args == input.arguments,
                "mutation proposal differs from completed generation");
            ensure!(!continuation.request.context.read_only, "selected context is read-only");
            ensure!(input.plugin_id != "chat_response", "model cannot operate its own response protocol");
            let (_, declaration) = ctx.runtime.pinned_method(&input.plugin_id, &input.method)?;
            ensure!(declaration.side_effect == op_state_store::SideEffect::Mutation,
                "read methods do not use mutation approval");
            crate::method_contract_admission::validate_admitted_arguments(&declaration, &input.arguments)?;
            let owner = self.owners.lock().map_err(|_| anyhow::anyhow!("pending owner lock poisoned"))?
                .get(id).cloned().context("pending owner absent")?.0;
            ctx.runtime.authorize_nested(&owner, &input.plugin_id, &input.method).await?;
            ctx.runtime.authorize_nested(ctx.caller, &input.plugin_id, &input.method).await?;
            let call = uuid::Uuid::now_v7().to_string();
            let input_text = serde_json::to_string(&input.arguments)?;
            let digest = format!("{:x}", Sha256::digest(serde_json::to_vec(&json!([
                "identity-chat-confirm/1", ctx.caller.principal_id, id, call,
                input.plugin_id, input.method, input_text
            ]))?));
            let proposal = PendingToolProposal {
                turn_id: id.to_owned(), tool_call_id: call,
                plugin_id: input.plugin_id, method: input.method, arguments: input.arguments,
                required_capability: declaration.required_capability.context("operation capability absent")?,
                digest, expires_at: (chrono::Utc::now() + chrono::Duration::seconds(300)).to_rfc3339(),
                decision: "pending".into(),
            };
            {
                let mut continuations = self.continuations.lock()
                    .map_err(|_| anyhow::anyhow!("pending continuation lock poisoned"))?;
                let request = &mut continuations.get_mut(id).context("pending request absent")?.request;
                ensure!(request.pending_tool.as_ref().is_none_or(|tool| tool.decision == "consumed" || tool.decision == "rejected"),
                    "an earlier tool proposal remains unsettled");
                request.pending_tool = Some(proposal.clone());
            }
            let key = crate::runtime_mqtt::owner_key_for_claims(owner.claims())?;
            op_plugins::state_plugins::emqx::dispatch_emqx_method("publish_wake",
                &json!({"owner_key": key, "seq": ctx.request_event_id})).await
                .map_err(RuntimeError::unknown)?;
            return Ok(unchanged(ctx, serde_json::to_value(proposal)?));
        }
        if method == "consume_pending_tool" {
            let continuation = self.pending(ctx, id).await?;
            let proposed = continuation.request.pending_tool.context("pending tool absent")?;
            let owner = self.owners.lock().map_err(|_| anyhow::anyhow!("pending owner lock poisoned"))?
                .get(id).cloned().context("pending owner absent")?.0;
            ctx.runtime.authorize_nested(&owner, &proposed.plugin_id, &proposed.method).await?;
            ctx.runtime.authorize_nested(ctx.caller, &proposed.plugin_id, &proposed.method).await?;
            let mut continuations = self.continuations.lock()
                .map_err(|_| anyhow::anyhow!("pending continuation lock poisoned"))?;
            let tool = continuations.get_mut(id).context("pending request absent")?
                .request.pending_tool.as_mut().context("pending tool absent")?;
            ensure!(chrono::DateTime::parse_from_rfc3339(&tool.expires_at)? > chrono::Utc::now(),
                "tool approval expired");
            ensure!(tool.decision == "approved", "tool approval is not available for consumption");
            let proposal = tool.clone();
            tool.decision = "consumed".into();
            return Ok(unchanged(ctx, serde_json::to_value(PendingToolOutput { proposal: Some(proposal) })?));
        }
        // Only the captured human owner can see or decide an approval. The
        // designated service/model cannot approve its own proposed mutation.
        if method == "get_pending_tool" && !self.owners.lock()
            .map_err(|_| anyhow::anyhow!("pending owner lock poisoned"))?.contains_key(id) {
            return Ok(unchanged(ctx, serde_json::to_value(PendingToolOutput { proposal: None })?));
        }
        let (owner, expires) = self.owners.lock().map_err(|_| anyhow::anyhow!("pending owner lock poisoned"))?
            .get(id).cloned().context("pending owner absent")?;
        ensure!(expires > chrono::Utc::now()
            && owner.principal_id == ctx.caller.principal_id
            && owner.session_id == ctx.caller.session_id
            && owner.session_genesis == ctx.caller.session_genesis,
            "approval belongs to another asking session");
        owner.authority.revalidate().await?;
        let mut continuations = self.continuations.lock()
            .map_err(|_| anyhow::anyhow!("pending continuation lock poisoned"))?;
        let tool = &mut continuations.get_mut(id).context("pending request absent")?.request.pending_tool;
        if method == "approve_pending_tool" {
            let input: ApprovePendingToolInput = decode(args)?;
            let proposal = tool.as_mut().context("pending tool absent")?;
            ensure!(proposal.decision == "pending"
                && proposal.tool_call_id == input.tool_call_id && proposal.digest == input.digest
                && chrono::DateTime::parse_from_rfc3339(&proposal.expires_at)? > chrono::Utc::now(),
                "approval is stale, mismatched, expired or already used");
            proposal.decision = if input.approved { "approved" } else { "rejected" }.into();
        }
        Ok(unchanged(ctx, serde_json::to_value(PendingToolOutput { proposal: tool.clone() })?))
    }

    async fn pending(&self, ctx: &BackendContext<'_>, id: &str) -> Result<PendingContinuation> {
        validate_uuid("turn_id", id)?;
        ensure!(self.turns.list_pending_for_responder(&ctx.caller.principal_id)
            .iter().any(|turn| turn.turn_id() == id),
            "turn is not pending for this verified responder");
        let (owner, expires) = self.owners.lock()
            .map_err(|_| anyhow::anyhow!("pending owner lock poisoned"))?
            .get(id).cloned().context("pending owner authority absent")?;
        ensure!(expires > chrono::Utc::now(), "pending turn expired");
        owner.authority.revalidate().await?;
        ctx.caller.authority.revalidate().await?;
        let continuation = self.continuations.lock()
            .map_err(|_| anyhow::anyhow!("pending continuation lock poisoned"))?
            .get(id).cloned().context("pending request is absent")?;
        let owner_ctx = BackendContext {
            runtime: ctx.runtime, caller: &owner, schema: ctx.schema,
            declaration: ctx.declaration, operation_id: ctx.operation_id,
            operation: ctx.operation, request_event_id: ctx.request_event_id,
            request_event_hash: ctx.request_event_hash.clone(), checkpoint: None,
            checkpoint_target: ctx.checkpoint_target,
        };
        let current = crate::runtime_workspace::project::current_chat_context(&owner_ctx).await?;
        let current_operations = super::offered_pending_operations(&owner_ctx, ctx.caller,
            &continuation.request.context).await?;
        ensure!(continuation.request.offered_operations.iter().all(|path| current_operations.contains(path)),
            "asking session or responder operation authority changed");
        ensure!(serde_json::to_value(current)? == serde_json::to_value(&continuation.request.context)?,
            "asking session context changed while the turn was pending; no generation replay");
        Ok(continuation)
    }

    async fn generate(&self, ctx: &BackendContext<'_>, input: GeneratePendingTurnInput)
        -> Result<BackendOutcome>
    {
        let continuation = self.pending(ctx, &input.turn_id).await?;
        let first_generation = {
            let mut pending = self.continuations.lock()
                .map_err(|_| anyhow::anyhow!("pending continuation lock poisoned"))?;
            let current = pending.get_mut(&input.turn_id).context("pending request disappeared")?;
            ensure!(current.claimed, "turn processing has not been claimed by the container");
            ensure!(!current.generating && current.steps < 32, "pending generation already started or exhausted");
            current.generating = true;
            current.steps += 1;
            current.steps == 1
        };
        let (owner, _) = self.owners.lock()
            .map_err(|_| anyhow::anyhow!("pending owner lock poisoned"))?
            .get(&input.turn_id).cloned().context("pending owner authority absent")?;
        ctx.runtime.authorize_nested(&owner, "chat_router", "Chat").await?;
        let admission = PendingGenerationAdmission::for_turn(ctx, owner).await?;
        let _selection = crate::chat_router_runtime::ChatRouterRuntimeClient::selection_guard().await;
        let alias = &continuation.request.native_agent_alias;
        let client = super::native_client(ctx, alias).await?;
        let schema = super::sealed_schema(ctx, "chat_router")?;
        let selection = crate::chat_router_runtime::ChatRouterRuntimeClient::for_agent_alias(alias)
            .with_sealed_identity(Some(super::original_payload(&admission.owner)?))
            .with_trace_id(&admission.owner.trace_id);
        let observation = selection.observe_config_and_state(&super::config_secrets(&schema)?)
            .await.inspect_err(|_| {
                tracing::warn!(operation_id = %ctx.operation_id, "could not verify the requesting container's native route");
            })?;
        let unchanged_route = crate::core::BridgeCore::method_value_digest(&observation.native_config)?
            == continuation.request.native_config_digest;
        if !unchanged_route {
            tracing::warn!(operation_id = %ctx.operation_id, "requesting container's native route changed before generation");
        }
        ensure!(unchanged_route, "selected native route changed while queued; generation refused without replay");
        let output = match client.complete_under_guard(ChatInput {
            messages: input.messages,
            provider: continuation.request.provider,
            model: continuation.request.model,
            conversation_id: Some(continuation.request.conversation_id),
            ..ChatInput::default()
        }, admission).await {
            Ok(output) => output,
            Err(error) => {
                use crate::chat_router_generation::NativeGenerationError;
                if first_generation && matches!(
                    error.downcast_ref::<NativeGenerationError>(),
                    Some(NativeGenerationError::IdentityAcknowledgmentMissing)
                ) {
                    // This handshake failed before the first prompt could be sent.
                    // Retain the saved human message and withdraw only its pending reply.
                    self.turns.cancel(&input.turn_id);
                    self.continuations.lock()
                        .map_err(|_| anyhow::anyhow!("pending continuation lock poisoned"))?
                        .remove(&input.turn_id);
                    self.owners.lock()
                        .map_err(|_| anyhow::anyhow!("pending owner lock poisoned"))?
                        .remove(&input.turn_id);
                    tracing::warn!(operation_id = %ctx.operation_id, "released reply after pre-prompt identity acknowledgment failure");
                }
                return Err(RuntimeError::unknown(error));
            }
        };
        // A failed/ambiguous call leaves generating=true. It must never silently
        // become a fresh model attempt when the wake is delivered twice.
        let mut pending = self.continuations.lock()
            .map_err(|_| anyhow::anyhow!("pending continuation lock poisoned"))?;
        let current = pending.get_mut(&input.turn_id).context("pending request disappeared")?;
        current.generation = Some((ctx.operation_id, output.clone()));
        current.generating = false;
        Ok(unchanged(ctx, serde_json::to_value(output)?))
    }

    async fn validate_generated_response(&self, ctx: &BackendContext<'_>, id: &str,
        content: &ResponseContent) -> Result<()>
    {
        let continuation = self.pending(ctx, id).await?;
        let (operation, generated) = continuation.generation
            .context("response has no completed generation evidence")?;
        let recovered = ctx.runtime.core().recover_operation(&operation.to_string(),
            &ctx.caller.principal_id, &ctx.caller.session_id)?;
        ensure!(recovered.status == op_state_store::method_receipt::MethodRecoveryStatus::Completed
            && recovered.plugin_id.as_deref() == Some("chat_response")
            && recovered.method.as_deref() == Some("generate_pending_turn"),
            "generation receipt is missing, mismatched or unconfirmed");
        ctx.runtime.core().verify_operation_result(&operation.to_string(),
            &ctx.caller.principal_id, &ctx.caller.session_id, &serde_json::to_value(&generated)?)?;
        let ModelAction::Message { message, canvas } = serde_json::from_str(&generated.content)?
            else { anyhow::bail!("generation did not propose a final message"); };
        let ResponseContent::UserResponse { message: actual, message_type, related_actions, canvas: actual_canvas } = content
            else { anyhow::bail!("generation evidence is not a user response"); };
        ensure!(&message == actual && &canvas == actual_canvas,
            "response differs from completed generation");
        ensure!(*message_type == MessageType::Info,
            "model prose cannot set authoritative operation status");
        ensure!(continuation.request.response_mode != "application" || canvas.is_some(),
            "application response omitted its sealed Canvas");
        ensure!(related_actions == &continuation.actions,
            "response action evidence does not equal this turn's recorded operations");
        for action in related_actions {
            let (method, operation) = action.rsplit_once('#').context("action has no operation receipt")?;
            let (plugin, method) = method.split_once('.').context("invalid action attribution")?;
            let receipt = ctx.runtime.core().recover_operation(operation,
                &ctx.caller.principal_id, &ctx.caller.session_id)?;
            ensure!(receipt.status == op_state_store::method_receipt::MethodRecoveryStatus::Completed
                && receipt.plugin_id.as_deref() == Some(plugin)
                && receipt.method.as_deref() == Some(method),
                "action receipt is missing, mismatched or unconfirmed");
        }
        match crate::container_ui::setting("CHAT_RESPONSE_JEV_REVIEW")?.as_deref() {
            None | Some("disabled") => {}
            Some("required") => {
                // Role alias, endpoint and key remain owned by the existing
                // native provider configuration. No model/endpoint override.
                let (owner, _) = self.owners.lock()
                    .map_err(|_| anyhow::anyhow!("pending owner lock poisoned"))?
                    .get(id).cloned().context("pending owner absent")?;
                let question = op_agents::judgment::checkpoint_request(
                    "Answer the user's captured request without unsupported operation, status or state claims.",
                    &serde_json::to_value(&continuation.request)?,
                    Some(&json!({"proposed_response":content, "recorded_actions":continuation.actions,
                        "operation_results":continuation.evidence, "generation_operation":operation.to_string()})),
                )?;
                let bytes = serde_json::to_vec(&question)?;
                ensure!(crate::cockpit::secret_guard::SecretGuard::new().check_item(&bytes).is_clean(),
                    "message review withheld credential-bearing context");
                let admission = PendingGenerationAdmission::for_turn(ctx, owner).await?;
                let _selection = crate::chat_router_runtime::ChatRouterRuntimeClient::selection_guard().await;
                let schema = super::sealed_schema(ctx, "chat_router")?;
                let reviewer = super::native_client(ctx, "judgment").await?;
                let review = reviewer.judge_under_guard(question.clone(), admission,
                    &super::config_secrets(&schema)?).await
                    .map_err(|_| anyhow::anyhow!("required Jev message review unavailable; response withheld"))?;
                ensure!(matches!(op_agents::judgment::checkpoint_decision(&question, Some(&review)),
                    op_agents::judgment::SelectionOutcome::Selected { .. }),
                    "Jev message review rejected or requires human review; response withheld");
            }
            _ => anyhow::bail!("invalid CHAT_RESPONSE_JEV_REVIEW policy"),
        }
        Ok(())
    }

    async fn snapshot(&self, principal: &str) -> Result<Value> {
        let store = self.store.clone();
        let principal = principal.to_owned();
        blocking(move || {
            snapshot_state(
                store.snapshot_chat_responses(&principal, 32, 64)?,
                &principal,
            )
        })
        .await
    }

    async fn commit(
        &self,
        ctx: &BackendContext<'_>,
        pending: PendingTurn,
        author: &str,
        content: ResponseContent,
        canvas_schema_hash: Option<String>,
    ) -> Result<(ChatTurn, ChatResponseSnapshot)> {
        let store = self.store.clone();
        let author = author.to_owned();
        let event_id = ctx.request_event_id;
        let event_hash = ctx.request_event_hash.clone();
        blocking(move || {
            let response_id = uuid::Uuid::now_v7().to_string();
            let committed_at = now();
            let draft = |head: &ChatConversationRecord,
                         _revision|
             -> std::result::Result<ChatTurnDraft, CozoError> {
                let build = || -> Result<ChatTurnDraft> {
                    let mut summary = decode_conversation(head, &pending.owner.principal_id)?;
                    let turn = build_turn(NewTurn {
                        principal_id: pending.owner.principal_id.clone(),
                        session_id: pending.owner.session_id.clone(),
                        session_genesis: pending.owner.session_genesis.clone(),
                        conversation_id: pending.conversation_id().to_owned(),
                        turn_id: pending.turn_id().to_owned(),
                        turn_index: head.turn_count,
                        prev_turn_hash: head.head_turn_hash.clone(),
                        response_id: response_id.clone(),
                        responder_principal_id: author.clone(),
                        opened_at: pending.opened_at.to_rfc3339(),
                        committed_at: committed_at.clone(),
                        request_event_id: event_id,
                        request_event_hash: event_hash.clone(),
                        canvas_schema_hash: canvas_schema_hash.clone(),
                        content: content.clone(),
                    })?;
                    summary.turn_count = head
                        .turn_count
                        .checked_add(1)
                        .context("turn index exhausted")?;
                    summary.head_turn_hash = turn.turn_hash.clone();
                    summary.updated_at = committed_at.clone();
                    validate_conversation(&summary)?;
                    Ok(ChatTurnDraft {
                        turn_hash: turn.turn_hash.clone(),
                        content_hash: turn.content_hash.clone(),
                        turn: serde_json::to_value(turn)?,
                        conversation: serde_json::to_value(summary)?,
                    })
                };
                build().map_err(|error| CozoError::Other(error.to_string()))
            };
            let (turn, snapshot) = store.insert_chat_turn_and_snapshot(
                &pending.owner.principal_id,
                pending.conversation_id(),
                pending.turn_id(),
                pending.turn_index(),
                &draft,
                32,
                64,
            )?;
            Ok((decode_turn(turn, &pending.owner.principal_id)?, snapshot))
        })
        .await
    }
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum ModelAction {
    Describe {
        path: String,
    },
    Call {
        path: String,
        args: Value,
    },
    Message {
        message: String,
        #[serde(default)]
        canvas: Option<serde_json::Map<String, Value>>,
    },
}


fn canvas_hash(ctx: &BackendContext<'_>, content: &ResponseContent) -> Result<Option<String>> {
    let Some(canvas) = content.canvas() else {
        return Ok(None);
    };
    let renderer = pinned_renderer(ctx)?;
    let validator = ChatCanvasValidator::from_pinned_schema_json(
        renderer
            .sealed_schema_json
            .as_deref()
            .context("sealed renderer bytes are absent")?,
    )?;
    validator
        .validate(&Value::Object(canvas.clone()))
        .map_err(|error| anyhow::anyhow!("canvas rejected by sealed renderer: {error}"))?;
    Ok(Some(renderer.schema_hash))
}
fn pinned_renderer(ctx: &BackendContext<'_>) -> Result<crate::catalog::PluginEntry> {
    let renderer =
        crate::catalog::Catalog::sealed_plugin(ctx.runtime.catalog_dir(), "json_render")?;
    ensure!(
        ctx.runtime
            .catalog()
            .plugin("json_render")
            .map(|entry| entry.schema_hash.as_str())
            == Some(renderer.schema_hash.as_str()),
        "sealed renderer changed during the conversation"
    );
    Ok(renderer)
}
fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}
fn turn_owner(caller: &VerifiedCaller) -> TurnOwner {
    TurnOwner {
        principal_id: caller.principal_id.clone(),
        session_id: caller.session_id.clone(),
        session_genesis: caller.session_genesis.clone(),
    }
}
fn principal_key(principal: &str) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(principal.as_bytes()))
}
fn decode_conversation(
    row: &ChatConversationRecord,
    principal: &str,
) -> Result<ConversationSummary> {
    let summary: ConversationSummary = serde_json::from_value(row.conversation.clone())?;
    validate_conversation(&summary)?;
    ensure!(
        row.principal_id == principal
            && summary.principal_id == principal
            && summary.conversation_id == row.conversation_id
            && summary.turn_count == row.turn_count
            && summary.head_turn_hash == row.head_turn_hash,
        "conversation document differs from its durable principal key"
    );
    Ok(summary)
}
fn decode_turn(row: op_cozo_store::ChatTurnRecord, principal: &str) -> Result<ChatTurn> {
    let turn: ChatTurn = serde_json::from_value(row.turn)?;
    validate_turn(&turn)?;
    ensure!(
        row.principal_id == principal
            && turn.principal_id == principal
            && turn.turn_id == row.turn_id
            && turn.turn_index == row.turn_index
            && turn.turn_hash == row.turn_hash,
        "turn document differs from its durable principal key"
    );
    Ok(turn)
}
fn snapshot_state(snapshot: ChatResponseSnapshot, principal: &str) -> Result<Value> {
    let conversations = snapshot
        .conversations
        .iter()
        .map(|row| {
            Ok((
                row.conversation_id.clone(),
                decode_conversation(row, principal)?,
            ))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    let turns = snapshot
        .turns
        .into_iter()
        .map(|row| {
            let turn = decode_turn(row, principal)?;
            Ok((turn.turn_id.clone(), turn))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    Ok(
        json!({"conversations_truncated": snapshot.conversation_count > conversations.len() as u64, "conversations": conversations, "conversation_count": snapshot.conversation_count, "turns_truncated": snapshot.turn_count > turns.len() as u64, "turns": turns, "turn_count": snapshot.turn_count, "revision": snapshot.revision}),
    )
}
