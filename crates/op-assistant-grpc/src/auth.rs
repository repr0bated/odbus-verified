//! Admission for the retired standalone Assistant compatibility transport.
//!
//! The canonical bridge verifies SID1 against the private sled, registered
//! principal, revocation state, and exact grants. This process cannot reuse
//! that complete receiver across the bridge-owned Cozo store lock, so it must
//! never turn asserted genesis, trace, public-key, or SID1 claims into access.

use tonic::{Request, Status};

pub const WIREGUARD_PUBKEY_HEADER: &str = "x-wireguard-pubkey";
const SEALED_ID_METADATA_KEY: &str = op_identity::sealed_id::HTTP_HEADER_NAME;
const ASSERTION_METADATA_KEY: &str = "x-oracle-identity-assertion-bin";

/// Retained for callers that name the former extension type. No standalone
/// request receives this extension while canonical admission is unavailable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WireGuardIdentity {
    pub pubkey: String,
    pub footprint: String,
    pub session_id: String,
}

/// Reject legacy asserted headers and every unverified SID1/OIA1 credential.
/// A structurally valid SID1 is deliberately insufficient for authorization.
#[allow(clippy::result_large_err)]
pub fn wireguard_auth_interceptor(req: Request<()>) -> Result<Request<()>, Status> {
    if req.metadata().contains_key(ASSERTION_METADATA_KEY) {
        return Err(Status::unauthenticated(
            "standalone Assistant transport cannot verify OIA1",
        ));
    }
    let mut values = req
        .metadata()
        .get_all_bin(SEALED_ID_METADATA_KEY)
        .into_iter();
    let value = values
        .next()
        .ok_or_else(|| Status::unauthenticated("SID1 credential required"))?;
    if values.next().is_some() {
        return Err(Status::unauthenticated("multiple SID1 credentials"));
    }
    let wire = value
        .to_bytes()
        .map_err(|_| Status::unauthenticated("malformed SID1 metadata"))?;
    op_identity::sealed_id::SealedId::open(&wire)
        .map_err(|_| Status::unauthenticated("invalid SID1 credential"))?;
    Err(Status::unavailable(
        "standalone Assistant RPCs disabled until canonical identity and grant admission is shared",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tonic::{metadata::MetadataValue, Code};

    #[test]
    fn missing_identity_is_rejected() {
        let error = wireguard_auth_interceptor(Request::new(())).unwrap_err();
        assert_eq!(error.code(), Code::Unauthenticated);
    }

    #[test]
    fn matching_legacy_genesis_and_pubkey_are_not_credentials() {
        let mut request = Request::new(());
        request.metadata_mut().insert(
            "x-ghostbridge-genesis",
            MetadataValue::from_static("known-genesis"),
        );
        request.metadata_mut().insert(
            WIREGUARD_PUBKEY_HEADER,
            MetadataValue::from_static("known-pubkey"),
        );
        let error = wireguard_auth_interceptor(request).unwrap_err();
        assert_eq!(error.code(), Code::Unauthenticated);
    }

    #[test]
    fn structurally_valid_sid1_is_still_denied_without_canonical_checks() {
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
        };
        let mut request = Request::new(());
        request.metadata_mut().insert_bin(
            SEALED_ID_METADATA_KEY,
            MetadataValue::from_bytes(&claims.seal().unwrap()),
        );
        let error = wireguard_auth_interceptor(request).unwrap_err();
        assert_eq!(error.code(), Code::Unavailable);
    }
}
