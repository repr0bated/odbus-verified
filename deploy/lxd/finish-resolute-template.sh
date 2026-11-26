#!/bin/bash
# Complete an unbooted, private Ubuntu root; never edit an existing image/seed.
set -euo pipefail
umask 022
REPO=$(cd -- "$(dirname -- "$0")/../.." && pwd)
W=${1:?prepared Ubuntu build directory required}
OUT=${2:?new image archive required}
UI=${3:?built browser UI directory required (kiroween ui/build)}
[[ -f $UI/index.html && -d $UI/assets ]]
R=$W/img/rootfs
BUILD=/mnt/@opdbus-system-a/opt/op-build
[[ -d $R && ! -L $R && ! -e $OUT && ! -L $OUT && ! -e $OUT.part ]]
grep -qx 'VERSION_CODENAME=resolute' "$R/usr/lib/os-release"
[[ ! -e $R/agent/user/issued_identity.iid1 && ! -e $R/agent/user/sealed_id ]]
# A mounted or booted root is not a template preparation target.
if findmnt -rn -o TARGET | awk -v root="$R" '$0==root || index($0,root"/")==1 {found=1} END {exit !found}'; then
    echo 'refusing a mounted template root' >&2; exit 78
fi
jq -e '.exit == 0 and (.source_commit | test("^[0-9a-f]{40}$"))' "$BUILD/execution-receipt.json" >/dev/null
for name in zcall op-guest-backend op-identity-accept op-tunnel-door op-grpc-bridge; do
    sha=$(jq -er --arg name "$name" '.artifacts[$name] | select(test("^[0-9a-f]{64}$"))' "$BUILD/execution-receipt.json")
    bin=$BUILD/target/x86_64-unknown-linux-gnu/release/$name
    printf '%s  %s\n' "$sha" "$bin" | sha256sum -c -
    dest=$R/usr/local/bin/$name
    [[ $name != op-guest-backend ]] || dest=$R/usr/local/libexec/op-dbus/$name
    [[ $name != op-tunnel-door && $name != op-grpc-bridge ]] || dest=$R/usr/local/libexec/op-dbus/bin/$name
    install -D -m 0755 "$bin" "$dest"
    printf '%s  %s\n' "$sha" "$dest" | sha256sum -c -
done
version=1.50.2
sha=df65a100940decdd564a37024a8261be566a629c9747a909f70421c616a2575c
install -d -m 0700 "$W/dl"
if [[ ! -f $W/dl/nextdns.tar.gz ]]; then
    curl --proto '=https' --tlsv1.2 --connect-timeout 15 --max-time 180 -fLsS \
        "https://github.com/nextdns/nextdns/releases/download/v$version/nextdns_${version}_linux_amd64.tar.gz" -o "$W/dl/nextdns.tar.gz"
fi
printf '%s  %s\n' "$sha" "$W/dl/nextdns.tar.gz" | sha256sum -c -
tar -xzf "$W/dl/nextdns.tar.gz" -C "$W/dl" nextdns
install -m 0755 "$W/dl/nextdns" "$R/usr/bin/nextdns"
install -d -m 0755 "$R/usr/local/libexec/op-dbus" "$R/etc/profile.d" "$R/etc/systemd/system/nextdns.service.d" "$R/etc/systemd/system/multi-user.target.wants"
install -d -m 0700 "$R/etc/op-dbus/env" "$R/etc/op-dbus/secrets"
for name in guest-env nextdns-run; do
    install -m 0755 "$REPO/deploy/seed/rootfs/usr/local/libexec/op-dbus/$name" "$R/usr/local/libexec/op-dbus/$name"
done
install -m 0644 "$REPO/deploy/config/profile.d/op-dbus-env.sh" "$R/etc/profile.d/op-dbus-env.sh"
install -m 0644 /etc/op-dbus/decoy-trust.json "$R/etc/op-dbus/decoy-trust.json"
# NIC-less BPF egress (owner 2026-11-23): no in-guest relay at all. The
# host loader (ct-egress-bpf-<instance>) serves DNS on 127.0.0.2:53 in
# this netns and redirects TCP connect() transparently; the guest only
# needs resolv.conf. No Xray/socat units, no apt proxy, no proxy env.
# Same DNS shape as every seed: in-guest NextDNS (sealed identity)
# first, the BPF stub as fallback; nextdns's own DoH TCP is carried by
# the BPF egress like any other flow. A prepared root may already carry
# the retired Xray/socat carrier from an earlier finish; remove all of it.
rm -f "$R"/etc/systemd/system/opdbus-egress-{proxy,dns,nextdns}.service \
    "$R"/etc/systemd/system/multi-user.target.wants/opdbus-egress-{proxy,dns,nextdns}.service \
    "$R/etc/systemd/system/nextdns.service.d/20-egress.conf" \
    "$R/etc/apt/apt.conf.d/95opdbus-egress-proxy" "$R/etc/profile.d/opdbus-egress.sh" \
    "$R/etc/op-dbus/env/nextdns-bootstrap"
install -m 0644 "$REPO/deploy/seed/rootfs/etc/systemd/system/nextdns.service" "$R/etc/systemd/system/nextdns.service"
install -m 0755 "$REPO/deploy/provisioning/runit/op-tunnel-door/run" "$R/usr/local/libexec/op-dbus/tunnel-door-run"
install -m 0644 "$REPO/deploy/seed/rootfs/etc/systemd/system/op-tunnel-door.service" "$R/etc/systemd/system/op-tunnel-door.service"
ln -sfn ../op-tunnel-door.service "$R/etc/systemd/system/multi-user.target.wants/op-tunnel-door.service"
# The container's own door (owner 2026-11-25): the bridge serves the browser UI on
# 10.0.0.2:8443 over this container's own 3tchedFS, which the host projects into
# /run with the sealed catalog beside it (deploy/runit/subsidiary-*-3tchedfs).
L=$REPO/deploy/lxd/rootfs S=$REPO/deploy/seed/rootfs
install -d -m 0755 "$R/etc/tmpfiles.d" "$R/etc/sysusers.d" "$R/usr/local/share/op-browser-ui"
install -m 0755 "$S/usr/local/libexec/op-dbus/door-tls-init" "$R/usr/local/libexec/op-dbus/door-tls-init"
for unit in opdbus-lo door-tls-init; do
    install -m 0644 "$S/etc/systemd/system/$unit.service" "$R/etc/systemd/system/$unit.service"
done
install -m 0644 "$L/etc/systemd/system/op-grpc-bridge.service" "$R/etc/systemd/system/op-grpc-bridge.service"
install -m 0644 "$L/etc/op-dbus/bridge.env" "$R/etc/op-dbus/bridge.env"
install -m 0644 "$L/etc/tmpfiles.d/opdbus-runtime.conf" "$R/etc/tmpfiles.d/opdbus-runtime.conf"
install -m 0644 "$S/etc/sysusers.d/opdbus.conf" "$R/etc/sysusers.d/opdbus.conf"
for unit in opdbus-lo door-tls-init op-grpc-bridge; do
    ln -sfn "../$unit.service" "$R/etc/systemd/system/multi-user.target.wants/$unit.service"
done
printf '%s\n' 10.0.0.2:8443 >"$R/etc/op-dbus/env/OP_UI_LISTEN"
printf '%s\n' /usr/local/share/op-browser-ui >"$R/etc/op-dbus/env/CHAT_UI_DIR"
chmod 0600 "$R/etc/op-dbus/env/OP_UI_LISTEN" "$R/etc/op-dbus/env/CHAT_UI_DIR"
cp -R --no-preserve=ownership "$UI/." "$R/usr/local/share/op-browser-ui/"
install -m 0644 "$REPO/deploy/seed/rootfs/etc/resolv.conf" "$R/etc/resolv.conf"
ln -sfn "../nextdns.service" "$R/etc/systemd/system/multi-user.target.wants/nextdns.service"
# gRPC-ready client config (all seeds/containers): env store defaults —
# local/door endpoints never proxied, system CA bundle for TLS stacks.
# No http(s)-proxy settings anywhere: outbound gRPC dials directly and
# the BPF egress carries it. (Env files are gitignored as a class, so
# they are written here explicitly, not copied from the repo tree.)
install -m 0644 "$REPO/deploy/seed/rootfs/etc/profile.d/op-dbus-env.sh" "$R/etc/profile.d/op-dbus-env.sh"
install -d -m 0755 "$R/etc/op-dbus/env"
printf '%s\n' 'localhost,127.0.0.1,::1,10.0.0.2' >"$R/etc/op-dbus/env/no-proxy"
printf '%s\n' '/etc/ssl/certs/ca-certificates.crt' >"$R/etc/op-dbus/env/ssl-cert-file"
rm -f "$R/etc/op-dbus/env/http-proxy" "$R/etc/op-dbus/env/https-proxy"
ln -sfn /lib/systemd/system/multi-user.target "$R/etc/systemd/system/default.target"
awk -F: 'length($2)>3{print "password set: "$1; b=1}END{exit b}' "$R/etc/shadow"
install -d "$W/img/templates"
printf '{{ container.name }}\n' >"$W/img/templates/hostname.tpl"
printf '127.0.0.1 localhost\n127.0.1.1 {{ container.name }}\n::1 localhost ip6-localhost ip6-loopback\n' >"$W/img/templates/hosts.tpl"
ZCALL_SHA=$(jq -er '.artifacts.zcall' "$BUILD/execution-receipt.json")
GUEST_BACKEND_SHA=$(jq -er '.artifacts["op-guest-backend"]' "$BUILD/execution-receipt.json")
COMMIT=$(jq -er .source_commit "$BUILD/execution-receipt.json")
cat >"$W/img/metadata.yaml" <<Y
architecture: x86_64
creation_date: $(date +%s)
properties:
  description: Ubuntu 26.04 LTS identity-bound LXC template
  os: ubuntu
  release: "26.04"
  codename: resolute
  opdbus.zcall.sha256: "$ZCALL_SHA"
  opdbus.guest-backend.sha256: "$GUEST_BACKEND_SHA"
  opdbus.source.commit: "$COMMIT"
templates:
  /etc/hostname:
    when: [create, copy]
    template: hostname.tpl
  /etc/hosts:
    when: [create, copy]
    template: hosts.tpl
Y
install -m 0644 "$BUILD/execution-receipt.json" "$R/usr/local/libexec/op-dbus/template-build-receipt.json"
tar --numeric-owner --xattrs --xattrs-include='*' --acls -C "$W/img" -cf - metadata.yaml templates rootfs | zstd -q -T2 -10 -o "$OUT.part"
chown 0:0 "$OUT.part"; chmod 600 "$OUT.part"; mv "$OUT.part" "$OUT"
sha256sum "$OUT"
printf 'exit=0\nphase=image_prepared\nsource_commit=%s\nfinished_at=%s\n' "$COMMIT" "$(date -u +%FT%TZ)" >"$W/image.receipt"
sync -f "$W/image.receipt"
