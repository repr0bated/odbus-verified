---
exported: 2026-10-15T22:45:11.211Z
source: NotebookLM
type: chat
title: "Thoughts expand_moreHere is a detailed breakdown o..."
---

# Thoughts expand_moreHere is a detailed breakdown o...

Exported at: 10/15/2026, 6:45:11 PM

---

Thoughts

Here is a detailed breakdown of **item #2 (the network architecture refactoring, consolidation into crates/op-network, OVS bridge configuration, GRE elimination, and encrypted transport)** grounded in the source documentation:

* * *

### **1\. Core Architectural Strategy for Network Consolidation**

-   **Consolidation Target**: Unify all socket listeners, network transport abstractions, identity metadata injectors, and door forwarders into `crates/op-network` within the `odbus-verified` monorepo workspace\[1\].
-   **Decoy-Host Separation**: Server-side WireGuard interfaces are eliminated from the main server host\[1\]\[2\]. Public WireGuard and browser TLS (`10.0.0.1`) terminate exclusively on the external decoy VPS (`129.80.64.127`)\[4\].
-   **Single gRPC Ingress Door**: All host-bound gRPC traffic is consolidated onto a single listener at `10.0.0.2:8090`, using tonic with the internal `svc0` CA certificate from the `op-dbus` local CA\[2\]. Legacy listeners on `127.0.0.1:8090` and `10.0.0.3:8090` are retired\[2\].

* * *

### **2\. Open vSwitch Bridge Fabric (ovsbr0)**

The `ovsbr0` Open vSwitch bridge provides the host-side layer-2/layer-3 virtual network fabric\[7\]:

-   `10.200.0.1/24`: Assigned to `ovsbr0` as the main bridge address for OpenFlow tag-based packet isolation across container ports\[7\].
-   `10.0.0.3/32`: Aliased on `ovsbr0` beside `10.200.0.1` to expose the EMQX MQTT broker service to internal containers\[7\].
-   `10.0.0.2/24`: Assigned to the `3tched` interface connected to the `ovsbr0` fabric for `op-grpc-bridge` door listener bindings\[8\]\[11\].
-   `10.0.0.4/32`: Serves as `op-web`'s page address (`http://10.0.0.4:8080` and `https://10.0.0.4:443`), operating as a plain HTTP/HTTPS web dashboard outside the internal TLS zone\[7\].

* * *

### **3\. Decoy Offloading & Identity Assertions (OIA1 / SID1)**

-   `OIA1 (Oracle Assertion)`: Signed proof generated on the decoy verifying the WireGuard peer identity\[2\].
-   `SID1 (Sealed Session Identity)`: Sealed human credential blob for session anchoring\[2\].
-   **Door Interceptor**: Arriving requests at `10.0.0.2:8090` pass through the gRPC interceptor (`anchor_verified_human_session` and `insert_identity_metadata` in `crates/op-grpc-bridge/src/interceptor.rs`)\[2\]. The interceptor validates `OIA1`/`SID1` metadata headers before forwarding requests to `MutationEngine`\[2\].

* * *

### **4\. GRE Removal & Encrypted User-Space Transport**

-   **Legacy GRE Setup**: The legacy setup used unencrypted point-to-point GRE tunnels (`gre-decoy` and `gre-host`) between host `188.68.58.237` and decoy `129.80.64.127`, utilizing inner IPs `192.168.255.1/30` and `10.0.0.2/32`\[5\]. Because GRE is unencrypted, plain HTTP traffic across the tunnel was readable on the wire\[2\].
-   **Purge Steps**:
    1.  Remove the `deploy/runit/gre-decoy/run` service script\[5\].
    2.  Remove decoy GRE routing scripts (`/usr/local/sbin/gre-host-up`)\[15\].
    3.  Add filter rules in `nftables` blocking raw IP protocol 47 (GRE)\[15\].
-   **Encrypted User-Space Transport Replacement**:
    -   **MASQUE**: HTTP/3 QUIC CONNECT-UDP/IP stream proxying between decoy and host\[1\].
    -   **Cloudflare WARP**: Tunneling user traffic from the decoy directly to the host interceptor\[1\].
    -   **Reverse SSH**: `decoy-admin-reverse-ssh` running over port 2222 (`-R 127.0.0.1:2222:127.0.0.1:22`) for persistent administrative access\[4\]\[21\].

* * *

### **5\. op-web In-Process Door Proxying**

-   `door\_proxy.rs`: `op-web` includes a lightweight, in-process door connector (`crates/op-web/src/door_proxy.rs`)\[14\]\[22\].
-   **gRPC-Web Forwarding**: gRPC-Web calls reaching `op-web` on `:8080` or `:443` are forwarded unchanged to `https://10.0.0.2:8090` using the internal `tonic-svc0` CA certificate\[14\]\[22\].
-   **Passthrough Behavior**: Passes all `x-opdbus-*-bin` identity headers through untouched without checking or stripping credentials, preserving strict interceptor-driven authorization\[14\]\[22\].

* * *

### **6\. Retired Legacy Components**

-   **Netmaker Mesh Stack**: Netmaker mesh adapter, `netclient`, `op-grpc-adapters` (`op.adapters.v1`), and related forwarding scripts (`fwd-8090`, `nm-identity-route`, `wgcf-egress`) were retired and purged on September 14, 2026\[1\].
-   **Standalone MCP Services**: Standalone runit services (`op-mcp-cognitive`, `op-mcp-blob-schema`, `op-mcp-agents`, `op-mcp-compact`) were retired and consolidated directly into `op-grpc-bridge`\[23\]\[24\].
-   **Container Socket "Lungs"**: Identity containers avoid binding IP addresses inside their namespaces, relying instead on mounted Unix domain sockets (`/run/opdbus/lungs.sock` or `/run/ghostbridge/container.sock`) for local IPC\[9\].

* * *

💡 Would you like me to generate a shell script or Rust unit test suite to verify the `op-network` door proxying logic?
---

## References

[1] model-claude-opus-5-5-005912be__0001__bc558d05dd6010cd.txt
[2] model-claude-opus-5-5-005912be__0003__a56c55e4535357bb.txt
[4] model-claude-opus-5-5-005912be__0001__bc558d05dd6010cd.txt
[5] model-claude-opus-5-5-005912be__0001__bc558d05dd6010cd.txt
[7] model-claude-opus-5-5-005912be__0001__bc558d05dd6010cd.txt
[8] model-claude-opus-5-5-005912be__0002__f1b8815cf231d707.txt
[9] model-claude-opus-5-5-005912be__0003__a56c55e4535357bb.txt
[11] model-claude-opus-5-5-005912be__0003__a56c55e4535357bb.txt
[14] model-claude-opus-5-5-005912be__0003__a56c55e4535357bb.txt
[15] model-claude-opus-5-5-005912be__0003__a56c55e4535357bb.txt
[21] model-claude-opus-5-5-005912be__0003__a56c55e4535357bb.txt
[22] model-claude-opus-5-5-005912be__0003__a56c55e4535357bb.txt
[23] model-claude-opus-5-5-005912be__0005__1b01eb890676614b.txt
[24] model-claude-opus-5-5-005912be__0006__1bb1d71accc2921c.txt
