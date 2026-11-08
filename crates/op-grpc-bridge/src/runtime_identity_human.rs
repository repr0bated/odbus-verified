//! Native human registry adapter. Read errors are never empty success; the
//! registry's existing Cozo owner remains distinct from identity and documents.
use super::*;
use op_cozo_store::HumanPrincipalRecord;
use op_plugins::state_plugins::human_principal::{HumanPrincipal, HumanPrincipalState};

fn principal(record: &HumanPrincipalRecord) -> HumanPrincipal {
    HumanPrincipal { principal_id: record.principal_id.clone(), human_pubkey: record.human_pubkey.clone(),
        display_alias: record.display_alias.clone(), registered_at: record.registered_at,
        revoked_at: (record.revoked_at != 0).then_some(record.revoked_at) }
}

pub(super) async fn dispatch(engine: &NativeOwner, method: &str, args: &JsonValue) -> anyhow::Result<JsonValue> {
    static OWNER: LazyLock<Arc<tokio::sync::Mutex<()>>> = LazyLock::new(|| Arc::new(tokio::sync::Mutex::new(())));
    engine.retain(OWNER.clone().lock_owned().await);
    engine.revalidate().await?;
    let cozo = crate::human_principal_registry::store().context("human principal native store unavailable")?;
    let method = method.to_owned();
    let args = args.clone();
    let lifetime = engine.lifetime.clone();
    let (result, observed) = tokio::task::spawn_blocking(move || {
        let operation = (|| -> anyhow::Result<JsonValue> {
            cozo.sync_runtime_document_store()?;
            let key = arg_str(&args, "human_pubkey");
            let alias = arg_str(&args, "display_alias");
            let target = arg_str(&args, "principal_id");
            let selected = match method.as_str() {
                "register_key" => {
                    canonical_key(key)?;
                    let all = cozo.list_human_principals()?;
                    let id = op_identity::session::derive_principal_id(key);
                    ensure!(!all.iter().any(|record| record.human_pubkey == key || record.principal_id == id),
                        "human key is already registered, including revoked tombstones");
                    ensure!(alias.is_empty() || !all.iter().any(|record| record.revoked_at == 0 && record.display_alias == alias),
                        "display alias belongs to another active principal");
                    let record = HumanPrincipalRecord { principal_id: id.clone(), human_pubkey: key.into(),
                        display_alias: alias.into(), registered_at: now(), revoked_at: 0 };
                    lifetime.effect.store(true, Ordering::Release);
                    cozo.put_human_principal(&record)?;
                    cozo.sync_runtime_document_store()?;
                    let actual = cozo.get_human_principal(&id)?.context("registered principal missing after native commit")?;
                    ensure!(actual.principal_id == record.principal_id && actual.human_pubkey == record.human_pubkey
                        && actual.display_alias == record.display_alias && actual.registered_at == record.registered_at
                        && actual.revoked_at == 0, "registered principal readback differs");
                    Some(actual)
                }
                "revoke_key" => {
                    let existing = cozo.get_human_principal_by_pubkey(key)?.context("human key is not registered")?;
                    if existing.revoked_at == 0 {
                        let revoked_at = now();
                        lifetime.effect.store(true, Ordering::Release);
                        cozo.revoke_human_principal(&existing.principal_id, revoked_at)?;
                        cozo.sync_runtime_document_store()?;
                        let actual = cozo.get_human_principal(&existing.principal_id)?.context("revoked principal missing after native commit")?;
                        ensure!(actual.revoked_at == revoked_at && actual.human_pubkey == existing.human_pubkey,
                            "principal revocation readback differs");
                        Some(actual)
                    } else { Some(existing) }
                }
                "set_alias" => {
                    let existing = cozo.get_human_principal(target)?.context("human principal is not registered")?;
                    if existing.revoked_at == 0 && !alias.is_empty() {
                        ensure!(!cozo.list_human_principals()?.iter().any(|record| record.revoked_at == 0
                            && record.principal_id != target && record.display_alias == alias),
                            "display alias belongs to another active principal");
                    }
                    lifetime.effect.store(true, Ordering::Release);
                    cozo.update_human_principal_alias(target, alias)?;
                    cozo.sync_runtime_document_store()?;
                    let actual = cozo.get_human_principal(target)?.context("retitled principal missing after native commit")?;
                    ensure!(actual.display_alias == alias && actual.human_pubkey == existing.human_pubkey
                        && actual.revoked_at == existing.revoked_at, "principal alias readback differs");
                    Some(actual)
                }
                "resolve_key" => cozo.get_human_principal_by_pubkey(key)?,
                "get_principal" => cozo.get_human_principal(target)?,
                "list_principals" => None,
                _ => anyhow::bail!("unknown human registry method"),
            };
            Ok(serde_json::json!({"principal": selected.as_ref().map(principal)}))
        })();
        // Even a failed write can have committed. One durable readback records
        // what happened; there is no retry or replacement with an empty list.
        let observed = (|| -> anyhow::Result<HumanPrincipalState> {
            cozo.sync_runtime_document_store()?;
            let records = cozo.list_human_principals()?;
            let mut principals = records.iter().map(principal).collect::<Vec<_>>();
            principals.sort_unstable_by(|a,b| a.principal_id.cmp(&b.principal_id));
            Ok(HumanPrincipalState { principals })
        })();
        let result = match (method.as_str(), operation, &observed) {
            ("list_principals", Ok(_), Ok(state)) => serde_json::to_value(state).map_err(Into::into),
            (_, operation, _) => operation,
        };
        (result, observed)
    }).await.context("human registry native owner failed")?;
    if !engine.lifetime.effect.load(Ordering::Acquire) {
        // Policy refusals before a write must remain before-effect refusals,
        // not unknown outcomes merely because a diagnostic list succeeded.
        if result.is_err() { return result; }
        engine.revalidate().await?;
    }
    match observed {
        Ok(state) => {
            let projections = project_native(&engine.runtime, &engine.caller, "human_principal", serde_json::to_value(state)?)?;
            engine.lifetime.observed.lock().expect("native observations poisoned").extend(projections);
        }
        Err(error) => return Err(error.context("human principal state readback unavailable")),
    }
    let result = result?;
    // A mutation may intentionally revoke its caller. The parent still commits
    // the native observation; disclosure revalidation is the runtime's job.
    Ok(result)
}
