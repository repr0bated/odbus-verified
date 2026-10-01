#!/bin/sh
# Bounded live check for only i0/i1. Runs writes under unique temporary names,
# then cleans them. Lifecycle recovery trap restarts only the one test candidate.
set -eu

ROOT=/srv/git/odbus-verified
SOURCE=$ROOT/deploy/runit/libexec-3tched/op-identity-root
STATE=/var/lib/opdbus-runtime/seed-identities
REPORT=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
TOKEN=$(date -u +%Y%m%dT%H%M%SZ)-$$
LOG=$REPORT/validation-final-v7-repair1.log
EXPECTED_SEED=2dcac52c-fb0b-418a-95a4-cb6eacc80209
EXPECTED_SEED_PARTUUID=260b98c7-b4f8-4c77-9fe7-67173c08dc90
EXPECTED_SOURCE_HASH=ce71c46c0c3be3d5ca241252295f59af8a4312d26f9f997c5df4c3427a4d9f4d
EXPECTED_BINARY=bef3929ffd0647e4b3549d3fd8f3a3e9787e10351ba98d75c2bf9703b45abede
EXPECTED_AGENTS=feed8ca7b302eb44a9f270b59ae884cb3c662b148447c94c520b7a0047c40a48
SOURCE_HASH=$(sha256sum "$SOURCE" | awk '{print $1}')
SEED_DEVICE=$(readlink -f "/dev/disk/by-partuuid/$EXPECTED_SEED_PARTUUID")
RECOVERY_CANDIDATE=

exec >"$LOG" 2>&1
printf 'identity physical seed validation UTC=%s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
printf 'source_helper_sha256=%s path=%s\n' "$SOURCE_HASH" "$SOURCE"
printf 'test_token=%s\n' "$TOKEN"
printf 'seed_device=%s partuuid=%s\n' "$SEED_DEVICE" "$EXPECTED_SEED_PARTUUID"

fail() { printf 'FAIL: %s\n' "$*" >&2; exit 1; }
root() { sudo -n "$@"; }
source_unchanged() {
    current=$(sha256sum "$SOURCE" | awk '{print $1}')
    [ "$current" = "$SOURCE_HASH" ] && [ "$current" = "$EXPECTED_SOURCE_HASH" ] || fail 'source helper changed during validation or did not match frozen v7'
}
status_of() {
    root incus query "/1.0/instances/$1/state" | jq -er '.status'
}
expect_running() {
    actual=$(status_of "$1")
    [ "$actual" = Running ] || fail "$1 expected Running, got $actual"
}
expect_stopped() {
    actual=$(status_of "$1")
    [ "$actual" = Stopped ] || fail "$1 expected Stopped, got $actual"
}

recover() {
    rc=$?
    trap - EXIT HUP INT TERM
    if [ -n "$RECOVERY_CANDIDATE" ]; then
        printf 'RECOVERY: trying source seed-up/start for %s\n' "$RECOVERY_CANDIDATE" >&2
        if source_unchanged; then
            root sh "$SOURCE" seed-up "$RECOVERY_CANDIDATE" || true
            root incus start "$RECOVERY_CANDIDATE" || true
        else
            printf 'RECOVERY BLOCKED: helper hash changed; candidate %s may remain stopped\n' "$RECOVERY_CANDIDATE" >&2
        fi
    fi
    for candidate in i0 i1; do
        if [ "$(status_of "$candidate" 2>/dev/null || true)" = Running ]; then
            guest_action "$candidate" cleanup || true
        fi
    done
    exit "$rc"
}
trap recover EXIT HUP INT TERM

guest_action() {
    candidate=$1 action=$2 other=${3:-}
    root incus exec "$candidate" -- sh -c '
        action=$1 token=$2 name=$3 other=$4
        dirs="/agent/user /agent/memory /agent/skills /opt/run-mounts/persist"
        case "$action" in
        protected)
            for dir in /agent/soul /agent/rules /agent/shared; do
                if mountpoint -q "$dir"; then
                    mount_opts=$(findmnt -rn -M "$dir" -o VFS-OPTIONS 2>/dev/null || true)
                    case ",$mount_opts," in *,ro,*) echo "protected_guest_mount=$dir ro=true" ;; *) echo "FAIL protected guest mount is not read-only: $dir ($mount_opts)"; exit 1 ;; esac
                else
                    prop=$(btrfs property get -ts "$dir" ro 2>/dev/null || true)
                    [ "$prop" = ro=true ] || { echo "FAIL unmounted guest subvolume is not readonly: $dir ($prop)"; exit 1; }
                    echo "protected_guest_mount=$dir unmounted_subvolume_ro=true"
                fi
                file="$dir/.physical-seed-ro-check-$token-$name"
                test ! -e "$file" || { echo "FAIL protected test file pre-exists"; exit 1; }
                if (umask 077; set -C; : > "$file") 2>/dev/null; then
                    rm -f -- "$file"
                    echo "FAIL protected write allowed $dir"
                    exit 1
                fi
                echo "protected_write=$dir denied"
            done
            ;;
        ro-inspect)
            btrfs property get -ts /agent/rules ro
            ;;
        create)
            for dir in $dirs; do
                file="$dir/.physical-seed-validation-$token-$name"
                test ! -e "$file" || { echo "FAIL writable test file pre-exists"; exit 1; }
                printf "private:%s:%s\n" "$name" "$token" > "$file"
                chmod 600 "$file"
                test "$(cat "$file")" = "private:$name:$token" || exit 1
                echo "writable=$dir created"
            done
            ;;
        persisted)
            for dir in $dirs; do
                file="$dir/.physical-seed-validation-$token-$name"
                test "$(cat "$file")" = "private:$name:$token" || { echo "FAIL persistence $dir"; exit 1; }
                echo "persisted=$dir verified"
            done
            ;;
        absent)
            for dir in $dirs; do
                file="$dir/.physical-seed-validation-$token-$other"
                test ! -e "$file" || { echo "FAIL cross-candidate isolation $dir"; exit 1; }
            done
            echo "cross_candidate_absence=$other verified"
            ;;
        cleanup)
            for dir in $dirs; do rm -f -- "$dir/.physical-seed-validation-$token-$name"; done
            echo "temporary_sentinels=$name removed"
            ;;
        esac
    ' sh "$action" "$TOKEN" "$candidate" "$other"
}

check_host_protected_views() {
    candidate=$1
    native_root=$(root jq -er '.root' "$STATE/$candidate/state.json")
    for member in soul rules shared; do
        source="$STATE/$candidate/volume/guest/agent/$member"
        target="$native_root/agent/$member"
        opts=$(root findmnt -rn -M "$target" -o FSTYPE,VFS-OPTIONS) || fail "$candidate missing host protected view $member"
        case "$opts" in btrfs*ro*) ;; *) fail "$candidate host protected view $member not btrfs+ro: $opts" ;; esac
        source_id=$(root stat -c '%d:%i' "$source")
        target_id=$(root stat -c '%d:%i' "$target")
        [ "$source_id" = "$target_id" ] || fail "$candidate host protected view $member is not the expected subvolume"
        printf 'host_protected_view=%s/agent/%s fstype=btrfs ro=true source_inode=match\n' "$candidate" "$member"
    done
}

check_hashes() {
    candidate=$1
    actual=$(root incus exec "$candidate" -- sha256sum \
        /usr/local/bin/threetched-fs \
        /agent/rules/AGENTS.md \
        /agent/shared/skills/mutation-dispatch/SKILL.md \
        /agent/shared/skills/ovs-network-triage/SKILL.md \
        /agent/shared/skills/plugin-schema/SKILL.md \
        /agent/shared/skills/qdrant-collections/SKILL.md)
    printf '%s\n' "$actual" | while read -r got path; do
        case "$path" in
            /usr/local/bin/threetched-fs) expected=$EXPECTED_BINARY ;;
            /agent/rules/AGENTS.md) expected=$EXPECTED_AGENTS ;;
            /agent/shared/skills/mutation-dispatch/SKILL.md) expected=8931dc98443f513fbf2228c58d26d92e63f4608e452612d92074c5ee10f5535e ;;
            /agent/shared/skills/ovs-network-triage/SKILL.md) expected=c29d3a77c126eda83b7e141d366401aa49cb2e3a1def13d7c98f63f2870cc3fb ;;
            /agent/shared/skills/plugin-schema/SKILL.md) expected=a05380b89278c9f4a5ed2a3c48fcd0df38151e573d05644293dd3a886a8725b5 ;;
            /agent/shared/skills/qdrant-collections/SKILL.md) expected=28572c1d31a456b775bec4e4d594c7cbd95b8233bfee04b94d831aca94e68189 ;;
            *) echo "FAIL unexpected hash path $path" >&2; exit 1 ;;
        esac
        [ "$got" = "$expected" ] || { echo "FAIL hash mismatch $candidate $path" >&2; exit 1; }
        printf 'hash_match=%s %s\n' "$candidate" "$path"
    done
}

check_network() {
    candidate=$1
    instance=$(root incus query "/1.0/instances/$candidate?recursion=1")
    printf '%s\n' "$instance" | jq -c '{name,status,device_names:(.expanded_devices|keys),nic_count:([.expanded_devices[]|select(.type=="nic")]|length),proxy_count:([.expanded_devices[]|select(.type=="proxy")]|length)}'
    printf '%s\n' "$instance" | jq -e '([.expanded_devices[]|select(.type=="nic")]|length)==0 and ([.expanded_devices[]|select(.type=="proxy")]|length)==0' >/dev/null || fail "$candidate Incus NIC/proxy found"
    for dev in gre0 gretap0 erspan0; do
        link=$(root incus exec "$candidate" -- ip -d -j link show dev "$dev" 2>/dev/null || printf '[]')
        summary=$(printf '%s\n' "$link" | jq -c --arg dev "$dev" 'if length==0 then {device:$dev,present:false} else .[0]|{device:.ifname,kind:(.linkinfo.info_kind//"unknown"),state:.operstate,flags:.flags} end')
        printf '%s\n' "$summary"
        printf '%s\n' "$summary" | jq -e '.present == false or .state == "DOWN"' >/dev/null || fail "$candidate fallback tunnel $dev is not down"
        count=$(root incus exec "$candidate" -- ip -j addr show dev "$dev" | jq '[.[].addr_info[]?] | length')
        [ "$count" = 0 ] || fail "$candidate fallback tunnel $dev has addresses"
        printf 'interface_address_count=%s %s\n' "$dev" "$count"
    done
    routes=$(root incus exec "$candidate" -- sh -c 'for fam in 4 6; do ip -"$fam" -o route show table all | awk -v fam="$fam" '\''{dev=""; for(i=1;i<=NF;i++) if($i=="dev") dev=$(i+1); if($1=="default") def++; else if(dev=="lo") loop++; else other++} END {printf "ipv%s default=%d loopback_only=%d nonloopback=%d\n",fam,def,loop,other}'\''; done')
    printf '%s\n' "$routes"
    printf '%s\n' "$routes" | awk '$2 != "default=0" || $4 != "nonloopback=0" {bad=1} END {exit bad}' || fail "$candidate has a default or non-loopback route"
}

source_unchanged
[ -b "$SEED_DEVICE" ] || fail "expected physical seed device is unavailable: $SEED_DEVICE"
root btrfs inspect-internal dump-super "$SEED_DEVICE" | grep -q SEEDING || fail 'physical seed is not flagged SEEDING'
printf 'seed_device=%s seed_fsid=%s seeding=confirmed\n' "$SEED_DEVICE" "$EXPECTED_SEED"

for candidate in i0 i1; do
    expect_running "$candidate"
    seed_fsid=$(root jq -er '.seed_fsid' "$STATE/$candidate/state.json")
    leaf_size=$(root jq -er '.leaf_size' "$STATE/$candidate/state.json")
    [ "$seed_fsid" = "$EXPECTED_SEED" ] || fail "$candidate seed FSID mismatch"
    [ "$leaf_size" = 8589934592 ] || fail "$candidate leaf is not 8 GiB"
    printf 'candidate=%s initial_status=Running seed_fsid=%s leaf_size_bytes=%s\n' "$candidate" "$seed_fsid" "$leaf_size"
    root jq '{phase,seed_loop,leaf_loop}' "$STATE/$candidate/state.json"
    guest_action "$candidate" ro-inspect ''
    root btrfs property get -ts "$(root jq -er '.root' "$STATE/$candidate/state.json")/agent/rules" ro | grep -q 'ro=true' || fail "$candidate host rules subvolume lost readonly property"
    printf 'host_btrfs_property=%s /agent/rules ro=true\n' "$candidate"
    check_host_protected_views "$candidate"
    check_hashes "$candidate"
    check_network "$candidate"
    guest_action "$candidate" protected ''
done

# i0 first: create private write markers, verify i1 cannot see them, then close
# and reopen the same leaf while i1 remains Running.
guest_action i0 create ''
guest_action i1 absent i0
expect_running i1
root incus stop i0
a=0
while [ "$a" -lt 60 ] && [ "$(status_of i0)" != Stopped ]; do sleep 1; a=$((a + 1)); done
expect_stopped i0
RECOVERY_CANDIDATE=i0
expect_running i1
source_unchanged
    root sh "$SOURCE" seed-down i0
expect_running i1
expect_stopped i0
root jq -e '.phase=="ready" and .seed_loop=="" and .leaf_loop==""' "$STATE/i0/state.json" >/dev/null
if root mountpoint -q "$(root jq -er '.root' "$STATE/i0/state.json")"; then fail 'i0 guest root stayed mounted after seed-down'; fi
source_unchanged
    root sh "$SOURCE" seed-up i0
root incus start i0
RECOVERY_CANDIDATE=
expect_running i0
expect_running i1
guest_action i0 persisted ''
guest_action i1 absent i0
check_hashes i0
check_network i0
printf 'candidate=i0 seed-down-up-persistence=PASS other_candidate=i1-running=PASS\n'

# i1 second: reciprocal isolation and persistence while i0 stays Running.
guest_action i1 create ''
guest_action i0 absent i1
expect_running i0
root incus stop i1
a=0
while [ "$a" -lt 60 ] && [ "$(status_of i1)" != Stopped ]; do sleep 1; a=$((a + 1)); done
expect_stopped i1
RECOVERY_CANDIDATE=i1
expect_running i0
source_unchanged
    root sh "$SOURCE" seed-down i1
expect_running i0
expect_stopped i1
root jq -e '.phase=="ready" and .seed_loop=="" and .leaf_loop==""' "$STATE/i1/state.json" >/dev/null
if root mountpoint -q "$(root jq -er '.root' "$STATE/i1/state.json")"; then fail 'i1 guest root stayed mounted after seed-down'; fi
source_unchanged
    root sh "$SOURCE" seed-up i1
root incus start i1
RECOVERY_CANDIDATE=
expect_running i0
expect_running i1
guest_action i1 persisted ''
guest_action i0 absent i1
check_hashes i1
check_network i1
printf 'candidate=i1 seed-down-up-persistence=PASS other_candidate=i0-running=PASS\n'

guest_action i0 cleanup ''
guest_action i1 cleanup ''
expect_running i0
expect_running i1
source_unchanged
printf 'FINAL i0=Running i1=Running sentinels=removed source_helper_unchanged=PASS\n'
