//! Minimal render-contract-compliant plugin fixture for op-plugin-lint tests.
use serde::{Deserialize, Serialize};

/// Widget section.
#[derive(Debug, Clone, Serialize, Deserialize, Default, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.software.demo.widget.schema@v1"))]
pub struct Widget {
    /// Widget identifier.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.software.demo.widget-id@v1"))]
    pub id: String,

    /// Listen port.
    #[serde(default)]
    #[schemars(
        range(min = 1, max = 65535),
        extend("x-oscal-subid" = "mut.software.demo.widget-port@v1")
    )]
    pub port: u16,
}

/// Demo plugin state.
#[derive(Debug, Clone, Serialize, Deserialize, Default, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.software.plugin.demo.schema@v1"))]
#[schemars(extend("x-oscal-category" = "software"))]
pub struct DemoState {
    /// Operational status.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.software.demo.status@v1"))]
    pub status: String,

    /// Configured widgets.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "exp.software.demo.widgets@v1"))]
    pub widgets: Vec<Widget>,
}

pub(crate) fn demo_schema() -> op_state_store::PluginSchema {
    let root = serde_json::to_value(schemars::schema_for!(DemoState)).unwrap();
    let mut schema = super::schemars_adapter::plugin_schema_from_json(
        "demo",
        "1.0.0",
        "Demo plugin for lint fixtures",
        &root,
    );

    use super::plugin_scaffold_helpers::{method_decl_from_schemars_with_output, EmptyInput};
    use op_state_store::SideEffect;

    #[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
    pub struct ListWidgetsOutput {
        pub widgets: Vec<Widget>,
    }

    schema.methods.insert(
        "list_widgets".to_string(),
        method_decl_from_schemars_with_output::<EmptyInput, ListWidgetsOutput>(
            "list_widgets",
            SideEffect::Read,
            true,
            "demo.read",
            "obs.software.demo.widgets.list@v1",
        ),
    );

    schema
}

inventory::submit! {
    crate::default_registry::PluginReg::new("demo", |_ctx| std::sync::Arc::new(DemoPlugin))
}

pub struct DemoPlugin;

#[cfg(test)]
mod tests {
    #[test]
    fn schema_is_schemars_seeded_and_typed() {}

    #[test]
    fn all_subids_are_valid() {}
}
