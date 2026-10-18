//! Durable json_render UI gallery candidates and the active chat Canvas
//! catalog, in a dedicated Cozo domain owner.
//!
//! Candidates are keyed by the principal who saved them, one row each, so
//! the gallery has no size limit and is read a page at a time, newest first
//! (candidate IDs are UUIDv7). Catalog entries are system-wide; an active
//! entry's name is unique. Promotion and retirement are single transactions:
//! a candidate is promoted once, and its row records the entry it became.
//! Every stored document must agree with its key columns; the json_render
//! plugin owns the typed contract.

use super::identity_transactions::commit_checked;
use super::*;
use cozo::MultiTransaction;

const MAX_PAGE: usize = 100;
const MAX_PRINCIPAL_BYTES: usize = 512;
const MAX_ID_BYTES: usize = 128;
const MAX_NAME_BYTES: usize = 200;
/// Largest stored candidate or catalog document.
pub const MAX_UI_GALLERY_DOCUMENT_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq)]
pub struct UiCandidateRecord {
    pub principal_id: String,
    pub candidate_id: String,
    pub spec_hash: String,
    pub promoted_entry_id: Option<String>,
    pub candidate: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct UiCandidatePage {
    /// Newest first.
    pub candidates: Vec<UiCandidateRecord>,
    /// Older candidates exist beyond this page.
    pub more: bool,
    pub total: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct UiCatalogRecord {
    pub entry_id: String,
    pub name: String,
    /// `active` or `retired`.
    pub status: String,
    pub spec_hash: String,
    pub from_principal: String,
    pub from_candidate_id: String,
    pub entry: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct UiCatalogPage {
    /// Newest promotion first.
    pub entries: Vec<UiCatalogRecord>,
    pub more: bool,
    /// Active entries, or all entries when retired ones were requested.
    pub total: u64,
    /// Catalog revision: advances on every promotion and retirement.
    pub revision: u64,
}

fn error(message: &str) -> CozoError {
    CozoError::Other(message.into())
}

fn bounded(value: &str, maximum: usize) -> bool {
    !value.is_empty()
        && value.len() <= maximum
        && !value.chars().any(char::is_control)
        && value.trim() == value
}

fn is_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn text(value: &str) -> DataValue {
    DataValue::Str(value.into())
}

fn int(value: u64, what: &str) -> std::result::Result<DataValue, CozoError> {
    Ok(DataValue::Num(Num::Int(i64::try_from(value).map_err(
        |_| CozoError::Other(format!("ui gallery {what} exhausted")),
    )?)))
}

fn as_u64(value: &DataValue) -> std::result::Result<u64, CozoError> {
    match value {
        DataValue::Num(Num::Int(value)) if *value >= 0 => Ok(*value as u64),
        _ => Err(error("corrupt ui gallery integer column")),
    }
}

fn document(value: &Value) -> std::result::Result<String, CozoError> {
    let json = serde_json::to_string(value)?;
    if json.len() > MAX_UI_GALLERY_DOCUMENT_BYTES {
        return Err(error("ui gallery document exceeds its size limit"));
    }
    Ok(json)
}

fn field<'a>(value: &'a Value, name: &str) -> Option<&'a str> {
    value.get(name).and_then(Value::as_str)
}

fn validate_candidate(row: &UiCandidateRecord) -> std::result::Result<(), CozoError> {
    if !bounded(&row.principal_id, MAX_PRINCIPAL_BYTES)
        || !bounded(&row.candidate_id, MAX_ID_BYTES)
        || !is_hash(&row.spec_hash)
        || row
            .promoted_entry_id
            .as_deref()
            .is_some_and(|entry| !bounded(entry, MAX_ID_BYTES))
    {
        return Err(error("invalid ui gallery candidate key"));
    }
    let promoted = row
        .candidate
        .get("promoted_entry_id")
        .and_then(Value::as_str);
    if field(&row.candidate, "principal_id") != Some(&row.principal_id)
        || field(&row.candidate, "candidate_id") != Some(&row.candidate_id)
        || field(&row.candidate, "spec_hash") != Some(&row.spec_hash)
        || promoted != row.promoted_entry_id.as_deref()
    {
        return Err(error(
            "ui gallery candidate document differs from its durable key",
        ));
    }
    Ok(())
}

fn validate_entry(row: &UiCatalogRecord) -> std::result::Result<(), CozoError> {
    if !bounded(&row.entry_id, MAX_ID_BYTES)
        || !bounded(&row.name, MAX_NAME_BYTES)
        || !matches!(row.status.as_str(), "active" | "retired")
        || !is_hash(&row.spec_hash)
        || !bounded(&row.from_principal, MAX_PRINCIPAL_BYTES)
        || !bounded(&row.from_candidate_id, MAX_ID_BYTES)
    {
        return Err(error("invalid ui catalog entry key"));
    }
    if field(&row.entry, "entry_id") != Some(&row.entry_id)
        || field(&row.entry, "name") != Some(&row.name)
        || field(&row.entry, "status") != Some(&row.status)
        || field(&row.entry, "spec_hash") != Some(&row.spec_hash)
        || field(&row.entry, "promoted_by") != Some(&row.from_principal)
        || field(&row.entry, "from_candidate_id") != Some(&row.from_candidate_id)
    {
        return Err(error("ui catalog document differs from its durable key"));
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
        .map_err(|error| CozoError::Other(format!("ui gallery {what} failed: {error}")))
}

fn candidate_rows(
    rows: NamedRows,
    principal_id: &str,
) -> std::result::Result<Vec<UiCandidateRecord>, CozoError> {
    rows.rows
        .into_iter()
        .map(|row| {
            let [
                DataValue::Str(candidate_id),
                DataValue::Str(spec_hash),
                DataValue::Str(promoted),
                DataValue::Str(json),
            ] = row.as_slice()
            else {
                return Err(error("corrupt ui gallery candidate row"));
            };
            let record = UiCandidateRecord {
                principal_id: principal_id.to_string(),
                candidate_id: candidate_id.to_string(),
                spec_hash: spec_hash.to_string(),
                promoted_entry_id: (!promoted.is_empty()).then(|| promoted.to_string()),
                candidate: serde_json::from_str(json)
                    .map_err(|_| error("corrupt ui gallery candidate JSON"))?,
            };
            validate_candidate(&record)?;
            Ok(record)
        })
        .collect()
}

fn entry_rows(rows: NamedRows) -> std::result::Result<Vec<UiCatalogRecord>, CozoError> {
    rows.rows
        .into_iter()
        .map(|row| {
            let [
                DataValue::Str(entry_id),
                DataValue::Str(name),
                DataValue::Str(status),
                DataValue::Str(spec_hash),
                DataValue::Str(from_principal),
                DataValue::Str(from_candidate_id),
                DataValue::Str(json),
            ] = row.as_slice()
            else {
                return Err(error("corrupt ui catalog row"));
            };
            let record = UiCatalogRecord {
                entry_id: entry_id.to_string(),
                name: name.to_string(),
                status: status.to_string(),
                spec_hash: spec_hash.to_string(),
                from_principal: from_principal.to_string(),
                from_candidate_id: from_candidate_id.to_string(),
                entry: serde_json::from_str(json).map_err(|_| error("corrupt ui catalog JSON"))?,
            };
            validate_entry(&record)?;
            Ok(record)
        })
        .collect()
}

const CANDIDATE_COLUMNS: &str = "candidate_id, spec_hash, promoted_entry_id, candidate_json";
const ENTRY_COLUMNS: &str =
    "entry_id, name, status, spec_hash, from_principal, from_candidate_id, entry_json";

fn get_candidate(
    tx: &MultiTransaction,
    principal_id: &str,
    candidate_id: &str,
) -> std::result::Result<Option<UiCandidateRecord>, CozoError> {
    let values = BTreeMap::from([
        ("principal".into(), text(principal_id)),
        ("candidate".into(), text(candidate_id)),
    ]);
    let rows = run(
        tx,
        &format!(
            "?[{CANDIDATE_COLUMNS}] := *ui_gallery_candidates{{principal_id, {CANDIDATE_COLUMNS}}}, principal_id = $principal, candidate_id = $candidate"
        ),
        values,
        "candidate read",
    )?;
    Ok(candidate_rows(rows, principal_id)?.pop())
}

fn write_candidate(
    tx: &MultiTransaction,
    row: &UiCandidateRecord,
) -> std::result::Result<(), CozoError> {
    validate_candidate(row)?;
    let values = BTreeMap::from([
        ("principal".into(), text(&row.principal_id)),
        ("candidate".into(), text(&row.candidate_id)),
        ("hash".into(), text(&row.spec_hash)),
        (
            "promoted".into(),
            text(row.promoted_entry_id.as_deref().unwrap_or_default()),
        ),
        ("json".into(), text(&document(&row.candidate)?)),
    ]);
    run(
        tx,
        "?[principal_id, candidate_id, spec_hash, promoted_entry_id, candidate_json] <- [[$principal, $candidate, $hash, $promoted, $json]] :put ui_gallery_candidates {principal_id, candidate_id => spec_hash, promoted_entry_id, candidate_json}",
        values,
        "candidate write",
    )?;
    Ok(())
}

fn candidate_count(
    tx: &MultiTransaction,
    principal_id: &str,
) -> std::result::Result<u64, CozoError> {
    let rows = run(
        tx,
        "?[candidate_count] := *ui_gallery_scopes{principal_id, candidate_count}, principal_id = $principal",
        BTreeMap::from([("principal".into(), text(principal_id))]),
        "scope read",
    )?;
    match rows.rows.as_slice() {
        [] => Ok(0),
        [row] => as_u64(
            row.first()
                .ok_or_else(|| error("corrupt ui gallery scope row"))?,
        ),
        _ => Err(error("corrupt ui gallery scope row")),
    }
}

fn write_candidate_count(
    tx: &MultiTransaction,
    principal_id: &str,
    count: u64,
) -> std::result::Result<(), CozoError> {
    run(
        tx,
        "?[principal_id, candidate_count] <- [[$principal, $count]] :put ui_gallery_scopes {principal_id => candidate_count}",
        BTreeMap::from([
            ("principal".into(), text(principal_id)),
            ("count".into(), int(count, "candidate count")?),
        ]),
        "scope write",
    )?;
    Ok(())
}

fn get_entry(
    tx: &MultiTransaction,
    entry_id: &str,
) -> std::result::Result<Option<UiCatalogRecord>, CozoError> {
    let rows = run(
        tx,
        &format!("?[{ENTRY_COLUMNS}] := *ui_catalog_entries{{{ENTRY_COLUMNS}}}, entry_id = $entry"),
        BTreeMap::from([("entry".into(), text(entry_id))]),
        "catalog read",
    )?;
    Ok(entry_rows(rows)?.pop())
}

fn write_entry(tx: &MultiTransaction, row: &UiCatalogRecord) -> std::result::Result<(), CozoError> {
    validate_entry(row)?;
    let values = BTreeMap::from([
        ("entry".into(), text(&row.entry_id)),
        ("name".into(), text(&row.name)),
        ("status".into(), text(&row.status)),
        ("hash".into(), text(&row.spec_hash)),
        ("principal".into(), text(&row.from_principal)),
        ("candidate".into(), text(&row.from_candidate_id)),
        ("json".into(), text(&document(&row.entry)?)),
    ]);
    run(
        tx,
        "?[entry_id, name, status, spec_hash, from_principal, from_candidate_id, entry_json] <- [[$entry, $name, $status, $hash, $principal, $candidate, $json]] :put ui_catalog_entries {entry_id => name, status, spec_hash, from_principal, from_candidate_id, entry_json}",
        values,
        "catalog write",
    )?;
    Ok(())
}

/// (revision, active entries, all entries)
fn catalog_state(tx: &MultiTransaction) -> std::result::Result<(u64, u64, u64), CozoError> {
    let rows = run(
        tx,
        "?[revision, active_count, total_count] := *ui_catalog_state{key, revision, active_count, total_count}, key = 'catalog'",
        BTreeMap::new(),
        "catalog state read",
    )?;
    match rows.rows.as_slice() {
        [] => Ok((0, 0, 0)),
        [row] => match row.as_slice() {
            [revision, active, total] => Ok((as_u64(revision)?, as_u64(active)?, as_u64(total)?)),
            _ => Err(error("corrupt ui catalog state row")),
        },
        _ => Err(error("corrupt ui catalog state row")),
    }
}

fn write_catalog_state(
    tx: &MultiTransaction,
    revision: u64,
    active: u64,
    total: u64,
) -> std::result::Result<(), CozoError> {
    run(
        tx,
        "?[key, revision, active_count, total_count] <- [['catalog', $revision, $active, $total]] :put ui_catalog_state {key => revision, active_count, total_count}",
        BTreeMap::from([
            ("revision".into(), int(revision, "catalog revision")?),
            ("active".into(), int(active, "active count")?),
            ("total".into(), int(total, "catalog count")?),
        ]),
        "catalog state write",
    )?;
    Ok(())
}

fn name_owner(tx: &MultiTransaction, name: &str) -> std::result::Result<Option<String>, CozoError> {
    let rows = run(
        tx,
        "?[entry_id] := *ui_catalog_names{name, entry_id}, name = $name",
        BTreeMap::from([("name".into(), text(name))]),
        "catalog name read",
    )?;
    match rows.rows.as_slice() {
        [] => Ok(None),
        [row] => match row.as_slice() {
            [DataValue::Str(entry)] => Ok(Some(entry.to_string())),
            _ => Err(error("corrupt ui catalog name row")),
        },
        _ => Err(error("corrupt ui catalog name row")),
    }
}

fn page_limit(limit: usize) -> std::result::Result<usize, CozoError> {
    if limit == 0 || limit > MAX_PAGE {
        return Err(error("ui gallery page limit must be 1 to 100"));
    }
    Ok(limit)
}

fn cursor(before: Option<&str>) -> std::result::Result<Option<&str>, CozoError> {
    if let Some(before) = before
        && !bounded(before, MAX_ID_BYTES)
    {
        return Err(error("invalid ui gallery page cursor"));
    }
    Ok(before)
}

impl CozoGraphShuttle {
    /// Opt this handle into the json_render UI gallery domain. Generic Cozo
    /// stores deliberately do not expose these relations.
    pub fn initialize_ui_gallery_store(&self) -> std::result::Result<(), CozoError> {
        let _guard = self
            .episode_transaction
            .lock()
            .map_err(|_| error("ui gallery owner is poisoned"))?;
        self.runtime_document_preflight()?;
        for relation in [
            r#":create ui_gallery_scopes {
                principal_id: String
                => candidate_count: Int
            }"#,
            r#":create ui_gallery_candidates {
                principal_id: String, candidate_id: String
                =>
                spec_hash: String,
                promoted_entry_id: String,
                candidate_json: String
            }"#,
            r#":create ui_catalog_entries {
                entry_id: String
                =>
                name: String,
                status: String,
                spec_hash: String,
                from_principal: String,
                from_candidate_id: String,
                entry_json: String
            }"#,
            r#":create ui_catalog_names {
                name: String
                => entry_id: String
            }"#,
            r#":create ui_catalog_state {
                key: String
                => revision: Int, active_count: Int, total_count: Int
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
                        "ui gallery relation initialization failed: {message}"
                    )));
                }
            }
        }
        for probe in [
            "?[principal_id, candidate_count] := *ui_gallery_scopes{principal_id, candidate_count} :limit 0",
            "?[principal_id, candidate_id, spec_hash, promoted_entry_id, candidate_json] := *ui_gallery_candidates{principal_id, candidate_id, spec_hash, promoted_entry_id, candidate_json} :limit 0",
            "?[entry_id, name, status, spec_hash, from_principal, from_candidate_id, entry_json] := *ui_catalog_entries{entry_id, name, status, spec_hash, from_principal, from_candidate_id, entry_json} :limit 0",
            "?[name, entry_id] := *ui_catalog_names{name, entry_id} :limit 0",
            "?[key, revision, active_count, total_count] := *ui_catalog_state{key, revision, active_count, total_count} :limit 0",
        ] {
            self.db
                .run_script(probe, BTreeMap::new(), ScriptMutability::Immutable)
                .map_err(|error| {
                    CozoError::Other(format!("ui gallery relation verification failed: {error}"))
                })?;
        }
        self.sync_runtime_document_store()
    }

    fn ui_gallery_write<T>(
        &self,
        body: impl FnOnce(&MultiTransaction) -> std::result::Result<T, CozoError>,
    ) -> std::result::Result<T, CozoError> {
        let _guard = self
            .episode_transaction
            .lock()
            .map_err(|_| error("ui gallery owner is poisoned"))?;
        self.runtime_document_preflight()?;
        let tx = self.db.multi_transaction(true);
        let value = body(&tx)?;
        commit_checked(&tx)?;
        self.sync_runtime_document_store()?;
        Ok(value)
    }

    fn ui_gallery_read<T>(
        &self,
        body: impl FnOnce(&MultiTransaction) -> std::result::Result<T, CozoError>,
    ) -> std::result::Result<T, CozoError> {
        let tx = self.db.multi_transaction(false);
        let value = body(&tx);
        let _ = tx.abort();
        value
    }

    /// Store a new, unpromoted candidate in its principal's gallery. Returns
    /// the principal's candidate count.
    pub fn insert_ui_candidate(
        &self,
        row: &UiCandidateRecord,
    ) -> std::result::Result<u64, CozoError> {
        validate_candidate(row)?;
        if row.promoted_entry_id.is_some() {
            return Err(error("a new ui gallery candidate is unpromoted"));
        }
        self.ui_gallery_write(|tx| {
            if get_candidate(tx, &row.principal_id, &row.candidate_id)?.is_some() {
                return Err(error("ui gallery candidate already exists"));
            }
            let count = candidate_count(tx, &row.principal_id)?
                .checked_add(1)
                .ok_or_else(|| error("ui gallery candidate count exhausted"))?;
            write_candidate(tx, row)?;
            write_candidate_count(tx, &row.principal_id, count)?;
            Ok(count)
        })
    }

    pub fn get_ui_candidate(
        &self,
        principal_id: &str,
        candidate_id: &str,
    ) -> std::result::Result<Option<UiCandidateRecord>, CozoError> {
        self.ui_gallery_read(|tx| get_candidate(tx, principal_id, candidate_id))
    }

    /// A page of the principal's candidates, newest first, older than
    /// `before` when given.
    pub fn list_ui_candidates(
        &self,
        principal_id: &str,
        before: Option<&str>,
        limit: usize,
    ) -> std::result::Result<UiCandidatePage, CozoError> {
        let limit = page_limit(limit)?;
        let before = cursor(before)?;
        self.ui_gallery_read(|tx| {
            let mut values = BTreeMap::from([("principal".into(), text(principal_id))]);
            let bound = match before {
                Some(before) => {
                    values.insert("before".into(), text(before));
                    ", candidate_id < $before"
                }
                None => "",
            };
            let rows = run(
                tx,
                &format!(
                    "?[{CANDIDATE_COLUMNS}] := *ui_gallery_candidates{{principal_id, {CANDIDATE_COLUMNS}}}, principal_id = $principal{bound} :order -candidate_id :limit {}",
                    limit + 1
                ),
                values,
                "candidate page read",
            )?;
            let mut candidates = candidate_rows(rows, principal_id)?;
            let more = candidates.len() > limit;
            candidates.truncate(limit);
            Ok(UiCandidatePage {
                candidates,
                more,
                total: candidate_count(tx, principal_id)?,
            })
        })
    }

    /// Remove one of the principal's candidates. A promoted candidate may be
    /// removed; its catalog entry keeps its own copy.
    pub fn delete_ui_candidate(
        &self,
        principal_id: &str,
        candidate_id: &str,
    ) -> std::result::Result<bool, CozoError> {
        self.ui_gallery_write(|tx| {
            if get_candidate(tx, principal_id, candidate_id)?.is_none() {
                return Ok(false);
            }
            run(
                tx,
                "?[principal_id, candidate_id] <- [[$principal, $candidate]] :rm ui_gallery_candidates {principal_id, candidate_id}",
                BTreeMap::from([
                    ("principal".into(), text(principal_id)),
                    ("candidate".into(), text(candidate_id)),
                ]),
                "candidate delete",
            )?;
            let count = candidate_count(tx, principal_id)?
                .checked_sub(1)
                .ok_or_else(|| error("ui gallery candidate count is inconsistent"))?;
            write_candidate_count(tx, principal_id, count)?;
            Ok(true)
        })
    }

    /// Promote one of the principal's candidates into the catalog. `build`
    /// receives the durable candidate and returns the new active entry; the
    /// candidate is then marked with the entry's ID. A candidate is promoted
    /// once, and an active entry's name is unique.
    pub fn promote_ui_candidate(
        &self,
        principal_id: &str,
        candidate_id: &str,
        build: &dyn Fn(&UiCandidateRecord) -> std::result::Result<UiCatalogRecord, CozoError>,
    ) -> std::result::Result<(UiCatalogRecord, UiCandidateRecord), CozoError> {
        self.ui_gallery_write(|tx| {
            let candidate = get_candidate(tx, principal_id, candidate_id)?
                .ok_or_else(|| error("ui gallery candidate not found"))?;
            if candidate.promoted_entry_id.is_some() {
                return Err(error("ui gallery candidate is already promoted"));
            }
            let entry = build(&candidate)?;
            if entry.status != "active"
                || entry.from_principal != principal_id
                || entry.from_candidate_id != candidate_id
                || entry.spec_hash != candidate.spec_hash
            {
                return Err(error("promoted entry differs from its candidate"));
            }
            if get_entry(tx, &entry.entry_id)?.is_some() {
                return Err(error("ui catalog entry already exists"));
            }
            if name_owner(tx, &entry.name)?.is_some() {
                return Err(error("an active catalog entry already has this name"));
            }
            write_entry(tx, &entry)?;
            run(
                tx,
                "?[name, entry_id] <- [[$name, $entry]] :put ui_catalog_names {name => entry_id}",
                BTreeMap::from([
                    ("name".into(), text(&entry.name)),
                    ("entry".into(), text(&entry.entry_id)),
                ]),
                "catalog name write",
            )?;
            let mut promoted = candidate;
            promoted.promoted_entry_id = Some(entry.entry_id.clone());
            promoted.candidate["promoted_entry_id"] = Value::String(entry.entry_id.clone());
            write_candidate(tx, &promoted)?;
            let (revision, active, total) = catalog_state(tx)?;
            write_catalog_state(
                tx,
                revision
                    .checked_add(1)
                    .ok_or_else(|| error("ui catalog revision exhausted"))?,
                active
                    .checked_add(1)
                    .ok_or_else(|| error("ui catalog count exhausted"))?,
                total
                    .checked_add(1)
                    .ok_or_else(|| error("ui catalog count exhausted"))?,
            )?;
            Ok((entry, promoted))
        })
    }

    pub fn get_ui_catalog_entry(
        &self,
        entry_id: &str,
    ) -> std::result::Result<Option<UiCatalogRecord>, CozoError> {
        self.ui_gallery_read(|tx| get_entry(tx, entry_id))
    }

    /// A page of the catalog, newest promotion first (entry IDs are UUIDv7).
    pub fn list_ui_catalog(
        &self,
        before: Option<&str>,
        limit: usize,
        include_retired: bool,
    ) -> std::result::Result<UiCatalogPage, CozoError> {
        let limit = page_limit(limit)?;
        let before = cursor(before)?;
        self.ui_gallery_read(|tx| {
            let mut values = BTreeMap::new();
            let mut filter = String::new();
            if let Some(before) = before {
                values.insert("before".into(), text(before));
                filter.push_str(", entry_id < $before");
            }
            if !include_retired {
                filter.push_str(", status = 'active'");
            }
            let rows = run(
                tx,
                &format!(
                    "?[{ENTRY_COLUMNS}] := *ui_catalog_entries{{{ENTRY_COLUMNS}}}{filter} :order -entry_id :limit {}",
                    limit + 1
                ),
                values,
                "catalog page read",
            )?;
            let mut entries = entry_rows(rows)?;
            let more = entries.len() > limit;
            entries.truncate(limit);
            let (revision, active, total) = catalog_state(tx)?;
            Ok(UiCatalogPage {
                entries,
                more,
                total: if include_retired { total } else { active },
                revision,
            })
        })
    }

    /// Retire an active entry: it leaves the active catalog and frees its
    /// name, and its record stays. `build` returns the retired document.
    pub fn retire_ui_catalog_entry(
        &self,
        entry_id: &str,
        build: &dyn Fn(&UiCatalogRecord) -> std::result::Result<UiCatalogRecord, CozoError>,
    ) -> std::result::Result<UiCatalogRecord, CozoError> {
        self.ui_gallery_write(|tx| {
            let entry =
                get_entry(tx, entry_id)?.ok_or_else(|| error("ui catalog entry not found"))?;
            if entry.status != "active" {
                return Err(error("ui catalog entry is already retired"));
            }
            let retired = build(&entry)?;
            if retired.status != "retired"
                || retired.entry_id != entry.entry_id
                || retired.name != entry.name
                || retired.spec_hash != entry.spec_hash
                || retired.from_principal != entry.from_principal
                || retired.from_candidate_id != entry.from_candidate_id
            {
                return Err(error("retired entry differs from the active entry"));
            }
            write_entry(tx, &retired)?;
            if name_owner(tx, &entry.name)?.as_deref() == Some(entry_id) {
                run(
                    tx,
                    "?[name] <- [[$name]] :rm ui_catalog_names {name}",
                    BTreeMap::from([("name".into(), text(&entry.name))]),
                    "catalog name delete",
                )?;
            }
            let (revision, active, total) = catalog_state(tx)?;
            write_catalog_state(
                tx,
                revision
                    .checked_add(1)
                    .ok_or_else(|| error("ui catalog revision exhausted"))?,
                active
                    .checked_sub(1)
                    .ok_or_else(|| error("ui catalog count is inconsistent"))?,
                total,
            )?;
            Ok(retired)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const ALICE: &str = "did:op:human:alice";
    const BOB: &str = "did:op:human:bob";

    fn store() -> CozoGraphShuttle {
        let store = CozoGraphShuttle::new_in_memory().unwrap();
        store.initialize_ui_gallery_store().unwrap();
        store
    }

    fn id(n: u32) -> String {
        format!("0190{n:04x}-0000-7000-8000-000000000000")
    }

    fn candidate(principal: &str, n: u32) -> UiCandidateRecord {
        let hash = format!("{n:064x}");
        UiCandidateRecord {
            principal_id: principal.into(),
            candidate_id: id(n),
            spec_hash: hash.clone(),
            promoted_entry_id: None,
            candidate: json!({"principal_id": principal, "candidate_id": id(n), "spec_hash": hash, "title": format!("c{n}")}),
        }
    }

    fn entry_for(candidate: &UiCandidateRecord, entry: u32, name: &str) -> UiCatalogRecord {
        UiCatalogRecord {
            entry_id: id(entry),
            name: name.into(),
            status: "active".into(),
            spec_hash: candidate.spec_hash.clone(),
            from_principal: candidate.principal_id.clone(),
            from_candidate_id: candidate.candidate_id.clone(),
            entry: json!({
                "entry_id": id(entry), "name": name, "status": "active",
                "spec_hash": candidate.spec_hash, "promoted_by": candidate.principal_id,
                "from_candidate_id": candidate.candidate_id
            }),
        }
    }

    #[test]
    fn candidates_page_newest_first_per_principal_without_a_cap() {
        let store = store();
        for n in 1..=250 {
            assert_eq!(
                store.insert_ui_candidate(&candidate(ALICE, n)).unwrap(),
                n as u64
            );
        }
        store.insert_ui_candidate(&candidate(BOB, 900)).unwrap();
        assert!(
            store.insert_ui_candidate(&candidate(ALICE, 7)).is_err(),
            "duplicate"
        );

        let mut seen = Vec::new();
        let mut before = None;
        loop {
            let page = store
                .list_ui_candidates(ALICE, before.as_deref(), 100)
                .unwrap();
            assert_eq!(page.total, 250);
            seen.extend(page.candidates.iter().map(|row| row.candidate_id.clone()));
            if !page.more {
                break;
            }
            before = seen.last().cloned();
        }
        let expected: Vec<_> = (1..=250).rev().map(id).collect();
        assert_eq!(seen, expected);
        assert!(
            store.get_ui_candidate(BOB, &id(1)).unwrap().is_none(),
            "private"
        );
        assert!(store.list_ui_candidates(ALICE, None, 101).is_err());

        assert!(store.delete_ui_candidate(ALICE, &id(3)).unwrap());
        assert!(!store.delete_ui_candidate(ALICE, &id(3)).unwrap());
        assert!(
            !store.delete_ui_candidate(BOB, &id(4)).unwrap(),
            "not Bob's"
        );
        assert_eq!(store.list_ui_candidates(ALICE, None, 1).unwrap().total, 249);
    }

    #[test]
    fn promotion_is_once_with_unique_active_names_and_retirement_frees_the_name() {
        let store = store();
        let one = candidate(ALICE, 1);
        let two = candidate(BOB, 2);
        store.insert_ui_candidate(&one).unwrap();
        store.insert_ui_candidate(&two).unwrap();

        let (entry, promoted) = store
            .promote_ui_candidate(ALICE, &one.candidate_id, &|row| {
                Ok(entry_for(row, 100, "Status"))
            })
            .unwrap();
        assert_eq!(
            promoted.promoted_entry_id.as_deref(),
            Some(entry.entry_id.as_str())
        );
        assert_eq!(
            store
                .get_ui_candidate(ALICE, &one.candidate_id)
                .unwrap()
                .unwrap(),
            promoted
        );
        assert!(
            store
                .promote_ui_candidate(ALICE, &one.candidate_id, &|row| Ok(entry_for(
                    row, 101, "Other"
                )))
                .is_err(),
            "promoted once"
        );
        assert!(
            store
                .promote_ui_candidate(ALICE, &two.candidate_id, &|row| Ok(entry_for(
                    row, 102, "X"
                )))
                .is_err(),
            "only the owner promotes"
        );
        assert!(
            store
                .promote_ui_candidate(BOB, &two.candidate_id, &|row| Ok(entry_for(
                    row, 103, "Status"
                )))
                .is_err(),
            "active names are unique"
        );

        let page = store.list_ui_catalog(None, 50, false).unwrap();
        assert_eq!((page.total, page.revision, page.entries.len()), (1, 1, 1));

        let retired = store
            .retire_ui_catalog_entry(&entry.entry_id, &|row| {
                let mut retired = row.clone();
                retired.status = "retired".into();
                retired.entry["status"] = json!("retired");
                Ok(retired)
            })
            .unwrap();
        assert_eq!(retired.status, "retired");
        assert!(
            store
                .list_ui_catalog(None, 50, false)
                .unwrap()
                .entries
                .is_empty()
        );
        let all = store.list_ui_catalog(None, 50, true).unwrap();
        assert_eq!((all.total, all.revision), (1, 2));
        assert!(
            store
                .retire_ui_catalog_entry(&entry.entry_id, &|row| Ok(row.clone()))
                .is_err()
        );

        store
            .promote_ui_candidate(BOB, &two.candidate_id, &|row| {
                Ok(entry_for(row, 104, "Status"))
            })
            .unwrap();
        assert_eq!(store.list_ui_catalog(None, 50, false).unwrap().total, 1);
    }

    #[test]
    fn a_document_must_match_its_key() {
        let store = store();
        let mut row = candidate(ALICE, 1);
        row.candidate["spec_hash"] = json!("0".repeat(64));
        assert!(store.insert_ui_candidate(&row).is_err());
        let row = candidate(ALICE, 2);
        store.insert_ui_candidate(&row).unwrap();
        assert!(
            store
                .promote_ui_candidate(ALICE, &row.candidate_id, &|row| {
                    let mut entry = entry_for(row, 5, "Mismatch");
                    entry.spec_hash = "f".repeat(64);
                    entry.entry["spec_hash"] = json!("f".repeat(64));
                    Ok(entry)
                })
                .is_err()
        );
    }

    #[test]
    fn generic_store_has_no_gallery_relations() {
        let generic = CozoGraphShuttle::new_in_memory().unwrap();
        let relations = generic
            .db
            .run_script("::relations", BTreeMap::new(), ScriptMutability::Immutable)
            .unwrap();
        assert!(!relations.rows.iter().any(|row| {
            matches!(row.first(), Some(DataValue::Str(name)) if name.starts_with("ui_gallery") || name.starts_with("ui_catalog"))
        }));
    }
}
