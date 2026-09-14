use super::*;
use op_network::wireguard_kernel::KernelPeerSnapshot;
use std::collections::BTreeMap;

#[derive(Default)]
struct FixtureKernel {
    devices: Mutex<BTreeMap<String, KernelDeviceSnapshot>>,
    ignore_apply: bool,
}

impl FixtureKernel {
    fn empty(name: &str) -> KernelDeviceSnapshot {
        KernelDeviceSnapshot {
            name: name.into(),
            public_key: None,
            listen_port: None,
            fwmark: None,
            peers: vec![],
            addresses: vec![],
            mtu: None,
            admin_up: false,
        }
    }
}

#[async_trait]
impl KernelWireGuard for FixtureKernel {
    fn generate_keypair(&self) -> Result<WireGuardKeyPair> {
        Ok(WireGuardKeyPair {
            private_key: STANDARD.encode([7_u8; 32]),
            public_key: STANDARD.encode([8_u8; 32]),
        })
    }
    fn public_from_private(&self, _private: &str) -> Result<String> {
        Ok(STANDARD.encode([8_u8; 32]))
    }
    async fn apply_device(
        &self,
        name: &str,
        config: KernelDeviceConfig,
    ) -> Result<KernelDeviceSnapshot> {
        let mut devices = self.devices.lock().await;
        let device = devices
            .entry(name.into())
            .or_insert_with(|| Self::empty(name));
        if self.ignore_apply {
            return Ok(device.clone());
        }
        if config.private_key.is_some() {
            device.public_key = Some(STANDARD.encode([8_u8; 32]));
        }
        if let Some(port) = config.listen_port {
            device.listen_port = Some(port);
        }
        if let Some(mark) = config.fwmark {
            device.fwmark = Some(mark);
        }
        if config.replace_peers {
            device.peers.clear();
        }
        for peer in config.peers {
            device.peers.retain(|old| old.public_key != peer.public_key);
            device.peers.push(KernelPeerSnapshot {
                public_key: peer.public_key,
                preshared_key_sha256: peer
                    .preshared_key
                    .as_deref()
                    .map(psk_fingerprint)
                    .transpose()?,
                endpoint: peer.endpoint,
                persistent_keepalive: peer.persistent_keepalive,
                allowed_ips: peer.allowed_ips,
                latest_handshake_unix: None,
                rx_bytes: 0,
                tx_bytes: 0,
            });
        }
        Ok(device.clone())
    }
    async fn remove_peer(&self, name: &str, public_key: &str) -> Result<KernelDeviceSnapshot> {
        let mut devices = self.devices.lock().await;
        let device = devices.get_mut(name).context("missing fixture interface")?;
        device.peers.retain(|peer| peer.public_key != public_key);
        Ok(device.clone())
    }
    async fn configure_link(
        &self,
        name: &str,
        addresses: &[(IpAddr, u8)],
        mtu: Option<u32>,
    ) -> Result<KernelDeviceSnapshot> {
        let mut devices = self.devices.lock().await;
        let device = devices.get_mut(name).context("missing fixture interface")?;
        device.addresses = addresses.to_vec();
        device.mtu = mtu;
        device.admin_up = true;
        Ok(device.clone())
    }
    async fn get_device(&self, name: &str) -> Result<Option<KernelDeviceSnapshot>> {
        Ok(self.devices.lock().await.get(name).cloned())
    }
}

fn key(byte: u8) -> String {
    STANDARD.encode([byte; 32])
}

fn owner() -> WireGuardOwner {
    WireGuardOwner::new(
        DecoyRole::Decoy,
        ["wg-decoy".to_string(), "wgcf".to_string()],
        Arc::new(FixtureKernel::default()),
    )
    .unwrap()
}

fn ignoring_owner() -> WireGuardOwner {
    WireGuardOwner::new(
        DecoyRole::Decoy,
        ["wg-decoy".to_string()],
        Arc::new(FixtureKernel {
            devices: Mutex::new(BTreeMap::new()),
            ignore_apply: true,
        }),
    )
    .unwrap()
}

#[tokio::test]
async fn six_wireguard_methods_apply_and_return_sanitized_readback() {
    let owner = owner();
    let private = key(1);
    let psk = key(2);
    let public = key(3);
    let calls = [
        (
            "set_device",
            serde_json::json!({"interface":"wg-decoy","private_key":private.clone(),"listen_port":51820,"fwmark":7}),
        ),
        ("get_device", serde_json::json!({"interface":"wg-decoy"})),
        (
            "add_peer",
            serde_json::json!({"interface":"wg-decoy","public_key":public,"preshared_key":psk.clone(),"endpoint":"192.0.2.9:51820","allowed_ips":["10.8.0.2/32"],"persistent_keepalive":25}),
        ),
        ("list_peers", serde_json::json!({"interface":"wg-decoy"})),
        (
            "set_config",
            serde_json::json!({"interface":"wg-decoy","config":{"listen_port":51821,"peers":[{"public_key":key(4),"allowed_ips":["2001:db8::1/128"]}]}}),
        ),
        (
            "remove_peer",
            serde_json::json!({"interface":"wg-decoy","public_key":key(4)}),
        ),
    ];
    for (method, args) in calls {
        let receipt = owner
            .dispatch("wireguard", method, args.clone())
            .await
            .unwrap();
        let projection = receipt.wg_opdbus_state.as_ref().unwrap();
        assert_eq!(projection.interfaces.len(), 2);
        assert!(projection.interfaces.contains_key("wg-decoy"));
        assert!(projection.interfaces.contains_key("wgcf"));
        let serialized = serde_json::to_string(&receipt).unwrap_or_default();
        assert!(!serialized.contains(&private));
        assert!(!serialized.contains(&psk));
        if method == "add_peer" {
            assert_eq!(receipt.output["changed"], true);
            let repeated = owner.dispatch("wireguard", method, args).await.unwrap();
            assert_eq!(repeated.output["changed"], false);
        }
    }
    let device = owner.device("wg-decoy").await.unwrap().unwrap();
    assert_eq!(device.listen_port, Some(51821));
    assert!(device.peers.is_empty());
}

#[tokio::test]
async fn wg_opdbus_uses_same_owner_and_preserves_anna_identity() {
    let owner = owner();
    let receipt = owner
        .dispatch(
            "wg_opdbus",
            "create_interface",
            serde_json::json!({
                "interface_name":"wg-decoy",
                "addresses":["10.8.0.1/24","2001:db8::1/64"],
                "mtu":1420,
                "device":{"private_key":key(1),"listen_port":51820,"peers":[]}
            }),
        )
        .await
        .unwrap();
    let projection = receipt.wg_opdbus_state.unwrap();
    let status = &projection.interfaces["wg-decoy"];
    assert_eq!(status.mtu, Some(1420));
    assert!(status.admin_up);
    assert_eq!(status.addresses.len(), 2);
    let identity = serde_json::to_value(&projection.service_persona).unwrap();
    assert_eq!(
        identity["principal_id"],
        crate::state_plugins::wg_opdbus::ANNA_SCRIBE_PRINCIPAL_ID
    );
    let read = owner
        .dispatch(
            "wg_opdbus",
            "get_status",
            serde_json::json!({"interface_name":"wg-decoy"}),
        )
        .await
        .unwrap();
    assert!(read.output["admin_up"].as_bool().unwrap());
    assert!(read.wg_opdbus_state.unwrap().interfaces["wg-decoy"].admin_up);
}

#[tokio::test]
async fn aggregate_projection_preserves_siblings_and_reports_known_absence() {
    let owner = owner();
    let absent = owner
        .dispatch(
            "wg_opdbus",
            "get_status",
            serde_json::json!({"interface_name":"wgcf"}),
        )
        .await
        .unwrap();
    assert!(absent.output["interface"].is_null());
    assert_eq!(absent.output["admin_up"], false);
    let absent_projection = absent.wg_opdbus_state.unwrap();
    assert_eq!(absent_projection.interfaces.len(), 2);
    assert!(absent_projection.interfaces["wgcf"].interface.is_none());
    assert!(absent_projection.interfaces["wg-decoy"].interface.is_none());

    for (name, address, port) in [
        ("wg-decoy", "10.8.0.1/24", 51820),
        ("wgcf", "10.9.0.1/24", 51821),
    ] {
        let receipt = owner
            .dispatch(
                "wg_opdbus",
                "create_interface",
                serde_json::json!({
                    "interface_name":name,
                    "addresses":[address],
                    "device":{"private_key":key(1),"listen_port":port,"peers":[]}
                }),
            )
            .await
            .unwrap();
        let projection = receipt.wg_opdbus_state.unwrap();
        assert_eq!(projection.interfaces.len(), 2);
        assert!(projection.interfaces[name].interface.is_some());
    }

    let projection = owner.wg_opdbus_state().await.unwrap();
    assert_eq!(
        projection.interfaces["wg-decoy"]
            .interface
            .as_ref()
            .unwrap()
            .listen_port,
        Some(51820)
    );
    assert_eq!(
        projection.interfaces["wgcf"]
            .interface
            .as_ref()
            .unwrap()
            .listen_port,
        Some(51821)
    );
}

#[tokio::test]
async fn ignored_kernel_updates_fail_exact_readback() {
    let owner = ignoring_owner();
    assert!(owner
        .dispatch(
            "wireguard",
            "set_device",
            serde_json::json!({
                "interface":"wg-decoy",
                "private_key":key(1),
                "listen_port":51820,
                "fwmark":7
            }),
        )
        .await
        .unwrap_err()
        .to_string()
        .contains("readback mismatch"));
    assert!(owner
        .dispatch(
            "wireguard",
            "add_peer",
            serde_json::json!({
                "interface":"wg-decoy",
                "public_key":key(3),
                "preshared_key":key(2),
                "endpoint":"192.0.2.9:51820",
                "allowed_ips":["10.8.0.7/24"],
                "persistent_keepalive":25
            }),
        )
        .await
        .unwrap_err()
        .to_string()
        .contains("peer readback mismatch"));
}

#[test]
fn validation_rejects_escape_names_noncanonical_keys_bad_cidrs_and_dns_endpoints() {
    assert!(validate_interface("../../wg0").is_err());
    assert!(validate_key("not-a-key", "public_key").is_err());
    assert!(validate_allowed_networks(&["192.0.2.1/33".into()]).is_err());
    assert_eq!(
        validate_allowed_networks(&["192.0.2.9/24".into()]).unwrap(),
        vec![("192.0.2.0".parse().unwrap(), 24)]
    );
    let peer = WireGuardPeerConfig {
        public_key: key(3),
        allowed_ips: vec!["10.0.0.1/32".into()],
        endpoint: Some("example.com:51820".into()),
        persistent_keepalive: None,
        preshared_key: None,
    };
    assert!(kernel_peer(peer).is_err());
}

#[tokio::test]
async fn interface_addresses_preserve_dual_stack_host_bits() {
    let owner = owner();
    let receipt = owner
        .dispatch(
            "wg_opdbus",
            "create_interface",
            serde_json::json!({
                "interface_name":"wg-decoy",
                "addresses":["10.44.0.9/24","2001:db8:44::9/64"],
                "device":{"private_key":key(1),"peers":[]}
            }),
        )
        .await
        .unwrap();
    let status = &receipt.wg_opdbus_state.unwrap().interfaces["wg-decoy"];
    assert_eq!(status.addresses, vec!["10.44.0.9/24", "2001:db8:44::9/64"]);
    assert_eq!(
        validate_interface_addresses(&["10.44.0.9/24".into(), "2001:db8:44::9/64".into()]).unwrap(),
        vec![
            ("10.44.0.9".parse().unwrap(), 24),
            ("2001:db8:44::9".parse().unwrap(), 64)
        ]
    );
}

fn validate_typed_call(
    schema: &op_state_store::PluginSchema,
    method: &str,
    args: &serde_json::Value,
    output: &serde_json::Value,
) {
    let declaration = schema.methods.get(method).expect("declared method");
    let args_schema = serde_json::to_value(&declaration.args).unwrap();
    jsonschema::Validator::new(&args_schema)
        .unwrap()
        .validate(args)
        .unwrap();
    let output_schema =
        serde_json::to_value(declaration.returns.as_ref().expect("typed method output")).unwrap();
    jsonschema::Validator::new(&output_schema)
        .unwrap()
        .validate(output)
        .unwrap();
}

/// Opt-in native proof. The calling thread enters a fresh network namespace
/// before constructing Tokio, netlink, D-Bus, or WireGuard handles, so no host
/// interface can be observed or changed.
#[test]
fn all_native_methods_execute_in_isolated_network_namespace() {
    if std::env::var("OP_WG_KERNEL_NETNS_TEST").as_deref() != Ok("1") {
        return;
    }
    assert_eq!(unsafe { nix::libc::geteuid() }, 0, "fixture requires root");
    assert_eq!(
        unsafe { nix::libc::unshare(nix::libc::CLONE_NEWNET) },
        0,
        "unshare(CLONE_NEWNET) failed"
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let owner = WireGuardOwner::new(
            DecoyRole::Decoy,
            ["fixture0".to_string()],
            Arc::new(LinuxKernelWireGuard),
        )
        .unwrap();
        let device_key = owner.generate_keypair().unwrap();
        let first_peer = owner.generate_keypair().unwrap();
        let second_peer = owner.generate_keypair().unwrap();
        let psk = owner.generate_keypair().unwrap();
        let secrets = [device_key.private_key(), psk.private_key()];
        let wireguard_schema = crate::state_plugins::wireguard::wireguard_schema();
        let wg_opdbus_schema = crate::state_plugins::wg_opdbus::wg_opdbus_schema();

        let calls = vec![
            (
                "set_device",
                serde_json::json!({
                    "interface":"fixture0",
                    "private_key":device_key.private_key(),
                    "listen_port":51820,
                    "fwmark":77
                }),
            ),
            ("get_device", serde_json::json!({"interface":"fixture0"})),
            (
                "add_peer",
                serde_json::json!({
                    "interface":"fixture0",
                    "public_key":first_peer.public_key(),
                    "preshared_key":psk.private_key(),
                    "endpoint":"192.0.2.9:51820",
                    "allowed_ips":["10.8.0.9/24","2001:db8::9/64"],
                    "persistent_keepalive":25
                }),
            ),
            ("list_peers", serde_json::json!({"interface":"fixture0"})),
            (
                "set_config",
                serde_json::json!({
                    "interface":"fixture0",
                    "config":{
                        "private_key":device_key.private_key(),
                        "listen_port":51821,
                        "fwmark":78,
                        "peers":[{
                            "public_key":second_peer.public_key(),
                            "allowed_ips":["203.0.113.7/24"],
                            "endpoint":"198.51.100.8:51820",
                            "persistent_keepalive":15
                        }]
                    }
                }),
            ),
            (
                "remove_peer",
                serde_json::json!({
                    "interface":"fixture0",
                    "public_key":second_peer.public_key()
                }),
            ),
        ];
        for (method, args) in calls {
            let receipt = owner
                .dispatch("wireguard", method, args.clone())
                .await
                .unwrap();
            validate_typed_call(&wireguard_schema, method, &args, &receipt.output);
            let encoded = serde_json::to_string(&receipt).unwrap();
            assert!(secrets.iter().all(|secret| !encoded.contains(*secret)));
        }

        let create_args = serde_json::json!({
            "interface_name":"fixture0",
            "addresses":["10.44.0.9/24","2001:db8:44::9/64"],
            "mtu":1420,
            "device":{
                "private_key":device_key.private_key(),
                "listen_port":51822,
                "fwmark":79,
                "peers":[{
                    "public_key":first_peer.public_key(),
                    "preshared_key":psk.private_key(),
                    "endpoint":"192.0.2.10:51820",
                    "allowed_ips":["10.99.0.9/24"],
                    "persistent_keepalive":20
                }]
            }
        });
        let created = owner
            .dispatch("wg_opdbus", "create_interface", create_args.clone())
            .await
            .unwrap();
        validate_typed_call(
            &wg_opdbus_schema,
            "create_interface",
            &create_args,
            &created.output,
        );
        let projection = created.wg_opdbus_state.as_ref().unwrap();
        let status = &projection.interfaces["fixture0"];
        assert!(status.admin_up);
        assert_eq!(status.mtu, Some(1420));
        assert!(status.addresses.contains(&"10.44.0.9/24".to_string()));
        assert!(status.addresses.contains(&"2001:db8:44::9/64".to_string()));

        let status_args = serde_json::json!({"interface_name":"fixture0"});
        let read = owner
            .dispatch("wg_opdbus", "get_status", status_args.clone())
            .await
            .unwrap();
        validate_typed_call(&wg_opdbus_schema, "get_status", &status_args, &read.output);
        let encoded = serde_json::to_string(&(created, read)).unwrap();
        assert!(secrets.iter().all(|secret| !encoded.contains(*secret)));
    });
}
