//! One native operation inside an identity-owned guest. The host admits and
//! records the enclosing operation and its owned-container exec. This process
//! has no listener, grant owner, host target or independent plugin schema.
use anyhow::{Context, Result, ensure};
use op_plugins::state_plugins::{btrfs_plugin, packagekit, rtnetlink, service, software, users};
use op_state::StatePlugin;
use op_state_store::SideEffect;
use serde_json::{Value, json};

#[path = "op-guest-backend/service.rs"]
mod guest_service;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("guest backend: {error:#}");
        std::process::exit(1);
    }
}

async fn run() -> Result<()> {
    let mut argv = std::env::args().skip(1);
    let session = argv.next().context("guest session is required")?;
    let plugin = argv.next().context("plugin is required")?;
    let method = argv.next().context("method is required")?;
    let contract: Value = serde_json::from_str(&argv.next().context("sealed method contract is required")?)?;
    let args: Value = serde_json::from_str(&argv.next().context("method arguments are required")?)?;
    ensure!(argv.next().is_none(), "unexpected guest backend argument");
    ensure!(std::fs::read_to_string("/agent/user/session_id")?.trim() == session,
        "guest backend identity differs from the admitted session");
    let schema = match plugin.as_str() {
        "users" => users::UsersPlugin::default().schema(),
        "software" => software::SoftwarePlugin::default().schema(),
        "packagekit" => packagekit::PackageKitPlugin::default().schema(),
        "rtnetlink" => rtnetlink::RtnetlinkPlugin::default().schema(),
        "service" => service::ServicePlugin::default().schema(),
        "btrfs" => btrfs_plugin::BtrfsPlugin::default().schema(),
        _ => anyhow::bail!("unsupported guest backend plugin"),
    }.context("guest plugin schema is absent")?;
    ensure!(contract["version"].as_str() == Some(schema.version.as_str()),
        "guest plugin version differs from the sealed contract");
    let declaration = schema.methods.get(&method).context("guest method is absent")?;
    ensure!(serde_json::to_value(declaration)? == contract["declaration"],
        "guest method differs from the sealed contract");
    let input = serde_json::to_value(&declaration.args)?;
    jsonschema::validator_for(&input)?.validate(&args)
        .map_err(|error| anyhow::anyhow!("guest method arguments violate its sealed schema: {error}"))?;
    let output = serde_json::to_value(declaration.returns.as_ref().context("guest method output schema is absent")?)?;
    let output_validator = jsonschema::validator_for(&output)?;
    let result = match plugin.as_str() {
        "users" => users::dispatch_users_method(&method, &args).await?,
        "software" => software::dispatch_software_method(&method, &args).await?,
        "packagekit" => packagekit::dispatch_packagekit_method(&method, &args).await?,
        "rtnetlink" => rtnetlink::dispatch_rtnetlink_method(&method, &args).await?,
        "service" => guest_service::dispatch(&method, &args).await?,
        "btrfs" => btrfs_plugin::dispatch_btrfs_method(&method, &args).await?,
        _ => unreachable!("plugin schema was selected above"),
    };
    output_validator.validate(&result)
        .map_err(|error| anyhow::anyhow!("guest result violates its sealed schema: {error}"))?;
    let observation = match plugin.as_str() {
        "software" | "packagekit" if declaration.side_effect == SideEffect::Mutation => {
            serde_json::to_value(packagekit::installed_package_inventory().await?)?
        }
        "rtnetlink" => json!({
            "interfaces": op_network::rtnetlink::list_interfaces_strict().await?,
            "default_route": op_network::rtnetlink::get_default_route().await?,
        }),
        _ => Value::Null,
    };
    serde_json::to_writer(std::io::stdout().lock(), &json!({"result":result,"observation":observation}))?;
    Ok(())
}
