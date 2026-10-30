// Compatibility gate for embedded CognitiveToolService consumers. The active
// external MCP gateway and its authentication policy live in op-grpc-bridge.
// Authentication here resolves one projected session and verifies the request's
// genesis against that record.

use op_identity::SessionGenesisVerifyError;
use tonic::{Request, Status};

/// Identity extracted by the Ghostbridge interceptor and attached to each
/// accepted request for downstream authorization / trace linking.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GhostbridgeIdentity {
    pub principal_id: String,
    pub session_id: String,
    pub session_genesis: String,
}

#[allow(clippy::result_large_err)]
pub fn ghostbridge_interceptor(mut req: Request<()>) -> Result<Request<()>, Status> {
    let genesis_value = req.metadata().get("x-ghostbridge-genesis").cloned();
    let trace_value = req.metadata().get("x-ghostbridge-trace-id").cloned();
    let pubkey_value = req.metadata().get("x-wireguard-pubkey").cloned();

    if genesis_value.is_none() || (trace_value.is_none() && pubkey_value.is_none()) {
        return Err(Status::unauthenticated(
            "Missing Ghostbridge session identity.",
        ));
    }

    let request_genesis = genesis_value
        .as_ref()
        .unwrap()
        .to_str()
        .map_err(|_| Status::invalid_argument("Invalid genesis header encoding"))?;
    let trace_id = trace_value.as_ref().and_then(|value| value.to_str().ok());
    let wireguard_pubkey = pubkey_value.as_ref().and_then(|value| value.to_str().ok());

    let identity =
        op_identity::resolve_verified_session(request_genesis, trace_id, wireguard_pubkey)
            .map_err(|error| match error {
                SessionGenesisVerifyError::SledUnreachable => {
                    Status::internal("Identity session projection unreachable")
                }
                SessionGenesisVerifyError::InvalidSled => {
                    Status::failed_precondition("Identity session is not anchored")
                }
                SessionGenesisVerifyError::Inactive => {
                    Status::permission_denied("Identity session is inactive")
                }
                SessionGenesisVerifyError::Expired => {
                    Status::permission_denied("Identity session term has expired")
                }
                SessionGenesisVerifyError::Mismatch => Status::permission_denied(
                    "Session genesis does not match the projected identity record.",
                ),
            })?;

    req.extensions_mut().insert(GhostbridgeIdentity {
        principal_id: op_identity::session::derive_principal_id(&identity.wireguard_pubkey),
        session_id: identity.session_id,
        session_genesis: request_genesis.to_string(),
    });

    Ok(req)
}
