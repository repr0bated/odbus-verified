//! Pre-TLS transfer of a login-owned browser socket to its committed identity.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result, ensure};
use op_network::browser_ingress::{BrowserIngressMap, socket_cookie};
use op_network::browser_route::{BrowserRouteHandle, CommittedBrowserIdentity, derive_route_handle};
use tokio::sync::Semaphore;

use crate::runtime::Runtime;
use crate::runtime_lungs::BrowserHandoff;

const ROUTE_TTL_NS: u64 = 10_000_000_000;
const MAX_HANDOFFS: usize = 128;

struct RouteEntryGuard {
    map: Arc<BrowserIngressMap>,
    key: op_network::browser_route::BrowserConnectionKey,
}

impl Drop for RouteEntryGuard {
    fn drop(&mut self) {
        let _ = self.map.remove(self.key);
    }
}

#[derive(Clone)]
pub(crate) struct BrowserIngress {
    runtime: Arc<Runtime>,
    map: Arc<BrowserIngressMap>,
    lungs: BrowserHandoff,
    handoffs: Arc<Semaphore>,
}

impl BrowserIngress {
    pub(crate) fn from_env(runtime: Arc<Runtime>, lungs: BrowserHandoff) -> Result<Self> {
        let path = std::env::var_os("OP_BROWSER_INGRESS_MAP")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/sys/fs/bpf/opdbus/browser-ingress/browser_routes"));
        Ok(Self {
            runtime,
            map: Arc::new(BrowserIngressMap::open(&path)?),
            lungs,
            handoffs: Arc::new(Semaphore::new(MAX_HANDOFFS)),
        })
    }

    /// Consume the exact socket accepted by login. No application byte is read.
    pub(crate) async fn handoff(
        &self,
        stream: tokio::net::TcpStream,
        identity: &crate::identity_receiver::VerifiedSid1,
    ) -> Result<()> {
        let committed_session_id = &identity.claims().session_id;
        let _permit = self.handoffs.clone().try_acquire_owned()
            .context("browser handoff bound reached")?;
        let key = socket_cookie(&stream)?;
        let route = self.resolve(committed_session_id).await?;
        self.map.bind(key, route, ROUTE_TTL_NS)?;
        let _entry = RouteEntryGuard { map: self.map.clone(), key };
        let current = self.resolve(committed_session_id).await?;
        ensure!(current == route, "committed browser identity changed during handoff");
        self.map.verify(key, route)?;
        self.lungs.offer(committed_session_id, stream).await
    }

    async fn resolve(&self, session_id: &str) -> Result<BrowserRouteHandle> {
        let catalog = self.runtime.catalog_dir().to_path_buf();
        let state = op_core::projection_shm::shm_state_dir();
        let session_id = session_id.to_owned();
        tokio::task::spawn_blocking(move || {
            let mut matches = crate::identity_receiver::committed_sessions_in(&catalog, &state)?
                .into_iter()
                .filter(|sled| sled.session_id == session_id);
            let sled = matches.next().context("committed browser identity is absent")?;
            ensure!(matches.next().is_none(), "committed browser identity is ambiguous");
            let instance = sled.instance.as_ref().context("committed browser container is absent")?;
            ensure!(crate::identity_binding::identity_instance_matches(&sled, instance),
                "committed browser container binding is invalid");
            let native_uuid = instance.config.as_ref()
                .and_then(|config| config.get("volatile.uuid"))
                .context("committed browser native UUID is absent")?;
            let identity = CommittedBrowserIdentity {
                session_id: &sled.session_id,
                genesis: sled.genesis.as_deref().unwrap_or_default(),
                native_name: &instance.name,
                native_uuid,
                active: sled.active,
                anchored: sled.is_anchored(),
                expires_at: sled.expires_at,
            };
            derive_route_handle(&identity, chrono::Utc::now().timestamp()).map_err(anyhow::Error::from)
        }).await?
    }
}
