//! Explicit one-time storage migration. Original repair/provision journals are
//! retained verbatim; this receipt supersedes their storage target, never identity.
use super::*;

pub struct IdentityPhysicalRootMigration {
    pub expected: IdentityRevision,
    pub record: IdentitySledRecord,
    pub expected_root_repair: Option<RootRepairJournal>,
    pub expected_seed_provision: Option<SeedProvisionJournal>,
    pub plan_sha256: String,
    pub provenance_json: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PhysicalRootMigrationReceipt {
    pub plan_sha256: String,
    pub operation_id: String,
    pub expected_revision: u64,
    pub committed_revision: u64,
    pub btrfs_device_json: String,
    pub instance_json: String,
    pub provenance_json: String,
}

/// Container power observations are mutable; storage, credentials, config,
/// native UUID and every other instance field remain pinned by migration.
pub fn physical_root_instance_binding_matches(pinned: &str, current: &str) -> bool {
    let (Ok(mut pinned), Ok(mut current)) = (object(pinned), object(current)) else {
        return false;
    };
    for value in [&mut pinned, &mut current] {
        let Some(status) = value.get("status").and_then(Value::as_str) else {
            return false;
        };
        let power = match status {
            "Running" => "RUNNING",
            "Stopped" => "STOPPED",
            "Frozen" => "FROZEN",
            _ => return false,
        };
        for field in ["config", "expanded_config"] {
            if let Some(config) = value.get_mut(field) {
                let Some(config) = config.as_object_mut() else {
                    return false;
                };
                if let Some(observed) = config.remove("volatile.last_state.power")
                    && observed.as_str() != Some(power)
                {
                    return false;
                }
            }
        }
        let object = value.as_object_mut().expect("validated object");
        object.remove("status");
        object.remove("status_code");
    }
    pinned == current
}

fn object(text: &str) -> std::result::Result<Value, CozoError> {
    if text.len() > 64 * 1024 {
        return Err(err("physical migration document exceeds64KiB"));
    }
    let value: Value =
        serde_json::from_str(text).map_err(|_| err("invalid physical migration document"))?;
    if !value.is_object() {
        return Err(err("physical migration document must be an object"));
    }
    Ok(value)
}

pub(super) fn read_in_tx(
    tx: &MultiTransaction,
    sid: &str,
) -> std::result::Result<Option<PhysicalRootMigrationReceipt>, CozoError> {
    let rows = query(
        tx,
        "?[plan_sha256,operation_id,expected_revision,committed_revision,btrfs_device_json,instance_json,provenance_json] := *identity_physical_root_migrations{session_id,plan_sha256,operation_id,expected_revision,committed_revision,btrfs_device_json,instance_json,provenance_json}, session_id = $sid",
        sid_params(sid),
    )?;
    let row = match rows.rows.as_slice() {
        [] => return Ok(None),
        [row] if row.len() == 7 => row,
        _ => return Err(err("corrupt physical migration receipt")),
    };
    let receipt = PhysicalRootMigrationReceipt {
        plan_sha256: string(row, 0)?,
        operation_id: string(row, 1)?,
        expected_revision: integer(row, 2)? as u64,
        committed_revision: integer(row, 3)? as u64,
        btrfs_device_json: string(row, 4)?,
        instance_json: string(row, 5)?,
        provenance_json: string(row, 6)?,
    };
    if !valid_root_repair_digest(&receipt.plan_sha256)
        || receipt.operation_id != format!("physical-migrate-{}", receipt.plan_sha256)
        || receipt.expected_revision == 0
        || receipt.expected_revision.checked_add(1) != Some(receipt.committed_revision)
        || receipt.committed_revision > i64::MAX as u64
    {
        return Err(err("corrupt physical migration receipt identity"));
    }
    object(&receipt.btrfs_device_json)?;
    // A committed version-2 shared-human migration retires the per-human
    // instance binding: the receipt's instance column is empty by design —
    // the same shape `validate_pair` already accepts for the record —
    // because the shared runtime's container belongs to the service session,
    // never to the human. For every other receipt an empty or non-object
    // column is corruption and must keep failing closed.
    let shared_human_v2 = receipt.instance_json.is_empty()
        && serde_json::from_str::<Value>(&receipt.provenance_json).is_ok_and(|provenance| {
            provenance.get("version").and_then(Value::as_u64) == Some(2)
                && provenance.get("session_id").and_then(Value::as_str) == Some(sid)
        });
    if !shared_human_v2 {
        object(&receipt.instance_json)?;
    }
    object(&receipt.provenance_json)?;
    Ok(Some(receipt))
}

impl CozoGraphShuttle {
    /// Private owner metadata. Never expose its payload in public state/inventory.
    pub fn read_physical_root_migration(
        &self,
        sid: &str,
    ) -> std::result::Result<Option<PhysicalRootMigrationReceipt>, CozoError> {
        let _guard = self
            .identity_transaction
            .lock()
            .map_err(|_| err("identity transaction owner is poisoned"))?;
        let tx = self.db.multi_transaction(false);
        let receipt = read_in_tx(&tx, sid)?;
        commit_checked(&tx)?;
        Ok(receipt)
    }
}

pub(super) fn validate_intent(
    tx: &MultiTransaction,
    sid: &str,
    operation: &str,
    current: &IdentityRevision,
    metadata: &RevisionMetadata,
    migration: &IdentityPhysicalRootMigration,
    receipt: Option<&PhysicalRootMigrationReceipt>,
) -> std::result::Result<(), CozoError> {
    if !valid_root_repair_digest(&migration.plan_sha256)
        || operation != format!("physical-migrate-{}", migration.plan_sha256)
        || migration.expected.session_id != sid
        || migration.expected.revision == 0
    {
        return Err(err("invalid physical migration operation identity"));
    }
    let root = read_root_repair_journal_in_tx(tx, sid)?;
    let seed = read_seed_provision_journal_in_tx(tx, sid)?;
    if root != migration.expected_root_repair
        || seed != migration.expected_seed_provision
        || (root.is_none() && seed.is_none())
        || root
            .as_ref()
            .is_some_and(|j| j.phase != "identity-committed")
        || seed
            .as_ref()
            .is_some_and(|j| j.phase != "identity-committed")
    {
        return Err(err(
            "physical migration requires exact completed historical journals",
        ));
    }
    let old = migration
        .expected
        .record
        .as_ref()
        .ok_or_else(|| err("physical migration requires existing identity"))?;
    let genesis = migration
        .expected
        .genesis
        .as_ref()
        .ok_or_else(|| err("physical migration requires existing genesis"))?;
    if old.principal_kind.as_deref() != Some("human")
        || old.sealed_id.is_empty()
        || old.session_genesis.is_empty()
        || genesis.arrival_timestamp <= 0
        || genesis.chain_head_at_arrival.is_empty()
    {
        return Err(err("physical migration requires anchored human identity"));
    }
    let mut allowed = old.clone();
    allowed
        .btrfs_device_json
        .clone_from(&migration.record.btrfs_device_json);
    allowed
        .instance_json
        .clone_from(&migration.record.instance_json);
    allowed.mutation_index = old
        .mutation_index
        .checked_add(1)
        .ok_or_else(|| err("identity index overflow"))?;
    if allowed != migration.record {
        return Err(err(
            "physical migration changed identity authority or unrelated fields",
        ));
    }
    validate_pair(sid, Some(&migration.record), Some(genesis))?;
    let old_device = object(&old.btrfs_device_json)?;
    let new_device = object(&migration.record.btrfs_device_json)?;
    let instance = object(&migration.record.instance_json)?;
    object(&migration.provenance_json)?;
    let cozo = old_device
        .get("cozo_id")
        .and_then(Value::as_str)
        .filter(|v| !v.is_empty())
        .ok_or_else(|| err("physical migration requires existing Cozo owner"))?;
    if new_device.get("cozo_id").and_then(Value::as_str) != Some(cozo)
        || new_device.get("attached").and_then(Value::as_bool) != Some(true)
        || instance.get("name").and_then(Value::as_str) != Some(sid)
        || new_device == old_device
    {
        return Err(err("physical migration target binding is invalid"));
    }
    for key in ["device_path", "mount_point", "btrfs_uuid"] {
        if new_device
            .get(key)
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
        {
            return Err(err("physical migration target binding is incomplete"));
        }
    }
    if current.operation_id == operation {
        let receipt = receipt.ok_or_else(|| err("physical migration replay receipt is absent"))?;
        if metadata.kind != "physical-migrate"
            || metadata.value != migration.expected.revision as i64
            || current.record.as_ref() != Some(&migration.record)
            || current.genesis != migration.expected.genesis
            || receipt.plan_sha256 != migration.plan_sha256
            || receipt.provenance_json != migration.provenance_json
            || receipt.btrfs_device_json != migration.record.btrfs_device_json
            || receipt.instance_json != migration.record.instance_json
            || receipt.expected_revision != migration.expected.revision
            || receipt.committed_revision != current.revision
        {
            return Err(err(
                "physical migration operation reused for different intent",
            ));
        }
        let history = query(
            tx,
            "?[old_btrfs_device_json,old_instance_json,old_journals_json,old_revision_operation_id] := *identity_physical_root_history{session_id,committed_revision,old_btrfs_device_json,old_instance_json,old_journals_json,old_revision_operation_id}, session_id = $sid, committed_revision = $revision",
            BTreeMap::from([
                ("sid".into(), DataValue::Str(sid.into())),
                ("revision".into(), dv_int(current.revision as i64)),
            ]),
        )?;
        let row = match history.rows.as_slice() {
            [row] if row.len() == 4 => row,
            _ => return Err(err("physical migration replay history is absent")),
        };
        let journals = serde_json::to_string(&(
            migration.expected_root_repair.clone(),
            migration.expected_seed_provision.clone(),
        ))?;
        if string(row, 0)? != old.btrfs_device_json
            || string(row, 1)? != old.instance_json
            || string(row, 2)? != journals
            || string(row, 3)? != migration.expected.operation_id
        {
            return Err(err("physical migration replay changed historical intent"));
        }
    } else if receipt.is_some() || current != &migration.expected {
        return Err(err(
            "physical migration already exists or identity revision changed",
        ));
    }
    Ok(())
}

pub(super) fn append_in_tx(
    tx: &MultiTransaction,
    sid: &str,
    operation: &str,
    migration: &IdentityPhysicalRootMigration,
) -> std::result::Result<(), CozoError> {
    let revision = migration
        .expected
        .revision
        .checked_add(1)
        .filter(|v| *v <= i64::MAX as u64)
        .ok_or_else(|| err("identity revision overflow"))?;
    let history = query(
        tx,
        "?[committed_revision] := *identity_physical_root_history{session_id,committed_revision}, session_id = $sid",
        sid_params(sid),
    )?;
    if !history.rows.is_empty() {
        return Err(err("physical migration history already exists"));
    }
    let old = migration
        .expected
        .record
        .as_ref()
        .ok_or_else(|| err("migration identity absent"))?;
    let journals = serde_json::to_string(&(
        migration.expected_root_repair.clone(),
        migration.expected_seed_provision.clone(),
    ))?;
    let params = BTreeMap::from([
        ("sid".into(), DataValue::Str(sid.into())),
        (
            "plan".into(),
            DataValue::Str(migration.plan_sha256.as_str().into()),
        ),
        ("op".into(), DataValue::Str(operation.into())),
        (
            "expected".into(),
            dv_int(migration.expected.revision as i64),
        ),
        ("committed".into(), dv_int(revision as i64)),
        (
            "device".into(),
            DataValue::Str(migration.record.btrfs_device_json.as_str().into()),
        ),
        (
            "instance".into(),
            DataValue::Str(migration.record.instance_json.as_str().into()),
        ),
        (
            "provenance".into(),
            DataValue::Str(migration.provenance_json.as_str().into()),
        ),
        (
            "old_device".into(),
            DataValue::Str(old.btrfs_device_json.as_str().into()),
        ),
        (
            "old_instance".into(),
            DataValue::Str(old.instance_json.as_str().into()),
        ),
        ("journals".into(), DataValue::Str(journals.into())),
        (
            "old_op".into(),
            DataValue::Str(migration.expected.operation_id.as_str().into()),
        ),
    ]);
    query(
        tx,
        "?[session_id,plan_sha256,operation_id,expected_revision,committed_revision,btrfs_device_json,instance_json,provenance_json] <- [[$sid,$plan,$op,$expected,$committed,$device,$instance,$provenance]] :put identity_physical_root_migrations {session_id => plan_sha256,operation_id,expected_revision,committed_revision,btrfs_device_json,instance_json,provenance_json}",
        params.clone(),
    )?;
    query(
        tx,
        "?[session_id,committed_revision,plan_sha256,operation_id,old_btrfs_device_json,old_instance_json,old_journals_json,old_revision_operation_id] <- [[$sid,$committed,$plan,$op,$old_device,$old_instance,$journals,$old_op]] :put identity_physical_root_history {session_id,committed_revision => plan_sha256,operation_id,old_btrfs_device_json,old_instance_json,old_journals_json,old_revision_operation_id}",
        params,
    )?;
    Ok(())
}
