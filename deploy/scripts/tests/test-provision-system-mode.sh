#!/bin/sh
set -eu

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
PROVISIONER=$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)/provision-workspace-subscriber.sh
TMP_ROOT=$(mktemp -d "${TMPDIR:-/tmp}/provision-system-test.XXXXXX")
trap 'rm -rf "$TMP_ROOT"' EXIT HUP INT TERM
BIN="$TMP_ROOT/bin"
mkdir -p "$BIN"

cat > "$BIN/incus" <<'STUB'
#!/bin/sh
printf '%s\n' "$*" >> "$INCUS_LOG"
case "$1" in
    query)
        [ "${QUERY_FAIL:-0}" = 0 ] || exit 17
        if [ "${QUERY_MALFORMED:-0}" != 0 ]; then
            printf '{"metadata":[]}\n'
            exit 0
        fi
        if [ "${EXISTING_INSTANCE:-}" != "" ]; then
            printf '[{"name":"%s"}]\n' "$EXISTING_INSTANCE"
        else
            printf '[]\n'
        fi
        ;;
    init)
        [ "${INIT_FAIL:-0}" = 0 ] || exit 18
        [ "${CONFIG_FAIL:-0}" = 0 ] || exit 19
        ;;
    start) [ "${START_FAIL:-0}" = 0 ] ;;
esac
STUB
chmod +x "$BIN/incus"
for forbidden in wg busctl btrfs losetup zcall; do
    cat > "$BIN/$forbidden" <<'STUB'
#!/bin/sh
printf '%s %s\n' "${0##*/}" "$*" >> "$FORBIDDEN_LOG"
exit 99
STUB
    chmod +x "$BIN/$forbidden"
done

fail() { printf 'FAIL: %s\n' "$*" >&2; exit 1; }
run() {
    : > "$INCUS_LOG"
    if PATH="$BIN:/usr/bin:/bin" INCUS_LOG="$INCUS_LOG" OPDBUS_IDENTITY_ROOT="$TMP_ROOT/identity-must-not-exist" "$@"; then
        return 0
    else
        return $?
    fi
}
assert_log() { grep -F -- "$1" "$INCUS_LOG" >/dev/null || fail "missing Incus call: $1"; }
assert_no_log() { ! grep -E -- "$1" "$INCUS_LOG" >/dev/null || fail "unexpected Incus call matching: $1"; }
INCUS_LOG="$TMP_ROOT/incus.log"
export INCUS_LOG
FORBIDDEN_LOG="$TMP_ROOT/forbidden.log"
export FORBIDDEN_LOG

# Both accepted argument orders, explicit regular image/pool defaults, and ordinary start.
run "$PROVISIONER" app-api --system
assert_log 'query /1.0/instances?recursion=1'
assert_log 'init raccoon app-api --storage 3tched-storage --no-profiles'
assert_log '--config security.privileged=false --config security.nesting=false --config boot.autostart=false'
assert_log 'start app-api'
assert_no_log '(^| )(profile|device|delete|stop)( |$)'

run "$PROVISIONER" --system queue-worker --image local:noble-system --storage custom-pool --no-start
assert_log 'init local:noble-system queue-worker --storage custom-pool --no-profiles'
assert_no_log '^start '

# An explicit image equal to the identity default is still an explicit override.
run "$PROVISIONER" app-gateway --system --image 3a639373bb7a --no-start
assert_log 'init 3a639373bb7a app-gateway --storage 3tched-storage --no-profiles'
assert_no_log '^start '

# Environment defaults select the regular system image and system storage pool.
: > "$INCUS_LOG"
if PATH="$BIN:/usr/bin:/bin" INCUS_LOG="$INCUS_LOG" SYSTEM_CONTAINER_IMAGE=images:ubuntu/noble SYSTEM_STORAGE_POOL=system-pool \
    "$PROVISIONER" --system worker-env --no-start; then
    :
else
    fail 'environment image/storage defaults failed'
fi
assert_log 'init images:ubuntu/noble worker-env --storage system-pool --no-profiles'

# Exact existing instance is refused before any mutation.
: > "$INCUS_LOG"
if PATH="$BIN:/usr/bin:/bin" INCUS_LOG="$INCUS_LOG" EXISTING_INSTANCE=app-api "$PROVISIONER" app-api --system; then
    fail 'existing instance was accepted'
fi
assert_no_log '^(init|config|start|delete|stop) '

# Identity-only switches are rejected before making any Incus call.
for option in --pubkey --identity-dir --private-key-file --email --mesh-ip --fstorage-gib --session-id; do
    : > "$INCUS_LOG"
    if PATH="$BIN:/usr/bin:/bin" INCUS_LOG="$INCUS_LOG" "$PROVISIONER" app-api --system "$option" ignored; then
        fail "identity-only option accepted in system mode: $option"
    fi
    [ ! -s "$INCUS_LOG" ] || fail "Incus called for rejected identity-only option: $option"
done
for option in --ghostbridge --semantic --recreate; do
    : > "$INCUS_LOG"
    if PATH="$BIN:/usr/bin:/bin" INCUS_LOG="$INCUS_LOG" "$PROVISIONER" app-api --system "$option"; then
        fail "identity-only option accepted in system mode: $option"
    fi
    [ ! -s "$INCUS_LOG" ] || fail "Incus called for rejected identity-only option: $option"
done
if PATH="$BIN:/usr/bin:/bin" INCUS_LOG="$INCUS_LOG" "$PROVISIONER" app-api --storage custom-pool; then
    fail '--storage without --system was accepted'
fi

# Input validation and every Incus failure boundary propagate without later mutations.
for name in '-bad' 'bad/name' 'bad_name' 'trailing-' ''; do
    if PATH="$BIN:/usr/bin:/bin" INCUS_LOG="$INCUS_LOG" "$PROVISIONER" --system "$name"; then
        fail "invalid name accepted: $name"
    fi
done
: > "$INCUS_LOG"
if PATH="$BIN:/usr/bin:/bin" INCUS_LOG="$INCUS_LOG" "$PROVISIONER" --system bad-image --image --invalid; then
    fail 'option-prefixed image was accepted'
fi
[ ! -s "$INCUS_LOG" ] || fail 'Incus called for rejected option-prefixed image'
: > "$INCUS_LOG"
if PATH="$BIN:/usr/bin:/bin" INCUS_LOG="$INCUS_LOG" QUERY_FAIL=1 "$PROVISIONER" --system query-fail; then fail 'query failure swallowed'; fi
assert_no_log '^(init|config|start) '
: > "$INCUS_LOG"
if PATH="$BIN:/usr/bin:/bin" INCUS_LOG="$INCUS_LOG" QUERY_MALFORMED=1 "$PROVISIONER" --system malformed-inventory; then
    fail 'malformed Incus inventory was accepted'
fi
assert_no_log '^(init|config|start) '
: > "$INCUS_LOG"
if PATH="$BIN:/usr/bin:/bin" INCUS_LOG="$INCUS_LOG" INIT_FAIL=1 "$PROVISIONER" --system init-fail; then fail 'init failure swallowed'; fi
assert_no_log '^(config|start) '
: > "$INCUS_LOG"
if PATH="$BIN:/usr/bin:/bin" INCUS_LOG="$INCUS_LOG" CONFIG_FAIL=1 "$PROVISIONER" --system config-fail; then fail 'config failure swallowed'; fi
assert_no_log '^start '
: > "$INCUS_LOG"
if PATH="$BIN:/usr/bin:/bin" INCUS_LOG="$INCUS_LOG" START_FAIL=1 "$PROVISIONER" --system start-fail; then fail 'start failure swallowed'; fi

[ ! -e "$TMP_ROOT/identity-must-not-exist" ] || fail 'system mode created identity storage'
[ ! -s "$FORBIDDEN_LOG" ] || fail 'system mode invoked an identity/storage/Cozo helper'
printf 'PASS: standalone --system provisioning branch\n'
