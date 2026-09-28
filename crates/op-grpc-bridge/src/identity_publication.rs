//! Identity's durable intent/publication boundary, owned by MutationEngine.
//! The identity producer supplies typed state; only it mints SealedId bytes.
use super::{ChangeSource, ChangeType, MutationEngine, StateChange, identity_projection_gid};
use anyhow::{Context, Result};
use op_core::projection_shm::{CredentialPublication, ProjectionCommit};
use simd_json::OwnedValue;

type State<T> = (Option<OwnedValue>, T);

impl MutationEngine {
    pub(crate) fn configure_nextdns_bindings(
        &self,
        bindings: crate::identity_nextdns::NextDnsBindings,
    ) -> Result<()> {
        self.nextdns_bindings
            .set(bindings)
            .map_err(|_| anyhow::anyhow!("NextDNS identity assignments are already configured"))
    }

    pub(crate) fn assigned_nextdns(
        &self,
        key: &str,
    ) -> Option<op_identity::sealed_id::NextDnsContext> {
        self.nextdns_bindings
            .get()
            .and_then(|bindings| bindings.get(key))
            .cloned()
    }

    /// Author a replacement SID1 for an already anchored session. This keeps
    /// the arrival claims immutable while refreshing transport scope and the
    /// operator-owned NextDNS assignment. Missing policy cannot erase a claim.
    pub(crate) fn refresh_identity_sealed_id(
        &self,
        sled: &op_plugins::state_plugins::identity_sled::ContainerIdentitySled,
    ) -> Result<String> {
        anyhow::ensure!(
            sled.is_anchored(),
            "credential refresh requires an anchored session"
        );
        let stamp = super::GenesisStamp {
            session_id: sled.session_id.clone(),
            wireguard_pubkey: sled.wireguard_pubkey.clone(),
            genesis_hex: sled.genesis.clone().unwrap_or_default(),
            trace_id: sled.trace_id.clone(),
            sealed_id: String::new(),
            arrival_timestamp: sled.arrival_timestamp,
            chain_head_at_arrival: sled.chain_head_at_arrival.clone(),
            catalog_hash_at_arrival: sled.catalog_hash_at_arrival.clone(),
            head_timestamp_at_arrival: sled.head_timestamp_at_arrival,
        };
        let nextdns = self.assigned_nextdns(&sled.wireguard_pubkey);
        if let Some(inline) = sled.sealed_id.as_deref() {
            let old = op_identity::sealed_id::SealedId::from_inline_ref(inline)?;
            anyhow::ensure!(
                old.nextdns.is_none() || nextdns.is_some(),
                "credential refresh requires the existing NextDNS assignment to remain configured"
            );
        }
        super::sealed_sealed_id(
            &stamp,
            sled.schema_version,
            sled.expires_at.unwrap_or(0),
            nextdns,
        )
    }

    /// Recover the explicitly configured durable owner before an embedding
    /// exposes transports. This trusted Rust API is not a plugin method.
    pub async fn recover_configured_identity_store(&self) -> Result<()> {
        crate::identity_sled_dispatch::recover_identity_startup(self).await
    }

    /// Install the one identity store owned by this engine. This trusted Rust
    /// embedding API is not exposed as a transport method. An in-memory store
    /// is suitable for isolated fixtures, not crash-durable deployment.
    pub fn configure_identity_store(&self, store: op_cozo_store::CozoGraphShuttle) -> Result<()> {
        self.identity_sled_store
            .set(crate::identity_sled_dispatch::IdentitySledStore::new(store))
            .map_err(|_| anyhow::anyhow!("identity store is already initialized"))
    }

    pub(crate) fn identity_sled_store(
        &self,
    ) -> Result<&crate::identity_sled_dispatch::IdentitySledStore> {
        self.identity_sled_store
            .get()
            .context("identity store is not initialized")
    }

    pub(crate) async fn initialize_identity_sled_store(&self) -> Result<()> {
        let path = std::env::var_os("IDENTITY_SLED_COZO_DB_PATH")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| "/var/lib/op-dbus/identity-cozo".into());
        self.identity_sled_store
            .get_or_try_init(|| async move {
                let cozo = tokio::task::spawn_blocking(move || {
                    op_cozo_store::CozoGraphShuttle::new_persistent_private(path)
                })
                .await??;
                Ok::<_, anyhow::Error>(crate::identity_sled_dispatch::IdentitySledStore::new(cozo))
            })
            .await?;
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn install_identity_sled_store_for_test(
        &self,
        store: crate::identity_sled_dispatch::IdentitySledStore,
    ) -> Result<()> {
        self.identity_sled_store
            .set(store)
            .map_err(|_| anyhow::anyhow!("identity store is already initialized"))
    }

    /// Prepare an arrival stamp without mutating identity, Cozo, the event
    /// chain or SHM. The identity producer submits these exact bytes with its
    /// typed durable intent; readers and clients never reconstruct them.
    pub(crate) async fn mint_identity_genesis_stamp(
        &self,
        session_id: &str,
        wireguard_pubkey: &str,
        expires_at: i64,
    ) -> Result<super::GenesisStamp> {
        use base64::Engine as _;
        let key = base64::engine::general_purpose::STANDARD.decode(wireguard_pubkey)?;
        let key: [u8; 32] = key
            .try_into()
            .map_err(|_| anyhow::anyhow!("identity public key must contain 32 bytes"))?;
        anyhow::ensure!(
            base64::engine::general_purpose::STANDARD.encode(key) == wireguard_pubkey,
            "identity public key must use canonical base64"
        );
        anyhow::ensure!(
            op_identity::session::derive_session_id(wireguard_pubkey) == session_id,
            "identity session does not match its public key"
        );
        anyhow::ensure!(expires_at >= 0, "identity expiry cannot be negative");
        let (chain_head_hex, head_timestamp) = {
            let chain = self.event_chain.read().await;
            (
                chain.last_hash().to_string(),
                chain
                    .events()
                    .last()
                    .map(|event| event.timestamp.timestamp())
                    .unwrap_or(0),
            )
        };
        // Preserve the actual EventChain digest. Its existing MD5 chain heads
        // contain 16 bytes; accepting a future 32-byte digest does not justify
        // replacing current native hashes with zeroes or padded values.
        let chain_head = hex::decode(&chain_head_hex)?;
        anyhow::ensure!(
            matches!(chain_head.len(), 16 | 32),
            "identity arrival chain head has an unsupported digest width"
        );
        let catalog_dir = crate::sealed_schema_reader::blob_catalog_dir_from_env();
        let catalog = tokio::task::spawn_blocking(move || {
            crate::sealed_schema_reader::read_manifest_pinned_catalog(&catalog_dir)
        })
        .await??;
        anyhow::ensure!(
            catalog.plugins.contains_key("identity_sled"),
            "identity plugin is not sealed"
        );
        let catalog_hash: [u8; 32] = hex::decode(&catalog.catalog_hash)?
            .try_into()
            .map_err(|_| anyhow::anyhow!("identity arrival catalog hash is invalid"))?;
        let arrival_timestamp = chrono::Utc::now().timestamp();
        let genesis = op_identity::session_genesis::mint_genesis(
            &key,
            &chain_head,
            head_timestamp,
            &catalog_hash,
            arrival_timestamp,
        );
        let mut stamp = super::GenesisStamp {
            session_id: session_id.into(),
            wireguard_pubkey: wireguard_pubkey.into(),
            genesis_hex: hex::encode(genesis),
            trace_id: hex::encode(uuid::Uuid::new_v4().as_bytes()),
            sealed_id: String::new(),
            arrival_timestamp,
            chain_head_at_arrival: chain_head_hex,
            catalog_hash_at_arrival: catalog.catalog_hash,
            head_timestamp_at_arrival: head_timestamp,
        };
        stamp.sealed_id = super::sealed_sealed_id(
            &stamp,
            op_plugins::state_plugins::identity_sled::RECORD_FORMAT,
            expires_at,
            self.assigned_nextdns(wireguard_pubkey),
        )?;
        Ok(stamp)
    }

    pub(crate) async fn commit_identity_intent<T, F>(
        &self,
        operation_id: String,
        commit: F,
    ) -> Result<CredentialPublication<T>>
    where
        T: Send + 'static,
        F: FnOnce(Option<&[u8]>) -> Result<State<T>> + Send + 'static,
    {
        self.run_identity_publication(operation_id, Some(commit), None::<fn() -> Result<State<T>>>)
            .await
    }

    pub(crate) async fn recover_identity_projection<T, F>(
        &self,
        operation_id: String,
        load: F,
    ) -> Result<CredentialPublication<T>>
    where
        T: Send + 'static,
        F: FnOnce() -> Result<State<T>> + Send + 'static,
    {
        self.run_identity_publication(
            operation_id,
            None::<fn(Option<&[u8]>) -> Result<State<T>>>,
            Some(load),
        )
        .await
    }

    async fn run_identity_publication<T, C, R>(
        &self,
        operation_id: String,
        commit: Option<C>,
        recover: Option<R>,
    ) -> Result<CredentialPublication<T>>
    where
        T: Send + 'static,
        C: FnOnce(Option<&[u8]>) -> Result<State<T>> + Send + 'static,
        R: FnOnce() -> Result<State<T>> + Send + 'static,
    {
        let state_dir = op_core::projection_shm::shm_state_dir();
        let catalog = crate::sealed_schema_reader::blob_catalog_dir_from_env();
        let gid = identity_projection_gid(&state_dir)?.as_raw();
        self.run_identity_publication_in(state_dir, catalog, gid, operation_id, commit, recover)
            .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn run_identity_publication_in<T, C, R>(
        &self,
        state_dir: String,
        catalog: std::path::PathBuf,
        gid: u32,
        operation_id: String,
        commit: Option<C>,
        recover: Option<R>,
    ) -> Result<CredentialPublication<T>>
    where
        T: Send + 'static,
        C: FnOnce(Option<&[u8]>) -> Result<State<T>> + Send + 'static,
        R: FnOnce() -> Result<State<T>> + Send + 'static,
    {
        // Own even the wait for the publication lock. Once a native lifecycle
        // worker submits an intent, cancellation of its original RPC cannot
        // drop it between lock admission, Cozo and SHM publication.
        let engine = self.clone();
        tokio::spawn(async move {
            let guard = engine.state_publication.clone().lock_owned().await;
            let runtime = tokio::runtime::Handle::current();
            tokio::task::spawn_blocking(move || -> Result<_> {
                let _guard = guard;
                let document = crate::sealed_schema_reader::read_manifest_pinned_schema_document(
                    &catalog,
                    "identity_sled",
                )?;
                crate::method_contract_admission::admit_compiled_state_contract(
                    &document,
                    "identity_sled",
                )?;
                document.schema.validate_public_state_policy()?;
                let mut full_state = None;
                let mut public_state = None;
                let mut public_previous = None;
                let mut derive_pair = |state: Option<OwnedValue>| -> Result<_> {
                    let now = crate::sealed_schema_reader::read_manifest_pinned_schema_document(
                        &catalog,
                        "identity_sled",
                    )?;
                    anyhow::ensure!(
                        now.schema_hash == document.schema_hash
                            && now.blob.schema_json == document.blob.schema_json,
                        "identity seal changed during durable publication; recovery is required"
                    );
                    let pair = if let Some(state) = &state {
                        let metadata = op_plugins::state_plugins::identity_sled::identity_metadata_projection(state)?;
                        // The local metadata file retains existing scalar
                        // identity readers. Wire disclosure is separately
                        // gated by the exact sealed policy over these same
                        // committed bytes, never the credential-bearing tree.
                        public_state = document.schema.project_public_state(&metadata)?;
                        Some((simd_json::to_vec(&metadata)?, simd_json::to_vec(state)?))
                    } else {
                        public_state = None;
                        None
                    };
                    full_state = state;
                    Ok(pair)
                };
                let publication = if let Some(commit) = commit {
                    op_core::projection_shm::commit_identity_projection_durable_in(
                        &state_dir,
                        gid,
                        &operation_id,
                        |previous| {
                            if let Some(previous) = previous {
                                let mut bytes = previous.to_vec();
                                let previous = simd_json::to_owned_value(&mut bytes)?;
                                let metadata = op_plugins::state_plugins::identity_sled::identity_metadata_projection(&previous)?;
                                public_previous = document.schema.project_public_state(&metadata)?;
                            }
                            let (state, durable) = commit(previous)?;
                            Ok((derive_pair(state)?, durable))
                        },
                    )?
                } else {
                    let recover = recover.context("identity publication has no operation")?;
                    op_core::projection_shm::recover_identity_projection_in(
                        &state_dir,
                        gid,
                        &operation_id,
                        || {
                            let (state, durable) = recover()?;
                            Ok((derive_pair(state)?, durable))
                        },
                    )?
                };
                if let CredentialPublication::Committed { projection, .. } = &publication {
                    // These values were validated before the core commit.
                    // No fallible reread can relabel a committed publication;
                    // another process's later generation is not this receipt.
                    engine.finish_identity_publication(
                        projection,
                        full_state,
                        public_previous,
                        public_state,
                        &runtime,
                    );
                }
                Ok(publication)
            })
            .await
            .context("identity publication worker failed")?
        })
        .await
        .context("identity publication owner failed")?
    }

    fn finish_identity_publication(
        &self,
        _receipt: &ProjectionCommit,
        full_state: Option<OwnedValue>,
        previous: Option<OwnedValue>,
        current: Option<OwnedValue>,
        runtime: &tokio::runtime::Handle,
    ) {
        let removed = full_state.is_none();
        {
            let mut cache = self.state_cache.blocking_write();
            if let Some(state) = full_state {
                cache.insert("identity_sled".into(), state);
            } else {
                cache.remove("identity_sled");
            }
        }
        // Recovery can remove a pair whose predecessor is intentionally not
        // readable; private-only edits can change its generation too. Always
        // wake committed SHM readers, which deduplicate their projected view.
        {
            let _ = self.change_tx.send(StateChange {
                change_id: uuid::Uuid::new_v4().to_string(),
                event_id: 0,
                plugin_id: "identity_sled".into(),
                object_path: op_plugins::canonical::plugin_path("identity_sled"),
                change_type: if removed {
                    ChangeType::ObjectRemoved
                } else {
                    ChangeType::PropertySet
                },
                member_name: None,
                old_value: previous,
                new_value: current.unwrap_or(simd_json::json!(null)),
                tags_touched: Vec::new(),
                event_hash: String::new(),
                timestamp: chrono::Utc::now(),
                actor_id: "identity_sled_dispatch".into(),
                source: ChangeSource::Internal,
            });
            let bus = self.signal_bus.clone();
            runtime.spawn(async move {
                Self::emit_updated_signal_with_bus(bus, "identity_sled".into(), None, None).await;
            });
        }
    }
}

#[cfg(test)]
#[path = "identity_publication_tests.rs"]
mod tests;
