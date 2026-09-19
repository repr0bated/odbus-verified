//! Event triggers for agent runs (OD-63): sealed plugin state owned by the
//! `persona` plugin, stored as its durable runtime document.
//!
//! A trigger binds a broker topic filter to an **agent identity sled** and a
//! bounded set of plugin methods the run may call. The broker only delivers
//! the event; the table here is the only policy, and it is plugin state
//! (schema-validated, revisioned), never broker state.

use anyhow::{ensure, Context, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::runtime_documents::RuntimeDocuments;

/// Runtime document id under the `persona` plugin.
pub const TRIGGERS_DOCUMENT: &str = "persona/triggers";
pub const MAX_TRIGGERS: usize = 64;
pub const MAX_PAYLOAD_BYTES_CEILING: usize = 256 * 1024;
pub const MAX_ACTIONS_CEILING: u32 = 16;
pub const MAX_TIMEOUT_MS_CEILING: u64 = 10 * 60 * 1000;
/// The doorbell namespace: triggers never fire on wakes.
const RESERVED_PREFIX: &str = "opdbus/";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AgentTrigger {
    #[schemars(description = "Stable trigger id (1..=64 chars, [A-Za-z0-9_-])")]
    pub trigger_id: String,
    #[schemars(description = "MQTT topic filter (`+`/`#`); never under opdbus/ or `$`")]
    pub topic_filter: String,
    #[schemars(description = "Identity sled session id of the agent the run executes as [PII]")]
    pub agent_session_id: String,
    #[schemars(
        description = "What the agent is for; composed with shared pieces into its persona"
    )]
    pub instructions: String,
    #[schemars(
        description = "`plugin.method` names the run may dispatch (allow-list; grants still apply)"
    )]
    pub allowed_methods: Vec<String>,
    #[serde(default = "default_payload_bytes")]
    #[schemars(description = "Largest event payload handed to the run (bytes)")]
    pub max_payload_bytes: u32,
    #[serde(default = "default_actions")]
    #[schemars(description = "Actions a single run may perform")]
    pub max_actions: u32,
    #[serde(default = "default_timeout_ms")]
    #[schemars(description = "Run deadline in milliseconds")]
    pub timeout_ms: u64,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_payload_bytes() -> u32 {
    16 * 1024
}
fn default_actions() -> u32 {
    4
}
fn default_timeout_ms() -> u64 {
    60_000
}
fn default_true() -> bool {
    true
}

/// The durable document: every trigger, with the revision the store assigned.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TriggerDocument {
    #[serde(default)]
    pub triggers: Vec<AgentTrigger>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListTriggersInput {}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListTriggersOutput {
    pub triggers: Vec<AgentTrigger>,
    pub revision: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetTriggerInput {
    pub trigger: AgentTrigger,
    /// Durable revision the caller read (0 = document absent). CAS.
    pub expected_revision: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetTriggerOutput {
    pub trigger: AgentTrigger,
    pub revision: u64,
    pub created: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DeleteTriggerInput {
    pub trigger_id: String,
    pub expected_revision: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DeleteTriggerOutput {
    pub deleted: bool,
    pub revision: u64,
}

pub fn validate_trigger_id(id: &str) -> Result<()> {
    ensure!(
        !id.is_empty()
            && id.len() <= 64
            && id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-'),
        "trigger_id must be 1..=64 chars of [A-Za-z0-9_-]"
    );
    Ok(())
}

/// MQTT topic-filter grammar plus this system's exclusions.
pub fn validate_topic_filter(filter: &str) -> Result<()> {
    ensure!(
        !filter.is_empty() && filter.len() <= 512 && !filter.chars().any(char::is_control),
        "topic_filter must be 1..=512 bytes without control characters"
    );
    ensure!(
        !filter.starts_with('$'),
        "topic_filter must not name `$` topics"
    );
    ensure!(
        !filter.starts_with(RESERVED_PREFIX) && filter != "#" && filter != "+",
        "topic_filter must not cover the opdbus/ doorbell namespace"
    );
    let levels: Vec<&str> = filter.split('/').collect();
    for (i, level) in levels.iter().enumerate() {
        if *level == "#" {
            ensure!(i == levels.len() - 1, "`#` must be the last level");
        } else if *level != "+" {
            ensure!(
                !level.contains('#') && !level.contains('+'),
                "wildcards must occupy a whole level"
            );
        }
    }
    // A leading `+` or a wildcard first level could still match opdbus/…
    ensure!(
        !matches!(levels[0], "+" | "#"),
        "the first level must be literal so the doorbell namespace is unreachable"
    );
    Ok(())
}

pub fn validate_trigger(trigger: &AgentTrigger) -> Result<()> {
    validate_trigger_id(&trigger.trigger_id)?;
    validate_topic_filter(&trigger.topic_filter)?;
    ensure!(
        !trigger.agent_session_id.is_empty()
            && trigger.agent_session_id.len() <= 256
            && trigger.agent_session_id.trim() == trigger.agent_session_id,
        "agent_session_id must be a bounded session id"
    );
    ensure!(
        !trigger.instructions.trim().is_empty() && trigger.instructions.len() <= 16 * 1024,
        "instructions must be 1..=16384 bytes"
    );
    ensure!(
        !trigger.allowed_methods.is_empty() && trigger.allowed_methods.len() <= 32,
        "allowed_methods must list 1..=32 plugin.method names"
    );
    for name in &trigger.allowed_methods {
        let (plugin, method) = name
            .split_once('.')
            .context("allowed_methods entries are `plugin.method`")?;
        ensure!(
            !plugin.is_empty()
                && !method.is_empty()
                && plugin
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_')
                && method
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_'),
            "allowed_methods entries are `plugin.method` identifiers"
        );
    }
    ensure!(
        (1..=MAX_PAYLOAD_BYTES_CEILING as u32).contains(&trigger.max_payload_bytes),
        "max_payload_bytes must be 1..={MAX_PAYLOAD_BYTES_CEILING}"
    );
    ensure!(
        (1..=MAX_ACTIONS_CEILING).contains(&trigger.max_actions),
        "max_actions must be 1..={MAX_ACTIONS_CEILING}"
    );
    ensure!(
        (1_000..=MAX_TIMEOUT_MS_CEILING).contains(&trigger.timeout_ms),
        "timeout_ms must be 1000..={MAX_TIMEOUT_MS_CEILING}"
    );
    Ok(())
}

pub fn validate_document(document: &TriggerDocument) -> Result<()> {
    ensure!(
        document.triggers.len() <= MAX_TRIGGERS,
        "at most {MAX_TRIGGERS} triggers"
    );
    let mut ids = std::collections::BTreeSet::new();
    for trigger in &document.triggers {
        validate_trigger(trigger)?;
        ensure!(
            ids.insert(trigger.trigger_id.as_str()),
            "duplicate trigger_id {}",
            trigger.trigger_id
        );
    }
    Ok(())
}

/// MQTT 3.1.1 topic matching (`+` one level, `#` the rest). Topics starting
/// with `$` never match wildcards at the first level.
pub fn topic_matches(filter: &str, topic: &str) -> bool {
    if topic.starts_with('$') && (filter.starts_with('+') || filter.starts_with('#')) {
        return false;
    }
    let mut f = filter.split('/');
    let mut t = topic.split('/');
    loop {
        match (f.next(), t.next()) {
            (Some("#"), _) => return true,
            (Some("+"), Some(_)) => continue,
            (Some(level), Some(actual)) if level == actual => continue,
            (None, None) => return true,
            _ => return false,
        }
    }
}

fn parse_document(value: Option<Value>) -> Result<TriggerDocument> {
    match value {
        None => Ok(TriggerDocument::default()),
        Some(value) => {
            let document: TriggerDocument =
                serde_json::from_value(value).context("durable trigger document is malformed")?;
            validate_document(&document)?;
            Ok(document)
        }
    }
}

/// Every enabled trigger, for the exhook matcher. Fails closed: a malformed
/// document yields no triggers.
pub async fn enabled_triggers(documents: &RuntimeDocuments) -> Result<Vec<AgentTrigger>> {
    let row = documents.read(TRIGGERS_DOCUMENT).await?;
    let document = parse_document(row.map(|r| r.document))?;
    Ok(document
        .triggers
        .into_iter()
        .filter(|t| t.enabled)
        .collect())
}

pub async fn list_triggers(documents: &RuntimeDocuments) -> Result<ListTriggersOutput> {
    let row = documents.read(TRIGGERS_DOCUMENT).await?;
    let revision = row.as_ref().map(|r| r.revision).unwrap_or(0);
    let document = parse_document(row.map(|r| r.document))?;
    Ok(ListTriggersOutput {
        triggers: document.triggers,
        revision,
    })
}

pub async fn set_trigger(
    documents: &RuntimeDocuments,
    input: SetTriggerInput,
) -> Result<SetTriggerOutput> {
    validate_trigger(&input.trigger)?;
    let trigger = input.trigger.clone();
    let (row, created) = documents
        .update_at_revision_with_result(TRIGGERS_DOCUMENT, input.expected_revision, move |old| {
            let mut document = parse_document(old)?;
            let created = match document
                .triggers
                .iter_mut()
                .find(|t| t.trigger_id == trigger.trigger_id)
            {
                Some(existing) => {
                    *existing = trigger.clone();
                    false
                }
                None => {
                    document.triggers.push(trigger.clone());
                    true
                }
            };
            validate_document(&document)?;
            Ok((serde_json::to_value(&document)?, created))
        })
        .await?;
    Ok(SetTriggerOutput {
        trigger: input.trigger,
        revision: row.revision,
        created,
    })
}

pub async fn delete_trigger(
    documents: &RuntimeDocuments,
    input: DeleteTriggerInput,
) -> Result<DeleteTriggerOutput> {
    validate_trigger_id(&input.trigger_id)?;
    let id = input.trigger_id.clone();
    let (row, deleted) = documents
        .update_at_revision_with_result(TRIGGERS_DOCUMENT, input.expected_revision, move |old| {
            let mut document = parse_document(old)?;
            let before = document.triggers.len();
            document.triggers.retain(|t| t.trigger_id != id);
            Ok((
                serde_json::to_value(&document)?,
                document.triggers.len() != before,
            ))
        })
        .await?;
    Ok(DeleteTriggerOutput {
        deleted,
        revision: row.revision,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use op_cozo_store::CozoGraphShuttle;

    fn trigger(id: &str, filter: &str) -> AgentTrigger {
        AgentTrigger {
            trigger_id: id.into(),
            topic_filter: filter.into(),
            agent_session_id: "agent-session".into(),
            instructions: "Watch line telemetry and raise alerts.".into(),
            allowed_methods: vec![
                "emqx.publish_wake".into(),
                "cognitive_mcp.memory_store".into(),
            ],
            max_payload_bytes: 4096,
            max_actions: 2,
            timeout_ms: 30_000,
            enabled: true,
        }
    }

    #[test]
    fn matcher_follows_mqtt_wildcards() {
        assert!(topic_matches(
            "factory/+/telemetry",
            "factory/line1/telemetry"
        ));
        assert!(!topic_matches(
            "factory/+/telemetry",
            "factory/line1/alerts"
        ));
        assert!(!topic_matches(
            "factory/+/telemetry",
            "factory/a/b/telemetry"
        ));
        assert!(topic_matches("factory/#", "factory/a/b/c"));
        assert!(topic_matches("factory/#", "factory"));
        assert!(!topic_matches("factory/#", "plant/a"));
        assert!(topic_matches("a/b", "a/b"));
        assert!(!topic_matches("a/b", "a/b/c"));
        assert!(!topic_matches("+/x", "$SYS/x"));
    }

    #[test]
    fn filters_can_never_reach_the_doorbell_or_broker_internals() {
        for bad in [
            "opdbus/#",
            "opdbus/wake/+",
            "#",
            "+",
            "+/wake/#",
            "$SYS/#",
            "a/#/b",
            "a/b+",
        ] {
            assert!(validate_topic_filter(bad).is_err(), "{bad}");
        }
        for ok in ["factory/+/telemetry", "devices/#", "a/b/c"] {
            validate_topic_filter(ok).unwrap();
        }
        let mut t = trigger("t1", "factory/#");
        validate_trigger(&t).unwrap();
        t.allowed_methods = vec!["not-a-method".into()];
        assert!(validate_trigger(&t).is_err());
        t.allowed_methods = vec![];
        assert!(validate_trigger(&t).is_err());
    }

    #[tokio::test]
    async fn triggers_are_a_revisioned_durable_document_with_cas() {
        let documents = RuntimeDocuments::new(CozoGraphShuttle::new_in_memory().unwrap());
        assert_eq!(list_triggers(&documents).await.unwrap().revision, 0);
        let created = set_trigger(
            &documents,
            SetTriggerInput {
                trigger: trigger("t1", "factory/#"),
                expected_revision: 0,
            },
        )
        .await
        .unwrap();
        assert!(created.created);
        assert_eq!(created.revision, 1);
        // Stale revision is refused before the closure runs.
        assert!(set_trigger(
            &documents,
            SetTriggerInput {
                trigger: trigger("t2", "plant/#"),
                expected_revision: 0,
            },
        )
        .await
        .is_err());
        let updated = set_trigger(
            &documents,
            SetTriggerInput {
                trigger: AgentTrigger {
                    enabled: false,
                    ..trigger("t1", "factory/#")
                },
                expected_revision: 1,
            },
        )
        .await
        .unwrap();
        assert!(!updated.created);
        assert!(enabled_triggers(&documents).await.unwrap().is_empty());
        let listed = list_triggers(&documents).await.unwrap();
        assert_eq!(listed.triggers.len(), 1);
        assert_eq!(listed.revision, 2);
        let deleted = delete_trigger(
            &documents,
            DeleteTriggerInput {
                trigger_id: "t1".into(),
                expected_revision: 2,
            },
        )
        .await
        .unwrap();
        assert!(deleted.deleted);
        assert!(list_triggers(&documents).await.unwrap().triggers.is_empty());
    }
}
