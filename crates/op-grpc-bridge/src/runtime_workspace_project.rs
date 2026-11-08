//! Project ACLs and chat context use the existing identity Cozo catalog.
//! Project documents stay in their existing UUID-bound private project store.
use std::{collections::{BTreeSet, HashMap}, path::{Path, PathBuf}, sync::Arc};
use anyhow::{Context, Result, ensure, bail};
use op_cozo_store::CozoGraphShuttle;
use op_plugins::state_plugins::{incus::IncusPlugin, project::*};
use serde_json::{Value, json};
use tokio::sync::Mutex;
use crate::{core::BackendStateUpdate, runtime::{BackendContext, BackendOutcome, RuntimeError}};

const PROJECT_ROOT: &str = "/var/lib/op-dbus/projects";
pub(super) struct Projects { stores: Mutex<HashMap<String, Arc<CozoGraphShuttle>>> }

pub(super) fn supports(method: &str) -> bool {
    matches!(method, "create" | "get" | "list" | "invite" | "accept_invite" | "revoke_member" | "select_agent" | "put_document" | "list_documents" | "resolve_context" | "select_context" | "get_context")
}

fn canonical_id(id: &str) -> Result<()> {
    ensure!(uuid::Uuid::parse_str(id)?.to_string() == id, "expected canonical session/project UUID");
    Ok(())
}

async fn query(ctx: &BackendContext<'_>, statement: &'static str, parameters: Value, mutation: bool) -> Result<Value> {
    let store = ctx.runtime.identity_store().clone();
    let authority = ctx.caller.authority.clone();
    let handle = tokio::runtime::Handle::current();
    tokio::task::spawn_blocking(move || {
        handle.block_on(authority.revalidate())?;
        let rows = store.run_query(statement, Some(parameters))?;
        if mutation { store.sync_runtime_document_store()?; }
        Ok(rows)
    }).await.context("project catalog worker failed")?
}

fn projects(rows: Value) -> Result<Vec<Project>> {
    rows.as_array().context("invalid project catalog rows")?.iter().map(|row| {
        serde_json::from_str(row["document"].as_str().context("invalid project document")?).map_err(Into::into)
    }).collect()
}

async fn list(ctx: &BackendContext<'_>) -> Result<Vec<Project>> {
    projects(query(ctx, "?[document] := *project_catalog{document}", json!({}), false).await?)
}

async fn get(ctx: &BackendContext<'_>, id: &str) -> Result<Project> {
    canonical_id(id)?;
    projects(query(ctx, "?[document] := *project_catalog{project_id, document}, project_id = $id", json!({"id":id}), false).await?)?
        .into_iter().next().context("project access denied")
}

async fn save(ctx: &BackendContext<'_>, project: &Project) -> Result<Project> {
    query(ctx, "?[project_id, document] <- [[$id, $document]] :put project_catalog {project_id => document}",
        json!({"id":project.project_id,"document":serde_json::to_string(project)?}), true).await?;
    get(ctx, &project.project_id).await
}

fn visible(mut project: Project, session: &str) -> Project {
    if project.role_for(session) != Some(ProjectRole::Admin) { project.invitations.clear(); }
    project
}

async fn ready(project: &Project) -> Result<()> {
    ensure!(project.status == ProjectStatus::Ready, "project provisioning is incomplete");
    IncusPlugin::verify_project_container(&project.project_id, &project.container_id, &project.owner_session_id, &project.persistence_path).await
}

impl Projects {
    pub(super) fn new() -> Self { Self { stores: Mutex::new(HashMap::new()) } }

    async fn store(&self, project: &Project, create: bool) -> Result<Arc<CozoGraphShuttle>> {
        canonical_id(&project.project_id)?;
        let expected = Path::new(PROJECT_ROOT).join(&project.project_id);
        ensure!(expected.to_str() == Some(project.persistence_path.as_str()), "project persistence binding differs");
        let mut stores = self.stores.lock().await;
        if let Some(store) = stores.get(&project.project_id) { return Ok(store.clone()); }
        let store = tokio::task::spawn_blocking(move || {
            if !create {
                let metadata = std::fs::symlink_metadata(expected.join("cozo"))?;
                ensure!(metadata.is_dir() && !metadata.file_type().is_symlink(), "project database missing or unsafe; refusing replacement");
            }
            for directory in [PathBuf::from(PROJECT_ROOT), expected.clone()] {
                if let Ok(metadata) = std::fs::symlink_metadata(&directory) {
                    ensure!(metadata.is_dir() && !metadata.file_type().is_symlink(), "unsafe project store directory");
                } else { ensure!(create, "project store directory missing"); }
            }
            Ok::<_, anyhow::Error>(Arc::new(CozoGraphShuttle::new_persistent_private(expected.join("cozo"))?))
        }).await??;
        stores.insert(project.project_id.clone(), store.clone());
        Ok(store)
    }

    pub(super) async fn dispatch(&self, ctx: &BackendContext<'_>, method: &str, args: &Value) -> Result<BackendOutcome, RuntimeError> {
        let result = self.execute(ctx, method, args).await.map_err(|error| super::domain_error(error, matches!(ctx.declaration.side_effect, op_state_store::SideEffect::Mutation)))?;
        // ProjectState declares availability only. ACLs, context and documents
        // are method-owned private rows, never copied into public availability.
        Ok(super::outcome(result, BackendStateUpdate::Unchanged, super::private_scope(ctx)))
    }

    async fn execute(&self, ctx: &BackendContext<'_>, method: &str, args: &Value) -> Result<Value> {
        let session = &ctx.caller.session_id;
        canonical_id(session)?;
        let grants: BTreeSet<_> = ctx.runtime.current_grants(ctx.caller).await?.into_iter().collect();
        match method {
            "resolve_context" => return Ok(serde_json::to_value(resolve(ctx, &grants, serde_json::from_value(args.clone())?).await?)?),
            "get_context" => {
                let _: GetContextInput = serde_json::from_value(args.clone())?;
                let selection = stored_selection(ctx).await?;
                return Ok(serde_json::to_value(resolve(ctx, &grants, ResolveContextInput { mode: selection.mode, project_id: selection.project_id, personal_namespace: selection.personal_namespace }).await?)?);
            }
            "select_context" => {
                let input: SelectContextInput = serde_json::from_value(args.clone())?;
                let selection = ChatContextSelection { mode: input.mode, project_id: input.project_id, personal_namespace: input.personal_namespace };
                let context = resolve(ctx, &grants, ResolveContextInput { mode: selection.mode, project_id: selection.project_id.clone(), personal_namespace: selection.personal_namespace.clone() }).await?;
                query(ctx, "?[session_id, document] <- [[$session, $document]] :put chat_context_selection {session_id => document}", json!({"session":session,"document":serde_json::to_string(&selection)?}), true).await?;
                return Ok(serde_json::to_value(context)?);
            }
            "list" => {
                let _: EmptyInput = serde_json::from_value(args.clone())?;
                return Ok(serde_json::to_value(ProjectListOutput { projects: list(ctx).await?.into_iter().filter(|project| project.role_for(session).is_some()).map(|project| visible(project, session)).collect() })?);
            }
            "create" => return self.create(ctx, args, &grants).await,
            _ => {}
        }
        let id = args["project_id"].as_str().context("project_id is required")?;
        let mut project = get(ctx, id).await?;
        if method == "accept_invite" {
            let input: AcceptInviteInput = serde_json::from_value(args.clone())?;
            project.accept_invitation(session, &input.invitation_id, chrono::Utc::now().timestamp())?;
            project = save(ctx, &project).await?;
        } else {
            project.authorize(session, method == "put_document", matches!(method, "invite" | "revoke_member" | "select_agent"))?;
            match method {
                "get" => { let _: ProjectInput = serde_json::from_value(args.clone())?; }
                "invite" => {
                    let input: InviteInput = serde_json::from_value(args.clone())?;
                    canonical_id(&input.invited_session_id)?;
                    ensure!((1..=604800).contains(&input.ttl_seconds), "invitation lifetime must be 1 second to 7 days");
                    ensure!(project.role_for(&input.invited_session_id).is_none(), "session is already a project member");
                    let store = ctx.runtime.identity_store().clone();
                    let invited = input.invited_session_id.clone();
                    let exists = tokio::task::spawn_blocking(move || store.get_identity_sled(&invited)).await??;
                    ensure!(exists.is_some(), "invited session does not exist");
                    project.invitations.retain(|invite| invite.invited_session_id != input.invited_session_id);
                    project.invitations.push(ProjectInvitation { invitation_id: uuid::Uuid::new_v4().to_string(), invited_session_id: input.invited_session_id, invited_by_session_id: session.clone(), role: input.role, expires_at: chrono::Utc::now().timestamp() + i64::from(input.ttl_seconds) });
                    project = save(ctx, &project).await?;
                }
                "revoke_member" => {
                    let input: RevokeMemberInput = serde_json::from_value(args.clone())?;
                    ensure!(input.session_id != project.owner_session_id, "cannot revoke the project owner");
                    project.members.retain(|member| member.session_id != input.session_id);
                    project.invitations.retain(|invite| invite.invited_session_id != input.session_id);
                    project = save(ctx, &project).await?;
                }
                "select_agent" => {
                    let input: SelectAgentInput = serde_json::from_value(args.clone())?;
                    ensure!(project.allowed_agents.contains(&input.agent_id) && known_agent_ids().contains(&input.agent_id), "agent is not allowed for this project");
                    project.selected_agent = Some(input.agent_id);
                    project = save(ctx, &project).await?;
                }
                "put_document" => {
                    let input: PutDocumentInput = serde_json::from_value(args.clone())?;
                    ensure!(!input.document_id.is_empty() && input.document_id.len() <= 128 && input.content.len() <= 1_048_576 && !input.media_type.is_empty() && input.media_type.len() <= 128, "invalid project document size or media type");
                    ready(&project).await?;
                    let store = self.store(&project, false).await?;
                    let document = ProjectDocument { document_id: input.document_id, collection: input.collection, content: input.content, media_type: input.media_type, author_session_id: session.clone(), updated_at: chrono::Utc::now().timestamp() };
                    let parameters = json!({"collection":document.collection,"id":document.document_id,"document":serde_json::to_string(&document)?});
                    ctx.caller.authority.revalidate().await?;
                    tokio::task::spawn_blocking(move || {
                        store.run_query("?[collection, document_id, document] <- [[$collection, $id, $document]] :put project_documents {collection, document_id => document}", Some(parameters))?;
                        store.sync_runtime_document_store()
                    }).await??;
                    return Ok(serde_json::to_value(DocumentOutput { document })?);
                }
                "list_documents" => {
                    let input: ListDocumentsInput = serde_json::from_value(args.clone())?;
                    ready(&project).await?;
                    let store = self.store(&project, false).await?;
                    let rows = tokio::task::spawn_blocking(move || store.run_query("?[document] := *project_documents{collection, document}, collection = $collection :limit 100", Some(json!({"collection":input.collection})))).await??;
                    let documents = rows.as_array().context("invalid project document rows")?.iter().map(|row| serde_json::from_str::<ProjectDocument>(row["document"].as_str().context("invalid project document")?).map_err(anyhow::Error::from)).collect::<Result<Vec<_>>>()?;
                    return Ok(serde_json::to_value(DocumentsOutput { documents })?);
                }
                _ => bail!("unknown project method {method}"),
            }
        }
        ctx.caller.authority.revalidate().await?;
        Ok(serde_json::to_value(ProjectOutput { project: visible(project, session) })?)
    }

    async fn create(&self, ctx: &BackendContext<'_>, args: &Value, grants: &BTreeSet<String>) -> Result<Value> {
        let input: CreateProjectInput = serde_json::from_value(args.clone())?;
        ensure!(!input.title.trim().is_empty() && input.title.len() <= 256, "project title must contain 1-256 bytes");
        ensure!(input.capability_ceiling.is_subset(grants), "project ceiling cannot exceed creator grants");
        ensure!(input.allowed_agents.is_subset(&known_agent_ids()), "project agent is not in declared catalog");
        let fingerprint = std::env::var("OP_PROJECT_IMAGE_FINGERPRINT").context("project image fingerprint is not configured")?;
        let pool = std::env::var("OP_PROJECT_STORAGE_POOL").context("project storage pool is not configured")?;
        let pending: Vec<_> = list(ctx).await?.into_iter().filter(|project| project.status == ProjectStatus::Provisioning && project.owner_session_id == ctx.caller.session_id && project.title == input.title && project.capability_ceiling == input.capability_ceiling && project.allowed_agents == input.allowed_agents).collect();
        ensure!(pending.len() <= 1, "ambiguous pending project creation");
        let mut project = if let Some(project) = pending.into_iter().next() { project } else {
            let id = uuid::Uuid::new_v4().to_string();
            Project { project_id: id.clone(), title: input.title, owner_session_id: ctx.caller.session_id.clone(), container_id: id.clone(), status: ProjectStatus::Provisioning, persistence_path: Path::new(PROJECT_ROOT).join(&id).to_str().context("non-UTF8 project path")?.to_owned(), members: vec![], invitations: vec![], capability_ceiling: input.capability_ceiling, allowed_agents: input.allowed_agents, selected_agent: None, created_at: chrono::Utc::now().timestamp() }
        };
        project = save(ctx, &project).await?;
        self.store(&project, true).await?;
        let binding = ctx.runtime.create_project_container(ctx.caller, &project.project_id, &project, &fingerprint, &pool).await?;
        ensure!(binding.phase == "complete" && binding.request.owner_kind == "project" && binding.request.logical_target == project.project_id && binding.request.caller_session == ctx.caller.session_id && binding.request.actor_id == ctx.caller.principal_id && binding.request.caller_genesis == ctx.caller.session_genesis, "project native creation ownership differs");
        project.container_id = binding.name;
        project.status = ProjectStatus::Ready;
        project = save(ctx, &project).await?;
        Ok(serde_json::to_value(ProjectOutput { project })?)
    }
}

async fn stored_selection(ctx: &BackendContext<'_>) -> Result<ChatContextSelection> {
    let rows = query(ctx, "?[document] := *chat_context_selection{session_id, document}, session_id = $session", json!({"session":ctx.caller.session_id}), false).await?;
    let rows = rows.as_array().context("invalid chat context selection rows")?;
    ensure!(rows.len() <= 1, "ambiguous session chat context");
    match rows.first() {
        None => Ok(ChatContextSelection::default()),
        Some(row) => Ok(serde_json::from_str(row["document"].as_str().context("invalid stored chat context")?)?),
    }
}

async fn resolve(ctx: &BackendContext<'_>, grants: &BTreeSet<String>, input: ResolveContextInput) -> Result<ResolvedChatContext> {
    ensure!(input.mode == ChatMode::Project || input.project_id.is_none(), "project_id is only valid in project mode");
    ensure!(input.mode == ChatMode::Personal || input.personal_namespace.is_none(), "personal_namespace is only valid in personal mode");
    let session = &ctx.caller.session_id;
    let mut context = ResolvedChatContext { mode: input.mode, session_id: session.clone(), project_id: None, memory_namespace: String::new(), read_only: input.mode == ChatMode::Accountability, effective_capabilities: BTreeSet::new(), allowed_agents: BTreeSet::new(), selected_agent: None };
    let admitted: &[&str] = match input.mode {
        ChatMode::Project => {
            let id = input.project_id.context("project mode requires project_id")?;
            let project = get(ctx, &id).await?;
            context.effective_capabilities = project.effective_capabilities(session, grants)?;
            ready(&project).await?;
            context.read_only = project.role_for(session) == Some(ProjectRole::Reader);
            context.memory_namespace = format!("project/{id}");
            context.project_id = Some(id);
            context.allowed_agents = project.allowed_agents;
            context.selected_agent = project.selected_agent;
            ctx.caller.authority.revalidate().await?;
            return Ok(context);
        }
        ChatMode::Personal => {
            context.memory_namespace = memory_namespace_for(input.mode, session, None, input.personal_namespace.as_deref())?;
            &["chat.context", "cognitive.read", "cognitive.memory.write"]
        }
        ChatMode::ControlPlane => {
            ensure!(grants.contains("chat.control_plane"), "control-plane mode requires explicit grant");
            context.memory_namespace = format!("session/{session}/control_plane");
            &["chat.context", "cognitive.read", "cognitive.memory.write", "workflows.read", "workflows.execute"]
        }
        ChatMode::Accountability => {
            context.memory_namespace = format!("session/{session}/accountability");
            &["chat.context", "snowball.read"]
        }
        ChatMode::NotebookLm => {
            let rows = query(ctx, "?[notebook_id] := *notebooklm_selection{session_id, notebook_id}, session_id = $session", json!({"session":session}), false).await?;
            let notebook = rows.as_array().and_then(|rows| rows.first()).and_then(|row| row["notebook_id"].as_str()).filter(|id| !id.is_empty()).context("NotebookLM mode requires selected notebook")?;
            context.memory_namespace = memory_namespace_for(input.mode, session, Some(notebook), None)?;
            &["chat.context", "cognitive.read", "cognitive.memory.write", "notebooklm.read", "notebooklm.query"]
        }
        ChatMode::UiRender => { context.read_only = true; &["chat.context"] }
    };
    context.effective_capabilities = grants.iter().filter(|capability| admitted.contains(&capability.as_str())).cloned().collect();
    ctx.caller.authority.revalidate().await?;
    Ok(context)
}
