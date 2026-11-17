#!/bin/sh
# Install the verified static receiver as rootfs bootstrap, never release content.
set -eu
PATH=/usr/sbin:/usr/bin:/sbin:/bin
export PATH
umask 077
fail() { printf 'install-receiver: %s\n' "$*" >&2; exit 1; }
[ "$(id -u)" = 0 ] && [ "$#" = 0 ] || fail 'run as root without arguments'
repo=$(CDPATH='' cd -- "$(dirname -- "$(readlink -f "$0")")/../.." && pwd)
artifacts=$(realpath -e -- "${OP_RECEIVER_ARTIFACT_DIR:-$repo/target/btrfs-receiver-artifacts}")
state=/var/lib/opdbus-release
base=/usr/local/libexec/opdbus-release-receiver
secure_directory() {
    [ -d "$1" ] && [ ! -L "$1" ] && [ "$(stat -c %u:%g "$1")" = 0:0 ] &&
        [ "$((0$(stat -c %a "$1") & 07022))" = 0 ] &&
        [ "$(stat -c %d "$1")" = "$(stat -c %d /)" ] &&
        [ "$(findmnt -n -o FSTYPE --target "$1")" != overlay ] || fail "insecure or non-rootfs directory: $1"
}
for directory in / /usr /usr/local /var /var/lib "$state"; do secure_directory "$directory"; done
for lock in "$state/kit.lock" "$state/lock"; do
    [ -f "$lock" ] && [ ! -L "$lock" ] && [ "$(stat -c %u:%g:%a:%h "$lock")" = 0:0:600:1 ] || fail 'prepare rootfs release locks first'
done
exec 8<"$state/kit.lock"
flock -n -x 8 || fail 'release kit is in use'
exec 9<"$state/lock"
flock -n -x 9 || fail 'release transaction is active'
[ ! -e "$state/slots.json" ] || jq -e '.journal == null' "$state/slots.json" >/dev/null || fail 'pending transaction must be recovered first'
[ -f "$artifacts/btrfs" ] && [ ! -L "$artifacts/btrfs" ] && [ -f "$artifacts/manifest.json" ] || fail 'prepared receiver artifacts missing'
sha=$(sha256sum "$artifacts/btrfs" | cut -d' ' -f1)
recipe=$(sha256sum "$repo/vendor/btrfs-receiver/build-receiver.sh" | cut -d' ' -f1)
pin=$(sha256sum "$repo/vendor/btrfs-receiver/source.pin" | cut -d' ' -f1)
jq -e --arg sha "$sha" --arg recipe "$recipe" --arg pin "$pin" \
    '.format == "opdbus.btrfs-receiver.v1" and .artifact.sha256 == $sha and
     .inputs.recipe_sha256 == $recipe and .inputs.source_pin_sha256 == $pin and
     .artifact.target == "x86_64-unknown-linux-gnu" and .artifact.elf.pt_interp == false and
     .artifact.elf.dt_needed == false' "$artifacts/manifest.json" >/dev/null || fail 'receiver provenance differs'
inspection=$(mktemp -d "$state/.receiver-inspect.XXXXXX")
candidate=
cleanup() {
    rm -rf -- "$inspection"
    [ -z "$candidate" ] || rm -f -- "$candidate"
}
trap cleanup EXIT
trap 'exit 1' HUP INT TERM
readelf -hW "$artifacts/btrfs" > "$inspection/header"
readelf -lW "$artifacts/btrfs" > "$inspection/programs"
readelf -dW "$artifacts/btrfs" > "$inspection/dynamic"
grep -q 'Advanced Micro Devices X86-64' "$inspection/header" || fail 'receiver must be native x86_64'
if grep -Eq '^[[:space:]]*INTERP[[:space:]]' "$inspection/programs" ||
    grep -F '(NEEDED)' "$inspection/dynamic" >/dev/null; then
    fail 'receiver is not static'
fi
for directory in /usr/local/libexec "$base"; do
    [ -e "$directory" ] || install -d -m 0755 "$directory"
    secure_directory "$directory"
done
for path in "$base/btrfs" "$state/receiver.json" "$state/receiver-build.json"; do
    [ ! -L "$path" ] || fail "symlink destination: $path"
    if [ -e "$path" ]; then
        [ -f "$path" ] && [ "$(stat -c %u:%g:%h "$path")" = 0:0:1 ] &&
            [ "$((0$(stat -c %a "$path") & 022))" = 0 ] &&
            [ "$(stat -c %d "$path")" = "$(stat -c %d /)" ] || fail "insecure destination: $path"
        mountpoint -q "$path" && fail "mounted destination: $path"
    fi
done
backup=$(mktemp -d "$state/receiver-backup.XXXXXX")
for path in "$base/btrfs" "$state/receiver.json" "$state/receiver-build.json"; do
    [ ! -e "$path" ] || cp -p "$path" "$backup/$(basename "$path")"
done
candidate=$(mktemp "$base/.btrfs.XXXXXX")
install -m 0755 "$artifacts/btrfs" "$candidate"
[ "$(sha256sum "$candidate" | cut -d' ' -f1)" = "$sha" ] || fail 'candidate bytes changed'
mv -T "$candidate" "$base/btrfs"
metadata=$(stat -c '%d %i %u %g %f' "$base/btrfs")
set -- $metadata
approval=$(mktemp "$state/.receiver.XXXXXX")
jq -n --arg path "$base/btrfs" --arg sha "$sha" --argjson device "$1" --argjson inode "$2" \
    --argjson uid "$3" --argjson gid "$4" --argjson mode "$((0x$5))" \
    '{path:$path,sha256:$sha,device:$device,inode:$inode,uid:$uid,gid:$gid,mode:$mode}' > "$approval"
provenance=$(mktemp "$state/.receiver-build.XXXXXX")
install -m 0600 "$artifacts/manifest.json" "$provenance"
mv -T "$provenance" "$state/receiver-build.json"
mv -T "$approval" "$state/receiver.json"
sync -f "$base"
sync -f "$state"
printf 'Installed static receiver on rootfs; SHA256 %s; backup %s\n' "$sha" "$backup"
