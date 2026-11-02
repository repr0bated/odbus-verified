//! Emit the catch-up compositions (illustrative data) as one JSON object.
//! Run: cargo run --example compositions > local/compositions.json

use catalog_routing::catalogs::specgen::{
    feed_spec, orgchart_spec, provenance_spec, ChartNode, FeedEpisode, ProvenanceRow,
};
use std::collections::BTreeMap;

fn main() {
    let mut out = BTreeMap::new();
    out.insert(
        "provenance",
        provenance_spec(
            "Capability provenance",
            &[
                ProvenanceRow {
                    capability: "cap.identity.sled.self.read@v1",
                    role: "reader",
                    position: "desk",
                    event: "chart-ev-001",
                },
                ProvenanceRow {
                    capability: "cron.invoke",
                    role: "scheduler",
                    position: "desk",
                    event: "chart-ev-002",
                },
                ProvenanceRow {
                    capability: "cap.governance.schema-tag-registry.read@v1",
                    role: "reader",
                    position: "desk",
                    event: "chart-ev-003",
                },
            ],
        ),
    );
    out.insert(
        "orgchart",
        orgchart_spec(
            "Org chart",
            &[
                ChartNode {
                    id: "secretary",
                    label: "Secretary",
                    parent: None,
                    roles: "secretary",
                },
                ChartNode {
                    id: "bureau",
                    label: "Bureau",
                    parent: Some("secretary"),
                    roles: "archivist",
                },
                ChartNode {
                    id: "division",
                    label: "Division",
                    parent: Some("bureau"),
                    roles: "field-agent",
                },
                ChartNode {
                    id: "team",
                    label: "Team",
                    parent: Some("division"),
                    roles: "field-agent",
                },
                ChartNode {
                    id: "audit",
                    label: "Audit",
                    parent: Some("secretary"),
                    roles: "auditor",
                },
            ],
        ),
    );
    out.insert(
        "feed",
        feed_spec(
            "Chat feed",
            &[
                FeedEpisode {
                    id: "ep-101",
                    trigger: "chat turn",
                    outcome: "GoalAchieved",
                    vectorized: true,
                    purged: true,
                },
                FeedEpisode {
                    id: "ep-102",
                    trigger: "chat turn",
                    outcome: "GoalAchieved",
                    vectorized: true,
                    purged: false,
                },
                FeedEpisode {
                    id: "ep-103",
                    trigger: "chat turn",
                    outcome: "pending",
                    vectorized: false,
                    purged: false,
                },
            ],
        ),
    );
    println!("{}", serde_json::to_string_pretty(&out).unwrap());
}
