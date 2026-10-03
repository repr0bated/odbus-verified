//! The system-wide model registry: every configured provider's refresh
//! status and every model a provider has listed, in a dedicated Cozo owner.
//!
//! A successful refresh replaces a provider's availability with exactly what
//! it listed: new models are added, listed models are marked available, and
//! models it no longer lists are kept but marked unavailable, so their first
//! and last sighting survive. A failed refresh changes only the provider's
//! status; its last good list stays. A provider removed from the
//! configuration is marked `removed` and its models unavailable. Every
//! refresh advances the registry revision. A model's Control Plane profile,
//! recorded by `set_model_registry_profile`, lives in its own relation so a
//! refresh replaces the provider's list without erasing that record. The
//! model_registry plugin owns the typed contract; this module stores what
//! it is given.

use std::collections::BTreeSet;

use super::identity_transactions::commit_checked;
use super::*;
use cozo::MultiTransaction;

const MAX_ID_BYTES: usize = 200;
const MAX_TEXT_BYTES: usize = 300;
const PUT_CHUNK: usize = 100;
const MODEL_COLUMNS: &str = "provider_id, model_id, name, context_length, prompt_price, completion_price, available, first_seen, last_seen";
const PROVIDER_COLUMNS: &str =
    "provider_id, status, error, refreshed_at, succeeded_at, available_count, known_count";
const PROFILE_COLUMNS: &str = "provider_id, model_id, profile";

/// A model as a provider listed it.
#[derive(Debug, Clone, PartialEq)]
pub struct RegistryModel {
    pub model_id: String,
    pub name: Option<String>,
    pub context_length: Option<u64>,
    pub prompt_price: Option<String>,
    pub completion_price: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RegistryModelRecord {
    pub provider_id: String,
    pub model: RegistryModel,
    pub available: bool,
    pub first_seen: String,
    pub last_seen: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RegistryProviderRecord {
    pub provider_id: String,
    /// `ok`, `error` or `removed`.
    pub status: String,
    pub error: Option<String>,
    pub refreshed_at: String,
    pub succeeded_at: Option<String>,
    pub available_count: u64,
    pub known_count: u64,
}

/// What one provider's refresh changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RegistryRefreshCounts {
    pub available: u64,
    pub added: u64,
    pub dropped: u64,
}

fn error(message: &str) -> CozoError {
    CozoError::Other(message.into())
}

fn bounded(value: &str, maximum: usize) -> bool {
    !value.is_empty() && value.len() <= maximum && !value.chars().any(char::is_control)
}

fn text(value: &str) -> DataValue {
    DataValue::Str(value.into())
}

fn optional(value: Option<&str>) -> DataValue {
    text(value.unwrap_or(""))
}

fn int(value: u64) -> std::result::Result<DataValue, CozoError> {
    Ok(DataValue::Num(Num::Int(i64::try_from(value).map_err(
        |_| error("model registry integer exceeds its column"),
    )?)))
}

fn as_u64(value: &DataValue) -> std::result::Result<u64, CozoError> {
    match value {
        DataValue::Num(Num::Int(value)) if *value >= 0 => Ok(*value as u64),
        _ => Err(error("corrupt model registry integer column")),
    }
}

fn as_str(value: &DataValue) -> std::result::Result<String, CozoError> {
    match value {
        DataValue::Str(value) => Ok(value.to_string()),
        _ => Err(error("corrupt model registry text column")),
    }
}

fn as_bool(value: &DataValue) -> std::result::Result<bool, CozoError> {
    match value {
        DataValue::Bool(value) => Ok(*value),
        _ => Err(error("corrupt model registry flag column")),
    }
}

fn non_empty(value: String) -> Option<String> {
    (!value.is_empty()).then_some(value)
}

fn validate_model(provider_id: &str, model: &RegistryModel) -> std::result::Result<(), CozoError> {
    let optional_ok = |value: &Option<String>| {
        value
            .as_deref()
            .is_none_or(|value| bounded(value, MAX_TEXT_BYTES))
    };
    if !bounded(provider_id, MAX_ID_BYTES)
        || !bounded(&model.model_id, MAX_ID_BYTES)
        || !optional_ok(&model.name)
        || !optional_ok(&model.prompt_price)
        || !optional_ok(&model.completion_price)
    {
        return Err(error("invalid model registry row"));
    }
    Ok(())
}

fn run(
    tx: &MultiTransaction,
    script: &str,
    values: Params,
    what: &str,
) -> std::result::Result<NamedRows, CozoError> {
    tx.run_script(script, values)
        .map_err(|error| CozoError::Other(format!("model registry {what} failed: {error}")))
}

fn model_rows(rows: NamedRows) -> std::result::Result<Vec<RegistryModelRecord>, CozoError> {
    rows.rows
        .into_iter()
        .map(|row| match row.as_slice() {
            [
                provider_id,
                model_id,
                name,
                context_length,
                prompt_price,
                completion_price,
                available,
                first_seen,
                last_seen,
            ] => Ok(RegistryModelRecord {
                provider_id: as_str(provider_id)?,
                model: RegistryModel {
                    model_id: as_str(model_id)?,
                    name: non_empty(as_str(name)?),
                    context_length: Some(as_u64(context_length)?).filter(|length| *length > 0),
                    prompt_price: non_empty(as_str(prompt_price)?),
                    completion_price: non_empty(as_str(completion_price)?),
                },
                available: as_bool(available)?,
                first_seen: as_str(first_seen)?,
                last_seen: as_str(last_seen)?,
            }),
            _ => Err(error("corrupt model registry model row")),
        })
        .collect()
}

fn provider_models(
    tx: &MultiTransaction,
    provider_id: &str,
) -> std::result::Result<Vec<RegistryModelRecord>, CozoError> {
    model_rows(run(
        tx,
        &format!(
            "?[{MODEL_COLUMNS}] := *model_registry_models{{{MODEL_COLUMNS}}}, provider_id = $provider"
        ),
        BTreeMap::from([("provider".into(), text(provider_id))]),
        "provider model read",
    )?)
}

fn provider_rows(rows: NamedRows) -> std::result::Result<Vec<RegistryProviderRecord>, CozoError> {
    rows.rows
        .into_iter()
        .map(|row| match row.as_slice() {
            [
                provider_id,
                status,
                error_text,
                refreshed_at,
                succeeded_at,
                available_count,
                known_count,
            ] => Ok(RegistryProviderRecord {
                provider_id: as_str(provider_id)?,
                status: as_str(status)?,
                error: non_empty(as_str(error_text)?),
                refreshed_at: as_str(refreshed_at)?,
                succeeded_at: non_empty(as_str(succeeded_at)?),
                available_count: as_u64(available_count)?,
                known_count: as_u64(known_count)?,
            }),
            _ => Err(error("corrupt model registry provider row")),
        })
        .collect()
}

fn all_providers(
    tx: &MultiTransaction,
) -> std::result::Result<Vec<RegistryProviderRecord>, CozoError> {
    provider_rows(run(
        tx,
        &format!("?[{PROVIDER_COLUMNS}] := *model_registry_providers{{{PROVIDER_COLUMNS}}}"),
        BTreeMap::new(),
        "provider read",
    )?)
}

fn put_models(
    tx: &MultiTransaction,
    rows: &[RegistryModelRecord],
) -> std::result::Result<(), CozoError> {
    for chunk in rows.chunks(PUT_CHUNK) {
        let mut values = BTreeMap::new();
        let mut tuples = Vec::with_capacity(chunk.len());
        for (index, row) in chunk.iter().enumerate() {
            validate_model(&row.provider_id, &row.model)?;
            let columns = [
                text(&row.provider_id),
                text(&row.model.model_id),
                optional(row.model.name.as_deref()),
                int(row.model.context_length.unwrap_or(0))?,
                optional(row.model.prompt_price.as_deref()),
                optional(row.model.completion_price.as_deref()),
                DataValue::Bool(row.available),
                text(&row.first_seen),
                text(&row.last_seen),
            ];
            let names: Vec<String> = (0..columns.len())
                .map(|c| format!("$v{index}_{c}"))
                .collect();
            for (name, value) in names.iter().zip(columns) {
                values.insert(name[1..].to_string(), value);
            }
            tuples.push(format!("[{}]", names.join(", ")));
        }
        run(
            tx,
            &format!(
                "?[{MODEL_COLUMNS}] <- [{}] :put model_registry_models {{provider_id, model_id => name, context_length, prompt_price, completion_price, available, first_seen, last_seen}}",
                tuples.join(", ")
            ),
            values,
            "model write",
        )?;
    }
    Ok(())
}

fn put_provider(
    tx: &MultiTransaction,
    row: &RegistryProviderRecord,
) -> std::result::Result<(), CozoError> {
    if !bounded(&row.provider_id, MAX_ID_BYTES)
        || !matches!(row.status.as_str(), "ok" | "error" | "removed")
        || row
            .error
            .as_deref()
            .is_some_and(|text| !bounded(text, MAX_TEXT_BYTES))
    {
        return Err(error("invalid model registry provider row"));
    }
    run(
        tx,
        &format!(
            "?[{PROVIDER_COLUMNS}] <- [[$provider, $status, $error, $refreshed, $succeeded, $available, $known]] :put model_registry_providers {{provider_id => status, error, refreshed_at, succeeded_at, available_count, known_count}}"
        ),
        BTreeMap::from([
            ("provider".into(), text(&row.provider_id)),
            ("status".into(), text(&row.status)),
            ("error".into(), optional(row.error.as_deref())),
            ("refreshed".into(), text(&row.refreshed_at)),
            ("succeeded".into(), optional(row.succeeded_at.as_deref())),
            ("available".into(), int(row.available_count)?),
            ("known".into(), int(row.known_count)?),
        ]),
        "provider write",
    )?;
    Ok(())
}

fn revision(tx: &MultiTransaction) -> std::result::Result<u64, CozoError> {
    let rows = run(
        tx,
        "?[revision] := *model_registry_state{key, revision}, key = 'registry'",
        BTreeMap::new(),
        "revision read",
    )?;
    match rows.rows.as_slice() {
        [] => Ok(0),
        [row] => match row.as_slice() {
            [revision] => as_u64(revision),
            _ => Err(error("corrupt model registry revision")),
        },
        _ => Err(error("duplicate model registry revision")),
    }
}

fn advance(tx: &MultiTransaction) -> std::result::Result<u64, CozoError> {
    let next = revision(tx)?
        .checked_add(1)
        .ok_or_else(|| error("model registry revision exhausted"))?;
    run(
        tx,
        "?[key, revision] <- [['registry', $revision]] :put model_registry_state {key => revision}",
        BTreeMap::from([("revision".into(), int(next)?)]),
        "revision write",
    )?;
    Ok(next)
}

impl CozoGraphShuttle {
    /// Opt this handle into the model registry domain. Generic Cozo stores
    /// deliberately do not expose these relations.
    pub fn initialize_model_registry_store(&self) -> std::result::Result<(), CozoError> {
        let _guard = self
            .episode_transaction
            .lock()
            .map_err(|_| error("model registry owner is poisoned"))?;
        self.runtime_document_preflight()?;
        for relation in [
            r#":create model_registry_models {
                provider_id: String, model_id: String
                =>
                name: String,
                context_length: Int,
                prompt_price: String,
                completion_price: String,
                available: Bool,
                first_seen: String,
                last_seen: String
            }"#,
            r#":create model_registry_providers {
                provider_id: String
                =>
                status: String,
                error: String,
                refreshed_at: String,
                succeeded_at: String,
                available_count: Int,
                known_count: Int
            }"#,
            r#":create model_registry_state {
                key: String
                => revision: Int
            }"#,
            r#":create model_registry_profiles {
                provider_id: String, model_id: String
                =>
                profile: String
            }"#,
        ] {
            if let Err(error) =
                self.db
                    .run_script(relation, BTreeMap::new(), ScriptMutability::Mutable)
            {
                let message = error.to_string();
                if !message.contains("already exists")
                    && !message.contains("AlreadyExists")
                    && !message.contains("conflicts with an existing")
                {
                    return Err(CozoError::Other(format!(
                        "model registry relation initialization failed: {message}"
                    )));
                }
            }
        }
        for probe in [
            format!("?[{MODEL_COLUMNS}] := *model_registry_models{{{MODEL_COLUMNS}}} :limit 0"),
            format!(
                "?[{PROVIDER_COLUMNS}] := *model_registry_providers{{{PROVIDER_COLUMNS}}} :limit 0"
            ),
            "?[key, revision] := *model_registry_state{key, revision} :limit 0".to_string(),
            format!(
                "?[{PROFILE_COLUMNS}] := *model_registry_profiles{{{PROFILE_COLUMNS}}} :limit 0"
            ),
        ] {
            self.db
                .run_script(&probe, BTreeMap::new(), ScriptMutability::Immutable)
                .map_err(|error| {
                    CozoError::Other(format!(
                        "model registry relation verification failed: {error}"
                    ))
                })?;
        }
        self.sync_runtime_document_store()
    }

    fn model_registry_write<T>(
        &self,
        body: impl FnOnce(&MultiTransaction) -> std::result::Result<T, CozoError>,
    ) -> std::result::Result<T, CozoError> {
        let _guard = self
            .episode_transaction
            .lock()
            .map_err(|_| error("model registry owner is poisoned"))?;
        self.runtime_document_preflight()?;
        let tx = self.db.multi_transaction(true);
        let value = body(&tx)?;
        commit_checked(&tx)?;
        if let Err(e) = self.sync_runtime_document_store() {
            tracing::warn!("model registry sync after commit failed: {}", e);
        }
        Ok(value)
    }

    fn model_registry_read<T>(
        &self,
        body: impl FnOnce(&MultiTransaction) -> std::result::Result<T, CozoError>,
    ) -> std::result::Result<T, CozoError> {
        let tx = self.db.multi_transaction(false);
        let value = body(&tx);
        let _ = tx.abort();
        value
    }

    /// Record one provider's refresh at `at` (RFC 3339): its complete model
    /// list, or why it could not be read. Returns the counts and the new
    /// registry revision.
    pub fn record_model_registry_refresh(
        &self,
        provider_id: &str,
        at: &str,
        listed: std::result::Result<&[RegistryModel], &str>,
    ) -> std::result::Result<(RegistryRefreshCounts, u64), CozoError> {
        if !bounded(provider_id, MAX_ID_BYTES) || !bounded(at, MAX_ID_BYTES) {
            return Err(error("invalid model registry refresh key"));
        }
        self.model_registry_write(|tx| {
            let existing = provider_models(tx, provider_id)?;
            let previous = all_providers(tx)?
                .into_iter()
                .find(|row| row.provider_id == provider_id);
            let counts = match listed {
                Ok(models) => {
                    if models.is_empty() && !existing.is_empty() {
                        return Err(error(
                            "provider returned empty model list but previously had models"
                        ));
                    }
                    let known: BTreeMap<&str, &RegistryModelRecord> = existing
                        .iter()
                        .map(|row| (row.model.model_id.as_str(), row))
                        .collect();
                    let mut listed_ids = BTreeSet::new();
                    let mut rows = Vec::with_capacity(models.len());
                    let mut added = 0;
                    for model in models {
                        validate_model(provider_id, model)?;
                        if !listed_ids.insert(model.model_id.as_str()) {
                            return Err(error("model listed twice in one refresh"));
                        }
                        let before = known.get(model.model_id.as_str());
                        if !before.is_some_and(|row| row.available) {
                            added += 1;
                        }
                        rows.push(RegistryModelRecord {
                            provider_id: provider_id.to_string(),
                            model: model.clone(),
                            available: true,
                            first_seen: before
                                .map(|row| row.first_seen.clone())
                                .unwrap_or_else(|| at.to_string()),
                            last_seen: at.to_string(),
                        });
                    }
                    let mut dropped = 0;
                    for row in &existing {
                        if row.available && !listed_ids.contains(row.model.model_id.as_str()) {
                            dropped += 1;
                            rows.push(RegistryModelRecord {
                                available: false,
                                ..row.clone()
                            });
                        }
                    }
                    put_models(tx, &rows)?;
                    let known_count = existing
                        .iter()
                        .map(|row| row.model.model_id.as_str())
                        .chain(listed_ids.iter().copied())
                        .collect::<BTreeSet<_>>()
                        .len() as u64;
                    put_provider(
                        tx,
                        &RegistryProviderRecord {
                            provider_id: provider_id.to_string(),
                            status: "ok".into(),
                            error: None,
                            refreshed_at: at.to_string(),
                            succeeded_at: Some(at.to_string()),
                            available_count: models.len() as u64,
                            known_count,
                        },
                    )?;
                    RegistryRefreshCounts {
                        available: models.len() as u64,
                        added,
                        dropped,
                    }
                }
                Err(reason) => {
                    let available = existing.iter().filter(|row| row.available).count() as u64;
                    put_provider(
                        tx,
                        &RegistryProviderRecord {
                            provider_id: provider_id.to_string(),
                            status: "error".into(),
                            error: Some(reason.to_string()),
                            refreshed_at: at.to_string(),
                            succeeded_at: previous.and_then(|row| row.succeeded_at),
                            available_count: available,
                            known_count: existing.len() as u64,
                        },
                    )?;
                    RegistryRefreshCounts {
                        available,
                        added: 0,
                        dropped: 0,
                    }
                }
            };
            Ok((counts, advance(tx)?))
        })
    }

    /// Mark every recorded provider not in `configured` as removed and its
    /// models unavailable. Returns what changed per provider, and the
    /// registry revision.
    pub fn retire_model_registry_providers(
        &self,
        configured: &[String],
        at: &str,
    ) -> std::result::Result<(Vec<(String, RegistryRefreshCounts)>, u64), CozoError> {
        if !bounded(at, MAX_ID_BYTES) {
            return Err(error("invalid model registry refresh time"));
        }
        let configured: BTreeSet<&str> = configured.iter().map(String::as_str).collect();
        self.model_registry_write(|tx| {
            let mut retired = Vec::new();
            for provider in all_providers(tx)? {
                if configured.contains(provider.provider_id.as_str())
                    || provider.status == "removed"
                {
                    continue;
                }
                let existing = provider_models(tx, &provider.provider_id)?;
                let rows: Vec<RegistryModelRecord> = existing
                    .iter()
                    .filter(|row| row.available)
                    .map(|row| RegistryModelRecord {
                        available: false,
                        ..row.clone()
                    })
                    .collect();
                put_models(tx, &rows)?;
                put_provider(
                    tx,
                    &RegistryProviderRecord {
                        status: "removed".into(),
                        error: None,
                        refreshed_at: at.to_string(),
                        available_count: 0,
                        known_count: existing.len() as u64,
                        ..provider.clone()
                    },
                )?;
                retired.push((
                    provider.provider_id,
                    RegistryRefreshCounts {
                        available: 0,
                        added: 0,
                        dropped: rows.len() as u64,
                    },
                ));
            }
            let revision = if retired.is_empty() {
                revision(tx)?
            } else {
                advance(tx)?
            };
            Ok((retired, revision))
        })
    }

    /// Every recorded provider, by id, and the registry revision.
    pub fn list_model_registry_providers(
        &self,
    ) -> std::result::Result<(Vec<RegistryProviderRecord>, u64), CozoError> {
        self.model_registry_read(|tx| {
            let mut providers = all_providers(tx)?;
            providers.sort_by(|left, right| left.provider_id.cmp(&right.provider_id));
            Ok((providers, revision(tx)?))
        })
    }

    /// Every recorded model (of one provider when given), ordered by
    /// provider and model id, and the registry revision.
    pub fn list_model_registry_models(
        &self,
        provider_id: Option<&str>,
    ) -> std::result::Result<(Vec<RegistryModelRecord>, u64), CozoError> {
        self.model_registry_read(|tx| {
            let mut models = match provider_id {
                Some(provider_id) => provider_models(tx, provider_id)?,
                None => model_rows(run(
                    tx,
                    &format!("?[{MODEL_COLUMNS}] := *model_registry_models{{{MODEL_COLUMNS}}}"),
                    BTreeMap::new(),
                    "model read",
                )?)?,
            };
            models.sort_by(|left, right| {
                (left.provider_id.as_str(), left.model.model_id.as_str())
                    .cmp(&(right.provider_id.as_str(), right.model.model_id.as_str()))
            });
            Ok((models, revision(tx)?))
        })
    }

    /// The Control Plane profile recorded for each model, if one was set.
    pub fn list_model_registry_profiles(
        &self,
    ) -> std::result::Result<Vec<(String, String, String)>, CozoError> {
        self.model_registry_read(profiles)
    }

    /// Record which Control Plane profile `model_id` needs, or clear it when
    /// `profile` is absent. The model must already be in the catalog. An
    /// unchanged record does not advance the revision.
    pub fn set_model_registry_profile(
        &self,
        provider_id: &str,
        model_id: &str,
        profile: Option<&str>,
    ) -> std::result::Result<u64, CozoError> {
        if !bounded(provider_id, MAX_ID_BYTES) || !bounded(model_id, MAX_ID_BYTES) {
            return Err(error("invalid model registry profile key"));
        }
        if let Some(profile) = profile
            && !bounded(profile, MAX_ID_BYTES)
        {
            return Err(error("invalid model registry profile"));
        }
        let provider_id = provider_id.to_string();
        let model_id = model_id.to_string();
        let profile = profile.map(str::to_string);
        self.model_registry_write(|tx| {
            let known = provider_models(tx, &provider_id)?;
            if !known.iter().any(|row| row.model.model_id == model_id) {
                return Err(error("model is not in the catalog"));
            }
            let current = profiles(tx)?
                .into_iter()
                .find(|(provider, model, _)| provider == &provider_id && model == &model_id)
                .map(|(_, _, recorded)| recorded);
            if current.as_deref() == profile.as_deref() {
                return revision(tx);
            }
            match &profile {
                Some(profile) => {
                    run(
                        tx,
                        &format!(
                            "?[{PROFILE_COLUMNS}] <- [[$provider, $model, $profile]] :put model_registry_profiles {{provider_id, model_id => profile}}"
                        ),
                        BTreeMap::from([
                            ("provider".into(), text(&provider_id)),
                            ("model".into(), text(&model_id)),
                            ("profile".into(), text(profile)),
                        ]),
                        "profile write",
                    )?;
                }
                None => {
                    run(
                        tx,
                        "?[provider_id, model_id] <- [[$provider, $model]] :rm model_registry_profiles {provider_id, model_id}",
                        BTreeMap::from([
                            ("provider".into(), text(&provider_id)),
                            ("model".into(), text(&model_id)),
                        ]),
                        "profile clear",
                    )?;
                }
            }
            advance(tx)
        })
    }
}

fn profiles(
    tx: &MultiTransaction,
) -> std::result::Result<Vec<(String, String, String)>, CozoError> {
    let rows = run(
        tx,
        &format!("?[{PROFILE_COLUMNS}] := *model_registry_profiles{{{PROFILE_COLUMNS}}}"),
        BTreeMap::new(),
        "profile read",
    )?;
    rows.rows
        .into_iter()
        .map(|row| match row.as_slice() {
            [provider_id, model_id, profile] => {
                Ok((as_str(provider_id)?, as_str(model_id)?, as_str(profile)?))
            }
            _ => Err(error("corrupt model registry profile row")),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> CozoGraphShuttle {
        let store = CozoGraphShuttle::new_in_memory().unwrap();
        store.initialize_model_registry_store().unwrap();
        store
    }

    fn model(id: &str) -> RegistryModel {
        RegistryModel {
            model_id: id.into(),
            name: Some(format!("{id} name")),
            context_length: Some(128_000),
            prompt_price: Some("0.000001".into()),
            completion_price: None,
        }
    }

    fn available(store: &CozoGraphShuttle, provider: &str) -> Vec<String> {
        store
            .list_model_registry_models(Some(provider))
            .unwrap()
            .0
            .into_iter()
            .filter(|row| row.available)
            .map(|row| row.model.model_id)
            .collect()
    }

    #[test]
    fn refresh_adds_keeps_and_drops_models() {
        let store = store();
        let first = [model("a"), model("b")];
        let (counts, revision) = store
            .record_model_registry_refresh("openrouter", "2026-09-29T10:00:00Z", Ok(&first))
            .unwrap();
        assert_eq!((counts.available, counts.added, counts.dropped), (2, 2, 0));
        assert_eq!(revision, 1);

        let second = [model("b"), model("c")];
        let (counts, revision) = store
            .record_model_registry_refresh("openrouter", "2026-09-29T11:00:00Z", Ok(&second))
            .unwrap();
        assert_eq!((counts.available, counts.added, counts.dropped), (2, 1, 1));
        assert_eq!(revision, 2);
        assert_eq!(available(&store, "openrouter"), ["b", "c"]);

        let (models, _) = store
            .list_model_registry_models(Some("openrouter"))
            .unwrap();
        let a = models.iter().find(|row| row.model.model_id == "a").unwrap();
        assert!(!a.available);
        assert_eq!(a.last_seen, "2026-09-29T10:00:00Z");
        let b = models.iter().find(|row| row.model.model_id == "b").unwrap();
        assert_eq!(b.first_seen, "2026-09-29T10:00:00Z");
        assert_eq!(b.last_seen, "2026-09-29T11:00:00Z");
        assert_eq!(b.model.context_length, Some(128_000));
        assert_eq!(b.model.completion_price, None);

        let (providers, _) = store.list_model_registry_providers().unwrap();
        assert_eq!(providers[0].available_count, 2);
        assert_eq!(providers[0].known_count, 3);
        assert_eq!(providers[0].status, "ok");
    }

    #[test]
    fn failed_refresh_keeps_the_last_good_list() {
        let store = store();
        store
            .record_model_registry_refresh("opencode_go", "t1", Ok(&[model("glm")]))
            .unwrap();
        let (counts, _) = store
            .record_model_registry_refresh("opencode_go", "t2", Err("HTTP 503"))
            .unwrap();
        assert_eq!(counts.available, 1);
        assert_eq!(available(&store, "opencode_go"), ["glm"]);
        let (providers, _) = store.list_model_registry_providers().unwrap();
        assert_eq!(providers[0].status, "error");
        assert_eq!(providers[0].error.as_deref(), Some("HTTP 503"));
        assert_eq!(providers[0].succeeded_at.as_deref(), Some("t1"));
        assert_eq!(providers[0].refreshed_at, "t2");
    }

    #[test]
    fn removed_providers_are_retired_once() {
        let store = store();
        store
            .record_model_registry_refresh("old", "t1", Ok(&[model("x"), model("y")]))
            .unwrap();
        store
            .record_model_registry_refresh("kept", "t1", Ok(&[model("z")]))
            .unwrap();
        let (retired, revision) = store
            .retire_model_registry_providers(&["kept".to_string()], "t2")
            .unwrap();
        assert_eq!(retired.len(), 1);
        assert_eq!(retired[0].0, "old");
        assert_eq!(retired[0].1.dropped, 2);
        assert!(available(&store, "old").is_empty());
        assert_eq!(available(&store, "kept"), ["z"]);
        let (again, same) = store
            .retire_model_registry_providers(&["kept".to_string()], "t3")
            .unwrap();
        assert!(again.is_empty());
        assert_eq!(same, revision);
    }

    #[test]
    fn models_list_in_provider_then_id_order_across_chunks() {
        let store = store();
        let many: Vec<_> = (0..250).map(|i| model(&format!("m{i:03}"))).collect();
        store
            .record_model_registry_refresh("zeta", "t", Ok(&many))
            .unwrap();
        store
            .record_model_registry_refresh("alpha", "t", Ok(&[model("only")]))
            .unwrap();
        let (models, revision) = store.list_model_registry_models(None).unwrap();
        assert_eq!(models.len(), 251);
        assert_eq!(models[0].provider_id, "alpha");
        assert_eq!(models[1].model.model_id, "m000");
        assert_eq!(models[250].model.model_id, "m249");
        assert_eq!(revision, 2);
    }

    #[test]
    fn invalid_rows_are_rejected() {
        let store = store();
        let bad = [RegistryModel {
            model_id: "bad\nid".into(),
            ..model("x")
        }];
        assert!(
            store
                .record_model_registry_refresh("p", "t", Ok(&bad))
                .is_err()
        );
        let twice = [model("x"), model("x")];
        assert!(
            store
                .record_model_registry_refresh("p", "t", Ok(&twice))
                .is_err()
        );
        assert_eq!(store.list_model_registry_providers().unwrap().1, 0);
    }

    #[test]
    fn a_recorded_profile_survives_refresh_and_clears() {
        let store = store();
        store
            .record_model_registry_refresh("opencode_go", "t1", Ok(&[model("glm")]))
            .unwrap();
        assert!(
            store
                .set_model_registry_profile("opencode_go", "missing", Some("opencode.go_responses"))
                .is_err()
        );
        let revision = store
            .set_model_registry_profile(
                "opencode_go",
                "glm",
                Some("opencode.go_control_plane_responses"),
            )
            .unwrap();
        assert_eq!(revision, 2);
        let same = store
            .set_model_registry_profile(
                "opencode_go",
                "glm",
                Some("opencode.go_control_plane_responses"),
            )
            .unwrap();
        assert_eq!(same, revision);
        store
            .record_model_registry_refresh("opencode_go", "t2", Ok(&[model("glm"), model("kimi")]))
            .unwrap();
        let profiles = store.list_model_registry_profiles().unwrap();
        assert_eq!(
            profiles,
            [(
                "opencode_go".into(),
                "glm".into(),
                "opencode.go_control_plane_responses".into()
            )]
        );
        let cleared = store
            .set_model_registry_profile("opencode_go", "glm", None)
            .unwrap();
        assert!(cleared > revision);
        assert!(store.list_model_registry_profiles().unwrap().is_empty());
    }
}
