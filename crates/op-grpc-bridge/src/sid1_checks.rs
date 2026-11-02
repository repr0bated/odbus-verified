//! Shared SID1 checks. These checks alone do not authenticate an identity:
//! callers must obtain the record from committed private state and verify its
//! provisioned container or human binding before accepting the request.

use base64::Engine as _;
use op_identity::sealed_id::SealedId;
use op_plugins::state_plugins::identity_sled::ContainerIdentitySled;

/// Decode the authored envelope and verify its transport and derived IDs.
pub fn parse_for_transport(wire: &[u8], transport: &str) -> Result<SealedId, String> {
    let claims = SealedId::open(wire).map_err(|error| error.to_string())?;
    if claims.principal_kind != "wireguard-principal"
        || !claims
            .transport_scope
            .split(',')
            .any(|scope| scope.trim() == transport)
    {
        return Err(format!(
            "sealed identity is not valid for the {transport} transport"
        ));
    }
    if op_identity::session::derive_session_id(&claims.wireguard_pubkey) != claims.session_id
        || op_identity::session::derive_principal_id(&claims.wireguard_pubkey)
            != claims.principal_id
    {
        return Err(
            "sealed identity identifiers do not match its WARP-transported identity".into(),
        );
    }
    Ok(claims)
}

/// Compare claims with a record already read from the committed private projection.
/// This does not verify the record's container or human binding.
pub fn match_committed_session(
    wire: &[u8],
    sled: &ContainerIdentitySled,
    transport: &str,
    now: i64,
) -> Result<SealedId, String> {
    let claims = parse_for_transport(wire, transport)?;
    if !sled.is_anchored()
        || sled
            .expires_at
            .is_some_and(|expiry| expiry != 0 && expiry <= now)
    {
        return Err("sealed identity session is unanchored or expired".into());
    }
    let inline = format!(
        "{}{}",
        op_identity::sealed_id::INLINE_PREFIX,
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(wire)
    );
    if sled.session_id != claims.session_id
        || sled.sealed_id.as_deref() != Some(inline.as_str())
        || sled.wireguard_pubkey != claims.wireguard_pubkey
        || sled.genesis.as_deref() != Some(claims.session_genesis.as_str())
        || sled.trace_id != claims.trace_id
        || sled.schema_version != claims.schema_version
        || sled.expires_at.unwrap_or(0) != claims.expires_at
        || sled.arrival_timestamp != claims.arrival_timestamp
        || claims.issued_at != claims.arrival_timestamp
        || sled.chain_head_at_arrival != claims.chain_head_at_arrival
        || sled.catalog_hash_at_arrival != claims.catalog_hash_at_arrival
        || sled.head_timestamp_at_arrival != claims.head_timestamp_at_arrival
    {
        return Err("sealed identity does not match the authoritative sled".into());
    }
    Ok(claims)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (Vec<u8>, ContainerIdentitySled) {
        let key = "test-wireguard-key".to_string();
        let claims = SealedId {
            principal_id: op_identity::session::derive_principal_id(&key),
            principal_kind: "wireguard-principal".into(),
            session_id: op_identity::session::derive_session_id(&key),
            wireguard_pubkey: key,
            session_genesis: "genesis".into(),
            trace_id: "trace".into(),
            schema_version: 1,
            issued_at: 100,
            expires_at: 200,
            arrival_timestamp: 100,
            chain_head_at_arrival: "chain".into(),
            catalog_hash_at_arrival: "catalog".into(),
            head_timestamp_at_arrival: 99,
            transport_scope: "grpc".into(),
            nextdns: None,
        };
        let wire = claims.seal().unwrap();
        let sled = serde_json::from_value(serde_json::json!({
            "session_id": claims.session_id,
            "wireguard_pubkey": claims.wireguard_pubkey,
            "genesis": claims.session_genesis,
            "trace_id": claims.trace_id,
            "schema_version": claims.schema_version,
            "sealed_id": format!("{}{}", op_identity::sealed_id::INLINE_PREFIX,
                base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&wire)),
            "expires_at": claims.expires_at,
            "arrival_timestamp": claims.arrival_timestamp,
            "chain_head_at_arrival": claims.chain_head_at_arrival,
            "catalog_hash_at_arrival": claims.catalog_hash_at_arrival,
            "head_timestamp_at_arrival": claims.head_timestamp_at_arrival
        }))
        .unwrap();
        (wire, sled)
    }

    #[test]
    fn checks_transport_and_exact_committed_session() {
        let (wire, mut sled) = fixture();
        parse_for_transport(&wire, "grpc").unwrap();
        assert!(parse_for_transport(&wire, "mcp").is_err());
        assert!(match_committed_session(&wire, &sled, "grpc", 150).is_ok());
        assert!(match_committed_session(&wire, &sled, "grpc", 200).is_err());
        sled.session_id = "other".into();
        assert!(match_committed_session(&wire, &sled, "grpc", 150).is_err());
        let mut corrupted = wire.clone();
        *corrupted.last_mut().unwrap() ^= 1;
        assert!(parse_for_transport(&corrupted, "grpc").is_err());
    }
}
