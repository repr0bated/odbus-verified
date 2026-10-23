#!/bin/sh
set -eu

package_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
. "$package_dir/source.pin"
source_pin_sha256=$(sha256sum "$package_dir/source.pin" | awk '{print $1}')

output_dir=target/threetched-artifacts/tched-router
cache_dir=
source_dir=
source_archive=
verify_source_only=0

usage() {
    cat >&2 <<'EOF'
usage: build-threetched.sh --cache-dir DIR [--output-dir DIR]
       build-threetched.sh --cache-dir DIR --source-dir CLEAN-GIT-DIR [--output-dir DIR]
       build-threetched.sh --cache-dir DIR --source-archive PINNED-TAR [--output-dir DIR]

Builds the pinned, patched tched-router threetched dependency. It writes only the
cache, Cargo target directory and output directory; it never installs a binary.
With no explicit source option it prefers the checked-in source.tar.gz, then a
verified cache entry, and only then the exact upstream tag. --verify-source-only
stops after materializing and verifying the deterministic source tar.
EOF
    exit 2
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --cache-dir) [ "$#" -ge 2 ] || usage; cache_dir=$2; shift 2 ;;
        --output-dir) [ "$#" -ge 2 ] || usage; output_dir=$2; shift 2 ;;
        --source-dir) [ "$#" -ge 2 ] || usage; source_dir=$2; shift 2 ;;
        --source-archive) [ "$#" -ge 2 ] || usage; source_archive=$2; shift 2 ;;
        --verify-source-only) verify_source_only=1; shift ;;
        -h|--help) usage ;;
        *) usage ;;
    esac
done

[ -n "$cache_dir" ] || usage
[ -z "$source_dir" ] || [ -z "$source_archive" ] || {
    echo "choose only one of --source-dir and --source-archive" >&2
    exit 2
}
for command_name in awk cargo git grep gzip install jq patch rustc sha256sum strings tar; do
    command -v "$command_name" >/dev/null 2>&1 || {
        echo "missing threetched build command: $command_name" >&2
        exit 127
    }
done

verify_package_file() {
    path=$1
    expected=$2
    actual=$(sha256sum "$path" | awk '{print $1}')
    [ "$actual" = "$expected" ] || {
        echo "threetched patch-package digest mismatch: $path" >&2
        exit 2
    }
}
verify_build_inputs() {
    verify_package_file "$package_dir/source.pin" "$source_pin_sha256"
    verify_package_file "$package_dir/patches/0001-close-threetched-event-stream-on-lag.patch" "$PATCH_LAGGED_SSE_SHA256"
    verify_package_file "$package_dir/patches/0002-stateless-provider-generation.patch" "$PATCH_STATELESS_GENERATION_SHA256"
    verify_package_file "$package_dir/patches/0003-threetched-branding.patch" "$PATCH_BRANDING_SHA256"
    verify_package_file "$package_dir/patch-inventory" "$PATCH_INVENTORY_SHA256"
    verify_package_file "$package_dir/base-sha256sums" "$BASE_MANIFEST_SHA256"
}
verify_build_inputs
[ ! -f "$package_dir/source.tar.gz" ] ||
    verify_package_file "$package_dir/source.tar.gz" "$VENDORED_SOURCE_GZIP_SHA256"

case "$cache_dir" in /*) ;; *) cache_dir=$(pwd)/$cache_dir ;; esac
case "$output_dir" in /*) ;; *) output_dir=$(pwd)/$output_dir ;; esac
mkdir -p -- "$cache_dir" "$output_dir"
cache_archive="$cache_dir/zeroclaw-${UPSTREAM_TAG}-${UPSTREAM_COMMIT}.tar"
cargo_target_dir="$cache_dir/cargo-target-${UPSTREAM_COMMIT}"
work_dir=$(mktemp -d "${TMPDIR:-/tmp}/tched-router-threetched.XXXXXX")
cleanup() { status=$?; rm -rf -- "$work_dir"; exit "$status"; }
trap cleanup EXIT
trap 'exit 130' HUP INT TERM

verify_archive() {
    archive=$1
    actual=$(sha256sum "$archive" | awk '{print $1}')
    [ "$actual" = "$SOURCE_ARCHIVE_SHA256" ] || {
        echo "pinned tched-router source archive digest mismatch" >&2
        exit 2
    }
}

if [ -n "$source_dir" ]; then
    [ -d "$source_dir/.git" ] || {
        echo "--source-dir must be a clean Git checkout" >&2
        exit 2
    }
    [ -z "$(git -C "$source_dir" status --porcelain --untracked-files=all)" ] || {
        echo "--source-dir is dirty; refusing an unreproducible threetched input" >&2
        exit 2
    }
    actual_commit=$(git -C "$source_dir" rev-parse HEAD)
    [ "$actual_commit" = "$UPSTREAM_COMMIT" ] || {
        echo "--source-dir is $actual_commit, expected $UPSTREAM_COMMIT" >&2
        exit 2
    }
    candidate_archive="$work_dir/source.tar"
    git -C "$source_dir" archive --format=tar "$UPSTREAM_COMMIT" > "$candidate_archive"
    verify_archive "$candidate_archive"
    cp -- "$candidate_archive" "$cache_archive.new"
    mv -f -- "$cache_archive.new" "$cache_archive"
elif [ -n "$source_archive" ]; then
    verify_archive "$source_archive"
    cp -- "$source_archive" "$cache_archive.new"
    mv -f -- "$cache_archive.new" "$cache_archive"
elif [ -f "$package_dir/source.tar.gz" ]; then
    gzip -dc "$package_dir/source.tar.gz" > "$cache_archive.new"
    verify_archive "$cache_archive.new"
    mv -f -- "$cache_archive.new" "$cache_archive"
elif [ -f "$cache_archive" ]; then
    verify_archive "$cache_archive"
else
    fetch_dir="$work_dir/fetch"
    git init -q "$fetch_dir"
    git -C "$fetch_dir" fetch -q --depth 1 "$UPSTREAM_URL" "refs/tags/$UPSTREAM_TAG"
    actual_commit=$(git -C "$fetch_dir" rev-parse 'FETCH_HEAD^{commit}')
    [ "$actual_commit" = "$UPSTREAM_COMMIT" ] || {
        echo "upstream tag resolved to $actual_commit, expected $UPSTREAM_COMMIT" >&2
        exit 2
    }
    git -C "$fetch_dir" archive --format=tar "$actual_commit" > "$cache_archive.new"
    verify_archive "$cache_archive.new"
    mv -f -- "$cache_archive.new" "$cache_archive"
fi

verify_archive "$cache_archive"
if [ "$verify_source_only" = 1 ]; then
    printf '%s  %s\n' "$SOURCE_ARCHIVE_SHA256" "$cache_archive"
    exit 0
fi
source_root="$work_dir/source"
mkdir -p -- "$source_root"
tar -xf "$cache_archive" -C "$source_root"
"$package_dir/apply-threetched-patches.sh" "$source_root"
verify_build_inputs

export CARGO_TARGET_DIR="$cargo_target_dir"
(cd "$source_root" && cargo build --locked --release --bin threetched --features plugins-wasm-cranelift)
# A shared checkout may change during a long compile. Never label an older
# materialized source tree with the newly edited patch package's provenance.
verify_build_inputs
candidate_binary="$cargo_target_dir/release/threetched"
[ -x "$candidate_binary" ] || {
    echo "threetched build did not produce threetched" >&2
    exit 2
}
"$candidate_binary" plugin --help >/dev/null
"$candidate_binary" --version | grep -Fx "threetched ${UPSTREAM_TAG#v}" >/dev/null || {
    echo "threetched candidate reports an unexpected version" >&2
    exit 2
}
# threetched schema discovery must not initialize, migrate or decrypt an instance.
# Use a fresh explicit selector and a cleared environment so this preparation
# probe cannot observe inherited credentials or select a live installation.
introspection_dir="$work_dir/introspection"
mkdir -p -- "$introspection_dir"
env -i PATH="$PATH" THREETCHED_CONFIG_DIR="$introspection_dir" \
    LANGUAGE=en LC_ALL=C LANG=C NO_COLOR=1 \
    "$candidate_binary" config schema > "$work_dir/config-schema.json"
jq -e 'type == "object"' "$work_dir/config-schema.json" >/dev/null || {
    echo "threetched config schema did not emit a JSON object" >&2
    exit 2
}
[ -z "$(find "$introspection_dir" -mindepth 1 -print -quit)" ] || {
    echo "threetched config schema mutated the isolated runtime installation" >&2
    exit 2
}
strings "$candidate_binary" | grep -F "tched_router.generation.v1" >/dev/null || {
    echo "threetched candidate lacks the patched generation protocol" >&2
    exit 2
}

binary_sha256=$(sha256sum "$candidate_binary" | awk '{print $1}')
binary_bytes=$(wc -c < "$candidate_binary" | tr -d ' ')
patch_one_sha256=$PATCH_LAGGED_SSE_SHA256
patch_two_sha256=$PATCH_STATELESS_GENERATION_SHA256
branding_sha256=$PATCH_BRANDING_SHA256
patch_inventory_sha256=$PATCH_INVENTORY_SHA256
base_manifest_sha256=$BASE_MANIFEST_SHA256
patched_lock_sha256=$(sha256sum "$source_root/Cargo.lock" | awk '{print $1}')
rust_target=$(rustc -vV | sed -n 's/^host: //p')
rust_version=$(rustc --version)
cargo_version=$(cargo --version)

artifact_tmp="$output_dir/threetched.new"
manifest_tmp="$output_dir/manifest.json.new"
install -m 0755 "$candidate_binary" "$artifact_tmp"
jq -n \
    --arg format "opdbus.threetched.v1" \
    --arg upstream_url "$UPSTREAM_URL" \
    --arg upstream_tag "$UPSTREAM_TAG" \
    --arg upstream_commit "$UPSTREAM_COMMIT" \
    --arg source_archive_sha256 "$SOURCE_ARCHIVE_SHA256" \
    --arg vendored_source_gzip_sha256 "$VENDORED_SOURCE_GZIP_SHA256" \
    --arg patch_one_sha256 "$patch_one_sha256" \
    --arg patch_two_sha256 "$patch_two_sha256" \
    --arg branding_sha256 "$branding_sha256" \
    --arg patch_inventory_sha256 "$patch_inventory_sha256" \
    --arg base_manifest_sha256 "$base_manifest_sha256" \
    --arg patched_lock_sha256 "$patched_lock_sha256" \
    --arg binary_sha256 "$binary_sha256" \
    --argjson binary_bytes "$binary_bytes" \
    --arg rust_target "$rust_target" \
    --arg rust_version "$rust_version" \
    --arg cargo_version "$cargo_version" \
    '{format:$format,upstream:{url:$upstream_url,tag:$upstream_tag,commit:$upstream_commit,source_archive_sha256:$source_archive_sha256,vendored_source_gzip_sha256:$vendored_source_gzip_sha256},patches:{lagged_sse_sha256:$patch_one_sha256,stateless_generation_sha256:$patch_two_sha256,branding_sha256:$branding_sha256,inventory_sha256:$patch_inventory_sha256,base_manifest_sha256:$base_manifest_sha256,patched_cargo_lock_sha256:$patched_lock_sha256},artifact:{name:"threetched",sha256:$binary_sha256,bytes:$binary_bytes,rust_target:$rust_target},toolchain:{rustc:$rust_version,cargo:$cargo_version}}' \
    > "$manifest_tmp"
mv -f -- "$artifact_tmp" "$output_dir/threetched"
mv -f -- "$manifest_tmp" "$output_dir/manifest.json"
sha256sum "$output_dir/threetched" "$output_dir/manifest.json"
