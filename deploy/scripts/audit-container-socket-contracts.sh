#!/bin/sh
# Verify declared service, Unix socket, and TCP readiness inside every existing
# container.  A running Incus instance alone is never considered ready.
set -eu

REPO_ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
CONTRACT=${1:-"$REPO_ROOT/deploy/config/container-socket-contracts.json"}
failures=0

for name in $(jq -r '.containers[].name' "$CONTRACT"); do
    item=$(jq -c --arg name "$name" '.containers[] | select(.name == $name)' "$CONTRACT")
    echo "[$name]"

    if ! incus info "$name" >/dev/null 2>&1; then
        echo "  FAIL container is absent"
        failures=$((failures + 1))
        continue
    fi

    for service in $(printf '%s' "$item" | jq -r '.services[]?'); do
        if incus exec "$name" -- sh -s -- "$service" 2>/dev/null <<'SERVICE_CHECK'
set -eu
declared=$1
service=${declared%.service}
if command -v sv >/dev/null 2>&1; then
    for directory in "/etc/runit/runsvdir/default/$service" "/etc/runit/sv/$service"; do
        if [ -e "$directory" ] || [ -L "$directory" ]; then
            exec sv check "$directory"
        fi
    done
    exit 1
fi
if command -v rc-service >/dev/null 2>&1; then
    exec rc-service "$service" status
fi
echo "no supported runit/OpenRC service manager" >&2
exit 127
SERVICE_CHECK
        then
            echo "  ok   service $service"
        else
            echo "  FAIL service $service"
            failures=$((failures + 1))
        fi
    done

    for socket in $(printf '%s' "$item" | jq -r '.unix_sockets[]?'); do
        if incus exec "$name" -- test -S "$socket" 2>/dev/null; then
            echo "  ok   socket $socket"
        else
            echo "  FAIL socket $socket"
            failures=$((failures + 1))
        fi
    done

    for port in $(printf '%s' "$item" | jq -r '.tcp_ports[]?'); do
        if incus exec "$name" -- sh -lc "
            if command -v ss >/dev/null 2>&1; then
                ss -H -lnt
            elif command -v netstat >/dev/null 2>&1; then
                netstat -lnt
            else
                exit 127
            fi | grep -Eq '[:.]$port[[:space:]]'
        "; then
            echo "  ok   tcp $port"
        else
            echo "  FAIL tcp $port"
            failures=$((failures + 1))
        fi
    done

    require_endpoint=$(printf '%s' "$item" | jq -r '.require_endpoint // false')
    endpoint_count=$(printf '%s' "$item" | jq '(.unix_sockets | length) + (.tcp_ports | length)')
    if [ "$require_endpoint" = true ] && [ "$endpoint_count" -eq 0 ]; then
        echo "  FAIL no workload endpoint declared"
        failures=$((failures + 1))
    fi
done

for listener in $(jq -r '.host_listeners[] | "\(.address):\(.port)"' "$CONTRACT"); do
    address=${listener%:*}
    port=${listener##*:}
    if ss -H -lnt 2>/dev/null | grep -Eq "[[:space:]]${address}:${port}[[:space:]]"; then
        echo "[host] ok   tcp $listener"
    else
        echo "[host] FAIL tcp $listener"
        failures=$((failures + 1))
    fi
done

[ "$failures" -eq 0 ]
