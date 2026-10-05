#!/bin/sh
# Exercise fresh-guest hostname preparation without Incus or storage effects.
set -eu
repo=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
helper=$repo/deploy/runit/libexec-3tched/op-identity-root
fixture=$(mktemp -d)
trap 'rm -rf -- "$fixture"' EXIT
guest=$fixture/guest
mkdir -p "$guest/etc"
seed_session=b0c777e3-0955-cd8b-3d01-ca12998cae28
seed_fail() { printf '%s\n' "$*" >&2; exit 1; }
valid_session() { printf '%s' "$1" | grep -Eq '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'; }
# The fixture is owned by the test user; production additionally verifies all
# ancestors are root-controlled. Preserve the symlink check in this stub.
seed_secure_path() { [ ! -L "$1" ] || seed_fail 'symlink parent'; }
eval "$(sed -n '/^seed_write_hostname() {/,/^}/p' "$helper")"
printf 'seed-hostname\n' > "$guest/etc/hostname"
seed_write_hostname
[ "$(cat "$guest/etc/hostname")" = "$seed_session" ]
[ "$(stat -c %a "$guest/etc/hostname")" = 644 ]
seed_session=not-a-session
if (seed_write_hostname) 2>/dev/null; then exit 1; fi
[ "$(cat "$guest/etc/hostname")" = b0c777e3-0955-cd8b-3d01-ca12998cae28 ]
seed_session=414fbc56-03fb-20ab-758a-bcc7a7cbdc0e
printf 'untouched\n' > "$fixture/outside"
rm "$guest/etc/hostname"
ln -s "$fixture/outside" "$guest/etc/hostname"
if (seed_write_hostname) 2>/dev/null; then exit 1; fi
[ "$(cat "$fixture/outside")" = untouched ]
rm "$guest/etc/hostname"
seed_write_hostname
[ "$(cat "$guest/etc/hostname")" = "$seed_session" ]
mv "$guest/etc" "$fixture/other-etc"
ln -s "$fixture/other-etc" "$guest/etc"
if (seed_write_hostname) 2>/dev/null; then exit 1; fi
printf '%s\n' 'PASS: session hostname written only beneath the fresh guest root'
