//! One chat-mode rule shared by both mode resolvers (Runtime
//! `runtime_workspace_project.rs` and MutationEngine `project_dispatch.rs`),
//! the exchange point and the tracker -> sled boundary
//! (owner, 2026-11-22; `.kiro/specs/identity-chat-interface` Requirement 34,
//! task 27.1/27.2).
//!
//! * A subsidiary identity has no Personal mode. Its default is ControlPlane
//!   when it holds `chat.control_plane`, otherwise Accountability.
//! * The configured control-plane chatbot defaults to ControlPlane.
//! * Every other identity keeps the documented default, Personal.
//! * Personal content never reaches the sled ledger (and so never the
//!   end-of-session vectors): personal-mode calls, writes into
//!   `session/<sid>/personal[/...]` and soul writes are excluded.

use op_plugins::state_plugins::project::ChatMode;
use serde_json::Value;
use std::collections::BTreeSet;

pub(crate) const CONTROL_PLANE_GRANT: &str = "chat.control_plane";

/// Every mode, in the order the exchange point lists them.
pub(crate) const ALL_MODES: [ChatMode; 6] = [
    ChatMode::Personal,
    ChatMode::Project,
    ChatMode::ControlPlane,
    ChatMode::Accountability,
    ChatMode::NotebookLm,
    ChatMode::UiRender,
];

/// What the asking session is, as far as mode admission is concerned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SessionKind {
    /// A human or ordinary identity container.
    Identity,
    /// A container whose native config names a parent principal.
    Subsidiary,
    /// The configured singleton control-plane chatbot.
    ControlPlaneChatbot,
}

/// Classify from the sled record's stored native instance and wireguard key.
/// `instance_json` is the committed `IncusInstance` binding.
pub(crate) fn classify(instance_json: Option<&str>, wireguard_pubkey: Option<&str>) -> SessionKind {
    if let (Some(key), Some(configured)) = (
        wireguard_pubkey,
        crate::identity_binding::configured_chatbot_key(),
    ) && key == configured
    {
        return SessionKind::ControlPlaneChatbot;
    }
    let parent = instance_json
        .and_then(|json| serde_json::from_str::<Value>(json).ok())
        .and_then(|instance| {
            instance["config"]["user.opdbus.parent_principal_id"]
                .as_str()
                .map(str::to_owned)
        });
    if parent.is_some_and(|parent| !parent.trim().is_empty()) {
        SessionKind::Subsidiary
    } else {
        SessionKind::Identity
    }
}

/// Kind and native instance name of `session`, from its committed sled record
/// in the durable identity owner. No record: an ordinary identity.
pub(crate) fn session_identity(
    store: &op_cozo_store::CozoGraphShuttle,
    session: &str,
) -> anyhow::Result<(SessionKind, Option<String>)> {
    Ok(match store.get_identity_sled(session)? {
        None => (SessionKind::Identity, None),
        Some(record) => {
            let name = serde_json::from_str::<Value>(&record.instance_json)
                .ok()
                .and_then(|instance| instance["name"].as_str().map(str::to_owned));
            (
                classify(Some(&record.instance_json), Some(&record.wireguard_pubkey)),
                name,
            )
        }
    })
}

/// Mode-level admission. Per-mode grant/ACL checks stay in the resolvers;
/// this only removes what a kind of session may never select.
pub(crate) fn admissible(mode: ChatMode, grants: &BTreeSet<String>, kind: SessionKind) -> bool {
    match mode {
        ChatMode::Personal => kind != SessionKind::Subsidiary,
        ChatMode::ControlPlane => grants.contains(CONTROL_PLANE_GRANT),
        ChatMode::Project
        | ChatMode::Accountability
        | ChatMode::NotebookLm
        | ChatMode::UiRender => true,
    }
}

/// The modes the exchange point may offer this session (same rule).
pub(crate) fn allowed_modes(grants: &BTreeSet<String>, kind: SessionKind) -> Vec<ChatMode> {
    ALL_MODES
        .into_iter()
        .filter(|mode| admissible(*mode, grants, kind))
        .collect()
}

/// Mode used when the session has stored no selection.
pub(crate) fn default_mode(grants: &BTreeSet<String>, kind: SessionKind) -> ChatMode {
    match kind {
        SessionKind::Identity => ChatMode::Personal,
        SessionKind::Subsidiary | SessionKind::ControlPlaneChatbot
            if grants.contains(CONTROL_PLANE_GRANT) =>
        {
            ChatMode::ControlPlane
        }
        SessionKind::Subsidiary => ChatMode::Accountability,
        // The chatbot without the grant has no control plane; fall back to the
        // documented default rather than inventing a mode.
        SessionKind::ControlPlaneChatbot => ChatMode::Personal,
    }
}

/// The effective mode for ledger routing when grants are not at hand: only
/// "is it Personal" matters there, and every non-Identity default is
/// non-personal except the chatbot without its grant (treated as personal,
/// fail closed).
pub(crate) fn effective_mode_is_personal(stored: Option<ChatMode>, kind: SessionKind) -> bool {
    match stored {
        Some(mode) => mode == ChatMode::Personal,
        None => kind == SessionKind::Identity,
    }
}

fn names_personal(value: Option<&Value>, personal_root: &str) -> bool {
    value.and_then(Value::as_str).is_some_and(|namespace| {
        namespace == personal_root
            || namespace
                .strip_prefix(personal_root)
                .is_some_and(|rest| rest.starts_with('/'))
    })
}

/// Soul state is personal by definition (`op-cognitive` stores it under key
/// and tag `soul`; the assistant client uses namespace `assistant:soul`).
pub(crate) fn is_soul_write(method: &str, args: &Value) -> bool {
    method.contains("soul")
        || args["key"].as_str() == Some("soul")
        || args["namespace"].as_str() == Some("assistant:soul")
        || args["tags"]
            .as_array()
            .is_some_and(|tags| tags.iter().any(|tag| tag.as_str() == Some("soul")))
}

/// True when this committed call must not be appended to the sled ledger.
pub(crate) fn excluded_from_sled(
    personal_mode: bool,
    session: &str,
    method: &str,
    args: &Value,
) -> bool {
    if personal_mode {
        return true;
    }
    let personal_root = format!("session/{session}/personal");
    names_personal(args.get("namespace"), &personal_root)
        || names_personal(args.get("container_id"), &personal_root)
        || names_personal(args.get("memory_namespace"), &personal_root)
        || is_soul_write(method, args)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn grants(list: &[&str]) -> BTreeSet<String> {
        list.iter().map(|grant| (*grant).to_owned()).collect()
    }

    #[test]
    fn subsidiary_never_gets_personal() {
        let cp = grants(&[CONTROL_PLANE_GRANT]);
        assert!(!admissible(
            ChatMode::Personal,
            &cp,
            SessionKind::Subsidiary
        ));
        assert!(!allowed_modes(&cp, SessionKind::Subsidiary).contains(&ChatMode::Personal));
        assert_eq!(
            default_mode(&cp, SessionKind::Subsidiary),
            ChatMode::ControlPlane
        );
        assert_eq!(
            default_mode(&grants(&[]), SessionKind::Subsidiary),
            ChatMode::Accountability
        );
        assert!(!effective_mode_is_personal(None, SessionKind::Subsidiary));
    }

    #[test]
    fn identity_keeps_documented_personal_default() {
        let none = grants(&[]);
        assert_eq!(
            default_mode(&none, SessionKind::Identity),
            ChatMode::Personal
        );
        assert!(admissible(ChatMode::Personal, &none, SessionKind::Identity));
        assert!(!admissible(
            ChatMode::ControlPlane,
            &none,
            SessionKind::Identity
        ));
        assert!(effective_mode_is_personal(None, SessionKind::Identity));
    }

    #[test]
    fn chatbot_defaults_to_control_plane_with_its_grant() {
        let cp = grants(&[CONTROL_PLANE_GRANT]);
        assert_eq!(
            default_mode(&cp, SessionKind::ControlPlaneChatbot),
            ChatMode::ControlPlane
        );
        assert!(!effective_mode_is_personal(
            None,
            SessionKind::ControlPlaneChatbot
        ));
    }

    #[test]
    fn subsidiary_classified_from_native_parent() {
        let instance =
            json!({"name":"kiroween-subsidiary","config":{"user.opdbus.parent_principal_id":"p"}})
                .to_string();
        assert_eq!(classify(Some(&instance), None), SessionKind::Subsidiary);
        let plain = json!({"name":"x","config":{}}).to_string();
        assert_eq!(classify(Some(&plain), None), SessionKind::Identity);
        assert_eq!(classify(None, None), SessionKind::Identity);
    }

    #[test]
    fn personal_content_is_excluded_from_the_sled() {
        let sid = "2a2d9aa8-5468-309c-2db4-342221450de8";
        assert!(excluded_from_sled(
            true,
            sid,
            "chat_conversations.submit_turn",
            &json!({})
        ));
        assert!(excluded_from_sled(
            false,
            sid,
            "memory_store",
            &json!({"namespace": format!("session/{sid}/personal")})
        ));
        assert!(excluded_from_sled(
            false,
            sid,
            "memory_store",
            &json!({"namespace": format!("session/{sid}/personal/notes")})
        ));
        assert!(excluded_from_sled(
            false,
            sid,
            "memory_store",
            &json!({"key":"soul"})
        ));
        assert!(excluded_from_sled(
            false,
            sid,
            "memory_store",
            &json!({"tags":["soul","chatbot"]})
        ));
        assert!(excluded_from_sled(false, sid, "upsert_soul", &json!({})));
        // Not personal: another prefix, or control-plane memory.
        assert!(!excluded_from_sled(
            false,
            sid,
            "memory_store",
            &json!({"namespace": format!("session/{sid}/personalx")})
        ));
        assert!(!excluded_from_sled(
            false,
            sid,
            "memory_store",
            &json!({"namespace": format!("session/{sid}/control_plane")})
        ));
        assert!(!excluded_from_sled(
            false,
            sid,
            "workflows.execute",
            &json!({"name":"x"})
        ));
    }
}
