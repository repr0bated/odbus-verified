# OVS Bridge Setup - Native OVSDB JSON-RPC

This guide shows how to create and manage OVS bridges using **native OVSDB JSON-RPC** without any CLI commands like `ovs-vsctl`.

## Why Native JSON-RPC?

- **No CLI dependencies**: Works without `ovs-vsctl`, `ovs-ofctl`, or any shell commands
- **Atomic transactions**: All operations are transactional and consistent
- **Programmatic**: Easy to integrate into automation and services
- **D-Bus routed**: The public Rust client uses the system D-Bus control plane rather than opening the OVSDB socket directly

## Architecture

The public API is `op_network::OvsdbClient`. It sends OVSDB JSON-RPC payloads over the system D-Bus to the `org.opdbus.rovs.jsonrpc` service, which then forwards to the OVSDB Unix socket.

```
Your Application
       ↓
  op_network::OvsdbClient
       ↓
  D-Bus (org.opdbus.rovs.jsonrpc)
       ↓
  rovs JSON-RPC daemon
       ↓
  OVSDB Unix socket
       ↓
  OVSDB Server → ovs-vswitchd → Kernel Datapath
```

For lower-level direct-socket access (e.g., the `op-ovsbr0-setup` bootstrap binary), the OVSDB socket is tried in this order:

1. `/usr/local/var/run/openvswitch/db.sock` (primary)
2. `/run/openvswitch/db.sock` (fallback)
3. `/var/run/openvswitch/db.sock` (fallback)

Application code should prefer `op_network::OvsdbClient` and the D-Bus route.

## Quick Start

### Using Rust Code

```rust
use op_network::OvsdbClient;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let client = OvsdbClient::new();
    
    // Create bridge
    client.create_bridge("br0").await?;
    
    // Add port
    client.add_port("br0", "eth1").await?;
    
    // List bridges
    let bridges = client.list_bridges().await?;
    println!("Bridges: {:?}", bridges);
    
    Ok(())
}
```

## Common Operations

### Check OVS Availability

```rust
let client = OvsdbClient::new();
match client.list_dbs().await {
    Ok(dbs) => println!("OVS is available: {:?}", dbs),
    Err(e) => eprintln!("OVS not available: {}", e),
}
```

### List All Bridges

```rust
let bridges = client.list_bridges().await?;
for bridge in bridges {
    println!("Bridge: {}", bridge);
}
```

### Get Bridge Details

```rust
let info = client.get_bridge_info("br0").await?;
println!("Bridge info: {:?}", info);
```

### List Ports on a Bridge

```rust
let ports = client.list_ports("br0").await?;
println!("Ports on br0: {:?}", ports);
```

### Delete a Bridge

```rust
client.delete_bridge("br0").await?;
```

### Monitor Database Changes

```rust
let mut monitor_rx = client.monitor_db("Open_vSwitch").await?;
while let Some(update) = monitor_rx.recv().await {
    println!("Database update: {:?}", update);
}
```

## OVSDB Transaction Format

All write operations use OVSDB transactions with these operations:

- **insert**: Create a new row in a table
- **delete**: Remove rows from a table
- **update**: Modify existing rows
- **mutate**: Modify set/map columns (add/remove elements)
- **select**: Query rows (read-only)

### Example Transaction: Create Bridge

```json
{
  "method": "transact",
  "params": [
    "Open_vSwitch",
    [
      {
        "op": "insert",
        "table": "Bridge",
        "row": {
          "name": "br0",
          "ports": ["set", [["named-uuid", "port-br0"]]]
        },
        "uuid-name": "bridge-br0"
      },
      {
        "op": "insert",
        "table": "Port",
        "row": {
          "name": "br0",
          "interfaces": ["set", [["named-uuid", "iface-br0"]]]
        },
        "uuid-name": "port-br0"
      },
      {
        "op": "insert",
        "table": "Interface",
        "row": {
          "name": "br0",
          "type": "internal"
        },
        "uuid-name": "iface-br0"
      },
      {
        "op": "mutate",
        "table": "Open_vSwitch",
        "where": [],
        "mutations": [
          ["bridges", "insert", ["set", [["named-uuid", "bridge-br0"]]]]
        ]
      }
    ]
  ],
  "id": 0
}
```

## Monitor Database Changes

```rust
let mut monitor_rx = client.monitor_db("Open_vSwitch").await?;
while let Some(update) = monitor_rx.recv().await {
    println!("Database update: {:?}", update);
}
```

> Note: `monitor_db` currently returns a channel that stays open but idle until the D-Bus notification feed from the rovs daemon is wired. It is a compatibility placeholder, not a live streaming monitor today.

## Performance Notes

- **Batch operations**: Multiple OVSDB operations can be submitted in a single transaction.
- **No shell overhead**: Avoids `ovs-vsctl`/`ovs-ofctl` subprocess invocations.
- Low-level SIMD JSON parsing and connection pooling are implementation details inside the rovs daemon and `op-jsonrpc`, not properties of the public `OvsdbClient` API.

## Security

- D-Bus method access is controlled by the D-Bus policy and the rovs service ACL.
- No shell command injection possible.
- Atomic transactions prevent partial updates.
- Audit logging via the immutable snowball.

## References

- [OVSDB RFC 7047](https://datatracker.ietf.org/doc/html/rfc7047)
- [OVS Documentation](https://docs.openvswitch.org/)
- `op-network` crate — see `crates/op-network/src/ovsdb.rs` and `crates/op-network/src/rovs_proxy.rs`

<!-- Extracted from OVS-NATIVE-SETUP.md on 2026-07-20 and corrected against the current codebase on 2026-07-20 -->
