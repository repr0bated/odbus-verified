//! Browser contract data read inside the container from its own pinned FUSE
//! namespaces. The full catalog is disclosed to the renderer: the requesting
//! human's grants annotate each operation (`currently_granted`), they do not
//! filter plugins or methods out — rendering must never lose a plugin because
//! a grant is absent. Reading a container mount never delegates the
//! container's operation authority; execution is still grant-enforced in
//! `Runtime::call`, so an ungranted operation renders but cannot run.
use anyhow::{Context, Result, ensure};
use op_plugins::state_plugins::fuse::{GetCatalogInput, GetCatalogOutput, CatalogView, CatalogPlugin, CatalogOperation};
use serde_json::Value;
use crate::runtime::BackendContext;

pub(crate) async fn get_catalog(ctx: &BackendContext<'_>, args: &Value) -> Result<Value> {
    let input: GetCatalogInput = serde_json::from_value(args.clone())?;
    ensure!(crate::container_ui::setting("CHAT_UI_DIR")?.is_some(), "container UI is not configured");
    let mounts = [
        crate::container_ui::setting("CHAT_UI_MOUNT")?.unwrap_or_else(|| "/run/mounts/3tchedFS".into()),
        crate::container_ui::setting("CHAT_UI_BINARY_MOUNT")?.unwrap_or_else(|| "/run/mounts/threetchedFS".into()),
    ];
    let grants = ctx.runtime.current_grants(ctx.caller).await?;
    let catalog = ctx.runtime.catalog().clone();
    let rendering_grants = grants.clone();
    let output = tokio::task::spawn_blocking(move || -> Result<GetCatalogOutput> {
        let mut views = Vec::with_capacity(2);
        let mut seen = std::collections::BTreeSet::new();
        for mount in mounts {
            let namespace = _3tched_fs::mounted::MountedNamespace::open_for_identity_mode(
                std::path::Path::new(&mount), _3tched_fs::mounted::IdentityMode::GuestKernel, None)
                .context("required container schema mount is unavailable")?;
            let view = namespace.catalog_view()?;
            let mut plugins = Vec::new();
            for plugin in namespace.plugins()? {
                if input.plugin.as_ref().is_some_and(|selected| selected != &plugin) { continue; }
                ensure!(seen.insert(plugin.clone()), "plugin appears in more than one container view");
                let schema = namespace.schema(&plugin)?;
                let pinned = catalog.plugin(&plugin).context("mounted plugin is absent from sealed runtime")?;
                ensure!(pinned.schema.as_ref() == Some(&serde_json::to_value(&schema)?),
                    "mounted schema differs from current sealed runtime");
                let operations = schema.methods.iter()
                    .map(|(name, method)| {
                        let prefix = format!("plugins/{}/operations/{}",
                            _3tched_fs::namespace::encode_component(&plugin), _3tched_fs::namespace::encode_component(name));
                        CatalogOperation { method:name.clone(), call_path:format!("{prefix}/call"),
                            metadata_path:format!("{prefix}/metadata.json"),
                            currently_granted:method.required_capability.as_ref()
                                .is_none_or(|capability| grants.contains(capability)) }
                    }).collect::<Vec<_>>();
                plugins.push(CatalogPlugin { plugin_id:plugin.clone(),
                    object_hash:view["objects"][&plugin].as_str().context("mounted object hash absent")?.to_owned(),
                    schema_hash:pinned.schema_hash.clone(), schema:serde_json::to_value(schema)?,
                    operations, state:"not_requested".into() });
            }
            namespace.verify()?;
            views.push(CatalogView {
                view_id:view["view_id"].as_str().context("mounted view id absent")?.to_owned(),
                source_catalog_hash:view["source_catalog_hash"].as_str().context("mounted catalog hash absent")?.to_owned(),
                source_catalog_generation:view["source_catalog_generation"].as_u64().context("mounted generation absent")?,
                plugins,
            });
        }
        Ok(GetCatalogOutput { views })
    }).await.context("container schema read worker failed")??;
    ctx.caller.authority.revalidate().await?;
    ensure!(ctx.runtime.current_grants(ctx.caller).await? == rendering_grants,
        "catalog grants changed while reading the container mount");
    Ok(serde_json::to_value(output)?)
}
