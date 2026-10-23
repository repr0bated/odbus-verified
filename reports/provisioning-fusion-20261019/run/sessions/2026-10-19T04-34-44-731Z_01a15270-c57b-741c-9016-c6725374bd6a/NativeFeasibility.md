## Summary

**Native Incus containers can use a Btrfs sprout. The supplied historical implementation did exactly that by constructing and proving the host filesystem, then binding its prepared root into an Incus `dir`-pool container.** That is different from declaring an ordinary Incus Btrfs root volume and custom filesystem volume.

The evidence supports five conclusions:

1. **Subvolumes-only cannot replace the writable block member of a true Btrfs seed/sprout without changing the contract.** Subvolumes remain useful for native Incus filesystem volumes and upgrade send/receive.
2. **`btrfs device add` does not require a loop device.** A suitable non-loop block device can satisfy that primitive. The supplied receipts do not establish an available, authorized writable device.
3. **The permanently mounted original seed is a separate obstacle on the reported 6.18 kernel.** Upstream documents that, since 6.17, the same seed block device cannot be mounted standalone and through a sprout concurrently.
4. **Supported native YAML exists for the root and additional filesystem volumes, but it does not construct the required upgrade-over-seed root.** Incus 7.0.1 explicitly rejects a `source` on the root disk.
5. **No supplied end-to-end binding satisfies all the latest constraints.** Historical implementations and current proof code require loops; the reported Incus `3tched` pool is itself loop-backed. This is an evidence-based integration gap, not a proof that every possible non-loop implementation is impossible.

No runtime inspection, historical command execution, mutation, check, test, build, deployment, or provisioning was performed.

---

## 1. Controlling constraints and original owner statements

Paths below are relative to:

`/srv/git/odbus-verified/reports/provisioning-fusion-20261019/`

The latest correction is unambiguous:

> “No host loop devices. Do not silently exempt pool-level backing, seed wrappers, per-container images or device mappings described as stored in Cozo.”

It expressly identifies existing `/dev/loop0` backing `/var/lib/incus/disks/3tched.img` as drift, **not an exemption or migration authorization**.  
Citation: `latest-owner-correction.json`.

The latest preference is conditional:

> “Subvolumes only, without disk images, if feasible while retaining the required provisioning semantics.”

It also says:

> “Do not silently substitute copying, snapshots, VM conversion, new LVM management or weakened proofs.”

Citation: `subvolume-preference.json`.

### Kiro transcript: relevant exact original excerpts

Transcript file:

`evidence/kiro/sess_e49a5a84-8491-4c1d-987a-a3b2916a8615.jsonl`

The export preserves original `source_line` identifiers; these are distinct from its 148 physical JSONL lines.

| Original `source_line` | Exact owner excerpt | Consequence |
|---|---|---|
| 109 | “you are wrong about the nativ incus cannot be a spout we had it working before” | Do not equate native Incus with an inability to run a sprout. |
| 141 | “the trick to doing the rw is the cozo loop/perrsistce is added to the seed with btrfs add device that is how we qachieve rwq  on sprout” | The retained write-isolation mechanism is Btrfs device membership, not merely an OverlayFS upper directory. |
| 178 | “there is no retiring of the orignal seed mout . it is markeed with btrfstun -S 1 that makes it  a see. it ajnnot be kmodified after of all leafs becime invaliddatedd, hence the overlay for upgrades” | Preserve the original immutable seed and dependent sprouts. |
| 180 | “the seed is mounted at /run/mount/3tched-seed a[permanently” | Permanent standalone mounting must be reconciled explicitly with the kernel restriction. |
| 201 | “the loop is eswsentially wstored inside the dvbb  as a device so no loops o n the host” | A database association is not proof that a kernel loop mapping disappears. |
| 202 | “for btrfs device addk, it does not need to be a loop devvice” | Correct as a statement about the Btrfs primitive. |

**Transcript interpretation:** the supplied export contains investigation/tool activity and interruptions, not a completed public architecture specification. Its only substantive public assistant opening was “Mandatory skill preload first.” I found no completed Kiro proposal to adopt as an authoritative solution. The attempted write was cancelled before execution. Parent should not attribute a finished design to this transcript.

---

## 2. What the supplied receipts establish

These are **reported observations in supplied exports**, not fresh runtime findings.

### Native container and pool state

Kiro original `source_line` 85 and 88 report:

- Incus client/server **7.0.1**.
- Pool `3tched`: driver `btrfs`, source `/var/lib/incus/disks/3tched.img`, size `150GiB`.
- Pool `identity-roots`: driver `dir`, source `/var/lib/incus-storage/identity-roots`.
- `3tchedChatbot`: stopped native container, profile `3tched-lungs`.
- Its instance-local root uses **`identity-roots`**, not `3tched`.
- The default profile’s `3tched` root does not override that instance-local root.
- No custom volumes were listed in the reported `3tched` pool.

Identity pins in that native receipt:

| Pin | Reported value |
|---|---|
| Session | `b0c777e3-0955-cd8b-3d01-ca12998cae28` |
| Native UUID | `137c61ff-435c-4300-b3c0-c605e461be13` |
| Creation operation | `e3d3ab26-d3a2-48e8-a4a6-1d11e6667770` |
| Creation digest | `d8a22c79d619a3fc62b64a5a2b998ca2ddca25e2c69cd092f08f015441a8f9a7` |

These are preservation requirements, not values to regenerate.

Kiro original `source_line` 92 reports incomplete storage preparation:

- Native preparation: `container-defined`.
- `identity_registered:false`, `root_bound:false`, `started:false` in that preparation receipt.
- Disk phase: `filesystem-prepare-intent`.
- `filesystem_prepared:false`.
- Referenced old raw image absent; root/upgrade directories and native rootfs empty.

Those preparation flags must not be generalized into “bootstrap does not exist.” Bootstrap is explicitly outside this assignment.

### Seed, upgrade, kernel, and loop evidence

Kiro original `source_line` 96 reports:

- Kernel: `6.18.57-x64v3-op-release-lifetime1-xanmod1`.
- Existing loop: `/dev/loop0` → `/var/lib/incus/disks/3tched.img`.
- Seed: `/dev/mapper/3tched--vg-3tched--seed`, UUID  
  `8468cc14-a5e4-409c-96f6-1e157d470ab5`, block read-only, mounted at `/run/mount/3tched-seed`.
- Upgrade: existing LV, UUID  
  `7ac6c955-4028-439c-80b4-c55a647abcb4`, mounted at `/run/mount/3tched-ct-upgrade`.

Original `source_line` 184 reports seed generation **72**, seeding flags, and the seven inherited sibling subvolumes.

**Not established:** a spare, authorized writable partition/device suitable for identity allocation. Blank or incomplete filesystem output for another partition is not allocation permission.

---

## 3. Native Incus declarations: supported fragments and their limits

These are **configuration fragments, not commands, complete replacement configurations, or authorization to apply them**. Preserve all existing identity/security/profile/device fields not shown.

### A. Existing native root: supported and already reported

```yaml
devices:
  root:
    type: disk
    pool: identity-roots
    path: /
```

This is the reported native declaration.

**What it establishes:** Incus owns a container root volume in its `identity-roots` pool.

**What it does not establish:** that the root currently contains the prepared sprout/overlay, that storage proofs pass, or that authoritative admission/start completed.

### B. Ordinary Incus Btrfs root: supported but conditional

```yaml
devices:
  root:
    type: disk
    pool: 3tched
    path: /
```

**Conditions and limitations:**

- `3tched` must first have compliant, explicitly authorized, non-loop backing.
- This is not a valid in-place pool-switch recipe for the existing container.
- Incus 7.0.1 says:

> “The storage pool of the root disk can only be changed through move”

- Any migration must separately preserve the native UUID, identity binding, creation provenance, storage receipts, and lifecycle guarantees.
- An ordinary Btrfs root volume is a subvolume/snapshot-managed root, **not automatically a true sprout or the required two-lower overlay**.

Sources: [Incus 7.0.1 disk validation/update implementation](https://github.com/lxc/incus/blob/v7.0.1/internal/server/device/disk.go#L1797-L1818), [7.0.1 Btrfs driver documentation](https://raw.githubusercontent.com/lxc/incus/v7.0.1/doc/reference/storage_btrfs.md).

### C. Additional custom filesystem volume: supported but conditional

The following uses an **illustrative volume name and guest path**, not an observed existing volume:

```yaml
devices:
  upgrade-replica:
    type: disk
    pool: 3tched
    source: identity-upgrade
    path: /upgrade
    readonly: "true"
    required: "true"
```

Conditions:

- `identity-upgrade` must exist as an Incus **custom volume with `filesystem` content**.
- Pool backing must satisfy no-host-loops.
- Ownership/idmapping must be compatible with the existing unprivileged container.
- Send/receive, publication, ancestry, recovery, and pairing proofs remain separate obligations.

**This merely exposes a filesystem at `/upgrade`. It does not place that filesystem above the seed in `/`.** Its `readonly` mount option also does not by itself make a Btrfs snapshot suitable as an incremental-send baseline.

The version-matched documentation states:

> “Custom storage volumes of … content type … `block` or `iso` cannot be attached to containers, but only to virtual machines.”

Source: [Incus 7.0.1 storage volumes](https://raw.githubusercontent.com/lxc/incus/v7.0.1/doc/howto/storage_volumes.md).

Thus the presence of a `custom_block_volumes` API extension in a receipt is **not** proof that a custom block volume can be attached to this container.

### D. Arbitrary host root `source`: explicitly unsupported

Do **not** present this as valid:

```yaml
devices:
  root:
    type: disk
    source: /some/prepared/root-view
    path: /
```

Incus 7.0.1 validation contains the exact errors:

> `Root disk entry may not have a "source" property set`

> `Root disk entry must have a "pool" property set`

Source: [disk.go:513–519](https://github.com/lxc/incus/blob/v7.0.1/internal/server/device/disk.go#L513-L519).

Likewise, a custom-volume `source` cannot simply be assigned to `path: /`.

### Pool backing without a loop is supported independently

Version-matched Incus configuration documents pool `source` as:

> “Path to an existing block device, loop file or Btrfs subvolume”

Source: [Incus 7.0.1 config_options.txt:5761–5767](https://github.com/lxc/incus/blob/v7.0.1/doc/config_options.txt#L5761-L5767).

Therefore an authorized existing Btrfs subvolume on non-loop storage can support the **pool layer** without introducing new LVM or a VM. Merely changing a source string does not migrate the existing pool, and this capability does not solve the sprout writable-member requirement.

---

## 4. Why the seed contract needs block storage

Upstream describes a seeding device as:

> “extending a read-only filesystem on a device with another device that captures all writes.”

It further states:

> “Mounting such device will not allow any writes, except adding a new device by **btrfs device add**.”

Source: [Btrfs seeding-device documentation](https://btrfs.readthedocs.io/en/latest/Seeding-device.html).

The device documentation identifies Btrfs storage as one or more **block devices**. The subvolume documentation distinguishes subvolumes from block-level logical volumes:

> “A subvolume in BTRFS is not like an LVM logical volume, which is block-level snapshot while BTRFS subvolumes are file extent-based.”

Sources: [btrfs-device](https://btrfs.readthedocs.io/en/latest/btrfs-device.html), [btrfs-subvolume](https://btrfs.readthedocs.io/en/latest/btrfs-subvolume.html).

Consequences:

- A subvolume path is not the writable block member passed to `btrfs device add`.
- A writable snapshot of a read-only subvolume can isolate file changes, but **substituting it for a sprout changes the specified mechanism and proof model**.
- Keeping a backing file inside a subvolume still requires some block-device presentation for that file. Recording it in Cozo does not provide that presentation.
- A real non-loop block device can be added; a loop is an implementation choice, not a Btrfs requirement.

### Upgrade replica is a different issue

Btrfs send/receive operates on subvolumes and snapshots. There is no general Btrfs requirement that an upgrade replica occupy a second partition.

Upstream says:

> “All snapshots involved in one send command must be read-only…”

and:

> “Read-only mount of the subvolume is not sufficient…”

The receive documentation also requires protecting the receive destination against concurrent modification until reception completes.

Sources: [btrfs-send](https://btrfs.readthedocs.io/en/latest/btrfs-send.html), [btrfs-receive](https://btrfs.readthedocs.io/en/latest/btrfs-receive.html).

**However, the supplied implementation currently requires more than that general primitive:** its V4 proof requires a separate upgrade filesystem, top-level mount, a single-device filesystem, GPT geometry, and a loop-backed second partition. Replacing that with a custom filesystem subvolume requires an explicit proof/schema migration, not a path substitution.

Citation: `evidence/source/crates/op-grpc-bridge/src/identity_clean_provision/partitioned.rs:109-138`.

---

## 5. Kernel 6.17+ and the permanent seed mount

The upstream caveat is exact:

> “Before v6.17 kernel, a seed device could have been mounted independently along with sprouted filesystems. But since 6.17 kernel, a seed device can only be mounted either through a sprouted filesystem, or the seed device itself, not both at the same time.”

Its reason:

> “This is to ensure a block device to have only a single filesystem bound to it, so that runtime device missing events can be properly handled.”

Source: [Btrfs seeding-device documentation](https://btrfs.readthedocs.io/en/latest/Seeding-device.html).

This applies directly to the **same-block-device** interpretation of the supplied 6.18 receipt and permanent standalone mount requirement.

It does **not** authorize:

- Unmounting the permanent seed.
- Clearing its seeding flag.
- Removing the seed from sprouts.
- Rebuilding/copying the seed.
- Downgrading or modifying the kernel.
- Assuming that aliases preserve supported sharing semantics.

The documentation itself warns that clearing the seeding flag and changing the seed invalidates existing dependent filesystems.

The contemporary maintainer discussion also explicitly discusses only one sprout mount at a time on the same device, but is a discussion rather than proof of every alias topology.  
Source: [August 2025 upstream filesystem discussion](https://www.spinics.net/lists/linux-fsdevel/msg309379.html).

**Do not combine the generic multi-sprout example with the version caveat to claim unrestricted concurrent same-device mounting on 6.18. Conversely, do not turn the caveat into an unsupported impossibility claim about every distinct block-view implementation.**

---

## 6. What actually worked historically—and what differs now

### October 5: native Incus plus external sprout construction

Historical `op-identity-root` creates distinct read-only seed loops and writable leaf loops:

```sh
seed_view=$(losetup --find --show --read-only --loop-ref "seed:$native_uuid" "$seed_device")
```

```sh
leaf_loop=$(losetup --find --show --loop-ref "leaf:$native_uuid" "$leaf")
```

It then performs:

```sh
btrfs device add "$leaf_loop" "$seed_dir/volume"
```

and binds the prepared filesystem root:

```sh
mount --bind "$guest" "$native_root"
```

Citations:

- `evidence/history/oct05-admission-helper/deploy/runit/libexec-3tched/op-identity-root:1003-1024`
- Same file `:1104-1124`
- Same file `:1147-1198`

It explicitly rejects a standalone physical-source mount:

> `directly mounted seed source; unmount it and keep its fstab entry noauto`

Citation: same file `:494-505`.

**Conclusion:** strong evidence that native Incus and sprouts were integrated, but this historical implementation violates the latest no-host-loops constraint and conflicts with the standalone permanent-mount requirement.

### October 11: upgrade-over-seed read-only root

The historical binder contains:

```sh
mount --bind -o ro,X-mount.idmap=b:0:1000000:1000000000 "$root/@" "$seed_layer"
mount --bind -o ro,X-mount.idmap=b:0:1000000:1000000000 "$replica/@" "$upgrade_layer"
mount -t overlay overlay -o "ro,lowerdir=$upgrade_layer:$seed_layer" "$view"
mount --bind "$view" "$container_root"
```

Citation: `evidence/history/oct11-start/deploy/scripts/bind-instance-root.sh:82-85`.

Important distinctions:

- Upgrade is the higher-priority **lower**.
- The other lower is the sprout’s inherited `@`.
- Root is read-only.
- There is no writable OverlayFS `upperdir`/`workdir`.
- Persistent identity writes live in separately bound inherited siblings and `@agent_user/persist`.

The supplied historical handoff states:

> “Canonical identity_sled.touch_session completed: 3tchedChatbot Running, PID11719.”

It also explicitly limits the persistence claim:

> “Boot wiring is deployed; actual reboot/cold-sync path unexercised…”

Citation: `evidence/context/HANDOFF.md:5940-5962`.

This is a historical handoff report. The separately named original runtime receipt files were not supplied as independent evidence inspected here.

### Current source, installed files, and history are not interchangeable

Concrete drift:

| Evidence layer | Observed binding |
|---|---|
| Current source prepare/bind | `/var/lib/incus/storage-pools/3tched-pool`; new storage pins |
| Current source bind | Host upgrade UUID `7ac6c955-4028-439c-80b4-c55a647abcb4` |
| Current source sync | Still `/var/lib/3tched-leaf-pool`; host upgrade UUID `5a88e7cd-9701-40a7-8301-ef076761886c` |
| Installed prepare helper | Old leaf-pool image/GPT/loop implementation |
| Installed `identity-roots/run` | GPT chatbot early-return branch |
| Current source `identity-roots/run` | Different closure/preflight implementation |

Citations:

- `evidence/source/deploy/scripts/bind-instance-root.sh:27-30,79-81`
- `evidence/source/deploy/scripts/sync-instance-upgrade.sh:34-80`
- `evidence/installed/usr/local/libexec/op-dbus/helpers/prepare-instance-disk:40-132`
- `evidence/installed/etc/sv/identity-roots/run:33-39`

The receipts do not prove that the installed bridge executable matches the inspected source. A transcript binary lookup used a missing path.

---

## 7. No-loop alternatives: viability matrix

| Alternative | No host loops / no new LVM / no VM | Preserves true seed/sprout? | Status under all requirements |
|---|---|---|---|
| Incus Btrfs root plus custom filesystem subvolumes | Potentially, on authorized non-loop pool backing | **Not by itself** | Supported native storage shape; not an end-to-end replacement |
| Existing authorized physical block partition/device as writable sprout member | Yes | Yes, at the Btrfs primitive level | No authorized allocation established; same-device permanent seed concurrency still unresolved |
| Existing already-created non-loop block allocation | Potentially; must not imply new LVM management | Potentially | Supplied receipts do not establish an available allocation or safe ownership |
| Kernel `dm-linear` block views | Does not require loop devices or an LVM daemon | Potentially, if the resulting block topology is supported and proven | **Research candidate only**, not an evidenced solution |
| File-backed block presentation other than loop | Might avoid the loop driver but retains images and introduces other mechanisms | Potentially | Not supplied, authorized, or proven; not selected |
| Copying seed data or substituting ordinary snapshots | Can avoid loops | **No: changes retained contract** | Rejected as silent substitution |
| VM or new LVM provisioning layer | Outside constraints | Irrelevant | Rejected |
| Existing loop pool plus “loops recorded in Cozo” | **No** | Historical implementation may sprout | Rejected by latest correction |

### `dm-linear`: exact supported primitive, explicit gap

Official kernel documentation says:

> “Device-Mapper's ‘linear’ target maps a linear range of the Device-Mapper device onto a linear range of another device.”

Source: [Linux v6.18 dm-linear documentation](https://raw.githubusercontent.com/torvalds/linux/v6.18/Documentation/admin-guide/device-mapper/linear.rst).

This establishes a non-loop block-view primitive without requiring new LVM management. It **does not establish**:

- Safe concurrent standalone seed and sprout mounts over aliases of the same bytes.
- Btrfs scan/FSID behavior for those aliases.
- Safe holder/event propagation.
- Writable allocation ownership.
- Compatibility with this kernel’s configuration.
- Existing admission/boot/recovery support.

Accordingly, presenting `dm-linear` as a completed workaround would exceed the evidence.

---

## 8. Why configuration alone cannot complete the cutover

The proof code intentionally recognizes the old storage shape.

Exact current-source requirements include:

```rust
self.seed_device.starts_with("/dev/")
    && !self.binding.device_path.starts_with("/dev/")
```

Citation: `evidence/source/crates/op-grpc-bridge/src/identity_clean_provision.rs:167-174`.

```rust
ensure!(leaf_meta.is_file() ...
```

Citation: `.../identity_clean_provision/storage.rs:269-270`.

```rust
let loop_number = loop_name.strip_prefix("loop")
    .context("partition parent is not loop storage")?;
```

Citation: `.../identity_clean_provision/partitioned.rs:54-56`.

```rust
"member is not a pinned loop mapping"
```

Citation: `.../identity_clean_provision/storage.rs:352-355`.

It also pins the native root pool to `identity-roots` and verifies that pool’s driver is `dir`.  
Citations: `identity_clean_provision.rs:191-205,385-434`.

Any eventual non-loop implementation therefore needs a coordinated change to:

- Allocation/device identity and held-descriptor proofs.
- Seed immutability and actual seed/writable-member ancestry.
- Seven inherited sibling identities and mount protections.
- Upgrade receipt identity, receive ancestry, and publication.
- Native UUID/security/idmapping binding.
- Intent retention and cold restoration.
- Admission rechecks and final revision/CAS publication.

The existing authoritative path rechecks admission and native/storage proof before committing through `commit_identity_intent` / `apply_identity_intent`. Preserve that ordering and authority; do not replace it with a shell success flag.

Citation: `identity_clean_provision.rs:532-679`.

This path also contains identity-genesis creation behavior. **It must not be replayed merely to repair storage for the already-bootstrapped identity.** The assignment requires preservation, not reprovisioning.

---

## 9. Smallest unresolved owner decision

The first topology decision is:

> **Does “permanently mounted seed” require a standalone filesystem mount on the exact same seed block device while dependent sprouts are mounted, or may the permanent availability requirement be satisfied through an explicitly approved, proven alternative block-view/mount topology that leaves the original seed bytes and dependent identities unchanged?**

Why this is the smallest meaningful question:

- No-host-loops is already settled.
- True seed/sprout semantics are already settled.
- Subvolumes-only is a preference, expressly subordinate to those semantics.
- A YAML preference cannot resolve the kernel’s same-device restriction.
- “Use a real block device” solves the writable-member primitive, **not** simultaneous permanent standalone mounting.

If the exact same-device standalone mount is non-negotiable, the supplied kernel/documentation establishes a conflict with concurrent direct sprout mounting. No approved alternative in the supplied evidence resolves it.

If an alternative topology is acceptable, the next concrete prerequisite is **an identified, authorized non-loop writable allocation**, plus evidence supporting the seed-view topology and preservation proofs. There is no basis here to nominate an apparently unused partition or create new LVM.

**Neither answer authorizes this diagnostic run to mutate storage.**

---

## Architecture

```text
Immutable seed block device
        +
isolated writable block member
        |
        v
Btrfs sprout with inherited @ and six @agent_* siblings
        |
        +---- inherited @ ----------------------+
                                               |
Host upgrade producer -- send/receive --> replica @
                                               |
                         read-only overlay:
                         upgrade @ : inherited @
                                               |
                         native Incus rootfs binding
                                               |
                    separate sibling/persistence binds
                                               |
                  storage/native/admission proof + CAS
                                               |
                     existing authoritative lifecycle
```

A managed Incus filesystem-volume declaration addresses only part of this graph. It neither creates Btrfs seed membership nor proves the completed identity binding.

---

## Files and read coverage

### Task and transcript coverage

| Material | Coverage |
|---|---|
| `CASE.md` | Entire file; long constraint line 18 reread raw |
| `latest-owner-correction.json` | Entire file |
| `subvolume-preference.json` | Entire file |
| `jev-initial-decision.json`, `jev-no-host-loops.json` | Entire files |
| `manifest.json` | Lines 1–1256 |
| Kiro JSONL export | **All physical lines 1–148**, including raw recovery of truncated tool output |
| Mandatory skills | All ten required repository `SKILL.md` files read fully; also `using-superpowers` and relevant `systematic-debugging` |

Kiro raw-read coverage was:

`1-3, 4-14, 15-20, 21-28, 29-32, 33-35, 36-40, 41-43, 44-48, 49-53, 54-60, 61-70, 71-80, 81-90, 91-100, 101-112, 113-122, 123-134, 135-148`.

### Relevant implementation coverage

“Transcript-full” means the complete file content was read in a supplied Kiro tool-result record; direct snapshot ranges below provide additional anchors.

| File under task root | Coverage / contents |
|---|---|
| `evidence/source/deploy/scripts/prepare-instance-disk.sh` | Direct 1–188; image/GPT preparation, loops, seed membership |
| `evidence/source/deploy/scripts/bind-instance-root.sh` | Direct 1–284; identity pins, mapped lowers, root/sibling binds |
| `evidence/source/deploy/scripts/sync-instance-upgrade.sh` | Transcript-full 1–407; direct 34–125, 376–407; targeted grep of root restoration/pins |
| `evidence/source/deploy/scripts/prepare-chatbot-adoption.sh` | Transcript-full 1–171; protected adoption plan |
| `evidence/source/deploy/scripts/restore-chatbot-root.sh` | Transcript-full 1–109; boot restoration ownership |
| `evidence/source/deploy/scripts/provision-workspace-subscriber.sh` | Transcript-full 1–375; dispatch and external preparation boundaries |
| `evidence/source/deploy/runit/identity-roots/run` | Transcript-full 1–180; current boot closure |
| `evidence/source/crates/op-grpc-bridge/src/identity_clean_provision.rs` | Transcript-full 1–704; direct 100–175, 190–205, 360–704 |
| `evidence/source/crates/op-grpc-bridge/src/identity_clean_provision/storage.rs` | Transcript-full 1–369; direct 255–280, 315–369 |
| `evidence/source/crates/op-grpc-bridge/src/identity_clean_provision/partitioned.rs` |