//! Authoritative D-Bus sender-to-session identity binding.
//!
//! D-Bus does not traverse the gRPC interceptor. A caller therefore registers
//! its current, bus-assigned unique name with a SID1 sealed envelope (local)
//! or a fresh OIA1 proof (remote human) before it can use `PluginV1.Call`.
//! The registration never accepts a caller-selected principal, session,
//! sender, UID, GID, PID, or genesis value.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use base64::Engine as _;
use futures::StreamExt as _;
use tokio::sync::{Mutex, RwLock};
use zbus::Connection;
use zbus::message::Header;
use zbus::names::BusName;

use crate::interceptor::{GhostbridgeIdentity, load_capability_grants};
use crate::mutation_engine::MutationEngine;
use crate::oracle_assertion::AssertionValidator;

pub const DBUS_IDENTITY_OBJECT_PATH: &str = "/org/opdbus/v1/identity/dbus";
pub const DBUS_IDENTITY_INTERFACE: &str = "org.opdbus.v1.DbusIdentityV1";
pub const DBUS_BIND_CAPABILITY: &str = "cap.identity.dbus.bind@v1";
const MAX_REGISTRATION_PROOF_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
struct DbusPeerCredentials {
    uid: u32,
    effective_uid: u32,
    effective_gid: u32,
    group_ids: Vec<u32>,
    pid: u32,
    process_start_ticks: u64,
}

#[derive(Debug, Clone)]
#[allow(dead_code)] // registered_at is stored for auditing but not yet read
struct DbusBinding {
    unique_name: String,
    peer: DbusPeerCredentials,
    principal_id: String,
    session_id: String,
    session_genesis: String,
    registered_at: i64,
}

fn take_binding(
    bindings: &mut HashMap<String, DbusBinding>,
    sender: &str,
) -> Option<(DbusBinding, bool)> {
    let removed = bindings.remove(sender)?;
    let last_for_session = !bindings
        .values()
        .any(|binding| binding.session_id == removed.session_id);
    Some((removed, last_for_session))
}

#[derive(Debug, thiserror::Error)]
pub enum DbusIdentityError {
    #[error("D-Bus caller has no unique sender")]
    NoSender,
    #[error("D-Bus sender-exit monitor is unavailable")]
    MonitorUnavailable,
    #[error("D-Bus peer credentials are unavailable")]
    PeerCredentialsUnavailable,
    #[error("D-Bus caller has no registered identity binding")]
    NoBinding,
    #[error("D-Bus caller binding is stale")]
    StaleBinding,
    #[error("D-Bus registration proof was rejected")]
    RegistrationProofRejected,
    #[error("D-Bus registration capability was denied")]
    CapabilityDenied,
    #[error("D-Bus authoritative session is unavailable")]
    SessionUnavailable,
}

/// Resolves a bus-owned unique sender through immutable peer credentials and
/// an already-authenticated, still-current human session.
pub struct DbusIdentityResolver {
    connection: Connection,
    bindings: RwLock<HashMap<String, DbusBinding>>,
    engine: Arc<MutationEngine>,
    validator: Arc<AssertionValidator>,
    monitor_live: AtomicBool,
    /// Serializes first-binding activation with last-binding parking.
    lifecycle_transition: Mutex<()>,
}

impl DbusIdentityResolver {
    pub fn new(
        connection: Connection,
        engine: Arc<MutationEngine>,
        validator: Arc<AssertionValidator>,
    ) -> Arc<Self> {
        Arc::new(Self {
            connection,
            bindings: RwLock::new(HashMap::new()),
            engine,
            validator,
            monitor_live: AtomicBool::new(false),
            lifecycle_transition: Mutex::new(()),
        })
    }

    /// Subscribe before exposing the registration object. If the subscription
    /// ever ends, every binding is discarded and resolution fails closed.
    pub async fn start_exit_monitor(self: &Arc<Self>) -> anyhow::Result<()> {
        let resolver = Arc::clone(self);
        let connection = self.connection.clone();
        let (ready_tx, ready_rx) = tokio::sync::oneshot::channel::<Result<(), String>>();

        tokio::spawn(async move {
            let proxy = match zbus::fdo::DBusProxy::new(&connection).await {
                Ok(proxy) => proxy,
                Err(error) => {
                    let _ = ready_tx.send(Err(error.to_string()));
                    return;
                }
            };
            let mut stream = match proxy.receive_name_owner_changed().await {
                Ok(stream) => stream,
                Err(error) => {
                    let _ = ready_tx.send(Err(error.to_string()));
                    return;
                }
            };

            resolver.monitor_live.store(true, Ordering::Release);
            let _ = ready_tx.send(Ok(()));

            while let Some(signal) = stream.next().await {
                let Ok(args) = signal.args() else {
                    continue;
                };
                if args.new_owner().as_ref().is_none()
                    && let Err(error) = resolver.revoke_sender(args.name().as_str()).await
                {
                    tracing::error!(%error, sender = args.name().as_str(), "failed to park disconnected D-Bus identity session");
                }
            }

            resolver.monitor_live.store(false, Ordering::Release);
            if let Err(error) = resolver.revoke_all_senders().await {
                tracing::error!(%error, "failed to park all sessions after D-Bus monitor exit");
            }
            tracing::error!("D-Bus NameOwnerChanged stream ended; identity bindings disabled");
        });

        match ready_rx.await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(error)) => anyhow::bail!("subscribe to D-Bus NameOwnerChanged: {error}"),
            Err(_) => anyhow::bail!("D-Bus NameOwnerChanged monitor stopped during startup"),
        }
    }

    /// Bind only the sender named by this message header. The opaque proof is
    /// a SID1 sealed envelope or a fresh OIA1 envelope; all identity/session
    /// values are resolved by the existing validator and MutationEngine.
    pub async fn register_current_sender(
        &self,
        header: &Header<'_>,
        registration_proof: &[u8],
    ) -> Result<GhostbridgeIdentity, DbusIdentityError> {
        if !self.monitor_live.load(Ordering::Acquire) {
            return Err(DbusIdentityError::MonitorUnavailable);
        }
        if registration_proof.is_empty() || registration_proof.len() > MAX_REGISTRATION_PROOF_BYTES
        {
            return Err(DbusIdentityError::RegistrationProofRejected);
        }
        if registration_proof.starts_with(b"SID1") {
            return self
                .register_current_sender_sid1(header, registration_proof)
                .await;
        }

        let sender = sender_from_header(header)?;
        let initial_peer = self.read_peer_credentials(&sender).await?;
        let now = chrono::Utc::now().timestamp();
        let mut pending = self
            .validator
            .validate_pending_with_bootstrap(registration_proof, None, now, false)
            .map_err(|error| {
                tracing::warn!(%error, sender, "D-Bus registration OIA rejected");
                DbusIdentityError::RegistrationProofRejected
            })?;

        let session = self
            .verified_session_for_activation(&pending.identity().human_pubkey)
            .await?
            .ok_or(DbusIdentityError::SessionUnavailable)?;
        pending.identity_mut().session_id = session.session_id.clone();
        pending.identity_mut().session_genesis = session.genesis_hex.clone();

        if !load_capability_grants(&pending.identity().principal_id).contains(DBUS_BIND_CAPABILITY)
        {
            return Err(DbusIdentityError::CapabilityDenied);
        }

        self.authoritative_identity(&pending.identity().principal_id, &session.session_id, false)
            .await?;

        // Re-read credentials after proof validation so a disconnect or
        // process replacement during registration cannot inherit the proof.
        let current_peer = self.read_peer_credentials(&sender).await?;
        if current_peer != initial_peer {
            return Err(DbusIdentityError::StaleBinding);
        }

        let identity = self.validator.consume_pending(pending).map_err(|error| {
            tracing::warn!(%error, sender, "D-Bus registration OIA replay rejected");
            DbusIdentityError::RegistrationProofRejected
        })?;
        // Serialize binding insertion with disconnect/logout. The first
        // authenticated binding activates (and starts) the parked container;
        // additional bindings share that live term without a second start.
        let _transition = self.lifecycle_transition.lock().await;
        {
            let bindings = self.bindings.read().await;
            if bindings
                .get(&sender)
                .is_some_and(|binding| binding.session_id != identity.session_id)
            {
                return Err(DbusIdentityError::StaleBinding);
            }
        }
        let first_binding = !self
            .bindings
            .read()
            .await
            .values()
            .any(|binding| binding.session_id == identity.session_id);
        crate::identity_sled_dispatch::activate_session(
            self.engine.as_ref(),
            &identity.session_id,
        )
        .await
        .map_err(|error| {
            tracing::error!(%error, session_id = %identity.session_id, "authenticated session activation failed");
            DbusIdentityError::SessionUnavailable
        })?;
        let resolved = match self
            .authoritative_identity(&identity.principal_id, &identity.session_id, true)
            .await
        {
            Ok(resolved) => resolved,
            Err(error) => {
                if first_binding {
                    let _ = crate::identity_sled_dispatch::deactivate_session(
                        self.engine.as_ref(),
                        &identity.session_id,
                    )
                    .await;
                }
                return Err(error);
            }
        };

        self.bindings.write().await.insert(
            sender.clone(),
            DbusBinding {
                unique_name: sender.clone(),
                peer: current_peer.clone(),
                principal_id: resolved.principal_id.clone(),
                session_id: resolved.session_id.clone(),
                session_genesis: resolved.session_genesis.clone(),
                registered_at: now,
            },
        );
        drop(_transition);

        // Close the final insert race: if the sender vanished between the
        // second credential read and insertion, remove the just-added entry.
        if !matches!(
            self.read_peer_credentials(&sender).await,
            Ok(peer) if peer == current_peer
        ) {
            if let Err(error) = self.revoke_sender(&sender).await {
                tracing::error!(%error, sender, "failed to park stale D-Bus identity session");
            }
            return Err(DbusIdentityError::StaleBinding);
        }

        // Publish the in-memory context only after proof consumption,
        // activation, authoritative resolution, binding insertion, and the
        // final peer-credential race check have all succeeded.
        self.engine
            .register_session_context(crate::mutation_engine::SessionContext {
                genesis_hex: resolved.session_genesis.clone(),
                session_id: resolved.session_id.clone(),
                wireguard_pubkey: session.wireguard_pubkey,
                sealed_id_digest: crate::identity_sled_dispatch::stored_session(
                    self.engine.as_ref(),
                    &resolved.session_id,
                )
                .await
                .ok()
                .flatten()
                .and_then(|sled| {
                    crate::mutation_engine::sealed_id_digest(sled.sealed_id.as_deref())
                }),
            })
            .await;

        tracing::info!(
            sender,
            principal_id = %resolved.principal_id,
            session_id = %resolved.session_id,
            "registered current D-Bus sender identity"
        );
        Ok(resolved)
    }

    async fn register_current_sender_sid1(
        &self,
        header: &Header<'_>,
        registration_proof: &[u8],
    ) -> Result<GhostbridgeIdentity, DbusIdentityError> {
        let sender = sender_from_header(header)?;
        let initial_peer = self.read_peer_credentials(&sender).await?;
        let now = chrono::Utc::now().timestamp();
        let claims = crate::identity_sled_dispatch::authenticate_sid1_wire(
            self.engine.as_ref(),
            registration_proof,
            "dbus",
        )
        .await
        .map_err(|error| {
            tracing::warn!(%error, sender, "D-Bus registration SID1 rejected");
            DbusIdentityError::RegistrationProofRejected
        })?;

        if !load_capability_grants(&claims.principal_id).contains(DBUS_BIND_CAPABILITY) {
            return Err(DbusIdentityError::CapabilityDenied);
        }

        self.authoritative_identity(&claims.principal_id, &claims.session_id, false)
            .await?;

        let current_peer = self.read_peer_credentials(&sender).await?;
        if current_peer != initial_peer {
            return Err(DbusIdentityError::StaleBinding);
        }

        let _transition = self.lifecycle_transition.lock().await;
        {
            let bindings = self.bindings.read().await;
            if bindings
                .get(&sender)
                .is_some_and(|binding| binding.session_id != claims.session_id)
            {
                return Err(DbusIdentityError::StaleBinding);
            }
        }
        let first_binding = !self
            .bindings
            .read()
            .await
            .values()
            .any(|binding| binding.session_id == claims.session_id);
        crate::identity_sled_dispatch::activate_session(self.engine.as_ref(), &claims.session_id)
            .await
            .map_err(|error| {
                tracing::error!(%error, session_id = %claims.session_id, "authenticated session activation failed");
                DbusIdentityError::SessionUnavailable
            })?;
        let resolved = match self
            .authoritative_identity(&claims.principal_id, &claims.session_id, true)
            .await
        {
            Ok(resolved) => resolved,
            Err(error) => {
                if first_binding {
                    let _ = crate::identity_sled_dispatch::deactivate_session(
                        self.engine.as_ref(),
                        &claims.session_id,
                    )
                    .await;
                }
                return Err(error);
            }
        };

        self.bindings.write().await.insert(
            sender.clone(),
            DbusBinding {
                unique_name: sender.clone(),
                peer: current_peer.clone(),
                principal_id: resolved.principal_id.clone(),
                session_id: resolved.session_id.clone(),
                session_genesis: resolved.session_genesis.clone(),
                registered_at: now,
            },
        );
        drop(_transition);

        if !matches!(
            self.read_peer_credentials(&sender).await,
            Ok(peer) if peer == current_peer
        ) {
            if let Err(error) = self.revoke_sender(&sender).await {
                tracing::error!(%error, sender, "failed to park stale D-Bus identity session");
            }
            return Err(DbusIdentityError::StaleBinding);
        }

        self.engine
            .register_session_context(crate::mutation_engine::SessionContext {
                genesis_hex: resolved.session_genesis.clone(),
                session_id: resolved.session_id.clone(),
                wireguard_pubkey: claims.wireguard_pubkey,
                sealed_id_digest: crate::identity_sled_dispatch::stored_session(
                    self.engine.as_ref(),
                    &resolved.session_id,
                )
                .await
                .ok()
                .flatten()
                .and_then(|sled| {
                    crate::mutation_engine::sealed_id_digest(sled.sealed_id.as_deref())
                }),
            })
            .await;

        tracing::info!(
            sender,
            principal_id = %resolved.principal_id,
            session_id = %resolved.session_id,
            "registered current D-Bus sender identity from SID1"
        );
        Ok(resolved)
    }

    /// Resolve and revalidate the binding for the current message sender.
    pub async fn resolve(
        &self,
        header: &Header<'_>,
    ) -> Result<GhostbridgeIdentity, DbusIdentityError> {
        if !self.monitor_live.load(Ordering::Acquire) {
            return Err(DbusIdentityError::MonitorUnavailable);
        }
        let sender = sender_from_header(header)?;
        let binding = self
            .bindings
            .read()
            .await
            .get(&sender)
            .cloned()
            .ok_or(DbusIdentityError::NoBinding)?;

        let peer = match self.read_peer_credentials(&sender).await {
            Ok(peer) if peer == binding.peer => peer,
            _ => {
                let _ = self.revoke_sender(&sender).await;
                return Err(DbusIdentityError::StaleBinding);
            }
        };
        if peer.process_start_ticks != binding.peer.process_start_ticks
            || binding.unique_name != sender
        {
            let _ = self.revoke_sender(&sender).await;
            return Err(DbusIdentityError::StaleBinding);
        }

        let identity = match self
            .authoritative_identity(&binding.principal_id, &binding.session_id, true)
            .await
        {
            Ok(identity) if identity.session_genesis == binding.session_genesis => identity,
            _ => {
                let _ = self.revoke_sender(&sender).await;
                return Err(DbusIdentityError::SessionUnavailable);
            }
        };
        Ok(identity)
    }

    /// Revoke only the bus-assigned current sender named by the caller header.
    pub async fn logout_current_sender(
        &self,
        header: &Header<'_>,
    ) -> Result<bool, DbusIdentityError> {
        let sender = sender_from_header(header)?;
        self.revoke_sender(&sender).await
    }

    async fn revoke_sender(&self, sender: &str) -> Result<bool, DbusIdentityError> {
        let _transition = self.lifecycle_transition.lock().await;
        let removed = {
            let mut bindings = self.bindings.write().await;
            take_binding(&mut bindings, sender)
        };
        let Some((removed, last_binding)) = removed else {
            return Ok(false);
        };
        if last_binding {
            let parking = crate::identity_sled_dispatch::deactivate_session(
                self.engine.as_ref(),
                &removed.session_id,
            )
            .await;
            self.engine
                .forget_session_context(&removed.session_id)
                .await;
            parking.map_err(|error| {
                tracing::error!(%error, session_id = %removed.session_id, "identity container parking failed");
                DbusIdentityError::SessionUnavailable
            })?;
        }
        tracing::info!(sender, session_id = %removed.session_id, last_binding, "revoked D-Bus sender identity binding");
        Ok(true)
    }

    async fn revoke_all_senders(&self) -> Result<(), DbusIdentityError> {
        let _transition = self.lifecycle_transition.lock().await;
        let sessions: std::collections::HashSet<String> = self
            .bindings
            .write()
            .await
            .drain()
            .map(|(_, binding)| binding.session_id)
            .collect();
        let mut failed = false;
        for session_id in sessions {
            if let Err(error) =
                crate::identity_sled_dispatch::deactivate_session(self.engine.as_ref(), &session_id)
                    .await
            {
                failed = true;
                tracing::error!(%error, %session_id, "identity container parking failed during binding drain");
            }
            self.engine.forget_session_context(&session_id).await;
        }
        if failed {
            Err(DbusIdentityError::SessionUnavailable)
        } else {
            Ok(())
        }
    }

    async fn read_peer_credentials(
        &self,
        sender: &str,
    ) -> Result<DbusPeerCredentials, DbusIdentityError> {
        let proxy = zbus::fdo::DBusProxy::new(&self.connection)
            .await
            .map_err(|_| DbusIdentityError::PeerCredentialsUnavailable)?;
        let bus_name =
            BusName::try_from(sender).map_err(|_| DbusIdentityError::PeerCredentialsUnavailable)?;
        let credentials = proxy
            .get_connection_credentials(bus_name)
            .await
            .map_err(|_| DbusIdentityError::PeerCredentialsUnavailable)?;
        let uid = credentials
            .unix_user_id()
            .ok_or(DbusIdentityError::PeerCredentialsUnavailable)?;
        let pid = credentials
            .process_id()
            .filter(|pid| *pid != 0)
            .ok_or(DbusIdentityError::PeerCredentialsUnavailable)?;
        let mut group_ids = credentials
            .unix_group_ids()
            .cloned()
            .filter(|groups| !groups.is_empty())
            .ok_or(DbusIdentityError::PeerCredentialsUnavailable)?;
        group_ids.sort_unstable();
        group_ids.dedup();

        let first_start = read_process_start_ticks(pid)
            .map_err(|_| DbusIdentityError::PeerCredentialsUnavailable)?;
        let (effective_uid, effective_gid) =
            read_effective_ids(pid).map_err(|_| DbusIdentityError::PeerCredentialsUnavailable)?;
        let process_start_ticks = read_process_start_ticks(pid)
            .map_err(|_| DbusIdentityError::PeerCredentialsUnavailable)?;
        if first_start != process_start_ticks
            || effective_uid != uid
            || !group_ids.contains(&effective_gid)
        {
            return Err(DbusIdentityError::PeerCredentialsUnavailable);
        }

        Ok(DbusPeerCredentials {
            uid,
            effective_uid,
            effective_gid,
            group_ids,
            pid,
            process_start_ticks,
        })
    }

    /// Resolve a verified login against an existing sled without requiring
    /// that sled to already be active. This is the sole parked-session
    /// exception: the fresh OIA proof and principal checks precede it, and
    /// registration immediately activates the exact server-derived session.
    async fn verified_session_for_activation(
        &self,
        wireguard_pubkey: &str,
    ) -> Result<Option<crate::mutation_engine::SessionContext>, DbusIdentityError> {
        let valid_wireguard_key = base64::engine::general_purpose::STANDARD
            .decode(wireguard_pubkey.trim())
            .is_ok_and(|raw| raw.len() == 32);
        if !valid_wireguard_key {
            return Ok(None);
        }
        let session_id = op_identity::session::derive_session_id(wireguard_pubkey);
        if let Some(record) = crate::identity_sled_dispatch::stored_session(
            self.engine.as_ref(),
            &session_id,
        )
        .await
        .map_err(|error| {
            tracing::error!(%error, %session_id, "identity credential projection unavailable");
            DbusIdentityError::SessionUnavailable
        })? {
            let now = chrono::Utc::now().timestamp();
            if !record.is_anchored()
                || record.wireguard_pubkey != wireguard_pubkey
                || !record
                    .expires_at
                    .is_none_or(|expires_at| expires_at == 0 || expires_at > now)
            {
                return Ok(None);
            }
            let context = crate::mutation_engine::SessionContext {
                genesis_hex: record.genesis.unwrap_or_default(),
                sealed_id_digest: crate::mutation_engine::sealed_id_digest(
                    record.sealed_id.as_deref(),
                ),
                session_id: record.session_id,
                wireguard_pubkey: record.wireguard_pubkey,
            };
            return Ok(Some(context));
        }
        // D-Bus login activates an already provisioned/authored sled. Minting
        // here would cache a SessionContext before nonce consumption and the
        // final peer-credential check; provisioning/arrival owns that step.
        Ok(None)
    }

    async fn authoritative_identity(
        &self,
        expected_principal: &str,
        session_id: &str,
        require_active: bool,
    ) -> Result<GhostbridgeIdentity, DbusIdentityError> {
        authoritative_session_identity(
            self.engine.as_ref(),
            Some(expected_principal),
            session_id,
            require_active,
        )
        .await
    }
}

/// Resolve a session to its registered principal from the committed sled
/// projection and the key registry alone. The session must be anchored,
/// unexpired and, when `require_active`, active; its WireGuard key must map
/// to an unrevoked principal, which must equal `expected_principal` when one
/// is given. Nothing here trusts a caller-supplied principal or genesis.
pub(crate) async fn authoritative_session_identity(
    engine: &MutationEngine,
    expected_principal: Option<&str>,
    session_id: &str,
    require_active: bool,
) -> Result<GhostbridgeIdentity, DbusIdentityError> {
    let session = crate::identity_sled_dispatch::stored_session(engine, session_id)
        .await
        .map_err(|error| {
            tracing::error!(%error, %session_id, "identity credential projection unavailable");
            DbusIdentityError::SessionUnavailable
        })?
        .filter(|session| session.is_anchored())
        .filter(|session| !require_active || session.active)
        .filter(|session| {
            session
                .expires_at
                .is_none_or(|expires| expires == 0 || expires > chrono::Utc::now().timestamp())
        })
        .ok_or(DbusIdentityError::SessionUnavailable)?;
    let session_genesis = session
        .genesis
        .filter(|genesis| !genesis.is_empty())
        .ok_or(DbusIdentityError::SessionUnavailable)?;
    if session.wireguard_pubkey.is_empty() {
        return Err(DbusIdentityError::SessionUnavailable);
    }

    let principal =
        crate::human_principal_dispatch::resolve_key_for_assertion(&session.wireguard_pubkey)
            .await
            .map_err(|_| DbusIdentityError::SessionUnavailable)?
            .filter(|record| record.revoked_at == 0)
            .filter(|record| {
                expected_principal.is_none_or(|expected| record.principal_id == expected)
            })
            .ok_or(DbusIdentityError::SessionUnavailable)?;

    Ok(GhostbridgeIdentity {
        principal_id: principal.principal_id,
        session_id: session.session_id,
        session_genesis,
    })
}

/// Separate bootstrap interface: PluginV1 remains unavailable until this
/// current-sender operation has established the authoritative binding.
pub struct DbusIdentityBootstrap {
    resolver: Arc<DbusIdentityResolver>,
}

impl DbusIdentityBootstrap {
    pub fn new(resolver: Arc<DbusIdentityResolver>) -> Self {
        Self { resolver }
    }
}

#[zbus::interface(name = "org.opdbus.v1.DbusIdentityV1")]
impl DbusIdentityBootstrap {
    #[zbus(name = "RegisterCurrentSender")]
    async fn register_current_sender(
        &self,
        registration_proof: Vec<u8>,
        #[zbus(header)] header: Header<'_>,
    ) -> zbus::fdo::Result<bool> {
        self.resolver
            .register_current_sender(&header, &registration_proof)
            .await
            .map(|_| true)
            .map_err(|error| zbus::fdo::Error::AccessDenied(error.to_string()))
    }

    #[zbus(name = "LogoutCurrentSender")]
    async fn logout_current_sender(
        &self,
        #[zbus(header)] header: Header<'_>,
    ) -> zbus::fdo::Result<bool> {
        self.resolver
            .logout_current_sender(&header)
            .await
            .map_err(|error| zbus::fdo::Error::AccessDenied(error.to_string()))
    }
}

fn sender_from_header(header: &Header<'_>) -> Result<String, DbusIdentityError> {
    header
        .sender()
        .map(ToString::to_string)
        .ok_or(DbusIdentityError::NoSender)
}

fn proc_path(pid: u32, file: &str) -> PathBuf {
    PathBuf::from("/proc").join(pid.to_string()).join(file)
}

fn read_process_start_ticks(pid: u32) -> std::io::Result<u64> {
    let stat = std::fs::read_to_string(proc_path(pid, "stat"))?;
    parse_process_start_ticks(&stat)
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid proc stat"))
}

fn parse_process_start_ticks(stat: &str) -> Option<u64> {
    let (_, fields) = stat.rsplit_once(") ")?;
    // The suffix starts at field 3 (`state`); process start time is field 22.
    fields.split_whitespace().nth(19)?.parse().ok()
}

fn read_effective_ids(pid: u32) -> std::io::Result<(u32, u32)> {
    let status = std::fs::read_to_string(proc_path(pid, "status"))?;
    let effective = |label: &str| -> Option<u32> {
        status
            .lines()
            .find_map(|line| line.strip_prefix(label))?
            .split_whitespace()
            .nth(1)?
            .parse()
            .ok()
    };
    match (effective("Uid:"), effective("Gid:")) {
        (Some(uid), Some(gid)) => Ok((uid, gid)),
        _ => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "effective IDs absent from proc status",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Stdio;
    use std::time::Duration;
    use tokio::io::{AsyncBufReadExt as _, BufReader};

    fn peer(pid: u32, start: u64) -> DbusPeerCredentials {
        DbusPeerCredentials {
            uid: 1001,
            effective_uid: 1001,
            effective_gid: 1001,
            group_ids: vec![1001, 1003],
            pid,
            process_start_ticks: start,
        }
    }

    fn binding(unique_name: &str, peer: DbusPeerCredentials) -> DbusBinding {
        DbusBinding {
            unique_name: unique_name.to_string(),
            peer,
            principal_id: "principal-test".to_string(),
            session_id: "session-test".to_string(),
            session_genesis: "genesis-test".to_string(),
            registered_at: 1,
        }
    }

    #[test]
    fn parses_proc_stat_with_spaces_and_parentheses_in_comm() {
        let mut fields = vec!["S".to_string()];
        fields.extend((4..=21).map(|field| field.to_string()));
        fields.push("987654".to_string());
        fields.push("23".to_string());
        let stat = format!("42 (worker name (local)) {}\n", fields.join(" "));
        assert_eq!(parse_process_start_ticks(&stat), Some(987654));
    }

    #[test]
    fn pid_reuse_invalidates_bound_process_credentials() {
        assert_ne!(peer(4242, 100), peer(4242, 101));
    }

    #[test]
    fn revocation_parks_only_after_last_session_binding() {
        let mut bindings = HashMap::from([
            (":1.10".to_string(), binding(":1.10", peer(10, 100))),
            (":1.11".to_string(), binding(":1.11", peer(11, 101))),
        ]);

        let (first, first_was_last) = take_binding(&mut bindings, ":1.10").expect("first");
        assert_eq!(first.unique_name, ":1.10");
        assert!(!first_was_last, "shared session must remain running");
        let (second, second_was_last) = take_binding(&mut bindings, ":1.11").expect("second");
        assert_eq!(second.unique_name, ":1.11");
        assert!(second_was_last, "last logout must park the session");
        assert!(bindings.is_empty());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn private_bus_injects_current_sender_and_revokes_on_exit() {
        let mut command = tokio::process::Command::new("dbus-daemon");
        command
            .args(["--session", "--nofork", "--nopidfile", "--print-address=1"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        let mut daemon = command.spawn().expect("spawn private dbus-daemon");
        let stdout = daemon.stdout.take().expect("private bus stdout");
        let mut lines = BufReader::new(stdout).lines();
        let address = tokio::time::timeout(Duration::from_secs(3), lines.next_line())
            .await
            .expect("private bus address timeout")
            .expect("read private bus address")
            .expect("private bus printed no address");

        let service = zbus::connection::Builder::address(address.as_str())
            .expect("service bus address")
            .build()
            .await
            .expect("connect service to private bus");
        let client = zbus::connection::Builder::address(address.as_str())
            .expect("client bus address")
            .build()
            .await
            .expect("connect client to private bus");
        let event_chain = Arc::new(tokio::sync::RwLock::new(op_state_store::EventChain::new(
            op_state_store::ChainConfig::default(),
        )));
        let engine = Arc::new(MutationEngine::new(
            event_chain,
            Arc::new(op_network::rovs_proxy::OvsdbDbusClient::new()),
        ));
        let validator = Arc::new(AssertionValidator::new(
            crate::oracle_assertion::DecoyTrustStore::parse_bytes(b"{}"),
        ));
        let resolver = DbusIdentityResolver::new(service.clone(), engine, validator);
        resolver
            .start_exit_monitor()
            .await
            .expect("start private-bus exit monitor");
        service
            .object_server()
            .at(
                DBUS_IDENTITY_OBJECT_PATH,
                DbusIdentityBootstrap::new(resolver.clone()),
            )
            .await
            .expect("mount identity bootstrap");
        service
            .request_name("org.opdbus.test.Identity")
            .await
            .expect("request private test name");

        let sender = client
            .unique_name()
            .expect("client has unique name")
            .to_string();
        let captured = resolver
            .read_peer_credentials(&sender)
            .await
            .expect("capture bus-owned client credentials");
        {
            let mut bindings = resolver.bindings.write().await;
            bindings.insert(sender.clone(), binding(&sender, captured.clone()));
            bindings.insert(
                ":1.999999".to_string(),
                binding(":1.999999", captured.clone()),
            );
        }

        let proxy = zbus::Proxy::new(
            &client,
            "org.opdbus.test.Identity",
            DBUS_IDENTITY_OBJECT_PATH,
            DBUS_IDENTITY_INTERFACE,
        )
        .await
        .expect("identity bootstrap proxy");
        let removed: bool = proxy
            .call("LogoutCurrentSender", &())
            .await
            .expect("logout current sender");
        assert!(removed);
        {
            let bindings = resolver.bindings.read().await;
            assert!(!bindings.contains_key(&sender));
            assert!(bindings.contains_key(":1.999999"));
        }

        resolver
            .bindings
            .write()
            .await
            .insert(sender.clone(), binding(&sender, captured));
        drop(proxy);
        drop(client);
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if !resolver.bindings.read().await.contains_key(&sender) {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("NameOwnerChanged did not revoke disconnected sender");
        assert!(resolver.bindings.read().await.contains_key(":1.999999"));

        daemon.kill().await.expect("stop private dbus-daemon");
        let _ = daemon.wait().await;
    }
}
