use schemars::JsonSchema;
// dnsresolver_plugin.rs
use anyhow::{Context, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use simd_json::OwnedValue as Value;

use op_state::{
    ApplyResult, Checkpoint, DiffMetadata, PluginCapabilities, StateAction, StateDiff, StatePlugin,
};
use op_state_store::{Constraint, FieldSchema, FieldType, PluginSchema};

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct DnsState {
    /// DNS plugin state schema.
    #[schemars(extend("x-oscal-subid" = "sch.software.plugin.dnsresolver.schema@v1"))]
    pub version: u32,
    #[schemars(extend("x-oscal-subid" = "obs.software.dnsresolver.state.items@v1"))]
    pub items: Vec<DnsItem>,
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Mode {
    #[schemars(extend("x-oscal-subid" = "obs.software.dnsresolver.mode.enforce@v1"))]
    Enforce,
    #[schemars(extend("x-oscal-subid" = "obs.software.dnsresolver.mode.observe-only@v1"))]
    ObserveOnly,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct DnsItem {
    /// DNS item identifier.
    #[schemars(extend("x-oscal-subid" = "exp.software.dnsresolver.item.id@v1"))]
    pub id: String,
    /// DNS mode.
    #[schemars(extend("x-oscal-subid" = "obs.software.dnsresolver.item.mode@v1"))]
    pub mode: Mode,
    /// DNS server list.
    #[schemars(extend("x-oscal-subid" = "mut.software.dnsresolver.item.servers@v1"))]
    pub servers: Vec<String>,
    /// DNS search domains.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(extend("x-oscal-subid" = "obs.software.dnsresolver.item.search@v1"))]
    pub search: Option<Vec<String>>,
    /// DNS options.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(extend("x-oscal-subid" = "obs.software.dnsresolver.item.options@v1"))]
    pub options: Option<Vec<String>>,
}

pub struct DnsResolverPlugin;

impl Default for DnsResolverPlugin {
    fn default() -> Self {
        Self
    }
}

impl DnsResolverPlugin {
    pub fn new() -> Self {
        Self
    }

    fn normalize(v: &[String]) -> Vec<String> {
        let mut out = v.to_vec();
        out.sort();
        out.dedup();
        out
    }

    fn equal_desired(cur: &DnsItem, want: &DnsItem) -> bool {
        Self::normalize(&cur.servers) == Self::normalize(&want.servers)
            && cur.search.as_ref().map(|v| Self::normalize(v))
                == want.search.as_ref().map(|v| Self::normalize(v))
            && cur.options.as_ref().map(|v| Self::normalize(v))
                == want.options.as_ref().map(|v| Self::normalize(v))
    }
}

#[async_trait]
impl StatePlugin for DnsResolverPlugin {
    fn name(&self) -> &str {
        "dnsresolver"
    }
    fn version(&self) -> &str {
        "2.0.0"
    }

    fn schema(&self) -> Option<PluginSchema> {
        Some(dnsresolver_schema())
    }

    async fn calculate_diff(&self, _current: &Value, desired: &Value) -> Result<StateDiff> {
        let want: DnsState = match simd_json::serde::from_owned_value(desired.clone()) {
            Ok(v) => v,
            Err(_) => DnsState {
                version: 1,
                items: Vec::new(),
            },
        };
        let cur_all = query_manager_state().await?;
        let cur = cur_all.first();
        let mut actions = Vec::new();
        for item in &want.items {
            match cur {
                Some(c) if Self::equal_desired(c, item) => actions.push(StateAction::NoOp {
                    resource: item.id.clone(),
                }),
                Some(_) => actions.push(StateAction::Modify {
                    resource: item.id.clone(),
                    changes: simd_json::serde::to_owned_value(item).unwrap_or(simd_json::json!({})),
                }),
                None => actions.push(StateAction::Create {
                    resource: item.id.clone(),
                    config: simd_json::serde::to_owned_value(item).unwrap_or(simd_json::json!({})),
                }),
            }
        }
        let meta = DiffMetadata {
            timestamp: chrono::Utc::now().timestamp(),
            current_hash: format!(
                "{:x}",
                md5::compute(
                    simd_json::to_string(&simd_json::json!({"items": cur_all})).unwrap_or_default()
                )
            ),
            desired_hash: format!(
                "{:x}",
                md5::compute(simd_json::to_string(&want).unwrap_or_default())
            ),
        };
        Ok(StateDiff {
            plugin: self.name().to_string(),
            actions,
            metadata: meta,
        })
    }

    async fn apply_state(&self, diff: &StateDiff) -> Result<ApplyResult> {
        let mut changes_applied = Vec::new();
        let mut errors = Vec::new();
        for action in &diff.actions {
            match action {
                StateAction::Create { resource, config }
                | StateAction::Modify {
                    resource,
                    changes: config,
                } => {
                    let item: DnsItem = match simd_json::serde::from_owned_value(config.clone()) {
                        Ok(v) => v,
                        Err(_) => {
                            errors.push(format!("{}: invalid payload", resource));
                            continue;
                        }
                    };
                    match item.mode {
                        Mode::ObserveOnly => {
                            changes_applied.push(format!("{}: no action required", resource))
                        }
                        Mode::Enforce => errors.push(format!(
                            "{}: use set_link_dns or set_link_dns_ex; global resolv.conf writes are unsupported",
                            resource
                        )),
                    }
                }
                StateAction::Delete { resource } => {
                    changes_applied.push(format!("{}: delete not supported", resource));
                }
                StateAction::NoOp { resource } => {
                    changes_applied.push(format!("{}: no action required", resource));
                }
            }
        }
        Ok(ApplyResult {
            success: errors.is_empty(),
            changes_applied,
            errors,
            checkpoint: None,
        })
    }

    async fn verify_state(&self, desired: &Value) -> Result<bool> {
        let want: DnsState = match simd_json::serde::from_owned_value(desired.clone()) {
            Ok(v) => v,
            Err(_) => return Ok(true),
        };
        let cur_all = query_manager_state().await?;
        let cur = match cur_all.first() {
            Some(v) => v,
            None => return Ok(false),
        };
        for item in &want.items {
            if !Self::equal_desired(cur, item) {
                return Ok(false);
            }
        }
        Ok(true)
    }

    async fn create_checkpoint(&self) -> Result<Checkpoint> {
        Ok(Checkpoint {
            id: format!("{}-{}", self.name(), chrono::Utc::now().timestamp()),
            plugin: self.name().to_string(),
            timestamp: chrono::Utc::now().timestamp(),
            state_snapshot: simd_json::json!({}),
            backend_checkpoint: None,
        })
    }
    async fn rollback(&self, _checkpoint: &Checkpoint) -> Result<()> {
        Ok(())
    }

    fn capabilities(&self) -> PluginCapabilities {
        PluginCapabilities {
            supports_rollback: false,
            supports_checkpoints: true,
            supports_verification: true,
            atomic_operations: false,
        }
    }
}

const RESOLVE1_DESTINATION: &str = "org.freedesktop.resolve1";
const RESOLVE1_PATH: &str = "/org/freedesktop/resolve1";
const RESOLVE1_MANAGER: &str = "org.freedesktop.resolve1.Manager";

async fn resolve1_manager<'a>(connection: &'a zbus::Connection) -> Result<zbus::Proxy<'a>> {
    zbus::Proxy::new(
        connection,
        RESOLVE1_DESTINATION,
        RESOLVE1_PATH,
        RESOLVE1_MANAGER,
    )
    .await
    .context("create resolve1 Manager proxy")
}

async fn query_manager_state() -> Result<Vec<DnsItem>> {
    let connection = zbus::Connection::system()
        .await
        .context("connect to system D-Bus")?;
    let manager = resolve1_manager(&connection).await?;
    let dns: Vec<(i32, i32, Vec<u8>)> = manager
        .get_property("DNS")
        .await
        .context("read resolve1 DNS property")?;
    let servers: Vec<String> = dns
        .into_iter()
        .filter_map(|(_, family, address)| {
            let ip = match (family, address.as_slice()) {
                (2, [a, b, c, d]) => std::net::Ipv4Addr::new(*a, *b, *c, *d).to_string(),
                (10, bytes) if bytes.len() == 16 => {
                    let mut octets = [0u8; 16];
                    octets.copy_from_slice(bytes);
                    std::net::Ipv6Addr::from(octets).to_string()
                }
                _ => return None,
            };
            Some(ip)
        })
        .collect();
    Ok(if servers.is_empty() {
        Vec::new()
    } else {
        vec![DnsItem {
            id: "manager".into(),
            mode: Mode::ObserveOnly,
            servers,
            search: None,
            options: None,
        }]
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema, zbus::zvariant::Type)]
pub struct DnsAddress {
    pub ifindex: i32,
    pub family: i32,
    pub address: Vec<u8>,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema, zbus::zvariant::Type)]
pub struct DnsName {
    pub ifindex: i32,
    pub name: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema, zbus::zvariant::Type)]
pub struct DnsRecord {
    pub ifindex: i32,
    pub class: u16,
    pub record_type: u16,
    pub data: Vec<u8>,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema, zbus::zvariant::Type)]
pub struct DnsServiceRecord {
    pub priority: u16,
    pub weight: u16,
    pub port: u16,
    pub hostname: String,
    pub addresses: Vec<DnsAddress>,
    pub canonical: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ResolveHostnameOutput {
    pub addresses: Vec<DnsAddress>,
    pub canonical: String,
    pub flags: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ResolveAddressOutput {
    pub names: Vec<DnsName>,
    pub flags: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ResolveRecordOutput {
    pub records: Vec<DnsRecord>,
    pub flags: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ResolveServiceOutput {
    pub srv_data: Vec<DnsServiceRecord>,
    pub txt_data: Vec<Vec<u8>>,
    pub canonical_name: String,
    pub canonical_type: String,
    pub canonical_domain: String,
    pub flags: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct LinkOutput {
    pub path: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct DnsMutationOutput {
    pub disposition: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ServiceRegistrationOutput {
    pub service_path: String,
}

/// Dispatch all schema-declared resolver methods through systemd-resolved's
/// native org.freedesktop.resolve1.Manager API. The `_on` variant keeps the
/// connection injectable for isolated peer-bus tests.
pub async fn dispatch_dnsresolver_method(
    method: &str,
    args: &serde_json::Value,
) -> Result<serde_json::Value> {
    let connection = zbus::Connection::system()
        .await
        .context("connect to system D-Bus")?;
    dispatch_dnsresolver_method_on(&connection, method, args).await
}

pub async fn dispatch_dnsresolver_method_on(
    connection: &zbus::Connection,
    method: &str,
    args: &serde_json::Value,
) -> Result<serde_json::Value> {
    let manager = resolve1_manager(connection).await?;
    macro_rules! input {
        ($ty:ty, $msg:literal) => {
            serde_json::from_value::<$ty>(args.clone()).context($msg)?
        };
    }
    macro_rules! mutation {
        ($call:expr) => {{
            $call.await?;
            Ok(serde_json::to_value(DnsMutationOutput {
                disposition: "accepted".to_string(),
            })?)
        }};
    }
    match method {
        "resolve_hostname" => {
            let i = input!(ResolveHostnameInput, "invalid resolve_hostname arguments");
            let r: (Vec<DnsAddress>, String, u64) = manager
                .call("ResolveHostname", &(i.ifindex, i.name, i.family, i.flags))
                .await?;
            Ok(serde_json::to_value(ResolveHostnameOutput {
                addresses: r.0,
                canonical: r.1,
                flags: r.2,
            })?)
        }
        "resolve_address" => {
            let i = input!(ResolveAddressInput, "invalid resolve_address arguments");
            let r: (Vec<DnsName>, u64) = manager
                .call("ResolveAddress", &(i.ifindex, i.family, i.address, i.flags))
                .await?;
            Ok(serde_json::to_value(ResolveAddressOutput {
                names: r.0,
                flags: r.1,
            })?)
        }
        "resolve_record" => {
            let i = input!(ResolveRecordInput, "invalid resolve_record arguments");
            let r: (Vec<DnsRecord>, u64) = manager
                .call(
                    "ResolveRecord",
                    &(i.ifindex, i.name, i.class, i.record_type, i.flags),
                )
                .await?;
            Ok(serde_json::to_value(ResolveRecordOutput {
                records: r.0,
                flags: r.1,
            })?)
        }
        "resolve_service" => {
            let i = input!(ResolveServiceInput, "invalid resolve_service arguments");
            let r: (
                Vec<DnsServiceRecord>,
                Vec<Vec<u8>>,
                String,
                String,
                String,
                u64,
            ) = manager
                .call(
                    "ResolveService",
                    &(
                        i.ifindex,
                        i.name,
                        i.service_type,
                        i.domain,
                        i.family,
                        i.flags,
                    ),
                )
                .await?;
            Ok(serde_json::to_value(ResolveServiceOutput {
                srv_data: r.0,
                txt_data: r.1,
                canonical_name: r.2,
                canonical_type: r.3,
                canonical_domain: r.4,
                flags: r.5,
            })?)
        }
        "get_link" => {
            let i = input!(GetLinkInput, "invalid get_link arguments");
            let path: zbus::zvariant::OwnedObjectPath =
                manager.call("GetLink", &(i.ifindex,)).await?;
            Ok(serde_json::to_value(LinkOutput {
                path: path.to_string(),
            })?)
        }
        "set_link_dns" => {
            let i = input!(SetLinkDNSInput, "invalid set_link_dns arguments");
            mutation!(manager.call::<_, _, ()>("SetLinkDNS", &(i.ifindex, i.addresses)))
        }

        "set_link_dns_ex" => {
            let i = input!(SetLinkDNSExInput, "invalid set_link_dns_ex arguments");
            mutation!(manager.call::<_, _, ()>("SetLinkDNSEx", &(i.ifindex, i.addresses)))
        }

        "set_link_domains" => {
            let i = input!(SetLinkDomainsInput, "invalid set_link_domains arguments");
            mutation!(manager.call::<_, _, ()>("SetLinkDomains", &(i.ifindex, i.domains)))
        }

        "set_link_default_route" => {
            let i = input!(
                SetLinkDefaultRouteInput,
                "invalid set_link_default_route arguments"
            );
            mutation!(manager.call::<_, _, ()>("SetLinkDefaultRoute", &(i.ifindex, i.enable)))
        }

        "set_link_llmnr" => {
            let i = input!(SetLinkLLMNRInput, "invalid set_link_llmnr arguments");
            mutation!(manager.call::<_, _, ()>("SetLinkLLMNR", &(i.ifindex, i.mode)))
        }

        "set_link_multicast_dns" => {
            let i = input!(
                SetLinkMulticastDNSInput,
                "invalid set_link_multicast_dns arguments"
            );
            mutation!(manager.call::<_, _, ()>("SetLinkMulticastDNS", &(i.ifindex, i.mode)))
        }

        "set_link_dns_over_tls" => {
            let i = input!(
                SetLinkDNSOverTLSInput,
                "invalid set_link_dns_over_tls arguments"
            );
            mutation!(manager.call::<_, _, ()>("SetLinkDNSOverTLS", &(i.ifindex, i.mode)))
        }

        "set_link_dnssec" => {
            let i = input!(SetLinkDNSSECInput, "invalid set_link_dnssec arguments");
            mutation!(manager.call::<_, _, ()>("SetLinkDNSSEC", &(i.ifindex, i.mode)))
        }

        "set_link_dnssec_nta" => {
            let i = input!(
                SetLinkDNSSECNegativeTrustAnchorsInput,
                "invalid set_link_dnssec_nta arguments"
            );
            mutation!(manager
                .call::<_, _, ()>("SetLinkDNSSECNegativeTrustAnchors", &(i.ifindex, i.names)))
        }
        "revert_link" => {
            let i = input!(RevertLinkInput, "invalid revert_link arguments");
            mutation!(manager.call::<_, _, ()>("RevertLink", &(i.ifindex,)))
        }

        "register_service" => {
            let i = input!(RegisterServiceInput, "invalid register_service arguments");
            let path: zbus::zvariant::OwnedObjectPath = manager
                .call(
                    "RegisterService",
                    &(
                        i.id,
                        i.name_template,
                        i.type_,
                        i.port,
                        i.priority,
                        i.weight,
                        i.txt,
                    ),
                )
                .await?;
            Ok(serde_json::to_value(ServiceRegistrationOutput {
                service_path: path.to_string(),
            })?)
        }
        "unregister_service" => {
            let i = input!(
                UnregisterServiceInput,
                "invalid unregister_service arguments"
            );
            let path = zbus::zvariant::ObjectPath::try_from(i.service_path.as_str())
                .context("invalid service object path")?;
            mutation!(manager.call::<_, _, ()>("UnregisterService", &(path,)))
        }

        "reset_statistics" => {
            let _fixture_arg_1: ResetStatisticsInput =
                input!(ResetStatisticsInput, "invalid reset_statistics arguments");
            mutation!(manager.call::<_, _, ()>("ResetStatistics", &()))
        }

        "flush_caches" => {
            let _fixture_arg_2: FlushCachesInput =
                input!(FlushCachesInput, "invalid flush_caches arguments");
            mutation!(manager.call::<_, _, ()>("FlushCaches", &()))
        }

        "reset_server_features" => {
            let _fixture_arg_3: ResetServerFeaturesInput = input!(
                ResetServerFeaturesInput,
                "invalid reset_server_features arguments"
            );
            mutation!(manager.call::<_, _, ()>("ResetServerFeatures", &()))
        }

        _ => anyhow::bail!(
            "unknown dnsresolver method '{}'; no canonical backend dispatch",
            method
        ),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ResolveHostnameInput {
    pub ifindex: i32,
    pub name: String,
    pub family: i32,
    pub flags: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ResolveAddressInput {
    pub ifindex: i32,
    pub family: i32,
    pub address: Vec<u8>,
    pub flags: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ResolveRecordInput {
    pub ifindex: i32,
    pub name: String,
    pub class: u16,
    pub record_type: u16,
    pub flags: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ResolveServiceInput {
    pub ifindex: i32,
    pub name: String,
    #[serde(rename = "type")]
    pub service_type: String,
    pub domain: String,
    pub family: i32,
    pub flags: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct GetLinkInput {
    pub ifindex: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SetLinkDNSInput {
    pub ifindex: i32,
    pub addresses: Vec<(i32, Vec<u8>)>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SetLinkDNSExInput {
    pub ifindex: i32,
    pub addresses: Vec<(i32, Vec<u8>, u16, String)>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SetLinkDomainsInput {
    pub ifindex: i32,
    pub domains: Vec<(String, bool)>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SetLinkDefaultRouteInput {
    pub ifindex: i32,
    pub enable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SetLinkLLMNRInput {
    pub ifindex: i32,
    pub mode: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SetLinkMulticastDNSInput {
    pub ifindex: i32,
    pub mode: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SetLinkDNSOverTLSInput {
    pub ifindex: i32,
    pub mode: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SetLinkDNSSECInput {
    pub ifindex: i32,
    pub mode: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SetLinkDNSSECNegativeTrustAnchorsInput {
    pub ifindex: i32,
    pub names: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct RevertLinkInput {
    pub ifindex: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct RegisterServiceInput {
    pub id: String,
    pub name_template: String,
    pub type_: String,
    pub port: u16,
    pub priority: u16,
    pub weight: u16,
    pub txt: Vec<std::collections::BTreeMap<String, Vec<u8>>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct UnregisterServiceInput {
    pub service_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ResetStatisticsInput {}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct FlushCachesInput {}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ResetServerFeaturesInput {}

pub(crate) fn dnsresolver_schema() -> PluginSchema {
    let mut schema = PluginSchema::builder("dnsresolver")
        .category("network")
        .version("2.0.0")
        .description("DNS resolver declaration state")
        .field(
            "version",
            FieldSchema {
                field_type: FieldType::Integer,
                required: false,
                description: "Schema version".to_string(),
                default: Some(simd_json::json!(1)),
                example: None,
                constraints: vec![Constraint::Min { value: 1.0 }],
                read_only: false,
                read_only_when: None,
            },
        )
        .field(
            "items",
            FieldSchema {
                field_type: FieldType::Array(Box::new(FieldType::Any)),
                required: true,
                description: "Resolver items".to_string(),
                default: Some(simd_json::json!([])),
                example: None,
                constraints: Vec::new(),
                read_only: false,
                read_only_when: None,
            },
        )
        .build();

    // org.freedesktop.resolve1.Manager methods
    // Spec: https://www.freedesktop.org/software/systemd/man/latest/org.freedesktop.resolve1.html
    schema.methods.insert(
        "resolve_hostname".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            ResolveHostnameInput,
            ResolveHostnameOutput,
        >(
            "ResolveHostname",
            op_state_store::SideEffect::Read,
            true,
            "dnsresolver.read",
            "obs.software.dnsresolver.hostname.resolve@v1",
        ),
    );
    schema.methods.insert(
        "resolve_address".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            ResolveAddressInput,
            ResolveAddressOutput,
        >(
            "ResolveAddress",
            op_state_store::SideEffect::Read,
            true,
            "dnsresolver.read",
            "obs.software.dnsresolver.address.resolve@v1",
        ),
    );
    schema.methods.insert(
        "resolve_record".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            ResolveRecordInput,
            ResolveRecordOutput,
        >(
            "ResolveRecord",
            op_state_store::SideEffect::Read,
            true,
            "dnsresolver.read",
            "obs.software.dnsresolver.record.resolve@v1",
        ),
    );
    schema.methods.insert(
        "resolve_service".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            ResolveServiceInput,
            ResolveServiceOutput,
        >(
            "ResolveService",
            op_state_store::SideEffect::Read,
            true,
            "dnsresolver.read",
            "obs.software.dnsresolver.service.resolve@v1",
        ),
    );
    schema.methods.insert(
        "get_link".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            GetLinkInput,
            LinkOutput,
        >(
            "GetLink",
            op_state_store::SideEffect::Read,
            true,
            "dnsresolver.read",
            "obs.software.dnsresolver.link.get@v1",
        ),
    );
    schema.methods.insert(
        "set_link_dns".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            SetLinkDNSInput,
            DnsMutationOutput,
        >(
            "SetLinkDNS",
            op_state_store::SideEffect::Mutation,
            false,
            "dnsresolver.write",
            "mut.software.dnsresolver.link.set-dns@v1",
        ),
    );
    schema.methods.insert(
        "set_link_dns_ex".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            SetLinkDNSExInput,
            DnsMutationOutput,
        >(
            "SetLinkDNSEx",
            op_state_store::SideEffect::Mutation,
            false,
            "dnsresolver.write",
            "mut.software.dnsresolver.link.set-dns-ex@v1",
        ),
    );
    schema.methods.insert(
        "set_link_domains".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            SetLinkDomainsInput,
            DnsMutationOutput,
        >(
            "SetLinkDomains",
            op_state_store::SideEffect::Mutation,
            false,
            "dnsresolver.write",
            "mut.software.dnsresolver.link.set-domains@v1",
        ),
    );
    schema.methods.insert(
        "set_link_default_route".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            SetLinkDefaultRouteInput,
            DnsMutationOutput,
        >(
            "SetLinkDefaultRoute",
            op_state_store::SideEffect::Mutation,
            false,
            "dnsresolver.write",
            "mut.software.dnsresolver.link.set-default-route@v1",
        ),
    );
    schema.methods.insert(
        "set_link_llmnr".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            SetLinkLLMNRInput,
            DnsMutationOutput,
        >(
            "SetLinkLLMNR",
            op_state_store::SideEffect::Mutation,
            false,
            "dnsresolver.write",
            "mut.software.dnsresolver.link.set-llmnr@v1",
        ),
    );
    schema.methods.insert(
        "set_link_multicast_dns".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            SetLinkMulticastDNSInput,
            DnsMutationOutput,
        >(
            "SetLinkMulticastDNS",
            op_state_store::SideEffect::Mutation,
            false,
            "dnsresolver.write",
            "mut.software.dnsresolver.link.set-mdns@v1",
        ),
    );
    schema.methods.insert(
        "set_link_dns_over_tls".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            SetLinkDNSOverTLSInput,
            DnsMutationOutput,
        >(
            "SetLinkDNSOverTLS",
            op_state_store::SideEffect::Mutation,
            false,
            "dnsresolver.write",
            "mut.software.dnsresolver.link.set-dot@v1",
        ),
    );
    schema.methods.insert(
        "set_link_dnssec".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            SetLinkDNSSECInput,
            DnsMutationOutput,
        >(
            "SetLinkDNSSEC",
            op_state_store::SideEffect::Mutation,
            false,
            "dnsresolver.write",
            "mut.software.dnsresolver.link.set-dnssec@v1",
        ),
    );
    schema.methods.insert(
        "set_link_dnssec_nta".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            SetLinkDNSSECNegativeTrustAnchorsInput,
            DnsMutationOutput,
        >(
            "SetLinkDNSSECNegativeTrustAnchors",
            op_state_store::SideEffect::Mutation,
            false,
            "dnsresolver.write",
            "mut.software.dnsresolver.link.set-dnssec-nta@v1",
        ),
    );
    schema.methods.insert(
        "revert_link".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            RevertLinkInput,
            DnsMutationOutput,
        >(
            "RevertLink",
            op_state_store::SideEffect::Mutation,
            false,
            "dnsresolver.write",
            "mut.software.dnsresolver.link.revert@v1",
        ),
    );
    schema.methods.insert(
        "register_service".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            RegisterServiceInput,
            ServiceRegistrationOutput,
        >(
            "RegisterService",
            op_state_store::SideEffect::Mutation,
            false,
            "dnsresolver.write",
            "mut.software.dnsresolver.service.register@v1",
        ),
    );
    schema.methods.insert(
        "unregister_service".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            UnregisterServiceInput,
            DnsMutationOutput,
        >(
            "UnregisterService",
            op_state_store::SideEffect::Mutation,
            false,
            "dnsresolver.write",
            "mut.software.dnsresolver.service.unregister@v1",
        ),
    );
    schema.methods.insert(
        "reset_statistics".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            ResetStatisticsInput,
            DnsMutationOutput,
        >(
            "ResetStatistics",
            op_state_store::SideEffect::Mutation,
            false,
            "dnsresolver.write",
            "mut.software.dnsresolver.statistics.reset@v1",
        ),
    );
    schema.methods.insert(
        "flush_caches".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            FlushCachesInput,
            DnsMutationOutput,
        >(
            "FlushCaches",
            op_state_store::SideEffect::Mutation,
            false,
            "dnsresolver.write",
            "mut.software.dnsresolver.caches.flush@v1",
        ),
    );
    schema.methods.insert(
        "reset_server_features".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            ResetServerFeaturesInput,
            DnsMutationOutput,
        >(
            "ResetServerFeatures",
            op_state_store::SideEffect::Mutation,
            false,
            "dnsresolver.write",
            "mut.software.dnsresolver.server-features.reset@v1",
        ),
    );

    schema.capabilities.insert(
        "dnsresolver.read".to_string(),
        op_state_store::CapabilityDecl {
            id: "dnsresolver.read".to_string(),
            description: "Grants: resolve_hostname, resolve_address, resolve_record, resolve_service, get_link.".to_string(),
        },
    );
    schema.capabilities.insert(
        "dnsresolver.write".to_string(),
        op_state_store::CapabilityDecl {
            id: "dnsresolver.write".to_string(),
            description: "Grants: set_link_dns, set_link_dns_ex, set_link_domains, set_link_default_route, set_link_llmnr, set_link_multicast_dns, set_link_dns_over_tls, set_link_dnssec, set_link_dnssec_nta, revert_link, register_service, unregister_service, reset_statistics, flush_caches, reset_server_features.".to_string(),
        },
    );

    schema
}

// Self-registration: the plugin registry discovers this via inventory
// (single source of the catalog; no central dispatch list).
inventory::submit! {
    crate::default_registry::PluginReg::new("dnsresolver", |_ctx| std::sync::Arc::new(DnsResolverPlugin::new()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::UnixStream;
    use zbus::connection;
    use zbus::zvariant::OwnedObjectPath;

    struct ResolveFixture;

    #[zbus::interface(name = "org.freedesktop.resolve1.Manager")]
    impl ResolveFixture {
        #[zbus(name = "ResolveHostname")]
        async fn resolve_hostname(
            &self,
            _fixture_arg_4: i32,
            _fixture_arg_5: String,
            _fixture_arg_6: i32,
            _fixture_arg_7: u64,
        ) -> (Vec<DnsAddress>, String, u64) {
            (
                vec![DnsAddress {
                    ifindex: 2,
                    family: 2,
                    address: vec![127, 0, 0, 1],
                }],
                "example".into(),
                1,
            )
        }
        #[zbus(name = "ResolveAddress")]
        async fn resolve_address(
            &self,
            _fixture_arg_8: i32,
            _fixture_arg_9: i32,
            _fixture_arg_10: Vec<u8>,
            _fixture_arg_11: u64,
        ) -> (Vec<DnsName>, u64) {
            (
                vec![DnsName {
                    ifindex: 2,
                    name: "example".into(),
                }],
                1,
            )
        }
        #[zbus(name = "ResolveRecord")]
        async fn resolve_record(
            &self,
            _fixture_arg_12: i32,
            _fixture_arg_13: String,
            _fixture_arg_14: u16,
            _fixture_arg_15: u16,
            _fixture_arg_16: u64,
        ) -> (Vec<DnsRecord>, u64) {
            (
                vec![DnsRecord {
                    ifindex: 2,
                    class: 1,
                    record_type: 1,
                    data: vec![127, 0, 0, 1],
                }],
                1,
            )
        }
        #[zbus(name = "ResolveService")]
        async fn resolve_service(
            &self,
            _fixture_arg_17: i32,
            _fixture_arg_18: String,
            _fixture_arg_19: String,
            _fixture_arg_20: String,
            _fixture_arg_21: i32,
            _fixture_arg_22: u64,
        ) -> (
            Vec<DnsServiceRecord>,
            Vec<Vec<u8>>,
            String,
            String,
            String,
            u64,
        ) {
            (
                vec![DnsServiceRecord {
                    priority: 1,
                    weight: 2,
                    port: 80,
                    hostname: "host".into(),
                    addresses: vec![],
                    canonical: "host".into(),
                }],
                vec![vec![1, 2]],
                "name".into(),
                "type".into(),
                "domain".into(),
                1,
            )
        }
        #[zbus(name = "GetLink")]
        async fn get_link(&self, _ifindex: i32) -> OwnedObjectPath {
            OwnedObjectPath::try_from("/org/freedesktop/resolve1/link/_2f2").unwrap()
        }

        #[zbus(name = "SetLinkDNSEx")]
        async fn set_link_dns_ex(
            &self,
            _fixture_arg_23: i32,
            _fixture_arg_24: Vec<(i32, Vec<u8>, u16, String)>,
        ) {
        }
        #[zbus(name = "SetLinkDomains")]
        async fn set_link_domains(
            &self,
            _fixture_arg_25: i32,
            _fixture_arg_26: Vec<(String, bool)>,
        ) {
        }
        #[zbus(name = "SetLinkDefaultRoute")]
        async fn set_link_default_route(&self, _fixture_arg_27: i32, _fixture_arg_28: bool) {}
        #[zbus(name = "SetLinkLLMNR")]
        async fn set_link_llmnr(&self, _fixture_arg_29: i32, _fixture_arg_30: String) {}
        #[zbus(name = "SetLinkMulticastDNS")]
        async fn set_link_multicast_dns(&self, _fixture_arg_31: i32, _fixture_arg_32: String) {}
        #[zbus(name = "SetLinkDNSOverTLS")]
        async fn set_link_dns_over_tls(&self, _fixture_arg_33: i32, _fixture_arg_34: String) {}
        #[zbus(name = "SetLinkDNSSEC")]
        async fn set_link_dnssec(&self, _fixture_arg_35: i32, _fixture_arg_36: String) {}
        #[zbus(name = "SetLinkDNSSECNegativeTrustAnchors")]
        async fn set_link_dnssec_nta(&self, _fixture_arg_37: i32, _fixture_arg_38: Vec<String>) {}
        #[zbus(name = "RevertLink")]
        async fn revert_link(&self, _fixture_arg_39: i32) {}
        #[zbus(name = "RegisterService")]
        // The fixture must implement the native D-Bus method's exact ABI.
        #[allow(clippy::too_many_arguments)]
        async fn register_service(
            &self,
            _fixture_arg_40: String,
            _fixture_arg_41: String,
            _fixture_arg_42: String,
            _fixture_arg_43: u16,
            _fixture_arg_44: u16,
            _fixture_arg_45: u16,
            _fixture_arg_46: Vec<std::collections::BTreeMap<String, Vec<u8>>>,
        ) -> OwnedObjectPath {
            OwnedObjectPath::try_from("/org/freedesktop/resolve1/service/1").unwrap()
        }
        #[zbus(name = "UnregisterService")]
        async fn unregister_service(&self, _fixture_arg_47: OwnedObjectPath) {}
        #[zbus(name = "ResetStatistics")]
        async fn reset_statistics(&self) {}
        #[zbus(name = "FlushCaches")]
        async fn flush_caches(&self) {}
        #[zbus(name = "ResetServerFeatures")]
        async fn reset_server_features(&self) {}

        #[zbus(name = "SetLinkDNS")]
        async fn set_link_dns(&self, _ifindex: i32, _addresses: Vec<(i32, Vec<u8>)>) {}
    }

    async fn fixture_connection() -> (zbus::Connection, zbus::Connection) {
        let (server_stream, client_stream) = UnixStream::pair().unwrap();
        let server_builder = connection::Builder::unix_stream(server_stream)
            .server(zbus::Guid::generate())
            .unwrap()
            .p2p();
        let client_builder = connection::Builder::unix_stream(client_stream).p2p();
        let (server, client) =
            tokio::try_join!(server_builder.build(), client_builder.build()).unwrap();
        server
            .object_server()
            .at(RESOLVE1_PATH, ResolveFixture)
            .await
            .unwrap();
        (server, client)
    }

    #[tokio::test]
    async fn manager_reads_and_link_mutations_use_typed_outputs() -> anyhow::Result<()> {
        let (_server, client) = fixture_connection().await;
        let link =
            dispatch_dnsresolver_method_on(&client, "get_link", &serde_json::json!({"ifindex": 2}))
                .await
                .unwrap();
        assert_eq!(link["path"], "/org/freedesktop/resolve1/link/_2f2");

        let changed = dispatch_dnsresolver_method_on(
            &client,
            "set_link_dns",
            &serde_json::json!({"ifindex": 2, "addresses": [[2, [127, 0, 0, 1]]]}),
        )
        .await
        .unwrap();
        assert_eq!(changed["disposition"], "accepted");
        assert!(changed.get("success").is_none());

        let calls = [
            (
                "resolve_hostname",
                serde_json::json!({"ifindex":2,"name":"example","family":0,"flags":0}),
            ),
            (
                "resolve_address",
                serde_json::json!({"ifindex":2,"family":2,"address":[127,0,0,1],"flags":0}),
            ),
            (
                "resolve_record",
                serde_json::json!({"ifindex":2,"name":"example","class":1,"record_type":1,"flags":0}),
            ),
            (
                "resolve_service",
                serde_json::json!({"ifindex":2,"name":"svc","type":"_http._tcp","domain":"local","family":0,"flags":0}),
            ),
            (
                "set_link_dns_ex",
                serde_json::json!({"ifindex":2,"addresses": [[2,[127,0,0,1],853,"dns.example"]]}),
            ),
            (
                "set_link_domains",
                serde_json::json!({"ifindex":2,"domains":[["example",true]]}),
            ),
            (
                "set_link_default_route",
                serde_json::json!({"ifindex":2,"enable":true}),
            ),
            (
                "set_link_llmnr",
                serde_json::json!({"ifindex":2,"mode":"yes"}),
            ),
            (
                "set_link_multicast_dns",
                serde_json::json!({"ifindex":2,"mode":"yes"}),
            ),
            (
                "set_link_dns_over_tls",
                serde_json::json!({"ifindex":2,"mode":"yes"}),
            ),
            (
                "set_link_dnssec",
                serde_json::json!({"ifindex":2,"mode":"yes"}),
            ),
            (
                "set_link_dnssec_nta",
                serde_json::json!({"ifindex":2,"names":["example"]}),
            ),
            ("revert_link", serde_json::json!({"ifindex":2})),
            (
                "register_service",
                serde_json::json!({"id":"test","name_template":"Test","type_":"_http._tcp","port":80,"priority":1,"weight":1,"txt":[]}),
            ),
            (
                "unregister_service",
                serde_json::json!({"service_path":"/org/freedesktop/resolve1/service/1"}),
            ),
            ("reset_statistics", serde_json::json!({})),
            ("flush_caches", serde_json::json!({})),
            ("reset_server_features", serde_json::json!({})),
        ];
        for (method, args) in calls {
            let output = dispatch_dnsresolver_method_on(&client, method, &args).await?;
            assert!(
                output.get("success").is_none(),
                "{method} must have a typed output"
            );
        }
        Ok(())
    }

    #[tokio::test]
    async fn manager_errors_are_propagated() {
        let (server, client) = fixture_connection().await;
        drop(server);
        assert!(dispatch_dnsresolver_method_on(
            &client,
            "get_link",
            &serde_json::json!({"ifindex": 2}),
        )
        .await
        .is_err());
    }
}
