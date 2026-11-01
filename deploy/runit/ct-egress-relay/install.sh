#!/bin/sh
# install.sh — NIC-less socket egress for one Incus container.
#
#   sudo deploy/runit/ct-egress-relay/install.sh <instance>
#
# Host (base rootfs, never a release overlay): system user ct-egress,
# /etc/ct-egress/{xray.json,ct_egress.nft}, runit ct-egress-proxy and
# ct-egress-relay-<instance>, both enabled.
# Guest (inside the container): socat units on 127.0.0.1:3128 (HTTP proxy) and
# 127.0.0.2:53 (DNS), /etc/resolv.conf -> 127.0.0.2, proxy environment and the
# apt proxy. Adds no NIC, Incus device or mount. Every replaced file is kept
# beside itself as <file>.before-ct-egress-<stamp>.
set -eu
PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
export PATH
umask 022

die() { printf 'ct-egress-install: %s\n' "$*" >&2; exit 1; }
say() { printf 'ct-egress-install: %s\n' "$*"; }

[ $# -eq 1 ] || die "usage: $0 <instance>"
CT=$1
case $CT in '' | *[!A-Za-z0-9-]*) die "invalid instance name: $CT" ;; esac
[ "$(id -u)" = 0 ] || die "run as root"
for tool in incus jq nft setpriv socat svlogd chpst xray; do
    command -v "$tool" >/dev/null || die "missing $tool"
done

HERE=$(CDPATH= cd -- "$(dirname -- "$(readlink -f "$0")")" && pwd)
PROXY_SRC=$(CDPATH= cd -- "$HERE/../ct-egress-proxy" && pwd)
GUEST=$HERE/guest
STAMP=$(date -u +%Y%m%dT%H%M%SZ)

info=$(incus query "/1.0/instances/$CT?recursion=1") || die "no such instance: $CT"
[ "$(printf '%s' "$info" | jq -r .type)" = container ] || die "$CT is not a container"
[ "$(printf '%s' "$info" | jq -r '.expanded_config["security.privileged"] // "false"')" != true ] ||
    die "$CT is privileged"
[ "$(printf '%s' "$info" | jq -r .state.status)" = Running ] || die "$CT is not running"

# put MODE OWNER SRC DEST — host file, keeping a differing original.
put() {
    if [ -e "$4" ] && ! cmp -s "$3" "$4"; then
        cp -p "$4" "$4.before-ct-egress-$STAMP"
        say "kept $4.before-ct-egress-$STAMP"
    fi
    install -D -o "${2%:*}" -g "${2#*:}" -m "$1" "$3" "$4"
}

# push MODE SRC DEST — guest file, keeping a differing original (or symlink).
push() {
    if incus exec "$CT" -- test -L "$3"; then
        incus exec "$CT" -- mv "$3" "$3.before-ct-egress-$STAMP"
        say "kept $CT:$3.before-ct-egress-$STAMP"
    elif incus exec "$CT" -- test -e "$3" &&
        ! incus exec "$CT" -- cat "$3" | cmp -s - "$2"; then
        incus exec "$CT" -- cp -p "$3" "$3.before-ct-egress-$STAMP"
        say "kept $CT:$3.before-ct-egress-$STAMP"
    fi
    incus file push --create-dirs --mode "$1" --uid 0 --gid 0 "$2" "$CT$3" >/dev/null
}

# --- host ---------------------------------------------------------------
getent group ct-egress >/dev/null || groupadd --system ct-egress
getent passwd ct-egress >/dev/null ||
    useradd --system --gid ct-egress --no-create-home --home-dir /nonexistent \
        --shell /usr/sbin/nologin ct-egress

put 0640 root:ct-egress "$PROXY_SRC/xray.json" /etc/ct-egress/xray.json
put 0644 root:root "$PROXY_SRC/ct_egress.nft" /etc/ct-egress/ct_egress.nft
nft -c -f /etc/ct-egress/ct_egress.nft || die "nft rejects /etc/ct-egress/ct_egress.nft"
XRAY_LOCATION_ASSET=/usr/local/share/xray chpst -u ct-egress:ct-egress \
    xray run -test -config /etc/ct-egress/xray.json >/dev/null || die "xray rejects /etc/ct-egress/xray.json"

put 0755 root:root "$PROXY_SRC/run" /etc/sv/ct-egress-proxy/run
put 0755 root:root "$PROXY_SRC/log/run" /etc/sv/ct-egress-proxy/log/run
put 0755 root:root "$HERE/run" "/etc/sv/ct-egress-relay-$CT/run"
put 0755 root:root "$HERE/log/run" "/etc/sv/ct-egress-relay-$CT/log/run"

# --- guest --------------------------------------------------------------
tmp=$(mktemp -d)
trap 'rm -f "$tmp/environment" "$tmp/profile.sh"; rmdir "$tmp"' EXIT
incus exec "$CT" -- cat /etc/environment 2>/dev/null |
    grep -v -E '^(# opdbus-egress:|(http|https|no)_proxy=|(HTTP|HTTPS|NO)_PROXY=)' >"$tmp/environment" || :
cat "$GUEST/environment" >>"$tmp/environment"
{
    sed -n '1p' "$GUEST/environment"
    sed -n 's/^\([A-Za-z_][A-Za-z_]*=.*\)$/export \1/p' "$GUEST/environment"
} >"$tmp/profile.sh"

push 0644 "$GUEST/opdbus-egress-proxy.service" /etc/systemd/system/opdbus-egress-proxy.service
push 0644 "$GUEST/opdbus-egress-dns.service" /etc/systemd/system/opdbus-egress-dns.service
push 0644 "$GUEST/resolv.conf" /etc/resolv.conf
push 0644 "$tmp/environment" /etc/environment
push 0644 "$tmp/profile.sh" /etc/profile.d/opdbus-egress.sh
push 0644 "$GUEST/95opdbus-egress-proxy" /etc/apt/apt.conf.d/95opdbus-egress-proxy
incus exec "$CT" -- systemctl daemon-reload
incus exec "$CT" -- systemctl enable --now opdbus-egress-proxy.service opdbus-egress-dns.service

# --- enable host services ----------------------------------------------
for svc in ct-egress-proxy "ct-egress-relay-$CT"; do
    [ -e "/etc/runit/runsvdir/default/$svc" ] || ln -s "/etc/sv/$svc" "/etc/runit/runsvdir/default/$svc"
done
say "installed for $CT; host services ct-egress-proxy and ct-egress-relay-$CT enabled"
