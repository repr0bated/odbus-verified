#!/bin/sh
# install.sh — NIC-less BPF egress for one container (Incus or LXD).
#
#   sudo deploy/runit/ct-egress-bpf/install.sh <instance>
#
# Host: builds ct-egress-bpf + ct_egress.bpf.o on the host if absent
# (crates/op-network/bpf/build.sh), installs them under
# /usr/local/libexec/op-dbus/bin and /usr/local/lib/op-dbus, and enables
# the runit service ct-egress-bpf-<instance>. No Xray, no ct-egress user,
# no nft table, no host Unix proxy socket.
# Guest (Incus, when running): removes the retired socat/Xray egress — disables
# opdbus-egress-proxy / opdbus-egress-dns / opdbus-egress-nextdns units,
# deletes the apt proxy drop-in, /etc/profile.d/opdbus-egress.sh and the
# per-setting http(s)-proxy files — and points DNS at the loader stub:
# /etc/resolv.conf gains nameserver 127.0.0.2 (keeping an in-guest
# NextDNS 127.0.0.1 first where that service exists). No NIC, Incus/LXD
# device or mount is added. TCP egress needs nothing in the guest: the
# BPF program redirects connect() transparently.
set -eu
PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
export PATH
umask 022

die() { printf 'ct-egress-bpf-install: %s\n' "$*" >&2; exit 1; }
say() { printf 'ct-egress-bpf-install: %s\n' "$*"; }

[ $# -eq 1 ] || die "usage: $0 <instance>"
CT=$1
case $CT in '' | *[!A-Za-z0-9-]*) die "invalid instance name: $CT" ;; esac
[ "$(id -u)" = 0 ] || die "run as root"

HERE=$(CDPATH='' cd -- "$(dirname -- "$(readlink -f "$0")")" && pwd)
REPO=$(CDPATH='' cd -- "$HERE/../../.." && pwd)
STAMP=$(date -u +%Y%m%dT%H%M%SZ)

# --- host binaries ---------------------------------------------------
BIN=/usr/local/libexec/op-dbus/bin/ct-egress-bpf
OBJ=/usr/local/lib/op-dbus/ct_egress.bpf.o
if [ ! -x "$BIN" ] || [ ! -r "$OBJ" ]; then
    command -v clang >/dev/null || die "clang missing (needed to build the BPF egress)"
    [ -f "$REPO/crates/op-network/bpf/vmlinux.h" ] || command -v bpftool >/dev/null ||
        die "bpftool missing (needed once to generate vmlinux.h)"
    sh "$REPO/crates/op-network/bpf/build.sh" /tmp/ct-egress-bpf-build
    install -D -m 0755 /tmp/ct-egress-bpf-build/ct-egress-bpf "$BIN"
    install -D -m 0644 /tmp/ct-egress-bpf-build/ct_egress.bpf.o "$OBJ"
    say "built and installed ct-egress-bpf + ct_egress.bpf.o"
fi

# --- host service ------------------------------------------------------
put() { # MODE SRC DEST
    if [ -e "$3" ] && ! cmp -s "$2" "$3"; then
        cp -p "$3" "$3.before-ct-egress-bpf-$STAMP"; say "kept $3.before-ct-egress-bpf-$STAMP"
    fi
    install -D -o root -g root -m "$1" "$2" "$3"
}
put 0755 "$HERE/run" "/etc/sv/ct-egress-bpf-$CT/run"
put 0755 "$HERE/log/run" "/etc/sv/ct-egress-bpf-$CT/log/run"
[ -e "/etc/runit/runsvdir/default/ct-egress-bpf-$CT" ] ||
    ln -s "/etc/sv/ct-egress-bpf-$CT" "/etc/runit/runsvdir/default/ct-egress-bpf-$CT"

# retire the Xray proxy services, if a previous install left them
for svc in ct-egress-proxy "ct-egress-relay-$CT"; do
    if [ -L "/etc/runit/runsvdir/default/$svc" ]; then
        rm -f "/etc/runit/runsvdir/default/$svc"; say "disabled retired runit service $svc"
    fi
done

# --- guest cleanup (Incus guests only) ----------------------------------
# LXD guests are subsidiaries: every LXD step goes through the sealed lxd
# plugin, so this host script never enters one with raw `lxc exec`. Their
# root comes from a template without the retired carrier.
runner=
if [ -S /var/lib/incus/unix.socket ] && incus query "/1.0/instances/$CT" >/dev/null 2>&1; then runner=incus
fi
if [ -n "$runner" ]; then
    running=$($runner exec "$CT" -- true 2>/dev/null && echo yes || echo no)
    if [ "$running" = yes ]; then
        $runner exec "$CT" -- sh -eu -c '
            for u in opdbus-egress-proxy.service opdbus-egress-dns.service opdbus-egress-nextdns.service; do
                systemctl disable --now "$u" 2>/dev/null || true
                [ -e "/etc/systemd/system/$u" ] && mv "/etc/systemd/system/$u" "/etc/systemd/system/$u.before-ct-egress-bpf-'"$STAMP"'" || true
            done
            rm -f /etc/apt/apt.conf.d/95opdbus-egress-proxy /etc/profile.d/opdbus-egress.sh
            rm -f /etc/op-dbus/env/http-proxy /etc/op-dbus/env/https-proxy /etc/op-dbus/env/nextdns-bootstrap
            grep -vE "^(http_proxy|https_proxy|HTTP_PROXY|HTTPS_PROXY)=" /etc/environment > /etc/environment.tmp 2>/dev/null || true
            [ -s /etc/environment.tmp ] && mv /etc/environment.tmp /etc/environment || rm -f /etc/environment.tmp
            if [ -e /etc/systemd/system/nextdns.service ]; then
                printf "%s\n" "# DNS: in-guest NextDNS (sealed identity), fallback the BPF egress stub." "nameserver 127.0.0.1" "nameserver 127.0.0.2" "options edns0" > /etc/resolv.conf
            else
                printf "%s\n" "# DNS: BPF egress stub (host loader, 127.0.0.2:53). No NIC." "nameserver 127.0.0.2" "options edns0" > /etc/resolv.conf
            fi
            systemctl daemon-reload 2>/dev/null || true
        ' && say "guest $CT: retired socat/Xray egress, resolv.conf -> 127.0.0.2 stub" ||
            say "guest $CT: cleanup failed; host service still installed"
    else
        say "guest $CT not running; guest cleanup deferred to seed/provisioning state"
    fi
else
    say "no Incus instance $CT (LXD subsidiaries are not entered); host service installed only"
fi
say "installed ct-egress-bpf-$CT (host service enabled)"
