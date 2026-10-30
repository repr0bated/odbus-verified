#!/usr/bin/env bash
# Single-node policy regression. Expected to fail against the recorded baseline.
set -euo pipefail
binary=${1:?usage: check-numa.sh BINARY OUTPUT_LOG}
output=${2:?usage: check-numa.sh BINARY OUTPUT_LOG}
timeout -k 2 10 "$binary" numa > "$output" 2>&1
rg -q -F 'policy=Default apply=Ok(()) query_status=0 query_mode=0 query_mask=0' "$output"
rg -q -F 'policy=Preferred(0) apply=Ok(()) query_status=0 query_mode=1 query_mask=1' "$output"
rg -q -F 'policy=Bind([0]) apply=Ok(()) query_status=0 query_mode=2 query_mask=1' "$output"
rg -q -F 'policy=Interleave([0]) apply=Ok(()) query_status=0 query_mode=3 query_mask=1' "$output"
