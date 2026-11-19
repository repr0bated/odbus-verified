#!/usr/bin/env bash
# Tests for golden.sh activate (deploy/lib/golden/activate.sh) against a fake
# host: fake runit dirs, a fake `sv`, a fake live slot and state.json.
# No subvolumes, no real services.
set -euo pipefail
PATH=/usr/sbin:/usr/bin:/sbin:/bin
export PATH
HERE=$(CDPATH='' cd -- "$(dirname -- "$(readlink -f -- "$0")")" && pwd)
GOLDEN=$HERE/../golden.sh
unset GOLDEN_SEED GOLDEN_ROOT GOLDEN_POOL_ROOT GOLDEN_OS_SUBDIR GOLDEN_LOWER
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
    mkdir -p "$h/sv" "$h/enabled" "$h/root/slots/live/usr" "$h/root/slots/live/opt/op-build/target/release" \
        "$h/prefix/usr/local/bin" "$h/prefix/usr/local/libexec/op-dbus/bin" "$h/lib"
    local rel=$h/root/slots/live/opt/op-build/target/release
    for b in op-mcp-door op-chat-responder op-web-server 3tched-fs op-grpc-bridge cozo-ublk; do
        printf 'new %s\n' "$b" > "$rel/$b"; chmod 0755 "$rel/$b"
    done
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
    status) echo "run: $dir: (pid $(cat "$pidf")) 10s" ;;
    restart)
        echo "$(( $(cat "$pidf") + 1 ))" > "$pidf"
        echo restart >> "$dir/.restarts"
        [ -e "$dir/.fail" ] && [ "$(readlink -f "$(cat "$dir/.binpath")" 2>/dev/null)" != "$(cat "$dir/.goodtarget" 2>/dev/null)" ] && exit 1
        exit 0 ;;
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
    GOLDEN_OS_SUBDIR='' GOLDEN_ALLOW_NONROOT_ACTIVATE=1 GOLDEN_SKIP_SUBVOL_CHECK=1 \
    GOLDEN_PROBE_TIMEOUT=2 \
        "$GOLDEN" --pool "$TMP" --seed "$TMP/seed" --root "$h/root" "$@"
}

printf 'activate: dry-run plan\n'
setup
h=$TMP/h
out=$(run_golden activate 2>&1) || true
check "plan is dry-run" has "DRY-RUN" "$out"
check "door swaps" has "service op-mcp-door: swap" "$out"
check "responder swaps" has "service op-chat-responder: swap" "$out"
check "op-web skipped: not enabled" has "installed in $h/sv but not enabled" "$out"
check "run-script mismatch skipped" has "does not exec /usr/local/bin/op-of-controller" "$out"
check "3tched-fs excluded" has "service 3tched-fs: excluded" "$out"
check "bridge excluded" has "service op-grpc-bridge: excluded" "$out"
check "enabled cozo-ublk-500g excluded" has "service cozo-ublk-500g: excluded" "$out"
check "plan shows new versioned path" has "$h/releases/879e7d7f556d-261/bin/op-mcp-door" "$out"
check "plan says kit ledger untouched" has "is NOT modified" "$out"
check "dry-run staged nothing" test ! -e "$h/releases"
check "dry-run left door a file" test ! -L "$h/prefix/usr/local/bin/op-mcp-door"
check "install alias plans too" has "plan: activate" "$(run_golden install 2>&1 || true)"

printf 'activate: apply success\n'
setup
run_golden --apply activate >"$TMP/out" 2>&1 || { cat "$TMP/out"; bad "apply exited non-zero"; }
d=$h/prefix/usr/local/bin/op-mcp-door
check "door is a symlink" test -L "$d"
check "door points at the versioned release" test "$(readlink "$d")" = "$h/releases/879e7d7f556d-261/bin/op-mcp-door"
check "door content is new" test "$(cat "$d")" = "new op-mcp-door"
check "previous door preserved" grep -rqx "old door" "$h/releases" --include=op-mcp-door
check "only swapped binaries staged" test "$(ls "$h/releases/879e7d7f556d-261/bin" | tr '\n' ' ')" = "op-chat-responder op-mcp-door "
check "3tched-fs untouched" test "$(cat "$h/prefix/usr/local/libexec/op-dbus/bin/3tched-fs")" = "old fuse"
check "3tched-fs not restarted" test ! -e "$h/sv/3tched-fs/.restarts"
check "op-web not started" test ! -e "$h/sv/op-web/.restarts"
check "ledger active release" test "$(jq -r .active.release "$h/golden-activation.json")" = "879e7d7f556d-261"
check "ledger matches state.json commit" test "$(jq -r .active.commit "$h/golden-activation.json")" = "879e7d7f556df051e7544083f1c2c08f49b279b9"
check "ledger lists two services" test "$(jq '.active.services|length' "$h/golden-activation.json")" = 2
check "kit slots.json untouched" test "$(cat "$h/slots.json")" = '{"version":1,"active":null,"previous":null}'
check "log service dir kept" test -d "$h/sv/op-mcp-door/log"
run_golden --apply activate >"$TMP/out2" 2>&1 || bad "re-activate exited non-zero"
check "re-activate keeps previous pointer" test "$(jq -r .previous.release "$h/golden-activation.json")" = "879e7d7f556d-261"

printf 'activate: failure rolls back every flipped service\n'
setup
r=$h/prefix/usr/local/bin/op-chat-responder
echo "$r" > "$h/sv/op-chat-responder/.binpath"
echo "/nonexistent" > "$h/sv/op-chat-responder/.goodtarget"
touch "$h/sv/op-chat-responder/.fail"
if run_golden --apply activate >"$TMP/out3" 2>&1; then bad "failing activation exited zero"; else ok "failing activation exits non-zero"; fi
check "failure reported" has "activation rolled back (failed: op-chat-responder)" "$(cat "$TMP/out3")"
check "door rolled back to old content" test "$(cat "$h/prefix/usr/local/bin/op-mcp-door")" = "old door"
check "responder rolled back to old content" test "$(cat "$r")" = "old responder"
check "ledger has no active release" test "$(jq -r '.active' "$h/golden-activation.json")" = null
check "ledger records rolled-back attempt" test "$(jq -r .last_attempt.result "$h/golden-activation.json")" = "rolled-back:op-chat-responder"
check "door restarted twice (flip + rollback)" test "$(wc -l < "$h/sv/op-mcp-door/.restarts")" = 2

printf '\n%d passed, %d failed\n' "$PASS" "$FAIL"
[ "$FAIL" = 0 ]
