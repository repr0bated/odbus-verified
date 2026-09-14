//! Install packages via PackageKit D-Bus (no CLI fallbacks).

use anyhow::{Context, Result};
use clap::Parser;
use futures::StreamExt;
use zbus::Connection;
use zbus::Proxy;

#[derive(Parser, Debug)]
#[command(name = "op-packagekit-install")]
#[command(about = "Install packages via PackageKit D-Bus using zbus")]
struct Args {
    /// Package names to install
    #[arg(required = true)]
    packages: Vec<String>,

    /// PackageKit resolve filters
    #[arg(long, default_value_t = 0)]
    resolve_filters: u64,

    /// PackageKit transaction flags
    #[arg(long, default_value_t = 0)]
    transaction_flags: u64,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let connection = Connection::system().await?;

    let package_ids = resolve_packages(&connection, args.resolve_filters, &args.packages)
        .await
        .context("Failed to resolve package IDs")?;

    if package_ids.is_empty() {
        anyhow::bail!(
            "No packages resolved for requested names: {:?}",
            args.packages
        );
    }

    install_packages(&connection, args.transaction_flags, &package_ids)
        .await
        .context("PackageKit install failed")?;

    println!(
        "Installed packages via PackageKit: {}",
        package_ids.join(", ")
    );

    Ok(())
}

async fn create_transaction(connection: &Connection) -> Result<zbus::zvariant::OwnedObjectPath> {
    let proxy = Proxy::new(
        connection,
        "org.freedesktop.PackageKit",
        "/org/freedesktop/PackageKit",
        "org.freedesktop.PackageKit",
    )
    .await?;

    let path: zbus::zvariant::OwnedObjectPath = proxy.call("CreateTransaction", &()).await?;
    Ok(path)
}

async fn resolve_packages(
    connection: &Connection,
    filters: u64,
    packages: &[String],
) -> Result<Vec<String>> {
    let tx_path = create_transaction(connection).await?;
    let tx_proxy = Proxy::new(
        connection,
        "org.freedesktop.PackageKit",
        &tx_path,
        "org.freedesktop.PackageKit.Transaction",
    )
    .await?;

    let mut package_stream = tx_proxy.receive_signal("Package").await?;
    let mut finished_stream = tx_proxy.receive_signal("Finished").await?;

    let _: () = tx_proxy
        .call("Resolve", &(filters, packages.to_vec()))
        .await?;

    let mut resolved = Vec::new();

    loop {
        tokio::select! {
            Some(signal) = package_stream.next() => {
                if let Ok((_, package_id, _)) = signal.body().deserialize::<(u32, String, String)>() {
                    resolved.push(package_id);
                }
            }
            Some(_) = finished_stream.next() => {
                break;
            }
        }
    }

    resolved.sort();
    resolved.dedup();
    Ok(resolved)
}

async fn install_packages(
    connection: &Connection,
    flags: u64,
    package_ids: &[String],
) -> Result<()> {
    let tx_path = create_transaction(connection).await?;
    let tx_proxy = Proxy::new(
        connection,
        "org.freedesktop.PackageKit",
        &tx_path,
        "org.freedesktop.PackageKit.Transaction",
    )
    .await?;

    let mut finished_stream = tx_proxy.receive_signal("Finished").await?;

    let _: () = tx_proxy
        .call("InstallPackages", &(flags, package_ids.to_vec()))
        .await?;

    // Wait for installation to complete
    if let Some(signal) = finished_stream.next().await {
        if let Ok((exit_code, _runtime)) = signal.body().deserialize::<(u32, u32)>() {
            if exit_code != 1 {
                // 1 = PK_EXIT_ENUM_SUCCESS
                anyhow::bail!("Package installation failed with exit code: {}", exit_code);
            }
        }
    }

    Ok(())
}
