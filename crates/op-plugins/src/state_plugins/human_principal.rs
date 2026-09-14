//! human_principal — the registry of human principals (humans ≠ containers).
//!
//! A human principal is a WireGuard pubkey registered as belonging to a human
//! operator (the Oracle decoy flow authenticates humans; this registry is what
//! the bridge resolves those pubkeys against). The `principal_id` is DERIVED
//! from the pubkey via `op_identity::session::derive_principal_id` (blake3
//! derive_key, context "op-identity human-principal v1") — never
//! caller-supplied — so a principal id can never collide with a container
//! session id (distinct derivation context) and cannot be forged by a caller.
//! `display_alias` is display-only: no method resolves by alias, so alias
//! substitution cannot impersonate a key.
//!
//! Records persist to the Cozo `human_principals` / `human_principal_pubkeys`
//! relations (op-cozo-store); every call is notarized in the immutable event
//! chain by MutationEngine. Method dispatch lives in op-grpc-bridge's
//! `human_principal_dispatch` module — this plugin is the schema of record.

use anyhow::Result;
use async_trait::async_trait;
use op_state::{ApplyResult, Checkpoint, DiffMetadata, PluginCapabilities, StateDiff, StatePlugin};
use op_state_store::PluginSchema;
use serde::{Deserialize, Serialize};
use simd_json::OwnedValue as Value;

/// One registered human principal.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.service.human-principal.record.schema@v1"))]
pub struct HumanPrincipal {
    /// Derived principal id (blake3 derive over the pubkey under the
    /// "op-identity human-principal v1" context). Never caller-supplied.
    #[schemars(extend("x-oscal-subid" = "obs.service.human-principal.principal-id@v1"))]
    pub principal_id: String,
    /// WireGuard public key (base64, decodes to 32 bytes) — unique across all
    /// principals, active or revoked; the cryptographic root of the identity.
    #[schemars(extend("x-oscal-subid" = "src.service.human-principal.wireguard-pubkey@v1"))]
    pub human_pubkey: String,
    /// Display-only alias. NEVER authoritative: nothing resolves by alias.
    #[serde(default)]
    pub display_alias: String,
    /// Unix seconds when the key was registered.
    pub registered_at: i64,
    /// Unix seconds when the key was revoked; `None` while active. Revocation
    /// is a permanent tombstone — a revoked key can never be re-registered.
    #[serde(default)]
    pub revoked_at: Option<i64>,
}

/// Runtime state: every registered human principal, active and revoked.
#[derive(Debug, Clone, Serialize, Deserialize, Default, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.service.plugin.human-principal.schema@v1"))]
#[schemars(extend("x-oscal-category" = "service"))]
pub struct HumanPrincipalState {
    /// All known human principals (active and revoked tombstones), sorted by
    /// principal_id.
    #[schemars(extend("x-oscal-subid" = "obs.service.human-principal.principals@v1"))]
    pub principals: Vec<HumanPrincipal>,
}

pub struct HumanPrincipalPlugin;

impl HumanPrincipalPlugin {
    pub fn new() -> Self {
        Self
    }

    /// This plugin's present state, read from its own sealed blob in the SHM
    /// catalog (the blob IS the plugin).
    fn read_state() -> HumanPrincipalState {
        let Some(schema) = op_blob::catalog::read_plugin_schema_shm("human_principal") else {
            return HumanPrincipalState::default();
        };
        let Some(state) = schema.example else {
            return HumanPrincipalState::default();
        };
        simd_json::serde::from_owned_value::<HumanPrincipalState>(state).unwrap_or_default()
    }
}

impl Default for HumanPrincipalPlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl StatePlugin for HumanPrincipalPlugin {
    fn name(&self) -> &str {
        "human_principal"
    }

    fn version(&self) -> &str {
        "1.0.0"
    }

    fn schema(&self) -> Option<PluginSchema> {
        let mut schema = super::plugin_scaffold_helpers::human_principal_plugin_schema();
        super::common::oscal::ensure_category_metadata_fields(&mut schema);
        Some(schema)
    }

    async fn calculate_diff(&self, _current: &Value, _desired: &Value) -> Result<StateDiff> {
        Ok(StateDiff {
            plugin: self.name().to_string(),
            actions: vec![],
            metadata: DiffMetadata {
                timestamp: chrono::Utc::now().timestamp(),
                current_hash: "schema".to_string(),
                desired_hash: "schema".to_string(),
            },
        })
    }

    async fn apply_state(&self, _diff: &StateDiff) -> Result<ApplyResult> {
        // Mutations go through the human_principal dispatcher (MutationEngine);
        // reconciliation has nothing to converge here.
        Ok(ApplyResult {
            success: true,
            changes_applied: vec![],
            errors: vec![],
            checkpoint: None,
        })
    }

    async fn verify_state(&self, _desired: &Value) -> Result<bool> {
        Ok(true)
    }

    async fn create_checkpoint(&self) -> Result<Checkpoint> {
        Ok(Checkpoint {
            id: uuid::Uuid::new_v4().to_string(),
            plugin: self.name().to_string(),
            timestamp: chrono::Utc::now().timestamp(),
            state_snapshot: simd_json::serde::to_owned_value(Self::read_state())?,
            backend_checkpoint: None,
        })
    }

    async fn rollback(&self, _checkpoint: &Checkpoint) -> Result<()> {
        Ok(())
    }

    fn capabilities(&self) -> PluginCapabilities {
        PluginCapabilities {
            supports_rollback: false,
            supports_checkpoints: true,
            supports_verification: true,
            atomic_operations: false,
        }
    }
}

/// Canonical `human_principal` schema derived from [`HumanPrincipalState`] via
/// schemars, plus the callable method surface: exactly the six contract
/// methods (register_key / revoke_key / set_alias as Mutations,
/// resolve_key / get_principal / list_principals as Queries).
pub(crate) fn human_principal_schema() -> PluginSchema {
    let root = serde_json::to_value(schemars::schema_for!(HumanPrincipalState))
        .expect("schemars schema serializes to JSON");
    let mut schema = super::schemars_adapter::plugin_schema_from_json(
        "human_principal",
        "1.0.0",
        "Registry of human principals (humans ≠ containers): principal_id derived from the WireGuard pubkey, alias display-only, revocation a permanent tombstone",
        &root,
    );

    // Typed method inputs / outputs.
    #[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
    pub struct RegisterKeyInput {
        /// WireGuard public key (base64, must decode to 32 bytes). The
        /// principal_id is DERIVED from this — never supplied — so a forged
        /// id in extra arguments is never honored.
        pub human_pubkey: String,
        /// Display-only alias (optional; must be unique among ACTIVE
        /// principals when non-empty).
        #[serde(default)]
        pub display_alias: String,
    }
    #[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
    pub struct RegisterKeyOutput {
        pub principal: HumanPrincipal,
    }
    #[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
    pub struct RevokeKeyInput {
        /// WireGuard public key (base64) to revoke. Idempotent on an
        /// already-revoked key (the original `revoked_at` is preserved);
        /// an unknown key is an error.
        pub human_pubkey: String,
    }
    #[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
    pub struct RevokeKeyOutput {
        pub principal: HumanPrincipal,
    }
    #[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
    pub struct SetAliasInput {
        /// Principal to retitle (by derived id, never by alias).
        pub principal_id: String,
        /// New display-only alias; empty clears it.
        #[serde(default)]
        pub display_alias: String,
    }
    #[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
    pub struct SetAliasOutput {
        pub principal: HumanPrincipal,
    }
    #[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
    pub struct ResolveKeyInput {
        /// WireGuard public key (base64) to resolve. Aliases are never
        /// accepted here — alias is display-only data.
        pub human_pubkey: String,
    }
    #[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
    pub struct ResolveKeyOutput {
        /// The principal record, or null when the key is unknown. Revoked
        /// principals resolve with `revoked_at` set (visibility — never as
        /// active).
        pub principal: Option<HumanPrincipal>,
    }
    #[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
    pub struct GetPrincipalInput {
        /// Derived principal id to fetch.
        pub principal_id: String,
    }
    #[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
    pub struct GetPrincipalOutput {
        /// The principal record, or null when the id is unknown.
        pub principal: Option<HumanPrincipal>,
    }
    #[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
    pub struct ListPrincipalsOutput {
        /// All principals, revoked tombstones included.
        pub principals: Vec<HumanPrincipal>,
    }

    use super::plugin_scaffold_helpers::method_decl_from_schemars_with_output;
    use super::plugin_scaffold_helpers::EmptyInput;
    use op_state_store::SideEffect;

    schema.methods.insert(
        "register_key".to_string(),
        method_decl_from_schemars_with_output::<RegisterKeyInput, RegisterKeyOutput>(
            "register_key",
            SideEffect::Mutation,
            false,
            "human_principal.write",
            "mut.service.human-principal.key.register@v1",
        ),
    );
    schema.methods.insert(
        "revoke_key".to_string(),
        method_decl_from_schemars_with_output::<RevokeKeyInput, RevokeKeyOutput>(
            "revoke_key",
            SideEffect::Mutation,
            true,
            "human_principal.write",
            "mut.service.human-principal.key.revoke@v1",
        ),
    );
    schema.methods.insert(
        "set_alias".to_string(),
        method_decl_from_schemars_with_output::<SetAliasInput, SetAliasOutput>(
            "set_alias",
            SideEffect::Mutation,
            true,
            "human_principal.write",
            "mut.service.human-principal.alias.set@v1",
        ),
    );
    schema.methods.insert(
        "resolve_key".to_string(),
        method_decl_from_schemars_with_output::<ResolveKeyInput, ResolveKeyOutput>(
            "resolve_key",
            SideEffect::Read,
            true,
            "human_principal.read",
            "obs.service.human-principal.key.resolve@v1",
        ),
    );
    schema.methods.insert(
        "get_principal".to_string(),
        method_decl_from_schemars_with_output::<GetPrincipalInput, GetPrincipalOutput>(
            "get_principal",
            SideEffect::Read,
            true,
            "human_principal.read",
            "obs.service.human-principal.get@v1",
        ),
    );
    schema.methods.insert(
        "list_principals".to_string(),
        method_decl_from_schemars_with_output::<EmptyInput, ListPrincipalsOutput>(
            "list_principals",
            SideEffect::Read,
            true,
            "human_principal.read",
            "obs.service.human-principal.list@v1",
        ),
    );

    // Method subids are ALSO collected into schema.subids keyed by method
    // name (the unix_socket pattern) so the registry-wide
    // all_plugin_subids_are_valid_and_unique gate collects them. The
    // MethodDecl stays the single source of truth; this is the derived copy.
    for (method, decl) in &schema.methods {
        schema.subids.insert(method.clone(), decl.subid.clone());
    }

    // The mut.* method subids oblige actor_id/capability_id metadata fields;
    // declaring the subids is enough — the fields cannot be forgotten.
    super::common::oscal::ensure_category_metadata_fields(&mut schema);

    schema
}

#[cfg(test)]
mod tests {
    use super::human_principal_schema;
    use op_state_store::SideEffect;

    /// The exact six contract methods — no more, no less (VAL-REGISTRY-001).
    const CONTRACT_METHODS: [&str; 6] = [
        "register_key",
        "revoke_key",
        "set_alias",
        "resolve_key",
        "get_principal",
        "list_principals",
    ];

    #[test]
    fn schema_has_method_surface() {
        let schema = human_principal_schema();
        assert_eq!(
            schema.methods.len(),
            6,
            "exactly the six contract methods, got {:?}",
            schema.methods.keys().collect::<Vec<_>>()
        );
        for method in CONTRACT_METHODS {
            assert!(schema.methods.contains_key(method), "missing {method}");
        }
        // No alias-based lookup method anywhere in the surface: alias is
        // display-only data, never a resolution key.
        for banned in [
            "resolve_alias",
            "get_by_alias",
            "lookup_alias",
            "resolve_by_alias",
            "get_principal_by_alias",
        ] {
            assert!(
                !schema.methods.contains_key(banned),
                "alias-lookup method {banned} must not exist"
            );
        }
    }

    #[test]
    fn method_side_effects_and_capabilities() {
        let schema = human_principal_schema();
        for method in ["register_key", "revoke_key", "set_alias"] {
            let decl = &schema.methods[method];
            assert_eq!(decl.side_effect, SideEffect::Mutation, "{method}");
            assert_eq!(
                decl.required_capability.as_deref(),
                Some("human_principal.write"),
                "{method}"
            );
        }
        for method in ["resolve_key", "get_principal", "list_principals"] {
            let decl = &schema.methods[method];
            assert_eq!(decl.side_effect, SideEffect::Read, "{method}");
            assert_eq!(
                decl.required_capability.as_deref(),
                Some("human_principal.read"),
                "{method}"
            );
        }
    }

    #[test]
    fn human_principal_subids() {
        let schema = human_principal_schema();
        let expected = [
            (
                "register_key",
                "mut.service.human-principal.key.register@v1",
            ),
            ("revoke_key", "mut.service.human-principal.key.revoke@v1"),
            ("set_alias", "mut.service.human-principal.alias.set@v1"),
            ("resolve_key", "obs.service.human-principal.key.resolve@v1"),
            ("get_principal", "obs.service.human-principal.get@v1"),
            ("list_principals", "obs.service.human-principal.list@v1"),
        ];
        for (method, subid) in expected {
            // Declared on the method itself...
            assert_eq!(schema.methods[method].subid, subid, "method {method}");
            // ...and present in the collected registry set under the method
            // key (what all_plugin_subids_are_valid_and_unique collects).
            assert_eq!(
                schema.subids.get(method).map(String::as_str),
                Some(subid),
                "collected subid for {method}"
            );
            crate::state_plugins::common::oscal::validate_subid(subid)
                .unwrap_or_else(|e| panic!("invalid subid {subid}: {e}"));
        }
    }

    #[test]
    fn all_subids_are_valid() {
        let raw = serde_json::to_value(schemars::schema_for!(super::HumanPrincipalState)).unwrap();
        let mut subids = Vec::new();
        collect_subids(&raw, &mut subids);
        assert!(!subids.is_empty());
        for subid in subids {
            assert!(
                crate::state_plugins::common::oscal::validate_subid(&subid).is_ok(),
                "invalid subid: {subid}"
            );
        }
    }

    fn collect_subids(value: &serde_json::Value, out: &mut Vec<String>) {
        if let Some(obj) = value.as_object() {
            if let Some(subid) = obj.get("x-oscal-subid").and_then(|v| v.as_str()) {
                out.push(subid.to_string());
            }
            for v in obj.values() {
                collect_subids(v, out);
            }
        }
        if let Some(arr) = value.as_array() {
            for v in arr {
                collect_subids(v, out);
            }
        }
    }

    #[tokio::test]
    async fn human_principal_discoverable_through_inventory() {
        let store = std::sync::Arc::new(op_state_store::MemoryStore::new());
        let registry = crate::default_registry::DefaultPluginRegistry::new(store);

        let available = crate::default_registry::DefaultPluginRegistry::available_plugins();
        assert!(
            available.iter().any(|p| p == "human_principal"),
            "inventory must list human_principal, got {available:?}"
        );

        let plugin = registry.load_plugin("human_principal").await.unwrap();
        assert_eq!(plugin.name(), "human_principal");
        // The real six-method schema — NOT the auto-create review-draft
        // fallback an unknown plugin name would produce.
        let schema = plugin.schema().expect("human_principal schema");
        assert_eq!(schema.methods.len(), 6);
        for method in CONTRACT_METHODS {
            assert!(schema.methods.contains_key(method), "missing {method}");
        }
    }
}

// Self-registration: the plugin registry discovers this via inventory
// (single source of the catalog; no central dispatch list).
inventory::submit! {
    crate::default_registry::PluginReg::new("human_principal", |_ctx| std::sync::Arc::new(HumanPrincipalPlugin::new()))
}
