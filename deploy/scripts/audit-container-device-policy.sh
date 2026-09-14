#!/bin/sh
# Audit live Incus containers against the NIC-less / proxy-less device policy.
#
# The plugin (`NamedDevice::enforce_device_policy`) refuses to CREATE `proxy` and
# `nic` devices, but anything added out-of-band with `incus config device add`
# bypasses the control plane entirely. That is how this fleet drifted back to
# per-port host forwards. This script makes that drift visible.
#
# Exit 0 = clean, 1 = violations found. Safe to run repeatedly; reads only.
set -eu

violations=0
containers=$(incus list -c n --format csv 2>/dev/null)

for ct in $containers; do
    devices=$(incus config device show "$ct" 2>/dev/null || true)
    [ -n "$devices" ] || continue

    # Device blocks are "name:" at column 0 followed by indented keys; track the
    # current name so a violation can be reported as <container>/<device>.
    echo "$devices" | awk -v ct="$ct" '
        /^[^ ][^:]*:/ { name = $0; sub(/:.*/, "", name); next }
        /^[[:space:]]+type:[[:space:]]*(proxy|nic)[[:space:]]*$/ {
            type = $2
            printf "VIOLATION %s/%s type=%s\n", ct, name, type
        }
    '
    if echo "$devices" | grep -qE '^[[:space:]]+type:[[:space:]]*(proxy|nic)[[:space:]]*$'; then
        violations=$((violations + 1))
    fi
done

if [ "$violations" -eq 0 ]; then
    echo "device policy: clean (no proxy/nic devices on any container)"
    exit 0
fi

echo ""
echo "device policy: $violations container(s) in violation."
echo "Publish the service over the ghostbridge UDS with op-uds-relay instead:"
echo "  CT   : op-uds-relay --unix-to-tcp /opt/run-mounts/ghostbridge/<svc>/<name>.sock=127.0.0.1:<port>"
echo "  host : op-uds-relay --tcp-to-unix <addr>:<port>=/run/ghostbridge/<svc>/<name>.sock"
exit 1
