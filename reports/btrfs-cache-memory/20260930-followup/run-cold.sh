#!/usr/bin/env bash
set -euo pipefail
cd /srv/git/odbus-verified
report=reports/btrfs-cache-memory/20260930-followup
for repeat in 1 2 3; do
 timeout --signal=TERM --kill-after=5s 125s "$report/run-one.sh" cold-corruption 65536 "$repeat"
done
