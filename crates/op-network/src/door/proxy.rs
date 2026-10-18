//! Handoff from a terminated transport into the door's local socket.
//!
//! The door is reached over a Unix socket mounted into the identity
//! container, not over an IP address. That choice removes a whole class of
//! failure the fabric used to carry: no address has to exist on an interface,
//! no certificate has to be trusted for a loopback hop, and a container never
//! binds an address inside its own namespace.
//!
//! Two different things authenticate here, and they are not interchangeable:
//! the kernel authenticates the *process* on the other end of the socket from
//! its peer credentials, while the SID1/OIA1 credential names the *remote
//! human* whose traffic this process is forwarding. This module carries the
//! second unchanged and relies on the kernel for the first.

use std::path::{Path, PathBuf};

use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt, copy_bidirectional};
use tokio::net::UnixStream;
use tonic::metadata::MetadataMap;

use crate::error::NetworkError;
use crate::identity::sealed_id::SealedIdentityBlob;
use crate::identity::{IdentityMetadataInjector, PassthroughInjector};

/// The door socket this forwarder hands streams to.
#[derive(Debug, Clone)]
pub struct DoorTarget {
    path: PathBuf,
}

impl DoorTarget {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Default for DoorTarget {
    fn default() -> Self {
        Self::new(super::DEFAULT_LUNGS_SOCKET)
    }
}

/// Forwards terminated transport streams into the door's socket.
#[derive(Debug, Clone)]
pub struct DoorProxy {
    target: DoorTarget,
    injector: PassthroughInjector,
}

impl DoorProxy {
    pub fn new(target: DoorTarget) -> Self {
        Self {
            target,
            injector: PassthroughInjector,
        }
    }

    pub fn target(&self) -> &DoorTarget {
        &self.target
    }

    /// Metadata this stream will carry, or an error if it cannot carry one.
    ///
    /// Built before connecting so a credential that cannot be represented
    /// costs no connection to the door.
    pub fn metadata_for(&self, identity: &SealedIdentityBlob) -> Result<MetadataMap, NetworkError> {
        let mut metadata = MetadataMap::new();
        self.injector
            .inject_identity_metadata(&mut metadata, identity)?;
        Ok(metadata)
    }

    /// Pipe one terminated stream into the door until either side closes.
    ///
    /// Returns the bytes copied in each direction. A credential that cannot
    /// be placed into metadata aborts before the socket is opened, so a
    /// rejected identity never reaches the door.
    pub async fn forward_stream<S>(
        &self,
        mut incoming: S,
        identity: SealedIdentityBlob,
    ) -> Result<(u64, u64), NetworkError>
    where
        S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        let _metadata = self.metadata_for(&identity)?;

        let mut door = UnixStream::connect(self.target.path()).await.map_err(|source| {
            NetworkError::Door {
                path: self.target.path().to_path_buf(),
                reason: format!("connect failed: {source}"),
            }
        })?;

        let copied = copy_bidirectional(&mut incoming, &mut door)
            .await
            .map_err(|source| NetworkError::Door {
                path: self.target.path().to_path_buf(),
                reason: format!("stream copy failed: {source}"),
            })?;

        // Flush before dropping so the door observes a clean close rather
        // than a reset it would otherwise record as a client fault.
        door.shutdown().await.ok();
        Ok(copied)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::{ASSERTION_METADATA_KEY, SEALED_ID_METADATA_KEY};
    use tokio::io::{AsyncReadExt, AsyncWriteExt as _};
    use tokio::net::UnixListener;

    fn blob(magic: &[u8; 4]) -> SealedIdentityBlob {
        let mut wire = magic.to_vec();
        wire.extend_from_slice(b"credential");
        SealedIdentityBlob::from_wire(wire).expect("fixture credential is well formed")
    }

    #[test]
    fn each_credential_lands_on_its_own_metadata_key() {
        let proxy = DoorProxy::new(DoorTarget::default());
        let sid = blob(b"SID1");
        let metadata = proxy.metadata_for(&sid).expect("SID1 maps to metadata");
        let written = metadata
            .get_bin(SEALED_ID_METADATA_KEY)
            .expect("sealed id key present")
            .to_bytes()
            .expect("binary metadata decodes");
        assert_eq!(written.as_ref(), sid.wire());

        let metadata = proxy
            .metadata_for(&blob(b"OIA1"))
            .expect("OIA1 maps to metadata");
        assert!(metadata.get_bin(ASSERTION_METADATA_KEY).is_some());
    }

    #[tokio::test]
    async fn bytes_cross_the_socket_in_both_directions() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("lungs.sock");
        let listener = UnixListener::bind(&path).expect("bind door socket");

        let door = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("door accepts");
            let mut seen = [0u8; 5];
            stream.read_exact(&mut seen).await.expect("door reads");
            stream.write_all(b"pong").await.expect("door replies");
            stream.shutdown().await.ok();
            seen
        });

        let (mut client, server_side) = UnixStream::pair().expect("stream pair");
        let proxy = DoorProxy::new(DoorTarget::new(&path));
        let forward = tokio::spawn(async move {
            proxy
                .forward_stream(server_side, blob(b"SID1"))
                .await
                .expect("forwarded")
        });

        client.write_all(b"hello").await.expect("client writes");
        let mut reply = Vec::new();
        client.read_to_end(&mut reply).await.expect("client reads");

        assert_eq!(&door.await.expect("door task")[..], b"hello");
        assert_eq!(reply, b"pong");
        let (to_door, from_door) = forward.await.expect("forward task");
        assert_eq!((to_door, from_door), (5, 4));
    }

    #[tokio::test]
    async fn a_missing_door_socket_reports_the_path_it_tried() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("absent.sock");
        let proxy = DoorProxy::new(DoorTarget::new(&path));
        let (_client, server_side) = UnixStream::pair().expect("stream pair");
        let err = proxy
            .forward_stream(server_side, blob(b"SID1"))
            .await
            .expect_err("absent door refused");
        match err {
            NetworkError::Door { path: tried, .. } => assert_eq!(tried, path),
            other => panic!("expected a door failure, got {other:?}"),
        }
    }
}
