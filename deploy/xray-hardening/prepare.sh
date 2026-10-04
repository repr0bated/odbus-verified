#!/bin/bash
# Offline candidate preparation only; never installs or reloads Xray.
set -euo pipefail
umask 077
fail() { printf 'xray-hardening: %s\n' "$*" >&2; exit 1; }
[[ $# == 4 ]] || fail 'usage: prepare.sh PROFILE INPUT.json PRIVATE-PARAMS.json NEW-CANDIDATE.json'
profile=$1 input=$2 params=$3 output=$4
case $profile in host-reality|decoy-reality|host-xhttp|decoy-xhttp) ;; *) fail 'unknown profile';; esac
[[ -f $input && -f $params ]] || fail 'input and private parameter files required'
[[ ! -e $output && ! -L $output ]] || fail 'output must not already exist'
output=$(realpath -m -- "$output")
case $output in /etc/xray/.od106-prepared-20261004/*) ;; /etc/*|/dev/*|/proc/*|/sys/*) fail 'output must be an offline preparation path';; esac
[[ -d $(dirname -- "$output") ]] || fail 'output directory must exist'
# Reserve the destination without replacing any existing file (including symlinks).
(set -o noclobber; : > "$output") 2>/dev/null || fail 'cannot reserve new output'
tmp=
completed=false
cleanup() { if [[ -n $tmp ]]; then rm -f -- "$tmp"; fi; if [[ $completed == false ]]; then rm -f -- "$output"; fi; }
trap cleanup EXIT
tmp=$(mktemp "${output}.tmp.XXXXXX")
# jq errors intentionally contain only static diagnoses, never input values.
jq -e -s --arg profile "$profile" --slurpfile parameters "$params" -f "$(dirname -- "$0")/transform.jq" "$input" > "$tmp"
jq -e 'type == "object"' "$tmp" >/dev/null
mv -- "$tmp" "$output"
completed=true
printf 'Prepared offline candidate: %s\n' "$output"
