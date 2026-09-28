//! MutationEngine-owned private `chat_response` snapshots, one per human
//! principal. Cozo owns the durable rows; the private SHM document is the
//! committed state a reader streams from; wakes carry an owner key only.
//!
//! The owner key is `sha256(principal_id)`: a conversation belongs to the
//! principal and spans logins, so its view is not keyed by session.
use std::{collections::BTreeMap, path::PathBuf, sync::Arc};

use anyhow::{Context, Result};
use op_core::projection_shm::{
    PrivateDocumentCommit, PrivateDocumentPublication, PrivateProjectionDirectory,
};
use op_cozo_store::{ChatResponseSnapshot, CozoGraphShuttle};
use op_plugins::state_plugins::{
    chat_response::{ChatTurn, ConversationSummary},
    chat_response_methods::{validate_conversation, validate_turn},
};
use op_state_store::PluginSchema;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::sync::{Mutex, broadcast};

use crate::{
    method_contract_admission,
    sealed_schema_reader::{ManifestPinnedSchemaDocument, read_manifest_pinned_schema_document},
};

pub(crate) const PLUGIN: &str = op_plugins::state_plugins::chat_response::PLUGIN_ID;
/// Conversations kept in the private SHM snapshot (most recently active).
pub(crate) const CONVERSATION_SNAPSHOT_LIMIT: usize = 32;
/// Turns kept in the private SHM snapshot (newest commits, all conversations).
pub(crate) const TURN_SNAPSHOT_LIMIT: usize = 32;
/// Stay inside the exact-schema validator's document bounds (16 MiB, 262,144
/// nodes) with room for the envelope; older turns are dropped first.
const MAX_SNAPSHOT_BYTES: usize = 15 * 1024 * 1024;
const MAX_SNAPSHOT_NODES: usize = 200_000;

/// Private projection owner key and in-process wake key of a principal.
pub(crate) fn owner_key(principal_id: &str) -> String {
    hex::encode(Sha256::digest(principal_id.as_bytes()))
}

/// A content-free doorbell ring: the session-scoped doorbell owner key of
/// the asking session and the scope revision, nothing else.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub(crate) struct DoorbellWake {
    pub owner_key: String,
    pub seq: u64,
}

pub(crate) type Doorbell = Arc<dyn Fn(DoorbellWake) + Send + Sync>;

/// Production doorbell: `{"seq": revision}` on the asking session's
/// `opdbus/wake/<owner-key>` topic through the `emqx` plugin, best effort
/// and off the commit path. The committed SHM document is the truth; a
/// broker outage only delays the UI.
pub(crate) fn emqx_doorbell() -> Doorbell {
    Arc::new(|wake: DoorbellWake| {
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return;
        };
        runtime.spawn(async move {
            let args = json!({"owner_key": wake.owner_key, "seq": wake.seq});
            if let Err(error) =
                op_plugins::state_plugins::emqx::dispatch_emqx_method("publish_wake", &args).await
            {
                tracing::debug!(%error, "chat_response doorbell wake not published");
            }
        });
    })
}

pub(crate) struct ChatResponsePublisher {
    root: PrivateProjectionDirectory,
    catalog: PathBuf,
    operation: Arc<Mutex<()>>,
    wakes: broadcast::Sender<String>,
    doorbell: std::sync::OnceLock<Doorbell>,
}

/// The owner's publication lock plus the seal pinned under it. Held from
/// admission through the durable commit and its SHM publication.
pub(crate) struct ChatEffect {
    principal_id: String,
    pin: ManifestPinnedSchemaDocument,
    operation: tokio::sync::OwnedMutexGuard<()>,
}

fn json_nodes(value: &Value) -> usize {
    match value {
        Value::Object(object) => 1 + object.values().map(json_nodes).sum::<usize>(),
        Value::Array(items) => 1 + items.iter().map(json_nodes).sum::<usize>(),
        _ => 1,
    }
}

/// The committed private document for `snapshot`, checked against the
/// durable keys, the plugin's digests and the sealed state contract.
fn state(
    snapshot: &ChatResponseSnapshot,
    principal_id: &str,
    schema: &PluginSchema,
) -> Result<Value> {
    let mut conversations = BTreeMap::new();
    for row in &snapshot.conversations {
        anyhow::ensure!(
            row.principal_id == principal_id,
            "conversation snapshot crosses principal scope"
        );
        let summary: ConversationSummary = serde_json::from_value(row.conversation.clone())
            .context("durable conversation is not the typed contract")?;
        validate_conversation(&summary)?;
        anyhow::ensure!(
            summary.principal_id == principal_id
                && summary.conversation_id == row.conversation_id
                && summary.turn_count == row.turn_count
                && summary.head_turn_hash == row.head_turn_hash,
            "conversation document differs from its durable key"
        );
        conversations.insert(row.conversation_id.clone(), row.conversation.clone());
    }
    let mut turns = BTreeMap::new();
    let mut bytes = 0usize;
    let mut nodes = 0usize;
    for row in &snapshot.turns {
        anyhow::ensure!(
            row.principal_id == principal_id,
            "turn snapshot crosses principal scope"
        );
        let turn: ChatTurn = serde_json::from_value(row.turn.clone())
            .context("durable turn is not the typed contract")?;
        validate_turn(&turn)?;
        anyhow::ensure!(
            turn.principal_id == principal_id
                && turn.conversation_id == row.conversation_id
                && turn.turn_id == row.turn_id
                && turn.turn_index == row.turn_index
                && turn.prev_turn_hash == row.prev_turn_hash
                && turn.turn_hash == row.turn_hash
                && turn.content_hash == row.content_hash,
            "turn document differs from its durable key"
        );
        let row_bytes = serde_json::to_vec(&row.turn)?.len() + row.turn_id.len() + 4;
        let row_nodes = json_nodes(&row.turn) + 1;
        if bytes + row_bytes > MAX_SNAPSHOT_BYTES - 1024 * 1024
            || nodes + row_nodes > MAX_SNAPSHOT_NODES
        {
            break;
        }
        bytes += row_bytes;
        nodes += row_nodes;
        turns.insert(row.turn_id.clone(), row.turn.clone());
    }
    let conversations_truncated = snapshot.conversation_count > conversations.len() as u64;
    let turns_truncated = snapshot.turn_count > turns.len() as u64;
    let value = json!({
        "conversations": conversations,
        "conversation_count": snapshot.conversation_count,
        "conversations_truncated": conversations_truncated,
        "turns": turns,
        "turn_count": snapshot.turn_count,
        "turns_truncated": turns_truncated,
        "revision": snapshot.revision,
    });
    anyhow::ensure!(
        serde_json::to_vec(&value)?.len() <= MAX_SNAPSHOT_BYTES,
        "private chat_response snapshot exceeds its committed byte bound"
    );
    validate_state(&value, schema)?;
    Ok(value)
}

fn validate_state(value: &Value, schema: &PluginSchema) -> Result<()> {
    let owned = simd_json::serde::to_owned_value(value.clone())?;
    let checked = schema.validate(&owned);
    anyhow::ensure!(
        checked.valid && checked.warnings.is_empty(),
        "private chat_response snapshot differs from the sealed state contract"
    );
    schema
        .validate_exact_schema_fields(&owned)
        .map_err(|_| anyhow::anyhow!("invalid exact chat_response state fields"))?;
    Ok(())
}

// Readers of the committed private view: the stage-3 private state stream
// (3tched.com renders from SHM) is their production caller.
#[cfg_attr(not(test), allow(dead_code))]
fn validate_scope(value: &Value, principal_id: &str) -> Result<()> {
    for (key, field) in [("conversations", "conversation_id"), ("turns", "turn_id")] {
        let documents = value[key]
            .as_object()
            .with_context(|| format!("private {key} map is absent"))?;
        for (id, document) in documents {
            anyhow::ensure!(
                document["principal_id"] == principal_id && document[field] == id.as_str(),
                "committed chat_response document crosses principal scope"
            );
        }
    }
    Ok(())
}

fn compare_revision(previous: Option<&[u8]>, bytes: &[u8], revision: u64) -> Result<()> {
    if let Some(previous) = previous {
        let value: Value = serde_json::from_slice(previous)?;
        let before = value["revision"]
            .as_u64()
            .context("missing committed chat_response revision")?;
        anyhow::ensure!(
            revision >= before && (revision != before || bytes == previous),
            "chat_response projection revision regressed or changed without a durable revision"
        );
    }
    Ok(())
}

impl ChatResponsePublisher {
    pub(crate) fn new(root: PrivateProjectionDirectory, catalog: PathBuf) -> Self {
        let (wakes, _) = broadcast::channel(64);
        Self {
            root,
            catalog,
            operation: Arc::new(Mutex::new(())),
            wakes,
            doorbell: std::sync::OnceLock::new(),
        }
    }

    /// Install the content-free EMQX doorbell. Without one, commits wake
    /// in-process subscribers only.
    pub(crate) fn set_doorbell(&self, doorbell: Doorbell) -> Result<()> {
        self.doorbell
            .set(doorbell)
            .map_err(|_| anyhow::anyhow!("chat_response doorbell is already installed"))
    }

    pub(crate) fn ring(&self, wake: DoorbellWake) {
        if let Some(doorbell) = self.doorbell.get() {
            doorbell(wake);
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
            "chat_response plugin seal changed during operation"
        );
        Ok(())
    }

    /// Take the owner's publication lock and pin the seal. A publication
    /// left pending by an earlier uncertain commit is recovered here from
    /// the durable owner, under the same lock, before any new effect.
    pub(crate) async fn admit(
        self: &Arc<Self>,
        store: Arc<CozoGraphShuttle>,
        principal_id: &str,
    ) -> Result<ChatEffect> {
        let operation = self.operation.clone().lock_owned().await;
        let publisher = self.clone();
        let principal = principal_id.to_string();
        let (pin, recovered) = tokio::task::spawn_blocking(move || -> Result<_> {
            let pin = publisher.pin()?;
            let directory = publisher.root.prepare_child(&owner_key(&principal))?;
            directory.ensure_distinct_from(std::path::Path::new(
                &op_core::projection_shm::shm_state_dir(),
            ))?;
            let Some(pending) = directory.pending_document()? else {
                return Ok((pin, false));
            };
            anyhow::ensure!(
                pending.plugin_id == PLUGIN,
                "pending chat_response owner mismatch"
            );
            let publication =
                directory.recover_document_with_decision(PLUGIN, &pending.operation_id, || {
                    publisher.republish(&store, &principal, &pin, None)
                })?;
            anyhow::ensure!(
                matches!(publication, PrivateDocumentPublication::Committed { .. }),
                "chat_response projection recovery is pending"
            );
            Ok((pin, true))
        })
        .await??;
        if recovered {
            let _ = self.wakes.send(owner_key(principal_id));
        }
        Ok(ChatEffect {
            principal_id: principal_id.to_string(),
            pin,
            operation,
        })
    }

    fn republish(
        &self,
        store: &CozoGraphShuttle,
        principal_id: &str,
        pin: &ManifestPinnedSchemaDocument,
        previous: Option<&[u8]>,
    ) -> Result<PrivateDocumentCommit<()>> {
        let receipt = store.snapshot_chat_responses(
            principal_id,
            CONVERSATION_SNAPSHOT_LIMIT,
            TURN_SNAPSHOT_LIMIT,
        )?;
        let bytes = serde_json::to_vec(&state(&receipt, principal_id, &pin.schema)?)?;
        compare_revision(previous, &bytes, receipt.revision)?;
        self.recheck(pin)?;
        Ok(PrivateDocumentCommit::Publish {
            current: Some(bytes),
            durable: (),
        })
    }

    /// Run the durable write for an admitted effect, then publish the
    /// principal's private document and wake its subscribers. The lock, the
    /// durable write and the wake stay owned if the requester is cancelled.
    pub(crate) async fn commit<T: Send + 'static>(
        self: &Arc<Self>,
        effect: ChatEffect,
        durable: impl FnOnce() -> Result<(T, ChatResponseSnapshot)> + Send + 'static,
    ) -> Result<(T, ChatResponseSnapshot)> {
        let ChatEffect {
            principal_id,
            pin,
            operation,
        } = effect;
        let publisher = self.clone();
        tokio::spawn(async move {
            let _operation = operation;
            let owner = owner_key(&principal_id);
            let wake_owner = owner.clone();
            let worker = publisher.clone();
            let publication = tokio::task::spawn_blocking(move || -> Result<_> {
                let directory = worker.root.prepare_child(&owner)?;
                directory.commit_document_durable_with_decision(
                    PLUGIN,
                    &uuid::Uuid::new_v4().to_string(),
                    |previous| {
                        let receipt = durable()?;
                        let prepared = (|| -> Result<Vec<u8>> {
                            let bytes = serde_json::to_vec(&state(
                                &receipt.1,
                                &principal_id,
                                &pin.schema,
                            )?)?;
                            compare_revision(previous, &bytes, receipt.1.revision)?;
                            worker.recheck(&pin)?;
                            Ok(bytes)
                        })();
                        Ok(match prepared {
                            Ok(bytes) => PrivateDocumentCommit::Publish {
                                current: Some(bytes),
                                durable: receipt,
                            },
                            Err(error) => PrivateDocumentCommit::RetainPending {
                                durable: receipt,
                                error,
                            },
                        })
                    },
                )
            })
            .await??;
            match publication {
                PrivateDocumentPublication::Committed { durable, .. } => {
                    let _ = publisher.wakes.send(wake_owner);
                    Ok(durable)
                }
                PrivateDocumentPublication::Pending { .. } => anyhow::bail!(
                    "chat_response committed durably but its private state is pending recovery"
                ),
            }
        })
        .await
        .context("owned chat_response publication task failed")?
    }

    /// Explicit authenticated read admission restores the private view from
    /// its sole durable owner. The caller has verified `principal_id`.
    pub(crate) async fn recover(
        self: &Arc<Self>,
        store: Arc<CozoGraphShuttle>,
        principal_id: &str,
    ) -> Result<()> {
        let operation = self.operation.clone().lock_owned().await;
        let publisher = self.clone();
        let principal = principal_id.to_string();
        tokio::spawn(async move {
            let _operation = operation;
            let owner = owner_key(&principal);
            let wake_owner = owner.clone();
            let worker = publisher.clone();
            let publication = tokio::task::spawn_blocking(move || -> Result<_> {
                let pin = worker.pin()?;
                let directory = worker.root.prepare_child(&owner)?;
                directory.ensure_distinct_from(std::path::Path::new(
                    &op_core::projection_shm::shm_state_dir(),
                ))?;
                if let Some(pending) = directory.pending_document()? {
                    anyhow::ensure!(
                        pending.plugin_id == PLUGIN,
                        "pending chat_response owner mismatch"
                    );
                    directory.recover_document_with_decision(PLUGIN, &pending.operation_id, || {
                        worker.republish(&store, &principal, &pin, None)
                    })
                } else {
                    directory.commit_document_durable_with_decision(
                        PLUGIN,
                        &uuid::Uuid::new_v4().to_string(),
                        |previous| worker.republish(&store, &principal, &pin, previous),
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
                    anyhow::bail!("chat_response projection recovery is pending")
                }
            }
        })
        .await
        .context("owned chat_response recovery task failed")?
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn subscribe(&self) -> broadcast::Receiver<String> {
        self.wakes.subscribe()
    }

    /// The committed private document of `principal_id`, revalidated.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) async fn read(self: &Arc<Self>, principal_id: &str) -> Result<Value> {
        let publisher = self.clone();
        let principal = principal_id.to_string();
        tokio::task::spawn_blocking(move || -> Result<_> {
            let pin = publisher.pin()?;
            let directory = publisher
                .root
                .open_child(&owner_key(&principal))?
                .context("chat_response state requires list_conversations activation")?;
            let snapshot = directory.read_snapshot(&[PLUGIN.into()])?;
            let bytes = snapshot
                .states
                .get(PLUGIN)
                .context("chat_response state requires list_conversations activation")?;
            let value: Value = serde_json::from_slice(bytes)?;
            validate_state(&value, &pin.schema)?;
            validate_scope(&value, &principal)?;
            publisher.recheck(&pin)?;
            Ok(value)
        })
        .await?
    }
}
