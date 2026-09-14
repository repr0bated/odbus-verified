//! Throwaway tool to update an already-provisioned identity through
//! identity_sled's write_identity via the real
//! PluginService.CallMethod gRPC path, so provisioning goes through the
//! authoritative mutation/notarization pipeline instead of manual patches.
use op_grpc_bridge::grpc_client::{GrpcClientPool, RemoteOperationClient};
use std::sync::Arc;

const USAGE: &str = "usage: provision_identity <wireguard_pubkey>";

fn parse_args<I>(args: I) -> anyhow::Result<String>
where
    I: IntoIterator<Item = String>,
{
    let mut args = args.into_iter();
    let _program = args.next();
    let pubkey = args.next().ok_or_else(|| anyhow::anyhow!(USAGE))?;
    if args.next().is_some() {
        anyhow::bail!("unexpected extra argument; {USAGE}");
    }
    Ok(pubkey)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let pubkey = parse_args(std::env::args())?;

    let pool = Arc::new(GrpcClientPool::new());
    let client =
        RemoteOperationClient::new(pool, "http://127.0.0.1:8090", "provision-identity-tool");

    let args = simd_json::json!({"wireguard_pubkey": pubkey});

    let result = client
        .call_method(
            "identity_sled",
            "",
            "",
            "write_identity",
            vec![args],
            "provision-identity-tool",
            "identity_sled.write",
        )
        .await?;

    println!("{}", simd_json::to_string_pretty(&result)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::parse_args;

    fn argv(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    #[test]
    fn accepts_pubkey() {
        assert_eq!(
            parse_args(argv(&["provision_identity", "pubkey"])).unwrap(),
            "pubkey"
        );
    }

    #[test]
    fn rejects_extra_arguments() {
        assert!(parse_args(argv(&["provision_identity", "pubkey", "3600"])).is_err());
    }
}
