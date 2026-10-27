# Unmodified ZeroClaw — owner correction

Task `identity-container-provision-20261024`; Codex `/root`; 2026-10-25T01:16Z.
Owner explicitly said: if vendor patches mean patching ZeroClaw at all, remove
it. Owner then reaffirmed SID1 as the main identity; acknowledged that our own
identity admission stays while patched upstream hooks are removed.

Original numbered identity provisioning remains the goal. Owner prepares disk;
no live identity/key/container/grant/storage/service/deploy change is authorized
by this source cleanup alone. No tests/checks/Clippy/formatting/smoke fixtures run.

## Ownership and source state

- `/root/label_path_inventory`: `vendor/tched-router/**`. Frozen. Removed four
  patches, patch apply/regeneration scripts, inventory and base manifests.
  Recipe builds pinned native `zeroclaw` with `--locked`; archive unchanged.
  Native version remains `zeroclaw 0.8.4`, external filename `threetched`.
- `/root/numbered_storage_paths`: shared capture/provenance in
  `threetched-surface.rs`, `fuse_process.rs`, op-plugins `threetched.rs`; golden,
  tched-router runit bootstrap, prepare-salad-control-plane, source exporter and
  deploy documentation. Independent source review is clear. Native
  `ZEROCLAW_CONFIG_DIR` replaces patched alias. Native config/provider/bootstrap
  operations are retained. Unsupported filesystem opt-in fails before grants or
  secrets are changed; existing ownership receipts remain untouched.
- `/root/numbered_provision_review`: `lungs_ingress.rs`, `lungs_socket.rs`,
  filesystem `main.rs`. Root source review clear: removed patched prompt callback,
  special terminal decoder and application-ticket environment. Generic tickets,
  mounted identity/capability/kernel/mount/deadline checks remain. Stale captures
  of patched `stateless-generate` are denied before execution.
- Root: removes obsolete `generate_spec` schema/types/capability/dispatch/fixtures,
  corrects renderer method count and version to5.0.0, keeps its admitted seal
  through async enrichment/dispatch/publication. Preserves scoped application
  prompt and Canvas validation. Removes only callback-specific argument/prompt
  helpers and patched credential stdin injection. Updates AGENTS and .gitignore.
  Root reviews source changes; peer independently reviews SID1/admission.

Before snapshots, never overwritten:
`/var/tmp/identity-container-provision-20261024/vendor-before/` and
`/var/tmp/identity-container-provision-20261024/ui-retirement-before/`.
They preserve unrelated WIP. No reset/stash/commit.

## Contract

Manifest `opdbus.threetched.v2`:
- upstream URL/tag/commit and raw/compressed archive hashes;
- `source.mode=unmodified-upstream`, exact upstream Cargo.lock hash;
- artifact name `threetched`, upstream binary `zeroclaw`, binary hash/bytes,
  `x86_64-unknown-linux-gnu` target; compiler/Cargo versions.

No patches object or required custom command marker. Native config get/set JSON,
config patch/init, providers/agents/risk/runtime profiles and daemon flags were
checked against the pinned archive; early assumptions that they required patches
were corrected before edits. Native code does not read the patched SID1 receiver
variables or implement custom stateless generation/callbacks. Those removals do
not remove SID1 from our identity subsystem or authorize bypassing its checks.

## Build and remaining work

Attempt3 before this correction: opblob passed; bridge exited101 with exactly the
old generation dispatcher and admitted-schema mismatch. Receipt in adjacent
`build-attempt3.json`; not the current release build.

Attempt4 native worker launched as jeremy, PID1306, shell source
`/var/tmp/identity-container-provision-20261024/attempt4/build-native.sh`.
Persistent native-build.log and native-build-receipt.json in that directory.
Uses normal Cargo/JEV with stable GNU rustc1.99.1 and matching explicit
RUSTC/RUSTDOC, shared target, closed stdin and a separate process group.
Output: `/var/tmp/identity-container-provision-20261024/unmodified-artifact/`.
Source archive cache is separate from the shared Cargo target. No installation.

Attempt4 completed successfully at00:52:33Z in12m27s. Binary SHA256
`454ff4c18822535e782b0f9010d348302d4555709872b9f0e3830239c07ddf65`;
manifest `a08341900e3fc3bb7a1f4f263bbbfe23c2e73a2eced66724bc3359e01e48bfad`.
Capture now isolates every schema/help/version child in one private temporary
config/HOME/XDG/temp context, with explicit success cleanup and error RAII.
Existing tempfile3 is a production dependency; runtime paths stay descriptor-only.
This source change and downstream SID1 preservation were independently reviewed.

Attempt5 failed before compilation (Cargo missing from detached PATH, exit127).
Attempt6 fixed only the worker PATH and completed the targeted GNU opblob, bridge
and 3tched-fs build at01:12:43Z, exit0, using this new capture pair. The freshly
built opblob then prepared an isolated catalog at01:13:02Z, exit0. It contains71
plugins, including `threetched`, without `tched_router`; manifest SHA256
`b437baf2f873b2876775d34db739a052a3bc0cc986ee4063218cc4bb003d4f09`.
Receipts and artifact hashes are copied beside this report. No Cargo remains
active for this task. No live catalog was changed and no deployment occurred.
Do not reuse patched capture artifacts. Offline seed preparation stages matching native
artifacts and an empty private config bind from existing identity persistence;
final source review is clear, but no seed helper or live operation ran.

Known separate incomplete own-code path: op-chat-responder::our_stack_generate
unconditionally refuses; large_language_model.stream_generate still forwards to
a retired tched_router generation path. Do not bypass SID1 response/ACK checks or
restore patched upstream generation to make it run. Source retirement/build does
not prove UI generation or identity provisioning end-to-end.
