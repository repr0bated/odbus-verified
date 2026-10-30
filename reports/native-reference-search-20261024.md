# Native reference search and routing authority trace

## Implementation follow-up — 2026-10-24

Owner subsequently requested changing the identified paths and explicitly
confirmed: “Remove hard-coded routing/config overrides.” Task
`native-routing-change-20261024`, Codex `/root`, removes the additional chat
prerequisites and implicit configuration writes. The mounted binary capture still declares
`agents.<alias>.model_provider` and `providers.models.<family>.<alias>`.

Implemented in source:

- `/srv/git/3tched.com/src/chat/door-chat-adapter.ts`: all chat submissions use
  the existing `chat_response` path without requiring an applied browser model
  or pre-reading ordinary/control-plane/accountability routing bindings.
  Credential transport, server admission, committed-response validation and
  in-flight identity/context invalidation remain. Explicit model Apply keeps
  its exact binding and readback checks.
- Removed the now-unused `verifiedControlPlaneModel` and
  `verifiedAccountabilityModel` preflight helpers. Updated paired test source
  for chat without Apply, Notebook denial, in-flight identity replacement,
  and displaying observed routes that do not map into the UI catalog.
- `crates/op-grpc-bridge/src/mutation_engine.rs`: removed automatic profile
  materialization from catalog Apply. Deleted the sole-use
  `catalog_ordinary_profile.rs` helper and its `lib.rs` module declaration.
  Apply now requires an existing supported threetched profile; it no longer
  creates/reconfigures profile credentials, endpoints, wire APIs or tool flags.
  Explicit admitted provider/model selection and coherent readback remain.

No plugin schema changed. Before-file snapshots and build receipts are under
`/var/tmp/native-routing-change-20261024.RTBG5m/`. Subagents were bounded to the
AGENTS-mandated Cargo worker role and the requesting-code-review skill's
read-only review; each performed its own mandatory preload.

Build status: final `npm run build` passed (existing Svelte/compiler and chunk
size warnings). The first targeted GNU bridge build failed on two stale
dependency artifacts; both missing symbols existed in source. The worker
confirmed stale GNU-target artifacts, refreshed only the two source timestamps
without changing contents, and retried in the same shared cache. The retry
resolved both symbol errors, then failed in bridge schema capture because the
installed threetched manifest lacks `application_generation_cli_sha256`.
The existing prepared artifact at `target/threetched-artifacts/tched-router`
also lacks that field, so it cannot supply the required capture provenance.
No manifest was edited to manufacture provenance and no validation was bypassed.
Command: `CXXFLAGS='-include cstdint' cargo build -p op-grpc-bridge --bin op-grpc-bridge --target x86_64-unknown-linux-gnu`, as jeremy UID1000.
Retry ran 18:19:04Z–18:21:35Z and exited 101. Evidence:
[backend retry log](/var/tmp/native-routing-change-20261024.RTBG5m/bridge-build-retry.log:381),
[final UI build log](/var/tmp/native-routing-change-20261024.RTBG5m/ui-build-final.log).

Independent read-only review found no introduced
critical, important or minor defect in the bounded delta and requested no fixes;
the reviewer explicitly excluded inherited responder completeness, live profile
availability and unrun checks from that verdict. The isolated task diff is
[implementation.patch](/var/tmp/native-routing-change-20261024.RTBG5m/implementation.patch).

Tests, type-check, lint, formatting and smoke runs were not executed, per the
owner's manual-check policy. No deployment, seal, service, identity, grant or
live config changes. The pinned vendor UI archive was not refreshed, since it
would capture other workstreams' unfinished UI changes.

Inherited limitation found in review: the current uncommitted responder source
still has an unavailable generation stub, and this source trace does not
establish notebook/project context in the committed-turn workflow. The new
mock cases cover UI submission behavior, not Notebook end-to-end completion.
The owner's report that deployed chat/identity work is retained separately.

Completion status: source changes implemented and reviewed; UI built; backend
build blocked on the existing threetched producer-artifact prerequisite; tests
unrun and deployment unchanged. Exact next action for backend build is to
produce the pinned patched threetched artifact with its real provenance through
the existing vendor build workflow, then select it with
`THREETCHED_CAPTURE_BIN`/`THREETCHED_CAPTURE_MANIFEST` for the bridge build.
This task did not rebuild or publish the other workstream's unfinished vendor
patches, reseal the catalog, or package its shared UI work into a release.

The search-only findings below describe the pre-edit snapshot and are
superseded for the removed gate/helper by this implementation entry.

## Original search

Recorded 2026-10-24T18:11:09Z by Codex `/root`; task
`native-reference-search-20261024`. Owner reported Notebook displaying
“Apply an available catalog model with a confirmed native routing binding before
sending a chat message”, requested a codebase-wide `native` search, and asked
whether these references override threetched. Chat and identity remain working
per the owner's preceding confirmation.

Backend HEAD: `86f03d2dede738e2d780ab789dd5800b58678479`.
UI HEAD: `31eae4be68ce437e5c0d5c90c0a08d4a319630fa`.
Both checkouts contain substantial pre-existing changes. Findings describe the
observed working trees, not necessarily those commits or deployed binaries.

## Search coverage

Case-insensitive substring search, including hidden project files, across
`/srv/git/odbus-verified` and `/srv/git/3tched.com`:

| Checkout | Matching lines | Files |
| --- | ---: | ---: |
| Backend | 6,111 | 483 |
| UI | 189 | 41 |

This includes identifiers and prose, so words such as `alternatives` also match.
Backend matches include 3,530 lines under `crates`, 461 under `deploy`, 132 under
`vendor`, and 203 under `docs`; root handoffs/history contribute heavily.
These are search counts, not counts of defects or overrides.

Default ripgrep ignore rules apply. Excluded build/dependency directories:
`.git`, `node_modules`, `target`, `build`, `dist`, `.svelte-kit`; backend skill,
archive and report directories; JSONL transcripts and `*.lock`. Compressed source
archives were not unpacked. Checked-in vendor text and patches were searched.

Complete captured matches and per-file counts:
[backend matches](/var/tmp/native-search-20261024.rZZC4c/backend.txt),
[backend file inventory](/var/tmp/native-search-20261024.rZZC4c/backend-files.tsv),
[UI matches](/var/tmp/native-search-20261024.rZZC4c/ui.txt),
[UI file inventory](/var/tmp/native-search-20261024.rZZC4c/ui-files.tsv).

## Confirmed source findings

1. **Notebook is stopped by a UI routing prerequisite.**
   [door-chat-adapter.ts](/srv/git/3tched.com/src/chat/door-chat-adapter.ts:323)
   exempts control-plane, accountability and UI-render modes from its
   `appliedRoute` requirement, but not `notebook_lm`. Without the matching
   browser-confirmed route it returns the owner's exact quoted message at
   line 332, before `submitAndAwaitCommittedReply` at line 365. This branch
   does not submit a turn or receive an answer from threetched.

2. **Catalog Apply really can change threetched configuration.**
   The UI's `selectRoute` calls `applyCatalogModelBinding` in
   [native-routing.ts](/srv/git/3tched.com/src/chat/native-routing.ts:88).
   [MutationEngine](/srv/git/odbus-verified/crates/op-grpc-bridge/src/mutation_engine.rs:5482)
   resolves catalog authority, can call `ensure_ordinary_profile`, and then
   invokes the dashboard runtime's `set_selection`. This is an explicitly
   admitted Apply operation with identity/context checks, not a write caused
   merely by seeing the word `native` or submitting Notebook text.

3. **Profile materialization imposes settings from our metadata.**
   [catalog_ordinary_profile.rs](/srv/git/odbus-verified/crates/op-grpc-bridge/src/catalog_ordinary_profile.rs:31)
   replaces the selected model in an existing ordinary/subscription profile
   when needed. For an absent ordinary profile, it initializes a profile and
   writes model, URI, wire API, `native_tools=false`, and a provider key read
   from the designated source. Its `wire_api` helper maps both model-list
   formats to `chat_completions`. These are concrete configuration choices
   imposed by bridge code. No credential value was read or recorded here.

4. **Runtime selection setters persist provider/model changes.**
   [tched_router_runtime.rs](/srv/git/odbus-verified/crates/op-grpc-bridge/src/tched_router_runtime.rs:1741)
   `set_selection` PATCHes `/api/config` for the agent provider reference and
   profile model, then checks authoritative readback. `set_provider` and
   `set_model` also write. Their presence establishes source behavior;
   this search did not invoke them or establish which recent user action did.

5. **Some routing code instead preserves threetched's selection.**
   [control_plane_routing.rs](/srv/git/odbus-verified/crates/op-grpc-bridge/src/control_plane_routing.rs:24)
   replaces request fields with the observed dedicated agent selection, not
   the reverse. `chat_under_guard` in
   [tched_router_runtime.rs](/srv/git/odbus-verified/crates/op-grpc-bridge/src/tched_router_runtime.rs:1793)
   rejects a provider/model mismatch and sends the message through the selected
   agent. Read helpers, schema projections, tests, native container/storage
   handling and CLI transports are additional matches that do not by themselves
   prove a threetched override.

## Runtime and completion limits

The mounted threetched view exposes `tched_router` and `threetched`; its operation
inventory includes `apply_catalog_model_binding`, `resolve_catalog_model_binding`,
`SetSelection`, and config methods. Listing dynamic `data`/`value.json` entries
for threetched and chat_response returned EIO. Static directory discovery
succeeded. No alternate identity or transport was used to read that state.
No claim is made that current working-tree behavior is the deployed implementation.

This was a search and source trace, not a blanket audit of every matching line.
Only this report and the task's handoff entry were authored. No application
source edits, builds, tests, config mutations, service actions or deployment.
The next concrete repair target is the shared UI submission prerequisite;
changing backend profile ownership is a separate behavioral change requiring
the intended model-selection contract to be preserved.

Self-review: accuracy 4/5 (source evidence, runtime provenance unestablished);
completeness 4/5 (both codebases searched, prioritized routing trace rather than
all-line audit); clarity 4/5 (write paths distinguished from reads); actionability
4/5 (exact gate and write chain identified); conciseness 4/5 (raw inventory kept
outside the report). Overall 4.0/5.
