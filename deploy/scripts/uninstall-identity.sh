#!/bin/sh
# uninstall-identity.sh — safe teardown of an identity workspace CT.
#
# Order (mandatory if fstorage was btrfs device-add'd):
#   1) btrfs device delete <loop|img> <seed-rootfs>
#   2) losetup -d <loop>
#   3) incus stop + incus delete
#   4) optional host crumbs (socket reg, fstorage, keys, vault, mesh peer)
#
# Never deletes infra CTs (assistant, xray, cozo, …).
#
# Usage:
#   sudo ./uninstall-identity.sh <role|session-id>
#   sudo ./uninstall-identity.sh chatbot
#   sudo ./uninstall-identity.sh jeremy --purge-keys --remove-mesh-peer
#   sudo ./uninstall-identity.sh --all --yes
#
# Options:
#   --yes                 skip confirmation
#   --keep-keys           leave /var/lib/opdbus-runtime/identities/<role>/ (default)
#   --purge-keys          remove identity runtime dir (keys, fstorage, memory-seed)
#   --purge-fstorage      remove fstorage.img only (implies detach first)
#   --keep-fstorage       keep fstorage.img on host after CT delete (default unless --purge-*)
#   --remove-mesh-peer    wg set 3tched peer <pubkey> remove (+ note conf edit)
#   --purge-vault         drop pubkey entry from /etc/ghostbridge/user-vault.json
#   --purge-socket-reg    remove socket-registrations/<session-id>.json (default on)
#   --keep-socket-reg     leave registration metadata
#   --session-id UUID     override session/container name
#   --pubkey B64          for mesh/vault when identity.json missing
#   --identity-dir PATH   override runtime dir
#   --storage-pool NAME   default: default
#   --dry-run             print actions only
#
set -eu

RUNTIME_ROOT="${OPDBUS_IDENTITY_ROOT:-/var/lib/opdbus-runtime/identities}"
SOCKET_REG_DIR="${OPDBUS_SOCKET_REG_DIR:-/var/lib/opdbus-runtime/socket-registrations}"
VAULT_PATH="${GHOSTBRIDGE_VAULT:-/etc/ghostbridge/user-vault.json}"
MESH_IFACE="${WG_MESH_IFACE:-3tched}"
STORAGE_POOL="${IDENTITY_STORAGE_POOL:-default}"

ROLE=""
SESSION_ID=""
PUBKEY=""
IDENTITY_DIR=""
YES=false
DRY_RUN=false
PURGE_KEYS=false
PURGE_FSTORAGE=false
REMOVE_MESH=false
PURGE_VAULT=false
PURGE_SOCKET_REG=true
ALL=false

die() { echo "ERROR: $*" >&2; exit 1; }
info() { echo "    $*"; }
run() {
    if [ "$DRY_RUN" = "true" ]; then
        echo "    DRY: $*"
        return 0
    fi
    # shellcheck disable=SC2068
    "$@"
}

usage() {
    sed -n '2,40p' "$0"
    exit 0
}

if [ $# -eq 0 ]; then
    usage
fi

while [ $# -gt 0 ]; do
    case "$1" in
        -h|--help) usage ;;
        --all) ALL=true ;;
        --yes|-y) YES=true ;;
        --dry-run) DRY_RUN=true ;;
        --keep-keys) PURGE_KEYS=false ;;
        --purge-keys) PURGE_KEYS=true; PURGE_FSTORAGE=true ;;
        --purge-fstorage) PURGE_FSTORAGE=true ;;
        --keep-fstorage) PURGE_FSTORAGE=false ;;
        --remove-mesh-peer) REMOVE_MESH=true ;;
        --purge-vault) PURGE_VAULT=true ;;
        --purge-socket-reg) PURGE_SOCKET_REG=true ;;
        --keep-socket-reg) PURGE_SOCKET_REG=false ;;
        --session-id) shift; SESSION_ID="${1:?}" ;;
        --pubkey) shift; PUBKEY="${1:?}" ;;
        --identity-dir) shift; IDENTITY_DIR="${1:?}" ;;
        --storage-pool) shift; STORAGE_POOL="${1:?}" ;;
        --*)
            die "unknown option: $1"
            ;;
        *)
            if [ -z "$ROLE" ]; then
                ROLE="$1"
            else
                die "unexpected arg: $1"
            fi
            ;;
    esac
    shift
done

need_cmd() { command -v "$1" >/dev/null 2>&1 || die "missing command: $1"; }
need_cmd incus
need_cmd wg

container_rootfs() {
    echo "/var/lib/incus/storage-pools/${STORAGE_POOL}/containers/${1}/rootfs"
}

fstorage_loop_for() {
    losetup -j "$1" 2>/dev/null | head -1 | cut -d: -f1 || true
}

# Known role → pubkey / session_id (prefer-existing map)
resolve_known() {
    _role="$1"
    case "$_role" in
        jeremy|user)
            ROLE=jeremy
            [ -n "$PUBKEY" ] || PUBKEY='GEMLT/+I81zs5HDPsOF22ntqGf71OEy6eFKMm7P7Dzk='
            [ -n "$SESSION_ID" ] || SESSION_ID='f036f8d8-aabb-c5f2-49c9-18dac19f41ea'
            ;;
        chatbot|assistant-identity)
            ROLE=chatbot
            [ -n "$PUBKEY" ] || PUBKEY='VaRh9EUieQxA3zIoOj3qNiNIqZoPGpqztPU4muyF1zM='
            [ -n "$SESSION_ID" ] || SESSION_ID='bea37ecb-92be-197c-660f-09e806f1a34f'
            ;;
        *)
            # Maybe ROLE is already a UUID session id
            case "$_role" in
                *-[0-9a-fA-F]*-*)
                    SESSION_ID="$_role"
                    ROLE="${ROLE:-unknown}"
                    ;;
            esac
            ;;
    esac
}

load_identity_meta() {
    _dir="$1"
    if [ -f "$_dir/identity.json" ] && command -v python3 >/dev/null 2>&1; then
        _meta=$(python3 - "$_dir/identity.json" <<'PY'
import json, sys
try:
    d = json.load(open(sys.argv[1]))
except Exception:
    sys.exit(0)
def clean(s):
    return (s or "").replace("\n", "").replace("'", "")
print(clean(d.get("session_id")))
print(clean(d.get("pubkey")))
print(clean(d.get("role")))
PY
)
        _ms=$(printf '%s\n' "$_meta" | sed -n '1p')
        _mp=$(printf '%s\n' "$_meta" | sed -n '2p')
        _mr=$(printf '%s\n' "$_meta" | sed -n '3p')
        [ -n "$_ms" ] && [ -z "$SESSION_ID" ] && SESSION_ID="$_ms"
        [ -n "$_mp" ] && [ -z "$PUBKEY" ] && PUBKEY="$_mp"
        [ -n "$_mr" ] && [ -z "$ROLE" ] && ROLE="$_mr"
        [ -n "$_mr" ] && [ "$ROLE" = "unknown" ] && ROLE="$_mr"
    fi
    if [ -z "$PUBKEY" ] && [ -f "$_dir/public.key" ]; then
        PUBKEY=$(tr -d ' \n' < "$_dir/public.key")
    fi
}

detach_fstorage_before_delete() {
    _name="$1"
    _img="$2"
    _rootfs=$(container_rootfs "$_name")
    _timeout="${DETACH_FSTORAGE_TIMEOUT:-120}"

    info "[1/4] detach fstorage from btrfs array (before CT delete)"

    if [ ! -f "$_img" ]; then
        info "no fstorage image at $_img — skip device delete"
        if incus info "$_name" >/dev/null 2>&1; then
            run incus stop "$_name" --timeout 30 2>/dev/null || run incus stop "$_name" --force 2>/dev/null || true
        fi
        return 0
    fi

    _loop=$(fstorage_loop_for "$_img")

    if [ ! -d "$_rootfs" ]; then
        info "rootfs missing — skip btrfs device delete"
        if [ -n "$_loop" ]; then
            run losetup -d "$_loop" 2>/dev/null || true
            info "detached orphan loop $_loop"
        fi
        return 0
    fi

    if [ -z "$_loop" ]; then
        if btrfs filesystem show "$_rootfs" 2>/dev/null | grep -qF "$_img"; then
            _loop=$(losetup -f --show "$_img" 2>/dev/null) || true
            info "re-looped $_img as ${_loop:-failed}"
        fi
    fi

    _dev="${_loop:-}"
    if [ -z "$_dev" ] && btrfs filesystem show "$_rootfs" 2>/dev/null | grep -qF "$_img"; then
        _dev="$_img"
    fi

    if [ -n "$_dev" ] && btrfs filesystem show "$_rootfs" 2>/dev/null | grep -qE "$(basename "$_dev")|$_dev|$_img"; then
        info "stop CT so rootfs is quiet"
        run incus stop "$_name" --timeout 30 2>/dev/null || run incus stop "$_name" --force 2>/dev/null || true
        sleep 1
        info "btrfs device delete $_dev → $_rootfs (timeout ${_timeout}s)"
        if [ "$DRY_RUN" = "true" ]; then
            info "DRY: would btrfs device delete"
        elif command -v timeout >/dev/null 2>&1; then
            if ! timeout "$_timeout" btrfs device delete "$_dev" "$_rootfs"; then
                die "btrfs device delete failed/timeout — NOT deleting CT (avoid host hang). Fix manually then re-run."
            fi
            info "ok: removed from btrfs array"
        else
            btrfs device delete "$_dev" "$_rootfs" || die "btrfs device delete failed"
            info "ok: removed from btrfs array"
        fi
    else
        info "fstorage not in btrfs array — ok"
        if incus info "$_name" >/dev/null 2>&1; then
            run incus stop "$_name" --timeout 30 2>/dev/null || run incus stop "$_name" --force 2>/dev/null || true
        fi
    fi

    _loop=$(fstorage_loop_for "$_img")
    if [ -n "$_loop" ]; then
        run losetup -d "$_loop" 2>/dev/null || info "WARN: could not detach $_loop"
        info "detached loop ${_loop:-}"
    fi
    return 0
}

purge_vault_entry() {
    _pk="$1"
    [ -n "$_pk" ] || return 0
    [ -f "$VAULT_PATH" ] || { info "no vault at $VAULT_PATH"; return 0; }
    if ! command -v python3 >/dev/null 2>&1; then
        info "WARN: python3 missing — edit vault by hand: $VAULT_PATH"
        return 0
    fi
    info "remove pubkey from vault $VAULT_PATH"
    if [ "$DRY_RUN" = "true" ]; then
        info "DRY: vault purge $_pk"
        return 0
    fi
    python3 - "$VAULT_PATH" "$_pk" <<'PY'
import json, sys
path, pk = sys.argv[1], sys.argv[2]
with open(path) as f:
    data = json.load(f)
users = data.get("users") or {}
if pk in users:
    del users[pk]
    data["users"] = users
    import os
    tmp = path + ".tmp"
    with open(tmp, "w") as f:
        json.dump(data, f, indent=2)
        f.write("\n")
    os.replace(tmp, path)
    print("removed", pk)
else:
    print("pubkey not in vault")
PY
}

remove_mesh_peer() {
    _pk="$1"
    [ -n "$_pk" ] || return 0
    info "remove mesh peer on $MESH_IFACE"
    if [ "$DRY_RUN" = "true" ]; then
        info "DRY: wg set $MESH_IFACE peer $_pk remove"
        return 0
    fi
    if wg show "$MESH_IFACE" 2>/dev/null | grep -q "$_pk"; then
        wg set "$MESH_IFACE" peer "$_pk" remove && info "wg peer removed" || info "WARN: wg set remove failed"
    else
        info "peer not active on $MESH_IFACE"
    fi
    conf="/etc/wireguard/${MESH_IFACE}.conf"
    if [ -f "$conf" ] && grep -q "$_pk" "$conf"; then
        info "NOTE: remove [Peer] block for $_pk from $conf manually (or re-write conf)"
    fi
}

uninstall_one() {
    # expects ROLE, SESSION_ID, PUBKEY, IDENTITY_DIR set
    echo ""
    echo "=== Uninstall identity ==="
    echo "    role        : $ROLE"
    echo "    session_id  : $SESSION_ID"
    echo "    pubkey      : ${PUBKEY:-<none>}"
    echo "    identity_dir: $IDENTITY_DIR"
    echo "    storage_pool: $STORAGE_POOL"
    echo "    purge_keys  : $PURGE_KEYS"
    echo "    purge_fstore: $PURGE_FSTORAGE"
    echo "    remove_mesh : $REMOVE_MESH"
    echo "    purge_vault : $PURGE_VAULT"
    echo ""

    case "$SESSION_ID" in
        assistant|cozo|xray|qdrant|netmaker|mail-3tched)
            die "refusing to delete infra container name: $SESSION_ID"
            ;;
    esac

    if [ "$YES" != "true" ] && [ "$DRY_RUN" != "true" ]; then
        printf "Delete identity CT %s (%s)? [y/N] " "$SESSION_ID" "$ROLE"
        read -r ans
        case "$ans" in
            y|Y|yes|YES) ;;
            *) echo "aborted."; return 1 ;;
        esac
    fi

    IMG="${IDENTITY_DIR}/fstorage.img"

    # 1–2) fstorage off array + loop
    if incus info "$SESSION_ID" >/dev/null 2>&1; then
        detach_fstorage_before_delete "$SESSION_ID" "$IMG" || die "detach failed — CT not deleted"
        info "[2/4] incus delete $SESSION_ID"
        run incus delete "$SESSION_ID" --force
    else
        info "[2/4] container $SESSION_ID not present"
        # still free orphan loop
        _loop=$(fstorage_loop_for "$IMG")
        if [ -n "$_loop" ]; then
            run losetup -d "$_loop" 2>/dev/null || true
        fi
    fi

    # 3) socket registration
    if [ "$PURGE_SOCKET_REG" = "true" ]; then
        reg="${SOCKET_REG_DIR}/${SESSION_ID}.json"
        if [ -f "$reg" ]; then
            info "[3/4] remove socket reg $reg"
            run rm -f "$reg"
        else
            info "[3/4] no socket reg file"
        fi
    fi

    # 4) host crumbs
    info "[4/4] host crumbs"
    if [ "$PURGE_FSTORAGE" = "true" ] && [ -f "$IMG" ]; then
        info "remove fstorage $IMG"
        run rm -f "$IMG"
    fi
    if [ "$PURGE_KEYS" = "true" ] && [ -d "$IDENTITY_DIR" ]; then
        info "remove identity dir $IDENTITY_DIR"
        run rm -rf "$IDENTITY_DIR"
    else
        info "keeping identity dir (keys/fstorage) at $IDENTITY_DIR"
    fi
    if [ "$REMOVE_MESH" = "true" ]; then
        remove_mesh_peer "$PUBKEY"
    fi
    if [ "$PURGE_VAULT" = "true" ]; then
        purge_vault_entry "$PUBKEY"
    fi

    echo "=== Done: $ROLE / $SESSION_ID ==="
}

# ── main ─────────────────────────────────────────────────────────────────────

if [ "$ALL" = "true" ]; then
    [ "$YES" = "true" ] || [ "$DRY_RUN" = "true" ] || {
        printf "Uninstall ALL known identities (jeremy + chatbot)? [y/N] "
        read -r ans
        case "$ans" in y|Y|yes|YES) ;; *) echo "aborted."; exit 1 ;; esac
        YES=true
    }
    for r in jeremy chatbot; do
        ROLE=""; SESSION_ID=""; PUBKEY=""; IDENTITY_DIR=""
        resolve_known "$r"
        IDENTITY_DIR="${IDENTITY_DIR:-$RUNTIME_ROOT/$ROLE}"
        load_identity_meta "$IDENTITY_DIR"
        uninstall_one || true
    done
    exit 0
fi

[ -n "$ROLE" ] || die "role or session-id required (or --all)"

# If ROLE looks like UUID, treat as session id
case "$ROLE" in
    *-[0-9a-fA-F]*-*)
        [ -n "$SESSION_ID" ] || SESSION_ID="$ROLE"
        # try match known
        if [ "$SESSION_ID" = "f036f8d8-aabb-c5f2-49c9-18dac19f41ea" ]; then
            ROLE=jeremy
        elif [ "$SESSION_ID" = "bea37ecb-92be-197c-660f-09e806f1a34f" ]; then
            ROLE=chatbot
        fi
        ;;
    *)
        resolve_known "$ROLE"
        ;;
esac

IDENTITY_DIR="${IDENTITY_DIR:-$RUNTIME_ROOT/$ROLE}"
load_identity_meta "$IDENTITY_DIR"

[ -n "$SESSION_ID" ] || die "could not resolve session_id (pass --session-id)"
# Infra safety: UUID-only names for identity CTs; block known infra
case "$SESSION_ID" in
    assistant|cozo|xray|qdrant|netmaker|mail-3tched)
        die "refusing infra name"
        ;;
esac

uninstall_one
