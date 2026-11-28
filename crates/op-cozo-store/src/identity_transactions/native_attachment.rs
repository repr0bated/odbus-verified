//! Native-pool adoption has a distinct intent: it never relaxes clean replacement.
use super::*;

fn device<'a>(instance: &'a Value, name: &str) -> Option<&'a Value> {
    instance["devices"].as_array()?.iter().find(|entry| entry["name"] == name).map(|entry| &entry["device"])
}

pub(super) fn validate(
    tx: &MultiTransaction, sid: &str, operation: &str, previous: &IdentityRevision,
    metadata: &RevisionMetadata, provision: &IdentityCleanProvision,
) -> std::result::Result<(), CozoError> {
    let plan: Value = serde_json::from_str(&provision.provenance_json)?;
    let row = &provision.record;
    let instance: Value = serde_json::from_str(&row.instance_json)?;
    let sled = device(&instance, "identity-sled").or_else(|| device(&instance, "sled"))
        .ok_or_else(|| err("native sled disk absent"))?;
    let caller_session = plan["caller_session_id"].as_str().ok_or_else(|| err("attachment caller absent"))?;
    let (caller, _) = read_one(tx, caller_session)?;
    let owner = caller.record.as_ref().ok_or_else(|| err("attachment owner absent"))?;
    if !valid_root_repair_digest(&provision.plan_sha256)
        || operation != format!("native-attachment-{}", provision.plan_sha256)
        || provision.provenance_json.len() > 32768
        || plan["version"] != 5 || plan["session_id"] != sid
        || plan["wireguard_pubkey"] != row.wireguard_pubkey
        || provision.expected.session_id != sid || row.session_id != sid
        || op_identity::session::derive_session_id(&row.wireguard_pubkey) != sid
        || owner.principal_kind.as_deref() != Some("human")
        || owner.sealed_id.is_empty() || plan["caller_genesis"] != owner.session_genesis
        || plan["caller_principal_id"] != op_identity::session::derive_principal_id(&owner.wireguard_pubkey)
        || instance["type"] != "container"
        || !matches!(instance["status"].as_str(), Some("Running" | "Stopped"))
        || row.active != (instance["status"] == "Running")
        || instance["config"]["user.opdbus.session_id"] != sid
        || instance["config"]["user.opdbus.wireguard_public_key"] != row.wireguard_pubkey
        || instance["config"]["security.privileged"] != "false"
        || instance["config"]["security.nesting"] != "false"
        || instance["config"]["security.idmap.isolated"] != "true"
        || device(&instance, "root").is_none_or(|root| root["pool"] != "3tched-pool")
        || sled["source"] != format!("identity-{sid}")
        || sled["pool"] != "3tched-pool"
        || sled["path"] != "/agent/user"
        || sled["readonly"] != "true"
    { return Err(err("invalid owner-bound native attachment")); }
    validate_pair(sid, Some(row), Some(&provision.genesis))?;
    match plan["creation_mode"].as_str() {
        Some("attach_existing_human") => {
            let old = provision.expected.record.as_ref().ok_or_else(|| err("human attachment requires existing identity"))?;
            if sid != caller_session || old.principal_kind.as_deref() != Some("human")
                || old.sealed_id.is_empty()
                || (!old.instance_json.is_empty() && !clean_provision_instance_binding_matches(&old.instance_json, &row.instance_json))
                || !old.btrfs_device_json.is_empty()
                || !clean_provision::attachment_preserves_identity(old, row)
                || provision.expected.genesis.as_ref() != Some(&provision.genesis)
                || !plan["parent_principal_id"].is_null()
                || !plan["creation_operation_id"].is_null()
            { return Err(err("human attachment cannot replace identity, storage or credentials")); }
        }
        Some("create_subsidiary") => {
            if provision.expected.revision != 0 || !provision.expected.operation_id.is_empty()
                || provision.expected.record.is_some() || provision.expected.genesis.is_some()
                || row.principal_kind.as_deref() != Some("service") || row.sealed_id.is_empty()
                || !row.btrfs_device_json.is_empty() || row.mutation_index != 0
                || row.expires_at != 0 || !row.interface.is_empty() || !row.peer_ip.is_empty()
                || !row.vector_id.is_empty() || row.session_started_at <= 0
                || row.session_started_at != row.last_seen_at
                || plan["parent_principal_id"] != plan["caller_principal_id"]
                || instance["config"]["user.opdbus.parent_principal_id"] != plan["parent_principal_id"]
            { return Err(err("subsidiary must be fresh and scoped to its human parent")); }
            let creation = native_creation::records(tx)?.into_iter().find(|record|
                plan["creation_operation_id"] == record.request.operation_id)
                .ok_or_else(|| err("subsidiary creation receipt absent"))?;
            let body: Value = serde_json::from_str(&creation.request.request_json)?;
            if creation.phase != "complete" || creation.request.owner_kind != "identity"
                || creation.request.logical_target != sid || creation.request.caller_session != caller_session
                || plan["caller_principal_id"] != creation.request.actor_id
                || plan["caller_genesis"] != creation.request.caller_genesis
                || instance["name"] != creation.name
                || instance["config"]["volatile.uuid"] != creation.native_uuid
                || body["config"]["user.opdbus.creation_mode"] != "create_subsidiary"
                || body["config"]["user.opdbus.golden_seed_name"] != plan["golden_seed_name"]
                || body["config"]["user.opdbus.golden_seed_uuid"] != plan["golden_subvolume_uuid"]
                || body["config"]["user.opdbus.pool_fsid"] != plan["pool_fsid"]
            { return Err(err("subsidiary native creation owner or golden source differs")); }
            let claims = op_identity::sealed_id::SealedId::from_inline_ref(&row.sealed_id).map_err(native)?;
            if claims.session_id != sid || claims.wireguard_pubkey != row.wireguard_pubkey
                || claims.principal_id != op_identity::session::derive_principal_id(&row.wireguard_pubkey)
                || claims.session_genesis != row.session_genesis || claims.trace_id != row.trace_id
                || claims.issued_at != provision.genesis.arrival_timestamp
                || claims.arrival_timestamp != provision.genesis.arrival_timestamp
                || claims.catalog_hash_at_arrival != provision.genesis.catalog_hash_at_arrival
                || claims.chain_head_at_arrival != provision.genesis.chain_head_at_arrival
                || claims.head_timestamp_at_arrival != provision.genesis.head_timestamp_at_arrival
                || i64::from(claims.schema_version) != row.schema_version || claims.expires_at != row.expires_at
            { return Err(err("subsidiary authored claims differ from durable record")); }
        }
        _ => return Err(err("native attachment requires explicit creation mode")),
    }
    if previous.operation_id == operation {
        let receipt = clean_provision::read(tx, sid)?.ok_or_else(|| err("native attachment replay receipt absent"))?;
        if metadata.kind != "native-attachment" || metadata.value != provision.expected.revision as i64
            || previous.record.as_ref() != Some(row) || previous.genesis.as_ref() != Some(&provision.genesis)
            || receipt.plan_sha256 != provision.plan_sha256 || receipt.provenance_json != provision.provenance_json
        { return Err(err("native attachment replay changed")); }
    } else if previous != &provision.expected {
        return Err(err("native attachment compare/exchange conflict"));
    }
    Ok(())
}
