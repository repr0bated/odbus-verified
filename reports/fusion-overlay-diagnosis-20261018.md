# Fusion-assisted overlay deployment diagnosis

Recorded 2026-10-18, Codex `/root`, task `fusion-overlay-diagnosis-20261018` / OD-121. Owner selected **diagnose and prepare the fix for the existing deployment agent**. Devin retains activation and controller ownership. This task changed only this report, the proposed patch, the saved model response, and its own shared notes.

## Outcome

**15:23 UTC update after reading the actual Devin conversation:** the earlier layer-construction failures are confirmed and have recorded controller fixes; a separate schema-capture defect is confirmed in the preserved release input. The bridge startup cause remains uncaptured. The original diagnostic patch addresses evidence retention, not the schema defect. See the transcript reconciliation below.

**16:10 UTC follow-on completion:** the owner subsequently requested finishing that capture, a full workspace release including the cognitive rename, and Luna execution. The schema repair and cognitive environment-name corrections are implemented. Luna's full GNU workspace release passed exit0 in33m50s. Fresh opblob sealed an isolated74-blob catalog at `target/schema-capture-release-20261018.g7uixn/catalog`; its actual router/plugin schemas now use `/var/lib/tched-router/plugins`, and canonical `cognitive` replaces `cognitive_mcp`. This supersedes the unfinished-source status below, which describes the earlier diagnosis. The dated `schema-capture-finish-20261018` entry in [HANDOFF.md](../HANDOFF.md) records exact files, hashes and receipts. Tests remain unrun. Devin retains remaining cognitive release-configuration integration and activation; no live activation or bridge-startup fix is established by this follow-on work.

The demonstrated failure is **`op-grpc-bridge` readiness during activation**. The saved receipt shows catalog publication and several service starts, followed by a bridge timeout and restoration of the original state. No bridge stderr from that attempt was captured. It does not establish whether the run wrapper failed, the native bridge exited, a prerequisite remained pending, or the readiness check rejected a running process.

The prepared [diagnostic patch](fusion-overlay-diagnostics-20261018.patch) fixes the missing evidence path. It is **proposed, not applied, built, deployed, or tested**. It does not claim to repair the still-unidentified startup error.

## Fusion receipt and provenance

- Used the recovered checkout `/home/jeremy/litellm-fusion`, commit `883c7df95766d367d74eac887730facb1f3d8a2c`, origin `https://github.com/repr0bated/litellm-fusion.git`.
- Ran its existing `run_fusion.main()` with the deployment brief replacing its hardcoded sample question. No files in that checkout changed. Request-local wrappers supplied the complete evidence and nine mandatory skill files to the panel and analyst, and printed content-free final metadata. No credentials were included in the prompt.
- Final metadata: `invoked=true`, protocol `fusion-tool-v1`, **3 panel successes, 2 panel failures, analysis_available=true**. Final model: `openai/gpt-6-astra-pro`. The invocation exited 0. Provider completion callbacks appeared for all five members, but those callbacks do not prove accepted panel candidates. The two failure causes were not captured; no second request was made.
- Full response and receipt: [fusion-overlay-result-20261018.txt](fusion-overlay-result-20261018.txt). SHA-256 `15eafd899a7947b4409b511561ddc4f5bc3b2ea500338f5652cd85850c47e177`.
- Private local prompt: `/tmp/fusion-overlay-diagnosis-20261018.l9Qvwf/prompt.md`, SHA-256 `56f89c8ef5778e65dc9e330d47a263fe90f484a0422d8b3e81179d17da894c66`. Source/proposal scratch is in that same directory. The durable result and actionable findings are retained here.

The original conversation is OMP session `01a1483b-454b-7608-b8a1-2fdc42110cbf`, `/home/jeremy/.omp/agent/sessions/-tmp/2026-10-17T05-00-06-347Z_01a1483b-454b-7608-b8a1-2fdc42110cbf.jsonl`: line 23 has the fusion-model request; line 952 requests a repo without secrets; the tool receipt at 07:47:15Z confirms creation/push of `883c7df` to `main`. Latest title: “LITE: Verify Fable access through Hermes”; original title: “Install LiteLLM Fusion with OAuth”. Historical private visibility was recorded at 07:51:37Z; remote visibility was not rechecked today.

## Evidence independently checked

Checkout HEAD was `051a20d056afba074e21d797aa34f8947c543c5a`, with other agents' uncommitted controller and plugin edits. Current source is not proven byte-identical to the received release.

The installed supported launcher `sudo -n /usr/local/libexec/opdbus-release/op-release-launch status` returned active/previous/journal/readiness null, `transaction_in_progress=false`, and no overlay mounts. Slot A remains received `release-8189f894-2d36-4160-bdea-71ca669f3f00`, root 286; B is empty. This supports completed rollback, not successful deployment or proof that every recovery detail is flawless.

The actual receipt `/var/lib/opdbus-release/protected-recovery-20261018T102801Z/activate-a-20261018T135614Z.log` records:

1. Persisted and staged 74 blobs plus manifest.
2. Started op-runit-systemctl, opdbus-rundirs, op-session-bus, opdbus-grants, tched-router, and unified-execution.
3. `timeout: run: /etc/sv/op-grpc-bridge: (pid 25981) 12s`.
4. Stopped affected services and restored original state.

The `12s` is the observed process incarnation's age, not the configured timeout. The crash-loop description in HANDOFF is secondary evidence; the actual native error is missing.

The associated `runsvdir-title-20261018T135614Z.log` shows missing logger run targets beginning at 13:57:21, during rollback. Those warnings must not be promoted into proof that the logger files were missing while the candidate was mounted. The current source contains those three logger scripts.

Current bridge log files under `/var/log/op-dbus/op-grpc-bridge` end on October 9. The directory and files belong to `jeremy`; they were not changed or treated as evidence of the October 18 failure. Current source has no `deploy/runit/op-grpc-bridge/log/run`.

Metadata-only inspection found several absent optional/protected configuration paths, while TLS directory and root:root0600 router token file exist. No secret content was read. Absence alone is not a startup diagnosis: local-human initialization is explicitly optional for missing state in `mutation_engine.rs:875`, and decoy configuration reads at `decoy_network.rs:379` occur in admitted dispatch, not a demonstrated unconditional startup call.

## Prepared patch and review

The [patch](fusion-overlay-diagnostics-20261018.patch), SHA-256 `103002377638a703984912cea76bc693583dfe96eaf85215259688842a5eba82`, proposes only:

- A runit logger using the **existing `svlog` account**, with a new private directory `/var/log/op-dbus/op-grpc-bridge-release`. It drops privileges before writes, rejects incompatible existing directory/configuration state, and configures rotation at 4,000,000 bytes with ten retained archives and daily rotation. It neither deletes locks nor recursively changes ownership. Historical logs are preserved.
- Fixed startup-phase labels and dependency/address completion labels in the existing run wrapper. No environment dump or shell tracing.
- An early `finish` record of runsv's exit code and raw wait-status low byte, with stderr joined to stdout. Existing cleanup remains intact.

Additional checks resolved the prerequisite left open in Fusion's report: `svlog` exists (uid993/gid992), `/var/log/op-dbus` belongs to svlog:svlog0755, the proposed leaf is absent, and `chpst`/`svlogd` exist. `deploy/lib/golden/build-golden.sh:829` stages `run`, `finish`, `check`, and `log/run` as executable application release inputs. `services.rs:111`, `loggers.rs`, and `preserve.rs:152` already plan and materialize logger definitions. Installed runit manuals confirm that both service `run` and `finish` stdout feed the logger pipe and document the finish arguments.

These are source/manual/metadata checks, not an executed logger or activation test. The patch retains readiness criteria, timeout values, dependency behavior, identity handling, and native exec semantics. A pre-exec label proves only that execution was attempted. The controller's logger shutdown/drain behavior may still affect final-message retention; do not assume every trailing message is durable merely because a persistent logger was added.

## Next action for Devin

1. Review the patch against the exact release inputs you own; preserve concurrent changes. Apply it to source only if accepted. Reconcile the candidate's scripts and executable provenance with the checkout before drawing source-based conclusions.
2. Prepare and deliver a coherent release through the existing snapshot/send-receive/controller path. The source packaging helper already includes logger scripts; no manual live stub installation, binary copy, or received-snapshot edit is proposed. Scripts alone require no Rust compilation, but this report does not certify the rest of the current candidate's inputs.
3. Make one owner-controlled activation attempt with its receipt retained. Capture `/var/log/op-dbus/op-grpc-bridge-release/current` and any rotated segment covering the failure. Within the existing readiness window, take two bounded native `sv status` observations and, if available, correlate PID/start time with `/proc/<pid>/exe`. Do not dump environments. A separately launched `sv check` does not necessarily write into the service logger; retain its own failure output.
4. Stop after success or rollback. Record the first failing phase/error, finish record, candidate release ID, and final supported-launcher status. Distinguish rollback-induced TERM from the initiating failure.

| Observed next | Narrow repair direction |
|---|---|
| Wrapper exits before native execution | Repair the named runtime input, dependency, token constraint, storage preflight, or executable/NUMA failure through its existing owner. |
| Native bridge error before readiness | Follow the exact TLS/catalog/projection/audit/identity error against received bytes. Restore verified inputs or rebuild a coherent release as required; do not invent credentials or reset state. |
| Wrapper remains in a dependency/address wait | Return that exact unmet prerequisite to its owner. The controller's 30-second bound versus wrapper's 90-iteration waits is a real discrepancy, but not a proven cause. Change no timeout without observed forward progress and a justified bound. |
| Native process remains alive but check fails | Identify the first failed marker/socket/TLS/HTTP condition. Preserve the requirement for a CA-validated 401 response. |

The missing bridge error is the remaining blocker to an evidence-backed startup repair. No activation, application source change, Cargo command, tests, Clippy, formatting check, commit, or push was performed by this task. The final deployment status remains **received, inactive**.

## Latest Devin/Claude Fusion conversation, reconciled at 15:23 UTC

Owner requested reading the latest Devin conversation directly. Source: `/home/jeremy/.local/share/devin/cli/transcripts/aboard-stream.json`, session `aboard-stream`, 214 steps, October18 13:14:49–14:24:26 UTC. SHA-256 at read: `7590de592b21eb06b10c4a1ca2ac22ce704de2c0edd8f7868a624066b43e9e3b`. The session metadata names **Fusion (Claude Opus 5.5 Medium + SWE-2 Medium)**. The exported steps label the main responses `claude-opus-5-5-medium`, plus seven early tool steps `gpt-6-astra-medium`; metadata alone does not establish each internal Fusion contribution. This is a different Fusion implementation from the LiteLLM panel run above. These new findings were not submitted to that panel.

Read the user/assistant deployment and schema exchanges and their relevant tool receipts. No raw transcript was copied into the repository. Identity-bound NotebookLM remains unavailable because this session has no mounted 3tchedFS namespace.

### Earlier failures really did involve layers

The transcript preserves the kernel errors `at least 2 lowerdir are needed while upperdir nonexistent` and `failed to clone lowerpath`. Devin's step131 explains the two corresponding fixes: add a verified empty second lower directory, and retain the detached Btrfs mount descriptor in `SlotRoot`. The next recorded activation placed overlays, published the catalog and started six services before bridge readiness failed. A third fix admitted runit's transient supervision filenames during recovery. This supports progress past the original construction errors; it is not proof that every layer/content dependency is correct.

Current source corroborates the construction: `release/fs.rs::detached_overlay` receives the release directory and verified empty base as its two lower descriptors, with no writable upper. The underlying host directory and previous release are not additional lower layers. `release/mod.rs` switches all targets before restoring services. Consequently, missing files in the received payload do not fall back to an older application's files; host configuration outside the target list remains a separate dependency. No new mount or activation was performed to inspect a live candidate view.

### Later schema finding omitted from the earlier handoff

At step149 (14:08:56), Devin initially said the schema matched the binary. After the owner's correction, steps155–163 traced the actual sources and step164 (14:12:09) explicitly corrected that conclusion:

- `threetched` captures the binary with a fixed runtime config directory and manifest/digest checks.
- The then-shipped `tched_router.native_config` reads the embedded `tched_router_contract.rs` document; matching schema bytes do not establish a fresh capture on every seal.
- `model_routes`, `tched_plugins` and `tched_skills` separately capture `config schema` with the sealer's ambient environment.
- Step163's actual receipt finds `/root/.threetched/plugins` in `tched_plugins.2cb35996ce669377.blob`. At 15:22:59 UTC, a fresh bounded literal read independently found the same path in `/opt/op-dbus/release-send/release-8189f894-2d36-4160-bdea-71ca669f3f00/usr/share/op-dbus/plugin-blobs/tched_plugins.2cb35996ce669377.blob`.

**Provenance qualification:** Devin's blob inspection commands and the fresh check read the retained `release-send` snapshot, not a directly mounted received slot. Earlier controller status identifies the associated received release. Do not describe this fresh check as an independent byte comparison of source and received slots.

The confirmed defect is inconsistent schema capture/default paths in the release input. It is not yet a demonstrated cause of the bridge's exit or readiness failure. The record also shows that activation copied an already sealed 74-blob catalog; it did not reseal it. Repeating activation cannot incorporate later source edits into that catalog.

### Existing repair and next action

Devin's final step214 says its rewrite search found only the completed `threetched` capture, not a completed rewrite of all consumers; NotebookLM was unavailable. That is a bounded historical search result, not proof that no other copy exists anywhere. The owner requested a workspace rebuild at step172, then redirected Devin to recover the already-completed work at step175. No completed rebuild, new seal or deployment is established by that exchange.

The current dirty checkout independently contains Devin's six partial edits: `threetched.rs`, `tched_router_contract.rs`, `tched_router.rs`, `binary_namespace.rs`, `tched_plugins.rs` and `tched_skills.rs`, all under `crates/op-plugins/src/state_plugins/`. They route schema consumers toward the shared capture and remove the embedded import. They remain uncommitted and were explicitly described as unfinished/unbuilt. This task did not alter them or certify their runtime behavior.

The deployment-agent handoff now has two concrete items: retain the initiating bridge error using the proposed diagnostic patch, and reconcile/finish the existing shared-capture repair before claiming a coherent newly built/sealed release. Ensure the capture remains a seal-time operation and runtime consumers use the sealed catalog; sharing a helper alone does not prove that boundary. Then use the existing build, explicit seal, golden preparation and snapshot/send-receive workflow for the accepted source. Preserve the owner-directed search outcome and concurrent edits; do not start a duplicate rewrite or treat the current slot as containing them. Tests remain unrun and deployment remains Devin-owned.
