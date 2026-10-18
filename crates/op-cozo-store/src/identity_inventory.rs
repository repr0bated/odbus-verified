//! Bounded metadata-only inspection through the private identity store owner.
//! Queries deliberately never select credential, key, genesis, instance or event values.
use super::*;
use cozo::MultiTransaction;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

const MAX_RESULT_BYTES: usize = 128 * 1024;
const MAX_INVENTORY_KEYS: u64 = 50_000;
const IDS: &str = "ids[session_id] := *identity_sessions{session_id}\n\
ids[session_id] := *identity_revisions{session_id}\n\
ids[session_id] := *identity_root_repairs{session_id}\n\
ids[session_id] := *identity_seed_provisions{session_id}\n\
ids[session_id] := *identity_physical_root_migrations{session_id}\n";
const PAGE: &str = "page[session_id] <- $page\n";

fn default_limit() -> u32 {
    50
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IdentityInventoryQuery {
    #[serde(default)]
    #[schemars(length(min = 1, max = 128))]
    pub after_session_id: Option<String>,
    #[serde(default = "default_limit")]
    #[schemars(range(min = 1, max = 100))]
    pub limit: u32,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct IdentityInventoryCounts {
    pub session_rows: u64,
    pub revision_rows: u64,
    /// A retained revision with no current identity row, never a deletion candidate by itself.
    pub tombstones: u64,
    /// IDs present only in storage journals, with neither identity nor revision row.
    pub journal_only_ids: u64,
    pub total_ids: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct IdentityInventoryEntry {
    #[schemars(length(min = 1, max = 128))]
    pub session_id: String,
    pub record_present: bool,
    pub tombstone: bool,
    pub revision: Option<u64>,
    #[schemars(length(max = 16))]
    pub principal_kind: Option<String>,
    pub active: Option<bool>,
    pub mutation_index: Option<u64>,
    pub last_seen_at: Option<i64>,
    #[schemars(length(max = 64))]
    pub root_repair_phase: Option<String>,
    #[schemars(length(max = 64))]
    pub seed_provision_phase: Option<String>,
    pub physical_root_migration_revision: Option<u64>,
    #[schemars(length(max = 64))]
    pub physical_root_migration_plan_sha256: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct IdentityInventoryPage {
    pub counts: IdentityInventoryCounts,
    #[schemars(length(max = 100))]
    pub entries: Vec<IdentityInventoryEntry>,
    #[schemars(length(max = 128))]
    pub next_after_session_id: Option<String>,
    /// Counts and entries share one read transaction. Later pages may see later commits.
    pub consistency: String,
}

fn fail(message: &str) -> CozoError {
    CozoError::Other(message.into())
}
fn query(
    tx: &MultiTransaction,
    script: &str,
    params: Params,
    started: Instant,
) -> std::result::Result<NamedRows, CozoError> {
    if started.elapsed() > Duration::from_secs(5) {
        return Err(fail("identity inventory read budget exceeded"));
    }
    tx.run_script(script, params)
        .map_err(|_| fail("identity inventory metadata query failed"))
}
fn text(row: &[DataValue], at: usize, max: usize) -> std::result::Result<String, CozoError> {
    match row.get(at) {
        Some(DataValue::Str(value))
            if value.len() <= max && !value.chars().any(char::is_control) =>
        {
            Ok(value.to_string())
        }
        _ => Err(fail("corrupt identity inventory string metadata")),
    }
}
fn integer(row: &[DataValue], at: usize) -> std::result::Result<i64, CozoError> {
    match row.get(at) {
        Some(DataValue::Num(Num::Int(value))) if *value >= 0 => Ok(*value),
        _ => Err(fail("corrupt identity inventory integer metadata")),
    }
}
fn count(
    tx: &MultiTransaction,
    body: &str,
    started: Instant,
) -> std::result::Result<u64, CozoError> {
    let rows = query(tx, body, BTreeMap::new(), started)?;
    match rows.rows.as_slice() {
        [] => Ok(0),
        [row] if row.len() == 1 => Ok(integer(row, 0)? as u64),
        _ => Err(fail("corrupt identity inventory count metadata")),
    }
}
fn member<'a>(
    entries: &'a mut BTreeMap<String, IdentityInventoryEntry>,
    row: &[DataValue],
) -> std::result::Result<&'a mut IdentityInventoryEntry, CozoError> {
    let sid = text(row, 0, 128)?;
    entries
        .get_mut(&sid)
        .ok_or_else(|| fail("identity inventory metadata escaped page"))
}

impl CozoGraphShuttle {
    pub fn inspect_identity_inventory(
        &self,
        input: IdentityInventoryQuery,
    ) -> std::result::Result<IdentityInventoryPage, CozoError> {
        if !(1..=100).contains(&input.limit)
            || input.after_session_id.as_ref().is_some_and(|id| {
                id.is_empty() || id.len() > 128 || id.chars().any(char::is_control)
            })
        {
            return Err(fail("invalid identity inventory page bounds"));
        }
        let _guard = self
            .identity_transaction
            .lock()
            .map_err(|_| fail("identity transaction owner is poisoned"))?;
        let started = Instant::now();
        let tx = self.db.multi_transaction(false);
        let counts = IdentityInventoryCounts {
            session_rows: count(
                &tx,
                "?[count(session_id)] := *identity_sessions{session_id} :timeout 1",
                started,
            )?,
            revision_rows: count(
                &tx,
                "?[count(session_id)] := *identity_revisions{session_id} :timeout 1",
                started,
            )?,
            tombstones: count(
                &tx,
                "?[count(session_id)] := *identity_revisions{session_id}, not *identity_sessions{session_id} :timeout 1",
                started,
            )?,
            journal_only_ids: count(
                &tx,
                "journal[session_id] := *identity_root_repairs{session_id}\njournal[session_id] := *identity_seed_provisions{session_id}\n?[count(session_id)] := journal[session_id], not *identity_sessions{session_id}, not *identity_revisions{session_id} :timeout 1",
                started,
            )?,
            total_ids: count(
                &tx,
                &format!("{IDS}?[count(session_id)] := ids[session_id] :timeout 1"),
                started,
            )?,
        };
        if counts.total_ids > MAX_INVENTORY_KEYS {
            return Err(fail("identity inventory exceeds supported key bound"));
        }
        let after = input.after_session_id.unwrap_or_default();
        let ids = query(
            &tx,
            &format!(
                "{IDS}?[session_id] := ids[session_id], session_id > $after :order session_id :limit {} :timeout 1",
                input.limit + 1
            ),
            BTreeMap::from([("after".into(), DataValue::Str(after.into()))]),
            started,
        )?;
        let mut entries = BTreeMap::new();
        let mut more = false;
        for row in ids.rows {
            if row.len() != 1 {
                return Err(fail("corrupt identity inventory key row"));
            }
            let sid = text(&row, 0, 128)?;
            if sid.is_empty() {
                return Err(fail("empty identity inventory key"));
            }
            if entries.len() == input.limit as usize {
                more = true;
                break;
            }
            if entries
                .insert(
                    sid.clone(),
                    IdentityInventoryEntry {
                        session_id: sid,
                        record_present: false,
                        tombstone: false,
                        revision: None,
                        principal_kind: None,
                        active: None,
                        mutation_index: None,
                        last_seen_at: None,
                        root_repair_phase: None,
                        seed_provision_phase: None,
                        physical_root_migration_revision: None,
                        physical_root_migration_plan_sha256: None,
                    },
                )
                .is_some()
            {
                return Err(fail("duplicate identity inventory key"));
            }
        }
        let params = BTreeMap::from([(
            "page".into(),
            DataValue::List(
                entries
                    .keys()
                    .map(|id| DataValue::List(vec![DataValue::Str(id.clone().into())]))
                    .collect(),
            ),
        )]);
        if !entries.is_empty() {
            let rows = query(
                &tx,
                &format!(
                    "{PAGE}?[session_id,active,mutation_index,last_seen_at] := page[session_id], *identity_sessions{{session_id,active,mutation_index,last_seen_at}} :timeout 1"
                ),
                params.clone(),
                started,
            )?;
            for row in rows.rows {
                if row.len() != 4 {
                    return Err(fail("corrupt identity inventory session row"));
                }
                let entry = member(&mut entries, &row)?;
                if entry.record_present {
                    return Err(fail("duplicate identity inventory session metadata"));
                }
                entry.record_present = true;
                entry.active = match row.get(1) {
                    Some(DataValue::Bool(value)) => Some(*value),
                    _ => return Err(fail("corrupt identity inventory active metadata")),
                };
                entry.mutation_index = Some(integer(&row, 2)? as u64);
                entry.last_seen_at = Some(integer(&row, 3)?);
            }
            let rows = query(
                &tx,
                &format!(
                    "{PAGE}?[session_id,revision,principal_kind] := page[session_id], *identity_revisions{{session_id,revision,principal_kind}} :timeout 1"
                ),
                params.clone(),
                started,
            )?;
            for row in rows.rows {
                if row.len() != 3 {
                    return Err(fail("corrupt identity inventory revision row"));
                }
                let entry = member(&mut entries, &row)?;
                let revision = integer(&row, 1)? as u64;
                if revision == 0 || entry.revision.replace(revision).is_some() {
                    return Err(fail("corrupt identity inventory revision metadata"));
                }
                let kind = text(&row, 2, 16)?;
                entry.principal_kind = match kind.as_str() {
                    "" => None,
                    "human" | "service" => Some(kind),
                    _ => return Err(fail("corrupt identity inventory actor metadata")),
                };
                entry.tombstone = !entry.record_present;
            }
            let migrations = query(
                &tx,
                &format!(
                    "{PAGE}?[session_id,committed_revision,plan_sha256] := page[session_id], *identity_physical_root_migrations{{session_id,committed_revision,plan_sha256}} :timeout 1"
                ),
                params.clone(),
                started,
            )?;
            for row in migrations.rows {
                if row.len() != 3 {
                    return Err(fail("corrupt physical migration inventory row"));
                }
                let revision = integer(&row, 1)? as u64;
                let digest = text(&row, 2, 64)?;
                if revision == 0
                    || digest.len() != 64
                    || !digest
                        .bytes()
                        .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
                {
                    return Err(fail("corrupt physical migration inventory metadata"));
                }
                let entry = member(&mut entries, &row)?;
                if entry
                    .physical_root_migration_revision
                    .replace(revision)
                    .is_some()
                {
                    return Err(fail("duplicate physical migration inventory metadata"));
                }
                entry.physical_root_migration_plan_sha256 = Some(digest);
            }
            for (relation, repair) in [
                ("identity_root_repairs", true),
                ("identity_seed_provisions", false),
            ] {
                let rows = query(
                    &tx,
                    &format!(
                        "{PAGE}?[session_id,phase] := page[session_id], *{relation}{{session_id,phase}} :timeout 1"
                    ),
                    params.clone(),
                    started,
                )?;
                for row in rows.rows {
                    if row.len() != 2 {
                        return Err(fail("corrupt identity inventory journal row"));
                    }
                    let phase = text(&row, 1, 64)?;
                    if phase.is_empty()
                        || !phase.bytes().all(|b| b.is_ascii_lowercase() || b == b'-')
                    {
                        return Err(fail("corrupt identity inventory journal phase"));
                    }
                    let entry = member(&mut entries, &row)?;
                    let old = if repair {
                        entry.root_repair_phase.replace(phase)
                    } else {
                        entry.seed_provision_phase.replace(phase)
                    };
                    if old.is_some() {
                        return Err(fail("duplicate identity inventory journal metadata"));
                    }
                }
            }
        }
        super::identity_transactions::commit_checked(&tx)?;
        let next_after_session_id = more.then(|| {
            entries
                .keys()
                .next_back()
                .expect("nonempty bounded page")
                .clone()
        });
        let page = IdentityInventoryPage { counts, entries: entries.into_values().collect(), next_after_session_id, consistency: "single_read_transaction; subsequent_pages_may_change; metadata_is_not_native_runtime_state".into() };
        if serde_json::to_vec(&page)?.len() > MAX_RESULT_BYTES {
            return Err(fail("identity inventory result exceeds byte bound"));
        }
        Ok(page)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn query_page(
        store: &CozoGraphShuttle,
        after: Option<&str>,
        limit: u32,
    ) -> IdentityInventoryPage {
        store
            .inspect_identity_inventory(IdentityInventoryQuery {
                after_session_id: after.map(str::to_string),
                limit,
            })
            .unwrap()
    }
    fn seed(store: &CozoGraphShuttle) {
        // Disposable rows deliberately include secret sentinels in columns never queried.
        cozo_run(&store.db, "?[session_id,wireguard_pubkey,sealed_id,session_genesis,instance_json,active,mutation_index,last_seen_at] <- [['a','PRIVATE_KEY_SENTINEL','SID1_PRIVATE_SENTINEL','GENESIS_PRIVATE_SENTINEL','INSTANCE_PRIVATE_SENTINEL',true,7,100],['c','PRIVATE_KEY_SENTINEL','SID1_PRIVATE_SENTINEL','GENESIS_PRIVATE_SENTINEL','INSTANCE_PRIVATE_SENTINEL',false,9,200]] :put identity_sessions {session_id => wireguard_pubkey,sealed_id,session_genesis,instance_json,active,mutation_index,last_seen_at}", BTreeMap::new()).unwrap();
        cozo_run(&store.db, "?[session_id,revision,operation_id,intent_kind,intent_value,principal_kind] <- [['a',1,'a-op','cas',0,'human'],['b',2,'b-op','cas',1,'service']] :put identity_revisions {session_id => revision,operation_id,intent_kind,intent_value,principal_kind}", BTreeMap::new()).unwrap();
        cozo_run(&store.db, "?[session_id,version,plan_sha256,operation_id,phase,expected_revision,expected_revision_operation_id,updated_at] <- [['d',1,'PLAN_PRIVATE_SENTINEL','d-op','prepared',0,'d-before',1]] :put identity_root_repairs {session_id => version,plan_sha256,operation_id,phase,expected_revision,expected_revision_operation_id,updated_at}", BTreeMap::new()).unwrap();
        cozo_run(&store.db, "?[session_id,version,plan_sha256,operation_id,phase,expected_revision,expected_revision_operation_id,updated_at] <- [['a',1,'PLAN_PRIVATE_SENTINEL','a-op','identity-committed',0,'a-before',1],['d',1,'PLAN_PRIVATE_SENTINEL','d-op','prepared',0,'d-before',1]] :put identity_seed_provisions {session_id => version,plan_sha256,operation_id,phase,expected_revision,expected_revision_operation_id,updated_at}", BTreeMap::new()).unwrap();
    }
    #[test]
    fn identity_inventory_empty_and_page_bounds() {
        let store = CozoGraphShuttle::new_in_memory().unwrap();
        let page = query_page(&store, None, 100);
        assert_eq!(page.counts.total_ids, 0);
        assert!(page.entries.is_empty());
        for limit in [0, 101, u32::MAX] {
            assert!(
                store
                    .inspect_identity_inventory(IdentityInventoryQuery {
                        after_session_id: None,
                        limit
                    })
                    .is_err()
            );
        }
        for cursor in ["", "\n"] {
            assert!(
                store
                    .inspect_identity_inventory(IdentityInventoryQuery {
                        after_session_id: Some(cursor.into()),
                        limit: 1
                    })
                    .is_err()
            );
        }
    }
    #[test]
    fn identity_inventory_pages_include_legacy_tombstone_and_journal_only_without_secrets() {
        let store = CozoGraphShuttle::new_in_memory().unwrap();
        seed(&store);
        let first = query_page(&store, None, 2);
        assert_eq!(
            first.counts,
            IdentityInventoryCounts {
                session_rows: 2,
                revision_rows: 2,
                tombstones: 1,
                journal_only_ids: 1,
                total_ids: 4
            }
        );
        assert_eq!(
            first
                .entries
                .iter()
                .map(|e| e.session_id.as_str())
                .collect::<Vec<_>>(),
            vec!["a", "b"]
        );
        assert!(first.entries[0].record_present);
        assert_eq!(first.entries[0].principal_kind.as_deref(), Some("human"));
        assert_eq!(
            first.entries[0].seed_provision_phase.as_deref(),
            Some("identity-committed")
        );
        assert!(first.entries[1].tombstone);
        let second = query_page(&store, first.next_after_session_id.as_deref(), 2);
        assert_eq!(first.counts, second.counts);
        assert_eq!(
            second
                .entries
                .iter()
                .map(|e| e.session_id.as_str())
                .collect::<Vec<_>>(),
            vec!["c", "d"]
        );
        assert!(second.entries[0].record_present && second.entries[0].revision.is_none());
        assert!(!second.entries[1].record_present && !second.entries[1].tombstone);
        assert_eq!(
            second.entries[1].root_repair_phase.as_deref(),
            Some("prepared")
        );
        assert!(second.next_after_session_id.is_none());
        let serialized = serde_json::to_string(&(first, second)).unwrap();
        for forbidden in [
            "SENTINEL",
            "wireguard_pubkey",
            "sealed_id",
            "genesis",
            "instance_json",
            "btrfs_device",
        ] {
            assert!(!serialized.contains(forbidden));
        }
    }
    #[test]
    fn identity_inventory_rejects_corrupt_metadata_without_diagnostics() {
        let store = CozoGraphShuttle::new_in_memory().unwrap();
        seed(&store);
        cozo_run(&store.db,"?[session_id,mutation_index] <- [['a',-1]] :update identity_sessions {session_id => mutation_index}",BTreeMap::new()).unwrap();
        let error = store
            .inspect_identity_inventory(IdentityInventoryQuery {
                after_session_id: None,
                limit: 100,
            })
            .unwrap_err()
            .to_string();
        assert!(error.contains("integer metadata"));
        assert!(!error.contains("SENTINEL"));
    }
}
