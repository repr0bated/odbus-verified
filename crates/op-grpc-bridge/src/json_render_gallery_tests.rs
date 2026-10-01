//! End-to-end json_render gallery and catalog dispatch through the canonical
//! MutationEngine path: sealed catalog, verified identities, exact grants,
//! the chat Canvas profile and the control plane's durable owner.

use std::sync::Arc;

use op_cozo_store::CozoGraphShuttle;
use op_plugins::state_plugins::{
    elevation::{AUTHORIZE_CAPABILITY, ElevationPlugin},
    human_principal::HumanPrincipalPlugin,
    identity_sled::IdentitySledPlugin,
    json_render::{
        JsonRenderPlugin,
        chat_canvas::ChatCanvasProfile,
        gallery::{GALLERY_CAPABILITY, PROMOTE_CAPABILITY},
    },
};
use op_state::StatePlugin;
use serde_json::{Value, json};

use crate::{
    human_principal_dispatch::tests::{
        TempCozo, TempShm, pk, register, temp_cozo, temp_shm_with_schemas, test_engine,
    },
    identity_sled_dispatch::{IdentitySledStore, tests::write_identity},
    mutation_engine::MutationEngine,
};

const PRIVATE_PROMPT: &str = "PRIVATE-PROMPT-SENTINEL draw my network";
/// A fixture root password, never a real host credential.
pub(crate) const SUDO_PASSWORD: &str = "SUDO-PASSWORD-SENTINEL correct horse";

/// Accepts `SUDO_PASSWORD` for the listed Unix accounts only.
pub(crate) struct FakeSudo(pub Vec<&'static str>);
impl crate::task_elevation::SudoAuthority for FakeSudo {
    fn verify(&self, user: &str, password: &str) -> anyhow::Result<bool> {
        Ok(self.0.contains(&user) && password == SUDO_PASSWORD)
    }
}

pub(crate) struct Environment(Vec<(&'static str, Option<std::ffi::OsString>)>);
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

pub(crate) struct Identity {
    pub(crate) principal: String,
    pub(crate) session: String,
    pub(crate) genesis: String,
}

pub(crate) struct Fixture {
    pub(crate) engine: Arc<MutationEngine>,
    pub(crate) alice: Identity,
    pub(crate) bob: Identity,
    pub(crate) carol: Identity,
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
    }
}

pub(crate) async fn fixture() -> Fixture {
    let cozo = temp_cozo();
    let shm = temp_shm_with_schemas(&[
        HumanPrincipalPlugin::new().schema().unwrap(),
        IdentitySledPlugin::new().schema().unwrap(),
        JsonRenderPlugin::new().schema().unwrap(),
        ElevationPlugin::new().schema().unwrap(),
    ]);
    let grants_dir = tempfile::tempdir().unwrap();
    let grants_path = grants_dir.path().join("grants.json");
    let users_path = grants_dir.path().join("elevation-users.json");
    let environment = Environment::set(&[
        ("OP_GRANTS_PATH", grants_path.display().to_string()),
        ("OP_ELEVATION_USERS_PATH", users_path.display().to_string()),
    ]);
    let engine = test_engine();
    engine
        .install_identity_sled_store_for_test(IdentitySledStore::new(
            CozoGraphShuttle::new_in_memory().unwrap(),
        ))
        .unwrap();
    engine
        .configure_ui_gallery_store(Arc::new(CozoGraphShuttle::new_in_memory().unwrap()))
        .unwrap();
    let alice = identity(&engine, 0xA1, "gallery-alice").await;
    let bob = identity(&engine, 0xB2, "gallery-bob").await;
    let carol = identity(&engine, 0xC3, "gallery-carol").await;
    engine
        .task_elevation()
        .set_authority_for_test(Arc::new(FakeSudo(vec!["root"])));
    // The retired map path deliberately does not exist. Every elevation
    // regression must succeed without a per-principal Unix account mapping.
    assert!(!users_path.exists());
    std::fs::write(
        &grants_path,
        serde_json::to_vec(&json!({
            (alice.principal.clone()): {"capabilities": [GALLERY_CAPABILITY, PROMOTE_CAPABILITY, AUTHORIZE_CAPABILITY]},
            (bob.principal.clone()): {"capabilities": [GALLERY_CAPABILITY, AUTHORIZE_CAPABILITY]},
            (carol.principal.clone()): {"capabilities": []},
        }))
        .unwrap(),
    )
    .unwrap();
    Fixture {
        engine,
        alice,
        bob,
        carol,
        _grants: grants_dir,
        _environment: environment,
        _shm: shm,
        _cozo: cozo,
    }
}

fn capability(method: &str) -> &'static str {
    if matches!(method, "promote_candidate" | "retire_catalog_entry") {
        PROMOTE_CAPABILITY
    } else {
        GALLERY_CAPABILITY
    }
}

/// Obtain the one-time sudo ticket for a protected call.
pub(crate) async fn authorize(
    fixture: &Fixture,
    who: &Identity,
    plugin: &str,
    method: &str,
    args: &Value,
    password: &str,
) -> anyhow::Result<Value> {
    let envelope = fixture
        .engine
        .dispatch_method_call_with_identity(
            "elevation",
            "authorize_task",
            &json!({"plugin": plugin, "method": method, "args": args, "password": password})
                .to_string(),
            Some(AUTHORIZE_CAPABILITY),
            &who.principal,
            Some(&who.session),
            Some(&who.genesis),
        )
        .await?;
    Ok(envelope["result"].clone())
}

/// A gallery call; promote and retire first obtain their sudo ticket.
pub(crate) async fn call(
    fixture: &Fixture,
    who: &Identity,
    method: &str,
    args: Value,
) -> anyhow::Result<Value> {
    if matches!(method, "promote_candidate" | "retire_catalog_entry") {
        authorize(fixture, who, "json_render", method, &args, SUDO_PASSWORD).await?;
    }
    call_unelevated(fixture, who, method, args).await
}

pub(crate) async fn call_unelevated(
    fixture: &Fixture,
    who: &Identity,
    method: &str,
    args: Value,
) -> anyhow::Result<Value> {
    let envelope = fixture
        .engine
        .dispatch_method_call_with_identity(
            "json_render",
            method,
            &args.to_string(),
            Some(capability(method)),
            &who.principal,
            Some(&who.session),
            Some(&who.genesis),
        )
        .await?;
    Ok(envelope["result"].clone())
}

pub(crate) fn error_of(result: anyhow::Result<Value>) -> String {
    format!("{:#}", result.expect_err("call must be refused"))
}

pub(crate) fn canvas(title: &str) -> Value {
    json!({
        "root": "page",
        "elements": {
            "page": {"type": "container", "props": {"className": null}, "children": ["title", "metric"]},
            "title": {"type": "heading", "props": {"text": title, "level": 2}},
            "metric": {"type": "stat", "props": {"label": "Nodes", "value": "4", "unit": null, "trend": null}}
        }
    })
}

pub(crate) async fn save(fixture: &Fixture, who: &Identity, title: &str) -> Value {
    call(
        fixture,
        who,
        "save_candidate",
        json!({"spec": canvas(title), "title": title, "mode": "ui_render", "prompt": PRIVATE_PROMPT}),
    )
    .await
    .unwrap()["candidate"]
        .clone()
}

#[tokio::test(flavor = "multi_thread")]
async fn candidates_are_private_and_validated_against_the_sealed_profile() {
    let fixture = fixture().await;
    let candidate = save(&fixture, &fixture.alice, "Network map").await;
    let id = candidate["candidate_id"].as_str().unwrap().to_string();
    assert_eq!(uuid::Uuid::parse_str(&id).unwrap().get_version_num(), 7);
    assert_eq!(candidate["principal_id"], fixture.alice.principal.as_str());
    assert_eq!(
        candidate["profile_sha256"],
        ChatCanvasProfile::default()
            .evidence
            .profile_sha256
            .as_str()
    );

    let mut invalid = canvas("x");
    invalid["elements"]["title"]["props"]["html"] = json!("<script>");
    let refused = error_of(
        call(
            &fixture,
            &fixture.alice,
            "save_candidate",
            json!({"spec": invalid, "title": "Bad", "mode": "ui_render"}),
        )
        .await,
    );
    assert!(refused.contains("sealed chat Canvas profile"), "{refused}");
    let listed = call(&fixture, &fixture.alice, "list_candidates", json!({}))
        .await
        .unwrap();
    assert_eq!(listed["total"], 1, "a refused canvas is not saved");

    let hidden = error_of(
        call(
            &fixture,
            &fixture.bob,
            "get_candidate",
            json!({"candidate_id": id}),
        )
        .await,
    );
    assert!(hidden.contains("not found"), "{hidden}");
    let bobs = call(&fixture, &fixture.bob, "list_candidates", json!({}))
        .await
        .unwrap();
    assert_eq!(bobs["total"], 0);

    let ungranted = error_of(
        call(
            &fixture,
            &fixture.carol,
            "save_candidate",
            json!({"spec": canvas("c"), "title": "C", "mode": "ui_render"}),
        )
        .await,
    );
    assert!(ungranted.contains("json_render.gallery"), "{ungranted}");

    let deleted = call(
        &fixture,
        &fixture.bob,
        "delete_candidate",
        json!({"candidate_id": id}),
    )
    .await
    .unwrap();
    assert_eq!(
        deleted["deleted"], false,
        "Bob cannot delete Alice's candidate"
    );
    let fetched = call(
        &fixture,
        &fixture.alice,
        "get_candidate",
        json!({"candidate_id": id}),
    )
    .await
    .unwrap();
    assert_eq!(fetched["candidate"], candidate);
}

#[tokio::test(flavor = "multi_thread")]
async fn candidates_page_without_a_cap() {
    let fixture = fixture().await;
    let mut ids = Vec::new();
    for n in 0..5 {
        ids.push(
            save(&fixture, &fixture.alice, &format!("Draft {n}")).await["candidate_id"]
                .as_str()
                .unwrap()
                .to_string(),
        );
    }
    ids.reverse();
    let mut seen = Vec::new();
    let mut before = Value::Null;
    loop {
        let page = call(
            &fixture,
            &fixture.alice,
            "list_candidates",
            json!({"limit": 2, "before": before}),
        )
        .await
        .unwrap();
        assert_eq!(page["total"], 5);
        seen.extend(
            page["candidates"]
                .as_array()
                .unwrap()
                .iter()
                .map(|c| c["candidate_id"].as_str().unwrap().to_string()),
        );
        before = page["next_before"].clone();
        if before.is_null() {
            break;
        }
    }
    assert_eq!(seen, ids, "newest first, every candidate once");
}

#[tokio::test(flavor = "multi_thread")]
async fn promotion_publishes_to_everyone_once_and_retirement_keeps_the_record() {
    let fixture = fixture().await;
    let first = save(&fixture, &fixture.alice, "Status page").await;
    let second = save(&fixture, &fixture.alice, "Status page two").await;
    let bobs = save(&fixture, &fixture.bob, "Bob's page").await;

    let refused = error_of(
        call(
            &fixture,
            &fixture.bob,
            "promote_candidate",
            json!({"candidate_id": bobs["candidate_id"], "name": "Bob"}),
        )
        .await,
    );
    assert!(refused.contains("json_render.promote"), "{refused}");

    let entry = call(
        &fixture,
        &fixture.alice,
        "promote_candidate",
        json!({"candidate_id": first["candidate_id"], "name": "Status", "description": "Host status"}),
    )
    .await
    .unwrap()["entry"]
        .clone();
    assert_eq!(entry["status"], "active");
    assert_eq!(entry["spec"], first["spec"]);
    assert_eq!(entry["promoted_by"], fixture.alice.principal.as_str());

    let again = error_of(
        call(
            &fixture,
            &fixture.alice,
            "promote_candidate",
            json!({"candidate_id": first["candidate_id"], "name": "Status again"}),
        )
        .await,
    );
    assert!(again.contains("already promoted"), "{again}");
    let taken = error_of(
        call(
            &fixture,
            &fixture.alice,
            "promote_candidate",
            json!({"candidate_id": second["candidate_id"], "name": "Status"}),
        )
        .await,
    );
    assert!(taken.contains("already has this name"), "{taken}");

    // Every human with the gallery grant reads the catalog.
    let catalog = call(&fixture, &fixture.bob, "list_catalog", json!({}))
        .await
        .unwrap();
    assert_eq!(catalog["total"], 1);
    assert_eq!(catalog["entries"][0]["entry_id"], entry["entry_id"]);
    let read = call(
        &fixture,
        &fixture.bob,
        "get_catalog_entry",
        json!({"entry_id": entry["entry_id"]}),
    )
    .await
    .unwrap();
    assert_eq!(read["entry"], entry);
    let promoted = call(
        &fixture,
        &fixture.alice,
        "get_candidate",
        json!({"candidate_id": first["candidate_id"]}),
    )
    .await
    .unwrap();
    assert_eq!(
        promoted["candidate"]["promoted_entry_id"],
        entry["entry_id"]
    );

    let not_bob = error_of(
        call(
            &fixture,
            &fixture.bob,
            "retire_catalog_entry",
            json!({"entry_id": entry["entry_id"]}),
        )
        .await,
    );
    assert!(not_bob.contains("json_render.promote"), "{not_bob}");
    let retired = call(
        &fixture,
        &fixture.alice,
        "retire_catalog_entry",
        json!({"entry_id": entry["entry_id"], "reason": "superseded"}),
    )
    .await
    .unwrap()["entry"]
        .clone();
    assert_eq!(retired["status"], "retired");
    assert_eq!(retired["retired_by"], fixture.alice.principal.as_str());
    let active = call(&fixture, &fixture.bob, "list_catalog", json!({}))
        .await
        .unwrap();
    assert_eq!(active["total"], 0);
    let all = call(
        &fixture,
        &fixture.bob,
        "list_catalog",
        json!({"include_retired": true}),
    )
    .await
    .unwrap();
    assert_eq!(all["entries"][0]["status"], "retired");

    // The name is free again once its entry is retired.
    call(
        &fixture,
        &fixture.alice,
        "promote_candidate",
        json!({"candidate_id": second["candidate_id"], "name": "Status"}),
    )
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn the_chain_records_gallery_calls_without_their_content() {
    let fixture = fixture().await;
    save(&fixture, &fixture.alice, "Chain check").await;
    let events = fixture.engine.event_chain.try_read().unwrap();
    let recorded: Vec<_> = events
        .events()
        .iter()
        .filter(|event| event.plugin_id == "json_render")
        .collect();
    assert!(!recorded.is_empty(), "the save is on the chain");
    for event in recorded {
        let text = serde_json::to_string(event).unwrap();
        assert!(!text.contains("PRIVATE-PROMPT-SENTINEL"), "{text}");
        assert!(!text.contains("Chain check"), "{text}");
    }
}
