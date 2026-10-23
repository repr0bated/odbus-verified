//! Typed APIs for Soul Memory (persistent agent identity) and
//! Agent → Namespace bindings. Both relations live in the same CozoDB instance
//! backing the rest of the cognitive memory store.

use crate::cozo_shuttle::CozoGraphShuttle;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use cozo::{DataValue, NamedRows, ScriptMutability};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Arc;

type Params = BTreeMap<String, DataValue>;

/// Soul memory = persistent identity for an agent. Survives sessions and
/// agent migrations. Versioned on every update.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SoulMemory {
    pub agent_id: String,
    pub identity: String,
    pub personality: String,
    pub traits: serde_json::Value,
    pub version: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Binding from an agent to its owning memory namespace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentNamespaceBinding {
    pub agent_id: String,
    pub namespace: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Default)]
pub struct SoulUpdate {
    pub identity: Option<String>,
    pub personality: Option<String>,
    pub traits: Option<serde_json::Value>,
}

pub struct SoulMemoryStore {
    shuttle: Arc<CozoGraphShuttle>,
}

impl SoulMemoryStore {
    pub fn new(shuttle: Arc<CozoGraphShuttle>) -> Self {
        Self { shuttle }
    }

    fn run(&self, script: &str, params: Params) -> Result<NamedRows> {
        self.shuttle
            .db()
            .run_script(script, params, ScriptMutability::Mutable)
            .map_err(|e| anyhow::anyhow!("{e}"))
    }

    // ── Soul ─────────────────────────────────────────────────────────────────

    pub async fn upsert_soul(&self, agent_id: &str, update: SoulUpdate) -> Result<SoulMemory> {
        let existing = self.get_soul(agent_id).await?;
        let now = Utc::now();
        let now_s = now.to_rfc3339();

        let identity = update
            .identity
            .or_else(|| existing.as_ref().map(|s| s.identity.clone()))
            .unwrap_or_default();
        let personality = update
            .personality
            .or_else(|| existing.as_ref().map(|s| s.personality.clone()))
            .unwrap_or_default();
        let traits = update
            .traits
            .or_else(|| existing.as_ref().map(|s| s.traits.clone()))
            .unwrap_or(serde_json::Value::Object(Default::default()));
        let version = existing.as_ref().map(|s| s.version + 1).unwrap_or(1);
        let created_at = existing
            .as_ref()
            .map(|s| s.created_at.to_rfc3339())
            .unwrap_or_else(|| now_s.clone());

        let q = r#"
            ?[agent_id, identity, personality, traits, version, created_at, updated_at]
                <- [[$id, $ident, $pers, $traits, $ver, $ca, $now]]
            :put soul_memories {
                agent_id => identity, personality, traits, version, created_at, updated_at
            }
        "#;
        let mut p: Params = BTreeMap::new();
        p.insert("id".into(), DataValue::Str(agent_id.into()));
        p.insert("ident".into(), DataValue::Str(identity.into()));
        p.insert("pers".into(), DataValue::Str(personality.into()));
        p.insert(
            "traits".into(),
            DataValue::Str(serde_json::to_string(&traits)?.into()),
        );
        p.insert("ver".into(), DataValue::Num(cozo::Num::Int(version)));
        p.insert("ca".into(), DataValue::Str(created_at.into()));
        p.insert("now".into(), DataValue::Str(now_s.into()));
        self.run(q, p).context("upsert soul")?;

        self.get_soul(agent_id)
            .await?
            .context("soul vanished after upsert")
    }

    pub async fn get_soul(&self, agent_id: &str) -> Result<Option<SoulMemory>> {
        let q = r#"
            ?[agent_id, identity, personality, traits, version, created_at, updated_at]
                := *soul_memories[agent_id, identity, personality, traits, version, created_at, updated_at],
                   agent_id = $id
        "#;
        let mut p: Params = BTreeMap::new();
        p.insert("id".into(), DataValue::Str(agent_id.into()));
        let rows = self.run(q, p).context("get soul")?;
        Ok(rows.rows.first().map(|r| row_to_soul(r)))
    }

    pub async fn delete_soul(&self, agent_id: &str) -> Result<bool> {
        if self.get_soul(agent_id).await?.is_none() {
            return Ok(false);
        }
        let mut p: Params = BTreeMap::new();
        p.insert("id".into(), DataValue::Str(agent_id.into()));
        self.run("?[agent_id] <- [[$id]] :rm soul_memories { agent_id }", p)
            .context("delete soul")?;
        Ok(true)
    }

    pub async fn list_souls(&self) -> Result<Vec<SoulMemory>> {
        let q = r#"
            ?[agent_id, identity, personality, traits, version, created_at, updated_at]
                := *soul_memories[agent_id, identity, personality, traits, version, created_at, updated_at]
            :order agent_id
        "#;
        let rows = self.run(q, BTreeMap::new()).context("list souls")?;
        Ok(rows.rows.iter().map(|r| row_to_soul(r)).collect())
    }

    // ── Agent → Namespace binding ────────────────────────────────────────────

    pub async fn bind_namespace(
        &self,
        agent_id: &str,
        namespace: &str,
    ) -> Result<AgentNamespaceBinding> {
        let existing = self.get_binding(agent_id).await?;
        let now = Utc::now();
        let now_s = now.to_rfc3339();
        let created_at = existing
            .as_ref()
            .map(|b| b.created_at.to_rfc3339())
            .unwrap_or_else(|| now_s.clone());

        let q = r#"
            ?[agent_id, namespace, created_at, updated_at]
                <- [[$id, $ns, $ca, $now]]
            :put agent_namespace_bindings {
                agent_id => namespace, created_at, updated_at
            }
        "#;
        let mut p: Params = BTreeMap::new();
        p.insert("id".into(), DataValue::Str(agent_id.into()));
        p.insert("ns".into(), DataValue::Str(namespace.into()));
        p.insert("ca".into(), DataValue::Str(created_at.into()));
        p.insert("now".into(), DataValue::Str(now_s.into()));
        self.run(q, p).context("bind namespace")?;

        self.get_binding(agent_id)
            .await?
            .context("binding vanished after upsert")
    }

    pub async fn get_binding(&self, agent_id: &str) -> Result<Option<AgentNamespaceBinding>> {
        let q = r#"
            ?[agent_id, namespace, created_at, updated_at]
                := *agent_namespace_bindings[agent_id, namespace, created_at, updated_at],
                   agent_id = $id
        "#;
        let mut p: Params = BTreeMap::new();
        p.insert("id".into(), DataValue::Str(agent_id.into()));
        let rows = self.run(q, p).context("get binding")?;
        Ok(rows.rows.first().map(|r| row_to_binding(r)))
    }

    pub async fn clear_binding(&self, agent_id: &str) -> Result<bool> {
        if self.get_binding(agent_id).await?.is_none() {
            return Ok(false);
        }
        let mut p: Params = BTreeMap::new();
        p.insert("id".into(), DataValue::Str(agent_id.into()));
        self.run(
            "?[agent_id] <- [[$id]] :rm agent_namespace_bindings { agent_id }",
            p,
        )
        .context("clear binding")?;
        Ok(true)
    }

    pub async fn list_bindings(&self) -> Result<Vec<AgentNamespaceBinding>> {
        let q = r#"
            ?[agent_id, namespace, created_at, updated_at]
                := *agent_namespace_bindings[agent_id, namespace, created_at, updated_at]
            :order agent_id
        "#;
        let rows = self.run(q, BTreeMap::new()).context("list bindings")?;
        Ok(rows.rows.iter().map(|r| row_to_binding(r)).collect())
    }
}

// ── Row → struct ────────────────────────────────────────────────────────────

fn row_to_soul(row: &[DataValue]) -> SoulMemory {
    SoulMemory {
        agent_id: dv_str(&row[0]),
        identity: dv_str(&row[1]),
        personality: dv_str(&row[2]),
        traits: serde_json::from_str(&dv_str(&row[3])).unwrap_or(serde_json::Value::Null),
        version: dv_int(&row[4]),
        created_at: parse_ts(&dv_str(&row[5])),
        updated_at: parse_ts(&dv_str(&row[6])),
    }
}

fn row_to_binding(row: &[DataValue]) -> AgentNamespaceBinding {
    AgentNamespaceBinding {
        agent_id: dv_str(&row[0]),
        namespace: dv_str(&row[1]),
        created_at: parse_ts(&dv_str(&row[2])),
        updated_at: parse_ts(&dv_str(&row[3])),
    }
}

fn dv_str(dv: &DataValue) -> String {
    if let DataValue::Str(s) = dv {
        s.to_string()
    } else {
        String::new()
    }
}

fn dv_int(dv: &DataValue) -> i64 {
    if let DataValue::Num(cozo::Num::Int(i)) = dv {
        *i
    } else {
        0
    }
}

fn parse_ts(s: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(s)
        .map(|t| t.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
}
