//! s6d — D-Bus wrapper for the canonical runit control-plane service.

use anyhow::{bail, Context, Result};
use zbus::{proxy, Connection};

const RUNIT_BUS_NAME: &str = "org.opdbus.v1.Runit.Systemctl";
const RUNIT_OBJECT_PATH: &str = "/org/opdbus/v1/plugins/runit/systemctl";

#[proxy(
    default_service = "org.opdbus.v1.Runit.Systemctl",
    default_path = "/org/opdbus/v1/plugins/runit/systemctl",
    interface = "org.opdbus.v1.Runit.Systemctl"
)]
trait RunitSystemctl {
    async fn start(&self, unit: &str) -> zbus::Result<(bool, String)>;
    async fn stop(&self, unit: &str) -> zbus::Result<(bool, String)>;
    async fn restart(&self, unit: &str) -> zbus::Result<(bool, String)>;
    async fn reload(&self, unit: &str) -> zbus::Result<(bool, String)>;
    async fn enable(&self, unit: &str) -> zbus::Result<(bool, String)>;
    async fn disable(&self, unit: &str) -> zbus::Result<(bool, String)>;
    async fn status(&self, unit: &str) -> zbus::Result<String>;
    async fn is_active(&self, unit: &str) -> zbus::Result<String>;
    async fn is_enabled_method(&self, unit: &str) -> zbus::Result<String>;
    async fn show(&self, unit: &str) -> zbus::Result<String>;
    async fn list_units(&self) -> zbus::Result<String>;
    async fn list_unit_files(&self) -> zbus::Result<String>;
    async fn journalctl(&self, unit: &str, lines: u32) -> zbus::Result<String>;
    async fn daemon_reload(&self) -> zbus::Result<(bool, String)>;
    async fn daemon_status(&self) -> zbus::Result<String>;
    async fn get_unit_type(&self, unit: &str) -> zbus::Result<String>;
}

#[tokio::main]
async fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let Some(command) = args.next() else {
        usage();
        return Ok(());
    };

    let conn = Connection::system()
        .await
        .context("connect to system D-Bus for RunitSystemctl")?;
    let proxy = connect_proxy(&conn).await?;

    match command.as_str() {
        "start" => print_result(proxy.start(&required_unit(args)?).await?)?,
        "stop" => print_result(proxy.stop(&required_unit(args)?).await?)?,
        "restart" => print_result(proxy.restart(&required_unit(args)?).await?)?,
        "reload" => print_result(proxy.reload(&required_unit(args)?).await?)?,
        "enable" => print_result(proxy.enable(&required_unit(args)?).await?)?,
        "disable" => print_result(proxy.disable(&required_unit(args)?).await?)?,
        "status" => println!("{}", proxy.status(&required_unit(args)?).await?),
        "is-active" => println!("{}", proxy.is_active(&required_unit(args)?).await?),
        "is-enabled" => println!("{}", proxy.is_enabled_method(&required_unit(args)?).await?),
        "show" => println!("{}", proxy.show(&required_unit(args)?).await?),
        "list-units" => println!("{}", proxy.list_units().await?),
        "list-unit-files" => println!("{}", proxy.list_unit_files().await?),
        "journalctl" => {
            let unit = required_unit(args.by_ref())?;
            let lines = args
                .next()
                .and_then(|raw| raw.parse::<u32>().ok())
                .unwrap_or(80);
            println!("{}", proxy.journalctl(&unit, lines).await?);
        }
        "daemon-reload" => print_result(proxy.daemon_reload().await?)?,
        "daemon-status" => println!("{}", proxy.daemon_status().await?),
        "get-unit-type" => println!("{}", proxy.get_unit_type(&required_unit(args)?).await?),
        "help" | "--help" | "-h" => usage(),
        other => {
            usage();
            bail!("unknown s6d command: {other}");
        }
    }

    Ok(())
}

async fn connect_proxy(conn: &Connection) -> Result<RunitSystemctlProxy<'_>> {
    RunitSystemctlProxy::builder(conn)
        .destination(RUNIT_BUS_NAME)
        .map_err(|error| anyhow::anyhow!("{error}"))?
        .path(RUNIT_OBJECT_PATH)
        .map_err(|error| anyhow::anyhow!("{error}"))?
        .build()
        .await
        .with_context(|| format!("connect to {RUNIT_BUS_NAME}"))
}

fn required_unit(mut args: impl Iterator<Item = String>) -> Result<String> {
    args.next().context("missing unit name")
}

fn print_result((ok, message): (bool, String)) -> Result<()> {
    println!("{message}");
    if ok {
        Ok(())
    } else {
        bail!("{message}")
    }
}

fn usage() {
    eprintln!(
        "usage: s6d <command> [unit] [args]\n\
         commands: start stop restart reload enable disable status is-active is-enabled show \n\
         commands: list-units list-unit-files journalctl daemon-reload daemon-status get-unit-type"
    );
}
