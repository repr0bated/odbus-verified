//! Out-of-band chat responder wake relay for OP-DBUS.
//!
//! This service monitors for pending chat turns via the responder's MQTT wake
//! topic (`opdbus/responder-wake/<sha256(principal_id)>`) and forwards them
//! to the control-plane chatbot via D-Bus. The actual response generation
//! happens in the chatbot; this service only ensures the chatbot is notified
//! of pending work.
//!
//! The service can also poll `list_pending_turns` via 3tchedFS as a fallback
//! when MQTT delivery is delayed.

use anyhow::{Context, Result};
use clap::Parser;
use rumqttc::{AsyncClient, Event, MqttOptions, Packet, QoS};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::time::Duration;
use tokio::fs;
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
    #[arg(long, env = "MQTT_HOST", default_value = "10.0.0.3")]
    mqtt_host: String,

    /// MQTT broker port.
    #[arg(long, env = "MQTT_PORT", default_value = "1883")]
    mqtt_port: u16,

    /// Poll interval in seconds for 3tchedFS fallback.
    #[arg(long, env = "POLL_INTERVAL", default_value = "30")]
    poll_interval: u64,

    /// Enable polling mode (don't rely on MQTT wake).
    #[arg(long, env = "POLLING_MODE")]
    polling_mode: bool,

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

/// A pending turn from list_pending_turns.
#[derive(Debug, Clone, Deserialize)]
struct PendingTurnInfo {
    turn_id: String,
    conversation_id: String,
    turn_index: u64,
    owner_key: String,
    owner_principal_id: String,
    opened_at: String,
}

/// Output from list_pending_turns.
#[derive(Debug, Clone, Deserialize)]
struct ListPendingTurnsOutput {
    pending_turns: Vec<PendingTurnInfo>,
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

    /// Path to the list_pending_turns call file.
    fn list_pending_path(&self) -> PathBuf {
        self.args
            .lungs_mount
            .join("chat_response")
            .join("list_pending_turns")
    }

    /// List pending turns via 3tchedFS.
    async fn list_pending_turns(&self) -> Result<Vec<PendingTurnInfo>> {
        let path = self.list_pending_path();
        if !path.exists() {
            debug!(?path, "list_pending_turns call file not found");
            return Ok(vec![]);
        }

        // 3tchedFS call: write empty JSON to the call file and read the response
        let input = serde_json::json!({});
        fs::write(&path, serde_json::to_string(&input)?).await?;

        // Read the response (3tchedFS provides response on read after write)
        let response = fs::read_to_string(&path).await?;
        let output: ListPendingTurnsOutput = serde_json::from_str(&response)
            .context("failed to parse list_pending_turns response")?;

        Ok(output.pending_turns)
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

    /// Poll for pending turns and forward them to the chatbot.
    async fn poll_pending(&self) {
        match self.list_pending_turns().await {
            Ok(turns) if turns.is_empty() => {
                debug!("no pending turns");
            }
            Ok(turns) => {
                info!(count = turns.len(), "found pending turns");
                for turn in turns {
                    info!(
                        turn_id = %turn.turn_id,
                        conversation_id = %turn.conversation_id,
                        turn_index = turn.turn_index,
                        opened_at = %turn.opened_at,
                        "processing pending turn"
                    );
                    let wake = ResponderWake {
                        request_id: turn.turn_id,
                        owner_key: turn.owner_key,
                        owner_principal_id: turn.owner_principal_id,
                        conversation_id: Some(turn.conversation_id),
                        preview: None,
                    };
                    self.handle_wake(wake).await;
                }
            }
            Err(error) => {
                warn!(%error, "failed to list pending turns");
            }
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

        // Initial poll to catch any pending turns from before we subscribed
        self.poll_pending().await;

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
                    info!("MQTT connected, resubscribing");
                    if let Err(error) = client.subscribe(&topic, QoS::AtLeastOnce).await {
                        error!(%error, "failed to resubscribe");
                    }
                }
                Ok(_) => {}
                Err(error) => {
                    error!(%error, "MQTT error, reconnecting in 5s");
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
            }
        }
    }

    /// Run the relay loop with polling only.
    async fn run_polling(&self) -> Result<()> {
        let interval = Duration::from_secs(self.args.poll_interval);

        info!(
            principal_id = %self.args.principal_id,
            interval_secs = self.args.poll_interval,
            "starting polling wake relay"
        );

        loop {
            self.poll_pending().await;
            tokio::time::sleep(interval).await;
        }
    }

    /// Run the relay.
    async fn run(&self) -> Result<()> {
        if self.args.polling_mode {
            self.run_polling().await
        } else {
            self.run_mqtt().await
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
    relay.run().await
}
