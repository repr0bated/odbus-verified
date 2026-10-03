# Chatbot Physical Root Migration (OD-98)

**Status**: Chatbot leaf subvolume live. No seeding. No chatbot loop.

## Model (owner-corrected)

- `/dev/sdb2` is the **writable leaf** (`leaf-root`), **not** a btrfs seed.
  - SEEDING cleared (`btrfstune -S 0`); `btrfs property ro=false`.
- The identity sled is a **subvolume** on that leaf:
  `@identity/b0c777e3-0955-cd8b-3d01-ca12998cae28` (id 262).
- Mount point:
  `/var/lib/opdbus-runtime/seed-identities/<session>/volume`
- Cozo `btrfs_device`:
  - `device_path`: `/dev/sdb2`
  - `mount_point`: the subvolume mount above
  - `btrfs_uuid`: `bc95536f-1d38-46ad-b87e-2637c5c548ec`
  - `attached`: `true`
- Lifecycle may `btrfs device add` extra members; never `RM_DEV` the sole leaf.
- No loop images for the chatbot (`fstorage.img` / `leaf.img` retired for this identity).

## Verified end state (2026-10-03)

1. Cozo `write_identity` committed mutation_index 3 with leaf binding.
2. Chatbot container mounts leaf `/agent` and `/opt/run-mounts/persist`.
3. Guest wrote `CHATBOT_SELF_VERIFY_OK` markers; host reads them on the leaf subvolume.
4. Superblock: label `leaf-root`, no SEEDING flag.

## Validate

```bash
# Inventory (authoritative metadata)
# identity_sled__inspect_identity_inventory {limit:10}
# expect chatbot mutation_index >= 3, device via get_identity {session_id}

cargo run --bin validate-chatbot-state -p op-cozo-store
# (requires IDENTITY_SLED_COZO_DB_PATH; bridge holds the lock while running)

incus exec b0c777e3-0955-cd8b-3d01-ca12998cae28 -- \
  cat /agent/user/.self-verify /opt/run-mounts/persist/.self-verify
```

## Do not

- Re-enable SEEDING on `/dev/sdb2`
- Remount chatbot persist via `/dev/loop*`
- Raw-edit identity-cozo while the bridge holds it
- Restart OVS / uplink / DHCP / Xray for this work
