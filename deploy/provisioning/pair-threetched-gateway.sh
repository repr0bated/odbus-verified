#!/bin/sh
# Pair ONE client (the op-grpc-bridge) with the threetched gateway and store
# its bearer token as /etc/op-dbus/secrets/threetched-runtime-token
# (root:root 0600, atomic rename). Never prints the code or the token.
#   pair-threetched-gateway.sh [--force]
# Flow (zeroclaw-gateway lib.rs):
#   1. POST /admin/paircode/new   localhost-only; new one-time code, does NOT
#      revoke existing tokens (lib.rs:1638, 3984-4120; CLI: `gateway get-paircode --new`)
#   2. POST /pair  header X-Pairing-Code: <code>  ->  {"paired":true,"token":...}
#      (lib.rs:1642, 2242-2348); token is persisted encrypted in config.toml
#   3. verify: GET /api/status with Authorization: Bearer <token> (api.rs:241-248)
# Existing valid token file -> no-op (pair once). --force pairs a new client
# (old token stays valid until revoked with `gateway get-paircode --rotate-device`).
# Never uses zeroclaw-gateway.token.
set -eu
GW=${THREETCHED_RUNTIME_ENDPOINT:-http://127.0.0.1:8084}
OUT=${THREETCHED_RUNTIME_TOKEN_FILE:-/etc/op-dbus/secrets/threetched-runtime-token}
DIR=$(dirname "$OUT")
fail() { echo "pair-threetched-gateway: $*" >&2; exit 1; }
[ "$(id -u)" = 0 ] || fail "run as root"
case $GW in http://127.0.0.1:*|http://localhost:*|http://\[::1\]:*) ;; *) fail "gateway must be loopback (admin routes are localhost-only)";; esac
command -v curl >/dev/null && command -v jq >/dev/null || fail "curl and jq required"
umask 077
T=$(mktemp -d); trap 'rm -rf "$T"' EXIT INT TERM
valid_file() { [ -f "$1" ] && [ ! -L "$1" ] && [ "$(stat -Lc '%u %a' "$1")" = "0 600" -o "$(stat -Lc '%u %a' "$1")" = "0 400" ] \
  && tr -d '\n' <"$1" | grep -Eq '^[A-Za-z0-9._~-]{1,8192}$'; }
works() { printf 'Authorization: Bearer %s\n' "$(tr -d '\n' <"$1")" >"$T/auth.h"
  [ "$(curl -s -o /dev/null -w '%{http_code}' --max-time 5 -H @"$T/auth.h" "$GW/api/status")" = 200 ]; }

curl -fsS -o /dev/null --max-time 5 "$GW/health" || fail "gateway not answering at $GW/health (start threetched-gateway first)"
if [ "${1:-}" != --force ] && valid_file "$OUT"; then
  works "$OUT" && { echo "runtime token present and accepted by the gateway; nothing to do"; exit 0; }
  fail "runtime token file exists but the gateway rejects it; rerun with --force to pair a new client"
fi
# 1. one-time code (kept only in a 0600 temp file)
curl -fsS --max-time 10 -X POST "$GW/admin/paircode/new" >"$T/code.json" || fail "admin paircode request failed"
jq -er '.pairing_code | strings | select(length>0)' "$T/code.json" >"$T/code" 2>/dev/null || fail "gateway returned no pairing code (pairing disabled?)"
printf 'X-Pairing-Code: %s\n' "$(cat "$T/code")" >"$T/code.h"
# 2. pair (header from file: the code never appears in argv/ps)
code=$(curl -s -o "$T/pair.json" -w '%{http_code}' --max-time 15 -X POST -H @"$T/code.h" "$GW/pair")
[ "$code" = 200 ] || fail "pairing refused (HTTP $code: $(jq -r '.error // empty' "$T/pair.json" 2>/dev/null))"
jq -er 'select(.paired==true and .persisted==true) | .token | strings' "$T/pair.json" | tr -d '\n' >"$T/token" \
  || fail "pair response has no persisted token"
grep -Eq '^[A-Za-z0-9._~-]{1,8192}$' "$T/token" || fail "token has characters/length the bridge refuses"
works "$T/token" || fail "new token not accepted by /api/status"
# 3. atomic install root:root 0600
install -d -o root -g root -m 0700 "$DIR" 2>/dev/null || [ -d "$DIR" ] || fail "cannot create $DIR"
install -o root -g root -m 0600 "$T/token" "$OUT.new.$$"
mv -f "$OUT.new.$$" "$OUT"
echo "paired: runtime token written to $OUT (root:root 0600)"
