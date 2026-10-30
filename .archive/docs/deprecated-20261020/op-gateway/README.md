# op-gateway

MCP Gateway with WireGuard authentication and smart routing.

## Overview

op-gateway provides the authentication and routing layer for MCP clients:
- **WireGuard authentication** - Cryptographic identity verification
- **Smart routing** - Route clients to appropriate backends based on auth status
- **Encrypted storage** - Secure key storage using Btrfs/LUKS

## Architecture

```
┌─────────────────────────────────────────┐
│            MCP Clients                  │
│                 │                       │
│          WireGuard Auth                 │
└─────────────────┼───────────────────────┘
                  ▼
┌─────────────────────────────────────────┐
│            op-gateway                   │
│  ┌─────────────────────────────────┐    │
│  │      McpGatewayManager          │    │
│  │  ┌───────────┐ ┌─────────────┐  │    │
│  │  │ WireGuard │ │ Encrypted   │  │    │
│  │  │ Auth      │ │ Storage     │  │    │
│  │  └───────────┘ └─────────────┘  │    │
│  └─────────────────────────────────┘    │
└─────────────────┬───────────────────────┘
                  │
    ┌─────────────┴─────────────┐
    ▼                           ▼
┌─────────────┐         ┌─────────────┐
│ Full Access │         │ Cognitive   │
│ gRPC:50051  │         │ Only:50052  │
│ All tools   │         │ Limited     │
└─────────────┘         └─────────────┘
```

## Smart Routing

Clients are routed based on authentication:

| Auth Status    | Endpoint    | Access Level    |
|----------------|-------------|-----------------|
| Authenticated  | gRPC:50051  | Full (all tools)|
| Unauthenticated| gRPC:50052  | Cognitive only  |

## Components

### WireGuardAuthManager

Handles WireGuard key-based authentication:
- Session creation and validation
- Key derivation (X25519, HKDF)
- Session expiry and cleanup

### EncryptedKeyStorage

Secure storage for private keys:
- Btrfs subvolume encryption (native or LUKS)
- ChaCha20-Poly1305 encryption
- Argon2 key derivation
- Zeroize on drop

### McpGatewayManager

Routes MCP clients to appropriate backends:
- Authentication check
- Routing decision
- Session management
- Capability filtering

## Usage

```rust
use op_gateway::{McpGatewayManager, WireGuardAuthManager};

// Initialize
let auth = Arc::new(WireGuardAuthManager::new().await?);
let gateway = McpGatewayManager::new(auth).await?;

// Route client
let client_info = McpClientInfo {
    name: "my-client".to_string(),
    peer_pubkey: Some(pubkey),
    ..Default::default()
};

let routing = gateway.route_client(client_info).await?;
// routing.endpoint = "grpc://localhost:50051" (if authenticated)
```

## Security

- X25519 key exchange
- ChaCha20-Poly1305 AEAD
- Argon2id key derivation
- Blake2s hashing
- Zeroize sensitive data
