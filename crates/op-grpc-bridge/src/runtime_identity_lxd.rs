//! Admission of LXD subsidiaries from their sled volume. The parent's durable
//! Cozo intent, the Decoy-issued IID1 and the exact live instance must agree;
//! nothing is minted here and Cozo stays an index of the volume.
use super::*;
use op_cozo_store::LxdSubsidiary;
use op_plugins::state_plugins::lxd::{self as native_lxd, LxdClient as _};
use std::path::Path;

pub(crate) const SOCKET: &str = "/var/lib/lxd/unix.socket";
const CUSTOM: &str = "/var/lib/lxd/storage-pools/lxd-pool/custom";

/// Host tmpfs the container receives as its own `/run`; the host mounts the
/// identity's 3tchedFS inside it, so the container needs no host socket.
pub(crate) fn run_source(session: &str) -> String {
    format!("/run/opdbus/subsidiary/{session}/run")
}

/// LXD's typed named-device list, as the map the shared conversion expects.
fn device_map(devices: &JsonValue) -> anyhow::Result<JsonValue> {
    let Some(list) = devices.as_array() else { return Ok(devices.clone()) };
    let mut map = serde_json::Map::new();
    for entry in list {
        let name = entry["name"].as_str().context("LXD device has no name")?;
        map.insert(name.into(), entry["device"].clone());
    }
    Ok(JsonValue::Object(map))
}

/// The live instance must equal the admitted request: no added device,
/// profile, raw option or policy change since the parent created it.
fn exact_instance(row: &LxdSubsidiary, mut live: JsonValue) -> anyhow::Result<IncusInstance> {
    let want: JsonValue = serde_json::from_str(&row.request_json)?;
    ensure!(live["name"] == row.name && live["type"] == "container"
        && live["profiles"] == serde_json::json!([])
        && matches!(live["status"].as_str(), Some("Running" | "Stopped")),
        "LXD subsidiary is not its own profile-free container");
    let devices = device_map(&want["devices"])?;
    for field in ["devices", "expanded_devices"] {
        let map = device_map(&live[field])?;
        ensure!(map == devices, "LXD subsidiary devices changed");
        live[field] = map;
    }
    let config = want["config"].as_object().context("admitted config absent")?;
    for field in ["config", "expanded_config"] {
        let current = live[field].as_object().context("LXD config absent")?;
        ensure!(config.iter().all(|(key, value)| current.get(key) == Some(value))
            && current.keys().all(|key| config.contains_key(key)
                || key.starts_with("volatile.") || key.starts_with("image.")),
            "LXD subsidiary configuration changed");
    }
    let instance = IncusPlugin::instance_from_native_metadata(live)?;
    let uuid = instance.config.as_ref().and_then(|c| c.get("volatile.uuid"))
        .context("LXD subsidiary UUID absent")?;
    ensure!(row.native_uuid.is_empty() || row.native_uuid == *uuid, "LXD subsidiary was replaced");
    Ok(instance)
}

/// Admit or refresh one subsidiary. `Ok(false)` when it has no LXD intent or
/// nothing changed; the caller republishes the identity projection on `true`.
pub(crate) async fn adopt(cozo: Arc<CozoGraphShuttle>, session: &str) -> anyhow::Result<bool> {
    let reader = cozo.clone();
    let rows = tokio::task::spawn_blocking(move || reader.lxd_subsidiaries()).await??;
    let Some(row) = rows.into_iter().find(|row| row.session_id == session) else { return Ok(false) };
    let client = native_lxd::Client::local(Path::new(SOCKET))?;
    let live = client.get_instance(&row.name).await?.0;
    let instance = exact_instance(&row, serde_json::to_value(live)?)?;
    tokio::task::spawn_blocking(move || commit(&cozo, &row, instance)).await?
}

fn commit(cozo: &CozoGraphShuttle, row: &LxdSubsidiary, instance: IncusInstance) -> anyhow::Result<bool> {
    let session = row.session_id.as_str();
    ensure!(configured_chatbot_key().as_deref() != Some(row.wireguard_pubkey.as_str()),
        "the chatbot is not a subsidiary");
    let parent = revision_sled(&cozo.read_identity_revision(&row.caller_session)?)?
        .context("subsidiary parent is absent")?;
    ensure!(parent.active && parent.principal_kind.as_deref() == Some("human")
        && op_identity::session::derive_principal_id(&parent.wireguard_pubkey) == row.actor_id,
        "subsidiary parent is not its live human");
    let dir = Path::new(CUSTOM).join(format!("default_identity-{session}"));
    let read = |name: &str| std::fs::read_to_string(dir.join(name))
        .with_context(|| format!("LXD sled has no {name}"));
    ensure!(read("session_id")?.trim() == session, "sled session_id differs from its volume name");
    let trust_path = std::env::var("OP_DECOY_TRUST_STORE")
        .unwrap_or_else(|_| crate::oracle_assertion::DEFAULT_DECOY_TRUST_STORE.into());
    let trust = std::fs::read(&trust_path).with_context(|| format!("reading {trust_path}"))?;
    let claims = verify_issued_claims(&read("issued_identity.iid1")?, &trust, session, &row.wireguard_pubkey, 0)?;
    let inline = claims.to_inline_ref().map_err(|error| anyhow::anyhow!("seal identity: {error}"))?;
    ensure!(read("sealed_id")?.trim() == inline, "sled sealed_id differs from the decoy-issued identity");
    let uuid = instance.config.as_ref().and_then(|c| c.get("volatile.uuid")).cloned()
        .context("LXD subsidiary UUID absent")?;
    cozo.bind_lxd_subsidiary(session, &uuid)?;
    let relation = op_plugins::state_plugins::identity_sled::OrgRelation {
        kind: op_plugins::state_plugins::identity_sled::OrgRelationKind::Subsidiary,
        name: row.name.clone(),
        parent: Some(row.actor_id.clone()),
    };
    commit_volume_identity(cozo, &row.wireguard_pubkey, &claims, &inline, instance, None, Some(relation))
}
