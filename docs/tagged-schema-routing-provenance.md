# Tagged schema routing + private IP provenance — design spec v1 (draft)

Status: **draft, no code, no deployment.** Maintained in
`odbus-verified` (this repo); the chatbot/UI surface in `3tched.com`
consumes it only through the door. It assembles the sketches in
`/srv/git/etched-in-schema` and the sealed-plugin reality here, and
names the one missing piece. Proven vs planned is marked honestly
throughout.

## Basis (already sketched in etched-in-schema)

- `catalog-routing/` (proven, tested): pure routing — control tags → OSCAL
  controls → candidate plugin operations. `decide()` order: match controls
  → root needs two humans → fail closed on unsealed methods → allow the
  grant-covered subset. **Tags never grant**; grants are exact sealed
  capability strings.
- `oscal/TAG-TAXONOMY.md` (draft): tag → control → sealed operations.
  The `integrity` row already names the gap: *"anchor needs a sealed
  `web3` method (none yet — drafts only)"*, evidence = anchor record
  (index entry + tx). `schema_tag_registry` is authoritative at runtime.
- `routing/DATA-MODEL.md`: `Outcome { decision_ref, operation_id?,
  event_hash?, outcome, redaction? }` — the receipt fields exist today.
- `schema/index-entry.schema.json` (draft): the Web3 index entry —
  `seq`, `prev_hash` (sha256 of the previous entry's canonical JSON),
  `kind` (contract, token, attestation, ipfs_object, domain, offer,
  transaction, other), `artifact` (chain_id, ref), `identity`
  (session_id, genesis), `receipt` (operation_id, event_hash, plugin,
  method, required_capability).
- `web3/README.md`: append-only index + Merkle anchoring.
  Default: **Base Sepolia + EAS**.
- `docs/SUBSIDIARY-SLED.md` (planned): a secp256k1 wallet is bound to the
  identity by a signed link recorded in its sled and in the index (the
  WireGuard X25519 key cannot sign Ethereum transactions). Every index
  entry references the receipt that authorised it.

## Basis (already in this repo, odbus-verified)

- `crates/op-state-store/src/event_chain.rs`: the **hash stream** —
  hash-linked events for every state transition,
  `event_hash = H(prev_hash || canonical_event_payload)`, Merkle tree
  batching, schema-aware canonical hashing, tag-scoped proofs, and
  replay. Event records carry actor, capability, plugin, schema
  version, decision (allow/deny + reason), bounded redacted input
  payload, and result hash. This is the private IP provenance stream;
  only its head/Merkle root is ever published (see boundary below).
- `crates/op-plugins/src/state_plugins/routing_decision.rs`:
  owner-scoped, deterministic routing-decision records behind sealed
  capabilities (`cap.governance.routing-decision.read@v1`,
  `cap.governance.routing-decision.record@v1`); the bridge supplies the
  verified caller, sealed-catalog resolution, and policy snapshot —
  the plugin creates no alternate dispatch or state-write path.
- `crates/op-plugins/src/state_plugins/schema_tag_registry_methods.rs`:
  typed, owner-scoped schema-tag and routing-policy contracts —
  the runtime tag authority the tagged routing resolves against.
- `crates/op-grpc-bridge/src/mutation_engine.rs` and
  `method_receipt_audit.rs`: the receipt machinery (`operation_id`,
  `event_id`/`event_hash`, outcome) that index entries point back to.

## Boundary: private provenance / public proof

**Our hash stream stays in OP-DBUS (private IP provenance):**

- full index entries, artifacts, prompts, schemas, source material,
  identity mappings, creators/owners/reviewers, derivation records;
- salts, private tags, complete arguments/results, execution history;
- access only through sealed methods and compiled grants.

**Ethereum carries public proof and accountability only:**

- periodic anchors of the stream: `{seq_from, seq_to, stream_head_hash,
  merkle_root, chain_id}` as an EAS attestation (default) — one anchor
  covers a range, so individual records stay unlinkable;
- registration/governance facts that are meant to be public: plugin and
  schema digests, tag registrations, successor versions, revocations;
- never: payloads, private tags, salts, credentials, or identity data
  beyond the commitments already in the index schema.

Anyone can verify accountability — *who did what, under what
authority* — from the anchored head plus a selectively revealed entry:
recompute the entry's canonical sha256, walk `prev_hash` to the anchored
head, recompute the Merkle root. The private record itself is revealed
only to chosen verifiers.

## Tagged schema routing

1. Adapter emits a `Request` with ≥1 control tag plus qualifiers
   (`action:*`, `approval:root`, `surface:*`, `data:private|public`).
2. `decide()` resolves tags → controls → candidate operations over the
   authoritative OSCAL JSON. Routing is pure and local; it must not
   depend on chain availability at call time.
3. Candidates without a sealed blob are refused
   (`method-not-sealed:<plugin>.<method>`) — fail closed, as tested.
4. Grants come from the compiled profile, never from tags or chain data.
5. Execution goes through the identity's OP-DBUS mount only. The result
   is an `Outcome` carrying `operation_id`/`event_hash`, which becomes
   the receipt an index entry points to.
6. The index appends a hash-linked entry for public artifacts the
   identity creates (contracts, attestations, anchored releases).
   Private provenance entries use the same stream with the artifact
   replaced by its hash.

**Ethereum's role in routing is publication, not runtime:** tag →
control → operation-descriptor digests are published/guverned on-chain
so third parties can audit *what routing basis was in force*. The router
pins a published version by digest and verifies it against the sealed
local contract before use; a mismatch fails closed. Chain downtime never
blocks a locally sealed, grant-covered route.

## The one missing piece

A sealed `web3` anchor method. Today it exists only as a draft stub in
the compliance catalog (`web3/anchor`, capability `None`), and the
router correctly refuses it as unsealed; the anchor-release acceptance
path requires two humans.

To build it, follow `docs/DEV-CYCLE.md` in the OP-DBUS repo (logic →
plugin registration → sealed blob → operation file → call → receipt):

- signer: the identity's bound secp256k1 wallet (signed link in sled +
  index), held server-side; no key material in any UI or chatbot;
- input: an anchor record (seq range, stream head hash, Merkle root);
- output: chain id + transaction/attestation reference, recorded as an
  index entry whose receipt is the anchor call itself;
- capability and approvals: root-class — two different humans, per the
  existing approvals crate and `anchor_release_needs_two_humans`;
- transport: a configured Ethereum RPC endpoint. A public read gateway
  is optional for reads; broadcasting needs the signer's RPC path.

## Decisions still open

- EAS attestation vs a dedicated anchor contract (default: EAS on Base
  Sepolia, per `web3/README.md`).
- Anchor cadence: per-release vs periodic batched root (recommended:
  batched; per-entry anchoring leaks timing and costs more).
- Which tags/registrations are public on-chain vs hash-only (default:
  `data:private` tag sets stay private; only their roots anchor).
- Production chain and contract ownership (Safe/multisig) — requires a
  separate owner confirmation; nothing here deploys to mainnet.

## Blockers for the sealed inventory (honest status)

- The live door was not reachable from the authoring session
  (`zcall` transport not connected) and no blob inspector was available,
  so method-level sealed inventory was **not** re-verified for this
  draft. Tag/operation rows above are the drafts already recorded in
  `oscal/TAG-TAXONOMY.md`; the registry is authoritative at runtime and
  this spec must be re-checked against it before any code is written.
- The subsidiary's public Ethereum gateway hostname is registered, but
  an RPC probe of the gateway currently returns a Cloudflare error, and
  IPFS gateway creation is not entitled on the account. Neither blocks
  this design (anchoring uses the signer's RPC path), but gateway-based
  public reads should not be promised until re-probed successfully.

## Next steps (in order, each separately confirmable)

1. Re-run the sealed inventory against the live door; reconcile
   `TAG-TAXONOMY.md` rows with `schema_tag_registry`.
2. Prototype the anchor record + Merkle batching over the existing index
   schema (pure code, tests only, no chain).
3. Seal a draft `web3` anchor method in the OP-DBUS repo; router stops
   refusing it only once its capability is sealed.
4. Sepolia pilot: bind a test wallet, anchor one release with two-human
   approval, verify selective disclosure end-to-end.
5. Only then: production chain, ownership, and public registration
   decisions.

## Live verification (2026-11-01, through the chatbot container's mounts)

Read-only verification against sealed blobs (no code changed):

- `memory.create_namespace` sealed metadata requires
  `mut.data.memory.create-namespace@v1` (subid
  `mut.data.memory.create-namespace@v1`-family) — the **canonical**
  form. The identity holds only legacy `memory.namespace.create`.
  Execution was refused with the exact missing capability named:
  `AccessDenied: capability 'memory.stats.read' / 'memory.namespace.read' /
  'cap.governance.routing-decision.record@v1' is not granted to the
  authenticated caller`. This confirms field-catalog conflict #4
  (capability ids mid-migration) with live evidence: the routing
  boundary must normalize to `cap.*@v1` / `mut.*@v1`, legacy names are
  aliases only.
- The chatbot container's mounts are lungs views: reads succeed
  (`get_own_identity` returned the full sled), every mutation is
  refused. The seed-image daemon (`/usr/local/bin/3tched-fs`) speaks
  the bare-args call protocol; the `3tchedfs.call/1` envelope in
  odbus-verified source is newer than the deployed seed daemon, which
  rejects it at validation. Both `operations/<plugin>/<method>/call`
  and `plugins/<plugin>/operations/<method>/call` layouts are served.
- Refusal receipts are structured
  (`{plugin, method, stage, error, operation_id: null}`) — denials are
  first-class evidence, exactly as the router's `decide()` models them.
- No Ethereum/secp256k1 signer material exists in the operator's
  secrets (checked names only). Sepolia anchoring is blocked on a
  deployer key + working RPC route, both owner decisions.

## Pending owner decisions (execution gate)

1. Grant canonical `mut.data.memory.*@v1` (+ read/stat/query set) to
   the project identity so the two approved namespace provisions
   (`hackathon-chat`, `hackathon-provenance`) can execute — approvals
   already recorded: owner yes in chat, chatbot human identity second.
2. Provide an ETH deployer key and confirm Sepolia RPC before any
   anchor work.
