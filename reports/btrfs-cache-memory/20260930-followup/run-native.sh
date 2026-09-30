#!/usr/bin/env bash
set -euo pipefail
repo=/srv/git/odbus-verified
report=$repo/reports/btrfs-cache-memory/20260930-followup
cd "$repo"
export PATH=/usr/bin:/bin
args=()
while IFS= read -r arg; do args+=("$arg"); done < "$report/rustc-externs.txt"
native=()
while IFS= read -r archive; do native+=("-Lnative=$(dirname "$archive")"); done < "$report/selected-native-archives.txt"
printf '%s\n' "${args[@]}" "${native[@]}" > "$report/native-raw/build-args.txt"
timeout -k 5 300 /usr/bin/rustc --edition=2024 --target=x86_64-unknown-linux-gnu --crate-name cache_native_probe -C debuginfo=1 -L dependency=target/debug/deps "${args[@]}" "${native[@]}" "$report/native_probe.rs" -o "$report/native-raw/cache_native_probe" > "$report/native-raw/build.log" 2>&1
