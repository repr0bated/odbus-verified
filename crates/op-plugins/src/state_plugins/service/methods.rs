//! Runit lifecycle through the existing root-guarded native D-Bus provider.
use super::{InitInput, RunInput, ServiceName, ShutdownInput};
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;
use zbus::{Connection, Proxy};

const DESTINATION: &str = "org.opdbus.v1.plugins.runit";
const PATH: &str = "/org/opdbus/v1/plugins/runit";

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ServiceInitOutput {
    pub name: String,
    pub definition_type: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ServiceStatus {
    pub name: String,
    pub active_state: String,
    pub sub_state: String,
    pub main_pid: Option<u32>,
    pub up_time: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ServiceOperationOutput {
    pub name: String,
    pub operation: String,
    /// The native supervisor accepted the operation; status is its observed
    /// process state, not an application readiness/health assertion.
    pub disposition: String,
    pub status: ServiceStatus,
}

pub async fn dispatch_service_method(method: &str, args: &Value) -> Result<Value> {
    tokio::time::timeout(Duration::from_secs(30), async {
        let connection = Connection::system().await?;
        dispatch_on(&connection, method, args).await
    })
    .await
    .context(
        "runit lifecycle request timed out; supervisor state must be checked before retrying",
    )?
}

async fn dispatch_on(connection: &Connection, method: &str, args: &Value) -> Result<Value> {
    // Validate before making even a read-only provider call.
    let (name, operation, expected_type) = match method {
        "init" => {
            let input: InitInput = serde_json::from_value(args.clone())?;
            (input.name, "Enable", Some(input.service_type))
        }
        "run" => {
            let input: RunInput = serde_json::from_value(args.clone())?;
            ensure!(
                input.args.is_empty(),
                "runit arguments belong in the installed service definition"
            );
            (input.name, "Start", None)
        }
        "shutdown" => {
            let input: ShutdownInput = serde_json::from_value(args.clone())?;
            (
                input.name,
                if input.force { "ForceStop" } else { "Stop" },
                None,
            )
        }
        _ => anyhow::bail!("unknown service method: {method}"),
    };
    ServiceName::new(&name)?;
    let proxy = Proxy::new(connection, DESTINATION, PATH, DESTINATION).await?;
    let definition_type = if let Some(expected) = expected_type {
        let native: String = proxy.call("GetUnitType", &(name.as_str(),)).await?;
        ensure!(!native.is_empty(), "provider returned no definition type");
        ensure!(
            expected.is_empty() || native == expected,
            "installed service type does not match requested type"
        );
        Some(native)
    } else {
        None
    };
    let (accepted, diagnostic): (bool, String) = proxy.call(operation, &(name.as_str(),)).await?;
    ensure!(
        accepted,
        "runit {operation} rejected for {name}: {diagnostic}"
    );
    if let Some(definition_type) = definition_type {
        let enabled: String = proxy.call("IsEnabled", &(name.as_str(),)).await?;
        ensure!(
            enabled == "enabled",
            "service enablement accepted but readback is {enabled}"
        );
        return Ok(serde_json::to_value(ServiceInitOutput {
            name,
            definition_type,
            enabled: true,
        })?);
    }
    let raw: String = proxy.call("Status", &(name.as_str(),)).await?;
    let status: ServiceStatus = serde_json::from_str(&raw)
        .context("lifecycle operation accepted but provider status is unavailable")?;
    ensure!(
        status.name == name,
        "provider status refers to another service"
    );
    Ok(serde_json::to_value(ServiceOperationOutput {
        name,
        operation: operation.to_owned(),
        disposition: "accepted".to_owned(),
        status,
    })?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::{Arc, Mutex};

    struct Fixture {
        calls: Arc<Mutex<Vec<String>>>,
        reject: bool,
    }
    #[zbus::interface(name = "org.opdbus.v1.plugins.runit")]
    impl Fixture {
        fn get_unit_type(&self, _name: &str) -> &str {
            "longrun"
        }
        fn is_enabled(&self, _name: &str) -> &str {
            "enabled"
        }
        fn enable(&self, name: &str) -> (bool, String) {
            self.reply("Enable", name)
        }
        fn start(&self, name: &str) -> (bool, String) {
            self.reply("Start", name)
        }
        fn stop(&self, name: &str) -> (bool, String) {
            self.reply("Stop", name)
        }
        fn force_stop(&self, name: &str) -> (bool, String) {
            self.reply("ForceStop", name)
        }
        fn status(&self, name: &str) -> String {
            json!({"name":name,"active_state":"active","sub_state":"running","main_pid":123,"up_time":"2s"}).to_string()
        }
    }
    impl Fixture {
        fn reply(&self, operation: &str, name: &str) -> (bool, String) {
            self.calls
                .lock()
                .unwrap()
                .push(format!("{operation}:{name}"));
            (!self.reject, "fixture diagnostic".into())
        }
    }
    async fn pair(reject: bool) -> (Connection, Connection, Arc<Mutex<Vec<String>>>) {
        let (server, client) = tokio::net::UnixStream::pair().unwrap();
        let guid = zbus::Guid::generate();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let server = zbus::connection::Builder::unix_stream(server)
            .server(guid)
            .unwrap()
            .p2p()
            .serve_at(
                PATH,
                Fixture {
                    calls: calls.clone(),
                    reject,
                },
            )
            .unwrap()
            .build();
        let client = zbus::connection::Builder::unix_stream(client).p2p().build();
        let (server, client) = tokio::time::timeout(Duration::from_secs(5), async {
            tokio::try_join!(server, client)
        })
        .await
        .unwrap()
        .unwrap();
        (server, client, calls)
    }
    #[tokio::test]
    async fn lifecycle_wire_calls_and_observed_receipts() {
        let (_server, client, calls) = pair(false).await;
        let init = dispatch_on(
            &client,
            "init",
            &json!({"name":"fixture","service_type":"longrun"}),
        )
        .await
        .unwrap();
        assert_eq!(init["definition_type"], "longrun");
        for (method, args, operation) in [
            ("run", json!({"name":"fixture","args":[]}), "Start"),
            ("shutdown", json!({"name":"fixture","force":false}), "Stop"),
            (
                "shutdown",
                json!({"name":"fixture","force":true}),
                "ForceStop",
            ),
        ] {
            let result = dispatch_on(&client, method, &args).await.unwrap();
            assert_eq!(result["operation"], operation);
            assert_eq!(result["status"]["main_pid"], 123);
            assert_eq!(result["disposition"], "accepted");
        }
        assert_eq!(
            *calls.lock().unwrap(),
            [
                "Enable:fixture",
                "Start:fixture",
                "Stop:fixture",
                "ForceStop:fixture"
            ]
        );
    }
    #[tokio::test]
    async fn invalid_requests_never_reach_lifecycle_and_rejections_propagate() {
        let (_server, client, calls) = pair(false).await;
        for (method, args) in [
            ("run", json!({"name":"../bad"})),
            ("run", json!({"name":"fixture","args":["override"]})),
            ("init", json!({"name":"fixture","service_type":"mismatch"})),
        ] {
            assert!(dispatch_on(&client, method, &args).await.is_err());
        }
        assert!(calls.lock().unwrap().is_empty());
        let (_server, client, _) = pair(true).await;
        assert!(dispatch_on(&client, "run", &json!({"name":"fixture"}))
            .await
            .unwrap_err()
            .to_string()
            .contains("rejected"));
    }
}
