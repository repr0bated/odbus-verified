//! Authenticated project effects, entered only through MutationEngine/D-Bus.
//! No ambient user, caller-supplied owner or cross-project store path is used.
use anyhow::{Context, Result, anyhow, bail};
use op_cozo_store::CozoGraphShuttle;
use op_plugins::state_plugins::{incus::IncusPlugin, project::*};
use serde_json::{Value, json};
use std::collections::{BTreeSet, HashMap};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use tokio::sync::Mutex;

use crate::{interceptor::load_capability_grants, mutation_engine::MutationEngine};

const PROJECT_ROOT: &str = "/var/lib/op-dbus/projects";
static PROJECT_LOCK: Mutex<()> = Mutex::const_new(());
type Stores = HashMap<String, Arc<CozoGraphShuttle>>;
static STORES: OnceLock<Mutex<Stores>> = OnceLock::new();

fn canonical_id(id: &str) -> Result<()> {
    if uuid::Uuid::parse_str(id)?.to_string() != id {
        bail!("expected a canonical session/project UUID");
    }
    Ok(())
}

fn project_path(id: &str) -> Result<PathBuf> {
    canonical_id(id)?;
    Ok(Path::new(PROJECT_ROOT).join(id))
}

async fn catalog(engine: &MutationEngine, query: &'static str, params: Value) -> Result<Value> {
    let store = engine.identity_sled_store()?.cozo().clone();
    tokio::task::spawn_blocking(move || store.run_query(query, Some(params)))
        .await?
        .map_err(Into::into)
}

fn decode_project_rows(value: Value) -> Result<Vec<Project>> {
    value
        .as_array()
        .ok_or_else(|| anyhow!("invalid project catalog rows"))?
        .iter()
        .map(|row| {
            serde_json::from_str(
                row["document"]
                    .as_str()
                    .ok_or_else(|| anyhow!("invalid project catalog document"))?,
            )
            .map_err(Into::into)
        })
        .collect()
}

async fn list_projects(engine: &MutationEngine) -> Result<Vec<Project>> {
    decode_project_rows(
        catalog(
            engine,
            "?[document] := *project_catalog{document}",
            json!({}),
        )
        .await?,
    )
}

async fn get_project(engine: &MutationEngine, id: &str) -> Result<Project> {
    canonical_id(id)?;
    decode_project_rows(
        catalog(
            engine,
            "?[document] := *project_catalog{project_id, document}, project_id = $id",
            json!({"id":id}),
        )
        .await?,
    )?
    .into_iter()
    .next()
    .ok_or_else(|| anyhow!("project access denied"))
}

async fn save_project(engine: &MutationEngine, project: &Project) -> Result<()> {
    catalog(engine, "?[project_id, document] <- [[$id, $document]] :put project_catalog {project_id => document}",
        json!({"id":project.project_id, "document":serde_json::to_string(project)?})).await?;
    Ok(())
}

async fn project_store(project: &Project, create: bool) -> Result<Arc<CozoGraphShuttle>> {
    let mut stores = STORES
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .await;
    if let Some(store) = stores.get(&project.project_id) {
        return Ok(store.clone());
    }
    let path = project_path(&project.project_id)?;
    if path.to_str() != Some(project.persistence_path.as_str()) {
        bail!("project persistence binding mismatch");
    }
    let store = tokio::task::spawn_blocking(move || -> Result<CozoGraphShuttle> {
        if create {
            std::fs::create_dir_all(&path)?;
        }
        // The project namespace is a UUID, never a user path. Refuse symlinks
        // at every owned level and retain private permissions on the host.
        for directory in [Path::new(PROJECT_ROOT).to_path_buf(), path.clone()] {
            let meta = std::fs::symlink_metadata(&directory)?;
            if !meta.is_dir() || meta.file_type().is_symlink() {
                bail!("unsafe project store directory");
            }
            std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700))?;
        }
        let db_path = path.join("cozo");
        if let Ok(meta) = std::fs::symlink_metadata(&db_path) {
            if meta.file_type().is_symlink() {
                bail!("unsafe project database path");
            }
        } else if !create {
            bail!("project database is missing; refusing to create an empty replacement");
        }
        Ok(CozoGraphShuttle::new_persistent(db_path)?)
    })
    .await??;
    let store = Arc::new(store);
    stores.insert(project.project_id.clone(), store.clone());
    Ok(store)
}

async fn require_ready(project: &Project) -> Result<()> {
    if project.status != ProjectStatus::Ready {
        bail!("project provisioning is incomplete");
    }
    IncusPlugin::verify_project_container(
        &project.container_id,
        &project.owner_session_id,
        &project.persistence_path,
    )
    .await
}

fn view_for(mut project: Project, session: &str) -> Project {
    if project.role_for(session) != Some(ProjectRole::Admin) {
        project.invitations.clear();
    }
    project
}

pub(crate) async fn verified_session(
    engine: &MutationEngine,
    actor: &str,
    session: Option<&str>,
    genesis: Option<&str>,
) -> Result<String> {
    let session = session
        .filter(|id| !id.is_empty())
        .ok_or_else(|| anyhow!("verified session is required"))?;
    canonical_id(session)?;
    if let Some(external) = crate::external_mcp_session::verify_connected_session(
        engine, actor, session, genesis,
    ).await? {
        return Ok(external);
    }
    let sled = crate::identity_sled_dispatch::stored_session(engine, session)
        .await
        .context("identity credential projection is unavailable")?
        .ok_or_else(|| anyhow!("verified session does not exist"))?;
    if !sled.is_anchored()
        || sled.genesis.as_deref() != genesis
        || op_identity::session::derive_principal_id(&sled.wireguard_pubkey) != actor
        || sled
            .expires_at
            .is_some_and(|at| at != 0 && at <= chrono::Utc::now().timestamp())
    {
        bail!("verified session binding is invalid");
    }
    let principal =
        crate::human_principal_dispatch::resolve_key_for_assertion(&sled.wireguard_pubkey)
            .await
            .map_err(|_| anyhow!("principal registry is unavailable"))?
            .ok_or_else(|| anyhow!("session key is not registered"))?;
    if principal.revoked_at != 0 || principal.principal_id != actor {
        bail!("session principal is revoked or mismatched");
    }
    Ok(session.into())
}

pub(crate) async fn dispatch(
    engine: &MutationEngine,
    method: &str,
    args: &Value,
    actor: &str,
    session: Option<&str>,
    genesis: Option<&str>,
) -> Result<Value> {
    let session = verified_session(engine, actor, session, genesis).await?;
    let grants: BTreeSet<String> = load_capability_grants(actor).into_iter().collect();
    let required = match method {
        "create" => "project.create",
        "invite" | "revoke_member" | "select_agent" => "project.admin",
        "put_document" => "project.write",
        "resolve_context" | "select_context" | "get_context" => "chat.context",
        "get" | "list" | "accept_invite" | "list_documents" => "project.read",
        _ => bail!("unknown project method"),
    };
    if !grants.contains(required) {
        bail!("exact project capability grant is required");
    }
    // Serializes ACL changes with all data access: a revocation cannot race a
    // stale read/modify/write of the project document. Cross-process entry is
    // exclusively the bridge; Cozo's process lock prevents a second writer.
    let _lock = PROJECT_LOCK.lock().await;
    if method == "resolve_context" {
        let input: ResolveContextInput = serde_json::from_value(args.clone())?;
        return Ok(serde_json::to_value(
            resolve_context(engine, &session, &grants, input).await?,
        )?);
    }
    if method == "select_context" {
        let input: SelectContextInput = serde_json::from_value(args.clone())?;
        let selection = ChatContextSelection {
            mode: input.mode,
            project_id: input.project_id,
            personal_namespace: input.personal_namespace,
        };
        // Validate exactly as a turn would resolve it before persisting.
        let resolved = resolve_selection(engine, &session, &grants, &selection).await?;
        let document = serde_json::to_string(&selection)?;
        catalog(
            engine,
            "?[session_id, document] <- [[$session, $document]] :put chat_context_selection {session_id => document}",
            json!({"session": session, "document": document}),
        )
        .await?;
        return Ok(serde_json::to_value(resolved)?);
    }
    if method == "get_context" {
        let _: GetContextInput = serde_json::from_value(args.clone())?;
        return Ok(serde_json::to_value(
            current_chat_context(engine, &session, &grants).await?,
        )?);
    }
    if method == "list" {
        let _: EmptyInput = serde_json::from_value(args.clone())?;
        return Ok(serde_json::to_value(ProjectListOutput {
            projects: list_projects(engine)
                .await?
                .into_iter()
                .filter(|p| p.role_for(&session).is_some())
                .map(|p| view_for(p, &session))
                .collect(),
        })?);
    }
    if method == "create" {
        let input: CreateProjectInput = serde_json::from_value(args.clone())?;
        if input.title.trim().is_empty() || input.title.len() > 256 {
            bail!("project title must contain 1–256 bytes");
        }
        if !input.capability_ceiling.is_subset(&grants) {
            bail!("project ceiling cannot exceed creator's exact grants");
        }
        if !input.allowed_agents.is_subset(&known_agent_ids()) {
            bail!("project agent is not in the declared catalog");
        }
        let fingerprint = std::env::var("OP_PROJECT_IMAGE_FINGERPRINT")
            .context("project image fingerprint is not configured")?;
        let pool = std::env::var("OP_PROJECT_STORAGE_POOL")
            .context("project storage pool is not configured")?;
        let id = uuid::Uuid::new_v4().to_string();
        let mut project = Project {
            project_id: id.clone(),
            title: input.title,
            owner_session_id: session.clone(),
            container_id: id.clone(),
            status: ProjectStatus::Provisioning,
            persistence_path: project_path(&id)?.to_string_lossy().into_owned(),
            members: vec![],
            invitations: vec![],
            capability_ceiling: input.capability_ceiling,
            allowed_agents: input.allowed_agents,
            selected_agent: None,
            created_at: chrono::Utc::now().timestamp(),
        };
        save_project(engine, &project).await?;
        project_store(&project, true).await?;
        IncusPlugin::create_project_container(
            &id,
            &session,
            &fingerprint,
            &pool,
            &project.persistence_path,
        )
        .await
        .with_context(|| format!("project {id} remains provisioning; inspect before retrying"))?;
        project.status = ProjectStatus::Ready;
        save_project(engine, &project).await?;
        return Ok(serde_json::to_value(ProjectOutput { project })?);
    }
    let id = args["project_id"]
        .as_str()
        .ok_or_else(|| anyhow!("project_id is required"))?;
    let mut project = get_project(engine, id).await?;
    if method == "accept_invite" {
        let input: AcceptInviteInput = serde_json::from_value(args.clone())?;
        project.accept_invitation(
            &session,
            &input.invitation_id,
            chrono::Utc::now().timestamp(),
        )?;
        save_project(engine, &project).await?;
    } else {
        project.authorize(
            &session,
            method == "put_document",
            matches!(method, "invite" | "revoke_member" | "select_agent"),
        )?;
        match method {
            "get" => {
                let _: ProjectInput = serde_json::from_value(args.clone())?;
            }
            "invite" => {
                let input: InviteInput = serde_json::from_value(args.clone())?;
                canonical_id(&input.invited_session_id)?;
                if !(1..=604800).contains(&input.ttl_seconds) {
                    bail!("invitation lifetime must be 1 second to 7 days");
                }
                if project.role_for(&input.invited_session_id).is_some() {
                    bail!("session is already a member");
                }
                if crate::identity_sled_dispatch::stored_session(engine, &input.invited_session_id)
                    .await
                    .context("identity credential projection is unavailable")?
                    .is_none()
                {
                    bail!("invited session does not exist");
                }
                project
                    .invitations
                    .retain(|i| i.invited_session_id != input.invited_session_id);
                project.invitations.push(ProjectInvitation {
                    invitation_id: uuid::Uuid::new_v4().to_string(),
                    invited_session_id: input.invited_session_id,
                    invited_by_session_id: session.clone(),
                    role: input.role,
                    expires_at: chrono::Utc::now().timestamp() + i64::from(input.ttl_seconds),
                });
                save_project(engine, &project).await?;
            }
            "revoke_member" => {
                let input: RevokeMemberInput = serde_json::from_value(args.clone())?;
                if input.session_id == project.owner_session_id {
                    bail!("cannot revoke the project owner");
                }
                project.members.retain(|m| m.session_id != input.session_id);
                project
                    .invitations
                    .retain(|i| i.invited_session_id != input.session_id);
                save_project(engine, &project).await?;
            }
            "select_agent" => {
                let input: SelectAgentInput = serde_json::from_value(args.clone())?;
                if !project.allowed_agents.contains(&input.agent_id)
                    || !known_agent_ids().contains(&input.agent_id)
                {
                    bail!("agent is not allowed for this project");
                }
                project.selected_agent = Some(input.agent_id);
                save_project(engine, &project).await?;
            }
            "put_document" => {
                let input: PutDocumentInput = serde_json::from_value(args.clone())?;
                if input.document_id.is_empty()
                    || input.document_id.len() > 128
                    || input.content.len() > 1_048_576
                    || input.media_type.is_empty()
                    || input.media_type.len() > 128
                {
                    bail!("invalid project document size or media type");
                }
                require_ready(&project).await?;
                let store = project_store(&project, false).await?;
                let document = ProjectDocument {
                    document_id: input.document_id,
                    collection: input.collection,
                    content: input.content,
                    media_type: input.media_type,
                    author_session_id: session.clone(),
                    updated_at: chrono::Utc::now().timestamp(),
                };
                let params = json!({"collection": serde_json::to_value(document.collection)?, "id":document.document_id, "document":serde_json::to_string(&document)?});
                tokio::task::spawn_blocking(move || store.run_query(
                    "?[collection, document_id, document] <- [[$collection, $id, $document]] :put project_documents {collection, document_id => document}", Some(params))).await??;
                return Ok(serde_json::to_value(DocumentOutput { document })?);
            }
            "list_documents" => {
                let input: ListDocumentsInput = serde_json::from_value(args.clone())?;
                require_ready(&project).await?;
                let store = project_store(&project, false).await?;
                let rows = tokio::task::spawn_blocking(move || store.run_query(
                    "?[document] := *project_documents{collection, document}, collection = $collection :limit 100",
                    Some(json!({"collection":input.collection})))).await??;
                let documents = rows
                    .as_array()
                    .ok_or_else(|| anyhow!("invalid project document rows"))?
                    .iter()
                    .map(|row| {
                        serde_json::from_str(
                            row["document"]
                                .as_str()
                                .ok_or_else(|| anyhow!("invalid project document"))?,
                        )
                        .map_err(Into::into)
                    })
                    .collect::<Result<Vec<ProjectDocument>>>()?;
                return Ok(serde_json::to_value(DocumentsOutput { documents })?);
            }
            _ => bail!("unknown project method"),
        }
    }
    Ok(serde_json::to_value(ProjectOutput {
        project: view_for(project, &session),
    })?)
}

async fn resolve_context(
    engine: &MutationEngine,
    session: &str,
    grants: &BTreeSet<String>,
    input: ResolveContextInput,
) -> Result<ResolvedChatContext> {
    if input.mode != ChatMode::Project && input.project_id.is_some() {
        bail!("project_id is only valid in project mode");
    }
    if input.mode != ChatMode::Personal && input.personal_namespace.is_some() {
        bail!("personal_namespace is only valid in personal mode");
    }
    let mut context = ResolvedChatContext {
        mode: input.mode,
        session_id: session.into(),
        project_id: None,
        memory_namespace: String::new(),
        read_only: input.mode == ChatMode::Accountability,
        effective_capabilities: BTreeSet::new(),
        allowed_agents: BTreeSet::new(),
        selected_agent: None,
    };
    match input.mode {
        ChatMode::Project => {
            let id = input
                .project_id
                .ok_or_else(|| anyhow!("project mode requires project_id"))?;
            let project = get_project(engine, &id).await?;
            context.effective_capabilities = project.effective_capabilities(session, grants)?;
            require_ready(&project).await?;
            context.read_only = project.role_for(session) == Some(ProjectRole::Reader);
            context.memory_namespace = format!("project/{id}");
            context.project_id = Some(id);
            context.allowed_agents = project.allowed_agents;
            context.selected_agent = project.selected_agent;
        }
        ChatMode::Personal => {
            context.memory_namespace = memory_namespace_for(
                ChatMode::Personal,
                session,
                None,
                input.personal_namespace.as_deref(),
            )?;
            context.effective_capabilities = grants
                .iter()
                .filter(|cap| {
                    matches!(
                        cap.as_str(),
                        "chat.context" | "cognitive_mcp.read" | "cognitive_mcp.memory.write"
                    )
                })
                .cloned()
                .collect();
        }
        ChatMode::ControlPlane => {
            if !grants.contains("chat.control_plane") {
                bail!("control-plane mode requires an explicit grant");
            }
            context.memory_namespace = format!("session/{session}/control_plane");
            context.effective_capabilities = grants
                .iter()
                .filter(|cap| {
                    matches!(
                        cap.as_str(),
                        "chat.context"
                            | "cognitive_mcp.read"
                            | "cognitive_mcp.memory.write"
                            | "workflows.read"
                            | "workflows.execute"
                    )
                })
                .cloned()
                .collect();
        }
        ChatMode::Accountability => {
            context.memory_namespace = format!("session/{session}/accountability");
            context.effective_capabilities = grants
                .iter()
                .filter(|cap| matches!(cap.as_str(), "chat.context" | "snowball.read"))
                .cloned()
                .collect();
        }
        ChatMode::NotebookLm => {
            // The partition is the notebook this session selected through the
            // notebooklm plugin; none selected → the mode cannot be entered.
            let notebook = selected_notebook(engine, session)
                .await?
                .ok_or_else(|| anyhow!("notebooklm mode requires a selected notebook"))?;
            context.memory_namespace =
                memory_namespace_for(ChatMode::NotebookLm, session, Some(&notebook), None)?;
            context.effective_capabilities = grants
                .iter()
                .filter(|cap| {
                    matches!(
                        cap.as_str(),
                        "chat.context"
                            | "cognitive_mcp.read"
                            | "cognitive_mcp.memory.write"
                            | "notebooklm.read"
                            | "notebooklm.query"
                    )
                })
                .cloned()
                .collect();
        }
        ChatMode::UiRender => {
            // No memory at all: nothing recalled, nothing written.
            context.memory_namespace = String::new();
            context.read_only = true;
            context.effective_capabilities = grants
                .iter()
                .filter(|cap| matches!(cap.as_str(), "chat.context"))
                .cloned()
                .collect();
        }
    }
    Ok(context)
}

async fn selected_notebook(engine: &MutationEngine, session: &str) -> Result<Option<String>> {
    let rows = catalog(
        engine,
        "?[notebook_id] := *notebooklm_selection{session_id, notebook_id}, session_id = $session",
        json!({"session": session}),
    )
    .await?;
    Ok(rows
        .as_array()
        .and_then(|rows| rows.first())
        .and_then(|row| row["notebook_id"].as_str())
        .filter(|id| !id.is_empty())
        .map(str::to_owned))
}

async fn resolve_selection(
    engine: &MutationEngine,
    session: &str,
    grants: &BTreeSet<String>,
    selection: &ChatContextSelection,
) -> Result<ResolvedChatContext> {
    resolve_context(
        engine,
        session,
        grants,
        ResolveContextInput {
            mode: selection.mode,
            project_id: selection.project_id.clone(),
            personal_namespace: selection.personal_namespace.clone(),
        },
    )
    .await
}

/// The session's persisted selection (personal when none was made).
pub(crate) async fn stored_selection(
    engine: &MutationEngine,
    session: &str,
) -> Result<ChatContextSelection> {
    let rows = catalog(
        engine,
        "?[document] := *chat_context_selection{session_id, document}, session_id = $session",
        json!({"session": session}),
    )
    .await?;
    let Some(document) = rows
        .as_array()
        .and_then(|rows| rows.first())
        .and_then(|row| row["document"].as_str())
    else {
        return Ok(ChatContextSelection::default());
    };
    Ok(serde_json::from_str(document)?)
}

/// What this session's memory calls and turns bind to right now. Re-resolved
/// on every use so a revoked membership or grant takes effect immediately;
/// a selection that no longer resolves fails closed (no silent fallback to
/// personal).
pub(crate) async fn current_chat_context(
    engine: &MutationEngine,
    session: &str,
    grants: &BTreeSet<String>,
) -> Result<ResolvedChatContext> {
    let selection = stored_selection(engine, session).await?;
    resolve_selection(engine, session, grants, &selection).await
}

#[cfg(test)]
#[path = "project_dispatch_tests.rs"]
mod tests;
