# Privacy Network Architecture

## Overview
This document describes the privacy network architecture for operation-dbus-proto, which serves as foundational infrastructure for the larger AI/chatbot control plane system.

## Core Architecture

### Identity Layer
- **WireGuard** serves as the primary cryptographic identity
- Magic links provide user registration and access control
- Session tokens are derived from WireGuard identity
- WG private keys should be protected in a vault-like system

### Privacy/Obfuscation Layer  
- **`wgcf`**: Cloudflare WARP tunnel used purely for privacy/obfuscation
  - No identity attached (consumer 1.1.1.1 app style)
  - Can use WARP or MASQUE protocol
  - Managed by `wg-quick` with dinit service

- **Privacy tunnels**: `priv_wg`, `priv_xray`, `priv_warp`
  - Predefined privacy routing paths

### Privacy Ingress
- **Xray client** serves as the privacy ingress point
- External traffic flows through Xray client before privacy routing

### Socket Networking
- **Shared socket port** (`ovsbr0-sock`) for container networking
- Multiple containers share one port instead of per-container ports
- OpenFlow controller manages routing and policy decisions

### Traffic Classification
- **Packet traffic**: Routes through full privacy chain
- **Internal agent communication**: Uses Tonic gRPC, may have different routing
- **System containers**: May bypass some privacy layers

## Network Components

### OVS Bridge (`ovsbr0`)
- Main bridge for privacy and container networking
- Managed by netplan for persistence and D-Bus awareness
- OpenFlow controller for intelligent routing

### Key Ports
- `wgcf`: Privacy/obfuscation tunnel
- Xray client port: Privacy ingress
- `ovsbr0-sock`: Shared container socket port
- `ovsbr0-mgmt`: Management port
- Privacy ports: `priv_wg`, `priv_xray`, `priv_warp`

## Implementation Approach

### Service Dependencies
1. `netplan-apply` - Creates and maintains OVS bridge
2. `wgcf` dinit service - Manages WARP tunnel (after netplan)
3. `ovs-attach-ports` - Programmatically adds ports via JSON-RPC
4. OpenFlow controller - Handles routing based on policies

### Configuration Strategy
- **Netplan**: For OVS bridge persistence and D-Bus integration
- **wg-quick**: For WireGuard tunnel management with `Table = off`
- **JSON-RPC**: Via op-dbus for bridge and port management
- **OpenFlow**: For policy-based routing

## Related Components
- User registration and magic link system (existing in `op-web` and `op-identity`)
- OpenFlow plugin for policy-based routing
- Tonic gRPC for internal agent communication
- Container management integration (Incus)

This architecture provides a clean separation between:
- Identity (WireGuard + magic links)
- Privacy/obfuscation (wgcf WARP tunnel)  
- Container networking (shared socket ports)
- Management (dedicated management port)
