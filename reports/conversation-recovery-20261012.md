# October 12 local conversation recovery and architecture reconciliation

This is an evidence report, not a new task board. Work remains tracked in
WISHLIST.md (OD-119/OD-120). Times below are UTC. Original conversation records
were read; handoffs were used as indexes and checked against selected tool
receipts. Historical test/deployment receipts are not fresh validation of the
current working tree or running release.

## Current owner corrections

These statements in the current conversation supersede older summaries:

- The main codebase is `/srv/git/odbus-verified`; `/srv/git/3tched.com` is its UI.
- The identity is the chatbot. The schema is the code. The container is the
  identity shuttle. Sockets are lungs; FUSE is the spinal cord; gRPC is the arms.
- One authoritative schema, sealed into one blob per plugin, drives UI, data,
  ledger/audit, memory, learning, compliance enforcement and tag routing.
  Catalogs and analysis graphs are derived views, not other schema authorities.
- D-Bus first. All methods declared in plugin blobs are exposed through the
  filesystem. The chatbot can call D-Bus through FUSE operation files. Reads
  expose sealed declarations and committed live state; invocation retains
  identity, schema, capability, receipt and audit boundaries.
- There is one Tonic endpoint for internal and external access. Everything
  must go through lungs with the same grants. Adding another endpoint or
  independent authority is not an acceptable reconciliation.
- Subsequent owner clarification: human and machine principals have different
  execution/binding paths **by design**. The initial audit incorrectly treated
  the physical dispatch differences as sufficient evidence of a defect. Keep
  their authentication distinct and trace common schema/grant authority; do
  not force human callers through a machine/container binding. The proposed
  shared-socket consolidation below was withdrawn before implementation.
- Consolidation occurred rapidly across sealed blobs, FUSE, transport and
  network. The owner states that six earlier sockets were consolidated into
  one shared socket. Identity containers use socket networking, not individual
  IP addresses; currently there is one identity container. Do not equate
  per-request connections/private IPC socket pairs with additional exposed
  socket-network endpoints. Old module/service names may describe migration
  history rather than current architectural layers.
- There is no internal MCP protocol in the intended FUSE-based cognition path.
  Legacy module/plugin names containing `mcp` do not establish protocol use.
  MCP remains the external client adapter, including Cursor.
- `$EXTERNAL-MCP` already exists. Resolve group-based capabilities and external
  client admission using that group and the existing identity authority.
- EMQX and OpenFlow internal addresses belong to `ovsbr0` itself, not member
  ports. Do not restore older port-based placements from stale documentation.
- The decoy is a separate server. Incoming WireGuard terminates there; the
  peer key anchors identity, and identity is minted on the decoy. The host is
  the receiver/validator of that authored identity, not a replacement issuer.
  On the decoy, wgcf/fwmark policy forces WireGuard-origin traffic through WARP.
  Remaining traffic uses MASQUE-based WARP tunnels, with SSH carried inside.
  These are the owner's current topology statements; this audit did not
  inspect the live decoy or verify its mark/routing tables. Distinguish host
  session bookkeeping from credential minting when auditing the receiver.
- There is no WireGuard on the OP-DBUS host. Host-facing WARP/MASQUE transport
  is intended to appear as ordinary WARP traffic; this audit did not perform
  traffic-classification or packet-capture verification. Xray provides the
  outbound REALITY transport using configured SNI/cover-destination behavior.
  Preserve that layer in the transport model without inventing a new egress.
- Admission identity is based on the WireGuard key; email or a personal profile
  is not required. Privacy, visibility, retention and auditing should remain
  configurable policies around that identity, with grants still authoritative.
  The owner's privacy/accountability range is an architectural requirement,
  not evidence that every policy mode is already implemented or verified.
  Mounted `human_principal/schema.json` corroborates the input contract:
  `register_key` requires only `human_pubkey`; `display_alias` is optional.
  Its write capability and the separate read capability on `resolve_key`
  remain enforced; this inspection did not register or resolve an identity.
- 3tched is owner-written code. The ZeroClaw-backed native integration exposes
  commands and flags as passthrough methods sealed in plugin blobs, preserving
  one schema authority and native command behavior.

Primary current record:
`/home/jeremy/.codex/sessions/2026/10/12/rollout-2026-10-12T06-12-38-01a12ebd-e120-78e3-b8d1-b06b4e92071e.jsonl`,
especially records 463, 489/493, 521, 557/568, 643 and subsequent owner turns.
The older AGENTS wording equating container with identity and FUSE with lungs
does not override these corrections.

## Corpus and coverage

| Client | Local coverage | Reading boundary |
| --- | --- | --- |
| Codex | 72 unique collector inputs: 34 home, 15 root, 23 retained; 16 threads with Oct 12 records including this root and its three workers | Relevant original conversation messages and selected actual tool receipts; included older-created sessions with today's turns |
| Claude | All 40 project JSONL files inventoried; one main thread and three subagents active today | Main conversation prose, question answers, relevant implementation/check receipts and final screenshot; repeated skill/source dumps excluded |
| Cursor Agent | 49 captured transcripts inventoried; four active today, 444 native records and 4,213 backing database blobs | All 140 visible dialogue messages and selected actual tool receipts; not every incidental tool/source dump |

Root-owned Codex originals were read through existing readable collector copies.
Claude's root home had no project JSONL files. Capture dates are not conversation
dates. Model groups can contain the same conversation more than once when its
model changes.

Cursor's private extraction is at
`/var/tmp/cursor-history-20261012-466geip6/{inventory.json,dialogue.md,tool-evidence.md}`.
It preserves original transcript paths/line locators and database row/blob IDs.
The collector inventory is
`/home/jeremy/.notebooklm-sources/.local-capture/plan.json`.
No authentication values were included in this report.

## Recovered chronology

### Claude: generated UI contract, 00:04–01:47

Primary record:
`/home/jeremy/.claude/projects/-srv-git-3tched-com/ead0c0ba-1d68-45cc-bcf0-d3d4deaad9c5.jsonl`.
It starts Oct 11 23:53 and ends Oct 12 01:47; Sonnet 5 changes to Opus 5.5 at 00:38.

- Records 254/292: the generated interface is part of the existing rendered
  surface, not an adjacent canvas or second dashboard.
- 366/381/397: no stubs; legacy pages are references; generate one page on
  demand when visited, from actual schemas and changing data.
- 456: add progressive streaming now. 519: consider multiple resolution
  strategies. 668: use native json-render mechanisms.
- 1312: explicitly approve interactive bindings, visibility, repetition,
  watchers, state and allowlisted actions, with matching backend validation.
- Tool receipts 2420/2431 show 88 door tests and 521 suite tests passed, one
  skipped. Build receipts 1906/2490 passed. Final type-check/lint were still
  blocked; no complete clean gate was established.
- HTTP 200 at 2444 did not prove route behavior: 2468 rendered a client 404.
  Served assets changed at 2521, but no publication command by Claude was found.
  The final screenshot showed only “Loading available models…”. Successful
  authenticated page generation remained unverified.

### Cursor: catalog, publication and installs, 01:50–06:25

Four threads: `7761512f-6617-4278-ad8a-fb4bb9827e1f` (recovery discussion),
`b0ed1c10-5ca9-4c91-86ce-e5176238678f` (UI),
`48417f79-c551-4905-8824-73c1294b28c5` (gallery audit), and
`a6222592-4faa-4421-8511-465a8373f670` (runtime installs).
The latter three record model `grok-4.7`.

- UI native records 94/101/108/114: old page content only, first prove one
  page, chat inside the canvas, and constrain the generated region.
- 129/144/161: register reusable missing components, accept renderer
  promotions, and reduce the active catalog to chatbot scaffolding.
- 295/307: explicit commit/push and main-branch authorization for Lovable.
  Actual tool results show commit `e741fe4` and successful branch/main pushes.
  Current local UI HEAD was independently observed at that commit on main.
- Extracted tool evidence lines 356, 480/645, 719 and 1087 support 526 tests
  passed/one skipped, publication, a served asset HTTP 200, and the main push.
  These are not authenticated interaction proof.
- The gallery audit's missing default path/no open descriptor was a snapshot,
  not proof that the gallery had never opened. The current implementation uses
  Cozo even though its default path ends in `ui-gallery.db`.
- Tool installation was explicitly authorized. Installed Zero/learning/MCP
  utilities and a listener are not proof of integration, weight training, or
  an internal MCP architecture. The later install thread was absent from the
  earlier grouped-sync plan but present in the 06:25 local capture.

### Codex: groups, subscriptions and local recovery, 01:11–06:10

The continuing Astra thread `01a129ee-7859-7cd0-8733-2c1ce831d046` was created
Oct 11 but has today's work. Retained model-gpt-6-astra chunks 14 and 16 carry
the original events and receipts. Tagged routing remained a shadow
implementation: no traffic steering, redaction, migrated fields or deployment
was established by that audit. OSCAL classification source still had producer-
annotation and initial-rule gaps.

Sol thread
`/home/jeremy/.codex/sessions/2026/10/12/rollout-2026-10-12T02-35-49-01a12df7-6264-7082-9ca5-8cd359396ee5.jsonl`
contains the controlling UI/group decisions:

- 434/676: umbrella role groups, not per-widget/per-principal grant patches.
- 450/465: exactly one ROOT chatbot; humans may be SUPERADMIN.
- 473/483/507/513: chatbot orchestrates; agents execute; UI worker remains
  confined to its view/chat.
- 615/629: UI-generation turns must not remember the previous turn; normal
  control-plane memory remains enabled.
- 517/545: no production internet, but retain development internet and the
  current chatbot identity during development.
- 765: menu available to ROOT and SUPERADMIN.
- 1069/1084: recover the local files NotebookLM syncs from.
- Final record 1702: the original ROOT/group paper remained unfound and
  role/menu implementation was pending.

The subscription repair has stronger evidence than a handoff claim:
`model-gpt-6-astra-576a599a__20261012-recovery__0016__66d49e827ee3aae8.txt`
under `.notebooklm-sources/.local-capture/chunks`, records 971/998/1046/1064,
shows received release `20261012T043625Z-bound-state-subscription`, activation,
72 byte-identical live/persisted/received blobs, 24 served file checks, and
37 UI plus 3 backend tests. It explicitly says the authenticated browser was
not exercised. Static generated-spec restrictions were still present.

Collector recovery receipts at `/var/tmp/notebook-local-recovery-20261012/`
support 63 groups, 247 chunks, 513 native files and a remote content readback.
The saved sync cycle was incomplete. The original role paper is still missing.

Previous generative thread `01a12e9e-3e05-7602-8572-200d54dad4b5` delivered
uncommitted discovery/graph, layout/rendering and supervisor modules through
three workers, with 22, 35 and 24 focused tests respectively. Integration
remained outstanding. At 05:41:32 the owner explicitly retired the saved
`cloud-recovery-40` target; its SSH failure is not a current blocker.

## Reconciliation with current source

- `src/svelte/Shell.svelte` and `src/json-render/pages/shell.pagespec.ts` in the
  UI already implement the shared shell and on-demand route state. Reuse that
  path; the new `src/generative/controller.ts` is not yet referenced by it.
- UI `canvas-contract.ts` / `reference-chat.ts` and backend
  `json_render_chat_canvas.rs` still reject dynamic state/actions/bindings.
  `chat-session.ts::generateView` waits for a complete response. These are
  concrete gaps against the approved interactive/progressive behavior.
- Gallery Cozo lives behind `mutation_engine.rs::ui_gallery_cozo` and defaults
  to `/var/lib/op-dbus/ui-gallery.db`. The separate private workspace uses
  `RuntimeDocuments`, default `/var/lib/op-dbus/plugin-runtime-cozo`.
- Native naming is distinct: service `tched-router`, executable
  `/usr/bin/threetched`, framework plugin `tched_router`, and binary-discovered
  plugin `threetched`. Upstream pins preserve ZeroClaw provenance. The
  `threetched.rs` plugin owns sealed admission and validates paths/digests before
  reconstructing native arguments. Direct binary execution inside FUSE's
  `run_threetched_call` is test-only; production uses the sealed backend route.
- `fuse_buffers.rs` charges bounded bytes/handles through RAII; `fuse_fs.rs`
  releases directory handles and retains in-flight charges after close. This
  is source inspection, not a new stress-test or leak-free-runtime claim.

## Grants and principal-specific execution paths

`interceptor.rs::load_capability_grants` expands ten closed groups and unions
their capabilities with direct principal grants. Groups cannot be callers or
nested groups. The checked-in grant document contains `$EXTERNAL-MCP` with
`cap.data.cozo.system-graph.read@v1` and no members. ROOT/SUPERADMIN principal
entries duplicate their groups' capability arrays: membership removal alone
therefore does not revoke those direct capabilities. Exactly-one-ROOT is not
enforced by the group validator. No sealed general group administration surface
was found. `op-web/groups_admin.rs` instead manages toolset profiles.

This is source state. An Oct 11 22:00 transcript receipt corroborates the then-
empty External MCP membership; today's live membership was not reread.

The code shares the `:8090` Tonic/Axum TCP listener and has distinct dispatch:

| Entry | Current path |
| --- | --- |
| FUSE operation file | Shared lungs socket, bound identity, D-Bus PluginV1 |
| External MCP | Private per-call D-Bus socket pair; not the shared lungs socket |
| Native gRPC CallMethod | Direct MutationEngine dispatch |

Relevant source: `server.rs:648,724,1261`, `mcp_dbus_dispatch.rs:88`,
`grpc_server.rs:1996`, `schema_router.rs:1034`, and
`threetched-fs/src/dispatch.rs:270`. The owner subsequently confirmed distinct
human/machine principal paths are intentional. External MCP's private D-Bus
pair has no listener and preserves the already-verified human identity without
substitution by a shared root sender. Native lungs uses a kernel-proven peer
and the bound native session. gRPC uses the shared dispatcher while carrying
its validated request identity/proof. Different transport plumbing alone does
not demonstrate another control plane or an authorization bypass. Verify
principal attribution, grants, sealed contracts and receipts for each path.

Current local Codex config contains the 3tched CLI `opdbus-context` adapter.
Current Cursor MCP config contains `ecc-memory-vault` and `kaeru`, but no
`opdbus-context`; older docs claiming both clients are configured are stale.
No new client connection, grant change or deployment was performed.

An earlier source-only proposal to consolidate all callers onto the literal
shared socket was based on an overly narrow reading of the one-door requirement.
It is withdrawn after the owner's human/machine-principal clarification; no
handoff API or dispatch change was implemented. In particular, replacing the
human path with host-root `Bind(session)` would misrepresent identity.

Source inspection observed that external MCP's `AuthenticatedCaller` retains
origin identity but not the original request proof expiry/digest, while gRPC
carries `BoundOracleProof`. Whether this violates the intended per-request or
session policy needs a separate targeted analysis; it is not proof that all
paths should be merged. The owner also flags possible deprecated interceptor
code. Trace actual route registration and running release provenance before
classifying a module as active or retired. No new tests were executed here.

Follow-up source tracing confirms the interceptor **module is active**:
`grpc_server.rs:1340` constructs `make_ghostbridge_interceptor` and attaches it
to canonical Tonic routes; `build.rs:321` uses the same factory for generated
plugin routes. External MCP intentionally uses `OracleHttpAuthenticator` with
the same serving `AssertionValidator`. The bare `ghostbridge_interceptor`
function at `interceptor.rs:570` is explicitly a legacy/test entry, so neither
its presence nor its name establishes a live path. Selected release
`20261012T043625Z-bound-state-subscription` matches the historical binary hash;
dirty current source and denied process-executable access prevent claiming
checkout-to-running-binary parity. No interceptor removal is warranted by this
audit.

Read-only kernel inventory in this shell currently shows only `10.0.0.3/32`
on `ovsbr0`; the second owner-described internal address is not visible here.
That discrepancy is recorded for reconciliation, not permission to add/remove
addresses or alter bridge ports. Current `deploy/config/network.conf:40` sets
only the MQTT alias; lines 43/47 reserve/comment the OpenFlow listen/controller
at `10.200.0.1:6653`, and `ovsbr0-cutover/run:81` explicitly leaves it inert.
This is a source/live mismatch with the newly restated two-address design.
Mounted 3tchedFS metadata reports 72 objects
and source catalog generation 2550; it is not proof of current selected-catalog
parity. No operation-file writes or identity changes were made in this audit.

## Work performed in this turn

Three parallel history workstreams covered Codex, Claude and Cursor; root
verified key primary records and reconciled current source. Required backend
skills were loaded by the source-reading workers. Root specifically applied
FUSE, kernel-FUSE diagnosis, Rust memory optimization and memory-safety guidance.

Earlier isolated frontend work in this turn has 12 connection tests, 34 runtime/
learning tests, and 4 Zero-adapter tests reported passing, with scoped checks.
Learning remains a session-only fixture, with no Cozo durability or weight
training. Zero 0.3.4 (5b3a90a) retains a limited-experiment recommendation;
concurrent graph edits demonstrated a lost update. None of this establishes
integrated product behavior or completion of OD-120. The history/source audit
ran no new application tests or builds and made no service/network/grant changes.

## Seed/leaf and build-capacity follow-up, 2026-10-12T07:05Z

The owner describes two concurrent incident factors: the loop-backed
send/receive workflow exposed the same filesystem UUID on host/container paths,
and Cargo builds exhausted the disk despite repeated requests for automatic
reclamation. This is the owner's account, not a newly established forensic
causal chain. The prior incident audit already recorded ENOSPC and missing
maintenance; do not reduce the incident to a UUID-only explanation.

Corroborating history is more specific than a general maintenance request:
SIGNALS.md's September 24 entries record 25 MB remaining during release receive,
a dormant reclaimer, and subsequent monitoring with no automatic deletion.
HANDOFF.md's October 4 01:24Z entry records 223,264,768 bytes available before
manual reclamation of 158.72 GiB from three inactive debug caches; the reclaimer
was still unscheduled. In Claude transcript
`438d838a-2c88-4fef-89b8-bfe5ec54e466.jsonl`, record 1254 at October 11 00:38:38Z
contains the owner's explicit two-factor account. Actual tool outputs at
records 1259 and 1275 show the stale reclaim path and then-current Cargo target
on the host main pool. They are post-recovery receipts, not a measurement of
100% usage at the original loss instant.

The intended lifecycle remains the owner-corrected contract in AGENTS.md: Cozo
owns identity-to-leaf/loop assignments; an immutable seed is shared read-only;
each identity has its own persistent writable leaf and the existing seven
subvolumes. Only explicit creation adds a blank device once. Runtime attachment
does not add/remove Btrfs members. Release send/receive is a separate lifecycle.

Fresh unprivileged kernel inventory (`lsblk`, `findmnt`, `losetup --list`, and
public `/sys/fs/btrfs/*/devices`) now observes:

| Storage | Current evidence |
| --- | --- |
| Host root | `/dev/sda3`, filesystem UUID `85316d90-025d-4b3d-9a52-a7d192301ff6` |
| Physical seed view | `/dev/loop2`, read-only loop backed by `/dev/sdc2`; lsblk reports UUID `3415d575-7224-4484-9e0d-06854ac5f032` |
| Chatbot root | Kernel Btrfs filesystem `30afd0a9-a81e-480f-89af-a72cc6aa88e9` lists `loop2` and `loop3p1`; loop3 is backed by `/var/lib/3tched-leaf-pool/3tchedChatbot.img` |
| Chatbot upgrade | `loop3p2`, filesystem UUID `f719b9cf-2160-46c8-b807-acada2bd2619` |
| Host upgrade | `/dev/sdc3`, filesystem UUID `5a88e7cd-9701-40a7-8301-ef076761886c` |
| Cargo target | `/dev/sdc1[/release-target]`, filesystem UUID `99c6c7c6-7167-43f6-93e8-76416c0940f5`, mounted at the backend's `target/` |

The chatbot has visible `@`, `@agent_memory`, `@agent_rules`, `@agent_shared`,
`@agent_skills`, `@agent_soul`, and `@agent_user` mounts. The seed-layer view is
read-only; memory/rules/skills/user are writable, shared/soul read-only. Persist
maps below `@agent_user/persist`. These mounts are also visible under the Incus
container root. This corroborates the mounted arrangement, not restart
durability, Cozo record correctness, or safe future device lifecycle behavior.

The current `identity_clean_provision.rs:115-135` version-4 source pins the same
image/mount paths and requires unique canonical UUID values across the disk,
two partitions, two upgrade filesystems, seed and writable-root filesystem.
That source guard is separately observed; it was not executed in this audit.
`identity_clean_provision/storage.rs:259,328` checks the seed's block read-only
status, pinned leaf inode/size, root FSID and exact two-member seed/leaf pairing.
`deploy/scripts/prepare-instance-disk.sh:75,110` requires a fresh creation receipt
before its single device-add phase. These are source controls, not a replay or
new validation of provisioning.

Cozo stores the identity-to-storage mapping: `op-cozo-store/src/lib.rs:491`
defines `identity_sessions`, including `btrfs_device_json` and `instance_json`,
with separate genesis and revision relations. The private store is initialized
through `identity_publication.rs:91`, defaulting to
`/var/lib/op-dbus/identity-cozo`. This does not establish that Cozo's own database
lives inside each container leaf. Worker inspection found start/stop only
verifies storage and changes logical attachment; no source change was made.

Active host, chatbot root, and upgrade filesystems have distinct observed IDs.
The retained recovery image devices still expose old UUID
`82978282-f6d6-4c74-862a-6ab261874c74`; repeated UUID text alone does not establish
another collision. The October 11 memory note's seed UUID is historical and
does not describe today's physical seed view.

Current `df -hT` reports host root 46% used (347 GiB available), Cargo target
5% used (661 GiB available), and recovery storage 70% used (455 GiB available).
The owner confirms that builds now run on the separate disk's subvolume and
have run many times over the past week, including many today. Today's mounted
`/dev/sdc1[/release-target]` corroborates the current placement. Historical
main-pool exhaustion must not be described as today's build layout or as proof
that current builds fail. Statements that this audit ran no builds concern
only this read-only follow-up, not other agents' work or prior build receipts.
Free space and a separate target mount do not establish automatic reclamation.
Current inspection also corrects any blanket assertion that no automatic
deletion is scheduled now: `/etc/cron.d/3tched-disk-retention` schedules
`deploy/scripts/disk-retention.sh` daily at 03:15. The script retains the selected
release plus two other releases, then ages files under `/var/cache/op-dbus` and
`target/debug/incremental`. It contains no build-capacity threshold/reservation
or active-Cargo locking and does not cover all Cargo target locations. No
successful-run receipt was established. The older stale `disk-reclaim` helper,
historical monitoring-only state, current retention job, and requested automatic
build-space reclamation are distinct; do not conflate them.
Direct `btrfs filesystem show` for the chatbot root/upgrade was denied at the
storage path and block devices. No privilege escalation or alternative identity
was used. Live Cozo assignment verification remains incomplete.

In response to the owner's status question: preliminary UI discovery,
learning/runtime, and Zero experiment code was written earlier and remains
unintegrated. The active three worker assignments are now read-only Cozo
lifecycle, Cargo cleanup, and incident-history audits. No Cargo builds,
deletions, mounts, device membership changes, or recovery actions were run.

## 2026-10-12T07:50Z — Demo focus, intended evidence lifecycle, and Luna builds

Task `generative-learning-zero-20261012` / OD-120; integration owner Codex
`/root`. This follow-up supersedes the earlier active storage-audit status.
Sources are the owner's subsequent conversation, bounded source inspections by
`/root/learning_runtime` and `/root/live_connection`, and the build receipts below.

### Current owner priorities

The immediate product focus is **two demos**: Accountability, where the user
retrieves attributable mutation evidence and questions the chatbot about who,
what, when, how and why; and document redaction through schema-tagged routing,
where the caller's identity and applicable policy govern released content.
Normalize grants and capabilities across existing groups as their foundation,
preserving the original ROOT/SUPERADMIN/external-client boundaries. This is not
authorization to broaden membership or capabilities.

The UI generator serves these demos and broader information-management systems.
The owner reports a successful roughly ten-minute schema-only drop-in CMS
experiment and earlier generated businesses across industries, including pet
stores and electric-car charging. Slack-like systems and a healthcare front
desk are further use cases. An approximately one-hour generated healthcare
application is a future target, not a measured delivery or compliance result.
Show the actual applicable controls, policy versions, enforcement decisions and
evidence for any demonstrated application. The owner's EU positioning is that
the same evidence can help both enforcement and audited organizations; no
particular EU regulation, legal conformance or certification was verified here.

Zero is explicitly deferred by the owner ("not at this moment"). Preserve the
isolated experiment, but adoption, a rewrite or further benchmarking is not on
the demo's critical path. Funding is intended to enable expert help. The public
YC application page checked this session lists the Winter 2027 deadline as
November 2, 2026 at 8pm Pacific: https://www.ycombinator.com/apply. No application
or contact was submitted.

### Architecture corrections and evidence boundaries

The owner's historical sequence is privacy network, MCP Linux-control server,
schema as code, OSCAL, sealed blobs, then FUSEFS. OSCAL, sub-IDs and schema-tagged
routing are foundational. Live operational state is SHM and committed changes
wake consumers through existing projections. Do not introduce polling or a
second state authority. Intentional Btrfs uses include transport, cache and
persistent storage; their unconventional nature alone is not a defect.

The intended mutation trail is immutable and hashed, vectorized on arrival,
and transferred as timing/vector Btrfs snapshots to production off-site Qdrant.
It supports both chatbot memory and user Accountability. Source establishes
pieces of this path: `event_chain.rs:10`, `mutation_engine.rs:2075,2223`,
`chain_vectors.rs:349`, `op-snowball/src/snowball.rs:737`, and
`op-snowball/src/replication.rs:316`. The receiver path in
`op-chain-vector-ingest.rs:19` / `chain_vectors.rs:507` ingests received vectors
without embedding again. The bounded inspection did not establish a production
caller of `dump_vectors`, off-site delivery, or universal mutation coverage.
Legacy best-effort persistence paths remain. Do not convert these source
observations into a claim that the complete production flow was verified.

The owner separately describes a **future** full system/dependency JSON record,
preserved whole and vectorized, for reconstruction and software reinstallation
through D-Bus/PackageKit. Whole JSON supplies reconstruction input; semantic
vectors support retrieval. This recovery feature is not currently complete.

The read-only Accountability review reconfirmed:

- UI `src/svelte/AccountabilityEvidence.svelte:18` explicitly reports
  "Not connected" and keeps local query drafts. It does not retrieve evidence
  or confront the chatbot; its Control plane guidance differs from the backend
  Accountability-mode gate.
- Backend `mutation_engine.rs:5111` dispatches the evidence-search branch;
  `accountability_search.rs:11` checks verified session, current capability and
  Accountability mode, canonicalizes same-actor/session records, and rechecks
  authorization before disclosure. This is checkout source, not proof of a
  current sealed contract or live deployment. Cross-actor access is not implied.
- `mutation_engine.rs:5691` uses a bounded recalled context bundle, but
  `context_bundle.rs:227` retrieves scoped cognitive memory rather than this
  audit-search path. Evidence retrieval, citations and chatbot confrontation
  remain an unconnected end-to-end workflow. No model weights were trained.

### Compliance corpus recovery boundary

The owner reports roughly 75 GB of previously vectorized official documents,
GitHub material and compliance policies/rules. Sources are recoverable; vector
recomputation time and cost are the concern. The owner is handling backup
recovery personally. No backup mount, restore, pruning, collection reset,
embedding job or paid indexing was performed by this workstream.

`/srv/git/repos-bulk/AGENTS.md:15` identifies independent `compliance-index` as
authoritative; legacy vector scripts and handoffs are historical. The maintained
package is absent at its recorded current and `1stRun` paths, which is not proof
that all backups or vectors are lost. Surviving
`reports/compliance-scaffold/20260930-discovery/contracts.md` pins package commit
`c651f896472f963a37754bcc886f9fe85eebf6cc`, NIST 5.2.0 and generation
`38cdcec0a21a316013fcc13efe8682347fceb628a2b5be362ab21197c08f7173`.
Those are historical fixtures, not current coverage. Use source mappings before
semantic retrieval and retain active-generation/group scope; corpus groups are
not principal grant groups.

Recover the maintained package, `.state`, source locks, response caches and
compatible vectors before choosing a resume operation. Compatibility requires
source revisions/hashes, extraction/chunk rules, context grouping, exact model
and dimensions, point IDs and checkpoints. Historical `compliance_context_v1`
and October 7 ledger counts do not establish current recoverable contents.
No current embedding-resume command is established. Audit-chain vectors and
this regulatory corpus are separate. A demo may use a disclosed real policy
slice; it must not imply that the complete corpus has been restored.

### Build receipts and current execution boundary

The owner authorizes builds and routes GPT/Codex Cargo work to Luna; Claude to
Haiku and Grok to Cursor Composer 2.5. Tests are deferred until after deployment
during the owner's downtime. The owner reports an idle-triggered Luna launcher;
this workstream did not inspect or change it. `/root/luna_build` used the existing
Cargo dispatch lock and performed builds only.

| Build | Receipt | Result |
| --- | --- | --- |
| Backend workspace release, GNU target | `/var/tmp/luna-build-20261012-071634-backend-isolated.log` | Exit 0; 07:16:36Z–07:48:07Z; Cargo reports 31m26s |
| UI production build in private source snapshot | `/var/tmp/luna-build-20261012-073028-ui-snapshot.log` | Exit 0; 07:30:32Z–07:30:44Z; output under `/srv/git/odbus-verified/target/luna-ui-snapshot-20261012-073028/dist` |

Backend HEAD was `34ff9a502d953bb7b1520d530eed849b507e266a`. Before/after dirty
fingerprints matched:
`d06b3f31fcb1a01787b7e74924613eaaaf176114f264d497877d24adc3568515`.
The exact command was:

```sh
CARGO_CALLER_FAMILY=gpt CARGO_DISPATCHED=1 \
  CARGO_TARGET_DIR=/srv/git/odbus-verified/target/release \
  CXXFLAGS='-include cstdint' \
  cargo build --workspace --release --target x86_64-unknown-linux-gnu --locked
```

The initial default-target attempt exited 101 on a root-owned target directory
before compilation (log `/var/tmp/luna-build-20261012-071516-backend.log`). The
successful retry used a writable location on the same separate build disk; no
permissions were changed. Backend lifetime/dead-code warnings remain.

UI HEAD was `e741fe47dd7c36ebd5310a14f698d9ebf756a5e0`; original/snapshot source
fingerprints matched:
`dc54af8ad2606314c970d0e62b0c701a768dfa040fa15d1ae7cb3c4a887fe03f`.
The original checkout build exited 1 on protected `.svelte-kit/output` removal
(log `/var/tmp/luna-build-20261012-072657-ui.log`). A private snapshot on the
separate disk reused installed dependencies and passed `npm run build`, with
Svelte state-reference and large-chunk warnings. No dependency install occurred.

Root inspected both successful logs. These are build receipts, not end-to-end
demo, test-suite, seal or deployment results. No tests, lint, formatting, source
edits, cleanup, grants, service changes, reseal or deployment occurred in the
build worker. Documentation was deliberately deferred until it finished.
Next implementation step: reconcile current live sealed evidence/routing
contracts and existing OD-119 group ownership, then connect the two demos to
the existing chat catalog and adapter without inventing another authority.

## 2026-10-12T07:54Z — Sole ROOT and desktop continuation

The owner explicitly reiterates: the control-plane chatbot is the **only ROOT**.
It may observe, research, recommend and delegate; it is not allowed to execute
operations itself. Delegated agents perform execution within existing grants.
Do not interpret ROOT as unrestricted execution authority, give delegated
workers ROOT, or change grants based on this documentation alone. This is the
owner's required architecture, not a fresh verification of its enforcement.

The owner is moving the conversation to GPT desktop. Resume from the OD-120 row
and latest entries in `HANDOFF.md`, this report and the UI architecture ledger.
The main backend remains `/srv/git/odbus-verified`; the UI is
`/srv/git/3tched.com`. Builds have passed, no deployment was performed, tests
remain deferred to the owner's downtime, and Zero is deferred. Preserve the
two-demo focus, existing OD-119 group ownership and the owner's backup recovery.

Owner follow-up: Accountability depends on vectors. Its semantic retrieval needs
searchable audit vectors linked to canonical mutation records; establish their
availability/recovery before presenting UI wiring as the remaining prerequisite.
Do not conflate this with rebuilding the entire regulatory corpus or assume
the vectors are all lost. Owner-led recovery and provenance-compatible reuse
come first. Cost and time are explicit constraints: keep work narrowly focused,
use Luna for authorized builds/tests, and require explicit authorization for
paid embedding or training jobs. No such jobs have been started.

## 2026-10-12 — Generative interaction clarification

The owner clarifies the original generator experiment: supply schema/data with
as little or as much semantic context as deliberately selected, and ask the
model to make it useful and accessible across people and industries. Leave room
for unexpected functionality, relationships, workflows and ways of interpreting
information. Cosmetic variation is insufficient. Do not predesign pages, map
the whole site or choose the business for the model. Later clarification allows
owner-provided references for content and structural relationships: strip their
visual design, supply content JSON and leave the new composition to the model.
Generate only the requested surfaces in normal use. A long exploration run may
retain many outputs for comparison, with explicit finite budgets.

UI generation is fresh on each attempt: no unrequested UI-turn memory, automatic
recall of prior designs, or promotion of earlier results into its context.
The owner may explicitly vary schema-only, data and semantic inputs. Keep normal
control-plane memory separate. Saved candidates/evaluations need not be fed
back into the generator. The six-hour run described by the owner is illustrative,
not execution or spending authorization.

Owner reports a single canvas with the chat panel inside it; no fresh visual
verification occurred here. Screens are the current output, not a fundamental
limit on the larger interaction concept. Existing Svelte/catalog/one-door rules
remain applicable to implementation. Schema-only experiments do not depend on
recovering audit vectors; semantic Accountability does.

The broader aim includes community: different accessible presentations of shared
data may help people understand each other and approach issues with less
polarization. Preserve evidence, context, provenance and access boundaries.
This is an owner-stated aspiration, not a measured effect. The UI architecture
ledger now distinguishes model-led functional exploration from the earlier
optional deterministic layout baseline. No application behavior changed here.

## 2026-10-12 — Saved Btrfs generation and richer interaction loop

Root and `/root/learning_runtime` inspected all eight messages and rendered DOM
in `/home/jeremy/Desktop/GLM5.3BTRFintrface.html`, SHA256
`8897c656f1a91026c4957fb73f449a3861ee2d2b9e19f84cbf41ba18f3432c25`.
Its two CSS files retain the hashes recorded in HANDOFF's02:50Z audit. No scripts
or external requests were executed. Root loaded Svelte, design-system and
Playwright skills; the worker loaded Svelte/core. Root's browser inventory was
empty and installed Playwright/Chromium unavailable, so this is DOM/content/CSS
inspection, not fresh visual verification or an interaction test.

The owner's initial prompt asks for an interface to maintain thousands of Btrfs
subvolumes based on the system architecture. One response produces three actual
result blocks: fleet console, grouped explorer, snapshot/lifecycle policy
manager. The first three contain19 disabled form controls across three disabled
fieldsets, plus enabled tab/gallery controls. Their runtime behavior was not
exercised. The requested raw-JSON follow-up changes content (including the fleet
count from3,842 to4,812) and produces three additional rendered results, not a
verbatim export. The final exchange proposes action/schema declarations after
the owner points out that operations should already be in the schema.

Fresh direct read of mounted `/run/mount/3tchedFS/plugins/btrfs/schema.json`
confirms btrfs2.0.0 with12 methods, including subvolume create/delete, snapshot,
balance and scrub, with required capabilities. The captured model's blanket
claim that mutations do not exist is not reliable infrastructure evidence.
Current dynamic state reads return EIO; exact historical grants remain unknown.
No permission or operation was changed. Prior source findings about thirteen
model-exposed components and static-prop validation describe that inspected
path, not proof that generation was prebuilt or every runtime path lacks
interaction. Preserve the successfully generated design as the baseline.

Owner clarifies references and steering: example layouts may be inspected for
content/structure, then stripped to content JSON so the model chooses a new
design. Deliberate feedback, other people's critique and requested research
are compatible with fresh generation. The two-week remark was illustrative.

The generator belongs under the sole ROOT control-plane chatbot's orchestration;
execution remains delegated under existing grants. Schema declarations contain
the information to find and invoke an operation; they are not the transport.
Rich sealed blobs and gRPC payload metadata feed session-linked Snowball ledger
events and on-the-fly vectorization. The owner describes contextual error
navigation, deep drill-down and operations in place as uses of that full loop.
This records the intended design and observed source/artifact boundaries, not
verification of every payload field, execution path or vector delivery.

## 2026-10-12 — Groups prioritized; provider/model responsibility clarified

Owner says the Btrfs incident is resolved: lost data remains a recovery concern,
but the system is running and the recurrence fixes have been made. Do not keep
treating that incident as the active blocker. The already-authored Btrfs cap
patch remains source-only, unsealed and undeployed; further storage work stopped.
Priority is consistent groups, grants and capabilities, supporting Accountability,
tagged redaction and a verified demo by November 2. Vector recovery/indexing stays
owner-controlled; no embedding jobs were started.

Current closed group names are `$ROOT`, `$SUPERADMIN`, `$ADMIN`, `$AGENT-L1`
through `$AGENT-L4`, `$EXTERNAL-MCP`, `$VENDOR` and `$GUEST`. There is no separate
partner alias and no implicit hierarchy. Valid membership unions group grants
with explicit principal grants. Checked-in ROOT/SUPERADMIN entries duplicate
their group capabilities; membership removal alone cannot revoke those direct
exceptions. The remaining groups currently share a single graph-read default.
This is source inventory, not confirmation of live memberships or a decision
about the intended distinctions between agent levels.

Root implemented a shared parsed-document validator in
`crates/op-grpc-bridge/src/interceptor.rs`: durable publication and runtime
expansion now apply the same structural checks. Previously runtime expansion
could salvage string capabilities from malformed arrays rejected by the
materializer. Valid entitlements and group names are unchanged. The existing
subscription edits in that file are preserved. The learning_runtime worker
authored all-ten-group, 25 malformed-document, revocation/direct-exception and
recovery coverage in the existing isolated lifecycle harness. No tests have
run for this change. Luna's focused GNU release build is in progress under
`/var/tmp/luna-grant-build-20261012-084612.log`.

The owner's responsibility boundary is **backend providers, frontend models**.
Backend owns the designated provider endpoints and protected execution/auth
configuration. Frontend requests a fresh model list on selector opening,
presents returned metadata and passes selection into the chatbot/runtime flow.
The chatbot directs supported requests using the available model information;
do not replace this with a newly invented provider routing table or send full
execution references to an inference provider.

Runtime access and provider-bound context are separate. Source confirms
`tched_router.native_config` is excluded from public state and gated by the
existing config-read capability; secret redaction follows its sealed schema.
The discovery executor consumes protected endpoint/auth/config inputs directly.
The separately declared **public** model_registry guide is deliberately appended
to Chat; it is not the private runtime reference. This distinction was already
recorded in HANDOFF's October 11 18:11 entry and must not be collapsed again.
Inspected source does not automatically serialize the full runtime config into
Chat. No secret values or live private configuration were read in this audit.

Current UI source refreshes at startup and by explicit Refresh button, but
RouteFields had no selector-opening trigger. The live_connection worker owns a
bounded fix through the existing deduplicated refresh path in both pickers,
plus deferred interaction regressions. No new endpoint, polling, credential,
grant or selection-apply behavior is authorized by this change. Exact current
served-source parity and authenticated browser behavior remain unverified.

### Follow-up: existing routing, EMQX and execution evidence

Owner attributes the main recent implementation to Ultra; Luna was its delegated
classification/audit worker. Root re-read current source and the retained parent
session (`01a129ee-7859-7cd0-8733-2c1ce831d046`), including records17626,17679,
17725,18105 and18363 from October12. These confirm the shared OSCAL assessment
implementation, scoped compile corrections and 20-plugin audit contribution.
Do not summarize this as merely a worker inventory or create a second taxonomy.
The current policy matrix in `reports/group-capability-matrix-20261012.md` is a
proposed extension to that existing chain, not a replacement authority.

Root corrects an earlier conflation: `evaluate_gate` is pure and writes nothing.
The distinct, uncommitted `routing_decision.record_shadow_decision` method builds
and persists an evaluation record through its own service. It is not the name
or definition of the lungs/security door. Its current implementation evaluates
a proposal without executing its target; it does not implement a paired human
decision versus machine recommendation. Owner rejected speculation about that
interpretation. No shadow feature was added, removed or activated by this turn.

The broader event path includes EMQX. Authenticated ExHook events call
`record_event` and `process_authoritative_change`, entering MutationEngine's
hashchain. Broker topic admission is in `wake_doorbell`; matching publish triggers
go through the queue, allowlist, sealed capability resolution and identity-bound
canonical dispatch. Method receipt audit records an attempt before dispatch and
a linked terminal event afterward. Chain vector projection consumes sealed
method/signal subids and declared fields with private/secret exclusions.
These are connected source paths, not freshly exercised live-event proof.

The new OSCAL field-assessment consumer was not found in the inspected broker
hooks or ordinary target-dispatch paths; existing identity/topic/capability and
Cozo policy gates do exist. Missing callers of the new recheck helper do not
establish that existing door security is absent. At fresh mounted-declaration
read, `oscal_subid_registry` is2.0.0 without `assess_fields`; `emqx` is2.1.0;
`routing_decision` and `schema_tag_registry` schemas were not readable in this
namespace. Do not turn this scoped observation into global deployment proof.

Owner additionally identifies the execution tracker, restored in recent work,
as the existing claim-versus-action verifier. Every response is intended to be
a forced response-tool call checked against actual execution evidence before
publication. Trace the current chat and response paths before claiming that
older op-chat implementations prove the live integration or adding a duplicate
tracker. This follow-up audit is read-only; its source findings follow.

### Current response and mode trace — source evidence, not live verification

The follow-up found durable method attempt and terminal receipts in
`method_receipt_audit.rs:495,657`, with identity-bound recovery at714. Those
receipts describe handler outcomes; they are not universal independent
postcondition readback. The current `chat_response` dispatch validates content,
canvas and turn ownership, but `related_actions` is a bounded list of names;
this inspected path does not resolve those names to successful execution
receipts before publication. Current UI Chat consumes `ChatOutput.content`
directly (`3tched.com/src/chat/door-chat-adapter.ts:451`).

Control Plane is a distinct native A2A agent path, not provider-only generation
(`control_plane_routing.rs:70`). Its bridge return extracts text
(`tched_router_runtime.rs:1833`); native-agent-side response-tool enforcement
was not established by this bounded trace. Other current modes use stateless
generation. Do not generalize their no-tools implementation to Control Plane.
The older `op-chat` forced-execution implementation is not proof of enforcement
in either current path. The owner's reported recent restoration remains
unmatched to a verified current publication path; no duplicate tracker was
added and no claim is made that the running native agent lacks it.

Owner distinguishes Control Plane's own rules/logs/vectors, Personal with soul
and private memory namespaces, shared member-invited Projects, Accountability,
and the UI generator. Personal tracking was explicitly uncertain; preserve
its privacy rather than treating conversational content as public evidence.
Current `project_dispatch.rs:445` and `project.rs:45` resolve these namespaces:

| Mode | Current memory namespace | Observed boundary |
| --- | --- | --- |
| Control Plane | `session/{session}/control_plane` | Explicit control-plane grant; native A2A routing |
| Personal | `session/{session}/personal[/name]` | Limited memory capabilities; personal sub-namespace validation |
| Project | `project/{id}` | Membership, project role and capability intersection |
| Accountability | `session/{session}/accountability` | Read-only mode; evidence search has additional admission |
| NotebookLM | `session/{session}/notebooklm/{selected notebook}` | Selection required |
| UI renderer | none | Skips memory recall and completed-chat episode write; authorization/audit still applies |

Namespace memory/semantic recall is partitioned. Episode history has a narrower
separation guarantee: `EpisodeScope` in
`op-cozo-store/src/ctl_plane_chatbot_episodes.rs:18` contains principal, session
and genesis, without mode/project/personal partition. Every memory-bearing
mode records a completed episode (`mutation_engine.rs:5764`), and
`context_bundle.rs:155` retrieves episodes by that identity scope without a
mode filter. Thus Personal currently has private episode persistence; separate
mode namespaces alone do not prove separate transcript recall. No live leak or
cross-user access was observed.

The completed-chat episode builder sets `pii_flagged=true` and starts without
an embedding. The dedicated `plan_vectorization` path omits goal, reasoning
summary and decision output when flagged. Those fields are also declared
private in the episode schema. Follow-up confirms the chain projector selects
the signal's `ReasoningEpisode` schema and `described_fields` skips private
properties (`op-state-store/src/schema_projection.rs:152`). Its old fixture at
`chain_vector_projector.rs:379` still expects those texts and is stale against
the current schema; it was not executed. Both inspected current-source paths
exclude these texts. Production projection depends on the mounted pinned
schema, and neither that deployed exclusion nor historical vectors were
verified. This does not establish personal-text vector leakage. No embedding
job was started.

Owner's banner report, “72 contracts, 3 current state projections,” describes
the three stateful feeds present in chat, not a new failure report. Root's
initial diagnostic interpretation is superseded. Source counts nonremoved
feed entries and those with a state path matching the current schema hash;
this is not a measure of executed operations or tracker verification. No feed
change was made.

### Implementation and build receipt update

The model-menu opening fix is authored in RouteFields, ModelPicker and
ControlPlanePicker, with 11 deferred regression cases. It uses the existing
refresh/coalescing path and preserves draft/applied selection. The shared
grant-validator fix and its isolated lifecycle regressions are also authored;
valid group entitlements remain unchanged. An independent static review found
no defect in the shared validator. Tests remain unrun by owner direction.

Luna's fresh UI production build exited0 at2026-10-12T08:56:23Z from
`/srv/git/odbus-verified/target/luna-ui-snapshot-20261012-085526`.
Source/snapshot fingerprint stayed
`119f32b76126a462afeb53cc2550c9bf744d6efa1cd4197cb02e4737c85223da`.
Log: `/var/tmp/luna-grant-build-20261012-085526-ui.log`; existing Svelte capture
and large-chunk warnings remain. After the conversation interruption, Luna
found no remaining focused-build process. The084612log ends at bridge warnings
without a Cargo finish line, exit status or final receipt. The backend result
is incomplete/unverified, not passed; it was not restarted. Current source
hashes still match the pre-build hashes:

- interceptor.rs: `9e76c2564283bbd737ac2e47fc7b80b1e3c585aab3657de28715d9e7d6166669`
- btrfs_plugin.rs: `17e30643569a7c0a888626210bc0e0c4d8843edcd250e225424661473eed961b`

No current change has been sealed, deployed or live-tested. Earlier successful
builds do not substitute for this missing completion receipt.

Owner's latest privacy clarification: personal data persists on the user's
own persistent storage and Cozo silo. Privacy here means owned and isolated,
not absent persistence. Do not disable personal memory or infer that personal
content belongs in shared accountability or learning. Current identity-scoped
storage supports part of that boundary; cross-mode recall and deployed policy
remain distinct verification questions.

Owner reiterates that lungs has no IP address and is a local socket. Fresh
source confirms `DEFAULT_LUNGS_SOCKET=/run/opdbus/lungs/lungs.sock` and
UnixListener binding in `lungs_socket.rs`, with UnixStream use by FUSE dispatch.
Network layering/OpenFlow must not be described as a directly addressable
lungs listener. Lack of an IP listener reduces direct reachability; it does
not itself establish absence of vulnerabilities in admitted callers, socket
message handling or operation authorization. This is a boundary distinction,
not a finding of an exploitable live defect or authorization to scan the host.
