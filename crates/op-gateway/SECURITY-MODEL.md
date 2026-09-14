# ⚠️ CRITICAL: WireGuard Session Security Model ⚠️

## DO NOT ROTATE WG PSK PER-LOGIN

WireGuard PSK has NO overlap mechanism. Rotating it locks you out.

## Correct Model

```
WG PSK (STATIC - rarely rotated, manual only)
    │
    └── + Server Nonce (per-login, server-issued)
            │
            └── derives → Session Key (HKDF)
                            │
                            └── hash → Session ID
                                        │
                                        └── MCP Access Token
```

## Rules

1. **PSK is identity** - treat like a certificate, not a password
2. **Server nonce prevents replay** - no timestamps, no clock drift
3. **Session keys rotate** - derived fresh each login
4. **simd-json at wire edge** - serde for config/internal

## What Rotates vs What Doesn't

| Component | Rotates | How Often |
|-----------|---------|-----------|
| WG PSK | NO | Manual/yearly |
| Server Nonce | YES | Per-login |
| Session Key | YES | Per-login |
| Session ID | YES | Per-login |
| MCP Token | YES | Per-request or session |

## Implementation

See `wireguard_auth.rs` - session derivation uses server nonce, not PSK rotation.
