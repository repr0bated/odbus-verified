//! Linux kernel WireGuard control for the explicit decoy role.
//!
//! This module never selects the userspace backend and never runs `wg`/`ip`.

use anyhow::{Context, Result};
use futures::TryStreamExt;
use netlink_packet_route::{
    address::AddressAttribute,
    link::{LinkAttribute, LinkFlag},
};
use sha2::{Digest, Sha256};
use std::{
    net::{IpAddr, SocketAddr},
    time::UNIX_EPOCH,
};
use wireguard_control::{
    AllowedIp, Backend, Device, DeviceUpdate, InterfaceName, Key, KeyPair, PeerConfigBuilder,
};

#[derive(Clone, PartialEq, Eq)]
pub struct KernelPeerConfig {
    pub public_key: String,
    pub preshared_key: Option<String>,
    pub endpoint: Option<SocketAddr>,
    pub persistent_keepalive: Option<u16>,
    pub allowed_ips: Vec<(IpAddr, u8)>,
}

#[derive(Clone, Default, PartialEq, Eq)]
pub struct KernelDeviceConfig {
    pub private_key: Option<String>,
    pub listen_port: Option<u16>,
    pub fwmark: Option<u32>,
    pub peers: Vec<KernelPeerConfig>,
    pub replace_peers: bool,
}

#[derive(Clone, PartialEq, Eq)]
pub struct KernelPeerSnapshot {
    pub public_key: String,
    /// Internal equality token for exact PSK readback. Raw PSK bytes and this
    /// fingerprint are deliberately absent from Debug and all public DTOs.
    pub preshared_key_sha256: Option<[u8; 32]>,
    pub endpoint: Option<SocketAddr>,
    pub persistent_keepalive: Option<u16>,
    pub allowed_ips: Vec<(IpAddr, u8)>,
    pub latest_handshake_unix: Option<i64>,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
}

impl std::fmt::Debug for KernelPeerSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KernelPeerSnapshot")
            .field("public_key", &self.public_key)
            .field("endpoint", &self.endpoint)
            .field("persistent_keepalive", &self.persistent_keepalive)
            .field("allowed_ips", &self.allowed_ips)
            .field("latest_handshake_unix", &self.latest_handshake_unix)
            .field("rx_bytes", &self.rx_bytes)
            .field("tx_bytes", &self.tx_bytes)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KernelDeviceSnapshot {
    pub name: String,
    pub public_key: Option<String>,
    pub listen_port: Option<u16>,
    pub fwmark: Option<u32>,
    pub peers: Vec<KernelPeerSnapshot>,
    pub addresses: Vec<(IpAddr, u8)>,
    pub mtu: Option<u32>,
    pub admin_up: bool,
}

pub fn generate_keypair() -> (String, String) {
    let pair = KeyPair::generate();
    (pair.private.to_base64(), pair.public.to_base64())
}

pub fn public_from_private(private: &str) -> Result<String> {
    Ok(Key::from_base64(private)
        .context("invalid WireGuard private key")?
        .get_public()
        .to_base64())
}

pub async fn apply_device(name: &str, config: KernelDeviceConfig) -> Result<KernelDeviceSnapshot> {
    let interface: InterfaceName = name.parse().context("invalid WireGuard interface name")?;
    tokio::task::spawn_blocking(move || {
        let mut update = DeviceUpdate::new();
        if let Some(private) = config.private_key {
            update = update.set_keypair(KeyPair::from_private(
                Key::from_base64(&private).context("invalid WireGuard private key")?,
            ));
        }
        if let Some(port) = config.listen_port {
            update = update.set_listen_port(port);
        }
        if let Some(mark) = config.fwmark {
            update = update.set_fwmark(mark);
        }
        if config.replace_peers {
            update = update.replace_peers();
        }
        for peer in config.peers {
            let public =
                Key::from_base64(&peer.public_key).context("invalid WireGuard peer public key")?;
            let mut builder = PeerConfigBuilder::new(&public).replace_allowed_ips();
            if let Some(psk) = peer.preshared_key {
                builder = builder.set_preshared_key(
                    Key::from_base64(&psk).context("invalid WireGuard preshared key")?,
                );
            }
            if let Some(endpoint) = peer.endpoint {
                builder = builder.set_endpoint(endpoint);
            }
            if let Some(interval) = peer.persistent_keepalive {
                builder = builder.set_persistent_keepalive_interval(interval);
            }
            let ips = peer
                .allowed_ips
                .into_iter()
                .map(|(address, cidr)| AllowedIp { address, cidr })
                .collect::<Vec<_>>();
            update = update.add_peer(builder.add_allowed_ips(&ips));
        }
        update
            .apply(&interface, Backend::Kernel)
            .context("apply Linux kernel WireGuard update")
    })
    .await??;
    get_device(name)
        .await?
        .context("kernel WireGuard readback missing after apply")
}

pub async fn remove_peer(name: &str, public_key: &str) -> Result<KernelDeviceSnapshot> {
    let interface: InterfaceName = name.parse().context("invalid WireGuard interface name")?;
    let key = Key::from_base64(public_key).context("invalid WireGuard peer public key")?;
    tokio::task::spawn_blocking(move || {
        DeviceUpdate::new()
            .remove_peer_by_key(&key)
            .apply(&interface, Backend::Kernel)
    })
    .await??;
    get_device(name)
        .await?
        .context("kernel WireGuard readback missing after peer removal")
}

pub async fn configure_link(
    name: &str,
    addresses: &[(IpAddr, u8)],
    mtu: Option<u32>,
) -> Result<KernelDeviceSnapshot> {
    let (connection, handle, _) = rtnetlink::new_connection()?;
    tokio::spawn(connection);
    let mut links = handle.link().get().match_name(name.to_owned()).execute();
    let link = links
        .try_next()
        .await?
        .context("WireGuard link missing after kernel create")?;
    let index = link.header.index;
    let mut set = handle.link().set(index).up();
    if let Some(mtu) = mtu {
        set = set.mtu(mtu);
    }
    set.execute().await?;
    let mut current = handle
        .address()
        .get()
        .set_link_index_filter(index)
        .execute();
    let mut present = Vec::new();
    while let Some(message) = current.try_next().await? {
        for attr in message.attributes {
            if let AddressAttribute::Address(address) = attr {
                present.push((address, message.header.prefix_len));
            }
        }
    }
    for &(address, prefix) in addresses {
        if !present.contains(&(address, prefix)) {
            handle
                .address()
                .add(index, address, prefix)
                .execute()
                .await?;
        }
    }
    get_device(name)
        .await?
        .context("kernel WireGuard readback missing after link configuration")
}

pub async fn get_device(name: &str) -> Result<Option<KernelDeviceSnapshot>> {
    let interface: InterfaceName = name.parse().context("invalid WireGuard interface name")?;
    let device =
        tokio::task::spawn_blocking(move || Device::get(&interface, Backend::Kernel)).await?;
    let device = match device {
        Ok(device) => device,
        Err(error)
            if error.kind() == std::io::ErrorKind::NotFound
                || error.raw_os_error() == Some(libc::ENODEV) =>
        {
            return Ok(None);
        }
        Err(error) => return Err(error).context("read Linux kernel WireGuard device"),
    };
    let (connection, handle, _) = rtnetlink::new_connection()?;
    tokio::spawn(connection);
    let mut links = handle.link().get().match_name(name.to_owned()).execute();
    let link = links
        .try_next()
        .await?
        .context("WireGuard netlink link readback missing")?;
    let mtu = link.attributes.iter().find_map(|a| {
        if let LinkAttribute::Mtu(value) = a {
            Some(*value)
        } else {
            None
        }
    });
    let admin_up = link
        .header
        .flags
        .iter()
        .any(|flag| matches!(flag, LinkFlag::Up));
    let mut addresses = Vec::new();
    let mut stream = handle
        .address()
        .get()
        .set_link_index_filter(link.header.index)
        .execute();
    while let Some(message) = stream.try_next().await? {
        for attr in message.attributes {
            if let AddressAttribute::Address(address) = attr {
                addresses.push((address, message.header.prefix_len));
            }
        }
    }
    let peers = device
        .peers
        .into_iter()
        .map(|peer| KernelPeerSnapshot {
            public_key: peer.config.public_key.to_base64(),
            preshared_key_sha256: peer.config.preshared_key.as_ref().and_then(|key| {
                key.as_bytes().iter().any(|byte| *byte != 0).then(|| {
                    let digest: [u8; 32] = Sha256::digest(key.as_bytes()).into();
                    digest
                })
            }),
            endpoint: peer.config.endpoint,
            persistent_keepalive: peer.config.persistent_keepalive_interval,
            allowed_ips: peer
                .config
                .allowed_ips
                .into_iter()
                .map(|ip| (ip.address, ip.cidr))
                .collect(),
            latest_handshake_unix: peer
                .stats
                .last_handshake_time
                .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                .map(|duration| duration.as_secs() as i64),
            rx_bytes: peer.stats.rx_bytes,
            tx_bytes: peer.stats.tx_bytes,
        })
        .collect();
    Ok(Some(KernelDeviceSnapshot {
        name: device.name.to_string(),
        public_key: device.public_key.map(|key| key.to_base64()),
        listen_port: device.listen_port,
        fwmark: device.fwmark,
        peers,
        addresses,
        mtu,
        admin_up,
    }))
}

pub async fn list_devices() -> Result<Vec<KernelDeviceSnapshot>> {
    let names = tokio::task::spawn_blocking(|| Device::list(Backend::Kernel)).await??;
    let mut devices = Vec::new();
    for name in names {
        if let Some(device) = get_device(&name.to_string()).await? {
            devices.push(device);
        }
    }
    Ok(devices)
}

pub async fn delete_device(name: &str) -> Result<()> {
    let interface: InterfaceName = name.parse().context("invalid WireGuard interface name")?;
    tokio::task::spawn_blocking(move || Device::get(&interface, Backend::Kernel)?.delete())
        .await??;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Opt-in native fixture: the test thread first enters a fresh network
    /// namespace, so no server or decoy interface can be touched.
    #[test]
    fn kernel_backend_effect_and_readback_in_isolated_netns() {
        if std::env::var("OP_WG_KERNEL_NETNS_TEST").as_deref() != Ok("1") {
            return;
        }
        assert_eq!(unsafe { libc::geteuid() }, 0, "fixture requires root");
        let result = unsafe { libc::unshare(libc::CLONE_NEWNET) };
        assert_eq!(result, 0, "unshare(CLONE_NEWNET) failed");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let name = "wg-odbus-test";
            let (private_key, public_key) = generate_keypair();
            let device = apply_device(
                name,
                KernelDeviceConfig {
                    private_key: Some(private_key),
                    listen_port: Some(51_820),
                    ..KernelDeviceConfig::default()
                },
            )
            .await
            .unwrap();
            assert_eq!(device.public_key.as_deref(), Some(public_key.as_str()));
            let device = configure_link(name, &[("192.0.2.1".parse().unwrap(), 32)], Some(1420))
                .await
                .unwrap();
            assert!(device.admin_up);
            assert_eq!(device.mtu, Some(1420));
            assert!(
                device
                    .addresses
                    .contains(&("192.0.2.1".parse().unwrap(), 32))
            );
            delete_device(name).await.unwrap();
            assert!(get_device(name).await.unwrap().is_none());
        });
    }
}
