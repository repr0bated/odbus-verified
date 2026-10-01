# AGENTS.md

This file provides guidance to the AI agent when working with code in this repository.

## Mandatory skill preload — every agent and model

At the start of every session, before any analysis, planning, edits, or subagent work, read these four files completely:

- `.agents/skills/grpc-expert/SKILL.md`
- `.agents/skills/grpc-protocol-expert/SKILL.md`
- `.agents/skills/json-render/SKILL.md`
- `.agents/skills/ovs-db-analysis/SKILL.md`

Mandatory even when the task does not mention these domains. Every subagent and model handoff must repeat the preload for its own session — a parent's preload does not count for a child. For OP-DBUS PluginSchema, generated plugin services, reflection, and seal/freeze/hot work, `grpc-expert` takes precedence over the generic `grpc-protocol-expert`; use the generic skill for protocol, channel, TLS, streaming, and observability concerns.

The container is the identity. 3tchedFS is its lungs: the local, typed
filesystem interface projected from the sealed blobs and live SHM state.

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


```
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
```
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

## Xray configuration policy — mandatory

**Xray's live config must exist only at `/etc/xray/xray_config.json` inside the container.** Never point Xray at `/dev/shm/xray_config.json`, `/usr/local/etc/xray/config.json`, or any other disk-backed live path. The static bootstrap config is materialized at boot; the future model-generated generator replaces the same file atomically and reloads through D-Bus. Models must not write or reload Xray directly.

## Deployment (btrfs send/receive)

**Owner clarification (2026-09-13): there is no deployment script. Deployment
is btrfs send/receive of the prepared filesystem snapshot.** The release
boundary is the snapshot's contents and the send/receive transfer. Existing
`deploy/` scripts are source/history or preparation tooling; neither
`build-golden.sh` nor `btrfs-layout.sh` defines the current deployment mechanism.
Never hand-copy binaries onto a running host as a deploy step.

`deploy/runit/build-golden.sh` only populates and validates
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

After builds, explicitly run the freshly built `opblob seal-shm`; bridge startup only consumes sealed blobs and never reseals. Use the service's schema-shaping environment when sealing: on this host, `sudo env COGNITIVE_MCP_QDRANT_URL=http://10.200.0.2:6334 target/release/opblob seal-shm` matches `deploy/runit/op-grpc-bridge/run`. Verify the live and persisted catalogs match after restart. Review catalog removals and keep a recoverable copy before a full reseal, which sweeps plugin IDs absent from the build.

The commands above describe build/catalog preparation, not deployment. Do not
infer a live install or restart from a successful build or snapshot preparation.
Network-critical services (OVS, uplink, DHCP, session bus) require deliberate
operator action; a learning or inventory pass never restarts them.

**Clean-baseline activation (2026-09-22):** local releases are received under
`/opt/op-dbus/releases/<release-id>` and selected by the atomic
`/opt/op-dbus/current` symlink. The operator procedure and rollback contract
are in `docs/src/getting-started/local-release-activation.md`. Executable and
selected configuration paths link into the received read-only tree; writable
service supervision, credentials and runtime data stay outside it. The
`tched-router` runit service owns the pinned native executable on loopback
8084; the duplicate `zeroclaw` service name is retired. This defines the
previously missing activation contract, not a new deployment script.

## Workspace shape and build

The workspace has **37 member crates under `crates/`, plus the root `op-dbus`
package** (38 workspace packages total). The root package has no application
binary. Binary targets live in members: for example `op-grpc-bridge` and
`op-grants-materializer` in `op-grpc-bridge`, `op-web-server` and `opdbus` in
`op-web`, `opblob` in `op-plugins`, `threetched-fs` (the 3tchedFS schema filesystem,
imported 2026-09-27, Rust 2024 edition) in `threetched-fs`, and `op-runit-systemctl`/`svd` in
`op-runit-systemctl`. A binary target or runit directory is source inventory,
not proof that it is deployed; reconcile it with `enabled-services` and
`retired-services`. There is no `op-projection` crate. `[patch.crates-io]`
patches only `rovs-openflow` (vendored fork for OF1.5 `packet_type` match at
`vendor/rovs-openflow`). `op-ml`, `op-gallery-gen`, and `op-uds-relay` have manifests under `crates/`
but are outside workspace membership; `op-ml` remains disabled for the ort API breakage (`#ml-fixme`).

**Rust-first: no new Python.** New automation, probes, and helpers go in Rust (`cargo run --bin <name> -p <crate> -- <args>`, a small binary in the relevant crate, or a `cargo-script`-style one-shot) or in shell + POSIX tools (`jq`, `curl`, `busctl`, `sv`). Do not add new `.py` files to the repo. Pre-existing scripts under `scripts/` (`export-llm-sessions-to-notebooklm-sources.py`, `or-fusion-archive.py`, `notebook-sources-cleanup.py`, etc.) are grandfathered — maintain them in place, don't propagate the pattern.

All cargo commands run from repo root:

Delegate Cargo execution to a subagent using the least-expensive available
model that preserves accuracy, completeness and care, subject to the owner's
current session preference. This includes metadata, formatting, checks,
builds, tests, Clippy and scripts that invoke Cargo. Keep these repository
instructions model-agnostic; record a temporary model choice in the session
handoff. The primary agent stays available for
source review, decisions and user communication.

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

The production UI is **`/srv/git/3tched.com`** (github.com/repr0bated/3tched.com; not in this repo). It is a Vite + React Json Render chat app that talks only to the `:8090` door (one generic `PluginService/CallMethod` over gRPC-Web, exactly one OIA1 or SID1 credential) and that op-web serves as static files from `/usr/local/share/op-dbus/dashboard/` — the dashboard root, `http://<host>:8080/`. The pinned release archive of that checkout is `vendor/3tched-json-render-ui`.

```bash
# Build and deploy at op-web's root (from the UI repo)
cd /srv/git/3tched.com && LANDING=prod VITE_OPDBUS_DOOR=https://10.0.0.2:8090 npm run publish:op-web
cd /srv/git/3tched.com && URL_PREFIX=/ VITE_OPDBUS_DOOR=https://10.0.0.2:8090 npm run publish:op-web

# Dev server
cd /srv/git/3tched.com && npm run dev
```

`LANDING` is the mount name under the static root (`live`, `prod`, `3tched`). `URL_PREFIX=/` is the dashboard root, and there the script replaces only `assets/` and `index.html`. A named mount is a separate directory under that root. op-web does **not** embed the UI via RustEmbed — it serves the deployed static files. The browser's credential comes from `GET /api/identity/self` (op-web, `OP_WEB_HUMAN_PEERS`), from an injecting host page, or from the connect gate; there is no pairing.

Fresh source exports also include the pinned UI source archive and verifier at
`vendor/3tched-json-render-ui/build-assets.sh`. That helper builds and checks the
captured UI without the sibling checkout or live rsync; golden preparation
stages only its validated asset archive and provenance manifest.

**Deprecated** (do not use for new work):
- `crates/op-web/ui/` and `crates/zeroclaw-gui/` — deleted 2026-09-18 together with the device-pairing path they existed to call (`/pair`, `/admin/paircode/new`, removed 2026-09-04). Do not resurrect either.
- `/srv/git/operation-dashboard-ui-07` — old operator console, superseded by `/srv/git/3tched.com`.
- `crates/` dev dashboard (`crates/src/`) — legacy dev tooling.

**CI does NOT gate `cargo test` or `cargo clippy`** — never assume "CI green" means tests pass. The only CI code review is the Codex full-repo-review action (best-effort LLM pass, not a deterministic check).

## Conventional commits

`feat` / `fix` / `docs` / etc. with scoped `(<crate>)` and `!` for breaking changes (e.g., `feat(op-plugins)!:`, `fix(mcp):`). Convention only — no enforced hook.

## Verify before marking done

Run `/verify` before reporting work complete. CI won't catch fmt drift, clippy warnings, or test regressions, so the agent must. `/verify` runs `cargo fmt --all -- --check` + `cargo clippy --workspace --all-targets --all-features -- -D warnings` + `cargo test --workspace --all-targets --all-features`.

## Architecture — the load-bearing invariants (read these before touching plugins, state, or identity)

- **D-Bus is the only control plane.** Every plugin is a D-Bus object at `/org/opdbus/v1/plugins/<name>` under `org.opdbus.v1`. No `Command::new(...)` subprocesses, no direct file reads for live state, no polling loops in plugin/service code. New backend capabilities register as plugins in `crates/op-plugins/src/default_registry.rs` — never as new gRPC proto service packages.
- **The plugin IS the schema — one source of truth, nonnegotiable.** That single schema is sealed into the blob, which IS the plugin. D-Bus method signatures, MCP tool inputs, gRPC shapes, and UI field renderers derive from it. Validate inputs against it. Each plugin defines its schema under `crates/op-plugins/src/state_plugins/`, using `plugin_scaffold_helpers.rs`. **There must be no parallel plugin schema source in a repository-level or plugin `schemas/` directory.** The former plugin capture directories, separate section files, and JSON-file loader have been removed. `crates/op-grpc-bridge/src/schema_loader.rs` still exists by name, but it reads only a manifest-pinned `PluginSchema` from the sealed blob catalog; it is not a file-schema fallback. The tched_router upstream document is imported once into `tched_router_contract.rs`; `tched_router_config_surface.rs` and its methods derive from that plugin-owned contract. Importing a new binary schema must replace that embedded contract through `gen_tched_router_config_surface --import-json`; it must not create a second captured-schema authority. Older docs claiming a single `plugin_schema_defs.rs` are also stale — that file does not exist. Owner reaffirmed this rule on 2026-09-13.
- **The sealed blob IS the plugin.** Present state lives in the sealed blob catalog at `/dev/shm/opdbus/plugin-blobs/<plugin_id>.<schema_hash16>.blob`. A plugin exists ⟺ its blob is in the catalog. The sole writer is the blob sealer in `op-blob`. Consumers read SHM directly (zero-copy) — they never re-hash and never consult the Rust registry for existence. The old `/dev/shm/opdbus/schemas` folder and `live-schema.json` are gone.
- **MutationEngine owns state; streaming comes from SHM (owner reaffirmed 2026-09-13).** MutationEngine folds authoritative backend results into the complete state tree, including child objects, and commits that state to canonical SHM before change notifications. Child updates/deletions must preserve unrelated children and parent fields. Existing subscriptions derive their values from committed SHM; an in-memory event payload, method argument, or generator buffer is not a second state authority. Preserve verified principal/session scope for private state. Do not add a separate per-method streaming control plane or claim a buffered unary response provides state streaming.
- **MCP gateway (settled).** One public MCP door: `:8090` on `op-grpc-bridge` (cognitive MCP is in-process there; no standalone `op-cognitive-mcp` runit service). `compact_mcp` is a retired plugin id. Compact *mode* (4 meta-tools) is in-process in op-web for the singleton control-plane chatbot; it is not an MCP plugin and not a second listener. Never create new shims or point external clients at `op-assistant-grpc`.
- **External binaries back plugins; the plugin remains the single schema source.** 3tched-router / tched-router (`tched_router` in code) is the framework/schema/plugin integration backed by the autonomous `/usr/bin/zeroclaw` executable. The reproducible native input is pinned in `vendor/tched-router/source.pin`; `vendor/tched-router/build-native.sh` applies the checked-in patches and emits `target/native-artifacts/tched-router/{zeroclaw,manifest.json}` without installing it. `build-golden.sh` validates the pin, patch and binary provenance, then stages the binary, manifest, `/etc/op-dbus/zeroclaw-runtime.toml`, and the tracked default skill bundle into the golden filesystem. The `tched-router` runit service verifies that manifest, exact commit, binary digest, and version before exec, then seeds its private runtime config and skills from those staged bootstrap files. A fresh checkout includes the digest-pinned upstream source archive, patches and build recipe; the helper verifies and uses that archive by default. Cargo registry dependencies and system build tools still need provisioning. The executable keeps its upstream name. The plugin contract remains `tched_router_contract.rs`, and source attribution may identify `zeroclaw config schema`.
- **Owner-confirmed lifecycle and model roles (2026-09-13, reaffirmed 2026-09-14).** Waypipe is retired: its crate directory, Cargo membership, binary target and protocol are absent; `retired-binaries`/`retired-services` prevent stale build or service artifacts from entering golden. Do not restore it in source exports or release inputs. Assistant is the control-plane chatbot, not an `assistant` container. The historical Gemma routing role is model agnostic; the `op-gemma` crate is retired (2026-09-19: directory, Cargo membership and binary removed, listed in `retired-binaries`) — the `gemma_brain` plugin remains and its role does not constrain the model/provider. Cargo membership and legacy names do not establish active deployments.
- **Factory role correction (2026-09-14).** The former Factory account/platform plugin is retired from the registry and dispatch. The intended access is CLI/SSH; routing belongs to `tched_router`, and Factory billing requires an actual native binding. Provider names or relabelled direct upstream routes do not prove that binding. A future Missions integration needs its own verified native contract; do not restore the retired REST platform methods from historical fixtures or docs.
- **Standalone vendor adapters are retired (2026-09-14).** The `op-grpc-adapters` crate directory and its `op.adapters.v1` protocol are absent after Netmaker retirement. Its duplicate mail/MQ services bypassed canonical plugin admission. Do not restore this crate or its old Netmaker clients. The existing bridge `operation.mail.v1` service and retained EMQX plugin are separate surfaces; their presence does not prove every mail or MQTT method is implemented.
- **Owner-confirmed network topology (2026-09-13).** There is no WireGuard on this server. WireGuard terminates at the decoy; identity is transported by WARP. WireGuard types, plugin declarations, credentials or historical configuration paths in source do not establish a server WireGuard interface or a requirement to install one. Classify those surfaces by their decoy/server role before proposing implementation or retirement. An unwired `wgcf` method is not evidence that the actual WARP identity transport is broken.
- **Identity minting and metadata (owner reaffirmed 2026-09-13).** The identity subsystem owns minting the identity and its SealedId/SID1 credential blob. This is distinct from an OPBLOB01 plugin blob and does not belong in the plugin catalog. Clients forward the authored credential through the transport's identity metadata; they do not mint or reconstruct it from genesis/trace/key headers. Verify minting, persistence, injection and receiver validation as separate links. Current source assigns session/SID1 creation to MutationEngine with the op-identity format; the existence of an HTTP/MCP helper does not prove every gRPC client injects SID1.
- **Owner decisions: decoy GRE, TLS and identity injection (2026-09-25).** The owner directed these changes; do not restore the previous layout. The door stays `10.0.0.2:8090`: tonic with the internal `svc0` certificate from the op-dbus local CA. The bridge no longer listens on `127.0.0.1:8090` or `10.0.0.3:8090`, and it no longer serves a second browser certificate. Browser-facing TLS terminates on the decoy at `10.0.0.1`. The decoy mints the sealed identity blob, and WARP delivers it to the 8090 interceptor, which anchors the session and writes the identity metadata (`anchor_verified_human_session` and `insert_identity_metadata` in `crates/op-grpc-bridge/src/interceptor.rs`). The oracle (OIA1) is the decoy's signed proof of the WireGuard peer, because WireGuard exists only on the decoy; the human credential (SID1) is the session identity for that key. The bridge's own client and MutationEngine are the wrong place for session or identity injection. Source and live state do not match this yet (2026-09-25): the interceptor still mints genesis server-side when no session exists (`mint_and_store_genesis` in `anchor_verified_human_session`), the decoy issuer mints only OIA1, and the decoy routes browser traffic for `10.0.0.2` over `gre-host` with no WARP route in either main routing table. The `gre-decoy` tunnel joins decoy and host (`deploy/runit/gre-decoy`): outer `188.68.58.237` ↔ `129.80.64.127`, inner `192.168.255.1/30` plus `10.0.0.2/32`, with `10.0.0.1` and the WireGuard clients `10.0.0.5`–`10.0.0.7` routed via `192.168.255.2`. GRE routing replaces NAT on this path: nftables is the firewall only, never a router, so do not add DNAT, SNAT or masquerade for it. Filter rules such as accepting GRE from the decoy are fine. GRE is not encrypted, so anything crossing it that is not TLS is readable on the path. MQTT stays on `10.0.0.3`, aliased on `ovsbr0` beside the OpenFlow address `10.200.0.1`. Xray runs in its container. op-web's page address is `http://10.0.0.4:8080`: plain HTTP outside the TLS zone, with `10.0.0.4/32` on `gre-decoy` and routed from the decoy over `gre-host`. The decoy issuer (`/etc/systemd/system/op-decoy-issuer.service.d/browser-auth.conf` on the decoy, not in this repo) hands out the IP door `https://10.0.0.2:8090` and allows the `10.0.0.4` and `10.0.0.2` page origins; no domain name points browsers at the decoy. Still open on 2026-09-25: how the decoy-minted oracle reaches the interceptor. The owner's direction is that it travels over GRE; whether it rides inside the relayed request or separately is undecided. Until that lands, the deployed page still fetches OIA1 from `http://10.0.0.1:51888/identity/self`, and the door rejects a request that carries no credential.
- **Btrfs storage and transport (owner correction 2026-09-23).** Deployment remains the prepared snapshot transferred by `btrfs send/receive`. The earlier global five-subvolume limit and instruction to replace Snowball/cache subvolumes with ordinary directories are withdrawn. Preserve the original `operation-dbus-proto` cache/NUMA/timing architecture: Btrfs cache category subvolumes; authoritative timing plus vector/state subvolumes; aligned read-only snapshot sets and send/receive. See the original repo `branding/product-baseline.md` §12 and `crates/op-snowball/{DESIGN,REQUIREMENTS}.md` (R5). Retention is configurable per storage owner, not a global five-subvolume cap. Parent snapshots do not include nested subvolume contents: snapshot and transport every required member explicitly. Never flatten existing subvolumes or delete historical data to satisfy the withdrawn limit. Preserve security, writer-locking, identity and error-handling fixes while correcting storage regressions.
- **No SQLite — use CozoDB.** CozoDB (`op-cozo-store`) is the embedded database for this project. Do not add new `rusqlite` or other SQLite use. `op-cache/src/btrfs_cache.rs` already uses a Cozo index; current `workstack_cache.rs`, `workflow_cache.rs`, `workflow_tracker.rs`, and `pattern_tracker.rs` still use `rusqlite` and remain migration debt rather than an approved pattern.
- **RAG pipeline and Voyage-4 models.** Live source file indexing goes through `code_index` (mode='source') into Qdrant collections: `repos_lsp_*_voyage_4_large` and `repos_lsp_*_voyage_4_lite`. Voyage-4 models are interchangeable across their dimension (1024 for all variants); `_large` and `_lite` refer to model size/quality tier, not embedding space. All collections share a single fused retrieval space and can be queried together. Legacy `repos_lsp_*_voyage_code_4` collections and `repomix_rag` collection are retired — do not use them. NotebookLM integration is removed.

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

**Owner preference (2026-09-13, model names withdrawn 2026-09-24,
reaffirmed 2026-09-30): assess task
difficulty and required reasoning before delegation, and use the cheapest
available model that can complete it at the same quality.** A lower price must
not thin the answer, skip evidence, or weaken a judgment. If the cheaper model
would lose accuracy, completeness, or care, use the more capable model. Pick
from the models the session can actually launch. Do not inherit an expensive
parent model by default, and do not pin a product or model id in this file.
Use a low-cost model for lookups, inventory extraction, documentation
assembly, builds, tests, and routine checks when that model can match the
quality of a more capable one. Choose low effort for mechanical work and
medium effort for bounded reasoning. Escalate when the task's quality needs
it or a cheaper attempt falls short, and briefly state why.

Keep the primary agent available for user communication and coordination.
Delegate longer execution and research jobs, use nonblocking progress checks,
and avoid long blocking tool calls on the primary. User messages steer active
work; answer them promptly while delegated work continues. Small immediate
edits and decisions can remain with the primary when delegation would add
overhead.

Prefer the **least-expensive subagent whose output matches a more capable model** for builds and tests:

- `Explore` (fast read-only) for locating code, grepping symbols, answering "where is X."
- `Plan` for architecture and design before writing code.
- `general-purpose` only when the task needs multi-step reasoning or execution that Explore or Plan cannot cover.
- Use a more capable model for complex cross-crate reasoning, security review, or ambiguous architectural decisions, and for any standard edit, build fix, or test whose quality would drop on the low-cost agent.
- Do not spawn a subagent for a single-file edit or a direct grep you can do yourself.
