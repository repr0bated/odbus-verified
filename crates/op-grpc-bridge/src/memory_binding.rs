//! Bind every memory call to the principal's selected chat mode (OD-65).
//!
//! Owner rule: the scope is always the principal; projects, notebooks and
//! personal sub-spaces are partitions the principal controls. So a
//! `memory_*` call never chooses its namespace — the session's selected
//! mode (`project.select_context`) does, re-resolved on every call:
//!
//! | mode           | namespace                                   | writes |
//! |----------------|---------------------------------------------|--------|
//! | personal       | `session/{s}/personal[/{user-defined}]`     | yes    |
//! | project        | `project/{id}` (member; Reader = read-only) | role   |
//! | control_plane  | `session/{s}/control_plane` (grant)         | yes    |
//! | accountability | `session/{s}/accountability`                | no     |
//! | notebooklm     | `session/{s}/notebooklm/{notebook}`         | yes    |
//! | ui_render      | — (no memory at all)                        | no     |
//!
//! A caller-supplied `namespace` is honoured only in personal mode and only
//! as a bare sub-name; anything else is replaced, never trusted.

use std::collections::BTreeSet;

use anyhow::{bail, Result};
use op_plugins::state_plugins::project::{
    namespace_prefix_for_session, validate_personal_namespace, ChatMode, ResolvedChatContext,
};

use crate::mutation_engine::MutationEngine;
use crate::specialist::PIECES_NAMESPACE;

/// The memory methods the binding governs.
pub(crate) const MEMORY_METHODS: &[&str] = &[
    "memory_store",
    "memory_recall",
    "memory_query",
    "memory_retrieve",
    "memory_delete",
    "memory_list_namespaces",
    "memory_semantic_query",
];

const WRITE_METHODS: &[&str] = &["memory_store", "memory_delete"];

pub(crate) fn is_memory_method(method: &str) -> bool {
    MEMORY_METHODS.contains(&method)
}

/// Resolve the caller's current context. No verified session → no memory.
pub(crate) async fn context_for(
    engine: &MutationEngine,
    actor_id: &str,
    session_id: Option<&str>,
) -> Result<ResolvedChatContext> {
    let Some(session) = session_id.filter(|s| !s.is_empty()) else {
        bail!("memory requires a verified session");
    };
    let grants: BTreeSet<String> = crate::interceptor::load_capability_grants(actor_id)
        .into_iter()
        .collect();
    crate::project_dispatch::current_chat_context(engine, session, &grants).await
}

/// Rewrite `args` so the tool acts only inside the bound namespace.
/// Returns the namespace the call was bound to.
pub(crate) fn bind_args(
    method: &str,
    context: &ResolvedChatContext,
    args: &mut serde_json::Value,
) -> Result<String> {
    if !context.mode.has_memory() {
        bail!("memory is disabled in ui_render mode");
    }
    if context.read_only && WRITE_METHODS.contains(&method) {
        bail!("memory is read-only in this mode");
    }
    let namespace = match (context.mode, args.get("namespace").and_then(|v| v.as_str())) {
        // Personal mode: a bare sub-name selects a user-defined partition
        // under the principal. A path is not a name.
        (ChatMode::Personal, Some(requested)) if !requested.contains('/') => {
            validate_personal_namespace(requested)?;
            format!(
                "{}personal/{requested}",
                namespace_prefix_for_session(&context.session_id)
            )
        }
        // The exact bound namespace is fine to repeat.
        (_, Some(requested)) if requested == context.memory_namespace => {
            context.memory_namespace.clone()
        }
        // Anything else the caller named is replaced, not trusted.
        (_, Some(requested)) => {
            tracing::info!(
                requested,
                bound = %context.memory_namespace,
                "memory namespace request replaced by the selected mode"
            );
            context.memory_namespace.clone()
        }
        (_, None) => context.memory_namespace.clone(),
    };
    if let Some(object) = args.as_object_mut() {
        object.insert("namespace".into(), namespace.clone().into());
        // `container_id` derives a namespace inside the tool; the binding is
        // the only source of truth, so it carries the same value.
        object.insert("container_id".into(), namespace.clone().into());
    }
    Ok(namespace)
}

/// Namespaces the caller may see in a listing: everything under the
/// principal, the bound partition (project/notebook), and the shared pieces.
pub(crate) fn visible_namespace(context: &ResolvedChatContext, name: &str) -> bool {
    name == PIECES_NAMESPACE
        || name == context.memory_namespace
        || name.starts_with(&namespace_prefix_for_session(&context.session_id))
}

/// Filter a `list_namespaces` result to what the caller may see.
pub(crate) fn filter_listing(context: &ResolvedChatContext, value: &mut serde_json::Value) {
    let keep = |entry: &serde_json::Value| -> bool {
        let name = entry
            .get("name")
            .or_else(|| entry.get("namespace"))
            .and_then(|v| v.as_str())
            .or_else(|| entry.as_str());
        name.is_some_and(|n| visible_namespace(context, n))
    };
    for key in ["namespaces", "data", "result"] {
        if let Some(items) = value.get_mut(key).and_then(|v| v.as_array_mut()) {
            items.retain(|entry| keep(entry));
            return;
        }
    }
    if let Some(items) = value.as_array_mut() {
        items.retain(|entry| keep(entry));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context(mode: ChatMode, namespace: &str, read_only: bool) -> ResolvedChatContext {
        ResolvedChatContext {
            mode,
            session_id: "sess".into(),
            project_id: None,
            memory_namespace: namespace.into(),
            read_only,
            effective_capabilities: BTreeSet::new(),
            allowed_agents: BTreeSet::new(),
            selected_agent: None,
        }
    }

    #[test]
    fn callers_never_choose_a_namespace_outside_their_mode() {
        let personal = context(ChatMode::Personal, "session/sess/personal", false);
        let mut args = serde_json::json!({"key": "k", "value": "v", "namespace": "notes"});
        assert_eq!(
            bind_args("memory_store", &personal, &mut args).unwrap(),
            "session/sess/personal/notes"
        );
        assert_eq!(args["container_id"], "session/sess/personal/notes");

        let mut escape = serde_json::json!({"namespace": "project/other"});
        assert_eq!(
            bind_args("memory_query", &personal, &mut escape).unwrap(),
            "session/sess/personal"
        );
        let mut traversal = serde_json::json!({"namespace": "../secrets"});
        assert_eq!(
            bind_args("memory_query", &personal, &mut traversal).unwrap(),
            "session/sess/personal"
        );
        assert!(bind_args(
            "memory_store",
            &personal,
            &mut serde_json::json!({"namespace": "Bad Name"})
        )
        .is_err());

        let project = context(ChatMode::Project, "project/p1", false);
        let mut named = serde_json::json!({"namespace": "notes"});
        assert_eq!(
            bind_args("memory_store", &project, &mut named).unwrap(),
            "project/p1",
            "sub-names exist only in personal mode"
        );
    }

    #[test]
    fn read_only_and_ui_render_refuse_as_the_mode_says() {
        let accountability = context(
            ChatMode::Accountability,
            "session/sess/accountability",
            true,
        );
        assert!(bind_args("memory_store", &accountability, &mut serde_json::json!({})).is_err());
        assert!(bind_args("memory_query", &accountability, &mut serde_json::json!({})).is_ok());
        let ui = context(ChatMode::UiRender, "", true);
        for method in MEMORY_METHODS {
            assert!(
                bind_args(method, &ui, &mut serde_json::json!({})).is_err(),
                "{method}"
            );
        }
    }

    #[test]
    fn listings_show_only_the_principal_partitions_and_shared_pieces() {
        let project = context(ChatMode::Project, "project/p1", false);
        let mut listing = serde_json::json!({"namespaces": [
            {"name": "session/sess/personal"},
            {"name": "session/sess/personal/notes"},
            {"name": "project/p1"},
            {"name": "project/p2"},
            {"name": "session/other/personal"},
            {"name": "agent_pieces"},
        ]});
        filter_listing(&project, &mut listing);
        let names: Vec<&str> = listing["namespaces"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["name"].as_str().unwrap())
            .collect();
        assert_eq!(
            names,
            [
                "session/sess/personal",
                "session/sess/personal/notes",
                "project/p1",
                "agent_pieces"
            ]
        );
    }
}
