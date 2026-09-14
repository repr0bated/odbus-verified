//! Live probe: distinguish frozen per-method descriptors being *advertised* from *served*.
//!
//! Sealing a plugin's descriptors into the SHM blob and hydrating the reflection
//! catalog only makes them **discoverable**. Mounting them as callable typed gRPC
//! services requires `freeze_plugin_method_reflection()` to run before routes are
//! built, because tonic-reflection is immutable once mounted.
//!
//! Regression this guards: `run_tched_router_server` (the path the `op-grpc-bridge`
//! binary actually takes) called `hydrate_reflection_from_shm()` but not
//! `freeze_plugin_method_reflection()`, so the bridge advertised every sealed
//! plugin while serving none of their per-method services. `cognitive_mcp` is
//! bridge-owned and compiled in, so activation must remain independent of the
//! deliberately absent standalone service definition.
//!
//! Ignored by default: requires a running bridge.
//!   cargo test -p op-grpc-bridge --test live_reflection_probe -- --nocapture --ignored

#[tokio::test]
#[ignore = "requires a running bridge on 127.0.0.1:50051"]
async fn cognitive_mcp_per_method_services_are_advertised() {
    use tonic_reflection::pb::v1::{
        server_reflection_client::ServerReflectionClient,
        server_reflection_request::MessageRequest, server_reflection_response::MessageResponse,
        ServerReflectionRequest,
    };

    let channel = tonic::transport::Channel::from_static("http://127.0.0.1:50051")
        .connect()
        .await
        .expect("connect to live bridge :50051");

    let mut client = ServerReflectionClient::new(channel);
    let (tx, rx) = tokio::sync::mpsc::channel(16);
    let request_stream = tokio_stream::wrappers::ReceiverStream::new(rx);
    let mut stream = client
        .server_reflection_info(tonic::Request::new(request_stream))
        .await
        .expect("open reflection stream")
        .into_inner();

    tx.send(ServerReflectionRequest {
        host: String::new(),
        message_request: Some(MessageRequest::ListServices(String::new())),
    })
    .await
    .expect("send ListServices");

    let names: Vec<String> = match stream.message().await.expect("reflection response") {
        Some(resp) => match resp.message_response {
            Some(MessageResponse::ListServicesResponse(r)) => {
                r.service.into_iter().map(|s| s.name).collect()
            }
            other => panic!("unexpected reflection response: {other:?}"),
        },
        None => panic!("no reflection response"),
    };

    let cognitive: Vec<&String> = names
        .iter()
        .filter(|n| n.contains("cognitive_mcp"))
        .collect();

    println!("total services: {}", names.len());
    println!("cognitive_mcp services: {}", cognitive.len());
    for n in &cognitive {
        println!("  {n}");
    }

    assert!(
        !cognitive.is_empty(),
        "no cognitive_mcp per-method services mounted — descriptors were sealed but \
         never activated (is freeze_plugin_method_reflection running before routes \
         are built, and is the plugin reported available?)"
    );

    assert!(
        names
            .iter()
            .any(|n| n == "operation.method.cognitive_mcp.invoke_tool.InvokeToolService"),
        "invoke_tool's typed service is not advertised; found: {cognitive:?}"
    );
}

/// The generated plugin-level service **is** served over tonic-web.
///
/// Two distinct naming schemes coexist, and conflating them is easy:
///
///   served    `operation.plugin.v1.{Plugin}PluginMethods/{Rpc}`
///             generated at compile time by build.rs from the plugin schemas,
///             dispatching through `call_generated_plugin_method_typed` which
///             decodes the request with prost-reflect, hands JSON to
///             MutationEngine, and re-encodes the response.
///
///   reflection-only  `operation.method.{plugin}.{method}.{Svc}/{Rpc}`
///             the per-method frozen descriptors from the sealed blob. Registered
///             with the reflection registry so they are discoverable, but no
///             handler is mounted for these names.
///
/// So the cognitive surface *is* reachable from gRPC and gRPC-Web clients — under
/// the first scheme. Asserting against the second yields a misleading UNIMPLEMENTED.
///
/// This probe sends a valid `tool_name` and expects the call to reach the
/// capability gate (status 7), proving routing + protobuf decode + dispatch all
/// work. Status 12 would mean the service is not mounted at all.
#[tokio::test]
#[ignore = "requires a running bridge on 127.0.0.1:8090"]
async fn cognitive_mcp_plugin_service_is_served_over_tonic_web() {
    // CognitiveMcpInvokeToolRequest { string tool_name = 322087922; }
    // varint tag for field 322087922, wire type 2 -> 92 BF D5 CC 09
    let mut msg: Vec<u8> = vec![0x92, 0xBF, 0xD5, 0xCC, 0x09];
    let tool = b"get_health";
    msg.push(tool.len() as u8);
    msg.extend_from_slice(tool);

    // gRPC-Web frame: 1 flag byte + 4-byte big-endian length + payload
    let mut body: Vec<u8> = vec![0x00];
    body.extend_from_slice(&(msg.len() as u32).to_be_bytes());
    body.extend_from_slice(&msg);

    let resp = reqwest::Client::new()
        .post("http://127.0.0.1:8090/operation.plugin.v1.CognitiveMcpPluginMethods/InvokeTool")
        .header("Content-Type", "application/grpc-web+proto")
        .body(body)
        .send()
        .await
        .expect("post to tonic-web :8090");

    let status = resp
        .headers()
        .get("grpc-status")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("<absent>")
        .to_string();
    let message = resp
        .headers()
        .get("grpc-message")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    assert_ne!(
        status, "12",
        "CognitiveMcpPluginMethods/InvokeTool returned UNIMPLEMENTED — the generated \
         plugin service is not mounted (grpc-message: {message})"
    );

    // Reaching the capability gate proves routing, protobuf decode and dispatch ran.
    // The gRPC path requires the caller to *declare* capability_id, not merely hold
    // the grant, so an undeclared call is expected to be denied here.
    assert!(
        status == "7" || status == "0",
        "unexpected grpc-status {status} (grpc-message: {message})"
    );
}
