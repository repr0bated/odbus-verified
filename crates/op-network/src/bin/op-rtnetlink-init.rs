//! Native rtnetlink boot configuration for the ovsbr0 service chain.
//!
//! This binary keeps boot-critical link, address, and route mutations independent
//! of op-grpc-bridge so the bridge can start only after egress is ready.

use anyhow::{bail, Context, Result};
use op_network::rtnetlink;
use std::env;
use std::net::Ipv4Addr;
use std::path::Path;

fn env_or(name: &str, default: &str) -> String {
    env::var(name).unwrap_or_else(|_| default.to_string())
}

fn parse_cidr(cidr: &str) -> Result<(Ipv4Addr, u8)> {
    let (address, prefix) = cidr
        .split_once('/')
        .with_context(|| format!("address '{cidr}' must use CIDR notation"))?;
    let address = address
        .parse::<Ipv4Addr>()
        .with_context(|| format!("invalid IPv4 address '{address}'"))?;
    let prefix = prefix
        .parse::<u8>()
        .with_context(|| format!("invalid IPv4 prefix '{prefix}'"))?;
    if prefix > 32 {
        bail!("IPv4 prefix must be between 0 and 32");
    }
    Ok((address, prefix))
}

async fn add_address(interface: &str, cidr: &str) -> Result<()> {
    let (address, prefix) = parse_cidr(cidr)?;
    rtnetlink::add_ipv4_address(interface, &address.to_string(), prefix)
        .await
        .with_context(|| format!("add {cidr} to {interface}"))
}

async fn link_up(interface: &str) -> Result<()> {
    rtnetlink::link_up(interface)
        .await
        .with_context(|| format!("bring {interface} up"))
}

fn require_interface(interface: &str) -> Result<()> {
    if !Path::new("/sys/class/net").join(interface).exists() {
        bail!("required interface '{interface}' does not exist");
    }
    Ok(())
}

async fn configure_public() -> Result<()> {
    let physical = env_or("UPLINK_PHYS", "eth0");
    let public = env_or("PUBLIC_PORT", "pub0");
    let public_address = env_or("PUBLIC_ADDR", "188.68.58.237/22");
    let public_gateway = env_or("PUBLIC_GW", "188.68.56.1");
    let grpc = env_or("GRPC_PORT", "grpc0");
    let grpc_address = env_or("GRPC_ADDR", "10.200.0.2/24");
    let internal_ports = env_or("INTERNAL_PORTS", "svc0 grpc0");

    require_interface(&physical)?;
    require_interface(&public)?;
    link_up(&physical).await?;

    let physical_mac = std::fs::read_to_string(format!("/sys/class/net/{physical}/address"))
        .with_context(|| format!("read {physical} MAC"))?;
    let physical_mac = physical_mac.trim();
    let public_mac = std::fs::read_to_string(format!("/sys/class/net/{public}/address"))
        .with_context(|| format!("read {public} MAC"))?;
    if public_mac.trim() != physical_mac {
        rtnetlink::link_down(&public).await?;
        rtnetlink::set_mac_address(&public, physical_mac).await?;
    }

    link_up(&public).await?;
    for interface in internal_ports.split_whitespace() {
        if Path::new("/sys/class/net").join(interface).exists() {
            link_up(interface).await?;
        }
    }

    add_address(&public, &public_address).await?;
    if Path::new("/sys/class/net").join(&grpc).exists() {
        add_address(&grpc, &grpc_address).await?;
    }
    rtnetlink::add_default_route_onlink(&public, &public_gateway)
        .await
        .with_context(|| format!("move default route to {public} via {public_gateway}"))?;

    println!("public network ready: {physical} -> {public}={public_address} via {public_gateway}");
    Ok(())
}

async fn configure_services() -> Result<()> {
    let bridge = env_or("BRIDGE", "ovsbr0");
    let bridge_address = env::var("BRIDGE_ADDR").unwrap_or_default();
    let grpc = env_or("GRPC_PORT", "grpc0");
    let grpc_address = env_or("GRPC_ADDR", "10.200.0.2/24");
    let internal_ports = env_or("INTERNAL_PORTS", "svc0 grpc0");
    let service_addresses = env::var("BRIDGE_SVC_ADDRS").unwrap_or_default();

    require_interface(&bridge)?;
    link_up(&bridge).await?;
    for interface in internal_ports.split_whitespace() {
        if Path::new("/sys/class/net").join(interface).exists() {
            link_up(interface).await?;
        }
    }

    if Path::new("/sys/class/net").join(&grpc).exists() {
        add_address(&grpc, &grpc_address).await?;
    }
    if !bridge_address.is_empty() {
        add_address(&bridge, &bridge_address).await?;
    }
    for specification in service_addresses.split_whitespace() {
        let (cidr, interface) = specification
            .split_once('@')
            .with_context(|| format!("service address '{specification}' must be CIDR@interface"))?;
        require_interface(interface)?;
        add_address(interface, cidr).await?;
    }

    println!(
        "service network ready: {grpc}={grpc_address} {bridge}={}",
        if bridge_address.is_empty() {
            "none"
        } else {
            &bridge_address
        }
    );
    Ok(())
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    match env::args().nth(1).as_deref() {
        Some("public") => configure_public().await,
        Some("services") => configure_services().await,
        _ => bail!("usage: op-rtnetlink-init <public|services>"),
    }
}
