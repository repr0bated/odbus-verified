//! Caller-scoped runtime session metadata.
//!
//! This plugin never creates an identity or credential. The bridge supplies an
//! already verified principal and identity-authored session ID; declarations
//! only attach a display label to that exact pair in the shared Cozo runtime
//! document store.

use std::sync::Arc;

use anyhow::{Context, Result, bail, ensure};
use async_trait::async_trait;
use op_state::{ApplyResult, Checkpoint, DiffMetadata, PluginCapabilities, StateDiff, StatePlugin};
use op_state_store::{CapabilityDecl, PluginSchema};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use sha2::{Digest, Sha256};
use simd_json::OwnedValue as Value;

use super::runtime_documents::RuntimeDocuments;

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("x-oscal-subid" = "sch.software.plugin.sessdecl.schema@v2"))]
#[schemars(extend("x-oscal-category" = "software"))]
pub struct SessDeclState {
    #[serde(default)]
    #[schemars(
        description = "Caller-scoped display metadata for verified identity sessions",
        extend("default" = serde_json::json!([]), "x-oscal-subid" = "obs.software.plugin.sessdecl.sessions@v2")
    )]
    pub sessions: Vec<SessionDeclaration>,
}

/// Non-authoritative display metadata for an identity-authored session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SessionDeclaration {
    /// Exact session ID supplied by the verified invocation context.
    pub id: String,
    /// Exact stable principal ID supplied by the verified invocation context.
    pub user: String,
    /// Caller-selected display label. It has no authentication meaning.
    pub label: String,
    pub declared_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DeclareSessionInput {
    #[schemars(length(min = 1, max = 128))]
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SessionListScope {
    CurrentSession,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DeclareSessionOutput {
    pub declaration: SessionDeclaration,
    /// Whether this call created the metadata record. Repeating the same label
    /// for the same verified session is idempotent.
    pub created: bool,
    pub revision: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListSessionsOutput {
    pub scope: SessionListScope,
    pub sessions: Vec<SessionDeclaration>,
    pub revision: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessDeclAccessContext {
    pub principal_id: String,
    pub session_id: String,
}

impl SessDeclAccessContext {
    fn validate(&self) -> Result<()> {
        validate_identity_component("principal_id", &self.principal_id)?;
        validate_identity_component("session_id", &self.session_id)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SessionDocument {
    declaration: Option<SessionDeclaration>,
}

/// Long-lived adapter over the bridge's shared runtime-document handle.
pub struct SessDeclMethods {
    documents: Arc<RuntimeDocuments>,
}

impl SessDeclMethods {
    pub fn new(documents: Arc<RuntimeDocuments>) -> Self {
        Self { documents }
    }

    pub async fn dispatch(
        &self,
        context: &SessDeclAccessContext,
        method: &str,
        args: &JsonValue,
    ) -> Result<JsonValue> {
        context.validate()?;
        match method {
            "declare_session" => {
                let input: DeclareSessionInput = serde_json::from_value(args.clone())
                    .context("invalid declare_session arguments")?;
                self.declare_session(context, input).await
            }
            "list_sessions" => {
                validate_empty_input(args)?;
                self.list_sessions(context).await
            }
            _ => bail!("unsupported sess_decl method"),
        }
    }

    async fn declare_session(
        &self,
        context: &SessDeclAccessContext,
        input: DeclareSessionInput,
    ) -> Result<JsonValue> {
        validate_label(&input.label)?;
        let owner = owner_document_key(context);
        let principal_id = context.principal_id.clone();
        let session_id = context.session_id.clone();
        let label = input.label;
        let declared_at = chrono::Utc::now().to_rfc3339();
        let (row, (declaration, created)) = self
            .documents
            .update_with_result(&owner, move |old| {
                let mut document = parse_optional_document(old)?;
                if let Some(existing) = &document.declaration {
                    validate_stored_owner(existing, &principal_id, &session_id)?;
                    ensure!(
                        existing.label == label,
                        "session metadata is already declared with a different label"
                    );
                    return Ok((serde_json::to_value(&document)?, (existing.clone(), false)));
                }
                let declaration = SessionDeclaration {
                    id: session_id,
                    user: principal_id,
                    label,
                    declared_at,
                };
                document.declaration = Some(declaration.clone());
                Ok((serde_json::to_value(document)?, (declaration, true)))
            })
            .await?;
        Ok(serde_json::to_value(DeclareSessionOutput {
            declaration,
            created,
            revision: row.revision,
        })?)
    }

    async fn list_sessions(&self, context: &SessDeclAccessContext) -> Result<JsonValue> {
        let owner = owner_document_key(context);
        let Some(row) = self.documents.read(&owner).await? else {
            return Ok(serde_json::to_value(ListSessionsOutput {
                scope: SessionListScope::CurrentSession,
                sessions: Vec::new(),
                revision: 0,
            })?);
        };
        let document = parse_document(row.document)?;
        let sessions = match document.declaration {
            Some(declaration) => {
                validate_stored_owner(&declaration, &context.principal_id, &context.session_id)?;
                vec![declaration]
            }
            None => Vec::new(),
        };
        Ok(serde_json::to_value(ListSessionsOutput {
            scope: SessionListScope::CurrentSession,
            sessions,
            revision: row.revision,
        })?)
    }
}

fn parse_optional_document(value: Option<JsonValue>) -> Result<SessionDocument> {
    value
        .map(parse_document)
        .transpose()
        .map(Option::unwrap_or_default)
}

fn parse_document(value: JsonValue) -> Result<SessionDocument> {
    serde_json::from_value(value).context("invalid stored sess_decl document")
}

fn validate_identity_component(field: &str, value: &str) -> Result<()> {
    ensure!(!value.is_empty(), "verified {field} is required");
    ensure!(value.len() <= 512, "verified {field} is too long");
    ensure!(
        !value.chars().any(char::is_control),
        "verified {field} contains control characters"
    );
    Ok(())
}

fn validate_label(value: &str) -> Result<()> {
    ensure!(!value.trim().is_empty(), "session label is required");
    ensure!(value.len() <= 128, "session label is too long");
    ensure!(
        !value.chars().any(char::is_control),
        "session label contains control characters"
    );
    Ok(())
}

fn validate_stored_owner(
    declaration: &SessionDeclaration,
    principal_id: &str,
    session_id: &str,
) -> Result<()> {
    ensure!(
        declaration.user == principal_id && declaration.id == session_id,
        "stored session metadata owner binding is invalid"
    );
    Ok(())
}

fn validate_empty_input(value: &JsonValue) -> Result<()> {
    let object = value
        .as_object()
        .context("method input must be an object")?;
    ensure!(object.is_empty(), "method does not accept input fields");
    Ok(())
}

fn owner_document_key(context: &SessDeclAccessContext) -> String {
    let mut hash = Sha256::new();
    hash.update(b"op-dbus.sess-decl.owner.v2\0");
    hash.update(
        u64::try_from(context.principal_id.len())
            .expect("validated identity length fits u64")
            .to_be_bytes(),
    );
    hash.update(context.principal_id.as_bytes());
    hash.update(
        u64::try_from(context.session_id.len())
            .expect("validated identity length fits u64")
            .to_be_bytes(),
    );
    hash.update(context.session_id.as_bytes());
    format!("sess_decl:{:x}", hash.finalize())
}

pub struct SessDeclPlugin;

impl Default for SessDeclPlugin {
    fn default() -> Self {
        Self
    }
}

impl SessDeclPlugin {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl StatePlugin for SessDeclPlugin {
    fn name(&self) -> &str {
        "sess_decl"
    }

    fn version(&self) -> &str {
        "2.0.0"
    }

    fn schema(&self) -> Option<PluginSchema> {
        Some(sess_decl_schema())
    }

    async fn calculate_diff(&self, _current: &Value, _desired: &Value) -> Result<StateDiff> {
        Ok(StateDiff {
            plugin: self.name().to_string(),
            actions: vec![],
            metadata: DiffMetadata {
                timestamp: chrono::Utc::now().timestamp(),
                current_hash: "unknown".to_string(),
                desired_hash: "unknown".to_string(),
            },
        })
    }

    async fn apply_state(&self, _diff: &StateDiff) -> Result<ApplyResult> {
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
            state_snapshot: simd_json::json!(null),
            backend_checkpoint: None,
        })
    }

    async fn rollback(&self, _checkpoint: &Checkpoint) -> Result<()> {
        Ok(())
    }

    fn capabilities(&self) -> PluginCapabilities {
        PluginCapabilities {
            supports_rollback: false,
            supports_checkpoints: false,
            supports_verification: false,
            atomic_operations: false,
        }
    }
}

pub(crate) fn sess_decl_schema() -> PluginSchema {
    let root = serde_json::to_value(schemars::schema_for!(SessDeclState))
        .expect("schemars schema serializes to JSON");
    let mut schema = super::schemars_adapter::plugin_schema_from_json(
        "sess_decl",
        "2.0.0",
        "Display metadata bound to an already verified identity session; this plugin does not mint identity or grant authentication",
        &root,
    );

    use super::plugin_scaffold_helpers::{EmptyInput, method_decl_from_schemars_with_output};
    use op_state_store::SideEffect;

    schema.methods.insert(
        "list_sessions".to_string(),
        method_decl_from_schemars_with_output::<EmptyInput, ListSessionsOutput>(
            "list_sessions",
            SideEffect::Read,
            true,
            "sess_decl.read",
            "obs.software.plugin.sessdecl.sessions.list@v2",
        ),
    );
    schema.methods.insert(
        "declare_session".to_string(),
        method_decl_from_schemars_with_output::<DeclareSessionInput, DeclareSessionOutput>(
            "declare_session",
            SideEffect::Mutation,
            false,
            "sess_decl.write",
            "mut.software.plugin.sessdecl.session.declare@v2",
        ),
    );

    schema.capabilities.insert(
        "sess_decl.read".to_string(),
        CapabilityDecl {
            id: "sess_decl.read".to_string(),
            description: "Grants: list metadata for the caller's exact verified session."
                .to_string(),
        },
    );
    schema.capabilities.insert(
        "sess_decl.write".to_string(),
        CapabilityDecl {
            id: "sess_decl.write".to_string(),
            description: "Grants: attach display metadata to the caller's exact verified session. Does not create identity or authentication.".to_string(),
        },
    );
    schema.example = Some(simd_json::json!({"scope":"current_session","sessions":[]}));
    schema
}

#[cfg(test)]
mod tests {
    use super::*;
    use op_cozo_store::CozoGraphShuttle;
    use serde_json::json;

    fn context(principal: &str, session: &str) -> SessDeclAccessContext {
        SessDeclAccessContext {
            principal_id: principal.to_string(),
            session_id: session.to_string(),
        }
    }

    #[tokio::test]
    async fn declaration_uses_verified_owner_and_is_idempotent() {
        let service = SessDeclMethods::new(Arc::new(RuntimeDocuments::new(
            CozoGraphShuttle::new_in_memory().unwrap(),
        )));
        let owner = context("principal-a", "session-a");
        let first = service
            .dispatch(&owner, "declare_session", &json!({"label":"terminal"}))
            .await
            .unwrap();
        assert_eq!(first["declaration"]["id"], "session-a");
        assert_eq!(first["declaration"]["user"], "principal-a");
        assert_eq!(first["created"], true);
        assert!(first.get("sealed_id").is_none());

        let repeated = service
            .dispatch(&owner, "declare_session", &json!({"label":"terminal"}))
            .await
            .unwrap();
        assert_eq!(repeated["created"], false);
        assert!(
            service
                .dispatch(&owner, "declare_session", &json!({"label":"other"}))
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn listing_isolated_by_exact_principal_and_session() {
        let service = SessDeclMethods::new(Arc::new(RuntimeDocuments::new(
            CozoGraphShuttle::new_in_memory().unwrap(),
        )));
        let owner = context("principal-a", "session-a");
        service
            .dispatch(&owner, "declare_session", &json!({"label":"private"}))
            .await
            .unwrap();
        let own = service
            .dispatch(&owner, "list_sessions", &json!({}))
            .await
            .unwrap();
        assert_eq!(own["scope"], "current_session");
        assert_eq!(own["sessions"].as_array().unwrap().len(), 1);

        for other in [
            context("principal-a", "session-b"),
            context("principal-b", "session-a"),
        ] {
            let hidden = service
                .dispatch(&other, "list_sessions", &json!({}))
                .await
                .unwrap();
            assert!(hidden["sessions"].as_array().unwrap().is_empty());
        }
        assert_ne!(
            owner_document_key(&context("ab", "c")),
            owner_document_key(&context("a", "bc"))
        );
    }

    #[tokio::test]
    async fn declarations_survive_store_reopen() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("session-metadata");
        let owner = context("principal", "session");
        {
            let service = SessDeclMethods::new(Arc::new(RuntimeDocuments::new(
                CozoGraphShuttle::new_persistent(path.clone()).unwrap(),
            )));
            service
                .dispatch(&owner, "declare_session", &json!({"label":"durable"}))
                .await
                .unwrap();
        }
        let service = SessDeclMethods::new(Arc::new(RuntimeDocuments::new(
            CozoGraphShuttle::new_persistent(path).unwrap(),
        )));
        let listed = service
            .dispatch(&owner, "list_sessions", &json!({}))
            .await
            .unwrap();
        assert_eq!(listed["sessions"][0]["label"], "durable");
        assert_eq!(listed["sessions"][0]["id"], "session");
    }

    #[tokio::test]
    async fn invalid_or_caller_asserted_identity_inputs_are_rejected() {
        let service = SessDeclMethods::new(Arc::new(RuntimeDocuments::new(
            CozoGraphShuttle::new_in_memory().unwrap(),
        )));
        let owner = context("principal", "session");
        for args in [
            json!({"label":""}),
            json!({"label":"bad\nlabel"}),
            json!({"label":"x".repeat(129)}),
            json!({"label":"ok","id":"forged"}),
            json!({"label":"ok","user":"forged"}),
        ] {
            assert!(
                service
                    .dispatch(&owner, "declare_session", &args)
                    .await
                    .is_err()
            );
        }
        assert!(
            service
                .dispatch(&context("", "session"), "list_sessions", &json!({}))
                .await
                .is_err()
        );
        assert!(
            service
                .dispatch(&owner, "list_sessions", &json!({"principal":"forged"}))
                .await
                .is_err()
        );
    }

    #[test]
    fn schema_declares_only_the_two_backed_methods() {
        let schema = sess_decl_schema();
        assert_eq!(schema.methods.len(), 2);
        assert!(schema.methods.contains_key("declare_session"));
        assert!(schema.methods.contains_key("list_sessions"));
        assert_eq!(schema.version, "2.0.0");
    }

    #[test]
    fn all_state_subids_are_valid() {
        fn collect(value: &JsonValue, output: &mut Vec<String>) {
            if let Some(object) = value.as_object() {
                if let Some(subid) = object.get("x-oscal-subid").and_then(JsonValue::as_str) {
                    output.push(subid.to_string());
                }
                for nested in object.values() {
                    collect(nested, output);
                }
            }
            if let Some(array) = value.as_array() {
                for nested in array {
                    collect(nested, output);
                }
            }
        }
        let schema = serde_json::to_value(schemars::schema_for!(SessDeclState)).unwrap();
        let mut subids = Vec::new();
        collect(&schema, &mut subids);
        assert!(!subids.is_empty());
        for subid in subids {
            assert!(crate::state_plugins::common::oscal::validate_subid(&subid).is_ok());
        }
    }
}

inventory::submit! {
    crate::default_registry::PluginReg::new("sess_decl", |_ctx| Arc::new(SessDeclPlugin::new()))
}
