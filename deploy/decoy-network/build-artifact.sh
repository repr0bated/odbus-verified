#!/bin/sh
# Package the decoy-only network owner, opblob catalog materializer and the
# already sealed/persisted canonical catalog. This script never seals schemas,
# generates credentials, or changes a running service.
set -eu

die() {
    printf 'decoy-network artifact: %s\n' "$*" >&2
    exit 1
}

SCRIPT_DIR=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
PROJECT_ROOT=$(CDPATH='' cd -- "$SCRIPT_DIR/../.." && pwd)
RELEASE_DIR=${OP_DECOY_RELEASE_DIR:-"$PROJECT_ROOT/target/x86_64-unknown-linux-gnu/release"}
CATALOG_SOURCE=${OP_DECOY_PERSISTED_CATALOG_SOURCE:-/var/lib/opdbus/plugin-blobs}
OUTPUT_DIR=${OP_DECOY_ARTIFACT_DIR:-"$PROJECT_ROOT/target/container-artifacts"}
ARTIFACT_NAME=decoy-network-x86_64-linux-gnu-bundle.tar
: "${OP_DECOY_EXECUTION_RECEIPT:?provide the successful same-source GNU execution receipt}"
: "${OP_DECOY_CATALOG_SEAL_RECEIPT:?provide the matching explicit catalog seal receipt}"
for receipt in "$OP_DECOY_EXECUTION_RECEIPT" "$OP_DECOY_CATALOG_SEAL_RECEIPT"; do
    [ -f "$receipt" ] && [ ! -L "$receipt" ] || die "release receipt is not a regular file"
done
jq -e '.exit==0 and (.source_commit|test("^[0-9a-f]{40}$")) and (.source_digest|test("^[0-9a-f]{64}$")) and .artifact_dir=="/opt/op-build/target/x86_64-unknown-linux-gnu/release" and (.artifacts["op-decoy-network"]|test("^[0-9a-f]{64}$")) and (.artifacts.opblob|test("^[0-9a-f]{64}$"))' "$OP_DECOY_EXECUTION_RECEIPT" >/dev/null ||
    die "execution receipt does not pin the required GNU artifacts"
jq -e --slurpfile execution "$OP_DECOY_EXECUTION_RECEIPT" \
    '.exit==0 and .source_commit==$execution[0].source_commit and .source_digest==$execution[0].source_digest and (.catalog_manifest_sha256|test("^[0-9a-f]{64}$"))' "$OP_DECOY_CATALOG_SEAL_RECEIPT" >/dev/null ||
    die "catalog seal is not from the successful built source"
helper_digest() {
    (CDPATH='' cd -- "$SCRIPT_DIR" &&
      sha256sum build-artifact.sh run op-decoy-network.wrapper opblob.wrapper apply-rootfs.sh environment.example) |
        sha256sum | cut -d' ' -f1
}
helpers_sha=$(helper_digest)

for command_name in readelf ldd tar jq sha256sum; do
    command -v "$command_name" >/dev/null 2>&1 || die "$command_name is unavailable"
done
for binary_name in op-decoy-network opblob; do
    binary="$RELEASE_DIR/$binary_name"
    [ -x "$binary" ] || die "missing release binary $binary"
    [ ! -L "$binary" ] &&
        [ "$(sha256sum "$binary" | cut -d' ' -f1)" = "$(jq -er --arg name "$binary_name" '.artifacts[$name]' "$OP_DECOY_EXECUTION_RECEIPT")" ] ||
        die "$binary_name differs from the execution receipt"
    readelf -h "$binary" | grep -q 'Machine:.*X86-64' ||
        die "$binary_name is not x86_64 ELF"
done
[ -f "$CATALOG_SOURCE/.manifest.json" ] ||
    die "persisted sealer-owned catalog is absent: $CATALOG_SOURCE/.manifest.json"
[ "$(sha256sum "$CATALOG_SOURCE/.manifest.json" | cut -d' ' -f1)" = "$(jq -er .catalog_manifest_sha256 "$OP_DECOY_CATALOG_SEAL_RECEIPT")" ] ||
    die "catalog manifest differs from the matching seal receipt"
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
done


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
    [ "$(sha256sum "$bundle/$binary_name.bin" | cut -d' ' -f1)" = "$(jq -er --arg name "$binary_name" '.artifacts[$name]' "$OP_DECOY_EXECUTION_RECEIPT")" ] ||
        die "$binary_name changed during packaging"

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

# Opblob alone validates/authenticates the complete blob closure and stages its
# exact committed manifest into this OFFLINE prepared application root.
"$rootfs/usr/local/bin/opblob" persist "$CATALOG_SOURCE" "$rootfs/var/lib/opdbus/plugin-blobs"
[ "$(sha256sum "$rootfs/var/lib/opdbus/plugin-blobs/.manifest.json" | cut -d' ' -f1)" = "$(jq -er .catalog_manifest_sha256 "$OP_DECOY_CATALOG_SEAL_RECEIPT")" ] ||
    die "catalog changed while packaging"
jq '{version:1,source_commit,source_digest,artifact_dir,artifacts:{"op-decoy-network":.artifacts["op-decoy-network"],opblob:.artifacts.opblob}}' \
    "$OP_DECOY_EXECUTION_RECEIPT" >"$stage/PROVENANCE.json"

install -Dm755 "$SCRIPT_DIR/apply-rootfs.sh" "$stage/apply-rootfs.sh"

commit=$(jq -er .source_commit "$OP_DECOY_EXECUTION_RECEIPT")
{
    printf 'artifact: decoy-network\n'
    printf 'source-commit: %s\n' "$commit"
    printf 'source-digest: %s\n' "$(jq -er .source_digest "$OP_DECOY_EXECUTION_RECEIPT")"
    printf 'bundle-helper-sha256: %s\n' "$(sha256sum "$SCRIPT_DIR/build-artifact.sh" | cut -d' ' -f1)"
    printf 'bundle-helpers-sha256: %s\n' "$helpers_sha"
    printf 'catalog-manifest-sha256: %s\n' "$(jq -er .catalog_manifest_sha256 "$OP_DECOY_CATALOG_SEAL_RECEIPT")"
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
    (CDPATH='' cd -- "$stage" && find PROVENANCE.json apply-rootfs.sh rootfs -type f -print | sort |
        while IFS= read -r file; do
            printf '%s  %s\n' "$(sha256sum "$file" | cut -d' ' -f1)" "$file"
        done)
} > "$stage/MANIFEST"
[ "$(helper_digest)" = "$helpers_sha" ] || die "release helper sources changed during packaging"

artifact_stage="$OUTPUT_DIR/.$ARTIFACT_NAME.tmp.$$"
tar --sort=name --mtime=@0 --owner=0 --group=0 --numeric-owner \
    -cf "$artifact_stage" -C "$stage" MANIFEST PROVENANCE.json apply-rootfs.sh rootfs
artifact="$OUTPUT_DIR/$ARTIFACT_NAME"
mv -f -- "$artifact_stage" "$artifact"
printf '%s  %s\n' "$(sha256sum "$artifact" | cut -d' ' -f1)" "$artifact"
