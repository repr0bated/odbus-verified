//! Durable own-native-root exchange fence. Old roots and credentials are retained.
use super::*;

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeRootUpgradeJournal {
    pub operation_id: String,
    pub plan_sha256: String,
    pub plan_json: String,
    pub expected: IdentityRevision,
    pub phase: String,
    pub native_before_json: String,
    pub native_after_json: String,
    pub stopped_native_raw_json: String,
    pub stopped_native_etag: String,
    pub hardened_native_raw_json: String,
    pub old_root_uuid: String,
    pub new_root_uuid: String,
    pub new_root_id: u64,
    pub rollback_name: String,
    pub committed_revision: u64,
}

fn valid(journal: &NativeRootUpgradeJournal) -> std::result::Result<(), CozoError> {
    let plan: Value = serde_json::from_str(&journal.plan_json)?;
    let old = journal.expected.record.as_ref().ok_or_else(|| err("root upgrade identity absent"))?;
    if !valid_root_repair_digest(&journal.plan_sha256)
        || journal.operation_id != format!("native-upgrade-{}", journal.plan_sha256)
        || journal.plan_json.len() > 32768 || plan["version"] != 1
        || plan["session_id"] != journal.expected.session_id
        || plan["caller_session_id"] != journal.expected.session_id
        || plan["caller_genesis"] != old.session_genesis
        || plan["caller_principal_id"] != op_identity::session::derive_principal_id(&old.wireguard_pubkey)
        || old.principal_kind.as_deref() != Some("human") || old.sealed_id.is_empty()
        || journal.expected.genesis.is_none() || journal.expected.revision == 0
        || !old.btrfs_device_json.is_empty()
        || !matches!(journal.phase.as_str(), "prepared" | "stop-intent" | "stopped" | "harden-intent"
            | "hardened" | "exchange-intent" | "exchanged" | "start-intent" | "running" | "identity-committed")
    { return Err(err("invalid own-native-root upgrade journal")); }
    if !journal.stopped_native_raw_json.is_empty() {
        let stopped: Value = serde_json::from_str(&journal.stopped_native_raw_json)?;
        if journal.stopped_native_etag.is_empty() || stopped["status"] != "Stopped"
            || stopped["config"]["volatile.uuid"] != plan["native_uuid"]
            || !clean_provision_instance_binding_matches(&plan["current"]["instance"].to_string(),
                &journal.stopped_native_raw_json)
        { return Err(err("root hardening original UUID, definition or ETag differs from plan")); }
    } else if !matches!(journal.phase.as_str(), "prepared" | "stop-intent" | "stopped") {
        return Err(err("root upgrade lacks durable stopped hardening intent"));
    }
    if !matches!(journal.phase.as_str(), "prepared" | "stop-intent" | "stopped" | "harden-intent")
        && !native_root_policy_transition_matches(&journal.stopped_native_raw_json, &journal.hardened_native_raw_json)
    { return Err(err("root upgrade has no exact hardened native proof")); }
    Ok(())
}

pub(super) fn read(tx: &MultiTransaction, sid: &str) -> std::result::Result<Option<NativeRootUpgradeJournal>, CozoError> {
    let rows = query(tx, "?[document] := *identity_native_root_upgrades{session_id,document}, session_id = $sid", sid_params(sid))?;
    let mut latest: Option<NativeRootUpgradeJournal> = None;
    for row in &rows.rows {
        let journal: NativeRootUpgradeJournal = serde_json::from_str(&string(row, 0)?)?;
        valid(&journal)?;
        if journal.expected.session_id != sid { return Err(err("root upgrade session differs")); }
        if latest.as_ref().is_none_or(|old| old.expected.revision < journal.expected.revision) { latest = Some(journal); }
    }
    Ok(latest)
}

fn put(tx: &MultiTransaction, journal: &NativeRootUpgradeJournal) -> std::result::Result<(), CozoError> {
    query(tx, "?[session_id,operation_id,document] <- [[$sid,$op,$doc]] :put identity_native_root_upgrades {session_id,operation_id => document}",
        BTreeMap::from([("sid".into(), DataValue::Str(journal.expected.session_id.as_str().into())),
            ("op".into(), DataValue::Str(journal.operation_id.as_str().into())),
            ("doc".into(), DataValue::Str(serde_json::to_string(journal)?.into()))]))?;
    Ok(())
}

fn next_phase(before: &str, after: &str) -> bool {
    matches!((before, after), ("prepared", "stop-intent") | ("stop-intent", "stopped")
        | ("stopped", "harden-intent") | ("harden-intent", "hardened")
        | ("hardened", "exchange-intent") | ("exchange-intent", "exchanged")
        | ("exchanged", "start-intent") | ("start-intent", "running"))
}

impl CozoGraphShuttle {
    pub fn read_native_root_upgrade(&self, sid: &str) -> std::result::Result<Option<NativeRootUpgradeJournal>, CozoError> {
        let _guard = self.identity_transaction.lock().map_err(|_| err("identity owner poisoned"))?;
        let tx = self.db.multi_transaction(false);
        let value = read(&tx, sid)?;
        commit_checked(&tx)?;
        Ok(value)
    }

    pub fn begin_native_root_upgrade(&self, journal: NativeRootUpgradeJournal) -> std::result::Result<NativeRootUpgradeJournal, CozoError> {
        valid(&journal)?;
        if journal.phase != "prepared" || journal.committed_revision != 0 || !journal.new_root_uuid.is_empty()
            || !journal.rollback_name.is_empty() || !journal.native_after_json.is_empty()
        { return Err(err("new root upgrade is not prepared")); }
        let _guard = self.identity_transaction.lock().map_err(|_| err("identity owner poisoned"))?;
        let tx = self.db.multi_transaction(true);
        if let Some(old) = read(&tx, &journal.expected.session_id)? {
            if old.operation_id == journal.operation_id {
                if old.plan_json != journal.plan_json || old.expected != journal.expected
                    || old.old_root_uuid != journal.old_root_uuid || old.native_before_json != journal.native_before_json
                { return Err(err("root upgrade operation reused")); }
                commit_checked(&tx)?;
                return Ok(old);
            }
            if old.committed_revision == 0 { return Err(err("another root upgrade is unresolved")); }
        }
        if read_one(&tx, &journal.expected.session_id)?.0 != journal.expected {
            return Err(err("root upgrade compare/exchange conflict"));
        }
        put(&tx, &journal)?;
        commit_checked(&tx)?;
        self.sync_identity_store()?;
        Ok(journal)
    }

    pub fn advance_native_root_upgrade(&self, expected: &NativeRootUpgradeJournal, phase: &str,
        native_after_json: &str, new_root_uuid: &str, new_root_id: u64, rollback_name: &str,
    ) -> std::result::Result<NativeRootUpgradeJournal, CozoError> {
        if !next_phase(&expected.phase, phase) || matches!(phase, "harden-intent" | "hardened") {
            return Err(err("invalid root upgrade phase transition"));
        }
        let _guard = self.identity_transaction.lock().map_err(|_| err("identity owner poisoned"))?;
        let tx = self.db.multi_transaction(true);
        let current = read(&tx, &expected.expected.session_id)?.ok_or_else(|| err("root upgrade journal absent"))?;
        let mut next = expected.clone();
        next.new_root_id = new_root_id;
        next.phase = phase.into();
        next.native_after_json = native_after_json.into();
        next.new_root_uuid = new_root_uuid.into();
        next.rollback_name = rollback_name.into();
        if current != *expected && current != next { return Err(err("root upgrade phase changed")); }
        if read_one(&tx, &expected.expected.session_id)?.0 != expected.expected {
            return Err(err("root upgrade identity revoked or changed"));
        }
        valid(&next)?;
        put(&tx, &next)?;
        commit_checked(&tx)?;
        self.sync_identity_store()?;
        Ok(next)
    }
    pub fn record_native_root_hardening(&self, expected: &NativeRootUpgradeJournal,
        phase: &str, raw: &str, etag: &str,
    ) -> std::result::Result<NativeRootUpgradeJournal, CozoError> {
        if !matches!((expected.phase.as_str(), phase), ("stopped", "harden-intent") | ("harden-intent", "hardened")) {
            return Err(err("invalid native hardening phase"));
        }
        let _guard = self.identity_transaction.lock().map_err(|_| err("identity owner poisoned"))?;
        let tx = self.db.multi_transaction(true);
        let current = read(&tx, &expected.expected.session_id)?.ok_or_else(|| err("upgrade journal absent"))?;
        let mut next = expected.clone();
        next.phase = phase.into();
        if phase == "harden-intent" {
            next.stopped_native_raw_json = raw.into();
            next.stopped_native_etag = etag.into();
        } else {
            next.hardened_native_raw_json = raw.into();
        }
        if current != *expected && current != next {
            return Err(err("native hardening journal changed"));
        }
        if read_one(&tx, &expected.expected.session_id)?.0 != expected.expected {
            return Err(err("native hardening identity revoked or changed"));
        }
        valid(&next)?;
        put(&tx, &next)?;
        commit_checked(&tx)?;
        self.sync_identity_store()?;
        Ok(next)
    }
}

pub(super) fn check_fence(tx: &MultiTransaction, previous: &IdentityRevision, intent: &IdentityIntent) -> std::result::Result<(), CozoError> {
    if let Some(journal) = read(tx, &previous.session_id)? && journal.committed_revision == 0 {
        let completing = matches!(intent, IdentityIntent::UpgradeNativeRoot {journal: expected,..} if **expected == journal);
        let revoking = matches!(intent, IdentityIntent::CompareExchange(exchange) if exchange.record.is_none() && exchange.genesis.is_none());
        if !completing && !revoking { return Err(err("native root upgrade fence blocks unrelated mutation")); }
    }
    Ok(())
}

pub(super) fn validate_commit(tx: &MultiTransaction, sid: &str, operation: &str, previous: &IdentityRevision,
    journal: &NativeRootUpgradeJournal, record: &IdentitySledRecord,
) -> std::result::Result<(), CozoError> {
    valid(journal)?;
    let current = read(tx, sid)?.ok_or_else(|| err("root upgrade journal absent"))?;
    let old = journal.expected.record.as_ref().ok_or_else(|| err("root upgrade identity absent"))?;
    if operation != journal.operation_id || journal.expected.session_id != sid
        || journal.phase != "running" || journal.new_root_uuid.is_empty() || journal.rollback_name.is_empty()
        || !clean_provision::attachment_preserves_identity(old, record) || !record.active
        || record.instance_json != journal.native_after_json
        || !native_root_policy_transition_matches(&old.instance_json, &record.instance_json)
    { return Err(err("root upgrade cannot alter identity or native authority")); }
    if previous.operation_id == operation {
        if current.committed_revision != previous.revision || previous.record.as_ref() != Some(record)
            || previous.genesis != journal.expected.genesis
        { return Err(err("root upgrade replay differs")); }
    } else if current != *journal || *previous != journal.expected {
        return Err(err("root upgrade commit compare/exchange conflict"));
    }
    Ok(())
}

pub(super) fn complete(tx: &MultiTransaction, journal: &NativeRootUpgradeJournal, _record: &IdentitySledRecord) -> std::result::Result<(), CozoError> {
    let mut completed = journal.clone();
    completed.phase = "identity-committed".into();
    completed.committed_revision = journal.expected.revision.checked_add(1).ok_or_else(|| err("identity revision overflow"))?;
    put(tx, &completed)
}

pub(super) fn current_instance(tx: &MultiTransaction, sid: &str, original: &str) -> std::result::Result<String, CozoError> {
    let Some(journal) = read(tx, sid)?.filter(|journal| journal.committed_revision != 0) else {
        return Ok(original.into());
    };
    if native_root_policy_transition_matches(original, &journal.native_after_json) {
        Ok(journal.native_after_json)
    } else {
        Err(err("completed native root upgrade does not preserve original instance authority"))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn root_upgrade_phases_cannot_reexchange_or_skip_power_proof() {
        assert!(!super::next_phase("stopped", "exchange-intent"));
        assert!(super::next_phase("stopped", "harden-intent"));
        assert!(super::next_phase("harden-intent", "hardened"));
        assert!(super::next_phase("hardened", "exchange-intent"));
        assert!(super::next_phase("exchange-intent", "exchanged"));
        assert!(!super::next_phase("exchanged", "exchange-intent"));
        assert!(!super::next_phase("exchanged", "running"));
        assert!(!super::next_phase("prepared", "identity-committed"));
    }
}
