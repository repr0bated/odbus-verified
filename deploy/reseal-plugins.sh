#!/usr/bin/env bash
# reseal-plugins.sh
#
# Rebuilds opblob + op-grpc-bridge from the current checkout, reseals the SHM
# plugin blob catalog, and restarts op-grpc-bridge so the new catalog is
# picked up.
#
# Exists because of a real incident (2026-08-08): `opblob seal-shm` was run
# on a deployed checkout that was behind origin/main by one commit (the
# antigravity schema change) and had uncommitted local edits. It silently
# resealed the *old* schema — nothing in the pipeline caught the drift, so
# the reseal looked like it worked but changed nothing. This script makes
# that failure mode impossible to hit silently: it refuses to build+seal
# unless the working tree is clean and HEAD already contains origin/main's
# tip.
#
# Usage:
#   ./deploy/reseal-plugins.sh              verify clean + merged with origin/main, then build+seal+restart
#   ./deploy/reseal-plugins.sh --force       skip the dirty/behind-main checks
#   NO_RESTART=1 ./deploy/reseal-plugins.sh  seal only; leave the running op-grpc-bridge alone

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

FORCE=0
[[ "${1:-}" == "--force" ]] && FORCE=1

if [[ "$FORCE" -ne 1 ]]; then
  # Tracked-file changes only (staged or unstaged) — untracked scratch files
  # (spec drafts, examples, etc.) are routine and not what caused the
  # incident, so they don't block a reseal.
  if ! git diff --quiet HEAD --; then
    echo "reseal-plugins: refusing to seal — tracked files have uncommitted changes." >&2
    echo "Commit or stash first, or pass --force to override." >&2
    exit 1
  fi

  echo "reseal-plugins: fetching origin/main to check for drift..."
  git fetch origin main --quiet

  if ! git merge-base --is-ancestor origin/main HEAD; then
    echo "reseal-plugins: refusing to seal — HEAD does not contain origin/main" \
         "($(git rev-parse --short origin/main))." >&2
    echo "Merge or rebase origin/main first, or pass --force to override." >&2
    exit 1
  fi
else
  echo "reseal-plugins: --force set, skipping dirty/behind-main checks." >&2
fi

echo "reseal-plugins: building opblob + op-grpc-bridge (release)..."
cargo build --release -p op-grpc-bridge -p op-plugins --bin op-grpc-bridge --bin opblob

echo "reseal-plugins: sealing SHM plugin blob catalog..."
sudo "$REPO_ROOT/target/release/opblob" seal-shm

if [[ "${NO_RESTART:-0}" != "1" ]]; then
  echo "reseal-plugins: installing binaries (rename, not truncate — avoids ETXTBSY on the running process)..."
  sudo cp "$REPO_ROOT/target/release/op-grpc-bridge" /usr/local/bin/op-grpc-bridge.new
  sudo mv /usr/local/bin/op-grpc-bridge.new /usr/local/bin/op-grpc-bridge
  sudo cp "$REPO_ROOT/target/release/opblob" /usr/local/bin/opblob.new
  sudo mv /usr/local/bin/opblob.new /usr/local/bin/opblob

  echo "reseal-plugins: restarting op-grpc-bridge..."
  sudo sv restart op-grpc-bridge
  sleep 2
  sudo sv status op-grpc-bridge
else
  echo "reseal-plugins: NO_RESTART=1 set — blob catalog resealed, running op-grpc-bridge left untouched."
  echo "Note: the D-Bus/gRPC-reflection surfaces already resync reactively on next arrival" \
       "(see schema_router.rs / dynamic_reflection.rs); only the frozen per-method gRPC" \
       "service descriptors require the restart above to pick up new method signatures."
fi

echo "reseal-plugins: done."
