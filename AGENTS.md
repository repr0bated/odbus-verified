# AGENTS.md

**Owner authority correction (2026-10-30):** newer source is authoritative; hackathon code takes precedence over older architectural prose. PR #46 contains hackathon HEAD `2a6e40e` and its newer bridge rewrite. Determine relevance from current module graphs and callers. Do not resurrect removed or unlinked implementations to satisfy historical review comments; repair independently demonstrated defects in retained code. This does not authorize deployment or changes to live identities, grants or storage.

This file provides guidance to the AI agent when working with code in this repository.

## Mandatory skill preload — every agent and model

At the start of every session, before any analysis, planning, edits, or subagent work, read these files completely:

- `.agents/skills/grpc-expert/SKILL.md`
- `.agents/skills/grpc-protocol-expert/SKILL.md`
- `.agents/skills/json-render/SKILL.md`
- `.agents/skills/ovs-db-analysis/SKILL.md`
- `.agents/skills/kernel-fuse-diagnosis/SKILL.md`
- `.agents/skills/fuse/SKILL.md`
- `.agents/skills/btrfs/SKILL.md`
- `.agents/skills/incus/SKILL.md`
- `.agents/skills/rust-memory-optimization/SKILL.md`
- `.agents/skills/memory-safety-patterns/SKILL.md`
- `.agents/skills/requesting-code-review/SKILL.md`
- `.agents/skills/requesting-code-review/code-reviewer.md`
- `.agents/skills/receiving-code-review/SKILL.md`
- `.agents/skills/quality-auditor/SKILL.md`
- `.agents/skills/quality-auditor/references/verification-gap-protocol.md`
- `.agents/skills/quality-auditor/references/audit-rubric.md`
- `.agents/skills/quality-auditor/references/anti-patterns-guide.md`
- `.agents/skills/verification-before-completion/SKILL.md`
- `.agents/skills/agent-self-evaluation/SKILL.md`
- `.agents/skills/application-security/SKILL.md`
- `.agents/skills/rust/SKILL.md`
- `.agents/skills/rust-patterns/SKILL.md`
- `.agents/skills/rust-testing/SKILL.md`
- `.agents/skills/svelte/SKILL.md`
- `.agents/skills/svelte-code-writer/SKILL.md`
- `.agents/skills/web-design-guidelines/SKILL.md`
- `.agents/skills/unified-memory/SKILL.md`
- `.agents/skills/using-superpowers/SKILL.md`

Mandatory even when the task does not mention these domains. Every subagent and model handoff must repeat the preload for its own session — a parent's preload does not count for a child. For OP-DBUS PluginSchema, generated plugin services, reflection, and seal/freeze/hot work, `grpc-expert` takes precedence over the generic `grpc-protocol-expert`; use the generic skill for protocol, channel, TLS, streaming, and observability concerns. For a live 3tchedFS mount failure, EIO, hang, or kernel FUSE diagnosis, `kernel-fuse-diagnosis` takes precedence over the generic `fuse` skill; use `fuse` for libfuse userspace API, callbacks, and mount debugging. Use `btrfs` for the designed cache subvolumes, snapshots, and send/receive. For the mount's 256-slot table, byte budget, `Drop` charges, arenas, and Rust allocation, `rust-memory-optimization` takes precedence over the general `memory-safety-patterns` skill; use `memory-safety-patterns` for cross-language ownership, RAII, and resource lifetime.

For Incus instance/profile configuration, disk declarations and managed storage volumes, read `incus` before applying lower-level Btrfs operations. Its Btrfs container filesystem volumes are subvolumes; VM disks use backing files. Distinguish a subvolume containing a partitioned disk image from a subvolume used directly as a filesystem, and distinguish pool-level backing from per-instance storage. Do not assume a native volume declaration implements an external seeded-leaf/GPT layout. The skill is project-local guidance sourced from official Incus documentation; it does not authorize bypassing identity-bound provisioning.

The container is the identity. 3tchedFS is its lungs: the local, typed
filesystem interface projected from the sealed blobs and live SHM state.

**Owner naming distinction (2026-10-18):** our filesystem is `3tched-fs`
(Cargo package `tched-fs`, plugin `tched_fs`, retained source directory
`crates/threetched-fs`). The external ZeroClaw-derived executable and surface
are `threetched`. Host views are separated: `/run/mounts/3tchedFS` exposes our
OP-DBUS plugins; `/run/mounts/threetched` exposes `threetched`, `tched_router`,
`tched_plugins` and `tched_skills`. Both views use our FUSE engine and preserve
the same sealed catalog and identity enforcement. Source definitions do not
establish that either mount is currently deployed; verify runtime receipts.

- Begin with the identity's mounted 3tchedFS namespace.
- Use its declared state and operation files for supported actions.
- Discover operation arguments, results, and errors through that namespace;
  do not invent parallel API clients, endpoints, or schema definitions.
- Preserve identity and capability enforcement through every operation.
  Local filesystem access does not remove the underlying permission boundary.
- Verify completion through the operation receipt and resulting authoritative
  state, not merely a successful local write.
- If a required surface is absent or denied, diagnose the projection,
  capability, or backing implementation. Do not route around the identity.
- Use underlying D-Bus/RPC interfaces only when explicitly working on or
  diagnosing that infrastructure through its supported maintenance path.
## Agent operating contract

### Scope and execution
- Identify the requested outcome, affected components, and evidence needed
  to show completion before editing.
- Maintain one shared task with clear file ownership when delegating.
  Subagents inherit the same scope and permission limits.
- Complete authorized work autonomously. Do not ask permission for routine
  reads, isolated tests, reversible edits, or implementation choices.
- Follow demonstrated dependencies. Do not expand a repair into unrelated
  refactoring, new infrastructure, or speculative optimization.
- When new evidence expands the scope, state why and update the task.

### Identity, permissions, and grants
- Use the identity and supported control-plane entry point assigned to
  the task. Preserve caller identity through delegated work.
- Existing authorization persists; do not ask for the same approval twice.
- A denied operation is evidence of a permission boundary. Record the
  operation, caller, exact missing capability or policy, and error.
- Do not bypass a denial through another identity, direct database edits,
  alternate transports, or privileged host commands.
- Do not broaden grants, weaken autonomy/security policies, expose secrets,
  or change unrelated identities to make a task pass.
- If a permission change is necessary, first prepare the smallest concrete
  change: affected principal, capability, resource scope, duration, expected
  effect, verification, and rollback. Request approval only for the part
  not already explicitly authorized.

### Evidence and verification
- Distinguish proposed, implemented, tested, and deployed work.
- Verify the complete affected workflow, including failure and cancellation
  paths where relevant. A successful component test is not end-to-end proof.
- Use disposable fixtures and bounded workloads. Preserve authoritative
  data and unrelated work.
- Report failed or unavailable checks honestly. Do not convert missing
  coverage into a pass or repeatedly run unchanged failing checks.
- Prefer the smallest evidence-backed fix. Add technology only when a
  measured problem justifies its complexity.

### Shared memory and handoff
- Sync decisions, conversations, and tool results to the shared repository
  with agent identity, timestamps, task ID, and source references.
- Use NotebookLM to recover context. Verify current code, permissions,
  configuration, and deployment state against their authoritative sources.
- Record the current commit, modified files, validation results, unresolved
  blockers, and exact next action. Update stale conclusions explicitly.
- Never describe planned work or another agent's claim as completed work.

### Completion
- Finish with the outcome, changes, verification, remaining limitations,
  and deployment status.
- Stop when the requested outcome is verified; do not continue into
  speculative optimization or unrelated cleanup.


### Memory routing and authority
- Maintain a map of each memory source: its purpose, scope, owner, and
  how to retrieve and update it.
- Retrieve relevant prior decisions before repeating an investigation
  or proposing an architectural change.
- Treat conversations and tool histories as evidence of prior work.
  Verify present code, deployment, permissions, and runtime state at
  their authoritative sources.
- When sources conflict, identify the disagreement, compare dates and
  provenance, and verify the affected claim. Do not silently pick one.
- Write findings back to the appropriate context with source links,
  verification status, and uncertainty. Mark superseded conclusions.
- Store secrets only in the designated secret store, never in general
  conversation memory.

## Host-service policy (runit)

This server runs **Artix Linux with glibc**, not Chimera or a musl-based host.
Build and test the server for `x86_64-unknown-linux-gnu`. An installed musl
cross target or the Codex executable's own musl packaging does not change the
repository's native target. Any separate container portability test must name
its target and must not substitute for the Artix/glibc verification gate.

This host runs **runit** as PID 1. Manage services with `sudo sv <command> <service>` (e.g., `sudo sv restart op-grpc-bridge`). `sv` needs root to read `supervise/ok`.

- `/etc/runit/sv/<service>/run` is the service definition.
- Enabled by a symlink in `/etc/runit/runsvdir/default`.
- Update path: edit the `run` script or install a new binary, then `sudo sv restart <service>`.

Do not edit `/run/runit/service` directly. Do not invoke `runsv`/`runsvdir` by hand. **s6 is legacy**: `service6`, `s6-rc`, `s6-*`, `/run/service` are stale — treat any doc assuming them as historical. Note: `/usr/local/bin/systemctl` on this host is a shim that returns plausible answers — using it masks the deprecation; always use `sv`. Do not use `systemctl` or other foreign service-manager CLIs. Container/app deployment uses D-Bus via `busctl`.

## Network

Network topology, Xray policy, decoy/WARP transport and network-critical
service rules are in `docs/src/operations/network.md`. Read it before any
network work; it is not needed otherwise.


**Owner clarification (2026-09-13): there is no deployment script. Deployment
is btrfs send/receive of the prepared filesystem snapshot.** The release
boundary is the snapshot's contents and the send/receive transfer. Existing
`deploy/` scripts are source/history or preparation tooling; neither
`build-golden.sh` nor `btrfs-layout.sh` defines the current deployment mechanism.
Never hand-copy binaries onto a running host as a deploy step.

**Owner decision (2026-10-14): `./activate-golden.sh` at the repo root is the one
command for deploying a new version.** It runs the internal
`deploy/lib/golden/build-golden.sh` helper, then `op-release-launch release`:
btrfs send/receive of a golden snapshot into the inactive of two slots, then a
live read-only overlay swap at `/opt/op-dbus/current` (active + previous only;
`op-release-launch rollback` undoes it). The 2026-09-13 "no deployment script"
rule above is superseded for this entry point; the transport is still send/receive.

`deploy/lib/golden/build-golden.sh` only populates and validates
`/opt/op-dbus/golden` from already built artifacts and an already sealed,
persisted catalog. Set `OP_PERSISTED_CATALOG_SOURCE` to use an isolated release
catalog; the script validates and copies it but never reseals it. It does not
create the read-only release snapshot, send
or receive it, activate a received filesystem, install files into the running
host, or restart services. `--golden-only` remains a compatibility alias for
its default behavior; the former `--live-only` and `--no-restart` modes are
retired. Snapshot creation, `btrfs send/receive`, target activation, and any
deliberate target service action are separate operator steps.

```sh
CXXFLAGS="-include cstdint" cargo build --workspace --release
sudo target/release/opblob seal-shm            # explicitly reseal after the build
sudo target/release/opblob persist             # retain the catalog across reboot
```

After builds, explicitly run the freshly built `opblob` to seal the release catalog; bridge startup only consumes sealed blobs and never reseals. For isolated release preparation, use `seal-plugins <new-catalog-dir>` and pass that catalog as `OP_PERSISTED_CATALOG_SOURCE`; publishing live SHM remains part of activation. Match the service's schema-shaping environment. The renamed cognitive code reads `COGNITIVE_*`: `COGNITIVE_QDRANT_URL` defaults to `http://127.0.0.1:6334`, matching the current `deploy/runit/op-grpc-bridge/run`; the historical `COGNITIVE_MCP_QDRANT_URL=http://10.200.0.2:6334` instruction is obsolete. After an explicit GNU-target build, use `target/x86_64-unknown-linux-gnu/release/opblob`, not an older default-target artifact. Verify the live and persisted catalogs match after activation. Review catalog removals and keep a recoverable copy before a full reseal, which sweeps plugin IDs absent from the build.

The commands above describe build/catalog preparation, not deployment. Do not
infer a live install or restart from a successful build or snapshot preparation.

**Clean-baseline activation (2026-09-22):** local releases are received under
`/opt/op-dbus/releases/<release-id>` and selected by the atomic
`/opt/op-dbus/current` symlink. The operator procedure and rollback contract
are in `docs/src/getting-started/local-release-activation.md`. Executable and
selected configuration paths link into the received read-only tree; writable
service supervision, credentials and runtime data stay outside it. The
`tched-router` runit service owns the pinned threetched executable on loopback
8084; the duplicate `zeroclaw` service name is retired. This defines the
previously missing activation contract, not a new deployment script.

## Workspace shape and build

The workspace has **37 member crates under `crates/`, plus the root `op-dbus`
package** (38 workspace packages total). The root package has no application
binary. Binary targets live in members: for example `op-grpc-bridge` and
`op-grants-materializer` in `op-grpc-bridge`, `op-web-server` and `opdbus` in
`op-web`, `opblob` in `op-plugins`, `3tched-fs` (our 3tchedFS schema filesystem,
package `tched-fs`, Rust 2024 edition) in the retained `crates/threetched-fs` directory, and `op-runit-systemctl`/`svd` in
`op-runit-systemctl`. A binary target or runit directory is source inventory,
not proof that it is deployed; reconcile it with `enabled-services` and
`retired-services`. There is no `op-projection` crate. `[patch.crates-io]`
patches only `rovs-openflow` (vendored fork for OF1.5 `packet_type` match at
`vendor/rovs-openflow`). `op-ml`, `op-gallery-gen`, and `op-uds-relay` have manifests under `crates/`
but are outside workspace membership; `op-ml` remains disabled for the ort API breakage (`#ml-fixme`).

**Rust-first: no new Python.** New automation, probes, and helpers go in Rust (`cargo run --bin <name> -p <crate> -- <args>`, a small binary in the relevant crate, or a `cargo-script`-style one-shot) or in shell + POSIX tools (`jq`, `curl`, `busctl`, `sv`). Do not add new `.py` files to the repo. Pre-existing scripts under `scripts/` (`export-llm-sessions-to-notebooklm-sources.py`, `or-fusion-archive.py`, `notebook-sources-cleanup.py`, etc.) are grandfathered — maintain them in place, don't propagate the pattern.

**Skill runtime tools may be run directly (owner, 2026-10-14).** The tools that back the skills installed by `install_3tched_skills.sh` — `zero`, `bd`/`bv` (beads), `trestle` (Compliance Trestle), and the Hugging Face stack (`hf`, `trl`, `trackio`, `transformers`, `sentence-transformers`, `datasets`) — are installed in the user environment (`~/.local/bin`, uv tools) and agents may invoke them directly, without delegation and despite being Python or third-party tools. This exception covers running those tools; it does not allow adding new `.py` files to the repo or wiring them into op-dbus services.

All cargo commands run from repo root:

**Owner decision (2026-10-17): JEV owns Cargo model selection and worker
routing.** Do not assign a Cargo-specific model, map caller families to models,
or wrap Cargo in another LLM dispatcher. Use JEV's existing routing without
model overrides. The primary agent stays available for source review,
decisions and user communication; execution workers run the assigned command
without recursively delegating it.

One Cargo process at a time: if Cargo, rustc or Clippy is running, do not start
another. Workspace-wide builds, checks or tests require an explicit workspace
request. The native target remains `x86_64-unknown-linux-gnu`; do not install
binaries or restart services as part of a Cargo run.

```bash
cargo build --workspace
cargo check -p <crate>
cargo test --workspace --all-targets --all-features
cargo test -p <crate> <test_name>
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

**Build gotchas**:
- **Release builds need `CXXFLAGS="-include cstdint"`** — vendored RocksDB in `cozorocks` fails on modern GCC without it.

**Frontend — single Vite app, external repo**:

The production UI is **`/srv/git/3tched.com`** (github.com/repr0bated/3tched.com; not in this repo). It is a SvelteKit + Svelte + Vite chat app using `@json-render/svelte` that talks only to the `:8090` door (one generic `PluginService/CallMethod` over gRPC-Web, exactly one OIA1 or SID1 credential) and that op-web serves as static files from `/usr/local/share/op-dbus/dashboard/` — the dashboard root, `http://<host>:8080/`. The current successor golden builder reads the bundled source at `vendor/3tched-json-render-ui`; check that builder and its inputs before treating newer sibling-checkout instructions as implemented.

**Owner UI stack rule (2026-10-11):** implement application interfaces only
with Svelte/SvelteKit, Vite and json-render's Svelte renderer. This applies to
every user and chat mode, including generated interfaces. Every visible UI
element must be declared in the active catalog and implemented in its registry.
Installed skills for other frameworks are reference material only; they do
not authorize React/JSX/Next.js components,
another renderer, or framework dependencies in this application. Apply their
framework-independent guidance only where it fits this stack. The shared
UI-authoring preflight applies across modes; each caller keeps its existing
grants. Documentation availability never grants file or operation access.

```bash
# Build and deploy at op-web's root (from the UI repo)
cd /srv/git/3tched.com && LANDING=prod VITE_OPDBUS_DOOR=https://10.0.0.2:8090 npm run publish:op-web
cd /srv/git/3tched.com && URL_PREFIX=/ VITE_OPDBUS_DOOR=https://10.0.0.2:8090 npm run publish:op-web

# Dev server
cd /srv/git/3tched.com && npm run dev
```

`LANDING` is the mount name under the static root (`live`, `prod`, `3tched`). `URL_PREFIX=/` is the dashboard root, and there the script replaces only `assets/` and `index.html`. A named mount is a separate directory under that root. op-web does **not** embed the UI via RustEmbed — it serves the deployed static files. There is no pairing. The current op-web identity endpoint refuses to infer a browser identity from a TCP source address; remote pickup uses the decoy issuer or an authenticated injecting host page.

On the configured HTTP pages at `10.0.0.2:8080` and `10.0.0.4:8080`, the UI uses op-web's existing same-origin Axum gRPC-Web connector, which forwards the unchanged identity metadata to the TLS door at `10.0.0.2:8090`. Other origins keep the direct HTTPS door. The generic `PluginService/CallMethod`, sealed capabilities, and interceptor identity checks remain the admission path; the connector does not mint or select an identity.

**Deprecated** (do not use for new work):
- `crates/op-web/ui/` and `crates/zeroclaw-gui/` — deleted 2026-09-18 together with the device-pairing path they existed to call (`/pair`, `/admin/paircode/new`, removed 2026-09-04). Do not resurrect either.
- `/srv/git/operation-dashboard-ui-07` — old operator console, superseded by `/srv/git/3tched.com`.
- `crates/` dev dashboard (`crates/src/`) — legacy dev tooling.

**CI does NOT gate `cargo test` or `cargo clippy`** — never assume "CI green" means tests pass. The only CI code review is the Codex full-repo-review action (best-effort LLM pass, not a deterministic check).

## Conventional commits

`feat` / `fix` / `docs` / etc. with scoped `(<crate>)` and `!` for breaking changes (e.g., `feat(op-plugins)!:`, `fix(mcp):`). Convention only — no enforced hook.

## Verification and testing — manual/on-request

**Owner correction, 2026-10-18:** implement → build → deploy. Tests, Clippy,
formatting checks and `/verify` run only when the owner explicitly requests
them; do not start them automatically before or after deployment. The owner
may assign those checks to a downtime agent. This supersedes the earlier
automatic/deferred verification workflow.

This includes disposable-VM tests and custom smoke fixtures. Do not make their
execution or repair a pre-installation/deployment gate. Further testing waits
until after deployment and an explicit owner request.

Perform the build required for the change and deploy only within the authorized
scope. Keep bounded operation receipts, safety checks and readiness evidence
separate from a test-suite pass; report built, installed, deployed and tested
status accurately. Do not repeat unchanged successful checks or turn a missing
test run into a pass.

When explicitly requested, `/verify` remains `cargo fmt --all -- --check` +
`cargo clippy --workspace --all-targets --all-features -- -D warnings` +
`cargo test --workspace --all-targets --all-features`. Workspace commands still
require an explicit workspace request. CI does not establish these results.

## Architecture — the load-bearing invariants (read these before touching plugins, state, or identity)

- **Identity storage lifecycle (owner correction 2026-10-05).** Cozo owns identity-to-leaf/loop assignment and isolation. One immutable physical seed is shared through read-only views; each identity keeps its own writable leaf and the existing seven subvolumes. Only the external host provisioner may add a new blank leaf once, during explicit creation. Login, start, stop, logout, restart, bridge recovery and failed-start cleanup must NEVER add/remove Btrfs members. Start verifies/remounts the existing pair through the host mount owner; stop leaves membership and data intact. The bridge only verifies externally mounted storage and records logical attachment. Do not interpret `attached=false` as device removal. No root copies, snapshots, new subvolumes or OverlayFS fallback during identity provisioning. Never modify a seed while retained leaves depend on it. Older add-on-start/remove-on-stop guidance in source, handoffs and signals is superseded; do not restore it. Host release snapshot/send/receive below is a separate deployment mechanism.
- **D-Bus is the only control plane.** Every plugin is a D-Bus object at `/org/opdbus/v1/plugins/<name>` under `org.opdbus.v1`. No `Command::new(...)` subprocesses, no direct file reads for live state, no polling loops in plugin/service code. New backend capabilities register as plugins in `crates/op-plugins/src/default_registry.rs` — never as new gRPC proto service packages.
- **The plugin IS the schema — one source of truth, nonnegotiable.** That single schema is sealed into the blob, which IS the plugin. D-Bus method signatures, 3tchedFS operation files, gRPC shapes, and UI field renderers derive from it. Validate inputs against it. Each plugin defines its schema under `crates/op-plugins/src/state_plugins/`, using `plugin_scaffold_helpers.rs`. **There must be no parallel plugin schema source in a repository-level or plugin `schemas/` directory.** The former plugin capture directories, separate section files, and JSON-file loader have been removed. `crates/op-grpc-bridge/src/schema_loader.rs` still exists by name, but it reads only a manifest-pinned `PluginSchema` from the sealed blob catalog; it is not a file-schema fallback. **Owner correction (2026-10-18): capture threetched's full `config schema` from the verified binary on every seal.** The shared capture fixes the runtime config directory, checks the binary and manifest, and supplies native_config, MCP, model_routes, plugins and skill_bundles. `tched_router_contract.rs` contains only access/section helpers; the embedded UPSTREAM IMPORT is retired. Build-time generated bridge contracts explicitly prepare the same capture; runtime consumes the sealed catalog and never starts capture as a fallback. Do not restore an embedded schema or add independent captures using the sealer's ambient environment. The per-section generator and generated surface were retired on 2026-10-04. Older docs claiming a single `plugin_schema_defs.rs` are also stale — that file does not exist.
- **The sealed blob IS the plugin.** Present state lives in the sealed blob catalog at `/dev/shm/opdbus/plugin-blobs/<plugin_id>.<schema_hash16>.blob`. A plugin exists ⟺ its blob is in the catalog. The sole writer is the blob sealer in `op-blob`. Consumers read SHM directly (zero-copy) — they never re-hash and never consult the Rust registry for existence. The old `/dev/shm/opdbus/schemas` folder and `live-schema.json` are gone.
- **MutationEngine owns state; streaming comes from SHM (owner reaffirmed 2026-09-13).** MutationEngine folds authoritative backend results into the complete state tree, including child objects, and commits that state to canonical SHM before change notifications. Child updates/deletions must preserve unrelated children and parent fields. Existing subscriptions derive their values from committed SHM; an in-memory event payload, method argument, or generator buffer is not a second state authority. Preserve verified principal/session scope for private state. Do not add a separate per-method streaming control plane or claim a buffered unary response provides state streaming.
- **MCP retired — initial setup error (owner correction 2026-10-28).** There is no MCP interface. MCP was part of the initial setup in error and has been replaced by 3tchedFS: an identity reads and calls sealed methods only through its own mounted namespace (`operations/<plugin>/<method>/call`), admitted and receipted by the bridge. Do not build on, document as current, or re-enable the bridge's `/mcp` endpoint, the leftover `3tched-fs mcp` subcommand, router MCP bundles, or in-process cognitive/compact MCP. The former text ("MCP gateway (settled): one public MCP door on `:8090`…, compact mode 4 meta-tools in op-web…") is superseded. Never create new shims or point external clients at `op-assistant-grpc`.
- **External binaries back plugins; the plugin remains the single schema source.** 3tched-router / tched-router (`tched_router` in code) is the framework/schema/plugin integration backed by the autonomous `/usr/bin/threetched` executable. The reproducible threetched input is pinned in `vendor/tched-router/source.pin`; `vendor/tched-router/build-threetched.sh` applies the checked-in patches and emits `target/threetched-artifacts/tched-router/{threetched,manifest.json}` without installing it. `build-golden.sh` validates the pin, patch and binary provenance, then stages the binary, `/usr/share/op-dbus/threetched-manifests/tched-router.json`, `/etc/op-dbus/threetched-runtime.toml`, and the tracked default skill bundle into the golden filesystem. The `tched-router` runit service verifies that manifest, exact commit, binary digest, and version before exec, then seeds its private runtime config and skills from those staged bootstrap files. A fresh checkout includes the digest-pinned upstream source archive, patches and build recipe; the helper verifies and uses that archive by default. Cargo registry dependencies and system build tools still need provisioning. Owner correction (2026-10-11): the executable and runtime-facing branding are lowercase `threetched`; the FUSE discovery source is `threetched-surface.rs`. Preserve upstream repository URLs, licenses and historical provenance. The former instruction to keep the executable named zeroclaw is superseded. The plugin contract remains `tched_router_contract.rs`, and source attribution may identify `zeroclaw config schema`.
- **Owner-confirmed lifecycle and model roles (2026-09-13, reaffirmed 2026-09-14).** Waypipe is retired: its crate directory, Cargo membership, binary target and protocol are absent; `retired-binaries`/`retired-services` prevent stale build or service artifacts from entering golden. Do not restore it in source exports or release inputs. Assistant is the control-plane chatbot, not an `assistant` container. The historical Gemma routing role is model agnostic; the `op-gemma` crate is retired (2026-09-19: directory, Cargo membership and binary removed, listed in `retired-binaries`) — the `gemma_brain` plugin remains and its role does not constrain the model/provider. Cargo membership and legacy names do not establish active deployments.
- **Factory role correction (2026-09-14).** The former Factory account/platform plugin is retired from the registry and dispatch. The intended access is CLI/SSH; routing belongs to `tched_router`, and Factory billing requires an actual native binding. Provider names or relabelled direct upstream routes do not prove that binding. A future Missions integration needs its own verified native contract; do not restore the retired REST platform methods from historical fixtures or docs.
- **Standalone vendor adapters are retired (2026-09-14).** The `op-grpc-adapters` crate directory and its `op.adapters.v1` protocol are absent after Netmaker retirement. Its duplicate mail/MQ services bypassed canonical plugin admission. Do not restore this crate or its old Netmaker clients. The existing bridge `operation.mail.v1` service and retained EMQX plugin are separate surfaces; their presence does not prove every mail or MQTT method is implemented.
- **Identity minting and metadata (owner reaffirmed 2026-09-13).** The identity subsystem owns minting the identity and its SealedId/SID1 credential blob. This is distinct from an OPBLOB01 plugin blob and does not belong in the plugin catalog. Clients forward the authored credential through the transport's identity metadata; they do not mint or reconstruct it from genesis/trace/key headers. Verify minting, persistence, injection and receiver validation as separate links. Current source assigns session/SID1 creation to MutationEngine with the op-identity format; the existence of an HTTP/MCP helper does not prove every gRPC client injects SID1.
- **Identity injection (owner decision 2026-09-25).** The decoy mints the sealed identity blob; the 8090 interceptor anchors the session and writes the identity metadata (`anchor_verified_human_session` and `insert_identity_metadata` in `crates/op-grpc-bridge/src/interceptor.rs`). The oracle (OIA1) is the decoy's signed peer proof; the human credential (SID1) is the session identity for that key. The bridge's own client and MutationEngine are the wrong place for session or identity injection. Source and live state did not match this on 2026-09-25: the interceptor still mints genesis server-side when no session exists (`mint_and_store_genesis` in `anchor_verified_human_session`), and the decoy issuer mints only OIA1. Transport paths (decoy, WARP, door addresses) are in `docs/src/operations/network.md`.
- **Btrfs storage and transport (owner correction 2026-09-23).** Deployment remains the prepared snapshot transferred by `btrfs send/receive`. The earlier global five-subvolume limit and instruction to replace Snowball/cache subvolumes with ordinary directories are withdrawn. Preserve the original `operation-dbus-proto` cache/NUMA/timing architecture: Btrfs cache category subvolumes; authoritative timing plus vector/state subvolumes; aligned read-only snapshot sets and send/receive. See the original repo `branding/product-baseline.md` §12 and `crates/op-snowball/{DESIGN,REQUIREMENTS}.md` (R5). Retention is configurable per storage owner, not a global five-subvolume cap. Parent snapshots do not include nested subvolume contents: snapshot and transport every required member explicitly. Never flatten existing subvolumes or delete historical data to satisfy the withdrawn limit. Preserve security, writer-locking, identity and error-handling fixes while correcting storage regressions.
- **No SQLite — use CozoDB.** CozoDB (`op-cozo-store`) is the embedded database for this project. Do not add new `rusqlite` or other SQLite use. `op-cache/src/btrfs_cache.rs` already uses a Cozo index; current `workstack_cache.rs`, `workflow_cache.rs`, `workflow_tracker.rs`, and `pattern_tracker.rs` still use `rusqlite` and remain migration debt rather than an approved pattern.
- **RAG pipeline and Voyage-4 models.** Live source file indexing goes through `code_index` (mode='source') into Qdrant collections: `repos_lsp_*_voyage_4_large` and `repos_lsp_*_voyage_4_lite`. Voyage-4 models are interchangeable across their dimension (1024 for all variants); `_large` and `_lite` refer to model size/quality tier, not embedding space. All collections share a single fused retrieval space and can be queried together. Legacy `repos_lsp_*_voyage_code_4` collections and `repomix_rag` collection are retired — do not use them. The standalone `notebooklm-mcp` service is retired; the plugin-owned `notebooklm` contract and bridge provider remain. Owner reaffirmed NotebookLM conversation/tool-history sync on 2026-10-05; use its declared methods through identity-bound 3tchedFS. Do not confuse this corpus sync with the Voyage source-indexing pipeline.

## Handoff between agents

Claude, Codex, Grok and any other agent share one handoff: `HANDOFF.md`. Read it
at the start of every session, after this file. Claim your task in its *Active
work* table before changing anything, and never reset, stash or overwrite work
another agent has claimed there. Before the session ends, or before a context or
usage limit, update your row and add a dated *Handoff* entry. A handoff kept only
in one agent's private memory is invisible to the others, so it does not count.

## Signals and task board

- **Append observations to `SIGNALS.md`** rather than letting them evaporate in chat. Three signal types: `💡 SUGGESTION` · `⚠️ CONCERN` · `👁️ OBSERVATION`. Each row: date · model · signal · optional `→ OD-##` link. Models leave status `open`. Promote anything actionable to `WISHLIST.md` as an `OD-##` and link it back.
- **`WISHLIST.md` is the canonical task board** — do not create new TODO docs. Priority buckets: Critical/Urgent → Current → Next → Future → When I Have Staff. Dispatch syntax: "dispatch <ID> to <agent>" spawns an agent with the task context and updates status here.
- **Read recent `SIGNALS.md` rows before non-trivial work** in the area they touch — the file records load-bearing gotchas (stale vendored deps, live/deployed drift, the live-vs-intent identity topology) that source alone will not surface.

## Stale documentation

- **README.md is stale**: crate count (31) is wrong, `cd lovable` is a deleted directory, `sudo service6` references are retired. Prefer AGENTS.md and source over README claims.
- Many older deploy scripts and HANDOFF docs still reference s6 — treat them as archival; runit/`sv` is current.
- Anything mentioning `/dev/shm/plugin_schema.dat`, `live-schema.json`, or `/dev/shm/opdbus/schemas` is describing a retired design.
- Anything mentioning `OIB1`, `oib1:`, `blob_ref`, `SessionIdentityBlob`, or `x-opdbus-identity-blob-bin` is pre-2026-09-01 and stale; the session-identity envelope is now `SID1` / `SealedId`.
- Anything referencing `crates/op-web/ui/`, `zeroclaw-gui`, pairing codes, or `operation-dashboard-ui-07` — the production UI is `/srv/git/3tched.com` (github.com/repr0bated/3tched.com), deployed from that repo through `scripts/publish-op-web.sh`. `LANDING` selects the mount name; `URL_PREFIX=/` selects the dashboard root.
- Older deployment-script and backup/restore hierarchy descriptions are not current deployment authority. Deployment is btrfs send/receive of the prepared snapshot — see architecture invariants above.

## Subagent and model selection

Cargo execution follows JEV routing above; the guidance below does not allocate
Cargo models.

**Owner preference (2026-09-13, model names withdrawn 2026-09-24,
reaffirmed 2026-09-30): assess task
difficulty and required reasoning before delegation, and use the cheapest
available model that can complete it at the same quality.** A lower price must
not thin the answer, skip evidence, or weaken a judgment. If the cheaper model
would lose accuracy, completeness, or care, use the more capable model. Pick
from the models the session can actually launch. Do not inherit an expensive
parent model by default, and do not pin a product or model id in this file.
Use a low-cost model for lookups, inventory extraction, documentation
assembly and routine non-Cargo checks when that model can match the
quality of a more capable one. Choose low effort for mechanical work and
medium effort for bounded reasoning. Escalate when the task's quality needs
it or a cheaper attempt falls short, and briefly state why.

Keep the primary agent available for user communication and coordination.
Delegate longer execution and research jobs, use nonblocking progress checks,
and avoid long blocking tool calls on the primary. User messages steer active
work; answer them promptly while delegated work continues. Small immediate
edits and decisions can remain with the primary when delegation would add
overhead.

**Owner requirement (2026-10-20), general across tasks:** long-running execution
must survive interruption or exit of the chat. Use an execution subagent or
background worker, with the actual host command detached from the chat/tool
process group, closed stdin, persistent logs and a durable completion receipt.
A background subagent alone does not establish process survival. Keep this chat
available; after reconnecting, reconcile the existing process and receipt rather
than rerunning creation or mutations. Preserve JEV Cargo routing, single-Cargo
exclusion and authorization boundaries; detachment grants no additional access.

For non-Cargo delegation, choose the appropriate agent role:

- `Explore` (fast read-only) for locating code, grepping symbols, answering "where is X."
- `Plan` for architecture and design before writing code.
- `general-purpose` only when the task needs multi-step reasoning or execution that Explore or Plan cannot cover.
- Use a more capable model for complex cross-crate reasoning, security review, or ambiguous architectural decisions, and for any standard edit, build fix, or test whose quality would drop on the low-cost agent.
- Do not spawn a subagent for a single-file edit or a direct grep you can do yourself.

## Cursor Cloud specific instructions

- Provision from the repository root with `sh scripts/claude-cloud-setup.sh`. It installs the verified-workspace packages and Rust 1.97.1, fetches the lockfile, and runs `scripts/check-workspace.sh preflight`. It is safe to run again. It does not start services, seal a catalog, or copy host credentials.
- Native Cozo/RocksDB builds need `CXXFLAGS` to include `-include cstdint`, and they need `CC=gcc` and `CXX=g++`. On the default image `/usr/bin/c++` is clang++, which does not find `cstdint` for that forced include. The setup script persists these variables for login shells and the `ubuntu` bashrc, and it links `/usr/lib/x86_64-linux-gnu/libstdc++.so` so rustc's bundled lld can resolve `-lstdc++`. `scripts/check-workspace.sh` also sets `CXXFLAGS` itself. Keep `CARGO_BUILD_JOBS=2` on a 16 GB agent VM.
- Smoke checks and tests are manual/on-request under the owner's policy. Do not run them automatically or treat the general `/verify` guidance above as authorization. When requested, `sh scripts/check-workspace.sh smoke` runs `cargo check --locked --workspace`, then `op-core` and `op-plugins` library tests. `verify` is the full fmt, Clippy, and workspace test gate and also requires an explicit request.
- The Cloud Agent kernel has no `btrfs` filesystem and no `/lib/modules`. `btrfs-progs` is installed, but `btrfs subvolume create` cannot succeed. These `op-plugins` library tests need a btrfs temp directory and fail here: `failed_unknown_rollback_preserves_state_and_retained_snapshots_after_reopen`, `rejects_missing_invalid_and_unsafe_requests`, and `seven_methods_use_persistent_snapshot_state`. Do not skip or weaken them.
- Do not start the control plane here. `op-web`, the gRPC bridge, OVS, catalog seal, and runit need the Artix host, the sealed blob catalog, and identity state. `op-plugin-lint` can read plugin source without those services.
