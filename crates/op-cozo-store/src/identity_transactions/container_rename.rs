//! One durable native-name cutover for an existing identity. Original creation,
//! storage and credential receipts remain untouched; only the current binding moves.
use super::*;

pub const CONTROL_PLANE_NATIVE_NAME: &str = "3tchedCockpitBot";

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContainerRenameRequest {
    pub operation_id: String,
    pub session_id: String,
    pub native_uuid: String,
    pub old_name: String,
    pub new_name: String,
    pub actor_id: String,
    pub caller_session: String,
    pub caller_genesis: String,
    pub schema_hash: String,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContainerRenameJournal {
    pub request: ContainerRenameRequest,
    pub expected_revision: u64,
    pub expected_revision_operation_id: String,
    /// Exact stopped native observation, not a caller-supplied desired definition.
    pub native_before_json: String,
    pub native_after_json: String,
    pub committed_revision: u64,
}

fn validate_request(request: &ContainerRenameRequest) -> std::result::Result<(), CozoError> {
    let canonical_uuid = |value: &str| uuid::Uuid::parse_str(value).is_ok_and(|id| id.to_string() == value);
    if !canonical_uuid(&request.operation_id) || !canonical_uuid(&request.session_id)
        || !canonical_uuid(&request.native_uuid)
        || request.old_name.is_empty() || request.old_name.len() > 63
        || !request.old_name.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        || request.old_name == request.new_name || request.new_name != CONTROL_PLANE_NATIVE_NAME
        || request.actor_id.is_empty() || request.caller_session.is_empty()
        || request.caller_genesis.is_empty() || !valid_root_repair_digest(&request.schema_hash)
    {
        return Err(err("invalid configured-service native rename intent"));
    }
    Ok(())
}

pub(super) fn read(tx: &MultiTransaction, sid: &str) -> std::result::Result<Option<ContainerRenameJournal>, CozoError> {
    let rows = query(tx, "?[document] := *identity_container_renames{session_id,document}, session_id = $sid", sid_params(sid))?;
    match rows.rows.as_slice() {
        [] => Ok(None),
        [row] => {
            let journal: ContainerRenameJournal = serde_json::from_str(&string(row, 0)?)?;
            validate_request(&journal.request)?;
            let native: Value = serde_json::from_str(&journal.native_before_json)?;
            if journal.request.session_id != sid || journal.expected_revision == 0
                || native["name"] != journal.request.old_name
                || native["config"]["volatile.uuid"] != journal.request.native_uuid
                || (journal.committed_revision == 0 && !journal.native_after_json.is_empty())
                || (journal.committed_revision != 0 && !container_rename_native_matches(
                    &journal.native_before_json, &journal.native_after_json, &journal.request.new_name))
                || (journal.committed_revision != 0
                    && journal.expected_revision.checked_add(1) != Some(journal.committed_revision))
            { return Err(err("identity rename journal is corrupt")); }
            Ok(Some(journal))
        }
        _ => Err(err("identity rename journal is not unique")),
    }
}

fn put(tx: &MultiTransaction, journal: &ContainerRenameJournal) -> std::result::Result<(), CozoError> {
    query(tx, "?[session_id,document] <- [[$sid,$doc]] :put identity_container_renames {session_id => document}",
        BTreeMap::from([
            ("sid".into(), DataValue::Str(journal.request.session_id.as_str().into())),
            ("doc".into(), DataValue::Str(serde_json::to_string(journal)?.into())),
        ]))?;
    Ok(())
}

/// Incus 7 queues its rename template and regenerates cloud-init's instance-id.
/// Those two daemon-owned volatile values are not the native UUID or SID1.
/// Every other config/device/profile/storage field must be byte-for-byte equal.
pub fn container_rename_native_matches(before: &str, after: &str, new_name: &str) -> bool {
    let (Ok(mut before), Ok(after)) = (serde_json::from_str::<Value>(before), serde_json::from_str::<Value>(after)) else {
        return false;
    };
    if before["type"] != "container" || before["status"] != "Stopped" || before["status_code"] != 102
        || after["name"] != new_name || before["name"] == new_name
    { return false; }
    before["name"] = new_name.into();
    let Some(cloud_id) = after["config"]["volatile.cloud-init.instance-id"].as_str() else { return false; };
    if !uuid::Uuid::parse_str(cloud_id).is_ok_and(|id| id.to_string() == cloud_id) { return false; }
    for field in ["config", "expanded_config"] {
        let Some(config) = before[field].as_object_mut() else { return false; };
        if after[field]["volatile.cloud-init.instance-id"] != cloud_id { return false; }
        let template = config.get("volatile.apply_template").and_then(Value::as_str).filter(|value| !value.is_empty()).unwrap_or("rename");
        if after[field]["volatile.apply_template"] != template { return false; }
        config.insert("volatile.apply_template".into(), after[field]["volatile.apply_template"].clone());
        config.insert("volatile.cloud-init.instance-id".into(), cloud_id.into());
    }
    before == after
}

fn apply_native_name(value: &mut Value, name: &str, after: &Value) {
    value["name"] = name.into();
    for field in ["config", "expanded_config"] {
        if let Some(config) = value[field].as_object_mut() {
            for key in ["volatile.apply_template", "volatile.cloud-init.instance-id"] {
                config.insert(key.into(), after[field][key].clone());
            }
        }
    }
}

/// Resolve an original receipt through this identity's completed, exact-UUID
/// rename observation. Original receipts are never rewritten.
pub(super) fn current_instance(tx: &MultiTransaction, sid: &str, original: &str)
    -> std::result::Result<String, CozoError>
{
    let Some(journal) = read(tx, sid)?.filter(|journal| journal.committed_revision != 0) else {
        return Ok(original.to_owned());
    };
    let mut value: Value = serde_json::from_str(original)?;
    if value["name"] == journal.request.old_name
        && value["config"]["volatile.uuid"] == journal.request.native_uuid
    {
        apply_native_name(&mut value, &journal.request.new_name, &serde_json::from_str(&journal.native_after_json)?);
        Ok(serde_json::to_string(&value)?)
    } else { Ok(original.to_owned()) }
}

pub(super) fn creation_name(tx: &MultiTransaction, creation: &NativeCreationRecord)
    -> std::result::Result<Option<String>, CozoError>
{
    if creation.request.owner_kind != "identity" { return Ok(None); }
    Ok(read(tx, &creation.request.logical_target)?.filter(|journal|
        journal.committed_revision != 0 && creation.phase == "complete"
        && creation.name == journal.request.old_name && creation.native_uuid == journal.request.native_uuid)
        .map(|journal| journal.request.new_name))
}

pub(super) fn check_fence(tx: &MultiTransaction, previous: &IdentityRevision, intent: &IdentityIntent)
    -> std::result::Result<(), CozoError>
{
    let Some(journal) = read(tx, &previous.session_id)? else { return Ok(()); };
    if journal.committed_revision == 0 {
        let completing = matches!(intent, IdentityIntent::RenameContainer { journal: expected, .. } if **expected == journal);
        let revoking = matches!(intent, IdentityIntent::CompareExchange(exchange)
            if exchange.record.is_none() && exchange.genesis.is_none());
        if !completing && !revoking { return Err(err("identity rename fence blocks unrelated mutation")); }
    } else if let IdentityIntent::CompareExchange(exchange) = intent
        && let Some(record) = &exchange.record
    {
        let old = previous.record.as_ref().ok_or_else(|| err("renamed identity tombstone requires explicit provision"))?;
        let previous_instance: Value = serde_json::from_str(&old.instance_json)?;
        let instance: Value = serde_json::from_str(&record.instance_json)?;
        // A later explicit CleanProvision may replace the native UUID while
        // preserving this historical receipt. Ordinary CAS may not do so.
        if previous_instance["config"]["volatile.uuid"] == journal.request.native_uuid
            && (instance["name"] != journal.request.new_name
                || instance["config"]["volatile.uuid"] != journal.request.native_uuid)
        { return Err(err("completed identity rename binding is immutable")); }
    }
    Ok(())
}

pub(super) fn complete(tx: &MultiTransaction, previous: &IdentityRevision, operation: &str,
    expected: &ContainerRenameJournal, native_after_json: &str)
    -> std::result::Result<IdentitySledRecord, CozoError>
{
    let current = read(tx, &previous.session_id)?.ok_or_else(|| err("identity rename intent absent"))?;
    if current != *expected || current.committed_revision != 0
        || operation != format!("rename-{}", current.request.operation_id)
        || previous.revision != current.expected_revision
        || previous.operation_id != current.expected_revision_operation_id
    { return Err(err("identity rename compare/exchange conflict")); }
    if !container_rename_native_matches(&current.native_before_json, native_after_json, &current.request.new_name) {
        return Err(err("native rename changed a pinned field"));
    }
    let mut record = previous.record.clone().ok_or_else(|| err("renamed identity was revoked"))?;
    let mut instance: Value = serde_json::from_str(&record.instance_json)?;
    if instance["name"] != current.request.old_name
        || instance["config"]["volatile.uuid"] != current.request.native_uuid
    { return Err(err("identity rename original binding differs")); }
    apply_native_name(&mut instance, &current.request.new_name, &serde_json::from_str(native_after_json)?);
    record.instance_json = serde_json::to_string(&instance)?;
    record.mutation_index = record.mutation_index.checked_add(1).ok_or_else(|| err("identity index overflow"))?;
    validate_pair(&previous.session_id, Some(&record), previous.genesis.as_ref())?;
    validate_binding(previous, Some(&record), previous.genesis.as_ref())?;
    let mut committed = current;
    committed.committed_revision = previous.revision.checked_add(1).ok_or_else(|| err("identity revision overflow"))?;
    committed.native_after_json = native_after_json.to_owned();
    put(tx, &committed)?;
    Ok(record)
}

impl CozoGraphShuttle {
    pub fn container_rename_name_reserved(&self, name: &str) -> std::result::Result<bool, CozoError> {
        let _guard = self.identity_transaction.lock().map_err(|_| err("identity transaction owner is poisoned"))?;
        let tx = self.db.multi_transaction(false);
        let rows = query(&tx, "?[document] := *identity_container_renames{document}", Params::new())?;
        let mut reserved = false;
        for row in &rows.rows {
            let journal: ContainerRenameJournal = serde_json::from_str(&string(row, 0)?)?;
            validate_request(&journal.request)?;
            reserved |= journal.request.old_name == name || journal.request.new_name == name;
        }
        commit_checked(&tx)?;
        Ok(reserved)
    }
}

impl CozoGraphShuttle {
    pub fn read_container_rename(&self, sid: &str) -> std::result::Result<Option<ContainerRenameJournal>, CozoError> {
        let _guard = self.identity_transaction.lock().map_err(|_| err("identity transaction owner is poisoned"))?;
        let tx = self.db.multi_transaction(false);
        let journal = read(&tx, sid)?;
        commit_checked(&tx)?;
        Ok(journal)
    }

    pub fn identity_instance_with_current_name(&self, sid: &str, original: &str) -> std::result::Result<String, CozoError> {
        let _guard = self.identity_transaction.lock().map_err(|_| err("identity transaction owner is poisoned"))?;
        let tx = self.db.multi_transaction(false);
        let value = current_instance(&tx, sid, original)?;
        commit_checked(&tx)?;
        Ok(value)
    }

    /// The returned `fresh` bit is the sole POST permission. Once persisted,
    /// every later call only reconciles UUID+names, even if the first POST reply
    /// was lost or the caller died before issuing it.
    pub fn begin_container_rename(&self, request: ContainerRenameRequest, expected: &IdentityRevision,
        native_before_json: String, observed_names: &[String])
        -> std::result::Result<(ContainerRenameJournal, bool), CozoError>
    {
        validate_request(&request)?;
        if self.identity_directory.is_none() { return Err(err("native rename requires persistent identity storage")); }
        let _guard = self.identity_transaction.lock().map_err(|_| err("identity transaction owner is poisoned"))?;
        self.sync_identity_store()?;
        let tx = self.db.multi_transaction(true);
        if let Some(journal) = read(&tx, &request.session_id)? {
            if journal.request != request { return Err(err("identity rename already has another owner or intent")); }
            commit_checked(&tx)?;
            self.sync_identity_store()?;
            return Ok((journal, false));
        }
        let (previous, _) = read_one(&tx, &request.session_id)?;
        if previous != *expected { return Err(err("identity rename revision changed")); }
        let record = previous.record.as_ref().ok_or_else(|| err("identity rename target absent"))?;
        let bound: Value = serde_json::from_str(&record.instance_json)?;
        let native: Value = serde_json::from_str(&native_before_json)?;
        if record.principal_kind.as_deref() != Some("service") || record.sealed_id.is_empty()
            || previous.genesis.is_none() || record.session_genesis.is_empty()
            || op_identity::session::derive_session_id(&record.wireguard_pubkey) != request.session_id
            || bound["name"] != request.old_name || bound["config"]["volatile.uuid"] != request.native_uuid
            || native["name"] != request.old_name || native["type"] != "container"
            || native["status"] != "Stopped" || native["status_code"] != 102
            || native["config"]["volatile.uuid"] != request.native_uuid
            || native["config"]["user.opdbus.session_id"] != request.session_id
            || native["config"]["user.opdbus.wireguard_public_key"] != record.wireguard_pubkey
            || observed_names.iter().filter(|name| **name == request.old_name).count() != 1
            || observed_names.contains(&request.new_name)
        { return Err(err("native rename does not match the existing stopped service identity")); }
        if read_root_repair_journal_in_tx(&tx, &request.session_id)?.is_some_and(|j| j.phase != "identity-committed")
            || read_seed_provision_journal_in_tx(&tx, &request.session_id)?.is_some_and(|j| j.phase != "identity-committed")
        { return Err(err("native rename is blocked by unresolved storage maintenance")); }
        let creations = native_creation::records(&tx)?;
        if creations.iter().any(|creation| creation.name == request.new_name
            || (creation.request.logical_target == request.session_id && creation.phase != "complete"))
        { return Err(err("native rename collides with a creation reservation")); }
        let other_bindings = query(&tx, "?[session_id,instance_json] := *identity_sessions{session_id,instance_json}", Params::new())?;
        for row in &other_bindings.rows {
            let text = string(row, 1)?;
            if string(row, 0)? != request.session_id && !text.is_empty() {
                let other: Value = serde_json::from_str(&text)?;
                if other["name"] == request.new_name || other["config"]["volatile.uuid"] == request.native_uuid {
                    return Err(err("native rename collides with another identity binding"));
                }
            }
        }
        let reservations = query(&tx, "?[document] := *identity_container_renames{document}", Params::new())?;
        for row in &reservations.rows {
            let other: ContainerRenameJournal = serde_json::from_str(&string(row, 0)?)?;
            if other.request.new_name == request.new_name || other.request.native_uuid == request.native_uuid {
                return Err(err("native rename name/UUID is already reserved"));
            }
        }
        let journal = ContainerRenameJournal { request, expected_revision: previous.revision,
            expected_revision_operation_id: previous.operation_id, native_before_json,
            native_after_json: String::new(), committed_revision: 0 };
        put(&tx, &journal)?;
        commit_checked(&tx)?;
        self.sync_identity_store()?;
        Ok((journal, true))
    }
}
