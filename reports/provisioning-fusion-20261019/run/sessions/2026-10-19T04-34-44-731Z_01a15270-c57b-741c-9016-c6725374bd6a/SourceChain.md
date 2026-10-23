## Summary

Completed the assigned source/installed-file investigation: **36 manifest entries accounted for, 33 distinct exported contents read completely**, with three identical-hash installed copies mapped to their source counterparts. No commands, tests, provisioning, storage operations, or edits were performed.

The supplied shell chain is **not an integrated, loop-free provisioning implementation**:

1. **Source and installed helpers disagree on storage location and upgrade FSID.** Even the current source is internally inconsistent: `sync-instance-upgrade.sh` still uses the old pool path and upgrade FSID.
2. **The installed base boot service is a different deployment domain.** Staging application helpers does not update `/etc/sv/identity-roots/run`; that installed service still selects the old, single-chatbot path and exits early.
3. **Every implemented storage branch examined requires host loops.** The newer GPT branch wraps both the seed and the per-instance image; the older sprout branch wraps both physical seed and private leaf. Neither meets the latest constraint.
4. **There are additional deterministic contract mismatches:** the binder accepts one FUSE fstab entry, while image preparation installs two; workspace links target the view that explicitly excludes their plugins; installed binding and current synchronization require `volatile.idmap.current` even though never-started preparation can expose only `.next`.
5. **Preparation is not adoption or startup.** The adoption helper publishes a plan and prints an operation path; it does not invoke adoption. The chatbot preparation branch does not start the container. GPT boot restoration also does not start it.
6. **Interrupted stages fail closed, but do not constitute complete resume support.** Important intents are preserved, yet the shell interfaces mostly require manual reconciliation rather than implementing phase-aware recovery.

These findings establish integration defects and requirement conflicts. They do **not** establish which defect was the last encountered runtime failure, nor prove that no supported no-loop implementation exists.

---

## Evidence notation

All paths below refer to supplied exports beneath:

`reports/provisioning-fusion-20261019/`

- **S/** = `evidence/source/`
- **I/** = `evidence/installed/`
- **H/** = `I/usr/local/libexec/op-dbus/helpers/`
- **B/** = `I/usr/local/libexec/3tched/`

Thus `S/deploy/scripts/bind-instance-root.sh:158-184` is an exact exported-file citation, not a claim about fresh runtime state.

Full hashes and original paths come from `manifest.json.source_and_installed`; I did not recompute hashes. Some exports differ from their original-file hashes because of redaction.

---

## Architecture and stage ownership

The existing chatbot path is:

```text
existing caller-bound 3tchedFS
  → identity_sled.prepare_container
  → local native-preparation receipt
  → prepare-instance-disk: create / prepare
  → sync-instance-upgrade: pair
  → bind-instance-root
  → prepare-chatbot-adoption
  → identity_sled.provision_container [separate operation]
  → authoritative identity/native/storage adoption [Rust owner]
  → authorized start [separate lifecycle operation]

base identity-roots service
  → select completed boot receipt
  → restore-chatbot-root
  → sync-instance-upgrade boot
  → stopped root restored and synchronized
```

A second, older branch remains in `op-identity-root`:

```text
seed-create → seed-up/check → seed-lungs → explicit boot attestation
```

It uses a writable Btrfs sprout root and host-created FUSE mount, rather than the newer read-only upgrade-over-seed overlay and guest-owned FUSE mounts. These are **different contracts**, not interchangeable recovery routes.

### Owner and receipt at every stage

| Stage | Existing owner/interface | Receipt and what it actually establishes | Missing completion boundary / required modification |
|---|---|---|---|
| Admission | Caller-bound `3tched-fs invoke`; backend `identity_sled` | Mount metadata/session and mount-ID pins; same-handle operation result. Wrapper uses existing operator session, not target identity impersonation. | Preserve this path. No alternate identity, local native-create fallback, grants repair, or bootstrap replay. |
| Native preparation | `identity_sled.prepare_container`, invoked by `provision-workspace-subscriber.sh` | `$pool/.provisioning/$sid.prepare.json`: exact caller/request/operation ID; `$name.incus.json`: returned name/native UUID and native session/key/creation-operation readback. Phase `container-defined`, identity not registered, root not bound, not started. | Existing target must be reconciled using its original creation operation/native UUID. Do not allocate another name or UUID. |
| Storage allocation | Host storage helper `prepare-instance-disk create` | `$name.json`: `disk-create-intent` → `disk-partitioned`; image inode/size/GPT table. | Receipt currently names the instance but does not itself carry the full native/session/creation-operation binding. Carry and verify that binding before storage establishment. Replace prohibited backing mechanism only after supported storage decision. |
| Seed/sprout and replica establishment | `prepare-instance-disk prepare` | `filesystem-prepare-intent` → `root-device-add-intent` → `upgrade-format-intent` → `filesystems-prepared`; root/upgrade FSIDs and attachment paths. | Current implementation creates loops and forbids mounted physical seed. Both conflict with retained requirements. Incomplete intent is not proof of an established member or formatted replica. |
| Initial upgrade receive/pair | `sync-instance-upgrade pair` | `$name.upgrade.json`: `send-intent` → `paired`, source/replica FSIDs, direction and generation. Received UUID and immutable baseline checked; published clone ancestry checked. | Resolve old path/FSID drift; retain mounted-destination proof and immutable common baseline; do not infer pairing from an empty upgrade directory. |
| Root binding | `bind-instance-root.sh` | `$name.incus.json`: `root-bind-intent` → `root-bound`; overlay/view paths, native definition, flags including `guest_fuse_mounted:false`. | Fix fstab contract, map selection and complete live mount proofs before writing completion. Root-bound remains neither adopted nor started. |
| Adoption preparation | `prepare-chatbot-adoption.sh`; read-only `inspect_provision_preflight` | Private `/var/lib/op-dbus/identity-provision-plans/$sid.json`, version 4; plan digest and `provision-args.json`. Includes expected revision, native ETag, seed/member/image/GPT pins. | Publishing the plan is not adoption. Preserve exclusive publication, root-private permissions, revision and digest binding. |
| Authoritative adoption | Existing `identity_sled.provision_container`, with Rust proof/CAS owner outside this assignment | Shell prints operation and exact arguments, but supplies no invocation/completion receipt. | Parent must connect the Rust owner’s final result/CAS/readback to this stage. Do not turn local `identity_registered` flags into authority. |
| Startup | Existing identity lifecycle owner; not the standalone-system branch | No chatbot-start completion receipt is produced by the assigned shell chain. | Extend the existing workflow to request authorized start only after authoritative adoption and root proof. Record requested transition, exact UUID and returned/native power state. Do not reuse `--system` creation as a bypass. |
| Stopped boot restoration/sync | Base `identity-roots` → `restore-chatbot-root` → `sync-instance-upgrade boot` | GPT `.boot.json` must attest `identity_registered:true` and `restore_existing_pair:true`; native/storage pins checked; synchronization publishes `paired` only after restoration. | Base service deployment is separate. GPT branch restores but does not start; explicit boot-start policy/owner integration remains separate. |
| Running service restart | `restore-chatbot-root` | Observes overlay lower paths and read-only mode; exits without receive/rotation. | Preserve no live-lower updates. Extend observation to the full established root/member/device contract instead of treating only overlay type/lower strings as complete health. |
| Failure/cancellation/resume | Local locks/intents plus backend operation identity | Native creation can reconcile the same operation; storage/bind/receive retain incomplete intent and refuse blind repeats. | Implement explicit phase-aware reconciliation inside these existing helpers/owner, not a new orchestrator. Resume must prove actual state and perform only the missing authorized transition. |

Principal citations: subscriber `:39-143,145-189,353-375`; disk helper `:68-188`; sync `:78-151,203-255,280-407`; binder `:45-84,199-284`; adoption `:65-171`; source boot service `:76-180`; restoration `:44-109`.

---

## Source-versus-installed drift ledger

### Exact paths and FSIDs

| Contract | Current source export | Installed export | Consequence |
|---|---|---|---|
| Preparation receipt/storage pool | Subscriber, disk, binder, adoption and restoration use `/var/lib/incus/storage-pools/3tched-pool` | Corresponding installed helpers use `/var/lib/3tched-leaf-pool` | Receipts, images and mount paths are not found by the other version. No receipt migration is supplied by these helpers. |
| Pool mount identity | New source requires Btrfs FSROOT `/`, FSID **`a1ab7b42-12a8-437c-8f65-d0469892c8dd`**, distinct from host `/` | Older installed paths require FSROOT **`/@3tched-leaf-pool`**, without equivalent explicit pool-FSID check | Changing only a pathname or copying receipts cannot establish equivalent storage. |
| Synchronization pool | **Still `/var/lib/3tched-leaf-pool`**, FSROOT `/@3tched-leaf-pool` | Identical bytes | Current source itself is not cut over end-to-end. |
| Host upgrade FSID | Binder/adoption require **`7ac6c955-4028-439c-80b4-c55a647abcb4`** | Installed binder/adoption require **`5a88e7cd-9701-40a7-8301-ef076761886c`** | A pair accepted/generated by one contract cannot satisfy the other. |
| Synchronization source FSID | **Still `5a88e7cd-9701-40a7-8301-ef076761886c`** | Identical bytes | Even if pool paths were reconciled, source pair generation contradicts source binder/adoption. |
| Native root pool | `identity-roots`, rootfs `/var/lib/incus/storage-pools/identity-roots/containers/$name/rootfs`; reported source `/var/lib/incus-storage/identity-roots`; sync requires driver `dir` | Same general native-root design | The root is not an Incus-managed Btrfs container volume merely because its externally mounted backing is Btrfs. |
| Physical-seed helper defaults | `RACCOON_UUID=b6561d54-3c69-4897-a2f3-79b867e33601`; default PARTUUID `911b270a-a88f-48cd-bf9c-9dcac4cdefe8` | Same | CASE records current seed FSID `8468cc14-a5e4-409c-96f6-1e157d470ab5`. Defaults are stale relative to that case evidence; environment overrides may change actual selection, which these snapshots do not establish. |
| Restore default name | `3tchedCtlChatbot` | `3tchedChatbot` | Installed base service also hard-codes only the old boot filename. |
| Root-helper executable | Source/application helper defaults to `/usr/local/libexec/op-dbus/bin/3tched-fs` | Base helper defaults to `/usr/local/libexec/op-dbus/bin/threetched-fs` | Base and application helper closures differ. |
| Image-helper dependency | Source/application root helper resolves selected sibling closure | Base root helper defaults to `/usr/local/libexec/3tched/op-identity-image` | Application staging does not prove the base dependency was refreshed. |
| GPT writer | Source uses `sgdisk`; `sfdisk` retained for inspection | Installed disk helper writes GPT with `sfdisk` | Source repair is not installed behavior; neither change removes loops. |
| Seed source resolution | Source disk/adoption accepts a unique physical `part` **or `lvm`** and explicitly verifies backing | Installed adoption and current sync select `part` only | Internal compatibility drift remains. Supporting an existing LV is not authorization to introduce LVM management. |
| Never-started idmap | Source binder accepts equal valid current/next maps, or next alone | Installed binder requires current; current/installed sync also requires current | Source binder repair has not propagated through the installed binder or the stopped synchronization path. |

**Do not equate the source’s `3tched-pool` path with the historical Incus pool named `3tched`.** CASE records that pool’s `/var/lib/incus/disks/3tched.img` → `/dev/loop0` backing. The supplied shell snapshots do not prove these names/paths are the same managed object. Either way, the recorded pool loop violates the latest no-host-loops constraint; it is not exempt.

### Original excerpts establishing the partial cutover

`S/deploy/scripts/sync-instance-upgrade.sh:34-35,77`:

```sh
pool=/var/lib/3tched-leaf-pool
[[ $(findmnt -rn -M "$pool" -o FSROOT) == /@3tched-leaf-pool ]] || fail 'leaf pool not mounted'
```

```sh
host_uuid=5a88e7cd-9701-40a7-8301-ef076761886c
```

`S/deploy/scripts/bind-instance-root.sh:27,79-81`:

```sh
pool=/var/lib/incus/storage-pools/3tched-pool
```

```sh
jq -e --arg replica "$(jq -r .upgrade_fsid "$disk")" '.phase=="paired" and
    .host_fsid=="7ac6c955-4028-439c-80b4-c55a647abcb4" and .replica_fsid==$replica and
    .direction=="host-to-container"' "$pair" >/dev/null || fail 'upgrade pair differs'
```

`S/deploy/scripts/prepare-chatbot-adoption.sh:156`:

```sh
      upgrade_fsid:$d.upgrade_fsid,host_upgrade_fsid:"7ac6c955-4028-439c-80b4-c55a647abcb4"}}
```

These mismatches are directly observable in the source exports; no execution is needed to identify them.

---

## Additional integration defects and retained semantics

### 1. The two-FUSE-view image cannot satisfy the binder’s one-entry validator

Image input, `S/deploy/workspace-image/3tchedfs.fstab:2-3`:

```fstab
3tchedFS /run/mount/3tchedfs fuse.3tchedFS noauto,rw,nosuid,nodev,noexec,noatime 0 0
threetched /run/mount/threetched fuse.3tchedFS noauto,rw,nosuid,nodev,noexec,noatime 0 0
```

Binder, `S/deploy/scripts/bind-instance-root.sh:171-183`:

```awk
            if ($1=="3tchedFS" || $2=="/run/mount/3tchedfs" || $3=="fuse.3tchedFS") {
                count++
                if ($1!="3tchedFS" || $2!="/run/mount/3tchedfs" || $3!="fuse.3tchedFS" ||
                    $5!="0" || $6!="0" || (NF>6 && $7 !~ /^#/)) bad=1
```

```awk
        END { if (bad || count!=1) exit 1 }
```

The second entry matches by filesystem type, increments the count and fails the source/mountpoint test. The `seen` options map also spans entries, so simply changing `count!=1` to `count!=2` is insufficient.

**Concrete change:** update this existing validator to recognize exactly the two supported source/target tuples, validate each tuple exactly once, reset per-entry option tracking, retain the exact six options, and continue rejecting active host-storage entries and unknown FUSE tuples. Apply the same effective-fstab check during stopped synchronization/restoration.

The installed binder has the same one-entry contract.

### 2. Agent links point to the explicitly excluded view

`S/deploy/workspace-image/prepare-lungs-client.sh:66-70`:

```sh
upstream_plugins=threetched,tched_router
"$client" --store "$store" capture --schema-only --catalog "$catalog" --state "$empty" \
    --exclude-plugin "$upstream_plugins"
"$client" --store "$upstream_store" capture --schema-only --catalog "$catalog" --state "$empty" \
    --include-plugin "$upstream_plugins"
```

But `S/deploy/workspace-agent/populate.sh:48-50` creates:

```sh
ln -s /run/mount/3tchedfs/plugins/threetched "$base/@agent_shared/threetched-plugins"
ln -s /run/mount/3tchedfs/plugins/tched_router "$base/@agent_skills/runtime"
ln -s /run/mount/3tchedfs/plugins/threetched "$base/@agent_skills/skills"
```

**Concrete change:** those three upstream-plugin links belong under `/run/mount/threetched/plugins/...`; cognitive and `agent_config` links remain under `/run/mount/3tchedfs`. Update the existing image/workspace documentation accordingly. Do not edit an accepted immutable seed in place to apply this correction.

### 3. Old sprout helper is not a compatible fallback for the canonical guest-FUSE container

The current binder requires the canonical local/expanded device:

```jq
.fuse=={type:"unix-char",source:"/dev/fuse",path:"/dev/fuse"}
```

`S/deploy/scripts/bind-instance-root.sh:120-124`.

The old/current shared-sprout helper instead requires:

```jq
(.expanded_devices | type == "object" and length > 0 and all(.[]; .type == "disk"))
```

`S/deploy/runit/libexec-3tched/op-identity-root:755`.

Its profile/device-key whitelist excludes `fuse`, and `seed-lungs` creates a **host FUSE mount** with `--lungs-session`, then exposes it as an Incus disk at **`/run/mount/3tchedFS`**. See `:700-726,967-1025`.

That differs from the image’s two guest-owned mounts and `BindCurrentContainer` behavior.

**Concrete change:** keep legacy receipt interpretation observational where necessary, but do not route a failed canonical candidate into `seed-lungs` or loosen the device allowlist indiscriminately. The selected existing workflow must use one agreed guest-FUSE/native-device contract.

### 4. Seed permanence and no-host-loops conflict with actual helper behavior

Source disk preparation, `S/deploy/scripts/prepare-instance-disk.sh:102-119`:

```sh
    if findmnt -rn -S "$physical_seed" -o TARGET; then
        fail 'standalone physical seed is mounted; unmount it before preparing its read-only loop view'
```

```sh
    seed_loop=$(losetup --find --show --read-only "$physical_seed")
    disk_loop=$(losetup --find --show --partscan "$image")
```

Older sprout attachment, `S/deploy/runit/libexec-3tched/op-identity-root:1062-1072`:

```sh
        seed_view=$(losetup --find --show --read-only --loop-ref "seed:$native_uuid" "$seed_device")
```

```sh
        leaf_loop=$(losetup --find --show --loop-ref "leaf:$native_uuid" "$leaf")
```

The actual one-time membership operation is:

```sh
    btrfs device add "$leaf_loop" "$seed_dir/volume"
```

`S/deploy/runit/libexec-3tched/op-identity-root:1241`.

The older helper also explicitly rejects directly mounted seed storage:

```sh
seed_fail "directly mounted seed source; unmount it and keep its fstab entry noauto"
```

`:521`.

**Conclusion:** recording mappings in Cozo cannot make these kernel loop mappings disappear. Reverting to the older helper does not satisfy the latest constraints, even though it preserves important genuine sprout semantics.

### 5. Correct root composition must not be simplified away

The newer root is:

```sh
mount -t overlay overlay -o "ro,lowerdir=$upgrade_layer:$seed_layer" "$view"
```

`S/deploy/scripts/bind-instance-root.sh:262`.

There is **no overlay upper/work directory**. The persistent writable leaf is the writable member of the sprout filesystem. Private agent subvolumes and `@agent_user/persist` are separately bound; shared/soul are read-only.

`sync-instance-upgrade.sh:329-407` retains immutable received snapshots, publishes a writable clone as replica `@`, verifies clone ancestry, exposes both OS lowers read-only and restores sibling/persistence mounts.

Preserve these distinctions:

- writable sprout member ≠ OverlayFS upper;
- writable replica filesystem/clone ≠ writable guest OS root;
- received snapshot ≠ published clone;
- immutable shared seed ≠ per-instance root copy.

The sync helper notes that deleting seed files requires whiteouts in the upgrade layer. It transports snapshots; it does **not** generate deletion whiteouts. Source excerpts alone therefore do not establish that the prepared upgrade stream encodes all required deletion semantics.

### 6. Binding proof is weaker than synchronization proof

The binder checks the replica mount is Btrfs FSROOT `/`, but does not compare its live FSID to `.upgrade_fsid` before binding it (`bind-instance-root.sh:77-84`). Its final root check verifies overlay type, not the full source identity, lower order, modes and sibling binds (`:275-280`).

Synchronization has stronger `assert_fs`, `assert_overlay`, `assert_bind`, ancestry and detach checks (`sync-instance-upgrade.sh:136-194,308-326,354-407`).

**Concrete change:** reuse the existing proof requirements in binding and running-restoration observation, including exact live replica FSID and all effective bind identities. Do not weaken the later authoritative Rust proof to match incomplete shell flags.

### 7. Image metadata still describes an older runtime/layout contract

`S/deploy/runit/libexec-3tched/op-identity-image:227-245` declares guest root `rw`, one guest FUSE mount, and host physical-lungs tmpfs. The newer GPT binder exposes a read-only overlay and image preparation installs two guest views.

This metadata may correctly describe the **older** `shared-partition-siblings-v2` contract; it must not be represented as verification of the newer partitioned overlay.

Also, image preparation accepts release IDs up to 128 characters with `+`, while `op-identity-root`’s metadata consumer accepts at most 80 without `+` (`op-identity-image:281-285,333-335`; root helper `:604-607`).

**Concrete change:** version or explicitly distinguish the existing layout metadata/contracts and align producer/consumer release-ID validation. Do not rewrite sealed manifests to conceal drift.

---

## Base boot service versus application staging

### Installed base service

`I/etc/sv/identity-roots/run:32-38`:

```sh
helper=/usr/local/libexec/3tched/op-identity-root
gpt=/var/lib/3tched-leaf-pool/.provisioning/3tchedChatbot.boot.json
if [ -e "$gpt" ] || [ -L "$gpt" ]; then
    /usr/local/libexec/3tched/restore-chatbot-root || exit 1
    /usr/local/libexec/3tched/runit-ready-signal identity-roots
    exec sleep infinity
fi
```

Consequences:

- It does not select `3tchedCtlChatbot.boot.json`.
- It calls base helpers, not the application helper path.
- A matching old GPT receipt bypasses remaining physical-root enumeration.
- Its physical selection permits only `3tchedChatbot` or canonical UUID names, unlike current source.
- The base `restore-chatbot-root`, base image verifier and base readiness helper invoked here are **not included as installed snapshots** in this assigned manifest subset. Their actual installed bytes remain a coverage gap.

### Source service

`S/deploy/runit/identity-roots/run:39-58,76-125` selects an explicit application helper closure, checks dependencies, enumerates GPT receipts, preflights selections before restoration, and rejects duplicate logical-session selections across GPT/physical receipts.

This is a meaningful improvement, but it still:

- hard-codes the new loop-pool contract;
- calls a sync helper with the old pool contract;
- restores GPT storage without a GPT start-policy transition;
- signals readiness through a marker, not guest operation/network proof.

### Why ordinary application staging does not fix base boot

`S/deploy/lib/golden/build-golden.sh:95-99`:

```sh
# RELEASE_ROOTS maps libexec/3tched to /usr/local/libexec/op-dbus/helpers, NOT
# the base-owned /usr/local/libexec/3tched. Guest files are inert image inputs;
# staging them does not install/enable a host or guest service or prepare a seed.
```

Service inclusion requires:

```sh
grep -qx '# op-dbus-release: application' "$SCRIPT_DIR/$1/run"
```

`:123-127`.

The source `identity-roots/run` deliberately lacks that application marker. `enabled-services` inclusion alone does not put it in the application service deployment set.

**Concrete change specification:** preserve this ownership separation. Update the existing base service through its separately authorized base-owner delivery path, with its own installed-file receipt and selected-helper-closure receipt. Do not add an application marker merely to make deployment appear complete. Keep the staged helper inventory explicit, and report preparation, received snapshot, activation and base-service installation separately.

---

## Failure, cancellation and resume specification

Existing useful protections must be retained:

- durable same-operation native preparation intents;
- per-instance locks;
- root-private receipts and exclusive plan publication;
- immutable baseline and received-UUID proof;
- stopped-instance checks;
- persisted send intent before detach;
- no automatic cleanup/reformat/member-add retry;
- no rotation beneath mounted overlay lowers.

Original sync failure contract, `S/deploy/scripts/sync-instance-upgrade.sh:95-105`:

```sh
    if [[ $result != 0 && $transition_started == true ]]; then
        printf 'sync-instance-upgrade: incomplete transition retained at %s; inspect mounts and intent before manual recovery; no automatic retry/rollback.\n' "$state" >&2
    fi
```

That is safe failure handling, **not completed recovery implementation**.

### Concrete changes in existing paths

| Existing target | Required modification |
|---|---|
| `provision-workspace-subscriber.sh` | Make the complete post-bootstrap progression explicit within this existing workflow: reconcile existing native preparation, storage, pair, bind, adoption, requested start and boot opt-in. Preserve original operation IDs and exact native/session binding. Do not treat a successful preparation result as completion. |
| `prepare-instance-disk.sh` | Replace the prohibited image/loop establishment branch only after the supported backing decision. Retain one-time membership establishment and pre-effect intent. Bind storage receipts to existing native UUID/session/creation operation. Add explicit reconciliation of incomplete phases; do not create a new image because the old one is absent but intent remains. |
| Same disk helper | Use the stronger existing write protocol consistently: exclusive staging, file fsync, atomic publication, directory fsync before effects. Current initial `disk-create-intent` is written directly before `qemu-img`; `phase()` does not explicitly fsync the staged file before rename. Classify this as a durability gap requiring repair, not a claimed observed crash. |
| `sync-instance-upgrade.sh` | Align storage/source identities with the selected owner-approved descriptor; do not blindly replace constants with the new loop pool. Preserve and re-establish exact destination mount proof immediately before receive under the storage/lifecycle exclusion. Propagate current/next idmap handling. Add phase-specific recovery for received-but-unpublished, partially rotated and restore-intent states using retained generation and ancestry. |
| `bind-instance-root.sh` | Fix both-view fstab validation; verify live replica FSID and full final root composition; preserve stopped-instance/device/session/UUID checks. Reconcile retained bind intents by observation rather than rerunning edits/mounts blindly. |
| `prepare-chatbot-adoption.sh` | Keep canonical preflight, expected revision, ETag, private plan and digest. Adjust its storage proof schema in lockstep with the Rust owner if backing changes. A no-image implementation cannot continue claiming image inode/GPT/loop proof. |
| Existing adoption/lifecycle owner | Parent should specify final CAS/result and start receipts from the Rust investigation. Start only the already-adopted UUID. Publish boot authorization only after authoritative completion, never merely because the local root is mounted. |
| `restore-chatbot-root` and `identity-roots/run` | Use the selected closure and one storage contract; preflight all selections; preserve running observation and stopped-only receive. Require boot receipt linkage to adopted UUID/revision/storage proof and explicit power policy. Do not introduce lifecycle member allocation or churn. |
| `op-identity-root` | Do not use its old seed branch as an unnoticed fallback. Retain relevant identity/member/immutability protections while removing obsolete active loop paths during an approved cutover. `seed_leaf_image_qemu` is explicitly unused and is not evidence the active `seed_create` stopped using `truncate`. |
| `prepare-lungs-client.sh`, binder validator, `populate.sh`, image metadata/docs | Align the two-view guest contract and link destinations; keep schema-only stores, empty state, guest socket binding and no credential capture. Do not equate configured services with mounted/identity-bound operations. |
| `attach-host-workspace-folders` | Its `observe()` accepts disk-only expanded devices (`:47-62`), conflicting with canonical `/dev/fuse`. If this optional attachment remains part of the selected workflow, validate the exact permitted FUSE device alongside disks rather than removing it or bypassing admission. It is not a prerequisite to native creation. |

### Resume classification that the existing helpers should expose

- **Prepared request, result uncertain:** reconcile the same backend operation ID and native UUID.
- **Storage intent, no proven effect:** retain intent; establish actual device/member/filesystem state before authorizing the missing step.
- **Member-add outcome uncertain:** never issue ADD_DEV again merely from the phase string.
- **Receive completed, publication uncertain:** prove received UUID and expected baseline; resume only the missing publication/restoration.
- **Root bound, adoption uncertain:** query authoritative identity state/revision; do not regenerate a different plan or create a replacement native.
- **Adopted, start uncertain:** native state readback and existing lifecycle result decide completion; no blind repeat.
- **Boot restoration interrupted:** preserve established membership and generation; do not format, allocate or reinterpret another layout.

---

## Native declarations: what is supported here, and what is not established

The existing profile construction is exact and source-backed (`op-identity-root:552-558`):

```yaml
name: 3tched-lungs
description: ""
config: {}
devices:
  runtime-run:
    type: disk
    source: "tmpfs:"
    path: /run
    initial.mode: "0755"
  lungs-socket-dir:
    type: disk
    source: /run/opdbus/lungs
    path: /run/opdbus/lungs
    readonly: "true"
```

The newer binder/restorer expect this existing-container fragment:

```yaml
profiles:
  - 3tched-lungs
config:
  security.privileged: "false"
  security.protection.shift: "true"
  boot.autostart: "false"
devices:
  root:
    type: disk
    pool: identity-roots
    path: /
  fuse:
    type: unix-char
    source: /dev/fuse
    path: /dev/fuse
```

These are **partial contract excerpts**, not a replacement instance configuration. Preserve existing session/key/creation records, native UUID, nonnesting, idmap, seccomp/storage-ioctl restrictions and other owner-required settings. Do not flatten expanded profile configuration into a blind replacement.

The mandatory Incus skill distinguishes:

- native Btrfs container filesystem volumes: subvolumes, without per-volume loops;
- Btrfs sprouting: adding a writable **block member**;
- a subvolume: not such a block member.

**[INFERENCE]** A native root declaration such as `type: disk`, `pool: 3tched`, `path: /` does not by itself reproduce the examined immutable-seed/sprout and separate upgrade-overlay behavior. The examined helpers contain no completed subvolume-only implementation preserving those semantics.

The smallest unresolved decision is therefore **not** “allow the existing pool loop.” It is:

1. identify a supported **non-loop** backing/integration that preserves the retained sprout behavior and immutable mounted seed; or
2. obtain an explicit owner decision changing the seed/sprout semantic requirement before proposing a subvolume/snapshot alternative.

No approved non-loop allocation inventory or end-to-end primitive proof appears in the assigned files. This is a precise evidence gap, not a general impossibility claim. Do not infer new LVM management, VM conversion, copied roots or seed retirement as its answer.

---

## Complete file/read-range coverage

“EOF” means the requested bounded read reached the end of the exported file. All entries below were fully consumed. Hash prefixes identify the corresponding full SHA-256 in `manifest.json`; they are not recomputed hashes.

### Source exports — 26 entries

| Original path under S/ | Read ranges | Hash prefix | Contents |
|---|---|---|---|
| `deploy/scripts/provision-workspace-subscriber.sh` | 1–200; 201–375 | `05c1b52e08a4` | Native admission/preparation; dispatch to storage helpers; separate standalone-system creation/start |
| `deploy/scripts/prepare-instance-disk.sh` | 1–188 | `65cef6512e1a` | Raw GPT allocation, seed attachment, filesystem establishment |
| `deploy/scripts/sync-instance-upgrade.sh` | 1–210; 211–407 | `e71cb7bc470c` | Pair/send/receive, stopped rotation and restoration |
| `deploy/scripts/bind-instance-root.sh` | 1–284 | `37d151a37724` | Native/root proof, guest fstab, idmapped lower composition |
| `deploy/scripts/prepare-chatbot-adoption.sh` | 1–171 | `506d0512b447` | Preflight and version-4 digest-bound plan publication |
| `deploy/scripts/3tchedfs-invoke.sh` | 1–150 → EOF | `83727f83a5fb` | Same-handle mounted invocation |
| `deploy/runit/libexec-3tched/op-identity-root` | 1–240; 241–480; 481–720; 721–960; 961–1200; 1201–1422 | `594401a69b35` | Legacy overlay, sprout creation/restoration, profile, host lungs, boot attestation |
| `deploy/runit/libexec-3tched/op-identity-image` | 1–225; 226–444 | `b57612ff5880` | Seven-member image provenance/layout preparation and verification |
| `deploy/runit/libexec-3tched/restore-chatbot-root` | 1–109 | `0747d1f4e6b4` | GPT boot receipt/native verification and sync dispatch |
| `deploy/runit/libexec-3tched/attach-host-workspace-folders` | 1–90 | `1085bf8224e5` | Admitted read-only `/etc/3tched` attachment |
| `deploy/runit/identity-roots/run` | 1–180 | `fbfaad6dbb4a` | Source base boot selection/restoration |
| `deploy/runit/identity-roots/check` | Complete, 2 lines | `721356501311` | Readiness-marker check |
| `deploy/runit/identity-roots/finish` | Complete, 5 lines | `f5cf87069a4c` | Removes readiness marker; does not detach roots |
| `deploy/lib/golden/build-golden.sh` | 1–250; 251–500; 501–750; 751–1000 → EOF at 943 | `ea9cb0230075` | Application artifact preparation, helper closure, service filter and provenance |
| `deploy/runit/enabled-services` | Complete | `12418e10bc16` | Includes identity-roots; no `jev-router` activation |
| `deploy/runit/retired-services` | Complete | `12492ddf746e` | Retired service ownership, including old container managers |
| `deploy/workspace-image/prepare-lungs-client.sh` | 1–100 | `d15a2cf21bd9` | Offline installation of two guest views |
| `deploy/workspace-image/3tchedfs-mount-preflight` | 1–120 → EOF at 34 | `64d3d4b1ffc6` | Guest socket/FUSE/tmpfs/mountpoint checks |
| `deploy/workspace-image/mount.fuse.3tchedFS` | 1–120 → EOF at 29 | `e2bb947f09b0` | Two supported fstab tuples, foreground FUSE client |
| `deploy/workspace-image/3tchedfs.service` | Complete | `8610a8f99fb9` | Main guest-view service |
| `deploy/workspace-image/threetchedfs.service` | Complete | `5a315c78d031` | Second guest-view service; not a retired alias |
| `deploy/workspace-image/3tchedfs.fstab` | Complete, 3 lines | `88a900719693` | Two guest FUSE entries |
| `deploy/workspace-image/3tched-lungs.conf` | Complete, 3 lines | `402870cc4f12` | Guest socket-entrypoint symlink |
| `deploy/workspace-image/README.md` | 1–220 → EOF | `86528039325c` | Image/guest runtime contract and explicit readiness limits |
| `deploy/workspace-agent/populate.sh` | 1–150 → EOF | `a08243b3f1df` | Agent assets, permissions, links and manifest |
| `deploy/workspace-agent/README.md` | 1–120 → EOF | `ed71d1379b98` | Asset/runtime boundaries and acknowledged unfinished consumers |

### Installed exports — 10 entries

| Original installed path | Read coverage / duplicate mapping | Hash prefix |
|---|---|---|
| `/usr/local/libexec/op-dbus/helpers/provision-workspace-subscriber` | 1–190; 191–400 → EOF at 369 | `fb472e1217a9` |
| `/usr/local/libexec/op-dbus/helpers/prepare-instance-disk` | 1–220 → EOF | `c449c4542cc1` |
| `/usr/local/libexec/op-dbus/helpers/sync-instance-upgrade` | Exact manifest-hash duplicate of source sync, read 1–407 there | `e71cb7bc470c` |
| `/usr/local/libexec/op-dbus/helpers/bind-instance-root` | 1–300 → EOF | `bd72a1f5321a` |
| `/usr/local/libexec/op-dbus/helpers/prepare-chatbot-adoption` | 1–220 → EOF | `6362f6b9627d` |
| `/usr/local/libexec/op-dbus/helpers/restore-chatbot-root` | 1–150 → EOF | `783c26f27e72` |
| `/usr/local/libexec/op-dbus/helpers/op-identity-root` | Exact manifest-hash duplicate of source root helper, read 1–1422 there | `594401a69b35` |
| `/usr/local/libexec/op-dbus/helpers/op-identity-image` | Exact manifest-hash duplicate of source image helper, read 1–444 there | `b57612ff5880` |
| `/etc/sv/identity-roots/run` | 1–200 → EOF | `0967daaf2c14` |
| `/usr/local/libexec/3tched/op-identity-root` | 1–240; 241–480; 481–720; 721–960; 961–1200; 1201–1397 | `d1b16b4f0149` |

### Supporting context consumed

- Complete `CASE.md`, including raw reread of its long scope line.
- Complete `latest-owner-correction.json` and `subvolume-preference.json`.
- Manifest header and complete `source_and_installed` inventory; a filtered inventory read retained all 36 assigned original paths, hashes and byte counts.
- Complete initial-scope and no-host-loops JEV receipts.
- All ten mandatory repository `SKILL.md` files, including continuation ranges for long skills; additionally `using-superpowers` and `systematic-debugging`.

---

## Explicit gaps and handoff boundaries

- **No missing assigned file/range remains.**
- Rust implementations, historical commit blobs, full conversations and report/document corpus are outside this delegated slice. The two deploy READMEs were nevertheless read to remove ambiguity about “all boot/golden/image files.”
- Backend admission repair, definitive native preparation state, final storage proof/CAS and current lifecycle surface must be joined with the parent’s Rust/transcript findings. The shell comments are not substitutes for those implementations.
- The installed base service references additional base helpers whose installed snapshots were not supplied here.
- Redacted exports contain `[REDACTED:opaque-base64]`, including some noncredential-looking golden-build assignments. Exact original unredacted bytes cannot be reconstructed; exported and source hashes must remain distinct.
- No live pool/device inventory, daemon-version/API-extension proof, current environment override, mount namespace state, guest boot, guest FUSE operation or sole-8090 connectivity was exercised.
- No new JEV adjudication was performed by this scout; only the supplied initial judgments were read. Parent should submit these original excerpts and the unresolved storage semantic decision for adjudication.
- Existing identity/session and native UUID **`137c61ff-435c-4300-b3c0-c605e461be13`** must remain preserved as directed by CASE. No bootstrap change or `jev-router` activation is proposed.

**Deliverable status:** assigned snapshot ingestion and diagnostic/modification specification complete; implementation, provisioning, adoption, startup and runtime verification are not claimed.