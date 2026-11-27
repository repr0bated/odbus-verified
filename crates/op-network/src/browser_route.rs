//! Opaque browser-route identity derived from one committed identity sled.
//!
//! The login path supplies the server-authored session ID for the same accepted
//! socket. This module never discovers identity from packet or request fields.

use std::fmt;

use thiserror::Error;

const HANDLE_CONTEXT: &str = "op-network browser route handle v1";

/// Kernel-visible key for one accepted socket. Zero is never a valid key.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct BrowserConnectionKey(u64);

impl BrowserConnectionKey {
    pub fn new(socket_cookie: u64) -> Result<Self, BrowserRouteError> {
        if socket_cookie == 0 {
            return Err(BrowserRouteError::InvalidConnectionKey);
        }
        Ok(Self(socket_cookie))
    }

    pub fn get(self) -> u64 {
        self.0
    }
}

impl fmt::Debug for BrowserConnectionKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("BrowserConnectionKey([redacted])")
    }
}

/// Opaque, fixed-width value stored in the browser-ingress BPF map.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct BrowserRouteHandle([u8; 16]);

impl BrowserRouteHandle {
    pub fn bytes(self) -> [u8; 16] {
        self.0
    }
}

impl fmt::Debug for BrowserRouteHandle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("BrowserRouteHandle([redacted])")
    }
}

/// The authoritative sled fields needed to derive an ephemeral browser route.
pub struct CommittedBrowserIdentity<'a> {
    pub session_id: &'a str,
    pub genesis: &'a str,
    pub native_name: &'a str,
    pub native_uuid: &'a str,
    pub active: bool,
    pub anchored: bool,
    pub expires_at: Option<i64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Error)]
pub enum BrowserRouteError {
    #[error("browser connection key is invalid")]
    InvalidConnectionKey,
    #[error("identity route is not eligible")]
    IneligibleIdentity,
    #[error("identity route fields are not canonical")]
    InvalidIdentity,
}

/// Derive one route only while the supplied committed identity is eligible.
pub fn derive_route_handle(
    identity: &CommittedBrowserIdentity<'_>,
    now: i64,
) -> Result<BrowserRouteHandle, BrowserRouteError> {
    if !identity.active
        || !identity.anchored
        || identity
            .expires_at
            .is_some_and(|expiry| expiry != 0 && expiry <= now)
    {
        return Err(BrowserRouteError::IneligibleIdentity);
    }
    let canonical_uuid = |value: &str| {
        uuid::Uuid::parse_str(value).is_ok_and(|parsed| !parsed.is_nil() && parsed.to_string() == value)
    };
    let canonical_hex = |value: &str| {
        value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    };
    if !canonical_uuid(identity.session_id)
        || !canonical_uuid(identity.native_uuid)
        || !canonical_hex(identity.genesis)
        || identity.native_name.is_empty()
        || identity.native_name.len() > 255
        || identity.native_name.bytes().any(|byte| byte.is_ascii_control())
    {
        return Err(BrowserRouteError::InvalidIdentity);
    }

    let mut material = Vec::with_capacity(
        identity.session_id.len()
            + identity.genesis.len()
            + identity.native_name.len()
            + identity.native_uuid.len()
            + 16,
    );
    for value in [
        identity.session_id,
        identity.genesis,
        identity.native_name,
        identity.native_uuid,
    ] {
        let length = u32::try_from(value.len()).map_err(|_| BrowserRouteError::InvalidIdentity)?;
        material.extend_from_slice(&length.to_be_bytes());
        material.extend_from_slice(value.as_bytes());
    }
    let derived = blake3::derive_key(HANDLE_CONTEXT, &material);
    let mut handle = [0; 16];
    handle.copy_from_slice(&derived[..16]);
    if handle == [0; 16] {
        handle[0] = 1;
    }
    Ok(BrowserRouteHandle(handle))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity() -> CommittedBrowserIdentity<'static> {
        CommittedBrowserIdentity {
            session_id: "12345678-9abc-4def-8123-0123456789ab",
            genesis: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
            native_name: "identity-a",
            native_uuid: "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee",
            active: true,
            anchored: true,
            expires_at: Some(200),
        }
    }

    #[test]
    fn eligible_committed_identity_has_one_stable_opaque_route() {
        let first = derive_route_handle(&identity(), 100).unwrap();
        let second = derive_route_handle(&identity(), 199).unwrap();
        assert_eq!(first, second);
        assert_ne!(first.bytes(), [0; 16]);
        assert_eq!(format!("{first:?}"), "BrowserRouteHandle([redacted])");
    }

    #[test]
    fn inactive_unanchored_expired_or_malformed_identity_has_no_route() {
        let mut candidate = identity();
        candidate.active = false;
        assert_eq!(derive_route_handle(&candidate, 100), Err(BrowserRouteError::IneligibleIdentity));
        candidate.active = true;
        candidate.anchored = false;
        assert_eq!(derive_route_handle(&candidate, 100), Err(BrowserRouteError::IneligibleIdentity));
        candidate.anchored = true;
        candidate.expires_at = Some(100);
        assert_eq!(derive_route_handle(&candidate, 100), Err(BrowserRouteError::IneligibleIdentity));
        candidate.expires_at = None;
        candidate.session_id = "not-a-session";
        assert_eq!(derive_route_handle(&candidate, 100), Err(BrowserRouteError::InvalidIdentity));
    }

    #[test]
    fn changing_any_committed_binding_field_changes_the_route() {
        let original = derive_route_handle(&identity(), 100).unwrap();
        let mut changed = identity();
        changed.native_uuid = "bbbbbbbb-cccc-4ddd-8eee-ffffffffffff";
        assert_ne!(derive_route_handle(&changed, 100).unwrap(), original);
    }

    #[test]
    fn socket_cookie_is_nonzero_and_redacted() {
        assert_eq!(BrowserConnectionKey::new(0), Err(BrowserRouteError::InvalidConnectionKey));
        let key = BrowserConnectionKey::new(42).unwrap();
        assert_eq!(key.get(), 42);
        assert_eq!(format!("{key:?}"), "BrowserConnectionKey([redacted])");
    }
}
