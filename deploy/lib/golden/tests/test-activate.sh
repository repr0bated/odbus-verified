#!/usr/bin/env bash
# Tests for golden.sh activate (deploy/lib/golden/activate.sh) against a fake
# host: fake runit dirs, a fake `sv`, a fake live slot and state.json.
# No subvolumes, no real services.
set -euo pipefail
PATH=/usr/sbin:/usr/bin:/sbin:/bin
export PATH
HERE=$(CDPATH='' cd -- "$(dirname -- "$(readlink -f -- "$0")")" && pwd)
GOLDEN=$HERE/../golden.sh
PASS=0
FAIL=0
ok() { PASS=$((PASS + 1)); printf '  ok  %s\n' "$1"; }
bad() { FAIL=$((FAIL + 1)); printf '  FAIL %s\n' "$1"; }
check() { local label=$1; shift; if "$@"; then ok "$label"; else bad "$label"; fi; }
has() { case "$2" in *"$1"*) return 0 ;; esac; return 1; }

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

setup() {
    rm -rf "$TMP/h"
    local h=$TMP/h
    mkdir -p "$h/sv" "$h/enabled" "$h/root/slots/live/usr" "$h/root/slots/live/opt/op-build/target/x86_64-unknown-linux-gnu/release" \
        "$h/prefix/usr/local/bin" "$h/prefix/usr/local/libexec/op-dbus/bin" "$h/lib"
    local rel=$h/root/slots/live/opt/op-build/target/x86_64-unknown-linux-gnu/release
    for b in op-mcp-door op-chat-responder op-web-server 3tched-fs op-grpc-bridge cozo-ublk opblob; do
        printf 'new %s\n' "$b" > "$rel/$b"; chmod 0755 "$rel/$b"
    done
    mkdir -p "$h/catalog" "$h/grants"
    printf '{"plugins":{}}\n' > "$h/catalog/.manifest.json"
    # A dummy blob so act_catalog_dir accepts this directory if the override is unset.
    printf 'not-a-real-blob\n' > "$h/catalog/dummy.blob"
    printf '{"$ROOT":{"capabilities":["held"]},"principal":{"capabilities":[]}}\n' > "$h/grants/capability-grants.json"
    printf '{"$ROOT":{"capabilities":["held"]},"principal":{"capabilities":[]}}\n' > "$h/grants/shm-capability-grants.json"
    cat > "$rel/op-grants-materializer" <<'MAT'
#!/bin/bash
# Fake live-slot materializer for activate tests.
set -e
cmd=${1:-}
shift || true
apply=0
while [ $# -gt 0 ]; do
    case "$1" in
        --apply-to-live) apply=1; shift ;;
        --catalog) shift; shift || true ;;
        *) shift ;;
    esac
done
source=${OP_GRANTS_SOURCE:-/etc/opdbus/capability-grants.json}
case "$cmd" in
    validate)
        if [ "${GOLDEN_FAKE_VALIDATE_FAIL:-}" = 1 ]; then
            echo "validate failed (test)" >&2
            exit 1
        fi
        echo Valid
        ;;
    check)
        if [ "${GOLDEN_FAKE_CHECK_STALE:-}" = 1 ]; then
            echo Stale
            exit 1
        fi
        echo Fresh
        ;;
    *)
        echo "unknown command $cmd" >&2
        exit 1
        ;;
esac
MAT
    chmod 0755 "$rel/op-grants-materializer"
    printf 'old door\n' > "$h/prefix/usr/local/bin/op-mcp-door"
    printf 'old responder\n' > "$h/prefix/usr/local/bin/op-chat-responder"
    printf 'old fuse\n' > "$h/prefix/usr/local/libexec/op-dbus/bin/3tched-fs"
    chmod 0755 "$h/prefix/usr/local/bin/"* "$h/prefix/usr/local/libexec/op-dbus/bin/"*
    mk() { mkdir -p "$h/sv/$1/log"; printf '#!/bin/sh\nexec %s\n' "$2" > "$h/sv/$1/run"; }
    mk op-mcp-door /usr/local/bin/op-mcp-door
    mk op-chat-responder /usr/local/bin/op-chat-responder
    mk op-web /usr/local/libexec/op-dbus/bin/op-web-server
    mk 3tched-fs /usr/local/libexec/op-dbus/bin/3tched-fs
    mk cozo-ublk-500g /opt/cozo-ublk/cozo-ublk
    mk op-of-controller /usr/local/bin/op-of-controller-other
    for s in op-mcp-door op-chat-responder 3tched-fs cozo-ublk-500g op-of-controller; do
        ln -s "$h/sv/$s" "$h/enabled/$s"
    done
    # op-web is installed but not enabled.
    cat > "$h/sv-fake" <<'SV'
#!/bin/bash
# fake sv: status prints a pid; restart bumps it unless <dir>/.fail exists.
if [ "$1" = -w ]; then shift 2; fi
verb=$1 dir=$2
pidf=$dir/.pid
[ -f "$pidf" ] || echo 100 > "$pidf"
case "$verb" in
    status) if [ -e "$dir/.down" ]; then echo "down: $dir: 0s"; else echo "run: $dir: (pid $(cat "$pidf")) 10s"; fi ;;
    restart)
        rm -f "$dir/.down"
        echo "$(( $(cat "$pidf") + 1 ))" > "$pidf"
        echo restart >> "$dir/.restarts"
        [ -e "$dir/.fail" ] && [ "$(readlink -f "$(cat "$dir/.binpath")" 2>/dev/null)" != "$(cat "$dir/.goodtarget" 2>/dev/null)" ] && exit 1
        exit 0 ;;
    down) touch "$dir/.down"; exit 0 ;;
esac
SV
    chmod 0755 "$h/sv-fake"
    cat > "$h/root/state.json" <<JSON
{"version":1,"live":{"path":"$h/root/slots/live","id":"261","readonly":true,"commit":"879e7d7f556df051e7544083f1c2c08f49b279b9"}}
JSON
    printf '{"version":1,"active":null,"previous":null}\n' > "$h/slots.json"
    cat > "$h/table.tsv" <<'T'
# test table
op-mcp-door	op-mcp-door	/usr/local/bin/op-mcp-door	stable:0
op-chat-responder	op-chat-responder	/usr/local/bin/op-chat-responder	stable:0
op-web	op-web-server	/usr/local/libexec/op-dbus/bin/op-web-server	stable:0
op-of-controller	op-of-controller	/usr/local/bin/op-of-controller	stable:0
3tched-fs	3tched-fs	/usr/local/libexec/op-dbus/bin/3tched-fs	stable:0
op-grpc-bridge	op-grpc-bridge	/usr/local/libexec/op-dbus/bin/op-grpc-bridge	stable:0
T
}

run_golden() {
    local h=$TMP/h
    GOLDEN_SV_DIR=$h/sv GOLDEN_ENABLED_DIR=$h/enabled GOLDEN_RELEASES_DIR=$h/releases \
    GOLDEN_LEDGER=$h/golden-activation.json GOLDEN_KIT_LEDGER=$h/slots.json GOLDEN_SV=$h/sv-fake \
    GOLDEN_INSTALL_PREFIX=$h/prefix GOLDEN_ACTIVATE_TABLE=$h/table.tsv GOLDEN_RESTART_WAIT=1 \
    GOLDEN_ALLOW_NONROOT_ACTIVATE=1 GOLDEN_SKIP_SUBVOL_CHECK=1 \
    GOLDEN_GRANTS_SOURCE=$h/grants/capability-grants.json \
    GOLDEN_GRANTS_PATH=$h/grants/shm-capability-grants.json \
    GOLDEN_CATALOG_DIR=$h/catalog \
    GOLDEN_GRANTS_FRESH_WAIT=${GOLDEN_GRANTS_FRESH_WAIT:-2} \
        bash -c '
            GOLDEN_LIB=1 source "$1"
            ROOT=$2; STATE=$ROOT/state.json; LIVE=$ROOT/slots/live
            shift 2
            parse_args "$@"
            do_activate
        ' bash "$GOLDEN" "$h/root" "$@"
}

printf 'activate: dry-run plan\n'
setup
h=$TMP/h
out=$(run_golden activate 2>&1) || true
check "plan is read-only" has "plan: activate" "$out"
check "door swaps" has "service op-mcp-door: swap" "$out"
check "responder swaps" has "service op-chat-responder: swap" "$out"
check "op-web skipped: not enabled" has "installed in $h/sv but not enabled" "$out"
check "run-script mismatch skipped" has "does not exec /usr/local/bin/op-of-controller" "$out"
check "3tched-fs excluded" has "service 3tched-fs: excluded" "$out"
check "disabled bridge skipped" has "service op-grpc-bridge: skip" "$out"
check "enabled cozo-ublk-500g excluded" has "service cozo-ublk-500g: excluded" "$out"
check "plan shows new versioned path" has "$h/releases/879e7d7f556d-261/bin/op-mcp-door" "$out"
check "plan says kit ledger untouched" has "is NOT modified" "$out"
check "plan preserves grants" has "no authority expansion" "$out"
check "dry-run staged nothing" test ! -e "$h/releases"
check "dry-run left grants unchanged" test "$(cat "$h/grants/capability-grants.json")" = '{"$ROOT":{"capabilities":["held"]},"principal":{"capabilities":[]}}'
check "dry-run left door a file" test ! -L "$h/prefix/usr/local/bin/op-mcp-door"

printf 'activate: apply success\n'
setup
run_golden --apply activate >"$TMP/out" 2>&1 || { cat "$TMP/out"; bad "apply exited non-zero"; }
d=$h/prefix/usr/local/bin/op-mcp-door
check "door is a symlink" test -L "$d"
check "door points at the versioned release" test "$(readlink "$d")" = "$h/releases/879e7d7f556d-261/bin/op-mcp-door"
check "door content is new" test "$(cat "$d")" = "new op-mcp-door"
check "previous door preserved" grep -rqx "old door" "$h/releases" --include=op-mcp-door
check "only swapped binaries and boot opblob staged" test "$(ls "$h/releases/879e7d7f556d-261/bin" | tr '\n' ' ')" = "op-chat-responder op-mcp-door opblob "
ob=$h/prefix/usr/local/libexec/op-dbus/bin/opblob
check "boot opblob projected onto live release" test "$(readlink "$ob")" = "$h/releases/879e7d7f556d-261/bin/opblob"
check "ledger records boot opblob projection" test "$(jq -r .active.projections.opblob.target "$h/golden-activation.json")" = "$h/releases/879e7d7f556d-261/bin/opblob"
check "3tched-fs untouched" test "$(cat "$h/prefix/usr/local/libexec/op-dbus/bin/3tched-fs")" = "old fuse"
check "3tched-fs not restarted" test ! -e "$h/sv/3tched-fs/.restarts"
check "op-web not started" test ! -e "$h/sv/op-web/.restarts"
check "ledger active release" test "$(jq -r .active.release "$h/golden-activation.json")" = "879e7d7f556d-261"
check "ledger matches state.json commit" test "$(jq -r .active.commit "$h/golden-activation.json")" = "879e7d7f556df051e7544083f1c2c08f49b279b9"
check "ledger lists two services" test "$(jq '.active.services|length' "$h/golden-activation.json")" = 2
check "kit slots.json untouched" test "$(cat "$h/slots.json")" = '{"version":1,"active":null,"previous":null}'
check "log service dir kept" test -d "$h/sv/op-mcp-door/log"
check "apply never expands grants" test "$(cat "$h/grants/capability-grants.json")" = '{"$ROOT":{"capabilities":["held"]},"principal":{"capabilities":[]}}'
run_golden --apply activate >"$TMP/out2" 2>&1 || bad "re-activate exited non-zero"
check "re-activate preserves absent prior release" test "$(jq -r .previous "$h/golden-activation.json")" = "null"

printf 'activate: resumes services quiesced for catalog publication\n'
setup
"$h/sv-fake" -w 1 down "$h/sv/op-mcp-door"
"$h/sv-fake" -w 1 down "$h/sv/op-chat-responder"
if run_golden --apply activate >"$TMP/quiesced" 2>&1; then ok "quiesced activation succeeds"; else cat "$TMP/quiesced"; bad "quiesced activation succeeds"; fi
check "quiesced door resumed" test ! -e "$h/sv/op-mcp-door/.down"
check "quiesced responder resumed" test ! -e "$h/sv/op-chat-responder/.down"

printf 'activate: failure delegates aggregate catalog-safe rollback\n'
setup
r=$h/prefix/usr/local/bin/op-chat-responder
echo "$r" > "$h/sv/op-chat-responder/.binpath"
echo "/nonexistent" > "$h/sv/op-chat-responder/.goodtarget"
touch "$h/sv/op-chat-responder/.fail"
if run_golden --apply activate >"$TMP/out3" 2>&1; then bad "failing activation exited zero"; else ok "failing activation exits non-zero"; fi
check "failure reported" has "activation failed (service: op-chat-responder)" "$(cat "$TMP/out3")"
check "door pin remains for aggregate restore" test "$(cat "$h/prefix/usr/local/bin/op-mcp-door")" = "new op-mcp-door"
check "responder pin remains for aggregate restore" test "$(cat "$r")" = "new op-chat-responder"
check "ledger has no active release" test "$(jq -r '.active' "$h/golden-activation.json")" = null
check "ledger requests aggregate rollback" test "$(jq -r .last_attempt.result "$h/golden-activation.json")" = "rollback-required:op-chat-responder"
check "old door never restarted against new catalog" test "$(wc -l < "$h/sv/op-mcp-door/.restarts")" = 1

printf 'activate: invalid existing grants abort before service flips\n'
setup
export GOLDEN_FAKE_VALIDATE_FAIL=1
before=$(cat "$h/grants/capability-grants.json")
if run_golden --apply activate >"$TMP/out4" 2>&1; then bad "invalid-grant activation exited zero"; else ok "invalid-grant activation exits non-zero"; fi
unset GOLDEN_FAKE_VALIDATE_FAIL
check "validation failure reported" has "existing grants validation/Fresh failed" "$(cat "$TMP/out4")"
check "grants unchanged after validation failure" test "$(cat "$h/grants/capability-grants.json")" = "$before"
check "door not flipped after validation failure" test ! -L "$h/prefix/usr/local/bin/op-mcp-door"
check "door content still old after validation failure" test "$(cat "$h/prefix/usr/local/bin/op-mcp-door")" = "old door"
check "no services restarted after validation failure" test ! -e "$h/sv/op-mcp-door/.restarts"

printf '\n%d passed, %d failed\n' "$PASS" "$FAIL"
[ "$FAIL" = 0 ]
