//! Carrying the decoy's credential into gRPC metadata.
//!
//! This host does not mint, read or judge credentials. It moves the exact
//! bytes the decoy produced into the metadata keys the door's interceptor
//! reads, and refuses anything it cannot place there unchanged.

pub mod sealed_id;

use tonic::metadata::{MetadataMap, MetadataValue};

use crate::error::NetworkError;
use sealed_id::{CredentialKind, SealedIdentityBlob};


/// Metadata key holding the sealed session identity (SID1).
///
/// Re-exported from `op-identity` rather than restated: the door interceptor
/// reads that same constant, and a second spelling here would mean a
/// credential the door never looks for.
pub use op_identity::sealed_id::HTTP_HEADER_NAME as SEALED_ID_METADATA_KEY;

/// Metadata key holding the oracle assertion (OIA1).
pub const ASSERTION_METADATA_KEY: &str = "x-oracle-identity-assertion-bin";

/// Places a credential into outbound gRPC metadata.
pub trait IdentityMetadataInjector {
    /// Write exactly one credential into `metadata`.
    ///
    /// Fails rather than overwriting: a map that already carries a credential
    /// means two sources are competing to name the caller, and the door must
    /// not have to guess which one is authoritative.
    fn inject_identity_metadata(
        &self,
        metadata: &mut MetadataMap,
        identity: &SealedIdentityBlob,
    ) -> Result<(), NetworkError>;
}

/// The default injector: one credential, written unchanged.
#[derive(Debug, Clone, Copy, Default)]
pub struct PassthroughInjector;

impl IdentityMetadataInjector for PassthroughInjector {
    fn inject_identity_metadata(
        &self,
        metadata: &mut MetadataMap,
        identity: &SealedIdentityBlob,
    ) -> Result<(), NetworkError> {
        if !identity.is_intact() {
            return Err(NetworkError::Identity(
                "credential magic no longer matches its kind".into(),
            ));
        }
        for key in [SEALED_ID_METADATA_KEY, ASSERTION_METADATA_KEY] {
            if metadata.get_bin(key).is_some() {
                return Err(NetworkError::Identity(format!(
                    "{key} is already present on this request"
                )));
            }
        }
        let key = match identity.kind() {
            CredentialKind::Sid1 => SEALED_ID_METADATA_KEY,
            CredentialKind::Oia1 => ASSERTION_METADATA_KEY,
        };
        let value = MetadataValue::from_bytes(identity.wire());
        metadata.insert_bin(key, value);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blob(magic: &[u8; 4]) -> SealedIdentityBlob {
        let mut wire = magic.to_vec();
        wire.extend_from_slice(b"payload");
        SealedIdentityBlob::from_wire(wire).expect("fixture credential is well formed")
    }

    #[test]
    fn each_kind_lands_on_its_own_key_with_original_bytes() {
        let sid = blob(b"SID1");
        let mut metadata = MetadataMap::new();
        PassthroughInjector
            .inject_identity_metadata(&mut metadata, &sid)
            .expect("SID1 injected");
        let written = metadata
            .get_bin(SEALED_ID_METADATA_KEY)
            .expect("sealed id key present")
            .to_bytes()
            .expect("binary metadata decodes");
        assert_eq!(written.as_ref(), sid.wire());
        assert!(metadata.get_bin(ASSERTION_METADATA_KEY).is_none());

        let mut metadata = MetadataMap::new();
        PassthroughInjector
            .inject_identity_metadata(&mut metadata, &blob(b"OIA1"))
            .expect("OIA1 injected");
        assert!(metadata.get_bin(ASSERTION_METADATA_KEY).is_some());
        assert!(metadata.get_bin(SEALED_ID_METADATA_KEY).is_none());
    }

    #[test]
    fn refuses_to_overwrite_a_credential_already_on_the_request() {
        let mut metadata = MetadataMap::new();
        PassthroughInjector
            .inject_identity_metadata(&mut metadata, &blob(b"SID1"))
            .expect("first credential injected");
        let second = PassthroughInjector.inject_identity_metadata(&mut metadata, &blob(b"OIA1"));
        assert!(second.is_err(), "second credential must not be written");
    }
}
