# Chatbot Physical Root Migration (OD-98)

**Status**: Migration helper utilities ready. Procedure awaits execution by owner.

## Problem
Chatbot identity is pointing to loop images (`/var/lib/opdbus-runtime/identity-leaves/i0/leaf.img`) instead of the shared physical seed device (`/dev/sdb2`, FSID `bc95536f-1d38-46ad-b87e-2637c5c548ec`). This prevents:
- Chatbot container from starting properly
- Door/tonic from serving both identities without soft-fail
- System from being self-validating

## Solution: Atomic Migration

This follows the official procedure from `reports/identity-replacement/20260930/cozo-reconciliation/receipt.md`.

### Step 1: List Current State

```bash
cd /srv/git/odbus-verified
cargo run --bin list-genesis -p op-cozo-store
```

**Expected output:**
- Human identity (`414fbc56...`): currently on loop image
- Chatbot identity (`b0c777e3...`): currently on loop image at `leaf.img`

### Step 2: Get Migration Preflight

The chatbot migration requires authenticated D-Bus access and proper capability. Call:

```
identity_sled__inspect_own_physical_root_migration_preflight {}
```

This requires:
- Authenticated D-Bus connection through `/run/opdbus/lungs.sock`
- Or SID1 credential through gRPC door
- Capability: `cap.identity.sled.physical_root_migrate@v1`

**Preflight response will give:**
- `session_id`: `b0c777e3-0955-cd8b-3d01-ca12998cae28`
- `expected_revision`: <current revision>
- `expected_revision_operation_id`: <operation ID>
- `previous_binding`: <old binding info>
- `previous_root_repair_sha256`: <hash>
- `previous_seed_provision_sha256`: <hash>

### Step 3: Prepare Migration Plan

Create `/var/lib/op-dbus/identity-physical-migration-plans/b0c777e3-0955-cd8b-3d01-ca12998cae28.json` with:

```json
{
  "version": 1,
  "session_id": "b0c777e3-0955-cd8b-3d01-ca12998cae28",
  "expected_revision": <from preflight>,
  "expected_revision_operation_id": <from preflight>,
  "previous_binding": <from preflight>,
  "previous_root_repair_sha256": <from preflight>,
  "previous_seed_provision_sha256": <from preflight>,
  "binding": {
    "device_path": "/dev/sdb2",
    "mount_point": "/var/lib/opdbus-runtime/seed-identities/b0c777e3-0955-cd8b-3d01-ca12998cae28/volume",
    "btrfs_uuid": "bc95536f-1d38-46ad-b87e-2637c5c548ec",
    "cozo_id": "ec388ad3-607d-455c-ad0e-3dc4fe2fdbb4",
    "attached": true
  },
  "seed_device": "/dev/sdb2",
  "seed_fsid": "2dcac52c-fb0b-418a-95a4-cb6eacc80209",
  "seed_generation": <from btrfs inspect-internal>,
  "leaf_inode": <from ls -i on leaf>,
  "leaf_size": <from stat on leaf>,
  "guest_subvolume_id": <from btrfs inspect-internal>,
  "native_root": "/var/lib/incus/containers/b0c777e3-0955-cd8b-3d01-ca12998cae28/rootfs",
  "pool_name": "3tched-storage",
  "pool_source": <from incus profile show>,
  "instance": <complete stopped Incus instance JSON>
}
```

**Required gathering:**

```bash
# Seed device info
btrfs filesystem show /dev/sdb2
btrfs filesystem df /dev/sdb2

# Leaf info
stat /var/lib/opdbus-runtime/identity-leaves/i0/leaf.img
ls -i /var/lib/opdbus-runtime/identity-leaves/i0/leaf.img

# Guest subvolume
btrfs subvolume show /var/lib/opdbus-runtime/identity-leaves/i0/leaf.img

# Incus instance
incus list -f json | jq '.[] | select(.name == "b0c777e3-0955-cd8b-3d01-ca12998cae28")'

# Pool source
incus profile show default | jq '.devices.root'
```

Compute SHA256:
```bash
sha256sum /var/lib/op-dbus/identity-physical-migration-plans/b0c777e3-0955-cd8b-3d01-ca12998cae28.json
```

### Step 4: Execute Migration

Call (via D-Bus or gRPC):

```
identity_sled__migrate_own_physical_root {
  "plan_sha256": "<SHA256 from step 3>"
}
```

This atomic transaction:
1. Verifies plan SHA256
2. Checks current revision and journals
3. Archives old bindings and provenance
4. Records new physical provenance
5. Advances revision atomically
6. **Preserves:** credentials, SID1, genesis, actor class

### Step 5: Verify Migration

```bash
# List again to confirm binding changed
cargo run --bin list-genesis -p op-cozo-store

# Expected: chatbot now shows device_path=/dev/sdb2 and correct FSID

# Run self-validation checks
cargo run --bin validate-chatbot-state -p op-cozo-store

# Expected: All checks pass ✓
```

### Step 6: Start Chatbot and Validate

```bash
# Restart the chatbot container
incus restart b0c777e3-0955-cd8b-3d01-ca12998cae28

# Verify it started
incus list | grep b0c777e3

# Check tonic is serving both identities
sudo sv status op-grpc-bridge

# Test door connectivity
grpcurl -plaintext 10.0.0.2:8090 list
```

### Success Criteria

- ✅ Chatbot container starts without errors
- ✅ Tonic/door serves on 10.0.0.2:8090 without soft-fail
- ✅ Both human and chatbot identities are operational
- ✅ `list-genesis` shows chatbot on `/dev/sdb2` with correct FSID
- ✅ System graph reflects updated state
- ✅ Chatbot can self-validate

## Rollback

If migration fails:

1. Stop chatbot container
2. Contact owner (migration is atomic at Cozo level)
3. Historical journals are preserved for diagnosis
4. Original loop images remain in place (attached=true logs the device)

## Troubleshooting

**Migration rejects plan:**
- Verify SHA256 matches exactly
- Verify revision hasn't changed since preflight
- Check journals are complete

**Container won't start:**
- Verify Btrfs device is readable: `btrfs filesystem show /dev/sdb2`
- Check mount point permissions
- Review Incus logs: `incus info b0c777e3...`

**Door still soft-fails:**
- Verify both identity records are committed
- Check mutation_engine startup logs
- May require bridge restart after Cozo update

## References

- Migration receipt: `reports/identity-replacement/20260930/cozo-reconciliation/receipt.md`
- Identity architecture: `docs/src/architecture/identity-transport.md`
- Snowball chain verification: `crates/op-plugins/src/state_plugins/snowball_plugin.rs`
