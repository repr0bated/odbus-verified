//! OVS Bridge Setup using Native OVSDB JSON-RPC over the rovs D-Bus proxy.
//!
//! This example demonstrates creating and managing OVS bridges using pure Rust
//! via `OvsdbClient` (native OVSDB JSON-RPC routed through the rovs proxy —
//! no `ovs-vsctl` CLI commands).
//!
//! Run with: cargo run --example ovs_native_rust

use anyhow::Result;
use op_network::OvsdbClient;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    println!("=== OVS Native Bridge Setup Example ===\n");

    // Create OVSDB client (routes JSON-RPC through the rovs D-Bus proxy).
    let client = OvsdbClient::new();

    // 1. Check connectivity
    println!("1. Checking OVSDB connectivity...");
    match client.list_dbs().await {
        Ok(dbs) => {
            println!("   ✓ Connected to OVSDB");
            println!("   Available databases: {:?}\n", dbs);
        }
        Err(e) => {
            eprintln!("   ✗ Failed to connect to OVSDB: {}", e);
            eprintln!("   Is Open vSwitch running?");
            return Err(e);
        }
    }

    // 2. List existing bridges
    println!("2. Listing existing bridges...");
    let bridges = client.list_bridges().await?;
    println!("   Existing bridges: {:?}\n", bridges);

    // 3. Create a new bridge
    let bridge_name = "br-example";
    println!("3. Creating bridge '{}'...", bridge_name);

    if bridges.contains(&bridge_name.to_string()) {
        println!(
            "   Bridge '{}' already exists, skipping creation\n",
            bridge_name
        );
    } else {
        client.create_bridge(bridge_name).await?;
        println!("   ✓ Bridge '{}' created successfully\n", bridge_name);
    }

    // 4. Verify bridge creation
    println!("4. Verifying bridge creation...");
    let bridges_after = client.list_bridges().await?;
    if bridges_after.contains(&bridge_name.to_string()) {
        println!("   ✓ Bridge '{}' verified in OVSDB\n", bridge_name);
    } else {
        eprintln!("   ✗ Bridge not found after creation!");
        return Err(anyhow::anyhow!("Bridge verification failed"));
    }

    // 5. Get detailed bridge info
    println!("5. Getting bridge information...");
    match client.get_bridge_info(bridge_name).await {
        Ok(info) => {
            println!("   Bridge info:");
            println!("   {}\n", info);
        }
        Err(e) => {
            eprintln!("   Failed to get bridge info: {}", e);
        }
    }

    // 6. List ports on the bridge
    println!("6. Listing ports on bridge '{}'...", bridge_name);
    let ports = client.list_bridge_ports(bridge_name).await?;
    println!("   Ports: {:?}\n", ports);

    // 7. Add a virtual port (veth pair example)
    let port_name = "veth-example";
    println!(
        "7. Adding port '{}' to bridge '{}'...",
        port_name, bridge_name
    );

    // Note: In a real scenario, you'd create the veth pair first using rtnetlink
    // For this example, we'll just show the OVSDB operation
    match client.add_port(bridge_name, port_name).await {
        Ok(_) => {
            println!("   ✓ Port '{}' added successfully\n", port_name);

            // List ports again to verify
            let ports_after = client.list_bridge_ports(bridge_name).await?;
            println!("   Updated ports: {:?}\n", ports_after);
        }
        Err(e) => {
            println!(
                "   Note: Port addition may fail if interface doesn't exist: {}\n",
                e
            );
        }
    }

    // 8. Demonstrate monitoring (optional - commented out as it's blocking)
    // println!("8. Setting up database monitoring...");
    // let mut monitor_rx = client.monitor_db("Open_vSwitch").await?;
    // println!("   Monitoring started. Press Ctrl+C to stop.");
    //
    // while let Some(update) = monitor_rx.recv().await {
    //     println!("   Update: {:?}", update);
    // }

    println!("=== Example Complete ===\n");
    println!("Summary:");
    println!(
        "  - Bridge '{}' created via native OVSDB JSON-RPC",
        bridge_name
    );
    println!("  - No CLI commands (ovs-vsctl) were used");
    println!("  - All operations performed over Unix socket");
    println!("\nTo clean up, run:");
    println!("  cargo run --example ovs_native_rust -- --cleanup");

    Ok(())
}
