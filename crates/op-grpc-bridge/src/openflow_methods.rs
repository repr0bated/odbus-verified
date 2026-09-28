//! Canonical exact compare-and-exchange adapter for OpenFlow flow members.

use anyhow::{Context, Result, ensure};
use op_plugins::state_plugins::openflow::{
    AddFlowInput, DeleteFlowInput, FlowAction, FlowEntry, FlowMutationKind, FlowMutationOutput,
    ModifyFlowInput,
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use zbus::{Connection, Proxy};

const SERVICE: &str = "org.opdbus.v1.plugins.openflow";
const PATH: &str = "/org/opdbus/v1/plugins/openflow";
const INTERFACE: &str = "org.opdbus.v1.plugins.openflow";
const CALL_TIMEOUT: Duration = Duration::from_secs(45);

#[derive(Debug, Clone)]
pub struct OpenFlowMethodReceipt {
    pub output: Value,
    /// Exact member known to be live immediately before the operation.
    pub before: Option<FlowEntry>,
    /// Exact member confirmed live after the operation; absent for delete.
    pub after: Option<FlowEntry>,
    pub bridge: String,
}

pub(crate) struct OpenFlowProjectionChange {
    pub object_path: String,
    pub change_type: crate::mutation_engine::ChangeType,
    pub before: Option<FlowEntry>,
    pub value: op_plugins::state_plugins::openflow::FlowObservation,
}

pub(crate) fn projection_change(
    receipt: &OpenFlowMethodReceipt,
) -> Result<OpenFlowProjectionChange> {
    let observed = receipt
        .after
        .as_ref()
        .or(receipt.before.as_ref())
        .context("OpenFlow receipt omitted both member revisions")?;
    let identity = canonical_flow_identity(observed);
    Ok(OpenFlowProjectionChange {
        object_path: format!(
            "{}/observed_flows/{}/{}",
            op_plugins::canonical::plugin_path("openflow"),
            receipt.bridge,
            identity
        ),
        change_type: crate::mutation_engine::ChangeType::ObjectAdded,
        before: receipt.before.clone(),
        value: op_plugins::state_plugins::openflow::FlowObservation {
            flow: observed.clone(),
            installed: receipt.after.is_some(),
        },
    })
}

fn canonical_flow_identity(flow: &FlowEntry) -> String {
    let mut digest = Sha256::new();
    digest.update([flow.table]);
    digest.update(flow.priority.to_be_bytes());
    let mut fields = flow.match_fields.iter().collect::<Vec<_>>();
    fields.sort_unstable();
    for (key, value) in fields {
        digest.update((key.len() as u64).to_be_bytes());
        digest.update(key.as_bytes());
        digest.update((value.len() as u64).to_be_bytes());
        digest.update(value.as_bytes());
    }
    hex::encode(digest.finalize())
}

struct CasRequest {
    bridge: String,
    desired: FlowEntry,
    expected: Option<FlowEntry>,
    delete: bool,
    operation: FlowMutationKind,
}

pub async fn dispatch_openflow_flow_method(
    method: &str,
    args: &Value,
) -> Result<OpenFlowMethodReceipt> {
    let connection = tokio::time::timeout(Duration::from_secs(10), Connection::system())
        .await
        .context("OpenFlow owner bus connection timed out")?
        .context("connect to the canonical OpenFlow D-Bus owner")?;
    tokio::time::timeout(
        CALL_TIMEOUT,
        dispatch_openflow_flow_method_on(&connection, method, args),
    )
    .await
    .context("OpenFlow compare/exchange timed out; reconcile the exact member before retrying")?
}

pub(crate) async fn dispatch_openflow_flow_method_on(
    connection: &Connection,
    method: &str,
    args: &Value,
) -> Result<OpenFlowMethodReceipt> {
    let request = cas_request(method, args)?;
    let desired_json = serde_json::to_string(&request.desired)?;
    let expected_json = request
        .expected
        .as_ref()
        .map(serde_json::to_string)
        .transpose()?
        .unwrap_or_default();
    let proxy = Proxy::new(connection, SERVICE, PATH, INTERFACE)
        .await
        .context("create OpenFlow controller D-Bus proxy")?;
    let response: String = proxy
        .call(
            "CompareExchangeFlow",
            &(
                request.bridge.as_str(),
                desired_json.as_str(),
                expected_json.as_str(),
                request.delete,
            ),
        )
        .await
        .context("OpenFlow native compare/exchange failed")?;
    let native: Value = serde_json::from_str(&response)
        .context("OpenFlow owner returned malformed compare/exchange readback")?;
    ensure!(
        native.get("verified").and_then(Value::as_bool) == Some(true),
        "OpenFlow owner did not return a verified readback"
    );
    let expected_action = if request.delete { "delete" } else { "add" };
    ensure!(
        native.get("action").and_then(Value::as_str) == Some(expected_action),
        "OpenFlow owner returned the wrong operation"
    );
    let returned: FlowEntry = serde_json::from_value(
        native
            .get("flow")
            .cloned()
            .context("OpenFlow owner omitted the observed member")?,
    )?;
    ensure!(
        returned == request.desired,
        "OpenFlow owner observed a different member"
    );
    let (before, after) = if request.delete {
        (request.expected, None)
    } else {
        (request.expected, Some(request.desired))
    };
    let output = serde_json::to_value(FlowMutationOutput {
        bridge: request.bridge.clone(),
        operation: request.operation,
        before: before.clone(),
        after: after.clone(),
        verified: true,
        observed_at_unix: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64,
    })?;
    Ok(OpenFlowMethodReceipt {
        output,
        before,
        after,
        bridge: request.bridge,
    })
}

fn cas_request(method: &str, args: &Value) -> Result<CasRequest> {
    let request = match method {
        "add_flow" => {
            let input: AddFlowInput = serde_json::from_value(args.clone())?;
            CasRequest {
                bridge: input.bridge,
                desired: input.flow,
                expected: None,
                delete: false,
                operation: FlowMutationKind::Added,
            }
        }
        "delete_flow" => {
            let input: DeleteFlowInput = serde_json::from_value(args.clone())?;
            CasRequest {
                bridge: input.bridge,
                desired: input.expected.clone(),
                expected: Some(input.expected),
                delete: true,
                operation: FlowMutationKind::Deleted,
            }
        }
        "modify_flow" => {
            let input: ModifyFlowInput = serde_json::from_value(args.clone())?;
            ensure!(
                flow_identity(&input.expected) == flow_identity(&input.replacement),
                "modify_flow cannot change table, priority or match_fields"
            );
            CasRequest {
                bridge: input.bridge,
                desired: input.replacement,
                expected: Some(input.expected),
                delete: false,
                operation: FlowMutationKind::Modified,
            }
        }
        _ => anyhow::bail!("unknown OpenFlow member method: {method}"),
    };
    validate_bridge(&request.bridge)?;
    validate_flow(&request.desired)?;
    if let Some(expected) = &request.expected {
        validate_flow(expected)?;
    }
    Ok(request)
}

fn flow_identity(flow: &FlowEntry) -> (u8, u16, Vec<(&str, &str)>) {
    let mut fields = flow
        .match_fields
        .iter()
        .map(|(key, value)| (key.as_str(), value.as_str()))
        .collect::<Vec<_>>();
    fields.sort_unstable();
    (flow.table, flow.priority, fields)
}

fn validate_bridge(bridge: &str) -> Result<()> {
    ensure!(
        !bridge.is_empty()
            && bridge.len() <= 15
            && bridge
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.')),
        "invalid OpenFlow bridge name"
    );
    Ok(())
}

fn validate_flow(flow: &FlowEntry) -> Result<()> {
    ensure!(flow.table < u8::MAX, "OpenFlow table 255 is reserved");
    ensure!(
        flow.priority > 0,
        "priority 0 is reserved for the protected NORMAL fallback"
    );
    ensure!(!flow.actions.is_empty(), "flow actions must be explicit");
    ensure!(
        !flow
            .actions
            .iter()
            .any(|action| matches!(action, FlowAction::Drop))
            || (flow.actions.len() == 1 && matches!(flow.actions.first(), Some(FlowAction::Drop))),
        "drop must be the only flow action"
    );
    ensure!(
        flow.match_fields.iter().all(|(key, value)| {
            !key.is_empty()
                && key.len() <= 64
                && value.len() <= 256
                && !key.chars().any(char::is_control)
                && !value.chars().any(char::is_control)
        }),
        "invalid OpenFlow match field"
    );
    for action in &flow.actions {
        match action {
            FlowAction::Output { port } => ensure!(!port.is_empty(), "output port is empty"),
            FlowAction::SetField { field, value } => ensure!(
                !field.is_empty() && !value.is_empty(),
                "set_field requires field and value"
            ),
            FlowAction::ArpResponder { mac, ip } => {
                ensure!(mac.split(':').count() == 6, "invalid ARP responder MAC");
                ip.parse::<std::net::Ipv4Addr>()
                    .context("invalid ARP responder IPv4 address")?;
            }
            _ => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        mutation_engine::ChangeType,
        state_tree_fold::{StateTreeFold, fold_plugin_state},
    };
    use simd_json::json as simd_json_value;
    use simd_json::prelude::*;
    use std::collections::HashMap;

    fn flow(actions: Vec<FlowAction>) -> FlowEntry {
        FlowEntry {
            table: 4,
            priority: 200,
            match_fields: HashMap::from([("in_port".into(), "uplink".into())]),
            actions,
            cookie: Some(42),
            idle_timeout: 0,
            hard_timeout: 0,
        }
    }

    #[test]
    fn three_methods_build_exact_compare_exchange_requests() {
        let before = flow(vec![FlowAction::Normal]);
        let after = flow(vec![FlowAction::Output {
            port: "LOCAL".into(),
        }]);
        let add = cas_request(
            "add_flow",
            &serde_json::json!({"bridge":"ovsbr0","flow":before.clone()}),
        )
        .unwrap();
        assert!(add.expected.is_none() && !add.delete);
        let delete = cas_request(
            "delete_flow",
            &serde_json::json!({"bridge":"ovsbr0","expected":before.clone()}),
        )
        .unwrap();
        assert_eq!(delete.expected.as_ref(), Some(&delete.desired));
        assert!(delete.delete);
        let modify = cas_request(
            "modify_flow",
            &serde_json::json!({"bridge":"ovsbr0","expected":before,"replacement":after}),
        )
        .unwrap();
        assert!(modify.expected.is_some() && !modify.delete);
    }

    #[test]
    fn wildcard_delete_identity_change_and_fallback_are_unrepresentable() {
        assert!(
            cas_request(
                "delete_flow",
                &serde_json::json!({"bridge":"ovsbr0","match_fields":{}})
            )
            .is_err()
        );
        let expected = flow(vec![FlowAction::Normal]);
        let mut changed = expected.clone();
        changed.priority += 1;
        assert!(
            cas_request(
                "modify_flow",
                &serde_json::json!({"bridge":"ovsbr0","expected":expected,"replacement":changed})
            )
            .is_err()
        );
        let mut fallback = flow(vec![FlowAction::Normal]);
        fallback.priority = 0;
        assert!(
            cas_request(
                "add_flow",
                &serde_json::json!({"bridge":"ovsbr0","flow":fallback})
            )
            .is_err()
        );
    }

    #[test]
    fn observed_member_fold_preserves_authored_config_and_unrelated_bridges() {
        let before = flow(vec![FlowAction::Normal]);
        let after = flow(vec![FlowAction::Output {
            port: "LOCAL".into(),
        }]);
        let add = projection_change(&OpenFlowMethodReceipt {
            output: Value::Null,
            before: None,
            after: Some(before.clone()),
            bridge: "ovsbr0".into(),
        })
        .unwrap();
        let initial = simd_json_value!({
            "bridges":[{"name":"configured","flows":[],"socket_ports":null}],
            "controller_endpoint":"tcp:127.0.0.1:6653",
            "auto_discover_containers":false,
            "enable_security_flows":true,
            "obfuscation_level":1,
            "observed_flows":{"other":{"keep":{"installed":true,"flow":{"table":1,"priority":1,"match_fields":{},"actions":[{"type":"normal"}],"idle_timeout":0,"hard_timeout":0}}}}
        });
        let StateTreeFold::Present(added) = fold_plugin_state(
            Some(&initial),
            "openflow",
            &add.object_path,
            add.change_type,
            None,
            &simd_json::serde::to_owned_value(add.value).unwrap(),
        )
        .unwrap() else {
            panic!("add must produce state")
        };
        assert_eq!(added["bridges"], initial["bridges"]);
        assert_eq!(
            added["observed_flows"]["other"],
            initial["observed_flows"]["other"]
        );

        let modify = projection_change(&OpenFlowMethodReceipt {
            output: Value::Null,
            before: Some(before),
            after: Some(after.clone()),
            bridge: "ovsbr0".into(),
        })
        .unwrap();
        assert_eq!(add.object_path, modify.object_path);
        let StateTreeFold::Present(modified) = fold_plugin_state(
            Some(&added),
            "openflow",
            &modify.object_path,
            ChangeType::ObjectAdded,
            None,
            &simd_json::serde::to_owned_value(modify.value).unwrap(),
        )
        .unwrap() else {
            panic!("modify must produce state")
        };
        let identity = canonical_flow_identity(&after);
        assert_eq!(
            modified["observed_flows"]["ovsbr0"][identity.as_str()]["flow"]["actions"][0]["type"],
            "output"
        );

        let delete = projection_change(&OpenFlowMethodReceipt {
            output: Value::Null,
            before: Some(after),
            after: None,
            bridge: "ovsbr0".into(),
        })
        .unwrap();
        let StateTreeFold::Present(deleted) = fold_plugin_state(
            Some(&modified),
            "openflow",
            &delete.object_path,
            delete.change_type,
            None,
            &simd_json::serde::to_owned_value(delete.value).unwrap(),
        )
        .unwrap() else {
            panic!("delete must retain the plugin state")
        };
        assert_eq!(
            deleted["observed_flows"]["ovsbr0"][identity.as_str()]["installed"],
            false
        );
        assert_eq!(deleted["bridges"], initial["bridges"]);
        assert!(deleted["observed_flows"]["other"]["keep"].is_object());
    }
}
