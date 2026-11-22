#!/usr/bin/env bash
# Bounded permanent-layout regressions. No Btrfs or host mutations.
set -euo pipefail
HERE=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
tmp=$(mktemp -d)
trap 'rm -rf -- "$tmp"' EXIT
export GOLDEN_HOST_MOUNT=$tmp/disk
export GOLDEN_WORK=$tmp/disk/@opdbus-system-a
export GOLDEN_ROOT=$tmp/disk/@upper
export GOLDEN_FSID=fixture GOLDEN_WORK_UUID=fixture GOLDEN_UPPER_UUID=fixture
GOLDEN_LIB=1 source "$HERE/../golden.sh"
resolve_layout

# A release must use the retained writable tree, not re-export HEAD into it.
mkdir -p "$WORK/opt/op-build.src" "$ROOT"
printf 'uncommitted source\n' >"$WORK/opt/op-build.src/keep"
before=$(sha256sum "$WORK/opt/op-build.src/keep")
APPLY=0
do_build >"$tmp/build-plan"
test "$before" = "$(sha256sum "$WORK/opt/op-build.src/keep")"
test -z "$(find "$WORK" -mindepth 1 -maxdepth 1 ! -name opt -print)"

# State follows immutable release paths; selecting rollback never renames
# an in-use lower directory and never deletes older snapshots.
mkdir -p "$ROOT/release-a/os" "$ROOT/release-b/os" "$ROOT/older/os"
printf '{"version":2,"live":{"path":"%s/release-a/os","id":"11","uuid":"a","commit":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"},"rollback":{"path":"%s/release-b/os","id":"12","uuid":"b","commit":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"}}\n' "$ROOT" "$ROOT" >"$STATE"
resolve_layout
test "$LIVE" = "$ROOT/release-a/os"
test "$ROLLBACK" = "$ROOT/release-b/os"
APPLY=1
# Rollback stages reversed selection; only the complete transaction may
# publish boot state. No mounted lower directory is renamed.
require_readonly_release() { test -d "$1"; }
require_lifecycle_lock() { :; }
deploy_candidate() { test "$(jq -r .live.path "$CANDIDATE_STATE")" = "$ROOT/release-b/os"; }
do_catalog_prepare() { :; } # Covered by the catalog-owner fixture.
do_rollback
test "$(jq -r .live.path "$STATE")" = "$ROOT/release-a/os"
test "$(jq -r .live.path "$CANDIDATE_STATE")" = "$ROOT/release-b/os"
test -d "$ROOT/release-a/os" && test -d "$ROOT/release-b/os" && test -d "$ROOT/older/os"

# An escaped selected lower must fail before mount/activation.
printf '{"live":{"path":"/var/lib/incus/storage-pools/3tched-pool/containers/victim"}}\n' >"$STATE"
if (resolve_layout) 2>/dev/null; then
    echo 'accepted an Incus-managed lower' >&2
    exit 1
fi
echo 'permanent-layout regressions passed'
