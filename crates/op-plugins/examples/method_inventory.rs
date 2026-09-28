//! Build-time evidence of declared method contracts. This report is never read
//! by runtime discovery, admission or dispatch: only the sealed catalog serves
//! those purposes. Run from the workspace root and redirect stdout as needed.
use anyhow::{Context, Result, ensure};
use op_plugins::default_registry::DefaultPluginRegistry;
use op_state_store::MemoryStore;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<()> {
    let registry = DefaultPluginRegistry::new(Arc::new(MemoryStore::new()));
    let mut names = DefaultPluginRegistry::available_plugins();
    names.sort();
    names.dedup();
    let mut plugins = std::collections::BTreeMap::new();
    for name in names {
        let plugin = registry.load_plugin(&name).await?;
        let schema = plugin
            .schema()
            .with_context(|| format!("{name} has no schema"))?;
        ensure!(!plugins.contains_key(&name), "duplicate plugin {name}");
        plugins.insert(
            name,
            serde_json::json!({
                "version": plugin.version(),
                "methods": schema.methods,
            }),
        );
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "purpose": "source declaration evidence; not a runtime catalog or implementation claim",
            "plugins": plugins,
        }))?
    );
    Ok(())
}
