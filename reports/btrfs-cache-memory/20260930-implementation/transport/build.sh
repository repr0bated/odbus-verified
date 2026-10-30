#!/usr/bin/env bash
set -euo pipefail
cd /srv/git/odbus-verified
report=reports/btrfs-cache-memory/20260930-implementation/transport
mapfile -t externs < reports/btrfs-cache-memory/20260930-followup/rustc-externs.txt
native=()
while IFS= read -r archive; do native+=("-Lnative=${archive%/*}"); done < reports/btrfs-cache-memory/20260930-followup/selected-native-archives.txt
timeout -k 5 120 /usr/bin/rustc --edition=2024 --crate-name cache_fixed_native -Cdebuginfo=0 -Ldependency=target/debug/deps "${externs[@]}" "${native[@]}" "$report/native.rs" -o "$report/native"
