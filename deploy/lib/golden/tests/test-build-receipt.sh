#!/bin/bash
set -euo pipefail
here=$(cd -- "$(dirname -- "$0")" && pwd)
tmp=$(mktemp -d); trap 'rm -rf "$tmp"' EXIT
export GOLDEN_HOST_MOUNT=$tmp/disk GOLDEN_WORK=$tmp/disk/@opdbus-system-a GOLDEN_ROOT=$tmp/disk/@upper GOLDEN_FSID=fixture GOLDEN_WORK_UUID=fixture GOLDEN_UPPER_UUID=fixture
GOLDEN_LIB=1 source "$here/../golden.sh"
resolve_layout
mkdir -p "$REPO" "$WORK$NATIVE_ARTIFACT_DIR" "$WORK/opt/op-build" "$WORK/usr/share/op-dbus/plugin-blobs" "$ROOT"
printf 'retained dirty source\n' >"$REPO/source.rs"
for binary in op-grpc-bridge 3tched-fs opblob; do printf '#!/bin/sh\nexit 0\n' >"$WORK$NATIVE_ARTIFACT_DIR/$binary"; chmod 0755 "$WORK$NATIVE_ARTIFACT_DIR/$binary"; done
digest=$(source_digest "$WORK")
commit=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
record_build_receipt "$digest" "$commit"
printf '{"plugins":{}}\n' >"$WORK/usr/share/op-dbus/plugin-blobs/.manifest.json"
catsha=$(sha256sum "$WORK/usr/share/op-dbus/plugin-blobs/.manifest.json" | cut -d' ' -f1)
jq -n --arg commit "$commit" --arg digest "$digest" --arg hash "$catsha" '{exit:0,source_commit:$commit,source_digest:$digest,catalog_manifest_sha256:$hash}' >"$WORK/opt/op-build/catalog-seal-receipt.json"
verify_release_inputs "$WORK"
printf 'changed\n' >>"$REPO/source.rs"
if (verify_release_inputs "$WORK") 2>/dev/null; then echo 'accepted changed source' >&2; exit 1; fi
printf 'retained dirty source\n' >"$REPO/source.rs"
printf 'stale\n' >>"$WORK$NATIVE_ARTIFACT_DIR/op-grpc-bridge"
if (verify_release_inputs "$WORK") 2>/dev/null; then echo 'accepted stale artifact' >&2; exit 1; fi
# A caller-supplied pair must be preserved and verified together.
mkdir -p "$WORK/opt/custom"
printf 'capture\n' >"$WORK/opt/custom/threetched"; chmod 0755 "$WORK/opt/custom/threetched"
sha=$(sha256sum "$WORK/opt/custom/threetched" | cut -d' ' -f1)
printf '{"artifact":{"sha256":"%s"}}\n' "$sha" >"$WORK/opt/custom/manifest.json"
THREETCHED_CAPTURE_BIN=/opt/custom/threetched THREETCHED_CAPTURE_MANIFEST=/opt/custom/manifest.json capture_pair
[[ $CAPTURE_BIN == /opt/custom/threetched && $CAPTURE_MANIFEST == /opt/custom/manifest.json ]]
if (THREETCHED_CAPTURE_BIN=/opt/custom/threetched THREETCHED_CAPTURE_MANIFEST= capture_pair) 2>/dev/null; then echo 'accepted unpaired capture' >&2; exit 1; fi
# The actual existing lifecycle lock is held, rather than a second dispatcher.
exec 9>"$ROOT/.golden.lock"; flock -n 9; require_lifecycle_lock
APPLY=1 WITH_BUILD=0
assert_no_cargo() { die 'fixture Cargo owner is active'; }
if (do_cut) 2>/dev/null; then echo 'cut accepted active Cargo' >&2; exit 1; fi
[[ ! -f $CANDIDATE_STATE ]]
echo 'provenance/capture/lock regressions passed'
