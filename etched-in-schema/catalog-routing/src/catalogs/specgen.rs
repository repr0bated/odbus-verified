//! Spec generator: trim rows to a renderable grants-console spec.
//!
//! The console is a generated spec, not a hand-built page: every element is
//! a catalog component, validated before rendering, actions call granted
//! methods only. The propose form is illustrative (`dashboardFormPreview`
//! submits nothing) — proposals travel the approval path, never the UI.

use serde_json::{json, Value};

/// One grant row: the unit the console table renders.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrantRow {
    pub principal: &'static str,
    pub capability: &'static str,
    pub effect: &'static str,
    pub decision: &'static str,
}
pub const ALLOWED_COMPONENTS: &[&str] = &[
    "container",
    "pageHeader",
    "dashboardStatus",
    "dashboardTable",
    "dashboardFormPreview",
    "dashboardEventLog",
    "dashboardTopology",
];

/// Build the flat-spec console for `rows`.
pub fn console_spec(title: &str, subtitle: &str, rows: &[GrantRow]) -> Value {
    let table_rows: Vec<Value> = rows
        .iter()
        .enumerate()
        .map(|(i, r)| {
            json!({
                "id": format!("grant-{i}"),
                "cells": [r.capability, r.effect, r.principal, r.decision],
            })
        })
        .collect();
    json!({
        "root": "root",
        "elements": {
            "root": {
                "type": "container",
                "props": { "className": null },
                "children": ["header", "reference", "grants-table", "propose"]
            },
            "header": {
                "type": "pageHeader",
                "props": { "title": title, "subtitle": subtitle }
            },
            "reference": {
                "type": "dashboardStatus",
                "props": {
                    "label": "Draft console",
                    "detail": "Illustrative data. Proposals travel the approval path; this form submits nothing.",
                    "tone": "info"
                }
            },
            "grants-table": {
                "type": "dashboardTable",
                "props": {
                    "caption": "Capability trim",
                    "columns": [
                        { "id": "capability", "label": "Capability" },
                        { "id": "effect", "label": "Effect" },
                        { "id": "principal", "label": "Principal" },
                        { "id": "decision", "label": "Decision" }
                    ],
                    "rows": table_rows
                }
            },
            "propose": {
                "type": "dashboardFormPreview",
                "props": {
                    "title": "Propose grant change",
                    "fields": [
                        { "id": "principal", "label": "Principal", "type": "text", "value": "", "help": "Reference control; no request is sent." },
                        { "id": "capability", "label": "Capability", "type": "text", "value": "", "help": "Reference control; no request is sent." },
                        { "id": "action", "label": "Action", "type": "select", "options": ["grant", "revoke", "delegate"], "help": "Reference control; no request is sent." },
                        { "id": "reason", "label": "Reason", "type": "textarea", "value": "", "help": "Reference control; no request is sent." }
                    ]
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows() -> Vec<GrantRow> {
        vec![
            GrantRow {
                principal: "control-plane-chatbot",
                capability: "cap.identity.sled.self.read@v1",
                effect: "obs",
                decision: "keep",
            },
            GrantRow {
                principal: "control-plane-chatbot",
                capability: "cron.invoke",
                effect: "mut",
                decision: "delegate",
            },
        ]
    }

    #[test]
    fn spec_uses_only_catalog_components() {
        let spec = console_spec("T", "S", &rows());
        let elements = spec["elements"].as_object().unwrap();
        for (id, el) in elements {
            let t = el["type"].as_str().unwrap();
            assert!(ALLOWED_COMPONENTS.contains(&t), "{id} uses {t}");
            assert!(el.get("props").is_some(), "{id} missing props");
        }
    }

    #[test]
    fn table_cells_match_columns() {
        let spec = console_spec("T", "S", &rows());
        let table = &spec["elements"]["grants-table"]["props"];
        let n_cols = table["columns"].as_array().unwrap().len();
        for row in table["rows"].as_array().unwrap() {
            assert_eq!(row["cells"].as_array().unwrap().len(), n_cols);
        }
        assert_eq!(table["rows"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn empty_rows_still_valid_shape() {
        let spec = console_spec("T", "S", &[]);
        assert_eq!(spec["root"], "root");
        assert!(spec["elements"]["grants-table"]["props"]["rows"]
            .as_array()
            .unwrap()
            .is_empty());
    }
}

/// One provenance row: why a member holds a capability.
pub struct ProvenanceRow {
    pub capability: &'static str,
    pub role: &'static str,
    pub position: &'static str,
    pub event: &'static str,
}

/// Provenance view: capability <- role <- position <- chart event.
pub fn provenance_spec(title: &str, rows: &[ProvenanceRow]) -> Value {
    let table_rows: Vec<Value> = rows
        .iter()
        .enumerate()
        .map(|(i, r)| {
            json!({
                "id": format!("prov-{i}"),
                "cells": [r.capability, r.role, r.position, r.event],
            })
        })
        .collect();
    json!({
        "root": "root",
        "elements": {
            "root": {
                "type": "container",
                "props": { "className": null },
                "children": ["header", "reference", "prov-table"]
            },
            "header": {
                "type": "pageHeader",
                "props": { "title": title, "subtitle": "Read-only provenance" }
            },
            "reference": {
                "type": "dashboardStatus",
                "props": {
                    "label": "Reference view",
                    "detail": "Why each capability is held. Proposals travel the approval path.",
                    "tone": "info"
                }
            },
            "prov-table": {
                "type": "dashboardTable",
                "props": {
                    "caption": "Capability provenance",
                    "columns": [
                        { "id": "capability", "label": "Capability" },
                        { "id": "role", "label": "Role" },
                        { "id": "position", "label": "Position" },
                        { "id": "event", "label": "Chart event" }
                    ],
                    "rows": table_rows
                }
            }
        }
    })
}

/// One chart node: id, label, parent id or `None` for roots.
pub struct ChartNode {
    pub id: &'static str,
    pub label: &'static str,
    pub parent: Option<&'static str>,
    pub roles: &'static str,
}

/// Org-chart viewer: topology (reports-to edges) plus a role table.
/// Layout is deterministic: depth sets x, sibling order sets y.
pub fn orgchart_spec(title: &str, nodes: &[ChartNode]) -> Value {
    let mut depth_of: std::collections::BTreeMap<&str, f64> = Default::default();
    for n in nodes {
        let mut depth: f64 = 0.0;
        let mut current = n.parent;
        let mut guard = 0;
        while let Some(pid) = current {
            guard += 1;
            if guard > 32 {
                break;
            }
            depth += 25.0;
            current = nodes.iter().find(|q| q.id == pid).and_then(|q| q.parent);
        }
        depth_of.insert(n.id, depth.min(100.0));
    }
    let mut per_level: std::collections::BTreeMap<i64, usize> = Default::default();
    let topo_nodes: Vec<Value> = nodes
        .iter()
        .map(|n| {
            let level = (depth_of[n.id] / 25.0) as i64;
            let slot = per_level.entry(level).or_insert(0);
            *slot += 1;
            let y = (*slot as f64 * 20.0).min(100.0);
            json!({ "id": n.id, "label": n.label, "x": depth_of[n.id], "y": y })
        })
        .collect();
    let edges: Vec<Value> = nodes
        .iter()
        .filter_map(|n| n.parent.map(|p| json!({ "from": p, "to": n.id })))
        .collect();
    let table_rows: Vec<Value> = nodes
        .iter()
        .enumerate()
        .map(|(i, n)| json!({ "id": format!("pos-{i}"), "cells": [n.label, n.roles] }))
        .collect();
    json!({
        "root": "root",
        "elements": {
            "root": {
                "type": "container",
                "props": { "className": null },
                "children": ["header", "reference", "org-topology", "org-table"]
            },
            "header": {
                "type": "pageHeader",
                "props": { "title": title, "subtitle": "Positions and reporting lines" }
            },
            "reference": {
                "type": "dashboardStatus",
                "props": {
                    "label": "Reference view",
                    "detail": "Explicit roles only; no inheritance. Depths computed from reports-to links.",
                    "tone": "info"
                }
            },
            "org-topology": {
                "type": "dashboardTopology",
                "props": { "title": "Reporting lines", "nodes": topo_nodes, "edges": edges }
            },
            "org-table": {
                "type": "dashboardTable",
                "props": {
                    "caption": "Positions and roles",
                    "columns": [
                        { "id": "position", "label": "Position" },
                        { "id": "roles", "label": "Roles" }
                    ],
                    "rows": table_rows
                }
            }
        }
    })
}

/// One feed episode: id, trigger, outcome, vectorized and purged flags.
pub struct FeedEpisode {
    pub id: &'static str,
    pub trigger: &'static str,
    pub outcome: &'static str,
    pub vectorized: bool,
    pub purged: bool,
}

/// Feed-loop monitor: status, episode event log, vectorize/purge table.
pub fn feed_spec(title: &str, episodes: &[FeedEpisode]) -> Value {
    let pending = episodes.iter().filter(|e| !e.vectorized).count();
    let entries: Vec<Value> = episodes
        .iter()
        .map(|e| {
            json!({
                "time": e.id,
                "title": e.trigger,
                "detail": e.outcome,
                "tone": if e.vectorized { "success" } else { "warning" }
            })
        })
        .collect();
    let table_rows: Vec<Value> = episodes
        .iter()
        .enumerate()
        .map(|(i, e)| {
            json!({
                "id": format!("ep-{i}"),
                "cells": [e.id, if e.vectorized { "yes" } else { "no" }, if e.purged { "yes" } else { "no" }]
            })
        })
        .collect();
    json!({
        "root": "root",
        "elements": {
            "root": {
                "type": "container",
                "props": { "className": null },
                "children": ["header", "reference", "feed-log", "feed-table"]
            },
            "header": {
                "type": "pageHeader",
                "props": { "title": title, "subtitle": "Episodes, vectorization, purge" }
            },
            "reference": {
                "type": "dashboardStatus",
                "props": {
                    "label": format!("{pending} unvectorized"),
                    "detail": "Vectorize-then-clear, in that order. Unvectorized episodes refuse purge.",
                    "tone": if pending == 0 { "success" } else { "warning" }
                }
            },
            "feed-log": {
                "type": "dashboardEventLog",
                "props": { "title": "Episodes", "entries": entries }
            },
            "feed-table": {
                "type": "dashboardTable",
                "props": {
                    "caption": "Vectorize and purge state",
                    "columns": [
                        { "id": "episode", "label": "Episode" },
                        { "id": "vectorized", "label": "Vectorized" },
                        { "id": "purged", "label": "Purged" }
                    ],
                    "rows": table_rows
                }
            }
        }
    })
}

#[cfg(test)]
mod builder_tests {
    use super::*;

    #[test]
    fn new_builders_use_only_catalog_components() {
        let specs = vec![
            provenance_spec(
                "P",
                &[ProvenanceRow {
                    capability: "c",
                    role: "r",
                    position: "p",
                    event: "e",
                }],
            ),
            orgchart_spec(
                "O",
                &[
                    ChartNode {
                        id: "a",
                        label: "A",
                        parent: None,
                        roles: "r",
                    },
                    ChartNode {
                        id: "b",
                        label: "B",
                        parent: Some("a"),
                        roles: "r2",
                    },
                ],
            ),
            feed_spec(
                "F",
                &[FeedEpisode {
                    id: "ep-1",
                    trigger: "chat",
                    outcome: "done",
                    vectorized: true,
                    purged: false,
                }],
            ),
        ];
        for spec in &specs {
            for (id, el) in spec["elements"].as_object().unwrap() {
                let t = el["type"].as_str().unwrap();
                assert!(ALLOWED_COMPONENTS.contains(&t), "{id} uses {t}");
            }
        }
    }

    #[test]
    fn orgchart_edges_reference_existing_nodes() {
        let spec = orgchart_spec(
            "O",
            &[
                ChartNode {
                    id: "a",
                    label: "A",
                    parent: None,
                    roles: "r",
                },
                ChartNode {
                    id: "b",
                    label: "B",
                    parent: Some("a"),
                    roles: "r2",
                },
            ],
        );
        let topo = &spec["elements"]["org-topology"]["props"];
        let ids: Vec<&str> = topo["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|n| n["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, vec!["a", "b"]);
        for e in topo["edges"].as_array().unwrap() {
            assert!(ids.contains(&e["from"].as_str().unwrap()));
            assert!(ids.contains(&e["to"].as_str().unwrap()));
        }
        let nodes = topo["nodes"].as_array().unwrap();
        assert!(nodes[1]["x"].as_f64().unwrap() > nodes[0]["x"].as_f64().unwrap());
    }

    #[test]
    fn feed_status_counts_unvectorized() {
        let spec = feed_spec(
            "F",
            &[
                FeedEpisode {
                    id: "e1",
                    trigger: "t",
                    outcome: "o",
                    vectorized: true,
                    purged: true,
                },
                FeedEpisode {
                    id: "e2",
                    trigger: "t",
                    outcome: "o",
                    vectorized: false,
                    purged: false,
                },
            ],
        );
        assert_eq!(
            spec["elements"]["reference"]["props"]["label"],
            "1 unvectorized"
        );
        assert_eq!(spec["elements"]["reference"]["props"]["tone"], "warning");
    }
}
