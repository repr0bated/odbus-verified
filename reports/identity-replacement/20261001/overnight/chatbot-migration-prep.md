# Chatbot physical-root migration — prepared notes (not applied)

Do **not** apply until the owner reconciles the seed filesystem.

## Live facts (2026-10-02T00:10Z)

| Item | Value |
|------|-------|
| Chatbot session | `b0c777e3-0955-cd8b-3d01-ca12998cae28` |
| Cozo device_path (failing) | `/var/lib/opdbus-runtime/identity-leaves/i0/leaf.img` |
| Soft-fail error | no loop device to add |
| Live `/dev/sdb2` UUID | `bc95536f-1d38-46ad-b87e-2637c5c548ec` |
| Live `/dev/sdb2` PARTUUID | `911b270a-a88f-48cd-bf9c-9dcac4cdefe8` |
| Documented seed FSID (stale?) | `2dcac52c-fb0b-418a-95a4-cb6eacc80209` |
| Documented PARTUUID (stale?) | `260b98c7-b4f8-4c77-9fe7-67173c08dc90` |
| Seed mount | `/mnt` not a btrfs mount of sdb2 |

## Required before migrate_own_physical_root

1. Owner confirms which device/FSID is the RO seed.
2. Mount that seed `-o ro` at the path that will be Cozo `mount_point`.
3. Write a root-owned plan under
   `/var/lib/op-dbus/identity-physical-migration-plans/b0c777e3-0955-cd8b-3d01-ca12998cae28.json`
   with SHA256 pin (see
   `reports/identity-replacement/20260930/cozo-reconciliation/receipt.md`).
4. Call `inspect_own_physical_root_migration_preflight` then
   `migrate_own_physical_root` with exact capability
   `cap.identity.sled.physical_root_migrate@v1`.
5. Do **not** raw-edit identity-cozo. Do **not** rewrite `/dev/sdb2`.

## Intended Cozo binding (once seed confirmed)

```json
{
  "device_path": "/dev/sdb2",
  "mount_point": "<confirmed RO seed mount>",
  "btrfs_uuid": "<FSID of that mounted seed>",
  "cozo_id": "<unchanged>",
  "attached": false
}
```
