## Summary

**The supplied current Rust snapshots do not support a no-host-loop storage layout.** Loops are not merely provisioning-script implementation details: the final adoption and subsequent start proofs explicitly require them. Replacing image creation with native Incus subvolumes while leaving those proofs unchanged fails admission; deleting the loop checks without replacing their identity/ancestry guarantees would weaken security.

The smallest defensible specification is:

1. Preserve the existing native container, UUID, identity selectors, creation journal, admission route and final adoption transaction.
2. Change the existing external storage helpers and the existing storage-proof representation together.
3. Retain actual immutable-seed/sprout membership, private persistent writes, separate upgrade replication and root composition.
4. **Do not assert that native Incus subvolumes satisfy the seed-member requirement.** They supply filesystem trees, not writable block members. A direct-block implementation is a conditional no-loop direction, not an established solution in these exports; compatibility with the permanently mounted seed and the recorded kernel restriction remains unresolved.
5. Treat existing pool loop backing as a separate required conflict—not an exemption and not authorization to migrate it.

All **ten manifest-listed current `crates/` snapshots**, the source provisioning document and both source reports were read completely, including their embedded tests. No tests, commands, changes or runtime operations were performed.

---

## Files and complete read coverage

All paths in this report beginning `crates/`, `docs/` or `reports/` refer to snapshots beneath:

`/srv/git/odbus-verified/reports/provisioning-fusion-20261019/evidence/source/`

They are **working-tree exports**, not proof of installed behavior or exact contents of commit `051a20d…`.

For files longer than 250 lines, reads were consecutive ranges `1–250`, `251–500`, etc., through EOF; ranges extending beyond EOF returned the remaining file. There are **no unread ranges in the following assigned files**.

| Snapshot | Complete covered lines | Manifest export SHA-256 | Holds |
|---|---:|---|---|
| `crates/op-grpc-bridge/src/native_container_creation.rs` | 1–173 | `9ab0eab86de8c3c289feacf02c1fa1edb186d5726a491ee84dfa27627edf43d2` | Native request pinning, admission, sole-POST permission, reconciliation |
| `crates/op-grpc-bridge/src/identity_sled_dispatch.rs` | 1–4833 | `52984d66ae3e145976171a517f2e5c2119c64d58b1df5549ebe9cdfb94756f64` | Verified preparation/adoption routes, lifecycle locks, power/readback/CAS, tests |
| `crates/op-grpc-bridge/src/identity_seed_provision.rs` | 1–1245 | `c5b49bfb51f63489ee8aa291203fdbbf3ce8f5e3cb44ebe0205472c5c183250b` | Admission, current clean-adoption routing, disabled older allocation continuation |
| `crates/op-grpc-bridge/src/identity_clean_provision.rs` | 1–704 | `69b6bd40178fba0c1164091310f3e0f06226d5f2603e4b69ad6336563bc48cbe` | Exact adoption plan, native proof, replay and authoritative commit |
| `crates/op-grpc-bridge/src/identity_clean_provision/storage.rs` | 1–369 | `59e9ee03f63c5c640bc279ad7ace28941fe96f178979da5e692703465ba80f99` | Held-FD physical seed, loop membership, inherited subvolume and persistence proofs |
| `crates/op-grpc-bridge/src/identity_clean_provision/partitioned.rs` | 1–181 | `7b1719d38f5275abffb4a9f22bbf867afbfa058e78c1e9a9077a15490ffbf345` | GPT/loop partition proof, overlay composition and upgrade ancestry |
| `crates/op-cozo-store/src/identity_transactions/clean_provision.rs` | 1–354 | `ed44e531654d48c5502a65f8a0fab890faeef5f334ce88b9e23c42da05dab0d7` | Clean-adoption transaction validation, receipt, immutable binding CAS |
| `crates/op-cozo-store/src/identity_transactions/native_creation.rs` | 1–229 | `83cc35825782a7dcabc92032581383079ae7b972a005c58c5782d9c3d45a8d81` | Durable native allocation sequence and intent/bound/complete transitions |
| `crates/op-grpc-bridge/src/lungs_socket.rs` | 1–782 | `9fc05dc5308c309ea6371d2889bfd126a920feebc0ecae9d97cb37f4a5b16a26` | Socket-origin pidfd, namespace/native identity and per-call revalidation |
| `crates/threetched-fs/src/mounted.rs` | 1–1108 | `2a83fd89057d38da2fec7ced8af159c18eed6857644a3f1a5417bc8d8988cab8` | Pinned mounted namespace and same-handle terminal receipts |
| `docs/src/operations/shared-seed-provisioning.md` | 1–259 | `318ee31d28f534a280604e8e27dc22dec5bdb6b9f559f67755d4c92a9ca4cffe` | Current GPT contract and explicitly separate older unpartitioned contract |
| `reports/provisioning-audit-20261018.md` | 1–153 | `1c65adec2a99048bfcdee631a6a2ddce9be3611f81d305d7ce425d60f6611a0c` | Original ten findings, evidence boundaries and repair directions |
| `reports/clean-provision-20261005.md` | 1–132 | `370d45d0a20111b3fbaa47b9d8a7e7012b459e86f8b55f88ad57add4a62c183c` | Historical source/preparation/admission/naming drift and incomplete activation |

Hashes above are **manifest values**, not independently recomputed hashes. The redacted `identity_seed_provision.rs` export has a different `source_sha256`; the table intentionally uses its exported-byte hash.

Additional coverage:

- `CASE.md`: complete, including rereading its initially line-truncated safety paragraph.
- `latest-owner-correction.json`, `subvolume-preference.json`, `jev-initial-decision.json`, `jev-no-host-loops.json`: complete.
- `manifest.json`: complete, lines 1–1256.
- All ten mandatory `.agents/skills/*/SKILL.md` files: complete; paged through the four initially truncated skills. Also read `using-superpowers`, `systematic-debugging`, `rust`, and `application-security`.
- Supplemental original history: **only Hermes record 3077**, physical line 80 of `evidence/hermes/20261018_222032_11fb9b.jsonl`, fully decoded, for the native declaration absent from current snapshot inventory. This is not full transcript ingestion.
- Supplemental shell search matches only: `bind-instance-root.sh` and `provision-workspace-subscriber.sh`; no full shell-file coverage claimed.
- Startup `HANDOFF.md` read returned lines 1–300 with truncations. It is not used below as primary source evidence; no full handoff ingestion claimed.

---

## Architecture

The current relevant chain is:

```text
MountedNamespace: pinned identity/catalog + same open call descriptor
  → identity-bound lungs/D-Bus admission
  → identity_sled.prepare_container
      → durable native creation reservation
      → at most one POST
      → native UUID/readback → canonical publication → creation complete

External host storage owner:
  seed/leaf establishment → upgrade pairing → mapped root composition
  → protected adoption plan

identity_sled.provision_container:
  valid digest → clean_provision::provision
  → caller/key/seal admission
  → exact expected revision + native ETag + held storage proof
  → final rechecks
  → IdentityIntent::CleanProvision → Cozo receipt + paired publication

touch_session / existing service startup:
  completed clean receipt → revalidate native/storage proof
  → bound native start → full power readback → identity CAS
```

**Creation completion is not adoption completion.** Native journal completion does not create the authoritative identity/storage binding.

---

## 1. Exact native declarations and ownership

### Current callable routes

`identity_sled_dispatch.rs:2546–2652` implements:

- `prepare_container`: strict input `{ wireguard_pubkey: String, operation_id: String }`.
- `inspect_provision_preflight`: exactly one argument, `wireguard_pubkey`.
- `provision_container`: `ProvisionInput { wireguard_pubkey, plan_sha256 }`.

These are existing methods—not proposed new APIs. Runtime availability still depends on the sealed contract and current grant.

Preparation is restricted to the configured chatbot and exact derived session. Existing native/identity bindings prevent a fresh creation branch. Existing operation IDs select reconciliation instead.

Original excerpt, `identity_sled_dispatch.rs:2573–2589`:

```rust
let revision = load_revision(&store, &sid).await?;
anyhow::ensure!(revision.record.is_none() && revision.genesis.is_none(),
    "retain the existing identity/native binding; explicit retirement is required first");
```

The existing target must therefore retain its native UUID and creation record. **Do not select a new operation UUID to evade a retained intent.**

### Native declaration provenance gap and recovered original

The manifest does **not** include current `crates/op-plugins/src/state_plugins/incus.rs`. An exported-file search found no `incus.rs` snapshot. I recovered the original declaration from:

- `evidence/hermes/20261018_222032_11fb9b.jsonl:80`
- record ID **3077**, active `1`, compacted `0`
- tool call `call_F4b41uBOjy8aNju2zllkPbbx`
- historical source lines **1464–1533**, including the complete function at 1464–1493.

This is historical source evidence, **not proof that the same function is installed today**.

Exact original declaration body, historical lines 1483–1492:

```rust
Ok(serde_json::json!({
    "type":"container","ephemeral":false,"source":{"type":"none"},"profiles":["3tched-lungs"],
    "config":{"security.privileged":"false","security.nesting":"false","security.protection.shift":"true",
        "boot.autostart":"false","security.syscalls.deny_default":"true",
        "security.syscalls.deny":"ioctl errno 1 [1,0x4008941a,SCMP_CMP_MASKED_EQ,0xffffffff]\nioctl errno 1 [1,0x5000940f,SCMP_CMP_MASKED_EQ,0xffffffff]\nioctl errno 1 [1,0x5000943f,SCMP_CMP_MASKED_EQ,0xffffffff]",
        "user.opdbus.session_id":session,"user.opdbus.wireguard_public_key":public_key,
        "user.display_name":"control-plane-chatbot","raw.lxc":format!("lxc.uts.name = {session}")},
    "devices":{"root":{"type":"disk","path":"/","pool":"identity-roots"},"runtime-run":runtime,
        "fuse":{"type":"unix-char","source":"/dev/fuse","path":"/dev/fuse"}}
}))
```

The function first requires:

```rust
pool["driver"] == "dir" && pool["status"] == "Created"
    && pool["config"]["source"] == "/var/lib/incus-storage/identity-roots"
```

It requires an empty profile `config`, only `runtime-run` and `lungs-socket-dir` device names, exact readonly lungs directory attachment, and runtime tmpfs after removing `initial.mode` from the cloned declaration.

The source document’s original profile YAML, `shared-seed-provisioning.md:190–207`, is:

```yaml
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

Current `native_container_creation.rs:106–124` corroborates the exact expanded-device closure: **four** devices—root, runtime-run, fuse, lungs-socket-dir—and stopped/unprivileged/nonnested/non-autostart security.

**Minimal no-loop proposal:** preserve these existing native declarations initially. Changing external storage does not require changing the guest into a VM, attaching a custom block volume, granting storage administration or inventing a new endpoint.

A proposed `root.pool: 3tched` would instead change the current native-root contract, require a supported migration preserving UUID, and still would not implement seed membership or upgrade composition. It is not a one-line repair.

---

## 2. Proof structs and exact fields

### Adoption plan

`identity_clean_provision.rs:29–79`, all deserialized plan structs use `deny_unknown_fields`.

```rust
struct MemberPin {
    id: u64,
    uuid: String,
}

struct PartitionPin {
    partuuid: String,
    start_sector: u64,
    sectors: u64,
}

struct PartitionedOverlay {
    disk_guid: String,
    root_partition: PartitionPin,
    upgrade_partition: PartitionPin,
    upgrade_fsid: String,
    host_upgrade_fsid: String,
}

struct CleanPlan {
    version: u8,
    session_id: String,
    wireguard_pubkey: String,
    expected_revision: u64,
    expected_revision_operation_id: String,
    binding: SledBtrfsDevice,
    seed_device: String,
    seed_fsid: String,
    seed_generation: u64,
    leaf_inode: u64,
    leaf_size: u64,
    members: BTreeMap<String, MemberPin>,
    persist_inode: u64,
    native_root: String,
    pool_name: String,
    pool_source: String,
    instance_etag: String,
    instance: JsonValue,
    previous_instance_etag: String,
    previous_instance: JsonValue,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    partitioned_overlay: Option<PartitionedOverlay>,
}
```

`SledBtrfsDevice`’s declaration is outside the snapshot set. Its exact fields are demonstrated by construction in `identity_seed_provision.rs:650–658`:

- `device_path`
- `mount_point`
- `btrfs_uuid`
- `cozo_id`
- `attached`

Here `cozo_id` is the derived session identifier. It is a durable association—not a block-device implementation.

### Other proof/receipt structures

| Structure | Exact fields / role | Anchor |
|---|---|---|
| `StorageGuard` | `_files: Vec<File>`; retains verified descriptors | `storage.rs:17–19` |
| `CleanRootGuard` | `should_start: bool`, `_storage: StorageGuard` | `identity_clean_provision.rs:435–438` |
| `PairReceipt` | `phase`, `host_fsid`, `replica_fsid`, `direction`, `generation`, all `String`; strict unknown-field rejection | `partitioned.rs:78–86` |
| `IdentityCleanProvision` | `expected: IdentityRevision`, `record: IdentitySledRecord`, `genesis: GenesisInputsRecord`, `plan_sha256: String`, `provenance_json: String` | Cozo `clean_provision.rs:4–10` |
| `CleanProvisionReceipt` | `plan_sha256`, `operation_id`, `expected_revision: u64`, `committed_revision: u64`, `provenance_json`, `btrfs_device_json`, `instance_json` | Cozo `clean_provision.rs:13–21` |
| `NativeCreationRequest` | `operation_id`, `actor_id`, `caller_session`, `caller_genesis`, `owner_kind`, `logical_target`, `schema_hash`, `request_digest`, `request_json`, `reserved_name: Option<String>` | Cozo `native_creation.rs:6–21` |
| `NativeCreationRecord` | `request`, `number: i64`, `name`, `phase`, `native_uuid` | Cozo `native_creation.rs:23–31` |
| `ProvisionSealPin` | `method`, `schema_hash`, `blob: PluginObjectBlob` | `identity_seed_provision.rs:256–260` |
| `ContainerBinding` | `session_id`, `session_genesis`, `native_name`, `native_uuid`, `init: ProcessIdentity` | `lungs_socket.rs:171–178` |
| `ProcessIdentity` | `pid`, `start_ticks`, held user/PID namespace identities | `lungs_socket.rs:111–117` |

The storage verifier also defines exact kernel ABI buffers `FsInfo`, `DevInfo`, `LoopInfo`, and tree-search structs; sizes are asserted as 1024, 4096, 232 and 4096 bytes respectively. These are read-only ioctl outputs, not authority supplied by the caller.

### Required physical properties

`storage.rs:259–369` verifies:

- Root-controlled, no-symlink descriptor walks.
- Seed is protected block storage; `BLKROGET == 1`.
- Seed superblock FSID, SEEDING flag, generation.
- Leaf is a root-owned private regular file with exact inode/length.
- Sprout has the pinned FSID and top-level subvolume ID 5.
- Exactly seven inherited subvolume IDs; each ID/UUID/protection matches the plan.
- Native guest siblings are the same dev/inode as the corresponding inherited siblings.
- Persistence is the exact ordinary mode-0700 directory, not inode 256, on `@agent_user`; native persistence points to the same object.
- Version 4 image lies on the pinned pool FSID/top-level root.
- Exactly one physical seed member and one writable leaf member are recognized.

The decisive loop dependency is explicit:

```rust
ensure!(unsafe { libc::ioctl(file.as_raw_fd(), 0x4c05 as libc::c_ulong, &mut status) } == 0,
    "member is not a pinned loop mapping");
```

And final ancestry closure:

```rust
ensure!(count == 2 && saw_seed && saw_leaf, "shared seed/leaf ancestry incomplete");
```

`storage.rs:353–367`.

For v4 partitions, `partitioned.rs:54–65` explicitly requires:

```rust
let loop_number = loop_name.strip_prefix("loop").context("partition parent is not loop storage")?;
```

It checks GPT CRC/header/table, disk GUID, Linux partition type, PARTUUID and geometry, kernel partition attributes, and loop backing dev/inode/rdev/offset/size/access mode.

**Therefore direct block partitions and native subvolumes are rejected by the current proof even if mounted correctly.**

### Root composition and upgrade ancestry

`partitioned.rs:88–181` requires:

- Readonly OverlayFS at both root-view and native root.
- Exact lowerdir order `upgrade-layer:seed-layer`.
- Same native/view object and mapped root UID 1000000.
- Seed layer equals inherited `@`.
- Upgrade is a standalone filesystem rooted at ID 5, with one writable device and a proven second partition.
- Published `@` equals mapped upgrade-layer.
- Both lower mounts readonly.
- Private bounded pair receipt, `phase == "paired"`, direction `host-to-container`, matching host/replica FSIDs.
- Host and received baseline subvolumes readonly.
- Published clone parent UUID equals received baseline UUID.
- Received UUID equals host baseline UUID.

Original ancestry predicate:

```rust
ensure!(u64::from_ne_bytes(received_info[288..296].try_into()?) & 1 != 0
    && u64::from_ne_bytes(host_info[288..296].try_into()?) & 1 != 0
    && published_info[312..328] == received_info[296..312]
    && received_info[328..344] == host_info[296..312], "incremental upgrade baseline ancestry differs");
```

`partitioned.rs:176–180`.

A no-loop change must preserve these relationships; matching names or a `"paired"` JSON field alone is insufficient.

---

## 3. Revision/digest CAS and admission

### Admission remains unchanged

`identity_seed_provision.rs:282–326` rechecks:

- Compiled method versus sealed method contract.
- Exact schema hash and complete pinned blob components.
- Current exact-principal `identity_sled.write`.
- Current caller session and genesis.
- Existing caller identity.
- For nonhuman callers, current repair SID1 verification.

Humans are deliberately exempt from the per-container self-instance requirement, **not** exempt from session/genesis/grant checks.

Target principal registration is checked by `target_key_is_registered`, lines 243–254. This must not be replaced with bootstrap, another identity, a grant edit or a direct socket call.

### Clean adoption

`identity_clean_provision.rs:500–675`:

1. Locks target lifecycle, then host observation.
2. Admits caller, verifies registered target key and loads expected revision.
3. Reads the protected plan using the target SID and supplied digest.
4. Validates complete plan.
5. Uses deterministic operation ID:

   ```rust
   let operation = format!("clean-provision-{}", input.plan_sha256);
   ```

6. Matching prior receipt enters replay verification, not fresh mint/adoption.
7. Fresh adoption verifies expected revision, strict native metadata and ETag, pool and physical storage.
8. Authors fresh parked service state through the existing identity owner.
9. Rechecks exact revision, admission and strict native/storage proof immediately before commit.
10. Commits through `commit_identity_intent` and `IdentityIntent::CleanProvision`.

Original final checks:

```rust
ensure!(
    load_revision(store, &sid).await? == expected,
    "identity changed during external adoption proof"
);
```

and:

```rust
let (_final_storage, _) = native_proof(&plan, true).await?;
```

`identity_clean_provision.rs:632–646`.

The Cozo validator, `clean_provision.rs:72–178`, enforces:

- Expected SID/revision/operation predecessor.
- Same derived key identity.
- Fresh service state, inactive, mutation index zero, fresh genesis/credential metadata.
- Version-specific fresh/deletion/replacement eligibility.
- Correct session-to-storage and stopped native binding.
- Exact replay equivalence or exact predecessor equality:

```rust
} else if previous != &provision.expected {
    return Err(err("clean provision compare/exchange conflict"));
}
```

Appending the receipt refuses incomplete prior storage journals, archives prior revision/journals privately, and removes superseded journal rows in the transaction. It does not treat prior metadata as the new storage proof.

### Immutable post-adoption binding

Cozo `clean_provision.rs:322–354` permits explicit deletion, but otherwise:

- Device binding cannot change except logical `attached`.
- Native UUID/config/device/profile binding remains pinned.
- Only a closed power/idmap observation normalization is allowed.
- Revision cannot predate the adoption receipt.

Original rejection:

```rust
return Err(err("completed clean provision binding is immutable"));
```

A storage migration after completed adoption cannot be smuggled through `write_identity`. For the recorded not-yet-adopted candidate, change the preparation/proof contract before final adoption rather than deleting/recreating identity or native state.

---

## 4. Cancellation, failure and resume boundaries

| Boundary | Existing behavior | Required preservation |
|---|---|---|
| Mounted operation | `mounted.rs:394–460`: write, fsync, seek, read **same descriptor**; validate mount before/after | No sibling `result`/`error` files; no automatic resubmission |
| Transport cancellation | Verified prepare/adoption routes spawn owned Tokio tasks; activation/deactivation do likewise | Disconnect/timeout is not proof of no effect |
| Native reservation | Cozo intent is committed and synchronized before `fresh == true` permits POST | Preserve original operation/request and pending fence |
| Native POST uncertainty | Every failure after reservation retains fence; missing instance does not authorize a second POST | Reconcile original operation; no reset or fresh-ID workaround |
| Native reconciliation | Ownership markers, stopped definition when incomplete, native UUID, full readback and canonical publication precede `complete` | Creation journal remains distinct from lifecycle/adoption authority |
| Adoption proof failure | No storage composition in bridge; prepared external state remains external | Reconcile retained helper intent/storage; do not cleanup/re-add blindly |
| Adoption replay | Same digest requires identical provenance, authoritative binding and current native/storage guard; recovers publication | No second mint or replacement commit |
| Durable/publication gap | `CredentialPublication::Pending` returns operation-scoped recovery diagnostic | Do not equate an error with no durable commit |
| Startup publication recovery | `identity_sled_dispatch.rs:664–701`: rereads authoritative Cozo state and republishes pending operation | No stale cached whole-array replacement |
| Start | Storage/native guard and expected revision precede bound native start; readback precedes active CAS | Failed start must not remove membership |
| Stop | Inactive power policy committed **before** stop; failed stop/readback preserves that policy and requires reconciliation | No rollback to active; no unmount/member churn |
| Boot/storage restoration | Document assigns restoration and upgrade sync to existing host helpers before separate start | Restore established resources only; no creation on lifecycle paths |

Exact native fence excerpt, `native_container_creation.rs:80–89`:

```rust
// Any failure from here retains the fence, including definite provider
// errors. An absent object is not proof that an earlier POST was rejected.
if fresh {
```

Exact durable store boundary, Cozo `native_creation.rs:130–133`:

```rust
/// `fresh` is the only permission to issue POST, returned after the barrier.
/// Replays, including intent-only replays, may only read/reconcile.
```

There is explicitly **“No abort/reset API”** at Cozo `native_creation.rs:187`.

The older `ProvisionExecution` phase machinery must not be mistaken for an available storage orchestrator. Current valid digests go directly to clean adoption at `identity_seed_provision.rs:719–730`; `compose_leaf` unconditionally rejects, and the old branch is gated before journal/native effects.

Original:

```rust
const EXTERNAL_PROVISIONING_REQUIRED: &str = "prepare the shared physical seed, private writable leaf and container through the external host provisioning script or Incus; the bridge cannot compose, mount or copy identity roots";
```

`identity_seed_provision.rs:820`.

---

## 5. Minimal no-loop modification specification

These are **proposed changes**, not implemented functionality or a proven feasible deployment.

### A. Preserve the existing native/identity control plane

Intentionally unchanged:

- Existing `3tchedCtlChatbot`, recorded UUID `137c61ff-435c-4300-b3c0-c605e461be13`, configured key/SID and creation operation.
- `prepare_container`, `inspect_provision_preflight`, `provision_container`, `touch_session`.
- Native reservation/POST fence.
- Mounted same-handle receipts.
- Exact-principal admission, sealed schema checks and lifecycle→host lock ordering.
- Lungs socket-origin pidfd/native UUID/namespace checks.
- Unprivileged/nonnested configuration, native idmapping and the three storage ioctl denials.
- Final deterministic digest/revision CAS and publication recovery.
- Bootstrap and model routing remain out of scope; do not activate `jev-router`.

### B. Change proof representation, not its strength

Target existing files:

1. **`identity_clean_provision.rs`**
   - Replace image-only assumptions in the selected no-loop layout: `<name>.img`, leaf inode/size as regular-file identity, mandatory GPT partition geometry.
   - Retain identity/SID/native UUID/ETag, seed FSID/generation, exact inherited members, persistence object, root composition and upgrade source/replica pins.
   - Use an explicitly distinguishable plan representation; do not reinterpret a v4 image plan as a subvolume or physical-device plan.
   - Update the existing plan author and Cozo version predicates together.
   - Do not broaden fresh/replacement eligibility or relax unknown-field rejection.

2. **`storage.rs`**
   - Replace `LOOP_GET_STATUS64` backing-file equivalence with an equally strong held-descriptor proof for the selected real storage primitive.
   - For a direct-block seed/sprout design, prove the actual protected seed block object and actual writable member, their kernel identities, FSIDs and access modes, and reject loop-backed ancestry.
   - Preserve exactly the intended membership; retain seed SEEDING/generation/block-RO verification and the seven inherited member identities.
   - Keep ordinary persistence-directory and native bind equality checks.
   - Do not merely remove `count == 2` to make an unrelated single-filesystem subvolume layout pass.

3. **`partitioned.rs`**
   - Replace image/loop partition ownership checks only where the selected layout actually eliminates GPT/image backing.
   - If physical partitions remain, retain genuine partition-parent/geometry/identity proof without requiring a loop parent.
   - If separate storage objects replace the two partitions, explicitly prove separation and each object’s ownership; do not fabricate PARTUUID/GPT fields.
   - Preserve readonly overlay lower order, ID-mapped source equality, paired generation receipt, received/host UUID ancestry and protected baselines.

4. **Cozo `clean_provision.rs`**
   - Extend only the recognized provenance-layout validation needed for the actual selected storage representation.
   - Preserve complete predecessor equality, deterministic digest operation, replay equality and immutable completed binding.
   - No new public API or direct database mutation is needed for the specification.

### C. Modify the existing external helper chain coherently

The document identifies the necessary existing integration targets:

- `prepare-instance-disk.sh`: remove image/loop allocation only after selecting a genuine replacement primitive and receipt.
- `provision-workspace-subscriber.sh`: retain existing staged workflow and original native operation; carry the selected storage receipt consistently.
- `sync-instance-upgrade.sh`: retain pre-receive mount/FSID proof, host→container pairing, incremental ancestry, stopped-only lower-layer changes and complete restoration.
- `bind-instance-root.sh`: consume the revised storage identity without altering native UUID/security; retain guest FUSE declaration, mapped layers, sibling/persistence binds.
- `prepare-chatbot-adoption.sh`: author the revised exact plan from verified receipts and current native/preflight readback.
- `restore-chatbot-root` / `identity-roots`: restore established no-loop backing, synchronize, rebuild and verify root before readiness/start; never allocate or change membership.

These shell modification targets are derived from the fully read source document and reports; their full implementation audit belongs to the parent’s shell-source work.

### D. Explicit unresolved requirement conflict

The Incus skill read in this session states:

> “Btrfs seeding/sprouting | Separate block-device filesystem operation; a subvolume is not a writable block member for `btrfs device add`.”

The source document says:

> “The filesystem stage attaches a read-only loop view of the sealed seed and adds the blank root partition once as its writable leaf. It inherits the seven seed subvolumes; it does not copy the root or create replacement agent subvolumes.”

`shared-seed-provisioning.md:44–47`.

It also says:

> “Linux 6.17 and newer cannot mount the standalone seed and a sprouted filesystem simultaneously; the host mount owner must reconcile the standalone seed mount and its boot declaration before pairing.”

`shared-seed-provisioning.md:21–24`.

Consequences:

- **Established:** simply changing to Incus-managed subvolumes does not satisfy the current seed-member proof.
- **Established:** current image/seed wrappers and pool image violate the hard no-host-loops rule.
- **[INFERENCE] Conditional direction:** actual non-loop writable block storage could retain block-level seeding semantics, without a disk image, if a supported available allocation and mounting arrangement is established.
- **Not established:** that such an arrangement is available, supports the required concurrent/permanent seed mounts on the deployed kernel, or can be adopted through the exact existing Incus storage owner without prohibited changes.
- **Not acceptable:** copied roots, ordinary snapshots or an overlay upper silently presented as the original writable sprout.

**Smallest decision needed:** resolve whether the retained contract requires literal Btrfs seed-device membership with the seed permanently mounted in the stated form, or explicitly permits another storage semantic. Until then, a subvolumes-only completion claim is unsupported. This is a requirement/primitive conflict, not a reason to weaken proofs or repeat old provisioning commands.

The pool-level loop additionally requires a separately authorized, data-preserving no-loop backing resolution. Merely changing the per-container storage does not meet the hard constraint.

---

## 6. What the two source reports establish—and do not establish

### Original Oct18 audit

The report records ten findings: obsolete receipt interface; absent host paths; rejected plan field; erased guest FUSE fstab; detached root after normal sync; invalid whole-policy grant updates; premature completion; lost-update grant race; native operations outside canonical admission; and boot early-exit suppressing other identities.

It explicitly distinguishes legacy v2/v3 from GPT v4 and source defects from observed runtime failures. It must not be treated as proof that every finding remains in the later snapshots.

Current source corroborates the intended repaired safety boundaries:

- Same-handle operation receipt: `mounted.rs:394–496`.
- No `principal_group` in strict `CleanPlan`.
- Verified native preparation route: `identity_sled_dispatch.rs:2546–2595`.
- Bridge verifies, rather than composes/mounts, external storage.

### Oct5 report

This is historical preparation/admission/naming evidence, not current readiness evidence.

Its final original statement is:

> “Both compiler runs exited0 (13m20s,10m21s), but both frozen module hashes fail verification. No claim of consistent source/artifact provenance is possible yet.”

and:

> “The old Cozo row and failed candidate remain; no provisioning is complete.”

`reports/clean-provision-20261005.md:124–132`.

Do not revive its historical native alias or admission denial as today’s target/blocker.

---

## Explicit gaps and evidence limits

1. **Current `incus.rs` is absent from the supplied snapshot manifest.** Exact declaration recovered from original Hermes record 3077 is historical; present compiler/installed equality is unproven.
2. Imported implementations not supplied here include `identity_root_repair.rs`, `identity_physical_root.rs`, `identity_delete_provision.rs`, MutationEngine publication internals, broader Cozo transaction machinery, `op_network` storage verification and `identity_sled_private_fixture_tests.rs`. Their call boundaries were read, not their full implementations.
3. Native plan-reader implementation is not supplied; this report establishes its invocation and expected digest/private-plan contract, not an independent audit of that reader.
4. No live schema, daemon version/extensions, kernel seed-mount feasibility, installed binary provenance, storage availability or no-loop backing state was probed.
5. No full installed-helper/history comparison or complete transcript ingestion is claimed by this scout; the parent owns those tasks.
6. No tests were executed. Embedded test source was read, including no-start-on-proof-failure, failed-start membership preservation, serialized lifecycle and immutable native-binding tests.
7. These findings have **not** been panel-reviewed or newly JEV-adjudicated in this session. The read initial judgments cover scope and rejection of a pool-loop exemption only.

**Deliverable status:** assigned current-source ingestion and diagnostic/specification are complete. A concrete executable no-loop/subvolumes-only implementation cannot be claimed from these exports without resolving the explicitly identified storage-semantic and kernel-mount constraints.