//! Read-only: print the fixed identity columns of one identity sled row as
//! canonical JSON. Live columns (mutation_index, last_seen_at, active,
//! expires_at, instance, btrfs_device) are deliberately left out.
//! Stopgap for provisioning (3tched-seed-launch) while the bridge does not
//! serve sled reads; run it against a copy of the DB, never the live one.
//! Usage: sled-row COZO_DB_PATH SESSION_ID
fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let db = args.next().ok_or_else(|| anyhow::anyhow!("usage: sled-row DB SESSION_ID"))?;
    let sid = args.next().ok_or_else(|| anyhow::anyhow!("usage: sled-row DB SESSION_ID"))?;
    let store = op_cozo_store::CozoGraphShuttle::new_persistent(std::path::PathBuf::from(db))?;
    let r = store
        .get_identity_sled(&sid)?
        .ok_or_else(|| anyhow::anyhow!("no identity sled row for {sid}"))?;
    anyhow::ensure!(r.sealed_id.starts_with("sid1:"), "row {sid} has no minted sealed_id");
    // serde_json::Value objects are BTreeMap-ordered, so the output is canonical.
    let fixed = serde_json::json!({
        "session_id": r.session_id,
        "wireguard_pubkey": r.wireguard_pubkey,
        "principal_kind": r.principal_kind,
        "genesis": r.session_genesis,
        "trace_id": r.trace_id,
        "schema_version": r.schema_version,
        "vector_id": r.vector_id,
        "sealed_id": r.sealed_id,
        "session_started_at": r.session_started_at,
    });
    println!("{}", serde_json::to_string(&fixed)?);
    Ok(())
}
