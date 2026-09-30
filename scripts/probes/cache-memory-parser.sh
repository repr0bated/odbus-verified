#!/bin/sh
# Run from the repository root. Compiles a bounded source-extracted parser
# against existing GNU debug rlibs; no Cargo, Btrfs, network or service actions.
# This is an allocation-ownership probe, not a full projector/live RSS proof.
set -eu
repo=$(pwd -P)
src="$repo/crates/op-snowball/src/snowball.rs"
expected_sha=2fe22e904734c8bf69454da13544ca8cf9d2c1cda7f82a36813ebddf1d51e239
if [ ! -f "$repo/Cargo.toml" ] || [ ! -f "$src" ]; then
    printf '%s\n' 'Run this probe from the repository root.' >&2
    exit 1
fi
actual_sha=$(sha256sum "$src" | cut -d ' ' -f 1)
if [ "$actual_sha" != "$expected_sha" ]; then
    printf '%s\n' 'Parser source changed: review extraction ranges and update the expected SHA before running.' >&2
    exit 1
fi
if ! rg -q '^const MAX_EMBEDDING_FIELD_LEN: usize = 512;$' "$src"; then
    printf '%s\n' 'Expected the exact 512-byte flattening constant.' >&2
    exit 1
fi
anyhow_lib=$(printf '%s\n' "$repo"/target/debug/deps/libanyhow-*.rlib | head -n 1)
simd_lib=$(printf '%s\n' "$repo"/target/debug/deps/libsimd_json-*.rlib | head -n 1)
if [ ! -f "$anyhow_lib" ] || [ ! -f "$simd_lib" ]; then
    printf '%s\n' 'Existing GNU target/debug/deps anyhow and simd_json rlibs are required; this probe never builds them.' >&2
    exit 1
fi
probe_tmp_parent=${TMPDIR:-/var/tmp}
out=$(mktemp -d "$probe_tmp_parent/opdbus-snowball-parser.XXXXXX")
printf 'Evidence directory: %s\n' "$out"
{
cat <<'RS'
use anyhow::Result;
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
struct TrackingAllocator;
fn account_alloc(size: usize) {
    let now = LIVE.fetch_add(size, Ordering::Relaxed) + size;
    PEAK.fetch_max(now, Ordering::Relaxed);
}
unsafe impl GlobalAlloc for TrackingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(layout) };
        if !p.is_null() { account_alloc(layout.size()); }
        p
    }
    unsafe fn dealloc(&self, p: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
        unsafe { System.dealloc(p, layout) }
    }
    unsafe fn realloc(&self, p: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let result = unsafe { System.realloc(p, layout, size) };
        if !result.is_null() {
            LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
            account_alloc(size);
        }
        result
    }
}
#[global_allocator]
static ALLOC: TrackingAllocator = TrackingAllocator;
const MAX_EMBEDDING_FIELD_LEN: usize = 512;
RS
sed -n '1040,1077p' "$src"
printf "}\n"
sed -n '1123,1185p' "$src"
cat <<'RS'
fn rss_kib() -> usize {
    std::fs::read_to_string("/proc/self/status").unwrap().lines()
        .find(|line| line.starts_with("VmRSS:"))
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap().parse().unwrap()
}
fn main() {
    let payload = "x".repeat(512);
    let record = format!(r#"{{"hash":"0123456789abcdef","category":"incus","action":"instance_updated","timestamp":1,"data":{{"plugin_id":"incus","operation":"instance_updated","payload":{{"event_id":1,"event_hash":"abcdef","prev_hash":"000000","input_patch_hash":"abcd","input_payload":{{"message":"{}","items":["one","two","three"]}}}},"metadata":{{"actor_id":"principal-1","session_genesis":"genesis","decision":"Allow"}}}}}}"#, payload);
    println!("exact_parser synthetic_record_bytes={} all_has_vector=true pass_limit=256", record.len());
    println!("N,baseline_live_bytes,scan_live_delta_bytes,scan_peak_delta_bytes,data_fields_count,flattened_string_capacity_bytes,post_drop_live_delta_bytes,rss_after_scan_kib,rss_after_drop_kib");
    for n in [1000, 5000, 10000] {
        let baseline = LIVE.load(Ordering::Relaxed);
        PEAK.store(baseline, Ordering::Relaxed);
        let mut all = Vec::new();
        for block_num in 1..=n {
            all.push(ChainBlockRef::from_timing_bytes(block_num, record.as_bytes().to_vec(), true).unwrap());
        }
        all.sort_by_key(|block| block.block_num);
        let scan_live = LIVE.load(Ordering::Relaxed).saturating_sub(baseline);
        let scan_peak = PEAK.load(Ordering::Relaxed).saturating_sub(baseline);
        let fields: usize = all.iter().map(|b| b.data_fields.len()).sum();
        let strings: usize = all.iter().flat_map(|b| &b.data_fields).map(|(k,v)| k.capacity()+v.capacity()).sum();
        let scan_rss = rss_kib();
        let already_present = all.iter().filter(|block| block.has_vector).count();
        let pending: Vec<ChainBlockRef> = all.into_iter().filter(|block| !block.has_vector).take(256).collect();
        assert_eq!(already_present, n as usize);
        assert!(pending.is_empty());
        let post_filter = LIVE.load(Ordering::Relaxed).saturating_sub(baseline);
        eprintln!("post_filter N={n} pending_len={} pending_capacity={} live_delta_bytes={post_filter}", pending.len(), pending.capacity());
        drop(pending);
        let post_drop = LIVE.load(Ordering::Relaxed).saturating_sub(baseline);
        let drop_rss = rss_kib();
        println!("{n},{baseline},{scan_live},{scan_peak},{fields},{strings},{post_drop},{scan_rss},{drop_rss}");
    }
    let baseline = LIVE.load(Ordering::Relaxed);
    let mut all: Vec<ChainBlockRef> = (1..=10000).map(|n| ChainBlockRef::from_timing_bytes(n, record.as_bytes().to_vec(), true).unwrap()).collect();
    let with_flattening = LIVE.load(Ordering::Relaxed).saturating_sub(baseline);
    for block in &mut all { block.data_fields = Vec::new(); }
    let structured_only = LIVE.load(Ordering::Relaxed).saturating_sub(baseline);
    drop(all);
    println!("flatten_duplication_10000: with_flattening_bytes={with_flattening} structured_only_bytes={structured_only} freed_flattened_bytes={} post_drop_live_delta_bytes={}", with_flattening - structured_only, LIVE.load(Ordering::Relaxed).saturating_sub(baseline));
    let large_payload = "y".repeat(65536);
    let large_record = format!(r#"{{"hash":"abcdef","category":"incus","action":"updated","timestamp":1,"data":{{"payload":{{"message":"{}"}}}}}}"#, large_payload);
    let baseline = LIVE.load(Ordering::Relaxed);
    PEAK.store(baseline, Ordering::Relaxed);
    let block = ChainBlockRef::from_timing_bytes(1, large_record.as_bytes().to_vec(), true).unwrap();
    let flat = &block.data_fields[0].1;
    let simd_json::OwnedValue::String(structured) = &block.data["payload"]["message"] else { panic!("expected string") };
    let live = LIVE.load(Ordering::Relaxed).saturating_sub(baseline);
    let peak = PEAK.load(Ordering::Relaxed).saturating_sub(baseline);
    println!("large_scalar_65536: flat_len={} flat_capacity={} structured_len={} structured_capacity={} live_delta_bytes={live} peak_delta_bytes={peak}", flat.len(), flat.capacity(), structured.len(), structured.capacity());
    drop(block);
    println!("large_scalar_post_drop_live_delta_bytes={}", LIVE.load(Ordering::Relaxed).saturating_sub(baseline));
}
RS
} > "$out/exact-parser-probe.rs"
{
    printf 'source=%s\nranges=1040-1077,1123-1185\n' "$src"
    sha256sum "$src" "$out/exact-parser-probe.rs" "$anyhow_lib" "$simd_lib"
    /usr/bin/rustc -vV
    printf 'compile_flags=--edition=2024 --target=x86_64-unknown-linux-gnu -C opt-level=2\n'
    printf 'anyhow_rlib=%s\nsimd_json_rlib=%s\n' "$anyhow_lib" "$simd_lib"
} > "$out/provenance.txt"
/usr/bin/rustc --edition=2024 --target=x86_64-unknown-linux-gnu -C opt-level=2 -L dependency="$repo/target/debug/deps" --extern anyhow="$anyhow_lib" --extern simd_json="$simd_lib" "$out/exact-parser-probe.rs" -o "$out/exact-parser-probe"
"$out/exact-parser-probe" > "$out/results.csv" 2> "$out/post-filter.txt"
cat "$out/results.csv"
cat "$out/post-filter.txt"
