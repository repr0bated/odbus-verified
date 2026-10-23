## Summary

**Completed the historical-blob review:** all **65 available references**, deduplicated into **29 distinct exported SHA-256 blobs**, were read completely. All **72 manifest references** are accounted for below, including **7 unavailable commit/path combinations**. I also read `evidence/research/ProvisionScriptHistory.md` completely, treating it as secondary research—not as proof of historical source contents or current runtime state.

Main conclusions:

1. **Oct5 was not demonstrated end-to-end scripted success.** Contemporary notes distinguish a running, manually assembled sprout from the provisioner’s different, incomplete leaf/receipt.
2. **Oct11 has stronger historical outcome evidence:** notes report canonical start and successful guest-bound identity retrieval. The subsequent boot release was deployed, but its **actual reboot/cold-sync path was explicitly unexercised**.
3. **The historical storage implementation genuinely uses host loops.** This is true of the older writable-overlay path, Oct5 physical-seed/sprout path, Oct11 GPT path, and pre-Hermes retained implementations. “Physical seed,” “subvolumes,” and “Incus pool” do not make these paths loop-free.
4. **The writable sprout is not an OverlayFS upper.** Oct11’s root is a **read-only overlay of upgrade and seed-root lowers**; persistent identity writes live in the sprout’s inherited agent siblings and ordinary `persist` directory.
5. **Pre-Hermes is not a native-subvolume storage cutover.** The GPT scripts and Rust storage proofs remain byte-identical to Oct11-start. Some legacy FUSE ownership/image-layout changes occurred, but the seed-* implementation still retains host FUSE and seed/leaf loops.
6. **These historical workflows cannot be replayed under today’s constraints.** They create native instances/leaf images and require loops; their historical fresh-adoption authorizations do not authorize replacing today’s existing identity/native UUID.

No commands from the scripts were executed. No checks, tests, builds, provisioning, writes, or runtime/storage operations were performed.

---

## Citation convention

Root of the supplied evidence:

`/srv/git/odbus-verified/reports/provisioning-fusion-20261019/`

The following aliases identify **exact immutable commits**:

| Alias | Manifest label | Exact commit |
|---|---|---|
| **A** | `oct05-admission-helper` | `26a4ad132fbbd4523b05724060bfdeb9ca661824` |
| **B** | `oct11-start` | `fc8675a9f18ae0b59fc90f402d8e9bd5f15906e6` |
| **C** | `oct11-boot` | `96a23f1596670d2f578b6d578217a05738f46cd0` |
| **D** | `pre-hermes-head` | `051a20d056afba074e21d797aa34f8947c543c5a` |

For example, **B:`deploy/scripts/prepare-instance-disk.sh:94-97`** means that exact commit/path/line range, supplied at:

`evidence/history/oct11-start/deploy/scripts/prepare-instance-disk.sh:94-97`

Context/HANDOFF and working-tree report citations below are **separately identified exports**, not immutable blobs at A/B/C/D.

---

## Architecture and historical differences

### 1. Three different storage paths must not be conflated

| Path | Actual storage/root mechanism | Evidence |
|---|---|---|
| Older `op-identity-root create/up` | Standalone `identity.img`, formatted Btrfs, attached through a loop; writable OverlayFS `upper`/`work` over the shared Raccoon lower | A:`deploy/runit/libexec-3tched/op-identity-root:190-217,289-343` |
| Oct5 `seed-*` | Physical SEEDING partition exposed through a separate RO loop per identity; sparse unpartitioned leaf file through RW loop; creation-only Btrfs device-add makes a sprout inheriting seven siblings | A:same path`:1003-1025,1155-1222` |
| Oct11 GPT stages | Sparse raw GPT image attached with loop partition scanning; p1 added to seed as writable sprout member; p2 separately formatted Btrfs for upgrade replica; RO two-lower overlay plus sibling/persist binds | B:`deploy/scripts/prepare-instance-disk.sh:94-129,148-168`; B:`deploy/scripts/bind-instance-root.sh:82-98` |

The older writable-overlay path really does use an upper directory:

```sh
mount -t overlay overlay \
    -o "lowerdir=$SEEDS_IDMAP,upperdir=$dir/upper,workdir=$dir/work" "$root"
```

**Citation:** A:`deploy/runit/libexec-3tched/op-identity-root:210-217`.

Its creation path formats `identity.img`, creates `upper`, `work`, and `cozo`, archives the earlier instance, and initializes an empty replacement. That is neither Oct5’s physical-seed sprout creation nor Oct11’s read-only OS composition, and it does not preserve an existing native UUID through recreation.

### 2. Oct5: true shared physical seed, but loop-backed private sprout

The source explicitly allocates distinct host loop mappings:

```sh
# Deliberately do NOT use --nooverlap: each identity owns a distinct
# read-only block-device view of the SAME partition, with no copies.
seed_view=$(losetup --find --show --read-only --loop-ref "seed:$native_uuid" "$seed_device")
```

```sh
leaf_loop=$(losetup --find --show --loop-ref "leaf:$native_uuid" "$leaf")
```

**Citation:** A:`deploy/runit/libexec-3tched/op-identity-root:1003-1025`.

Creation then uses:

```sh
truncate -s "$leaf_size" "$leaf"
```

and, after pinning the inherited layout and rechecking physical-seed properties:

```sh
seed_write '.phase="device-add-intent"'
btrfs device add "$leaf_loop" "$seed_dir/volume"
```

**Citation:** A:same path`:1155-1198`.

Important semantics:

- The code deliberately does **not** copy or snapshot the root during sprout creation.
- The physical seed remains SEEDING and block-read-only.
- The writable leaf is a **block member**, not an OverlayFS upper.
- The inherited layout is exactly `@`, `@agent_memory`, `@agent_rules`, `@agent_shared`, `@agent_skills`, `@agent_soul`, `@agent_user`.
- `@agent_user/persist` is an ordinary protected directory, not an eighth subvolume.
- Oct5 protects rules/shared/soul read-only; Oct11 changes rules to writable.
- `seed-up` restores mappings/mounts; `seed-down` detaches owned mappings. Neither is a license to repeat device-add.
- The helper pins loop ownership through native-UUID loop references and boot-aware receipts.

**Citations:** A:same path`:1045-1122,1217-1222,1260-1303`.

The historical helper also explicitly refuses a standalone direct mount of the physical seed:

```sh
seed_fail "directly mounted seed source; unmount it and keep its fstab entry noauto"
```

**Citation:** A:same path`:494-505`.

That is a historical behavior, **not an instruction to unmount today’s seed**. It conflicts with retaining a permanently mounted seed if that is part of the current required design.

#### Intended Oct5 completion chain

The wrapper prepares external host storage/native definition, submits a digest-pinned clean plan, reads back the authoritative binding, checks physical storage, starts through `identity_sled.touch_session`, reads authoritative/native state again, then publishes boot completion.

Original source:

```sh
# Canonical lifecycle admission verifies storage, starts the native guest
# and commits its active session state; native start alone is insufficient.
```

**Citation:** A:`deploy/scripts/provision-workspace-subscriber.sh:354-384`.

This is an intended completion protocol; it does not establish that the historical run reached its end.

### 3. Oct11: GPT root/upgrade separation and manual staging

B’s wrapper explicitly distinguishes its new staged path from the retained unpartitioned workflow:

> “Existing identity mode below remains the older unpartitioned leaf workflow; do not pass a partitioned image to that mode.”

**Citation:** B:`deploy/scripts/pro