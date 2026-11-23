//! The subsidiary boots systemd; only its own system bus is reachable here.
//! Host runit control continues to use the existing host service adapter.
use anyhow::{Context, Result, ensure};
use op_plugins::{service_def::ServiceName, state_plugins::service::{InitInput, RunInput, ShutdownInput, ServiceInitOutput, ServiceOperationOutput, ServiceStatus}};
use serde_json::Value;
use zbus::{Connection, Proxy, zvariant::OwnedObjectPath};

const DESTINATION: &str = "org.freedesktop.systemd1";
const MANAGER: &str = "/org/freedesktop/systemd1";

pub(super) async fn dispatch(method: &str, args: &Value) -> Result<Value> {
    tokio::time::timeout(std::time::Duration::from_secs(30), dispatch_inner(method, args)).await
        .context("guest systemd operation timed out; inspect the execution receipt before retrying")?
}

async fn dispatch_inner(method: &str, args: &Value) -> Result<Value> {
    let (name, expected_type, force) = match method {
        "init" => {
            let input: InitInput = serde_json::from_value(args.clone())?;
            (input.name, Some(input.service_type), false)
        }
        "run" => {
            let input: RunInput = serde_json::from_value(args.clone())?;
            ensure!(input.args.is_empty(), "runtime arguments belong in the installed guest unit");
            (input.name, None, false)
        }
        "shutdown" => {
            let input: ShutdownInput = serde_json::from_value(args.clone())?;
            (input.name, None, input.force)
        }
        _ => anyhow::bail!("unknown guest service method"),
    };
    ServiceName::new(&name)?;
    let unit_name = if name.ends_with(".service") { name.clone() } else { format!("{name}.service") };
    let connection = Connection::system().await?;
    let manager = Proxy::new(&connection, DESTINATION, MANAGER, "org.freedesktop.systemd1.Manager").await?;
    let path: OwnedObjectPath = manager.call("LoadUnit", &(unit_name.as_str(),)).await?;
    let unit = Proxy::new(&connection, DESTINATION, path.as_str(), "org.freedesktop.systemd1.Unit").await?;
    let service = Proxy::new(&connection, DESTINATION, path.as_str(), "org.freedesktop.systemd1.Service").await?;
    let load_state: String = unit.get_property("LoadState").await?;
    ensure!(load_state == "loaded", "guest service is not a loaded installed unit");
    if let Some(expected) = expected_type {
        let fragment: String = unit.get_property("FragmentPath").await?;
        ensure!(!fragment.is_empty(), "guest enablement requires an installed unit definition");
        let definition_type: String = service.get_property("Type").await?;
        ensure!(expected.is_empty() || expected == definition_type, "guest service type differs from the requested type");
        let _: (bool, Vec<(String, String, String)>) = manager.call(
            "EnableUnitFiles", &(vec![unit_name.as_str()], false, false),
        ).await?;
        let enabled: String = manager.call("GetUnitFileState", &(unit_name.as_str(),)).await?;
        ensure!(enabled == "enabled", "guest unit enablement readback is not enabled");
        return Ok(serde_json::to_value(ServiceInitOutput { name, definition_type, enabled: true })?);
    }
    if force {
        manager.call::<_, _, ()>("KillUnit", &(unit_name.as_str(), "all", 9i32)).await?;
    }
    let operation = if method == "run" { "Start" } else if force { "ForceStop" } else { "Stop" };
    let native_method = if method == "run" { "StartUnit" } else { "StopUnit" };
    let _: OwnedObjectPath = manager.call(native_method, &(unit_name.as_str(), "replace")).await?;
    let active_state: String = unit.get_property("ActiveState").await?;
    let sub_state: String = unit.get_property("SubState").await?;
    let pid: u32 = service.get_property("MainPID").await?;
    // The supervisor accepted the job; this is a current observation, not a
    // claim that the application is ready or that an asynchronous job finished.
    let output = ServiceOperationOutput {
        name: name.clone(), operation: operation.to_owned(), disposition: "accepted".into(),
        status: ServiceStatus {
            name, active_state, sub_state,
            main_pid: if pid == 0 { None } else { Some(pid) }, up_time: None,
        },
    };
    Ok(serde_json::to_value(output)?)
}
