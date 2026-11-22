#!/bin/bash
# Production shell owner with controlled opblob/Btrfs boundaries; no host catalogs.
set -euo pipefail
here=$(cd -- "$(dirname -- "$0")" && pwd)
h=$(mktemp -d); trap 'rm -rf "$h"' EXIT
export GOLDEN_HOST_MOUNT=$h/disk GOLDEN_WORK=$h/disk/@opdbus-system-a GOLDEN_ROOT=$h/root GOLDEN_FSID=fixture GOLDEN_WORK_UUID=fixture GOLDEN_UPPER_UUID=fixture
GOLDEN_LIB=1 source "$here/../golden.sh"
ROOT=$h/root; STATE=$ROOT/golden-state.json; CANDIDATE_STATE=$ROOT/golden-candidate.json
LIVE=$ROOT/new/os; TXN=$ROOT/golden-transition.fixture
CATALOG_LIVE=$h/live; CATALOG_PERSISTED=$h/persisted
mkdir -p "$ROOT" "$TXN" "$LIVE$NATIVE_ARTIFACT_DIR" "$LIVE/opt/op-build" "$LIVE/usr/share/op-dbus/plugin-blobs" "$CATALOG_LIVE" "$CATALOG_PERSISTED"
printf '{"live":{"path":"old"}}\n' >"$STATE"
printf '{"live":{"path":"%s"}}\n' "$LIVE" >"$CANDIDATE_STATE"
for d in "$CATALOG_LIVE" "$CATALOG_PERSISTED"; do printf 'old-manifest\n' >"$d/.manifest.json"; printf 'old-blob\n' >"$d/old.blob"; done
printf 'new-manifest\n' >"$LIVE/usr/share/op-dbus/plugin-blobs/.manifest.json"
printf 'new-blob\n' >"$LIVE/usr/share/op-dbus/plugin-blobs/new.blob"
sha=$(act_sha "$LIVE/usr/share/op-dbus/plugin-blobs/.manifest.json")
printf '{"catalog_manifest_sha256":"%s"}\n' "$sha" >"$LIVE/opt/op-build/catalog-seal-receipt.json"
printf '{"source_digest":"%064d"}\n' 1 >"$LIVE/opt/op-build/execution-receipt.json"
cat >"$LIVE$NATIVE_ARTIFACT_DIR/opblob" <<'PUBLISHER'
#!/bin/bash
# Boundary double only: production performs renameat2(RENAME_EXCHANGE).
set -euo pipefail
[[ $1 == persist && -d ${OPDBUS_RELEASE_CATALOG_STAGE:?} ]]
source=$2 destination=$3 stage=$OPDBUS_RELEASE_CATALOG_STAGE
cp "$source/.manifest.json" "$stage/.manifest.json"
for f in "$source"/*.blob; do [[ ! -e $f ]] || cp "$f" "$stage/"; done
[[ ${GOLDEN_TEST_PUBLISHER_FAULT:-} != before ]] || exit 71
if [[ -d $destination ]]; then
  mv "$destination" "$destination.exchanged"
  mv "$stage" "$destination"
  mv "$destination.exchanged" "$stage"
else mv "$stage" "$destination"; fi
[[ ${GOLDEN_TEST_PUBLISHER_FAULT:-} != after ]] || exit 72
PUBLISHER
chmod +x "$LIVE$NATIVE_ARTIFACT_DIR/opblob"
# Only Btrfs metadata and release admission are controlled. Hashes, file maps,
# durable intents, inode reconciliation, before selection and restoration are real.
btrfs() {
  case "$1 $2" in
    'subvolume create') mkdir "$3";;
    'subvolume show') printf 'UUID: 11111111-1111-1111-1111-111111111111\n';;
    'property set') printf 'ro=true\n' >"$h/ro";;
    'property get') cat "$h/ro";;
    *) return 1;;
  esac
}
require_lifecycle_lock() { :; }
assert_no_cargo() { :; }
require_readonly_release() { :; }
verify_release_inputs() { :; }
APPLY=1
do_catalog_prepare
cp "$ROOT/golden-catalog-before.json" "$TXN/catalog-selection.json"
cp "$STATE" "$TXN/state.before"; touch "$TXN/catalog-attempted"
tx_validate_catalog
old_files=$(catalog_files "$CATALOG_LIVE")
new_files=$(catalog_files "$LIVE/usr/share/op-dbus/plugin-blobs")
tx_catalog_publish
[[ $(catalog_files "$CATALOG_LIVE") == "$new_files" && ! -e $CATALOG_LIVE/old.blob ]]
tx_catalog_restore
[[ $(catalog_files "$CATALOG_LIVE") == "$old_files" && $(catalog_files "$CATALOG_PERSISTED") == "$old_files" && ! -e $CATALOG_LIVE/new.blob ]]
echo 'ok complete before capture, exact publication/removal of obsolete plugin, and old live+persistent restoration'
export GOLDEN_TEST_PUBLISHER_FAULT=before
if catalog_copy "$LIVE/usr/share/op-dbus/plugin-blobs" "$CATALOG_LIVE" before-error; then exit 1; fi
[[ $(catalog_files "$CATALOG_LIVE") == "$old_files" ]]
echo 'ok pre-exchange failure retains original destination'
export GOLDEN_TEST_PUBLISHER_FAULT=after
catalog_copy "$LIVE/usr/share/op-dbus/plugin-blobs" "$CATALOG_LIVE" after-error
[[ $(catalog_files "$CATALOG_LIVE") == "$new_files" ]]
jq -se 'any(.[]; .effect=="published" and .publisher_exit==72)' "$TXN"/catalog-operations/*/result.json >/dev/null
echo 'ok applied-but-error publisher reconciled by directory identities and complete file hashes'
unset GOLDEN_TEST_PUBLISHER_FAULT
# Run the REAL aggregate owner and REAL service-byte restore. The supervisor
# double rejects any old restart before BOTH exact old catalogs are restored.
mkdir "$TXN/service-0" "$h/enabled"
printf 'old-service\n' >"$TXN/service-0/binary"; chmod 0755 "$TXN/service-0/binary"
printf 'new-service\n' >"$h/service"
printf 'fixture\n' >"$TXN/service-0/service"
printf '%s\n' "$h/service" >"$TXN/service-0/path"
printf 'stable:0\n' >"$TXN/service-0/health"
ACT_LEDGER=$h/ledger; printf 'old-ledger\n' >"$TXN/ledger.before"; printf 'new-ledger\n' >"$ACT_LEDGER"
ACT_ENABLED_DIR=$h/enabled; ACT_SV=$h/sv
export GOLDEN_TEST_BEFORE=$CATALOG_BEFORE_DATA GOLDEN_TEST_LIVE=$CATALOG_LIVE GOLDEN_TEST_PERSISTED=$CATALOG_PERSISTED GOLDEN_TEST_SERVICE=$h/service GOLDEN_TEST_RESTARTS=$h/restarts
cat >"$ACT_SV" <<'SUPERVISOR'
#!/bin/bash
set -euo pipefail
[[ $1 != -w ]] || shift 2
case $1 in
  down) exit 0 ;;
  status) echo "run: fixture: (pid 42) 10s" ;;
  restart)
    cmp "$GOLDEN_TEST_BEFORE/live/.manifest.json" "$GOLDEN_TEST_LIVE/.manifest.json"
    cmp "$GOLDEN_TEST_BEFORE/live/old.blob" "$GOLDEN_TEST_LIVE/old.blob"
    cmp "$GOLDEN_TEST_BEFORE/persisted/.manifest.json" "$GOLDEN_TEST_PERSISTED/.manifest.json"
    cmp "$GOLDEN_TEST_BEFORE/persisted/old.blob" "$GOLDEN_TEST_PERSISTED/old.blob"
    [[ $(cat "$GOLDEN_TEST_SERVICE") == old-service ]]
    echo restart >>"$GOLDEN_TEST_RESTARTS" ;;
  *) exit 1 ;;
esac
SUPERVISOR
chmod +x "$ACT_SV"
tx_restore
[[ $(cat "$h/service") == old-service && $(cat "$ACT_LEDGER") == old-ledger && $(wc -l <"$h/restarts") == 1 ]]
[[ $(jq -r .phase "$ROOT/golden-transaction.json") == rolled-back ]]
echo 'ok real aggregate restore publishes complete old catalogs before restoring/restarting old service'
printf 'unrelated\n' >"$CATALOG_PERSISTED/unrelated.blob"
if tx_catalog_restore; then exit 1; fi
[[ -f $CATALOG_PERSISTED/unrelated.blob ]]
echo 'ok unknown concurrent catalog rejected without overwrite'
rm "$CATALOG_PERSISTED/unrelated.blob"
printf 'tampered\n' >"$CATALOG_BEFORE_DATA/live/old.blob"
if catalog_select_before "$ROOT/golden-catalog-before.json" "$STATE"; then exit 1; fi
echo 'ok retained blob tampering rejected even with unchanged manifest'
echo 'catalog owner regressions passed'
