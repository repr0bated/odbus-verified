#!/usr/bin/env bash
# cargo-chroot.sh — run any cargo command inside the golden build OS chroot.
#
#   sudo GOLDEN_CARGO_CWD="$PWD" deploy/lib/golden/cargo-chroot.sh <cargo args...>
#
# Normally reached through the `cargo` shim (cargo-shim.sh), not by hand.
# golden.sh is loaded as a library (GOLDEN_LIB=1) and is not changed: this uses
# its layout checks, lifecycle lock, single-Cargo rule, paired schema capture,
# caches and source bind, with the same environment as `golden.sh build`.
#
# Mounts inside the chroot, removed again on exit (only those this run made):
#   /opt/op-build.src  <- the golden source repo (GOLDEN_SOURCE_REPO)
#   /srv/git           <- the shared @git repositories (GOLDEN_SHARED_GIT)
#   /proc /sys /dev
# Working directory mapping (the caller's cwd must be under the shared @git):
#   golden source repo   -> /opt/op-build.src/...  target /opt/op-build/target
#   /srv/git/<repo>/...  -> /srv/git/<repo>/...    target /opt/op-build/target-<repo>
# GOLDEN_CARGO_TARGET_DIR overrides the target directory.
set -euo pipefail
CARGO_ARGS=("$@")
GOLDEN_LIB=1
# shellcheck source=golden.sh
. "$(cd -- "$(dirname -- "$(readlink -f -- "${BASH_SOURCE[0]}")")" && pwd)/golden.sh"

SHARED_GIT=$(normalize_path "$(setting GOLDEN_SHARED_GIT /srv/git)")

shared_git_bind_enter() {
    local mp=$WORK$SHARED_GIT
    [[ -d $SHARED_GIT && ! -L $SHARED_GIT ]] || die "shared repositories absent: $SHARED_GIT"
    if mountpoint -q -- "$mp"; then
        [[ $(stat -c %d:%i -- "$mp") == "$(stat -c %d:%i -- "$SHARED_GIT")" ]] ||
            die "$mp is busy with a mount other than $SHARED_GIT"
        return
    fi
    [[ ! -L $mp ]] || die "$mp is a symlink"
    if [[ -e $mp ]] && [[ ! -d $mp || -n $(find "$mp" -mindepth 1 -maxdepth 1 -print -quit) ]]; then
        die "$mp exists and is not empty; refusing to cover it"
    fi
    install -d -m 0755 -- "$mp"
    mount --bind -- "$SHARED_GIT" "$mp"
    BUILD_BINDS+=("$mp")
    mount --make-rslave -- "$mp"
}

# Same environment as do_build in golden.sh, without its execution receipt.
prepare_cargo_env() {
    capture_pair
    install -d -m 0700 "$WORK$CAPTURE_CONFIG"
    build_cache_prepare
    local rocks_dir sccache_line
    rocks_dir=$(rocksdb_cache_dir)
    rocksdb_cache_harvest "$rocks_dir"
    CARGO_ENV=(CXXFLAGS="$BUILD_CXXFLAGS"
        THREETCHED_CAPTURE_BIN="$CAPTURE_BIN" THREETCHED_CAPTURE_MANIFEST="$CAPTURE_MANIFEST"
        THREETCHED_CONFIG_DIR="$CAPTURE_CONFIG"
        HOME=/root PATH=/root/.cargo/bin:/usr/local/bin:/usr/bin:/bin)
    [[ ! -f $WORK$rocks_dir/librocksdb.a ]] || CARGO_ENV+=(ROCKSDB_LIB_DIR="$rocks_dir" ROCKSDB_STATIC=1)
    # ccache is only used when golden.sh build has already installed it.
    [[ ! -x $WORK/usr/bin/ccache ]] || CARGO_ENV+=(CC='ccache gcc' CXX='ccache g++'
        CCACHE_DIR="$BUILD_CACHE/ccache" CCACHE_MAXSIZE=20G CCACHE_COMPILERCHECK=content)
    while IFS= read -r sccache_line; do [[ -z $sccache_line ]] || CARGO_ENV+=("$sccache_line"); done < <(sccache_env)
}

# cargo runs as root in the chroot; give a rewritten Cargo.lock back to the caller.
restore_lock_owner() {
    local repo=$1
    [[ -n ${SUDO_UID:-} && -n ${SUDO_GID:-} ]] || return 0
    find "$repo" -maxdepth 4 -name Cargo.lock -user 0 -not -path '*/target/*' \
        -exec chown "$SUDO_UID:$SUDO_GID" {} + 2>/dev/null || true
}

main_cargo() {
    resolve_layout
    require_layout
    exec 9>"$ROOT/.golden.lock"
    flock -n 9 || die 'a golden operation owns this layout; cargo not started'
    assert_no_cargo
    [[ -x $WORK/root/.cargo/bin/cargo ]] || die 'retained toolchain absent'
    local cwd rel repo inside target host_repo
    cwd=$(realpath -e -- "${GOLDEN_CARGO_CWD:-$PWD}")
    if [[ $cwd == "$SOURCE_REPO" || $cwd == "$SOURCE_REPO/"* ]]; then
        inside=$SOURCE_MOUNT${cwd#"$SOURCE_REPO"}
        target=/opt/op-build/target
        host_repo=$SOURCE_REPO
    elif [[ $cwd == "$SHARED_GIT/"* ]]; then
        rel=${cwd#"$SHARED_GIT/"}
        repo=${rel%%/*}
        [[ $repo =~ ^[A-Za-z0-9._-]+$ && $repo != . && $repo != .. ]] || die "unsupported repository name: $repo"
        inside=$SHARED_GIT/$rel
        target=/opt/op-build/target-$repo
        host_repo=$SHARED_GIT/$repo
    else
        die "cargo runs in the build chroot only for repositories under $SHARED_GIT (cwd: $cwd)"
    fi
    prepare_cargo_env
    local status=0
    (
        build_mounts_enter
        shared_git_bind_enter
        chroot "$WORK" env CARGO_TARGET_DIR="${GOLDEN_CARGO_TARGET_DIR:-$target}" "${CARGO_ENV[@]}" \
            sh -c 'cd "$1" && shift && exec /root/.cargo/bin/cargo "$@"' sh "$inside" "${CARGO_ARGS[@]}"
    ) || status=$?
    restore_lock_owner "$host_repo"
    return "$status"
}

main_cargo
