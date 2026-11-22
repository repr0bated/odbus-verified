//! One admitted argv execution inside the exact owned Incus guest, never the host.
use super::*;
use op_cozo_store::{ContainerExecJournal, ContainerExecRequest};
use op_plugins::state_plugins::identity_sled::{ExecuteInOwnedContainerInput, ExecuteInOwnedContainerOutput};
use sha2::{Digest as _, Sha256};

const CAPABILITY: &str = "cap.service.identity-sled.container.exec@v1";
const OUTPUT_BYTES: usize = 1024 * 1024;

fn validate(input: &ExecuteInOwnedContainerInput) -> anyhow::Result<()> {
    for value in [&input.operation_id, &input.session_id] {
        ensure!(uuid::Uuid::parse_str(value)?.to_string() == *value, "noncanonical guest execution UUID");
    }
    ensure!(!input.command.is_empty() && input.command.len() <= 128
        && !input.command[0].is_empty() && input.command.iter().all(|arg| arg.len() <= 16384 && !arg.contains('\0'))
        && serde_json::to_vec(&input.command)?.len() <= 60000,
        "guest command exceeds argv count, byte or NUL bounds");
    Ok(())
}

fn bound_output(mut text: String) -> (String, u64, bool) {
    let bytes = text.len() as u64;
    let truncated = text.len() > OUTPUT_BYTES;
    if truncated {
        let mut end = OUTPUT_BYTES;
        while !text.is_char_boundary(end) { end -= 1; }
        text.truncate(end);
    }
    (text, bytes, truncated)
}

async fn root_receipt(store: &IdentitySledStore, target: &str) -> anyhow::Result<String> {
    let cozo = store.cozo.clone(); let target = target.to_owned();
    tokio::task::spawn_blocking(move || -> anyhow::Result<String> {
        if let Some(upgrade) = cozo.read_native_root_upgrade(&target)? {
            ensure!(upgrade.committed_revision != 0 && upgrade.phase == "identity-committed",
                "guest root upgrade is unresolved");
            return Ok(serde_json::json!({"operation_id":upgrade.operation_id,"plan_sha256":upgrade.plan_sha256,
                "new_root_uuid":upgrade.new_root_uuid,"committed_revision":upgrade.committed_revision}).to_string());
        }
        let receipt = cozo.read_clean_provision(&target)?.context("guest exec requires its committed root provenance")?;
        Ok(serde_json::json!({"plan_sha256":receipt.plan_sha256,"committed_revision":receipt.committed_revision,
            "provenance_sha256":format!("{:x}",Sha256::digest(receipt.provenance_json.as_bytes()))}).to_string())
    }).await?
}

async fn prove_target(engine: &NativeOwner, store: &IdentitySledStore, target: &str,
    pinned: Option<&ContainerExecRequest>,
) -> anyhow::Result<(ContainerIdentitySled, String)> {
    engine.revalidate().await?;
    let expected = load_revision(store, target).await?;
    let mut sled = revision_sled(&expected)?.context("guest execution identity absent")?;
    admit_lifecycle_owner(engine, store, &sled).await?;
    ensure!(sled.active && sled.is_anchored(), "guest execution requires its anchored running identity");
    let guard = prepare_identity_root_start(store, &expected, &mut sled, &native_identity_storage_verifier()).await?
        .context("guest execution root provenance guard absent")?;
    engine.retain(guard);
    let bound = sled.instance.as_ref().context("guest execution native binding absent")?;
    let (raw, _) = IncusRootRepairBackend::local().instance(&bound.name).await?;
    let actual = IncusPlugin::instance_from_native_metadata(raw.clone())?;
    ensure!(actual.status == "Running" && identity_instance_matches(&sled, &actual)
        && clean_provision::binding_matches(&serde_json::to_string(bound)?, &serde_json::to_string(&actual)?),
        "guest native UUID or isolation authority changed");
    if configured_chatbot_key().as_deref() != Some(sled.wireguard_pubkey.as_str()) {
        native_attachment::validate_instance(&raw, &sled.session_id, &sled.wireguard_pubkey,
            raw["config"]["user.opdbus.parent_principal_id"].as_str())?;
    }
    let root = root_receipt(store, target).await?;
    if let Some(pin) = pinned {
        ensure!(pin.target_root_receipt == root
            && raw["config"]["volatile.uuid"] == pin.native_uuid
            && clean_provision::binding_matches(&pin.target_instance_json, &serde_json::to_string(&actual)?),
            "guest execution pinned root, UUID or native definition changed");
    }
    admit_lifecycle_owner(engine, store, &sled).await?;
    Ok((sled, root))
}

async fn persist_result(store: &IdentitySledStore, journal: ContainerExecJournal,
    result: JsonValue, unknown: bool,
) -> anyhow::Result<ContainerExecJournal> {
    let cozo = store.cozo.clone();
    let json = serde_json::to_string(&result)?;
    Ok(tokio::task::spawn_blocking(move || cozo.finish_container_exec(&journal, &json, unknown)).await??)
}

pub(super) async fn execute(engine: &NativeOwner, store: &IdentitySledStore, args: &JsonValue) -> anyhow::Result<JsonValue> {
    let input: ExecuteInOwnedContainerInput = serde_json::from_value(args.clone())?;
    validate(&input)?;
    crate::method_contract_admission::admit_compiled_method("identity_sled", "execute_in_owned_container", Some(CAPABILITY))?;
    let seal = strict_identity_contract()?;
    engine.retain(store.lifecycle_lock(&input.session_id).await.lock_owned().await);
    let (sled, root) = prove_target(engine, store, &input.session_id, None).await?;
    let instance = sled.instance.as_ref().context("guest native binding absent")?;
    let native_uuid = instance.config.as_ref().and_then(|config| config.get("volatile.uuid"))
        .context("guest native UUID absent")?.clone();
    let arguments_json = serde_json::to_string(&input)?;
    let request = ContainerExecRequest {
        operation_id:input.operation_id.clone(), actor_id:engine.caller.principal_id.clone(),
        caller_session:engine.caller.session_id.clone(), caller_genesis:engine.caller.session_genesis.clone(),
        target_session:input.session_id.clone(), native_uuid, schema_hash:seal.schema_hash.clone(),
        arguments_sha256:format!("{:x}", Sha256::digest(arguments_json.as_bytes())), arguments_json,
        target_instance_json:serde_json::to_string(instance)?, target_root_receipt:root,
    };
    let cozo = store.cozo.clone(); let requested = request.clone();
    let (journal, fresh) = tokio::task::spawn_blocking(move || cozo.reserve_container_exec(requested)).await??;
    if !fresh {
        ensure!(journal.phase == "complete", "guest execution outcome is unknown; original operation must not be repeated");
        prove_target(engine, store, &input.session_id, Some(&request)).await?;
        let mut result: JsonValue = serde_json::from_str(&journal.result_json)?;
        result["replayed"] = true.into();
        return Ok(result);
    }
    store.mark_effect();
    prove_target(engine, store, &input.session_id, Some(&request)).await?;
    ensure!(strict_identity_contract()?.schema_hash == request.schema_hash, "guest execution schema pin changed");
    let outcome = op_plugins::state_plugins::incus::dispatch_incus_method_observed("exec_instance",
        &serde_json::json!({"name":instance.name,"command":input.command})).await;
    let result = match outcome {
        Ok(observation) => {
            let native: op_plugins::state_plugins::incus::ExecInstanceOutput = serde_json::from_value(observation.result)?;
            ensure!(native.completed && native.name == instance.name, "guest execution result target or completion differs");
            let (stdout, stdout_bytes, stdout_truncated) = bound_output(native.stdout);
            let (stderr, stderr_bytes, stderr_truncated) = bound_output(native.stderr);
            serde_json::to_value(ExecuteInOwnedContainerOutput {
                operation_id:input.operation_id, session_id:input.session_id.clone(), native_uuid:request.native_uuid.clone(),
                exit_code:native.exit_code, stdout, stderr, stdout_bytes, stderr_bytes,
                stdout_truncated, stderr_truncated, replayed:false,
            })?
        }
        Err(error) => {
            persist_result(store, journal, serde_json::json!({"operation_id":input.operation_id,
                "phase":"unknown","error":error.to_string()}), true).await?;
            return Err(error.context("guest execution outcome recorded as unknown; POST must not be repeated"));
        }
    };
    // Persist actual process evidence even if authority is revoked during the
    // command. Revalidation controls disclosure, never retries the process.
    let completed = persist_result(store, journal, result, false).await?;
    prove_target(engine, store, &input.session_id, Some(&request)).await?;
    ensure!(strict_identity_contract()?.schema_hash == request.schema_hash, "guest execution schema changed after effect");
    Ok(serde_json::from_str(&completed.result_json)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn input() -> ExecuteInOwnedContainerInput {
        ExecuteInOwnedContainerInput {operation_id:"00000000-0000-4000-8000-000000000001".into(),
            session_id:"fb86888e-d6ab-d4f7-6278-9fe116c46b1c".into(),
            command:vec!["/bin/sh".into(), "-c".into(), "printf '%s' 'literal;$(not-host-shell)'".into()]}
    }
    #[test]
    fn guest_exec_preserves_argv_and_rejects_malformed_bounds() {
        let original = input(); assert!(validate(&original).is_ok());
        let decoded: ExecuteInOwnedContainerInput = serde_json::from_str(&serde_json::to_string(&original).unwrap()).unwrap();
        assert_eq!(decoded.command, original.command);
        for command in [vec![], vec![String::new()], vec!["a\0b".into()], vec!["a".repeat(16385)], vec!["x".into();129]] {
            let mut bad = input(); bad.command = command; assert!(validate(&bad).is_err());
        }
        let mut bad = input(); bad.session_id = "../host".into(); assert!(validate(&bad).is_err());
    }
    #[test]
    fn guest_output_truncation_is_explicit_and_preserves_utf8() {
        let text = "é".repeat(OUTPUT_BYTES); let (bounded, original_bytes, truncated) = bound_output(text);
        assert!(truncated); assert_eq!(original_bytes, (OUTPUT_BYTES * 2) as u64);
        assert!(bounded.len() <= OUTPUT_BYTES); assert!(bounded.ends_with('é'));
        let (text, bytes, truncated) = bound_output("actual output".into());
        assert_eq!((text.as_str(), bytes, truncated), ("actual output",13,false));
    }
}
