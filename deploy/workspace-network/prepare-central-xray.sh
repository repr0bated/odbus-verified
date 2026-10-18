#!/usr/bin/env bash
# Candidate preparation only. Never writes an existing file or starts Xray.
# INPUT_JSON PRIVATE_TRANSPORT_UUID_FILE DEDICATED_UDS_PATH NEW_PRIVATE_OUTPUT
set -euo pipefail
die() { printf 'central Xray proposal: %s\n' "$*" >&2; exit 1; }
[[ $# == 4 ]] || die 'usage: INPUT_JSON PRIVATE_TRANSPORT_UUID_FILE DEDICATED_UDS_PATH NEW_PRIVATE_OUTPUT'
input=$1 credential=$2 socket=$3 output=$4
for file in "$input" "$credential"; do
    [[ -f $file && ! -L $file ]] || die 'input must be a regular non-symlink file'
done
[[ $(stat -c %u "$credential") == "$(id -u)" && $(stat -c %a "$credential") == 600 ]] || die 'transport UUID input must be owned by caller and mode 0600'
transport_uuid=$(cat -- "$credential")
[[ $transport_uuid =~ ^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$ && $transport_uuid != 00000000-0000-0000-0000-000000000000 ]] || die 'invalid provided transport UUID'
[[ $socket == /* && $socket != *$'\n'* && $socket != *$'\r'* && $(realpath -m -- "$socket") == "$socket" && $socket != / ]] || die 'dedicated socket path must be absolute and canonical'
[[ ! -e $socket && ! -L $socket ]] || die 'dedicated socket pathname is already occupied'
[[ $output == /* && ! -e $output && ! -L $output && $(realpath -m -- "$output") == "$output" ]] || die 'output must be a new canonical absolute path'
parent=$(dirname -- "$output")
[[ -d $parent && ! -L $parent && $(realpath -e "$parent") == "$parent" && $(stat -c %u "$parent") == "$(id -u)" && $(stat -c %a "$parent") == 700 ]] || die 'output directory must be caller-owned mode 0700'
umask 077
stage=$(mktemp "$parent/.central-xray.XXXXXX")
trap 'rm -f -- "$stage"; unset transport_uuid' EXIT
# Pass the private UUID through stdin, not a command-line argument or log.
printf '%s' "$transport_uuid" | jq -R --slurp --slurpfile config "$input" --arg socket "$socket" '
    . as $id | $config[0] |
    "workspace-egress" as $tag |
    if ($config | length) != 1 or (.inbounds | type) != "array" or
       (.outbounds | type) != "array" or (.routing.rules | type) != "array" then
        error("unsupported canonical configuration shape")
    elif ([.outbounds[] | select(.tag == "od106-reality-egress" and .protocol == "vless")] | length) != 1 or
         ([.outbounds[] | select(.tag == "od106-reality-egress")] | length) != 1 then
        error("required unique existing REALITY VLESS outbound missing")
    elif any(.inbounds[]; .tag == $tag or .listen == $socket) or
         any(.routing.rules[]; (.inboundTag // [] | index($tag)) != null) then
        error("workspace tag or socket collision")
    elif any(.routing.rules[];
        .type != "field" or (.inboundTag | type) != "array" or (.inboundTag | length) == 0) then
        error("existing unscoped route could shadow appended workspace rule; operator review required")
    else
        .inbounds += [{tag:$tag,listen:$socket,protocol:"vless",
            settings:{clients:[{id:$id}],decryption:"none"},
            streamSettings:{network:"tcp",security:"none"}}] |
        .routing.rules += [{type:"field",inboundTag:[$tag],outboundTag:"od106-reality-egress"}]
    end
' > "$stage"
chmod 600 "$stage"
# An exclusive hard link publishes the complete candidate without overwriting
# an existing target, including one that appeared during preparation.
ln -- "$stage" "$output" || die 'candidate output collision'
