#!/bin/sh
# Install the base-system release kit. Application files belong to release
# overlays; this installer never activates a release or changes kernel/storage pins.
set -eu
PATH=/usr/sbin:/usr/bin:/sbin:/bin
export PATH
umask 077
fail() { printf 'install-kit: %s\n' "$*" >&2; exit 1; }
[ "$(id -u)" = 0 ] || fail 'run as root'
[ "$#" = 0 ] || fail 'takes no arguments; OP_RELEASE_DIR selects built artifacts'
repo=$(CDPATH= cd -- "$(dirname -- "$(readlink -f "$0")")/../.." && pwd)
release_dir=${OP_RELEASE_DIR:-$repo/target/x86_64-unknown-linux-gnu/release}
[ -d "$release_dir" ] || fail "GNU release directory missing: $release_dir"
release=$(realpath -e -- "$release_dir")
base=/usr/local/libexec/opdbus-release
state=/var/lib/opdbus-release
secure_directory() {
    [ -d "$1" ] && [ ! -L "$1" ] &&
        [ "$(stat -c %u:%g "$1")" = 0:0 ] &&
        [ "$((0$(stat -c %a "$1") & 07022))" -eq 0 ] || fail "insecure directory: $1"
}
# A kit under an application overlay cannot recover that overlay at boot.
root_device=$(stat -c %d /)
for directory in / /usr /usr/local /var /var/lib /etc /etc/init.d; do
    secure_directory "$directory"
    [ "$(stat -c %d "$directory")" = "$root_device" ] || fail "not on rootfs: $directory"
    [ "$(findmnt -n -o FSTYPE --target "$directory")" != overlay ] || fail "rootfs kit path is covered by overlay: $directory"
done
for directory in /usr/local/libexec "$base" "$state"; do
    if [ ! -e "$directory" ] && [ ! -L "$directory" ]; then
        install -d -o root -g root -m 0755 "$directory"
    fi
    secure_directory "$directory"
    [ "$(stat -c %d "$directory")" = "$root_device" ] || fail "not on rootfs: $directory"
    [ "$(findmnt -n -o FSTYPE --target "$directory")" != overlay ] || fail "kit path is covered by overlay: $directory"
done
[ "$(stat -c %a "$base")" = 755 ] || fail 'kit directory must be mode 0755'
# Lock order: private kit lock, then transaction lock. Never replace either inode.
create_lock() {
    path=$1
    if [ ! -e "$path" ] && [ ! -L "$path" ]; then
        (set -C; : > "$path") 2>/dev/null || [ -f "$path" ] || fail "cannot create lock: $path"
    fi
    [ -f "$path" ] && [ ! -L "$path" ] &&
        [ "$(stat -c %u:%g:%h:%a "$path")" = 0:0:1:600 ] &&
        [ "$(stat -c %d "$path")" = "$root_device" ] || fail "insecure rootfs lock: $path"
    mountpoint -q "$path" && fail "lock is a mount: $path"
    return 0
}
create_lock "$state/kit.lock"
exec 8<"$state/kit.lock"
flock -n -x 8 || fail 'the release kit is in use'

for entry in "$base"/* "$base"/.[!.]* "$base"/..?*; do
    [ -e "$entry" ] || [ -L "$entry" ] || continue
    case "${entry##*/}" in op-release|op-release-launch|manifest.sha256) ;; *) fail "unexpected kit entry: $entry" ;; esac
done
for destination in "$base/op-release" "$base/op-release-launch" "$base/manifest.sha256" /etc/init.d/op-release-activate "$state/lock"; do
    [ ! -L "$destination" ] || fail "symlink destination: $destination"
    if [ -e "$destination" ]; then
        [ -f "$destination" ] && [ "$(stat -c %u:%g:%h "$destination")" = 0:0:1 ] &&
            [ "$((0$(stat -c %a "$destination") & 022))" -eq 0 ] || fail "insecure destination: $destination"
        [ "$(stat -c %d "$destination")" = "$root_device" ] || fail "destination is outside rootfs: $destination"
        mountpoint -q "$destination" && fail "destination is a mount: $destination"
    fi
done
create_lock "$state/lock"
exec 9>>"$state/lock"
flock -n 9 || fail 'a release operation or kit installation is active'
[ ! -e "$state/slots.json" ] || {
    jq -e '.journal == null' "$state/slots.json" >/dev/null || fail 'recover the release journal with the existing kit before replacing it'
}
[ -f "$release/op-release" ] && [ -x "$release/op-release" ] || fail "build op-release first: $release/op-release"
command -v readelf >/dev/null 2>&1 || fail 'readelf is required to verify the release controller'
header=$(readelf -hW "$release/op-release") || fail "op-release is not ELF: $release/op-release"
printf '%s\n' "$header" | grep -q 'Class:[[:space:]]*ELF64' || fail "op-release is not ELF64: $release/op-release"
printf '%s\n' "$header" | grep -q 'Machine:.*X86-64' || fail "op-release is not x86_64: $release/op-release"
readelf -lW "$release/op-release" | grep -q 'ld-linux-x86-64\.so\.2' || fail "op-release is not a GNU-target binary: $release/op-release"
[ -f "$repo/deploy/release/op-release-launch" ] && [ -f "$repo/deploy/release/op-release-activate" ] || fail 'kit source scripts missing'
stage=$(mktemp -d /usr/local/libexec/.opdbus-kit.XXXXXX)
boot_tmp=
cleanup() {
    rm -rf -- "$stage"
    [ -z "$boot_tmp" ] || rm -f -- "$boot_tmp"
}
trap cleanup EXIT
trap 'exit 1' HUP INT TERM
install -m 0755 "$release/op-release" "$stage/op-release"
install -m 0755 "$repo/deploy/release/op-release-launch" "$stage/op-release-launch"
install -m 0755 "$repo/deploy/release/op-release-activate" "$stage/op-release-activate"
for name in op-release op-release-launch op-release-activate; do
    case "$name" in op-release) source=$release/op-release ;; *) source=$repo/deploy/release/$name ;; esac
    cmp -s "$source" "$stage/$name" || fail "source changed during preparation: $source"
done
sh -n "$stage/op-release-launch"
sh -n "$stage/op-release-activate"
{
    printf '%s  %s\n' "$(sha256sum "$stage/op-release" | cut -d' ' -f1)" "$base/op-release"
    printf '%s  %s\n' "$(sha256sum "$stage/op-release-launch" | cut -d' ' -f1)" "$base/op-release-launch"
    printf '%s  %s\n' "$(sha256sum "$stage/op-release-activate" | cut -d' ' -f1)" /etc/init.d/op-release-activate
} > "$stage/manifest.sha256"
backup=$(mktemp -d "$state/kit-backup.XXXXXX")
for name in op-release op-release-launch manifest.sha256; do
    [ ! -e "$base/$name" ] || cp -p -- "$base/$name" "$backup/$name"
done
[ ! -e /etc/init.d/op-release-activate ] || cp -p /etc/init.d/op-release-activate "$backup/op-release-activate"
# Publish approval last: an interrupted mixed kit fails closed at its manifest.
mv -fT "$stage/op-release" "$base/op-release"
mv -fT "$stage/op-release-launch" "$base/op-release-launch"
boot_tmp=$(mktemp /etc/init.d/.op-release-activate.XXXXXX)
install -m 0755 "$stage/op-release-activate" "$boot_tmp"
mv -fT "$boot_tmp" /etc/init.d/op-release-activate
mv -fT "$stage/manifest.sha256" "$base/manifest.sha256"
sha256sum --strict --check --quiet "$base/manifest.sha256"
sync -f "$base"
sync -f /etc/init.d
printf 'Installed rootfs kit; backup: %s\n' "$backup"
printf 'No release activated. Kernel, receiver and first-activation policies are unchanged.\n'
