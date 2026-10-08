#!/bin/sh
# Rolling release retention and cache age limit.
# Keeps the selected release plus the two newest other received releases.
# Removes cache files older than two days. Does not touch golden, recovery,
# identity data, or an in-progress release build.
set -eu

releases=${OPDBUS_RELEASES:-/opt/op-dbus/releases}
current_link=${OPDBUS_CURRENT:-/opt/op-dbus/current}
keep_revisions=2
cache_days=2
cache_roots="/var/cache/op-dbus /srv/git/odbus-verified/target/debug/incremental"

current=$(readlink -f "$current_link")
[ -d "$current" ] || { printf '%s\n' "current release is not a directory: $current" >&2; exit 1; }
case "$current" in
    "$releases"/*) ;;
    *) printf '%s\n' "current release is outside $releases: $current" >&2; exit 1 ;;
esac

printf '%s\n' "keep $current (current)"
listing=$(mktemp)
trap 'rm -f "$listing"' EXIT HUP INT TERM
stat -c '%Y %n' "$releases"/*/ | sort -nr > "$listing"
kept_extra=0
while read -r _ path; do
    path=${path%/}
    path=$(readlink -f "$path")
    [ "$path" = "$current" ] && continue
    [ -d "$path" ] || continue
    case "$path" in
        "$releases"/*) ;;
        *) continue ;;
    esac
    if [ "$kept_extra" -lt "$keep_revisions" ]; then
        printf '%s\n' "keep $path"
        kept_extra=$((kept_extra + 1))
        continue
    fi
    printf '%s\n' "retire $path"
    if btrfs subvolume show "$path" >/dev/null 2>&1; then
        # Received releases stay read-only until retirement. -f is required
        # to clear that flag on a subvolume that has received_uuid.
        btrfs property set -f "$path" ro false
        btrfs subvolume delete "$path"
    else
        rm -rf -- "$path"
    fi
done < "$listing"

for root in $cache_roots; do
    [ -d "$root" ] || continue
    find "$root" -xdev -mindepth 1 -mtime +"$cache_days" -depth -print -delete || true
done
