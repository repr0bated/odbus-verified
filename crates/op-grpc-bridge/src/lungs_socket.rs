//! Identities' lungs reach the bridge over one shared, root-only socket.
//!
//! 3tchedFS turns a write to a `call` file into `PluginV1.Call`. On the shared
//! session bus that call is anonymous until the sender registers a SID1, and
//! the owner ruled out storing a SID1 on an identity's device. Here the bridge
//! serves peer-to-peer D-Bus on one socket for every identity. Each connection
//! starts unbound; its first call, `LungsV1.Bind(session_id)`, names the one
//! session it breathes for, and it can never rebind. Only host root can
//! connect, and host root can reach every identity anyway, so binding by name
//! proves as much as a socket per identity would, without one socket per
//! login. The session is resolved again on every call from the committed sled
//! projection and the key registry, so a parked, expired or revoked identity
//! cannot act through its lungs, and calls run through MutationEngine under
//! the identity's own grants, like any other caller.

use std::os::unix::fs::{FileTypeExt as _, PermissionsExt as _};
use std::path::Path;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use anyhow::{Context, Result, ensure};
use zbus::connection::AuthMechanism;

use crate::dbus_identity::{DbusIdentityError, authoritative_session_identity};
use crate::interceptor::GhostbridgeIdentity;
use crate::mutation_engine::MutationEngine;
use crate::schema_router::SchemaRouter;

pub const DEFAULT_LUNGS_SOCKET: &str = "/run/opdbus/lungs.sock";
pub const LUNGS_OBJECT_PATH: &str = "/org/opdbus/v1/lungs";
const NEGOTIATION_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_CONNECTIONS: usize = 64;

/// The identity one lungs connection breathes for, set once by `Bind`.
pub struct LungsIdentity {
    engine: Arc<MutationEngine>,
    session_id: OnceLock<String>,
}

impl LungsIdentity {
    pub fn new(engine: Arc<MutationEngine>) -> Self {
        Self {
            engine,
            session_id: OnceLock::new(),
        }
    }

    /// The identity as it stands now. An unbound connection has none, and
    /// only an active, anchored session whose key maps to an unrevoked
    /// principal can call.
    pub async fn resolve(&self) -> Result<GhostbridgeIdentity, DbusIdentityError> {
        let session_id = self.session_id.get().ok_or(DbusIdentityError::NoBinding)?;
        authoritative_session_identity(self.engine.as_ref(), None, session_id, true).await
    }

    /// Bind this connection to one session, once. The session must resolve
    /// now; its context is published so its mutations carry the digest of its
    /// sealed id on the chain, as a D-Bus registration does.
    async fn bind(&self, session_id: &str) -> Result<()> {
        ensure!(
            valid_session_id(session_id),
            "session id is not a plain name"
        );
        ensure!(
            self.session_id.get().is_none(),
            "this lungs connection is already bound"
        );
        let identity = authoritative_session_identity(self.engine.as_ref(), None, session_id, true)
            .await
            .map_err(|error| anyhow::anyhow!("lungs identity unavailable: {error}"))?;
        let sled = crate::identity_sled_dispatch::stored_session(
            self.engine.as_ref(),
            &identity.session_id,
        )
        .await?
        .context("lungs session disappeared")?;
        self.session_id
            .set(identity.session_id.clone())
            .map_err(|_| anyhow::anyhow!("this lungs connection is already bound"))?;
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

/// The per-connection bootstrap: `Bind` must come before any plugin call.
struct LungsBootstrap {
    identity: Arc<LungsIdentity>,
}

#[zbus::interface(name = "org.opdbus.v1.LungsV1")]
impl LungsBootstrap {
    async fn bind(&self, session_id: String) -> zbus::fdo::Result<bool> {
        self.identity.bind(&session_id).await.map_err(|error| {
            tracing::warn!(%error, %session_id, "lungs bind refused");
            zbus::fdo::Error::AccessDenied(error.to_string())
        })?;
        Ok(true)
    }
}

/// A plain session name, so it can never smuggle a path or an option.
fn valid_session_id(session_id: &str) -> bool {
    !session_id.is_empty()
        && session_id.len() <= 128
        && session_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

/// Only host root may connect: each identity's lungs daemon runs there, and a
/// process inside an identity reaches the bridge through its lungs, never
/// through this socket.
fn peer_allowed(uid: u32) -> bool {
    uid == 0
}

/// Bind the shared socket, replacing a stale one, owner-only.
fn bind_socket(path: &Path) -> Result<tokio::net::UnixListener> {
    // The parent is shared with the bridge's other sockets, so its mode is
    // left alone; the socket itself is owner-only.
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_socket() => std::fs::remove_file(path)?,
        Ok(_) => anyhow::bail!("{} exists and is not a socket", path.display()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let listener =
        tokio::net::UnixListener::bind(path).with_context(|| format!("bind {}", path.display()))?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(listener)
}

/// Serve the shared lungs socket until the listener fails.
pub async fn serve(
    path: &Path,
    router: Arc<SchemaRouter>,
    engine: Arc<MutationEngine>,
) -> Result<()> {
    let listener = bind_socket(path)?;
    tracing::info!(socket = %path.display(), "lungs socket listening");
    let connections = Arc::new(tokio::sync::Semaphore::new(MAX_CONNECTIONS));
    loop {
        let permit = connections.clone().acquire_owned().await?;
        let (stream, _) = listener.accept().await?;
        let uid = stream.peer_cred().map(|cred| cred.uid()).ok();
        if !uid.is_some_and(peer_allowed) {
            tracing::warn!(?uid, "lungs peer refused");
            continue;
        }
        let router = router.clone();
        let identity = Arc::new(LungsIdentity::new(engine.clone()));
        tokio::spawn(async move {
            let _permit = permit;
            let outcome = async {
                let mut builder = zbus::connection::Builder::unix_stream(stream)
                    .server(zbus::Guid::generate())?
                    .p2p()
                    .auth_mechanism(AuthMechanism::Anonymous)
                    .serve_at(
                        LUNGS_OBJECT_PATH,
                        LungsBootstrap {
                            identity: identity.clone(),
                        },
                    )?;
                for (object_path, object) in router.lungs_objects(identity.clone()).await {
                    builder = builder.serve_at(object_path, object)?;
                }
                let connection = tokio::time::timeout(NEGOTIATION_TIMEOUT, builder.build())
                    .await
                    .context("lungs D-Bus negotiation timed out")??;
                connection.closed().await;
                Ok::<_, anyhow::Error>(())
            }
            .await;
            if let Err(error) = outcome {
                tracing::warn!(%error, "lungs connection ended");
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_ids_are_plain_names() {
        assert!(valid_session_id("b0c777e3-0955-cd8b-3d01-ca12998cae28"));
        for session_id in ["", "../escape", "a/b", "a.b", "x\0y", &"a".repeat(129)] {
            assert!(!valid_session_id(session_id), "{session_id:?}");
        }
    }

    #[test]
    fn only_host_root_may_connect() {
        assert!(peer_allowed(0));
        assert!(!peer_allowed(1000));
        assert!(!peer_allowed(1_000_000));
    }

    #[tokio::test]
    async fn bind_socket_replaces_a_stale_socket_owner_only() {
        let directory = tempfile::tempdir().unwrap();
        // The parent does not exist yet, as on a bridge that binds its lungs
        // before any other socket.
        let path = directory.path().join("opdbus").join("lungs.sock");
        drop(bind_socket(&path).unwrap());
        let _again = bind_socket(&path).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[tokio::test]
    async fn bind_socket_refuses_to_replace_a_file_that_is_not_a_socket() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("lungs.sock");
        std::fs::write(&path, b"keep").unwrap();
        assert!(bind_socket(&path).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"keep");
    }
}
