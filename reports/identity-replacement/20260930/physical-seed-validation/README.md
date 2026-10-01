# Physical seed candidate validation

Both candidates `i0` and `i1` are running from the shared physical seed with their own writable 8 GiB leaves. The final bounded run used source helper SHA-256 `ce71c46c0c3be3d5ca241252295f59af8a4312d26f9f997c5df4c3427a4d9f4d` and the stable PARTUUID `260b98c7-b4f8-4c77-9fe7-67173c08dc90`, resolving to `/dev/sdb2`.

The final validation passed for each candidate: expected binary, AGENTS and four skill hashes; three protected paths denied file creation; all four writable private/persist paths accepted sentinels; neither candidate could see the other's sentinels; sentinels persisted through graceful stop, helper `seed-down`, default-PARTUUID `seed-up`, and Incus start while the peer stayed running; all candidates returned to Running. Guest read-only mount/property checks and host protected-bind source/inode checks passed. Incus exposed no NIC or proxy; `gre0`, `gretap0`, and `erspan0` were down without addresses, and there were no default or non-loopback routes. Sentinels were removed.

The first property-boundary probe found a real failure: guest root could change `/agent/rules` to `ro=false`. I restored it immediately and verified host and guest `ro=true`. After kernel-enforced protections were added, the parent separately verified that clearing the property after guest unmount is denied; the final validation confirmed the protected views and write-denial behavior. The earlier final-v6 run stopped because `i0`'s rules bind was intentionally unmounted for that boundary check, and the first v7 run stopped safely when i1's old nested seed-loop receipt no longer matched the physical source. The parent authorized a narrow repair: graceful stop, v7 `seed-down` against i1's recorded `/dev/loop2` mapping, then default-PARTUUID `seed-up`; no loop was manually detached. The repaired i1 mapping now points directly to `/dev/sdb2` read-only. The complete retry then passed.

The existing `op_network::btrfs::verify_dedicated_mount` kernel-ioctl verifier also returned PASS for both current leaf-image/volume/FSID triples using a temporary read-only harness. No mount or Cozo write was performed by that harness.

Evidence files:

- [`validate.sh`](./validate.sh) — bounded repeatable check; requires the exact frozen helper hash and uses a separate log per run.
- [`validation.log`](./validation.log) — original first mutation-probe failure and immediate RO-property restoration.
- [`validation-final-v6.log`](./validation-final-v6.log) — failed pre-repair run; no lifecycle operations.
- [`validation-final-v7.log`](./validation-final-v7.log) — first v7 run; i0 cycle passed, i1 failed closed on the old loop mapping, then recovery started i1.
- [`i1-repair-v7.log`](./i1-repair-v7.log) — exact preflight, graceful stop, safe helper repair, default-source reattachment, and start.
- [`validation-final-v7-repair1.log`](./validation-final-v7-repair1.log) — complete successful two-candidate retry.
- `/var/tmp/identity-sled-lifecycle-20260930/verify_dedicated_mount.log` — kernel ioctl verifier evidence.
- `/var/tmp/identity-sled-lifecycle-20260930/identity_sled_dispatch.log` — native GNU focused tests: 34 passed, 0 failed, 4 ignored.

No old identity records, archived identities, keys, grants, or Cozo state were changed. These checks validate candidate storage/runtime behavior only; they do not claim identity cutover or deployment completion.
