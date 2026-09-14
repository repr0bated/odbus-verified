//! Cognitive Memory Store
//!
//! Namespace-based shared memory backend for the op-dbus chatbot and openclaw.
//! Backed by the unified CozoDB store; no SQLite.
//!
//! Architecture:
//! - **Namespace** = a named context (project, session, database, workflow, cron job, agent, etc.)
//! - **Entry** = a key/value pair within a namespace, stored as JSON.
//! - Schema lives in [`CozoGraphShuttle::seed_schema`]; this module just exposes typed CRUD.

use crate::cozo_shuttle::CozoGraphShuttle;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use cozo::{DataValue, NamedRows, ScriptMutability};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Arc;
use uuid::Uuid;

type Params = BTreeMap<String, DataValue>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum NamespaceKind {
    Project,
    Session,
    Database,
    Workflow,
    Agent,
    Cron,
    Custom,
}

impl std::fmt::Display for NamespaceKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Project => "project",
            Self::Session => "session",
            Self::Database => "database",
            Self::Workflow => "workflow",
            Self::Agent => "agent",
            Self::Cron => "cron",
            Self::Custom => "custom",
        };
        write!(f, "{}", s)
    }
}

impl std::str::FromStr for NamespaceKind {
    type Err = ();
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        Ok(match s {
            "project" => Self::Project,
            "session" => Self::Session,
            "database" => Self::Database,
            "workflow" => Self::Workflow,
            "agent" => Self::Agent,
            "cron" => Self::Cron,
            _ => Self::Custom,
        })
    }
}

/// A named memory context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryNamespace {
    pub id: String,
    /// Canonical name: "project:op-dbus", "cron:backup", "db:ovsdb", etc.
    pub name: String,
    pub kind: NamespaceKind,
    pub description: Option<String>,
    pub linked_task_id: Option<String>,
    pub linked_cron: Option<String>,
    pub metadata: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A key/value entry within a namespace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub id: String,
    pub namespace_id: String,
    pub key: String,
    pub value: serde_json::Value,
    pub tags: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub access_count: i64,
    pub last_accessed: DateTime<Utc>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct EntryQuery {
    pub namespace_id: Option<String>,
    pub key_pattern: Option<String>,
    pub tags: Option<Vec<String>>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct MemoryStats {
    pub total_namespaces: i64,
    pub total_entries: i64,
    pub entries_by_kind: Vec<(String, i64)>,
}

pub struct CognitiveMemoryStore {
    shuttle: Arc<CozoGraphShuttle>,
}

impl CognitiveMemoryStore {
    pub async fn new(shuttle: Arc<CozoGraphShuttle>) -> Result<Self> {
        Ok(Self { shuttle })
    }

    fn run(&self, script: &str, params: Params) -> Result<NamedRows> {
        self.shuttle
            .db()
            .run_script(script, params, ScriptMutability::Mutable)
            .map_err(|e| anyhow::anyhow!("{e}"))
    }

    pub async fn upsert_namespace(
        &self,
        name: &str,
        kind: NamespaceKind,
        description: Option<&str>,
        linked_task_id: Option<&str>,
        linked_cron: Option<&str>,
        metadata: serde_json::Value,
    ) -> Result<MemoryNamespace> {
        let now = Utc::now().to_rfc3339();
        let id = Uuid::new_v4().to_string();
        let kind_str = kind.to_string();
        let meta_str = serde_json::to_string(&metadata)?;

        let q = r#"
            ?[name, id, kind, description, linked_task_id, linked_cron, metadata, created_at, updated_at]
                <- [[$name, $id, $kind, $desc, $task, $cron, $meta, $now, $now]]
            :put memory_namespaces {
                name => id, kind, description, linked_task_id, linked_cron, metadata, created_at, updated_at
            }
        "#;
        let mut p: Params = BTreeMap::new();
        p.insert("name".into(), DataValue::Str(name.into()));
        p.insert("id".into(), DataValue::Str(id.into()));
        p.insert("kind".into(), DataValue::Str(kind_str.into()));
        p.insert(
            "desc".into(),
            DataValue::Str(description.unwrap_or("").into()),
        );
        p.insert(
            "task".into(),
            DataValue::Str(linked_task_id.unwrap_or("").into()),
        );
        p.insert(
            "cron".into(),
            DataValue::Str(linked_cron.unwrap_or("").into()),
        );
        p.insert("meta".into(), DataValue::Str(meta_str.into()));
        p.insert("now".into(), DataValue::Str(now.into()));
        self.run(q, p).context("upsert namespace")?;

        self.get_namespace_by_name(name)
            .await?
            .context("namespace not found after upsert")
    }

    pub async fn get_namespace_by_name(&self, name: &str) -> Result<Option<MemoryNamespace>> {
        let q = r#"
            ?[name, id, kind, description, linked_task_id, linked_cron, metadata, created_at, updated_at]
                := *memory_namespaces[name, id, kind, description, linked_task_id, linked_cron, metadata, created_at, updated_at],
                   name = $name
        "#;
        let mut p: Params = BTreeMap::new();
        p.insert("name".into(), DataValue::Str(name.into()));
        let rows = self.run(q, p).context("get namespace by name")?;
        Ok(rows.rows.first().map(|r| row_to_namespace(r)))
    }

    pub async fn list_namespaces(
        &self,
        kind: Option<NamespaceKind>,
    ) -> Result<Vec<MemoryNamespace>> {
        let (q, params): (&str, Params) = if let Some(k) = kind {
            let mut p: Params = BTreeMap::new();
            p.insert("k".into(), DataValue::Str(k.to_string().into()));
            (
                r#"
                ?[name, id, kind, description, linked_task_id, linked_cron, metadata, created_at, updated_at]
                    := *memory_namespaces[name, id, kind, description, linked_task_id, linked_cron, metadata, created_at, updated_at],
                       kind = $k
                :order name
                "#,
                p,
            )
        } else {
            (
                r#"
                ?[name, id, kind, description, linked_task_id, linked_cron, metadata, created_at, updated_at]
                    := *memory_namespaces[name, id, kind, description, linked_task_id, linked_cron, metadata, created_at, updated_at]
                :order name
                "#,
                BTreeMap::new(),
            )
        };
        let rows = self.run(q, params).context("list namespaces")?;
        Ok(rows.rows.iter().map(|r| row_to_namespace(r)).collect())
    }

    pub async fn delete_namespace(&self, name: &str) -> Result<bool> {
        // Pre-check whether it exists; cozo :rm is silent.
        if self.get_namespace_by_name(name).await?.is_none() {
            return Ok(false);
        }
        // Cascade: remove all entries in this namespace atomically in a single query.
        let mut p: Params = BTreeMap::new();
        p.insert("ns".into(), DataValue::Str(name.into()));
        let cascade_q = r#"
            ?[namespace, key]
                := *memory_entries[namespace, key, _, _, _, _, _, _, _, _],
                   namespace = $ns
            :rm memory_entries { namespace, key }
        "#;
        self.run(cascade_q, p).context("cascade delete entries")?;

        // Remove namespace row.
        let mut pn: Params = BTreeMap::new();
        pn.insert("name".into(), DataValue::Str(name.into()));
        self.run("?[name] <- [[$name]] :rm memory_namespaces { name }", pn)
            .context("delete namespace")?;
        Ok(true)
    }

    pub async fn store_entry(
        &self,
        namespace_name: &str,
        key: &str,
        value: serde_json::Value,
        tags: Vec<String>,
        expires_at: Option<DateTime<Utc>>,
    ) -> Result<MemoryEntry> {
        // Ensure namespace exists.
        self.get_namespace_by_name(namespace_name)
            .await?
            .with_context(|| format!("namespace '{}' not found", namespace_name))?;

        let now = Utc::now().to_rfc3339();
        let value_str = serde_json::to_string(&value)?;
        let tags_str = serde_json::to_string(&tags)?;
        let exp_str = expires_at.map(|t| t.to_rfc3339()).unwrap_or_default();

        // Preserve created_at + access counters on update by reading existing row first.
        let existing = self.fetch_entry_row(namespace_name, key)?;
        let (id, created_at, access_count, last_accessed) = match existing {
            Some(ref row) => (
                dv_as_str(&row[2]).unwrap_or("").to_string(),
                dv_as_str(&row[5]).unwrap_or(now.as_str()).to_string(),
                dv_as_int(&row[8]).unwrap_or(0),
                dv_as_str(&row[9]).unwrap_or(now.as_str()).to_string(),
            ),
            None => (Uuid::new_v4().to_string(), now.clone(), 0, now.clone()),
        };

        let q = r#"
            ?[namespace, key, id, value, tags, created_at, updated_at, expires_at, access_count, last_accessed]
                <- [[$ns, $key, $id, $val, $tags, $ca, $now, $exp, $ac, $la]]
            :put memory_entries {
                namespace, key => id, value, tags, created_at, updated_at, expires_at, access_count, last_accessed
            }
        "#;
        let mut p: Params = BTreeMap::new();
        p.insert("ns".into(), DataValue::Str(namespace_name.into()));
        p.insert("key".into(), DataValue::Str(key.into()));
        p.insert("id".into(), DataValue::Str(id.into()));
        p.insert("val".into(), DataValue::Str(value_str.into()));
        p.insert("tags".into(), DataValue::Str(tags_str.into()));
        p.insert("ca".into(), DataValue::Str(created_at.into()));
        p.insert("now".into(), DataValue::Str(now.into()));
        p.insert("exp".into(), DataValue::Str(exp_str.into()));
        p.insert("ac".into(), DataValue::Num(cozo::Num::Int(access_count)));
        p.insert("la".into(), DataValue::Str(last_accessed.into()));
        self.run(q, p).context("store entry")?;

        self.retrieve_entry(namespace_name, key)
            .await?
            .context("entry not found after store")
    }

    pub async fn retrieve_entry(
        &self,
        namespace_name: &str,
        key: &str,
    ) -> Result<Option<MemoryEntry>> {
        let Some(row) = self.fetch_entry_row(namespace_name, key)? else {
            return Ok(None);
        };
        let entry = row_to_entry(&row);

        // Bump access counters (best-effort; ignore errors).
        let now = Utc::now().to_rfc3339();
        let q = r#"
            ?[namespace, key, id, value, tags, created_at, updated_at, expires_at, access_count, last_accessed]
                <- [[$ns, $key, $id, $val, $tags, $ca, $ua, $exp, $ac, $la]]
            :put memory_entries {
                namespace, key => id, value, tags, created_at, updated_at, expires_at, access_count, last_accessed
            }
        "#;
        let mut p: Params = BTreeMap::new();
        p.insert("ns".into(), DataValue::Str(namespace_name.into()));
        p.insert("key".into(), DataValue::Str(key.into()));
        p.insert("id".into(), DataValue::Str(entry.id.clone().into()));
        p.insert(
            "val".into(),
            DataValue::Str(serde_json::to_string(&entry.value)?.into()),
        );
        p.insert(
            "tags".into(),
            DataValue::Str(serde_json::to_string(&entry.tags)?.into()),
        );
        p.insert(
            "ca".into(),
            DataValue::Str(entry.created_at.to_rfc3339().into()),
        );
        p.insert(
            "ua".into(),
            DataValue::Str(entry.updated_at.to_rfc3339().into()),
        );
        p.insert(
            "exp".into(),
            DataValue::Str(
                entry
                    .expires_at
                    .map(|t| t.to_rfc3339())
                    .unwrap_or_default()
                    .into(),
            ),
        );
        p.insert(
            "ac".into(),
            DataValue::Num(cozo::Num::Int(entry.access_count + 1)),
        );
        p.insert("la".into(), DataValue::Str(now.into()));
        let _ = self.run(q, p);

        Ok(Some(entry))
    }

    pub async fn query_entries(&self, q: EntryQuery) -> Result<Vec<MemoryEntry>> {
        let now = Utc::now().to_rfc3339();
        let limit = q.limit.unwrap_or(100) as usize;
        let offset = q.offset.unwrap_or(0) as usize;

        let (script, params): (&str, Params) = match q.namespace_id.as_deref() {
            Some(ns) => {
                let mut p: Params = BTreeMap::new();
                p.insert("ns".into(), DataValue::Str(ns.into()));
                p.insert("now".into(), DataValue::Str(now.into()));
                (
                    r#"
                    ?[namespace, key, id, value, tags, created_at, updated_at, expires_at, access_count, last_accessed]
                        := *memory_entries[namespace, key, id, value, tags, created_at, updated_at, expires_at, access_count, last_accessed],
                           namespace = $ns,
                           (expires_at == "" || expires_at > $now)
                    :order -updated_at
                    "#,
                    p,
                )
            }
            None => {
                let mut p: Params = BTreeMap::new();
                p.insert("now".into(), DataValue::Str(now.into()));
                (
                    r#"
                    ?[namespace, key, id, value, tags, created_at, updated_at, expires_at, access_count, last_accessed]
                        := *memory_entries[namespace, key, id, value, tags, created_at, updated_at, expires_at, access_count, last_accessed],
                           (expires_at == "" || expires_at > $now)
                    :order -updated_at
                    "#,
                    p,
                )
            }
        };
        let rows = self.run(script, params).context("query entries")?;

        let mut entries: Vec<MemoryEntry> = rows.rows.iter().map(|r| row_to_entry(r)).collect();

        // Apply key_pattern (substring match) post-fetch.
        if let Some(pat) = &q.key_pattern {
            entries.retain(|e| e.key.contains(pat));
        }
        // Tag filter: every requested tag must be present.
        if let Some(tags) = &q.tags {
            entries.retain(|e| tags.iter().all(|t| e.tags.contains(t)));
        }
        // Offset + limit.
        Ok(entries.into_iter().skip(offset).take(limit).collect())
    }

    pub async fn delete_entry(&self, namespace_name: &str, key: &str) -> Result<bool> {
        if self.fetch_entry_row(namespace_name, key)?.is_none() {
            return Ok(false);
        }
        let mut p: Params = BTreeMap::new();
        p.insert("ns".into(), DataValue::Str(namespace_name.into()));
        p.insert("key".into(), DataValue::Str(key.into()));
        self.run(
            "?[namespace, key] <- [[$ns, $key]] :rm memory_entries { namespace, key }",
            p,
        )
        .context("delete entry")?;
        Ok(true)
    }

    pub async fn cleanup_expired(&self) -> Result<u64> {
        let now = Utc::now().to_rfc3339();
        // Collect expired keys.
        let q = r#"
            ?[namespace, key]
                := *memory_entries[namespace, key, _, _, _, _, _, expires_at, _, _],
                   expires_at != "",
                   expires_at < $now
        "#;
        let mut p: Params = BTreeMap::new();
        p.insert("now".into(), DataValue::Str(now.into()));
        let rows = self.run(q, p).context("collect expired")?;
        let mut removed: u64 = 0;
        for row in &rows.rows {
            let ns = dv_as_str(&row[0]).unwrap_or("").to_string();
            let key = dv_as_str(&row[1]).unwrap_or("").to_string();
            let mut pr: Params = BTreeMap::new();
            pr.insert("ns".into(), DataValue::Str(ns.into()));
            pr.insert("key".into(), DataValue::Str(key.into()));
            if self
                .run(
                    "?[namespace, key] <- [[$ns, $key]] :rm memory_entries { namespace, key }",
                    pr,
                )
                .is_ok()
            {
                removed += 1;
            }
        }
        Ok(removed)
    }

    pub async fn get_stats(&self) -> Result<MemoryStats> {
        // Cheap counts: fetch all + count in Rust. Cozo aggregates would be tighter at scale.
        let ns_rows = self
            .run(
                r#"
                ?[name, kind]
                    := *memory_namespaces[name, _, kind, _, _, _, _, _, _]
                "#,
                BTreeMap::new(),
            )
            .context("count namespaces")?;
        let total_namespaces = ns_rows.rows.len() as i64;

        let entry_rows = self
            .run(
                r#"
                ?[namespace]
                    := *memory_entries[namespace, _, _, _, _, _, _, _, _, _]
                "#,
                BTreeMap::new(),
            )
            .context("count entries")?;
        let total_entries = entry_rows.rows.len() as i64;

        // Build name → kind map, then count entries per kind.
        let mut kind_by_ns: BTreeMap<String, String> = BTreeMap::new();
        for row in &ns_rows.rows {
            let name = dv_as_str(&row[0]).unwrap_or("").to_string();
            let kind = dv_as_str(&row[1]).unwrap_or("custom").to_string();
            kind_by_ns.insert(name, kind);
        }
        let mut tally: BTreeMap<String, i64> = BTreeMap::new();
        for row in &entry_rows.rows {
            let ns = dv_as_str(&row[0]).unwrap_or("").to_string();
            let kind = kind_by_ns
                .get(&ns)
                .cloned()
                .unwrap_or_else(|| "custom".to_string());
            *tally.entry(kind).or_insert(0) += 1;
        }
        let entries_by_kind: Vec<(String, i64)> = tally.into_iter().collect();

        Ok(MemoryStats {
            total_namespaces,
            total_entries,
            entries_by_kind,
        })
    }

    /// Internal helper: fetch a raw memory_entries row by (namespace, key).
    /// Column order matches the relation declaration in `cozo_shuttle::seed_schema`.
    fn fetch_entry_row(&self, namespace_name: &str, key: &str) -> Result<Option<Vec<DataValue>>> {
        let q = r#"
            ?[namespace, key, id, value, tags, created_at, updated_at, expires_at, access_count, last_accessed]
                := *memory_entries[namespace, key, id, value, tags, created_at, updated_at, expires_at, access_count, last_accessed],
                   namespace = $ns,
                   key = $key
        "#;
        let mut p: Params = BTreeMap::new();
        p.insert("ns".into(), DataValue::Str(namespace_name.into()));
        p.insert("key".into(), DataValue::Str(key.into()));
        let rows = self.run(q, p).context("fetch entry row")?;
        Ok(rows.rows.into_iter().next())
    }
}

// ── Row → typed struct conversion ─────────────────────────────────────────────

fn row_to_namespace(row: &[DataValue]) -> MemoryNamespace {
    // Order matches the rule head:
    //   name, id, kind, description, linked_task_id, linked_cron, metadata, created_at, updated_at
    let name = dv_as_str(&row[0]).unwrap_or("").to_string();
    let id = dv_as_str(&row[1]).unwrap_or("").to_string();
    let kind_str = dv_as_str(&row[2]).unwrap_or("custom").to_string();
    let description = opt_string(&row[3]);
    let linked_task_id = opt_string(&row[4]);
    let linked_cron = opt_string(&row[5]);
    let meta_str = dv_as_str(&row[6]).unwrap_or("{}");
    let created = dv_as_str(&row[7]).unwrap_or("");
    let updated = dv_as_str(&row[8]).unwrap_or("");

    MemoryNamespace {
        id,
        name,
        kind: kind_str.parse().unwrap_or(NamespaceKind::Custom),
        description,
        linked_task_id,
        linked_cron,
        metadata: serde_json::from_str(meta_str).unwrap_or(serde_json::Value::Null),
        created_at: parse_ts(created),
        updated_at: parse_ts(updated),
    }
}

fn row_to_entry(row: &[DataValue]) -> MemoryEntry {
    // Order matches the rule head:
    //   namespace, key, id, value, tags, created_at, updated_at, expires_at, access_count, last_accessed
    let namespace_id = dv_as_str(&row[0]).unwrap_or("").to_string();
    let key = dv_as_str(&row[1]).unwrap_or("").to_string();
    let id = dv_as_str(&row[2]).unwrap_or("").to_string();
    let value_str = dv_as_str(&row[3]).unwrap_or("null");
    let tags_str = dv_as_str(&row[4]).unwrap_or("[]");
    let created = dv_as_str(&row[5]).unwrap_or("");
    let updated = dv_as_str(&row[6]).unwrap_or("");
    let expires = dv_as_str(&row[7]).unwrap_or("");
    let access_count = dv_as_int(&row[8]).unwrap_or(0);
    let last_accessed = dv_as_str(&row[9]).unwrap_or("");

    MemoryEntry {
        id,
        namespace_id,
        key,
        value: serde_json::from_str(value_str).unwrap_or(serde_json::Value::Null),
        tags: serde_json::from_str(tags_str).unwrap_or_default(),
        created_at: parse_ts(created),
        updated_at: parse_ts(updated),
        expires_at: if expires.is_empty() {
            None
        } else {
            DateTime::parse_from_rfc3339(expires)
                .map(|t| t.with_timezone(&Utc))
                .ok()
        },
        access_count,
        last_accessed: parse_ts(last_accessed),
    }
}

fn opt_string(dv: &DataValue) -> Option<String> {
    match dv_as_str(dv) {
        Some(s) if !s.is_empty() => Some(s.to_string()),
        _ => None,
    }
}

fn dv_as_str(dv: &DataValue) -> Option<&str> {
    if let DataValue::Str(s) = dv {
        Some(s.as_str())
    } else {
        None
    }
}

fn dv_as_int(dv: &DataValue) -> Option<i64> {
    if let DataValue::Num(cozo::Num::Int(i)) = dv {
        Some(*i)
    } else {
        None
    }
}

fn parse_ts(s: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(s)
        .map(|t| t.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn query_entries_returns_active_rows_and_filters_expired_rows() {
        let shuttle = Arc::new(CozoGraphShuttle::new_in_memory().unwrap());
        let store = CognitiveMemoryStore::new(shuttle).await.unwrap();
        store
            .upsert_namespace(
                "container:expiry-probe",
                NamespaceKind::Custom,
                None,
                None,
                None,
                serde_json::json!({}),
            )
            .await
            .unwrap();
        store
            .store_entry(
                "container:expiry-probe",
                "active",
                serde_json::json!("keep"),
                Vec::new(),
                None,
            )
            .await
            .unwrap();
        store
            .store_entry(
                "container:expiry-probe",
                "expired",
                serde_json::json!("drop"),
                Vec::new(),
                Some(Utc::now() - chrono::Duration::minutes(1)),
            )
            .await
            .unwrap();

        let entries = store
            .query_entries(EntryQuery {
                namespace_id: Some("container:expiry-probe".to_string()),
                ..EntryQuery::default()
            })
            .await
            .unwrap();

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].key, "active");
    }
}
