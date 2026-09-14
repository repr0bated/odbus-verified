//! Container-backed projects. Membership names authenticated sessions until
//! user profiles exist; a project is never an identity credential or a grant.
use anyhow::{bail, Result};
use async_trait::async_trait;
use op_state::{ApplyResult, Checkpoint, PluginCapabilities, StateDiff, StatePlugin};
use op_state_store::{CapabilityDecl, PluginSchema, SideEffect};
use serde::{Deserialize, Serialize};
use simd_json::OwnedValue;
use std::collections::BTreeSet;

use super::plugin_scaffold_helpers::method_decl_from_schemars_with_output;

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum ChatMode {
    #[default]
    Personal,
    Project,
    ControlPlane,
    Accountability,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProjectRole {
    Reader,
    Contributor,
    Admin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProjectStatus {
    Provisioning,
    Ready,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProjectMember {
    pub session_id: String,
    pub role: ProjectRole,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ProjectInvitation {
    pub invitation_id: String,
    pub invited_session_id: String,
    pub invited_by_session_id: String,
    pub role: ProjectRole,
    pub expires_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Project {
    pub project_id: String,
    pub title: String,
    pub owner_session_id: String,
    pub container_id: String,
    pub status: ProjectStatus,
    /// D-Bus method dispatch owns the store mounted at /project in the container.
    pub persistence_path: String,
    pub members: Vec<ProjectMember>,
    pub invitations: Vec<ProjectInvitation>,
    /// Ceiling only: effective authority also intersects the caller's exact grants.
    pub capability_ceiling: BTreeSet<String>,
    pub allowed_agents: BTreeSet<String>,
    pub selected_agent: Option<String>,
    pub created_at: i64,
}

impl Project {
    pub fn role_for(&self, session: &str) -> Option<ProjectRole> {
        if session == self.owner_session_id {
            return Some(ProjectRole::Admin);
        }
        self.members
            .iter()
            .find(|m| m.session_id == session)
            .map(|m| m.role)
    }

    pub fn authorize(&self, session: &str, write: bool, admin: bool) -> Result<()> {
        match self.role_for(session) {
            Some(ProjectRole::Admin) => Ok(()),
            Some(ProjectRole::Contributor) if !admin => Ok(()),
            Some(ProjectRole::Reader) if !write && !admin => Ok(()),
            _ => bail!("project access denied"),
        }
    }

    pub fn effective_capabilities(
        &self,
        session: &str,
        grants: &BTreeSet<String>,
    ) -> Result<BTreeSet<String>> {
        self.authorize(session, false, false)?;
        let reader = self.role_for(session) == Some(ProjectRole::Reader);
        Ok(self
            .capability_ceiling
            .intersection(grants)
            .filter(|cap| {
                !reader
                    || matches!(
                        cap.as_str(),
                        "project.read" | "chat.context" | "memory.read"
                    )
            })
            .cloned()
            .collect())
    }

    pub fn accept_invitation(
        &mut self,
        session: &str,
        invitation_id: &str,
        now: i64,
    ) -> Result<()> {
        let index = self
            .invitations
            .iter()
            .position(|i| {
                i.invitation_id == invitation_id
                    && i.invited_session_id == session
                    && i.expires_at > now
            })
            .ok_or_else(|| anyhow::anyhow!("invitation is unavailable or expired"))?;
        let invite = self.invitations.remove(index);
        self.members.retain(|m| m.session_id != session);
        self.members.push(ProjectMember {
            session_id: session.into(),
            role: invite.role,
        });
        Ok(())
    }
}

/// Public state contains no tenant names, membership, invitations or documents.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ProjectState {
    pub available: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateProjectInput {
    pub title: String,
    #[serde(default)]
    pub capability_ceiling: BTreeSet<String>,
    #[serde(default)]
    pub allowed_agents: BTreeSet<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EmptyInput {}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProjectInput {
    pub project_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ProjectOutput {
    pub project: Project,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ProjectListOutput {
    pub projects: Vec<Project>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InviteInput {
    pub project_id: String,
    pub invited_session_id: String,
    pub role: ProjectRole,
    pub ttl_seconds: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AcceptInviteInput {
    pub project_id: String,
    pub invitation_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RevokeMemberInput {
    pub project_id: String,
    pub session_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SelectAgentInput {
    pub project_id: String,
    pub agent_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProjectCollection {
    Memory,
    Context,
    Artifacts,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ProjectDocument {
    pub document_id: String,
    pub collection: ProjectCollection,
    pub content: String,
    pub media_type: String,
    pub author_session_id: String,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PutDocumentInput {
    pub project_id: String,
    pub document_id: String,
    pub collection: ProjectCollection,
    pub content: String,
    pub media_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListDocumentsInput {
    pub project_id: String,
    pub collection: ProjectCollection,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct DocumentOutput {
    pub document: ProjectDocument,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct DocumentsOutput {
    pub documents: Vec<ProjectDocument>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ResolveContextInput {
    pub mode: ChatMode,
    #[serde(default)]
    pub project_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ResolvedChatContext {
    pub mode: ChatMode,
    pub session_id: String,
    pub project_id: Option<String>,
    pub memory_namespace: String,
    pub read_only: bool,
    pub effective_capabilities: BTreeSet<String>,
    pub allowed_agents: BTreeSet<String>,
    pub selected_agent: Option<String>,
}

pub struct ProjectPlugin;

pub fn known_agent_ids() -> BTreeSet<String> {
    op_agents::builtin_agent_descriptors()
        .into_iter()
        .map(|agent| agent.agent_type)
        .collect()
}

#[async_trait]
impl StatePlugin for ProjectPlugin {
    fn name(&self) -> &str {
        "project"
    }
    fn version(&self) -> &str {
        "1.0.0"
    }
    fn schema(&self) -> Option<PluginSchema> {
        Some(project_schema())
    }
    async fn calculate_diff(&self, _: &OwnedValue, _: &OwnedValue) -> Result<StateDiff> {
        bail!("projects require authenticated schema methods, not unscoped state replacement")
    }
    async fn apply_state(&self, _: &StateDiff) -> Result<ApplyResult> {
        bail!("use authenticated project methods")
    }
    async fn verify_state(&self, _: &OwnedValue) -> Result<bool> {
        bail!("project state is caller-scoped; use get")
    }
    async fn create_checkpoint(&self) -> Result<Checkpoint> {
        bail!("project checkpoints are not supported")
    }
    async fn rollback(&self, _: &Checkpoint) -> Result<()> {
        bail!("project rollback is not supported")
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

pub fn project_schema() -> PluginSchema {
    let root = serde_json::to_value(schemars::schema_for!(ProjectState)).expect("project schema");
    let mut schema = super::schemars_adapter::plugin_schema_from_json("project", "1.0.0",
        "Container-backed projects with session ACLs, invitations, persistent memory/context/artifacts and agent selection", &root);
    schema.category = "workspace".into();
    schema.display_name = Some("Projects".into());
    macro_rules! method {
        ($name:literal, $input:ty, $output:ty, $effect:ident, $cap:literal) => {
            schema.methods.insert(
                $name.into(),
                method_decl_from_schemars_with_output::<$input, $output>(
                    $name,
                    SideEffect::$effect,
                    matches!(SideEffect::$effect, SideEffect::Read),
                    $cap,
                    &format!(
                        "{}.service.project.{}@v1",
                        if matches!(SideEffect::$effect, SideEffect::Read) {
                            "obs"
                        } else {
                            "mut"
                        },
                        $name.replace('_', "-")
                    ),
                ),
            );
        };
    }
    method!(
        "create",
        CreateProjectInput,
        ProjectOutput,
        Mutation,
        "project.create"
    );
    method!("list", EmptyInput, ProjectListOutput, Read, "project.read");
    method!("get", ProjectInput, ProjectOutput, Read, "project.read");
    method!(
        "invite",
        InviteInput,
        ProjectOutput,
        Mutation,
        "project.admin"
    );
    method!(
        "accept_invite",
        AcceptInviteInput,
        ProjectOutput,
        Mutation,
        "project.read"
    );
    method!(
        "revoke_member",
        RevokeMemberInput,
        ProjectOutput,
        Mutation,
        "project.admin"
    );
    method!(
        "select_agent",
        SelectAgentInput,
        ProjectOutput,
        Mutation,
        "project.admin"
    );
    method!(
        "put_document",
        PutDocumentInput,
        DocumentOutput,
        Mutation,
        "project.write"
    );
    method!(
        "list_documents",
        ListDocumentsInput,
        DocumentsOutput,
        Read,
        "project.read"
    );
    method!(
        "resolve_context",
        ResolveContextInput,
        ResolvedChatContext,
        Read,
        "chat.context"
    );
    for id in [
        "project.create",
        "project.read",
        "project.write",
        "project.admin",
        "chat.context",
        "chat.control_plane",
    ] {
        schema.capabilities.insert(
            id.into(),
            CapabilityDecl {
                id: id.into(),
                description: format!("{id}; project membership is additionally required"),
            },
        );
    }
    schema
}

inventory::submit! {
    crate::default_registry::PluginReg::new("project", |_ctx| std::sync::Arc::new(ProjectPlugin))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn project() -> Project {
        Project {
            project_id: "p".into(),
            title: "test".into(),
            owner_session_id: "owner".into(),
            container_id: "p".into(),
            status: ProjectStatus::Ready,
            persistence_path: "/project".into(),
            members: vec![],
            invitations: vec![],
            capability_ceiling: BTreeSet::from(["memory.read".into(), "memory.write".into()]),
            allowed_agents: BTreeSet::new(),
            selected_agent: None,
            created_at: 1,
        }
    }
    #[test]
    fn acl_does_not_infer_membership_or_elevate_reader() {
        let mut p = project();
        assert!(p.authorize("stranger", false, false).is_err());
        p.members.push(ProjectMember {
            session_id: "reader".into(),
            role: ProjectRole::Reader,
        });
        assert!(p.authorize("reader", false, false).is_ok());
        assert!(p.authorize("reader", true, false).is_err());
        assert!(p.authorize("reader", false, true).is_err());
        assert!(p.authorize("owner", true, true).is_ok());
    }
    #[test]
    fn invitations_are_target_bound_expiring_and_single_use() {
        let mut p = project();
        p.invitations.push(ProjectInvitation {
            invitation_id: "i".into(),
            invited_session_id: "member".into(),
            invited_by_session_id: "owner".into(),
            role: ProjectRole::Contributor,
            expires_at: 20,
        });
        assert!(p.accept_invitation("other", "i", 1).is_err());
        assert!(p.accept_invitation("member", "i", 20).is_err());
        p.accept_invitation("member", "i", 19).unwrap();
        assert!(p.accept_invitation("member", "i", 19).is_err());
        assert_eq!(p.role_for("member"), Some(ProjectRole::Contributor));
    }
    #[test]
    fn project_ceiling_never_grants_authority() {
        let p = project();
        let grants = BTreeSet::from(["memory.read".into(), "shell.exec".into()]);
        assert_eq!(
            p.effective_capabilities("owner", &grants).unwrap(),
            BTreeSet::from(["memory.read".into()])
        );
        assert!(p.effective_capabilities("outsider", &grants).is_err());
    }
    #[test]
    fn schema_has_no_public_tenant_catalog_or_generic_success_results() {
        let schema = project_schema();
        assert!(!schema.fields.contains_key("projects"));
        for name in [
            "create",
            "invite",
            "accept_invite",
            "put_document",
            "resolve_context",
        ] {
            assert!(schema.methods.contains_key(name));
        }
        assert!(serde_json::from_value::<CreateProjectInput>(
            serde_json::json!({"title":"x", "owner_session_id":"victim"})
        )
        .is_err());
    }

    #[test]
    fn schema_classifies_reads_and_writes_with_stable_subids() {
        let schema = project_schema();
        let expected = [
            (
                "create",
                SideEffect::Mutation,
                false,
                "mut.service.project.create@v1",
            ),
            (
                "list",
                SideEffect::Read,
                true,
                "obs.service.project.list@v1",
            ),
            ("get", SideEffect::Read, true, "obs.service.project.get@v1"),
            (
                "invite",
                SideEffect::Mutation,
                false,
                "mut.service.project.invite@v1",
            ),
            (
                "accept_invite",
                SideEffect::Mutation,
                false,
                "mut.service.project.accept-invite@v1",
            ),
            (
                "revoke_member",
                SideEffect::Mutation,
                false,
                "mut.service.project.revoke-member@v1",
            ),
            (
                "select_agent",
                SideEffect::Mutation,
                false,
                "mut.service.project.select-agent@v1",
            ),
            (
                "put_document",
                SideEffect::Mutation,
                false,
                "mut.service.project.put-document@v1",
            ),
            (
                "list_documents",
                SideEffect::Read,
                true,
                "obs.service.project.list-documents@v1",
            ),
            (
                "resolve_context",
                SideEffect::Read,
                true,
                "obs.service.project.resolve-context@v1",
            ),
        ];
        assert_eq!(schema.methods.len(), expected.len());
        for (name, effect, idempotent, subid) in expected {
            let method = schema.methods.get(name).expect(name);
            assert_eq!(method.side_effect, effect, "{name} side effect");
            assert_eq!(method.idempotent, idempotent, "{name} idempotence");
            assert_eq!(method.subid, subid, "{name} subid");
        }
    }
}
