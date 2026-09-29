//! Single Linux-kernel WireGuard authority shared by `wireguard`, `wg_opdbus`,
//! and the decoy WGCF owner.

use crate::state_plugins::{
    wg_opdbus::{
        AnnaScribeServicePersona, CreateInterfaceInput, CreateInterfaceOutput, GetStatusInput,
        WgOpdbusAggregateState, WgOpdbusState,
    },
    wireguard::{
        AddPeerInput, GetDeviceInput, GetDeviceOutput, ListPeersInput, ListPeersOutput,
        PeerMutationOutput, RemovePeerInput, SetConfigInput, SetConfigOutput, SetDeviceInput,
        SetDeviceOutput, WireGuardDeviceConfig, WireGuardInterface, WireGuardPeer,
        WireGuardPeerConfig, WireGuardState,
    },
};
use anyhow::{ensure, Context, Result};
use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use op_network::wireguard_kernel::{
    self, KernelDeviceConfig, KernelDeviceSnapshot, KernelPeerConfig,
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    net::IpAddr,
    sync::Arc,
};
use tokio::sync::Mutex;

/// Explicit construction marker. There is intentionally no host/server role.
#[derive(Debug, Clone, Copy)]
pub enum DecoyRole {
    Decoy,
}

/// Secret keypair for in-process WGCF enrollment. It cannot be logged or serialized.
pub struct WireGuardKeyPair {
    private_key: String,
    public_key: String,
}

impl WireGuardKeyPair {
    pub fn private_key(&self) -> &str {
        &self.private_key
    }
    pub fn public_key(&self) -> &str {
        &self.public_key
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct WireGuardReceipt {
    pub output: Value,
    pub wireguard_state: WireGuardState,
    pub wg_opdbus_state: Option<WgOpdbusAggregateState>,
}

#[async_trait]
pub trait KernelWireGuard: Send + Sync {
    fn generate_keypair(&self) -> Result<WireGuardKeyPair>;
    fn public_from_private(&self, private: &str) -> Result<String>;
    async fn apply_device(
        &self,
        name: &str,
        config: KernelDeviceConfig,
    ) -> Result<KernelDeviceSnapshot>;
    async fn remove_peer(&self, name: &str, public_key: &str) -> Result<KernelDeviceSnapshot>;
    async fn configure_link(
        &self,
        name: &str,
        addresses: &[(IpAddr, u8)],
        mtu: Option<u32>,
    ) -> Result<KernelDeviceSnapshot>;
    async fn get_device(&self, name: &str) -> Result<Option<KernelDeviceSnapshot>>;
}

/// Real backend. Every call hardcodes wireguard-control's `Backend::Kernel`.
pub struct LinuxKernelWireGuard;

#[async_trait]
impl KernelWireGuard for LinuxKernelWireGuard {
    fn generate_keypair(&self) -> Result<WireGuardKeyPair> {
        let (private_key, public_key) = wireguard_kernel::generate_keypair();
        Ok(WireGuardKeyPair {
            private_key,
            public_key,
        })
    }
    fn public_from_private(&self, private: &str) -> Result<String> {
        wireguard_kernel::public_from_private(private)
    }
    async fn apply_device(
        &self,
        name: &str,
        config: KernelDeviceConfig,
    ) -> Result<KernelDeviceSnapshot> {
        wireguard_kernel::apply_device(name, config).await
    }
    async fn remove_peer(&self, name: &str, public_key: &str) -> Result<KernelDeviceSnapshot> {
        wireguard_kernel::remove_peer(name, public_key).await
    }
    async fn configure_link(
        &self,
        name: &str,
        addresses: &[(IpAddr, u8)],
        mtu: Option<u32>,
    ) -> Result<KernelDeviceSnapshot> {
        wireguard_kernel::configure_link(name, addresses, mtu).await
    }
    async fn get_device(&self, name: &str) -> Result<Option<KernelDeviceSnapshot>> {
        wireguard_kernel::get_device(name).await
    }
}

pub struct WireGuardOwner {
    kernel: Arc<dyn KernelWireGuard>,
    allowed_interfaces: BTreeSet<String>,
    effects: Mutex<()>,
}

impl WireGuardOwner {
    pub fn new(
        role: DecoyRole,
        allowed_interfaces: impl IntoIterator<Item = String>,
        kernel: Arc<dyn KernelWireGuard>,
    ) -> Result<Self> {
        ensure!(
            matches!(role, DecoyRole::Decoy),
            "WireGuard owner requires decoy role"
        );
        let allowed_interfaces = allowed_interfaces
            .into_iter()
            .map(|name| validate_interface(&name))
            .collect::<Result<BTreeSet<_>>>()?;
        ensure!(
            !allowed_interfaces.is_empty(),
            "WireGuard allowlist is empty"
        );
        Ok(Self {
            kernel,
            allowed_interfaces,
            effects: Mutex::new(()),
        })
    }

    pub fn generate_keypair(&self) -> Result<WireGuardKeyPair> {
        let pair = self.kernel.generate_keypair()?;
        validate_key(pair.private_key(), "generated private_key")?;
        validate_key(pair.public_key(), "generated public_key")?;
        Ok(pair)
    }

    pub fn public_from_private(&self, private: &str) -> Result<String> {
        validate_key(private, "private_key")?;
        let public = self.kernel.public_from_private(private)?;
        validate_key(&public, "derived public_key")?;
        Ok(public)
    }

    pub async fn device(&self, interface: &str) -> Result<Option<WireGuardInterface>> {
        let interface = self.allowed_interface(interface)?;
        self.kernel
            .get_device(&interface)
            .await?
            .map(sanitize_device)
            .transpose()
    }

    pub async fn state(&self) -> Result<WireGuardState> {
        let mut interfaces = Vec::new();
        for name in &self.allowed_interfaces {
            if let Some(device) = self.kernel.get_device(name).await? {
                interfaces.push(sanitize_device(device)?);
            }
        }
        Ok(WireGuardState { interfaces })
    }

    /// Complete WG-OPDBUS projection for the same allowlisted kernel owner.
    /// Entries are retained for absent devices so sibling disappearance is an
    /// explicit native observation rather than an omitted member.
    pub async fn wg_opdbus_state(&self) -> Result<WgOpdbusAggregateState> {
        let mut interfaces = BTreeMap::new();
        for name in &self.allowed_interfaces {
            let status = match self.kernel.get_device(name).await? {
                Some(snapshot) => wg_status(snapshot)?,
                None => WgOpdbusState::default(),
            };
            interfaces.insert(name.clone(), status);
        }
        Ok(WgOpdbusAggregateState {
            service_persona: AnnaScribeServicePersona::default(),
            interfaces,
        })
    }

    pub async fn apply_interface_profile(
        &self,
        interface: &str,
        profile: WireGuardDeviceConfig,
        addresses: Vec<String>,
        mtu: Option<u32>,
    ) -> Result<WgOpdbusState> {
        let _guard = self.effects.lock().await;
        let interface = self.allowed_interface(interface)?;
        let config = kernel_config(profile, true)?;
        let expected_public = config
            .private_key
            .as_deref()
            .map(|key| self.kernel.public_from_private(key))
            .transpose()?;
        let addresses = validate_interface_addresses(&addresses)?;
        if let Some(mtu) = mtu {
            ensure!(
                (576..=65_535).contains(&mtu),
                "mtu must be between 576 and 65535"
            );
        }
        self.kernel.apply_device(&interface, config.clone()).await?;
        let snapshot = self
            .kernel
            .configure_link(&interface, &addresses, mtu)
            .await?;
        verify_device_effect(&interface, &config, expected_public.as_deref(), &snapshot)?;
        ensure!(snapshot.admin_up, "WireGuard link did not become active");
        if let Some(mtu) = mtu {
            ensure!(snapshot.mtu == Some(mtu), "WireGuard MTU readback mismatch");
        }
        let wanted_addresses = addresses.iter().copied().collect::<BTreeSet<_>>();
        let observed_addresses = snapshot.addresses.iter().copied().collect::<BTreeSet<_>>();
        ensure!(
            wanted_addresses.is_subset(&observed_addresses),
            "WireGuard address readback mismatch"
        );
        wg_status(snapshot)
    }

    pub async fn dispatch(
        &self,
        plugin: &str,
        method: &str,
        args: Value,
    ) -> Result<WireGuardReceipt> {
        match plugin {
            "wireguard" => self.dispatch_wireguard(method, args).await,
            "wg_opdbus" => self.dispatch_wg_opdbus(method, args).await,
            _ => anyhow::bail!("unsupported decoy WireGuard plugin: {plugin}"),
        }
    }

    async fn dispatch_wireguard(&self, method: &str, args: Value) -> Result<WireGuardReceipt> {
        let output = match method {
            "set_device" => {
                let input: SetDeviceInput = serde_json::from_value(args)?;
                let _guard = self.effects.lock().await;
                let name = self.allowed_interface(&input.interface)?;
                let desired = kernel_config(
                    WireGuardDeviceConfig {
                        private_key: input.private_key,
                        listen_port: input.listen_port,
                        fwmark: input.fwmark,
                        peers: vec![],
                    },
                    false,
                )?;
                let expected_public = desired
                    .private_key
                    .as_deref()
                    .map(|key| self.kernel.public_from_private(key))
                    .transpose()?;
                let snapshot = self.kernel.apply_device(&name, desired.clone()).await?;
                verify_device_effect(&name, &desired, expected_public.as_deref(), &snapshot)?;
                serde_json::to_value(SetDeviceOutput {
                    interface: sanitize_device(snapshot)?,
                })?
            }
            "get_device" => {
                let input: GetDeviceInput = serde_json::from_value(args)?;
                serde_json::to_value(GetDeviceOutput {
                    interface: self.device(&input.interface).await?,
                })?
            }
            "add_peer" => {
                let input: AddPeerInput = serde_json::from_value(args)?;
                let _guard = self.effects.lock().await;
                let name = self.allowed_interface(&input.interface)?;
                let desired = kernel_peer(input.peer)?;
                let wanted_key = desired.public_key.clone();
                let before = self.kernel.get_device(&name).await?.and_then(|device| {
                    device
                        .peers
                        .into_iter()
                        .find(|peer| peer.public_key == wanted_key)
                });
                let changed = match before {
                    Some(before) => !peer_matches(&desired, &before)?,
                    None => true,
                };
                let config = KernelDeviceConfig {
                    private_key: None,
                    listen_port: None,
                    fwmark: None,
                    peers: vec![desired.clone()],
                    replace_peers: false,
                };
                let snapshot = self.kernel.apply_device(&name, config.clone()).await?;
                verify_device_effect(&name, &config, None, &snapshot)?;
                let interface = sanitize_device(snapshot)?;
                let peer = interface
                    .peers
                    .iter()
                    .find(|peer| peer.public_key == wanted_key)
                    .cloned()
                    .context("peer missing after kernel apply")?;
                serde_json::to_value(PeerMutationOutput {
                    interface,
                    peer: Some(peer),
                    changed,
                })?
            }
            "remove_peer" => {
                let input: RemovePeerInput = serde_json::from_value(args)?;
                let _guard = self.effects.lock().await;
                let name = self.allowed_interface(&input.interface)?;
                let key = validate_key(&input.public_key, "public_key")?;
                let before =
                    self.kernel.get_device(&name).await?.is_some_and(|device| {
                        device.peers.iter().any(|peer| peer.public_key == key)
                    });
                let interface = sanitize_device(self.kernel.remove_peer(&name, &key).await?)?;
                ensure!(
                    !interface.peers.iter().any(|peer| peer.public_key == key),
                    "peer remains after kernel removal"
                );
                serde_json::to_value(PeerMutationOutput {
                    interface,
                    peer: None,
                    changed: before,
                })?
            }
            "list_peers" => {
                let input: ListPeersInput = serde_json::from_value(args)?;
                let device = self
                    .device(&input.interface)
                    .await?
                    .context("WireGuard interface does not exist")?;
                serde_json::to_value(ListPeersOutput {
                    peers: device.peers,
                })?
            }
            "set_config" => {
                let input: SetConfigInput = serde_json::from_value(args)?;
                let _guard = self.effects.lock().await;
                let name = self.allowed_interface(&input.interface)?;
                let desired = kernel_config(input.config, true)?;
                let expected_public = desired
                    .private_key
                    .as_deref()
                    .map(|key| self.kernel.public_from_private(key))
                    .transpose()?;
                let snapshot = self.kernel.apply_device(&name, desired.clone()).await?;
                verify_device_effect(&name, &desired, expected_public.as_deref(), &snapshot)?;
                serde_json::to_value(SetConfigOutput {
                    interface: sanitize_device(snapshot)?,
                })?
            }
            _ => anyhow::bail!("unknown wireguard method: {method}"),
        };
        Ok(WireGuardReceipt {
            output,
            wireguard_state: self.state().await?,
            wg_opdbus_state: Some(self.wg_opdbus_state().await?),
        })
    }

    async fn dispatch_wg_opdbus(&self, method: &str, args: Value) -> Result<WireGuardReceipt> {
        let output = match method {
            "create_interface" => {
                let input: CreateInterfaceInput = serde_json::from_value(args)?;
                let state = self
                    .apply_interface_profile(
                        &input.interface_name,
                        input.device,
                        input.addresses,
                        input.mtu,
                    )
                    .await?;
                serde_json::to_value(CreateInterfaceOutput { status: state })?
            }
            "get_status" => {
                let input: GetStatusInput = serde_json::from_value(args)?;
                let name = self.allowed_interface(&input.interface_name)?;
                let state = match self.kernel.get_device(&name).await? {
                    Some(snapshot) => wg_status(snapshot)?,
                    None => WgOpdbusState::default(),
                };
                serde_json::to_value(&state)?
            }
            _ => anyhow::bail!("unknown wg_opdbus method: {method}"),
        };
        Ok(WireGuardReceipt {
            output,
            wireguard_state: self.state().await?,
            wg_opdbus_state: Some(self.wg_opdbus_state().await?),
        })
    }

    fn allowed_interface(&self, name: &str) -> Result<String> {
        let name = validate_interface(name)?;
        ensure!(
            self.allowed_interfaces.contains(&name),
            "WireGuard interface is not allowed for this decoy owner"
        );
        Ok(name)
    }
}

fn kernel_config(config: WireGuardDeviceConfig, replace_peers: bool) -> Result<KernelDeviceConfig> {
    if let Some(key) = &config.private_key {
        validate_key(key, "private_key")?;
    }
    let mut seen = BTreeSet::new();
    let peers = config
        .peers
        .into_iter()
        .map(|peer| {
            let parsed = kernel_peer(peer)?;
            ensure!(
                seen.insert(parsed.public_key.clone()),
                "duplicate WireGuard peer public key"
            );
            Ok(parsed)
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(KernelDeviceConfig {
        private_key: config.private_key,
        listen_port: config.listen_port,
        fwmark: config.fwmark,
        peers,
        replace_peers,
    })
}

fn kernel_peer(peer: WireGuardPeerConfig) -> Result<KernelPeerConfig> {
    let public_key = validate_key(&peer.public_key, "public_key")?;
    if let Some(psk) = &peer.preshared_key {
        validate_key(psk, "preshared_key")?;
    }
    let endpoint = peer
        .endpoint
        .map(|value| {
            value
                .parse()
                .context("endpoint must be an IP socket address")
        })
        .transpose()?;
    let allowed_ips = validate_allowed_networks(&peer.allowed_ips)?;
    Ok(KernelPeerConfig {
        public_key,
        preshared_key: peer.preshared_key,
        endpoint,
        persistent_keepalive: peer.persistent_keepalive,
        allowed_ips,
    })
}

fn validate_interface(value: &str) -> Result<String> {
    ensure!(
        !value.is_empty() && value.len() <= 15,
        "invalid interface name length"
    );
    ensure!(
        value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.')),
        "invalid interface name characters"
    );
    Ok(value.to_owned())
}

fn validate_key(value: &str, label: &str) -> Result<String> {
    let bytes = STANDARD
        .decode(value)
        .with_context(|| format!("invalid {label} base64"))?;
    ensure!(
        bytes.len() == 32 && bytes.iter().any(|byte| *byte != 0),
        "{label} must encode one non-zero 32-byte key"
    );
    ensure!(
        STANDARD.encode(&bytes) == value,
        "{label} must use canonical base64"
    );
    Ok(value.to_owned())
}

fn parse_cidrs(values: &[String], normalize_networks: bool) -> Result<Vec<(IpAddr, u8)>> {
    let mut seen = BTreeSet::new();
    values
        .iter()
        .map(|value| {
            let (address, prefix) = value
                .split_once('/')
                .context("address must use CIDR notation")?;
            let address: IpAddr = address.parse().context("invalid IP address")?;
            let prefix: u8 = prefix.parse().context("invalid CIDR prefix")?;
            ensure!(
                prefix <= if address.is_ipv4() { 32 } else { 128 },
                "CIDR prefix exceeds address width"
            );
            let address = if normalize_networks {
                canonical_network(address, prefix)
            } else {
                address
            };
            ensure!(seen.insert((address, prefix)), "duplicate CIDR");
            Ok((address, prefix))
        })
        .collect()
}

/// Interface assignments retain their host address. Converting these to the
/// containing network would configure a different address on the native link.
fn validate_interface_addresses(values: &[String]) -> Result<Vec<(IpAddr, u8)>> {
    parse_cidrs(values, false)
}

/// Peer AllowedIPs are network selectors, so their host bits are normalized
/// before native application and exact readback comparison.
fn validate_allowed_networks(values: &[String]) -> Result<Vec<(IpAddr, u8)>> {
    parse_cidrs(values, true)
}

fn canonical_network(address: IpAddr, prefix: u8) -> IpAddr {
    match address {
        IpAddr::V4(address) => {
            let bits = u32::from(address);
            let mask = if prefix == 0 {
                0
            } else {
                u32::MAX << (32 - prefix)
            };
            IpAddr::V4((bits & mask).into())
        }
        IpAddr::V6(address) => {
            let bits = u128::from(address);
            let mask = if prefix == 0 {
                0
            } else {
                u128::MAX << (128 - prefix)
            };
            IpAddr::V6((bits & mask).into())
        }
    }
}

fn psk_fingerprint(value: &str) -> Result<[u8; 32]> {
    let canonical = validate_key(value, "preshared_key")?;
    let bytes = STANDARD.decode(canonical)?;
    Ok(Sha256::digest(bytes).into())
}

fn peer_matches(
    expected: &KernelPeerConfig,
    observed: &op_network::wireguard_kernel::KernelPeerSnapshot,
) -> Result<bool> {
    let expected_ips = expected
        .allowed_ips
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    let observed_ips = observed
        .allowed_ips
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    let expected_psk = expected
        .preshared_key
        .as_deref()
        .map(psk_fingerprint)
        .transpose()?;
    Ok(expected.public_key == observed.public_key
        && expected.endpoint == observed.endpoint
        && expected.persistent_keepalive == observed.persistent_keepalive
        && expected_ips == observed_ips
        && expected_psk == observed.preshared_key_sha256)
}

fn verify_device_effect(
    interface: &str,
    expected: &KernelDeviceConfig,
    expected_public: Option<&str>,
    observed: &KernelDeviceSnapshot,
) -> Result<()> {
    ensure!(
        observed.name == interface,
        "WireGuard interface readback mismatch"
    );
    if let Some(public) = expected_public {
        ensure!(
            observed.public_key.as_deref() == Some(public),
            "WireGuard public-key readback mismatch"
        );
    }
    if let Some(port) = expected.listen_port {
        ensure!(
            observed.listen_port == Some(port),
            "WireGuard listen-port readback mismatch"
        );
    }
    if let Some(mark) = expected.fwmark {
        ensure!(
            observed.fwmark == Some(mark),
            "WireGuard fwmark readback mismatch"
        );
    }
    if expected.replace_peers {
        ensure!(
            observed.peers.len() == expected.peers.len(),
            "WireGuard peer-set readback mismatch"
        );
    }
    for peer in &expected.peers {
        let candidate = observed
            .peers
            .iter()
            .find(|candidate| candidate.public_key == peer.public_key)
            .context("WireGuard peer readback mismatch")?;
        ensure!(
            peer_matches(peer, candidate)?,
            "WireGuard peer readback mismatch"
        );
    }
    Ok(())
}

fn sanitize_device(device: KernelDeviceSnapshot) -> Result<WireGuardInterface> {
    let public_key = device
        .public_key
        .map(|key| validate_key(&key, "kernel public_key"))
        .transpose()?;
    let mut peers = device
        .peers
        .into_iter()
        .map(|peer| {
            validate_key(&peer.public_key, "kernel public_key")?;
            Ok(WireGuardPeer {
                public_key: peer.public_key,
                allowed_ips: peer
                    .allowed_ips
                    .into_iter()
                    .map(|(address, prefix)| format!("{address}/{prefix}"))
                    .collect(),
                endpoint: peer.endpoint.map(|value| value.to_string()),
                persistent_keepalive: peer.persistent_keepalive,
                latest_handshake_unix: peer.latest_handshake_unix,
                rx_bytes: peer.rx_bytes,
                tx_bytes: peer.tx_bytes,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    peers.sort_by(|a, b| a.public_key.cmp(&b.public_key));
    Ok(WireGuardInterface {
        name: validate_interface(&device.name)?,
        public_key,
        listen_port: device.listen_port,
        fwmark: device.fwmark,
        peers,
    })
}

fn wg_status(snapshot: KernelDeviceSnapshot) -> Result<WgOpdbusState> {
    let addresses = snapshot
        .addresses
        .iter()
        .map(|(address, prefix)| format!("{address}/{prefix}"))
        .collect();
    let mtu = snapshot.mtu;
    let admin_up = snapshot.admin_up;
    Ok(WgOpdbusState {
        service_persona: AnnaScribeServicePersona::default(),
        interface: Some(sanitize_device(snapshot)?),
        addresses,
        mtu,
        admin_up,
    })
}

#[cfg(test)]
#[path = "wireguard_owner_tests.rs"]
mod tests;
