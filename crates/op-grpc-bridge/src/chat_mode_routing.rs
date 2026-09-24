//! Resolve the model route for a verified session's selected chat mode.
//!
//! Dashboard selection remains authoritative for ordinary user modes. The
//! control-plane and accountability surfaces use separate native router agent
//! aliases so those roles cannot inherit a user's mutable dashboard route.

use anyhow::Result;
use op_plugins::state_plugins::{
    project::{ChatMode, ResolvedChatContext},
    tched_router::ChatInput,
};
use std::collections::BTreeSet;

use crate::{
    mutation_engine::MutationEngine, project_dispatch, specialist::CognitiveCaller,
    tched_router_runtime::TchedRouterRuntimeClient,
};

pub(crate) const CONTROL_PLANE_AGENT: &str = "control_plane";
pub(crate) const ACCOUNTABILITY_AGENT: &str = "accountability";

pub(crate) struct ResolvedChatRoute {
    pub(crate) mode: ChatMode,
    pub(crate) context: ResolvedChatContext,
    pub(crate) client: TchedRouterRuntimeClient,
    pub(crate) input: ChatInput,
}

pub(crate) fn dedicated_alias(mode: ChatMode) -> Option<&'static str> {
    match mode {
        ChatMode::ControlPlane => Some(CONTROL_PLANE_AGENT),
        ChatMode::Accountability => Some(ACCOUNTABILITY_AGENT),
        ChatMode::Personal | ChatMode::Project | ChatMode::NotebookLm | ChatMode::UiRender => None,
    }
}

fn apply_mode_route(mode: ChatMode, mut input: ChatInput) -> (Option<&'static str>, ChatInput) {
    if dedicated_alias(mode).is_some() {
        // Role routes inherit their dedicated native provider profiles,
        // never a dashboard override. Control Plane may use any configured
        // provider selected by its operator.
        input.provider.clear();
        input.model.clear();
    }
    (dedicated_alias(mode), input)
}

/// Resolve the route after re-reading the caller's persisted mode. Normal
/// modes retain the caller's explicit provider/model; role modes replace them
/// with their dedicated native route. Control-plane classification happens
/// after the caller has supplied its original generation admission.
pub(crate) async fn resolve(
    engine: &MutationEngine,
    caller: CognitiveCaller<'_>,
    input: ChatInput,
) -> Result<ResolvedChatRoute> {
    let grants: BTreeSet<String> = crate::interceptor::load_capability_grants(caller.actor_id)
        .into_iter()
        .collect();
    let context = project_dispatch::current_chat_context(
        engine,
        caller
            .session_id
            .ok_or_else(|| anyhow::anyhow!("verified session is required"))?,
        &grants,
    )
    .await?;
    let (alias, input) = apply_mode_route(context.mode, input);
    let (mode, client) = match alias {
        Some(alias) => (
            context.mode,
            TchedRouterRuntimeClient::for_agent_alias(alias),
        ),
        None => (context.mode, TchedRouterRuntimeClient::from_env()),
    };
    let sealed_identity = crate::native_sled_identity::credential_for(
        caller.actor_id,
        caller
            .session_id
            .ok_or_else(|| anyhow::anyhow!("verified session is required"))?,
        caller
            .session_genesis
            .ok_or_else(|| anyhow::anyhow!("verified genesis is required"))?,
    )?;
    Ok(ResolvedChatRoute {
        mode,
        context,
        client: client.with_sealed_identity(Some(&sealed_identity)),
        input,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_routes_have_distinct_native_aliases() {
        assert_ne!(CONTROL_PLANE_AGENT, ACCOUNTABILITY_AGENT);
    }

    #[test]
    fn only_role_modes_override_the_dashboard_route() {
        assert_eq!(dedicated_alias(ChatMode::Personal), None);
        assert_eq!(dedicated_alias(ChatMode::Project), None);
        assert_eq!(dedicated_alias(ChatMode::NotebookLm), None);
        assert_eq!(dedicated_alias(ChatMode::UiRender), None);
        assert_eq!(
            dedicated_alias(ChatMode::ControlPlane),
            Some(CONTROL_PLANE_AGENT)
        );
        assert_eq!(
            dedicated_alias(ChatMode::Accountability),
            Some(ACCOUNTABILITY_AGENT)
        );
    }

    #[test]
    fn normal_modes_preserve_explicit_dashboard_selection() {
        let input = ChatInput {
            provider: "custom.salad".into(),
            model: "selected-model".into(),
            ..ChatInput::default()
        };
        let (_, routed) = apply_mode_route(ChatMode::Personal, input);
        assert_eq!(routed.provider, "custom.salad");
        assert_eq!(routed.model, "selected-model");
    }

    #[test]
    fn role_modes_override_explicit_dashboard_selection() {
        let input = ChatInput {
            provider: "custom.salad".into(),
            model: "user-selected-model".into(),
            ..ChatInput::default()
        };
        let (alias, routed) = apply_mode_route(ChatMode::Accountability, input);
        assert_eq!(alias, Some(ACCOUNTABILITY_AGENT));
        assert!(routed.provider.is_empty());
        assert!(routed.model.is_empty());
    }
}
