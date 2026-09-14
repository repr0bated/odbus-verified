#!/bin/sh
# Package the normal GNU release binary with its exact loader and recursive
# DT_NEEDED closure. Alpine does not provide glibc, so the wrapper invokes only
# this private bundle. This script packages an existing release; it never runs
# Cargo and never touches a container.
set -eu

die() {
    printf 'mail-3tched artifact: %s\n' "$*" >&2
    exit 1
}

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
PROJECT_ROOT=$(CDPATH= cd -- "$SCRIPT_DIR/../.." && pwd)
RELEASE_DIR=${OP_MAIL_RELEASE_DIR:-"$PROJECT_ROOT/target/release"}
OUTPUT_DIR=${OP_MAIL_ARTIFACT_DIR:-"$PROJECT_ROOT/target/container-artifacts"}
BINARY="$RELEASE_DIR/op-mail-admin"
ARTIFACT_NAME=mail-3tched-x86_64-linux-gnu-bundle.tar

command -v readelf >/dev/null 2>&1 || die "readelf is unavailable"
command -v ldd >/dev/null 2>&1 || die "ldd is unavailable"
command -v tar >/dev/null 2>&1 || die "tar is unavailable"
[ -x "$BINARY" ] || die "missing release binary $BINARY"
readelf -h "$BINARY" | grep -q 'Machine:.*X86-64' ||
    die "op-mail-admin is not x86_64 ELF"
interpreter=$(readelf -l "$BINARY" |
    sed -n 's/.*Requesting program interpreter: \(.*\)]/\1/p')
[ -n "$interpreter" ] || die "op-mail-admin has no GNU ELF interpreter"
[ -f "$interpreter" ] || die "ELF interpreter is absent: $interpreter"
[ "$(basename -- "$interpreter")" = ld-linux-x86-64.so.2 ] ||
    die "unexpected GNU ELF interpreter: $interpreter"

mkdir -p "$PROJECT_ROOT/target" "$OUTPUT_DIR"
stage=$(mktemp -d "$PROJECT_ROOT/target/.mail-3tched-artifact.XXXXXX")
trap 'find "$stage" -depth -delete 2>/dev/null || true' EXIT HUP INT TERM
rootfs="$stage/rootfs"
bundle="$rootfs/usr/local/lib/op-mail-admin"
libs="$bundle/lib"
install -d -m 0755 "$libs"
install -m 0755 "$BINARY" "$bundle/op-mail-admin.bin"
install -m 0755 "$interpreter" "$bundle/ld-linux-x86-64.so.2"

# ldd asks the binary's own loader to resolve the complete recursive closure.
# Preserve each requested SONAME as the destination filename, rather than the
# host symlink target's basename.
LC_ALL=C ldd "$BINARY" > "$stage/ldd.txt"
grep -q 'not found' "$stage/ldd.txt" && die "release binary has unresolved libraries"
awk '$1 !~ /^\// && $2 == "=>" && $3 ~ /^\// { print $1 " " $3 }' "$stage/ldd.txt" |
while IFS=' ' read -r soname source; do
    case "$soname" in
        ''|*[!A-Za-z0-9._+-]*) die "unsafe DT_NEEDED name: $soname" ;;
    esac
    [ -f "$source" ] || die "resolved library is absent: $source"
    destination="$libs/$soname"
    if [ -e "$destination" ]; then
        cmp -s "$source" "$destination" ||
            die "two different libraries resolve as $soname"
    else
        install -m 0755 "$source" "$destination"
    fi
done

# glibc loads the files NSS backend at runtime rather than through DT_NEEDED.
# Group/user lookups must keep working inside Alpine without its musl NSS.
nss_files=$(ldconfig -p 2>/dev/null |
    awk '$1 == "libnss_files.so.2" && $NF ~ /^\// { print $NF; exit }')
if [ -n "$nss_files" ]; then
    install -m 0755 "$nss_files" "$libs/libnss_files.so.2"
fi

# Prove every direct dependency of the executable and every bundled library is
# present under the exact name requested by its dynamic section.
find "$bundle" -type f -print | while IFS= read -r elf; do
    readelf -h "$elf" >/dev/null 2>&1 || continue
    readelf -d "$elf" 2>/dev/null |
        sed -n 's/.*Shared library: \[\(.*\)\]/\1/p' |
    while IFS= read -r needed; do
        [ -f "$libs/$needed" ] || [ -f "$bundle/$needed" ] ||
            die "bundle is missing $needed required by $(basename -- "$elf")"
    done
done

install -Dm755 "$SCRIPT_DIR/op-mail-admin.wrapper" \
    "$rootfs/usr/local/bin/op-mail-admin"
install -Dm755 "$SCRIPT_DIR/op-mail-admin.initd" \
    "$rootfs/etc/init.d/op-mail-admin"
install -Dm644 "$SCRIPT_DIR/postfix-opdbus.cf" \
    "$rootfs/etc/postfix/opdbus-main.cf"
install -Dm644 "$SCRIPT_DIR/dovecot-opdbus.conf" \
    "$rootfs/etc/dovecot/opdbus.conf"
install -Dm644 "$SCRIPT_DIR/opdbus-domains" \
    "$rootfs/etc/postfix/opdbus-domains"
install -Dm755 "$SCRIPT_DIR/apply-rootfs.sh" "$stage/apply-rootfs.sh"

commit=$(git -C "$PROJECT_ROOT" rev-parse HEAD 2>/dev/null || printf unknown)
{
    printf 'artifact: mail-3tched\n'
    printf 'source-commit: %s\n' "$commit"
    printf 'rust-target: x86_64-unknown-linux-gnu\n'
    printf 'container-libc: musl\n'
    printf 'elf-runtime: private-glibc-loader-bundle\n'
    printf 'elf-interpreter: %s\n' "$interpreter"
    printf '\n[dt-needed]\n'
    readelf -d "$BINARY" |
        sed -n 's/.*Shared library: \[\(.*\)\]/\1/p' | sort
    printf '\n[sha256]\n'
    (CDPATH= cd -- "$stage" && find apply-rootfs.sh rootfs -type f -print | sort | while IFS= read -r file; do
        printf '%s  %s\n' "$(sha256sum "$file" | cut -d' ' -f1)" "$file"
    done)
} > "$stage/MANIFEST"

artifact_stage="$OUTPUT_DIR/.$ARTIFACT_NAME.tmp.$$"
tar --sort=name --mtime=@0 --owner=0 --group=0 --numeric-owner \
    -cf "$artifact_stage" -C "$stage" MANIFEST apply-rootfs.sh rootfs
artifact="$OUTPUT_DIR/$ARTIFACT_NAME"
mv -f -- "$artifact_stage" "$artifact"
printf '%s  %s\n' "$(sha256sum "$artifact" | cut -d' ' -f1)" "$artifact"
