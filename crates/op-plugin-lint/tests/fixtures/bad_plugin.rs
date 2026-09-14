//! Deliberately incomplete plugin — used to assert FAIL/WARN findings.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default, schemars::JsonSchema)]
pub struct BadState {
    pub status: String,
    pub port: u16,
}

pub(crate) fn bad_schema() -> op_state_store::PluginSchema {
    let mut schema = PluginSchema::builder("bad").build();
    // Hand-rolled / deprecated path — should FAIL.
    schema.methods.insert(
        "do_thing".to_string(),
        method_decl_from_schemars::<BadInput>(
            "do_thing",
            SideEffect::Mutation,
            false,
            "cap.thing",
            // Valid category prefix, invalid component-type → taxonomy FAIL.
            "obs.notatype.demo.widgets.list@v1",
        ),
    );
    schema
}

#[derive(schemars::JsonSchema)]
struct BadInput {
    x: String,
}
