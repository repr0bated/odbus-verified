# Zen Review — 2026-08-23

Whole-stack architecture, specification, and per-component Zen Review output produced
2026-08-23 between 04:58 and 05:10 UTC. Copied here from the Gemini/antigravity brain
directory, which is scratch storage and not versioned:

    ~/.gemini/antigravity-cli/brain/b279c9ba-83fb-4fab-8d0d-a6ad90aaa433/

The originals were left in place. Only the markdown was copied; the sidecar
`*.md.metadata.json` files were not.

## Contents

| File | What it is |
|---|---|
| `architecture.md` | Master architecture across odbus, golden deployment, 3tchedFS, and the operator console, with a mermaid topology |
| `specification.md` | Companion specification |
| `kiro-zen-review.md` | The Zen Review pass itself |
| `kiro-full-traceability-matrix.md` | Requirement traceability |
| `MASTER-28-SPECS-REQUIREMENT-AUDIT.md` | Requirement-by-requirement verification of all 28 specs against the live codebase |
| `audit-*.md` (6) | Protocol/reflection, supervision/services, cognitive-mcp/routing, declarative UI/catalog, security/ingress/identity, container/network |
| `spec-01…spec-28` | One per component |

## Known divergences from the live host (verified 2026-08-23, same day)

1. **The 8090 ingress is not covered.** `architecture.md` §4.3 reviews `op-grpc-bridge`
   only as the MutationEngine — event chain, actor resolution, projection drift. The TCP
   listener, the MQTT/WebSocket demux, and TLS certificate loading are not examined.
   Commit `ffcb4796`, made 46 minutes after these docs, deleted the demux and the
   `ZEROCLAW_TLS_CERT_FILE` loading — the third time that has happened — and nothing in
   this audit covered the surface that regressed.
2. **Field numbering is described two ways.** The blob review states `descriptor.rs` uses
   sequential numbers `(i + 1)` sorted by property name; `op-grpc-bridge/build.rs` uses an
   FNV-1a hash (`stable_field_number`) deliberately *not* sequential. Two descriptor
   generators can therefore assign the same plugin different field numbers.
3. **"Static wg0 Interface"** is intent, not live state. The interface is named `netmaker`
   and netclient still owns it; `wg-3tched` was never enabled.
4. **`NEVER_AUTO_RESTART` names `ovsbr0-addr`**, which is not a service on this host.
