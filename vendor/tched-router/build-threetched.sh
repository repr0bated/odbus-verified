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

Builds the pinned upstream zeroclaw threetched dependency. It writes only the
cache, Cargo target directory and output directory; it never installs a binary.
With no explicit source option it prefers the checked-in source.tar.gz, then a
verified cache entry, and only then the exact upstream tag. --verify-source-only
stops after materializing and verifying the deterministic source tar.
EOF
    exit 2
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --cache-dir)
            if [ "$#" -ge 2 ]; then
                cache_dir=$2
                shift 2
            else
                usage
            fi ;;
        --output-dir)
            if [ "$#" -ge 2 ]; then
                output_dir=$2
                shift 2
            else
                usage
            fi ;;
        --source-dir)
            if [ "$#" -ge 2 ]; then
                source_dir=$2
                shift 2
            else
                usage
            fi ;;
        --source-archive)
            if [ "$#" -ge 2 ]; then
                source_archive=$2
                shift 2
            else
                usage
            fi ;;
        --verify-source-only) verify_source_only=1; shift ;;
        -h|--help) usage ;;
        *) usage ;;
    esac
done

if [ -z "$cache_dir" ]; then
    usage
fi
if [ -n "$source_dir" ] && [ -n "$source_archive" ]; then
    echo "choose only one of --source-dir and --source-archive" >&2
    exit 2
fi
for command_name in awk cargo git grep gzip install jq rustc sha256sum strings tar; do
    if ! command -v "$command_name" >/dev/null 2>&1; then
        echo "missing threetched build command: $command_name" >&2
        exit 127
    fi
done

verify_package_file() {
    _vpf_path=$1
    _vpf_expected=$2
    _vpf_actual=$(sha256sum "$_vpf_path" | awk '{print $1}')
    if [ "$_vpf_actual" != "$_vpf_expected" ]; then
        echo "threetched package digest mismatch: $_vpf_path" >&2
        exit 2
    fi
}

verify_build_inputs() {
    if [ ! -f "$package_dir/source.pin" ]; then
        echo "missing source pin" >&2
        exit 1
    fi
}

case "$cache_dir" in /*) ;; *) cache_dir=$(pwd)/$cache_dir ;; esac
case "$output_dir" in /*) ;; *) output_dir=$(pwd)/$output_dir ;; esac
mkdir -p -- "$cache_dir" "$output_dir"
cache_archive="$cache_dir/zeroclaw-${UPSTREAM_TAG}-${UPSTREAM_COMMIT}.tar"
cargo_target_dir="$cache_dir/cargo-target-${UPSTREAM_COMMIT}"
work_dir=$(mktemp -d "${TMPDIR:-/tmp}/tched-router-threetched.XXXXXX")
cleanup() {
    _status=$?
    rm -rf -- "$work_dir"
    exit "$_status"
}
trap cleanup EXIT
trap 'exit 130' HUP INT TERM

verify_archive() {
    sha256sum -- "$1" |
        awk -v expected="$SOURCE_ARCHIVE_SHA256" '
            $1 != expected {
                print "source archive SHA256 mismatch" > "/dev/stderr"
                exit 1
            }
        '
}

if [ -n "$source_dir" ]; then
    actual_commit=$(git -C "$source_dir" rev-parse HEAD 2>/dev/null) || true
    if [ -z "$actual_commit" ]; then
        echo "cannot read git rev from source directory" >&2
        exit 2
    fi
    if [ "$actual_commit" != "$UPSTREAM_COMMIT" ]; then
        echo "upstream tag resolved to $actual_commit, expected $UPSTREAM_COMMIT" >&2
        exit 2
    fi
    git -C "$source_dir" archive --format=tar "$actual_commit" > "$cache_archive.new"
    verify_archive "$cache_archive.new"
    mv -f -- "$cache_archive.new" "$cache_archive"
elif [ -n "$source_archive" ]; then
    if [ ! -f "$source_archive" ]; then
        echo "source archive not found: $source_archive" >&2
        exit 2
    fi
    cp -- "$source_archive" "$cache_archive"
    verify_archive "$cache_archive"
elif [ -f "$cache_archive" ]; then
    verify_archive "$cache_archive"
elif [ -f "$package_dir/source.tar.gz" ]; then
    verify_package_file "$package_dir/source.tar.gz" "$VENDORED_SOURCE_GZIP_SHA256"
    gzip -dc -- "$package_dir/source.tar.gz" > "$cache_archive.new"
    verify_archive "$cache_archive.new"
    mv -f -- "$cache_archive.new" "$cache_archive"
else
    fetch_dir=$(mktemp -d "${TMPDIR:-/tmp}/tched-router-fetch.XXXXXX")
    trap 'rm -rf -- "$fetch_dir"; exit $?' EXIT HUP INT TERM
    git -C "$fetch_dir" clone --depth 1 --no-tags "https://github.com/zeroclaw-labs/zeroclaw.git" .
    actual_commit=$(git -C "$fetch_dir" rev-parse HEAD) || true
    if [ -z "$actual_commit" ]; then
        echo "cannot read git rev from fetch directory" >&2
        exit 2
    fi
    if [ "$actual_commit" != "$UPSTREAM_COMMIT" ]; then
        echo "upstream tag resolved to $actual_commit, expected $UPSTREAM_COMMIT" >&2
        exit 2
    fi
    git -C "$fetch_dir" archive --format=tar "$actual_commit" > "$cache_archive.new"
    verify_archive "$cache_archive.new"
    mv -f -- "$cache_archive.new" "$cache_archive"
fi

verify_build_inputs
verify_archive "$cache_archive"
if [ "$verify_source_only" = "1" ]; then
    printf '%s  %s\n' "$SOURCE_ARCHIVE_SHA256" "$cache_archive"
    exit 0
fi

source_root="$work_dir/source"
mkdir -p -- "$source_root"
tar -xf "$cache_archive" -C "$source_root"

export CARGO_TARGET_DIR="$cargo_target_dir"
(cd "$source_root" && cargo build --locked --release --bin zeroclaw --features plugins-wasm-cranelift)

candidate_binary="$cargo_target_dir/release/zeroclaw"
if [ ! -x "$candidate_binary" ]; then
    echo "threetched build did not produce upstream zeroclaw" >&2
    exit 2
fi
"$candidate_binary" plugin --help >/dev/null
"$candidate_binary" --version | grep -Fx "zeroclaw ${UPSTREAM_TAG#v}" >/dev/null

# Unmodified upstream `config schema` initializes the selected config directory
# (config.toml, data/, shared/). Point it at a fresh throwaway selector with a
# cleared environment so the probe cannot observe inherited credentials or
# touch a live installation; the directory is discarded with $work_dir.
introspection_dir="$work_dir/introspection"
mkdir -p -- "$introspection_dir/config" "$introspection_dir/home"
env -i PATH="$PATH" HOME="$introspection_dir/home" ZEROCLAW_CONFIG_DIR="$introspection_dir/config" \
    LANGUAGE=en LC_ALL=C LANG=C NO_COLOR=1 \
    "$candidate_binary" config schema > "$work_dir/config-schema.json"
jq -e 'type == "object"' "$work_dir/config-schema.json" >/dev/null
if [ -n "$(find "$introspection_dir/home" -mindepth 1 -print -quit)" ]; then
    echo "threetched config schema wrote outside its selected config directory" >&2
    exit 2
fi

binary_sha256=$(sha256sum "$candidate_binary" | awk '{print $1}')
binary_bytes=$(wc -c < "$candidate_binary" | tr -d ' ')
rust_target=$(rustc -vV | sed -n 's/^host: //p')
rust_version=$(rustc --version)
cargo_version=$(cargo --version)
cargo_lock_sha256=$(sha256sum "$source_root/Cargo.lock" | awk '{print $1}')

artifact_tmp="$output_dir/threetched.new"
manifest_tmp="$output_dir/manifest.json.new"
install -m 0755 "$candidate_binary" "$artifact_tmp"
jq -n \
    --arg format "opdbus.threetched.v2" \
    --arg upstream_url "$UPSTREAM_URL" \
    --arg upstream_tag "$UPSTREAM_TAG" \
    --arg upstream_commit "$UPSTREAM_COMMIT" \
    --arg source_archive_sha256 "$SOURCE_ARCHIVE_SHA256" \
    --arg vendored_source_gzip_sha256 "$VENDORED_SOURCE_GZIP_SHA256" \
    --arg binary_sha256 "$binary_sha256" \
    --argjson binary_bytes "$binary_bytes" \
    --arg rust_target "$rust_target" \
    --arg rust_version "$rust_version" \
    --arg cargo_version "$cargo_version" \
    --arg cargo_lock_sha256 "$cargo_lock_sha256" \
    '{format:$format,source:{mode:"unmodified-upstream",cargo_lock_sha256:$cargo_lock_sha256},upstream:{url:$upstream_url,tag:$upstream_tag,commit:$upstream_commit,source_archive_sha256:$source_archive_sha256,vendored_source_gzip_sha256:$vendored_source_gzip_sha256},artifact:{name:"threetched",upstream_binary:"zeroclaw",sha256:$binary_sha256,bytes:$binary_bytes,rust_target:$rust_target},toolchain:{rustc:$rust_version,cargo:$cargo_version}}' \
    > "$manifest_tmp"
mv -f -- "$artifact_tmp" "$output_dir/threetched"
mv -f -- "$manifest_tmp" "$output_dir/manifest.json"
sha256sum "$output_dir/threetched" "$output_dir/manifest.json"
