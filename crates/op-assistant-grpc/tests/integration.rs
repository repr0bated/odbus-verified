//! Integration tests for op-assistant-grpc.
//!
//! These hit the in-process CognitiveMemoryStore + SoulMemoryStore directly
//! through the service implementations (in-memory Cozo). Production serve
//! uses D-Bus PluginV1.Call to cognitive-mcp instead — see cognitive_client.rs.

use op_assistant_grpc::memory::{ensure_namespace, MemoryServiceImpl};
use op_assistant_grpc::namespace::NamespaceMemoryServiceImpl;
use op_assistant_grpc::proto::memory_service_server::MemoryService;
use op_assistant_grpc::proto::namespace_memory_service_server::NamespaceMemoryService;
use op_assistant_grpc::proto::soul_service_server::SoulService;
use op_assistant_grpc::proto::{
    GetSoulMemoryRequest, MemoryEntry, ReadMemoryRequest, SetMemoryNamespaceRequest,
    UpdateSoulMemoryRequest, WriteMemoryRequest,
};
use op_assistant_grpc::soul::SoulServiceImpl;
use op_cognitive_mcp::cozo_shuttle::CozoGraphShuttle;
use op_cognitive_mcp::memory_store::{CognitiveMemoryStore, NamespaceKind};
use op_cognitive_mcp::soul_memory::SoulMemoryStore;
use std::sync::Arc;
use tonic::Request;

async fn fixture() -> (Arc<CognitiveMemoryStore>, Arc<SoulMemoryStore>) {
    let shuttle = Arc::new(CozoGraphShuttle::new_in_memory().expect("cozo in-memory"));
    let memory = Arc::new(
        CognitiveMemoryStore::new(shuttle.clone())
            .await
            .expect("memory store"),
    );
    let soul = Arc::new(SoulMemoryStore::new(shuttle));
    (memory, soul)
}

#[tokio::test]
async fn write_then_read_memory_entry() {
    let (memory, _soul) = fixture().await;
    let svc = MemoryServiceImpl::new(memory.clone());

    ensure_namespace(&memory, "demo", NamespaceKind::Custom)
        .await
        .unwrap();

    let entry = MemoryEntry {
        id: String::new(),
        namespace: "demo".into(),
        key: "k1".into(),
        value: "\"hello\"".into(),
        metadata: None,
        created_at: None,
        updated_at: None,
    };
    let wr = svc
        .write_memory(Request::new(WriteMemoryRequest {
            namespace: "demo".into(),
            entries: vec![entry],
        }))
        .await
        .unwrap();
    assert_eq!(wr.into_inner().written, 1);

    let rd = svc
        .read_memory(Request::new(ReadMemoryRequest {
            namespace: "demo".into(),
            keys: vec!["k1".into()],
            pagination: None,
        }))
        .await
        .unwrap();
    let entries = rd.into_inner().entries;
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].key, "k1");
}

#[tokio::test]
async fn soul_upsert_and_get() {
    let (_memory, soul) = fixture().await;
    let svc = SoulServiceImpl::new(soul.clone());

    let updated = svc
        .update_soul_memory(Request::new(UpdateSoulMemoryRequest {
            agent_id: "agent-a".into(),
            identity: Some("identity-1".into()),
            personality: Some("calm".into()),
            traits: None,
        }))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(updated.identity, "identity-1");
    assert_eq!(updated.version, 1);

    let got = svc
        .get_soul_memory(Request::new(GetSoulMemoryRequest {
            agent_id: "agent-a".into(),
        }))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(got.personality, "calm");

    // Second update bumps version.
    let bumped = svc
        .update_soul_memory(Request::new(UpdateSoulMemoryRequest {
            agent_id: "agent-a".into(),
            identity: None,
            personality: Some("focused".into()),
            traits: None,
        }))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(bumped.version, 2);
    assert_eq!(bumped.identity, "identity-1"); // preserved
}

#[tokio::test]
async fn namespace_binding_round_trip() {
    let (memory, soul) = fixture().await;
    let svc = NamespaceMemoryServiceImpl::new(memory.clone(), soul.clone());

    let bound = svc
        .set_memory_namespace(Request::new(SetMemoryNamespaceRequest {
            agent_id: "agent-b".into(),
            namespace: "ns-b".into(),
        }))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(bound.namespace, "ns-b");
    assert_eq!(bound.agent_id, "agent-b");
}
