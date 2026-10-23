#!/usr/bin/env bash
# All Incus and ownership observations are stubs; files live in a temp fixture.
set -euo pipefail
here=$(cd "$(dirname "$0")/.." && pwd)
fixture=$(mktemp -d)
trap 'rm -rf -- "$fixture"' EXIT
mkdir -p "$fixture/bin" "$fixture/host"/{skills,rules,user,shared,soul,memory}
# Substitute only the fixed directory into a disposable script, leaving the
# production helper untouched and never examining or modifying live /etc.
sed "s|/etc/3tched|$fixture/host|g" "$here/../runit/libexec-3tched/attach-host-workspace-folders" > "$fixture/helper"
export FIXTURE=$fixture
cat > "$fixture/bin/id" <<'STUB'
#!/bin/sh
printf '0\n'
STUB
cat > "$fixture/bin/stat" <<'STUB'
#!/bin/sh
case "$2" in
%u) printf '0\n';;
%a) if [ "${UNSAFE_HOST:-0}" = 1 ] && [ "$4" = "$FIXTURE/host/skills" ]; then printf '777\n'; else printf '755\n'; fi;;
*) exit 90;;
esac
STUB
cat > "$fixture/bin/incus" <<'STUB'
#!/bin/bash
set -euo pipefail
case "$1" in
query) cat "$FIXTURE/native";;
config)
 [[ $2 == device && $3 == add && $4 == testct && $5 == 3tched-config && $6 == disk ]]
 [[ $7 == "source=$FIXTURE/host" && $8 == "path=$FIXTURE/host" && $9 == readonly=true && ${10} == shift=true ]]
 printf 'add\n' >> "$FIXTURE/events"
 jq --arg host "$FIXTURE/host" '.devices["3tched-config"]={type:"disk",source:$host,path:$host,readonly:"true",shift:"true"} | .expanded_devices=.devices' "$FIXTURE/native" > "$FIXTURE/native.tmp"
 mv "$FIXTURE/native.tmp" "$FIXTURE/native";;
*) exit 91;;
esac
STUB
chmod 755 "$fixture/bin/"*
export PATH="$fixture/bin:$PATH"
reset() {
 printf '{"name":"testct","status":"Stopped","type":"container","config":{"volatile.uuid":"instance-pin"},"expanded_config":{},"devices":{"root":{"type":"disk","path":"/","pool":"identity-roots"}},"expanded_devices":{"root":{"type":"disk","path":"/","pool":"identity-roots"}}}' > "$fixture/native"
 : > "$fixture/events"
}
reject() {
 if bash "$fixture/helper" testct > "$fixture/output" 2>&1; then echo 'unsafe attachment accepted' >&2; exit 1; fi
 [[ ! -s $fixture/events ]]
}
reset
bash "$fixture/helper" testct > "$fixture/output"
bash "$fixture/helper" testct > "$fixture/output"
[[ $(wc -l < "$fixture/events") == 1 ]]
for alteration in '.status="Running"' '.expanded_config["security.privileged"]="true"' \
 '.devices["3tched-config"]={type:"disk",source:"/wrong",path:"/wrong"}' \
 '.expanded_devices.parent={type:"disk",source:"/wrong",path:"/etc"}'; do
 reset
 jq "$alteration" "$fixture/native" > "$fixture/native.tmp"; mv "$fixture/native.tmp" "$fixture/native"
 reject
done
for flag in readonly shift; do
 reset
 jq --arg host "$fixture/host" --arg flag "$flag" '
   .devices["3tched-config"]={type:"disk",source:$host,path:$host,readonly:"true",shift:"true"} |
   .devices["3tched-config"][$flag]="false" | .expanded_devices=.devices
 ' "$fixture/native" > "$fixture/native.tmp"
 mv "$fixture/native.tmp" "$fixture/native"; reject
done
reset
jq --arg path "$fixture/host" '.expanded_devices.conflict={type:"disk",source:"/wrong",path:$path}' "$fixture/native" > "$fixture/native.tmp"
mv "$fixture/native.tmp" "$fixture/native"; reject
reset
export UNSAFE_HOST=1; reject; unset UNSAFE_HOST
reset
mv "$fixture/host/skills" "$fixture/host/other"
ln -s other "$fixture/host/skills"; reject
printf 'PASS: stopped-only exact readonly/shift disk, idempotence, conflicts and host path guards\n'
