//! Explicit retirement of the configured chatbot's current record before fresh provisioning.
//! Keys, grants, native storage and historical audit records remain owned by their owners.
use super::*;
use op_plugins::state_plugins::incus::IncusRootRepairBackend;
use serde::Deserialize;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct DeleteInput {
    pub wireguard_pubkey: String,
    pub expected_revision: u64,
    pub expected_revision_operation_id: String,
}

fn check_expected(
    input: &DeleteInput,
    expected: &IdentityRevision,
) -> anyhow::Result<ContainerIdentitySled> {
    anyhow::ensure!(
        input.expected_revision > 0 && !input.expected_revision_operation_id.is_empty(),
        "deletion requires an exact existing revision"
    );
    anyhow::ensure!(
        expected.revision == input.expected_revision
            && expected.operation_id == input.expected_revision_operation_id,
        "identity deletion revision changed"
    );
    let sled = revision_sled(expected)?.context("identity record is absent")?;
    anyhow::ensure!(
        sled.wireguard_pubkey == input.wireguard_pubkey
            && sled.principal_kind.as_deref() == Some("service"),
        "only the configured service record may be deleted"
    );
    Ok(sled)
}

fn check_native(
    native: &JsonValue,
    name: &str,
    native_uuid: &str,
    etag: &str,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        native["name"] == name
            && native["type"] == "container"
            && native["status"] == "Stopped"
            && native["status_code"] == 102
            && native["config"]["volatile.uuid"].as_str() == Some(native_uuid)
            && !etag.is_empty(),
        "native container must remain stopped with the committed UUID"
    );
    Ok(())
}

/// Absence is proved by the complete native default-project inventory, never
/// inferred from a failed request (which could be a transport failure).
pub(super) async fn retirement_native(
    bound: &IncusInstance,
) -> anyhow::Result<Option<(JsonValue, String)>> {
    let backend = IncusRootRepairBackend::local();
    let inventory = backend.default_project_state().await?;
    if !inventory
        .instances
        .iter()
        .any(|instance| instance.name == bound.name)
    {
        return Ok(None);
    }
    let native_uuid = bound
        .config
        .as_ref()
        .and_then(|c| c.get("volatile.uuid"))
        .context("previous native UUID is absent")?;
    let (native, etag) = backend.instance(&bound.name).await?;
    check_native(&native, &bound.name, native_uuid, &etag)?;
    Ok(Some((native, etag)))
}

pub(super) async fn delete(
    engine: &MutationEngine,
    store: &IdentitySledStore,
    actor: VerifiedIdentityActor<'_>,
    input: DeleteInput,
    seal: seed_provision::ProvisionSealPin,
) -> anyhow::Result<JsonValue> {
    let key_bytes = base64::engine::general_purpose::STANDARD.decode(&input.wireguard_pubkey)?;
    anyhow::ensure!(
        key_bytes.len() == 32
            && base64::engine::general_purpose::STANDARD.encode(key_bytes)
                == input.wireguard_pubkey,
        "noncanonical public key"
    );
    let sid = op_identity::session::derive_session_id(&input.wireguard_pubkey);
    anyhow::ensure!(
        configured_chatbot_key().as_deref() == Some(input.wireguard_pubkey.as_str())
            && control_plane_chatbot_session_id().as_deref() == Some(sid.as_str())
            && actor.session_id != sid,
        "deletion is restricted to the configured chatbot, never the caller"
    );
    let _lifecycle = store.lifecycle_lock(&sid).await.lock_owned().await;
    let _host = engine.lock_host_observation().await;
    seed_provision::admit(
        engine,
        store,
        actor.actor_id,
        actor.session_id,
        actor.genesis,
        &seal,
    )
    .await?;
    let caller = load_revision(store, actor.session_id).await?;
    anyhow::ensure!(
        caller
            .record
            .as_ref()
            .is_some_and(|r| r.principal_kind.as_deref() == Some("human")),
        "record deletion requires a verified human caller"
    );
    seed_provision::target_key_is_registered(&input.wireguard_pubkey).await?;
    let expected = load_revision(store, &sid).await?;
    let operation = format!("delete-provision-{sid}-{}", input.expected_revision);
    if expected.operation_id == operation
        && expected.record.is_none()
        && expected.genesis.is_none()
        && input.expected_revision.checked_add(1) == Some(expected.revision)
    {
        recover_identity_startup_with_store(engine, store).await?;
        return Ok(
            serde_json::json!({"session_id":sid,"operation_id":operation,"committed_revision":expected.revision,"record_present":false,"replayed":true}),
        );
    }
    let sled = check_expected(&input, &expected)?;
    let cozo = store.cozo.clone();
    let check_sid = sid.clone();
    tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
        anyhow::ensure!(
            cozo.read_root_repair_journal(&check_sid)?
                .is_none_or(|j| j.phase == "identity-committed")
                && cozo
                    .read_seed_provision_journal(&check_sid)?
                    .is_none_or(|j| j.phase == "identity-committed"),
            "incomplete storage intent blocks record deletion"
        );
        Ok(())
    })
    .await??;
    let bound = sled.instance.context("previous native binding is absent")?;
    anyhow::ensure!(
        bound.name == sid
            || matches!(
                bound.name.as_str(),
                "Chatbot3tched" | "3tchedCtlbot" | "3tchedChatbot"
            ),
        "native binding is outside the supported chatbot retirement scope"
    );
    let native = retirement_native(&bound).await?;
    preflight_native_identity_effect().await?;
    seed_provision::admit(
        engine,
        store,
        actor.actor_id,
        actor.session_id,
        actor.genesis,
        &seal,
    )
    .await?;
    let final_native = retirement_native(&bound).await?;
    anyhow::ensure!(
        final_native == native,
        "native container changed during deletion preflight"
    );
    seed_provision::admit(
        engine,
        store,
        actor.actor_id,
        actor.session_id,
        actor.genesis,
        &seal,
    )
    .await?;
    let revision = expected
        .revision
        .checked_add(1)
        .context("identity revision overflow")?;
    let result = serde_json::json!({"session_id":sid,"operation_id":operation,"committed_revision":revision,"record_present":false,"replayed":false});
    commit_cas(engine, store, operation, sid, expected, None, None, result).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deletion_requires_exact_stopped_native_identity() {
        let native = serde_json::json!({"name":"chatbot","type":"container","status":"Stopped","status_code":102,"config":{"volatile.uuid":"pinned"}});
        assert!(check_native(&native, "chatbot", "pinned", "etag").is_ok());
        for (field, value) in [
            ("status", serde_json::json!("Running")),
            ("status_code", serde_json::json!(103)),
            ("name", serde_json::json!("unrelated")),
            ("type", serde_json::json!("virtual-machine")),
        ] {
            let mut changed = native.clone();
            changed[field] = value;
            assert!(check_native(&changed, "chatbot", "pinned", "etag").is_err());
        }
        assert!(check_native(&native, "chatbot", "foreign", "etag").is_err());
        assert!(check_native(&native, "chatbot", "pinned", "").is_err());
    }

    #[test]
    fn deletion_rejects_missing_or_stale_record() {
        let revision = IdentityRevision {
            session_id: "test".into(),
            revision: 5,
            operation_id: "old".into(),
            record: None,
            genesis: None,
        };
        let mut input = DeleteInput {
            wireguard_pubkey: "key".into(),
            expected_revision: 5,
            expected_revision_operation_id: "old".into(),
        };
        assert!(check_expected(&input, &revision).is_err());
        input.expected_revision = 4;
        assert!(check_expected(&input, &revision)
            .unwrap_err()
            .to_string()
            .contains("revision changed"));
    }
}
