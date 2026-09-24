//! Admission for the standalone compatibility transport.

use std::sync::Arc;

use tonic::{Request, Status};

pub const SEALED_ID_METADATA_KEY: &str = op_identity::sealed_id::HTTP_HEADER_NAME;
const ASSERTION_METADATA_KEY: &str = "x-oracle-identity-assertion-bin";

#[tonic::async_trait]
pub trait CredentialVerifier: Send + Sync {
    async fn verify(&self, wire: &[u8], capability: &str) -> Result<(), Status>;
}

/// The bridge's complete SID1 receiver and principal registry cannot be
/// called by this separate process: the bridge owns an exclusive Cozo store
/// lock. Keep this compatibility transport closed until there is a shared
/// canonical verifier for identity, revocation, and exact capability grants.
pub struct FailClosedVerifier;

#[tonic::async_trait]
impl CredentialVerifier for FailClosedVerifier {
    async fn verify(&self, wire: &[u8], _capability: &str) -> Result<(), Status> {
        op_identity::sealed_id::SealedId::open(wire)
            .map_err(|_| Status::unauthenticated("invalid SID1 credential"))?;
        Err(Status::unavailable(
            "standalone service manager disabled until a shared identity and grant verifier exists",
        ))
    }
}

pub async fn authorize<T>(
    request: &Request<T>,
    capability: &str,
    verifier: &Arc<dyn CredentialVerifier>,
) -> Result<(), Status> {
    if request.metadata().contains_key(ASSERTION_METADATA_KEY) {
        return Err(Status::unauthenticated(
            "this compatibility listener requires one SID1 credential",
        ));
    }
    let mut values = request
        .metadata()
        .get_all_bin(SEALED_ID_METADATA_KEY)
        .into_iter();
    let value = values
        .next()
        .ok_or_else(|| Status::unauthenticated("missing SID1 credential"))?;
    if values.next().is_some() {
        return Err(Status::unauthenticated("multiple SID1 credentials"));
    }
    let wire = value
        .to_bytes()
        .map_err(|_| Status::unauthenticated("malformed SID1 metadata"))?;
    verifier.verify(&wire, capability).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use tonic::Code;

    #[tokio::test]
    async fn production_verifier_rejects_unsealed_claims() {
        let error = FailClosedVerifier
            .verify(b"SID1-forged-claims", "service_manager.create")
            .await
            .unwrap_err();
        assert_eq!(error.code(), Code::Unauthenticated);
    }

    #[tokio::test]
    async fn structurally_valid_sid1_cannot_authorize_without_the_canonical_registry() {
        let claims = op_identity::sealed_id::SealedId {
            principal_id: "principal".into(),
            principal_kind: "wireguard-principal".into(),
            session_id: "session".into(),
            wireguard_pubkey: "pubkey".into(),
            session_genesis: "genesis".into(),
            trace_id: "trace".into(),
            schema_version: 1,
            issued_at: 1,
            expires_at: 0,
            arrival_timestamp: 1,
            chain_head_at_arrival: "chain".into(),
            catalog_hash_at_arrival: "catalog".into(),
            head_timestamp_at_arrival: 1,
            transport_scope: "grpc".into(),
            nextdns: None,
        };
        let error = FailClosedVerifier
            .verify(&claims.seal().unwrap(), "service_manager.create")
            .await
            .unwrap_err();
        assert_eq!(error.code(), Code::Unavailable);
    }
}
