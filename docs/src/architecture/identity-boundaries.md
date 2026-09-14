# Identity Handoff Boundaries

This document is the authoritative boundary statement for the corrected
identity handoff. It has three parts: non-negotiable architecture boundaries,
the repudiated design path, and the external integration boundaries that this
mission documents but does not deploy.

## 1 · Non-negotiable architecture boundaries

1. **Oracle decoy is the sole incoming WireGuard termination point.** The
   main host has no incoming identity WireGuard interface; never add
   `wg-lan` or any host WG interface.
2. **No NetMaker transport remains.** The decoy's standalone `wg0` terminates
   human WireGuard; its standalone `wgcf-ingress` carries Xray egress through
   WARP. The retired static NetMaker carrier must not be recreated.
3. **Assertion carriage is INNER.** The short-lived Ed25519-signed
   `OracleIdentityAssertion` rides as gRPC metadata
   (`x-oracle-identity-assertion-bin`) inside the existing TLS channel.
   Xray is a passthrough and never sees plaintext identity state.
4. **`op-grpc-bridge` is the sole validator and the application
   authorization boundary.** Validation order: parse → trusted decoy key →
   signature → expiry → replay cache → configured transport binding →
   HumanPrincipal resolution → existing capability gate. Production WARP uses
   the trusted-decoy-signature binding; every step remains fail-closed.
5. **Human identity, WireGuard key, login session, workspace container, and
   display alias are separate concepts.** A workspace container is not the
   human. System containers are never users. The display alias is
   display-only and never authoritative.
6. **The authenticated client request triggers resolution.** No handshake
   watchers, polling loops, D-Bus watchers, or background expiry tasks (the
   replay cache purges lazily on access). SSH login alone is administration;
   it does not mint or carry OIA1 identity.
7. **Xray live config exists only at `/etc/xray/xray_config.json` inside the
   xray container.** Models do not write or reload xray directly.
   `op-xray-daemon` remains lifecycle-only.
8. **PluginSchema is the source of truth.** Identity operations are methods
   on the `human_principal` plugin via the generated gRPC surface
   (`PluginService` → D-Bus → `MutationEngine`). No hand-written per-plugin
   proto, no direct backend RPC, no second identity control plane.

## 2 · Repudiated design path (do not reintroduce)

The rejected spec `claude-redo/netmaker-xray-identity-handoff/` proposed the
following, all of which are forbidden in this architecture:

| Rejected mechanism | Why it is rejected |
|---|---|
| `wg-lan` host WireGuard interface | Violates boundary 1; WG terminates only at the Oracle decoy |
| `op-identity-shuttle` handshake-watcher service | Polling-based; violates boundary 6 |
| `TransportBindingIndex` (src_ip → pubkey SHM table) | Source-IP assertion is not a cryptographic binding; 180 s race window |
| Per-peer OpenFlow identity tagging (`NXM_NX_REG` tags) | Datapath theater; the real gate remained the IP lookup |
| Per-registration identity containers as verifiers | Turns a network segment into a trusted intermediary; verification belongs at the single existing gate |
| Xray header injection into TLS | Impossible without xray becoming a TLS-terminating MITM |

These tokens are enforced by the negative topology gates
(`crates/op-grpc-bridge/tests/negative_topology_gates.rs`,
`scripts/check-identity-topology.sh`).

## 3 · External integration boundaries (documented, NOT deployed by this mission)

The mission implements both sides LOCALLY: the decoy issuer as a library plus
a local decoy simulator for E2E tests over real TLS on ephemeral localhost
ports. The following are external assumptions that production deployment must
satisfy; they are documented here and tested only through their local
stand-ins.

### 3.1 Oracle decoy (EXTERNAL)

- Terminates the human's WireGuard tunnel; the kernel verifies the peer.
- Issues the signed assertion for the exact `/32` peer selected by the kernel
  (decoy Ed25519 key identified by
  `decoy_key_id`) with TTL ≤ 900 s.
- The client that requested the assertion carries the returned OIA1 envelope
  as `x-oracle-identity-assertion-bin` on its inner tonic request. Neither
  WGCF nor Xray creates or injects that metadata.
- The decoy's verifying keys are provisioned to the bridge's trust store
  (`OP_DECOY_TRUST_STORE`, default `/etc/opdbus/decoy-trust.json`) by an
  operational process outside this mission.

### 3.2 Assertion transport (EXTERNAL)

- Exactly one WARP tunnel carries decoy→host Xray traffic.
- WARP and Xray replace the human WireGuard inner source address. Deployments
  therefore set `OP_ORACLE_ASSERTION_SOURCE_BINDING=trusted-decoy-signature`:
  the bridge binds identity to the provisioned decoy Ed25519 key, short
  assertion lifetime, one-time nonce, and registered human WireGuard key.
- `exact-peer-ip` remains the strict default for transports that genuinely
  preserve the human inner source address. Unknown configuration values fall
  back to that strict mode.
- WARP is transport-level policy; it is not identity.

### 3.3 Xray container (EXTERNAL, unchanged)

- Passthrough only: SNI/protocol sniffing, no TLS termination for this
  traffic, no header injection, no identity logic.
- Live config only at `/etc/xray/xray_config.json` inside the container;
  models never write or reload xray.

## 4 · Mission safety boundaries (workers NEVER violate)

- No deploy, no sudo, no `/etc` edits, no service restarts, no live-host
  mutation. Cargo tests only.
- Never read credential files in `~/` (`master_key.txt`, `regkeys.txt`,
  `token.txt`, `netmk-rollback-*`).
- Rust-first: no new Python; scripts are shell.
- All cargo commands use `CXXFLAGS="-include cstdint"`; bridge lib tests run
  with `-- --test-threads=1` (pre-existing env/SHM race).
