#!/bin/sh
# fuse-swap.sh: swap a host 3tchedFS view (3tched-fs / threetched-fs runit
# service driven by opdbus-fuse-bootstrap/mount-3tchedfs) to a new 3tched-fs
# binary by rewriting the service's 0600 conf pin (OPDBUS_FUSE_BINARY +
# OPDBUS_FUSE_BINARY_SHA256). The adapter refuses symlinked binaries, so the
# golden symlink flip does not apply here. Dry-run unless --apply.
#
#   fuse-swap.sh <service-dir> <3tchedFS|threetched> <new-binary> [--apply|--transaction|--pin-down]
#
# --pin-down: for an administratively down, unmounted view only. Rewrites the
# conf pin exactly as a swap does, never starts the service and never removes
# its down file; mount health is unverifiable while it stays down.
#
# Health (fuse_health): mount is fuse.3tchedFS with the view's source, its
# .3tchedfs.json and view.json match the capture and the sealed catalog, and
# operations are listed; then the runsv pid must stay the same for 10s.
# The installed ./check also walks .methods in each schema.json, which the
# mounted documents do not carry, so it fails for every binary; `sv restart`
# blocks on it, hence the bounded restart below. Standalone failure restores
# its prior conf; --transaction leaves restore to the complete catalog owner.
set -eu
PATH=/usr/sbin:/usr/bin:/sbin:/bin
D=${1:?service dir}; view=${2:?view}; new=${3:?new binary}; apply=${4:-}
case "$view" in 3tchedFS) surface=3tched ;; threetched) surface=threetched ;; *) echo "unknown view $view" >&2; exit 2 ;; esac
log() { printf 'fuse-swap[%s]: %s\n' "${D##*/}" "$*"; }

fuse_health() {
    m=/run/mounts/$view; r=/run/opdbus/host-fuse/$view; c=/dev/shm/opdbus/plugin-blobs
    [ "$(findmnt -rn -M "$m" -o FSTYPE)" = fuse.3tchedFS ] || return 1
    [ "$(findmnt -rn -M "$m" -o SOURCE)" = "$view" ] || return 1
    timeout 10 jq -e --arg surface "$surface" --arg catalog "$c" --argjson identity "$identity" --slurpfile captured "$r/capture.json" '
      .format == "3tchedfs/1" and .mount_identity == $identity and .workspace_id == null and
      .surface == $surface and .declared_catalog_dir == $catalog and .view_id == $captured[0].view_id and
      .source_catalog_hash == $captured[0].source_catalog_hash and
      .source_catalog_generation == $captured[0].source_catalog_generation' "$m/.3tchedfs.json" >/dev/null || return 1
    timeout 10 jq -e --arg name "$view" --slurpfile catalog "$c/.manifest.json" '
      (.source_catalog_hash == $catalog[0].catalog_hash and .source_catalog_generation == $catalog[0].generation) and
      ((.objects | keys | sort) == (if $name == "threetched" then ["threetched"]
         else (($catalog[0].plugins | keys | sort) - ["threetched"]) end))' "$m/view.json" >/dev/null || return 1
    n=$(timeout 150 sh -c 'ls -d "$1"/plugins/*/operations/* 2>/dev/null' _ "$m" | wc -l)
    [ "$n" -gt 0 ]
}
svpid() { sv status "$D" | sed -n 's/^run: [^(]*(pid \([0-9]*\)).*/\1/p'; }
wait_healthy() {
    expected=$1
    i=0
    while [ "$i" -lt 40 ]; do
        if fuse_health; then
            p1=$(svpid); sleep 10; p2=$(svpid)
            if [ -n "$p1" ] && [ "$p1" = "$p2" ] &&
                { [ -z "$expected" ] || [ "/proc/$p2/exe" -ef "$expected" ]; } &&
                fuse_health; then
                return 0
            fi
        fi
        sleep 3; i=$((i + 1))
    done
    return 1
}
restart() { timeout 20 sv restart "$D" >/dev/null 2>&1 || true; }
write_pin() {
    backup=$D/conf.pre-golden-$(date +%Y%m%d-%H%M%S)
    cp -p "$D/conf" "$backup"
    tmp=$(mktemp "$D/.conf.XXXXXX")
    sed -e "s|^OPDBUS_FUSE_BINARY=.*|OPDBUS_FUSE_BINARY='$new'|" \
        -e "s|^OPDBUS_FUSE_BINARY_SHA256=.*|OPDBUS_FUSE_BINARY_SHA256='$newsha'|" "$D/conf" > "$tmp"
    chown 0:0 "$tmp"; chmod 600 "$tmp"; mv -f "$tmp" "$D/conf"
}

[ -f "$D/conf" ] || { echo "no conf in $D" >&2; exit 1; }
[ -f "$new" ] && [ -x "$new" ] && [ ! -L "$new" ] || { echo "new binary must be a regular executable: $new" >&2; exit 1; }
# Aggregate deployment recovery supplies the exact original conf and mount
# identity even if the failed new view is no longer mounted.
if [ "$apply" = --restore ]; then
    [ "$(id -u)" = 0 ] || exit 1
    saved=${5:?saved conf}; saved_identity=${6:?saved identity}
    [ -f "$saved" ] && [ ! -L "$saved" ] && [ -f "$saved_identity" ] || exit 1
    identity=$(jq -ce 'select(.kind=="host_session" or .kind=="guest_kernel")' "$saved_identity") || exit 1
    tmp=$(mktemp "$D/.conf.restore.XXXXXX")
    cp -p "$saved" "$tmp"; chown 0:0 "$tmp"; chmod 0600 "$tmp"
    mv -f "$tmp" "$D/conf"
    restart
    if wait_healthy "$new"; then log 'original conf restored and health verified'; exit 0; fi
    log 'original conf restored, but health is NOT verified'
    exit 2
fi
if [ "$apply" = --pin-down ]; then
    [ "$(id -u)" = 0 ] || { echo "--pin-down must run as root" >&2; exit 1; }
    [ -e "$D/down" ] || { log "not administratively down; refusing pin-only update"; exit 1; }
    ! mountpoint -q "/run/mounts/$view" || { log "view is mounted; refusing pin-only update"; exit 1; }
    newsha=$(sha256sum "$new" | cut -d' ' -f1)
    write_pin
    grep -qxF "OPDBUS_FUSE_BINARY='$new'" "$D/conf" && grep -qxF "OPDBUS_FUSE_BINARY_SHA256='$newsha'" "$D/conf" ||
        { log "conf has no OPDBUS_FUSE_BINARY/_SHA256 pin lines to rewrite"; exit 1; }
    log "pinned $new sha256=$newsha; service left down (down file kept); mount unverified"
    exit 0
fi
identity=$(timeout 10 jq -ce '.mount_identity | select(.kind == "host_session" or .kind == "guest_kernel")' \
    "/run/mounts/$view/.3tchedfs.json") || { log "cannot verify current mount identity"; exit 1; }
newsha=$(sha256sum "$new" | cut -d' ' -f1)
cur=$(sed -n "s/^OPDBUS_FUSE_BINARY='\{0,1\}\([^']*\)'\{0,1\}$/\1/p" "$D/conf")
log "current binary ${cur:-<adapter default>} sha256=$( [ -n "$cur" ] && sha256sum "$cur" | cut -d' ' -f1 )"
log "new binary $new sha256=$newsha"
log "health now: $(fuse_health && echo ok || echo FAIL)"
if [ "$apply" != --apply ] && [ "$apply" != --transaction ]; then
    log "dry-run: would rewrite conf pin, restart, health-check, roll back on failure"
    exit 0
fi
[ "$(id -u)" = 0 ] || { echo "--apply must run as root" >&2; exit 1; }
if [ "$cur" = "$new" ] && [ "/proc/$(svpid)/exe" -ef "$new" ] && fuse_health; then
    log "already healthy on the release binary"
    exit 0
fi
write_pin
restart
if wait_healthy "$new"; then
    log "HEALTHY pid=$(svpid) exe=$(readlink "/proc/$(svpid)/exe")"
    exit 0
fi
if [ "$apply" = --transaction ]; then
    log "health failed; complete transaction owner must restore catalog before previous FUSE"
    exit 1
fi
log "health failed; restoring $backup"
cp -p "$backup" "$D/conf"; chown 0:0 "$D/conf"; chmod 600 "$D/conf"
restart
if wait_healthy "$cur"; then log "ROLLED BACK, healthy on previous binary"; else log "ROLLED BACK but NOT healthy"; fi
exit 1
