//! Bridge: org-chart output into decide input.
//!
//! The compiler is the sole grant authority; `decide()` consumes grants.
//! This module is the only joint between them: member-keyed grants from a
//! [`CompileOutput`] become the [`Grants`] a decision checks. Provenance
//! stays on the compiler side; the decision sees exact capabilities only.

use crate::model::Grants;
use org_chart::CompileOutput;

/// Grants for a member only if the output verifies against the live sealed
/// catalog hash. Order: compile_pinned → verify → decide. Unpinned or
/// drifted outputs refuse before any decision is computed.
pub fn grants_for_verified(
    output: &org_chart::CompileOutput,
    member_key: &str,
    live_hash: &str,
) -> Result<Grants, org_chart::SealError> {
    org_chart::verify_seal(output, live_hash)?;
    Ok(grants_for(output, member_key))
}
/// Effective grant strings for one member key, in deterministic order.
pub fn grants_for(output: &CompileOutput, member_key: &str) -> Grants {
    let mut capabilities: Vec<String> = output
        .grants
        .iter()
        .filter(|g| g.member_key == member_key)
        .map(|g| g.capability.clone())
        .collect();
    capabilities.sort();
    capabilities.dedup();
    Grants { capabilities }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{decide, ActionKind, Profile, Request, Surface, Verdict};
    use org_chart::{Member, OrgChart, Position, Role};

    fn member(key: &str, attrs: &[&str]) -> Member {
        Member {
            key: key.into(),
            attributes: attrs.iter().map(|s| s.to_string()).collect(),
        }
    }

    /// Chart holding exactly the sealed audit capabilities the decision
    /// needs: self-read plus the routing-decision reads.
    fn audit_chart() -> OrgChart {
        OrgChart {
            id: "audit".into(),
            roles: vec![Role {
                id: "reader".into(),
                capabilities: vec![
                    "cap.identity.sled.self.read@v1".into(),
                    "cap.governance.routing-decision.read@v1".into(),
                ],
                requires_attributes: vec![],
            }],
            positions: vec![Position {
                id: "desk".into(),
                reports_to: None,
                roles: vec!["reader".into()],
                members: vec![member("wk-desk", &[])],
            }],
        }
    }

    fn profile() -> Profile {
        Profile {
            id: "small-business-portal".into(),
            controls: vec!["etched-au-1".into()],
        }
    }

    #[test]
    fn compiled_grants_drive_allow() {
        let output = org_chart::compile(&audit_chart()).unwrap();
        let grants = grants_for(&output, "wk-desk");
        assert_eq!(grants.capabilities.len(), 2);
        let request = Request {
            actor: "wk-desk".into(),
            surface: Surface::Web,
            action: ActionKind::Read,
            tags: vec!["audit".into(), "action:read".into()],
            payload_ref: "wiring".into(),
        };
        let decision = decide(&request, &profile(), &grants);
        assert_eq!(decision.verdict, Verdict::Allow);
        assert!(decision
            .operations
            .iter()
            .any(|o| o.method == "get_own_identity"));
    }

    #[test]
    fn skipped_member_has_no_grants_to_decide_with() {
        let chart = OrgChart {
            id: "gated".into(),
            roles: vec![Role {
                id: "field".into(),
                capabilities: vec!["civic.collect".into()],
                requires_attributes: vec!["compartment-field".into()],
            }],
            positions: vec![Position {
                id: "team".into(),
                reports_to: None,
                roles: vec!["field".into()],
                members: vec![member("wk-plain", &[])],
            }],
        };
        let output = org_chart::compile(&chart).unwrap();
        assert!(output.grants.is_empty());
        let grants = grants_for(&output, "wk-plain");
        assert!(grants.capabilities.is_empty());
    }
    #[test]
    fn verified_gate_enforces_live_hash() {
        use crate::org_bridge::grants_for_verified;
        let chart = audit_chart();
        let pinned = org_chart::compile_pinned(&chart, Some("live1")).unwrap();
        assert!(grants_for_verified(&pinned, "wk-desk", "live1").is_ok());
        assert!(grants_for_verified(&pinned, "wk-desk", "other").is_err());
        let unpinned = org_chart::compile(&chart).unwrap();
        assert!(grants_for_verified(&unpinned, "wk-desk", "live1").is_err());
    }
}
