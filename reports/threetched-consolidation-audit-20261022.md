# LiteLLM Fusion consolidation audit

Audit date: 2026-10-22
Reviewer: Hermes Agent
Repository publication: 2026-10-22 (documentation only; findings retain their audit-time scope)
Task: threetched-consolidation-fusion-20261018 / OD-121
Backend repository on VPS: /srv/git/odbus-verified
Backend HEAD: 051a20d056afba074e21d797aa34f8947c543c5a (shared dirty checkout)
UI repository on VPS: /srv/git/3tched.com
UI HEAD: 31eae4be68ce437e5c0d5c90c0a08d4a319630fa

## Verdict

The claimed consolidation and historical build results are substantiated, but the audit is not a clean pass. One integration defect concerns Fusion's added noninteractive-editor refusal. Three other findings concern current deployment drift, later guest-layout drift, and a preexisting UI incompatibility. Those three must not be attributed to Fusion as newly introduced defects.

The final Fusion continuation explicitly did not claim current service readiness, test-suite success, authenticated runtime exercise, or a new deployment. This audit found no basis to label its recorded build/removal claims fabricated.

## Task Identification

The original scope and final reconciliation are in:
  /srv/git/odbus-verified/reports/threetched-consolidation-handoff-20261018.md
  Original requirements: lines 12-40
  Final reconciliation: lines 679-794

The continuation trace is:
  /srv/git/odbus-verified/reports/threetched-consolidation-fusion-20261020-restart/agent.jsonl

Its final assistant record identifies provider consolidation-fusion, model litellm/fusion-1. The final completion timestamp is 2026-10-20T17:26:34.020Z. The saved initial prompt plus separator plus evidence reproduces the original submission digest.

## Findings

1. High operational priority — mounted views and active catalog disagree.
   Attribution: current deployment drift, not a demonstrated Fusion-introduced bug.
   Verification: observed directly with read-only VPS commands; rechecked after the source reviews finished.

   Both mounted views still pin generation 72:
     hash 6f576dbdcdb4416f3471028393be42852078b3ed54c6439fe28c79d7248d260c
     /run/mounts/3tchedFS/view.json: 70 plugins
     /run/mounts/threetched/view.json: 2 plugins

   Both active and packaged manifests instead declare generation 144:
     hash 151ed2d74e3479659703026d1ccf8bba8e6b281c22d18f93d169f2a32d17ab08
     /dev/shm/opdbus/plugin-blobs/.manifest.json
     /usr/share/op-dbus/plugin-blobs/.manifest.json

   Sampled public-state reads of both paths failed with Input/output error:
     /run/mounts/threetched/plugins/tched_router/data/.value.json
     /run/mounts/3tchedFS/plugins/tched_fs/data/.value.json

   The exact hash/generation equality check is in crates/threetched-fs/src/source.rs:42-59, called before public state reads by namespace.rs:691-726. The mount adapter also requires pin parity at deploy/runit/libexec-3tched/mount-3tchedfs:79-89. The observed mismatch fails those checks; no authenticated mutation was attempted.

   Of the 72 per-plugin hashes in the generated report, only emqx differs from the active manifest. Nevertheless, the whole-catalog pin changes. The active manifest no longer matches the generator's retained checksum receipt, while all three generated output files and the recorded generator/source digests still match.

   The current manifest modification timestamps are later than Fusion's final completion. Its report expressly leaves the separate release incident and no-restore boundary in force. Reconciliation belongs to the authorized release owner; this audit does not authorize remounting, restoration, resealing or activation.

2. Medium — deterministic unsupported editing is reported as an unknown outcome.
   Attribution: integration defect in the guard added by the consolidation.
   Verification: source-proven path, not a live editor invocation.

   crates/op-plugins/src/state_plugins/threetched.rs:292-296 refuses skills.edit and skills.add with --edit before spawning. The refusal is an ordinary anyhow error, propagated from prepare_command at line 343.

   crates/op-grpc-bridge/src/method_receipt_audit.rs:556-578 catches backend errors and records status unknown with backend_or_publication_failed, returning method_outcome_unknown. The otherwise-admitted caller therefore loses the actionable unsupported/before-effect explanation even though this particular request was deterministically refused before running the command.

   This does not mean the command executes, or that retiring the old contents API was wrong. It means the safe refusal is not carried through the public receipt boundary. The original requirements explicitly require truthful unsupported/unavailable outcomes and preserved receipt semantics (handoff lines 30 and 34).

   Recommended correction: preserve a typed, sanitized pre-effect refusal through admission/auditing, or perform the authorized preflight before the backend-effect boundary. Do not weaken the general unknown-outcome policy for genuinely uncertain backend errors.

3. Medium — workspace links point into a view that excludes their targets.
   Attribution: current integration drift after the later dual-guest-view change, not proven newly introduced by Fusion.
   Verification: source-proven mismatch; guest link resolution was not exercised.

   deploy/workspace-agent/populate.sh:48-50 creates three links beneath /run/mount/3tchedfs/plugins/ for threetched and tched_router.

   deploy/workspace-image/prepare-lungs-client.sh:66-70 excludes both plugins from that view and captures them in the upstream view. deploy/workspace-image/mount.fuse.3tchedFS:8-9 mounts that upstream view at /run/mount/threetched.

   Under this declared guest layout the three authored links are dangling. HANDOFF identifies the dual-view change as seed-dual-fuse-services-20261019. Reconcile the population links with the selected guest mount root; do not change grants or bypass the normal operation path.

4. Medium — existing UI parser rejects canonical native declarations.
   Attribution: preexisting consumer incompatibility, not introduced by Fusion.
   Verification: source-proven validation failure; no live browser/transport reproduction.

   In /srv/git/3tched.com/src/chat/threetchedfs-surface.ts:25-27, binaryMethods accepts only plugin_id zeroclaw, forbids dots in method names, and requires plugins/zeroclaw/operations/... paths.

   The backend's crates/op-plugins/src/state_plugins/threetched.rs:25 declares threetched and publishes captured command names directly at lines 183-203. Those names include dotted commands such as skills.add. A canonical declaration cannot satisfy the parser. The consumer calls it from src/chat/door-chat-adapter.ts:209-211.

   The identical parser is present at UI HEAD, committed October 9. This is not evidence that Fusion broke a previously valid UI caller. It is an unresolved canonical-surface integration gap. No working legacy transport was assumed.

## Claims Verified

- Complete comparison of the saved baseline and all 72 mounted plugin schemas confirms 74 -> 72 plugins and 1,388 -> 1,357 methods.
- Exactly 33 methods were removed: 18 tched_plugins/tched_skills wrappers, seven router methods, seven LLM facades, and gemma_brain.route.
- Exactly two unrelated methods were added: identity_sled.prepare_container and incus.reconcile_creation. Net reduction is therefore 31, not 33.
- Generated specialization covers the mounted method set exactly: 1,182 generated plus 175 explicitly unsupported methods. Eleven state/public-state subjects are also unsupported. All 147 native threetched methods remain sealed but are not statically specialized because of allOf. The report discloses this limitation.
- Retained large_language_model.stream_generate uses the canonical StartGenerationInput and GenerationMutationOutput, targets tched_router.start_generation, and retains nested admission, identity, origin and private-job ownership checks in the inspected paths.
- Targeted production searches found no surviving callers of the retired wrapper IDs, removed router accessors or gemma_brain.route. This is scoped search evidence, not an assertion about every possible external client.
- Exact per-method hashed native capabilities and queued session/seal/grant rechecks remain in the inspected dispatch path. Old grants were not silently widened to make the breaking cutover work.
- Native wrapper argument differences, including --yes, --no-tier-banner, --no-scaffold, changed result envelopes and retirement of interactive contents editing, are documented. They were not automatically counted as bugs.

## Build And Artifact Evidence

Historical tool-result rows were recovered read-only from the VPS /home/jeremy/.hermes/state.db. Relevant messages include 3848 (corrected GNU workspace build exit 0), 3954 (seal/specialization/export/provenance exits 0), 3957 (exact catalog delta), 3962 (11m27s release build finish), and 3994 (historical mounted parity). Agent prose was not treated as equivalent to a tool-result receipt.

Fusion's targeted generated-consumer receipt survives at:
  /var/tmp/typed-use-generated-1792516892107.receipt
  /var/tmp/typed-use-generated-1792516892107.log

It records exit 0 at 2026-10-20T17:24:20Z for:
  cargo build --release --target x86_64-unknown-linux-gnu -p tched-fs --example typed_use_generated

Generator receipts survive at:
  /var/tmp/threetched-generated-example-20261020.40Ea0x/

The original isolated scratch catalog has been removed, as Fusion disclosed. Its original contents cannot be reread; historical execution records and matching generated artifacts are the retained evidence. Historical activation evidence is not current readiness proof.

## Scope And Limitations

Read-only review of affected plugin/bridge paths, targeted HEAD diffs, canonical native argument mappings, generated artifacts, mount/workspace definitions, the UI consumer, historical execution records, and current public mounted metadata/state reads. Two independent source reviews were checked against the exact source before inclusion here.

No build/test suite was rerun; no authenticated operation, native command, browser interaction, service change, deployment, grant change, identity change, staging, reset, stash or commit was performed. The shared dirty tree contains other agents' work; findings are attributed narrowly. Deferred OD-122/OD-123 and documented policy-migration debt were not reclassified as automatically completed or newly broken.

No application or deployment files were changed by this audit. This repository report preserves the completed audit originally saved at /home/jeremy/Work/audits/fusion-consolidation-20261022.txt. Publication adds this report and its HANDOFF.md reference only; no remediation or commit is included. Raw local evidence collected during the audit is under /home/jeremy/.hermes/cache/scratch/fusion-consolidation-audit/ and is temporary; authoritative source and surviving VPS receipt paths are cited above.

## Remediation follow-up — 2026-10-23

Owner requested fixes for findings 2–4. The following records source changes,
not a rerun of the original audit or proof of deployed behavior.

### Finding 2: typed pre-effect refusal

The two noninteractive editor guards now return `UnsupportedBeforeEffect` with
static, sanitized diagnostics. Admission, queued-call rechecks, runtime
provenance and sealed argument validation remain ahead of those guards; command
construction and spawning remain after them.

Canonical method auditing recognizes only that concrete error type. It records
the existing `failed` terminal/recovery state and produces `AuditedMethodRefusal`
only after terminal-event and journal persistence succeed. The public wrapper
does not expose the raw marker as a source error: a nested refusal cannot prove
an outer operation had no effects. Ordinary backend failures, copied refusal
text, cancellation, panic and publication uncertainty retain unknown handling.

Adapters preserve the distinction as gRPC `Unimplemented`, D-Bus `NotSupported`,
an MCP tool error, and filesystem `EOPNOTSUPP`, retaining recovery correlation.
Changes are in `op-state-store/src/method_receipt.rs`,
`op-plugins/src/state_plugins/threetched.rs`, the bridge's
`method_receipt_audit.rs`, `grpc_server.rs`, `schema_router.rs`, and
`mcp_frontend.rs`, plus `threetched-fs/src/dispatch.rs` (all under `crates/`).

### Finding 3: guest workspace view paths

`deploy/workspace-agent/populate.sh` now authors these upstream-view links:

| Workspace link | Target |
|---|---|
| `/agent/shared/threetched-plugins` | `/run/mount/threetched/plugins/threetched` |
| `/agent/skills/runtime` | `/run/mount/threetched/plugins/tched_router` |
| `/agent/skills/skills` | `/run/mount/threetched/plugins/threetched` |

Cognitive and agent-config links remain in `/run/mount/3tchedfs`. The workspace
README documents the split and manual link checks. This task did not rewrite
populated bases or existing guests; their update remains within the storage
owner's image-update/provisioning lifecycle.

### Finding 4: canonical native UI declarations

In `/srv/git/3tched.com`, the parser now accepts canonical `threetched` command
names, including dotted names, while requiring exact operation paths,
matching declaration names, and consistent supplied hashed capabilities/subids.
Capabilities remain opaque and declaration-supplied; no grant or hash is
inferred client-side. Traversal, encoding tricks and trailing line terminators
are rejected by the source validation.

Related client methods, panel/navigation names and fixtures use `threetched`
without legacy aliases. The model-list action now calls the declared
`models.list` with `{}`, not the retired `models`/`subcommand` combination.
Explicit effect confirmation, identity transport and no-automatic-retry
behavior remain intact. Upstream ZeroClaw provenance references are unchanged.

### Verification and deployment boundary

No build, test suite, linter, formatter, smoke check, browser exercise or live
deployment was performed by this remediation task. Backend edits were prepared
in isolated copies until the concurrent registered-session build exited, then
integrated after matching all seven source baselines. That separately built
artifact predates these backend changes.

Behavioral regressions were added/updated but not run. Backend coverage targets
typed/contextual refusal propagation, nested/raw/lookalike distinctions,
editor aliases, sanitization and transport classification; UI coverage targets
canonical declarations, unsafe/mismatched metadata, model-call arguments,
confirmation and denial handling. No host-dependent Snowball/Btrfs fixture was
added for this repair.

Suggested manual checks, with Cargo serialized through the existing build owner:

```sh
# /srv/git/odbus-verified
cargo test --target x86_64-unknown-linux-gnu -p op-state-store method_receipt::tests
cargo test --target x86_64-unknown-linux-gnu -p op-plugins state_plugins::threetched::tests
cargo test --target x86_64-unknown-linux-gnu -p op-grpc-bridge method_receipt_audit::tests
cargo test --target x86_64-unknown-linux-gnu -p op-grpc-bridge unsupported_
cargo test --target x86_64-unknown-linux-gnu -p op-grpc-bridge method_refusal_survives_dbus_and_router_status_mapping
cargo test --target x86_64-unknown-linux-gnu -p tched-fs dispatch::tests

# /srv/git/3tched.com
npm test -- src/chat/threetchedfs-surface.test.ts src/chat/door-chat-adapter.test.ts src/workbench/Workbench.test.ts
npm run type-check
```

After an explicitly authorized build/deployment, manually exercise a disposable
authorized editor request: expect an actionable unsupported refusal, no editor
subprocess, and recovery status `failed`. Separately exercise denied/revoked
authorization and controlled persistence/backend failures: they must not become
successful or falsely definitive refusals. Durable terminal/journal behavior
has not been integration-tested here. After normal guest preparation/startup,
check the three links above resolve in the upstream view. In the deployed UI,
discover native methods and exercise the confirmed `models.list` call through
the existing caller-bound route; route availability and authenticated end-to-end
behavior remain unverified.
