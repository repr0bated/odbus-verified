//! Contract tests for the fresh bridge (the `src/lib.rs` design):
//! journal-every-accept, verify-by-recompute, catalog hash on every frame,
//! and rejections that journal nothing.

use op_grpc_bridge::catalog::{Catalog, PluginEntry, PluginMethod};
use op_grpc_bridge::core::{BridgeCore, CoreMutation, CoreOperation};
use op_grpc_bridge::proto::dbus_passthrough_client::DbusPassthroughClient;
use op_grpc_bridge::proto::event_chain_service_client::EventChainServiceClient;
use op_grpc_bridge::proto::event_chain_service_server::EventChainService;
use op_grpc_bridge::proto::ovsdb_mirror_client::OvsdbMirrorClient;
use op_grpc_bridge::proto::plugin_service_client::PluginServiceClient;
use op_grpc_bridge::proto::plugin_service_server::PluginService;
use op_grpc_bridge::proto::runtime_mirror_client::RuntimeMirrorClient;
use op_grpc_bridge::proto::state_sync_client::StateSyncClient;
use op_grpc_bridge::proto::state_sync_server::StateSync;
use op_grpc_bridge::proto::{DbusCallRequest, MutateRequest, OperationType, OvsdbEchoRequest};
use tonic_reflection::pb::v1::server_reflection_client::ServerReflectionClient;
use tonic_reflection::pb::v1::{ServerReflectionRequest, server_reflection_request};
use tokio_stream::StreamExt;

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

#[test]
fn atomic_batch_rejects_nontransactional_handlers_before_any_effect() {
    use op_grpc_bridge::{Handler, RouterError};
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    let core = BridgeCore::new(test_catalog());
    let calls = Arc::new(AtomicUsize::new(0));
    let handler_calls = calls.clone();
    core.register_handler(
        "demo.plugin",
        "Ping",
        Handler::new(move |_| {
            handler_calls.fetch_add(1, Ordering::SeqCst);
            Err(RouterError::HandlerFailed {
                plugin_id: "demo.plugin".into(),
                method: "Ping".into(),
                message: "runtime failure".into(),
            })
        }),
    );
    let mut states = core.subscribe_state();
    let mut events = core.subscribe_events();
    let outcome = core.batch_mutate(
        vec![
            set_property_op("volume", serde_json::json!(7)),
            CoreMutation {
                operation: CoreOperation::CallMethod,
                member_name: "Ping".into(),
                ..set_property_op("", serde_json::Value::Null)
            },
        ],
        true,
    );

    assert_eq!(outcome.failed_index, Some(1));
    assert!(outcome.results.iter().all(Result::is_err));
    assert_eq!(calls.load(Ordering::SeqCst), 0, "handler never ran");
    assert!(core.get_object("demo.plugin", "/demo/obj").is_none());
    assert!(core.journal().is_empty());
    assert!(states.try_recv().is_err());
    assert!(events.try_recv().is_err());
}

#[test]
fn atomic_state_batch_commits_all_or_rejects_all() {
    let core = BridgeCore::new(test_catalog());
    let accepted = core.batch_mutate(
        vec![
            set_property_op("volume", serde_json::json!(7)),
            CoreMutation {
                operation: CoreOperation::ApplyPatch,
                ..set_property_op("", serde_json::json!({"muted": true}))
            },
        ],
        true,
    );
    assert_eq!(accepted.failed_index, None);
    assert!(accepted.results.iter().all(Result::is_ok));
    assert_eq!(
        core.get_property("demo.plugin", "/demo/obj", "volume"),
        Some(serde_json::json!(7)),
    );
    assert_eq!(
        core.get_property("demo.plugin", "/demo/obj", "muted"),
        Some(serde_json::json!(true)),
    );

    let rejected = core.batch_mutate(
        vec![
            set_property_op("volume", serde_json::json!(9)),
            set_property_op("", serde_json::json!(false)),
        ],
        true,
    );
    assert_eq!(rejected.failed_index, Some(1));
    assert!(rejected.results.iter().all(Result::is_err));
    assert_eq!(
        core.get_property("demo.plugin", "/demo/obj", "volume"),
        Some(serde_json::json!(7)),
    );
    assert_eq!(core.at_event_id(), 2);
}

async fn next_stream_item<T>(
    stream: &mut tokio_stream::wrappers::ReceiverStream<Result<T, tonic::Status>>,
) -> Result<T, tonic::Status> {
    tokio::time::timeout(std::time::Duration::from_secs(5), stream.next())
        .await
        .expect("stream produced an item")
        .expect("stream remains open")
}

#[tokio::test]
async fn state_hydration_keeps_updates_received_while_output_is_full() {
    use op_grpc_bridge::core::proto_value_to_json;
    use op_grpc_bridge::proto::{StateFrameKind, SubscribeRequest};
    use op_grpc_bridge::service::BridgeService;

    let core = BridgeCore::new(test_catalog());
    // More than the service's 128 output slots: hydration must backpressure.
    for index in 0..130 {
        core.mutate(CoreMutation {
            object_path: format!("/demo/{index:03}"),
            ..set_property_op("volume", serde_json::json!(0))
        })
        .expect("seed object");
    }
    let service = BridgeService::new(core.clone());
    let mut stream = service
        .subscribe(tonic::Request::new(SubscribeRequest {
            include_initial_state: true,
            ..Default::default()
        }))
        .await
        .expect("subscribe")
        .into_inner();
    let first = next_stream_item(&mut stream).await.expect("first snapshot frame");
    assert_eq!(first.frame_kind, StateFrameKind::InitialState as i32);

    // The producer is blocked sending hydration. Change an object already
    // sent, so a later snapshot frame cannot accidentally cover this update.
    core.mutate(CoreMutation {
        object_path: first.object_path.clone(),
        ..set_property_op("volume", serde_json::json!(99))
    })
    .expect("concurrent update");
    for _ in 1..130 {
        let initial = next_stream_item(&mut stream).await.expect("snapshot frame");
        assert_eq!(initial.frame_kind, StateFrameKind::InitialState as i32);
    }
    let update = next_stream_item(&mut stream).await.expect("live update");
    assert_eq!(update.frame_kind, StateFrameKind::Update as i32);
    assert_eq!(update.object_path, first.object_path);
    assert_eq!(update.event_id, 131);
    assert_eq!(
        proto_value_to_json(update.new_value.as_ref().expect("new value")).as_f64(),
        Some(99.0),
    );
}

#[tokio::test]
async fn event_replay_keeps_concurrent_events_once_and_in_order() {
    use op_grpc_bridge::proto::SubscribeEventsRequest;
    use op_grpc_bridge::service::BridgeService;

    let core = BridgeCore::new(test_catalog());
    for index in 0..130 {
        core.mutate(set_property_op("volume", serde_json::json!(index)))
            .expect("seed event");
    }
    let service = BridgeService::new(core.clone());
    let mut stream = service
        .subscribe_events(tonic::Request::new(SubscribeEventsRequest {
            from_event_id: 1,
            ..Default::default()
        }))
        .await
        .expect("subscribe events")
        .into_inner();
    assert_eq!(
        next_stream_item(&mut stream).await.expect("first event").event_id,
        1,
    );
    core.mutate(set_property_op("volume", serde_json::json!(999)))
        .expect("concurrent event");
    for expected in 2..=131 {
        assert_eq!(
            next_stream_item(&mut stream).await.expect("next event").event_id,
            expected,
        );
    }
}

#[tokio::test]
async fn lagging_state_event_and_signal_streams_require_resynchronization() {
    use op_grpc_bridge::proto::{SubscribeEventsRequest, SubscribeRequest, SubscribeSignalsRequest};
    use op_grpc_bridge::service::BridgeService;

    let core = BridgeCore::new(test_catalog());
    let service = BridgeService::new(core.clone());
    let mut states = service
        .subscribe(tonic::Request::new(SubscribeRequest::default()))
        .await
        .expect("subscribe state")
        .into_inner();
    let mut events = service
        .subscribe_events(tonic::Request::new(SubscribeEventsRequest::default()))
        .await
        .expect("subscribe events")
        .into_inner();
    let mut signals = service
        .subscribe_signals(tonic::Request::new(SubscribeSignalsRequest::default()))
        .await
        .expect("subscribe signals")
        .into_inner();
    // All three receivers are registered before their RPC returns. On this
    // current-thread runtime no producer task can drain them until we await.
    for index in 0..1025 {
        core.mutate(set_property_op("volume", serde_json::json!(index)))
            .expect("publish update");
    }
    let state_error = next_stream_item(&mut states).await.expect_err("state lag is visible");
    let event_error = next_stream_item(&mut events).await.expect_err("event lag is visible");
    let signal_error = next_stream_item(&mut signals).await.expect_err("signal lag is visible");
    assert_eq!(state_error.code(), tonic::Code::DataLoss);
    assert_eq!(event_error.code(), tonic::Code::DataLoss);
    assert_eq!(signal_error.code(), tonic::Code::DataLoss);
}

#[tokio::test]
async fn reflection_and_mounts_cover_exactly_three_services() {
    let core = BridgeCore::new(test_catalog());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener");
    let endpoint = format!("http://{}", listener.local_addr().expect("addr"));
    let server = tokio::spawn(op_grpc_bridge::serve_with_listener(listener, core));

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

#[tokio::test]
async fn client_roundtrip_set_get_subscribe() {
    use op_grpc_bridge::grpc_client::{GhostbridgeCallMetadata, GrpcClientPool, RemoteOperationClient};
    use std::sync::Arc;
    use tokio_stream::StreamExt;

    let core = BridgeCore::new(test_catalog());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener");
    let endpoint = format!("http://{}", listener.local_addr().expect("addr"));
    let server = tokio::spawn(op_grpc_bridge::serve_with_listener(listener, core));

    let identity = GhostbridgeCallMetadata {
        sealed_id: Some(b"SID1-contract-test".to_vec()),
        oracle_assertion: None,
        session_genesis: None,
        trace_id: None,
        wireguard_pubkey: None,
    };
    let pool = Arc::new(GrpcClientPool::new());
    let client = RemoteOperationClient::new(pool, &endpoint, "contract-test");

    // Wait for the server, then set state through the real client.
    let mut set = None;
    let mut last_error = None;
    for _ in 0..100 {
        match client
            .set_state_with_identity(
                "demo.plugin",
                "/demo/obj",
                simd_json::json!({"volume": 7}),
                "contract-test",
                "",
                &identity,
            )
            .await
        {
            Ok(result) => {
                set = Some(result);
                break;
            }
            Err(error) => {
                last_error = Some(error);
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
        }
    }
    let set = match set {
        Some(result) => result,
        None => panic!("set_state through client never succeeded: {last_error:?}"),
    };
    assert!(set.event_id >= 1);

    let got = client
        .get_state_with_identity("demo.plugin", "/demo/obj", &identity, None)
        .await
        .expect("get_state through client");
    assert_eq!(got["volume"], simd_json::json!(7));

    // Subscribe receives the next mutation as a state frame.
    let mut stream = client
        .subscribe_with_identity(
            vec!["demo.plugin".to_string()],
            vec![],
            vec![],
            &identity,
            None,
        )
        .await
        .expect("subscribe through client");
    client
        .set_state_with_identity(
            "demo.plugin",
            "/demo/obj",
            simd_json::json!({"volume": 9}),
            "contract-test",
            "",
            &identity,
        )
        .await
        .expect("second set_state");
    let frame = tokio::time::timeout(std::time::Duration::from_secs(5), stream.next())
        .await
        .expect("frame within timeout")
        .expect("stream open")
        .expect("frame ok");
    assert_eq!(frame.plugin_id, "demo.plugin");

    server.abort();
}
