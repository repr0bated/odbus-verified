#!/usr/bin/env bash
# One permanent writable build OS; immutable release paths beneath @upper.
set -euo pipefail
PATH=/usr/sbin:/usr/bin:/sbin:/bin
export PATH
umask 022
SCRIPT_DIR=$(cd -- "$(dirname -- "$(readlink -f -- "${BASH_SOURCE[0]}")")" && pwd)
APPLY=0 WITH_BUILD=0 VERB=plan MOUNT_POINT=
JOBS=${GOLDEN_CARGO_JOBS:-}
die() { printf 'golden: %s\n' "$*" >&2; exit 1; }
log() { printf 'golden: %s\n' "$*"; }
warn() { printf 'golden: warning: %s\n' "$*" >&2; }
would() { printf '  would: %s\n' "$*"; }
normalize_path() { realpath -s -m -- "$1"; }
os_dir_of() { printf '%s' "$1"; }
subvol_id() { btrfs inspect-internal rootid "$1"; }
subvol_uuid() { btrfs subvolume show "$1" | awk '$1=="UUID:" {print $2}'; }
is_subvolume() { [[ -d $1 && ! -L $1 ]] && btrfs subvolume show "$1" >/dev/null 2>&1; }
subvol_readonly() { [[ $(btrfs property get -ts "$1" ro) == ro=true ]]; }
setting() {
    local name=$1 fallback=${2:-} file=/etc/op-dbus/env/$1
    if [[ -n ${!name:-} ]]; then printf '%s' "${!name}"; return; fi
    if [[ -e $file || -L $file ]]; then
        [[ -f $file && ! -L $file && $(stat -c %u:%g:%a "$file") == 0:0:600 ]] ||
            die "unsafe setting: $file"
        tr -d '\n' <"$file"
    else printf '%s' "$fallback"; fi
}
release_path() {
    local relative=${1#"$ROOT/"}
    [[ $1 == "$(normalize_path "$1")" && $1 == "$ROOT/"*/os &&
        $relative != */*/* ]] || die "release outside @upper: $1"
}
resolve_layout() {
    HOST_MOUNT=$(normalize_path "$(setting GOLDEN_HOST_MOUNT /mnt)")
    WORK=$(normalize_path "$(setting GOLDEN_WORK "$HOST_MOUNT/@opdbus-system-a")")
    ROOT=$(normalize_path "$(setting GOLDEN_ROOT "$HOST_MOUNT/@upper")")
    FSID=$(setting GOLDEN_FSID)
    WORK_UUID=$(setting GOLDEN_WORK_UUID)
    UPPER_UUID=$(setting GOLDEN_UPPER_UUID)
    STATE=$ROOT/golden-state.json
    CANDIDATE_STATE=$ROOT/golden-candidate.json
    SOURCE_REPO=$(setting GOLDEN_SOURCE_REPO /srv/git/odbus-verified)
    LIVE= ROLLBACK=
    if [[ -f $STATE ]]; then
        LIVE=$(jq -er '.live.path // empty' "$STATE")
        ROLLBACK=$(jq -r '.rollback.path // empty' "$STATE")
        release_path "$LIVE"
        [[ -z $ROLLBACK ]] || release_path "$ROLLBACK"
    fi
}
require_layout() {
    [[ $(id -u) == 0 ]] || die 'apply must run as root'
    [[ -n $FSID && -n $WORK_UUID && -n $UPPER_UUID ]] || die 'filesystem and subvolume UUID settings are required'
    [[ $WORK == "$HOST_MOUNT/@opdbus-system-a" && $ROOT == "$HOST_MOUNT/@upper" ]] || die 'unsupported permanent layout'
    [[ $(findmnt -rn -M "$HOST_MOUNT" -o FSTYPE,FSROOT) == 'btrfs /' &&
        $(findmnt -rn -M "$HOST_MOUNT" -o UUID) == "$FSID" ]] || die 'top-level FSID differs'
    [[ $(realpath -e "$WORK") == "$WORK" && $(realpath -e "$ROOT") == "$ROOT" ]] || die 'layout contains symlinks'
    [[ $(subvol_uuid "$WORK") == "$WORK_UUID" && $(subvol_uuid "$ROOT") == "$UPPER_UUID" ]] || die 'subvolume UUID differs'
    subvol_readonly "$WORK" && die 'build OS must remain writable'
    [[ -f $WORK/etc/os-release ]] || die 'full OS absent'
    [[ $SOURCE_REPO == /* && $(realpath -e -- "$SOURCE_REPO") == "$SOURCE_REPO" ]] ||
        die "source repository must be an absolute path without symlinks: $SOURCE_REPO"
    [[ -f $SOURCE_REPO/Cargo.toml && ! -L $SOURCE_REPO/Cargo.toml ]] || die "source repository has no Cargo.toml: $SOURCE_REPO"
}
require_readonly_release() {
    release_path "$1"
    is_subvolume "$1" && subvol_readonly "$1" || die "not a readonly release: $1"
    [[ $(findmnt -rn -T "$1" -o UUID) == "$FSID" ]] || die 'release FSID differs'
    local record
    record=$(jq -ec --arg path "$1" '.live,.rollback | select(.path==$path)' "${ACT_STATE:-$STATE}")
    [[ $(subvol_id "$1") == "$(jq -r .id <<<"$record")" &&
        $(subvol_uuid "$1") == "$(jq -r .uuid <<<"$record")" ]] || die 'release identity differs'
}
atomic_state() {
    local tmp
    tmp=$(mktemp "$ROOT/.golden-state.XXXXXX")
    cat >"$tmp"
    chmod 0644 "$tmp"; sync -f "$tmp"
    mv -T -- "$tmp" "$STATE"; sync -f "$ROOT"
}
usage() {
    cat <<'EOF'
Usage: golden.sh [--apply|--dry-run] [--with-build] [--jobs N] [--mount PATH] [--force] [--allow-catalog-change] VERB
Verbs: status plan init build seal cut release rotate rollback catalog-prepare mount-overlay activate activate-fuse deploy project recover
init verifies the existing writable @opdbus-system-a; it never creates an OS.
build bind-mounts GOLDEN_SOURCE_REPO (default /srv/git/odbus-verified) at
/opt/op-build.src and records execution only; a retained copy there is kept.
build preflight (preflight.sh) refuses a non-default source, a source/sealed-catalog
plugin version mismatch (--allow-catalog-change) and an already-built commit (--force).
seal separately reviews plugin removals and seals/stages inside the build OS.
cut refuses unless the repo digest equals the execution receipt, then creates a
unique readonly @upper/<release>/os and retains every old release.
rollback transacts the complete prior owner selection; recover retries an incomplete restore.
project journals the boot opblob projection and admin-down FUSE pins onto the current live release.
Settings: /etc/op-dbus/env/GOLDEN_{HOST_MOUNT,WORK,ROOT,FSID,WORK_UUID,UPPER_UUID,SOURCE_REPO}.
Dry-run is the default; no Incus/ublk build-root fallback. Cut/activation never seal.
Build never seals; seal never publishes the host's live or persisted catalog.
EOF
}
parse_args() {
    local selected=0
    while (($#)); do
        case $1 in
            --apply) APPLY=1 ;;
            --dry-run) APPLY=0 ;;
            --with-build) WITH_BUILD=1 ;;
            --force) FORCE=1 ;;
            --allow-catalog-change) ALLOW_CATALOG_CHANGE=1 ;;
            --jobs) [[ $# -ge 2 ]] || die 'missing jobs'; JOBS=$2; shift ;;
            --mount) [[ $# -ge 2 ]] || die 'missing mount'; MOUNT_POINT=$2; shift ;;
            -h|--help) usage; exit 0 ;;
            --*) die "unsupported flag: $1" ;;
            *) [[ $selected == 0 ]] || die 'only one verb accepted'; VERB=$1; selected=1 ;;
        esac
        shift
    done
    [[ -n $JOBS ]] || JOBS=$(setting GOLDEN_CARGO_JOBS 2)
    [[ $JOBS =~ ^[1-9][0-9]*$ ]] || die 'jobs must be positive'
    case $VERB in status|plan|init|build|seal|cut|release|rotate|rollback|catalog-prepare|mount-overlay|activate|activate-fuse|deploy|project|recover) ;; *) die "unknown verb: $VERB";; esac
}
print_status() {
    printf 'work=%s\nrelease_root=%s\nstate=%s\nlive=%s\nrollback=%s\n' "$WORK" "$ROOT" "$STATE" "$LIVE" "$ROLLBACK"
    [[ ! -f $STATE ]] || cat "$STATE"
}
# Build caches live in a nested subvolume of the build OS. A btrfs snapshot does
# not descend into nested subvolumes, so `cut` releases get an empty directory
# here, never the cache contents.
BUILD_CACHE=/var/cache/op-build
BUILD_CXXFLAGS='-include cstdint'
build_cache_prepare() {
    local host=$WORK$BUILD_CACHE
    if ! is_subvolume "$host"; then
        [[ ! -e $host && ! -L $host ]] || die "build cache exists but is not a subvolume: $host"
        install -d -m 0755 "${host%/*}"
        btrfs subvolume create "$host" >/dev/null
    fi
    install -d -m 0700 "$host/ccache" "$host/rocksdb"
}
# Keyed by the locked cozorocks source checksum, target and C++ flags, so a
# cozorocks bump or flag change compiles RocksDB again instead of reusing a stale archive.
cozorocks_locked() {
    awk -v field="$1" '/^\[\[package\]\]/{f=0} /^name = "cozorocks"$/{f=1} f && $1==field {gsub(/"/,"",$3); print $3; exit}' "$SOURCE_REPO/Cargo.lock"
}
rocksdb_cache_dir() {
    local sum
    sum=$(cozorocks_locked checksum)
    [[ $sum =~ ^[0-9a-f]{64}$ ]] || die 'cozorocks checksum absent from Cargo.lock'
    printf '%s/rocksdb/%s' "$BUILD_CACHE" \
        "$(printf '%s|x86_64-unknown-linux-gnu|%s' "$sum" "$BUILD_CXXFLAGS" | sha256sum | cut -c1-16)"
}
# Keep a RocksDB archive already compiled in the build target. Only an out dir
# whose build-script output names the locked cozorocks version is accepted.
rocksdb_cache_harvest() {
    local dir=$WORK$1 version pattern newest='' stamp path unit
    [[ ! -f $dir/librocksdb.a ]] || return 0
    version=$(cozorocks_locked version)
    [[ $version =~ ^[0-9]+\.[0-9]+\.[0-9]+([-+][0-9A-Za-z.-]+)?$ ]] || die 'cozorocks version absent from Cargo.lock'
    pattern="/cozorocks-${version//./\\.}([^0-9A-Za-z.+-]|$)"
    while read -r stamp path; do
        unit=${path%/out/librocksdb.a}
        if grep -Eqs -- "$pattern" "$unit/run/stdout" "$unit/output"; then newest=$path; break; fi
    done < <(find "$WORK/opt/op-build/target/x86_64-unknown-linux-gnu/release/build" \
        -path '*cozorocks*/out/librocksdb.a' -type f -printf '%T@ %p\n' 2>/dev/null | sort -rn)
    [[ -n $newest ]] || { warn "no built librocksdb.a for cozorocks $version to cache"; return 0; }
    install -d -m 0700 "$dir"
    install -m 0600 "$newest" "$dir/.librocksdb.a.tmp"
    mv -T "$dir/.librocksdb.a.tmp" "$dir/librocksdb.a"
    log "cached RocksDB archive for cozorocks $version at $1/librocksdb.a (from ${newest#"$WORK"})"
}
plan_build() {
    plan_preflight
    would "bind $SOURCE_REPO at $WORK/opt/op-build.src (retained copy renamed once, kept); chroot $WORK; cd /opt/op-build.src"
    would "build caches in nested subvolume $WORK$BUILD_CACHE: ccache gcc/g++ (installed into the build OS if absent); target librocksdb.a harvested first, then linked via ROCKSDB_LIB_DIR"
    would "paired retained schema capture; native --target x86_64-unknown-linux-gnu; CXXFLAGS='$BUILD_CXXFLAGS'; cargo build --workspace --release -j$JOBS"
}
do_build() {
    if [[ $APPLY != 1 ]]; then plan_build; return; fi
    require_lifecycle_lock
    assert_no_cargo
    [[ -x $WORK/root/.cargo/bin/cargo ]] || die 'retained toolchain absent'
    local commit dirty digest
    commit=$(source_commit) && [[ $commit =~ ^[0-9a-f]{40}$ ]] || die "source repository HEAD unreadable: $SOURCE_REPO"
    dirty=$(source_dirty)
    digest=$(source_digest "$SOURCE_REPO")
    preflight_source
    preflight_catalog
    preflight_receipt "$commit" "$digest"
    capture_pair
    install -d -m 0700 "$WORK$CAPTURE_CONFIG"
    build_cache_prepare
    local rocks_dir cache_env=()
    rocks_dir=$(rocksdb_cache_dir)
    rocksdb_cache_harvest "$rocks_dir"
    preflight_caches "$rocks_dir"
    if [[ -f $WORK$rocks_dir/librocksdb.a ]]; then
        cache_env+=(ROCKSDB_LIB_DIR="$rocks_dir" ROCKSDB_STATIC=1)
        log "linking cached RocksDB from $rocks_dir"
    fi
    if [[ ! -x $WORK/usr/bin/ccache ]]; then
        log 'installing ccache into the build OS only'
        ( build_mounts_enter
          chroot "$WORK" env DEBIAN_FRONTEND=noninteractive PATH=/usr/sbin:/usr/bin:/sbin:/bin \
              apt-get install -y --no-install-recommends ccache )
        [[ -x $WORK/usr/bin/ccache ]] || die 'ccache install into the build OS failed'
    fi
    cache_env+=(CC='ccache gcc' CXX='ccache g++' CCACHE_DIR="$BUILD_CACHE/ccache"
        CCACHE_MAXSIZE=20G CCACHE_COMPILERCHECK=content)
    local sccache_line
    while IFS= read -r sccache_line; do [[ -z $sccache_line ]] || cache_env+=("$sccache_line"); done < <(sccache_env)
    jq -n --arg commit "$commit" --argjson dirty "$dirty" --arg repo "$SOURCE_REPO" --arg digest "$digest" \
        '{exit:null,phase:"building",source_commit:$commit,source_dirty:$dirty,source_repo:$repo,source_digest:$digest}' \
        >"$WORK/opt/op-build/execution-receipt.json"
    sync -f "$WORK/opt/op-build/execution-receipt.json"
    (
        build_mounts_enter
        chroot "$WORK" env CXXFLAGS="$BUILD_CXXFLAGS" CARGO_TARGET_DIR=/opt/op-build/target \
            THREETCHED_CAPTURE_BIN="$CAPTURE_BIN" THREETCHED_CAPTURE_MANIFEST="$CAPTURE_MANIFEST" \
            THREETCHED_CONFIG_DIR="$CAPTURE_CONFIG" "${cache_env[@]}" \
            HOME=/root PATH=/root/.cargo/bin:/usr/local/bin:/usr/bin:/bin \
            sh -c 'cd /opt/op-build.src && exec /root/.cargo/bin/cargo build --workspace --release --target x86_64-unknown-linux-gnu -j"$1"' sh "$JOBS"
    )
    rocksdb_cache_harvest "$rocks_dir"
    record_build_receipt "$digest" "$commit" "$dirty"
}
do_cut() {
    if [[ $APPLY != 1 ]]; then
        [[ $WITH_BUILD != 1 ]] || plan_build
        would "readonly snapshot $WORK into new $ROOT/golden-release-<stamp>.XXXXXX/os"
        would "stage $CANDIDATE_STATE only; boot selection stays unchanged until complete deployment"
        return
    fi
    require_lifecycle_lock
    tx_no_pending
    [[ $WITH_BUILD != 1 ]] || do_build
    assert_no_cargo
    verify_release_inputs "$WORK" "$SOURCE_REPO"
    local directory commit digest record previous=null
    commit=$(jq -er .source_commit "$WORK/opt/op-build/execution-receipt.json")
    digest=$(jq -er .source_digest "$WORK/opt/op-build/execution-receipt.json")
    directory=$(mktemp -d "$ROOT/golden-release-$(date -u +%Y%m%dT%H%M%SZ).XXXXXX")
    btrfs subvolume snapshot -r "$WORK" "$directory/os"
    subvol_readonly "$directory/os" || die 'snapshot is not readonly'
    verify_release_inputs "$directory/os"
    [[ ! -f $STATE ]] || previous=$(jq -c .live "$STATE")
    record=$(jq -nc --arg path "$directory/os" --arg id "$(subvol_id "$directory/os")" \
        --arg uuid "$(subvol_uuid "$directory/os")" --arg commit "$commit" --arg digest "$digest" \
        '{path:$path,id:$id,uuid:$uuid,commit:$commit,source_digest:$digest,readonly:true}')
    ( STATE=$CANDIDATE_STATE
      jq -n --arg fsid "$FSID" --arg work "$WORK" --arg uuid "$WORK_UUID" \
        --argjson live "$record" --argjson rollback "$previous" \
        '{version:2,fsid:$fsid,work:{path:$work,uuid:$uuid},live:$live,rollback:$rollback}' | atomic_state
    )
}
do_rollback() {
    [[ -n $LIVE && -n $ROLLBACK ]] || die 'no live/rollback pair'
    if [[ $APPLY != 1 ]]; then would 'stage reverse selection and transact the complete owner transition'; return; fi
    require_lifecycle_lock
    require_readonly_release "$LIVE"; require_readonly_release "$ROLLBACK"
    jq '.live as $live | .live=.rollback | .rollback=$live' "$STATE" >"$CANDIDATE_STATE.new"
    sync -f "$CANDIDATE_STATE.new"; mv -T "$CANDIDATE_STATE.new" "$CANDIDATE_STATE"
    do_catalog_prepare
    deploy_candidate
}
mount_one() {
    local target=$1 lower=$2 upper=${2%/os}/upper work=${2%/os}/work options
    case $target in /run/opdbus-os-golden|/run/opdbus-os-live|/run/opdbus-os-stage.*) ;; *) die "unsupported target: $target";; esac
    options="lowerdir=$lower,upperdir=$upper,workdir=$work"
    if mountpoint -q "$target"; then
        [[ $(findmnt -rn -M "$target" -o FSTYPE) == overlay &&
            ,$(findmnt -rn -M "$target" -o OPTIONS), == *",$options,"* ]] || die "existing mount differs: $target"
        return
    fi
    install -d -m 0755 -- "$upper" "$work" "$target"
    [[ -z $(find "$target" -mindepth 1 -maxdepth 1 -print -quit) ]] || die 'nonempty mount target'
    mount -t overlay overlay -o "$options" "$target"
}
do_mount_overlay() {
    [[ -n $LIVE ]] || die 'no selected release'
    if [[ $APPLY != 1 ]]; then would "mount $LIVE at ${MOUNT_POINT:-/run/opdbus-os-golden}"; return; fi
    require_readonly_release "$LIVE"
    mount_one "${MOUNT_POINT:-/run/opdbus-os-golden}" "$LIVE"
}
do_deploy() {
    if [[ $APPLY != 1 ]]; then do_cut; would 'transact candidate overlay/services/FUSE, then publish boot selection last'; return; fi
    do_cut
    do_catalog_prepare
    deploy_candidate
}
. "$SCRIPT_DIR/activate.sh"
. "$SCRIPT_DIR/build-receipt.sh"
. "$SCRIPT_DIR/preflight.sh"
. "$SCRIPT_DIR/catalog-transition.sh"
. "$SCRIPT_DIR/transaction.sh"
main() {
    parse_args "$@"; resolve_layout
    if [[ $APPLY == 1 ]]; then
        require_layout
        exec 9>"$ROOT/.golden.lock"
        flock -n 9 || die 'another golden operation owns this layout'
    fi
    case $VERB in
        status) print_status ;;
        plan) print_status; plan_build; do_cut ;;
        init) [[ $APPLY != 1 ]] || log "retained writable build OS verified: $WORK" ;;
        build) do_build ;;
        seal) record_catalog_seal ;;
        cut|release|rotate) do_cut ;;
        rollback) do_rollback ;;
        catalog-prepare) do_catalog_prepare ;;
        mount-overlay) do_mount_overlay ;;
        activate|activate-fuse)
            if [[ $APPLY == 1 ]]; then deploy_candidate; else act_print_plan; fi
            ;;
        recover) do_recover ;;
        project) do_project ;;
        deploy) do_deploy ;;
    esac
}
if [[ ${GOLDEN_LIB:-0} != 1 ]]; then main "$@"; fi
