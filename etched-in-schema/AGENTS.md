# AGENTS.md — etched-in-schema (read by omp)

**Core rule — 3tchedFS dev cycle:** every method is a sealed blob; the identity acts only by writing to its mount's call files (`operations/<plugin>/<method>/call`) and keeps the receipt. New methods: logic here → plugin registration → sealed blob → operation file → call → receipt (`docs/DEV-CYCLE.md`). No MCP, no direct API calls, no `ovs-*` commands.

Start with `START-HERE.md`, then read `CONTEXT.md` in full (it bundles every doc).

## Mandatory skill preload

At the start of every session, before any work, read these files completely
(base: `/srv/git/odbus-verified/.agents/skills/<name>/SKILL.md`). A sub-agent must
repeat the preload for its own session.

- **Always:** `verification-before-completion`, `agent-self-evaluation`, `quality-auditor`, `application-security`, `requesting-code-review`, `receiving-code-review`
- **UI (Svelte + json-render):** `svelte`, `svelte-code-writer`, `sveltekit`, `json-render`, `core`, `devtools-svelte`, `web-design-guidelines`, `accessibility`, `typescript-patterns`, `zod-validation`
- **Rust (router, policy, enforcement, approvals):** `rust`, `rust-patterns`, `rust-testing`, `rust-memory-optimization`, `memory-safety-patterns`
- **Compliance (OSCAL):** `trestle-workspace`, `trestle-oscal-models`, `trestle-authoring-workflow`, `trestle-control-implementation`, `trestle-validation`, `trestle-governance`, `trestle-assessment`, `trestle-compliance-pipeline`, `trestle-poam`, `trestle-task-system`, `compliance-check`
- **Judgments (Jev):** `typesafe-ai`
- **Platform (3tched, used through interfaces):** `grpc-expert`, `grpc-protocol-expert`, `grpc`, `protobuf`, `incus`, `btrfs`
- **Network / OVS / OpenFlow:** `ovs-db-analysis`, `network-config-validation`. There is no OpenFlow skill: for OpenFlow use the platform's `openflow` plugin source (`/srv/git/odbus-verified/crates/op-plugins/src/state_plugins/openflow*.rs`) and the vendored `rovs-openflow` library (`/srv/git/odbus-verified/vendor/rovs-openflow`), and change flows only through the plugin, never with `ovs-*` commands.
- **FUSE / 3tchedFS:** `fuse`, `kernel-fuse-diagnosis`
- **Memory and workflow:** `unified-memory`, `using-superpowers`

UI references (from-scratch UI): `ui/README.md`, `ui/reference/`.

This repository is PUBLIC: placeholders only, push often, never rewrite history, never use `--no-verify`. Enable the guard once per clone: `git config core.hooksPath .githooks`.
Runtime and rules: `docs/RUNTIME.md`; specs: `docs/DROP-IN-SPEC.md`, `docs/APPROVALS-SPEC.md`.


Read `SPARK-BRIEF.md` first: it is the current project state, paths, rules and plan.
Then `README.md`, `docs/3TCHED-RESOURCES.md`, `docs/SUBSIDIARY-SLED.md`, `docs/PRIOR-ART.md`, `docs/WINNING.md` and `docs/DROP-IN-SPEC.md` (the drop-in to build).
For any backend work, also read `/srv/git/odbus-verified/AGENTS.md` and its `HANDOFF.md`.

- Schemas here are drafts. They become sealed 3tched plugins (Rust) later; do not copy them into op-dbus as files.
- Keep claims honest: mark what is proven vs planned.
- Record decisions and progress at the end of `SPARK-BRIEF.md` (dated), so Claude, Codex, Grok and Spark share one state.
