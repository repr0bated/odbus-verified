#!/usr/bin/env bash
# Incus API contract fixture only; never creates host /run directories or an Incus profile.
set -euo pipefail
repo=$(cd "$(dirname "$0")/../../.." && pwd)
helper=$repo/deploy/runit/libexec-3tched/op-identity-root
fixture=$(mktemp -d)
trap 'rm -rf -- "$fixture"' EXIT
seed_fail() { printf '%s\n' "$*" >&2; exit 1; }
eval "$(sed -n '/^seed_profile_validate_json() {/,/^}/p' "$helper")"
eval "$(sed -n '/^seed_profile() (/,/^)/p' "$helper")"
# Keep this fixture away from the live host paths while exercising Incus API behavior.
seed_profile_dir() { printf '%s\n' "$1" >> "$fixture/dirs"; }
id() { [[ $1 == -u ]] && { printf '0\n'; return; }; command id "$@"; }
expected=$(jq -cn '{name:"3tched-lungs",description:"",config:{},devices:{"runtime-run":{type:"disk",source:"tmpfs:",path:"/run","initial.mode":"0755"},"lungs-socket-dir":{type:"disk",source:"/run/opdbus/lungs",path:"/run/opdbus/lungs",readonly:"true"}}}')
posts_file=$fixture/posts
profile_file=$fixture/profile.json
: > "$posts_file"
incus() {
    [[ $1 == query ]] || return 90
    if [[ $2 == '/1.0/profiles?recursion=1' ]]; then
        if [[ -f $profile_file ]]; then jq -cn --slurpfile p "$profile_file" '$p'; else printf '[]\n'; fi
    elif [[ $2 == -X && $3 == POST && $4 == /1.0/profiles && $5 == -d ]]; then
        printf 'post\n' >> "$posts_file"
        printf '%s\n' "$6" > "$profile_file"
        printf '{"type":"sync","status":"Success"}\n'
    elif [[ $2 == /1.0/profiles/3tched-lungs && -f $profile_file ]]; then
        cat "$profile_file"
    else
        return 91
    fi
}
seed_profile
[[ $(wc -l < "$posts_file") == 1 && $(jq -cS . "$profile_file") == $(jq -cS . <<<"$expected") ]]
[[ $(wc -l < "$fixture/dirs") == 3 ]]
seed_profile
[[ $(wc -l < "$posts_file") == 1 ]] # exact existing profile is accepted without rewriting
jq -c '.devices["lungs-socket-dir"].readonly="false"' "$profile_file" > "$fixture/drift.json"
mv "$fixture/drift.json" "$profile_file"
if seed_profile >/dev/null 2>&1; then
    printf 'accepted drifted shared profile\n' >&2
    exit 1
fi
[[ $(wc -l < "$posts_file") == 1 ]]
printf 'PASS: exact profile create/readback, idempotence and drift refusal; no live effects\n'
