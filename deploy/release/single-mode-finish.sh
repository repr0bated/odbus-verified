#!/bin/sh
# Prepare a single-user console and start a Cursor agent to find what still
# holds btrfs root 495 and finish the parked install. Run this from sulogin
# after the runlevel is single.
#
#   sudo runsvchdir single
#   # sulogin on the console, then:
#   /srv/git/odbus-verified/deploy/release/single-mode-finish.sh
#
# The default runlevel is not restored. The agent must not create a subvolume,
# unload the guard, turn the guard off, or use an ordinary subvolume delete.
set -eu
PATH=/usr/sbin:/usr/bin:/sbin:/bin
export PATH
umask 077

REPO=/srv/git/odbus-verified
EVIDENCE=/var/tmp/single-mode-finish
GUARD=/usr/local/libexec/opdbus-storage/guardctl
AGENT=/home/jeremy/.local/bin/cursor-agent
PROMPT=$EVIDENCE/prompt.txt

die() { printf '%s\n' "single-mode-finish: $*" >&2; exit 1; }

[ "$(id -u)" = 0 ] || die "run as root from the single-user console"
[ "$#" = 0 ] || die "takes no arguments"
runlevel=$(readlink -f /etc/runit/runsvdir/current 2>/dev/null || true)
[ "$runlevel" = /etc/runit/runsvdir/single ] ||
    die "runlevel is ${runlevel:-unknown}; from a console run: runsvchdir single"

[ -x "$GUARD" ] || die "missing guardctl"
[ -x "$AGENT" ] || die "missing cursor-agent"
[ -d "$REPO" ] || die "missing repository"

mkdir -m 0700 -p "$EVIDENCE"
: > "$EVIDENCE/prepare.log"
log() { printf '%s %s\n' "$(date -Is)" "$*" | tee -a "$EVIDENCE/prepare.log"; }

log "recording guard and subvolume evidence"
{
    printf 'date %s\n' "$(date -Is)"
    printf 'uname %s\n' "$(uname -r)"
    printf 'runlevel %s\n' "$runlevel"
} > "$EVIDENCE/identity.txt"
"$GUARD" status > "$EVIDENCE/guard-status.txt" 2>&1 || true
btrfs subvolume list / > "$EVIDENCE/subvolumes.txt" 2>&1 || true
findmnt -A -o TARGET,SOURCE,FSTYPE,OPTIONS > "$EVIDENCE/findmnt.txt" 2>&1 || true
awk '
    $0 ~ /subvolid=495/ || $0 ~ /subvolid=489/ || $0 ~ /subvolid=497/ || $0 ~ /@release-b/ || $0 ~ /release-640f40aa/ || $0 ~ /f7c28383/ {
        print
    }
' /proc/1/mountinfo > "$EVIDENCE/release-mounts.txt" 2>/dev/null || true
grep -h -E 'subvolid=495|release-640f40aa|@release-b|f7c28383' /proc/[0-9]*/mountinfo \
    > "$EVIDENCE/root-495-mounts.txt" 2>/dev/null || true
/usr/local/libexec/opdbus-release/op-release-launch status \
    > "$EVIDENCE/release-status.json" 2>&1 || true
ip -br addr > "$EVIDENCE/addresses.txt" 2>&1 || true
ip route show > "$EVIDENCE/routes.txt" 2>&1 || true
for cgroup in opdbus-storage opdbus-release opdbus-incus; do
    printf '== %s\n' "$cgroup" >> "$EVIDENCE/cgroup-procs.txt"
    cat "/sys/fs/cgroup/$cgroup/cgroup.procs" >> "$EVIDENCE/cgroup-procs.txt" 2>&1 || true
done

if ! ip route show default | grep -q .; then
    log "no default route; the agent API call will fail until a route exists"
fi

if ! pgrep -x sshd >/dev/null 2>&1; then
    log "starting sshd so this console is not the only way back"
    mkdir -p /run/sshd
    chmod 0755 /run/sshd
    /usr/sbin/sshd -t || die "sshd config test failed"
    /usr/sbin/sshd -E "$EVIDENCE/sshd.log"
fi

cat > "$PROMPT" <<'EOF'
The host is in single mode so you can find what still holds btrfs root 495.
Read /var/tmp/single-mode-finish/ and HANDOFF.md before changing anything.

Golden b0f8caf (UI 8b067b2, build 20261101T050906Z) was received as top-level
root 497, name release-f7c28383-9cc3-4081-929c-1047f8fecf1f. It is not
installed. The deploy journal is parked at quiescent-delete:495. Root 495 is
@release-b, the previous slot. Checked-delete returns EBUSY. The kernel guard
allowed the ioctl. In multi-user mode no mount namespace still showed the old
slot B overlays. Live slot A is root 489, release-532099cb-00e9-422e-8347-a071fe1abcc2.

Finish only this:
- Find the live reference that keeps root 495 busy. Record the holder in the evidence directory.
- Release that reference. Do not use an ordinary btrfs subvolume delete, and do not call the quiescent-destroy ioctl by hand.
- Then run op-release-launch release. That resumes the parked install of root 497 into slot B. Do not start a second deploy while the journal exists.
- If that install completes, run op-release-launch activate b. Leave root 489 in place.
- Do not turn the guard off, unload it, or add a trust binding.
- Do not run runsvchdir default. Leave the host in single mode and write what the operator must run to return.

Do not create a subvolume. Do not put an address on hackbr0. Do not delete root 489.
EOF

log "launching cursor-agent as jeremy"
cd "$REPO"
exec runuser -u jeremy -- \
    env HOME=/home/jeremy USER=jeremy LOGNAME=jeremy \
        TERM="${TERM:-linux}" \
        PATH="/home/jeremy/.local/bin:/usr/sbin:/usr/bin:/sbin:/bin" \
    "$AGENT" --force --sandbox disabled  \
    "$(cat "$PROMPT")"
