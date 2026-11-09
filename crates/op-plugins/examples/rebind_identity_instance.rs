//! Re-bind an identity sled row to the Incus container that now carries it.
//!
//! Converts a native Incus instance record (`incus query /1.0/instances/NAME`)
//! with the bridge's own conversion, checks the same session/key/UUID binding
//! as `identity_instance_matches`, and replaces only the row's `instance_json`
//! through a compare/exchange identity intent.
//! Dry-run unless `--write`. Must run while op-grpc-bridge does not hold the DB.
//!
//! Usage: rebind_identity_instance COZO_DB SESSION_ID NATIVE_INSTANCE_JSON [--write]

use op_plugins::state_plugins::IncusPlugin;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    anyhow::ensure!(
        (3..=4).contains(&args.len()) && args.get(3).is_none_or(|flag| flag == "--write"),
        "usage: rebind_identity_instance COZO_DB SESSION_ID NATIVE_INSTANCE_JSON [--write]"
    );
    let (db, sid, native_path, write) = (&args[0], &args[1], &args[2], args.len() == 4);

    let native: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(native_path)?)?;
    let instance = IncusPlugin::instance_from_native_metadata(native)?;
    let store = op_cozo_store::CozoGraphShuttle::new_persistent(std::path::PathBuf::from(db))?;
    let mut row = store
        .get_identity_sled(sid)?
        .ok_or_else(|| anyhow::anyhow!("no identity sled row for {sid}"))?;

    let config = instance.config.clone().unwrap_or_default();
    anyhow::ensure!(instance.instance_type == "container", "instance is not a container");
    anyhow::ensure!(
        config.get("user.opdbus.session_id") == Some(&row.session_id),
        "instance user.opdbus.session_id does not match the row"
    );
    anyhow::ensure!(
        config.get("user.opdbus.wireguard_public_key") == Some(&row.wireguard_pubkey),
        "instance user.opdbus.wireguard_public_key does not match the row"
    );
    let uuid = config
        .get("volatile.uuid")
        .filter(|value| uuid::Uuid::parse_str(value).is_ok())
        .ok_or_else(|| anyhow::anyhow!("instance has no valid volatile.uuid"))?;
    // The bridge also requires the key to derive the session; that is a property
    // of the existing row, which this tool never changes.
    anyhow::ensure!(
        row.principal_kind.as_deref() == Some("service"),
        "row is not a service identity"
    );

    let old: serde_json::Value = serde_json::from_str(&row.instance_json).unwrap_or_default();
    eprintln!(
        "{sid}: {} ({}) -> {} ({uuid})",
        old["name"].as_str().unwrap_or("-"),
        old["config"]["volatile.uuid"].as_str().unwrap_or("-"),
        instance.name
    );
    if !write {
        eprintln!("dry run; pass --write to update the row");
        return Ok(());
    }
    // Versioned identities change only through an atomic intent: a
    // compare/exchange against the current revision that replaces instance_json.
    let expected = store.read_identity_revision(sid)?;
    anyhow::ensure!(
        expected.record.as_ref().is_some_and(|current| current.instance_json == row.instance_json),
        "identity changed while preparing the rebind"
    );
    row.instance_json = serde_json::to_string(&instance)?;
    let genesis = expected.genesis.clone();
    let operation = format!("rebind-instance-{}", &uuid[..8]);
    store.apply_identity_intent(
        sid,
        &operation,
        op_cozo_store::IdentityIntent::CompareExchange(Box::new(op_cozo_store::IdentityCompareExchange {
            expected,
            record: Some(row.clone()),
            genesis,
        })),
    )?;
    let check = store.get_identity_sled(sid)?.ok_or_else(|| anyhow::anyhow!("row vanished"))?;
    anyhow::ensure!(check.instance_json == row.instance_json, "write did not persist");
    eprintln!("written");
    Ok(())
}
