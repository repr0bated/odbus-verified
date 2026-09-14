#!/bin/sh
# Execute only the side-effect-free runtime check from an isolated Alpine /tmp
# tree. No release files, mail configuration, accounts, or services are changed.
set -eu

die() {
    printf 'mail-3tched verification: %s\n' "$*" >&2
    exit 1
}

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
PROJECT_ROOT=$(CDPATH= cd -- "$SCRIPT_DIR/../.." && pwd)
ARTIFACT=${1:-"$PROJECT_ROOT/target/container-artifacts/mail-3tched-x86_64-linux-gnu-bundle.tar"}
CONTAINER=${OP_MAIL_FIXTURE_CONTAINER:-mail-3tched}
[ -f "$ARTIFACT" ] || die "artifact is absent: $ARTIFACT"
incus info "$CONTAINER" >/dev/null || die "container is unavailable: $CONTAINER"
tar -tf "$ARTIFACT" | awk '
    /^\// || /(^|\/)\.\.($|\/)/ { bad = 1 }
    END { exit bad ? 1 : 0 }
' || die "artifact contains an unsafe path"
for required in \
    rootfs/usr/local/bin/op-mail-admin \
    rootfs/usr/local/lib/op-mail-admin/op-mail-admin.bin \
    rootfs/usr/local/lib/op-mail-admin/ld-linux-x86-64.so.2 \
    rootfs/etc/init.d/op-mail-admin \
    rootfs/etc/postfix/opdbus-main.cf \
    rootfs/etc/postfix/opdbus-domains \
    rootfs/etc/dovecot/opdbus.conf
do
    tar -tf "$ARTIFACT" | grep -qx "$required" ||
        die "artifact is missing $required"
done

root="/tmp/opdbus-mail-runtime-$$"
case "$root" in /tmp/opdbus-mail-runtime-*) ;; *) die "unsafe fixture root" ;; esac
cleanup() {
    incus exec "$CONTAINER" -- find "$root" -depth -delete >/dev/null 2>&1 || true
}
trap cleanup EXIT HUP INT TERM
incus exec "$CONTAINER" -- install -d -m 0700 "$root"
incus exec "$CONTAINER" -- dd "of=$root/artifact.tar" status=none < "$ARTIFACT"
incus exec "$CONTAINER" -- chmod 0600 "$root/artifact.tar"
incus exec "$CONTAINER" -- tar -xf "$root/artifact.tar" -C "$root"

output=$(incus exec "$CONTAINER" -- \
    "$root/rootfs/usr/local/bin/op-mail-admin" --check-runtime)
[ "$output" = '{"service":"op-mail-admin","runtime":"gnu","status":"ok"}' ] ||
    die "unexpected runtime check: $output"
printf '%s\n' "$output"
