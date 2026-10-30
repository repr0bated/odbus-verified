#!/usr/bin/env bash
set -euo pipefail
cd /srv/git/odbus-verified
report=reports/btrfs-cache-memory/20260930-followup
case=$1
size=$2
repeat=$3
binary=$report/cache-probe
if [[ $case = expiry-mitigated ]]; then binary=$report/cache-probe-mitigated; fi
if [[ $case = cold-corruption ]]; then binary=$report/cache-probe-cold; fi
log=$report/raw/$case-$size-r$repeat
export OPDBUS_CACHE_PLACEMENT=disabled OPDBUS_CACHE_MEMORY_POLICY=default
export OPDBUS_CACHE_MEMORY_MAX_ENTRIES=64 OPDBUS_CACHE_MEMORY_MAX_BYTES=4194304
if [[ $case = cold-corruption ]]; then export OPDBUS_CACHE_MEMORY_MAX_ENTRIES=0 OPDBUS_CACHE_MEMORY_MAX_BYTES=0; fi
"$binary" "$case" "$size" > "$log.jsonl" 2> "$log.stderr" &
probe_pid=$!
trap 'kill -TERM "$probe_pid" 2>/dev/null || true' EXIT TERM INT
printf 'pid=%s case=%s size=%s repeat=%s\n' "$probe_pid" "$case" "$size" "$repeat" > "$log.guard"
started=$SECONDS
max_rss=0
while kill -0 "$probe_pid" 2>/dev/null; do
 if [[ -f /proc/$probe_pid/status ]]; then
  rss=$(awk '$1 == "VmRSS:" {print $2}' "/proc/$probe_pid/status" 2>/dev/null || true)
  rss=${rss:-0}
  (( rss > max_rss )) && max_rss=$rss
  if (( rss > 524288 || SECONDS-started > 120 )); then
   printf 'GUARD_ABORT seconds=%s rss_kib=%s\n' "$((SECONDS-started))" "$rss" >> "$log.guard"
   kill -TERM "$probe_pid" 2>/dev/null || true
   sleep 1
   kill -KILL "$probe_pid" 2>/dev/null || true
   break
  fi
 fi
 sleep 0.1
done
set +e
wait "$probe_pid"
status=$?
set -e
printf 'exit=%s duration_s=%s sampled_peak_rss_kib=%s\n' "$status" "$((SECONDS-started))" "$max_rss" >> "$log.guard"
trap - EXIT TERM INT
exit "$status"
