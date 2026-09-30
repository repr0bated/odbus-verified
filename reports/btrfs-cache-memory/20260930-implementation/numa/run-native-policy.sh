#!/usr/bin/env bash
set -euo pipefail
export PATH=/usr/bin:/bin
repo=/srv/git/odbus-verified
report=$repo/reports/btrfs-cache-memory/20260930-implementation/numa
cd "$repo"
args=()
while IFS= read -r arg; do args+=("$arg"); done < /var/tmp/opdbus-cache-memory-review/api-wrapper.bL9tH1/rustc-externs.txt
run_guarded() {
 local label=$1 limit=$2 seconds=$3
 shift 3
 local log="$report/$label.log"
 setsid timeout -k 5 "$seconds" "$@" > "$log" 2>&1 &
 local pid=$! peak=0 rss=0 exceeded=0
 while kill -0 "$pid" 2>/dev/null; do
  rss=$(ps -eo pgid=,rss= | awk -v id="$pid" '$1==id {total += $2} END {print total+0}')
  if ((rss>peak)); then peak=$rss; fi
  if ((rss>limit)); then exceeded=1; kill -TERM -- "-$pid" 2>/dev/null || true; sleep 1; kill -KILL -- "-$pid" 2>/dev/null || true; break; fi
  sleep 0.1
 done
 local status=0
 wait "$pid" || status=$?
 printf 'guard_exit=%s peak_group_rss_kib=%s limit_kib=%s limit_exceeded=%s timeout_seconds=%s\n' "$status" "$peak" "$limit" "$exceeded" "$seconds" >> "$log"
 return "$status"
}
run_guarded native-build 1048576 180 /usr/bin/rustc --edition=2024 --target=x86_64-unknown-linux-gnu --crate-name numa_implementation_native_probe -C debuginfo=0 -C codegen-units=1 -C link-arg=-Wl,--no-keep-memory -L dependency=target/debug/deps "${args[@]}" "$report/native-policy-probe.rs" -o "$report/native-policy-probe"
run_guarded native-policy 524288 120 "$report/native-policy-probe"
