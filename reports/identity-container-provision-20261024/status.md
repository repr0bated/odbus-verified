# Numbered identity provisioning

Task: `identity-container-provision-20261024`  
Agent: Codex `/root`  
Updated: 2026-10-25T03:17Z  
Commit: `86f03d2dede738e2d780ab789dd5800b58678479`

## Current outcome

**LiteLLM with JEV configuration is now installed in the offline seed.**
Completed exit0 at03:16:40Z; the enabled guest systemd unit skips without failure
when the activation file or TYPESAFE_API_KEY is absent, empty or whitespace-only.
The per-leaf activation directory is empty; no key is included. Installed hashes
and enablement were read back; source review is clear after public-readability
fixes. No boot, API call, host service change or automatic tests were performed.
See `guest-exec-and-litellm.md`, `litellm-review.md` and `litellm-preparation.json`.

The requested canonical Incus exec inspection was denied for session
414fbc56-03fb-20ab-758a-bcc7a7cbdc0e: missing
`cap.service.incus.instance.exec@v1`; nothing executed. No bypass or grant change
was attempted. The current source also lacks the raw block attachment needed
for the owner's guest-side transfer design. This remains separate from the
completed offline LiteLLM installation. NotebookLM recall operation
34bf6f1d-4883-493e-bc4b-ef0b1c7f9044 timed out/outcome unknown; no retry or remote
sync completion is claimed. The detailed report records exact evidence paths.

**The requested seed preparation and OS partition shrink are complete.**
Final verification completed at02:52:24Z, exit0; receipts are
`storage-shrink-completion.json` and `storage-shrink-result.json`.

- Exact40GiB LV is now ext4, UUID5573f463-435c-4a21-8e92-d048524eea44,
  mounted at `/run/mounts/seed-3tched`; its approved fstab entry is persisted.
- The whole sparse40GiB disk is the regular file `seed.img` on that filesystem.
  Its current physical allocation is9318735872B (8.68GiB) after LiteLLM, not40GiB.
- All seven seed subvolumes were transferred by Btrfs send/receive, with source
  and received UUID correspondence recorded. Patch partition2 has only empty
  writable `@`; its full byte hash and geometry are unchanged by the shrink.
- Final p1 filesystem and partition sizes both equal11849957376B (11.04GiB):
  measured Btrfs minimum10776215552B plus1GiB headroom. Filesystem was shrunk
  before p1, preserving its start/UUID/type/name and every p2/GPT field.
- Fresh guest FUSE/client/native/catalog assets and generic workspace defaults
  are staged. Copied machine-specific identifiers were neutralized; private
  identity/config state remains empty. Root/@agent_user stay writable and the
  other five agent members returned to readonly. Seed remains unsealed.
- Root inspection is mounted at `/run/mounts/seed-3tched/rootfs` via loop1p1.
  Patch p2 is unmounted. Original30GiB backup, full40GiB old-LV rollback, staged
  new image, failed partial stage and old public-software retention all remain.

The initial shrink controller stopped only on the final readonly inspection's
unsupported standalone `nologreplay` option. A separately reviewed verification
tail used `ro,rescue=nologreplay`, proved empty writable `@` and unchanged p2
hash, then unmounted it. Resize was not repeated; original failure receipt is
`storage-shrink-initial-completion.json`. No missing check was reported as passed.

Broader provisioning remains unfinished: **no new identity or ID-0 container**,
no guest boot, no filesystem sealing, and no host software deployment. The
original base-archive verification receipt is unavailable; image acceptance,
image-aware mount/provisioning pins, namespace-safe boot send/receive, sealing
authorization and real human approvals remain required. No tests/Clippy/fmt
were requested or run. The following chronological paragraphs describe earlier
states and are superseded by this final seed-preparation outcome.

Current runtime correction supersedes the older progress paragraphs below:
all seven subvolumes were transferred into a new sparse 40GiB whole-disk image,
with an empty writable `@` on partition 2. The second controller stopped at
02:31:03Z because GNU dd rejected `oflag=excl` before creating the full rollback
file. Original LV remains intact, unmounted and temporarily block-readonly;
no ext4 format occurred. The frozen resume-only controller corrects this to
`conv=excl,sparse,fsync` and resumes at verified backup, without replaying staging.
It was independently reviewed and launched at02:37:35Z, detached supervisor30468,
root-private `/var/tmp/identity-container-storage-resume-20261025.6eBmCIsL`.
At02:41:44Z the full40GiB rollback file matched an independent full read of the
block-readonly LV; durable evidence is `storage-rollback-verified.json`.
Final preservation checks precede formatting; launch is not proof of conversion.
Owner has already authorized this takeover.

The outer LV and whole sparse disk file are both 40GiB logical. Initial transfer
preserves 20GiB root plus the remaining patch partition; this supersedes the
failed 8GiB and unexecuted 12GiB proposals. Owner now requests shrinking root
partition 1 after OS contents are finalized: filesystem first, partition second,
preserving partition 2 and the whole disk file. Seed must remain unsealed.
Attempt8 targeted GNU build exited0 at02:36:37Z with all20 source pins unchanged;
isolated71-plugin catalog exited0 at02:37:16Z. Its only change from attempt7 is
threetched:13 native default paths now resolve under `/var/lib/tched-router`,
with explicit sealed relocation provenance;147 methods/capabilities/bindings and
the upstream binary/manifest are unchanged. A root-controlled read-only offline
guest input subvolume is prepared at
`/opt/op-dbus/release-send/identity-container-provision-20261024-attempt8-guest-input`.
Build, catalog, schema-delta and input receipts are in this report directory.
Offline guest refresh source is being reviewed; it has not run.
No new identity or numbered container exists, and no guest boot or test was run.

Latest owner correction: provision the new disk image as **ID_0 / Incus ID-0**,
then convert that identity to the chatbot only at the end. Fresh numbering and
label validation are updated in15 source files, with independent source review.
Fresh histories get a durable zero-origin marker; existing v1 histories keep
their one-based origin and consumed numbers. No counter is reset or inferred
from native names. No chatbot privilege, identity or grant conversion occurred.
Attempt7 targeted GNU build completed successfully at02:05:54Z as jeremy,
including opblob, op-grpc-bridge, 3tched-fs and op-identity-storage. All15 source
pins still matched after the build. Isolated catalog preparation completed
at02:07:00Z with71 plugins. No tests, live catalog update or deployment occurred.
The threetched schema hash changed from attempt6 with the same native pair.
Read-only comparison traced the entire difference to13 native default strings
containing the disposable capture config directory, plus their two schema
digests. All147 methods, capabilities and producer hashes are identical. These
defaults refer to deleted paths, so our capture implementation is being corrected
to relocate only those default values to the declared runtime config directory,
with explicit sealed relocation provenance. ZeroClaw remains unchanged. This
follow-up needs its own targeted build/catalog before guest software staging.
Owner's image location `/run/mounts/seed-3tched/` exists but is empty/unmounted.
Antigravity's current preparation is unfinished: only root `@` was transferred
directly to LV p1, and the proposed standalone `seed.img` is absent. The outer
filesystem must be ext4 per owner. Full evidence is in `seed-inspection.md`.

Owner authorized offline seed inspection and clarified boot ordering: initial
sender/receiver pairing at provisioning, receive into partition 2 every boot,
then overlay that upgrade layer over the seed root. Requested Incus hook must
respect the receiver's intended container mount namespace. The existing host
helper cannot simply be registered as a hook; the source review identified
Incus reentry, user-namespace privilege and rootfs-pinning constraints. No hook
or privilege relaxation was added. Detailed disk inventory, systemd wiring,
storage mismatch and guard readback are in `seed-inspection.md`.

The requested obsolete seed-LV kernel-guard enrollment is absent. Loaded BPF
already excludes ext4 by filesystem magic. Guard mode OFF was observed and
left unchanged. No guard removal/reconfiguration was needed or performed.

Owner expanded the task: **no ZeroClaw vendor patches** and reaffirmed **SID1 is
the main identity**. All four vendor patches and patch tooling have been removed
from source; native source/lockfile/binary provenance replaces patch provenance.
Native config commands remain. Custom patched generation callbacks and credential
stdin injection are removed; our SID1 minting/persistence/mounted admission remain.
The obsolete `generate_spec` route/schema/types/fixtures are retired, and the
renderer helper now carries the admitted seal across asynchronous enrichment.
Source reviews, the targeted GNU build and isolated catalog preparation are
complete; these changes are not deployed. Details and file
ownership: `unmodified-vendor.md`.

Implemented and built; not deployed. No new key, identity, numbered container,
grant or role has been created or changed. Owner subsequently said **go right
ahead** to taking over the unfinished disk preparation, explicitly including
conversion of the exact40GiB seed LV to ext4 after verified preservation.
A guarded detached controller started at02:13:59Z after source review, then
stopped at02:15:41Z with ENOSPC while receiving the root into its8GiB partition.
It had not unmounted or formatted the original LV. Root's apparent8,000,108,374B
plus Btrfs metadata requires more space. A new12GiBbase/~26GiBupgrade attempt
(38GiBimage) is being prepared, with exact-pin reconciliation of the failed
stage and all logs/partial image retained. It will transfer all7 source siblings,
keep the new base writable/unsealed, preserve the original30GiB backup and
verify a separate full40GiB rollback before formatting. The final regular file
will be `/run/mounts/seed-3tched/seed.img`.
No additional owner permission is needed for this approved takeover.

Owner decisions, superseding the earlier pending-target/retirement wording:

- Mint a **separate new identity**; do not attach the existing human identity.
- First target is now `ID_0`, followed by `ID_1`, `ID_2`, etc. Only native Incus
  names use `ID-0`, `ID-1`, `ID-2`, etc.
- UUIDs remain backend ownership identifiers. New allocator histories start at
  0; existing histories preserve their origin and consumed numbers. Neither
  reconstructs its sequence from native names.
- The fixed system chatbot remains outside this sequence. It is not retired.
- The intended system policy is chatbot-only ROOT, human SUPERADMIN, two-human
  authorization and no chatbot execution. This task gates fresh numbered
  identity creation/adoption; it does **not** establish that policy globally
  or change existing grants, roles or the fixed chatbot's legacy workflow.
- Key custody remains unanswered: generate a separate private key file versus
  owner-supplied public key. No key generation has run.

## Source implementation

`deploy/scripts/provision-workspace-subscriber.sh` now accepts
`--prepare-container --public-key BASE64 --operation-id UUID` for a registered,
fresh target key. Each of two distinct human SUPERADMIN callers must invoke the
same request through their own mounted 3tchedFS identity. The first reserves a
number with phase `awaiting_approval`; it does not create a native instance.
The second validated approval receives the sole durable permission to create.
Legacy retries remain reconciliation-only. The system chatbot's reserved
allocation and existing receipt paths are retained.

The existing storage helpers consume `ID_N` receipts and produce a version-5
plan bound to the exact fresh key/session, numbered native instance, native UUID,
creation operation, labels, hostname and disk layout. Two humans approve its
exact SHA256 through `--approve-adoption ID_N`; an approved human then uses
`--adopt ID_N`. The existing identity minting subsystem produces SID1 and commits
identity/storage ownership through the Cozo transaction and SHM projection.
The wrapper retains only public adoption evidence, never SID1/private keys.
The resulting native container remains stopped.

Creation and plan approvals bind actor, verified session, genesis and timestamp.
They require current registered human keys, SUPERADMIN membership and the exact
existing capability. Approval expiry is one hour. A human may explicitly renew
only their own expired consent for the same immutable request/plan; a replay
cannot extend consent or authorize another native POST. Final adoption checks
both current approvals again after storage proof, and Cozo validates the exact
journal/plan binding in the same transaction as identity adoption.

`--cancel-preparation --operation-id UUID` withdraws only an unexecuted proposal
whose creation permission was never granted. A current human SUPERADMIN can
withdraw it even when its original approver's session has expired. Cancellation
records the caller, clears the pending allocation fence once, keeps the number
in history and creates/deletes no native resource. A qualified caller can recover
a lost cancellation receipt without changing its author. Cancelled operations
cannot execute or reuse their number. The wrapper archives terminal preparation
intents under `ID_N.prepare.json` before accepting another operation.

`opblob keygen` now requires an existing private owner-controlled parent, walks
components through held no-follow descriptors, exclusively creates a mode-0600
file before generating/writing key bytes, and fsyncs file and parent. Existing
files cannot be overwritten; failure retains the file as a retry fence. This
source change has not been used to generate a key.

Path work covers shared-seed root/lungs helpers, partitioned disk preparation,
root binding, adoption, upgrades and boot restoration. New receipt versions use
numbered labels; old receipts keep exact ownership and replay rules. The seven
inherited Btrfs member names and immutable physical seed remain unchanged.
Fresh fixed-chatbot GPT/shared-seed preparation keeps its explicit legacy
binding and labels. Temporary guest hostname validation uses a private path
with mount/inode-checked cleanup; Rust verifies a held regular hostname file
before opening/reading it, avoiding FIFO/device blocking.

## Build, review and deployment evidence

- Task source baselines: `/var/tmp/identity-container-provision-20261024/before/`.
  These isolate task changes from the checkout's extensive pre-existing WIP.
- Two targeted `op-identity-storage` GNU release builds passed. The latest is
  `attempt2/storage-build-exit-code` = 0; storage Rust has not changed since.
- First bridge attempt emitted two existing UI dispatch compile errors, then
  was terminated for source corrections: durable receipt exit **143**, not 101.
- Second bridge GNU release build exited **101** at 2026-10-24T23:59:58Z:
  `mutation_engine.rs:3844` lacks `json_render_generation_dispatch::dispatch`;
  `:3897` passes `AdmittedContract` where `PluginSchema` is required. These are
  other work's existing errors; this task has not changed those sites.
- Attempt3 `opblob` GNU release build passed in 3m23s, SHA256
  `387e7ddc2e467de22a67eb6a690be71bf9062334234e97077ea04872ee7ca197`.
  Bridge exited101 at2026-10-25T00:31:13Z with exactly the same two UI errors;
  receipt copied to `build-attempt3.json`. These artifacts predate the owner's
  no-vendor-patches correction and are not the next release inputs.
- The two UI errors are corrected in source as part of actual obsolete-method
  retirement and preserving admitted-schema ownership. Attempt4 unmodified
  upstream build completed successfully at2026-10-25T00:52:33Z in12m27s.
  Native binary SHA256 is
  `454ff4c18822535e782b0f9010d348302d4555709872b9f0e3830239c07ddf65`;
  manifest SHA256 is
  `a08341900e3fc3bb7a1f4f263bbbfe23c2e73a2eced66724bc3359e01e48bfad`.
  Native source is unmodified; output remains in task `unmodified-artifact/`.
- Attempt5 exited127 before compilation because the detached worker lacked
  Cargo on PATH. Its failure receipt is retained. Attempt6 completed the
  targeted opblob/bridge/3tched-fs GNU release build at2026-10-25T01:12:43Z,
  exit0, in16m21s. It ran as jeremy through normal Cargo/JEV with the new native
  capture pair. Receipt and artifact hashes are copied to `build-attempt6.json`
  and `build-attempt6-artifacts.sha256`; no further Cargo work is active.
- The freshly built opblob sealed a new isolated catalog at01:13:02Z, exit0:
  `/var/tmp/identity-container-provision-20261024/attempt6/catalog`.
  Its manifest has71 plugins, generation71, includes `threetched` and omits
  `tched_router`. Manifest SHA256:
  `b437baf2f873b2876775d34db739a052a3bc0cc986ee4063218cc4bb003d4f09`.
  Receipt is `catalog-attempt6.json`. This candidate is not a live catalog or
  a complete deployment; the old seed catalog and legacy generation facade
  remain incompatible with it.
- Capture isolation is independently source-reviewed: schema/help/version use
  one private disposable config/HOME/XDG/temp context; native runtime paths
  stay descriptor-only. Existing tempfile3 moved into production dependencies.
- Offline guest preparation source now includes native artifact/manifest and
  private config binding through existing per-identity persistence. Final
  private-bind review is clear, including fail-closed fstab parsing; helper
  has not run against the owner's seed.
- Prepared threetched capture input SHA256:
  `bbebdcc43c8a3984acb5463e27069cb881fafe47f23d787d62a9d1c5854934d1`;
  manifest `72c43f5066b0d5c06deeb80aa92a8d4f341b1e78cdc59dceaaec7858fe7576d7`.
  This is the obsolete patched pair used by attempts1–3 only. It is retained
  as historical evidence and must not be used for the new release.
- Independent source reviews covered naming/legacy replay, both storage paths,
  fresh mint/adoption authorization, approval expiry, key-file creation and
  cancellation. Final cancellation review found no substantive blocker.
  Review is not execution or end-to-end proof.
- Tests, custom smoke fixtures, formatting checks and Clippy were **not run**,
  per owner instruction. Four pre-existing shell fixtures were updated for
  numbered expectations during source work, without executing them.
- Only the isolated candidate catalog was sealed. No live catalog was changed,
  no snapshot transferred, no deployment/controller action taken, no binaries
  copied live, and no service restarted. An old
  installed script is not evidence that this source workflow is available.

## Earlier read-only runtime evidence

Seed/guard inspection was synced through the caller's mounted NotebookLM
operation, source `2b8ef03a-efbc-4d44-b747-4a45ba6854d4` in Astra notebook
`c5c7f347-77cd-464d-bb02-50f422a74df4`. Write operation
`24c611e6-03b3-493e-98f2-fc021fe3d446` and readback operation
`c426e1d0-45bd-42c8-b57c-272b96eed42a` both completed. Exact local receipts
are retained in the task's `/var/tmp` directory. No alternate identity or
transport was used.

Mounted caller: human session `414fbc56-03fb-20ab-758a-bcc7a7cbdc0e`.
Own-identity operation `89afa3a9-ff4b-4016-8aa5-a409b6f34ea5` showed no instance
or leaf. Configured-chatbot preflight operation
`d4aef49a-6473-4407-bb93-250401bc3552` returned revision 1 and operation
`initialize-configured-chatbot-1cbb00cb4f3b4ece8f95dea366be4e3b`.
Saved public receipt: `chatbot-preflight.json` in this report directory.

Supported host-maintenance readback confirmed stopped `3tchedCtlChatbot`, native
UUID `137c61ff-435c-4300-b3c0-c605e461be13`, chatbot session
`b0c777e3-0955-cd8b-3d01-ca12998cae28`, creation operation
`e3d3ab26-d3a2-48e8-a4a6-1d11e6667770`, profile `3tched-lungs`, root pool
`identity-roots`. A retained 30-GiB image and prepared/root-bound receipts exist;
identity_registered=false and started=false. The expected root mountpoints
were inactive at last read. Receipt existence is not current mount readiness.

The old version-4 SID-addressed plan has SHA256
`ecc0af0be008197296d940f8e601757d112af15526c8bb5dea6fda1e427d99e2` and expects
revision 0/empty operation: it is stale. Do not submit, silently overwrite or
reuse it for the new numbered identity. Do not recreate the fixed chatbot or
repeat the owner's disk work.

## Next action and context routing

Unmodified upstream and attempt8 are built; the matching isolated catalog and
read-only guest inputs are prepared. Disk preparation, offline guest refresh and
filesystem-first p1 shrink are now complete. Do not replay old controllers.
Next reconcile the current prepared geometry with image-aware provisioning and
mount-owner source, truthful image acceptance and the requested namespace-safe
boot transfer before sealing or assembling a deployment. Owner previously held
sealing; the image remains writable. Missing original archive evidence must not
be replaced by a fabricated checksum/signature claim.
The own-stack responder generation hook is an existing unconditional refusal;
its legacy large_language_model facade still targets retired tched_router.
Retiring the old generate_spec route does not establish working UI generation.
A working bridge/sealer and
coherent catalog/release must precede snapshot deployment. After deployment,
settle key custody/registration, obtain both human approvals through their own
namespaces, reconcile the owner's prepared storage against the exact numbered
plan, then adopt and verify canonical receipts/state. Do not bypass absent
operations, denied capabilities, approval requirements or the stale chatbot
plan. No automatic new UUID/key/operation on uncertain completion.

`HANDOFF.md`, `SIGNALS.md` and this report are repository context, owner-governed
and updated by task agents. Mounted NotebookLM is historical recall/sync under
the same caller; present permissions, state and deployment are authoritative at
their owning systems. Astra notebook `c5c7f347-77cd-464d-bb02-50f422a74df4` recall
query `648b7e90-3bc6-4ca8-b252-3f73e7eef89d` terminated `timed_out`; no conclusion
was inferred from that missing recall. Discovery source
`1987a52a-d615-4498-acba-f59b78b0d148` is superseded by the owner decisions above.
ECC CLI/vault are unavailable; do not install alternate memory infrastructure.
