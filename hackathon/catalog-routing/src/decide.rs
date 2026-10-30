//! The decision: allow / refuse / needs-approval.
//!
//! Order (`docs/DROP-IN-SPEC.md` §2.2, `routing/DATA-MODEL.md`): match
//! controls → root needs two humans → fail closed on unsealed methods →
//! allow the grant-covered subset. A control miss refuses before grants
//! are consulted; grants never come from tags.

use crate::catalogs::compliance;
use crate::model::{Decision, Grants, Profile, Request, Verdict};

/// Pure decision over a request, profile controls, and compiled grants.
pub fn decide(request: &Request, profile: &Profile, grants: &Grants) -> Decision {
    let matches = compliance::match_controls(&request.tags, &profile.controls);
    if matches.is_empty() {
        return Decision {
            operations: vec![],
            verdict: Verdict::Refuse {
                missing_capability: None,
                rule: "no-control-match".to_string(),
            },
        };
    }
    let operations: Vec<_> = matches.iter().flat_map(|m| m.operations.clone()).collect();

    if compliance::is_root(&request.tags) {
        return Decision {
            operations,
            verdict: Verdict::NeedsApproval {
                have: 0,
                need: 2,
                rule: "two-person-root".to_string(),
            },
        };
    }

    for op in &operations {
        if op.capability.is_none() {
            return Decision {
                operations: vec![],
                verdict: Verdict::Refuse {
                    missing_capability: None,
                    rule: format!("method-not-sealed:{}.{}", op.plugin, op.method),
                },
            };
        }
    }

    let (covered, uncovered): (Vec<_>, Vec<_>) = operations.into_iter().partition(|op| {
        op.capability
            .as_deref()
            .is_some_and(|cap| grants.covers(cap))
    });
    if covered.is_empty() {
        let op = &uncovered[0];
        let capability = op.capability.clone().unwrap_or_default();
        let rule = matches
            .iter()
            .find(|m| {
                m.operations
                    .iter()
                    .any(|o| o.capability.as_deref() == Some(capability.as_str()))
            })
            .map(|m| m.control.clone())
            .unwrap_or_default();
        return Decision {
            operations: vec![],
            verdict: Verdict::Refuse {
                missing_capability: Some(capability),
                rule,
            },
        };
    }

    Decision {
        operations: covered,
        verdict: Verdict::Allow,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ActionKind, Surface};

    fn profile() -> Profile {
        Profile {
            id: "small-business-portal".into(),
            controls: vec![
                "etched-pt-1".into(),
                "etched-ac-1".into(),
                "etched-au-1".into(),
                "etched-si-1".into(),
                "etched-in-1".into(),
            ],
        }
    }

    fn request(tags: &[&str]) -> Request {
        Request {
            actor: "principal-1".into(),
            surface: Surface::Web,
            action: ActionKind::Read,
            tags: tags.iter().map(|s| s.to_string()).collect(),
            payload_ref: "payload-1".into(),
        }
    }

    fn full_grants() -> Grants {
        Grants {
            capabilities: vec![
                "cap.governance.schema-tag-registry.read@v1".into(),
                "elevation.authorize".into(),
                "cap.identity.sled.self.read@v1".into(),
                "cap.governance.routing-decision.read@v1".into(),
                "cron.invoke".into(),
                "workflows.write".into(),
                "identity_sled.read".into(),
            ],
        }
    }

    #[test]
    fn allow_when_controls_match_and_grants_cover() {
        let d = decide(&request(&["audit"]), &profile(), &full_grants());
        assert_eq!(d.verdict, Verdict::Allow);
        assert_eq!(d.operations.len(), 3);
    }

    #[test]
    fn refuse_on_control_miss_before_grants() {
        let d = decide(&request(&["bogus"]), &profile(), &full_grants());
        assert_eq!(
            d.verdict,
            Verdict::Refuse {
                missing_capability: None,
                rule: "no-control-match".into(),
            }
        );
        assert!(d.operations.is_empty());
    }

    #[test]
    fn refuse_names_missing_capability_and_rule() {
        let d = decide(
            &request(&["retention"]),
            &profile(),
            &Grants {
                capabilities: vec![],
            },
        );
        assert_eq!(
            d.verdict,
            Verdict::Refuse {
                missing_capability: Some("cron.invoke".into()),
                rule: "etched-si-1".into(),
            }
        );
    }

    #[test]
    fn allow_covered_subset_drops_uncovered() {
        let grants = Grants {
            capabilities: vec!["workflows.write".into()],
        };
        let d = decide(&request(&["retention"]), &profile(), &grants);
        assert_eq!(d.verdict, Verdict::Allow);
        assert_eq!(d.operations.len(), 1);
        assert_eq!(d.operations[0].method, "start_workflow");
    }

    #[test]
    fn refuse_unsealed_method_before_grants() {
        let d = decide(&request(&["integrity"]), &profile(), &full_grants());
        assert_eq!(
            d.verdict,
            Verdict::Refuse {
                missing_capability: None,
                rule: "method-not-sealed:web3.anchor".into(),
            }
        );
        assert!(d.operations.is_empty());
    }

    #[test]
    fn root_needs_two_approvals() {
        let d = decide(
            &request(&["integrity", "approval:root"]),
            &profile(),
            &Grants {
                capabilities: vec![],
            },
        );
        assert_eq!(
            d.verdict,
            Verdict::NeedsApproval {
                have: 0,
                need: 2,
                rule: "two-person-root".into(),
            }
        );
    }
}
