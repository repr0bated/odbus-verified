#!/bin/sh
# Package the decoy-only network owner, opblob catalog materializer and the
# already sealed/persisted canonical catalog. This script never seals schemas,
# generates credentials, or changes a running service.
set -eu

die() {
    printf 'decoy-network artifact: %s\n' "$*" >&2
    exit 1
}

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
PROJECT_ROOT=$(CDPATH= cd -- "$SCRIPT_DIR/../.." && pwd)
RELEASE_DIR=${OP_DECOY_RELEASE_DIR:-"$PROJECT_ROOT/target/release"}
CATALOG_SOURCE=${OP_DECOY_PERSISTED_CATALOG_SOURCE:-/var/lib/opdbus/plugin-blobs}
OUTPUT_DIR=${OP_DECOY_ARTIFACT_DIR:-"$PROJECT_ROOT/target/container-artifacts"}
ARTIFACT_NAME=decoy-network-x86_64-linux-gnu-bundle.tar

for command_name in readelf ldd tar jq sha256sum; do
    command -v "$command_name" >/dev/null 2>&1 || die "$command_name is unavailable"
done
for binary_name in op-decoy-network opblob; do
    binary="$RELEASE_DIR/$binary_name"
    [ -x "$binary" ] || die "missing release binary $binary"
    readelf -h "$binary" | grep -q 'Machine:.*X86-64' ||
        die "$binary_name is not x86_64 ELF"
done
[ -f "$CATALOG_SOURCE/.manifest.json" ] ||
    die "persisted sealer-owned catalog is absent: $CATALOG_SOURCE/.manifest.json"
jq -e '.plugins | (type == "object") and (to_entries | all(.[];
    (.key | test("^[A-Za-z0-9_.-]+$")) and
    (.value | type == "string" and test("^[0-9a-f]{64}$"))))' \
    "$CATALOG_SOURCE/.manifest.json" >/dev/null ||
    die "catalog manifest contains an invalid plugin id or schema hash"

catalog_hash=$(jq -er '.catalog_hash | select(type == "string" and test("^[0-9a-f]{64}$"))' \
    "$CATALOG_SOURCE/.manifest.json") || die "catalog manifest has no valid catalog hash"
for plugin in wireguard wg_opdbus wgcf; do
    schema_hash=$(jq -er --arg plugin "$plugin" \
        '.plugins[$plugin] | select(type == "string" and test("^[0-9a-f]{64}$"))' \
        "$CATALOG_SOURCE/.manifest.json") || die "catalog does not contain $plugin"
    blob="$CATALOG_SOURCE/$plugin.$(printf '%.16s' "$schema_hash").blob"
    [ -f "$blob" ] || die "catalog is missing $(basename -- "$blob")"
    # `inspect` parses and authenticates the sealed blob. The decoy later pins
    # the same bytes to this manifest before every method effect.
    inspection=$("$RELEASE_DIR/opblob" inspect "$blob") ||
        die "$plugin blob is not a valid sealed plugin"
    printf '%s\n' "$inspection" | grep -Eq "^plugin[[:space:]]+$plugin v[0-9]+([.][0-9]+)*$" ||
        die "$plugin blob manifest names another plugin"
    printf '%s\n' "$inspection" | grep -Fqx "schema     sha256 $schema_hash" ||
        die "$plugin blob differs from the catalog manifest"
done

catalog_files=$(mktemp)
actual_catalog_files=$(mktemp)
trap 'rm -f -- "$catalog_files" "$actual_catalog_files"' HUP INT TERM
jq -r '.plugins | to_entries[] | "\(.key).\(.value[0:16]).blob"' \
    "$CATALOG_SOURCE/.manifest.json" | sort > "$catalog_files"
(CDPATH= cd -- "$CATALOG_SOURCE" && find . -maxdepth 1 -type f -name '*.blob' -print |
    sed 's|^\./||' | sort) > "$actual_catalog_files"
cmp -s "$catalog_files" "$actual_catalog_files" ||
    die "persisted blob set differs from its sealer-owned manifest"
rm -f -- "$catalog_files" "$actual_catalog_files"
trap - HUP INT TERM

mkdir -p "$PROJECT_ROOT/target" "$OUTPUT_DIR"
stage=$(mktemp -d "$PROJECT_ROOT/target/.decoy-network-artifact.XXXXXX")
cleanup() {
    find "$stage" -depth -delete 2>/dev/null || true
}
trap cleanup EXIT HUP INT TERM
rootfs="$stage/rootfs"
bundle="$rootfs/usr/local/lib/op-decoy-network"
libs="$bundle/lib"
install -d -m 0755 "$libs"

interpreter=
for binary_name in op-decoy-network opblob; do
    binary="$RELEASE_DIR/$binary_name"
    binary_interpreter=$(readelf -l "$binary" |
        sed -n 's/.*Requesting program interpreter: \(.*\)]/\1/p')
    [ -n "$binary_interpreter" ] || die "$binary_name has no GNU ELF interpreter"
    [ -f "$binary_interpreter" ] || die "ELF interpreter is absent: $binary_interpreter"
    [ "$(basename -- "$binary_interpreter")" = ld-linux-x86-64.so.2 ] ||
        die "unexpected ELF interpreter for $binary_name: $binary_interpreter"
    if [ -n "$interpreter" ] && [ "$interpreter" != "$binary_interpreter" ]; then
        die "release binaries use different GNU ELF interpreters"
    fi
    interpreter=$binary_interpreter
    install -m 0755 "$binary" "$bundle/$binary_name.bin"

    LC_ALL=C ldd "$binary" > "$stage/$binary_name.ldd"
    grep -q 'not found' "$stage/$binary_name.ldd" &&
        die "$binary_name has unresolved libraries"
    awk '$1 !~ /^\// && $2 == "=>" && $3 ~ /^\// { print $1 " " $3 }' \
        "$stage/$binary_name.ldd" |
    while IFS=' ' read -r soname source; do
        case "$soname" in ''|*[!A-Za-z0-9._+-]*) die "unsafe DT_NEEDED name: $soname" ;; esac
        [ -f "$source" ] || die "resolved library is absent: $source"
        destination="$libs/$soname"
        if [ -e "$destination" ]; then
            cmp -s "$source" "$destination" || die "different libraries resolve as $soname"
        else
            install -m 0755 "$source" "$destination"
        fi
    done
done
install -m 0755 "$interpreter" "$bundle/ld-linux-x86-64.so.2"

nss_files=$(ldconfig -p 2>/dev/null |
    awk '$1 == "libnss_files.so.2" && $NF ~ /^\// { print $NF; exit }')
if [ -n "$nss_files" ]; then
    install -m 0755 "$nss_files" "$libs/libnss_files.so.2"
fi
find "$bundle" -type f -print | while IFS= read -r elf; do
    readelf -h "$elf" >/dev/null 2>&1 || continue
    readelf -d "$elf" 2>/dev/null |
        sed -n 's/.*Shared library: \[\(.*\)\]/\1/p' |
    while IFS= read -r needed; do
        [ -f "$libs/$needed" ] || [ -f "$bundle/$needed" ] ||
            die "bundle is missing $needed required by $(basename -- "$elf")"
    done
done

install -Dm755 "$SCRIPT_DIR/op-decoy-network.wrapper" \
    "$rootfs/usr/local/bin/op-decoy-network"
install -Dm755 "$SCRIPT_DIR/opblob.wrapper" "$rootfs/usr/local/bin/opblob"
install -Dm755 "$SCRIPT_DIR/run" "$rootfs/etc/runit/sv/op-decoy-network/run"
install -Dm644 "$SCRIPT_DIR/environment.example" \
    "$rootfs/etc/op-dbus/decoy-network.env.example"
install -d -m 0755 "$rootfs/var/lib/opdbus/plugin-blobs"

# Copy the complete sealer-owned catalog and commit its manifest last. Keeping
# the manifest unchanged preserves its catalog hash and avoids a second schema
# or catalog writer in this packaging layer.
for blob in "$CATALOG_SOURCE"/*.blob; do
    [ -f "$blob" ] || die "persisted catalog contains no blob files"
    install -m 0644 "$blob" "$rootfs/var/lib/opdbus/plugin-blobs/$(basename -- "$blob")"
done
install -m 0644 "$CATALOG_SOURCE/.manifest.json" \
    "$rootfs/var/lib/opdbus/plugin-blobs/.manifest.json"

install -Dm755 "$SCRIPT_DIR/apply-rootfs.sh" "$stage/apply-rootfs.sh"

commit=$(git -C "$PROJECT_ROOT" rev-parse HEAD 2>/dev/null || printf unknown)
{
    printf 'artifact: decoy-network\n'
    printf 'source-commit: %s\n' "$commit"
    printf 'rust-target: x86_64-unknown-linux-gnu\n'
    printf 'elf-runtime: private-glibc-loader-bundle\n'
    printf 'catalog-hash: %s\n' "$catalog_hash"
    printf 'catalog-source: opblob-persist\n'
    printf '\n[required-plugins]\n'
    for plugin in wireguard wg_opdbus wgcf; do
        jq -r --arg plugin "$plugin" '"\($plugin) \(.plugins[$plugin])"' \
            "$CATALOG_SOURCE/.manifest.json"
    done
    printf '\n[sha256]\n'
    (CDPATH= cd -- "$stage" && find apply-rootfs.sh rootfs -type f -print | sort |
        while IFS= read -r file; do
            printf '%s  %s\n' "$(sha256sum "$file" | cut -d' ' -f1)" "$file"
        done)
} > "$stage/MANIFEST"

artifact_stage="$OUTPUT_DIR/.$ARTIFACT_NAME.tmp.$$"
tar --sort=name --mtime=@0 --owner=0 --group=0 --numeric-owner \
    -cf "$artifact_stage" -C "$stage" MANIFEST apply-rootfs.sh rootfs
artifact="$OUTPUT_DIR/$ARTIFACT_NAME"
mv -f -- "$artifact_stage" "$artifact"
printf '%s  %s\n' "$(sha256sum "$artifact" | cut -d' ' -f1)" "$artifact"
