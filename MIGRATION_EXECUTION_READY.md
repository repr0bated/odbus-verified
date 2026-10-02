# Chatbot Migration - Ready for Owner Execution

## Current State Verified
- Chatbot (b0c777e3-0955-cd8b-3d01-ca12998cae28) is on loop image
- Must migrate to /dev/sdb2 (FSID: bc95536f-1d38-46ad-b87e-2637c5c548ec)
- Bridge restarted (currently running on soft-fail)

## Data Gathered

### Seed Device
- Path: `/dev/sdb2`
- FSID: `bc95536f-1d38-46ad-b87e-2637c5c548ec`
- Label: `seed-root`
- Size: 100GB, used: 12.02GB

### Chatbot Leaf
- Path: `/var/lib/opdbus-runtime/identity-leaves/i0/leaf.img`
- Inode: `122256063`
- Size: `8589934592` bytes (8GB)

### Incus Instance
- Pool: `3tched-storage`
- Native root: `/var/lib/incus/containers/b0c777e3-0955-cd8b-3d01-ca12998cae28/rootfs`
- Status: Stopped (required for migration)

### Mount Point
- `/var/lib/opdbus-runtime/seed-identities/b0c777e3-0955-cd8b-3d01-ca12998cae28/volume`

### Cozo Owner Key
- `ec388ad3-607d-455c-ad0e-3dc4fe2fdbb4`

## Owner-Only Steps (Require Authentication)

### 1. Get Migration Preflight
Call with `cap.identity.sled.physical_root_migrate@v1` capability:
```
identity_sled__inspect_own_physical_root_migration_preflight {}
```

Response provides:
- `expected_revision`
- `expected_revision_operation_id`
- `previous_binding`
- `previous_root_repair_sha256`
- `previous_seed_provision_sha256`

### 2. Create Migration Plan
File location: `/var/lib/op-dbus/identity-physical-migration-plans/b0c777e3-0955-cd8b-3d01-ca12998cae28.json`

Use template from `CHATBOT_MIGRATION_PROCEDURE.md`, filling in:
- Preflight values (from step 1)
- `seed_generation` from `btrfs filesystem show /dev/sdb2`
- `guest_subvolume_id` from `btrfs subvolume list`
- `pool_source` from `incus profile show default`
- Full `instance` JSON from `incus list -f json`

### 3. Execute Migration
```bash
# Compute plan SHA256
sha256sum /var/lib/op-dbus/identity-physical-migration-plans/b0c777e3-0955-cd8b-3d01-ca12998cae28.json

# Call with authentication
identity_sled__migrate_own_physical_root {
  "plan_sha256": "<SHA256_FROM_ABOVE>"
}
```

## Validation (Post-Migration)

```bash
# Verify migration succeeded
cd /srv/git/odbus-verified && cargo run --bin validate-chatbot-state -p op-cozo-store

# Expected: All checks pass ✓
```

## Chatbot Restart

Once validation passes:
```bash
# Restart container
sudo incus restart b0c777e3-0955-cd8b-3d01-ca12998cae28

# Verify both identities operational
sudo sv status op-grpc-bridge
grpcurl -plaintext 10.0.0.2:8090 list
```

## Success Criteria

- ✅ Chatbot container starts without errors
- ✅ Tonic/door serves on 10.0.0.2:8090 without soft-fail
- ✅ Both human and chatbot identities operational
- ✅ `validate-chatbot-state` passes all checks
- ✅ Chatbot self-validates on startup

## Supporting Documentation

- `CHATBOT_MIGRATION_PROCEDURE.md` — comprehensive step-by-step guide
- `crates/op-cozo-store/src/bin/list-genesis.rs` — inspect current state
- `crates/op-cozo-store/src/bin/validate-chatbot-state.rs` — post-migration validation
- `crates/op-cozo-store/src/bin/migrate-chatbot-root.rs` — procedural guidance utility

## Summary

**All non-authenticated preparation is complete.** System data has been gathered and verified. Migration plan template is ready. The remaining steps require owner-authenticated D-Bus/gRPC access with the physical-root-migrate capability.
