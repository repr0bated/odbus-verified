//! Fold all native observations into one durable record, publish every affected
//! namespace, and only then permit a notification or completed receipt.
use super::*;

struct FoldedTree {
    previous: Option<Vec<u8>>,
    state: Option<Value>,
    schema_hash: String,
    object_path: String,
}

impl PersistentCore {
    fn raw_state(&self, scope: &StateScope, plugin: &str) -> Result<Option<Vec<u8>>> {
        let ids = [plugin.to_owned()];
        let snapshot = match scope {
            StateScope::Public => projection_shm::read_projection_snapshot_in(&self.state_dir, &ids)?,
            StateScope::Private { .. } => {
                let Some(directory) = self.private_directory(scope, false)? else {
                    ensure!(!self.heads.contains_key(&(scope.clone(), plugin.to_owned())),
                        "durable private namespace is missing; explicit recovery required");
                    return Ok(None);
                };
                directory.read_snapshot(&ids)?
            }
        };
        let value = snapshot.states.into_iter().next().map(|(_, bytes)| bytes);
        if let Some(head) = self.heads.get(&(scope.clone(), plugin.to_owned())) {
            let decoded: Option<Value> = value.as_deref().map(serde_json::from_slice).transpose()?;
            ensure!(head.state_hash == state_hash(decoded.as_ref())?,
                "committed raw state differs from durable bridge record");
        }
        Ok(value)
    }

    pub(in crate::core) fn commit_with_updates(
        &mut self,
        catalog: &Catalog,
        owner: StateScope,
        commit: BackendCommit,
        updates: Vec<BackendProjection>,
        terminal: Option<(String, MethodRecoveryStatus, String)>,
    ) -> Result<StateCommit> {
        self.require_available()?;
        validate_scope(&owner)?;
        let BackendCommit { mutation, scope, schema_hash, state, result: _ } = commit;
        self.pin(catalog, &mutation.plugin_id, &schema_hash)?;
        path_segments(&mutation.plugin_id, &mutation.object_path)?;
        let primary = BackendProjection {
            plugin_id: mutation.plugin_id.clone(), object_path: mutation.object_path.clone(),
            schema_hash: schema_hash.clone(), scope: scope.clone(), state,
        };
        let mut trees = BTreeMap::<(StateScope, String), FoldedTree>::new();
        let mut external_publications = Vec::new();
        let mut identity_publication = None;
        for update in std::iter::once(primary).chain(updates) {
            validate_scope(&update.scope)?;
            if matches!(update.scope, StateScope::Private { .. }) {
                ensure!(update.scope == owner, "backend observation crosses verified private scope");
            }
            self.pin(catalog, &update.plugin_id, &update.schema_hash)?;
            path_segments(&update.plugin_id, &update.object_path)?;
            if matches!(update.state, BackendStateUpdate::Unchanged) { continue; }
            if let BackendStateUpdate::IdentityPublished(proof) = update.state {
                ensure!(update.plugin_id == "identity_sled"
                    && path_segments(&update.plugin_id, &update.object_path)?.is_empty()
                    && terminal.is_some(), "paired identity receipt requires its native root operation");
                let actual = projection_shm::read_projection_snapshot_in(&self.state_dir, &[update.plugin_id.clone()])?;
                // Other plugins may advance the shared namespace generation.
                // The identity bytes must still be exactly the paired owner's
                // committed public metadata, never its private credentials.
                ensure!(proof.generation > 0 && actual.generation >= proof.generation
                    && actual.states.get(&update.plugin_id).map(Vec::as_slice) == proof.current.as_deref(),
                    "paired identity publication changed before terminal");
                ensure!(identity_publication.is_none(), "duplicate paired identity receipt");
                let metadata: Option<Value> = proof.current.as_deref().map(serde_json::from_slice).transpose()?;
                ensure!(metadata.as_ref().is_none_or(Value::is_object), "identity metadata root is not an object");
                identity_publication = Some(IdentityPublication {
                    schema_hash: update.schema_hash,
                    state_hash: state_hash(metadata.as_ref())?,
                    generation: proof.generation,
                });
                external_publications.push((StateScope::Public, update.plugin_id, actual.generation));
                continue;
            }
            ensure!(update.plugin_id != "identity_sled", "identity requires its dedicated paired durable owner");
            let key = (update.scope, update.plugin_id);
            if !trees.contains_key(&key) {
                let previous = self.raw_state(&key.0, &key.1)?;
                let state = previous.as_deref().map(serde_json::from_slice).transpose()?;
                trees.insert(key.clone(), FoldedTree {
                    previous, state, schema_hash: update.schema_hash.clone(), object_path: update.object_path.clone(),
                });
            }
            let tree = trees.get_mut(&key).expect("inserted state tree");
            ensure!(tree.schema_hash == update.schema_hash, "batch contains conflicting schema pins");
            tree.state = fold(tree.state.take(), &key.1, &update.object_path, update.state)?;
            tree.object_path = update.object_path;
        }
        let token = format!("bridge-{}", uuid::Uuid::new_v4());
        let mut publications = Vec::with_capacity(trees.len());
        let mut namespaces = BTreeMap::<StateScope, (ProjectionDocuments, ProjectionDocuments)>::new();
        let mut pins = Vec::with_capacity(trees.len());
        for ((state_scope, plugin_id), tree) in trees {
            let bytes = tree.state.as_ref().map(serde_json::to_vec).transpose()?;
            let (previous, current) = namespaces.entry(state_scope.clone()).or_default();
            previous.insert(plugin_id.clone(), tree.previous);
            current.insert(plugin_id.clone(), bytes);
            pins.push((plugin_id.clone(), tree.schema_hash.clone()));
            publications.push(Publication {
                scope: state_scope, plugin_id, schema_hash: tree.schema_hash,
                object_path: tree.object_path, operation_id: token.clone(), state: tree.state,
            });
        }
        let action = match terminal {
            Some((operation_id, status, code)) => LedgerAction::Terminal {
                operation_id, status, code, publications, identity_publication,
            },
            None => {
                ensure!(!publications.is_empty(), "state observation contains no authoritative update");
                LedgerAction::State { publications }
            }
        };
        // Persist every recovery tree before any namespace can acquire a
        // pending marker. The commit lock prevents receipt disclosure until
        // publication finishes; reopen can recover even a first-ever write.
        let event = self.append(catalog, owner, &mutation, &schema_hash, action)?;
        if namespaces.is_empty() {
            return Ok(StateCommit { event, published: external_publications, scope, object_path: mutation.object_path });
        }
        let mut published = external_publications;
        for (state_scope, (previous, current)) in namespaces {
            let ids: Vec<_> = current.keys().cloned().collect();
            let private = if matches!(state_scope, StateScope::Private { .. }) {
                match self.private_directory(&state_scope, true) {
                    Ok(Some(directory)) => Some(directory),
                    Ok(None) => unreachable!("private writer preparation returns a directory"),
                    Err(error) => {
                        self.failure = Some("namespace unavailable after durable terminal".into());
                        return Err(error);
                    }
                }
            } else { None };
            let state_dir = self.state_dir.clone();
            let callback = |actual: &ProjectionDocuments| -> Result<(ProjectionDocuments, ())> {
                ensure!(actual == &previous, "state changed before batch publication");
                Ok((current, ()))
            };
            let outcome = match private {
                Some(private) => private.commit_documents_durable(&ids, &token, callback),
                None => projection_shm::commit_projection_batch_durable_in(&state_dir, &ids, &token, callback),
            };
            match outcome {
                Ok(ProjectionBatchPublication::Committed { projection, .. }) => {
                    published.extend(ids.into_iter().map(|id| (state_scope.clone(), id, projection.generation)));
                }
                Ok(ProjectionBatchPublication::Pending { .. }) | Err(_) => {
                    self.failure = Some("multi-plugin state publication requires durable recovery".into());
                    bail!("state publication outcome unknown; reopen before any read or retry");
                }
            }
        }
        for (plugin, pin) in pins {
            if let Err(error) = self.pin(catalog, &plugin, &pin) {
                self.failure = Some("sealed state contract changed after durable batch".into());
                return Err(error);
            }
        }
        Ok(StateCommit {
            event, published, scope,
            object_path: mutation.object_path,
        })
    }
}
