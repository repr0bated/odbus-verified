#!/bin/sh
# Consume the release archive while preparing an offline Alpine mail rootfs.
# The expected archive digest comes from the enclosing golden snapshot
# MANIFEST; the embedded MANIFEST then authenticates every installed member.
set -eu

die() {
    printf 'mail-3tched preparation: %s\n' "$*" >&2
    exit 1
}

[ "$#" -eq 3 ] || die "usage: $0 ROOTFS ARTIFACT EXPECTED_SHA256"
[ "${OP_MAIL_OFFLINE_ROOTFS:-}" = 1 ] ||
    die "set OP_MAIL_OFFLINE_ROOTFS=1 for prepared-image application"

ROOTFS=$(readlink -f "$1")
ARTIFACT=$(readlink -f "$2")
EXPECTED_SHA256=$3
[ "$ROOTFS" != / ] || die "refusing the host rootfs"
[ -f "$ROOTFS/etc/alpine-release" ] || die "target is not an Alpine rootfs"
[ -f "$ARTIFACT" ] || die "artifact is absent: $ARTIFACT"
case "$EXPECTED_SHA256" in
    *[!0-9a-f]*|'') die "expected SHA-256 must be lowercase hexadecimal" ;;
esac
[ "${#EXPECTED_SHA256}" -eq 64 ] || die "expected SHA-256 must contain 64 digits"
[ "$(sha256sum "$ARTIFACT" | cut -d' ' -f1)" = "$EXPECTED_SHA256" ] ||
    die "artifact SHA-256 differs from the golden manifest"

tar -tf "$ARTIFACT" | awk '
    /^\// || /(^|\/)\.\.($|\/)/ { bad = 1 }
    seen[$0]++ { bad = 1 }
    END { exit bad ? 1 : 0 }
' || die "artifact contains an unsafe or duplicate path"
tar -tvf "$ARTIFACT" | awk 'substr($1,1,1) !~ /[-d]/ { bad=1 } END { exit bad ? 1 : 0 }' ||
    die "artifact must contain only regular files and directories"

stage=$(mktemp -d)
case "$stage" in /tmp/*) ;; *) die "temporary extraction escaped /tmp" ;; esac
cleanup() {
    find "$stage" -depth -delete 2>/dev/null || true
}
trap cleanup EXIT HUP INT TERM
tar -xf "$ARTIFACT" -C "$stage"

[ "$(sed -n 's/^artifact: //p' "$stage/MANIFEST")" = mail-3tched ] ||
    die "embedded manifest names another artifact"
[ "$(sed -n 's/^rust-target: //p' "$stage/MANIFEST")" = x86_64-unknown-linux-gnu ] ||
    die "embedded manifest has the wrong Rust target"
[ "$(sed -n 's/^elf-runtime: //p' "$stage/MANIFEST")" = private-glibc-loader-bundle ] ||
    die "embedded manifest has the wrong runtime contract"

manifest_files=$(mktemp)
actual_files=$(mktemp)
case "$manifest_files:$actual_files" in /tmp/*:/tmp/*) ;; *) die "temporary manifest escaped /tmp" ;; esac
trap 'rm -f -- "$manifest_files" "$actual_files"; cleanup' EXIT HUP INT TERM
awk '
    /^\[sha256\]$/ { hashes = 1; next }
    hashes && NF { print $2 }
' "$stage/MANIFEST" > "$manifest_files"

(CDPATH= cd -- "$stage" && find . -type f -print | sed 's|^\./||' |
    grep -vx MANIFEST | sort) > "$actual_files"
sort "$manifest_files" -o "$manifest_files"
cmp -s "$manifest_files" "$actual_files" ||
    die "embedded manifest file set differs from the archive"
awk '
    /^\[sha256\]$/ { hashes = 1; next }
    !hashes || !NF { next }
    length($1) != 64 || $1 ~ /[^0-9a-f]/ || NF != 2 || $2 ~ /^\// || $2 ~ /(^|\/)\.\.($|\/)/ { exit 1 }
    { print $1 "  " root "/" $2 }
' root="$stage" "$stage/MANIFEST" | sha256sum -c - >/dev/null ||
    die "embedded member digest verification failed"

OP_MAIL_OFFLINE_ROOTFS=1 "$stage/apply-rootfs.sh" "$ROOTFS" "$stage/rootfs"
