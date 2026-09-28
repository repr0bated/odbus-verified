//! MutationEngine-owned private episode snapshots. Cozo owns durable records;
//! only committed SHM supplies state frames, and wakes contain owner keys only.
use std::{collections::BTreeMap, path::PathBuf, sync::Arc};

use anyhow::{Context, Result};
use op_core::projection_shm::{
    PrivateDocumentCommit, PrivateDocumentPublication, PrivateProjectionDirectory,
};
use op_cozo_store::{EpisodeScope, EpisodeSnapshot};
use op_state_store::PluginSchema;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::sync::{Mutex, broadcast};

use crate::{
    method_contract_admission,
    mutation_engine::MutationEngine,
    sealed_schema_reader::{ManifestPinnedSchemaDocument, read_manifest_pinned_schema_document},
};

pub(crate) const PLUGIN: &str = "ctl_plane_chatbot";
pub(crate) const SNAPSHOT_LIMIT: usize = 32;
pub(crate) const READ_CAPABILITY: &str = "chatbot.read";

// The generic Cozo plugin deliberately exposes native queries and archives.
// Its database must never contain the private episode owner's relations.
pub(crate) fn require_distinct_database_paths(episode: &str) -> Result<()> {
    let cognitive = std::env::var("COGNITIVE_MCP_DB_PATH")
        .unwrap_or_else(|_| "/var/lib/op-dbus/cognitive-memory.db".into());
    distinct_databases(
        std::path::Path::new(episode),
        std::path::Path::new(&cognitive),
    )
}

fn resolved_database_path(path: &std::path::Path) -> Result<PathBuf> {
    let mut normalized = PathBuf::new();
    for component in std::path::absolute(path)?.components() {
        match component {
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            std::path::Component::CurDir => {}
            component => normalized.push(component.as_os_str()),
        }
    }
    let mut ancestor = normalized.as_path();
    let mut absent = Vec::new();
    while !ancestor.try_exists()? {
        absent.push(
            ancestor
                .file_name()
                .context("database has no existing ancestor")?
                .to_owned(),
        );
        ancestor = ancestor.parent().context("database has no parent")?;
    }
    let mut resolved = ancestor.canonicalize()?;
    for component in absent.into_iter().rev() {
        resolved.push(component);
    }
    Ok(resolved)
}

fn distinct_databases(episode: &std::path::Path, cognitive: &std::path::Path) -> Result<()> {
    let episode = resolved_database_path(episode)?;
    let cognitive = resolved_database_path(cognitive)?;
    anyhow::ensure!(
        !episode.starts_with(&cognitive) && !cognitive.starts_with(&episode),
        "private episode database must be separate from the generic cognitive database"
    );
    if episode.exists() && cognitive.exists() {
        use std::os::unix::fs::MetadataExt;
        let left = episode.metadata()?;
        let right = cognitive.metadata()?;
        anyhow::ensure!(
            left.dev() != right.dev() || left.ino() != right.ino(),
            "private episode database aliases the generic cognitive database"
        );
    }
    Ok(())
}

pub(crate) struct EpisodePublisher {
    root: PrivateProjectionDirectory,
    catalog: PathBuf,
    operation: Arc<Mutex<()>>,
    wakes: broadcast::Sender<String>,
}

/// Admission retained by the owned dispatch across a native effect. Revoking
/// its caller later can suppress the response, but cannot erase the completed
/// effect's durable receipt. This permit is never accepted from a transport.
pub(crate) struct EpisodeEffect {
    scope: EpisodeScope,
    pin: ManifestPinnedSchemaDocument,
    operation: tokio::sync::OwnedMutexGuard<()>,
}

impl EpisodeEffect {
    pub(crate) fn require_scope(&self, scope: &EpisodeScope) -> Result<()> {
        anyhow::ensure!(
            &self.scope == scope,
            "episode receipt differs from the admitted effect scope"
        );
        Ok(())
    }
}

fn owner_key(scope: &EpisodeScope) -> String {
    let bytes = serde_json::to_vec(&[
        &scope.principal_id,
        &scope.session_id,
        &scope.session_genesis,
    ])
    .expect("identity tuple serializes");
    hex::encode(Sha256::digest(bytes))
}

fn state(snapshot: &EpisodeSnapshot, scope: &EpisodeScope, schema: &PluginSchema) -> Result<Value> {
    anyhow::ensure!(
        snapshot.episode_count >= snapshot.episodes.len() as u64
            && snapshot.truncated == (snapshot.episode_count > snapshot.episodes.len() as u64),
        "episode snapshot count disagrees with its durable rows"
    );
    let mut episodes = BTreeMap::new();
    for row in &snapshot.episodes {
        anyhow::ensure!(
            &row.scope == scope,
            "episode snapshot crosses identity scope"
        );
        let episode: op_plugins::state_plugins::ctl_plane_chatbot::ReasoningEpisode =
            serde_json::from_value(row.episode.clone()).context("invalid durable episode")?;
        op_plugins::state_plugins::ctl_plane_chatbot_methods::validate_episode(&episode)?;
        anyhow::ensure!(
            episode.principal_id == scope.principal_id
                && episode.session_id == scope.session_id
                && episode.session_genesis == scope.session_genesis
                && episode.on_behalf_of == scope.principal_id
                && episode.episode_id == row.episode_id
                && episode.content_hash == row.content_hash,
            "episode document differs from durable identity key"
        );
        episodes.insert(row.episode_id.clone(), row.episode.clone());
    }
    let mut knowledge = BTreeMap::new();
    for row in &snapshot.knowledge {
        anyhow::ensure!(
            &row.scope == scope,
            "knowledge snapshot crosses identity scope"
        );
        let push: op_plugins::state_plugins::ctl_plane_chatbot::KnowledgePush =
            serde_json::from_value(row.push.clone()).context("invalid durable knowledge push")?;
        op_plugins::state_plugins::ctl_plane_chatbot_methods::validate_knowledge_push(&push)?;
        anyhow::ensure!(
            push.principal_id == scope.principal_id
                && push.session_id == scope.session_id
                && push.session_genesis == scope.session_genesis
                && push.push_id == row.push_id
                && push.content_hash == row.content_hash,
            "knowledge document differs from durable identity key"
        );
        knowledge.insert(row.push_id.clone(), row.push.clone());
    }
    anyhow::ensure!(
        snapshot.knowledge_count >= snapshot.knowledge.len() as u64
            && snapshot.knowledge_truncated
                == (snapshot.knowledge_count > snapshot.knowledge.len() as u64),
        "knowledge snapshot count disagrees with its durable rows"
    );
    let value = json!({"episodes":episodes, "episode_count":snapshot.episode_count,
        "episode_revision":snapshot.revision, "episodes_truncated":snapshot.truncated,
        "knowledge":knowledge, "knowledge_count":snapshot.knowledge_count,
        "knowledge_truncated":snapshot.knowledge_truncated});
    anyhow::ensure!(
        serde_json::to_vec(&value)?.len() <= op_cozo_store::MAX_CHATBOT_SNAPSHOT_BYTES,
        "private episode snapshot exceeds its committed byte bound"
    );
    validate_state(&value, schema)?;
    Ok(value)
}

fn validate_state(value: &Value, schema: &PluginSchema) -> Result<()> {
    let owned = simd_json::serde::to_owned_value(value.clone())?;
    let checked = schema.validate(&owned);
    anyhow::ensure!(
        checked.valid && checked.warnings.is_empty(),
        "private episode snapshot differs from sealed state contract"
    );
    schema
        .validate_exact_schema_fields(&owned)
        .map_err(|_| anyhow::anyhow!("invalid exact episode state fields"))?;
    Ok(())
}

fn validate_scope(value: &Value, scope: &EpisodeScope) -> Result<()> {
    let episodes = value["episodes"]
        .as_object()
        .context("private episode map is absent")?;
    for (id, document) in episodes {
        let episode: op_plugins::state_plugins::ctl_plane_chatbot::ReasoningEpisode =
            serde_json::from_value(document.clone())?;
        op_plugins::state_plugins::ctl_plane_chatbot_methods::validate_episode(&episode)?;
        anyhow::ensure!(
            episode.episode_id == *id
                && episode.principal_id == scope.principal_id
                && episode.on_behalf_of == scope.principal_id
                && episode.session_id == scope.session_id
                && episode.session_genesis == scope.session_genesis,
            "committed episode document crosses identity scope"
        );
    }
    let knowledge = value["knowledge"]
        .as_object()
        .context("private knowledge map is absent")?;
    for (id, document) in knowledge {
        let push: op_plugins::state_plugins::ctl_plane_chatbot::KnowledgePush =
            serde_json::from_value(document.clone())?;
        op_plugins::state_plugins::ctl_plane_chatbot_methods::validate_knowledge_push(&push)?;
        anyhow::ensure!(
            push.push_id == *id
                && push.principal_id == scope.principal_id
                && push.session_id == scope.session_id
                && push.session_genesis == scope.session_genesis,
            "committed knowledge document crosses identity scope"
        );
    }
    Ok(())
}

fn compare_revision(previous: Option<&[u8]>, bytes: &[u8], revision: u64) -> Result<()> {
    if let Some(previous) = previous {
        let value: Value = serde_json::from_slice(previous)?;
        let before = value["episode_revision"]
            .as_u64()
            .context("missing committed episode revision")?;
        anyhow::ensure!(
            revision >= before && (revision != before || bytes == previous),
            "episode projection revision regressed or changed without a durable revision"
        );
    }
    Ok(())
}

impl EpisodePublisher {
    pub(crate) fn new(root: PrivateProjectionDirectory, catalog: PathBuf) -> Self {
        let (wakes, _) = broadcast::channel(64);
        Self {
            root,
            catalog,
            operation: Arc::new(Mutex::new(())),
            wakes,
        }
    }

    fn pin(&self) -> Result<ManifestPinnedSchemaDocument> {
        let pinned = read_manifest_pinned_schema_document(&self.catalog, PLUGIN)?;
        method_contract_admission::admit_compiled_state_contract(&pinned, PLUGIN)?;
        Ok(pinned)
    }

    fn recheck(&self, expected: &ManifestPinnedSchemaDocument) -> Result<()> {
        let current = self.pin()?;
        anyhow::ensure!(
            current.schema_hash == expected.schema_hash
                && current.blob.schema_json == expected.blob.schema_json,
            "episode plugin seal changed during operation"
        );
        Ok(())
    }

    pub(crate) async fn preflight(
        self: &Arc<Self>,
        engine: &MutationEngine,
        scope: &EpisodeScope,
    ) -> Result<()> {
        crate::project_dispatch::verified_session(
            engine,
            &scope.principal_id,
            Some(&scope.session_id),
            Some(&scope.session_genesis),
        )
        .await?;
        let publisher = self.clone();
        let owner = owner_key(scope);
        tokio::task::spawn_blocking(move || -> Result<()> {
            publisher.pin()?;
            let directory = publisher.root.prepare_child(&owner)?;
            directory.ensure_distinct_from(std::path::Path::new(
                &op_core::projection_shm::shm_state_dir(),
            ))?;
            anyhow::ensure!(
                directory.pending_document()?.is_none(),
                "episode publication requires recovery through list_episodes"
            );
            Ok(())
        })
        .await??;
        Ok(())
    }

    #[cfg(test)]
    pub(crate) async fn commit(
        self: &Arc<Self>,
        engine: &MutationEngine,
        scope: EpisodeScope,
        durable: impl FnOnce() -> Result<EpisodeSnapshot> + Send + 'static,
    ) -> Result<EpisodeSnapshot> {
        let effect = self.admit_effect(engine, scope).await?;
        self.commit_effect(effect, durable).await
    }

    pub(crate) async fn admit_effect(
        self: &Arc<Self>,
        engine: &MutationEngine,
        scope: EpisodeScope,
    ) -> Result<EpisodeEffect> {
        self.preflight(engine, &scope).await?;
        let operation = self.operation.clone().lock_owned().await;
        crate::project_dispatch::verified_session(
            engine,
            &scope.principal_id,
            Some(&scope.session_id),
            Some(&scope.session_genesis),
        )
        .await?;
        self.preflight(engine, &scope).await?;
        let publisher = self.clone();
        let pin = tokio::task::spawn_blocking(move || publisher.pin()).await??;
        Ok(EpisodeEffect {
            scope,
            pin,
            operation,
        })
    }

    pub(crate) async fn commit_effect(
        self: &Arc<Self>,
        effect: EpisodeEffect,
        durable: impl FnOnce() -> Result<EpisodeSnapshot> + Send + 'static,
    ) -> Result<EpisodeSnapshot> {
        let EpisodeEffect {
            scope,
            pin,
            operation,
        } = effect;
        let publisher = self.clone();
        // Keep the lock, durable operation, and wake owned despite cancellation.
        tokio::spawn(async move {
            let _operation = operation;
            let owner = owner_key(&scope);
            let wake_owner = owner.clone();
            let worker = publisher.clone();
            let publication = tokio::task::spawn_blocking(move || -> Result<_> {
                let directory = worker.root.prepare_child(&owner)?;
                directory.commit_document_durable_with_decision(PLUGIN, &uuid::Uuid::new_v4().to_string(), |previous| {
                    let receipt = durable()?;
                    let prepared = (|| -> Result<Vec<u8>> {
                        let bytes = serde_json::to_vec(&state(&receipt, &scope, &pin.schema)?)?;
                        compare_revision(previous, &bytes, receipt.revision)?;
                        worker.recheck(&pin)?;
                        Ok(bytes)
                    })();
                    Ok(match prepared {
                        Ok(bytes) => PrivateDocumentCommit::Publish {current:Some(bytes), durable:receipt},
                        Err(error) => PrivateDocumentCommit::RetainPending {durable:receipt, error},
                    })
                })
            }).await??;
            match publication {
                PrivateDocumentPublication::Committed {durable, ..} => {
                    let _ = publisher.wakes.send(wake_owner);
                    Ok(durable)
                }
                PrivateDocumentPublication::Pending {..} => anyhow::bail!("episode committed state is unavailable; list_episodes must recover before retry"),
            }
        }).await.context("owned episode publication task failed")?
    }

    /// Explicit authenticated list/get admission can restore the private view
    /// from its sole durable owner. StateSync never performs this recovery.
    pub(crate) async fn recover(
        self: &Arc<Self>,
        engine: &MutationEngine,
        scope: EpisodeScope,
    ) -> Result<()> {
        crate::project_dispatch::verified_session(
            engine,
            &scope.principal_id,
            Some(&scope.session_id),
            Some(&scope.session_genesis),
        )
        .await?;
        let store = engine.ctl_plane_chatbot_episode_cozo().await?;
        let publisher = self.clone();
        let operation = self.operation.clone().lock_owned().await;
        crate::project_dispatch::verified_session(
            engine,
            &scope.principal_id,
            Some(&scope.session_id),
            Some(&scope.session_genesis),
        )
        .await?;
        tokio::spawn(async move {
            let _operation = operation;
            let owner = owner_key(&scope);
            let wake_owner = owner.clone();
            let worker = publisher.clone();
            let publication = tokio::task::spawn_blocking(move || -> Result<_> {
                let pin = worker.pin()?;
                let directory = worker.root.prepare_child(&owner)?;
                let recover = |previous: Option<&[u8]>| -> Result<PrivateDocumentCommit<()>> {
                    let receipt = store.snapshot_scoped_episodes(&scope, SNAPSHOT_LIMIT)?;
                    let bytes = serde_json::to_vec(&state(&receipt, &scope, &pin.schema)?)?;
                    compare_revision(previous, &bytes, receipt.revision)?;
                    worker.recheck(&pin)?;
                    Ok(PrivateDocumentCommit::Publish {
                        current: Some(bytes),
                        durable: (),
                    })
                };
                if let Some(pending) = directory.pending_document()? {
                    anyhow::ensure!(
                        pending.plugin_id == PLUGIN,
                        "pending episode owner mismatch"
                    );
                    directory.recover_document_with_decision(PLUGIN, &pending.operation_id, || {
                        recover(None)
                    })
                } else {
                    directory.commit_document_durable_with_decision(
                        PLUGIN,
                        &uuid::Uuid::new_v4().to_string(),
                        recover,
                    )
                }
            })
            .await??;
            match publication {
                PrivateDocumentPublication::Committed { .. } => {
                    let _ = publisher.wakes.send(wake_owner);
                    Ok(())
                }
                PrivateDocumentPublication::Pending { .. } => {
                    anyhow::bail!("episode projection recovery is pending")
                }
            }
        })
        .await
        .context("owned episode recovery task failed")?
    }

    /// Commit one knowledge push for `scope` through the owned effect path:
    /// durable row first, then the private SHM document, then the wake.
    /// The push document is the engine's push without identity fields; the
    /// store assigns `push_id` and the plugin's digest seals it.
    pub(crate) async fn commit_knowledge(
        self: &Arc<Self>,
        engine: &MutationEngine,
        scope: EpisodeScope,
        push: serde_json::Value,
    ) -> Result<EpisodeSnapshot> {
        let store = engine.ctl_plane_chatbot_episode_cozo().await?;
        let effect = self.admit_effect(engine, scope.clone()).await?;
        let announce_scope = scope.clone();
        let snapshot = self
            .commit_effect(effect, move || {
                let (_, snapshot) = store.insert_scoped_knowledge_and_snapshot(
                &scope,
                &push,
                &|document| {
                    let typed: op_plugins::state_plugins::ctl_plane_chatbot::KnowledgePush =
                        serde_json::from_value(document.clone()).map_err(|error| {
                            op_cozo_store::CozoError::Other(format!(
                                "knowledge push is not the typed contract: {error}"
                            ))
                        })?;
                    op_plugins::state_plugins::ctl_plane_chatbot_methods::knowledge_content_hash(
                        &typed,
                    )
                    .map_err(|error| op_cozo_store::CozoError::Other(error.to_string()))
                },
                SNAPSHOT_LIMIT,
            )?;
                Ok(snapshot)
            })
            .await?;
        // Newest first: the row this commit produced.
        if let Some(row) = snapshot.knowledge.first() {
            engine
                .announce_chatbot_write("knowledge_committed", &announce_scope, &row.push)
                .await;
        }
        Ok(snapshot)
    }

    pub(crate) fn subscribe(&self) -> broadcast::Receiver<String> {
        self.wakes.subscribe()
    }
    pub(crate) fn key(scope: &EpisodeScope) -> String {
        owner_key(scope)
    }

    pub(crate) async fn read(
        self: &Arc<Self>,
        scope: EpisodeScope,
    ) -> Result<crate::state_stream::StateSnapshot> {
        let publisher = self.clone();
        tokio::task::spawn_blocking(move || -> Result<_> {
            let pin = publisher.pin()?;
            let directory = publisher
                .root
                .open_child(&owner_key(&scope))?
                .context("episode state requires list_episodes activation")?;
            let snapshot = directory.read_snapshot(&[PLUGIN.into()])?;
            let bytes = snapshot
                .states
                .get(PLUGIN)
                .context("episode state requires list_episodes activation")?;
            let value: Value = serde_json::from_slice(bytes)?;
            validate_state(&value, &pin.schema)?;
            validate_scope(&value, &scope)?;
            publisher.recheck(&pin)?;
            Ok(crate::state_stream::StateSnapshot {
                generation: snapshot.generation,
                catalog_hash: pin.schema_hash.clone(),
                plugins: BTreeMap::from([(
                    PLUGIN.into(),
                    crate::state_stream::PluginSnapshot {
                        schema_hash: pin.schema_hash,
                        schema: serde_json::to_value(pin.schema)?,
                        state: Some(value),
                    },
                )]),
            })
        })
        .await?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        human_principal_dispatch::tests::{
            pk, register, temp_cozo, temp_shm_with_schemas, test_engine,
        },
        identity_sled_dispatch::{IdentitySledStore, tests::write_identity},
    };
    use op_plugins::state_plugins::{
        ctl_plane_chatbot::CtlPlaneChatbotPlugin,
        ctl_plane_chatbot_methods::{CompletedChatEpisode, build_completed_chat_episode},
        human_principal::HumanPrincipalPlugin,
        identity_sled::IdentitySledPlugin,
    };
    use op_state::StatePlugin;

    #[test]
    fn private_database_cannot_alias_or_nest_in_the_query_database() {
        let root = tempfile::tempdir().unwrap();
        let generic = root.path().join("generic");
        std::fs::create_dir(&generic).unwrap();
        let alias = root.path().join("alias");
        std::os::unix::fs::symlink(&generic, &alias).unwrap();
        assert!(distinct_databases(&generic, &alias).is_err());
        assert!(distinct_databases(&generic.join("private"), &generic).is_err());
        assert!(distinct_databases(&generic.join("../generic"), &generic).is_err());
        assert!(distinct_databases(&root.path().join("private"), &generic).is_ok());
    }

    #[tokio::test]
    async fn durable_episode_publication_is_private_and_recovers_after_seal_change() {
        let _cozo = temp_cozo();
        let schema = CtlPlaneChatbotPlugin::new().schema().unwrap();
        let _shm = temp_shm_with_schemas(&[
            schema.clone(),
            HumanPrincipalPlugin::new().schema().unwrap(),
            IdentitySledPlugin::new().schema().unwrap(),
        ]);
        let root = tempfile::tempdir().unwrap();
        let private = PrivateProjectionDirectory::prepare(&root.path().join("private")).unwrap();
        let engine = test_engine();
        engine
            .install_identity_sled_store_for_test(IdentitySledStore::new(
                op_cozo_store::CozoGraphShuttle::new_in_memory().unwrap(),
            ))
            .unwrap();
        let store = Arc::new(op_cozo_store::CozoGraphShuttle::new_in_memory().unwrap());
        engine
            .configure_chatbot_episode_store(store.clone())
            .unwrap();
        let catalog = crate::sealed_schema_reader::blob_catalog_dir_from_env();
        engine
            .configure_chatbot_episode_projection(private, catalog.clone())
            .unwrap();
        let key = pk(0xE9);
        register(&key, "episode-state-fixture").await.unwrap();
        let sled = write_identity(&engine, &key).await.unwrap();
        let scope = EpisodeScope {
            principal_id: op_identity::session::derive_principal_id(&key),
            session_id: sled.session_id,
            session_genesis: sled.genesis.unwrap(),
        };
        let make_episode = |answer: &str| {
            build_completed_chat_episode(CompletedChatEpisode {
                scope: scope.clone(),
                conversation_id: Some("private-conversation".into()),
                goal_text: Some("private goal".into()),
                decision_output: answer.into(),
                tools: vec![],
                started_at: chrono::Utc::now(),
                ended_at: chrono::Utc::now(),
            })
            .unwrap()
        };
        let publisher = engine.chatbot_episode_publisher().unwrap().clone();
        let mut wakes = publisher.subscribe();
        let first = make_episode("private first answer");
        engine.commit_chatbot_episode(first.clone()).await.unwrap();
        assert_eq!(wakes.recv().await.unwrap(), EpisodePublisher::key(&scope));
        let observed = publisher.read(scope.clone()).await.unwrap();
        assert_eq!(
            observed.plugins[PLUGIN].state.as_ref().unwrap()["episodes"][&first.episode_id],
            first.episode
        );
        assert!(engine.get_state(PLUGIN).await.is_none());
        assert!(op_core::projection_shm::read_projection_bytes(PLUGIN).is_none());
        let mut other = scope.clone();
        other.session_genesis.push_str("-other");
        assert!(publisher.read(other).await.is_err());

        let second = make_episode("private second answer");
        let committed_store = store.clone();
        let changed_catalog = catalog.clone();
        let mut changed_schema = schema.clone();
        changed_schema
            .description
            .push_str(" changed during durable commit");
        let second_id = second.episode_id.clone();
        let result = publisher
            .commit(&engine, scope.clone(), move || {
                let result =
                    committed_store.insert_scoped_episode_and_snapshot(&second, SNAPSHOT_LIMIT)?;
                op_blob::BlobStore::open(changed_catalog)?
                    .write(&op_blob::blobify_plugin_schema(PLUGIN, changed_schema))?;
                Ok(result)
            })
            .await;
        assert!(result.is_err());
        assert!(matches!(
            wakes.try_recv(),
            Err(broadcast::error::TryRecvError::Empty)
        ));
        assert!(publisher.read(scope.clone()).await.is_err());
        assert!(
            store
                .get_scoped_episode(&scope, &second_id)
                .unwrap()
                .is_some(),
            "post-commit seal failure must not erase the durable episode"
        );
        op_blob::BlobStore::open(catalog)
            .unwrap()
            .write(&op_blob::blobify_plugin_schema(PLUGIN, schema))
            .unwrap();
        publisher.recover(&engine, scope.clone()).await.unwrap();
        assert_eq!(wakes.recv().await.unwrap(), EpisodePublisher::key(&scope));
        let recovered = publisher.read(scope.clone()).await.unwrap();
        assert_eq!(
            recovered.plugins[PLUGIN].state.as_ref().unwrap()["episode_count"],
            2
        );
        assert!(
            recovered.plugins[PLUGIN].state.as_ref().unwrap()["episodes"]
                .get(&second_id)
                .is_some()
        );

        // Once the durable owner starts, dropping the transport cannot leave
        // a completed write without its SHM publication and metadata wake.
        let third = make_episode("answer after transport cancellation");
        let third_id = third.episode_id.clone();
        let durable_store = store.clone();
        let owned_publisher = publisher.clone();
        let owned_engine = engine.clone();
        let owned_scope = scope.clone();
        let (started, started_rx) = tokio::sync::oneshot::channel();
        let (release, release_rx) = std::sync::mpsc::channel();
        let transport = tokio::spawn(async move {
            owned_publisher
                .commit(&owned_engine, owned_scope, move || {
                    let _ = started.send(());
                    release_rx.recv()?;
                    Ok(durable_store.insert_scoped_episode_and_snapshot(&third, SNAPSHOT_LIMIT)?)
                })
                .await
        });
        started_rx.await.unwrap();
        transport.abort();
        release.send(()).unwrap();
        assert_eq!(
            tokio::time::timeout(std::time::Duration::from_secs(5), wakes.recv())
                .await
                .unwrap()
                .unwrap(),
            EpisodePublisher::key(&scope)
        );
        let completed = publisher.read(scope.clone()).await.unwrap();
        assert_eq!(
            completed.plugins[PLUGIN].state.as_ref().unwrap()["episode_count"],
            3
        );
        assert!(
            completed.plugins[PLUGIN].state.as_ref().unwrap()["episodes"]
                .get(&third_id)
                .is_some()
        );
        assert!(
            store
                .get_scoped_episode(&scope, &third_id)
                .unwrap()
                .is_some()
        );

        // A knowledge push is the same owned effect: durable row, private
        // document child, owner-key wake — and the episode window is intact.
        let push = op_cognitive_mcp::context_awareness::KnowledgePush {
            id: "push-1".into(),
            session_id: scope.session_id.clone(),
            trigger: op_cognitive_mcp::context_awareness::PushTrigger::ErrorAssistance,
            trigger_reason: "repeated build error".into(),
            relevance_score: 0.9,
            timestamp: chrono::Utc::now(),
            content: op_cognitive_mcp::context_awareness::KnowledgeContent::ToolGuidance {
                tool_name: "rust_pro".into(),
                usage_tips: vec!["check first".into()],
                example_usage: serde_json::Value::Null,
            },
            suggested_action: Some("run rust_pro check".into()),
            proactive: true,
        };
        let resolved = crate::knowledge_delivery::scope_for_session(&engine, &scope.session_id)
            .await
            .unwrap()
            .expect("anchored sled resolves to its scope");
        assert_eq!(resolved, scope);
        let snapshot = publisher
            .commit_knowledge(
                &engine,
                scope.clone(),
                crate::knowledge_delivery::push_document(&push),
            )
            .await
            .unwrap();
        assert_eq!(snapshot.knowledge_count, 1);
        assert_eq!(wakes.recv().await.unwrap(), EpisodePublisher::key(&scope));
        let with_knowledge = publisher.read(scope.clone()).await.unwrap();
        let state = with_knowledge.plugins[PLUGIN].state.as_ref().unwrap();
        assert_eq!(state["episode_count"], 3);
        assert_eq!(state["knowledge_count"], 1);
        assert_eq!(state["knowledge_truncated"], false);
        let (push_id, document) = state["knowledge"]
            .as_object()
            .unwrap()
            .iter()
            .next()
            .unwrap();
        assert_eq!(push_id, &format!("{:020}", snapshot.revision));
        assert_eq!(document["source_id"], "push-1");
        assert_eq!(document["principal_id"], scope.principal_id);
        assert_eq!(document["suggested_action"], "run rust_pro check");
        assert_eq!(
            document["content_hash"].as_str().map(str::len),
            Some(64),
            "sealed by the plugin digest"
        );
        // The same engine push again is a duplicate, not a second row.
        assert!(
            publisher
                .commit_knowledge(
                    &engine,
                    scope.clone(),
                    crate::knowledge_delivery::push_document(&push),
                )
                .await
                .is_err()
        );
        assert!(
            crate::knowledge_delivery::scope_for_session(&engine, "no-such-session")
                .await
                .unwrap()
                .is_none(),
            "no sled, no delivery"
        );

        // Every durable chatbot write is announced into the chain under the
        // principal, through its declared signal, so it is vectorized (OD-64).
        let chain = engine.event_chain.read().await;
        let announced: Vec<_> = chain
            .events()
            .iter()
            .filter(|e| e.plugin_id == PLUGIN)
            .collect();
        let episodes = announced
            .iter()
            .filter(|e| e.method_name.as_deref() == Some("episode_committed"))
            .count();
        let knowledge: Vec<_> = announced
            .iter()
            .filter(|e| e.method_name.as_deref() == Some("knowledge_committed"))
            .collect();
        // Only the engine path announces; the test-only publisher.commit bypasses it.
        assert_eq!(episodes, 1, "chat episodes announced: {episodes}");
        assert_eq!(knowledge.len(), 1);
        let event = knowledge[0];
        assert_eq!(event.actor_id, scope.principal_id);
        assert_eq!(event.session_id.as_deref(), Some(scope.session_id.as_str()));
        assert!(
            event.sealed_id_digest.is_some(),
            "identity digest on the announcement"
        );
        assert_eq!(
            event.input_payload.as_ref().unwrap()["source_id"],
            "push-1",
            "the committed document is the payload"
        );
    }
}
