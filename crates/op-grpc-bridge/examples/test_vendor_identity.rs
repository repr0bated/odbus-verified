//! Throwaway tool: exercise the per-identity interceptor path directly with
//! caller-supplied headers (not the host's own, via attach_ghostbridge_metadata),
//! to confirm a provisioned vendor/partner identity (e.g. Lovable) is actually
//! accepted — and that a wrong or expired one is actually rejected.
use op_grpc_bridge::proto::plugin_service_client::PluginServiceClient;
use tonic::metadata::MetadataValue;
use tonic::Request;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let genesis = std::env::args()
        .nth(1)
        .expect("usage: test_vendor_identity <genesis_hex> <trace_id>");
    let trace_id = std::env::args()
        .nth(2)
        .expect("usage: test_vendor_identity <genesis_hex> <trace_id>");

    let mut client = PluginServiceClient::connect("http://127.0.0.1:8090").await?;
    let mut request = Request::new(());
    request
        .metadata_mut()
        .insert("x-ghostbridge-genesis", MetadataValue::try_from(genesis)?);
    request
        .metadata_mut()
        .insert("x-ghostbridge-trace-id", MetadataValue::try_from(trace_id)?);

    match client.list_plugins(request).await {
        Ok(resp) => println!("ACCEPTED: {} plugins", resp.into_inner().plugins.len()),
        Err(status) => println!("REJECTED: {} — {}", status.code(), status.message()),
    }
    Ok(())
}
