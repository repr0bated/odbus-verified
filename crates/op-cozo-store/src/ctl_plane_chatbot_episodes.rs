//! Durable private chatbot episodes in a dedicated Cozo domain owner.

use super::identity_transactions::commit_checked;
use super::*;
use cozo::MultiTransaction;

const MAX_LIST_LIMIT: usize = 200;
pub const MAX_CHATBOT_EPISODE_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_CHATBOT_SNAPSHOT_BYTES: usize = 16 * 1024 * 1024;
const SNAPSHOT_ENVELOPE_BYTES: usize = 4 * 1024;
const MAX_SNAPSHOT_EPISODE_BYTES: usize = MAX_CHATBOT_SNAPSHOT_BYTES - SNAPSHOT_ENVELOPE_BYTES;
const MAX_PRINCIPAL_BYTES: usize = 512;
const MAX_SESSION_BYTES: usize = 256;
const MAX_GENESIS_BYTES: usize = 512;
const MAX_EPISODE_ID_BYTES: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EpisodeScope {
    pub principal_id: String,
    pub session_id: String,
    pub session_genesis: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EpisodeEmbeddingRecord {
    pub vector: Vec<f32>,
    pub provider: String,
    pub model: String,
    pub provider_model: Option<String>,
    pub dimensions: u32,
    pub config_revision: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ScopedEpisodeRecord {
    pub scope: EpisodeScope,
    pub episode_id: String,
    pub content_hash: String,
    pub episode: Value,
    pub embedding: Option<EpisodeEmbeddingRecord>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EpisodeSnapshot {
    pub episodes: Vec<ScopedEpisodeRecord>,
    pub episode_count: u64,
    pub revision: u64,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EpisodePage {
    pub episodes: Vec<ScopedEpisodeRecord>,
    pub episode_count: u64,
    pub revision: u64,
    pub next_episode_id: Option<String>,
}

fn error(message: &str) -> CozoError {
    CozoError::Other(message.into())
}

fn validate_embedding(value: &EpisodeEmbeddingRecord) -> std::result::Result<(), CozoError> {
    if value.dimensions == 0
        || value.vector.len() != value.dimensions as usize
        || !value.vector.iter().all(|component| component.is_finite())
        || value.provider.is_empty()
        || value.provider.len() > 128
        || value.model.is_empty()
        || value.model.len() > 512
        || value
            .provider_model
            .as_ref()
            .is_some_and(|model| model.is_empty() || model.len() > 512)
    {
        return Err(error("invalid chatbot episode embedding"));
    }
    Ok(())
}

fn bounded_identity(value: &str, maximum: usize) -> bool {
    !value.is_empty()
        && value.len() <= maximum
        && !value.chars().any(char::is_control)
        && value.trim() == value
}

fn validate_scope(scope: &EpisodeScope) -> std::result::Result<(), CozoError> {
    if !bounded_identity(&scope.principal_id, MAX_PRINCIPAL_BYTES)
        || !bounded_identity(&scope.session_id, MAX_SESSION_BYTES)
        || !bounded_identity(&scope.session_genesis, MAX_GENESIS_BYTES)
    {
        return Err(error("invalid chatbot episode identity scope"));
    }
    Ok(())
}

fn validate_episode_id(episode_id: &str) -> std::result::Result<(), CozoError> {
    if !bounded_identity(episode_id, MAX_EPISODE_ID_BYTES) {
        return Err(error("invalid chatbot episode id"));
    }
    Ok(())
}

fn validate_row(row: &ScopedEpisodeRecord) -> std::result::Result<(), CozoError> {
    validate_scope(&row.scope)?;
    validate_episode_id(&row.episode_id)?;
    if row.content_hash.len() != 64
        || !row
            .content_hash
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
        || serde_json::to_vec(&row.episode)?.len() > MAX_CHATBOT_EPISODE_BYTES
    {
        return Err(error("invalid chatbot episode document"));
    }
    let object = row
        .episode
        .as_object()
        .ok_or_else(|| error("invalid chatbot episode document"))?;
    let matches =
        |field: &str, expected: &str| object.get(field).and_then(Value::as_str) == Some(expected);
    if !matches("principal_id", &row.scope.principal_id)
        || !matches("session_id", &row.scope.session_id)
        || !matches("session_genesis", &row.scope.session_genesis)
        || !matches("on_behalf_of", &row.scope.principal_id)
        || !matches("episode_id", &row.episode_id)
        || !matches("content_hash", &row.content_hash)
    {
        return Err(error(
            "chatbot episode document differs from its identity key",
        ));
    }
    if let Some(embedding) = &row.embedding {
        validate_embedding(embedding)?;
    }
    Ok(())
}

fn params(scope: &EpisodeScope, episode_id: Option<&str>) -> Params {
    let mut values = BTreeMap::from([
        (
            "principal".into(),
            DataValue::Str(scope.principal_id.as_str().into()),
        ),
        (
            "session".into(),
            DataValue::Str(scope.session_id.as_str().into()),
        ),
        (
            "genesis".into(),
            DataValue::Str(scope.session_genesis.as_str().into()),
        ),
    ]);
    if let Some(episode_id) = episode_id {
        values.insert("episode".into(), DataValue::Str(episode_id.into()));
    }
    values
}

fn read_rows(
    tx: &MultiTransaction,
    script: &str,
    values: Params,
    scope: &EpisodeScope,
) -> std::result::Result<Vec<ScopedEpisodeRecord>, CozoError> {
    let rows = tx
        .run_script(script, values)
        .map_err(|error| CozoError::Other(format!("episode read failed: {error}")))?;
    rows.rows
        .into_iter()
        .map(|row| {
            let [DataValue::Str(episode_id), DataValue::Str(content_hash), DataValue::Str(episode_json), DataValue::Str(embedding_json)] =
                row.as_slice()
            else {
                return Err(error("corrupt chatbot episode row"));
            };
            let record = ScopedEpisodeRecord {
                scope: scope.clone(),
                episode_id: episode_id.to_string(),
                content_hash: content_hash.to_string(),
                episode: serde_json::from_str(episode_json)
                    .map_err(|_| error("corrupt chatbot episode JSON"))?,
                embedding: if embedding_json.is_empty() {
                    None
                } else {
                    Some(
                        serde_json::from_str::<EpisodeEmbeddingWire>(embedding_json)
                            .map_err(|_| error("corrupt chatbot episode embedding"))?
                            .into(),
                    )
                },
            };
            validate_row(&record)?;
            Ok(record)
        })
        .collect()
}

fn read_scope_state(
    tx: &MultiTransaction,
    scope: &EpisodeScope,
) -> std::result::Result<(u64, u64), CozoError> {
    let rows = tx
        .run_script(
            "?[revision, episode_count] := *ctl_plane_chatbot_episode_scopes{principal_id, session_id, session_genesis, revision, episode_count}, principal_id = $principal, session_id = $session, session_genesis = $genesis",
            params(scope, None),
        )
        .map_err(|error| CozoError::Other(format!("episode scope read failed: {error}")))?;
    match rows.rows.as_slice() {
        [] => Ok((0, 0)),
        [row] => match row.as_slice() {
            [DataValue::Num(Num::Int(revision)), DataValue::Num(Num::Int(count))]
                if *revision >= 0 && *count >= 0 =>
            {
                Ok((*revision as u64, *count as u64))
            }
            _ => Err(error("corrupt chatbot episode scope row")),
        },
        _ => Err(error("corrupt chatbot episode scope row")),
    }
}

fn write_scope_state(
    tx: &MultiTransaction,
    scope: &EpisodeScope,
    revision: u64,
    count: u64,
) -> std::result::Result<(), CozoError> {
    let revision = i64::try_from(revision).map_err(|_| error("episode revision exhausted"))?;
    let count = i64::try_from(count).map_err(|_| error("episode count exhausted"))?;
    let mut values = params(scope, None);
    values.insert("revision".into(), DataValue::Num(Num::Int(revision)));
    values.insert("count".into(), DataValue::Num(Num::Int(count)));
    tx.run_script(
        "?[principal_id, session_id, session_genesis, revision, episode_count] <- [[$principal, $session, $genesis, $revision, $count]] :put ctl_plane_chatbot_episode_scopes {principal_id, session_id, session_genesis => revision, episode_count}",
        values,
    )
    .map_err(|error| CozoError::Other(format!("episode scope write failed: {error}")))?;
    Ok(())
}

fn snapshot_in_transaction(
    tx: &MultiTransaction,
    scope: &EpisodeScope,
    limit: usize,
) -> std::result::Result<EpisodeSnapshot, CozoError> {
    if !(1..=MAX_LIST_LIMIT).contains(&limit) {
        return Err(error("chatbot episode snapshot limit must be 1..=200"));
    }
    let (revision, episode_count) = read_scope_state(tx, scope)?;
    let script = format!(
        "?[episode_id, content_hash, episode_json, embedding_json] := *ctl_plane_chatbot_episodes{{principal_id, session_id, session_genesis, episode_id, content_hash, episode_json, embedding_json}}, principal_id = $principal, session_id = $session, session_genesis = $genesis :order -episode_id :limit {limit}"
    );
    let candidates = read_rows(tx, &script, params(scope, None), scope)?;
    let mut episodes = Vec::with_capacity(candidates.len());
    let mut bytes = 0usize;
    for row in candidates {
        let row_bytes = serde_json::to_vec(&row.episode)?.len()
            + serde_json::to_vec(&row.episode_id)?.len()
            + 2;
        if bytes.saturating_add(row_bytes) > MAX_SNAPSHOT_EPISODE_BYTES {
            break;
        }
        bytes += row_bytes;
        episodes.push(row);
    }
    Ok(EpisodeSnapshot {
        truncated: episode_count > episodes.len() as u64,
        episodes,
        episode_count,
        revision,
    })
}

#[derive(serde::Serialize, serde::Deserialize)]
struct EpisodeEmbeddingWire {
    vector: Vec<f32>,
    provider: String,
    model: String,
    provider_model: Option<String>,
    dimensions: u32,
    config_revision: u64,
}

impl From<EpisodeEmbeddingRecord> for EpisodeEmbeddingWire {
    fn from(value: EpisodeEmbeddingRecord) -> Self {
        Self {
            vector: value.vector,
            provider: value.provider,
            model: value.model,
            provider_model: value.provider_model,
            dimensions: value.dimensions,
            config_revision: value.config_revision,
        }
    }
}

impl From<EpisodeEmbeddingWire> for EpisodeEmbeddingRecord {
    fn from(value: EpisodeEmbeddingWire) -> Self {
        Self {
            vector: value.vector,
            provider: value.provider,
            model: value.model,
            provider_model: value.provider_model,
            dimensions: value.dimensions,
            config_revision: value.config_revision,
        }
    }
}

impl CozoGraphShuttle {
    /// Opt this handle into the private chatbot episode domain. Generic Cozo
    /// stores deliberately do not expose these relations.
    pub fn initialize_chatbot_episode_store(&self) -> std::result::Result<(), CozoError> {
        let _guard = self
            .episode_transaction
            .lock()
            .map_err(|_| error("chatbot episode owner is poisoned"))?;
        self.runtime_document_preflight()?;
        for relation in [
            r#":create ctl_plane_chatbot_episodes {
                principal_id: String, session_id: String, session_genesis: String, episode_id: String
                =>
                content_hash: String,
                episode_json: String,
                embedding_json: String default ""
            }"#,
            r#":create ctl_plane_chatbot_episode_scopes {
                principal_id: String, session_id: String, session_genesis: String
                => revision: Int, episode_count: Int
            }"#,
        ] {
            if let Err(error) =
                self.db
                    .run_script(relation, BTreeMap::new(), ScriptMutability::Mutable)
            {
                let message = error.to_string();
                if !message.contains("already exists")
                    && !message.contains("AlreadyExists")
                    && !message.contains("conflicts with an existing")
                {
                    return Err(CozoError::Other(format!(
                        "chatbot episode relation initialization failed: {message}"
                    )));
                }
            }
        }
        for probe in [
            "?[principal_id, session_id, session_genesis, episode_id, content_hash, episode_json, embedding_json] := *ctl_plane_chatbot_episodes{principal_id, session_id, session_genesis, episode_id, content_hash, episode_json, embedding_json} :limit 0",
            "?[principal_id, session_id, session_genesis, revision, episode_count] := *ctl_plane_chatbot_episode_scopes{principal_id, session_id, session_genesis, revision, episode_count} :limit 0",
        ] {
            self.db
                .run_script(probe, BTreeMap::new(), ScriptMutability::Immutable)
                .map_err(|error| {
                    CozoError::Other(format!(
                        "chatbot episode relation verification failed: {error}"
                    ))
                })?;
        }
        self.sync_runtime_document_store()
    }

    pub fn insert_scoped_episode_and_snapshot(
        &self,
        row: &ScopedEpisodeRecord,
        snapshot_limit: usize,
    ) -> std::result::Result<EpisodeSnapshot, CozoError> {
        validate_row(row)?;
        if row.embedding.is_some() {
            return Err(error(
                "new chatbot episodes cannot contain a derived embedding",
            ));
        }
        let _guard = self
            .episode_transaction
            .lock()
            .map_err(|_| error("chatbot episode owner is poisoned"))?;
        self.runtime_document_preflight()?;
        let tx = self.db.multi_transaction(true);
        if !read_rows(
            &tx,
            "?[episode_id, content_hash, episode_json, embedding_json] := *ctl_plane_chatbot_episodes{principal_id, session_id, session_genesis, episode_id, content_hash, episode_json, embedding_json}, principal_id = $principal, session_id = $session, session_genesis = $genesis, episode_id = $episode",
            params(&row.scope, Some(&row.episode_id)),
            &row.scope,
        )?
        .is_empty()
        {
            return Err(error("chatbot episode already exists"));
        }
        let (revision, count) = read_scope_state(&tx, &row.scope)?;
        let revision = revision
            .checked_add(1)
            .ok_or_else(|| error("episode revision exhausted"))?;
        let count = count
            .checked_add(1)
            .ok_or_else(|| error("episode count exhausted"))?;
        let episode_json = serde_json::to_string(&row.episode)?;
        let embedding_json = row
            .embedding
            .clone()
            .map(EpisodeEmbeddingWire::from)
            .map(|value| serde_json::to_string(&value))
            .transpose()?
            .unwrap_or_default();
        let mut values = params(&row.scope, Some(&row.episode_id));
        values.insert(
            "hash".into(),
            DataValue::Str(row.content_hash.as_str().into()),
        );
        values.insert("document".into(), DataValue::Str(episode_json.into()));
        values.insert("embedding".into(), DataValue::Str(embedding_json.into()));
        tx.run_script(
            "?[principal_id, session_id, session_genesis, episode_id, content_hash, episode_json, embedding_json] <- [[$principal, $session, $genesis, $episode, $hash, $document, $embedding]] :put ctl_plane_chatbot_episodes {principal_id, session_id, session_genesis, episode_id => content_hash, episode_json, embedding_json}",
            values,
        )
        .map_err(|error| CozoError::Other(format!("episode write failed: {error}")))?;
        write_scope_state(&tx, &row.scope, revision, count)?;
        let snapshot = snapshot_in_transaction(&tx, &row.scope, snapshot_limit)?;
        commit_checked(&tx)?;
        self.sync_runtime_document_store()?;
        Ok(snapshot)
    }

    pub fn get_scoped_episode(
        &self,
        scope: &EpisodeScope,
        episode_id: &str,
    ) -> std::result::Result<Option<ScopedEpisodeRecord>, CozoError> {
        validate_scope(scope)?;
        validate_episode_id(episode_id)?;
        let _guard = self
            .episode_transaction
            .lock()
            .map_err(|_| error("chatbot episode owner is poisoned"))?;
        let tx = self.db.multi_transaction(false);
        let mut rows = read_rows(
            &tx,
            "?[episode_id, content_hash, episode_json, embedding_json] := *ctl_plane_chatbot_episodes{principal_id, session_id, session_genesis, episode_id, content_hash, episode_json, embedding_json}, principal_id = $principal, session_id = $session, session_genesis = $genesis, episode_id = $episode",
            params(scope, Some(episode_id)),
            scope,
        )?;
        commit_checked(&tx)?;
        match rows.len() {
            0 => Ok(None),
            1 => Ok(rows.pop()),
            _ => Err(error("duplicate chatbot episode rows")),
        }
    }

    pub fn list_scoped_episodes(
        &self,
        scope: &EpisodeScope,
        after_episode_id: Option<&str>,
        limit: usize,
    ) -> std::result::Result<Vec<ScopedEpisodeRecord>, CozoError> {
        validate_scope(scope)?;
        if let Some(cursor) = after_episode_id {
            validate_episode_id(cursor)?;
        }
        if !(1..=MAX_LIST_LIMIT).contains(&limit) {
            return Err(error("chatbot episode list limit must be 1..=200"));
        }
        let _guard = self
            .episode_transaction
            .lock()
            .map_err(|_| error("chatbot episode owner is poisoned"))?;
        let tx = self.db.multi_transaction(false);
        let cursor = after_episode_id.unwrap_or("");
        let mut values = params(scope, None);
        values.insert("cursor".into(), DataValue::Str(cursor.into()));
        let script = format!(
            "?[episode_id, content_hash, episode_json, embedding_json] := *ctl_plane_chatbot_episodes{{principal_id, session_id, session_genesis, episode_id, content_hash, episode_json, embedding_json}}, principal_id = $principal, session_id = $session, session_genesis = $genesis, episode_id > $cursor :order episode_id :limit {limit}"
        );
        let rows = read_rows(&tx, &script, values, scope)?;
        commit_checked(&tx)?;
        Ok(rows)
    }

    pub fn list_scoped_episode_page(
        &self,
        scope: &EpisodeScope,
        after_episode_id: Option<&str>,
        limit: usize,
    ) -> std::result::Result<EpisodePage, CozoError> {
        validate_scope(scope)?;
        if let Some(cursor) = after_episode_id {
            validate_episode_id(cursor)?;
        }
        if !(1..MAX_LIST_LIMIT).contains(&limit) {
            return Err(error("chatbot episode page limit must be 1..=199"));
        }
        let _guard = self
            .episode_transaction
            .lock()
            .map_err(|_| error("chatbot episode owner is poisoned"))?;
        let tx = self.db.multi_transaction(false);
        let (revision, episode_count) = read_scope_state(&tx, scope)?;
        let cursor = after_episode_id.unwrap_or("");
        let mut values = params(scope, None);
        values.insert("cursor".into(), DataValue::Str(cursor.into()));
        let fetch = limit + 1;
        let script = format!(
            "?[episode_id, content_hash, episode_json, embedding_json] := *ctl_plane_chatbot_episodes{{principal_id, session_id, session_genesis, episode_id, content_hash, episode_json, embedding_json}}, principal_id = $principal, session_id = $session, session_genesis = $genesis, episode_id > $cursor :order episode_id :limit {fetch}"
        );
        let candidates = read_rows(&tx, &script, values, scope)?;
        let mut episodes = Vec::with_capacity(candidates.len().min(limit));
        let mut bytes = 0usize;
        let mut more = false;
        for row in candidates {
            if episodes.len() == limit {
                more = true;
                break;
            }
            let row_bytes = serde_json::to_vec(&row.episode)?.len()
                + serde_json::to_vec(&row.episode_id)?.len()
                + 2;
            if bytes.saturating_add(row_bytes) > MAX_SNAPSHOT_EPISODE_BYTES {
                more = true;
                break;
            }
            bytes += row_bytes;
            episodes.push(row);
        }
        if more && episodes.is_empty() {
            return Err(error("chatbot episode exceeds the bounded page payload"));
        }
        let next_episode_id = more.then(|| {
            episodes
                .last()
                .expect("nonempty bounded episode page")
                .episode_id
                .clone()
        });
        commit_checked(&tx)?;
        Ok(EpisodePage {
            episodes,
            episode_count,
            revision,
            next_episode_id,
        })
    }

    pub fn snapshot_scoped_episodes(
        &self,
        scope: &EpisodeScope,
        limit: usize,
    ) -> std::result::Result<EpisodeSnapshot, CozoError> {
        validate_scope(scope)?;
        let _guard = self
            .episode_transaction
            .lock()
            .map_err(|_| error("chatbot episode owner is poisoned"))?;
        let tx = self.db.multi_transaction(false);
        let snapshot = snapshot_in_transaction(&tx, scope, limit)?;
        commit_checked(&tx)?;
        Ok(snapshot)
    }

    pub fn compare_exchange_episode_embedding(
        &self,
        scope: &EpisodeScope,
        episode_id: &str,
        content_hash: &str,
        expected: Option<&EpisodeEmbeddingRecord>,
        replacement: &EpisodeEmbeddingRecord,
        snapshot_limit: usize,
    ) -> std::result::Result<EpisodeSnapshot, CozoError> {
        validate_scope(scope)?;
        validate_episode_id(episode_id)?;
        validate_embedding(replacement)?;
        let _guard = self
            .episode_transaction
            .lock()
            .map_err(|_| error("chatbot episode owner is poisoned"))?;
        self.runtime_document_preflight()?;
        let tx = self.db.multi_transaction(true);
        let current = read_rows(
            &tx,
            "?[episode_id, content_hash, episode_json, embedding_json] := *ctl_plane_chatbot_episodes{principal_id, session_id, session_genesis, episode_id, content_hash, episode_json, embedding_json}, principal_id = $principal, session_id = $session, session_genesis = $genesis, episode_id = $episode",
            params(scope, Some(episode_id)),
            scope,
        )?
        .pop()
        .ok_or_else(|| error("chatbot episode does not exist"))?;
        if current.content_hash != content_hash || current.embedding.as_ref() != expected {
            return Err(error("chatbot episode embedding compare-exchange conflict"));
        }
        let (revision, count) = read_scope_state(&tx, scope)?;
        let revision = revision
            .checked_add(1)
            .ok_or_else(|| error("episode revision exhausted"))?;
        let embedding_json =
            serde_json::to_string(&EpisodeEmbeddingWire::from(replacement.clone()))?;
        let mut values = params(scope, Some(episode_id));
        values.insert("embedding".into(), DataValue::Str(embedding_json.into()));
        tx.run_script(
            "?[principal_id, session_id, session_genesis, episode_id, embedding_json] <- [[$principal, $session, $genesis, $episode, $embedding]] :update ctl_plane_chatbot_episodes {principal_id, session_id, session_genesis, episode_id => embedding_json}",
            values,
        )
        .map_err(|error| CozoError::Other(format!("episode embedding write failed: {error}")))?;
        write_scope_state(&tx, scope, revision, count)?;
        let snapshot = snapshot_in_transaction(&tx, scope, snapshot_limit)?;
        commit_checked(&tx)?;
        self.sync_runtime_document_store()?;
        Ok(snapshot)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scope(principal: &str) -> EpisodeScope {
        EpisodeScope {
            principal_id: principal.into(),
            session_id: "session".into(),
            session_genesis: "ab".repeat(32),
        }
    }

    fn row(scope: EpisodeScope, id: &str, hash: &str) -> ScopedEpisodeRecord {
        ScopedEpisodeRecord {
            episode: serde_json::json!({
                "principal_id": scope.principal_id.clone(),
                "session_id": scope.session_id.clone(),
                "session_genesis": scope.session_genesis.clone(),
                "on_behalf_of": scope.principal_id.clone(),
                "episode_id": id,
                "content_hash": hash,
            }),
            scope,
            episode_id: id.into(),
            content_hash: hash.into(),
            embedding: None,
        }
    }

    fn embedding(value: f32) -> EpisodeEmbeddingRecord {
        EpisodeEmbeddingRecord {
            vector: vec![value, value],
            provider: "fixture".into(),
            model: "model".into(),
            provider_model: Some("reported".into()),
            dimensions: 2,
            config_revision: 3,
        }
    }

    #[test]
    fn immutable_scoped_rows_cursor_and_embedding_cas_are_atomic() {
        let store = CozoGraphShuttle::new_in_memory().unwrap();
        store.initialize_chatbot_episode_store().unwrap();
        let alice = scope("alice");
        let bob = scope("bob");
        let first_hash = "aa".repeat(32);
        let second_hash = "bb".repeat(32);
        let hidden_hash = "cc".repeat(32);
        let first = row(alice.clone(), "001", &first_hash);
        let second = row(alice.clone(), "002", &second_hash);
        let hidden = row(bob.clone(), "001", &hidden_hash);
        let snapshot = store
            .insert_scoped_episode_and_snapshot(&first, 32)
            .unwrap();
        assert_eq!((snapshot.revision, snapshot.episode_count), (1, 1));
        store
            .insert_scoped_episode_and_snapshot(&second, 32)
            .unwrap();
        store
            .insert_scoped_episode_and_snapshot(&hidden, 32)
            .unwrap();
        assert!(store
            .insert_scoped_episode_and_snapshot(&first, 32)
            .is_err());
        let before_invalid = store.snapshot_scoped_episodes(&alice, 32).unwrap();
        let mut invalid = row(alice.clone(), "003", &"dd".repeat(32));
        invalid.episode["on_behalf_of"] = serde_json::json!("another-principal");
        assert!(store
            .insert_scoped_episode_and_snapshot(&invalid, 32)
            .is_err());
        assert_eq!(
            store.snapshot_scoped_episodes(&alice, 32).unwrap(),
            before_invalid
        );
        assert_eq!(
            store
                .list_scoped_episodes(&alice, Some("001"), 10)
                .unwrap()
                .iter()
                .map(|row| row.episode_id.as_str())
                .collect::<Vec<_>>(),
            ["002"]
        );
        let first_page = store.list_scoped_episode_page(&alice, None, 1).unwrap();
        assert_eq!(first_page.episodes[0].episode_id, "001");
        assert_eq!(first_page.next_episode_id.as_deref(), Some("001"));
        let second_page = store
            .list_scoped_episode_page(&alice, first_page.next_episode_id.as_deref(), 1)
            .unwrap();
        assert_eq!(second_page.episodes[0].episode_id, "002");
        assert!(second_page.next_episode_id.is_none());
        assert!(store.get_scoped_episode(&bob, "002").unwrap().is_none());

        let replacement = embedding(0.5);
        let snapshot = store
            .compare_exchange_episode_embedding(&alice, "001", &first_hash, None, &replacement, 1)
            .unwrap();
        assert_eq!((snapshot.revision, snapshot.episode_count), (3, 2));
        assert!(snapshot.truncated);
        assert_eq!(
            store
                .get_scoped_episode(&alice, "001")
                .unwrap()
                .unwrap()
                .embedding,
            Some(replacement.clone())
        );
        assert!(store
            .compare_exchange_episode_embedding(
                &alice,
                "001",
                "wrong",
                Some(&replacement),
                &embedding(0.75),
                32,
            )
            .is_err());
        assert_eq!(
            store.snapshot_scoped_episodes(&alice, 32).unwrap().revision,
            3
        );
        let mut invalid_embedding = embedding(0.5);
        invalid_embedding.vector[0] = f32::NAN;
        assert!(store
            .compare_exchange_episode_embedding(
                &alice,
                "001",
                &first_hash,
                Some(&replacement),
                &invalid_embedding,
                32,
            )
            .is_err());
    }

    #[test]
    fn generic_cozo_store_has_no_episode_relations_and_cannot_mutate_episode_owner() {
        let generic = CozoGraphShuttle::new_in_memory().unwrap();
        let private = CozoGraphShuttle::new_in_memory().unwrap();
        assert!(generic.ensure_distinct_from(&generic.clone()).is_err());
        generic.ensure_distinct_from(&private).unwrap();
        private.initialize_chatbot_episode_store().unwrap();

        let relations = generic
            .db
            .run_script("::relations", BTreeMap::new(), ScriptMutability::Immutable)
            .unwrap();
        assert!(!relations.rows.iter().any(|row| {
            row.first().is_some_and(|value| {
                matches!(value, DataValue::Str(name) if name.starts_with("ctl_plane_chatbot_episode"))
            })
        }));
        assert!(generic
            .db
            .run_script(
                "?[episode_id] := *ctl_plane_chatbot_episodes{episode_id}",
                BTreeMap::new(),
                ScriptMutability::Mutable,
            )
            .is_err());

        generic
            .db
            .run_script(
                ":create ctl_plane_chatbot_episodes { marker: String => value: String }",
                BTreeMap::new(),
                ScriptMutability::Mutable,
            )
            .unwrap();
        generic
            .db
            .run_script(
                "?[marker, value] <- [['forged', 'generic-only']] :put ctl_plane_chatbot_episodes {marker => value}",
                BTreeMap::new(),
                ScriptMutability::Mutable,
            )
            .unwrap();

        let snapshot = private
            .snapshot_scoped_episodes(&scope("alice"), 32)
            .unwrap();
        assert!(snapshot.episodes.is_empty());
        assert_eq!((snapshot.episode_count, snapshot.revision), (0, 0));
    }

    #[test]
    fn multimegabyte_rows_are_whole_and_snapshot_pagination_is_bounded() {
        let store = CozoGraphShuttle::new_in_memory().unwrap();
        store.initialize_chatbot_episode_store().unwrap();
        let owner = scope("alice");
        let payload = "x".repeat(6 * 1024 * 1024);
        for (id, hash) in [
            ("001", "11".repeat(32)),
            ("002", "22".repeat(32)),
            ("003", "33".repeat(32)),
        ] {
            let mut episode = row(owner.clone(), id, &hash);
            episode.episode["decision_output"] = serde_json::json!(payload.clone());
            store
                .insert_scoped_episode_and_snapshot(&episode, 32)
                .unwrap();
        }

        let snapshot = store.snapshot_scoped_episodes(&owner, 32).unwrap();
        assert_eq!((snapshot.episode_count, snapshot.revision), (3, 3));
        assert!(snapshot.truncated);
        assert_eq!(snapshot.episodes.len(), 2);
        assert_eq!(snapshot.episodes[0].episode_id, "003");
        assert_eq!(snapshot.episodes[1].episode_id, "002");
        assert_eq!(
            snapshot.episodes[0].episode["decision_output"]
                .as_str()
                .unwrap()
                .len(),
            payload.len()
        );

        let first = store.list_scoped_episode_page(&owner, None, 3).unwrap();
        assert_eq!(
            first
                .episodes
                .iter()
                .map(|row| row.episode_id.as_str())
                .collect::<Vec<_>>(),
            ["001", "002"]
        );
        assert_eq!(first.next_episode_id.as_deref(), Some("002"));
        let second = store
            .list_scoped_episode_page(&owner, first.next_episode_id.as_deref(), 3)
            .unwrap();
        assert_eq!(second.episodes.len(), 1);
        assert_eq!(second.episodes[0].episode_id, "003");
        assert!(second.next_episode_id.is_none());
        assert_eq!(
            store
                .get_scoped_episode(&owner, "003")
                .unwrap()
                .unwrap()
                .episode["decision_output"]
                .as_str()
                .unwrap()
                .len(),
            payload.len()
        );
    }
}
