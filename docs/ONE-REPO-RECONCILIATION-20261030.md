# One repo reconciliation — 2026-10-30

Single source rule: sealed SHM blobs are the registry. FUSE is the live byte-for-byte view, not a second source. Duplicates in blobs = wrong (verified 72/72 unique, zero dupes).

## Precedence
1. First-round hackathon work wins (purpose projection).
2. Later hackathon wins over earlier.
3. Hackathon wins over original `etched-in-schema`, except build dirs/untracked local files (ignored).

## Map

| Area | Status | What changed to bring up to date |
|---|---|---|
| Sealed blobs (`/dev/shm/opdbus/plugin-blobs/`) | Existing, kept | Resealed after build (`seal-shm` + `persist`); bridge consumes only, never reseals. |
| FUSE live view (`3tchedFS`) | Existing, kept | Remount after activation; exact copy of blobs, real-time representation. |
| Fresh bridge (`66f1d99 → f180bff`) | Changed (rewrite) | ~180k lines removed, 8/15–18 surfaces kept; compile 60min → <5min. Old bridge retired, review fixes retained (`6276a18`). |
| `chat_response` (ledger) | Existing, kept | Principal-scoped turns + hash chain; no change, distinct from router. |
| `chat_router` (router+generation) | Changed | Native threetched capture per seal; `llm_plugin` default for Antigravity; owns `Chat/ResolveRoute/ListModels/PatchConfig`. |
| `ctl_plane_chatbot` (episodes) | Existing, kept | Reasoning episodes + `query_context/vectorize`; control-plane only, not the subsidiary product. |
| Subsidiary chatbot (`3tchedCtlChatbot`) | Existing, unproven live | Sled model + container + lungs mount exist; needs one admitted `get_own_identity` + `Chat` receipt to prove anchored/active/bound. |
| Purpose projection (`hackathon/catalog-routing/.../projection.rs`) | New (first round, wins) | `Vectorize/Display/Overlay` closed set + PII gate; twin of `ProjectionPolicy` OD-64 in `chain_vectors.rs`. `Css` to be added same pattern. |
| Memory live (`memory_store` Cozo + `memory_loop` + `context_awareness` + `qdrant_shuttle`) | Existing, kept | Cozo = truth, Qdrant = reorder only; `activity_filter` drops noise/PII before Qdrant. |
| Snowball + Qdrant (`snowball_footprints`, `blob_vectors`, `user_memory`, Voyage-4) | Existing, kept | Origin embeds → `vectors/` subvolume → replica `ingest` after `btrfs receive`; `--upsert-qdrant` manual only. |
| MQTT wakes (responder-wake + doorbell-wake via EMQX) | Existing, needs decision | Both live; owner to name which to drop — no delete until approved. |
| Svelte/json-render pixels (`3tched.com` → op-web) | Existing, kept | Svelte-only, no React; renderer projects blob per purpose; GUI exists, CSS purpose styles it. |
| Central identity (sled = entity) | Existing, kept | Record + instance + private leaf + ledger; key/genesis/chain = identity; capability per op + receipts. |
| Web3 on-chain (wallet/anchor/contract buy) | New, planned | In `SUBSIDIARY-SLED.md` only; no `secp256k1`/EVM in crates. X25519 anchoring only today. |
| Jupyter/GPU remote kernels | Out of scope | Example of hardware detachment only; NotebookLM knowledge notebooks stay in. |
| MCP paths in source | Old, to retire | Retired 2026-10-28; 3tchedFS is the door. Removal scheduled, not done. |
| Today’s dirty (`AGENTS/HANDOFF/SIGNALS/WATCHDOG`, `wake_doorbell`, `emqx`, `routing_decision`, `snowball`, `typed_export`, `build-golden`) + junk `129:*`, `release-bridge.sh`, `test.sh` | Changed/untracked, triage | Keep later-wins edits, drop junk/shims on approval. No builds/tests until after deploy (downtime agents). |

## Next
1. Prove subsidiary: admitted identity + routes + `Chat` receipt.
2. Add `Purpose::Css`, style existing pixels.
3. Name MQTT duplicate to drop.
4. Commit one-repo state, then compliance-loop CMS/Slack render.
