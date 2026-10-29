//! Contract tests for the fresh bridge (the `src/lib.rs` design):
//! journal-every-accept, verify-by-recompute, catalog hash on every frame,
//! and rejections that journal nothing.

use op_grpc_bridge::catalog::{Catalog, PluginEntry, PluginMethod};
use op_grpc_bridge::core::{BridgeCore, CoreMutation, CoreOperation};
use op_grpc_bridge::proto::dbus_passthrough_client::DbusPassthroughClient;
use op_grpc_bridge::proto::event_chain_service_client::EventChainServiceClient;
use op_grpc_bridge::proto::ovsdb_mirror_client::OvsdbMirrorClient;
use op_grpc_bridge::proto::plugin_service_client::PluginServiceClient;
use op_grpc_bridge::proto::runtime_mirror_client::RuntimeMirrorClient;
use op_grpc_bridge::proto::state_sync_client::StateSyncClient;
use op_grpc_bridge::proto::{DbusCallRequest, MutateRequest, OperationType, OvsdbEchoRequest};
use tonic_reflection::pb::v1::server_reflection_client::ServerReflectionClient;
use tonic_reflection::pb::v1::{ServerReflectionRequest, server_reflection_request};

fn test_catalog() -> Catalog {
    Catalog::from_entries(vec![PluginEntry {
        id: "demo.plugin".to_string(),
        name: "Demo".to_string(),
        version: "1.0.0".to_string(),
        schema_hash: "schema-abc".to_string(),
        tags: vec!["demo".to_string()],
        methods: vec![PluginMethod {
            name: "Ping".to_string(),
        }],
        ..Default::default()
    }])
    .expect("valid catalog")
}

fn set_property_op(member: &str, value: serde_json::Value) -> CoreMutation {
    CoreMutation {
        plugin_id: "demo.plugin".to_string(),
        object_path: "/demo/obj".to_string(),
        operation: CoreOperation::SetProperty,
        member_name: member.to_string(),
        value,
        actor_id: "tester".to_string(),
        capability_id: String::new(),
        idempotency_key: String::new(),
    }
}

#[test]
fn mutate_then_get_state_roundtrip() {
    let core = BridgeCore::new(test_catalog());
    let outcome = core
        .mutate(set_property_op("volume", serde_json::json!(7)))
        .expect("mutation accepted");
    assert_eq!(outcome.event.event_id, 1);

    let (map, hash) = core
        .get_object("demo.plugin", "/demo/obj")
        .expect("object present");
    assert_eq!(map.get("volume"), Some(&serde_json::json!(7)));
    assert_eq!(hash, outcome.event.result_effective_hash);
    assert_eq!(core.at_event_id(), 1);
}

#[test]
fn verify_chain_recomputes_journal() {
    let core = BridgeCore::new(test_catalog());
    for i in 0..5 {
        core.mutate(set_property_op(&format!("key{i}"), serde_json::json!(i)))
            .expect("mutation accepted");
    }

    let checked = core.journal().verify(0, 0);
    assert!(checked.valid, "errors: {:?}", checked.errors);
    assert_eq!(checked.events_verified, 5);
    assert!(checked.errors.is_empty());

    // Sub-range recompute holds too.
    let sub = core.journal().verify(2, 4);
    assert!(sub.valid, "errors: {:?}", sub.errors);
    assert_eq!(sub.events_verified, 3);

    // Linkage: genesis has no predecessor; every other event links back.
    let events = core.journal().range(0, 0);
    assert_eq!(events.len(), 5);
    assert!(events[0].prev_hash.is_empty());
    for pair in events.windows(2) {
        assert_eq!(pair[1].prev_hash, pair[0].event_hash);
    }
}

#[test]
fn state_frames_carry_catalog_hash() {
    let catalog = test_catalog();
    let expected = catalog.hash().to_string();
    let core = BridgeCore::new(catalog);

    let mut live = core.subscribe_state();
    core.mutate(set_property_op("volume", serde_json::json!(7)))
        .expect("mutation accepted");
    let frame = live.try_recv().expect("exactly one frame published");
    assert_eq!(frame.catalog_hash, expected);
    assert_eq!(frame.schema_hash, "schema-abc");
    assert_eq!(frame.event_id, 1);
    assert!(live.try_recv().is_err(), "no second frame");

    // Hydration frames carry it too.
    let initial = core.initial_frames();
    assert_eq!(initial.len(), 1);
    assert_eq!(initial[0].catalog_hash, expected);
    let schemas = core.schema_frames();
    assert_eq!(schemas.len(), 1);
    assert_eq!(schemas[0].catalog_hash, expected);
    assert_eq!(schemas[0].schema_hash, "schema-abc");
}

#[test]
fn rejected_mutations_journal_nothing() {
    let core = BridgeCore::new(test_catalog());
    let mut states = core.subscribe_state();
    let mut events = core.subscribe_events();

    // Unknown plugin.
    let bad_plugin = CoreMutation {
        plugin_id: "no.such.plugin".to_string(),
        ..set_property_op("x", serde_json::json!(1))
    };
    assert!(core.mutate(bad_plugin).is_err());

    // Unknown operation.
    let bad_op = CoreMutation {
        operation: CoreOperation::Unknown,
        ..set_property_op("x", serde_json::json!(1))
    };
    assert!(core.mutate(bad_op).is_err());

    // Declared method with no handler registered.
    let no_handler = CoreMutation {
        operation: CoreOperation::CallMethod,
        member_name: "Ping".to_string(),
        ..set_property_op("x", serde_json::json!(1))
    };
    assert!(core.mutate(no_handler).is_err());

    // Missing member name.
    assert!(
        core.mutate(set_property_op("", serde_json::json!(1)))
            .is_err()
    );

    assert!(core.journal().is_empty());
    assert_eq!(core.at_event_id(), 0);
    assert!(core.get_object("demo.plugin", "/demo/obj").is_none());
    assert!(states.try_recv().is_err(), "no state frame published");
    assert!(events.try_recv().is_err(), "no chain event published");
}

#[tokio::test]
async fn reflection_and_mounts_cover_exactly_three_services() {
    let core = BridgeCore::new(test_catalog());
    let probe = std::net::TcpListener::bind("127.0.0.1:0").expect("free port");
    let port = probe.local_addr().expect("addr").port();
    drop(probe);
    let addr: std::net::SocketAddr = format!("127.0.0.1:{port}").parse().expect("addr");
    let server = tokio::spawn(op_grpc_bridge::serve(addr, core));
    let endpoint = format!("http://127.0.0.1:{port}");

    let mut reflection = None;
    for _ in 0..100 {
        if let Ok(ep) = tonic::transport::Endpoint::new(endpoint.clone()) {
            if let Ok(channel) = ep.connect().await {
                reflection = Some(ServerReflectionClient::new(channel));
                break;
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    let mut reflection = reflection.expect("server came up");

    // Reflection names the served contract. (The descriptor also defines the
    // generated-but-unmounted mirrors, so this is a subset check; the live
    // probes below prove the mount boundary.)
    let (tx, rx) = tokio::sync::mpsc::channel(4);
    tx.send(ServerReflectionRequest {
        host: String::new(),
        message_request: Some(
            server_reflection_request::MessageRequest::ListServices(String::new()),
        ),
    })
    .await
    .expect("request send");
    let mut inbound = reflection
        .server_reflection_info(tokio_stream::wrappers::ReceiverStream::new(rx))
        .await
        .expect("reflection call")
        .into_inner();
    let reply = inbound.message().await.expect("stream ok").expect("response");
    let names: Vec<String> = match reply.message_response {
        Some(
            tonic_reflection::pb::v1::server_reflection_response::MessageResponse::ListServicesResponse(
                list,
            ),
        ) => list.service.into_iter().map(|s| s.name).collect(),
        other => panic!("expected service list, got {other:?}"),
    };
    for expected in [
        "operation.v1.StateSync",
        "operation.v1.PluginService",
        "operation.v1.EventChainService",
    ] {
        assert!(
            names.contains(&expected.to_string()),
            "missing {expected}; got {names:?}"
        );
    }

    // Mounts: the three served services answer, the other three fail.
    let mut sync = StateSyncClient::connect(endpoint.clone())
        .await
        .expect("connect");
    let mutated = sync
        .mutate(tonic::Request::new(MutateRequest {
            plugin_id: "demo.plugin".to_string(),
            object_path: "/demo/obj".to_string(),
            operation: OperationType::SetProperty as i32,
            member_name: "volume".to_string(),
            value: Some(prost_types::Value {
                kind: Some(prost_types::value::Kind::NumberValue(7.0)),
            }),
            ..Default::default()
        }))
        .await
        .expect("mutate rpc")
        .into_inner();
    assert!(mutated.success);

    let mut plugins = PluginServiceClient::connect(endpoint.clone())
        .await
        .expect("connect");
    let listed = plugins
        .list_plugins(tonic::Request::new(()))
        .await
        .expect("list rpc")
        .into_inner();
    assert_eq!(listed.plugins.len(), 1);

    let mut chain = EventChainServiceClient::connect(endpoint.clone())
        .await
        .expect("connect");
    let events = chain
        .get_events(tonic::Request::new(
            op_grpc_bridge::proto::GetEventsRequest::default(),
        ))
        .await
        .expect("events rpc")
        .into_inner();
    assert_eq!(events.events.len(), 1);

    let mut ovsdb = OvsdbMirrorClient::connect(endpoint.clone())
        .await
        .expect("connect");
    assert!(
        ovsdb
            .echo(tonic::Request::new(OvsdbEchoRequest {
                payload: vec!["ping".to_string()],
            }))
            .await
            .is_err(),
        "OvsdbMirror must not be mounted"
    );

    let mut runtime = RuntimeMirrorClient::connect(endpoint.clone())
        .await
        .expect("connect");
    assert!(
        runtime
            .get_system_info(tonic::Request::new(()))
            .await
            .is_err(),
        "RuntimeMirror must not be mounted"
    );

    let mut passthrough = DbusPassthroughClient::connect(endpoint.clone())
        .await
        .expect("connect");
    assert!(
        passthrough
            .call(tonic::Request::new(DbusCallRequest::default()))
            .await
            .is_err(),
        "DbusPassthrough must not be mounted"
    );

    server.abort();
}
