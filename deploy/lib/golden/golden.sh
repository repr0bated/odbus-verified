#!/usr/bin/env bash
# golden.sh — approved golden release: OS subvolume, chroot build, two slots.
#
# A btrfs subvolume holds the OS (work). cargo builds inside a chroot of that
# OS. The work tree is then snapshotted read-only; that snapshot is the overlay
# lower dir forming the live system. There are exactly two slots: live and
# rollback. Cutting a release demotes old live to rollback and deletes the
# previous rollback.
#
# This is not op-cache BtrfsCache (one subvolume per category: embeddings,
# blocks, queries, diffs, kv) and it is not the historical application-payload
# tree at /opt/op-dbus/golden or the @opdbus-system-a/b transfer slots. Those
# layouts are left alone.
#
# Dry-run by default. Any create/delete/snapshot/mount/build requires --apply.
# --apply still refuses to delete images/@seed (and other protected paths) and
# refuses to operate on cozo-ublk / 3tched-pool unless GOLDEN_ALLOW_UBLK=1,
# which still cannot target Incus image/container paths.
#
# Usage:
#   deploy/lib/golden/golden.sh [flags] <verb>
#   verbs: status | plan | init | build | cut | rollback | mount-overlay
#   flags: --apply  --root PATH  --from PATH  --repo PATH  --jobs N
#          --with-build  --mount PATH  --dry-run
#
# Default root: /opt/op-dbus/os  (sibling of the historical golden payload)
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

APPLY=0
WITH_BUILD=0
FROM_OS=
MOUNT_POINT=
JOBS=${GOLDEN_CARGO_JOBS:-2}
ROOT=${GOLDEN_ROOT:-/opt/op-dbus/os}
REPO=${GOLDEN_REPO:-$DEFAULT_REPO}
VERB=

usage() {
    cat <<'EOF'
Usage: golden.sh [flags] <verb>

Verbs:
  status         Show work OS and live/rollback slots (read-only)
  plan           Print the full release plan without changing anything (default)
  init           Create the work OS subvolume (optionally --from an existing OS)
  build          cargo build --workspace --release inside the work OS chroot
  cut            Snapshot work, demote live to rollback, drop previous rollback
  rollback       Swap live and rollback (promote the rollback snapshot)
  mount-overlay  Mount the live snapshot as an OverlayFS lower dir at --mount

Flags:
  --dry-run      Print the plan only (default)
  --apply        Perform the verb (never the default)
  --root PATH    Release root (default /opt/op-dbus/os or $GOLDEN_ROOT)
  --from PATH    For init: snapshot this OS subvolume into work (source is not modified)
  --repo PATH    Repo to export into the chroot (default: this checkout)
  --jobs N       cargo -j (default 2; keep this modest)
  --with-build   For cut: run the chroot build before snapshotting
  --mount PATH   For mount-overlay: overlay mount point (not / or other host roots)

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
            --root)
                [ "$#" -ge 2 ] || die "missing value for --root"
                ROOT=$2
                shift
                ;;
            --from)
                [ "$#" -ge 2 ] || die "missing value for --from"
                FROM_OS=$2
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
        status|plan|init|build|cut|rollback|mount-overlay) ;;
        *) die "unknown verb: $VERB (try --help)" ;;
    esac
    case "$JOBS" in
        ''|*[!0-9]*|0) die "jobs must be a positive integer, got: $JOBS" ;;
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
    if [ -n "$FROM_OS" ]; then
        case "$FROM_OS" in
            /*) ;;
            *) die "--from must be an absolute path" ;;
        esac
        FROM_OS=$(normalize_path "$FROM_OS")
    fi
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
# A source --from may be a protected seed (read-only CoW snapshot into work).
normalize_path() {
    # Resolve . and .. without requiring the path to exist. Keep "/" intact.
    local p=$1
    [ -n "$p" ] || return 1
    realpath -m -- "$p"
}

path_is_protected() {
    local p n
    p=$(normalize_path "$1")
    [ "$p" = / ] && return 0
    p=${p%/}
    n=${p##*/}
    case "$p" in
        */images/@seed|*/images/@seed/*|*/images/@antix-seed|*/images/@antix-seed/*) return 0 ;;
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
        @seed|@antix-seed|@seed-send) return 0 ;;
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
    # Best-effort; empty if unreadable. Does not mount anything.
    local probe=$1
    while [ ! -e "$probe" ] && [ "$probe" != / ]; do
        probe=$(dirname -- "$probe")
    done
    findmnt -n -o SOURCE -T "$probe" 2>/dev/null || true
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
        # -P keeps an empty UUID column; plain columns would shift SOURCE left.
        findmnt -t btrfs -n -P -o TARGET,UUID,SOURCE 2>/dev/null || true
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
                # Test table and fallback: TARGET UUID SOURCE (whitespace)
                read -r target uuid source <<FRG
$line
FRG
                ;;
        esac
        [ -n "$target" ] || continue
        if path_under "$path" "$target" || [ "$path" = "$target" ]; then
            tlen=${#target}
            if [ "$tlen" -ge "$best_len" ]; then
                best_len=$tlen
                if [ -n "$uuid" ]; then
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

require_same_btrfs_for_from() {
    local src_ident dst_ident
    [ -n "$FROM_OS" ] || return 0
    src_ident=$(btrfs_ident_for_path "$FROM_OS")
    dst_ident=$(btrfs_ident_for_path "$ROOT")
    if [ -z "$src_ident" ] || [ -z "$dst_ident" ]; then
        die "cannot determine the btrfs filesystem for --from ($FROM_OS => ${src_ident:-unknown}) and GOLDEN_ROOT ($ROOT => ${dst_ident:-unknown}); refusing to plan a snapshot"
    fi
    if [ "$src_ident" = "$dst_ident" ]; then
        return 0
    fi
    cat >&2 <<EOF
golden: cannot CoW-snapshot --from $FROM_OS onto $WORK: they are on different btrfs filesystems
  source: $src_ident  ($FROM_OS)
  target: $dst_ident  ($ROOT)
  btrfs subvolume snapshot only works on the same filesystem.
  Options (not performed by this script):
  1) btrfs send/receive: if the source is already read-only, send it and receive into $ROOT, then rename the received subvolume to work. That reads the source and writes only on the target filesystem; it does not write or delete the source. If the source is writable, send needs a read-only snapshot, which would create a new subvolume on the source filesystem and needs explicit approval.
  2) full copy (rsync -aHAX or cp -a) into a newly created work subvolume on the target filesystem. No shared extents; more space and time. cp --reflink cannot work across filesystems.
  This tool will not run send/receive or a copy unless that is approved separately.
EOF
    exit 1
}

assert_root_allowed() {
    path_is_protected "$ROOT" && die "GOLDEN_ROOT is a protected path: $ROOT"
    case "$ROOT" in
        /opt/op-dbus|/opt/op-dbus/golden|/opt/op-dbus/overlay-base|/opt/op-dbus/release-send)
            die "GOLDEN_ROOT must be a dedicated OS-release directory, not $ROOT (default is /opt/op-dbus/os)"
            ;;
    esac
    if backing_is_ublk "$ROOT"; then
        if [ "${GOLDEN_ALLOW_UBLK:-0}" = 1 ]; then
            warn "GOLDEN_ROOT is on a ublk device; GOLDEN_ALLOW_UBLK=1 accepted, Incus/seed paths remain forbidden"
        else
            die "GOLDEN_ROOT sits on a ublk / cozo-ublk / 3tched-pool device ($(backing_device "$ROOT")); refusing. Use /opt/op-dbus/os on the host btrfs, or set GOLDEN_ALLOW_UBLK=1 only if you intend a dedicated slot path on that filesystem (still never images/@seed)"
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
    local ro id
    printf 'golden status\n'
    printf '  root:     %s\n' "$ROOT"
    if [ -e "$ROOT" ]; then
        printf '  fstype:   %s\n' "$(stat -f -c %T "$ROOT" 2>/dev/null || echo unknown)"
        printf '  backing:  %s\n' "$(backing_device "$ROOT")"
    else
        printf '  root does not exist yet (init will create it under /opt/op-dbus/os by default)\n'
    fi
    for label_path in "work:$WORK" "live:$LIVE" "rollback:$ROLLBACK"; do
        local label=${label_path%%:*}
        local p=${label_path#*:}
        if [ -d "$p" ]; then
            if is_subvolume "$p"; then
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
    if [ -f "$STATE" ]; then
        printf '  state:    %s\n' "$STATE"
        if command -v jq >/dev/null 2>&1; then
            jq -e . "$STATE" 2>/dev/null || cat "$STATE"
        else
            cat "$STATE"
        fi
    else
        printf '  state:    (none yet)\n'
    fi
    printf '  historical layouts left untouched:\n'
    printf '    /opt/op-dbus/golden  /opt/op-dbus/overlay-base  /opt/op-dbus/release-send\n'
    printf '    @opdbus-system-a / @opdbus-system-b  images/@seed  3tched-pool  BtrfsCache\n'
}

plan_safety() {
    printf 'safety:\n'
    printf '  default is dry-run; --apply is required to mutate\n'
    printf '  will refuse to delete images/@seed, images/@antix-seed, Incus pool data,\n'
    printf '    @seed-send, BtrfsCache categories, and any path outside %s/work|slots\n' "$ROOT"
    printf '  will refuse a GOLDEN_ROOT on ublk/cozo-ublk/3tched-pool unless GOLDEN_ALLOW_UBLK=1\n'
    printf '  will not restart services, remount the host root, or touch cozo-ublk\n'
}

plan_init() {
    printf 'plan: init (create the writable OS work subvolume)\n'
    would "install -d -m 0755 -- $ROOT $SLOTS"
    if [ -n "$FROM_OS" ]; then
        is_absolute "$FROM_OS" || die "--from must be an absolute path"
        path_under "$FROM_OS" "$ROOT" && die "--from must be outside the release root"
        require_same_btrfs_for_from
        would "btrfs subvolume snapshot -- $FROM_OS $WORK   # source is not modified, including if it is a seed"
        printf '  note: --from may snapshot images/@seed into work; the seed itself is never written or deleted\n'
    else
        would "btrfs subvolume create -- $WORK"
        printf '  note: work will be an empty subvolume; populate it with an OS, or re-run init --from <os-subvolume>\n'
    fi
    would "write $STATE (work path, empty live/rollback)"
}

plan_build() {
    local src=/opt/op-build.src
    local tgt=/opt/op-build/target
    printf 'plan: build (cargo inside the work OS chroot)\n'
    if [ ! -d "$WORK" ]; then
        printf '  missing work OS at %s — run init (and --from an OS) first\n' "$WORK"
    fi
    would "bind-mount proc, sys, dev, dev/pts, and optionally the cargo registry into $WORK"
    would "install a copy of /etc/resolv.conf inside the chroot for DNS"
    would "git -C $REPO archive HEAD | tar -x -C $WORK$src  (clean tree, no worktree state)"
    would "chroot $WORK env CXXFLAGS=-include\\ cstdint CARGO_TARGET_DIR=$tgt cargo build --workspace --release -j$JOBS"
    would "unmount chroot bind mounts"
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
    if [ ! -d "$WORK" ]; then
        printf '  missing work OS at %s — nothing to snapshot until init+build have populated it\n' "$WORK"
    fi
    would "btrfs subvolume snapshot -r -- $WORK $incoming"
    would "btrfs property get -ts $incoming ro  # require ro=true"
    printf '  rotation rule: delete the previous rollback only (exactly %s), demote current live to rollback, promote incoming to live\n' "$ROLLBACK"
    if [ -d "$ROLLBACK" ]; then
        would "btrfs subvolume delete -- $ROLLBACK"
    else
        printf '  no previous rollback is present to delete right now\n'
    fi
    if [ -d "$LIVE" ]; then
        would "mv -- $LIVE $ROLLBACK"
    else
        printf '  no current live slot; incoming becomes the first live snapshot\n'
    fi
    would "mv -- $incoming $LIVE"
    would "write $STATE with the new live and rollback identities"
    printf '  live snapshot %s is then the OverlayFS lower dir for the live system\n' "$LIVE"
    printf '  mounting that overlay is a separate verb (mount-overlay) and is not implied by cut\n'
}

plan_rollback() {
    printf 'plan: rollback (swap live and rollback)\n'
    if [ ! -d "$ROLLBACK" ]; then
        printf '  no rollback slot is present; nothing to promote\n'
    fi
    would "mv -- $LIVE ${INCOMING_PREFIX}swap-live"
    would "mv -- $ROLLBACK $LIVE"
    would "mv -- ${INCOMING_PREFIX}swap-live $ROLLBACK"
    would "update $STATE"
}

plan_mount() {
    local mnt=${MOUNT_POINT:-/run/opdbus-os-live}
    printf 'plan: mount-overlay (live snapshot as OverlayFS lower dir)\n'
    printf '  lower: %s\n' "$LIVE"
    printf '  upper: %s\n' "$OVERLAY_UPPER"
    printf '  work:  %s\n' "$OVERLAY_WORK"
    printf '  mount: %s\n' "$mnt"
    would "install -d -m 0755 -- $OVERLAY_UPPER $OVERLAY_WORK $mnt"
    would "mount -t overlay overlay -o lowerdir=$LIVE,upperdir=$OVERLAY_UPPER,workdir=$OVERLAY_WORK $mnt"
    printf '  this does not remount the host root; / and other system paths are refused\n'
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
            --arg commit "$commit" \
            --arg work_id "${work_id:-}" \
            --arg live_id "${live_id:-}" \
            --arg rollback_id "${rollback_id:-}" \
            '{version:1,root:$root,work:{path:$work,id:$work_id},live:(if $live_id=="" then null else {path:$live,id:$live_id,readonly:true,commit:$commit} end),rollback:(if $rollback_id=="" then null else {path:$rollback,id:$rollback_id,readonly:true} end)}' \
            > "$tmp"
    else
        cat > "$tmp" <<EOF
{"version":1,"root":"$ROOT","work":{"path":"$WORK","id":"$work_id"},"live":{"path":"$LIVE","id":"$live_id"},"rollback":{"path":"$ROLLBACK","id":"$rollback_id"},"commit":"$commit"}
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
    is_btrfs_path "$ROOT" || is_btrfs_path "$(dirname -- "$ROOT")" || die "GOLDEN_ROOT is not on btrfs: $ROOT"
    install -d -m 0755 -- "$ROOT" "$SLOTS"
    if [ -e "$WORK" ] || [ -L "$WORK" ]; then
        die "work already exists: $WORK"
    fi
    if [ -n "$FROM_OS" ]; then
        is_absolute "$FROM_OS" || die "--from must be an absolute path"
        [ -d "$FROM_OS" ] || die "--from is not a directory: $FROM_OS"
        path_under "$FROM_OS" "$ROOT" && die "--from must be outside the release root"
        require_same_btrfs_for_from
        is_subvolume "$FROM_OS" || die "--from is not a btrfs subvolume: $FROM_OS"
        perform btrfs subvolume snapshot -- "$FROM_OS" "$WORK"
    else
        perform btrfs subvolume create -- "$WORK"
    fi
    write_state
    log "work OS is ready at $WORK"
}

chroot_build() {
    local src_host=$WORK/opt/op-build.src
    local b
    [ -d "$WORK" ] || die "work OS is missing: $WORK"
    is_subvolume "$WORK" || die "work is not a btrfs subvolume: $WORK"
    if subvol_readonly "$WORK"; then
        die "work OS is read-only; it must stay writable (slots are the sealed snapshots)"
    fi
    if [ ! -d "$REPO" ] || [ ! -f "$REPO/Cargo.toml" ]; then
        die "repo is missing Cargo.toml: $REPO"
    fi
    require_owned_destination "$WORK"
    install -d -m 0755 -- "$src_host" "$WORK/opt/op-build/target"
    rm -rf -- "$src_host"
    install -d -m 0755 -- "$src_host"
    git -C "$REPO" archive HEAD | tar -x -C "$src_host"
    if [ -e "$WORK/etc/resolv.conf" ] || [ -L "$WORK/etc/resolv.conf" ]; then
        cp -a -- "$WORK/etc/resolv.conf" "$WORK/etc/resolv.conf.golden-bak" || true
    fi
    if [ -r /etc/resolv.conf ]; then
        rm -f -- "$WORK/etc/resolv.conf"
        cp -L -- /etc/resolv.conf "$WORK/etc/resolv.conf"
    fi
    for b in proc sys dev "dev/pts"; do
        install -d -m 0755 -- "$WORK/$b"
        mountpoint -q "$WORK/$b" || mount --bind "/$b" "$WORK/$b"
    done
    # shellcheck disable=SC2064
    trap "chroot_cleanup '$WORK'" EXIT HUP INT TERM
    chroot "$WORK" env \
        CXXFLAGS="-include cstdint" \
        CARGO_TARGET_DIR=/opt/op-build/target \
        PATH="/root/.cargo/bin:/usr/local/cargo/bin:/usr/bin:/bin" \
        cargo build --workspace --release -j"$JOBS"
    chroot_cleanup "$WORK"
    trap - EXIT HUP INT TERM
}

chroot_cleanup() {
    local root=$1 b
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
    local mnt=${MOUNT_POINT:-/run/opdbus-os-live}
    if [ "$APPLY" != 1 ]; then
        plan_mount
        return 0
    fi
    [ "$(id -u)" = 0 ] || die "mount-overlay --apply must run as root"
    [ -d "$LIVE" ] || die "live snapshot is missing: $LIVE"
    is_subvolume "$LIVE" || die "live is not a subvolume: $LIVE"
    subvol_readonly "$LIVE" || die "live snapshot must be read-only"
    forbid_system_mountpoint "$mnt"
    require_owned_destination "$OVERLAY_UPPER"
    require_owned_destination "$OVERLAY_WORK"
    install -d -m 0755 -- "$OVERLAY_UPPER" "$OVERLAY_WORK" "$mnt"
    perform mount -t overlay overlay \
        -o "lowerdir=$LIVE,upperdir=$OVERLAY_UPPER,workdir=$OVERLAY_WORK" \
        "$mnt"
    log "mounted live OS overlay at $mnt (lower=$LIVE)"
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
        mount-overlay) do_mount_overlay ;;
    esac
}

if [ "${GOLDEN_LIB:-0}" != 1 ]; then
    main "$@"
fi
