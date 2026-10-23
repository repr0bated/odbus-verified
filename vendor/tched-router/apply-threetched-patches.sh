#!/bin/sh
set -eu

package_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
source_dir=${1:?usage: $0 SOURCE-DIR}
. "$package_dir/source.pin"
pin=$UPSTREAM_COMMIT

fail() { echo "tched-router patch preflight: $*" >&2; exit 2; }

test -f "$source_dir/Cargo.toml" || fail "missing Cargo.toml"
test -f "$source_dir/crates/zeroclaw-gateway/src/sse.rs" || fail "missing pinned gateway source"
if [ -e "$source_dir/.git" ] || [ -L "$source_dir/.git" ]; then
    actual=$(git -C "$source_dir" rev-parse HEAD) || fail "cannot read git pin"
    test "$actual" = "$pin" || fail "expected commit $pin, found $actual"
fi

for path in $(sed -n '/^# Existing files/,/^# New files/{/^#/d;p;}' "$package_dir/patch-inventory"); do
    test -f "$source_dir/$path" || fail "missing existing path $path"
done
(cd "$source_dir" && sha256sum -c "$package_dir/base-sha256sums" --quiet) || fail "base source hash mismatch"
for path in $(sed -n '/^# New files/,$ {/^#/d;p;}' "$package_dir/patch-inventory"); do
    test ! -e "$source_dir/$path" && test ! -L "$source_dir/$path" || fail "new path already exists: $path"
done

patch --batch --fuzz=0 -d "$source_dir" -p1 < "$package_dir/patches/0001-close-threetched-event-stream-on-lag.patch"
patch --batch --fuzz=0 -d "$source_dir" -p1 < "$package_dir/patches/0002-stateless-provider-generation.patch"
patch --batch --fuzz=0 -d "$source_dir" -p1 < "$package_dir/patches/0003-threetched-branding.patch"
