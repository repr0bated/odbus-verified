//! Emit the grants-console example spec (illustrative rows) to stdout.
//! Run: cargo run --example grants_console > local/grants-console.spec.json

use catalog_routing::catalogs::specgen::{console_spec, GrantRow};

fn main() {
    // Illustrative subset of local/grants-trim.md — shape demo, not data export.
    let rows = vec![
        GrantRow {
            principal: "control-plane-chatbot",
            capability: "cap.identity.sled.self.read@v1",
            effect: "obs",
            decision: "keep",
        },
        GrantRow {
            principal: "control-plane-chatbot",
            capability: "cap.software.3tched-router.chat@v1",
            effect: "other",
            decision: "keep",
        },
        GrantRow {
            principal: "control-plane-chatbot",
            capability: "cron.invoke",
            effect: "mut",
            decision: "delegate",
        },
        GrantRow {
            principal: "control-plane-chatbot",
            capability: "elevation.authorize",
            effect: "mut",
            decision: "delegate",
        },
        GrantRow {
            principal: "control-plane-chatbot",
            capability: "workflows.write",
            effect: "mut",
            decision: "delegate",
        },
        GrantRow {
            principal: "control-plane-chatbot",
            capability: "cap.software.tched-router.chat@v1",
            effect: "unmapped",
            decision: "drop",
        },
        GrantRow {
            principal: "control-plane-chatbot",
            capability: "chat.control_plane",
            effect: "unmapped",
            decision: "proposed",
        },
        GrantRow {
            principal: "control-plane-chatbot",
            capability: "cap.governance.schema-tag-registry.read@v1",
            effect: "obs",
            decision: "proposed-add",
        },
    ];
    let spec = console_spec("Grants console", "Capability trim · draft console", &rows);
    println!("{}", serde_json::to_string_pretty(&spec).unwrap());
}
