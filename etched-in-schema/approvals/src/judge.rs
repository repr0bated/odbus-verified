//! Co-signer decision in code: docs/APPROVALS-SPEC.md §5.
//!
//! Judgments arrive as model scores; this module converts them with fixed
//! thresholds and applies the truth table. `destructive` is recorded only —
//! it raises the break-glass delay but never changes the decision.

/// Model score → atomic answer. Thresholds are fixed in code, not in the model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriState {
    Yes,
    No,
    Unsettled,
}

pub fn judge(score: f64) -> TriState {
    if score.is_nan() {
        return TriState::Unsettled;
    }
    if score >= 0.7 {
        TriState::Yes
    } else if score <= 0.3 {
        TriState::No
    } else {
        TriState::Unsettled
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Judgments {
    pub in_boundary: TriState,
    pub permitted: TriState,
    pub executor_ok: TriState,
    pub shared_infra: TriState,
    pub destructive: TriState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Approve,
    /// `failed` names the judgment that refused; every refusal is
    /// break-glass-eligible, never silently droppable.
    Refuse { failed: &'static str },
}

pub fn decide(j: &Judgments) -> Verdict {
    if j.in_boundary != TriState::Yes {
        return Verdict::Refuse {
            failed: "in_declared_boundary",
        };
    }
    if j.permitted != TriState::Yes {
        return Verdict::Refuse {
            failed: "operation_permitted_by_control",
        };
    }
    if j.executor_ok != TriState::Yes {
        return Verdict::Refuse {
            failed: "executor_matches_request",
        };
    }
    if j.shared_infra == TriState::Yes {
        return Verdict::Refuse {
            failed: "affects_shared_infrastructure",
        };
    }
    Verdict::Approve
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all_yes() -> Judgments {
        Judgments {
            in_boundary: TriState::Yes,
            permitted: TriState::Yes,
            executor_ok: TriState::Yes,
            shared_infra: TriState::No,
            destructive: TriState::No,
        }
    }

    #[test]
    fn routine_within_policy_approves() {
        assert_eq!(decide(&all_yes()), Verdict::Approve);
    }

    #[test]
    fn destructive_does_not_change_decision() {
        let j = Judgments {
            destructive: TriState::Yes,
            ..all_yes()
        };
        assert_eq!(decide(&j), Verdict::Approve);
    }

    #[test]
    fn shared_infra_refuses_with_named_judgment() {
        let j = Judgments {
            shared_infra: TriState::Yes,
            ..all_yes()
        };
        assert_eq!(
            decide(&j),
            Verdict::Refuse {
                failed: "affects_shared_infrastructure"
            }
        );
    }

    #[test]
    fn unsettled_any_gate_refuses() {
        for gate in [
            Judgments {
                in_boundary: TriState::Unsettled,
                ..all_yes()
            },
            Judgments {
                permitted: TriState::Unsettled,
                ..all_yes()
            },
            Judgments {
                executor_ok: TriState::Unsettled,
                ..all_yes()
            },
        ] {
            assert!(matches!(decide(&gate), Verdict::Refuse { .. }));
        }
    }

    #[test]
    fn thresholds_match_spec() {
        assert_eq!(judge(0.7), TriState::Yes);
        assert_eq!(judge(0.3), TriState::No);
        assert_eq!(judge(0.5), TriState::Unsettled);
        assert_eq!(judge(f64::NAN), TriState::Unsettled);
    }
}
