#!/bin/sh
# Finish the parked golden install once per boot, before application services
# can hold btrfs root 495.
#
#   install   early boot, from rcS, before the committed slot is mounted
#   activate  after runsvdir is up, switches the live slot to the received release
#
# The script never creates a subvolume, never uses an ordinary subvolume
# delete, never calls the quiescent-destroy ioctl itself, and never changes
# the kernel guard. A failure is recorded and boot continues.
set -eu
PATH=/usr/sbin:/usr/bin:/sbin:/bin
export PATH
umask 077

LAUNCH=/usr/local/libexec/opdbus-release/op-release-launch
EVIDENCE=/var/tmp/op-release-startup-finish
EXPECTED_ID=497
EXPECTED_NAME=release-f7c28383-9cc3-4081-929c-1047f8fecf1f

phase=${1:-}
[ "$phase" = install ] || [ "$phase" = activate ] || {
    printf '%s\n' 'usage: startup-finish.sh {install|activate}' >&2
    exit 2
}
[ "$(id -u)" = 0 ] || {
    printf '%s\n' 'startup-finish.sh: root required' >&2
    exit 1
}

mkdir -m 0700 -p "$EVIDENCE"
log() { printf '%s %s\n' "$(date -Is)" "$*" | tee -a "$EVIDENCE/log"; }

status_file() { "$LAUNCH" status > "$EVIDENCE/status.json" 2>"$EVIDENCE/status.err"; }

json_get() {
    jq -r "$1" "$EVIDENCE/status.json" 2>/dev/null || true
}

ensure_release_cgroup() {
    [ "$(stat -f -c %T /sys/fs/cgroup 2>/dev/null || true)" = cgroup2fs ] || return 1
    cgroup=/sys/fs/cgroup/opdbus-release
    if [ ! -e "$cgroup" ] && [ ! -L "$cgroup" ]; then
        mkdir -m 0700 "$cgroup"
    fi
    [ -d "$cgroup" ] && [ ! -L "$cgroup" ] || return 1
    chown 0:0 "$cgroup"
    chmod 0700 "$cgroup"
    for control in cgroup.procs cgroup.threads cgroup.subtree_control; do
        [ -f "$cgroup/$control" ] && [ ! -L "$cgroup/$control" ] || return 1
        chown 0:0 "$cgroup/$control"
        chmod 0600 "$cgroup/$control"
    done
}

ensure_shm() {
    if [ ! -d /dev/shm/opdbus ]; then
        mkdir -m 0755 /dev/shm/opdbus
    fi
    chown 0:0 /dev/shm/opdbus
    chmod 0755 /dev/shm/opdbus
}

# hackbr0 was a one-off hackathon NAT bridge. Clearing its address was not
# enough: Incus still recreates the interface on every boot while the managed
# network object exists. Delete the network (and unused ACL) if it returns.
remove_hackbr0() {
    command -v incus >/dev/null 2>&1 || return 0
    incus info >/dev/null 2>&1 || return 0
    if incus network show hackbr0 >/dev/null 2>&1; then
        log "hackbr0 present; deleting managed network"
        incus network delete hackbr0
    fi
    if incus network acl show internet-only >/dev/null 2>&1; then
        log "internet-only ACL present; deleting"
        incus network acl delete internet-only || true
    fi
}

run_launch() {
    seconds=$1
    shift
    timeout -k 20 "$seconds" "$LAUNCH" "$@" > "$EVIDENCE/${phase}.out" 2>&1
}

if ! ensure_release_cgroup; then
    log "${phase}: release cgroup unavailable; nothing changed"
    exit 0
fi
if ! status_file; then
    log "${phase}: release status failed; nothing changed"
    exit 0
fi

operation=$(json_get '.state.journal.operation // ""')
step=$(json_get '.state.journal.step // ""')
active=$(json_get '.state.active // ""')
slot_b_id=$(json_get '.state.slots.b.id // ""')
slot_b_name=$(json_get '.state.slots.b.name // ""')

case "$phase" in
    install)
        if [ "$operation" != deploy ] || [ "$step" != "quiescent-delete:${EXPECTED_ID}" ]; then
            log "install: no parked delete of root ${EXPECTED_ID} (operation=${operation:-none} step=${step:-none})"
            exit 0
        fi
        ensure_shm
        log "install: resuming parked delete of root ${EXPECTED_ID}"
        set +e
        run_launch 180 release
        launch_rc=$?
        set -e
        status_file || true
        operation=$(json_get '.state.journal.operation // ""')
        slot_b_id=$(json_get '.state.slots.b.id // ""')
        slot_b_name=$(json_get '.state.slots.b.name // ""')
        if [ -z "$operation" ] && [ "$slot_b_id" = "$EXPECTED_ID" ] && [ "$slot_b_name" = "$EXPECTED_NAME" ]; then
            log "install: slot B is ${EXPECTED_NAME}; activate follows after services start"
            printf 'installed %s\n' "$(date -Is)" > "$EVIDENCE/installed"
        else
            log "install: root ${EXPECTED_ID} still blocked (launcher ${launch_rc}, journal ${operation:-none}, slot B ${slot_b_id:-none})"
        fi
        ;;
    activate)
        if [ -n "$operation" ]; then
            log "activate: journal ${operation} ${step} is still open; not switching slots"
            exit 0
        fi
        if [ "$active" = b ] && [ "$slot_b_name" = "$EXPECTED_NAME" ]; then
            log "activate: ${EXPECTED_NAME} is already live"
        else
            if [ "$slot_b_id" != "$EXPECTED_ID" ] || [ "$slot_b_name" != "$EXPECTED_NAME" ]; then
                log "activate: slot B is ${slot_b_id:-none} ${slot_b_name:-none}; waiting for the early install"
            else
                ensure_shm
                log "activate: switching the live slot to ${EXPECTED_NAME}"
                set +e
                run_launch 240 activate b
                launch_rc=$?
                set -e
                status_file || true
                active=$(json_get '.state.active // ""')
                slot_b_name=$(json_get '.state.slots.b.name // ""')
                if [ "$launch_rc" -eq 0 ] && [ "$active" = b ] && [ "$slot_b_name" = "$EXPECTED_NAME" ]; then
                    log "activate: live slot is ${EXPECTED_NAME}"
                    printf 'active %s %s\n' "$EXPECTED_NAME" "$(date -Is)" > "$EVIDENCE/receipt"
                else
                    log "activate: switch failed (launcher ${launch_rc}, active ${active:-none})"
                fi
            fi
        fi
        if command -v incus >/dev/null 2>&1; then
            i=0
            while [ "$i" -lt 30 ]; do
                if incus info >/dev/null 2>&1; then
                    break
                fi
                i=$((i + 1))
                sleep 1
            done
            remove_hackbr0 || log "activate: hackbr0 removal failed"
        fi
        ;;
esac
exit 0
