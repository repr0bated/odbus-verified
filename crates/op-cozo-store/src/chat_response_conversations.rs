//! Durable private `chat_response` conversations in a dedicated Cozo domain
//! owner.
//!
//! The scope key is the asking human's principal only, so a conversation
//! spans logins. The chain fields of every row (`turn_index`,
//! `prev_turn_hash`, `turn_hash`, `content_hash`, the conversation head) are
//! columns, and each stored JSON document must agree with them. The plugin
//! owns the digests and the typed contract; this owner enforces order:
//! a turn is appended only at the conversation's current head, once.

use super::identity_transactions::commit_checked;
use super::*;
use cozo::MultiTransaction;

const MAX_LIST_LIMIT: usize = 200;
const MAX_PRINCIPAL_BYTES: usize = 512;
const MAX_ID_BYTES: usize = 128;
/// Largest stored turn document (typed turn, content included).
pub const MAX_CHAT_TURN_BYTES: usize = 4 * 1024 * 1024;
/// Largest stored conversation document.
pub const MAX_CHAT_CONVERSATION_BYTES: usize = 64 * 1024;
/// Byte budget of one snapshot's or page's turn documents.
pub const MAX_CHAT_TURN_WINDOW_BYTES: usize = 12 * 1024 * 1024;
const MAX_CONVERSATION_WINDOW_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq)]
pub struct ChatConversationRecord {
    pub principal_id: String,
    pub conversation_id: String,
    pub turn_count: u64,
    pub head_turn_hash: String,
    /// Scope revision of the newest write to this conversation.
    pub revision: u64,
    pub conversation: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChatTurnRecord {
    pub principal_id: String,
    pub conversation_id: String,
    pub turn_index: u64,
    pub turn_id: String,
    pub prev_turn_hash: String,
    pub turn_hash: String,
    pub content_hash: String,
    /// Scope revision that committed this turn.
    pub revision: u64,
    pub turn: Value,
}

/// The document a turn commit produces from the durable head it is given.
#[derive(Debug, Clone, PartialEq)]
pub struct ChatTurnDraft {
    pub turn: Value,
    pub turn_hash: String,
    pub content_hash: String,
    /// The conversation document advanced to this turn.
    pub conversation: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChatResponseSnapshot {
    /// Most recently active first.
    pub conversations: Vec<ChatConversationRecord>,
    pub conversation_count: u64,
    /// Newest commit first.
    pub turns: Vec<ChatTurnRecord>,
    pub turn_count: u64,
    pub revision: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChatTurnPage {
    /// Ascending `turn_index`: the newest bounded window.
    pub turns: Vec<ChatTurnRecord>,
    /// True when turns older than the first returned one exist.
    pub truncated: bool,
}

fn error(message: &str) -> CozoError {
    CozoError::Other(message.into())
}

fn bounded(value: &str, maximum: usize) -> bool {
    !value.is_empty()
        && value.len() <= maximum
        && !value.chars().any(char::is_control)
        && value.trim() == value
}

fn validate_principal(principal_id: &str) -> std::result::Result<(), CozoError> {
    if !bounded(principal_id, MAX_PRINCIPAL_BYTES) {
        return Err(error("invalid chat_response principal scope"));
    }
    Ok(())
}

fn validate_id(value: &str) -> std::result::Result<(), CozoError> {
    if !bounded(value, MAX_ID_BYTES) {
        return Err(error("invalid chat_response id"));
    }
    Ok(())
}

fn is_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn int(value: u64, what: &str) -> std::result::Result<DataValue, CozoError> {
    Ok(DataValue::Num(Num::Int(i64::try_from(value).map_err(
        |_| CozoError::Other(format!("chat_response {what} exhausted")),
    )?)))
}

fn validate_conversation(row: &ChatConversationRecord) -> std::result::Result<(), CozoError> {
    validate_principal(&row.principal_id)?;
    validate_id(&row.conversation_id)?;
    if !is_hash(&row.head_turn_hash)
        || serde_json::to_vec(&row.conversation)?.len() > MAX_CHAT_CONVERSATION_BYTES
    {
        return Err(error("invalid chat_response conversation document"));
    }
    let object = row
        .conversation
        .as_object()
        .ok_or_else(|| error("invalid chat_response conversation document"))?;
    let text =
        |field: &str, expected: &str| object.get(field).and_then(Value::as_str) == Some(expected);
    if !text("principal_id", &row.principal_id)
        || !text("conversation_id", &row.conversation_id)
        || !text("head_turn_hash", &row.head_turn_hash)
        || object.get("turn_count").and_then(Value::as_u64) != Some(row.turn_count)
    {
        return Err(error(
            "chat_response conversation document differs from its durable key",
        ));
    }
    Ok(())
}

fn validate_turn(row: &ChatTurnRecord) -> std::result::Result<(), CozoError> {
    validate_principal(&row.principal_id)?;
    validate_id(&row.conversation_id)?;
    validate_id(&row.turn_id)?;
    if !is_hash(&row.prev_turn_hash)
        || !is_hash(&row.turn_hash)
        || !is_hash(&row.content_hash)
        || serde_json::to_vec(&row.turn)?.len() > MAX_CHAT_TURN_BYTES
    {
        return Err(error("invalid chat_response turn document"));
    }
    let object = row
        .turn
        .as_object()
        .ok_or_else(|| error("invalid chat_response turn document"))?;
    let text =
        |field: &str, expected: &str| object.get(field).and_then(Value::as_str) == Some(expected);
    if !text("principal_id", &row.principal_id)
        || !text("conversation_id", &row.conversation_id)
        || !text("turn_id", &row.turn_id)
        || !text("prev_turn_hash", &row.prev_turn_hash)
        || !text("turn_hash", &row.turn_hash)
        || !text("content_hash", &row.content_hash)
        || object.get("turn_index").and_then(Value::as_u64) != Some(row.turn_index)
    {
        return Err(error(
            "chat_response turn document differs from its durable key",
        ));
    }
    Ok(())
}

fn principal_params(principal_id: &str) -> Params {
    BTreeMap::from([("principal".into(), DataValue::Str(principal_id.into()))])
}

fn as_u64(value: &DataValue) -> std::result::Result<u64, CozoError> {
    match value {
        DataValue::Num(Num::Int(value)) if *value >= 0 => Ok(*value as u64),
        _ => Err(error("corrupt chat_response integer column")),
    }
}

const CONVERSATION_COLUMNS: &str =
    "conversation_id, turn_count, head_turn_hash, revision, conversation_json";
const TURN_COLUMNS: &str = "conversation_id, turn_index, turn_id, prev_turn_hash, turn_hash, content_hash, revision, turn_json";

fn read_conversations(
    tx: &MultiTransaction,
    script: &str,
    values: Params,
    principal_id: &str,
) -> std::result::Result<Vec<ChatConversationRecord>, CozoError> {
    let rows = tx
        .run_script(script, values)
        .map_err(|error| CozoError::Other(format!("conversation read failed: {error}")))?;
    rows.rows
        .into_iter()
        .map(|row| {
            let [
                DataValue::Str(conversation_id),
                turn_count,
                DataValue::Str(head_turn_hash),
                revision,
                DataValue::Str(conversation_json),
            ] = row.as_slice()
            else {
                return Err(error("corrupt chat_response conversation row"));
            };
            let record = ChatConversationRecord {
                principal_id: principal_id.to_string(),
                conversation_id: conversation_id.to_string(),
                turn_count: as_u64(turn_count)?,
                head_turn_hash: head_turn_hash.to_string(),
                revision: as_u64(revision)?,
                conversation: serde_json::from_str(conversation_json)
                    .map_err(|_| error("corrupt chat_response conversation JSON"))?,
            };
            validate_conversation(&record)?;
            Ok(record)
        })
        .collect()
}

fn read_turns(
    tx: &MultiTransaction,
    script: &str,
    values: Params,
    principal_id: &str,
) -> std::result::Result<Vec<ChatTurnRecord>, CozoError> {
    let rows = tx
        .run_script(script, values)
        .map_err(|error| CozoError::Other(format!("turn read failed: {error}")))?;
    rows.rows
        .into_iter()
        .map(|row| {
            let [
                DataValue::Str(conversation_id),
                turn_index,
                DataValue::Str(turn_id),
                DataValue::Str(prev_turn_hash),
                DataValue::Str(turn_hash),
                DataValue::Str(content_hash),
                revision,
                DataValue::Str(turn_json),
            ] = row.as_slice()
            else {
                return Err(error("corrupt chat_response turn row"));
            };
            let record = ChatTurnRecord {
                principal_id: principal_id.to_string(),
                conversation_id: conversation_id.to_string(),
                turn_index: as_u64(turn_index)?,
                turn_id: turn_id.to_string(),
                prev_turn_hash: prev_turn_hash.to_string(),
                turn_hash: turn_hash.to_string(),
                content_hash: content_hash.to_string(),
                revision: as_u64(revision)?,
                turn: serde_json::from_str(turn_json)
                    .map_err(|_| error("corrupt chat_response turn JSON"))?,
            };
            validate_turn(&record)?;
            Ok(record)
        })
        .collect()
}

fn read_scope(
    tx: &MultiTransaction,
    principal_id: &str,
) -> std::result::Result<(u64, u64, u64), CozoError> {
    let rows = tx
        .run_script(
            "?[revision, conversation_count, turn_count] := *chat_response_scopes{principal_id, revision, conversation_count, turn_count}, principal_id = $principal",
            principal_params(principal_id),
        )
        .map_err(|error| CozoError::Other(format!("chat scope read failed: {error}")))?;
    match rows.rows.as_slice() {
        [] => Ok((0, 0, 0)),
        [row] => match row.as_slice() {
            [revision, conversations, turns] => {
                Ok((as_u64(revision)?, as_u64(conversations)?, as_u64(turns)?))
            }
            _ => Err(error("corrupt chat_response scope row")),
        },
        _ => Err(error("corrupt chat_response scope row")),
    }
}

fn write_scope(
    tx: &MultiTransaction,
    principal_id: &str,
    revision: u64,
    conversations: u64,
    turns: u64,
) -> std::result::Result<(), CozoError> {
    let mut values = principal_params(principal_id);
    values.insert("revision".into(), int(revision, "revision")?);
    values.insert(
        "conversations".into(),
        int(conversations, "conversation count")?,
    );
    values.insert("turns".into(), int(turns, "turn count")?);
    tx.run_script(
        "?[principal_id, revision, conversation_count, turn_count] <- [[$principal, $revision, $conversations, $turns]] :put chat_response_scopes {principal_id => revision, conversation_count, turn_count}",
        values,
    )
    .map_err(|error| CozoError::Other(format!("chat scope write failed: {error}")))?;
    Ok(())
}

fn write_conversation(
    tx: &MultiTransaction,
    row: &ChatConversationRecord,
) -> std::result::Result<(), CozoError> {
    validate_conversation(row)?;
    let mut values = principal_params(&row.principal_id);
    values.insert(
        "conversation".into(),
        DataValue::Str(row.conversation_id.as_str().into()),
    );
    values.insert("turns".into(), int(row.turn_count, "turn count")?);
    values.insert(
        "head".into(),
        DataValue::Str(row.head_turn_hash.as_str().into()),
    );
    values.insert("revision".into(), int(row.revision, "revision")?);
    values.insert(
        "document".into(),
        DataValue::Str(serde_json::to_string(&row.conversation)?.into()),
    );
    tx.run_script(
        "?[principal_id, conversation_id, turn_count, head_turn_hash, revision, conversation_json] <- [[$principal, $conversation, $turns, $head, $revision, $document]] :put chat_response_conversations {principal_id, conversation_id => turn_count, head_turn_hash, revision, conversation_json}",
        values,
    )
    .map_err(|error| CozoError::Other(format!("conversation write failed: {error}")))?;
    Ok(())
}

fn get_conversation(
    tx: &MultiTransaction,
    principal_id: &str,
    conversation_id: &str,
) -> std::result::Result<Option<ChatConversationRecord>, CozoError> {
    let mut values = principal_params(principal_id);
    values.insert(
        "conversation".into(),
        DataValue::Str(conversation_id.into()),
    );
    let mut rows = read_conversations(
        tx,
        &format!(
            "?[{CONVERSATION_COLUMNS}] := *chat_response_conversations{{principal_id, {CONVERSATION_COLUMNS}}}, principal_id = $principal, conversation_id = $conversation"
        ),
        values,
        principal_id,
    )?;
    match rows.len() {
        0 => Ok(None),
        1 => Ok(rows.pop()),
        _ => Err(error("duplicate chat_response conversation rows")),
    }
}

fn turn_location(
    tx: &MultiTransaction,
    principal_id: &str,
    turn_id: &str,
) -> std::result::Result<Option<(String, u64)>, CozoError> {
    let mut values = principal_params(principal_id);
    values.insert("turn".into(), DataValue::Str(turn_id.into()));
    let rows = tx
        .run_script(
            "?[conversation_id, turn_index] := *chat_response_turn_ids{principal_id, turn_id, conversation_id, turn_index}, principal_id = $principal, turn_id = $turn",
            values,
        )
        .map_err(|error| CozoError::Other(format!("turn id read failed: {error}")))?;
    match rows.rows.as_slice() {
        [] => Ok(None),
        [row] => match row.as_slice() {
            [DataValue::Str(conversation_id), index] => {
                Ok(Some((conversation_id.to_string(), as_u64(index)?)))
            }
            _ => Err(error("corrupt chat_response turn id row")),
        },
        _ => Err(error("duplicate chat_response turn id rows")),
    }
}

fn snapshot_in_transaction(
    tx: &MultiTransaction,
    principal_id: &str,
    conversation_limit: usize,
    turn_limit: usize,
) -> std::result::Result<ChatResponseSnapshot, CozoError> {
    if !(1..=MAX_LIST_LIMIT).contains(&conversation_limit)
        || !(1..=MAX_LIST_LIMIT).contains(&turn_limit)
    {
        return Err(error("chat_response snapshot limits must be 1..=200"));
    }
    let (revision, conversation_count, turn_count) = read_scope(tx, principal_id)?;
    let mut conversations = Vec::new();
    let mut bytes = 0usize;
    for row in read_conversations(
        tx,
        &format!(
            "?[{CONVERSATION_COLUMNS}] := *chat_response_conversations{{principal_id, {CONVERSATION_COLUMNS}}}, principal_id = $principal :order -revision :limit {conversation_limit}"
        ),
        principal_params(principal_id),
        principal_id,
    )? {
        let row_bytes =
            serde_json::to_vec(&row.conversation)?.len() + row.conversation_id.len() + 4;
        if bytes.saturating_add(row_bytes) > MAX_CONVERSATION_WINDOW_BYTES {
            break;
        }
        bytes += row_bytes;
        conversations.push(row);
    }
    let mut turns = Vec::new();
    let mut bytes = 0usize;
    for row in read_turns(
        tx,
        &format!(
            "?[{TURN_COLUMNS}] := *chat_response_turns{{principal_id, {TURN_COLUMNS}}}, principal_id = $principal :order -revision :limit {turn_limit}"
        ),
        principal_params(principal_id),
        principal_id,
    )? {
        let row_bytes = serde_json::to_vec(&row.turn)?.len() + row.turn_id.len() + 4;
        if bytes.saturating_add(row_bytes) > MAX_CHAT_TURN_WINDOW_BYTES {
            break;
        }
        bytes += row_bytes;
        turns.push(row);
    }
    Ok(ChatResponseSnapshot {
        conversations,
        conversation_count,
        turns,
        turn_count,
        revision,
    })
}

/// The newest `limit` turns of one conversation, ascending, byte bounded.
fn turn_page(
    tx: &MultiTransaction,
    principal_id: &str,
    conversation_id: &str,
    limit: usize,
) -> std::result::Result<ChatTurnPage, CozoError> {
    if !(1..=MAX_LIST_LIMIT).contains(&limit) {
        return Err(error("chat_response turn page limit must be 1..=200"));
    }
    let mut values = principal_params(principal_id);
    values.insert(
        "conversation".into(),
        DataValue::Str(conversation_id.into()),
    );
    let candidates = read_turns(
        tx,
        &format!(
            "?[{TURN_COLUMNS}] := *chat_response_turns{{principal_id, {TURN_COLUMNS}}}, principal_id = $principal, conversation_id = $conversation :order -turn_index :limit {limit}"
        ),
        values,
        principal_id,
    )?;
    let mut turns = Vec::with_capacity(candidates.len());
    let mut bytes = 0usize;
    for row in candidates {
        let row_bytes = serde_json::to_vec(&row.turn)?.len();
        if bytes.saturating_add(row_bytes) > MAX_CHAT_TURN_WINDOW_BYTES {
            break;
        }
        bytes += row_bytes;
        turns.push(row);
    }
    turns.reverse();
    let truncated = turns.first().is_some_and(|first| first.turn_index > 0);
    Ok(ChatTurnPage { turns, truncated })
}

impl CozoGraphShuttle {
    /// Opt this handle into the private chat_response domain. Generic Cozo
    /// stores deliberately do not expose these relations.
    pub fn initialize_chat_response_store(&self) -> std::result::Result<(), CozoError> {
        let _guard = self
            .episode_transaction
            .lock()
            .map_err(|_| error("chat_response owner is poisoned"))?;
        self.runtime_document_preflight()?;
        for relation in [
            r#":create chat_response_scopes {
                principal_id: String
                => revision: Int, conversation_count: Int, turn_count: Int
            }"#,
            r#":create chat_response_conversations {
                principal_id: String, conversation_id: String
                =>
                turn_count: Int,
                head_turn_hash: String,
                revision: Int,
                conversation_json: String
            }"#,
            r#":create chat_response_turns {
                principal_id: String, conversation_id: String, turn_index: Int
                =>
                turn_id: String,
                prev_turn_hash: String,
                turn_hash: String,
                content_hash: String,
                revision: Int,
                turn_json: String
            }"#,
            r#":create chat_response_turn_ids {
                principal_id: String, turn_id: String
                => conversation_id: String, turn_index: Int
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
                        "chat_response relation initialization failed: {message}"
                    )));
                }
            }
        }
        for probe in [
            "?[principal_id, revision, conversation_count, turn_count] := *chat_response_scopes{principal_id, revision, conversation_count, turn_count} :limit 0",
            "?[principal_id, conversation_id, turn_count, head_turn_hash, revision, conversation_json] := *chat_response_conversations{principal_id, conversation_id, turn_count, head_turn_hash, revision, conversation_json} :limit 0",
            "?[principal_id, conversation_id, turn_index, turn_id, prev_turn_hash, turn_hash, content_hash, revision, turn_json] := *chat_response_turns{principal_id, conversation_id, turn_index, turn_id, prev_turn_hash, turn_hash, content_hash, revision, turn_json} :limit 0",
            "?[principal_id, turn_id, conversation_id, turn_index] := *chat_response_turn_ids{principal_id, turn_id, conversation_id, turn_index} :limit 0",
        ] {
            self.db
                .run_script(probe, BTreeMap::new(), ScriptMutability::Immutable)
                .map_err(|error| {
                    CozoError::Other(format!(
                        "chat_response relation verification failed: {error}"
                    ))
                })?;
        }
        self.sync_runtime_document_store()
    }

    /// Create an empty conversation (turn_count 0, head = its genesis) in its
    /// principal scope. The id must be new.
    pub fn insert_chat_conversation_and_snapshot(
        &self,
        conversation: &ChatConversationRecord,
        conversation_limit: usize,
        turn_limit: usize,
    ) -> std::result::Result<(ChatConversationRecord, ChatResponseSnapshot), CozoError> {
        validate_conversation(conversation)?;
        if conversation.turn_count != 0 {
            return Err(error("a new chat_response conversation has no turns"));
        }
        let _guard = self
            .episode_transaction
            .lock()
            .map_err(|_| error("chat_response owner is poisoned"))?;
        self.runtime_document_preflight()?;
        let tx = self.db.multi_transaction(true);
        if get_conversation(
            &tx,
            &conversation.principal_id,
            &conversation.conversation_id,
        )?
        .is_some()
        {
            return Err(error("chat_response conversation already exists"));
        }
        let (revision, conversations, turns) = read_scope(&tx, &conversation.principal_id)?;
        let revision = revision
            .checked_add(1)
            .ok_or_else(|| error("chat_response revision exhausted"))?;
        let conversations = conversations
            .checked_add(1)
            .ok_or_else(|| error("chat_response conversation count exhausted"))?;
        let mut row = conversation.clone();
        row.revision = revision;
        write_conversation(&tx, &row)?;
        write_scope(&tx, &row.principal_id, revision, conversations, turns)?;
        let snapshot =
            snapshot_in_transaction(&tx, &row.principal_id, conversation_limit, turn_limit)?;
        commit_checked(&tx)?;
        self.sync_runtime_document_store()?;
        Ok((row, snapshot))
    }

    /// Append one turn at the conversation's durable head. `expected_turn_index`
    /// must still be the head (`turn_count`), and `turn_id` must be new in the
    /// principal scope: exactly one response per turn. `build` receives the
    /// durable conversation and the revision this commit will take, and
    /// returns the turn chained to that head and the advanced conversation.
    #[allow(clippy::type_complexity)]
    #[allow(clippy::too_many_arguments)]
    pub fn insert_chat_turn_and_snapshot(
        &self,
        principal_id: &str,
        conversation_id: &str,
        turn_id: &str,
        expected_turn_index: u64,
        build: &dyn Fn(
            &ChatConversationRecord,
            u64,
        ) -> std::result::Result<ChatTurnDraft, CozoError>,
        conversation_limit: usize,
        turn_limit: usize,
    ) -> std::result::Result<(ChatTurnRecord, ChatResponseSnapshot), CozoError> {
        validate_principal(principal_id)?;
        validate_id(conversation_id)?;
        validate_id(turn_id)?;
        let _guard = self
            .episode_transaction
            .lock()
            .map_err(|_| error("chat_response owner is poisoned"))?;
        self.runtime_document_preflight()?;
        let tx = self.db.multi_transaction(true);
        let conversation =
            get_conversation(&tx, principal_id, conversation_id)?.ok_or_else(|| {
                error("chat_response conversation does not exist in the principal scope")
            })?;
        if turn_location(&tx, principal_id, turn_id)?.is_some() {
            return Err(error("chat_response turn is already answered"));
        }
        if conversation.turn_count != expected_turn_index {
            return Err(error(
                "chat_response turn index is no longer the conversation head",
            ));
        }
        let (revision, conversations, turns) = read_scope(&tx, principal_id)?;
        let revision = revision
            .checked_add(1)
            .ok_or_else(|| error("chat_response revision exhausted"))?;
        let turns = turns
            .checked_add(1)
            .ok_or_else(|| error("chat_response turn count exhausted"))?;
        let draft = build(&conversation, revision)?;
        let record = ChatTurnRecord {
            principal_id: principal_id.to_string(),
            conversation_id: conversation_id.to_string(),
            turn_index: expected_turn_index,
            turn_id: turn_id.to_string(),
            prev_turn_hash: conversation.head_turn_hash.clone(),
            turn_hash: draft.turn_hash.clone(),
            content_hash: draft.content_hash.clone(),
            revision,
            turn: draft.turn,
        };
        validate_turn(&record)?;
        let advanced = ChatConversationRecord {
            principal_id: principal_id.to_string(),
            conversation_id: conversation_id.to_string(),
            turn_count: expected_turn_index
                .checked_add(1)
                .ok_or_else(|| error("chat_response turn index exhausted"))?,
            head_turn_hash: draft.turn_hash,
            revision,
            conversation: draft.conversation,
        };
        write_conversation(&tx, &advanced)?;
        let mut values = principal_params(principal_id);
        values.insert(
            "conversation".into(),
            DataValue::Str(conversation_id.into()),
        );
        values.insert("index".into(), int(record.turn_index, "turn index")?);
        values.insert("turn".into(), DataValue::Str(turn_id.into()));
        values.insert(
            "prev".into(),
            DataValue::Str(record.prev_turn_hash.as_str().into()),
        );
        values.insert(
            "hash".into(),
            DataValue::Str(record.turn_hash.as_str().into()),
        );
        values.insert(
            "content".into(),
            DataValue::Str(record.content_hash.as_str().into()),
        );
        values.insert("revision".into(), int(revision, "revision")?);
        values.insert(
            "document".into(),
            DataValue::Str(serde_json::to_string(&record.turn)?.into()),
        );
        tx.run_script(
            "?[principal_id, conversation_id, turn_index, turn_id, prev_turn_hash, turn_hash, content_hash, revision, turn_json] <- [[$principal, $conversation, $index, $turn, $prev, $hash, $content, $revision, $document]] :put chat_response_turns {principal_id, conversation_id, turn_index => turn_id, prev_turn_hash, turn_hash, content_hash, revision, turn_json}",
            values.clone(),
        )
        .map_err(|error| CozoError::Other(format!("turn write failed: {error}")))?;
        tx.run_script(
            "?[principal_id, turn_id, conversation_id, turn_index] <- [[$principal, $turn, $conversation, $index]] :put chat_response_turn_ids {principal_id, turn_id => conversation_id, turn_index}",
            values,
        )
        .map_err(|error| CozoError::Other(format!("turn id write failed: {error}")))?;
        write_scope(&tx, principal_id, revision, conversations, turns)?;
        let snapshot = snapshot_in_transaction(&tx, principal_id, conversation_limit, turn_limit)?;
        commit_checked(&tx)?;
        self.sync_runtime_document_store()?;
        Ok((record, snapshot))
    }

    pub fn get_chat_conversation(
        &self,
        principal_id: &str,
        conversation_id: &str,
    ) -> std::result::Result<Option<ChatConversationRecord>, CozoError> {
        validate_principal(principal_id)?;
        validate_id(conversation_id)?;
        let _guard = self
            .episode_transaction
            .lock()
            .map_err(|_| error("chat_response owner is poisoned"))?;
        let tx = self.db.multi_transaction(false);
        let row = get_conversation(&tx, principal_id, conversation_id)?;
        commit_checked(&tx)?;
        Ok(row)
    }

    /// The committed turn for `turn_id` in the principal scope, if any.
    pub fn get_chat_turn(
        &self,
        principal_id: &str,
        turn_id: &str,
    ) -> std::result::Result<Option<ChatTurnRecord>, CozoError> {
        validate_principal(principal_id)?;
        validate_id(turn_id)?;
        let _guard = self
            .episode_transaction
            .lock()
            .map_err(|_| error("chat_response owner is poisoned"))?;
        let tx = self.db.multi_transaction(false);
        let Some((conversation_id, index)) = turn_location(&tx, principal_id, turn_id)? else {
            commit_checked(&tx)?;
            return Ok(None);
        };
        let mut values = principal_params(principal_id);
        values.insert(
            "conversation".into(),
            DataValue::Str(conversation_id.as_str().into()),
        );
        values.insert("index".into(), int(index, "turn index")?);
        let mut rows = read_turns(
            &tx,
            &format!(
                "?[{TURN_COLUMNS}] := *chat_response_turns{{principal_id, {TURN_COLUMNS}}}, principal_id = $principal, conversation_id = $conversation, turn_index = $index"
            ),
            values,
            principal_id,
        )?;
        commit_checked(&tx)?;
        let row = rows
            .pop()
            .ok_or_else(|| error("chat_response turn id has no turn row"))?;
        if row.turn_id != turn_id || !rows.is_empty() {
            return Err(error("chat_response turn id index is inconsistent"));
        }
        Ok(Some(row))
    }

    /// Most recently active conversations first, with the scope counters.
    pub fn list_chat_conversations(
        &self,
        principal_id: &str,
        limit: usize,
    ) -> std::result::Result<ChatResponseSnapshot, CozoError> {
        validate_principal(principal_id)?;
        let _guard = self
            .episode_transaction
            .lock()
            .map_err(|_| error("chat_response owner is poisoned"))?;
        let tx = self.db.multi_transaction(false);
        let snapshot = snapshot_in_transaction(&tx, principal_id, limit, 1)?;
        commit_checked(&tx)?;
        Ok(ChatResponseSnapshot {
            turns: Vec::new(),
            ..snapshot
        })
    }

    pub fn count_principal_conversations(
        &self,
        principal_id: &str,
    ) -> std::result::Result<usize, CozoError> {
        validate_principal(principal_id)?;
        let _guard = self
            .episode_transaction
            .lock()
            .map_err(|_| error("chat_response owner is poisoned"))?;
        let tx = self.db.multi_transaction(false);
        let rows = tx
            .run_script(
                "?[cnt] := chat_conversation{principal_id: $principal_id, _}, cnt :=count",
                principal_params(principal_id),
            )
            .map_err(|e| error(&format!("count conversations query failed: {}", e)))?;
        if let Some(row) = rows.rows.first() {
            if let Some(DataValue::Int(count)) = row.first() {
                return Ok(*count as usize);
            }
        }
        Ok(0)
    }

    /// The newest `limit` turns of one conversation, ascending, byte bounded.
    pub fn list_chat_turns(
        &self,
        principal_id: &str,
        conversation_id: &str,
        limit: usize,
    ) -> std::result::Result<ChatTurnPage, CozoError> {
        validate_principal(principal_id)?;
        validate_id(conversation_id)?;
        let _guard = self
            .episode_transaction
            .lock()
            .map_err(|_| error("chat_response owner is poisoned"))?;
        let tx = self.db.multi_transaction(false);
        let page = turn_page(&tx, principal_id, conversation_id, limit)?;
        commit_checked(&tx)?;
        Ok(page)
    }

    /// One conversation and its newest turn window, read in one transaction
    /// so the page and the head it is checked against agree.
    pub fn get_chat_conversation_with_turns(
        &self,
        principal_id: &str,
        conversation_id: &str,
        limit: usize,
    ) -> std::result::Result<Option<(ChatConversationRecord, ChatTurnPage)>, CozoError> {
        validate_principal(principal_id)?;
        validate_id(conversation_id)?;
        let _guard = self
            .episode_transaction
            .lock()
            .map_err(|_| error("chat_response owner is poisoned"))?;
        let tx = self.db.multi_transaction(false);
        let Some(conversation) = get_conversation(&tx, principal_id, conversation_id)? else {
            commit_checked(&tx)?;
            return Ok(None);
        };
        let page = turn_page(&tx, principal_id, conversation_id, limit)?;
        commit_checked(&tx)?;
        Ok(Some((conversation, page)))
    }

    pub fn snapshot_chat_responses(
        &self,
        principal_id: &str,
        conversation_limit: usize,
        turn_limit: usize,
    ) -> std::result::Result<ChatResponseSnapshot, CozoError> {
        validate_principal(principal_id)?;
        let _guard = self
            .episode_transaction
            .lock()
            .map_err(|_| error("chat_response owner is poisoned"))?;
        let tx = self.db.multi_transaction(false);
        let snapshot = snapshot_in_transaction(&tx, principal_id, conversation_limit, turn_limit)?;
        commit_checked(&tx)?;
        Ok(snapshot)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hash(byte: char) -> String {
        byte.to_string().repeat(64)
    }

    fn conversation(principal: &str, id: &str) -> ChatConversationRecord {
        ChatConversationRecord {
            principal_id: principal.into(),
            conversation_id: id.into(),
            turn_count: 0,
            head_turn_hash: hash('0'),
            revision: 0,
            conversation: serde_json::json!({
                "principal_id": principal, "conversation_id": id,
                "turn_count": 0, "head_turn_hash": hash('0'),
            }),
        }
    }

    fn draft(
        principal: &str,
        turn_id: &str,
        turn_hash: String,
    ) -> impl Fn(&ChatConversationRecord, u64) -> std::result::Result<ChatTurnDraft, CozoError>
    {
        let principal = principal.to_string();
        let turn_id = turn_id.to_string();
        move |head, _revision| {
            Ok(ChatTurnDraft {
                turn: serde_json::json!({
                    "principal_id": principal, "conversation_id": head.conversation_id,
                    "turn_id": turn_id, "turn_index": head.turn_count,
                    "prev_turn_hash": head.head_turn_hash, "turn_hash": turn_hash,
                    "content_hash": "c".repeat(64), "content": "private",
                }),
                turn_hash: turn_hash.clone(),
                content_hash: "c".repeat(64),
                conversation: serde_json::json!({
                    "principal_id": principal, "conversation_id": head.conversation_id,
                    "turn_count": head.turn_count + 1, "head_turn_hash": turn_hash,
                }),
            })
        }
    }

    #[test]
    fn turns_append_only_at_the_head_once_and_survive_a_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("chat");
        {
            let store = CozoGraphShuttle::new_persistent(path.clone()).unwrap();
            store.initialize_chat_response_store().unwrap();
            let (created, snapshot) = store
                .insert_chat_conversation_and_snapshot(&conversation("alice", "c1"), 8, 8)
                .unwrap();
            assert_eq!((created.revision, snapshot.conversation_count), (1, 1));
            assert!(
                store
                    .insert_chat_conversation_and_snapshot(&conversation("alice", "c1"), 8, 8)
                    .is_err(),
                "conversation ids are unique per principal"
            );
            let (first, _) = store
                .insert_chat_turn_and_snapshot(
                    "alice",
                    "c1",
                    "t1",
                    0,
                    &draft("alice", "t1", hash('1')),
                    8,
                    8,
                )
                .unwrap();
            assert_eq!((first.turn_index, first.revision), (0, 2));
            assert_eq!(first.prev_turn_hash, hash('0'));
            // The same turn cannot be answered twice, even at the new head.
            assert!(store
                .insert_chat_turn_and_snapshot(
                    "alice",
                    "c1",
                    "t1",
                    1,
                    &draft("alice", "t1", hash('2')),
                    8,
                    8,
                )
                .is_err());
            // A stale index (not the head) is refused.
            assert!(store
                .insert_chat_turn_and_snapshot(
                    "alice",
                    "c1",
                    "t2",
                    0,
                    &draft("alice", "t2", hash('2')),
                    8,
                    8,
                )
                .is_err());
            // A draft that disagrees with the head it was given is refused.
            let wrong_prev = |head: &ChatConversationRecord, revision| {
                let mut draft = draft("alice", "t2", hash('2'))(head, revision)?;
                draft.turn["prev_turn_hash"] = serde_json::json!(hash('9'));
                Ok(draft)
            };
            assert!(store
                .insert_chat_turn_and_snapshot("alice", "c1", "t2", 1, &wrong_prev, 8, 8)
                .is_err());
            let (second, snapshot) = store
                .insert_chat_turn_and_snapshot(
                    "alice",
                    "c1",
                    "t2",
                    1,
                    &draft("alice", "t2", hash('2')),
                    8,
                    8,
                )
                .unwrap();
            assert_eq!(second.prev_turn_hash, first.turn_hash);
            assert_eq!((snapshot.turn_count, snapshot.revision), (2, 3));
            assert_eq!(snapshot.turns[0].turn_id, "t2", "newest commit first");
            assert_eq!(snapshot.conversations[0].head_turn_hash, hash('2'));
        }
        let store = CozoGraphShuttle::new_persistent(path).unwrap();
        store.initialize_chat_response_store().unwrap();
        let turn = store.get_chat_turn("alice", "t1").unwrap().unwrap();
        assert_eq!(turn.turn_hash, hash('1'));
        let page = store.list_chat_turns("alice", "c1", 1).unwrap();
        assert_eq!(page.turns.len(), 1);
        assert_eq!(page.turns[0].turn_id, "t2");
        assert!(page.truncated);
        let page = store.list_chat_turns("alice", "c1", 10).unwrap();
        assert_eq!(
            page.turns.iter().map(|t| t.turn_index).collect::<Vec<_>>(),
            [0, 1]
        );
        assert!(!page.truncated);
        let (head, window) = store
            .get_chat_conversation_with_turns("alice", "c1", 10)
            .unwrap()
            .unwrap();
        assert_eq!((head.turn_count, window.turns.len()), (2, 2));
        assert_eq!(window.turns[1].turn_hash, head.head_turn_hash);
        assert!(store
            .get_chat_conversation_with_turns("bob", "c1", 10)
            .unwrap()
            .is_none());
        let listed = store.list_chat_conversations("alice", 10).unwrap();
        assert_eq!(listed.conversation_count, 1);
        assert_eq!(listed.conversations[0].turn_count, 2);
    }

    #[test]
    fn principals_are_isolated_scopes() {
        let store = CozoGraphShuttle::new_in_memory().unwrap();
        store.initialize_chat_response_store().unwrap();
        store
            .insert_chat_conversation_and_snapshot(&conversation("alice", "c1"), 8, 8)
            .unwrap();
        store
            .insert_chat_turn_and_snapshot(
                "alice",
                "c1",
                "t1",
                0,
                &draft("alice", "t1", hash('1')),
                8,
                8,
            )
            .unwrap();
        assert!(store.get_chat_conversation("bob", "c1").unwrap().is_none());
        assert!(store.get_chat_turn("bob", "t1").unwrap().is_none());
        assert!(store
            .list_chat_turns("bob", "c1", 10)
            .unwrap()
            .turns
            .is_empty());
        let bob = store.snapshot_chat_responses("bob", 8, 8).unwrap();
        assert_eq!(
            (bob.conversation_count, bob.turn_count, bob.revision),
            (0, 0, 0)
        );
        // Bob cannot append into Alice's conversation through his own scope.
        assert!(store
            .insert_chat_turn_and_snapshot(
                "bob",
                "c1",
                "t9",
                1,
                &draft("bob", "t9", hash('9')),
                8,
                8,
            )
            .is_err());
        // A draft naming another principal is refused by the key check.
        assert!(store
            .insert_chat_turn_and_snapshot(
                "alice",
                "c1",
                "t9",
                1,
                &draft("bob", "t9", hash('9')),
                8,
                8,
            )
            .is_err());
    }

    #[test]
    fn generic_store_has_no_chat_relations() {
        let generic = CozoGraphShuttle::new_in_memory().unwrap();
        let relations = generic
            .db
            .run_script("::relations", BTreeMap::new(), ScriptMutability::Immutable)
            .unwrap();
        assert!(!relations.rows.iter().any(|row| {
            row.first().is_some_and(
                |value| matches!(value, DataValue::Str(name) if name.starts_with("chat_response")),
            )
        }));
    }
}
