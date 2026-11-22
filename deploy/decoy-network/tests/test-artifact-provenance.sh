#!/bin/bash
# Bundle/provenance checks with controlled ELF/loader/opblob boundaries; no Cargo.
set -euo pipefail
here=$(cd -- "$(dirname -- "$0")/.." && pwd)
h=$(mktemp -d); trap 'rm -rf "$h"' EXIT
mkdir "$h/bin" "$h/release" "$h/catalog" "$h/out"
cat >"$h/ld-linux-x86-64.so.2" <<'LOADER'
#!/bin/sh
# Controlled loader boundary; the production bundle carries the real GNU ELF loader.
while [ "$#" -gt 0 ]; do
    case "$1" in
        --inhibit-cache) shift ;;
        --library-path) shift 2 ;;
        *) exec "$@" ;;
    esac
done
exit 1
LOADER
chmod +x "$h/ld-linux-x86-64.so.2"
cat >"$h/bin/readelf" <<'ELF'
#!/bin/sh
case "$1" in
 -h) echo 'Machine: X86-64' ;;
 -l) printf 'Requesting program interpreter: %s]\n' "$GOLDEN_TEST_INTERPRETER" ;;
 -d) : ;;
 *) exit 1 ;;
esac
ELF
printf '#!/bin/sh\nexit 0\n' >"$h/bin/ldd"
printf '#!/bin/sh\nexit 0\n' >"$h/bin/ldconfig"
printf '#!/bin/sh\necho forbidden-git-head-inference >&2\nexit 99\n' >"$h/bin/git"
printf '#!/bin/sh\nexit 0\n' >"$h/release/op-decoy-network"
cat >"$h/release/opblob" <<'BLOB'
#!/bin/sh
set -eu
[ "$1" = persist ]
mkdir -p "$3"
for f in "$2"/*.blob; do cp "$f" "$3/"; done
cp "$2/.manifest.json" "$3/.manifest.json"
BLOB
chmod +x "$h/bin/"* "$h/release/"*
jq -n '{catalog_hash:("4"*64),plugins:{wireguard:("1"*64),wg_opdbus:("2"*64),wgcf:("3"*64)}}' >"$h/catalog/.manifest.json"
for pair in wireguard:1 wg_opdbus:2 wgcf:3; do
 hash=$(printf '%016d' 0 | tr 0 "${pair#*:}")
 printf 'fixture blob\n' >"$h/catalog/${pair%:*}.$hash.blob"
done
jq -n --arg network "$(sha256sum "$h/release/op-decoy-network" | cut -d' ' -f1)" --arg blob "$(sha256sum "$h/release/opblob" | cut -d' ' -f1)" \
 '{version:1,exit:0,source_commit:("a"*40),source_digest:("b"*64),artifact_dir:"/opt/op-build/target/x86_64-unknown-linux-gnu/release",artifacts:{"op-decoy-network":$network,opblob:$blob}}' >"$h/execution.json"
jq -n --arg manifest "$(sha256sum "$h/catalog/.manifest.json" | cut -d' ' -f1)" \
 '{exit:0,source_commit:("a"*40),source_digest:("b"*64),catalog_manifest_sha256:$manifest}' >"$h/seal.json"
export PATH="$h/bin:/usr/bin:/bin" GOLDEN_TEST_INTERPRETER=$h/ld-linux-x86-64.so.2
export OP_DECOY_RELEASE_DIR=$h/release OP_DECOY_PERSISTED_CATALOG_SOURCE=$h/catalog OP_DECOY_ARTIFACT_DIR=$h/out OP_DECOY_EXECUTION_RECEIPT=$h/execution.json OP_DECOY_CATALOG_SEAL_RECEIPT=$h/seal.json
sh "$here/build-artifact.sh" >"$h/success.log"
tar -xf "$h/out/decoy-network-x86_64-linux-gnu-bundle.tar" -C "$h/out"
grep -qx "source-commit: $(printf 'a%.0s' {1..40})" "$h/out/MANIFEST"
grep -qx "source-digest: $(printf 'b%.0s' {1..64})" "$h/out/MANIFEST"
grep -qx "bundle-helper-sha256: $(sha256sum "$here/build-artifact.sh" | cut -d' ' -f1)" "$h/out/MANIFEST"
jq -e '.source_commit==("a"*40) and .source_digest==("b"*64) and (.artifacts|keys)==["op-decoy-network","opblob"]' "$h/out/PROVENANCE.json" >/dev/null
echo 'ok validated original SHA/source digest, exact GNU artifacts and helper digest; no Git inference'
printf 'tamper\n' >>"$h/release/op-decoy-network"
if sh "$here/build-artifact.sh" >"$h/rejected.log" 2>&1; then exit 1; fi
echo 'ok changed artifact rejected'
printf '#!/bin/sh\nexit 0\n' >"$h/release/op-decoy-network"
jq '.source_digest=("c"*64)' "$h/seal.json" >"$h/seal.new"; mv "$h/seal.new" "$h/seal.json"
if sh "$here/build-artifact.sh" >"$h/rejected.log" 2>&1; then exit 1; fi
echo 'ok seal from different source rejected'
unset OP_DECOY_EXECUTION_RECEIPT
if sh "$here/build-artifact.sh" >"$h/rejected.log" 2>&1; then exit 1; fi
echo 'ok missing execution provenance rejected without HEAD fallback'
echo 'native artifact provenance regressions passed'
