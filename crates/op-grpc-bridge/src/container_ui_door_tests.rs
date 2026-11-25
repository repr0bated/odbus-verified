use super::*;
use crate::catalog::{Catalog, PluginEntry};
use axum::body::Body;
use axum::http::{Request as HttpRequest, header};
use std::sync::Mutex;
use tower::ServiceExt;

const PLUGIN: &str = "demo";
const CAP: &str = "cap.demo.read@v1";

fn schema() -> PluginSchema {
    serde_json::from_value(json!({
        "name": PLUGIN, "version": "1.0.0", "public_state": {"paths": ["/shown"]},
        "methods": {"read": {
            "name": "read", "subid": "demo.read", "side_effect": "read",
            "idempotent": true, "required_capability": CAP,
            "args": {"type": "object"}, "returns": {"type": "object"}
        }}
    }))
    .unwrap()
}

type Outcome = fn() -> Result<Value>;
type Calls = Arc<Mutex<Vec<(String, Value)>>>;

struct Fake(Calls, Outcome);
impl UiNamespace for Fake {
    fn view(&self) -> Result<Value> {
        Ok(json!({"objects": {PLUGIN: "hash"}}))
    }
    fn schema(&self, _plugin: &str) -> Result<PluginSchema> {
        Ok(schema())
    }
    fn state(&self, _plugin: &str) -> Result<Value> {
        Ok(json!({"shown": 1}))
    }
    fn invoke(&self, path: &str, arguments: Value) -> Result<Value> {
        self.0.lock().unwrap().push((path.to_owned(), arguments));
        (self.1)()
    }
}

fn completed() -> Result<Value> {
    Ok(json!({"plugin_id": PLUGIN, "method": "read", "result": {
        "success": true, "outcome": "completed", "operation_id": "op-1",
        "terminal_event_id": 7, "terminal_event_hash": "ab", "result": {"value": "seen"}
    }}))
}
fn diagnostic(stage: &str) -> Result<Value> {
    Err(MountedDiagnostic(json!({"plugin": PLUGIN, "method": "read", "stage": stage,
        "error": "method admission refused", "operation_id": "op-2"}))
    .into())
}

fn service(outcome: Outcome) -> (UiService, Calls) {
    let calls = Calls::default();
    let seen = calls.clone();
    let entry: PluginEntry = serde_json::from_value(json!({
        "id": PLUGIN, "schema_hash": "s",
        "dbus_path": op_plugins::canonical::plugin_path(PLUGIN),
        "schema": serde_json::to_value(schema()).unwrap(),
    }))
    .unwrap();
    let core = BridgeCore::new(Catalog::from_entries(vec![entry]).unwrap());
    let open: Open = Arc::new(move || Ok(Box::new(Fake(seen.clone(), outcome)) as Box<dyn UiNamespace>));
    (UiService::with_opener(core, open), calls)
}

fn call() -> Request<CallMethodRequest> {
    Request::new(CallMethodRequest {
        plugin_id: PLUGIN.into(),
        method_name: "read".into(),
        ..Default::default()
    })
}

#[tokio::test]
async fn call_is_one_call_file_write_and_returns_its_receipt() {
    let (service, calls) = service(completed);
    let response = service.call_method(call()).await.unwrap();
    assert_eq!(response.metadata().get("x-opdbus-operation-id").unwrap(), "op-1");
    let body = response.into_inner();
    assert!(body.success && body.error.is_none());
    assert_eq!((body.event_id, body.event_hash.as_str()), (7, "ab"));
    assert_eq!(proto_value_to_json(&body.result.unwrap()), json!({"value": "seen"}));
    assert_eq!(
        *calls.lock().unwrap(),
        [("plugins/demo/operations/read/call".to_owned(), json!({}))]
    );
}

#[tokio::test]
async fn refusal_is_returned_as_the_receipt() {
    let (service, calls) = service(|| diagnostic("call"));
    let body = service.call_method(call()).await.unwrap().into_inner();
    assert!(!body.success);
    assert_eq!(body.error.unwrap().code, ErrorCode::PermissionDenied as i32);
    assert_eq!(proto_value_to_json(&body.result.unwrap())["operation_id"], "op-2");
    assert_eq!(calls.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn unknown_outcome_is_neither_success_refusal_nor_retried() {
    for outcome in [
        (|| diagnostic("outcome_unknown")) as Outcome,
        || diagnostic("transport"),
        || anyhow::bail!("receipt unreadable"),
    ] {
        let (service, calls) = service(outcome);
        let body = service.call_method(call()).await.unwrap().into_inner();
        assert!(!body.success && body.error.is_none());
        assert_eq!(calls.lock().unwrap().len(), 1);
    }
}

#[tokio::test]
async fn capability_must_match_the_sealed_one() {
    let (service, calls) = service(completed);
    let mut request = call();
    request
        .metadata_mut()
        .insert("x-opdbus-capability", "cap.other@v1".parse().unwrap());
    let status = service.call_method(request).await.unwrap_err();
    assert_eq!(status.code(), tonic::Code::PermissionDenied);
    assert!(calls.lock().unwrap().is_empty());
    let mut request = call();
    request
        .metadata_mut()
        .insert("x-opdbus-capability", CAP.parse().unwrap());
    assert!(service.call_method(request).await.unwrap().into_inner().success);
}

#[tokio::test]
async fn caller_operation_id_is_refused() {
    let (service, calls) = service(completed);
    let mut request = call();
    request
        .metadata_mut()
        .insert("x-opdbus-operation-id", "op".parse().unwrap());
    assert!(service.call_method(request).await.is_err());
    assert!(calls.lock().unwrap().is_empty());
}

#[test]
fn identity_metadata_is_refused() {
    for name in [
        op_identity::sealed_id::HTTP_HEADER_NAME,
        "x-oracle-identity-assertion-bin",
    ] {
        let mut request = Request::new(());
        request.metadata_mut().insert_bin(
            name,
            tonic::metadata::MetadataValue::from_bytes(b"identity"),
        );
        assert_eq!(
            refuse_identity(request).unwrap_err().code(),
            tonic::Code::Unauthenticated
        );
    }
    assert!(refuse_identity(Request::new(())).is_ok());
}

#[tokio::test]
async fn other_rpcs_are_unimplemented() {
    let (service, calls) = service(completed);
    let app = router(service);
    for path in [
        "operation.v1.StateSync/Mutate",
        "operation.v1.StateSync/BatchMutate",
        "operation.v1.StateSync/GetState",
        "operation.v1.PluginService/SetProperty",
        "operation.v1.PluginService/GetProperty",
        "operation.v1.EventChainService/GetEvents",
    ] {
        let response = app
            .clone()
            .oneshot(
                HttpRequest::post(format!("/{path}"))
                    .header(header::CONTENT_TYPE, "application/grpc")
                    .body(Body::from(vec![0u8; 5]))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.headers()["grpc-status"], "12", "{path}");
    }
    assert!(calls.lock().unwrap().is_empty());
}

#[test]
fn door_stays_off_unless_given_its_own_guest_local_address() {
    let identity: SocketAddr = "10.0.0.2:8090".parse().unwrap();
    assert_eq!(parse_listen(None, identity).unwrap(), None);
    assert_eq!(parse_listen(Some("10.0.0.2:8090"), identity).unwrap(), None);
    assert!(parse_listen(Some("10.0.0.2:8443"), identity).unwrap().is_some());
    assert!(parse_listen(Some("127.0.0.1:8443"), identity).unwrap().is_some());
    for exposed in ["0.0.0.0:8443", "10.0.0.4:8443", "[::]:8443"] {
        assert!(parse_listen(Some(exposed), identity).is_err());
    }
}

#[test]
fn state_frames_carry_only_the_mounts_projection() {
    let (_, calls) = service(completed);
    let namespace = Fake(calls, completed);
    let view = namespace.view().unwrap();
    let frame = |plugin: &str| StateChange {
        plugin_id: plugin.into(),
        new_value: Some(json_to_proto_value(&json!({"shown": 0, "hidden": 1}))),
        state_generation: 3,
        ..Default::default()
    };
    let shown = UiService::mounted_frame(&namespace, &view, frame(PLUGIN))
        .unwrap()
        .unwrap();
    assert_eq!(
        proto_value_to_json(&shown.new_value.unwrap()),
        json!({"shown": 1})
    );
    assert_eq!(shown.state_generation, 3);
    assert!(UiService::mounted_frame(&namespace, &view, frame("absent")).unwrap().is_none());
}
