# Subsidiaries and the identity sled

How an identity works as the embodiment of a subsidiary. Marked **existing**
(in 3tched today), **proven** (demonstrated 2026-10-28) or **planned**.

## Hierarchy (owner model)

- **Root**: the 3tched control plane — the only root on the system. Its chatbot is a one-off part of the
  system core, to be fused with the WireGuard server key so it can never be a
  peer (session `<root-session>`). **Planned** — no
  supported re-key operation yet.
- **Root has no execution rights**: it holds authority but executes nothing
  itself; execution is delegated through the execution tracker. A human acting
  as root needs **two approvals** (two-person rule). Today `elevation` offers a
  single-use ticket (`authorize_task`, 60 s TTL); the second approval is new
  work. **Planned.**
- **Owner override** (owner requirement): while the owner is the only human,
  his identity must be able to override the two-approval rule. Proposed:
  routine root actions take two approvals from two independent owner devices
  (session + hardware key/phone); an override is break-glass — fresh
  hardware-key confirmation, a cancellable delay, a receipt anchored on-chain,
  and a notification on a second channel — so even the owner cannot act as
  root silently. A designated human second approver replaces this later.
  **Planned.**
- **Second approver = the control-plane chatbot** (owner decision): it co-signs
  the owner's root actions **by policy, not persuasion** — it receives only a
  structured request (operation, resource, identity, applicable policy), never
  free-text justification; atomic judgments (the chatbot or Jev) feed code that
  decides approve/refuse; anything outside written policy is refused and only
  break-glass can override; every decision is receipted with its reasons. It
  approves but never executes. Tiers: routine = owner + chatbot within policy;
  out of policy = refused, break-glass only; later = human second approver.
  **Planned.**
- **Subsidiaries**: Linux users on the system. Each subsidiary is bounded by
  its home directory. **Planned.**
- **Hackathon subsidiary**: embodied by identity `<subsidiary-session>` (today's
  `3tchedCtlChatbot`). **Existing.**

## What the sled is (existing)

The sled is the identity. For each identity session it holds:

| Part | What it is |
|---|---|
| Record | session id, WireGuard public key, principal kind, genesis hash, trace id, schema version, active flag, timestamps |
| Instance binding | the container that embodies it (name, native UUID, pool) |
| Private leaf | the identity's writable storage, with the canonical subvolumes `@agent_soul`, `@agent_memory`, `@agent_rules`, `@agent_skills`, `@agent_user`, `@agent_shared` (shared/soul read-only; memory/rules/skills/user writable) |
| Session ledger | session events and receipts; the input for vectorization |

Session and principal ids are derived from the public key
(`op-identity::session::derive_session_id` / `derive_principal_id`, Blake3
KDF), so the key, genesis and chain *are* the identity — not a machine.

Lifecycle rules (existing): the identity subsystem owns minting; Cozo owns
assignment; login/start/stop never add or remove Btrfs members; the bridge
only verifies externally mounted storage. Never edit sled rows directly.

## How a subsidiary acts

1. The subsidiary's identity runs in its own container with a 3tchedFS mount
   (lungs). **Proven** for `<subsidiary-session>`.
2. On the host, the same identity's mount is exposed inside the
   subsidiary's home, e.g. `/home/<user>/<Subsidiary>/3tchedfs` and
   `/home/<user>/<Subsidiary>/threetched`, bound to that session
   (`--lungs-session`) with call files owned by that Linux user only
   (call-owner UID). Other users can read schemas but cannot act. **Planned.**
3. Every call: kernel or credential identifies the identity → bridge checks the
   exact capability → MutationEngine commits → receipt. **Proven.**
4. threetched (model/agent) operations used by a subsidiary are its own:
   one router agent per subsidiary with its own model, tool set and budget
   (`max_actions_per_hour`, `max_cost_per_day_cents`). **Planned.**

## Sled + execution tracker (planned)

Every tool call is validated by the one shared execution tracker and written
to the identity's sled session ledger; that ledger is vectorized (Qdrant) so
the identity can answer questions about its own history. Code: branch
`recover/execution-tracker` (`identity_sled_dispatch::subscribe_execution_tracker`).

## Sled + Web3 (planned)

- The index's first entry for an identity records its origin (3tched origin
  session, genesis) and is anchored on-chain.
- A secp256k1 wallet is bound to the identity by a signed link recorded in
  its sled and in the index (the WireGuard key is X25519 and cannot sign
  Ethereum transactions).
- Each index entry references the receipt (`operation_id`, `event_hash`) that
  authorised it.

## Sled + OSCAL boundaries (planned)

The subsidiary's OSCAL profile compiles into: the identity's capability grants
(what it may call), router budgets (how much), and OpenFlow policy on its
network (where it may connect). The bridge already enforces grants on every
call; the new work is generating them from OSCAL.
