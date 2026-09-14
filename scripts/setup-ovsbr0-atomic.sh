#!/bin/bash
# Atomic OVS Bridge Setup: ovsbr0 with management port + ens3 uplink
#
# Uses busctl to call the op-dbus OvsdbV1 D-Bus interface, which
# drives the Rust OvsdbClient internally (proper newline-framed JSON-RPC).
# IP migration from ens3 to ovsbr0 uses ip -batch for atomicity.
#
# Usage: sudo ./setup-ovsbr0-atomic.sh [--no-ip-migrate]

set -euo pipefail

BRIDGE="ovsbr0"
UPLINK="ens3"
DBUS_DEST="org.opdbus"
OVSDB_PATH="/org/opdbus/ovsdb"
OVSDB_IFACE="org.opdbus.OvsdbV1"
MIGRATE_IP=true

if [[ "${1:-}" == "--no-ip-migrate" ]]; then
    MIGRATE_IP=false
fi

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

log_info()  { echo -e "${GREEN}[INFO]${NC} $1"; }
log_warn()  { echo -e "${YELLOW}[WARN]${NC} $1"; }
log_error() { echo -e "${RED}[ERROR]${NC} $1"; }

die() { log_error "$1"; exit 1; }

# --- Preflight checks ---
[[ $EUID -eq 0 ]] || die "Must run as root"
command -v busctl &>/dev/null || die "busctl not found — install systemd"
ip link show "$UPLINK" &>/dev/null || die "Uplink interface $UPLINK does not exist"

# Verify op-dbus service is on the bus
if ! busctl --system status "$DBUS_DEST" &>/dev/null; then
    die "D-Bus service $DBUS_DEST is not running — start op-dbus first"
fi
log_info "D-Bus service $DBUS_DEST is available"

# Check if bridge already exists via OvsdbV1
BRIDGE_EXISTS=$(busctl --system call "$DBUS_DEST" "$OVSDB_PATH" "$OVSDB_IFACE" BridgeExists s "$BRIDGE" 2>&1) || die "D-Bus call failed (BridgeExists): $BRIDGE_EXISTS"
if echo "$BRIDGE_EXISTS" | grep -q "true"; then
    die "Bridge '$BRIDGE' already exists in OVSDB"
fi

# Also check kernel
if ip link show "$BRIDGE" &>/dev/null 2>&1; then
    die "Bridge '$BRIDGE' already exists in kernel"
fi

# --- Capture ens3 IP config before we enslave it ---
ENS3_ADDRS=()
ENS3_GATEWAY=""

if $MIGRATE_IP; then
    log_info "Capturing IP configuration from $UPLINK..."

    while IFS= read -r addr; do
        [[ -n "$addr" ]] && ENS3_ADDRS+=("$addr")
    done < <(ip -4 -o addr show dev "$UPLINK" scope global | awk '{print $4}')

    ENS3_GATEWAY=$(ip -4 route show default dev "$UPLINK" 2>/dev/null | awk '{print $3}' | head -1)

    if [[ ${#ENS3_ADDRS[@]} -eq 0 ]]; then
        log_warn "$UPLINK has no global IPv4 addresses — IP migration will be skipped"
        MIGRATE_IP=false
    else
        log_info "  Addresses: ${ENS3_ADDRS[*]}"
        log_info "  Gateway:   ${ENS3_GATEWAY:-none}"
    fi
fi

# --- Create bridge via OvsdbV1 D-Bus interface ---
log_info "Creating bridge '$BRIDGE' with management port..."

RESULT=$(busctl --system call \
    "$DBUS_DEST" \
    "$OVSDB_PATH" \
    "$OVSDB_IFACE" \
    CreateBridge \
    s "$BRIDGE" 2>&1) || die "D-Bus call failed (CreateBridge): $RESULT"

log_info "Bridge created: $RESULT"

# --- Enslave uplink via OvsdbV1 AddPort ---
log_info "Enslaving uplink '$UPLINK' to bridge '$BRIDGE'..."

RESULT=$(busctl --system call \
    "$DBUS_DEST" \
    "$OVSDB_PATH" \
    "$OVSDB_IFACE" \
    AddPort \
    ss "$BRIDGE" "$UPLINK" 2>&1) || die "D-Bus call failed (AddPort): $RESULT"

log_info "Uplink enslaved: $RESULT"

# --- Wait for kernel interfaces to appear ---
log_info "Waiting for kernel interfaces..."
for i in $(seq 1 30); do
    ip link show "$BRIDGE" &>/dev/null && break
    sleep 0.1
done
ip link show "$BRIDGE" &>/dev/null || die "Bridge interface $BRIDGE did not appear in kernel"
log_info "  $BRIDGE is up in kernel"

# --- Atomic IP migration: ens3 -> ovsbr0 ---
if $MIGRATE_IP; then
    log_info "Migrating IP configuration from $UPLINK to $BRIDGE..."

    BATCH_FILE=$(mktemp /tmp/ovs-ip-migrate.XXXXXX)
    trap "rm -f $BATCH_FILE" EXIT

    echo "link set dev $BRIDGE up" >> "$BATCH_FILE"

    for addr in "${ENS3_ADDRS[@]}"; do
        echo "addr del $addr dev $UPLINK" >> "$BATCH_FILE"
        echo "addr add $addr dev $BRIDGE" >> "$BATCH_FILE"
    done

    if [[ -n "$ENS3_GATEWAY" ]]; then
        echo "route replace default via $ENS3_GATEWAY dev $BRIDGE" >> "$BATCH_FILE"
    fi

    log_info "  Executing ip batch:"
    while IFS= read -r line; do
        log_info "    ip $line"
    done < "$BATCH_FILE"

    ip -batch "$BATCH_FILE"
    log_info "IP migration complete"
fi

# --- Bring uplink up (OVS needs it for forwarding) ---
ip link set dev "$UPLINK" up

# --- Force mirror reconciliation so D-Bus tree reflects new bridge ---
log_info "Triggering D-Bus mirror reconciliation..."
busctl --system call "$DBUS_DEST" /org/opdbus org.opdbus.MirrorV1 Reconcile 2>/dev/null || log_warn "  MirrorV1.Reconcile not available (non-fatal)"

# --- Verification ---
log_info ""
log_info "=== Verification ==="

log_info "List bridges via OvsdbV1:"
busctl --system call "$DBUS_DEST" "$OVSDB_PATH" "$OVSDB_IFACE" ListBridges 2>/dev/null || log_warn "  ListBridges failed"

log_info ""
log_info "List ports on $BRIDGE via OvsdbV1:"
busctl --system call "$DBUS_DEST" "$OVSDB_PATH" "$OVSDB_IFACE" ListPorts s "$BRIDGE" 2>/dev/null || log_warn "  ListPorts failed"

log_info ""
log_info "D-Bus OVSDB mirror tree:"
busctl --system tree "$DBUS_DEST" 2>/dev/null | grep -i "ovsdb\|bridge" || log_warn "  No OVSDB objects in mirror tree"

log_info ""
log_info "Kernel state:"
ip -4 addr show dev "$BRIDGE" 2>/dev/null || log_warn "  $BRIDGE has no IPv4 address"
ip link show dev "$UPLINK" 2>/dev/null | head -2

log_info ""
log_info "=== Summary ==="
log_info "  Bridge:         $BRIDGE"
log_info "  Management:     $BRIDGE (type=internal)"
log_info "  Uplink:         $UPLINK (enslaved)"
log_info "  Fail mode:      standalone"
log_info "  Method:         busctl -> org.opdbus.OvsdbV1 -> OvsdbClient"
if $MIGRATE_IP; then
    log_info "  IP migrated:    ${ENS3_ADDRS[*]}"
    [[ -n "$ENS3_GATEWAY" ]] && log_info "  Default route:  via $ENS3_GATEWAY dev $BRIDGE"
fi
log_info ""
log_info "Done. Verify connectivity: ping -c1 -W2 ${ENS3_GATEWAY:-8.8.8.8}"
