# Network Address & Protocol Table

> **STALE for the 2026-07 Artix/runit host.** Addresses and interfaces below describe an older layout (ens3, incusbr0, 10.88.88.x).  
> **Current live inventory:** [operations/host-socket-topology-live.md](src/operations/host-socket-topology-live.md)  
> **Ops summary:** [src/operations/network.md](src/operations/network.md)

## Historical table (previous host — do not use for runbooks)

| Interface/Service | IPv4/CIDR | Type/Protocol | Port/Binding | Purpose |
| :--- | :--- | :--- | :--- | :--- |
| **ens3** | 148.113.204.83 | Physical / DHCP | N/A | Host WAN Gateway |
| **ovsbr0** | 10.88.88.1/24 | OVS Bridge (Internal) | N/A | Switching Fabric |
| **incusbr0** | 10.149.181.1/24 | Linux Bridge | N/A | Container Fabric |
| **wgcf** | 172.16.0.2 | WireGuard / WARP | N/A | Privacy Tunnel Egress |
| **priv_xray** | 15.235.37.41/32 | Internal OVS Port | N/A | Xray Identity Carrier |
| **privacy-xray-ingress**| 10.149.181.167 | Container (Incus) | N/A | WG Server + Xray Injector |
| **xray-server** | 10.149.181.100 | Container (Incus) | N/A | Egress Point |
| **gRPC Bridge** | 10.200.0.2 | gRPC Stream | 50051 (Listen) | Identity-stamped Transport |
| **OVS Controller** | 10.88.88.1 | OpenFlow | 6653 | Fabric Policy Controller |
| **op-mcp** | 127.0.0.1 | gRPC/HTTP/WS | 50051 | MCP Service Ingress |
| **op-chat** | 0.0.0.0 | gRPC | 50052 | Orchestration Ingress |
| **Agent Pool** | 127.0.0.1 | gRPC | 50051-50060 | Specialized Agent Transport |

## Protocol & Header Specifications

*   **Transport Fabric:** gRPC (over HTTP/2 / TCP)
*   **Identity Pinning:** `X-Ghostbridge-Footprint` (Hex-encoded 32-byte hash)
*   **Trace Context:** `X-Ghostbridge-Trace-ID` (UUID)
*   **Control Plane:** JSON-RPC 2.0 over Unix Sockets (`/var/run/openvswitch/db.sock`)
*   **Identity State:** Per-session `identity_sled` projection backed by durable Cozo records
*   **Fabric Policy:** OpenFlow 1.3 (via `tcp:10.88.88.1:6653`)
*   **Privacy:** DNS-over-HTTPS (DoH) to `nextdns.io` via Xray `dns-out` tagging
