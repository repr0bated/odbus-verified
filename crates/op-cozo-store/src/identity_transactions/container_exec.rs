//! Exactly one guest exec POST per immutable caller operation. Unknown is never retry permission.
use super::*;
use sha2::{Digest as _, Sha256};

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContainerExecRequest {
    pub operation_id: String,
    pub actor_id: String,
    pub caller_session: String,
    pub caller_genesis: String,
    pub target_session: String,
    pub native_uuid: String,
    pub schema_hash: String,
    pub arguments_sha256: String,
    pub arguments_json: String,
    pub target_instance_json: String,
    pub target_root_receipt: String,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContainerExecJournal {
    pub request: ContainerExecRequest,
    pub phase: String,
    pub result_json: String,
}

fn validate(request: &ContainerExecRequest) -> std::result::Result<(), CozoError> {
    if !uuid::Uuid::parse_str(&request.operation_id).is_ok_and(|id| id.to_string() == request.operation_id)
        || !uuid::Uuid::parse_str(&request.native_uuid).is_ok_and(|id| id.to_string() == request.native_uuid)
        || request.actor_id.is_empty() || request.caller_session.is_empty() || request.caller_genesis.is_empty()
        || request.target_session.is_empty() || request.arguments_json.len() > 65536
        || request.target_instance_json.len() > 32768 || request.target_root_receipt.is_empty()
        || !valid_root_repair_digest(&request.arguments_sha256) || !valid_root_repair_digest(&request.schema_hash)
        || request.target_root_receipt.len() > 4096
        || request.arguments_sha256 != format!("{:x}", Sha256::digest(request.arguments_json.as_bytes()))
    { return Err(err("invalid guest execution request binding")); }
    let arguments: Value = serde_json::from_str(&request.arguments_json)?;
    let command = arguments["command"].as_array().ok_or_else(|| err("guest argv absent"))?;
    if command.is_empty() || command.len() > 128 || command.iter().any(|arg|
        arg.as_str().is_none_or(|arg| arg.len() > 16384 || arg.contains('\0')))
        || command[0].as_str().is_none_or(str::is_empty)
    { return Err(err("invalid bounded guest command")); }
    Ok(())
}

fn read(tx: &MultiTransaction, operation: &str) -> std::result::Result<Option<ContainerExecJournal>, CozoError> {
    let rows = query(tx, "?[document] := *identity_container_execs{operation_id,document}, operation_id = $op",
        BTreeMap::from([("op".into(), DataValue::Str(operation.into()))]))?;
    match rows.rows.as_slice() {
        [] => Ok(None),
        [row] => {
            let journal: ContainerExecJournal = serde_json::from_str(&string(row, 0)?)?;
            validate(&journal.request)?;
            if journal.request.operation_id != operation || !matches!(journal.phase.as_str(), "intent" | "complete" | "unknown")
                || (journal.phase == "intent") != journal.result_json.is_empty()
            { return Err(err("corrupt guest exec journal")); }
            Ok(Some(journal))
        }
        _ => Err(err("nonunique guest exec journal")),
    }
}

fn put(tx: &MultiTransaction, journal: &ContainerExecJournal) -> std::result::Result<(), CozoError> {
    query(tx, "?[operation_id,document] <- [[$op,$doc]] :put identity_container_execs {operation_id => document}",
        BTreeMap::from([("op".into(), DataValue::Str(journal.request.operation_id.as_str().into())),
            ("doc".into(), DataValue::Str(serde_json::to_string(journal)?.into()))]))?;
    Ok(())
}

impl CozoGraphShuttle {
    /// Fresh=true is the sole POST permission, granted only after durable sync.
    pub fn reserve_container_exec(&self, request: ContainerExecRequest) -> std::result::Result<(ContainerExecJournal, bool), CozoError> {
        validate(&request)?;
        let _guard = self.identity_transaction.lock().map_err(|_| err("identity owner poisoned"))?;
        let tx = self.db.multi_transaction(true);
        if let Some(journal) = read(&tx, &request.operation_id)? {
            if journal.request != request { return Err(err("guest operation reused with different caller, UUID, source or argv")); }
            commit_checked(&tx)?;
            return Ok((journal, false));
        }
        let caller = read_one(&tx, &request.caller_session)?.0.record.ok_or_else(|| err("exec caller absent"))?;
        let target = read_one(&tx, &request.target_session)?.0.record.ok_or_else(|| err("exec target absent"))?;
        let instance: Value = serde_json::from_str(&target.instance_json)?;
        if caller.session_genesis != request.caller_genesis || caller.sealed_id.is_empty()
            || op_identity::session::derive_principal_id(&caller.wireguard_pubkey) != request.actor_id
            || target.sealed_id.is_empty() || instance["config"]["volatile.uuid"] != request.native_uuid
            || !clean_provision_instance_binding_matches(&target.instance_json, &request.target_instance_json)
        { return Err(err("guest exec caller or target changed before reservation")); }
        if let Some(upgrade) = native_upgrade::read(&tx, &request.target_session)? && upgrade.committed_revision == 0 {
            return Err(err("guest root upgrade is unresolved"));
        }
        let journal = ContainerExecJournal {request, phase:"intent".into(), result_json:String::new()};
        put(&tx, &journal)?;
        commit_checked(&tx)?;
        self.sync_identity_store()?;
        Ok((journal, true))
    }

    pub fn finish_container_exec(&self, expected: &ContainerExecJournal, result_json: &str, unknown: bool)
        -> std::result::Result<ContainerExecJournal, CozoError> {
        if result_json.is_empty() || result_json.len() > 16 * 1024 * 1024 {
            return Err(err("invalid guest exec result bound"));
        }
        let _: Value = serde_json::from_str(result_json)?;
        let _guard = self.identity_transaction.lock().map_err(|_| err("identity owner poisoned"))?;
        let tx = self.db.multi_transaction(true);
        let old = read(&tx, &expected.request.operation_id)?.ok_or_else(|| err("exec intent absent"))?;
        let mut next = expected.clone();
        next.phase = if unknown {"unknown"} else {"complete"}.into();
        next.result_json = result_json.into();
        if old != *expected && old != next { return Err(err("guest exec result changed")); }
        if expected.phase != "intent" { return Err(err("guest execution result cannot be overwritten")); }
        put(&tx, &next)?;
        commit_checked(&tx)?;
        self.sync_identity_store()?;
        Ok(next)
    }
}
