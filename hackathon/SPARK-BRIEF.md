# Etched in Schema — brief for Muse Spark

Hackathon: Meta Model API, **Startup Track — judged on a working product**.
Builder: Jeremy Hobson (3tched).

**Scope (owner, 2026-10-28):** the identity/3tched platform predates the
hackathon and is used through its interfaces. The new, judged development is
(1) compliance routing, (2) rule enforcement, (3) policy, and (4) a drop-in
consumer app (team chat first). The UI is optional. See README "Goals" for the
golden path. Sections below about the avatar, CMS demo and on-chain anchoring
are background, not the critical path.
State as of 2026-10-28. Everything marked **proven** was demonstrated on the
host that day; everything else is planned.

## The project in one paragraph

A self-contained 3tched identity — the "twin", the living embodiment of a
3tched subsidiary — turns a business goal into deployed, verified
infrastructure. It acts only through its own typed filesystem (3tchedFS),
every action is authorised against its capabilities and leaves a
tamper-evident receipt, and a Web3 index anchored on-chain lets anyone verify
who did what, under what authority, without trusting the agent or operator.
That link between on-chain artifacts and accountable, authorised actors is the
gap blocking Web3 adoption.

## Proven today

- **Identity container**: `3tchedCtlChatbot` (Incus, pool `3tched`, profile
  `3tched-lungs`), identity session `<subsidiary-session>`,
  service principal, persist volume, kept running by runit `chatbot-identity`.
- **First-person operation call**: inside the container, write the plain JSON
  arguments to `/run/mount/3tchedfs/operations/<plugin>/<method>/call` (open
  read-write, write, fsync, read back on the same handle). Example
  `identity_sled/get_own_identity` with `{}` returned the identity, operation
  `51c8f91c-c2f1-4688-a985-fb41684adfff`, chain events 10389/10390, outcome
  completed. The bridge identifies the caller from the kernel peer over the
  lungs Unix socket, checks the exact capability, commits via MutationEngine.
- **Infrastructure operations available to the identity**: `incus/*`
  (create/start/stop/snapshot/devices), `ovsdb_bridge/*` (bridges, ports,
  transact), plus ~70 plugins. Each call is checked against the identity's
  grants.
- **No network path to 3tched**: the twin talks to 3tched only through its FUSE
  mount. Its only network will be the internet (Meta API, blockchain).
- **Chat path**: UI (`http://<door-ip>:8080`) → door `<door-ip>:8090` →
  `chat_router.Chat` → zeroclaw router on `127.0.0.1:8084` (protocol
  `zeroclaw.v1`) → model. Every turn receipted.
- **Avatar**: `IdentityAvatar.svelte` speaks a fixed intro script (exact
  wording, no model) and answers spoken or typed questions via the chat path.

## Where things are

| Thing | Path |
|---|---|
| Backend (Rust workspace) | `/srv/git/odbus-verified` — read its `AGENTS.md` and `HANDOFF.md` first |
| UI (SvelteKit + json-render) | `/srv/git/3tched.com` (has uncommitted WIP — do not publish it as-is) |
| UI build actually live | scratch worktree of 3tched.com HEAD + `tched_router`→`chat_router` + avatar |
| This repo | `/srv/git/etched-in-schema` — Web3 index schema, LLC declaration schema |
| Recovered execution tracker | branch `recover/execution-tracker`, worktree `/srv/git/odbus-tracker-recover` (compiles; not deployed) |
| Router config | `/var/lib/tched-router/config.toml` (root; backups `*.bak-20261028-*`) |
| Qdrant | `127.0.0.1:6333` — `repos_specs_docs_voyage_4_lite`, `repos_repomix_voyage_context_4`, `snowball_footprints`, `blockchain_footprints` |

## Rules that must not be broken

- The plugin IS the schema. Backend schemas live only in Rust plugins under
  `crates/op-plugins/src/state_plugins/`, sealed into blobs. No standalone
  schema files in op-dbus (that is why drafts live in this repo).
- UI is Svelte/SvelteKit + Vite + json-render's Svelte renderer only. Every
  visible element is a catalog component.
- Never edit Cozo directly, never bypass a permission denial, never add or
  remove Btrfs members outside explicit identity provisioning.
- Deployment is `./activate-golden.sh` / `op-release-launch release`
  (btrfs send/receive slots). Never hand-copy binaries onto the host as a deploy.
- One Cargo process at a time; release builds need `CXXFLAGS="-include cstdint"`.
- Tests only when the owner asks.
- Commit and push at the end of every working turn (owner policy, 2026-10-28): public repo `github.com/repr0bated/etched-in-schema`, branch `master`. No turn ends with unpushed work.

## Plan (10 days)

1. OSCAL catalog of 3tched capabilities with a tag taxonomy; component
   definitions for 3–5 plugins (Compliance Trestle is installed: `trestle`).
2. Tag routing: profile → components → plugin operations.
3. Boundary rules compiled into capability grants and budgets (the bridge
   already enforces grants on every call).
4. "Create a CMS for this business": business declaration (start with
   `schema/llc-declaration.schema.json`) → profile → provision via the twin,
   receipts recorded as OSCAL assessment results and anchored on-chain.
5. UI: avatar intro, then "enough about me — here's what I can do for your
   business" (scripted once the business is chosen), vitals and nerve-trace
   views fed by real receipts, OSCAL views.
Golden path (must work three times in a row before any stretch goal):
scripted intro → "create a CMS for this business" from one small declaration
and one OSCAL profile → the twin provisions one or two real things through its
own mount, with a visible nerve trace and receipts → one receipt anchored
on-chain → a judge verifies it themselves.

Out of scope (owner, 2026-10-28): moving the identity between hosts
("astral projection") — no business case. Client acquisition is optional; add
it only if it fits the flow.

## Known open issues

- ~~Meta model not wired~~ — resolved 2026-10-28: Meta's Model API (`https://api.meta.ai/v1`, `muse-spark-1.3`) is wired for development (`~/.bash_secrets`) and the running service (`/home/3tched/.config/etched/env`); see `docs/RUNTIME.md`. 3tched's own chat router still uses a Qwen model.
- Router turn costs ~15k tokens (skills bundle removed to halve it). Qwen does
  not reliably emit text tool calls. (MCP is retired — replaced by the 3tchedFS
  mount — so do not rely on the router's MCP tools.)
- Live state reads through the mount (`plugins/*/value.json`) return EIO.
- Next release will fail activation: `notebook-sources-sync` cannot pass its
  30 s check after restart. Fix before releasing.
- Re-key of the core chatbot to the WireGuard server key (session
  `<root-session>`) has no supported op yet.
- UI overlay on `dashboard-release` is runtime-only; lost on reboot or release.

## CMS: compliant by construction (owner direction, 2026-10-28)

The CMS is the showcase because it comes out usable *and* evidently
controlled. Pick a handful of controls whose effect is visible on stage; each
control → a CMS behaviour → evidence (receipt or anchor):

| Control area | Visible behaviour | Evidence |
|---|---|---|
| PII handling (NIST 800-53 PT) | automatic redaction on publish; original kept only in the owner's private scope | redaction operation receipt naming the triggering policy |
| Audit (AU) | every create/edit/publish attributable | receipt chain: who, capability, when |
| Access control (AC) | roles compiled into capability grants | a live denial showing the exact missing capability |
| Retention (SI-12) | scheduled expiry/archive | scheduled operation receipt |
| Integrity | published versions hash-anchored | on-chain anchor a judge can check |

Pitch wording: "implements and evidences these controls" — never "compliant"
or "certified" (an assessor declares compliance).

## Remote identities: established design (owner, 2026-10-28)

WireGuard is only the establishment step: the peer key proves the identity
once, the decoy issues the signed peer proof (OIA1) and the session gets its
sealed credential (SID1). After that it is zero trust — every call carries the
credential and the door's interceptor re-verifies it, over any transport
(internet, Cloudflare). 3tched-fs `Credential` mode is the remote form of the
lungs. The mechanism exists; it needs testing and refinement, not a rebuild.
Do not add a mesh (Netmaker/Cloudflare) as an identity layer.

## Progress log

- 2026-10-28: Junie pinned to Muse Spark for this repo — new
  `.junie/config.json` sets `model: muse-spark-1.3`, `effort: medium`
  (matches `~/.junie/settings.json` launch default, provider `meta`).
  IDE Junie routes via JetBrains login, no key needed. Headless `junie --task`
  still unauthenticated ("Cannot find authorization", proven this date):
  needs `JUNIE_META_API_KEY` from `https://ai.developer.meta.com/`
  (documented in `docs/RUNTIME.md`); same key gap blocks runtime Meta calls
  for the drop-in.
- 2026-10-28: LiteLLM wired to Meta Model API — `META_API_KEY` (48ch),
  `META_MODEL=muse-spark-1.3`, `META_MODEL_BASE_URL=https://api.meta.ai/v1`
  present in `~/.bash_secrets` and `/home/3tched/.config/etched/env` (0600).
  Venv `~/.venvs/litellm-meta` (litellm lib) proven: `openai/muse-spark-1.3`
  + api_base override returns `litellm-meta-ok` (~7s, 204 reasoning tokens).
  Helper `tools/litellm_meta.py` uses only `META_*` env, `max_tokens=800`
  default. Note: SPARK-BRIEF line 148 `JUNIE_META_API_KEY` gap is separate
  (headless Junie auth); runtime Meta calls are unblocked.
- 2026-10-28: omp in this repo is meta+jev only — new `.omp/config.yml`
  (project scope, other dirs unaffected): `disabledProviders` denies all 22
  non-meta chat providers (proven: `omp models` here lists 1 provider,
  `openai`/`salad` gone; /tmp still lists 23, smol still global salad).
  `modelRoles` pins default/smol/slow/plan/advisor to meta, judge stays
  `typesafe/jev-latest`; all `task.agentModelOverrides` mapped to meta.
  `enabledProviders` allowlist does NOT work at project scope (tested).
  Shell env fixed for consistency: `~/.bashrc` now sources
  `~/.bash_secrets` first; `~/.profile` stale `/home/admin/.bash_secrets`
  path repointed to `$HOME`. Clean-env `omp -p --model meta/muse-spark-1.3`
  returns `omp-meta-ok` (~7s). Broker cred hash == `META_API_KEY` hash.
- 2026-10-28: OSCAL foundation built (planned, trestle-validated): `oscal/catalogs/etched-catalog/catalog.json` (5 controls: etched-pt-1 PII redaction, etched-au-1 audit, etched-ac-1 access control, etched-si-1 retention, etched-in-1 integrity; each names candidate plugin ops + evidence; `trestle validate` passes), `oscal/TAG-TAXONOMY.md` v1 (tag -> control -> operations), `oscal/profiles/small-business-portal/profile.json` (all 5 controls + two-person-rule tailoring; validated). `routing/DATA-MODEL.md` v1 proposes Request -> Route -> Decision -> Outcome before code. Next: implement pure `route(Request, Profile)` + policy engine skeleton.

- 2026-10-28 (owner): **Junie is out, omp is in.** omp is the coding agent for this repo (model `meta/muse-spark-1.3`, same Meta credential as `META_API_KEY`). Entry point: `AGENTS.md` → `START-HERE.md` → `CONTEXT.md`. Earlier Junie notes above are historical. New spec: `docs/APPROVALS-SPEC.md`.
- 2026-10-28: `approvals/` crate implements APPROVALS-SPEC §§4-7 as pure Rust (request types, canonical sha256 args hash, truth table with fixed 0.7/0.3 thresholds, single-use TicketStore; `cargo test` 12/12 incl. acceptance criteria 3 and 5). No I/O or network yet. Next: policy engine skeleton + `route(Request, Profile)`.
- 2026-10-28: Docs auto-publish live: `book/` (mdBook guide, single-source symlinks) + rustdoc API combined by `tools/build-docs.sh` into `site/` (gitignored), deployed by `.github/workflows/docs.yml` to https://repr0bated.github.io/etched-in-schema/ (verified 200 on guide + API). Local: `mdbook` 0.5.4 via cargo.
- 2026-10-29 (Claude): **omp 404 `model_not_found` diagnosed — Meta-side, not local.** Every `meta/muse-spark-*` ID 404s, including direct curl to `api.meta.ai/v1` (both `responses` and `chat/completions`) with omp bypassed; `/v1/models` returns HTTP 200 with `{"data":[]}`. omp's cache shows an authoritative 5-model list at 2026-10-28 15:49Z; last success 23:39:48Z, first 404 23:56:31Z. Key file unchanged since 17:38Z. Owner reports the same setup works from the laptop; open: whether the laptop uses the same key. `.omp/config.yml`: added `local` to `disabledProviders` and `retry.fallbackChains.meta/*: []` (global config fell back `meta/*` -> `local/lfm2.5-350m`, the tiny title model). Platform: `opblob seal-shm` changed `chat_router`/`threetched` blobs and the running bridge refused them (`compiled_sealed_contract_mismatch`); restored with `opblob stage-shm` from `/var/lib/opdbus/plugin-blobs`, `chat_router` verified working. Found: router config pins `model =` per provider (40 lines) and `chat_router` rejects any model other than the config's native selection — models should come only from live provider catalogs (owner rule). Proposed bridge fix: **Planned** (owner approval pending). `/usr/local/bin/3tched-callfile` lacks a shebang (sh runs it; broken).
- 2026-10-29 (owner, done by Claude): **Hackathon now lives on `odbus-verified` branch `hackathon`** (this file is `hackathon/SPARK-BRIEF.md` there; worktree `/srv/git/odbus-verified-hackathon`, branched from `restore/devuan-storage-20261013` at b28a343). etched-in-schema's full history (58 commits, master c779bed) was merged once with `--allow-unrelated-histories` under `hackathon/` (merge 4d815ca); verified all 58 SHAs, authors, committers, timestamps and trees identical and reachable. Tag `spark-provenance-20261029` -> 4d051ed (last Spark commit) in both repos. `hackathon` added to the root workspace `exclude` (f343af3). `/home/jeremy/Hackathon` now links to this directory. `repr0bated/etched-in-schema` is left as the original record; never rewrite its commits.
