//! `EventChainService.SubscribeEvents` is fed by the mutation pipeline itself.
//!
//! Both doors that append to the event chain — `dispatch_method_call` and
//! `process_authoritative_change` — must publish what they appended on the
//! engine's chain broadcast. If they don't, the audit stream is silently empty
//! while the ledger behind `GetEvents` keeps filling up, which is exactly the
//! failure these tests exist to catch.

use std::sync::Arc;
use std::time::Duration;

use op_grpc_bridge::{ChangeSource, ChangeType, MutationEngine};
use op_state::StatePlugin;
mod support;

fn engine() -> Arc<MutationEngine> {
    let event_chain = Arc::new(tokio::sync::RwLock::new(op_state_store::EventChain::new(
        op_state_store::ChainConfig::default(),
    )));
    let ovsdb = Arc::new(op_network::rovs_proxy::OvsdbDbusClient::new());
    Arc::new(MutationEngine::new(event_chain, ovsdb))
}

async fn next_event(
    rx: &mut tokio::sync::broadcast::Receiver<op_state_store::ChainEvent>,
) -> op_state_store::ChainEvent {
    tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("a recorded event reaches chain subscribers")
        .expect("chain broadcast stayed open")
}

#[tokio::test]
async fn dispatched_method_call_reaches_chain_subscribers() {
    let _catalog = support::CatalogScope::new(&[
        op_plugins::state_plugins::snowball_plugin::SnowballPlugin::new()
            .schema()
            .unwrap(),
    ]);
    let engine = engine();
    let mut rx = engine.chain_tx().subscribe();

    engine
        .dispatch_method_call(
            // A real engine-local read: cognitive health requires a separately
            // published runtime projection that this isolated fixture lacks.
            "snowball",
            "verify_chain",
            "{}",
            Some("snowball.read"),
            "test-actor",
        )
        .await
        .expect("dispatch records an event");

    let event = next_event(&mut rx).await;
    assert_eq!(event.plugin_id, "snowball");
    assert_eq!(event.actor_id, "test-actor");
    assert_eq!(event.capability_id.as_deref(), Some("snowball.read"));
    assert!(!event.event_hash.is_empty());

    // The broadcast event is the same one GetEvents serves, not a reconstruction.
    let chain = engine.event_chain.read().await;
    let recorded = chain.events().last().expect("chain holds the event");
    assert_eq!(recorded.event_id, event.event_id);
    assert_eq!(recorded.event_hash, event.event_hash);
}

#[tokio::test]
async fn authoritative_change_reaches_chain_subscribers() {
    let _catalog =
        support::CatalogScope::new(&[op_plugins::state_plugins::incus::IncusPlugin::new()
            .schema()
            .unwrap()]);
    let engine = engine();
    let mut rx = engine.chain_tx().subscribe();

    engine
        .process_authoritative_change(
            "incus".to_string(),
            "/org/opdbus/v1/plugins/incus".to_string(),
            ChangeType::PropertySet,
            Some("containers".to_string()),
            None,
            simd_json::json!([]),
            vec!["state".to_string()],
            "test-actor".to_string(),
            None,
            ChangeSource::Grpc,
        )
        .await
        .expect("authoritative change records an event");

    let event = next_event(&mut rx).await;
    assert_eq!(event.plugin_id, "incus");
    assert_eq!(event.actor_id, "test-actor");
    assert!(event.tags_touched.contains(&"state".to_string()));
    assert!(!event.event_hash.is_empty());
}
