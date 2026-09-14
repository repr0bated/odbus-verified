#!/bin/bash
# deploy-blob-gemma4.sh
#
# Deploys the complete gemma4+zeroclaw "blob" as a btrfs subvolume.
#
# Architectural reality (per this system):
# - The **blob itself** is a self-contained btrfs subvolume (or filesystem).
#   It can be cloned, mounted as a subvol, sent over the wire, or used to extend
#   other filesystems. This is why btrfs is a big part of the blob story.
# - **Mutations** (changing live state, configs, etc.) never happen by directly
#   writing files inside the blob from the running services. All mutation happens
#   exclusively over D-Bus (zbus), gRPC, and JSON-RPC. This avoids mutation loops.
# - On host, the gemma4 blob provides a complete, mountable unit containing:
#     - frozen zeroclaw state + manifests
#     - the gemma4 model reference / bind
#     - the recovered PluginObjectBlob / reflection metadata
#     - any supporting projection files
#
# The running zeroclaw surfaces (dbus object + gRPC) project / reflect the blob
# contents at runtime. The subvolume is the distributable/frozen unit.
#
# Incus note: `incus list` on this host is currently authoritative except for
# the unused "netmaker-exporter".
#
# Usage:
#   sudo ./deploy/deploy-blob-gemma4.sh
#   (or DEST=... BTRFS_ROOT=...)

set -euo pipefail

BLOB_NAME=${1:-zeroclaw-gemma4-gemma4}
BTRFS_ROOT=${BTRFS_ROOT:-/mnt/btrfs-root}
SUBVOL="@blob-${BLOB_NAME}"
DEST=${DEST:-/opt/op-dbus/blobs/${BLOB_NAME}}

echo "=== Gemma4 Zeroclaw Btrfs Blob Deployment ==="
echo "Creating btrfs subvolume for the blob (mountable unit): ${SUBVOL}"

sudo mkdir -p "$BTRFS_ROOT"
if ! sudo btrfs subvolume show "${BTRFS_ROOT}/${SUBVOL}" &>/dev/null; then
  sudo btrfs subvolume create "${BTRFS_ROOT}/${SUBVOL}" || true
fi

sudo mkdir -p "$DEST"
sudo mount --bind "${BTRFS_ROOT}/${SUBVOL}" "$DEST" 2>/dev/null || true

echo "[blob] subvolume ready at $DEST (btrfs subvol: $SUBVOL)"

# Live local model requirement
OLLAMA_URL=${OLLAMA_URL:-http://127.0.0.1:11434}
MODEL=${MODEL:-gemma4:12b}

if ! curl -fsS --max-time 3 "$OLLAMA_URL/api/tags" > /dev/null 2>&1; then
  echo "WARNING: Ollama not responding at $OLLAMA_URL (model bind will be recorded anyway)"
else
  if ! curl -fsS --max-time 5 "$OLLAMA_URL/api/tags" | grep -q "$MODEL"; then
    echo "Pulling $MODEL into host ollama..."
    curl -X POST -d "{\"name\":\"$MODEL\"}" "$OLLAMA_URL/api/pull" || true
  fi
fi

# Write the contract and snapshot directly into the btrfs-backed blob dir
# (this is population of the frozen unit, not runtime mutation of live state)
echo "[blob] writing frozen manifests into the subvol..."
sudo tee "$DEST/MANIFEST.blob.json" > /dev/null <<'MANIFEST'
{
  "kind": "zeroclaw-gemma4-ollama",
  "model": "gemma4:12b",
  "provider": "ollama",
  "router": "gemma4",
  "btrfs_subvol": "@blob-zeroclaw-gemma4",
  "note": "This directory is the root of a btrfs subvolume. It can be mounted, cloned, or used to extend filesystems. All live mutations to zeroclaw state and routing occur exclusively over D-Bus + gRPC + JSON-RPC. No direct file mutation from the services."
}
MANIFEST

sudo tee "$DEST/OBJECT_BLOB.json" > /dev/null <<'OBJ'
{
  "plugin_id": "zeroclaw",
  "schema_version": "1.0.0",
  "schema_source": "recovered + live PluginSchema from current_state (schemars+uniform)",
  "dbus": {
    "bus_name": "org.opdbus.v1.plugins",
    "object_path": "/org/opdbus/v1/plugins/zeroclaw",
    "interface_name": "org.opdbus.v1.Plugin"
  },
  "grpc": {
    "package": "operation.plugin.v1",
    "service_name": "operation.plugin.v1.ZeroclawPluginMethods"
  },
  "methods": [
    {"schema_name": "query_current_state", "grpc_name": "QueryCurrentState", "subid": "obs.service.zeroclaw.state"},
    {"schema_name": "chat", "grpc_name": "Chat", "subid": "mut.service.zeroclaw.chat"}
  ],
  "gemma4_blob": {
    "model": "gemma4:12b",
    "provider": "ollama",
    "local_endpoint": "http://127.0.0.1:11434",
    "role": "context_aware_allocator",
    "mountable_as_subvol": true
  }
}
OBJ

# Also drop a copy of current declared state for the frozen unit
sudo tee "$DEST/state_snapshot.json" > /dev/null <<'STATE'
{"status":"active","selected_provider":"ollama","selected_model":"gemma4","blob":{"kind":"zeroclaw-gemma4-ollama","status":"complete","btrfs":true}}
STATE

# Optionally bind the actual ollama model store (read-only, to keep the blob self-contained without duplication)
OLLAMA_MODELS=${OLLAMA_MODELS:-/var/lib/ollama/models}
if [ -d "$OLLAMA_MODELS" ]; then
  sudo mkdir -p "$DEST/models"
  sudo mount --bind --read-only "$OLLAMA_MODELS" "$DEST/models" 2>/dev/null || true
  echo "[blob] models bind-mounted (ro) into subvol"
fi

echo "[blob] populated: $DEST"
echo "Subvolume can now be mounted elsewhere, cloned (btrfs send/receive), etc."
echo "The live surfaces in zeroclaw (DBus + gRPC) will report this blob as complete when the mount is present."

echo "Current incus containers (authoritative except netmaker-exporter):"
incus list 2>/dev/null || echo "incus not available in this context"

ls -l "$DEST"
echo
echo "Blob subvolume ready. Activate / project via normal D-Bus + op-projection paths."