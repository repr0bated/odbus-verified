//! Door-side grants and elevation tickets (Requirements 9 and 10).
//!
//! * The durable grant document is read-only here; it has the same contract as
//!   the bridge's `grant_document.rs` (exact principal keys, closed `$`
//!   groups, no `*`, no 64-hex footprint keys).
//! * Ephemeral grants and tickets live in root-only directories. The door
//!   only consumes them; the `grant`/`ticket` CLI (run as root by a human)
//!   issues them.

use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

/// Closed grant groups (bridge `grant_document::GRANT_GROUPS`).
pub const GRANT_GROUPS: &[&str] = &[
    "$ROOT",
    "$ADMIN",
    "$SUPERADMIN",
    "$EXTERNAL-MCP",
    "$VENDOR",
    "$GUEST",
    "$AGENT-L1",
    "$AGENT-L2",
    "$AGENT-L3",
    "$AGENT-L4",
];

/// Maximum ticket lifetime in seconds (Requirement 10.3).
pub const MAX_TICKET_LIFETIME: i64 = 300;

pub fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

// ---------------------------------------------------------------- durable

type Expanded = BTreeMap<String, BTreeSet<String>>;
/// Cached document: its mtime when read, and the evaluation result.
type CachedDocument = (Option<SystemTime>, std::result::Result<Expanded, String>);

/// Validate a durable document and expand every principal's capabilities.
pub fn expand_grant_document(document: &Value) -> std::result::Result<Expanded, String> {
    let entries = document
        .as_object()
        .ok_or("grant document must be a JSON object keyed by principal_id or group")?;
    for key in entries.keys() {
        let footprint = key.len() == 64 && key.bytes().all(|b| b.is_ascii_hexdigit());
        if key == "*"
            || footprint
            || (key.starts_with('$') && !GRANT_GROUPS.contains(&key.as_str()))
        {
            return Err("prohibited wildcard, legacy footprint, or unknown group key".into());
        }
    }
    let mut own: BTreeMap<&str, (BTreeSet<String>, Vec<&str>)> = BTreeMap::new();
    for (key, entry) in entries {
        let entry = entry
            .as_object()
            .ok_or_else(|| format!("grant entry for {key} must be an object"))?;
        let capabilities = entry
            .get("capabilities")
            .and_then(Value::as_array)
            .ok_or_else(|| format!("grant entry for {key} must own a capabilities array"))?;
        let mut caps = BTreeSet::new();
        for cap in capabilities {
            let cap = cap
                .as_str()
                .filter(|c| !c.is_empty() && *c != "*")
                .ok_or_else(|| format!("grant entry for {key} has an invalid capability"))?;
            caps.insert(cap.to_owned());
        }
        let mut groups = Vec::new();
        if let Some(list) = entry.get("groups") {
            let list = list
                .as_array()
                .ok_or_else(|| format!("groups for {key} must be an array"))?;
            for group in list {
                let group = group
                    .as_str()
                    .ok_or_else(|| format!("groups for {key} must be strings"))?;
                if !GRANT_GROUPS.contains(&group) || !entries.contains_key(group) {
                    return Err(format!("grant entry for {key} names an unknown group"));
                }
                if key.starts_with('$') {
                    return Err("groups cannot nest".into());
                }
                groups.push(group);
            }
        }
        own.insert(key.as_str(), (caps, groups));
    }
    let mut expanded = Expanded::new();
    for (key, (caps, groups)) in &own {
        if key.starts_with('$') {
            continue; // group names are not caller ids
        }
        let mut all = caps.clone();
        for group in groups {
            all.extend(own[group].0.iter().cloned());
        }
        expanded.insert((*key).to_owned(), all);
    }
    Ok(expanded)
}

/// Read-only view of the materialized durable grant document, re-read when
/// its modification time changes.
pub struct DurableGrants {
    path: PathBuf,
    cache: Mutex<Option<CachedDocument>>,
}

impl DurableGrants {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            cache: Mutex::new(None),
        }
    }

    /// The principal's capabilities, or an error if the document cannot be
    /// evaluated (Requirement 8.4: the caller then allows public only).
    pub fn capabilities(&self, principal: &str) -> std::result::Result<BTreeSet<String>, String> {
        let mtime = std::fs::metadata(&self.path)
            .and_then(|m| m.modified())
            .ok();
        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        let stale = cache
            .as_ref()
            .is_none_or(|(cached, _)| *cached != mtime || mtime.is_none());
        if stale {
            let loaded = crate::catalog::read_json(&self.path)
                .map_err(|e| format!("grant document unreadable: {e:#}"))
                .and_then(|doc| expand_grant_document(&doc));
            *cache = Some((mtime, loaded));
        }
        match &cache.as_ref().expect("cached").1 {
            Ok(expanded) => Ok(expanded.get(principal).cloned().unwrap_or_default()),
            Err(error) => Err(error.clone()),
        }
    }
}

// -------------------------------------------------------------- ephemeral

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PropertyConstraint {
    #[serde(default, rename = "const", skip_serializing_if = "Option::is_none")]
    pub const_value: Option<Value>,
    #[serde(default, rename = "enum", skip_serializing_if = "Option::is_none")]
    pub one_of: Option<Vec<Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefix: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ArgsConstraint {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exact_args_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub properties: Option<BTreeMap<String, PropertyConstraint>>,
}

impl ArgsConstraint {
    pub fn matches(&self, args: &Value) -> bool {
        if let Some(hash) = &self.exact_args_sha256
            && crate::canonical::args_sha256(args) != *hash
        {
            return false;
        }
        if let Some(properties) = &self.properties {
            for (name, rule) in properties {
                let Some(value) = args.get(name) else {
                    return false;
                };
                if rule.const_value.as_ref().is_some_and(|c| c != value) {
                    return false;
                }
                if rule.one_of.as_ref().is_some_and(|set| !set.contains(value)) {
                    return false;
                }
                if let Some(prefix) = &rule.prefix {
                    match value.as_str() {
                        Some(s)
                            if s.starts_with(prefix.as_str())
                                && !s[prefix.len()..].contains("..") => {}
                        _ => return false,
                    }
                }
            }
        }
        true
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EphemeralGrant {
    pub grant_id: String,
    pub principal_id: String,
    /// `<plugin>.<method>` or `toolset:<id>`.
    pub tool: String,
    #[serde(default)]
    pub args_constraint: Option<ArgsConstraint>,
    pub not_before: i64,
    pub expires_at: i64,
    pub max_calls: u64,
    pub issued_by: String,
    #[serde(default)]
    pub approval_ticket_ref: Option<String>,
}

/// What a lookup is for: discovery ignores argument constraints.
pub struct GrantQuery<'a> {
    pub principal: &'a str,
    pub tool_key: &'a str,
    pub toolsets: &'a [String],
    pub toolset_grantable: bool,
    pub args: Option<&'a Value>,
    pub now: i64,
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// Refuse a store directory others could write or read.
fn private_dir(dir: &Path) -> Result<()> {
    let meta = std::fs::metadata(dir).with_context(|| format!("stat {}", dir.display()))?;
    ensure!(meta.is_dir(), "{} is not a directory", dir.display());
    ensure!(
        meta.mode() & 0o077 == 0,
        "{} must not be accessible to group or other",
        dir.display()
    );
    ensure!(
        meta.uid() == unsafe { libc::geteuid() },
        "{} must be owned by the door user",
        dir.display()
    );
    Ok(())
}

fn create_private_dir(dir: &Path) -> Result<()> {
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)?;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    Ok(())
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    let tmp = path.with_extension(format!("tmp-{}", uuid::Uuid::new_v4().simple()));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&tmp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

pub struct GrantStore {
    dir: PathBuf,
    lock: Mutex<()>,
}

impl GrantStore {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: dir.into(),
            lock: Mutex::new(()),
        }
    }

    /// Every well-formed record; a non-private or absent store yields none.
    pub fn list(&self) -> Vec<EphemeralGrant> {
        if private_dir(&self.dir).is_err() {
            return Vec::new();
        }
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        let mut grants: Vec<EphemeralGrant> = entries
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().ends_with(".json"))
            .filter_map(|e| crate::catalog::read_json(&e.path()).ok())
            .filter_map(|v| serde_json::from_value::<EphemeralGrant>(v).ok())
            .filter(|g| valid_id(&g.grant_id))
            .collect();
        grants.sort_by(|a, b| a.grant_id.cmp(&b.grant_id));
        grants
    }

    /// The first live grant matching the query (Requirements 9.2, 9.3, 9.5, 9.6).
    pub fn find(&self, query: &GrantQuery<'_>) -> Option<EphemeralGrant> {
        self.list().into_iter().find(|g| {
            let tool_match = g.tool == query.tool_key
                || (query.toolset_grantable
                    && g.tool
                        .strip_prefix("toolset:")
                        .is_some_and(|id| query.toolsets.iter().any(|t| t == id)));
            g.principal_id == query.principal
                && tool_match
                && g.not_before <= query.now
                && query.now < g.expires_at
                && g.max_calls > 0
                && match (query.args, &g.args_constraint) {
                    (Some(args), Some(constraint)) => constraint.matches(args),
                    _ => true,
                }
        })
    }

    /// Consume one call (Requirement 9.4). Fails if the grant is gone,
    /// expired or exhausted in the meantime.
    pub fn consume(&self, grant_id: &str, now: i64) -> Result<()> {
        ensure!(valid_id(grant_id), "invalid grant id");
        let _guard = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        private_dir(&self.dir)?;
        let path = self.dir.join(format!("{grant_id}.json"));
        let mut grant: EphemeralGrant = serde_json::from_value(crate::catalog::read_json(&path)?)?;
        ensure!(
            grant.max_calls > 0 && now < grant.expires_at,
            "grant exhausted or expired"
        );
        grant.max_calls -= 1;
        if grant.max_calls == 0 {
            std::fs::remove_file(&path)?;
        } else {
            write_atomic(&path, &serde_json::to_vec(&grant)?)?;
        }
        Ok(())
    }

    pub fn issue(&self, grant: &EphemeralGrant) -> Result<PathBuf> {
        ensure!(valid_id(&grant.grant_id), "invalid grant id");
        ensure!(grant.expires_at > grant.not_before, "grant window is empty");
        ensure!(grant.max_calls > 0, "max_calls must be positive");
        ensure!(
            grant.tool.starts_with("toolset:") || grant.tool.contains('.'),
            "tool must be <plugin>.<method> or toolset:<id>"
        );
        create_private_dir(&self.dir)?;
        private_dir(&self.dir)?;
        let path = self.dir.join(format!("{}.json", grant.grant_id));
        ensure!(!path.exists(), "grant {} already exists", grant.grant_id);
        write_atomic(&path, &serde_json::to_vec_pretty(grant)?)?;
        Ok(path)
    }

    pub fn revoke(&self, grant_id: &str) -> Result<()> {
        ensure!(valid_id(grant_id), "invalid grant id");
        std::fs::remove_file(self.dir.join(format!("{grant_id}.json"))).context("revoke grant")
    }
}

// ---------------------------------------------------------------- tickets

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ticket {
    pub ticket_id: String,
    pub principal_id: String,
    /// `<plugin>.<method>`.
    pub tool: String,
    pub args_sha256: String,
    pub issued_at: i64,
    pub expires_at: i64,
    pub issued_by: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum TicketError {
    #[error("no such ticket, or already used")]
    Missing,
    #[error("ticket bound to another principal, tool or arguments")]
    Mismatch,
    #[error("ticket expired or lifetime too long")]
    Expired,
    #[error("ticket store unavailable")]
    Store,
}

pub struct TicketStore {
    dir: PathBuf,
}

impl TicketStore {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// Validate then atomically claim a ticket (Requirements 10.1–10.3).
    pub fn consume(
        &self,
        ticket_id: &str,
        principal: &str,
        tool_key: &str,
        args_sha256: &str,
        now: i64,
    ) -> std::result::Result<Ticket, TicketError> {
        if !valid_id(ticket_id) {
            return Err(TicketError::Missing);
        }
        private_dir(&self.dir).map_err(|_| TicketError::Store)?;
        let path = self.dir.join(format!("{ticket_id}.json"));
        let ticket: Ticket = crate::catalog::read_json(&path)
            .ok()
            .and_then(|v| serde_json::from_value(v).ok())
            .ok_or(TicketError::Missing)?;
        if ticket.ticket_id != ticket_id
            || ticket.principal_id != principal
            || ticket.tool != tool_key
            || ticket.args_sha256 != args_sha256
        {
            return Err(TicketError::Mismatch);
        }
        if now >= ticket.expires_at
            || now + 5 < ticket.issued_at
            || ticket.expires_at - ticket.issued_at > MAX_TICKET_LIFETIME
        {
            return Err(TicketError::Expired);
        }
        let consumed = self.dir.join("consumed");
        create_private_dir(&consumed).map_err(|_| TicketError::Store)?;
        let claimed = consumed.join(format!(
            "{ticket_id}.{}.json",
            uuid::Uuid::new_v4().simple()
        ));
        // rename is atomic: exactly one caller wins the ticket.
        std::fs::rename(&path, &claimed).map_err(|_| TicketError::Missing)?;
        let _ = std::fs::remove_file(&claimed);
        Ok(ticket)
    }

    pub fn issue(&self, ticket: &Ticket) -> Result<PathBuf> {
        ensure!(valid_id(&ticket.ticket_id), "invalid ticket id");
        if ticket.expires_at - ticket.issued_at > MAX_TICKET_LIFETIME
            || ticket.expires_at <= ticket.issued_at
        {
            bail!("ticket lifetime must be 1..={MAX_TICKET_LIFETIME} seconds");
        }
        create_private_dir(&self.dir)?;
        private_dir(&self.dir)?;
        let path = self.dir.join(format!("{}.json", ticket.ticket_id));
        ensure!(!path.exists(), "ticket already exists");
        write_atomic(&path, &serde_json::to_vec_pretty(ticket)?)?;
        Ok(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn durable_document_expands_groups_and_rejects_bad_shapes() {
        let doc = json!({
            "$EXTERNAL-MCP": {"capabilities": ["cognitive.read"]},
            "p-1": {"groups": ["$EXTERNAL-MCP"], "capabilities": ["memory.write"]}
        });
        let expanded = expand_grant_document(&doc).unwrap();
        assert_eq!(
            expanded["p-1"],
            ["cognitive.read".to_owned(), "memory.write".to_owned()].into()
        );
        assert!(!expanded.contains_key("$EXTERNAL-MCP"));
        for bad in [
            json!({"*": {"capabilities": []}}),
            json!({"$UNKNOWN": {"capabilities": []}}),
            json!({"a".repeat(64): {"capabilities": []}}),
            json!({"p": {"groups": ["$GUEST"], "capabilities": []}}),
            json!({"p": {"capabilities": "x"}}),
            json!({"p": {"capabilities": ["*"]}}),
            json!([]),
        ] {
            assert!(expand_grant_document(&bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn durable_reader_reloads_and_reports_unreadable() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("grants.json");
        let grants = DurableGrants::new(&path);
        assert!(grants.capabilities("p").is_err());
        std::fs::write(&path, json!({"p": {"capabilities": ["a"]}}).to_string()).unwrap();
        assert_eq!(grants.capabilities("p").unwrap(), ["a".to_owned()].into());
        assert!(grants.capabilities("other").unwrap().is_empty());
        std::fs::write(&path, "{not json").unwrap();
        // Force a distinct mtime.
        let later = SystemTime::now() + std::time::Duration::from_secs(5);
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(later)
            .unwrap();
        assert!(grants.capabilities("p").is_err());
    }

    fn store() -> (tempfile::TempDir, GrantStore) {
        let dir = tempfile::tempdir().unwrap();
        let grants = dir.path().join("grants");
        (dir, GrantStore::new(grants))
    }

    fn grant(id: &str, tool: &str, max_calls: u64) -> EphemeralGrant {
        EphemeralGrant {
            grant_id: id.into(),
            principal_id: "p".into(),
            tool: tool.into(),
            args_constraint: None,
            not_before: 100,
            expires_at: 200,
            max_calls,
            issued_by: "test".into(),
            approval_ticket_ref: None,
        }
    }

    fn query<'a>(
        tool: &'a str,
        sets: &'a [String],
        grantable: bool,
        args: Option<&'a Value>,
        now: i64,
    ) -> GrantQuery<'a> {
        GrantQuery {
            principal: "p",
            tool_key: tool,
            toolsets: sets,
            toolset_grantable: grantable,
            args,
            now,
        }
    }

    #[test]
    fn window_count_and_identity_scope() {
        let (_d, store) = store();
        store
            .issue(&grant("g1", "cognitive.memory_recall", 2))
            .unwrap();
        let none: Vec<String> = Vec::new();
        assert!(
            store
                .find(&query("cognitive.memory_recall", &none, true, None, 99))
                .is_none()
        );
        assert!(
            store
                .find(&query("cognitive.memory_recall", &none, true, None, 200))
                .is_none()
        );
        assert!(
            store
                .find(&query("cognitive.memory_recall", &none, true, None, 150))
                .is_some()
        );
        let mut other = query("cognitive.memory_recall", &none, true, None, 150);
        other.principal = "q";
        assert!(store.find(&other).is_none());
        store.consume("g1", 150).unwrap();
        store.consume("g1", 150).unwrap();
        assert!(
            store
                .find(&query("cognitive.memory_recall", &none, true, None, 150))
                .is_none()
        );
        assert!(store.consume("g1", 150).is_err());
    }

    #[test]
    fn toolset_grants_only_cover_grantable_classes() {
        let (_d, store) = store();
        store.issue(&grant("g2", "toolset:reasoning", 5)).unwrap();
        let sets = vec!["reasoning".to_owned()];
        assert!(
            store
                .find(&query("cognitive.specialist", &sets, true, None, 150))
                .is_some()
        );
        assert!(
            store
                .find(&query("cognitive.workflow_run", &sets, false, None, 150))
                .is_none()
        );
    }

    #[test]
    fn argument_constraints() {
        let (_d, store) = store();
        let mut g = grant("g3", "cognitive.agent_bash_pro_exec", 1);
        let mut props = BTreeMap::new();
        props.insert(
            "cwd".to_owned(),
            PropertyConstraint {
                prefix: Some("/srv/".into()),
                ..Default::default()
            },
        );
        props.insert(
            "mode".to_owned(),
            PropertyConstraint {
                one_of: Some(vec![json!("a"), json!("b")]),
                ..Default::default()
            },
        );
        g.args_constraint = Some(ArgsConstraint {
            exact_args_sha256: None,
            properties: Some(props),
        });
        store.issue(&g).unwrap();
        let none: Vec<String> = Vec::new();
        let ok = json!({"cwd":"/srv/x","mode":"a"});
        let escape = json!({"cwd":"/srv/../etc","mode":"a"});
        let wrong = json!({"cwd":"/etc","mode":"a"});
        let missing = json!({"mode":"a"});
        let key = "cognitive.agent_bash_pro_exec";
        assert!(
            store
                .find(&query(key, &none, false, Some(&ok), 150))
                .is_some()
        );
        for bad in [&escape, &wrong, &missing] {
            assert!(
                store
                    .find(&query(key, &none, false, Some(bad), 150))
                    .is_none(),
                "{bad}"
            );
        }
        // Discovery ignores argument constraints.
        assert!(store.find(&query(key, &none, false, None, 150)).is_some());
        let exact = ArgsConstraint {
            exact_args_sha256: Some(crate::canonical::args_sha256(&ok)),
            properties: None,
        };
        assert!(exact.matches(&json!({"mode":"a","cwd":"/srv/x"})));
        assert!(!exact.matches(&wrong));
    }

    #[test]
    fn non_private_store_yields_nothing() {
        let (_d, store) = store();
        store.issue(&grant("g4", "a.b", 1)).unwrap();
        std::fs::set_permissions(&store.dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(store.list().is_empty());
        assert!(store.consume("g4", 150).is_err());
    }

    fn ticket(id: &str, issued: i64, expires: i64) -> Ticket {
        Ticket {
            ticket_id: id.into(),
            principal_id: "p".into(),
            tool: "cognitive.agent_bash_pro_exec".into(),
            args_sha256: "h".into(),
            issued_at: issued,
            expires_at: expires,
            issued_by: "test".into(),
        }
    }

    #[test]
    fn tickets_are_single_use_and_bound() {
        let dir = tempfile::tempdir().unwrap();
        let tickets = TicketStore::new(dir.path().join("tickets"));
        assert!(tickets.issue(&ticket("too-long", 0, 301)).is_err());
        tickets.issue(&ticket("t1", 100, 200)).unwrap();
        let key = "cognitive.agent_bash_pro_exec";
        assert_eq!(
            tickets.consume("t1", "q", key, "h", 150),
            Err(TicketError::Mismatch)
        );
        assert_eq!(
            tickets.consume("t1", "p", "x.y", "h", 150),
            Err(TicketError::Mismatch)
        );
        assert_eq!(
            tickets.consume("t1", "p", key, "other", 150),
            Err(TicketError::Mismatch)
        );
        assert_eq!(
            tickets.consume("t1", "p", key, "h", 200),
            Err(TicketError::Expired)
        );
        assert!(tickets.consume("t1", "p", key, "h", 150).is_ok());
        assert_eq!(
            tickets.consume("t1", "p", key, "h", 150),
            Err(TicketError::Missing)
        );
        assert_eq!(
            tickets.consume("../x", "p", key, "h", 150),
            Err(TicketError::Missing)
        );
        // A hand-written over-long ticket is refused at use.
        std::fs::write(
            dir.path().join("tickets/t2.json"),
            serde_json::to_vec(&ticket("t2", 0, 1000)).unwrap(),
        )
        .unwrap();
        assert_eq!(
            tickets.consume("t2", "p", key, "h", 10),
            Err(TicketError::Expired)
        );
    }
}
