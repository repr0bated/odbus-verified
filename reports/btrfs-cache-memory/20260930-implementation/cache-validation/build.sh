#!/usr/bin/env bash
# Exact-source fallback using the investigation's pinned GNU dependency set.
# Normal Cargo verification is owned by the parent task and logged separately.
set -euo pipefail
cd /srv/git/odbus-verified
report=reports/btrfs-cache-memory/20260930-implementation/cache-validation
mapfile -t cache_externs < reports/btrfs-cache-memory/20260930-followup/rustc-externs.txt
cache_native=()
while IFS= read -r archive; do
  cache_native+=("-Lnative=${archive%/*}")
done < reports/btrfs-cache-memory/20260930-followup/selected-native-archives.txt
for cache_probe in wrapper allocation-probe latency-current latency-baseline; do
  cache_mode=()
  if [ "$cache_probe" = wrapper ]; then cache_mode+=(--test); fi
  timeout --kill-after=5s 120s /usr/bin/rustc "${cache_mode[@]}" \
    --edition=2024 --target=x86_64-unknown-linux-gnu \
    --crate-name cache_implementation_final -Cdebuginfo=0 -Ccodegen-units=1 \
    -Clink-arg=-Wl,--no-keep-memory -Ldependency=target/debug/deps \
    "${cache_externs[@]}" "${cache_native[@]}" "$report/$cache_probe.rs" \
    -o "/var/tmp/opdbus-cache-final-$cache_probe" \
    > "$report/final-$cache_probe-build.log" 2>&1
done
