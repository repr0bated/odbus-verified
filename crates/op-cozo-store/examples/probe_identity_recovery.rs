use op_cozo_store::CozoGraphShuttle;
use std::path::PathBuf;

fn main() {
    let db = std::env::var("IDENTITY_SLED_COZO_DB_PATH")
        .unwrap_or_else(|_| "/var/lib/op-dbus/identity-cozo".to_string());
    let store = match CozoGraphShuttle::new_persistent(PathBuf::from(&db)) {
        Ok(store) => store,
        Err(error) => {
            eprintln!("open failed: {error:#}");
            std::process::exit(1);
        }
    };
    match store.read_identity_recovery_state(256) {
        Ok(snapshot) => {
            println!("identities: {}", snapshot.identities.len());
            for revision in &snapshot.identities {
                let record = revision.record.as_ref();
                println!(
                    "session={} record={} genesis={} instance={} principal_kind={}",
                    revision.session_id,
                    record.is_some(),
                    revision.genesis.is_some(),
                    record
                        .map(|r| if r.instance_json.is_empty() { "absent" } else { "present" })
                        .unwrap_or("absent"),
                    record
                        .and_then(|r| r.principal_kind.as_deref())
                        .unwrap_or("absent"),
                );
            }
            for revision in &snapshot.identities {
                if let Some(record) = &revision.record {
                    if !record.instance_json.is_empty() {
                        let instance: serde_json::Value =
                            serde_json::from_str(&record.instance_json).unwrap_or_default();
                        let config = instance.get("config");
                        println!(
                            "instance name={} type={} cfg_session={} cfg_key_matches_record={} cfg_uuid={}",
                            instance.get("name").and_then(|v| v.as_str()).unwrap_or("?"),
                            instance.get("type").and_then(|v| v.as_str()).unwrap_or("?"),
                            config
                                .and_then(|c| c.get("user.opdbus.session_id"))
                                .and_then(|v| v.as_str())
                                .unwrap_or("absent"),
                            config
                                .and_then(|c| c.get("user.opdbus.wireguard_public_key"))
                                .and_then(|v| v.as_str())
                                .map(|k| k == record.wireguard_pubkey)
                                .unwrap_or(false),
                            config
                                .and_then(|c| c.get("volatile.uuid"))
                                .and_then(|v| v.as_str())
                                .unwrap_or("absent"),
                        );
                        println!(
                            "record session={} sealed_id={} schema_version={} expires_at={} key={}",
                            record.session_id,
                            !record.sealed_id.is_empty(),
                            record.schema_version,
                            record.expires_at, record.wireguard_pubkey,
                        );
                    }
                }
            }
            println!("events: {}", snapshot.events.len());
        }
        Err(error) => {
            eprintln!("read_identity_recovery_state failed: {error:#}");
            std::process::exit(1);
        }
    }
}
