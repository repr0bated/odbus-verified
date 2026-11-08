//! Authentication state for broker connections, never an identity/grant authority.
//! Each admission and each wake rechecks the original authored SID1, committed
//! credential projection, live principal registry and shared runtime read policy.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context, Result, bail, ensure};
use base64::Engine as _;
use op_identity::sealed_id::SealedId;
use op_plugins::state_plugins::emqx::{WAKE_TRANSPORT_SCOPE, valid_owner_key, wake_topic};
use sha2::{Digest, Sha256};
use tokio::sync::RwLock;
use zeroize::Zeroizing;

use crate::identity_receiver::VerifiedSid1;
use crate::oracle_assertion::AssertionValidator;
use crate::oracle_receiver::VerifiedOracle;
use crate::runtime::{CallerAuthority, Runtime, VerifiedCaller};
use super::protocol::ClientInfo;

const MAX_CONNECTIONS: usize = 2048;
const MAX_PASSWORD_BYTES: usize = 24 * 1024;
const OIA1_PREFIX: &str = "oia1:";

/// Same byte representation as the retained private episode view. A bare
/// principal or a caller-selected client ID is never substituted for this key.
pub(crate) fn owner_key_for_claims(claims: &SealedId) -> Result<String> {
    ensure!(!claims.principal_id.is_empty() && !claims.session_id.is_empty()
        && !claims.session_genesis.is_empty(), "wake identity scope is incomplete");
    let scope = serde_json::to_vec(&[
        claims.principal_id.as_str(), claims.session_id.as_str(), claims.session_genesis.as_str(),
    ])?;
    Ok(hex::encode(Sha256::digest(scope)))
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Verdict { Ignore, Allow, Deny }

#[derive(PartialEq, Eq)]
struct ConnectionKey {
    node: String,
    peer: SocketAddr,
}

impl ConnectionKey {
    fn from_info(info: &ClientInfo) -> Result<Self> {
        ensure!(!info.node.is_empty() && info.node.len() <= 255,
            "broker connection node is missing or invalid");
        let host: IpAddr = info.peerhost.parse().context("invalid broker client peer address")?;
        let port = u16::try_from(info.peerport).context("invalid broker client peer port")?;
        ensure!(port != 0, "broker client peer port is missing");
        Ok(Self { node: info.node.clone(), peer: SocketAddr::new(host, port) })
    }

    fn matches(&self, info: &ClientInfo) -> bool {
        self.node == info.node && self.peer.port() as u32 == info.peerport
            && info.peerhost.parse::<IpAddr>().ok() == Some(self.peer.ip())
    }
}

struct MqttAuthority {
    identity: VerifiedSid1,
    wire: Zeroizing<Vec<u8>>,
    oracle: Option<VerifiedOracle>,
}

impl MqttAuthority {
    fn expires_at(&self) -> i64 {
        let sid_expiry = self.identity.claims().expires_at;
        self.oracle.as_ref().map_or(sid_expiry, |oracle| {
            if sid_expiry == 0 { oracle.assertion_expires_at() }
            else { sid_expiry.min(oracle.assertion_expires_at()) }
        })
    }
}

#[async_trait::async_trait]
impl CallerAuthority for MqttAuthority {
    async fn revalidate(&self) -> Result<()> {
        if let Some(oracle) = &self.oracle { oracle.revalidate().await?; }
        let current = crate::identity_receiver::verify_sid1(&self.wire, WAKE_TRANSPORT_SCOPE).await?;
        ensure!(current.claims() == self.identity.claims(), "MQTT identity changed after CONNECT");
        let expiry = self.expires_at();
        ensure!(expiry == 0 || expiry > chrono::Utc::now().timestamp(), "MQTT identity expired");
        Ok(())
    }
}

pub(super) struct Session {
    pub caller: VerifiedCaller,
    pub owner_key: String,
    pub topic: String,
    connection: ConnectionKey,
    expires_at: i64,
    subscribed: AtomicBool,
}

impl Session {
    fn expired(&self) -> bool {
        self.expires_at != 0 && self.expires_at <= chrono::Utc::now().timestamp()
    }

    pub fn subscribed(&self) -> bool { self.subscribed.load(Ordering::Acquire) }
}

pub(super) struct Sessions {
    pub runtime: Arc<Runtime>,
    oracle: Arc<AssertionValidator>,
    lifecycle: tokio::sync::Mutex<()>,
    connections: RwLock<HashMap<String, Arc<Session>>>,
}

impl Sessions {
    pub fn new(runtime: Arc<Runtime>, oracle: Arc<AssertionValidator>) -> Self {
        Self { runtime, oracle, lifecycle: tokio::sync::Mutex::new(()),
            connections: RwLock::new(HashMap::new()) }
    }

    pub async fn clear(&self) { self.connections.write().await.clear(); }

    pub async fn connected(&self) -> Vec<Arc<Session>> {
        self.connections.read().await.values().cloned().collect()
    }

    pub async fn is_current(&self, session: &Arc<Session>) -> bool {
        self.connections.read().await.get(&session.owner_key)
            .is_some_and(|current| Arc::ptr_eq(current, session))
    }

    /// Serialize eviction with CONNECT installation so a delayed kick cannot
    /// disconnect a newly verified connection using the same owner key.
    pub async fn revoke(&self, session: &Arc<Session>) -> Result<()> {
        let _lifecycle = self.lifecycle.lock().await;
        if !self.is_current(session).await { return Ok(()); }
        self.remove(session).await;
        op_plugins::state_plugins::emqx::dispatch_emqx_method(
            "kick_client", &serde_json::json!({ "client_id": session.owner_key }),
        ).await?;
        Ok(())
    }

    pub async fn remove(&self, session: &Arc<Session>) {
        let mut connections = self.connections.write().await;
        if connections.get(&session.owner_key).is_some_and(|current| Arc::ptr_eq(current, session)) {
            connections.remove(&session.owner_key);
        }
    }

    pub async fn disconnect(&self, info: &ClientInfo) {
        let mut connections = self.connections.write().await;
        if connections.get(&info.clientid).is_some_and(|session| session.connection.matches(info)) {
            connections.remove(&info.clientid);
        }
    }

    pub async fn authenticate(&self, info: &ClientInfo) -> Verdict {
        if !valid_owner_key(&info.clientid) {
            // Keep unrelated, pre-existing broker authentication in charge, but
            // never let an OP-DBUS credential become a different MQTT identity.
            return if info.password.starts_with(op_identity::sealed_id::INLINE_PREFIX)
                || info.password.starts_with(OIA1_PREFIX) { Verdict::Deny } else { Verdict::Ignore };
        }
        match self.verify_connect(info).await {
            Ok(session) => {
                let _lifecycle = self.lifecycle.lock().await;
                if session.caller.authority.revalidate().await.is_err() { return Verdict::Deny; }
                let mut connections = self.connections.write().await;
                connections.retain(|_, current| !current.expired());
                if connections.len() >= MAX_CONNECTIONS && !connections.contains_key(&info.clientid) {
                    return Verdict::Deny;
                }
                connections.insert(info.clientid.clone(), Arc::new(session));
                Verdict::Allow
            }
            Err(_) => Verdict::Deny,
        }
    }

    async fn verify_connect(&self, info: &ClientInfo) -> Result<Session> {
        ensure!(info.password.len() <= MAX_PASSWORD_BYTES, "MQTT credential exceeds its size bound");
        ensure!(info.mountpoint.is_empty(), "wake connections cannot remap their topic namespace");
        let connection = ConnectionKey::from_info(info)?;
        let (wire, oracle) = if let Some(encoded) = info.password.strip_prefix(op_identity::sealed_id::INLINE_PREFIX) {
            (Zeroizing::new(base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(encoded)?), None)
        } else if let Some(encoded) = info.password.strip_prefix(OIA1_PREFIX) {
            let assertion_wire = Zeroizing::new(base64::engine::general_purpose::STANDARD.decode(encoded)?);
            let oracle = crate::oracle_receiver::verify_assertion(
                self.oracle.clone(), &assertion_wire, Some(connection.peer),
            ).await?;
            let wire = Zeroizing::new(
                crate::identity_receiver::authored_transport_wire(oracle.identity()).await?,
            );
            (wire, Some(oracle))
        } else { bail!("MQTT CONNECT requires an authored SID1 or signed OIA1"); };
        let identity = crate::identity_receiver::verify_sid1(&wire, WAKE_TRANSPORT_SCOPE).await?;
        if let Some(oracle) = &oracle {
            ensure!(oracle.identity().claims() == identity.claims(), "Oracle MQTT identity mismatch");
        }
        let owner_key = owner_key_for_claims(identity.claims())?;
        ensure!(owner_key == info.clientid, "MQTT client ID does not match the verified owner scope");
        let authority = Arc::new(MqttAuthority { identity: identity.clone(), wire, oracle });
        let expires_at = authority.expires_at();
        let caller = VerifiedCaller::new(&identity, authority);
        ensure!(self.can_read_any(&caller).await?, "MQTT identity has no admitted readable projection");
        // Shared read admission may await policy/registry I/O. Revalidate once
        // more before installing connection evidence; the cache grants nothing.
        caller.authority.revalidate().await?;
        Ok(Session { caller, topic: wake_topic(&owner_key)?, owner_key, connection,
            expires_at, subscribed: AtomicBool::new(false) })
    }

    pub async fn can_read_any(&self, caller: &VerifiedCaller) -> Result<bool> {
        for plugin in self.runtime.catalog().plugins() {
            if self.runtime.can_read_plugin(caller, &plugin.id).await? { return Ok(true); }
        }
        Ok(false)
    }

    async fn authenticated(&self, info: &ClientInfo) -> Result<Arc<Session>> {
        let session = self.connections.read().await.get(&info.clientid).cloned()
            .context("MQTT connection has no verified CONNECT evidence")?;
        ensure!(session.connection.matches(info) && info.mountpoint.is_empty(), "MQTT connection changed");
        if let Err(error) = session.caller.authority.revalidate().await {
            self.remove(&session).await;
            return Err(error);
        }
        Ok(session)
    }

    pub async fn authorize(&self, info: &ClientInfo, subscribe: bool, topic: &str) -> Verdict {
        if !valid_owner_key(&info.clientid) {
            // A wildcard or shared subscription can overlap the private
            // namespace even when its literal prefix is not `opdbus/`.
            return if topic == "opdbus" || topic.starts_with("opdbus/")
                || topic.starts_with('$') || topic.contains(['#', '+']) {
                Verdict::Deny
            } else { Verdict::Ignore };
        }
        if !subscribe { return Verdict::Deny; }
        let Ok(session) = self.authenticated(info).await else { return Verdict::Deny; };
        if topic != session.topic { return Verdict::Deny; }
        match self.can_read_any(&session.caller).await {
            Ok(true) => Verdict::Allow,
            _ => Verdict::Deny,
        }
    }

    pub async fn subscribed(&self, info: &ClientInfo, topic: &str, subscribed: bool) -> Result<()> {
        let session = self.authenticated(info).await?;
        ensure!(topic == session.topic, "MQTT subscription is not the exact owner topic");
        if subscribed {
            ensure!(self.can_read_any(&session.caller).await?, "no admitted readable projection");
        }
        session.subscribed.store(subscribed, Ordering::Release);
        Ok(())
    }
}
