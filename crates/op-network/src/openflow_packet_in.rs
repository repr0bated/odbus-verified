//! Packet-in controller wiring (spec rovs-openflow, task 7, design D7).
//!
//! Off by default. When enabled, a `rovs_ext::controller::Controller` dials the
//! OVS listener (`ptcp:6653:10.200.0.1`, so `tcp:10.200.0.1:6653` from here),
//! registers the caller's packet handlers and runs on a task owned by the
//! returned [`PacketInController`]. Dropping the handle aborts the task.
//!
//! `rovs-ext`'s `Controller` holds a `VConn`, which negotiates OpenFlow 1.3;
//! that is sufficient for packet-in. No flow in this spec needs packet-ins, so
//! this module carries no handler logic of its own and writes no flows.

use anyhow::{Context, Result};
use rovs_ext::controller::{
    Controller, ControllerConfig, HandlerAction, HandlerContext, PacketHandler, PacketInEvent,
};
use rovs_transport::Address;
use tokio::task::JoinHandle;

/// Host of the OVS `ptcp` listener on `ovsbr0`.
pub const DEFAULT_PACKET_IN_HOST: &str = "10.200.0.1";
/// Port of the OVS `ptcp` listener on `ovsbr0`.
pub const DEFAULT_PACKET_IN_PORT: u16 = 6653;
/// Address of the OVS `ptcp` listener on `ovsbr0`, in rovs address syntax.
pub const DEFAULT_PACKET_IN_ADDRESS: &str = "tcp:10.200.0.1:6653";

/// Packet-in controller configuration.
#[derive(Debug, Clone)]
pub struct PacketInConfig {
    /// Whether the controller connects at all. `false` by default.
    pub enabled: bool,
    /// Where the OVS listener is.
    pub address: Address,
}

impl Default for PacketInConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            address: Address::Tcp {
                host: DEFAULT_PACKET_IN_HOST.to_owned(),
                port: DEFAULT_PACKET_IN_PORT,
            },
        }
    }
}

impl PacketInConfig {
    /// Build a config from an address string such as `tcp:10.200.0.1:6653`.
    pub fn from_address_str(address: &str, enabled: bool) -> Result<Self> {
        let address = address
            .parse::<Address>()
            .with_context(|| format!("invalid packet-in controller address: {address}"))?;
        Ok(Self { enabled, address })
    }
}

/// Lets a `Box<dyn PacketHandler>` be registered with the controller.
struct BoxedHandler(Box<dyn PacketHandler>);

impl PacketHandler for BoxedHandler {
    fn can_handle(&self, event: &PacketInEvent) -> bool {
        self.0.can_handle(event)
    }

    fn handle<'a>(
        &'a self,
        event: &'a PacketInEvent,
        ctx: &'a mut HandlerContext<'_>,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = rovs_ext::Result<HandlerAction>> + Send + 'a>,
    > {
        self.0.handle(event, ctx)
    }
}

/// A running packet-in controller. Dropping it stops the task.
#[derive(Debug)]
pub struct PacketInController {
    task: JoinHandle<rovs_ext::Result<()>>,
}

impl PacketInController {
    /// True once the controller loop has ended (connection lost or error).
    pub fn is_finished(&self) -> bool {
        self.task.is_finished()
    }

    /// Stop the controller task.
    pub fn abort(&self) {
        self.task.abort();
    }
}

impl Drop for PacketInController {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// Start the packet-in controller.
///
/// Returns `Ok(None)` without opening a socket when `config.enabled` is false.
/// Otherwise dials the address (a failure is returned), registers `handlers`
/// in order and runs the controller on a task owned by the returned handle.
pub async fn start(
    config: PacketInConfig,
    handlers: Vec<Box<dyn PacketHandler>>,
) -> Result<Option<PacketInController>> {
    if !config.enabled {
        return Ok(None);
    }
    let mut controller = Controller::new(&config.address, ControllerConfig::new())
        .await
        .with_context(|| format!("connect packet-in controller to {}", config.address))?;
    for handler in handlers {
        controller.register(BoxedHandler(handler));
    }
    let task = tokio::spawn(async move { controller.run().await });
    Ok(Some(PacketInController { task }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio::sync::mpsc;

    struct Recorder(mpsc::UnboundedSender<Vec<u8>>);

    impl PacketHandler for Recorder {
        fn can_handle(&self, _event: &PacketInEvent) -> bool {
            true
        }

        fn handle<'a>(
            &'a self,
            event: &'a PacketInEvent,
            _ctx: &'a mut HandlerContext<'_>,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = rovs_ext::Result<HandlerAction>> + Send + 'a>,
        > {
            Box::pin(async move {
                let _ = self.0.send(event.data().to_vec());
                Ok(HandlerAction::Handled)
            })
        }
    }

    fn frame(version: u8, msg_type: u8, xid: u32, body: &[u8]) -> Vec<u8> {
        let mut v = vec![version, msg_type];
        v.extend_from_slice(&((8 + body.len()) as u16).to_be_bytes());
        v.extend_from_slice(&xid.to_be_bytes());
        v.extend_from_slice(body);
        v
    }

    fn packet_in_body(data: &[u8]) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(&0xffff_ffffu32.to_be_bytes()); // buffer_id
        b.extend_from_slice(&(data.len() as u16).to_be_bytes());
        b.push(1); // reason: action
        b.push(0); // table_id
        b.extend_from_slice(&0u64.to_be_bytes()); // cookie
        b.extend_from_slice(&1u16.to_be_bytes()); // OFPMT_OXM
        b.extend_from_slice(&4u16.to_be_bytes()); // empty match
        b.extend_from_slice(&[0; 4]); // match padding to 8
        b.extend_from_slice(&[0; 2]); // data padding
        b.extend_from_slice(data);
        b
    }

    fn address_of(l: &TcpListener) -> Address {
        format!("tcp:127.0.0.1:{}", l.local_addr().unwrap().port())
            .parse()
            .unwrap()
    }

    #[test]
    fn default_config_is_off_and_targets_ovsbr0_listener() {
        let c = PacketInConfig::default();
        assert!(!c.enabled);
        assert_eq!(c.address.to_string(), "tcp:10.200.0.1:6653");
    }

    #[test]
    fn config_parses_address_string_and_rejects_garbage() {
        let c = PacketInConfig::from_address_str("tcp:10.200.0.1:6653", true).unwrap();
        assert!(c.enabled);
        assert_eq!(c.address.to_string(), "tcp:10.200.0.1:6653");
        assert!(PacketInConfig::from_address_str("nonsense", true).is_err());
    }

    #[tokio::test]
    async fn disabled_opens_no_socket() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let config = PacketInConfig {
            enabled: false,
            address: address_of(&listener),
        };
        let started = start(config, Vec::new()).await.unwrap();
        assert!(started.is_none());
        let accepted = tokio::time::timeout(Duration::from_millis(300), listener.accept()).await;
        assert!(accepted.is_err(), "disabled config must not connect");
    }

    #[tokio::test]
    async fn enabled_connects_sends_hello_and_delivers_packet_in() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let config = PacketInConfig {
            enabled: true,
            address: address_of(&listener),
        };
        let payload: Vec<u8> = (0u8..20).collect();
        let expected = payload.clone();

        let switch = tokio::spawn(async move {
            let (mut s, _) = listener.accept().await.unwrap();
            let mut hdr = [0u8; 8];
            s.read_exact(&mut hdr).await.unwrap();
            assert_eq!(hdr[1], 0, "first message must be HELLO");
            let offered = hdr[0];
            let xid = u32::from_be_bytes([hdr[4], hdr[5], hdr[6], hdr[7]]);
            s.write_all(&frame(offered, 0, xid, &[])).await.unwrap();
            s.write_all(&frame(offered, 10, 77, &packet_in_body(&payload)))
                .await
                .unwrap();
            // keep the connection open until the test is done
            tokio::time::sleep(Duration::from_secs(5)).await;
        });

        let (tx, mut rx) = mpsc::unbounded_channel();
        let handlers: Vec<Box<dyn PacketHandler>> = vec![Box::new(Recorder(tx))];
        let handle = start(config, handlers).await.unwrap().expect("enabled");
        let got = tokio::time::timeout(Duration::from_secs(3), rx.recv())
            .await
            .expect("packet-in delivered in time")
            .expect("channel open");
        assert_eq!(got, expected);
        assert!(!handle.is_finished());
        drop(handle);
        switch.abort();
    }

    #[tokio::test]
    async fn unreachable_address_is_an_error() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = address_of(&listener);
        drop(listener); // nothing listens on this port any more
        let config = PacketInConfig {
            enabled: true,
            address,
        };
        assert!(start(config, Vec::new()).await.is_err());
    }
}
