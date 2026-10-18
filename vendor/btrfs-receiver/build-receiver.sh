#!/bin/sh
# Preparation only: pristine upstream btrfs.static, never installation or Cargo.
set -eu
umask 022
export LC_ALL=C
package_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
. "$package_dir/source.pin"
cache_dir=
output_dir=target/btrfs-receiver-artifacts
jobs=1
usage() {
    echo 'usage: build-receiver.sh --cache-dir DIR [--output-dir DIR] [--jobs N]' >&2
    exit 2
}
fail() { printf '%s\n' "$*" >&2; exit 1; }
while [ "$#" -gt 0 ]; do
    case "$1" in
        --cache-dir) [ "$#" -ge 2 ] || usage; cache_dir=$2; shift 2 ;;
        --output-dir) [ "$#" -ge 2 ] || usage; output_dir=$2; shift 2 ;;
        --jobs) [ "$#" -ge 2 ] || usage; jobs=$2; shift 2 ;;
        *) usage ;;
    esac
done
[ -n "$cache_dir" ] || usage
case "$jobs" in ''|*[!0-9]*|0) usage ;; esac
for tool in gcc make ar ld pkg-config readelf jq curl tar xz sha256sum awk grep \
            sed bash getconf uname realpath mktemp cp chmod mv rm mkdir wc; do
    command -v "$tool" >/dev/null 2>&1 || fail "missing native receiver preparation tool: $tool"
done
[ "$(uname -m)" = x86_64 ] || fail 'receiver preparation requires native x86_64'
compiler_target=$(gcc -dumpmachine)
case "$compiler_target" in x86_64-*linux-gnu) ;; *) fail "not a native GNU compiler: $compiler_target" ;; esac
libc_version=$(getconf GNU_LIBC_VERSION) || fail 'native glibc is required'
case "$libc_version" in 'glibc '*) ;; *) fail 'musl is not a supported receiver target' ;; esac
# These are the compiler's native search paths, not a foreign static sysroot.
# Keep LZO/ZSTD: encoded writes can fall back to userspace decompression.
for library in libc.a libpthread.a libm.a libgcc.a libblkid.a libuuid.a libz.a libzstd.a liblzo2.a; do
    library_path=$(gcc -print-file-name="$library")
    [ "$library_path" != "$library" ] && [ -r "$library_path" ] ||
        fail "missing native static dependency: $library (supply its development package; do not disable stream support)"
done
pkg-config --exists blkid uuid zlib libzstd || fail 'missing native blkid/uuid/zlib/libzstd development metadata'
# Header preprocessing is part of preparation, not a dependency installation.
printf '#include <features.h>\n#include <lzo/lzo1x.h>\n#include <linux/fsverity.h>\n#ifndef __GLIBC__\n#error glibc required\n#endif\n' |
    gcc -E -x c - >/dev/null || fail 'native glibc, LZO2 and fsverity development headers are required'
digest() { sha256sum "$1" | awk '{print $1}'; }
verify_digest() { [ "$(digest "$1")" = "$2" ] || fail "receiver input digest mismatch: $1"; }
pin_sha256=$(digest "$package_dir/source.pin")
recipe_sha256=$(digest "$package_dir/build-receiver.sh")
provenance_sha256=$(digest "$package_dir/provenance.json")
verify_inputs() {
    verify_digest "$package_dir/source.pin" "$pin_sha256"
    verify_digest "$package_dir/build-receiver.sh" "$recipe_sha256"
    verify_digest "$package_dir/provenance.json" "$provenance_sha256"
    verify_digest "$package_dir/upstream-sha256sums.asc" "$UPSTREAM_CHECKSUMS_SHA256"
}
verify_inputs
grep -Fx "$SOURCE_ARCHIVE_SHA256  $SOURCE_ARCHIVE" "$package_dir/upstream-sha256sums.asc" >/dev/null ||
    fail 'source pin is absent from captured upstream checksums'
mkdir -p -- "$cache_dir" "$output_dir"
cache_dir=$(realpath "$cache_dir")
output_dir=$(realpath "$output_dir")
work_dir=$(mktemp -d "$cache_dir/build.XXXXXX")
cleanup() { status=$?; rm -rf -- "$work_dir"; exit "$status"; }
trap cleanup EXIT
trap 'exit 130' HUP INT TERM
archive="$cache_dir/$SOURCE_ARCHIVE"
if [ ! -f "$archive" ]; then
    curl --fail --location --proto '=https' --proto-redir '=https' \
        --output "$work_dir/$SOURCE_ARCHIVE" "$UPSTREAM_URL/$SOURCE_ARCHIVE"
    verify_digest "$work_dir/$SOURCE_ARCHIVE" "$SOURCE_ARCHIVE_SHA256"
    mv -- "$work_dir/$SOURCE_ARCHIVE" "$archive"
fi
verify_digest "$archive" "$SOURCE_ARCHIVE_SHA256"
tar -xJf "$archive" -C "$work_dir" --no-same-owner
source_dir="$work_dir/btrfs-progs-$UPSTREAM_TAG"
[ -f "$source_dir/configure" ] || fail 'pinned archive lacks its release configure script'
# Record the actual static inputs: source pin alone cannot fix host library versions.
: > "$work_dir/libraries.jsonl"
for library in libc.a libpthread.a libm.a libgcc.a libblkid.a libuuid.a libz.a libzstd.a liblzo2.a; do
    library_path=$(realpath "$(gcc -print-file-name="$library")")
    jq -n --arg name "$library" --arg path "$library_path" --arg sha256 "$(digest "$library_path")" \
        '{name:$name,path:$path,sha256:$sha256}' >> "$work_dir/libraries.jsonl"
done
jq -s . "$work_dir/libraries.jsonl" > "$work_dir/libraries.json"
# Use the upstream static target, not --enable-static alone (that enables libraries).
# env -i prevents inherited compiler/make/crypto overrides and nondeterministic flags.
(
    cd "$source_dir"
    env -i PATH="$PATH" LC_ALL=C TZ=UTC SOURCE_DATE_EPOCH="$SOURCE_DATE_EPOCH" \
        CC=gcc AR=ar CFLAGS="-O2 -Wall -D_FORTIFY_SOURCE=2 -ffile-prefix-map=$source_dir=." \
        LDFLAGS=-static ./configure \
        --build="$NATIVE_TARGET" --host="$NATIVE_TARGET" --target="$NATIVE_TARGET" \
        --enable-programs --enable-static --disable-shared --disable-documentation \
        --disable-python --disable-convert --disable-libudev --disable-backtrace \
        --disable-zoned --disable-experimental --with-crypto=builtin --enable-lzo --enable-zstd
    grep -Eq '^#define HAVE_LINUX_FSVERITY_H 1$' include/config.h ||
        fail 'fsverity receive support was not compiled in'
    grep -Eq '^HAVE_GLIBC = 1$' Makefile.inc || fail 'configure did not select glibc'
    env -i PATH="$PATH" LC_ALL=C TZ=UTC SOURCE_DATE_EPOCH="$SOURCE_DATE_EPOCH" \
        make -j "$jobs" btrfs.static
)
verify_inputs
# Detect changed dependency files during preparation before publishing provenance.
jq -r '.[] | [.path,.sha256] | @tsv' "$work_dir/libraries.json" |
    while IFS="$(printf '\t')" read -r library_path expected; do
        verify_digest "$library_path" "$expected"
    done
candidate="$source_dir/btrfs.static"
[ -x "$candidate" ] || fail 'upstream did not produce executable btrfs.static'
# Inspect before executing; an ordinary dynamic btrfs is never accepted.
readelf -hW "$candidate" > "$work_dir/elf-header"
readelf -lW "$candidate" > "$work_dir/elf-programs"
readelf -dW "$candidate" > "$work_dir/elf-dynamic"
grep -Eq 'Class:[[:space:]]+ELF64' "$work_dir/elf-header" || fail 'receiver is not ELF64'
grep -Eq 'Machine:[[:space:]]+Advanced Micro Devices X86-64' "$work_dir/elf-header" || fail 'receiver is not x86_64'
if grep -Eq '^[[:space:]]*INTERP[[:space:]]' "$work_dir/elf-programs" ||
   grep -F '(NEEDED)' "$work_dir/elf-dynamic" >/dev/null; then
    fail 'receiver has PT_INTERP or DT_NEEDED; detached chroot requires static ELF'
fi
env -i LC_ALL=C "$candidate" --version > "$work_dir/version"
version=$(awk 'NR == 1 {print}' "$work_dir/version")
[ "$version" = "btrfs-progs $UPSTREAM_TAG" ] || fail "unexpected receiver version: $version"
for feature in +STATIC +LZO +ZSTD +FSVERITY CRYPTO=builtin; do
    grep -Eq "(^| )$(printf '%s' "$feature" | sed 's/+/[+]/g')( |$)" "$work_dir/version" ||
        fail "receiver lacks required upstream feature: $feature"
done
cp -- "$candidate" "$work_dir/btrfs"
chmod 0755 "$work_dir/btrfs"
binary_sha256=$(digest "$work_dir/btrfs")
configure_options='--enable-programs --enable-static --disable-shared --disable-documentation --disable-python --disable-convert --disable-libudev --disable-backtrace --disable-zoned --disable-experimental --with-crypto=builtin --enable-lzo --enable-zstd'
jq -n --arg format opdbus.btrfs-receiver.v1 \
    --arg upstream_url "$UPSTREAM_URL/$SOURCE_ARCHIVE" --arg upstream_tag "$UPSTREAM_TAG" \
    --arg source_sha256 "$SOURCE_ARCHIVE_SHA256" --arg pin_sha256 "$pin_sha256" \
    --arg recipe_sha256 "$recipe_sha256" --arg provenance_sha256 "$provenance_sha256" \
    --arg path "$output_dir/btrfs" --arg sha256 "$binary_sha256" --arg version "$version" \
    --arg target "$NATIVE_TARGET" --arg compiler_target "$compiler_target" \
    --arg compiler "$(gcc --version)" --arg linker "$(ld --version)" --arg libc "$libc_version" \
    --arg pkg_versions "$(pkg-config --modversion blkid uuid zlib libzstd)" \
    --arg options "$configure_options" --argjson epoch "$SOURCE_DATE_EPOCH" \
    --argjson bytes "$(wc -c < "$work_dir/btrfs")" --slurpfile libraries "$work_dir/libraries.json" \
    '{format:$format,upstream:{url:$upstream_url,tag:$upstream_tag,source_archive_sha256:$source_sha256},inputs:{source_pin_sha256:$pin_sha256,recipe_sha256:$recipe_sha256,provenance_sha256:$provenance_sha256},artifact:{name:"btrfs",path:$path,sha256:$sha256,bytes:$bytes,version:$version,target:$target,elf:{class:64,machine:62,pt_interp:false,dt_needed:false}},build:{make_target:"btrfs.static",configure_options:$options,source_date_epoch:$epoch},toolchain:{compiler:$compiler,compiler_target:$compiler_target,linker:$linker,libc:$libc,pkg_config_versions_blkid_uuid_zlib_zstd:$pkg_versions,static_libraries:$libraries[0]},authorization:"Preparation only; this manifest is not receiver.json. Pin installed device/inode/uid/gid/full st_mode in a separate authorized bootstrap step."}' \
    > "$work_dir/manifest.json"
# Publish manifest last. Consumers must validate its binary hash, not existence alone.
cp -- "$work_dir/btrfs" "$output_dir/btrfs.new"
chmod 0755 "$output_dir/btrfs.new"
cp -- "$work_dir/manifest.json" "$output_dir/manifest.json.new"
mv -f -- "$output_dir/btrfs.new" "$output_dir/btrfs"
mv -f -- "$output_dir/manifest.json.new" "$output_dir/manifest.json"
sha256sum "$output_dir/btrfs" "$output_dir/manifest.json"
