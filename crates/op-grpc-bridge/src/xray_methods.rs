//! Native Xray HandlerService adapter for the plugin-owned typed contract.
//!
//! All callers enter through MutationEngine's pinned contract and identity /
//! capability admission. This backing transport does not expose another public
//! OP-DBUS service. Successful operations modify the running handler only;
//! /etc/xray/xray_config.json is neither read, written nor reloaded here.

use anyhow::{bail, ensure, Context, Result};
use op_plugins::state_plugins::xray::{
    AddInboundInput, AddInboundOutput, AddUserInput, AddUserOutput, RemoveUserInput,
    RemoveUserOutput, XrayHandlerAccount, XrayHandlerProtocol, XrayHandlerUser,
    XrayInboundSecurity, XrayInboundTransport, XrayVlessFlow,
};
use op_xray_daemon::commander_client::{self, handler, xray};
use prost::Message;
use serde_json::Value;
use std::{collections::HashSet, io::Cursor, net::IpAddr, sync::Arc, time::Duration};
use tonic::{transport::Channel, Request};

type Client = handler::handler_service_client::HandlerServiceClient<Channel>;
type TypedMessage = xray::common::serial::TypedMessage;

const CALL_TIMEOUT: Duration = Duration::from_secs(15);
const VLESS_ACCOUNT: &str = "xray.proxy.vless.Account";
const VMESS_ACCOUNT: &str = "xray.proxy.vmess.Account";
const TROJAN_ACCOUNT: &str = "xray.proxy.trojan.Account";
const VLESS_INBOUND: &str = "xray.proxy.vless.inbound.Config";
const VMESS_INBOUND: &str = "xray.proxy.vmess.inbound.Config";
const TROJAN_INBOUND: &str = "xray.proxy.trojan.ServerConfig";
const RECEIVER: &str = "xray.app.proxyman.ReceiverConfig";
const TLS: &str = "xray.transport.internet.tls.Config";
// Keep this process's target/credential preflight and subsequent mutation in
// one critical section. Xray has no compare-and-set API: out-of-band commander
// writers are not covered by this guard and must not be claimed atomic.
static HANDLER_MUTATIONS: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn typed(name: &str, message: impl Message) -> TypedMessage {
    TypedMessage {
        r#type: name.to_owned(),
        value: message.encode_to_vec(),
    }
}

fn validate_key(value: &str, maximum: usize, label: &str) -> Result<()> {
    ensure!(
        !value.trim().is_empty() && value.len() <= maximum && !value.chars().any(char::is_control),
        "{label} must be nonempty, bounded and free of control characters"
    );
    Ok(())
}

fn normalized_uuid(value: &str) -> Result<String> {
    ensure!(value.len() <= 64, "account UUID exceeds its size limit");
    Ok(uuid::Uuid::parse_str(value)
        .context("account requires a valid UUID")?
        .to_string())
}

fn account_protocol(account: &XrayHandlerAccount) -> XrayHandlerProtocol {
    match account {
        XrayHandlerAccount::Vless { .. } => XrayHandlerProtocol::Vless,
        XrayHandlerAccount::Vmess { .. } => XrayHandlerProtocol::Vmess,
        XrayHandlerAccount::Trojan { .. } => XrayHandlerProtocol::Trojan,
    }
}

fn account_identity(account: &XrayHandlerAccount) -> Result<Vec<u8>> {
    match account {
        XrayHandlerAccount::Vless { id, .. } => {
            let id = uuid::Uuid::parse_str(&normalized_uuid(id)?)?;
            let mut bytes = *id.as_bytes();
            // Exact v26.3.27 validator ProcessUUID semantics: these bytes are
            // masked before lookup, so distinct textual UUIDs can collide.
            bytes[6] = 0;
            bytes[7] = 0;
            Ok(bytes.to_vec())
        }
        XrayHandlerAccount::Vmess { id } => Ok(uuid::Uuid::parse_str(&normalized_uuid(id)?)?
            .as_bytes()
            .to_vec()),
        XrayHandlerAccount::Trojan { password } => Ok(password.as_bytes().to_vec()),
    }
}

fn native_account_identity(
    account: &TypedMessage,
    protocol: XrayHandlerProtocol,
) -> Result<Vec<u8>> {
    let parsed = match protocol {
        XrayHandlerProtocol::Vless => {
            ensure!(
                account.r#type == VLESS_ACCOUNT,
                "existing account protocol mismatch"
            );
            let value = xray::proxy::vless::Account::decode(account.value.as_slice())?;
            XrayHandlerAccount::Vless {
                id: value.id,
                flow: XrayVlessFlow::None,
            }
        }
        XrayHandlerProtocol::Vmess => {
            ensure!(
                account.r#type == VMESS_ACCOUNT,
                "existing account protocol mismatch"
            );
            let value = xray::proxy::vmess::Account::decode(account.value.as_slice())?;
            XrayHandlerAccount::Vmess { id: value.id }
        }
        XrayHandlerProtocol::Trojan => {
            ensure!(
                account.r#type == TROJAN_ACCOUNT,
                "existing account protocol mismatch"
            );
            let value = xray::proxy::trojan::Account::decode(account.value.as_slice())?;
            XrayHandlerAccount::Trojan {
                password: value.password,
            }
        }
    };
    account_identity(&parsed)
}

fn protocol_config_type(protocol: XrayHandlerProtocol) -> &'static str {
    match protocol {
        XrayHandlerProtocol::Vless => VLESS_INBOUND,
        XrayHandlerProtocol::Vmess => VMESS_INBOUND,
        XrayHandlerProtocol::Trojan => TROJAN_INBOUND,
    }
}

fn build_user(user: &XrayHandlerUser) -> Result<xray::common::protocol::User> {
    validate_key(&user.email, 320, "user email key")?;
    let account = match &user.account {
        XrayHandlerAccount::Vless { id, flow } => typed(
            VLESS_ACCOUNT,
            xray::proxy::vless::Account {
                id: normalized_uuid(id)?,
                flow: match flow {
                    XrayVlessFlow::None => String::new(),
                    XrayVlessFlow::Vision => "xtls-rprx-vision".to_owned(),
                },
                // Inbound account encryption must remain empty, unlike the
                // VLESS inbound Config.decryption field ("none").
                ..Default::default()
            },
        ),
        XrayHandlerAccount::Vmess { id } => typed(
            VMESS_ACCOUNT,
            xray::proxy::vmess::Account {
                id: normalized_uuid(id)?,
                ..Default::default()
            },
        ),
        XrayHandlerAccount::Trojan { password } => {
            ensure!(
                !password.is_empty() && password.len() <= 4096,
                "Trojan password must contain 1 to 4096 bytes"
            );
            typed(
                TROJAN_ACCOUNT,
                xray::proxy::trojan::Account {
                    password: password.clone(),
                },
            )
        }
    };
    Ok(xray::common::protocol::User {
        level: user.level,
        email: user.email.clone(),
        account: Some(account),
    })
}

fn build_tls(
    certificate_pem: &str,
    private_key_pem: &str,
    alpn: &[String],
) -> Result<TypedMessage> {
    ensure!(
        !certificate_pem.is_empty() && certificate_pem.len() <= 262_144,
        "TLS certificate material must contain 1 to 262144 bytes"
    );
    ensure!(
        !private_key_pem.is_empty() && private_key_pem.len() <= 65_536,
        "TLS private-key material must contain 1 to 65536 bytes"
    );
    ensure!(
        alpn.len() <= 16
            && alpn.iter().all(|value| !value.is_empty()
                && value.len() <= 255
                && !value.chars().any(char::is_control)),
        "TLS ALPN must contain at most 16 nonempty identifiers of at most 255 bytes"
    );
    let certificates = rustls_pemfile::certs(&mut Cursor::new(certificate_pem.as_bytes()))
        .collect::<std::io::Result<Vec<_>>>()
        .context("invalid TLS certificate PEM")?;
    ensure!(
        !certificates.is_empty() && certificates.len() <= 16,
        "TLS certificate PEM must contain 1 to 16 certificates"
    );
    let key = rustls_pemfile::private_key(&mut Cursor::new(private_key_pem.as_bytes()))
        .context("invalid TLS private-key PEM")?
        .context("TLS private-key PEM contains no supported key")?;
    // Check the key is valid and matches the leaf certificate before effects.
    // No file read, global crypto-provider installation, or live TLS handshake.
    rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::aws_lc_rs::default_provider(),
    ))
    .with_protocol_versions(&[&rustls::version::TLS13])?
    .with_no_client_auth()
    .with_single_cert(certificates, key)
    .context("TLS certificate/private-key validation failed")?;
    Ok(typed(
        TLS,
        xray::transport::internet::tls::Config {
            certificate: vec![xray::transport::internet::tls::Certificate {
                certificate: certificate_pem.as_bytes().to_vec(),
                key: private_key_pem.as_bytes().to_vec(),
                // Inline bytes only: no certificate_path/key_path authority.
                one_time_loading: true,
                ..Default::default()
            }],
            next_protocol: alpn.to_vec(),
            min_version: "1.3".to_owned(),
            ..Default::default()
        },
    ))
}

fn build_inbound(input: &AddInboundInput) -> Result<handler::AddInboundRequest> {
    validate_key(&input.tag, 256, "inbound tag")?;
    ensure!(
        input.listen_ip.len() <= 45,
        "listener IP exceeds its size limit"
    );
    let address: IpAddr = input
        .listen_ip
        .parse()
        .context("listen_ip must be a literal IPv4 or IPv6 address")?;
    ensure!(input.port > 0, "listener port must be in 1..=65535");
    ensure!(
        !input.users.is_empty() && input.users.len() <= 1024,
        "new inbound must contain between 1 and 1024 users"
    );
    let mut emails = HashSet::new();
    let mut credentials = HashSet::new();
    let mut users = Vec::with_capacity(input.users.len());
    for user in &input.users {
        ensure!(
            account_protocol(&user.account) == input.protocol,
            "every user account must match the inbound protocol"
        );
        ensure!(
            emails.insert(user.email.to_lowercase()),
            "duplicate user email key in new inbound"
        );
        let identity = account_identity(&user.account)?;
        ensure!(
            credentials.insert(identity),
            "duplicate account credential in new inbound"
        );
        users.push(build_user(user)?);
    }
    let mut stream = xray::transport::internet::StreamConfig {
        protocol_name: "tcp".to_owned(),
        ..Default::default()
    };
    match &input.transport {
        XrayInboundTransport::LoopbackPlaintext => {
            ensure!(
                address.is_loopback(),
                "plaintext TCP inbounds require a literal loopback address"
            );
            ensure!(
                !input.users.iter().any(|user| matches!(
                    user.account,
                    XrayHandlerAccount::Vless {
                        flow: XrayVlessFlow::Vision,
                        ..
                    }
                )),
                "VLESS Vision requires TLS on a new inbound"
            );
        }
        XrayInboundTransport::Tls {
            certificate_pem,
            private_key_pem,
            alpn,
        } => {
            stream.security_type = TLS.to_owned();
            stream
                .security_settings
                .push(build_tls(certificate_pem, private_key_pem, alpn)?);
        }
    }
    let receiver = xray::app::proxyman::ReceiverConfig {
        port_list: Some(xray::common::net::PortList {
            range: vec![xray::common::net::PortRange {
                from: u32::from(input.port),
                to: u32::from(input.port),
            }],
        }),
        listen: Some(xray::common::net::IpOrDomain {
            address: Some(xray::common::net::ip_or_domain::Address::Ip(
                match address {
                    IpAddr::V4(ip) => ip.octets().to_vec(),
                    IpAddr::V6(ip) => ip.octets().to_vec(),
                },
            )),
        }),
        stream_settings: Some(stream),
        ..Default::default()
    };
    let proxy = match input.protocol {
        XrayHandlerProtocol::Vless => typed(
            VLESS_INBOUND,
            xray::proxy::vless::inbound::Config {
                clients: users,
                decryption: "none".to_owned(),
                ..Default::default()
            },
        ),
        XrayHandlerProtocol::Vmess => typed(
            VMESS_INBOUND,
            xray::proxy::vmess::inbound::Config {
                user: users,
                ..Default::default()
            },
        ),
        XrayHandlerProtocol::Trojan => typed(
            TROJAN_INBOUND,
            xray::proxy::trojan::ServerConfig {
                users,
                ..Default::default()
            },
        ),
    };
    Ok(handler::AddInboundRequest {
        inbound: Some(xray::core::InboundHandlerConfig {
            tag: input.tag.clone(),
            receiver_settings: Some(typed(RECEIVER, receiver)),
            proxy_settings: Some(proxy),
        }),
    })
}

fn request<T>(message: T, timeout: Duration) -> Request<T> {
    let mut request = Request::new(message);
    request.set_timeout(timeout);
    request
}

pub(crate) async fn dispatch_stats_method(args: &Value) -> Result<Value> {
    dispatch_stats_at(commander_client::DEFAULT_API_SOCKET, args, CALL_TIMEOUT).await
}

async fn dispatch_stats_at(socket: &str, args: &Value, timeout: Duration) -> Result<Value> {
    let name = args
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("name required"))?;
    validate_key(name, 4096, "stat counter name")?;
    let execute = async {
        let mut client = commander_client::stats_client(socket)
            .await
            .context("connecting to xray commander API socket")?;
        let response = client
            .get_stats(request(
                commander_client::stats::GetStatsRequest {
                    name: name.to_owned(),
                    reset: false,
                },
                timeout,
            ))
            .await
            .context("xray StatsService.GetStats")?
            .into_inner();
        let stat = response
            .stat
            .ok_or_else(|| anyhow::anyhow!("no such stat counter: {name}"))?;
        ensure!(stat.name == name, "xray returned a different stat counter");
        Ok(serde_json::json!({"name":stat.name,"value":stat.value}))
    };
    tokio::time::timeout(timeout, execute)
        .await
        .context("xray stats deadline exceeded")?
}

async fn validate_user_target(
    client: &mut Client,
    input: &AddUserInput,
    timeout: Duration,
) -> Result<()> {
    let response = client
        .list_inbounds(request(
            handler::ListInboundsRequest {
                is_only_tags: false,
            },
            timeout,
        ))
        .await
        .context("Xray HandlerService.ListInbounds target preflight failed; no user mutation sent")?
        .into_inner();
    let mut matches = response
        .inbounds
        .into_iter()
        .filter(|inbound| inbound.tag == input.inbound_tag);
    let inbound = matches
        .next()
        .context("target Xray inbound does not exist; no user mutation sent")?;
    ensure!(
        matches.next().is_none(),
        "ambiguous Xray inbound tag; no user mutation sent"
    );
    let proxy = inbound
        .proxy_settings
        .context("Xray target omits protocol settings; no user mutation sent")?;
    ensure!(
        proxy.r#type == protocol_config_type(account_protocol(&input.user.account)),
        "user account protocol differs from target inbound; no user mutation sent"
    );
    if matches!(
        input.user.account,
        XrayHandlerAccount::Vless {
            flow: XrayVlessFlow::Vision,
            ..
        }
    ) {
        let receiver = inbound
            .receiver_settings
            .context("VLESS Vision target omits receiver settings")?;
        ensure!(
            receiver.r#type == RECEIVER,
            "VLESS Vision target has unknown receiver settings"
        );
        let receiver = xray::app::proxyman::ReceiverConfig::decode(receiver.value.as_slice())
            .context("invalid native Xray receiver settings")?;
        let stream = receiver
            .stream_settings
            .context("VLESS Vision target has no transport security")?;
        ensure!(
            stream.security_type == TLS
                || stream.security_type == "xray.transport.internet.reality.Config",
            "VLESS Vision requires an existing TLS or REALITY inbound"
        );
    }
    // Xray's VLESS/Trojan validators overwrite a credential map entry when a
    // different email reuses that credential. Reject that destructive alias,
    // as well as duplicate email keys, before issuing the native mutation.
    let existing = client
        .get_inbound_users(request(
            handler::GetInboundUserRequest {
                tag: input.inbound_tag.clone(),
                email: String::new(),
            },
            timeout,
        ))
        .await
        .context("Xray HandlerService.GetInboundUsers preflight failed; no user mutation sent")?
        .into_inner();
    let identity = account_identity(&input.user.account)?;
    for user in existing.users {
        ensure!(
            user.email.to_lowercase() != input.user.email.to_lowercase(),
            "user email key already exists; no user mutation sent"
        );
        let account = user
            .account
            .context("existing Xray user omits account; no user mutation sent")?;
        ensure!(
            native_account_identity(&account, account_protocol(&input.user.account))? != identity,
            "account credential already exists; no user mutation sent"
        );
    }
    Ok(())
}

pub(crate) async fn dispatch_handler_method(method: &str, args: &Value) -> Result<Value> {
    dispatch_at(
        commander_client::DEFAULT_API_SOCKET,
        CALL_TIMEOUT,
        method,
        args,
    )
    .await
}

/// Socket selection is backing configuration, never a caller-controlled method
/// argument. Fixtures inject an isolated UDS through this private helper.
async fn dispatch_at(socket: &str, timeout: Duration, method: &str, args: &Value) -> Result<Value> {
    let execute = async {
        let _guard = HANDLER_MUTATIONS.lock().await;
        match method {
            "add_user" => {
                let input: AddUserInput = serde_json::from_value(args.clone())
                    .context("invalid typed Xray add_user input")?;
                validate_key(&input.inbound_tag, 256, "inbound tag")?;
                let user = build_user(&input.user)?;
                let operation = typed(
                    "xray.app.proxyman.command.AddUserOperation",
                    handler::AddUserOperation { user: Some(user) },
                );
                let mut client = commander_client::handler_client(socket, timeout).await?;
                validate_user_target(&mut client, &input, timeout).await?;
                client.alter_inbound(request(handler::AlterInboundRequest { tag: input.inbound_tag.clone(), operation: Some(operation) }, timeout))
                    .await.context("Xray HandlerService.AlterInbound(AddUserOperation) failed; RPC errors may have an uncertain runtime outcome")?;
                Ok(serde_json::to_value(AddUserOutput {
                    inbound_tag: input.inbound_tag,
                    email: input.user.email,
                    protocol: account_protocol(&input.user.account),
                    level: input.user.level,
                })?)
            }
            "remove_user" => {
                let input: RemoveUserInput = serde_json::from_value(args.clone())
                    .context("invalid typed Xray remove_user input")?;
                validate_key(&input.inbound_tag, 256, "inbound tag")?;
                validate_key(&input.email, 320, "user email key")?;
                let operation = typed(
                    "xray.app.proxyman.command.RemoveUserOperation",
                    handler::RemoveUserOperation {
                        email: input.email.clone(),
                    },
                );
                let mut client = commander_client::handler_client(socket, timeout).await?;
                client.alter_inbound(request(handler::AlterInboundRequest { tag: input.inbound_tag.clone(), operation: Some(operation) }, timeout))
                    .await.context("Xray HandlerService.AlterInbound(RemoveUserOperation) failed; RPC errors may have an uncertain runtime outcome")?;
                Ok(serde_json::to_value(RemoveUserOutput {
                    inbound_tag: input.inbound_tag,
                    email: input.email,
                })?)
            }
            "add_inbound" => {
                let input: AddInboundInput = serde_json::from_value(args.clone())
                    .context("invalid typed Xray add_inbound input")?;
                let native = build_inbound(&input)?;
                let mut client = commander_client::handler_client(socket, timeout).await?;
                client.add_inbound(request(native, timeout)).await
                    .context("Xray HandlerService.AddInbound failed; RPC errors may have an uncertain runtime outcome")?;
                Ok(serde_json::to_value(AddInboundOutput {
                    tag: input.tag,
                    listen_ip: input.listen_ip,
                    port: input.port,
                    protocol: input.protocol,
                    user_count: u32::try_from(input.users.len())?,
                    security: match input.transport {
                        XrayInboundTransport::LoopbackPlaintext => {
                            XrayInboundSecurity::LoopbackPlaintext
                        }
                        XrayInboundTransport::Tls { .. } => XrayInboundSecurity::Tls,
                    },
                })?)
            }
            _ => bail!("unknown Xray HandlerService plugin method"),
        }
    };
    tokio::time::timeout(timeout, execute)
        .await
        .context("Xray HandlerService operation timed out; runtime outcome may be uncertain")?
}

#[cfg(test)]
#[path = "xray_methods_tests.rs"]
mod tests;
