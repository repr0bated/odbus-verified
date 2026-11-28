//! Native owner-definition locators, derived only from the existing ledger.
//! These are private selectors, never caller-supplied scopes or permissions.
//! Old records retain their hashes; binaries without State.orchestration cannot
//! read new metadata. Roll back readers, never delete/rewrite canonical history.
use super::*;
use crate::core::persistence::{LedgerAction, LedgerRecord, PersistentCore};
use op_cozo_store::{DataValue, Num};
use op_state_store::method_receipt::MethodReceiptRecovery;
use std::collections::BTreeMap;
use std::sync::Arc;

/// Factual source is committed by construction. Only Grounded may participate
/// in execution, and it still requires current controller authorization.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DefinitionApprovalState {
    Grounded, Disabled, Retired, NativeManagementUnfinished, GroundingUnverified,
}

pub(crate) struct OwnerDefinition {
    pub source_scope: StateScope,
    pub head: OrchestrationRecord,
    pub version: OrchestrationRecord,
    pub head_proof: MethodEventProof,
    pub version_proof: MethodEventProof,
    pub management: MethodReceiptRecovery,
    pub head_management: MethodReceiptRecovery,
    pub version_control: Option<op_execution_tracker::ExecutionRecord>,
    pub head_control: Option<op_execution_tracker::ExecutionRecord>,
    pub approval: DefinitionApprovalState,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DefinitionHead { version: u64, disabled: bool, retired: Vec<u64> }

#[derive(Clone)]
struct Locator {
    session: String, project: String, requestor: String, event_id: u64, event_hash: String,
    revision: u64, version: u64, writer: String, legacy: bool,
}

pub(in crate::core) struct DefinitionGuard {
    owner: String, session: String, project: String, kind: String, id: String,
    operation_id: String, source: Option<Locator>, next_version: Option<u64>,
}

pub(super) fn is_definition_key(key: &str) -> bool {
    ["role:", "role_version:", "policy:", "policy_version:"].iter().any(|prefix| key.starts_with(prefix))
}

fn selector(owner: &str, project: &str, kind: &str, id: &str) -> Result<()> {
    ensure!(matches!(kind, "role" | "policy"), "unknown owner definition kind");
    for value in [owner, project, id] {
        ensure!(!value.is_empty() && value.len() <= 256 && !value.chars().any(char::is_control),
            "invalid owner definition selector");
    }
    Ok(())
}

fn params(owner: &str, kind: &str, id: &str) -> BTreeMap<String, DataValue> {
    BTreeMap::from([
        ("owner".into(), DataValue::Str(owner.into())),
        ("kind".into(), DataValue::Str(kind.into())),
        ("id".into(), DataValue::Str(id.into())),
    ])
}

fn text(row: &[DataValue], column: usize) -> Result<&str> {
    match row.get(column) { Some(DataValue::Str(value)) => Ok(value.as_str()), _ => anyhow::bail!("invalid definition locator text") }
}
fn number(row: &[DataValue], column: usize) -> Result<u64> {
    match row.get(column) { Some(DataValue::Num(Num::Int(value))) if *value >= 0 => Ok(u64::try_from(*value)?), _ => anyhow::bail!("invalid definition locator number") }
}
fn boolean(row: &[DataValue], column: usize) -> Result<bool> {
    match row.get(column) { Some(DataValue::Bool(value)) => Ok(*value), _ => anyhow::bail!("invalid definition locator flag") }
}

impl PersistentCore {
    pub(in crate::core) fn reset_owner_definition_index(&self) -> Result<()> {
        self.control_query(
            "{ ?[owner,kind,id,session,project,requestor,event_id,event_hash,revision,version,writer,legacy] <- [] \
               :replace bridge_owner_definition_sources {owner: String,kind: String,id: String,session: String => \
               project: String,requestor: String,event_id: Int,event_hash: String,revision: Int,version: Int,writer: String,legacy: Bool} } \
             { ?[owner,project,kind,requestor,id,session] <- [] \
               :replace bridge_owner_definition_catalog {owner: String,project: String,kind: String,requestor: String,id: String,session: String} } \
             { ?[owner,project,kind,id,session] <- [] \
               :replace bridge_owner_definition_management_catalog {owner: String,project: String,kind: String,id: String,session: String} }",
            BTreeMap::new(), true)?;
        Ok(())
    }

    fn store_definition_locator(&self, owner: &str, kind: &str, id: &str, locator: Locator) -> Result<()> {
        let mut p = params(owner, kind, id);
        p.extend([
            ("session".into(), DataValue::Str(locator.session.into())),
            ("project".into(), DataValue::Str(locator.project.into())),
            ("requestor".into(), DataValue::Str(locator.requestor.into())),
            ("event".into(), DataValue::Num(Num::Int(i64::try_from(locator.event_id)?))),
            ("hash".into(), DataValue::Str(locator.event_hash.into())),
            ("revision".into(), DataValue::Num(Num::Int(i64::try_from(locator.revision)?))),
            ("version".into(), DataValue::Num(Num::Int(i64::try_from(locator.version)?))),
            ("writer".into(), DataValue::Str(locator.writer.into())),
            ("legacy".into(), DataValue::Bool(locator.legacy)),
        ]);
        self.control_query(
            "{ ?[owner,project,kind,requestor,id,session] := \
               *bridge_owner_definition_sources[owner,kind,id,session,project,requestor,event_id,event_hash,revision,version,writer,legacy], \
               owner = $owner, kind = $kind, id = $id, session = $session \
               :rm bridge_owner_definition_catalog {owner,project,kind,requestor,id,session} } \
             { ?[owner,project,kind,id,session] := \
               *bridge_owner_definition_sources[owner,kind,id,session,project,requestor,event_id,event_hash,revision,version,writer,legacy], \
               owner = $owner, kind = $kind, id = $id, session = $session \
               :rm bridge_owner_definition_management_catalog {owner,project,kind,id,session} } \
             { ?[owner,kind,id,session,project,requestor,event_id,event_hash,revision,version,writer,legacy] <- \
               [[$owner,$kind,$id,$session,$project,$requestor,$event,$hash,$revision,$version,$writer,$legacy]] \
               :put bridge_owner_definition_sources {owner,kind,id,session => project,requestor,event_id,event_hash,revision,version,writer,legacy} } \
             { ?[owner,project,kind,requestor,id,session] := \
               *bridge_owner_definition_sources[owner,kind,id,session,project,requestor,event_id,event_hash,revision,version,writer,legacy], \
               owner = $owner, kind = $kind, id = $id, session = $session, legacy = false \
               :put bridge_owner_definition_catalog {owner,project,kind,requestor,id,session} } \
             { ?[owner,project,kind,id,session] := \
               *bridge_owner_definition_sources[owner,kind,id,session,project,requestor,event_id,event_hash,revision,version,writer,legacy], \
               owner = $owner, kind = $kind, id = $id, session = $session, legacy = false \
               :put bridge_owner_definition_management_catalog {owner,project,kind,id,session} }", p, true)?;
        Ok(())
    }

    pub(in crate::core) fn index_owner_definitions(&self, entry: &LedgerRecord) -> Result<()> {
        let (publications, orchestration) = match &entry.action {
            LedgerAction::State { publications, orchestration } => (publications, orchestration.as_ref()),
            LedgerAction::Terminal { publications, .. } => (publications, None),
            _ => return Ok(()),
        };
        if let Some(delta) = orchestration {
            let StateScope::Private { principal_id, session_id } = &entry.owner else { return Ok(()); };
            if delta.domain != "workstacks" { return Ok(()); }
            let Some((kind, id)) = delta.key.split_once(':').filter(|(kind, _)| matches!(*kind, "role" | "policy")) else { return Ok(()); };
            selector(principal_id, "_", kind, id)?;
            let publication = publications.iter().find(|publication| publication.scope == entry.owner && publication.plugin_id == "workstacks")
                .context("definition delta has no original publication")?;
            self.validate_definition_delta(&entry.owner, &delta.operation_id, kind, &entry.event, &publication.schema_hash)?;
            let records = publication.state.as_ref().and_then(|state| state.get("records")).context("definition publication records absent")?;
            let record = records.get(&delta.key).context("definition delta head absent")?;
            let revision = record.get("revision").and_then(Value::as_u64).context("definition head revision absent")?;
            ensure!(revision == delta.revision && revision > 0, "definition delta revision differs from original head");
            let version_number = record.get("value").and_then(|value| value.get("version")).and_then(Value::as_u64)
                .filter(|version| *version > 0).context("definition head version absent")?;
            let version_key = format!("{kind}_version:{id}:{version_number:020}");
            if let Some(creation) = &delta.version {
                ensure!(creation.key == version_key && creation.revision == 1,
                    "atomic definition metadata differs from original immutable version");
                let proof = self.definition_version_source(&entry.owner, &delta.operation_id, &creation.key, creation.revision)?;
                ensure!(proof.event_id == entry.event.event_id && proof.event_hash == entry.event.event_hash,
                    "head and immutable definition do not share their original append");
            }
            let version = records.get(&version_key).and_then(|record| record.get("value"))
                .context("definition immutable version absent")?;
            let project = version.get("project_id").and_then(Value::as_str).context("definition project absent")?;
            selector(principal_id, project, kind, id)?;
            let requestor = if kind == "role" {
                version.get("requestor_principal_id").and_then(Value::as_str).context("definition requestor absent")?
            } else { "" };
            self.store_definition_locator(principal_id, kind, id, Locator {
                session: session_id.clone(), project: project.to_owned(), requestor: requestor.to_owned(), event_id: entry.event.event_id,
                event_hash: entry.event.event_hash.clone(), revision, version: version_number,
                writer: delta.operation_id.clone(), legacy: false,
            })?;
        } else {
            // Any untyped replacement (including a Terminal publication) can
            // change definition heads. Fence prior eligibility immediately;
            // retain only legacy namespace markers, never fabricated approval.
            for publication in publications.iter().filter(|publication| publication.plugin_id == "workstacks") {
                let StateScope::Private { principal_id, session_id } = &publication.scope else { continue; };
                let p = BTreeMap::from([
                    ("owner".into(), DataValue::Str(principal_id.as_str().into())),
                    ("session".into(), DataValue::Str(session_id.as_str().into())),
                ]);
                self.control_query(
                    "{ ?[owner,kind,id,session,project,requestor,event_id,event_hash,revision,version,writer,legacy] := \
                       *bridge_owner_definition_sources[owner,kind,id,session,project,requestor,event_id,event_hash,revision,version,writer,old_legacy], \
                       owner = $owner, session = $session, legacy = true \
                       :put bridge_owner_definition_sources {owner,kind,id,session => project,requestor,event_id,event_hash,revision,version,writer,legacy} } \
                     { ?[owner,project,kind,requestor,id,session] := *bridge_owner_definition_catalog[owner,project,kind,requestor,id,session], \
                       owner = $owner, session = $session :rm bridge_owner_definition_catalog {owner,project,kind,requestor,id,session} } \
                     { ?[owner,project,kind,id,session] := *bridge_owner_definition_management_catalog[owner,project,kind,id,session], \
                       owner = $owner, session = $session :rm bridge_owner_definition_management_catalog {owner,project,kind,id,session} }", p, true)?;
                let Some(records) = publication.state.as_ref().and_then(|state| state.get("records")).and_then(Value::as_object) else { continue; };
                for key in records.keys() {
                    let Some((kind, id)) = key.split_once(':').filter(|(kind, _)| matches!(*kind, "role" | "policy")) else { continue; };
                    selector(principal_id, "_", kind, id)?;
                    self.store_definition_locator(principal_id, kind, id, Locator {
                        session: session_id.clone(), project: String::new(), requestor: String::new(), event_id: entry.event.event_id,
                        event_hash: entry.event.event_hash.clone(), revision: 0, version: 0,
                        writer: String::new(), legacy: true,
                    })?;
                }
            }
        }
        Ok(())
    }

    fn definition_locator(&self, owner: &str, kind: &str, id: &str) -> Result<Option<Locator>> {
        let locator = self.read_definition_locator(owner, kind, id, None)?;
        ensure!(locator.as_ref().is_none_or(|locator| !locator.legacy),
            "owner definition has no typed management provenance; needs review");
        Ok(locator)
    }

    fn read_definition_locator(&self, owner: &str, kind: &str, id: &str, session: Option<&str>) -> Result<Option<Locator>> {
        let mut p = params(owner, kind, id);
        let session_filter = if let Some(session) = session {
            p.insert("session".into(), DataValue::Str(session.into()));
            ", session = $session"
        } else { "" };
        let rows = self.control_query(&format!(
            "?[session,project,requestor,event_id,event_hash,revision,version,writer,legacy] := \
             *bridge_owner_definition_sources[owner,kind,id,session,project,requestor,event_id,event_hash,revision,version,writer,legacy], \
             owner = $owner, kind = $kind, id = $id{session_filter} :order session :limit 2"), p, false)?;
        ensure!(rows.rows.len() <= 1, "owner definition has ambiguous source sessions; needs review");
        let Some(row) = rows.rows.first() else { return Ok(None); };
        ensure!(row.len() == 9, "invalid owner definition locator");
        Ok(Some(Locator { session: text(row, 0)?.to_owned(), project: text(row, 1)?.to_owned(), requestor: text(row, 2)?.to_owned(),
            event_id: number(row, 3)?, event_hash: text(row, 4)?.to_owned(), revision: number(row, 5)?,
            version: number(row, 6)?, writer: text(row, 7)?.to_owned(), legacy: boolean(row, 8)? }))
    }

    pub(in crate::core) fn validate_definition_guard(&self, operation: &MethodOperation,
        guard: &DefinitionGuard, key: &str, value: &Value, expected_revision: Option<u64>) -> Result<()> {
        let (scope, _, _, _) = self.definition_attribution(operation, &guard.kind, guard.next_version.is_some())?;
        ensure!(scope == (StateScope::Private { principal_id: guard.owner.clone(), session_id: guard.session.clone() })
            && operation.operation_id() == guard.operation_id, "definition writer does not own original guarded admission");
        let current = self.definition_locator(&guard.owner, &guard.kind, &guard.id)?;
        match (&guard.source, current) {
            (None, None) => {}
            (Some(expected), Some(current)) => ensure!(current.session == guard.session && current.project == guard.project
                && current.event_id == expected.event_id && current.event_hash == expected.event_hash
                && current.revision == expected.revision && current.version == expected.version,
                "global definition head changed; reload before mutation"),
            _ => anyhow::bail!("global definition source changed; needs review"),
        }
        let head_key = format!("{}:{}", guard.kind, guard.id);
        if key == head_key {
            let head = DefinitionHead::deserialize(value)?;
            if let Some(version) = guard.next_version { ensure!(head.version == version && !head.disabled, "published definition head differs from guarded version"); }
            else { ensure!(head.disabled && guard.source.as_ref().is_some_and(|source| source.version == head.version), "disable cannot publish or enable another version"); }
        } else {
            let version = guard.next_version.context("disable has no immutable version writer")?;
            ensure!(expected_revision.is_none(), "immutable definition versions cannot be updated");
            ensure!(key == format!("{}_version:{}:{version:020}", guard.kind, guard.id), "definition writer key differs");
            let id_field = if guard.kind == "role" { "role_id" } else { "policy_id" };
            ensure!(value.get(id_field).and_then(Value::as_str) == Some(guard.id.as_str())
                && value.get("project_id").and_then(Value::as_str) == Some(guard.project.as_str())
                && value.get("approved_by").and_then(Value::as_str) == Some(guard.owner.as_str())
                && value.get("version").and_then(Value::as_u64) == Some(version), "definition body differs from native guarded owner/project/version");
        }
        Ok(())
    }

    fn read_owner_definition(&self, owner_principal: &str, project: &str, kind: &str,
        id: &str, version: Option<u64>, requestor_principal: Option<&str>) -> Result<Option<OwnerDefinition>> {
        let owner = self;
        owner.require_available()?;
        let Some(locator) = owner.definition_locator(owner_principal, kind, id)? else { return Ok(None); };
        ensure!(locator.project == project, "definition belongs to another project");
        if kind == "role" { if let Some(requestor) = requestor_principal {
            ensure!(locator.requestor == requestor, "definition does not approve the actual requestor");
        } }
        let source_scope = StateScope::Private { principal_id: owner_principal.to_owned(), session_id: locator.session.clone() };
        let entry = owner.canonical_indexed_record(&source_scope, locator.event_id, &locator.event_hash)?;
        let LedgerAction::State { publications, orchestration: Some(delta) } = entry.action else { anyhow::bail!("definition has no original typed state proof"); };
        let key = format!("{kind}:{id}");
        ensure!(delta.domain == "workstacks" && delta.key == key && delta.revision == locator.revision
            && delta.operation_id == locator.writer, "definition locator differs from original state metadata");
        let publication = publications.into_iter().find(|publication| publication.scope == source_scope && publication.plugin_id == "workstacks")
            .context("original owner definition publication absent")?;
        let mut state = publication.state.context("original owner definition state absent")?;
        let records = state.get_mut("records").and_then(Value::as_object_mut).context("original owner definition records absent")?;
        let head: OrchestrationRecord = serde_json::from_value(records.remove(&key).context("original definition head absent")?)?;
        let parsed = DefinitionHead::deserialize(&head.value)?;
        ensure!(head.revision == locator.revision && parsed.version == locator.version, "definition head differs from original locator");
        let requested = version.unwrap_or(parsed.version);
        ensure!(requested > 0 && requested <= parsed.version, "definition version not published");
        let version_key = format!("{kind}_version:{id}:{requested:020}");
        let current_version = records.get(&version_key).context("current immutable version reference absent")?;
        let creator = current_version.get("owner_operation_id").and_then(Value::as_str)
            .context("immutable version creator absent")?.to_owned();
        let version_revision = current_version.get("revision").and_then(Value::as_u64)
            .context("immutable version revision absent")?;
        let version_proof = owner.definition_version_source(&source_scope, &creator, &version_key, version_revision)?;
        // Current head and body usually share one original publication. Older
        // pins load their own publication, never a copy inside a newer head.
        let (version_delta, mut original_state) = if version_proof.event_id == locator.event_id {
            ensure!(version_proof.event_hash == locator.event_hash, "definition source event hash differs");
            (delta, state)
        } else {
            drop(state);
            let original = owner.canonical_indexed_record(&source_scope, version_proof.event_id, &version_proof.event_hash)?;
            let LedgerAction::State { publications, orchestration: Some(version_delta) } = original.action
                else { anyhow::bail!("immutable version original metadata absent"); };
            let state = publications.into_iter().find(|publication|
                publication.scope == source_scope && publication.plugin_id == "workstacks")
                .and_then(|publication| publication.state).context("original immutable version state absent")?;
            (version_delta, state)
        };
        ensure!(version_delta.domain == "workstacks" && version_delta.key == key && version_delta.operation_id == creator
            && version_delta.version.as_ref().is_some_and(|version| version.key == version_key && version.revision == version_revision),
            "immutable version source metadata differs");
        let original_records = original_state.get_mut("records").and_then(Value::as_object_mut)
            .context("original immutable version records absent")?;
        let version: OrchestrationRecord = serde_json::from_value(original_records.remove(&version_key)
            .context("original immutable definition absent")?)?;
        ensure!(version.owner_operation_id == creator && version.revision == version_revision,
            "immutable version envelope differs from its original publication");
        let id_field = if kind == "role" { "role_id" } else { "policy_id" };
        ensure!(version.value.get("project_id").and_then(Value::as_str) == Some(project)
            && version.value.get(id_field).and_then(Value::as_str) == Some(id)
            && version.value.get("version").and_then(Value::as_u64) == Some(requested)
            && version.value.get("approved_by").and_then(Value::as_str) == Some(owner_principal),
            "definition body differs from original native owner/project/version");
        if kind == "role" { if let Some(requestor) = requestor_principal {
            ensure!(version.value.get("requestor_principal_id").and_then(Value::as_str) == Some(requestor),
                "definition does not approve the actual requestor");
        } }
        let management = owner.definition_management_proof(&source_scope, &version.owner_operation_id, kind, false)?;
        let head_management = owner.definition_management_proof(&source_scope, &locator.writer, kind, true)?;
        ensure!(parsed.retired.iter().all(|version| *version > 0 && *version <= parsed.version),
            "retirement references an unpublished definition version");
        let version_control = owner.current_definition_control(&source_scope, &version.owner_operation_id)?;
        let head_control = if locator.writer == version.owner_operation_id {
            version_control.clone()
        } else { owner.current_definition_control(&source_scope, &locator.writer)? };
        let approval = if parsed.disabled { DefinitionApprovalState::Disabled }
            else if parsed.retired.contains(&requested) { DefinitionApprovalState::Retired }
            else if management.status != op_state_store::method_receipt::MethodRecoveryStatus::Completed
                || head_management.status != op_state_store::method_receipt::MethodRecoveryStatus::Completed {
                DefinitionApprovalState::NativeManagementUnfinished
            } else if version_control.as_ref().is_some_and(|record| record.status == op_execution_tracker::ExecutionStatus::Completed)
                && head_control.as_ref().is_some_and(|record| record.status == op_execution_tracker::ExecutionStatus::Completed) {
                DefinitionApprovalState::Grounded
            } else { DefinitionApprovalState::GroundingUnverified };
        Ok(Some(OwnerDefinition { source_scope, head, version,
            head_proof: MethodEventProof { event_id: locator.event_id, event_hash: locator.event_hash },
            version_proof, management, head_management, version_control, head_control, approval }))
    }
}

impl BridgeCore {
    /// Controller MUST resolve the current canonical project owner and actual
    /// requestor before calling. This returns one definition, not a read grant.
    pub(crate) fn owner_definition(&self, owner_principal: &str, project: &str, kind: &str,
        id: &str, version: Option<u64>, requestor_principal: &str) -> Result<Option<OwnerDefinition>> {
        selector(owner_principal, project, kind, id)?;
        let _boundary = self.inner.commit.lock().map_err(|_| anyhow::anyhow!("bridge commit owner poisoned"))?;
        let owner = self.inner.persistent.as_ref().context("durable bridge required")?.lock()
            .map_err(|_| anyhow::anyhow!("durable bridge owner poisoned"))?;
        owner.require_available()?;
        owner.read_owner_definition(owner_principal, project, kind, id, version, Some(requestor_principal))
    }

    /// Controller resolves/authorizes the current project owner before entering.
    /// The original native operation additionally binds that actual management actor.
    pub(crate) fn owner_definition_for_management(&self, operation: &MethodOperation,
        owner_principal: &str, project: &str, kind: &str, id: &str, version: Option<u64>) -> Result<Option<OwnerDefinition>> {
        selector(owner_principal, project, kind, id)?;
        let _boundary = self.inner.commit.lock().map_err(|_| anyhow::anyhow!("bridge commit owner poisoned"))?;
        let owner = self.inner.persistent.as_ref().context("durable bridge required")?.lock()
            .map_err(|_| anyhow::anyhow!("durable bridge owner poisoned"))?;
        owner.require_available()?;
        owner.definition_management_reader(operation, owner_principal, kind)?;
        owner.read_owner_definition(owner_principal, project, kind, id, version, None)
    }

    /// Sorted exclusive IDs from committed typed-head facts, including pending,
    /// disabled and retired records. Execution requires Grounded approval plus
    /// current controller authorization; policy reads require a requester role.
    pub(crate) fn owner_definition_ids(&self, owner_principal: &str, project: &str, kind: &str,
        requestor_principal: &str, after: Option<&str>, limit: usize) -> Result<Vec<String>> {
        selector(owner_principal, project, kind, "_")?;
        if let Some(after) = after { selector(owner_principal, project, kind, after)?; }
        ensure!(limit > 0 && limit <= MAX_PAGE, "owner definition page limit must be 1..=256");
        let _boundary = self.inner.commit.lock().map_err(|_| anyhow::anyhow!("bridge commit owner poisoned"))?;
        let owner = self.inner.persistent.as_ref().context("durable bridge required")?.lock()
            .map_err(|_| anyhow::anyhow!("durable bridge owner poisoned"))?;
        owner.require_available()?;
        let p = BTreeMap::from([
            ("owner".into(), DataValue::Str(owner_principal.into())),
            ("project".into(), DataValue::Str(project.into())),
            ("kind".into(), DataValue::Str(kind.into())),
            ("requestor".into(), DataValue::Str(if kind == "role" { requestor_principal } else { "" }.into())),
            ("after".into(), DataValue::Str(after.unwrap_or("").into())),
        ]);
        let rows = owner.control_query(&format!(
            "?[id] := *bridge_owner_definition_catalog[owner,project,kind,requestor,id,session], \
             owner = $owner, project = $project, kind = $kind, requestor = $requestor, id > $after \
             :order id :limit {limit}"), p, false)?;
        ensure!(rows.rows.len() <= limit, "owner definition catalog page exceeds bound");
        let mut ids = Vec::with_capacity(rows.rows.len());
        let mut previous = after.unwrap_or("");
        for row in &rows.rows {
            ensure!(row.len() == 1, "invalid owner definition catalog row");
            let id = text(row, 0)?;
            ensure!(id > previous, "owner definition cursor is not stable and exclusive");
            owner.read_owner_definition(owner_principal, project, kind, id, None, Some(requestor_principal))?
                .context("committed typed catalog has no original definition")?;
            ids.push(id.to_owned());
            previous = id;
        }
        Ok(ids)
    }

    pub(crate) fn owner_definition_ids_for_management(&self, operation: &MethodOperation,
        owner_principal: &str, project: &str, kind: &str, after: Option<&str>, limit: usize) -> Result<Vec<String>> {
        selector(owner_principal, project, kind, "_")?;
        if let Some(after) = after { selector(owner_principal, project, kind, after)?; }
        ensure!(limit > 0 && limit <= MAX_PAGE, "owner definition page limit must be 1..=256");
        let _boundary = self.inner.commit.lock().map_err(|_| anyhow::anyhow!("bridge commit owner poisoned"))?;
        let owner = self.inner.persistent.as_ref().context("durable bridge required")?.lock()
            .map_err(|_| anyhow::anyhow!("durable bridge owner poisoned"))?;
        owner.require_available()?;
        owner.definition_management_reader(operation, owner_principal, kind)?;
        let p = BTreeMap::from([
            ("owner".into(), DataValue::Str(owner_principal.into())),
            ("project".into(), DataValue::Str(project.into())),
            ("kind".into(), DataValue::Str(kind.into())),
            ("after".into(), DataValue::Str(after.unwrap_or("").into())),
        ]);
        let rows = owner.control_query(&format!(
            "?[id] := *bridge_owner_definition_management_catalog[owner,project,kind,id,session], \
             owner = $owner, project = $project, kind = $kind, id > $after \
             :order id :limit {limit}"), p, false)?;
        ensure!(rows.rows.len() <= limit, "management definition catalog page exceeds bound");
        let mut ids = Vec::with_capacity(rows.rows.len());
        let mut previous = after.unwrap_or("");
        for row in &rows.rows {
            ensure!(row.len() == 1, "invalid management definition catalog row");
            let id = text(row, 0)?;
            ensure!(id > previous, "management definition cursor is not stable and exclusive");
            owner.read_owner_definition(owner_principal, project, kind, id, None, None)?
                .context("management catalog has no original definition")?;
            ids.push(id.to_owned());
            previous = id;
        }
        Ok(ids)
    }

    /// Native owner authorization belongs in the controller, before this factory.
    /// One guarded head writer can publish an atomic pair, or disable its head.
    pub(crate) fn owner_definition_writer(&self, operation: &MethodOperation, project: &str,
        kind: &str, id: &str, next_version: Option<u64>, expected_revision: Option<u64>)
        -> Result<OrchestrationWriter> {
        let _boundary = self.inner.commit.lock().map_err(|_| anyhow::anyhow!("bridge commit owner poisoned"))?;
        let owner = self.inner.persistent.as_ref().context("durable bridge required")?.lock()
            .map_err(|_| anyhow::anyhow!("durable bridge owner poisoned"))?;
        let (scope, schema_hash, method, capability) = owner.definition_attribution(operation, kind, next_version.is_some())?;
        let StateScope::Private { principal_id, session_id } = scope else { anyhow::bail!("private native definition owner required"); };
        selector(&principal_id, project, kind, id)?;
        let schema = owner.pin(&self.inner.catalog, "workstacks", &schema_hash)?;
        ensure!(schema.methods.get(&method).is_some_and(|declaration| declaration.side_effect == op_state_store::SideEffect::Mutation
            && declaration.required_capability.as_deref() == Some(capability.as_str())), "definition requires its exact native mutation capability");
        let source = owner.definition_locator(&principal_id, kind, id)?;
        if let Some(source) = &source {
            ensure!(source.session == session_id, "definition source session changed; explicit owner review required");
            ensure!(source.project == project, "definition project cannot silently migrate");
        }
        ensure!(source.as_ref().map(|source| source.revision) == expected_revision, "global definition expected revision mismatch");
        if let Some(version) = next_version {
            ensure!(version == source.as_ref().map_or(0, |source| source.version).checked_add(1).context("definition version exhausted")?, "definition requires next immutable version");
        } else { ensure!(source.is_some(), "disable requires an original definition head"); }
        let guard = Arc::new(DefinitionGuard { owner: principal_id, session: session_id, project: project.to_owned(),
            kind: kind.to_owned(), id: id.to_owned(), operation_id: operation.operation_id().to_owned(), source, next_version });
        Ok(OrchestrationWriter { operation: operation.clone(), domain: "workstacks".into(),
            key: format!("{kind}:{id}"), definition_guard: Some(guard) })
    }
}
