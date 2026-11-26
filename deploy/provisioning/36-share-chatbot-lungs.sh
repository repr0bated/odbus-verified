#!/usr/bin/env bash
# Operator maintenance: transfer listener creation to the configured chatbot.
# The host runtime must already contain runtime_lungs_shared before applying.
set -euo pipefail
fail() { printf 'shared-lungs: %s\n' "$*" >&2; exit 1; }
[[ ${1:-} == --apply && $# == 1 ]] || {
    printf '%s\n' 'Usage: sudo 36-share-chatbot-lungs.sh --apply' \
        'Installs the chatbot socket unit, restarts the chatbot and host bridge, and retains rollback settings.'
    exit 2
}
[[ $(id -u) == 0 ]] || fail 'operator maintenance requires root'
here=$(cd -- "$(dirname -- "$0")" && pwd)
name=3tchedChatbot
unit=opdbus-shared-lungs.socket
asset=$here/../seed/rootfs/etc/systemd/system/$unit
owner_file=/etc/op-dbus/env/OPDBUS_LUNGS_OWNER
config=$(incus query "/1.0/instances/$name")
key=$(jq -er '.config["user.opdbus.wireguard_public_key"]' <<<"$config")
read -r session principal < <(/usr/local/bin/op-identity-accept derive --pubkey "$key")
expected=$(cat /etc/op-dbus/env/op-chatbot-wireguard-pubkey)
[[ $key == "$expected" && $(jq -r '.config["user.opdbus.session_id"]' <<<"$config") == "$session" ]] ||
    fail 'native chatbot identity differs from the configured service key'
jq -e '.devices.lungs | .type=="disk" and .source=="/run/opdbus/lungs" and .path=="/run/mounts/opdbus/lungs"' \
    <<<"$config" >/dev/null || fail 'chatbot lacks the exact shared directory device'
[[ $(incus list -f csv -c s "^$name$") == RUNNING ]] || fail 'chatbot must initially be running'
[[ -f $asset ]] || fail 'socket unit asset missing'
if [[ -e $owner_file || -L $owner_file ]]; then
    [[ -f $owner_file && ! -L $owner_file && $(stat -c %u:%g:%a "$owner_file") == 0:0:600 ]] ||
        fail 'unsafe listener owner setting'
    [[ $(cat "$owner_file") != chatbot ]] || fail 'chatbot ownership is already configured; inspect it instead of repeating cutover'
fi
backup=$(mktemp -d /etc/op-dbus/shared-lungs-backup.XXXXXX)
printf '%s\n' "$config" >"$backup/chatbot.json"
printf '%s\n' "$backup" >"$backup/location"
[[ ! -e $owner_file ]] || cp -a "$owner_file" "$backup/owner"
readonly_before=$(jq -r '.devices.lungs.readonly // "false"' <<<"$config")
autostart_before=$(jq -r '.config["boot.autostart"] // ""' <<<"$config")
# Install within the guest namespace so a guest-controlled symlink can never
# redirect a privileged host write out of its own rootfs.
incus exec "$name" -- sh -ec '
    unit=opdbus-shared-lungs.socket
    dir=/etc/systemd/system
    test ! -L "$dir/$unit"
    if test -e "$dir/$unit"; then cp -a "$dir/$unit" "$dir/$unit.before-shared-lungs"; fi
    umask 022
    temp=$(mktemp "$dir/.$unit.XXXXXX")
    cat >"$temp"
    chmod 0644 "$temp"
    mv -T "$temp" "$dir/$unit"
    mkdir -p "$dir/sockets.target.wants"
    link="$dir/sockets.target.wants/$unit"
    if test -L "$link"; then
        test "$(readlink "$link")" = "$dir/$unit"
    else
        test ! -e "$link"
        ln -s "$dir/$unit" "$link"
    fi
' <"$asset"
rollback() {
    trap - EXIT
    printf '%s\n' 'Restoring host-owned listener'
    if [[ $(incus list -f csv -c s "^$name$") == RUNNING ]]; then
        incus stop "$name" --timeout 30 || return 1
    fi
    incus config device set "$name" lungs readonly="$readonly_before" || return 1
    if [[ -n $autostart_before ]]; then incus config set "$name" boot.autostart="$autostart_before"
    else incus config unset "$name" boot.autostart; fi
    if [[ -f $backup/owner ]]; then
        install -m 0600 "$backup/owner" "$owner_file"
    else
        # Keep one explicit source of truth and never leave chatbot mode selected.
        printf '%s\n' local >"$owner_file"
        chmod 0600 "$owner_file"
    fi
    sv -w 35 up op-grpc-bridge || return 1
    incus start "$name" || return 1
    incus exec "$name" -- sh -ec '
        unit=opdbus-shared-lungs.socket
        link=/etc/systemd/system/sockets.target.wants/$unit
        if test -L "$link"; then unlink "$link"; fi
        busctl --system call org.freedesktop.systemd1 /org/freedesktop/systemd1 \
            org.freedesktop.systemd1.Manager StopUnit ss "$unit" replace
    ' || true
    printf 'Rollback settings retained in %s\n' "$backup"
}
trap 'rc=$?; if ((rc)); then rollback || true; fi; exit "$rc"' EXIT
# Stop the chatbot while the original host bridge is still available.
incus stop "$name" --timeout 30
sv -w 35 down op-grpc-bridge
incus config device set "$name" lungs readonly=false
incus config set "$name" boot.autostart=true
[[ -d /etc/op-dbus/env && ! -L /etc/op-dbus/env ]] || fail 'unsafe settings directory'
temp=$(mktemp /etc/op-dbus/env/.lungs-owner.XXXXXX)
printf '%s\n' chatbot >"$temp"
chmod 0600 "$temp"
mv -T "$temp" "$owner_file"
incus start "$name"
sv -w 90 up op-grpc-bridge
sv -w 90 check op-grpc-bridge
[[ -S /run/opdbus/lungs/lungs.sock ]] || fail 'shared listener absent after startup'
printf 'CHATBOT_SHARED_LUNGS_READY session=%s principal=%s backup=%s\n' "$session" "$principal" "$backup"
trap - EXIT
