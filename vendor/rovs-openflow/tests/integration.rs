//! Integration tests for rovs-openflow.
//!
//! These tests require a running OVS with ovs-vswitchd and OpenFlow enabled.
//! Set the `OPENFLOW_ADDR` environment variable to the OpenFlow address
//! (e.g., `tcp:127.0.0.1:6653`).
//!
//! Run these tests with:
//! ```bash
//! ./scripts/test-with-ovs.sh openflow
//! ```
//!
//! Or manually:
//! ```bash
//! podman run --rm -d --privileged -p 6640:6640 -p 6653:6653 --name rovs-ovsdb rovs-ovsdb
//! OPENFLOW_ADDR=tcp:127.0.0.1:6653 cargo test -p rovs-openflow -- --ignored
//! ```

use rovs_openflow::{nxm, ActionList, Flow, Match, NxLearn, VConn, CT_COMMIT};
use rovs_transport::Address;

fn get_openflow_addr() -> Option<Address> {
    std::env::var("OPENFLOW_ADDR")
        .ok()
        .and_then(|s| s.parse().ok())
}

#[tokio::test]
#[ignore = "requires ovs-vswitchd"]
async fn connect_and_handshake() {
    let addr = get_openflow_addr().expect("OPENFLOW_ADDR not set");
    let conn = VConn::connect(&addr).await.expect("Failed to connect");

    // Verify we negotiated a version
    let version = conn.version();
    println!("Negotiated OpenFlow version: {:?}", version);
}

#[tokio::test]
#[ignore = "requires ovs-vswitchd"]
async fn echo_request() {
    let addr = get_openflow_addr().expect("OPENFLOW_ADDR not set");
    let mut conn = VConn::connect(&addr).await.expect("Failed to connect");

    // Send echo request
    conn.echo().await.expect("Echo failed");
}

#[tokio::test]
#[ignore = "requires ovs-vswitchd"]
async fn barrier_request() {
    let addr = get_openflow_addr().expect("OPENFLOW_ADDR not set");
    let mut conn = VConn::connect(&addr).await.expect("Failed to connect");

    // Send barrier request
    conn.barrier().await.expect("Barrier failed");
}

#[tokio::test]
#[ignore = "requires ovs-vswitchd"]
async fn add_simple_flow() {
    let addr = get_openflow_addr().expect("OPENFLOW_ADDR not set");
    let mut conn = VConn::connect(&addr).await.expect("Failed to connect");

    // Create a simple flow: in_port=1 actions=output:2
    let flow = Flow::add()
        .priority(100)
        .match_fields(Match::new().in_port(1))
        .actions(ActionList::new().output(2));

    // Send flow and wait for confirmation
    conn.send_flow_sync(&flow)
        .await
        .expect("Failed to add flow");

    // Clean up - delete the flow
    let delete_flow = Flow::delete()
        .priority(100)
        .match_fields(Match::new().in_port(1));

    conn.send_flow_sync(&delete_flow)
        .await
        .expect("Failed to delete flow");
}

#[tokio::test]
#[ignore = "requires ovs-vswitchd"]
async fn add_flow_with_ip_match() {
    let addr = get_openflow_addr().expect("OPENFLOW_ADDR not set");
    let mut conn = VConn::connect(&addr).await.expect("Failed to connect");

    // Create flow: ip,nw_dst=10.0.0.0/24 actions=output:1
    let flow = Flow::add()
        .priority(200)
        .match_fields(
            Match::new()
                .eth_type(0x0800) // IPv4
                .ipv4_dst("10.0.0.0".parse().unwrap(), 24),
        )
        .actions(ActionList::new().output(1));

    conn.send_flow_sync(&flow)
        .await
        .expect("Failed to add flow");

    // Clean up
    let delete_flow = Flow::delete()
        .priority(200)
        .match_fields(
            Match::new()
                .eth_type(0x0800)
                .ipv4_dst("10.0.0.0".parse().unwrap(), 24),
        );

    conn.send_flow_sync(&delete_flow)
        .await
        .expect("Failed to delete flow");
}

#[tokio::test]
#[ignore = "requires ovs-vswitchd"]
async fn add_flow_with_tcp_match() {
    let addr = get_openflow_addr().expect("OPENFLOW_ADDR not set");
    let mut conn = VConn::connect(&addr).await.expect("Failed to connect");

    // Create flow: tcp,tp_dst=80 actions=output:1
    let flow = Flow::add()
        .priority(300)
        .match_fields(
            Match::new()
                .eth_type(0x0800) // IPv4
                .ip_proto(6)      // TCP
                .tcp_dst(80),
        )
        .actions(ActionList::new().output(1));

    conn.send_flow_sync(&flow)
        .await
        .expect("Failed to add flow");

    // Clean up
    let delete_flow = Flow::delete()
        .priority(300)
        .match_fields(
            Match::new()
                .eth_type(0x0800)
                .ip_proto(6)
                .tcp_dst(80),
        );

    conn.send_flow_sync(&delete_flow)
        .await
        .expect("Failed to delete flow");
}

#[tokio::test]
#[ignore = "requires ovs-vswitchd"]
async fn delete_flow_by_match() {
    let addr = get_openflow_addr().expect("OPENFLOW_ADDR not set");
    let mut conn = VConn::connect(&addr).await.expect("Failed to connect");

    // Add a flow
    let flow = Flow::add()
        .priority(150)
        .match_fields(Match::new().in_port(3))
        .actions(ActionList::new().output(4));

    conn.send_flow_sync(&flow)
        .await
        .expect("Failed to add flow");

    // Delete the flow using Delete (not DeleteStrict)
    let delete_flow = Flow::delete()
        .match_fields(Match::new().in_port(3));

    conn.send_flow_sync(&delete_flow)
        .await
        .expect("Failed to delete flow");
}

#[tokio::test]
#[ignore = "requires ovs-vswitchd"]
async fn add_flow_with_vlan() {
    let addr = get_openflow_addr().expect("OPENFLOW_ADDR not set");
    let mut conn = VConn::connect(&addr).await.expect("Failed to connect");

    // Create flow with VLAN match and actions
    let flow = Flow::add()
        .priority(250)
        .match_fields(
            Match::new()
                .in_port(1)
                .vlan_vid(100), // VLAN 100
        )
        .actions(
            ActionList::new()
                .pop_vlan()
                .output(2),
        );

    conn.send_flow_sync(&flow)
        .await
        .expect("Failed to add flow");

    // Clean up
    let delete_flow = Flow::delete()
        .priority(250)
        .match_fields(
            Match::new()
                .in_port(1)
                .vlan_vid(100),
        );

    conn.send_flow_sync(&delete_flow)
        .await
        .expect("Failed to delete flow");
}

#[tokio::test]
#[ignore = "requires ovs-vswitchd"]
async fn add_flow_with_set_field() {
    let addr = get_openflow_addr().expect("OPENFLOW_ADDR not set");
    let mut conn = VConn::connect(&addr).await.expect("Failed to connect");

    // Create flow that sets destination MAC
    let flow = Flow::add()
        .priority(350)
        .match_fields(Match::new().in_port(1))
        .actions(
            ActionList::new()
                .set_eth_dst([0x00, 0x11, 0x22, 0x33, 0x44, 0x55])
                .output(2),
        );

    conn.send_flow_sync(&flow)
        .await
        .expect("Failed to add flow");

    // Clean up
    let delete_flow = Flow::delete()
        .priority(350)
        .match_fields(Match::new().in_port(1));

    conn.send_flow_sync(&delete_flow)
        .await
        .expect("Failed to delete flow");
}

#[tokio::test]
#[ignore = "requires ovs-vswitchd"]
async fn add_flow_with_dec_ttl() {
    let addr = get_openflow_addr().expect("OPENFLOW_ADDR not set");
    let mut conn = VConn::connect(&addr).await.expect("Failed to connect");

    // Create flow that decrements TTL
    let flow = Flow::add()
        .priority(400)
        .match_fields(
            Match::new()
                .in_port(1)
                .eth_type(0x0800), // IPv4
        )
        .actions(
            ActionList::new()
                .dec_ttl()
                .output(2),
        );

    conn.send_flow_sync(&flow)
        .await
        .expect("Failed to add flow");

    // Clean up
    let delete_flow = Flow::delete()
        .priority(400)
        .match_fields(
            Match::new()
                .in_port(1)
                .eth_type(0x0800),
        );

    conn.send_flow_sync(&delete_flow)
        .await
        .expect("Failed to delete flow");
}

#[tokio::test]
#[ignore = "requires ovs-vswitchd"]
async fn add_flow_with_timeout() {
    let addr = get_openflow_addr().expect("OPENFLOW_ADDR not set");
    let mut conn = VConn::connect(&addr).await.expect("Failed to connect");

    // Create flow with idle and hard timeout
    let flow = Flow::add()
        .priority(175)
        .idle_timeout(60)  // 60 seconds
        .hard_timeout(300) // 5 minutes
        .match_fields(Match::new().in_port(1))
        .actions(ActionList::new().output(2));

    conn.send_flow_sync(&flow)
        .await
        .expect("Failed to add flow");

    // Clean up
    let delete_flow = Flow::delete()
        .priority(175)
        .match_fields(Match::new().in_port(1));

    conn.send_flow_sync(&delete_flow)
        .await
        .expect("Failed to delete flow");
}

#[tokio::test]
#[ignore = "requires ovs-vswitchd"]
async fn add_flow_to_specific_table() {
    let addr = get_openflow_addr().expect("OPENFLOW_ADDR not set");
    let mut conn = VConn::connect(&addr).await.expect("Failed to connect");

    // Create flow in table 1
    let flow = Flow::add()
        .table(1)
        .priority(100)
        .match_fields(Match::new().in_port(1))
        .actions(ActionList::new().output(2));

    conn.send_flow_sync(&flow)
        .await
        .expect("Failed to add flow to table 1");

    // Clean up
    let delete_flow = Flow::delete()
        .table(1)
        .priority(100)
        .match_fields(Match::new().in_port(1));

    conn.send_flow_sync(&delete_flow)
        .await
        .expect("Failed to delete flow");
}

#[tokio::test]
#[ignore = "requires ovs-vswitchd"]
async fn multiple_flows_sequential() {
    let addr = get_openflow_addr().expect("OPENFLOW_ADDR not set");
    let mut conn = VConn::connect(&addr).await.expect("Failed to connect");

    // Add multiple flows
    for port in 1..=5u32 {
        let flow = Flow::add()
            .priority(100 + port as u16)
            .match_fields(Match::new().in_port(port))
            .actions(ActionList::new().output(port + 10));

        conn.send_flow_sync(&flow)
            .await
            .unwrap_or_else(|_| panic!("Failed to add flow for port {}", port));
    }

    // Delete all flows
    for port in 1..=5u32 {
        let delete_flow = Flow::delete()
            .priority(100 + port as u16)
            .match_fields(Match::new().in_port(port));

        conn.send_flow_sync(&delete_flow)
            .await
            .unwrap_or_else(|_| panic!("Failed to delete flow for port {}", port));
    }
}

#[tokio::test]
#[ignore = "requires ovs-vswitchd"]
async fn delete_all_flows_in_table() {
    let addr = get_openflow_addr().expect("OPENFLOW_ADDR not set");
    let mut conn = VConn::connect(&addr).await.expect("Failed to connect");

    // Add some flows to table 2
    for i in 1..=3u32 {
        let flow = Flow::add()
            .table(2)
            .priority(100 + i as u16)
            .match_fields(Match::new().in_port(i))
            .actions(ActionList::new().output(i + 10));

        conn.send_flow_sync(&flow)
            .await
            .expect("Failed to add flow");
    }

    // Delete all flows in table 2
    let delete_all = Flow::delete()
        .table(2);

    conn.send_flow_sync(&delete_all)
        .await
        .expect("Failed to delete all flows in table");
}

#[tokio::test]
#[ignore = "requires ovs-vswitchd"]
async fn add_flow_with_learn_action() {
    let addr = get_openflow_addr().expect("OPENFLOW_ADDR not set");
    let mut conn = VConn::connect(&addr).await.expect("Failed to connect");

    // Create a MAC learning flow:
    // Learns eth_src from incoming packets and installs a flow in table 10
    // that matches on eth_dst and outputs to the learned in_port
    let learn = NxLearn::new()
        .table(10)
        .priority(100)
        .idle_timeout(300)
        .hard_timeout(600);

    let flow = Flow::add()
        .table(0)
        .priority(500)
        .match_fields(Match::new().in_port(1))
        .actions(
            ActionList::new()
                .learn(learn)
                .output(2),
        );

    conn.send_flow_sync(&flow)
        .await
        .expect("Failed to add flow with learn action");

    // Clean up
    let delete_flow = Flow::delete()
        .table(0)
        .priority(500)
        .match_fields(Match::new().in_port(1));

    conn.send_flow_sync(&delete_flow)
        .await
        .expect("Failed to delete flow");
}

#[tokio::test]
#[ignore = "requires ovs-vswitchd"]
async fn add_flow_with_resubmit() {
    let addr = get_openflow_addr().expect("OPENFLOW_ADDR not set");
    let mut conn = VConn::connect(&addr).await.expect("Failed to connect");

    // Create a flow that resubmits to table 5
    let flow = Flow::add()
        .table(0)
        .priority(450)
        .match_fields(Match::new().in_port(1))
        .actions(ActionList::new().resubmit_table(5));

    conn.send_flow_sync(&flow)
        .await
        .expect("Failed to add flow with resubmit");

    // Add a flow in table 5 to complete the pipeline
    let flow_table5 = Flow::add()
        .table(5)
        .priority(100)
        .actions(ActionList::new().output(2));

    conn.send_flow_sync(&flow_table5)
        .await
        .expect("Failed to add flow in table 5");

    // Clean up both flows
    let delete_flow = Flow::delete()
        .table(0)
        .priority(450)
        .match_fields(Match::new().in_port(1));

    conn.send_flow_sync(&delete_flow)
        .await
        .expect("Failed to delete flow");

    let delete_table5 = Flow::delete().table(5);
    conn.send_flow_sync(&delete_table5)
        .await
        .expect("Failed to delete table 5 flows");
}

#[tokio::test]
#[ignore = "requires ovs-vswitchd"]
async fn add_flow_with_ct_commit() {
    let addr = get_openflow_addr().expect("OPENFLOW_ADDR not set");
    let mut conn = VConn::connect(&addr).await.expect("Failed to connect");

    // Create a flow that commits connections to connection tracking
    let flow = Flow::add()
        .table(0)
        .priority(550)
        .match_fields(
            Match::new()
                .in_port(1)
                .eth_type(0x0800), // IPv4
        )
        .actions(
            ActionList::new()
                .ct_commit(0) // zone 0
                .output(2),
        );

    conn.send_flow_sync(&flow)
        .await
        .expect("Failed to add flow with ct_commit");

    // Clean up
    let delete_flow = Flow::delete()
        .table(0)
        .priority(550)
        .match_fields(
            Match::new()
                .in_port(1)
                .eth_type(0x0800),
        );

    conn.send_flow_sync(&delete_flow)
        .await
        .expect("Failed to delete flow");
}

#[tokio::test]
#[ignore = "requires ovs-vswitchd"]
async fn add_flow_with_ct_and_recirc() {
    let addr = get_openflow_addr().expect("OPENFLOW_ADDR not set");
    let mut conn = VConn::connect(&addr).await.expect("Failed to connect");

    // Create a flow that does CT and recirculates to table 1
    let flow = Flow::add()
        .table(0)
        .priority(600)
        .match_fields(
            Match::new()
                .in_port(1)
                .eth_type(0x0800), // IPv4
        )
        .actions(ActionList::new().ct(CT_COMMIT, 100, Some(1)));

    conn.send_flow_sync(&flow)
        .await
        .expect("Failed to add flow with ct+recirc");

    // Add a flow in table 1 to handle recirculated packets
    let flow_table1 = Flow::add()
        .table(1)
        .priority(100)
        .match_fields(Match::new().eth_type(0x0800))
        .actions(ActionList::new().output(2));

    conn.send_flow_sync(&flow_table1)
        .await
        .expect("Failed to add flow in table 1");

    // Clean up
    let delete_flow = Flow::delete()
        .table(0)
        .priority(600)
        .match_fields(
            Match::new()
                .in_port(1)
                .eth_type(0x0800),
        );

    conn.send_flow_sync(&delete_flow)
        .await
        .expect("Failed to delete flow");

    let delete_table1 = Flow::delete().table(1);
    conn.send_flow_sync(&delete_table1)
        .await
        .expect("Failed to delete table 1 flows");
}

#[tokio::test]
#[ignore = "requires ovs-vswitchd"]
async fn add_flow_with_mac_translation() {
    let addr = get_openflow_addr().expect("OPENFLOW_ADDR not set");
    let mut conn = VConn::connect(&addr).await.expect("Failed to connect");

    // MAC addresses for translation
    let internal_mac = [0x02, 0x00, 0x00, 0x00, 0x00, 0x01];
    let external_mac = [0x02, 0x00, 0x00, 0x00, 0x00, 0x99];

    // Flow 1: Rewrite source MAC (internal -> external)
    let flow_src_rewrite = Flow::add()
        .table(3)
        .priority(100)
        .match_fields(
            Match::new()
                .in_port(1)
                .eth_src(internal_mac),
        )
        .actions(
            ActionList::new()
                .set_eth_src(external_mac)
                .output(2),
        );

    conn.send_flow_sync(&flow_src_rewrite)
        .await
        .expect("Failed to add source MAC rewrite flow");

    // Flow 2: Rewrite destination MAC (external -> internal)
    let flow_dst_rewrite = Flow::add()
        .table(3)
        .priority(100)
        .match_fields(
            Match::new()
                .in_port(2)
                .eth_dst(external_mac),
        )
        .actions(
            ActionList::new()
                .set_eth_dst(internal_mac)
                .output(1),
        );

    conn.send_flow_sync(&flow_dst_rewrite)
        .await
        .expect("Failed to add destination MAC rewrite flow");

    // Clean up
    let delete_flows = Flow::delete().table(3);
    conn.send_flow_sync(&delete_flows)
        .await
        .expect("Failed to delete flows");
}

#[tokio::test]
#[ignore = "requires ovs-vswitchd"]
async fn add_flow_with_arp_proxy_actions() {
    let addr = get_openflow_addr().expect("OPENFLOW_ADDR not set");
    let mut conn = VConn::connect(&addr).await.expect("Failed to connect");

    // Test NxMove and NxRegLoad actions for ARP proxy
    // This matches ARP requests and transforms them into replies
    let external_mac = [0x02, 0x00, 0x00, 0x00, 0x00, 0x99u8];
    let external_ip: u32 = 0x0a000063; // 10.0.0.99

    // Convert MAC to u64 for load_field
    let mac_u64 = ((external_mac[0] as u64) << 40)
        | ((external_mac[1] as u64) << 32)
        | ((external_mac[2] as u64) << 24)
        | ((external_mac[3] as u64) << 16)
        | ((external_mac[4] as u64) << 8)
        | (external_mac[5] as u64);

    let flow = Flow::add()
        .table(4)
        .priority(200)
        .match_fields(
            Match::new()
                .in_port(2)
                .eth_type(0x0806) // ARP
                .arp_op(1),       // ARP Request
        )
        .actions(
            ActionList::new()
                // Move ARP SHA -> ARP THA
                .move_field(nxm::ARP_SHA, nxm::ARP_THA, 48, 0, 0)
                // Move ARP SPA -> ARP TPA
                .move_field(nxm::ARP_SPA, nxm::ARP_TPA, 32, 0, 0)
                // Set ARP SHA to our MAC
                .set_arp_sha(mac_u64)
                // Set ARP SPA to our IP
                .set_arp_spa(external_ip)
                // Set ARP opcode to 2 (reply)
                .set_arp_op(2)
                // Move ETH_SRC -> ETH_DST
                .move_field(nxm::ETH_SRC, nxm::ETH_DST, 48, 0, 0)
                // Set ETH_SRC to our MAC
                .set_eth_src(external_mac)
                // Send back to input port
                .in_port(),
        );

    conn.send_flow_sync(&flow)
        .await
        .expect("Failed to add ARP proxy flow");

    // Clean up
    let delete_flows = Flow::delete().table(4);
    conn.send_flow_sync(&delete_flows)
        .await
        .expect("Failed to delete flows");
}

#[tokio::test]
#[ignore = "requires ovs-vswitchd"]
async fn ndp_proxy_flow_and_packet_out() {
    use rovs_openflow::ndp::{build_na_reply, parse_neighbor_solicitation};
    use rovs_openflow::{PacketOut, OFPP_CONTROLLER};
    use std::net::Ipv6Addr;

    let addr = get_openflow_addr().expect("OPENFLOW_ADDR not set");
    let mut conn = VConn::connect(&addr).await.expect("Failed to connect");

    // Install flow: ICMPv6 NS -> CONTROLLER
    // This is the flow pattern used for NDP proxy
    let ns_to_controller = Flow::add()
        .table(5) // Use table 5 to avoid conflicts
        .priority(500)
        .match_fields(Match::new().icmpv6_type(135)) // Neighbor Solicitation
        .actions(ActionList::new().controller(0xffff)); // Full packet

    conn.send_flow_sync(&ns_to_controller)
        .await
        .expect("Failed to install NDP flow");

    // Build a test Neighbor Solicitation packet
    let src_mac = [0x02, 0x00, 0x00, 0x00, 0x01, 0x00u8];
    let src_ipv6: Ipv6Addr = "fe80::1".parse().unwrap();
    let target_ipv6: Ipv6Addr = "fd00::100".parse().unwrap();
    let dst_mac = [0x33, 0x33, 0xff, 0x00, 0x01, 0x00u8];
    let dst_ipv6: Ipv6Addr = "ff02::1:ff00:100".parse().unwrap();

    let ns_packet = build_test_ns_packet(
        src_mac, dst_mac, src_ipv6, dst_ipv6, target_ipv6, Some(src_mac),
    );

    // Verify NS packet parsing works
    let parsed = parse_neighbor_solicitation(&ns_packet);
    assert!(parsed.is_some(), "Should parse test NS packet");

    let (eth, ipv6, ns) = parsed.unwrap();
    assert_eq!(eth.src_mac, src_mac);
    assert_eq!(ipv6.src_addr, src_ipv6);
    assert_eq!(ns.target_addr, target_ipv6);
    assert_eq!(ns.source_ll_addr, Some(src_mac));

    // Build NA reply
    let our_mac = [0x02, 0x00, 0x00, 0x00, 0x99, 0x00u8];
    let na_packet = build_na_reply(&eth, &ipv6, &ns, our_mac, target_ipv6);

    // Verify NA packet size (14 eth + 40 ipv6 + 32 icmpv6 = 86)
    assert_eq!(na_packet.len(), 86, "NA packet should be 86 bytes");

    // Verify NA packet starts with correct Ethernet header
    assert_eq!(&na_packet[0..6], &src_mac); // dst = original src
    assert_eq!(&na_packet[6..12], &our_mac); // src = our MAC
    assert_eq!(&na_packet[12..14], &[0x86, 0xdd]); // IPv6

    // Verify ICMPv6 type is Neighbor Advertisement (136)
    let icmpv6_offset = 14 + 40; // eth + ipv6
    assert_eq!(na_packet[icmpv6_offset], 136, "Should be NA type");

    // Send NA via PacketOut (verifies PacketOut encoding works)
    let na_out = PacketOut::new()
        .in_port(OFPP_CONTROLLER)
        .actions(ActionList::new().output(1)) // Output to port 1
        .data(na_packet);

    conn.send_packet_out(&na_out)
        .await
        .expect("Failed to send NA PacketOut");

    // Clean up
    let delete_flows = Flow::delete().table(5);
    conn.send_flow_sync(&delete_flows)
        .await
        .expect("Failed to delete flows");
}

/// Build a test Neighbor Solicitation packet.
fn build_test_ns_packet(
    src_mac: [u8; 6],
    dst_mac: [u8; 6],
    src_ipv6: std::net::Ipv6Addr,
    dst_ipv6: std::net::Ipv6Addr,
    target_ipv6: std::net::Ipv6Addr,
    source_ll_addr: Option<[u8; 6]>,
) -> Vec<u8> {
    use rovs_openflow::ndp::{icmpv6_checksum, ICMPV6_NEIGHBOR_SOLICITATION, NDP_OPT_SOURCE_LL_ADDR};

    let mut packet = Vec::with_capacity(86);

    // Ethernet header
    packet.extend_from_slice(&dst_mac);
    packet.extend_from_slice(&src_mac);
    packet.extend_from_slice(&0x86ddu16.to_be_bytes()); // IPv6

    // Build ICMPv6 NS first to get length for IPv6 header
    let mut icmpv6 = Vec::new();
    icmpv6.push(ICMPV6_NEIGHBOR_SOLICITATION); // Type
    icmpv6.push(0); // Code
    icmpv6.push(0); // Checksum placeholder
    icmpv6.push(0);
    icmpv6.extend_from_slice(&[0u8; 4]); // Reserved
    icmpv6.extend_from_slice(&target_ipv6.octets()); // Target

    // Source link-layer address option
    if let Some(mac) = source_ll_addr {
        icmpv6.push(NDP_OPT_SOURCE_LL_ADDR);
        icmpv6.push(1); // Length in 8-byte units
        icmpv6.extend_from_slice(&mac);
    }

    // Calculate checksum
    let checksum = icmpv6_checksum(&src_ipv6, &dst_ipv6, &icmpv6);
    icmpv6[2] = (checksum >> 8) as u8;
    icmpv6[3] = checksum as u8;

    // IPv6 header
    packet.push(0x60); // Version 6, TC high nibble = 0
    packet.push(0x00); // TC low nibble + flow label high
    packet.push(0x00); // Flow label
    packet.push(0x00); // Flow label low
    packet.extend_from_slice(&(icmpv6.len() as u16).to_be_bytes()); // Payload length
    packet.push(58); // Next header = ICMPv6
    packet.push(255); // Hop limit
    packet.extend_from_slice(&src_ipv6.octets());
    packet.extend_from_slice(&dst_ipv6.octets());

    // ICMPv6 payload
    packet.extend_from_slice(&icmpv6);

    packet
}
