#!/bin/sh
# Verify and consume the decoy archive while preparing an offline filesystem
# snapshot. TLS inputs remain external to the artifact and are copied only into
# the target snapshot.
set -eu

die() {
    printf 'decoy-network preparation: %s\n' "$*" >&2
    exit 1
}

[ "$#" -eq 8 ] ||
    die "usage: $0 ROOTFS ARTIFACT EXPECTED_SHA256 ENV_FILE CA_FILE CERT_FILE KEY_FILE BRIDGE_CERT_FILE"
[ "${OP_DECOY_OFFLINE_ROOTFS:-}" = 1 ] ||
    die "set OP_DECOY_OFFLINE_ROOTFS=1 for prepared-image application"

ROOTFS=$(readlink -f "$1")
ARTIFACT=$(readlink -f "$2")
EXPECTED_SHA256=$3
ENV_FILE=$4
CA_FILE=$5
CERT_FILE=$6
KEY_FILE=$7
BRIDGE_CERT_FILE=$8
[ "$ROOTFS" != / ] || die "refusing the host rootfs"
[ -f "$ROOTFS/etc/os-release" ] || die "target is not a prepared Linux rootfs"
[ -f "$ARTIFACT" ] || die "artifact is absent: $ARTIFACT"
command -v jq >/dev/null 2>&1 || die "jq is unavailable"
case "$EXPECTED_SHA256" in *[!0-9a-f]*|'') die "expected SHA-256 must be lowercase hexadecimal" ;; esac
[ "${#EXPECTED_SHA256}" -eq 64 ] || die "expected SHA-256 must contain 64 digits"
[ "$(sha256sum "$ARTIFACT" | cut -d' ' -f1)" = "$EXPECTED_SHA256" ] ||
    die "artifact SHA-256 differs from the release manifest"

tar -tf "$ARTIFACT" | awk '
    /^\// || /(^|\/)\.\.($|\/)/ { bad = 1 }
    seen[$0]++ { bad = 1 }
    END { exit bad ? 1 : 0 }
' || die "artifact contains an unsafe or duplicate path"
tar -tvf "$ARTIFACT" | awk 'substr($1,1,1) !~ /[-d]/ { bad=1 } END { exit bad ? 1 : 0 }' ||
    die "artifact must contain only regular files and directories"

stage=$(mktemp -d)
expected_files=$(mktemp)
actual_files=$(mktemp)
case "$stage:$expected_files:$actual_files" in /tmp/*:/tmp/*:/tmp/*) ;; *) die "temporary path escaped /tmp" ;; esac
cleanup() {
    rm -f -- "$expected_files" "$actual_files"
    find "$stage" -depth -delete 2>/dev/null || true
}
trap cleanup EXIT HUP INT TERM
tar -xf "$ARTIFACT" -C "$stage"

[ "$(sed -n 's/^artifact: //p' "$stage/MANIFEST")" = decoy-network ] ||
    die "embedded manifest names another artifact"
[ "$(sed -n 's/^rust-target: //p' "$stage/MANIFEST")" = x86_64-unknown-linux-gnu ] ||
    die "embedded manifest has the wrong Rust target"
[ "$(sed -n 's/^elf-runtime: //p' "$stage/MANIFEST")" = private-glibc-loader-bundle ] ||
    die "embedded manifest has the wrong runtime contract"
[ "$(sed -n 's/^catalog-source: //p' "$stage/MANIFEST")" = opblob-persist ] ||
    die "embedded catalog was not captured by opblob persist"
expected_catalog_hash=$(sed -n 's/^catalog-hash: //p' "$stage/MANIFEST")
case "$expected_catalog_hash" in *[!0-9a-f]*|'') die "embedded catalog hash is invalid" ;; esac
[ "${#expected_catalog_hash}" -eq 64 ] || die "embedded catalog hash must contain 64 digits"
actual_catalog_hash=$(jq -er '.catalog_hash' \
    "$stage/rootfs/var/lib/opdbus/plugin-blobs/.manifest.json") ||
    die "persisted catalog manifest has no catalog hash"
[ "$actual_catalog_hash" = "$expected_catalog_hash" ] ||
    die "persisted catalog hash differs from the artifact manifest"

awk '
    /^\[sha256\]$/ { hashes = 1; next }
    hashes && NF { print $2 }
' "$stage/MANIFEST" | sort > "$expected_files"
(CDPATH= cd -- "$stage" && find . -type f -print | sed 's|^\./||' |
    grep -vx MANIFEST | sort) > "$actual_files"
cmp -s "$expected_files" "$actual_files" ||
    die "embedded manifest file set differs from the archive"
awk '
    /^\[sha256\]$/ { hashes = 1; next }
    !hashes || !NF { next }
    length($1) != 64 || $1 ~ /[^0-9a-f]/ || NF != 2 || $2 ~ /^\// || $2 ~ /(^|\/)\.\.($|\/)/ { exit 1 }
    { print $1 "  " root "/" $2 }
' root="$stage" "$stage/MANIFEST" | sha256sum -c - >/dev/null ||
    die "embedded member digest verification failed"

for plugin in wireguard wg_opdbus wgcf; do
    expected=$(awk -v plugin="$plugin" '
        /^\[required-plugins\]$/ { plugins = 1; next }
        /^\[/ { plugins = 0 }
        plugins && $1 == plugin { print $2 }
    ' "$stage/MANIFEST")
    case "$expected" in *[!0-9a-f]*|'') die "manifest omits required $plugin schema hash" ;; esac
    [ "${#expected}" -eq 64 ] || die "manifest has invalid $plugin schema hash"
    actual=$(jq -er --arg plugin "$plugin" '.plugins[$plugin]' \
        "$stage/rootfs/var/lib/opdbus/plugin-blobs/.manifest.json") ||
        die "persisted catalog omits $plugin"
    [ "$actual" = "$expected" ] || die "$plugin schema hash differs across manifests"
done

OP_DECOY_OFFLINE_ROOTFS=1 "$stage/apply-rootfs.sh" \
    "$ROOTFS" "$stage/rootfs" "$ENV_FILE" "$CA_FILE" "$CERT_FILE" "$KEY_FILE" "$BRIDGE_CERT_FILE"
