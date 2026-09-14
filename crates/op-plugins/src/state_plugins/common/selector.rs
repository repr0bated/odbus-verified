//! Dynamic model selection (spec §4) — the Orchestration Layer's pure selector.
//!
//! `select_model` reads **only** from the in-memory `TchedRouterState`
//! (`model_routes`, `tools`, `selector_policy`) handed to it. It performs no
//! I/O, no env reads, no D-Bus calls, and contains **no `match` arm on provider
//! or model name** — selection is entirely data-driven from schema state
//! (REQ-03). Cost is compared via the 3tched Router-native `cost_profile`
//! budget-class string (ordinal rank), never `cost_per_token` or currency math.
//!
//! Determinism: given identical `state` and `input`, the selected route,
//! ordering, and confidence are identical. The non-deterministic audit fields
//! (`trace_id`, `timestamp`) are left empty here and populated by the D-Bus
//! method handler (T-21), so the selector itself stays pure.

use crate::state_plugins::common::errors::TchedRouterError;
use crate::state_plugins::common::llm_projection::{
    ModelRoute, SelectionEvent, SelectionInput, SelectionOutput, SelectorPolicy,
};
use crate::state_plugins::tched_router::TchedRouterState;

const SELECTOR_VERSION: &str = "1.0.0";

/// Ordinal for an effort budget class. Unknown values map to the middle so an
/// unannotated route is neither strongly preferred nor strongly penalized.
fn effort_ord(level: &str) -> i32 {
    match level.trim().to_ascii_lowercase().as_str() {
        "none" | "off" => 0,
        "low" | "minimal" => 1,
        "medium" | "moderate" | "" => 2,
        "high" => 3,
        "max" | "maximum" => 4,
        _ => 2,
    }
}

/// Ordinal for a cost budget class (unit-neutral; higher = more expensive).
fn cost_ord(profile: &str) -> i32 {
    match profile.trim().to_ascii_lowercase().as_str() {
        "free" | "none" => 0,
        "low" | "cheap" | "budget" => 1,
        "medium" | "standard" | "" => 2,
        "high" | "expensive" => 3,
        "premium" | "max" => 4,
        _ => 2,
    }
}

/// Ordinal for a privacy tier (higher = more sensitive / more restricted).
fn privacy_ord(tier: &str) -> i32 {
    match tier.trim().to_ascii_lowercase().as_str() {
        "public" | "" => 0,
        "internal" => 1,
        "confidential" | "private" => 2,
        "restricted" | "secret" => 3,
        _ => 1,
    }
}

/// Ordinal for a latency class (higher = slower).
fn latency_ord(class: &str) -> i32 {
    match class.trim().to_ascii_lowercase().as_str() {
        "realtime" | "fast" => 0,
        "interactive" | "" => 1,
        "standard" | "normal" => 2,
        "batch" | "slow" => 3,
        _ => 1,
    }
}

/// A scored candidate retained for ordering and fallback construction.
struct Scored<'a> {
    route: &'a ModelRoute,
    score: f64,
}

/// Select a provider/model from live schema state.
pub fn select_model(
    input: &SelectionInput,
    state: &TchedRouterState,
) -> Result<SelectionOutput, TchedRouterError> {
    let routes = &state.catalog.model_routes;
    let policy = &state.selector_policy;

    // REQ-02: a requested tool that is not declared in the global tool catalog
    // is rejected before any route scoring or network I/O.
    let declared_tools: Vec<&str> = state
        .catalog
        .tools
        .iter()
        .map(|t| t.name.as_str())
        .collect();
    for need in &input.tool_needs {
        if !declared_tools.iter().any(|t| t == need) {
            return Err(TchedRouterError::ToolNotDeclared { tool: need.clone() });
        }
    }

    // Explicit override: caller named a provider/model. It must be declared and
    // must itself pass the hard filters, otherwise a specific error is returned
    // (rather than silently selecting a different route).
    if input.explicit_provider.is_some() || input.explicit_model.is_some() {
        return select_explicit(input, routes, policy);
    }

    let mut scored: Vec<Scored> = Vec::new();
    for route in routes {
        if hard_filter(route, input).is_ok() {
            let score = score_route(route, input, policy);
            scored.push(Scored { route, score });
        }
    }

    if scored.is_empty() {
        return Err(TchedRouterError::NoCandidateAfterFiltering);
    }

    order_candidates(&mut scored);
    Ok(build_output(
        &scored,
        "best-scoring declared route after hard filters",
    ))
}

/// Resolve an explicit provider/model request against declared routes.
fn select_explicit(
    input: &SelectionInput,
    routes: &[ModelRoute],
    policy: &SelectorPolicy,
) -> Result<SelectionOutput, TchedRouterError> {
    let matches: Vec<&ModelRoute> = routes
        .iter()
        .filter(|r| {
            input
                .explicit_provider
                .as_ref()
                .map(|p| &r.provider == p)
                .unwrap_or(true)
                && input
                    .explicit_model
                    .as_ref()
                    .map(|m| &r.model == m)
                    .unwrap_or(true)
        })
        .collect();

    if matches.is_empty() {
        if let Some(provider) = &input.explicit_provider {
            if !routes.iter().any(|r| &r.provider == provider) {
                return Err(TchedRouterError::ProviderNotDeclared {
                    provider: provider.clone(),
                });
            }
        }
        if let Some(model) = &input.explicit_model {
            return Err(TchedRouterError::ModelNotDeclared {
                model: model.clone(),
            });
        }
        return Err(TchedRouterError::NoCandidateAfterFiltering);
    }

    // The explicit candidate(s) must still satisfy the hard filters; surface the
    // first specific failure so the caller knows why the override was rejected.
    let mut scored: Vec<Scored> = Vec::new();
    let mut last_err: Option<TchedRouterError> = None;
    for route in matches {
        match hard_filter(route, input) {
            Ok(()) => scored.push(Scored {
                route,
                score: score_route(route, input, policy),
            }),
            Err(e) => last_err = Some(e),
        }
    }

    if scored.is_empty() {
        return Err(last_err.unwrap_or(TchedRouterError::NoCandidateAfterFiltering));
    }

    order_candidates(&mut scored);
    Ok(build_output(
        &scored,
        "explicit provider/model authorized by schema",
    ))
}

/// Phase 1 hard filters (design §4). Each returns a specific error so explicit
/// requests can report why they were rejected; aggregate selection treats any
/// `Err` as "exclude this route".
fn hard_filter(route: &ModelRoute, input: &SelectionInput) -> Result<(), TchedRouterError> {
    if !route.available {
        return Err(TchedRouterError::RouteUnavailable {
            hint: route.hint.clone(),
            reason: if route.status_reason.is_empty() {
                "route marked unavailable".to_string()
            } else {
                route.status_reason.clone()
            },
        });
    }

    if privacy_ord(&route.privacy_tier) > privacy_ord(&input.privacy_tier) {
        return Err(TchedRouterError::PrivacyTierViolation {
            required: input.privacy_tier.clone(),
            provided: route.privacy_tier.clone(),
        });
    }

    for need in &input.tool_needs {
        if !route.tool_support.iter().any(|t| t == need) {
            return Err(TchedRouterError::ToolNotDeclared { tool: need.clone() });
        }
    }

    if route.context_window > 0 && route.context_window < input.context_tokens {
        return Err(TchedRouterError::ContextWindowExceeded {
            limit: route.context_window,
            requested: input.context_tokens,
        });
    }

    if let Some(policy) = &input.cost_policy {
        if cost_ord(&route.cost_profile) > cost_ord(policy) {
            return Err(TchedRouterError::CostPolicyViolation {
                policy: policy.clone(),
                reason: format!(
                    "route cost_profile `{}` exceeds ceiling",
                    route.cost_profile
                ),
            });
        }
    }

    Ok(())
}

/// Phase 2 scoring (design §4). Higher is better. No `match` on provider/model
/// name — all inputs are budget-class ordinals and schema-declared values.
fn score_route(route: &ModelRoute, input: &SelectionInput, policy: &SelectorPolicy) -> f64 {
    let requested_effort = if input.requested_effort.is_empty() {
        policy.default_effort.as_str()
    } else {
        input.requested_effort.as_str()
    };

    // Capability match: a route whose declared kind/hint aligns with the task
    // class is preferred. Pure string comparison, no provider-name arms.
    let capability_match_bonus = if route.kind.eq_ignore_ascii_case(&input.task_class)
        || route.hint.eq_ignore_ascii_case(&input.task_class)
    {
        2.0
    } else {
        0.0
    };

    // Effort: reward effort up to the requested ceiling; penalize exceeding it.
    let r_eff = effort_ord(&route.effort_level);
    let req_eff = effort_ord(requested_effort);
    let effort_quality_score = if r_eff <= req_eff {
        r_eff as f64
    } else {
        -((r_eff - req_eff) as f64) * 2.0
    };

    let cost_term = policy.cost_weight * cost_ord(&route.cost_profile) as f64;

    let latency_penalty = match input.latency_target.as_deref() {
        Some(target) if !target.is_empty() => {
            policy.latency_weight
                * latency_ord(&route.latency_class) as f64
                * latency_ord(target).max(1) as f64
                / latency_ord("interactive").max(1) as f64
        }
        _ => 0.0,
    };

    let route_health_bonus =
        policy.health_weight * route.health_score + if route.available { 0.1 } else { 0.0 };

    capability_match_bonus + policy.effort_weight * effort_quality_score
        - cost_term
        - latency_penalty
        + route_health_bonus
}

/// Order by score desc; ties broken deterministically by cheaper cost, then
/// route hint, then model, so equal-capability routes pick the lowest cost.
fn order_candidates(scored: &mut [Scored]) {
    scored.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| cost_ord(&a.route.cost_profile).cmp(&cost_ord(&b.route.cost_profile)))
            .then_with(|| a.route.hint.cmp(&b.route.hint))
            .then_with(|| a.route.model.cmp(&b.route.model))
    });
}

fn build_output(scored: &[Scored], reason: &str) -> SelectionOutput {
    let best = &scored[0];
    let confidence = if scored.len() == 1 {
        1.0
    } else {
        let margin = best.score - scored[1].score;
        (0.5 + margin / (best.score.abs() + 1.0)).clamp(0.0, 1.0)
    };
    let fallback_routes = scored
        .iter()
        .skip(1)
        .map(|s| s.route.hint.clone())
        .collect();

    SelectionOutput {
        selected_provider: best.route.provider.clone(),
        selected_model: best.route.model.clone(),
        reason: reason.to_string(),
        estimated_cost: if best.route.cost_profile.is_empty() {
            None
        } else {
            Some(best.route.cost_profile.clone())
        },
        effort_level: best.route.effort_level.clone(),
        confidence,
        fallback_routes,
        // trace_id/timestamp are filled by the D-Bus handler (keeps this pure).
        event_metadata: SelectionEvent {
            trace_id: String::new(),
            timestamp: String::new(),
            selector_version: SELECTOR_VERSION.to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state_plugins::tched_router::TchedRouterPlugin;

    #[allow(clippy::too_many_arguments)]
    fn route(
        hint: &str,
        provider: &str,
        model: &str,
        available: bool,
        cost: &str,
        effort: &str,
        privacy: &str,
        ctx: u32,
        health: f64,
    ) -> ModelRoute {
        ModelRoute {
            hint: hint.to_string(),
            provider: provider.to_string(),
            upstream_provider: provider.to_string(),
            transport: "direct".to_string(),
            model: model.to_string(),
            kind: "chat".to_string(),
            status: "declared".to_string(),
            available,
            status_reason: String::new(),
            source: None,
            api_key: None,
            cost_profile: cost.to_string(),
            effort_level: effort.to_string(),
            latency_class: "interactive".to_string(),
            privacy_tier: privacy.to_string(),
            context_window: ctx,
            health_score: health,
            fallback_routes: vec![],
            tool_support: vec![],
        }
    }

    fn state_with(routes: Vec<ModelRoute>) -> TchedRouterState {
        let mut s = TchedRouterPlugin::current_state();
        s.catalog.model_routes = routes;
        s
    }

    fn input() -> SelectionInput {
        SelectionInput {
            task_class: "chat".to_string(),
            requested_effort: "high".to_string(),
            cost_policy: None,
            latency_target: None,
            context_tokens: 1000,
            privacy_tier: "confidential".to_string(),
            tool_needs: vec![],
            explicit_provider: None,
            explicit_model: None,
        }
    }

    #[test]
    fn should_select_lowest_cost_for_equivalent_capability() {
        let routes = vec![
            route("a", "p1", "m1", true, "high", "high", "public", 8000, 0.9),
            route("b", "p2", "m2", true, "low", "high", "public", 8000, 0.9),
        ];
        let out = select_model(&input(), &state_with(routes)).unwrap();
        assert_eq!(out.selected_provider, "p2");
        assert_eq!(out.selected_model, "m2");
    }

    #[test]
    fn should_respect_effort_ceiling() {
        let mut inp = input();
        inp.requested_effort = "low".to_string();
        // Equal cost; the route exceeding the effort ceiling must score lower.
        let routes = vec![
            route("over", "p1", "m1", true, "low", "max", "public", 8000, 0.5),
            route("fit", "p2", "m2", true, "low", "low", "public", 8000, 0.5),
        ];
        let out = select_model(&inp, &state_with(routes)).unwrap();
        assert_eq!(out.selected_provider, "p2");
    }

    #[test]
    fn should_exclude_unavailable_routes() {
        let routes = vec![
            route("a", "p1", "m1", false, "low", "high", "public", 8000, 0.9),
            route("b", "p2", "m2", true, "high", "high", "public", 8000, 0.9),
        ];
        let out = select_model(&input(), &state_with(routes)).unwrap();
        assert_eq!(out.selected_provider, "p2");
        assert!(out.fallback_routes.is_empty());
    }

    #[test]
    fn should_enforce_privacy_tier() {
        let mut inp = input();
        inp.privacy_tier = "public".to_string();
        // Only a confidential route exists; public request must exclude it.
        let routes = vec![route(
            "a",
            "p1",
            "m1",
            true,
            "low",
            "high",
            "confidential",
            8000,
            0.9,
        )];
        let err = select_model(&inp, &state_with(routes)).unwrap_err();
        assert!(matches!(err, TchedRouterError::NoCandidateAfterFiltering));
    }

    #[test]
    fn should_reject_undeclared_route() {
        let mut inp = input();
        inp.explicit_provider = Some("ghost".to_string());
        let routes = vec![route(
            "a", "p1", "m1", true, "low", "high", "public", 8000, 0.9,
        )];
        let err = select_model(&inp, &state_with(routes)).unwrap_err();
        assert!(matches!(err, TchedRouterError::ProviderNotDeclared { .. }));
    }

    #[test]
    fn should_return_fallback_routes_ordered() {
        let routes = vec![
            route(
                "best", "p1", "m1", true, "free", "high", "public", 8000, 1.0,
            ),
            route("mid", "p2", "m2", true, "low", "high", "public", 8000, 0.8),
            route(
                "worst", "p3", "m3", true, "premium", "high", "public", 8000, 0.2,
            ),
        ];
        let out = select_model(&input(), &state_with(routes)).unwrap();
        assert_eq!(out.selected_provider, "p1");
        assert_eq!(
            out.fallback_routes,
            vec!["mid".to_string(), "worst".to_string()]
        );
    }

    #[test]
    fn should_select_explicit_provider_when_authorized() {
        let mut inp = input();
        inp.explicit_provider = Some("p2".to_string());
        let routes = vec![
            route("a", "p1", "m1", true, "free", "high", "public", 8000, 1.0),
            route(
                "b", "p2", "m2", true, "premium", "high", "public", 8000, 0.5,
            ),
        ];
        let out = select_model(&inp, &state_with(routes)).unwrap();
        assert_eq!(out.selected_provider, "p2");
        assert_eq!(out.selected_model, "m2");
    }

    #[test]
    fn should_reject_undeclared_tool_before_scoring() {
        let mut inp = input();
        inp.tool_needs = vec!["nonexistent.tool".to_string()];
        let routes = vec![route(
            "a", "p1", "m1", true, "low", "high", "public", 8000, 0.9,
        )];
        let err = select_model(&inp, &state_with(routes)).unwrap_err();
        assert!(matches!(err, TchedRouterError::ToolNotDeclared { .. }));
    }
}
