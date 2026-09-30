#!/usr/bin/env bash
set -euo pipefail
cd /srv/git/odbus-verified
report=reports/btrfs-cache-memory/20260930-followup
for repeat in 1 2 3; do
 for spec in warm:4096 warm:65536 kv-warm:65536 overwrite:65536 clear:65536 construction:65536 burst:65536 growth:65536 negative:65536 retention:65536 expiry:65536 expiry-mitigated:65536; do
  printf '%s run-one %s %s %s\n' "$(date -u +%FT%TZ)" "${spec%:*}" "${spec#*:}" "$repeat"
  timeout --signal=TERM --kill-after=5s 125s "$report/run-one.sh" "${spec%:*}" "${spec#*:}" "$repeat"
 done
done
