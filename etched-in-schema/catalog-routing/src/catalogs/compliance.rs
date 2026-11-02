//! Compliance catalog: control tags → OSCAL controls → candidate operations.
//!
//! Table is `oscal/TAG-TAXONOMY.md` as code; `oscal/catalogs/etched-catalog`
//! is authoritative for controls and the sealed blobs (`opblob inspect`)
//! for method names and capabilities. Qualifier tags (`action:*`,
//! `approval:*`, `surface:*`, `data:*`, `model:*`) never match controls —
//! they steer the decision.

use crate::model::Operation;

/// One taxonomy row: tag, control, and candidate `(plugin, method,
/// sealed capability)` triples. Capability is `None` when no blob is
/// sealed for the method yet (e.g. `web3` anchor: drafts only).
/// `(plugin, sealed method, sealed capability or None if unsealed)`.
type SealedOp = (&'static str, &'static str, Option<&'static str>);
/// `(control tag, OSCAL control, candidate sealed operations)`.
type TaxonomyRow = (&'static str, &'static str, &'static [SealedOp]);
const TAXONOMY: &[TaxonomyRow] = &[
    (
        "pii",
        "etched-pt-1",
        &[(
            "schema_tag_registry",
            "get_policy",
            Some("cap.governance.schema-tag-registry.read@v1"),
        )],
    ),
    (
        "access-control",
        "etched-ac-1",
        &[
            ("elevation", "authorize_task", Some("elevation.authorize")),
            (
                "schema_tag_registry",
                "get_tag",
                Some("cap.governance.schema-tag-registry.read@v1"),
            ),
        ],
    ),
    (
        "audit",
        "etched-au-1",
        &[
            (
                "identity_sled",
                "get_own_identity",
                Some("cap.identity.sled.self.read@v1"),
            ),
            (
                "routing_decision",
                "get_decision",
                Some("cap.governance.routing-decision.read@v1"),
            ),
            (
                "routing_decision",
                "list_decisions",
                Some("cap.governance.routing-decision.read@v1"),
            ),
        ],
    ),
    (
        "retention",
        "etched-si-1",
        &[
            ("cron", "create_job", Some("cron.invoke")),
            ("workflows", "start_workflow", Some("workflows.write")),
        ],
    ),
    (
        "integrity",
        "etched-in-1",
        &[
            ("identity_sled", "get_identity", Some("identity_sled.read")),
            ("web3", "anchor", None),
        ],
    ),
];

/// A control the request matched, with its candidate operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlMatch {
    pub control: String,
    pub operations: Vec<Operation>,
}
fn seal_operation(spec: &(&str, &str, Option<&str>)) -> Operation {
    Operation {
        plugin: spec.0.to_string(),
        method: spec.1.to_string(),
        capability: spec.2.map(str::to_string),
    }
}

/// Resolve request tags to controls, keeping taxonomy order and dropping
/// controls outside the profile. Unknown control tags match nothing —
/// a tag miss is a refusal downstream, never a default-allow.
pub fn match_controls(tags: &[String], profile_controls: &[String]) -> Vec<ControlMatch> {
    let mut out = Vec::new();
    for (tag, control, ops) in TAXONOMY {
        if !tags.iter().any(|t| t == tag) {
            continue;
        }
        if !profile_controls.iter().any(|c| c == control) {
            continue;
        }
        if out.iter().any(|m: &ControlMatch| m.control == *control) {
            continue;
        }
        out.push(ControlMatch {
            control: control.to_string(),
            operations: ops.iter().map(seal_operation).collect(),
        });
    }
    out
}

/// True when the request carries the two-person-rule marker.
pub fn is_root(tags: &[String]) -> bool {
    tags.iter().any(|t| t == "approval:root")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile() -> Vec<String> {
        vec![
            "etched-pt-1".into(),
            "etched-ac-1".into(),
            "etched-au-1".into(),
            "etched-si-1".into(),
            "etched-in-1".into(),
        ]
    }

    #[test]
    fn each_taxonomy_tag_resolves() {
        let cases = [
            ("pii", "etched-pt-1", 1),
            ("audit", "etched-au-1", 3),
            ("access-control", "etched-ac-1", 2),
            ("retention", "etched-si-1", 2),
            ("integrity", "etched-in-1", 2),
        ];
        for (tag, control, op_count) in cases {
            let m = match_controls(&[tag.into()], &profile());
            assert_eq!(m.len(), 1, "tag {tag}");
            assert_eq!(m[0].control, control);
            assert_eq!(m[0].operations.len(), op_count, "tag {tag}");
        }
    }

    #[test]
    fn sealed_capabilities_match_opblob() {
        let m = match_controls(&["audit".into()], &profile());
        let own = m[0]
            .operations
            .iter()
            .find(|o| o.method == "get_own_identity")
            .unwrap();
        assert_eq!(
            own.capability.as_deref(),
            Some("cap.identity.sled.self.read@v1")
        );
    }

    #[test]
    fn unsealed_method_carries_no_capability() {
        let m = match_controls(&["integrity".into()], &profile());
        let anchor = m[0].operations.iter().find(|o| o.plugin == "web3").unwrap();
        assert_eq!(anchor.capability, None);
    }

    #[test]
    fn qualifiers_never_match_and_miss_is_empty() {
        let tags: Vec<String> = vec![
            "action:publish".into(),
            "surface:web".into(),
            "bogus".into(),
        ];
        assert!(match_controls(&tags, &profile()).is_empty());
    }

    #[test]
    fn profile_filters_controls() {
        let m = match_controls(&["pii".into(), "audit".into()], &["etched-au-1".into()]);
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].control, "etched-au-1");
    }

    #[test]
    fn duplicate_tags_match_once() {
        let m = match_controls(&["audit".into(), "audit".into()], &profile());
        assert_eq!(m.len(), 1);
    }

    #[test]
    fn table_matches_sealed_catalog() {
        // The OSCAL catalog is the authority; the table must mirror its
        // `sealed-operations` exactly (plugin, method, capability).
        let raw = include_str!("../../../oscal/catalogs/etched-catalog/catalog.json");
        let catalog: serde_json::Value = serde_json::from_str(raw).unwrap();
        let controls = catalog["catalog"]["controls"].as_array().unwrap();
        let all_tags = ["pii", "audit", "access-control", "retention", "integrity"];
        let prof: Vec<String> = controls
            .iter()
            .map(|c| c["id"].as_str().unwrap().to_string())
            .collect();
        for tag in all_tags {
            let matched = match_controls(&[tag.to_string()], &prof);
            assert_eq!(matched.len(), 1, "tag {tag}");
            let control = controls
                .iter()
                .find(|c| {
                    c["props"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|p| p["name"] == "tag" && p["value"] == tag)
                })
                .unwrap();
            let sealed: Vec<(String, String, Option<String>)> = {
                let raw_ops = control["props"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|p| p["name"] == "sealed-operations")
                    .unwrap()["value"]
                    .as_str()
                    .unwrap();
                serde_json::from_str::<Vec<serde_json::Value>>(raw_ops)
                    .unwrap()
                    .iter()
                    .map(|o| {
                        (
                            o["plugin"].as_str().unwrap().to_string(),
                            o["method"].as_str().unwrap().to_string(),
                            o["capability"].as_str().map(str::to_string),
                        )
                    })
                    .collect()
            };
            let tabled: Vec<(String, String, Option<String>)> = matched[0]
                .operations
                .iter()
                .map(|o| (o.plugin.clone(), o.method.clone(), o.capability.clone()))
                .collect();
            assert_eq!(tabled, sealed, "tag {tag} diverged from catalog");
        }
    }

    #[test]
    fn root_marker_detected() {
        assert!(is_root(&["approval:root".into()]));
        assert!(!is_root(&["action:write".into()]));
    }
}
