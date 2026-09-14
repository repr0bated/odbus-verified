//! OpenFlow protocol types backed by `rovs-openflow`.
//!
//! This module re-exports and wraps rovs-openflow types to preserve
//! the public API surface expected by callers while delegating all
//! wire-protocol work to the library.

use anyhow::{Context, Result};
use bytes::Bytes;
use rovs_openflow::{Message, MessageType, Version};
use rovs_transport::Reconnect;
use std::net::SocketAddr;
use std::time::Duration;

// Re-export rovs-openflow types used directly by callers.
pub use rovs_openflow::Match as FlowMatch;

/// Flow match field (alias for rovs-openflow `Match`).
///
/// Callers that previously constructed `FlowMatch { in_port: Some(n), .. }` should
/// now use the builder API: `FlowMatch::new().in_port(n)`.
///
/// Flow action — a simplified action enum that covers what callers need.
#[derive(Debug, Clone)]
pub enum FlowAction {
    /// Output to a specific port number.
    Output { port: u32 },
    /// Drop the packet (no instructions).
    Drop,
}

/// A flow entry for OpenFlow operations.
///
/// Maps to `rovs_openflow::Flow` when sent to the switch.
#[derive(Debug, Clone)]
pub struct FlowEntry {
    pub priority: u16,
    pub match_fields: FlowMatch,
    pub actions: Vec<FlowAction>,
    pub idle_timeout: u16,
    pub hard_timeout: u16,
    pub cookie: u64,
}

impl FlowEntry {
    /// Convert to a `rovs_openflow::Flow` ADD command.
    pub fn to_rovs_flow(&self) -> rovs_openflow::Flow {
        let mut action_list = rovs_openflow::ActionList::new();
        for action in &self.actions {
            match action {
                FlowAction::Output { port } => {
                    action_list = action_list.output(rovs_openflow::OutputPort::Port(*port));
                }
                FlowAction::Drop => {
                    // No actions = drop
                }
            }
        }

        rovs_openflow::Flow::add()
            .priority(self.priority)
            .match_fields(self.match_fields.clone())
            .actions(action_list)
            .idle_timeout(self.idle_timeout)
            .hard_timeout(self.hard_timeout)
            .cookie(self.cookie)
    }
}

/// OpenFlow protocol versions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenFlowVersion {
    V1_0,
    V1_3,
}

impl OpenFlowVersion {
    pub fn as_u8(self) -> u8 {
        match self {
            Self::V1_0 => 0x01,
            Self::V1_3 => 0x04,
        }
    }
}

impl From<OpenFlowVersion> for rovs_openflow::Version {
    fn from(v: OpenFlowVersion) -> Self {
        match v {
            OpenFlowVersion::V1_0 => rovs_openflow::Version::Of10,
            OpenFlowVersion::V1_3 => rovs_openflow::Version::Of13,
        }
    }
}

/// OpenFlow client — connects actively to an OpenFlow switch.
///
/// Backed by `rovs_openflow::VConn`.  Carries a `Reconnect` state machine so
/// callers can query backoff state and drive retry loops.
pub struct OpenFlowClient {
    vconn: rovs_openflow::VConn,
    /// Reconnection state machine — tracks backoff for the caller.
    pub reconnect: Reconnect,
}

impl OpenFlowClient {
    /// Connect to an OpenFlow switch at the given address.
    ///
    /// Performs the OF1.3 Hello handshake automatically.
    /// On success `reconnect` is moved to the `Active` state.
    /// On failure the returned error carries context; callers that retry should
    /// call `reconnect.disconnected()` / `reconnect.increase_backoff()` and then
    /// `reconnect.connecting()` before the next attempt.
    pub async fn connect(addr: SocketAddr) -> Result<Self> {
        let rovs_addr = rovs_transport::Address::Tcp {
            host: addr.ip().to_string(),
            port: addr.port(),
        };

        let mut reconnect = Reconnect::new();
        reconnect.set_max_backoff(Duration::from_secs(30));
        reconnect.connecting();

        match rovs_openflow::VConn::connect(&rovs_addr).await {
            Ok(vconn) => {
                reconnect.connected();
                Ok(Self { vconn, reconnect })
            }
            Err(e) => {
                reconnect.disconnected();
                reconnect.increase_backoff();
                Err(e).with_context(|| format!("Failed to connect to OpenFlow switch at {addr}"))
            }
        }
    }

    /// Add a flow entry to the switch.
    pub async fn add_flow(&mut self, flow: &FlowEntry) -> Result<()> {
        let rovs_flow = flow.to_rovs_flow();
        self.vconn
            .send_flow_sync(&rovs_flow)
            .await
            .context("Failed to install flow")?;
        self.reconnect.activity();
        Ok(())
    }

    /// Add a flow rule from an ovs-ofctl format string.
    ///
    /// Currently logs a warning — full format parsing is not implemented.
    pub async fn add_flow_rule(&mut self, rule: &str) -> Result<()> {
        log::warn!("String-based flow rules not yet implemented: {}", rule);
        Ok(())
    }

    /// Delete all flows on the switch (wildcard delete).
    pub async fn delete_all_flows(&mut self) -> Result<()> {
        let delete_all = rovs_openflow::Flow::delete();
        self.vconn
            .send_flow(&delete_all)
            .await
            .context("Failed to delete all flows")?;
        self.reconnect.activity();
        Ok(())
    }

    /// Send an echo request and wait for reply (keepalive).
    pub async fn echo(&mut self) -> Result<()> {
        self.vconn.echo().await.context("Echo request failed")?;
        self.reconnect.activity();
        Ok(())
    }

    /// Send a FeaturesRequest and wait for FeaturesReply.
    ///
    /// Used as a connectivity probe to verify the controller is responsive.
    /// The reply is consumed and discarded; callers that need the datapath ID
    /// should parse `msg.body` directly via the lower-level `VConn` API.
    pub async fn request_features(&mut self) -> Result<()> {
        // Send FeaturesRequest (type 5, empty body)
        let xid = 1u32;
        let req = Message::new(
            Version::Of13,
            MessageType::FeaturesRequest,
            xid,
            Bytes::new(),
        );
        self.vconn
            .send_message(&req)
            .await
            .context("Failed to send FeaturesRequest")?;

        // Drain messages until we see FeaturesReply (type 6), handling echo requests.
        loop {
            let msg = self
                .vconn
                .recv_message()
                .await
                .context("Failed to receive FeaturesReply")?;
            match msg.header.msg_type {
                MessageType::FeaturesReply => return Ok(()),
                MessageType::EchoRequest => {
                    let reply = Message::new(
                        Version::Of13,
                        MessageType::EchoReply,
                        msg.header.xid,
                        msg.body.clone(),
                    );
                    self.vconn
                        .send_message(&reply)
                        .await
                        .context("Failed to send EchoReply during features probe")?;
                }
                _ => {} // Skip async messages
            }
        }
    }

    /// Dump all flows from the switch.
    pub async fn query_flows(&mut self) -> Result<Vec<String>> {
        // Return empty list — callers that use this path rely on ovs-ofctl text parsing.
        Ok(Vec::new())
    }
}
