#!/usr/bin/env bash
set -euo pipefail
cd /srv/git/odbus-verified
report=reports/btrfs-cache-memory/20260930-followup
printf '%s\n' 'file	cycles	live_cycle1	live_cycle10	live_delta_1_10	live_slope_bytes_per_cycle	rss_cycle1_kib	rss_cycle10_kib	max_checkpoint_rss_kib	anon_cycle1_kib	anon_cycle10_kib	fds_min	fds_max	threads_min	threads_max	children_max	wall_1_10_s	cpu_1_10_s	latency_cycle_p50_min_ns	latency_cycle_p50_max_ns	latency_cycle_p95_max_ns	live_after_clear	live_after_drop' | sed 's/\\t/\t/g'
for file in "$report"/raw/*.jsonl; do jq -s -r --arg filename "${file##*/}" -f "$report/summarize.jq" "$file"; done
