#!/bin/sh
# release-bridge.sh — one-command bridge release, in the only safe order:
# pull -> checkout -> build -> seal -> activate (snapshot/send-receive/rotate).
#
#   sudo ./release-bridge.sh
#
# Overrides: REPO=/srv/git/odbus-verified BRANCH=rewrite-fresh-bridge
#
# Order matters and is not negotiable:
# - cargo build never reads SHM, so build first.
# - opblob seal-shm/persist writes the catalog the new bridge will consume.
# - activate-golden.sh prepares golden, sends/receives into the inactive
#   slot, rotates, and keeps active+previous (previous = rollback).
# The bridge is never started before the seal, and binaries are never
# hand-copied onto the running host.
set -eu
PATH=/usr/sbin:/usr/bin:/sbin:/bin
export PATH

[ "$(id -u)" = 0 ] || { echo "release-bridge: run as root: sudo $0" >&2; exit 1; }
OWNER=${SUDO_USER:-jeremy}
BRANCH=${BRANCH:-rewrite-fresh-bridge}

# --- locate the repo -------------------------------------------------------
REPO=${REPO:-}
if [ -z "$REPO" ]; then
    for candidate in /srv/git/odbus-verified /home/$OWNER/odbus-verified /home/$OWNER/odbus-combined; do
        if [ -d "$candidate/.git" ] || [ -f "$candidate/.git" ]; then REPO=$candidate; break; fi
    done
fi
[ -n "$REPO" ] && [ -d "$REPO" ] || { echo "release-bridge: repo not found; set REPO=<path>" >&2; exit 1; }
echo "release-bridge: repo=$REPO branch=$BRANCH owner=$OWNER"

as_owner() {
    # Run a command as the repo owner (root-owned git in a user repo trips
    # safe.directory and creates root-owned build artifacts).
    runuser -u "$OWNER" -- env HOME="/home/$OWNER" \
        PATH="/home/$OWNER/.cargo/bin:/usr/local/bin:/usr/bin:/bin" "$@"
}

# --- 1. pull + checkout ----------------------------------------------------
echo "== fetch =="
as_owner git -C "$REPO" fetch origin --prune
if [ -n "$(as_owner git -C "$REPO" status --porcelain)" ]; then
    echo "release-bridge: working tree is dirty; refusing to checkout over it:" >&2
    as_owner git -C "$REPO" status --short >&2
    exit 1
fi
echo "== checkout $BRANCH =="
if as_owner git -C "$REPO" show-ref --verify --quiet "refs/remotes/origin/$BRANCH"; then
    as_owner git -C "$REPO" checkout -B "$BRANCH" "origin/$BRANCH"
elif as_owner git -C "$REPO" show-ref --verify --quiet "refs/heads/$BRANCH"; then
    as_owner git -C "$REPO" checkout "$BRANCH"
    echo "release-bridge: NOTE: $BRANCH is local-only (not on origin); releasing local tip" >&2
else
    echo "release-bridge: branch $BRANCH not found locally or on origin" >&2
    exit 1
fi
as_owner git -C "$REPO" log --oneline -1

# --- 2. build (compile only; never reads SHM) ------------------------------
echo "== cargo build --workspace --release =="
as_owner sh -c "cd '$REPO' && CXXFLAGS='-include cstdint' cargo build --workspace --release"

# --- 3. seal with the freshly built opblob ---------------------------------
echo "== opblob seal-shm + persist =="
cd "$REPO"
COGNITIVE_QDRANT_URL=${COGNITIVE_QDRANT_URL:-http://127.0.0.1:6334} \
    target/release/opblob seal-shm
target/release/opblob persist

# --- 4. activate: golden -> snapshot -> send/receive -> rotate -------------
echo "== activate-golden.sh =="
./activate-golden.sh

# --- 5. verify + rollback handle -------------------------------------------
echo "== bridge status =="
sleep 5
sv status op-grpc-bridge || true
echo
echo "release-bridge: done. Current release is live; previous kept as rollback."
echo "Undo:  sudo /usr/local/libexec/opdbus-release/op-release-launch rollback"
