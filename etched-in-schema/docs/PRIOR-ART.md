# Prior art and positioning

How Etched in Schema relates to production identity systems, and what it adds.
Use this for the pitch, the YC application, and design decisions.

## The pattern is proven in production

The remote-identity design (establish once, then zero trust over any
transport) matches **SPIFFE/SPIRE**, the CNCF standard for workload identity:

| 3tched | SPIFFE/SPIRE equivalent |
|---|---|
| Kernel-peer check on the lungs Unix socket | SPIRE Unix workload attestor — identifies the calling process over a Unix socket using kernel peer credentials (same mechanism) |
| WireGuard key establishes the identity once (decoy issues OIA1, session gets SID1) | Node attestation: the machine proves itself once (cloud identity document, TPM, join token) |
| Sealed credential (SID1), then zero trust over any transport | Short-lived SVIDs (X.509/JWT) verified on every connection (mutual TLS) |
| Door interceptor re-verifies every call | Each service verifies the peer's SVID per request |

Other production relatives:

- **Google BeyondCorp, Cloudflare Access** — zero trust: nothing is trusted for
  being on the network; every request is verified against identity and policy.
- **AWS IAM Roles Anywhere, cloud instance identity** — a workload proves
  itself once, then uses short-lived credentials from anywhere.
- **W3C DIDs and Verifiable Credentials** — the standard for portable,
  self-controlled identity anyone can verify; the Web3-adjacent reference.

## What 3tched adds (the differentiators)

1. **The identity is the entity, not a label.** SPIFFE names a workload whose
   state and history live elsewhere. In 3tched the identity *is* the key,
   genesis and receipt chain, with its own container, memory and storage.
2. **Capability per typed operation.** SPIFFE answers "who is this?" and leaves
   "what may it do?" to each service. 3tched checks the exact capability for
   each schema-declared operation centrally and records a receipt.
3. **Verifiable history.** Workload identity proves who is calling now. 3tched
   adds a tamper-evident record of everything the identity has done,
   anchorable on-chain.
4. **Compliance by construction.** OSCAL requirements compile into the grants,
   budgets and network policy that are actually enforced, with receipts as the
   assessment evidence.

## Transport and identity (decided)

- WireGuard is only the establishment step; after that the credential carries
  identity over any transport. The mechanism exists; it needs testing and
  refinement.
- **Netmaker** (WireGuard mesh) would only add connectivity; it is retired from
  3tched and not needed.
- **Cloudflare** (WARP/mesh) is acceptable as a pipe, never as the identity
  layer: devices authenticate to Cloudflare with Cloudflare's keys, so the
  bridge would see a Cloudflare address, not the identity's key. Identity stays
  proven end to end by the credential.
- Sealed blobs (schemas) are content-addressed: they travel with the identity
  or are fetched by hash over any channel and verified.
- What the model depends on: credential binding to the key, expiry,
  revocation, and replay refusal. Known gap (HANDOFF, 2026-09-25): the
  interceptor still mints genesis server-side when no session exists, and the
  decoy mints only OIA1 — verify before relying on remote identities.

## Pitch lines

- *"Workload identity like SPIFFE proves who is calling. Etched in Schema also
  proves what the identity was allowed to do — and what it actually did."*
- *"Every on-chain thing the agent creates comes with a verifiable answer to
  who did it, what they were allowed to do, and what really happened."*
- *"Implements and evidences these controls"* — never "compliant" or
  "certified".
