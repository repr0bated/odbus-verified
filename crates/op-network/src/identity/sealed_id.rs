//! The one credential a transport carries to the door.
//!
//! The decoy mints these; this host never does. A blob is held as the exact
//! wire bytes the peer sent, because the door's interceptor verifies those
//! bytes — re-encoding a parsed form would change what gets verified.

use crate::error::NetworkError;

/// Largest credential accepted off the wire.
///
/// Matches the door interceptor's own bound, so a blob that would be refused
/// there is refused here instead of crossing the handoff first.
pub const MAX_BLOB_BYTES: usize = 16 * 1024;

const SID1_MAGIC: &[u8; 4] = b"SID1";
const OIA1_MAGIC: &[u8; 4] = b"OIA1";

/// Which credential the peer presented.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredentialKind {
    /// Sealed session identity: the human's own credential for this session.
    Sid1,
    /// Oracle assertion: the decoy's signed proof of a WireGuard peer.
    Oia1,
}

impl CredentialKind {
    const fn magic(self) -> &'static [u8; 4] {
        match self {
            Self::Sid1 => SID1_MAGIC,
            Self::Oia1 => OIA1_MAGIC,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Sid1 => "SID1",
            Self::Oia1 => "OIA1",
        }
    }
}

/// Exact credential bytes plus the kind implied by their magic.
///
/// Construction is the only validation point: a value of this type always
/// carries a non-empty, length-bounded blob whose magic matches its kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SealedIdentityBlob {
    kind: CredentialKind,
    wire: Vec<u8>,
}

impl SealedIdentityBlob {
    /// Classify and bounds-check credential bytes taken off a transport.
    pub fn from_wire(wire: Vec<u8>) -> Result<Self, NetworkError> {
        if wire.len() > MAX_BLOB_BYTES {
            return Err(NetworkError::Identity(format!(
                "credential is {} bytes, limit is {MAX_BLOB_BYTES}",
                wire.len()
            )));
        }
        let kind = if wire.starts_with(SID1_MAGIC) {
            CredentialKind::Sid1
        } else if wire.starts_with(OIA1_MAGIC) {
            CredentialKind::Oia1
        } else {
            return Err(NetworkError::Identity(
                "credential carries neither the SID1 nor the OIA1 magic".into(),
            ));
        };
        Ok(Self { kind, wire })
    }

    /// Reject the ambiguous case before it reaches the door.
    ///
    /// The interceptor accepts exactly one credential per call, so a peer
    /// offering both has to be refused rather than silently narrowed.
    pub fn exactly_one(
        sid1: Option<Vec<u8>>,
        oia1: Option<Vec<u8>>,
    ) -> Result<Self, NetworkError> {
        match (sid1, oia1) {
            (Some(_), Some(_)) => Err(NetworkError::Identity(
                "peer presented both SID1 and OIA1".into(),
            )),
            (None, None) => Err(NetworkError::Identity("peer presented no credential".into())),
            (Some(wire), None) | (None, Some(wire)) => Self::from_wire(wire),
        }
    }

    pub const fn kind(&self) -> CredentialKind {
        self.kind
    }

    /// The bytes exactly as received, for metadata insertion.
    pub fn wire(&self) -> &[u8] {
        &self.wire
    }

    /// True when the magic still matches the recorded kind.
    ///
    /// Cheap enough to re-check at the handoff, which is the last point
    /// before the bytes leave this process.
    pub fn is_intact(&self) -> bool {
        self.wire.starts_with(self.kind.magic())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sid1(extra: usize) -> Vec<u8> {
        let mut wire = SID1_MAGIC.to_vec();
        wire.extend(std::iter::repeat_n(0x41, extra));
        wire
    }

    #[test]
    fn classifies_each_magic_and_keeps_bytes_unchanged() {
        let wire = sid1(8);
        let blob = SealedIdentityBlob::from_wire(wire.clone()).expect("SID1 accepted");
        assert_eq!(blob.kind(), CredentialKind::Sid1);
        assert_eq!(blob.wire(), wire.as_slice());
        assert!(blob.is_intact());

        let oia = SealedIdentityBlob::from_wire(OIA1_MAGIC.to_vec()).expect("OIA1 accepted");
        assert_eq!(oia.kind(), CredentialKind::Oia1);
    }

    #[test]
    fn rejects_unknown_magic_and_oversized_blobs() {
        assert!(SealedIdentityBlob::from_wire(b"NOPE1234".to_vec()).is_err());
        assert!(SealedIdentityBlob::from_wire(Vec::new()).is_err());
        assert!(SealedIdentityBlob::from_wire(sid1(MAX_BLOB_BYTES)).is_err());
        // The limit itself is accepted; only beyond it is refused.
        assert!(SealedIdentityBlob::from_wire(sid1(MAX_BLOB_BYTES - 4)).is_ok());
    }

    #[test]
    fn exactly_one_rejects_both_and_neither() {
        assert!(SealedIdentityBlob::exactly_one(Some(sid1(4)), None).is_ok());
        assert!(SealedIdentityBlob::exactly_one(None, Some(OIA1_MAGIC.to_vec())).is_ok());
        assert!(SealedIdentityBlob::exactly_one(Some(sid1(4)), Some(OIA1_MAGIC.to_vec())).is_err());
        assert!(SealedIdentityBlob::exactly_one(None, None).is_err());
    }
}
