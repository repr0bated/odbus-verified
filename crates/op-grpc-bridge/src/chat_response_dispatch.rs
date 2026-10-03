//! Canonical bridge dispatch for `chat_response` (OD-90 stage 1).
//!
//! Humans start and read conversations in their own principal scope
//! (`chat_response.read`). Agents answer bridge-minted turns
//! (`chat_response.respond`): a response is admitted only for an open,
//! unexpired turn whose designated responder is the verified caller, and it
//! commits into the asking human's scope, never the agent's. Each commit
//! extends the conversation's public turn-hash chain and is announced by a
//! content-free `response_committed` signal.

use anyhow::{Context, Result};
use op_cozo_store::{ChatConversationRecord, ChatTurnDraft, CozoError, EpisodeScope};
use op_plugins::state_plugins::{
    chat_response::{
        ChatTurn, ConversationSummary, RESPOND_CAPABILITY, RESPONSE_COMMITTED_SIGNAL,
        ResponseCommittedSignal, ResponseContent,
    },
    chat_response_methods::{
        CannotPerformInput, GetConversationInput, GetConversationOutput, GetTurnResponseInput,
        GetTurnResponseOutput, ListConversationsInput, ListConversationsOutput, NewConversation,
        NewTurn, RequestClarificationInput, RespondToUserInput, RespondToUserOutput,
        ResponseCommitOutput, StartConversationInput, StartConversationOutput, build_conversation,
        build_turn, validate_conversation, validate_title, validate_turn, validate_uuid,
        verify_turn_chain,
    },
    json_render::canvas_profile::PinnedCanvasValidator,
};
use op_state_store::PluginSchema;

use crate::{
    chat_response_state::{
        CONVERSATION_SNAPSHOT_LIMIT, DoorbellWake, PLUGIN, TURN_SNAPSHOT_LIMIT, owner_key,
    },
    chat_response_turns::{TurnClaim, TurnOwner},
    interceptor::load_capability_grants,
    method_contract_admission,
    mutation_engine::{ChangeSource, ChangeType, MutationEngine},
    project_dispatch,
    sealed_schema_reader::{blob_catalog_dir_from_env, read_manifest_pinned_schema_document},
};

#[cfg(test)]
#[path = "chat_response_tests.rs"]
mod tests;

/// Conversations returned by `list_conversations` (most recently active).
const LIST_LIMIT: usize = 100;
/// Turns returned by `get_conversation` (newest window, ascending).
const TURN_PAGE_LIMIT: usize = 100;
const RENDERER: &str = "json_render";

/// The admitted method call's own global chain event, recorded before
/// dispatch. A committed turn keeps it as its link into the global chain.
pub(crate) struct RequestEvent {
    pub event_id: u64,
    pub event_hash: String,
}

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
        "admitted chat_response schema differs from the current sealed plugin"
    );
    Ok(SealPin {
        schema_hash: pinned.schema_hash,
        schema_json: pinned.blob.schema_json,
    })
}

fn seal_is_unchanged(expected: &SealPin) -> Result<()> {
    let current = read_manifest_pinned_schema_document(&blob_catalog_dir_from_env(), PLUGIN)?;
    method_contract_admission::admit_compiled_state_contract(&current, PLUGIN)?;
    anyhow::ensure!(
        current.schema_hash == expected.schema_hash
            && current.blob.schema_json == expected.schema_json,
        "chat_response plugin seal changed during operation"
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
        .context("admitted chat_response method has no exact capability")
}

async fn blocking<T: Send + 'static>(
    operation: impl FnOnce() -> Result<T> + Send + 'static,
) -> Result<T> {
    tokio::task::spawn_blocking(operation)
        .await
        .context("chat_response worker failed")?
}

/// The caller's session, current exact grant and the pinned seal, rechecked
/// at each execution boundary.
async fn revalidate_boundary(
    engine: &MutationEngine,
    method: &str,
    caller: &TurnOwner,
    admitted: &PluginSchema,
    pin: &SealPin,
) -> Result<()> {
    project_dispatch::verified_session(
        engine,
        &caller.principal_id,
        Some(&caller.session_id),
        Some(&caller.session_genesis),
    )
    .await?;
    let required = required_capability(admitted, method)?;
    anyhow::ensure!(
        load_capability_grants(&caller.principal_id).contains(required),
        "current exact {required} grant is required"
    );
    let expected = pin.clone();
    blocking(move || seal_is_unchanged(&expected)).await
}

/// The asking human's session must still be valid when their turn is
/// answered; a revoked or expired session receives nothing.
async fn verify_owner(engine: &MutationEngine, owner: &TurnOwner) -> Result<()> {
    project_dispatch::verified_session(
        engine,
        &owner.principal_id,
        Some(&owner.session_id),
        Some(&owner.session_genesis),
    )
    .await
    .context("the session that asked this turn is no longer verified")?;
    Ok(())
}

fn validate_output(schema: &PluginSchema, method: &str, output: &serde_json::Value) -> Result<()> {
    let declaration = schema
        .methods
        .get(method)
        .with_context(|| format!("admitted {PLUGIN}.{method} declaration is absent"))?;
    method_contract_admission::validate_admitted_output(declaration, output)
        .context("chat_response output does not conform to the admitted sealed contract")
}

/// Validate a Canvas with the sealed json_render profile, returning the
/// renderer schema hash it was validated against.
async fn validate_canvas(canvas: serde_json::Map<String, serde_json::Value>) -> Result<String> {
    blocking(move || {
        let renderer = read_manifest_pinned_schema_document(&blob_catalog_dir_from_env(), RENDERER)
            .context("json_render is absent from the sealed catalog; a canvas cannot be validated")?;
        method_contract_admission::admit_compiled_state_contract(&renderer, RENDERER)?;
        let validator = PinnedCanvasValidator::from_manifest_pinned_schema(
            renderer.blob.schema_json.clone().into(),
            renderer.schema_hash.clone(),
        )?;
        validator
            .validate(&serde_json::Value::Object(canvas))
            .map_err(|error| {
                anyhow::anyhow!(
                    "canvas rejected by the sealed json_render Canvas profile: {error}; the response was not committed"
                )
            })?;
        Ok(renderer.schema_hash)
    })
    .await
}

fn decode_conversation(
    row: &ChatConversationRecord,
    principal_id: &str,
) -> Result<ConversationSummary> {
    let summary: ConversationSummary = serde_json::from_value(row.conversation.clone())
        .context("durable conversation is not the typed contract")?;
    validate_conversation(&summary)?;
    anyhow::ensure!(
        row.principal_id == principal_id
            && summary.principal_id == principal_id
            && summary.conversation_id == row.conversation_id
            && summary.turn_count == row.turn_count
            && summary.head_turn_hash == row.head_turn_hash,
        "durable conversation differs from its principal key"
    );
    Ok(summary)
}

fn decode_turn(row: &op_cozo_store::ChatTurnRecord, principal_id: &str) -> Result<ChatTurn> {
    let turn: ChatTurn = serde_json::from_value(row.turn.clone())
        .context("durable turn is not the typed contract")?;
    validate_turn(&turn)?;
    anyhow::ensure!(
        row.principal_id == principal_id
            && turn.principal_id == principal_id
            && turn.turn_id == row.turn_id
            && turn.turn_index == row.turn_index
            && turn.turn_hash == row.turn_hash,
        "durable turn differs from its principal key"
    );
    Ok(turn)
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
    request: RequestEvent,
) -> Result<serde_json::Value> {
    let engine = engine.clone();
    let method = method.to_string();
    let args = args.clone();
    let actor = actor.to_string();
    let session = session.map(str::to_string);
    let genesis = genesis.map(str::to_string);
    let admitted = admitted.clone();
    // The task owns the turn claim, durable commit, SHM publication, wake and
    // chain announcement even if the requesting transport is cancelled.
    tokio::spawn(async move {
        dispatch_owned(
            &engine,
            &method,
            &args,
            &actor,
            session.as_deref(),
            genesis.as_deref(),
            &admitted,
            request,
        )
        .await
    })
    .await
    .context("chat_response operation owner failed")?
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
    request: RequestEvent,
) -> Result<serde_json::Value> {
    let verified = project_dispatch::verified_session(engine, actor, session, genesis).await?;
    let caller = TurnOwner {
        principal_id: actor.to_owned(),
        session_id: verified,
        session_genesis: genesis
            .context("verified chat_response session genesis is required")?
            .to_owned(),
    };
    let pin = blocking({
        let admitted = admitted.clone();
        move || pin_contract(&admitted)
    })
    .await?;
    revalidate_boundary(engine, method, &caller, admitted, &pin).await?;
    let output = match method {
        "start_conversation" => {
            let input: StartConversationInput = serde_json::from_value(args.clone())
                .context("invalid start_conversation arguments")?;
            serde_json::to_value(
                start_conversation(engine, &caller, admitted, &pin, method, input).await?,
            )?
        }
        "respond_to_user" => {
            let input: RespondToUserInput = serde_json::from_value(args.clone())
                .context("invalid respond_to_user arguments")?;
            let (turn_id, content) = input.into_content()?;
            // An invalid canvas rejects the whole response before any claim.
            let canvas_schema_hash = match content.canvas() {
                Some(canvas) => Some(validate_canvas(canvas.clone()).await?),
                None => None,
            };
            let (turn, revision) = commit_response(
                engine,
                &caller,
                admitted,
                &pin,
                method,
                &turn_id,
                content,
                canvas_schema_hash,
                request,
            )
            .await?;
            serde_json::to_value(RespondToUserOutput {
                response_id: turn.response_id,
                conversation_id: turn.conversation_id,
                turn_index: turn.turn_index,
                turn_hash: turn.turn_hash,
                revision,
                canvas_schema_hash: turn.canvas_schema_hash,
            })?
        }
        "request_clarification" | "cannot_perform" => {
            let (turn_id, content) = if method == "request_clarification" {
                serde_json::from_value::<RequestClarificationInput>(args.clone())
                    .context("invalid request_clarification arguments")?
                    .into_content()?
            } else {
                serde_json::from_value::<CannotPerformInput>(args.clone())
                    .context("invalid cannot_perform arguments")?
                    .into_content()?
            };
            let (turn, revision) = commit_response(
                engine, &caller, admitted, &pin, method, &turn_id, content, None, request,
            )
            .await?;
            serde_json::to_value(ResponseCommitOutput {
                response_id: turn.response_id,
                conversation_id: turn.conversation_id,
                turn_index: turn.turn_index,
                turn_hash: turn.turn_hash,
                revision,
            })?
        }
        "get_turn_response" => {
            let input: GetTurnResponseInput = serde_json::from_value(args.clone())
                .context("invalid get_turn_response arguments")?;
            validate_uuid("turn_id", &input.turn_id)?;
            let store = engine.chat_response_cozo().await?;
            let principal = caller.principal_id.clone();
            let row = blocking(move || {
                store
                    .get_chat_turn(&principal, &input.turn_id)
                    .map_err(Into::into)
            })
            .await?;
            let turn = row
                .as_ref()
                .map(|row| decode_turn(row, &caller.principal_id))
                .transpose()?;
            revalidate_boundary(engine, method, &caller, admitted, &pin).await?;
            serde_json::to_value(GetTurnResponseOutput { turn })?
        }
        "list_conversations" => {
            let _: ListConversationsInput = serde_json::from_value(args.clone())
                .context("invalid list_conversations arguments")?;
            let store = engine.chat_response_cozo().await?;
            engine
                .chat_response_publisher()?
                .recover(store.clone(), &caller.principal_id)
                .await?;
            let principal = caller.principal_id.clone();
            let listed = blocking(move || {
                store
                    .list_chat_conversations(&principal, LIST_LIMIT)
                    .map_err(Into::into)
            })
            .await?;
            let conversations = listed
                .conversations
                .iter()
                .map(|row| decode_conversation(row, &caller.principal_id))
                .collect::<Result<Vec<_>>>()?;
            revalidate_boundary(engine, method, &caller, admitted, &pin).await?;
            serde_json::to_value(ListConversationsOutput {
                conversations_truncated: listed.conversation_count > conversations.len() as u64,
                conversations,
                conversation_count: listed.conversation_count,
                revision: listed.revision,
            })?
        }
        "get_conversation" => {
            let input: GetConversationInput = serde_json::from_value(args.clone())
                .context("invalid get_conversation arguments")?;
            validate_uuid("conversation_id", &input.conversation_id)?;
            let store = engine.chat_response_cozo().await?;
            engine
                .chat_response_publisher()?
                .recover(store.clone(), &caller.principal_id)
                .await?;
            let principal = caller.principal_id.clone();
            let found = blocking(move || {
                store
                    .get_chat_conversation_with_turns(
                        &principal,
                        &input.conversation_id,
                        TURN_PAGE_LIMIT,
                    )
                    .map_err(Into::into)
            })
            .await?
            .context("conversation does not exist in the verified principal scope")?;
            let (row, page) = found;
            let conversation = decode_conversation(&row, &caller.principal_id)?;
            let turns = page
                .turns
                .iter()
                .map(|row| decode_turn(row, &caller.principal_id))
                .collect::<Result<Vec<_>>>()?;
            let first_turn_index = turns
                .first()
                .map_or(conversation.turn_count, |turn| turn.turn_index);
            verify_turn_chain(&conversation, &turns, first_turn_index)?;
            revalidate_boundary(engine, method, &caller, admitted, &pin).await?;
            serde_json::to_value(GetConversationOutput {
                conversation,
                turns,
                turns_truncated: page.truncated,
                first_turn_index,
            })?
        }
        _ => anyhow::bail!("unsupported {PLUGIN} method: {method}"),
    };
    validate_output(admitted, method, &output)?;
    Ok(output)
}

async fn start_conversation(
    engine: &MutationEngine,
    caller: &TurnOwner,
    admitted: &PluginSchema,
    pin: &SealPin,
    method: &str,
    input: StartConversationInput,
) -> Result<StartConversationOutput> {
    validate_title(input.title.as_deref())?;
    // The bridge mints the id; UUIDv7 orders conversations by start time.
    let conversation_id = uuid::Uuid::now_v7().to_string();
    let started_at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let summary = build_conversation(NewConversation {
        principal_id: caller.principal_id.clone(),
        session_id: caller.session_id.clone(),
        session_genesis: caller.session_genesis.clone(),
        conversation_id: conversation_id.clone(),
        title: input.title,
        started_at: started_at.clone(),
    })?;
    let record = ChatConversationRecord {
        principal_id: summary.principal_id.clone(),
        conversation_id: summary.conversation_id.clone(),
        turn_count: 0,
        head_turn_hash: summary.head_turn_hash.clone(),
        revision: 0,
        conversation: serde_json::to_value(&summary)?,
    };
    let store = engine.chat_response_cozo().await?;
    let publisher = engine.chat_response_publisher()?.clone();
    let effect = publisher.admit(store.clone(), &caller.principal_id).await?;
    revalidate_boundary(engine, method, caller, admitted, pin).await?;
    publisher
        .commit(effect, move || {
            store
                .insert_chat_conversation_and_snapshot(
                    &record,
                    CONVERSATION_SNAPSHOT_LIMIT,
                    TURN_SNAPSHOT_LIMIT,
                )
                .map_err(Into::into)
        })
        .await?;
    Ok(StartConversationOutput {
        conversation_id,
        started_at,
    })
}

/// Claim, verify and commit one response. Returns the committed turn and
/// the owner's scope revision.
#[allow(clippy::too_many_arguments)]
async fn commit_response(
    engine: &MutationEngine,
    caller: &TurnOwner,
    admitted: &PluginSchema,
    pin: &SealPin,
    method: &str,
    turn_id: &str,
    content: ResponseContent,
    canvas_schema_hash: Option<String>,
    request: RequestEvent,
) -> Result<(ChatTurn, u64)> {
    let claim = engine.chat_turns().claim(turn_id, &caller.principal_id)?;
    verify_owner(engine, &claim.turn().owner).await?;
    let store = engine.chat_response_cozo().await?;
    let result = commit_claimed(
        engine,
        &claim,
        caller,
        admitted,
        pin,
        method,
        store.clone(),
        content,
        canvas_schema_hash,
        request,
    )
    .await;
    match result {
        Ok(committed) => {
            claim.answered();
            Ok(committed)
        }
        Err(error) => {
            // A durable row means the turn is answered even though a later
            // step failed; otherwise the designated responder may retry.
            let owner = claim.turn().owner.principal_id.clone();
            let id = claim.turn().turn_id.clone();
            let answered = blocking(move || store.get_chat_turn(&owner, &id).map_err(Into::into))
                .await
                .map(|row| row.is_some())
                .unwrap_or(false);
            if answered {
                claim.answered();
            } else {
                claim.release();
            }
            Err(error)
        }
    }
}

/// The responding agent's method boundary, revalidated under the owner's lock.
struct AgentBoundary<'a> {
    method: &'a str,
    caller: &'a TurnOwner,
    admitted: &'a PluginSchema,
    pin: &'a SealPin,
}

#[allow(clippy::too_many_arguments)]
async fn commit_claimed(
    engine: &MutationEngine,
    claim: &TurnClaim,
    caller: &TurnOwner,
    admitted: &PluginSchema,
    pin: &SealPin,
    method: &str,
    store: std::sync::Arc<op_cozo_store::CozoGraphShuttle>,
    content: ResponseContent,
    canvas_schema_hash: Option<String>,
    request: RequestEvent,
) -> Result<(ChatTurn, u64)> {
    let boundary = AgentBoundary {
        method,
        caller,
        admitted,
        pin,
    };
    commit_turn(
        engine,
        claim.turn().clone(),
        &caller.principal_id,
        Some(boundary),
        store,
        content,
        canvas_schema_hash,
        request,
    )
    .await
}

/// Record a verified human's message as the next turn of their conversation,
/// authored by that human, so the conversation chain holds both sides. The
/// door's chat operation calls this through `open_chat_turn`, with its own
/// admitted chat request as the turn's chain event.
pub(crate) async fn commit_human_turn(
    engine: &MutationEngine,
    owner: &TurnOwner,
    conversation_id: &str,
    message: String,
    request: RequestEvent,
) -> Result<ChatTurn> {
    let content = ResponseContent::HumanMessage { message };
    op_plugins::state_plugins::chat_response_methods::validate_content(&content)?;
    let store = engine.chat_response_cozo().await?;
    let head_store = store.clone();
    let principal = owner.principal_id.clone();
    let conversation = conversation_id.to_string();
    let head = blocking(move || {
        head_store
            .get_chat_conversation(&principal, &conversation)?
            .context("conversation does not exist in the asking principal's scope")
    })
    .await?;
    let now = chrono::Utc::now();
    let pending = crate::chat_response_turns::PendingTurn {
        turn_id: uuid::Uuid::now_v7().to_string(),
        owner: owner.clone(),
        conversation_id: conversation_id.to_string(),
        turn_index: head.turn_count,
        responder_principal_id: owner.principal_id.clone(),
        opened_at: now,
        expires_at: now,
    };
    let (turn, _revision) = commit_turn(
        engine,
        pending,
        &owner.principal_id,
        None,
        store,
        content,
        None,
        request,
    )
    .await?;
    Ok(turn)
}

/// Commit one turn authored by `author` at the conversation head: an agent's
/// response (with its method boundary) or a human's message (without one).
#[allow(clippy::too_many_arguments)]
async fn commit_turn(
    engine: &MutationEngine,
    pending: crate::chat_response_turns::PendingTurn,
    author: &str,
    boundary: Option<AgentBoundary<'_>>,
    store: std::sync::Arc<op_cozo_store::CozoGraphShuttle>,
    content: ResponseContent,
    canvas_schema_hash: Option<String>,
    request: RequestEvent,
) -> Result<(ChatTurn, u64)> {
    let owner = pending.owner.clone();
    let publisher = engine.chat_response_publisher()?.clone();
    let effect = publisher.admit(store.clone(), &owner.principal_id).await?;
    // Under the owner's lock: a responder's session, grant and seal, and the
    // asking session, are all still current.
    if let Some(boundary) = &boundary {
        revalidate_boundary(
            engine,
            boundary.method,
            boundary.caller,
            boundary.admitted,
            boundary.pin,
        )
        .await?;
    }
    verify_owner(engine, &owner).await?;

    let response_id = uuid::Uuid::now_v7().to_string();
    let committed_at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let make_turn = {
        let pending = pending.clone();
        let responder = author.to_string();
        let request_hash = request.event_hash.clone();
        let request_id = request.event_id;
        move |head: &ChatConversationRecord| -> Result<(ChatTurn, ConversationSummary)> {
            let mut summary = decode_conversation(head, &pending.owner.principal_id)?;
            anyhow::ensure!(
                head.turn_count == pending.turn_index,
                "chat turn is no longer the conversation head"
            );
            let turn = build_turn(NewTurn {
                principal_id: pending.owner.principal_id.clone(),
                session_id: pending.owner.session_id.clone(),
                session_genesis: pending.owner.session_genesis.clone(),
                conversation_id: pending.conversation_id.clone(),
                turn_id: pending.turn_id.clone(),
                turn_index: head.turn_count,
                prev_turn_hash: head.head_turn_hash.clone(),
                response_id: response_id.clone(),
                responder_principal_id: responder.clone(),
                opened_at: pending
                    .opened_at
                    .to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
                committed_at: committed_at.clone(),
                request_event_id: request_id,
                request_event_hash: request_hash.clone(),
                canvas_schema_hash: canvas_schema_hash.clone(),
                content: content.clone(),
            })?;
            summary.turn_count = head.turn_count + 1;
            summary.head_turn_hash = turn.turn_hash.clone();
            summary.updated_at = committed_at.clone();
            validate_conversation(&summary)?;
            Ok((turn, summary))
        }
    };
    // Precheck under the lock: nothing below may fail for a reason known
    // now, so the durable write is attempted only for an admissible turn.
    let precheck_store = store.clone();
    let precheck_owner = owner.principal_id.clone();
    let precheck_turn = pending.turn_id.clone();
    let precheck_conversation = pending.conversation_id.clone();
    let (head, answered) = blocking(move || {
        let head = precheck_store
            .get_chat_conversation(&precheck_owner, &precheck_conversation)?
            .context("conversation does not exist in the asking principal's scope")?;
        let answered = precheck_store
            .get_chat_turn(&precheck_owner, &precheck_turn)?
            .is_some();
        Ok((head, answered))
    })
    .await?;
    anyhow::ensure!(!answered, "chat turn is already answered");
    make_turn(&head)?;

    let principal = owner.principal_id.clone();
    let conversation_id = pending.conversation_id.clone();
    let turn_key = pending.turn_id.clone();
    let index = pending.turn_index;
    let durable_store = store.clone();
    let ((record, snapshot), published) = publisher
        .commit_reporting(effect, move || {
            let draft = |head: &ChatConversationRecord,
                         _revision: u64|
             -> std::result::Result<ChatTurnDraft, CozoError> {
                let (turn, summary) =
                    make_turn(head).map_err(|error| CozoError::Other(error.to_string()))?;
                Ok(ChatTurnDraft {
                    turn_hash: turn.turn_hash.clone(),
                    content_hash: turn.content_hash.clone(),
                    turn: serde_json::to_value(&turn)?,
                    conversation: serde_json::to_value(&summary)?,
                })
            };
            durable_store
                .insert_chat_turn_and_snapshot(
                    &principal,
                    &conversation_id,
                    &turn_key,
                    index,
                    &draft,
                    CONVERSATION_SNAPSHOT_LIMIT,
                    TURN_SNAPSHOT_LIMIT,
                )
                .map_err(Into::into)
        })
        .await?;
    let turn = decode_turn(&record, &owner.principal_id)?;
    anyhow::ensure!(
        turn.responder_principal_id == author && turn.turn_id == pending.turn_id,
        "committed turn differs from the claimed turn"
    );

    // Content-free doorbell for the session that asked, once its private
    // document holds the turn; the in-process wake (owner key only) was sent
    // by the publication itself. A pending publication rings from recovery.
    if published {
        publisher.ring(DoorbellWake {
            owner_key: crate::wake_doorbell::owner_key(&EpisodeScope {
                principal_id: owner.principal_id.clone(),
                session_id: owner.session_id.clone(),
                session_genesis: owner.session_genesis.clone(),
            }),
            seq: snapshot.revision,
        });
    }
    // The durable turn belongs on the public chain whether or not its
    // private document is published yet: the chain is how a conversation's
    // completeness is verified, and a retry would be refused as answered.
    announce(
        engine,
        author,
        boundary.is_some().then(|| RESPOND_CAPABILITY.to_string()),
        &ResponseCommittedSignal {
            owner_key: owner_key(&owner.principal_id),
            conversation_id: turn.conversation_id.clone(),
            turn_id: turn.turn_id.clone(),
            response_id: turn.response_id.clone(),
            turn_index: turn.turn_index,
            prev_turn_hash: turn.prev_turn_hash.clone(),
            turn_hash: turn.turn_hash.clone(),
            content_hash: turn.content_hash.clone(),
            revision: snapshot.revision,
            request_event_id: turn.request_event_id,
            request_event_hash: turn.request_event_hash.clone(),
        },
    )
    .await;
    anyhow::ensure!(
        published,
        "chat_response turn {} committed durably and announced, but its private state \
         is pending recovery; recovery republishes it and rings the asking session",
        turn.turn_id
    );
    Ok((turn, snapshot.revision))
}

/// Record the commit on the global chain under the responding agent, through
/// the declared `response_committed` signal. The payload is public: ids and
/// hashes only. Best effort: the durable row and private document are
/// already committed; a failed announcement is logged, never unwound.
async fn announce(
    engine: &MutationEngine,
    author: &str,
    capability: Option<String>,
    signal: &ResponseCommittedSignal,
) {
    let value = match simd_json::serde::to_owned_value(signal) {
        Ok(value) => value,
        Err(error) => {
            tracing::warn!(%error, "chat_response commit not announced");
            return;
        }
    };
    if let Err(error) = engine
        .process_authoritative_change(
            PLUGIN.to_string(),
            crate::state_stream::plugin_object_path(PLUGIN),
            ChangeType::Signal,
            Some(RESPONSE_COMMITTED_SIGNAL.to_string()),
            None,
            value,
            vec!["state".into(), PLUGIN.into()],
            author.to_string(),
            capability,
            ChangeSource::Internal,
        )
        .await
    {
        tracing::warn!(%error, "chat_response commit not announced");
    }
}
