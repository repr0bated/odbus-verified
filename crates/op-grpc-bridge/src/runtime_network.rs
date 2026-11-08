//! Admitted network methods backed by the retained native owners. No shell,
//! local WireGuard fallback, separate identity authority, or adapter receipts.
use crate::{
    core::{BackendProjection, BackendStateUpdate, StateScope},
    openflow_permit::{FinalAdmission, OpenFlowPermitAuthority},
    runtime::{BackendContext, BackendOutcome, Runtime, RuntimeError, VerifiedCaller},
};
use anyhow::{Context, Result, ensure};
use op_network::rovs_proxy::{OvsdbDbusClient, monitor::ProtocolObservation};
use op_plugins::state_plugins::{
    openflow::{FlowAction, FlowEntry},
    ovsdb_bridge::{self as ovs, OvsdbMonitorState},
    rovs_commands as rovs,
    runtime_documents::RuntimeDocuments,
};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashMap},
    sync::{Arc, Mutex as StdMutex, atomic::{AtomicBool, Ordering}},
    time::Duration,
};
use tokio::sync::{Mutex, OnceCell, OwnedMutexGuard, oneshot};
use tokio::task::JoinHandle;

pub(crate) fn supports(plugin: &str, method: &str) -> bool {
    match plugin {
        "openflow" => matches!(method, "add_flow" | "delete_flow" | "modify_flow"
            | "ensure_fallback_normal" | "set_fail_mode" | "del_controller"
            | "set_controller" | "get_datapath_health" | "attach_controller_safe"),
        "openflow_obfuscation" => matches!(method, "obfuscate_flow" | "deobfuscate_flow"),
        "ovsdb_bridge" => matches!(method, "list_dbs" | "get_schema" | "transact"
            | "monitor" | "monitor_cond" | "cancel" | "lock" | "steal" | "unlock"
            | "echo" | "create_bridge" | "delete_bridge" | "add_port" | "remove_port"),
        "rovs_commands" => matches!(method, "create_bridge" | "delete_bridge" | "add_port"
            | "ensure_internal_port" | "remove_port" | "list_bridges" | "list_ports" | "list_dbs"),
        "proxy_server" => matches!(method, "start_proxy" | "stop_proxy" | "configure"),
        "wireguard" => matches!(method, "set_device" | "get_device" | "add_peer"
            | "remove_peer" | "list_peers" | "set_config"),
        "wg_opdbus" => matches!(method, "create_interface" | "get_status"),
        "wgcf" => matches!(method, "register" | "update" | "status" | "rotate_keys"
            | "set_endpoint" | "generate_config" | "apply_config" | "refresh"),
        "xray" => matches!(method, "get_stats" | "add_user" | "remove_user" | "add_inbound"),
        _ => false,
    }
}

#[derive(Default)]
struct Effect {
    started: AtomicBool,
    denied: AtomicBool,
}
impl Effect {
    fn admission(self: &Arc<Self>, ctx: &BackendContext<'_>, plugin: &str, method: &str) -> FinalAdmission {
        let effect = self.clone();
        let runtime = ctx.runtime.clone();
        let caller = ctx.caller.clone();
        let plugin = plugin.to_owned();
        let method = method.to_owned();
        let has_effect = ctx.declaration.side_effect == op_state_store::SideEffect::Mutation
            || (plugin == "ovsdb_bridge" && matches!(method.as_str(), "monitor" | "monitor_cond"));
        Arc::new(move || {
            let effect = effect.clone();
            let runtime = runtime.clone();
            let caller = caller.clone();
            let plugin = plugin.clone();
            let method = method.clone();
            Box::pin(async move {
                if let Err(error) = runtime.authorize_nested(&caller, &plugin, &method).await {
                    effect.denied.store(true, Ordering::Release);
                    return Err(error.into());
                }
                if has_effect {
                    effect.started.store(true, Ordering::Release);
                }
                Ok(())
            })
        })
    }
    fn classify(&self, error: anyhow::Error) -> RuntimeError {
        // A later denial cannot erase an earlier effect in a composed method.
        if self.started.load(Ordering::Acquire) {
            RuntimeError::unknown(error)
        } else if self.denied.load(Ordering::Acquire) {
            RuntimeError::denied(error.to_string())
        } else {
            RuntimeError::before_effect(error)
        }
    }
}

struct MethodGuard {
    _operation: OwnedMutexGuard<()>,
    operation_id: uuid::Uuid,
    pending: Arc<StdMutex<HashMap<uuid::Uuid, oneshot::Sender<()>>>>,
}
impl Drop for MethodGuard {
    fn drop(&mut self) {
        // Dropping an outcome also happens when canonical commit fails. Only
        // after_commit may release a new observer; dropping cancels its barrier.
        if let Ok(mut pending) = self.pending.lock() {
            pending.remove(&self.operation_id);
        }
    }
}

struct MonitorEntry {
    owner: String,
    state: OvsdbMonitorState,
    task: JoinHandle<()>,
}

pub(crate) struct NetworkRuntime {
    ovsdb: OvsdbDbusClient,
    documents: Arc<RuntimeDocuments>,
    proxy: crate::proxy_methods::ProxyMethods,
    proxy_caller: Arc<Mutex<Option<VerifiedCaller>>>,
    permits: OnceCell<OpenFlowPermitAuthority>,
    operations: Arc<Mutex<()>>,
    monitors: Mutex<BTreeMap<String, MonitorEntry>>,
    pending_observers: Arc<StdMutex<HashMap<uuid::Uuid, oneshot::Sender<()>>>>,
}

impl NetworkRuntime {
    pub(crate) fn new(documents: Arc<RuntimeDocuments>) -> Self {
        Self {
            ovsdb: OvsdbDbusClient::new(),
            proxy: crate::proxy_methods::ProxyMethods::new(documents.clone()),
            documents,
            proxy_caller: Arc::new(Mutex::new(None)),
            permits: OnceCell::new(),
            operations: Arc::new(Mutex::new(())),
            monitors: Mutex::new(BTreeMap::new()),
            pending_observers: Arc::new(StdMutex::new(HashMap::new())),
        }
    }

    async fn permits(&self) -> Result<&OpenFlowPermitAuthority> {
        self.permits.get_or_try_init(|| async {
            let bus = tokio::time::timeout(Duration::from_secs(10), zbus::Connection::system())
                .await.context("OpenFlow admission bus timed out")??;
            OpenFlowPermitAuthority::new(bus).await
        }).await
    }

    pub(crate) async fn dispatch(
        &self, ctx: &BackendContext<'_>, plugin: &str, method: &str, args: &Value,
    ) -> std::result::Result<BackendOutcome, RuntimeError> {
        if !supports(plugin, method) {
            return Err(RuntimeError::unsupported(format!("unknown network method {plugin}.{method}")));
        }
        let guard = MethodGuard {
            _operation: self.operations.clone().lock_owned().await,
            operation_id: ctx.operation_id,
            pending: self.pending_observers.clone(),
        };
        ctx.runtime.authorize_nested(ctx.caller, plugin, method).await?;
        let effect = Arc::new(Effect::default());
        let admission = effect.admission(ctx, plugin, method);
        match self.dispatch_owned(ctx, plugin, method, args, admission).await {
            Ok(mut outcome) => {
                outcome.guard = Some(Box::new(guard));
                Ok(outcome)
            }
            Err(error) => {
                let mut failure = effect.classify(error);
                // Failed native operations are never replayed. If readback is
                // available, retain it even when the operation outcome is unknown.
                if effect.started.load(Ordering::Acquire)
                    && matches!(plugin, "ovsdb_bridge" | "rovs_commands" | "openflow") {
                    if let Ok(state) = self.topology().await {
                        if plugin == "ovsdb_bridge" {
                            let private = private_ovs_method(method);
                            failure = failure.with_state(BackendStateUpdate::Merge(state.clone()),
                                if private { private_scope(ctx.caller) } else { StateScope::Public });
                            if private {
                                if let Ok(write) = projection(ctx, "ovsdb_bridge", state, StateScope::Public) {
                                    failure = failure.with_additional(vec![write]);
                                }
                            }
                        } else if let Ok(write) = projection(ctx, "ovsdb_bridge", state, StateScope::Public) {
                            failure = failure.with_additional(vec![write]);
                        }
                    }
                } else if effect.started.load(Ordering::Acquire) && plugin == "proxy_server" {
                    if let Ok(state) = self.proxy.snapshot().await {
                        if let Ok(state) = serde_json::to_value(state) {
                            failure = failure.with_state(BackendStateUpdate::Replace(state), StateScope::Public);
                        }
                    }
                }
                Err(failure.with_guard(Box::new(guard)))
            }
        }
    }

    async fn dispatch_owned(
        &self, ctx: &BackendContext<'_>, plugin: &str, method: &str, args: &Value,
        admission: FinalAdmission,
    ) -> Result<BackendOutcome> {
        match plugin {
            "wireguard" | "wg_opdbus" | "wgcf" => {
                let mut receipt = crate::decoy_network::dispatch(ctx, plugin, method, args, admission).await?;
                let state = receipt.states.remove(plugin).context("decoy omitted called plugin state")?;
                let scope = private_scope(ctx.caller);
                let mut outcome = outcome(receipt.output, BackendStateUpdate::Replace(state), scope.clone());
                for (other, state) in receipt.states {
                    let mut write = projection(ctx, &other, state, scope.clone())?;
                    write.state = match write.state {
                        BackendStateUpdate::Merge(value) => BackendStateUpdate::Replace(value),
                        update => update,
                    };
                    ensure!(receipt.state_schema_hashes.get(&other) == Some(&write.schema_hash),
                        "decoy observation schema changed before commit");
                    outcome.additional.push(write);
                }
                Ok(outcome)
            }
            "proxy_server" => {
                let caller_slot = self.proxy_caller.clone();
                let admission: FinalAdmission = Arc::new(move || {
                    let admission = admission.clone();
                    let caller_slot = caller_slot.clone();
                    Box::pin(async move {
                        admission().await?;
                        // A new native effect must receive its own durable
                        // terminal before any subsequent event publication.
                        caller_slot.lock().await.take();
                        Ok(())
                    })
                });
                let (result, state) = self.proxy.dispatch_with_admission(method, args.clone(), admission).await?;
                Ok(outcome(result, BackendStateUpdate::Replace(serde_json::to_value(state)?), StateScope::Public))
            }
            "openflow_obfuscation" => {
                let owner = Arc::new(crate::obfuscation_methods::NativeFlowOwner {
                    authority: self.permits().await?.clone(), admission: admission.clone(),
                });
                let service = crate::obfuscation_methods::ObfuscationMethods::new(self.documents.clone(), owner);
                let (result, state) = service.dispatch_with_admission(method, args.clone(), admission).await?;
                Ok(outcome(result, BackendStateUpdate::Replace(serde_json::to_value(state)?), StateScope::Public))
            }
            "openflow" => self.openflow(ctx, method, args, admission).await,
            "ovsdb_bridge" => self.ovsdb(ctx, method, args, admission).await,
            "rovs_commands" => self.rovs(ctx, method, args, admission).await,
            "xray" => {
                let result = if method == "get_stats" {
                    crate::xray_methods::dispatch_stats_method(args).await?
                } else {
                    crate::xray_methods::dispatch_handler_method(method, args, admission).await?
                };
                // A successful commander reply observes the running process.
                // Runtime handlers do not rewrite Xray's authored bootstrap config.
                let state = json!({"running":true});
                let mut outcome = outcome(result, BackendStateUpdate::Merge(state.clone()), private_scope(ctx.caller));
                outcome.additional.push(projection(ctx, "xray", state, StateScope::Public)?);
                Ok(outcome)
            }
            _ => unreachable!("supports checked before native dispatch"),
        }
    }

    async fn topology(&self) -> Result<Value> {
        let state = ovs::OvsBridgePlugin::new().query_bridges().await?;
        Ok(json!({"bridges":state.bridges}))
    }

    async fn rovs(&self, ctx: &BackendContext<'_>, method: &str, args: &Value, admit: FinalAdmission) -> Result<BackendOutcome> {
        let native = self.ovsdb.admitted(admit);
        // Connect before admitting a mutation so an absent OVSDB is a known
        // pre-effect failure, not a fictitious successful availability flag.
        native.get_schema().await?;
        let result = match method {
            "create_bridge" => {
                let input: rovs::CreateBridgeInput = decode(args)?;
                native.create_bridge(&input.bridge_name).await?;
                native.set_bridge_property(&input.bridge_name, "datapath_type", &input.datapath_type).await?;
                json!({"success":true})
            }
            "delete_bridge" => {
                let input: rovs::DeleteBridgeInput = decode(args)?;
                native.delete_bridge(&input.bridge_name).await?;
                json!({"success":true})
            }
            "add_port" => {
                let input: rovs::AddPortInput = decode(args)?;
                ensure!(native.bridge_exists(&input.bridge_name).await?, "OVS bridge does not exist");
                native.add_port_with_type(&input.bridge_name, &input.port_name, Some(&input.interface_type)).await?;
                json!({"success":true})
            }
            "ensure_internal_port" => {
                let input: rovs::EnsureInternalPortInput = decode(args)?;
                let created = native.ensure_internal_port(&input.bridge_name, &input.port_name).await?;
                json!({"bridge_name":input.bridge_name,"port_name":input.port_name,"interface_type":"internal","created":created})
            }
            "remove_port" => {
                let input: rovs::RemovePortInput = decode(args)?;
                let bridge = if input.bridge_name.is_empty() {
                    let topology = ovs::OvsBridgePlugin::new().query_bridges().await?;
                    let matches: Vec<_> = topology.bridges.into_iter()
                        .filter(|bridge| bridge.ports.iter().any(|port| port.name == input.port_name)).collect();
                    ensure!(matches.len() == 1, "port must have exactly one owning bridge");
                    matches.into_iter().next().context("OVS port has no bridge")?.name
                } else { input.bridge_name };
                native.delete_port(&bridge, &input.port_name).await?;
                json!({"success":true})
            }
            "list_bridges" => json!({"success":true,"bridges":native.list_bridges().await?}),
            "list_ports" => {
                let input: rovs::ListPortsInput = decode(args)?;
                json!({"success":true,"ports":native.list_bridge_ports(&input.bridge_name).await?})
            }
            "list_dbs" => json!({"success":true,"databases":native.list_dbs().await?}),
            _ => unreachable!(),
        };
        let topology = self.topology().await?;
        let mut outcome = outcome(result, BackendStateUpdate::Merge(json!({"available":true})), StateScope::Public);
        outcome.additional.push(projection(ctx, "ovsdb_bridge", topology, StateScope::Public)?);
        Ok(outcome)
    }

    async fn ovsdb(
        &self, ctx: &BackendContext<'_>, method: &str, args: &Value,
        admit: FinalAdmission,
    ) -> Result<BackendOutcome> {
        if matches!(method, "monitor" | "monitor_cond" | "cancel") {
            return self.monitor(ctx, method, args, admit).await;
        }
        let owner = owner_scope(ctx.caller);
        let native = self.ovsdb.admitted(admit);
        native.get_schema().await?;
        let result = match method {
            "list_dbs" => json!({"databases":native.list_dbs().await?}),
            "get_schema" => {
                let input: ovs::GetSchemaInput = decode(args)?;
                require_database(&input.db_name)?;
                json!({"schema":native.get_schema().await?})
            }
            "transact" => {
                let input: ovs::TransactInput = decode(args)?;
                require_database(&input.db_name)?;
                let results = native.transact(input.operations).await?;
                ensure!(results.is_array(), "OVSDB transaction result is not an array");
                json!({"results":results})
            }
            "create_bridge" => {
                let input: ovs::CreateBridgeInput = decode(args)?;
                if let Some(value) = &input.fail_mode {
                    ensure!(matches!(value.as_str(), "secure" | "standalone"), "invalid OVS fail mode");
                }
                let before = self.select("Bridge", json!([["name","==",input.name]]),
                    &["name", "datapath_type", "fail_mode", "stp_enable"]).await?;
                native.create_bridge(&input.name).await?;
                if let Some(value) = input.datapath_type {
                    native.set_bridge_property(&input.name, "datapath_type", &value).await?;
                }
                if let Some(value) = input.fail_mode {
                    native.set_bridge_property(&input.name, "fail_mode", &value).await?;
                }
                if let Some(value) = input.stp_enable {
                    native.set_bridge_stp(&input.name, value).await?;
                }
                let after = self.select("Bridge", json!([["name","==",input.name]]),
                    &["name", "datapath_type", "fail_mode", "stp_enable"]).await?;
                ensure!(after.len() == 1, "created OVS bridge did not read back");
                json!({"bridge":input.name,"changed":before != after})
            }
            "delete_bridge" => {
                let input: ovs::DeleteBridgeInput = decode(args)?;
                let existed = native.bridge_exists(&input.name).await?;
                native.delete_bridge(&input.name).await?;
                ensure!(!native.bridge_exists(&input.name).await?, "deleted OVS bridge remains present");
                json!({"bridge":input.name,"changed":existed})
            }
            "add_port" => {
                let input: ovs::AddPortInput = decode(args)?;
                if let Some(tag) = input.tag {
                    ensure!((0..=4095).contains(&tag), "OVS VLAN tag must be 0..=4095");
                }
                ensure!(native.bridge_exists(&input.bridge).await?, "OVS bridge does not exist");
                native.add_port_with_options(&input.bridge, &input.port, input.interface.as_deref(), input.tag).await?;
                ensure!(native.list_bridge_ports(&input.bridge).await?.contains(&input.port), "OVS port readback is absent");
                json!({"bridge":input.bridge,"port":input.port,"changed":true})
            }
            "remove_port" => {
                let input: ovs::RemovePortInput = decode(args)?;
                let existed = native.list_bridge_ports(&input.bridge).await?.contains(&input.port);
                native.delete_port(&input.bridge, &input.port).await?;
                ensure!(!native.list_bridge_ports(&input.bridge).await?.contains(&input.port), "deleted OVS port remains present");
                json!({"bridge":input.bridge,"port":input.port,"changed":existed})
            }
            "echo" => {
                let input: ovs::EchoInput = decode(args)?;
                let params = native.native_rpc("echo", json!(input.params)).await?;
                ensure!(params.is_array(), "OVSDB echo result is not an array");
                json!({"params":params})
            }
            "lock" | "steal" => {
                let input: ovs::LockInput = decode(args)?;
                let receipt = if method == "lock" { native.lock(&owner, &input.lock_id).await? }
                    else { native.steal(&owner, &input.lock_id).await? };
                json!({"lock_id":receipt.lock_id,"locked":receipt.locked,"queued":receipt.queued})
            }
            "unlock" => {
                let input: ovs::UnlockInput = decode(args)?;
                let receipt = native.unlock(&owner, &input.lock_id).await?;
                json!({"lock_id":receipt.lock_id,"unlocked":receipt.unlocked,
                    "acquired_while_queued":receipt.acquired_while_queued,"stolen_before_unlock":receipt.stolen_before_unlock})
            }
            _ => unreachable!(),
        };
        let topology = self.topology().await?;
        if private_ovs_method(method) {
            let mut outcome = outcome(result, BackendStateUpdate::Merge(topology.clone()), private_scope(ctx.caller));
            outcome.additional.push(projection(ctx, "ovsdb_bridge", topology, StateScope::Public)?);
            Ok(outcome)
        } else {
            Ok(outcome(result, BackendStateUpdate::Merge(topology), StateScope::Public))
        }
    }

    async fn openflow(&self, ctx: &BackendContext<'_>, method: &str, args: &Value, admit: FinalAdmission) -> Result<BackendOutcome> {
        if matches!(method, "add_flow" | "delete_flow" | "modify_flow") {
            let receipt = crate::openflow_methods::dispatch_openflow_flow_method_attributed(
                self.permits().await?, method, args, admit,
            ).await?;
            let patch = crate::openflow_methods::projection_patch(&receipt)?;
            return Ok(outcome(receipt.output, BackendStateUpdate::Merge(patch), StateScope::Public));
        }
        let bridge = args.get("bridge").and_then(Value::as_str).context("bridge is required")?;
        validate_bridge(bridge)?;
        self.ovsdb.get_schema().await?;
        let mut patch = BackendStateUpdate::Unchanged;
        let result = match method {
            "get_datapath_health" => self.datapath_health(bridge).await?,
            "ensure_fallback_normal" => {
                let receipt = self.ensure_fallback(bridge, admit).await?;
                patch = BackendStateUpdate::Merge(crate::openflow_methods::projection_patch(&receipt)?);
                json!({"success":true})
            }
            "set_fail_mode" => {
                let input: op_plugins::state_plugins::openflow::SetFailModeInput = decode(args)?;
                ensure!(matches!(input.mode.as_str(), "secure" | "standalone"), "invalid fail mode");
                ensure!(self.ovsdb.bridge_exists(bridge).await?, "OVS bridge does not exist");
                self.ovsdb.admitted(admit).set_bridge_property(bridge, "fail_mode", &input.mode).await?;
                let row = self.bridge_row(bridge).await?;
                ensure!(ovs_strings(row.get("fail_mode").context("OVS fail_mode absent")?)? == vec![input.mode], "OVS fail mode readback differs");
                json!({"success":true})
            }
            "del_controller" => {
                self.controllers(bridge, None, false, admit).await?;
                json!({"success":true})
            }
            "set_controller" | "attach_controller_safe" => {
                let endpoint = args.get("endpoint").and_then(Value::as_str).context("endpoint is required")?;
                ensure!(endpoint.starts_with("ssl:") && endpoint.len() > 4 && !endpoint.chars().any(char::is_control),
                    "OpenFlow controller endpoint must use ssl: with configured OVS client credentials");
                let receipt = self.ensure_fallback(bridge, admit.clone()).await?;
                patch = BackendStateUpdate::Merge(crate::openflow_methods::projection_patch(&receipt)?);
                self.controllers(bridge, Some(endpoint), method == "attach_controller_safe", admit).await?;
                let health = self.datapath_health(bridge).await?;
                ensure!(health["fallback_normal"] == true && health["disable_in_band"] == false,
                    "OpenFlow controller attachment failed native safety readback");
                if method == "attach_controller_safe" { health } else { json!({"success":true}) }
            }
            _ => unreachable!(),
        };
        let mut outcome = outcome(result, patch, StateScope::Public);
        outcome.additional.push(projection(ctx, "ovsdb_bridge", self.topology().await?, StateScope::Public)?);
        Ok(outcome)
    }

    async fn verify_flow(&self, bridge: &str, flow: &FlowEntry, absent: bool) -> Result<bool> {
        let authority = self.permits().await?;
        let owner = crate::openflow_permit::owner(authority.connection(), crate::openflow_permit::CONTROLLER_SERVICE).await?;
        let proxy = zbus::Proxy::new(authority.connection(), owner.as_str(),
            "/org/opdbus/v1/plugins/openflow", "org.opdbus.v1.plugins.openflow").await?;
        Ok(tokio::time::timeout(Duration::from_secs(15), proxy.call("VerifyFlow", &(bridge, serde_json::to_string(flow)?, absent)))
            .await.context("OpenFlow live verification timed out")??)
    }

    async fn ensure_fallback(&self, bridge: &str, admit: FinalAdmission) -> Result<crate::openflow_methods::OpenFlowMethodReceipt> {
        let flow = fallback_flow();
        let existed = self.verify_flow(bridge, &flow, false).await?;
        if !existed {
            let owner = crate::obfuscation_methods::NativeFlowOwner { authority: self.permits().await?.clone(), admission: admit };
            crate::obfuscation_methods::FlowOwner::exchange(&owner, bridge, &flow, None, false).await?;
        }
        ensure!(self.verify_flow(bridge, &flow, false).await?, "NORMAL fallback not confirmed by native switch readback");
        Ok(crate::openflow_methods::OpenFlowMethodReceipt {
            output: json!({"success":true}), before: existed.then(|| flow.clone()), after: Some(flow), bridge: bridge.into(),
        })
    }

    async fn select(&self, table: &str, predicate: Value, columns: &[&str]) -> Result<Vec<Value>> {
        let value = self.ovsdb.transact(vec![json!({"op":"select","table":table,"where":predicate,"columns":columns})]).await?;
        value.get(0).and_then(|result| result.get("rows")).and_then(Value::as_array)
            .cloned().context("OVSDB select omitted rows")
    }

    async fn bridge_row(&self, bridge: &str) -> Result<Value> {
        let mut rows = self.select("Bridge", json!([["name","==",bridge]]),
            &["name", "controller", "fail_mode", "other_config"]).await?;
        ensure!(rows.len() == 1, "OVS bridge must have exactly one native row");
        Ok(rows.remove(0))
    }

    async fn controllers(&self, bridge: &str, endpoint: Option<&str>, standalone: bool, admit: FinalAdmission) -> Result<()> {
        let before = self.bridge_row(bridge).await?;
        let mut operations = vec![json!({"op":"wait","table":"Bridge","where":[["name","==",bridge]],
            "columns":["controller","fail_mode","other_config"],"until":"==","timeout":0,
            "rows":[{"controller":before["controller"],"fail_mode":before["fail_mode"],"other_config":before["other_config"]}]})];
        let controllers = if let Some(endpoint) = endpoint {
            operations.push(json!({"op":"insert","table":"Controller","uuid-name":"admitted_controller",
                "row":{"target":endpoint,"connection_mode":"in-band"}}));
            json!(["set",[["named-uuid","admitted_controller"]]])
        } else { json!(["set",[]]) };
        let mut row = json!({"controller":controllers});
        if standalone { row["fail_mode"] = json!("standalone"); }
        if endpoint.is_some() {
            let mut config = ovs_map(before.get("other_config").context("OVS other_config absent")?)?;
            config.remove("disable-in-band");
            row["other_config"] = json!(["map", config.into_iter().collect::<Vec<_>>()]);
        }
        operations.push(json!({"op":"update","table":"Bridge","where":[["name","==",bridge]],"row":row}));
        self.ovsdb.admitted(admit).transact(operations).await?;
        let observed = self.bridge_row(bridge).await?;
        let ids = ovs_atoms(&observed["controller"])?;
        if let Some(endpoint) = endpoint {
            ensure!(ids.len() == 1, "OVS controller readback must contain one reference");
            let rows = self.select("Controller", json!([["_uuid","==",ids[0]]]), &["target","connection_mode"]).await?;
            ensure!(rows.len() == 1 && rows[0]["target"] == endpoint
                && ovs_strings(&rows[0]["connection_mode"])? == vec!["in-band".to_string()],
                "OVS controller readback differs from the admitted endpoint");
        } else { ensure!(ids.is_empty(), "OVS controller removal did not read back"); }
        if standalone { ensure!(ovs_strings(&observed["fail_mode"])? == vec!["standalone".to_string()], "OVS standalone readback differs"); }
        Ok(())
    }

    async fn datapath_health(&self, bridge: &str) -> Result<Value> {
        let row = self.bridge_row(bridge).await?;
        let mut targets = Vec::new();
        let mut modes = Vec::new();
        for id in ovs_atoms(&row["controller"])? {
            let rows = self.select("Controller", json!([["_uuid","==",id]]), &["target","connection_mode"]).await?;
            ensure!(rows.len() == 1, "OVS controller reference is missing or ambiguous");
            targets.push(rows[0]["target"].as_str().context("OVS controller target is not text")?.to_owned());
            let values = ovs_strings(&rows[0]["connection_mode"])?;
            modes.push(values.first().cloned().unwrap_or_else(|| "in-band".into()));
        }
        let fail_mode = ovs_strings(&row["fail_mode"])?;
        let other = ovs_map(&row["other_config"])?;
        let fallback = self.verify_flow(bridge, &fallback_flow(), false).await?;
        Ok(json!({"bridge":bridge,"fail_mode":fail_mode.first().map(String::as_str).unwrap_or("standalone"),
            "controllers":targets,"fallback_normal":fallback,"fallback_priority":0,
            "fallback_cookie":format!("{:#x}", op_network::datapath_safe::FALLBACK_COOKIE),
            "connection_mode":if modes.is_empty() {"n/a".into()} else {modes.join(",")},
            "disable_in_band":other.get("disable-in-band").is_some_and(|value| value == "true")}))
    }

    async fn monitor(&self, ctx: &BackendContext<'_>, method: &str, args: &Value,
        admit: FinalAdmission) -> Result<BackendOutcome> {
        let owner = owner_scope(ctx.caller);
        let native = self.ovsdb.admitted(admit);
        let topology = self.topology().await?;
        let result;
        if method == "cancel" {
            let input: ovs::CancelInput = decode(args)?;
            let key = monitor_key(&owner, &input.id);
            ensure!(self.monitors.lock().await.contains_key(&key), "OVS monitor is not owned by this caller");
            native.cancel_monitor(&owner, &input.id).await?;
            if let Some(entry) = self.monitors.lock().await.remove(&key) {
                entry.task.abort();
                let _ = entry.task.await;
            }
            result = json!({"monitor_id":input.id,"canceled":true});
        } else {
            let (database, id, requests) = if method == "monitor" {
                let input: ovs::MonitorInput = decode(args)?;
                (input.db_name, input.monitor_id, serde_json::to_value(input.monitor_requests)?)
            } else {
                let input: ovs::MonitorCondInput = decode(args)?;
                (input.db_name, input.monitor_id, serde_json::to_value(input.monitor_cond_requests)?)
            };
            require_database(&database)?;
            let key = monitor_key(&owner, &id);
            {
                let mut monitors = self.monitors.lock().await;
                if monitors.get(&key).is_some_and(|entry| !entry.state.active && entry.task.is_finished()) {
                    monitors.remove(&key);
                }
            }
            ensure!(!self.monitors.lock().await.contains_key(&key), "OVS monitor ID is already retained for this caller");
            let started = native.start_protocol_monitor(&owner, &database, &id, method, requests).await?;
            let state = monitor_state(&key, &started.initial, true, None);
            result = json!({"monitor_id":state.monitor_id,"method":state.method,
                "sequence":state.sequence,"tables":state.tables});
            let (ready, after_receipt) = oneshot::channel();
            let runtime = ctx.runtime.clone();
            let caller = ctx.caller.for_background();
            let task_key = key.clone();
            let task_owner = owner.clone();
            let mut receiver = started.receiver;
            let task = tokio::spawn(async move {
                if after_receipt.await.is_err() {
                    let network = runtime.network();
                    let _operation = network.operations.lock().await;
                    network.monitors.lock().await.remove(&task_key);
                    if let Err(error) = network.ovsdb.cancel_monitor(&task_owner, &id).await {
                        tracing::warn!(%error, "uncommitted OVS monitor cancellation is unconfirmed");
                    }
                    return;
                }
                loop {
                    let observation = receiver.borrow_and_update().clone();
                    let network = runtime.network();
                    let _operation = network.operations.lock().await;
                    let terminal = matches!(&observation, ProtocolObservation::Unavailable(_));
                    {
                        let mut monitors = network.monitors.lock().await;
                        let Some(entry) = monitors.get_mut(&task_key) else { return; };
                        match observation {
                            ProtocolObservation::Snapshot(snapshot) => entry.state = monitor_state(&task_key, &snapshot, true, None),
                            ProtocolObservation::Unavailable(reason) => {
                                entry.state.active = false;
                                entry.state.error = Some(reason.to_string());
                            }
                        }
                    }
                    let update = network.monitor_patch(&task_owner).await;
                    if let Err(error) = runtime.publish_backend_state(&caller, "ovsdb_bridge",
                        BackendStateUpdate::Merge(update), private_scope(&caller)).await {
                        tracing::warn!(%error, "OVS monitor state publication failed");
                        if let Some(entry) = network.monitors.lock().await.get_mut(&task_key) {
                            entry.state.active = false;
                            entry.state.error = Some("observer publication authority unavailable".into());
                        }
                        if let Err(error) = network.ovsdb.cancel_monitor(&task_owner, &id).await {
                            tracing::warn!(%error, "OVS monitor cancellation after publication failure is unconfirmed");
                        }
                        return;
                    }
                    drop(_operation);
                    if terminal || receiver.changed().await.is_err() { return; }
                }
            });
            self.monitors.lock().await.insert(key, MonitorEntry { owner: owner.clone(), state, task });
            self.pending_observers.lock()
                .map_err(|_| anyhow::anyhow!("OVS monitor commit barrier poisoned"))?
                .insert(ctx.operation_id, ready);
        }
        let mut state = topology.clone();
        state["monitors"] = self.monitor_patch(&owner).await["monitors"].take();
        let mut outcome = outcome(result, BackendStateUpdate::Merge(state), private_scope(ctx.caller));
        outcome.additional.push(projection(ctx, "ovsdb_bridge", topology, StateScope::Public)?);
        Ok(outcome)
    }

    async fn monitor_patch(&self, owner: &str) -> Value {
        let monitors = self.monitors.lock().await;
        json!({"monitors":monitors.values().filter(|entry| entry.owner == owner)
            .map(|entry| &entry.state).collect::<Vec<_>>()})
    }

    /// Called by the parent only after the terminal and every projection commit.
    pub(crate) async fn after_commit(
        &self, ctx: &BackendContext<'_>, plugin: &str, _method: &str, _result: &Value,
    ) -> std::result::Result<(), RuntimeError> {
        if let Some(ready) = self.pending_observers.lock()
            .map_err(|_| RuntimeError::unknown(anyhow::anyhow!("OVS monitor commit barrier poisoned")))?
            .remove(&ctx.operation_id) {
            ready.send(()).map_err(|_| RuntimeError::unknown(anyhow::anyhow!("committed OVS monitor observer exited")))?;
        }
        if plugin == "proxy_server" {
            *self.proxy_caller.lock().await = Some(ctx.caller.for_background());
            self.proxy.changed.notify_one();
        }
        Ok(())
    }

    /// Parent owns this event-driven future and its shutdown; it performs no polling.
    pub(crate) async fn observe(runtime: Runtime) -> Result<()> {
        loop {
            runtime.network().proxy.changed.notified().await;
            let network = runtime.network();
            let _operation = network.operations.lock().await;
            let Some(caller) = network.proxy_caller.lock().await.clone() else { continue; };
            let state = serde_json::to_value(network.proxy.snapshot().await?)?;
            if let Err(error) = runtime.publish_backend_state(&caller, "proxy_server",
                BackendStateUpdate::Replace(state), StateScope::Public).await {
                tracing::warn!(%error, "proxy observation publication failed; awaiting a newly admitted owner");
                network.proxy_caller.lock().await.take();
            }
        }
    }

    pub(crate) async fn shutdown(&self) {
        let _operation = self.operations.lock().await;
        let entries = std::mem::take(&mut *self.monitors.lock().await);
        for (_, entry) in entries {
            entry.task.abort();
            let _ = entry.task.await;
            if let Err(error) = self.ovsdb.cancel_monitor(&entry.owner, &entry.state.monitor_id).await {
                tracing::warn!(%error, "OVS monitor shutdown cancellation is unconfirmed");
            }
        }
        self.ovsdb.close_sessions().await;
        self.proxy_caller.lock().await.take();
        self.proxy.shutdown().await;
    }
}

fn outcome(result: Value, state: BackendStateUpdate, scope: StateScope) -> BackendOutcome {
    BackendOutcome { result, state, scope, additional: Vec::new(), guard: None }
}
fn projection(ctx: &BackendContext<'_>, plugin: &str, state: Value, scope: StateScope) -> Result<BackendProjection> {
    let entry = ctx.runtime.catalog().plugin(plugin).context("observed plugin is absent from sealed catalog")?;
    Ok(BackendProjection { plugin_id: plugin.into(), object_path: op_plugins::canonical::plugin_path(plugin),
        schema_hash: entry.schema_hash.clone(), scope, state: BackendStateUpdate::Merge(state) })
}
fn private_scope(caller: &VerifiedCaller) -> StateScope {
    StateScope::Private { principal_id: caller.principal_id.clone(), session_id: caller.session_id.clone() }
}
fn private_ovs_method(method: &str) -> bool {
    matches!(method, "transact" | "lock" | "steal" | "unlock" | "echo" | "monitor" | "monitor_cond" | "cancel")
}
fn owner_scope(caller: &VerifiedCaller) -> String {
    format!("{}\0{}\0{}", caller.principal_id, caller.session_id, caller.session_genesis)
}
fn monitor_key(owner: &str, id: &str) -> String {
    let mut hash = blake3::Hasher::new();
    hash.update(owner.as_bytes()); hash.update(&[0]); hash.update(id.as_bytes());
    hash.finalize().to_hex()[..32].into()
}
fn monitor_state(key: &str, snapshot: &op_network::rovs_proxy::monitor::ProtocolSnapshot,
    active: bool, error: Option<String>) -> OvsdbMonitorState {
    OvsdbMonitorState { projection_key: key.into(), monitor_id: snapshot.monitor_id.clone(),
        method: snapshot.method.clone(), sequence: snapshot.sequence, active, error,
        tables: ovs::materialize_monitor_tables(&snapshot.tables) }
}
fn decode<T: DeserializeOwned>(value: &Value) -> Result<T> { Ok(T::deserialize(value)?) }
fn require_database(database: &str) -> Result<()> {
    ensure!(database == "Open_vSwitch", "unsupported OVSDB database"); Ok(())
}
fn validate_bridge(bridge: &str) -> Result<()> {
    ensure!(!bridge.is_empty() && bridge.len() <= 15
        && bridge.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.')),
        "invalid OpenFlow bridge name"); Ok(())
}
fn fallback_flow() -> FlowEntry {
    FlowEntry { table: 0, priority: 0, cookie: Some(op_network::datapath_safe::FALLBACK_COOKIE),
        match_fields: HashMap::new(), actions: vec![FlowAction::Normal], idle_timeout: 0, hard_timeout: 0 }
}
fn ovs_atoms(value: &Value) -> Result<Vec<Value>> {
    if let Some(array) = value.as_array() {
        if array.first().and_then(Value::as_str) == Some("set") {
            ensure!(array.len() == 2, "invalid OVSDB set");
            return array[1].as_array().cloned().context("invalid OVSDB set members");
        }
    }
    Ok(vec![value.clone()])
}
fn ovs_strings(value: &Value) -> Result<Vec<String>> {
    ovs_atoms(value)?.into_iter().map(|value| value.as_str().map(str::to_owned).context("OVSDB value is not a string")).collect()
}
fn ovs_map(value: &Value) -> Result<BTreeMap<String, String>> {
    let encoded = value.as_array().context("OVSDB map is not an array")?;
    ensure!(encoded.len() == 2 && encoded[0] == "map", "invalid OVSDB map");
    let mut result = BTreeMap::new();
    for pair in encoded[1].as_array().context("invalid OVSDB map pairs")? {
        let pair = pair.as_array().context("invalid OVSDB map entry")?;
        ensure!(pair.len() == 2, "invalid OVSDB map entry length");
        let key = pair[0].as_str().context("OVSDB map key is not text")?;
        let value = pair[1].as_str().context("OVSDB map value is not text")?;
        ensure!(result.insert(key.into(), value.into()).is_none(), "duplicate OVSDB map key");
    }
    Ok(result)
}
