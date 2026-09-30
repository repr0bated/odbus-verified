#!/usr/bin/env bash
set -euo pipefail
umask 077
repo=/srv/git/odbus-verified
report=$repo/reports/btrfs-cache-memory/20260930-implementation/cron
session=${LUNGS_SESSION:-b0c777e3-0955-cd8b-3d01-ca12998cae28}
tool=${1:?tool name}
arguments_file=${2:?arguments JSON file}
output=${3:?output JSON file}
arguments=$(jq -c . "$arguments_file")
coproc MCP { sudo -n timeout -k 5 90 /usr/local/bin/threetched-fs --store "$report/store" mcp --lungs-socket /run/opdbus/lungs.sock --lungs-session "$session" --load cron --load model_registry --load tched_router 2>"$output.stderr"; }
pid=$MCP_PID
exec {read_fd}<&"${MCP[0]}"
exec {write_fd}>&"${MCP[1]}"
old_read=${MCP[0]}
old_write=${MCP[1]}
exec {old_read}<&-
exec {old_write}>&-
cleanup() { exec {write_fd}>&-; exec {read_fd}<&-; wait "$pid" 2>/dev/null || true; }
trap cleanup EXIT
printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"opdbus-od95-registration","version":"1"}}}' >&"$write_fd"
while IFS= read -r -t 30 -u "$read_fd" line; do
 if jq -e '.id == 1' <<< "$line" >/dev/null; then break; fi
done
printf '%s\n' '{"jsonrpc":"2.0","method":"notifications/initialized"}' >&"$write_fd"
jq -cn --arg name "$tool" --argjson arguments "$arguments" '{jsonrpc:"2.0",id:2,method:"tools/call",params:{name:$name,arguments:$arguments}}' >&"$write_fd"
while IFS= read -r -t 60 -u "$read_fd" line; do
 if jq -e '.id == 2' <<< "$line" >/dev/null; then
  printf '%s\n' "$line" > "$output"
  jq -e 'has("result") and (.result.isError != true)' "$output" >/dev/null
  exit $?
 fi
done
printf 'No matching MCP response\n' >&2
exit 1
