#!/bin/bash
# Maintenance-window OVS + systemd-networkd cutover script.
# Run this from noVNC or another out-of-band console as root.

set -euo pipefail

SCRIPT_DIR="$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)"
REPO_ROOT="$(CDPATH='' cd -- "$SCRIPT_DIR/.." && pwd)"

BRIDGE="${OP_DBUS_BRIDGE_NAME:-ovsbr0}"
UPLINK="${OP_DBUS_UPLINK_PORT:-eth0}"
ALLOW_UPLINK_CUTOVER="${OP_DBUS_ALLOW_UPLINK_CUTOVER:-0}"
SECONDARY_UPLINK_IFACE="${OP_DBUS_SECONDARY_UPLINK_IFACE:-uplink1}"
SECONDARY_UPLINK_IPV4="${OP_DBUS_SECONDARY_UPLINK_IPV4:-15.235.37.41/32}"
DBUS_DEST="${OP_DBUS_DEST:-org.opdbus}"
OVSDB_PATH="${OP_DBUS_OVS_PATH:-/org/opdbus/ovsdb}"
OVSDB_IFACE="${OP_DBUS_OVS_IFACE:-org.opdbus.OvsdbV1}"
BACKUP_DIR="${BACKUP_DIR:-/root/op-dbus-cutover-$(date +%Y%m%d-%H%M%S)}"
AUTO_YES=false
declare -a UPLINK_ADDRS=()
GATEWAY=""
BATCH_FILE=""

while [ $# -gt 0 ]; do
    case "$1" in
        --yes)
            AUTO_YES=true
            shift
            ;;
        *)
            echo "Unknown argument: $1" >&2
            echo "Usage: sudo $0 [--yes]" >&2
            exit 2
            ;;
    esac
done

log() {
    printf '[cutover] %s\n' "$*"
}

die() {
    printf '[cutover] ERROR: %s\n' "$*" >&2
    exit 1
}

start_service_via_dinit_dbus() {
    local service="$1"
    local output

    if command -v gdbus >/dev/null 2>&1; then
        if output="$(gdbus call \
            --system \
            --dest org.chimera.dinit \
            --object-path /org/chimera/dinit \
            --method org.chimera.dinit.Manager.StartService \
            "$service" \
            false 2>&1)"; then
            return 0
        fi

        if printf '%s' "$output" | grep -q 'org.chimera.dinit.Error.ServiceAlready'; then
            return 0
        fi
    fi

    if command -v dinitctl >/dev/null 2>&1; then
        dinitctl start "$service" >/dev/null 2>&1 && return 0
    fi

    return 1
}

stop_service_via_dinit_dbus() {
    local service="$1"
    local output

    if command -v gdbus >/dev/null 2>&1; then
        if output="$(gdbus call \
            --system \
            --dest org.chimera.dinit \
            --object-path /org/chimera/dinit \
            --method org.chimera.dinit.Manager.StopService \
            "$service" \
            false \
            false \
            false 2>&1)"; then
            return 0
        fi

        if printf '%s' "$output" | grep -q 'org.chimera.dinit.Error.ServiceAlready'; then
            return 0
        fi
    fi

    if command -v dinitctl >/dev/null 2>&1; then
        dinitctl stop "$service" >/dev/null 2>&1 && return 0
    fi

    return 1
}

restart_service_via_dinit_dbus() {
    local service="$1"
    stop_service_via_dinit_dbus "$service" || true
    start_service_via_dinit_dbus "$service"
}

cleanup() {
    if [ -n "${BATCH_FILE:-}" ] && [ -f "$BATCH_FILE" ]; then
        rm -f "$BATCH_FILE"
    fi
}

trap cleanup EXIT

confirm() {
    local prompt="$1"
    local reply

    if $AUTO_YES; then
        return 0
    fi

    printf '\n[cutover] %s\n' "$prompt"
    printf '[cutover] Type YES to continue: '
    read -r reply
    [ "$reply" = "YES" ]
}

bridge_exists_ovsdb() {
    busctl --system call "$DBUS_DEST" "$OVSDB_PATH" "$OVSDB_IFACE" BridgeExists s "$BRIDGE" \
        2>/dev/null | grep -q 'true'
}

port_present_ovsdb() {
    busctl --system call "$DBUS_DEST" "$OVSDB_PATH" "$OVSDB_IFACE" ListPorts s "$BRIDGE" \
        2>/dev/null | grep -F "\"$UPLINK\"" >/dev/null 2>&1
}

wait_for_kernel_link() {
    local iface="$1"
    local i=0
    while [ "$i" -lt 50 ]; do
        if ip link show "$iface" >/dev/null 2>&1; then
            return 0
        fi
        i=$((i + 1))
        sleep 0.2
    done
    return 1
}

require_cmd() {
    command -v "$1" >/dev/null 2>&1 || die "missing required command: $1"
}

capture_state() {
    UPLINK_ADDRS=()
    mapfile -t UPLINK_ADDRS < <(ip -4 -o addr show dev "$UPLINK" scope global | awk '{print $4}')

    GATEWAY="$(ip -4 route show default dev "$UPLINK" 2>/dev/null | awk '{print $3}' | head -n1)"
    if [ -z "$GATEWAY" ]; then
        GATEWAY="$(ip route get 1.1.1.1 2>/dev/null | awk '/via/ {for (i=1;i<=NF;i++) if ($i=="via") print $(i+1)}' | head -n1)"
    fi
}

write_rollback_script() {
    local rollback="$BACKUP_DIR/rollback-eth0.sh"
    {
        echo '#!/bin/bash'
        echo 'set -euo pipefail'
        echo
        echo "ip link set dev \"$UPLINK\" up"
        echo "ip link set dev \"$BRIDGE\" up || true"
        for addr in "${UPLINK_ADDRS[@]}"; do
            echo "ip addr del \"$addr\" dev \"$BRIDGE\" 2>/dev/null || true"
            echo "ip addr add \"$addr\" dev \"$UPLINK\" 2>/dev/null || true"
        done
        if [ -n "$GATEWAY" ]; then
            echo "ip route replace default via \"$GATEWAY\" dev \"$UPLINK\" onlink"
        fi
        echo "echo rollback complete"
    } > "$rollback"
    chmod 700 "$rollback"
    log "Rollback helper written to $rollback"
}

backup_file() {
    local src="$1"
    local dest_dir="$2"
    if [ -f "$src" ]; then
        install -D -m 0644 "$src" "$dest_dir/$(basename "$src")"
    fi
}

install_repo_config() {
    install -d /etc/systemd/network /etc/dinit.d
    for network_file in "$REPO_ROOT"/deploy/systemd/networkd/*; do
        install -m 0644 "$network_file" "/etc/systemd/network/$(basename "$network_file")"
    done
    install -m 0644 "$REPO_ROOT/deploy/dinit/systemd-networkd" /etc/dinit.d/systemd-networkd
    log "Installed repo networkd and dinit files"
}

create_ovs_bridge_if_needed() {
    if bridge_exists_ovsdb; then
        log "OVSDB already contains bridge $BRIDGE"
        return 0
    fi

    if ip link show "$BRIDGE" >/dev/null 2>&1; then
        log "Kernel link $BRIDGE exists, but OVSDB bridge is missing"
        if ! confirm "Delete the conflicting kernel link $BRIDGE before creating the OVS bridge?"; then
            die "operator aborted before deleting conflicting kernel link"
        fi
        ip link set dev "$BRIDGE" down || true
        ip link del dev "$BRIDGE"
        log "Deleted conflicting kernel link $BRIDGE"
    fi

    log "Creating OVS bridge $BRIDGE through org.opdbus"
    busctl --system call "$DBUS_DEST" "$OVSDB_PATH" "$OVSDB_IFACE" CreateBridge s "$BRIDGE" >/dev/null
    bridge_exists_ovsdb || die "OVSDB bridge $BRIDGE was not created"
}

attach_uplink_if_needed() {
    if port_present_ovsdb; then
        log "OVSDB already shows uplink $UPLINK on bridge $BRIDGE"
        return 0
    fi

    log "Attaching uplink $UPLINK to OVS bridge $BRIDGE"
    busctl --system call "$DBUS_DEST" "$OVSDB_PATH" "$OVSDB_IFACE" AddPort ss "$BRIDGE" "$UPLINK" >/dev/null
    port_present_ovsdb || die "uplink $UPLINK not visible on OVS bridge $BRIDGE after AddPort"
}

atomic_ip_migration() {
    if [ "${#UPLINK_ADDRS[@]}" -eq 0 ]; then
        die "no global IPv4 address found on $UPLINK"
    fi
    [ -n "$GATEWAY" ] || die "no default gateway detected for $UPLINK"

    BATCH_FILE="$(mktemp /tmp/op-dbus-cutover.XXXXXX)"

    {
        echo "link set dev $BRIDGE up"
        for addr in "${UPLINK_ADDRS[@]}"; do
            echo "addr del $addr dev $UPLINK"
            echo "addr add $addr dev $BRIDGE"
        done
        echo "route replace default via $GATEWAY dev $BRIDGE onlink"
    } > "$BATCH_FILE"

    log "Executing atomic IP migration from $UPLINK to $BRIDGE"
    ip -batch "$BATCH_FILE"
    ip link set dev "$UPLINK" up
}

activate_networkd() {
    restart_service_via_dinit_dbus systemd-networkd || start_service_via_dinit_dbus systemd-networkd || true

    if command -v networkctl >/dev/null 2>&1; then
        if busctl --system status org.freedesktop.network1 >/dev/null 2>&1; then
            networkctl reload || true
            networkctl reconfigure "$BRIDGE" || true
        else
            log "systemd-networkd D-Bus service is not available yet; skipping networkctl reconfigure"
        fi
    fi
}

show_state() {
    log "Current link state:"
    ip -br link
    printf '\n'
    log "Current IPv4 state:"
    ip -4 addr show dev "$UPLINK" 2>/dev/null || true
    ip -4 addr show dev "$SECONDARY_UPLINK_IFACE" 2>/dev/null || true
    ip -4 addr show dev "$BRIDGE" 2>/dev/null || true
    printf '\n'
    log "Current default routes:"
    ip route show default
    printf '\n'
    log "Current OVSDB state:"
    busctl --system call "$DBUS_DEST" "$OVSDB_PATH" "$OVSDB_IFACE" ListBridges 2>/dev/null || true
    if bridge_exists_ovsdb; then
        busctl --system call "$DBUS_DEST" "$OVSDB_PATH" "$OVSDB_IFACE" ListPorts s "$BRIDGE" 2>/dev/null || true
    fi
}

main() {
    local secondary_repo_netdev=""
    local secondary_repo_network=""

    [ "$EUID" -eq 0 ] || die "must run as root"
    [ "$ALLOW_UPLINK_CUTOVER" = "1" ] || die "refusing to attach standalone uplink $UPLINK to $BRIDGE without OP_DBUS_ALLOW_UPLINK_CUTOVER=1"

    require_cmd ip
    require_cmd busctl
    require_cmd install

    [ -f "$REPO_ROOT/deploy/systemd/networkd/10-eth0.network" ] || die "missing repo file: deploy/systemd/networkd/10-eth0.network"
    secondary_repo_netdev="$(find "$REPO_ROOT/deploy/systemd/networkd" -maxdepth 1 -type f -name "*${SECONDARY_UPLINK_IFACE}.netdev" | head -n1)"
    secondary_repo_network="$(find "$REPO_ROOT/deploy/systemd/networkd" -maxdepth 1 -type f -name "*${SECONDARY_UPLINK_IFACE}.network" | head -n1)"
    [ -n "$secondary_repo_netdev" ] || die "missing repo netdev file for secondary uplink interface $SECONDARY_UPLINK_IFACE"
    [ -n "$secondary_repo_network" ] || die "missing repo network file for secondary uplink interface $SECONDARY_UPLINK_IFACE"
    [ -f "$REPO_ROOT/deploy/systemd/networkd/20-ovsbr0.network" ] || die "missing repo file: deploy/systemd/networkd/20-ovsbr0.network"
    [ -f "$REPO_ROOT/deploy/dinit/systemd-networkd" ] || die "missing repo file: deploy/dinit/systemd-networkd"

    busctl --system status "$DBUS_DEST" >/dev/null 2>&1 || die "D-Bus service $DBUS_DEST is not available"
    ip link show "$UPLINK" >/dev/null 2>&1 || die "uplink $UPLINK does not exist"

    capture_state

    log "This script is intended for noVNC / out-of-band console use only"
    log "Bridge: $BRIDGE"
    log "Uplink: $UPLINK"
    log "Secondary uplink interface: $SECONDARY_UPLINK_IFACE"
    log "Secondary uplink IPv4: $SECONDARY_UPLINK_IPV4"
    log "Captured IPv4 addresses on $UPLINK: ${UPLINK_ADDRS[*]:-(none)}"
    log "Captured default gateway: ${GATEWAY:-none}"
    printf '\n'
    show_state

    if ! confirm "Proceed with backup and cutover preparation?"; then
        die "operator aborted before changes"
    fi

    install -d "$BACKUP_DIR/etc-systemd-network" "$BACKUP_DIR/etc-dinit.d"
    backup_file /etc/systemd/network/10-eth0.network "$BACKUP_DIR/etc-systemd-network"
    backup_file "/etc/systemd/network/$(basename "$secondary_repo_netdev")" "$BACKUP_DIR/etc-systemd-network"
    backup_file "/etc/systemd/network/$(basename "$secondary_repo_network")" "$BACKUP_DIR/etc-systemd-network"
    backup_file /etc/systemd/network/20-ovsbr0.network "$BACKUP_DIR/etc-systemd-network"
    backup_file /etc/dinit.d/systemd-networkd "$BACKUP_DIR/etc-dinit.d"
    write_rollback_script
    install_repo_config

    create_ovs_bridge_if_needed
    attach_uplink_if_needed

    wait_for_kernel_link "$BRIDGE" || die "kernel link $BRIDGE did not appear after OVS bridge creation"

    if ! confirm "Run the atomic IPv4/default-route migration from $UPLINK to $BRIDGE now?"; then
        die "operator aborted before IP migration"
    fi

    atomic_ip_migration
    activate_networkd

    printf '\n'
    log "Post-cutover state:"
    show_state
    printf '\n'
    log "If recovery is needed, run: $BACKUP_DIR/rollback-eth0.sh"
}

main "$@"
