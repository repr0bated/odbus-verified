#!/bin/sh
# cargo shim: install as `cargo` ahead of rustup's cargo in PATH, e.g.
#   ln -s /srv/git/odbus-verified/deploy/lib/golden/cargo-shim.sh ~/.local/bin/cargo
# Under the shared @git (/srv/git) every cargo command runs in the golden build
# OS chroot (cargo-chroot.sh, via sudo). Elsewhere, or with GOLDEN_CARGO_HOST=1,
# it runs the next real cargo on PATH unchanged.
set -eu
self=$(readlink -f -- "$0")
shared=${GOLDEN_SHARED_GIT:-/srv/git}
case $(pwd -P)/ in
    "$shared"/*/*)
        if [ "${GOLDEN_CARGO_HOST:-0}" != 1 ]; then
            exec sudo -n env GOLDEN_CARGO_CWD="$(pwd -P)" \
                ${GOLDEN_CARGO_TARGET_DIR:+GOLDEN_CARGO_TARGET_DIR="$GOLDEN_CARGO_TARGET_DIR"} \
                "$(dirname -- "$self")/cargo-chroot.sh" "$@"
        fi
        ;;
esac
# Pass through to the next cargo on PATH that is not this shim.
IFS=:
for dir in $PATH; do
    candidate=$dir/cargo
    [ -x "$candidate" ] || continue
    [ "$(readlink -f -- "$candidate")" = "$self" ] && continue
    exec "$candidate" "$@"
done
echo 'cargo shim: no real cargo found on PATH' >&2
exit 127
