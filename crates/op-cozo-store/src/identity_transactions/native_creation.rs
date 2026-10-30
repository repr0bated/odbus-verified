//! Persistent allocation of human-facing Incus names, not identity authority.
//! Never infer the counter from provider names. Creation intents fence POST
//! retries only; identity records and their UUIDs own subsequent lifecycle.
use super::*;

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeCreationRequest {
    pub operation_id: String,
    pub actor_id: String,
    pub caller_session: String,
    pub caller_genesis: String,
    pub owner_kind: String,
    pub logical_target: String,
    pub schema_hash: String,
    pub request_digest: String,
    pub request_json: String,
    /// Backend-selected singleton exception; never a caller-selected label.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reserved_name: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeCreationRecord {
    pub request: NativeCreationRequest,
    pub number: i64,
    pub name: String,
    pub phase: String,
    pub native_uuid: String,
}

pub fn is_numbered_native_name(name: &str) -> bool {
    name.strip_prefix("ID-").is_some_and(|number| {
        number.parse::<i64>().is_ok_and(|n| n > 0 && n.to_string() == number)
    })
}

fn is_chatbot_reservation(request: &NativeCreationRequest) -> bool {
    request.reserved_name.as_deref() == Some("3tchedCtlChatbot")
        && request.owner_kind == "identity"
        && request.logical_target == "b0c777e3-0955-cd8b-3d01-ca12998cae28"
}

fn records(tx: &MultiTransaction) -> std::result::Result<Vec<NativeCreationRecord>, CozoError> {
    query(tx, "?[document,operation_id] := *native_container_creations{document,operation_id}", Params::new())?
        .rows.iter().map(|row| {
            let record: NativeCreationRecord = serde_json::from_str(&string(row, 0)?)?;
            if string(row, 1)? != record.request.operation_id
                || record.request.schema_hash.is_empty()
                || !valid_root_repair_digest(&record.request.request_digest)
                || (if record.request.reserved_name.is_some() {
                    !is_chatbot_reservation(&record.request)
                        || record.number != 0 || record.name != "3tchedCtlChatbot"
                } else {
                    record.number <= 0 || record.name != format!("ID-{}", record.number)
                })
                || !matches!(record.phase.as_str(), "intent" | "bound" | "complete")
                || (record.phase != "intent" && uuid::Uuid::parse_str(&record.native_uuid)
                    .map_or(true, |id| id.to_string() != record.native_uuid))
            {
                return Err(err("native creation journal is corrupt"));
            }
            Ok(record)
        }).collect()
}

fn put(tx: &MultiTransaction, record: &NativeCreationRecord) -> std::result::Result<(), CozoError> {
    query(tx, "?[operation_id,document] <- [[$op,$doc]] :put native_container_creations {operation_id => document}",
        BTreeMap::from([
            ("op".into(), DataValue::Str(record.request.operation_id.as_str().into())),
            ("doc".into(), DataValue::Str(serde_json::to_string(record)?.into())),
        ]))?;
    Ok(())
}

fn sequence(tx: &MultiTransaction, rows: &[NativeCreationRecord])
    -> std::result::Result<(i64, String), CozoError>
{
    let result = query(tx, "?[next,pending] := *native_container_sequence{key,next,pending}, key = 'default'", Params::new())?;
    let (next, pending) = match result.rows.as_slice() {
        [] if rows.is_empty() => {
            let marker = query(tx, "?[completed] := *schema_migrations{name,completed}, name = 'native-container-sequence-v1'", Params::new())?;
            if !marker.rows.is_empty() { return Err(err("native creation sequence is missing; refusing reset")); }
            (1, String::new())
        }
        [row] => (integer(row, 0)?, string(row, 1)?),
        _ => return Err(err("native creation sequence is missing or corrupt")),
    };
    // Container zero shares the durable POST fence but never advances ID-N.
    let special = rows.iter().filter(|r| is_chatbot_reservation(&r.request)).collect::<Vec<_>>();
    if special.len() > 1 || special.iter().any(|r| r.phase != "complete" && r.request.operation_id != pending) {
        return Err(err("reserved chatbot creation history or fence differs"));
    }
    let special_pending = special.iter().any(|r| r.phase != "complete" && r.request.operation_id == pending);
    let mut sorted = rows.iter().filter(|r| r.request.reserved_name.is_none()).collect::<Vec<_>>();
    sorted.sort_by_key(|r| r.number);
    let mut expected = 1i64;
    for row in sorted {
        if row.number != expected
            || (row.number < next && row.phase != "complete")
            || (row.number == next && (row.phase == "complete" || row.request.operation_id != pending))
            || row.number > next
        { return Err(err("native creation history is incomplete; refusing sequence recovery")); }
        expected = expected.checked_add(1).ok_or_else(|| err("native name sequence exhausted"))?;
    }
    if next < 1 || ((pending.is_empty() || special_pending) && expected != next)
        || (!pending.is_empty() && !special_pending && next.checked_add(1) != Some(expected))
    { return Err(err("native sequence and pending fence disagree")); }
    Ok((next, pending))
}

fn put_sequence(tx: &MultiTransaction, next: i64, pending: &str) -> std::result::Result<(), CozoError> {
    query(tx, "?[key,next,pending] <- [['default',$next,$pending]] :put native_container_sequence {key => next,pending}",
        BTreeMap::from([("next".into(), dv_int(next)), ("pending".into(), DataValue::Str(pending.into()))]))?;
    query(tx, "?[name,completed] <- [['native-container-sequence-v1',true]] :put schema_migrations {name => completed}", Params::new())?;
    Ok(())
}

impl CozoGraphShuttle {
    pub fn native_creation_records(&self) -> std::result::Result<Vec<NativeCreationRecord>, CozoError> {
        let _guard = self.identity_transaction.lock().map_err(|_| err("identity transaction owner is poisoned"))?;
        let tx = self.db.multi_transaction(false);
        let rows = records(&tx)?;
        sequence(&tx, &rows)?;
        commit_checked(&tx)?;
        Ok(rows)
    }

    /// `fresh` is the only permission to issue POST, returned after the barrier.
    /// Replays, including intent-only replays, may only read/reconcile.
    pub fn reserve_native_creation(&self, request: NativeCreationRequest, observed_names: &[String])
        -> std::result::Result<(NativeCreationRecord, bool), CozoError>
    {
        if self.identity_directory.is_none() { return Err(err("native allocation requires persistent Cozo")); }
        if uuid::Uuid::parse_str(&request.operation_id).map_or(true, |id| id.to_string() != request.operation_id)
            || request.actor_id.is_empty() || request.caller_session.is_empty() || request.caller_genesis.is_empty()
            || !matches!(request.owner_kind.as_str(), "identity" | "project" | "incus")
            || request.logical_target.is_empty() || request.schema_hash.is_empty() || !valid_root_repair_digest(&request.request_digest)
            || request.request_json.len() > 65536
            || (request.reserved_name.is_some() && !is_chatbot_reservation(&request))
        { return Err(err("invalid native creation intent")); }
        let _guard = self.identity_transaction.lock().map_err(|_| err("identity transaction owner is poisoned"))?;
        self.sync_identity_store()?;
        let tx = self.db.multi_transaction(true);
        let rows = records(&tx)?;
        let (next, pending) = sequence(&tx, &rows)?;
        // Existing labels do not acquire ownership or require a journal entry.
        // Only a collision with this allocation matters; never rename them or
        // reconstruct the persistent counter from a provider-side maximum.
        if let Some(row) = rows.iter().find(|row| row.request.operation_id == request.operation_id) {
            if row.request != request { return Err(err("native operation_id reused with different caller or request")); }
            commit_checked(&tx)?;
            return Ok((row.clone(), false));
        }
        if !pending.is_empty() { return Err(err("native creation unresolved; reconcile the owning operation before allocating another name")); }
        if request.owner_kind != "incus" && rows.iter().any(|row| row.request.owner_kind == request.owner_kind
            && row.request.logical_target == request.logical_target && observed_names.contains(&row.name))
        { return Err(err("logical target already has a native container; retain its binding")); }
        let (number, name) = if is_chatbot_reservation(&request) {
            if rows.iter().any(|row| is_chatbot_reservation(&row.request)) {
                return Err(err("reserved chatbot creation already used; reconcile its original operation"));
            }
            (0, "3tchedCtlChatbot".to_owned())
        } else {
            next.checked_add(1).ok_or_else(|| err("native name sequence exhausted"))?;
            (next, format!("ID-{next}"))
        };
        let record = NativeCreationRecord { request, number, name, phase: "intent".into(), native_uuid: String::new() };
        if observed_names.contains(&record.name) { return Err(err("native name collision")); }
        put(&tx, &record)?;
        put_sequence(&tx, next, &record.request.operation_id)?;
        commit_checked(&tx)?;
        self.sync_identity_store()?;
        Ok((record, true))
    }

    pub fn bind_native_creation(&self, expected: &NativeCreationRecord, native_uuid: &str)
        -> std::result::Result<NativeCreationRecord, CozoError>
    {
        if uuid::Uuid::parse_str(native_uuid).map_or(true, |id| id.to_string() != native_uuid) {
            return Err(err("native creation readback lacks canonical UUID"));
        }
        self.update_native_creation(expected, native_uuid, false)
    }

    /// Called only after complete canonical SHM publication. No abort/reset API.
    pub fn complete_native_creation(&self, expected: &NativeCreationRecord)
        -> std::result::Result<NativeCreationRecord, CozoError>
    {
        self.update_native_creation(expected, &expected.native_uuid, true)
    }

    fn update_native_creation(&self, expected: &NativeCreationRecord, native_uuid: &str, complete: bool)
        -> std::result::Result<NativeCreationRecord, CozoError>
    {
        let _guard = self.identity_transaction.lock().map_err(|_| err("identity transaction owner is poisoned"))?;
        let tx = self.db.multi_transaction(true);
        let rows = records(&tx)?;
        let (next, pending) = sequence(&tx, &rows)?;
        let current = rows.iter().find(|r| r.request.operation_id == expected.request.operation_id)
            .ok_or_else(|| err("native creation intent disappeared"))?;
        if current != expected { return Err(err("native creation compare/exchange conflict")); }
        if current.phase == "complete" {
            if current.native_uuid != native_uuid { return Err(err("native UUID changed")); }
            commit_checked(&tx)?;
            self.sync_identity_store()?;
            return Ok(current.clone());
        }
        if pending != current.request.operation_id
            || (!is_chatbot_reservation(&current.request) && next != current.number)
            || (!current.native_uuid.is_empty() && current.native_uuid != native_uuid)
            || (complete && current.phase != "bound")
        { return Err(err("native creation fence or UUID changed")); }
        let mut updated = current.clone();
        updated.native_uuid = native_uuid.into();
        updated.phase = if complete { "complete" } else { "bound" }.into();
        put(&tx, &updated)?;
        if complete {
            let next = if is_chatbot_reservation(&current.request) { next } else {
                next.checked_add(1).ok_or_else(|| err("native sequence exhausted"))?
            };
            put_sequence(&tx, next, "")?;
        }
        commit_checked(&tx)?;
        self.sync_identity_store()?;
        Ok(updated)
    }
}
