#!/bin/bash
# Stage a verified official dependency in the writable Golden build OS only.
# The following full Golden build records its executable hash; activation owns
# projection into the live host. This script never installs or starts a service.
set -euo pipefail
[[ $EUID == 0 ]] || { echo 'root is required for build-OS preparation'; exit 77; }
P=${1:?verified unpacked Xray release directory required}
PIN=$(dirname -- "$0")/artifact.json
WORK=/mnt/@opdbus-system-a
TARGET=$WORK/opt/op-build/target/x86_64-unknown-linux-gnu/release
[[ -d $TARGET && -f $P/release.zip && -f $P/xray && ! -L $P/xray ]]
if ps -eo comm= | grep -Eq '^(cargo|rustc|clippy-driver)$'; then
    echo 'another Cargo owner is active'; exit 75
fi
sha=$(jq -er .archive_sha256 "$PIN")
printf '%s  %s\n' "$sha" "$P/release.zip" | sha256sum -c -
sha=$(jq -er .binary_sha256 "$PIN")
printf '%s  %s\n' "$sha" "$P/xray" | sha256sum -c -
for asset in geoip.dat geosite.dat; do
    sha=$(jq -er --arg asset "$asset" '.assets[$asset]' "$PIN")
    printf '%s  %s\n' "$sha" "$P/$asset" | sha256sum -c -
done
install -d -m 0755 "$WORK/usr/local/share/xray" "$WORK/usr/share/op-dbus/egress"
install -m 0755 "$P/xray" "$TARGET/xray"
install -m 0644 "$P/geoip.dat" "$P/geosite.dat" "$P/LICENSE" "$WORK/usr/local/share/xray/"
install -m 0644 "$PIN" "$WORK/usr/share/op-dbus/egress/xray.json"
printf 'exit=0\nphase=build_os_dependency_prepared\nfinished_at=%s\n' "$(date -u +%FT%TZ)"
