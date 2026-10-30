# Provisioning audit — D-Bus first

- Task: `provision-audit-20261018`, follow-up to OD-111.
- Reviewer: Hermes / GPT-6 Astra Pro, with independent read-only storage and identity-contract reviewers; findings below cross-checked by the parent.
- Evidence timestamp: 2026-10-18T19:25:38Z.
- Repository: `/srv/git/odbus-verified`, branch `restore/devuan-storage-20261013`, HEAD `051a20d056afba074e21d797aa34f8947c543c5a` plus existing shared working-tree changes.
- Request: audit the provisioning script; owner correction: “be sure using dbus first”. This is review, not authorization to provision, repair, change grants, build or deploy.

## Verdict

Do not treat `deploy/scripts/provision-workspace-subscriber.sh` as a working end-to-end provisioner against the current deployed namespace. Its older identity flow has independent path, operation-receipt and adoption-plan incompatibilities. The newer partitioned-disk flow has guest-FUSE and upgrade-root lifecycle defects. Direct native control and grant edits also fall outside the canonical D-Bus admission/readback path.

The two flows are deliberately different: the legacy identity mode expects an unpartitioned leaf and plan versions 2/3; the manual GPT/overlay path uses version 4. Version 4 and its `partitioned_overlay` field are recognized by the owner. Findings about one flow are not assertions that the other has the same defect. Later-phase legacy findings are latent behind its earlier failures, not observed live outages.

## Findings

### F1 — High: legacy operation calls use a removed receipt interface

Location: `deploy/scripts/provision-workspace-subscriber.sh:331-343`, callers at `396-401`, `454-455`, `493-507`.

`lungs_call` writes and closes `operations/<method>/call`, then looks for sibling `error` and `result` files. Current `crates/threetched-fs/src/namespace.rs:1027-1037` projects only `call`. The live `inspect_provision_preflight` directory likewise contains only `call`, `input.schema.json`, `metadata.json`, and `output.schema.json`.

Terminal receipt/diagnostic bytes are per-open-handle (`fuse_fs.rs:600-623`), and the supported mounted client writes, fsyncs, seeks and reads the same descriptor (`mounted.rs:394-410`). Closing the wrapper's write-only descriptor discards its access to that receipt. Even after correcting paths, the initial inventory gate cannot consume the current interface.

Every invocation is also on the left side of `||`; Bash's conditional-function errexit behavior means the internal `set -e` is not a substitute for explicit checks on the write and first `jq`. On an older mount with sibling receipt files, the wrapper additionally lacks request correlation and mount-continuity checks. That stale-receipt possibility is conditional, not a demonstrated exploit on the current mount.

Repair direction: use the supported mounted `invoke` path, keep its same-handle terminal diagnostic, and preserve its contract/mount validation. Do not add a parallel backend client or restore global receipt files.

### F2 — High: both adoption entry points select absent host paths

Locations:
- `provision-workspace-subscriber.sh:290,324-328`: `/usr/local/bin/3tched-fs`, `/run/mount/3tchedFS`.
- `prepare-chatbot-adoption.sh:26-29`: `/opt/op-dbus/current/bin/3tched-fs`, `/run/mount/3tchedFS`.

Read-only `stat` verified all three historical paths are absent on this host. The actual client is `/usr/local/libexec/op-dbus/bin/3tched-fs`; `findmnt` confirms `/run/mounts/3tchedFS` is mounted as `fuse.3tchedFS`.

Effect: default legacy invocation stops before owner admission; the new preparation helper cannot issue its owner preflight. Fixing only the executable does not fix the mount or F1. Guest mount paths are a different contract and must not be globally renamed to match the host.

### F3 — High: legacy plans contain a field the owner rejects

Location: `provision-workspace-subscriber.sh:459-474`, especially `principal_group` at line 464.

The digest-pinned plan is decoded as `CleanPlan`, which has `#[serde(deny_unknown_fields)]` and no `principal_group` field (`crates/op-grpc-bridge/src/identity_clean_provision.rs:52-77,575`). A valid digest selects this decoder through `identity_seed_provision.rs:719-728`.

Effect, once the earlier blockers are repaired: adoption rejects the wrapper's plan after the stopped native container, writable leaf and mounts have already been created. The intentionally retained candidate requires reconciliation; rerunning creation is not a repair.

Repair direction: reconcile the author with the existing owner contract. Group policy is a separate authorization concern; do not silently expand the adoption schema merely to accept this field.

### F4 — High: binding the new root deletes the guest's FUSE mount declaration

Location: `deploy/scripts/bind-instance-root.sh:76-78`.

The helper truncates the inherited `/etc/fstab` to two comments. Image preparation explicitly appends `3tchedfs.fstab` (`deploy/workspace-image/prepare-lungs-client.sh:57-63`), and the guest service calls `mount /run/mount/3tchedfs` with no source/options (`deploy/workspace-image/3tchedfs.service:7-11`), relying on that declaration.

Trigger: documented initial provisioning with an empty upgrade layer, so the seed-root `/etc/fstab` is the effective guest file. Effect: guest FUSE cannot mount through its service even though the root-binding receipt sets `guest_fuse_configured=true` (`bind-instance-root.sh:91-95`). This is a source-to-source incompatibility; the current private guest filesystem was not inspected.

Repair direction: preserve the intended FUSE declaration while removing only unsafe host/physical-storage mounts. A blanket fstab replacement is not sufficient.

### F5 — High: normal upgrade synchronization returns success with the native root detached

Location: `deploy/scripts/sync-instance-upgrade.sh:88-109,132-162`.

Both `sync` and `boot` detach agent/persistence mounts, native root, merged view and lower-layer mounts before transfer. Both publish phase `paired`, but only `boot` rebuilds the mounts. Thus normal `--sync-upgrade` on a bound, stopped guest exits successfully with its root absent.

Canonical start verifies the overlay and all sibling mounts (`identity_clean_provision/storage.rs:289-326`, `partitioned.rs:107-125`), so a later start cannot accept that state. `bind-instance-root.sh:15-16` is not an ordinary recovery path: it only admits `container-defined` with `root_bound=false`, not the already-bound receipt.

Repair direction: define and implement complete post-sync restoration for an existing bound guest, retaining the no-live-lower-mutation rule. Publish completion only after mount/readback verification.

### F6 — High, latent legacy phase: a selected group can invalidate grants for every principal

Location: `provision-workspace-subscriber.sh:264-267,525-548`.

The script accepts any name from its closed group list, but writes that name without verifying the group's definition exists in the actual policy document or validating the replacement document. `interceptor.rs:1818-1828` requires referenced groups to exist. An otherwise valid policy without the selected group becomes invalid after this edit.

The materializer rejects the entire document, removes readiness and publishes `{}` (`src/bin/op-grants-materializer.rs:95-111`); runtime grant readers also validate the whole policy. Consequence: selecting a syntactically accepted but undefined group can remove effective access for unrelated principals once the materializer observes the replacement.

No live grants were read or changed. This finding does not assert that any particular group is currently missing.

### F7 — High, latent legacy phase: running/boot-enabled completion precedes grant publication

Location: `provision-workspace-subscriber.sh:503-548`.

The sequence is adoption, optional start via `touch_session`, active/native readback, `seed-enable-boot`, then checking and rewriting the grants file. Missing/unwritable policy, malformed JSON, absent `python3` (not in dependency precheck at 281), or F6 therefore happens after the guest can be running and boot-enabled under its old grants.

Even a successful replacement is not followed by materialized-policy readback. `OP_CAPABILITY_GRANTS` selects the script's file independently of materializer `OP_GRANTS_SOURCE` and reader `OP_GRANTS_PATH` (`op-grants-materializer.rs:140-150`, `interceptor.rs:1845-1854`). Selecting a group also retains explicit principal capabilities through `setdefault` at line 536; it is not a complete privilege replacement.

Repair direction: keep authorization changes separate and explicitly approved, validate and verify their effective state before depending on them, and do not publish full provisioning completion ahead of a required authorization step.

### F8 — Medium/security, latent legacy phase: whole-policy replacement can undo another writer's revocation

Location: `provision-workspace-subscriber.sh:530-544`.

The script reads and atomically replaces the entire grant document, without a policy-wide writer lock or compare-and-swap. Its lock at 314-319 covers only this provisioning session. If another authorized writer changes an unrelated principal between the read and replacement, that update is lost; a concurrent revocation can be restored from stale input. Atomic rename prevents partial-file visibility, not lost updates. No post-write policy readback detects this.

### F9 — Medium/control-plane: implemented native operations are invoked outside D-Bus admission

Locations:
- `provision-workspace-subscriber.sh:227-245`: standalone image creation and start use direct Incus CLI.
- `bind-instance-root.sh:66-74`: native device removal/addition uses direct Incus CLI.

Existing plugin implementations include image-based `create_instance`, `start_instance`, `add_device`, and `remove_device`, declared in `crates/op-plugins/src/state_plugins/incus.rs:2947-2998,3078-3102`; device changes perform ETag-conditioned native updates at 2855-2897. Canonical dispatch coordinates observation and commits full authoritative state (`incus_observer.rs:193-221`). Direct CLI calls skip that caller admission and coordinated publication path.

For the requested D-Bus-first behavior, prefer the declared identity-bound methods wherever they cover the operation; preserve capability checks and stop on denial. This is a control-plane/audit-boundary violation, not proof of unprivileged escalation.

Important exceptions: do not claim the generic method can already replace empty Incus creation (`image_source` requires an image, incus.rs:2355-2373), provider-owned `volatile.last_state.idmap` changes (`validate_config`, 2345-2350), or every storage/profile operation. These require explicit maintenance contracts, not invented plugin methods. Raw GPT creation, creation-only blank-leaf addition and host mount ownership remain assigned to the external provisioner. A generic Btrfs method's existence does not cancel that ownership rule.

### F10 — Medium, conditional: chatbot GPT restoration suppresses other identities

Location: `deploy/runit/identity-roots/run:33-38`.

When the chatbot boot receipt exists, the service restores it, publishes global readiness and executes `sleep infinity`. The subsequent physical-identity loop and other legacy handling at 44-112 are unreachable.

Trigger: this GPT chatbot coexists with another completed boot-enabled identity. Effect: the other root is not restored, while the shared identity-roots readiness signal is published. This audit did not establish that a second such identity currently exists.

## Additional limitations, not claimed runtime incidents

- Fresh-only version-4 preparation: `prepare-chatbot-adoption.sh:31-36` accepts only revision 0 / empty operation ID. The owner also supports an exact explicit-deletion tombstone (`identity_clean_provision.rs:294-363`). Preparing a partitioned replacement after supported deletion therefore lacks a matching helper path. Existing-plan refusal is deliberate fail-closed behavior, not itself a bug.
- Retention/recovery: every stopped `boot` performs a new transfer; snapshots and prior writable `@` copies are retained (`sync-instance-upgrade.sh:110-134`). Cleanup is explicitly documented as unfinished (`docs/src/operations/shared-seed-provisioning.md:54-58`). Capacity failure can leave detached views and `send-intent`, which ordinary reruns reject. No exhaustion or data loss was observed. Retention must follow the storage owner's policy; do not invent a global subvolume cap or delete history in this audit.
- `incus-ct-chatbot` is listed as retired; it was not treated as the active boot owner. The demonstrated GPT path is `identity-roots → restore-chatbot-root → sync-instance-upgrade boot`.

## D-Bus and live evidence limits

Mounted discovery was used first. The namespace identifies the configured human session `414fbc56-03fb-20ab-758a-bcc7a7cbdc0e`, with catalog hash `8a34d0da9517b137063314e07f5fa9a24fe3a3fe33920b1de2df941380041fd6`. Mounted method declarations establish available schemas, not successful admission.

The read-only NotebookLM list attempt through the mounted supported client failed at identity registration; no conversation sync was completed. No missing capability was established by that failure, and no credential, alternative identity or privileged bypass was used.

After the D-Bus-first correction, discovery/introspection only was attempted. System-bus lookup of the AGENTS.md name `org.opdbus.v1` returned “The name org.opdbus.v1 was not provided by any .service files”. Source establishes the actual plugin destination as `org.opdbus.v1.plugins` and default private bus `unix:path=/run/opdbus/session-bus.sock` (`threetched-fs/src/dispatch.rs:13-21`, `op-grpc-bridge/src/server.rs:1040-1049`). The private socket exists, but ordinary-user discovery/introspection returned exit 1 without usable diagnostics. No method was called on that alternate transport. Do not conclude that all D-Bus service is absent or that a particular grant is missing from these results.

The mounted operation implementation uses identity-bound D-Bus (`dispatch.rs:265-335,347-389`). A supported mounted `invoke` is therefore compatible with D-Bus-first; direct Incus CLI is not equivalent.

No provisioning helper, mutation operation, test suite, smoke fixture, Cargo command, service action, mount change, grant change or deployment was executed. No secrets or credentials were inspected. Audit checks consisted of source/contract tracing, existing fixture review, bounded read-only path/mount/schema evidence and the unsuccessful read-only discovery described above. No end-to-end pass is claimed.

## Source pins

SHA-256 at the evidence timestamp:

```
efdca1d5ce85f6ffc0939c706e5ebf9715adef6536de063945a1c402890a9718  deploy/scripts/provision-workspace-subscriber.sh
74cf71f0760b384d64f357c8bc12d96a191622f0ea7fae09fd9c0532e8b5955e  deploy/scripts/prepare-chatbot-adoption.sh
c449c4542cc1ecfd9558b21ee60e256bd336e3e9e23e73771b5d8855863852ca  deploy/scripts/prepare-instance-disk.sh
af3a33e5b5908d1665dd3708802c9ecb43eb48b88676bd6b017cea5b98cfc930  deploy/scripts/bind-instance-root.sh
d6bed89aa87b81b9010b434748b87b7814427a56c4e4112314cda224d819dce1  deploy/scripts/sync-instance-upgrade.sh
0967daaf2c14c4fb659a55f71eb232dd8d04b8068da2a9e3c0faa49388c79e3f  deploy/runit/identity-roots/run
42cc5a5f28c193cea7a5ec5e863ff247d46ca611b0f1f415051c17a28843d721  deploy/runit/libexec-3tched/restore-chatbot-root
```

Independent review provenance:
- `/home/jeremy/.hermes/cache/delegation/live/deleg_0c806a1f/task-0.log`
- `/home/jeremy/.hermes/cache/delegation/live/deleg_0c806a1f/task-1.log`

## Handoff

Only this report and the review's HANDOFF/SIGNALS/WISHLIST annotations are authored by this audit. Existing source edits belong to other work and remain untouched. The next implementation should first reconcile mounted invocation and deployed paths, keep group policy out of the rejected plan, then repair the guest mount and upgrade lifecycle under the existing D-Bus/external-storage ownership split. It must not revive retired schemas, bypass a denied identity, enable the held-off kernel guard, or deploy the repository's unrelated dirty work.
