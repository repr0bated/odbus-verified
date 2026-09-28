use super::*;
use crate::{
    human_principal_dispatch::tests::{
        pk, register, temp_cozo, temp_shm_with_schemas, test_engine,
    },
    identity_sled_dispatch::{IdentitySledStore, tests::write_identity},
    oracle_assertion::HumanPrincipalIdentity,
};
use op_state::StatePlugin;

struct GrantsEnvironment(Option<std::ffi::OsString>);
impl Drop for GrantsEnvironment {
    fn drop(&mut self) {
        match self.0.take() {
            // FIXME: Audit that the environment access only happens in single-threaded code.
            Some(path) => unsafe { std::env::set_var("OP_GRANTS_PATH", path) },
            // FIXME: Audit that the environment access only happens in single-threaded code.
            None => unsafe { std::env::remove_var("OP_GRANTS_PATH") },
        }
    }
}

async fn invoke(
    engine: &MutationEngine,
    identity: &HumanPrincipalIdentity,
    method: &str,
    args: Value,
) -> Result<Value> {
    let schema = ProjectPlugin.schema().unwrap();
    let declaration = &schema.methods[method];
    let capability = schema.resolve_method_capability(declaration);
    let output = engine
        .dispatch_method_call_with_identity(
            "project",
            method,
            &args.to_string(),
            Some(capability.capability()),
            &identity.principal_id,
            Some(&identity.session_id),
            Some(&identity.session_genesis),
        )
        .await?["result"]
        .clone();
    jsonschema::Validator::new(&serde_json::to_value(
        declaration.returns.as_ref().unwrap(),
    )?)
    .map_err(|error| anyhow!("invalid project output schema: {error}"))?
    .validate(&output)
    .map_err(|error| anyhow!("invalid project output: {error}"))?;
    Ok(output)
}

#[tokio::test(flavor = "multi_thread")]
async fn project_membership_and_context_methods_enforce_durable_session_ownership() {
    let _cozo = temp_cozo();
    let _shm = temp_shm_with_schemas(&[
        op_plugins::state_plugins::human_principal::HumanPrincipalPlugin::new()
            .schema()
            .unwrap(),
        op_plugins::state_plugins::identity_sled::IdentitySledPlugin::new()
            .schema()
            .unwrap(),
        ProjectPlugin.schema().unwrap(),
    ]);
    let engine = test_engine();
    engine
        .install_identity_sled_store_for_test(IdentitySledStore::new(
            CozoGraphShuttle::new_in_memory().unwrap(),
        ))
        .unwrap();
    let root = tempfile::tempdir().unwrap();
    let _environment = GrantsEnvironment(std::env::var_os("OP_GRANTS_PATH"));
    let grants_path = root.path().join("grants.json");
    // FIXME: Audit that the environment access only happens in single-threaded code.
    unsafe { std::env::set_var("OP_GRANTS_PATH", &grants_path) };
    let mut identities = Vec::new();
    for (byte, name) in [(0xD5, "project-owner"), (0xD6, "project-member")] {
        let key = pk(byte);
        register(&key, name).await.unwrap();
        let sled = write_identity(&engine, &key).await.unwrap();
        identities.push(HumanPrincipalIdentity {
            principal_id: op_identity::session::derive_principal_id(&key),
            human_pubkey: key,
            session_id: sled.session_id,
            session_genesis: sled.genesis.unwrap(),
            expires_at: 0,
        });
    }
    let owner = &identities[0];
    let member = &identities[1];
    std::fs::write(&grants_path, serde_json::to_vec(&json!({
        (owner.principal_id.clone()): {"capabilities":["project.read","project.admin","chat.context","chat.control_plane","cognitive_mcp.read","cognitive_mcp.memory.write","snowball.read"]},
        (member.principal_id.clone()): {"capabilities":["project.read","project.admin","chat.context"]}
    })).unwrap()).unwrap();

    // Seed a trusted catalog fixture for access-control operations. This does
    // not claim provisioning, Incus verification or project document I/O.
    let id = uuid::Uuid::new_v4().to_string();
    let agent = known_agent_ids()
        .into_iter()
        .next()
        .expect("builtin agent catalog");
    let project = Project {
        project_id: id.clone(),
        title: "private project fixture".into(),
        owner_session_id: owner.session_id.clone(),
        container_id: id.clone(),
        status: ProjectStatus::Ready,
        persistence_path: root.path().join(&id).to_string_lossy().into_owned(),
        members: vec![],
        invitations: vec![],
        capability_ceiling: BTreeSet::from(["project.read".into(), "chat.context".into()]),
        allowed_agents: BTreeSet::from([agent.clone()]),
        selected_agent: None,
        created_at: chrono::Utc::now().timestamp(),
    };
    save_project(&engine, &project).await.unwrap();
    let args = json!({"project_id":id});
    let owned = invoke(&engine, owner, "get", args.clone()).await.unwrap();
    assert_eq!(owned["project"]["owner_session_id"], owner.session_id);
    assert!(invoke(&engine, member, "get", args.clone()).await.is_err());
    assert_eq!(
        invoke(&engine, member, "list", json!({})).await.unwrap()["projects"],
        json!([])
    );
    let listed = invoke(&engine, owner, "list", json!({})).await.unwrap();
    assert_eq!(listed["projects"].as_array().unwrap().len(), 1);
    assert_eq!(listed["projects"][0], owned["project"]);

    let selected = invoke(
        &engine,
        owner,
        "select_agent",
        json!({"project_id":id,"agent_id":agent}),
    )
    .await
    .unwrap();
    assert_eq!(selected["project"]["selected_agent"], agent);
    assert_eq!(
        get_project(&engine, &id)
            .await
            .unwrap()
            .selected_agent
            .as_deref(),
        Some(agent.as_str())
    );
    let invited = invoke(
        &engine,
        owner,
        "invite",
        json!({"project_id":id,
        "invited_session_id":member.session_id,"role":"contributor","ttl_seconds":300}),
    )
    .await
    .unwrap();
    let invitation = invited["project"]["invitations"][0]["invitation_id"]
        .as_str()
        .unwrap();
    let acceptance = json!({"project_id":id,"invitation_id":invitation});
    assert!(
        invoke(&engine, owner, "accept_invite", acceptance.clone())
            .await
            .is_err()
    );
    let accepted = invoke(&engine, member, "accept_invite", acceptance.clone())
        .await
        .unwrap();
    assert_eq!(
        accepted["project"]["members"][0]["session_id"],
        member.session_id
    );
    assert_eq!(accepted["project"]["members"][0]["role"], "contributor");
    assert_eq!(accepted["project"]["invitations"], json!([]));
    assert!(
        invoke(&engine, member, "accept_invite", acceptance)
            .await
            .is_err()
    );
    let durable = get_project(&engine, &id).await.unwrap();
    assert_eq!(
        durable.role_for(&member.session_id),
        Some(ProjectRole::Contributor)
    );
    assert!(durable.invitations.is_empty());
    assert_eq!(
        invoke(&engine, member, "list", json!({})).await.unwrap()["projects"][0]["project_id"],
        id
    );
    // A global admin capability does not grant membership administration.
    assert!(
        invoke(
            &engine,
            member,
            "select_agent",
            json!({"project_id":id,"agent_id":agent})
        )
        .await
        .is_err()
    );
    let revoked = invoke(
        &engine,
        owner,
        "revoke_member",
        json!({"project_id":id,"session_id":member.session_id}),
    )
    .await
    .unwrap();
    assert_eq!(revoked["project"]["members"], json!([]));
    assert!(
        get_project(&engine, &id)
            .await
            .unwrap()
            .role_for(&member.session_id)
            .is_none()
    );
    assert!(invoke(&engine, member, "get", args.clone()).await.is_err());
    assert!(
        invoke(
            &engine,
            owner,
            "revoke_member",
            json!({"project_id":id,"session_id":owner.session_id})
        )
        .await
        .is_err()
    );

    for (mode, suffix, read_only, caps) in [
        (
            "personal",
            "personal",
            false,
            json!([
                "chat.context",
                "cognitive_mcp.memory.write",
                "cognitive_mcp.read"
            ]),
        ),
        (
            "control_plane",
            "control_plane",
            false,
            json!([
                "chat.context",
                "cognitive_mcp.memory.write",
                "cognitive_mcp.read"
            ]),
        ),
        (
            "accountability",
            "accountability",
            true,
            json!(["chat.context", "snowball.read"]),
        ),
    ] {
        let context = invoke(&engine, owner, "resolve_context", json!({"mode":mode}))
            .await
            .unwrap();
        assert_eq!(context["session_id"], owner.session_id);
        assert_eq!(
            context["memory_namespace"],
            format!("session/{}/{suffix}", owner.session_id)
        );
        assert_eq!(context["read_only"], read_only);
        assert_eq!(context["effective_capabilities"], caps);
        assert_eq!(context["project_id"], Value::Null);
    }
    assert!(
        invoke(
            &engine,
            member,
            "resolve_context",
            json!({"mode":"control_plane"})
        )
        .await
        .is_err()
    );
    // OD-65: the selection is per session, persisted, re-resolved on read,
    // and every memory call binds to it (see memory_binding).
    let selected = invoke(
        &engine,
        owner,
        "select_context",
        json!({"mode":"personal","personal_namespace":"notes"}),
    )
    .await
    .unwrap();
    assert_eq!(
        selected["memory_namespace"],
        format!("session/{}/personal/notes", owner.session_id)
    );
    let current = invoke(&engine, owner, "get_context", json!({}))
        .await
        .unwrap();
    assert_eq!(current["memory_namespace"], selected["memory_namespace"]);
    // Nothing selected yet for the member: personal by default.
    let default = invoke(&engine, member, "get_context", json!({}))
        .await
        .unwrap();
    assert_eq!(
        default["memory_namespace"],
        format!("session/{}/personal", member.session_id)
    );
    // Invalid selections are refused before they are stored.
    for bad in [
        json!({"mode":"personal","personal_namespace":"Not Valid"}),
        json!({"mode":"project","personal_namespace":"notes"}),
        json!({"mode":"notebooklm"}),
        json!({"mode":"project","project_id":id}),
    ] {
        assert!(
            invoke(&engine, member, "select_context", bad.clone())
                .await
                .is_err(),
            "{bad}"
        );
    }
    // ui_render: no memory at all, and it persists like any other selection.
    let ui = invoke(
        &engine,
        owner,
        "select_context",
        json!({"mode":"ui_render"}),
    )
    .await
    .unwrap();
    assert_eq!(ui["memory_namespace"], "");
    assert_eq!(ui["read_only"], true);
    let ui_again = invoke(&engine, owner, "get_context", json!({}))
        .await
        .unwrap();
    assert_eq!(ui_again["mode"], "ui_render");
    let stored = crate::project_dispatch::stored_selection(&engine, &owner.session_id)
        .await
        .unwrap();
    assert_eq!(stored.mode, ChatMode::UiRender);
    // (Project mode selection needs a provisioned, Ready container — the
    // resolve path is covered by the project fixture above.)

    let mut forged = owner.clone();
    forged.session_id = member.session_id.clone();
    assert!(invoke(&engine, &forged, "get", args).await.is_err());
    let retained = get_project(&engine, &id).await.unwrap();
    assert_eq!(retained.owner_session_id, owner.session_id);
    assert_eq!(retained.selected_agent.as_deref(), Some(agent.as_str()));
    assert!(retained.members.is_empty());
}
