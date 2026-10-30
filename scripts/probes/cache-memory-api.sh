#!/usr/bin/env bash
# Compile the current BtrfsCache/NUMA/snapshot source modules against reusable
# workspace rlibs, then run isolated allocation probes in separate processes.
# This is a source-accurate fallback when Cargo would rebuild RocksDB; it is not
# a Cargo build and does not validate native Btrfs behavior.
# To repeat a known compatible set, set OPDBUS_CACHE_MEMORY_ARTIFACT_RECEIPT
# to its evidence directory. OPDBUS_RLIB_<UPPERCASE_CRATE> overrides one rlib.
set -euo pipefail
shopt -s nullglob

repo_dir=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
cd "$repo_dir"
export PATH=/usr/bin:/bin

if [[ ! -d target/debug/deps ]]; then
    echo "missing target/debug/deps; run a normal Cargo build first" >&2
    exit 2
fi

evidence_root=${OPDBUS_CACHE_MEMORY_EVIDENCE_ROOT:-/var/tmp/opdbus-cache-memory-review}
mkdir -p "$evidence_root"
output_dir=$(mktemp -d "$evidence_root/api-wrapper.XXXXXX")
echo "evidence_dir=$output_dir"

pick_rlib() {
    local crate=$1
    local matches=(target/debug/deps/lib"$crate"-*.rlib)
    ((${#matches[@]} > 0)) || { echo "missing rlib for $crate" >&2; return 1; }
    local sorted
    sorted=$(ls -t "${matches[@]}")
    printf '%s\n' "${sorted%%$'\n'*}"
}

pick_archive() {
    (($# > 0)) || return 0
    local sorted
    sorted=$(ls -t "$@")
    printf '%s\n' "${sorted%%$'\n'*}"
}

# Replay an already recorded compatible artifact set. Newest rlibs alone may
# belong to different Cargo feature/dependency graphs. Explicit per-crate
# OPDBUS_RLIB_* overrides still take precedence.
artifact_receipt=${OPDBUS_CACHE_MEMORY_ARTIFACT_RECEIPT:-}
declare -A recorded_rlibs=()
if [[ -n "$artifact_receipt" ]]; then
    [[ -f "$artifact_receipt/rustc-externs.txt" && -f "$artifact_receipt/selected-native-archives.txt" ]] || {
        echo "artifact receipt is missing selection files: $artifact_receipt" >&2
        exit 2
    }
    while IFS= read -r spec; do
        [[ "$spec" == --extern=*=* ]] || { echo "invalid recorded extern: $spec" >&2; exit 2; }
        spec=${spec#--extern=}
        recorded_rlibs[${spec%%=*}]=${spec#*=}
    done < "$artifact_receipt/rustc-externs.txt"
fi

{
    echo "checkout=$(git rev-parse HEAD)"
    echo "branch=$(git branch --show-current)"
    echo "artifact_receipt=${artifact_receipt:-automatic-newest}"
    /usr/bin/rustc --version
    /usr/bin/rustc -vV
    /usr/bin/cargo --version
    echo "Cargo.lock sha256:"
    sha256sum Cargo.lock
    echo "source sha256:"
    sha256sum crates/op-cache/examples/cache_memory_probe.rs \
        crates/op-cache/src/btrfs_cache.rs crates/op-cache/src/numa.rs \
        crates/op-cache/src/snapshot_manager.rs crates/op-cache/src/managed_process.rs crates/op-cozo-store/src/lib.rs
} > "$output_dir/provenance.txt"

# The example uses op_cache::BtrfsCache. Inline it beside the exact current
# source modules and redirect that single import to the wrapper's crate root.
sed '1,5d; s/use op_cache::BtrfsCache;/use crate::btrfs_cache::BtrfsCache;/' \
    crates/op-cache/examples/cache_memory_probe.rs > "$output_dir/probe.rs"
cat > "$output_dir/wrapper.rs" <<EOF_WRAPPER
#[path = "$repo_dir/crates/op-cache/src/managed_process.rs"]
mod managed_process;
#[path = "$repo_dir/crates/op-cache/src/numa.rs"]
mod numa;
#[path = "$repo_dir/crates/op-cache/src/snapshot_manager.rs"]
mod snapshot_manager;
#[path = "$repo_dir/crates/op-cache/src/btrfs_cache.rs"]
mod btrfs_cache;
include!("$output_dir/probe.rs");
EOF_WRAPPER

crates=(anyhow bincode chrono libc log num_cpus op_cozo_store serde serde_json sha2 simd_json tempfile tokio tracing uuid)
externs=()
selected_rlibs=()
for crate in "${crates[@]}"; do
    override_name=OPDBUS_RLIB_$(printf '%s' "$crate" | tr '[:lower:]' '[:upper:]')
    if [[ -n "${!override_name:-}" ]]; then
        lib=${!override_name}
    elif [[ -n "$artifact_receipt" ]]; then
        lib=${recorded_rlibs[$crate]:-}
        [[ -n "$lib" ]] || { echo "receipt has no rlib for $crate" >&2; exit 2; }
    else
        lib=$(pick_rlib "$crate")
    fi
    [[ -f "$lib" ]] || { echo "missing selected rlib: $lib" >&2; exit 1; }
    printf '%s\n' "$lib" >> "$output_dir/selected-rlibs.txt"
    printf '%s\n' "--extern=$crate=$lib" >> "$output_dir/rustc-externs.txt"
    externs+=("--extern" "$crate=$lib")
    selected_rlibs+=("$lib")
done
{
    echo "selected rlib sha256:"
    sha256sum "${selected_rlibs[@]}"
    echo "native search directories:"
    if [[ -n "$artifact_receipt" ]]; then
        cp "$artifact_receipt/selected-native-archives.txt" "$output_dir/selected-native-archives.txt"
    else
        for lib in rocksdb zstd sqlite3 lz4; do
            case "$lib" in
                rocksdb) archive=$(pick_archive target/debug/build/cozorocks-*/out/librocksdb.a) ;;
                zstd) archive=$(pick_archive target/debug/build/zstd-sys-*/out/libzstd.a) ;;
                sqlite3) archive=$(pick_archive target/debug/build/libsqlite3-sys-*/out/libsqlite3.a) ;;
                lz4) archive=$(pick_archive target/debug/build/lz4-sys-*/out/liblz4.a) ;;
            esac
            [[ -n "$archive" ]] || { echo "missing native archive for $lib" >&2; exit 2; }
            printf '%s\n' "$archive" >> "$output_dir/selected-native-archives.txt"
        done
    fi
    [[ -f "$output_dir/selected-native-archives.txt" ]] || { echo "no native archives selected" >&2; exit 2; }
    while IFS= read -r archive; do
        [[ -f "$archive" ]] || { echo "missing selected native archive: $archive" >&2; exit 2; }
        echo "$(dirname "$archive")"
        echo "$archive"
        sha256sum "$archive"
        if [[ "$(basename "$archive")" == librocksdb.a ]]; then
            # The same directory also supplies the Cozo C++ binding archive.
            sha256sum "$(dirname "$archive")/libcozorocks.a"
        fi
    done < "$output_dir/selected-native-archives.txt"
} >> "$output_dir/provenance.txt"

native_args=()
while IFS= read -r archive; do
    native_args+=("-Lnative=$(dirname "$archive")")
done < "$output_dir/selected-native-archives.txt"
set -x
if /usr/bin/rustc --edition=2024 --target=x86_64-unknown-linux-gnu \
    --crate-name cache_memory_probe --crate-type bin \
    -C debuginfo=0 -L dependency=target/debug/deps \
    "${externs[@]}" "${native_args[@]}" \
    "$output_dir/wrapper.rs" -o "$output_dir/cache_memory_probe" \
    > "$output_dir/build.log" 2>&1; then
    build_status=0
else
    build_status=$?
fi
set +x
echo "wrapper_build_exit=$build_status" | tee -a "$output_dir/build.log"
if ((build_status != 0)); then
    cat "$output_dir/build.log" >&2
    exit "$build_status"
fi

run_case() {
    local case=$1 entries=$2 bytes=$3
    local log="$output_dir/$case.log" status
    if env OPDBUS_CACHE_MEMORY_MAX_ENTRIES="$entries" \
        OPDBUS_CACHE_MEMORY_MAX_BYTES="$bytes" \
        "$output_dir/cache_memory_probe" "$case" > "$log" 2>&1; then
        status=0
    else
        status=$?
    fi
    cat "$log"
    echo "case=$case exit=$status" | tee -a "$log"
    ((status == 0))
}
run_case index 0 0
run_case keys 32 32
run_case cleanup 8 4194304
