# Permanent Golden build OS

The writable build OS is `/mnt/@opdbus-system-a` on the separately pinned
OS Btrfs filesystem. Its `/opt/op-build.src` and `/opt/op-build/target`
are retained between builds. Building never exports HEAD over source edits,
initializes a replacement OS, or falls back to the Incus/ublk pool.

Releases are unique read-only snapshots at
`/mnt/@upper/golden-release-<stamp>.<unique>/os`. Each release has ordinary
sibling `upper/` and `work/` directories. `golden-candidate.json` stages a
candidate; `golden-state.json` remains the boot selection until the complete
overlay/service/FUSE transition succeeds. Cutting never deletes a previous
release or renames an in-use lower. Rollback uses that same transaction.

## Configuration and boot

One root-owned mode-0600 file per setting lives in `/etc/op-dbus/env/`:

| Setting | Value on the current host |
| --- | --- |
| `GOLDEN_HOST_MOUNT` | `/mnt` |
| `GOLDEN_WORK` | `/mnt/@opdbus-system-a` |
| `GOLDEN_ROOT` | `/mnt/@upper` |
| `GOLDEN_FSID` | OS filesystem UUID |
| `GOLDEN_WORK_UUID` | Permanent writable OS subvolume UUID |
| `GOLDEN_UPPER_UUID` | Snapshot parent subvolume UUID |

The top-level OS filesystem is mounted from its UUID and `subvolid=5` in
fstab. No release operation covers the Incus pool mount.

The existing `op-release-activate` stage-1 owner retains its authenticated
native-kit behavior when `OPDBUS_GOLDEN_BOOT` is absent or zero. An explicit
value of one selects the SHA256-pinned `boot-golden.sh` helper through
`OPDBUS_GOLDEN_BOOT_SHA256`. The helper validates filesystem and subvolume
pins before restoring the existing selected overlay and its primary alias.
It never builds, recovers a transaction, grants authority or manages services.
A selected Golden failure does not silently fall through to another layout.

## Commands

`./activate-golden.sh` and `build-golden.sh` invoke `golden.sh`. Dry-run is
the default; `--apply` is required for every mutation.

```sh
./activate-golden.sh status
./activate-golden.sh init                 # verify the existing OS, not recreate it
./activate-golden.sh build                # retained-source chroot build plan
./activate-golden.sh cut                  # prepare candidate; do not publish boot selection
./activate-golden.sh rollback             # complete transition to a pinned prior release
./activate-golden.sh recover              # restore an interrupted/failed transaction
./activate-golden.sh mount-overlay        # refuses a differing existing mount
./activate-golden.sh activate             # enabled-service plan
./activate-golden.sh deploy --with-build  # build, cut, activate, replace aliases
```

Cargo runs from the retained source root with `CXXFLAGS="-include cstdint"`,
paired verified `THREETCHED_CAPTURE_BIN`/`THREETCHED_CAPTURE_MANIFEST`, an
isolated capture config directory and explicit `--target x86_64-unknown-linux-gnu`.
Activation consumes `target/x86_64-unknown-linux-gnu/release` as well.
`GOLDEN_CARGO_JOBS` may select one worker. Build/cut/deploy and the assigned
external Cargo worker share the existing `/mnt/@upper/.golden.lock`; cut also
refuses any active Cargo/rustc/Clippy process.

The execution owner records original export SHA in
`/opt/op-build/source-origin.json` (`source_commit`, 40 hex characters).
`build-receipt.sh` records normalized source-tree digest and executable
artifact hashes in `/opt/op-build/execution-receipt.json` after success.
It never infers a SHA from an unrelated HEAD. Explicit catalog sealing must
produce `/opt/op-build/catalog-seal-receipt.json` with `exit:0`,
`source_commit`, `source_digest`, and `catalog_manifest_sha256`, and stage
that exact catalog under `/usr/share/op-dbus/plugin-blobs`. Cut rechecks
source/artifacts/catalog both before and after snapshot creation. Missing
legacy receipts remain unavailable evidence, not fabricated provenance.

`transaction.sh` saves original boot state, activation ledger, executable pins
and both FUSE conf/identity pins before owner effects. It stages the overlay
before service changes and publishes boot selection last. Any failure attempts
the complete saved restore and verifies service/FUSE health; failed restore is
recorded `rollback-incomplete`, never success. A durable unfinished journal
blocks another transition until explicit recovery. Retained historical lower
trees are not automatically certified as new-format receipt-matched releases.
The catalog must already be acknowledged by both live and persisted catalog
owners; deployment never publishes an unverified catalog fallback.

`activate.sh` preserves the existing enabled-service table, prior executable
pins and rollback. It validates the durable grant document and requires Fresh
materialization; it never adds catalog capabilities to `$ROOT` or changes any
principal. Explicit narrow grant maintenance uses a validated atomic durable
document update followed by materializer `once` and `check`.

`fuse-swap.sh` remains the host FUSE binary-pin owner. Canonical human-session
selection is separate from changing a binary, and must preserve the previous
valid identity until the new authored identity can bind.

## Native identity seed preparation

OS builds remain on the OS disk; identity roots and data remain native
subvolumes on `3tched-pool`. The one-time host import in
`deploy/provisioning/15-import-golden.sh` transfers an approved read-only
snapshot to a new private pool baseline, then wraps a writable derivative
with an **ordinary directory** `rootfs/` and metadata before sealing it.
Nested rootfs subvolumes are refused because parent snapshots omit them.
Existing seeds and guests are never changed by this import.

`16-stage-golden-runtime.sh` prepares a private writable successor with runit
guest integrations and per-setting configuration. It intentionally does not
seal or publish a final seed. After the fresh same-source build and explicit
catalog seal, `17-finalize-golden-runtime.sh` accepts exact artifact/catalog
digests, assembles the unused candidate and seals it. Only then may the
director configure its seed UUID/name for identity operation plan generation.
An imported/staged seed is not evidence of guest admission or readiness.

The retained native identity owner creates an admitted empty placeholder
and invokes `op-container` with the pinned SeedLocation. Existing nonempty
roots, such as Jeremy's, require the dedicated digest-plan native upgrade;
operator scripts must not replace those roots behind Incus.

## Build preflight

`build` runs `preflight.sh` before any mount or compile, so a build behaves the
same whether it is started from a terminal, an agent or a cloud desktop:

| Check | Refuses when | Override |
| --- | --- | --- |
| Source | `GOLDEN_SOURCE_REPO` is not `/srv/git/odbus-verified` | `GOLDEN_ALLOW_SOURCE_OVERRIDE=1` |
| Static catalog match | a state plugin's literal `version()` in the source differs from the sealed catalog (`GOLDEN_SEALED_CATALOG`, default `/var/lib/opdbus/plugin-blobs`, the persisted catalog containers mount, read with `opblob inspect`) | `--allow-catalog-change` (deliberate bump, sealed after the build) |
| No rebuild | the build OS already holds a verified successful execution receipt for the same commit and source digest | `--force` |

Caches: C/C++ goes through ccache (`CC`/`CXX`, `/var/cache/op-build/ccache`), the
cozorocks `librocksdb.a` is linked from `/var/cache/op-build/rocksdb/<key>`, and
when `sccache` is installed in the build OS it is used as `RUSTC_WRAPPER` with
`/var/cache/op-build/sccache`. The build log names each cache it uses.
