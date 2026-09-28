//! Full exact-schema validation must run after child folding, before publication.
use op_grpc_bridge::{ChangeSource, ChangeType, MutationEngine};
use op_state_store::{FieldSchema, FieldType, PluginSchema};
use simd_json::{OwnedValue, json};
use std::sync::Arc;
use tokio::sync::broadcast::error::TryRecvError;
mod support;
fn engine() -> MutationEngine {
    MutationEngine::new(
        Arc::new(tokio::sync::RwLock::new(op_state_store::EventChain::new(
            op_state_store::ChainConfig::default(),
        ))),
        Arc::new(op_network::rovs_proxy::OvsdbDbusClient::new()),
    )
}
async fn change(
    engine: &MutationEngine,
    suffix: &str,
    member: Option<&str>,
    value: OwnedValue,
) -> Result<op_grpc_bridge::mutation_engine::StateChange, String> {
    engine
        .process_authoritative_change(
            "incus".into(),
            format!("/org/opdbus/v1/plugins/incus{suffix}"),
            ChangeType::PropertySet,
            member.map(str::to_owned),
            None,
            value,
            vec![],
            "fixture".into(),
            None,
            ChangeSource::Internal,
        )
        .await
}
#[tokio::test]
async fn protected_exact_grandchild_writes_preserve_siblings_and_reject_before_shm() {
    let native = serde_json::json!({"type":"object","properties":{"jobs":{"type":"object","additionalProperties":{"$ref":"#/$defs/Job"}}},"required":["jobs"],"additionalProperties":false,"$defs":{"Job":{"type":"object","properties":{"children":{"type":"object","additionalProperties":{"type":"object","properties":{"ready":{"type":"boolean"}},"required":["ready"],"additionalProperties":false}}},"required":["children"],"additionalProperties":false}}});
    let schema = PluginSchema::builder("incus")
        .version("2.0.0")
        .field(
            "native",
            FieldSchema {
                field_type: FieldType::exact_json_schema(native).unwrap(),
                required: true,
                description: String::new(),
                default: None,
                example: None,
                constraints: vec![],
                read_only: false,
                read_only_when: None,
            },
        )
        .build();
    let scope = support::CatalogScope::new(&[schema]);
    let engine = engine();
    let initial = json!({"native":{"jobs":{"first":{"children":{"grandchild":{"ready":false}}},"sibling":{"children":{"keep":{"ready":true}}}}}});
    change(&engine, "", None, initial).await.unwrap();
    let mut rx = engine.change_tx().subscribe();
    change(
        &engine,
        "/native/jobs/first/children/grandchild",
        Some("ready"),
        json!(true),
    )
    .await
    .unwrap();
    assert_eq!(rx.try_recv().unwrap().new_value, json!(null)); // wakeup contains no private state
    let path = scope.root().join("state/incus.json");
    let manifest = scope.root().join("state/.manifest.json");
    let before = std::fs::read(&path).unwrap();
    let generation = std::fs::read(&manifest).unwrap();
    let committed: serde_json::Value = serde_json::from_slice(&before).unwrap();
    assert_eq!(
        committed["native"]["jobs"]["first"]["children"]["grandchild"]["ready"],
        true
    );
    assert_eq!(
        committed["native"]["jobs"]["sibling"]["children"]["keep"]["ready"],
        true
    );
    let cache = engine.get_state("incus").await;
    assert!(
        change(
            &engine,
            "/native/jobs/first/children/grandchild",
            Some("ready"),
            json!("PRIVATE_INVALID")
        )
        .await
        .is_err()
    );
    assert_eq!(std::fs::read(path).unwrap(), before);
    assert_eq!(std::fs::read(manifest).unwrap(), generation);
    assert_eq!(engine.get_state("incus").await, cache);
    assert!(matches!(rx.try_recv(), Err(TryRecvError::Empty)));
    assert!(engine.get_public_state("incus").await.is_none());
}
