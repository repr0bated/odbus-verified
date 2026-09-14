use op_grpc_bridge::proto::dbus_passthrough_client::DbusPassthroughClient;
use op_grpc_bridge::proto::DbusCallRequest;
use tonic::metadata::MetadataValue;
use tonic::Request;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let genesis = std::env::args()
        .nth(1)
        .expect("usage: test_dbus_passthrough <genesis_hex> <trace_id>");
    let trace_id = std::env::args()
        .nth(2)
        .expect("usage: test_dbus_passthrough <genesis_hex> <trace_id>");

    let mut client = DbusPassthroughClient::connect("http://127.0.0.1:8090").await?;
    let mut request = Request::new(DbusCallRequest {
        bus: "system".to_string(),
        destination: "ai.assistant.v1".to_string(),
        path: "/ai/assistant".to_string(),
        interface: "ai.assistant.v1".to_string(),
        method: "ListSessions".to_string(),
        json_body: "{}".to_string(),
    });
    request
        .metadata_mut()
        .insert("x-ghostbridge-genesis", MetadataValue::try_from(genesis)?);
    request
        .metadata_mut()
        .insert("x-ghostbridge-trace-id", MetadataValue::try_from(trace_id)?);

    match client.call(request).await {
        Ok(resp) => println!("OK: {:?}", resp.into_inner()),
        Err(status) => println!("ERROR: {} — {}", status.code(), status.message()),
    }
    Ok(())
}
