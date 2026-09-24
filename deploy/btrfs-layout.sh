#!/bin/bash
#
# btrfs-layout.sh — prepare the host filesystem for OP-DBUS deployment.
#
# btrfs is transport, not backup. build-golden.sh creates the golden subvolume
# for btrfs send. This script only ensures the host prerequisites:
#
#   - the root filesystem is btrfs (refuse otherwise)
#   - the mount point directory exists
#   - the btrfs root-level (subvolid=5) mount is available
#
# Subvolume creation and population belong to build-golden.sh, not here.
# Owner correction 2026-09-23: cache and Snowball retain their Btrfs
# subvolumes and configured snapshot retention. There is no global five-
# subvolume limit. This prerequisite script does not alter runtime storage.
#

set -euo pipefail

# ============================================================================
# CONFIGURATION
# ============================================================================

# Where the golden subvolume lives (build-golden.sh creates it).
OPDBUS_ROOT="/opt/op-dbus"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
BLUE='\033[0;34m'
NC='\033[0m'

log_info() { echo -e "${BLUE}[INFO]${NC} $1"; }
log_success() { echo -e "${GREEN}[OK]${NC} $1"; }
log_error() { echo -e "${RED}[ERROR]${NC} $1"; }

# ============================================================================
# FUNCTIONS
# ============================================================================

check_btrfs() {
    log_info "Checking btrfs filesystem..."

    local fs_type
    fs_type=$(findmnt -n -o FSTYPE -T /)

    if [[ "$fs_type" != "btrfs" ]]; then
        log_error "Root filesystem is not btrfs (found: $fs_type)"
        log_error "btrfs is required for subvolume snapshots and send/receive"
        exit 1
    fi

    local btrfs_dev
    btrfs_dev=$(findmnt -n -o SOURCE -T / | head -1)
    log_success "btrfs filesystem detected: $btrfs_dev"
}

setup_mount_point() {
    log_info "Ensuring mount point directory exists: $OPDBUS_ROOT"
    mkdir -p "$OPDBUS_ROOT"
    log_success "Mount point ready: $OPDBUS_ROOT"
}

print_summary() {
    echo
    echo "============================================================================"
    echo -e "${GREEN}btrfs LAYOUT — READY${NC}"
    echo "============================================================================"
    echo
    echo "  Host filesystem: btrfs"
    echo "  Deploy root:     $OPDBUS_ROOT"
    echo
    echo "Next step:"
    echo "  sudo deploy/runit/build-golden.sh          # prepare golden subvolume only"
    echo "  sudo deploy/runit/build-golden.sh --dry-run # review first"
    echo
    echo "build-golden.sh creates the golden subvolume at $OPDBUS_ROOT/golden,"
    echo "populates it with release binaries, runit service definitions, and config."
    echo "A separate operator step snapshots it read-only and deploys that snapshot"
    echo "to the target with btrfs send/receive; neither script performs the transfer."
    echo
}

# ============================================================================
# MAIN
# ============================================================================

main() {
    echo
    echo "============================================================================"
    echo "OP-DBUS btrfs LAYOUT SETUP"
    echo "============================================================================"
    echo

    if [[ $EUID -ne 0 ]]; then
        log_error "This script must be run as root"
        exit 1
    fi

    check_btrfs
    setup_mount_point
    print_summary
}

main "$@"
