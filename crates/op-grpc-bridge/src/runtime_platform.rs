//! Admitted native platform operations. The runtime holds its execution gate
//! through native dispatch, authoritative observation and the core commit.
//! These adapters never mint receipts, identities, grants or listener owners.
use crate::core::{BackendStateUpdate, StateScope};
use crate::runtime::{BackendContext, BackendOutcome, RuntimeError};
use anyhow::{Context, Result, ensure};
use op_plugins::state_plugins as native;
use serde_json::{Map, Value, json};

/// Only sealed method keys with a concrete native execution path belong here.
/// Incus creation, retired MCP and listener ownership are integrated separately.
pub(crate) fn supports(plugin: &str, method: &str) -> bool {
    match plugin {
        "adc" => method == "get_status",
        "agent_config" => matches!(method, "get_config" | "update_config" | "list_tools" | "register_tool" | "reset_config"),
        "btrfs" => matches!(method, "create_subvolume" | "delete_subvolume" | "snapshot" | "list_subvolumes" | "scrub_start" | "scrub_status" | "balance_start" | "balance_status" | "filesystem_df" | "filesystem_usage" | "device_add" | "device_remove"),
        "config" => matches!(method, "list_keys" | "get_value" | "set_value" | "delete_key" | "export_config"),
        "dnsresolver" => matches!(method, "resolve_hostname" | "resolve_address" | "resolve_record" | "resolve_service" | "get_link" | "set_link_dns" | "set_link_dns_ex" | "set_link_domains" | "set_link_default_route" | "set_link_llmnr" | "set_link_multicast_dns" | "set_link_dns_over_tls" | "set_link_dnssec" | "set_link_dnssec_nta" | "revert_link" | "register_service" | "unregister_service" | "reset_statistics" | "flush_caches" | "reset_server_features"),
        "embedding_model" => matches!(method, "get_config" | "set_model" | "update_config" | "embed" | "list_models"),
        "emqx" => matches!(method, "get_status" | "list_listeners" | "list_hooks" | "get_hook_status" | "configure_hooks" | "start" | "stop" | "restart" | "publish_wake" | "kick_client" | "publish_toolsets"),
        "endpoint" => matches!(method, "list_endpoints" | "get_endpoint" | "add_endpoint" | "remove_endpoint"),
        "fail2ban" => matches!(method, "ban_ip" | "unban_ip" | "get_jail_status"),
        "gcloud_adc" => method == "check_auth_status",
        "hardware" => matches!(method, "list_devices" | "get_device" | "get_stats" | "scan_hardware" | "refresh_inventory"),
        "incus" => matches!(method, "modify_instance" | "delete_instance" | "start_instance" | "stop_instance" | "restart_instance" | "freeze_instance" | "unfreeze_instance" | "snapshot_instance" | "exec_instance" | "add_device" | "update_device" | "remove_device"),
        "keypair" => method == "list_keypairs",
        "keyring" => matches!(method, "list_collections" | "get_default_collection"),
        "login1" => matches!(method, "list_sessions" | "list_users" | "list_seats" | "get_session" | "get_user" | "get_seat" | "create_session" | "release_session" | "terminate_session" | "lock_session" | "unlock_session" | "kill_session" | "terminate_user" | "kill_user" | "set_user_linger" | "terminate_seat" | "attach_device" | "power_off" | "reboot" | "suspend" | "hibernate" | "flush_devices" | "sleep"),
        "mail_server" => matches!(method, "add_domain" | "remove_domain" | "add_mailbox" | "remove_mailbox" | "add_alias" | "remove_alias" | "set_quota" | "get_queue" | "flush_queue"),
        "net" => matches!(method, "apply_interface" | "delete_ovs_bridge"),
        "oci" => matches!(method, "pull_image" | "run_container" | "create" | "start" | "kill" | "delete" | "state"),
        "packagekit" => matches!(method, "resolve" | "install_packages" | "remove_packages" | "update_packages" | "get_updates" | "search_names"),
        "pcidecl" => matches!(method, "enumerate_devices" | "get_device_info"),
        "privacy_routes" => matches!(method, "list_routes" | "delete_route"),
        "procfs" => matches!(method, "list_processes" | "get_process_info" | "get_meminfo" | "get_cpuinfo" | "get_loadavg" | "get_uptime" | "get_net_dev"),
        "qdrant" => matches!(method, "create_collection" | "delete_collection" | "list_collections" | "upsert_points" | "search_points" | "delete_points" | "scroll_points" | "create_snapshot" | "list_snapshots" | "create_payload_index"),
        "rtnetlink" => matches!(method, "set_link_state" | "add_ipv4_address" | "del_ipv4_address" | "set_mac_address" | "set_default_route" | "add_link" | "del_link" | "add_route" | "del_route" | "add_rule"),
        "service" => matches!(method, "init" | "run" | "shutdown"),
        "software" => matches!(method, "get_info" | "install" | "uninstall" | "update"),
        "users" => matches!(method, "create_user" | "delete_user" | "modify_user" | "list_users"),
        _ => false,
    }
}

fn private_scope(ctx: &BackendContext<'_>) -> StateScope {
    StateScope::Private {
        principal_id: ctx.caller.principal_id.clone(),
        session_id: ctx.caller.session_id.clone(),
    }
}

fn operation_error(ctx: &BackendContext<'_>, error: anyhow::Error) -> RuntimeError {
    use native::runtime_documents::{RuntimeDocumentRejected, RuntimeDocumentUncertain};
    if error.downcast_ref::<RuntimeDocumentUncertain>().is_some() {
        RuntimeError::unknown(error)
    } else if error.downcast_ref::<RuntimeDocumentRejected>().is_some()
        || matches!(ctx.declaration.side_effect, op_state_store::SideEffect::Read)
    {
        RuntimeError::backend(error)
    } else {
        // Most native owners expose anyhow chains, not a proof of rejection
        // before submission. Never infer that a timed-out mutation did nothing.
        RuntimeError::unknown(error)
    }
}

fn committed_root(ctx: &BackendContext<'_>, plugin: &str, scope: &StateScope) -> Result<Map<String, Value>> {
    let path = op_plugins::canonical::plugin_path(plugin);
    match ctx.runtime.core().committed_snapshot(scope, plugin, &path)? {
        Some(snapshot) => snapshot.state.as_object().cloned().context("committed platform state is not an object"),
        None => Ok(Map::new()),
    }
}

/// Replace observed root fields, not the entire existing plugin. This also
/// preserves deletion semantics for authoritative maps (config/package keys):
/// recursively merging such maps would resurrect deleted entries.
fn replace_fields(ctx: &BackendContext<'_>, plugin: &str, scope: &StateScope, fields: Value) -> Result<BackendStateUpdate> {
    let mut root = committed_root(ctx, plugin, scope)?;
    let fields = fields.as_object().context("native state observation is not an object")?;
    for (key, value) in fields {
        root.insert(key.clone(), value.clone());
    }
    Ok(BackendStateUpdate::Replace(Value::Object(root)))
}

fn merge_object(target: &mut Map<String, Value>, patch: Map<String, Value>) {
    for (key, value) in patch {
        if let (Some(Value::Object(old)), Value::Object(new)) = (target.get_mut(&key), &value) {
            merge_object(old, new.clone());
        } else {
            target.insert(key, value);
        }
    }
}

/// Update only the native child represented by a receipt/readback, retaining
/// every unrelated child and every parent/child property it did not observe.
fn named_child(ctx: &BackendContext<'_>, plugin: &str, scope: &StateScope, field: &str, key: &str, id: &str, observed: Option<Value>) -> Result<BackendStateUpdate> {
    let mut root = committed_root(ctx, plugin, scope)?;
    let children = root.entry(field.to_owned()).or_insert_with(|| json!([]))
        .as_array_mut().context("committed native child inventory is not an array")?;
    let mut found = None;
    for (index, child) in children.iter().enumerate() {
        let native_id = child.get(key).and_then(Value::as_str).context("committed native child has no identity")?;
        if native_id == id {
            ensure!(found.is_none(), "duplicate native child identity");
            found = Some(index);
        }
    }
    match (found, observed) {
        (Some(index), Some(Value::Object(patch))) => merge_object(children[index].as_object_mut().context("native child is not an object")?, patch),
        (None, Some(Value::Object(patch))) => children.push(Value::Object(patch)),
        (Some(index), None) => { children.remove(index); }
        (None, None) => {}
        (_, Some(_)) => anyhow::bail!("native child observation is not an object"),
    }
    Ok(BackendStateUpdate::Replace(Value::Object(root)))
}

pub(crate) async fn dispatch(ctx: &BackendContext<'_>, plugin: &str, method: &str, args: &Value) -> Result<BackendOutcome, RuntimeError> {
    if !supports(plugin, method) {
        return Err(RuntimeError::unsupported(format!("no compiled platform route for {plugin}.{method}")));
    }
    // The parent already revalidates after its gate; retain this immediate
    // execution-boundary check for direct adapter callers as well.
    ctx.caller.authority.revalidate().await.map_err(|error| RuntimeError::denied(error.to_string()))?;
    let scope = private_scope(ctx);
    let mut state = BackendStateUpdate::Unchanged;
    let result = match plugin {
        "config" | "endpoint" | "agent_config" => {
            let store = ctx.runtime.documents();
            let result = match plugin {
                "config" => native::config::dispatch_config_with_store(store.clone(), method, args).await,
                "endpoint" => native::endpoint::dispatch_endpoint_with_store(&store, method, args).await,
                _ => native::agent_config::dispatch_agent_config_with_store(store.clone(), method, args).await,
            }.map_err(|error| operation_error(ctx, error))?;
            if let Some(row) = store.read_durable(plugin).await.map_err(|error| operation_error(ctx, error))? {
                state = replace_fields(ctx, plugin, &scope, row.document).map_err(|error| operation_error(ctx, error))?;
            } else if matches!(ctx.declaration.side_effect, op_state_store::SideEffect::Mutation) {
                return Err(RuntimeError::unknown(anyhow::anyhow!("native document commit returned without durable state")));
            }
            result
        }
        "hardware" => {
            let method = method.to_owned();
            let args = args.clone();
            let observation = tokio::task::spawn_blocking(move || native::hardware::HardwarePlugin::dispatch_hardware_method_observed(&method, &args))
                .await.map_err(|error| RuntimeError::backend(error.into()))?.map_err(RuntimeError::backend)?;
            state = replace_fields(ctx, plugin, &scope, serde_json::to_value(observation.state).map_err(|error| RuntimeError::backend(error.into()))?).map_err(RuntimeError::backend)?;
            observation.output
        }
        "procfs" => {
            let method = method.to_owned();
            let args = args.clone();
            let observation = tokio::task::spawn_blocking(move || native::procfs::dispatch_procfs_method_observed(&method, &args))
                .await.map_err(|error| RuntimeError::backend(error.into()))?.map_err(RuntimeError::backend)?;
            state = replace_fields(ctx, plugin, &scope, serde_json::to_value(observation.state).map_err(|error| RuntimeError::backend(error.into()))?).map_err(RuntimeError::backend)?;
            observation.output
        }
        "incus" => {
            guard_unmanaged_instance(ctx, args, if matches!(method, "add_device" | "update_device" | "remove_device") { "instance_name" } else { "name" }).await?;
            let observation = match native::incus::dispatch_incus_method_observed(method, args).await {
                Ok(observation) => observation,
                Err(error) => {
                    let observed = error.downcast_ref::<native::incus::IncusMethodObservationError>()
                        .map(|failure| serde_json::to_value(&failure.state))
                        .transpose().map_err(|error| RuntimeError::unknown(error.into()))?;
                    let update = observed.map(|value| replace_fields(ctx, plugin, &scope, value))
                        .transpose().map_err(RuntimeError::unknown)?;
                    let failure = operation_error(ctx, error);
                    return Err(match update {
                        Some(update) => failure.with_state(update, scope),
                        None => failure,
                    });
                }
            };
            if let Some(observed) = observation.state {
                state = replace_fields(ctx, plugin, &scope, serde_json::to_value(observed).map_err(|error| RuntimeError::unknown(error.into()))?).map_err(RuntimeError::unknown)?;
            }
            observation.result
        }
        "mail_server" => {
            let receipt = native::mail_server::methods::dispatch_mail_server_method(method, args).await.map_err(|error| operation_error(ctx, error))?;
            state = replace_fields(ctx, plugin, &scope, json!({"administration": receipt.administration})).map_err(|error| operation_error(ctx, error))?;
            receipt.output
        }
        "embedding_model" => {
            let (result, observed) = native::embedding_model::dispatch_embedding_model_method_observed(ctx.runtime.documents().clone(), method, args).await.map_err(|error| operation_error(ctx, error))?;
            state = replace_fields(ctx, plugin, &scope, serde_json::to_value(observed).map_err(|error| operation_error(ctx, error.into()))?).map_err(|error| operation_error(ctx, error))?;
            result
        }
        "qdrant" => {
            let result = native::qdrant::dispatch_qdrant_method(method, args).await.map_err(|error| operation_error(ctx, error))?;
            let observed = native::qdrant::current_state().await.map_err(|error| operation_error(ctx, error))?;
            let mut fields = serde_json::to_value(observed).map_err(|error| operation_error(ctx, error.into()))?;
            // The native inventory does not sample these optional surfaces.
            if let Some(fields) = fields.as_object_mut() {
                fields.remove("telemetry");
                fields.remove("cluster_status");
            }
            state = replace_fields(ctx, plugin, &scope, fields).map_err(|error| operation_error(ctx, error))?;
            result
        }
        "privacy_routes" => {
            let result = native::privacy_routes::dispatch_privacy_routes_method(method, args).await.map_err(|error| operation_error(ctx, error))?;
            // Read the committed owner again; a delete response is not itself a
            // persistence/readback proof.
            let observed = native::privacy_routes::dispatch_privacy_routes_method("list_routes", &json!({})).await.map_err(|error| operation_error(ctx, error))?;
            state = replace_fields(ctx, plugin, &scope, observed).map_err(|error| operation_error(ctx, error))?;
            result
        }
        "dnsresolver" => {
            let result = native::dnsresolver::dispatch_dnsresolver_method(method, args).await.map_err(|error| operation_error(ctx, error))?;
            if matches!(method, "set_link_dns" | "set_link_dns_ex" | "set_link_domains" | "revert_link") {
                let observed = native::dnsresolver::observe_dnsresolver_manager().await.map_err(RuntimeError::unknown)?;
                let item = observed.into_iter().next().context("resolve1 omitted manager observation").map_err(RuntimeError::unknown)?;
                state = named_child(ctx, plugin, &scope, "items", "id", "manager", Some(serde_json::to_value(item).map_err(|error| RuntimeError::unknown(error.into()))?)).map_err(RuntimeError::unknown)?;
            }
            result
        }
        "fail2ban" => {
            let socket = std::env::var_os("OP_FAIL2BAN_SOCKET").map(std::path::PathBuf::from).unwrap_or_else(|| "/run/fail2ban/fail2ban.sock".into());
            let owner = native::fail2ban_methods::Fail2banMethods::new(socket, std::time::Duration::from_secs(15)).map_err(RuntimeError::backend)?;
            let result = owner.dispatch(method, args).await.map_err(|error| operation_error(ctx, error))?;
            let jail = args.get("jail").and_then(Value::as_str).context("jail is required").map_err(RuntimeError::backend)?;
            let observed = if method == "get_jail_status" { result.clone() } else {
                owner.dispatch("get_jail_status", &json!({"jail": jail})).await.map_err(RuntimeError::unknown)?
            };
            let observed: native::fail2ban::JailStatusOutput = serde_json::from_value(observed).map_err(|error| operation_error(ctx, error.into()))?;
            ensure_jail_receipt(method, args, &observed).map_err(|error| operation_error(ctx, error))?;
            state = named_child(ctx, plugin, &scope, "jails", "jail", jail, Some(serde_json::to_value(observed).map_err(|error| operation_error(ctx, error.into()))?)).map_err(|error| operation_error(ctx, error))?;
            result
        }
        "net" => {
            let result = native::net::dispatch_net_method(method, args).await.map_err(|error| operation_error(ctx, error))?;
            state = net_state(ctx, &scope, method, &result).map_err(RuntimeError::unknown)?;
            result
        }
        "rtnetlink" => {
            let result = native::rtnetlink::dispatch_rtnetlink_method(method, args).await.map_err(|error| operation_error(ctx, error))?;
            state = rtnetlink_state(ctx, &scope).await.map_err(RuntimeError::unknown)?;
            result
        }
        "service" => {
            let result = native::service::dispatch_service_method(method, args).await.map_err(|error| operation_error(ctx, error))?;
            state = service_state(ctx, &scope, method, &result).map_err(RuntimeError::unknown)?;
            result
        }
        "software" | "packagekit" => {
            let result = if plugin == "software" {
                native::software::dispatch_software_method(method, args).await
            } else {
                native::packagekit::dispatch_packagekit_method(method, args).await
            }.map_err(|error| operation_error(ctx, error))?;
            if matches!(ctx.declaration.side_effect, op_state_store::SideEffect::Mutation) {
                let inventory = native::packagekit::installed_package_inventory().await.map_err(RuntimeError::unknown)?;
                state = package_state(ctx, plugin, &scope, inventory).map_err(RuntimeError::unknown)?;
            }
            result
        }
        "login1" => {
            let result = native::login1::dispatch_login1_method(method, args).await.map_err(|error| operation_error(ctx, error))?;
            if matches!(method, "list_sessions" | "create_session" | "release_session" | "terminate_session" | "kill_session" | "terminate_user" | "kill_user" | "terminate_seat") {
                let observed = if method == "list_sessions" { result.clone() } else {
                    native::login1::dispatch_login1_method("list_sessions", &json!({})).await.map_err(RuntimeError::unknown)?
                };
                let observed: native::login1::ListSessionsOutput = serde_json::from_value(observed).map_err(|error| operation_error(ctx, error.into()))?;
                state = replace_fields(ctx, plugin, &scope, json!({"sessions": observed.sessions})).map_err(|error| operation_error(ctx, error))?;
            }
            result
        }
        "adc" | "gcloud_adc" => {
            let result = if plugin == "adc" { native::adc::dispatch_adc_method(method, args).await } else { native::gcloud_adc::dispatch_gcloud_adc_method(method, args).await }.map_err(RuntimeError::backend)?;
            let observed: native::gcloud_adc::AdcAuthStatus = serde_json::from_value(result.clone()).map_err(|error| RuntimeError::backend(error.into()))?;
            let fields = if plugin == "adc" { json!({"configured": observed.configured, "authenticated": observed.authenticated, "validation": observed.validation}) } else { json!({"status": observed}) };
            state = replace_fields(ctx, plugin, &scope, fields).map_err(RuntimeError::backend)?;
            result
        }
        "keypair" => {
            let result = native::keypair::dispatch_keypair_method(method, args).await.map_err(RuntimeError::backend)?;
            let observed: native::keypair::ListKeypairsOutput = serde_json::from_value(result.clone()).map_err(|error| RuntimeError::backend(error.into()))?;
            let keys: Vec<_> = observed.keypairs.into_iter().map(|key| native::keypair::Keypair { name: key.name, algorithm: key.algorithm, public_key: key.public_key, present: key.present }).collect();
            state = replace_fields(ctx, plugin, &scope, json!({"keypairs": keys})).map_err(RuntimeError::backend)?;
            result
        }
        "keyring" => {
            let (result, observed) = native::keyring::dispatch_keyring_method_observed(method, args).await.map_err(RuntimeError::backend)?;
            state = replace_fields(ctx, plugin, &scope, serde_json::to_value(observed).map_err(|error| RuntimeError::backend(error.into()))?).map_err(RuntimeError::backend)?;
            result
        }
        "emqx" => {
            authorize_emqx(ctx, method, args).await?;
            // Broker liveness/connection counts and wake publications are not
            // fields in EmqxState (its surface is the installed declaration).
            native::emqx::dispatch_emqx_method(method, args).await.map_err(|error| operation_error(ctx, error))?
        }
        "oci" => {
            if method != "pull_image" { guard_unmanaged_instance(ctx, args, "name").await?; }
            // OciState declares provider/type, not native instances. The native
            // receipt contains its own observed instance/postcondition.
            native::oci::dispatch_oci_method(method, args).await.map_err(|error| operation_error(ctx, error))?
        }
        "btrfs" => native::btrfs_plugin::dispatch_btrfs_method(method, args).await.map_err(|error| operation_error(ctx, error))?,
        "pcidecl" => native::pcidecl::dispatch_pcidecl_method(method, args).await.map_err(RuntimeError::backend)?,
        "users" => native::users::dispatch_users_method(method, args).await.map_err(|error| operation_error(ctx, error))?,
        _ => return Err(RuntimeError::unsupported(format!("no compiled platform route for {plugin}.{method}"))),
    };
    Ok(BackendOutcome { result, state, scope, additional: Vec::new(), guard: None })
}

fn ensure_jail_receipt(method: &str, args: &Value, observed: &native::fail2ban::JailStatusOutput) -> Result<()> {
    ensure!(args.get("jail").and_then(Value::as_str) == Some(observed.jail.as_str()), "Fail2ban readback refers to another jail");
    if matches!(method, "ban_ip" | "unban_ip") {
        let wanted: std::net::IpAddr = args.get("ip").and_then(Value::as_str).context("ip is required")?.parse()?;
        let present = observed.banned_ips.iter().map(|ip| ip.parse::<std::net::IpAddr>()).collect::<std::result::Result<Vec<_>, _>>()?.contains(&wanted);
        ensure!(present == (method == "ban_ip"), "Fail2ban accepted the operation but its ban inventory does not confirm it");
    }
    Ok(())
}

fn net_state(ctx: &BackendContext<'_>, scope: &StateScope, method: &str, result: &Value) -> Result<BackendStateUpdate> {
    if method == "delete_ovs_bridge" {
        let receipt: native::net::BridgeDeleteOutput = serde_json::from_value(result.clone())?;
        ensure!(receipt.absent, "native bridge removal has no absence observation");
        return named_child(ctx, "net", scope, "interfaces", "name", &receipt.name, None);
    }
    let receipt: native::net::InterfaceApplyOutput = serde_json::from_value(result.clone())?;
    let mut fields = json!({
        "name": receipt.name,
        "type": receipt.interface_type,
        "ipv4": {"enabled": !receipt.ipv4_addresses.is_empty(), "address": receipt.ipv4_addresses},
    });
    if receipt.interface_type == native::net::ApplyInterfaceType::OvsBridge {
        fields["ports"] = serde_json::to_value(receipt.attached_ports)?;
    }
    // None means the method did not observe a gateway, not native absence.
    if let Some(gateway) = receipt.default_gateway { fields["ipv4"]["gateway"] = gateway.into(); }
    named_child(ctx, "net", scope, "interfaces", "name", &receipt.name, Some(fields))
}

async fn rtnetlink_state(ctx: &BackendContext<'_>, scope: &StateScope) -> Result<BackendStateUpdate> {
    let interfaces = op_network::rtnetlink::list_interfaces_strict().await?;
    let route = op_network::rtnetlink::get_default_route().await?;
    let mut root = committed_root(ctx, "rtnetlink", scope)?;
    let previous = root.remove("interfaces").unwrap_or_else(|| json!([]));
    let previous = previous.as_array().context("committed interface inventory is not an array")?;
    let mut observed = Vec::with_capacity(interfaces.len());
    for interface in interfaces {
        let mut child = previous.iter().find(|item| item.get("name").and_then(Value::as_str) == Some(interface.name.as_str()))
            .and_then(Value::as_object).cloned().unwrap_or_default();
        let gateway = route.as_ref().filter(|route| route["interface_name"].as_str() == Some(interface.name.as_str())).and_then(|route| route.get("gateway")).cloned();
        child.insert("name".into(), interface.name.into());
        child.insert("state".into(), interface.state.into());
        child.insert("addresses".into(), Value::Array(interface.addresses.into_iter().map(|address| Value::String(format!("{}/{}", address.address, address.prefix_len))).collect()));
        if let Some(mac) = interface.mac_address { child.insert("mac_address".into(), mac.into()); } else { child.remove("mac_address"); }
        if let Some(gateway) = gateway.filter(|gateway| !gateway.is_null()) { child.insert("default_gateway".into(), gateway); } else { child.remove("default_gateway"); }
        observed.push(Value::Object(child));
    }
    root.insert("interfaces".into(), Value::Array(observed));
    Ok(BackendStateUpdate::Replace(root.into()))
}

fn service_state(ctx: &BackendContext<'_>, scope: &StateScope, method: &str, result: &Value) -> Result<BackendStateUpdate> {
    let (name, fields) = if method == "init" {
        let receipt: native::service::ServiceInitOutput = serde_json::from_value(result.clone())?;
        (receipt.name, json!({"enabled": receipt.enabled, "definition_type": receipt.definition_type}))
    } else {
        let receipt: native::service::ServiceOperationOutput = serde_json::from_value(result.clone())?;
        (receipt.name, serde_json::to_value(receipt.status)?)
    };
    let mut root = committed_root(ctx, "service", scope)?;
    let services = root.entry("services").or_insert_with(|| json!({}));
    if services.is_null() { *services = json!({}); }
    let services = services.as_object_mut().context("committed services is not a map")?;
    let service = services.entry(name).or_insert_with(|| json!({})).as_object_mut().context("committed service is not an object")?;
    merge_object(service, fields.as_object().context("native service readback is not an object")?.clone());
    Ok(BackendStateUpdate::Replace(root.into()))
}

fn package_state(ctx: &BackendContext<'_>, plugin: &str, scope: &StateScope, inventory: native::packagekit::PackageKitResult) -> Result<BackendStateUpdate> {
    let mut packages = Vec::new();
    let mut declarations: std::collections::HashMap<String, native::packagekit::PackageState> = std::collections::HashMap::new();
    if plugin == "software" { packages.reserve(inventory.package_details.len()); }
    for package in inventory.package_details {
        let mut parts = package.package_id.split(';');
        let name = parts.next().filter(|part| !part.is_empty()).context("PackageKit package name is absent")?;
        let version = parts.next().filter(|part| !part.is_empty()).context("PackageKit package version is absent")?;
        let architecture = parts.next().filter(|part| !part.is_empty()).context("PackageKit package architecture is absent")?;
        let source = parts.next().filter(|part| !part.is_empty()).context("PackageKit package source is absent")?;
        ensure!(package.info == 1 && source.starts_with("installed") && parts.next().is_none(), "PackageKit returned invalid installed-package inventory");
        if plugin == "packagekit" {
            let declaration = native::packagekit::PackageState { ensure: "installed".into(), provider: None, version: Some(version.to_owned()) };
            if let Some(previous) = declarations.insert(name.to_owned(), declaration) {
                ensure!(previous.version.as_deref() == Some(version), "multiple installed versions cannot be represented in PackageKit declaration state");
            }
        } else {
            packages.push(native::software::PackageInfo { name: name.to_owned(), version: version.to_owned(), architecture: architecture.to_owned(), package_id: package.package_id, status: "installed".into(), manager: "packagekit".into() });
        }
    }
    let fields = if plugin == "software" { json!({"packages": packages}) } else { json!({"version": 1, "packages": declarations}) };
    replace_fields(ctx, plugin, scope, fields)
}

/// Identity storage is never controlled through a generic native lifecycle
/// method. Parent-owned identity routes retain their provenance/mount guards.
async fn guard_unmanaged_instance(ctx: &BackendContext<'_>, args: &Value, field: &str) -> Result<(), RuntimeError> {
    let target = args.get(field).and_then(Value::as_str).context("native instance name is required").map_err(RuntimeError::backend)?.to_owned();
    let store = ctx.runtime.identity_store().clone();
    let protected = tokio::task::spawn_blocking(move || -> Result<bool> {
        let recovery = store.read_identity_recovery_state(0)?;
        for revision in recovery.identities {
            if let Some(record) = revision.record {
                if !record.instance_json.is_empty() {
                    let instance: native::incus::IncusInstance = serde_json::from_str(&record.instance_json)?;
                    if instance.name == target { return Ok(true); }
                }
            }
        }
        if store.native_creation_records()?.iter().any(|record| record.name == target && record.request.owner_kind == "identity") { return Ok(true); }
        if store.container_rename_name_reserved(&target)? { return Ok(true); }
        Ok(false)
    }).await.map_err(|error| RuntimeError::backend(error.into()))?.map_err(RuntimeError::backend)?;
    if protected { return Err(RuntimeError::denied("identity-managed native targets require their identity lifecycle/provenance owner")); }
    ctx.caller.authority.revalidate().await.map_err(|error| RuntimeError::denied(error.to_string()))
}

async fn authorize_emqx(ctx: &BackendContext<'_>, method: &str, args: &Value) -> Result<(), RuntimeError> {
    if method != "publish_wake" { return Ok(()); }
    let input: native::emqx::PublishWakeInput = serde_json::from_value(args.clone()).map_err(|error| RuntimeError::backend(error.into()))?;
    let owner = crate::runtime_mqtt::owner_key_for_claims(ctx.caller.claims()).map_err(RuntimeError::backend)?;
    if input.owner_key != owner { return Err(RuntimeError::denied("wake owner does not match the verified caller")); }
    // Fresh runtime wakes use the durable event cursor. A global event head
    // alone does not prove that this caller has any committed private state.
    let scope = private_scope(ctx);
    let mut private_head = None;
    for plugin in ctx.runtime.catalog().plugins() {
        let path = op_plugins::canonical::plugin_path(&plugin.id);
        if let Some(snapshot) = ctx.runtime.core().committed_snapshot(&scope, &plugin.id, &path).map_err(RuntimeError::backend)? {
            private_head = Some(private_head.map_or(snapshot.event_id, |head: u64| head.max(snapshot.event_id)));
        }
    }
    if private_head.is_none_or(|head| input.seq > head) {
        return Err(RuntimeError::denied("wake sequence has no caller-private committed state head"));
    }
    ctx.caller.authority.revalidate().await.map_err(|error| RuntimeError::denied(error.to_string()))
}
