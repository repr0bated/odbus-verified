//! End-to-end `chat_response` dispatch through the canonical MutationEngine
//! path: sealed catalog, compiled contract admission, verified identities,
//! exact grants, the turn registry, the durable owner and the private SHM.

use std::sync::{Arc, Mutex};

use op_core::projection_shm::PrivateProjectionDirectory;
use op_cozo_store::CozoGraphShuttle;
use op_plugins::state_plugins::{
    chat_response::{ChatResponsePlugin, READ_CAPABILITY, RESPOND_CAPABILITY},
    chat_response_methods::{GetConversationOutput, GetTurnResponseOutput},
    human_principal::HumanPrincipalPlugin,
    identity_sled::IdentitySledPlugin,
    json_render::JsonRenderPlugin,
};
use op_state::StatePlugin;

use super::*;
use crate::{
    chat_response_state::DoorbellWake,
    chat_response_turns::TurnOwner,
    human_principal_dispatch::tests::{
        TempCozo, TempShm, pk, register, temp_cozo, temp_shm_with_schemas, test_engine,
    },
    identity_sled_dispatch::{
        IdentitySledStore,
        tests::{write_identity, write_second_session},
    },
};

const PRIVATE_MESSAGE: &str = "PRIVATE-MESSAGE-SENTINEL answer text";
const PRIVATE_CANVAS: &str = "PRIVATE-CANVAS-SENTINEL";
const PRIVATE_TITLE: &str = "PRIVATE-TITLE-SENTINEL";

struct Environment(Vec<(&'static str, Option<std::ffi::OsString>)>);
impl Environment {
    fn set(values: &[(&'static str, String)]) -> Self {
        let previous = values
            .iter()
            .map(|(key, _)| (*key, std::env::var_os(key)))
            .collect();
        for (key, value) in values {
            // FIXME: Audit that the environment access only happens in single-threaded code.
            unsafe { std::env::set_var(key, value) };
        }
        Self(previous)
    }
}
impl Drop for Environment {
    fn drop(&mut self) {
        for (key, value) in self.0.drain(..) {
            match value {
                // FIXME: Audit that the environment access only happens in single-threaded code.
                Some(value) => unsafe { std::env::set_var(key, value) },
                // FIXME: Audit that the environment access only happens in single-threaded code.
                None => unsafe { std::env::remove_var(key) },
            }
        }
    }
}

#[derive(Clone)]
struct Identity {
    key: String,
    principal: String,
    session: String,
    genesis: String,
}

impl Identity {
    fn owner(&self) -> TurnOwner {
        TurnOwner {
            principal_id: self.principal.clone(),
            session_id: self.session.clone(),
            session_genesis: self.genesis.clone(),
        }
    }
}

struct Fixture {
    engine: Arc<MutationEngine>,
    store: Arc<CozoGraphShuttle>,
    human: Identity,
    agent: Identity,
    other: Identity,
    rung: Arc<Mutex<Vec<DoorbellWake>>>,
    _private: tempfile::TempDir,
    _grants: tempfile::TempDir,
    _environment: Environment,
    _shm: TempShm,
    _cozo: TempCozo,
}

async fn identity(engine: &MutationEngine, byte: u8, alias: &str) -> Identity {
    let key = pk(byte);
    register(&key, alias).await.unwrap();
    let sled = write_identity(engine, &key).await.unwrap();
    Identity {
        principal: op_identity::session::derive_principal_id(&key),
        session: sled.session_id,
        genesis: sled.genesis.unwrap(),
        key,
    }
}

async fn fixture() -> Fixture {
    let cozo = temp_cozo();
    let shm = temp_shm_with_schemas(&[
        HumanPrincipalPlugin::new().schema().unwrap(),
        IdentitySledPlugin::new().schema().unwrap(),
        ChatResponsePlugin::new().schema().unwrap(),
        JsonRenderPlugin::new().schema().unwrap(),
    ]);
    let grants_dir = tempfile::tempdir().unwrap();
    let grants_path = grants_dir.path().join("grants.json");
    let environment = Environment::set(&[("OP_GRANTS_PATH", grants_path.display().to_string())]);
    let engine = test_engine();
    engine
        .install_identity_sled_store_for_test(IdentitySledStore::new(
            CozoGraphShuttle::new_in_memory().unwrap(),
        ))
        .unwrap();
    let store = Arc::new(CozoGraphShuttle::new_in_memory().unwrap());
    engine.configure_chat_response_store(store.clone()).unwrap();
    let private = tempfile::tempdir().unwrap();
    engine
        .configure_chat_response_projection(
            PrivateProjectionDirectory::prepare(&private.path().join("chat")).unwrap(),
            std::env::var("OP_BLOB_CATALOG_DIR").unwrap().into(),
        )
        .unwrap();
    let rung = Arc::new(Mutex::new(Vec::new()));
    let sink = rung.clone();
    engine
        .chat_response_publisher()
        .unwrap()
        .set_doorbell(Arc::new(move |wake| sink.lock().unwrap().push(wake)))
        .unwrap();
    let human = identity(&engine, 0xC1, "chat-response-human").await;
    let agent = identity(&engine, 0xC2, "chat-response-agent").await;
    let other = identity(&engine, 0xC3, "chat-response-other-human").await;
    // The agent answers; humans read their own scope. `other` also holds the
    // respond grant, so its refusal below is the turn binding, not a grant.
    std::fs::write(
        &grants_path,
        serde_json::to_vec(&serde_json::json!({
            (human.principal.clone()): {"capabilities": [READ_CAPABILITY]},
            (agent.principal.clone()): {"capabilities": [RESPOND_CAPABILITY]},
            (other.principal.clone()): {"capabilities": [READ_CAPABILITY, RESPOND_CAPABILITY]},
        }))
        .unwrap(),
    )
    .unwrap();
    Fixture {
        engine,
        store,
        human,
        agent,
        other,
        rung,
        _private: private,
        _grants: grants_dir,
        _environment: environment,
        _shm: shm,
        _cozo: cozo,
    }
}

async fn call(
    fixture: &Fixture,
    who: &Identity,
    method: &str,
    args: serde_json::Value,
) -> anyhow::Result<serde_json::Value> {
    let capability = op_plugins::state_plugins::chat_response::required_capability(method).unwrap();
    let envelope = fixture
        .engine
        .dispatch_method_call_with_identity(
            PLUGIN,
            method,
            &args.to_string(),
            Some(capability),
            &who.principal,
            Some(&who.session),
            Some(&who.genesis),
        )
        .await?;
    Ok(envelope["result"].clone())
}

async fn start(fixture: &Fixture, who: &Identity) -> String {
    let started = call(
        fixture,
        who,
        "start_conversation",
        serde_json::json!({"title": PRIVATE_TITLE}),
    )
    .await
    .unwrap();
    let id = started["conversation_id"].as_str().unwrap().to_string();
    assert_eq!(uuid::Uuid::parse_str(&id).unwrap().get_version_num(), 7);
    chrono::DateTime::parse_from_rfc3339(started["started_at"].as_str().unwrap()).unwrap();
    id
}

fn canvas(text: &str) -> serde_json::Value {
    serde_json::json!({"root": "root", "elements": {"root": {
        "type": "text", "props": {"text": text, "className": null}, "children": []
    }}})
}

fn error_of(result: anyhow::Result<serde_json::Value>) -> String {
    format!("{:#}", result.expect_err("call must be refused"))
}

fn chain_events(fixture: &Fixture) -> Vec<op_state_store::ChainEvent> {
    fixture
        .engine
        .event_chain
        .try_read()
        .unwrap()
        .events()
        .iter()
        .filter(|event| event.plugin_id == PLUGIN)
        .cloned()
        .collect()
}

fn payload(event: &op_state_store::ChainEvent) -> serde_json::Value {
    serde_json::from_str(&simd_json::to_string(event.input_payload.as_ref().unwrap()).unwrap())
        .unwrap()
}

#[tokio::test]
async fn responses_bind_to_the_designated_responder_once_into_the_human_scope() {
    let f = fixture().await;
    let publisher = f.engine.chat_response_publisher().unwrap().clone();
    let mut wakes = publisher.subscribe();
    let conversation = start(&f, &f.human).await;
    assert_eq!(wakes.recv().await.unwrap(), owner_key(&f.human.principal));

    // Only a verified human's own conversation, answered by a holder of the
    // respond grant other than that human, can have a turn.
    assert!(
        f.engine
            .open_chat_turn(f.human.owner(), &conversation, &f.human.principal)
            .await
            .is_err()
    );
    assert!(
        f.engine
            .open_chat_turn(f.other.owner(), &conversation, &f.agent.principal)
            .await
            .is_err(),
        "a conversation outside the owner's scope has no turns"
    );
    let turn = f
        .engine
        .open_chat_turn(f.human.owner(), &conversation, &f.agent.principal)
        .await
        .unwrap();
    assert_eq!(turn.turn_index, 0);
    assert_eq!(
        uuid::Uuid::parse_str(&turn.turn_id)
            .unwrap()
            .get_version_num(),
        7
    );
    let respond = |text: &str| {
        serde_json::json!({
            "turn_id": turn.turn_id, "message": text, "message_type": "success",
            "related_actions": ["workflows.query_workflows"],
            "canvas": canvas(PRIVATE_CANVAS),
        })
    };

    // Another principal holding the respond grant is not this turn's responder.
    let refused = error_of(call(&f, &f.other, "respond_to_user", respond("forged")).await);
    assert!(refused.contains("designated responder"), "{refused}");
    // The human cannot answer at all: no respond grant.
    let refused = error_of(call(&f, &f.human, "respond_to_user", respond("self")).await);
    assert!(refused.contains("chat_response.respond"), "{refused}");
    // A turn the bridge never opened is refused.
    let unopened = serde_json::json!({
        "turn_id": uuid::Uuid::now_v7().to_string(), "question": "which?"
    });
    let refused = error_of(call(&f, &f.agent, "request_clarification", unopened).await);
    assert!(refused.contains("not open"), "{refused}");
    // An invalid canvas rejects the whole response and leaves the turn open.
    let mut invalid = respond(PRIVATE_MESSAGE);
    invalid["canvas"]["elements"]["root"]["type"] = serde_json::json!("Card");
    let refused = error_of(call(&f, &f.agent, "respond_to_user", invalid).await);
    assert!(refused.contains("canvas rejected"), "{refused}");
    assert!(f.engine.chat_turns().is_open(&turn.turn_id));
    assert!(
        f.store
            .get_chat_turn(&f.human.principal, &turn.turn_id)
            .unwrap()
            .is_none()
    );

    let committed = call(&f, &f.agent, "respond_to_user", respond(PRIVATE_MESSAGE))
        .await
        .unwrap();
    let renderer = crate::sealed_schema_reader::read_manifest_pinned_schema_document(
        &blob_catalog_dir_from_env(),
        "json_render",
    )
    .unwrap();
    assert_eq!(committed["conversation_id"], conversation);
    assert_eq!(committed["turn_index"], 0);
    assert_eq!(committed["revision"], 2);
    assert_eq!(committed["canvas_schema_hash"], renderer.schema_hash);
    let response_id = committed["response_id"].as_str().unwrap();
    assert_eq!(
        uuid::Uuid::parse_str(response_id)
            .unwrap()
            .get_version_num(),
        7
    );
    let turn_hash = committed["turn_hash"].as_str().unwrap().to_string();

    let request_event_id = chain_events(&f)
        .iter()
        .rev()
        .find(|event| event.method_name.as_deref() == Some("respond_to_user"))
        .unwrap()
        .event_id;

    // Exactly once.
    let again = error_of(call(&f, &f.agent, "respond_to_user", respond("second")).await);
    assert!(again.contains("already answered"), "{again}");

    // The human reads it from their own scope, with its provenance and chain.
    let read = call(
        &f,
        &f.human,
        "get_turn_response",
        serde_json::json!({"turn_id": turn.turn_id}),
    )
    .await
    .unwrap();
    let read: GetTurnResponseOutput = serde_json::from_value(read).unwrap();
    let stored = read.turn.unwrap();
    assert_eq!(stored.turn_hash, turn_hash);
    assert_eq!(stored.responder_principal_id, f.agent.principal);
    assert_eq!(stored.principal_id, f.human.principal);
    assert_eq!(stored.session_id, f.human.session);
    assert_eq!(stored.request_event_id, request_event_id);
    let summary = f
        .store
        .get_chat_conversation(&f.human.principal, &conversation)
        .unwrap()
        .unwrap();
    assert_eq!(summary.turn_count, 1);
    assert_eq!(summary.head_turn_hash, turn_hash);
    assert_eq!(
        stored.prev_turn_hash, summary.conversation["genesis_hash"],
        "turn 0 chains to the conversation genesis"
    );

    // Never in the agent's scope; another human cannot read it.
    let agent_scope = f
        .store
        .snapshot_chat_responses(&f.agent.principal, 8, 8)
        .unwrap();
    assert_eq!(
        (agent_scope.conversation_count, agent_scope.turn_count),
        (0, 0)
    );
    assert!(publisher.read(&f.agent.principal).await.is_err());
    let foreign = call(
        &f,
        &f.other,
        "get_turn_response",
        serde_json::json!({"turn_id": turn.turn_id}),
    )
    .await
    .unwrap();
    assert!(foreign["turn"].is_null());
    let foreign = error_of(
        call(
            &f,
            &f.other,
            "get_conversation",
            serde_json::json!({"conversation_id": conversation}),
        )
        .await,
    );
    assert!(foreign.contains("does not exist"), "{foreign}");
    let listed = call(&f, &f.other, "list_conversations", serde_json::json!({}))
        .await
        .unwrap();
    assert_eq!(listed["conversation_count"], 0);

    // The committed private document holds the content, in the human's view.
    let state = publisher.read(&f.human.principal).await.unwrap();
    assert_eq!(
        state["turns"][&turn.turn_id]["content"]["message"],
        PRIVATE_MESSAGE
    );
    assert_eq!(state["conversations"][&conversation]["turn_count"], 1);
    assert_eq!(state["revision"], 2);

    // Wakes carry owner keys only.
    let wake = wakes.recv().await.unwrap();
    assert_eq!(wake, owner_key(&f.human.principal));
    assert_ne!(wake, owner_key(&f.agent.principal));
    let rung = f.rung.lock().unwrap().clone();
    assert_eq!(rung.len(), 1, "one doorbell per committed response");
    assert_eq!(
        rung[0].owner_key,
        crate::wake_doorbell::owner_key(&EpisodeScope {
            principal_id: f.human.principal.clone(),
            session_id: f.human.session.clone(),
            session_genesis: f.human.genesis.clone(),
        })
    );
    assert_eq!(rung[0].seq, 2);
    let ring = serde_json::to_value(&rung[0]).unwrap();
    assert_eq!(ring.as_object().unwrap().len(), 2, "owner_key and seq only");

    // The chain records the public sub-chain fields under the agent, and no
    // content in either the signal or the redacted method-call payloads.
    let events = chain_events(&f);
    let signal = events
        .iter()
        .find(|event| event.method_name.as_deref() == Some("response_committed"))
        .expect("response_committed is announced");
    assert_eq!(signal.actor_id, f.agent.principal);
    assert_eq!(signal.capability_id.as_deref(), Some(RESPOND_CAPABILITY));
    let public = payload(signal);
    assert_eq!(public["conversation_id"], conversation);
    assert_eq!(public["turn_index"], 0);
    assert_eq!(public["turn_hash"], turn_hash);
    assert_eq!(public["prev_turn_hash"], stored.prev_turn_hash);
    assert_eq!(public["content_hash"], stored.content_hash);
    assert_eq!(public["owner_key"], owner_key(&f.human.principal));
    for event in &events {
        let Some(input) = event.input_payload.as_ref() else {
            continue;
        };
        let encoded = simd_json::to_string(input).unwrap();
        for private in [PRIVATE_MESSAGE, PRIVATE_CANVAS, PRIVATE_TITLE] {
            assert!(
                !encoded.contains(private),
                "{:?} leaked content into the chain",
                event.method_name
            );
        }
    }
}

#[tokio::test]
async fn turns_chain_across_turns_and_a_later_session_of_the_same_principal() {
    let f = fixture().await;
    let conversation = start(&f, &f.human).await;
    let first = f
        .engine
        .open_chat_turn(f.human.owner(), &conversation, &f.agent.principal)
        .await
        .unwrap();
    assert!(
        f.engine
            .open_chat_turn(f.human.owner(), &conversation, &f.agent.principal)
            .await
            .is_err(),
        "one open turn per conversation"
    );
    call(
        &f,
        &f.agent,
        "respond_to_user",
        serde_json::json!({"turn_id": first.turn_id, "message": "first", "message_type": "info"}),
    )
    .await
    .unwrap();

    // A later login of the same principal continues the same conversation.
    let later = write_second_session(&f.engine, &f.human.key, &uuid::Uuid::new_v4().to_string())
        .await
        .unwrap();
    let later = Identity {
        session: later.session_id,
        genesis: later.genesis.unwrap(),
        ..f.human.clone()
    };
    assert_ne!(later.genesis, f.human.genesis);
    let second = f
        .engine
        .open_chat_turn(later.owner(), &conversation, &f.agent.principal)
        .await
        .unwrap();
    assert_eq!(second.turn_index, 1);
    let clarified = call(
        &f,
        &f.agent,
        "request_clarification",
        serde_json::json!({
            "turn_id": second.turn_id, "question": "which bridge?",
            "context": "two candidates", "options": ["br0", "br1"],
        }),
    )
    .await
    .unwrap();
    assert_eq!(clarified["turn_index"], 1);
    assert!(clarified.get("canvas_schema_hash").is_none());

    let third = f
        .engine
        .open_chat_turn(f.human.owner(), &conversation, &f.agent.principal)
        .await
        .unwrap();
    assert_eq!(third.turn_index, 2);
    let refused = call(
        &f,
        &f.agent,
        "cannot_perform",
        serde_json::json!({
            "turn_id": third.turn_id, "reason": "not permitted",
            "category": "not_allowed", "alternatives": ["ask an operator"],
        }),
    )
    .await
    .unwrap();
    assert_eq!(refused["turn_index"], 2);

    // Read from the later session: one conversation, three chained turns.
    let read = call(
        &f,
        &later,
        "get_conversation",
        serde_json::json!({"conversation_id": conversation}),
    )
    .await
    .unwrap();
    let read: GetConversationOutput = serde_json::from_value(read).unwrap();
    assert_eq!(read.conversation.turn_count, 3);
    assert!(!read.turns_truncated);
    assert_eq!(read.first_turn_index, 0);
    assert_eq!(
        read.turns.iter().map(|t| t.turn_index).collect::<Vec<_>>(),
        [0, 1, 2]
    );
    assert_eq!(read.turns[0].prev_turn_hash, read.conversation.genesis_hash);
    assert_eq!(read.turns[1].prev_turn_hash, read.turns[0].turn_hash);
    assert_eq!(read.turns[2].prev_turn_hash, read.turns[1].turn_hash);
    assert_eq!(read.conversation.head_turn_hash, read.turns[2].turn_hash);
    assert_eq!(read.turns[0].session_id, f.human.session);
    assert_eq!(read.turns[1].session_id, later.session);
    assert_eq!(read.turns[1].session_genesis, later.genesis);
    op_plugins::state_plugins::chat_response_methods::verify_turn_chain(
        &read.conversation,
        &read.turns,
        0,
    )
    .unwrap();
    // The public chain alone reproduces the same hashes.
    let events = chain_events(&f);
    let announced: Vec<_> = events
        .iter()
        .filter(|event| event.method_name.as_deref() == Some("response_committed"))
        .map(payload)
        .collect();
    assert_eq!(announced.len(), 3);
    for (turn, public) in read.turns.iter().zip(&announced) {
        assert_eq!(public["turn_hash"], turn.turn_hash);
        assert_eq!(public["turn_index"], turn.turn_index);
    }
    let listed = call(&f, &f.human, "list_conversations", serde_json::json!({}))
        .await
        .unwrap();
    assert_eq!(listed["conversation_count"], 1);
    assert_eq!(listed["conversations"][0]["turn_count"], 3);
    assert_eq!(listed["conversations"][0]["title"], PRIVATE_TITLE);
}

#[tokio::test]
async fn expired_turns_are_refused_without_a_commit() {
    let f = fixture().await;
    let conversation = start(&f, &f.human).await;
    let expired = f
        .engine
        .chat_turns()
        .open_at(
            f.human.owner(),
            &conversation,
            0,
            &f.agent.principal,
            chrono::Utc::now() - chrono::Duration::minutes(30),
            chrono::Duration::minutes(15),
        )
        .unwrap();
    let refused = error_of(
        call(
            &f,
            &f.agent,
            "cannot_perform",
            serde_json::json!({
                "turn_id": expired.turn_id, "reason": "late", "category": "not_supported"
            }),
        )
        .await,
    );
    assert!(refused.contains("expired"), "{refused}");
    let scope = f
        .store
        .snapshot_chat_responses(&f.human.principal, 8, 8)
        .unwrap();
    assert_eq!((scope.turn_count, scope.revision), (0, 1));
    // The conversation can still open a fresh turn at the same head.
    let fresh = f
        .engine
        .open_chat_turn(f.human.owner(), &conversation, &f.agent.principal)
        .await
        .unwrap();
    assert_eq!(fresh.turn_index, 0);
}
