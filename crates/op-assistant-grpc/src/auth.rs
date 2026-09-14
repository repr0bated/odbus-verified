//! Ghostbridge identity extraction and authentication middleware.
//!
//! Requires a session genesis plus either `x-ghostbridge-trace-id` or
//! `x-wireguard-pubkey`, then verifies both against the per-session identity
//! projection.

use tonic::{Request, Status};

pub const WIREGUARD_PUBKEY_HEADER: &str = "x-wireguard-pubkey";

/// Identity extracted by the Ghostbridge interceptor and attached to each
/// accepted request for downstream authorization / trace linking.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WireGuardIdentity {
    pub pubkey: String,
    pub footprint: String,
    pub session_id: String,
}

/// Tonic interceptor: Ghostbridge footprint gate matching op-cognitive-mcp.
/// Returns `Unauthenticated` when identity headers are missing.
#[allow(clippy::result_large_err)]
pub fn wireguard_auth_interceptor(mut req: Request<()>) -> Result<Request<()>, Status> {
    let footprint_value = req
        .metadata()
        .get("x-ghostbridge-genesis")
        .or_else(|| req.metadata().get("x-ghostbridge-footprint"))
        .cloned();
    let trace_value = req
        .metadata()
        .get("x-ghostbridge-trace-id")
        .or_else(|| req.metadata().get("x-wireguard-pubkey"))
        .cloned();

    if footprint_value.is_none() || trace_value.is_none() {
        return Err(Status::unauthenticated(
            "Missing Ghostbridge session identity.",
        ));
    }

    let request_footprint = footprint_value
        .as_ref()
        .unwrap()
        .to_str()
        .map_err(|_| Status::invalid_argument("Invalid footprint header encoding"))?;

    let trace = req
        .metadata()
        .get("x-ghostbridge-trace-id")
        .and_then(|value| value.to_str().ok());
    let pubkey = req
        .metadata()
        .get(WIREGUARD_PUBKEY_HEADER)
        .and_then(|value| value.to_str().ok());
    let identity = op_identity::resolve_verified_session(request_footprint, trace, pubkey)
        .map_err(|_| {
            Status::permission_denied("Session genesis does not match a current identity")
        })?;
    let pubkey = req
        .metadata()
        .get(WIREGUARD_PUBKEY_HEADER)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    req.extensions_mut().insert(WireGuardIdentity {
        pubkey,
        footprint: request_footprint.to_string(),
        session_id: identity.session_id,
    });

    Ok(req)
}

#[cfg(test)]
mod tests {
    use super::*;
    use op_core::projection_shm::update_projection_with_credentials_in;
    use std::os::unix::fs::MetadataExt;
    use tonic::metadata::MetadataValue;

    const FOOTPRINT_HEADER: &str = "x-ghostbridge-footprint";

    #[test]
    fn should_reject_missing_identity() {
        let req = Request::new(());
        let result = wireguard_auth_interceptor(req);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code(), tonic::Code::Unauthenticated);
    }

    #[test]
    fn should_reject_pubkey_without_footprint() {
        let mut req = Request::new(());
        req.metadata_mut().insert(
            WIREGUARD_PUBKEY_HEADER,
            MetadataValue::from_static("abcd1234"),
        );
        let result = wireguard_auth_interceptor(req);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code(), tonic::Code::Unauthenticated);
    }

    #[test]
    fn should_reject_empty_identity() {
        let mut req = Request::new(());
        req.metadata_mut()
            .insert(WIREGUARD_PUBKEY_HEADER, MetadataValue::from_static(""));
        let result = wireguard_auth_interceptor(req);
        assert!(result.is_err());
    }

    #[test]
    fn should_verify_genesis_against_identity_session() {
        static STATE_DIR_ENV: std::sync::Mutex<()> = std::sync::Mutex::new(());
        struct StateDirGuard(Option<std::ffi::OsString>);
        impl Drop for StateDirGuard {
            fn drop(&mut self) {
                match self.0.take() {
                    Some(previous) => unsafe {
                        std::env::set_var("OP_SHM_STATE_DIR", previous);
                    },
                    None => unsafe {
                        std::env::remove_var("OP_SHM_STATE_DIR");
                    },
                }
            }
        }

        let _serial = STATE_DIR_ENV.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().to_str().unwrap();
        let projection = serde_json::to_vec(&serde_json::json!({
            "sleds": [{
                "session_id": "session-a",
                "wireguard_pubkey": "abcd1234",
                "genesis": "ab".repeat(32),
                "trace_id": "11".repeat(16),
                "schema_version": 3,
                "active": false,
                "arrival_timestamp": 1,
                "chain_head_at_arrival": "22".repeat(32),
                "catalog_hash_at_arrival": "33".repeat(32),
                "head_timestamp_at_arrival": 1
            }]
        }))
        .unwrap();
        update_projection_with_credentials_in(
            state_dir,
            "identity_sled",
            dir.path().metadata().unwrap().gid(),
            |_| Ok(Some((projection.clone(), projection))),
        )
        .unwrap();
        let _state_dir = StateDirGuard(std::env::var_os("OP_SHM_STATE_DIR"));
        unsafe {
            std::env::set_var("OP_SHM_STATE_DIR", state_dir);
        }
        let matching = "ab".repeat(32);
        let mismatch = hex::encode([0x00u8; 32]);

        let mut bad = Request::new(());
        bad.metadata_mut().insert(
            FOOTPRINT_HEADER,
            MetadataValue::try_from(mismatch.as_str()).unwrap(),
        );
        bad.metadata_mut().insert(
            WIREGUARD_PUBKEY_HEADER,
            MetadataValue::from_static("abcd1234"),
        );
        let err = wireguard_auth_interceptor(bad).unwrap_err();
        assert_eq!(err.code(), tonic::Code::PermissionDenied);

        let mut good = Request::new(());
        good.metadata_mut().insert(
            FOOTPRINT_HEADER,
            MetadataValue::try_from(matching.as_str()).unwrap(),
        );
        good.metadata_mut().insert(
            WIREGUARD_PUBKEY_HEADER,
            MetadataValue::from_static("abcd1234"),
        );
        let out = wireguard_auth_interceptor(good).unwrap();
        let id = out.extensions().get::<WireGuardIdentity>().unwrap();
        assert_eq!(id.pubkey, "abcd1234");
        assert_eq!(id.session_id, "session-a");
    }
}
