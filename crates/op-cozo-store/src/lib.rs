use anyhow::Result;
use cozo::DbInstance;
pub use cozo::{DataValue, NamedRows, Num, ScriptMutability, Vector};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{info, warn};

type Params = BTreeMap<String, DataValue>;

mod external_mcp_sessions;
pub use external_mcp_sessions::ExternalMcpSessionRecord;

mod system_graph;
pub use system_graph::*;

mod identity_inventory;
mod identity_transactions;
pub use identity_inventory::{
    IdentityInventoryCounts, IdentityInventoryEntry, IdentityInventoryPage, IdentityInventoryQuery,
};
pub use identity_transactions::{
    CleanProvisionReceipt, IdentityCleanProvision, clean_provision_instance_binding_matches,
    IdentityCommit, IdentityCompareExchange, IdentityIntent, IdentityPhysicalRootMigration,
    IdentityRecoverySnapshot, IdentityRevision, PhysicalRootMigrationReceipt, RootRepairJournal,
    SeedProvisionJournal, physical_root_instance_binding_matches,
};
mod runtime_document_transactions;
pub use runtime_document_transactions::{
    RuntimeDocumentCommit, RuntimeDocumentFailure, RuntimeDocumentRecord,
    RuntimeDocumentUncertainPhase,
};
mod model_registry;
pub use model_registry::{
    RegistryModel, RegistryModelRecord, RegistryProviderRecord, RegistryRefreshCounts,
};
mod ui_gallery;
pub use ui_gallery::{
    MAX_UI_GALLERY_DOCUMENT_BYTES, UiCandidatePage, UiCandidateRecord, UiCatalogPage,
    UiCatalogRecord,
};
mod chat_response_conversations;
pub use chat_response_conversations::{
    ChatConversationRecord, ChatResponseSnapshot, ChatTurnDraft, ChatTurnPage, ChatTurnRecord,
    MAX_CHAT_CONVERSATION_BYTES, MAX_CHAT_TURN_BYTES, MAX_CHAT_TURN_WINDOW_BYTES,
};
mod ctl_plane_chatbot_episodes;
mod workflow_executions;
pub use workflow_executions::WorkflowExecutionRecord;
pub use ctl_plane_chatbot_episodes::{
    EpisodeEmbeddingRecord, EpisodePage, EpisodeScope, EpisodeSnapshot, KNOWLEDGE_SNAPSHOT_LIMIT,
    MAX_CHATBOT_EPISODE_BYTES, MAX_CHATBOT_SNAPSHOT_BYTES, ScopedEpisodeRecord,
    ScopedKnowledgeRecord,
};

/// Strongly-typed error for CozoDB graph operations.
///
/// Keeps `anyhow` out of the public API surface while preserving
/// ergonomic `?` propagation inside the crate.
#[derive(Debug, thiserror::Error)]
pub enum CozoError {
    #[error("{0}")]
    RuntimeDocument(#[from] RuntimeDocumentFailure),

    #[error("CozoDB error: {0}")]
    Database(String),

    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("{0}")]
    Other(String),
}

impl From<anyhow::Error> for CozoError {
    fn from(e: anyhow::Error) -> Self {
        Self::Other(e.to_string())
    }
}

/// Run a CozoDB script, converting the non-std Error to anyhow.
fn cozo_run(db: &DbInstance, script: &str, params: Params) -> Result<NamedRows> {
    db.run_script(script, params, ScriptMutability::Mutable)
        .map_err(|e| anyhow::anyhow!("{e}"))
}

/// Verdict returned by the compliance graph evaluation.
#[derive(Debug, Clone)]
pub struct PolicyVerdict {
    pub allow: bool,
    pub reason: String,
}

/// Full WireGuard gateway session record (mirrors op-gateway's `WireGuardSession`),
/// persisted so a gateway restart doesn't drop live sessions. `flags_json` is the
/// caller's flags map, pre-serialized to a JSON string.
#[derive(Debug, Clone)]
pub struct WgSessionRecord {
    pub session_id: String,
    pub peer_pubkey: String,
    pub psk: String,
    pub created_at: u64,
    pub expires_at: u64,
    pub is_active: bool,
    pub last_used: u64,
    pub client_ip: Option<String>,
    pub client_version: Option<String>,
    pub auth_method: String,
    pub key_rotation_count: u32,
    pub flags_json: String,
}

/// One container identity sled row (mirrors op-plugins' `ContainerIdentitySled`).
/// `btrfs_device_json` is the sled's btrfs device record pre-serialized to JSON;
/// `peer_ip`, `sealed_id`, and `btrfs_device_json` use `""` for absent.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct IdentitySledRecord {
    pub session_id: String,
    pub wireguard_pubkey: String,
    pub interface: String,
    pub peer_ip: String,
    pub mutation_index: i64,
    pub session_genesis: String,
    pub trace_id: String,
    pub schema_version: i64,
    pub vector_id: String,
    pub sealed_id: String,
    /// Actor class is persisted with the atomic identity revision metadata.
    /// It is distinct from SID1's transport principal_kind.
    pub principal_kind: Option<String>,
    pub btrfs_device_json: String,
    pub instance_json: String,
    pub session_started_at: i64,
    pub last_seen_at: i64,
    pub active: bool,
    /// 0 = permanent (no expiry); otherwise unix seconds this identity
    /// stops being valid (temporary/consumer identities like Lovable).
    pub expires_at: i64,
}

/// The genesis inputs stored beside a session's sled row (version 3 records).
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct GenesisInputsRecord {
    pub session_id: String,
    pub arrival_timestamp: i64,
    pub chain_head_at_arrival: String,
    pub catalog_hash_at_arrival: String,
    pub head_timestamp_at_arrival: i64,
    /// `SCHEMA_CONTENT_HASH` of the record shape this row was written against.
    pub schema_content_hash: String,
}

/// One row of a session's append-only "snowball" event ledger.
#[derive(Debug, Clone)]
pub struct SessionEventRecord {
    pub session_id: String,
    pub seq: i64,
    pub kind: String,
    pub subid: String,
    pub content: String,
    pub created_at: i64,
}

/// One registered human principal row (mirrors op-plugins' `HumanPrincipal`).
///
/// Humans are not containers: `principal_id` is derived from the WireGuard
/// pubkey via `op_identity::session::derive_principal_id` (never
/// caller-supplied), so it can never collide with a container session id.
/// `display_alias` is display-only ("" = none) and never authoritative.
/// `revoked_at` = 0 means active; revocation is a permanent tombstone — the
/// row (and its pubkey mapping) is never deleted, so a revoked key can never
/// be re-registered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HumanPrincipalRecord {
    pub principal_id: String,
    pub human_pubkey: String,
    pub display_alias: String,
    pub registered_at: i64,
    pub revoked_at: i64,
}

/// Default on-disk location of the human_principal plugin's Cozo database.
pub const DEFAULT_HUMAN_PRINCIPAL_COZO_DB_PATH: &str = "/var/lib/op-dbus/human-principal-cozo";

/// Resolve the human_principal Cozo DB path: the
/// `OP_HUMAN_PRINCIPAL_COZO_DB_PATH` override when set, else the default.
pub fn human_principal_cozo_db_path() -> PathBuf {
    std::env::var("OP_HUMAN_PRINCIPAL_COZO_DB_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(DEFAULT_HUMAN_PRINCIPAL_COZO_DB_PATH))
}

/// CozoDB graph database shuttle.
///
/// Manages the unified Datalog relations:
///   - `compliance_rule`     — NIST/EU-AI-Act deny rules evaluated before mutations
///   - `subid_registry`      — canonical OSCAL subid taxonomy entries
///   - `graph_node`          — arbitrary identity-graph nodes
///   - `graph_edge`          — directed relationships between nodes
///   - `audit_event`         — append-only compliance audit log
///   - `users`               — wg_pubkey-keyed user identities (no PII)
///   - `sessions`            — session_id → wg_pubkey, with expiry
///   - `memory_namespaces`   — named MCP memory contexts
///   - `memory_entries`      — key/value entries within a namespace
#[derive(Clone)]
pub struct CozoGraphShuttle {
    pub(crate) db: Arc<DbInstance>,
    identity_directory: Option<Arc<identity_transactions::PersistentIdentityBoundary>>,
    identity_transaction: Arc<std::sync::Mutex<()>>,
    runtime_document_transaction: Arc<std::sync::Mutex<()>>,
    episode_transaction: Arc<std::sync::Mutex<()>>,
    system_graph_transaction: Arc<std::sync::Mutex<()>>,
}

impl CozoGraphShuttle {
    /// Fail closed when domain owners share one database handle or when two
    /// persistent handles resolve to the same held root or data directory.
    pub fn ensure_distinct_from(&self, other: &Self) -> std::result::Result<(), CozoError> {
        if Arc::ptr_eq(&self.db, &other.db) {
            return Err(CozoError::Other(
                "Cozo domain owners must not share a database handle".into(),
            ));
        }
        match (&self.identity_directory, &other.identity_directory) {
            (Some(left), Some(right)) => left.ensure_distinct_from(right),
            _ => Ok(()),
        }
    }

    pub fn new_in_memory() -> std::result::Result<Self, CozoError> {
        let db = DbInstance::new("mem", "", Default::default()).map_err(|e| {
            CozoError::Other(format!("failed to create CozoDB in-memory instance: {e}"))
        })?;
        let s = Self {
            db: Arc::new(db),
            identity_directory: None,
            identity_transaction: Arc::new(std::sync::Mutex::new(())),
            runtime_document_transaction: Arc::new(std::sync::Mutex::new(())),
            episode_transaction: Arc::new(std::sync::Mutex::new(())),
            system_graph_transaction: Arc::new(std::sync::Mutex::new(())),
        };
        s.seed_schema().map_err(CozoError::from)?;
        Ok(s)
    }

    pub fn new_persistent(path: PathBuf) -> std::result::Result<Self, CozoError> {
        Self::open_persistent(path, false)
    }

    /// Open a credential-bearing store beneath service-only root/data
    /// directories. Restricts their held, nofollow-validated descriptors to
    ///0700 before Cozo can create/read its files; never changes process umask
    /// or generic store permissions. Custom options that could redirect data
    /// outside this private boundary are rejected before opening Cozo.
    pub fn new_persistent_private(path: PathBuf) -> std::result::Result<Self, CozoError> {
        Self::open_persistent(path, true)
    }

    fn open_persistent(path: PathBuf, private: bool) -> std::result::Result<Self, CozoError> {
        let ps = path
            .to_str()
            .ok_or_else(|| CozoError::Other("Cozo database path must be valid UTF-8".into()))?
            .to_string();
        let directory = if private {
            identity_transactions::PersistentIdentityBoundary::open_private(&path)?
        } else {
            identity_transactions::PersistentIdentityBoundary::open(&path)?
        };
        // RocksDB is the durable storage engine beneath CozoDB.
        let db = DbInstance::new("rocksdb", &ps, Default::default())
            .map_err(|e| CozoError::Other(format!("failed to open CozoDB at {ps}: {e}")))?;
        directory.verify()?;
        let s = Self {
            db: Arc::new(db),
            identity_directory: Some(Arc::new(directory)),
            identity_transaction: Arc::new(std::sync::Mutex::new(())),
            runtime_document_transaction: Arc::new(std::sync::Mutex::new(())),
            episode_transaction: Arc::new(std::sync::Mutex::new(())),
            system_graph_transaction: Arc::new(std::sync::Mutex::new(())),
        };
        s.seed_schema().map_err(CozoError::from)?;
        Ok(s)
    }

    pub fn from_env() -> std::result::Result<Self, CozoError> {
        if let Ok(p) = std::env::var("COGNITIVE_MCP_COZO_DB_PATH") {
            Self::new_persistent(PathBuf::from(p))
        } else {
            Self::new_in_memory()
        }
    }

    fn seed_schema(&self) -> Result<()> {
        let relations = [
            // Migration markers survive empty catalogs; row count is not a
            // migration version and must never resurrect retired identities.
            r#":create schema_migrations { name: String => completed: Bool }"#,
            // Private project catalog. Document stores reside in each project's
            // own persistent mount, not in this control-plane catalog.
            r#":create project_catalog { project_id: String => document: String }"#,
            r#":create project_documents { collection: String, document_id: String => document: String }"#,
            r#":create notebooklm_selection { session_id: String => notebook_id: String }"#,
            // The chat mode a principal selected in the UI (OD-65): mode,
            // project and personal sub-namespace. Memory tools and recall
            // bind to it; the principal is always the scope.
            r#":create chat_context_selection { session_id: String => document: String }"#,
            // Durable plugin-owned runtime values. These documents contain no
            // method schemas and never participate in plugin discovery.
            r#":create plugin_runtime_documents { plugin_id: String => revision: Int, document: String }"#,
            // plugin × op → action(Deny/Allow) + reason + control_ref
            r#":create compliance_rule {
                plugin: String, op: String, action: String
                =>
                reason: String default "",
                control_ref: String default "",
                created_at: String default ""
            }"#,
            // canonical OSCAL subid entries
            r#":create subid_registry {
                subid: String
                =>
                category: String default "",
                component_type: String default "",
                subject: String default "",
                verb: String default "",
                facet: String default "",
                version: Int default 1,
                control_source: String default "",
                control_refs: String default "",
                statement_refs: String default "",
                registered_at: String default ""
            }"#,
            // identity-graph nodes
            r#":create graph_node {
                id: String
                =>
                label: String default "",
                props: String default "{}"
            }"#,
            // directed relationships
            r#":create graph_edge {
                src: String, rel: String, dst: String
                =>
                props: String default "{}"
            }"#,
            // append-only audit log
            r#":create audit_event {
                event_id: String
                =>
                subid: String default "",
                plugin_id: String default "",
                operation: String default "",
                actor: String default "",
                verdict: String default "allow",
                reason: String default "",
                control_ref: String default "",
                timestamp: String default ""
            }"#,
            // wg_pubkey-keyed user identities; no email or PII persisted
            r#":create users {
                wg_pubkey: String
                =>
                created_at: String default ""
            }"#,
            // full privacy router user accounts (PII stored per explicit directive)
            r#":create privacy_users {
                id: String
                =>
                email: String default "",
                email_verified: String default "false",
                wg_public_key: String default "",
                wg_private_key_encrypted: String default "",
                assigned_ip: String default "",
                privacy_quota_bytes: Int default 1073741824,
                privacy_quota_used_bytes: Int default 0,
                privacy_container_name: String default "",
                privacy_route_id: String default "",
                privacy_network_connected: String default "false",
                privacy_network_connected_at: String default "",
                google_id: String default "",
                google_email: String default "",
                api_credentials_json: String default "null",
                created_at: String default ""
            }"#,
            // session_id → wg_pubkey with optional expiry (RFC3339)
            r#":create sessions {
                session_id: String
                =>
                wg_pubkey: String default "",
                created_at: String default "",
                expires_at: String default ""
            }"#,
            // consumer account proof — wg_pubkey keyed, no email/PII
            r#":create account_sessions {
                wg_pubkey: String
                =>
                session_id: String default "",
                session_proof: String default "",
                created_at: String default ""
            }"#,
            // GhostBridge consumer operational state — session_id keyed, no email/PII
            r#":create consumer_accounts {
                session_id: String
                =>
                wg_public_key: String default "",
                wg_private_key_encrypted: String default "",
                assigned_ip: String default "",
                email_verified: String default "true",
                privacy_quota_bytes: Int default 1073741824,
                privacy_quota_used_bytes: Int default 0,
                privacy_container_name: String default "",
                privacy_route_id: String default "",
                privacy_network_connected: String default "false",
                privacy_network_connected_at: String default "",
                api_credentials_json: String default "null",
                created_at: String default ""
            }"#,
            // full WireGuard gateway session record (op-gateway::WireGuardSession),
            // persisted so restarts don't drop live sessions
            r#":create wg_sessions {
                session_id: String
                =>
                peer_pubkey: String default "",
                psk: String default "",
                created_at: Int default 0,
                expires_at: Int default 0,
                is_active: Bool default true,
                last_used: Int default 0,
                client_ip: String default "",
                client_version: String default "",
                auth_method: String default "",
                key_rotation_count: Int default 0,
                flags_json: String default "{}"
            }"#,
            // peer_pubkey → session_id, mirrors op-gateway's in-memory peer_sessions map
            r#":create wg_peer_sessions {
                peer_pubkey: String
                =>
                session_id: String default ""
            }"#,
            // named MCP memory contexts (replaces SQLite memory_namespaces)
            r#":create memory_namespaces {
                name: String
                =>
                id: String default "",
                kind: String default "custom",
                description: String default "",
                linked_task_id: String default "",
                linked_cron: String default "",
                metadata: String default "{}",
                created_at: String default "",
                updated_at: String default ""
            }"#,
            // key/value entries within a namespace (replaces SQLite memory_entries)
            r#":create memory_entries {
                namespace: String, key: String
                =>
                id: String default "",
                value: String default "null",
                tags: String default "[]",
                created_at: String default "",
                updated_at: String default "",
                expires_at: String default "",
                access_count: Int default 0,
                last_accessed: String default ""
            }"#,
            // persistent agent identity / personality (Soul Memory)
            r#":create soul_memories {
                agent_id: String
                =>
                identity: String default "",
                personality: String default "",
                traits: String default "{}",
                version: Int default 1,
                created_at: String default "",
                updated_at: String default ""
            }"#,
            // 1:1 binding from agent → owning memory namespace
            r#":create agent_namespace_bindings {
                agent_id: String
                =>
                namespace: String default "",
                created_at: String default "",
                updated_at: String default ""
            }"#,
            // one row per container identity sled (the container IS the sled IS
            // the identity; host = container zero). btrfs_device_json is the
            // sled's Cozo-registered btrfs persistence device, pre-serialized
            // ("" = none); peer_ip/sealed_id use "" for absent.
            r#":create identity_sessions {
                session_id: String
                =>
                wireguard_pubkey: String,
                interface: String default "",
                peer_ip: String default "",
                mutation_index: Int default 0,
                session_genesis: String default "",
                trace_id: String default "",
                schema_version: Int default 0,
                vector_id: String default "",
                sealed_id: String default "",
                btrfs_device_json: String default "",
                instance_json: String default "",
                session_started_at: Int default 0,
                last_seen_at: Int default 0,
                active: Bool default false,
                expires_at: Int default 0
            }"#,
            r#":create identity_genesis {
                session_id: String
                =>
                arrival_timestamp: Int default 0,
                chain_head_at_arrival: String default "",
                catalog_hash_at_arrival: String default "",
                head_timestamp_at_arrival: Int default 0,
                schema_content_hash: String default ""
            }"#,
            // Atomic revision/actor metadata, joined with identity_sessions.
            // Actor class is stored once here: adding it does not rewrite the
            // existing16-column relation or its uninspectable default expressions.
            // Tombstones prevent stale expected-row/legacy resurrection.
            r#":create identity_revisions {
                session_id: String => revision: Int,
                operation_id: String, intent_kind: String, intent_value: Int,
                principal_kind: String default ""
            }"#,
            // Private one-row-per-session repair fence in this same Cozo DB.
            // Ordinary session events cannot create, overwrite or truncate it.
            r#":create identity_root_repairs {
                session_id: String => version: Int,
                plan_sha256: String, operation_id: String, phase: String,
                expected_revision: Int, expected_revision_operation_id: String,
                committed_revision: Int default 0, stub_receipt_json: String default "", updated_at: Int
            }"#,
            // A separately fenced, absent-record native provision. The
            // session key is derived from the public key; only this owner
            // writes the relation before creating a leaf or Incus instance.
            r#":create identity_seed_provisions {
                session_id: String => version: Int,
                plan_sha256: String, operation_id: String, phase: String,
                expected_revision: Int, expected_revision_operation_id: String,
                committed_revision: Int default 0, committed_genesis: String default "", updated_at: Int
            }"#,
            // append-only per-session "snowball" event ledger archive
            r#":create identity_physical_root_migrations {
                session_id: String => plan_sha256: String, operation_id: String,
                expected_revision: Int, committed_revision: Int,
                btrfs_device_json: String, instance_json: String, provenance_json: String
            }"#,
            r#":create identity_physical_root_history {
                session_id: String, committed_revision: Int => plan_sha256: String,
                operation_id: String, old_btrfs_device_json: String,
                old_instance_json: String, old_journals_json: String,
                old_revision_operation_id: String
            }"#,
            r#":create identity_clean_provisions {
                session_id: String, committed_revision: Int => plan_sha256: String,
                operation_id: String, expected_revision: Int, provenance_json: String,
                btrfs_device_json: String, instance_json: String,
                old_revision_json: String, old_journals_json: String
            }"#,
            // (the immutable event chain is the proof; this is the queryable copy)
            r#":create session_events {
                session_id: String,
                seq: Int
                =>
                kind: String,
                subid: String default "",
                content: String default "",
                created_at: Int default 0
            }"#,
            // one row per registered human principal (humans ≠ containers;
            // principal_id is derived, never caller-supplied). revoked_at = 0
            // means active; revocation is a permanent tombstone — the row is
            // never deleted, so a revoked key can never be re-registered.
            r#":create human_principals {
                principal_id: String
                =>
                human_pubkey: String,
                display_alias: String default "",
                registered_at: Int default 0,
                revoked_at: Int default 0
            }"#,
            r#":create chain_vector_link {
                point_id: String,
                kind: String
                =>
                value: String
            }"#,
            r#":create external_mcp_sessions { session_id: String => principal_id: String, document: String }"#,
            // human_pubkey → principal_id lookup; the pubkey is unique across
            // ALL principals, active or revoked (tombstones keep their mapping)
            r#":create human_principal_pubkeys {
                human_pubkey: String
                =>
                principal_id: String default ""
            }"#,
        ];

        for script in &relations {
            if let Err(e) = cozo_run(&self.db, script, BTreeMap::new()) {
                let msg = e.to_string();
                if !msg.contains("already exists")
                    && !msg.contains("AlreadyExists")
                    && !msg.contains("conflicts with an existing")
                {
                    eprintln!("COZO_SCHEMA_ERR: {}", msg);
                    warn!(error = %msg, "CozoDB schema init warning");
                }
            }
        }

        self.initialize_system_graph_schema()?;

        // One-way copy from the pre-v3 `identity_sleds` relation. This must not
        // `:put` over `identity_sessions` that already exist — every process
        // open used to replay stale host-only rows and wipe bound instance/btrfs
        // JSON written while the bridge was down.
        let migration_done = !cozo_run(&self.db,
            "?[name] := *schema_migrations{name, completed}, name = 'identity-sessions-v3', completed = true",
            BTreeMap::new())?.rows.is_empty();
        let sessions_already_present = !self.list_identity_sessions()?.is_empty();
        let relations = cozo_run(&self.db, "::relations", BTreeMap::new())?;
        let legacy_exists = relations
            .rows
            .iter()
            .any(|row| row.first().and_then(dv_as_str) == Some("identity_sleds"));
        if !migration_done && !sessions_already_present && legacy_exists {
            // A relation may have a different historical width, even when it
            // contains no rows. Avoid planning a projection it cannot satisfy.
            let has_legacy_rows = !cozo_run(
                &self.db,
                "?[session_id] := *identity_sleds{session_id} :limit 1",
                BTreeMap::new(),
            )?
            .rows
            .is_empty();
            if has_legacy_rows {
                let columns = cozo_run(&self.db, "::columns identity_sleds", BTreeMap::new())?;
                let names: std::collections::BTreeSet<&str> = columns
                    .rows
                    .iter()
                    .filter_map(|row| row.first().and_then(dv_as_str))
                    .collect();
                // A populated pre-SealedID relation cannot supply an authored
                // sealed credential. In particular blob_ref is not sealed_id.
                let required = [
                    "session_id",
                    "wireguard_pubkey",
                    "interface",
                    "peer_ip",
                    "mutation_index",
                    "hashed_footprint",
                    "trace_id",
                    "schema_version",
                    "vector_id",
                    "sealed_id",
                    "btrfs_device_json",
                    "instance_json",
                    "session_started_at",
                    "last_seen_at",
                    "active",
                    "expires_at",
                ];
                for name in required {
                    anyhow::ensure!(
                        names.contains(name),
                        "populated legacy identity_sleds missing required field {name}; retired identity requires an authored credential"
                    );
                }
                let legacy_fields = required.join(", ");
                // Named fields tolerate unrelated added columns without inventing
                // credential or expiry data. Existing v3 stores are never replayed.
                let query = format!(
                    r#"
                ?[session_id, wireguard_pubkey, interface, peer_ip, mutation_index,
                  session_genesis, trace_id, schema_version, vector_id, sealed_id,
                  btrfs_device_json, instance_json, session_started_at, last_seen_at,
                  active, expires_at] :=
                    *identity_sleds{{{legacy_fields}}},
                    session_genesis = hashed_footprint
                :put identity_sessions {{
                    session_id => wireguard_pubkey, interface, peer_ip, mutation_index,
                    session_genesis, trace_id, schema_version, vector_id, sealed_id,
                    btrfs_device_json, instance_json, session_started_at, last_seen_at,
                    active, expires_at
                }}
            "#
                );
                cozo_run(&self.db, &query, BTreeMap::new())?;
            }
        }
        if !migration_done {
            cozo_run(
                &self.db,
                "?[name, completed] <- [['identity-sessions-v3', true]] :put schema_migrations {name => completed}",
                BTreeMap::new(),
            )?;
        }

        info!("CozoDB schema ready");
        Ok(())
    }

    // ── Raw query ──────────────────────────────────────────────────────────────

    pub fn run_query(
        &self,
        query: &str,
        params: Option<Value>,
    ) -> std::result::Result<Value, CozoError> {
        let p = params.map(json_obj_to_params).unwrap_or_default();
        let rows = cozo_run(&self.db, query, p)
            .map_err(|e| CozoError::Other(format!("CozoDB query failed: {e}")))?;
        Ok(named_rows_to_json(rows))
    }

    // ── Compliance ─────────────────────────────────────────────────────────────

    /// Evaluate (plugin_id, operation) against all Deny rules in the compliance graph.
    pub fn evaluate_mutation(&self, plugin_id: &str, operation: &str) -> PolicyVerdict {
        // Bind columns under aliases — `op` is awkward as a bare Datalog var.
        let query = r#"
            deny_rule[r] :=
                *compliance_rule{plugin: p, op: o, action: a, reason: r},
                a = 'Deny',
                p = $plugin,
                o = $operation
            deny_rule[r] :=
                *compliance_rule{plugin: p, op: o, action: a, reason: r},
                a = 'Deny',
                p = '*',
                o = $operation
            deny_rule[r] :=
                *compliance_rule{plugin: p, op: o, action: a, reason: r},
                a = 'Deny',
                p = $plugin,
                o = '*'
            deny_rule[r] :=
                *compliance_rule{plugin: p, op: o, action: a, reason: r},
                a = 'Deny',
                p = '*',
                o = '*'
            ?[r] := deny_rule[r]
        "#;
        let mut p: Params = BTreeMap::new();
        p.insert("plugin".into(), DataValue::Str(plugin_id.into()));
        p.insert("operation".into(), DataValue::Str(operation.into()));

        match cozo_run(&self.db, query, p) {
            Ok(rows) if rows.rows.is_empty() => PolicyVerdict {
                allow: true,
                reason: "no deny rule matched".into(),
            },
            Ok(rows) => {
                let reason = rows.rows[0]
                    .first()
                    .and_then(dv_as_str)
                    .unwrap_or("compliance rule violated")
                    .to_string();
                PolicyVerdict {
                    allow: false,
                    reason,
                }
            }
            Err(error) => PolicyVerdict {
                allow: true,
                reason: format!("compliance query failed: {error}"),
            },
        }
    }

    pub fn store_compliance_rule(
        &self,
        plugin: &str,
        op: &str,
        action: &str,
        reason: &str,
        control_ref: &str,
    ) -> std::result::Result<(), CozoError> {
        let query = r#"
            ?[plugin, op, action, reason, control_ref, created_at]
                <- [[$plugin, $op, $action, $reason, $control_ref, $ts]]
            :put compliance_rule { plugin, op, action => reason, control_ref, created_at }
        "#;
        let mut p: Params = BTreeMap::new();
        p.insert("plugin".into(), DataValue::Str(plugin.into()));
        p.insert("op".into(), DataValue::Str(op.into()));
        p.insert("action".into(), DataValue::Str(action.into()));
        p.insert("reason".into(), DataValue::Str(reason.into()));
        p.insert("control_ref".into(), DataValue::Str(control_ref.into()));
        p.insert("ts".into(), DataValue::Str(now_rfc3339().into()));
        cozo_run(&self.db, query, p)
            .map_err(|e| CozoError::Other(format!("store compliance rule: {e}")))?;
        Ok(())
    }

    // ── Subid registry ─────────────────────────────────────────────────────────

    #[allow(clippy::too_many_arguments)]
    pub fn register_subid(
        &self,
        subid: &str,
        category: &str,
        component_type: &str,
        subject: &str,
        verb: &str,
        facet: &str,
        version: u8,
        control_source: &str,
        control_refs: &str,
        statement_refs: &str,
    ) -> std::result::Result<(), CozoError> {
        let query = r#"
            ?[subid, category, component_type, subject, verb, facet, version,
              control_source, control_refs, statement_refs, registered_at]
                <- [[$sid, $cat, $ctype, $subj, $verb, $facet, $ver,
                     $csrc, $crefs, $srefs, $ts]]
            :put subid_registry {
                subid => category, component_type, subject, verb, facet, version,
                         control_source, control_refs, statement_refs, registered_at
            }
        "#;
        let mut p: Params = BTreeMap::new();
        p.insert("sid".into(), DataValue::Str(subid.into()));
        p.insert("cat".into(), DataValue::Str(category.into()));
        p.insert("ctype".into(), DataValue::Str(component_type.into()));
        p.insert("subj".into(), DataValue::Str(subject.into()));
        p.insert("verb".into(), DataValue::Str(verb.into()));
        p.insert("facet".into(), DataValue::Str(facet.into()));
        p.insert("ver".into(), dv_int(version as i64));
        p.insert("csrc".into(), DataValue::Str(control_source.into()));
        p.insert("crefs".into(), DataValue::Str(control_refs.into()));
        p.insert("srefs".into(), DataValue::Str(statement_refs.into()));
        p.insert("ts".into(), DataValue::Str(now_rfc3339().into()));
        cozo_run(&self.db, query, p)
            .map_err(|e| CozoError::Other(format!("register subid: {e}")))?;
        Ok(())
    }

    // ── Graph ──────────────────────────────────────────────────────────────────

    pub fn store_node(
        &self,
        id: &str,
        label: &str,
        props: Value,
    ) -> std::result::Result<(), CozoError> {
        let query = r#"
            ?[id, label, props] <- [[$id, $label, $props]]
            :put graph_node { id => label, props }
        "#;
        let mut p: Params = BTreeMap::new();
        p.insert("id".into(), DataValue::Str(id.into()));
        p.insert("label".into(), DataValue::Str(label.into()));
        p.insert("props".into(), DataValue::Str(props.to_string().into()));
        cozo_run(&self.db, query, p)
            .map_err(|e| CozoError::Other(format!("store graph node: {e}")))?;
        Ok(())
    }

    pub fn store_edge(
        &self,
        src: &str,
        rel: &str,
        dst: &str,
        props: Option<Value>,
    ) -> std::result::Result<(), CozoError> {
        let query = r#"
            ?[src, rel, dst, props] <- [[$src, $rel, $dst, $props]]
            :put graph_edge { src, rel, dst => props }
        "#;
        let mut p: Params = BTreeMap::new();
        p.insert("src".into(), DataValue::Str(src.into()));
        p.insert("rel".into(), DataValue::Str(rel.into()));
        p.insert("dst".into(), DataValue::Str(dst.into()));
        p.insert(
            "props".into(),
            DataValue::Str(
                props
                    .as_ref()
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "{}".into())
                    .into(),
            ),
        );
        cozo_run(&self.db, query, p)
            .map_err(|e| CozoError::Other(format!("store graph edge: {e}")))?;
        Ok(())
    }

    /// Replace every chain-vector link for one Qdrant point id (idempotent re-ingest).
    pub fn replace_chain_vector_links(
        &self,
        point_id: &str,
        links: &[(&str, &str)],
    ) -> std::result::Result<(), CozoError> {
        let mut p: Params = BTreeMap::new();
        p.insert("pid".into(), DataValue::Str(point_id.into()));
        cozo_run(
            &self.db,
            "?[point_id, kind] := *chain_vector_link{point_id, kind, value}, point_id = $pid \
             :rm chain_vector_link {point_id, kind}",
            p.clone(),
        )
        .map_err(|e| CozoError::Other(format!("replace chain_vector_link rm: {e}")))?;

        if links.is_empty() {
            return Ok(());
        }

        let rows: Vec<DataValue> = links
            .iter()
            .map(|(kind, value)| {
                DataValue::List(vec![
                    DataValue::Str(point_id.into()),
                    DataValue::Str(kind.into()),
                    DataValue::Str(value.into()),
                ])
            })
            .collect();
        p.insert("rows".into(), DataValue::List(rows));
        cozo_run(
            &self.db,
            "?[point_id, kind, value] <- $rows :put chain_vector_link { point_id, kind => value }",
            p,
        )
        .map_err(|e| CozoError::Other(format!("replace chain_vector_link put: {e}")))?;
        Ok(())
    }

    pub fn query_edges_from(&self, src: &str) -> std::result::Result<Value, CozoError> {
        let mut p: Params = BTreeMap::new();
        p.insert("src".into(), DataValue::Str(src.into()));
        let r = cozo_run(
            &self.db,
            "?[src, rel, dst, props] := *graph_edge[src, rel, dst, props], src = $src",
            p,
        )
        .map_err(|e| CozoError::Other(format!("query edges from: {e}")))?;
        Ok(named_rows_to_json(r))
    }

    pub fn query_edges_to(&self, dst: &str) -> std::result::Result<Value, CozoError> {
        let mut p: Params = BTreeMap::new();
        p.insert("dst".into(), DataValue::Str(dst.into()));
        let r = cozo_run(
            &self.db,
            "?[src, rel, dst, props] := *graph_edge[src, rel, dst, props], dst = $dst",
            p,
        )
        .map_err(|e| CozoError::Other(format!("query edges to: {e}")))?;
        Ok(named_rows_to_json(r))
    }

    pub fn query_node(&self, id: &str) -> std::result::Result<Value, CozoError> {
        let mut p: Params = BTreeMap::new();
        p.insert("id".into(), DataValue::Str(id.into()));
        let r = cozo_run(
            &self.db,
            "?[id, label, props] := *graph_node[id, label, props], id = $id",
            p,
        )
        .map_err(|e| CozoError::Other(format!("query node: {e}")))?;
        Ok(named_rows_to_json(r))
    }

    /// BFS traversal using CozoDB's native recursive rules.
    pub fn traverse_graph(
        &self,
        start_node: &str,
        max_depth: u32,
    ) -> std::result::Result<Value, CozoError> {
        let query = r#"
            reachable[to, d] := to = $start, d = 0
            reachable[to, d] := reachable[from, d0], *graph_edge[from, _, to, _],
                                d = d0 + 1, d <= $max_depth
            ?[node, depth] := reachable[node, depth]
            :order depth
        "#;
        let mut p: Params = BTreeMap::new();
        p.insert("start".into(), DataValue::Str(start_node.into()));
        p.insert("max_depth".into(), dv_int(max_depth as i64));
        let r = cozo_run(&self.db, query, p)
            .map_err(|e| CozoError::Other(format!("traverse graph: {e}")))?;
        Ok(named_rows_to_json(r))
    }

    // ── Audit ──────────────────────────────────────────────────────────────────

    #[allow(clippy::too_many_arguments)]
    pub fn append_audit_event(
        &self,
        event_id: &str,
        subid: &str,
        plugin_id: &str,
        operation: &str,
        actor: &str,
        verdict: bool,
        reason: &str,
        control_ref: &str,
    ) -> std::result::Result<(), CozoError> {
        let query = r#"
            ?[event_id, subid, plugin_id, operation, actor, verdict, reason, control_ref, timestamp]
                <- [[$eid, $subid, $plugin, $op, $actor, $verdict, $reason, $cref, $ts]]
            :put audit_event {
                event_id => subid, plugin_id, operation, actor, verdict, reason, control_ref, timestamp
            }
        "#;
        let mut p: Params = BTreeMap::new();
        p.insert("eid".into(), DataValue::Str(event_id.into()));
        p.insert("subid".into(), DataValue::Str(subid.into()));
        p.insert("plugin".into(), DataValue::Str(plugin_id.into()));
        p.insert("op".into(), DataValue::Str(operation.into()));
        p.insert("actor".into(), DataValue::Str(actor.into()));
        p.insert(
            "verdict".into(),
            DataValue::Str(if verdict { "allow" } else { "deny" }.into()),
        );
        p.insert("reason".into(), DataValue::Str(reason.into()));
        p.insert("cref".into(), DataValue::Str(control_ref.into()));
        p.insert("ts".into(), DataValue::Str(now_rfc3339().into()));
        cozo_run(&self.db, query, p)
            .map_err(|e| CozoError::Other(format!("append audit event: {e}")))?;
        Ok(())
    }

    // ── Users (wg_pubkey-keyed) ────────────────────────────────────────────────

    /// Insert or refresh a user keyed by WireGuard public key. No PII stored.
    pub fn upsert_user(&self, wg_pubkey: &str) -> std::result::Result<(), CozoError> {
        let query = r#"
            ?[wg_pubkey, created_at] <- [[$wg, $ts]]
            :put users { wg_pubkey => created_at }
        "#;
        let mut p: Params = BTreeMap::new();
        p.insert("wg".into(), DataValue::Str(wg_pubkey.into()));
        p.insert("ts".into(), DataValue::Str(now_rfc3339().into()));
        cozo_run(&self.db, query, p).map_err(|e| CozoError::Other(format!("upsert user: {e}")))?;
        Ok(())
    }

    /// True if a user row exists for this wg_pubkey.
    pub fn user_exists(&self, wg_pubkey: &str) -> std::result::Result<bool, CozoError> {
        let mut p: Params = BTreeMap::new();
        p.insert("wg".into(), DataValue::Str(wg_pubkey.into()));
        let r = cozo_run(
            &self.db,
            "?[wg_pubkey] := *users[wg_pubkey, _], wg_pubkey = $wg",
            p,
        )
        .map_err(|e| CozoError::Other(format!("user exists: {e}")))?;
        Ok(!r.rows.is_empty())
    }

    // ── Privacy Users (full PII per explicit directive) ─────────────────────────

    #[allow(clippy::too_many_arguments)]
    pub fn upsert_privacy_user(
        &self,
        id: &str,
        email: &str,
        email_verified: bool,
        wg_public_key: &str,
        wg_private_key_encrypted: &str,
        assigned_ip: &str,
        privacy_quota_bytes: i64,
        privacy_quota_used_bytes: i64,
        privacy_container_name: &str,
        privacy_route_id: &str,
        privacy_network_connected: bool,
        privacy_network_connected_at: &str,
        google_id: &str,
        google_email: &str,
        api_credentials_json: &str,
        created_at: &str,
    ) -> std::result::Result<(), CozoError> {
        let query = r#"
            ?[id, email, email_verified, wg_public_key, wg_private_key_encrypted,
              assigned_ip, privacy_quota_bytes, privacy_quota_used_bytes,
              privacy_container_name, privacy_route_id, privacy_network_connected,
              privacy_network_connected_at, google_id, google_email,
              api_credentials_json, created_at]
                <- [[$id, $email, $ev, $wg, $wg_priv, $ip, $quota, $used,
                     $container, $route, $pnc, $pnc_at, $gid, $gmail,
                     $api_json, $ts]]
            :put privacy_users {
                id => email, email_verified, wg_public_key, wg_private_key_encrypted,
                      assigned_ip, privacy_quota_bytes, privacy_quota_used_bytes,
                      privacy_container_name, privacy_route_id, privacy_network_connected,
                      privacy_network_connected_at, google_id, google_email,
                      api_credentials_json, created_at
            }
        "#;
        let mut p: Params = BTreeMap::new();
        p.insert("id".into(), DataValue::Str(id.into()));
        p.insert("email".into(), DataValue::Str(email.into()));
        p.insert(
            "ev".into(),
            DataValue::Str(email_verified.to_string().into()),
        );
        p.insert("wg".into(), DataValue::Str(wg_public_key.into()));
        p.insert(
            "wg_priv".into(),
            DataValue::Str(wg_private_key_encrypted.into()),
        );
        p.insert("ip".into(), DataValue::Str(assigned_ip.into()));
        p.insert("quota".into(), dv_int(privacy_quota_bytes));
        p.insert("used".into(), dv_int(privacy_quota_used_bytes));
        p.insert(
            "container".into(),
            DataValue::Str(privacy_container_name.into()),
        );
        p.insert("route".into(), DataValue::Str(privacy_route_id.into()));
        p.insert(
            "pnc".into(),
            DataValue::Str(privacy_network_connected.to_string().into()),
        );
        p.insert(
            "pnc_at".into(),
            DataValue::Str(privacy_network_connected_at.into()),
        );
        p.insert("gid".into(), DataValue::Str(google_id.into()));
        p.insert("gmail".into(), DataValue::Str(google_email.into()));
        p.insert(
            "api_json".into(),
            DataValue::Str(api_credentials_json.into()),
        );
        p.insert("ts".into(), DataValue::Str(created_at.into()));
        cozo_run(&self.db, query, p)
            .map_err(|e| CozoError::Other(format!("upsert privacy user: {e}")))?;
        Ok(())
    }

    pub fn get_privacy_user(
        &self,
        id: &str,
    ) -> std::result::Result<Option<Vec<DataValue>>, CozoError> {
        let mut p: Params = BTreeMap::new();
        p.insert("id".into(), DataValue::Str(id.into()));
        let r = cozo_run(
            &self.db,
            "?[email, email_verified, wg_public_key, wg_private_key_encrypted, \
             assigned_ip, privacy_quota_bytes, privacy_quota_used_bytes, \
             privacy_container_name, privacy_route_id, privacy_network_connected, \
             privacy_network_connected_at, google_id, google_email, \
             api_credentials_json, created_at] := \
             *privacy_users[id, email, email_verified, wg_public_key, wg_private_key_encrypted, \
             assigned_ip, privacy_quota_bytes, privacy_quota_used_bytes, \
             privacy_container_name, privacy_route_id, privacy_network_connected, \
             privacy_network_connected_at, google_id, google_email, \
             api_credentials_json, created_at], id = $id",
            p,
        )
        .map_err(|e| CozoError::Other(format!("get privacy user: {e}")))?;
        Ok(r.rows.into_iter().next())
    }

    pub fn get_privacy_user_by_email(
        &self,
        email: &str,
    ) -> std::result::Result<Option<Vec<DataValue>>, CozoError> {
        let mut p: Params = BTreeMap::new();
        p.insert("email".into(), DataValue::Str(email.into()));
        let r = cozo_run(
            &self.db,
            "?[id, email_verified, wg_public_key, wg_private_key_encrypted, \
             assigned_ip, privacy_quota_bytes, privacy_quota_used_bytes, \
             privacy_container_name, privacy_route_id, privacy_network_connected, \
             privacy_network_connected_at, google_id, google_email, \
             api_credentials_json, created_at] := \
             *privacy_users[id, email, email_verified, wg_public_key, wg_private_key_encrypted, \
             assigned_ip, privacy_quota_bytes, privacy_quota_used_bytes, \
             privacy_container_name, privacy_route_id, privacy_network_connected, \
             privacy_network_connected_at, google_id, google_email, \
             api_credentials_json, created_at], email = $email",
            p,
        )
        .map_err(|e| CozoError::Other(format!("get privacy user by email: {e}")))?;
        Ok(r.rows.into_iter().next())
    }

    pub fn get_privacy_user_by_google_id(
        &self,
        google_id: &str,
    ) -> std::result::Result<Option<Vec<DataValue>>, CozoError> {
        let mut p: Params = BTreeMap::new();
        p.insert("gid".into(), DataValue::Str(google_id.into()));
        let r = cozo_run(
            &self.db,
            "?[id, email, email_verified, wg_public_key, wg_private_key_encrypted, \
             assigned_ip, privacy_quota_bytes, privacy_quota_used_bytes, \
             privacy_container_name, privacy_route_id, privacy_network_connected, \
             privacy_network_connected_at, google_email, \
             api_credentials_json, created_at] := \
             *privacy_users[id, email, email_verified, wg_public_key, wg_private_key_encrypted, \
             assigned_ip, privacy_quota_bytes, privacy_quota_used_bytes, \
             privacy_container_name, privacy_route_id, privacy_network_connected, \
             privacy_network_connected_at, google_id, google_email, \
             api_credentials_json, created_at], google_id = $gid",
            p,
        )
        .map_err(|e| CozoError::Other(format!("get privacy user by google id: {e}")))?;
        Ok(r.rows.into_iter().next())
    }

    pub fn list_privacy_users(&self) -> std::result::Result<Vec<Vec<DataValue>>, CozoError> {
        let r = cozo_run(
            &self.db,
            "?[id, email, email_verified, wg_public_key, wg_private_key_encrypted, \
             assigned_ip, privacy_quota_bytes, privacy_quota_used_bytes, \
             privacy_container_name, privacy_route_id, privacy_network_connected, \
             privacy_network_connected_at, google_id, google_email, \
             api_credentials_json, created_at] := \
             *privacy_users[id, email, email_verified, wg_public_key, wg_private_key_encrypted, \
             assigned_ip, privacy_quota_bytes, privacy_quota_used_bytes, \
             privacy_container_name, privacy_route_id, privacy_network_connected, \
             privacy_network_connected_at, google_id, google_email, \
             api_credentials_json, created_at]",
            BTreeMap::new(),
        )
        .map_err(|e| CozoError::Other(format!("list privacy users: {e}")))?;
        Ok(r.rows)
    }

    pub fn delete_privacy_user(&self, id: &str) -> std::result::Result<(), CozoError> {
        let mut p: Params = BTreeMap::new();
        p.insert("id".into(), DataValue::Str(id.into()));
        cozo_run(&self.db, "?[id] <- [[$id]] :rm privacy_users { id }", p)
            .map_err(|e| CozoError::Other(format!("delete privacy user: {e}")))?;
        Ok(())
    }

    // ── Sessions ───────────────────────────────────────────────────────────────

    /// Bind a session_id to a wg_pubkey. `expires_at` is RFC3339 ("" = no expiry).
    pub fn create_session(
        &self,
        session_id: &str,
        wg_pubkey: &str,
        expires_at: Option<&str>,
    ) -> std::result::Result<(), CozoError> {
        let query = r#"
            ?[session_id, wg_pubkey, created_at, expires_at]
                <- [[$sid, $wg, $now, $exp]]
            :put sessions { session_id => wg_pubkey, created_at, expires_at }
        "#;
        let mut p: Params = BTreeMap::new();
        p.insert("sid".into(), DataValue::Str(session_id.into()));
        p.insert("wg".into(), DataValue::Str(wg_pubkey.into()));
        p.insert("now".into(), DataValue::Str(now_rfc3339().into()));
        p.insert(
            "exp".into(),
            DataValue::Str(expires_at.unwrap_or("").into()),
        );
        cozo_run(&self.db, query, p)
            .map_err(|e| CozoError::Other(format!("create session: {e}")))?;
        Ok(())
    }

    /// Resolve a session_id to (wg_pubkey, created_at, expires_at). None if absent.
    pub fn lookup_session(
        &self,
        session_id: &str,
    ) -> std::result::Result<Option<(String, String, String)>, CozoError> {
        let mut p: Params = BTreeMap::new();
        p.insert("sid".into(), DataValue::Str(session_id.into()));
        let r = cozo_run(
            &self.db,
            "?[wg_pubkey, created_at, expires_at] := \
             *sessions[sid, wg_pubkey, created_at, expires_at], sid = $sid",
            p,
        )
        .map_err(|e| CozoError::Other(format!("lookup session: {e}")))?;
        if let Some(row) = r.rows.first() {
            let wg = dv_as_str(&row[0]).unwrap_or("").to_string();
            let created = dv_as_str(&row[1]).unwrap_or("").to_string();
            let expires = dv_as_str(&row[2]).unwrap_or("").to_string();
            Ok(Some((wg, created, expires)))
        } else {
            Ok(None)
        }
    }

    /// Delete a session row.
    pub fn delete_session(&self, session_id: &str) -> std::result::Result<(), CozoError> {
        let mut p: Params = BTreeMap::new();
        p.insert("sid".into(), DataValue::Str(session_id.into()));
        cozo_run(
            &self.db,
            "?[session_id] <- [[$sid]] :rm sessions { session_id }",
            p,
        )
        .map_err(|e| CozoError::Other(format!("delete session: {e}")))?;
        Ok(())
    }

    // ── Account sessions (consumer path — no PII) ─────────────────────────────

    /// Persist a verified account: session_id + blake3(session_id) proof keyed by wg_pubkey.
    pub fn upsert_account_session(
        &self,
        wg_pubkey: &str,
        session_id: &str,
        session_proof: &str,
    ) -> std::result::Result<(), CozoError> {
        let query = r#"
            ?[wg_pubkey, session_id, session_proof, created_at]
                <- [[$wg, $sid, $proof, $ts]]
            :put account_sessions {
                wg_pubkey => session_id, session_proof, created_at
            }
        "#;
        let mut p: Params = BTreeMap::new();
        p.insert("wg".into(), DataValue::Str(wg_pubkey.into()));
        p.insert("sid".into(), DataValue::Str(session_id.into()));
        p.insert("proof".into(), DataValue::Str(session_proof.into()));
        p.insert("ts".into(), DataValue::Str(now_rfc3339().into()));
        cozo_run(&self.db, query, p)
            .map_err(|e| CozoError::Other(format!("upsert account session: {e}")))?;
        Ok(())
    }

    // ── Consumer accounts (GhostBridge path — no email) ───────────────────────

    #[allow(clippy::too_many_arguments)]
    pub fn upsert_consumer_account(
        &self,
        session_id: &str,
        wg_public_key: &str,
        wg_private_key_encrypted: &str,
        assigned_ip: &str,
        email_verified: bool,
        privacy_quota_bytes: i64,
        privacy_quota_used_bytes: i64,
        privacy_container_name: &str,
        privacy_route_id: &str,
        privacy_network_connected: bool,
        privacy_network_connected_at: &str,
        api_credentials_json: &str,
        created_at: &str,
    ) -> std::result::Result<(), CozoError> {
        let query = r#"
            ?[session_id, wg_public_key, wg_private_key_encrypted, assigned_ip, email_verified,
              privacy_quota_bytes, privacy_quota_used_bytes, privacy_container_name,
              privacy_route_id, privacy_network_connected, privacy_network_connected_at,
              api_credentials_json, created_at]
                <- [[$sid, $wg, $wg_priv, $ip, $ev, $quota, $used, $container, $route,
                     $pnc, $pnc_at, $api_json, $ts]]
            :put consumer_accounts {
                session_id => wg_public_key, wg_private_key_encrypted, assigned_ip,
                    email_verified, privacy_quota_bytes, privacy_quota_used_bytes,
                    privacy_container_name, privacy_route_id, privacy_network_connected,
                    privacy_network_connected_at, api_credentials_json, created_at
            }
        "#;
        let mut p: Params = BTreeMap::new();
        p.insert("sid".into(), DataValue::Str(session_id.into()));
        p.insert("wg".into(), DataValue::Str(wg_public_key.into()));
        p.insert(
            "wg_priv".into(),
            DataValue::Str(wg_private_key_encrypted.into()),
        );
        p.insert("ip".into(), DataValue::Str(assigned_ip.into()));
        p.insert(
            "ev".into(),
            DataValue::Str(email_verified.to_string().into()),
        );
        p.insert("quota".into(), dv_int(privacy_quota_bytes));
        p.insert("used".into(), dv_int(privacy_quota_used_bytes));
        p.insert(
            "container".into(),
            DataValue::Str(privacy_container_name.into()),
        );
        p.insert("route".into(), DataValue::Str(privacy_route_id.into()));
        p.insert(
            "pnc".into(),
            DataValue::Str(privacy_network_connected.to_string().into()),
        );
        p.insert(
            "pnc_at".into(),
            DataValue::Str(privacy_network_connected_at.into()),
        );
        p.insert(
            "api_json".into(),
            DataValue::Str(api_credentials_json.into()),
        );
        p.insert("ts".into(), DataValue::Str(created_at.into()));
        cozo_run(&self.db, query, p)
            .map_err(|e| CozoError::Other(format!("upsert consumer account: {e}")))?;
        Ok(())
    }

    pub fn get_consumer_account(
        &self,
        session_id: &str,
    ) -> std::result::Result<Option<Vec<DataValue>>, CozoError> {
        let mut p: Params = BTreeMap::new();
        p.insert("sid".into(), DataValue::Str(session_id.into()));
        let r = cozo_run(
            &self.db,
            "?[wg_public_key, wg_private_key_encrypted, assigned_ip, email_verified,
             privacy_quota_bytes, privacy_quota_used_bytes, privacy_container_name,
             privacy_route_id, privacy_network_connected, privacy_network_connected_at,
             api_credentials_json, created_at] := \
             *consumer_accounts[session_id, wg_public_key, wg_private_key_encrypted, assigned_ip,
             email_verified, privacy_quota_bytes, privacy_quota_used_bytes,
             privacy_container_name, privacy_route_id, privacy_network_connected,
             privacy_network_connected_at, api_credentials_json, created_at], session_id = $sid",
            p,
        )
        .map_err(|e| CozoError::Other(format!("get consumer account: {e}")))?;
        Ok(r.rows.into_iter().next())
    }

    pub fn list_consumer_accounts(&self) -> std::result::Result<Vec<Vec<DataValue>>, CozoError> {
        let r = cozo_run(
            &self.db,
            "?[session_id, wg_public_key, wg_private_key_encrypted, assigned_ip, email_verified,
             privacy_quota_bytes, privacy_quota_used_bytes, privacy_container_name,
             privacy_route_id, privacy_network_connected, privacy_network_connected_at,
             api_credentials_json, created_at] := \
             *consumer_accounts[session_id, wg_public_key, wg_private_key_encrypted, assigned_ip,
             email_verified, privacy_quota_bytes, privacy_quota_used_bytes,
             privacy_container_name, privacy_route_id, privacy_network_connected,
             privacy_network_connected_at, api_credentials_json, created_at]",
            BTreeMap::new(),
        )
        .map_err(|e| CozoError::Other(format!("list consumer accounts: {e}")))?;
        Ok(r.rows)
    }

    /// Lookup account session proof by wg_pubkey.
    pub fn lookup_account_session(
        &self,
        wg_pubkey: &str,
    ) -> std::result::Result<Option<(String, String, String)>, CozoError> {
        let mut p: Params = BTreeMap::new();
        p.insert("wg".into(), DataValue::Str(wg_pubkey.into()));
        let r = cozo_run(
            &self.db,
            "?[session_id, session_proof, created_at] := \
             *account_sessions[wg_pubkey, session_id, session_proof, created_at], \
             wg_pubkey = $wg",
            p,
        )
        .map_err(|e| CozoError::Other(format!("lookup account session: {e}")))?;
        if let Some(row) = r.rows.first() {
            let sid = dv_as_str(&row[0]).unwrap_or("").to_string();
            let proof = dv_as_str(&row[1]).unwrap_or("").to_string();
            let created = dv_as_str(&row[2]).unwrap_or("").to_string();
            Ok(Some((sid, proof, created)))
        } else {
            Ok(None)
        }
    }

    // ── WireGuard gateway sessions ───────────────────────────────────────────────

    /// Upsert a full WireGuard gateway session record and its peer_pubkey → session_id
    /// mapping, atomically. Uses Cozo's `multi_transaction` so a failure partway
    /// through (e.g. the peer-mapping write) rolls back the session write too —
    /// otherwise a restart could resurrect a session row with no peer mapping.
    /// Also deactivates any *other* still-active `wg_sessions` row for this same
    /// peer_pubkey — otherwise `load_wireguard_sessions()` would reload both the
    /// old and new session as valid bearers after a restart.
    pub fn put_wg_session(&self, rec: &WgSessionRecord) -> std::result::Result<(), CozoError> {
        let txn = self.db.multi_transaction(true);

        let mut dp: Params = BTreeMap::new();
        dp.insert(
            "peer".into(),
            DataValue::Str(rec.peer_pubkey.as_str().into()),
        );
        dp.insert(
            "new_sid".into(),
            DataValue::Str(rec.session_id.as_str().into()),
        );
        if let Err(e) = txn.run_script(
            r#"
                superseded[session_id] := *wg_peer_sessions[peer_pubkey, session_id],
                                           peer_pubkey = $peer, session_id != $new_sid
                ?[session_id, is_active] := superseded[session_id], is_active = false
                :update wg_sessions { session_id => is_active }
            "#,
            dp,
        ) {
            let _ = txn.abort();
            return Err(CozoError::Other(format!(
                "deactivate superseded wg session: {e}"
            )));
        }

        let query = r#"
            ?[session_id, peer_pubkey, psk, created_at, expires_at, is_active, last_used,
              client_ip, client_version, auth_method, key_rotation_count, flags_json]
                <- [[$sid, $peer, $psk, $created, $expires, $active, $last_used,
                     $client_ip, $client_version, $auth_method, $rotations, $flags]]
            :put wg_sessions {
                session_id => peer_pubkey, psk, created_at, expires_at, is_active, last_used,
                client_ip, client_version, auth_method, key_rotation_count, flags_json
            }
        "#;
        let mut p: Params = BTreeMap::new();
        p.insert("sid".into(), DataValue::Str(rec.session_id.as_str().into()));
        p.insert(
            "peer".into(),
            DataValue::Str(rec.peer_pubkey.as_str().into()),
        );
        p.insert("psk".into(), DataValue::Str(rec.psk.as_str().into()));
        p.insert("created".into(), dv_int(rec.created_at as i64));
        p.insert("expires".into(), dv_int(rec.expires_at as i64));
        p.insert("active".into(), DataValue::Bool(rec.is_active));
        p.insert("last_used".into(), dv_int(rec.last_used as i64));
        p.insert(
            "client_ip".into(),
            DataValue::Str(rec.client_ip.clone().unwrap_or_default().into()),
        );
        p.insert(
            "client_version".into(),
            DataValue::Str(rec.client_version.clone().unwrap_or_default().into()),
        );
        p.insert(
            "auth_method".into(),
            DataValue::Str(rec.auth_method.as_str().into()),
        );
        p.insert("rotations".into(), dv_int(rec.key_rotation_count as i64));
        p.insert(
            "flags".into(),
            DataValue::Str(rec.flags_json.as_str().into()),
        );
        if let Err(e) = txn.run_script(query, p) {
            let _ = txn.abort();
            return Err(CozoError::Other(format!("put wg session: {e}")));
        }

        let mut pp: Params = BTreeMap::new();
        pp.insert(
            "peer".into(),
            DataValue::Str(rec.peer_pubkey.as_str().into()),
        );
        pp.insert("sid".into(), DataValue::Str(rec.session_id.as_str().into()));
        if let Err(e) = txn.run_script(
            "?[peer_pubkey, session_id] <- [[$peer, $sid]] :put wg_peer_sessions { peer_pubkey => session_id }",
            pp,
        ) {
            let _ = txn.abort();
            return Err(CozoError::Other(format!("put wg peer session: {e}")));
        }

        txn.commit()
            .map_err(|e| CozoError::Other(format!("commit wg session write: {e}")))?;
        Ok(())
    }

    /// Atomically bump only the `last_used` column of a WireGuard gateway session.
    /// Uses Cozo's `:update` (partial-column update) rather than a read-modify-write
    /// of the full record, so a concurrent `put_wg_session` (e.g. a key rotation
    /// changing `flags`/`key_rotation_count`) can't be silently clobbered by a
    /// stale copy of those fields being written back here.
    pub fn update_wg_session_last_used(
        &self,
        session_id: &str,
        last_used: u64,
    ) -> std::result::Result<(), CozoError> {
        let mut p: Params = BTreeMap::new();
        p.insert("sid".into(), DataValue::Str(session_id.into()));
        p.insert("last_used".into(), dv_int(last_used as i64));
        cozo_run(
            &self.db,
            "?[session_id, last_used] <- [[$sid, $last_used]] \
             :update wg_sessions { session_id => last_used }",
            p,
        )
        .map_err(|e| CozoError::Other(format!("update wg session last_used: {e}")))?;
        Ok(())
    }

    /// Fetch a single WireGuard gateway session by session_id.
    pub fn get_wg_session(
        &self,
        session_id: &str,
    ) -> std::result::Result<Option<WgSessionRecord>, CozoError> {
        let mut p: Params = BTreeMap::new();
        p.insert("sid".into(), DataValue::Str(session_id.into()));
        let r = cozo_run(
            &self.db,
            "?[session_id, peer_pubkey, psk, created_at, expires_at, is_active, last_used, \
             client_ip, client_version, auth_method, key_rotation_count, flags_json] := \
             *wg_sessions[session_id, peer_pubkey, psk, created_at, expires_at, is_active, \
             last_used, client_ip, client_version, auth_method, key_rotation_count, flags_json], \
             session_id = $sid",
            p,
        )
        .map_err(|e| CozoError::Other(format!("get wg session: {e}")))?;
        Ok(r.rows.first().map(|r| row_to_wg_session(r)))
    }

    /// List every persisted WireGuard gateway session (used to warm the in-memory
    /// cache on `WireGuardAuthManager` startup).
    pub fn list_wg_sessions(&self) -> std::result::Result<Vec<WgSessionRecord>, CozoError> {
        let r = cozo_run(
            &self.db,
            "?[session_id, peer_pubkey, psk, created_at, expires_at, is_active, last_used, \
             client_ip, client_version, auth_method, key_rotation_count, flags_json] := \
             *wg_sessions[session_id, peer_pubkey, psk, created_at, expires_at, is_active, \
             last_used, client_ip, client_version, auth_method, key_rotation_count, flags_json]",
            BTreeMap::new(),
        )
        .map_err(|e| CozoError::Other(format!("list wg sessions: {e}")))?;
        Ok(r.rows.iter().map(|r| row_to_wg_session(r)).collect())
    }

    /// Resolve a peer_pubkey to its current session_id, if any.
    pub fn lookup_wg_peer_session(
        &self,
        peer_pubkey: &str,
    ) -> std::result::Result<Option<String>, CozoError> {
        let mut p: Params = BTreeMap::new();
        p.insert("peer".into(), DataValue::Str(peer_pubkey.into()));
        let r = cozo_run(
            &self.db,
            "?[session_id] := *wg_peer_sessions[peer_pubkey, session_id], peer_pubkey = $peer",
            p,
        )
        .map_err(|e| CozoError::Other(format!("lookup wg peer session: {e}")))?;
        Ok(r.rows
            .first()
            .and_then(|row| dv_as_str(&row[0]))
            .map(String::from))
    }

    /// Delete a WireGuard gateway session and its peer mapping.
    pub fn delete_wg_session(
        &self,
        session_id: &str,
        peer_pubkey: &str,
    ) -> std::result::Result<(), CozoError> {
        let mut p: Params = BTreeMap::new();
        p.insert("sid".into(), DataValue::Str(session_id.into()));
        cozo_run(
            &self.db,
            "?[session_id] <- [[$sid]] :rm wg_sessions { session_id }",
            p,
        )
        .map_err(|e| CozoError::Other(format!("delete wg session: {e}")))?;

        // Only clear the peer -> session mapping if it still points at the session
        // being deleted. Without this check, deleting an *expired* session whose
        // peer has since re-authenticated into a newer session would wipe that
        // newer session's mapping too — the derivation of the delete set from
        // *wg_peer_sessions itself (matching on both peer_pubkey and session_id)
        // makes this a single atomic conditional delete, not a check-then-act.
        let mut pp: Params = BTreeMap::new();
        pp.insert("peer".into(), DataValue::Str(peer_pubkey.into()));
        pp.insert("sid".into(), DataValue::Str(session_id.into()));
        cozo_run(
            &self.db,
            r#"
                matched[peer_pubkey] := *wg_peer_sessions[peer_pubkey, session_id],
                                         peer_pubkey = $peer, session_id = $sid
                ?[peer_pubkey] := matched[peer_pubkey]
                :rm wg_peer_sessions { peer_pubkey }
            "#,
            pp,
        )
        .map_err(|e| CozoError::Other(format!("delete wg peer session: {e}")))?;
        Ok(())
    }

    // ── Container identity sleds ─────────────────────────────────────────────────

    /// Upsert a full container identity sled row.
    pub fn put_identity_sled(
        &self,
        rec: &IdentitySledRecord,
    ) -> std::result::Result<(), CozoError> {
        let _guard = self.legacy_identity_write_guard(&rec.session_id)?;
        if rec.principal_kind.is_some() {
            return Err(CozoError::Other(
                "actor classification requires atomic identity intent".into(),
            ));
        }
        let query = r#"
            ?[session_id, wireguard_pubkey, interface, peer_ip, mutation_index,
              session_genesis, trace_id, schema_version, vector_id, sealed_id,
              btrfs_device_json, instance_json, session_started_at, last_seen_at, active,
              expires_at]
                <- [[$sid, $pubkey, $iface, $peer_ip, $mut_idx, $genesis, $trace,
                     $schema_ver, $vector, $sealed_id, $btrfs_dev, $instance, $started, $seen, $active,
                     $expires]]
            :put identity_sessions {
                session_id => wireguard_pubkey, interface, peer_ip, mutation_index,
                session_genesis, trace_id, schema_version, vector_id, sealed_id,
                btrfs_device_json, instance_json, session_started_at, last_seen_at, active,
                expires_at
            }
        "#;
        let mut p: Params = BTreeMap::new();
        p.insert("sid".into(), DataValue::Str(rec.session_id.as_str().into()));
        p.insert(
            "pubkey".into(),
            DataValue::Str(rec.wireguard_pubkey.as_str().into()),
        );
        p.insert(
            "iface".into(),
            DataValue::Str(rec.interface.as_str().into()),
        );
        p.insert(
            "peer_ip".into(),
            DataValue::Str(rec.peer_ip.as_str().into()),
        );
        p.insert("mut_idx".into(), dv_int(rec.mutation_index));
        p.insert(
            "genesis".into(),
            DataValue::Str(rec.session_genesis.as_str().into()),
        );
        p.insert("trace".into(), DataValue::Str(rec.trace_id.as_str().into()));
        p.insert("schema_ver".into(), dv_int(rec.schema_version));
        p.insert(
            "vector".into(),
            DataValue::Str(rec.vector_id.as_str().into()),
        );
        p.insert(
            "sealed_id".into(),
            DataValue::Str(rec.sealed_id.as_str().into()),
        );
        p.insert(
            "btrfs_dev".into(),
            DataValue::Str(rec.btrfs_device_json.as_str().into()),
        );
        p.insert(
            "instance".into(),
            DataValue::Str(rec.instance_json.as_str().into()),
        );
        p.insert("started".into(), dv_int(rec.session_started_at));
        p.insert("seen".into(), dv_int(rec.last_seen_at));
        p.insert("active".into(), DataValue::Bool(rec.active));
        p.insert("expires".into(), dv_int(rec.expires_at));
        cozo_run(&self.db, query, p)
            .map_err(|e| CozoError::Other(format!("put identity sled: {e}")))?;
        Ok(())
    }

    /// Written once at arrival. Without `arrival_timestamp` the genesis cannot
    /// be recomputed, so this write is the durability of the anchor.
    pub fn put_identity_genesis(
        &self,
        rec: &GenesisInputsRecord,
    ) -> std::result::Result<(), CozoError> {
        let _guard = self.legacy_identity_write_guard(&rec.session_id)?;
        let query = r#"
            ?[session_id, arrival_timestamp, chain_head_at_arrival,
              catalog_hash_at_arrival, head_timestamp_at_arrival, schema_content_hash]
                <- [[$sid, $arrival, $head, $catalog, $head_ts, $shape]]
            :put identity_genesis {
                session_id => arrival_timestamp, chain_head_at_arrival,
                catalog_hash_at_arrival, head_timestamp_at_arrival, schema_content_hash
            }
        "#;
        let mut p: Params = BTreeMap::new();
        p.insert("sid".into(), DataValue::Str(rec.session_id.as_str().into()));
        p.insert("arrival".into(), dv_int(rec.arrival_timestamp));
        p.insert(
            "head".into(),
            DataValue::Str(rec.chain_head_at_arrival.as_str().into()),
        );
        p.insert(
            "catalog".into(),
            DataValue::Str(rec.catalog_hash_at_arrival.as_str().into()),
        );
        p.insert("head_ts".into(), dv_int(rec.head_timestamp_at_arrival));
        p.insert(
            "shape".into(),
            DataValue::Str(rec.schema_content_hash.as_str().into()),
        );
        cozo_run(&self.db, query, p)
            .map_err(|e| CozoError::Other(format!("put identity genesis: {e}")))?;
        Ok(())
    }

    /// Every stored genesis-input row, for the one hydration read.
    pub fn list_identity_genesis(
        &self,
    ) -> std::result::Result<Vec<GenesisInputsRecord>, CozoError> {
        let r = cozo_run(
            &self.db,
            "?[session_id, arrival_timestamp, chain_head_at_arrival, \
             catalog_hash_at_arrival, head_timestamp_at_arrival, schema_content_hash] := \
             *identity_genesis[session_id, arrival_timestamp, chain_head_at_arrival, \
             catalog_hash_at_arrival, head_timestamp_at_arrival, schema_content_hash]",
            BTreeMap::new(),
        )
        .map_err(|e| CozoError::Other(format!("list identity genesis: {e}")))?;
        Ok(r.rows.iter().map(|r| row_to_genesis_inputs(r)).collect())
    }

    /// Fetch a single identity sled row by session_id.
    pub fn get_identity_sled(
        &self,
        session_id: &str,
    ) -> std::result::Result<Option<IdentitySledRecord>, CozoError> {
        let mut p: Params = BTreeMap::new();
        p.insert("sid".into(), DataValue::Str(session_id.into()));
        let r = cozo_run(
            &self.db,
            "actor[session_id, principal_kind] := *identity_revisions{session_id, principal_kind}\n\
             actor[session_id, principal_kind] := *identity_sessions{session_id}, not *identity_revisions{session_id}, principal_kind = ''\n\
             ?[session_id, wireguard_pubkey, interface, peer_ip, mutation_index, \
             session_genesis, trace_id, schema_version, vector_id, sealed_id, \
             btrfs_device_json, instance_json, session_started_at, last_seen_at, active, \
             expires_at, principal_kind] := \
             *identity_sessions[session_id, wireguard_pubkey, interface, peer_ip, mutation_index, \
             session_genesis, trace_id, schema_version, vector_id, sealed_id, \
             btrfs_device_json, instance_json, session_started_at, last_seen_at, active, \
             expires_at], actor[session_id, principal_kind], \
             session_id = $sid",
            p,
        )
        .map_err(|e| CozoError::Other(format!("get identity sled: {e}")))?;
        Ok(r.rows
            .first()
            .map(|row| row_to_identity_sled(&r.headers, row)))
    }

    /// List every persisted identity sled (used to warm the dispatch cache on
    /// engine startup).
    pub fn list_identity_sessions(
        &self,
    ) -> std::result::Result<Vec<IdentitySledRecord>, CozoError> {
        let r = cozo_run(
            &self.db,
            "actor[session_id, principal_kind] := *identity_revisions{session_id, principal_kind}\n\
             actor[session_id, principal_kind] := *identity_sessions{session_id}, not *identity_revisions{session_id}, principal_kind = ''\n\
             ?[session_id, wireguard_pubkey, interface, peer_ip, mutation_index, \
             session_genesis, trace_id, schema_version, vector_id, sealed_id, \
             btrfs_device_json, instance_json, session_started_at, last_seen_at, active, \
             expires_at, principal_kind] := \
             *identity_sessions[session_id, wireguard_pubkey, interface, peer_ip, mutation_index, \
             session_genesis, trace_id, schema_version, vector_id, sealed_id, \
             btrfs_device_json, instance_json, session_started_at, last_seen_at, active, \
             expires_at], actor[session_id, principal_kind]",
            BTreeMap::new(),
        )
        .map_err(|e| CozoError::Other(format!("list identity sleds: {e}")))?;
        Ok(r.rows
            .iter()
            .map(|row| row_to_identity_sled(&r.headers, row))
            .collect())
    }

    /// Atomically bump only `last_seen_at`/`active` on an identity sled — an
    /// `:update` (partial-column) so a concurrent `put_identity_sled` can't be
    /// clobbered by a stale full-record write-back.
    pub fn touch_identity_sled(
        &self,
        session_id: &str,
        last_seen_at: i64,
        active: bool,
    ) -> std::result::Result<(), CozoError> {
        let _guard = self.legacy_identity_write_guard(session_id)?;
        let mut p: Params = BTreeMap::new();
        p.insert("sid".into(), DataValue::Str(session_id.into()));
        p.insert("seen".into(), dv_int(last_seen_at));
        p.insert("active".into(), DataValue::Bool(active));
        cozo_run(
            &self.db,
            "?[session_id, last_seen_at, active] <- [[$sid, $seen, $active]] \
             :update identity_sessions { session_id => last_seen_at, active }",
            p,
        )
        .map_err(|e| CozoError::Other(format!("touch identity sled: {e}")))?;
        Ok(())
    }

    /// Remove one identity sled and its genesis/snowball archive rows.
    pub fn delete_identity_sled(&self, session_id: &str) -> std::result::Result<(), CozoError> {
        let _guard = self.legacy_identity_write_guard(session_id)?;
        let mut p: Params = BTreeMap::new();
        p.insert("sid".into(), DataValue::Str(session_id.into()));
        cozo_run(
            &self.db,
            r#"
                ?[session_id] <- [[$sid]]
                :rm identity_sessions { session_id }
            "#,
            p.clone(),
        )
        .map_err(|e| CozoError::Other(format!("delete identity sled: {e}")))?;
        cozo_run(
            &self.db,
            r#"
                ?[session_id] <- [[$sid]]
                :rm identity_genesis { session_id }
            "#,
            p.clone(),
        )
        .map_err(|e| CozoError::Other(format!("delete identity genesis: {e}")))?;
        cozo_run(
            &self.db,
            r#"
                doomed[session_id, seq] := *session_events[session_id, seq, kind, subid, content, created_at], session_id = $sid
                ?[session_id, seq] := doomed[session_id, seq]
                :rm session_events { session_id, seq }
            "#,
            p,
        )
        .map_err(|e| CozoError::Other(format!("delete session events: {e}")))?;
        Ok(())
    }

    // ── Human principals ───────────────────────────────────────────────────────

    /// Upsert a full human principal row and its human_pubkey → principal_id
    /// mapping, atomically (both rows or neither, mirroring `put_wg_session`).
    /// Policy (pubkey shape, duplicate/alias rules, tombstone enforcement)
    /// lives with the caller; this layer persists the record verbatim.
    pub fn put_human_principal(
        &self,
        rec: &HumanPrincipalRecord,
    ) -> std::result::Result<(), CozoError> {
        let txn = self.db.multi_transaction(true);

        let mut p: Params = BTreeMap::new();
        p.insert(
            "pid".into(),
            DataValue::Str(rec.principal_id.as_str().into()),
        );
        p.insert(
            "pk".into(),
            DataValue::Str(rec.human_pubkey.as_str().into()),
        );
        p.insert(
            "alias".into(),
            DataValue::Str(rec.display_alias.as_str().into()),
        );
        p.insert("registered".into(), dv_int(rec.registered_at));
        p.insert("revoked".into(), dv_int(rec.revoked_at));
        if let Err(e) = txn.run_script(
            r#"
                ?[principal_id, human_pubkey, display_alias, registered_at, revoked_at]
                    <- [[$pid, $pk, $alias, $registered, $revoked]]
                :put human_principals {
                    principal_id => human_pubkey, display_alias, registered_at, revoked_at
                }
            "#,
            p,
        ) {
            let _ = txn.abort();
            return Err(CozoError::Other(format!("put human principal: {e}")));
        }

        let mut pp: Params = BTreeMap::new();
        pp.insert(
            "pk".into(),
            DataValue::Str(rec.human_pubkey.as_str().into()),
        );
        pp.insert(
            "pid".into(),
            DataValue::Str(rec.principal_id.as_str().into()),
        );
        if let Err(e) = txn.run_script(
            "?[human_pubkey, principal_id] <- [[$pk, $pid]] \
             :put human_principal_pubkeys { human_pubkey => principal_id }",
            pp,
        ) {
            let _ = txn.abort();
            return Err(CozoError::Other(format!("put human principal pubkey: {e}")));
        }

        txn.commit()
            .map_err(|e| CozoError::Other(format!("commit human principal write: {e}")))?;
        Ok(())
    }

    /// Fetch a single human principal row by its derived principal_id.
    pub fn get_human_principal(
        &self,
        principal_id: &str,
    ) -> std::result::Result<Option<HumanPrincipalRecord>, CozoError> {
        let mut p: Params = BTreeMap::new();
        p.insert("pid".into(), DataValue::Str(principal_id.into()));
        let r = cozo_run(
            &self.db,
            "?[principal_id, human_pubkey, display_alias, registered_at, revoked_at] := \
             *human_principals[principal_id, human_pubkey, display_alias, registered_at, revoked_at], \
             principal_id = $pid",
            p,
        )
        .map_err(|e| CozoError::Other(format!("get human principal: {e}")))?;
        Ok(r.rows.first().map(|r| row_to_human_principal(r)))
    }

    /// Resolve a human principal by its unique WireGuard pubkey. Revoked
    /// principals keep their mapping, so they resolve here with `revoked_at`
    /// set (visibility — never as active, never not-found).
    pub fn get_human_principal_by_pubkey(
        &self,
        human_pubkey: &str,
    ) -> std::result::Result<Option<HumanPrincipalRecord>, CozoError> {
        let mut p: Params = BTreeMap::new();
        p.insert("pk".into(), DataValue::Str(human_pubkey.into()));
        let r = cozo_run(
            &self.db,
            "?[principal_id] := \
             *human_principal_pubkeys[human_pubkey, principal_id], human_pubkey = $pk",
            p,
        )
        .map_err(|e| CozoError::Other(format!("lookup human principal pubkey: {e}")))?;
        let Some(row) = r.rows.first() else {
            return Ok(None);
        };
        let Some(principal_id) = dv_as_str(&row[0]) else {
            return Ok(None);
        };
        self.get_human_principal(principal_id)
    }

    /// List every registered human principal, revoked tombstones included.
    pub fn list_human_principals(
        &self,
    ) -> std::result::Result<Vec<HumanPrincipalRecord>, CozoError> {
        let r = cozo_run(
            &self.db,
            "?[principal_id, human_pubkey, display_alias, registered_at, revoked_at] := \
             *human_principals[principal_id, human_pubkey, display_alias, registered_at, revoked_at]",
            BTreeMap::new(),
        )
        .map_err(|e| CozoError::Other(format!("list human principals: {e}")))?;
        Ok(r.rows.iter().map(|r| row_to_human_principal(r)).collect())
    }

    /// Stamp `revoked_at` on a human principal — a partial-column `:update`,
    /// so the rest of the row cannot be clobbered by a stale full-record
    /// write-back. Idempotency (never re-stamping an existing `revoked_at`)
    /// is the caller's policy.
    pub fn revoke_human_principal(
        &self,
        principal_id: &str,
        revoked_at: i64,
    ) -> std::result::Result<(), CozoError> {
        let mut p: Params = BTreeMap::new();
        p.insert("pid".into(), DataValue::Str(principal_id.into()));
        p.insert("revoked".into(), dv_int(revoked_at));
        cozo_run(
            &self.db,
            "?[principal_id, revoked_at] <- [[$pid, $revoked]] \
             :update human_principals { principal_id => revoked_at }",
            p,
        )
        .map_err(|e| CozoError::Other(format!("revoke human principal: {e}")))?;
        Ok(())
    }

    /// Update only the display-only alias of a human principal — a
    /// partial-column `:update`, so no other field can be clobbered.
    pub fn update_human_principal_alias(
        &self,
        principal_id: &str,
        display_alias: &str,
    ) -> std::result::Result<(), CozoError> {
        let mut p: Params = BTreeMap::new();
        p.insert("pid".into(), DataValue::Str(principal_id.into()));
        p.insert("alias".into(), DataValue::Str(display_alias.into()));
        cozo_run(
            &self.db,
            "?[principal_id, display_alias] <- [[$pid, $alias]] \
             :update human_principals { principal_id => display_alias }",
            p,
        )
        .map_err(|e| CozoError::Other(format!("update human principal alias: {e}")))?;
        Ok(())
    }

    /// Append one event to a session's snowball ledger, allocating the next
    /// `seq` inside a single write transaction (max+1 and the `:put` commit
    /// together, so two concurrent appends can't mint the same seq and
    /// silently overwrite each other). Returns the allocated seq.
    pub fn append_session_event(
        &self,
        session_id: &str,
        kind: &str,
        subid: &str,
        content: &str,
        created_at: i64,
    ) -> std::result::Result<i64, CozoError> {
        self.append_session_event_durable(session_id, kind, subid, content, created_at)
    }

    /// List a session's events, newest first; `limit` 0 = all.
    pub fn list_session_events(
        &self,
        session_id: &str,
        limit: usize,
    ) -> std::result::Result<Vec<SessionEventRecord>, CozoError> {
        let mut p: Params = BTreeMap::new();
        p.insert("sid".into(), DataValue::Str(session_id.into()));
        let script = if limit > 0 {
            format!(
                "?[session_id, seq, kind, subid, content, created_at] := \
                 *session_events[session_id, seq, kind, subid, content, created_at], \
                 session_id = $sid \
                 :order -seq :limit {limit}"
            )
        } else {
            "?[session_id, seq, kind, subid, content, created_at] := \
             *session_events[session_id, seq, kind, subid, content, created_at], \
             session_id = $sid \
             :order -seq"
                .to_string()
        };
        let r = cozo_run(&self.db, &script, p)
            .map_err(|e| CozoError::Other(format!("list session events: {e}")))?;
        Ok(r.rows
            .iter()
            .map(|row| SessionEventRecord {
                session_id: dv_as_str(&row[0]).unwrap_or("").to_string(),
                seq: dv_as_int(&row[1]),
                kind: dv_as_str(&row[2]).unwrap_or("").to_string(),
                subid: dv_as_str(&row[3]).unwrap_or("").to_string(),
                content: dv_as_str(&row[4]).unwrap_or("").to_string(),
                created_at: dv_as_int(&row[5]),
            })
            .collect())
    }

    /// Return a shared handle to the underlying DbInstance for advanced queries.
    pub fn db(&self) -> Arc<DbInstance> {
        self.db.clone()
    }
}

// ── Helpers ────────────────────────────────────────────────────────────────────

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn dv_int(i: i64) -> DataValue {
    DataValue::Num(cozo::Num::Int(i))
}

fn dv_as_text(dv: &DataValue) -> String {
    match dv {
        DataValue::Str(s) => s.as_str().to_string(),
        DataValue::Json(_) => {
            let rendered = format!("{dv}");
            rendered
                .strip_prefix("json(")
                .and_then(|inner| inner.strip_suffix(')'))
                .unwrap_or(rendered.as_str())
                .to_string()
        }
        DataValue::List(_) => match dv_to_json(dv) {
            Value::String(s) => s,
            Value::Null => String::new(),
            other => other.to_string(),
        },
        DataValue::Bytes(bytes) => String::from_utf8_lossy(bytes).into_owned(),
        DataValue::Null => String::new(),
        other => match dv_to_json(other) {
            Value::String(s) => s,
            Value::Null => String::new(),
            v => v.to_string(),
        },
    }
}

fn dv_as_str(dv: &DataValue) -> Option<&str> {
    if let DataValue::Str(s) = dv {
        Some(s.as_str())
    } else {
        None
    }
}

fn dv_as_int(dv: &DataValue) -> i64 {
    match dv {
        DataValue::Num(cozo::Num::Int(i)) => *i,
        DataValue::Num(cozo::Num::Float(f)) => *f as i64,
        _ => 0,
    }
}

fn dv_as_bool(dv: &DataValue) -> bool {
    matches!(dv, DataValue::Bool(true))
}

fn row_to_wg_session(row: &[DataValue]) -> WgSessionRecord {
    let s = |i: usize| dv_as_str(&row[i]).unwrap_or("").to_string();
    let opt = |i: usize| {
        let v = s(i);
        if v.is_empty() { None } else { Some(v) }
    };
    WgSessionRecord {
        session_id: s(0),
        peer_pubkey: s(1),
        psk: s(2),
        created_at: dv_as_int(&row[3]) as u64,
        expires_at: dv_as_int(&row[4]) as u64,
        is_active: dv_as_bool(&row[5]),
        last_used: dv_as_int(&row[6]) as u64,
        client_ip: opt(7),
        client_version: opt(8),
        auth_method: s(9),
        key_rotation_count: dv_as_int(&row[10]) as u32,
        flags_json: {
            let v = s(11);
            if v.is_empty() { "{}".to_string() } else { v }
        },
    }
}

fn row_cell<'a>(headers: &[String], row: &'a [DataValue], name: &str) -> Option<&'a DataValue> {
    headers
        .iter()
        .position(|header| header == name)
        .and_then(|idx| row.get(idx))
}

fn row_text(headers: &[String], row: &[DataValue], name: &str) -> String {
    row_cell(headers, row, name)
        .map(dv_as_text)
        .unwrap_or_default()
}

fn row_str(headers: &[String], row: &[DataValue], name: &str) -> String {
    row_cell(headers, row, name)
        .and_then(dv_as_str)
        .unwrap_or("")
        .to_string()
}

fn row_to_identity_sled(headers: &[String], row: &[DataValue]) -> IdentitySledRecord {
    IdentitySledRecord {
        session_id: row_str(headers, row, "session_id"),
        wireguard_pubkey: row_str(headers, row, "wireguard_pubkey"),
        interface: row_str(headers, row, "interface"),
        peer_ip: row_str(headers, row, "peer_ip"),
        mutation_index: row_cell(headers, row, "mutation_index")
            .map(dv_as_int)
            .unwrap_or(0),
        session_genesis: row_str(headers, row, "session_genesis"),
        trace_id: row_str(headers, row, "trace_id"),
        schema_version: row_cell(headers, row, "schema_version")
            .map(dv_as_int)
            .unwrap_or(0),
        vector_id: row_str(headers, row, "vector_id"),
        sealed_id: row_str(headers, row, "sealed_id"),
        principal_kind: {
            let value = row_str(headers, row, "principal_kind");
            (!value.is_empty()).then_some(value)
        },
        btrfs_device_json: row_text(headers, row, "btrfs_device_json"),
        instance_json: row_text(headers, row, "instance_json"),
        session_started_at: row_cell(headers, row, "session_started_at")
            .map(dv_as_int)
            .unwrap_or(0),
        last_seen_at: row_cell(headers, row, "last_seen_at")
            .map(dv_as_int)
            .unwrap_or(0),
        active: row_cell(headers, row, "active")
            .map(dv_as_bool)
            .unwrap_or(false),
        expires_at: row_cell(headers, row, "expires_at")
            .map(dv_as_int)
            .unwrap_or(0),
    }
}

fn row_to_genesis_inputs(row: &[DataValue]) -> GenesisInputsRecord {
    let s = |i: usize| dv_as_str(&row[i]).unwrap_or("").to_string();
    GenesisInputsRecord {
        session_id: s(0),
        arrival_timestamp: dv_as_int(&row[1]),
        chain_head_at_arrival: s(2),
        catalog_hash_at_arrival: s(3),
        head_timestamp_at_arrival: dv_as_int(&row[4]),
        schema_content_hash: s(5),
    }
}

fn row_to_human_principal(row: &[DataValue]) -> HumanPrincipalRecord {
    let s = |i: usize| dv_as_str(&row[i]).unwrap_or("").to_string();
    HumanPrincipalRecord {
        principal_id: s(0),
        human_pubkey: s(1),
        display_alias: s(2),
        registered_at: dv_as_int(&row[3]),
        revoked_at: dv_as_int(&row[4]),
    }
}

fn json_obj_to_params(v: Value) -> Params {
    let mut map: Params = BTreeMap::new();
    if let Value::Object(obj) = v {
        for (k, val) in obj {
            map.insert(k, json_to_dv(val));
        }
    }
    map
}

fn json_to_dv(v: Value) -> DataValue {
    match v {
        Value::Null => DataValue::Null,
        Value::Bool(b) => DataValue::Bool(b),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                dv_int(i)
            } else {
                DataValue::Num(cozo::Num::Float(n.as_f64().unwrap_or(0.0)))
            }
        }
        Value::String(s) => DataValue::Str(s.into()),
        Value::Array(arr) => DataValue::List(arr.into_iter().map(json_to_dv).collect()),
        Value::Object(_) => DataValue::Str(v.to_string().into()),
    }
}

pub fn named_rows_to_json(rows: NamedRows) -> Value {
    let headers = &rows.headers;
    let out: Vec<Value> = rows
        .rows
        .iter()
        .map(|row| {
            let obj: serde_json::Map<String, Value> = headers
                .iter()
                .zip(row.iter())
                .map(|(h, dv)| (h.clone(), dv_to_json(dv)))
                .collect();
            Value::Object(obj)
        })
        .collect();
    Value::Array(out)
}

fn dv_to_json(dv: &DataValue) -> Value {
    match dv {
        DataValue::Null => Value::Null,
        DataValue::Bool(b) => Value::Bool(*b),
        DataValue::Num(cozo::Num::Int(i)) => Value::Number((*i).into()),
        DataValue::Num(cozo::Num::Float(f)) => serde_json::Number::from_f64(*f)
            .map(Value::Number)
            .unwrap_or(Value::Null),
        DataValue::Str(s) => Value::String(s.to_string()),
        DataValue::List(list) => Value::Array(list.iter().map(dv_to_json).collect()),
        DataValue::Json(_) => {
            let rendered = format!("{dv}");
            let trimmed = rendered
                .strip_prefix("json(")
                .and_then(|inner| inner.strip_suffix(')'))
                .unwrap_or(rendered.as_str());
            serde_json::from_str(trimmed).unwrap_or(Value::String(rendered))
        }
        other => Value::String(format!("{other}")),
    }
}

#[cfg(test)]
mod identity_sled_tests {
    use super::*;

    const PRE_SEALED_ID_SCHEMA: &str = r#":create identity_sleds {
        session_id: String => wireguard_pubkey: String,
        interface: String default "", peer_ip: String default "",
        mutation_index: Int default 0, hashed_footprint: String default "",
        trace_id: String default "", schema_version: Int default 0,
        vector_id: String default "", blob_ref: String default "",
        btrfs_device_json: String default "", instance_json: String default "",
        session_started_at: Int default 0, last_seen_at: Int default 0,
        active: Bool default false
    }"#;

    fn unseeded_store() -> CozoGraphShuttle {
        CozoGraphShuttle {
            db: Arc::new(DbInstance::new("mem", "", Default::default()).unwrap()),
            identity_directory: None,
            identity_transaction: Arc::new(std::sync::Mutex::new(())),
            runtime_document_transaction: Arc::new(std::sync::Mutex::new(())),
            episode_transaction: Arc::new(std::sync::Mutex::new(())),
            system_graph_transaction: Arc::new(std::sync::Mutex::new(())),
        }
    }

    fn migration_marked(store: &CozoGraphShuttle) -> bool {
        !cozo_run(
            &store.db,
            "?[name] := *schema_migrations{name, completed}, name = 'identity-sessions-v3', completed = true",
            BTreeMap::new(),
        )
        .unwrap()
        .rows
        .is_empty()
    }

    fn sample(session_id: &str) -> IdentitySledRecord {
        IdentitySledRecord {
            session_id: session_id.to_string(),
            wireguard_pubkey: "pubkey-A".to_string(),
            interface: "".to_string(),
            peer_ip: "10.0.0.2".to_string(),
            mutation_index: 3,
            session_genesis: "fp".to_string(),
            trace_id: "tr".to_string(),
            schema_version: 1,
            vector_id: "".to_string(),
            sealed_id: "sid1:abc".to_string(),
            principal_kind: None,
            btrfs_device_json: r#"{"device_path":"/dev/loop9","mount_point":"/mnt/x","btrfs_uuid":"","cozo_id":"","attached":false}"#.to_string(),
            instance_json: r#"{"name":"sid-1","status":"Stopped","type":"container"}"#.to_string(),
            session_started_at: 100,
            last_seen_at: 200,
            active: true,
            expires_at: 0,
        }
    }

    #[test]
    fn empty_legacy_identity_relation_does_not_require_current_column_width() {
        let store = unseeded_store();
        cozo_run(&store.db, PRE_SEALED_ID_SCHEMA, BTreeMap::new()).unwrap();
        store.seed_schema().unwrap();
        assert!(migration_marked(&store));
        assert!(store.list_identity_sessions().unwrap().is_empty());
    }

    #[test]
    fn populated_pre_sealed_id_identity_relation_fails_without_migration_marker() {
        let store = unseeded_store();
        cozo_run(&store.db, PRE_SEALED_ID_SCHEMA, BTreeMap::new()).unwrap();
        cozo_run(
            &store.db,
            "?[session_id, wireguard_pubkey, hashed_footprint, instance_json] <- [['s', 'wg', 'genesis', '{\"old\":true}']] :put identity_sleds {session_id => wireguard_pubkey, hashed_footprint, instance_json}",
            BTreeMap::new(),
        )
        .unwrap();
        let err = store.seed_schema().unwrap_err().to_string();
        assert!(err.contains("sealed_id"), "{err}");
        assert!(!migration_marked(&store));
        assert!(store.list_identity_sessions().unwrap().is_empty());
    }

    #[test]
    fn wider_sealed_legacy_identity_rows_migrate_by_name_without_replay() {
        let store = unseeded_store();
        let schema = PRE_SEALED_ID_SCHEMA.replace(
            "blob_ref: String default \"\"",
            "sealed_id: String default \"\", expires_at: Int default 0, extra_a: String default \"\"",
        );
        cozo_run(&store.db, &schema, BTreeMap::new()).unwrap();
        cozo_run(
            &store.db,
            "?[session_id, wireguard_pubkey, hashed_footprint, sealed_id, instance_json, expires_at] <- [['s', 'wg', 'genesis', 'sid1:abc', '{\"old\":true}', 1234567890]] :put identity_sleds {session_id => wireguard_pubkey, hashed_footprint, sealed_id, instance_json, expires_at}",
            BTreeMap::new(),
        )
        .unwrap();
        store.seed_schema().unwrap();
        assert!(migration_marked(&store));
        let mut row = store.get_identity_sled("s").unwrap().unwrap();
        assert_eq!(row.wireguard_pubkey, "wg");
        assert_eq!(row.session_genesis, "genesis");
        assert_eq!(row.sealed_id, "sid1:abc");
        assert_eq!(row.expires_at, 1234567890);
        assert_eq!(row.instance_json, "{\"old\":true}");

        row.instance_json = "{\"updated\":true}".into();
        store.put_identity_sled(&row).unwrap();
        store.seed_schema().unwrap();
        assert_eq!(
            store.get_identity_sled("s").unwrap().unwrap().instance_json,
            "{\"updated\":true}"
        );
    }

    #[test]
    fn malformed_populated_legacy_identity_relation_never_marks_migration_complete() {
        let store = unseeded_store();
        cozo_run(
            &store.db,
            ":create identity_sleds { session_id: String => wireguard_pubkey: String }",
            BTreeMap::new(),
        )
        .unwrap();
        cozo_run(
            &store.db,
            "?[session_id, wireguard_pubkey] <- [['s', 'wg']] :put identity_sleds {session_id => wireguard_pubkey}",
            BTreeMap::new(),
        )
        .unwrap();
        assert!(store.seed_schema().is_err());
        assert!(!migration_marked(&store));
        assert!(store.list_identity_sessions().unwrap().is_empty());
    }

    #[test]
    fn identity_sled_round_trip() {
        let store = CozoGraphShuttle::new_in_memory().unwrap();
        let rec = sample("sid-1");
        store.put_identity_sled(&rec).unwrap();

        let got = store.get_identity_sled("sid-1").unwrap().unwrap();
        assert_eq!(got.wireguard_pubkey, "pubkey-A");
        assert_eq!(got.instance_json, rec.instance_json);
        assert_eq!(got.btrfs_device_json, rec.btrfs_device_json);
        assert_eq!(got.mutation_index, 3);
        assert!(got.active);

        store.touch_identity_sled("sid-1", 999, false).unwrap();
        let touched = store.get_identity_sled("sid-1").unwrap().unwrap();
        assert_eq!(touched.last_seen_at, 999);
        assert!(!touched.active);
        // Partial-column update must not clobber the rest of the row.
        assert_eq!(touched.instance_json, rec.instance_json);

        assert_eq!(store.list_identity_sessions().unwrap().len(), 1);
        assert!(store.get_identity_sled("nope").unwrap().is_none());
    }

    #[test]
    fn identity_sled_persists_through_rocksdb_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("identity-rocksdb");
        {
            let store = CozoGraphShuttle::new_persistent(db_path.clone()).unwrap();
            store.put_identity_sled(&sample("chatbot-first")).unwrap();
        }
        let reopened = CozoGraphShuttle::new_persistent(db_path).unwrap();
        let row = reopened
            .get_identity_sled("chatbot-first")
            .unwrap()
            .unwrap();
        assert_eq!(row.wireguard_pubkey, "pubkey-A");
        assert_eq!(
            row.btrfs_device_json,
            sample("chatbot-first").btrfs_device_json
        );
        assert_eq!(row.instance_json, sample("chatbot-first").instance_json);
    }

    #[test]
    fn identity_sled_reopen_keeps_overlay_sized_instance_json() {
        let overlay = r#"{"name":"bea37ecb-92be-197c-660f-09e806f1a34f","status":"Stopped","type":"container","profiles":["identity"],"storage_pool":"3tched-storage","config":{"boot.autostart":"false","user.opdbus.role":"chatbot"},"devices":[{"name":"ghostbridge-socket","device":{"type":"disk","path":"/opt/run-mounts/ghostbridge","source":"/run/ghostbridge"}},{"name":"identity","device":{"type":"disk","path":"/opt/run-mounts/identity","source":"/var/lib/opdbus-runtime/identities/chatbot","readonly":"true"}},{"name":"persist","device":{"type":"disk","path":"/opt/run-mounts/persist","source":"/var/lib/opdbus-runtime/identities/chatbot/persist"}},{"name":"root","device":{"type":"disk","path":"/","pool":"3tched-storage"}}]}"#;
        let btrfs = r#"{"device_path":"/var/lib/opdbus-runtime/identities/chatbot/fstorage.img","mount_point":"/var/lib/opdbus-runtime/identities/chatbot/persist","btrfs_uuid":"cb63f57a-194e-4547-93cb-31bb8b8da50c","cozo_id":"cb63f57a-194e-4547-93cb-31bb8b8da50c","attached":true}"#;
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("identity-rocksdb");
        let mut rec = sample("bea37ecb-92be-197c-660f-09e806f1a34f");
        rec.instance_json = overlay.to_string();
        rec.btrfs_device_json = btrfs.to_string();
        {
            let store = CozoGraphShuttle::new_persistent(db_path.clone()).unwrap();
            store.put_identity_sled(&rec).unwrap();
        }
        let reopened = CozoGraphShuttle::new_persistent(db_path.clone()).unwrap();
        let row = reopened
            .get_identity_sled(&rec.session_id)
            .unwrap()
            .unwrap();
        assert_eq!(row.instance_json, overlay);
        assert_eq!(row.btrfs_device_json, btrfs);
        let parsed: serde_json::Value = serde_json::from_str(&row.instance_json).unwrap();
        assert_eq!(parsed["devices"].as_array().unwrap().len(), 4);
    }

    #[test]
    fn identity_sessions_survive_reopen_when_legacy_identity_sleds_exist() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("identity-rocksdb");
        let mut rec = sample("bea37ecb-92be-197c-660f-09e806f1a34f");
        rec.instance_json = r#"{"name":"bea37ecb-92be-197c-660f-09e806f1a34f","status":"Stopped","type":"container"}"#.to_string();
        rec.btrfs_device_json = r#"{"device_path":"/dev/loop0","attached":true}"#.to_string();
        {
            let store = CozoGraphShuttle::new_persistent(db_path.clone()).unwrap();
            store.put_identity_sled(&rec).unwrap();
            let _ = store.run_query(
                r#":create identity_sleds {
                    session_id: String
                    =>
                    wireguard_pubkey: String,
                    interface: String default "",
                    peer_ip: String default "",
                    mutation_index: Int default 0,
                    hashed_footprint: String default "",
                    trace_id: String default "",
                    schema_version: Int default 0,
                    vector_id: String default "",
                    sealed_id: String default "",
                    btrfs_device_json: String default "",
                    instance_json: String default "",
                    session_started_at: Int default 0,
                    last_seen_at: Int default 0,
                    active: Bool default false,
                    expires_at: Int default 0
                }"#,
                None,
            );
            let _ = store.run_query(
                r#"
                ?[session_id, wireguard_pubkey, interface, peer_ip, mutation_index,
                  hashed_footprint, trace_id, schema_version, vector_id, sealed_id,
                  btrfs_device_json, instance_json, session_started_at, last_seen_at,
                  active, expires_at] <- [[$sid, 'old', '', '', 0, 'legacy', '', 0, '', '',
                    '', '', 0, 0, false, 0]]
                :put identity_sleds {
                    session_id => wireguard_pubkey, interface, peer_ip, mutation_index,
                    hashed_footprint, trace_id, schema_version, vector_id, sealed_id,
                    btrfs_device_json, instance_json, session_started_at, last_seen_at,
                    active, expires_at
                }
                "#,
                Some(serde_json::json!({"sid": rec.session_id})),
            );
        }
        let reopened = CozoGraphShuttle::new_persistent(db_path.clone()).unwrap();
        let row = reopened
            .get_identity_sled(&rec.session_id)
            .unwrap()
            .unwrap();
        assert_eq!(row.instance_json, rec.instance_json);
        assert_eq!(row.btrfs_device_json, rec.btrfs_device_json);
        reopened.delete_identity_sled(&rec.session_id).unwrap();
        drop(reopened);
        let empty = CozoGraphShuttle::new_persistent(db_path).unwrap();
        assert!(
            empty.list_identity_sessions().unwrap().is_empty(),
            "an intentionally emptied catalog must not resurrect legacy ghost rows"
        );
    }

    #[test]
    fn account_session_round_trip() {
        let store = CozoGraphShuttle::new_in_memory().unwrap();
        store.upsert_user("wg-public-key").unwrap();
        store
            .upsert_account_session("wg-public-key", "stable-session", "session-proof")
            .unwrap();
        store
            .create_session("stable-session", "wg-public-key", None)
            .unwrap();

        assert!(store.user_exists("wg-public-key").unwrap());
        let (session_id, proof, _) = store
            .lookup_account_session("wg-public-key")
            .unwrap()
            .unwrap();
        assert_eq!(session_id, "stable-session");
        assert_eq!(proof, "session-proof");
        assert_eq!(
            store.lookup_session("stable-session").unwrap().unwrap().0,
            "wg-public-key"
        );
    }

    #[test]
    fn session_events_allocate_monotonic_seq() {
        let store = CozoGraphShuttle::new_in_memory().unwrap();
        assert_eq!(
            store
                .append_session_event("s", "arrival", "", "a", 1)
                .unwrap(),
            0
        );
        assert_eq!(
            store
                .append_session_event("s", "mutation", "", "b", 2)
                .unwrap(),
            1
        );
        assert_eq!(
            store
                .append_session_event("other", "arrival", "", "c", 3)
                .unwrap(),
            0
        );

        let newest_first = store.list_session_events("s", 0).unwrap();
        assert_eq!(newest_first.len(), 2);
        assert_eq!(newest_first[0].seq, 1);
        assert_eq!(newest_first[0].kind, "mutation");

        let limited = store.list_session_events("s", 1).unwrap();
        assert_eq!(limited.len(), 1);
        assert_eq!(limited[0].seq, 1);
    }
}

#[cfg(test)]
mod compliance_mutation_tests {
    use super::*;

    #[test]
    fn evaluate_mutation_allows_when_unseeded_or_empty() {
        let store = CozoGraphShuttle::new_in_memory().unwrap();
        let verdict = store.evaluate_mutation("openflow", "add_flow");
        assert!(verdict.allow);
    }

    #[test]
    fn evaluate_mutation_denies_matching_rule() {
        let store = CozoGraphShuttle::new_in_memory().unwrap();
        store
            .store_compliance_rule(
                "openflow",
                "add_flow",
                "Deny",
                "tag routing incomplete",
                "OD-06",
            )
            .expect("compliance_rule relation must accept Deny rows");
        let denied = store.evaluate_mutation("openflow", "add_flow");
        assert!(!denied.allow, "reason={}", denied.reason);
        assert!(denied.reason.contains("tag routing"));
        let other = store.evaluate_mutation("openflow", "dump_flows");
        assert!(other.allow);
    }
}

#[cfg(test)]
mod human_principal_tests {
    use super::*;

    fn sample(principal_id: &str, pubkey: &str) -> HumanPrincipalRecord {
        HumanPrincipalRecord {
            principal_id: principal_id.to_string(),
            human_pubkey: pubkey.to_string(),
            display_alias: String::new(),
            registered_at: 1_700_000_000,
            revoked_at: 0,
        }
    }

    #[test]
    fn human_principal_round_trip() {
        let store = CozoGraphShuttle::new_in_memory().unwrap();
        let mut rec = sample("pid-1", "pubkey-A");
        rec.display_alias = "alice".to_string();
        store.put_human_principal(&rec).unwrap();

        // Fetch by derived principal id and by unique pubkey: full equality.
        let got = store.get_human_principal("pid-1").unwrap().unwrap();
        assert_eq!(got, rec);
        let by_key = store
            .get_human_principal_by_pubkey("pubkey-A")
            .unwrap()
            .unwrap();
        assert_eq!(by_key, rec);

        // Unknown lookups are absent, never fabricated.
        assert!(store.get_human_principal("nope").unwrap().is_none());
        assert!(
            store
                .get_human_principal_by_pubkey("nope")
                .unwrap()
                .is_none()
        );

        // Alias update is a partial-column update: nothing else is clobbered.
        store
            .update_human_principal_alias("pid-1", "alice2")
            .unwrap();
        let aliased = store.get_human_principal("pid-1").unwrap().unwrap();
        assert_eq!(aliased.display_alias, "alice2");
        assert_eq!(aliased.human_pubkey, rec.human_pubkey);
        assert_eq!(aliased.registered_at, rec.registered_at);
        assert_eq!(aliased.revoked_at, 0);

        // Revocation is a tombstone: the row stays resolvable by id AND key.
        store
            .revoke_human_principal("pid-1", 1_700_000_999)
            .unwrap();
        let revoked = store.get_human_principal("pid-1").unwrap().unwrap();
        assert_eq!(revoked.revoked_at, 1_700_000_999);
        assert_eq!(revoked.display_alias, "alice2");
        let revoked_by_key = store
            .get_human_principal_by_pubkey("pubkey-A")
            .unwrap()
            .unwrap();
        assert_eq!(revoked_by_key.revoked_at, 1_700_000_999);

        assert_eq!(store.list_human_principals().unwrap().len(), 1);
    }

    /// Reopen a persistent shuttle, tolerating cozo 0.7.6's deferred close:
    /// `multi_transaction` clones the DbInstance into a rayon worker that can
    /// outlive the shuttle drop by a few ms, so the RocksDB LOCK release lags.
    /// The durability contract being tested is unaffected — this only waits
    /// for the engine to finish closing.
    fn reopen_persistent(path: &std::path::Path) -> CozoGraphShuttle {
        let mut last_err = None;
        for _ in 0..100 {
            match CozoGraphShuttle::new_persistent(path.to_path_buf()) {
                Ok(store) => return store,
                Err(e) => {
                    last_err = Some(e);
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
            }
        }
        panic!("reopen never acquired the RocksDB lock: {last_err:?}");
    }

    #[test]
    fn human_principal_persists_through_rocksdb_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("human-principal-rocksdb");
        let mut active = sample("pid-active", "pubkey-active");
        active.display_alias = "alice".to_string();
        let mut revoked = sample("pid-revoked", "pubkey-revoked");
        revoked.display_alias = "bob".to_string();
        revoked.revoked_at = 1_700_000_111;
        {
            let store = CozoGraphShuttle::new_persistent(db_path.clone()).unwrap();
            store.put_human_principal(&active).unwrap();
            store.put_human_principal(&revoked).unwrap();
        }
        // Records, aliases, and revoked_at markers all survive the reopen.
        let reopened = reopen_persistent(&db_path);
        assert_eq!(
            reopened.get_human_principal("pid-active").unwrap().unwrap(),
            active
        );
        assert_eq!(
            reopened
                .get_human_principal("pid-revoked")
                .unwrap()
                .unwrap(),
            revoked
        );
        assert_eq!(
            reopened
                .get_human_principal_by_pubkey("pubkey-revoked")
                .unwrap()
                .unwrap(),
            revoked
        );
        let mut listed = reopened.list_human_principals().unwrap();
        listed.sort_by(|a, b| a.principal_id.cmp(&b.principal_id));
        assert_eq!(listed, vec![active, revoked]);
    }

    #[test]
    fn human_principal_db_path_env_override() {
        // Unset (no other test in this binary touches the var) → default.
        assert_eq!(
            human_principal_cozo_db_path(),
            PathBuf::from(DEFAULT_HUMAN_PRINCIPAL_COZO_DB_PATH)
        );
        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::set_var("OP_HUMAN_PRINCIPAL_COZO_DB_PATH", "/tmp/hp-override") };
        assert_eq!(
            human_principal_cozo_db_path(),
            PathBuf::from("/tmp/hp-override")
        );
        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::remove_var("OP_HUMAN_PRINCIPAL_COZO_DB_PATH") };
        assert_eq!(
            human_principal_cozo_db_path(),
            PathBuf::from(DEFAULT_HUMAN_PRINCIPAL_COZO_DB_PATH)
        );
    }
}
