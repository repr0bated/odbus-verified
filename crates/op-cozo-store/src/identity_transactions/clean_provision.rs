//! Atomic fresh same-key adoption. History is private and never projected into the new record.
use super::*;

pub struct IdentityCleanProvision {
    pub expected: IdentityRevision,
    pub record: IdentitySledRecord,
    pub genesis: GenesisInputsRecord,
    pub plan_sha256: String,
    pub provenance_json: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CleanProvisionReceipt {
    pub plan_sha256: String,
    pub operation_id: String,
    pub expected_revision: u64,
    pub committed_revision: u64,
    pub provenance_json: String,
    pub btrfs_device_json: String,
    pub instance_json: String,
}

fn read(
    tx: &MultiTransaction,
    sid: &str,
) -> std::result::Result<Option<CleanProvisionReceipt>, CozoError> {
    let rows = query(
        tx,
        "?[committed_revision,plan_sha256,operation_id,expected_revision,provenance_json,btrfs_device_json,instance_json] := *identity_clean_provisions{session_id,committed_revision,plan_sha256,operation_id,expected_revision,provenance_json,btrfs_device_json,instance_json}, session_id = $sid :order -committed_revision :limit 1",
        sid_params(sid),
    )?;
    let row = match rows.rows.as_slice() {
        [] => return Ok(None),
        [row] if row.len() == 7 => row,
        _ => return Err(err("corrupt clean provision receipt")),
    };
    let receipt = CleanProvisionReceipt {
        committed_revision: integer(row, 0)? as u64,
        plan_sha256: string(row, 1)?,
        operation_id: string(row, 2)?,
        expected_revision: integer(row, 3)? as u64,
        provenance_json: string(row, 4)?,
        btrfs_device_json: string(row, 5)?,
        instance_json: string(row, 6)?,
    };
    if !valid_root_repair_digest(&receipt.plan_sha256)
        || receipt.operation_id != format!("clean-provision-{}", receipt.plan_sha256)
        || receipt.expected_revision.checked_add(1) != Some(receipt.committed_revision)
        || receipt.provenance_json.len() > 32768
    {
        return Err(err("corrupt clean provision receipt identity"));
    }
    Ok(Some(receipt))
}

impl CozoGraphShuttle {
    pub fn read_clean_provision(
        &self,
        sid: &str,
    ) -> std::result::Result<Option<CleanProvisionReceipt>, CozoError> {
        let _guard = self
            .identity_transaction
            .lock()
            .map_err(|_| err("identity transaction owner is poisoned"))?;
        let tx = self.db.multi_transaction(false);
        let receipt = read(&tx, sid)?;
        commit_checked(&tx)?;
        Ok(receipt)
    }
}

pub(super) fn validate(
    tx: &MultiTransaction,
    sid: &str,
    operation: &str,
    previous: &IdentityRevision,
    metadata: &RevisionMetadata,
    provision: &IdentityCleanProvision,
) -> std::result::Result<(), CozoError> {
    let old = provision.expected.record.as_ref();
    let new = &provision.record;
    let explicitly_deleted = provision.expected.revision > 1
        && old.is_none()
        && provision.expected.genesis.is_none()
        && provision.expected.operation_id
            == format!("delete-provision-{sid}-{}", provision.expected.revision - 1);
    let provenance: Value = serde_json::from_str(&provision.provenance_json)?;
    let fresh_partitioned = provenance["version"] == 4
        && provenance["partitioned_overlay"].is_object()
        && provision.expected.revision == 0 && provision.expected.operation_id.is_empty()
        && old.is_none() && provision.expected.genesis.is_none();
    if !explicitly_deleted && !fresh_partitioned && old.is_none() {
        return Err(err(
            "clean provision requires an existing key or explicit deletion tombstone",
        ));
    }
    if !valid_root_repair_digest(&provision.plan_sha256)
        || operation != format!("clean-provision-{}", provision.plan_sha256)
        || provision.expected.session_id != sid
        || new.session_id != sid
        || old.is_some_and(|old| {
            old.session_id != sid || new.wireguard_pubkey != old.wireguard_pubkey
        })
        || op_identity::session::derive_session_id(&new.wireguard_pubkey) != sid
        || new.principal_kind.as_deref() != Some("service")
        || new.active
        || new.mutation_index != 0
        || new.expires_at != 0
        || !new.interface.is_empty()
        || !new.peer_ip.is_empty()
        || !new.vector_id.is_empty()
        || new.session_started_at <= 0
        || new.session_started_at != new.last_seen_at
        || new.sealed_id.is_empty()
        || new.session_genesis.is_empty()
        || new.trace_id.is_empty()
        || old.is_some_and(|old| {
            new.sealed_id == old.sealed_id
                || new.session_genesis == old.session_genesis
                || new.trace_id == old.trace_id
                || new.btrfs_device_json == old.btrfs_device_json
                || new.instance_json == old.instance_json
        })
        || provision.provenance_json.len() > 32768
    {
        return Err(err("invalid clean same-key replacement intent"));
    }
    validate_pair(sid, Some(new), Some(&provision.genesis))?;
    let claims =
        op_identity::sealed_id::SealedId::from_inline_ref(&new.sealed_id).map_err(native)?;
    if claims.session_id != sid
        || claims.wireguard_pubkey != new.wireguard_pubkey
        || claims.principal_id != op_identity::session::derive_principal_id(&new.wireguard_pubkey)
        || claims.session_genesis != new.session_genesis
        || claims.trace_id != new.trace_id
        || i64::from(claims.schema_version) != new.schema_version
        || claims.expires_at != 0
        || claims.arrival_timestamp != provision.genesis.arrival_timestamp
        || claims.issued_at != provision.genesis.arrival_timestamp
        || claims.chain_head_at_arrival != provision.genesis.chain_head_at_arrival
        || claims.catalog_hash_at_arrival != provision.genesis.catalog_hash_at_arrival
        || claims.head_timestamp_at_arrival != provision.genesis.head_timestamp_at_arrival
    {
        return Err(err(
            "fresh authored credential does not match clean record/genesis",
        ));
    }
    let device: Value = serde_json::from_str(&new.btrfs_device_json)?;
    let instance: Value = serde_json::from_str(&new.instance_json)?;
    if device["cozo_id"] != sid
        || device["attached"] != true
        || instance["status"] != "Stopped"
        || instance["type"] != "container"
        || !(instance["name"] == "3tchedChatbot" || instance["name"] == "3tchedCtlChatbot"
            || instance["name"].as_str().is_some_and(is_numbered_native_name))
        || !(provenance["version"] == if explicitly_deleted { 3 } else { 2 }
            || (provenance["version"] == 4 && (fresh_partitioned || explicitly_deleted)))
        || provenance["session_id"] != sid
        || provenance["wireguard_pubkey"] != new.wireguard_pubkey
    {
        return Err(err("invalid clean provision binding"));
    }
    if previous.operation_id == operation {
        let receipt =
            read(tx, sid)?.ok_or_else(|| err("clean provision replay receipt missing"))?;
        if metadata.kind != "clean-provision"
            || metadata.value != provision.expected.revision as i64
            || previous.record.as_ref() != Some(new)
            || previous.genesis.as_ref() != Some(&provision.genesis)
            || receipt.plan_sha256 != provision.plan_sha256
            || receipt.provenance_json != provision.provenance_json
        {
            return Err(err("clean provision operation reused for another intent"));
        }
    } else if previous != &provision.expected {
        return Err(err("clean provision compare/exchange conflict"));
    }
    Ok(())
}

pub(super) fn append(
    tx: &MultiTransaction,
    sid: &str,
    operation: &str,
    provision: &IdentityCleanProvision,
) -> std::result::Result<(), CozoError> {
    let revision = provision
        .expected
        .revision
        .checked_add(1)
        .filter(|v| *v <= i64::MAX as u64)
        .ok_or_else(|| err("identity revision overflow"))?;
    let root = read_root_repair_journal_in_tx(tx, sid)?;
    let seed = read_seed_provision_journal_in_tx(tx, sid)?;
    if root
        .as_ref()
        .is_some_and(|j| j.phase != "identity-committed")
        || seed
            .as_ref()
            .is_some_and(|j| j.phase != "identity-committed")
    {
        return Err(err(
            "clean provision cannot supersede incomplete storage intent",
        ));
    }
    let history = serde_json::to_string(&(root, seed, physical_root::read_in_tx(tx, sid)?))?;
    let params = BTreeMap::from([
        ("sid".into(), DataValue::Str(sid.into())),
        ("revision".into(), dv_int(revision as i64)),
        (
            "plan".into(),
            DataValue::Str(provision.plan_sha256.as_str().into()),
        ),
        ("op".into(), DataValue::Str(operation.into())),
        (
            "expected".into(),
            dv_int(provision.expected.revision as i64),
        ),
        (
            "provenance".into(),
            DataValue::Str(provision.provenance_json.as_str().into()),
        ),
        (
            "device".into(),
            DataValue::Str(provision.record.btrfs_device_json.as_str().into()),
        ),
        (
            "instance".into(),
            DataValue::Str(provision.record.instance_json.as_str().into()),
        ),
        (
            "old".into(),
            DataValue::Str(serde_json::to_string(&provision.expected)?.into()),
        ),
        ("journals".into(), DataValue::Str(history.into())),
    ]);
    query(
        tx,
        "?[session_id,committed_revision,plan_sha256,operation_id,expected_revision,provenance_json,btrfs_device_json,instance_json,old_revision_json,old_journals_json] <- [[$sid,$revision,$plan,$op,$expected,$provenance,$device,$instance,$old,$journals]] :put identity_clean_provisions {session_id,committed_revision => plan_sha256,operation_id,expected_revision,provenance_json,btrfs_device_json,instance_json,old_revision_json,old_journals_json}",
        params,
    )?;
    for relation in [
        "identity_root_repairs",
        "identity_seed_provisions",
        "identity_physical_root_migrations",
    ] {
        query(
            tx,
            &format!("?[session_id] <- [[$sid]] :rm {relation} {{session_id}}"),
            sid_params(sid),
        )?;
    }
    Ok(())
}

/// Closed allowance for native power observations and the first Incus idmap application.
/// Native UUID, profile/device/config authority and all other fields remain pinned.
pub fn clean_provision_instance_binding_matches(pinned: &str, current: &str) -> bool {
    fn normalized(text: &str, pinned: &Value) -> Option<Value> {
        let mut value: Value = serde_json::from_str(text).ok()?;
        let status = value["status"].as_str()?;
        let expected_code = match status {
            "Stopped" => 102,
            "Running" => 103,
            _ => return None,
        };
        if value
            .get("status_code")
            .is_some_and(|code| code.as_i64() != Some(expected_code))
        {
            return None;
        }
        let object = value.as_object_mut()?;
        object.remove("status");
        object.remove("status_code");
        object.remove("last_used_at");
        for field in ["config", "expanded_config"] {
            let config = value[field].as_object_mut()?;
            let expected_idmap = [
                "volatile.idmap.next",
                "volatile.idmap.current",
                "volatile.last_state.idmap",
            ]
            .iter()
            .find_map(|key| pinned[field][*key].as_str().filter(|s| !s.is_empty()));
            for key in [
                "volatile.idmap.current",
                "volatile.idmap.next",
                "volatile.last_state.idmap",
            ] {
                if let Some(actual) = config
                    .get(key)
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                    && Some(actual) != expected_idmap
                    && Some(actual) != pinned[field][key].as_str()
                {
                    return None;
                }
            }
            for key in [
                "volatile.last_state.power",
                "volatile.last_state.ready",
                "volatile.idmap.current",
                "volatile.idmap.next",
                "volatile.last_state.idmap",
                "volatile.apply_template",
            ] {
                config.remove(key);
            }
        }
        Some(value)
    }
    let Ok(pin) = serde_json::from_str::<Value>(pinned) else {
        return false;
    };
    normalized(pinned, &pin)
        .zip(normalized(current, &pin))
        .is_some_and(|(a, b)| a == b)
}

pub(super) fn validate_cas(
    tx: &MultiTransaction,
    previous: &IdentityRevision,
    next: Option<&IdentitySledRecord>,
) -> std::result::Result<(), CozoError> {
    let Some(receipt) = read(tx, &previous.session_id)? else {
        return Ok(());
    };
    let Some(next) = next else {
        return Ok(());
    }; // Explicit revocation remains possible.
    let old = previous
        .record
        .as_ref()
        .ok_or_else(|| err("clean provision tombstone cannot be resurrected"))?;
    let mut old_device: Value = serde_json::from_str(&old.btrfs_device_json)?;
    let mut new_device: Value = serde_json::from_str(&next.btrfs_device_json)?;
    old_device
        .as_object_mut()
        .ok_or_else(|| err("invalid clean device"))?
        .remove("attached");
    new_device
        .as_object_mut()
        .ok_or_else(|| err("invalid clean device"))?
        .remove("attached");
    if previous.revision < receipt.committed_revision
        || old_device != new_device
        || !clean_provision_instance_binding_matches(&receipt.instance_json, &next.instance_json)
    {
        return Err(err("completed clean provision binding is immutable"));
    }
    Ok(())
}
