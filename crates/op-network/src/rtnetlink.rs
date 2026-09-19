//! Rtnetlink helpers - native netlink operations for IP addresses and routes

use anyhow::{Context, Result};
use futures::TryStreamExt;
use netlink_packet_route::address::AddressAttribute;
use netlink_packet_route::link::LinkAttribute;
use netlink_packet_route::route::{RouteAddress, RouteAttribute, RouteMessage};
use rtnetlink::{new_connection, IpVersion};
use serde::{Deserialize, Serialize};
use std::net::Ipv4Addr;

/// Network interface information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkInterface {
    pub name: String,
    pub index: u32,
    pub mac_address: Option<String>,
    pub mtu: Option<u32>,
    pub flags: Vec<String>,
    pub state: String,
    pub kind: Option<String>,
    pub addresses: Vec<InterfaceAddress>,
}

/// IP address on an interface
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterfaceAddress {
    pub address: String,
    pub prefix_len: u8,
    pub family: String,
}

/// List all network interfaces with their details
pub async fn list_interfaces() -> Result<Vec<NetworkInterface>> {
    let (connection, handle, _) = new_connection()?;
    tokio::spawn(connection);

    let mut interfaces = Vec::new();
    let mut links = handle.link().get().execute();

    while let Some(link) = links.try_next().await? {
        let index = link.header.index;
        let mut name = String::new();
        let mut mac_address = None;
        let mut mtu = None;
        let mut kind = None;

        // Extract attributes
        for attr in &link.attributes {
            match attr {
                LinkAttribute::IfName(n) => name = n.clone(),
                LinkAttribute::Address(addr) => {
                    mac_address = Some(
                        addr.iter()
                            .map(|b| format!("{:02x}", b))
                            .collect::<Vec<_>>()
                            .join(":"),
                    );
                }
                LinkAttribute::Mtu(m) => mtu = Some(*m),
                LinkAttribute::LinkInfo(infos) => {
                    for info in infos {
                        if let netlink_packet_route::link::LinkInfo::Kind(k) = info {
                            kind = Some(format!("{:?}", k));
                        }
                    }
                }
                _ => {}
            }
        }

        // Determine state from flags
        let flags_val = &link.header.flags;
        let flags: Vec<String> = flags_val.iter().map(|f| format!("{:?}", f)).collect();
        let is_up = flags_val
            .iter()
            .any(|f| matches!(f, netlink_packet_route::link::LinkFlag::Up));
        let state = if is_up {
            "up".to_string()
        } else {
            "down".to_string()
        };

        // Get addresses for this interface
        let addresses = match get_interface_addresses(&handle, index).await {
            Ok(addrs) => addrs,
            Err(e) => {
                log::warn!("Failed to get addresses for interface {}: {}", name, e);
                Vec::new()
            }
        };

        interfaces.push(NetworkInterface {
            name,
            index,
            mac_address,
            mtu,
            flags,
            state,
            kind,
            addresses,
        });
    }

    Ok(interfaces)
}

/// Get addresses for a specific interface
async fn get_interface_addresses(
    handle: &rtnetlink::Handle,
    ifindex: u32,
) -> Result<Vec<InterfaceAddress>> {
    let mut addresses = Vec::new();
    let mut addr_stream = handle
        .address()
        .get()
        .set_link_index_filter(ifindex)
        .execute();

    while let Some(addr_msg) = addr_stream.try_next().await? {
        let family = match addr_msg.header.family {
            netlink_packet_route::AddressFamily::Inet => "inet".to_string(),
            netlink_packet_route::AddressFamily::Inet6 => "inet6".to_string(),
            f => format!("{:?}", f),
        };

        for attr in &addr_msg.attributes {
            if let AddressAttribute::Address(addr) = attr {
                let addr_str = addr.to_string();

                addresses.push(InterfaceAddress {
                    address: addr_str,
                    prefix_len: addr_msg.header.prefix_len,
                    family: family.clone(),
                });
            }
        }
    }

    Ok(addresses)
}

/// Extract a canonical IP gateway without exposing enum Debug formatting.
fn route_gateway(route: &RouteMessage) -> Result<Option<String>> {
    let mut gateway = None;
    for attribute in &route.attributes {
        match attribute {
            RouteAttribute::Gateway(RouteAddress::Inet(address)) => {
                gateway = Some(address.to_string());
            }
            RouteAttribute::Gateway(RouteAddress::Inet6(address)) => {
                gateway = Some(address.to_string());
            }
            RouteAttribute::Gateway(_) => {
                anyhow::bail!("unsupported route gateway address family");
            }
            _ => {}
        }
    }
    Ok(gateway)
}

/// Get default route information
pub async fn get_default_route() -> Result<Option<serde_json::Value>> {
    use netlink_packet_route::route::RouteAttribute;

    let (connection, handle, _) = new_connection()?;
    tokio::spawn(connection);

    let mut routes = handle.route().get(IpVersion::V4).execute();

    while let Some(route) = routes.try_next().await? {
        // Check if this is a default route (destination 0.0.0.0/0)
        if route.header.destination_prefix_length == 0 {
            let gateway = route_gateway(&route)?;
            let mut oif_index = None;

            for attr in &route.attributes {
                if let RouteAttribute::Oif(idx) = attr {
                    oif_index = Some(idx);
                }
            }

            // Try to get interface name for the output interface
            let mut oif_name = None;
            if let Some(idx) = oif_index {
                let mut links = handle.link().get().match_index(*idx).execute();
                if let Some(link) = links.try_next().await? {
                    for attr in &link.attributes {
                        if let LinkAttribute::IfName(name) = attr {
                            oif_name = Some(name.clone());
                            break;
                        }
                    }
                }
            }

            return Ok(Some(serde_json::json!({
                "gateway": gateway,
                "interface_index": oif_index,
                "interface_name": oif_name,
                "destination": "0.0.0.0/0",
            })));
        }
    }

    Ok(None)
}

/// Add IPv4 address to interface
pub async fn add_ipv4_address(ifname: &str, ip: &str, prefix: u8) -> Result<()> {
    let (connection, handle, _) = new_connection()?;
    tokio::spawn(connection);

    // Find interface by name
    let mut links = handle.link().get().match_name(ifname.to_string()).execute();
    let link = links
        .try_next()
        .await?
        .context(format!("Interface '{}' not found", ifname))?;

    let ifindex = link.header.index;

    // Parse IP address
    let addr: Ipv4Addr = ip.parse().context("Invalid IPv4 address")?;

    // Add address to interface
    handle
        .address()
        .add(ifindex, addr.into(), prefix)
        .execute()
        .await
        .context("Failed to add IP address")?;

    Ok(())
}

/// Delete IPv4 address from interface
#[allow(dead_code)]
pub async fn del_ipv4_address(ifname: &str, ip: &str, prefix: u8) -> Result<()> {
    let (connection, handle, _) = new_connection()?;
    tokio::spawn(connection);

    // Find interface by name
    let mut links = handle.link().get().match_name(ifname.to_string()).execute();
    let link = links
        .try_next()
        .await?
        .context(format!("Interface '{}' not found", ifname))?;

    let ifindex = link.header.index;

    // Parse IP address
    let addr: Ipv4Addr = ip.parse().context("Invalid IPv4 address")?;

    // Get addresses filtered by interface, prefix, and address
    let mut addresses = handle
        .address()
        .get()
        .set_link_index_filter(ifindex)
        .set_prefix_length_filter(prefix)
        .set_address_filter(std::net::IpAddr::V4(addr))
        .execute();

    if let Some(addr_msg) = addresses.try_next().await? {
        handle.address().del(addr_msg).execute().await?;
    }

    Ok(())
}

/// Flush all addresses from interface
#[allow(dead_code)]
pub async fn flush_addresses(ifname: &str) -> Result<()> {
    let (connection, handle, _) = new_connection()?;
    tokio::spawn(connection);

    // Find interface by name
    let mut links = handle.link().get().match_name(ifname.to_string()).execute();
    let link = links
        .try_next()
        .await?
        .context(format!("Interface '{}' not found", ifname))?;

    let ifindex = link.header.index;

    // Get all addresses on this interface
    let mut addresses = handle
        .address()
        .get()
        .set_link_index_filter(ifindex)
        .execute();

    while let Some(addr) = addresses.try_next().await? {
        // Delete this address
        if let Err(e) = handle.address().del(addr).execute().await {
            log::warn!("Failed to delete address: {}", e);
        }
    }

    Ok(())
}

/// Set link up
pub async fn link_up(ifname: &str) -> Result<()> {
    let (connection, handle, _) = new_connection()?;
    tokio::spawn(connection);

    // Find interface by name
    let mut links = handle.link().get().match_name(ifname.to_string()).execute();
    let link = links
        .try_next()
        .await?
        .context(format!("Interface '{}' not found", ifname))?;

    let ifindex = link.header.index;

    // Set link up
    handle
        .link()
        .set(ifindex)
        .up()
        .execute()
        .await
        .context("Failed to bring link up")?;

    Ok(())
}

/// Set link down
#[allow(dead_code)]
pub async fn link_down(ifname: &str) -> Result<()> {
    let (connection, handle, _) = new_connection()?;
    tokio::spawn(connection);

    // Find interface by name
    let mut links = handle.link().get().match_name(ifname.to_string()).execute();
    let link = links
        .try_next()
        .await?
        .context(format!("Interface '{}' not found", ifname))?;

    let ifindex = link.header.index;

    // Set link down
    handle
        .link()
        .set(ifindex)
        .down()
        .execute()
        .await
        .context("Failed to bring link down")?;

    Ok(())
}

/// Add default route
pub async fn add_default_route(ifname: &str, gateway: &str) -> Result<()> {
    let (connection, handle, _) = new_connection()?;
    tokio::spawn(connection);

    // Find interface by name
    let mut links = handle.link().get().match_name(ifname.to_string()).execute();
    let link = links
        .try_next()
        .await?
        .context(format!("Interface '{}' not found", ifname))?;

    let ifindex = link.header.index;

    // Parse gateway address
    let gw: Ipv4Addr = gateway.parse().context("Invalid gateway address")?;

    // Add default route
    handle
        .route()
        .add()
        .v4()
        .destination_prefix(Ipv4Addr::new(0, 0, 0, 0), 0)
        .gateway(gw)
        .output_interface(ifindex)
        .execute()
        .await
        .context("Failed to add default route")?;

    Ok(())
}

/// Add default route with `onlink` flag via native netlink.
///
/// Required when the output interface is an OVS internal port: the kernel
/// cannot verify gateway reachability until OVS is forwarding, so we must
/// bypass nexthop validation.  We build the `RTM_NEWROUTE` message manually
/// using `netlink-packet-route` so that the `RTNH_F_ONLINK` (0x04) flag is
/// set in the nexthop attribute — the high-level `rtnetlink` crate does not
/// expose this flag through its typed builder.
pub async fn add_default_route_onlink(ifname: &str, gateway: &str) -> Result<()> {
    use netlink_packet_core::{
        NetlinkHeader, NetlinkMessage, NetlinkPayload, NLM_F_ACK, NLM_F_CREATE, NLM_F_REPLACE,
        NLM_F_REQUEST,
    };
    use netlink_packet_route::route::{
        RouteAddress, RouteAttribute, RouteHeader, RouteMessage, RouteNextHop, RouteNextHopFlag,
        RouteProtocol, RouteScope, RouteType,
    };
    use netlink_packet_route::RouteNetlinkMessage;
    use netlink_sys::{protocols::NETLINK_ROUTE, Socket};

    // Resolve interface index
    let (connection, handle, _) = new_connection()?;
    tokio::spawn(connection);
    let mut links = handle.link().get().match_name(ifname.to_string()).execute();
    let link = links
        .try_next()
        .await?
        .context(format!("Interface '{}' not found", ifname))?;
    let ifindex = link.header.index;

    let gw: Ipv4Addr = gateway.parse().context("Invalid gateway address")?;

    // Build the route message manually.
    let mut route_msg = RouteMessage::default();
    route_msg.header.address_family = netlink_packet_route::AddressFamily::Inet;
    route_msg.header.destination_prefix_length = 0;
    route_msg.header.source_prefix_length = 0;
    route_msg.header.tos = 0;
    route_msg.header.table = RouteHeader::RT_TABLE_MAIN;
    route_msg.header.protocol = RouteProtocol::Static;
    route_msg.header.scope = RouteScope::Link; // RT_SCOPE_LINK — onlink semantics
    route_msg.header.kind = RouteType::Unicast;
    route_msg.header.flags = Vec::new();

    // Destination prefix (0.0.0.0/0)
    route_msg
        .attributes
        .push(RouteAttribute::Destination(RouteAddress::Inet(
            Ipv4Addr::new(0, 0, 0, 0),
        )));

    // Encode the single nexthop with RTNH_F_ONLINK via RTA_MULTIPATH.
    // iproute2 uses RTA_NEXTHOP (type 3) for single-path routes with flags,
    // but netlink-packet-route 0.19 does not expose a typed RTA_NEXTHOP
    // variant.  The kernel accepts RTA_MULTIPATH with one element and
    // treats it identically for non-ECMP routes.
    let mut nh = RouteNextHop::default();
    nh.flags = vec![RouteNextHopFlag::Onlink];
    nh.hops = 0;
    nh.interface_index = ifindex;
    nh.attributes = vec![RouteAttribute::Gateway(RouteAddress::Inet(gw))];
    route_msg
        .attributes
        .push(RouteAttribute::MultiPath(vec![nh]));

    // Build netlink envelope
    let mut header = NetlinkHeader::default();
    header.sequence_number = 1;
    header.flags = NLM_F_REQUEST | NLM_F_ACK | NLM_F_CREATE | NLM_F_REPLACE;
    let mut nl_msg = NetlinkMessage::new(
        header,
        NetlinkPayload::from(RouteNetlinkMessage::NewRoute(route_msg)),
    );
    nl_msg.finalize();

    let socket = Socket::new(NETLINK_ROUTE).context("open NETLINK_ROUTE socket")?;
    let mut buf = vec![0u8; nl_msg.buffer_len()];
    nl_msg.serialize(&mut buf);
    socket.send(&buf, 0).context("send RTM_NEWROUTE")?;

    // Wait for ACK (or error)
    let mut recv_buf = vec![0u8; 4096];
    let len = socket.recv(&mut recv_buf, 0).context("recv netlink ACK")?;
    recv_buf.truncate(len);

    let ack_msg: NetlinkMessage<RouteNetlinkMessage> =
        NetlinkMessage::deserialize(&recv_buf).context("deserialize netlink ACK")?;
    if let NetlinkPayload::Error(e) = ack_msg.payload {
        if e.code.is_some() {
            anyhow::bail!(
                "netlink RTM_NEWROUTE failed for default via {} dev {} onlink: {:?}",
                gateway,
                ifname,
                e,
            );
        }
    }

    Ok(())
}

/// Set MAC address on interface
pub async fn set_mac_address(ifname: &str, mac: &str) -> Result<()> {
    let (connection, handle, _) = new_connection()?;
    tokio::spawn(connection);

    // Find interface by name
    let mut links = handle.link().get().match_name(ifname.to_string()).execute();
    let link = links
        .try_next()
        .await?
        .context(format!("Interface '{}' not found", ifname))?;

    let ifindex = link.header.index;

    // Parse MAC address "aa:bb:cc:dd:ee:ff" -> [u8; 6]
    let mac_bytes: Vec<u8> = mac
        .split(':')
        .map(|byte| u8::from_str_radix(byte, 16).context("Invalid MAC address byte"))
        .collect::<Result<Vec<u8>>>()?;

    if mac_bytes.len() != 6 {
        return Err(anyhow::anyhow!(
            "Invalid MAC address '{}': expected 6 octets",
            mac
        ));
    }

    // Set MAC address
    handle
        .link()
        .set(ifindex)
        .address(mac_bytes)
        .execute()
        .await
        .context("Failed to set MAC address")?;

    Ok(())
}

/// Delete default route
pub async fn del_default_route() -> Result<()> {
    let (connection, handle, _) = new_connection()?;
    tokio::spawn(connection);

    // Get all routes
    let mut routes = handle.route().get(IpVersion::V4).execute();

    while let Some(route) = routes.try_next().await? {
        // Check if this is a default route (destination 0.0.0.0/0)
        if route.header.destination_prefix_length == 0 {
            // Delete this route
            if let Err(e) = handle.route().del(route).execute().await {
                log::warn!("Failed to delete default route: {}", e);
            }
        }
    }

    Ok(())
}

/// List IPv4 routes for a given interface (by name)
pub async fn list_routes_for_interface(ifname: &str) -> Result<Vec<serde_json::Value>> {
    let (connection, handle, _) = new_connection()?;
    tokio::spawn(connection);
    let mut links = handle.link().get().match_name(ifname.to_string()).execute();
    let link = links
        .try_next()
        .await?
        .context(format!("Interface '{ifname}' not found"))?;
    let mut routes = handle.route().get(IpVersion::V4).execute();
    let mut output = Vec::new();
    while let Some(route) = routes.try_next().await? {
        let oif = route.attributes.iter().find_map(|a| {
            if let RouteAttribute::Oif(i) = a {
                Some(*i)
            } else {
                None
            }
        });
        if oif != Some(link.header.index) {
            continue;
        }
        let destination = route
            .attributes
            .iter()
            .find_map(|a| match a {
                RouteAttribute::Destination(RouteAddress::Inet(ip)) => Some(ip.to_string()),
                _ => None,
            })
            .unwrap_or_else(|| "0.0.0.0".to_string());
        let gateway = route.attributes.iter().find_map(|a| match a {
            RouteAttribute::Gateway(RouteAddress::Inet(ip)) => Some(ip.to_string()),
            _ => None,
        });
        let table = route
            .attributes
            .iter()
            .find_map(|a| {
                if let RouteAttribute::Table(i) = a {
                    Some(*i)
                } else {
                    None
                }
            })
            .unwrap_or(route.header.table as u32);
        output.push(serde_json::json!({"destination": format!("{}/{}", destination, route.header.destination_prefix_length), "gateway": gateway, "interface": ifname, "table": table}));
    }
    Ok(output)
}

/// List all veth interfaces (simplified implementation)
pub async fn list_veth_interfaces() -> Result<Vec<String>> {
    let (connection, handle, _) = new_connection()?;
    tokio::spawn(connection);
    let mut links = handle.link().get().execute();
    let mut output = Vec::new();
    while let Some(link) = links.try_next().await? {
        let is_veth = link.attributes.iter().any(|attr| matches!(attr, LinkAttribute::LinkInfo(infos) if infos.iter().any(|info| matches!(info, netlink_packet_route::link::LinkInfo::Kind(netlink_packet_route::link::InfoKind::Veth)))));
        if is_veth {
            if let Some(name) = link.attributes.iter().find_map(|attr| {
                if let LinkAttribute::IfName(name) = attr {
                    Some(name.clone())
                } else {
                    None
                }
            }) {
                output.push(name);
            }
        }
    }
    output.sort();
    Ok(output)
}

/// Rename network interface
pub async fn link_set_name(old_name: &str, new_name: &str) -> Result<()> {
    let (connection, handle, _) = new_connection()?;
    tokio::spawn(connection);

    // Find interface by current name
    let mut links = handle
        .link()
        .get()
        .match_name(old_name.to_string())
        .execute();
    let link = links
        .try_next()
        .await?
        .context(format!("Interface '{}' not found", old_name))?;

    let ifindex = link.header.index;

    // Set new name
    handle
        .link()
        .set(ifindex)
        .name(new_name.to_string())
        .execute()
        .await
        .context(format!("Failed to rename {} to {}", old_name, new_name))?;

    Ok(())
}

/// Create a kernel link using native RTM_NEWLINK.
pub async fn add_link(name: &str, link_type: &str, peer_name: Option<&str>) -> Result<()> {
    anyhow::ensure!(
        !name.is_empty() && name.len() <= 15,
        "invalid interface name"
    );
    let (connection, handle, _) = new_connection()?;
    tokio::spawn(connection);
    let request = handle.link().add();
    match link_type {
        "dummy" => request.dummy(name.to_string()).execute().await?,
        "bridge" => request.bridge(name.to_string()).execute().await?,
        "veth" => {
            let peer_name = peer_name.context("veth requires peer name")?;
            request
                .veth(name.to_string(), peer_name.to_string())
                .execute()
                .await?
        }
        other => anyhow::bail!("unsupported native link type '{other}'"),
    }
    Ok(())
}

/// Delete a kernel link by its interface name using native RTM_DELLINK.
pub async fn del_link(name: &str) -> Result<()> {
    let (connection, handle, _) = new_connection()?;
    tokio::spawn(connection);
    let mut links = handle.link().get().match_name(name.to_string()).execute();
    let link = links
        .try_next()
        .await?
        .context(format!("Interface '{name}' not found"))?;
    handle.link().del(link.header.index).execute().await?;
    Ok(())
}

fn ipv4_route(value: &str) -> Result<(Ipv4Addr, u8)> {
    let (address, prefix) = value
        .split_once('/')
        .context("route must use CIDR notation")?;
    let address = address.parse().context("invalid IPv4 route address")?;
    let prefix = prefix.parse().context("invalid route prefix")?;
    anyhow::ensure!(prefix <= 32, "IPv4 route prefix must be <= 32");
    Ok((address, prefix))
}

fn route_matches(
    route: &RouteMessage,
    wanted: Ipv4Addr,
    prefix: u8,
    gateway: Option<Ipv4Addr>,
    ifindex: Option<u32>,
    table: u32,
) -> bool {
    let actual_table = route
        .attributes
        .iter()
        .find_map(|attr| {
            if let RouteAttribute::Table(value) = attr {
                Some(*value)
            } else {
                None
            }
        })
        .unwrap_or(route.header.table as u32);
    let actual_destination = route
        .attributes
        .iter()
        .find_map(|attr| match attr {
            RouteAttribute::Destination(RouteAddress::Inet(addr)) => Some(*addr),
            _ => None,
        })
        .unwrap_or(Ipv4Addr::UNSPECIFIED);
    let gateway_ok = gateway.map(|wanted| route.attributes.iter().any(|attr| matches!(attr, RouteAttribute::Gateway(RouteAddress::Inet(addr)) if *addr == wanted))).unwrap_or(true);
    let interface_ok = ifindex
        .map(|wanted| {
            route
                .attributes
                .iter()
                .any(|attr| matches!(attr, RouteAttribute::Oif(value) if *value == wanted))
        })
        .unwrap_or(true);
    actual_table == table
        && route.header.destination_prefix_length == prefix
        && actual_destination == wanted
        && gateway_ok
        && interface_ok
}

/// Add an IPv4 route through the native route netlink API.
pub async fn add_route(
    destination: &str,
    gateway: &str,
    interface: Option<&str>,
    table: u32,
) -> Result<()> {
    let (destination, prefix) = ipv4_route(destination)?;
    let gateway: Ipv4Addr = gateway.parse().context("invalid IPv4 route gateway")?;
    let (connection, handle, _) = new_connection()?;
    tokio::spawn(connection);
    let mut request = handle
        .route()
        .add()
        .v4()
        .destination_prefix(destination, prefix)
        .gateway(gateway)
        .table_id(table);
    if let Some(name) = interface {
        let mut links = handle.link().get().match_name(name.to_string()).execute();
        let link = links
            .try_next()
            .await?
            .context(format!("Interface '{name}' not found"))?;
        request = request.output_interface(link.header.index);
    }
    request.execute().await?;
    Ok(())
}

/// Atomically replace the main-table IPv4 default route.
pub async fn replace_default_route(ifname: &str, gateway: &str) -> Result<()> {
    let gateway: Ipv4Addr = gateway.parse().context("invalid IPv4 route gateway")?;
    let (connection, handle, _) = new_connection()?;
    tokio::spawn(connection);
    let mut links = handle.link().get().match_name(ifname.to_string()).execute();
    let link = links
        .try_next()
        .await?
        .context(format!("Interface '{ifname}' not found"))?;
    handle
        .route()
        .add()
        .v4()
        .destination_prefix(Ipv4Addr::UNSPECIFIED, 0)
        .gateway(gateway)
        .output_interface(link.header.index)
        .table_id(254)
        .replace()
        .execute()
        .await?;
    Ok(())
}

/// Return whether the main-table IPv4 default route exactly matches an
/// interface and gateway. This is used to verify mutations rather than treating
/// an acknowledged RTM_NEWROUTE request as proof of resulting state.
pub async fn default_route_matches(ifname: &str, gateway: &str) -> Result<bool> {
    let wanted_gateway: Ipv4Addr = gateway.parse().context("invalid IPv4 route gateway")?;
    let (connection, handle, _) = new_connection()?;
    tokio::spawn(connection);
    let mut links = handle.link().get().match_name(ifname.to_string()).execute();
    let link = links
        .try_next()
        .await?
        .context(format!("Interface '{ifname}' not found"))?;
    let mut routes = handle.route().get(IpVersion::V4).execute();
    while let Some(route) = routes.try_next().await? {
        if route_matches(
            &route,
            Ipv4Addr::UNSPECIFIED,
            0,
            Some(wanted_gateway),
            Some(link.header.index),
            254,
        ) {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Delete an IPv4 route matching a destination prefix and gateway.
pub async fn del_route(
    destination: &str,
    gateway: Option<&str>,
    interface: Option<&str>,
    table: u32,
) -> Result<()> {
    let (wanted, prefix) = ipv4_route(destination)?;
    let (connection, handle, _) = new_connection()?;
    tokio::spawn(connection);
    let mut routes = handle.route().get(IpVersion::V4).execute();
    let wanted_gateway = gateway
        .map(|value| value.parse::<Ipv4Addr>())
        .transpose()
        .context("invalid IPv4 route gateway")?;
    let interface_index = if let Some(name) = interface {
        let mut links = handle.link().get().match_name(name.to_string()).execute();
        Some(
            links
                .try_next()
                .await?
                .context(format!("Interface '{name}' not found"))?
                .header
                .index,
        )
    } else {
        None
    };
    let mut matches = Vec::new();
    while let Some(route) = routes.try_next().await? {
        if route_matches(
            &route,
            wanted,
            prefix,
            wanted_gateway,
            interface_index,
            table,
        ) {
            matches.push(route);
        }
    }
    anyhow::ensure!(
        matches.len() == 1,
        "route selection matched {} routes; specify table, gateway, and interface",
        matches.len()
    );
    handle
        .route()
        .del(matches.into_iter().next().unwrap())
        .execute()
        .await?;
    Ok(())
}

/// Add an IPv4 policy rule matching a destination CIDR. The compact plugin
/// contract uses the main table (254), matching the kernel's normal table.
pub async fn add_rule(destination: &str, table: u32, priority: Option<u32>) -> Result<()> {
    let (destination, prefix) = ipv4_route(destination)?;
    let (connection, handle, _) = new_connection()?;
    tokio::spawn(connection);
    let mut request = handle
        .rule()
        .add()
        .v4()
        .destination_prefix(destination, prefix)
        .table_id(table);
    if let Some(priority) = priority {
        request = request.priority(priority);
    }
    request.execute().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_gateway_serializes_typed_ipv4_and_ipv6() {
        let cases = [
            (
                RouteAddress::Inet(Ipv4Addr::new(192, 0, 2, 254)),
                "192.0.2.254",
            ),
            (
                RouteAddress::Inet6("2001:db8::1".parse().unwrap()),
                "2001:db8::1",
            ),
        ];
        for (gateway, expected) in cases {
            let mut route = RouteMessage::default();
            route.attributes.push(RouteAttribute::Gateway(gateway));
            assert_eq!(route_gateway(&route).unwrap().as_deref(), Some(expected));
        }
    }

    #[test]
    fn route_gateway_preserves_missing_gateway() {
        let mut route = RouteMessage::default();
        route.attributes.push(RouteAttribute::Oif(4));
        assert_eq!(route_gateway(&route).unwrap(), None);
    }

    #[test]
    fn route_gateway_rejects_unsupported_family() {
        let mut route = RouteMessage::default();
        route
            .attributes
            .push(RouteAttribute::Gateway(RouteAddress::Other(vec![0x42])));
        assert_eq!(
            route_gateway(&route).unwrap_err().to_string(),
            "unsupported route gateway address family"
        );
    }

    #[test]
    fn route_selector_handles_default_and_disambiguators() {
        let mut route = RouteMessage::default();
        route.header.table = 254;
        route.header.destination_prefix_length = 0;
        route
            .attributes
            .push(RouteAttribute::Gateway(RouteAddress::Inet(Ipv4Addr::new(
                10, 0, 0, 1,
            ))));
        route.attributes.push(RouteAttribute::Oif(7));
        assert!(route_matches(
            &route,
            Ipv4Addr::UNSPECIFIED,
            0,
            Some(Ipv4Addr::new(10, 0, 0, 1)),
            Some(7),
            254
        ));
        assert!(!route_matches(
            &route,
            Ipv4Addr::UNSPECIFIED,
            0,
            Some(Ipv4Addr::new(10, 0, 0, 2)),
            Some(7),
            254
        ));
        assert!(!route_matches(
            &route,
            Ipv4Addr::UNSPECIFIED,
            0,
            None,
            Some(7),
            100
        ));
    }

    #[test]
    fn route_selector_requires_unique_match_before_delete() {
        let mut first = RouteMessage::default();
        first.header.table = 254;
        first.header.destination_prefix_length = 24;
        first
            .attributes
            .push(RouteAttribute::Destination(RouteAddress::Inet(
                Ipv4Addr::new(10, 0, 0, 0),
            )));
        let mut second = first.clone();
        second.attributes.push(RouteAttribute::Oif(9));
        let candidates = [first, second]
            .iter()
            .filter(|route| route_matches(route, Ipv4Addr::new(10, 0, 0, 0), 24, None, None, 254))
            .count();
        assert_eq!(candidates, 2);
    }
}
