#!/usr/bin/env bash
# golden.sh — approved golden release: the pool seed IS the release image.
#
# The release image is the seed subvolume on the pool (default
# images/@antix-seed on 3tched-pool). Nothing is sent, received, or copied to
# another filesystem. work is a writable CoW snapshot of the seed on the SAME
# btrfs; cargo builds inside a chroot of work. work is then snapshotted
# read-only into the live slot; that snapshot (or, before the first release,
# the seed itself) is the OverlayFS lower dir on the live rootfs. There are
# exactly two slots: live and rollback. Cutting a release demotes old live to
# rollback and deletes the previous rollback.
#
# This is not op-cache BtrfsCache (one subvolume per category: embeddings,
# blocks, queries, diffs, kv) and it is not the historical application-payload
# tree at /opt/op-dbus/golden or the @opdbus-system-a/b transfer slots. Those
# layouts are left alone.
#
# Dry-run by default. Any create/delete/snapshot/mount/build requires --apply.
# --apply still refuses to delete or write the seeds (images/@seed,
# images/@antix-seed) and Incus image/container paths. The only paths it
# creates or deletes are work, slots/{live,rollback,.incoming-*}, the overlay
# upper/work dirs, and state.json under GOLDEN_ROOT.
#
# Usage:
#   deploy/lib/golden/golden.sh [flags] <verb>
#   verbs: status | plan | init | build | cut | release | rotate | rollback | mount-overlay | activate (install)
#   flags: --apply  --pool PATH  --seed PATH  --root PATH  --lower auto|live|seed
#          --repo PATH  --jobs N  --with-build  --mount PATH  --dry-run
#
# Defaults (env overrides in brackets):
#   pool  /var/lib/incus/storage-pools/3tched-pool     [GOLDEN_POOL_ROOT]
#   seed  <pool>/images/@antix-seed                    [GOLDEN_SEED]
#   root  <pool>/golden-os  (same btrfs as the seed)   [GOLDEN_ROOT]
#   OS dir inside seed/slots: rootfs (Incus image)     [GOLDEN_OS_SUBDIR]
set -euo pipefail
PATH=/usr/sbin:/usr/bin:/sbin:/bin
export PATH
umask 022
LC_ALL=C
export LC_ALL

SCRIPT_PATH=$(readlink -f -- "${BASH_SOURCE[0]}")
SCRIPT_DIR=$(CDPATH='' cd -- "$(dirname -- "$SCRIPT_PATH")" && pwd)
if [ -f "$SCRIPT_DIR/../../../Cargo.toml" ]; then
    DEFAULT_REPO=$(CDPATH='' cd -- "$SCRIPT_DIR/../../.." && pwd)
else
    DEFAULT_REPO=${OP_DBUS_ROOT:-/srv/git/odbus-verified}
fi

DEFAULT_POOL_ROOT=/var/lib/incus/storage-pools/3tched-pool
DEFAULT_SEED_NAME=images/@antix-seed
DEFAULT_ROOT_NAME=golden-os

APPLY=0
WITH_BUILD=0
TOOLCHAIN=0
MOUNT_POINT=
LOWER_MODE=${GOLDEN_LOWER:-auto}
JOBS=${GOLDEN_CARGO_JOBS:-2}
POOL_ROOT=${GOLDEN_POOL_ROOT:-$DEFAULT_POOL_ROOT}
# SEED and ROOT default from POOL_ROOT in resolve_layout (after flags).
SEED=${GOLDEN_SEED:-}
ROOT=${GOLDEN_ROOT:-}
OS_SUBDIR=${GOLDEN_OS_SUBDIR-rootfs}
REPO=${GOLDEN_REPO:-$DEFAULT_REPO}
VERB=

usage() {
    cat <<'EOF'
Usage: golden.sh [flags] <verb>

Verbs:
  status         Show seed, work OS and live/rollback slots (read-only)
  plan           Print the full release plan without changing anything (default)
  init           CoW-snapshot the seed into the writable work subvolume (same btrfs)
  build          cargo build --workspace --release inside the work OS chroot
  cut            Snapshot work, demote live to rollback, drop previous rollback
  rollback       Swap live and rollback (promote the rollback snapshot)
  release        Snapshot work, rotate live to rollback, plan the live overlay
  rotate         Same slot rotation as release (no mount)
  mount-overlay  Mount the live release (or the seed) as the OverlayFS lower dir
  activate       (alias install) Stage live-slot release binaries into a versioned
                 dir and flip enabled services one at a time with health checks
                 and automatic rollback (table: activate-services.tsv)

Flags:
  --dry-run      Print the plan only (default)
  --apply        Perform the verb (never the default)
  --pool PATH    Pool mount holding the seed (default
                 /var/lib/incus/storage-pools/3tched-pool or $GOLDEN_POOL_ROOT)
  --seed PATH    Seed subvolume = release image (default <pool>/images/@antix-seed
                 or $GOLDEN_SEED). Never written or deleted. --from is an alias.
  --root PATH    Release root for work/slots (default <pool>/golden-os or
                 $GOLDEN_ROOT). Must be on the same btrfs as the seed.
  --lower MODE   Overlay lower dir: auto (live release, else the seed), live, seed
  --repo PATH    Repo to export into the chroot (default: this checkout)
  --jobs N       cargo -j (default 2; keep this modest)
  --with-build   For cut/release: run the chroot build before snapshotting
  --toolchain    For build: if the work OS has no /root/.cargo/bin/cargo, apt-install
                 build deps and rustup (minimal) inside the work chroot first
  --mount PATH   For mount-overlay: overlay mount point (not / or other host roots)

There is no send/receive and no copy of the seed to another filesystem.

Dry-run is the default. --apply is required to create, delete, snapshot, mount,
or build. The script will not delete images/@seed, Incus pool data, BtrfsCache
category subvolumes, or anything outside the configured work/live/rollback
paths.
EOF
}

die() { printf 'golden: %s\n' "$*" >&2; exit 1; }
log() { printf 'golden: %s\n' "$*"; }
warn() { printf 'golden: warning: %s\n' "$*" >&2; }

parse_args() {
    while [ "$#" -gt 0 ]; do
        case "$1" in
            --apply) APPLY=1 ;;
            --dry-run) APPLY=0 ;;
            --with-build) WITH_BUILD=1 ;;
            --toolchain) TOOLCHAIN=1 ;;
            --send-receive)
                die "--send-receive was removed: the release image is the seed on the pool; init snapshots it on the same btrfs (see --seed / --root)"
                ;;
            --pool)
                [ "$#" -ge 2 ] || die "missing value for --pool"
                POOL_ROOT=$2
                shift
                ;;
            --seed|--from)
                [ "$#" -ge 2 ] || die "missing value for $1"
                SEED=$2
                shift
                ;;
            --lower)
                [ "$#" -ge 2 ] || die "missing value for --lower"
                LOWER_MODE=$2
                shift
                ;;
            --root)
                [ "$#" -ge 2 ] || die "missing value for --root"
                ROOT=$2
                shift
                ;;
            --repo)
                [ "$#" -ge 2 ] || die "missing value for --repo"
                REPO=$2
                shift
                ;;
            --jobs)
                [ "$#" -ge 2 ] || die "missing value for --jobs"
                JOBS=$2
                shift
                ;;
            --mount)
                [ "$#" -ge 2 ] || die "missing value for --mount"
                MOUNT_POINT=$2
                shift
                ;;
            -h|--help) usage; exit 0 ;;
            --*) die "unknown flag: $1" ;;
            *)
                [ -z "$VERB" ] || die "unexpected argument: $1"
                VERB=$1
                ;;
        esac
        shift
    done
    [ -n "$VERB" ] || VERB=plan
    case "$VERB" in
        status|plan|init|build|cut|rollback|release|rotate|mount-overlay|activate) ;;
        install) VERB=activate ;;
        *) die "unknown verb: $VERB (try --help)" ;;
    esac
    case "$JOBS" in
        ''|*[!0-9]*|0) die "jobs must be a positive integer, got: $JOBS" ;;
    esac
    case "$LOWER_MODE" in
        auto|live|seed) ;;
        *) die "--lower must be auto, live, or seed, got: $LOWER_MODE" ;;
    esac
}

# Logical paths under ROOT. resolve_layout is called after parse_args.
WORK=
SLOTS=
LIVE=
ROLLBACK=
STATE=
OVERLAY_UPPER=
OVERLAY_WORK=
INCOMING_PREFIX=

resolve_layout() {
    case "$POOL_ROOT" in
        /*) ;;
        *) die "GOLDEN_POOL_ROOT must be an absolute path: $POOL_ROOT" ;;
    esac
    POOL_ROOT=$(normalize_path "$POOL_ROOT")
    POOL_ROOT=${POOL_ROOT%/}
    [ -n "$SEED" ] || SEED=$POOL_ROOT/$DEFAULT_SEED_NAME
    [ -n "$ROOT" ] || ROOT=$POOL_ROOT/$DEFAULT_ROOT_NAME
    case "$SEED" in
        /*) ;;
        *) die "seed must be an absolute path: $SEED" ;;
    esac
    SEED=$(normalize_path "$SEED")
    SEED=${SEED%/}
    case "$OS_SUBDIR" in
        /*|*..*) die "GOLDEN_OS_SUBDIR must be a relative path without ..: $OS_SUBDIR" ;;
    esac
    OS_SUBDIR=${OS_SUBDIR%/}
    case "$ROOT" in
        /*) ;;
        *) die "GOLDEN_ROOT must be an absolute path: $ROOT" ;;
    esac
    # Do not require the root to exist (init creates it). Collapse . and ...
    ROOT=$(normalize_path "$ROOT")
    ROOT=${ROOT%/}
    [ -n "$ROOT" ] || die "GOLDEN_ROOT resolved empty"
    WORK=$ROOT/work
    SLOTS=$ROOT/slots
    LIVE=$SLOTS/live
    ROLLBACK=$SLOTS/rollback
    STATE=$ROOT/state.json
    OVERLAY_UPPER=$ROOT/overlay-upper
    OVERLAY_WORK=$ROOT/overlay-work
    INCOMING_PREFIX=$SLOTS/.incoming-
    if [ -n "$MOUNT_POINT" ]; then
        case "$MOUNT_POINT" in
            /*) ;;
            *) die "--mount must be an absolute path" ;;
        esac
        MOUNT_POINT=$(normalize_path "$MOUNT_POINT")
    fi
}

is_absolute() {
    case "$1" in /*) return 0 ;; *) return 1 ;; esac
}

# Path protection: never delete or overwrite these, even with --apply.
# The seed may be the source of a CoW snapshot into work; it is never a destination.
normalize_path() {
    # Resolve . and .. without requiring the path to exist. Keep "/" intact.
    # -s: lexical only. Never lstat path components, which can block in D
    # state on a hung pool (3tched-pool / ublkb1).
    local p=$1
    [ -n "$p" ] || return 1
    realpath -s -m -- "$p"
}

# The OS tree inside a seed or slot (Incus image volumes keep it in rootfs/).
os_dir_of() {
    if [ -n "$OS_SUBDIR" ]; then
        printf '%s/%s' "$1" "$OS_SUBDIR"
    else
        printf '%s' "$1"
    fi
}

# Run a read-only probe with a hard timeout so status/plan never hang on a
# pool whose I/O is stuck. Exit 124/137 means "unknown".
probe() {
    timeout -s KILL "${GOLDEN_PROBE_TIMEOUT:-5}" "$@"
}

# 0 = directory present, 1 = absent, 2 = could not tell (probe timed out).
dir_state() {
    local rc=0
    probe test -d "$1" || rc=$?
    case "$rc" in
        0) return 0 ;;
        1) return 1 ;;
        *) return 2 ;;
    esac
}

dir_present() {
    local rc=0
    dir_state "$1" || rc=$?
    if [ "$rc" = 2 ]; then
        warn "could not read $1 within ${GOLDEN_PROBE_TIMEOUT:-5}s (pool I/O may be hung); treating as absent for this plan"
    fi
    [ "$rc" = 0 ]
}

# Incus-managed names directly under a storage pool. golden never uses them.
incus_pool_reserved() {
    case "$1" in
        images|containers|containers-snapshots|virtual-machines|virtual-machines-snapshots|custom|custom-snapshots|buckets|backups) return 0 ;;
    esac
    return 1
}

# GOLDEN_ROOT may be a dedicated directory directly under the pool (same btrfs
# as the seed), e.g. <pool>/golden-os, but never an Incus-managed directory.
root_is_pool_release_dir() {
    local r=${1:-${ROOT:-}} parent name
    [ -n "$r" ] && [ -n "${POOL_ROOT:-}" ] || return 1
    r=$(normalize_path "$r")
    r=${r%/}
    parent=${r%/*}
    name=${r##*/}
    [ "$parent" = "$POOL_ROOT" ] || return 1
    [ -n "$name" ] || return 1
    case "$name" in
        @*|.*) return 1 ;;
    esac
    incus_pool_reserved "$name" && return 1
    return 0
}

path_is_protected() {
    local p n
    p=$(normalize_path "$1")
    [ "$p" = / ] && return 0
    p=${p%/}
    n=${p##*/}
    case "$p" in
        */images/@seed|*/images/@seed/*|*/images/@antix-seed|*/images/@antix-seed/*) return 0 ;;
    esac
    # The configured seed is the release image: never written or deleted.
    if [ -n "${SEED:-}" ] && path_under "$p" "$SEED"; then
        return 0
    fi
    # The dedicated release root under the pool (and its children) is ours.
    if [ -n "${ROOT:-}" ] && root_is_pool_release_dir "$ROOT" && path_under "$p" "$ROOT"; then
        return 1
    fi
    case "$p" in
        /var/lib/incus|/var/lib/incus/*) return 0 ;;
        /var/lib/op-dbus/cache|/var/lib/op-dbus/cache/*|/var/lib/op-dbus/@cache-snapshots|/var/lib/op-dbus/@cache-snapshots/*) return 0 ;;
        /var/lib/opdbus/cache|/var/lib/opdbus/cache/*) return 0 ;;
        */@cache-snapshots|*/@cache-snapshots/*) return 0 ;;
        */@seed-send|*/@seed-send/*) return 0 ;;
        */@opdbus-system-a|*/@opdbus-system-b|/opt/op-dbus/golden|/opt/op-dbus/golden/*|/opt/op-dbus/overlay-base|/opt/op-dbus/overlay-base/*|/opt/op-dbus/release-send|/opt/op-dbus/release-send/*) return 0 ;;
        */@home|*/@git|*/@build|*/@release-a|*/@upper) return 0 ;;
        /|/usr|/usr/*|/etc|/etc/*|/home|/home/*|/boot|/boot/*|/var|/proc|/sys|/dev|/dev/*) return 0 ;;
    esac
    case "$n" in
        @seed|@antix-seed|@seed-send)
            return 0
            ;;
        embeddings|blocks|queries|diffs|kv)
            case "$p" in
                */cache/"$n"|*/op-dbus/cache/"$n"|*/op-cache/*) return 0 ;;
            esac
            ;;
    esac
    return 1
}

path_under() {
    local child parent
    child=$(normalize_path "$1")
    parent=$(normalize_path "$2")
    child=${child%/}
    parent=${parent%/}
    [ "$parent" = / ] && return 0
    [ "$child" = "$parent" ] && return 0
    case "$child" in
        "$parent"/*) return 0 ;;
    esac
    return 1
}

# Only these paths may be created, renamed, or deleted.
path_is_owned() {
    local p=$1
    p=${p%/}
    path_under "$p" "$ROOT" || return 1
    path_is_protected "$p" && return 1
    case "$p" in
        "$ROOT"|"$SLOTS"|"$WORK"|"$LIVE"|"$ROLLBACK"|"$STATE"|"$OVERLAY_UPPER"|"$OVERLAY_WORK") return 0 ;;
        "$INCOMING_PREFIX"*) return 0 ;;
        "$SLOTS"/.incoming-*) return 0 ;;
    esac
    return 1
}

backing_device() {
    # Mount table only (longest TARGET prefix); never stats the path, so a
    # root on a hung pool does not block. Empty if unknown.
    local path line target source best_len=0 best='' tlen
    path=$(normalize_path "$1")
    while IFS= read -r line; do
        [ -n "$line" ] || continue
        target=${line%% *}
        source=${line#* }
        [ "$source" != "$line" ] || source=''
        target=$(printf '%b' "$target")
        if path_under "$path" "$target"; then
            tlen=${#target}
            if [ "$tlen" -ge "$best_len" ]; then
                best_len=$tlen
                best=$source
            fi
        fi
    done <<EOF
$(if [ -n "${GOLDEN_MOUNTS:-}" ]; then printf '%s\n' "$GOLDEN_MOUNTS"; else findmnt -rn -o TARGET,SOURCE 2>/dev/null || true; fi)
EOF
    printf '%s' "$best"
}

backing_is_ublk() {
    local source
    source=$(backing_device "$1")
    case "$source" in
        *ublkb*|*ublka*|/dev/ublk*) return 0 ;;
    esac
    return 1
}

# Identify a path's btrfs filesystem from the mount table only (longest
# TARGET prefix). This does not stat the path itself, so a seed on a busy
# ublk device can be compared with /opt/op-dbus without touching the seed.
btrfs_mount_table() {
    if [ -n "${GOLDEN_BTRFS_MOUNTS:-}" ]; then
        printf '%s\n' "$GOLDEN_BTRFS_MOUNTS"
    else
        # TARGET+SOURCE only. Requesting UUID makes findmnt/blkid probe
        # ublkb1 and can hang; device names are enough to tell filesystems apart.
        findmnt -t btrfs -n -o TARGET,SOURCE 2>/dev/null || true
    fi
}

# Extract KEY="value" from a findmnt --pairs line without eval.
findmnt_pair_value() {
    local line=$1 key=$2 prefix rest
    prefix="$key=\""
    case "$line" in
        *"$prefix"*)
            rest=${line#*"$prefix"}
            printf '%s' "${rest%%\"*}"
            ;;
    esac
}

normalize_btrfs_source() {
    # /dev/sdb3[/@opdbus-prep] -> /dev/sdb3
    local s=$1
    s=${s%%[*}
    printf '%s' "$s"
}

btrfs_ident_for_path() {
    local path line target uuid source best_len=0 ident="" tlen
    path=$(normalize_path "$1")
    while IFS= read -r line; do
        [ -n "$line" ] || continue
        target='' uuid='' source=''
        case "$line" in
            TARGET=*)
                target=$(findmnt_pair_value "$line" TARGET)
                uuid=$(findmnt_pair_value "$line" UUID)
                source=$(findmnt_pair_value "$line" SOURCE)
                ;;
            *)
                # Test table: TARGET UUID SOURCE. Live findmnt: TARGET SOURCE.
                read -r target f2 f3 <<FRG
$line
FRG
                uuid='' source=''
                case "$f3" in
                    '') source=$f2 ;;
                    *) uuid=$f2; source=$f3 ;;
                esac
                ;;
        esac
        [ -n "$target" ] || continue
        if path_under "$path" "$target" || [ "$path" = "$target" ]; then
            tlen=${#target}
            if [ "$tlen" -ge "$best_len" ]; then
                best_len=$tlen
                if [ -n "$uuid" ] && [ "${uuid#/}" = "$uuid" ]; then
                    ident="uuid:$uuid"
                else
                    ident="dev:$(normalize_btrfs_source "$source")"
                fi
            fi
        fi
    done <<EOF
$(btrfs_mount_table)
EOF
    printf '%s' "$ident"
}

same_btrfs_filesystem() {
    local a b
    a=$(btrfs_ident_for_path "$1")
    b=$(btrfs_ident_for_path "$2")
    [ -n "$a" ] && [ -n "$b" ] && [ "$a" = "$b" ]
}

# The seed is the release image. It must not be work, a slot, or an overlay
# directory, and must not sit inside GOLDEN_ROOT.
seed_path_allowed() {
    local seed=$1
    seed=$(normalize_path "$seed")
    case "$seed" in
        "$ROOT"|"$WORK"|"$LIVE"|"$ROLLBACK"|"$SLOTS"|"$STATE"|"$OVERLAY_UPPER"|"$OVERLAY_WORK")
            die "seed cannot be $seed"
            ;;
    esac
    if path_under "$seed" "$ROOT"; then
        die "seed cannot be inside GOLDEN_ROOT ($ROOT): $seed"
    fi
    if path_under "$ROOT" "$seed"; then
        die "GOLDEN_ROOT cannot be inside the seed ($seed): $ROOT"
    fi
}

# work/slots are CoW snapshots of the seed: both must be on one btrfs.
# Identified from the mount table only, so the seed is not touched.
require_same_btrfs_as_seed() {
    local src_ident dst_ident
    src_ident=$(btrfs_ident_for_path "$SEED")
    dst_ident=$(btrfs_ident_for_path "$ROOT")
    if [ -z "$src_ident" ] || [ -z "$dst_ident" ]; then
        die "cannot determine the btrfs filesystem for the seed ($SEED => ${src_ident:-unknown}) and GOLDEN_ROOT ($ROOT => ${dst_ident:-unknown}); refusing to plan a snapshot"
    fi
    [ "$src_ident" = "$dst_ident" ] && return 0
    cat >&2 <<EOF
golden: cannot CoW-snapshot the seed $SEED onto $WORK: they are on different btrfs filesystems
  seed:   $src_ident  ($SEED)
  target: $dst_ident  ($ROOT)
  btrfs subvolume snapshot only works on the same filesystem.
  The release image is the seed itself; golden does not send/receive or copy it.
  Put GOLDEN_ROOT on the seed's filesystem (default: $POOL_ROOT/$DEFAULT_ROOT_NAME),
  or point --pool/--seed at a pool that holds the seed.
EOF
    exit 1
}

assert_root_allowed() {
    path_is_protected "$ROOT" && die "GOLDEN_ROOT is a protected path: $ROOT"
    case "$ROOT" in
        /opt/op-dbus|/opt/op-dbus/golden|/opt/op-dbus/overlay-base|/opt/op-dbus/release-send)
            die "GOLDEN_ROOT must be a dedicated OS-release directory, not $ROOT (default is $POOL_ROOT/$DEFAULT_ROOT_NAME)"
            ;;
    esac
    if path_under "$ROOT" /var/lib/incus && ! root_is_pool_release_dir "$ROOT"; then
        die "GOLDEN_ROOT under /var/lib/incus must be a dedicated directory directly under the pool ($POOL_ROOT/<name>, not images/containers/...): $ROOT"
    fi
    # The pool is expected to be a ublk/cozo-ublk device: a dedicated release
    # dir directly under it is the design. Any other ublk-backed root still
    # needs GOLDEN_ALLOW_UBLK=1.
    if backing_is_ublk "$ROOT" && ! root_is_pool_release_dir "$ROOT"; then
        if [ "${GOLDEN_ALLOW_UBLK:-0}" = 1 ]; then
            warn "GOLDEN_ROOT is on a ublk device outside $POOL_ROOT; GOLDEN_ALLOW_UBLK=1 accepted, Incus/seed paths remain forbidden"
        else
            die "GOLDEN_ROOT sits on a ublk device ($(backing_device "$ROOT")) but is not a release dir directly under $POOL_ROOT; refusing (set GOLDEN_ALLOW_UBLK=1 to override; seeds stay forbidden)"
        fi
    fi
}

is_btrfs_path() {
    local probe=$1
    while [ ! -e "$probe" ] && [ "$probe" != / ]; do
        probe=$(dirname -- "$probe")
    done
    [ "$(stat -f -c %T "$probe" 2>/dev/null || true)" = btrfs ]
}

is_subvolume() {
    local p=$1
    [ -d "$p" ] && [ ! -L "$p" ] || return 1
    btrfs subvolume show "$p" >/dev/null 2>&1
}

subvol_readonly() {
    local p=$1 value
    value=$(btrfs property get -ts "$p" ro 2>/dev/null || true)
    case "$value" in
        ro=true) return 0 ;;
        *) return 1 ;;
    esac
}

subvol_id() {
    btrfs inspect-internal rootid "$1" 2>/dev/null || true
}

announce_mode() {
    if [ "$APPLY" = 1 ]; then
        log "mode: APPLY (mutations enabled, still refusing protected paths)"
    else
        log "mode: DRY-RUN (default; pass --apply to mutate)"
    fi
    log "pool: $POOL_ROOT"
    log "seed: $SEED (release image; read-only source, never written)"
    log "root: $ROOT"
    log "work: $WORK"
    log "live: $LIVE"
    log "rollback: $ROLLBACK"
    log "repo: $REPO"
}

would() {
    printf '  would: %s\n' "$*"
}

# Execute a mutation, or print it in dry-run. All destructive calls go through here.
perform() {
    if [ "$APPLY" = 1 ]; then
        log "apply: $*"
        "$@"
    else
        would "$*"
    fi
}

require_owned_destination() {
    local dest
    dest=$(normalize_path "$1")
    path_is_protected "$dest" && die "refusing protected destination: $dest"
    path_is_owned "$dest" || die "refusing to write outside release slot paths: $dest"
}

require_owned_delete() {
    local dest
    dest=$(normalize_path "$1")
    path_is_protected "$dest" && die "refusing to delete protected path: $dest"
    path_is_owned "$dest" || die "refusing to delete a path this tool does not own: $dest"
    case "$dest" in
        "$ROLLBACK"|"$LIVE"|"$WORK"|"$INCOMING_PREFIX"*|"$SLOTS"/.incoming-*) ;;
        *) die "refusing to delete a path that is not a release slot: $dest" ;;
    esac
}

# Delete only an owned slot subvolume. The last-resort guard against wiping seeds.
delete_slot_subvolume() {
    local dest=$1
    require_owned_delete "$dest"
    [ -e "$dest" ] || [ -L "$dest" ] || return 0
    [ ! -L "$dest" ] || die "refusing to delete a symlink: $dest"
    is_subvolume "$dest" || die "refusing to delete a non-subvolume: $dest"
    if [ "$APPLY" = 1 ]; then
        perform btrfs subvolume delete -- "$dest"
    else
        would "btrfs subvolume delete -- $dest"
    fi
}

print_status() {
    local ro id label p rc
    printf 'golden status\n'
    printf '  pool:     %s  (backing %s)\n' "$POOL_ROOT" "$(backing_device "$POOL_ROOT")"
    rc=0; dir_state "$SEED" || rc=$?
    case "$rc" in
        0) printf '  seed:     present  %s  (release image, never written)\n' "$SEED" ;;
        1) printf '  seed:     absent   %s\n' "$SEED" ;;
        *) printf '  seed:     unknown  %s  (no answer within %ss; pool I/O may be hung)\n' "$SEED" "${GOLDEN_PROBE_TIMEOUT:-5}" ;;
    esac
    printf '  root:     %s  (backing %s)\n' "$ROOT" "$(backing_device "$ROOT")"
    rc=0; dir_state "$ROOT" || rc=$?
    case "$rc" in
        0) ;;
        1) printf '  root does not exist yet (init will create it next to the seed)\n' ;;
        *) printf '  root unreadable within %ss (pool I/O may be hung); skipping slot probes\n' "${GOLDEN_PROBE_TIMEOUT:-5}"
           print_status_footer
           return 0 ;;
    esac
    for label_path in "work:$WORK" "live:$LIVE" "rollback:$ROLLBACK"; do
        label=${label_path%%:*}
        p=${label_path#*:}
        if dir_present "$p"; then
            if probe btrfs subvolume show "$p" >/dev/null 2>&1; then
                if subvol_readonly "$p"; then ro=yes; else ro=no; fi
                id=$(subvol_id "$p")
                printf '  %-8s  present  subvolume id=%s ro=%s  %s\n' "$label" "${id:-?}" "$ro" "$p"
            else
                printf '  %-8s  present  (not a btrfs subvolume)  %s\n' "$label" "$p"
            fi
        else
            printf '  %-8s  absent   %s\n' "$label" "$p"
        fi
    done
    if probe test -f "$STATE"; then
        printf '  state:    %s\n' "$STATE"
        if command -v jq >/dev/null 2>&1; then
            probe jq -e . "$STATE" 2>/dev/null || probe cat "$STATE"
        else
            probe cat "$STATE"
        fi
    else
        printf '  state:    (none yet)\n'
    fi
    print_status_footer
}

print_status_footer() {
    printf '  historical layouts left untouched:\n'
    printf '    /opt/op-dbus/golden  /opt/op-dbus/overlay-base  /opt/op-dbus/release-send\n'
    printf '    @opdbus-system-a / @opdbus-system-b  images/@seed  Incus containers  BtrfsCache\n'
}

plan_safety() {
    printf 'safety:\n'
    printf '  default is dry-run; --apply is required to mutate\n'
    printf '  will refuse to write or delete images/@seed, images/@antix-seed (the seed %s),\n' "$SEED"
    printf '    Incus images/containers, @seed-send, BtrfsCache categories, and any path outside %s/work|slots\n' "$ROOT"
    printf '  no btrfs send/receive and no copy of the seed to another filesystem\n'
    printf '  GOLDEN_ROOT must be a dedicated dir on the seed'"'"'s btrfs (default %s/%s)\n' "$POOL_ROOT" "$DEFAULT_ROOT_NAME"
    printf '  will not restart services, remount the host root, or touch cozo-ublk\n'
}

plan_init() {
    printf 'plan: init (writable work OS = CoW snapshot of the seed, same btrfs)\n'
    seed_path_allowed "$SEED"
    require_same_btrfs_as_seed
    would "install -d -m 0755 -- $ROOT $SLOTS"
    would "btrfs subvolume snapshot -- $SEED $WORK   # the seed is the release image; it is not modified"
    printf '  no send/receive, no copy: work shares extents with the seed on %s\n' "$(btrfs_ident_for_path "$SEED")"
    would "write $STATE (seed path, work path, empty live/rollback)"
}

plan_build() {
    local src=/opt/op-build.src
    local tgt=/opt/op-build/target
    printf 'plan: build (cargo inside the work OS chroot)\n'
    if ! dir_present "$WORK"; then
        printf '  missing work OS at %s — run init first (snapshots the seed %s)\n' "$WORK" "$SEED"
    fi
    would "bind-mount proc, sys, dev, dev/pts, and optionally the cargo registry into $WORK"
    would "install a copy of /etc/resolv.conf inside the chroot for DNS"
    would "git -C $REPO archive HEAD | tar -x -C $WORK$src  (clean tree, no worktree state)"
    if [ "$TOOLCHAIN" = 1 ]; then
        would "if /root/.cargo/bin/cargo is missing in work: chroot apt-get install build deps; rustup --profile minimal"
    fi
    would "chroot <work-or-work/rootfs> env CXXFLAGS='-include cstdint' CARGO_TARGET_DIR=$tgt HOME=/root sh -c 'cd $src && /root/.cargo/bin/cargo build --workspace --release -j$JOBS'"
    would "unmount chroot bind mounts"
    printf '  chroot root is %s/rootfs if this is an Incus image layout, otherwise %s\n' "$WORK" "$WORK"
    printf '  uses the work OS toolchain at /root/.cargo/bin/cargo (host glibc bins will not run here)\n'
    printf '  target dir stays inside the work OS (%s); the host cargo build is not used or disturbed\n' "$tgt"
}

plan_cut() {
    local stamp incoming
    stamp='<YYYYMMDD-HHMMSS>'
    incoming="${INCOMING_PREFIX}${stamp}"
    printf 'plan: cut (seal work, rotate exactly two slots)\n'
    if [ "$WITH_BUILD" = 1 ]; then
        plan_build
    fi
    if ! dir_present "$WORK"; then
        printf '  missing work OS at %s — nothing to snapshot until init+build have populated it\n' "$WORK"
    fi
    would "btrfs subvolume snapshot -r -- $WORK $incoming"
    would "btrfs property get -ts $incoming ro  # require ro=true"
    printf '  rotation rule: delete the previous rollback only (exactly %s), demote current live to rollback, promote incoming to live\n' "$ROLLBACK"
    if dir_present "$ROLLBACK"; then
        would "btrfs subvolume delete -- $ROLLBACK"
    else
        printf '  no previous rollback is present to delete right now\n'
    fi
    if dir_present "$LIVE"; then
        would "mv -- $LIVE $ROLLBACK"
    else
        printf '  no current live slot; incoming becomes the first live snapshot\n'
    fi
    would "mv -- $incoming $LIVE"
    would "write $STATE with the new live and rollback identities"
    printf '  live snapshot %s (%s) is then the OverlayFS lower dir on the live rootfs\n' "$LIVE" "$(os_dir_of "$LIVE")"
    printf '  mounting that overlay is a separate verb (mount-overlay) and is not implied by cut\n'
}

plan_rollback() {
    printf 'plan: rollback (swap live and rollback)\n'
    if ! dir_present "$ROLLBACK"; then
        printf '  no rollback slot is present; nothing to promote\n'
    fi
    would "mv -- $LIVE ${INCOMING_PREFIX}swap-live"
    would "mv -- $ROLLBACK $LIVE"
    would "mv -- ${INCOMING_PREFIX}swap-live $ROLLBACK"
    would "update $STATE"
}

# Pick the overlay lower base: the live release snapshot of the seed, or the
# seed itself (before the first release, or with --lower seed).
# Prints the base subvolume; the lowerdir is os_dir_of <base>.
overlay_lower_base() {
    case "$LOWER_MODE" in
        seed) printf '%s' "$SEED" ;;
        live) printf '%s' "$LIVE" ;;
        auto)
            if dir_present "$LIVE"; then
                printf '%s' "$LIVE"
            else
                printf '%s' "$SEED"
            fi
            ;;
    esac
}

plan_mount() {
    local mnt=${MOUNT_POINT:-/run/opdbus-os-live} base lower
    base=$(overlay_lower_base)
    lower=$(os_dir_of "$base")
    printf 'plan: mount-overlay (release image as OverlayFS lower dir on the live rootfs)\n'
    printf '  lower mode: %s (auto = live release snapshot of the seed, else the seed)\n' "$LOWER_MODE"
    printf '  lower: %s\n' "$lower"
    if [ "$base" = "$SEED" ]; then
        printf '  lower is the seed itself (%s); overlayfs never writes the lower dir\n' "$SEED"
    fi
    printf '  upper: %s\n' "$OVERLAY_UPPER"
    printf '  work:  %s\n' "$OVERLAY_WORK"
    printf '  mount: %s\n' "$mnt"
    would "install -d -m 0755 -- $OVERLAY_UPPER $OVERLAY_WORK $mnt"
    would "mount -t overlay overlay -o lowerdir=$lower,upperdir=$OVERLAY_UPPER,workdir=$OVERLAY_WORK $mnt"
    printf '  this does not remount the host root at runtime; / and other system paths are refused\n'
}

write_state() {
    local work_id live_id rollback_id commit tmp
    work_id=$(subvol_id "$WORK" || true)
    live_id=$(subvol_id "$LIVE" || true)
    rollback_id=$(subvol_id "$ROLLBACK" || true)
    commit=$(git -C "$REPO" rev-parse HEAD 2>/dev/null || printf 'unknown')
    tmp=$(mktemp "$ROOT/.state.XXXXXX")
    if command -v jq >/dev/null 2>&1; then
        jq -n \
            --arg root "$ROOT" \
            --arg work "$WORK" \
            --arg live "$LIVE" \
            --arg rollback "$ROLLBACK" \
            --arg seed "$SEED" \
            --arg commit "$commit" \
            --arg work_id "${work_id:-}" \
            --arg live_id "${live_id:-}" \
            --arg rollback_id "${rollback_id:-}" \
            '{version:1,root:$root,work:{path:$work,id:$work_id},seed:{path:$seed},live:(if $live_id=="" then null else {path:$live,id:$live_id,readonly:true,commit:$commit} end),rollback:(if $rollback_id=="" then null else {path:$rollback,id:$rollback_id,readonly:true} end)}' \
            > "$tmp"
    else
        cat > "$tmp" <<EOF
{"version":1,"root":"$ROOT","work":{"path":"$WORK","id":"$work_id"},"seed":{"path":"$SEED"},"live":{"path":"$LIVE","id":"$live_id"},"rollback":{"path":"$ROLLBACK","id":"$rollback_id"},"commit":"$commit"}
EOF
    fi
    chmod 0644 "$tmp"
    mv -T "$tmp" "$STATE"
}

do_init() {
    require_owned_destination "$ROOT"
    require_owned_destination "$SLOTS"
    require_owned_destination "$WORK"
    if [ "$APPLY" != 1 ]; then
        plan_init
        return 0
    fi
    [ "$(id -u)" = 0 ] || die "init --apply must run as root"
    seed_path_allowed "$SEED"
    require_same_btrfs_as_seed
    [ -d "$SEED" ] || die "seed is not a directory: $SEED"
    is_subvolume "$SEED" || die "seed is not a btrfs subvolume: $SEED"
    is_btrfs_path "$ROOT" || is_btrfs_path "$(dirname -- "$ROOT")" || die "GOLDEN_ROOT is not on btrfs: $ROOT"
    install -d -m 0755 -- "$ROOT" "$SLOTS"
    if [ -e "$WORK" ] || [ -L "$WORK" ]; then
        die "work already exists: $WORK"
    fi
    perform btrfs subvolume snapshot -- "$SEED" "$WORK"
    write_state
    log "work OS is ready at $WORK (CoW snapshot of the seed $SEED)"
}

# Incus image volumes (including @antix-seed) store the OS in rootfs/.
# Host-built binaries (glibc 2.43 on antix1) will not run on @antix-seed
# (Debian trixie glibc 2.41); builds must use the work OS toolchain,
# following deploy/provisioning/25-build-in-chroot.sh.
chroot_os_root() {
    if [ -d "$WORK/rootfs" ] && { [ -f "$WORK/rootfs/etc/os-release" ] || [ -d "$WORK/rootfs/usr" ]; }; then
        printf '%s' "$WORK/rootfs"
    else
        printf '%s' "$WORK"
    fi
}

chroot_build() {
    local os_root src_host cargo_bin b
    os_root=$(chroot_os_root)
    src_host=$os_root/opt/op-build.src
    cargo_bin=/root/.cargo/bin/cargo
    [ -d "$WORK" ] || die "work OS is missing: $WORK"
    is_subvolume "$WORK" || die "work is not a btrfs subvolume: $WORK"
    if subvol_readonly "$WORK"; then
        die "work OS is read-only; it must stay writable (slots are the sealed snapshots)"
    fi
    if [ ! -d "$REPO" ] || [ ! -f "$REPO/Cargo.toml" ]; then
        die "repo is missing Cargo.toml: $REPO"
    fi
    if [ ! -x "$os_root$cargo_bin" ] && [ "$TOOLCHAIN" != 1 ]; then
        die "no cargo toolchain in the work OS at $os_root$cargo_bin (the seed needs the overlay toolchain; host binaries will not run on this glibc)"
    fi
    require_owned_destination "$WORK"
    install -d -m 0755 -- "$src_host" "$os_root/opt/op-build/target"
    rm -rf -- "$src_host"
    install -d -m 0755 -- "$src_host"
    git -C "$REPO" archive HEAD | tar -x -C "$src_host"
    chroot_stage_threetched_capture "$os_root"
    if [ -e "$os_root/etc/resolv.conf" ] || [ -L "$os_root/etc/resolv.conf" ]; then
        cp -a -- "$os_root/etc/resolv.conf" "$os_root/etc/resolv.conf.golden-bak" || true
    fi
    if [ -r /etc/resolv.conf ]; then
        rm -f -- "$os_root/etc/resolv.conf"
        cp -L -- /etc/resolv.conf "$os_root/etc/resolv.conf"
    fi
    # 25-build-in-chroot.sh bind-mounts proc only; also bind sys/dev so a
    # full workspace build can resolve devices and UTS as a normal chroot.
    for b in proc sys dev "dev/pts"; do
        install -d -m 0755 -- "$os_root/$b"
        mountpoint -q "$os_root/$b" || mount --bind "/$b" "$os_root/$b"
    done
    # shellcheck disable=SC2064
    trap "chroot_cleanup '$os_root'" EXIT HUP INT TERM
    if [ ! -x "$os_root$cargo_bin" ]; then
        chroot_toolchain "$os_root"
    fi
    # chroot(1) chdirs to /, so cd into the exported tree before cargo runs
    # (otherwise cargo cannot find Cargo.toml).
    chroot "$os_root" env \
        CXXFLAGS="-include cstdint" \
        CARGO_TARGET_DIR=/opt/op-build/target \
        THREETCHED_CAPTURE_BIN="$GOLDEN_CAPTURE_DIR/threetched" \
        THREETCHED_CAPTURE_MANIFEST="$GOLDEN_CAPTURE_MANIFEST" \
        PATH="/root/.cargo/bin:/usr/local/cargo/bin:/usr/bin:/bin" \
        HOME=/root \
        sh -c 'cd /opt/op-build.src && exec "$0" build --workspace --release -j"$1"' \
        "$cargo_bin" "$JOBS"
    chroot_cleanup "$os_root"
    trap - EXIT HUP INT TERM
}

# The op-grpc-bridge build script captures the pinned threetched CLI surface
# (op_plugins threetched::prepare_schema_capture), so the work OS needs the
# pinned binary. Stage the host's installed threetched, sha-checked against
# the committed manifest, into a build-only dir and point the capture pair
# there. The binary is glibc <= 2.39, so it runs on the seed. The capture
# writes a scratch config into the runtime config dir (/var/lib/threetched);
# when the work OS had none, chroot_cleanup removes it again.
GOLDEN_CAPTURE_DIR=/opt/op-build/threetched-capture
GOLDEN_CAPTURE_MANIFEST=/opt/op-build.src/crates/op-grpc-bridge/threetched-manifests/threetched.json
GOLDEN_THREETCHED_BIN=${GOLDEN_THREETCHED_BIN:-/usr/local/libexec/op-dbus/threetched/threetched}
chroot_stage_threetched_capture() {
    local os_root=$1 want have
    [ -x "$GOLDEN_THREETCHED_BIN" ] || die "pinned threetched binary missing on the host: $GOLDEN_THREETCHED_BIN"
    [ -r "$os_root$GOLDEN_CAPTURE_MANIFEST" ] || die "threetched manifest missing in the exported tree: $GOLDEN_CAPTURE_MANIFEST"
    command -v jq >/dev/null 2>&1 || die "jq is required to read the threetched manifest"
    want=$(jq -r '.artifact.sha256 // empty' "$os_root$GOLDEN_CAPTURE_MANIFEST")
    have=$(sha256sum -- "$GOLDEN_THREETCHED_BIN" | cut -d' ' -f1)
    [ -n "$want" ] && [ "$want" = "$have" ] \
        || die "host threetched sha256 $have does not match the manifest ($want)"
    rm -rf -- "$os_root$GOLDEN_CAPTURE_DIR"
    install -d -m 0755 -- "$os_root$GOLDEN_CAPTURE_DIR"
    install -m 0755 -- "$GOLDEN_THREETCHED_BIN" "$os_root$GOLDEN_CAPTURE_DIR/threetched"
    if [ ! -e "$os_root/var/lib/threetched" ] && [ ! -L "$os_root/var/lib/threetched" ]; then
        : > "$os_root$GOLDEN_CAPTURE_DIR/.created-config-dir"
    fi
    log "staged threetched $have for the build-time schema capture"
}

# Same packages and rustup install as 20-seed-overlay.sh --toolchain, but only
# inside the writable work OS (never the seed).
chroot_toolchain() {
    local os_root=$1
    log "installing build deps and rustup inside the work OS chroot $os_root"
    chroot "$os_root" env DEBIAN_FRONTEND=noninteractive HOME=/root \
        PATH=/usr/sbin:/usr/bin:/sbin:/bin \
        sh -c 'apt-get update -q && apt-get install -y -q --no-install-recommends ca-certificates curl build-essential pkg-config protobuf-compiler libprotobuf-dev libssl-dev clang cmake git' \
        || die "apt in the work chroot failed"
    chroot "$os_root" env HOME=/root PATH=/usr/sbin:/usr/bin:/sbin:/bin \
        sh -c 'curl -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal' \
        || die "rustup in the work chroot failed"
    [ -x "$os_root/root/.cargo/bin/cargo" ] || die "rustup finished but $os_root/root/.cargo/bin/cargo is missing"
}

chroot_cleanup() {
    local root=$1 b
    if [ -e "$root$GOLDEN_CAPTURE_DIR/.created-config-dir" ]; then
        rm -rf -- "$root/var/lib/threetched"
    fi
    rm -rf -- "$root$GOLDEN_CAPTURE_DIR"
    for b in "dev/pts" dev sys proc; do
        if mountpoint -q "$root/$b"; then
            umount -l "$root/$b" || true
        fi
    done
    if [ -e "$root/etc/resolv.conf.golden-bak" ] || [ -L "$root/etc/resolv.conf.golden-bak" ]; then
        rm -f -- "$root/etc/resolv.conf"
        mv -- "$root/etc/resolv.conf.golden-bak" "$root/etc/resolv.conf" || true
    fi
}

do_build() {
    if [ "$APPLY" != 1 ]; then
        plan_build
        return 0
    fi
    [ "$(id -u)" = 0 ] || die "build --apply must run as root"
    chroot_build
    log "chroot workspace release build finished in $WORK"
}

do_cut() {
    local stamp incoming
    if [ "$APPLY" != 1 ]; then
        plan_cut
        return 0
    fi
    [ "$(id -u)" = 0 ] || die "cut --apply must run as root"
    [ -d "$WORK" ] || die "work OS is missing: $WORK"
    is_subvolume "$WORK" || die "work is not a btrfs subvolume: $WORK"
    if [ "$WITH_BUILD" = 1 ]; then
        chroot_build
    fi
    stamp=$(date +%Y%m%d-%H%M%S)
    incoming="${INCOMING_PREFIX}${stamp}"
    require_owned_destination "$incoming"
    require_owned_destination "$LIVE"
    require_owned_destination "$ROLLBACK"
    perform btrfs subvolume snapshot -r -- "$WORK" "$incoming"
    if [ "$APPLY" = 1 ]; then
        subvol_readonly "$incoming" || die "incoming snapshot is not read-only: $incoming"
    fi
    if [ -e "$ROLLBACK" ] || [ -L "$ROLLBACK" ]; then
        delete_slot_subvolume "$ROLLBACK"
    fi
    if [ -e "$LIVE" ] || [ -L "$LIVE" ]; then
        require_owned_destination "$ROLLBACK"
        perform mv -- "$LIVE" "$ROLLBACK"
    fi
    perform mv -- "$incoming" "$LIVE"
    if [ "$APPLY" = 1 ]; then
        write_state
    fi
    log "cut complete: live=$LIVE rollback=${ROLLBACK}"
}

do_rollback() {
    local swap=${INCOMING_PREFIX}swap-live
    if [ "$APPLY" != 1 ]; then
        plan_rollback
        return 0
    fi
    [ "$(id -u)" = 0 ] || die "rollback --apply must run as root"
    [ -d "$ROLLBACK" ] || die "no rollback slot to promote"
    is_subvolume "$LIVE" || die "live is not a subvolume: $LIVE"
    is_subvolume "$ROLLBACK" || die "rollback is not a subvolume: $ROLLBACK"
    require_owned_destination "$swap"
    require_owned_delete "$LIVE"
    require_owned_delete "$ROLLBACK"
    perform mv -- "$LIVE" "$swap"
    perform mv -- "$ROLLBACK" "$LIVE"
    perform mv -- "$swap" "$ROLLBACK"
    write_state
    log "rollback complete: live=$LIVE (previous rollback promoted)"
}

forbid_system_mountpoint() {
    local mnt=$1
    is_absolute "$mnt" || die "mount point must be absolute: $mnt"
    path_is_protected "$mnt" && die "refusing to mount over a protected/system path: $mnt"
    case "$mnt" in
        /|/usr|/usr/*|/etc|/etc/*|/home|/home/*|/boot|/boot/*|/var|/var/*|/opt|/opt/op-dbus|/run/mounts/*)
            die "refusing to mount the live OS over a host system path: $mnt"
            ;;
    esac
}

do_mount_overlay() {
    local mnt=${MOUNT_POINT:-/run/opdbus-os-live} base lower
    if [ "$APPLY" != 1 ]; then
        plan_mount
        return 0
    fi
    [ "$(id -u)" = 0 ] || die "mount-overlay --apply must run as root"
    base=$(overlay_lower_base)
    [ -d "$base" ] || die "overlay lower base is missing: $base"
    is_subvolume "$base" || die "overlay lower base is not a subvolume: $base"
    if [ "$base" = "$LIVE" ]; then
        subvol_readonly "$LIVE" || die "live snapshot must be read-only"
    elif ! subvol_readonly "$base"; then
        warn "seed $base is not read-only; overlayfs will still not write it"
    fi
    lower=$(os_dir_of "$base")
    [ -d "$lower" ] || die "overlay lower dir is missing: $lower (set GOLDEN_OS_SUBDIR= for a flat subvolume)"
    forbid_system_mountpoint "$mnt"
    require_owned_destination "$OVERLAY_UPPER"
    require_owned_destination "$OVERLAY_WORK"
    install -d -m 0755 -- "$OVERLAY_UPPER" "$OVERLAY_WORK" "$mnt"
    perform mount -t overlay overlay \
        -o "lowerdir=$lower,upperdir=$OVERLAY_UPPER,workdir=$OVERLAY_WORK" \
        "$mnt"
    log "mounted live OS overlay at $mnt (lower=$lower)"
}

plan_release() {
    printf 'plan: release (snapshot work, rotate live -> rollback, plan overlay)\n'
    printf '  the seed %s is the release image and stays put; work (its CoW clone) is snapshotted for the live overlay\n' "$SEED"
    printf '  will not touch /opt/op-dbus/golden or @opdbus-system-a/b\n'
    printf '  will not remount the host root; mount-overlay is a separate verb and refuses /\n'
    plan_cut
    plan_mount
}

do_release() {
    if [ "$APPLY" != 1 ]; then
        plan_release
        return 0
    fi
    log "release --apply rotates slots only; it does not mount an overlay or remount the live rootfs"
    do_cut
}

do_rotate() {
    if [ "$APPLY" != 1 ]; then
        printf 'plan: rotate (same slot rotation as release, no overlay mount)\n'
        plan_cut
        return 0
    fi
    do_cut
}

do_plan() {
    printf 'golden release plan\n'
    plan_safety
    printf '\n'
    plan_init
    printf '\n'
    plan_build
    printf '\n'
    plan_cut
    printf '\n'
    plan_rollback
    printf '\n'
    plan_release
    printf '\n'
    plan_mount
}

main() {
    parse_args "$@"
    resolve_layout
    assert_root_allowed
    announce_mode
    case "$VERB" in
        status) print_status ;;
        plan) do_plan ;;
        init) do_init ;;
        build) do_build ;;
        cut) do_cut ;;
        rollback) do_rollback ;;
        release) do_release ;;
        rotate) do_rotate ;;
        mount-overlay) do_mount_overlay ;;
        activate) do_activate ;;
    esac
}

# shellcheck source=activate.sh
. "$SCRIPT_DIR/activate.sh"

if [ "${GOLDEN_LIB:-0}" != 1 ]; then
    main "$@"
fi
