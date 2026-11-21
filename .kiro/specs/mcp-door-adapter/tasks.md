# Implementation Plan — `op-mcp-door`

Spec id: `mcp-door-adapter`. Design: `design.md`. Requirements:
`requirements.md` 1–12. One task = one commit on `dev`, pushed fast-forward.
Cargo: `-p op-mcp-door` only, one process at a time, private target dir
(`.kiro/steering/subagent-cargo.md`). No task touches the bridge, 3tchedFS,
`op-tunnel-mux`, live services, grants or sealed blobs.

- [x] 1. Crate scaffold, configuration and canonical encodings
  - Add `crates/op-mcp-door` to the workspace (lib + bin `op-mcp-door`).
  - `config`: env resolution, TLS path fallback, listen default `127.0.0.1:8091`.
  - `canonical`: sorted-key canonical JSON and `args_sha256`.
  - `encode`: `encode_component` matching 3tchedFS.
  - Unit tests for all three.
  - _Requirements: 1.5, 1.6, 1.8, 3.1, 10.1_

- [x] 2. Catalog scan, tool naming and capability classes
  - `catalog`: scan `plugins/*/operations/*`, skip broken entries, names,
    annotations, `confirm_side_effects` on mutations.
  - `classify`: ordered class rules, public allow-list, hard deny-list.
  - Unit tests: truncation hash, collisions, deny-list beats allow-list,
    `command` args are `shell`, memory never public.
  - _Requirements: 4.1–4.4, 7.1–7.3_

- [x] 3. Tiers, toolsets file and selection cap
  - `toolsets`: tiers, generation from the catalog, file read/write,
    selection parsing (path, query, header), 50 cap with counts.
  - CLI `op-mcp-door toolsets generate --mount <path> --out <file>`.
  - Unit tests: core is 19 on a full catalog fixture, missing hot tools are
    reported, shell/exec never in core, over-cap refusal.
  - _Requirements: 5.1–5.4, 6.2–6.4, 10.4_

- [x] 4. Grant store and elevation tickets
  - `grants`: durable document reader with the bridge contract; ephemeral
    grants with window, count, argument constraint, toolset scope rules;
    tickets with ≤300 s lifetime and atomic single use.
  - CLI `grant issue|list|revoke`, `ticket issue`.
  - Unit tests for each scope and for the rejected document shapes.
  - _Requirements: 9.1–9.7, 10.1–10.3_

- [x] 5. The single `authorize()`
  - `policy`: `Purpose::{Discover, Call}`, bases, confirmation, tickets,
    consumption only after all checks.
  - Unit tests: public vs anonymous, no mount → deny, shell only ephemeral,
    unreadable grant document → public only.
  - _Requirements: 7.4, 8.1–8.4, 9.4, 10.1, 12.5_

- [x] 6. FUSE call-file client
  - `fusecall`: mount declaration check, one write, fsync, same-handle receipt,
    attribution checks, unknown outcome, no retry; `CallOpener` trait.
  - Integration tests on a tempdir tree with a fake engine.
  - _Requirements: 3.1–3.8_

- [x] 7. Audit log
  - `audit`: record type, redaction rules, append-only writer (stdout or file).
  - Unit tests for redaction and append mode.
  - _Requirements: 11.1–11.3, 1.7_

- [x] 8. OIA1 identity, principal and mount mapping
  - `identity`: header parsing, `op-identity` verification, key file,
    principal map, mount map, nonce-to-session binding.
  - Unit tests with a test signing key.
  - _Requirements: 12.1–12.5, 2.8_

- [x] 9. MCP endpoints and meta-tools
  - `mcp`: JSON-RPC handling, sessions, `/mcp/full` and `/mcp/loader`,
    meta-tools, origin and body checks, rate limit for public calls.
  - Integration tests over the axum router: hidden tools, `forbidden_or_unknown`
    parity, cap refusal, `call_tool` parity, one write per allowed call, zero
    writes per denial, audit lines.
  - _Requirements: 2.1–2.8, 6.1–6.7, 7.4, 7.5, 8.1–8.3_

- [x] 10. TLS-only server
  - `server`: rustls acceptor, key-mode check, `serve` subcommand.
  - Tests: TLS round trip with an rcgen certificate; plaintext HTTP gets no
    HTTP response; startup refuses a missing or group-readable key.
  - _Requirements: 1.1–1.5, 1.7_

- [x] 11. Deploy templates and handoff
  - `deploy/runit/op-mcp-door/{run,check,log/run,README.md}`,
    `deploy/config/op-mcp-door/*.example`; not installed.
  - Update `HANDOFF.md` and this file's status.
  - _Requirements: 11.4_

- [x] 12. Cognitive hot/lazy layout (owner decision 2026-11-19)
  - `catalog::TIER_HOT`: the owner's ten, with coding and knowledge
    (`search_own_events`) mandatory; `oscal_subids` leaves the hot tier.
  - `toolsets::generate`: one lazy `cognitive` set (every cognitive operation
    in neither tier); drop `coding`, `reasoning`, `agents:<persona>` and
    `plugin:cognitive`; `schema` keeps the schema plugins only.
  - `Toolsets::new`: memory is never grouped, even from a hand-edited file.
  - Unit tests: hot ten, single lazy set, memory never grouped, every tool in
    a tier or a set.
  - _Requirements: 5.1, 5.2, 5.5, 5.6_

- [x] 13. Vector readiness gate for the hot coding and knowledge tools
  - `readiness`: read-only Qdrant REST probe (`GET /collections/<name>`),
    coding/knowledge gates, 30 s cache, `pending_revectorization` body.
  - `config`: `OP_MCP_QDRANT_REST_URL`, `OP_MCP_QDRANT_API_KEY_FILE`,
    `OP_MCP_CODING_COLLECTIONS`, `OP_MCP_KNOWLEDGE_COLLECTIONS`.
  - `mcp`: gate in the single call path before the call-purpose
    `authorize()`; `list_toolsets` reports `vector_readiness`.
  - CLI `op-mcp-door vectors status` (exit 3 while pending).
  - Tests: probe against a local socket (GET only, api-key header), chunked
    parsing, cache, path-safe names; door flow: pending answer with zero FUSE
    writes and an audit line, pass-through once populated.
  - _Requirements: 5.7, 5.8, 5.9_

- [x] 14. FUSE toolset manifest for Wonder's projection
  - `deploy/config/toolsets/cognitive.toolsets.json` (data only, not
    installed) + `README.md` listing what the projection must read and do.
  - `tests/manifest.rs`: manifest tiers, single lazy set and gates equal the
    door's constants.
  - _Requirements: 5.10_

- [ ] 15. Switch to Wonder's toolset projection when the shared FUSE contract
  lands on `dev` (read the manifest through it; retire `toolsets generate`).
  Blocked on that contract.
  - _Requirements: 5.3, 5.10_
