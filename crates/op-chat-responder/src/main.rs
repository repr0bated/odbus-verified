//! Out-of-band chat responder wake relay for OP-DBUS.
//!
//! This service monitors for pending chat turns via the responder's MQTT wake
//! topic (`opdbus/responder-wake/<sha256(principal_id)>`) and forwards them
//! to the control-plane chatbot via D-Bus. The actual response generation
//! happens in the chatbot; this service only ensures the chatbot is notified
//! of pending work.
//!
//! MQTT wake delivery is required. There is no pending-turn scan or timer retry.
//! A transport failure terminates the relay explicitly; a missing publication
//! cannot be inferred from an idle subscription and requires producer-side evidence.

use anyhow::{Context, Result};
use clap::Parser;
use rumqttc::{AsyncClient, Event, MqttOptions, Packet, QoS};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::time::Duration;
use tracing::{debug, error, info, warn};

/// Out-of-band chat responder wake relay for OP-DBUS chatbot.
#[derive(Parser, Debug)]
#[command(name = "op-chat-responder")]
#[command(about = "Relays responder wake notifications to the control-plane chatbot")]
struct Args {
    /// Principal ID of this responder (chatbot container identity).
    #[arg(long, env = "RESPONDER_PRINCIPAL_ID")]
    principal_id: String,

    /// Path to the 3tchedFS lungs mount.
    #[arg(long, env = "LUNGS_MOUNT", default_value = "/opt/run-mounts/3tched")]
    lungs_mount: PathBuf,

    /// MQTT broker host.
    #[arg(long, env = "MQTT_HOST", default_value = "127.0.0.1")]
    mqtt_host: String,

    /// MQTT broker port.
    #[arg(long, env = "MQTT_PORT", default_value = "1883")]
    mqtt_port: u16,

    /// D-Bus destination for the chatbot notification.
    #[arg(
        long,
        env = "DBUS_CHATBOT_DEST",
        default_value = "org.opdbus.v1.plugins.ctl_plane_chatbot"
    )]
    dbus_dest: String,

    /// D-Bus object path for the chatbot.
    #[arg(
        long,
        env = "DBUS_CHATBOT_PATH",
        default_value = "/org/opdbus/v1/plugins/ctl_plane_chatbot"
    )]
    dbus_path: String,
}

/// The responder wake message published by the bridge.
#[derive(Debug, Clone, Deserialize)]
struct ResponderWake {
    request_id: String,
    owner_key: String,
    owner_principal_id: String,
    conversation_id: Option<String>,
    preview: Option<String>,
}

fn responder_owner_key(principal_id: &str) -> String {
    format!("{:x}", Sha256::digest(principal_id.as_bytes()))
}

fn responder_wake_topic(principal_id: &str) -> String {
    let key = responder_owner_key(principal_id);
    format!("opdbus/responder-wake/{key}")
}

struct WakeRelay {
    args: Args,
}

impl WakeRelay {
    fn new(args: Args) -> Self {
        Self { args }
    }

    /// Notify the control-plane chatbot of a pending turn via D-Bus.
    async fn notify_chatbot(&self, turn_id: &str, conversation_id: Option<&str>) -> Result<()> {
        use zbus::Connection;

        let connection = Connection::session()
            .await
            .context("failed to connect to session D-Bus")?;

        // Call the chatbot's ProcessPendingTurn method
        let reply: Result<(), zbus::Error> = connection
            .call_method(
                Some(self.args.dbus_dest.as_str()),
                self.args.dbus_path.as_str(),
                Some("org.opdbus.v1.ChatResponder"),
                "ProcessPendingTurn",
                &(turn_id, conversation_id.unwrap_or("")),
            )
            .await
            .map(|_| ());

        match reply {
            Ok(()) => {
                info!(turn_id, conversation_id, "notified chatbot of pending turn");
                Ok(())
            }
            Err(zbus::Error::MethodError(name, detail, _)) => {
                // Method errors are expected if the chatbot is busy or the turn
                // was already claimed. Log and continue.
                debug!(
                    turn_id,
                    error_name = %name,
                    ?detail,
                    "chatbot notification returned error (may be expected)"
                );
                Ok(())
            }
            Err(error) => {
                warn!(turn_id, %error, "failed to notify chatbot");
                Err(error.into())
            }
        }
    }

    /// Process a wake notification by forwarding to the chatbot.
    async fn handle_wake(&self, wake: ResponderWake) {
        info!(
            request_id = %wake.request_id,
            owner_key = %wake.owner_key,
            owner_principal_id = %wake.owner_principal_id,
            conversation_id = ?wake.conversation_id,
            preview = ?wake.preview,
            "received responder wake"
        );

        if let Err(error) = self
            .notify_chatbot(&wake.request_id, wake.conversation_id.as_deref())
            .await
        {
            error!(
                request_id = %wake.request_id,
                %error,
                "failed to forward wake to chatbot"
            );
        }
    }

    /// Run the relay loop with MQTT subscription.
    async fn run_mqtt(&self) -> Result<()> {
        let topic = responder_wake_topic(&self.args.principal_id);
        let client_id = format!(
            "op-chat-responder-{}",
            &responder_owner_key(&self.args.principal_id)[..8]
        );

        info!(
            principal_id = %self.args.principal_id,
            topic = %topic,
            "starting MQTT wake relay"
        );

        let mut mqtt_options =
            MqttOptions::new(&client_id, &self.args.mqtt_host, self.args.mqtt_port);
        mqtt_options.set_keep_alive(Duration::from_secs(30));

        let (client, mut eventloop) = AsyncClient::new(mqtt_options, 10);
        client.subscribe(&topic, QoS::AtLeastOnce).await?;

        loop {
            match eventloop.poll().await {
                Ok(Event::Incoming(Packet::Publish(publish))) => {
                    if publish.topic == topic {
                        match serde_json::from_slice::<ResponderWake>(&publish.payload) {
                            Ok(wake) => self.handle_wake(wake).await,
                            Err(error) => {
                                warn!(%error, "failed to parse responder wake payload");
                            }
                        }
                    }
                }
                Ok(Event::Incoming(Packet::ConnAck(_))) => {
                    info!("MQTT connected, subscribing to required wake channel");
                    client
                        .subscribe(&topic, QoS::AtLeastOnce)
                        .await
                        .context("required MQTT wake subscription could not be queued")?;
                }
                Ok(Event::Incoming(Packet::SubAck(ack))) => {
                    anyhow::ensure!(
                        ack.return_codes.len() == 1
                            && matches!(
                                ack.return_codes[0],
                                rumqttc::SubscribeReasonCode::Success(_)
                            ),
                        "required MQTT wake subscription rejected; pending turns cannot be discovered"
                    );
                    info!("required MQTT wake subscription accepted");
                }
                Ok(_) => {}
                Err(error) => {
                    error!(%error, "required MQTT wake channel failed; pending turns cannot be discovered");
                    return Err(error).context("required MQTT wake channel unavailable");
                }
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,rumqttc=warn".into()),
        )
        .init();

    let args = Args::parse();
    let relay = WakeRelay::new(args);
    relay.run_mqtt().await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_arguments_use_mqtt_only() {
        let args = Args::try_parse_from([
            "op-chat-responder",
            "--principal-id",
            "fixture",
            "--lungs-mount",
            "/tmp/fixture-lungs",
            "--mqtt-host",
            "localhost",
            "--mqtt-port",
            "1883",
        ])
        .unwrap();
        assert_eq!(args.principal_id, "fixture");
        assert_eq!(args.mqtt_host, "localhost");
        assert_eq!(args.mqtt_port, 1883);
    }

    #[test]
    fn retired_polling_arguments_are_rejected() {
        for retired in ["--poll-interval", "--polling-mode"] {
            let error =
                Args::try_parse_from(["op-chat-responder", "--principal-id", "fixture", retired])
                    .unwrap_err();
            assert_eq!(error.kind(), clap::error::ErrorKind::UnknownArgument);
        }
    }
}
