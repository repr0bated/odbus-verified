#!/bin/bash
# Real transaction journal/backups/publication with controlled kernel/service boundaries.
set -euo pipefail
here=$(cd -- "$(dirname -- "$0")" && pwd)
if [[ ${1:-} != worker ]]; then
  tmp=$(mktemp -d); trap 'rm -rf "$tmp"' EXIT
  for fault in stage quiesce catalog-persist catalog-live activate move-first move-second bind-second fuse publish restore-failure catalog-restore none; do
    "$0" worker "$tmp/$fault" "$fault"
  done
  echo 'transaction failure regressions passed'
  exit
fi
h=$2 fault=$3
mkdir -p "$h/root/old/os" "$h/root/new/os" "$h/enabled/3tched-fs" "$h/enabled/threetched-fs" "$h/sv"
export GOLDEN_HOST_MOUNT=$h/disk GOLDEN_WORK=$h/disk/@opdbus-system-a GOLDEN_ROOT=$h/root GOLDEN_FSID=fixture GOLDEN_WORK_UUID=fixture GOLDEN_UPPER_UUID=fixture
export GOLDEN_LEDGER=$h/ledger.json GOLDEN_ENABLED_DIR=$h/enabled GOLDEN_SV=$h/sv-fake
GOLDEN_LIB=1 source "$here/../golden.sh"
ROOT=$h/root STATE=$h/root/golden-state.json CANDIDATE_STATE=$h/root/golden-candidate.json
printf '{"live":{"path":"%s/old/os","id":"1","uuid":"old","commit":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"},"rollback":null}\n' "$ROOT" >"$STATE"
printf '{"live":{"path":"%s/new/os","id":"2","uuid":"new","commit":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"},"rollback":{"path":"%s/old/os","id":"1","uuid":"old"}}\n' "$ROOT" "$ROOT" >"$CANDIDATE_STATE"
printf '{}\n' >"$ROOT/golden-catalog-before.json"
printf 'old-ledger\n' >"$ACT_LEDGER"
printf 'old-service\n' >"$h/service"
printf 'old-one\n' >"$ACT_ENABLED_DIR/3tched-fs/conf"
printf 'old-two\n' >"$ACT_ENABLED_DIR/threetched-fs/conf"
printf 'old\n' >"$h/alias-first"; printf 'old\n' >"$h/alias-second"
printf 'old-catalog\n' >"$h/catalog-live"; printf 'old-catalog\n' >"$h/catalog-persist"
printf 'running\n' >"$h/owners"
cp "$STATE" "$h/state.before"; cp "$ACT_LEDGER" "$h/ledger.before"
# Inject only external owners: production journal, ordering, snapshots and
# state/ledger restore run unchanged. Boundary doubles retain actual file effects.
require_readonly_release() { :; }
verify_release_inputs() { :; }
assert_no_cargo() { :; }
require_lifecycle_lock() { :; }
tx_snapshot_owners() {
  cp "$h/service" "$TXN/service.fixture"
  cp "$ACT_ENABLED_DIR/3tched-fs/conf" "$TXN/fuse-one.fixture"
  cp "$ACT_ENABLED_DIR/threetched-fs/conf" "$TXN/fuse-two.fixture"
}
tx_validate_catalog() { :; }
tx_catalog_publish() {
  printf 'new-catalog\n' >"$h/catalog-persist"
  [[ $fault != catalog-persist ]] || return 1
  printf 'new-catalog\n' >"$h/catalog-live"
  [[ $fault != catalog-live ]]
}
tx_catalog_restore() {
  [[ $fault != catalog-restore ]] || return 1
  printf 'old-catalog\n' >"$h/catalog-persist"; printf 'old-catalog\n' >"$h/catalog-live"
}
tx_quiesce_owners() { printf 'stopped\n' >"$h/owners"; touch "$TXN/services-attempted"; [[ ${1:-all} != services || $fault != quiesce ]]; }
tx_mount_prepare() { [[ $fault != stage ]]; }
tx_mount_switch() {
  local i=$1
  [[ $fault != move-first || $i != 0 ]] || return 1
  [[ $fault != move-second || $i != 1 ]] || return 1
  if [[ $i == 0 ]]; then printf 'new\n' >"$h/alias-first"; else
    [[ $fault != bind-second ]] || return 1
    printf 'new\n' >"$h/alias-second"
  fi
}
tx_mount_restore() { printf 'old\n' >"$h/alias-first"; printf 'old\n' >"$h/alias-second"; }
tx_mount_cleanup() { :; }
tx_restore_owners() {
  [[ $(cat "$h/catalog-live") == old-catalog && $(cat "$h/catalog-persist") == old-catalog ]]
  printf 'running\n' >"$h/owners"
  cp "$TXN/service.fixture" "$h/service"
  cp "$TXN/fuse-one.fixture" "$ACT_ENABLED_DIR/3tched-fs/conf"
  cp "$TXN/fuse-two.fixture" "$ACT_ENABLED_DIR/threetched-fs/conf"
  [[ $fault != restore-failure ]]
}
do_activate() { printf 'new-service\n' >"$h/service"; printf 'new-ledger\n' >"$ACT_LEDGER"; [[ $fault != activate ]]; }
do_activate_fuse() { printf 'new-one\n' >"$ACT_ENABLED_DIR/3tched-fs/conf"; [[ $fault != fuse && $fault != restore-failure && $fault != catalog-restore ]] || return 1; printf 'new-two\n' >"$ACT_ENABLED_DIR/threetched-fs/conf"; }
original_atomic=$(declare -f atomic_state)
# Preserve real publication implementation except the requested failure boundary.
eval "${original_atomic/atomic_state ()/real_atomic_state ()}"
atomic_state() { [[ $fault != publish ]] || return 1; real_atomic_state; }
APPLY=1
# Separate shell preserves errexit/EXIT trap behavior even when the parent
# tests a nonzero result. Functions above are the worker's actual boundaries.
set +e
( set -e; deploy_candidate )
rc=$?
set -e
if [[ $fault == none ]]; then
  [[ $rc == 0 && $(jq -r .live.id "$STATE") == 2 && $(cat "$h/service") == new-service && $(jq -r .phase "$ROOT/golden-transaction.json") == committed ]]
  [[ $(cat "$h/catalog-live") == new-catalog && $(cat "$h/catalog-persist") == new-catalog ]]
else
  [[ $rc != 0 ]]; cmp "$STATE" "$h/state.before"
  if [[ $fault == catalog-restore ]]; then
    [[ $(cat "$h/owners") == stopped && $(cat "$h/service") == new-service && $(jq -r .phase "$ROOT/golden-transaction.json") == rollback-incomplete ]]
    fault=none; do_recover
    [[ $(jq -r .phase "$ROOT/golden-transaction.json") == rolled-back ]]
    fault=catalog-restore
  fi
  cmp "$ACT_LEDGER" "$h/ledger.before"
  [[ $(cat "$h/service") == old-service && $(cat "$h/alias-first") == old && $(cat "$h/alias-second") == old && $(cat "$ACT_ENABLED_DIR/3tched-fs/conf") == old-one && $(cat "$ACT_ENABLED_DIR/threetched-fs/conf") == old-two ]]
  [[ $(cat "$h/catalog-live") == old-catalog && $(cat "$h/catalog-persist") == old-catalog && $(cat "$h/owners") == running ]]
  phase=$(jq -r .phase "$ROOT/golden-transaction.json")
  if [[ $fault == restore-failure ]]; then
    [[ $phase == rollback-incomplete ]]
    fault=none
    do_recover
    [[ $(jq -r .phase "$ROOT/golden-transaction.json") == rolled-back ]]
    fault=restore-failure
  else [[ $phase == rolled-back ]]; fi
fi
printf 'ok %s\n' "$fault"
