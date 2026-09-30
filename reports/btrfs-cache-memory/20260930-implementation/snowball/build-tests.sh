#!/usr/bin/env bash
set -euo pipefail
cd /srv/git/odbus-verified
report=reports/btrfs-cache-memory/20260930-implementation/snowball
mapfile -t externs < "$report/rustc-externs.txt"
native=()
while IFS= read -r archive; do native+=("-Lnative=${archive%/*}"); done < reports/btrfs-cache-memory/20260930-followup/selected-native-archives.txt
set -x
timeout --signal=TERM --kill-after=5s 300s /usr/bin/rustc --edition=2024 --target=x86_64-unknown-linux-gnu --crate-name snowball_focused --test -Cdebuginfo=0 -Ldependency=target/debug/deps "${externs[@]}" --extern=op_snowball=target/debug/deps/libop_snowball-6d8072a930ef13a3.rlib --extern=nix=target/debug/deps/libnix-3fbb86f11b6744f2.rlib "${native[@]}" "$report/test-wrapper.rs" -o "$report/focused-tests"
