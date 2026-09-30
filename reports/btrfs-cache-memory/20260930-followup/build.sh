#!/usr/bin/env bash
set -euo pipefail
cd /srv/git/odbus-verified
report=reports/btrfs-cache-memory/20260930-followup
wrapper=${1:-wrapper.rs}
output=${2:-cache-probe}
mapfile -t externs < "$report/rustc-externs.txt"
native=()
while IFS= read -r archive; do native+=("-Lnative=${archive%/*}"); done < "$report/selected-native-archives.txt"
set -x
timeout --signal=TERM --kill-after=5s 300s /usr/bin/rustc --edition=2024 --target=x86_64-unknown-linux-gnu --crate-name bounded_cache_probe --crate-type bin -Cdebuginfo=0 -Ldependency=target/debug/deps "${externs[@]}" "${native[@]}" "$report/$wrapper" -o "$report/$output"
