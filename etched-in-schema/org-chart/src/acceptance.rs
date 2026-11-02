//! Acceptance: the deep hierarchy and the lone key compile through the
//! identical function with no code differences — only the chart differs.
//!
//! The agency below is fictional (placeholder names). It exercises depth,
//! compartments, and one member lacking a compartment.

#[cfg(test)]
mod tests {
    use crate::compile::compile;
    use crate::model::{Member, OrgChart, Position, Role};

    fn member(key: &str, attrs: &[&str]) -> Member {
        Member {
            key: key.into(),
            attributes: attrs.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn role(id: &str, caps: &[&str], attrs: &[&str]) -> Role {
        Role {
            id: id.into(),
            capabilities: caps.iter().map(|s| s.to_string()).collect(),
            requires_attributes: attrs.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn position(id: &str, parent: Option<&str>, roles: &[&str], members: Vec<Member>) -> Position {
        Position {
            id: id.into(),
            reports_to: parent.map(str::to_string),
            roles: roles.iter().map(|s| s.to_string()).collect(),
            members,
        }
    }

    /// Fictional Department of Civic Records: secretary → bureau →
    /// division → field team, with two compartments. Every level holds
    /// explicit roles; nothing is inherited.
    fn deep_chart() -> OrgChart {
        OrgChart {
            id: "civic-records".into(),
            roles: vec![
                role(
                    "secretary",
                    &["civic.records.read", "civic.records.publish"],
                    &[],
                ),
                role(
                    "archivist",
                    &["civic.records.read", "civic.records.archive"],
                    &[],
                ),
                role(
                    "field-agent",
                    &["civic.records.read", "civic.records.collect"],
                    &["compartment-field"],
                ),
                role(
                    "auditor",
                    &["civic.records.read", "civic.records.anchor"],
                    &["compartment-audit"],
                ),
            ],
            positions: vec![
                position(
                    "secretary",
                    None,
                    &["secretary"],
                    vec![member("wk-secretary", &[])],
                ),
                position(
                    "bureau-records",
                    Some("secretary"),
                    &["archivist"],
                    vec![member("wk-archivist", &[])],
                ),
                position(
                    "division-field",
                    Some("bureau-records"),
                    &["field-agent"],
                    vec![member("wk-agent-cleared", &["compartment-field"])],
                ),
                position(
                    "team-survey",
                    Some("division-field"),
                    &["field-agent"],
                    vec![member("wk-agent-plain", &[])],
                ),
                position(
                    "office-audit",
                    Some("secretary"),
                    &["auditor"],
                    vec![member("wk-auditor", &["compartment-audit"])],
                ),
            ],
        }
    }

    /// The far end of the spectrum: one position, one key, no attributes.
    fn lone_key_chart() -> OrgChart {
        OrgChart {
            id: "lone-key".into(),
            roles: vec![role("holder", &["civic.records.read"], &[])],
            positions: vec![position(
                "sole",
                None,
                &["holder"],
                vec![member("wk-lone-key-1", &[])],
            )],
        }
    }

    #[test]
    fn deep_hierarchy_compiles_with_provenance() {
        let out = compile(&deep_chart()).unwrap();
        let caps: Vec<(&str, &str)> = out
            .grants
            .iter()
            .map(|g| (g.member_key.as_str(), g.capability.as_str()))
            .collect();
        assert!(caps.contains(&("wk-secretary", "civic.records.publish")));
        assert!(caps.contains(&("wk-archivist", "civic.records.archive")));
        assert!(caps.contains(&("wk-agent-cleared", "civic.records.collect")));
        assert!(caps.contains(&("wk-auditor", "civic.records.anchor")));
        // Every grant names its role and position.
        for g in &out.grants {
            assert!(!g.via_role.is_empty() && !g.via_position.is_empty());
        }
    }

    #[test]
    fn missing_compartment_skips_with_reason() {
        let out = compile(&deep_chart()).unwrap();
        // Role gating is all-or-nothing per member: without the
        // compartment, the whole role is withheld with reason — even read.
        for cap in ["civic.records.collect", "civic.records.read"] {
            assert!(
                out.skips.iter().any(|s| s.member_key == "wk-agent-plain"
                    && s.capability == cap
                    && s.reason.contains("compartment-field")),
                "{cap}"
            );
        }
        assert!(!out.grants.iter().any(|g| g.member_key == "wk-agent-plain"));
    }

    #[test]
    fn lone_key_compiles_through_the_same_function() {
        let out = compile(&lone_key_chart()).unwrap();
        assert_eq!(out.grants.len(), 1);
        assert!(out.skips.is_empty());
        let g = &out.grants[0];
        assert_eq!(g.member_key, "wk-lone-key-1");
        assert_eq!(g.capability, "civic.records.read");
        assert_eq!(g.via_role, "holder");
        assert_eq!(g.via_position, "sole");
    }
}
