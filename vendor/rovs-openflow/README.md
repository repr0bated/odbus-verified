# rovs

Rust Open vSwitch library - a complete Rust replacement for Python OVS bindings.

Provides async, type-safe APIs for OVSDB and OpenFlow protocols with Nicira extension support.

## Crate Structure

```
rovs-transport  → Network layer (Unix/TCP/TLS)
rovs-jsonrpc    → JSON-RPC 1.0 (brace-depth parsing)
rovs-ovsdb      → OVSDB client, IDL, transactions
rovs-openflow   → OpenFlow 1.3 + Nicira extensions
rovs-types      → Shared types (Atom, MacAddr)
rovs-client     → High-level API
rovs-ext        → Flow templates, topology builders, controller framework
```

## Quick Start

```bash
cargo add rovs-ovsdb   # OVSDB client
cargo add rovs-openflow # OpenFlow protocol
cargo add rovs-ext      # High-level abstractions
```

### OVSDB Transaction

```rust
use rovs_ovsdb::{Client, Transaction};

let mut client = Client::connect("unix:/tmp/ovs-test/db.sock").await?;

let mut txn = Transaction::new("Open_vSwitch");
txn.create_bridge("br0");
txn.add_internal_port("br0", "vport0");
client.commit(&mut txn).await?;
```

### OpenFlow with Nicira Extensions

```rust
use rovs_openflow::{VConn, Flow, Match, ActionList, OxmHeader};

let mut conn = VConn::connect(&addr).await?;

let flow = Flow::add()
    .table(0).priority(100)
    .match_fields(Match::new().eth_type(0x0800).in_port(1))
    .actions(ActionList::new()
        .nx_reg_load(OxmHeader::EthSrc, mac_bytes)
        .output(2));
conn.send_flow_sync(&flow).await?;
```

### NAT Gateway

```rust
use rovs_ext::flows::{SnatConfig, SnatGateway};
use std::net::Ipv4Addr;

let snat = SnatGateway::new(
    SnatConfig::new(Ipv4Addr::new(203, 0, 113, 1), 1, 2)
        .zone(1)
        .port_range(10000, 65000)
);
snat.install(&mut conn, 0, 100).await?;
```

## Features

| Feature | Status |
|---------|--------|
| Transport (Unix/TCP/TLS) | Complete |
| JSON-RPC connection | Complete |
| OVSDB client & IDL | Complete |
| OpenFlow 1.3 protocol | Complete |
| Nicira extensions (NxLearn, ct, NAT) | Complete |
| Controller framework | Complete |
| Flow templates (SNAT, DNAT, MAC NAT, ARP/NDP proxy) | Complete |
| Topology builders | Complete |

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT) at your option.
