//! An identity's lungs reach the bridge over a socket that belongs to that
//! identity alone.
//!
//! 3tchedFS turns a write to a `call` file into `PluginV1.Call`. On the shared
//! session bus that call is anonymous until the sender registers a SID1, and
//! the owner ruled out storing a SID1 on the identity's device. Here the
//! bridge serves peer-to-peer D-Bus on `<dir>/<session_id>.sock` instead, and
//! every object on that connection is bound to that one session: holding the
//! path is the proof, so nothing is registered and no credential exists at
//! rest. The session is still resolved on every call from the committed sled
//! projection and the key registry, so a parked, expired or revoked identity
//! cannot act through its lungs, and calls run through MutationEngine under
//! the identity's own grants, like any other caller.

use std::os::unix::fs::{DirBuilderExt as _, FileTypeExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{ensure, Context, Result};
use zbus::connection::AuthMechanism;

use crate::dbus_identity::{authoritative_session_identity, DbusIdentityError};
use crate::interceptor::GhostbridgeIdentity;
use crate::mutation_engine::MutationEngine;
use crate::schema_router::SchemaRouter;

pub const DEFAULT_LUNGS_SOCKET_DIR: &str = "/run/opdbus/lungs";
const NEGOTIATION_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_CONNECTIONS: usize = 16;

/// The one identity a lungs socket speaks for.
pub struct LungsIdentity {
    engine: Arc<MutationEngine>,
    session_id: String,
}

impl LungsIdentity {
    pub fn new(engine: Arc<MutationEngine>, session_id: String) -> Self {
        Self { engine, session_id }
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// The identity as it stands now. Only an active, anchored session whose
    /// key maps to an unrevoked principal can call.
    pub async fn resolve(&self) -> Result<GhostbridgeIdentity, DbusIdentityError> {
        authoritative_session_identity(self.engine.as_ref(), None, &self.session_id, true).await
    }

    /// Publish the session context, so this identity's mutations carry the
    /// digest of its sealed id on the chain, as a D-Bus registration does.
    async fn publish_session_context(&self) -> Result<()> {
        let identity = self
            .resolve()
            .await
            .map_err(|error| anyhow::anyhow!("lungs identity unavailable: {error}"))?;
        let sled = crate::identity_sled_dispatch::stored_session(
            self.engine.as_ref(),
            &identity.session_id,
        )
        .await?
        .context("lungs session disappeared")?;
        self.engine
            .register_session_context(crate::mutation_engine::SessionContext {
                genesis_hex: identity.session_genesis,
                session_id: identity.session_id,
                wireguard_pubkey: sled.wireguard_pubkey.clone(),
                sealed_id_digest: crate::mutation_engine::sealed_id_digest(
                    sled.sealed_id.as_deref(),
                ),
            })
            .await;
        Ok(())
    }
}

/// `<dir>/<session_id>.sock`, refusing any session id that could leave `dir`.
pub fn socket_path(dir: &Path, session_id: &str) -> Result<PathBuf> {
    ensure!(
        !session_id.is_empty()
            && session_id.len() <= 128
            && session_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_'),
        "session id is not a plain name"
    );
    Ok(dir.join(format!("{session_id}.sock")))
}

/// Only host root may connect: the lungs daemon that owns the identity's
/// mount runs there, and a process inside the identity reaches the bridge
/// through its lungs, never through this socket.
fn peer_allowed(uid: u32) -> bool {
    uid == 0
}

/// Bind the identity's socket, replacing a stale one, owner-only.
fn bind(dir: &Path, session_id: &str) -> Result<(tokio::net::UnixListener, PathBuf)> {
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
        .with_context(|| format!("create {}", dir.display()))?;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    let path = socket_path(dir, session_id)?;
    match std::fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.file_type().is_socket() => std::fs::remove_file(&path)?,
        Ok(_) => anyhow::bail!("{} exists and is not a socket", path.display()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let listener = tokio::net::UnixListener::bind(&path)
        .with_context(|| format!("bind {}", path.display()))?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    Ok((listener, path))
}

/// Serve one identity's lungs socket until the listener fails.
pub async fn serve(
    dir: &Path,
    router: Arc<SchemaRouter>,
    identity: Arc<LungsIdentity>,
) -> Result<()> {
    let (listener, path) = bind(dir, identity.session_id())?;
    tracing::info!(
        socket = %path.display(),
        session_id = identity.session_id(),
        "lungs socket listening"
    );
    let connections = Arc::new(tokio::sync::Semaphore::new(MAX_CONNECTIONS));
    loop {
        let permit = connections.clone().acquire_owned().await?;
        let (stream, _) = listener.accept().await?;
        let uid = stream.peer_cred().map(|cred| cred.uid()).ok();
        if !uid.is_some_and(peer_allowed) {
            tracing::warn!(
                ?uid,
                session_id = identity.session_id(),
                "lungs peer refused"
            );
            continue;
        }
        let router = router.clone();
        let identity = identity.clone();
        tokio::spawn(async move {
            let _permit = permit;
            let outcome = async {
                identity.publish_session_context().await?;
                let mut builder = zbus::connection::Builder::unix_stream(stream)
                    .server(zbus::Guid::generate())?
                    .p2p()
                    .auth_mechanism(AuthMechanism::Anonymous);
                for (path, object) in router.lungs_objects(identity.clone()).await {
                    builder = builder.serve_at(path, object)?;
                }
                let connection = tokio::time::timeout(NEGOTIATION_TIMEOUT, builder.build())
                    .await
                    .context("lungs D-Bus negotiation timed out")??;
                connection.closed().await;
                Ok::<_, anyhow::Error>(())
            }
            .await;
            if let Err(error) = outcome {
                tracing::warn!(%error, session_id = identity.session_id(), "lungs connection ended");
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn socket_path_is_the_session_inside_the_directory() {
        let dir = Path::new("/run/opdbus/lungs");
        assert_eq!(
            socket_path(dir, "b0c777e3-4a").unwrap(),
            PathBuf::from("/run/opdbus/lungs/b0c777e3-4a.sock")
        );
    }

    #[test]
    fn socket_path_refuses_names_that_leave_the_directory() {
        let dir = Path::new("/run/opdbus/lungs");
        for session_id in ["", "../escape", "a/b", "a.b", "x\0y", &"a".repeat(129)] {
            assert!(socket_path(dir, session_id).is_err(), "{session_id:?}");
        }
    }

    #[test]
    fn only_host_root_may_connect() {
        assert!(peer_allowed(0));
        assert!(!peer_allowed(1000));
        assert!(!peer_allowed(1_000_000));
    }

    #[tokio::test]
    async fn bind_replaces_a_stale_socket_owner_only() {
        let directory = tempfile::tempdir().unwrap();
        let dir = directory.path().join("lungs");
        let (first, path) = bind(&dir, "session").unwrap();
        drop(first);
        let (_second, again) = bind(&dir, "session").unwrap();
        assert_eq!(path, again);
        let mode = std::fs::metadata(&again).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        let dir_mode = std::fs::metadata(&dir).unwrap().permissions().mode();
        assert_eq!(dir_mode & 0o777, 0o700);
    }

    #[tokio::test]
    async fn bind_refuses_to_replace_a_file_that_is_not_a_socket() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("session.sock"), b"keep").unwrap();
        assert!(bind(directory.path(), "session").is_err());
        assert_eq!(
            std::fs::read(directory.path().join("session.sock")).unwrap(),
            b"keep"
        );
    }
}
