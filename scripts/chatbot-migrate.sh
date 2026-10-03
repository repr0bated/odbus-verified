#!/bin/bash
set -euo pipefail

# Chatbot Physical Root Migration Script
# This script guides you through migrating the chatbot from loop images to /dev/sdb2

CHATBOT_ID="b0c777e3-0955-cd8b-3d01-ca12998cae28"
PLAN_DIR="/var/lib/op-dbus/identity-physical-migration-plans"
PLAN_FILE="$PLAN_DIR/$CHATBOT_ID.json"

echo "=== Chatbot Physical Root Migration ==="
echo ""

# Step 1: Get preflight values
echo "STEP 1: Migration Preflight (Owner-Authenticated)"
echo "---"
echo "You must call this with authenticated D-Bus/gRPC access:"
echo "  identity_sled__inspect_own_physical_root_migration_preflight {}"
echo ""
echo "Paste the response values below:"
echo ""

read -p "expected_revision: " expected_revision
read -p "expected_revision_operation_id: " expected_revision_operation_id
read -p "previous_binding (paste JSON): " previous_binding
read -p "previous_root_repair_sha256: " previous_root_repair_sha256
read -p "previous_seed_provision_sha256: " previous_seed_provision_sha256

# Step 2: Gather system data
echo ""
echo "STEP 2: Gathering System Data"
echo "---"

echo "Getting seed generation..."
seed_generation=$(sudo btrfs filesystem show /dev/sdb2 2>/dev/null | grep -i generation | head -1 | awk '{print $NF}' | head -1)
[[ -z $seed_generation ]] && seed_generation="0"
echo "Seed generation: $seed_generation"

echo "Getting pool source..."
pool_source=$(sudo incus profile show default 2>/dev/null | jq -r '.devices.root.source // ""' 2>/dev/null)
[[ -z $pool_source ]] && pool_source="unknown"
echo "Pool source: $pool_source"

echo "Getting guest subvolume ID..."
guest_subvolume_id=$(sudo btrfs subvolume list /var/lib/opdbus-runtime/identity-leaves/ 2>/dev/null | grep "leaf.img" | awk '{print $2}' | head -1)
[[ -z $guest_subvolume_id ]] && guest_subvolume_id="0"
echo "Guest subvolume ID: $guest_subvolume_id"

echo "Getting Incus instance JSON..."
instance_json=$(sudo incus list -f json 2>/dev/null | jq ".[] | select(.name == \"$CHATBOT_ID\")" || echo "{}")

# Step 3: Create migration plan
echo ""
echo "STEP 3: Creating Migration Plan"
echo "---"

mkdir -p "$PLAN_DIR"

sudo tee "$PLAN_FILE" > /dev/null << EOF
{
  "version": 1,
  "session_id": "$CHATBOT_ID",
  "expected_revision": $expected_revision,
  "expected_revision_operation_id": "$expected_revision_operation_id",
  "previous_binding": $previous_binding,
  "previous_root_repair_sha256": "$previous_root_repair_sha256",
  "previous_seed_provision_sha256": "$previous_seed_provision_sha256",
  "binding": {
    "device_path": "/dev/sdb2",
    "mount_point": "/var/lib/opdbus-runtime/seed-identities/$CHATBOT_ID/volume",
    "btrfs_uuid": "bc95536f-1d38-46ad-b87e-2637c5c548ec",
    "cozo_id": "ec388ad3-607d-455c-ad0e-3dc4fe2fdbb4",
    "attached": true
  },
  "seed_device": "/dev/sdb2",
  "seed_fsid": "2dcac52c-fb0b-418a-95a4-cb6eacc80209",
  "seed_generation": $seed_generation,
  "leaf_inode": 122256063,
  "leaf_size": 8589934592,
  "guest_subvolume_id": $guest_subvolume_id,
  "native_root": "/var/lib/incus/containers/$CHATBOT_ID/rootfs",
  "pool_name": "3tched-storage",
  "pool_source": "$pool_source",
  "instance": $instance_json
}
EOF

if ! jq . "$PLAN_FILE" > /dev/null 2>&1; then
  die "Generated migration plan is not valid JSON; check system data gathering above"
fi

echo "Migration plan created at: $PLAN_FILE"

# Step 4: Compute SHA256
echo ""
echo "STEP 4: Computing Plan SHA256"
echo "---"

PLAN_SHA=$(sudo sha256sum "$PLAN_FILE" | cut -d' ' -f1)
echo "Plan SHA256: $PLAN_SHA"

# Step 5: Execute migration
echo ""
echo "STEP 5: Execute Migration (Owner-Authenticated)"
echo "---"
echo "You must call this with authenticated D-Bus/gRPC access:"
echo "  identity_sled__migrate_own_physical_root {\"plan_sha256\": \"$PLAN_SHA\"}"
echo ""

read -p "Press Enter once migration is complete..."

# Step 6: Validate
echo ""
echo "STEP 6: Validating Migration"
echo "---"

cd /srv/git/odbus-verified
cargo run --bin validate-chatbot-state -p op-cozo-store

# Step 7: Restart container
echo ""
echo "STEP 7: Restarting Chatbot Container"
echo "---"

read -p "Ready to restart chatbot container? (y/n) " -n 1 -r
echo
if [[ $REPLY =~ ^[Yy]$ ]]; then
    sudo incus restart "$CHATBOT_ID"
    echo "Container restart initiated..."
    sleep 3
fi

# Step 8: Verify
echo ""
echo "STEP 8: Verification"
echo "---"

echo "Bridge status:"
sudo sv status op-grpc-bridge || echo "Bridge status check failed"

echo ""
echo "Testing gRPC door:"
grpcurl -plaintext 10.0.0.2:8090 list 2>/dev/null | head -5 || echo "Door check failed (expected if bridge still starting)"

echo ""
echo "=== Migration Complete ==="
echo "Chatbot should now be using /dev/sdb2 as its seed device."
echo "Both human and chatbot identities should be operational."
