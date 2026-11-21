//! Authenticated MQTT wake relay for the existing responder identity container.
//!
//! Wakes carry metadata only. Every accepted turn is queued once, then processed
//! sequentially inside the verified existing container. No scan, polling,
//! reconnect loop, automatic effect replay, D-Bus shim, or host chat fallback.

mod identity;
mod launcher;

use std::collections::HashSet;
use std::net::IpAddr;
use std::time::Duration;

use anyhow::{Context, Result, ensure};
use clap::Parser;
use rumqttc::{AsyncClient, Event, MqttOptions, Packet, QoS};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::sync::mpsc;
use tracing::{info, warn};
use uuid::Uuid;

use identity::ResponderIdentity;
use launcher::ContainerLauncher;

const QUEUE_CAPACITY: usize = 16;
const MAX_SEEN_TURNS: usize = 4096;
const MAX_WAKE_BYTES: usize = 8 * 1024;

#[derive(Parser, Debug)]
#[command(name = "op-chat-responder")]
#[command(about = "Processes authenticated turn wakes inside the bound responder container")]
struct Args {
    #[arg(long, env = "RESPONDER_PRINCIPAL_ID")]
    principal_id: String,

    #[arg(long, env = "OP_CONTROL_PLANE_CHATBOT_SESSION_ID")]
    session_id: String,

    #[arg(long, env = "OP_CONTROL_PLANE_CHATBOT_WIREGUARD_PUBKEY")]
    wireguard_pubkey: String,

    /// Existing private-loopback EMQX listener, never a remote credential sink.
    #[arg(long, env = "MQTT_HOST", default_value = "127.0.0.1")]
    mqtt_host: String,

    #[arg(long, env = "MQTT_PORT", default_value = "1883")]
    mqtt_port: u16,
}

/// The bridge binds this metadata to a designated responder. The turn ID is
/// still only a hint: container FUSE admission independently checks assignment.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct ResponderWake {
    request_id: String,
    owner_key: String,
    owner_principal_id: String,
    conversation_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CommittedWake {
    seq: u64,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Wake {
    Pending(ResponderWake),
    Committed(CommittedWake),
}

fn request_owner_key(principal_id: &str) -> String {
    format!("{:x}", Sha256::digest(principal_id.as_bytes()))
}

fn validate_wake(wake: &ResponderWake) -> Result<Uuid> {
    let turn_id = Uuid::parse_str(&wake.request_id)?;
    ensure!(turn_id.to_string() == wake.request_id,
        "responder wake turn ID is not canonical");
    ensure!(!wake.owner_principal_id.is_empty()
        && wake.owner_principal_id.len() <= 1024
        && !wake.owner_principal_id.chars().any(char::is_control)
        && wake.owner_key == request_owner_key(&wake.owner_principal_id),
        "responder wake owner binding does not match");
    if let Some(conversation) = &wake.conversation_id {
        ensure!(Uuid::parse_str(conversation)?.to_string() == *conversation,
            "responder wake conversation ID is not canonical");
    }
    Ok(turn_id)
}

fn admit_turn(seen: &mut HashSet<Uuid>, turn: Uuid) -> Result<bool> {
    if seen.contains(&turn) {
        return Ok(false);
    }
    // Never evict an unresolved or completed ID merely to allow its replay.
    ensure!(seen.len() < MAX_SEEN_TURNS, "responder deduplication capacity exhausted; no retry");
    seen.insert(turn);
    Ok(true)
}

fn validate_broker(host: &str, port: u16) -> Result<()> {
    ensure!(port != 0 && (host == "localhost"
        || host.parse::<IpAddr>().is_ok_and(|address| address.is_loopback())),
        "responder MQTT credentials may only reach the existing loopback broker");
    Ok(())
}

async fn process_turns(launcher: ContainerLauncher, mut turns: mpsc::Receiver<Uuid>) -> Result<()> {
    while let Some(turn_id) = turns.recv().await {
        // A failure can follow an accepted provider/tool effect. Never requeue it.
        launcher.process(turn_id).await?;
        info!(%turn_id, "container response receipt and readback confirmed");
    }
    anyhow::bail!("responder work queue closed");
}

async fn run(args: Args) -> Result<()> {
    validate_broker(&args.mqtt_host, args.mqtt_port)?;
    let (identity, authentication) = ResponderIdentity::load(
        args.principal_id, args.session_id, args.wireguard_pubkey,
    )?;
    let launcher = ContainerLauncher::connect(identity).await?;
    let topic = format!("opdbus/wake/{}", authentication.owner_key);
    let mut options = MqttOptions::new(
        authentication.owner_key.clone(), args.mqtt_host, args.mqtt_port,
    );
    options.set_credentials(authentication.owner_key, authentication.password);
    options.set_clean_session(true);
    options.set_keep_alive(Duration::from_secs(30));
    options.set_max_packet_size(MAX_WAKE_BYTES + 512, 32 * 1024);
    let (client, mut eventloop) = AsyncClient::new(options, 10);
    let (sender, receiver) = mpsc::channel(QUEUE_CAPACITY);
    let worker = process_turns(launcher, receiver);
    tokio::pin!(worker);
    let mut seen = HashSet::new();

    loop {
        // Keep MQTT keepalives and acknowledgements live throughout generation.
        let event = tokio::select! {
            result = &mut worker => return result.context("container response processing stopped; no replay"),
            event = eventloop.poll() => event,
        }.context("required authenticated MQTT wake channel unavailable")?;
        match event {
            Event::Incoming(Packet::Publish(publish)) if publish.topic == topic => {
                if publish.retain || publish.payload.len() > MAX_WAKE_BYTES {
                    warn!("rejected retained or oversized responder wake");
                    continue;
                }
                match serde_json::from_slice::<Wake>(&publish.payload) {
                    Ok(Wake::Pending(wake)) => {
                        let Ok(turn_id) = validate_wake(&wake) else {
                            warn!("rejected invalid responder wake");
                            continue;
                        };
                        if admit_turn(&mut seen, turn_id)? {
                            sender.try_send(turn_id)
                                .context("responder work queue unavailable or full; no retry")?;
                        }
                    }
                    // Ordinary committed-state hints share this exact identity
                    // topic. They never trigger a pending-turn scan.
                    Ok(Wake::Committed(wake)) if wake.seq != 0 => {}
                    _ => warn!("rejected malformed responder wake metadata"),
                }
            }
            Event::Incoming(Packet::ConnAck(_)) => {
                client.subscribe(&topic, QoS::AtLeastOnce).await
                    .context("required authenticated MQTT subscription could not be queued")?;
            }
            Event::Incoming(Packet::SubAck(ack)) => {
                ensure!(ack.return_codes.len() == 1
                    && matches!(ack.return_codes[0], rumqttc::SubscribeReasonCode::Success(_)),
                    "required authenticated MQTT subscription rejected");
                info!("authenticated responder wake subscription accepted");
            }
            _ => {}
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "info".into())
        .add_directive("rumqttc=warn".parse()?);
    tracing_subscriber::fmt().with_env_filter(filter).with_writer(std::io::stderr).init();
    run(Args::parse()).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(extra: &[&str]) -> Result<Args, clap::Error> {
        Args::try_parse_from(["op-chat-responder", "--principal-id", "fixture",
            "--session-id", "fixture-session", "--wireguard-pubkey", "fixture-key"]
            .into_iter().chain(extra.iter().copied()))
    }

    fn wake() -> ResponderWake {
        ResponderWake {
            request_id: "00000000-0000-4000-8000-00000000000a".into(),
            owner_key: request_owner_key("fixture-owner"),
            owner_principal_id: "fixture-owner".into(),
            conversation_id: None,
        }
    }

    #[test]
    fn service_arguments_use_authenticated_mqtt_only() {
        let parsed = args(&["--mqtt-host", "localhost", "--mqtt-port", "1883"]).unwrap();
        assert_eq!(parsed.principal_id, "fixture");
        assert_eq!(parsed.mqtt_host, "localhost");
        assert_eq!(parsed.mqtt_port, 1883);
        for retired in ["--poll-interval", "--polling-mode", "--dbus-dest", "--dbus-path", "--lungs-mount"] {
            assert_eq!(args(&[retired]).unwrap_err().kind(), clap::error::ErrorKind::UnknownArgument);
        }
    }

    #[test]
    fn wakes_require_canonical_turn_and_matching_owner() {
        let mut wake = wake();
        assert!(validate_wake(&wake).is_ok());
        wake.owner_key = request_owner_key("another-owner");
        assert!(validate_wake(&wake).is_err());
        wake.owner_key = request_owner_key(&wake.owner_principal_id);
        wake.request_id = wake.request_id.to_uppercase();
        assert!(validate_wake(&wake).is_err());
        wake.request_id = "not-a-turn".into();
        assert!(validate_wake(&wake).is_err());
    }

    #[test]
    fn wake_conversation_is_optional_but_cannot_be_malformed() {
        let mut wake = wake();
        wake.conversation_id = Some("00000000-0000-4000-8000-000000000002".into());
        assert!(validate_wake(&wake).is_ok());
        wake.conversation_id = Some("../../another-turn".into());
        assert!(validate_wake(&wake).is_err());
    }

    #[test]
    fn duplicates_never_consume_queue_capacity_or_evict_prior_turns() {
        let turn = validate_wake(&wake()).unwrap();
        let mut seen = HashSet::new();
        assert!(admit_turn(&mut seen, turn).unwrap());
        assert!(!admit_turn(&mut seen, turn).unwrap());
        for id in 1..MAX_SEEN_TURNS {
            seen.insert(Uuid::from_u128(id as u128));
        }
        assert_eq!(seen.len(), MAX_SEEN_TURNS);
        assert!(!admit_turn(&mut seen, turn).unwrap());
        assert!(admit_turn(&mut seen, Uuid::from_u128(u128::MAX)).is_err());
    }

    #[test]
    fn ordinary_wakes_do_not_become_pending_turns_and_broker_stays_loopback() {
        assert!(matches!(serde_json::from_str::<Wake>(r#"{"seq":1}"#).unwrap(), Wake::Committed(_)));
        assert!(serde_json::from_str::<Wake>(r#"{"seq":1,"request_id":"forged"}"#).is_err());
        assert!(validate_broker("127.0.0.1", 1883).is_ok());
        assert!(validate_broker("::1", 1883).is_ok());
        assert!(validate_broker("198.51.100.1", 1883).is_err());
        assert!(validate_broker("localhost", 0).is_err());
    }
}
