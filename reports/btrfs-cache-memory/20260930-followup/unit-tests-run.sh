#!/usr/bin/env bash
set -u
export PATH=/usr/bin:/bin
repo=/srv/git/odbus-verified
report=$repo/reports/btrfs-cache-memory/20260930-followup
cd "$repo" || exit 1
args=()
while IFS= read -r arg; do args+=("$arg"); done < /var/tmp/opdbus-cache-memory-review/api-wrapper.bL9tH1/rustc-externs.txt
native=()
while IFS= read -r archive; do native+=("-Lnative=$(dirname "$archive")"); done < /var/tmp/opdbus-cache-memory-review/api-wrapper.bL9tH1/selected-native-archives.txt
run_guarded() {
  local label=$1 seconds=$2
  shift 2
  local log="$report/native-raw/unit-tests-${label}.txt"
  setsid timeout -k 5 "$seconds" "$@" > "$log" 2>&1 &
  local guarded_pid=$! peak=0 rss=0 exceeded=0
  while kill -0 "$guarded_pid" 2>/dev/null; do
    rss=$(ps -eo pgid=,rss= | awk -v id="$guarded_pid" '$1==id {total += $2} END {print total+0}')
    if (( rss > peak )); then peak=$rss; fi
    if (( rss > 524288 )); then
      exceeded=1
      kill -TERM -- "-$guarded_pid" 2>/dev/null || true
      sleep 1
      kill -KILL -- "-$guarded_pid" 2>/dev/null || true
      break
    fi
    sleep 0.2
  done
  wait "$guarded_pid"
  local status=$?
  printf 'guard_exit=%s peak_group_rss_kib=%s limit_kib=524288 limit_exceeded=%s timeout_seconds=%s\n' "$status" "$peak" "$exceeded" "$seconds" >> "$log"
  return "$status"
}
run_guarded build 120 /usr/bin/rustc --test --edition=2024 --target=x86_64-unknown-linux-gnu --crate-name cache_native_unit_tests -C debuginfo=0 -C codegen-units=1 -C link-arg=-Wl,--no-keep-memory -L dependency=target/debug/deps "${args[@]}" "${native[@]}" "$report/unit-tests-probe.rs" -o "$report/native-raw/unit-tests" || exit $?
TMPDIR=$(mktemp -d /dev/shm/opdbus-cache-unit-tests.XXXXXX) || exit 1
export TMPDIR
printf 'tmpdir=%s filesystem=tmpfs ordinary-filesystem-fallback-only\n' "$TMPDIR" > "$report/native-raw/unit-tests-fixture.txt"
run_guarded run 120 "$report/native-raw/unit-tests" 'numa::tests::' 'snapshot_manager::tests::' --test-threads=1
status=$?
if rmdir "$TMPDIR"; then printf 'empty_fixture_removed=true\n' >> "$report/native-raw/unit-tests-fixture.txt"; else printf 'empty_fixture_removed=false\n' >> "$report/native-raw/unit-tests-fixture.txt"; fi
exit "$status"
